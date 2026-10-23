//! Performed takes (#192, docs/spec/performance-capture.md): once a take's
//! control video is approved, a take model on Higgsfield makes the persona
//! perform it.
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
//!   a photo's metadata) are staged, Higgsfield's quote is asked, and the
//!   request id is saved before any waiting, so a restart (or a retry of a
//!   cancelled job) polls the same request instead of paying again.
//!   Cancelling stops the waiting, not Higgsfield.
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
    ClipHandle, ClipModelRef, ClipStatus, CostPurpose, ImageFormat, Job, JobFailure,
    JobFailureKind, JobId, JobKind, JobState, LookSource, MAX_TAKE_PROMPT, MediaAsset,
    MediaAssetId, MediaAssetRepository, MediaKind, Metered, Money, Outbound, PerformedClip,
    PersonaId, PersonaLook, PersonaRepository, PictureSize, ProfileId, ProjectFiles, Provider,
    ProviderFailure, RepositoryError, SecretStore, StagedFile, TakeGenerator, TakeInput, TakeMode,
    TakeModel, TakePrompt, TakePromptError, TakeRequest, TakeResult, TakeStep, TemplateKind,
    TemplateUsed, TemplateValues, TemplateVariable, TemplateVersionId, TextFormat, TextGenerator,
    TextImage, TextRequest, ThemeRepository, VideoProjectId, take_model_of, take_prompt_text,
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

/// The provider take models run on.
const PROVIDER: Provider = Provider::Higgsfield;

/// How many status checks in a row may fail on Higgsfield's side before
/// the job gives up for now (the queue retries it later, polling the same
/// request).
const MAX_HICCUPS: u32 = 10;

/// What a refusal says: Higgsfield gives no reason.
pub const REFUSED: &str = "Higgsfield refused the content.";

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
    /// The take's control video is not approved as it is now.
    ControlNotApproved,
}

impl PerformedBlock {
    pub const ALL: [PerformedBlock; 5] = [
        PerformedBlock::NoModel,
        PerformedBlock::ModeNotTaken,
        PerformedBlock::Length,
        PerformedBlock::NoLook,
        PerformedBlock::ControlNotApproved,
    ];

    /// A stable name, for the interface's texts.
    pub fn code(self) -> &'static str {
        match self {
            PerformedBlock::NoModel => "no_model",
            PerformedBlock::ModeNotTaken => "mode_not_taken",
            PerformedBlock::Length => "length",
            PerformedBlock::NoLook => "no_look",
            PerformedBlock::ControlNotApproved => "control_not_approved",
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
    /// Its latest prompt job, and its latest draft or final job with the
    /// step it makes: one under way, else one made for its control video
    /// and model as they are now.
    pub prompt_job: Option<Job>,
    pub job: Option<(Job, TakeStep)>,
    /// Whether a prompt or a performed take of it is being made.
    pub busy: bool,
    /// Whether its latest draft or final job stopped (or failed) after
    /// Higgsfield took the request: checking again polls it, and nothing
    /// new is paid for until then.
    pub pending: bool,
    /// The next paid step, and what it would cost.
    pub next: PerformedStep,
    pub estimate: Option<SpendEstimate>,
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
}

/// Whether a take job's saved state holds a request Higgsfield took.
fn has_request(job: &Job) -> bool {
    job.external_handle()
        .and_then(|saved| serde_json::from_str::<TakeState>(saved).ok())
        .is_some_and(|state| state.request.is_some())
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
    /// that could not be read, Higgsfield down) resends the same
    /// submission, which Higgsfield answers with the first one.
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
fn look_print(bytes: &[u8]) -> String {
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

fn provider_failure(failure: ProviderFailure) -> JobFailure {
    let kind = JobFailureKind::from(failure.kind);
    if kind == JobFailureKind::Declined {
        return JobFailure::new(kind, REFUSED);
    }
    JobFailure::new(kind, format!("Higgsfield: {}", failure.detail))
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
}

impl JobHandler for PerformedTakeHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: TakePayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let capture: CaptureId = id(&payload.capture)?;
        let step = TakeStep::from_code(&payload.step)
            .ok_or_else(|| unexpected(format!("unknown take step {}", payload.step)))?;
        if self
            .captures
            .take_results(project)
            .map_err(unexpected)?
            .iter()
            .any(|made| {
                made.position == payload.position && made.step == step && made.job == Some(cx.id())
            })
        {
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
        let state = match cx.external_handle() {
            Some(saved) => parse(saved)?,
            None => TakeState::default(),
        };
        TakeRun {
            handler: self,
            payload,
            project,
            capture,
            step,
            model,
            generator,
            key,
            state,
        }
        .run(cx)
    }
}

impl TakeRun<'_> {
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
                Ok(ClipStatus::Queued | ClipStatus::Running) => hiccups = 0,
                Ok(ClipStatus::Done { video }) => {
                    // The take is made and paid: whatever goes wrong now,
                    // the next attempt polls the same request again.
                    match self.generator.download(&self.key, &video) {
                        Ok(clip) => return self.save(&clip.bytes, cx),
                        Err(failure) if JobFailureKind::from(failure.kind).is_transient() => {
                            hiccups += 1;
                            if hiccups >= MAX_HICCUPS {
                                return Err(provider_failure(failure));
                            }
                        }
                        Err(failure) => return Err(provider_failure(failure)),
                    }
                }
                Ok(ClipStatus::Failed(failure)) => return self.ended(failure, cx),
                Err(failure) if JobFailureKind::from(failure.kind).is_transient() => {
                    hiccups += 1;
                    if hiccups >= MAX_HICCUPS {
                        return Err(provider_failure(failure));
                    }
                }
                // The key or the account: retrying polls the same request.
                Err(failure) => return Err(provider_failure(failure)),
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

    /// The request ended without a take: a retry by the user submits it
    /// again, a new paid request, so the queue never retries it on its
    /// own.
    fn ended(&mut self, failure: ProviderFailure, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.state.restart();
        self.save_state(cx)?;
        let failure = provider_failure(failure);
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

    /// Stages what is not staged yet, asks the quote and submits. `false`
    /// when stopped before anything was paid.
    fn submit(&mut self, cx: &mut JobContext) -> Result<bool, JobFailure> {
        // Nothing paid yet: the take must still be the one approved.
        if !self.still_approved()? {
            return Err(take_changed());
        }
        if self.state.video.is_none() {
            let bytes = self
                .handler
                .files
                .read(self.project, &self.payload.control_file)
                .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))?;
            if bytes.len() as u64 > self.model.max_video_bytes {
                return Err(JobFailure::new(
                    JobFailureKind::Media,
                    format!(
                        "the control video is over the take model's {} MB",
                        self.model.max_video_bytes / 1_000_000
                    ),
                ));
            }
            let staged = self.stage(&bytes, "video/mp4")?;
            self.state.video = Some(staged);
            self.save_state(cx)?;
        }
        if self.state.look.is_none() {
            let persona: PersonaId = id(&self.payload.persona)?;
            let look = self
                .handler
                .personas
                .look(persona)
                .map_err(unexpected)?
                .ok_or_else(|| JobFailure::unexpected("the persona has no look"))?;
            let jpeg = self.fit_look(&look)?;
            let staged = self.stage(&jpeg, "image/jpeg")?;
            self.state.look = Some(staged);
            self.state.look_print = Some(look_print(look.image.bytes()));
            self.state.realistic = look.realistic;
            self.save_state(cx)?;
        }
        let request = self.request();
        if !self.state.quoted {
            self.state.quote_micros = self.generator.quote(&self.key, &request).map(Money::micros);
            self.state.quoted = true;
            self.save_state(cx)?;
        }
        if cx.should_stop() {
            return Ok(false);
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
                    self.state.restart();
                    self.save_state(cx)?;
                }
                Err(provider_failure(failure))
            }
        }
    }

    fn stage(&self, bytes: &[u8], mime: &str) -> Result<String, JobFailure> {
        self.generator
            .stage(&self.key, TakeInput { bytes, mime })
            .map(|staged| staged.0)
            .map_err(provider_failure)
    }

    fn request(&self) -> TakeRequest {
        TakeRequest {
            model: self.payload.model.clone(),
            step: self.step,
            resolution: self.payload.resolution.clone(),
            prompt: self.payload.prompt.clone(),
            video: StagedFile(self.state.video.clone().unwrap_or_default()),
            look: StagedFile(self.state.look.clone().unwrap_or_default()),
        }
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

    /// Records the take's cost (once) and saves it as the take's draft or
    /// final; the one it replaces goes. A take that changed while it was
    /// made keeps what it has: the result is paid for but not kept.
    fn save(&mut self, bytes: &[u8], cx: &mut JobContext) -> Result<(), JobFailure> {
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
                    "Higgsfield: the take is not a video",
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
                    provider: PROVIDER,
                    model: &self.payload.priced_model,
                    purpose: match self.step {
                        TakeStep::Draft => CostPurpose::TakeDraft,
                        TakeStep::Final => CostPurpose::TakeFinal,
                    },
                    usage: Metered::video_seconds(u64::from(self.payload.seconds)),
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
        let replaced = captures
            .take_results(self.project)
            .map_err(unexpected)?
            .into_iter()
            .find(|old| old.position == self.payload.position && old.step == self.step);
        let template = match (&self.payload.template, self.payload.template_number) {
            (Some(template), Some(number)) => Some(TemplateUsed {
                id: id::<TemplateVersionId>(template)?,
                number,
            }),
            _ => None,
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
        };
        if !captures.save_take_result(&result).map_err(unexpected)? {
            // The take is gone.
            let _ = files.remove(self.project, &file);
            return Err(take_changed());
        }
        if let Some(replaced) = replaced.filter(|old| old.file != file) {
            let _ = files.remove(self.project, &replaced.file);
        }
        Ok(())
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

/// The calls a take's next step makes.
fn step_calls(model: &TakeModel, next: PerformedStep, length: Duration) -> Vec<PlannedCall> {
    match next {
        PerformedStep::Prompt => {
            vec![PlannedCall::new(
                Provider::Claude,
                CostPurpose::TakePrompt,
                1,
            )]
        }
        PerformedStep::Make(step) => vec![
            PlannedCall::new(
                model.id.provider(),
                match step {
                    TakeStep::Draft => CostPurpose::TakeDraft,
                    TakeStep::Final => CostPurpose::TakeFinal,
                },
                1,
            )
            .with_model(&model.priced_model(step))
            .with_video_seconds(billed_seconds(length)),
        ],
    }
}

/// What a take's next paid step is: its prompt, then its draft until one
/// is approved (where the model makes drafts), then its final.
fn next_step(
    model: Option<&TakeModel>,
    prompt: Option<&TakePrompt>,
    draft: Option<&TakeResult>,
) -> PerformedStep {
    if prompt.is_none() {
        return PerformedStep::Prompt;
    }
    let drafts = model.is_some_and(|model| model.draft.is_some());
    if drafts && draft.is_none_or(|draft| draft.accepted_at.is_none()) {
        PerformedStep::Make(TakeStep::Draft)
    } else {
        PerformedStep::Make(TakeStep::Final)
    }
}

impl Bardo {
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
        let prompts = self.captures.take_prompts(project)?;
        let results = self.captures.take_results(project)?;
        let used = self.media_assets.performed_clips(project)?;
        let jobs = self.performed_jobs(capture);
        let mut takes = Vec::with_capacity(capture.takes.as_slice().len());
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
            let of_take = || jobs.iter().filter(|(_, of)| of.position == position);
            let busy = of_take().any(|(job, _)| job.state().is_active());
            // One under way, else the latest made for the take as it is now:
            // one for another control video or model is history.
            let latest = |kind: JobKind| {
                of_take().rev().find(|(job, of)| {
                    job.kind() == kind
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
            let next = next_step(model.as_ref(), prompt.as_ref(), draft.as_ref());
            let estimate = match (&model, block) {
                (Some(model), None) => {
                    Some(self.estimate(&step_calls(model, next, take.length()))?)
                }
                _ => None,
            };
            takes.push(PerformedTakeView {
                follows_channel: take.model.is_none()
                    || take.model.as_ref() == channel_model.as_ref().map(|m| &m.id),
                model,
                block,
                prompt,
                draft,
                final_take,
                prompt_job,
                job,
                busy,
                pending,
                next,
                estimate,
                length: take.length(),
                used: used
                    .iter()
                    .filter(|clip| clip.capture == capture.id && clip.position == position)
                    .count(),
            });
        }
        Ok(PerformedView {
            models,
            channel_model,
            has_look: look.is_some(),
            realistic: look.is_some_and(|(_, look)| look.realistic),
            takes,
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
        let calls = step_calls(model, PerformedStep::Prompt, take.length());
        if let Err(estimate) = self.check_budget(&calls, consent)? {
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

    /// Starts a job in which take `index`'s model makes its draft or
    /// final from its prompt, its approved control video and the persona's
    /// look. A final needs an approved draft where the model makes drafts.
    /// Past Higgsfield's budget it needs `consent`.
    pub fn make_performed_take(
        &self,
        project: VideoProjectId,
        index: usize,
        step: TakeStep,
        consent: BudgetConsent,
    ) -> Result<JobId, CaptureError> {
        let (capture, view) = self.performed_take(project, index)?;
        let model = Self::check_take(&view)?;
        let prompt = view
            .prompt
            .as_ref()
            .ok_or(CaptureError::Prompt(TakePromptError::Empty))?;
        let text = take_prompt_text(&prompt.text).map_err(CaptureError::Prompt)?;
        let tier = model.tier(step).ok_or(CaptureError::NoDrafts)?;
        if step == TakeStep::Final
            && model.draft.is_some()
            && view
                .draft
                .as_ref()
                .is_none_or(|draft| draft.accepted_at.is_none())
        {
            return Err(CaptureError::DraftNotApproved);
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
        let calls = step_calls(model, PerformedStep::Make(step), take.length());
        if let Err(estimate) = self.check_budget(&calls, consent)? {
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
            seconds: billed_seconds(take.length()),
        };
        let job = Job::new(self.profile.id, JobKind::PerformedTake, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Checks again on take `index`'s draft or final job that stopped (or
    /// failed) after Higgsfield took its request: it polls that request,
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
        let made = match step {
            TakeStep::Draft => view.draft.as_ref(),
            TakeStep::Final => view.final_take.as_ref(),
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
        let clip = PerformedClip {
            asset: id,
            project,
            capture: capture.id,
            position: index as u32,
            model: made.model.clone(),
            realistic: made.realistic,
            used_at: now,
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
        }
        .ok_or(CaptureError::NoTakeResult)?;
        let controls = self.control_views(&capture)?;
        let control = controls
            .get(index)
            .and_then(|view| view.built.clone())
            .ok_or(CaptureError::ControlNotReady)?;
        let size = bardo_domain::control_size(capture.video.width, capture.video.height);
        Ok(ControlPlayer::with_sound(
            Arc::clone(&self.media),
            Arc::clone(&self.audio),
            self.take_result_path(&made),
            self.control_video_path(&control),
            (made.duration, view.length),
            size,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    use bardo_domain::{ChannelDraft, CostRepository, JobState};
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

    fn made(s: &Setup, project: VideoProjectId, index: usize, step: TakeStep) -> Job {
        let id = s
            .app
            .make_performed_take(project, index, step, BudgetConsent::Ask)
            .unwrap();
        wait_done(&s.app, id)
    }

    fn costs(s: &Setup, project: VideoProjectId) -> Vec<(CostPurpose, String, u64)> {
        s.h.db
            .project_costs(project)
            .unwrap()
            .into_iter()
            .filter(|record| record.provider == Provider::Higgsfield)
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
    fn a_video_edit_take_needs_an_approved_draft_before_its_final() {
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
        assert!(matches!(
            s.app
                .make_performed_take(project, 0, TakeStep::Final, BudgetConsent::Ask),
            Err(CaptureError::DraftNotApproved)
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
        assert!(s.app.captures.take_results(project).unwrap().is_empty());
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
}
