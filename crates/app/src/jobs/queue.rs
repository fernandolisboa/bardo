//! The persistent job queue: runs due jobs on worker threads, records their
//! progress, retries transient failures with backoff and resumes interrupted
//! jobs from their last checkpoint when the app starts again.
//!
//! The UI never waits on a job. It reads snapshots (`jobs`) and watches
//! `revision` to know when to read again.
//!
//! The app and the background agent each run a queue over the same
//! database (`bardo_domain::runner`). A queue leases a job before it runs
//! it, renews its leases, says it is up and reads what the other runner
//! changed every tick, and leaves a job another runner holds alone.

use std::collections::{HashMap, HashSet};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use bardo_domain::{
    InvalidJobTransition, JOB_LEASE, Job, JobFailure, JobId, JobKind, JobRepository, JobState,
    ProfileId, Progress, RUNNER_ALIVE, RUNNER_TICK, Redactor, RepositoryError, RetryPolicy,
    RunnerRole, RunnerSeen,
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
    /// How long a lease on a running job holds without being renewed.
    pub lease: Duration,
    /// How often the queue renews its leases, says it is up and reads what
    /// another runner changed. Well under `lease`.
    pub tick: Duration,
}

impl Default for JobSettings {
    fn default() -> Self {
        Self {
            max_running: 2,
            retry: RetryPolicy::default(),
            lease: JOB_LEASE,
            tick: RUNNER_TICK,
        }
    }
}

/// Which jobs a runner takes.
pub(crate) type JobScope = Arc<dyn Fn(&Job) -> bool + Send + Sync>;

/// The process a queue runs in, and the jobs it takes.
#[derive(Clone)]
pub(crate) struct Runner {
    pub(crate) role: RunnerRole,
    /// Every job of the profile when `None`.
    pub(crate) scope: Option<JobScope>,
    /// Whether the queue starts paused (`JobQueue::pause`).
    pub(crate) paused: bool,
}

impl Runner {
    /// Bardo's window: every job.
    pub(crate) fn app() -> Self {
        Self {
            role: RunnerRole::App,
            scope: None,
            paused: false,
        }
    }

    fn takes(&self, job: &Job) -> bool {
        self.scope.as_ref().is_none_or(|scope| scope(job))
    }
}

/// How long a closing queue waits for its stopped handlers to return, so
/// the next runner finds their jobs free at once.
const CLOSING_WAIT: Duration = Duration::from_secs(2);

/// Why a cancel or retry did not happen.
#[derive(Debug, thiserror::Error)]
pub enum JobActionError {
    #[error("job not found")]
    NotFound,
    /// The other Bardo process (the app or the background agent) runs it
    /// now.
    #[error("the job runs in another Bardo process")]
    Elsewhere,
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
    /// Set by `defer`: the attempt ends waiting, not done.
    deferred: Option<SystemTime>,
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

    /// Asks the queue to run the job again at `until` once the handler
    /// returns `Ok`, instead of counting it done: it waits on something
    /// outside Bardo (a network's publishing limit). The job stays queued,
    /// with its checkpoint and external handle, and holds no place in the
    /// queue meanwhile. A handler that defers and then fails fails as
    /// usual.
    pub fn defer(&mut self, until: SystemTime) {
        self.deferred = Some(until);
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
    /// Every job of the profile this runner takes, oldest first, written
    /// through to the repository and read again from it every tick (another
    /// runner may have changed it).
    jobs: Vec<Job>,
    /// Stop signals of jobs whose worker thread has not finished.
    workers: HashMap<JobId, Arc<StopSignal>>,
    /// Jobs another runner's live lease held when last read.
    elsewhere: HashSet<JobId>,
    /// The other runners up when last read.
    others: Vec<RunnerSeen>,
    /// Starts no job while true; running ones go on.
    paused: bool,
    closing: bool,
    /// When the queue next renews, says it is up and reads storage.
    next_tick: Instant,
}

impl State {
    fn job_mut(&mut self, id: JobId) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|job| job.id() == id)
    }
}

struct Shared {
    repository: Arc<dyn JobRepository>,
    owner: ProfileId,
    runner: Runner,
    /// When this queue started.
    started_at: SystemTime,
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

    /// Records how a worker's attempt ended: done, failed, or waiting
    /// until `deferred`.
    fn finish(&self, id: JobId, result: Result<(), JobFailure>, deferred: Option<SystemTime>) {
        let mut state = self.state();
        state.workers.remove(&id);
        if state.closing {
            // The job stays running in storage, and its handler is done
            // with it: the next runner resumes it at once.
            let _ = self.repository.release(id);
            self.wake.notify_all();
            return;
        }
        let retry = self.settings.retry;
        if let Some(job) = state.job_mut(id) {
            let ended = match result {
                Ok(()) => match deferred {
                    Some(until) => job.defer(until),
                    None => job.complete(),
                },
                Err(failure) => {
                    let failure = self.redacted(id, failure);
                    job.fail_attempt(failure, SystemTime::now(), &retry)
                }
            };
            if ended.is_ok() {
                // If this save fails, the job runs again from its last
                // checkpoint after a restart; nothing is lost. Refused when
                // another runner took the job meanwhile: its copy wins on
                // the next tick.
                let _ = self.repository.save(job);
            }
        }
        let _ = self.repository.release(id);
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
        let running_here = state.workers.contains_key(&id);
        let job = state.job_mut(id).ok_or(JobActionError::NotFound)?;
        if !running_here && let Ok(Some(stored)) = self.repository.job(id) {
            // Another runner may have moved it on since the last tick.
            *job = stored;
        }
        let mut updated = job.clone();
        apply(&mut updated)?;
        match self.repository.save(&updated) {
            Ok(()) => {}
            Err(error) if error.is_held_elsewhere() => return Err(JobActionError::Elsewhere),
            Err(error) => return Err(error.into()),
        }
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
        runner: Runner,
        handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
        settings: JobSettings,
        redactor: Redactor,
    ) -> Result<Self, RepositoryError> {
        let now = SystemTime::now();
        repository.heartbeat(runner.role, now, now)?;
        let others = repository.runners(now.checked_sub(RUNNER_ALIVE).unwrap_or(now))?;
        let elsewhere = repository.held_elsewhere(owner, now)?;
        let mut jobs: Vec<Job> = repository
            .list(owner)?
            .into_iter()
            .filter(|job| runner.takes(job))
            .collect();
        // Jobs a closed app (or a dead runner) left running.
        for job in jobs
            .iter_mut()
            .filter(|job| job.state() == JobState::Running && !elsewhere.contains(&job.id()))
        {
            if repository.lease(job.id(), now, now + settings.lease)? {
                job.interrupt().expect("a running job can be interrupted");
                repository.save(job)?;
                repository.release(job.id())?;
            }
        }
        let paused = runner.paused;
        let shared = Arc::new(Shared {
            repository,
            owner,
            runner,
            started_at: now,
            handlers,
            settings,
            redactor,
            state: Mutex::new(State {
                jobs,
                workers: HashMap::new(),
                elsewhere,
                others,
                paused,
                closing: false,
                next_tick: Instant::now() + settings.tick,
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

    /// The other runners (the app, the background agent) up as of the last
    /// tick.
    pub(crate) fn others(&self) -> Vec<RunnerSeen> {
        self.shared.state().others.clone()
    }

    /// Stops starting jobs while `paused`; running ones go on.
    pub(crate) fn pause(&self, paused: bool) {
        let mut state = self.shared.state();
        if state.paused != paused {
            state.paused = paused;
            self.shared.wake.notify_all();
        }
    }

    #[cfg(test)]
    pub(crate) fn is_paused(&self) -> bool {
        self.shared.state().paused
    }

    /// Whether the job runs in another runner's process as of the last
    /// tick.
    pub(crate) fn held_elsewhere(&self, id: JobId) -> bool {
        self.shared.state().elsewhere.contains(&id)
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

    /// Queues a failed or cancelled job again, like `retry`, to start no
    /// earlier than `at` (now when `None`).
    pub(crate) fn retry_at(&self, id: JobId, at: Option<SystemTime>) -> Result<(), JobActionError> {
        self.shared
            .transition(id, |job| {
                job.retry()?;
                match at {
                    Some(at) => job.wait_until(at),
                    None => Ok(()),
                }
            })
            .map(drop)
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
        // Stopped handlers give up their leases as they return (`finish`):
        // wait for them a little, so the next runner resumes their jobs at
        // once. One still busy keeps its lease until it runs out.
        let deadline = Instant::now() + CLOSING_WAIT;
        let mut state = self.shared.state();
        while !state.workers.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = self
                .shared
                .wake
                .wait_timeout(state, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        drop(state);
        let _ = self.shared.repository.leave();
    }
}

/// Scheduler thread: every tick renews leases, says the runner is up and
/// reads storage again; in between it starts due jobs up to the limit, then
/// sleeps until a job changes, the earliest retry or time is due, or the
/// next tick.
fn schedule(shared: &Arc<Shared>) {
    let mut state = shared.state();
    loop {
        if state.closing {
            return;
        }
        if Instant::now() >= state.next_tick {
            tick(shared, &mut state);
            state.next_tick = Instant::now() + shared.settings.tick;
        }
        let now = SystemTime::now();
        if !state.paused {
            start_due(shared, &mut state, now);
        }

        // A queued job waits for its backoff and its time, whichever is
        // later.
        let next_retry = state
            .jobs
            .iter()
            .filter(|job| job.state() == JobState::Queued)
            .filter_map(|job| job.retry_at().max(job.run_at()))
            .filter(|at| *at > now)
            .min()
            .map(|at| at.duration_since(now).unwrap_or_default());
        let next_tick = state.next_tick.saturating_duration_since(Instant::now());
        let wait = next_retry.map_or(next_tick, |retry| retry.min(next_tick));
        state = shared
            .wake
            .wait_timeout(state, wait)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0;
    }
}

/// Starts due jobs up to the limit, each once this runner leases it.
fn start_due(shared: &Arc<Shared>, state: &mut MutexGuard<'_, State>, now: SystemTime) {
    let repository = &shared.repository;
    let mut tried = HashSet::new();
    while state.workers.len() < shared.settings.max_running {
        // A job whose cancelled run is still stopping waits for it: a job
        // never has two workers. A job whose time came goes before the
        // others, oldest time first: its time is a promise.
        let State {
            jobs,
            workers,
            elsewhere,
            ..
        } = &mut **state;
        let Some(index) = jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| {
                job.is_due(now)
                    && !workers.contains_key(&job.id())
                    && !elsewhere.contains(&job.id())
                    && !tried.contains(&job.id())
            })
            .min_by_key(|(_, job)| job.run_at().map_or((1, None), |at| (0, Some(at))))
            .map(|(index, _)| index)
        else {
            break;
        };
        let id = jobs[index].id();
        tried.insert(id);
        match repository.lease(id, now, now + shared.settings.lease) {
            Ok(true) => {}
            Ok(false) => {
                elsewhere.insert(id);
                continue;
            }
            Err(error) => {
                tracing::warn!(job = %id, "could not lease a job: {error}");
                continue;
            }
        }
        // What another runner did with it since this copy was read.
        match repository.job(id) {
            Ok(Some(stored)) => jobs[index] = stored,
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(job = %id, "could not read a leased job: {error}");
                let _ = repository.release(id);
                continue;
            }
        }
        let job = &mut jobs[index];
        if job.state() == JobState::Running {
            // Left running by a runner that is gone.
            job.interrupt().expect("a running job can be interrupted");
            let _ = repository.save(job);
        }
        if !job.is_due(now) {
            let _ = repository.release(id);
            shared.changed();
            continue;
        }
        job.start(now).expect("a due job starts");
        // A failed save leaves the job queued in storage; it runs now
        // and, after a restart, again.
        let _ = repository.save(job);
        let job = job.clone();
        let stop = Arc::new(StopSignal::default());
        state.workers.insert(id, Arc::clone(&stop));
        if let Err(error) = spawn_worker(shared, &job, stop) {
            state.workers.remove(&id);
            if let Some(job) = state.job_mut(id) {
                let failure = shared.redacted(
                    id,
                    JobFailure::unexpected(format!("could not start: {error}")),
                );
                let _ = job.fail_attempt(failure, now, &shared.settings.retry);
                let _ = repository.save(job);
            }
            let _ = repository.release(id);
        }
        shared.changed();
    }
}

/// Renews this runner's leases, says it is up, and reads the jobs and the
/// other runners again.
fn tick(shared: &Arc<Shared>, state: &mut MutexGuard<'_, State>) {
    let repository = &shared.repository;
    let now = SystemTime::now();
    if let Err(error) = repository.heartbeat(shared.runner.role, shared.started_at, now) {
        tracing::warn!("could not say the job runner is up: {error}");
    }
    match repository.runners(now.checked_sub(RUNNER_ALIVE).unwrap_or(now)) {
        Ok(others) => {
            // The app shows whether the agent runs: a view watching the
            // revision reads it again when one comes or goes.
            let roles = |seen: &[RunnerSeen]| seen.iter().map(|r| r.role).collect::<HashSet<_>>();
            if roles(&others) != roles(&state.others) {
                shared.changed();
            }
            state.others = others;
        }
        Err(error) => tracing::warn!("could not read the other job runners: {error}"),
    }
    let running: Vec<JobId> = state.workers.keys().copied().collect();
    if !running.is_empty() {
        match repository.renew(&running, now + shared.settings.lease) {
            Ok(lost) => {
                for id in lost {
                    // Another runner took it (this process slept past its
                    // lease): this run stops, and that one's goes on.
                    tracing::warn!(job = %id, "lost the lease on a running job");
                    if let Some(stop) = state.workers.get(&id) {
                        stop.stop();
                    }
                }
            }
            Err(error) => tracing::warn!("could not renew job leases: {error}"),
        }
    }
    reconcile(shared, state, now);
}

/// Reads the runner's jobs from storage again: another runner may have
/// queued, run, finished or cancelled any of them. Jobs running here keep
/// their copy (it has the live progress). A job left running by a runner
/// that is gone is queued again.
fn reconcile(shared: &Arc<Shared>, state: &mut MutexGuard<'_, State>, now: SystemTime) {
    let repository = &shared.repository;
    let (stored, held) = match (
        repository.list(shared.owner),
        repository.held_elsewhere(shared.owner, now),
    ) {
        (Ok(stored), Ok(held)) => (stored, held),
        (Err(error), _) | (_, Err(error)) => {
            tracing::warn!("could not read the jobs again: {error}");
            return;
        }
    };
    let mut jobs = Vec::with_capacity(stored.len());
    for mut job in stored {
        let id = job.id();
        if state.workers.contains_key(&id)
            && let Some(ours) = state.jobs.iter().find(|ours| ours.id() == id)
        {
            jobs.push(ours.clone());
            continue;
        }
        if !shared.runner.takes(&job) {
            continue;
        }
        if job.state() == JobState::Running
            && !held.contains(&id)
            && repository
                .lease(id, now, now + shared.settings.lease)
                .unwrap_or(false)
        {
            if let Ok(Some(stored)) = repository.job(id) {
                job = stored;
            }
            if job.state() == JobState::Running {
                job.interrupt().expect("a running job can be interrupted");
                let _ = repository.save(&job);
            }
            let _ = repository.release(id);
        }
        jobs.push(job);
    }
    // A job running here that storage lost stays until its run ends.
    for ours in &state.jobs {
        if state.workers.contains_key(&ours.id()) && !jobs.iter().any(|j| j.id() == ours.id()) {
            jobs.push(ours.clone());
        }
    }
    let changed = jobs != state.jobs || held != state.elsewhere;
    state.jobs = jobs;
    state.elsewhere = held;
    if changed {
        shared.changed();
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
        deferred: None,
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
            shared.finish(cx.id, result, cx.deferred);
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
