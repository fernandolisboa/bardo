//! X sign-in against recorded responses (see `fixtures/x-oauth/README.md`).
//! No test calls X.

mod common;

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use bardo_ai::http::Method;
use bardo_domain::{
    AppCredentials, BrowserSignIn, ConnectedIdentity, Network, NetworkSignIn, SecretText,
    SignInFailureKind, TokenGrant, TokenSet,
};
use bardo_publish::XSignIn;
use bardo_publish::oauth::{ChallengeEncoding, Pkce};
use bardo_publish::x::{CALLBACK_PATH, CALLBACK_PORT, CALLBACK_URL};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use common::{Scripted, fixture};

// Fake values, split so secret scanners do not take them for real ones.
const CLIENT_ID: &str = concat!("Rml4dHVyZUNsaWVudElk", "MDAwMDAxOjE6Y2k");
const CLIENT_SECRET: &str = concat!("Fixture-X-client-secret_", "0000000000000000000000000");
const USER_ID: &str = "1840000000000000001";

fn credentials() -> AppCredentials {
    AppCredentials::parse(Network::X, CLIENT_ID, CLIENT_SECRET).unwrap()
}

fn answers(names: &[&str]) -> XSignIn<Scripted> {
    XSignIn::with_transport(Scripted::new(
        names.iter().map(|name| fixture("x-oauth", name)).collect(),
    ))
}

fn query(url: &str) -> HashMap<String, String> {
    let (_, query) = url.split_once('?').unwrap();
    form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

fn account() -> ConnectedIdentity {
    ConnectedIdentity {
        id: USER_ID.into(),
        name: "@arquivosdoespaco".into(),
    }
}

fn tokens(refresh: Option<&str>) -> TokenSet {
    TokenSet::granted(
        &TokenGrant {
            access_token: SecretText::new("fixture-access-token"),
            refresh_token: refresh.map(SecretText::new),
            expires_in: Duration::from_secs(7_200),
            scopes: Vec::new(),
        },
        SystemTime::now(),
    )
}

/// The app's credentials as X's token endpoint takes them.
fn basic() -> String {
    format!(
        "Basic {}",
        STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"))
    )
}

#[test]
fn x_signs_in_in_the_browser_at_a_fixed_callback() {
    let sign_in = answers(&[]);
    assert_eq!(sign_in.network(), Network::X);
    assert_eq!(
        sign_in.scopes(),
        [
            "tweet.read",
            "tweet.write",
            "users.read",
            "media.write",
            "offline.access"
        ]
    );
    assert!(sign_in.pasted().is_none());
    assert!(!sign_in.revokes_app_wide());
    let browser = sign_in.browser().expect("a browser sign-in");
    assert_eq!(browser.redirect_path(), "/callback");
    assert_eq!(browser.redirect_port(), Some(47147));
    // What the guide tells the user to register is what Bardo listens on.
    assert_eq!(
        CALLBACK_URL,
        format!("http://127.0.0.1:{CALLBACK_PORT}{CALLBACK_PATH}")
    );
}

#[test]
fn the_consent_address_asks_every_scope_with_an_s256_challenge_and_state() {
    let sign_in = answers(&[]);
    let request = sign_in.consent_request(&credentials(), CALLBACK_URL);
    assert!(
        request.url.starts_with("https://x.com/i/oauth2/authorize?"),
        "{}",
        request.url
    );
    let query = query(&request.url);
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["client_id"], CLIENT_ID);
    assert_eq!(query["redirect_uri"], CALLBACK_URL);
    assert_eq!(
        query["scope"],
        "tweet.read tweet.write users.read media.write offline.access"
    );
    assert_eq!(query["state"], request.state.expose());
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(
        query["code_challenge"],
        Pkce::from_verifier(request.verifier.expose(), ChallengeEncoding::Base64Url).challenge()
    );
    // The client secret never goes to the browser.
    assert!(!request.url.contains(CLIENT_SECRET));
}

#[test]
fn the_code_exchange_authenticates_the_app_and_keeps_the_refresh_token() {
    let sign_in = answers(&["token-exchange"]);
    let grant = sign_in
        .exchange(
            &credentials(),
            &SecretText::new("fixture-code"),
            &SecretText::new("fixture-verifier-0123456789-0123456789-0123456789"),
            CALLBACK_URL,
        )
        .unwrap();
    assert_eq!(
        grant.access_token.expose(),
        "Rml4dHVyZUFjY2Vzc1Rva2VuMDAwMQ:1:1:at:1"
    );
    assert_eq!(
        grant.refresh_token.unwrap().expose(),
        "Rml4dHVyZVJlZnJlc2hUb2tlbjAwMDE:1:1:rt:1"
    );
    assert_eq!(grant.expires_in, Duration::from_secs(7_200));
    assert_eq!(
        grant.scopes,
        [
            "tweet.read",
            "tweet.write",
            "users.read",
            "media.write",
            "offline.access"
        ]
    );
    let sent = &sign_in.transport().sent()[0];
    assert_eq!(sent.method, Method::Post);
    assert_eq!(sent.url, "https://api.x.com/2/oauth2/token");
    assert_eq!(sent.header("authorization"), Some(basic().as_str()));
    assert_eq!(
        sent.field("grant_type").as_deref(),
        Some("authorization_code")
    );
    assert_eq!(sent.field("code").as_deref(), Some("fixture-code"));
    assert_eq!(sent.field("redirect_uri").as_deref(), Some(CALLBACK_URL));
    assert_eq!(
        sent.field("code_verifier").as_deref(),
        Some("fixture-verifier-0123456789-0123456789-0123456789")
    );
    // The secret goes in the header only.
    assert!(!sent.body.contains(CLIENT_SECRET));
}

#[test]
fn an_exchange_without_a_refresh_token_is_refused() {
    let sign_in = answers(&["token-exchange-no-refresh"]);
    let failure = sign_in
        .exchange(
            &credentials(),
            &SecretText::new("fixture-code"),
            &SecretText::new("fixture-verifier"),
            CALLBACK_URL,
        )
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Unexpected);
}

#[test]
fn a_code_x_no_longer_takes_is_refused_and_the_granted_scopes_are_reported() {
    let exchange = |name: &str| {
        answers(&[name]).exchange(
            &credentials(),
            &SecretText::new("fixture-code"),
            &SecretText::new("fixture-verifier"),
            CALLBACK_URL,
        )
    };
    // The code lasts 30 seconds: one used late is refused, to sign in again.
    assert_eq!(
        exchange("token-code-invalid").unwrap_err().kind,
        SignInFailureKind::Refused
    );
    // A grant short of a scope says so, and the connection names what is
    // missing.
    assert!(
        !exchange("token-exchange-missing-scope")
            .unwrap()
            .scopes
            .iter()
            .any(|scope| scope == "media.write")
    );
}

#[test]
fn a_refresh_trades_the_refresh_token_for_a_new_one() {
    let sign_in = answers(&["token-refresh-rotated"]);
    let grant = sign_in
        .refresh(
            &credentials(),
            &tokens(Some("fixture-refresh-token")),
            &account(),
        )
        .unwrap();
    assert_eq!(
        grant.refresh_token.unwrap().expose(),
        "Rml4dHVyZVJlZnJlc2hUb2tlbjAwMDI:1:1:rt:1"
    );
    let sent = &sign_in.transport().sent()[0];
    assert_eq!(sent.header("authorization"), Some(basic().as_str()));
    assert_eq!(sent.field("grant_type").as_deref(), Some("refresh_token"));
    assert_eq!(
        sent.field("refresh_token").as_deref(),
        Some("fixture-refresh-token")
    );
}

#[test]
fn oauth_errors_say_what_to_do() {
    let refresh = |name: &str| {
        answers(&[name])
            .refresh(&credentials(), &tokens(Some("rt")), &account())
            .unwrap_err()
    };
    // A refresh token X no longer takes: reconnect.
    let refused = refresh("token-refresh-invalid-token");
    assert_eq!(refused.kind, SignInFailureKind::Refused);
    assert_eq!(
        refused.detail,
        "HTTP 400: invalid_request: Value passed for the token was invalid."
    );
    // Recorded with a client id X does not know.
    assert_eq!(
        refresh("token-invalid-client").kind,
        SignInFailureKind::ClientRejected
    );
    assert_eq!(refresh("server-error").kind, SignInFailureKind::NetworkDown);
    assert_eq!(
        answers(&[])
            .refresh(&credentials(), &tokens(None), &account())
            .unwrap_err()
            .kind,
        SignInFailureKind::Refused
    );
}

#[test]
fn the_account_is_the_user_id_and_username() {
    let sign_in = answers(&["users-me"]);
    assert_eq!(
        sign_in.identity("fixture-access-token").unwrap(),
        ConnectedIdentity {
            id: USER_ID.into(),
            name: "@arquivosdoespaco".into(),
        }
    );
    let sent = &sign_in.transport().sent()[0];
    assert_eq!(sent.method, Method::Get);
    assert_eq!(sent.url, "https://api.x.com/2/users/me");
    assert_eq!(
        sent.header("authorization"),
        Some("Bearer fixture-access-token")
    );
}

#[test]
fn account_errors_tell_a_dead_token_from_spent_credits() {
    let identity = |name: &str| answers(&[name]).identity("token").unwrap_err();
    assert_eq!(
        identity("users-me-unauthorized").kind,
        SignInFailureKind::Refused
    );
    // Recorded with a made-up token, which X takes for an app's.
    let not_a_user = identity("users-me-not-a-user-token");
    assert_eq!(not_a_user.kind, SignInFailureKind::Refused);
    assert!(
        not_a_user
            .detail
            .starts_with("HTTP 403: Unsupported Authentication: Authenticating with OAuth 2.0"),
        "{}",
        not_a_user.detail
    );
    assert_eq!(
        identity("users-me-credits-depleted").kind,
        SignInFailureKind::LimitReached
    );
}

#[test]
fn revoking_ends_the_refresh_token_then_the_access_token_with_their_hints() {
    let sign_in = answers(&["revoke-ok", "revoke-ok"]);
    sign_in
        .revoke(Some(&credentials()), &tokens(Some("fixture-refresh-token")))
        .unwrap();
    let sent = sign_in.transport().sent();
    assert_eq!(sent.len(), 2);
    for request in &sent {
        assert_eq!(request.url, "https://api.x.com/2/oauth2/revoke");
        assert_eq!(request.header("authorization"), Some(basic().as_str()));
    }
    assert_eq!(
        sent[0].field("token").as_deref(),
        Some("fixture-refresh-token")
    );
    assert_eq!(
        sent[0].field("token_type_hint").as_deref(),
        Some("refresh_token")
    );
    assert_eq!(
        sent[1].field("token").as_deref(),
        Some("fixture-access-token")
    );
    assert_eq!(
        sent[1].field("token_type_hint").as_deref(),
        Some("access_token")
    );
}

#[test]
fn revoking_needs_the_app_credentials() {
    let sign_in = answers(&[]);
    assert_eq!(
        sign_in.revoke(None, &tokens(Some("rt"))).unwrap_err().kind,
        SignInFailureKind::ClientRejected
    );
    assert!(sign_in.transport().sent().is_empty());
    // X answers a request without the hint as malformed (recorded).
    let failure = answers(&["revoke-missing-hint"])
        .revoke(Some(&credentials()), &tokens(None))
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Unexpected);
}
