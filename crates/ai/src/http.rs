//! The HTTP seam every provider adapter goes through, so adapters can be
//! tested against recorded responses instead of live calls (ADR-0001).

use std::fmt;
use std::time::Duration;

/// A GET request. Header values may hold keys, so `Debug` shows only the
/// header names.
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
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
        let names: Vec<_> = self.headers.iter().map(|(name, _)| *name).collect();
        f.debug_struct("HttpRequest")
            .field("url", &self.url)
            .field("headers", &names)
            .finish()
    }
}

/// Any answer from the server, error statuses included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// No answer: DNS, connection, TLS, proxy or timeout.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TransportError(pub String);

/// Sends requests. Blocking: call it off the UI thread.
pub trait Transport: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError>;
}

/// Real HTTPS through `ureq`. Trusts the operating system's certificate
/// store (so a corporate proxy's root installed on Windows works) and the
/// proxy set in the standard environment variables.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    /// Bodies bigger than this are cut; key checks only read error messages.
    const MAX_BODY_BYTES: u64 = 256 * 1024;

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
        Self { agent }
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
        Self { agent }
    }
}

impl Transport for UreqTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let mut call = self.agent.get(&request.url);
        for (name, value) in &request.headers {
            call = call.header(*name, value.as_str());
        }
        let mut response = call
            .call()
            .map_err(|error| TransportError(error.to_string()))?;
        let status = response.status().as_u16();
        // An unreadable body still tells the status; the body is only detail.
        let body = response
            .body_mut()
            .with_config()
            .limit(Self::MAX_BODY_BYTES)
            .read_to_string()
            .unwrap_or_default();
        Ok(HttpResponse { status, body })
    }
}
