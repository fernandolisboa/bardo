//! The persistent job queue: runs due jobs on worker threads, records their
//! progress, retries transient failures with backoff and resumes interrupted
//! jobs from their last checkpoint when the app starts again.
//!
//! The UI never waits on a job. It reads snapshots (`jobs`) and watches
//! `revision` to know when to read again.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use bardo_domain::{
    InvalidJobTransition, Job, JobFailure, JobId, JobKind, JobRepository, JobState, ProfileId,
    Progress, Redactor, RepositoryError, RetryPolicy,
};

/// Runs one kind of job. Handlers run on a worker thread, read their payload
/// and resume point from the context, and should return soon after
/// `JobContext::should_stop` turns true.
pub trait JobHandler: Send + Sync {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure>;
}

/// How the queue runs jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobSettings {
    /// Jobs running at the same time; the rest wait in the queue.
    pub max_running: usize,
    pub retry: RetryPolicy,
}

impl Default for JobSettings {
    fn default() -> Self {
        Self {
            max_running: 2,
            retry: RetryPolicy::default(),
        }
    }
}

/// Why a cancel or retry did not happen.
#[derive(Debug, thiserror::Error)]
pub enum JobActionError {
    #[error("job not found")]
    NotFound,
    #[error(transparent)]
    NotAllowed(#[from] InvalidJobTransition),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

/// Tells a running handler to stop, because the user cancelled the job or
/// the app is closing. Handlers cannot tell the two apart and need not.
#[derive(Default)]
struct StopSignal {
    stopped: Mutex<bool>,
    changed: Condvar,
}

impl StopSignal {
    fn stop(&self) {
        *lock(&self.stopped) = true;
        self.changed.notify_all();
    }

    fn is_stopped(&self) -> bool {
        *lock(&self.stopped)
    }

    /// Waits `duration` unless stopped first. True when the time ran out.
    fn sleep(&self, duration: Duration) -> bool {
        let stopped = lock(&self.stopped);
        let (stopped, _) = self
            .changed
            .wait_timeout_while(stopped, duration, |stopped| !*stopped)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !*stopped
    }
}

/// What a handler gets: where to resume, and how to report back.
pub struct JobContext {
    id: JobId,
    kind: JobKind,
    attempt: u32,
    checkpoint: Option<String>,
    external_handle: Option<String>,
    stop: Arc<StopSignal>,
    shared: Arc<Shared>,
}

impl JobContext {
    /// The running job.
    pub fn id(&self) -> JobId {
        self.id
    }

    /// The running job's kind, for handlers that serve several.
    pub fn kind(&self) -> JobKind {
        self.kind
    }

    /// 1 for the first attempt, counting automatic retries.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// The last saved checkpoint, to resume from.
    pub fn checkpoint(&self) -> Option<&str> {
        self.checkpoint.as_deref()
    }

    /// The provider handle saved by an earlier attempt. When present, poll
    /// that work instead of submitting it again.
    pub fn external_handle(&self) -> Option<&str> {
        self.external_handle.as_deref()
    }

    /// True once the job is cancelled or the app is closing.
    pub fn should_stop(&self) -> bool {
        self.stop.is_stopped()
    }

    /// Waits `duration`, waking early when told to stop. True when the full
    /// time passed, false when the handler should return.
    pub fn sleep(&self, duration: Duration) -> bool {
        self.stop.sleep(duration)
    }

    /// Shows progress in the UI. Not saved: after a restart the job shows
    /// the progress of its last checkpoint.
    pub fn report_progress(&self, progress: Progress) {
        let _ =
            self.shared
                .update_running(self.id, |job| job.record_progress(progress, None), false);
    }

    /// Saves where to resume from, with the progress reached there.
    pub fn save_checkpoint(
        &mut self,
        checkpoint: impl Into<String>,
        progress: Progress,
    ) -> Result<(), RepositoryError> {
        let checkpoint = checkpoint.into();
        self.checkpoint = Some(checkpoint.clone());
        self.shared.update_running(
            self.id,
            |job| job.record_progress(progress, Some(checkpoint)),
            true,
        )
    }

    /// Saves the provider's handle for submitted work before waiting on it,
    /// so a restart polls instead of paying for the work twice.
    pub fn save_external_handle(
        &mut self,
        handle: impl Into<String>,
    ) -> Result<(), RepositoryError> {
        let handle = handle.into();
        self.external_handle = Some(handle.clone());
        self.shared
            .update_running(self.id, |job| job.record_external_handle(handle), true)
    }
}

struct State {
    /// Every job of the profile, oldest first; the source of truth while
    /// the app runs, written through to the repository.
    jobs: Vec<Job>,
    /// Stop signals of jobs whose worker thread has not finished.
    workers: HashMap<JobId, Arc<StopSignal>>,
    closing: bool,
}

impl State {
    fn job_mut(&mut self, id: JobId) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|job| job.id() == id)
    }
}

struct Shared {
    repository: Arc<dyn JobRepository>,
    handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
    settings: JobSettings,
    /// Failure details are stored and shown, so known keys are masked first.
    redactor: Redactor,
    state: Mutex<State>,
    /// Wakes the scheduler: a job was queued or finished, or the app closes.
    wake: Condvar,
    revision: AtomicU64,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::Release);
    }

    /// Masks known keys in the failure and logs it.
    fn redacted(&self, id: JobId, failure: JobFailure) -> JobFailure {
        let failure = JobFailure {
            detail: self.redactor.redact(&failure.detail),
            ..failure
        };
        tracing::warn!(job = %id, kind = failure.kind.code(), detail = %failure.detail, "job attempt failed");
        failure
    }

    /// Applies a handler report to its job. Reports for a job that is no
    /// longer running (cancelled meanwhile) are dropped, and nothing is
    /// written once the app is closing.
    fn update_running(
        &self,
        id: JobId,
        apply: impl FnOnce(&mut Job) -> Result<(), InvalidJobTransition>,
        persist: bool,
    ) -> Result<(), RepositoryError> {
        let mut state = self.state();
        if state.closing {
            return Ok(());
        }
        let Some(job) = state.job_mut(id) else {
            return Ok(());
        };
        if apply(job).is_err() {
            return Ok(());
        }
        let saved = if persist {
            self.repository.save(job)
        } else {
            Ok(())
        };
        self.changed();
        saved
    }

    /// Records how a worker's attempt ended.
    fn finish(&self, id: JobId, result: Result<(), JobFailure>) {
        let mut state = self.state();
        if state.closing {
            // The job stays running in storage and resumes on next start.
            return;
        }
        state.workers.remove(&id);
        let retry = self.settings.retry;
        if let Some(job) = state.job_mut(id) {
            let ended = match result {
                Ok(()) => job.complete(),
                Err(failure) => {
                    let failure = self.redacted(id, failure);
                    job.fail_attempt(failure, SystemTime::now(), &retry)
                }
            };
            if ended.is_ok() {
                // If this save fails, the job runs again from its last
                // checkpoint after a restart; nothing is lost.
                let _ = self.repository.save(job);
            }
        }
        self.changed();
        self.wake.notify_all();
    }

    /// Changes one job through `apply` on a copy, saves the copy and only
    /// then keeps it, so a failed save leaves the job as it was.
    fn transition(
        &self,
        id: JobId,
        apply: impl FnOnce(&mut Job) -> Result<(), InvalidJobTransition>,
    ) -> Result<JobState, JobActionError> {
        let mut state = self.state();
        let job = state.job_mut(id).ok_or(JobActionError::NotFound)?;
        let mut updated = job.clone();
        apply(&mut updated)?;
        self.repository.save(&updated)?;
        *job = updated;
        let new_state = job.state();
        if new_state == JobState::Cancelled
            && let Some(stop) = state.workers.get(&id)
        {
            stop.stop();
        }
        self.changed();
        self.wake.notify_all();
        Ok(new_state)
    }
}

/// See the module docs.
pub(crate) struct JobQueue {
    shared: Arc<Shared>,
    scheduler: Option<JoinHandle<()>>,
}

impl JobQueue {
    /// Loads the owner's jobs, queues again those a closed app left running,
    /// and starts scheduling.
    pub(crate) fn start(
        repository: Arc<dyn JobRepository>,
        owner: ProfileId,
        handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
        settings: JobSettings,
        redactor: Redactor,
    ) -> Result<Self, RepositoryError> {
        let mut jobs = repository.list(owner)?;
        for job in jobs.iter_mut().filter(|j| j.state() == JobState::Running) {
            job.interrupt().expect("a running job can be interrupted");
            repository.save(job)?;
        }
        let shared = Arc::new(Shared {
            repository,
            handlers,
            settings,
            redactor,
            state: Mutex::new(State {
                jobs,
                workers: HashMap::new(),
                closing: false,
            }),
            wake: Condvar::new(),
            revision: AtomicU64::new(0),
        });
        let scheduler = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("job-scheduler".into())
                .spawn(move || schedule(&shared))
                .map_err(|error| RepositoryError(Box::new(error)))?
        };
        Ok(Self {
            shared,
            scheduler: Some(scheduler),
        })
    }

    /// Every job, oldest first, as of now.
    pub(crate) fn jobs(&self) -> Vec<Job> {
        self.shared.state().jobs.clone()
    }

    pub(crate) fn settings(&self) -> JobSettings {
        self.shared.settings
    }

    /// Grows whenever any job changes, progress included.
    pub(crate) fn revision(&self) -> u64 {
        self.shared.revision.load(Ordering::Acquire)
    }

    pub(crate) fn enqueue(&self, job: Job) -> Result<JobId, RepositoryError> {
        let mut state = self.shared.state();
        self.shared.repository.save(&job)?;
        let id = job.id();
        state.jobs.push(job);
        self.shared.changed();
        self.shared.wake.notify_all();
        Ok(id)
    }

    pub(crate) fn cancel(&self, id: JobId) -> Result<(), JobActionError> {
        self.shared.transition(id, Job::cancel).map(drop)
    }

    pub(crate) fn retry(&self, id: JobId) -> Result<(), JobActionError> {
        self.shared.transition(id, Job::retry).map(drop)
    }
}

impl Drop for JobQueue {
    /// Tells running handlers to stop and waits for the scheduler. Running
    /// jobs stay running in storage, so the next start resumes them. Once
    /// this returns, no worker writes to the repository again.
    fn drop(&mut self) {
        {
            let mut state = self.shared.state();
            state.closing = true;
            for stop in state.workers.values() {
                stop.stop();
            }
        }
        self.shared.wake.notify_all();
        if let Some(scheduler) = self.scheduler.take() {
            let _ = scheduler.join();
        }
    }
}

/// Scheduler thread: starts due jobs up to the limit, then sleeps until a
/// job changes or the earliest retry is due.
fn schedule(shared: &Arc<Shared>) {
    let mut state = shared.state();
    loop {
        if state.closing {
            return;
        }
        let now = SystemTime::now();
        while state.workers.len() < shared.settings.max_running {
            let Some(job) = state.jobs.iter_mut().find(|job| job.is_due(now)) else {
                break;
            };
            job.start(now).expect("a due job starts");
            // A failed save leaves the job queued in storage; it runs now
            // and, after a restart, again.
            let _ = shared.repository.save(job);
            let job = job.clone();
            let stop = Arc::new(StopSignal::default());
            state.workers.insert(job.id(), Arc::clone(&stop));
            if let Err(error) = spawn_worker(shared, &job, stop) {
                state.workers.remove(&job.id());
                if let Some(job) = state.job_mut(job.id()) {
                    let failure = shared.redacted(
                        job.id(),
                        JobFailure::unexpected(format!("could not start: {error}")),
                    );
                    let _ = job.fail_attempt(failure, now, &shared.settings.retry);
                    let _ = shared.repository.save(job);
                }
            }
            shared.changed();
        }

        let next_retry = state
            .jobs
            .iter()
            .filter(|job| job.state() == JobState::Queued)
            .filter_map(Job::retry_at)
            .filter(|at| *at > now)
            .min();
        state = match next_retry {
            Some(at) => {
                let wait = at.duration_since(now).unwrap_or_default();
                shared
                    .wake
                    .wait_timeout(state, wait)
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .0
            }
            None => shared
                .wake
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        };
    }
}

fn spawn_worker(shared: &Arc<Shared>, job: &Job, stop: Arc<StopSignal>) -> std::io::Result<()> {
    let handler = shared.handlers.get(&job.kind()).cloned();
    let kind = job.kind();
    let payload = job.payload().to_owned();
    let mut cx = JobContext {
        id: job.id(),
        kind,
        attempt: job.attempts(),
        checkpoint: job.checkpoint().map(str::to_owned),
        external_handle: job.external_handle().map(str::to_owned),
        stop,
        shared: Arc::clone(shared),
    };
    thread::Builder::new()
        .name(format!("job-{kind}"))
        .spawn(move || {
            let result = match handler {
                None => Err(JobFailure::unexpected(format!("no handler for {kind}"))),
                Some(handler) => {
                    panic::catch_unwind(AssertUnwindSafe(|| handler.run(&payload, &mut cx)))
                        .unwrap_or_else(|panic| Err(JobFailure::unexpected(panic_message(&*panic))))
                }
            };
            let shared = Arc::clone(&cx.shared);
            shared.finish(cx.id, result);
        })
        .map(drop)
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    let message = panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned());
    format!("handler panicked: {message}")
}

/// A poisoned lock only means a thread panicked while holding it; the job
/// list stays consistent because every change is a single assignment.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
