//! YouTube sign-in: Google's OAuth 2.0 for desktop apps (authorization code
//! with PKCE `S256` over a loopback redirect), the user's own Desktop OAuth
//! client (ADR-0008), and the connected channel from `channels.list
//! mine=true`.
//!
//! Every scope is asked at once: installed apps cannot add scopes later.

use std::time::Duration;

use bardo_ai::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use bardo_domain::{
    AppCredentials, BrowserSignIn, ConnectedIdentity, ConsentRequest, Network, NetworkSignIn,
    SecretText, SignInFailure, SignInFailureKind, TokenGrant, TokenSet,
};
use serde_json::Value;

use crate::oauth::{ChallengeEncoding, Pkce, new_state, parse_grant};

/// Upload videos and set thumbnails.
pub const SCOPE_UPLOAD: &str = "https://www.googleapis.com/auth/youtube.upload";
/// Manage the channel's videos: `publishAt`, reading the channel.
pub const SCOPE_MANAGE: &str = "https://www.googleapis.com/auth/youtube";
/// The owner's YouTube Analytics reports.
pub const SCOPE_ANALYTICS: &str = "https://www.googleapis.com/auth/yt-analytics.readonly";
/// Revenue and CPM reports (answered only for Partner Program channels).
pub const SCOPE_MONETARY: &str = "https://www.googleapis.com/auth/yt-analytics-monetary.readonly";

const SCOPES: &[&str] = &[SCOPE_UPLOAD, SCOPE_MANAGE, SCOPE_ANALYTICS, SCOPE_MONETARY];

/// Where Google's endpoints live. Tests point them at a local fake server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleEndpoints {
    pub authorize: String,
    pub token: String,
    pub revoke: String,
    /// The YouTube Data API's base address.
    pub api: String,
    /// The YouTube Analytics API's base address.
    pub analytics: String,
}

impl Default for GoogleEndpoints {
    fn default() -> Self {
        Self {
            authorize: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            api: "https://www.googleapis.com".into(),
            analytics: "https://youtubeanalytics.googleapis.com".into(),
        }
    }
}

/// Signs in to YouTube over HTTPS.
pub struct YouTubeSignIn<T = UreqTransport> {
    transport: T,
    endpoints: GoogleEndpoints,
}

impl YouTubeSignIn {
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for YouTubeSignIn {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> YouTubeSignIn<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: GoogleEndpoints::default(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: GoogleEndpoints) -> Self {
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

    /// A token endpoint call: an exchange or a refresh.
    fn token(&self, fields: &[(&str, &str)]) -> Result<TokenGrant, SignInFailure> {
        let response = self.send(&HttpRequest::post_form(&self.endpoints.token, fields))?;
        if !(200..=299).contains(&response.status) {
            return Err(oauth_failure(&response));
        }
        parse_grant(&response.body)
    }
}

impl<T: Transport> NetworkSignIn for YouTubeSignIn<T> {
    fn network(&self) -> Network {
        Network::YouTube
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
        self.token(&[
            ("client_id", credentials.client_id()),
            ("client_secret", credentials.client_secret()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
    }

    fn revoke(
        &self,
        _credentials: Option<&AppCredentials>,
        tokens: &TokenSet,
    ) -> Result<(), SignInFailure> {
        // Revoking the refresh token revokes its access tokens too.
        let token = tokens.refresh_token().unwrap_or(tokens.access_token());
        let response = self.send(&HttpRequest::post_form(
            &self.endpoints.revoke,
            &[("token", token)],
        ))?;
        match response.status {
            200..=299 => Ok(()),
            // Already revoked or expired: nothing is left to revoke.
            400 if error_code(&response).as_deref() == Some("invalid_token") => Ok(()),
            _ => Err(oauth_failure(&response)),
        }
    }

    fn identity(&self, access_token: &str) -> Result<ConnectedIdentity, SignInFailure> {
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("part", "snippet"),
                ("mine", "true"),
                ("fields", "items(id,snippet/title)"),
            ])
            .finish();
        let request = HttpRequest::get(format!(
            "{}/youtube/v3/channels?{query}",
            self.endpoints.api
        ))
        .header("authorization", format!("Bearer {access_token}"));
        let response = self.send(&request)?;
        if !(200..=299).contains(&response.status) {
            return Err(api_failure(&response));
        }
        let body: Value = serde_json::from_str(&response.body)
            .map_err(|error| unexpected(&format!("unreadable channels answer: {error}")))?;
        let Some(channel) = body["items"].as_array().and_then(|items| items.first()) else {
            return Err(SignInFailure::new(
                SignInFailureKind::NoChannel,
                "the Google account has no YouTube channel",
            ));
        };
        match (channel["id"].as_str(), channel["snippet"]["title"].as_str()) {
            (Some(id), Some(name)) if !id.is_empty() => Ok(ConnectedIdentity {
                id: id.to_owned(),
                name: name.to_owned(),
            }),
            _ => Err(unexpected("the channel has no id or title")),
        }
    }
}

impl<T: Transport> BrowserSignIn for YouTubeSignIn<T> {
    fn consent_request(&self, credentials: &AppCredentials, redirect_uri: &str) -> ConsentRequest {
        let pkce = Pkce::generate(ChallengeEncoding::Base64Url);
        let state = new_state();
        let scope = SCOPES.join(" ");
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("client_id", credentials.client_id()),
                ("redirect_uri", redirect_uri),
                ("response_type", "code"),
                ("scope", scope.as_str()),
                ("code_challenge", pkce.challenge()),
                ("code_challenge_method", Pkce::METHOD),
                ("state", state.expose()),
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
            ("client_id", credentials.client_id()),
            ("client_secret", credentials.client_secret()),
            ("code", code.expose()),
            ("code_verifier", verifier.expose()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
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

/// The `error` code of an OAuth error answer.
fn error_code(response: &HttpResponse) -> Option<String> {
    let body: Value = serde_json::from_str(&response.body).ok()?;
    body["error"].as_str().map(str::to_owned)
}

/// Classifies an OAuth endpoint's error answer (RFC 6749 section 5.2).
fn oauth_failure(response: &HttpResponse) -> SignInFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
    let code = body["error"].as_str().unwrap_or_default();
    let description = body["error_description"].as_str().unwrap_or_default();
    let kind = match (response.status, code) {
        (_, "invalid_grant") => SignInFailureKind::Refused,
        (_, "invalid_client" | "unauthorized_client") => SignInFailureKind::ClientRejected,
        (429, _) => SignInFailureKind::LimitReached,
        (500..=599, _) => SignInFailureKind::NetworkDown,
        _ => SignInFailureKind::Unexpected,
    };
    let detail = match (code, description) {
        ("", "") => format!("HTTP {}", response.status),
        (code, "") => format!("HTTP {}: {code}", response.status),
        (code, description) => format!("HTTP {}: {code}: {description}", response.status),
    };
    SignInFailure::new(kind, detail)
}

/// Classifies a YouTube Data API error answer.
fn api_failure(response: &HttpResponse) -> SignInFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
    let error = &body["error"];
    let message = error["message"].as_str().unwrap_or_default();
    let reasons: Vec<&str> = error["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["reason"].as_str())
        .chain(
            error["details"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|detail| detail["reason"].as_str()),
        )
        .collect();
    let has = |reason: &str| reasons.contains(&reason);
    let kind = match response.status {
        401 => SignInFailureKind::Refused,
        403 | 429 if has("quotaExceeded") || has("rateLimitExceeded") => {
            SignInFailureKind::LimitReached
        }
        403 => SignInFailureKind::NotAllowed,
        429 => SignInFailureKind::LimitReached,
        500..=599 => SignInFailureKind::NetworkDown,
        _ => SignInFailureKind::Unexpected,
    };
    let detail = if message.is_empty() {
        format!("HTTP {}", response.status)
    } else {
        format!("HTTP {}: {message}", response.status)
    };
    SignInFailure::new(kind, detail)
}
