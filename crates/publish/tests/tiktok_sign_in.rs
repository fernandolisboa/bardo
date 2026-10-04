//! TikTok sign-in against recorded responses (see
//! `fixtures/tiktok-oauth/README.md`). No test calls TikTok.

mod common;

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use bardo_ai::http::Method;
use bardo_domain::{
    AppCredentials, BrowserSignIn, ConnectedIdentity, Network, NetworkSignIn, SecretText,
    SignInFailureKind, TokenGrant, TokenSet,
};
use bardo_publish::TikTokSignIn;
use bardo_publish::oauth::{ChallengeEncoding, Pkce};
use common::{Scripted, fixture};

// Fake values, split so secret scanners do not take them for real ones.
const CLIENT_KEY: &str = concat!("awfixture", "key0000001");
const CLIENT_SECRET: &str = concat!("FixtureClientSecret", "0000000000000");
const REDIRECT: &str = "http://127.0.0.1:53682/callback/";
const OPEN_ID: &str = "723f24d7-e717-40f8-a2b6-cb8464cd23b4";

fn credentials() -> AppCredentials {
    AppCredentials::parse(Network::TikTok, CLIENT_KEY, CLIENT_SECRET).unwrap()
}

fn answers(names: &[&str]) -> TikTokSignIn<Scripted> {
    TikTokSignIn::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("tiktok-oauth", name))
            .collect(),
    ))
}

fn query(url: &str) -> HashMap<String, String> {
    let (_, query) = url.split_once('?').unwrap();
    form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

fn creator() -> ConnectedIdentity {
    ConnectedIdentity {
        id: OPEN_ID.into(),
        name: "Arquivos do Espaço".into(),
    }
}

/// Tokens granted `age` ago with TikTok's 24-hour access token.
fn tokens_aged(refresh: Option<&str>, age: Duration) -> TokenSet {
    TokenSet::granted(
        &TokenGrant {
            access_token: SecretText::new("act.fixture-access"),
            refresh_token: refresh.map(SecretText::new),
            expires_in: Duration::from_secs(86_400),
            scopes: Vec::new(),
        },
        SystemTime::now() - age,
    )
}

fn tokens(refresh: Option<&str>) -> TokenSet {
    tokens_aged(refresh, Duration::ZERO)
}

#[test]
fn tiktok_signs_in_in_the_browser_at_its_callback_path() {
    let sign_in = answers(&[]);
    assert_eq!(sign_in.network(), Network::TikTok);
    assert_eq!(
        sign_in.scopes(),
        ["user.info.basic", "video.upload", "video.list"]
    );
    assert!(sign_in.pasted().is_none());
    let browser = sign_in.browser().expect("a browser sign-in");
    assert_eq!(browser.redirect_path(), "/callback/");
    assert!(!sign_in.revokes_app_wide());
}

#[test]
fn the_consent_address_asks_every_scope_with_a_hex_pkce_challenge_and_state() {
    let sign_in = answers(&[]);
    let request = sign_in.consent_request(&credentials(), REDIRECT);
    assert!(
        request
            .url
            .starts_with("https://www.tiktok.com/v2/auth/authorize/?"),
        "{}",
        request.url
    );
    let query = query(&request.url);
    assert_eq!(query["client_key"], CLIENT_KEY);
    assert!(!query.contains_key("client_id"));
    assert_eq!(query["redirect_uri"], REDIRECT);
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["scope"], "user.info.basic,video.upload,video.list");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["state"], request.state.expose());
    // TikTok wants the SHA-256 in lowercase hex, not base64url.
    let challenge = &query["code_challenge"];
    assert_eq!(challenge.len(), 64);
    assert!(
        challenge
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "{challenge}"
    );
    assert_eq!(
        challenge,
        Pkce::from_verifier(request.verifier.expose(), ChallengeEncoding::Hex).challenge()
    );
    // The client secret never goes to the browser.
    assert!(!request.url.contains(CLIENT_SECRET));
}

#[test]
fn the_hex_challenge_matches_a_known_vector() {
    // SHA-256 of the RFC 7636 appendix B verifier, in lowercase hex.
    assert_eq!(
        Pkce::from_verifier(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            ChallengeEncoding::Hex
        )
        .challenge(),
        "13d31e961a1ad8ec2f16b10c4c982e0876a878ad6df144566ee1894acb70f9c3"
    );
    // And of a verifier written like TikTok's own example (letters,
    // digits and `-._~`).
    assert_eq!(
        Pkce::from_verifier(
            "Fixture-verifier_0123456789.abcdefghijklmnopqrstuvwxyz~ABCDEFGH",
            ChallengeEncoding::Hex
        )
        .challenge(),
        "17c2d1c561a2dd05d20b8d59b224997f6cdceea500186060b8d71c63533c634d"
    );
}

#[test]
fn every_consent_has_a_fresh_verifier_and_state() {
    let sign_in = answers(&[]);
    let a = sign_in.consent_request(&credentials(), REDIRECT);
    let b = sign_in.consent_request(&credentials(), REDIRECT);
    assert_ne!(a.verifier, b.verifier);
    assert_ne!(a.state, b.state);
}

#[test]
fn the_code_exchange_sends_the_secret_and_verifier_and_reads_comma_separated_scopes() {
    let sign_in = answers(&["token-exchange"]);
    let grant = sign_in
        .exchange(
            &credentials(),
            &SecretText::new("fixture-code*v!1"),
            &SecretText::new("fixture-verifier"),
            REDIRECT,
        )
        .unwrap();
    assert_eq!(grant.access_token.expose(), "act.fixture-access-token-0001");
    assert_eq!(
        grant.refresh_token.as_ref().map(SecretText::expose),
        Some("rft.fixture-refresh-token-0001")
    );
    assert_eq!(grant.expires_in, Duration::from_secs(86_400));
    assert_eq!(
        grant.scopes,
        ["user.info.basic", "video.upload", "video.list"]
    );

    let sent = sign_in.transport().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, Method::Post);
    assert_eq!(sent[0].url, "https://open.tiktokapis.com/v2/oauth/token/");
    assert_eq!(
        sent[0].header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    let form: HashMap<_, _> = sent[0].form().into_iter().collect();
    assert_eq!(form["client_key"], CLIENT_KEY);
    assert_eq!(form["client_secret"], CLIENT_SECRET);
    assert_eq!(form["code"], "fixture-code*v!1");
    assert_eq!(form["code_verifier"], "fixture-verifier");
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["redirect_uri"], REDIRECT);
    assert!(!form.contains_key("client_id"));
}

#[test]
fn a_missing_scope_reaches_the_app_which_refuses_the_connection() {
    let grant = answers(&["token-exchange-missing-scope"])
        .exchange(
            &credentials(),
            &SecretText::new("c"),
            &SecretText::new("v"),
            REDIRECT,
        )
        .unwrap();
    assert_eq!(grant.scopes, ["user.info.basic", "video.list"]);
}

#[test]
fn an_exchange_without_a_refresh_token_is_refused() {
    let failure = answers(&["token-exchange-no-refresh"])
        .exchange(
            &credentials(),
            &SecretText::new("c"),
            &SecretText::new("v"),
            REDIRECT,
        )
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Unexpected);
}

#[test]
fn tiktok_answers_oauth_errors_with_http_200() {
    let sign_in = answers(&[
        "token-code-expired",
        "token-invalid-client",
        "token-unavailable",
    ]);
    let exchange = || {
        sign_in.exchange(
            &credentials(),
            &SecretText::new("c"),
            &SecretText::new("v"),
            REDIRECT,
        )
    };
    let expired = exchange().unwrap_err();
    assert_eq!(expired.kind, SignInFailureKind::Refused);
    assert!(
        expired.detail.contains("Authorization code is expired."),
        "{}",
        expired.detail
    );
    assert_eq!(
        exchange().unwrap_err().kind,
        SignInFailureKind::ClientRejected
    );
    assert_eq!(exchange().unwrap_err().kind, SignInFailureKind::NetworkDown);
}

#[test]
fn a_failing_or_unreachable_token_endpoint_is_reported() {
    let failure = answers(&["server-error"])
        .exchange(
            &credentials(),
            &SecretText::new("c"),
            &SecretText::new("v"),
            REDIRECT,
        )
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::NetworkDown);
    let offline = TikTokSignIn::with_transport(Scripted::offline())
        .exchange(
            &credentials(),
            &SecretText::new("c"),
            &SecretText::new("v"),
            REDIRECT,
        )
        .unwrap_err();
    assert_eq!(offline.kind, SignInFailureKind::Unreachable);
}

#[test]
fn a_refresh_keeps_the_rotated_refresh_token() {
    let sign_in = answers(&["token-refresh-rotated"]);
    let before = tokens(Some("rft.fixture-refresh-token-0001"));
    let grant = sign_in
        .refresh(&credentials(), &before, &creator())
        .unwrap();
    let after = before.refreshed(&grant, SystemTime::now());
    assert_eq!(after.access_token(), "act.fixture-access-token-0002");
    assert_eq!(
        after.refresh_token(),
        Some("rft.fixture-refresh-token-0002")
    );

    let sent = sign_in.transport().sent();
    let form: HashMap<_, _> = sent[0].form().into_iter().collect();
    assert_eq!(sent[0].url, "https://open.tiktokapis.com/v2/oauth/token/");
    assert_eq!(form["client_key"], CLIENT_KEY);
    assert_eq!(form["client_secret"], CLIENT_SECRET);
    assert_eq!(form["grant_type"], "refresh_token");
    assert_eq!(form["refresh_token"], "rft.fixture-refresh-token-0001");
}

#[test]
fn a_refused_refresh_is_reported_as_refused() {
    let failure = answers(&["token-refresh-invalid-grant"])
        .refresh(&credentials(), &tokens(Some("rft.gone")), &creator())
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Refused);
    let none = answers(&[])
        .refresh(&credentials(), &tokens(None), &creator())
        .unwrap_err();
    assert_eq!(none.kind, SignInFailureKind::Refused);
}

#[test]
fn the_creator_is_read_from_user_info() {
    let sign_in = answers(&["user-info"]);
    assert_eq!(sign_in.identity("act.fixture-access").unwrap(), creator());
    let sent = sign_in.transport().sent();
    assert_eq!(sent[0].method, Method::Get);
    assert_eq!(
        sent[0].url,
        "https://open.tiktokapis.com/v2/user/info/?fields=open_id%2Cdisplay_name"
    );
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer act.fixture-access")
    );
}

#[test]
fn user_info_errors_are_classified() {
    let sign_in = answers(&[
        "user-info-invalid-token",
        "user-info-scope-not-authorized",
        "user-info-rate-limited",
        "server-error",
    ]);
    let invalid = sign_in.identity("act.gone").unwrap_err();
    assert_eq!(invalid.kind, SignInFailureKind::Refused);
    assert!(
        invalid.detail.contains("access_token_invalid"),
        "{}",
        invalid.detail
    );
    for kind in [
        SignInFailureKind::NotAllowed,
        SignInFailureKind::LimitReached,
        SignInFailureKind::NetworkDown,
    ] {
        assert_eq!(sign_in.identity("act.x").unwrap_err().kind, kind);
    }
}

#[test]
fn disconnecting_revokes_the_access_token_with_the_app_credentials() {
    let sign_in = answers(&["revoke-ok"]);
    sign_in
        .revoke(Some(&credentials()), &tokens(Some("rft.r")))
        .unwrap();
    let sent = sign_in.transport().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].url, "https://open.tiktokapis.com/v2/oauth/revoke/");
    let form: HashMap<_, _> = sent[0].form().into_iter().collect();
    assert_eq!(form["client_key"], CLIENT_KEY);
    assert_eq!(form["client_secret"], CLIENT_SECRET);
    assert_eq!(form["token"], "act.fixture-access");
}

#[test]
fn a_token_tiktok_no_longer_knows_counts_as_revoked() {
    answers(&["revoke-invalid-token"])
        .revoke(Some(&credentials()), &tokens(Some("rft.r")))
        .unwrap();
}

#[test]
fn an_expired_access_token_is_renewed_before_it_is_revoked() {
    let sign_in = answers(&["token-refresh-rotated", "revoke-ok"]);
    sign_in
        .revoke(
            Some(&credentials()),
            &tokens_aged(
                Some("rft.fixture-refresh-token-0001"),
                Duration::from_secs(2 * 86_400),
            ),
        )
        .unwrap();
    let sent = sign_in.transport().sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        sent[0].field("grant_type").as_deref(),
        Some("refresh_token")
    );
    assert_eq!(
        sent[1].field("token").as_deref(),
        Some("act.fixture-access-token-0002")
    );
}

#[test]
fn an_expired_grant_tiktok_refuses_to_renew_has_nothing_left_to_revoke() {
    let sign_in = answers(&["token-refresh-invalid-grant"]);
    sign_in
        .revoke(
            Some(&credentials()),
            &tokens_aged(Some("rft.gone"), Duration::from_secs(2 * 86_400)),
        )
        .unwrap();
    assert_eq!(sign_in.transport().sent().len(), 1);
}

#[test]
fn revoking_needs_the_app_credentials_and_a_reachable_tiktok() {
    let none = answers(&[])
        .revoke(None, &tokens(Some("rft.r")))
        .unwrap_err();
    assert_eq!(none.kind, SignInFailureKind::ClientRejected);
    let down = answers(&["server-error"])
        .revoke(Some(&credentials()), &tokens(Some("rft.r")))
        .unwrap_err();
    assert_eq!(down.kind, SignInFailureKind::NetworkDown);
    let refused = answers(&["token-invalid-client"])
        .revoke(Some(&credentials()), &tokens(Some("rft.r")))
        .unwrap_err();
    assert_eq!(refused.kind, SignInFailureKind::ClientRejected);
}

#[test]
fn no_failure_quotes_a_secret() {
    let sign_in = answers(&["token-invalid-client", "token-refresh-invalid-grant"]);
    let failures = [
        sign_in
            .exchange(
                &credentials(),
                &SecretText::new("fixture-code"),
                &SecretText::new("fixture-verifier"),
                REDIRECT,
            )
            .unwrap_err(),
        sign_in
            .refresh(
                &credentials(),
                &tokens(Some("rft.secret-refresh")),
                &creator(),
            )
            .unwrap_err(),
    ];
    for failure in failures {
        for secret in [
            CLIENT_SECRET,
            "fixture-code",
            "fixture-verifier",
            "rft.secret-refresh",
        ] {
            assert!(!failure.detail.contains(secret), "{}", failure.detail);
        }
    }
}
