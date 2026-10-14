//! X sign-in (#147, ADR-0008): OAuth 2.0 authorization code with PKCE
//! through the user's own X developer app, over a loopback redirect, with
//! X's own rules (docs checked 2026-10-14):
//!
//! - the app is a confidential client ("Web App, Automated App or Bot"):
//!   the token endpoint takes the client id and secret as HTTP Basic, and
//!   the user enters both in Settings like every other network's app;
//! - the redirect must match one of the app's callback addresses exactly,
//!   port included, so Bardo listens on a fixed port and the user
//!   registers `http://127.0.0.1:47147/callback` (X asks for `127.0.0.1`,
//!   not `localhost`);
//! - the access token lasts two hours; `offline.access` brings a refresh
//!   token, which a refresh replaces with a new one;
//! - revoking needs `token_type_hint`, although the guide leaves it out
//!   (recorded 2026-10-14), and X answers `{"revoked":true}` even for a
//!   token it does not know;
//! - the OAuth endpoints answer errors with HTTP 400 and RFC 6749's
//!   `error` and `error_description`; the API answers with a problem
//!   (`title`, `detail`, `type`), and a token that is not a user's comes
//!   back as 403 "Unsupported Authentication".
//!
//! The connected account is the user id and `@username` from
//! `GET /2/users/me`, which X bills as one user read (US$ 0.010).

use std::time::Duration;

use bardo_ai::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use bardo_domain::{
    AppCredentials, BrowserSignIn, ConnectedIdentity, ConsentRequest, Network, NetworkSignIn,
    SecretText, SignInFailure, SignInFailureKind, TokenGrant, TokenSet,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;

use crate::oauth::{ChallengeEncoding, Pkce, new_state, parse_grant};
use crate::text::{SHOWN_TEXT, plain};

/// Read posts: X asks for it to create one.
pub const SCOPE_TWEET_READ: &str = "tweet.read";
/// Create posts.
pub const SCOPE_TWEET_WRITE: &str = "tweet.write";
/// Read the signed-in account (`/2/users/me`); creating a post asks for it
/// too.
pub const SCOPE_USERS_READ: &str = "users.read";
/// Upload the video.
pub const SCOPE_MEDIA_WRITE: &str = "media.write";
/// A refresh token, so the connection outlives the two-hour access token.
pub const SCOPE_OFFLINE: &str = "offline.access";

const SCOPES: &[&str] = &[
    SCOPE_TWEET_READ,
    SCOPE_TWEET_WRITE,
    SCOPE_USERS_READ,
    SCOPE_MEDIA_WRITE,
    SCOPE_OFFLINE,
];

/// The path X sends the browser back to.
pub const CALLBACK_PATH: &str = "/callback";

/// The port Bardo listens on for X's callback: X matches the whole
/// address, so it cannot be one the system picks. Below Windows' dynamic
/// range (49152 and up), where other programs' ports come from.
pub const CALLBACK_PORT: u16 = 47147;

/// What the user registers as the app's callback address.
pub const CALLBACK_URL: &str = "http://127.0.0.1:47147/callback";

/// Where X's endpoints live. Tests point them at a local fake server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XEndpoints {
    /// The consent page.
    pub authorize: String,
    /// The API's base address: OAuth tokens, the account, media, posts.
    pub api: String,
}

impl Default for XEndpoints {
    fn default() -> Self {
        Self {
            authorize: "https://x.com/i/oauth2/authorize".into(),
            api: "https://api.x.com".into(),
        }
    }
}

/// Signs in to X over HTTPS.
pub struct XSignIn<T = UreqTransport> {
    transport: T,
    endpoints: XEndpoints,
}

impl XSignIn {
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for XSignIn {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> XSignIn<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: XEndpoints::default(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: XEndpoints) -> Self {
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

    /// A call to an OAuth endpoint (`/2/oauth2/<path>`) as the app.
    fn oauth(
        &self,
        path: &str,
        credentials: &AppCredentials,
        fields: &[(&str, &str)],
    ) -> Result<HttpResponse, SignInFailure> {
        let url = format!("{}/2/oauth2/{path}", self.endpoints.api);
        let request =
            HttpRequest::post_form(url, fields).header("authorization", basic(credentials));
        let response = self.send(&request)?;
        if !(200..=299).contains(&response.status) {
            return Err(oauth_failure(&response));
        }
        Ok(response)
    }

    fn token(
        &self,
        credentials: &AppCredentials,
        fields: &[(&str, &str)],
    ) -> Result<TokenGrant, SignInFailure> {
        parse_grant(&self.oauth("token", credentials, fields)?.body)
    }

    fn revoke_one(
        &self,
        credentials: &AppCredentials,
        token: &str,
        hint: &str,
    ) -> Result<(), SignInFailure> {
        match self.oauth(
            "revoke",
            credentials,
            &[("token", token), ("token_type_hint", hint)],
        ) {
            Ok(_) => Ok(()),
            // Expired or revoked already: nothing is left to revoke.
            Err(failure) if failure.kind == SignInFailureKind::Refused => Ok(()),
            Err(failure) => Err(failure),
        }
    }
}

/// `Basic base64(client_id:client_secret)`, as X's token endpoint takes a
/// confidential client.
fn basic(credentials: &AppCredentials) -> String {
    let pair = format!(
        "{}:{}",
        credentials.client_id(),
        credentials.client_secret()
    );
    format!("Basic {}", STANDARD.encode(pair))
}

impl<T: Transport> NetworkSignIn for XSignIn<T> {
    fn network(&self) -> Network {
        Network::X
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
        self.token(
            credentials,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ],
        )
    }

    /// Revokes the refresh token, which ends the grant, then the access
    /// token, which may still work for up to two hours otherwise. Both
    /// authenticate the app, so a disconnect without saved app credentials
    /// forgets the tokens without revoking them.
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
        if let Some(refresh_token) = tokens.refresh_token() {
            self.revoke_one(credentials, refresh_token, "refresh_token")?;
        }
        self.revoke_one(credentials, tokens.access_token(), "access_token")
    }

    fn identity(&self, access_token: &str) -> Result<ConnectedIdentity, SignInFailure> {
        let request = HttpRequest::get(format!("{}/2/users/me", self.endpoints.api))
            .header("authorization", format!("Bearer {access_token}"));
        let response = self.send(&request)?;
        let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
        if !(200..=299).contains(&response.status) {
            return Err(api_failure(&response, &body));
        }
        let user = &body["data"];
        let Some(id) = user["id"].as_str().filter(|id| is_user_id(id)) else {
            return Err(unexpected("the account answer has no user id"));
        };
        // Every X account has a username; should one come back empty, the
        // card still names the account by its id.
        let name = user["username"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(|| id.to_owned(), |name| format!("@{name}"));
        Ok(ConnectedIdentity {
            id: id.to_owned(),
            name,
        })
    }
}

impl<T: Transport> BrowserSignIn for XSignIn<T> {
    fn redirect_path(&self) -> &'static str {
        CALLBACK_PATH
    }

    fn redirect_port(&self) -> Option<u16> {
        Some(CALLBACK_PORT)
    }

    fn consent_request(&self, credentials: &AppCredentials, redirect_uri: &str) -> ConsentRequest {
        let pkce = Pkce::generate(ChallengeEncoding::Base64Url);
        let state = new_state();
        let scope = SCOPES.join(" ");
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("response_type", "code"),
                ("client_id", credentials.client_id()),
                ("redirect_uri", redirect_uri),
                ("scope", scope.as_str()),
                ("state", state.expose()),
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

    /// X's code lasts 30 seconds, so the exchange follows the callback
    /// at once.
    fn exchange(
        &self,
        credentials: &AppCredentials,
        code: &SecretText,
        verifier: &SecretText,
        redirect_uri: &str,
    ) -> Result<TokenGrant, SignInFailure> {
        let grant = self.token(
            credentials,
            &[
                ("code", code.expose()),
                ("grant_type", "authorization_code"),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier.expose()),
            ],
        )?;
        if grant.refresh_token.is_none() {
            // Without it the connection would die with the access token.
            return Err(unexpected("the token answer has no refresh token"));
        }
        Ok(grant)
    }
}

/// A user id as X hands them out: 1 to 19 digits, so it goes in an address
/// as it is.
pub(crate) fn is_user_id(id: &str) -> bool {
    (1..=19).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

fn unexpected(detail: &str) -> SignInFailure {
    SignInFailure::new(SignInFailureKind::Unexpected, detail)
}

/// Whether X's `invalid_request` is about the code or token it was given
/// ("Value passed for the token was invalid", "…for the authorization
/// code…"), which X answers where RFC 6749 says `invalid_grant`.
fn names_a_bad_grant(description: &str) -> bool {
    let description = description.to_ascii_lowercase();
    [
        "value passed for the token",
        "value passed for the authorization code",
    ]
    .iter()
    .any(|phrase| description.contains(phrase))
}

/// Classifies an OAuth endpoint's error answer (RFC 6749 codes).
fn oauth_failure(response: &HttpResponse) -> SignInFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
    let code = body["error"].as_str().unwrap_or_default();
    let description = body["error_description"].as_str().unwrap_or_default();
    let kind = match (response.status, code) {
        (_, "invalid_grant") => SignInFailureKind::Refused,
        (_, "invalid_request") if names_a_bad_grant(description) => SignInFailureKind::Refused,
        (_, "invalid_client" | "unauthorized_client") | (401, _) => {
            SignInFailureKind::ClientRejected
        }
        (_, "access_denied" | "invalid_scope") => SignInFailureKind::NotAllowed,
        (429, _) => SignInFailureKind::LimitReached,
        (_, "server_error" | "temporarily_unavailable") | (500..=599, _) => {
            SignInFailureKind::NetworkDown
        }
        _ => SignInFailureKind::Unexpected,
    };
    SignInFailure::new(kind, detail(response.status, code, description))
}

/// Classifies an API error answer: a problem (`title`, `detail`, `type`)
/// or the older `errors` list.
fn api_failure(response: &HttpResponse, body: &Value) -> SignInFailure {
    let problem = problem_type(body);
    let kind = match (response.status, problem) {
        (401, _) | (403, "unsupported-authentication") => SignInFailureKind::Refused,
        // Out of credits, or over a rate limit.
        (402 | 429, _) => SignInFailureKind::LimitReached,
        (403, _) => SignInFailureKind::NotAllowed,
        (500..=599, _) => SignInFailureKind::NetworkDown,
        _ => SignInFailureKind::Unexpected,
    };
    SignInFailure::new(kind, api_detail(response.status, body))
}

/// The last part of a problem's `type`
/// (`https://api.twitter.com/2/problems/usage-capped` → `usage-capped`).
pub(crate) fn problem_type(body: &Value) -> &str {
    body["type"]
        .as_str()
        .and_then(|kind| kind.rsplit('/').next())
        .unwrap_or_default()
}

/// `HTTP 400: invalid_client: Value passed for the client id was invalid.`
fn detail(status: u16, code: &str, message: &str) -> String {
    let mut detail = format!("HTTP {status}");
    for part in [code, message] {
        let part = plain(part, SHOWN_TEXT);
        if !part.is_empty() {
            detail.push_str(": ");
            detail.push_str(&part);
        }
    }
    detail
}

/// `HTTP 403: Unsupported Authentication: Authenticating with …`, from a
/// problem and the first of its `errors`, which names what was refused
/// when the problem only says a parameter was invalid, for the log.
pub(crate) fn api_detail(status: u16, body: &Value) -> String {
    let first = &body["errors"][0];
    let title = body["title"]
        .as_str()
        .or_else(|| first["title"].as_str())
        .unwrap_or_default();
    let mut detail = detail(
        status,
        title,
        body["detail"]
            .as_str()
            .or_else(|| first["detail"].as_str())
            .unwrap_or_default(),
    );
    if let Some(message) = first["message"].as_str() {
        let message = plain(message, SHOWN_TEXT);
        if !message.is_empty() && !detail.contains(&message) {
            detail.push_str(": ");
            detail.push_str(&message);
        }
    }
    detail
}
