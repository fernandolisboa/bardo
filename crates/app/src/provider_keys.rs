//! Provider key use cases: save, replace, remove and test the profile's API
//! keys. Keys go only to the secret store; what the app keeps is whether a
//! key is saved, its last four characters and the last test result.

use std::collections::HashMap;
use std::sync::Arc;

use bardo_domain::{
    ApiKey, ApiKeyError, KeyCheck, KeyChecker, ProfileId, Provider, Redactor, SecretStore,
    SecretStoreError,
};

use crate::{Bardo, Text};

#[derive(Debug, thiserror::Error)]
pub enum ProviderKeyError {
    #[error("invalid key: {0}")]
    Invalid(#[from] ApiKeyError),
    #[error("no key saved for this provider")]
    NotSet,
    #[error(transparent)]
    Store(#[from] SecretStoreError),
}

impl ProviderKeyError {
    /// What the settings screen says.
    pub fn message(&self) -> Text {
        match self {
            ProviderKeyError::Invalid(error) => Text::ApiKeyError(*error),
            ProviderKeyError::NotSet => Text::ProviderKeyNotSet,
            ProviderKeyError::Store(_) => Text::ProviderKeyStoreFailed,
        }
    }
}

/// Whether a provider has a key, as far as the app knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyState {
    NotSet,
    /// Saved; the last four characters, to tell keys apart.
    Saved {
        hint: String,
    },
    /// The secret store could not be read at startup.
    Unreadable,
}

/// One row of the settings screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderKeyStatus {
    pub provider: Provider,
    pub state: KeyState,
    /// The last test of the current key in this session, already redacted.
    pub last_check: Option<KeyCheck>,
}

/// What the app tracks per provider.
#[derive(Debug, Clone)]
struct Slot {
    state: KeyState,
    last_check: Option<KeyCheck>,
    /// Bumped on every save or removal, so a test that finishes after the
    /// key changed does not report on the new key.
    generation: u64,
}

pub(crate) struct ProviderKeys {
    store: Arc<dyn SecretStore>,
    checker: Arc<dyn KeyChecker>,
    redactor: Redactor,
    slots: HashMap<Provider, Slot>,
}

impl ProviderKeys {
    /// Reads which keys are saved and teaches the redactor every one of
    /// them, so they are masked from the first log line on. A store that
    /// cannot be read does not stop the app; the provider shows as
    /// unreadable.
    pub(crate) fn load(
        store: Arc<dyn SecretStore>,
        checker: Arc<dyn KeyChecker>,
        redactor: Redactor,
        owner: ProfileId,
    ) -> Self {
        let slots = Provider::ALL
            .into_iter()
            .map(|provider| {
                let state = match store.get(owner, provider) {
                    Ok(Some(key)) => {
                        redactor.add(&key);
                        KeyState::Saved { hint: key.hint() }
                    }
                    Ok(None) => KeyState::NotSet,
                    Err(error) => {
                        tracing::warn!(
                            provider = provider.code(),
                            error = %redactor.redact(&error.to_string()),
                            "could not read the provider key"
                        );
                        KeyState::Unreadable
                    }
                };
                let slot = Slot {
                    state,
                    last_check: None,
                    generation: 0,
                };
                (provider, slot)
            })
            .collect();
        Self {
            store,
            checker,
            redactor,
            slots,
        }
    }

    fn slot_mut(&mut self, provider: Provider) -> &mut Slot {
        self.slots
            .get_mut(&provider)
            .expect("every provider has a slot")
    }

    fn replaced(&mut self, provider: Provider, state: KeyState) {
        let slot = self.slot_mut(provider);
        slot.state = state;
        slot.last_check = None;
        slot.generation += 1;
    }

    pub(crate) fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    /// The saved key, read fresh from the store and masked from then on.
    pub(crate) fn read(
        &self,
        owner: ProfileId,
        provider: Provider,
    ) -> Result<ApiKey, ProviderKeyError> {
        let key = self
            .store
            .get(owner, provider)
            .map_err(|error| store_failure(&self.redactor, provider, "read", error))?
            .ok_or(ProviderKeyError::NotSet)?;
        self.redactor.add(&key);
        Ok(key)
    }
}

/// A key test ready to run. `run` calls the provider and blocks, so the UI
/// runs it on a background thread and hands the result to
/// `Bardo::record_key_test`.
pub struct KeyTest {
    provider: Provider,
    key: ApiKey,
    generation: u64,
    checker: Arc<dyn KeyChecker>,
    redactor: Redactor,
}

impl KeyTest {
    pub fn provider(&self) -> Provider {
        self.provider
    }

    pub fn run(self) -> KeyTestResult {
        let mut check = self.checker.check(self.provider, &self.key);
        check.detail = check.detail.map(|detail| self.redactor.redact(&detail));
        tracing::info!(
            provider = self.provider.code(),
            outcome = check.outcome.code(),
            detail = check.detail.as_deref().unwrap_or(""),
            "tested provider key"
        );
        KeyTestResult {
            provider: self.provider,
            generation: self.generation,
            check,
        }
    }
}

impl std::fmt::Debug for KeyTest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyTest")
            .field("provider", &self.provider)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// How a key test ended, for `Bardo::record_key_test`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyTestResult {
    pub provider: Provider,
    generation: u64,
    pub check: KeyCheck,
}

impl Bardo {
    /// Every provider, in settings order, with its key state.
    pub fn provider_keys(&self) -> Vec<ProviderKeyStatus> {
        Provider::ALL
            .into_iter()
            .map(|provider| self.provider_key(provider))
            .collect()
    }

    pub fn provider_key(&self, provider: Provider) -> ProviderKeyStatus {
        let slot = &self.provider_keys.slots[&provider];
        ProviderKeyStatus {
            provider,
            state: slot.state.clone(),
            last_check: slot.last_check.clone(),
        }
    }

    /// Validates the typed key and saves it in the secret store, replacing
    /// any earlier key. The earlier key's test result no longer applies.
    pub fn save_provider_key(
        &mut self,
        provider: Provider,
        input: &str,
    ) -> Result<(), ProviderKeyError> {
        let key = ApiKey::parse(provider, input)?;
        let keys = &mut self.provider_keys;
        // Masked before the store sees it, in case its error quotes it.
        keys.redactor.add(&key);
        keys.store
            .set(self.profile.id, provider, &key)
            .map_err(|error| store_failure(&keys.redactor, provider, "save", error))?;
        tracing::info!(provider = provider.code(), "saved provider key");
        keys.replaced(provider, KeyState::Saved { hint: key.hint() });
        self.forget_voice_list(provider);
        Ok(())
    }

    /// Removes the key from the secret store.
    pub fn remove_provider_key(&mut self, provider: Provider) -> Result<(), ProviderKeyError> {
        let keys = &mut self.provider_keys;
        keys.store
            .delete(self.profile.id, provider)
            .map_err(|error| store_failure(&keys.redactor, provider, "remove", error))?;
        tracing::info!(provider = provider.code(), "removed provider key");
        keys.replaced(provider, KeyState::NotSet);
        self.forget_voice_list(provider);
        Ok(())
    }

    /// Prepares a test of the saved key. Reads the key fresh from the
    /// secret store, so it tests exactly what features will use.
    pub fn key_test(&self, provider: Provider) -> Result<KeyTest, ProviderKeyError> {
        let keys = &self.provider_keys;
        let key = keys.read(self.profile.id, provider)?;
        Ok(KeyTest {
            provider,
            key,
            generation: keys.slots[&provider].generation,
            checker: Arc::clone(&keys.checker),
            redactor: keys.redactor.clone(),
        })
    }

    /// Keeps a finished test's result for the settings screen. Returns false
    /// and drops it when the key was replaced or removed while it ran.
    pub fn record_key_test(&mut self, result: KeyTestResult) -> bool {
        let slot = self.provider_keys.slot_mut(result.provider);
        if slot.generation != result.generation {
            return false;
        }
        slot.last_check = Some(result.check);
        true
    }
}

/// Logs a secret store failure and returns it with any known key masked,
/// so no caller can print the original.
fn store_failure(
    redactor: &Redactor,
    provider: Provider,
    action: &str,
    error: SecretStoreError,
) -> ProviderKeyError {
    let cause = redactor.redact(&error.0.to_string());
    tracing::warn!(
        provider = provider.code(),
        error = %cause,
        "could not {action} the provider key"
    );
    ProviderKeyError::Store(SecretStoreError(cause.into()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bardo_domain::KeyCheckOutcome;
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::testing::FakeKeyChecker;
    use crate::{Providers, Repositories, testing};

    const CLAUDE_KEY: &str = "sk-ant-api03-test-0001-wxyz";

    struct Harness {
        db: Arc<Database>,
        secrets: Arc<MemorySecretStore>,
        checker: Arc<FakeKeyChecker>,
    }

    impl Harness {
        fn new() -> Self {
            Self::with_db(Database::open_in_memory().unwrap())
        }

        fn with_db(db: Database) -> Self {
            Self {
                db: Arc::new(db),
                secrets: Arc::default(),
                checker: Arc::default(),
            }
        }

        fn start_with_store(&self, secrets: Arc<dyn SecretStore>) -> Bardo {
            let repositories = Repositories {
                profiles: Box::new(Arc::clone(&self.db)),
                channels: Box::new(Arc::clone(&self.db)),
                jobs: Arc::clone(&self.db) as _,
                themes: Arc::clone(&self.db) as _,
                templates: Arc::clone(&self.db) as _,
                scripts: Arc::clone(&self.db) as _,
                personas: Arc::clone(&self.db) as _,
                narrations: Arc::clone(&self.db) as _,
                scene_plans: Arc::clone(&self.db) as _,
                timelines: Arc::clone(&self.db) as _,
                media_assets: Arc::clone(&self.db) as _,
                music_prompts: Arc::clone(&self.db) as _,
                network_accounts: Arc::clone(&self.db) as _,
                renders: Arc::clone(&self.db) as _,
                exports: Arc::clone(&self.db) as _,
                publications: Arc::clone(&self.db) as _,
                cut_suggestions: Arc::clone(&self.db) as _,
                export_files: Arc::new(bardo_storage::MemoryExportFiles::default()),
                costs: Arc::clone(&self.db) as _,
                files: Arc::new(bardo_storage::MemoryProjectFiles::default()),
                research: Arc::clone(&self.db) as _,
                secrets,
            };
            let providers = Providers {
                key_checker: Arc::clone(&self.checker) as _,
                ..testing::providers()
            };
            Bardo::start(repositories, providers, Some("en-US")).unwrap()
        }

        /// Starts (or restarts) the app over the same database and store.
        fn start(&self) -> Bardo {
            self.start_with_store(Arc::clone(&self.secrets) as _)
        }

        fn answer(&self, outcome: KeyCheckOutcome, detail: Option<&str>) {
            *self.checker.answer.lock().unwrap() =
                Some(KeyCheck::new(outcome, detail.map(str::to_owned)));
        }

        fn stored(&self, app: &Bardo, provider: Provider) -> Option<String> {
            self.secrets
                .get(app.profile().id, provider)
                .unwrap()
                .map(|key| key.expose().to_owned())
        }
    }

    /// A store whose every call fails, quoting the key it was given, as a
    /// careless adapter might.
    struct BrokenStore;

    impl SecretStore for BrokenStore {
        fn get(&self, _: ProfileId, _: Provider) -> Result<Option<ApiKey>, SecretStoreError> {
            Err(SecretStoreError("credential store locked".into()))
        }

        fn set(&self, _: ProfileId, _: Provider, key: &ApiKey) -> Result<(), SecretStoreError> {
            Err(SecretStoreError(
                format!("could not write {}", key.expose()).into(),
            ))
        }

        fn delete(&self, _: ProfileId, _: Provider) -> Result<(), SecretStoreError> {
            Err(SecretStoreError("credential store locked".into()))
        }
    }

    fn state(app: &Bardo, provider: Provider) -> KeyState {
        app.provider_key(provider).state
    }

    fn run_test(app: &mut Bardo, provider: Provider) -> bool {
        let result = app.key_test(provider).unwrap().run();
        app.record_key_test(result)
    }

    #[test]
    fn a_new_profile_has_no_keys() {
        let app = Harness::new().start();
        let keys = app.provider_keys();
        assert_eq!(
            keys.iter().map(|k| k.provider).collect::<Vec<_>>(),
            Provider::ALL
        );
        assert!(
            keys.iter()
                .all(|k| k.state == KeyState::NotSet && k.last_check.is_none())
        );
    }

    #[test]
    fn a_saved_key_goes_to_the_secret_store_and_shows_only_its_end() {
        let harness = Harness::new();
        let mut app = harness.start();
        app.save_provider_key(Provider::Claude, &format!(" {CLAUDE_KEY}\n"))
            .unwrap();

        assert_eq!(
            harness.stored(&app, Provider::Claude).as_deref(),
            Some(CLAUDE_KEY)
        );
        assert_eq!(
            state(&app, Provider::Claude),
            KeyState::Saved {
                hint: "…wxyz".into()
            }
        );
        assert_eq!(state(&app, Provider::Gemini), KeyState::NotSet);
    }

    #[test]
    fn an_invalid_key_is_refused_and_nothing_is_saved() {
        let harness = Harness::new();
        let mut app = harness.start();
        let error = app
            .save_provider_key(Provider::Higgsfield, "no-colon-here")
            .unwrap_err();

        assert_eq!(
            error.message(),
            Text::ApiKeyError(ApiKeyError::NotIdAndSecret)
        );
        assert_eq!(harness.stored(&app, Provider::Higgsfield), None);
        assert_eq!(state(&app, Provider::Higgsfield), KeyState::NotSet);
    }

    #[test]
    fn a_key_can_be_replaced_and_the_old_test_result_is_dropped() {
        let harness = Harness::new();
        let mut app = harness.start();
        app.save_provider_key(Provider::Gemini, "AIzaFirstKey-0001")
            .unwrap();
        assert!(run_test(&mut app, Provider::Gemini));
        assert!(app.provider_key(Provider::Gemini).last_check.is_some());

        app.save_provider_key(Provider::Gemini, "AIzaSecondKey-0002")
            .unwrap();

        assert_eq!(
            harness.stored(&app, Provider::Gemini).as_deref(),
            Some("AIzaSecondKey-0002")
        );
        let status = app.provider_key(Provider::Gemini);
        assert_eq!(
            status.state,
            KeyState::Saved {
                hint: "…0002".into()
            }
        );
        assert_eq!(status.last_check, None);
    }

    #[test]
    fn a_key_can_be_removed() {
        let harness = Harness::new();
        let mut app = harness.start();
        app.save_provider_key(Provider::TypeSafe, "ts-key-0001-abcd")
            .unwrap();
        app.remove_provider_key(Provider::TypeSafe).unwrap();

        assert_eq!(harness.stored(&app, Provider::TypeSafe), None);
        assert_eq!(state(&app, Provider::TypeSafe), KeyState::NotSet);
        assert!(matches!(
            app.key_test(Provider::TypeSafe),
            Err(ProviderKeyError::NotSet)
        ));
        // Removing again is harmless.
        app.remove_provider_key(Provider::TypeSafe).unwrap();
    }

    #[test]
    fn testing_a_key_checks_the_saved_key_and_keeps_the_result() {
        let harness = Harness::new();
        let mut app = harness.start();
        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        harness.answer(KeyCheckOutcome::Rejected, Some("API key is invalid."));

        assert!(run_test(&mut app, Provider::Claude));

        assert_eq!(
            *harness.checker.asked.lock().unwrap(),
            [(Provider::Claude, CLAUDE_KEY.to_owned())]
        );
        assert_eq!(
            app.provider_key(Provider::Claude).last_check,
            Some(KeyCheck::new(
                KeyCheckOutcome::Rejected,
                Some("API key is invalid.".into())
            ))
        );
    }

    #[test]
    fn a_key_cannot_be_tested_before_it_is_saved() {
        let error = Harness::new()
            .start()
            .key_test(Provider::ElevenLabs)
            .unwrap_err();
        assert_eq!(error.message(), Text::ProviderKeyNotSet);
    }

    #[test]
    fn a_test_that_ends_after_the_key_changed_is_ignored() {
        let harness = Harness::new();
        let mut app = harness.start();
        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        let test = app.key_test(Provider::Claude).unwrap();

        app.save_provider_key(Provider::Claude, "sk-ant-api03-newer-0002")
            .unwrap();

        assert!(!app.record_key_test(test.run()));
        assert_eq!(app.provider_key(Provider::Claude).last_check, None);
    }

    #[test]
    fn a_provider_message_that_quotes_the_key_is_redacted() {
        let harness = Harness::new();
        let mut app = harness.start();
        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        harness.answer(
            KeyCheckOutcome::Rejected,
            Some(&format!("Incorrect API key provided: {CLAUDE_KEY}")),
        );

        run_test(&mut app, Provider::Claude);

        let detail = app
            .provider_key(Provider::Claude)
            .last_check
            .unwrap()
            .detail
            .unwrap();
        assert_eq!(detail, "Incorrect API key provided: [redacted]");
    }

    #[test]
    fn saved_keys_are_known_after_a_restart_and_masked_from_the_start() {
        let harness = Harness::new();
        harness
            .start()
            .save_provider_key(Provider::YouTubeData, "AIzaYouTubeKey-0001")
            .unwrap();

        let restarted = harness.start();

        assert_eq!(
            state(&restarted, Provider::YouTubeData),
            KeyState::Saved {
                hint: "…0001".into()
            }
        );
        assert_eq!(
            restarted.redactor().redact("key=AIzaYouTubeKey-0001"),
            "key=[redacted]"
        );
    }

    #[test]
    fn an_unreadable_store_does_not_stop_the_app() {
        let app = Harness::new().start_with_store(Arc::new(BrokenStore));
        assert!(
            app.provider_keys()
                .iter()
                .all(|key| key.state == KeyState::Unreadable)
        );
    }

    #[test]
    fn a_store_failure_is_reported_without_the_key() {
        let mut app = Harness::new().start_with_store(Arc::new(BrokenStore));
        let error = app
            .save_provider_key(Provider::Claude, CLAUDE_KEY)
            .unwrap_err();

        assert_eq!(error.message(), Text::ProviderKeyStoreFailed);
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains(CLAUDE_KEY), "{shown}");
        assert!(shown.contains("could not write [redacted]"), "{shown}");
        assert_eq!(state(&app, Provider::Claude), KeyState::Unreadable);
    }

    /// Every byte of every file under `dir`.
    fn files_under(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                found.extend(files_under(&path));
            } else {
                found.push((path.display().to_string(), std::fs::read(&path).unwrap()));
            }
        }
        found
    }

    fn contains(haystack: &[u8], needle: &str) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    }

    /// The acceptance check of the slice: keys reach only the secret store,
    /// never the database or the log file, even when a provider or a job
    /// echoes them.
    #[test]
    fn keys_never_reach_the_database_or_the_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let harness = Harness::with_db(Database::open(&dir.path().join("bardo.db")).unwrap());
        let higgsfield = "hf-key-id-0001:hf-secret-0001";
        let mut app = harness.start();
        // The log shares the app's redactor, as `main` wires it.
        let log = crate::logging::file_subscriber(
            &dir.path().join("logs").join("bardo.log"),
            app.redactor(),
        )
        .unwrap();

        tracing::subscriber::with_default(log, || {
            app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
            app.save_provider_key(Provider::Higgsfield, higgsfield)
                .unwrap();
            harness.answer(
                KeyCheckOutcome::Rejected,
                Some(&format!("bad key {CLAUDE_KEY} / {higgsfield}")),
            );
            run_test(&mut app, Provider::Claude);
            run_test(&mut app, Provider::Higgsfield);
            tracing::error!("careless log line with {CLAUDE_KEY} and hf-secret-0001");
            app.set_ui_language(bardo_domain::UiLanguage::PtBr).unwrap();
        });
        assert_eq!(
            harness.stored(&app, Provider::Claude).as_deref(),
            Some(CLAUDE_KEY)
        );
        drop(app);

        let files = files_under(dir.path());
        assert!(files.iter().any(|(name, _)| name.ends_with("bardo.db")));
        let log_text = files
            .iter()
            .find(|(name, _)| name.ends_with("bardo.log"))
            .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
            .unwrap();
        assert!(log_text.contains("tested provider key"), "{log_text}");
        assert!(log_text.contains(Redactor::MASK), "{log_text}");
        for (name, bytes) in &files {
            for secret in [CLAUDE_KEY, higgsfield, "hf-secret-0001"] {
                assert!(!contains(bytes, secret), "{name} contains {secret}");
            }
        }
    }
}
