//! The one-shot loopback callback (ADR-0008): a listener on `127.0.0.1` at
//! a random port, which takes exactly one browser request carrying the
//! expected `state` and then closes.
//!
//! Plain `std::net`, no unsafe code: the listener is polled so a timeout or
//! a cancel from the app ends the wait.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bardo_domain::{ConsentCallback, ConsentError, ConsentPages, ConsentReceiver, SecretText};

use crate::oauth::same_secret;

/// How often a waiting callback checks for a request, a timeout or a
/// cancel.
const POLL: Duration = Duration::from_millis(50);
/// A browser sends its request at once; anything slower is not one.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Request lines and headers a browser sends fit far below this.
const MAX_HEAD_BYTES: usize = 16 * 1024;

/// Opens callbacks on the loopback interface.
#[derive(Debug, Default, Clone, Copy)]
pub struct LoopbackReceiver;

impl ConsentReceiver for LoopbackReceiver {
    fn listen(&self) -> Result<Box<dyn ConsentCallback>, ConsentError> {
        Ok(Box::new(LoopbackCallback::bind()?))
    }
}

/// A listener waiting for one consent callback.
#[derive(Debug)]
pub struct LoopbackCallback {
    listener: TcpListener,
    redirect_uri: String,
}

impl LoopbackCallback {
    /// Listens on `127.0.0.1` at a port the system picks. The literal IP,
    /// not `localhost`, as Google asks: `localhost` could resolve to
    /// another interface.
    pub fn bind() -> Result<Self, ConsentError> {
        let listen = |error: std::io::Error| ConsentError::Listen(error.to_string());
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(listen)?;
        listener.set_nonblocking(true).map_err(listen)?;
        let port = listener.local_addr().map_err(listen)?.port();
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}"),
        })
    }
}

/// What one browser request said.
enum Callback {
    /// The consent came back with a code.
    Code(SecretText),
    /// The network answered with an error code, e.g. `access_denied`.
    Denied(String),
    /// Not the callback (a favicon, a wrong `state`, a malformed request):
    /// answered and ignored.
    Ignored,
}

impl ConsentCallback for LoopbackCallback {
    fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    fn wait(
        self: Box<Self>,
        state: &SecretText,
        timeout: Duration,
        cancel: &AtomicBool,
        pages: &ConsentPages,
    ) -> Result<SecretText, ConsentError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ConsentError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(ConsentError::TimedOut);
            }
            match self.listener.accept() {
                Ok((stream, _)) => match answer(stream, state, pages) {
                    Callback::Code(code) => return Ok(code),
                    Callback::Denied(error) => return Err(ConsentError::Denied(error)),
                    Callback::Ignored => {}
                },
                Err(error) if error.kind() == ErrorKind::WouldBlock => std::thread::sleep(POLL),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(ConsentError::Listen(error.to_string())),
            }
        }
    }
}

/// Reads one request and answers it with a page.
fn answer(mut stream: TcpStream, state: &SecretText, pages: &ConsentPages) -> Callback {
    // An accepted socket may inherit the listener's non-blocking mode.
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(READ_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(READ_TIMEOUT)).is_err()
    {
        return Callback::Ignored;
    }
    let Some(target) = read_target(&mut stream) else {
        respond(&mut stream, 400, "Bad Request", &pages.failed);
        return Callback::Ignored;
    };
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    if path != "/" {
        respond(&mut stream, 404, "Not Found", &pages.failed);
        return Callback::Ignored;
    }
    let mut code = None;
    let mut error = None;
    let mut returned_state = None;
    for (name, value) in form_urlencoded::parse(query.as_bytes()) {
        match name.as_ref() {
            "code" => code = Some(SecretText::new(value.into_owned())),
            "error" => error = Some(value.into_owned()),
            "state" => returned_state = Some(SecretText::new(value.into_owned())),
            _ => {}
        }
    }
    let state_matches = returned_state
        .as_ref()
        .is_some_and(|returned| same_secret(returned.expose(), state.expose()));
    if !state_matches {
        // Not this sign-in's answer: another page, or a forged request.
        respond(&mut stream, 400, "Bad Request", &pages.failed);
        return Callback::Ignored;
    }
    if let Some(error) = error {
        respond(&mut stream, 200, "OK", &pages.failed);
        return Callback::Denied(error);
    }
    match code {
        Some(code) if is_code(code.expose()) => {
            respond(&mut stream, 200, "OK", &pages.done);
            Callback::Code(code)
        }
        _ => {
            respond(&mut stream, 400, "Bad Request", &pages.failed);
            Callback::Ignored
        }
    }
}

fn is_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 2048 && code.chars().all(|c| c.is_ascii_graphic())
}

/// The target of a `GET` request (`/?code=…&state=…`), from its request
/// line. Headers are read and dropped.
fn read_target(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        if head.len() > MAX_HEAD_BYTES {
            return None;
        }
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        head.extend_from_slice(&chunk[..read]);
    }
    let head = String::from_utf8(head).ok()?;
    let line = head.lines().next()?;
    let mut parts = line.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    (method == "GET" && version.starts_with("HTTP/") && parts.next().is_none())
        .then(|| target.to_owned())
}

/// A small page; the browser tab is all the user sees of the callback.
fn respond(stream: &mut TcpStream, status: u16, reason: &str, message: &str) {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Bardo</title></head>\
         <body style=\"font-family:system-ui,sans-serif;margin:3em\"><p>{}</p></body></html>",
        escape_html(message)
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Referrer-Policy: no-referrer\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    // The browser may already be gone; the outcome does not depend on it.
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn escape_html(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            c => c.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::*;

    fn pages() -> ConsentPages {
        ConsentPages {
            done: "Connected. You can close this tab.".into(),
            failed: "Not connected <try again>.".into(),
        }
    }

    /// What a browser does: one GET to `url`, returning the status and body.
    fn browse(url: &str) -> (u16, String) {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, target) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let target = if target.is_empty() { "/" } else { target };
        let mut stream = TcpStream::connect(host).unwrap();
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: test\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status = response.split(' ').nth(1).unwrap().parse().unwrap();
        let body = response.split_once("\r\n\r\n").unwrap().1.to_owned();
        (status, body)
    }

    fn start(
        timeout: Duration,
    ) -> (
        String,
        thread::JoinHandle<Result<SecretText, ConsentError>>,
        Arc<AtomicBool>,
    ) {
        let callback = Box::new(LoopbackCallback::bind().unwrap());
        let uri = callback.redirect_uri().to_owned();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let waiting = thread::spawn(move || {
            callback.wait(&SecretText::new("state-123"), timeout, &flag, &pages())
        });
        (uri, waiting, cancel)
    }

    #[test]
    fn the_redirect_is_the_loopback_ip_with_a_random_port() {
        let a = LoopbackCallback::bind().unwrap();
        let b = LoopbackCallback::bind().unwrap();
        assert!(a.redirect_uri().starts_with("http://127.0.0.1:"));
        assert_ne!(a.redirect_uri(), b.redirect_uri());
        assert!(!a.redirect_uri().ends_with(":0"));
    }

    #[test]
    fn the_matching_callback_brings_the_code_and_closes_the_listener() {
        let (uri, waiting, _) = start(Duration::from_secs(10));
        let (status, body) = browse(&format!("{uri}/?state=state-123&code=4%2F0Abc-def&scope=x"));
        assert_eq!(status, 200);
        assert!(
            body.contains("Connected. You can close this tab."),
            "{body}"
        );
        assert_eq!(waiting.join().unwrap().unwrap().expose(), "4/0Abc-def");
        // Exactly one: nothing listens any more.
        assert!(TcpStream::connect(uri.trim_start_matches("http://")).is_err());
    }

    #[test]
    fn a_wrong_state_is_refused_and_the_wait_goes_on() {
        let (uri, waiting, _) = start(Duration::from_secs(10));
        let (status, body) = browse(&format!("{uri}/?state=forged&code=attacker-code"));
        assert_eq!(status, 400);
        assert!(body.contains("Not connected &lt;try again&gt;."), "{body}");
        let (status, _) = browse(&format!("{uri}/?code=no-state"));
        assert_eq!(status, 400);
        let (status, _) = browse(&format!("{uri}/favicon.ico"));
        assert_eq!(status, 404);
        browse(&format!("{uri}/?state=state-123&code=real-code"));
        assert_eq!(waiting.join().unwrap().unwrap().expose(), "real-code");
    }

    #[test]
    fn a_declined_consent_ends_the_wait() {
        let (uri, waiting, _) = start(Duration::from_secs(10));
        let (status, body) = browse(&format!("{uri}/?error=access_denied&state=state-123"));
        assert_eq!(status, 200);
        assert!(body.contains("Not connected"), "{body}");
        assert_eq!(
            waiting.join().unwrap().unwrap_err(),
            ConsentError::Denied("access_denied".into())
        );
    }

    #[test]
    fn a_declined_consent_with_a_wrong_state_is_ignored() {
        let (uri, waiting, cancel) = start(Duration::from_secs(10));
        browse(&format!("{uri}/?error=access_denied&state=forged"));
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            waiting.join().unwrap().unwrap_err(),
            ConsentError::Cancelled
        );
    }

    #[test]
    fn the_wait_times_out_when_consent_never_comes() {
        let (_, waiting, _) = start(Duration::from_millis(200));
        assert_eq!(waiting.join().unwrap().unwrap_err(), ConsentError::TimedOut);
    }

    #[test]
    fn a_cancel_ends_the_wait() {
        let (_, waiting, cancel) = start(Duration::from_secs(30));
        thread::sleep(Duration::from_millis(100));
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            waiting.join().unwrap().unwrap_err(),
            ConsentError::Cancelled
        );
    }

    #[test]
    fn only_get_requests_count() {
        let (uri, waiting, cancel) = start(Duration::from_secs(10));
        let host = uri.trim_start_matches("http://");
        let mut stream = TcpStream::connect(host).unwrap();
        write!(
            stream,
            "POST /?state=state-123&code=posted HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            waiting.join().unwrap().unwrap_err(),
            ConsentError::Cancelled
        );
    }
}
