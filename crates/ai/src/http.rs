//! The HTTP seam every provider adapter goes through, so adapters can be
//! tested against recorded responses instead of live calls (ADR-0001).

use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
}

/// A request. Header values may hold keys and bodies may hold user content,
/// so `Debug` shows only the header names and the body size.
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    /// Names are fixed by adapters, or given by a provider (presigned
    /// upload headers).
    pub headers: Vec<(Cow<'static, str>, String)>,
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// A POST with a JSON body.
    pub fn post_json(url: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: Method::Post,
            url: url.into(),
            headers: vec![("content-type".into(), "application/json".to_owned())],
            body: Some(body.into().into_bytes()),
        }
    }

    /// A PUT of raw bytes, e.g. a file to a presigned upload URL.
    pub fn put(url: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            method: Method::Put,
            url: url.into(),
            headers: Vec::new(),
            body: Some(body),
        }
    }

    pub fn header(mut self, name: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The value of the first header with this name, ignoring case.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<_> = self.headers.iter().map(|(name, _)| name.as_ref()).collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &names)
            .field("body_bytes", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

/// Any answer from the server, error statuses included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// Names in lowercase.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn with_header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_ascii_lowercase(), value.into()));
        self
    }

    /// The value of the first header with this name, ignoring case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Parses a recorded response, as adapter test fixtures store them: the
    /// status line (`HTTP/x <status>`), headers, a blank line, the body.
    /// Line endings may be CRLF (Windows checkouts).
    pub fn from_recording(raw: &str) -> Option<Self> {
        let raw = raw.replace("\r\n", "\n");
        let (head, body) = raw.split_once("\n\n")?;
        let mut lines = head.lines();
        let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        Some(Self {
            status,
            headers,
            body: body.to_owned(),
        })
    }
}

/// No answer: DNS, connection, TLS, proxy or timeout.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String);

/// An answer whose body is kept as bytes (a downloaded file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryResponse {
    pub status: u16,
    pub bytes: Vec<u8>,
}

/// Sends requests. Blocking: call it off the UI thread.
pub trait Transport: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError>;

    /// Sends `request` and keeps the body as bytes, up to `max_bytes`;
    /// a longer body is an error rather than a cut file. Transports that
    /// only replay text answer with the text's bytes.
    fn send_for_bytes(
        &self,
        request: &HttpRequest,
        max_bytes: u64,
    ) -> Result<BinaryResponse, TransportError> {
        let response = self.send(request)?;
        if response.body.len() as u64 > max_bytes {
            return Err(TransportError(format!(
                "the body is over {max_bytes} bytes"
            )));
        }
        Ok(BinaryResponse {
            status: response.status,
            bytes: response.body.into_bytes(),
        })
    }
}

/// Real HTTPS through `ureq`. Trusts the operating system's certificate
/// store (so a corporate proxy's root installed on Windows works) and the
/// proxy set in the standard environment variables.
pub struct UreqTransport {
    agent: ureq::Agent,
    max_body_bytes: u64,
}

impl UreqTransport {
    /// Bodies bigger than this are cut, unless an adapter allows more.
    /// Generated text and decisions stay far below it.
    pub const MAX_BODY_BYTES: u64 = 1024 * 1024;

    pub fn new(timeout: Duration) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .tls_config(tls)
            .user_agent(concat!("Bardo/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self {
            agent,
            max_body_bytes: Self::MAX_BODY_BYTES,
        }
    }

    /// Accepts bodies up to `bytes` (audio comes back inside JSON).
    pub fn with_max_body(mut self, bytes: u64) -> Self {
        self.max_body_bytes = bytes;
        self
    }

    /// For tests against a local server: plain HTTP, no proxy.
    #[doc(hidden)]
    pub fn without_proxy(timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .proxy(None)
            .build()
            .new_agent();
        Self {
            agent,
            max_body_bytes: Self::MAX_BODY_BYTES,
        }
    }
}

impl UreqTransport {
    fn call(
        &self,
        request: &HttpRequest,
    ) -> Result<ureq::http::Response<ureq::Body>, TransportError> {
        let sent = match request.method {
            Method::Get => {
                let mut call = self.agent.get(&request.url);
                for (name, value) in &request.headers {
                    call = call.header(name.as_ref(), value.as_str());
                }
                call.call()
            }
            Method::Post | Method::Put => {
                let mut call = if request.method == Method::Post {
                    self.agent.post(&request.url)
                } else {
                    self.agent.put(&request.url)
                };
                for (name, value) in &request.headers {
                    call = call.header(name.as_ref(), value.as_str());
                }
                call.send(request.body.as_deref().unwrap_or_default())
            }
        };
        sent.map_err(|error| TransportError(error.to_string()))
    }
}

impl Transport for UreqTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let mut response = self.call(request)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect();
        // An unreadable body still tells the status; the body is only detail.
        let body = response
            .body_mut()
            .with_config()
            .limit(self.max_body_bytes)
            .read_to_string()
            .unwrap_or_default();
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    fn send_for_bytes(
        &self,
        request: &HttpRequest,
        max_bytes: u64,
    ) -> Result<BinaryResponse, TransportError> {
        let mut response = self.call(request)?;
        let status = response.status().as_u16();
        // A file is useless when cut, so reading too much is an error.
        let bytes = response
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()
            .map_err(|error| TransportError(error.to_string()))?;
        Ok(BinaryResponse { status, bytes })
    }
}
