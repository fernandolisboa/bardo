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

use std::io::Read;
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    Job, JobFailure, JobFailureKind, JobId, JobKind, JobState, MetadataProblem, Network,
    NetworkAccount, NetworkAccountId, NetworkAccountRepository, Post, PostLink, Progress,
    ProjectFiles, Publication, PublicationId, PublicationKind, PublicationRepository,
    RenderRepository, RepositoryError, ScheduleProblem, SecretText, SignInFailureKind, Upload,
    UploadError, UploadErrorKind, UploadFailure, UploadOutcome, UploadRun, UploadStatus,
    VideoProjectId, VideoState, VideoUpload, VideoUploader, Visibility, check_publish_time,
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
    /// No metadata generated yet.
    NoMetadata,
    /// The metadata breaks the network's rules.
    Problems,
}

impl UploadBlock {
    pub const ALL: [UploadBlock; 9] = [
        UploadBlock::NotOffered,
        UploadBlock::NotConnected,
        UploadBlock::ReconnectNeeded,
        UploadBlock::Uploading,
        UploadBlock::Rendering,
        UploadBlock::NoRender,
        UploadBlock::RenderOutdated,
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
            ConnectionState::NotConnected | ConnectionState::Connecting => {
                UploadBlock::NotConnected
            }
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
        }
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
            UploadReviewError::NotNow
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
    /// The network was still processing the video when Bardo stopped
    /// waiting for it: check again later.
    StillProcessing,
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
    match job.state() {
        JobState::Running if *status == UploadStatus::Processing => UploadState::Processing,
        JobState::Running => UploadState::Uploading(job.progress()),
        JobState::Queued if job.failure().is_some() => UploadState::Retrying,
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
        })
    }
}

fn from_millis(millis: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(millis)
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
        UploadErrorKind::Invalid | UploadErrorKind::Unexpected => (
            JobFailureKind::UnexpectedAnswer,
            Some(UploadFailure::Job(JobFailureKind::UnexpectedAnswer)),
        ),
        UploadErrorKind::Local => (
            JobFailureKind::Media,
            Some(UploadFailure::Job(JobFailureKind::Media)),
        ),
        // YouTube may not schedule a time already past: the user reviews
        // the upload again with a new time.
        UploadErrorKind::Late => (
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
        Err(ConnectionError::SignIn(failure))
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

/// How many times an upload job checks on a video the network is
/// processing before it stops waiting: about an hour with YouTube's
/// `poll_delay`.
const PROCESSING_POLLS: u32 = 16;

/// Runs uploads.
pub(crate) struct UploadHandler {
    pub(crate) publications: Arc<dyn PublicationRepository>,
    pub(crate) accounts: Arc<dyn NetworkAccountRepository>,
    pub(crate) renders: Arc<dyn RenderRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) connections: Connections,
    pub(crate) uploaders: Vec<Arc<dyn VideoUploader>>,
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
        self.cx.external_handle().map(str::to_owned)
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
                run.cx
                    .save_checkpoint(to_json(&checkpoint), sending_progress(9, 10))
                    .map_err(JobRun::local)?;
                Ok(Some(checkpoint))
            }
        }
    }
}

/// The network's address for an uploaded video.
fn video_link(network: Network, id: &str) -> Option<PostLink> {
    match network {
        Network::YouTube => {
            PostLink::parse(network, &format!("https://www.youtube.com/watch?v={id}")).ok()
        }
        _ => None,
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
        let mut checkpoint: UploadCheckpoint = match cx.checkpoint() {
            Some(text) => parse(text)?,
            None => UploadCheckpoint::default(),
        };
        checkpoint.video = checkpoint.video.or_else(|| payload.video.clone());

        if checkpoint.video.is_none() {
            let project: VideoProjectId = id(&payload.project)?;
            if !self.render_unchanged(project, &payload)? {
                // Not retryable: the user reviews the new render.
                return self.update(publication_id, job, |p| {
                    p.upload_mut()
                        .map_or(Ok(()), |upload| upload.fail(UploadFailure::RenderChanged))
                });
            }
            self.update(publication_id, job, |p| {
                p.upload_mut().map_or(Ok(()), Upload::start)
            })?;
            match self.send(cx, &payload, &account, uploader.as_ref(), checkpoint) {
                Ok(Some(sent)) => checkpoint = sent,
                // Stopped: the session and the bytes are saved.
                Ok(None) => return Ok(()),
                Err(_) if cx.should_stop() => return Ok(()),
                Err(error) => return Err(self.failed(publication_id, job, &error, network)),
            }
        }
        let video = checkpoint.video.clone().expect("sent above or before");
        let link = video_link(network, &video).ok_or_else(|| {
            let error = UploadError::new(
                UploadErrorKind::Unexpected,
                format!("the network answered an unknown video id {video:?}"),
            );
            self.failed(publication_id, job, &error, network)
        })?;
        self.update(publication_id, job, |p| p.sent(link))?;

        // Wait for the network to process it, for a while: the job holds a
        // place in the queue meanwhile. Past that the video shows as still
        // processing, and the user checks again later.
        let mut polls = 0;
        loop {
            if cx.should_stop() {
                return Ok(());
            }
            let state = access_token(&self.connections, &account)
                .and_then(|token| uploader.state(&token, &video));
            let outcome = match state {
                Ok(VideoState::Processing) => {
                    if polls >= PROCESSING_POLLS || !cx.sleep(uploader.poll_delay(polls)) {
                        return Ok(());
                    }
                    polls += 1;
                    continue;
                }
                Ok(VideoState::Ready {
                    visibility,
                    publish_at,
                    ..
                }) => {
                    let now = SystemTime::now();
                    return self.update(publication_id, job, |p| {
                        p.processed(visibility, publish_at, now)
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
        Some(upload_state(upload, self.job(upload.job).as_ref()))
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
            UploadFailure::Job(kind) => self.text(Text::JobFailureKindName(*kind)).into_owned(),
        }
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
        let stamp = format!(
            "{}|{}|{}|{}|{synthetic}|{channel}",
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
            stamp,
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
            check_publish_time(at, SystemTime::now()).map_err(UploadReviewError::Schedule)?;
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
            made_for_kids: choices.made_for_kids,
            synthetic: choices.synthetic,
            video: None,
            publish_at: choices.publish_at.map(to_millis),
        };
        let job = Job::new(self.profile.id, JobKind::Upload, to_json(&payload));
        let upload = match choices.publish_at {
            Some(at) => Upload::scheduled(from_millis(to_millis(at)), job.id()),
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
        let video = publication.post_id().map(str::to_owned);
        let Some(upload) = publication.upload_mut() else {
            return Err(UploadReviewError::NotNow);
        };
        let (old, mut payload) = self.upload_job(upload.job)?;
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
        Network, ScheduleChange, ScheduleOutcome, SecretText, UploadError, UploadErrorKind,
        UploadOutcome, UploadRun, UploadedVideo, VideoState, VideoUpload, VideoUploader,
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
    /// knows resumes where it stopped, as YouTube's do.
    #[derive(Default)]
    pub(crate) struct FakeUploader {
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
    }

    impl FakeUploader {
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
            Network::YouTube
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
                Some(Ok(state)) => Ok(state),
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
    const ACCESS: &str = "ya29.upload-access";

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
        let error = UploadError::new(UploadErrorKind::Late, "late");
        let (job, failure) = failure_of(&error, Network::YouTube);
        assert_eq!(failure, Some(UploadFailure::ScheduleMissed));
        assert!(!job.kind.is_transient(), "the user reviews it again");
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
        for _ in 0..=PROCESSING_POLLS {
            s.h.uploader.answer(Ok(VideoState::Processing));
        }
        let review = review(&s);
        let first = start(&s, review.choices());
        done(&s.app, first);
        assert_eq!(
            s.h.uploader.checks.lock().unwrap().len(),
            PROCESSING_POLLS as usize + 1
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
