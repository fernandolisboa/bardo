//! The HTTP seam every provider adapter goes through, so adapters can be
//! tested against recorded responses instead of live calls (ADR-0001).

use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

use bardo_domain::Stop;

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
#[derive(Clone)]
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

/// Whether a request that got no answer may have reached the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Nothing the server could act on left this computer: no address, no
    /// connection, no TLS session, or part of the body was never handed
    /// over. Sending the request again is safe.
    NotSent,
    /// The request was sent, in whole or in part, or nothing says it was
    /// not: the server may have acted on it.
    Unknown,
}

/// No answer: DNS, connection, TLS, proxy, timeout, or an answer cut
/// before its end. The second field says whether the request may have
/// reached the server, which decides if a paid call is safe to make again.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String, pub Delivery);

impl TransportError {
    /// A request that never left.
    pub fn not_sent(detail: impl Into<String>) -> Self {
        Self(detail.into(), Delivery::NotSent)
    }

    /// A request that was sent, or may have been, and got no answer.
    pub fn unanswered(detail: impl Into<String>) -> Self {
        Self(detail.into(), Delivery::Unknown)
    }

    /// Whether the request never left, so sending it again is safe.
    pub fn never_left(&self) -> bool {
        self.1 == Delivery::NotSent
    }
}

/// An answer whose body is kept as bytes (a downloaded file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryResponse {
    pub status: u16,
    pub bytes: Vec<u8>,
}

/// An answer whose body is read as it arrives, so a large file never sits
/// whole in memory (an update's installer).
pub struct StreamedResponse {
    pub status: u16,
    /// Names in lowercase.
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn std::io::Read + Send>,
}

impl fmt::Debug for StreamedResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamedResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
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
            // The server answered, so it did the work.
            return Err(TransportError::unanswered(format!(
                "the body is over {max_bytes} bytes"
            )));
        }
        Ok(BinaryResponse {
            status: response.status,
            bytes: response.body.into_bytes(),
        })
    }

    /// Sends `request` and hands its body over as a reader. Transports that
    /// only replay text answer with the text's bytes.
    fn send_streamed(&self, request: &HttpRequest) -> Result<StreamedResponse, TransportError> {
        let response = self.send(request)?;
        Ok(StreamedResponse {
            status: response.status,
            headers: response.headers,
            body: Box::new(std::io::Cursor::new(response.body.into_bytes())),
        })
    }

    /// `send_streamed`, given up as soon as `stop` asks: while the answer
    /// is awaited, and at every read of its body.
    ///
    /// Every transport asks `stop` as the request is about to leave, and
    /// sends nothing when it asks then: the error says the request never
    /// left. A caller whose `stop` was not asked yet knows by that that
    /// nothing was sent (`Watched` decides so what a wait given up before
    /// the answer began may have cost). A transport that cannot give an
    /// answer up (this default) asks that once and reads the answer to its
    /// end.
    fn send_streamed_until(
        &self,
        request: &HttpRequest,
        stop: &Stop,
    ) -> Result<StreamedResponse, TransportError> {
        if stop.asked() {
            return Err(TransportError::not_sent(GIVEN_UP_UNSENT));
        }
        self.send_streamed(request)
    }
}

/// What a request given up before it left says.
pub const GIVEN_UP_UNSENT: &str = "given up before the request was sent";

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
        Self::configured(timeout, follow, false, true)
    }

    /// For calls the provider bills for the work it does: `new`, with a
    /// new connection for every request. A kept connection that died
    /// unnoticed takes the request before it fails, which would make a
    /// request that never arrived look like one that may have.
    pub fn for_paid_calls(timeout: Duration) -> Self {
        Self::configured(timeout, true, false, false)
    }

    /// `for_paid_calls` for a request that makes something public (a post
    /// on a network): no redirect is followed either, since following one
    /// turns the request into another one. It comes back as it is.
    pub fn for_posts(timeout: Duration) -> Self {
        Self::configured(timeout, false, false, false)
    }

    fn configured(timeout: Duration, follow: bool, https_only: bool, keep: bool) -> Self {
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
        if !keep {
            config = config
                .max_idle_connections(0)
                .max_idle_connections_per_host(0);
        }
        Self {
            agent: config.build().new_agent(),
            max_body_bytes: Self::MAX_BODY_BYTES,
        }
    }

    /// For public links (voice previews): follows redirects, but only to
    /// HTTPS, so a link checked as HTTPS stays HTTPS to the end.
    pub fn https_only(timeout: Duration) -> Self {
        Self::configured(timeout, true, true, true)
    }

    /// For downloading large files (an update's installer): follows
    /// redirects, but only to HTTPS. A whole file may take a long time on a
    /// slow line, so no global timeout applies: connecting and the first
    /// answer each have `wait`, the body has `body`.
    pub fn for_downloads(wait: Duration, body: Duration) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(wait))
            .timeout_recv_response(Some(wait))
            .timeout_recv_body(Some(body))
            .tls_config(tls)
            .https_only(true)
            .user_agent(concat!("Bardo/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self {
            agent,
            max_body_bytes: Self::MAX_BODY_BYTES,
        }
    }

    /// For answers read as they are written (a streamed generation). The
    /// answer may take many minutes, so no timeout bounds the request as
    /// a whole: finding and reaching the server have `connect`, sending
    /// the request and the first byte of the answer each have `wait`, and
    /// the body has `whole`, a bound far past any real answer. What bounds
    /// silence inside the body is the caller's (`idle::Watched`). HTTPS
    /// only.
    pub fn for_streams(connect: Duration, wait: Duration, whole: Duration) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_resolve(Some(connect))
            .timeout_connect(Some(connect))
            .timeout_send_request(Some(wait))
            .timeout_send_body(Some(wait))
            .timeout_recv_response(Some(wait))
            .timeout_recv_body(Some(whole))
            // A new connection for every request, as for every paid call:
            // a kept one that died unnoticed would take the request before
            // failing.
            .max_idle_connections(0)
            .max_idle_connections_per_host(0)
            .tls_config(tls)
            .https_only(true)
            .user_agent(concat!("Bardo/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self {
            agent,
            max_body_bytes: Self::MAX_BODY_BYTES,
        }
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

/// A request body that knows whether all of it was handed to the
/// connection. The body's length is announced, so a server cannot act on
/// a request while part of its body is still here: this tells a request
/// that cannot have arrived whole from one that may have, whatever the
/// error that followed. The last piece handed over may not have been
/// written, so "all handed over" is an upper bound, never the reverse.
struct TrackedBody<'a> {
    /// What was not handed over yet.
    bytes: &'a [u8],
    /// Asked once, when the connection first asks for the body: the last
    /// moment the request can be kept from leaving. Asked then, nothing of
    /// the body is handed over.
    leave: Option<&'a Stop>,
}

impl TrackedBody<'_> {
    fn all_handed_over(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl std::io::Read for TrackedBody<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.leave.take().is_some_and(Stop::asked) {
            return Err(std::io::Error::other(GIVEN_UP_UNSENT));
        }
        self.bytes.read(buffer)
    }
}

/// Whether a request that failed with `error` may have reached the
/// server. `body_handed_over` says whether the whole body was handed to
/// the connection; it is `None` for a request without a body, where only
/// the error tells: one from before the request is written (no address,
/// no connection, no TLS session) means it never left.
fn delivery(error: &ureq::Error, body_handed_over: Option<bool>) -> Delivery {
    use std::io::ErrorKind;

    let never_left = match body_handed_over {
        Some(handed_over) => !handed_over,
        None => match error {
            ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::ConnectProxyFailed(_)
            | ureq::Error::InvalidProxyUrl
            | ureq::Error::BadUri(_)
            | ureq::Error::Http(_)
            | ureq::Error::RequireHttpsOnly(_)
            | ureq::Error::TlsRequired
            | ureq::Error::Tls(_)
            | ureq::Error::Timeout(ureq::Timeout::Resolve | ureq::Timeout::Connect) => true,
            // Only what connecting alone reports: an unreachable network
            // can also show on a connection the request already went down.
            ureq::Error::Io(error) => matches!(
                error.kind(),
                ErrorKind::ConnectionRefused | ErrorKind::AddrNotAvailable
            ),
            _ => false,
        },
    };
    if never_left {
        Delivery::NotSent
    } else {
        Delivery::Unknown
    }
}

impl UreqTransport {
    fn call(
        &self,
        request: &HttpRequest,
    ) -> Result<ureq::http::Response<ureq::Body>, TransportError> {
        self.call_until(request, &Stop::never())
    }

    /// Sends `request` unless `stop` asks as it is about to leave: when
    /// the connection is there and asks for the body, or, for a request
    /// without one, before connecting.
    fn call_until(
        &self,
        request: &HttpRequest,
        stop: &Stop,
    ) -> Result<ureq::http::Response<ureq::Body>, TransportError> {
        let failed = |error: ureq::Error, body_handed_over: Option<bool>| {
            TransportError(error.to_string(), delivery(&error, body_handed_over))
        };
        let has_body = matches!(request.method, Method::Post | Method::Put)
            && request.body.as_deref().is_some_and(|body| !body.is_empty());
        if !has_body && stop.asked() {
            return Err(TransportError::not_sent(GIVEN_UP_UNSENT));
        }
        match request.method {
            Method::Get => {
                let mut call = self.agent.get(&request.url);
                for (name, value) in &request.headers {
                    call = call.header(name.as_ref(), value.as_str());
                }
                call.call().map_err(|error| failed(error, None))
            }
            Method::Delete => {
                let mut call = self.agent.delete(&request.url);
                for (name, value) in &request.headers {
                    call = call.header(name.as_ref(), value.as_str());
                }
                call.call().map_err(|error| failed(error, None))
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
                let bytes = request.body.as_deref().unwrap_or_default();
                if bytes.is_empty() {
                    return call.send(bytes).map_err(|error| failed(error, None));
                }
                // Read through the tracker, the body has no length of its
                // own, so the header states it.
                if request.header_value("content-length").is_none()
                    && request.header_value("transfer-encoding").is_none()
                {
                    call = call.header("content-length", bytes.len().to_string());
                }
                let mut body = TrackedBody {
                    bytes,
                    leave: Some(stop),
                };
                let sent = call.send(ureq::SendBody::from_reader(&mut body));
                sent.map_err(|error| failed(error, Some(body.all_handed_over())))
            }
        }
    }
}

/// Whether reading a body failed because it stopped arriving, as opposed
/// to arriving whole but too long or not as text.
fn is_cut(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::BodyExceedsLimit(_) => false,
        ureq::Error::Io(error) => error.kind() != std::io::ErrorKind::InvalidData,
        _ => true,
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
        let read = response
            .body_mut()
            .with_config()
            .limit(self.max_body_bytes)
            .read_to_string();
        let body = match read {
            Ok(body) => body,
            // A success cut before its end (the connection dropped, the
            // time ran out) is no answer: what was asked for is lost.
            Err(error) if (200..=299).contains(&status) && is_cut(&error) => {
                return Err(TransportError::unanswered(format!(
                    "the answer was cut off: {error}"
                )));
            }
            // Otherwise an unreadable body (too long, not text) still
            // tells the status; the body is only detail.
            Err(_) => String::new(),
        };
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
        let read = response
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec();
        let bytes = match read {
            Ok(bytes) => bytes,
            // A file is useless when cut, so a success that is cut or too
            // long is an error. The server answered: it did the work.
            Err(error) if (200..=299).contains(&status) => {
                return Err(TransportError::unanswered(error.to_string()));
            }
            // A failed status still tells; its body is only detail.
            Err(_) => Vec::new(),
        };
        Ok(BinaryResponse { status, bytes })
    }

    fn send_streamed(&self, request: &HttpRequest) -> Result<StreamedResponse, TransportError> {
        self.send_streamed_until(request, &Stop::never())
    }

    /// Asks `stop` only as the request is about to leave: the answer is
    /// given up by whoever reads it (`Watched`).
    fn send_streamed_until(
        &self,
        request: &HttpRequest,
        stop: &Stop,
    ) -> Result<StreamedResponse, TransportError> {
        let response = self.call_until(request, stop)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect();
        // Not limited here: the caller stops reading at its own limit.
        let body = response.into_body().into_reader();
        Ok(StreamedResponse {
            status,
            headers,
            body: Box::new(body),
        })
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
    fn a_streamed_body_is_read_as_it_arrives() {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nX-Note: hi\r\n\r\nhello world",
                )
                .unwrap();
        });
        let transport = UreqTransport::without_proxy(Duration::from_secs(5));
        let mut response = transport
            .send_streamed(&HttpRequest::get(format!("http://{address}/file")))
            .unwrap();
        assert_eq!(response.status, 200);
        assert!(
            response
                .headers
                .iter()
                .any(|(name, value)| name == "x-note" && value == "hi")
        );
        let mut body = String::new();
        response.body.read_to_string(&mut body).unwrap();
        assert_eq!(body, "hello world");
        server.join().unwrap();
    }

    /// A server on this computer that takes one request and does `then`
    /// with the connection. Returns its address and the request it read.
    fn one_request(
        then: impl FnOnce(&mut std::net::TcpStream) + Send + 'static,
    ) -> (std::net::SocketAddr, std::thread::JoinHandle<String>) {
        use std::io::Read as _;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            // Headers, then as many body bytes as they announce.
            loop {
                let read = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                let Some(head) = text.find("\r\n\r\n") else {
                    assert!(read > 0, "the request ended early");
                    continue;
                };
                let length: usize = text[..head]
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map_or(0, |value| value.trim().parse().unwrap());
                if request.len() >= head + 4 + length {
                    break;
                }
                assert!(read > 0, "the request ended early");
            }
            then(&mut stream);
            String::from_utf8_lossy(&request).into_owned()
        });
        (address, server)
    }

    fn paid_call(address: std::net::SocketAddr) -> HttpRequest {
        HttpRequest::post_json(format!("http://{address}/v1/generate"), r#"{"prompt":"a"}"#)
    }

    #[test]
    fn a_refused_connection_never_left() {
        // Nothing listens on a port that was just given back.
        let address = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap()
        };
        let transport = UreqTransport::without_proxy(Duration::from_secs(5));
        let error = transport.send(&paid_call(address)).unwrap_err();
        assert!(error.never_left(), "{error}");
        assert_eq!(error.1, Delivery::NotSent);
    }

    #[test]
    fn a_request_the_server_took_without_answering_may_have_been_done() {
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let (address, server) = one_request(move |_| {
            // Busy with the work until the client has given up.
            let _ = hold.recv();
        });
        let transport = UreqTransport::without_proxy(Duration::from_millis(300));
        let error = transport.send(&paid_call(address)).unwrap_err();
        assert!(!error.never_left(), "{error}");
        assert_eq!(error.1, Delivery::Unknown);
        release.send(()).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn a_connection_dropped_after_the_request_may_have_been_done() {
        let (address, server) = one_request(|_| {});
        let transport = UreqTransport::without_proxy(Duration::from_secs(5));
        let error = transport.send(&paid_call(address)).unwrap_err();
        assert!(!error.never_left(), "{error}");
        server.join().unwrap();
    }

    #[test]
    fn a_body_cut_on_its_way_never_arrived_whole() {
        use std::io::Read as _;

        // The server reads the start of a large body and hangs up.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut start = [0u8; 1024];
            stream.read_exact(&mut start).unwrap();
        });
        let transport = UreqTransport::without_proxy(Duration::from_secs(5));
        let request = HttpRequest::post_json(
            format!("http://{address}/v1/speech-to-text"),
            "a".repeat(32 * 1024 * 1024),
        );
        let error = transport.send(&request).unwrap_err();
        assert!(error.never_left(), "{error}");
        server.join().unwrap();
    }

    #[test]
    fn a_failed_tls_session_never_left() {
        // A server that is not speaking TLS: the session is never made.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || drop(listener.accept().unwrap()));
        let transport = UreqTransport::without_proxy(Duration::from_secs(5));
        let request = HttpRequest::post_json(format!("https://{address}/v1/generate"), "{}");
        let error = transport.send(&request).unwrap_err();
        assert!(error.never_left(), "{error}");
        server.join().unwrap();
    }

    #[test]
    fn a_body_goes_whole_with_its_length() {
        use std::io::Write as _;

        let (address, server) = one_request(|stream| {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .unwrap();
        });
        let transport = UreqTransport::without_proxy(Duration::from_secs(5));
        let response = transport.send(&paid_call(address)).unwrap();
        assert_eq!((response.status, response.body.as_str()), (200, "ok"));
        let request = server.join().unwrap();
        let (head, body) = request.split_once("\r\n\r\n").unwrap();
        let head = head.to_ascii_lowercase();
        assert!(head.contains("content-length: 14"), "{head}");
        assert!(!head.contains("transfer-encoding"), "{head}");
        assert_eq!(body, r#"{"prompt":"a"}"#);
    }

    #[test]
    fn an_answer_cut_before_its_end_is_no_answer() {
        use std::io::Write as _;

        let cut = |stream: &mut std::net::TcpStream| {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{\"half\":")
                .unwrap();
        };
        let transport = UreqTransport::without_proxy(Duration::from_secs(5));

        let (address, server) = one_request(cut);
        let error = transport.send(&paid_call(address)).unwrap_err();
        assert!(!error.never_left(), "{error}");
        server.join().unwrap();

        let (address, server) = one_request(cut);
        let error = transport
            .send_for_bytes(&paid_call(address), 1024)
            .unwrap_err();
        assert!(!error.never_left(), "{error}");
        server.join().unwrap();
    }

    #[test]
    fn a_failed_status_is_told_even_when_its_body_is_cut() {
        use std::io::Write as _;

        let (address, server) = one_request(|stream| {
            stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 100\r\n\r\n<html>")
                .unwrap();
        });
        let transport = UreqTransport::without_proxy(Duration::from_secs(5));
        let response = transport.send(&paid_call(address)).unwrap();
        assert_eq!(response.status, 502);
        server.join().unwrap();
    }

    #[test]
    fn a_file_keeps_a_failed_status_when_cut_and_is_no_answer_when_a_success_is_cut() {
        use std::io::Write as _;

        for (status_line, told) in [("401 Unauthorized", Some(401)), ("200 OK", None)] {
            let (address, server) = one_request(move |stream| {
                let head = format!("HTTP/1.1 {status_line}\r\nContent-Length: 100\r\n\r\nhalf");
                stream.write_all(head.as_bytes()).unwrap();
            });
            let transport = UreqTransport::without_proxy(Duration::from_secs(5));
            let answer = transport.send_for_bytes(&paid_call(address), 1024);
            match told {
                Some(status) => assert_eq!(answer.unwrap().status, status),
                None => assert!(!answer.unwrap_err().never_left()),
            }
            server.join().unwrap();
        }
    }

    #[test]
    fn an_answer_over_the_size_limit_keeps_its_status() {
        use std::io::Write as _;

        let (address, server) = one_request(|stream| {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nhello world")
                .unwrap();
        });
        let transport = UreqTransport::without_proxy(Duration::from_secs(5)).with_max_body(4);
        let response = transport.send(&paid_call(address)).unwrap();
        assert_eq!((response.status, response.body.as_str()), (200, ""));
        server.join().unwrap();
    }

    #[test]
    fn without_a_body_the_error_tells_whether_the_request_left() {
        use std::io::ErrorKind;

        let io = |kind: ErrorKind| ureq::Error::Io(kind.into());
        for error in [
            ureq::Error::HostNotFound,
            ureq::Error::ConnectionFailed,
            ureq::Error::ConnectProxyFailed("407".into()),
            ureq::Error::Timeout(ureq::Timeout::Resolve),
            ureq::Error::Timeout(ureq::Timeout::Connect),
            ureq::Error::Tls("no certificate"),
            io(ErrorKind::ConnectionRefused),
            io(ErrorKind::AddrNotAvailable),
        ] {
            assert_eq!(delivery(&error, None), Delivery::NotSent, "{error}");
        }
        for error in [
            ureq::Error::Timeout(ureq::Timeout::Global),
            ureq::Error::Timeout(ureq::Timeout::RecvResponse),
            io(ErrorKind::ConnectionReset),
            io(ErrorKind::NetworkUnreachable),
            io(ErrorKind::UnexpectedEof),
            io(ErrorKind::InvalidData),
        ] {
            assert_eq!(delivery(&error, None), Delivery::Unknown, "{error}");
        }
    }

    #[test]
    fn a_request_given_up_as_it_is_about_to_leave_hands_no_body_over() {
        use std::io::Read as _;

        // The connection is made, and the job is cancelled before the
        // request goes down it (a slow network, a slow TLS session).
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            let _ = stream.read_to_end(&mut received);
            String::from_utf8_lossy(&received).into_owned()
        });
        let transport = UreqTransport::without_proxy(Duration::from_secs(10));
        let request = HttpRequest::post_json(
            format!("http://{address}/v1/messages"),
            "{\"a paid request\":true}",
        );
        let Err(error) = transport.send_streamed_until(&request, &Stop::when(|| true)) else {
            panic!("given up");
        };
        assert!(error.never_left(), "{}", error.0);
        assert!(error.0.contains(GIVEN_UP_UNSENT), "{}", error.0);
        // Its length was announced and none of it came: the server has
        // nothing to act on.
        let received = server.join().unwrap();
        assert!(!received.contains("a paid request"), "{received}");

        // A request without a body is not sent at all.
        let Err(error) = transport.send_streamed_until(
            &HttpRequest::get(format!("http://{address}/")),
            &Stop::when(|| true),
        ) else {
            panic!("given up");
        };
        assert!(error.never_left());
    }

    #[test]
    fn with_a_body_whether_it_was_handed_over_tells() {
        // Without its whole body the server has nothing to act on; with
        // it, nothing says the server did not.
        let timed_out = ureq::Error::Timeout(ureq::Timeout::Global);
        assert_eq!(delivery(&timed_out, Some(false)), Delivery::NotSent);
        assert_eq!(delivery(&timed_out, Some(true)), Delivery::Unknown);
    }

    #[test]
    fn a_download_transport_refuses_plain_http() {
        let transport =
            UreqTransport::for_downloads(Duration::from_secs(1), Duration::from_secs(1));
        let error = transport
            .send_streamed(&HttpRequest::get("http://127.0.0.1:9/setup.exe"))
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
