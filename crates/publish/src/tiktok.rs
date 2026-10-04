//! TikTok sign-in (ADR-0008): Login Kit for Desktop through the user's own
//! TikTok app or sandbox. Authorization code with PKCE over a loopback
//! redirect, like YouTube, with TikTok's own rules:
//!
//! - the app is named by `client_key`, not `client_id`, and scopes are
//!   comma-separated;
//! - the PKCE challenge is the SHA-256 of the verifier in **hex**;
//! - the redirect must be registered with its path: Bardo answers on
//!   `http://127.0.0.1:<port>/callback/`, registered as
//!   `http://127.0.0.1:*/callback/` (TikTok takes a wildcard port);
//! - the token endpoint wants the client secret, the access token lasts
//!   24 hours and the refresh token 365 days, and a refresh may return a
//!   new refresh token, which replaces the old one;
//! - the OAuth endpoints answer errors with HTTP 200 and an `error` field
//!   (recorded 2026-10-04), so the body decides, not the status.
//!
//! The connected creator is the `open_id` and display name from
//! `user/info`.

use std::time::{Duration, SystemTime};

use bardo_ai::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use bardo_domain::{
    AppCredentials, BrowserSignIn, ConnectedIdentity, ConsentRequest, Network, NetworkSignIn,
    SecretText, SignInFailure, SignInFailureKind, TokenGrant, TokenSet,
};
use serde_json::Value;

use crate::oauth::{ChallengeEncoding, Pkce, new_state, parse_grant};

/// The creator's `open_id`, display name and avatar.
pub const SCOPE_USER_INFO: &str = "user.info.basic";
/// Upload a video to the creator's inbox as a draft (#84).
pub const SCOPE_UPLOAD: &str = "video.upload";
/// List the creator's public videos with their counts (owner metrics).
pub const SCOPE_VIDEO_LIST: &str = "video.list";

const SCOPES: &[&str] = &[SCOPE_USER_INFO, SCOPE_UPLOAD, SCOPE_VIDEO_LIST];

/// The path TikTok sends the browser back to; the user registers
/// `http://127.0.0.1:*/callback/` in Login Kit's desktop settings.
pub const CALLBACK_PATH: &str = "/callback/";

/// Where TikTok's endpoints live. Tests point them at a local fake server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TikTokEndpoints {
    /// The consent page.
    pub authorize: String,
    /// The Open API's base address: OAuth tokens, user info, posting.
    pub api: String,
}

impl Default for TikTokEndpoints {
    fn default() -> Self {
        Self {
            authorize: "https://www.tiktok.com/v2/auth/authorize/".into(),
            api: "https://open.tiktokapis.com".into(),
        }
    }
}

/// Signs in to TikTok over HTTPS.
pub struct TikTokSignIn<T = UreqTransport> {
    transport: T,
    endpoints: TikTokEndpoints,
}

impl TikTokSignIn {
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for TikTokSignIn {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> TikTokSignIn<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: TikTokEndpoints::default(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: TikTokEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, SignInFailure> {
        self.transport
            .send(request)
            .map_err(|error| SignInFailure::new(SignInFailureKind::Unreachable, error.0))
    }

    /// A call to an OAuth endpoint (`/v2/oauth/<path>/`). An answer with
    /// an `error` is a failure, whatever its status.
    fn oauth(&self, path: &str, fields: &[(&str, &str)]) -> Result<HttpResponse, SignInFailure> {
        let url = format!("{}/v2/oauth/{path}/", self.endpoints.api);
        let response = self.send(&HttpRequest::post_form(url, fields))?;
        if !(200..=299).contains(&response.status) || oauth_error(&response).is_some() {
            return Err(oauth_failure(&response));
        }
        Ok(response)
    }

    fn token(&self, fields: &[(&str, &str)]) -> Result<TokenGrant, SignInFailure> {
        parse_grant(&self.oauth("token", fields)?.body)
    }

    fn renew(
        &self,
        credentials: &AppCredentials,
        refresh_token: &str,
    ) -> Result<TokenGrant, SignInFailure> {
        self.token(&[
            ("client_key", credentials.client_id()),
            ("client_secret", credentials.client_secret()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
    }
}

impl<T: Transport> NetworkSignIn for TikTokSignIn<T> {
    fn network(&self) -> Network {
        Network::TikTok
    }

    fn scopes(&self) -> &'static [&'static str] {
        SCOPES
    }

    fn browser(&self) -> Option<&dyn BrowserSignIn> {
        Some(self)
    }

    fn refresh(
        &self,
        credentials: &AppCredentials,
        tokens: &TokenSet,
        _identity: &ConnectedIdentity,
    ) -> Result<TokenGrant, SignInFailure> {
        let refresh_token = tokens
            .refresh_token()
            .ok_or_else(|| SignInFailure::new(SignInFailureKind::Refused, "no refresh token"))?;
        self.renew(credentials, refresh_token)
    }

    /// TikTok revokes by access token, and refuses an expired one as
    /// unknown, which would leave the grant alive: an access token past
    /// its 24 hours is renewed first. A refresh token TikTok refuses means
    /// the grant is already gone.
    fn revoke(
        &self,
        credentials: Option<&AppCredentials>,
        tokens: &TokenSet,
    ) -> Result<(), SignInFailure> {
        let credentials = credentials.ok_or_else(|| {
            SignInFailure::new(
                SignInFailureKind::ClientRejected,
                "revoking needs the app credentials, which are not saved",
            )
        })?;
        let renewed;
        let access_token = match tokens.refresh_token() {
            Some(refresh_token) if tokens.needs_refresh(SystemTime::now()) => {
                match self.renew(credentials, refresh_token) {
                    Ok(grant) => {
                        renewed = grant.access_token;
                        renewed.expose()
                    }
                    Err(failure) if failure.kind == SignInFailureKind::Refused => return Ok(()),
                    Err(failure) => return Err(failure),
                }
            }
            _ => tokens.access_token(),
        };
        match self.oauth(
            "revoke",
            &[
                ("client_key", credentials.client_id()),
                ("client_secret", credentials.client_secret()),
                ("token", access_token),
            ],
        ) {
            Ok(_) => Ok(()),
            // Already revoked or expired: nothing is left to revoke.
            Err(failure) if failure.kind == SignInFailureKind::Refused => Ok(()),
            Err(failure) => Err(failure),
        }
    }

    fn identity(&self, access_token: &str) -> Result<ConnectedIdentity, SignInFailure> {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("fields", "open_id,display_name")
            .finish();
        let request = HttpRequest::get(format!("{}/v2/user/info/?{query}", self.endpoints.api))
            .header("authorization", format!("Bearer {access_token}"));
        let response = self.send(&request)?;
        let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
        let code = body["error"]["code"].as_str().unwrap_or_default();
        if !(200..=299).contains(&response.status) || !matches!(code, "" | "ok") {
            return Err(api_failure(&response, &body));
        }
        let user = &body["data"]["user"];
        let Some(open_id) = user["open_id"].as_str().filter(|id| !id.is_empty()) else {
            return Err(unexpected("the user info has no open_id"));
        };
        // Every TikTok account has a display name; should one come back
        // empty, the card still names the account by its id.
        let name = user["display_name"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(open_id);
        Ok(ConnectedIdentity {
            id: open_id.to_owned(),
            name: name.to_owned(),
        })
    }
}

impl<T: Transport> BrowserSignIn for TikTokSignIn<T> {
    fn redirect_path(&self) -> &'static str {
        CALLBACK_PATH
    }

    fn consent_request(&self, credentials: &AppCredentials, redirect_uri: &str) -> ConsentRequest {
        let pkce = Pkce::generate(ChallengeEncoding::Hex);
        let state = new_state();
        let scope = SCOPES.join(",");
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("client_key", credentials.client_id()),
                ("scope", scope.as_str()),
                ("redirect_uri", redirect_uri),
                ("state", state.expose()),
                ("response_type", "code"),
                ("code_challenge", pkce.challenge()),
                ("code_challenge_method", Pkce::METHOD),
            ])
            .finish();
        ConsentRequest {
            url: format!("{}?{query}", self.endpoints.authorize),
            state,
            verifier: pkce.into_verifier(),
        }
    }

    fn exchange(
        &self,
        credentials: &AppCredentials,
        code: &SecretText,
        verifier: &SecretText,
        redirect_uri: &str,
    ) -> Result<TokenGrant, SignInFailure> {
        let grant = self.token(&[
            ("client_key", credentials.client_id()),
            ("client_secret", credentials.client_secret()),
            ("code", code.expose()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier.expose()),
        ])?;
        if grant.refresh_token.is_none() {
            // Without it the connection would die with the access token.
            return Err(unexpected("the token answer has no refresh token"));
        }
        Ok(grant)
    }
}

fn unexpected(detail: &str) -> SignInFailure {
    SignInFailure::new(SignInFailureKind::Unexpected, detail)
}

/// The `error` code of an OAuth answer, when it carries one.
fn oauth_error(response: &HttpResponse) -> Option<String> {
    let body: Value = serde_json::from_str(&response.body).ok()?;
    body["error"]
        .as_str()
        .filter(|code| !code.is_empty())
        .map(str::to_owned)
}

/// Classifies an OAuth endpoint's error answer (TikTok's codes follow RFC
/// 6749). The detail keeps TikTok's message and `log_id`, never a field
/// Bardo sent.
fn oauth_failure(response: &HttpResponse) -> SignInFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
    let code = body["error"].as_str().unwrap_or_default();
    let description = body["error_description"].as_str().unwrap_or_default();
    let kind = match (response.status, code) {
        (_, "invalid_grant") => SignInFailureKind::Refused,
        (_, "invalid_client" | "unauthorized_client") => SignInFailureKind::ClientRejected,
        (_, "access_denied" | "invalid_scope") => SignInFailureKind::NotAllowed,
        (_, "rate_limit_exceeded") | (429, _) => SignInFailureKind::LimitReached,
        (_, "server_error" | "temporarily_unavailable") | (500..=599, _) => {
            SignInFailureKind::NetworkDown
        }
        _ => SignInFailureKind::Unexpected,
    };
    SignInFailure::new(
        kind,
        detail(response.status, code, description, &body["log_id"]),
    )
}

/// Classifies an Open API error answer (`error.code`).
fn api_failure(response: &HttpResponse, body: &Value) -> SignInFailure {
    let error = &body["error"];
    let code = error["code"].as_str().unwrap_or_default();
    let kind = match (response.status, code) {
        (_, "access_token_invalid") | (401, "") => SignInFailureKind::Refused,
        (_, "scope_not_authorized" | "scope_permission_missed") => SignInFailureKind::NotAllowed,
        (_, "rate_limit_exceeded") | (429, _) => SignInFailureKind::LimitReached,
        (_, "internal_error") | (500..=599, _) => SignInFailureKind::NetworkDown,
        _ => SignInFailureKind::Unexpected,
    };
    SignInFailure::new(
        kind,
        detail(
            response.status,
            code,
            error["message"].as_str().unwrap_or_default(),
            &error["log_id"],
        ),
    )
}

/// `HTTP 200: invalid_grant: Authorization code is expired. (log_id …)`,
/// for the log; TikTok's support asks for the `log_id`.
fn detail(status: u16, code: &str, message: &str, log_id: &Value) -> String {
    let mut detail = format!("HTTP {status}");
    for part in [code, message] {
        if !part.is_empty() {
            detail.push_str(": ");
            detail.push_str(part);
        }
    }
    if let Some(log_id) = log_id.as_str().filter(|id| !id.is_empty()) {
        detail.push_str(&format!(" (log_id {log_id})"));
    }
    detail
}
