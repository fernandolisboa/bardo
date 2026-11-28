//! AI bridges over a cut in the editor (#363, ADR-0050): with the clip after
//! a cut selected, **Smooth with AI** asks a video model that can end on a
//! given frame (#362) for a short clip from the frame the picture before
//! the cut shows just before the bridge's stretch to the frame the one
//! after it shows where the stretch ends, moved by a fixed prompt and the
//! user's note. The bridge covers the cut, half its length on each side;
//! it never adds time, so the narration stays in sync.
//!
//! The price is shown before anything is paid, and budgets and the video's
//! cap apply as for scene clips. The clip is made by a job that sends it
//! once with the guarantees of scene clips (ADR-0030), through the same
//! submission code (`crate::clips::ClipRequests`). A finished bridge waits
//! on its cut until the user previews it in place and uses it (it becomes a
//! project media asset, placed with one undoable `Edit::Replace`) or
//! discards it. A bridge whose cut changed since says so and can only be
//! discarded or made again.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    AssetSource, BRIDGE_NOTE_MAX_CHARS, Bridge, BridgeCut, BridgeId, BridgeRefusal,
    BridgeRepository, BridgeSpan, ClipDurations, ClipGenerator, ClipImage, ClipModel, ClipRequest,
    CostPurpose, EditError, FrameSource, ImageFormat, ItemRef, Job, JobFailure, JobFailureKind,
    JobId, JobKind, JobState, LastFrame, MediaAsset, MediaAssetId, MediaKind, Metered, Money,
    Picture, PictureSize, ProfileId, ProjectFiles, Provider, ProviderFailure, ProviderFailureKind,
    RepositoryError, SecretStore, StagedImage, Timeline, VideoItem, VideoProjectId, VideoSource,
    no_last_frame, timecode,
};
use bardo_media::MediaEngine;
use serde::{Deserialize, Serialize};

use crate::clips::{
    ClipCalls, ClipOrder, ClipRequests, ClipRun, ClipsState, MAX_HICCUPS, Step, ask_again,
    saved_after_asking,
};
use crate::costs::{BudgetConsent, CostBook, PlannedCall, SpendEstimate};
use crate::editor::{Editor, EditorError};
use crate::jobs::{JobContext, JobHandler};
use crate::scenes::{id, parse, to_json, unexpected};
use crate::{Bardo, KeyState, Text};

/// The most a bridge's frame may weigh: well within what every video
/// provider takes (Kling and Higgsfield read up to 10 MB).
const MAX_FRAME_BYTES: u64 = 5_000_000;

/// The one request a bridge job makes, by its index in the job's state.
const ONLY: usize = 0;

/// Why a bridge was not asked for, used or discarded.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("{0}")]
    Refused(BridgeRefusal),
    /// No model that can end on a given frame has its key or plan set up.
    #[error("no model that can end on a given frame is set up")]
    NoModel,
    /// The model or length picked is not one the cut offers.
    #[error("that model or length does not fit the cut")]
    NotOffered,
    #[error("the note for the model is too long")]
    NoteTooLong,
    /// The cut already has a bridge waiting or being made.
    #[error("the cut already has a bridge")]
    Busy,
    /// The bridge's cut changed since it was asked for.
    #[error("the cut changed since the bridge was asked for")]
    CutChanged,
    /// The bridge is still being made, or was never made.
    #[error("the bridge is not ready")]
    NotReady,
    /// The job making it is at work: it is cancelled from Jobs first.
    #[error("the bridge is still being made")]
    StillMaking,
    #[error("no such bridge")]
    NotFound,
    #[error("over budget")]
    OverBudget(Box<SpendEstimate>),
    #[error(transparent)]
    Editor(#[from] EditorError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("could not queue the bridge: {0}")]
    Queue(String),
}

impl From<EditError> for BridgeError {
    fn from(error: EditError) -> Self {
        BridgeError::Editor(error.into())
    }
}

impl BridgeError {
    /// What the editor says.
    pub fn message(&self) -> Text {
        match self {
            BridgeError::Refused(BridgeRefusal::NoCut) => Text::EditorBridgeNoCut,
            BridgeError::Refused(BridgeRefusal::TooLong) => Text::EditorBridgeTooLong,
            BridgeError::Refused(BridgeRefusal::NoPicture) => Text::EditorBridgeNoPicture,
            BridgeError::NoModel => Text::EditorBridgeNoModel,
            BridgeError::NotOffered => Text::EditorBridgeNotOffered,
            BridgeError::NoteTooLong => Text::EditorBridgeNoteTooLong,
            BridgeError::Busy => Text::EditorBridgeBusy,
            BridgeError::CutChanged => Text::EditorBridgeStale,
            BridgeError::NotReady => Text::EditorBridgeNotReady,
            BridgeError::StillMaking => Text::EditorBridgeStillMaking,
            BridgeError::NotFound => Text::EditorBridgeNotFound,
            BridgeError::OverBudget(_) => Text::EditorBridgeOverBudget,
            BridgeError::Editor(error) => error.message(),
            BridgeError::Repository(_) | BridgeError::Queue(_) => Text::EditorBridgeNotSaved,
        }
    }
}

/// One length a bridge over the cut can take, and its price.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeLength {
    pub seconds: u32,
    /// The rate table's price; `None` when no rate covers the model.
    pub price: Option<Money>,
}

/// A model that can make the bridge, with the lengths that fit the cut,
/// shortest first.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeModel {
    pub model: ClipModel,
    pub lengths: Vec<BridgeLength>,
}

/// What the bridge panel offers for a cut.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeOptions {
    /// The cut, by the index of the clip after it.
    pub cut: usize,
    /// The models that can end on a given frame and have their key or
    /// plan set up, with a length that fits: the channel's clip model
    /// first when it is one of them.
    pub models: Vec<BridgeModel>,
    /// Why no bridge fits, when none does: no cut, the pictures too
    /// short for every length, or one with nothing to show.
    pub refusal: Option<BridgeRefusal>,
    /// Whether a model that can end on a frame is offered at all, keyed
    /// or not: without one the panel says to set up a key.
    pub any_model: bool,
}

/// What the user asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeAsk {
    pub cut: usize,
    pub provider: Provider,
    pub model: String,
    pub seconds: u32,
    pub note: String,
}

/// Where a bridge stands.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeState {
    /// Its job is at work.
    Making,
    /// Its job stopped without a clip.
    Failed(Option<JobFailure>),
    /// Made, waiting for the user.
    Ready { path: PathBuf },
}

/// A bridge, as the editor shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeView {
    pub id: BridgeId,
    /// The cut it goes over now, by the index of the clip after it;
    /// `None` once a picture around it changed.
    pub cut: Option<usize>,
    /// The stretch it covers.
    pub from: Duration,
    pub to: Duration,
    pub provider: Provider,
    /// The model's name as people call it, else as the provider does.
    pub model: String,
    pub seconds: u32,
    pub note: String,
    pub job: JobId,
    pub state: BridgeState,
    /// Its clip in the project folder, once made.
    pub file: Option<String>,
}

impl BridgeView {
    pub fn is_ready(&self) -> bool {
        matches!(self.state, BridgeState::Ready { .. })
    }

    pub fn span(&self) -> Option<BridgeSpan> {
        Some(BridgeSpan {
            cut: self.cut?,
            from: bardo_domain::nearest_frame(self.from),
            to: bardo_domain::nearest_frame(self.to),
        })
    }
}

/// The lengths `durations` allows, in whole seconds, shortest first.
fn whole_lengths(durations: &ClipDurations) -> Vec<u32> {
    match durations {
        ClipDurations::Range { min, max } => ((*min).max(1)..=(*max).max(*min)).collect(),
        ClipDurations::Choices(choices) => {
            let mut choices: Vec<u32> = choices.iter().copied().filter(|s| *s > 0).collect();
            choices.sort_unstable();
            choices.dedup();
            choices
        }
    }
}

/// The bridge placed as footage: its clip from the start, as long as it
/// was asked for (the edit cuts it to the stretch).
pub(crate) fn bridge_item(file: &str, seconds: u32) -> VideoItem {
    let length = Duration::from_secs(u64::from(seconds));
    VideoItem {
        picture: Picture::Footage(file.to_owned()),
        source: VideoSource::Clip {
            file: file.to_owned(),
            length,
            stretched_from: None,
        },
        start: Duration::ZERO,
        at: Duration::ZERO,
        duration: length,
        framing: bardo_domain::Framing::default(),
        transition: None,
        motion: None,
    }
}

/// A bridge job's order: which bridge, with which model (the budget was
/// checked against it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BridgePayload {
    project: String,
    bridge: String,
    provider: String,
    model: String,
}

impl BridgePayload {
    fn order(&self) -> ClipOrder {
        ClipOrder {
            scene: ONLY,
            provider: self.provider.clone(),
            model: self.model.clone(),
            had: None,
        }
    }
}

/// A bridge job's project and bridge, to find its bridge.
fn bridge_payload(job: &Job) -> Option<BridgePayload> {
    (job.kind() == JobKind::BridgeClip)
        .then(|| serde_json::from_str(job.payload()).ok())
        .flatten()
}

/// Makes bridges.
pub(crate) struct BridgeHandler {
    pub(crate) owner: ProfileId,
    pub(crate) bridges: Arc<dyn BridgeRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) generators: Vec<Arc<dyn ClipGenerator>>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) plan_sessions: crate::plans::PlanSessions,
    pub(crate) costs: CostBook,
}

impl JobHandler for BridgeHandler {
    /// The user's retry asks again for a bridge whose request ended.
    fn asked_again(&self, external_handle: Option<&str>) -> Option<String> {
        ask_again(external_handle)
    }

    fn saved_after_ask(&self, asked: Option<&str>, saved: String) -> String {
        saved_after_asking(asked, saved)
    }

    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: BridgePayload = parse(payload)?;
        let state = match cx.external_handle() {
            Some(saved) => parse(saved)?,
            None => ClipsState::default(),
        };
        let mut run = BridgeJob {
            handler: self,
            job: cx.id(),
            project: id(&payload.project)?,
            bridge: id(&payload.bridge)?,
            order: payload.order(),
            state,
            calls: ClipCalls::new(
                self.owner,
                Arc::clone(&self.secrets),
                self.plan_sessions.clone(),
            ),
        };
        run.run(cx)
    }
}

/// One bridge job's run.
struct BridgeJob<'a> {
    handler: &'a BridgeHandler,
    job: JobId,
    project: VideoProjectId,
    bridge: BridgeId,
    order: ClipOrder,
    state: ClipsState,
    calls: ClipCalls,
}

impl ClipRequests for BridgeJob<'_> {
    fn calls(&mut self) -> &mut ClipCalls {
        &mut self.calls
    }

    fn state(&self) -> &ClipsState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut ClipsState {
        &mut self.state
    }

    fn job(&self) -> JobId {
        self.job
    }

    fn project(&self) -> VideoProjectId {
        self.project
    }

    fn costs(&self) -> &CostBook {
        &self.handler.costs
    }

    fn purpose(&self) -> CostPurpose {
        CostPurpose::BridgeClip
    }

    fn label(&self, _order: &ClipOrder) -> String {
        "The bridge".to_owned()
    }

    /// Never `{job}-{scene}-{n}`, a scene clip's: the Higgsfield API keys
    /// its idempotency on it.
    fn submission_id(&self, _order: &ClipOrder, submission: u32) -> String {
        format!("{}-bridge-{}", self.job, submission)
    }

    /// The bridge shows its job's failure: nothing more to keep.
    fn show_failed(&self, _order: &ClipOrder, _failure: &JobFailure) -> Result<(), JobFailure> {
        Ok(())
    }

    /// Saves the clip as the bridge's, for the user to review. A bridge
    /// discarded meanwhile keeps nothing (its cost is in Costs).
    fn save_clip(&mut self, order: &ClipOrder, bytes: &[u8]) -> Result<(), JobFailure> {
        let Some(mut bridge) = self
            .handler
            .bridges
            .bridge(self.bridge)
            .map_err(unexpected)?
        else {
            return Ok(());
        };
        let file = Bridge::file_name(bridge.id);
        self.handler
            .files
            .write(self.project, &file, bytes)
            .map_err(unexpected)?;
        bridge.file = Some(file);
        self.handler
            .bridges
            .save_bridge(&bridge)
            .map_err(unexpected)?;
        remove_frames(self.handler.files.as_ref(), self.project, bridge.id);
        // Once more, in case storage refused the record at acceptance.
        self.record(order)
    }
}

/// Removes a bridge's two frames from the project folder.
fn remove_frames(files: &dyn ProjectFiles, project: VideoProjectId, id: BridgeId) {
    let (first, last) = Bridge::frame_names(id);
    for name in [first, last] {
        if files.exists(project, &name) {
            let _ = files.remove(project, &name);
        }
    }
}

impl BridgeJob<'_> {
    fn run(&mut self, cx: &mut JobContext) -> Result<(), JobFailure> {
        let order = self.order.clone();
        // A request accepted by a run cut before it recorded the cost.
        self.record(&order)?;
        if let Some(ended) = self.state.ended.first() {
            return Err(ended.failure());
        }
        let generator = self
            .handler
            .generators
            .iter()
            .find(|generator| generator.provider().code() == order.provider)
            .cloned()
            .ok_or_else(|| unexpected(format!("{} does not make clips", order.provider)))?;
        let mut polls = 0;
        let mut hiccups = 0;
        loop {
            if cx.should_stop() {
                return Ok(());
            }
            // Discarded meanwhile: nothing is waited for.
            let Some(bridge) = self
                .handler
                .bridges
                .bridge(self.bridge)
                .map_err(unexpected)?
            else {
                return Ok(());
            };
            if bridge.file.is_some() || self.state.delivered.contains(&ONLY) {
                return Ok(());
            }
            let mut key = self.calls.key(generator.provider())?;
            let request = self.run_of(ONLY).and_then(|run| run.request.clone());
            let step = match request {
                Some(request) => self.poll(&order, &generator, &mut key, request, cx)?,
                None => self.submit(&bridge, &generator, &mut key, cx)?,
            };
            match step {
                Step::Waiting => hiccups = 0,
                Step::Hiccup(failure) => {
                    hiccups += 1;
                    if hiccups >= MAX_HICCUPS {
                        return Err(failure);
                    }
                }
                Step::NotSubmitted(failure) | Step::Failed(failure) => return Err(failure),
                Step::Saved | Step::Left | Step::Stopped => return Ok(()),
            }
            if !cx.sleep(generator.poll_delay(polls)) {
                return Ok(());
            }
            polls += 1;
        }
    }

    /// Reads the bridge's two frames, hands both over (once per
    /// submission) and sends it. Nothing is handed over for a model that
    /// cannot end on a frame.
    fn submit(
        &mut self,
        bridge: &Bridge,
        generator: &Arc<dyn ClipGenerator>,
        key: &mut bardo_domain::ApiKey,
        cx: &mut JobContext,
    ) -> Result<Step, JobFailure> {
        let order = self.order.clone();
        if self.ends_unseen(&order, generator) {
            return self.may_have_started(
                &order,
                "Bardo stopped while the submission was on its way",
                cx,
            );
        }
        let model = generator
            .models()
            .into_iter()
            .find(|model| model.id.model() == order.model);
        match model {
            None => {
                let failure = ProviderFailure::new(
                    ProviderFailureKind::NotAllowed,
                    format!("the model {} is no longer offered", order.model),
                );
                return self.scene_failed(&order, failure, cx);
            }
            Some(model) if !model.ends_on_frame => {
                return self.scene_failed(&order, no_last_frame(&order.model), cx);
            }
            Some(_) => {}
        }
        let (first, last) = self.frames(bridge)?;
        if self.run_of(ONLY).is_none() {
            let (first_name, _) = Bridge::frame_names(bridge.id);
            self.state.scenes.push(ClipRun {
                scene: ONLY,
                submission: 1,
                prompt: bridge.prompt(),
                seconds: bridge.seconds,
                stretch_percent: 0,
                source_image: String::new(),
                image_file: first_name,
                staged: None,
                staged_last: None,
                request: None,
                quote_micros: None,
                sent_at: None,
                cost: None,
            });
            self.save_state(cx)?;
        }
        let staged = self.run_of(ONLY).and_then(|run| run.staged.clone());
        let staged = match staged {
            Some(staged) => staged,
            None => match self.call(generator, key, |key| generator.stage_image(key, &first)) {
                Ok(staged) => {
                    self.run_of(ONLY).expect("the run was added").staged = Some(staged.0.clone());
                    self.save_state(cx)?;
                    staged.0
                }
                Err(failure) => return self.submit_failed(&order, failure, cx),
            },
        };
        let staged_last = self.run_of(ONLY).and_then(|run| run.staged_last.clone());
        let staged_last = match staged_last {
            Some(staged) => staged,
            None => match self.call(generator, key, |key| generator.stage_image(key, &last)) {
                Ok(staged) => {
                    self.run_of(ONLY).expect("the run was added").staged_last =
                        Some(staged.0.clone());
                    self.save_state(cx)?;
                    staged.0
                }
                Err(failure) => return self.submit_failed(&order, failure, cx),
            },
        };
        let request = ClipRequest {
            model: order.model.clone(),
            prompt: bridge.prompt(),
            image: StagedImage(staged),
            aspect: first.aspect(),
            first_frame: first,
            seconds: bridge.seconds,
            last: Some(LastFrame {
                image: StagedImage(staged_last),
                frame: last,
            }),
        };
        self.send(&order, generator, key, &request, cx)
    }

    /// The bridge's two frames as JPEGs, the last the same size as the
    /// first. Read from their sources once and kept in the project folder
    /// until the clip is made, so every attempt sends the same pictures.
    fn frames(&self, bridge: &Bridge) -> Result<(ClipImage, ClipImage), JobFailure> {
        let (first_name, last_name) = Bridge::frame_names(bridge.id);
        let first = self.frame(&bridge.first, &first_name, None)?;
        let size = bardo_domain::image_dimensions(&first)
            .ok_or_else(|| unexpected("the bridge's first frame is not a picture"))?;
        let last = self.frame(&bridge.last, &last_name, Some(size))?;
        Ok((
            ClipImage {
                bytes: first,
                format: ImageFormat::Jpeg,
            },
            ClipImage {
                bytes: last,
                format: ImageFormat::Jpeg,
            },
        ))
    }

    /// One frame's JPEG, read from `source` the first time.
    fn frame(
        &self,
        source: &FrameSource,
        name: &str,
        size: Option<(u32, u32)>,
    ) -> Result<Vec<u8>, JobFailure> {
        let files = &self.handler.files;
        if files.exists(self.project, name) {
            return files.read(self.project, name).map_err(unexpected);
        }
        let folder = tempfile::Builder::new()
            .prefix("bardo-bridge-")
            .tempdir()
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))?;
        let destination = folder.path().join("frame.jpg");
        let at = match source {
            FrameSource::Video { at, .. } => Some(*at),
            FrameSource::Still { .. } => None,
        };
        self.handler
            .media
            .bridge_frame(
                &files.path(self.project, source.file()),
                at,
                size,
                &destination,
                MAX_FRAME_BYTES,
            )
            .map_err(|e| JobFailure::new(JobFailureKind::Media, e.to_string()))?;
        let bytes = std::fs::read(&destination)
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))?;
        files
            .write(self.project, name, &bytes)
            .map_err(unexpected)?;
        Ok(bytes)
    }
}

impl Bardo {
    /// Every model that can end a clip on a given frame and has its key or
    /// plan set up, the video's clip model first when it is one of them.
    fn bridge_models(
        &self,
        video: &bardo_domain::VideoProject,
    ) -> Result<Vec<ClipModel>, RepositoryError> {
        let models = self.clip_models();
        let channel = self.channel_clip_model(&models, video)?;
        let mut usable: Vec<ClipModel> = models
            .into_iter()
            .filter(|model| {
                model.ends_on_frame && self.work_key_state(model.id.provider()) != KeyState::NotSet
            })
            .collect();
        if let Some(position) =
            channel.and_then(|channel| usable.iter().position(|model| model.id == channel.id))
        {
            let first = usable.remove(position);
            usable.insert(0, first);
        }
        Ok(usable)
    }

    /// What the bridge panel offers for the cut before clip `cut` of the
    /// editor's timeline: each usable model with the lengths that fit and
    /// their prices.
    pub fn bridge_options(
        &self,
        editor: &Editor,
        cut: usize,
    ) -> Result<BridgeOptions, BridgeError> {
        let view = editor.view();
        let timeline = view.timeline.as_ref().ok_or(EditorError::NothingToCut)?;
        let any_model = self.clip_models().iter().any(|model| model.ends_on_frame);
        let mut options = BridgeOptions {
            cut,
            models: Vec::new(),
            refusal: None,
            any_model,
        };
        // No cut, or a picture with nothing to show, whatever the length.
        if cut == 0 || cut >= timeline.video().len() {
            options.refusal = Some(BridgeRefusal::NoCut);
            return Ok(options);
        }
        let shortest = BridgeSpan {
            cut,
            from: bardo_domain::nearest_frame(timeline.video()[cut].at).saturating_sub(1),
            to: bardo_domain::nearest_frame(timeline.video()[cut].at) + 1,
        };
        if let Err(refusal) = timeline.bridge_ends(&shortest) {
            options.refusal = Some(refusal);
            return Ok(options);
        }
        let rates = self.cost_book.rates()?;
        for model in self.bridge_models(&view.project)? {
            let lengths: Vec<BridgeLength> = whole_lengths(&model.durations)
                .into_iter()
                .filter(|&seconds| timeline.bridge_span(cut, seconds).is_ok())
                .map(|seconds| BridgeLength {
                    seconds,
                    price: rates.price(
                        model.id.provider(),
                        model.id.model(),
                        &Metered::video_seconds(u64::from(seconds)),
                    ),
                })
                .collect();
            if !lengths.is_empty() {
                options.models.push(BridgeModel { model, lengths });
            }
        }
        if options.models.is_empty() && !self.bridge_models(&view.project)?.is_empty() {
            options.refusal = Some(BridgeRefusal::TooLong);
        }
        Ok(options)
    }

    /// The project's bridges, as the editor shows them over `timeline`.
    pub(crate) fn bridge_views(
        &self,
        project: VideoProjectId,
        timeline: Option<&Timeline>,
    ) -> Result<Vec<BridgeView>, RepositoryError> {
        let bridges = self.bridges.bridges(project)?;
        if bridges.is_empty() {
            return Ok(Vec::new());
        }
        let jobs = self.jobs();
        let models = self.clip_models();
        Ok(bridges
            .into_iter()
            .map(|bridge| {
                let job = jobs.iter().find(|job| job.id() == bridge.job);
                let state = match (&bridge.file, job) {
                    (Some(file), _) => BridgeState::Ready {
                        path: self.files.path(project, file),
                    },
                    (None, Some(job)) if job.state().is_active() => BridgeState::Making,
                    (None, Some(job)) if job.state() == JobState::Failed => {
                        BridgeState::Failed(job.failure().cloned())
                    }
                    (None, _) => BridgeState::Failed(None),
                };
                let model = models
                    .iter()
                    .find(|model| {
                        model.id.provider() == bridge.provider && model.id.model() == bridge.model
                    })
                    .map_or_else(|| bridge.model.clone(), |model| model.name.to_string());
                BridgeView {
                    id: bridge.id,
                    cut: timeline
                        .and_then(|timeline| bridge.span_in(timeline))
                        .map(|span| span.cut),
                    from: bardo_domain::frame_time(bridge.from),
                    to: bardo_domain::frame_time(bridge.to),
                    provider: bridge.provider,
                    model,
                    seconds: bridge.seconds,
                    note: bridge.note.clone(),
                    job: bridge.job,
                    state,
                    file: bridge.file.clone(),
                }
            })
            .collect())
    }

    /// Asks for a bridge over the cut `ask.cut` of the editor's timeline:
    /// queues the job that makes it and leaves it waiting on the cut.
    /// Past the provider's budget or the video's cap it needs `consent`.
    pub fn make_bridge(
        &self,
        editor: &mut Editor,
        ask: BridgeAsk,
        consent: BudgetConsent,
    ) -> Result<BridgeId, BridgeError> {
        self.refresh_editor(editor)?;
        let view = editor.view().clone();
        let project = view.project.clone();
        let timeline = view.timeline.as_ref().ok_or(EditorError::NothingToCut)?;
        let note = ask.note.trim().to_owned();
        if note.chars().count() > BRIDGE_NOTE_MAX_CHARS {
            return Err(BridgeError::NoteTooLong);
        }
        let span = timeline
            .bridge_span(ask.cut, ask.seconds)
            .map_err(BridgeError::Refused)?;
        let (first, last) = timeline.bridge_ends(&span).map_err(BridgeError::Refused)?;
        if view
            .bridges
            .iter()
            .any(|bridge| bridge.cut == Some(ask.cut))
        {
            return Err(BridgeError::Busy);
        }
        let models = self.bridge_models(&project)?;
        if models.is_empty() {
            return Err(BridgeError::NoModel);
        }
        let model = models
            .iter()
            .find(|model| model.id.provider() == ask.provider && model.id.model() == ask.model)
            .ok_or(BridgeError::NotOffered)?;
        if !whole_lengths(&model.durations).contains(&ask.seconds) {
            return Err(BridgeError::NotOffered);
        }
        let call = PlannedCall::new(ask.provider, CostPurpose::BridgeClip, 1)
            .with_model(&ask.model)
            .with_video_seconds(ask.seconds);
        self.check_video_budget(project.id, &[call], consent)?
            .map_err(|estimate| BridgeError::OverBudget(Box::new(estimate)))?;
        let cut =
            BridgeCut::of(timeline, ask.cut).ok_or(BridgeError::Refused(BridgeRefusal::NoCut))?;
        let id = BridgeId::new();
        let payload = BridgePayload {
            project: project.id.to_string(),
            bridge: id.to_string(),
            provider: ask.provider.code().to_owned(),
            model: ask.model.clone(),
        };
        let job = Job::new(self.profile.id, JobKind::BridgeClip, to_json(&payload));
        let bridge = Bridge {
            id,
            project: project.id,
            owner: self.profile.id,
            cut,
            from: span.from,
            to: span.to,
            first,
            last,
            provider: ask.provider,
            model: ask.model,
            seconds: ask.seconds,
            note,
            job: job.id(),
            file: None,
            asked_at: SystemTime::now(),
        };
        self.bridges.save_bridge(&bridge)?;
        if let Err(error) = self.jobs.enqueue(job) {
            self.bridges.delete_bridge(id)?;
            return Err(BridgeError::Queue(error.to_string()));
        }
        self.refresh_editor(editor)?;
        Ok(id)
    }

    /// Puts the made bridge `id` on its cut: its clip becomes a project
    /// media asset, placed over its stretch with one undoable edit.
    pub fn use_bridge(&self, editor: &mut Editor, id: BridgeId) -> Result<(), BridgeError> {
        self.refresh_editor(editor)?;
        editor.preview_bridge(None);
        let view = editor
            .view()
            .bridges
            .iter()
            .find(|bridge| bridge.id == id)
            .cloned()
            .ok_or(BridgeError::NotFound)?;
        let file = match (&view.state, &view.file) {
            (BridgeState::Ready { .. }, Some(file)) => file.clone(),
            _ => return Err(BridgeError::NotReady),
        };
        let span = view.span().ok_or(BridgeError::CutChanged)?;
        let project = editor.project();
        let timeline = editor
            .view()
            .timeline
            .clone()
            .ok_or(EditorError::NothingToCut)?;
        // Checked before anything is saved: the stretch must still fit.
        let (edit, placed) = timeline.put_bridge(&span, bridge_item(&file, view.seconds))?;
        let asset = self.bridge_asset(project, &file, &view, span, timeline.aspect())?;
        self.media_assets.save_media_asset(&asset)?;
        self.apply_edit(editor, edit, Some(placed))?;
        self.bridges.delete_bridge(id)?;
        remove_frames(self.files.as_ref(), project, id);
        self.refresh_editor(editor)?;
        editor.select(Some(ItemRef::video(placed.index)));
        Ok(())
    }

    /// The media asset a used bridge's clip becomes, as ffmpeg reads it;
    /// as it was asked for, in the cut's shape, when ffmpeg cannot read it now.
    fn bridge_asset(
        &self,
        project: VideoProjectId,
        file: &str,
        view: &BridgeView,
        span: BridgeSpan,
        aspect: bardo_domain::AspectRatio,
    ) -> Result<MediaAsset, BridgeError> {
        let info = self.media.probe(&self.files.path(project, file)).ok();
        let duration = info
            .as_ref()
            .map(|info| info.duration)
            .filter(|duration| !duration.is_zero())
            .unwrap_or(Duration::from_secs(u64::from(view.seconds)));
        // The cut's own frame when ffprobe cannot say.
        let picture = info
            .and_then(|info| info.video)
            .map(|video| PictureSize::new(video.width, video.height))
            .unwrap_or_else(|| {
                let (width, height) = bardo_domain::Resolution::FullHd1080.dimensions(aspect);
                PictureSize::new(width, height)
            });
        Ok(MediaAsset {
            id: MediaAssetId::new(),
            project,
            owner: self.profile.id,
            kind: MediaKind::Video,
            source: AssetSource::Generated,
            file: file.to_owned(),
            name: format!("AI bridge {}", timecode(span.start())),
            duration,
            picture: Some(picture),
            imported_at: SystemTime::now(),
        })
    }

    /// Drops bridge `id` and its clip. One still being made is cancelled
    /// from Jobs first.
    pub fn discard_bridge(&self, editor: &mut Editor, id: BridgeId) -> Result<(), BridgeError> {
        let project = editor.project();
        let bridge = self
            .bridges
            .bridge(id)?
            .filter(|bridge| bridge.project == project)
            .ok_or(BridgeError::NotFound)?;
        let making = self
            .jobs()
            .iter()
            .any(|job| job.id() == bridge.job && job.state().is_active())
            || self.jobs.runs_here(bridge.job);
        if making {
            return Err(BridgeError::StillMaking);
        }
        editor.preview_bridge(None);
        self.bridges.delete_bridge(id)?;
        if let Some(file) = &bridge.file {
            crate::proxies::remove_media(self.files.as_ref(), project, file);
        }
        remove_frames(self.files.as_ref(), project, id);
        self.refresh_editor(editor)?;
        Ok(())
    }

    /// The paid call a retry of bridge job `job` would still make: none
    /// once its request is out (it is asked about, not paid for again),
    /// its clip is made or the bridge is gone.
    pub(crate) fn bridge_left(&self, job: &Job) -> Option<(VideoProjectId, Vec<PlannedCall>)> {
        let payload = bridge_payload(job)?;
        let project = VideoProjectId::from(payload.project.parse::<uuid::Uuid>().ok()?);
        let bridge = self.bridges.bridge(id(&payload.bridge).ok()?).ok()??;
        if bridge.file.is_some() {
            return None;
        }
        let state: ClipsState = job
            .external_handle()
            .and_then(|saved| serde_json::from_str(saved).ok())
            .unwrap_or_default();
        let out = state
            .scenes
            .iter()
            .any(|run| run.request.is_some() || run.sent_at.is_some());
        if out || state.delivered.contains(&ONLY) {
            return None;
        }
        let provider: Provider = payload.provider.parse().ok()?;
        Some((
            project,
            vec![
                PlannedCall::new(provider, CostPurpose::BridgeClip, 1)
                    .with_model(&payload.model)
                    .with_video_seconds(bridge.seconds),
            ],
        ))
    }
}

#[cfg(test)]
mod tests;
