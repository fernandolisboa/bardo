//! Scene and image use cases (PRD stories 34-36, 42, 44): Claude splits a
//! video project's narration into scenes on its sentences, so each scene is
//! timed by the word timings, and writes one image prompt per scene from the
//! image prompt template; the user edits any prompt; Nano Banana draws one
//! image per scene. Any single image can be drawn again: the new one waits
//! beside the current one until the user accepts it.
//!
//! Planning and drawing call providers, so they run as jobs. The plan's
//! template is rendered when the job is queued, so the job sends exactly the
//! prompt it records. The image job draws its scenes one by one and saves
//! each image as soon as it arrives: a scene the provider declines does not
//! stop the others, and retrying the job draws only the scenes it has not
//! drawn yet. Every image records its generation (provider, model, prompt,
//! template version and tokens).
//!
//! The plan names the video's recurring characters (#246). Before the
//! first scene that shows one is drawn, Nano Banana draws their reference
//! portrait, and every scene showing them is drawn with it, so a person
//! keeps one face from scene to scene. The user can edit a member's
//! description or draw the portrait again; scenes already drawn keep
//! theirs until drawn again.
//!
//! The plan is a storyboard (#258): it names the recurring places once,
//! each with one fixed description, and says which place each scene
//! happens in. A scene is drawn with its place's description and with
//! other scenes' images as references (the first image of its place, the
//! scene right before it there, or the video's first image for its look),
//! so the scenes of a video read as one sequence. The scenes are drawn in
//! order, which is what gives each one its earlier shots.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, AspectRatio, CastDraft, CastMember, CostPurpose, Generation, GenerationId, ImageFormat,
    ImageGenerator, ImageRequest, ImageShape, Job, JobFailure, JobFailureKind, JobId, JobKind,
    Narration, NarrationId, NarrationRepository, NoPendingClip, NoPendingImage, NoSuchCastMember,
    NoSuchPlace, NoSuchScene, OtherShot, PlaceDraft, PlanDraft, ProfileId, Progress, ProjectFiles,
    Provider, ReferenceImage, RenderedPrompt, RepositoryError, SceneDraft, SceneFieldError,
    SceneImage, ScenePlan, ScenePlanId, ScenePlanRepository, ScenePlanSize, ScenePrompt,
    SceneSetting, SecretStore, TemplateKind, TemplateUsed, TemplateVariable, TemplateVersion,
    TemplateVersionId, TextEffort, TextFormat, TextGenerator, TextRequest, VideoProject,
    VideoProjectId, scene_image_prompt, sentences,
};
use bardo_media::MediaEngine;
use serde::{Deserialize, Serialize};

use crate::clips::{ClipsView, SceneClipView};
use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, KeyState, ScriptError, TemplateError, Text};

#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    /// The typed prompt breaks a rule; the editor shows it.
    #[error("invalid prompt: {0:?}")]
    Invalid(SceneFieldError),
    #[error("video project not found")]
    ProjectNotFound,
    /// Scenes are timed on the narration: generate it first.
    #[error("the project has no narration yet")]
    NoNarration,
    /// Images are drawn for planned scenes: plan them first.
    #[error("the project has no scene plan yet")]
    NoPlan,
    #[error(transparent)]
    SceneNotFound(#[from] NoSuchScene),
    #[error(transparent)]
    CastMemberNotFound(#[from] NoSuchCastMember),
    #[error(transparent)]
    PlaceNotFound(#[from] NoSuchPlace),
    /// Generation calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// Scenes or images of the project are being generated.
    #[error("scenes or images are already being generated for this project")]
    Busy,
    /// The narration has more scenes than one plan can hold.
    #[error("the narration is too long to plan its scenes")]
    NarrationTooLong,
    /// Planning again would discard this many images; ask first.
    #[error("planning again discards {0} images")]
    WouldDiscardImages(usize),
    /// Every scene already has an image.
    #[error("every scene already has an image")]
    NothingToGenerate,
    /// The work would reach a provider's budget; the screen asks before
    /// starting it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    #[error(transparent)]
    NothingToReview(#[from] NoPendingImage),
    #[error(transparent)]
    NoClipToReview(#[from] NoPendingClip),
    /// Clips animate a scene's image: draw it first.
    #[error("the scene has no image to animate")]
    NoImageToAnimate,
    /// Every scene with an image has a clip or one being made.
    #[error("every scene with an image already has a clip")]
    NothingToAnimate,
    /// The chosen video model is not one the providers offer.
    #[error("the video model is not offered")]
    ClipModelNotOffered,
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl SceneError {
    /// What the projects screen says.
    pub fn message(&self) -> Text {
        match self {
            SceneError::Invalid(error) => Text::SceneFieldError(*error),
            SceneError::ProjectNotFound => Text::ProjectNotFound,
            SceneError::NoNarration => Text::ScenesNoNarration,
            SceneError::NoPlan => Text::ScenesNoPlan,
            SceneError::NarrationTooLong => Text::ScenesNarrationTooLong,
            SceneError::SceneNotFound(_)
            | SceneError::CastMemberNotFound(_)
            | SceneError::PlaceNotFound(_) => Text::SceneNotFound,
            SceneError::MissingKey(Provider::Claude) => Text::ScenesMissingClaudeKey,
            SceneError::MissingKey(Provider::Higgsfield) => Text::ScenesMissingHiggsfieldKey,
            SceneError::MissingKey(Provider::Kling) => Text::ScenesMissingKlingKey,
            SceneError::MissingKey(Provider::HiggsfieldPlan) => Text::ScenesMissingHiggsfieldPlan,
            SceneError::MissingKey(_) => Text::ScenesMissingGeminiKey,
            SceneError::Busy => Text::ScenesBusy,
            SceneError::WouldDiscardImages(_) => Text::ScenesWouldDiscardImages,
            SceneError::NothingToGenerate => Text::ScenesNothingToGenerate,
            SceneError::OverBudget(_) => Text::BudgetReachedTitle,
            SceneError::NothingToReview(_) | SceneError::NoClipToReview(_) => {
                Text::SceneNothingToReview
            }
            SceneError::NoImageToAnimate => Text::SceneNoImageToAnimate,
            SceneError::NothingToAnimate => Text::ScenesNothingToAnimate,
            SceneError::ClipModelNotOffered => Text::SceneClipModelNotOffered,
            SceneError::Template(error) => error.message(),
            SceneError::Repository(_) => Text::ScenesNotLoaded,
        }
    }

    pub fn field_error(&self) -> Option<SceneFieldError> {
        match self {
            SceneError::Invalid(error) => Some(*error),
            _ => None,
        }
    }
}

/// A video project's scenes panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenesView {
    pub project: VideoProject,
    /// The narration scenes are timed on; `None` until one is generated.
    pub narration: Option<NarrationId>,
    /// `None` until the scenes are planned.
    pub plan: Option<ScenePlan>,
    /// Whether the narration changed since the plan was made.
    pub stale: bool,
    /// The project's latest scene plan or image job.
    pub job: Option<Job>,
    /// The template version the next plan uses.
    pub template: TemplateVersion,
    /// What planning the scenes would cost; `None` without a narration.
    pub plan_estimate: Option<SpendEstimate>,
    /// What drawing the missing images would cost; `None` when none is
    /// missing.
    pub images_estimate: Option<SpendEstimate>,
    /// What drawing each scene again would cost, its people's missing
    /// portraits included; empty without a plan.
    pub image_estimates: Vec<SpendEstimate>,
    /// The scenes' clips: models, lengths, prices and jobs.
    pub clips: ClipsView,
    /// For a version: the scenes it carried and where they stand.
    pub version: Option<crate::VersionScenes>,
}

/// Claude's call that plans the scenes of `narration` from `rendered`. The
/// answer grows with the narration, and the thought before it is billed as
/// output too: half as much again as the answer is assumed for it, until
/// plans of this length have been paid for and say what they took.
fn plan_call(rendered: &RenderedPrompt, narration: &Narration) -> PlannedCall {
    let answer = ScenePlanSize::of(narration).answer_tokens_usually();
    PlannedCall::new(Provider::Claude, CostPurpose::ScenePlan, 1)
        .with_prompt(&rendered.instructions, &rendered.prompt)
        .with_output_tokens_at_least(answer.saturating_add(answer / 2))
}

/// The room a plan of `narration` asks Claude to leave for its answer, or
/// that no single answer can hold it: a plan cut at the model's limit is
/// paid for and lost, so it is not asked for.
fn plan_room(narration: &Narration) -> Result<u64, SceneError> {
    let room = ScenePlanSize::of(narration).answer_tokens_at_most();
    if room > bardo_ai::claude::longest_answer() {
        return Err(SceneError::NarrationTooLong);
    }
    Ok(room)
}

/// Nano Banana's calls that draw `images` images, portraits included.
fn images_call(images: usize) -> PlannedCall {
    PlannedCall::new(Provider::Gemini, CostPurpose::SceneImage, images as u64)
}

impl ScenesView {
    /// How many images "generate images" draws: the scenes without one,
    /// and the portraits of the people they show that have none yet.
    pub fn missing_images(&self) -> usize {
        self.plan.as_ref().map_or(0, |plan| {
            let missing = plan.missing_images();
            missing.len() + plan.missing_portraits(&missing).len()
        })
    }

    /// Whether a scene plan or image job of the project is running.
    pub fn is_busy(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }

    /// Whether a clip of some scene is being made.
    pub fn is_animating(&self) -> bool {
        self.clips.scenes.iter().any(SceneClipView::is_busy)
    }
}

/// The shape Claude answers in, in the order it thinks the video through:
/// the story the images tell from first to last, the cast, the places,
/// then the scenes, each with the cast members it shows (by their index
/// in `cast`), the place it happens in (by its index in `places`, -1 for
/// a place of its own) and how its clip moves. The story is only there to
/// be written before the scenes; Bardo keeps it in the plan's provenance.
/// The fields a template may not mention say what they are for, so a
/// template written before them still gets an answer Bardo can use.
const PLAN_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "story": {
      "type": "string",
      "description": "One short paragraph: the story the images tell from the first to the last, in order."
    },
    "cast": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "name": {"type": "string"},
          "description": {"type": "string"}
        },
        "required": ["name", "description"],
        "additionalProperties": false
      }
    },
    "places": {
      "type": "array",
      "description": "The places where more than one scene happens, each once, with one fixed description. Empty when no place recurs.",
      "items": {
        "type": "object",
        "properties": {
          "name": {"type": "string"},
          "description": {"type": "string"}
        },
        "required": ["name", "description"],
        "additionalProperties": false
      }
    },
    "scenes": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "first_sentence": {"type": "integer"},
          "place": {
            "type": "integer",
            "description": "The position in `places` (from 0) of the place this scene happens in, or -1 when it happens in none of them."
          },
          "prompt": {"type": "string"},
          "cast": {"type": "array", "items": {"type": "integer"}},
          "motion": {"type": "string"}
        },
        "required": ["first_sentence", "place", "prompt", "cast", "motion"],
        "additionalProperties": false
      }
    }
  },
  "required": ["story", "cast", "places", "scenes"],
  "additionalProperties": false
}"#;

#[derive(Debug, Deserialize)]
struct PlannedScenes {
    #[serde(default)]
    cast: Vec<PlannedMember>,
    #[serde(default)]
    places: Vec<PlannedMember>,
    scenes: Vec<PlannedScene>,
}

/// A cast member or a place: a name and its fixed description.
#[derive(Debug, Deserialize)]
struct PlannedMember {
    name: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct PlannedScene {
    first_sentence: i64,
    prompt: String,
    #[serde(default)]
    cast: Vec<i64>,
    #[serde(default)]
    motion: String,
    /// Negative for a scene in a place of its own.
    #[serde(default)]
    place: Option<i64>,
}

/// The scene plan job's payload: the rendered prompt, where it came from,
/// and the narration it was written from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PlanPayload {
    project: String,
    narration: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
}

/// The image job's payload: the plan and which of its scenes to draw.
/// Each scene's prompt is read when its turn comes, so an edit made while
/// the job waits (or before a retry) is what gets drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ImagesPayload {
    project: String,
    plan: String,
    scenes: Vec<usize>,
    /// The frame shape, from the project's format when the job was
    /// started; jobs from before formats drew wide images.
    #[serde(default = "landscape")]
    aspect: String,
    /// Cast members whose portrait the user asked to draw again, drawn
    /// before any scene (#246).
    #[serde(default)]
    portraits: Vec<usize>,
}

fn landscape() -> String {
    AspectRatio::Landscape.code().to_owned()
}

/// The field both payloads share, to find a project's jobs.
#[derive(Debug, Deserialize)]
struct ProjectOf {
    project: String,
}

pub(crate) fn parse<T: for<'de> Deserialize<'de>>(payload: &str) -> Result<T, JobFailure> {
    serde_json::from_str(payload)
        .map_err(|e| JobFailure::unexpected(format!("invalid scene payload: {e}")))
}

pub(crate) fn to_json(payload: &impl Serialize) -> String {
    serde_json::to_string(payload).expect("a scene payload serializes")
}

pub(crate) fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

pub(crate) fn id<T: From<uuid::Uuid>>(text: &str) -> Result<T, JobFailure> {
    uuid::Uuid::parse_str(text).map(T::from).map_err(unexpected)
}

/// `m:ss`.
pub(crate) fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The narration as the planner reads it: one sentence per line, with its
/// number and when it is spoken.
pub(crate) fn sentence_lines(narration: &Narration) -> String {
    let text = narration.text.as_str();
    sentences(narration)
        .iter()
        .enumerate()
        .map(|(index, sentence)| {
            let words: Vec<&str> = text[sentence.text.clone()].split_whitespace().collect();
            format!(
                "[{index}] {}-{} {}",
                clock(sentence.start),
                clock(sentence.end),
                words.join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The scene image's file name in the project folder.
fn image_file(generation: GenerationId, extension: &str) -> String {
    format!("scene-{generation}.{extension}")
}

/// A cast member's portrait's file name in the project folder.
fn portrait_file(generation: GenerationId, extension: &str) -> String {
    format!("cast-{generation}.{extension}")
}

/// The format of an image file Bardo saved, from its extension.
fn saved_format(file: &str) -> Option<ImageFormat> {
    let extension = file.rsplit_once('.')?.1.to_ascii_lowercase();
    [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Webp]
        .into_iter()
        .find(|format| format.extension() == extension)
}

/// How many bytes one reference image takes at most. A drawing at 2K is
/// several megabytes as it was saved, and five of them would not fit in a
/// request, so a larger one goes as a JPEG of the same size within this.
const REFERENCE_BYTES: usize = 2 * 1024 * 1024;

/// How many bytes of reference images one image request carries at most.
/// Google's documentation gives 20 MB for a whole request with inline
/// images in one place and 100 MB in another; this keeps to the smaller
/// (base64 adds a third), with room for five references.
const MAX_REFERENCE_BYTES: usize = 12 * 1024 * 1024;

/// The name of the small copy of the image `file` that goes as a
/// reference, kept beside it in the project folder.
pub(crate) fn reference_name(file: &str) -> String {
    format!("reference-{file}.jpg")
}

/// The image `file` as a JPEG within [`REFERENCE_BYTES`]: the copy made
/// before, or a new one, kept for the next request. `None` when the image
/// is small enough as it is, or will not be rewritten (then it goes as it
/// was saved, if the request has room).
fn small_copy(
    files: &dyn ProjectFiles,
    media: &dyn MediaEngine,
    project: VideoProjectId,
    file: &str,
) -> Option<Vec<u8>> {
    let name = reference_name(file);
    if let Ok(bytes) = files.read(project, &name) {
        return Some(bytes);
    }
    if files.size(project, file).ok()? <= REFERENCE_BYTES as u64 {
        return None;
    }
    let folder = tempfile::Builder::new()
        .prefix("bardo-reference-")
        .tempdir()
        .ok()?;
    let source = folder.path().join(file);
    let destination = folder.path().join("reference.jpg");
    std::fs::write(&source, files.read(project, file).ok()?).ok()?;
    if let Err(error) = media.fit_picture(&source, &destination, REFERENCE_BYTES as u64) {
        tracing::warn!("could not make a small copy of a reference image: {error}");
        return None;
    }
    let bytes = std::fs::read(&destination).ok()?;
    let _ = files.write(project, &name, &bytes);
    Some(bytes)
}

/// The reference images of one image request, in the order they are sent.
struct References {
    images: Vec<ReferenceImage>,
    /// How many more bytes of images the request takes.
    room: usize,
}

impl Default for References {
    fn default() -> Self {
        Self {
            images: Vec::new(),
            room: MAX_REFERENCE_BYTES,
        }
    }
}

impl References {
    /// Adds the project's image `file`, as a small copy when it is large;
    /// `false` when it is not an image Bardo saved, will not read, or
    /// would take the request past [`MAX_REFERENCE_BYTES`], and the
    /// request then goes without it.
    fn add(
        &mut self,
        files: &dyn ProjectFiles,
        media: &dyn MediaEngine,
        project: VideoProjectId,
        file: &str,
    ) -> bool {
        let Some(format) = saved_format(file) else {
            return false;
        };
        let small = small_copy(files, media, project, file).map(|bytes| (bytes, ImageFormat::Jpeg));
        let Some((bytes, format)) =
            small.or_else(|| Some((files.read(project, file).ok()?, format)))
        else {
            return false;
        };
        let Some(room) = self.room.checked_sub(bytes.len()) else {
            return false;
        };
        self.room = room;
        self.images.push(ReferenceImage { bytes, format });
        true
    }
}

/// Drawing failures that are about one scene (its prompt, the answer to
/// it): the job goes on with the other scenes. Any other failure (key,
/// quota, outage) would hit every scene, so the job stops.
fn is_about_the_scene(kind: JobFailureKind) -> bool {
    matches!(
        kind,
        JobFailureKind::Declined | JobFailureKind::UnexpectedAnswer
    )
}

/// Runs scene plan and image jobs.
pub(crate) struct SceneHandler {
    pub(crate) owner: ProfileId,
    pub(crate) plans: Arc<dyn ScenePlanRepository>,
    pub(crate) narrations: Arc<dyn NarrationRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) images: Arc<dyn ImageGenerator>,
    /// Makes the small copies of the images sent as references.
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for SceneHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        match cx.kind() {
            JobKind::ScenePlan => self.plan(payload, cx.id()),
            _ => self.draw(payload, cx.id(), Some(cx)),
        }
    }
}

impl SceneHandler {
    fn key(&self, provider: Provider) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, provider)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(
                    JobFailureKind::MissingKey,
                    format!("no {provider} key is saved"),
                )
            })
    }

    /// Plans the scenes `job` asked for and makes them the project's plan.
    fn plan(&self, payload: &str, job: JobId) -> Result<(), JobFailure> {
        let payload: PlanPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let current = self.plans.scene_plan(project).map_err(unexpected)?;
        // An earlier attempt may have saved the plan and stopped before the
        // queue recorded it as done.
        if current
            .as_ref()
            .is_some_and(|plan| plan.generation.job == Some(job))
        {
            return Ok(());
        }
        let narration = self
            .narrations
            .narration(project)
            .map_err(unexpected)?
            .filter(|narration| narration.id.to_string() == payload.narration)
            .ok_or_else(|| {
                JobFailure::unexpected("the narration changed while the scenes were planned")
            })?;

        let key = self.key(Provider::Claude)?;
        let request = TextRequest {
            effort: TextEffort::Careful,
            // Checked before the job was queued; a narration that does not
            // fit gets all the room there is.
            answer_tokens: Some(ScenePlanSize::of(&narration).answer_tokens_at_most()),
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            images: Vec::new(),
            format: TextFormat::Json {
                schema: PLAN_SCHEMA.to_owned(),
            },
        };
        let generated = self.text.generate(&key, &request).map_err(|failure| {
            JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
        })?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::ScenePlan,
                usage: generated.usage.into(),
                job: Some(job),
                reported: None,
            },
            project,
        );
        let not_usable = |detail: String| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Claude: the scene plan is not usable ({detail})"),
            )
        };
        let planned: PlannedScenes =
            serde_json::from_str(&generated.text).map_err(|e| not_usable(e.to_string()))?;
        let cast = planned
            .cast
            .into_iter()
            .map(|member| CastDraft {
                name: member.name,
                description: member.description,
            })
            .collect();
        let places = planned
            .places
            .into_iter()
            .map(|place| PlaceDraft {
                name: place.name,
                description: place.description,
            })
            .collect();
        let scenes = planned
            .scenes
            .into_iter()
            .filter_map(|scene| {
                Some(SceneDraft {
                    first_sentence: usize::try_from(scene.first_sentence).ok()?,
                    prompt: scene.prompt,
                    cast: scene
                        .cast
                        .into_iter()
                        .filter_map(|member| usize::try_from(member).ok())
                        .collect(),
                    motion: scene.motion,
                    place: scene.place.and_then(|place| usize::try_from(place).ok()),
                })
            })
            .collect();
        let now = SystemTime::now();
        let generation = Generation {
            id: GenerationId::new(),
            owner: self.owner,
            project,
            provider: Provider::Claude,
            model: generated.model,
            template: TemplateUsed {
                id: id::<TemplateVersionId>(&payload.template)?,
                number: payload.template_number,
            },
            instructions: payload.instructions,
            prompt: payload.prompt,
            output: generated.text,
            usage: generated.usage,
            generated_at: now,
            job: Some(job),
        };
        let draft = PlanDraft {
            cast,
            places,
            scenes,
        };
        let plan = ScenePlan::from_draft(generation, &narration, draft, now)
            .map_err(|error| not_usable(error.to_string()))?;
        self.plans.save_scene_plan(&plan).map_err(unexpected)?;
        // The replaced plan's images go with it. A file that will not go
        // (open in a viewer) only costs disk space.
        for file in current.iter().flat_map(ScenePlan::files) {
            crate::proxies::remove_media(self.files.as_ref(), project, file);
        }
        Ok(())
    }

    /// The saved plan, if it is still `plan`.
    fn saved_plan(
        &self,
        project: VideoProjectId,
        plan: ScenePlanId,
    ) -> Result<Option<ScenePlan>, JobFailure> {
        Ok(self
            .plans
            .scene_plan(project)
            .map_err(unexpected)?
            .filter(|saved| saved.id == plan))
    }

    /// Draws the scenes `job` asked for, saving each image as it arrives,
    /// and before them the portraits asked for and those of the people
    /// the scenes show that have none yet. Without a context (tests
    /// replaying a job) nothing reports progress or stops it.
    fn draw(
        &self,
        payload: &str,
        job: JobId,
        mut cx: Option<&mut JobContext>,
    ) -> Result<(), JobFailure> {
        let payload: ImagesPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let plan_id: ScenePlanId = id(&payload.plan)?;
        let aspect: AspectRatio = payload
            .aspect
            .parse()
            .map_err(|_| unexpected(format!("unknown frame shape {}", payload.aspect)))?;
        // Portraits drawn on the way to a scene are counted as they come.
        let mut total = payload.portraits.len() + payload.scenes.len();
        let mut failed: Vec<JobFailure> = Vec::new();
        let mut key = None;
        // Portraits this run drew or tried: one that fails is not tried
        // again for every scene its member is in.
        let mut tried: Vec<usize> = Vec::new();
        let mut done = 0;
        let step = |cx: &mut Option<&mut JobContext>, done: &mut usize, total: usize| {
            *done += 1;
            match cx.as_mut() {
                Some(cx) => cx
                    .save_checkpoint(
                        format!("{} of {} images", done, total),
                        Progress::of(*done as u64, total as u64),
                    )
                    .map_err(unexpected),
                None => Ok(()),
            }
        };
        for &member in &payload.portraits {
            let Some(plan) = self.saved_plan(project, plan_id)? else {
                return Ok(());
            };
            let Ok(cast) = plan.cast_member(member) else {
                continue;
            };
            tried.push(member);
            let drawn = cast
                .portrait()
                .is_some_and(|portrait| portrait.generation.job == Some(job));
            if !drawn {
                if cx.as_ref().is_some_and(|cx| cx.should_stop()) {
                    return Ok(());
                }
                let key = match &key {
                    Some(key) => key,
                    None => key.insert(self.key(Provider::Gemini)?),
                };
                self.draw_portrait(key, project, plan_id, member, job, &mut failed)?;
            }
            step(&mut cx, &mut done, total)?;
        }
        for &index in &payload.scenes {
            // A new plan replaced this one: its scenes are gone.
            let Some(plan) = self.saved_plan(project, plan_id)? else {
                return Ok(());
            };
            let Ok(scene) = plan.scene(index) else {
                continue;
            };
            let drawn = scene
                .image()
                .into_iter()
                .chain(scene.pending())
                .any(|image| image.generation.job == Some(job));
            if drawn {
                continue;
            }
            if cx.as_ref().is_some_and(|cx| cx.should_stop()) {
                return Ok(());
            }
            let key = match &key {
                Some(key) => key,
                None => key.insert(self.key(Provider::Gemini)?),
            };
            // The people it shows get their portrait first.
            for member in plan.missing_portraits(&[index]) {
                if !tried.contains(&member) {
                    tried.push(member);
                    total += 1;
                    self.draw_portrait(key, project, plan_id, member, job, &mut failed)?;
                }
            }
            let Some(plan) = self.saved_plan(project, plan_id)? else {
                return Ok(());
            };
            let Ok(scene) = plan.scene(index) else {
                continue;
            };
            let shown: Vec<&CastMember> = plan
                .scene_cast(index)
                .into_iter()
                .filter_map(|member| plan.cast_member(member).ok())
                .collect();
            let mut references = References::default();
            // A portrait that will not read is described instead.
            let referenced: Vec<bool> = shown
                .iter()
                .map(|member| {
                    member.portrait().is_some_and(|portrait| {
                        references.add(
                            self.files.as_ref(),
                            self.media.as_ref(),
                            project,
                            &portrait.file,
                        )
                    })
                })
                .collect();
            // Then the other scenes it belongs with; one whose image will
            // not read is left out.
            let shots: Vec<OtherShot> = plan
                .reference_shots(index)
                .into_iter()
                .filter(|(other, _)| {
                    plan.scene(*other)
                        .ok()
                        .and_then(|other| other.image())
                        .is_some_and(|image| {
                            references.add(
                                self.files.as_ref(),
                                self.media.as_ref(),
                                project,
                                &image.file,
                            )
                        })
                })
                .map(|(_, shot)| shot)
                .collect();
            let setting = SceneSetting {
                place: plan
                    .scene_place(index)
                    .and_then(|place| plan.place(place).ok()),
                shots: &shots,
            };
            let references = references.images;
            let prompt = scene_image_prompt(scene.prompt().as_str(), &shown, &referenced, setting);
            let request = ImageRequest {
                prompt: prompt.clone(),
                shape: aspect.into(),
                references,
            };
            match self.images.generate(key, &request) {
                Ok(image) => {
                    self.costs.record_for_project(
                        PaidCall {
                            provider: Provider::Gemini,
                            model: &image.model,
                            purpose: CostPurpose::SceneImage,
                            usage: image.usage,
                            job: Some(job),
                            reported: None,
                        },
                        project,
                    );
                    let generation = GenerationId::new();
                    let file = image_file(generation, image.format.extension());
                    self.files
                        .write(project, &file, &image.bytes)
                        .map_err(unexpected)?;
                    let image = SceneImage {
                        file: file.clone(),
                        generation: Generation {
                            id: generation,
                            owner: self.owner,
                            project,
                            provider: Provider::Gemini,
                            model: image.model,
                            template: plan.generation.template,
                            instructions: String::new(),
                            prompt,
                            output: file.clone(),
                            usage: image.usage.tokens(),
                            generated_at: SystemTime::now(),
                            job: Some(job),
                        },
                    };
                    // Read again: the user may have changed the scene while
                    // it was drawn, and their change must survive.
                    let Some(mut plan) = self.saved_plan(project, plan_id)? else {
                        crate::proxies::remove_media(self.files.as_ref(), project, &file);
                        return Ok(());
                    };
                    let pushed = plan
                        .scene_mut(index, SystemTime::now())
                        .map_err(unexpected)?
                        .add_image(image);
                    self.plans.save_scene(&plan, index).map_err(unexpected)?;
                    if let Some(pushed) = pushed {
                        crate::proxies::remove_media(self.files.as_ref(), project, &pushed.file);
                    }
                }
                Err(failure) => {
                    let kind = JobFailureKind::from(failure.kind);
                    let failure = JobFailure::new(
                        kind,
                        format!("Gemini, scene {}: {}", index + 1, failure.detail),
                    );
                    if let Some(mut plan) = self.saved_plan(project, plan_id)? {
                        plan.scene_mut(index, SystemTime::now())
                            .map_err(unexpected)?
                            .fail(kind);
                        self.plans.save_scene(&plan, index).map_err(unexpected)?;
                    }
                    if !is_about_the_scene(kind) {
                        return Err(failure);
                    }
                    failed.push(failure);
                }
            }
            step(&mut cx, &mut done, total)?;
        }
        match failed.first() {
            None => Ok(()),
            Some(first) => Err(JobFailure::new(
                first.kind,
                format!(
                    "{} of {} images were not drawn. {}",
                    failed.len(),
                    total,
                    failed
                        .iter()
                        .map(|failure| failure.detail.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
            )),
        }
    }

    /// Draws cast member `member`'s portrait and puts it in place of the
    /// current one. A failure about the portrait is recorded on the member
    /// and in `failed`, and the scenes are drawn without it; any other
    /// failure stops the job.
    fn draw_portrait(
        &self,
        key: &ApiKey,
        project: VideoProjectId,
        plan_id: ScenePlanId,
        member: usize,
        job: JobId,
        failed: &mut Vec<JobFailure>,
    ) -> Result<(), JobFailure> {
        let Some(plan) = self.saved_plan(project, plan_id)? else {
            return Ok(());
        };
        let Ok(cast) = plan.cast_member(member) else {
            return Ok(());
        };
        let name = cast.name.clone();
        let description = cast.description().clone();
        let prompt = cast.portrait_prompt();
        let request = ImageRequest {
            prompt: prompt.clone(),
            shape: ImageShape::Portrait,
            references: Vec::new(),
        };
        match self.images.generate(key, &request) {
            Ok(image) => {
                self.costs.record_for_project(
                    PaidCall {
                        provider: Provider::Gemini,
                        model: &image.model,
                        purpose: CostPurpose::SceneImage,
                        usage: image.usage,
                        job: Some(job),
                        reported: None,
                    },
                    project,
                );
                let generation = GenerationId::new();
                let file = portrait_file(generation, image.format.extension());
                self.files
                    .write(project, &file, &image.bytes)
                    .map_err(unexpected)?;
                let portrait = SceneImage {
                    file: file.clone(),
                    generation: Generation {
                        id: generation,
                        owner: self.owner,
                        project,
                        provider: Provider::Gemini,
                        model: image.model,
                        template: plan.generation.template,
                        instructions: String::new(),
                        prompt,
                        output: file.clone(),
                        usage: image.usage.tokens(),
                        generated_at: SystemTime::now(),
                        job: Some(job),
                    },
                };
                // Read again: a description the user changed meanwhile is
                // not what this portrait shows.
                let Some(mut plan) = self.saved_plan(project, plan_id)? else {
                    crate::proxies::remove_media(self.files.as_ref(), project, &file);
                    return Ok(());
                };
                let still = plan
                    .cast_member(member)
                    .is_ok_and(|cast| *cast.description() == description);
                if !still {
                    crate::proxies::remove_media(self.files.as_ref(), project, &file);
                    return Ok(());
                }
                let replaced = plan
                    .cast_mut(member, SystemTime::now())
                    .map_err(unexpected)?
                    .set_portrait(portrait);
                self.plans
                    .save_cast_member(&plan, member)
                    .map_err(unexpected)?;
                if let Some(replaced) = replaced {
                    crate::proxies::remove_media(self.files.as_ref(), project, &replaced.file);
                }
                Ok(())
            }
            Err(failure) => {
                let kind = JobFailureKind::from(failure.kind);
                let failure = JobFailure::new(
                    kind,
                    format!("Gemini, portrait of {name}: {}", failure.detail),
                );
                // A failure is about the description it was drawn from.
                if let Some(mut plan) = self.saved_plan(project, plan_id)?
                    && let Ok(cast) = plan.cast_mut(member, SystemTime::now())
                    && *cast.description() == description
                {
                    cast.fail(kind);
                    self.plans
                        .save_cast_member(&plan, member)
                        .map_err(unexpected)?;
                }
                if !is_about_the_scene(kind) {
                    return Err(failure);
                }
                failed.push(failure);
                Ok(())
            }
        }
    }
}

impl Bardo {
    pub(crate) fn scenes_project(&self, id: VideoProjectId) -> Result<VideoProject, SceneError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(SceneError::ProjectNotFound)
    }

    /// Whether a scene plan or image job of the project is running.
    pub(crate) fn scenes_busy(&self, project: VideoProjectId) -> bool {
        self.latest_scene_job(project)
            .is_some_and(|job| job.state().is_active())
    }

    fn latest_scene_job(&self, project: VideoProjectId) -> Option<Job> {
        let project = project.to_string();
        self.jobs().into_iter().rev().find(|job| {
            matches!(job.kind(), JobKind::ScenePlan | JobKind::SceneImages)
                && serde_json::from_str::<ProjectOf>(job.payload())
                    .is_ok_and(|p| p.project == project)
        })
    }

    fn scenes_idle(&self, project: VideoProjectId) -> Result<(), SceneError> {
        if self
            .latest_scene_job(project)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(SceneError::Busy);
        }
        Ok(())
    }

    pub(crate) fn require_key(&self, provider: Provider) -> Result<(), SceneError> {
        if self.provider_key(provider).state == KeyState::NotSet {
            return Err(SceneError::MissingKey(provider));
        }
        Ok(())
    }

    pub(crate) fn own_plan(&self, project: VideoProjectId) -> Result<ScenePlan, SceneError> {
        let project = self.scenes_project(project)?;
        self.scene_plans
            .scene_plan(project.id)?
            .ok_or(SceneError::NoPlan)
    }

    /// The project's scenes panel.
    pub fn scenes(&self, project: VideoProjectId) -> Result<ScenesView, SceneError> {
        let project = self.scenes_project(project)?;
        let narration = self.narrations.narration(project.id)?;
        let plan = self.scene_plans.scene_plan(project.id)?;
        let template = self.current_template(TemplateKind::ImagePrompt)?;
        let plan_estimate = match &narration {
            Some(narration) => {
                let rendered = self.render_plan(&project, narration, &template)?;
                Some(self.estimate(&[plan_call(&rendered, narration)])?)
            }
            None => None,
        };
        let missing = plan.as_ref().map_or(0, |plan| {
            let missing = plan.missing_images();
            missing.len() + plan.missing_portraits(&missing).len()
        });
        let clips = self.clips_view(&project, plan.as_ref())?;
        let version = self.version_scenes(project.id, plan.as_ref(), narration.as_ref())?;
        Ok(ScenesView {
            clips,
            version,
            narration: narration.as_ref().map(|narration| narration.id),
            stale: plan
                .as_ref()
                .is_some_and(|plan| plan.is_stale(narration.as_ref())),
            images_estimate: match missing {
                0 => None,
                n => Some(self.estimate(&[images_call(n)])?),
            },
            image_estimates: match &plan {
                Some(plan) => (0..plan.scenes().len())
                    .map(|index| {
                        let portraits = plan.missing_portraits(&[index]).len();
                        self.estimate(&[images_call(1 + portraits)])
                    })
                    .collect::<Result<_, _>>()?,
                None => Vec::new(),
            },
            plan,
            job: self.latest_scene_job(project.id),
            template,
            plan_estimate,
            project,
        })
    }

    /// The image prompt template filled with the project's facts and its
    /// narration's sentences.
    fn render_plan(
        &self,
        project: &VideoProject,
        narration: &Narration,
        template: &TemplateVersion,
    ) -> Result<RenderedPrompt, SceneError> {
        let mut values = self.script_values(project).map_err(|error| match error {
            ScriptError::Repository(error) => SceneError::Repository(error),
            _ => SceneError::ProjectNotFound,
        })?;
        values.insert(
            TemplateVariable::NarrationSentences,
            sentence_lines(narration),
        );
        template.body.render(&values).map_err(|missing| {
            // Every image prompt variable has a value above.
            SceneError::Repository(RepositoryError(Box::new(missing)))
        })
    }

    /// Past a budget without `consent`, the estimate to ask about.
    fn scenes_budget(
        &self,
        project: VideoProjectId,
        call: PlannedCall,
        consent: BudgetConsent,
    ) -> Result<(), SceneError> {
        self.check_video_budget(project, &[call], consent)?
            .map_err(SceneError::OverBudget)
    }

    /// Starts a job in which Claude splits the project's narration into
    /// scenes with the current image prompt template. The new plan replaces
    /// the current one when it is ready; when that one has images, they go
    /// with it, so `discard_images` must say so. Past Claude's budget it
    /// needs `consent`.
    pub fn plan_scenes(
        &self,
        project: VideoProjectId,
        discard_images: bool,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let project = self.scenes_project(project)?;
        let narration = self
            .narrations
            .narration(project.id)?
            .ok_or(SceneError::NoNarration)?;
        self.scenes_idle(project.id)?;
        if self.clips_busy(project.id) {
            return Err(SceneError::Busy);
        }
        let images = self
            .scene_plans
            .scene_plan(project.id)?
            .map_or(0, |plan| plan.files().count());
        if images > 0 && !discard_images {
            return Err(SceneError::WouldDiscardImages(images));
        }
        self.require_key(Provider::Claude)?;
        plan_room(&narration)?;
        let template = self.current_template(TemplateKind::ImagePrompt)?;
        let rendered = self.render_plan(&project, &narration, &template)?;
        self.scenes_budget(project.id, plan_call(&rendered, &narration), consent)?;
        let payload = PlanPayload {
            project: project.id.to_string(),
            narration: narration.id.to_string(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
        };
        let job = Job::new(self.profile.id, JobKind::ScenePlan, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Replaces scene `index`'s prompt with the user's. The next image of
    /// the scene is drawn from it.
    pub fn edit_scene_prompt(
        &self,
        project: VideoProjectId,
        index: usize,
        prompt: &str,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        let prompt = ScenePrompt::new(prompt).map_err(SceneError::Invalid)?;
        if plan
            .scene_mut(index, SystemTime::now())?
            .edit_prompt(prompt)
        {
            self.scene_plans.save_scene(&plan, index)?;
        }
        Ok(plan)
    }

    fn draw_scenes(
        &self,
        plan: &ScenePlan,
        scenes: Vec<usize>,
        portraits: Vec<usize>,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        self.scenes_idle(plan.project)?;
        self.require_key(Provider::Gemini)?;
        let first = plan
            .missing_portraits(&scenes)
            .into_iter()
            .filter(|member| !portraits.contains(member))
            .count();
        self.scenes_budget(
            plan.project,
            images_call(portraits.len() + first + scenes.len()),
            consent,
        )?;
        let project = self
            .themes
            .project(plan.project)?
            .ok_or(SceneError::ProjectNotFound)?;
        let payload = ImagesPayload {
            project: plan.project.to_string(),
            plan: plan.id.to_string(),
            scenes,
            aspect: self.video_format(&project)?.aspect().code().to_owned(),
            portraits,
        };
        let job = Job::new(self.profile.id, JobKind::SceneImages, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Starts a job that draws every scene without an image. Past
    /// Gemini's budget it needs `consent`.
    pub fn generate_scene_images(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let plan = self.own_plan(project)?;
        let missing = plan.missing_images();
        if missing.is_empty() {
            return Err(SceneError::NothingToGenerate);
        }
        self.draw_scenes(&plan, missing, Vec::new(), consent)
    }

    /// Starts a job that draws scene `index` again. Its current image stays
    /// until the user accepts the new one; the other scenes are untouched.
    pub fn regenerate_scene_image(
        &self,
        project: VideoProjectId,
        index: usize,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let plan = self.own_plan(project)?;
        plan.scene(index)?;
        self.draw_scenes(&plan, vec![index], Vec::new(), consent)
    }

    /// Starts a job that draws cast member `member`'s portrait again. It
    /// replaces the current one once drawn; scenes already drawn keep
    /// their images until drawn again.
    pub fn redraw_cast_portrait(
        &self,
        project: VideoProjectId,
        member: usize,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let plan = self.own_plan(project)?;
        plan.cast_member(member)?;
        self.draw_scenes(&plan, Vec::new(), vec![member], consent)
    }

    /// Replaces cast member `member`'s description with the user's. Its
    /// portrait no longer shows it, so it goes; the next scene drawn with
    /// the member draws a new one first.
    pub fn edit_cast_description(
        &self,
        project: VideoProjectId,
        member: usize,
        description: &str,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        // A job drawing the scenes would draw some with the old look.
        self.scenes_idle(plan.project)?;
        let description = ScenePrompt::new(description).map_err(SceneError::Invalid)?;
        let (changed, dropped) = plan
            .cast_mut(member, SystemTime::now())?
            .edit_description(description);
        if changed {
            self.scene_plans.save_cast_member(&plan, member)?;
        }
        if let Some(dropped) = dropped {
            crate::proxies::remove_media(self.files.as_ref(), plan.project, &dropped.file);
        }
        Ok(plan)
    }

    /// Replaces place `place`'s description with the user's. The next
    /// images drawn there follow it; scenes already drawn keep theirs
    /// until drawn again, and those images stop being references of the
    /// place.
    pub fn edit_place_description(
        &self,
        project: VideoProjectId,
        place: usize,
        description: &str,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        // A job drawing the scenes would draw some in the old place.
        self.scenes_idle(plan.project)?;
        let description = ScenePrompt::new(description).map_err(SceneError::Invalid)?;
        if plan
            .place_mut(place, SystemTime::now())?
            .edit_description(description)
        {
            self.scene_plans.save_place(&plan, place)?;
        }
        Ok(plan)
    }

    /// Makes scene `index`'s new image its image; the replaced file goes.
    pub fn accept_scene_image(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        let replaced = plan.scene_mut(index, SystemTime::now())?.accept_image()?;
        self.scene_plans.save_scene(&plan, index)?;
        if let Some(replaced) = replaced {
            crate::proxies::remove_media(self.files.as_ref(), plan.project, &replaced.file);
        }
        Ok(plan)
    }

    /// Drops scene `index`'s new image and keeps the current one.
    pub fn reject_scene_image(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        let dropped = plan.scene_mut(index, SystemTime::now())?.reject_image()?;
        self.scene_plans.save_scene(&plan, index)?;
        crate::proxies::remove_media(self.files.as_ref(), plan.project, &dropped.file);
        Ok(plan)
    }

    /// Where a scene image is on disk, for the screen to show it.
    pub fn scene_image_path(&self, image: &SceneImage) -> PathBuf {
        self.files.path(image.generation.project, &image.file)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Instant;

    use bardo_domain::{
        Alignment, ChannelDraft, CharTiming, ContentLanguage, Country, JobState, ProviderFailure,
        ProviderFailureKind, ScriptText, TokenUsage, WordTimings,
    };
    use bardo_storage::{Database, MemoryProjectFiles, MemorySecretStore};

    use super::*;
    use crate::testing::{
        FakeClips, FakeDecisionEngine, FakeImages, FakeKeyChecker, FakeMarketData, FakeSpeech,
        FakeTextGenerator, FakeVoiceLibrary, SCENE_IMAGE,
    };
    use crate::{JobSettings, Providers, Repositories};

    const CLAUDE_KEY: &str = "sk-ant-api03-test-key-0001";
    pub(crate) const ELEVENLABS_KEY: &str = "sk_test_elevenlabs_key_0001";
    pub(crate) const GEMINI_KEY: &str = "AIzaSyTest-gemini-key-0001";
    const HIGGSFIELD_KEY: &str = "hf-key-id-0001:hf-key-secret-0001";
    const PATIENCE: Duration = Duration::from_secs(10);
    pub(crate) const SCRIPT: &str = "Era uma vez, em 1969, uma sonda. Ela partiu para longe. \
                          O sinal sumiu em março. Ninguém sabe por quê.";

    /// Three scenes: the launch (sentence 0), the departure (1) and the
    /// lost signal (2 and 3).
    pub(crate) fn plan_answer() -> String {
        serde_json::json!({"scenes": [
            {"first_sentence": 0, "prompt": "A probe on the launch pad, archival photo"},
            {"first_sentence": 1, "prompt": "The probe drifting away from Earth"},
            {"first_sentence": 2, "prompt": "A control room after the signal is lost"},
        ]})
        .to_string()
    }

    pub(crate) struct Harness {
        pub(crate) db: Arc<Database>,
        /// Where the background agent is registered.
        pub(crate) agent_task: Arc<bardo_storage::MemoryAgentTask>,
        pub(crate) files: Arc<MemoryProjectFiles>,
        pub(crate) text: Arc<FakeTextGenerator>,
        pub(crate) images: Arc<FakeImages>,
        /// The video's music.
        pub(crate) music: Arc<crate::testing::FakeMusic>,
        pub(crate) clips: Arc<FakeClips>,
        /// Video providers after the fake one.
        pub(crate) more_clips: Vec<Arc<dyn bardo_domain::ClipGenerator>>,
        /// Plans signed in to instead of a key (ADR-0028).
        pub(crate) plans: Vec<Arc<dyn bardo_domain::PlanSignIn>>,
        pub(crate) secrets: Arc<MemorySecretStore>,
        /// Writes its proxies into `files`.
        pub(crate) media: Arc<crate::editor::testing::FakeMedia>,
        pub(crate) audio: Arc<crate::narrations::testing::FakeAudioOutput>,
        /// Public statistics of linked posts.
        pub(crate) stats: Arc<crate::testing::FakeVideoStats>,
        pub(crate) decisions: Arc<FakeDecisionEngine>,
        pub(crate) speech: Arc<FakeSpeech>,
        /// YouTube uploads.
        pub(crate) uploader: Arc<crate::uploads::testing::FakeUploader>,
        /// Instagram Reels uploads.
        pub(crate) reels: Arc<crate::uploads::testing::FakeUploader>,
        /// TikTok drafts.
        pub(crate) tiktok: Arc<crate::uploads::testing::FakeUploader>,
        /// X posts.
        pub(crate) x: Arc<crate::uploads::testing::FakeUploader>,
        /// App credentials and network tokens.
        pub(crate) connection_secrets: Arc<MemorySecretStore>,
        /// Owner metrics of connected accounts.
        pub(crate) analytics: Arc<crate::testing::FakeAnalytics>,
        /// Instagram and TikTok numbers of connected accounts.
        pub(crate) instagram_insights: Arc<crate::testing::FakeInsights>,
        pub(crate) tiktok_insights: Arc<crate::testing::FakeInsights>,
        /// Backups the app makes and restores; each start gets its own
        /// in memory when `None`.
        pub(crate) backups: Option<Arc<dyn bardo_domain::BackupStore>>,
        /// Bardo's releases, for updates.
        pub(crate) releases: Arc<crate::updates::testing::FakeReleases>,
        /// Downloaded updates' installers, kept across starts.
        pub(crate) installers: Arc<dyn bardo_domain::InstallerStore>,
        pub(crate) installer_runner: Arc<dyn bardo_domain::InstallerRunner>,
    }

    impl Harness {
        pub(crate) fn new() -> Self {
            let files: Arc<MemoryProjectFiles> = Arc::default();
            Self {
                db: Arc::new(Database::open_in_memory().unwrap()),
                agent_task: Arc::default(),
                media: Arc::new(crate::editor::testing::FakeMedia::writing_to(Arc::clone(
                    &files,
                ))),
                audio: Arc::default(),
                files,
                text: Arc::default(),
                images: Arc::default(),
                music: Arc::default(),
                clips: Arc::default(),
                more_clips: Vec::new(),
                plans: Vec::new(),
                secrets: Arc::default(),
                stats: Arc::default(),
                decisions: Arc::default(),
                speech: Arc::default(),
                uploader: Arc::default(),
                reels: Arc::new(crate::uploads::testing::FakeUploader::reels()),
                tiktok: Arc::new(crate::uploads::testing::FakeUploader::tiktok()),
                x: Arc::new(crate::uploads::testing::FakeUploader::x()),
                connection_secrets: Arc::default(),
                analytics: Arc::default(),
                instagram_insights: Arc::new(crate::testing::FakeInsights::instagram()),
                tiktok_insights: Arc::new(crate::testing::FakeInsights::tiktok()),
                backups: None,
                releases: Arc::default(),
                installers: Arc::new(bardo_storage::MemoryInstallers::default()),
                installer_runner: Arc::new(bardo_storage::MemoryInstallerRunner::default()),
            }
        }

        pub(crate) fn start(&self) -> Bardo {
            self.start_from(Repositories::shared_with_files(
                self.process_db(),
                Arc::clone(&self.secrets) as _,
                Arc::clone(&self.files) as _,
            ))
        }

        /// `start` with export packages written to `export_files`.
        pub(crate) fn start_with_exports(
            &self,
            export_files: Arc<dyn bardo_domain::ExportFiles>,
        ) -> Bardo {
            self.start_from(Repositories {
                export_files,
                ..Repositories::shared_with_files(
                    self.process_db(),
                    Arc::clone(&self.secrets) as _,
                    Arc::clone(&self.files) as _,
                )
            })
        }

        /// `start` with the editor's cuts kept by `timelines`.
        pub(crate) fn start_with_timelines(
            &self,
            timelines: Arc<dyn bardo_domain::TimelineRepository>,
        ) -> Bardo {
            self.start_from(Repositories {
                timelines,
                ..Repositories::shared_with_files(
                    self.process_db(),
                    Arc::clone(&self.secrets) as _,
                    Arc::clone(&self.files) as _,
                )
            })
        }

        /// The background agent over the same data, as its own process
        /// opens it, if it starts.
        pub(crate) fn start_agent(&self) -> Option<Bardo> {
            let repositories = Repositories {
                connection_secrets: Arc::clone(&self.connection_secrets) as _,
                agent_task: Arc::clone(&self.agent_task) as _,
                ..Repositories::shared_with_files(
                    self.process_db(),
                    Arc::clone(&self.secrets) as _,
                    Arc::clone(&self.files) as _,
                )
            };
            Bardo::start_agent_with(repositories, self.providers(), Self::job_settings()).unwrap()
        }

        /// The database as a new process opens it: the same data, under a
        /// job runner of its own, so a restart cannot run on as the closed
        /// app's runner.
        pub(crate) fn process_db(&self) -> Arc<Database> {
            Arc::new(self.db.other_runner())
        }

        /// Quick retries, and a quick tick so the app and the agent see
        /// each other's changes soon.
        fn job_settings() -> JobSettings {
            JobSettings {
                retry: bardo_domain::RetryPolicy {
                    max_attempts: 2,
                    first_delay: Duration::from_millis(20),
                    max_delay: Duration::from_millis(20),
                },
                lease: Duration::from_secs(2),
                tick: Duration::from_millis(50),
                ..JobSettings::default()
            }
        }

        pub(crate) fn start_from(&self, repositories: Repositories) -> Bardo {
            let backups = self
                .backups
                .clone()
                .unwrap_or_else(|| Arc::clone(&repositories.backups));
            let repositories = Repositories {
                backups,
                connection_secrets: Arc::clone(&self.connection_secrets) as _,
                agent_task: Arc::clone(&self.agent_task) as _,
                installers: Arc::clone(&self.installers),
                installer_runner: Arc::clone(&self.installer_runner),
                ..repositories
            };
            let mut app = Bardo::start_with(
                repositories,
                self.providers(),
                Some("en-US"),
                Self::job_settings(),
            )
            .unwrap();
            app.accept_privacy_policy().unwrap();
            for (provider, key) in [
                (Provider::Claude, CLAUDE_KEY),
                (Provider::ElevenLabs, ELEVENLABS_KEY),
                (Provider::Gemini, GEMINI_KEY),
                (Provider::Higgsfield, HIGGSFIELD_KEY),
            ] {
                app.save_provider_key(provider, key).unwrap();
            }
            app
        }

        fn providers(&self) -> Providers {
            Providers {
                key_checker: Arc::new(FakeKeyChecker::default()),
                market_data: Arc::new(FakeMarketData::default()),
                video_stats: Arc::clone(&self.stats) as _,
                text: Arc::clone(&self.text) as _,
                decisions: vec![Arc::clone(&self.decisions) as _],
                voices: Arc::new(FakeVoiceLibrary::default()),
                speech: Arc::clone(&self.speech) as _,
                previews: Arc::new(crate::testing::NoPreviews),
                aligner: Arc::new(crate::narration_import::testing::FakeAligner::default()),
                transcriber: Arc::new(crate::captures::testing::FakeTranscriber::default()),
                voice_changer: Arc::new(crate::captures::testing::FakeVoiceChanger::default()),
                voice_isolator: Arc::new(crate::captures::testing::FakeVoiceIsolator::default()),
                vision: Arc::new(crate::take_analysis::testing::FakeVision::default()),
                images: Arc::clone(&self.images) as _,
                music: Arc::clone(&self.music) as _,
                clips: std::iter::once(Arc::clone(&self.clips) as _)
                    .chain(self.more_clips.iter().cloned())
                    .collect(),
                takes: vec![Arc::new(crate::testing::FakeTakes::default())],
                audio: Arc::clone(&self.audio) as _,
                media: Arc::clone(&self.media) as _,
                sign_ins: Vec::new(),
                plans: self.plans.clone(),
                consent: Arc::new(crate::connections::testing::NoConsent),
                uploaders: vec![
                    Arc::clone(&self.uploader) as _,
                    Arc::clone(&self.reels) as _,
                    Arc::clone(&self.tiktok) as _,
                    Arc::clone(&self.x) as _,
                ],
                analytics: vec![Arc::clone(&self.analytics) as _],
                post_insights: vec![
                    Arc::clone(&self.instagram_insights) as _,
                    Arc::clone(&self.tiktok_insights) as _,
                ],
                releases: Arc::clone(&self.releases) as _,
            }
        }

        pub(crate) fn answer(&self, text: String) {
            self.text.answers.lock().unwrap().push(text);
        }

        /// A project with a script and its narration.
        pub(crate) fn narrated_project(&self, app: &Bardo) -> VideoProject {
            self.answer(SCRIPT.to_owned());
            let project = project(app);
            done(
                app,
                app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
            );
            done(
                app,
                app.generate_narration(project.id, BudgetConsent::Ask)
                    .unwrap(),
            );
            project
        }

        /// A narrated project with its scenes planned.
        pub(crate) fn planned_project(&self, app: &Bardo) -> (VideoProject, ScenePlan) {
            let project = self.narrated_project(app);
            self.answer(plan_answer());
            done(
                app,
                app.plan_scenes(project.id, false, BudgetConsent::Ask)
                    .unwrap(),
            );
            let plan = app.scenes(project.id).unwrap().plan.unwrap();
            (project, plan)
        }

        /// A planned project with every scene drawn.
        pub(crate) fn drawn_project(&self, app: &Bardo) -> (VideoProject, ScenePlan) {
            let (project, _) = self.planned_project(app);
            done(
                app,
                app.generate_scene_images(project.id, BudgetConsent::Ask)
                    .unwrap(),
            );
            let plan = app.scenes(project.id).unwrap().plan.unwrap();
            (project, plan)
        }
    }

    pub(crate) fn wait_done(app: &Bardo, id: JobId) -> Job {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(job) = app
                .jobs()
                .into_iter()
                .find(|j| j.id() == id && !j.state().is_active())
            {
                return job;
            }
            assert!(Instant::now() < deadline, "job {id} never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub(crate) fn done(app: &Bardo, id: JobId) -> Job {
        let job = wait_done(app, id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        job
    }

    /// A channel of long videos whose default persona is the documentary
    /// narrator, and a project on one of its themes.
    pub(crate) fn project(app: &Bardo) -> VideoProject {
        let persona = app
            .personas()
            .unwrap()
            .into_iter()
            .find(|p| p.details.name() == "Documentary Narrator (en-US)")
            .unwrap();
        let channel = app
            .create_channel(ChannelDraft {
                name: format!("Space Archives {}", app.channels().unwrap().len() + 1),
                niche: "space history".into(),
                aesthetic_notes: "grainy 1960s film".into(),
                language: ContentLanguage::Portuguese,
                country: Country::Brazil,
                default_persona: Some(persona.id),
                format: bardo_domain::VideoFormat::Long,
                ..ChannelDraft::default()
            })
            .unwrap();
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel.id,
            bardo_domain::Niche::new("space history").unwrap(),
            bardo_domain::ThemeIdea::new("The probe that never came home", "").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let project = theme.approve(SystemTime::now()).unwrap();
        app.themes.start_project(&theme, &project).unwrap();
        project
    }

    fn images_of(plan: &ScenePlan) -> Vec<Option<&SceneImage>> {
        plan.scenes().iter().map(|scene| scene.image()).collect()
    }

    #[test]
    fn claude_splits_the_whole_narration_into_timed_scenes() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        let narration = app.narrations.narration(project.id).unwrap().unwrap();
        let before = app.scenes(project.id).unwrap();
        assert_eq!(before.plan, None);
        assert_eq!(before.narration, Some(narration.id));

        h.answer(plan_answer());
        let job = done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );

        let request = h.text.requests().pop().unwrap();
        assert!(matches!(request.format, TextFormat::Json { .. }));
        assert!(request.instructions.contains("Portuguese"));
        assert!(request.prompt.contains("grainy 1960s film"));
        assert!(request.prompt.contains("[0] 0:00-"));
        assert!(request.prompt.contains("[3] "));
        assert!(request.prompt.contains("Ninguém sabe por quê."));

        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.job.as_ref().map(Job::id), Some(job.id()));
        assert!(!view.stale);
        assert_eq!(view.missing_images(), 3);
        let plan = view.plan.unwrap();
        assert_eq!(plan.narration, narration.id);
        let scenes = plan.scenes();
        assert_eq!(scenes.len(), 3);
        assert_eq!(scenes[0].start, Duration::ZERO);
        assert_eq!(scenes[0].end, scenes[1].start);
        assert_eq!(scenes[1].end, scenes[2].start);
        assert!(scenes[2].end >= narration.duration);
        assert_eq!(scenes[0].text, "Era uma vez, em 1969, uma sonda.");
        assert_eq!(
            scenes[2].text,
            "O sinal sumiu em março. Ninguém sabe por quê."
        );
        assert_eq!(
            scenes[1].prompt().as_str(),
            "The probe drifting away from Earth"
        );

        let generation = &plan.generation;
        assert_eq!(generation.provider, Provider::Claude);
        assert_eq!(generation.model, "claude-fake");
        assert_eq!(generation.template.id, view.template.id);
        assert_eq!(generation.prompt, request.prompt);
        assert_eq!(generation.output, plan_answer());
        assert_eq!(generation.job, Some(job.id()));
    }

    /// The plan of `plan_answer` with a chef in the first and last scenes.
    /// Replaces `project`'s narration with one of `sentences` short
    /// sentences that lasts `length`.
    fn narrate_at_length(
        app: &Bardo,
        project: &VideoProject,
        sentences: usize,
        length: Duration,
    ) -> Narration {
        let mut narration = app.narrations.narration(project.id).unwrap().unwrap();
        let text = vec!["Segue."; sentences].join(" ");
        let chars: Vec<char> = text.chars().collect();
        let step = length / chars.len() as u32;
        let alignment = Alignment {
            chars: chars
                .iter()
                .enumerate()
                .map(|(n, c)| CharTiming {
                    text: c.to_string(),
                    start: step * n as u32,
                    end: step * (n as u32 + 1),
                })
                .collect(),
        };
        narration.words = WordTimings::from_alignment(&text, &alignment);
        narration.text = ScriptText::new(&text).unwrap();
        narration.duration = length;
        app.narrations.save_narration(&narration).unwrap();
        narration
    }

    #[test]
    fn the_plan_asks_for_room_by_the_length_of_the_narration() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        // The harness narrates for one second: one scene at most.
        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        let request = h.text.requests().pop().unwrap();
        assert_eq!(request.answer_tokens, Some(2_500 + 250));

        // Ten minutes with a sentence every four seconds: up to 150 scenes,
        // far more than the room a request gets when it does not say.
        let narration = narrate_at_length(&app, &project, 150, Duration::from_secs(600));
        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        let request = h.text.requests().pop().unwrap();
        assert_eq!(request.effort, TextEffort::Careful);
        assert_eq!(
            request.answer_tokens,
            Some(ScenePlanSize::of(&narration).answer_tokens_at_most())
        );
        assert_eq!(request.answer_tokens, Some(2_500 + 150 * 250));
    }

    #[test]
    fn a_narration_too_long_for_one_plan_is_refused_before_it_costs() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        let asked = h.text.requests().len();
        let jobs = app.jobs().len();

        // 340 scenes at most: more than one answer holds with its thought.
        narrate_at_length(&app, &project, 340, Duration::from_secs(23 * 60));
        let error = app
            .plan_scenes(project.id, false, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, SceneError::NarrationTooLong));
        assert_eq!(error.message(), Text::ScenesNarrationTooLong);
        assert_eq!(h.text.requests().len(), asked, "Claude was not asked");
        assert_eq!(app.jobs().len(), jobs, "and no job waits to ask");

        // A little shorter fits.
        let narration = narrate_at_length(&app, &project, 330, Duration::from_secs(22 * 60));
        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        let request = h.text.requests().pop().unwrap();
        let room = request.answer_tokens.unwrap();
        assert_eq!(room, ScenePlanSize::of(&narration).answer_tokens_at_most());
        assert!(room <= bardo_ai::claude::longest_answer());
    }

    #[test]
    fn the_estimate_of_a_plan_follows_the_length_of_the_narration() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        let template = app.current_template(TemplateKind::ImagePrompt).unwrap();
        let with_output = |narration: &Narration, tokens: u64| {
            let rendered = app.render_plan(&project, narration, &template).unwrap();
            let call = PlannedCall::new(Provider::Claude, CostPurpose::ScenePlan, 1)
                .with_prompt(&rendered.instructions, &rendered.prompt)
                .with_output_tokens(tokens);
            app.estimate(&[call]).unwrap()
        };
        let estimate = || app.scenes(project.id).unwrap().plan_estimate.unwrap();

        // A Short: the first guess covers its few scenes and their thought.
        let short = app.narrations.narration(project.id).unwrap().unwrap();
        assert_eq!(estimate(), with_output(&short, 12_000));

        // Ten minutes: a hundred scenes usually (16,600 tokens) and half as
        // much again of thought.
        let long = narrate_at_length(&app, &project, 150, Duration::from_secs(600));
        assert_eq!(estimate(), with_output(&long, 24_900));
        assert!(estimate().total() > with_output(&long, 12_000).total());

        // A plan that took more than that is what the next one is
        // estimated from.
        *h.text.usage.lock().unwrap() = TokenUsage {
            input_tokens: 9_000,
            output_tokens: 40_000,
        };
        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(estimate(), with_output(&long, 40_000));
    }

    fn cast_answer() -> String {
        serde_json::json!({
            "cast": [
                {"name": "the chef", "description": "A man in his fifties, brown skin, grey beard, white apron"},
            ],
            "scenes": [
                {"first_sentence": 0, "prompt": "The chef at the launch pad, archival photo",
                 "cast": [0], "motion": "Slow push in on the chef"},
                {"first_sentence": 1, "prompt": "The probe drifting away from Earth",
                 "cast": [], "motion": "The probe drifts left"},
                {"first_sentence": 2, "prompt": "The chef alone in a control room",
                 "cast": [0], "motion": "Static shot, the chef lowers his head"},
            ],
        })
        .to_string()
    }

    fn cast_project(h: &Harness, app: &Bardo) -> VideoProject {
        let project = h.narrated_project(app);
        h.answer(cast_answer());
        done(
            app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        project
    }

    #[test]
    fn a_recurring_person_gets_one_portrait_sent_with_every_scene_they_are_in() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        let request = h.text.requests().pop().unwrap();
        let TextFormat::Json { schema } = &request.format else {
            panic!("the plan is asked as JSON");
        };
        assert!(schema.contains("\"cast\"") && schema.contains("\"motion\""));
        assert!(request.instructions.contains("fixed"));

        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.missing_images(), 4, "three scenes and the chef");
        let plan = view.plan.unwrap();
        assert_eq!(plan.cast().len(), 1);
        assert_eq!(plan.scenes_with(0), [0, 2]);
        assert_eq!(
            plan.scenes()[2].motion_prompt().as_str(),
            "Static shot, the chef lowers his head",
            "clips move as planned"
        );

        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let requests = h.images.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4, "the chef is drawn once");
        assert_eq!(requests[0].shape, ImageShape::Portrait);
        assert!(
            requests[0]
                .prompt
                .contains("the chef: A man in his fifties")
        );
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        let portrait = plan.cast()[0].portrait().unwrap().clone();
        assert!(portrait.file.starts_with("cast-"));
        let bytes = h.files.read(project.id, &portrait.file).unwrap();
        for scene in [1, 3] {
            let request = &requests[scene];
            assert_eq!(request.references[0].bytes, bytes, "scene {scene}");
            assert!(
                request
                    .prompt
                    .contains("Reference 1: the chef, A man in his fifties")
            );
        }
        assert_eq!(requests[1].references.len(), 1, "the first image");
        // The later scenes also get the first image, for its look.
        let first = &plan.scenes()[0].image().unwrap().file;
        let first = h.files.read(project.id, first).unwrap();
        assert_eq!(requests[2].references.len(), 1, "nobody recurs there");
        assert_eq!(requests[2].references[0].bytes, first);
        assert!(
            requests[2]
                .prompt
                .starts_with("The probe drifting away from Earth\n\n")
        );
        assert!(
            requests[2]
                .prompt
                .contains("Reference 1: another shot of this video.")
        );
        assert_eq!(requests[3].references.len(), 2);
        assert_eq!(requests[3].references[1].bytes, first);
        assert!(
            requests[3]
                .prompt
                .contains("Reference 2: another shot of this video.")
        );
        assert_eq!(
            plan.scenes()[0].image().unwrap().generation.prompt,
            requests[1].prompt,
            "the generation records what was sent"
        );
        assert!(plan.files().any(|file| file == portrait.file));
    }

    #[test]
    fn a_declined_portrait_is_tried_once_and_its_scenes_describe_the_person() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        h.images.decline("reference portrait");

        let job = wait_done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::Declined);
        assert!(
            failure.detail.contains("portrait of the chef"),
            "{}",
            failure.detail
        );

        let requests = h.images.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4, "one portrait try, three scenes");
        assert!(requests[1].references.is_empty());
        assert!(
            requests[1]
                .prompt
                .contains("the chef: A man in his fifties, brown skin")
        );
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert_eq!(plan.cast()[0].failure(), Some(JobFailureKind::Declined));
        assert_eq!(plan.missing_images().len(), 0, "the scenes were drawn");

        // Drawing the portrait again works once the provider accepts it.
        h.images.accept_all();
        done(
            &app,
            app.redraw_cast_portrait(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert!(plan.cast()[0].portrait().is_some());
        assert_eq!(plan.cast()[0].failure(), None);
        assert_eq!(h.images.requests.lock().unwrap().len(), 5);
    }

    #[test]
    fn a_new_description_drops_the_portrait_and_the_next_scene_draws_one() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        let first = plan.cast()[0].portrait().unwrap().file.clone();

        let plan = app
            .edit_cast_description(project.id, 0, "A woman in her thirties, red hair")
            .unwrap();
        assert!(plan.cast()[0].portrait().is_none());
        assert!(plan.cast()[0].is_edited());
        assert!(!h.files.exists(project.id, &first), "the old portrait goes");
        assert!(matches!(
            app.edit_cast_description(project.id, 0, " "),
            Err(SceneError::Invalid(SceneFieldError::PromptRequired))
        ));
        assert!(matches!(
            app.edit_cast_description(project.id, 4, "Anyone"),
            Err(SceneError::CastMemberNotFound(_))
        ));

        done(
            &app,
            app.regenerate_scene_image(project.id, 2, BudgetConsent::Ask)
                .unwrap(),
        );
        let requests = h.images.requests.lock().unwrap().clone();
        let portrait = &requests[requests.len() - 2];
        assert_eq!(portrait.shape, ImageShape::Portrait);
        assert!(
            portrait
                .prompt
                .contains("A woman in her thirties, red hair")
        );
        let scene = requests.last().unwrap();
        assert_eq!(scene.references.len(), 2, "her portrait and another shot");
        assert_eq!(scene.references[0].bytes, crate::testing::LOOK_IMAGE);
        assert!(scene.prompt.contains("A woman in her thirties, red hair"));
    }

    /// The plan of `plan_answer` as a storyboard: the launch pad twice,
    /// then the control room.
    pub(crate) fn storyboard_answer() -> String {
        serde_json::json!({
            "story": "A probe is launched, drifts away and is lost; the control room goes quiet.",
            "cast": [],
            "places": [
                {"name": "the launch pad", "description": "A concrete pad by the sea at dawn, 1969, a white rocket, orange light"},
                {"name": "the control room", "description": "A dim room of green screens and grey consoles at night, 1970"},
                {"name": "the archive", "description": "Shelves of tapes"},
            ],
            "scenes": [
                {"first_sentence": 0, "place": 0, "prompt": "Wide shot of the launch pad, the rocket waiting",
                 "cast": [], "motion": "Slow push in"},
                {"first_sentence": 1, "place": 0, "prompt": "Close on the rocket lifting off the launch pad",
                 "cast": [], "motion": "Tilt up"},
                {"first_sentence": 2, "place": 0, "prompt": "The launch pad empty, smoke drifting over the sea",
                 "cast": [], "motion": "Slow drift"},
                {"first_sentence": 3, "place": 1, "prompt": "The control room, a screen gone dark",
                 "cast": [], "motion": "Static shot"},
            ],
        })
        .to_string()
    }

    pub(crate) fn storyboard_project(h: &Harness, app: &Bardo) -> VideoProject {
        let project = h.narrated_project(app);
        h.answer(storyboard_answer());
        done(
            app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        project
    }

    #[test]
    fn a_storyboard_names_its_places_and_each_scene_is_drawn_from_the_ones_before() {
        let h = Harness::new();
        let app = h.start();
        let project = storyboard_project(&h, &app);
        let request = h.text.requests().pop().unwrap();
        assert_eq!(request.effort, TextEffort::Careful);
        let TextFormat::Json { schema } = &request.format else {
            panic!("the plan is asked as JSON");
        };
        let at = |field: &str| schema.find(&format!("\"{field}\": {{")).unwrap();
        assert!(
            at("story") < at("cast") && at("cast") < at("places") && at("places") < at("scenes"),
            "the story is thought through before the scenes"
        );
        assert!(request.instructions.contains("storyboard"));

        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        let places: Vec<&str> = plan
            .places()
            .iter()
            .map(|place| place.name.as_str())
            .collect();
        assert_eq!(
            places,
            ["the launch pad", "the control room"],
            "a place no scene happens in is dropped"
        );
        let of: Vec<_> = plan.scenes().iter().map(|scene| scene.place()).collect();
        assert_eq!(of, [Some(0), Some(0), Some(0), Some(1)]);
        assert!(plan.generation.output.contains("\"story\""));

        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let requests = h.images.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        let references: Vec<_> = requests.iter().map(|r| r.references.len()).collect();
        assert_eq!(references, [0, 1, 2, 1]);
        // The first scene of a place: its description, nothing to follow.
        assert_eq!(
            requests[0].prompt,
            "Wide shot of the launch pad, the rocket waiting\n\nThe scene happens in the launch \
             pad: A concrete pad by the sea at dawn, 1969, a white rocket, orange light"
        );
        // The next one there continues from it.
        assert!(
            requests[1]
                .prompt
                .contains("The scene happens in the launch pad:")
        );
        assert!(
            requests[1]
                .prompt
                .contains("Reference 1: the shot right before this one, in this same place.")
        );
        // The third there gets the place's first image and the one before.
        assert!(
            requests[2]
                .prompt
                .contains("Reference 1: another shot in this same place.")
        );
        assert!(
            requests[2]
                .prompt
                .contains("Reference 2: the shot right before this one, in this same place.")
        );
        // A new place: its own description, and only the video's look.
        assert!(
            requests[3]
                .prompt
                .contains("The scene happens in the control room:")
        );
        assert!(
            requests[3]
                .prompt
                .contains("Reference 1: another shot of this video.")
        );
        assert!(!requests[3].prompt.contains("in this same place"));

        // Drawn again, the first scene follows the other shot of its place.
        done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        let again = h.images.requests.lock().unwrap().last().unwrap().clone();
        assert_eq!(again.references.len(), 1);
        assert!(
            again
                .prompt
                .contains("Reference 1: another shot in this same place.")
        );

        // Described again, the place is drawn from its new description: the
        // images of how it was only lend their look.
        app.edit_place_description(project.id, 0, "A pad in a desert at noon")
            .unwrap();
        done(
            &app,
            app.regenerate_scene_image(project.id, 2, BudgetConsent::Ask)
                .unwrap(),
        );
        let moved = h.images.requests.lock().unwrap().last().unwrap().clone();
        assert!(
            moved
                .prompt
                .contains("The scene happens in the launch pad: A pad in a desert at noon")
        );
        assert_eq!(moved.references.len(), 1);
        assert!(
            moved
                .prompt
                .contains("Reference 1: another shot of this video.")
        );
        assert!(!moved.prompt.contains("in this same place"));
    }

    #[test]
    fn a_place_described_again_is_what_the_next_images_there_show() {
        let h = Harness::new();
        let app = h.start();
        let project = storyboard_project(&h, &app);

        let plan = app
            .edit_place_description(project.id, 0, "A pad in a desert at noon, harsh light")
            .unwrap();
        assert!(plan.places()[0].is_edited());
        assert!(!plan.places()[1].is_edited());
        assert_eq!(
            app.scenes(project.id).unwrap().plan.unwrap().places(),
            plan.places(),
            "the edit is saved"
        );
        assert!(matches!(
            app.edit_place_description(project.id, 0, " "),
            Err(SceneError::Invalid(SceneFieldError::PromptRequired))
        ));
        assert!(matches!(
            app.edit_place_description(project.id, 7, "Anywhere"),
            Err(SceneError::PlaceNotFound(_))
        ));

        // A scene written somewhere else leaves the place.
        app.edit_scene_prompt(project.id, 1, "Close on the rocket above the clouds")
            .unwrap();
        *h.images.delay.lock().unwrap() = Duration::from_millis(150);
        let job = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert!(
            matches!(
                app.edit_place_description(project.id, 0, "A pad on the moon"),
                Err(SceneError::Busy)
            ),
            "a place does not change while its scenes are drawn"
        );
        done(&app, job);
        let prompts = h.images.prompts();
        assert!(prompts[0].contains("the launch pad: A pad in a desert at noon, harsh light"));
        assert!(!prompts[1].contains("The scene happens in"));
        assert!(prompts[1].contains("Reference 1: another shot of this video."));
    }

    #[test]
    fn a_reference_that_will_not_fit_or_read_is_left_out() {
        let files = MemoryProjectFiles::default();
        let project = VideoProjectId::new();
        files.write(project, "scene-a.png", &[1; 40]).unwrap();
        files.write(project, "scene-b.png", &[2; 40]).unwrap();
        files.write(project, "notes.txt", &[3; 4]).unwrap();
        let media = crate::editor::testing::FakeMedia::default();
        let mut references = References {
            images: Vec::new(),
            room: 60,
        };
        assert!(references.add(&files, &media, project, "scene-a.png"));
        assert!(
            !references.add(&files, &media, project, "scene-b.png"),
            "past the request's limit"
        );
        assert!(!references.add(&files, &media, project, "scene-gone.png"));
        assert!(!references.add(&files, &media, project, "notes.txt"));
        assert_eq!(references.images.len(), 1);
        assert_eq!(references.images[0].bytes, [1; 40]);
        assert_eq!(references.images[0].format, ImageFormat::Png);
        assert_eq!(references.room, 20);
        assert!(media.fitted.lock().unwrap().is_empty(), "small as they are");
    }

    #[test]
    fn a_large_reference_goes_as_a_small_copy_made_once() {
        let files = MemoryProjectFiles::default();
        let project = VideoProjectId::new();
        // A PNG header of a 2752 x 1536 drawing, then megabytes of pixels.
        let mut drawing = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        drawing.extend(2752u32.to_be_bytes());
        drawing.extend(1536u32.to_be_bytes());
        drawing.resize(REFERENCE_BYTES + 1, 7);
        files.write(project, "scene-a.png", &drawing).unwrap();
        // One that is not a picture cannot be rewritten: it goes as it is.
        files
            .write(project, "scene-b.png", &vec![9; REFERENCE_BYTES + 1])
            .unwrap();
        let media = crate::editor::testing::FakeMedia::default();

        let mut references = References::default();
        assert!(references.add(&files, &media, project, "scene-a.png"));
        assert_eq!(references.images[0].format, ImageFormat::Jpeg);
        assert!(references.images[0].bytes.starts_with(&[0xFF, 0xD8]));
        assert!(references.images[0].bytes.len() < 100);
        assert_eq!(
            *media.fitted.lock().unwrap(),
            [REFERENCE_BYTES as u64],
            "within a reference's size"
        );
        assert_eq!(
            files.read(project, "reference-scene-a.png.jpg").unwrap(),
            references.images[0].bytes,
            "kept beside the image"
        );
        assert!(references.add(&files, &media, project, "scene-b.png"));
        assert_eq!(references.images[1].format, ImageFormat::Png);
        assert_eq!(references.images[1].bytes.len(), REFERENCE_BYTES + 1);

        // The next request reads the copy instead of making it again.
        let mut again = References::default();
        assert!(again.add(&files, &media, project, "scene-a.png"));
        assert_eq!(again.images[0].format, ImageFormat::Jpeg);
        assert_eq!(media.fitted.lock().unwrap().len(), 1);

        // And the copy goes with its image.
        crate::proxies::remove_media(&files, project, "scene-a.png");
        assert!(!files.exists(project, "reference-scene-a.png.jpg"));
    }

    #[test]
    fn a_look_cannot_change_while_the_scenes_are_drawn() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        *h.images.delay.lock().unwrap() = Duration::from_millis(300);
        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert!(matches!(
            app.edit_cast_description(project.id, 0, "A woman, red hair"),
            Err(SceneError::Busy)
        ));
        *h.images.delay.lock().unwrap() = Duration::ZERO;
        done(&app, id);
        let view = app.scenes(project.id).unwrap();
        let plan = view.plan.unwrap();
        assert!(!plan.cast()[0].is_edited());
        assert!(plan.cast()[0].portrait().is_some());
        assert_eq!(view.image_estimates.len(), 3);
    }

    #[test]
    fn every_scene_gets_one_image_with_its_provenance() {
        let h = Harness::new();
        let app = h.start();
        let (project, planned) = h.planned_project(&app);

        let job = done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );

        // The first scene is drawn from its prompt alone; the others also
        // get the first image, for its look.
        let sent = h.images.prompts();
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[0], planned.scenes()[0].prompt().as_str());
        let requests = h.images.requests.lock().unwrap().clone();
        assert!(requests[0].references.is_empty());
        for (scene, request) in planned.scenes().iter().zip(&requests).skip(1) {
            let prompt = format!("{}\n\n", scene.prompt().as_str());
            assert!(request.prompt.starts_with(&prompt), "{}", request.prompt);
            assert!(request.prompt.contains("Match only how it is rendered"));
            assert!(!request.prompt.contains("The scene happens in"));
            assert_eq!(request.references.len(), 1);
            assert_eq!(request.references[0].bytes, SCENE_IMAGE);
        }
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        for (scene, sent) in plan.scenes().iter().zip(&sent) {
            let image = scene.image().unwrap();
            assert_eq!(scene.pending(), None);
            assert_eq!(scene.failure(), None);
            assert!(image.file.starts_with("scene-") && image.file.ends_with(".png"));
            assert_eq!(h.files.read(project.id, &image.file).unwrap(), SCENE_IMAGE);
            let generation = &image.generation;
            assert_eq!(generation.provider, Provider::Gemini);
            assert_eq!(generation.model, "nano-banana-fake");
            assert_eq!(&generation.prompt, sent, "what was sent is recorded");
            assert_eq!(generation.template, planned.generation.template);
            assert_eq!(generation.usage.output_tokens, 1_890);
            assert_eq!(generation.job, Some(job.id()));
        }
        assert!(matches!(
            app.generate_scene_images(project.id, BudgetConsent::Ask),
            Err(SceneError::NothingToGenerate)
        ));
    }

    #[test]
    fn every_paid_step_of_a_video_records_its_cost() {
        use bardo_domain::{CostPurpose, CostRepository};

        let h = Harness::new();
        let app = h.start();
        let (project, plan) = h.drawn_project(&app);

        let records = h.db.project_costs(project.id).unwrap();
        let count = |purpose| records.iter().filter(|r| r.purpose == purpose).count();
        assert_eq!(count(CostPurpose::Script), 1);
        assert!(count(CostPurpose::Narration) >= 1);
        assert_eq!(count(CostPurpose::ScenePlan), 1);
        assert_eq!(count(CostPurpose::SceneImage), plan.scenes().len());
        assert!(
            records
                .iter()
                .all(|r| r.channel == Some(project.channel) && r.job.is_some())
        );
        let image = records
            .iter()
            .find(|r| r.purpose == CostPurpose::SceneImage)
            .unwrap();
        assert_eq!(image.provider, Provider::Gemini);
        assert_eq!(image.usage.image_tokens, 1_680);
        let narration = records
            .iter()
            .find(|r| r.purpose == CostPurpose::Narration)
            .unwrap();
        assert!(narration.usage.characters > 0);
    }

    #[test]
    fn a_declined_scene_keeps_the_others_and_a_retry_draws_only_it() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        h.images.decline("signal");

        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, id);
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::Declined);
        assert!(
            failure.detail.contains("1 of 3 images"),
            "{}",
            failure.detail
        );

        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        let drawn: Vec<_> = images_of(&plan).iter().map(Option::is_some).collect();
        assert_eq!(drawn, [true, true, false]);
        assert_eq!(plan.scenes()[2].failure(), Some(JobFailureKind::Declined));
        let kept: Vec<SceneImage> = images_of(&plan).into_iter().flatten().cloned().collect();

        // The user softens the prompt, and the retry draws only that scene.
        app.edit_scene_prompt(project.id, 2, "An empty control room at night")
            .unwrap();
        h.images.accept_all();
        let asked = h.images.prompts().len();
        app.retry_job(id).unwrap();
        done(&app, id);

        let retried = &h.images.prompts()[asked..];
        assert_eq!(retried.len(), 1);
        assert!(retried[0].starts_with("An empty control room at night\n\n"));
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        let images: Vec<SceneImage> = images_of(&plan).into_iter().flatten().cloned().collect();
        assert_eq!(images.len(), 3);
        assert_eq!(images[..2], kept[..]);
        assert_eq!(plan.scenes()[2].failure(), None);
    }

    #[test]
    fn an_outage_stops_the_job_and_keeps_what_was_drawn() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        *h.images.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "API key not valid",
        ));

        let job = wait_done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );

        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(h.images.prompts().len(), 1, "a rejected key stops the job");
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert!(images_of(&plan).iter().all(Option::is_none));
    }

    #[test]
    fn a_new_image_waits_beside_the_current_one_until_accepted() {
        let h = Harness::new();
        let app = h.start();
        let (project, drawn) = h.drawn_project(&app);
        let old = drawn.scenes()[1].image().unwrap().clone();

        done(
            &app,
            app.regenerate_scene_image(project.id, 1, BudgetConsent::Ask)
                .unwrap(),
        );

        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert_eq!(h.images.prompts().len(), 4, "only scene 2 was drawn again");
        assert_eq!(images_of(&plan), images_of(&drawn), "nothing replaced yet");
        let new = plan.scenes()[1].pending().unwrap().clone();
        assert_ne!(new.file, old.file);
        assert!(h.files.exists(project.id, &new.file));
        assert!(matches!(
            app.accept_scene_image(project.id, 0),
            Err(SceneError::NothingToReview(_))
        ));

        let plan = app.accept_scene_image(project.id, 1).unwrap();
        assert_eq!(plan.scenes()[1].image(), Some(&new));
        assert_eq!(plan.scenes()[1].pending(), None);
        assert!(
            !h.files.exists(project.id, &old.file),
            "the replaced file goes"
        );
        assert_eq!(
            app.scenes(project.id).unwrap().plan.unwrap().scenes()[1].image(),
            Some(&new)
        );
    }

    #[test]
    fn rejecting_a_new_image_keeps_the_current_one() {
        let h = Harness::new();
        let app = h.start();
        let (project, drawn) = h.drawn_project(&app);
        done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        let new = app.scenes(project.id).unwrap().plan.unwrap().scenes()[0]
            .pending()
            .unwrap()
            .clone();

        let plan = app.reject_scene_image(project.id, 0).unwrap();

        assert_eq!(images_of(&plan), images_of(&drawn));
        assert_eq!(plan.scenes()[0].pending(), None);
        assert!(!h.files.exists(project.id, &new.file));
        assert!(
            h.files
                .exists(project.id, &drawn.scenes()[0].image().unwrap().file)
        );
    }

    #[test]
    fn edited_prompts_are_kept_beside_claudes() {
        let h = Harness::new();
        let app = h.start();
        let (project, planned) = h.planned_project(&app);

        let plan = app
            .edit_scene_prompt(project.id, 0, "  A rocket at dawn, 1969  ")
            .unwrap();
        let scene = &plan.scenes()[0];
        assert_eq!(scene.prompt().as_str(), "A rocket at dawn, 1969");
        assert_eq!(scene.generated_prompt(), planned.scenes()[0].prompt());
        assert!(scene.is_edited());
        assert_eq!(
            app.scenes(project.id).unwrap().plan.unwrap().scenes(),
            plan.scenes()
        );

        let error = app.edit_scene_prompt(project.id, 0, "   ").unwrap_err();
        assert_eq!(error.field_error(), Some(SceneFieldError::PromptRequired));
        assert!(matches!(
            app.edit_scene_prompt(project.id, 9, "A rocket"),
            Err(SceneError::SceneNotFound(NoSuchScene(9)))
        ));
    }

    #[test]
    fn scenes_wait_for_a_narration_and_the_keys_they_need() {
        let h = Harness::new();
        let mut app = h.start();
        let bare = project(&app);
        assert!(matches!(
            app.plan_scenes(bare.id, false, BudgetConsent::Ask),
            Err(SceneError::NoNarration)
        ));
        assert!(matches!(
            app.generate_scene_images(bare.id, BudgetConsent::Ask),
            Err(SceneError::NoPlan)
        ));

        let (project, _) = h.planned_project(&app);
        app.remove_provider_key(Provider::Gemini).unwrap();
        let error = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, SceneError::MissingKey(Provider::Gemini)));
        assert_eq!(error.message(), Text::ScenesMissingGeminiKey);
        app.remove_provider_key(Provider::Claude).unwrap();
        assert!(matches!(
            app.plan_scenes(project.id, false, BudgetConsent::Ask),
            Err(SceneError::MissingKey(Provider::Claude))
        ));
    }

    #[test]
    fn one_scene_job_runs_at_a_time_per_project() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        *h.images.delay.lock().unwrap() = Duration::from_millis(100);

        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert!(app.scenes(project.id).unwrap().is_busy());
        assert!(matches!(
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask),
            Err(SceneError::Busy)
        ));
        assert!(matches!(
            app.plan_scenes(project.id, true, BudgetConsent::Ask),
            Err(SceneError::Busy)
        ));
        app.cancel_job(id).unwrap();
        wait_done(&app, id);
    }

    #[test]
    fn planning_again_asks_before_discarding_images() {
        let h = Harness::new();
        let app = h.start();
        let (project, drawn) = h.drawn_project(&app);

        assert!(matches!(
            app.plan_scenes(project.id, false, BudgetConsent::Ask),
            Err(SceneError::WouldDiscardImages(3))
        ));

        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, true, BudgetConsent::Ask)
                .unwrap(),
        );

        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert_ne!(plan.id, drawn.id);
        assert!(images_of(&plan).iter().all(Option::is_none));
        for file in drawn.files() {
            assert!(!h.files.exists(project.id, file), "{file} was not removed");
        }
    }

    #[test]
    fn a_new_narration_makes_the_plan_stale() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);

        done(
            &app,
            app.generate_narration(project.id, BudgetConsent::Ask)
                .unwrap(),
        );

        let view = app.scenes(project.id).unwrap();
        assert!(view.stale);
        assert!(view.plan.is_some());
    }

    #[test]
    fn an_unusable_plan_fails_and_keeps_the_current_one() {
        let h = Harness::new();
        let app = h.start();
        let (project, planned) = h.planned_project(&app);

        h.answer(r#"{"scenes": []}"#.to_owned());
        let job = wait_done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );

        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::UnexpectedAnswer
        );
        assert_eq!(app.scenes(project.id).unwrap().plan, Some(planned));
    }
}
