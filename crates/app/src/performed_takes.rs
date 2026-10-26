//! Performed takes (#192, #194, docs/spec/performance-capture.md): once a
//! take's control video is approved, a take model on Higgsfield or on
//! BytePlus ModelArk makes the persona perform it.
//!
//! - **Take model.** A channel picks the model its takes go to; any take can
//!   pick another. A take's mode must be one its model takes.
//! - **Prompt.** Claude writes it from the take prompt template: the look,
//!   the set (one frame of the control video when the set is the room),
//!   the take's words and the instruction its mode needs. The user edits
//!   it. It holds for the control video and model it was written for.
//! - **Draft and final.** Where the model has a cheap tier, a draft comes
//!   first and is approved before the final. Each is a paid job: the
//!   control video and the look (always re-encoded as a JPEG, which drops
//!   a photo's metadata) are staged, the provider's quote is asked, and
//!   the request id is saved before any waiting, so a restart (or a retry
//!   of a cancelled job) polls the same request instead of paying again.
//!   Cancelling stops the waiting, not the provider.
//! - **Drafts finished (ModelArk).** Where a model's final finishes its
//!   draft, the final names only the draft's task, within the draft's
//!   deadline; past it, only a new draft can be made. ModelArk takes no
//!   idempotency key: the send time is saved first, and a send whose
//!   answer was lost is looked for in the task list before sending again.
//! - **Staged files (ModelArk).** The files go to the user's own bucket and
//!   stay there until Bardo deletes them: once the final is made, the
//!   draft is replaced, the job fails, the capture goes, or at the
//!   deadline. A look the provider's asset library holds goes by its id;
//!   a photo of the user needs its verified likeness there.
//! - **Use.** A reviewed final is copied into the project's media as a
//!   performed clip, for the editor's media bin.
//!
//! Nothing is resent in another mode, with another look or prompt, on its
//! own: a refusal ends the job and says so.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, AssetSource, Capture, CaptureId, CaptureRepository, ChannelId, ChannelRepository,
    ClipHandle, ClipModelRef, CostPurpose, DEFAULT_TAKE_CAP, FaceLandmarks, ImageFormat, Job,
    JobFailure, JobFailureKind, JobId, JobKind, JobRepository, JobState, LookAssetKind, LookFace,
    LookSource, MAX_TAKE_PROMPT, MIN_TAKE_CAP, MediaAsset, MediaAssetId, MediaAssetRepository,
    MediaKind, Metered, Money, Outbound, PerformedClip, PersonaId, PersonaLook, PersonaRepository,
    PictureSize, Preflight, PreflightOverride, ProfileId, ProjectFiles, Provider, ProviderFailure,
    ProviderFailureKind, RepositoryError, SecretStore, StagedFile, StagedUpload, StagingRepository,
    TakeGenerator, TakeInput, TakeMode, TakeModel, TakePrompt, TakePromptError, TakeRequest,
    TakeResult, TakeSpend, TakeStatus, TakeStep, TakeStretch, TemplateKind, TemplateUsed,
    TemplateValues, TemplateVariable, TemplateVersionId, TextFormat, TextGenerator, TextImage,
    TextRequest, ThemeRepository, VideoProjectId, preflight, take_model_of, take_prompt_text,
};
use bardo_media::MediaEngine;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::captures::CaptureError;
use crate::control_videos::{ControlPlayer, ControlVideoView};
use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::publications::whole_millis;
use crate::scenes::{id, parse, to_json, unexpected};
use crate::{Bardo, KeyState};

/// How many status checks in a row may fail on the provider's side before
/// the job gives up for now (the queue retries it later, polling the same
/// request).
const MAX_HICCUPS: u32 = 10;

/// What a refusal on Higgsfield says: it gives no reason.
pub const REFUSED: &str = "Higgsfield refused the content.";

/// How long staged files that were never sent stay usable for a first
/// send: older ones are staged again, so a draft's deadline (counted from
/// staging) starts close to its send.
const RESTAGE_AFTER: Duration = Duration::from_secs(60 * 60);

/// How long a staged file is kept for a model whose drafts have no
/// deadline.
const STAGED_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// The largest frame sent to Claude, in bytes.
const MAX_FRAME_BYTES: u64 = 4_000_000;

/// `{{instruction}}` for a take in `mode`: what the video model does with
/// the control video and the look.
pub(crate) fn instruction(mode: TakeMode) -> &'static str {
    match mode {
        TakeMode::Recording => {
            "Animate the character in the reference image with the motion, gestures, expressions \
             and lip movements of the person in the reference video. Keep the character's face, \
             hair, build and clothes exactly as in the image."
        }
        TakeMode::Mesh => {
            "Replace the person in the video, whose face is covered by a drawn face mesh, with \
             the character in the reference image. Keep the motion, head turns, expressions and \
             lip movements of the video exactly, and show the character's own face where the \
             mesh is."
        }
        TakeMode::Depth => {
            "Replace the colored depth figure in the video with the character in the reference \
             image. Keep the figure's motion, build, head turns, expressions and lip movements \
             exactly."
        }
    }
}

/// `{{set}}` when the set is the recording's room: a frame goes with it.
const SET_FROM_FRAME: &str = "the place shown in the attached frame of the control video: \
describe its room, light and colors, never anyone in it.";

/// `{{set}}` over a set image: it is in the control video already.
const SET_FROM_IMAGE: &str = "the background already in the control video, kept as it is.";

/// `{{look}}` for an imported photo: it has no description.
const LOOK_FROM_PHOTO: &str = "the person in the reference image.";

/// Why a take cannot go to its take model yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformedBlock {
    /// No take model is offered.
    NoModel,
    /// The take's mode is not one its model takes: pick one.
    ModeNotTaken,
    /// The take is shorter or longer than its model makes.
    Length,
    /// The persona that performs has no look.
    NoLook,
    /// The look is a photo of the user, and the model needs its verified
    /// likeness from the provider's asset library.
    NoVerifiedLikeness,
    /// The take's control video is not approved as it is now.
    ControlNotApproved,
    /// The model reads the control video from a link, and no storage it
    /// can be staged in has a key saved (BytePlus or Higgsfield).
    NoVideoHost,
}

impl PerformedBlock {
    pub const ALL: [PerformedBlock; 7] = [
        PerformedBlock::NoModel,
        PerformedBlock::ModeNotTaken,
        PerformedBlock::Length,
        PerformedBlock::NoLook,
        PerformedBlock::NoVerifiedLikeness,
        PerformedBlock::ControlNotApproved,
        PerformedBlock::NoVideoHost,
    ];

    /// A stable name, for the interface's texts.
    pub fn code(self) -> &'static str {
        match self {
            PerformedBlock::NoModel => "no_model",
            PerformedBlock::ModeNotTaken => "mode_not_taken",
            PerformedBlock::Length => "length",
            PerformedBlock::NoLook => "no_look",
            PerformedBlock::NoVerifiedLikeness => "no_verified_likeness",
            PerformedBlock::ControlNotApproved => "control_not_approved",
            PerformedBlock::NoVideoHost => "no_video_host",
        }
    }
}

/// The next paid step of a take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformedStep {
    /// Claude writes its prompt.
    Prompt,
    /// The take model makes it at `TakeStep`.
    Make(TakeStep),
}

/// A take's performed clip work, for the Capture stage.
#[derive(Debug, Clone, PartialEq)]
pub struct PerformedTakeView {
    /// The take model it goes to; `None` when none is offered.
    pub model: Option<TakeModel>,
    /// Whether it follows the channel's model.
    pub follows_channel: bool,
    /// Why nothing can be made yet, if anything.
    pub block: Option<PerformedBlock>,
    /// Its prompt, when written for its control video and model as they
    /// are now.
    pub prompt: Option<TakePrompt>,
    /// Its draft and final, when made from its control video and model as
    /// they are now, and on disk.
    pub draft: Option<TakeResult>,
    pub final_take: Option<TakeResult>,
    /// Whether its draft is past its deadline: it can no longer be
    /// finished, only made again.
    pub expired: bool,
    /// Whether its draft already has the final that finishes it: the same
    /// draft would make the same final, so only a new draft goes on.
    pub finished: bool,
    /// Its latest prompt job, and its latest draft or final job with the
    /// step it makes: one under way, else one made for its control video
    /// and model as they are now.
    pub prompt_job: Option<Job>,
    pub job: Option<(Job, TakeStep)>,
    /// Whether a prompt or a performed take of it is being made.
    pub busy: bool,
    /// Whether its latest draft or final job stopped (or failed) after
    /// the provider took the request: checking again polls it, and nothing
    /// new is paid for until then.
    pub pending: bool,
    /// The next paid step, and what it would cost.
    pub next: PerformedStep,
    pub estimate: Option<SpendEstimate>,
    /// Where the next step is a draft the model does not require: what a
    /// final made without it would cost.
    pub skip_draft: Option<SpendEstimate>,
    /// The free checks of the take against its model and the look, once
    /// it has a model; and whether the user chose to send it anyway, for
    /// its approved control video.
    pub preflight: Option<Preflight>,
    pub overridden: bool,
    /// The take's length.
    pub length: Duration,
    /// How many performed clips the take's finals became.
    pub used: usize,
}

impl PerformedTakeView {
    /// Whether a prompt or a performed take is being made.
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// The draft waiting for review: made and not approved.
    pub fn draft_to_review(&self) -> Option<&TakeResult> {
        self.draft
            .as_ref()
            .filter(|draft| draft.accepted_at.is_none())
    }

    /// Whether the model makes drafts.
    pub fn has_drafts(&self) -> bool {
        self.model
            .as_ref()
            .is_some_and(|model| model.draft.is_some())
    }

    /// Whether a failed preflight check holds its paid steps back.
    pub fn preflight_holds(&self) -> bool {
        !self.overridden && self.preflight.as_ref().is_some_and(Preflight::holds)
    }
}

/// The performed clips part of the Capture stage.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PerformedView {
    /// Every take model offered, the default first.
    pub models: Vec<TakeModel>,
    /// The model of takes without their own.
    pub channel_model: Option<TakeModel>,
    /// Whether the persona that performs has a look, and whether it reads
    /// as a real person.
    pub has_look: bool,
    pub realistic: bool,
    /// One per take, in order.
    pub takes: Vec<PerformedTakeView>,
    /// The capture's test take, made for the performer's current look and
    /// on disk; its latest job; the take a new test would be made from,
    /// with what it would cost.
    pub test: Option<TakeResult>,
    pub test_job: Option<Job>,
    pub test_pending: bool,
    pub test_take: Option<usize>,
    pub test_estimate: Option<SpendEstimate>,
    /// Whether drafts and finals wait for an approved test: none was made
    /// of this capture yet.
    pub needs_test: bool,
    /// What the video's takes cost so far and will cost, against its cap.
    pub spend: TakeSpend,
    /// Whether the cap is the user's own (else the default).
    pub own_cap: bool,
    /// How the face in the look was measured, for the framing check.
    pub look_face: LookFace,
}

impl PerformedView {
    /// Whether the test take waits for review.
    pub fn test_to_review(&self) -> bool {
        self.test
            .as_ref()
            .is_some_and(|test| test.accepted_at.is_none())
    }

    /// Whether a test of this capture's takes is being made.
    pub fn test_busy(&self) -> bool {
        self.test_job
            .as_ref()
            .is_some_and(|job| job.state().is_active())
    }
}

/// A prompt job's payload: the take, the control video and model it is
/// written for, and the rendered template (its estimate was made from it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PromptPayload {
    project: String,
    capture: String,
    position: u32,
    control: String,
    control_file: String,
    provider: String,
    model: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
    /// Where the frame Claude describes is, in milliseconds into the
    /// control video; `None` sends no frame.
    frame_at_ms: Option<u64>,
}

/// A draft or final job's payload, fixed when the user pressed the button
/// (the estimate and budget were checked against it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TakePayload {
    project: String,
    capture: String,
    position: u32,
    step: String,
    control: String,
    control_file: String,
    provider: String,
    model: String,
    priced_model: String,
    resolution: Option<String>,
    prompt: String,
    template: Option<String>,
    template_number: Option<u32>,
    persona: String,
    /// The take's length in whole seconds, as estimated.
    seconds: u32,
    /// For a final that finishes its draft: the draft's request, job,
    /// look and realism, and until when it can be finished (Unix time in
    /// milliseconds). Nothing is staged for it.
    #[serde(default)]
    draft_task: Option<String>,
    #[serde(default)]
    draft_job: Option<String>,
    #[serde(default)]
    draft_look: Option<String>,
    #[serde(default)]
    draft_realistic: bool,
    #[serde(default)]
    draft_deadline: Option<u64>,
    /// For a test: how much of the control video's start it sends.
    #[serde(default)]
    piece: Option<TestPiece>,
}

/// The start of a control video a test sends (#243), with how the control
/// video is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct TestPiece {
    /// Milliseconds.
    length: u64,
    width: u32,
    height: u32,
    fps_milli: u32,
}

/// Picks the take out of a prompt or take job's payload.
#[derive(Deserialize)]
struct TakeOf {
    project: String,
    capture: String,
    position: u32,
    #[serde(default)]
    step: Option<String>,
    control: String,
    provider: String,
    model: String,
}

impl TakeOf {
    /// Whether the job was made for `control` and `model`.
    fn made_for(&self, control: Option<&str>, model: Option<&TakeModel>) -> bool {
        control == Some(self.control.as_str())
            && model.is_some_and(|model| {
                model.id.provider().code() == self.provider && model.id.model() == self.model
            })
    }
}

/// What a take job sent, saved as its external handle after every change:
/// a restart reads it to poll instead of submitting again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TakeState {
    /// Counts the failed submissions of this job. Part of the idempotency
    /// key: resending the same submission (after a crash right after
    /// sending it) returns the first one instead of a second take.
    restarts: u32,
    /// The control video and the look as Higgsfield refers to them.
    video: Option<String>,
    look: Option<String>,
    /// A fingerprint of the look sent, and whether it reads as a real
    /// person.
    look_print: Option<String>,
    realistic: bool,
    /// Whether Higgsfield's quote was asked, and what it said, in
    /// micro-dollars.
    quoted: bool,
    quote_micros: Option<u64>,
    /// Higgsfield's request id, once submitted.
    request: Option<String>,
    /// Whether the take's cost is in the cost book, so an attempt after a
    /// failed save does not count it again.
    #[serde(default)]
    recorded: bool,
    /// When the first file was staged, and when the submission was about to
    /// be sent to a provider without an idempotency key (Unix time in
    /// milliseconds): an attempt that finds it set looks for the request
    /// before sending again.
    #[serde(default)]
    staged_at: Option<u64>,
    #[serde(default)]
    sent_at: Option<u64>,
    /// For a model that reads the control video from a link: the provider
    /// it is staged with.
    #[serde(default)]
    host: Option<String>,
}

/// Whether a take job's saved state holds a request the provider took,
/// or one it may have taken: sent to a provider without an idempotency
/// key and its answer lost. Either is checked again, never sent anew.
fn has_request(job: &Job) -> bool {
    job.external_handle()
        .and_then(|saved| serde_json::from_str::<TakeState>(saved).ok())
        .is_some_and(|state| state.request.is_some() || state.sent_at.is_some())
}

impl TakeState {
    /// Forgets the submission, so the next attempt starts a new one.
    fn restart(&mut self) {
        *self = TakeState {
            restarts: self.restarts + 1,
            ..TakeState::default()
        };
    }

    /// Whether a submission that failed with `kind` surely made nothing,
    /// so the next attempt may be a new one. Any other failure (an answer
    /// that could not be read, the provider down) resends the same
    /// submission, which Higgsfield answers with the first one (ModelArk's
    /// task list is looked at first).
    fn refused(kind: JobFailureKind) -> bool {
        matches!(
            kind,
            JobFailureKind::Declined
                | JobFailureKind::KeyRejected
                | JobFailureKind::NotAllowed
                | JobFailureKind::LimitReached
                | JobFailureKind::MissingKey
        )
    }
}

/// The shape Claude answers in.
const PROMPT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "prompt": {"type": "string"}
  },
  "required": ["prompt"],
  "additionalProperties": false
}"#;

#[derive(Deserialize)]
struct Answer {
    prompt: String,
}

/// A draft's or final's file: the capture, the take (counted from 1), the
/// step and the job, so two results never share a file.
fn take_file(capture: CaptureId, position: u32, step: TakeStep, job: JobId) -> String {
    let job = job.to_string();
    format!(
        "take-{capture}-{:03}-{}-{}.mp4",
        position + 1,
        step.code(),
        &job[..8.min(job.len())]
    )
}

/// `at` in milliseconds since the Unix epoch, as job states keep times.
fn unix_millis(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

fn from_unix_millis(millis: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
}

/// The take's length in whole seconds, rounded up: what it is billed by.
fn billed_seconds(length: Duration) -> u32 {
    length.as_millis().div_ceil(1000) as u32
}

/// `text` with its whitespace collapsed, cut at the prompt's limit on a
/// word.
fn fit_prompt(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= MAX_TAKE_PROMPT {
        return text;
    }
    let cut: String = text.chars().take(MAX_TAKE_PROMPT).collect();
    match cut.rfind(' ') {
        Some(space) => cut[..space].to_owned(),
        None => cut,
    }
}

/// A short fingerprint of a look's bytes.
pub(crate) fn look_print(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn key_of(
    secrets: &dyn SecretStore,
    owner: ProfileId,
    provider: Provider,
) -> Result<ApiKey, JobFailure> {
    secrets
        .get(owner, provider)
        .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
        .ok_or_else(|| {
            JobFailure::new(
                JobFailureKind::MissingKey,
                format!("no {} key is saved", provider.code()),
            )
        })
}

/// How a take provider is named in failures.
fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Higgsfield => "Higgsfield",
        Provider::BytePlus => "BytePlus",
        other => other.code(),
    }
}

fn provider_failure(provider: Provider, failure: ProviderFailure) -> JobFailure {
    let kind = JobFailureKind::from(failure.kind);
    match (kind, provider) {
        (JobFailureKind::Declined, Provider::Higgsfield) => JobFailure::new(kind, REFUSED),
        // ModelArk says which rule and why, in its own words: the take
        // names the provider beside them.
        (JobFailureKind::Declined, _) => JobFailure::new(kind, failure.detail),
        _ => JobFailure::new(
            kind,
            format!("{}: {}", provider_name(provider), failure.detail),
        ),
    }
}

/// Deletes the staged files that are due (given up or past their time)
/// with the take generator that staged them, and forgets them. Files of a
/// provider without a saved key wait. Returns the first failure that may
/// pass, so a cleanup job tries again; the others are logged.
pub(crate) fn sweep_staged(
    owner: ProfileId,
    staging: &dyn StagingRepository,
    generators: &[Arc<dyn TakeGenerator>],
    secrets: &dyn SecretStore,
    now: SystemTime,
) -> Result<(), JobFailure> {
    let due = staging.staged_due(owner, now).map_err(unexpected)?;
    let mut keys: Vec<(Provider, Option<ApiKey>)> = Vec::new();
    let mut first = None;
    for upload in due {
        let Some(generator) = generators
            .iter()
            .find(|generator| generator.provider() == upload.provider && generator.keeps_staged())
        else {
            // Nothing can delete it any more.
            staging.remove_staged(&upload.file).map_err(unexpected)?;
            continue;
        };
        let key = match keys
            .iter()
            .find(|(provider, _)| *provider == upload.provider)
        {
            Some((_, key)) => key.clone(),
            None => {
                let key = secrets.get(owner, upload.provider).ok().flatten();
                keys.push((upload.provider, key.clone()));
                key
            }
        };
        let Some(key) = key else {
            continue;
        };
        match generator.unstage(&key, &StagedFile(upload.file.clone())) {
            Ok(()) => staging.remove_staged(&upload.file).map_err(unexpected)?,
            // Not in the bucket the saved key names (it changed): the
            // bucket's own expiry rule removes it.
            Err(failure) if failure.kind == ProviderFailureKind::NotAllowed => {
                tracing::warn!(detail = %failure.detail, "a staged file cannot be deleted");
                staging.remove_staged(&upload.file).map_err(unexpected)?;
            }
            Err(failure) => {
                tracing::warn!(detail = %failure.detail, "a staged file was not deleted yet");
                let failure = provider_failure(upload.provider, failure);
                if first.is_none() && failure.kind.is_transient() {
                    first = Some(failure);
                }
            }
        }
    }
    first.map_or(Ok(()), Err)
}

/// Deletes staged files that are due, at startup and after a capture
/// goes.
pub(crate) struct StagingCleanupHandler {
    pub(crate) owner: ProfileId,
    pub(crate) staging: Arc<dyn StagingRepository>,
    pub(crate) generators: Vec<Arc<dyn TakeGenerator>>,
    pub(crate) secrets: Arc<dyn SecretStore>,
}

impl JobHandler for StagingCleanupHandler {
    fn run(&self, _payload: &str, _cx: &mut JobContext) -> Result<(), JobFailure> {
        sweep_staged(
            self.owner,
            self.staging.as_ref(),
            &self.generators,
            self.secrets.as_ref(),
            SystemTime::now(),
        )
    }
}

/// Whether take `position` of capture `capture`, the project's, still has
/// control video `control` in `file`, approved and on disk.
fn control_approved(
    captures: &dyn CaptureRepository,
    files: &dyn ProjectFiles,
    project: VideoProjectId,
    capture: CaptureId,
    position: u32,
    (control, file): (&str, &str),
) -> Result<bool, JobFailure> {
    let current = captures
        .capture(project)
        .map_err(unexpected)?
        .is_some_and(|current| current.id == capture);
    Ok(current
        && captures
            .control_videos(project)
            .map_err(unexpected)?
            .iter()
            .any(|made| {
                made.capture == capture
                    && made.position == position
                    && made.fingerprint == control
                    && made.file == file
                    && made.approved_at.is_some()
                    && files.exists(project, &made.file)
            }))
}

/// A folder only the user can read, removed with everything in it when
/// dropped: the photo of the user and the frame sent pass through it.
fn private_folder() -> Result<tempfile::TempDir, JobFailure> {
    tempfile::Builder::new()
        .prefix("bardo-take-")
        .tempdir()
        .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))
}

/// The failure of a job whose take changed (or went) while it ran.
fn take_changed() -> JobFailure {
    JobFailure::new(
        JobFailureKind::NotAllowed,
        "the take changed while this was made (its control video or model); approve it again",
    )
}

/// Writes take prompts.
pub(crate) struct TakePromptHandler {
    pub(crate) owner: ProfileId,
    pub(crate) captures: Arc<dyn CaptureRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for TakePromptHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: PromptPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let capture: CaptureId = id(&payload.capture)?;
        // An earlier attempt may have saved it and stopped before the queue
        // recorded the job as done.
        if self
            .captures
            .take_prompts(project)
            .map_err(unexpected)?
            .iter()
            .any(|prompt| prompt.position == payload.position && prompt.job == Some(cx.id()))
        {
            return Ok(());
        }
        let key = key_of(self.secrets.as_ref(), self.owner, Provider::Claude)?;
        let approved = || {
            control_approved(
                self.captures.as_ref(),
                self.files.as_ref(),
                project,
                capture,
                payload.position,
                (&payload.control, &payload.control_file),
            )
        };
        // Nothing of a take that changed goes to Claude.
        if !approved()? {
            return Err(take_changed());
        }
        let images = match payload.frame_at_ms {
            Some(at) => vec![self.frame(project, &payload.control_file, at)?],
            None => Vec::new(),
        };
        if cx.should_stop() {
            return Ok(());
        }
        let request = TextRequest {
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            images,
            format: TextFormat::Json {
                schema: PROMPT_SCHEMA.to_owned(),
            },
        };
        let generated = self.text.generate(&key, &request).map_err(|failure| {
            JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
        })?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::TakePrompt,
                usage: generated.usage.into(),
                job: Some(cx.id()),
                reported: None,
            },
            project,
        );
        let answer: Answer = serde_json::from_str(&generated.text).map_err(|e| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Claude: the take prompt is not usable ({e})"),
            )
        })?;
        let text = fit_prompt(&answer.prompt);
        if text.is_empty() {
            return Err(JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                "Claude: the take prompt is empty",
            ));
        }
        // Stopped, or the take changed: the prompt it has (maybe edited
        // since) stays.
        if cx.should_stop() {
            return Ok(());
        }
        if !approved()? {
            return Err(take_changed());
        }
        let provider: Provider = payload.provider.parse().map_err(unexpected)?;
        let prompt = TakePrompt {
            project,
            capture,
            position: payload.position,
            control: payload.control,
            model: ClipModelRef::new(provider, &payload.model).map_err(unexpected)?,
            text,
            template: Some(TemplateUsed {
                id: id::<TemplateVersionId>(&payload.template)?,
                number: payload.template_number,
            }),
            writer: generated.model,
            written_at: whole_millis(SystemTime::now()),
            job: Some(cx.id()),
            edited: false,
        };
        // Not kept when the take is gone.
        self.captures
            .save_take_prompt(&prompt)
            .map_err(unexpected)?;
        Ok(())
    }
}

impl TakePromptHandler {
    /// The control video's frame at `at` milliseconds, as a JPEG.
    fn frame(
        &self,
        project: VideoProjectId,
        control_file: &str,
        at: u64,
    ) -> Result<TextImage, JobFailure> {
        let source = self.files.path(project, control_file);
        let folder = private_folder()?;
        let destination = folder.path().join("frame.jpg");
        let bytes = self
            .media
            .frame_picture(
                &source,
                Duration::from_millis(at),
                &destination,
                MAX_FRAME_BYTES,
            )
            .map_err(|e| JobFailure::new(JobFailureKind::Media, e.to_string()))
            .and_then(|()| {
                std::fs::read(&destination)
                    .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))
            })?;
        Ok(TextImage {
            bytes,
            format: ImageFormat::Jpeg,
        })
    }
}

/// Makes drafts and finals.
pub(crate) struct PerformedTakeHandler {
    pub(crate) owner: ProfileId,
    pub(crate) captures: Arc<dyn CaptureRepository>,
    pub(crate) projects: Arc<dyn ThemeRepository>,
    pub(crate) channels: Arc<dyn ChannelRepository>,
    pub(crate) personas: Arc<dyn PersonaRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) staging: Arc<dyn StagingRepository>,
    /// To tell a request of this job from another take's.
    pub(crate) jobs: Arc<dyn JobRepository>,
    pub(crate) generators: Vec<Arc<dyn TakeGenerator>>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

/// One take job's run.
struct TakeRun<'a> {
    handler: &'a PerformedTakeHandler,
    payload: TakePayload,
    project: VideoProjectId,
    capture: CaptureId,
    step: TakeStep,
    model: TakeModel,
    generator: Arc<dyn TakeGenerator>,
    key: ApiKey,
    state: TakeState,
    /// Where the control video is staged, for a model that reads it from
    /// a link, with its key.
    host: Option<(Arc<dyn TakeGenerator>, ApiKey)>,
    /// The look as it goes inside the request, for a model that takes it
    /// so.
    look_bytes: Option<Vec<u8>>,
}

/// The storage a control video is staged in for a model that reads it from
/// a link (#243): the user's own BytePlus bucket when its key is saved,
/// else Higgsfield's upload; `None` with neither key.
fn video_host(
    generators: &[Arc<dyn TakeGenerator>],
    has_key: impl Fn(Provider) -> bool,
) -> Option<&Arc<dyn TakeGenerator>> {
    [Provider::BytePlus, Provider::Higgsfield]
        .into_iter()
        .filter(|provider| has_key(*provider))
        .find_map(|provider| {
            generators
                .iter()
                .find(|generator| generator.provider() == provider && generator.stages_video())
        })
}

impl PerformedTakeHandler {
    /// Removes `old`, a test another test replaced, with its file and
    /// what its job staged.
    fn drop_test(&self, project: VideoProjectId, old: &TakeResult) -> Result<(), JobFailure> {
        self.captures
            .remove_take_result(project, old.position, TakeStep::Test)
            .map_err(unexpected)?;
        let _ = self.files.remove(project, &old.file);
        if let Some(job) = old.job {
            self.staging
                .release_staged_by(job, SystemTime::now())
                .map_err(unexpected)?;
        }
        Ok(())
    }
}

impl JobHandler for PerformedTakeHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: TakePayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let capture: CaptureId = id(&payload.capture)?;
        let step = TakeStep::from_code(&payload.step)
            .ok_or_else(|| unexpected(format!("unknown take step {}", payload.step)))?;
        let results = self.captures.take_results(project).map_err(unexpected)?;
        if results.iter().any(|made| {
            made.position == payload.position && made.step == step && made.job == Some(cx.id())
        }) {
            // A test saved before the others went finishes replacing them.
            if step == TakeStep::Test {
                for old in results
                    .iter()
                    .filter(|old| old.step == TakeStep::Test && old.position != payload.position)
                {
                    self.drop_test(project, old)?;
                }
            }
            return Ok(());
        }
        let provider: Provider = payload.provider.parse().map_err(unexpected)?;
        let generator = self
            .generators
            .iter()
            .find(|generator| generator.provider() == provider)
            .cloned()
            .ok_or_else(|| unexpected(format!("{provider} makes no takes")))?;
        let model = generator
            .models()
            .into_iter()
            .find(|model| model.id.model() == payload.model)
            .ok_or_else(|| {
                JobFailure::new(
                    JobFailureKind::NotAllowed,
                    format!("the take model {} is no longer offered", payload.model),
                )
            })?;
        let key = key_of(self.secrets.as_ref(), self.owner, provider)?;
        let state: TakeState = match cx.external_handle() {
            Some(saved) => parse(saved)?,
            None => TakeState::default(),
        };
        let host = if generator.stages_video() {
            None
        } else {
            let host = match &state.host {
                // The files are where the first attempt staged them.
                Some(code) => {
                    let staged_with: Provider = code.parse().map_err(unexpected)?;
                    self.generators
                        .iter()
                        .find(|generator| generator.provider() == staged_with)
                        .cloned()
                        .ok_or_else(|| unexpected(format!("{staged_with} stages nothing")))?
                }
                None => video_host(&self.generators, |provider| {
                    matches!(self.secrets.get(self.owner, provider), Ok(Some(_)))
                })
                .cloned()
                .ok_or_else(|| {
                    JobFailure::new(
                        JobFailureKind::MissingKey,
                        format!(
                            "{} reads the control video from a link: save a BytePlus or \
                             Higgsfield key to stage it",
                            provider_name(provider)
                        ),
                    )
                })?,
            };
            match key_of(self.secrets.as_ref(), self.owner, host.provider()) {
                Ok(host_key) => Some((host, host_key)),
                // A take already sent is checked on and downloaded from its
                // own provider; its files expire on their own.
                Err(_) if state.request.is_some() => None,
                Err(failure) => return Err(failure),
            }
        };
        let mut run = TakeRun {
            handler: self,
            payload,
            project,
            capture,
            step,
            model,
            generator,
            key,
            state,
            host,
            look_bytes: None,
        };
        // Files given up earlier go first, and what this run gave up after.
        run.sweep();
        let made = run.run(cx);
        run.sweep();
        made
    }
}

impl TakeRun<'_> {
    fn provider(&self) -> Provider {
        self.generator.provider()
    }

    fn failure(&self, failure: ProviderFailure) -> JobFailure {
        provider_failure(self.provider(), failure)
    }

    fn run(&mut self, cx: &mut JobContext) -> Result<(), JobFailure> {
        if self.state.request.is_none() && !self.submit(cx)? {
            return Ok(());
        }
        let request = ClipHandle(self.state.request.clone().expect("submitted"));
        let (mut polls, mut hiccups) = (0, 0);
        loop {
            if cx.should_stop() {
                return Ok(());
            }
            match self.generator.status(&self.key, &request) {
                Ok(TakeStatus::Queued | TakeStatus::Running) => hiccups = 0,
                Ok(TakeStatus::Done(done)) => {
                    // The take is made and paid: whatever goes wrong now,
                    // the next attempt polls the same request again.
                    match self.generator.download(&self.key, &done.video) {
                        Ok(clip) => return self.save(&clip.bytes, done.usage, done.seed, cx),
                        Err(failure) if JobFailureKind::from(failure.kind).is_transient() => {
                            hiccups += 1;
                            if hiccups >= MAX_HICCUPS {
                                return Err(self.failure(failure));
                            }
                        }
                        Err(failure) => return Err(self.failure(failure)),
                    }
                }
                Ok(TakeStatus::Failed(failure)) => return self.ended(failure, cx),
                Err(failure) if JobFailureKind::from(failure.kind).is_transient() => {
                    hiccups += 1;
                    if hiccups >= MAX_HICCUPS {
                        return Err(self.failure(failure));
                    }
                }
                // The key or the account: retrying polls the same request.
                Err(failure) => return Err(self.failure(failure)),
            }
            if !cx.sleep(self.generator.poll_delay(polls)) {
                return Ok(());
            }
            polls += 1;
        }
    }

    fn save_state(&self, cx: &mut JobContext) -> Result<(), JobFailure> {
        cx.save_external_handle(to_json(&self.state))
            .map_err(unexpected)
    }

    /// Deletes the staged files that are due, where the provider keeps
    /// them. Best effort: what is not deleted now is deleted later.
    fn sweep(&self) {
        let generators: Vec<Arc<dyn TakeGenerator>> = std::iter::once(&self.generator)
            .chain(self.host.as_ref().map(|(host, _)| host))
            .filter(|generator| generator.keeps_staged())
            .cloned()
            .collect();
        if generators.is_empty() {
            return;
        }
        let handler = self.handler;
        if let Err(failure) = sweep_staged(
            handler.owner,
            handler.staging.as_ref(),
            &generators,
            handler.secrets.as_ref(),
            SystemTime::now(),
        ) {
            tracing::warn!(detail = %failure.detail, "staged files were not deleted yet");
        }
    }

    /// Whether files this run stages stay until Bardo deletes them.
    fn keeps_staged(&self) -> bool {
        self.host
            .as_ref()
            .map_or(&self.generator, |(host, _)| host)
            .keeps_staged()
    }

    /// Gives up the files `job` staged: they are deleted at the next sweep.
    fn release(&self, job: JobId) -> Result<(), JobFailure> {
        self.handler
            .staging
            .release_staged_by(job, SystemTime::now())
            .map_err(unexpected)
    }

    /// Forgets the submission, giving up what it staged, so the next
    /// attempt starts a new one.
    fn restart(&mut self, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.release(cx.id())?;
        self.state.restart();
        self.save_state(cx)
    }

    /// The request ended without a take: a retry by the user submits it
    /// again, a new paid request, so the queue never retries it on its
    /// own.
    fn ended(&mut self, failure: ProviderFailure, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.restart(cx)?;
        let failure = self.failure(failure);
        if failure.kind.is_transient() {
            return Err(JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                failure.detail,
            ));
        }
        Err(failure)
    }

    /// Whether the take is still the one the job was made for: its control
    /// video built from the same take, approved and on disk, and the same
    /// take model.
    fn still_approved(&self) -> Result<bool, JobFailure> {
        let handler = self.handler;
        if !control_approved(
            handler.captures.as_ref(),
            handler.files.as_ref(),
            self.project,
            self.capture,
            self.payload.position,
            (&self.payload.control, &self.payload.control_file),
        )? {
            return Ok(false);
        }
        let own = handler
            .captures
            .capture(self.project)
            .map_err(unexpected)?
            .and_then(|capture| {
                capture
                    .takes
                    .as_slice()
                    .get(self.payload.position as usize)
                    .and_then(|take| take.model.clone())
            });
        let channel = match handler.projects.project(self.project).map_err(unexpected)? {
            Some(project) => handler
                .channels
                .get(project.channel)
                .map_err(unexpected)?
                .and_then(|channel| channel.details.take_model().cloned()),
            None => return Ok(false),
        };
        let models: Vec<TakeModel> = handler
            .generators
            .iter()
            .flat_map(|generator| generator.models())
            .collect();
        Ok(take_model_of(&models, channel.as_ref(), own.as_ref())
            .is_some_and(|model| model.id == self.model.id))
    }

    /// Stages what is not staged yet (nothing for a final that finishes
    /// its draft), asks the quote and submits. `false` when stopped before
    /// anything was paid.
    fn submit(&mut self, cx: &mut JobContext) -> Result<bool, JobFailure> {
        // Nothing paid yet: the take must still be the one approved.
        if !self.still_approved()? {
            return Err(take_changed());
        }
        // Past the draft's deadline a final is not sent; one already sent
        // whose answer was lost is still looked for.
        let past_deadline = self.payload.draft_task.is_some()
            && self
                .payload
                .draft_deadline
                .is_some_and(|deadline| SystemTime::now() >= from_unix_millis(deadline));
        let too_late = || {
            JobFailure::new(
                JobFailureKind::NotAllowed,
                "the draft can no longer be finished; make a new draft",
            )
        };
        if past_deadline && self.state.sent_at.is_none() {
            return Err(too_late());
        }
        if self.payload.draft_task.is_some() {
            self.state.look_print = self.payload.draft_look.clone();
            self.state.realistic = self.payload.draft_realistic;
        } else if let Err(failure) = self.stage_inputs(cx) {
            // A retry reuses what was staged; a failure that ends the job
            // lets it go.
            if !failure.kind.is_transient() {
                self.restart(cx)?;
            }
            return Err(failure);
        }
        let request = self.request()?;
        if !self.state.quoted {
            self.state.quote_micros = self.generator.quote(&self.key, &request).map(Money::micros);
            self.state.quoted = true;
            self.save_state(cx)?;
        }
        if cx.should_stop() {
            return Ok(false);
        }
        if !self.generator.idempotent() {
            if let Some(sent) = self.state.sent_at {
                // An earlier attempt may have sent it and lost the answer.
                let found = self
                    .generator
                    .find_submitted(&self.key, &request, from_unix_millis(sent))
                    .map_err(|failure| self.failure(failure))?;
                // Requests another take or job holds are theirs.
                let mut unknown = Vec::new();
                for handle in found {
                    if !self.known_request(&handle, cx.id())? {
                        unknown.push(handle);
                    }
                }
                match unknown.as_slice() {
                    [] if past_deadline => return Err(too_late()),
                    [] => {}
                    [handle] => {
                        self.state.request = Some(handle.0.clone());
                        self.save_state(cx)?;
                        return Ok(true);
                    }
                    _ => {
                        return Err(JobFailure::new(
                            JobFailureKind::UnexpectedAnswer,
                            format!(
                                "{}: several takes were sent at that time, so Bardo sent \
                                 nothing again; look at the provider's task list",
                                provider_name(self.provider())
                            ),
                        ));
                    }
                }
            }
            self.state.sent_at = Some(unix_millis(SystemTime::now()));
            self.save_state(cx)?;
        } else if self.state.sent_at.is_none() {
            // Sent from here on: a lost answer leaves the job pending, so
            // checking again resends the same submission (which the
            // provider recognizes) instead of a new one, and its files stay.
            self.state.sent_at = Some(unix_millis(SystemTime::now()));
            self.save_state(cx)?;
        }
        let submission = format!(
            "{}-{}-{}-{}",
            cx.id(),
            self.payload.position,
            self.step.code(),
            self.state.restarts + 1
        );
        match self.generator.submit(&self.key, &request, &submission) {
            Ok(handle) => {
                self.state.request = Some(handle.0);
                self.save_state(cx)?;
                Ok(true)
            }
            Err(failure) => {
                if TakeState::refused(JobFailureKind::from(failure.kind)) {
                    self.restart(cx)?;
                }
                Err(self.failure(failure))
            }
        }
    }

    /// Stages the control video and the look, unless staged already. Files
    /// staged long ago and never sent are staged again first.
    fn stage_inputs(&mut self, cx: &mut JobContext) -> Result<(), JobFailure> {
        if self.keeps_staged()
            && self.state.request.is_none()
            && self.state.sent_at.is_none()
            && self.state.staged_at.is_some_and(|staged| {
                SystemTime::now()
                    .duration_since(from_unix_millis(staged))
                    .is_ok_and(|age| age > RESTAGE_AFTER)
            })
        {
            self.release(cx.id())?;
            self.state.video = None;
            self.state.look = None;
            self.state.look_print = None;
            self.state.staged_at = None;
            self.save_state(cx)?;
        }
        if self.state.video.is_none() {
            let bytes = self.control_bytes()?;
            if bytes.len() as u64 > self.model.max_video_bytes {
                return Err(JobFailure::new(
                    JobFailureKind::Media,
                    format!(
                        "the control video is over the take model's {} MB",
                        self.model.max_video_bytes / 1_000_000
                    ),
                ));
            }
            let staged = self.stage(&bytes, "video/mp4", true, cx)?;
            self.state.video = Some(staged);
            self.save_state(cx)?;
        }
        if self.generator.inline_look() {
            self.inline_look(cx)?;
        } else if self.state.look.is_none() {
            let persona: PersonaId = id(&self.payload.persona)?;
            let personas = &self.handler.personas;
            let look = personas
                .look(persona)
                .map_err(unexpected)?
                .ok_or_else(|| JobFailure::unexpected("the persona has no look"))?;
            let print = look_print(look.image.bytes());
            let asset = if self.model.look_assets {
                personas
                    .look_asset(persona, self.provider())
                    .map_err(unexpected)?
                    .filter(|asset| asset.holds_for(&look, &print))
            } else {
                None
            };
            let sent = match asset {
                // The provider's own library holds it: nothing to stage.
                Some(asset) => asset.uri(),
                None if self.model.look_assets && look.source == LookSource::Imported => {
                    return Err(JobFailure::new(
                        JobFailureKind::NotAllowed,
                        format!(
                            "{} needs the photo's verified likeness: add its asset id on the \
                             persona's look",
                            provider_name(self.provider())
                        ),
                    ));
                }
                None => {
                    let jpeg = self.fit_look(&look)?;
                    self.stage(&jpeg, "image/jpeg", false, cx)?
                }
            };
            self.state.look = Some(sent);
            self.state.look_print = Some(print);
            self.state.realistic = look.realistic;
            self.save_state(cx)?;
        }
        Ok(())
    }

    /// Hands a file to the provider. One staged in the user's own storage
    /// is listed first, so it is deleted however the job ends.
    /// The control video goes to the run's video host where it has one.
    fn stage(
        &mut self,
        bytes: &[u8],
        mime: &str,
        video: bool,
        cx: &mut JobContext,
    ) -> Result<String, JobFailure> {
        let (generator, key) = match &self.host {
            Some((host, key)) if video => (Arc::clone(host), key.clone()),
            _ => (Arc::clone(&self.generator), self.key.clone()),
        };
        if video && self.host.is_some() && self.state.host.is_none() {
            self.state.host = Some(generator.provider().code().to_owned());
            self.save_state(cx)?;
        }
        let staged = generator
            .stage(&key, TakeInput { bytes, mime })
            .map_err(|failure| provider_failure(generator.provider(), failure))?;
        if generator.keeps_staged() {
            let now = whole_millis(SystemTime::now());
            let listed = self.handler.staging.add_staged(&StagedUpload {
                file: staged.0.clone(),
                owner: self.handler.owner,
                provider: generator.provider(),
                project: self.project,
                capture: self.capture,
                position: self.payload.position,
                job: cx.id(),
                staged_at: now,
                expires_at: now + self.model.draft_lifetime.unwrap_or(STAGED_FOR),
                released_at: None,
            });
            if let Err(error) = listed {
                // Not listed, so nothing would ever delete it.
                let _ = generator.unstage(&key, &staged);
                return Err(unexpected(error));
            }
            if self.state.staged_at.is_none() {
                self.state.staged_at = Some(unix_millis(now));
            }
        }
        Ok(staged.0)
    }

    /// Whether `handle` is the request of another take: a result of the
    /// project, or another take job's.
    fn known_request(&self, handle: &ClipHandle, job: JobId) -> Result<bool, JobFailure> {
        let handler = self.handler;
        if handler
            .captures
            .take_results(self.project)
            .map_err(unexpected)?
            .iter()
            .any(|made| made.request == handle.0)
        {
            return Ok(true);
        }
        Ok(handler
            .jobs
            .list(handler.owner)
            .map_err(unexpected)?
            .iter()
            .filter(|other| other.kind() == JobKind::PerformedTake && other.id() != job)
            .filter_map(|other| other.external_handle())
            .filter_map(|saved| serde_json::from_str::<TakeState>(saved).ok())
            .any(|state| state.request.as_deref() == Some(handle.0.as_str())))
    }

    /// What goes to the provider: a test is sent as the step the model
    /// makes it at, and a control video staged elsewhere goes by a link
    /// signed now.
    fn request(&self) -> Result<TakeRequest, JobFailure> {
        let video = StagedFile(self.state.video.clone().unwrap_or_default());
        let video_link = match &self.host {
            Some((host, key)) if self.payload.draft_task.is_none() => Some(
                host.link(key, &video)
                    .map_err(|failure| provider_failure(host.provider(), failure))?,
            ),
            _ => None,
        };
        Ok(TakeRequest {
            model: self.payload.model.clone(),
            step: self.model.sent_step(self.step),
            resolution: self.payload.resolution.clone(),
            prompt: self.payload.prompt.clone(),
            video,
            look: StagedFile(self.state.look.clone().unwrap_or_default()),
            draft_task: self.payload.draft_task.clone().map(ClipHandle),
            video_link,
            look_bytes: self.look_bytes.clone(),
        })
    }

    /// The control video, or its first seconds for a test.
    fn control_bytes(&self) -> Result<Vec<u8>, JobFailure> {
        let files = &self.handler.files;
        let Some(piece) = self.payload.piece else {
            return files
                .read(self.project, &self.payload.control_file)
                .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()));
        };
        let folder = private_folder()?;
        let destination = folder.path().join("test.mp4");
        crate::control_videos::cut_piece(
            self.handler.media.as_ref(),
            &files.path(self.project, &self.payload.control_file),
            (piece.width, piece.height, piece.fps_milli),
            Duration::from_millis(piece.length),
            &destination,
        )
        .map_err(|e| JobFailure::new(JobFailureKind::Media, e.to_string()))?;
        std::fs::read(&destination)
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))
    }

    /// The look, for a model that takes it inside the request: re-encoded
    /// as for staging, named by the look's print so a request sent again
    /// is the same request.
    fn inline_look(&mut self, cx: &mut JobContext) -> Result<(), JobFailure> {
        let persona: PersonaId = id(&self.payload.persona)?;
        let look = self
            .handler
            .personas
            .look(persona)
            .map_err(unexpected)?
            .ok_or_else(|| JobFailure::unexpected("the persona has no look"))?;
        let print = look_print(look.image.bytes());
        self.look_bytes = Some(self.fit_look(&look)?);
        let named = format!("inline:{print}");
        if self.state.look.as_deref() != Some(named.as_str()) {
            self.state.look = Some(named);
            self.state.look_print = Some(print);
            self.state.realistic = look.realistic;
            self.save_state(cx)?;
        }
        Ok(())
    }

    /// The look re-encoded by ffmpeg as a JPEG within the model's limit,
    /// which also drops any metadata a photo carries (where and with what
    /// it was taken).
    fn fit_look(&self, look: &PersonaLook) -> Result<Vec<u8>, JobFailure> {
        let folder = private_folder()?;
        let source = folder
            .path()
            .join(format!("look.{}", look.image.format().extension()));
        let destination = folder.path().join("look-out.jpg");
        std::fs::write(&source, look.image.bytes())
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))?;
        self.handler
            .media
            .fit_picture(&source, &destination, self.model.max_image_bytes)
            .map_err(|e| JobFailure::new(JobFailureKind::Media, e.to_string()))?;
        std::fs::read(&destination)
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))
    }

    /// Records the take's cost (once, from what the provider says it used
    /// when it says) and saves it as the take's draft or final; the one it
    /// replaces goes, and so do the staged files nothing needs any more. A
    /// take that changed while it was made keeps what it has: the result
    /// is paid for but not kept.
    fn save(
        &mut self,
        bytes: &[u8],
        usage: Option<Metered>,
        seed: Option<i64>,
        cx: &mut JobContext,
    ) -> Result<(), JobFailure> {
        let files = &self.handler.files;
        let file = take_file(self.capture, self.payload.position, self.step, cx.id());
        files
            .write(self.project, &file, bytes)
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))?;
        let duration = match self.handler.media.probe(&files.path(self.project, &file)) {
            Ok(info) if !info.duration.is_zero() && info.video.is_some() => info.duration,
            Ok(_) => {
                let _ = files.remove(self.project, &file);
                return Err(JobFailure::new(
                    JobFailureKind::UnexpectedAnswer,
                    format!(
                        "{}: the take is not a video",
                        provider_name(self.provider())
                    ),
                ));
            }
            Err(error) => {
                let _ = files.remove(self.project, &file);
                return Err(JobFailure::new(JobFailureKind::Media, error.to_string()));
            }
        };
        let quote = self.state.quote_micros.map(Money::from_micros);
        if !self.state.recorded {
            self.handler.costs.record_for_project(
                PaidCall {
                    provider: self.provider(),
                    model: &self.payload.priced_model,
                    purpose: take_purpose(self.step),
                    usage: usage
                        .unwrap_or_else(|| Metered::video_seconds(u64::from(self.payload.seconds))),
                    job: Some(cx.id()),
                    reported: quote,
                },
                self.project,
            );
            self.state.recorded = true;
            self.save_state(cx)?;
        }
        if !self.still_approved()? {
            let _ = files.remove(self.project, &file);
            return Err(take_changed());
        }
        let captures = &self.handler.captures;
        let results = captures.take_results(self.project).map_err(unexpected)?;
        let replaced = results
            .iter()
            .find(|old| old.position == self.payload.position && old.step == self.step)
            .cloned();
        // A capture has one test: one of another take goes.
        let other_tests: Vec<TakeResult> = results
            .into_iter()
            .filter(|old| {
                self.step == TakeStep::Test
                    && old.step == TakeStep::Test
                    && old.position != self.payload.position
            })
            .collect();
        let template = match (&self.payload.template, self.payload.template_number) {
            (Some(template), Some(number)) => Some(TemplateUsed {
                id: id::<TemplateVersionId>(template)?,
                number,
            }),
            _ => None,
        };
        // A draft can be finished until its files' links (and the
        // provider's record of it) run out.
        let deadline = match self.step {
            TakeStep::Draft => self
                .state
                .staged_at
                .or(self.state.sent_at)
                .and_then(|from| self.model.draft_deadline(from_unix_millis(from)))
                .map(whole_millis),
            TakeStep::Final | TakeStep::Test => None,
        };
        let result = TakeResult {
            project: self.project,
            capture: self.capture,
            position: self.payload.position,
            step: self.step,
            control: self.payload.control.clone(),
            model: self.model.id.clone(),
            priced_model: self.payload.priced_model.clone(),
            prompt: self.payload.prompt.clone(),
            template,
            look: self.state.look_print.clone().unwrap_or_default(),
            realistic: self.state.realistic,
            request: self.state.request.clone().unwrap_or_default(),
            cost: quote,
            file: file.clone(),
            duration,
            made_at: whole_millis(SystemTime::now()),
            job: Some(cx.id()),
            accepted_at: None,
            seed,
            deadline,
        };
        if !captures.save_take_result(&result).map_err(unexpected)? {
            // The take is gone.
            let _ = files.remove(self.project, &file);
            return Err(take_changed());
        }
        if let Some(replaced) = replaced.filter(|old| old.file != file) {
            let _ = files.remove(self.project, &replaced.file);
            if let Some(job) = replaced.job.filter(|job| *job != cx.id()) {
                self.release(job)?;
            }
        }
        for old in &other_tests {
            self.handler.drop_test(self.project, old)?;
        }
        // The draft's files served their final.
        if let Some(draft) = &self.payload.draft_job {
            self.release(id(draft)?)?;
        }
        // Only a draft a final finishes needs its files again.
        if self.step != TakeStep::Draft || !self.model.finishes_draft {
            self.release(cx.id())?;
        }
        Ok(())
    }
}

/// What a take step's cost is recorded as.
fn take_purpose(step: TakeStep) -> CostPurpose {
    match step {
        TakeStep::Draft => CostPurpose::TakeDraft,
        TakeStep::Final => CostPurpose::TakeFinal,
        TakeStep::Test => CostPurpose::TakeTest,
    }
}

/// Removes the drafts' and finals' files of `capture`, which is going.
/// `results` are the project's, read before the capture went (its rows go
/// with it). A file that will not go (open in a player) stays.
pub(crate) fn remove_take_files(
    files: &dyn ProjectFiles,
    capture: &Capture,
    results: &[TakeResult],
) {
    for result in results.iter().filter(|r| r.capture == capture.id) {
        let _ = files.remove(capture.project, &result.file);
    }
}

/// The take of a prompt or take job.
fn take_of(job: &Job) -> Option<TakeOf> {
    matches!(job.kind(), JobKind::TakePrompt | JobKind::PerformedTake)
        .then(|| serde_json::from_str(job.payload()).ok())
        .flatten()
}

/// The calls a take's next step makes. A model billed by tokens counts
/// them from the take's `frame` (its control video's width and height).
fn step_calls(
    model: &TakeModel,
    next: PerformedStep,
    length: Duration,
    frame: (u32, u32),
) -> Vec<PlannedCall> {
    match next {
        PerformedStep::Prompt => {
            vec![PlannedCall::new(
                Provider::Claude,
                CostPurpose::TakePrompt,
                1,
            )]
        }
        PerformedStep::Make(step) => {
            let call = PlannedCall::new(model.id.provider(), take_purpose(step), 1)
                .with_model(&model.priced_model(step));
            // A model billed in tokens is priced by those alone: a rate
            // table has no seconds rate for it.
            vec![match model.estimated_tokens(step, length, frame) {
                Some(tokens) => call.with_output_tokens(tokens),
                None => call.with_video_seconds(billed_seconds(length)),
            }]
        }
    }
}

/// What a take's next paid step is: its prompt, then its draft until one
/// is approved (where the model makes drafts) and can still be finished,
/// then its final. Where the draft is optional and a final was made
/// without one, the final.
fn next_step(
    model: Option<&TakeModel>,
    prompt: Option<&TakePrompt>,
    draft: Option<&TakeResult>,
    final_made: bool,
    now: SystemTime,
) -> PerformedStep {
    if prompt.is_none() {
        return PerformedStep::Prompt;
    }
    let drafts = model.is_some_and(|model| model.draft.is_some());
    let skipped = final_made && draft.is_none() && model.is_some_and(TakeModel::draft_optional);
    if drafts
        && !skipped
        && draft.is_none_or(|draft| draft.accepted_at.is_none() || draft.expired(now))
    {
        PerformedStep::Make(TakeStep::Draft)
    } else {
        PerformedStep::Make(TakeStep::Final)
    }
}

impl Bardo {
    /// Queues a cleanup of the staged files that are due, unless one is
    /// queued or none is due.
    pub(crate) fn queue_staging_cleanup(&self) {
        let due = self
            .staging
            .staged_due(self.profile.id, SystemTime::now())
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "could not read the staged files");
                Vec::new()
            });
        let queued = self
            .jobs()
            .iter()
            .any(|job| job.kind() == JobKind::StagingCleanup && job.state().is_active());
        if due.is_empty() || queued {
            return;
        }
        let job = Job::new(self.profile.id, JobKind::StagingCleanup, "{}".to_owned());
        if let Err(error) = self.jobs.enqueue(job) {
            tracing::warn!(%error, "could not queue the staged files' cleanup");
        }
    }

    /// When a draft's deadline is, on this computer's clock.
    pub fn take_deadline_text(&self, at: SystemTime) -> String {
        self.backup_time_text(at)
    }

    /// Every take model offered, the default first.
    pub fn take_models(&self) -> Vec<TakeModel> {
        self.takes
            .iter()
            .flat_map(|generator| generator.models())
            .collect()
    }

    /// Whether `model` is offered.
    pub(crate) fn offers_take_model(&self, model: &ClipModelRef) -> bool {
        self.take_models()
            .iter()
            .any(|offered| offered.id == *model)
    }

    /// The take model a channel picked, if any.
    fn channel_take_ref(
        &self,
        channel: ChannelId,
    ) -> Result<Option<ClipModelRef>, RepositoryError> {
        Ok(self
            .channels
            .get(channel)?
            .and_then(|channel| channel.details.take_model().cloned()))
    }

    /// The take model of a new capture's takes: the channel's, else the
    /// default.
    pub(crate) fn capture_take_model(&self, channel: ChannelId) -> Option<TakeModel> {
        let models = self.take_models();
        let channel = self.channel_take_ref(channel).ok().flatten();
        take_model_of(&models, channel.as_ref(), None).cloned()
    }

    /// The take model of take `index` of `capture`, with the channel's.
    fn take_model_at(
        &self,
        capture: &Capture,
        index: usize,
    ) -> Result<(Option<TakeModel>, Option<TakeModel>), CaptureError> {
        let project = self
            .themes
            .project(capture.project)?
            .ok_or(CaptureError::ProjectNotFound)?;
        let models = self.take_models();
        let channel = self.channel_take_ref(project.channel)?;
        let own = capture
            .takes
            .as_slice()
            .get(index)
            .ok_or(CaptureError::Boundary(
                bardo_domain::BoundaryError::NoSuchTake,
            ))?
            .model
            .clone();
        Ok((
            take_model_of(&models, channel.as_ref(), own.as_ref()).cloned(),
            take_model_of(&models, channel.as_ref(), None).cloned(),
        ))
    }

    /// The prompt and take jobs of `capture`, oldest first.
    fn performed_jobs(&self, capture: &Capture) -> Vec<(Job, TakeOf)> {
        let (project, id) = (capture.project.to_string(), capture.id.to_string());
        self.jobs()
            .into_iter()
            .filter_map(|job| take_of(&job).map(|of| (job, of)))
            .filter(|(_, of)| of.project == project && of.capture == id)
            .collect()
    }

    /// The look of the persona that performs `project`'s takes.
    pub(crate) fn performer_look(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<(PersonaId, PersonaLook)>, CaptureError> {
        let project = self
            .themes
            .project(project)?
            .ok_or(CaptureError::ProjectNotFound)?;
        let Some(persona) = self.narrator(&project)? else {
            return Ok(None);
        };
        Ok(self
            .personas
            .look(persona.id)?
            .map(|look| (persona.id, look)))
    }

    /// Whether a key is saved for `provider`.
    fn has_take_key(&self, provider: Provider) -> bool {
        self.provider_key(provider).state != KeyState::NotSet
    }

    /// The generator of `model`'s provider.
    fn generator_of(&self, model: &TakeModel) -> Option<&Arc<dyn TakeGenerator>> {
        self.takes
            .iter()
            .find(|generator| generator.provider() == model.id.provider())
    }

    /// Whether `model` can have its control video staged: its provider
    /// stages it, or storage with a saved key does (#243).
    fn video_hosted(&self, model: &TakeModel) -> bool {
        self.generator_of(model)
            .is_none_or(|generator| generator.stages_video())
            || video_host(&self.takes, |provider| self.has_take_key(provider)).is_some()
    }

    /// The face in the performer's look, as measured; a look not measured
    /// yet gets its measuring job queued.
    fn look_face(&self, persona: PersonaId, look: &PersonaLook) -> Result<LookFace, CaptureError> {
        let print = look_print(look.image.bytes());
        if let Some(measure) = self
            .personas
            .look_face(persona)?
            .filter(|measure| measure.image == print)
        {
            return Ok(match measure.face_height {
                Some(height) => LookFace::Height(height),
                None => LookFace::NoFace,
            });
        }
        crate::look_framing::queue_look_framing(self, persona, &print);
        Ok(LookFace::Unmeasured)
    }

    /// The video's cap: the user's, else the default.
    fn take_cap(&self, project: VideoProjectId) -> Result<(Money, bool), CaptureError> {
        Ok(match self.captures.take_cap(project)? {
            Some(cap) => (cap, true),
            None => (DEFAULT_TAKE_CAP, false),
        })
    }

    /// What `project`'s performed takes cost so far: prompts, tests,
    /// drafts and finals, as Costs records them.
    fn takes_spent(&self, project: VideoProjectId) -> Result<Money, CaptureError> {
        let rates = self.cost_book.rates()?;
        Ok(self
            .cost_book
            .costs
            .project_costs(project)?
            .into_iter()
            .filter(|record| {
                matches!(
                    record.purpose,
                    CostPurpose::TakePrompt
                        | CostPurpose::TakeDraft
                        | CostPurpose::TakeFinal
                        | CostPurpose::TakeTest
                )
            })
            .map(|record| record.priced_with(&rates).cost.amount())
            .sum())
    }

    /// The performed clips part of the Capture stage for `capture`, whose
    /// takes' control videos are `controls`.
    pub(crate) fn performed_view(
        &self,
        capture: &Capture,
        controls: &[ControlVideoView],
    ) -> Result<PerformedView, CaptureError> {
        let project = capture.project;
        let channel = self
            .themes
            .project(project)?
            .ok_or(CaptureError::ProjectNotFound)?
            .channel;
        let models = self.take_models();
        let channel_ref = self.channel_take_ref(channel)?;
        let channel_model = take_model_of(&models, channel_ref.as_ref(), None).cloned();
        let look = self.performer_look(project)?;
        let look_print_now = look
            .as_ref()
            .map(|(_, look)| look_print(look.image.bytes()));
        let look_face = match &look {
            Some((persona, look)) => self.look_face(*persona, look)?,
            None => LookFace::Unmeasured,
        };
        // The looks the providers' asset libraries hold for it.
        let likeness = |model: &TakeModel| -> Result<bool, CaptureError> {
            let Some((persona, look)) = &look else {
                return Ok(true);
            };
            if !model.look_assets || look.source != LookSource::Imported {
                return Ok(true);
            }
            let print = look_print(look.image.bytes());
            Ok(self
                .personas
                .look_asset(*persona, model.id.provider())?
                .is_some_and(|asset| {
                    asset.kind == LookAssetKind::VerifiedLikeness && asset.holds_for(look, &print)
                }))
        };
        let frame = bardo_domain::control_size(capture.video.width, capture.video.height);
        let now = SystemTime::now();
        let prompts = self.captures.take_prompts(project)?;
        let results = self.captures.take_results(project)?;
        let overrides = self.captures.preflight_overrides(project)?;
        let analyses = self.analysis_views(capture)?;
        let used = self.media_assets.performed_clips(project)?;
        let jobs = self.performed_jobs(capture);
        let is_test = |of: &TakeOf| of.step.as_deref() == Some(TakeStep::Test.code());
        let mut takes = Vec::with_capacity(capture.takes.as_slice().len());
        let (mut to_come, mut partial) = (Money::ZERO, false);
        for (index, take) in capture.takes.as_slice().iter().enumerate() {
            let position = index as u32;
            let model = take_model_of(&models, channel_ref.as_ref(), take.model.as_ref()).cloned();
            let approved = controls
                .get(index)
                .filter(|view| view.is_approved())
                .and_then(|view| view.built.as_ref())
                .map(|built| built.fingerprint.clone());
            let block = match (&model, &approved) {
                (None, _) => Some(PerformedBlock::NoModel),
                (Some(model), _) if !model.takes(take.mode) => Some(PerformedBlock::ModeNotTaken),
                (Some(model), _) if !model.fits(take.length()) => Some(PerformedBlock::Length),
                _ if look.is_none() => Some(PerformedBlock::NoLook),
                (Some(model), _) if !likeness(model)? => Some(PerformedBlock::NoVerifiedLikeness),
                (Some(model), _) if !self.video_hosted(model) => Some(PerformedBlock::NoVideoHost),
                (_, None) => Some(PerformedBlock::ControlNotApproved),
                _ => None,
            };
            let fits = |control: &str, made: &ClipModelRef| {
                approved.as_deref() == Some(control)
                    && model.as_ref().is_some_and(|model| model.id == *made)
            };
            let prompt = prompts
                .iter()
                .find(|p| {
                    p.position == position && p.capture == capture.id && fits(&p.control, &p.model)
                })
                .cloned();
            let result = |step: TakeStep| {
                results
                    .iter()
                    .find(|r| {
                        r.position == position
                            && r.step == step
                            && r.capture == capture.id
                            && fits(&r.control, &r.model)
                            && self.files.exists(project, &r.file)
                    })
                    .cloned()
            };
            let (draft, final_take) = (result(TakeStep::Draft), result(TakeStep::Final));
            let finished = model.as_ref().is_some_and(|model| model.finishes_draft)
                && draft
                    .as_ref()
                    .zip(final_take.as_ref())
                    .is_some_and(|(draft, made)| {
                        draft.accepted_at.is_some() && made.made_at >= draft.made_at
                    });
            let of_take = || jobs.iter().filter(|(_, of)| of.position == position);
            let busy = of_take().any(|(job, _)| job.state().is_active());
            // One under way, else the latest made for the take as it is now:
            // one for another control video or model is history. Tests are
            // the capture's, apart.
            let latest = |kind: JobKind| {
                of_take().rev().find(|(job, of)| {
                    job.kind() == kind
                        && !is_test(of)
                        && (job.state().is_active()
                            || of.made_for(approved.as_deref(), model.as_ref()))
                })
            };
            let prompt_job = latest(JobKind::TakePrompt).map(|(job, _)| job.clone());
            let job = latest(JobKind::PerformedTake).and_then(|(job, of)| {
                let step = TakeStep::from_code(of.step.as_deref()?)?;
                Some((job.clone(), step))
            });
            let pending = job.as_ref().is_some_and(|(job, _)| {
                matches!(job.state(), JobState::Failed | JobState::Cancelled) && has_request(job)
            });
            let next = next_step(
                model.as_ref(),
                prompt.as_ref(),
                draft.as_ref().filter(|_| !finished),
                final_take.is_some(),
                now,
            );
            let estimate_of = |model: &TakeModel, step: PerformedStep| {
                self.estimate(&step_calls(model, step, take.length(), frame))
            };
            let estimate = match (&model, block) {
                (Some(model), None) => Some(estimate_of(model, next)?),
                _ => None,
            };
            let skip_draft = match (&model, block, next) {
                (Some(model), None, PerformedStep::Make(TakeStep::Draft))
                    if model.draft_optional() =>
                {
                    Some(estimate_of(model, PerformedStep::Make(TakeStep::Final))?)
                }
                _ => None,
            };
            // What is still to pay for this take: its prompt, a draft its
            // model requires, and its final, until each is made.
            if let Some(model) = &model {
                let mut steps = Vec::new();
                if prompt.is_none() {
                    steps.push(PerformedStep::Prompt);
                }
                if model.draft_required()
                    && !finished
                    && draft
                        .as_ref()
                        .is_none_or(|draft| draft.accepted_at.is_none() || draft.expired(now))
                {
                    steps.push(PerformedStep::Make(TakeStep::Draft));
                }
                if final_take.is_none() {
                    steps.push(PerformedStep::Make(TakeStep::Final));
                }
                for step in steps {
                    let estimate = estimate_of(model, step)?;
                    to_come = to_come.saturating_add(estimate.total());
                    partial |= estimate.is_partial();
                }
            }
            let preflight = model.as_ref().map(|model| {
                preflight(
                    model,
                    // The checks read the face mesh: a depth part alone
                    // says nothing about faces.
                    analyses
                        .get(index)
                        .filter(|view| matches!(view.face, crate::AnalysisStatus::Done { .. }))
                        .and_then(|view| view.checks.as_ref()),
                    look_face,
                )
            });
            let overridden = approved.as_deref().is_some_and(|control| {
                overrides.iter().any(|o| {
                    o.capture == capture.id && o.position == position && o.control == control
                })
            });
            let expired = draft.as_ref().is_some_and(|draft| draft.expired(now));
            takes.push(PerformedTakeView {
                finished,
                follows_channel: take.model.is_none()
                    || take.model.as_ref() == channel_model.as_ref().map(|m| &m.id),
                model,
                block,
                prompt,
                draft,
                final_take,
                expired,
                prompt_job,
                job,
                busy,
                pending,
                next,
                estimate,
                skip_draft,
                preflight,
                overridden,
                length: take.length(),
                used: used
                    .iter()
                    .filter(|clip| clip.capture == capture.id && clip.position == position)
                    .count(),
            });
        }

        // The capture's test: made for the look as it is now, on disk.
        // The latest, should an older one of another take outlive the job
        // that replaced it.
        let test = results
            .iter()
            .filter(|r| {
                r.step == TakeStep::Test
                    && r.capture == capture.id
                    && look_print_now.as_deref() == Some(r.look.as_str())
                    && self.files.exists(project, &r.file)
            })
            .max_by_key(|r| r.made_at)
            .cloned();
        let test_job = jobs
            .iter()
            .rev()
            .find(|(job, of)| job.kind() == JobKind::PerformedTake && is_test(of))
            .map(|(job, _)| job.clone());
        let test_pending = test_job.as_ref().is_some_and(|job| {
            matches!(job.state(), JobState::Failed | JobState::Cancelled) && has_request(job)
        });
        // A test comes from the first take that could go to its model now,
        // one its preflight checks let through first. While the test runs,
        // its own take stays the one shown.
        let test_running = test_job.as_ref().is_some_and(|job| job.state().is_active());
        let ready = |view: &PerformedTakeView| {
            view.block.is_none()
                && (test_running || !view.is_busy())
                && !view.pending
                && view.prompt.is_some()
                && view
                    .model
                    .as_ref()
                    .is_some_and(|model| model.test_length(view.length).is_some())
        };
        let test_take = takes
            .iter()
            .position(|view| ready(view) && !view.preflight_holds())
            .or_else(|| takes.iter().position(ready));
        let test_estimate = match test_take {
            Some(index) => {
                let view = &takes[index];
                let model = view.model.as_ref().expect("a test take has a model");
                let piece = model.test_length(view.length).expect("it fits");
                Some(self.estimate(&step_calls(
                    model,
                    PerformedStep::Make(TakeStep::Test),
                    piece,
                    frame,
                ))?)
            }
            None => None,
        };
        // A capture that already made a draft or final with the look as it
        // is now (before tests existed, or after an approved test) goes on.
        let made_before = results.iter().any(|r| {
            r.capture == capture.id
                && matches!(r.step, TakeStep::Draft | TakeStep::Final)
                && look_print_now.as_deref() == Some(r.look.as_str())
        });
        let needs_test =
            !made_before && test.as_ref().is_none_or(|test| test.accepted_at.is_none());
        let (cap, own_cap) = self.take_cap(project)?;
        Ok(PerformedView {
            models,
            channel_model,
            has_look: look.is_some(),
            realistic: look.is_some_and(|(_, look)| look.realistic),
            takes,
            test,
            test_job,
            test_pending,
            test_take,
            test_estimate,
            needs_test,
            spend: TakeSpend {
                spent: self.takes_spent(project)?,
                to_come,
                partial,
                cap,
            },
            own_cap,
            look_face,
        })
    }

    /// Take `index`'s view as the project is now.
    fn performed_take(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(Capture, PerformedTakeView), CaptureError> {
        let (capture, _) = self.current_capture(project)?;
        let controls = self.control_views(&capture)?;
        let view = self
            .performed_view(&capture, &controls)?
            .takes
            .into_iter()
            .nth(index)
            .ok_or(CaptureError::Boundary(
                bardo_domain::BoundaryError::NoSuchTake,
            ))?;
        Ok((capture, view))
    }

    /// Makes take `index` go to `model`, or to the channel's with `None`
    /// (or when `model` is the channel's). A mode the model does not take
    /// becomes its first, which builds the control video again.
    pub fn set_take_model(
        &self,
        project: VideoProjectId,
        index: usize,
        model: Option<ClipModelRef>,
    ) -> Result<(), CaptureError> {
        let (mut capture, _) = self.current_capture(project)?;
        if self.performed_busy(&capture, index) {
            return Err(CaptureError::TakeBusy);
        }
        let models = self.take_models();
        if model
            .as_ref()
            .is_some_and(|wanted| !models.iter().any(|offered| offered.id == *wanted))
        {
            return Err(CaptureError::TakeModelNotOffered);
        }
        let (_, channel) = self.take_model_at(&capture, index)?;
        let own = model.filter(|wanted| channel.as_ref().is_none_or(|c| c.id != *wanted));
        let take = &capture.takes.as_slice()[index];
        let effective = take_model_of(&models, channel.as_ref().map(|c| &c.id), own.as_ref());
        let mode = match effective {
            Some(model) if !model.takes(take.mode) => model.default_mode(),
            _ => take.mode,
        };
        if take.model == own && take.mode == mode {
            return Ok(());
        }
        let mode_changed = take.mode != mode;
        capture.takes.set_model(index, own, mode)?;
        self.captures
            .save_takes(capture.project, capture.id, &capture.takes)?;
        if mode_changed {
            self.queue_take_analyses(&capture, index, false)?;
            self.queue_control_video(&capture, index)?;
        }
        Ok(())
    }

    /// Whether take `index` of `capture` may be in `mode`: its model takes
    /// it.
    pub(crate) fn takes_mode(
        &self,
        capture: &Capture,
        index: usize,
        mode: TakeMode,
    ) -> Result<bool, CaptureError> {
        let (model, _) = self.take_model_at(capture, index)?;
        Ok(model.is_none_or(|model| model.takes(mode)))
    }

    fn require_take_key(&self, provider: Provider) -> Result<(), CaptureError> {
        if self.provider_key(provider).state == KeyState::NotSet {
            return Err(CaptureError::MissingKey(provider));
        }
        Ok(())
    }

    fn check_take(view: &PerformedTakeView) -> Result<&TakeModel, CaptureError> {
        if let Some(block) = view.block {
            return Err(CaptureError::Performed(block));
        }
        if view.is_busy() {
            return Err(CaptureError::TakeBusy);
        }
        if view.pending {
            return Err(CaptureError::TakePending);
        }
        view.model
            .as_ref()
            .ok_or(CaptureError::Performed(PerformedBlock::NoModel))
    }

    /// Starts a job in which Claude writes take `index`'s prompt, for its
    /// approved control video and its model. Past Claude's budget it needs
    /// `consent`.
    pub fn write_take_prompt(
        &self,
        project: VideoProjectId,
        index: usize,
        consent: BudgetConsent,
    ) -> Result<JobId, CaptureError> {
        let (capture, view) = self.performed_take(project, index)?;
        let model = Self::check_take(&view)?;
        self.require_take_key(Provider::Claude)?;
        let controls = self.control_views(&capture)?;
        let control = controls
            .get(index)
            .and_then(|view| view.built.clone())
            .ok_or(CaptureError::ControlNotReady)?;
        let take = &capture.takes.as_slice()[index];
        let (_, narration) = self.current_capture(project)?;
        let words = match (
            narration.words.as_slice().get(take.words.start),
            take.words
                .end
                .checked_sub(1)
                .and_then(|last| narration.words.as_slice().get(last)),
        ) {
            (Some(first), Some(last)) => narration
                .text
                .as_str()
                .get(first.text.start..last.text.end)
                .unwrap_or_default()
                .to_owned(),
            _ => String::new(),
        };
        let (_, look) = self
            .performer_look(project)?
            .ok_or(CaptureError::Performed(PerformedBlock::NoLook))?;
        let look_text = match (look.source, &look.drawing) {
            (LookSource::Drawn, Some(drawing)) if !drawing.description.trim().is_empty() => {
                drawing.description.trim().to_owned()
            }
            _ => LOOK_FROM_PHOTO.to_owned(),
        };
        let frame = Outbound::of(take.mode, &take.set).sends_set_frame();
        let template = self.current_template(TemplateKind::TakePrompt)?;
        let values = TemplateValues::from([
            (TemplateVariable::Look, look_text),
            (
                TemplateVariable::Set,
                if frame {
                    SET_FROM_FRAME
                } else {
                    SET_FROM_IMAGE
                }
                .to_owned(),
            ),
            (TemplateVariable::Words, words),
            (
                TemplateVariable::Instruction,
                instruction(take.mode).to_owned(),
            ),
        ]);
        let rendered = template
            .body
            .render(&values)
            .map_err(|missing| CaptureError::Repository(RepositoryError(Box::new(missing))))?;
        let calls = step_calls(model, PerformedStep::Prompt, take.length(), (0, 0));
        if let Err(estimate) = self.check_video_budget(project, &calls, consent)? {
            return Err(CaptureError::OverBudget(estimate));
        }
        let payload = PromptPayload {
            project: project.to_string(),
            capture: capture.id.to_string(),
            position: index as u32,
            control: control.fingerprint.clone(),
            control_file: control.file.clone(),
            provider: model.id.provider().code().to_owned(),
            model: model.id.model().to_owned(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
            frame_at_ms: frame.then(|| (take.length() / 2).as_millis() as u64),
        };
        let job = Job::new(self.profile.id, JobKind::TakePrompt, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Replaces take `index`'s prompt with the user's text.
    pub fn edit_take_prompt(
        &self,
        project: VideoProjectId,
        index: usize,
        text: &str,
    ) -> Result<(), CaptureError> {
        let text = take_prompt_text(text).map_err(CaptureError::Prompt)?;
        let (_, view) = self.performed_take(project, index)?;
        if view.is_busy() {
            return Err(CaptureError::TakeBusy);
        }
        let prompt = view
            .prompt
            .ok_or(CaptureError::Prompt(TakePromptError::Empty))?;
        if prompt.text == text {
            return Ok(());
        }
        let edited = TakePrompt {
            text,
            edited: true,
            ..prompt
        };
        if !self.captures.save_take_prompt(&edited)? {
            return Err(CaptureError::NoCapture);
        }
        Ok(())
    }

    /// Starts a job in which take `index`'s model makes its draft, final
    /// or the capture's test from its prompt, its approved control video
    /// and the persona's look. A final needs an approved draft where the
    /// model requires one, and finishes it within its deadline where the
    /// model's final finishes its draft. Drafts and finals wait for an
    /// approved test, and every step for the take's preflight checks to
    /// pass or be overridden. Past the provider's budget or the video's
    /// cap it needs `consent`.
    pub fn make_performed_take(
        &self,
        project: VideoProjectId,
        index: usize,
        step: TakeStep,
        consent: BudgetConsent,
    ) -> Result<JobId, CaptureError> {
        let (capture, payload) = self.take_payload(project, index, step, consent)?;
        // Files an earlier attempt staged and never sent are not needed.
        let now = SystemTime::now();
        for (old, of) in self.performed_jobs(&capture) {
            if of.position == payload.position && !old.state().is_active() && !has_request(&old) {
                self.staging.release_staged_by(old.id(), now)?;
            }
        }
        let job = Job::new(self.profile.id, JobKind::PerformedTake, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// What a job making take `index`'s `step` sends, once everything
    /// [`Self::make_performed_take`] asks holds.
    fn take_payload(
        &self,
        project: VideoProjectId,
        index: usize,
        step: TakeStep,
        consent: BudgetConsent,
    ) -> Result<(Capture, TakePayload), CaptureError> {
        let (capture, _) = self.current_capture(project)?;
        let controls = self.control_views(&capture)?;
        let performed = self.performed_view(&capture, &controls)?;
        let view = performed
            .takes
            .get(index)
            .ok_or(CaptureError::Boundary(
                bardo_domain::BoundaryError::NoSuchTake,
            ))?
            .clone();
        let model = Self::check_take(&view)?;
        let prompt = view
            .prompt
            .as_ref()
            .ok_or(CaptureError::Prompt(TakePromptError::Empty))?;
        let text = take_prompt_text(&prompt.text).map_err(CaptureError::Prompt)?;
        let tier = model.tier(step).ok_or(CaptureError::NoDrafts)?;
        if step == TakeStep::Test && performed.test_busy() {
            return Err(CaptureError::TakeBusy);
        }
        if step == TakeStep::Test && performed.test_pending {
            return Err(CaptureError::TakePending);
        }
        if step != TakeStep::Test && performed.needs_test {
            return Err(CaptureError::TestNotApproved);
        }
        if view.preflight_holds() {
            return Err(CaptureError::PreflightFailed);
        }
        let piece = match step {
            TakeStep::Test => Some(
                model
                    .test_length(view.length)
                    .ok_or(CaptureError::Performed(PerformedBlock::Length))?,
            ),
            _ => None,
        };
        if step == TakeStep::Final
            && model.draft_required()
            && view
                .draft
                .as_ref()
                .is_none_or(|draft| draft.accepted_at.is_none())
        {
            return Err(CaptureError::DraftNotApproved);
        }
        let finishes = (step == TakeStep::Final && model.finishes_draft)
            .then_some(view.draft.as_ref())
            .flatten();
        if finishes.is_some_and(|draft| draft.expired(SystemTime::now())) {
            return Err(CaptureError::DraftExpired);
        }
        if finishes.is_some() && view.finished {
            return Err(CaptureError::DraftFinished);
        }
        self.require_take_key(model.id.provider())?;
        let take = &capture.takes.as_slice()[index];
        let controls = self.control_views(&capture)?;
        let control = controls
            .get(index)
            .and_then(|view| view.built.clone())
            .ok_or(CaptureError::ControlNotReady)?;
        let (persona, _) = self
            .performer_look(project)?
            .ok_or(CaptureError::Performed(PerformedBlock::NoLook))?;
        let frame = bardo_domain::control_size(capture.video.width, capture.video.height);
        let length = piece.unwrap_or(take.length());
        let calls = step_calls(model, PerformedStep::Make(step), length, frame);
        // A step still to come is in the projection already; a test, an
        // optional draft or a step made again is not.
        let counted = match step {
            TakeStep::Final => view.final_take.is_none(),
            TakeStep::Draft => {
                model.draft_required()
                    && !view.finished
                    && view.draft.as_ref().is_none_or(|draft| {
                        draft.accepted_at.is_none() || draft.expired(SystemTime::now())
                    })
            }
            TakeStep::Test => false,
        };
        // Past the cap, one question asks about the cap and any budget the
        // step reaches together; the user's word covers both.
        let estimate = self.estimate(&calls)?;
        let call = estimate.total();
        if consent == BudgetConsent::Ask && performed.spend.goes_over(call, counted) {
            // The video's cap, when the step passes it too, joins the card.
            let estimate = match self.check_video_budget(project, &calls, consent)? {
                Err(video) => video,
                Ok(()) => estimate,
            };
            let spend = TakeSpend {
                to_come: if counted {
                    performed.spend.to_come
                } else {
                    performed.spend.to_come.saturating_add(call)
                },
                ..performed.spend
            };
            return Err(CaptureError::OverCap(spend, estimate));
        }
        if let Err(estimate) = self.check_video_budget(project, &calls, consent)? {
            return Err(CaptureError::OverBudget(estimate));
        }
        let payload = TakePayload {
            project: project.to_string(),
            capture: capture.id.to_string(),
            position: index as u32,
            step: step.code().to_owned(),
            control: control.fingerprint.clone(),
            control_file: control.file.clone(),
            provider: model.id.provider().code().to_owned(),
            model: model.id.model().to_owned(),
            priced_model: model.priced_model(step),
            resolution: tier.resolution.clone(),
            prompt: text,
            template: prompt.template.map(|t| t.id.to_string()),
            template_number: prompt.template.map(|t| t.number),
            persona: persona.to_string(),
            seconds: billed_seconds(length),
            draft_task: finishes.map(|draft| draft.request.clone()),
            draft_job: finishes
                .and_then(|draft| draft.job)
                .map(|job| job.to_string()),
            draft_look: finishes.map(|draft| draft.look.clone()),
            draft_realistic: finishes.is_some_and(|draft| draft.realistic),
            draft_deadline: finishes.and_then(|draft| draft.deadline).map(unix_millis),
            piece: piece.map(|piece| {
                let (width, height) =
                    bardo_domain::control_size(capture.video.width, capture.video.height);
                TestPiece {
                    length: piece.as_millis() as u64,
                    width,
                    height,
                    fps_milli: bardo_domain::control_fps(take.mode, capture.video.fps_milli),
                }
            }),
        };
        Ok((capture, payload))
    }

    /// Checks again on take `index`'s draft or final job that stopped (or
    /// failed) after the provider took its request: it polls that request,
    /// paying nothing new.
    pub fn check_take_again(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(), CaptureError> {
        let (_, view) = self.performed_take(project, index)?;
        if view.is_busy() {
            return Err(CaptureError::TakeBusy);
        }
        let (job, _) = view
            .job
            .filter(|_| view.pending)
            .ok_or(CaptureError::NoTakeResult)?;
        self.retry_job(job.id())
            .map_err(|_| CaptureError::NoTakeResult)
    }

    /// Whether `job` is a performed take that never reached its provider
    /// and that making it from the capture now would not send without a
    /// question: past the video's cap or a budget, waiting for a test, or
    /// held by a preflight check. Running it again from the jobs panel
    /// would pay without that question.
    pub(crate) fn unsent_take_needs_asking(&self, job: JobId) -> bool {
        let Some(payload) = self
            .jobs()
            .into_iter()
            .find(|candidate| {
                candidate.id() == job
                    && candidate.kind() == JobKind::PerformedTake
                    && !has_request(candidate)
            })
            .and_then(|job| serde_json::from_str::<TakePayload>(job.payload()).ok())
        else {
            return false;
        };
        let (Ok(project), Some(step)) = (
            payload.project.parse::<uuid::Uuid>(),
            TakeStep::from_code(&payload.step),
        ) else {
            return false;
        };
        // A take that changed fails on its own when it runs, paying nothing.
        match self.take_payload(
            VideoProjectId::from(project),
            payload.position as usize,
            step,
            BudgetConsent::Ask,
        ) {
            Ok(_) => false,
            Err(error) => matches!(
                error,
                CaptureError::OverCap(..)
                    | CaptureError::OverBudget(_)
                    | CaptureError::TestNotApproved
                    | CaptureError::PreflightFailed
            ),
        }
    }

    /// Whether a prompt or a performed take of take `index` of `capture`
    /// is being made: its take, set, mode and model stay as they are until
    /// it ends.
    pub(crate) fn performed_busy(&self, capture: &Capture, index: usize) -> bool {
        self.performed_jobs(capture)
            .iter()
            .any(|(job, of)| of.position as usize == index && job.state().is_active())
    }

    /// Whether a prompt or a performed take of `project`'s capture is
    /// being made: the capture (and the narration it made) stays until it
    /// ends, so a paid result is never thrown away.
    pub(crate) fn takes_busy(&self, project: VideoProjectId) -> bool {
        let project = project.to_string();
        self.jobs().into_iter().any(|job| {
            job.state().is_active() && take_of(&job).is_some_and(|of| of.project == project)
        })
    }

    /// Approves take `index`'s draft: its final can be made.
    pub fn approve_take_draft(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(), CaptureError> {
        self.accept_take(project, index, TakeStep::Draft, Some(SystemTime::now()))
    }

    /// Takes take `index` back to its draft, which waits for review again.
    pub fn back_to_take_drafts(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(), CaptureError> {
        self.accept_take(project, index, TakeStep::Draft, None)
    }

    fn accept_take(
        &self,
        project: VideoProjectId,
        index: usize,
        step: TakeStep,
        at: Option<SystemTime>,
    ) -> Result<(), CaptureError> {
        let (_, view) = self.performed_take(project, index)?;
        // A draft past its deadline can no longer lead to a final.
        if step == TakeStep::Draft && at.is_some() && view.expired {
            return Err(CaptureError::DraftExpired);
        }
        let made = match step {
            TakeStep::Draft => view.draft.as_ref(),
            TakeStep::Final => view.final_take.as_ref(),
            TakeStep::Test => return Err(CaptureError::NoTakeResult),
        }
        .ok_or(CaptureError::NoTakeResult)?;
        if !self.captures.accept_take_result(
            made.project,
            made.position,
            step,
            &made.control,
            at.map(whole_millis),
        )? {
            return Err(CaptureError::NoTakeResult);
        }
        Ok(())
    }

    /// The capture's test take as the Capture stage shows it.
    fn current_test(
        &self,
        project: VideoProjectId,
    ) -> Result<(Capture, PerformedView), CaptureError> {
        let (capture, _) = self.current_capture(project)?;
        let controls = self.control_views(&capture)?;
        let view = self.performed_view(&capture, &controls)?;
        Ok((capture, view))
    }

    /// Approves the capture's test take: its takes' drafts and finals can
    /// be made.
    pub fn approve_take_test(&self, project: VideoProjectId) -> Result<(), CaptureError> {
        let (_, view) = self.current_test(project)?;
        let test = view.test.ok_or(CaptureError::NoTakeResult)?;
        if !self.captures.accept_take_result(
            test.project,
            test.position,
            TakeStep::Test,
            &test.control,
            Some(whole_millis(SystemTime::now())),
        )? {
            return Err(CaptureError::NoTakeResult);
        }
        Ok(())
    }

    /// Discards the capture's test take: its row and file go, and the
    /// takes wait for another.
    pub fn discard_take_test(&self, project: VideoProjectId) -> Result<(), CaptureError> {
        let (_, view) = self.current_test(project)?;
        if view.test_busy() {
            return Err(CaptureError::TakeBusy);
        }
        let test = view.test.ok_or(CaptureError::NoTakeResult)?;
        self.captures
            .remove_take_result(project, test.position, TakeStep::Test)?;
        let _ = self.files.remove(project, &test.file);
        Ok(())
    }

    /// Checks again on the test's job that stopped (or failed) after the
    /// provider took its request.
    pub fn check_test_again(&self, project: VideoProjectId) -> Result<(), CaptureError> {
        let (_, view) = self.current_test(project)?;
        if view.test_busy() {
            return Err(CaptureError::TakeBusy);
        }
        let job = view
            .test_job
            .filter(|_| view.test_pending)
            .ok_or(CaptureError::NoTakeResult)?;
        self.retry_job(job.id())
            .map_err(|_| CaptureError::NoTakeResult)
    }

    /// Sends take `index` on although a preflight check failed: it holds
    /// for its approved control video.
    pub fn send_take_anyway(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(), CaptureError> {
        let (capture, view) = self.performed_take(project, index)?;
        if view.preflight.is_none() {
            return Err(CaptureError::Performed(PerformedBlock::NoModel));
        }
        let controls = self.control_views(&capture)?;
        let control = controls
            .get(index)
            .filter(|view| view.is_approved())
            .and_then(|view| view.built.clone())
            .ok_or(CaptureError::Performed(PerformedBlock::ControlNotApproved))?;
        let chosen = PreflightOverride {
            project,
            capture: capture.id,
            position: index as u32,
            control: control.fingerprint,
            at: whole_millis(SystemTime::now()),
        };
        if !self.captures.save_preflight_override(&chosen)? {
            return Err(CaptureError::NoCapture);
        }
        Ok(())
    }

    /// Sets what the video's performed takes may cost before Bardo asks,
    /// from the amount the user typed; nothing typed goes back to the
    /// default. At least [`MIN_TAKE_CAP`].
    pub fn set_take_cap(&self, project: VideoProjectId, typed: &str) -> Result<(), CaptureError> {
        let cap = match typed.trim() {
            "" => None,
            typed => Some(Money::parse(typed).map_err(|_| CaptureError::CapInvalid)?),
        };
        if cap.is_some_and(|cap| cap < MIN_TAKE_CAP) {
            return Err(CaptureError::CapInvalid);
        }
        self.themes
            .project(project)?
            .ok_or(CaptureError::ProjectNotFound)?;
        let cap = cap.filter(|cap| *cap != DEFAULT_TAKE_CAP);
        self.captures.save_take_cap(project, cap)?;
        Ok(())
    }

    /// Uses take `index`'s final: it is copied into the project's media as
    /// a performed clip, named after its take. Each use adds one.
    pub fn use_performed_take(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<MediaAsset, CaptureError> {
        let (capture, view) = self.performed_take(project, index)?;
        let made = view.final_take.clone().ok_or(CaptureError::NoTakeResult)?;
        let id = MediaAssetId::new();
        let name = format!(
            "{}.mp4",
            self.text_with(
                crate::Text::PerformedClipName,
                &[("n", &(index + 1).to_string())]
            )
        );
        let file = MediaAsset::file_name(id, &name);
        let source = self.files.path(project, &made.file);
        self.files
            .copy_in(project, &file, &source)
            .map_err(|_| CaptureError::NoTakeResult)?;
        let picture = self
            .media
            .probe(&self.files.path(project, &file))
            .ok()
            .and_then(|info| info.video)
            .map(|video| PictureSize::new(video.width, video.height));
        let now = whole_millis(SystemTime::now());
        let asset = MediaAsset {
            id,
            project,
            owner: self.profile.id,
            kind: MediaKind::Video,
            source: AssetSource::Performed,
            file: file.clone(),
            name: name.trim_end_matches(".mp4").to_owned(),
            duration: made.duration,
            // The take's own frame when ffprobe cannot say.
            picture: Some(picture.unwrap_or_else(|| {
                let (width, height) =
                    bardo_domain::control_size(capture.video.width, capture.video.height);
                PictureSize::new(width, height)
            })),
            imported_at: now,
        };
        let take = &capture.takes.as_slice()[index];
        let clip = PerformedClip {
            asset: id,
            project,
            capture: capture.id,
            position: index as u32,
            model: made.model.clone(),
            realistic: made.realistic,
            used_at: now,
            stretch: Some(TakeStretch {
                narration: capture.narration,
                start: take.start,
                end: take.end,
            }),
            face: self.performed_face(&capture, index, &made.model),
        };
        if let Err(error) = self.media_assets.save_performed_clip(&asset, &clip) {
            let _ = self.files.remove(project, &file);
            return Err(error.into());
        }
        // The clip is in the media already: a failure to mark the final
        // used must not make the user add it twice.
        if let Err(error) = self.captures.accept_take_result(
            project,
            made.position,
            TakeStep::Final,
            &made.control,
            Some(now),
        ) {
            tracing::warn!(%error, "the used final could not be marked");
        }
        Ok(asset)
    }

    /// Where the performer's face is over take `index`, in thousandths of
    /// the picture, when `model` keeps the take's framing (#193): the cut
    /// starts a 9:16 crop on it. `None` keeps the crop centered.
    fn performed_face(
        &self,
        capture: &Capture,
        index: usize,
        model: &ClipModelRef,
    ) -> Option<(u16, u16)> {
        let keeps = self
            .take_models()
            .iter()
            .any(|offered| offered.id == *model && offered.keeps_framing);
        if !keeps {
            return None;
        }
        let take = capture.takes.as_slice().get(index)?;
        let file = crate::take_analysis::face_file(capture.id, index as u32, take.revision);
        let bytes = self.files.read(capture.project, &file).ok()?;
        let (x, y) = FaceLandmarks::from_json(&String::from_utf8_lossy(&bytes))
            .ok()?
            .face_center()?;
        let thousandths = |share: f32| (share.clamp(0.0, 1.0) * 1_000.0).round() as u16;
        Some((thousandths(x), thousandths(y)))
    }

    /// Where a draft or final is on disk.
    pub fn take_result_path(&self, made: &TakeResult) -> PathBuf {
        self.files.path(made.project, &made.file)
    }

    /// A player of take `index`'s draft or final with the take's voice
    /// (the control video's sound), never the provider's audio, playing.
    pub fn take_result_player(
        &self,
        project: VideoProjectId,
        index: usize,
        step: TakeStep,
    ) -> Result<ControlPlayer, CaptureError> {
        let (capture, view) = self.performed_take(project, index)?;
        let made = match step {
            TakeStep::Draft => view.draft,
            TakeStep::Final => view.final_take,
            TakeStep::Test => self
                .current_test(project)?
                .1
                .test
                .filter(|test| test.position as usize == index),
        }
        .ok_or(CaptureError::NoTakeResult)?;
        let controls = self.control_views(&capture)?;
        let control = controls
            .get(index)
            .and_then(|view| view.built.clone())
            .ok_or(CaptureError::ControlNotReady)?;
        let size = bardo_domain::control_size(capture.video.width, capture.video.height);
        // A test is the take's first seconds: its voice stops with it.
        let sound = match step {
            TakeStep::Test => made.duration.min(view.length),
            _ => view.length,
        };
        Ok(ControlPlayer::with_sound(
            Arc::clone(&self.media),
            Arc::clone(&self.audio),
            self.take_result_path(&made),
            self.control_video_path(&control),
            (made.duration, sound),
            size,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    use bardo_domain::{
        BodyFraming, ChannelDraft, CostRepository, JobState, PreflightCheck, PreflightOutcome,
        StagingRepository, TEST_TAKE_LENGTH,
    };
    use bardo_media::ffmpeg::{MediaInfo, VideoStream};

    use super::*;
    use crate::captures::tests::{Setup, setup};
    use crate::control_videos::tests::{imported, settled};
    use crate::narrations::tests::{documentary_narrator, wait_done};
    use crate::testing::{FakeTakes, LOOK_IMAGE, TAKE};

    const HIGGSFIELD_KEY: &str = "3f9e2c1a-7b4d-4e8f-9a21-5c6d7e8f9a0b:hf_secret_test_0001";

    #[test]
    fn a_long_prompt_is_cut_on_a_word_and_whitespace_collapses() {
        assert_eq!(fit_prompt("  one\n\ntwo   three "), "one two three");
        let long = "word ".repeat(1_000);
        let cut = fit_prompt(&long);
        assert!(cut.chars().count() <= MAX_TAKE_PROMPT);
        assert!(cut.ends_with("word"));
    }

    #[test]
    fn takes_are_billed_by_the_second_rounded_up() {
        assert_eq!(billed_seconds(Duration::from_millis(12_001)), 13);
        assert_eq!(billed_seconds(Duration::from_secs(12)), 12);
    }

    #[test]
    fn seedance_on_modelark_is_estimated_in_tokens_at_its_rates() {
        let seedance = &bardo_ai::byteplus::take_models()[0];
        let rates = bardo_domain::RateTable::with_changes(crate::costs::default_rates(), &[]);
        let length = Duration::from_secs(10);
        for (step, height, price) in [
            (TakeStep::Draft, 480u64, 6_400_000u64),
            (TakeStep::Final, 1080, 7_000_000),
        ] {
            let calls = step_calls(seedance, PerformedStep::Make(step), length, (720, 1280));
            let tokens = calls[0].output_tokens.unwrap();
            let long = (height * 1280 + 360) / 720;
            assert_eq!(tokens, 20 * height * long * 24 / 1024, "{step:?}");
            // Priced by its tokens alone, nothing else metered.
            assert_eq!(calls[0].video_seconds, None, "{step:?}");
            let usage = Metered {
                output_tokens: tokens,
                ..Metered::default()
            };
            assert_eq!(
                rates.price(Provider::BytePlus, &calls[0].model, &usage),
                Some(Money::from_micros(tokens * price / 1_000_000)),
                "{step:?}"
            );
        }
    }

    /// A capture of two takes with the Higgsfield key saved and the
    /// fake's takes readable as 1280 × 720 videos of 6.5 s.
    fn ready() -> (Setup, VideoProjectId, Capture) {
        let mut s = setup(13);
        s.app
            .save_provider_key(Provider::Higgsfield, HIGGSFIELD_KEY)
            .unwrap();
        s.h.media.probes.lock().unwrap().insert(
            String::from_utf8_lossy(TAKE).into_owned(),
            MediaInfo {
                duration: Duration::from_millis(6_500),
                video: Some(VideoStream {
                    codec: "h264".into(),
                    width: 1280,
                    height: 720,
                    frame_rate: (30, 1),
                }),
                audio: None,
            },
        );
        let (project, capture) = imported(&s);
        settled(&s.app, project);
        (s, project, capture)
    }

    /// Imports a photo as the narrator's look.
    fn with_look(s: &Setup) {
        let path = s._folders.path().join("me.png");
        std::fs::write(&path, LOOK_IMAGE).unwrap();
        let persona = documentary_narrator(&s.app).id;
        s.app.import_look(persona, &path, true).unwrap();
    }

    fn view(s: &Setup, project: VideoProjectId, index: usize) -> PerformedTakeView {
        s.app
            .narration(project)
            .unwrap()
            .performed
            .unwrap()
            .takes
            .into_iter()
            .nth(index)
            .unwrap()
    }

    fn prompt_written(s: &Setup, project: VideoProjectId, index: usize, prompt: &str) {
        s.h.text
            .answers
            .lock()
            .unwrap()
            .push(serde_json::json!({ "prompt": prompt }).to_string());
        let id = s
            .app
            .write_take_prompt(project, index, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&s.app, id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
    }

    fn performed(s: &Setup, project: VideoProjectId) -> PerformedView {
        s.app.narration(project).unwrap().performed.unwrap()
    }

    /// Makes and approves the capture's test take from take `index`.
    fn tested(s: &Setup, project: VideoProjectId, index: usize) -> Job {
        let id = s
            .app
            .make_performed_take(project, index, TakeStep::Test, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&s.app, id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        s.app.approve_take_test(project).unwrap();
        job
    }

    /// Gives the capture an approved test of take `index` without asking
    /// the provider, for tests about drafts and finals.
    fn test_passed(s: &Setup, project: VideoProjectId, index: usize) {
        let view = view(s, project, index);
        let capture = s.app.narration(project).unwrap().capture.unwrap();
        let control = s.app.narration(project).unwrap().controls[index]
            .built
            .clone()
            .unwrap()
            .fingerprint;
        let persona = documentary_narrator(&s.app).id;
        let look = s.h.db.look(persona).unwrap().unwrap();
        let file = format!("take-test-{index}.mp4");
        s.h.files.write(project, &file, TAKE).unwrap();
        let model = view.model.unwrap();
        let saved =
            s.h.db
                .save_take_result(&TakeResult {
                    project,
                    capture: capture.id,
                    position: index as u32,
                    step: TakeStep::Test,
                    control,
                    model: model.id.clone(),
                    priced_model: model.priced_model(TakeStep::Test),
                    prompt: "test".into(),
                    template: None,
                    look: look_print(look.image.bytes()),
                    realistic: look.realistic,
                    request: "test-request".into(),
                    cost: None,
                    file,
                    duration: Duration::from_secs(5),
                    made_at: whole_millis(SystemTime::now()),
                    job: None,
                    accepted_at: Some(whole_millis(SystemTime::now())),
                    seed: None,
                    deadline: None,
                })
                .unwrap();
        assert!(saved);
        assert!(!performed(s, project).needs_test);
    }

    /// Makes take `index`'s `step`; drafts and finals get the capture's
    /// test passed first when they wait for one.
    fn made(s: &Setup, project: VideoProjectId, index: usize, step: TakeStep) -> Job {
        if step != TakeStep::Test && performed(s, project).needs_test {
            test_passed(s, project, index);
        }
        let id = s
            .app
            .make_performed_take(project, index, step, BudgetConsent::Ask)
            .unwrap();
        wait_done(&s.app, id)
    }

    /// The Higgsfield costs of drafts, finals and prompts (tests apart).
    fn costs(s: &Setup, project: VideoProjectId) -> Vec<(CostPurpose, String, u64)> {
        s.h.db
            .project_costs(project)
            .unwrap()
            .into_iter()
            .filter(|record| record.provider == Provider::Higgsfield)
            .filter(|record| record.purpose != CostPurpose::TakeTest)
            .map(|record| (record.purpose, record.model, record.usage.video_seconds))
            .collect()
    }

    #[test]
    fn a_recording_take_is_performed_from_its_prompt_and_used_as_a_clip() {
        let (s, project, capture) = ready();
        let take = &capture.takes.as_slice()[0];
        assert_eq!(view(&s, project, 0).block, Some(PerformedBlock::NoLook));
        with_look(&s);
        let first = view(&s, project, 0);
        assert_eq!(first.block, Some(PerformedBlock::ControlNotApproved));
        assert_eq!(first.model.unwrap().id.model(), FakeTakes::MOTION);
        assert!(first.follows_channel);
        assert!(matches!(
            s.app.write_take_prompt(project, 0, BudgetConsent::Ask),
            Err(CaptureError::Performed(PerformedBlock::ControlNotApproved))
        ));

        s.app.approve_control_video(project, 0).unwrap();
        let approved = view(&s, project, 0);
        assert_eq!(approved.block, None);
        assert_eq!(approved.next, PerformedStep::Prompt);
        assert!(approved.estimate.is_some());
        assert!(matches!(
            s.app
                .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask),
            Err(CaptureError::Prompt(TakePromptError::Empty))
        ));

        prompt_written(&s, project, 0, "  The host   speaks calmly. ");
        // One frame of the control video, from the middle of the take, for
        // the set; the instruction of its mode; the photo's stand-in.
        let request = s.h.text.requests().pop().unwrap();
        assert_eq!(request.images.len(), 1);
        assert_eq!(request.images[0].format, ImageFormat::Jpeg);
        assert!(request.prompt.contains(instruction(TakeMode::Recording)));
        assert!(request.prompt.contains(LOOK_FROM_PHOTO));
        assert!(request.prompt.contains(SET_FROM_FRAME));
        let frames = s.h.media.frames.lock().unwrap().clone();
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].1.as_millis() as u64,
            (take.length() / 2).as_millis() as u64
        );
        assert_eq!(frames[0].2, MAX_FRAME_BYTES);
        let written = view(&s, project, 0);
        assert_eq!(
            written.prompt.as_ref().unwrap().text,
            "The host speaks calmly."
        );
        // Motion control makes no drafts: the final is next.
        assert_eq!(written.next, PerformedStep::Make(TakeStep::Final));
        assert!(matches!(
            s.app
                .make_performed_take(project, 0, TakeStep::Draft, BudgetConsent::Ask),
            Err(CaptureError::NoDrafts)
        ));
        assert!(s.h.takes.staged.lock().unwrap().is_empty());

        s.app
            .edit_take_prompt(project, 0, "The host speaks slowly.")
            .unwrap();
        *s.h.takes.quote.lock().unwrap() = Some(Money::from_micros(1_100_000));
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        let staged = s.h.takes.staged.lock().unwrap().clone();
        assert_eq!(staged.len(), 2);
        assert_eq!(staged[0].0, "video/mp4");
        assert_eq!(staged[1].0, "image/jpeg", "the look is always re-encoded");
        assert_eq!(s.h.media.fitted.lock().unwrap().last(), Some(&10_000_000));
        assert_eq!(*s.h.takes.quotes.lock().unwrap(), 1);
        let requests = s.h.takes.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        let (sent, submission) = &requests[0];
        assert_eq!(sent.model, FakeTakes::MOTION);
        assert_eq!(sent.step, TakeStep::Final);
        assert_eq!(sent.resolution, None);
        assert_eq!(sent.prompt, "The host speaks slowly.");
        assert!(submission.ends_with("-0-final-1"), "{submission}");

        let done = view(&s, project, 0);
        let final_take = done.final_take.clone().unwrap();
        assert_eq!(final_take.duration, Duration::from_millis(6_500));
        assert!(final_take.realistic, "a photo reads as a real person");
        assert_eq!(final_take.cost, Some(Money::from_micros(1_100_000)));
        assert_eq!(s.h.files.read(project, &final_take.file).unwrap(), TAKE);
        assert_eq!(
            costs(&s, project),
            [(
                CostPurpose::TakeFinal,
                FakeTakes::MOTION.to_owned(),
                u64::from(billed_seconds(take.length()))
            )]
        );

        let asset = s.app.use_performed_take(project, 0).unwrap();
        assert_eq!(asset.source, AssetSource::Performed);
        assert_eq!(asset.kind, MediaKind::Video);
        assert_eq!(asset.name, "Performed clip 1");
        assert_eq!(asset.picture, Some(PictureSize::new(1280, 720)));
        assert_eq!(s.h.files.read(project, &asset.file).unwrap(), TAKE);
        let clips = s.h.db.performed_clips(project).unwrap();
        assert_eq!(clips.len(), 1);
        assert!(clips[0].realistic);
        assert_eq!(clips[0].capture, capture.id);
        // It belongs to its take's stretch of the narration; motion control
        // follows the look's framing, so no face to start a crop on.
        let take = &capture.takes.as_slice()[0];
        assert_eq!(
            clips[0].stretch,
            Some(bardo_domain::TakeStretch {
                narration: capture.narration,
                start: take.start,
                end: take.end,
            })
        );
        assert_eq!(clips[0].face, None);
        assert_eq!(view(&s, project, 0).used, 1);
        let again = s.app.use_performed_take(project, 0).unwrap();
        assert_ne!(again.file, asset.file);
        assert_eq!(view(&s, project, 0).used, 2);
        assert!(
            view(&s, project, 0)
                .final_take
                .unwrap()
                .accepted_at
                .is_some()
        );
    }

    #[test]
    fn a_video_edit_take_offers_a_draft_before_its_final() {
        let (s, project, _) = ready();
        with_look(&s);
        let edit = ClipModelRef::new(Provider::Higgsfield, FakeTakes::EDIT).unwrap();
        s.app
            .set_take_model(project, 0, Some(edit.clone()))
            .unwrap();
        // Recording is not a mode it takes: the take goes to its first.
        let controls = settled(&s.app, project);
        assert_eq!(controls[0].mode, TakeMode::Mesh);
        assert!(matches!(
            s.app.set_take_mode(project, 0, TakeMode::Recording),
            Err(CaptureError::Performed(PerformedBlock::ModeNotTaken))
        ));
        s.app.approve_control_video(project, 0).unwrap();
        let picked = view(&s, project, 0);
        assert!(!picked.follows_channel);
        assert!(picked.has_drafts());
        assert_eq!(
            view(&s, project, 1).model.unwrap().id.model(),
            FakeTakes::MOTION
        );

        prompt_written(&s, project, 0, "The host speaks.");
        let request = s.h.text.requests().pop().unwrap();
        assert!(request.prompt.contains(instruction(TakeMode::Mesh)));
        assert_eq!(
            view(&s, project, 0).next,
            PerformedStep::Make(TakeStep::Draft)
        );
        // Its final is a new take: the draft is a check the user may skip,
        // once the capture's test is approved.
        assert!(view(&s, project, 0).skip_draft.is_some());
        assert!(matches!(
            s.app
                .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask),
            Err(CaptureError::TestNotApproved)
        ));

        let job = made(&s, project, 0, TakeStep::Draft);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        let first = view(&s, project, 0).draft_to_review().cloned().unwrap();
        assert_eq!(first.priced_model, "fake/video-edit/480p");
        // Regenerating replaces the draft and its file.
        made(&s, project, 0, TakeStep::Draft);
        let second = view(&s, project, 0).draft.unwrap();
        assert_ne!(second.file, first.file);
        assert!(!s.h.files.exists(project, &first.file));

        s.app.approve_take_draft(project, 0).unwrap();
        assert_eq!(
            view(&s, project, 0).next,
            PerformedStep::Make(TakeStep::Final)
        );
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        let resolutions: Vec<_> =
            s.h.takes
                .submissions()
                .into_iter()
                .map(|sent| (sent.step, sent.resolution))
                .collect();
        assert_eq!(
            resolutions,
            [
                (TakeStep::Draft, Some("480p".to_owned())),
                (TakeStep::Draft, Some("480p".to_owned())),
                (TakeStep::Final, Some("1080p".to_owned())),
            ]
        );
        let purposes: Vec<_> = costs(&s, project)
            .into_iter()
            .map(|(purpose, model, _)| (purpose, model))
            .collect();
        assert_eq!(
            purposes,
            [
                (CostPurpose::TakeDraft, "fake/video-edit/480p".to_owned()),
                (CostPurpose::TakeDraft, "fake/video-edit/480p".to_owned()),
                (CostPurpose::TakeFinal, "fake/video-edit/1080p".to_owned()),
            ]
        );

        // A video edit keeps the take's framing: the clip keeps where the
        // face is, from the take's face mesh.
        s.app.use_performed_take(project, 0).unwrap();
        let face = s.h.db.performed_clips(project).unwrap()[0].face;
        assert!(
            face.is_some_and(|(x, y)| x <= 1_000 && y <= 1_000),
            "{face:?}"
        );

        s.app.back_to_take_drafts(project, 0).unwrap();
        assert_eq!(
            view(&s, project, 0).next,
            PerformedStep::Make(TakeStep::Draft)
        );

        // A prompt holds for the model it was written for.
        s.app.set_take_model(project, 0, None).unwrap();
        settled(&s.app, project);
        let back = view(&s, project, 0);
        assert!(back.follows_channel);
        assert_eq!(back.prompt, None);
        assert_eq!(back.draft, None);
    }

    #[test]
    fn a_refused_take_says_so_and_a_retry_submits_it_again() {
        let (s, project, _) = ready();
        with_look(&s);
        s.app.approve_control_video(project, 0).unwrap();
        prompt_written(&s, project, 0, "The host speaks.");
        s.h.takes.refuse.store(true, Ordering::SeqCst);
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::Declined);
        assert_eq!(failure.detail, REFUSED);
        assert_eq!(view(&s, project, 0).final_take, None);
        assert!(
            costs(&s, project).is_empty(),
            "nothing made, nothing billed"
        );

        s.h.takes.refuse.store(false, Ordering::SeqCst);
        s.app.retry_job(job.id()).unwrap();
        let job = wait_done(&s.app, job.id());
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        let submissions: Vec<_> =
            s.h.takes
                .requests
                .lock()
                .unwrap()
                .iter()
                .map(|(_, submission)| submission.clone())
                .collect();
        assert_eq!(submissions.len(), 2);
        assert!(submissions[0].ends_with("-1"));
        assert!(submissions[1].ends_with("-2"));
    }

    #[test]
    fn cancelling_stops_the_waiting_and_a_retry_polls_the_same_request() {
        let (s, project, _) = ready();
        with_look(&s);
        s.app.approve_control_video(project, 0).unwrap();
        prompt_written(&s, project, 0, "The host speaks.");
        test_passed(&s, project, 0);
        s.h.takes.hold(true);
        let id = s
            .app
            .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while s.h.takes.polls.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "never polled");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(view(&s, project, 0).is_busy());
        let busy = |result: Result<(), CaptureError>| matches!(result, Err(CaptureError::TakeBusy));
        assert!(busy(s.app.edit_take_prompt(project, 0, "Another prompt.")));
        assert!(busy(s.app.set_take_model(
            project,
            0,
            Some(FakeTakes::edit())
        )));
        assert!(busy(s.app.set_take_mode(project, 0, TakeMode::Mesh)));
        assert!(busy(s.app.build_control_video(project, 0)));
        assert!(busy(s.app.move_take_boundary(
            project,
            0,
            bardo_domain::BoundaryMove::Later
        )));
        assert!(busy(s.app.remove_capture(project)), "nothing paid is lost");
        s.app.cancel_job(id).unwrap();
        let job = wait_done(&s.app, id);
        assert_eq!(job.state(), JobState::Cancelled);
        assert!(view(&s, project, 0).pending);
        assert!(
            matches!(
                s.app
                    .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask),
                Err(CaptureError::TakePending)
            ),
            "checking again comes before paying for another"
        );

        s.h.takes.hold(false);
        s.app.check_take_again(project, 0).unwrap();
        let job = wait_done(&s.app, id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(s.h.takes.requests.lock().unwrap().len(), 1, "paid once");
        assert_eq!(s.h.takes.staged.lock().unwrap().len(), 2, "staged once");
        assert_eq!(*s.h.takes.quotes.lock().unwrap(), 1);
        let polls = s.h.takes.polls.lock().unwrap().clone();
        assert!(polls.iter().all(|request| request == "take-0"));
        assert!(!view(&s, project, 0).pending);
    }

    #[test]
    fn a_restart_while_a_take_is_made_polls_the_same_request() {
        let (s, project, _) = ready();
        with_look(&s);
        s.app.approve_control_video(project, 0).unwrap();
        prompt_written(&s, project, 0, "The host speaks.");
        test_passed(&s, project, 0);
        s.h.takes.hold(true);
        let id = s
            .app
            .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while s.h.takes.polls.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "never polled");
            std::thread::sleep(Duration::from_millis(2));
        }

        let s = s.restart();
        s.h.takes.hold(false);
        let job = wait_done(&s.app, id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(s.h.takes.requests.lock().unwrap().len(), 1, "paid once");
        assert_eq!(s.h.takes.staged.lock().unwrap().len(), 2, "staged once");
        assert_eq!(*s.h.takes.quotes.lock().unwrap(), 1);
        let polls = s.h.takes.polls.lock().unwrap().clone();
        assert!(polls.iter().all(|request| request == "take-0"));
        assert_eq!(costs(&s, project).len(), 1, "counted once");
        assert!(view(&s, project, 0).final_take.is_some());
    }

    #[test]
    fn a_take_that_fails_on_higgsfield_is_never_sent_again_on_its_own() {
        let (s, project, _) = ready();
        with_look(&s);
        s.app.approve_control_video(project, 0).unwrap();
        prompt_written(&s, project, 0, "The host speaks.");
        s.h.takes.fail.store(true, Ordering::SeqCst);
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert!(!failure.kind.is_transient(), "{failure:?}");
        assert!(failure.detail.contains("the generation failed"));
        assert_eq!(
            s.h.takes.requests.lock().unwrap().len(),
            1,
            "a new request is the user's call"
        );
        let after = view(&s, project, 0);
        assert!(!after.pending, "nothing to check again");
        assert!(costs(&s, project).is_empty());

        s.h.takes.fail.store(false, Ordering::SeqCst);
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(s.h.takes.requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_take_made_for_another_model_never_lands_on_the_take() {
        let (s, project, _) = ready();
        with_look(&s);
        s.app.approve_control_video(project, 0).unwrap();
        prompt_written(&s, project, 0, "The host speaks.");
        test_passed(&s, project, 0);
        s.h.takes.hold(true);
        let id = s
            .app
            .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while s.h.takes.polls.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline, "never polled");
            std::thread::sleep(Duration::from_millis(2));
        }
        s.app.cancel_job(id).unwrap();
        wait_done(&s.app, id);
        s.app
            .set_take_model(project, 0, Some(FakeTakes::edit()))
            .unwrap();
        settled(&s.app, project);
        let now = view(&s, project, 0);
        assert!(now.job.is_none() && !now.pending, "the old job is history");

        // Retried from the Jobs screen: Higgsfield's take is paid, not kept.
        s.h.takes.hold(false);
        s.app.retry_job(id).unwrap();
        let job = wait_done(&s.app, id);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::NotAllowed);
        assert_eq!(costs(&s, project).len(), 1);
        assert!(
            s.app
                .captures
                .take_results(project)
                .unwrap()
                .iter()
                .all(|made| made.step == TakeStep::Test),
            "only the capture's test"
        );
    }

    #[test]
    fn a_channel_model_that_does_not_take_the_mode_blocks_and_new_captures_start_in_its_mode() {
        let (s, project, _) = ready();
        with_look(&s);
        let channel = s.app.themes.project(project).unwrap().unwrap().channel;
        let details = s.app.channels.get(channel).unwrap().unwrap().details;
        s.app
            .update_channel(
                channel,
                ChannelDraft {
                    take_model: Some(
                        ClipModelRef::new(Provider::Higgsfield, FakeTakes::EDIT).unwrap(),
                    ),
                    ..ChannelDraft::from(&details)
                },
            )
            .unwrap();
        let blocked = view(&s, project, 0);
        assert_eq!(blocked.block, Some(PerformedBlock::ModeNotTaken));
        assert_eq!(blocked.model.unwrap().id.model(), FakeTakes::EDIT);

        // Another import starts in the channel model's first mode.
        let (_, capture, _) = s.imported_at(
            project,
            crate::CaptureVoiceChoice::Persona,
            crate::control_videos::tests::SIZE,
        );
        assert!(
            capture
                .takes
                .as_slice()
                .iter()
                .all(|take| take.mode == TakeMode::Mesh)
        );
    }

    const BYTEPLUS_FIELDS: [&str; 5] = [
        "0b6c2f6e-5a1d-4f3e-9d2a-7c8b9e0f1a2b",
        "AKLTNjA0ZTVkMmY0YmQ2",
        "TVRWa1pUVTBNRGM9Pz8+/abc=",
        "bardo-takes",
        "ap-southeast-1",
    ];

    /// `ready` with the BytePlus fields saved, take 0 on the ModelArk-like
    /// model and the narrator's photo look held as a verified likeness,
    /// its control video approved and its prompt written.
    fn on_modelark() -> (Setup, VideoProjectId, Capture) {
        let (mut s, project, capture) = ready();
        let [api, ak, sk, bucket, region] = BYTEPLUS_FIELDS;
        s.app
            .save_byteplus_key(api, ak, sk, bucket, region)
            .unwrap();
        with_look(&s);
        s.app
            .set_take_model(project, 0, Some(crate::testing::FakeModelArk::edit()))
            .unwrap();
        settled(&s.app, project);
        s.app.approve_control_video(project, 0).unwrap();
        let persona = documentary_narrator(&s.app).id;
        assert_eq!(
            view(&s, project, 0).block,
            Some(PerformedBlock::NoVerifiedLikeness),
            "a photo goes only as a verified likeness"
        );
        assert!(matches!(
            s.app.set_look_asset(
                persona,
                Provider::BytePlus,
                LookAssetKind::VirtualPortrait,
                "asset-1"
            ),
            Err(crate::LookError::Asset(
                bardo_domain::LookAssetError::KindNotForLook
            ))
        ));
        s.app
            .set_look_asset(
                persona,
                Provider::BytePlus,
                LookAssetKind::VerifiedLikeness,
                " asset://asset-20261023-me ",
            )
            .unwrap();
        assert_eq!(view(&s, project, 0).block, None);
        prompt_written(&s, project, 0, "The host speaks.");
        // A 30-second ModelArk projection is over the default cap.
        s.app.set_take_cap(project, "100").unwrap();
        (s, project, capture)
    }

    fn byteplus_costs(s: &Setup, project: VideoProjectId) -> Vec<(CostPurpose, String, u64)> {
        s.h.db
            .project_costs(project)
            .unwrap()
            .into_iter()
            .filter(|record| record.provider == Provider::BytePlus)
            .filter(|record| record.purpose != CostPurpose::TakeTest)
            .map(|record| (record.purpose, record.model, record.usage.output_tokens))
            .collect()
    }

    #[test]
    fn a_modelark_final_finishes_its_draft_and_the_drafts_files_go() {
        let (s, project, capture) = on_modelark();
        let fake = &s.h.modelark;
        let before = SystemTime::now();
        // Estimated from the take's frame, in tokens, and priced by them
        // alone.
        s.app
            .save_rate(
                Provider::BytePlus,
                crate::testing::FakeModelArk::EDIT,
                bardo_domain::Meter::OutputTokens,
                "6.40",
            )
            .unwrap();
        let waiting = view(&s, project, 0);
        assert_eq!(waiting.next, PerformedStep::Make(TakeStep::Draft));
        let estimate = waiting.estimate.unwrap();
        assert!(estimate.providers[0].amount.is_some(), "{estimate:?}");
        assert!(estimate.total() > Money::ZERO);
        let job = made(&s, project, 0, TakeStep::Draft);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        // Only the control video is staged: the library holds the look.
        let staged = fake.staged.lock().unwrap().clone();
        assert_eq!(staged.len(), 1);
        let sent = fake.requests.lock().unwrap()[0].clone();
        assert_eq!(sent.look.0, "asset://asset-20261023-me");
        assert_eq!(sent.video.0, staged[0]);
        assert_eq!(sent.draft_task, None);
        let draft = view(&s, project, 0).draft.unwrap();
        assert_eq!(draft.seed, Some(crate::testing::FakeModelArk::SEED));
        let deadline = draft.deadline.unwrap();
        let week = Duration::from_secs(7 * 24 * 3600);
        assert!(deadline >= before + week - Duration::from_secs(1));
        assert!(deadline <= SystemTime::now() + week);
        assert!(draft.realistic);
        assert!(
            fake.unstaged.lock().unwrap().is_empty(),
            "kept for the final"
        );

        s.app.approve_take_draft(project, 0).unwrap();
        let approved = view(&s, project, 0);
        assert_eq!(approved.next, PerformedStep::Make(TakeStep::Final));
        assert!(!approved.expired);
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        let requests = fake.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        let finished = &requests[1];
        assert_eq!(finished.draft_task, Some(ClipHandle(draft.request.clone())));
        assert_eq!(finished.resolution.as_deref(), Some("1080p"));
        assert_eq!(fake.staged.lock().unwrap().len(), 1, "nothing staged again");
        let final_take = view(&s, project, 0).final_take.unwrap();
        assert_eq!(final_take.look, draft.look);
        assert!(final_take.realistic);
        assert_eq!(final_take.deadline, None);
        // The draft's files went once the final was made.
        assert_eq!(*fake.unstaged.lock().unwrap(), staged);
        assert!(
            s.h.db
                .staged_due(s.app.profile().id, SystemTime::now() + week * 2)
                .unwrap()
                .is_empty()
        );
        // The tokens ModelArk reported are what Costs records.
        let edit = crate::testing::FakeModelArk::EDIT;
        assert_eq!(
            byteplus_costs(&s, project),
            [
                (
                    CostPurpose::TakeDraft,
                    format!("{edit}/480p"),
                    crate::testing::FakeModelArk::TOKENS
                ),
                (
                    CostPurpose::TakeFinal,
                    format!("{edit}/1080p"),
                    crate::testing::FakeModelArk::TOKENS
                ),
            ]
        );
        let _ = capture;
    }

    #[test]
    fn a_look_asset_holds_for_its_look_only() {
        let (s, project, _) = on_modelark();
        let persona = documentary_narrator(&s.app).id;
        let held = s.app.persona_look(persona).unwrap().assets;
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].0, Provider::BytePlus);
        assert_eq!(held[0].1.as_ref().unwrap().id, "asset-20261023-me");
        assert!(matches!(
            s.app.set_look_asset(
                persona,
                Provider::Higgsfield,
                LookAssetKind::VerifiedLikeness,
                "asset-1"
            ),
            Err(crate::LookError::NoAssetLibrary(Provider::Higgsfield))
        ));
        // Another photo is another look: the asset no longer stands for it.
        let path = s._folders.path().join("me-again.png");
        let mut other = LOOK_IMAGE.to_vec();
        other.extend_from_slice(b"another");
        std::fs::write(&path, &other).unwrap();
        s.app.import_look(persona, &path, true).unwrap();
        assert_eq!(s.app.persona_look(persona).unwrap().assets[0].1, None);
        assert_eq!(
            view(&s, project, 0).block,
            Some(PerformedBlock::NoVerifiedLikeness)
        );
        s.app
            .remove_look_asset(persona, Provider::BytePlus)
            .unwrap();
        assert_eq!(
            s.h.db.look_asset(persona, Provider::BytePlus).unwrap(),
            None
        );
    }

    #[test]
    fn a_draft_past_its_deadline_offers_only_a_new_draft() {
        let (s, project, _) = on_modelark();
        made(&s, project, 0, TakeStep::Draft);
        s.app.approve_take_draft(project, 0).unwrap();
        let draft = view(&s, project, 0).draft.unwrap();
        s.h.db
            .save_take_result(&TakeResult {
                deadline: Some(SystemTime::now() - Duration::from_secs(1)),
                ..draft
            })
            .unwrap();
        let expired = view(&s, project, 0);
        assert!(expired.expired);
        assert_eq!(expired.next, PerformedStep::Make(TakeStep::Draft));
        assert!(matches!(
            s.app
                .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask),
            Err(CaptureError::DraftExpired)
        ));
        let job = made(&s, project, 0, TakeStep::Draft);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert!(!view(&s, project, 0).expired);
        // The draft it replaced gave up its files.
        let staged = s.h.modelark.staged.lock().unwrap().clone();
        assert_eq!(staged.len(), 2);
        assert_eq!(*s.h.modelark.unstaged.lock().unwrap(), [staged[0].clone()]);
    }

    #[test]
    fn a_send_whose_answer_was_lost_is_found_in_the_task_list() {
        let (s, project, _) = on_modelark();
        let fake = &s.h.modelark;
        fake.lose_answer.store(true, Ordering::SeqCst);
        let job = made(&s, project, 0, TakeStep::Draft);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(fake.requests.lock().unwrap().len(), 1, "paid once");
        assert_eq!(fake.lookups.lock().unwrap().len(), 1);
        assert_eq!(view(&s, project, 0).draft.unwrap().request, "cgt-task-1");
    }

    #[test]
    fn a_lost_send_leaves_out_what_another_take_holds() {
        let (s, project, _) = on_modelark();
        let fake = &s.h.modelark;
        made(&s, project, 0, TakeStep::Draft);
        s.app
            .set_take_model(project, 1, Some(crate::testing::FakeModelArk::edit()))
            .unwrap();
        settled(&s.app, project);
        s.app.approve_control_video(project, 1).unwrap();
        prompt_written(&s, project, 1, "The host leans in.");
        // The list shows take 1's draft too; take 2 adopts only its own.
        fake.lose_answer.store(true, Ordering::SeqCst);
        let job = made(&s, project, 1, TakeStep::Draft);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(fake.requests.lock().unwrap().len(), 2, "each paid once");
        assert_eq!(view(&s, project, 1).draft.unwrap().request, "cgt-task-2");
        assert_eq!(view(&s, project, 0).draft.unwrap().request, "cgt-task-1");
    }

    #[test]
    fn a_draft_makes_one_final_and_then_a_new_draft_is_due() {
        let (s, project, _) = on_modelark();
        made(&s, project, 0, TakeStep::Draft);
        s.app.approve_take_draft(project, 0).unwrap();
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        // The same draft would make the same final, and its files are gone.
        let finished = view(&s, project, 0);
        assert!(finished.finished);
        assert_eq!(finished.next, PerformedStep::Make(TakeStep::Draft));
        assert!(matches!(
            s.app
                .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask),
            Err(CaptureError::DraftFinished)
        ));
        // A new draft, once approved, can be finished again.
        made(&s, project, 0, TakeStep::Draft);
        assert!(!view(&s, project, 0).finished);
        s.app.approve_take_draft(project, 0).unwrap();
        assert_eq!(
            view(&s, project, 0).next,
            PerformedStep::Make(TakeStep::Final)
        );
    }

    #[test]
    fn a_draft_past_its_deadline_cannot_be_approved() {
        let (s, project, _) = on_modelark();
        made(&s, project, 0, TakeStep::Draft);
        let draft = view(&s, project, 0).draft.unwrap();
        s.h.db
            .save_take_result(&TakeResult {
                deadline: Some(SystemTime::now() - Duration::from_secs(1)),
                ..draft
            })
            .unwrap();
        assert!(matches!(
            s.app.approve_take_draft(project, 0),
            Err(CaptureError::DraftExpired)
        ));
    }

    #[test]
    fn a_send_without_an_answer_is_checked_again_never_sent_anew() {
        let (s, project, _) = on_modelark();
        let fake = &s.h.modelark;
        // Sent, its answer lost, and the task list out of reach each time.
        fake.lose_answer.store(true, Ordering::SeqCst);
        fake.lookup_fails.store(true, Ordering::SeqCst);
        let job = made(&s, project, 0, TakeStep::Draft);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(fake.requests.lock().unwrap().len(), 1);
        assert!(view(&s, project, 0).pending);
        assert!(matches!(
            s.app
                .make_performed_take(project, 0, TakeStep::Draft, BudgetConsent::Ask),
            Err(CaptureError::TakePending)
        ));
        // Checking again finds it, paying nothing new.
        fake.lookup_fails.store(false, Ordering::SeqCst);
        s.app.check_take_again(project, 0).unwrap();
        let ended = wait_done(&s.app, job.id());
        assert_eq!(ended.state(), JobState::Done, "{:?}", ended.failure());
        assert_eq!(fake.requests.lock().unwrap().len(), 1, "paid once");
    }

    #[test]
    fn a_modelark_refusal_says_why_and_its_files_go() {
        let (s, project, _) = on_modelark();
        let fake = &s.h.modelark;
        fake.refuse.store(true, Ordering::SeqCst);
        let job = made(&s, project, 0, TakeStep::Draft);
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::Declined);
        assert_eq!(
            failure.detail,
            "InputVideoSensitiveContentDetected: the input video may contain a real face"
        );
        assert_eq!(*fake.unstaged.lock().unwrap(), *fake.staged.lock().unwrap());
        assert!(byteplus_costs(&s, project).is_empty());
    }

    #[test]
    fn a_removed_capture_gives_up_its_staged_files() {
        let (s, project, _) = on_modelark();
        let fake = &s.h.modelark;
        made(&s, project, 0, TakeStep::Draft);
        settled(&s.app, project);
        // The bucket is down: the files wait for a later cleanup.
        fake.keep_files.store(true, Ordering::SeqCst);
        s.app.remove_capture(project).unwrap();
        let cleanup = s
            .app
            .jobs()
            .into_iter()
            .find(|job| job.kind() == JobKind::StagingCleanup)
            .expect("a cleanup is queued");
        let ended = wait_done(&s.app, cleanup.id());
        assert_eq!(ended.state(), JobState::Failed);
        assert!(fake.unstaged.lock().unwrap().is_empty());
        fake.keep_files.store(false, Ordering::SeqCst);
        s.app.retry_job(cleanup.id()).unwrap();
        let ended = wait_done(&s.app, cleanup.id());
        assert_eq!(ended.state(), JobState::Done, "{:?}", ended.failure());
        assert_eq!(*fake.unstaged.lock().unwrap(), *fake.staged.lock().unwrap());
        assert!(
            s.h.db
                .staged_due(s.app.profile().id, SystemTime::now())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_removed_capture_takes_its_drafts_and_finals_along() {
        let (s, project, _) = ready();
        with_look(&s);
        s.app.approve_control_video(project, 0).unwrap();
        prompt_written(&s, project, 0, "The host speaks.");
        made(&s, project, 0, TakeStep::Final);
        let made = view(&s, project, 0).final_take.unwrap();
        let used = s.app.use_performed_take(project, 0).unwrap();
        settled(&s.app, project);
        s.app.remove_capture(project).unwrap();
        assert!(!s.h.files.exists(project, &made.file));
        // The clip the user chose outlives the capture.
        assert!(s.h.files.exists(project, &used.file));
        assert_eq!(s.h.db.performed_clips(project).unwrap().len(), 1);
    }

    const KLING_KEY: &str = "kling-test-key-0001";

    /// The take's analysis has finished and the look's framing is measured.
    fn measured(s: &Setup, project: VideoProjectId) {
        let _ = performed(s, project);
        if let Some(job) = s
            .app
            .jobs()
            .into_iter()
            .find(|job| job.kind() == JobKind::LookFraming)
        {
            let job = wait_done(&s.app, job.id());
            assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        }
    }

    /// Take 0 of a two-take capture on Kling's 3.0 720p model, its control
    /// video approved and its prompt written; with the Higgsfield key saved
    /// when `higgsfield`.
    fn on_kling(higgsfield: bool) -> (Setup, VideoProjectId) {
        let (mut s, project, _) = ready();
        if !higgsfield {
            s.app.remove_provider_key(Provider::Higgsfield).unwrap();
        }
        s.app.save_provider_key(Provider::Kling, KLING_KEY).unwrap();
        with_look(&s);
        s.app
            .set_take_model(project, 0, Some(crate::testing::FakeKling::motion()))
            .unwrap();
        settled(&s.app, project);
        s.app.approve_control_video(project, 0).unwrap();
        (s, project)
    }

    fn kling_costs(s: &Setup, project: VideoProjectId) -> Vec<(CostPurpose, String, u64)> {
        s.h.db
            .project_costs(project)
            .unwrap()
            .into_iter()
            .filter(|record| record.provider == Provider::Kling)
            .map(|record| (record.purpose, record.model, record.usage.video_seconds))
            .collect()
    }

    #[test]
    fn a_capture_waits_for_a_short_test_take_before_its_drafts_and_finals() {
        let (s, project, _) = ready();
        with_look(&s);
        s.app.approve_control_video(project, 0).unwrap();
        let waiting = performed(&s, project);
        assert!(waiting.needs_test);
        assert_eq!(waiting.test_take, None, "no take has a prompt yet");
        prompt_written(&s, project, 0, "The host speaks.");
        let ready = performed(&s, project);
        assert_eq!(ready.test_take, Some(0));
        assert!(ready.test_estimate.is_some());
        assert!(matches!(
            s.app
                .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask),
            Err(CaptureError::TestNotApproved)
        ));

        let job = tested_unapproved(&s, project);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        // Five seconds of the take, cut from its control video.
        let piece = s.h.media.controls.lock().unwrap().last().cloned().unwrap();
        assert_eq!(piece.voice.length, TEST_TAKE_LENGTH);
        let sent = s.h.takes.submissions();
        assert_eq!(sent.len(), 1);
        // A model without drafts makes its test as a final would.
        assert_eq!(sent[0].step, TakeStep::Final);
        let test = performed(&s, project).test.unwrap();
        assert_eq!(test.step, TakeStep::Test);
        assert_eq!(test.deadline, None);
        assert!(s.h.files.exists(project, &test.file));
        let tests: Vec<_> =
            s.h.db
                .project_costs(project)
                .unwrap()
                .into_iter()
                .filter(|record| record.purpose == CostPurpose::TakeTest)
                .map(|record| record.usage.video_seconds)
                .collect();
        assert_eq!(tests, [5]);
        // Still waiting for the user's look at it.
        assert!(performed(&s, project).needs_test);
        assert!(s.app.take_result_player(project, 0, TakeStep::Test).is_ok());

        // Discarded, it goes with its file; another replaces it.
        s.app.discard_take_test(project).unwrap();
        assert!(!s.h.files.exists(project, &test.file));
        assert_eq!(performed(&s, project).test, None);
        tested_unapproved(&s, project);
        tested_unapproved(&s, project);
        let tests: Vec<_> = s
            .app
            .captures
            .take_results(project)
            .unwrap()
            .into_iter()
            .filter(|made| made.step == TakeStep::Test)
            .collect();
        assert_eq!(tests.len(), 1, "one test per capture");
        s.app.approve_take_test(project).unwrap();
        assert!(!performed(&s, project).needs_test);
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());

        // Another look is another test, though a final was made.
        let persona = documentary_narrator(&s.app).id;
        let path = s._folders.path().join("me-again.png");
        let mut other = LOOK_IMAGE.to_vec();
        other.extend_from_slice(b"another");
        std::fs::write(&path, &other).unwrap();
        s.app.import_look(persona, &path, true).unwrap();
        let again = performed(&s, project);
        assert_eq!(again.test, None);
        assert!(again.needs_test);
    }

    /// Makes the capture's test take from take 0, leaving it unapproved.
    fn tested_unapproved(s: &Setup, project: VideoProjectId) -> Job {
        let id = s
            .app
            .make_performed_take(project, 0, TakeStep::Test, BudgetConsent::Ask)
            .unwrap();
        wait_done(&s.app, id)
    }

    #[test]
    fn kling_reads_the_video_staged_with_another_provider_and_the_look_inline() {
        let (mut s, project) = on_kling(false);
        assert_eq!(
            view(&s, project, 0).block,
            Some(PerformedBlock::NoVideoHost),
            "Kling keeps no files and no storage key is saved"
        );
        s.app
            .save_provider_key(Provider::Higgsfield, HIGGSFIELD_KEY)
            .unwrap();
        assert_eq!(view(&s, project, 0).block, None);
        prompt_written(&s, project, 0, "The host speaks.");
        let job = tested(&s, project, 0);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(*s.h.kling.staged.lock().unwrap(), 0);
        let staged = s.h.takes.staged.lock().unwrap().clone();
        assert_eq!(staged.len(), 1, "the video alone: {staged:?}");
        assert_eq!(staged[0].0, "video/mp4");
        let sent = s.h.kling.submissions();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].video_link.as_deref(), Some("https://fake/input-1"));
        assert!(sent[0].look.0.starts_with("inline:"), "{}", sent[0].look.0);
        assert!(
            sent[0]
                .look_bytes
                .as_ref()
                .is_some_and(|bytes| !bytes.is_empty())
        );
        // A Kling test is a 2.6 draft, the cheapest tier.
        assert_eq!(sent[0].step, TakeStep::Draft);
        assert_eq!(
            kling_costs(&s, project),
            [(
                CostPurpose::TakeTest,
                bardo_ai::kling_takes::DRAFT_MODEL.to_owned(),
                5
            )]
        );

        // Storage on BytePlus goes first, signed for Kling to read.
        let [api, ak, sk, bucket, region] = BYTEPLUS_FIELDS;
        s.app
            .save_byteplus_key(api, ak, sk, bucket, region)
            .unwrap();
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        let staged = s.h.modelark.staged.lock().unwrap().clone();
        assert_eq!(staged.len(), 1);
        let sent = s.h.kling.submissions().pop().unwrap();
        assert_eq!(sent.video_link, Some(format!("{}&linked", staged[0])));
        // A final is a new take: its staged video goes once it is made.
        assert_eq!(*s.h.modelark.unstaged.lock().unwrap(), staged);
    }

    #[test]
    fn a_kling_draft_is_optional_and_made_on_2_6() {
        let (s, project) = on_kling(true);
        prompt_written(&s, project, 0, "The host speaks.");
        tested(&s, project, 0);
        let next = view(&s, project, 0);
        assert_eq!(next.next, PerformedStep::Make(TakeStep::Draft));
        let skip = next.skip_draft.clone().unwrap();
        assert!(skip.total() > next.estimate.clone().unwrap().total());
        let job = made(&s, project, 0, TakeStep::Draft);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        // Not approved, the draft does not hold the final back.
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        let steps: Vec<_> =
            s.h.kling
                .submissions()
                .iter()
                .map(|sent| sent.step)
                .collect();
        assert_eq!(steps, [TakeStep::Draft, TakeStep::Draft, TakeStep::Final]);
        let costs: Vec<_> = kling_costs(&s, project)
            .into_iter()
            .map(|(purpose, model, _)| (purpose, model))
            .collect();
        assert_eq!(
            costs,
            [
                (
                    CostPurpose::TakeTest,
                    bardo_ai::kling_takes::DRAFT_MODEL.to_owned()
                ),
                (
                    CostPurpose::TakeDraft,
                    bardo_ai::kling_takes::DRAFT_MODEL.to_owned()
                ),
                (
                    CostPurpose::TakeFinal,
                    bardo_ai::kling_takes::TAKE_MODEL_720P.to_owned()
                ),
            ]
        );
        assert!(view(&s, project, 0).final_take.is_some());
    }

    #[test]
    fn a_take_over_the_videos_cap_waits_for_the_user() {
        let (s, project) = on_kling(true);
        prompt_written(&s, project, 0, "The host speaks.");
        assert!(matches!(
            s.app.set_take_cap(project, "0.50"),
            Err(CaptureError::CapInvalid)
        ));
        assert!(matches!(
            s.app.set_take_cap(project, "lots"),
            Err(CaptureError::CapInvalid)
        ));
        let spend = performed(&s, project).spend;
        assert_eq!(spend.cap, DEFAULT_TAKE_CAP);
        assert!(spend.to_come > Money::ZERO);
        s.app.set_take_cap(project, "1").unwrap();
        let low = performed(&s, project);
        assert!(low.own_cap);
        assert_eq!(low.spend.cap, Money::from_micros(1_000_000));
        let Err(CaptureError::OverCap(over, _)) =
            s.app
                .make_performed_take(project, 0, TakeStep::Test, BudgetConsent::Ask)
        else {
            panic!("not over the cap");
        };
        // The test is on top of what the takes still need.
        let test = low.test_estimate.unwrap().total();
        assert_eq!(over.to_come, low.spend.to_come.saturating_add(test));
        assert!(s.h.kling.submissions().is_empty());
        let id = s
            .app
            .make_performed_take(project, 0, TakeStep::Test, BudgetConsent::Confirmed)
            .unwrap();
        let job = wait_done(&s.app, id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        let spent = performed(&s, project).spend;
        assert_eq!(spent.spent, test.saturating_add(prompt_cost(&s, project)));
        // Back to the default, stored as none.
        s.app.set_take_cap(project, " ").unwrap();
        assert!(!performed(&s, project).own_cap);
        s.app.set_take_cap(project, "10").unwrap();
        assert!(!performed(&s, project).own_cap);
        assert_eq!(s.h.db.take_cap(project).unwrap(), None);
    }

    #[test]
    fn a_take_never_sent_is_retried_from_the_jobs_panel_only_once_tested() {
        let (s, project, _) = ready();
        with_look(&s);
        s.app.approve_control_video(project, 0).unwrap();
        prompt_written(&s, project, 0, "The host speaks.");
        s.h.takes.refuse.store(true, Ordering::SeqCst);
        let job = made(&s, project, 0, TakeStep::Final);
        assert_eq!(job.state(), JobState::Failed);
        s.h.takes.refuse.store(false, Ordering::SeqCst);

        // Another look asks for another test: the capture asks for it, so
        // the jobs panel does not send the final.
        let persona = documentary_narrator(&s.app).id;
        let path = s._folders.path().join("me-again.png");
        let mut other = LOOK_IMAGE.to_vec();
        other.extend_from_slice(b"another");
        std::fs::write(&path, &other).unwrap();
        s.app.import_look(persona, &path, true).unwrap();
        settled(&s.app, project);
        assert!(performed(&s, project).needs_test);
        let sent = s.h.takes.submissions().len();
        assert!(matches!(
            s.app.retry_job(job.id()),
            Err(crate::jobs::JobActionError::TakeFromItsPage)
        ));
        settled(&s.app, project);
        assert_eq!(s.h.takes.submissions().len(), sent, "nothing sent again");

        test_passed(&s, project, 0);
        s.app.retry_job(job.id()).unwrap();
        let job = wait_done(&s.app, job.id());
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
    }

    /// What the take prompts written for `project` cost.
    fn prompt_cost(s: &Setup, project: VideoProjectId) -> Money {
        let rates = s.app.cost_book.rates().unwrap();
        s.h.db
            .project_costs(project)
            .unwrap()
            .into_iter()
            .filter(|record| record.purpose == CostPurpose::TakePrompt)
            .map(|record| record.priced_with(&rates).cost.amount())
            .sum()
    }

    #[test]
    fn a_take_that_fails_a_preflight_check_waits_until_sent_anyway() {
        let mut s = setup(13);
        // The performer walked out of the shot.
        *s.h.vision.seen.faces.lock().unwrap() = Some(0);
        s.app
            .save_provider_key(Provider::Higgsfield, HIGGSFIELD_KEY)
            .unwrap();
        s.h.media.probes.lock().unwrap().insert(
            String::from_utf8_lossy(TAKE).into_owned(),
            MediaInfo {
                duration: Duration::from_millis(6_500),
                video: Some(VideoStream {
                    codec: "h264".into(),
                    width: 1280,
                    height: 720,
                    frame_rate: (30, 1),
                }),
                audio: None,
            },
        );
        let (project, _) = imported(&s);
        settled(&s.app, project);
        *s.h.vision.seen.faces.lock().unwrap() = None;
        with_look(&s);
        s.app.approve_control_video(project, 0).unwrap();
        prompt_written(&s, project, 0, "The host speaks.");
        measured(&s, project);
        let held = view(&s, project, 0);
        let preflight = held.preflight.clone().unwrap();
        assert_eq!(
            preflight.failed().collect::<Vec<_>>(),
            [PreflightCheck::InFrame, PreflightCheck::FaceSize]
        );
        assert_eq!(
            preflight.outcome(PreflightCheck::OnePerson),
            Some(PreflightOutcome::Passed)
        );
        assert_eq!(
            preflight.outcome(PreflightCheck::Framing),
            Some(PreflightOutcome::Unknown),
            "no face to tell the recording's framing by"
        );
        assert_eq!(preflight.look, Some(BodyFraming::HalfBody));
        assert!(held.preflight_holds());
        assert!(matches!(
            s.app
                .make_performed_take(project, 0, TakeStep::Test, BudgetConsent::Ask),
            Err(CaptureError::PreflightFailed)
        ));
        assert!(s.h.takes.submissions().is_empty());
        s.app.send_take_anyway(project, 0).unwrap();
        let sent = view(&s, project, 0);
        assert!(sent.overridden);
        assert!(!sent.preflight_holds());
        let job = tested(&s, project, 0);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        // It holds for that take alone.
        assert!(!view(&s, project, 1).overridden);
    }

    #[test]
    fn the_looks_face_is_measured_once_per_look() {
        let (s, project, _) = ready();
        with_look(&s);
        assert_eq!(performed(&s, project).look_face, LookFace::Unmeasured);
        measured(&s, project);
        let LookFace::Height(height) = performed(&s, project).look_face else {
            panic!("not measured");
        };
        assert!((height - 0.33).abs() < 0.01, "{height}");
        let _ = performed(&s, project);
        let framings = s
            .app
            .jobs()
            .into_iter()
            .filter(|job| job.kind() == JobKind::LookFraming)
            .count();
        assert_eq!(framings, 1);
    }
}
