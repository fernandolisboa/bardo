//! Uploads (ADR-0008, PRD stories 84, 85 and 88): Bardo sends a rendered
//! file to the network after the user reviewed it, and keeps the post as an
//! uploaded publication with a status.
//!
//! An upload is resumable: the network hands out a session, the file goes
//! in chunks, and the bytes the network confirmed are never sent again. The
//! adapter (`VideoUploader`) speaks the network's protocol; what it needs
//! from the app (a fresh token, the file, where to keep the session and the
//! confirmed bytes) comes through `UploadRun`, so a job can resume after a
//! restart from what it saved.

use std::fmt;
use std::time::{Duration, SystemTime};

use crate::{JobFailureKind, JobId, Network, SecretText, Visibility};

/// The unit chunk sizes are multiples of: YouTube takes chunks in
/// multiples of 256 KiB, except the last one.
pub const UPLOAD_CHUNK_UNIT: u64 = 256 * 1024;

/// How much one request sends: 8 MiB, small enough to hold in memory and
/// to lose little when a request fails, large enough to keep the requests
/// few.
pub const UPLOAD_CHUNK: u64 = 32 * UPLOAD_CHUNK_UNIT;

/// Why an uploaded publication failed, for the user.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UploadFailure {
    /// The project's daily uploads bucket is used up (YouTube: 100 a day
    /// per Google Cloud project). It refills the next day.
    QuotaExceeded,
    /// The channel uploaded as much as the network lets it for now.
    UploadLimit,
    /// The account's sign-in no longer works: reconnect it.
    ReconnectNeeded,
    /// The network took the file and then rejected the video, with its
    /// reason (YouTube's `rejectionReason`: `copyright`, `duplicate`, …).
    Rejected(String),
    /// The network could not process the file, with its reason (YouTube's
    /// `failureReason`: `codec`, `invalidFile`, …).
    ProcessingFailed(String),
    /// The video was removed on the network before it was ready.
    Removed,
    /// The rendered file changed after the review (rendered again while
    /// the upload was stopped): the rest of it is not what was reviewed.
    RenderChanged,
    /// The publish time passed before the upload could start: the network
    /// would publish the video at once. The user reviews it again.
    ScheduleMissed,
    /// The upload job failed for another reason.
    Job(JobFailureKind),
}

impl UploadFailure {
    /// Stable text for storage.
    pub fn code(&self) -> String {
        match self {
            UploadFailure::QuotaExceeded => "quota".to_owned(),
            UploadFailure::UploadLimit => "upload_limit".to_owned(),
            UploadFailure::ReconnectNeeded => "reconnect".to_owned(),
            UploadFailure::Rejected(reason) => format!("rejected:{reason}"),
            UploadFailure::ProcessingFailed(reason) => format!("processing:{reason}"),
            UploadFailure::Removed => "removed".to_owned(),
            UploadFailure::RenderChanged => "render_changed".to_owned(),
            UploadFailure::ScheduleMissed => "schedule_missed".to_owned(),
            UploadFailure::Job(kind) => format!("job:{}", kind.code()),
        }
    }

    /// Reads a stored code. An unknown one reads as an unexpected failure,
    /// so a newer version's reason still shows as a failure.
    pub fn from_code(code: &str) -> Self {
        match code.split_once(':') {
            Some(("rejected", reason)) => UploadFailure::Rejected(reason.to_owned()),
            Some(("processing", reason)) => UploadFailure::ProcessingFailed(reason.to_owned()),
            Some(("job", kind)) => {
                UploadFailure::Job(kind.parse().unwrap_or(JobFailureKind::Unexpected))
            }
            _ => match code {
                "quota" => UploadFailure::QuotaExceeded,
                "upload_limit" => UploadFailure::UploadLimit,
                "reconnect" => UploadFailure::ReconnectNeeded,
                "removed" => UploadFailure::Removed,
                "render_changed" => UploadFailure::RenderChanged,
                "schedule_missed" => UploadFailure::ScheduleMissed,
                _ => UploadFailure::Job(JobFailureKind::Unexpected),
            },
        }
    }
}

/// Where an uploaded publication stands.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UploadStatus {
    /// Reviewed and waiting for the job to start.
    Queued,
    /// The file is going to the network.
    Uploading,
    /// The network has the file and is processing it.
    Processing,
    /// On the network, private until its publish time (`Upload::publish_at`),
    /// when the network makes it public by itself.
    Scheduled,
    /// On the network with the visibility the upload asked for.
    Published,
    /// On the network, but kept private by it although the upload asked
    /// for unlisted or public: the user's API project has not passed the
    /// network's audit (YouTube's API Services audit).
    Restricted,
    Failed(UploadFailure),
}

impl UploadStatus {
    /// Stable name for storage; a failure keeps its reason apart
    /// (`UploadFailure::code`).
    pub fn code(&self) -> &'static str {
        match self {
            UploadStatus::Queued => "queued",
            UploadStatus::Uploading => "uploading",
            UploadStatus::Processing => "processing",
            UploadStatus::Scheduled => "scheduled",
            UploadStatus::Published => "published",
            UploadStatus::Restricted => "restricted",
            UploadStatus::Failed(_) => "failed",
        }
    }

    /// Reads a stored status and, for a failure, its reason.
    pub fn from_code(code: &str, failure: Option<&str>) -> Option<Self> {
        Some(match code {
            "queued" => UploadStatus::Queued,
            "uploading" => UploadStatus::Uploading,
            "processing" => UploadStatus::Processing,
            "scheduled" => UploadStatus::Scheduled,
            "published" => UploadStatus::Published,
            "restricted" => UploadStatus::Restricted,
            "failed" => UploadStatus::Failed(UploadFailure::from_code(failure.unwrap_or(""))),
            _ => return None,
        })
    }

    /// Whether the video is on the network and nothing more will happen to
    /// the upload.
    pub fn is_final(&self) -> bool {
        matches!(self, UploadStatus::Published | UploadStatus::Restricted)
    }

    /// Whether the network has the video, live or waiting for its publish
    /// time: the upload job has nothing left to do.
    pub fn is_on_network(&self) -> bool {
        self.is_final() || *self == UploadStatus::Scheduled
    }

    pub fn failure(&self) -> Option<&UploadFailure> {
        match self {
            UploadStatus::Failed(failure) => Some(failure),
            _ => None,
        }
    }
}

/// An upload step that does not apply to the status it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cannot {action} an upload that is {from}")]
pub struct InvalidUploadTransition {
    pub from: &'static str,
    pub action: &'static str,
}

/// The upload side of an uploaded publication: its status, the
/// visibility it asked for, its publish time when scheduled, and the job
/// that sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub status: UploadStatus,
    /// What the user chose in the review: public for a scheduled upload
    /// (what it becomes at its publish time). The network may keep the
    /// video private anyway (`UploadStatus::Restricted`).
    pub visibility: Visibility,
    /// When the network makes the video public: it goes up private until
    /// then. `None` for an upload that goes live once processed.
    pub publish_at: Option<SystemTime>,
    /// The job that uploads it; retrying that job resumes the upload.
    pub job: JobId,
}

impl Upload {
    /// A reviewed upload waiting for its job.
    pub fn queued(visibility: Visibility, job: JobId) -> Self {
        Self {
            status: UploadStatus::Queued,
            visibility,
            publish_at: None,
            job,
        }
    }

    /// A reviewed upload that goes up private and turns public at
    /// `publish_at`, waiting for its job.
    pub fn scheduled(publish_at: SystemTime, job: JobId) -> Self {
        Self {
            publish_at: Some(publish_at),
            ..Self::queued(Visibility::Public, job)
        }
    }

    fn invalid(&self, action: &'static str) -> InvalidUploadTransition {
        InvalidUploadTransition {
            from: self.status.code(),
            action,
        }
    }

    /// The job starts (or resumes) sending the file. A failed upload starts
    /// again when its job is retried.
    pub fn start(&mut self) -> Result<(), InvalidUploadTransition> {
        match self.status {
            UploadStatus::Queued | UploadStatus::Uploading | UploadStatus::Failed(_) => {
                self.status = UploadStatus::Uploading;
                Ok(())
            }
            _ => Err(self.invalid("start")),
        }
    }

    /// The network has the whole file and is processing it. A retried job
    /// whose file was already sent comes back here from a failure.
    pub fn sent(&mut self) -> Result<(), InvalidUploadTransition> {
        match self.status {
            UploadStatus::Uploading | UploadStatus::Processing | UploadStatus::Failed(_) => {
                self.status = UploadStatus::Processing;
                Ok(())
            }
            _ => Err(self.invalid("finish sending")),
        }
    }

    /// The network processed the video and shows it as `visibility`, with
    /// the publish time it holds. A scheduled upload still private with a
    /// publish time is scheduled (at the network's time); one the network
    /// keeps private without it, like a private video where unlisted or
    /// public was asked, is restricted.
    pub fn processed(
        &mut self,
        visibility: Visibility,
        publish_at: Option<SystemTime>,
    ) -> Result<(), InvalidUploadTransition> {
        if self.status != UploadStatus::Processing {
            return Err(self.invalid("finish processing"));
        }
        self.status = match (self.publish_at, visibility, publish_at) {
            (Some(_), Visibility::Private, Some(at)) => {
                self.publish_at = Some(at);
                UploadStatus::Scheduled
            }
            (_, Visibility::Private, _) if self.visibility != Visibility::Private => {
                UploadStatus::Restricted
            }
            _ => UploadStatus::Published,
        };
        Ok(())
    }

    /// The network made the scheduled video public at its publish time.
    pub fn went_live(&mut self) -> Result<(), InvalidUploadTransition> {
        self.from_scheduled("go live")?;
        self.status = UploadStatus::Published;
        Ok(())
    }

    /// The publish time passed and the network kept the scheduled video
    /// private: the user's API project has not passed the network's audit.
    pub fn kept_private(&mut self) -> Result<(), InvalidUploadTransition> {
        self.from_scheduled("keep private")?;
        self.status = UploadStatus::Restricted;
        Ok(())
    }

    /// The scheduled video now goes live at `publish_at`.
    pub fn reschedule(&mut self, publish_at: SystemTime) -> Result<(), InvalidUploadTransition> {
        self.from_scheduled("reschedule")?;
        self.publish_at = Some(publish_at);
        Ok(())
    }

    /// The schedule is gone: the video stays private, with no publish time.
    pub fn unschedule(&mut self) -> Result<(), InvalidUploadTransition> {
        self.from_scheduled("cancel the schedule of")?;
        self.status = UploadStatus::Published;
        self.visibility = Visibility::Private;
        self.publish_at = None;
        Ok(())
    }

    fn from_scheduled(&self, action: &'static str) -> Result<(), InvalidUploadTransition> {
        match self.status {
            UploadStatus::Scheduled => Ok(()),
            _ => Err(self.invalid(action)),
        }
    }

    /// Another job checks on a video the network was still processing
    /// when the first job stopped waiting.
    pub fn check_again(&mut self, job: JobId) -> Result<(), InvalidUploadTransition> {
        match self.status {
            UploadStatus::Processing => {
                self.job = job;
                Ok(())
            }
            _ => Err(self.invalid("check again")),
        }
    }

    /// The upload failed for `failure`. A video already on the network
    /// (published, restricted or scheduled) cannot fail any more.
    pub fn fail(&mut self, failure: UploadFailure) -> Result<(), InvalidUploadTransition> {
        if self.status.is_on_network() {
            return Err(self.invalid("fail"));
        }
        self.status = UploadStatus::Failed(failure);
        Ok(())
    }
}

/// What an upload sends with the file, as the user reviewed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoUpload {
    pub title: String,
    /// The description with the account's footer, as the network takes it.
    pub description: String,
    pub tags: Vec<String>,
    pub visibility: Visibility,
    /// The video is made for kids (YouTube's `selfDeclaredMadeForKids`).
    pub made_for_kids: bool,
    /// The video holds realistic altered or synthetic content (YouTube's
    /// `containsSyntheticMedia`).
    pub synthetic: bool,
    /// When the network makes the video public: it goes up private until
    /// then (YouTube's `publishAt`). `visibility` is public.
    pub publish_at: Option<SystemTime>,
}

impl VideoUpload {
    /// Whether the publish time is no longer ahead of `now`: a new upload
    /// session would make the video public at once.
    pub fn is_late(&self, now: SystemTime) -> bool {
        self.publish_at.is_some_and(|at| at <= now)
    }
}

/// The video the network made of a finished upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedVideo {
    /// The network's id for the video (a YouTube video id).
    pub id: String,
}

/// How an upload call ended without an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadOutcome {
    /// The run asked to stop (cancel, app closing); the session and the
    /// confirmed bytes are saved, so the next call resumes.
    Stopped,
    Uploaded(UploadedVideo),
}

/// Where a sent video stands on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoState {
    Processing,
    /// Processed, showing as `visibility`.
    Ready {
        visibility: Visibility,
        /// When the network makes the private video public, if scheduled.
        publish_at: Option<SystemTime>,
        /// When it went public, as the network says (YouTube's
        /// `publishedAt`); only meaningful once it is not private.
        published_at: Option<SystemTime>,
    },
    /// Processing failed, with the network's reason.
    Failed(String),
    /// The network rejected the video, with its reason.
    Rejected(String),
    /// The video is gone (deleted, or not found any more).
    Removed,
}

/// Why an upload call gave no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UploadErrorKind {
    /// No token to send with: the account needs to reconnect, or is not
    /// connected. Comes from the run, not the network.
    SignedOut,
    /// The network refused the token (revoked).
    Refused,
    /// The token may not upload: a scope or an API not enabled.
    NotAllowed,
    /// The project's daily uploads bucket is used up.
    QuotaExceeded,
    /// The channel's own upload limit is reached.
    UploadLimit,
    /// Too many requests right now; the next attempt may pass.
    RateLimited,
    /// The network refused what was sent (a title it does not take).
    Invalid,
    /// The network failed on its side (500, 502, 503, 504).
    NetworkDown,
    /// No answer: offline, DNS, TLS, proxy or timeout.
    Unreachable,
    /// An answer the adapter does not understand.
    Unexpected,
    /// The file could not be read, or the run could not save its state.
    Local,
    /// The publish time passed before the upload could start: the network
    /// would publish it at once.
    Late,
    /// The video is no longer on the network.
    NotFound,
}

impl UploadErrorKind {
    /// Whether another attempt, later, may get through.
    pub fn is_transient(self) -> bool {
        matches!(
            self,
            UploadErrorKind::RateLimited
                | UploadErrorKind::NetworkDown
                | UploadErrorKind::Unreachable
        )
    }
}

/// A failed upload call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind:?}: {detail}")]
pub struct UploadError {
    pub kind: UploadErrorKind,
    /// The network's message or the transport error, in English, for the
    /// log. Never holds the session address or a token.
    pub detail: String,
}

impl UploadError {
    pub fn new(kind: UploadErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

/// A new publish time for a scheduled video, or none to cancel it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleChange {
    pub publish_at: Option<SystemTime>,
    /// What the upload declared, kept when the network does not say: a
    /// change replaces the network's whole status.
    pub made_for_kids: bool,
    pub synthetic: bool,
}

/// How a schedule change ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleOutcome {
    /// The network took the new publish time, or the cancel.
    Changed,
    /// Too late: the video is no longer private (it went live), so its
    /// publish time cannot change. Nothing was sent.
    Live {
        visibility: Visibility,
        published_at: Option<SystemTime>,
    },
}

/// What an upload needs from whoever runs it: a token, the file, and where
/// to keep the session and the confirmed bytes so a later run resumes.
pub trait UploadRun {
    /// A token fresh enough for the next request.
    fn access_token(&mut self) -> Result<SecretText, UploadError>;

    /// The file's size in bytes.
    fn size(&self) -> u64;

    /// `len` bytes of the file from `offset` (fewer only at its end).
    fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, UploadError>;

    /// The session an earlier run started, if any.
    fn session(&self) -> Option<String>;

    /// Keeps a new session before any byte goes to it.
    fn session_started(&mut self, session: &str) -> Result<(), UploadError>;

    /// Keeps how many bytes the network confirmed.
    fn confirmed(&mut self, bytes: u64) -> Result<(), UploadError>;

    /// True once the run should stop and leave the rest for later.
    fn should_stop(&self) -> bool;
}

/// Uploads videos to one network, resumably. Calls the network and blocks,
/// so the app runs it in a job.
pub trait VideoUploader: Send + Sync {
    fn network(&self) -> Network;

    /// Sends the file with `video`'s details, resuming the run's session
    /// when it has one and starting a new one when it has none or the
    /// network forgot it. Returns when the network has the whole file or
    /// the run asks to stop.
    fn upload(
        &self,
        video: &VideoUpload,
        run: &mut dyn UploadRun,
    ) -> Result<UploadOutcome, UploadError>;

    /// Where the uploaded video `id` stands.
    fn state(&self, access_token: &SecretText, id: &str) -> Result<VideoState, UploadError>;

    /// Changes the publish time of the scheduled video `id`, or cancels it
    /// (`change.publish_at` is `None`: the video stays private). Only while
    /// the video is private and was never published.
    fn reschedule(
        &self,
        access_token: &SecretText,
        id: &str,
        change: &ScheduleChange,
    ) -> Result<ScheduleOutcome, UploadError> {
        let _ = (access_token, id, change);
        Err(UploadError::new(
            UploadErrorKind::NotAllowed,
            "the network does not schedule videos",
        ))
    }

    /// How long to wait before processing check number `polls` (from 0).
    fn poll_delay(&self, polls: u32) -> Duration {
        // Fifteen seconds, doubling each time up to five minutes.
        Duration::from_secs(15u64.saturating_mul(1 << polls.min(5)).min(5 * 60))
    }
}

impl fmt::Display for UploadFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upload(visibility: Visibility) -> Upload {
        Upload::queued(visibility, JobId::new())
    }

    #[test]
    fn an_upload_goes_from_queued_to_published() {
        let mut u = upload(Visibility::Public);
        u.start().unwrap();
        assert_eq!(u.status, UploadStatus::Uploading);
        u.start().unwrap();
        assert_eq!(
            u.status,
            UploadStatus::Uploading,
            "a resumed job starts again"
        );
        u.sent().unwrap();
        assert_eq!(u.status, UploadStatus::Processing);
        u.processed(Visibility::Public, None).unwrap();
        assert_eq!(u.status, UploadStatus::Published);
        assert!(u.status.is_final());
    }

    #[test]
    fn a_video_kept_private_when_more_was_asked_is_restricted() {
        for asked in [Visibility::Public, Visibility::Unlisted] {
            let mut u = upload(asked);
            u.start().unwrap();
            u.sent().unwrap();
            u.processed(Visibility::Private, None).unwrap();
            assert_eq!(u.status, UploadStatus::Restricted, "{asked:?}");
            assert!(u.status.is_final());
        }
        let mut private = upload(Visibility::Private);
        private.start().unwrap();
        private.sent().unwrap();
        private.processed(Visibility::Private, None).unwrap();
        assert_eq!(private.status, UploadStatus::Published, "private was asked");
    }

    #[test]
    fn a_failed_upload_starts_again_when_retried() {
        let mut u = upload(Visibility::Public);
        u.start().unwrap();
        u.fail(UploadFailure::QuotaExceeded).unwrap();
        assert_eq!(u.status.failure(), Some(&UploadFailure::QuotaExceeded));
        u.start().unwrap();
        assert_eq!(u.status, UploadStatus::Uploading);
    }

    #[test]
    fn processing_can_fail_but_a_published_video_cannot() {
        let mut u = upload(Visibility::Unlisted);
        u.start().unwrap();
        u.sent().unwrap();
        u.fail(UploadFailure::Rejected("duplicate".into())).unwrap();
        assert!(matches!(u.status, UploadStatus::Failed(_)));

        let mut done = upload(Visibility::Public);
        done.start().unwrap();
        done.sent().unwrap();
        done.processed(Visibility::Public, None).unwrap();
        assert!(done.fail(UploadFailure::Removed).is_err());
        assert!(done.start().is_err(), "nothing left to send");
        assert_eq!(done.status, UploadStatus::Published);
    }

    #[test]
    fn steps_out_of_order_are_refused() {
        let mut u = upload(Visibility::Public);
        let error = u.sent().unwrap_err();
        assert_eq!(error.from, "queued");
        assert!(u.processed(Visibility::Public, None).is_err());
        u.start().unwrap();
        assert!(
            u.processed(Visibility::Public, None).is_err(),
            "not sent yet"
        );
        u.sent().unwrap();
        u.sent().unwrap();
        assert_eq!(
            u.status,
            UploadStatus::Processing,
            "a resumed job waits again"
        );
        assert!(u.start().is_err(), "already sent");
    }

    #[test]
    fn a_retried_job_whose_file_was_sent_waits_for_processing_again() {
        let mut u = upload(Visibility::Public);
        u.start().unwrap();
        u.sent().unwrap();
        u.fail(UploadFailure::Job(JobFailureKind::KeyRejected))
            .unwrap();
        u.sent().unwrap();
        assert_eq!(u.status, UploadStatus::Processing);
        u.processed(Visibility::Public, None).unwrap();
        assert!(u.sent().is_err(), "published");
    }

    #[test]
    fn only_a_video_being_processed_is_checked_again() {
        let first = JobId::new();
        let mut u = Upload::queued(Visibility::Public, first);
        let next = JobId::new();
        assert!(u.check_again(next).is_err(), "queued");
        u.start().unwrap();
        assert!(u.check_again(next).is_err(), "uploading");
        u.sent().unwrap();
        u.check_again(next).unwrap();
        assert_eq!(u.job, next);
        assert_eq!(u.status, UploadStatus::Processing);
        u.processed(Visibility::Public, None).unwrap();
        assert!(u.check_again(first).is_err(), "published");
        assert_eq!(u.job, next);
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn scheduled(publish_at: SystemTime) -> Upload {
        let mut u = Upload::scheduled(publish_at, JobId::new());
        u.start().unwrap();
        u.sent().unwrap();
        u
    }

    #[test]
    fn a_scheduled_upload_asks_for_public_at_its_time() {
        let u = Upload::scheduled(at(100), JobId::new());
        assert_eq!(u.visibility, Visibility::Public);
        assert_eq!(u.publish_at, Some(at(100)));
        assert_eq!(u.status, UploadStatus::Queued);
        assert_eq!(upload(Visibility::Public).publish_at, None);
    }

    #[test]
    fn a_processed_private_video_with_its_time_is_scheduled_at_the_networks_time() {
        let mut u = scheduled(at(100));
        u.processed(Visibility::Private, Some(at(160))).unwrap();
        assert_eq!(u.status, UploadStatus::Scheduled);
        assert_eq!(u.publish_at, Some(at(160)), "the network's time");
        assert!(!u.status.is_final(), "it still goes live");
        assert!(u.status.is_on_network());
        assert!(u.start().is_err(), "nothing left to send");
        assert!(
            u.fail(UploadFailure::Removed).is_err(),
            "on the network already"
        );
    }

    #[test]
    fn a_scheduled_upload_the_network_kept_private_without_a_time_is_restricted() {
        let mut u = scheduled(at(100));
        u.processed(Visibility::Private, None).unwrap();
        assert_eq!(u.status, UploadStatus::Restricted);
    }

    #[test]
    fn a_scheduled_upload_processed_after_its_time_may_be_public_already() {
        let mut u = scheduled(at(100));
        u.processed(Visibility::Public, None).unwrap();
        assert_eq!(u.status, UploadStatus::Published);
    }

    #[test]
    fn a_schedule_goes_live_or_is_kept_private() {
        let mut live = scheduled(at(100));
        live.processed(Visibility::Private, Some(at(100))).unwrap();
        live.went_live().unwrap();
        assert_eq!(live.status, UploadStatus::Published);
        assert!(live.went_live().is_err(), "once");

        let mut kept = scheduled(at(100));
        kept.processed(Visibility::Private, Some(at(100))).unwrap();
        kept.kept_private().unwrap();
        assert_eq!(kept.status, UploadStatus::Restricted);
        assert!(kept.reschedule(at(200)).is_err(), "no schedule any more");
    }

    #[test]
    fn a_schedule_changes_or_is_cancelled_only_while_scheduled() {
        let mut u = scheduled(at(100));
        assert!(u.reschedule(at(200)).is_err(), "still processing");
        u.processed(Visibility::Private, Some(at(100))).unwrap();
        u.reschedule(at(200)).unwrap();
        assert_eq!(u.publish_at, Some(at(200)));
        assert_eq!(u.status, UploadStatus::Scheduled);

        u.unschedule().unwrap();
        assert_eq!(u.status, UploadStatus::Published);
        assert_eq!(u.visibility, Visibility::Private, "a private video");
        assert_eq!(u.publish_at, None);
        assert!(u.unschedule().is_err());
        assert!(u.reschedule(at(300)).is_err());
    }

    #[test]
    fn a_video_is_late_once_its_publish_time_is_not_ahead() {
        let mut video = VideoUpload {
            title: "t".into(),
            description: String::new(),
            tags: Vec::new(),
            visibility: Visibility::Public,
            made_for_kids: false,
            synthetic: false,
            publish_at: None,
        };
        assert!(!video.is_late(at(100)), "not scheduled");
        video.publish_at = Some(at(100));
        assert!(video.is_late(at(100)));
        assert!(!video.is_late(at(99)));
    }

    #[test]
    fn statuses_and_failures_round_trip_through_their_codes() {
        let statuses = [
            UploadStatus::Queued,
            UploadStatus::Uploading,
            UploadStatus::Processing,
            UploadStatus::Scheduled,
            UploadStatus::Published,
            UploadStatus::Restricted,
            UploadStatus::Failed(UploadFailure::QuotaExceeded),
            UploadStatus::Failed(UploadFailure::ScheduleMissed),
            UploadStatus::Failed(UploadFailure::UploadLimit),
            UploadStatus::Failed(UploadFailure::ReconnectNeeded),
            UploadStatus::Failed(UploadFailure::Rejected("copyright".into())),
            UploadStatus::Failed(UploadFailure::ProcessingFailed("codec".into())),
            UploadStatus::Failed(UploadFailure::Removed),
            UploadStatus::Failed(UploadFailure::RenderChanged),
            UploadStatus::Failed(UploadFailure::Job(JobFailureKind::ProviderUnavailable)),
        ];
        for status in statuses {
            let failure = status.failure().map(UploadFailure::code);
            assert_eq!(
                UploadStatus::from_code(status.code(), failure.as_deref()),
                Some(status.clone())
            );
        }
        assert_eq!(UploadStatus::from_code("lost", None), None);
        assert_eq!(
            UploadFailure::from_code("something:new"),
            UploadFailure::Job(JobFailureKind::Unexpected)
        );
    }

    #[test]
    fn only_network_and_connection_hiccups_are_transient() {
        let transient: Vec<UploadErrorKind> = [
            UploadErrorKind::SignedOut,
            UploadErrorKind::Refused,
            UploadErrorKind::NotAllowed,
            UploadErrorKind::QuotaExceeded,
            UploadErrorKind::UploadLimit,
            UploadErrorKind::RateLimited,
            UploadErrorKind::Invalid,
            UploadErrorKind::NetworkDown,
            UploadErrorKind::Unreachable,
            UploadErrorKind::Unexpected,
            UploadErrorKind::Local,
            UploadErrorKind::Late,
            UploadErrorKind::NotFound,
        ]
        .into_iter()
        .filter(|kind| kind.is_transient())
        .collect();
        assert_eq!(
            transient,
            [
                UploadErrorKind::RateLimited,
                UploadErrorKind::NetworkDown,
                UploadErrorKind::Unreachable
            ],
            "a spent quota is not retried in a loop"
        );
    }
}
