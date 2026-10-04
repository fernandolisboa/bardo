//! Instagram sign-in (ADR-0008): the Instagram API with Facebook Login for
//! Business, through the user's own Meta app, for an Instagram professional
//! account linked to a Facebook Page.
//!
//! Facebook Login documents no loopback redirect for desktop apps: only an
//! embedded web view sent to `login_success.html`, or an HTTPS address
//! matched exactly ("Enforce HTTPS" and Strict Mode are on for every app).
//! So the user generates a user access token for their app in the Graph API
//! Explorer, with every permission ticked, and pastes it. Bardo trades it
//! for a long-lived user token (about 60 days), lists the Pages it reaches
//! with their linked Instagram accounts, and keeps the chosen Page's token:
//! content publishing with Facebook Login takes a Page token, and one read
//! with a long-lived user token has no expiry date. The user token is kept
//! too, to read the Page token again and to revoke; Bardo trades it for a
//! fresh one before it runs out.

use std::time::Duration;

use bardo_ai::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use bardo_domain::{
    AppCredentials, ConnectedIdentity, DiscoveredAccount, Network, NetworkSignIn,
    PastedTokenSignIn, SecretText, SignInFailure, SignInFailureKind, TokenGrant, TokenSet,
};
use serde_json::Value;

/// The Graph API version Bardo was checked against (2026-10-04).
pub const GRAPH_VERSION: &str = "v25.0";

/// Read the Instagram account's profile and media.
pub const SCOPE_BASIC: &str = "instagram_basic";
/// Publish Reels.
pub const SCOPE_PUBLISH: &str = "instagram_content_publish";
/// Read the account's and its media's insights.
pub const SCOPE_INSIGHTS: &str = "instagram_manage_insights";
/// List the Pages the user can act on (`/me/accounts`).
pub const SCOPE_PAGES: &str = "pages_show_list";
/// Content publishing with Facebook Login needs it next to the Instagram
/// permissions.
pub const SCOPE_PAGE_ENGAGEMENT: &str = "pages_read_engagement";

const SCOPES: &[&str] = &[
    SCOPE_BASIC,
    SCOPE_PUBLISH,
    SCOPE_INSIGHTS,
    SCOPE_PAGES,
    SCOPE_PAGE_ENGAGEMENT,
];

/// The user token cannot be traded once expired, so Bardo trades it a week
/// before: an app left closed for a few days still finds it alive.
pub const RENEW_AHEAD: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// "A long-lived token generally lasts about 60 days": the lifetime assumed
/// when Meta's answer leaves `expires_in` out.
const LONG_LIVED: Duration = Duration::from_secs(60 * 24 * 60 * 60);

/// Pages of `/me/accounts` read at most; 100 Pages each.
const MAX_PAGE_READS: usize = 10;

/// Where Meta's Graph API and its upload host live. Tests point them at a
/// local fake server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaEndpoints {
    /// The Graph API's base address, version included.
    pub graph: String,
    /// Where Reels' files go (`rupload.facebook.com`), version included.
    pub rupload: String,
}

impl Default for MetaEndpoints {
    fn default() -> Self {
        Self {
            graph: format!("https://graph.facebook.com/{GRAPH_VERSION}"),
            rupload: format!("https://rupload.facebook.com/ig-api-upload/{GRAPH_VERSION}"),
        }
    }
}

/// Signs in to Instagram over HTTPS.
pub struct InstagramSignIn<T = UreqTransport> {
    transport: T,
    endpoints: MetaEndpoints,
}

impl InstagramSignIn {
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for InstagramSignIn {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> InstagramSignIn<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: MetaEndpoints::default(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: MetaEndpoints) -> Self {
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

    /// A Graph API read as the token's owner, answered in JSON.
    fn read(&self, path_and_query: &str, token: &str) -> Result<Value, SignInFailure> {
        let request = HttpRequest::get(format!("{}/{path_and_query}", self.endpoints.graph))
            .header("authorization", format!("Bearer {token}"));
        let response = self.send(&request)?;
        if !(200..=299).contains(&response.status) {
            return Err(graph_failure(&response));
        }
        serde_json::from_str(&response.body)
            .map_err(|error| unexpected(&format!("unreadable Graph API answer: {error}")))
    }

    /// Trades a valid user token for a long-lived one. The token endpoint
    /// takes the app secret, so it is the user's own app (ADR-0008).
    fn long_lived(
        &self,
        credentials: &AppCredentials,
        user_token: &str,
    ) -> Result<(SecretText, Duration), SignInFailure> {
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("grant_type", "fb_exchange_token"),
                ("client_id", credentials.client_id()),
                ("client_secret", credentials.client_secret()),
                ("fb_exchange_token", user_token),
            ])
            .finish();
        let response = self.send(&HttpRequest::get(format!(
            "{}/oauth/access_token?{query}",
            self.endpoints.graph
        )))?;
        if !(200..=299).contains(&response.status) {
            return Err(graph_failure(&response));
        }
        let body: Value = serde_json::from_str(&response.body)
            .map_err(|error| unexpected(&format!("unreadable token answer: {error}")))?;
        let token = body["access_token"]
            .as_str()
            .and_then(token_text)
            .ok_or_else(|| unexpected("the token answer has no access token"))?;
        if body["token_type"]
            .as_str()
            .is_some_and(|kind| !kind.eq_ignore_ascii_case("bearer"))
        {
            return Err(unexpected("the token answer is not a bearer token"));
        }
        let lifetime = match &body["expires_in"] {
            Value::Null => LONG_LIVED,
            value => value
                .as_u64()
                .filter(|secs| *secs > 0)
                .map(Duration::from_secs)
                .ok_or_else(|| unexpected("the token answer has no lifetime"))?,
        };
        Ok((token, lifetime))
    }

    /// The permissions the user granted to the app.
    fn granted(&self, user_token: &str) -> Result<Vec<String>, SignInFailure> {
        let body = self.read("me/permissions", user_token)?;
        let items = body["data"]
            .as_array()
            .ok_or_else(|| unexpected("the permissions answer has no list"))?;
        Ok(items
            .iter()
            .filter(|item| item["status"].as_str() == Some("granted"))
            .filter_map(|item| item["permission"].as_str())
            .map(str::to_owned)
            .collect())
    }
}

impl<T: Transport> NetworkSignIn for InstagramSignIn<T> {
    fn network(&self) -> Network {
        Network::InstagramReels
    }

    fn scopes(&self) -> &'static [&'static str] {
        SCOPES
    }

    fn refresh_margin(&self) -> Duration {
        RENEW_AHEAD
    }

    /// Trades the user token for a fresh long-lived one and reads the Page
    /// token of `identity` again with it. The account no longer reachable
    /// through any Page is a refusal: the user lost the Page or unlinked it.
    fn refresh(
        &self,
        credentials: &AppCredentials,
        tokens: &TokenSet,
        identity: &ConnectedIdentity,
    ) -> Result<TokenGrant, SignInFailure> {
        let user_token = tokens
            .refresh_token()
            .ok_or_else(|| SignInFailure::new(SignInFailureKind::Refused, "no user token"))?;
        let (user_token, lifetime) = self.long_lived(credentials, user_token)?;
        let page = self
            .discover(user_token.expose())?
            .into_iter()
            .find(|found| {
                found
                    .identity
                    .as_ref()
                    .is_some_and(|account| account.id == identity.id)
            })
            .ok_or_else(|| {
                SignInFailure::new(
                    SignInFailureKind::Refused,
                    "the Instagram account is no longer linked to a Page the user manages",
                )
            })?;
        Ok(TokenGrant {
            access_token: page.token,
            refresh_token: Some(user_token),
            expires_in: lifetime,
            scopes: Vec::new(),
        })
    }

    /// Removes every permission the user granted to the app, which ends
    /// its user and Page tokens.
    fn revoke(
        &self,
        _credentials: Option<&AppCredentials>,
        tokens: &TokenSet,
    ) -> Result<(), SignInFailure> {
        let user_token = tokens.refresh_token().ok_or_else(|| {
            SignInFailure::new(
                SignInFailureKind::Unexpected,
                "no user token to revoke with",
            )
        })?;
        let request = HttpRequest::delete(format!("{}/me/permissions", self.endpoints.graph))
            .header("authorization", format!("Bearer {user_token}"));
        let response = self.send(&request)?;
        if (200..=299).contains(&response.status) {
            return Ok(());
        }
        match graph_error(&response) {
            // The token no longer works: nothing is left to revoke.
            Some(error) if error.code == 190 => Ok(()),
            _ => Err(graph_failure(&response)),
        }
    }

    fn revokes_app_wide(&self) -> bool {
        true
    }

    /// With a Page token, `me` is the Page: its linked Instagram account.
    fn identity(&self, access_token: &str) -> Result<ConnectedIdentity, SignInFailure> {
        let body = self.read(
            "me?fields=instagram_business_account%7Bid%2Cusername%7D",
            access_token,
        )?;
        instagram_account(&body["instagram_business_account"])?.ok_or_else(|| {
            SignInFailure::new(
                SignInFailureKind::NoChannel,
                "the Page has no Instagram professional account linked",
            )
        })
    }

    fn pasted(&self) -> Option<&dyn PastedTokenSignIn> {
        Some(self)
    }
}

impl<T: Transport> PastedTokenSignIn for InstagramSignIn<T> {
    fn exchange_pasted(
        &self,
        credentials: &AppCredentials,
        pasted: &SecretText,
    ) -> Result<TokenGrant, SignInFailure> {
        let (user_token, lifetime) = self.long_lived(credentials, pasted.expose())?;
        let scopes = self.granted(user_token.expose())?;
        Ok(TokenGrant {
            access_token: user_token,
            refresh_token: None,
            expires_in: lifetime,
            scopes,
        })
    }

    fn discover(&self, user_token: &str) -> Result<Vec<DiscoveredAccount>, SignInFailure> {
        let mut found = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..MAX_PAGE_READS {
            let mut query = form_urlencoded::Serializer::new(String::new());
            query.extend_pairs([
                (
                    "fields",
                    "id,name,access_token,instagram_business_account{id,username}",
                ),
                ("limit", "100"),
            ]);
            if let Some(cursor) = &after {
                query.append_pair("after", cursor);
            }
            let body = self.read(&format!("me/accounts?{}", query.finish()), user_token)?;
            let pages = body["data"]
                .as_array()
                .ok_or_else(|| unexpected("the Pages answer has no list"))?;
            for page in pages {
                // A Page whose token Meta does not hand out (no task that
                // allows it) cannot publish: it is not offered.
                let Some(token) = page["access_token"].as_str().and_then(token_text) else {
                    continue;
                };
                let via = page["name"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .or_else(|| page["id"].as_str())
                    .unwrap_or_default()
                    .to_owned();
                found.push(DiscoveredAccount {
                    via,
                    identity: instagram_account(&page["instagram_business_account"])?,
                    token,
                });
            }
            after = body["paging"]["next"]
                .as_str()
                .and_then(|_| body["paging"]["cursors"]["after"].as_str())
                .map(str::to_owned);
            if after.is_none() {
                break;
            }
        }
        Ok(found)
    }
}

/// The Instagram account a Page links to: `None` when there is none.
fn instagram_account(value: &Value) -> Result<Option<ConnectedIdentity>, SignInFailure> {
    if value.is_null() {
        return Ok(None);
    }
    match (value["id"].as_str(), value["username"].as_str()) {
        (Some(id), Some(username)) if !id.is_empty() && !username.is_empty() => {
            Ok(Some(ConnectedIdentity {
                id: id.to_owned(),
                name: format!("@{username}"),
            }))
        }
        _ => Err(unexpected("the Instagram account has no id or username")),
    }
}

/// A token as Meta sends it: printable ASCII without spaces.
fn token_text(token: &str) -> Option<SecretText> {
    (!token.is_empty() && token.chars().all(|c| c.is_ascii_graphic()))
        .then(|| SecretText::new(token))
}

fn unexpected(detail: &str) -> SignInFailure {
    SignInFailure::new(SignInFailureKind::Unexpected, detail)
}

/// The `error` object of a Graph API answer.
struct GraphError {
    code: i64,
    message: String,
}

fn graph_error(response: &HttpResponse) -> Option<GraphError> {
    let body: Value = serde_json::from_str(&response.body).ok()?;
    let error = body.get("error")?;
    Some(GraphError {
        code: error["code"].as_i64().unwrap_or_default(),
        message: error["message"].as_str().unwrap_or_default().to_owned(),
    })
}

/// Classifies a Graph API error answer by its documented codes.
fn graph_failure(response: &HttpResponse) -> SignInFailure {
    let error = graph_error(response);
    let (code, message) = error
        .as_ref()
        .map_or((0, ""), |error| (error.code, error.message.as_str()));
    let kind = match (response.status, code) {
        // 190: expired, revoked or otherwise invalid; 102: session.
        (_, 190 | 102) => SignInFailureKind::Refused,
        // 101: the app id is unknown. A wrong secret comes back as code 1
        // ("Error validating client secret."), which otherwise means a
        // passing failure on Meta's side.
        (_, 101) => SignInFailureKind::ClientRejected,
        (_, 1) if message.to_ascii_lowercase().contains("client secret") => {
            SignInFailureKind::ClientRejected
        }
        // 3: capability; 10 and 200-299: a permission not granted; 368: a
        // policy block, which waiting does not lift.
        (_, 3 | 10 | 200..=299 | 368) => SignInFailureKind::NotAllowed,
        (_, 4 | 17 | 32 | 341 | 613) | (429, _) => SignInFailureKind::LimitReached,
        (_, 1 | 2) | (500..=599, _) => SignInFailureKind::NetworkDown,
        (401, _) => SignInFailureKind::Refused,
        _ => SignInFailureKind::Unexpected,
    };
    let detail = match (code, message) {
        (0, "") => format!("HTTP {}", response.status),
        (code, message) => format!("HTTP {}: {code}: {message}", response.status),
    };
    SignInFailure::new(kind, detail)
}
