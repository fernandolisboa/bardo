//! YouTube sign-in against recorded responses (see
//! `fixtures/google-oauth/README.md`). No test calls Google.

mod common;

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use bardo_ai::http::Method;
use bardo_domain::{
    AppCredentials, BrowserSignIn, ConnectedIdentity, Network, NetworkSignIn, SecretText,
    SignInFailureKind, TokenGrant, TokenSet,
};
use bardo_publish::YouTubeSignIn;
use bardo_publish::oauth::{ChallengeEncoding, Pkce};
use common::{Scripted, fixture};

// Fake values, split so secret scanners do not take them for real ones.
const CLIENT_ID: &str = concat!("1234567890-abc123def456", ".apps.googleusercontent.com");
const CLIENT_SECRET: &str = concat!("GOCSPX", "-fixture-client-secret");
const REDIRECT: &str = "http://127.0.0.1:53682";

fn credentials() -> AppCredentials {
    AppCredentials::parse(Network::YouTube, CLIENT_ID, CLIENT_SECRET).unwrap()
}

fn answers(names: &[&str]) -> YouTubeSignIn<Scripted> {
    YouTubeSignIn::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("google-oauth", name))
            .collect(),
    ))
}

fn query(url: &str) -> HashMap<String, String> {
    let (_, query) = url.split_once('?').unwrap();
    form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

fn channel() -> ConnectedIdentity {
    ConnectedIdentity {
        id: "UCabcdefghijklmnopqrstuv".into(),
        name: "Arquivos do Espaço".into(),
    }
}

fn tokens(refresh: Option<&str>) -> TokenSet {
    TokenSet::granted(
        &TokenGrant {
            access_token: SecretText::new("ya29.fixture-access"),
            refresh_token: refresh.map(SecretText::new),
            expires_in: Duration::from_secs(3599),
            scopes: Vec::new(),
        },
        SystemTime::now(),
    )
}

#[test]
fn the_consent_address_asks_every_scope_with_pkce_and_state() {
    let sign_in = answers(&[]);
    let request = sign_in.consent_request(&credentials(), REDIRECT);
    assert!(
        request
            .url
            .starts_with("https://accounts.google.com/o/oauth2/v2/auth?")
    );
    let query = query(&request.url);
    assert_eq!(query["client_id"], CLIENT_ID);
    assert_eq!(query["redirect_uri"], REDIRECT);
    assert_eq!(query["response_type"], "code");
    assert_eq!(
        query["scope"],
        "https://www.googleapis.com/auth/youtube.upload \
         https://www.googleapis.com/auth/youtube \
         https://www.googleapis.com/auth/yt-analytics.readonly \
         https://www.googleapis.com/auth/yt-analytics-monetary.readonly"
    );
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(
        query["code_challenge"],
        Pkce::from_verifier(request.verifier.expose(), ChallengeEncoding::Base64Url).challenge()
    );
    assert_eq!(query["state"], request.state.expose());
    // The secret never travels to the browser.
    assert!(!request.url.contains(CLIENT_SECRET));
    assert!(!request.url.contains(request.verifier.expose()));
    assert_eq!(sign_in.scopes().len(), 4);
}

#[test]
fn each_consent_gets_its_own_verifier_and_state() {
    let sign_in = answers(&[]);
    let a = sign_in.consent_request(&credentials(), REDIRECT);
    let b = sign_in.consent_request(&credentials(), REDIRECT);
    assert_ne!(a.state, b.state);
    assert_ne!(a.verifier, b.verifier);
}

#[test]
fn the_code_exchange_proves_the_verifier_and_reads_the_grant() {
    let sign_in = answers(&["token-exchange"]);
    let grant = sign_in
        .exchange(
            &credentials(),
            &SecretText::new("4/0AeaY-fixture-code"),
            &SecretText::new("verifier-0001-abcdefghijklmnopqrstuvwxyz0123456789"),
            REDIRECT,
        )
        .unwrap();
    assert_eq!(
        grant.access_token.expose(),
        "ya29.a0AfB_byC-fixture-access-token-0001"
    );
    assert_eq!(
        grant.refresh_token.as_ref().map(SecretText::expose),
        Some("1//0gFixture-refresh-token-0001")
    );
    assert_eq!(grant.expires_in, Duration::from_secs(3599));
    assert_eq!(grant.scopes.len(), 4);

    let sent = sign_in.transport().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, Method::Post);
    assert_eq!(sent[0].url, "https://oauth2.googleapis.com/token");
    assert_eq!(
        sent[0].header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        sent[0].field("grant_type").as_deref(),
        Some("authorization_code")
    );
    assert_eq!(
        sent[0].field("code").as_deref(),
        Some("4/0AeaY-fixture-code")
    );
    assert_eq!(
        sent[0].field("code_verifier").as_deref(),
        Some("verifier-0001-abcdefghijklmnopqrstuvwxyz0123456789")
    );
    assert_eq!(sent[0].field("redirect_uri").as_deref(), Some(REDIRECT));
    assert_eq!(sent[0].field("client_id").as_deref(), Some(CLIENT_ID));
    assert_eq!(
        sent[0].field("client_secret").as_deref(),
        Some(CLIENT_SECRET)
    );
    // Secrets travel in the body, never the address.
    assert!(!sent[0].url.contains('?'));
}

#[test]
fn an_exchange_without_a_refresh_token_is_refused() {
    let failure = answers(&["token-exchange-no-refresh"])
        .exchange(
            &credentials(),
            &SecretText::new("code"),
            &SecretText::new("verifier"),
            REDIRECT,
        )
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Unexpected);
}

#[test]
fn a_refresh_sends_the_refresh_token_and_keeps_no_new_one() {
    let sign_in = answers(&["token-refresh"]);
    let grant = sign_in
        .refresh(
            &credentials(),
            &tokens(Some("1//0gFixture-refresh-token-0001")),
            &channel(),
        )
        .unwrap();
    assert_eq!(
        grant.access_token.expose(),
        "ya29.a0AfB_byC-fixture-access-token-0002"
    );
    assert_eq!(grant.refresh_token, None);
    let sent = &sign_in.transport().sent()[0];
    assert_eq!(sent.field("grant_type").as_deref(), Some("refresh_token"));
    assert_eq!(
        sent.field("refresh_token").as_deref(),
        Some("1//0gFixture-refresh-token-0001")
    );
    assert_eq!(sent.field("client_secret").as_deref(), Some(CLIENT_SECRET));
}

#[test]
fn token_endpoint_failures_are_classified() {
    let refresh = |name: &str| {
        answers(&[name])
            .refresh(&credentials(), &tokens(Some("1//refresh")), &channel())
            .unwrap_err()
    };
    let refused = refresh("token-invalid-grant");
    assert_eq!(refused.kind, SignInFailureKind::Refused);
    assert_eq!(
        refused.detail,
        "HTTP 400: invalid_grant: Token has been expired or revoked."
    );
    assert_eq!(
        refresh("token-invalid-client").kind,
        SignInFailureKind::ClientRejected
    );
    assert_eq!(
        refresh("token-server-error").kind,
        SignInFailureKind::NetworkDown
    );
    let offline = YouTubeSignIn::with_transport(Scripted::offline())
        .refresh(&credentials(), &tokens(Some("1//refresh")), &channel())
        .unwrap_err();
    assert_eq!(offline.kind, SignInFailureKind::Unreachable);
}

#[test]
fn revoking_sends_the_refresh_token() {
    let sign_in = answers(&["revoke-ok"]);
    sign_in
        .revoke(&tokens(Some("1//0gRefresh-to-revoke")))
        .unwrap();
    let sent = &sign_in.transport().sent()[0];
    assert_eq!(sent.url, "https://oauth2.googleapis.com/revoke");
    assert_eq!(sent.method, Method::Post);
    assert_eq!(
        sent.field("token").as_deref(),
        Some("1//0gRefresh-to-revoke")
    );
}

#[test]
fn without_a_refresh_token_the_access_token_is_revoked() {
    let sign_in = answers(&["revoke-ok"]);
    sign_in.revoke(&tokens(None)).unwrap();
    assert_eq!(
        sign_in.transport().sent()[0].field("token").as_deref(),
        Some("ya29.fixture-access")
    );
}

#[test]
fn a_token_already_revoked_counts_as_revoked() {
    answers(&["revoke-invalid-token"])
        .revoke(&tokens(Some("1//gone")))
        .unwrap();
    let down = answers(&["token-server-error"])
        .revoke(&tokens(Some("1//r")))
        .unwrap_err();
    assert_eq!(down.kind, SignInFailureKind::NetworkDown);
}

#[test]
fn the_connected_channel_comes_from_channels_list_mine() {
    let sign_in = answers(&["channels-mine"]);
    let identity = sign_in.identity("ya29.fixture-access").unwrap();
    assert_eq!(
        identity,
        ConnectedIdentity {
            id: "UCx9Fixture-Channel-Id01".into(),
            name: "Arquivos do Espaço".into(),
        }
    );
    let sent = &sign_in.transport().sent()[0];
    assert!(
        sent.url
            .starts_with("https://www.googleapis.com/youtube/v3/channels?")
    );
    let query = query(&sent.url);
    assert_eq!(query["part"], "snippet");
    assert_eq!(query["mine"], "true");
    assert_eq!(query["fields"], "items(id,snippet/title)");
    assert_eq!(
        sent.header("authorization"),
        Some("Bearer ya29.fixture-access")
    );
    assert!(!sent.url.contains("ya29"));
}

#[test]
fn channel_lookup_failures_are_classified() {
    let identity = |name: &str| answers(&[name]).identity("ya29.x").unwrap_err().kind;
    assert_eq!(identity("channels-none"), SignInFailureKind::NoChannel);
    assert_eq!(
        identity("channels-api-disabled"),
        SignInFailureKind::NotAllowed
    );
    assert_eq!(identity("channels-quota"), SignInFailureKind::LimitReached);
    assert_eq!(
        identity("channels-invalid-token"),
        SignInFailureKind::Refused
    );
}
