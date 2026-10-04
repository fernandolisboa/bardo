//! Upload with review (PRD stories 84, 85 and 88; ADR-0008). At the
//! Publish stage the user opens the upload review of a connected network
//! account: the rendered file of its preset, the post's title, description
//! and tags, the connected channel it goes to, the visibility, whether it
//! is made for kids, and the synthetic-content disclosure (on when the
//! narration was read with a realistic voice, and editable).
//!
//! An upload cannot be taken back, so nothing starts from the review
//! itself: `Bardo::start_upload` takes the review the user saw and their
//! choices, reads everything again, and refuses when the render, the cut
//! or the post changed since (the user reviews again). It then keeps the
//! upload as the project's publication on that network (replacing a linked
//! post only once the user confirmed it) and queues the upload job.
//!
//! The job sends the file resumably: the network's session is the job's
//! external handle and the confirmed bytes its checkpoint, so a cancelled,
//! failed or interrupted upload resumes without sending those bytes again.
//! Then it waits for the network to process the video and records how it
//! ended: published, scheduled (private until its publish time),
//! restricted (kept private by the network although more was asked) or
//! failed with the network's reason.
//!
//! A scheduled upload (#78) is reviewed and confirmed the same way, with a
//! publish time in the user's time zone that must still be ahead when they
//! confirm. It goes up private with that time, and the network makes it
//! public by itself, with Bardo and the PC off (ADR-0006).
//!
//! A Reel (#81) is reviewed with its caption, cover frame, "also show in
//! Feed" and AI label, and its rendered file must pass Instagram's Reel
//! specs. Instagram only makes the post when asked: once the container is
//! processed, the job publishes it, within the account's publishing limit.
//! Over the limit the job waits queued, holding no place in the queue,
//! until the oldest post Bardo published in the window leaves it. A
//! container Instagram dropped (24 hours unpublished) goes again.
//!
//! A Reel scheduled in its review (#82) has a due time instead: Instagram
//! takes no publish time, so the job publishes it at that time with Bardo
//! open (`crate::scheduler`, `bardo_domain::due_step`). It sends and
//! processes the file ahead, no more than Instagram keeps a container,
//! claims the publication once due and publishes it; a post whose due time
//! passed without it waits for the user.

use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    DueStep, Job, JobFailure, JobFailureKind, JobId, JobKind, JobState, LIMIT_RECHECK,
    MetadataProblem, Network, NetworkAccount, NetworkAccountId, NetworkAccountRepository, Post,
    Progress, ProjectFiles, Publication, PublicationId, PublicationKind, PublicationRepository,
    ReelFile, ReelSpecProblem, Render, RenderRepository, RepositoryError, ScheduleProblem,
    SecretText, SignInFailureKind, Upload, UploadError, UploadErrorKind, UploadFailure,
    UploadOutcome, UploadRun, UploadStatus, VideoProjectId, VideoState, VideoUpload, VideoUploader,
    Visibility, check_publish_time, check_reel, due_step, mp4_layout, prepare_at,
};
use serde::{Deserialize, Serialize};

use crate::connections::Connections;
use crate::export::{ExportError, ExportTarget};
use crate::jobs::{JobActionError, JobContext, JobHandler};
use crate::scenes::{id, parse, to_json, unexpected};
use crate::{Bardo, ConnectionError, ConnectionState, Text};

/// Why a network account cannot upload now, first thing first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UploadBlock {
    /// Bardo does not upload to this network: export it instead.
    NotOffered,
    /// The account is not signed in to the network.
    NotConnected,
    /// The network refused the account's tokens: reconnect it.
    ReconnectNeeded,
    /// An upload of this network is running or waiting.
    Uploading,
    /// A render of the project is running or waiting: its files may be
    /// rewritten while they are sent.
    Rendering,
    /// Nothing rendered for the account yet.
    NoRender,
    /// The cut or the preset changed since the render: render again.
    RenderOutdated,
    /// The rendered file falls short of the network's specs (a Reel's).
    Specs,
    /// No metadata generated yet.
    NoMetadata,
    /// The metadata breaks the network's rules.
    Problems,
}

impl UploadBlock {
    pub const ALL: [UploadBlock; 10] = [
        UploadBlock::NotOffered,
        UploadBlock::NotConnected,
        UploadBlock::ReconnectNeeded,
        UploadBlock::Uploading,
        UploadBlock::Rendering,
        UploadBlock::NoRender,
        UploadBlock::RenderOutdated,
        UploadBlock::Specs,
        UploadBlock::NoMetadata,
        UploadBlock::Problems,
    ];

    pub fn code(self) -> &'static str {
        match self {
            UploadBlock::NotOffered => "not_offered",
            UploadBlock::NotConnected => "not_connected",
            UploadBlock::ReconnectNeeded => "reconnect_needed",
            UploadBlock::Uploading => "uploading",
            UploadBlock::Rendering => "rendering",
            UploadBlock::NoRender => "no_render",
            UploadBlock::RenderOutdated => "render_outdated",
            UploadBlock::Specs => "specs",
            UploadBlock::NoMetadata => "no_metadata",
            UploadBlock::Problems => "problems",
        }
    }
}

/// What the upload review shows for one network account.
#[derive(Debug, Clone, PartialEq)]
pub struct UploadReview {
    pub project: VideoProjectId,
    pub account: NetworkAccountId,
    pub network: Network,
    pub handle: String,
    pub connection: ConnectionState,
    /// The file that goes: the account's last render.
    pub render: Option<bardo_domain::Render>,
    /// Whether `render` was made from the cut and preset as they are now.
    pub render_current: bool,
    /// The title, description (with the account's footer) and tags as the
    /// network takes them.
    pub post: Option<Post>,
    pub problems: Vec<MetadataProblem>,
    /// What keeps the rendered file from being a Reel, in the order the
    /// review lists them; empty on networks without such specs.
    pub spec_problems: Vec<ReelSpecProblem>,
    /// The account's default visibility, picked until the user changes it.
    pub visibility: Visibility,
    /// The narration was read with a realistic voice: the disclosure
    /// starts on.
    pub synthetic: bool,
    /// The network's publication now, which the upload replaces.
    pub replaces: Option<Publication>,
    rendering: bool,
    uploading: bool,
    /// The rendered file's size now; `None` when it is gone.
    size: Option<u64>,
    /// The connected account's id on the network (Instagram's IG user id),
    /// which the upload goes to.
    destination: String,
    /// What was reviewed (render, cut, post, publication replaced), to
    /// tell when it changed.
    stamp: String,
}

impl UploadReview {
    /// The connected channel the upload goes to.
    pub fn channel(&self) -> Option<&str> {
        match &self.connection {
            ConnectionState::Connected { channel }
            | ConnectionState::ReconnectNeeded { channel } => Some(channel),
            _ => None,
        }
    }

    /// What keeps the account from uploading, first thing first.
    pub fn block(&self) -> Option<UploadBlock> {
        Some(match &self.connection {
            ConnectionState::Unavailable => UploadBlock::NotOffered,
            ConnectionState::NotConnected
            | ConnectionState::Connecting
            | ConnectionState::Choosing { .. } => UploadBlock::NotConnected,
            ConnectionState::ReconnectNeeded { .. } => UploadBlock::ReconnectNeeded,
            ConnectionState::Connected { .. } if self.uploading => UploadBlock::Uploading,
            ConnectionState::Connected { .. } if self.rendering => UploadBlock::Rendering,
            ConnectionState::Connected { .. }
                if self.render.is_none() || self.size.is_none_or(|size| size == 0) =>
            {
                UploadBlock::NoRender
            }
            ConnectionState::Connected { .. } if !self.render_current => {
                UploadBlock::RenderOutdated
            }
            ConnectionState::Connected { .. } if !self.spec_problems.is_empty() => {
                UploadBlock::Specs
            }
            ConnectionState::Connected { .. } if self.post.is_none() => UploadBlock::NoMetadata,
            ConnectionState::Connected { .. } if !self.problems.is_empty() => UploadBlock::Problems,
            ConnectionState::Connected { .. } => return None,
        })
    }

    pub fn can_upload(&self) -> bool {
        self.block().is_none()
    }

    /// The choices the review starts with.
    pub fn choices(&self) -> UploadChoices {
        UploadChoices {
            visibility: self.visibility,
            made_for_kids: false,
            synthetic: self.synthetic,
            replace: false,
            publish_at: None,
            share_to_feed: true,
            cover: Duration::ZERO,
        }
    }

    /// Where the post goes: the network's own post page (a Reel shows its
    /// caption, cover and feed choice) or a video with a title.
    pub fn is_reel(&self) -> bool {
        self.network.uploads_reels()
    }
}

/// What the user picked in the upload review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadChoices {
    pub visibility: Visibility,
    pub made_for_kids: bool,
    /// The synthetic-content disclosure.
    pub synthetic: bool,
    /// The user confirmed the upload replaces the network's publication.
    pub replace: bool,
    /// When the network makes the video public: it goes up private until
    /// then. A scheduled upload is public at that time, whatever
    /// `visibility` says.
    pub publish_at: Option<SystemTime>,
    /// A Reel also shows in the feed.
    pub share_to_feed: bool,
    /// The frame of a Reel its cover shows, from the start.
    pub cover: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum UploadReviewError {
    #[error("video project not found")]
    ProjectNotFound,
    /// The channel has no account on the network.
    #[error("the channel has no account on this network")]
    NoAccount,
    #[error("the upload is blocked: {0:?}")]
    Blocked(UploadBlock),
    /// The render, the cut or the post changed since the review: review it
    /// again.
    #[error("the upload changed since the review")]
    Changed,
    /// The network has a publication and the user did not confirm the
    /// upload replaces it.
    #[error("the upload would replace the network's publication")]
    ReplaceNotConfirmed,
    /// The upload is not stopped or failed (resume), or not waiting on the
    /// network (check again).
    #[error("the upload cannot do that now")]
    NotNow,
    /// The publish time cannot be used (already past).
    #[error("the publish time cannot be used: {0}")]
    Schedule(ScheduleProblem),
    /// Neither the network nor Bardo schedules its posts.
    #[error("the network takes no publish time")]
    NoSchedule,
    /// The cover frame is past the video's end.
    #[error("the cover is past the end of the video")]
    CoverPastEnd,
    #[error(transparent)]
    Job(#[from] JobActionError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl UploadReviewError {
    /// What the Publish stage says.
    pub fn message(&self) -> Text {
        match self {
            UploadReviewError::ProjectNotFound => Text::ProjectNotFound,
            UploadReviewError::NoAccount => Text::PublicationNoAccount,
            UploadReviewError::Blocked(block) => Text::UploadBlocked(*block),
            UploadReviewError::Changed => Text::UploadChanged,
            UploadReviewError::ReplaceNotConfirmed => Text::UploadReplaceNotConfirmed,
            UploadReviewError::Schedule(problem) => Text::ScheduleProblem(*problem),
            UploadReviewError::CoverPastEnd => Text::UploadCoverPastEnd,
            UploadReviewError::NoSchedule
            | UploadReviewError::NotNow
            | UploadReviewError::Job(_)
            | UploadReviewError::Repository(_) => Text::UploadNotStarted,
        }
    }
}

impl From<ExportError> for UploadReviewError {
    fn from(error: ExportError) -> Self {
        match error {
            ExportError::Repository(error) => UploadReviewError::Repository(error),
            ExportError::NoAccounts => UploadReviewError::NoAccount,
            _ => UploadReviewError::ProjectNotFound,
        }
    }
}

impl From<ConnectionError> for UploadReviewError {
    fn from(error: ConnectionError) -> Self {
        match error {
            ConnectionError::Repository(error) => UploadReviewError::Repository(error),
            other => UploadReviewError::Repository(RepositoryError(Box::new(other))),
        }
    }
}

/// Where an uploaded publication stands, with its job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadState {
    /// Reviewed; the job has not started yet.
    Waiting,
    /// Sending the file.
    Uploading(Progress),
    /// A try failed on the way (the network or the connection); the queue
    /// tries again shortly.
    Retrying,
    /// The network has the file and is processing it.
    Processing,
    /// On the network, private until this time, when the network makes it
    /// public.
    Scheduled(SystemTime),
    /// Scheduled in Bardo: it publishes the post at this time (its due
    /// time), with the app open. The upload waits until it may start, or
    /// waits for that time once processed.
    Due(SystemTime),
    /// Its due time passed without it (Bardo was closed): the user sends it
    /// now, gives it a new time or cancels it.
    Missed(SystemTime),
    /// The network was still processing the video when Bardo stopped
    /// waiting for it: check again later.
    StillProcessing,
    /// Over the network's publishing limit (Instagram's): queued until
    /// `until`, when Bardo reads the limit again; `frees` when a place
    /// frees by then, so the post goes. `used` of `total` posts in the
    /// window, when the network said.
    OverLimit {
        until: SystemTime,
        frees: bool,
        used: Option<u32>,
        total: Option<u32>,
    },
    Published,
    /// Kept private by the network although more was asked.
    Restricted,
    /// The user stopped it; it resumes from what the network has.
    Stopped,
    Failed {
        failure: UploadFailure,
        /// Whether retrying the job may get it through (a video the network
        /// rejected needs a new upload).
        retryable: bool,
    },
}

impl UploadState {
    /// Whether the upload's job is running or waiting.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            UploadState::Waiting
                | UploadState::Uploading(_)
                | UploadState::Retrying
                | UploadState::Processing
                | UploadState::OverLimit { .. }
                | UploadState::Due(_)
        )
    }
}

/// Where `upload` stands given its job (`None` when the job is gone).
pub fn upload_state(upload: &Upload, job: Option<&Job>) -> UploadState {
    let status = &upload.status;
    match status {
        UploadStatus::Published => return UploadState::Published,
        UploadStatus::Restricted => return UploadState::Restricted,
        UploadStatus::Scheduled => {
            if let Some(at) = upload.publish_at {
                return UploadState::Scheduled(at);
            }
        }
        _ => {}
    }
    let Some(job) = job else {
        return match status {
            UploadStatus::Failed(failure) => UploadState::Failed {
                failure: failure.clone(),
                retryable: false,
            },
            _ => UploadState::Failed {
                failure: UploadFailure::Job(JobFailureKind::Unexpected),
                retryable: false,
            },
        };
    };
    let held = held(job);
    match job.state() {
        JobState::Running if *status == UploadStatus::Processing => UploadState::Processing,
        JobState::Running => UploadState::Uploading(job.progress()),
        JobState::Queued if job.failure().is_some() => UploadState::Retrying,
        JobState::Queued if held.is_some() => {
            let held = held.expect("checked");
            UploadState::OverLimit {
                until: from_millis(held.until),
                frees: held.frees,
                used: held.used,
                total: held.total,
            }
        }
        JobState::Queued if *status == UploadStatus::Processing => UploadState::Processing,
        JobState::Queued => UploadState::Waiting,
        JobState::Cancelled => match status {
            UploadStatus::Failed(failure) => UploadState::Failed {
                failure: failure.clone(),
                retryable: job.can_retry(),
            },
            _ => UploadState::Stopped,
        },
        JobState::Failed => UploadState::Failed {
            failure: match status {
                UploadStatus::Failed(failure) => failure.clone(),
                _ => UploadFailure::Job(
                    job.failure()
                        .map_or(JobFailureKind::Unexpected, |failure| failure.kind),
                ),
            },
            retryable: job.can_retry(),
        },
        JobState::Done => match status {
            UploadStatus::Failed(failure) => UploadState::Failed {
                failure: failure.clone(),
                retryable: false,
            },
            UploadStatus::Processing => UploadState::StillProcessing,
            _ => UploadState::Failed {
                failure: UploadFailure::Job(JobFailureKind::Unexpected),
                retryable: false,
            },
        },
    }
}

/// Why `job` waits queued for the publishing limit, if it does: deferred,
/// so queued with a time and no failure, and the hold in its checkpoint.
fn held(job: &Job) -> Option<Held> {
    job.checkpoint()
        .and_then(|text| serde_json::from_str::<UploadCheckpoint>(text).ok())
        .and_then(|checkpoint| checkpoint.held)
        .filter(|_| job.state() == JobState::Queued && job.run_at().is_some())
}

/// Where a publication Bardo publishes at its due time stands, when that
/// time decides it: missed, or waiting for it (to start the upload, or to
/// publish once processed). `None` otherwise.
fn due_state(publication: &Publication, job: Option<&Job>) -> Option<UploadState> {
    let due = publication.due()?;
    if publication.is_missed() {
        return Some(UploadState::Missed(due));
    }
    let job = job?;
    let waiting = job.state() == JobState::Queued
        && job.failure().is_none()
        && job.run_at().is_some()
        && held(job).is_none();
    waiting.then_some(UploadState::Due(due))
}

/// The upload job's payload: the publication it sends, the file and what
/// goes with it, fixed when the user confirmed the review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct UploadPayload {
    project: String,
    publication: String,
    account: String,
    network: String,
    /// The render reviewed, and its file: a run refuses to send another.
    render: String,
    file: String,
    size: u64,
    title: String,
    description: String,
    tags: Vec<String>,
    visibility: String,
    made_for_kids: bool,
    synthetic: bool,
    /// The network's video, for a job that only checks on its processing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    video: Option<String>,
    /// When the network makes the video public, in Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    publish_at: Option<u64>,
    /// A Reel also shows in the feed.
    #[serde(default = "shown_in_feed")]
    share_to_feed: bool,
    /// A Reel's cover frame, in milliseconds from the start.
    #[serde(default)]
    cover_ms: u64,
    /// The connected account's id on the network when the user confirmed:
    /// a job refuses to send to another one. Empty in jobs from before.
    #[serde(default)]
    destination: String,
}

fn shown_in_feed() -> bool {
    true
}

impl UploadPayload {
    fn video(&self) -> Result<VideoUpload, JobFailure> {
        Ok(VideoUpload {
            title: self.title.clone(),
            description: self.description.clone(),
            tags: self.tags.clone(),
            visibility: self.visibility.parse().map_err(unexpected)?,
            made_for_kids: self.made_for_kids,
            synthetic: self.synthetic,
            publish_at: self.publish_at.map(from_millis),
            share_to_feed: self.share_to_feed,
            cover: Duration::from_millis(self.cover_ms),
        })
    }
}

fn from_millis(millis: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
}

fn to_millis(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// How far an upload job got: the bytes the network confirmed, and the
/// video once the network has the whole file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct UploadCheckpoint {
    confirmed: u64,
    video: Option<String>,
    /// Waiting for the publishing limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    held: Option<Held>,
    /// The video the network dropped (Instagram expires an unpublished
    /// container): the file goes again in a new one, whatever video the
    /// payload or the job's session names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dropped: Option<String>,
    /// How many times the file went again so far.
    #[serde(default, skip_serializing_if = "is_zero")]
    restarts: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// How many times an upload goes again in a new container before it fails:
/// a network that keeps dropping it won't take it on the next try either.
const MAX_RESTARTS: u32 = 2;

/// Why a job waits queued: the publishing limit, read again `until` (Unix
/// milliseconds), with what the network said of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Held {
    until: u64,
    /// A place frees by `until` (Bardo's own oldest post leaves the
    /// window), so the post goes then; otherwise Bardo reads again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    frees: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    total: Option<u32>,
}

/// The part of an upload job's progress the file takes; processing takes
/// the rest.
fn sending_progress(confirmed: u64, size: u64) -> Progress {
    Progress::of(confirmed.saturating_mul(9), size.saturating_mul(10).max(1))
}

/// A job failure and, when it ends the upload, why for the publication.
/// Transient ones leave the publication as it is: the queue tries again.
fn failure_of(error: &UploadError, network: Network) -> (JobFailure, Option<UploadFailure>) {
    let (kind, failure) = match error.kind {
        UploadErrorKind::SignedOut | UploadErrorKind::Refused => (
            JobFailureKind::KeyRejected,
            Some(UploadFailure::ReconnectNeeded),
        ),
        UploadErrorKind::QuotaExceeded => (
            JobFailureKind::LimitReached,
            Some(UploadFailure::QuotaExceeded),
        ),
        UploadErrorKind::UploadLimit => (
            JobFailureKind::LimitReached,
            Some(UploadFailure::UploadLimit),
        ),
        UploadErrorKind::NotAllowed => (
            JobFailureKind::NotAllowed,
            Some(UploadFailure::Job(JobFailureKind::NotAllowed)),
        ),
        UploadErrorKind::RateLimited
        | UploadErrorKind::NetworkDown
        | UploadErrorKind::Unreachable => (JobFailureKind::ProviderUnavailable, None),
        // Not ready and expired are handled where they come (publishing);
        // anywhere else they are answers Bardo did not expect.
        UploadErrorKind::Invalid
        | UploadErrorKind::Unexpected
        | UploadErrorKind::NotReady
        | UploadErrorKind::Expired => (
            JobFailureKind::UnexpectedAnswer,
            Some(UploadFailure::Job(JobFailureKind::UnexpectedAnswer)),
        ),
        UploadErrorKind::Local => (
            JobFailureKind::Media,
            Some(UploadFailure::Job(JobFailureKind::Media)),
        ),
        // YouTube may not schedule a time already past: the user reviews
        // the upload again with a new time.
        UploadErrorKind::Late | UploadErrorKind::ScheduleRefused => (
            JobFailureKind::NotAllowed,
            Some(UploadFailure::ScheduleMissed),
        ),
        UploadErrorKind::NotFound => (
            JobFailureKind::UnexpectedAnswer,
            Some(UploadFailure::Removed),
        ),
    };
    let detail = format!("{}: {}", network.brand(), error.detail);
    (JobFailure::new(kind, detail), failure)
}

/// A token for the account, as an upload wants it.
pub(crate) fn access_token(
    connections: &Connections,
    account: &NetworkAccount,
) -> Result<SecretText, UploadError> {
    match connections.access_token(account) {
        Ok(tokens) => Ok(SecretText::new(tokens.access_token())),
        Err(ConnectionError::SignIn(_, failure))
            if matches!(
                failure.kind,
                SignInFailureKind::NetworkDown
                    | SignInFailureKind::Unreachable
                    | SignInFailureKind::LimitReached
            ) =>
        {
            Err(UploadError::new(
                UploadErrorKind::Unreachable,
                format!("could not refresh the sign-in: {}", failure.detail),
            ))
        }
        Err(ConnectionError::Store(error)) => Err(UploadError::new(
            UploadErrorKind::Local,
            format!("could not read the sign-in: {error}"),
        )),
        Err(error) => Err(UploadError::new(
            UploadErrorKind::SignedOut,
            error.to_string(),
        )),
    }
}

/// Runs uploads.
pub(crate) struct UploadHandler {
    pub(crate) publications: Arc<dyn PublicationRepository>,
    pub(crate) accounts: Arc<dyn NetworkAccountRepository>,
    pub(crate) renders: Arc<dyn RenderRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) connections: Connections,
    pub(crate) uploaders: Vec<Arc<dyn VideoUploader>>,
    /// When this session of Bardo opened: a due time that passed before
    /// it passed while Bardo was closed (`bardo_domain::due_step`).
    pub(crate) opened_at: SystemTime,
}

/// Why a job of a scheduled upload stops when its due time passed without
/// it: not retried, so it waits for the user.
fn missed() -> JobFailure {
    JobFailure::new(
        JobFailureKind::Missed,
        "the scheduled upload's due time passed without it",
    )
}

/// An upload job as the adapter sees it: tokens from the connection, the
/// file from the project folder, the session and confirmed bytes saved
/// with the job.
struct JobRun<'a> {
    cx: &'a mut JobContext,
    handler: &'a UploadHandler,
    account: &'a NetworkAccount,
    project: VideoProjectId,
    payload: &'a UploadPayload,
    checkpoint: UploadCheckpoint,
    /// The open file and where it is.
    reader: Option<(Box<dyn Read + Send>, u64)>,
}

impl JobRun<'_> {
    fn local(error: impl std::fmt::Display) -> UploadError {
        UploadError::new(UploadErrorKind::Local, error.to_string())
    }
}

impl UploadRun for JobRun<'_> {
    fn access_token(&mut self) -> Result<SecretText, UploadError> {
        access_token(&self.handler.connections, self.account)
    }

    fn size(&self) -> u64 {
        self.payload.size
    }

    fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, UploadError> {
        if self.reader.as_ref().is_none_or(|(_, at)| *at != offset) {
            let mut file = self
                .handler
                .files
                .open(self.project, &self.payload.file)
                .map_err(Self::local)?;
            let skipped = std::io::copy(&mut (&mut file).take(offset), &mut std::io::sink())
                .map_err(Self::local)?;
            if skipped != offset {
                return Err(Self::local("the file is shorter than its upload"));
            }
            self.reader = Some((file, offset));
        }
        let (file, at) = self.reader.as_mut().expect("opened above");
        let mut bytes = Vec::with_capacity(len);
        file.take(len as u64)
            .read_to_end(&mut bytes)
            .map_err(Self::local)?;
        *at += bytes.len() as u64;
        Ok(bytes)
    }

    fn session(&self) -> Option<String> {
        live_session(self.cx, &self.checkpoint)
    }

    fn session_started(&mut self, session: &str) -> Result<(), UploadError> {
        self.cx.save_external_handle(session).map_err(Self::local)
    }

    fn confirmed(&mut self, bytes: u64) -> Result<(), UploadError> {
        self.checkpoint.confirmed = bytes;
        self.cx
            .save_checkpoint(
                to_json(&self.checkpoint),
                sending_progress(bytes, self.payload.size),
            )
            .map_err(Self::local)
    }

    fn should_stop(&self) -> bool {
        self.cx.should_stop()
    }

    /// The account's id on the network the user reviewed the upload for,
    /// not Bardo's account id.
    fn account(&self) -> &str {
        self.payload.destination.as_str()
    }
}

/// The upload session the job started at the network, unless it is the
/// one the network dropped.
fn live_session(cx: &JobContext, checkpoint: &UploadCheckpoint) -> Option<String> {
    cx.external_handle()
        .filter(|handle| checkpoint.dropped.as_deref() != Some(*handle))
        .map(str::to_owned)
}

/// How the publish step of a processed upload ended, short of a failure.
enum Publishing {
    Done,
    /// The network is not done with it after all: check again.
    NotReady,
    /// Over the publishing limit.
    Held(Held),
    /// The network dropped it: send it again.
    Expired,
}

impl UploadHandler {
    /// Applies `step` to the stored publication and saves its upload,
    /// unless it was replaced since (also while this saves).
    fn update(
        &self,
        id: PublicationId,
        job: JobId,
        step: impl FnOnce(&mut Publication) -> Result<(), bardo_domain::InvalidUploadTransition>,
    ) -> Result<(), JobFailure> {
        let Some(mut publication) = self.publications.publication(id).map_err(unexpected)? else {
            return Ok(());
        };
        if publication.upload().is_none_or(|upload| upload.job != job) {
            return Ok(());
        }
        step(&mut publication).map_err(unexpected)?;
        self.publications
            .save_upload(&publication)
            .map(drop)
            .map_err(unexpected)
    }

    /// Claims the scheduled upload for this job's run (`bardo_domain::due`):
    /// false when another run has it, or it is no longer this job's to
    /// publish (replaced, cancelled, missed).
    fn claim(&self, id: PublicationId, job: JobId) -> Result<bool, JobFailure> {
        let Some(publication) = self.publications.publication(id).map_err(unexpected)? else {
            return Ok(false);
        };
        if publication.upload().is_none_or(|upload| upload.job != job) {
            return Ok(false);
        }
        let claimed = self
            .publications
            .claim_upload(&publication, SystemTime::now())
            .map_err(unexpected)?;
        if !claimed {
            tracing::info!("a scheduled upload was not this run's to publish");
        }
        Ok(claimed)
    }

    /// Whether the project's render is still the one reviewed, with the
    /// same file: a render made while the upload was stopped rewrote it.
    fn render_unchanged(
        &self,
        project: VideoProjectId,
        payload: &UploadPayload,
    ) -> Result<bool, JobFailure> {
        let render = self
            .renders
            .renders(project)
            .map_err(unexpected)?
            .into_iter()
            .find(|render| render.id.to_string() == payload.render);
        Ok(render.is_some_and(|render| render.file == payload.file)
            && self
                .files
                .size(project, &payload.file)
                .is_ok_and(|size| size == payload.size))
    }

    /// Records why the upload failed and hands the failure to the queue.
    fn failed(
        &self,
        id: PublicationId,
        job: JobId,
        error: &UploadError,
        network: Network,
    ) -> JobFailure {
        let (failure, reason) = failure_of(error, network);
        if let Some(reason) = reason
            && let Err(error) = self.update(id, job, |p| {
                p.upload_mut().map_or(Ok(()), |upload| upload.fail(reason))
            })
        {
            tracing::warn!("could not record the upload failure: {}", error.detail);
        }
        failure
    }

    fn send(
        &self,
        cx: &mut JobContext,
        payload: &UploadPayload,
        account: &NetworkAccount,
        uploader: &dyn VideoUploader,
        checkpoint: UploadCheckpoint,
    ) -> Result<Option<UploadCheckpoint>, UploadError> {
        let project: VideoProjectId = id(&payload.project)
            .map_err(|failure| UploadError::new(UploadErrorKind::Local, failure.detail))?;
        let video = payload
            .video()
            .map_err(|failure| UploadError::new(UploadErrorKind::Local, failure.detail))?;
        let mut run = JobRun {
            cx,
            handler: self,
            account,
            project,
            payload,
            checkpoint,
            reader: None,
        };
        match uploader.upload(&video, &mut run)? {
            UploadOutcome::Stopped => Ok(None),
            UploadOutcome::Uploaded(video) => {
                let mut checkpoint = run.checkpoint;
                checkpoint.confirmed = payload.size;
                checkpoint.video = Some(video.id);
                checkpoint.dropped = None;
                run.cx
                    .save_checkpoint(to_json(&checkpoint), sending_progress(9, 10))
                    .map_err(JobRun::local)?;
                Ok(Some(checkpoint))
            }
        }
    }

    /// Whether the account is still connected to the one the user
    /// reviewed the upload for: reconnected to another, the upload would go
    /// there.
    fn check_target(
        &self,
        account: &NetworkAccount,
        payload: &UploadPayload,
    ) -> Result<bool, UploadError> {
        if payload.destination.is_empty() {
            return Ok(true);
        }
        let identity = self
            .connections
            .identity(account)
            .map_err(|error| UploadError::new(UploadErrorKind::SignedOut, error.to_string()))?;
        Ok(identity.id == payload.destination)
    }

    /// When the account is over the network's publishing limit, how long
    /// the upload waits; `None` when it may go now or the network has no
    /// limit. A limit Bardo may not read does not hold the upload: the
    /// network enforces it when publishing anyway.
    fn limit_hold(
        &self,
        uploader: &dyn VideoUploader,
        account: &NetworkAccount,
        payload: &UploadPayload,
    ) -> Result<Option<Held>, UploadError> {
        let token = access_token(&self.connections, account)?;
        let limit = match uploader.publishing_limit(&token, &payload.destination) {
            Ok(Some(limit)) => limit,
            Ok(None) => return Ok(None),
            Err(error) if error.kind == UploadErrorKind::NotAllowed => {
                tracing::warn!(
                    network = account.network.code(),
                    "could not read the publishing limit: {}",
                    error.detail
                );
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if limit.has_room() {
            return Ok(None);
        }
        let published = self
            .publications
            .all_publications(account.owner)
            .map_err(|error| UploadError::new(UploadErrorKind::Local, error.to_string()))?
            .into_iter()
            .filter(|publication| {
                publication.account == account.id
                    && publication
                        .upload()
                        .is_some_and(|upload| upload.status == UploadStatus::Published)
            })
            .map(|publication| publication.posted_at)
            .collect::<Vec<_>>();
        let next = limit.next_try(SystemTime::now(), &published);
        tracing::info!(
            network = account.network.code(),
            used = limit.used,
            total = limit.total,
            "an upload waits for the publishing limit"
        );
        Ok(Some(Held {
            until: to_millis(next.at),
            frees: next.frees,
            used: Some(limit.used),
            total: Some(limit.total),
        }))
    }

    /// Leaves the job queued until `held.until`, keeping where it got.
    fn hold(
        cx: &mut JobContext,
        mut checkpoint: UploadCheckpoint,
        held: Held,
        size: u64,
    ) -> Result<(), JobFailure> {
        checkpoint.held = Some(held);
        let progress = match checkpoint.video {
            Some(_) => sending_progress(9, 10),
            None => sending_progress(checkpoint.confirmed, size),
        };
        cx.save_checkpoint(to_json(&checkpoint), progress)
            .map_err(unexpected)?;
        cx.defer(from_millis(held.until));
        Ok(())
    }

    /// Leaves a processed upload queued until its due time, holding no
    /// place in the queue, with where it got.
    fn wait_for_due(
        cx: &mut JobContext,
        mut checkpoint: UploadCheckpoint,
        due: SystemTime,
    ) -> Result<(), JobFailure> {
        checkpoint.held = None;
        cx.save_checkpoint(to_json(&checkpoint), sending_progress(9, 10))
            .map_err(unexpected)?;
        cx.defer(due);
        Ok(())
    }

    /// The network dropped `video` before it was published: the job sends
    /// the file again, from its first byte, right away, at most
    /// [`MAX_RESTARTS`] times. The checkpoint goes first, so a job stopped
    /// in between still starts over.
    fn start_over(
        &self,
        cx: &mut JobContext,
        checkpoint: &UploadCheckpoint,
        video: &str,
        publication: PublicationId,
        job: JobId,
        network: Network,
    ) -> Result<(), JobFailure> {
        if checkpoint.restarts >= MAX_RESTARTS {
            let error = UploadError::new(
                UploadErrorKind::Unexpected,
                "the network dropped the upload again",
            );
            return Err(self.failed(publication, job, &error, network));
        }
        let restarted = UploadCheckpoint {
            dropped: Some(video.to_owned()),
            restarts: checkpoint.restarts + 1,
            ..UploadCheckpoint::default()
        };
        cx.save_checkpoint(to_json(&restarted), Progress::of(0, 1))
            .map_err(unexpected)?;
        self.update(publication, job, |p| {
            p.upload_mut().map_or(Ok(()), Upload::expired)
        })?;
        tracing::info!("the network dropped an upload; it goes again");
        cx.defer(SystemTime::now());
        Ok(())
    }

    /// Publishes the processed upload `video`, within the publishing limit.
    #[allow(clippy::too_many_arguments)]
    fn publish(
        &self,
        uploader: &dyn VideoUploader,
        account: &NetworkAccount,
        payload: &UploadPayload,
        video: &str,
        publication: PublicationId,
        job: JobId,
        network: Network,
    ) -> Result<Publishing, JobFailure> {
        match self.limit_hold(uploader, account, payload) {
            Ok(Some(held)) => return Ok(Publishing::Held(held)),
            Ok(None) => {}
            Err(error) => return Err(self.failed(publication, job, &error, network)),
        }
        let post = access_token(&self.connections, account)
            .and_then(|token| uploader.publish(&token, &payload.destination, video));
        match post {
            Ok(post) => {
                if let Some(issue) = &post.issue {
                    tracing::info!(network = network.code(), %issue, "published with an issue");
                }
                self.update(publication, job, |p| p.published(post, SystemTime::now()))?;
                Ok(Publishing::Done)
            }
            Err(error) => match error.kind {
                UploadErrorKind::NotReady => Ok(Publishing::NotReady),
                UploadErrorKind::Expired => Ok(Publishing::Expired),
                // Over the limit after all (another app published in
                // between): read it again later.
                UploadErrorKind::UploadLimit => Ok(Publishing::Held(Held {
                    until: to_millis(SystemTime::now() + LIMIT_RECHECK),
                    frees: false,
                    used: None,
                    total: None,
                })),
                _ => Err(self.failed(publication, job, &error, network)),
            },
        }
    }
}

impl JobHandler for UploadHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: UploadPayload = parse(payload)?;
        let publication_id: PublicationId = id(&payload.publication)?;
        let job = cx.id();
        let Some(publication) = self
            .publications
            .publication(publication_id)
            .map_err(unexpected)?
        else {
            // Replaced or removed since: nothing to send.
            return Ok(());
        };
        let Some(upload) = publication.upload() else {
            return Ok(());
        };
        if upload.job != job || upload.status.is_on_network() {
            return Ok(());
        }
        let network: Network = payload.network.parse().map_err(unexpected)?;
        // A scheduled upload Bardo publishes itself (`bardo_domain::due`):
        // what its due time allows now. Nothing goes once it is missed.
        let due = publication.due();
        let mut claimed = upload.claimed_at.is_some();
        if let Some(due) = due {
            if upload.is_missed() {
                return Err(missed());
            }
            match due_step(due, upload.claimed_at, SystemTime::now(), self.opened_at) {
                DueStep::Wait(at) => {
                    cx.defer(at);
                    return Ok(());
                }
                DueStep::Prepare => {}
                DueStep::Publish => {
                    if !self.claim(publication_id, job)? {
                        return Ok(());
                    }
                    claimed = true;
                }
                DueStep::Missed => {
                    self.update(publication_id, job, |p| {
                        p.upload_mut().map_or(Ok(()), Upload::miss)
                    })?;
                    tracing::info!(
                        network = network.code(),
                        "a scheduled upload missed its due time"
                    );
                    return Err(missed());
                }
            }
        }
        // Once the due time came during the run, the run claims the upload
        // before it goes on; false when it is not this run's any more.
        let claim_due = |claimed: &mut bool| -> Result<bool, JobFailure> {
            match due {
                Some(due) if !*claimed && SystemTime::now() >= due => {
                    *claimed = self.claim(publication_id, job)?;
                    Ok(*claimed)
                }
                _ => Ok(true),
            }
        };
        let uploader = self
            .uploaders
            .iter()
            .find(|uploader| uploader.network() == network)
            .ok_or_else(|| JobFailure::unexpected(format!("no uploader for {network:?}")))?;
        let account_id: NetworkAccountId = id(&payload.account)?;
        let Some(account) = self.accounts.get(account_id).map_err(unexpected)? else {
            let error = UploadError::new(UploadErrorKind::SignedOut, "the account was removed");
            return Err(self.failed(publication_id, job, &error, network));
        };
        match self.check_target(&account, &payload) {
            Ok(true) => {}
            // Not retryable: the user reviews the upload for the account
            // as it is connected now.
            Ok(false) => {
                return self.update(publication_id, job, |p| {
                    p.upload_mut()
                        .map_or(Ok(()), |upload| upload.fail(UploadFailure::AccountChanged))
                });
            }
            Err(error) => return Err(self.failed(publication_id, job, &error, network)),
        }
        let mut checkpoint: UploadCheckpoint = match cx.checkpoint() {
            Some(text) => parse(text)?,
            None => UploadCheckpoint::default(),
        };
        if checkpoint.dropped.is_none() {
            checkpoint.video = checkpoint.video.or_else(|| payload.video.clone());
        } else {
            // Starting over: the publication too, if the job stopped
            // before it was saved.
            self.update(publication_id, job, |p| {
                p.upload_mut().map_or(Ok(()), |upload| match upload.status {
                    UploadStatus::Uploading | UploadStatus::Processing => upload.expired(),
                    _ => Ok(()),
                })
            })?;
        }

        if checkpoint.video.is_none() {
            let project: VideoProjectId = id(&payload.project)?;
            if !self.render_unchanged(project, &payload)? {
                // Not retryable: the user reviews the new render.
                return self.update(publication_id, job, |p| {
                    p.upload_mut()
                        .map_or(Ok(()), |upload| upload.fail(UploadFailure::RenderChanged))
                });
            }
            // A new post only goes up within the publishing limit; one on
            // its way finishes, and the limit is read again before it is
            // published. One scheduled for later reads it then.
            let early = due.is_some_and(|due| SystemTime::now() < due);
            if live_session(cx, &checkpoint).is_none() && !early {
                match self.limit_hold(uploader.as_ref(), &account, &payload) {
                    Ok(Some(held)) => return Self::hold(cx, checkpoint, held, payload.size),
                    Ok(None) => {}
                    Err(_) if cx.should_stop() => return Ok(()),
                    Err(error) => return Err(self.failed(publication_id, job, &error, network)),
                }
            }
            checkpoint.held = None;
            self.update(publication_id, job, |p| {
                p.upload_mut().map_or(Ok(()), Upload::start)
            })?;
            match self.send(cx, &payload, &account, uploader.as_ref(), checkpoint) {
                Ok(Some(sent)) => checkpoint = sent,
                // Stopped: the session and the bytes are saved.
                Ok(None) => return Ok(()),
                Err(_) if cx.should_stop() => return Ok(()),
                // Not retryable: the publish time passed before the video
                // went, and the user reviews the upload with a new one.
                Err(error)
                    if matches!(
                        error.kind,
                        UploadErrorKind::Late | UploadErrorKind::ScheduleRefused
                    ) =>
                {
                    return self.update(publication_id, job, |p| {
                        p.upload_mut()
                            .map_or(Ok(()), |upload| upload.fail(UploadFailure::ScheduleMissed))
                    });
                }
                Err(error) => return Err(self.failed(publication_id, job, &error, network)),
            }
        }
        let video = checkpoint.video.clone().expect("sent above or before");
        let link = uploader
            .link(&video)
            .map_err(|error| self.failed(publication_id, job, &error, network))?;
        // Sent by an earlier run of the job, which the user queued again
        // (a missed upload sent now or rescheduled).
        self.update(publication_id, job, |p| {
            if let Some(upload) = p.upload_mut()
                && upload.status == UploadStatus::Queued
            {
                upload.start()?;
            }
            p.sent(link)
        })?;
        if !claim_due(&mut claimed)? {
            return Ok(());
        }

        // Wait for the network to process it, for a while: the job holds a
        // place in the queue meanwhile. Past that the video shows as still
        // processing, and the user checks again later.
        let mut polls = 0;
        loop {
            if cx.should_stop() || !claim_due(&mut claimed)? {
                return Ok(());
            }
            let state = access_token(&self.connections, &account)
                .and_then(|token| uploader.state(&token, &video));
            let outcome = match state {
                Ok(VideoState::Processing) => {
                    if polls >= uploader.processing_polls() || !cx.sleep(uploader.poll_delay(polls))
                    {
                        return Ok(());
                    }
                    polls += 1;
                    continue;
                }
                // Instagram: processed, and Bardo makes the post, at its due
                // time when it has one.
                Ok(VideoState::Processed) => {
                    if let Some(due) = due.filter(|due| SystemTime::now() < *due) {
                        return Self::wait_for_due(cx, checkpoint, due);
                    }
                    if !claim_due(&mut claimed)? {
                        return Ok(());
                    }
                    match self.publish(
                        uploader.as_ref(),
                        &account,
                        &payload,
                        &video,
                        publication_id,
                        job,
                        network,
                    )? {
                        Publishing::Done => return Ok(()),
                        Publishing::Held(held) => {
                            return Self::hold(cx, checkpoint, held, payload.size);
                        }
                        Publishing::Expired => {
                            return self.start_over(
                                cx,
                                &checkpoint,
                                &video,
                                publication_id,
                                job,
                                network,
                            );
                        }
                        Publishing::NotReady => {
                            if polls >= uploader.processing_polls()
                                || !cx.sleep(uploader.poll_delay(polls))
                            {
                                return Ok(());
                            }
                            polls += 1;
                            continue;
                        }
                    }
                }
                Ok(VideoState::Expired) => {
                    return self.start_over(cx, &checkpoint, &video, publication_id, job, network);
                }
                Ok(VideoState::Ready {
                    visibility,
                    publish_at,
                    published_at,
                }) => {
                    // A schedule YouTube already published has its own time.
                    let at = published_at
                        .filter(|_| visibility != Visibility::Private)
                        .unwrap_or_else(SystemTime::now);
                    return self.update(publication_id, job, |p| {
                        p.processed(visibility, publish_at, at)
                    });
                }
                Ok(VideoState::Failed(reason)) => UploadFailure::ProcessingFailed(reason),
                Ok(VideoState::Rejected(reason)) => UploadFailure::Rejected(reason),
                Ok(VideoState::Removed) => UploadFailure::Removed,
                Err(_) if cx.should_stop() => return Ok(()),
                Err(error) => return Err(self.failed(publication_id, job, &error, network)),
            };
            // The upload worked; the network did not take the video. The job
            // is done: retrying it would not change that.
            return self.update(publication_id, job, |p| {
                p.upload_mut().map_or(Ok(()), |upload| upload.fail(outcome))
            });
        }
    }
}

impl Bardo {
    /// Whether the network has an uploader.
    pub fn uploads_to(&self, network: Network) -> bool {
        self.uploaders
            .iter()
            .any(|uploader| uploader.network() == network)
    }

    /// Whether the job is running or waiting.
    pub(crate) fn job_active(&self, job: JobId) -> bool {
        self.job(job).is_some_and(|job| job.state().is_active())
    }

    /// Whether an upload of the project is running or waiting: a render
    /// would rewrite the file it sends.
    pub(crate) fn project_uploading(&self, project: VideoProjectId) -> bool {
        let project = project.to_string();
        self.jobs().iter().any(|job| {
            job.kind() == JobKind::Upload
                && job.state().is_active()
                && serde_json::from_str::<UploadPayload>(job.payload())
                    .is_ok_and(|payload| payload.project == project)
        })
    }

    fn job(&self, id: JobId) -> Option<Job> {
        self.jobs().into_iter().find(|job| job.id() == id)
    }

    /// Where an uploaded publication stands, with its job; `None` for a
    /// manual one.
    pub fn upload_state(&self, publication: &Publication) -> Option<UploadState> {
        let upload = publication.upload()?;
        let job = self.job(upload.job);
        Some(
            due_state(publication, job.as_ref())
                .unwrap_or_else(|| upload_state(upload, job.as_ref())),
        )
    }

    /// Why an upload to `network` failed, as the Publish stage says it.
    pub fn upload_failure_text(&self, failure: &UploadFailure, network: Network) -> String {
        let name = self.text(Text::NetworkName(network));
        let with = |text: Text, reason: &str| {
            self.text_with(text, &[("network", &name), ("reason", reason)])
        };
        match failure {
            UploadFailure::QuotaExceeded => self.text(Text::UploadFailureQuota).into_owned(),
            UploadFailure::UploadLimit => self.text(Text::UploadFailureUploadLimit).into_owned(),
            UploadFailure::ReconnectNeeded => self.text(Text::UploadFailureReconnect).into_owned(),
            UploadFailure::Rejected(reason) => with(Text::UploadFailureRejected, reason),
            UploadFailure::ProcessingFailed(reason) => with(Text::UploadFailureProcessing, reason),
            UploadFailure::Removed => with(Text::UploadFailureRemoved, ""),
            UploadFailure::RenderChanged => {
                self.text(Text::UploadFailureRenderChanged).into_owned()
            }
            UploadFailure::ScheduleMissed => {
                self.text(Text::UploadFailureScheduleMissed).into_owned()
            }
            UploadFailure::AccountChanged => with(Text::UploadFailureAccountChanged, ""),
            UploadFailure::Job(kind) => self.text(Text::JobFailureKindName(*kind)).into_owned(),
        }
    }

    /// One Reel spec problem as the review lists it.
    pub fn reel_spec_text(&self, problem: &ReelSpecProblem) -> String {
        let text = Text::UploadSpec(problem.code());
        let duration =
            |at: &Duration| self.localized_decimal(&bardo_domain::format_cover_time(*at));
        match problem {
            ReelSpecProblem::Unreadable
            | ReelSpecProblem::Container
            | ReelSpecProblem::MoovNotFirst
            | ReelSpecProblem::NoVideo => self.text(text).into_owned(),
            ReelSpecProblem::Codec(codec) => self.text_with(text, &[("codec", codec)]),
            ReelSpecProblem::FrameRate(hundredths) => {
                let fps = f64::from(*hundredths) / 100.0;
                let fps = if hundredths % 100 == 0 {
                    format!("{}", hundredths / 100)
                } else {
                    self.localized_decimal(&format!("{fps:.2}"))
                };
                self.text_with(text, &[("fps", &fps)])
            }
            ReelSpecProblem::TooWide(width) => {
                self.text_with(text, &[("width", &width.to_string())])
            }
            ReelSpecProblem::TooShort(at) | ReelSpecProblem::TooLong(at) => {
                self.text_with(text, &[("duration", &duration(at))])
            }
            ReelSpecProblem::TooBig(bytes) => {
                let size = self.text_with(
                    Text::RenderSize,
                    &[(
                        "value",
                        &self.tenths(*bytes as f64 / 1_000_000.0).replace('+', ""),
                    )],
                );
                self.text_with(text, &[("size", &size)])
            }
        }
    }

    /// Why a Reel waits queued for Instagram's publishing limit, and when
    /// it goes or Bardo reads the limit again (`frees`, see
    /// [`UploadState::OverLimit`]).
    pub fn upload_over_limit_text(
        &self,
        until: SystemTime,
        frees: bool,
        used: Option<u32>,
        total: Option<u32>,
    ) -> String {
        let when = self.publish_time_text(until);
        match (used, total) {
            (Some(used), Some(total)) => self.text_with(
                if frees {
                    Text::UploadOverLimitHint
                } else {
                    Text::UploadOverLimitRecheckHint
                },
                &[
                    ("used", &used.to_string()),
                    ("total", &total.to_string()),
                    ("when", &when),
                ],
            ),
            _ => self.text_with(Text::UploadOverLimitSoonHint, &[("when", &when)]),
        }
    }

    /// The warning Instagram published the Reel with, in its words.
    pub fn upload_issue_text(&self, issue: &str) -> String {
        self.text_with(Text::UploadIssue, &[("issue", issue)])
    }

    fn upload_target(
        &self,
        project: VideoProjectId,
        network: Network,
    ) -> Result<(ExportTarget, bool), UploadReviewError> {
        let view = self.export_view(project)?;
        let target = view
            .targets
            .into_iter()
            .find(|target| target.network == network)
            .ok_or(UploadReviewError::NoAccount)?;
        Ok((target, view.disclosure))
    }

    /// The upload review of the project's account on `network`.
    pub fn upload_review(
        &self,
        project: VideoProjectId,
        network: Network,
    ) -> Result<UploadReview, UploadReviewError> {
        let (target, synthetic) = self.upload_target(project, network)?;
        let account = self
            .network_accounts
            .get(target.account)?
            .ok_or(UploadReviewError::NoAccount)?;
        let connection = if self.uploads_to(network) {
            self.connection_state(&account)?
        } else {
            ConnectionState::Unavailable
        };
        let replaces = target.posted.map(|post| post.publication);
        let uploading = replaces
            .as_ref()
            .and_then(Publication::upload)
            .is_some_and(|upload| self.job_active(upload.job));
        let rendering = self
            .latest_job_of(JobKind::Render, project)
            .is_some_and(|job| job.state().is_active());
        let post = target
            .metadata
            .as_ref()
            .map(|metadata| metadata.post(&target.footer, target.visibility));
        let size = target
            .render
            .as_ref()
            .and_then(|render| self.files.size(project, &render.file).ok());
        // The channel too: reconnecting the account to another channel
        // sends the video elsewhere.
        let channel = match &connection {
            ConnectionState::Connected { channel }
            | ConnectionState::ReconnectNeeded { channel } => channel.as_str(),
            _ => "",
        };
        let destination = match &connection {
            ConnectionState::Connected { .. } | ConnectionState::ReconnectNeeded { .. } => self
                .connection_book
                .connections()
                .identity(&account)
                .map(|identity| identity.id)
                .unwrap_or_default(),
            _ => String::new(),
        };
        let spec_problems = match (&target.render, size) {
            // A render on its way rewrites the file: checked once it is done.
            (Some(render), Some(size)) if network.uploads_reels() && size > 0 && !rendering => {
                self.reel_problems(project, render, size)
            }
            _ => Vec::new(),
        };
        let stamp = format!(
            "{}|{}|{}|{}|{synthetic}|{channel}|{destination}",
            target
                .render
                .as_ref()
                .map(|render| format!("{}:{}:{size:?}", render.id, render.cut))
                .unwrap_or_default(),
            target.render_current,
            post.as_ref().map(Post::fingerprint).unwrap_or_default(),
            replaces
                .as_ref()
                .map(|publication| publication.id.to_string())
                .unwrap_or_default(),
        );
        Ok(UploadReview {
            project,
            account: account.id,
            network,
            handle: target.handle,
            connection,
            render: target.render,
            render_current: target.render_current,
            post,
            problems: target.problems,
            visibility: target.visibility,
            synthetic,
            replaces,
            rendering,
            uploading,
            size,
            destination,
            spec_problems,
            stamp,
        })
    }

    /// What keeps `render` (of `size` bytes) from being a Reel. Read once
    /// per render and size, a file Bardo could not read included: probing
    /// runs ffprobe, and the review is read again whenever a job moves.
    fn reel_problems(
        &self,
        project: VideoProjectId,
        render: &Render,
        size: u64,
    ) -> Vec<ReelSpecProblem> {
        let key = (render.id, size);
        let mut checked = self
            .reel_checks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(problems) = checked.get(&key) {
            return problems.clone();
        }
        let problems = match self.read_reel(project, render, size) {
            Some(file) => check_reel(&file),
            None => vec![ReelSpecProblem::Unreadable],
        };
        checked.insert(key, problems.clone());
        problems
    }

    fn read_reel(&self, project: VideoProjectId, render: &Render, size: u64) -> Option<ReelFile> {
        let mut file = self.files.open(project, &render.file).ok()?;
        let layout = mp4_layout(&mut file).ok()?;
        let info = match self.media.probe(&self.files.path(project, &render.file)) {
            Ok(info) => info,
            Err(error) => {
                tracing::warn!("could not probe the render for the Reel specs: {error}");
                return None;
            }
        };
        Some(ReelFile {
            layout,
            codec: info.video.as_ref().map(|video| video.codec.clone()),
            width: info.video.as_ref().map_or(0, |video| video.width),
            fps: info.video.as_ref().map_or(0.0, |video| video.fps()),
            duration: info.duration,
            size,
        })
    }

    /// Starts the upload the user reviewed, with their choices. Refuses
    /// what blocks it now, a review that no longer matches the render, cut
    /// and post, and replacing the network's publication without the
    /// user's confirmation.
    pub fn start_upload(
        &self,
        review: &UploadReview,
        choices: UploadChoices,
    ) -> Result<JobId, UploadReviewError> {
        // What is stored now, not what the screen last read.
        let now = self.upload_review(review.project, review.network)?;
        if let Some(block) = now.block() {
            return Err(UploadReviewError::Blocked(block));
        }
        if now.stamp != review.stamp || now.account != review.account {
            return Err(UploadReviewError::Changed);
        }
        if now.replaces.is_some() && !choices.replace {
            return Err(UploadReviewError::ReplaceNotConfirmed);
        }
        let (Some(render), Some(post), Some(size)) = (&now.render, &now.post, now.size) else {
            return Err(UploadReviewError::Changed);
        };
        if let Some(at) = choices.publish_at {
            if !now.network.schedules_uploads() && !now.network.schedules_in_app() {
                return Err(UploadReviewError::NoSchedule);
            }
            check_publish_time(at, SystemTime::now()).map_err(UploadReviewError::Schedule)?;
        }
        // Whole milliseconds, as stored.
        let publish_at = choices.publish_at.map(|at| from_millis(to_millis(at)));
        // Instagram takes no publish time: Bardo publishes the Reel at it,
        // and the upload starts no earlier than the network keeps it.
        let starts_at = publish_at
            .filter(|_| now.network.schedules_in_app())
            .map(prepare_at)
            .filter(|start| *start > SystemTime::now());
        let reel = now.is_reel();
        if reel && choices.cover >= render.duration {
            return Err(UploadReviewError::CoverPastEnd);
        }
        // A scheduled upload is public at its time.
        let visibility = match choices.publish_at {
            Some(_) => Visibility::Public,
            None => choices.visibility,
        };
        let publication_id = PublicationId::new();
        let payload = UploadPayload {
            project: now.project.to_string(),
            publication: publication_id.to_string(),
            account: now.account.to_string(),
            network: now.network.code().to_owned(),
            render: render.id.to_string(),
            file: render.file.clone(),
            size,
            title: post.title.clone().unwrap_or_default(),
            description: post.text.clone().unwrap_or_default(),
            tags: post.tags.clone(),
            visibility: visibility.code().to_owned(),
            made_for_kids: choices.made_for_kids && now.network.asks_made_for_kids(),
            synthetic: choices.synthetic,
            video: None,
            publish_at: publish_at
                .filter(|_| now.network.schedules_uploads())
                .map(to_millis),
            share_to_feed: !reel || choices.share_to_feed,
            cover_ms: if reel {
                choices.cover.as_millis() as u64
            } else {
                0
            },
            destination: now.destination.clone(),
        };
        let job = match starts_at {
            Some(at) => Job::scheduled(self.profile.id, JobKind::Upload, to_json(&payload), at),
            None => Job::new(self.profile.id, JobKind::Upload, to_json(&payload)),
        };
        let upload = match publish_at {
            Some(at) => Upload::scheduled(at, job.id()),
            None => Upload::queued(visibility, job.id()),
        };
        let reviewed_at = crate::publications::whole_millis(SystemTime::now());
        let publication = Publication {
            id: publication_id,
            owner: self.profile.id,
            project: now.project,
            account: now.account,
            network: now.network,
            render: render.id,
            link: None,
            kind: PublicationKind::Uploaded(upload),
            posted_at: reviewed_at,
            linked_at: reviewed_at,
            checked_at: None,
            missing_since: None,
        };
        self.publications.save_publication(&publication)?;
        match self.jobs.enqueue(job) {
            Ok(id) => {
                tracing::info!(network = now.network.code(), "queued an upload");
                Ok(id)
            }
            Err(error) => {
                if let Err(error) = self.publications.remove_publication(publication.id) {
                    tracing::warn!("could not remove the unqueued upload: {error}");
                }
                Err(error.into())
            }
        }
    }

    /// The upload job of the publication shown on the Publish stage, with
    /// its payload.
    fn upload_job(&self, job: JobId) -> Result<(Job, UploadPayload), UploadReviewError> {
        let job = self
            .job(job)
            .filter(|job| job.kind() == JobKind::Upload)
            .ok_or(UploadReviewError::NotNow)?;
        let payload = serde_json::from_str(job.payload())
            .map_err(|error| RepositoryError(Box::new(error)))?;
        Ok((job, payload))
    }

    /// What the upload of `job` declared: made for kids, and the
    /// synthetic-content disclosure. Both off when the job is gone.
    pub(crate) fn upload_declarations(&self, job: JobId) -> (bool, bool) {
        self.upload_job(job)
            .map(|(_, payload)| (payload.made_for_kids, payload.synthetic))
            .unwrap_or_default()
    }

    /// Resumes a stopped upload, or retries a failed one, from what the
    /// network has. Not while the project renders: the render rewrites the
    /// file being sent (and the job then refuses the new file).
    pub fn resume_upload(&self, job: JobId) -> Result<(), UploadReviewError> {
        let (_, payload) = self.upload_job(job)?;
        let project: VideoProjectId =
            id(&payload.project).map_err(|failure| RepositoryError(failure.detail.into()))?;
        if self
            .latest_job_of(JobKind::Render, project)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(UploadReviewError::Blocked(UploadBlock::Rendering));
        }
        Ok(self.retry_job(job)?)
    }

    /// Checks again on a video the network was still processing when its
    /// upload job stopped waiting: a new job reads where it stands.
    pub fn check_upload(&self, publication: PublicationId) -> Result<JobId, UploadReviewError> {
        let mut publication = self
            .publications
            .publication(publication)?
            .filter(|publication| publication.owner == self.profile.id)
            .ok_or(UploadReviewError::NotNow)?;
        let network = publication.network();
        let post_id = publication.post_id().map(str::to_owned);
        let Some(upload) = publication.upload_mut() else {
            return Err(UploadReviewError::NotNow);
        };
        let (old, mut payload) = self.upload_job(upload.job)?;
        // A Reel's post id is its shortcode, which only comes once Bardo
        // publishes it: the container is the job's.
        let video = if network.uploads_reels() {
            let checkpoint = old
                .checkpoint()
                .and_then(|text| serde_json::from_str::<UploadCheckpoint>(text).ok())
                .unwrap_or_default();
            // A job that only checked has no video of its own: its payload
            // names it.
            match checkpoint.dropped {
                Some(_) => None,
                None => checkpoint.video.or_else(|| payload.video.clone()),
            }
        } else {
            post_id
        };
        if old.state().is_active() || video.is_none() {
            return Err(UploadReviewError::NotNow);
        }
        payload.video = video;
        let job = Job::new(self.profile.id, JobKind::Upload, to_json(&payload));
        upload
            .check_again(job.id())
            .map_err(|_| UploadReviewError::NotNow)?;
        self.publications.save_publication(&publication)?;
        match self.jobs.enqueue(job) {
            Ok(id) => Ok(id),
            Err(error) => {
                // Back to the job that stopped waiting.
                if let Some(upload) = publication.upload_mut() {
                    upload.job = old.id();
                }
                if let Err(error) = self.publications.save_publication(&publication) {
                    tracing::warn!("could not restore the upload's job: {error}");
                }
                Err(error.into())
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use bardo_domain::{
        Network, NetworkPost, PostLink, PublishingLimit, ScheduleChange, ScheduleOutcome,
        SecretText, UploadError, UploadErrorKind, UploadOutcome, UploadRun, UploadedVideo,
        VideoState, VideoUpload, VideoUploader,
    };

    /// The bytes the fake sends per request.
    const CHUNK: usize = 4;

    /// How one upload attempt goes.
    #[derive(Debug, Clone, Copy)]
    pub(crate) enum Attempt {
        /// Sends the rest of the file.
        Whole,
        /// Sends this many chunks, then fails.
        FailAfter(usize, UploadErrorKind),
        /// Sends this many chunks, then waits to be stopped.
        HoldAfter(usize),
    }

    /// A network that keeps what each session received: a session it
    /// knows resumes where it stopped, as YouTube's do. As Instagram
    /// (`reels`), a processed upload waits for Bardo to publish it, within
    /// a publishing limit.
    #[derive(Default)]
    pub(crate) struct FakeUploader {
        pub(crate) reels: bool,
        /// Answers to publishing limit reads; when empty, there is no limit.
        pub(crate) limits: Mutex<VecDeque<Result<PublishingLimit, UploadErrorKind>>>,
        /// The account each limit read was for.
        pub(crate) limit_reads: Mutex<Vec<String>>,
        /// Answers to publishing; when empty, the post is made.
        pub(crate) publishes: Mutex<VecDeque<Result<NetworkPost, UploadErrorKind>>>,
        /// Every publish: the account and the upload.
        pub(crate) published: Mutex<Vec<(String, String)>>,
        /// The account each upload went to.
        pub(crate) accounts: Mutex<Vec<String>>,
        pub(crate) attempts: Mutex<VecDeque<Attempt>>,
        /// Answers to the processing checks; when empty, the video is ready
        /// with the visibility asked.
        pub(crate) states: Mutex<VecDeque<Result<VideoState, UploadErrorKind>>>,
        sessions: Mutex<HashMap<String, Vec<u8>>>,
        /// Every chunk sent: its session and first byte.
        pub(crate) chunks: Mutex<Vec<(String, u64)>>,
        /// The videos as each attempt described them.
        pub(crate) videos: Mutex<Vec<VideoUpload>>,
        /// The whole files that arrived.
        pub(crate) files: Mutex<Vec<Vec<u8>>>,
        /// The tokens each request carried.
        pub(crate) tokens: Mutex<Vec<String>>,
        pub(crate) checks: Mutex<Vec<String>>,
        /// Answers to schedule changes; when empty, the change is taken.
        pub(crate) reschedules: Mutex<VecDeque<Result<ScheduleOutcome, UploadErrorKind>>>,
        /// Every schedule change: the video and the change.
        pub(crate) changes: Mutex<Vec<(String, ScheduleChange)>>,
        /// An attempt is holding.
        pub(crate) holding: AtomicBool,
        /// Publishing takes the post, then hangs until this is cleared, as
        /// when Bardo closes while Instagram answers.
        pub(crate) stall_publish: AtomicBool,
        /// A publish is hanging.
        pub(crate) stalled: AtomicBool,
    }

    /// The address of the Reel the fake makes.
    pub(crate) const REEL: &str = "https://www.instagram.com/reel/C1aBcDeFgHi/";

    impl FakeUploader {
        pub(crate) fn reels() -> Self {
            Self {
                reels: true,
                ..Self::default()
            }
        }

        pub(crate) fn limit(&self, limit: Result<PublishingLimit, UploadErrorKind>) {
            self.limits.lock().unwrap().push_back(limit);
        }

        pub(crate) fn publish_answer(&self, post: Result<NetworkPost, UploadErrorKind>) {
            self.publishes.lock().unwrap().push_back(post);
        }

        pub(crate) fn will(&self, attempt: Attempt) {
            self.attempts.lock().unwrap().push_back(attempt);
        }

        pub(crate) fn answer(&self, state: Result<VideoState, UploadErrorKind>) {
            self.states.lock().unwrap().push_back(state);
        }

        /// The network forgets its sessions, as when they expire.
        pub(crate) fn forget_sessions(&self) {
            self.sessions.lock().unwrap().clear();
        }

        pub(crate) fn chunk_starts(&self) -> Vec<u64> {
            self.chunks
                .lock()
                .unwrap()
                .iter()
                .map(|(_, at)| *at)
                .collect()
        }
    }

    impl VideoUploader for FakeUploader {
        fn network(&self) -> Network {
            if self.reels {
                Network::InstagramReels
            } else {
                Network::YouTube
            }
        }

        fn link(&self, id: &str) -> Result<Option<PostLink>, UploadError> {
            if self.reels {
                return Ok(None);
            }
            Ok(Some(
                PostLink::parse(
                    Network::YouTube,
                    &format!("https://www.youtube.com/watch?v={id}"),
                )
                .unwrap(),
            ))
        }

        fn publishing_limit(
            &self,
            access_token: &SecretText,
            account: &str,
        ) -> Result<Option<PublishingLimit>, UploadError> {
            self.tokens
                .lock()
                .unwrap()
                .push(access_token.expose().to_owned());
            self.limit_reads.lock().unwrap().push(account.to_owned());
            match self.limits.lock().unwrap().pop_front() {
                Some(Ok(limit)) => Ok(Some(limit)),
                Some(Err(kind)) => Err(UploadError::new(kind, "the fake failed")),
                None => Ok(None),
            }
        }

        fn publish(
            &self,
            access_token: &SecretText,
            account: &str,
            id: &str,
        ) -> Result<NetworkPost, UploadError> {
            self.tokens
                .lock()
                .unwrap()
                .push(access_token.expose().to_owned());
            self.published
                .lock()
                .unwrap()
                .push((account.to_owned(), id.to_owned()));
            if self.stall_publish.load(Ordering::SeqCst) {
                self.stalled.store(true, Ordering::SeqCst);
                while self.stall_publish.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(2));
                }
                self.stalled.store(false, Ordering::SeqCst);
                return Err(UploadError::new(UploadErrorKind::NotReady, "no answer"));
            }
            match self.publishes.lock().unwrap().pop_front() {
                Some(Ok(post)) => Ok(post),
                Some(Err(kind)) => Err(UploadError::new(kind, "the fake failed")),
                None => Ok(NetworkPost {
                    id: "17900000000000001".into(),
                    link: Some(PostLink::parse(Network::InstagramReels, REEL).unwrap()),
                    issue: None,
                }),
            }
        }

        fn upload(
            &self,
            video: &VideoUpload,
            run: &mut dyn UploadRun,
        ) -> Result<UploadOutcome, UploadError> {
            let token = run.access_token()?;
            self.tokens.lock().unwrap().push(token.expose().to_owned());
            if run.session().is_none() && video.is_late(std::time::SystemTime::now()) {
                return Err(UploadError::new(UploadErrorKind::Late, "the fake refused"));
            }
            self.videos.lock().unwrap().push(video.clone());
            self.accounts.lock().unwrap().push(run.account().to_owned());
            let attempt = self
                .attempts
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Attempt::Whole);
            let known = run
                .session()
                .filter(|session| self.sessions.lock().unwrap().contains_key(session));
            let session = match known {
                Some(session) => session,
                None => {
                    let mut sessions = self.sessions.lock().unwrap();
                    let session = format!("https://upload.test/session-{}", sessions.len() + 1);
                    sessions.insert(session.clone(), Vec::new());
                    drop(sessions);
                    run.session_started(&session)?;
                    session
                }
            };
            let mut sent = 0;
            loop {
                let have = self.sessions.lock().unwrap()[&session].len() as u64;
                run.confirmed(have)?;
                if have == run.size() {
                    let file = self.sessions.lock().unwrap()[&session].clone();
                    let mut files = self.files.lock().unwrap();
                    files.push(file);
                    return Ok(UploadOutcome::Uploaded(UploadedVideo {
                        id: format!("Xb7kQ2mN9p{}", files.len() % 10),
                    }));
                }
                match attempt {
                    Attempt::FailAfter(chunks, kind) if sent == chunks => {
                        return Err(UploadError::new(kind, "the fake failed"));
                    }
                    Attempt::HoldAfter(chunks) if sent == chunks => {
                        self.holding.store(true, Ordering::SeqCst);
                        while !run.should_stop() {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        self.holding.store(false, Ordering::SeqCst);
                        return Ok(UploadOutcome::Stopped);
                    }
                    _ => {}
                }
                let len = CHUNK.min((run.size() - have) as usize);
                let bytes = run.read(have, len)?;
                self.chunks.lock().unwrap().push((session.clone(), have));
                self.sessions
                    .lock()
                    .unwrap()
                    .get_mut(&session)
                    .unwrap()
                    .extend_from_slice(&bytes);
                sent += 1;
            }
        }

        fn state(&self, access_token: &SecretText, id: &str) -> Result<VideoState, UploadError> {
            self.tokens
                .lock()
                .unwrap()
                .push(access_token.expose().to_owned());
            self.checks.lock().unwrap().push(id.to_owned());
            match self.states.lock().unwrap().pop_front() {
                // Instagram drops the container and its bytes.
                Some(Ok(VideoState::Expired)) => {
                    self.forget_sessions();
                    Ok(VideoState::Expired)
                }
                Some(Ok(state)) => Ok(state),
                None if self.reels => Ok(VideoState::Processed),
                Some(Err(kind)) => Err(UploadError::new(kind, "the fake failed")),
                // As asked: a scheduled video stays private with its time.
                None => {
                    let last = self.videos.lock().unwrap().last().cloned();
                    let publish_at = last.as_ref().and_then(|video| video.publish_at);
                    Ok(VideoState::Ready {
                        visibility: match (&last, publish_at) {
                            (_, Some(_)) | (None, _) => bardo_domain::Visibility::Private,
                            (Some(video), None) => video.visibility,
                        },
                        publish_at,
                        published_at: None,
                    })
                }
            }
        }

        fn reschedule(
            &self,
            access_token: &SecretText,
            id: &str,
            change: &ScheduleChange,
        ) -> Result<ScheduleOutcome, UploadError> {
            self.tokens
                .lock()
                .unwrap()
                .push(access_token.expose().to_owned());
            self.changes
                .lock()
                .unwrap()
                .push((id.to_owned(), change.clone()));
            match self.reschedules.lock().unwrap().pop_front() {
                Some(Ok(outcome)) => Ok(outcome),
                Some(Err(kind)) => Err(UploadError::new(kind, "the fake failed")),
                None => Ok(ScheduleOutcome::Changed),
            }
        }

        fn poll_delay(&self, _polls: u32) -> Duration {
            Duration::from_millis(1)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, Instant, SystemTime};

    use bardo_domain::{
        CaptionStyle, ConnectedIdentity, ConnectionSecrets, ConnectionStatus, NetworkConnection,
        NetworkConnectionRepository, PersonaDetails, PersonaDraft, PersonaRepository, Provider,
        TokenGrant, TokenSet, VideoMetadataDraft,
    };

    use super::testing::Attempt;
    use super::*;
    use crate::export::tests::{Setup, generated, rendered};
    use crate::render::tests::checked;
    use crate::scenes::tests::{done, wait_done};
    use crate::{EditAction, PublicationError, RenderError};

    const FILE: &[u8] = b"0123456789";
    pub(crate) const ACCESS: &str = "ya29.upload-access";

    fn youtube(s: &Setup) -> NetworkAccount {
        NetworkAccountRepository::list(&*s.h.db, s.project.channel)
            .unwrap()
            .into_iter()
            .find(|account| account.network == Network::YouTube)
            .unwrap()
    }

    /// The project rendered (its YouTube file is `FILE`) with metadata
    /// written, and its YouTube account connected.
    pub(crate) fn ready() -> Setup {
        let s = rendered();
        generated(&s);
        s.h.files
            .write(s.project.id, "render-youtube.mp4", FILE)
            .unwrap();
        connect(&s);
        s
    }

    pub(crate) fn connect(s: &Setup) {
        connect_to(s, "Space Archives");
    }

    fn connect_to(s: &Setup, channel: &str) {
        let account = youtube(s);
        let now = SystemTime::now();
        let tokens = TokenSet::granted(
            &TokenGrant {
                access_token: SecretText::new(ACCESS),
                refresh_token: Some(SecretText::new("1//refresh")),
                expires_in: Duration::from_secs(3600),
                scopes: Vec::new(),
            },
            now,
        );
        s.h.connection_secrets
            .set_tokens(s.app.profile().id, account.id, &tokens)
            .unwrap();
        NetworkConnectionRepository::save(
            &*s.h.db,
            &NetworkConnection {
                account: account.id,
                owner: s.app.profile().id,
                status: ConnectionStatus::Connected,
                identity: ConnectedIdentity {
                    id: "UCarchives".into(),
                    name: channel.into(),
                },
                scopes: Vec::new(),
                expires_at: tokens.expires_at(),
                connected_at: now,
                refreshed_at: None,
            },
        )
        .unwrap();
    }

    pub(crate) fn review(s: &Setup) -> UploadReview {
        s.app.upload_review(s.project.id, Network::YouTube).unwrap()
    }

    pub(crate) fn start(s: &Setup, choices: UploadChoices) -> JobId {
        let review = review(s);
        s.app.start_upload(&review, choices).unwrap()
    }

    pub(crate) fn upload(s: &Setup) -> Publication {
        s.app
            .publications
            .publications(s.project.id)
            .unwrap()
            .into_iter()
            .find(|publication| publication.network() == Network::YouTube)
            .unwrap()
    }

    pub(crate) fn state(s: &Setup) -> UploadState {
        s.app.upload_state(&upload(s)).unwrap()
    }

    fn upload_jobs(app: &Bardo) -> Vec<Job> {
        app.jobs()
            .into_iter()
            .filter(|job| job.kind() == JobKind::Upload)
            .collect()
    }

    fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(Instant::now() < deadline, "never {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_publish_time_that_passed_before_the_upload_started_is_not_retried() {
        let s = ready();
        s.h.uploader
            .will(Attempt::FailAfter(0, UploadErrorKind::Late));
        let review = review(&s);
        let at = SystemTime::now() + std::time::Duration::from_secs(3600);
        let choices = UploadChoices {
            publish_at: Some(at),
            ..review.choices()
        };
        let job = done(&s.app, start(&s, choices));
        assert_eq!(job.attempts(), 1, "not retried by the queue");
        assert_eq!(
            state(&s),
            UploadState::Failed {
                failure: UploadFailure::ScheduleMissed,
                retryable: false,
            },
            "the user reviews it again with a new time"
        );
        let gone = UploadError::new(UploadErrorKind::NotFound, "gone");
        assert_eq!(
            failure_of(&gone, Network::YouTube).1,
            Some(UploadFailure::Removed)
        );
    }

    #[test]
    fn no_upload_starts_without_a_connected_account_and_a_confirmed_review() {
        let s = rendered();
        generated(&s);
        let before = review(&s);
        assert_eq!(before.block(), Some(UploadBlock::NotConnected));
        assert!(matches!(
            s.app.start_upload(&before, before.choices()),
            Err(UploadReviewError::Blocked(UploadBlock::NotConnected))
        ));
        // TikTok has no uploader yet: export it.
        assert_eq!(
            s.app
                .upload_review(s.project.id, Network::TikTok)
                .unwrap()
                .block(),
            Some(UploadBlock::NotOffered)
        );
        assert!(matches!(
            s.app.upload_review(s.project.id, Network::X),
            Err(UploadReviewError::NoAccount)
        ));

        s.h.files
            .write(s.project.id, "render-youtube.mp4", FILE)
            .unwrap();
        connect(&s);
        let review = review(&s);
        assert_eq!(review.block(), None);
        assert_eq!(review.channel(), Some("Space Archives"));
        assert_eq!(review.render.as_ref().unwrap().file, "render-youtube.mp4");
        assert_eq!(review.replaces, None);
        // Reviewing sends nothing.
        assert!(upload_jobs(&s.app).is_empty());
        assert!(s.h.uploader.videos.lock().unwrap().is_empty());
        assert!(
            s.app
                .publications
                .publications(s.project.id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_change_after_the_review_needs_a_new_review() {
        let s = ready();

        // The post.
        let seen = review(&s);
        s.app
            .edit_metadata(
                s.project.id,
                Network::YouTube,
                VideoMetadataDraft {
                    title: "A better title".into(),
                    description: "In 1969 a probe went silent.".into(),
                    tags: vec!["nasa".into()],
                },
            )
            .unwrap();
        assert!(matches!(
            s.app.start_upload(&seen, seen.choices()),
            Err(UploadReviewError::Changed)
        ));

        // The rendered file.
        let seen = review(&s);
        s.h.files
            .write(s.project.id, "render-youtube.mp4", b"0123456789abc")
            .unwrap();
        assert!(matches!(
            s.app.start_upload(&seen, seen.choices()),
            Err(UploadReviewError::Changed)
        ));

        // The synthetic-content disclosure the review started with.
        let seen = review(&s);
        assert!(!seen.synthetic);
        let narrator = s
            .app
            .personas()
            .unwrap()
            .into_iter()
            .find(|p| p.details.name() == "Documentary Narrator (en-US)")
            .unwrap();
        let mut flagged = narrator.clone();
        flagged.details = PersonaDetails::validate(PersonaDraft {
            realistic_voice: true,
            ..PersonaDraft::from(&narrator.details)
        })
        .unwrap();
        PersonaRepository::save(&*s.h.db, &flagged).unwrap();
        assert!(matches!(
            s.app.start_upload(&seen, seen.choices()),
            Err(UploadReviewError::Changed)
        ));
        assert!(review(&s).synthetic, "a realistic voice turns it on");

        // The channel: the account was connected to another one.
        let seen = review(&s);
        connect_to(&s, "Other Channel");
        assert!(matches!(
            s.app.start_upload(&seen, seen.choices()),
            Err(UploadReviewError::Changed)
        ));

        // The cut: the render no longer matches it.
        let seen = review(&s);
        let mut editor = s.app.open_editor(s.project.id).unwrap();
        s.app
            .edit(
                &mut editor,
                EditAction::SetCaptionStyle(CaptionStyle::Punch),
            )
            .unwrap();
        assert!(matches!(
            s.app.start_upload(&seen, seen.choices()),
            Err(UploadReviewError::Blocked(UploadBlock::RenderOutdated))
        ));
        assert_eq!(review(&s).block(), Some(UploadBlock::RenderOutdated));

        assert!(upload_jobs(&s.app).is_empty());
        assert!(s.h.uploader.videos.lock().unwrap().is_empty());
    }

    #[test]
    fn a_confirmed_upload_sends_the_file_with_the_choices_and_is_published() {
        let mut s = ready();
        let review = review(&s);
        let choices = UploadChoices {
            visibility: Visibility::Unlisted,
            made_for_kids: true,
            synthetic: true,
            replace: false,
            publish_at: None,
            share_to_feed: true,
            cover: Duration::ZERO,
        };
        let job = s.app.start_upload(&review, choices).unwrap();
        done(&s.app, job);

        let post = review.post.as_ref().unwrap();
        let sent = s.h.uploader.videos.lock().unwrap().clone();
        assert_eq!(
            sent,
            [VideoUpload {
                title: post.title.clone().unwrap(),
                description: post.text.clone().unwrap(),
                tags: post.tags.clone(),
                visibility: Visibility::Unlisted,
                made_for_kids: true,
                synthetic: true,
                publish_at: None,
                share_to_feed: true,
                cover: Duration::ZERO,
            }]
        );
        assert!(
            post.text
                .as_ref()
                .unwrap()
                .contains("Sources in the channel.")
        );
        assert_eq!(*s.h.uploader.files.lock().unwrap(), [FILE.to_vec()]);
        assert_eq!(s.h.uploader.chunk_starts(), [0, 4, 8]);
        assert!(
            s.h.uploader
                .tokens
                .lock()
                .unwrap()
                .iter()
                .all(|token| token == ACCESS)
        );

        let publication = upload(&s);
        assert_eq!(publication.kind.code(), "uploaded");
        assert_eq!(
            publication.upload().unwrap().status,
            UploadStatus::Published
        );
        assert_eq!(publication.render, review.render.as_ref().unwrap().id);
        assert_eq!(
            publication.link.as_ref().unwrap().url(),
            "https://www.youtube.com/watch?v=Xb7kQ2mN9p1"
        );
        assert_eq!(state(&s), UploadState::Published);
        assert!(publication.has_public_metrics());

        // It joins the public metrics sync.
        s.h.stats.set("Xb7kQ2mN9p1", 42, Some(3));
        s.app
            .save_provider_key(Provider::YouTubeData, "AIzaSyTestKey0001abcdefghij")
            .unwrap();
        done(&s.app, s.app.sync_metrics().unwrap());
        let view = s.app.export_view(s.project.id).unwrap();
        let posted = crate::export::tests::target(&view, Network::YouTube)
            .posted
            .clone()
            .unwrap();
        assert_eq!(posted.latest().unwrap().views, 42);
    }

    #[test]
    fn a_video_kept_private_by_the_network_is_restricted_not_failed() {
        let s = ready();
        s.h.uploader.answer(Ok(VideoState::Processing));
        s.h.uploader.answer(Ok(VideoState::Ready {
            visibility: Visibility::Private,
            publish_at: None,
            published_at: None,
        }));
        let review = review(&s);
        let job = start(
            &s,
            UploadChoices {
                visibility: Visibility::Public,
                ..review.choices()
            },
        );
        done(&s.app, job);

        assert_eq!(s.h.uploader.checks.lock().unwrap().len(), 2, "polled");
        let publication = upload(&s);
        assert_eq!(
            publication.upload().unwrap().status,
            UploadStatus::Restricted
        );
        assert_eq!(state(&s), UploadState::Restricted);
        assert!(!publication.has_public_metrics());
        assert_eq!(
            s.app.text(Text::UploadRestrictedHint),
            "Kept private by YouTube: your Google project has not passed the YouTube API audit."
        );
    }

    #[test]
    fn a_stopped_upload_resumes_without_sending_what_arrived() {
        let s = ready();
        s.h.uploader.will(Attempt::HoldAfter(1));
        let review = review(&s);
        let job = start(&s, review.choices());
        wait_until("held", || {
            s.h.uploader
                .holding
                .load(std::sync::atomic::Ordering::SeqCst)
        });
        assert!(matches!(state(&s), UploadState::Uploading(_)));
        // While it runs, nothing replaces it or rewrites its file.
        assert_eq!(self::review(&s).block(), Some(UploadBlock::Uploading));
        assert!(matches!(
            s.app.remove_publication(upload(&s).id),
            Err(PublicationError::Uploading)
        ));
        let render = checked(&s.app, s.project.id);
        assert!(matches!(
            s.app.start_render(&render, &render.renderable()),
            Err(RenderError::Uploading)
        ));

        s.app.cancel_job(job).unwrap();
        wait_done(&s.app, job);
        assert_eq!(state(&s), UploadState::Stopped);
        let stopped = upload_jobs(&s.app).pop().unwrap();
        assert_eq!(
            stopped.external_handle(),
            Some("https://upload.test/session-1")
        );

        s.app.resume_upload(job).unwrap();
        done(&s.app, job);
        assert_eq!(s.h.uploader.chunk_starts(), [0, 4, 8], "no byte twice");
        assert!(
            s.h.uploader
                .chunks
                .lock()
                .unwrap()
                .iter()
                .all(|(session, _)| session == "https://upload.test/session-1")
        );
        assert_eq!(*s.h.uploader.files.lock().unwrap(), [FILE.to_vec()]);
        assert_eq!(state(&s), UploadState::Published);
    }

    #[test]
    fn a_render_made_while_stopped_ends_the_upload_for_a_new_review() {
        let s = ready();
        s.h.uploader.will(Attempt::HoldAfter(1));
        let review = review(&s);
        let job = start(&s, review.choices());
        wait_until("held", || {
            s.h.uploader
                .holding
                .load(std::sync::atomic::Ordering::SeqCst)
        });
        s.app.cancel_job(job).unwrap();
        wait_done(&s.app, job);
        assert_eq!(state(&s), UploadState::Stopped);

        // Stopped, the project renders again: the file is another one.
        let render = checked(&s.app, s.project.id);
        done(
            &s.app,
            s.app.start_render(&render, &render.renderable()).unwrap(),
        );
        s.h.files
            .write(s.project.id, "render-youtube.mp4", b"9876543210")
            .unwrap();

        s.app.resume_upload(job).unwrap();
        done(&s.app, job);
        assert_eq!(s.h.uploader.chunk_starts(), [0], "nothing of the new file");
        assert_eq!(
            state(&s),
            UploadState::Failed {
                failure: UploadFailure::RenderChanged,
                retryable: false,
            }
        );
        assert_eq!(
            s.app
                .upload_failure_text(&UploadFailure::RenderChanged, Network::YouTube),
            "The render changed after the review, so the rest of the file is not what you reviewed. Review the upload again."
        );

        // A new review sends the new file whole.
        let again = self::review(&s);
        assert_eq!(again.block(), None);
        let next = s
            .app
            .start_upload(
                &again,
                UploadChoices {
                    replace: true,
                    ..again.choices()
                },
            )
            .unwrap();
        done(&s.app, next);
        assert_eq!(
            *s.h.uploader.files.lock().unwrap(),
            [b"9876543210".to_vec()]
        );
        assert_eq!(state(&s), UploadState::Published);
    }

    #[test]
    fn a_video_still_processing_frees_the_queue_and_is_checked_again() {
        let s = ready();
        let polls = s.h.uploader.processing_polls();
        for _ in 0..=polls {
            s.h.uploader.answer(Ok(VideoState::Processing));
        }
        let review = review(&s);
        let first = start(&s, review.choices());
        done(&s.app, first);
        assert_eq!(
            s.h.uploader.checks.lock().unwrap().len(),
            polls as usize + 1
        );
        assert_eq!(state(&s), UploadState::StillProcessing);
        let publication = upload(&s);
        assert_eq!(
            publication.upload().unwrap().status,
            UploadStatus::Processing
        );
        assert!(!publication.is_posted());
        assert!(matches!(
            s.app.resume_upload(first),
            Err(UploadReviewError::Job(_))
        ));

        let check = s.app.check_upload(publication.id).unwrap();
        assert_ne!(check, first);
        done(&s.app, check);
        assert_eq!(state(&s), UploadState::Published);
        let publication = upload(&s);
        assert_eq!(publication.upload().unwrap().job, check);
        assert_eq!(s.h.uploader.chunk_starts(), [0, 4, 8], "not sent again");
        assert_eq!(s.h.uploader.videos.lock().unwrap().len(), 1);
        assert!(matches!(
            s.app.check_upload(publication.id),
            Err(UploadReviewError::NotNow)
        ));
    }

    #[test]
    fn a_dropped_connection_retries_from_what_arrived() {
        let s = ready();
        s.h.uploader
            .will(Attempt::FailAfter(2, UploadErrorKind::NetworkDown));
        let review = review(&s);
        let job = done(&s.app, start(&s, review.choices()));
        assert_eq!(job.attempts(), 2, "retried by the queue");
        assert_eq!(s.h.uploader.chunk_starts(), [0, 4, 8]);
        assert_eq!(state(&s), UploadState::Published);
    }

    #[test]
    fn an_expired_session_sends_the_file_again_in_a_new_one() {
        let s = ready();
        s.h.uploader
            .will(Attempt::FailAfter(1, UploadErrorKind::NetworkDown));
        s.h.uploader.will(Attempt::HoldAfter(0));
        let review = review(&s);
        let job = start(&s, review.choices());
        wait_until("held", || {
            s.h.uploader
                .holding
                .load(std::sync::atomic::Ordering::SeqCst)
        });
        s.app.cancel_job(job).unwrap();
        wait_done(&s.app, job);
        s.h.uploader.forget_sessions();
        s.app.retry_job(job).unwrap();
        done(&s.app, job);
        assert_eq!(s.h.uploader.chunk_starts(), [0, 0, 4, 8]);
        assert_eq!(*s.h.uploader.files.lock().unwrap(), [FILE.to_vec()]);
    }

    #[test]
    fn a_used_up_quota_stops_the_upload_without_retrying() {
        let s = ready();
        s.h.uploader
            .will(Attempt::FailAfter(0, UploadErrorKind::QuotaExceeded));
        let review = review(&s);
        let job = wait_done(&s.app, start(&s, review.choices()));
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.attempts(), 1, "no retry loop");
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::LimitReached);
        assert_eq!(
            upload(&s).upload().unwrap().status,
            UploadStatus::Failed(UploadFailure::QuotaExceeded)
        );
        assert!(matches!(
            state(&s),
            UploadState::Failed {
                failure: UploadFailure::QuotaExceeded,
                retryable: true
            }
        ));
        assert!(
            s.app
                .upload_failure_text(&UploadFailure::QuotaExceeded, Network::YouTube)
                .contains("quota")
        );
    }

    #[test]
    fn a_refused_sign_in_asks_to_reconnect() {
        let s = ready();
        s.h.uploader
            .will(Attempt::FailAfter(0, UploadErrorKind::Refused));
        let review = review(&s);
        let job = wait_done(&s.app, start(&s, review.choices()));
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.attempts(), 1);
        assert_eq!(
            upload(&s).upload().unwrap().status,
            UploadStatus::Failed(UploadFailure::ReconnectNeeded)
        );
    }

    #[test]
    fn a_video_the_network_rejects_fails_the_upload_for_good() {
        let s = ready();
        s.h.uploader
            .answer(Ok(VideoState::Rejected("duplicate".into())));
        let review = review(&s);
        done(&s.app, start(&s, review.choices()));
        assert_eq!(
            upload(&s).upload().unwrap().status,
            UploadStatus::Failed(UploadFailure::Rejected("duplicate".into()))
        );
        assert_eq!(
            state(&s),
            UploadState::Failed {
                failure: UploadFailure::Rejected("duplicate".into()),
                retryable: false
            }
        );
        assert_eq!(
            s.app.upload_failure_text(
                &UploadFailure::Rejected("duplicate".into()),
                Network::YouTube
            ),
            "YouTube rejected the video (duplicate). Fix it and upload again."
        );
        // A new upload replaces it once confirmed.
        let again = self::review(&s);
        assert_eq!(again.block(), None);
        assert!(again.replaces.is_some());
    }

    #[test]
    fn an_upload_replaces_a_linked_post_only_once_confirmed_and_back() {
        let s = ready();
        let view = s.app.export_view(s.project.id).unwrap();
        done(
            &s.app,
            s.app.start_export(&view, &[Network::YouTube]).unwrap(),
        );
        const SHORT: &str = "https://youtube.com/shorts/dQw4w9WgXcQ";
        let manual = s
            .app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();

        let review = review(&s);
        assert_eq!(review.replaces.as_ref(), Some(&manual));
        assert!(matches!(
            s.app.start_upload(&review, review.choices()),
            Err(UploadReviewError::ReplaceNotConfirmed)
        ));
        assert!(upload_jobs(&s.app).is_empty());
        let job = s
            .app
            .start_upload(
                &review,
                UploadChoices {
                    replace: true,
                    ..review.choices()
                },
            )
            .unwrap();
        done(&s.app, job);
        assert_eq!(
            s.app.publications.publications(s.project.id).unwrap().len(),
            1
        );
        assert!(upload(&s).upload().is_some());

        // And back: a pasted link replaces the upload once confirmed.
        assert!(matches!(
            s.app
                .mark_posted(s.project.id, Network::YouTube, SHORT, false),
            Err(PublicationError::ReplacesUpload)
        ));
        let linked = s
            .app
            .mark_posted(s.project.id, Network::YouTube, SHORT, true)
            .unwrap();
        assert_eq!(linked.kind, PublicationKind::Manual);
        assert_eq!(upload(&s), linked);
    }

    #[test]
    fn a_publication_and_its_upload_states_follow_the_job() {
        let s = ready();
        let failed = Upload {
            status: UploadStatus::Failed(UploadFailure::UploadLimit),
            ..Upload::queued(Visibility::Public, JobId::new())
        };
        assert_eq!(
            upload_state(&failed, None),
            UploadState::Failed {
                failure: UploadFailure::UploadLimit,
                retryable: false
            }
        );
        assert!(!upload_state(&failed, None).is_active());

        s.h.uploader.will(Attempt::HoldAfter(0));
        let review = review(&s);
        let job = start(&s, review.choices());
        wait_until("held", || {
            s.h.uploader
                .holding
                .load(std::sync::atomic::Ordering::SeqCst)
        });
        assert!(state(&s).is_active());
        s.app.cancel_job(job).unwrap();
        wait_done(&s.app, job);
        assert!(!state(&s).is_active());
        // A stopped upload can be unlinked; its job then sends nothing.
        s.app.remove_publication(upload(&s).id).unwrap();
        s.app.retry_job(job).unwrap();
        done(&s.app, job);
        assert!(s.h.uploader.files.lock().unwrap().is_empty());
    }
}

/// Reels (#81): the review's specs and choices, publishing within the
/// limit, and a dropped container sent again.
#[cfg(test)]
pub(crate) mod reel_tests {
    use std::time::{Duration, Instant, SystemTime};

    use bardo_domain::{
        ConnectedIdentity, ConnectionSecrets, ConnectionStatus, NetworkConnection,
        NetworkConnectionRepository, NetworkPost, PublishingLimit, TokenGrant, TokenSet,
    };
    use bardo_media::ffmpeg::{AudioStream, MediaInfo, VideoStream};

    use super::testing::{Attempt, REEL};
    use super::*;
    use crate::export::tests::{Setup, rendered_with};
    use crate::scenes::tests::{done, wait_done};

    const IG_USER: &str = "17841400008460056";
    const ACCESS: &str = "EAAG-reel-page-token";
    const FILE: &str = "render-instagram_reels.mp4";

    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(body);
        bytes
    }

    /// A small MP4 file, its index first or last.
    fn reel_file(fast_start: bool) -> Vec<u8> {
        let ftyp = mp4_box(b"ftyp", b"isom\0\0\x02\0isomavc1");
        let moov = mp4_box(b"moov", b"index");
        let mdat = mp4_box(b"mdat", b"frames frames");
        if fast_start {
            [ftyp, moov, mdat].concat()
        } else {
            [ftyp, mdat, moov, b"!".to_vec()].concat()
        }
    }

    fn write_reel(s: &Setup, bytes: &[u8]) {
        s.h.files.write(s.project.id, FILE, bytes).unwrap();
        s.h.media.probes.lock().unwrap().insert(
            String::from_utf8_lossy(bytes).into_owned(),
            MediaInfo {
                duration: Duration::from_secs(30),
                video: Some(VideoStream {
                    codec: "h264".into(),
                    width: 1080,
                    height: 1920,
                    frame_rate: (30, 1),
                }),
                audio: Some(AudioStream {
                    codec: "aac".into(),
                    sample_rate: 48_000,
                    channels: 2,
                }),
            },
        );
    }

    fn reels(s: &Setup) -> NetworkAccount {
        NetworkAccountRepository::list(&*s.h.db, s.project.channel)
            .unwrap()
            .into_iter()
            .find(|account| account.network == Network::InstagramReels)
            .unwrap()
    }

    fn connect_as(s: &Setup, id: &str, name: &str) {
        let account = reels(s);
        let now = SystemTime::now();
        let tokens = TokenSet::granted(
            &TokenGrant {
                access_token: SecretText::new(ACCESS),
                refresh_token: Some(SecretText::new("EAAG-user-token")),
                expires_in: Duration::from_secs(60 * 24 * 60 * 60),
                scopes: Vec::new(),
            },
            now,
        );
        s.h.connection_secrets
            .set_tokens(s.app.profile().id, account.id, &tokens)
            .unwrap();
        NetworkConnectionRepository::save(
            &*s.h.db,
            &NetworkConnection {
                account: account.id,
                owner: s.app.profile().id,
                status: ConnectionStatus::Connected,
                identity: ConnectedIdentity {
                    id: id.into(),
                    name: name.into(),
                },
                scopes: Vec::new(),
                expires_at: tokens.expires_at(),
                connected_at: now,
                refreshed_at: None,
            },
        )
        .unwrap();
    }

    /// Metadata for every account of the project, the Reel's caption with
    /// hashtags.
    fn generate(s: &Setup) {
        let answer = serde_json::json!({ "posts": [
            {
                "network": "youtube",
                "title": "The probe nobody found",
                "description": "In 1969 a probe went silent.",
                "tags": ["space history", "nasa"]
            },
            {
                "network": "tiktok",
                "title": "",
                "description": "The probe nobody found.",
                "tags": ["#space", "nasa"]
            },
            {
                "network": "instagram_reels",
                "title": "",
                "description": "In 1969 a probe went silent. Nobody found it.",
                "tags": ["space", "nasa"]
            }
        ]});
        s.h.text.answers.lock().unwrap().push(answer.to_string());
        done(
            &s.app,
            s.app
                .generate_metadata(s.project.id, crate::BudgetConsent::Ask)
                .unwrap(),
        );
    }

    /// The project rendered with metadata, its Instagram account connected
    /// and its Reel's file in fast start.
    pub(crate) fn ready() -> Setup {
        let s = rendered_with(&[Network::InstagramReels]);
        generate(&s);
        write_reel(&s, &reel_file(true));
        connect_as(&s, IG_USER, "@arquivosdoespaco");
        s
    }

    pub(crate) fn review(s: &Setup) -> UploadReview {
        s.app
            .upload_review(s.project.id, Network::InstagramReels)
            .unwrap()
    }

    fn start(s: &Setup, choices: UploadChoices) -> JobId {
        let review = review(s);
        s.app.start_upload(&review, choices).unwrap()
    }

    pub(crate) fn upload(s: &Setup) -> Publication {
        s.app
            .publications
            .publications(s.project.id)
            .unwrap()
            .into_iter()
            .find(|publication| publication.network() == Network::InstagramReels)
            .unwrap()
    }

    pub(crate) fn state(s: &Setup) -> UploadState {
        s.app.upload_state(&upload(s)).unwrap()
    }

    fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(Instant::now() < deadline, "never {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn full() -> PublishingLimit {
        PublishingLimit {
            used: 50,
            total: 50,
            window: Duration::from_secs(24 * 60 * 60),
        }
    }

    #[test]
    fn a_job_saved_before_reels_reads_with_their_defaults() {
        let payload: UploadPayload = serde_json::from_str(
            r#"{"project":"p","publication":"q","account":"a","network":"youtube",
                "render":"r","file":"f","size":1,"title":"t","description":"d",
                "tags":[],"visibility":"public","made_for_kids":false,"synthetic":false}"#,
        )
        .unwrap();
        assert!(payload.share_to_feed);
        assert_eq!(payload.cover_ms, 0);
        assert_eq!(payload.destination, "");
        let checkpoint: UploadCheckpoint =
            serde_json::from_str(r#"{"confirmed":4,"video":null}"#).unwrap();
        assert_eq!(checkpoint.held, None);
        assert_eq!(checkpoint.dropped, None);
        assert_eq!(
            to_json(&UploadCheckpoint {
                confirmed: 4,
                ..UploadCheckpoint::default()
            }),
            r#"{"confirmed":4,"video":null}"#
        );
    }

    #[test]
    fn spec_problems_read_with_their_numbers() {
        let s = ready();
        let text = |problem| s.app.reel_spec_text(&problem);
        assert_eq!(
            text(ReelSpecProblem::Codec("vp9".into())),
            "Video codec vp9: Instagram takes H.264 or HEVC."
        );
        assert_eq!(
            text(ReelSpecProblem::FrameRate(1500)),
            "15 frames per second: Instagram takes 23 to 60."
        );
        assert_eq!(
            text(ReelSpecProblem::FrameRate(2397)),
            "23.97 frames per second: Instagram takes 23 to 60."
        );
        assert_eq!(
            text(ReelSpecProblem::TooShort(Duration::from_millis(2_500))),
            "0:02.5 long: a Reel is at least 3 seconds."
        );
        assert!(text(ReelSpecProblem::TooBig(312_400_000)).contains("312.4"));
    }

    #[test]
    fn no_reel_goes_up_without_a_confirmed_review_of_a_file_that_meets_the_specs() {
        let s = rendered_with(&[Network::InstagramReels]);
        generate(&s);
        write_reel(&s, &reel_file(false));
        connect_as(&s, IG_USER, "@arquivosdoespaco");

        let seen = review(&s);
        assert!(seen.is_reel());
        assert_eq!(seen.spec_problems, [ReelSpecProblem::MoovNotFirst]);
        assert_eq!(seen.block(), Some(UploadBlock::Specs));
        assert!(matches!(
            s.app.start_upload(&seen, seen.choices()),
            Err(UploadReviewError::Blocked(UploadBlock::Specs))
        ));
        assert_eq!(
            s.app.reel_spec_text(&ReelSpecProblem::MoovNotFirst),
            "The file's index comes after the media (not fast start)."
        );

        // A file that meets them, reviewed before it changed: review again.
        write_reel(&s, &reel_file(true));
        let review = review(&s);
        assert_eq!(review.block(), None);
        assert_eq!(review.channel(), Some("@arquivosdoespaco"));
        let caption = review.post.as_ref().unwrap().text.clone().unwrap();
        assert!(caption.ends_with("#space #nasa"), "{caption}");
        assert!(matches!(
            s.app.start_upload(&seen, seen.choices()),
            Err(UploadReviewError::Blocked(_) | UploadReviewError::Changed)
        ));
        // Nor a due time already past, nor a cover past the end.
        let earlier = SystemTime::now() - Duration::from_secs(60);
        assert!(matches!(
            s.app.start_upload(
                &review,
                UploadChoices {
                    publish_at: Some(earlier),
                    ..review.choices()
                }
            ),
            Err(UploadReviewError::Schedule(ScheduleProblem::Past))
        ));
        let end = review.render.as_ref().unwrap().duration;
        assert!(matches!(
            s.app.start_upload(
                &review,
                UploadChoices {
                    cover: end + Duration::from_secs(1),
                    ..review.choices()
                }
            ),
            Err(UploadReviewError::CoverPastEnd)
        ));
        assert!(s.h.reels.videos.lock().unwrap().is_empty(), "nothing sent");
        assert!(s.h.reels.limit_reads.lock().unwrap().is_empty());
        assert!(
            s.app
                .publications
                .publications(s.project.id)
                .unwrap()
                .is_empty()
        );

        done(
            &s.app,
            s.app.start_upload(&review, review.choices()).unwrap(),
        );
        assert_eq!(state(&s), UploadState::Published);
    }

    #[test]
    fn a_confirmed_reel_goes_up_with_its_choices_and_bardo_publishes_it() {
        let s = ready();
        let review = review(&s);
        let caption = review.post.as_ref().unwrap().text.clone().unwrap();
        let cover = review.render.as_ref().unwrap().duration / 2;
        let choices = UploadChoices {
            synthetic: true,
            share_to_feed: false,
            cover,
            ..review.choices()
        };
        done(&s.app, s.app.start_upload(&review, choices).unwrap());

        let sent = s.h.reels.videos.lock().unwrap().clone();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].description, caption);
        assert!(sent[0].synthetic);
        assert!(!sent[0].share_to_feed);
        assert_eq!(
            sent[0].cover,
            Duration::from_millis(cover.as_millis() as u64)
        );
        assert_eq!(sent[0].publish_at, None);
        assert_eq!(*s.h.reels.accounts.lock().unwrap(), [IG_USER]);
        assert_eq!(*s.h.reels.files.lock().unwrap(), [reel_file(true)]);
        // The limit before the container and again before publishing.
        assert_eq!(*s.h.reels.limit_reads.lock().unwrap(), [IG_USER, IG_USER]);
        let published = s.h.reels.published.lock().unwrap().clone();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, IG_USER);
        assert_eq!(
            published[0].1,
            *s.h.reels.checks.lock().unwrap().last().unwrap()
        );
        assert!(
            s.h.reels
                .tokens
                .lock()
                .unwrap()
                .iter()
                .all(|token| token == ACCESS)
        );

        let publication = upload(&s);
        let upload = publication.upload().unwrap();
        assert_eq!(upload.status, UploadStatus::Published);
        assert_eq!(upload.issue, None);
        assert_eq!(publication.link.as_ref().unwrap().url(), REEL);
        assert_eq!(state(&s), UploadState::Published);
        assert!(
            s.h.uploader.videos.lock().unwrap().is_empty(),
            "not YouTube"
        );
    }

    #[test]
    fn a_reel_over_the_publishing_limit_waits_queued_and_says_when_it_goes() {
        let s = ready();
        s.h.reels.limit(Ok(full()));
        let review = review(&s);
        let before = SystemTime::now();
        let job = start(&s, review.choices());
        wait_until("held", || {
            matches!(state(&s), UploadState::OverLimit { .. })
        });

        let UploadState::OverLimit {
            until,
            frees,
            used,
            total,
        } = state(&s)
        else {
            unreachable!()
        };
        assert_eq!((used, total), (Some(50), Some(50)));
        assert!(!frees, "other apps filled it: Bardo only reads it again");
        // No post of Bardo's in the window: read again in an hour.
        let hour = Duration::from_secs(3600);
        assert!(until >= before + hour - Duration::from_secs(1));
        assert!(until <= SystemTime::now() + hour + Duration::from_secs(1));
        assert!(state(&s).is_active());
        assert_eq!(self::review(&s).block(), Some(UploadBlock::Uploading));
        assert!(s.h.reels.videos.lock().unwrap().is_empty(), "nothing sent");
        let queued = s.app.jobs().into_iter().find(|j| j.id() == job).unwrap();
        assert_eq!(queued.state(), JobState::Queued);
        assert_eq!(queued.attempts(), 0, "waiting is not a failed attempt");
        assert!(
            s.app
                .upload_over_limit_text(until, frees, used, total)
                .contains("and 50 went out, some through other apps")
        );

        // Stopped while it waits, it resumes and goes when there is room.
        s.app.cancel_job(job).unwrap();
        assert_eq!(state(&s), UploadState::Stopped);
        s.app.resume_upload(job).unwrap();
        done(&s.app, job);
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(*s.h.reels.files.lock().unwrap(), [reel_file(true)]);
    }

    #[test]
    fn a_reel_refused_over_the_limit_when_publishing_waits_for_the_next_read() {
        let s = ready();
        s.h.reels.publish_answer(Err(UploadErrorKind::UploadLimit));
        let job = start(&s, review(&s).choices());
        wait_until("held", || {
            matches!(state(&s), UploadState::OverLimit { .. })
        });
        let UploadState::OverLimit { used, total, .. } = state(&s) else {
            unreachable!()
        };
        assert_eq!((used, total), (None, None));
        assert_eq!(s.h.reels.files.lock().unwrap().len(), 1, "the file went");
        assert_eq!(
            upload(&s).upload().unwrap().status,
            UploadStatus::Processing
        );
        s.app.cancel_job(job).unwrap();
        s.app.resume_upload(job).unwrap();
        done(&s.app, job);
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(s.h.reels.files.lock().unwrap().len(), 1, "not sent again");
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 2);
    }

    #[test]
    fn an_expired_container_sends_the_file_again_and_then_publishes() {
        let s = ready();
        s.h.reels.answer(Ok(VideoState::Expired));
        let job = done(&s.app, start(&s, review(&s).choices()));

        assert_eq!(
            *s.h.reels.files.lock().unwrap(),
            [reel_file(true), reel_file(true)]
        );
        let size = reel_file(true).len() as u64;
        let starts = s.h.reels.chunk_starts();
        assert_eq!(starts.first(), Some(&0));
        assert_eq!(starts.iter().filter(|at| **at == 0).count(), 2);
        assert!(starts.iter().all(|at| *at < size));
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 1);
        assert_eq!(job.attempts(), 1, "starting over is not a failed attempt");
        assert!(job.failure().is_none());
        assert_eq!(state(&s), UploadState::Published);
    }

    #[test]
    fn a_reel_instagram_keeps_dropping_goes_again_twice_and_then_fails() {
        let s = ready();
        for _ in 0..=MAX_RESTARTS {
            s.h.reels.answer(Ok(VideoState::Expired));
        }
        let job = wait_done(&s.app, start(&s, review(&s).choices()));

        assert_eq!(
            s.h.reels.files.lock().unwrap().len(),
            1 + MAX_RESTARTS as usize
        );
        assert!(job.failure().is_some(), "{:?}", job.state());
        assert!(s.h.reels.published.lock().unwrap().is_empty());
    }

    #[test]
    fn what_instagram_published_without_shows_on_the_publication() {
        let s = ready();
        s.h.reels.publish_answer(Ok(NetworkPost {
            id: "17900000000000001".into(),
            link: None,
            issue: Some("The audio could not be added to the reel".into()),
        }));
        done(&s.app, start(&s, review(&s).choices()));

        let publication = upload(&s);
        assert_eq!(
            publication.upload().unwrap().issue.as_deref(),
            Some("The audio could not be added to the reel")
        );
        assert_eq!(publication.link, None, "made, without a known address");
        assert_eq!(
            publication.upload().unwrap().network_id.as_deref(),
            Some("17900000000000001"),
            "its media id is kept to read its insights"
        );
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(
            s.app
                .upload_issue_text("The audio could not be added to the reel"),
            "Instagram published the Reel with a warning: The audio could not be added to the reel"
        );
    }

    #[test]
    fn a_reel_not_ready_to_publish_is_checked_again_first() {
        let s = ready();
        s.h.reels.publish_answer(Err(UploadErrorKind::NotReady));
        done(&s.app, start(&s, review(&s).choices()));
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 2);
        assert_eq!(s.h.reels.checks.lock().unwrap().len(), 2);
        assert_eq!(state(&s), UploadState::Published);
    }

    #[test]
    fn a_limit_bardo_may_not_read_does_not_hold_the_reel() {
        let s = ready();
        s.h.reels.limit(Err(UploadErrorKind::NotAllowed));
        done(&s.app, start(&s, review(&s).choices()));
        assert_eq!(state(&s), UploadState::Published);
    }

    #[test]
    fn a_reel_still_processing_is_checked_again_on_its_container() {
        let s = ready();
        let polls = s.h.reels.processing_polls();
        // Still processing after the upload and after the first check.
        for _ in 0..2 * (polls + 1) {
            s.h.reels.answer(Ok(VideoState::Processing));
        }
        done(&s.app, start(&s, review(&s).choices()));
        assert_eq!(state(&s), UploadState::StillProcessing);

        let check = s.app.check_upload(upload(&s).id).unwrap();
        done(&s.app, check);
        assert_eq!(state(&s), UploadState::StillProcessing);
        let check = s.app.check_upload(upload(&s).id).unwrap();
        done(&s.app, check);
        assert_eq!(state(&s), UploadState::Published);
        let checks = s.h.reels.checks.lock().unwrap().clone();
        assert!(checks.iter().all(|id| *id == checks[0]), "{checks:?}");
        assert_eq!(s.h.reels.files.lock().unwrap().len(), 1, "not sent again");
    }

    #[test]
    fn a_reel_never_goes_to_another_account_than_the_one_reviewed() {
        let s = ready();
        s.h.reels.will(Attempt::HoldAfter(1));
        let job = start(&s, review(&s).choices());
        wait_until("held", || {
            s.h.reels.holding.load(std::sync::atomic::Ordering::SeqCst)
        });
        s.app.cancel_job(job).unwrap();
        wait_done(&s.app, job);

        connect_as(&s, "17841400000000001", "@another");
        s.app.resume_upload(job).unwrap();
        wait_done(&s.app, job);
        assert_eq!(
            upload(&s).upload().unwrap().status,
            UploadStatus::Failed(UploadFailure::AccountChanged)
        );
        assert_eq!(s.h.reels.chunk_starts(), [0], "nothing more sent");
        assert!(s.h.reels.published.lock().unwrap().is_empty());
    }
}
