//! The whole sign-in against a fake Google on `127.0.0.1`: the consent
//! address, the browser coming back to the loopback callback, the code
//! exchange (with the PKCE check Google does), the channel lookup, a
//! refresh and the revocation. Real HTTP and sockets; no call leaves the
//! machine.

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use bardo_ai::http::UreqTransport;
use bardo_domain::{
    AppCredentials, ConsentPages, ConsentReceiver, Network, NetworkSignIn, SignInFailureKind,
    TokenSet,
};
use bardo_publish::oauth::{ChallengeEncoding, Pkce};
use bardo_publish::{GoogleEndpoints, LoopbackReceiver, YouTubeSignIn};
use common::fixture;

// Fake values, split so secret scanners do not take them for real ones.
const CLIENT_ID: &str = concat!("1234567890-abc123def456", ".apps.googleusercontent.com");
const CLIENT_SECRET: &str = concat!("GOCSPX", "-fake-server-secret");
const CODE: &str = "4/0AeaY-fake-server-code";

/// What the fake Google remembers between requests.
#[derive(Default)]
struct Google {
    /// The challenge the consent address carried.
    challenge: Mutex<Option<String>>,
    revoked: Mutex<Vec<String>>,
    /// Refresh tokens Google no longer accepts.
    refused: Mutex<Vec<String>>,
}

fn fields(text: &str) -> Vec<(String, String)> {
    form_urlencoded::parse(text.as_bytes())
        .into_owned()
        .collect()
}

fn field(fields: &[(String, String)], name: &str) -> String {
    fields
        .iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

/// Serves Google's token, revoke and channels endpoints until the test
/// ends.
fn serve(google: Arc<Google>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            handle(stream, &google);
        }
    });
    address
}

fn handle(mut stream: TcpStream, google: &Google) {
    let mut raw = Vec::new();
    let mut chunk = [0; 4096];
    let (head, mut body) = loop {
        let read = stream.read(&mut chunk).unwrap();
        raw.extend_from_slice(&chunk[..read]);
        if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break (
                String::from_utf8(raw[..at].to_vec()).unwrap(),
                raw[at + 4..].to_vec(),
            );
        }
    };
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap_or(0);
    while body.len() < length {
        let read = stream.read(&mut chunk).unwrap();
        body.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8(body).unwrap();
    let line = head.lines().next().unwrap().to_owned();
    let response = route(&line, &head, &body, google);
    let reply = format!(
        "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        response.status,
        response.body.len(),
        response.body
    );
    stream.write_all(reply.as_bytes()).unwrap();
}

fn route(line: &str, head: &str, body: &str, google: &Google) -> bardo_ai::http::HttpResponse {
    let form = fields(body);
    if line.starts_with("POST /token ") {
        assert_eq!(field(&form, "client_id"), CLIENT_ID);
        assert_eq!(field(&form, "client_secret"), CLIENT_SECRET);
        return match field(&form, "grant_type").as_str() {
            "authorization_code" => {
                let challenge = google.challenge.lock().unwrap().clone().unwrap();
                let proven = Pkce::from_verifier(
                    &field(&form, "code_verifier"),
                    ChallengeEncoding::Base64Url,
                );
                if field(&form, "code") != CODE || proven.challenge() != challenge {
                    fixture("google-oauth", "token-invalid-grant")
                } else {
                    fixture("google-oauth", "token-exchange")
                }
            }
            "refresh_token" => {
                let token = field(&form, "refresh_token");
                if google.refused.lock().unwrap().contains(&token) {
                    fixture("google-oauth", "token-invalid-grant")
                } else {
                    fixture("google-oauth", "token-refresh")
                }
            }
            other => panic!("grant type {other}"),
        };
    }
    if line.starts_with("POST /revoke ") {
        google.revoked.lock().unwrap().push(field(&form, "token"));
        return fixture("google-oauth", "revoke-ok");
    }
    if line.starts_with("GET /youtube/v3/channels?") {
        let bearer = head
            .lines()
            .find_map(|line| line.strip_prefix("authorization: Bearer "))
            .unwrap_or_default();
        return if bearer.starts_with("ya29.a0AfB_byC-fixture-access-token") {
            fixture("google-oauth", "channels-mine")
        } else {
            fixture("google-oauth", "channels-invalid-token")
        };
    }
    panic!("unexpected request {line}");
}

/// What the browser does once the user consents: Google sends it to the
/// redirect address with the code and the same `state`.
fn consent_in_browser(consent_url: &str, google: &Google) {
    let query: Vec<(String, String)> = fields(consent_url.split_once('?').unwrap().1);
    *google.challenge.lock().unwrap() = Some(field(&query, "code_challenge"));
    let redirect = field(&query, "redirect_uri");
    let state = field(&query, "state");
    let host = redirect.trim_start_matches("http://").to_owned();
    let target = format!(
        "/?state={}&code={}&scope=youtube",
        form_urlencoded::byte_serialize(state.as_bytes()).collect::<String>(),
        form_urlencoded::byte_serialize(CODE.as_bytes()).collect::<String>()
    );
    thread::spawn(move || {
        let mut stream = TcpStream::connect(host).unwrap();
        write!(stream, "GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut page = String::new();
        stream.read_to_string(&mut page).unwrap();
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    });
}

fn sign_in(address: &str) -> YouTubeSignIn {
    YouTubeSignIn::with_transport(UreqTransport::without_proxy(Duration::from_secs(10)))
        .with_endpoints(GoogleEndpoints {
            authorize: format!("{address}/authorize"),
            token: format!("{address}/token"),
            revoke: format!("{address}/revoke"),
            api: address.to_owned(),
            analytics: address.to_owned(),
        })
}

#[test]
fn a_full_sign_in_refresh_and_revoke_against_a_fake_google() {
    let google = Arc::new(Google::default());
    let address = serve(Arc::clone(&google));
    let youtube = sign_in(&address);
    let credentials = AppCredentials::parse(Network::YouTube, CLIENT_ID, CLIENT_SECRET).unwrap();

    let callback = LoopbackReceiver.listen().unwrap();
    let redirect = callback.redirect_uri().to_owned();
    let request = youtube.consent_request(&credentials, &redirect);
    assert!(request.url.starts_with(&format!("{address}/authorize?")));
    consent_in_browser(&request.url, &google);
    let code = callback
        .wait(
            &request.state,
            Duration::from_secs(10),
            &AtomicBool::new(false),
            &ConsentPages {
                done: "done".into(),
                failed: "failed".into(),
            },
        )
        .unwrap();
    assert_eq!(code.expose(), CODE);

    let grant = youtube
        .exchange(&credentials, &code, &request.verifier, &redirect)
        .unwrap();
    let tokens = TokenSet::granted(&grant, SystemTime::now());
    let channel = youtube.identity(tokens.access_token()).unwrap();
    assert_eq!(channel.name, "Arquivos do Espaço");

    let refreshed = youtube
        .refresh(&credentials, tokens.refresh_token().unwrap())
        .map(|grant| tokens.refreshed(&grant, SystemTime::now()))
        .unwrap();
    assert_ne!(refreshed.access_token(), tokens.access_token());
    assert_eq!(refreshed.refresh_token(), tokens.refresh_token());

    youtube.revoke(&refreshed).unwrap();
    assert_eq!(
        *google.revoked.lock().unwrap(),
        [tokens.refresh_token().unwrap().to_owned()]
    );
}

#[test]
fn a_verifier_from_another_sign_in_is_refused() {
    let google = Arc::new(Google::default());
    let address = serve(Arc::clone(&google));
    let youtube = sign_in(&address);
    let credentials = AppCredentials::parse(Network::YouTube, CLIENT_ID, CLIENT_SECRET).unwrap();

    let ours = youtube.consent_request(&credentials, "http://127.0.0.1:1");
    let other = youtube.consent_request(&credentials, "http://127.0.0.1:1");
    *google.challenge.lock().unwrap() = Some(
        fields(ours.url.split_once('?').unwrap().1)
            .into_iter()
            .find(|(name, _)| name == "code_challenge")
            .unwrap()
            .1,
    );
    let failure = youtube
        .exchange(
            &credentials,
            &bardo_domain::SecretText::new(CODE),
            &other.verifier,
            "http://127.0.0.1:1",
        )
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Refused);
}

#[test]
fn a_refresh_google_refuses_is_reported_as_refused() {
    let google = Arc::new(Google::default());
    google
        .refused
        .lock()
        .unwrap()
        .push("1//0gExpired-testing-client".into());
    let address = serve(Arc::clone(&google));
    let credentials = AppCredentials::parse(Network::YouTube, CLIENT_ID, CLIENT_SECRET).unwrap();
    let failure = sign_in(&address)
        .refresh(&credentials, "1//0gExpired-testing-client")
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Refused);
}
