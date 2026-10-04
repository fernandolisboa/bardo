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
    Delete,
}

/// A request. Header values may hold keys, query strings tokens and
/// secrets, and bodies user content, so `Debug` shows only the address
/// without its query, the header names and the body size.
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

    /// A DELETE without a body, e.g. revoking an app's permissions.
    pub fn delete(url: impl Into<String>) -> Self {
        Self {
            method: Method::Delete,
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

    /// A POST of an HTML form without files
    /// (`application/x-www-form-urlencoded`), e.g. an OAuth token request.
    pub fn post_form(url: impl Into<String>, fields: &[(&str, &str)]) -> Self {
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish();
        Self {
            method: Method::Post,
            url: url.into(),
            headers: vec![(
                "content-type".into(),
                "application/x-www-form-urlencoded".to_owned(),
            )],
            body: Some(body.into_bytes()),
        }
    }

    /// A POST of an HTML form with files (`multipart/form-data`), e.g. an
    /// audio file and its text.
    pub fn post_multipart(url: impl Into<String>, parts: &[FormPart<'_>]) -> Self {
        let boundary = boundary_for(parts);
        let mut body = Vec::with_capacity(
            parts
                .iter()
                .map(|part| part.bytes().len() + 160)
                .sum::<usize>()
                + 64,
        );
        for part in parts {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            match part {
                FormPart::Text { name, value } => {
                    body.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                            quoted(name)
                        )
                        .as_bytes(),
                    );
                    body.extend_from_slice(value.as_bytes());
                }
                FormPart::File {
                    name,
                    file_name,
                    content_type,
                    bytes,
                } => {
                    body.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n\
                             Content-Type: {content_type}\r\n\r\n",
                            quoted(name),
                            quoted(file_name)
                        )
                        .as_bytes(),
                    );
                    body.extend_from_slice(bytes);
                }
            }
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        Self {
            method: Method::Post,
            url: url.into(),
            headers: vec![(
                "content-type".into(),
                format!("multipart/form-data; boundary={boundary}"),
            )],
            body: Some(body),
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

/// One field of a multipart form.
#[derive(Debug, Clone, Copy)]
pub enum FormPart<'a> {
    Text {
        name: &'a str,
        value: &'a str,
    },
    File {
        name: &'a str,
        file_name: &'a str,
        content_type: &'a str,
        bytes: &'a [u8],
    },
}

impl FormPart<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            FormPart::Text { value, .. } => value.as_bytes(),
            FormPart::File { bytes, .. } => bytes,
        }
    }
}

/// A boundary that appears in no part, so the server cannot cut a part
/// short. A collision with audio bytes is all but impossible; checking
/// makes it impossible.
fn boundary_for(parts: &[FormPart<'_>]) -> String {
    (0u32..)
        .map(|n| format!("bardo-form-boundary-7d1c4f9a2e{n}"))
        .find(|boundary| {
            let needle = boundary.as_bytes();
            parts
                .iter()
                .all(|part| !part.bytes().windows(needle.len()).any(|w| w == needle))
        })
        .expect("some boundary is free")
}

/// A form field or file name inside double quotes: quotes and line breaks
/// would end it early, so they become underscores.
fn quoted(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c, '"' | '\r' | '\n') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<_> = self.headers.iter().map(|(name, _)| name.as_ref()).collect();
        let address = self
            .url
            .split_once('?')
            .map_or(self.url.as_str(), |(path, _)| path);
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &address)
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
        Self::with_redirects(timeout, true)
    }

    /// For resumable uploads: a 308 answer is "resume incomplete", not a
    /// redirect to follow, so redirects come back as they are. `timeout`
    /// bounds each request, a chunk of the file included.
    pub fn for_uploads(timeout: Duration) -> Self {
        Self::with_redirects(timeout, false)
    }

    fn with_redirects(timeout: Duration, follow: bool) -> Self {
        Self::configured(timeout, follow, false)
    }

    fn configured(timeout: Duration, follow: bool, https_only: bool) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let mut config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .tls_config(tls)
            .https_only(https_only)
            .user_agent(concat!("Bardo/", env!("CARGO_PKG_VERSION")));
        if !follow {
            config = config.max_redirects(0);
        }
        Self {
            agent: config.build().new_agent(),
            max_body_bytes: Self::MAX_BODY_BYTES,
        }
    }

    /// For public links (voice previews): follows redirects, but only to
    /// HTTPS, so a link checked as HTTPS stays HTTPS to the end.
    pub fn https_only(timeout: Duration) -> Self {
        Self::configured(timeout, true, true)
    }

    /// `for_uploads` against a local server: plain HTTP, no proxy.
    #[doc(hidden)]
    pub fn for_uploads_without_proxy(timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(timeout))
            .proxy(None)
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
            Method::Delete => {
                let mut call = self.agent.delete(&request.url);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_https_only_transport_refuses_plain_http_at_any_hop() {
        // Checked before connecting, on the first link and on each
        // redirect alike: nothing listens here.
        let transport = UreqTransport::https_only(Duration::from_secs(1));
        let error = transport
            .send_for_bytes(&HttpRequest::get("http://127.0.0.1:9/preview.mp3"), 1024)
            .unwrap_err();
        assert!(error.0.contains("https only"), "{}", error.0);
    }

    #[test]
    fn debug_hides_the_query_headers_and_body() {
        let request = HttpRequest::get("https://example.test/oauth?client_secret=s3cr3t")
            .header("authorization", "Bearer t0ken");
        let shown = format!("{request:?}");
        assert!(shown.contains("https://example.test/oauth"));
        assert!(!shown.contains("s3cr3t"));
        assert!(!shown.contains("t0ken"));
    }

    #[test]
    fn a_plain_form_is_url_encoded() {
        let request = HttpRequest::post_form(
            "https://example.test/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", "1//a b+c"),
            ],
        );
        assert_eq!(
            request.header_value("content-type"),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(
            request.body.as_deref(),
            Some(&b"grant_type=refresh_token&refresh_token=1%2F%2Fa+b%2Bc"[..])
        );
    }

    #[test]
    fn a_multipart_form_holds_each_part_between_boundaries() {
        let request = HttpRequest::post_multipart(
            "https://example.test/upload",
            &[
                FormPart::File {
                    name: "file",
                    file_name: "take \"3\".wav",
                    content_type: "audio/wav",
                    bytes: b"RIFF\x00\x01",
                },
                FormPart::Text {
                    name: "text",
                    value: "Olá, mundo.",
                },
            ],
        );
        assert_eq!(request.method, Method::Post);
        let content_type = request.header_value("content-type").unwrap();
        let boundary = content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
                 filename=\"take _3_.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
            )
            .as_bytes(),
        );
        expected.extend_from_slice(b"RIFF\x00\x01\r\n");
        expected.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\n\
                 Olá, mundo.\r\n--{boundary}--\r\n"
            )
            .as_bytes(),
        );
        assert_eq!(request.body.unwrap(), expected);
    }

    #[test]
    fn the_boundary_never_appears_in_a_part() {
        let taken = b"xx bardo-form-boundary-7d1c4f9a2e0 xx";
        let request = HttpRequest::post_multipart(
            "https://example.test/upload",
            &[FormPart::File {
                name: "file",
                file_name: "a.mp3",
                content_type: "audio/mpeg",
                bytes: taken,
            }],
        );
        assert!(
            request
                .header_value("content-type")
                .unwrap()
                .ends_with("bardo-form-boundary-7d1c4f9a2e1")
        );
    }
}
