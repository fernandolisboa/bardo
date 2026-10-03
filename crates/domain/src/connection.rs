//! Network connections (ADR-0008): a network account signed in through the
//! network's OAuth with the user's own app credentials.
//!
//! Secrets (the app credentials and the tokens) live only in the secret
//! store; the database keeps the connection's state: who is connected,
//! the scopes granted, when the access token expires and when it was last
//! refreshed. Signing in, the browser callback and the network calls are
//! ports, implemented in `publish`.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use zeroize::{Zeroize, Zeroizing};

use crate::{Network, NetworkAccountId, ProfileId, RepositoryError, SecretStoreError};

/// Text that must never be shown or logged: an OAuth token, a client
/// secret, a PKCE verifier. `Debug` hides it, there is no `Display`, and its
/// memory is wiped when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretText(String);

impl SecretText {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text itself. Only for the request that carries it or the
    /// secret store.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The last four characters, so the user can tell secrets apart.
    pub fn hint(&self) -> String {
        let start = self
            .0
            .char_indices()
            .rev()
            .nth(3)
            .map_or(0, |(index, _)| index);
        format!("…{}", &self.0[start..])
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Printable ASCII with no spaces: what client ids, secrets and tokens are
/// made of. Anything else is a paste mistake or a broken answer.
fn is_token_text(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_graphic())
}

impl Network {
    /// Whether Bardo signs in to this network (ADR-0008). TikTok and
    /// Instagram Reels follow in their own slices; X and Kick stay export
    /// only.
    pub fn signs_in(self) -> bool {
        matches!(self, Network::YouTube)
    }

    /// The networks Bardo signs in to, in network order.
    pub fn sign_in_networks() -> impl Iterator<Item = Network> {
        Network::ALL
            .into_iter()
            .filter(|network| network.signs_in())
    }
}

/// Why typed app credentials are not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppCredentialsFieldError {
    ClientIdRequired,
    /// Spaces, characters ids never have, or (Google) not a Google OAuth
    /// client id.
    ClientIdInvalid,
    ClientSecretRequired,
    ClientSecretInvalid,
}

impl AppCredentialsFieldError {
    pub const ALL: [AppCredentialsFieldError; 4] = [
        AppCredentialsFieldError::ClientIdRequired,
        AppCredentialsFieldError::ClientIdInvalid,
        AppCredentialsFieldError::ClientSecretRequired,
        AppCredentialsFieldError::ClientSecretInvalid,
    ];
}

/// The client id and secret of the OAuth app the user registered on a
/// network (CONTEXT.md). Both are masked by the redactor; the id is not
/// secret by itself, but it names the user's project.
#[derive(Clone, PartialEq, Eq)]
pub struct AppCredentials {
    client_id: SecretText,
    client_secret: SecretText,
}

impl AppCredentials {
    /// Generous for every network's ids and secrets, small enough for the
    /// secret store.
    pub const MAX_CHARS: usize = 256;
    /// Shorter secrets are a paste mistake, and the redactor needs a
    /// minimum length to mask safely.
    pub const MIN_SECRET_CHARS: usize = 8;
    /// Google OAuth client ids end with this.
    pub const GOOGLE_CLIENT_SUFFIX: &str = ".apps.googleusercontent.com";

    /// Validates credentials as the user typed or pasted them. Surrounding
    /// whitespace is dropped.
    pub fn parse(
        network: Network,
        client_id: &str,
        client_secret: &str,
    ) -> Result<Self, Vec<AppCredentialsFieldError>> {
        let mut errors = Vec::new();
        let id = client_id.trim();
        if id.is_empty() {
            errors.push(AppCredentialsFieldError::ClientIdRequired);
        } else if !is_token_text(id)
            || id.len() > Self::MAX_CHARS
            || (network == Network::YouTube
                && (!id.ends_with(Self::GOOGLE_CLIENT_SUFFIX)
                    || id.len() == Self::GOOGLE_CLIENT_SUFFIX.len()))
        {
            errors.push(AppCredentialsFieldError::ClientIdInvalid);
        }
        let secret = client_secret.trim();
        if secret.is_empty() {
            errors.push(AppCredentialsFieldError::ClientSecretRequired);
        } else if !is_token_text(secret)
            || secret.len() < Self::MIN_SECRET_CHARS
            || secret.len() > Self::MAX_CHARS
        {
            errors.push(AppCredentialsFieldError::ClientSecretInvalid);
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            client_id: SecretText::new(id),
            client_secret: SecretText::new(secret),
        })
    }

    pub fn client_id(&self) -> &str {
        self.client_id.expose()
    }

    pub fn client_secret(&self) -> &str {
        self.client_secret.expose()
    }

    /// The secret's last four characters, for the settings screen.
    pub fn hint(&self) -> String {
        self.client_secret.hint()
    }

    /// Every string the redactor must mask.
    pub fn sensitive_parts(&self) -> Vec<&str> {
        vec![self.client_id.expose(), self.client_secret.expose()]
    }

    /// The form the secret store keeps: the id and the secret on two lines.
    pub fn encode(&self) -> Zeroizing<String> {
        Zeroizing::new(format!(
            "{}\n{}",
            self.client_id.expose(),
            self.client_secret.expose()
        ))
    }

    /// Reads what `encode` wrote. `None` when it is not credentials for
    /// `network` (edited by hand, or written by a newer version).
    pub fn decode(network: Network, stored: &str) -> Option<Self> {
        let (id, secret) = stored.split_once('\n')?;
        Self::parse(network, id, secret).ok()
    }
}

impl fmt::Debug for AppCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AppCredentials(<redacted>)")
    }
}

/// What a network's token endpoint granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenGrant {
    pub access_token: SecretText,
    /// Absent when a refresh keeps the refresh token the app already has.
    pub refresh_token: Option<SecretText>,
    /// How long the access token lasts from now.
    pub expires_in: Duration,
    /// The scopes granted, which may be fewer than asked.
    pub scopes: Vec<String>,
}

/// The token set is over what the secret store holds. It is refused, never
/// cut, since a cut token is a broken one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the token set is {bytes} bytes, over the {max} the secret store holds")]
pub struct TokenSetTooLarge {
    pub bytes: usize,
    pub max: usize,
}

/// A network account's OAuth tokens, as the secret store keeps them.
#[derive(Clone, PartialEq, Eq)]
pub struct TokenSet {
    access_token: SecretText,
    refresh_token: Option<SecretText>,
    expires_at: SystemTime,
}

/// The stored token set keeps its expiry in whole seconds; rounding down
/// here keeps the vault and the connection state in agreement.
fn whole_seconds(at: SystemTime) -> SystemTime {
    let secs = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

impl TokenSet {
    /// A generic Windows Credential Manager entry holds at most this many
    /// bytes (`CRED_MAX_CREDENTIAL_BLOB_SIZE`).
    pub const MAX_STORED_BYTES: usize = 2560;
    /// An access token this close to its expiry is refreshed before use,
    /// so it cannot expire halfway through a request.
    pub const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);
    const FORMAT: &str = "bardo-tokens-1";

    /// The tokens of a new connection, from the code exchange.
    pub fn granted(grant: &TokenGrant, now: SystemTime) -> Self {
        Self {
            access_token: grant.access_token.clone(),
            refresh_token: grant.refresh_token.clone(),
            expires_at: whole_seconds(now + grant.expires_in),
        }
    }

    /// These tokens after a refresh: the new access token, and the new
    /// refresh token when the network rotated it.
    pub fn refreshed(&self, grant: &TokenGrant, now: SystemTime) -> Self {
        Self {
            access_token: grant.access_token.clone(),
            refresh_token: grant
                .refresh_token
                .clone()
                .or_else(|| self.refresh_token.clone()),
            expires_at: whole_seconds(now + grant.expires_in),
        }
    }

    pub fn access_token(&self) -> &str {
        self.access_token.expose()
    }

    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_ref().map(SecretText::expose)
    }

    pub fn expires_at(&self) -> SystemTime {
        self.expires_at
    }

    /// Whether the access token is expired or within `REFRESH_MARGIN` of
    /// it at `now`.
    pub fn needs_refresh(&self, now: SystemTime) -> bool {
        self.expires_at
            .checked_sub(Self::REFRESH_MARGIN)
            .is_none_or(|limit| now >= limit)
    }

    /// Every string the redactor must mask.
    pub fn sensitive_parts(&self) -> Vec<&str> {
        let mut parts = vec![self.access_token.expose()];
        parts.extend(self.refresh_token());
        parts
    }

    /// The form the secret store keeps. Over `MAX_STORED_BYTES` it is
    /// refused.
    pub fn encode(&self) -> Result<Zeroizing<String>, TokenSetTooLarge> {
        let expires = self
            .expires_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let encoded = Zeroizing::new(format!(
            "{}\n{expires}\n{}\n{}",
            Self::FORMAT,
            self.access_token.expose(),
            self.refresh_token().unwrap_or_default()
        ));
        if encoded.len() > Self::MAX_STORED_BYTES {
            return Err(TokenSetTooLarge {
                bytes: encoded.len(),
                max: Self::MAX_STORED_BYTES,
            });
        }
        Ok(encoded)
    }

    /// Reads what `encode` wrote; `None` for anything else.
    pub fn decode(stored: &str) -> Option<Self> {
        let mut lines = stored.split('\n');
        if lines.next()? != Self::FORMAT {
            return None;
        }
        let expires: u64 = lines.next()?.parse().ok()?;
        let access = lines.next()?;
        let refresh = lines.next()?;
        if lines.next().is_some() || !is_token_text(access) {
            return None;
        }
        let refresh_token = match refresh {
            "" => None,
            token if is_token_text(token) => Some(SecretText::new(token)),
            _ => return None,
        };
        Some(Self {
            access_token: SecretText::new(access),
            refresh_token,
            expires_at: SystemTime::UNIX_EPOCH + Duration::from_secs(expires),
        })
    }
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// Who the tokens act as on the network: for YouTube, the channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedIdentity {
    /// The network's id, e.g. a YouTube channel id (`UC…`).
    pub id: String,
    /// The name the network shows, e.g. the channel title.
    pub name: String,
}

/// Whether a connection's tokens still work, as far as Bardo knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionStatus {
    Connected,
    /// The network refused a refresh: revoked, expired (a Google client in
    /// "Testing" status after seven days) or the app credentials changed.
    /// Signing in again fixes it.
    ReconnectNeeded,
}

impl ConnectionStatus {
    /// Stable identifier for storage.
    pub fn code(self) -> &'static str {
        match self {
            ConnectionStatus::Connected => "connected",
            ConnectionStatus::ReconnectNeeded => "reconnect_needed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        [
            ConnectionStatus::Connected,
            ConnectionStatus::ReconnectNeeded,
        ]
        .into_iter()
        .find(|status| status.code() == code)
    }
}

/// A network account's connection as the database keeps it: never a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkConnection {
    pub account: NetworkAccountId,
    pub owner: ProfileId,
    pub status: ConnectionStatus,
    pub identity: ConnectedIdentity,
    pub scopes: Vec<String>,
    /// When the stored access token expires.
    pub expires_at: SystemTime,
    pub connected_at: SystemTime,
    /// The last refresh the network accepted.
    pub refreshed_at: Option<SystemTime>,
}

/// Persistence port for connection state.
pub trait NetworkConnectionRepository: Send + Sync {
    fn get(&self, account: NetworkAccountId) -> Result<Option<NetworkConnection>, RepositoryError>;

    /// Inserts or replaces the account's connection.
    fn save(&self, connection: &NetworkConnection) -> Result<(), RepositoryError>;

    /// Removing one that does not exist is not an error.
    fn delete(&self, account: NetworkAccountId) -> Result<(), RepositoryError>;
}

impl<T: NetworkConnectionRepository + ?Sized> NetworkConnectionRepository for Arc<T> {
    fn get(&self, account: NetworkAccountId) -> Result<Option<NetworkConnection>, RepositoryError> {
        (**self).get(account)
    }

    fn save(&self, connection: &NetworkConnection) -> Result<(), RepositoryError> {
        (**self).save(connection)
    }

    fn delete(&self, account: NetworkAccountId) -> Result<(), RepositoryError> {
        (**self).delete(account)
    }
}

/// Where app credentials and tokens live: the operating system's credential
/// store (Windows Credential Manager), never files or the database. App
/// credentials belong to a profile and a network; tokens to a profile and a
/// network account.
pub trait ConnectionSecrets: Send + Sync {
    fn app_credentials(
        &self,
        owner: ProfileId,
        network: Network,
    ) -> Result<Option<AppCredentials>, SecretStoreError>;

    fn set_app_credentials(
        &self,
        owner: ProfileId,
        network: Network,
        credentials: &AppCredentials,
    ) -> Result<(), SecretStoreError>;

    /// Removing credentials that are not there succeeds.
    fn delete_app_credentials(
        &self,
        owner: ProfileId,
        network: Network,
    ) -> Result<(), SecretStoreError>;

    fn tokens(
        &self,
        owner: ProfileId,
        account: NetworkAccountId,
    ) -> Result<Option<TokenSet>, SecretStoreError>;

    /// Saves the tokens, replacing earlier ones. A set over
    /// `TokenSet::MAX_STORED_BYTES` is refused.
    fn set_tokens(
        &self,
        owner: ProfileId,
        account: NetworkAccountId,
        tokens: &TokenSet,
    ) -> Result<(), SecretStoreError>;

    /// Removing tokens that are not there succeeds.
    fn delete_tokens(
        &self,
        owner: ProfileId,
        account: NetworkAccountId,
    ) -> Result<(), SecretStoreError>;
}

impl<T: ConnectionSecrets + ?Sized> ConnectionSecrets for Arc<T> {
    fn app_credentials(
        &self,
        owner: ProfileId,
        network: Network,
    ) -> Result<Option<AppCredentials>, SecretStoreError> {
        (**self).app_credentials(owner, network)
    }

    fn set_app_credentials(
        &self,
        owner: ProfileId,
        network: Network,
        credentials: &AppCredentials,
    ) -> Result<(), SecretStoreError> {
        (**self).set_app_credentials(owner, network, credentials)
    }

    fn delete_app_credentials(
        &self,
        owner: ProfileId,
        network: Network,
    ) -> Result<(), SecretStoreError> {
        (**self).delete_app_credentials(owner, network)
    }

    fn tokens(
        &self,
        owner: ProfileId,
        account: NetworkAccountId,
    ) -> Result<Option<TokenSet>, SecretStoreError> {
        (**self).tokens(owner, account)
    }

    fn set_tokens(
        &self,
        owner: ProfileId,
        account: NetworkAccountId,
        tokens: &TokenSet,
    ) -> Result<(), SecretStoreError> {
        (**self).set_tokens(owner, account, tokens)
    }

    fn delete_tokens(
        &self,
        owner: ProfileId,
        account: NetworkAccountId,
    ) -> Result<(), SecretStoreError> {
        (**self).delete_tokens(owner, account)
    }
}

/// Why a call to a network's sign-in endpoints gave no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignInFailureKind {
    /// The network refused the code or refresh token (`invalid_grant`):
    /// revoked, expired, or issued to other app credentials.
    Refused,
    /// The network does not know the app credentials (`invalid_client`).
    ClientRejected,
    /// The tokens may not do this: an API not enabled on the user's
    /// project, or a scope not granted.
    NotAllowed,
    /// The signed-in account has no channel on the network.
    NoChannel,
    /// A quota or rate limit stops the call right now.
    LimitReached,
    /// The network is failing on its side.
    NetworkDown,
    /// No answer: offline, DNS, TLS, proxy or timeout.
    Unreachable,
    /// An answer the adapter does not understand.
    Unexpected,
}

/// A failed sign-in call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind:?}: {detail}")]
pub struct SignInFailure {
    pub kind: SignInFailureKind,
    /// The network's own message or the transport error, in English, for
    /// the log. Callers redact it before showing it.
    pub detail: String,
}

impl SignInFailure {
    pub fn new(kind: SignInFailureKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

/// What the browser opens for consent, and what the app keeps to finish
/// the sign-in: the `state` the callback must carry back and the PKCE
/// verifier the code exchange proves possession with.
#[derive(Debug, Clone)]
pub struct ConsentRequest {
    pub url: String,
    pub state: SecretText,
    pub verifier: SecretText,
}

/// A network's OAuth: authorization code with PKCE (ADR-0008). Calls the
/// network and blocks, so the app runs it off the UI thread.
pub trait NetworkSignIn: Send + Sync {
    fn network(&self) -> Network;

    /// Every scope Bardo asks at the first connection.
    fn scopes(&self) -> &'static [&'static str];

    /// A fresh PKCE verifier and `state`, and the consent address that
    /// carries them, sending the browser back to `redirect_uri`.
    fn consent_request(&self, credentials: &AppCredentials, redirect_uri: &str) -> ConsentRequest;

    /// Trades the code the callback brought for tokens.
    fn exchange(
        &self,
        credentials: &AppCredentials,
        code: &SecretText,
        verifier: &SecretText,
        redirect_uri: &str,
    ) -> Result<TokenGrant, SignInFailure>;

    fn refresh(
        &self,
        credentials: &AppCredentials,
        refresh_token: &str,
    ) -> Result<TokenGrant, SignInFailure>;

    /// Revokes the tokens at the network. Tokens the network no longer
    /// knows count as revoked.
    fn revoke(&self, tokens: &TokenSet) -> Result<(), SignInFailure>;

    /// Who the access token acts as.
    fn identity(&self, access_token: &str) -> Result<ConnectedIdentity, SignInFailure>;
}

/// Why no authorization code came back from the browser.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConsentError {
    /// The local callback could not start listening.
    #[error("could not listen for the callback: {0}")]
    Listen(String),
    /// The user declined, or the network answered with this error code.
    #[error("consent was not given: {0}")]
    Denied(String),
    #[error("consent did not come back in time")]
    TimedOut,
    #[error("the sign-in was cancelled")]
    Cancelled,
}

/// The two pages the callback shows in the browser, in the interface
/// language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentPages {
    pub done: String,
    pub failed: String,
}

/// A callback listening for one consent.
pub trait ConsentCallback: Send {
    /// The address the network sends the browser back to.
    fn redirect_uri(&self) -> &str;

    /// Blocks until the browser brings back a code with `state`, the user
    /// declines, `timeout` passes or `cancel` is set. Requests with any
    /// other `state` are refused and the wait goes on.
    fn wait(
        self: Box<Self>,
        state: &SecretText,
        timeout: Duration,
        cancel: &AtomicBool,
        pages: &ConsentPages,
    ) -> Result<SecretText, ConsentError>;
}

/// Starts callbacks: a one-shot loopback listener on `127.0.0.1`.
pub trait ConsentReceiver: Send + Sync {
    fn listen(&self) -> Result<Box<dyn ConsentCallback>, ConsentError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fake values, split so secret scanners do not take them for real ones.
    const GOOGLE_ID: &str = concat!("1234567890-abc123def456", ".apps.googleusercontent.com");
    const GOOGLE_SECRET: &str = concat!("GOCSPX", "-abcdefghijklmnopqrstuvwxyz12");

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    fn grant(access: &str, refresh: Option<&str>, secs: u64) -> TokenGrant {
        TokenGrant {
            access_token: SecretText::new(access),
            refresh_token: refresh.map(SecretText::new),
            expires_in: Duration::from_secs(secs),
            scopes: vec!["https://www.googleapis.com/auth/youtube".into()],
        }
    }

    #[test]
    fn only_youtube_signs_in_for_now() {
        assert_eq!(
            Network::sign_in_networks().collect::<Vec<_>>(),
            [Network::YouTube]
        );
    }

    #[test]
    fn pasted_credentials_lose_surrounding_whitespace() {
        let credentials = AppCredentials::parse(
            Network::YouTube,
            &format!(" {GOOGLE_ID}\n"),
            &format!("\t{GOOGLE_SECRET} "),
        )
        .unwrap();
        assert_eq!(credentials.client_id(), GOOGLE_ID);
        assert_eq!(credentials.client_secret(), GOOGLE_SECRET);
        assert_eq!(credentials.hint(), "…yz12");
    }

    #[test]
    fn both_fields_are_required() {
        assert_eq!(
            AppCredentials::parse(Network::YouTube, " ", "").unwrap_err(),
            [
                AppCredentialsFieldError::ClientIdRequired,
                AppCredentialsFieldError::ClientSecretRequired
            ]
        );
    }

    #[test]
    fn a_youtube_client_id_must_be_a_google_oauth_client() {
        for bad in [
            "1234567890-abc123def456",
            ".apps.googleusercontent.com",
            concat!("123 456", ".apps.googleusercontent.com"),
            "AIzaSyNotAClientId0123456789",
        ] {
            assert_eq!(
                AppCredentials::parse(Network::YouTube, bad, GOOGLE_SECRET).unwrap_err(),
                [AppCredentialsFieldError::ClientIdInvalid],
                "{bad}"
            );
        }
    }

    #[test]
    fn a_secret_must_look_like_one() {
        for bad in ["short", "has space inside", "sëcret-with-accent"] {
            assert_eq!(
                AppCredentials::parse(Network::YouTube, GOOGLE_ID, bad).unwrap_err(),
                [AppCredentialsFieldError::ClientSecretInvalid],
                "{bad}"
            );
        }
        let long = "x".repeat(AppCredentials::MAX_CHARS + 1);
        assert_eq!(
            AppCredentials::parse(Network::YouTube, GOOGLE_ID, &long).unwrap_err(),
            [AppCredentialsFieldError::ClientSecretInvalid]
        );
    }

    #[test]
    fn credentials_round_trip_through_the_stored_form() {
        let credentials =
            AppCredentials::parse(Network::YouTube, GOOGLE_ID, GOOGLE_SECRET).unwrap();
        let stored = credentials.encode();
        assert_eq!(
            AppCredentials::decode(Network::YouTube, &stored),
            Some(credentials)
        );
        assert_eq!(AppCredentials::decode(Network::YouTube, "garbage"), None);
    }

    #[test]
    fn debug_output_never_shows_a_secret() {
        let credentials =
            AppCredentials::parse(Network::YouTube, GOOGLE_ID, GOOGLE_SECRET).unwrap();
        let tokens = TokenSet::granted(
            &grant("ya29.access-0001", Some("1//refresh-0001"), 3599),
            at(0),
        );
        let shown = format!(
            "{credentials:?} {tokens:?} {:?} {:?}",
            SecretText::new("verifier-0001"),
            grant("ya29.access-0002", Some("1//refresh-0002"), 10)
        );
        for secret in [
            GOOGLE_ID,
            GOOGLE_SECRET,
            "ya29.access",
            "1//refresh",
            "verifier-0001",
        ] {
            assert!(!shown.contains(secret), "{shown}");
        }
    }

    #[test]
    fn a_grant_expires_from_now() {
        let tokens = TokenSet::granted(&grant("ya29.a", Some("1//r"), 3599), at(10));
        assert_eq!(tokens.expires_at(), at(3609));
        assert_eq!(tokens.access_token(), "ya29.a");
        assert_eq!(tokens.refresh_token(), Some("1//r"));
    }

    #[test]
    fn a_refresh_keeps_the_refresh_token_unless_the_network_rotates_it() {
        let tokens = TokenSet::granted(&grant("ya29.a", Some("1//r"), 3599), at(0));
        let kept = tokens.refreshed(&grant("ya29.b", None, 3599), at(4000));
        assert_eq!(kept.access_token(), "ya29.b");
        assert_eq!(kept.refresh_token(), Some("1//r"));
        assert_eq!(kept.expires_at(), at(7599));
        let rotated = tokens.refreshed(&grant("ya29.c", Some("1//s"), 60), at(4000));
        assert_eq!(rotated.refresh_token(), Some("1//s"));
    }

    #[test]
    fn tokens_are_refreshed_within_the_margin_of_their_expiry() {
        let tokens = TokenSet::granted(&grant("ya29.a", Some("1//r"), 3600), at(0));
        let margin = TokenSet::REFRESH_MARGIN.as_secs();
        assert!(!tokens.needs_refresh(at(0)));
        assert!(!tokens.needs_refresh(at(3600 - margin - 1)));
        assert!(tokens.needs_refresh(at(3600 - margin)));
        assert!(tokens.needs_refresh(at(9000)));
    }

    #[test]
    fn tokens_round_trip_through_the_stored_form() {
        let tokens = TokenSet::granted(&grant("ya29.a-b_c.d", Some("1//0g-x_y"), 3599), at(0));
        assert_eq!(TokenSet::decode(&tokens.encode().unwrap()), Some(tokens));
        let no_refresh = TokenSet::granted(&grant("ya29.a", None, 3599), at(0));
        assert_eq!(
            TokenSet::decode(&no_refresh.encode().unwrap()),
            Some(no_refresh)
        );
    }

    #[test]
    fn stored_text_from_elsewhere_is_not_tokens() {
        for stored in [
            "",
            "ya29.a",
            "bardo-tokens-1\nsoon\nya29.a\n",
            "bardo-tokens-1\n1\n\n1//r",
            "bardo-tokens-1\n1\nya 29\n",
            "bardo-tokens-1\n1\nya29.a\n1//r\nextra",
            "bardo-tokens-9\n1\nya29.a\n1//r",
        ] {
            assert_eq!(TokenSet::decode(stored), None, "{stored:?}");
        }
    }

    #[test]
    fn a_token_set_over_the_store_limit_is_refused_not_cut() {
        let room = TokenSet::MAX_STORED_BYTES - "bardo-tokens-1\n1800003599\n\n".len();
        let fits = TokenSet::granted(&grant(&"a".repeat(room), None, 3599), at(0));
        assert_eq!(fits.encode().unwrap().len(), TokenSet::MAX_STORED_BYTES);
        let over = TokenSet::granted(&grant(&"a".repeat(room + 1), None, 3599), at(0));
        assert_eq!(
            over.encode().unwrap_err(),
            TokenSetTooLarge {
                bytes: TokenSet::MAX_STORED_BYTES + 1,
                max: TokenSet::MAX_STORED_BYTES
            }
        );
    }

    #[test]
    fn sensitive_parts_cover_both_tokens() {
        let tokens = TokenSet::granted(&grant("ya29.access", Some("1//refresh"), 1), at(0));
        assert_eq!(tokens.sensitive_parts(), ["ya29.access", "1//refresh"]);
    }

    #[test]
    fn statuses_round_trip_their_codes() {
        for status in [
            ConnectionStatus::Connected,
            ConnectionStatus::ReconnectNeeded,
        ] {
            assert_eq!(ConnectionStatus::from_code(status.code()), Some(status));
        }
        assert_eq!(ConnectionStatus::from_code("pending"), None);
    }

    #[test]
    fn a_hint_counts_characters() {
        assert_eq!(SecretText::new("abc").hint(), "…abc");
        assert_eq!(SecretText::new("xxxxéabc").hint(), "…éabc");
    }
}
