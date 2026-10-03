//! Helpers shared by the adapter tests that replay recorded responses.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use bardo_ai::http::{HttpRequest, HttpResponse, Method, Transport, TransportError};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// A recorded response from `fixtures/<dir>/<name>.http`.
pub fn fixture(dir: &str, name: &str) -> HttpResponse {
    let path = Path::new(FIXTURES).join(dir).join(format!("{name}.http"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    HttpResponse::from_recording(&raw).expect("a recorded response")
}

/// A request as the transport saw it.
#[derive(Debug, Clone)]
pub struct Sent {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Sent {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The fields of a form body.
    pub fn form(&self) -> Vec<(String, String)> {
        form_urlencoded::parse(self.body.as_bytes())
            .into_owned()
            .collect()
    }

    pub fn field(&self, name: &str) -> Option<String> {
        self.form()
            .into_iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
    }
}

/// Answers with the given responses in order, then fails the test; keeps
/// every request.
#[derive(Default)]
pub struct Scripted {
    answers: Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
    sent: Mutex<Vec<Sent>>,
}

impl Scripted {
    pub fn new(answers: Vec<HttpResponse>) -> Self {
        Self {
            answers: Mutex::new(answers.into_iter().map(Ok).collect()),
            ..Self::default()
        }
    }

    pub fn offline() -> Self {
        Self {
            answers: Mutex::new(VecDeque::from([Err(TransportError(
                "dns error: no such host".into(),
            ))])),
            ..Self::default()
        }
    }

    pub fn sent(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }
}

impl Transport for Scripted {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        self.sent.lock().unwrap().push(Sent {
            method: request.method,
            url: request.url.clone(),
            headers: request
                .headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect(),
            body: String::from_utf8(request.body.clone().unwrap_or_default()).unwrap(),
        });
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("no scripted answer left")
    }
}
