//! Instagram sign-in against recorded responses (see
//! `fixtures/meta-graph/README.md`). No test calls Meta.

mod common;

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use bardo_ai::http::Method;
use bardo_domain::{
    AppCredentials, ConnectedIdentity, Network, NetworkSignIn, PastedTokenSignIn, SecretText,
    SignInFailureKind, TokenGrant, TokenSet,
};
use bardo_publish::InstagramSignIn;
use bardo_publish::instagram::RENEW_AHEAD;
use common::{Scripted, fixture};

// A Meta app id and secret in the published shapes (digits; 32 hex), split
// so secret scanners do not take them for real ones.
const APP_ID: &str = "1234567890123456";
const APP_SECRET: &str = concat!("0123456789abcdef", "0123456789abcdef");
const PASTED: &str = "fixture-short-lived-user-token";
const GRAPH: &str = "https://graph.facebook.com/v25.0";

fn credentials() -> AppCredentials {
    AppCredentials::parse(Network::InstagramReels, APP_ID, APP_SECRET).unwrap()
}

fn answers(names: &[&str]) -> InstagramSignIn<Scripted> {
    InstagramSignIn::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("meta-graph", name))
            .collect(),
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
        id: "17841405309211844".into(),
        name: "@arquivosdoespaco".into(),
    }
}

fn kept(user: Option<&str>) -> TokenSet {
    TokenSet::granted(
        &TokenGrant {
            access_token: SecretText::new("fixture-page-token-0001"),
            refresh_token: user.map(SecretText::new),
            expires_in: Duration::from_secs(5_183_944),
            scopes: Vec::new(),
        },
        SystemTime::now(),
    )
}

#[test]
fn every_scope_publishing_and_insights_need_is_checked() {
    let sign_in = answers(&[]);
    assert_eq!(sign_in.network(), Network::InstagramReels);
    assert_eq!(
        sign_in.scopes(),
        [
            "instagram_basic",
            "instagram_content_publish",
            "instagram_manage_insights",
            "pages_show_list",
            "pages_read_engagement"
        ]
    );
    assert!(sign_in.pasted().is_some());
    assert!(sign_in.browser().is_none());
    assert!(sign_in.revokes_app_wide());
    assert_eq!(sign_in.refresh_margin(), RENEW_AHEAD);
    assert_eq!(RENEW_AHEAD, Duration::from_secs(7 * 86_400));
}

#[test]
fn a_pasted_token_is_traded_for_a_long_lived_one_with_its_permissions() {
    let sign_in = answers(&["token-exchange", "permissions-all"]);
    let grant = sign_in
        .exchange_pasted(&credentials(), &SecretText::new(PASTED))
        .unwrap();
    assert_eq!(
        grant.access_token.expose(),
        "fixture-long-lived-user-token-0001"
    );
    assert_eq!(grant.refresh_token, None);
    assert_eq!(grant.expires_in, Duration::from_secs(5_183_944));
    // Declined or extra permissions are kept as Meta lists them granted.
    assert_eq!(
        grant.scopes,
        [
            "instagram_basic",
            "instagram_content_publish",
            "instagram_manage_insights",
            "pages_show_list",
            "pages_read_engagement",
            "business_management",
            "public_profile"
        ]
    );

    let sent = sign_in.transport().sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].method, Method::Get);
    assert!(
        sent[0]
            .url
            .starts_with(&format!("{GRAPH}/oauth/access_token?"))
    );
    let exchange = query(&sent[0].url);
    assert_eq!(exchange["grant_type"], "fb_exchange_token");
    assert_eq!(exchange["client_id"], APP_ID);
    assert_eq!(exchange["client_secret"], APP_SECRET);
    assert_eq!(exchange["fb_exchange_token"], PASTED);
    assert_eq!(sent[1].url, format!("{GRAPH}/me/permissions"));
    assert_eq!(
        sent[1].header("authorization"),
        Some("Bearer fixture-long-lived-user-token-0001")
    );
}

#[test]
fn a_declined_permission_is_not_listed() {
    let grant = answers(&["token-exchange", "permissions-declined"])
        .exchange_pasted(&credentials(), &SecretText::new(PASTED))
        .unwrap();
    assert!(
        !grant
            .scopes
            .iter()
            .any(|s| s == "instagram_content_publish")
    );
}

#[test]
fn a_long_lived_token_without_a_lifetime_is_taken_for_60_days() {
    let grant = answers(&["token-exchange-no-lifetime", "permissions-all"])
        .exchange_pasted(&credentials(), &SecretText::new(PASTED))
        .unwrap();
    assert_eq!(grant.expires_in, Duration::from_secs(60 * 86_400));
}

#[test]
fn rejected_app_credentials_and_tokens_are_told_apart() {
    let exchange = |name: &str| {
        answers(&[name])
            .exchange_pasted(&credentials(), &SecretText::new(PASTED))
            .unwrap_err()
    };
    let unknown_app = exchange("token-invalid-client");
    assert_eq!(unknown_app.kind, SignInFailureKind::ClientRejected);
    assert_eq!(unknown_app.detail, "HTTP 400: 101: Invalid Client ID");
    assert_eq!(
        exchange("token-bad-secret").kind,
        SignInFailureKind::ClientRejected
    );
    assert_eq!(exchange("token-expired").kind, SignInFailureKind::Refused);
    assert_eq!(
        exchange("rate-limited").kind,
        SignInFailureKind::LimitReached
    );
    assert_eq!(
        exchange("server-error").kind,
        SignInFailureKind::NetworkDown
    );
    let offline = InstagramSignIn::with_transport(Scripted::offline())
        .exchange_pasted(&credentials(), &SecretText::new(PASTED))
        .unwrap_err();
    assert_eq!(offline.kind, SignInFailureKind::Unreachable);
}

#[test]
fn discovery_lists_every_page_with_its_linked_account_and_token() {
    let sign_in = answers(&["accounts-several"]);
    let found = sign_in
        .discover("fixture-long-lived-user-token-0001")
        .unwrap();
    let summary: Vec<_> = found
        .iter()
        .map(|page| {
            (
                page.via.as_str(),
                page.identity.as_ref().map(|account| account.name.as_str()),
                page.token.expose(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                "Arquivos do Espaço",
                Some("@arquivosdoespaco"),
                "fixture-page-token-0001"
            ),
            (
                "Space Archives",
                Some("@spacearchives"),
                "fixture-page-token-0002"
            ),
            ("Padaria da Esquina", None, "fixture-page-token-0003"),
        ]
    );
    assert_eq!(found[0].identity.as_ref().unwrap().id, "17841405309211844");

    let sent = &sign_in.transport().sent()[0];
    assert!(sent.url.starts_with(&format!("{GRAPH}/me/accounts?")));
    let asked = query(&sent.url);
    assert_eq!(
        asked["fields"],
        "id,name,access_token,instagram_business_account{id,username}"
    );
    assert_eq!(asked["limit"], "100");
    assert!(!asked.contains_key("after"));
    assert_eq!(
        sent.header("authorization"),
        Some("Bearer fixture-long-lived-user-token-0001")
    );
}

#[test]
fn discovery_follows_the_next_page_and_skips_pages_without_a_token() {
    let sign_in = answers(&["accounts-first-of-two", "accounts-second-of-two"]);
    let found = sign_in
        .discover("fixture-long-lived-user-token-0001")
        .unwrap();
    let names: Vec<_> = found.iter().map(|page| page.via.as_str()).collect();
    assert_eq!(names, ["Arquivos do Espaço", "Space Archives"]);
    let sent = sign_in.transport().sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(query(&sent[1].url)["after"], "MTM0ODk1NzkzNzkxOTE3");
    // The token rides in the header, never the address.
    assert!(!sent[1].url.contains("fixture-long-lived"));
}

#[test]
fn discovery_with_no_page_or_no_linked_account_says_so() {
    assert!(
        answers(&["accounts-none"])
            .discover("fixture-user")
            .unwrap()
            .is_empty()
    );
    let found = answers(&["accounts-no-instagram"])
        .discover("fixture-user")
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].identity, None);
    let refused = answers(&["accounts-invalid-token"])
        .discover("fixture-user")
        .unwrap_err();
    assert_eq!(refused.kind, SignInFailureKind::Refused);
    assert_eq!(
        answers(&["permission-missing"])
            .discover("fixture-user")
            .unwrap_err()
            .kind,
        SignInFailureKind::NotAllowed
    );
}

#[test]
fn the_page_token_names_its_instagram_account() {
    let sign_in = answers(&["page-instagram"]);
    assert_eq!(
        sign_in.identity("fixture-page-token-0001").unwrap(),
        account()
    );
    let sent = &sign_in.transport().sent()[0];
    assert!(sent.url.starts_with(&format!("{GRAPH}/me?")));
    assert_eq!(
        query(&sent.url)["fields"],
        "instagram_business_account{id,username}"
    );
    assert_eq!(
        sent.header("authorization"),
        Some("Bearer fixture-page-token-0001")
    );
    let unlinked = answers(&["page-no-instagram"])
        .identity("fixture-page-token-0003")
        .unwrap_err();
    assert_eq!(unlinked.kind, SignInFailureKind::NoChannel);
}

#[test]
fn a_refresh_renews_the_user_token_and_reads_the_page_token_again() {
    let sign_in = answers(&["token-exchange", "accounts-refreshed"]);
    let tokens = kept(Some("fixture-long-lived-user-token-0000"));
    let grant = sign_in
        .refresh(&credentials(), &tokens, &account())
        .unwrap();
    assert_eq!(grant.access_token.expose(), "fixture-page-token-0009");
    assert_eq!(
        grant.refresh_token.as_ref().map(SecretText::expose),
        Some("fixture-long-lived-user-token-0001")
    );
    assert_eq!(grant.expires_in, Duration::from_secs(5_183_944));
    let refreshed = tokens.refreshed(&grant, SystemTime::now());
    assert_eq!(refreshed.access_token(), "fixture-page-token-0009");
    assert_eq!(
        refreshed.refresh_token(),
        Some("fixture-long-lived-user-token-0001")
    );

    let sent = sign_in.transport().sent();
    assert_eq!(
        query(&sent[0].url)["fb_exchange_token"],
        "fixture-long-lived-user-token-0000"
    );
    assert_eq!(
        sent[1].header("authorization"),
        Some("Bearer fixture-long-lived-user-token-0001")
    );
}

#[test]
fn a_refresh_is_refused_when_the_token_expired_or_the_account_left_the_pages() {
    let expired = answers(&["token-expired"])
        .refresh(
            &credentials(),
            &kept(Some("fixture-user-token-old")),
            &account(),
        )
        .unwrap_err();
    assert_eq!(expired.kind, SignInFailureKind::Refused);
    let gone = answers(&["token-exchange", "accounts-no-instagram"])
        .refresh(
            &credentials(),
            &kept(Some("fixture-user-token-old")),
            &account(),
        )
        .unwrap_err();
    assert_eq!(gone.kind, SignInFailureKind::Refused);
    let without_user_token = answers(&[])
        .refresh(&credentials(), &kept(None), &account())
        .unwrap_err();
    assert_eq!(without_user_token.kind, SignInFailureKind::Refused);
}

#[test]
fn a_refresh_meta_cannot_answer_now_keeps_the_connection() {
    let failure = answers(&["server-error"])
        .refresh(&credentials(), &kept(Some("fixture-user")), &account())
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::NetworkDown);
}

#[test]
fn revoking_removes_the_apps_permissions_with_the_user_token() {
    let sign_in = answers(&["revoke-ok"]);
    sign_in
        .revoke(None, &kept(Some("fixture-long-lived-user-token-0001")))
        .unwrap();
    let sent = &sign_in.transport().sent()[0];
    assert_eq!(sent.method, Method::Delete);
    assert_eq!(sent.url, format!("{GRAPH}/me/permissions"));
    assert_eq!(
        sent.header("authorization"),
        Some("Bearer fixture-long-lived-user-token-0001")
    );
    // A token Meta no longer reads has nothing left to revoke.
    answers(&["revoke-invalid-token"])
        .revoke(None, &kept(Some("fixture-user")))
        .unwrap();
    assert_eq!(
        answers(&["server-error"])
            .revoke(None, &kept(Some("fixture-user")))
            .unwrap_err()
            .kind,
        SignInFailureKind::NetworkDown
    );
}
