//! A project's own API keys and budgets (#343, `docs/spec/projects.md`
//! "Keys and budgets per project").
//!
//! A job runs for the project it was queued for (what Jobs shows as its
//! project). While it runs, every key it reads goes through `KeyRing`: the
//! project's own key for the provider when the project has one, else the
//! app's. The queue runs each job on a thread of its own and marks that
//! thread with the job's project (`work_for`), so no handler can read a key
//! for the wrong project, and the cost book notes, from the same mark,
//! which key paid (`payer_used`). Outside a job (a key test, a voice
//! sample heard on a screen) the app's key is used, as before.
//!
//! Spending with a project's key counts against the project's budget for
//! the provider, never the app's; spending with the app's key counts
//! against the app's, as before (`Budgets::of`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, Budget, KeyPayer, Money, ProfileId, ProjectBudget, ProjectId, ProjectKey,
    ProjectKeyRepository, Provider, RepositoryError, SecretStore, SecretStoreError,
};

use crate::costs::CostError;
use crate::provider_keys::{KeyState, ProviderKeyError, ProviderKeyStatus, store_failure};
use crate::{Bardo, KeyTest};

/// What the running job's thread is working for, and which key paid each
/// provider so far.
struct Scope {
    project: Option<ProjectId>,
    used: Vec<(Provider, KeyPayer)>,
}

thread_local! {
    static SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
}

/// Marks this thread as working for `project` until dropped. Not `Send`:
/// the mark belongs to the thread that made it.
pub(crate) struct WorkScope {
    previous: Option<Scope>,
    _thread: std::marker::PhantomData<*const ()>,
}

impl Drop for WorkScope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        SCOPE.with(|scope| *scope.borrow_mut() = previous);
    }
}

/// Runs this thread's keys for `project` (the job's) until the returned
/// guard is dropped; `None` for work of no project, which uses the app's
/// keys.
pub(crate) fn work_for(project: Option<ProjectId>) -> WorkScope {
    let previous = SCOPE.with(|scope| {
        scope.borrow_mut().replace(Scope {
            project,
            used: Vec::new(),
        })
    });
    WorkScope {
        previous,
        _thread: std::marker::PhantomData,
    }
}

/// The project this thread works for; `None` outside a job or for work of
/// no project.
pub(crate) fn working_for() -> Option<ProjectId> {
    SCOPE.with(|scope| scope.borrow().as_ref().and_then(|scope| scope.project))
}

/// Whose key this thread last read for `provider`, if it read one.
pub(crate) fn payer_used(provider: Provider) -> Option<KeyPayer> {
    SCOPE.with(|scope| {
        scope.borrow().as_ref().and_then(|scope| {
            scope
                .used
                .iter()
                .rev()
                .find(|(used, _)| *used == provider)
                .map(|(_, payer)| *payer)
        })
    })
}

fn note(provider: Provider, payer: KeyPayer) {
    SCOPE.with(|scope| {
        if let Some(scope) = scope.borrow_mut().as_mut() {
            scope.used.push((provider, payer));
        }
    });
}

fn repository_failure(error: RepositoryError) -> SecretStoreError {
    SecretStoreError(format!("could not tell whether the project has its own key: {error}").into())
}

/// The secret store job handlers read keys from: for a thread marked with
/// a project (`work_for`), that project's own key when it has one, else
/// the app's. Writes go to the app's keys, as the store's own do.
pub(crate) struct KeyRing {
    store: Arc<dyn SecretStore>,
    projects: Arc<dyn ProjectKeyRepository>,
}

impl KeyRing {
    pub(crate) fn new(
        store: Arc<dyn SecretStore>,
        projects: Arc<dyn ProjectKeyRepository>,
    ) -> Self {
        Self { store, projects }
    }
}

impl SecretStore for KeyRing {
    fn get(
        &self,
        owner: ProfileId,
        provider: Provider,
    ) -> Result<Option<ApiKey>, SecretStoreError> {
        let project = working_for();
        let has_own = match project {
            Some(project) => self
                .projects
                .has_project_key(project, provider)
                .map_err(repository_failure)?,
            None => false,
        };
        match KeyPayer::for_work(project, has_own) {
            KeyPayer::Project(project) => {
                // A project that has its own key never falls back to the
                // app's: its work would count against the wrong budget. A
                // key missing on this computer (a database restored here)
                // stops the job as a missing key.
                let key = self.store.project_key(owner, project, provider)?;
                if key.is_some() {
                    note(provider, KeyPayer::Project(project));
                }
                Ok(key)
            }
            KeyPayer::App => {
                let key = self.store.get(owner, provider)?;
                if key.is_some() {
                    note(provider, KeyPayer::App);
                }
                Ok(key)
            }
        }
    }

    fn set(
        &self,
        owner: ProfileId,
        provider: Provider,
        key: &ApiKey,
    ) -> Result<(), SecretStoreError> {
        self.store.set(owner, provider, key)
    }

    fn delete(&self, owner: ProfileId, provider: Provider) -> Result<(), SecretStoreError> {
        self.store.delete(owner, provider)
    }

    fn project_key(
        &self,
        owner: ProfileId,
        project: ProjectId,
        provider: Provider,
    ) -> Result<Option<ApiKey>, SecretStoreError> {
        self.store.project_key(owner, project, provider)
    }

    fn set_project_key(
        &self,
        owner: ProfileId,
        project: ProjectId,
        provider: Provider,
        key: &ApiKey,
    ) -> Result<(), SecretStoreError> {
        self.store.set_project_key(owner, project, provider, key)
    }

    fn delete_project_key(
        &self,
        owner: ProfileId,
        project: ProjectId,
        provider: Provider,
    ) -> Result<(), SecretStoreError> {
        self.store.delete_project_key(owner, project, provider)
    }
}

/// A provider in a project's settings: its key and its budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKeyStatus {
    pub key: ProviderKeyStatus,
    /// The project's monthly budget for what its own key pays; `None`
    /// without one (or without a key).
    pub budget: Option<Money>,
}

impl Bardo {
    /// The open project's keys, for every provider with a key, in settings
    /// order, with each one's budget.
    pub fn project_keys(&self, project: ProjectId) -> Vec<ProjectKeyStatus> {
        let budgets = self
            .cost_book
            .costs
            .project_budgets(self.profile.id)
            .unwrap_or_default();
        Provider::with_keys()
            .map(|provider| ProjectKeyStatus {
                key: self.provider_keys.project_status(project, provider),
                budget: budgets
                    .iter()
                    .find(|set| set.project == project && set.budget.provider == provider)
                    .map(|set| set.budget.monthly),
            })
            .collect()
    }

    /// Whether `project` keeps its own key for `provider`, as far as the
    /// app knows (saved, or noted but not readable on this computer).
    pub(crate) fn project_has_key(&self, project: ProjectId, provider: Provider) -> bool {
        self.provider_keys.project_status(project, provider).state != KeyState::NotSet
    }

    /// Which key work for `project` pays `provider` with.
    pub(crate) fn payer_for(&self, project: Option<ProjectId>, provider: Provider) -> KeyPayer {
        KeyPayer::for_work(
            project,
            project.is_some_and(|project| self.project_has_key(project, provider)),
        )
    }

    /// The state of the key work for `project` would use for `provider`:
    /// the project's own when it has one, else the app's (or a plan's
    /// sign-in). What screens check before queueing paid work.
    pub(crate) fn key_state_for(&self, project: Option<ProjectId>, provider: Provider) -> KeyState {
        match self.payer_for(project, provider) {
            KeyPayer::Project(project) if !provider.signs_in() => {
                self.provider_keys.project_status(project, provider).state
            }
            _ => self.provider_key(provider).state,
        }
    }

    /// `key_state_for` the open project, where work screens queue.
    pub(crate) fn work_key_state(&self, provider: Provider) -> KeyState {
        self.key_state_for(self.open_project_id().ok().flatten(), provider)
    }

    /// Saves `project`'s own key for `provider`, replacing any earlier one.
    pub fn save_project_key(
        &mut self,
        project: ProjectId,
        provider: Provider,
        input: &str,
    ) -> Result<(), ProviderKeyError> {
        let key = ApiKey::parse(provider, input)?;
        self.save_parsed_project_key(project, provider, key)
    }

    /// Saves the BytePlus card's five fields together as the project's
    /// key, as Settings › API keys does (ADR-0025).
    pub fn save_project_byteplus_key(
        &mut self,
        project: ProjectId,
        fields: [&str; 5],
    ) -> Result<(), ProviderKeyError> {
        let [api_key, access_key_id, secret_access_key, bucket, region] = fields;
        let key = bardo_domain::BytePlusCredentials::compose(
            api_key,
            access_key_id,
            secret_access_key,
            bucket,
            region,
        )
        .map_err(ProviderKeyError::BytePlusFields)?;
        self.save_parsed_project_key(project, Provider::BytePlus, key)
    }

    fn save_parsed_project_key(
        &mut self,
        project: ProjectId,
        provider: Provider,
        key: ApiKey,
    ) -> Result<(), ProviderKeyError> {
        if !provider.has_key() {
            return Err(ProviderKeyError::NotSet);
        }
        if provider == Provider::YouTubeData && !self.profile.accepted_privacy() {
            return Err(ProviderKeyError::PrivacyNotAccepted);
        }
        self.owned_project(project)?;
        let owner = self.profile.id;
        let keys = &mut self.provider_keys;
        keys.redactor().add(&key);
        keys.store()
            .set_project_key(owner, project, provider, &key)
            .map_err(|error| store_failure(keys.redactor(), provider, "save", error))?;
        // Noted after the key is in the store: a job never finds the note
        // without the key. A note that fails takes the key back out, so no
        // secret is left that nothing points to.
        let noted = keys.rows().save_project_key(&ProjectKey {
            project,
            provider,
            saved_at: SystemTime::now(),
        });
        if let Err(error) = noted {
            if let Err(undo) = keys.store().delete_project_key(owner, project, provider) {
                tracing::warn!(provider = provider.code(), %project, "a project's key stayed after its note failed: {undo}");
            }
            return Err(ProviderKeyError::Store(SecretStoreError(
                error.to_string().into(),
            )));
        }
        tracing::info!(provider = provider.code(), %project, "saved a project's key");
        keys.replace_project(project, provider, KeyState::Saved { hint: key.hint() });
        Ok(())
    }

    /// Removes `project`'s own key for `provider`, and the budget that
    /// counted what it paid: the project's work uses the app's key again.
    pub fn remove_project_key(
        &mut self,
        project: ProjectId,
        provider: Provider,
    ) -> Result<(), ProviderKeyError> {
        self.owned_project(project)?;
        let owner = self.profile.id;
        let keys = &mut self.provider_keys;
        // The key goes first: when it cannot, nothing changed. When the
        // note then cannot go, the project still says it has a key that is
        // no longer there, and its work stops rather than pay with the
        // app's key; removing again finishes.
        keys.store()
            .delete_project_key(owner, project, provider)
            .map_err(|error| store_failure(keys.redactor(), provider, "remove", error))?;
        if let Err(error) = keys.rows().remove_project_key(project, provider) {
            keys.replace_project(project, provider, KeyState::Unreadable);
            return Err(ProviderKeyError::Store(SecretStoreError(
                error.to_string().into(),
            )));
        }
        tracing::info!(provider = provider.code(), %project, "removed a project's key");
        keys.replace_project(project, provider, KeyState::NotSet);
        Ok(())
    }

    /// Prepares a test of `project`'s own key, read fresh from the store.
    pub fn project_key_test(
        &self,
        project: ProjectId,
        provider: Provider,
    ) -> Result<KeyTest, ProviderKeyError> {
        if provider == Provider::YouTubeData && !self.profile.accepted_privacy() {
            return Err(ProviderKeyError::PrivacyNotAccepted);
        }
        self.provider_keys
            .project_test(self.profile.id, project, provider)
    }

    /// Sets `project`'s monthly budget for what its own key pays
    /// `provider`. Only a provider the project has its own key for, and
    /// that costs money, has one.
    pub fn set_project_budget(
        &mut self,
        project: ProjectId,
        provider: Provider,
        input: &str,
    ) -> Result<(), CostError> {
        if !provider.is_paid() {
            return Err(CostError::NotPaid(provider));
        }
        if !self.project_has_key(project, provider) {
            return Err(CostError::NoProjectKey(provider));
        }
        let monthly = Money::parse(input).map_err(CostError::InvalidBudget)?;
        self.cost_book.costs.save_project_budget(&ProjectBudget {
            project,
            budget: Budget { provider, monthly },
        })?;
        tracing::info!(provider = provider.code(), %project, "set a project's budget");
        Ok(())
    }

    pub fn remove_project_budget(
        &mut self,
        project: ProjectId,
        provider: Provider,
    ) -> Result<(), CostError> {
        self.cost_book
            .costs
            .remove_project_budget(project, provider)?;
        Ok(())
    }

    fn owned_project(&self, project: ProjectId) -> Result<(), ProviderKeyError> {
        match self.projects.project(project) {
            Ok(Some(found)) if found.owner == self.profile.id => Ok(()),
            Ok(_) => Err(ProviderKeyError::NoProject),
            Err(error) => Err(ProviderKeyError::Store(SecretStoreError(
                error.to_string().into(),
            ))),
        }
    }

    /// The providers `project` keeps its own key for, to remove with it.
    pub(crate) fn project_key_providers(&self, project: ProjectId) -> Vec<Provider> {
        Provider::with_keys()
            .filter(|provider| self.project_has_key(project, *provider))
            .collect()
    }
}

/// What deleting a project does to its keys: removes each from the store,
/// off the UI thread with the rest of the deletion.
pub(crate) struct KeyRemoval {
    pub(crate) owner: ProfileId,
    pub(crate) project: ProjectId,
    pub(crate) providers: Vec<Provider>,
    pub(crate) store: Arc<dyn SecretStore>,
}

impl KeyRemoval {
    /// Removes every key; the first failure, after trying them all.
    pub(crate) fn run(&self) -> Result<(), SecretStoreError> {
        let mut first = None;
        for provider in &self.providers {
            if let Err(error) = self
                .store
                .delete_project_key(self.owner, self.project, *provider)
            {
                tracing::warn!(
                    provider = provider.code(),
                    "could not remove a deleted project's key"
                );
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }
}

/// The keys of each project as the app last knew them.
#[derive(Default)]
pub(crate) struct ProjectSlots(HashMap<(ProjectId, Provider), crate::provider_keys::Slot>);

impl ProjectSlots {
    pub(crate) fn get(
        &self,
        project: ProjectId,
        provider: Provider,
    ) -> Option<&crate::provider_keys::Slot> {
        self.0.get(&(project, provider))
    }

    pub(crate) fn get_mut(
        &mut self,
        project: ProjectId,
        provider: Provider,
    ) -> Option<&mut crate::provider_keys::Slot> {
        self.0.get_mut(&(project, provider))
    }

    pub(crate) fn insert(
        &mut self,
        project: ProjectId,
        provider: Provider,
        slot: crate::provider_keys::Slot,
    ) {
        self.0.insert((project, provider), slot);
    }

    pub(crate) fn remove(&mut self, project: ProjectId, provider: Provider) {
        self.0.remove(&(project, provider));
    }

    /// Forgets every key of `project` (deleted).
    pub(crate) fn forget(&mut self, project: ProjectId) {
        self.0.retain(|(of, _), _| *of != project);
    }

    /// Every project key the app knows of, with its state.
    pub(crate) fn all(&self) -> impl Iterator<Item = (ProjectId, Provider, &KeyState)> {
        self.0
            .iter()
            .map(|((project, provider), slot)| (*project, *provider, &slot.state))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use bardo_domain::{
        Job, JobFailure, JobFailureKind, JobKind, JobRepository, JobState, ProfileRepository as _,
        ProjectColor, ProjectDetails, ProjectDraft, ProjectRepository as _, Redactor, RunnerRole,
        UiLanguage, UserProfile,
    };
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::jobs::{JobContext, JobHandler, JobQueue, JobSettings, Runner};

    const PATIENCE: Duration = Duration::from_secs(60);

    struct Keys {
        db: Arc<Database>,
        store: Arc<MemorySecretStore>,
        owner: ProfileId,
        ring: Arc<KeyRing>,
    }

    impl Keys {
        fn new() -> Self {
            let db = Arc::new(Database::open_in_memory().unwrap());
            let profile = UserProfile::new(UiLanguage::EnUs);
            db.save(&profile).unwrap();
            let store = Arc::new(MemorySecretStore::default());
            let ring = Arc::new(KeyRing::new(Arc::clone(&store) as _, Arc::clone(&db) as _));
            Self {
                db,
                store,
                owner: profile.id,
                ring,
            }
        }

        fn project(&self, name: &str) -> ProjectId {
            let project = bardo_domain::Project::new(
                self.owner,
                ProjectDetails::validate(ProjectDraft {
                    name: name.into(),
                    color: ProjectColor::Teal,
                })
                .unwrap(),
                SystemTime::now(),
            );
            self.db.save_project(&project).unwrap();
            project.id
        }

        /// A key for `provider` whose text tells whose it is.
        fn key(provider: Provider, whose: &str) -> ApiKey {
            let text = match provider {
                Provider::Higgsfield => format!("{whose}-id:{whose}-secret"),
                Provider::BytePlus => format!(
                    "{whose}-api-key-0001|{whose}-access-0001|{whose}-secret-0001|bucket|ap-southeast-1"
                ),
                _ => format!("{whose}-key-0001"),
            };
            ApiKey::parse(provider, &text).unwrap()
        }

        fn save_app(&self, provider: Provider) {
            self.store
                .set(self.owner, provider, &Self::key(provider, "app"))
                .unwrap();
        }

        fn save_own(&self, project: ProjectId, provider: Provider) {
            self.store
                .set_project_key(self.owner, project, provider, &Self::key(provider, "own"))
                .unwrap();
            self.db
                .save_project_key(&ProjectKey {
                    project,
                    provider,
                    saved_at: SystemTime::now(),
                })
                .unwrap();
        }

        fn read(&self, provider: Provider) -> Option<String> {
            self.ring
                .get(self.owner, provider)
                .unwrap()
                .map(|key| key.expose().to_owned())
        }
    }

    #[test]
    fn work_for_a_project_reads_its_own_key_for_every_provider_and_the_apps_otherwise() {
        let keys = Keys::new();
        let (brand, other) = (keys.project("Brand"), keys.project("Other"));
        for provider in Provider::with_keys() {
            let (app, own) = (Keys::key(provider, "app"), Keys::key(provider, "own"));
            keys.save_app(provider);
            keys.save_own(brand, provider);
            {
                let _work = work_for(Some(brand));
                assert_eq!(
                    keys.read(provider).as_deref(),
                    Some(own.expose()),
                    "{provider}"
                );
                assert_eq!(payer_used(provider), Some(KeyPayer::Project(brand)));
            }
            {
                // A project without its own key uses the app's.
                let _work = work_for(Some(other));
                assert_eq!(
                    keys.read(provider).as_deref(),
                    Some(app.expose()),
                    "{provider}"
                );
                assert_eq!(payer_used(provider), Some(KeyPayer::App));
            }
            {
                // Work of no project (a loose video, a global item) too.
                let _work = work_for(None);
                assert_eq!(keys.read(provider).as_deref(), Some(app.expose()));
            }
            // Outside a job: the app's, and nothing is noted.
            assert_eq!(keys.read(provider).as_deref(), Some(app.expose()));
            assert_eq!(payer_used(provider), None);
        }
    }

    #[test]
    fn a_project_key_missing_on_this_computer_is_missing_not_the_apps() {
        let keys = Keys::new();
        let brand = keys.project("Brand");
        keys.save_app(Provider::Claude);
        keys.save_own(brand, Provider::Claude);
        // The database came from another computer: the note, no key.
        keys.store
            .delete_project_key(keys.owner, brand, Provider::Claude)
            .unwrap();
        let _work = work_for(Some(brand));
        assert_eq!(keys.read(Provider::Claude), None);
        assert_eq!(payer_used(Provider::Claude), None);
    }

    #[test]
    fn scopes_nest_and_end_with_their_guard() {
        let keys = Keys::new();
        let brand = keys.project("Brand");
        assert_eq!(working_for(), None);
        let outer = work_for(Some(brand));
        {
            let _inner = work_for(None);
            assert_eq!(working_for(), None);
        }
        assert_eq!(working_for(), Some(brand));
        drop(outer);
        assert_eq!(working_for(), None);
    }

    /// Runs Countdown jobs by reading `provider`'s key through the ring,
    /// as every provider-calling handler does, and keeps what it read.
    struct ReadsKey {
        ring: Arc<KeyRing>,
        owner: ProfileId,
        provider: Provider,
        read: Arc<Mutex<Vec<String>>>,
    }

    impl JobHandler for ReadsKey {
        fn run(&self, _: &str, _: &mut JobContext) -> Result<(), JobFailure> {
            let key = self
                .ring
                .get(self.owner, self.provider)
                .map_err(|error| JobFailure::unexpected(error.to_string()))?
                .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no key"))?;
            self.read.lock().unwrap().push(key.expose().to_owned());
            Ok(())
        }
    }

    fn queue(keys: &Keys, role: RunnerRole, read: &Arc<Mutex<Vec<String>>>) -> JobQueue {
        let mut handlers: HashMap<JobKind, Arc<dyn JobHandler>> = HashMap::new();
        handlers.insert(
            JobKind::Countdown,
            Arc::new(ReadsKey {
                ring: Arc::clone(&keys.ring),
                owner: keys.owner,
                provider: Provider::Claude,
                read: Arc::clone(read),
            }),
        );
        JobQueue::start(
            Arc::clone(&keys.db) as Arc<dyn JobRepository>,
            keys.owner,
            Runner {
                role,
                scope: None,
                paused: true,
            },
            handlers,
            JobSettings::default(),
            Redactor::new(),
        )
        .unwrap()
    }

    fn wait_done(queue: &JobQueue, id: bardo_domain::JobId) -> Job {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(job) = queue
                .jobs()
                .into_iter()
                .find(|job| job.id() == id && !job.state().is_active())
            {
                return job;
            }
            assert!(Instant::now() < deadline, "job {id} never ended");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_job_queued_before_its_projects_key_was_added_runs_with_it_in_the_app_and_the_agent() {
        for role in [RunnerRole::App, RunnerRole::Agent] {
            let keys = Keys::new();
            let brand = keys.project("Brand");
            keys.save_app(Provider::Claude);
            let read = Arc::default();
            let queue = queue(&keys, role, &read);
            // Queued for the project (what Jobs files it under), while no
            // key of its own exists.
            let payload = format!(r#"{{"owner_project":"{brand}"}}"#);
            let id = queue
                .enqueue(Job::new(keys.owner, JobKind::Countdown, payload))
                .unwrap();
            keys.save_own(brand, Provider::Claude);
            queue.pause(false);
            assert_eq!(wait_done(&queue, id).state(), JobState::Done, "{role:?}");
            let own = Keys::key(Provider::Claude, "own");
            assert_eq!(*read.lock().unwrap(), [own.expose().to_owned()], "{role:?}");

            // And a job of no project, beside it, keeps the app's.
            let id = queue
                .enqueue(Job::new(keys.owner, JobKind::Countdown, "{}"))
                .unwrap();
            wait_done(&queue, id);
            let app = Keys::key(Provider::Claude, "app");
            assert_eq!(read.lock().unwrap()[1], app.expose(), "{role:?}");
        }
    }
}
