//! Key checks against recorded provider responses (see
//! `fixtures/key-check/README.md`). No test calls a provider.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use bardo_ai::HttpKeyChecker;
use bardo_ai::http::{HttpRequest, HttpResponse, Transport, TransportError, UreqTransport};
use bardo_ai::key_check::{check_request, classify};
use bardo_domain::{ApiKey, KeyCheck, KeyCheckOutcome, KeyChecker, Provider};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/key-check");

/// The raw text of a recorded response.
fn raw_fixture(name: &str) -> String {
    let path = Path::new(FIXTURES).join(format!("{name}.http"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A recorded response.
fn fixture(name: &str) -> HttpResponse {
    HttpResponse::from_recording(&raw_fixture(name)).expect("a recorded response")
}

fn key(provider: Provider) -> ApiKey {
    let text = match provider {
        Provider::Higgsfield => "key-id-0001:key-secret-0001",
        _ => "test-key-0001-abcdef",
    };
    ApiKey::parse(provider, text).unwrap()
}

fn check(provider: Provider, fixture_name: &str) -> KeyCheck {
    classify(provider, &fixture(fixture_name))
}

#[test]
fn a_valid_key_is_valid_for_every_provider() {
    for provider in Provider::ALL {
        let checked = check(provider, &format!("{}-valid", provider.code()));
        assert_eq!(checked.outcome, KeyCheckOutcome::Valid, "{provider}");
    }
}

#[test]
fn an_invalid_key_is_rejected_by_every_provider_with_its_message() {
    let messages = [
        (Provider::Claude, "API key is invalid."),
        (Provider::ElevenLabs, "Invalid API key"),
        (
            Provider::Gemini,
            "API key not valid. Please pass a valid API key.",
        ),
        (Provider::Higgsfield, "Invalid credentials"),
        (
            Provider::TypeSafe,
            "Cannot authenticate with the server. Please check your API key and try again.",
        ),
        (
            Provider::YouTubeData,
            "API key not valid. Please pass a valid API key.",
        ),
    ];
    for (provider, message) in messages {
        let checked = check(provider, &format!("{}-rejected", provider.code()));
        assert_eq!(
            checked,
            KeyCheck::new(KeyCheckOutcome::Rejected, Some(message.into())),
            "{provider}"
        );
    }
}

#[test]
fn a_key_without_the_needed_permission_is_not_allowed() {
    let elevenlabs = check(Provider::ElevenLabs, "elevenlabs-missing-permission");
    assert_eq!(elevenlabs.outcome, KeyCheckOutcome::NotAllowed);
    assert!(elevenlabs.detail.unwrap().contains("voices_read"));

    let gemini = check(Provider::Gemini, "gemini-service-disabled");
    assert_eq!(gemini.outcome, KeyCheckOutcome::NotAllowed);
}

#[test]
fn quotas_rate_limits_and_credits_are_limits_not_bad_keys() {
    for (provider, name) in [
        (Provider::YouTubeData, "youtube-data-quota"),
        (Provider::TypeSafe, "typesafe-rate-limited"),
        (Provider::Higgsfield, "higgsfield-no-credits"),
    ] {
        assert_eq!(
            check(provider, name).outcome,
            KeyCheckOutcome::LimitReached,
            "{name}"
        );
    }
}

#[test]
fn provider_failures_are_not_blamed_on_the_key() {
    assert_eq!(
        check(Provider::Claude, "claude-overloaded"),
        KeyCheck::new(KeyCheckOutcome::ProviderDown, Some("Overloaded".into()))
    );
    let proxy = check(Provider::Gemini, "proxy-error-page");
    assert_eq!(proxy, KeyCheck::new(KeyCheckOutcome::ProviderDown, None));
}

#[test]
fn an_unknown_answer_is_unexpected_and_names_the_status() {
    assert_eq!(
        check(Provider::TypeSafe, "unknown-status"),
        KeyCheck::new(KeyCheckOutcome::Unexpected, Some("HTTP 418".into()))
    );
    // A 404 only means "valid" for Higgsfield's deliberate lookup.
    assert_eq!(
        check(Provider::Claude, "higgsfield-valid").outcome,
        KeyCheckOutcome::Unexpected
    );
}

#[test]
fn long_provider_messages_are_cut() {
    let long = "x".repeat(1000);
    let checked = classify(
        Provider::Claude,
        &HttpResponse::new(401, format!(r#"{{"error":{{"message":"{long}"}}}}"#)),
    );
    let detail = checked.detail.unwrap();
    assert_eq!(detail.chars().count(), 301);
    assert!(detail.ends_with('…'));
}

#[test]
fn each_provider_gets_its_key_in_its_own_header_and_never_in_the_url() {
    let expected = [
        (Provider::Claude, "x-api-key", "test-key-0001-abcdef"),
        (Provider::ElevenLabs, "xi-api-key", "test-key-0001-abcdef"),
        (Provider::Gemini, "x-goog-api-key", "test-key-0001-abcdef"),
        (
            Provider::Higgsfield,
            "authorization",
            "Key key-id-0001:key-secret-0001",
        ),
        (
            Provider::TypeSafe,
            "authorization",
            "Bearer test-key-0001-abcdef",
        ),
        (
            Provider::YouTubeData,
            "x-goog-api-key",
            "test-key-0001-abcdef",
        ),
    ];
    for (provider, header, value) in expected {
        let request = check_request(provider, &key(provider));
        assert!(request.url.starts_with("https://"), "{provider}");
        assert_eq!(request.header_value(header), Some(value), "{provider}");
        assert!(!request.url.contains("test-key"), "{provider}");
        assert!(!request.url.contains("key-secret"), "{provider}");
    }
    let claude = check_request(Provider::Claude, &key(Provider::Claude));
    assert_eq!(claude.header_value("anthropic-version"), Some("2023-06-01"));
}

#[test]
fn request_debug_output_hides_header_values() {
    let request = check_request(Provider::Claude, &key(Provider::Claude));
    let shown = format!("{request:?}");
    assert!(shown.contains("x-api-key"), "{shown}");
    assert!(!shown.contains("test-key"), "{shown}");
}

/// Replays one fixture and remembers what was asked.
struct Replay {
    response: Result<HttpResponse, TransportError>,
    asked: Mutex<Vec<String>>,
}

impl Transport for Replay {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        self.asked.lock().unwrap().push(request.url.clone());
        self.response.clone()
    }
}

#[test]
fn the_checker_sends_the_check_request_and_classifies_the_answer() {
    let replay = Replay {
        response: Ok(fixture("gemini-rejected")),
        asked: Mutex::default(),
    };
    let checker = HttpKeyChecker::with_transport(replay);
    let checked = checker.check(Provider::Gemini, &key(Provider::Gemini));
    assert_eq!(checked.outcome, KeyCheckOutcome::Rejected);
    let expected = check_request(Provider::Gemini, &key(Provider::Gemini)).url;
    assert_eq!(*checker.transport().asked.lock().unwrap(), [expected]);
}

#[test]
fn no_answer_is_unreachable_with_the_network_error() {
    let checker = HttpKeyChecker::with_transport(Replay {
        response: Err(TransportError("dns error: no such host".into())),
        asked: Mutex::default(),
    });
    assert_eq!(
        checker.check(Provider::Claude, &key(Provider::Claude)),
        KeyCheck::new(
            KeyCheckOutcome::Unreachable,
            Some("dns error: no such host".into())
        )
    );
}

/// The real transport against a local server replaying a recorded answer:
/// headers go out, error statuses come back as answers, not errors.
#[test]
fn ureq_transport_sends_headers_and_returns_error_statuses() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            head.push(line.trim().to_owned());
        }
        let body = r#"{"detail":"Invalid credentials"}"#;
        let mut stream = stream;
        write!(
            stream,
            "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        head
    });

    let transport = UreqTransport::without_proxy(Duration::from_secs(5));
    let request = HttpRequest::get(format!("http://{address}/requests/x/status"))
        .header("authorization", "Key id-0001:secret-0001");
    let response = transport.send(&request).unwrap();

    let head = server.join().unwrap();
    assert!(head[0].starts_with("GET /requests/x/status "), "{head:?}");
    assert!(
        head.iter()
            .any(|line| line.eq_ignore_ascii_case("authorization: Key id-0001:secret-0001")),
        "{head:?}"
    );
    assert_eq!(response.status, 401);
    assert_eq!(
        classify(Provider::Higgsfield, &response).outcome,
        KeyCheckOutcome::Rejected
    );
}

#[test]
fn ureq_transport_reports_a_refused_connection_as_unreachable() {
    let address = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let transport = UreqTransport::without_proxy(Duration::from_secs(5));
    assert!(
        transport
            .send(&HttpRequest::get(format!("http://{address}/")))
            .is_err()
    );
}

#[test]
fn ureq_transport_posts_json_and_reads_response_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            head.push(line.trim().to_owned());
        }
        let length: usize = head
            .iter()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().ok())?
            })
            .expect("a content-length");
        let mut body = vec![0; length];
        std::io::Read::read_exact(&mut reader, &mut body).unwrap();
        let answer = r#"{"detail":{"message":"Rate limit exceeded."}}"#;
        let mut stream = stream;
        write!(
            stream,
            "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\nretry-after: 20\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
            answer.len()
        )
        .unwrap();
        (head, String::from_utf8(body).unwrap())
    });

    let transport = UreqTransport::without_proxy(Duration::from_secs(5));
    let request = HttpRequest::post_json(format!("http://{address}/v1/systemone"), r#"{"a":1}"#);
    let response = transport.send(&request).unwrap();

    let (head, body) = server.join().unwrap();
    assert!(head[0].starts_with("POST /v1/systemone "), "{head:?}");
    assert!(
        head.iter()
            .any(|line| line.eq_ignore_ascii_case("content-type: application/json")),
        "{head:?}"
    );
    assert_eq!(body, r#"{"a":1}"#);
    assert_eq!(response.status, 429);
    assert_eq!(response.header("Retry-After"), Some("20"));
}
