//! Network connection use cases (ADR-0008, PRD story 83): the profile's
//! app credentials per network, and signing a network account in, checking
//! it, and disconnecting it.
//!
//! Signing in waits for the user in the browser, so it is not a job: a job
//! resumes after a restart, and a consent cannot (its callback listener and
//! PKCE verifier end with the app). Like a key test, each step is prepared
//! here, run on a background thread and handed back.
//!
//! Tokens go only to the secret store and the redactor; the database keeps
//! the connection's state. A refresh the network refuses marks the account
//! "reconnect needed" instead of failing each job that needs it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bardo_domain::{
    AppCredentials, AppCredentialsFieldError, ConnectedIdentity, ConnectionSecrets,
    ConnectionStatus, ConsentCallback, ConsentError, ConsentPages, ConsentReceiver, ConsentRequest,
    Network, NetworkAccount, NetworkAccountId, NetworkConnection, NetworkConnectionRepository,
    NetworkSignIn, ProfileId, Redactor, RepositoryError, SecretStoreError, SignInFailure,
    SignInFailureKind, TokenSet, TokenSetTooLarge,
};

use crate::{Bardo, KeyState, Text};

/// How long a sign-in waits for the user to consent in the browser.
pub const CONSENT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    /// The typed app credentials break one or more field rules.
    #[error("invalid app credentials: {0:?}")]
    Invalid(Vec<AppCredentialsFieldError>),
    /// Bardo does not sign in to this network (yet).
    #[error("{0:?} has no sign-in")]
    NotOffered(Network),
    #[error("network account not found")]
    AccountNotFound,
    /// The network's app credentials are not saved in Settings.
    #[error("no app credentials for {0:?}")]
    NoAppCredentials(Network),
    #[error("the account is not connected")]
    NotConnected,
    /// The network refused a refresh; signing in again fixes it.
    #[error("the account needs to reconnect")]
    ReconnectNeeded,
    #[error(transparent)]
    Consent(#[from] ConsentError),
    /// The user unticked some of the permissions on the consent screen.
    #[error("not every scope was granted")]
    MissingScopes,
    /// A call to the network failed. The detail is already redacted.
    #[error(transparent)]
    SignIn(SignInFailure),
    #[error(transparent)]
    TooLarge(#[from] TokenSetTooLarge),
    #[error(transparent)]
    Store(SecretStoreError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ConnectionError {
    /// What the screen says, or `None` for field errors, which are shown
    /// next to their fields.
    pub fn message(&self) -> Option<Text> {
        Some(match self {
            ConnectionError::Invalid(_) => return None,
            ConnectionError::NotOffered(_) => Text::ConnectionNotOffered,
            ConnectionError::AccountNotFound => Text::NetworkAccountNotFound,
            ConnectionError::NoAppCredentials(_) => Text::ConnectionNeedsAppCredentials,
            ConnectionError::NotConnected => Text::ConnectionNotConnected,
            ConnectionError::ReconnectNeeded => Text::ConnectionReconnectHint,
            ConnectionError::Consent(ConsentError::Denied(_)) => Text::ConnectionDenied,
            ConnectionError::Consent(ConsentError::TimedOut) => Text::ConnectionTimedOut,
            ConnectionError::Consent(ConsentError::Cancelled) => Text::ConnectionCancelled,
            ConnectionError::Consent(ConsentError::Listen(_)) => Text::ConnectionListenFailed,
            ConnectionError::MissingScopes => Text::ConnectionMissingScopes,
            ConnectionError::SignIn(failure) => Text::SignInFailure(failure.kind),
            ConnectionError::TooLarge(_) => Text::ConnectionTokensTooLarge,
            ConnectionError::Store(_) => Text::ConnectionStoreFailed,
            ConnectionError::Repository(_) => Text::ConnectionNotSaved,
        })
    }

    pub fn field_errors(&self) -> &[AppCredentialsFieldError] {
        match self {
            ConnectionError::Invalid(errors) => errors,
            _ => &[],
        }
    }
}

/// What a network account card shows about its connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Bardo does not sign in to this network: export only.
    Unavailable,
    NotConnected,
    /// Waiting for the user's consent in the browser.
    Connecting,
    /// Signed in; the network's name for who the tokens act as.
    Connected {
        channel: String,
    },
    /// The network refused a refresh.
    ReconnectNeeded {
        channel: String,
    },
}

/// One row of Settings › Networks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCredentialsStatus {
    pub network: Network,
    /// Saved credentials show the secret's last four characters.
    pub state: KeyState,
}

/// How a disconnection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disconnected {
    /// False when the network could not be reached to revoke the tokens.
    /// Bardo forgot them anyway; the user can revoke access from the
    /// network's account settings.
    pub revoked: bool,
}

/// Everything that touches tokens, shared with background work: it reads
/// and writes the secret store and the connection state, and refreshes.
#[derive(Clone)]
pub(crate) struct Connections {
    owner: ProfileId,
    secrets: Arc<dyn ConnectionSecrets>,
    states: Arc<dyn NetworkConnectionRepository>,
    sign_ins: Arc<[Arc<dyn NetworkSignIn>]>,
    redactor: Redactor,
    /// One refresh at a time, so two callers never spend the same refresh
    /// token (networks that rotate it would refuse the second).
    refreshing: Arc<Mutex<()>>,
}

impl Connections {
    fn sign_in(&self, network: Network) -> Result<Arc<dyn NetworkSignIn>, ConnectionError> {
        self.sign_ins
            .iter()
            .find(|sign_in| sign_in.network() == network)
            .cloned()
            .ok_or(ConnectionError::NotOffered(network))
    }

    fn store_failure(&self, action: &str, error: SecretStoreError) -> ConnectionError {
        let cause = self.redactor.redact(&error.0.to_string());
        tracing::warn!(error = %cause, "could not {action} in the secret store");
        ConnectionError::Store(SecretStoreError(cause.into()))
    }

    /// A network call's failure, with every known secret masked, logged.
    fn sign_in_failure(&self, network: Network, failure: SignInFailure) -> ConnectionError {
        let failure = SignInFailure::new(failure.kind, self.redactor.redact(&failure.detail));
        tracing::warn!(
            network = network.code(),
            kind = ?failure.kind,
            detail = %failure.detail,
            "network sign-in call failed"
        );
        ConnectionError::SignIn(failure)
    }

    /// The saved app credentials, masked from then on.
    fn app_credentials(&self, network: Network) -> Result<AppCredentials, ConnectionError> {
        let credentials = self
            .secrets
            .app_credentials(self.owner, network)
            .map_err(|error| self.store_failure("read app credentials", error))?
            .ok_or(ConnectionError::NoAppCredentials(network))?;
        self.redactor.add_parts(&credentials.sensitive_parts());
        Ok(credentials)
    }

    fn state(&self, account: NetworkAccountId) -> Result<NetworkConnection, ConnectionError> {
        self.states
            .get(account)?
            .filter(|connection| connection.owner == self.owner)
            .ok_or(ConnectionError::NotConnected)
    }

    /// The account's tokens, fresh: refreshed first when they expire
    /// within `TokenSet::REFRESH_MARGIN`. A refused refresh marks the
    /// account "reconnect needed". Blocks on the network when refreshing.
    pub(crate) fn access_token(
        &self,
        account: &NetworkAccount,
    ) -> Result<TokenSet, ConnectionError> {
        let _one_at_a_time = self
            .refreshing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = self.state(account.id)?;
        if state.status == ConnectionStatus::ReconnectNeeded {
            return Err(ConnectionError::ReconnectNeeded);
        }
        let tokens = self
            .secrets
            .tokens(self.owner, account.id)
            .map_err(|error| self.store_failure("read tokens", error))?
            .ok_or(ConnectionError::NotConnected)?;
        self.redactor.add_parts(&tokens.sensitive_parts());
        let now = SystemTime::now();
        if !tokens.needs_refresh(now) {
            return Ok(tokens);
        }
        let Some(refresh_token) = tokens.refresh_token() else {
            self.mark_reconnect_needed(state)?;
            return Err(ConnectionError::ReconnectNeeded);
        };
        let sign_in = self.sign_in(account.network)?;
        let credentials = self.app_credentials(account.network)?;
        match sign_in.refresh(&credentials, refresh_token) {
            Ok(grant) => {
                let fresh = tokens.refreshed(&grant, now);
                self.redactor.add_parts(&fresh.sensitive_parts());
                fresh.encode()?;
                self.secrets
                    .set_tokens(self.owner, account.id, &fresh)
                    .map_err(|error| self.store_failure("save tokens", error))?;
                self.states.save(&NetworkConnection {
                    expires_at: fresh.expires_at(),
                    refreshed_at: Some(now),
                    ..state
                })?;
                tracing::info!(network = account.network.code(), "refreshed network tokens");
                Ok(fresh)
            }
            Err(failure) if failure.kind == SignInFailureKind::Refused => {
                let _ = self.sign_in_failure(account.network, failure);
                self.mark_reconnect_needed(state)?;
                Err(ConnectionError::ReconnectNeeded)
            }
            Err(failure) => Err(self.sign_in_failure(account.network, failure)),
        }
    }

    fn mark_reconnect_needed(&self, state: NetworkConnection) -> Result<(), ConnectionError> {
        tracing::warn!(account = %state.account, "the network refused a refresh; reconnect needed");
        self.states.save(&NetworkConnection {
            status: ConnectionStatus::ReconnectNeeded,
            ..state
        })?;
        Ok(())
    }

    /// Keeps a new connection: tokens to the secret store first, then the
    /// state, so the database never names a connection whose tokens are
    /// missing.
    fn keep(
        &self,
        account: &NetworkAccount,
        tokens: &TokenSet,
        scopes: Vec<String>,
        identity: ConnectedIdentity,
        now: SystemTime,
    ) -> Result<NetworkConnection, ConnectionError> {
        tokens.encode()?;
        self.secrets
            .set_tokens(self.owner, account.id, tokens)
            .map_err(|error| self.store_failure("save tokens", error))?;
        let connection = NetworkConnection {
            account: account.id,
            owner: self.owner,
            status: ConnectionStatus::Connected,
            identity,
            scopes,
            expires_at: tokens.expires_at(),
            connected_at: now,
            refreshed_at: None,
        };
        self.states.save(&connection)?;
        tracing::info!(
            network = account.network.code(),
            "connected network account"
        );
        Ok(connection)
    }

    /// Revokes tokens that will not be kept. Best effort: nothing else
    /// holds them, so a failure leaves them unused until they expire.
    fn discard(&self, network: Network, sign_in: &dyn NetworkSignIn, tokens: &TokenSet) {
        if let Err(failure) = sign_in.revoke(tokens) {
            let _ = self.sign_in_failure(network, failure);
        }
    }

    fn disconnect(&self, account: &NetworkAccount) -> Result<Disconnected, ConnectionError> {
        let sign_in = self.sign_in(account.network)?;
        let tokens = self
            .secrets
            .tokens(self.owner, account.id)
            .map_err(|error| self.store_failure("read tokens", error))?;
        let revoked = match &tokens {
            Some(tokens) => {
                self.redactor.add_parts(&tokens.sensitive_parts());
                match sign_in.revoke(tokens) {
                    Ok(()) => true,
                    Err(failure) => {
                        let _ = self.sign_in_failure(account.network, failure);
                        false
                    }
                }
            }
            None => true,
        };
        self.secrets
            .delete_tokens(self.owner, account.id)
            .map_err(|error| self.store_failure("delete tokens", error))?;
        self.states.delete(account.id)?;
        tracing::info!(
            network = account.network.code(),
            revoked,
            "disconnected network account"
        );
        Ok(Disconnected { revoked })
    }
}

/// A sign-in in progress for `Bardo`'s view.
struct Attempt {
    generation: u64,
    cancel: Arc<AtomicBool>,
}

/// What `Bardo` holds for connections.
pub(crate) struct ConnectionBook {
    connections: Connections,
    consent: Arc<dyn ConsentReceiver>,
    credentials: HashMap<Network, KeyState>,
    attempts: HashMap<NetworkAccountId, Attempt>,
    generations: u64,
}

impl ConnectionBook {
    /// The token side, for background work.
    pub(crate) fn connections(&self) -> &Connections {
        &self.connections
    }

    /// Reads which app credentials are saved and teaches the redactor each
    /// of them. A store that cannot be read shows the network as
    /// unreadable.
    pub(crate) fn load(
        owner: ProfileId,
        secrets: Arc<dyn ConnectionSecrets>,
        states: Arc<dyn NetworkConnectionRepository>,
        sign_ins: Vec<Arc<dyn NetworkSignIn>>,
        consent: Arc<dyn ConsentReceiver>,
        redactor: Redactor,
    ) -> Self {
        let credentials = Network::sign_in_networks()
            .map(|network| {
                let state = match secrets.app_credentials(owner, network) {
                    Ok(Some(credentials)) => {
                        redactor.add_parts(&credentials.sensitive_parts());
                        KeyState::Saved {
                            hint: credentials.hint(),
                        }
                    }
                    Ok(None) => KeyState::NotSet,
                    Err(error) => {
                        tracing::warn!(
                            network = network.code(),
                            error = %redactor.redact(&error.to_string()),
                            "could not read the app credentials"
                        );
                        KeyState::Unreadable
                    }
                };
                (network, state)
            })
            .collect();
        Self {
            connections: Connections {
                owner,
                secrets,
                states,
                sign_ins: sign_ins.into(),
                redactor,
                refreshing: Arc::default(),
            },
            consent,
            credentials,
            attempts: HashMap::new(),
            generations: 0,
        }
    }

    /// Whether the account has a connection, even one needing reconnect.
    pub(crate) fn is_connected(&self, account: NetworkAccountId) -> Result<bool, RepositoryError> {
        Ok(self.connections.states.get(account)?.is_some())
    }
}

/// A sign-in ready to run. The UI opens `consent_url` in the browser, runs
/// `run` on a background thread and hands the result to
/// `Bardo::record_connect`.
pub struct ConnectAttempt {
    account: NetworkAccount,
    generation: u64,
    callback: Box<dyn ConsentCallback>,
    request: ConsentRequest,
    credentials: AppCredentials,
    sign_in: Arc<dyn NetworkSignIn>,
    connections: Connections,
    cancel: Arc<AtomicBool>,
    pages: ConsentPages,
    timeout: Duration,
}

impl std::fmt::Debug for ConnectAttempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectAttempt")
            .field("account", &self.account.id)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// How a sign-in ended, for `Bardo::record_connect`.
#[derive(Debug)]
pub struct ConnectResult {
    pub account: NetworkAccountId,
    generation: u64,
    pub outcome: Result<ConnectedIdentity, ConnectionError>,
}

impl ConnectAttempt {
    /// The network's consent page, for the system browser.
    pub fn consent_url(&self) -> &str {
        &self.request.url
    }

    pub fn account(&self) -> NetworkAccountId {
        self.account.id
    }

    /// Waits for the consent, trades the code for tokens, looks up the
    /// connected channel and keeps the connection. Blocks for up to
    /// `CONSENT_TIMEOUT`, or until cancelled.
    pub fn run(mut self) -> ConnectResult {
        let callback = std::mem::replace(&mut self.callback, Box::new(Spent));
        let outcome = self.finish(callback);
        if let Err(error) = &outcome {
            tracing::info!(
                network = self.account.network.code(),
                error = %error,
                "sign-in did not finish"
            );
        }
        ConnectResult {
            account: self.account.id,
            generation: self.generation,
            outcome,
        }
    }

    fn finish(
        &self,
        callback: Box<dyn ConsentCallback>,
    ) -> Result<ConnectedIdentity, ConnectionError> {
        let network = self.account.network;
        let redirect_uri = callback.redirect_uri().to_owned();
        let code = callback.wait(&self.request.state, self.timeout, &self.cancel, &self.pages)?;
        let connections = &self.connections;
        connections.redactor.add_parts(&[code.expose()]);
        let grant = self
            .sign_in
            .exchange(
                &self.credentials,
                &code,
                &self.request.verifier,
                &redirect_uri,
            )
            .map_err(|failure| connections.sign_in_failure(network, failure))?;
        let now = SystemTime::now();
        let tokens = TokenSet::granted(&grant, now);
        connections.redactor.add_parts(&tokens.sensitive_parts());
        // From here on the tokens exist at the network: any way out that
        // does not keep them revokes them.
        let kept = (|| {
            let missing = self
                .sign_in
                .scopes()
                .iter()
                .any(|scope| !grant.scopes.iter().any(|granted| granted == scope));
            if missing {
                return Err(ConnectionError::MissingScopes);
            }
            let identity = self
                .sign_in
                .identity(tokens.access_token())
                .map_err(|failure| connections.sign_in_failure(network, failure))?;
            if self.cancel.load(Ordering::Relaxed) {
                return Err(ConnectionError::Consent(ConsentError::Cancelled));
            }
            connections
                .keep(&self.account, &tokens, grant.scopes.clone(), identity, now)
                .map(|connection| connection.identity)
        })();
        if kept.is_err() {
            connections.discard(network, self.sign_in.as_ref(), &tokens);
        }
        kept
    }
}

/// Stands in for the callback once `run` took it.
struct Spent;

impl ConsentCallback for Spent {
    fn redirect_uri(&self) -> &str {
        ""
    }

    fn wait(
        self: Box<Self>,
        _: &bardo_domain::SecretText,
        _: Duration,
        _: &AtomicBool,
        _: &ConsentPages,
    ) -> Result<bardo_domain::SecretText, ConsentError> {
        Err(ConsentError::Cancelled)
    }
}

/// A connection check ready to run on a background thread: refreshes the
/// tokens when due and reads the connected channel again.
pub struct ConnectionCheck {
    account: NetworkAccount,
    sign_in: Arc<dyn NetworkSignIn>,
    connections: Connections,
}

impl ConnectionCheck {
    pub fn account(&self) -> NetworkAccountId {
        self.account.id
    }

    pub fn run(self) -> Result<ConnectedIdentity, ConnectionError> {
        let connections = &self.connections;
        let network = self.account.network;
        let tokens = connections.access_token(&self.account)?;
        match self.sign_in.identity(tokens.access_token()) {
            Ok(identity) => {
                let state = connections.state(self.account.id)?;
                connections.states.save(&NetworkConnection {
                    identity: identity.clone(),
                    ..state
                })?;
                Ok(identity)
            }
            Err(failure) if failure.kind == SignInFailureKind::Refused => {
                connections.mark_reconnect_needed(connections.state(self.account.id)?)?;
                let _ = connections.sign_in_failure(network, failure);
                Err(ConnectionError::ReconnectNeeded)
            }
            Err(failure) => Err(connections.sign_in_failure(network, failure)),
        }
    }
}

/// A disconnection ready to run on a background thread: revokes the tokens
/// at the network and forgets them.
pub struct Disconnection {
    account: NetworkAccount,
    connections: Connections,
}

impl Disconnection {
    pub fn account(&self) -> NetworkAccountId {
        self.account.id
    }

    pub fn run(self) -> Result<Disconnected, ConnectionError> {
        self.connections.disconnect(&self.account)
    }
}

impl Bardo {
    /// The networks Bardo signs in to, with whether their app credentials
    /// are saved.
    pub fn app_credentials(&self) -> Vec<AppCredentialsStatus> {
        Network::sign_in_networks()
            .map(|network| AppCredentialsStatus {
                network,
                state: self
                    .connection_book
                    .credentials
                    .get(&network)
                    .cloned()
                    .unwrap_or(KeyState::NotSet),
            })
            .collect()
    }

    /// Validates and saves the network's app credentials, replacing earlier
    /// ones. Connections made with the earlier ones may need to reconnect.
    pub fn save_app_credentials(
        &mut self,
        network: Network,
        client_id: &str,
        client_secret: &str,
    ) -> Result<(), ConnectionError> {
        if !network.signs_in() {
            return Err(ConnectionError::NotOffered(network));
        }
        let credentials = AppCredentials::parse(network, client_id, client_secret)
            .map_err(ConnectionError::Invalid)?;
        let connections = &self.connection_book.connections;
        // Masked before the store sees them, in case its error quotes them.
        connections
            .redactor
            .add_parts(&credentials.sensitive_parts());
        connections
            .secrets
            .set_app_credentials(self.profile.id, network, &credentials)
            .map_err(|error| connections.store_failure("save app credentials", error))?;
        tracing::info!(network = network.code(), "saved app credentials");
        self.connection_book.credentials.insert(
            network,
            KeyState::Saved {
                hint: credentials.hint(),
            },
        );
        Ok(())
    }

    pub fn remove_app_credentials(&mut self, network: Network) -> Result<(), ConnectionError> {
        let connections = &self.connection_book.connections;
        connections
            .secrets
            .delete_app_credentials(self.profile.id, network)
            .map_err(|error| connections.store_failure("remove app credentials", error))?;
        tracing::info!(network = network.code(), "removed app credentials");
        self.connection_book
            .credentials
            .insert(network, KeyState::NotSet);
        Ok(())
    }

    /// The account's connection as its card shows it.
    pub fn connection_state(
        &self,
        account: &NetworkAccount,
    ) -> Result<ConnectionState, ConnectionError> {
        if !account.network.signs_in() {
            return Ok(ConnectionState::Unavailable);
        }
        if self.connection_book.attempts.contains_key(&account.id) {
            return Ok(ConnectionState::Connecting);
        }
        let book = &self.connection_book;
        Ok(match book.connections.states.get(account.id)? {
            Some(connection) => match connection.status {
                ConnectionStatus::Connected => ConnectionState::Connected {
                    channel: connection.identity.name,
                },
                ConnectionStatus::ReconnectNeeded => ConnectionState::ReconnectNeeded {
                    channel: connection.identity.name,
                },
            },
            None => ConnectionState::NotConnected,
        })
    }

    /// Prepares a sign-in (or a reconnect) of the account: starts the
    /// callback listener and builds the consent address. An earlier attempt
    /// for the same account is cancelled.
    pub fn connect(&mut self, id: NetworkAccountId) -> Result<ConnectAttempt, ConnectionError> {
        let account = self.connection_account(id)?;
        let connections = self.connection_book.connections.clone();
        let sign_in = connections.sign_in(account.network)?;
        let credentials = connections.app_credentials(account.network)?;
        let callback = self.connection_book.consent.listen()?;
        let request = sign_in.consent_request(&credentials, callback.redirect_uri());
        connections
            .redactor
            .add_parts(&[request.state.expose(), request.verifier.expose()]);
        self.cancel_connect(id);
        let book = &mut self.connection_book;
        book.generations += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        book.attempts.insert(
            id,
            Attempt {
                generation: book.generations,
                cancel: Arc::clone(&cancel),
            },
        );
        Ok(ConnectAttempt {
            account,
            generation: book.generations,
            callback,
            request,
            credentials,
            sign_in,
            connections,
            cancel,
            pages: ConsentPages {
                done: self.text(Text::ConsentPageDone).into_owned(),
                failed: self.text(Text::ConsentPageFailed).into_owned(),
            },
            timeout: CONSENT_TIMEOUT,
        })
    }

    /// Stops waiting for the account's consent. The card goes back to its
    /// earlier state at once.
    pub fn cancel_connect(&mut self, account: NetworkAccountId) {
        if let Some(attempt) = self.connection_book.attempts.remove(&account) {
            attempt.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Ends the account's "connecting" state with a finished sign-in.
    /// Returns `None` for an attempt that was cancelled or replaced while
    /// it ran: its outcome no longer describes the card.
    pub fn record_connect(
        &mut self,
        result: ConnectResult,
    ) -> Option<Result<ConnectedIdentity, ConnectionError>> {
        let book = &mut self.connection_book;
        let current = book
            .attempts
            .get(&result.account)
            .is_some_and(|attempt| attempt.generation == result.generation);
        if !current {
            return None;
        }
        book.attempts.remove(&result.account);
        Some(result.outcome)
    }

    /// Prepares a check of the account's connection.
    pub fn connection_check(
        &self,
        id: NetworkAccountId,
    ) -> Result<ConnectionCheck, ConnectionError> {
        let account = self.connection_account(id)?;
        let connections = self.connection_book.connections.clone();
        connections.state(id)?;
        Ok(ConnectionCheck {
            sign_in: connections.sign_in(account.network)?,
            account,
            connections,
        })
    }

    /// Prepares disconnecting the account. A sign-in in progress for it is
    /// cancelled.
    pub fn disconnection(
        &mut self,
        id: NetworkAccountId,
    ) -> Result<Disconnection, ConnectionError> {
        let account = self.connection_account(id)?;
        self.connection_book.connections.sign_in(account.network)?;
        self.cancel_connect(id);
        Ok(Disconnection {
            account,
            connections: self.connection_book.connections.clone(),
        })
    }

    fn connection_account(&self, id: NetworkAccountId) -> Result<NetworkAccount, ConnectionError> {
        let account = self
            .network_accounts
            .get(id)?
            .filter(|account| account.owner == self.profile.id)
            .ok_or(ConnectionError::AccountNotFound)?;
        if !account.network.signs_in() {
            return Err(ConnectionError::NotOffered(account.network));
        }
        Ok(account)
    }
}

/// Test doubles for the connection ports.
#[cfg(test)]
pub(crate) mod testing {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use bardo_domain::{SecretText, TokenGrant};

    use super::*;

    /// A receiver for tests that never sign in.
    pub(crate) struct NoConsent;

    impl ConsentReceiver for NoConsent {
        fn listen(&self) -> Result<Box<dyn ConsentCallback>, ConsentError> {
            Err(ConsentError::Listen("no browser in this test".into()))
        }
    }

    pub(crate) const REDIRECT: &str = "http://127.0.0.1:50505";

    /// Answers each wait with the next scripted outcome, and remembers the
    /// `state` it was asked to expect.
    #[derive(Default)]
    pub(crate) struct FakeConsent {
        pub(crate) outcomes: Mutex<VecDeque<Result<SecretText, ConsentError>>>,
        pub(crate) states: Arc<Mutex<Vec<String>>>,
    }

    impl FakeConsent {
        pub(crate) fn will(&self, outcome: Result<&str, ConsentError>) {
            self.outcomes
                .lock()
                .unwrap()
                .push_back(outcome.map(SecretText::new));
        }
    }

    struct FakeCallback {
        outcome: Result<SecretText, ConsentError>,
        states: Arc<Mutex<Vec<String>>>,
    }

    impl ConsentCallback for FakeCallback {
        fn redirect_uri(&self) -> &str {
            REDIRECT
        }

        fn wait(
            self: Box<Self>,
            state: &SecretText,
            _: Duration,
            cancel: &AtomicBool,
            _: &ConsentPages,
        ) -> Result<SecretText, ConsentError> {
            self.states.lock().unwrap().push(state.expose().to_owned());
            if cancel.load(Ordering::Relaxed) {
                return Err(ConsentError::Cancelled);
            }
            self.outcome
        }
    }

    impl ConsentReceiver for FakeConsent {
        fn listen(&self) -> Result<Box<dyn ConsentCallback>, ConsentError> {
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(SecretText::new("4/fake-code")));
            Ok(Box::new(FakeCallback {
                outcome,
                states: Arc::clone(&self.states),
            }))
        }
    }

    pub(crate) const ALL_SCOPES: [&str; 2] = ["scope.upload", "scope.manage"];

    /// One call to the fake network.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Call {
        Exchange {
            code: String,
            verifier: String,
            redirect: String,
            client_secret: String,
        },
        Refresh(String),
        Revoke(String),
        Identity(String),
    }

    /// A network whose answers tests script. Each exchange or refresh
    /// grants numbered tokens unless a failure is set.
    pub(crate) struct FakeSignIn {
        pub(crate) calls: Mutex<Vec<Call>>,
        pub(crate) issued: Mutex<u32>,
        /// Lifetime of each granted access token.
        pub(crate) expires_in: Mutex<Duration>,
        pub(crate) scopes: Mutex<Vec<String>>,
        /// Overrides the access token granted (to make it huge).
        pub(crate) access_token: Mutex<Option<String>>,
        pub(crate) exchange_failure: Mutex<Option<SignInFailure>>,
        pub(crate) refresh_failure: Mutex<Option<SignInFailure>>,
        pub(crate) revoke_failure: Mutex<Option<SignInFailure>>,
        pub(crate) identity: Mutex<Result<ConnectedIdentity, SignInFailure>>,
    }

    impl Default for FakeSignIn {
        fn default() -> Self {
            Self {
                calls: Mutex::default(),
                issued: Mutex::default(),
                expires_in: Mutex::new(Duration::from_secs(3600)),
                scopes: Mutex::new(ALL_SCOPES.map(str::to_owned).to_vec()),
                access_token: Mutex::default(),
                exchange_failure: Mutex::default(),
                refresh_failure: Mutex::default(),
                revoke_failure: Mutex::default(),
                identity: Mutex::new(Ok(ConnectedIdentity {
                    id: "UCfake-channel-0001".into(),
                    name: "Arquivos do Espaço".into(),
                })),
            }
        }
    }

    impl FakeSignIn {
        pub(crate) fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn grant(&self, with_refresh: bool) -> TokenGrant {
            let mut issued = self.issued.lock().unwrap();
            *issued += 1;
            let access = self
                .access_token
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| format!("ya29.fake-access-{:04}", *issued));
            TokenGrant {
                access_token: SecretText::new(access),
                refresh_token: with_refresh
                    .then(|| SecretText::new(format!("1//fake-refresh-{:04}", *issued))),
                expires_in: *self.expires_in.lock().unwrap(),
                scopes: self.scopes.lock().unwrap().clone(),
            }
        }
    }

    impl NetworkSignIn for FakeSignIn {
        fn network(&self) -> Network {
            Network::YouTube
        }

        fn scopes(&self) -> &'static [&'static str] {
            &ALL_SCOPES
        }

        fn consent_request(
            &self,
            credentials: &AppCredentials,
            redirect_uri: &str,
        ) -> ConsentRequest {
            ConsentRequest {
                url: format!(
                    "https://consent.test/?client_id={}&redirect_uri={redirect_uri}&state=state-0001",
                    credentials.client_id()
                ),
                state: SecretText::new("state-0001-abcdefgh"),
                verifier: SecretText::new("verifier-0001-abcdefgh"),
            }
        }

        fn exchange(
            &self,
            credentials: &AppCredentials,
            code: &SecretText,
            verifier: &SecretText,
            redirect_uri: &str,
        ) -> Result<TokenGrant, SignInFailure> {
            self.calls.lock().unwrap().push(Call::Exchange {
                code: code.expose().to_owned(),
                verifier: verifier.expose().to_owned(),
                redirect: redirect_uri.to_owned(),
                client_secret: credentials.client_secret().to_owned(),
            });
            match self.exchange_failure.lock().unwrap().clone() {
                Some(failure) => Err(failure),
                None => Ok(self.grant(true)),
            }
        }

        fn refresh(
            &self,
            _: &AppCredentials,
            refresh_token: &str,
        ) -> Result<TokenGrant, SignInFailure> {
            self.calls
                .lock()
                .unwrap()
                .push(Call::Refresh(refresh_token.to_owned()));
            match self.refresh_failure.lock().unwrap().clone() {
                Some(failure) => Err(failure),
                None => Ok(self.grant(false)),
            }
        }

        fn revoke(&self, tokens: &TokenSet) -> Result<(), SignInFailure> {
            let token = tokens.refresh_token().unwrap_or(tokens.access_token());
            self.calls
                .lock()
                .unwrap()
                .push(Call::Revoke(token.to_owned()));
            match self.revoke_failure.lock().unwrap().clone() {
                Some(failure) => Err(failure),
                None => Ok(()),
            }
        }

        fn identity(&self, access_token: &str) -> Result<ConnectedIdentity, SignInFailure> {
            self.calls
                .lock()
                .unwrap()
                .push(Call::Identity(access_token.to_owned()));
            self.identity.lock().unwrap().clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bardo_domain::{ChannelDraft, ContentLanguage, NetworkAccountDraft};
    use bardo_storage::{Database, MemorySecretStore};

    use super::testing::{ALL_SCOPES, Call, FakeConsent, FakeSignIn, REDIRECT};
    use super::*;
    use crate::{NetworkAccountError, Providers, Repositories, testing};

    // Fake values, split so secret scanners do not take them for real ones.
    const CLIENT_ID: &str = concat!("1234567890-abc123def456", ".apps.googleusercontent.com");
    const CLIENT_SECRET: &str = concat!("GOCSPX", "-client-secret-0001");

    struct Harness {
        db: Arc<Database>,
        secrets: Arc<MemorySecretStore>,
        sign_in: Arc<FakeSignIn>,
        consent: Arc<FakeConsent>,
    }

    impl Harness {
        fn new() -> Self {
            Self::over(Arc::new(Database::open_in_memory().unwrap()))
        }

        fn over(db: Arc<Database>) -> Self {
            Self {
                db,
                secrets: Arc::default(),
                sign_in: Arc::default(),
                consent: Arc::default(),
            }
        }

        fn start(&self) -> Bardo {
            let repositories = Repositories {
                connection_secrets: Arc::clone(&self.secrets) as _,
                ..Repositories::shared(Arc::clone(&self.db), Arc::new(MemorySecretStore::default()))
            };
            let providers = Providers {
                sign_ins: vec![Arc::clone(&self.sign_in) as _],
                consent: Arc::clone(&self.consent) as _,
                ..testing::providers()
            };
            Bardo::start(repositories, providers, Some("en-US")).unwrap()
        }

        /// A started app with a channel, its YouTube account and saved app
        /// credentials.
        fn ready(&self) -> (Bardo, NetworkAccount) {
            let mut app = self.start();
            let account = youtube_account(&app);
            app.save_app_credentials(Network::YouTube, CLIENT_ID, CLIENT_SECRET)
                .unwrap();
            (app, account)
        }

        fn tokens(&self, app: &Bardo, account: &NetworkAccount) -> Option<TokenSet> {
            self.secrets.tokens(app.profile().id, account.id).unwrap()
        }

        fn state(&self, account: &NetworkAccount) -> Option<NetworkConnection> {
            NetworkConnectionRepository::get(&*self.db, account.id).unwrap()
        }
    }

    fn youtube_account(app: &Bardo) -> NetworkAccount {
        let channel = app
            .create_channel(ChannelDraft {
                name: format!("Space {}", app.channels().unwrap().len()),
                language: ContentLanguage::Portuguese,
                ..ChannelDraft::default()
            })
            .unwrap()
            .id;
        app.add_network_account(
            channel,
            Network::YouTube,
            NetworkAccountDraft {
                handle: "arquivosdoespaco".into(),
                ..NetworkAccountDraft::default()
            },
        )
        .unwrap()
    }

    fn connect(
        app: &mut Bardo,
        account: &NetworkAccount,
    ) -> Result<ConnectedIdentity, ConnectionError> {
        let attempt = app.connect(account.id).unwrap();
        let result = attempt.run();
        app.record_connect(result).expect("the current attempt")
    }

    fn failure(kind: SignInFailureKind, detail: &str) -> SignInFailure {
        SignInFailure::new(kind, detail)
    }

    #[test]
    fn app_credentials_are_saved_masked_and_removed() {
        let harness = Harness::new();
        let mut app = harness.start();
        assert_eq!(
            app.app_credentials(),
            [AppCredentialsStatus {
                network: Network::YouTube,
                state: KeyState::NotSet
            }]
        );
        app.save_app_credentials(Network::YouTube, &format!(" {CLIENT_ID} "), CLIENT_SECRET)
            .unwrap();
        assert_eq!(
            app.app_credentials()[0].state,
            KeyState::Saved {
                hint: "…0001".into()
            }
        );
        let stored = harness
            .secrets
            .app_credentials(app.profile().id, Network::YouTube)
            .unwrap()
            .unwrap();
        assert_eq!(stored.client_id(), CLIENT_ID);
        assert_eq!(
            app.redactor()
                .redact(&format!("id={CLIENT_ID} secret={CLIENT_SECRET}")),
            "id=[redacted] secret=[redacted]"
        );

        app.remove_app_credentials(Network::YouTube).unwrap();
        assert_eq!(app.app_credentials()[0].state, KeyState::NotSet);
        assert_eq!(
            harness
                .secrets
                .app_credentials(app.profile().id, Network::YouTube)
                .unwrap(),
            None
        );
    }

    #[test]
    fn invalid_app_credentials_report_their_fields_and_save_nothing() {
        let harness = Harness::new();
        let mut app = harness.start();
        let error = app
            .save_app_credentials(Network::YouTube, "AIzaNotAClient", "")
            .unwrap_err();
        assert_eq!(
            error.field_errors(),
            [
                AppCredentialsFieldError::ClientIdInvalid,
                AppCredentialsFieldError::ClientSecretRequired
            ]
        );
        assert_eq!(error.message(), None);
        assert_eq!(app.app_credentials()[0].state, KeyState::NotSet);
        assert!(matches!(
            app.save_app_credentials(Network::X, CLIENT_ID, CLIENT_SECRET),
            Err(ConnectionError::NotOffered(Network::X))
        ));
    }

    #[test]
    fn saved_app_credentials_are_masked_from_the_next_start() {
        let harness = Harness::new();
        harness.ready();
        let app = harness.start();
        assert_eq!(
            app.redactor().redact(CLIENT_SECRET),
            bardo_domain::Redactor::MASK
        );
        assert!(matches!(
            app.app_credentials()[0].state,
            KeyState::Saved { .. }
        ));
    }

    #[test]
    fn connecting_needs_app_credentials_and_a_network_with_sign_in() {
        let harness = Harness::new();
        let mut app = harness.start();
        let account = youtube_account(&app);
        let error = app.connect(account.id).unwrap_err();
        assert!(matches!(
            error,
            ConnectionError::NoAppCredentials(Network::YouTube)
        ));
        assert_eq!(error.message(), Some(Text::ConnectionNeedsAppCredentials));

        let channel = account.channel;
        let x = app
            .add_network_account(
                channel,
                Network::X,
                NetworkAccountDraft {
                    handle: "space".into(),
                    ..NetworkAccountDraft::default()
                },
            )
            .unwrap();
        assert_eq!(
            app.connection_state(&x).unwrap(),
            ConnectionState::Unavailable
        );
        assert!(matches!(
            app.connect(x.id),
            Err(ConnectionError::NotOffered(Network::X))
        ));
        assert!(matches!(
            app.connect(NetworkAccountId::new()),
            Err(ConnectionError::AccountNotFound)
        ));
    }

    #[test]
    fn a_sign_in_shows_connecting_then_the_connected_channel() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::NotConnected
        );

        let attempt = app.connect(account.id).unwrap();
        assert!(attempt.consent_url().contains(CLIENT_ID));
        assert!(attempt.consent_url().contains(REDIRECT));
        assert!(!attempt.consent_url().contains(CLIENT_SECRET));
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::Connecting
        );

        let result = attempt.run();
        assert_eq!(
            app.record_connect(result).unwrap().unwrap().name,
            "Arquivos do Espaço"
        );
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::Connected {
                channel: "Arquivos do Espaço".into()
            }
        );
        // The callback expected this consent's state; the exchange proved
        // its verifier from the callback's address.
        assert_eq!(
            *harness.consent.states.lock().unwrap(),
            ["state-0001-abcdefgh"]
        );
        assert_eq!(
            harness.sign_in.calls(),
            [
                Call::Exchange {
                    code: "4/fake-code".into(),
                    verifier: "verifier-0001-abcdefgh".into(),
                    redirect: REDIRECT.into(),
                    client_secret: CLIENT_SECRET.into(),
                },
                Call::Identity("ya29.fake-access-0001".into()),
            ]
        );
        let tokens = harness.tokens(&app, &account).unwrap();
        assert_eq!(tokens.access_token(), "ya29.fake-access-0001");
        assert_eq!(tokens.refresh_token(), Some("1//fake-refresh-0001"));
        let state = harness.state(&account).unwrap();
        assert_eq!(state.status, ConnectionStatus::Connected);
        assert_eq!(state.identity.id, "UCfake-channel-0001");
        assert_eq!(state.scopes, ALL_SCOPES);
        assert_eq!(state.expires_at, tokens.expires_at());
        assert_eq!(state.refreshed_at, None);
        // Every secret of the sign-in is masked.
        let masked = app.redactor().redact(
            "ya29.fake-access-0001 1//fake-refresh-0001 4/fake-code verifier-0001-abcdefgh",
        );
        assert_eq!(masked, "[redacted] [redacted] [redacted] [redacted]");
    }

    #[test]
    fn the_connection_survives_a_restart() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        connect(&mut app, &account).unwrap();
        drop(app);
        let app = harness.start();
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::Connected {
                channel: "Arquivos do Espaço".into()
            }
        );
    }

    #[test]
    fn a_declined_or_timed_out_consent_keeps_nothing() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        harness
            .consent
            .will(Err(ConsentError::Denied("access_denied".into())));
        let error = connect(&mut app, &account).unwrap_err();
        assert_eq!(error.message(), Some(Text::ConnectionDenied));
        harness.consent.will(Err(ConsentError::TimedOut));
        let error = connect(&mut app, &account).unwrap_err();
        assert_eq!(error.message(), Some(Text::ConnectionTimedOut));

        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::NotConnected
        );
        assert!(harness.sign_in.calls().is_empty());
        assert_eq!(harness.tokens(&app, &account), None);
    }

    #[test]
    fn unticked_permissions_refuse_the_connection_and_revoke_what_was_granted() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        *harness.sign_in.scopes.lock().unwrap() = vec!["scope.upload".into()];
        let error = connect(&mut app, &account).unwrap_err();
        assert!(matches!(error, ConnectionError::MissingScopes));
        assert_eq!(error.message(), Some(Text::ConnectionMissingScopes));
        assert_eq!(
            harness.sign_in.calls().last(),
            Some(&Call::Revoke("1//fake-refresh-0001".into()))
        );
        assert_eq!(harness.tokens(&app, &account), None);
        assert_eq!(harness.state(&account), None);
    }

    #[test]
    fn a_google_account_without_a_channel_is_not_connected() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        *harness.sign_in.identity.lock().unwrap() =
            Err(failure(SignInFailureKind::NoChannel, "no channel"));
        let error = connect(&mut app, &account).unwrap_err();
        assert_eq!(
            error.message(),
            Some(Text::SignInFailure(SignInFailureKind::NoChannel))
        );
        assert!(matches!(
            harness.sign_in.calls().last(),
            Some(Call::Revoke(_))
        ));
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::NotConnected
        );
    }

    #[test]
    fn a_token_set_over_the_store_limit_is_refused_and_revoked() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        *harness.sign_in.access_token.lock().unwrap() =
            Some("a".repeat(TokenSet::MAX_STORED_BYTES));
        let error = connect(&mut app, &account).unwrap_err();
        assert!(matches!(error, ConnectionError::TooLarge(_)));
        assert_eq!(error.message(), Some(Text::ConnectionTokensTooLarge));
        assert!(matches!(
            harness.sign_in.calls().last(),
            Some(Call::Revoke(_))
        ));
        assert_eq!(harness.tokens(&app, &account), None);
        assert_eq!(harness.state(&account), None);
    }

    #[test]
    fn a_cancelled_sign_in_is_forgotten_at_once() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        let attempt = app.connect(account.id).unwrap();
        app.cancel_connect(account.id);
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::NotConnected
        );
        let result = attempt.run();
        assert!(matches!(
            result.outcome,
            Err(ConnectionError::Consent(ConsentError::Cancelled))
        ));
        assert!(app.record_connect(result).is_none());
        assert_eq!(harness.tokens(&app, &account), None);
    }

    #[test]
    fn a_new_attempt_replaces_the_earlier_one() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        let first = app.connect(account.id).unwrap();
        let second = app.connect(account.id).unwrap();
        assert!(app.record_connect(first.run()).is_none());
        assert!(app.record_connect(second.run()).unwrap().is_ok());
    }

    #[test]
    fn a_check_refreshes_only_within_the_margin_of_expiry() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        connect(&mut app, &account).unwrap();

        let identity = app.connection_check(account.id).unwrap().run().unwrap();
        assert_eq!(identity.name, "Arquivos do Espaço");
        assert!(
            !harness
                .sign_in
                .calls()
                .iter()
                .any(|call| matches!(call, Call::Refresh(_)))
        );

        // Tokens that expire within the margin are refreshed first.
        *harness.sign_in.expires_in.lock().unwrap() =
            TokenSet::REFRESH_MARGIN - Duration::from_secs(1);
        connect(&mut app, &account).unwrap();
        *harness.sign_in.expires_in.lock().unwrap() = Duration::from_secs(3600);
        *harness.sign_in.identity.lock().unwrap() = Ok(ConnectedIdentity {
            id: "UCfake-channel-0001".into(),
            name: "Space Archives".into(),
        });
        app.connection_check(account.id).unwrap().run().unwrap();
        assert!(
            harness
                .sign_in
                .calls()
                .contains(&Call::Refresh("1//fake-refresh-0002".into()))
        );
        let tokens = harness.tokens(&app, &account).unwrap();
        assert_eq!(tokens.access_token(), "ya29.fake-access-0003");
        assert_eq!(tokens.refresh_token(), Some("1//fake-refresh-0002"));
        let state = harness.state(&account).unwrap();
        assert!(state.refreshed_at.is_some());
        assert_eq!(state.expires_at, tokens.expires_at());
        // The check also follows a renamed channel.
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::Connected {
                channel: "Space Archives".into()
            }
        );
    }

    #[test]
    fn a_refused_refresh_needs_a_reconnect_and_a_reconnect_fixes_it() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        *harness.sign_in.expires_in.lock().unwrap() = Duration::from_secs(1);
        connect(&mut app, &account).unwrap();
        *harness.sign_in.refresh_failure.lock().unwrap() = Some(failure(
            SignInFailureKind::Refused,
            "HTTP 400: invalid_grant: Token has been expired or revoked.",
        ));

        let error = app.connection_check(account.id).unwrap().run().unwrap_err();
        assert!(matches!(error, ConnectionError::ReconnectNeeded));
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::ReconnectNeeded {
                channel: "Arquivos do Espaço".into()
            }
        );
        // Later work fails fast without asking the network again.
        let refreshes = harness.sign_in.calls().len();
        assert!(matches!(
            app.connection_check(account.id).unwrap().run(),
            Err(ConnectionError::ReconnectNeeded)
        ));
        assert_eq!(harness.sign_in.calls().len(), refreshes);

        // It survives a restart, and signing in again clears it.
        drop(app);
        let mut app = harness.start();
        assert!(matches!(
            app.connection_state(&account).unwrap(),
            ConnectionState::ReconnectNeeded { .. }
        ));
        *harness.sign_in.expires_in.lock().unwrap() = Duration::from_secs(3600);
        connect(&mut app, &account).unwrap();
        assert!(matches!(
            app.connection_state(&account).unwrap(),
            ConnectionState::Connected { .. }
        ));
    }

    #[test]
    fn other_refresh_failures_keep_the_connection() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        *harness.sign_in.expires_in.lock().unwrap() = Duration::from_secs(1);
        connect(&mut app, &account).unwrap();
        *harness.sign_in.refresh_failure.lock().unwrap() =
            Some(failure(SignInFailureKind::Unreachable, "dns error"));
        let error = app.connection_check(account.id).unwrap().run().unwrap_err();
        assert_eq!(
            error.message(),
            Some(Text::SignInFailure(SignInFailureKind::Unreachable))
        );
        assert!(matches!(
            app.connection_state(&account).unwrap(),
            ConnectionState::Connected { .. }
        ));
    }

    #[test]
    fn a_rejected_access_token_needs_a_reconnect() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        connect(&mut app, &account).unwrap();
        *harness.sign_in.identity.lock().unwrap() =
            Err(failure(SignInFailureKind::Refused, "HTTP 401"));
        assert!(matches!(
            app.connection_check(account.id).unwrap().run(),
            Err(ConnectionError::ReconnectNeeded)
        ));
        assert!(matches!(
            app.connection_state(&account).unwrap(),
            ConnectionState::ReconnectNeeded { .. }
        ));
    }

    #[test]
    fn checking_an_account_that_is_not_connected_says_so() {
        let harness = Harness::new();
        let (app, account) = harness.ready();
        assert!(matches!(
            app.connection_check(account.id),
            Err(ConnectionError::NotConnected)
        ));
    }

    #[test]
    fn disconnecting_revokes_and_forgets_the_tokens() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        connect(&mut app, &account).unwrap();
        let disconnected = app.disconnection(account.id).unwrap().run().unwrap();
        assert_eq!(disconnected, Disconnected { revoked: true });
        assert_eq!(
            harness.sign_in.calls().last(),
            Some(&Call::Revoke("1//fake-refresh-0001".into()))
        );
        assert_eq!(harness.tokens(&app, &account), None);
        assert_eq!(harness.state(&account), None);
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::NotConnected
        );
    }

    #[test]
    fn a_reconnect_needed_account_can_be_disconnected() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        *harness.sign_in.expires_in.lock().unwrap() = Duration::from_secs(1);
        connect(&mut app, &account).unwrap();
        *harness.sign_in.refresh_failure.lock().unwrap() =
            Some(failure(SignInFailureKind::Refused, "invalid_grant"));
        let _ = app.connection_check(account.id).unwrap().run();
        app.disconnection(account.id).unwrap().run().unwrap();
        assert_eq!(
            app.connection_state(&account).unwrap(),
            ConnectionState::NotConnected
        );
    }

    #[test]
    fn a_network_out_of_reach_still_disconnects_here() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        connect(&mut app, &account).unwrap();
        *harness.sign_in.revoke_failure.lock().unwrap() =
            Some(failure(SignInFailureKind::Unreachable, "dns error"));
        let disconnected = app.disconnection(account.id).unwrap().run().unwrap();
        assert_eq!(disconnected, Disconnected { revoked: false });
        assert_eq!(harness.tokens(&app, &account), None);
        assert_eq!(harness.state(&account), None);
    }

    #[test]
    fn disconnecting_cancels_a_sign_in_in_progress() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        let attempt = app.connect(account.id).unwrap();
        app.disconnection(account.id).unwrap().run().unwrap();
        assert!(app.record_connect(attempt.run()).is_none());
        assert_eq!(harness.state(&account), None);
    }

    #[test]
    fn a_connected_account_must_be_disconnected_before_removal() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        connect(&mut app, &account).unwrap();
        let error = app.remove_network_account(account.id).unwrap_err();
        assert!(matches!(error, NetworkAccountError::StillConnected));
        assert_eq!(
            error.form_message(),
            Some(Text::NetworkAccountStillConnected)
        );
        app.disconnection(account.id).unwrap().run().unwrap();
        app.remove_network_account(account.id).unwrap();
    }

    #[test]
    fn errors_never_carry_a_token() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        *harness.sign_in.expires_in.lock().unwrap() = Duration::from_secs(1);
        connect(&mut app, &account).unwrap();
        *harness.sign_in.refresh_failure.lock().unwrap() = Some(failure(
            SignInFailureKind::NetworkDown,
            concat!(
                "HTTP 503 for refresh_token=1//fake-refresh-0001 with secret GOCSPX",
                "-client-secret-0001"
            ),
        ));
        let error = app.connection_check(account.id).unwrap().run().unwrap_err();
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains("1//fake-refresh-0001"), "{shown}");
        assert!(!shown.contains(CLIENT_SECRET), "{shown}");
        assert!(shown.contains("[redacted]"), "{shown}");
    }

    /// Reads every byte SQLite wrote: the database and its write-ahead log.
    fn database_bytes(path: &Path) -> Vec<u8> {
        let mut bytes = std::fs::read(path).unwrap();
        if let Ok(wal) = std::fs::read(path.with_extension("db-wal")) {
            bytes.extend(wal);
        }
        bytes
    }

    fn contains(haystack: &[u8], needle: &str) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    }

    #[test]
    fn tokens_and_app_credentials_never_reach_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let harness = Harness::over(Arc::new(Database::open(&path).unwrap()));
        let (mut app, account) = harness.ready();
        *harness.sign_in.expires_in.lock().unwrap() = Duration::from_secs(1);
        connect(&mut app, &account).unwrap();
        *harness.sign_in.expires_in.lock().unwrap() = Duration::from_secs(3600);
        app.connection_check(account.id).unwrap().run().unwrap();
        let tokens = harness.tokens(&app, &account).unwrap();
        assert_eq!(tokens.access_token(), "ya29.fake-access-0002");

        let written = database_bytes(&path);
        assert!(
            contains(&written, "UCfake-channel-0001"),
            "the state is there"
        );
        for secret in [
            "ya29.fake-access-0001",
            "ya29.fake-access-0002",
            "1//fake-refresh-0001",
            "4/fake-code",
            "verifier-0001-abcdefgh",
            CLIENT_ID,
            CLIENT_SECRET,
        ] {
            assert!(!contains(&written, secret), "{secret} reached SQLite");
        }
    }

    #[test]
    fn the_consent_pages_follow_the_interface_language() {
        let harness = Harness::new();
        let (mut app, account) = harness.ready();
        app.set_ui_language(bardo_domain::UiLanguage::PtBr).unwrap();
        let attempt = app.connect(account.id).unwrap();
        assert_eq!(attempt.pages.done, app.text(Text::ConsentPageDone));
        assert_ne!(
            attempt.pages.done,
            crate::Catalog::load(bardo_domain::UiLanguage::EnUs).get(Text::ConsentPageDone)
        );
    }
}
