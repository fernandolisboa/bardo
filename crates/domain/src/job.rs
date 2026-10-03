//! Jobs: long-running work (generation, render, upload, metrics sync) that
//! runs in a queue with progress, cancel, retry and resume (CONTEXT.md).
//!
//! This module holds the job state machine. Running the work, threads and
//! timing belong to the queue in `app`; time comes in as arguments.

use std::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

use uuid::Uuid;

use crate::{ProfileId, ProviderFailureKind, RepositoryError};

/// Identifies a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(Uuid);

impl JobId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for JobId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What a job does. Each kind has one handler in the queue and its own
/// payload format; slices that add long work add a kind here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    /// Built-in test job: counts down in steps, with a checkpoint per step.
    Countdown,
    /// Fetches market data for a channel's seed niches, one checkpoint per
    /// niche.
    NicheResearch,
    /// Has Claude propose themes for a niche, then ranks them with the
    /// decision engine.
    ThemeSuggestion,
    /// Ranks a channel's themes that have no ranking (e.g. after an edit).
    ThemeRanking,
    /// Has Claude write a video project's script from the script template.
    ScriptGeneration,
    /// Reads a video project's script aloud with the persona's voice, one
    /// checkpoint per part of the text.
    Narration,
    /// Copies a recording the user made of a video project's script into
    /// the project folder and has its words timed against the script.
    NarrationImport,
    /// Has Claude split a video project's narration into scenes and write
    /// an image prompt for each.
    ScenePlan,
    /// Draws the images of some of a project's scenes, one checkpoint per
    /// scene. A scene that fails does not stop the others.
    SceneImages,
    /// Animates some of a project's scene images into video clips: submits
    /// each to the video provider, then polls them all, saving each clip as
    /// it is ready. A scene that fails does not stop the others.
    SceneClips,
    /// Builds the editor's preview proxies of a project's media (small
    /// copies of clips and images, waveform peaks of the narration), one
    /// file after another. A file that fails does not stop the others.
    Proxies,
    /// Has Claude write the prompt for a video project's music, for the
    /// user to take to a music tool.
    MusicPrompt,
    /// Renders a video project's cut to one file per chosen network
    /// account, in that account's preset, after the user reviewed it.
    /// Outputs already finished are not rendered again on resume.
    Render,
    /// Has Claude write each network's title, description and tags for a
    /// video project, from the metadata template.
    Metadata,
    /// Writes a video project's export package: one folder per chosen
    /// network with its rendered file and metadata. Networks already
    /// written are not written again on resume.
    Export,
    /// Reads the public statistics of the user's YouTube publications and
    /// keeps a snapshot of each, a batch of posts per checkpoint.
    MetricsSync,
    /// Has the decision engine score a video project's candidate cut
    /// points, a stretch of the script per call and per checkpoint.
    CutSuggestions,
}

impl JobKind {
    pub const ALL: [JobKind; 17] = [
        JobKind::Countdown,
        JobKind::NicheResearch,
        JobKind::ThemeSuggestion,
        JobKind::ThemeRanking,
        JobKind::ScriptGeneration,
        JobKind::Narration,
        JobKind::NarrationImport,
        JobKind::ScenePlan,
        JobKind::SceneImages,
        JobKind::SceneClips,
        JobKind::Proxies,
        JobKind::MusicPrompt,
        JobKind::Render,
        JobKind::Metadata,
        JobKind::Export,
        JobKind::MetricsSync,
        JobKind::CutSuggestions,
    ];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            JobKind::Countdown => "countdown",
            JobKind::NicheResearch => "niche_research",
            JobKind::ThemeSuggestion => "theme_suggestion",
            JobKind::ThemeRanking => "theme_ranking",
            JobKind::ScriptGeneration => "script_generation",
            JobKind::Narration => "narration",
            JobKind::NarrationImport => "narration_import",
            JobKind::ScenePlan => "scene_plan",
            JobKind::SceneImages => "scene_images",
            JobKind::SceneClips => "scene_clips",
            JobKind::Proxies => "proxies",
            JobKind::MusicPrompt => "music_prompt",
            JobKind::Render => "render",
            JobKind::Metadata => "metadata",
            JobKind::Export => "export",
            JobKind::MetricsSync => "metrics_sync",
            JobKind::CutSuggestions => "cut_suggestions",
        }
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown job kind: {0}")]
pub struct UnknownJobKind(pub String);

impl FromStr for JobKind {
    type Err = UnknownJobKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        JobKind::ALL
            .into_iter()
            .find(|kind| kind.code() == s)
            .ok_or_else(|| UnknownJobKind(s.to_owned()))
    }
}

/// Where a job is in its life.
///
/// ```text
/// Queued ──start──▶ Running ──complete──▶ Done
///   ▲   ◀─failed attempt (retry later)─┘ │
///   │   ◀─interrupted by a restart───────┤
///   │                                    ├──failed, no attempts left──▶ Failed
///   │                                    └──cancel──▶ Cancelled
///   └────────────── retry ◀── Failed | Cancelled
/// Queued ──cancel──▶ Cancelled
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobState {
    Queued,
    Running,
    Failed,
    Cancelled,
    Done,
}

impl JobState {
    pub const ALL: [JobState; 5] = [
        JobState::Queued,
        JobState::Running,
        JobState::Failed,
        JobState::Cancelled,
        JobState::Done,
    ];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
            JobState::Done => "done",
        }
    }

    /// Whether the job still has work ahead without the user acting.
    pub fn is_active(self) -> bool {
        matches!(self, JobState::Queued | JobState::Running)
    }
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown job state: {0}")]
pub struct UnknownJobState(pub String);

impl FromStr for JobState {
    type Err = UnknownJobState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        JobState::ALL
            .into_iter()
            .find(|state| state.code() == s)
            .ok_or_else(|| UnknownJobState(s.to_owned()))
    }
}

/// How far a job got, in thousandths, so it stores as an integer and
/// compares exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Progress(u16);

impl Progress {
    pub const ZERO: Progress = Progress(0);
    pub const DONE: Progress = Progress(1000);

    /// `done` out of `total`, clamped to the range; an empty total is done.
    pub fn of(done: u64, total: u64) -> Self {
        if total == 0 || done >= total {
            return Self::DONE;
        }
        // done < total, so the result is below 1000.
        Self((done * 1000 / total) as u16)
    }

    /// Thousandths; values above 1000 are clamped.
    pub fn from_permille(permille: u16) -> Self {
        Self(permille.min(1000))
    }

    pub fn permille(self) -> u16 {
        self.0
    }

    /// Whole percent, rounded down.
    pub fn percent(self) -> u8 {
        (self.0 / 10) as u8
    }
}

/// Why an attempt failed, for the user (by kind) and for logs (detail).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobFailureKind {
    /// The test job failed on purpose. Transient, so it shows the backoff.
    Simulated,
    /// A defect inside Bardo: a panic, an unreadable payload, a missing
    /// handler or a failed save. Retrying would fail the same way.
    Unexpected,
    /// The provider's key is not saved. The user adds it in settings.
    MissingKey,
    /// The provider rejected the saved key.
    KeyRejected,
    /// The key may not do this (permission, API not enabled).
    NotAllowed,
    /// A quota, rate limit or credit balance stops the provider. Retrying
    /// within seconds would hit it again.
    LimitReached,
    /// The provider is down or unreachable. Transient.
    ProviderUnavailable,
    /// The provider answered in a way Bardo does not understand.
    UnexpectedAnswer,
    /// The provider's safety rules declined the request.
    Declined,
    /// ffmpeg could not read or write a media file (a broken or missing
    /// file, or no ffmpeg). Retrying fails the same way until that changes.
    Media,
}

impl JobFailureKind {
    pub const ALL: [JobFailureKind; 10] = [
        JobFailureKind::Simulated,
        JobFailureKind::Unexpected,
        JobFailureKind::MissingKey,
        JobFailureKind::KeyRejected,
        JobFailureKind::NotAllowed,
        JobFailureKind::LimitReached,
        JobFailureKind::ProviderUnavailable,
        JobFailureKind::UnexpectedAnswer,
        JobFailureKind::Declined,
        JobFailureKind::Media,
    ];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            JobFailureKind::Simulated => "simulated",
            JobFailureKind::Unexpected => "unexpected",
            JobFailureKind::MissingKey => "missing_key",
            JobFailureKind::KeyRejected => "key_rejected",
            JobFailureKind::NotAllowed => "not_allowed",
            JobFailureKind::LimitReached => "limit_reached",
            JobFailureKind::ProviderUnavailable => "provider_unavailable",
            JobFailureKind::UnexpectedAnswer => "unexpected_answer",
            JobFailureKind::Declined => "declined",
            JobFailureKind::Media => "media",
        }
    }

    /// Whether another attempt may succeed, so the queue retries it.
    pub fn is_transient(self) -> bool {
        match self {
            JobFailureKind::Simulated | JobFailureKind::ProviderUnavailable => true,
            JobFailureKind::Unexpected
            | JobFailureKind::MissingKey
            | JobFailureKind::KeyRejected
            | JobFailureKind::NotAllowed
            | JobFailureKind::LimitReached
            | JobFailureKind::UnexpectedAnswer
            | JobFailureKind::Declined
            | JobFailureKind::Media => false,
        }
    }
}

impl From<ProviderFailureKind> for JobFailureKind {
    fn from(kind: ProviderFailureKind) -> Self {
        match kind {
            ProviderFailureKind::Rejected => JobFailureKind::KeyRejected,
            ProviderFailureKind::NotAllowed => JobFailureKind::NotAllowed,
            ProviderFailureKind::LimitReached => JobFailureKind::LimitReached,
            ProviderFailureKind::ProviderDown | ProviderFailureKind::Unreachable => {
                JobFailureKind::ProviderUnavailable
            }
            ProviderFailureKind::Unexpected => JobFailureKind::UnexpectedAnswer,
            ProviderFailureKind::Declined => JobFailureKind::Declined,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown job failure kind: {0}")]
pub struct UnknownJobFailureKind(pub String);

impl FromStr for JobFailureKind {
    type Err = UnknownJobFailureKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        JobFailureKind::ALL
            .into_iter()
            .find(|kind| kind.code() == s)
            .ok_or_else(|| UnknownJobFailureKind(s.to_owned()))
    }
}

/// A failed attempt.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {detail}", kind.code())]
pub struct JobFailure {
    pub kind: JobFailureKind,
    /// Technical detail in English, for logs and support.
    pub detail: String,
}

impl JobFailure {
    pub fn new(kind: JobFailureKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub fn unexpected(detail: impl Into<String>) -> Self {
        Self::new(JobFailureKind::Unexpected, detail)
    }
}

/// How often and how patiently a job is retried after transient failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Attempts in total, the first one included.
    pub max_attempts: u32,
    /// Wait after the first failed attempt; each later wait doubles.
    pub first_delay: Duration,
    /// Upper bound for any wait.
    pub max_delay: Duration,
}

impl RetryPolicy {
    /// Wait before the attempt after failed attempt number `failed_attempt`
    /// (1-based): `first_delay`, then doubling, capped at `max_delay`.
    pub fn delay_after(&self, failed_attempt: u32) -> Duration {
        let doublings = failed_attempt.saturating_sub(1).min(31);
        self.first_delay
            .saturating_mul(1 << doublings)
            .min(self.max_delay)
    }
}

impl Default for RetryPolicy {
    /// Four attempts, waiting 5 s, 10 s and 20 s between them.
    fn default() -> Self {
        Self {
            max_attempts: 4,
            first_delay: Duration::from_secs(5),
            max_delay: Duration::from_secs(120),
        }
    }
}

/// An action that does not apply to the job's current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cannot {action} a job that is {from}")]
pub struct InvalidJobTransition {
    pub from: JobState,
    pub action: &'static str,
}

/// Every stored field of a job, for adapters that rebuild one. `Job::restore`
/// checks that the fields agree with each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub id: JobId,
    pub owner: ProfileId,
    pub kind: JobKind,
    pub payload: String,
    pub state: JobState,
    pub progress: Progress,
    pub attempts: u32,
    pub checkpoint: Option<String>,
    pub external_handle: Option<String>,
    pub failure: Option<JobFailure>,
    pub retry_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("stored job is inconsistent: {0}")]
pub struct InconsistentJob(pub &'static str);

/// A unit of long-running work and where it stands. Its fields change only
/// through the transitions below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    id: JobId,
    owner: ProfileId,
    kind: JobKind,
    payload: String,
    state: JobState,
    progress: Progress,
    attempts: u32,
    checkpoint: Option<String>,
    external_handle: Option<String>,
    failure: Option<JobFailure>,
    retry_at: Option<SystemTime>,
}

impl Job {
    /// A queued job that has not run yet. The payload is in the format the
    /// kind's handler reads.
    pub fn new(owner: ProfileId, kind: JobKind, payload: impl Into<String>) -> Self {
        Self {
            id: JobId::new(),
            owner,
            kind,
            payload: payload.into(),
            state: JobState::Queued,
            progress: Progress::ZERO,
            attempts: 0,
            checkpoint: None,
            external_handle: None,
            failure: None,
            retry_at: None,
        }
    }

    /// Rebuilds a stored job, rejecting field combinations no transition
    /// produces (e.g. a failed job without its failure).
    pub fn restore(record: JobRecord) -> Result<Self, InconsistentJob> {
        if record.state == JobState::Failed && record.failure.is_none() {
            return Err(InconsistentJob("a failed job needs its failure"));
        }
        if record.retry_at.is_some() && record.state != JobState::Queued {
            return Err(InconsistentJob("only a queued job waits for a retry"));
        }
        if record.state == JobState::Done && record.progress != Progress::DONE {
            return Err(InconsistentJob("a done job has full progress"));
        }
        Ok(Self {
            id: record.id,
            owner: record.owner,
            kind: record.kind,
            payload: record.payload,
            state: record.state,
            progress: record.progress,
            attempts: record.attempts,
            checkpoint: record.checkpoint,
            external_handle: record.external_handle,
            failure: record.failure,
            retry_at: record.retry_at,
        })
    }

    pub fn id(&self) -> JobId {
        self.id
    }

    pub fn owner(&self) -> ProfileId {
        self.owner
    }

    pub fn kind(&self) -> JobKind {
        self.kind
    }

    pub fn payload(&self) -> &str {
        &self.payload
    }

    pub fn state(&self) -> JobState {
        self.state
    }

    pub fn progress(&self) -> Progress {
        self.progress
    }

    /// Attempts started since the job was queued or last retried by hand.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Where the handler resumes from, in the handler's own format.
    pub fn checkpoint(&self) -> Option<&str> {
        self.checkpoint.as_deref()
    }

    /// The provider's id for work submitted outside Bardo, so a resumed job
    /// polls it instead of submitting (and paying) again.
    pub fn external_handle(&self) -> Option<&str> {
        self.external_handle.as_deref()
    }

    /// The last failed attempt: the final error of a failed job, or why a
    /// queued job is waiting for a retry.
    pub fn failure(&self) -> Option<&JobFailure> {
        self.failure.as_ref()
    }

    /// When a queued job may start again after a failed attempt.
    pub fn retry_at(&self) -> Option<SystemTime> {
        self.retry_at
    }

    /// Whether the job may start at `now`.
    pub fn is_due(&self, now: SystemTime) -> bool {
        self.state == JobState::Queued && self.retry_at.is_none_or(|at| at <= now)
    }

    pub fn can_cancel(&self) -> bool {
        self.state.is_active()
    }

    pub fn can_retry(&self) -> bool {
        matches!(self.state, JobState::Failed | JobState::Cancelled)
    }

    /// Starts an attempt. The job must be queued and due.
    pub fn start(&mut self, now: SystemTime) -> Result<(), InvalidJobTransition> {
        if !self.is_due(now) {
            return Err(self.invalid("start"));
        }
        self.state = JobState::Running;
        self.attempts += 1;
        self.retry_at = None;
        Ok(())
    }

    /// Records progress and, when given, a new checkpoint.
    pub fn record_progress(
        &mut self,
        progress: Progress,
        checkpoint: Option<String>,
    ) -> Result<(), InvalidJobTransition> {
        self.expect_running("record progress of")?;
        self.progress = progress;
        if checkpoint.is_some() {
            self.checkpoint = checkpoint;
        }
        Ok(())
    }

    pub fn record_external_handle(
        &mut self,
        handle: impl Into<String>,
    ) -> Result<(), InvalidJobTransition> {
        self.expect_running("record the external handle of")?;
        self.external_handle = Some(handle.into());
        Ok(())
    }

    pub fn complete(&mut self) -> Result<(), InvalidJobTransition> {
        self.expect_running("complete")?;
        self.state = JobState::Done;
        self.progress = Progress::DONE;
        self.failure = None;
        Ok(())
    }

    /// Ends the running attempt with `failure`. A transient failure with
    /// attempts left queues the job again after the policy's backoff;
    /// anything else fails it until the user retries.
    pub fn fail_attempt(
        &mut self,
        failure: JobFailure,
        now: SystemTime,
        policy: &RetryPolicy,
    ) -> Result<(), InvalidJobTransition> {
        self.expect_running("fail")?;
        if failure.kind.is_transient() && self.attempts < policy.max_attempts {
            self.state = JobState::Queued;
            self.retry_at = Some(now + policy.delay_after(self.attempts));
        } else {
            self.state = JobState::Failed;
        }
        self.failure = Some(failure);
        Ok(())
    }

    /// Stops a queued or running job for good. A running handler is told to
    /// stop by the queue; whatever it reports afterwards is rejected here.
    pub fn cancel(&mut self) -> Result<(), InvalidJobTransition> {
        if !self.can_cancel() {
            return Err(self.invalid("cancel"));
        }
        self.state = JobState::Cancelled;
        self.retry_at = None;
        Ok(())
    }

    /// Queues a failed or cancelled job again with a fresh set of attempts.
    /// Checkpoint and external handle stay, so it resumes where it stopped.
    pub fn retry(&mut self) -> Result<(), InvalidJobTransition> {
        if !self.can_retry() {
            return Err(self.invalid("retry"));
        }
        self.state = JobState::Queued;
        self.attempts = 0;
        self.failure = None;
        self.retry_at = None;
        Ok(())
    }

    /// Queues a job whose attempt was cut short by the app closing. The cut
    /// attempt does not count against the retry policy.
    pub fn interrupt(&mut self) -> Result<(), InvalidJobTransition> {
        self.expect_running("interrupt")?;
        self.state = JobState::Queued;
        self.attempts = self.attempts.saturating_sub(1);
        Ok(())
    }

    fn expect_running(&self, action: &'static str) -> Result<(), InvalidJobTransition> {
        if self.state == JobState::Running {
            Ok(())
        } else {
            Err(self.invalid(action))
        }
    }

    fn invalid(&self, action: &'static str) -> InvalidJobTransition {
        InvalidJobTransition {
            from: self.state,
            action,
        }
    }
}

impl From<&Job> for JobRecord {
    fn from(job: &Job) -> Self {
        Self {
            id: job.id,
            owner: job.owner,
            kind: job.kind,
            payload: job.payload.clone(),
            state: job.state,
            progress: job.progress,
            attempts: job.attempts,
            checkpoint: job.checkpoint.clone(),
            external_handle: job.external_handle.clone(),
            failure: job.failure.clone(),
            retry_at: job.retry_at,
        }
    }
}

/// Persistence port for jobs. Shared with the queue's worker threads, so it
/// must be thread-safe.
pub trait JobRepository: Send + Sync {
    /// The owner's jobs, oldest first.
    fn list(&self, owner: ProfileId) -> Result<Vec<Job>, RepositoryError>;

    /// Inserts or updates the job.
    fn save(&self, job: &Job) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)
    }

    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            first_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(60),
        }
    }

    fn queued() -> Job {
        Job::new(ProfileId::new(), JobKind::Countdown, r#"{"steps":3}"#)
    }

    fn running() -> Job {
        let mut job = queued();
        job.start(now()).unwrap();
        job
    }

    fn transient() -> JobFailure {
        JobFailure::new(JobFailureKind::Simulated, "boom")
    }

    #[test]
    fn a_new_job_is_queued_with_nothing_done() {
        let job = queued();
        assert_eq!(job.state(), JobState::Queued);
        assert_eq!(job.progress(), Progress::ZERO);
        assert_eq!(job.attempts(), 0);
        assert_eq!(job.checkpoint(), None);
        assert!(job.is_due(now()));
    }

    #[test]
    fn starting_runs_the_job_and_counts_the_attempt() {
        let job = running();
        assert_eq!(job.state(), JobState::Running);
        assert_eq!(job.attempts(), 1);
    }

    #[test]
    fn only_a_queued_job_starts() {
        let mut job = running();
        assert_eq!(
            job.start(now()),
            Err(InvalidJobTransition {
                from: JobState::Running,
                action: "start"
            })
        );
        job.complete().unwrap();
        assert!(job.start(now()).is_err());
    }

    #[test]
    fn progress_and_checkpoint_are_recorded_while_running() {
        let mut job = running();
        job.record_progress(Progress::of(1, 3), Some("1".into()))
            .unwrap();
        job.record_progress(Progress::of(2, 3), None).unwrap();
        assert_eq!(job.progress(), Progress::from_permille(666));
        assert_eq!(job.checkpoint(), Some("1"));
    }

    #[test]
    fn progress_is_rejected_unless_running() {
        let mut job = queued();
        assert!(job.record_progress(Progress::DONE, None).is_err());
        assert!(job.record_external_handle("ext-1").is_err());
    }

    #[test]
    fn completing_fills_progress_and_clears_the_last_failure() {
        let mut job = running();
        job.fail_attempt(transient(), now(), &policy()).unwrap();
        job.start(now() + Duration::from_secs(2)).unwrap();
        job.complete().unwrap();
        assert_eq!(job.state(), JobState::Done);
        assert_eq!(job.progress(), Progress::DONE);
        assert_eq!(job.failure(), None);
    }

    #[test]
    fn a_transient_failure_queues_a_retry_after_the_backoff() {
        let mut job = running();
        job.fail_attempt(transient(), now(), &policy()).unwrap();

        assert_eq!(job.state(), JobState::Queued);
        assert_eq!(job.retry_at(), Some(now() + Duration::from_secs(2)));
        assert_eq!(job.failure(), Some(&transient()));
        assert!(!job.is_due(now() + Duration::from_secs(1)));
        assert!(job.is_due(now() + Duration::from_secs(2)));
        assert!(job.start(now()).is_err());
    }

    #[test]
    fn backoff_doubles_until_attempts_run_out_then_the_job_fails() {
        let policy = policy();
        let mut job = running();
        let mut at = now();
        let mut waits = Vec::new();
        while job.state() == JobState::Running {
            job.fail_attempt(transient(), at, &policy).unwrap();
            if let Some(retry_at) = job.retry_at() {
                waits.push(retry_at.duration_since(at).unwrap());
                at = retry_at;
                job.start(at).unwrap();
            }
        }
        assert_eq!(waits, [Duration::from_secs(2), Duration::from_secs(4)]);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.attempts(), 3);
        assert_eq!(job.failure(), Some(&transient()));
        assert_eq!(job.retry_at(), None);
    }

    #[test]
    fn an_unexpected_failure_is_not_retried() {
        let mut job = running();
        job.fail_attempt(JobFailure::unexpected("panic"), now(), &policy())
            .unwrap();
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.retry_at(), None);
    }

    #[test]
    fn delay_is_capped() {
        let policy = RetryPolicy {
            max_attempts: 100,
            first_delay: Duration::from_secs(5),
            max_delay: Duration::from_secs(30),
        };
        assert_eq!(policy.delay_after(1), Duration::from_secs(5));
        assert_eq!(policy.delay_after(3), Duration::from_secs(20));
        assert_eq!(policy.delay_after(4), Duration::from_secs(30));
        assert_eq!(policy.delay_after(90), Duration::from_secs(30));
    }

    #[test]
    fn queued_and_running_jobs_can_be_cancelled() {
        let mut job = queued();
        job.cancel().unwrap();
        assert_eq!(job.state(), JobState::Cancelled);

        let mut job = running();
        job.cancel().unwrap();
        assert_eq!(job.state(), JobState::Cancelled);
    }

    #[test]
    fn a_job_waiting_for_a_retry_can_be_cancelled() {
        let mut job = running();
        job.fail_attempt(transient(), now(), &policy()).unwrap();
        job.cancel().unwrap();
        assert_eq!(job.state(), JobState::Cancelled);
        assert_eq!(job.retry_at(), None);
    }

    #[test]
    fn a_cancelled_job_ignores_late_reports_from_its_handler() {
        let mut job = running();
        job.cancel().unwrap();
        assert!(job.record_progress(Progress::DONE, None).is_err());
        assert!(job.complete().is_err());
        assert!(job.fail_attempt(transient(), now(), &policy()).is_err());
        assert_eq!(job.state(), JobState::Cancelled);
    }

    #[test]
    fn finished_jobs_cannot_be_cancelled() {
        let mut job = running();
        job.complete().unwrap();
        assert!(!job.can_cancel());
        assert!(job.cancel().is_err());
    }

    #[test]
    fn retry_requeues_failed_and_cancelled_jobs_with_fresh_attempts() {
        let mut failed = running();
        failed
            .record_progress(Progress::of(1, 2), Some("1".into()))
            .unwrap();
        failed
            .fail_attempt(JobFailure::unexpected("x"), now(), &policy())
            .unwrap();
        failed.retry().unwrap();
        assert_eq!(failed.state(), JobState::Queued);
        assert_eq!(failed.attempts(), 0);
        assert_eq!(failed.failure(), None);
        assert!(failed.is_due(now()));
        assert_eq!(failed.checkpoint(), Some("1"), "resumes where it stopped");

        let mut cancelled = queued();
        cancelled.cancel().unwrap();
        cancelled.retry().unwrap();
        assert_eq!(cancelled.state(), JobState::Queued);
    }

    #[test]
    fn active_and_done_jobs_cannot_be_retried() {
        assert!(queued().retry().is_err());
        assert!(running().retry().is_err());
        let mut done = running();
        done.complete().unwrap();
        assert!(!done.can_retry());
        assert!(done.retry().is_err());
    }

    #[test]
    fn an_interrupted_job_is_queued_again_without_using_an_attempt() {
        let mut job = running();
        job.record_progress(Progress::of(1, 3), Some("1".into()))
            .unwrap();
        job.record_external_handle("ext-42").unwrap();
        job.interrupt().unwrap();

        assert_eq!(job.state(), JobState::Queued);
        assert_eq!(job.attempts(), 0);
        assert_eq!(job.checkpoint(), Some("1"));
        assert_eq!(job.external_handle(), Some("ext-42"));
        assert!(job.is_due(now()));
    }

    #[test]
    fn only_running_jobs_are_interrupted() {
        assert!(queued().interrupt().is_err());
    }

    #[test]
    fn progress_of_counts_and_clamps() {
        assert_eq!(Progress::of(0, 4), Progress::ZERO);
        assert_eq!(Progress::of(1, 4).permille(), 250);
        assert_eq!(Progress::of(1, 3).percent(), 33);
        assert_eq!(Progress::of(5, 4), Progress::DONE);
        assert_eq!(Progress::of(0, 0), Progress::DONE);
        assert_eq!(Progress::from_permille(5000), Progress::DONE);
    }

    #[test]
    fn codes_round_trip() {
        for kind in JobKind::ALL {
            assert_eq!(kind.code().parse(), Ok(kind));
        }
        for state in JobState::ALL {
            assert_eq!(state.code().parse(), Ok(state));
        }
        for kind in JobFailureKind::ALL {
            assert_eq!(kind.code().parse(), Ok(kind));
        }
        assert!("rendering".parse::<JobState>().is_err());
    }

    #[test]
    fn only_outages_among_provider_failures_are_retried() {
        let retried: Vec<_> = [
            ProviderFailureKind::Rejected,
            ProviderFailureKind::NotAllowed,
            ProviderFailureKind::LimitReached,
            ProviderFailureKind::ProviderDown,
            ProviderFailureKind::Unreachable,
            ProviderFailureKind::Unexpected,
        ]
        .into_iter()
        .filter(|&kind| JobFailureKind::from(kind).is_transient())
        .collect();
        assert_eq!(
            retried,
            [
                ProviderFailureKind::ProviderDown,
                ProviderFailureKind::Unreachable
            ]
        );
        assert!(!JobFailureKind::MissingKey.is_transient());
    }

    #[test]
    fn a_job_round_trips_through_its_record() {
        let mut job = running();
        job.record_progress(Progress::of(1, 3), Some("1".into()))
            .unwrap();
        job.fail_attempt(transient(), now(), &policy()).unwrap();
        assert_eq!(Job::restore(JobRecord::from(&job)), Ok(job));
    }

    #[test]
    fn inconsistent_records_are_rejected() {
        let base = JobRecord::from(&queued());
        let failed_without_failure = JobRecord {
            state: JobState::Failed,
            ..base.clone()
        };
        let running_with_retry = JobRecord {
            state: JobState::Running,
            retry_at: Some(now()),
            ..base.clone()
        };
        let done_halfway = JobRecord {
            state: JobState::Done,
            progress: Progress::of(1, 2),
            ..base
        };
        for record in [failed_without_failure, running_with_retry, done_halfway] {
            assert!(Job::restore(record).is_err());
        }
    }
}
