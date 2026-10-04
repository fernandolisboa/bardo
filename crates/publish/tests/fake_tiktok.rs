//! The whole sign-in against a fake TikTok on `127.0.0.1`: the consent
//! address, the browser coming back to the loopback callback at
//! `/callback/`, the code exchange (with the hex PKCE check and the exact
//! redirect match TikTok does), the creator lookup, a refresh that rotates
//! the refresh token, and the revocation. Real HTTP and sockets; no call
//! leaves the machine.

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use bardo_ai::http::UreqTransport;
use bardo_domain::{
    AppCredentials, BrowserSignIn, ConsentPages, ConsentReceiver, Network, NetworkSignIn,
    SignInFailureKind, TokenSet,
};
use bardo_publish::oauth::{ChallengeEncoding, Pkce};
use bardo_publish::{LoopbackReceiver, TikTokEndpoints, TikTokSignIn};
use common::fixture;

// Fake values, split so secret scanners do not take them for real ones.
const CLIENT_KEY: &str = concat!("awfakeserver", "key000001");
const CLIENT_SECRET: &str = concat!("FakeServerSecret", "0000000000000000");
const CODE: &str = "fake-server-code*1!";

/// What the fake TikTok remembers between requests.
#[derive(Default)]
struct TikTok {
    /// The challenge and redirect the consent address carried.
    challenge: Mutex<Option<String>>,
    redirect: Mutex<Option<String>>,
    revoked: Mutex<Vec<String>>,
    /// Refresh tokens already traded: TikTok rotated them.
    spent: Mutex<Vec<String>>,
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

/// Serves TikTok's OAuth and user info endpoints until the test ends.
fn serve(tiktok: Arc<TikTok>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            handle(stream, &tiktok);
        }
    });
    address
}

fn handle(mut stream: TcpStream, tiktok: &TikTok) {
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
    let response = route(&line, &head, &body, tiktok);
    let reply = format!(
        "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        response.status,
        response.body.len(),
        response.body
    );
    stream.write_all(reply.as_bytes()).unwrap();
}

fn route(line: &str, head: &str, body: &str, tiktok: &TikTok) -> bardo_ai::http::HttpResponse {
    let form = fields(body);
    if line.starts_with("POST /v2/oauth/token/ ") {
        if field(&form, "client_key") != CLIENT_KEY
            || field(&form, "client_secret") != CLIENT_SECRET
        {
            return fixture("tiktok-oauth", "token-invalid-client");
        }
        return match field(&form, "grant_type").as_str() {
            "authorization_code" => {
                let challenge = tiktok.challenge.lock().unwrap().clone().unwrap();
                let redirect = tiktok.redirect.lock().unwrap().clone().unwrap();
                let proven =
                    Pkce::from_verifier(&field(&form, "code_verifier"), ChallengeEncoding::Hex);
                if field(&form, "code") != CODE
                    || proven.challenge() != challenge
                    || field(&form, "redirect_uri") != redirect
                {
                    fixture("tiktok-oauth", "token-code-expired")
                } else {
                    fixture("tiktok-oauth", "token-exchange")
                }
            }
            "refresh_token" => {
                let token = field(&form, "refresh_token");
                let mut spent = tiktok.spent.lock().unwrap();
                if token == "rft.fixture-refresh-token-0001" && !spent.contains(&token) {
                    spent.push(token);
                    fixture("tiktok-oauth", "token-refresh-rotated")
                } else {
                    fixture("tiktok-oauth", "token-refresh-invalid-grant")
                }
            }
            other => panic!("grant type {other}"),
        };
    }
    if line.starts_with("POST /v2/oauth/revoke/ ") {
        assert_eq!(field(&form, "client_key"), CLIENT_KEY);
        assert_eq!(field(&form, "client_secret"), CLIENT_SECRET);
        tiktok.revoked.lock().unwrap().push(field(&form, "token"));
        return fixture("tiktok-oauth", "revoke-ok");
    }
    if line.starts_with("GET /v2/user/info/?") {
        let bearer = head
            .lines()
            .find_map(|line| line.strip_prefix("authorization: Bearer "))
            .unwrap_or_default();
        return if bearer.starts_with("act.fixture-access-token") {
            fixture("tiktok-oauth", "user-info")
        } else {
            fixture("tiktok-oauth", "user-info-invalid-token")
        };
    }
    panic!("unexpected request {line}");
}

/// What the browser does once the user authorizes: TikTok sends it to the
/// redirect address with the code, the granted scopes and the same
/// `state`.
fn consent_in_browser(consent_url: &str, tiktok: &TikTok) {
    let query: Vec<(String, String)> = fields(consent_url.split_once('?').unwrap().1);
    *tiktok.challenge.lock().unwrap() = Some(field(&query, "code_challenge"));
    let redirect = field(&query, "redirect_uri");
    *tiktok.redirect.lock().unwrap() = Some(redirect.clone());
    let state = field(&query, "state");
    let (host, path) = redirect
        .trim_start_matches("http://")
        .split_once('/')
        .unwrap();
    let (host, path) = (host.to_owned(), format!("/{path}"));
    let target = format!(
        "{path}?code={}&scopes=user.info.basic%2Cvideo.upload%2Cvideo.list&state={}",
        form_urlencoded::byte_serialize(CODE.as_bytes()).collect::<String>(),
        form_urlencoded::byte_serialize(state.as_bytes()).collect::<String>()
    );
    thread::spawn(move || {
        let mut stream = TcpStream::connect(host).unwrap();
        write!(stream, "GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut page = String::new();
        stream.read_to_string(&mut page).unwrap();
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    });
}

fn sign_in(address: &str) -> TikTokSignIn {
    TikTokSignIn::with_transport(UreqTransport::without_proxy(Duration::from_secs(10)))
        .with_endpoints(TikTokEndpoints {
            authorize: format!("{address}/v2/auth/authorize/"),
            api: address.to_owned(),
        })
}

#[test]
fn a_full_sign_in_refresh_and_revoke_against_a_fake_tiktok() {
    let tiktok = Arc::new(TikTok::default());
    let address = serve(Arc::clone(&tiktok));
    let sign_in = sign_in(&address);
    let credentials = AppCredentials::parse(Network::TikTok, CLIENT_KEY, CLIENT_SECRET).unwrap();

    let callback = LoopbackReceiver.listen(sign_in.redirect_path()).unwrap();
    let redirect = callback.redirect_uri().to_owned();
    assert!(redirect.ends_with("/callback/"), "{redirect}");
    let request = sign_in.consent_request(&credentials, &redirect);
    assert!(
        request
            .url
            .starts_with(&format!("{address}/v2/auth/authorize/?"))
    );
    consent_in_browser(&request.url, &tiktok);
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

    let grant = sign_in
        .exchange(&credentials, &code, &request.verifier, &redirect)
        .unwrap();
    let tokens = TokenSet::granted(&grant, SystemTime::now());
    let creator = sign_in.identity(tokens.access_token()).unwrap();
    assert_eq!(creator.name, "Arquivos do Espaço");
    assert_eq!(creator.id, "723f24d7-e717-40f8-a2b6-cb8464cd23b4");

    let refreshed = sign_in
        .refresh(&credentials, &tokens, &creator)
        .map(|grant| tokens.refreshed(&grant, SystemTime::now()))
        .unwrap();
    assert_eq!(refreshed.access_token(), "act.fixture-access-token-0002");
    assert_eq!(
        refreshed.refresh_token(),
        Some("rft.fixture-refresh-token-0002")
    );
    // The old refresh token is spent: TikTok refuses it now.
    let spent = sign_in.refresh(&credentials, &tokens, &creator);
    assert_eq!(spent.unwrap_err().kind, SignInFailureKind::Refused);

    sign_in.revoke(Some(&credentials), &refreshed).unwrap();
    assert_eq!(
        *tiktok.revoked.lock().unwrap(),
        ["act.fixture-access-token-0002".to_owned()]
    );
}

#[test]
fn a_verifier_from_another_sign_in_is_refused() {
    let tiktok = Arc::new(TikTok::default());
    let address = serve(Arc::clone(&tiktok));
    let sign_in = sign_in(&address);
    let credentials = AppCredentials::parse(Network::TikTok, CLIENT_KEY, CLIENT_SECRET).unwrap();
    let redirect = "http://127.0.0.1:1/callback/";
    let ours = sign_in.consent_request(&credentials, redirect);
    let other = sign_in.consent_request(&credentials, redirect);
    let query = fields(ours.url.split_once('?').unwrap().1);
    *tiktok.challenge.lock().unwrap() = Some(field(&query, "code_challenge"));
    *tiktok.redirect.lock().unwrap() = Some(redirect.into());
    let failure = sign_in
        .exchange(
            &credentials,
            &bardo_domain::SecretText::new(CODE),
            &other.verifier,
            redirect,
        )
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Refused);
}

#[test]
fn wrong_app_credentials_are_rejected() {
    let tiktok = Arc::new(TikTok::default());
    let address = serve(Arc::clone(&tiktok));
    let wrong = AppCredentials::parse(
        Network::TikTok,
        concat!("awwrongserver", "key00001"),
        CLIENT_SECRET,
    )
    .unwrap();
    let failure = sign_in(&address)
        .exchange(
            &wrong,
            &bardo_domain::SecretText::new(CODE),
            &bardo_domain::SecretText::new("v"),
            "http://127.0.0.1:1/callback/",
        )
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::ClientRejected);
}
