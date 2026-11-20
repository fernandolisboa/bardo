//! A small client for remote Model Context Protocol servers over Streamable
//! HTTP (ADR-0028): enough to call a server's tools with an OAuth access
//! token. Blocking, on the same transport seam as every adapter, so it is
//! tested against recorded answers.
//!
//! A session starts with `initialize` and the `notifications/initialized`
//! notice; the server may name it in an `Mcp-Session-Id` header, which
//! every later request carries. Answers come as plain JSON or as a
//! server-sent event stream holding the JSON-RPC response. A 404 to a
//! request that carried a session id means the server dropped the
//! session: the client starts a new one and sends the request again,
//! which is safe because the server did not take it. Nothing else is ever
//! sent twice: a tool call that starts paid work must not be repeated
//! after an answer that may have been lost.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::http::{HttpRequest, HttpResponse, Transport};

/// The protocol revision Bardo speaks. A server may answer with another
/// it supports; later requests then carry that one.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Why a call gave no result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpError {
    /// The server refused the access token (HTTP 401).
    #[error("the server refused the access token")]
    Unauthorized,
    /// The token may not do this (HTTP 403).
    #[error("the server forbids this: {0}")]
    Forbidden(String),
    /// No answer: offline, DNS, TLS, proxy or timeout.
    #[error("no answer: {0}")]
    Unreachable(String),
    /// Too many calls (HTTP 429).
    #[error("too many calls")]
    RateLimited,
    /// The server is failing on its side (HTTP 5xx).
    #[error("the server is failing (HTTP {0})")]
    ServerDown(u16),
    /// The tool ran and reported a failure, in its own words.
    #[error("{0}")]
    Tool(String),
    /// A JSON-RPC error, or an answer the client does not understand.
    #[error("{0}")]
    Protocol(String),
}

struct Session {
    /// SHA-256 of the access token the session was opened with: a new
    /// token (after a refresh or another sign-in) opens a new session.
    token: [u8; 32],
    id: Option<String>,
    version: String,
}

/// Calls the tools of one MCP server.
pub struct McpClient<T> {
    url: String,
    transport: T,
    session: Mutex<Option<Session>>,
    next_id: AtomicU64,
}

impl<T: Transport> McpClient<T> {
    pub fn new(url: impl Into<String>, transport: T) -> Self {
        Self {
            url: url.into(),
            transport,
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Calls tool `name` and gives its result: the structured content
    /// when the server sends it, else the first text block read as JSON
    /// (or as a JSON string when it is not JSON).
    pub fn call_tool(&self, token: &str, name: &str, arguments: Value) -> Result<Value, McpError> {
        let params = json!({ "name": name, "arguments": arguments });
        let result = self.request(token, "tools/call", params)?;
        tool_result(&result)
    }

    /// Sends one request in the current session, opening one first when
    /// there is none for this token.
    fn request(&self, token: &str, method: &str, params: Value) -> Result<Value, McpError> {
        let fingerprint: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        for attempt in 0..2 {
            let (id, version) = self.open(token, fingerprint)?;
            let rpc = self.next_id.fetch_add(1, Ordering::Relaxed);
            let body = json!({ "jsonrpc": "2.0", "id": rpc, "method": method, "params": params });
            let response = match self.post(token, id.as_deref(), Some(&version), &body) {
                Err(McpError::Unauthorized) => {
                    // The next call with a renewed token opens a new session.
                    self.forget(fingerprint);
                    return Err(McpError::Unauthorized);
                }
                other => other?,
            };
            if response.status == 404 && id.is_some() && attempt == 0 {
                // The server forgot the session; it did not take the call.
                self.forget(fingerprint);
                continue;
            }
            return answer(&response, rpc);
        }
        Err(McpError::Protocol(
            "the server keeps dropping the session".into(),
        ))
    }

    /// The session for this token: kept, or opened now.
    fn open(
        &self,
        token: &str,
        fingerprint: [u8; 32],
    ) -> Result<(Option<String>, String), McpError> {
        let mut session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(open) = session.as_ref().filter(|open| open.token == fingerprint) {
            return Ok((open.id.clone(), open.version.clone()));
        }
        let rpc = self.next_id.fetch_add(1, Ordering::Relaxed);
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": rpc,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "Bardo", "version": env!("CARGO_PKG_VERSION") },
            },
        });
        let response = self.post(token, None, None, &initialize)?;
        let result = answer(&response, rpc)?;
        let id = response
            .header("mcp-session-id")
            .map(str::trim)
            .filter(|id| is_session_id(id))
            .map(str::to_owned);
        let version = result["protocolVersion"]
            .as_str()
            .filter(|version| is_version(version))
            .unwrap_or(PROTOCOL_VERSION)
            .to_owned();
        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let notice = self.post(token, id.as_deref(), Some(&version), &initialized)?;
        if !(200..=299).contains(&notice.status) {
            return Err(status_error(&notice));
        }
        *session = Some(Session {
            token: fingerprint,
            id: id.clone(),
            version: version.clone(),
        });
        Ok((id, version))
    }

    fn forget(&self, fingerprint: [u8; 32]) {
        let mut session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if session
            .as_ref()
            .is_some_and(|open| open.token == fingerprint)
        {
            *session = None;
        }
    }

    fn post(
        &self,
        token: &str,
        session: Option<&str>,
        version: Option<&str>,
        body: &Value,
    ) -> Result<HttpResponse, McpError> {
        let mut request = HttpRequest::post_json(self.url.clone(), body.to_string())
            .header("authorization", format!("Bearer {token}"))
            .header("accept", "application/json, text/event-stream");
        if let Some(session) = session {
            request = request.header("mcp-session-id", session.to_owned());
        }
        if let Some(version) = version {
            request = request.header("mcp-protocol-version", version.to_owned());
        }
        let response = self
            .transport
            .send(&request)
            .map_err(|error| McpError::Unreachable(error.0))?;
        match response.status {
            401 => Err(McpError::Unauthorized),
            _ => Ok(response),
        }
    }
}

/// Session ids are visible ASCII (the specification's rule), kept short.
fn is_session_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 512 && id.chars().all(|c| c.is_ascii_graphic())
}

/// Protocol versions are dates, `YYYY-MM-DD`.
fn is_version(version: &str) -> bool {
    version.len() == 10 && version.chars().all(|c| c.is_ascii_digit() || c == '-')
}

fn status_error(response: &HttpResponse) -> McpError {
    match response.status {
        403 => McpError::Forbidden(short(&response.body)),
        429 => McpError::RateLimited,
        status @ 500..=599 => McpError::ServerDown(status),
        status => McpError::Protocol(format!(
            "unexpected answer (HTTP {status}): {}",
            short(&response.body)
        )),
    }
}

/// The first few hundred characters of a body, for a log line.
fn short(text: &str) -> String {
    text.trim().chars().take(300).collect()
}

/// The JSON-RPC answer to request `rpc`: from a JSON body or from the
/// events of a stream.
fn answer(response: &HttpResponse, rpc: u64) -> Result<Value, McpError> {
    if !(200..=299).contains(&response.status) {
        return Err(status_error(response));
    }
    let streamed = response
        .header("content-type")
        .is_some_and(|kind| kind.to_ascii_lowercase().contains("text/event-stream"));
    let messages: Vec<Value> = if streamed {
        events(&response.body)
            .filter_map(|data| serde_json::from_str(&data).ok())
            .collect()
    } else {
        match serde_json::from_str::<Value>(&response.body) {
            Ok(Value::Array(batch)) => batch,
            Ok(message) => vec![message],
            Err(error) => {
                return Err(McpError::Protocol(format!("unreadable answer: {error}")));
            }
        }
    };
    // Each request gets its own answer, so the one response in it is the
    // answer even when a server numbers it differently.
    let mut responses: Vec<Value> = messages
        .into_iter()
        .filter(|message| message.get("result").is_some() || message.get("error").is_some())
        .collect();
    let index = responses
        .iter()
        .position(|message| message["id"].as_u64() == Some(rpc))
        .or_else(|| (responses.len() == 1).then_some(0))
        .ok_or_else(|| McpError::Protocol("the answer does not answer the request".into()))?;
    let message = responses.swap_remove(index);
    if let Some(error) = message.get("error") {
        let text = error["message"].as_str().unwrap_or("JSON-RPC error");
        return Err(McpError::Protocol(format!(
            "{} ({})",
            short(text),
            error["code"].as_i64().unwrap_or_default()
        )));
    }
    message
        .get("result")
        .cloned()
        .ok_or_else(|| McpError::Protocol("the answer has no result".into()))
}

/// The data of each event in a server-sent event stream: `data:` lines
/// joined by line breaks, events ended by a blank line.
fn events(body: &str) -> impl Iterator<Item = String> + '_ {
    body.replace("\r\n", "\n")
        .split("\n\n")
        .map(|event| {
            event
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|data| data.strip_prefix(' ').unwrap_or(data))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|data| !data.is_empty())
        .collect::<Vec<_>>()
        .into_iter()
}

/// What a `tools/call` result says.
fn tool_result(result: &Value) -> Result<Value, McpError> {
    let text = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"].as_str() == Some("text"))
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if result["isError"].as_bool() == Some(true) {
        return Err(McpError::Tool(if text.trim().is_empty() {
            "the tool failed without saying why".into()
        } else {
            short(&text)
        }));
    }
    if let Some(structured) = result.get("structuredContent").filter(|v| v.is_object()) {
        return Ok(structured.clone());
    }
    Ok(serde_json::from_str(text.trim()).unwrap_or(Value::String(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_join_data_lines_and_skip_others() {
        let body = "event: message\r\ndata: {\"a\":\r\ndata: 1}\r\n\r\n: comment\n\ndata: x\n\n";
        let found: Vec<String> = events(body).collect();
        assert_eq!(found, ["{\"a\":\n1}", "x"]);
    }

    #[test]
    fn a_tool_result_prefers_structured_content_then_json_text() {
        let structured = json!({
            "content": [{ "type": "text", "text": "{\"credits\":1}" }],
            "structuredContent": { "credits": 2 },
        });
        assert_eq!(tool_result(&structured).unwrap(), json!({ "credits": 2 }));
        let text = json!({ "content": [{ "type": "text", "text": " {\"credits\":1} " }] });
        assert_eq!(tool_result(&text).unwrap(), json!({ "credits": 1 }));
        let plain = json!({ "content": [{ "type": "text", "text": "done" }] });
        assert_eq!(tool_result(&plain).unwrap(), json!("done"));
        let failed = json!({ "isError": true, "content": [{ "type": "text", "text": "no" }] });
        assert_eq!(tool_result(&failed), Err(McpError::Tool("no".into())));
    }

    #[test]
    fn session_ids_and_versions_are_checked_before_they_go_in_a_header() {
        assert!(is_session_id("abc-123"));
        assert!(!is_session_id("a b"));
        assert!(!is_session_id(""));
        assert!(is_version("2025-06-18"));
        assert!(!is_version("2025-06-18\r\nx: y"));
    }
}
