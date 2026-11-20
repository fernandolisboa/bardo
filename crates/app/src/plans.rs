//! Plans Bardo signs in to instead of saving a key (ADR-0028): the
//! Higgsfield plan, whose credits pay for clips through Higgsfield's MCP
//! server.
//!
//! Signing in waits for the user, so like a network's sign-in it is not a
//! job: each step is prepared here, run on a background thread and handed
//! back. It has one step more than a network's: Bardo registers itself as
//! a client first (dynamic client registration), which calls the provider,
//! so the consent address is only known once that ran.
//!
//! The client id and the tokens go only to the secret store and the
//! redactor. A refresh the provider refuses marks the plan "sign in
//! again" for the rest of the session instead of failing each clip.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, ConnectionSecrets, ConsentCallback, ConsentError, ConsentPages, ConsentReceiver,
    ConsentRequest, PlanAccount, PlanSession, PlanSignIn, ProfileId, Provider, Redactor,
    SecretStoreError, SignInFailure, SignInFailureKind, TokenSet, TokenSetTooLarge,
};

use crate::connections::CONSENT_TIMEOUT;
use crate::{Bardo, KeyState, Text};

/// How long before it expires an access token is renewed. A clip asks for
/// the token on every step; renewing well ahead leaves room to try again
/// when a renewal fails while the token still works.
const RENEW_AHEAD: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    /// Bardo does not sign in to this provider.
    #[error("{0:?} has no sign-in")]
    NotOffered(Provider),
    #[error("the plan is not signed in")]
    NotConnected(Provider),
    /// The provider refused a refresh; signing in again fixes it.
    #[error("the plan needs to sign in again")]
    ReconnectNeeded(Provider),
    #[error(transparent)]
    Consent(#[from] ConsentError),
    /// A call to the provider failed. The detail is already redacted.
    #[error("{1}")]
    SignIn(Provider, SignInFailure),
    #[error(transparent)]
    TooLarge(#[from] TokenSetTooLarge),
    #[error(transparent)]
    Store(SecretStoreError),
}

impl PlanError {
    /// What the settings screen says.
    pub fn message(&self) -> Text {
        match self {
            PlanError::NotOffered(_) => Text::ConnectionNotOffered,
            PlanError::NotConnected(_) => Text::PlanNotSignedIn,
            PlanError::ReconnectNeeded(_) => Text::PlanSignInAgainHint,
            PlanError::Consent(ConsentError::Denied(_)) => Text::ConnectionDenied,
            PlanError::Consent(ConsentError::TimedOut) => Text::ConnectionTimedOut,
            PlanError::Consent(ConsentError::Cancelled) => Text::ConnectionCancelled,
            PlanError::Consent(ConsentError::Listen(_)) => Text::ConnectionListenFailed,
            PlanError::Consent(ConsentError::PortInUse(_)) => Text::ConnectionPortInUse,
            PlanError::SignIn(_, failure) => Text::PlanFailure(failure.kind),
            PlanError::TooLarge(_) => Text::ConnectionTokensTooLarge,
            PlanError::Store(_) => Text::ConnectionStoreFailed,
        }
    }

    /// Whether trying again later may work: no answer, or the provider
    /// failing or busy on its side.
    pub(crate) fn is_transient(&self) -> bool {
        matches!(
            self,
            PlanError::SignIn(
                _,
                SignInFailure {
                    kind: SignInFailureKind::Unreachable
                        | SignInFailureKind::NetworkDown
                        | SignInFailureKind::LimitReached,
                    ..
                }
            )
        )
    }
}

/// Whether the profile is signed in to a plan, as far as the app knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanState {
    NotConnected,
    Connected,
    /// The provider refused to renew the sign-in.
    ReconnectNeeded,
    /// The secret store could not be read at startup.
    Unreadable,
}

/// One plan's card in Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStatus {
    pub provider: Provider,
    pub state: PlanState,
    /// The plan and its credits as last read in this session.
    pub account: Option<PlanAccount>,
    /// A sign-in waits for the browser.
    pub connecting: bool,
}

/// Everything that touches a plan's tokens, shared with the clip jobs: it
/// reads and writes the secret store and refreshes.
#[derive(Clone)]
pub(crate) struct PlanSessions {
    owner: ProfileId,
    secrets: Arc<dyn ConnectionSecrets>,
    sign_ins: Arc<[Arc<dyn PlanSignIn>]>,
    redactor: Redactor,
    /// One refresh at a time, so two clips never spend the same refresh
    /// token (a provider that rotates it would refuse the second).
    refreshing: Arc<Mutex<()>>,
    /// Plans whose refresh the provider refused in this session.
    reconnect: Arc<Mutex<HashSet<Provider>>>,
}

impl PlanSessions {
    fn sign_in(&self, provider: Provider) -> Result<Arc<dyn PlanSignIn>, PlanError> {
        self.sign_ins
            .iter()
            .find(|sign_in| sign_in.provider() == provider)
            .cloned()
            .ok_or(PlanError::NotOffered(provider))
    }

    fn store_failure(&self, action: &str, error: SecretStoreError) -> PlanError {
        let cause = self.redactor.redact(&error.0.to_string());
        tracing::warn!(error = %cause, "could not {action} in the secret store");
        PlanError::Store(SecretStoreError(cause.into()))
    }

    /// A call's failure, with every known secret masked, logged.
    fn sign_in_failure(&self, provider: Provider, failure: SignInFailure) -> PlanError {
        let failure = SignInFailure::new(failure.kind, self.redactor.redact(&failure.detail));
        tracing::warn!(
            provider = provider.code(),
            kind = ?failure.kind,
            detail = %failure.detail,
            "plan sign-in call failed"
        );
        PlanError::SignIn(provider, failure)
    }

    /// The stored session, masked from then on.
    fn session(&self, provider: Provider) -> Result<Option<PlanSession>, PlanError> {
        let session = self
            .secrets
            .plan_session(self.owner, provider)
            .map_err(|error| self.store_failure("read the plan's sign-in", error))?;
        if let Some(session) = &session {
            self.redactor.add_parts(&session.sensitive_parts());
        }
        Ok(session)
    }

    fn needs_reconnect(&self, provider: Provider) -> bool {
        self.reconnect
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&provider)
    }

    fn set_reconnect(&self, provider: Provider, needed: bool) {
        let mut reconnect = self
            .reconnect
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if needed {
            reconnect.insert(provider);
        } else {
            reconnect.remove(&provider);
        }
    }

    /// The access token as a key the clip adapter sends, fresh: refreshed
    /// first when it expires within `RENEW_AHEAD`. A refused
    /// refresh marks the plan "sign in again". Blocks on the provider when
    /// refreshing.
    pub(crate) fn access_token(&self, provider: Provider) -> Result<ApiKey, PlanError> {
        let _one_at_a_time = self
            .refreshing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.needs_reconnect(provider) {
            return Err(PlanError::ReconnectNeeded(provider));
        }
        let session = self
            .session(provider)?
            .ok_or(PlanError::NotConnected(provider))?;
        let tokens = session.tokens();
        let now = SystemTime::now();
        if !tokens.needs_refresh_within(now, RENEW_AHEAD) {
            return self.key(provider, tokens);
        }
        let refused = |failure: SignInFailure| {
            let _ = self.sign_in_failure(provider, failure);
            self.set_reconnect(provider, true);
            Err(PlanError::ReconnectNeeded(provider))
        };
        let Some(refresh_token) = tokens.refresh_token() else {
            if !tokens.needs_refresh(now) {
                return self.key(provider, tokens);
            }
            return refused(SignInFailure::new(
                SignInFailureKind::Refused,
                "the sign-in expired and has no refresh token",
            ));
        };
        let sign_in = self.sign_in(provider)?;
        match sign_in.refresh(session.client_id(), refresh_token) {
            Ok(grant) => {
                let fresh = tokens.refreshed(&grant, now);
                self.redactor.add_parts(&fresh.sensitive_parts());
                let renewed = session.with_tokens(fresh);
                renewed.encode()?;
                self.secrets
                    .set_plan_session(self.owner, provider, &renewed)
                    .map_err(|error| self.store_failure("save the plan's sign-in", error))?;
                tracing::info!(provider = provider.code(), "refreshed plan tokens");
                self.key(provider, renewed.tokens())
            }
            Err(failure) if failure.kind == SignInFailureKind::Refused => refused(failure),
            // Still valid for a while: renewing early failed, so the next
            // use tries again.
            Err(failure) if !tokens.needs_refresh(now) => {
                let _ = self.sign_in_failure(provider, failure);
                self.key(provider, tokens)
            }
            Err(failure) => Err(self.sign_in_failure(provider, failure)),
        }
    }

    fn key(&self, provider: Provider, tokens: &TokenSet) -> Result<ApiKey, PlanError> {
        ApiKey::access_token(tokens.access_token()).map_err(|_| {
            self.sign_in_failure(
                provider,
                SignInFailure::new(
                    SignInFailureKind::Unexpected,
                    "the access token has characters a header cannot carry",
                ),
            )
        })
    }

    /// Keeps a new sign-in, replacing any earlier one.
    fn keep(&self, provider: Provider, session: &PlanSession) -> Result<(), PlanError> {
        session.encode()?;
        self.secrets
            .set_plan_session(self.owner, provider, session)
            .map_err(|error| self.store_failure("save the plan's sign-in", error))?;
        self.set_reconnect(provider, false);
        tracing::info!(provider = provider.code(), "signed in to the plan");
        Ok(())
    }

    fn forget(&self, provider: Provider) -> Result<(), PlanError> {
        self.secrets
            .delete_plan_session(self.owner, provider)
            .map_err(|error| self.store_failure("delete the plan's sign-in", error))?;
        self.set_reconnect(provider, false);
        tracing::info!(provider = provider.code(), "signed out of the plan");
        Ok(())
    }
}

/// A sign-in in progress for `Bardo`'s view.
struct Attempt {
    generation: u64,
    cancel: Arc<AtomicBool>,
}

/// What `Bardo` holds for plans.
pub(crate) struct PlanBook {
    sessions: PlanSessions,
    consent: Arc<dyn ConsentReceiver>,
    states: HashMap<Provider, PlanState>,
    accounts: HashMap<Provider, PlanAccount>,
    attempts: HashMap<Provider, Attempt>,
    generations: u64,
}

impl PlanBook {
    /// Reads which plans are signed in and teaches the redactor their
    /// tokens. A store that cannot be read shows the plan as unreadable.
    pub(crate) fn load(
        owner: ProfileId,
        secrets: Arc<dyn ConnectionSecrets>,
        sign_ins: Vec<Arc<dyn PlanSignIn>>,
        consent: Arc<dyn ConsentReceiver>,
        redactor: Redactor,
    ) -> Self {
        let sessions = PlanSessions {
            owner,
            secrets,
            sign_ins: sign_ins.into(),
            redactor,
            refreshing: Arc::default(),
            reconnect: Arc::default(),
        };
        let states = sessions
            .sign_ins
            .iter()
            .map(|sign_in| {
                let provider = sign_in.provider();
                let state = match sessions.session(provider) {
                    Ok(Some(_)) => PlanState::Connected,
                    Ok(None) => PlanState::NotConnected,
                    Err(_) => PlanState::Unreadable,
                };
                (provider, state)
            })
            .collect();
        Self {
            sessions,
            consent,
            states,
            accounts: HashMap::new(),
            attempts: HashMap::new(),
            generations: 0,
        }
    }

    /// The token side, for the clip jobs.
    pub(crate) fn sessions(&self) -> &PlanSessions {
        &self.sessions
    }

    fn state(&self, provider: Provider) -> PlanState {
        if self.sessions.needs_reconnect(provider) {
            return PlanState::ReconnectNeeded;
        }
        self.states
            .get(&provider)
            .copied()
            .unwrap_or(PlanState::NotConnected)
    }

    /// What the plan stands for where features ask for a key: signed in
    /// is a saved key (its hint the plan's name), signed out or refused is
    /// none.
    pub(crate) fn key_state(&self, provider: Provider) -> KeyState {
        match self.state(provider) {
            PlanState::Connected => KeyState::Saved {
                hint: self
                    .accounts
                    .get(&provider)
                    .map(|account| account.plan.clone())
                    .unwrap_or_default(),
            },
            PlanState::NotConnected | PlanState::ReconnectNeeded => KeyState::NotSet,
            PlanState::Unreadable => KeyState::Unreadable,
        }
    }
}

/// A plan sign-in ready to start. `register` calls the provider and blocks,
/// so the UI runs it on a background thread, opens the consent page it
/// gives in the browser, runs the consent's `run` on a background thread
/// too and hands the result to `Bardo::record_plan_connect`.
pub struct PlanConnect {
    provider: Provider,
    generation: u64,
    callback: Box<dyn ConsentCallback>,
    sign_in: Arc<dyn PlanSignIn>,
    sessions: PlanSessions,
    cancel: Arc<AtomicBool>,
    pages: ConsentPages,
    timeout: Duration,
}

impl std::fmt::Debug for PlanConnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanConnect")
            .field("provider", &self.provider)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl PlanConnect {
    pub fn provider(&self) -> Provider {
        self.provider
    }

    /// Registers Bardo as a client that returns to the listener, and
    /// builds the consent address. The `Err` is the attempt's end, for
    /// `Bardo::record_plan_connect`.
    pub fn register(self) -> Result<PlanConsent, PlanConnectResult> {
        let provider = self.provider;
        let redirect_uri = self.callback.redirect_uri().to_owned();
        let registered = self
            .sign_in
            .register(&redirect_uri)
            .map_err(|failure| self.sessions.sign_in_failure(provider, failure));
        let client_id = match registered {
            Ok(client_id) if !self.cancel.load(Ordering::Relaxed) => client_id,
            Ok(_) => return Err(self.end(Err(ConsentError::Cancelled.into()))),
            Err(error) => return Err(self.end(Err(error))),
        };
        let request = self.sign_in.consent_request(&client_id, &redirect_uri);
        self.sessions
            .redactor
            .add_parts(&[request.state.expose(), request.verifier.expose()]);
        Ok(PlanConsent {
            client_id,
            redirect_uri,
            request,
            connect: self,
        })
    }

    fn end(&self, outcome: Result<PlanAccount, PlanError>) -> PlanConnectResult {
        if let Err(error) = &outcome {
            tracing::info!(provider = self.provider.code(), error = %error, "plan sign-in did not finish");
        }
        PlanConnectResult {
            provider: self.provider,
            generation: self.generation,
            outcome,
        }
    }
}

/// A plan sign-in waiting for the user's consent in the browser.
pub struct PlanConsent {
    client_id: String,
    redirect_uri: String,
    request: ConsentRequest,
    connect: PlanConnect,
}

impl std::fmt::Debug for PlanConsent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanConsent")
            .field("provider", &self.connect.provider)
            .finish_non_exhaustive()
    }
}

impl PlanConsent {
    /// The provider's consent page, for the system browser.
    pub fn consent_url(&self) -> &str {
        &self.request.url
    }

    pub fn provider(&self) -> Provider {
        self.connect.provider
    }

    /// Waits for the consent, trades the code for tokens, reads the plan
    /// and keeps the sign-in. Blocks for up to `CONSENT_TIMEOUT`, or until
    /// cancelled.
    pub fn run(mut self) -> PlanConnectResult {
        let callback = std::mem::replace(&mut self.connect.callback, Box::new(Spent));
        let outcome = self.finish(callback);
        self.connect.end(outcome)
    }

    fn finish(&self, callback: Box<dyn ConsentCallback>) -> Result<PlanAccount, PlanError> {
        let connect = &self.connect;
        let (provider, sessions) = (connect.provider, &connect.sessions);
        let code = callback.wait(
            &self.request.state,
            connect.timeout,
            &connect.cancel,
            &connect.pages,
        )?;
        sessions.redactor.add_parts(&[code.expose()]);
        let grant = connect
            .sign_in
            .exchange(
                &self.client_id,
                &code,
                &self.request.verifier,
                &self.redirect_uri,
            )
            .map_err(|failure| sessions.sign_in_failure(provider, failure))?;
        let tokens = TokenSet::granted(&grant, SystemTime::now());
        sessions.redactor.add_parts(&tokens.sensitive_parts());
        // Kept only once the tokens are shown to reach the plan.
        let account = connect
            .sign_in
            .account(tokens.access_token())
            .map_err(|failure| sessions.sign_in_failure(provider, failure))?;
        if connect.cancel.load(Ordering::Relaxed) {
            return Err(ConsentError::Cancelled.into());
        }
        let session = PlanSession::new(&self.client_id, tokens).ok_or_else(|| {
            sessions.sign_in_failure(
                provider,
                SignInFailure::new(
                    SignInFailureKind::Unexpected,
                    "the registration gave a client id that is not one",
                ),
            )
        })?;
        sessions.keep(provider, &session)?;
        Ok(account)
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

/// How a plan sign-in ended, for `Bardo::record_plan_connect`.
#[derive(Debug)]
pub struct PlanConnectResult {
    pub provider: Provider,
    generation: u64,
    pub outcome: Result<PlanAccount, PlanError>,
}

/// A read of the plan and its credits, renewing the sign-in if due. `run`
/// blocks on the provider; hand its result to `Bardo::record_plan_check`.
pub struct PlanCheck {
    provider: Provider,
    sign_in: Arc<dyn PlanSignIn>,
    sessions: PlanSessions,
}

impl std::fmt::Debug for PlanCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanCheck")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

/// How a plan check ended, for `Bardo::record_plan_check`.
#[derive(Debug)]
pub struct PlanCheckResult {
    pub provider: Provider,
    pub outcome: Result<PlanAccount, PlanError>,
}

impl PlanCheck {
    pub fn provider(&self) -> Provider {
        self.provider
    }

    pub fn run(self) -> PlanCheckResult {
        let provider = self.provider;
        let outcome = self.sessions.access_token(provider).and_then(|token| {
            self.sign_in
                .account(token.expose())
                .map_err(|failure| self.sessions.sign_in_failure(provider, failure))
        });
        PlanCheckResult { provider, outcome }
    }
}

impl Bardo {
    /// Every plan Bardo signs in to, in settings order.
    pub fn plans(&self) -> Vec<PlanStatus> {
        self.plan_book
            .sessions
            .sign_ins
            .iter()
            .map(|sign_in| self.plan(sign_in.provider()))
            .collect()
    }

    pub fn plan(&self, provider: Provider) -> PlanStatus {
        let book = &self.plan_book;
        PlanStatus {
            provider,
            state: book.state(provider),
            account: book.accounts.get(&provider).cloned(),
            connecting: book.attempts.contains_key(&provider),
        }
    }

    /// Prepares a sign-in (or a new one after a refusal) to the plan:
    /// starts the callback listener. An earlier attempt is cancelled.
    pub fn connect_plan(&mut self, provider: Provider) -> Result<PlanConnect, PlanError> {
        let sessions = self.plan_book.sessions.clone();
        let sign_in = sessions.sign_in(provider)?;
        let callback = self
            .plan_book
            .consent
            .listen(sign_in.redirect_path(), None)?;
        let book = &mut self.plan_book;
        if let Some(earlier) = book.attempts.remove(&provider) {
            earlier.cancel.store(true, Ordering::Relaxed);
        }
        book.generations += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        book.attempts.insert(
            provider,
            Attempt {
                generation: book.generations,
                cancel: Arc::clone(&cancel),
            },
        );
        Ok(PlanConnect {
            provider,
            generation: book.generations,
            callback,
            sign_in,
            sessions,
            cancel,
            pages: ConsentPages {
                done: self.text(Text::ConsentPageDone).into_owned(),
                failed: self.text(Text::ConsentPageFailed).into_owned(),
            },
            timeout: CONSENT_TIMEOUT,
        })
    }

    /// Stops waiting for the plan's consent. The card goes back to its
    /// earlier state at once.
    pub fn cancel_plan_connect(&mut self, provider: Provider) {
        if let Some(attempt) = self.plan_book.attempts.remove(&provider) {
            attempt.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Ends the plan's "signing in" state with a finished attempt. Returns
    /// `None` for an attempt cancelled or replaced while it ran.
    pub fn record_plan_connect(
        &mut self,
        result: PlanConnectResult,
    ) -> Option<Result<PlanAccount, PlanError>> {
        let book = &mut self.plan_book;
        let current = book
            .attempts
            .get(&result.provider)
            .is_some_and(|attempt| attempt.generation == result.generation);
        if !current {
            return None;
        }
        book.attempts.remove(&result.provider);
        if let Ok(account) = &result.outcome {
            book.states.insert(result.provider, PlanState::Connected);
            book.accounts.insert(result.provider, account.clone());
        }
        Some(result.outcome)
    }

    /// Prepares a read of the plan and its credits.
    pub fn plan_check(&self, provider: Provider) -> Result<PlanCheck, PlanError> {
        let sessions = self.plan_book.sessions.clone();
        Ok(PlanCheck {
            provider,
            sign_in: sessions.sign_in(provider)?,
            sessions,
        })
    }

    /// Keeps what a check read: the plan and its credits, or that the
    /// plan needs signing in again.
    pub fn record_plan_check(&mut self, result: PlanCheckResult) -> Result<PlanAccount, PlanError> {
        let book = &mut self.plan_book;
        match &result.outcome {
            Ok(account) => {
                book.accounts.insert(result.provider, account.clone());
            }
            Err(PlanError::NotConnected(_)) => {
                book.states.insert(result.provider, PlanState::NotConnected);
                book.accounts.remove(&result.provider);
            }
            Err(_) => {}
        }
        result.outcome
    }

    /// Signs out: forgets the plan's client id and tokens. Credits already
    /// spent stay spent; clips under way stop at their next step.
    pub fn disconnect_plan(&mut self, provider: Provider) -> Result<(), PlanError> {
        self.cancel_plan_connect(provider);
        let book = &mut self.plan_book;
        book.sessions.forget(provider)?;
        book.states.insert(provider, PlanState::NotConnected);
        book.accounts.remove(&provider);
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::collections::VecDeque;

    use bardo_domain::{Credits, SecretText, TokenGrant};

    use super::*;

    /// One call to the fake plan.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum PlanCall {
        Register(String),
        Exchange {
            client_id: String,
            code: String,
            verifier: String,
            redirect: String,
        },
        Refresh {
            client_id: String,
            refresh_token: String,
        },
        Account(String),
    }

    /// A plan that signs in without a network: each refresh hands out the
    /// next numbered access token.
    pub(crate) struct FakePlan {
        pub(crate) calls: Mutex<Vec<PlanCall>>,
        pub(crate) register: Mutex<Result<String, SignInFailure>>,
        pub(crate) exchange: Mutex<Option<SignInFailure>>,
        pub(crate) refreshes: Mutex<VecDeque<Result<(), SignInFailure>>>,
        pub(crate) account: Mutex<Result<PlanAccount, SignInFailure>>,
        pub(crate) expires_in: Mutex<Duration>,
        issued: Mutex<u32>,
    }

    impl Default for FakePlan {
        fn default() -> Self {
            Self {
                calls: Mutex::default(),
                register: Mutex::new(Ok("client-0001".into())),
                exchange: Mutex::default(),
                refreshes: Mutex::default(),
                account: Mutex::new(Ok(PlanAccount {
                    plan: "plus".into(),
                    credits: Credits::from_hundredths(98_250),
                })),
                expires_in: Mutex::new(Duration::from_secs(3_600)),
                issued: Mutex::new(0),
            }
        }
    }

    impl FakePlan {
        pub(crate) fn calls(&self) -> Vec<PlanCall> {
            self.calls.lock().unwrap().clone()
        }

        fn grant(&self) -> TokenGrant {
            let mut issued = self.issued.lock().unwrap();
            *issued += 1;
            TokenGrant {
                access_token: SecretText::new(format!("plan-access-{issued:04}")),
                refresh_token: Some(SecretText::new(format!("plan-refresh-{issued:04}"))),
                expires_in: *self.expires_in.lock().unwrap(),
                scopes: vec!["openid".into(), "offline_access".into()],
            }
        }
    }

    impl PlanSignIn for FakePlan {
        fn provider(&self) -> Provider {
            Provider::HiggsfieldPlan
        }

        fn register(&self, redirect_uri: &str) -> Result<String, SignInFailure> {
            self.calls
                .lock()
                .unwrap()
                .push(PlanCall::Register(redirect_uri.into()));
            self.register.lock().unwrap().clone()
        }

        fn consent_request(&self, client_id: &str, redirect_uri: &str) -> ConsentRequest {
            ConsentRequest {
                url: format!(
                    "https://plan.example/authorize?client_id={client_id}&redirect_uri={redirect_uri}"
                ),
                state: SecretText::new("plan-state-0001"),
                verifier: SecretText::new("plan-verifier-0001"),
            }
        }

        fn exchange(
            &self,
            client_id: &str,
            code: &SecretText,
            verifier: &SecretText,
            redirect_uri: &str,
        ) -> Result<TokenGrant, SignInFailure> {
            self.calls.lock().unwrap().push(PlanCall::Exchange {
                client_id: client_id.into(),
                code: code.expose().into(),
                verifier: verifier.expose().into(),
                redirect: redirect_uri.into(),
            });
            match self.exchange.lock().unwrap().take() {
                Some(failure) => Err(failure),
                None => Ok(self.grant()),
            }
        }

        fn refresh(
            &self,
            client_id: &str,
            refresh_token: &str,
        ) -> Result<TokenGrant, SignInFailure> {
            self.calls.lock().unwrap().push(PlanCall::Refresh {
                client_id: client_id.into(),
                refresh_token: refresh_token.into(),
            });
            let outcome = self.refreshes.lock().unwrap().pop_front().unwrap_or(Ok(()));
            outcome.map(|()| self.grant())
        }

        fn account(&self, access_token: &str) -> Result<PlanAccount, SignInFailure> {
            self.calls
                .lock()
                .unwrap()
                .push(PlanCall::Account(access_token.into()));
            self.account.lock().unwrap().clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{Credits, SecretText, TokenGrant};
    use bardo_storage::MemorySecretStore;

    use super::testing::{FakePlan, PlanCall};
    use super::*;
    use crate::connections::testing::{FakeConsent, REDIRECT};
    use crate::testing;
    use crate::{Providers, Repositories};

    const PLAN: Provider = Provider::HiggsfieldPlan;

    struct Harness {
        db: Arc<bardo_storage::Database>,
        secrets: Arc<MemorySecretStore>,
        plan: Arc<FakePlan>,
        consent: Arc<FakeConsent>,
    }

    impl Default for Harness {
        fn default() -> Self {
            Self {
                db: Arc::new(bardo_storage::Database::open_in_memory().unwrap()),
                secrets: Arc::default(),
                plan: Arc::default(),
                consent: Arc::default(),
            }
        }
    }

    impl Harness {
        /// Starts (or restarts) Bardo over the same database and stores.
        fn start(&self) -> Bardo {
            let repositories = Repositories {
                connection_secrets: Arc::clone(&self.secrets) as _,
                ..Repositories::shared(Arc::clone(&self.db), Arc::new(MemorySecretStore::default()))
            };
            let providers = Providers {
                plans: vec![Arc::clone(&self.plan) as _],
                consent: Arc::clone(&self.consent) as _,
                ..testing::providers()
            };
            Bardo::start(repositories, providers, Some("en-US")).unwrap()
        }

        fn session(&self, app: &Bardo) -> Option<PlanSession> {
            self.secrets.plan_session(app.profile().id, PLAN).unwrap()
        }

        /// Signs in through the fake browser.
        fn signed_in(&self) -> Bardo {
            let mut app = self.start();
            let outcome = sign_in(&mut app);
            assert!(matches!(outcome, Some(Ok(_))), "{outcome:?}");
            app
        }

        /// Bardo started over a stored sign-in whose access token expires
        /// in `secs`.
        fn stored(&self, secs: u64) -> Bardo {
            let app = self.start();
            self.store(&app, secs);
            self.start()
        }

        /// Stores a session whose access token expires in `secs`.
        fn store(&self, app: &Bardo, secs: u64) {
            let tokens = TokenSet::granted(
                &TokenGrant {
                    access_token: SecretText::new("stored-access"),
                    refresh_token: Some(SecretText::new("stored-refresh")),
                    expires_in: Duration::from_secs(secs),
                    scopes: Vec::new(),
                },
                SystemTime::now(),
            );
            let session = PlanSession::new("client-stored", tokens).unwrap();
            self.secrets
                .set_plan_session(app.profile().id, PLAN, &session)
                .unwrap();
        }
    }

    fn sign_in(app: &mut Bardo) -> Option<Result<PlanAccount, PlanError>> {
        let connect = app.connect_plan(PLAN).unwrap();
        let result = match connect.register() {
            Ok(consent) => consent.run(),
            Err(ended) => ended,
        };
        app.record_plan_connect(result)
    }

    fn token(app: &Bardo) -> Result<String, PlanError> {
        app.plan_book
            .sessions()
            .access_token(PLAN)
            .map(|key| key.expose().to_owned())
    }

    #[test]
    fn signing_in_registers_consents_and_keeps_the_client_with_its_tokens() {
        let harness = Harness::default();
        let mut app = harness.start();
        assert_eq!(app.plan(PLAN).state, PlanState::NotConnected);
        assert_eq!(app.provider_key(PLAN).state, KeyState::NotSet);
        harness.consent.will(Ok("plan-code-0001"));

        let connect = app.connect_plan(PLAN).unwrap();
        assert!(app.plan(PLAN).connecting);
        let consent = connect.register().unwrap();
        assert!(consent.consent_url().contains("client_id=client-0001"));
        let account = app.record_plan_connect(consent.run()).unwrap().unwrap();

        assert_eq!(account.plan, "plus");
        assert_eq!(harness.consent.paths.lock().unwrap()[0], "/callback");
        assert_eq!(
            harness.plan.calls(),
            [
                PlanCall::Register(REDIRECT.into()),
                PlanCall::Exchange {
                    client_id: "client-0001".into(),
                    code: "plan-code-0001".into(),
                    verifier: "plan-verifier-0001".into(),
                    redirect: REDIRECT.into(),
                },
                PlanCall::Account("plan-access-0001".into()),
            ]
        );
        assert_eq!(harness.consent.states.lock().unwrap()[0], "plan-state-0001");
        let session = harness.session(&app).unwrap();
        assert_eq!(session.client_id(), "client-0001");
        assert_eq!(session.tokens().access_token(), "plan-access-0001");
        let status = app.plan(PLAN);
        assert_eq!(status.state, PlanState::Connected);
        assert!(!status.connecting);
        assert_eq!(
            status.account.unwrap().credits,
            Credits::from_hundredths(98_250)
        );
        assert_eq!(
            app.provider_key(PLAN).state,
            KeyState::Saved {
                hint: "plus".into()
            }
        );
    }

    #[test]
    fn a_sign_in_survives_a_restart_and_its_tokens_are_masked_from_the_start() {
        let harness = Harness::default();
        drop(harness.signed_in());
        let app = harness.start();
        assert_eq!(app.plan(PLAN).state, PlanState::Connected);
        assert_eq!(app.plan(PLAN).account, None);
        let redacted = app
            .plan_book
            .sessions()
            .redactor
            .redact("Bearer plan-access-0001 and plan-refresh-0001");
        assert!(!redacted.contains("plan-access-0001"), "{redacted}");
        assert!(!redacted.contains("plan-refresh-0001"), "{redacted}");
    }

    #[test]
    fn an_access_token_close_to_expiring_is_renewed_with_the_registered_client() {
        let harness = Harness::default();
        let app = harness.stored(60);
        assert_eq!(token(&app).unwrap(), "plan-access-0001");
        assert_eq!(
            harness.plan.calls(),
            [PlanCall::Refresh {
                client_id: "client-stored".into(),
                refresh_token: "stored-refresh".into(),
            }]
        );
        let session = harness.session(&app).unwrap();
        assert_eq!(session.client_id(), "client-stored");
        assert_eq!(session.tokens().refresh_token(), Some("plan-refresh-0001"));
        // Fresh now: no second refresh.
        assert_eq!(token(&app).unwrap(), "plan-access-0001");
        assert_eq!(harness.plan.calls().len(), 1);
    }

    #[test]
    fn a_fresh_token_is_used_as_it_is() {
        let harness = Harness::default();
        let app = harness.stored(3_600);
        assert_eq!(token(&app).unwrap(), "stored-access");
        assert!(harness.plan.calls().is_empty());
    }

    #[test]
    fn a_refused_refresh_asks_to_sign_in_again_until_the_user_does() {
        let harness = Harness::default();
        let mut app = harness.stored(60);
        harness
            .plan
            .refreshes
            .lock()
            .unwrap()
            .push_back(Err(SignInFailure::new(
                SignInFailureKind::Refused,
                "invalid_grant",
            )));
        assert!(matches!(token(&app), Err(PlanError::ReconnectNeeded(PLAN))));
        assert_eq!(app.plan(PLAN).state, PlanState::ReconnectNeeded);
        assert_eq!(app.provider_key(PLAN).state, KeyState::NotSet);
        // No more calls until the user signs in again.
        assert!(matches!(token(&app), Err(PlanError::ReconnectNeeded(PLAN))));
        assert_eq!(harness.plan.calls().len(), 1);

        assert!(matches!(sign_in(&mut app), Some(Ok(_))));
        assert_eq!(app.plan(PLAN).state, PlanState::Connected);
        assert!(token(&app).is_ok());
    }

    #[test]
    fn a_passing_refresh_failure_keeps_a_token_that_still_works() {
        let harness = Harness::default();
        let app = harness.stored(600);
        let offline = || SignInFailure::new(SignInFailureKind::Unreachable, "dns error");
        harness
            .plan
            .refreshes
            .lock()
            .unwrap()
            .push_back(Err(offline()));
        assert_eq!(token(&app).unwrap(), "stored-access");
        assert_eq!(app.plan(PLAN).state, PlanState::Connected);

        harness.store(&app, 0);
        harness
            .plan
            .refreshes
            .lock()
            .unwrap()
            .push_back(Err(offline()));
        let error = token(&app).unwrap_err();
        assert!(error.is_transient(), "{error:?}");
        assert_eq!(app.plan(PLAN).state, PlanState::Connected);
    }

    #[test]
    fn signing_out_forgets_the_client_and_tokens() {
        let harness = Harness::default();
        let mut app = harness.signed_in();
        app.disconnect_plan(PLAN).unwrap();
        assert_eq!(harness.session(&app), None);
        assert_eq!(app.plan(PLAN).state, PlanState::NotConnected);
        assert_eq!(app.plan(PLAN).account, None);
        assert!(matches!(token(&app), Err(PlanError::NotConnected(PLAN))));
    }

    #[test]
    fn a_sign_in_that_does_not_reach_the_plan_keeps_nothing() {
        let harness = Harness::default();
        let mut app = harness.start();
        *harness.plan.account.lock().unwrap() = Err(SignInFailure::new(
            SignInFailureKind::NotAllowed,
            "Bearer plan-access-0001 lacks access",
        ));
        let outcome = sign_in(&mut app).unwrap();
        let Err(PlanError::SignIn(PLAN, failure)) = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(failure.kind, SignInFailureKind::NotAllowed);
        assert!(!failure.detail.contains("plan-access-0001"), "{failure:?}");
        assert_eq!(harness.session(&app), None);
        assert_eq!(app.plan(PLAN).state, PlanState::NotConnected);
    }

    #[test]
    fn a_failed_registration_or_denied_consent_ends_the_attempt() {
        let harness = Harness::default();
        let mut app = harness.start();
        *harness.plan.register.lock().unwrap() = Err(SignInFailure::new(
            SignInFailureKind::Unreachable,
            "connection refused",
        ));
        let outcome = sign_in(&mut app).unwrap();
        assert!(
            matches!(&outcome, Err(error) if error.message() == Text::PlanFailure(SignInFailureKind::Unreachable))
        );
        assert!(!app.plan(PLAN).connecting);

        *harness.plan.register.lock().unwrap() = Ok("client-0002".into());
        harness
            .consent
            .will(Err(ConsentError::Denied("access_denied".into())));
        let outcome = sign_in(&mut app).unwrap();
        assert!(matches!(
            outcome,
            Err(PlanError::Consent(ConsentError::Denied(_)))
        ));
        assert_eq!(harness.session(&app), None);
    }

    #[test]
    fn a_cancelled_or_replaced_sign_in_reports_nothing() {
        let harness = Harness::default();
        let mut app = harness.start();
        let first = app.connect_plan(PLAN).unwrap();
        app.cancel_plan_connect(PLAN);
        assert!(!app.plan(PLAN).connecting);
        let ended = match first.register() {
            Ok(consent) => consent.run(),
            Err(ended) => ended,
        };
        assert!(app.record_plan_connect(ended).is_none());
        // The cancelled attempt never asked for tokens.
        assert!(
            !harness
                .plan
                .calls()
                .iter()
                .any(|call| matches!(call, PlanCall::Exchange { .. }))
        );
        assert_eq!(harness.session(&app), None);
    }

    #[test]
    fn a_check_reads_the_credits_and_notices_a_sign_out() {
        let harness = Harness::default();
        let mut app = harness.stored(3_600);
        let check = app.plan_check(PLAN).unwrap();
        let account = app.record_plan_check(check.run()).unwrap();
        assert_eq!(account.credits.to_string(), "982.5");
        assert_eq!(
            harness.plan.calls(),
            [PlanCall::Account("stored-access".into())]
        );
        assert_eq!(app.plan(PLAN).account, Some(account));

        harness
            .secrets
            .delete_plan_session(app.profile().id, PLAN)
            .unwrap();
        let check = app.plan_check(PLAN).unwrap();
        assert!(matches!(
            app.record_plan_check(check.run()),
            Err(PlanError::NotConnected(PLAN))
        ));
        assert_eq!(app.plan(PLAN).state, PlanState::NotConnected);
    }

    #[test]
    fn a_plan_bardo_does_not_sign_in_to_is_refused() {
        let harness = Harness::default();
        let mut app = harness.start();
        assert!(matches!(
            app.connect_plan(Provider::Kling),
            Err(PlanError::NotOffered(Provider::Kling))
        ));
        assert_eq!(app.plans().len(), 1);
    }
}
