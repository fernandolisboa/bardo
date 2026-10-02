//! Job use cases: start, cancel and retry jobs, and group them for the jobs
//! panel. The queue itself lives in `queue`.

mod countdown;
mod queue;

use std::collections::HashMap;
use std::sync::Arc;

use bardo_domain::{Job, JobId, JobKind, JobState};

pub use countdown::TestJob;
pub(crate) use queue::JobQueue;
pub use queue::{JobActionError, JobContext, JobHandler, JobSettings};

use crate::narrations::NarrationHandler;
use crate::research::NicheResearchHandler;
use crate::scenes::SceneHandler;
use crate::scripts::ScriptHandler;
use crate::themes::ThemeHandler;
use crate::{AppError, Bardo, Text};

/// The handler of every job kind Bardo ships.
pub(crate) fn built_in_handlers(
    research: NicheResearchHandler,
    themes: ThemeHandler,
    scripts: ScriptHandler,
    narrations: NarrationHandler,
    scenes: SceneHandler,
) -> HashMap<JobKind, Arc<dyn JobHandler>> {
    let themes = Arc::new(themes);
    let mut handlers: HashMap<JobKind, Arc<dyn JobHandler>> = HashMap::new();
    handlers.insert(JobKind::Countdown, Arc::new(countdown::Countdown));
    handlers.insert(JobKind::NicheResearch, Arc::new(research));
    handlers.insert(JobKind::ThemeSuggestion, Arc::clone(&themes) as _);
    handlers.insert(JobKind::ThemeRanking, themes);
    handlers.insert(JobKind::ScriptGeneration, Arc::new(scripts));
    handlers.insert(JobKind::Narration, Arc::new(narrations));
    let scenes = Arc::new(scenes);
    handlers.insert(JobKind::ScenePlan, Arc::clone(&scenes) as _);
    handlers.insert(JobKind::SceneImages, scenes);
    handlers
}

impl JobActionError {
    /// What the jobs panel says when cancel or retry did not happen.
    pub fn message(&self) -> Text {
        Text::JobNotUpdated
    }
}

/// Jobs as the panel shows them: what runs, what waits, what needs the
/// user, and what is over.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JobGroups {
    /// Oldest first.
    pub running: Vec<Job>,
    /// Next to start first, including jobs waiting to retry.
    pub queued: Vec<Job>,
    /// Oldest first.
    pub failed: Vec<Job>,
    /// Done and cancelled, newest first.
    pub finished: Vec<Job>,
}

impl JobGroups {
    pub fn group(jobs: Vec<Job>) -> Self {
        let mut groups = Self::default();
        for job in jobs {
            match job.state() {
                JobState::Running => groups.running.push(job),
                JobState::Queued => groups.queued.push(job),
                JobState::Failed => groups.failed.push(job),
                JobState::Cancelled | JobState::Done => groups.finished.push(job),
            }
        }
        groups.finished.reverse();
        groups
    }

    /// Jobs still working or waiting to.
    pub fn active(&self) -> usize {
        self.running.len() + self.queued.len()
    }

    pub fn is_empty(&self) -> bool {
        self.active() + self.failed.len() + self.finished.len() == 0
    }
}

impl Bardo {
    /// Every job of the profile, as of now.
    pub fn jobs(&self) -> Vec<Job> {
        self.jobs.jobs()
    }

    pub fn job_groups(&self) -> JobGroups {
        JobGroups::group(self.jobs())
    }

    /// Grows whenever any job changes, so a view can poll it cheaply and
    /// re-read `jobs` only when it moved.
    pub fn jobs_revision(&self) -> u64 {
        self.jobs.revision()
    }

    pub fn job_settings(&self) -> JobSettings {
        self.jobs.settings()
    }

    pub fn start_test_job(&self, job: TestJob) -> Result<JobId, AppError> {
        let job = Job::new(self.profile.id, JobKind::Countdown, job.payload());
        Ok(self.jobs.enqueue(job)?)
    }

    pub fn cancel_job(&self, id: JobId) -> Result<(), JobActionError> {
        self.jobs.cancel(id)
    }

    /// Queues a failed or cancelled job again; it resumes from its last
    /// checkpoint.
    pub fn retry_job(&self, id: JobId) -> Result<(), JobActionError> {
        self.jobs.retry(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::{Duration, Instant, SystemTime};

    use bardo_domain::{
        ApiKey, JobFailure, JobFailureKind, JobRepository, ProfileId, ProfileRepository, Progress,
        Provider, Redactor, RepositoryError, RetryPolicy, UiLanguage, UserProfile,
    };
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::Repositories;

    const PATIENCE: Duration = Duration::from_secs(10);

    type RunFn = dyn Fn(&str, &mut JobContext) -> Result<(), JobFailure> + Send + Sync;

    /// A handler written inline in a test. Shared, so the same handler can
    /// serve the queue before and after a restart.
    struct FnHandler(Arc<RunFn>);

    impl JobHandler for FnHandler {
        fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
            (self.0)(payload, cx)
        }
    }

    fn shared_handlers(run: &Arc<RunFn>) -> HashMap<JobKind, Arc<dyn JobHandler>> {
        let mut handlers: HashMap<JobKind, Arc<dyn JobHandler>> = HashMap::new();
        handlers.insert(JobKind::Countdown, Arc::new(FnHandler(Arc::clone(run))));
        handlers
    }

    fn handlers(
        run: impl Fn(&str, &mut JobContext) -> Result<(), JobFailure> + Send + Sync + 'static,
    ) -> HashMap<JobKind, Arc<dyn JobHandler>> {
        shared_handlers(&(Arc::new(run) as Arc<RunFn>))
    }

    /// Runs until told to stop, as a long provider call would.
    fn until_stopped(cx: &JobContext) {
        while cx.sleep(Duration::from_millis(2)) {}
    }

    fn settings() -> JobSettings {
        JobSettings {
            max_running: 2,
            retry: RetryPolicy {
                max_attempts: 3,
                first_delay: Duration::from_millis(40),
                max_delay: Duration::from_secs(1),
            },
        }
    }

    fn database_with_profile(db: Database) -> (Arc<Database>, ProfileId) {
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        (Arc::new(db), profile.id)
    }

    fn memory_db() -> (Arc<Database>, ProfileId) {
        database_with_profile(Database::open_in_memory().unwrap())
    }

    fn queue(
        db: &Arc<Database>,
        owner: ProfileId,
        handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
        settings: JobSettings,
    ) -> JobQueue {
        JobQueue::start(
            Arc::clone(db) as Arc<dyn JobRepository>,
            owner,
            handlers,
            settings,
            Redactor::new(),
        )
        .unwrap()
    }

    fn enqueue(queue: &JobQueue, owner: ProfileId) -> JobId {
        queue
            .enqueue(Job::new(owner, JobKind::Countdown, "{}"))
            .unwrap()
    }

    /// Polls snapshots, as the UI does, until the job matches.
    fn wait_for(jobs: impl Fn() -> Vec<Job>, id: JobId, matches: impl Fn(&Job) -> bool) -> Job {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let job = jobs().into_iter().find(|job| job.id() == id);
            if let Some(job) = job.filter(|job| matches(job)) {
                return job;
            }
            assert!(Instant::now() < deadline, "job {id} never matched");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn stored(db: &Database, owner: ProfileId, id: JobId) -> Job {
        JobRepository::list(db, owner)
            .unwrap()
            .into_iter()
            .find(|job| job.id() == id)
            .unwrap()
    }

    #[test]
    fn a_queued_job_runs_to_done_and_is_saved() {
        let (db, owner) = memory_db();
        let q = queue(&db, owner, handlers(|_, _| Ok(())), settings());
        let id = enqueue(&q, owner);

        let done = wait_for(|| q.jobs(), id, |j| j.state() == JobState::Done);
        assert_eq!(done.attempts(), 1);
        assert_eq!(done.progress(), Progress::DONE);
        drop(q);
        assert_eq!(stored(&db, owner, id).state(), JobState::Done);
    }

    #[test]
    fn progress_is_visible_while_the_job_runs() {
        let (db, owner) = memory_db();
        let q = queue(
            &db,
            owner,
            handlers(|_, cx| {
                cx.report_progress(Progress::from_permille(420));
                until_stopped(cx);
                Ok(())
            }),
            settings(),
        );
        let before = q.revision();
        let id = enqueue(&q, owner);

        let running = wait_for(
            || q.jobs(),
            id,
            |j| j.progress() == Progress::from_permille(420),
        );
        assert_eq!(running.state(), JobState::Running);
        assert!(q.revision() > before);
    }

    #[test]
    fn cancel_stops_a_running_job_and_marks_it_cancelled() {
        let (db, owner) = memory_db();
        let stopped = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&stopped);
        let q = queue(
            &db,
            owner,
            handlers(move |_, cx| {
                until_stopped(cx);
                seen.store(true, Ordering::SeqCst);
                // Whatever the handler returns after a cancel is ignored.
                Ok(())
            }),
            settings(),
        );
        let id = enqueue(&q, owner);
        wait_for(|| q.jobs(), id, |j| j.state() == JobState::Running);

        q.cancel(id).unwrap();

        assert_eq!(
            wait_for(|| q.jobs(), id, |_| stopped.load(Ordering::SeqCst)).state(),
            JobState::Cancelled
        );
        assert_eq!(stored(&db, owner, id).state(), JobState::Cancelled);
    }

    #[test]
    fn a_cancelled_queued_job_never_runs() {
        let (db, owner) = memory_db();
        let runs = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&runs);
        let q = queue(
            &db,
            owner,
            handlers(move |_, cx| {
                counter.fetch_add(1, Ordering::SeqCst);
                until_stopped(cx);
                Ok(())
            }),
            JobSettings {
                max_running: 1,
                ..settings()
            },
        );
        let first = enqueue(&q, owner);
        let second = enqueue(&q, owner);
        wait_for(|| q.jobs(), first, |j| j.state() == JobState::Running);
        assert_eq!(
            q.jobs()[1].state(),
            JobState::Queued,
            "limit of one running job"
        );

        q.cancel(second).unwrap();
        q.cancel(first).unwrap();
        wait_for(|| q.jobs(), first, |j| j.state() == JobState::Cancelled);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(q.jobs()[1].state(), JobState::Cancelled);
    }

    #[test]
    fn known_keys_are_masked_in_failures_before_they_are_stored() {
        let (db, owner) = memory_db();
        let redactor = Redactor::new();
        redactor.add(&ApiKey::parse(Provider::Claude, "sk-ant-job-secret-0001").unwrap());
        let q = JobQueue::start(
            Arc::clone(&db) as Arc<dyn JobRepository>,
            owner,
            handlers(|_, _| {
                Err(JobFailure::new(
                    JobFailureKind::Simulated,
                    "401 for x-api-key sk-ant-job-secret-0001",
                ))
            }),
            JobSettings {
                retry: RetryPolicy {
                    max_attempts: 1,
                    ..settings().retry
                },
                ..settings()
            },
            redactor,
        )
        .unwrap();
        let id = enqueue(&q, owner);

        let failed = wait_for(|| q.jobs(), id, |j| j.state() == JobState::Failed);
        assert_eq!(
            failed.failure().unwrap().detail,
            "401 for x-api-key [redacted]"
        );
        let stored = JobRepository::list(&*db, owner).unwrap();
        assert_eq!(
            stored[0].failure().unwrap().detail,
            "401 for x-api-key [redacted]"
        );
    }

    #[test]
    fn a_failing_job_retries_with_backoff_then_fails_until_retried_by_hand() {
        let (db, owner) = memory_db();
        let starts = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let log = Arc::clone(&starts);
        let q = queue(
            &db,
            owner,
            handlers(move |_, _| {
                log.lock().unwrap().push(Instant::now());
                Err(JobFailure::new(
                    JobFailureKind::Simulated,
                    "provider timeout",
                ))
            }),
            settings(),
        );
        let id = enqueue(&q, owner);

        let failed = wait_for(|| q.jobs(), id, |j| j.state() == JobState::Failed);
        assert_eq!(failed.attempts(), 3);
        assert_eq!(failed.failure().unwrap().detail, "provider timeout");
        {
            let starts = starts.lock().unwrap();
            assert_eq!(starts.len(), 3);
            assert!(starts[1] - starts[0] >= Duration::from_millis(40));
            assert!(starts[2] - starts[1] >= Duration::from_millis(80));
        }

        q.retry(id).unwrap();
        wait_for(
            || q.jobs(),
            id,
            |j| j.state() == JobState::Failed && starts.lock().unwrap().len() == 6,
        );
    }

    #[test]
    fn a_transient_failure_can_be_followed_by_success() {
        let (db, owner) = memory_db();
        let q = queue(
            &db,
            owner,
            handlers(|_, cx| {
                if cx.attempt() == 1 {
                    Err(JobFailure::new(JobFailureKind::Simulated, "blip"))
                } else {
                    Ok(())
                }
            }),
            settings(),
        );
        let id = enqueue(&q, owner);

        let done = wait_for(|| q.jobs(), id, |j| j.state() == JobState::Done);
        assert_eq!(done.attempts(), 2);
        assert_eq!(done.failure(), None);
    }

    #[test]
    fn a_waiting_retry_shows_the_failure_and_when_it_retries() {
        let (db, owner) = memory_db();
        let q = queue(
            &db,
            owner,
            handlers(|_, _| Err(JobFailure::new(JobFailureKind::Simulated, "blip"))),
            JobSettings {
                retry: RetryPolicy {
                    first_delay: Duration::from_secs(60),
                    ..settings().retry
                },
                ..settings()
            },
        );
        let id = enqueue(&q, owner);

        let waiting = wait_for(|| q.jobs(), id, |j| j.failure().is_some());
        assert_eq!(waiting.state(), JobState::Queued);
        assert!(waiting.retry_at().unwrap() > SystemTime::now());

        q.cancel(id).unwrap();
        assert_eq!(q.jobs()[0].state(), JobState::Cancelled);
    }

    #[test]
    fn a_panicking_handler_fails_the_job_without_retries() {
        let (db, owner) = memory_db();
        let runs = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&runs);
        let q = queue(
            &db,
            owner,
            handlers(move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                panic!("bug in handler")
            }),
            settings(),
        );
        let id = enqueue(&q, owner);

        let failed = wait_for(|| q.jobs(), id, |j| j.state() == JobState::Failed);
        let failure = failed.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::Unexpected);
        assert!(failure.detail.contains("bug in handler"), "{failure}");
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_job_without_a_handler_fails() {
        let (db, owner) = memory_db();
        let q = queue(&db, owner, HashMap::new(), settings());
        let id = enqueue(&q, owner);
        let failed = wait_for(|| q.jobs(), id, |j| j.state() == JobState::Failed);
        assert_eq!(failed.failure().unwrap().kind, JobFailureKind::Unexpected);
    }

    #[test]
    fn closing_mid_job_resumes_from_the_last_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let (db, owner) = database_with_profile(Database::open(&path).unwrap());
        let resumed_from = Arc::new(Mutex::new(None::<String>));
        let seen = Arc::clone(&resumed_from);
        let run = move |_: &str, cx: &mut JobContext| match cx.checkpoint() {
            None => {
                cx.save_checkpoint("step-1", Progress::from_permille(500))
                    .unwrap();
                until_stopped(cx);
                Ok(())
            }
            Some(checkpoint) => {
                *seen.lock().unwrap() = Some(checkpoint.to_owned());
                Ok(())
            }
        };
        let run: Arc<RunFn> = Arc::new(run);

        let first = queue(&db, owner, shared_handlers(&run), settings());
        let id = enqueue(&first, owner);
        wait_for(|| first.jobs(), id, |j| j.checkpoint() == Some("step-1"));
        drop(first);
        drop(db);

        let reopened = Arc::new(Database::open(&path).unwrap());
        let left = stored(&reopened, owner, id);
        assert_eq!(left.state(), JobState::Running, "closing is not cancelling");
        assert_eq!(left.progress(), Progress::from_permille(500));

        let second = queue(&reopened, owner, shared_handlers(&run), settings());
        let done = wait_for(|| second.jobs(), id, |j| j.state() == JobState::Done);
        assert_eq!(resumed_from.lock().unwrap().as_deref(), Some("step-1"));
        assert_eq!(done.attempts(), 1, "the interrupted attempt does not count");
    }

    #[test]
    fn a_resumed_job_polls_its_external_handle_instead_of_resubmitting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let (db, owner) = database_with_profile(Database::open(&path).unwrap());
        let submissions = Arc::new(AtomicU32::new(0));
        let polled = Arc::new(Mutex::new(None::<String>));
        let (submitted, seen) = (Arc::clone(&submissions), Arc::clone(&polled));
        let run: Arc<RunFn> = Arc::new(move |_: &str, cx: &mut JobContext| {
            match cx.external_handle() {
                None => {
                    submitted.fetch_add(1, Ordering::SeqCst);
                    cx.save_external_handle("provider-123").unwrap();
                    until_stopped(cx);
                }
                Some(handle) => *seen.lock().unwrap() = Some(handle.to_owned()),
            }
            Ok(())
        });

        let first = queue(&db, owner, shared_handlers(&run), settings());
        let id = enqueue(&first, owner);
        wait_for(|| first.jobs(), id, |j| j.external_handle().is_some());
        drop(first);

        let second = queue(&db, owner, shared_handlers(&run), settings());
        wait_for(|| second.jobs(), id, |j| j.state() == JobState::Done);
        assert_eq!(submissions.load(Ordering::SeqCst), 1);
        assert_eq!(polled.lock().unwrap().as_deref(), Some("provider-123"));
    }

    #[test]
    fn a_job_waiting_to_retry_keeps_waiting_after_a_restart() {
        let (db, owner) = memory_db();
        let mut job = Job::new(owner, JobKind::Countdown, "{}");
        job.start(SystemTime::now()).unwrap();
        job.fail_attempt(
            JobFailure::new(JobFailureKind::Simulated, "blip"),
            SystemTime::now(),
            &RetryPolicy {
                first_delay: Duration::from_secs(3600),
                ..RetryPolicy::default()
            },
        )
        .unwrap();
        JobRepository::save(&*db, &job).unwrap();

        let q = queue(&db, owner, handlers(|_, _| Ok(())), settings());
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(q.jobs()[0].state(), JobState::Queued);
    }

    #[test]
    fn cancel_and_retry_check_the_job() {
        let (db, owner) = memory_db();
        let q = queue(
            &db,
            owner,
            handlers(|_, cx| {
                until_stopped(cx);
                Ok(())
            }),
            settings(),
        );
        assert!(matches!(
            q.cancel(JobId::new()),
            Err(JobActionError::NotFound)
        ));
        let id = enqueue(&q, owner);
        wait_for(|| q.jobs(), id, |j| j.state() == JobState::Running);
        assert!(matches!(q.retry(id), Err(JobActionError::NotAllowed(_))));
    }

    #[test]
    fn a_failed_save_changes_nothing() {
        struct Broken;
        impl JobRepository for Broken {
            fn list(&self, _: ProfileId) -> Result<Vec<Job>, RepositoryError> {
                Ok(Vec::new())
            }
            fn save(&self, _: &Job) -> Result<(), RepositoryError> {
                Err(RepositoryError("disk full".into()))
            }
        }

        let q = JobQueue::start(
            Arc::new(Broken),
            ProfileId::new(),
            handlers(|_, _| Ok(())),
            settings(),
            Redactor::new(),
        )
        .unwrap();
        assert!(
            q.enqueue(Job::new(ProfileId::new(), JobKind::Countdown, "{}"))
                .is_err()
        );
        assert!(q.jobs().is_empty());
    }

    // Use cases through `Bardo`, with the real test job.

    fn fast(steps: u32) -> TestJob {
        TestJob {
            steps,
            step: Duration::from_millis(20),
            fail_at_step: None,
        }
    }

    fn start(db: &Arc<Database>) -> Bardo {
        let repositories = Repositories {
            profiles: Box::new(Arc::clone(db)),
            channels: Box::new(Arc::clone(db)),
            jobs: Arc::clone(db) as Arc<dyn JobRepository>,
            themes: Arc::clone(db) as _,
            templates: Arc::clone(db) as _,
            scripts: Arc::clone(db) as _,
            personas: Arc::clone(db) as _,
            narrations: Arc::clone(db) as _,
            scene_plans: Arc::clone(db) as _,
            network_accounts: Arc::clone(db) as _,
            costs: Arc::clone(db) as _,
            files: Arc::new(bardo_storage::MemoryProjectFiles::default()),
            research: Arc::clone(db) as _,
            secrets: Arc::new(MemorySecretStore::default()),
        };
        Bardo::start_with(
            repositories,
            crate::testing::providers(),
            Some("en-US"),
            settings(),
        )
        .unwrap()
    }

    #[test]
    fn the_test_job_shows_live_progress_and_finishes() {
        let app = start(&Arc::new(Database::open_in_memory().unwrap()));
        let id = app.start_test_job(fast(5)).unwrap();

        let midway = wait_for(
            || app.jobs(),
            id,
            |j| j.state() == JobState::Running && j.progress() > Progress::ZERO,
        );
        assert!(midway.progress() < Progress::DONE);
        let done = wait_for(|| app.jobs(), id, |j| j.state() == JobState::Done);
        assert_eq!(done.checkpoint(), Some("5"));
        assert_eq!(app.job_groups().finished, [done]);
    }

    #[test]
    fn the_failing_test_job_ends_with_a_clear_error_and_can_be_retried() {
        let app = start(&Arc::new(Database::open_in_memory().unwrap()));
        let id = app
            .start_test_job(TestJob {
                fail_at_step: Some(2),
                ..fast(4)
            })
            .unwrap();

        let failed = wait_for(|| app.jobs(), id, |j| j.state() == JobState::Failed);
        assert_eq!(failed.failure().unwrap().kind, JobFailureKind::Simulated);
        assert_eq!(failed.attempts(), 3);
        assert_eq!(
            failed.checkpoint(),
            Some("2"),
            "steps before the failure stay done"
        );
        assert_eq!(app.job_groups().failed, [failed]);

        app.retry_job(id).unwrap();
        assert_ne!(app.jobs()[0].state(), JobState::Failed);
    }

    #[test]
    fn closing_the_app_mid_test_job_resumes_it_from_the_last_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let open = || Arc::new(Database::open(&path).unwrap());

        let app = start(&open());
        let id = app.start_test_job(fast(30)).unwrap();
        wait_for(
            || app.jobs(),
            id,
            |j| j.checkpoint().and_then(|c| c.parse::<u32>().ok()) >= Some(3),
        );
        drop(app);

        let restarted = start(&open());
        let resumed = restarted.jobs().remove(0);
        let at_close: u32 = resumed.checkpoint().unwrap().parse().unwrap();
        assert!(at_close >= 3);
        assert!(resumed.progress() >= Progress::of(3, 30));
        assert!(resumed.state().is_active());

        let done = wait_for(|| restarted.jobs(), id, |j| j.state() == JobState::Done);
        assert_eq!(done.attempts(), 1);
    }

    #[test]
    fn groups_split_jobs_for_the_panel() {
        let owner = ProfileId::new();
        let now = SystemTime::now();
        let make = |state: JobState| {
            let mut job = Job::new(owner, JobKind::Countdown, "{}");
            if state != JobState::Queued {
                job.start(now).unwrap();
            }
            match state {
                JobState::Done => job.complete().unwrap(),
                JobState::Cancelled => job.cancel().unwrap(),
                JobState::Failed => job
                    .fail_attempt(JobFailure::unexpected("x"), now, &RetryPolicy::default())
                    .unwrap(),
                JobState::Queued | JobState::Running => {}
            }
            job
        };
        let jobs = vec![
            make(JobState::Done),
            make(JobState::Running),
            make(JobState::Queued),
            make(JobState::Failed),
            make(JobState::Cancelled),
            make(JobState::Queued),
        ];

        let groups = JobGroups::group(jobs.clone());
        assert_eq!(groups.running, [jobs[1].clone()]);
        assert_eq!(groups.queued, [jobs[2].clone(), jobs[5].clone()]);
        assert_eq!(groups.failed, [jobs[3].clone()]);
        assert_eq!(groups.finished, [jobs[4].clone(), jobs[0].clone()]);
        assert_eq!(groups.active(), 3);
        assert!(JobGroups::group(Vec::new()).is_empty());
    }
}
