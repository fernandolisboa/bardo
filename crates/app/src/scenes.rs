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
//! theirs until drawn again, and are marked meanwhile: each image
//! records the portraits it was drawn with (#269, ADR-0035). A scene is never drawn
//! without the portrait of everyone it shows, since it would be paid for
//! and show another face: when a portrait fails or will not read, the
//! scenes with that person are left undrawn and point at the portrait.
//! Drawing someone from the description alone is the user's choice, per
//! person.
//!
//! The plan is a storyboard (#258): it names the recurring places once,
//! each with one fixed description, and says which place each scene
//! happens in. A scene is drawn with its place's description and with
//! other scenes' images as references (the first image of its place, the
//! scene right before it there, or the video's first image for its look),
//! so the scenes of a video read as one sequence. The scenes are drawn in
//! order, which is what gives each one its earlier shots.
//!
//! A plan is timed on one narration (#262). Once the narration is made
//! again the plan is out of date, and nothing more is drawn or animated
//! for it: its pictures would be timed for a narration that is gone. A new
//! narration of the same text takes the plan as it is: fitting moves each
//! scene to where its words are said now and keeps every image and clip.
//! A narration of other text leaves planning again as the only way. While
//! scenes are planned, drawn or animated the narration cannot be made
//! again, and while a narration is being made no scene work starts.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, AspectRatio, CastDraft, CastMember, CostPurpose, GeneratedText, Generation,
    GenerationId, ImageFormat, ImageGenerator, ImageRequest, ImageShape, Job, JobFailure,
    JobFailureKind, JobId, JobKind, Narration, NarrationId, NarrationRepository, NoPendingClip,
    NoPendingImage, NoSuchCastMember, NoSuchPlace, NoSuchScene, OtherShot, OtherText, PlaceDraft,
    PlanDraft, PortraitsUsed, ProductId, ProductRepository, ProfileId, Progress, ProjectFiles,
    Provider, ReferenceImage, RenderedPrompt, RepositoryError, SceneDraft, SceneFieldError,
    SceneImage, SceneKindError, ScenePlan, ScenePlanId, ScenePlanRepository, ScenePlanSize,
    ScenePrompt, SceneSetting, SecretStore, ShotComposer, StyleChoice, TemplateKind, TemplateUsed,
    TemplateVariable, TemplateVersion, TemplateVersionId, TextEffort, TextFormat, TextGenerator,
    TextRequest, VideoProject, VideoProjectId, scene_image_prompt, sentences,
};
use bardo_media::MediaEngine;
use serde::{Deserialize, Serialize};

use crate::ad_scenes::{AdPlan, AdScenes};
use crate::clips::{ClipsView, SceneClipView};
use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobActionError, JobContext, JobHandler, Kept};
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
    /// A narration of the project is being made: the scenes wait for it,
    /// since what is planned, drawn or animated now would be out of date
    /// as soon as it lands.
    #[error("a narration is being made for this project")]
    NarrationBusy,
    /// The narration was made again since the scenes were planned (or, on
    /// a version, its scenes are not placed yet): images and clips made
    /// now would be timed for a narration that is gone. Fit the scenes to
    /// the narration, or plan them again, first.
    #[error("the scenes are not on the project's narration")]
    PlanOutOfDate,
    /// The narration reads other text than the scenes were planned on, so
    /// they cannot be fitted to it; planning again is the way.
    #[error(transparent)]
    OtherText(#[from] OtherText),
    /// The narration has more scenes than one plan is asked for.
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
    /// The scene cannot be of that kind (#278): the project is no ad, the
    /// product's images do not allow it, or only the last scene is the
    /// end card.
    #[error(transparent)]
    Kind(#[from] SceneKindError),
    /// The drawn backdrop named is not one of the project's, or was not
    /// kept (#285).
    #[error("no such backdrop")]
    NoSuchBackdrop,
    /// A scene still shows the backdrop: it cannot be discarded.
    #[error("a scene shows the backdrop")]
    BackdropInUse,
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
            SceneError::NarrationBusy => Text::ScenesNarrationBusy,
            SceneError::PlanOutOfDate => Text::ScenesPlanOutOfDate,
            SceneError::OtherText(_) => Text::ScenesOtherText,
            SceneError::WouldDiscardImages(_) => Text::ScenesWouldDiscardImages,
            SceneError::NothingToGenerate => Text::ScenesNothingToGenerate,
            SceneError::OverBudget(_) => Text::BudgetReachedTitle,
            SceneError::NothingToReview(_) | SceneError::NoClipToReview(_) => {
                Text::SceneNothingToReview
            }
            SceneError::NoImageToAnimate => Text::SceneNoImageToAnimate,
            SceneError::NothingToAnimate => Text::ScenesNothingToAnimate,
            SceneError::ClipModelNotOffered => Text::SceneClipModelNotOffered,
            SceneError::Kind(SceneKindError::NoSuchScene(_)) => Text::SceneNotFound,
            SceneError::Kind(_) => Text::SceneKindNotAllowed,
            SceneError::NoSuchBackdrop => Text::ShotBackdropGone,
            SceneError::BackdropInUse => Text::ShotBackdropInUse,
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
    /// Whether the narration changed since the plan was made. Nothing is
    /// drawn or animated for the scenes until they are on the narration
    /// again.
    pub stale: bool,
    /// Whether the out-of-date scenes can be fitted to the narration: it
    /// reads the text they were planned on, so they keep their images and
    /// clips and only their times change. Never for a version's scenes
    /// that wait to be placed, which is their own way onto a narration.
    pub can_fit: bool,
    /// The project's latest scene plan or image job.
    pub job: Option<Job>,
    /// A scene job that was cancelled has not let go of its work yet (a
    /// request it gave up is being closed): nothing new starts until it
    /// has, which takes a moment.
    pub stopping: bool,
    /// The template version the next plan uses.
    pub template: TemplateVersion,
    /// What planning the scenes would cost; `None` without a narration or
    /// with one too long to plan.
    pub plan_estimate: Option<SpendEstimate>,
    /// The narration is too long to plan its scenes: **Plan scenes** would
    /// refuse it.
    pub too_long: bool,
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
    /// The style the video's next images are drawn in (#259): the
    /// project's own, else its channel's.
    pub style: StyleChoice,
    /// The scenes whose image was drawn in another style. They stay until
    /// the user draws them again.
    pub restyled: Vec<usize>,
    /// What drawing those again would cost, their people's portraits
    /// included; `None` when there is none.
    pub restyle_estimate: Option<SpendEstimate>,
    /// The scenes whose image shows a person as an older portrait of
    /// theirs did, or was drawn without their portrait (#269). They stay
    /// until the user draws them again.
    pub old_portraits: Vec<usize>,
    /// What drawing those again would cost, missing portraits included;
    /// `None` when there is none.
    pub old_portraits_estimate: Option<SpendEstimate>,
    /// For an ad (#278): the kinds its scenes may be and the product
    /// images a composed scene may show. Nothing for any other project.
    pub kinds: crate::SceneKinds,
    /// The composed scenes that wait for their picture in the video's
    /// frame: Bardo makes those for free.
    pub missing_shots: Vec<usize>,
}

/// Claude's call that plans the scenes of a narration of `size`. What the
/// plan writes follows the narration's length, so it is estimated from
/// that and not from what plans of other videos took.
pub(crate) fn sized_plan_call(size: ScenePlanSize) -> PlannedCall {
    PlannedCall::new(Provider::Claude, CostPurpose::ScenePlan, 1)
        .with_output_tokens(size.output_tokens_usually())
}

/// The size of the plan of `narration`: an ad's has a scene for every
/// sentence (#278).
fn plan_size(narration: &Narration, ad: bool) -> ScenePlanSize {
    if ad {
        ScenePlanSize::of_ad(narration)
    } else {
        ScenePlanSize::of(narration)
    }
}

/// Claude's call that plans the scenes of `narration` from `rendered`,
/// as an ad's when `ad`.
fn plan_call(rendered: &RenderedPrompt, narration: &Narration, ad: bool) -> PlannedCall {
    sized_plan_call(plan_size(narration, ad)).with_prompt(&rendered.instructions, &rendered.prompt)
}

/// Whether a plan of `narration` is too large to ask for: its room passes
/// what Claude can be asked to leave for one answer. A plan cut at the
/// model's limit is paid for and lost.
fn too_long_to_plan(narration: &Narration, ad: bool) -> bool {
    plan_size(narration, ad).answer_tokens_at_most() > bardo_ai::claude::longest_answer()
}

/// Nano Banana's calls that draw `images` images, portraits included.
pub(crate) fn images_call(images: usize) -> PlannedCall {
    PlannedCall::new(Provider::Gemini, CostPurpose::SceneImage, images as u64)
}

impl ScenesView {
    /// How many images "generate images" draws: the scenes without one,
    /// and the portraits of the people they show that have none yet.
    pub fn missing_images(&self) -> usize {
        self.plan.as_ref().map_or(0, |plan| {
            let missing = plan.missing_images();
            missing.len() + plan.missing_portraits(&missing, self.style).len()
        })
    }

    /// Whether a scene plan or image job of the project is running, or
    /// one that was cancelled is still stopping.
    pub fn is_busy(&self) -> bool {
        self.stopping || self.job.as_ref().is_some_and(|job| job.state().is_active())
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
pub(crate) const PLAN_SCHEMA: &str = r#"{
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
    /// What the scene is, in an ad's plan (#278).
    #[serde(default)]
    kind: String,
    /// The product image a product shot or an end card shows, by its
    /// position; negative for none. Read as any JSON value: one that is no
    /// position (a fraction, a huge number, text) shows the first image
    /// rather than losing a paid answer.
    #[serde(default)]
    product_image: Option<serde_json::Value>,
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
    /// For an ad's plan (#278): its product's images as the request
    /// numbered them, and the frame its product shots are composed for.
    /// Absent for every other plan, whose scenes are all drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ad: Option<AdPlan>,
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
    /// The video's visual style when the job was started, by its code
    /// (#259); jobs from before styles took the look from the notes.
    #[serde(default)]
    style: String,
    /// For each of `scenes`, in that order: the images (current and
    /// waiting, by generation) the scene held when the job was started. An
    /// image that is not among them and not this job's was drawn by
    /// another job since, and the scene is left to it. `None` in a job
    /// from before this was kept: nothing is known, and nothing is left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    had: Option<Vec<Vec<String>>>,
    /// Composed scenes whose picture Bardo makes from a product image
    /// (#278): free, and made before anything is drawn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    shots: Vec<usize>,
    /// The product those pictures are composed from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    product: Option<String>,
}

/// What an image job has delivered, saved as the job's external handle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ImagesState {
    /// The scenes this job saved an image for. They are done for good: a
    /// retry never draws them again, whatever the user did with the image
    /// since (rejected it, replaced it with another job's).
    #[serde(default)]
    drawn: Vec<usize>,
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
    /// would take the request past [`MAX_REFERENCE_BYTES`]. A request goes
    /// without another scene's image that was not added; it does not go
    /// without a portrait (#269).
    fn add(
        &mut self,
        files: &dyn ProjectFiles,
        media: &dyn MediaEngine,
        project: VideoProjectId,
        file: &str,
    ) -> bool {
        self.try_add(files, media, project, file).is_ok()
    }

    /// [`References::add`], saying why the image was not added.
    fn try_add(
        &mut self,
        files: &dyn ProjectFiles,
        media: &dyn MediaEngine,
        project: VideoProjectId,
        file: &str,
    ) -> Result<(), NotAdded> {
        let format = saved_format(file).ok_or(NotAdded::Unreadable)?;
        let small = small_copy(files, media, project, file).map(|bytes| (bytes, ImageFormat::Jpeg));
        let (bytes, format) = small
            .or_else(|| Some((files.read(project, file).ok()?, format)))
            .ok_or(NotAdded::Unreadable)?;
        self.room = self.room.checked_sub(bytes.len()).ok_or(NotAdded::NoRoom)?;
        self.images.push(ReferenceImage { bytes, format });
        Ok(())
    }
}

/// Why an image is not among a request's references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotAdded {
    /// Not an image Bardo saved, or its file is gone or will not read.
    Unreadable,
    /// It would take the request past [`MAX_REFERENCE_BYTES`].
    NoRoom,
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

/// Whether the scene has an image, current or waiting, drawn by `job`.
fn drawn_by(scene: &bardo_domain::Scene, job: JobId) -> bool {
    scene
        .image()
        .into_iter()
        .chain(scene.pending())
        .any(|image| image.generation.job == Some(job))
}

/// Whether the scene has an image, current or waiting, that another job
/// drew after `job` was started, when the scene held the images `had`.
fn drawn_since(scene: &bardo_domain::Scene, job: JobId, had: &[String]) -> bool {
    scene
        .image()
        .into_iter()
        .chain(scene.pending())
        .any(|image| {
            image.generation.job != Some(job) && !had.contains(&image.generation.id.to_string())
        })
}

/// Whether `job` plans or draws the scenes of `project`.
fn is_scene_job_of(job: &Job, project: &str) -> bool {
    matches!(job.kind(), JobKind::ScenePlan | JobKind::SceneImages)
        && serde_json::from_str::<ProjectOf>(job.payload()).is_ok_and(|p| p.project == project)
}

/// Why a job stops paying for scenes the narration left behind.
pub(crate) fn out_of_date(what: &str) -> JobFailure {
    JobFailure::new(
        JobFailureKind::OutOfDate,
        format!(
            "the narration was made again since the scenes were planned, so no more {what} \
             were made for them; fit the scenes to the narration or plan them again"
        ),
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
    /// The images of an ad's product, which its product shots show.
    pub(crate) products: Arc<dyn ProductRepository>,
    /// Composes product shots on the computer (#278).
    pub(crate) composer: Arc<dyn ShotComposer>,
}

impl JobHandler for SceneHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        match cx.kind() {
            JobKind::ScenePlan => self.plan(payload, cx),
            JobKind::ShotBackdrop => self.draw_backdrop(payload, cx.id()),
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

    /// Plans the scenes the job asked for and makes them the project's
    /// plan, from the answer an earlier run kept when there is one
    /// ([`Kept`]).
    fn plan(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let job = cx.id();
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

        // Asked before the job was queued, and again here: a job queued
        // before the question existed, or tried again from the queue, did
        // not pass it.
        if too_long_to_plan(&narration, payload.ad.is_some()) {
            return Err(JobFailure::unexpected(
                "the narration is too long to plan its scenes in one answer",
            ));
        }
        let generated = match Kept::answer(cx) {
            // Paid for and recorded when it arrived.
            Some(answer) => answer,
            None => match self.ask_for_plan(&payload, &narration, project, cx)? {
                Some(generated) => generated,
                // Stopped: nothing to plan from now.
                None => return Ok(()),
            },
        };
        let saved = self.save_plan(payload, job, &narration, current, generated);
        // The answer is kept until it is the project's plan, and no
        // longer. One that turns out not to be a plan is let go as well,
        // so that a retry asks Claude again instead of failing on it for
        // good. Any other failure (storage) leaves it for the next run.
        let done_with_it = match &saved {
            Ok(()) => true,
            Err(failure) => failure.kind == JobFailureKind::UnexpectedAnswer,
        };
        if done_with_it {
            Kept::settle(cx);
        }
        saved
    }

    /// Makes Claude's answer the project's plan, in place of `current`.
    fn save_plan(
        &self,
        payload: PlanPayload,
        job: JobId,
        narration: &Narration,
        current: Option<ScenePlan>,
        generated: GeneratedText,
    ) -> Result<(), JobFailure> {
        let project: VideoProjectId = id(&payload.project)?;
        let ad = payload.ad.clone();
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
                    kind: scene.kind,
                    product_image: scene
                        .product_image
                        .and_then(|image| image.as_u64())
                        .and_then(|image| usize::try_from(image).ok()),
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
            // Only an ad's scenes have kinds, read against the images the
            // request listed.
            product: ad.as_ref().map(AdPlan::pictures),
        };
        let plan = ScenePlan::from_draft(generation, narration, draft, now)
            .map_err(|error| not_usable(error.to_string()))?;
        self.plans.save_scene_plan(&plan).map_err(unexpected)?;
        // The replaced plan's images go with it. A file that will not go
        // (open in a viewer) only costs disk space.
        for file in current.iter().flat_map(ScenePlan::files) {
            crate::proxies::remove_media(self.files.as_ref(), project, file);
        }
        // An ad's product shots are composed at once: they are free. One
        // that cannot be made is marked on its scene and does not undo
        // the plan, which is paid for.
        if let Some(ad) = ad
            && let (Ok(product), Ok(aspect)) = (
                id::<ProductId>(&ad.product),
                ad.aspect.parse::<AspectRatio>(),
            )
            && let Err(failure) = self.compose_missing(project, plan.id, aspect, product)
        {
            tracing::warn!(
                "could not compose the plan's product shots: {}",
                failure.detail
            );
        }
        Ok(())
    }

    /// Sends the plan's request to Claude and waits for the answer, which
    /// is given up as soon as the job is cancelled. `None` when the job
    /// was stopped and has no answer to plan from now: nothing was sent,
    /// or the answer arrived as the job was cancelled and is kept for the
    /// user to ask for again (`Kept::Answered`).
    fn ask_for_plan(
        &self,
        payload: &PlanPayload,
        narration: &Narration,
        project: VideoProjectId,
        cx: &mut JobContext,
    ) -> Result<Option<GeneratedText>, JobFailure> {
        let job = cx.id();
        let size = plan_size(narration, payload.ad.is_some());
        let request = TextRequest {
            effort: TextEffort::Careful,
            answer_tokens: Some(size.answer_tokens_at_most()),
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            images: Vec::new(),
            format: TextFormat::Json {
                schema: crate::ad_scenes::plan_schema(PLAN_SCHEMA, payload.ad.as_ref()),
            },
        };
        // One that was lost is recorded as a plan of this length, as it was
        // estimated: what other plans took says little.
        let paid_call = sized_plan_call(size).with_prompt(&request.instructions, &request.prompt);
        let key = self.key(Provider::Claude)?;
        // A job told to stop sends nothing new.
        if cx.should_stop() {
            return Ok(None);
        }
        let generated = {
            let _paying = self.costs.paying_for_project(job, &paid_call, project)?;
            self.text.generate_until(&key, &request, &cx.stop())
        };
        let generated = generated.map_err(|failure| {
            // An answer that was lost, cut or given up may still have been
            // billed.
            self.costs
                .record_failed_for_project(&failure, &paid_call, Some(job), project);
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
        // Paid for: kept until it is the project's plan, whatever stops
        // this run before that.
        Kept::keep(cx, &generated);
        // The user cancelled as the answer arrived whole: the job was
        // cancelled, so it does not replace the project's plan behind
        // their back. Asked for again, by retrying the job or planning
        // from the stage, the plan is made from this answer without asking
        // Claude again.
        Ok((!cx.cancelled()).then_some(generated))
    }

    /// The saved plan, if it is still `plan`.
    pub(crate) fn saved_plan(
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

    /// Fails when `plan` is no longer on the project's narration: a job
    /// run again after the narration was made again draws nothing for it.
    fn on_narration(&self, plan: &ScenePlan) -> Result<(), JobFailure> {
        let narration = self
            .narrations
            .narration(plan.project)
            .map_err(unexpected)?;
        match plan.is_stale(narration.as_ref()) {
            true => Err(out_of_date("images")),
            false => Ok(()),
        }
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
        let style = StyleChoice::from_code(&payload.style);
        // Portraits drawn on the way to a scene are counted as they come.
        let mut total = payload.portraits.len() + payload.shots.len() + payload.scenes.len();
        let mut failed: Vec<JobFailure> = Vec::new();
        // The scenes left undrawn for lack of a portrait, by scene number,
        // under the people whose portrait is missing.
        let mut waiting: Vec<(String, Vec<usize>)> = Vec::new();
        let mut key = None;
        // Portraits this run drew or tried: one that fails is not tried
        // again for every scene its member is in.
        let mut tried: Vec<usize> = Vec::new();
        let mut done = 0;
        let mut state: ImagesState = match cx.as_ref().and_then(|cx| cx.external_handle()) {
            Some(saved) => parse(saved)?,
            None => ImagesState::default(),
        };
        // Notes a scene as delivered by this job.
        let deliver = |cx: &mut Option<&mut JobContext>, state: &mut ImagesState, index: usize| {
            if state.drawn.contains(&index) {
                return Ok(());
            }
            state.drawn.push(index);
            match cx.as_mut() {
                Some(cx) => cx.save_external_handle(to_json(state)).map_err(unexpected),
                None => Ok(()),
            }
        };
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
            // The user chose since to draw this person without a portrait
            // (a retry of an older job): one drawn now would never be sent.
            let drawn = cast.is_described_only()
                || cast
                    .portrait()
                    .is_some_and(|portrait| portrait.generation.job == Some(job));
            if !drawn {
                if cx.as_ref().is_some_and(|cx| cx.should_stop()) {
                    return Ok(());
                }
                self.on_narration(&plan)?;
                let key = match &key {
                    Some(key) => key,
                    None => key.insert(self.key(Provider::Gemini)?),
                };
                self.draw_portrait(key, project, plan_id, (member, style), job, &mut failed)?;
            }
            step(&mut cx, &mut done, total)?;
        }
        // Product shots first: they are free and need no key (#278).
        let product = payload
            .product
            .as_deref()
            .map(id::<ProductId>)
            .transpose()?;
        for &index in &payload.shots {
            if cx.as_ref().is_some_and(|cx| cx.should_stop()) {
                return Ok(());
            }
            if let Some(product) = product {
                match self.compose_shot(project, plan_id, index, aspect, product) {
                    Ok(()) => {}
                    Err(failure) if failure.kind == JobFailureKind::Files => failed.push(failure),
                    Err(failure) => return Err(failure),
                }
            }
            step(&mut cx, &mut done, total)?;
        }
        for (position, &index) in payload.scenes.iter().enumerate() {
            // A new plan replaced this one: its scenes are gone.
            let Some(plan) = self.saved_plan(project, plan_id)? else {
                return Ok(());
            };
            let Ok(scene) = plan.scene(index) else {
                continue;
            };
            if state.drawn.contains(&index) {
                continue;
            }
            // The scene became a composed one since the job was started:
            // a drawing would be paid for and never shown.
            if scene.is_composed() {
                step(&mut cx, &mut done, total)?;
                continue;
            }
            // Saved by a run cut before it noted so, or by a job from
            // before deliveries were noted.
            if drawn_by(scene, job) {
                deliver(&mut cx, &mut state, index)?;
                continue;
            }
            // Another job drew the scene since this one was started (the
            // user did not wait for a retry): drawing it now would pay for
            // an image nobody is missing.
            let left = payload
                .had
                .as_ref()
                .and_then(|had| had.get(position))
                .is_some_and(|had| drawn_since(scene, job, had));
            if left {
                step(&mut cx, &mut done, total)?;
                continue;
            }
            if cx.as_ref().is_some_and(|cx| cx.should_stop()) {
                return Ok(());
            }
            self.on_narration(&plan)?;
            let key = match &key {
                Some(key) => key,
                None => key.insert(self.key(Provider::Gemini)?),
            };
            // The people it shows get their portrait first, in the video's
            // style. Once one of them is left without (it failed now, or
            // earlier in this run), the scene will not be drawn, so the
            // portraits of the others are not bought for it.
            let missing = plan.missing_portraits(&[index], style);
            if !missing.iter().any(|member| tried.contains(member)) {
                for member in missing {
                    tried.push(member);
                    total += 1;
                    self.draw_portrait(key, project, plan_id, (member, style), job, &mut failed)?;
                    let drawn = self.saved_plan(project, plan_id)?.is_some_and(|plan| {
                        plan.cast_member(member)
                            .is_ok_and(|cast| cast.portrait_in(style).is_some())
                    });
                    if !drawn {
                        break;
                    }
                }
            }
            let Some(plan) = self.saved_plan(project, plan_id)? else {
                return Ok(());
            };
            let Ok(scene) = plan.scene(index) else {
                continue;
            };
            let members: Vec<(usize, &CastMember)> = plan
                .scene_cast(index)
                .into_iter()
                .filter_map(|member| Some((member, plan.cast_member(member).ok()?)))
                .collect();
            let shown: Vec<&CastMember> = members.iter().map(|(_, cast)| *cast).collect();
            let mut references = References::default();
            // Each person goes with their portrait, or from the description
            // alone when the user chose so. Without the portrait (it failed,
            // will not read, or is in another style and could not be drawn
            // again) the scene is not drawn: it would be paid for and show
            // another face (#269).
            let mut used: PortraitsUsed = Vec::new();
            let mut without: Vec<&str> = Vec::new();
            // The people whose portrait is there and will not read.
            let mut unreadable: Vec<usize> = Vec::new();
            for (member, cast) in &members {
                if cast.is_described_only() {
                    used.push((*member, None));
                    continue;
                }
                let added = cast.portrait_in(style).map(|portrait| {
                    references
                        .try_add(
                            self.files.as_ref(),
                            self.media.as_ref(),
                            project,
                            &portrait.file,
                        )
                        .map(|()| portrait.generation.id)
                });
                match added {
                    Some(Ok(portrait)) => used.push((*member, Some(portrait))),
                    Some(Err(NotAdded::Unreadable)) => {
                        unreadable.push(*member);
                        without.push(&cast.name);
                    }
                    Some(Err(NotAdded::NoRoom)) | None => without.push(&cast.name),
                }
            }
            if !without.is_empty() {
                let people = without.join(" and ");
                match waiting.iter_mut().find(|(names, _)| *names == people) {
                    Some((_, scenes)) => scenes.push(index + 1),
                    None => waiting.push((people, vec![index + 1])),
                }
                if let Some(mut plan) = self.saved_plan(project, plan_id)? {
                    plan.scene_mut(index, SystemTime::now())
                        .map_err(unexpected)?
                        .fail(JobFailureKind::PortraitMissing);
                    self.plans.save_scene(&plan, index).map_err(unexpected)?;
                    // A portrait that will not read is let go, so the person
                    // shows as having none and the next run draws it again
                    // instead of failing the same way. Not this run: what it
                    // was started for did not count that portrait.
                    for member in unreadable {
                        if !tried.contains(&member) {
                            tried.push(member);
                        }
                        let now = SystemTime::now();
                        let gone = plan
                            .cast_mut(member, now)
                            .ok()
                            .filter(|cast| cast.portrait_in(style).is_some())
                            .and_then(|cast| cast.drop_portrait(JobFailureKind::Files));
                        if let Some(gone) = gone {
                            self.plans
                                .save_cast_member(&plan, member)
                                .map_err(unexpected)?;
                            crate::proxies::remove_media(self.files.as_ref(), project, &gone.file);
                        }
                    }
                }
                step(&mut cx, &mut done, total)?;
                continue;
            }
            let referenced: Vec<bool> = used.iter().map(|(_, used)| used.is_some()).collect();
            // Then the other scenes it belongs with; one whose image will
            // not read is left out.
            let shots: Vec<OtherShot> = plan
                .reference_shots(index, style)
                .into_iter()
                .filter(|(other, _)| {
                    plan.shot_in(*other, style).is_some_and(|image| {
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
                style,
            };
            let references = references.images;
            let prompt = scene_image_prompt(scene.prompt().as_str(), &shown, &referenced, setting);
            let request = ImageRequest {
                prompt: prompt.clone(),
                shape: aspect.into(),
                references,
            };
            let generated = {
                let _paying = self
                    .costs
                    .paying_for_project(job, &images_call(1), project)?;
                self.images.generate(key, &request)
            };
            match generated {
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
                        .add_image_drawn_with(image, Some(used));
                    self.plans.save_scene(&plan, index).map_err(unexpected)?;
                    if let Some(pushed) = pushed {
                        crate::proxies::remove_media(self.files.as_ref(), project, &pushed.file);
                    }
                    deliver(&mut cx, &mut state, index)?;
                }
                Err(failure) => {
                    // An answer that was lost may still have been billed.
                    self.costs.record_failed_for_project(
                        &failure,
                        &images_call(1),
                        Some(job),
                        project,
                    );
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
        if failed.is_empty() && waiting.is_empty() {
            return Ok(());
        }
        let undrawn = failed.len()
            + waiting
                .iter()
                .map(|(_, scenes)| scenes.len())
                .sum::<usize>();
        let details = failed
            .iter()
            .map(|failure| failure.detail.clone())
            .chain(waiting.iter().map(|(people, scenes)| {
                let numbers: Vec<String> = scenes.iter().map(ToString::to_string).collect();
                format!(
                    "{} {} not drawn, for lack of the portrait of {people}",
                    if scenes.len() == 1 { "scene" } else { "scenes" },
                    numbers.join(", ") + if scenes.len() == 1 { " was" } else { " were" },
                )
            }))
            .collect::<Vec<_>>()
            .join(" | ");
        Err(JobFailure::new(
            failed
                .first()
                .map_or(JobFailureKind::PortraitMissing, |first| first.kind),
            format!("{undrawn} of {total} images were not drawn. {details}"),
        ))
    }

    /// Draws cast member `member`'s portrait in `style` and puts it in
    /// place of the current one. A failure about the portrait is recorded
    /// on the member and in `failed`, and the scenes that show the member
    /// are then left undrawn; any other failure stops the job.
    fn draw_portrait(
        &self,
        key: &ApiKey,
        project: VideoProjectId,
        plan_id: ScenePlanId,
        (member, style): (usize, StyleChoice),
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
        let prompt = cast.portrait_prompt(style);
        let request = ImageRequest {
            prompt: prompt.clone(),
            shape: ImageShape::Portrait,
            references: Vec::new(),
        };
        let generated = {
            let _paying = self
                .costs
                .paying_for_project(job, &images_call(1), project)?;
            self.images.generate(key, &request)
        };
        match generated {
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
                // The scenes left undrawn for lack of it no longer wait.
                for index in plan.settle_portrait_waits(style) {
                    self.plans.save_scene(&plan, index).map_err(unexpected)?;
                }
                if let Some(replaced) = replaced {
                    crate::proxies::remove_media(self.files.as_ref(), project, &replaced.file);
                }
                Ok(())
            }
            Err(failure) => {
                // An answer that was lost may still have been billed.
                self.costs
                    .record_failed_for_project(&failure, &images_call(1), Some(job), project);
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

    /// Whether a scene plan or image job of the project is queued or
    /// running (the latest, or an older one queued again from Jobs), or
    /// one that was cancelled is still stopping (`scenes_stopping`).
    pub(crate) fn scenes_busy(&self, project: VideoProjectId) -> bool {
        let project = project.to_string();
        self.jobs().iter().any(|job| {
            (job.state().is_active() || self.jobs.runs_here(job.id()))
                && is_scene_job_of(job, &project)
        })
    }

    /// A plan already paid for that `payload` asks for again: the
    /// project's latest scene job, with the same request and the answer
    /// kept (`Kept::Answered`). It was cancelled as its answer
    /// arrived, or failed because the plan could not be saved.
    fn kept_plan(&self, project: VideoProjectId, payload: &str) -> Option<JobId> {
        self.latest_scene_job(project)
            .filter(|job| {
                job.kind() == JobKind::ScenePlan && job.can_retry() && job.payload() == payload
            })
            .filter(Kept::held_by)
            .map(|job| job.id())
    }

    /// Whether a scene job of the project that no longer reads as running
    /// (it was cancelled) still has its handler at work here. A cancelled
    /// job reads cancelled at once, while its handler may be in a paid
    /// request it has yet to give up, or saving what that request brought.
    /// Until it returns, a new job would pay for the same work beside it
    /// and the two would save over each other.
    fn scenes_stopping(&self, project: VideoProjectId) -> bool {
        let project = project.to_string();
        self.jobs().iter().any(|job| {
            !job.state().is_active()
                && self.jobs.runs_here(job.id())
                && is_scene_job_of(job, &project)
        })
    }

    /// The project's scene plan or image job at work, else its latest:
    /// an older job the user retried counts while it runs.
    fn latest_scene_job(&self, project: VideoProjectId) -> Option<Job> {
        let project = project.to_string();
        let jobs = self.jobs();
        let of_project = || {
            jobs.iter()
                .rev()
                .filter(|job| is_scene_job_of(job, &project))
        };
        of_project()
            .find(|job| job.state().is_active())
            .or_else(|| of_project().next())
            .cloned()
    }

    pub(crate) fn scenes_idle(&self, project: VideoProjectId) -> Result<(), SceneError> {
        if self.scenes_busy(project) {
            return Err(SceneError::Busy);
        }
        Ok(())
    }

    /// Paid work on `plan` needs it to be on the project's narration, and
    /// no other narration on its way: an image or clip made for scenes a
    /// narration left behind is timed for words that are gone.
    pub(crate) fn scenes_current(&self, plan: &ScenePlan) -> Result<(), SceneError> {
        if self.narration_busy(plan.project) {
            return Err(SceneError::NarrationBusy);
        }
        let narration = self
            .narrations
            .narration(plan.project)?
            .ok_or(SceneError::NoNarration)?;
        if plan.is_stale(Some(&narration)) {
            return Err(SceneError::PlanOutOfDate);
        }
        Ok(())
    }

    /// Why `job` cannot be run again from the Jobs panel for now. The
    /// narration and the scenes timed on it are not made at the same time
    /// there either: a scene job waits for a narration being made, and a
    /// narration job for scenes being planned, drawn or animated. And what
    /// its stage asks about first is not skipped: a narration, or a new
    /// plan, over scenes that hold paid images or clips starts from its
    /// stage, where the user sees what is at stake.
    pub(crate) fn retry_held(&self, job: JobId) -> Option<JobActionError> {
        let job = self
            .jobs()
            .into_iter()
            .find(|candidate| candidate.id() == job)?;
        let project = serde_json::from_str::<ProjectOf>(job.payload())
            .ok()
            .and_then(|payload| payload.project.parse::<uuid::Uuid>().ok())
            .map(VideoProjectId::from)?;
        let scenes_busy = || self.scenes_busy(project) || self.clips_busy(project);
        match job.kind() {
            JobKind::ScenePlan | JobKind::SceneImages | JobKind::SceneClips
                if self.narration_busy(project) =>
            {
                Some(JobActionError::NarrationBusy)
            }
            // Planning replaces the plan other jobs are working on.
            JobKind::ScenePlan if scenes_busy() => Some(JobActionError::ScenesBusy),
            JobKind::ScenePlan
                if self
                    .scene_plans
                    .scene_plan(project)
                    .ok()
                    .flatten()
                    .is_some_and(|plan| !plan.paid_pictures().is_empty()) =>
            {
                Some(JobActionError::AsksFirst)
            }
            // A plan job that is no longer the project's latest scene job
            // would replace, unasked, the plan made since (one with paid
            // pictures asks first, above): the Scenes stage plans again.
            JobKind::ScenePlan => self
                .latest_scene_job(project)
                .is_some_and(|latest| latest.id() != job.id())
                .then_some(JobActionError::ScenesMovedOn),
            JobKind::SceneImages if self.scenes_busy(project) => Some(JobActionError::ScenesBusy),
            JobKind::Narration | JobKind::NarrationImport | JobKind::Capture if scenes_busy() => {
                Some(JobActionError::ScenesBusy)
            }
            JobKind::Narration | JobKind::NarrationImport | JobKind::Capture => self
                .pictures_at_stake(project, "")
                .ok()
                .flatten()
                .map(|_| JobActionError::AsksFirst),
            _ => None,
        }
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
        let template = self.scene_plan_template(&project)?;
        let too_long = narration
            .as_ref()
            .is_some_and(|narration| too_long_to_plan(narration, project.is_ad()));
        let plan_estimate = match &narration {
            Some(narration) if !too_long => {
                let (rendered, ad) = self.render_plan(&project, narration, &template)?;
                Some(self.estimate(&[plan_call(&rendered, narration, ad.is_some())])?)
            }
            _ => None,
        };
        let aspect = self.video_format(&project)?.aspect();
        let missing_shots = plan
            .as_ref()
            .map_or_else(Vec::new, |plan| plan.missing_shots(aspect));
        let kinds = self.scene_kinds(project.id)?;
        let style = self.video_style(&project)?;
        let missing = plan.as_ref().map_or(0, |plan| {
            let missing = plan.missing_images();
            missing.len() + plan.missing_portraits(&missing, style).len()
        });
        let restyled = plan
            .as_ref()
            .map_or_else(Vec::new, |plan| plan.in_another_style(style));
        let restyle_estimate = match (&plan, restyled.len()) {
            (Some(plan), scenes @ 1..) => {
                let portraits = plan.missing_portraits(&restyled, style).len();
                Some(self.estimate(&[images_call(scenes + portraits)])?)
            }
            _ => None,
        };
        let old_portraits = plan
            .as_ref()
            .map_or_else(Vec::new, ScenePlan::with_old_portraits);
        let old_portraits_estimate = match (&plan, old_portraits.len()) {
            (Some(plan), scenes @ 1..) => {
                let portraits = plan.missing_portraits(&old_portraits, style).len();
                Some(self.estimate(&[images_call(scenes + portraits)])?)
            }
            _ => None,
        };
        let clips = self.clips_view(&project, plan.as_ref())?;
        let version = self.version_scenes(project.id, plan.as_ref(), narration.as_ref())?;
        let stale = plan
            .as_ref()
            .is_some_and(|plan| plan.is_stale(narration.as_ref()));
        let can_fit = stale
            && !version.as_ref().is_some_and(|version| version.can_place)
            && plan
                .as_ref()
                .zip(narration.as_ref())
                .is_some_and(|(plan, narration)| plan.reads(narration.text.as_str()));
        Ok(ScenesView {
            clips,
            version,
            narration: narration.as_ref().map(|narration| narration.id),
            stale,
            can_fit,
            images_estimate: match missing {
                0 => None,
                n => Some(self.estimate(&[images_call(n)])?),
            },
            image_estimates: match &plan {
                Some(plan) => (0..plan.scenes().len())
                    .map(|index| {
                        let portraits = plan.missing_portraits(&[index], style).len();
                        self.estimate(&[images_call(1 + portraits)])
                    })
                    .collect::<Result<_, _>>()?,
                None => Vec::new(),
            },
            plan,
            stopping: self.scenes_stopping(project.id),
            job: self.latest_scene_job(project.id),
            template,
            plan_estimate,
            too_long,
            project,
            style,
            restyled,
            restyle_estimate,
            old_portraits,
            old_portraits_estimate,
            kinds,
            missing_shots,
        })
    }

    /// The template a project's scenes are planned from: the ad scene
    /// plan for an ad (#278), the storyboard plan for every other project.
    fn scene_plan_template(&self, project: &VideoProject) -> Result<TemplateVersion, SceneError> {
        Ok(self.current_template(if project.is_ad() {
            TemplateKind::AdScenePlan
        } else {
            TemplateKind::ImagePrompt
        })?)
    }

    /// The project's scene plan template filled with its facts and its
    /// narration's sentences, as the request sends it, and for an ad what
    /// the request tells of its product. An ad's request carries the
    /// kinds its scenes may be, its product's images and the rules of
    /// every ad whatever the template says; what the user typed or a page
    /// said goes in as data.
    fn render_plan(
        &self,
        project: &VideoProject,
        narration: &Narration,
        template: &TemplateVersion,
    ) -> Result<(RenderedPrompt, Option<AdScenes>), SceneError> {
        let mut values = self.script_values(project).map_err(|error| match error {
            ScriptError::Repository(error) => SceneError::Repository(error),
            _ => SceneError::ProjectNotFound,
        })?;
        values.insert(
            TemplateVariable::NarrationSentences,
            sentence_lines(narration),
        );
        let ad = match project.is_ad() {
            true => self.ad_scenes(project)?,
            false => None,
        };
        if project.is_ad() {
            for typed in [
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::AestheticNotes,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::NarrationSentences,
            ] {
                if let Some(value) = values.get_mut(&typed) {
                    *value = crate::ads::as_data(value);
                }
            }
            values.extend(match &ad {
                Some(ad) => ad.values(),
                None => crate::ad_scenes::unset_values(),
            });
        }
        let rendered = template.body.render(&values).map_err(|missing| {
            // Every variable of the template has a value above.
            SceneError::Repository(RepositoryError(Box::new(missing)))
        })?;
        Ok(match ad {
            Some(ad) => (ad.request(&template.body, rendered), Some(ad)),
            None => (rendered, None),
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
        if too_long_to_plan(&narration, project.is_ad()) {
            return Err(SceneError::NarrationTooLong);
        }
        self.scenes_idle(project.id)?;
        if self.clips_busy(project.id) {
            return Err(SceneError::Busy);
        }
        // A narration on its way would leave the new plan behind at once.
        if self.narration_busy(project.id) {
            return Err(SceneError::NarrationBusy);
        }
        let images = self
            .scene_plans
            .scene_plan(project.id)?
            .map_or(0, |plan| plan.files().count());
        if images > 0 && !discard_images {
            return Err(SceneError::WouldDiscardImages(images));
        }
        self.require_key(Provider::Claude)?;
        let template = self.scene_plan_template(&project)?;
        let (rendered, ad) = self.render_plan(&project, &narration, &template)?;
        let call = plan_call(&rendered, &narration, ad.is_some());
        let ad = match ad {
            Some(ad) => Some(AdPlan::of(
                ad.product,
                &ad.images,
                self.video_format(&project)?.aspect(),
            )),
            None => None,
        };
        let payload = to_json(&PlanPayload {
            project: project.id.to_string(),
            narration: narration.id.to_string(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
            ad,
        });
        // The same plan was asked for and answered, and is not the
        // project's plan (its job was cancelled as the answer arrived, or
        // could not save it): that answer is paid for, so its job goes on
        // from it and nothing is asked of Claude, or of the budget.
        if let Some(kept) = self.kept_plan(project.id, &payload)
            && self.jobs.retry(kept).is_ok()
        {
            return Ok(kept);
        }
        self.scenes_budget(project.id, call, consent)?;
        let job = Job::new(self.profile.id, JobKind::ScenePlan, payload);
        Ok(self.jobs.enqueue(job)?)
    }

    /// Fits the project's scenes to its narration, when that is a new
    /// reading of the text they were planned on: every scene keeps its
    /// words, prompts, people, place, images and clips, and starts and ends
    /// where the narration says its words now. Costs nothing. A narration
    /// of other text does not take the plan (`SceneError::OtherText`):
    /// those scenes are planned again. Scenes already on the narration are
    /// returned as they are.
    pub fn fit_scenes(&self, project: VideoProjectId) -> Result<ScenePlan, SceneError> {
        let plan = self.own_plan(project)?;
        let narration = self
            .narrations
            .narration(plan.project)?
            .ok_or(SceneError::NoNarration)?;
        if !plan.is_stale(Some(&narration)) {
            return Ok(plan);
        }
        // A version's carried scenes are placed, which also builds its cut.
        if self
            .localizations
            .localization(plan.project)?
            .is_some_and(|localization| localization.carries(&plan))
        {
            return Err(SceneError::PlanOutOfDate);
        }
        // A job saving a scene meanwhile would be written over.
        self.scenes_idle(plan.project)?;
        if self.clips_busy(plan.project) {
            return Err(SceneError::Busy);
        }
        if self.narration_busy(plan.project) {
            return Err(SceneError::NarrationBusy);
        }
        let fitted = plan.fitted_to(&narration, SystemTime::now())?;
        // A cut saved since the narration changed was laid out on the old
        // times under this same plan and narration: it goes, and the
        // editor builds the rough cut on the new ones.
        self.scene_plans.save_fitted_plan(&fitted)?;
        Ok(fitted)
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

    /// Whether `job` is an image job started when its video had another
    /// visual style (#259). Run again, it would draw in the style it
    /// carries: images the Scenes stage counts as in another style at
    /// once, and portraits drawn over the current ones, with no estimate
    /// shown. The user draws those scenes from the stage instead.
    pub(crate) fn restyled_images(&self, job: JobId) -> bool {
        let jobs = self.jobs();
        let Some(payload) = jobs
            .iter()
            .find(|candidate| candidate.id() == job && candidate.kind() == JobKind::SceneImages)
            .and_then(|job| parse::<ImagesPayload>(job.payload()).ok())
        else {
            return false;
        };
        let Ok(Some(project)) = id::<VideoProjectId>(&payload.project)
            .map_err(|_| ())
            .and_then(|project| self.themes.project(project).map_err(|_| ()))
        else {
            return false;
        };
        self.video_style(&project)
            .is_ok_and(|style| style != StyleChoice::from_code(&payload.style))
    }

    /// The paid calls a retry of the image job `job` would still make:
    /// the portraits and scenes it has not drawn and would not leave to
    /// another job, and the portraits those scenes still need. `None` when
    /// it is no image job or nothing of it is left to pay for.
    pub(crate) fn images_left(&self, job: &Job) -> Option<(VideoProjectId, Vec<PlannedCall>)> {
        if job.kind() != JobKind::SceneImages {
            return None;
        }
        let payload: ImagesPayload = serde_json::from_str(job.payload()).ok()?;
        let project = VideoProjectId::from(payload.project.parse::<uuid::Uuid>().ok()?);
        let plan = self.scene_plans.scene_plan(project).ok()??;
        // A new plan replaced the job's, or the narration left it behind:
        // run again, the job draws nothing.
        if plan.id.to_string() != payload.plan || self.scenes_current(&plan).is_err() {
            return None;
        }
        let state: ImagesState = job
            .external_handle()
            .and_then(|saved| serde_json::from_str(saved).ok())
            .unwrap_or_default();
        let portraits: Vec<usize> = payload
            .portraits
            .iter()
            .copied()
            .filter(|&member| {
                plan.cast_member(member).is_ok_and(|cast| {
                    !cast.is_described_only()
                        && !cast
                            .portrait()
                            .is_some_and(|portrait| portrait.generation.job == Some(job.id()))
                })
            })
            .collect();
        let scenes: Vec<usize> = payload
            .scenes
            .iter()
            .enumerate()
            .filter(|&(position, &index)| {
                let left = |scene: &bardo_domain::Scene| {
                    payload
                        .had
                        .as_ref()
                        .and_then(|had| had.get(position))
                        .is_some_and(|had| drawn_since(scene, job.id(), had))
                };
                !state.drawn.contains(&index)
                    && plan.scene(index).is_ok_and(|scene| {
                        !scene.is_composed() && !drawn_by(scene, job.id()) && !left(scene)
                    })
            })
            .map(|(_, &index)| index)
            .collect();
        let first = plan
            .missing_portraits(&scenes, StyleChoice::from_code(&payload.style))
            .into_iter()
            .filter(|member| !portraits.contains(member))
            .count();
        let images = portraits.len() + first + scenes.len();
        (images > 0).then(|| (project, vec![images_call(images)]))
    }

    /// Whether `job` is an image job of a project where another scene
    /// plan or image job runs or waits now. Run again meanwhile, it would
    /// not yet see that job's images and would pay for a scene a second
    /// time.
    pub(crate) fn images_taken(&self, job: JobId) -> bool {
        let jobs = self.jobs();
        let Some(project) = jobs
            .iter()
            .find(|candidate| candidate.id() == job && candidate.kind() == JobKind::SceneImages)
            .and_then(|job| serde_json::from_str::<ProjectOf>(job.payload()).ok())
        else {
            return false;
        };
        jobs.iter().any(|other| {
            other.id() != job
                && other.state().is_active()
                && matches!(other.kind(), JobKind::ScenePlan | JobKind::SceneImages)
                && serde_json::from_str::<ProjectOf>(other.payload())
                    .is_ok_and(|theirs| theirs.project == project.project)
        })
    }

    /// Starts a job that composes the pictures of the composed scenes
    /// `shots` of an ad's plan (#278). Free: no key, no budget, and the
    /// plan need not be on the narration.
    pub(crate) fn compose_scenes(
        &self,
        plan: &ScenePlan,
        project: &VideoProject,
        shots: Vec<usize>,
    ) -> Result<JobId, SceneError> {
        self.scenes_idle(plan.project)?;
        let payload = ImagesPayload {
            project: plan.project.to_string(),
            plan: plan.id.to_string(),
            had: Some(Vec::new()),
            scenes: Vec::new(),
            aspect: self.video_format(project)?.aspect().code().to_owned(),
            portraits: Vec::new(),
            style: self.video_style(project)?.code().to_owned(),
            shots,
            product: project.product.map(|product| product.to_string()),
        };
        let job = Job::new(self.profile.id, JobKind::SceneImages, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Starts a job that draws `scenes` and `portraits` and composes
    /// `shots`. A composed scene is never drawn: it is left out of
    /// `scenes`. Only drawing needs the key, the budget and the plan on
    /// its narration.
    fn draw_scenes(
        &self,
        plan: &ScenePlan,
        scenes: Vec<usize>,
        portraits: Vec<usize>,
        shots: Vec<usize>,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let project = self
            .themes
            .project(plan.project)?
            .ok_or(SceneError::ProjectNotFound)?;
        let scenes: Vec<usize> = scenes
            .into_iter()
            .filter(|&index| !plan.scene(index).is_ok_and(|scene| scene.is_composed()))
            .collect();
        if scenes.is_empty() && portraits.is_empty() {
            if shots.is_empty() {
                return Err(SceneError::NothingToGenerate);
            }
            return self.compose_scenes(plan, &project, shots);
        }
        self.scenes_idle(plan.project)?;
        self.scenes_current(plan)?;
        self.require_key(Provider::Gemini)?;
        let style = self.video_style(&project)?;
        let first = plan
            .missing_portraits(&scenes, style)
            .into_iter()
            .filter(|member| !portraits.contains(member))
            .count();
        self.scenes_budget(
            plan.project,
            images_call(portraits.len() + first + scenes.len()),
            consent,
        )?;
        let had = scenes
            .iter()
            .map(|&index| {
                plan.scene(index).map(|scene| {
                    scene
                        .image()
                        .into_iter()
                        .chain(scene.pending())
                        .map(|image| image.generation.id.to_string())
                        .collect()
                })
            })
            .collect::<Result<Vec<Vec<String>>, _>>()?;
        let payload = ImagesPayload {
            project: plan.project.to_string(),
            plan: plan.id.to_string(),
            had: Some(had),
            scenes,
            aspect: self.video_format(&project)?.aspect().code().to_owned(),
            portraits,
            style: style.code().to_owned(),
            product: project
                .product
                .filter(|_| !shots.is_empty())
                .map(|product| product.to_string()),
            shots,
        };
        let job = Job::new(self.profile.id, JobKind::SceneImages, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Starts a job that draws every scene without an image, and composes
    /// the picture of every composed scene that waits for one. Past
    /// Gemini's budget it needs `consent`; composing is free.
    pub fn generate_scene_images(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let plan = self.own_plan(project)?;
        let missing = plan.missing_images();
        let aspect = self.video_format(&self.scenes_project(project)?)?.aspect();
        let shots = plan.missing_shots(aspect);
        if missing.is_empty() && shots.is_empty() {
            return Err(SceneError::NothingToGenerate);
        }
        self.draw_scenes(&plan, missing, Vec::new(), shots, consent)
    }

    /// Starts a job that composes every picture an ad's composed scenes
    /// wait for (#278), and draws nothing: free, with no key asked.
    pub fn compose_missing_shots(&self, project: VideoProjectId) -> Result<JobId, SceneError> {
        let plan = self.own_plan(project)?;
        let project = self.scenes_project(project)?;
        let shots = plan.missing_shots(self.video_format(&project)?.aspect());
        if shots.is_empty() {
            return Err(SceneError::NothingToGenerate);
        }
        self.compose_scenes(&plan, &project, shots)
    }

    /// Starts a job that draws again every scene whose image was drawn in
    /// another style than the video's (#259). Each current image stays
    /// until the user accepts the new one. Past Gemini's budget it needs
    /// `consent`.
    pub fn redraw_restyled_scenes(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let plan = self.own_plan(project)?;
        let project = self.scenes_project(project)?;
        let restyled = plan.in_another_style(self.video_style(&project)?);
        if restyled.is_empty() {
            return Err(SceneError::NothingToGenerate);
        }
        self.draw_scenes(&plan, restyled, Vec::new(), Vec::new(), consent)
    }

    /// Starts a job that draws again every scene whose image shows a
    /// person as an older portrait of theirs did, or was drawn without
    /// their portrait (#269). Each current image stays until the user
    /// accepts the new one. Past Gemini's budget it needs `consent`.
    pub fn redraw_scenes_with_old_portraits(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let plan = self.own_plan(project)?;
        let old = plan.with_old_portraits();
        if old.is_empty() {
            return Err(SceneError::NothingToGenerate);
        }
        self.draw_scenes(&plan, old, Vec::new(), Vec::new(), consent)
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
        // A composed scene's picture is made again by composing it, free.
        if plan.scene(index)?.is_composed() {
            return self.draw_scenes(&plan, Vec::new(), Vec::new(), vec![index], consent);
        }
        self.draw_scenes(&plan, vec![index], Vec::new(), Vec::new(), consent)
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
        // A portrait of a person drawn from the description alone would be
        // paid for and never sent.
        if plan.cast_member(member)?.is_described_only() {
            return Err(SceneError::NothingToGenerate);
        }
        self.draw_scenes(&plan, Vec::new(), vec![member], Vec::new(), consent)
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

    /// Chooses to draw cast member `member` from the description alone,
    /// with no portrait (#269), or with a portrait again. Without one the
    /// person's face may change from scene to scene, which is why it is
    /// the user's choice and never what a failed portrait leads to.
    pub fn set_cast_described_only(
        &self,
        project: VideoProjectId,
        member: usize,
        described_only: bool,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        // A job drawing the scenes would draw some each way.
        self.scenes_idle(plan.project)?;
        if plan
            .cast_mut(member, SystemTime::now())?
            .set_described_only(described_only)
        {
            self.scene_plans.save_cast_member(&plan, member)?;
            // The scenes left undrawn for lack of this portrait no longer
            // wait for it.
            let project = self
                .themes
                .project(plan.project)?
                .ok_or(SceneError::ProjectNotFound)?;
            let style = self.video_style(&project)?;
            for index in plan.settle_portrait_waits(style) {
                self.scene_plans.save_scene(&plan, index)?;
            }
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

    /// Where the picture `scene` of `project` shows is on disk: the one
    /// Bardo composed for a composed scene (#278), else its drawn image.
    /// `None` while it has none.
    pub fn scene_picture_path(
        &self,
        project: VideoProjectId,
        scene: &bardo_domain::Scene,
    ) -> Option<PathBuf> {
        scene.picture().map(|file| self.files.path(project, file))
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
    use crate::{JobSettings, NarrationError, Providers, Repositories};

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
        /// Research on the web.
        pub(crate) researcher: Arc<crate::product_sheets::testing::FakeResearcher>,
        pub(crate) images: Arc<FakeImages>,
        /// The video's music.
        pub(crate) music: Arc<crate::testing::FakeMusic>,
        pub(crate) clips: Arc<FakeClips>,
        /// Video providers after the fake one.
        pub(crate) more_clips: Vec<Arc<dyn bardo_domain::ClipGenerator>>,
        /// How long a closing app waits for its handlers
        /// (`JobSettings::closing_wait`); the default when `None`.
        pub(crate) closing_wait: Option<Duration>,
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
                researcher: Arc::default(),
                images: Arc::default(),
                music: Arc::default(),
                clips: Arc::default(),
                more_clips: Vec::new(),
                closing_wait: None,
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
                JobSettings {
                    closing_wait: self
                        .closing_wait
                        .unwrap_or(JobSettings::default().closing_wait),
                    ..Self::job_settings()
                },
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
                researcher: Arc::clone(&self.researcher) as _,
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
                app.generate_narration(project.id, crate::PicturesConsent::Ask, BudgetConsent::Ask)
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
        let short = app.narrations.narration(project.id).unwrap().unwrap();
        assert_eq!(
            request.answer_tokens,
            Some(ScenePlanSize::of(&short).answer_tokens_at_most())
        );

        // Ten minutes with a sentence every four seconds: up to 150 scenes,
        // sixteen times the plan above.
        let narration = narrate_at_length(&app, &project, 150, Duration::from_secs(600));
        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        let request = h.text.requests().pop().unwrap();
        assert_eq!(request.effort, TextEffort::Careful);
        let size = ScenePlanSize::of(&narration);
        assert_eq!(size.scenes_at_most(), 150);
        assert_eq!(request.answer_tokens, Some(size.answer_tokens_at_most()));
    }

    /// The most sentences, one every four seconds, a narration can have and
    /// still be planned.
    fn most_scenes() -> usize {
        let fits = |scenes: usize| {
            ScenePlanSize::new(scenes, Duration::from_secs(4 * scenes as u64))
                .answer_tokens_at_most()
                <= bardo_ai::claude::longest_answer()
        };
        let most = (1..2_000).take_while(|scenes| fits(*scenes)).count();
        assert!((200..300).contains(&most), "{most}");
        most
    }

    #[test]
    fn a_narration_too_long_for_one_plan_is_refused_before_it_costs() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let asked = h.text.requests().len();
        let jobs = app.jobs().len();
        let most = most_scenes();

        // One scene more than a plan is asked for.
        narrate_at_length(
            &app,
            &project,
            most + 1,
            Duration::from_secs(4 * (most as u64 + 1)),
        );
        // Refused before the question about the images a new plan discards.
        let error = app
            .plan_scenes(project.id, false, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, SceneError::NarrationTooLong));
        assert_eq!(error.message(), Text::ScenesNarrationTooLong);
        assert_eq!(h.text.requests().len(), asked, "Claude was not asked");
        assert_eq!(app.jobs().len(), jobs, "and no job waits to ask");
        // The stage says so up front, and prices nothing.
        let view = app.scenes(project.id).unwrap();
        assert!(view.too_long);
        assert_eq!(view.plan_estimate, None);

        // One scene fewer fits.
        let narration =
            narrate_at_length(&app, &project, most, Duration::from_secs(4 * most as u64));
        let view = app.scenes(project.id).unwrap();
        assert!(!view.too_long);
        assert!(view.plan_estimate.is_some());
        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, true, BudgetConsent::Ask)
                .unwrap(),
        );
        let request = h.text.requests().pop().unwrap();
        assert_eq!(
            request.answer_tokens,
            Some(ScenePlanSize::of(&narration).answer_tokens_at_most())
        );
    }

    #[test]
    fn a_queued_plan_of_a_narration_too_long_fails_without_asking_claude() {
        // A job from before the question existed, or one tried again from
        // the queue, reaches the handler without passing `plan_scenes`.
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        let narration = narrate_at_length(&app, &project, 400, Duration::from_secs(30 * 60));
        let asked = h.text.requests().len();
        let payload = PlanPayload {
            project: project.id.to_string(),
            narration: narration.id.to_string(),
            template: app
                .current_template(TemplateKind::ImagePrompt)
                .unwrap()
                .id
                .to_string(),
            template_number: 1,
            instructions: "Plan the scenes.".into(),
            prompt: "Every sentence.".into(),
            ad: None,
        };
        let job = Job::new(app.profile.id, JobKind::ScenePlan, to_json(&payload));
        let job = wait_done(&app, app.jobs.enqueue(job).unwrap());
        assert_eq!(job.state(), JobState::Failed);
        assert!(
            job.failure().unwrap().detail.contains("too long"),
            "{:?}",
            job.failure()
        );
        assert_eq!(h.text.requests().len(), asked, "Claude was not asked");
        assert_eq!(app.scenes(project.id).unwrap().plan, None);
    }

    #[test]
    fn the_estimate_of_a_plan_follows_the_length_of_the_narration() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        let template = app.current_template(TemplateKind::ImagePrompt).unwrap();
        let with_output = |narration: &Narration, tokens: u64| {
            let (rendered, _) = app.render_plan(&project, narration, &template).unwrap();
            let call = PlannedCall::new(Provider::Claude, CostPurpose::ScenePlan, 1)
                .with_prompt(&rendered.instructions, &rendered.prompt)
                .with_output_tokens(tokens);
            app.estimate(&[call]).unwrap()
        };
        let estimate = || app.scenes(project.id).unwrap().plan_estimate.unwrap();

        // The harness narrates for one second: one scene, and the thought
        // a plan takes.
        let short = app.narrations.narration(project.id).unwrap().unwrap();
        let size = ScenePlanSize::of(&short);
        assert_eq!(size.output_tokens_usually(), 760 + 10_000);
        assert_eq!(
            estimate(),
            with_output(&short, size.output_tokens_usually())
        );

        // Ten minutes: a hundred scenes usually, and their thought.
        let long = narrate_at_length(&app, &project, 150, Duration::from_secs(600));
        let size = ScenePlanSize::of(&long);
        assert_eq!(size.output_tokens_usually(), 16_600 + 10_000);
        assert_eq!(estimate(), with_output(&long, size.output_tokens_usually()));
        assert!(estimate().total() > with_output(&long, 10_760).total());

        // What the plan of a long video took says nothing about a short
        // one: after one is paid for, a Short is estimated as before.
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
        let short = narrate_at_length(&app, &project, 4, Duration::from_secs(1));
        assert_eq!(estimate(), with_output(&short, 10_760));
    }

    /// The plan of `plan_answer` with a chef in the first and last scenes.
    pub(crate) fn cast_answer() -> String {
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

    pub(crate) fn cast_project(h: &Harness, app: &Bardo) -> VideoProject {
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
    fn a_declined_portrait_leaves_its_scenes_undrawn_until_it_is_drawn() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        h.images.decline("reference portrait");

        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, id);
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::Declined);
        assert!(
            failure.detail.starts_with("3 of 4 images were not drawn.")
                && failure.detail.contains("portrait of the chef: ")
                && failure
                    .detail
                    .contains("scenes 1, 3 were not drawn, for lack of the portrait of the chef"),
            "{}",
            failure.detail
        );

        // Nothing is paid for a scene that would show another face: only
        // the scene without the chef is drawn.
        let requests = h.images.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2, "one portrait try and scene 2");
        assert!(requests[1].prompt.starts_with("The probe drifting away"));
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert_eq!(plan.cast()[0].failure(), Some(JobFailureKind::Declined));
        assert_eq!(plan.missing_images(), [0, 2]);
        for scene in [0, 2] {
            assert_eq!(
                plan.scenes()[scene].failure(),
                Some(JobFailureKind::PortraitMissing),
                "scene {scene} points at the portrait"
            );
        }
        assert_eq!(plan.scenes()[1].failure(), None);

        // Once the provider takes the portrait, a retry draws it and the
        // scenes that waited for it, with it.
        h.images.accept_all();
        app.retry_job(id).unwrap();
        done(&app, id);
        let requests = h.images.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5, "the portrait and the two scenes");
        assert_eq!(requests[2].shape, ImageShape::Portrait);
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        let portrait = plan.cast()[0].portrait().unwrap().generation.id;
        assert_eq!(plan.cast()[0].failure(), None);
        for scene in [0, 2] {
            let scene = &plan.scenes()[scene];
            assert_eq!(scene.failure(), None);
            assert_eq!(scene.image_portraits(), Some(&vec![(0, Some(portrait))]));
        }
        assert_eq!(plan.scenes()[1].image_portraits(), Some(&Vec::new()));
        assert!(app.scenes(project.id).unwrap().old_portraits.is_empty());
    }

    #[test]
    fn a_person_is_drawn_from_the_description_alone_only_when_the_user_says_so() {
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

        // The user gives up on the chef's portrait: his scenes are drawn
        // from his description, and no portrait is tried or counted.
        let plan = app.set_cast_described_only(project.id, 0, true).unwrap();
        assert!(plan.cast()[0].is_described_only());
        assert_eq!(plan.cast()[0].failure(), None, "no portrait is asked for");
        for scene in [0, 2] {
            assert_eq!(plan.scenes()[scene].failure(), None, "no longer waiting");
        }
        assert!(matches!(
            app.redraw_cast_portrait(project.id, 0, BudgetConsent::Ask),
            Err(SceneError::NothingToGenerate)
        ));
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.missing_images(), 2, "two scenes, no portrait");
        let asked = h.images.requests.lock().unwrap().len();
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let requests = h.images.requests.lock().unwrap()[asked..].to_vec();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| request.shape != ImageShape::Portrait)
        );
        assert!(
            requests[0]
                .prompt
                .contains("the chef: A man in his fifties, brown skin")
        );
        let view = app.scenes(project.id).unwrap();
        let plan = view.plan.as_ref().unwrap();
        assert_eq!(plan.scenes()[0].image_portraits(), Some(&vec![(0, None)]));
        assert!(view.old_portraits.is_empty(), "as the user chose");
        assert_eq!(view.old_portraits_estimate, None);

        // Back to a portrait: the images drawn without one are out of date,
        // and are counted only for the button that draws them again.
        h.images.accept_all();
        app.set_cast_described_only(project.id, 0, false).unwrap();
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.old_portraits, [0, 2]);
        assert_eq!(view.missing_images(), 0);
        assert_eq!(
            view.old_portraits_estimate,
            Some(app.estimate(&[images_call(3)]).unwrap()),
            "two scenes and the portrait"
        );
        let asked = h.images.requests.lock().unwrap().len();
        done(
            &app,
            app.redraw_scenes_with_old_portraits(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let requests = h.images.requests.lock().unwrap()[asked..].to_vec();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].shape, ImageShape::Portrait);
        let view = app.scenes(project.id).unwrap();
        assert!(view.old_portraits.is_empty(), "the new images wait");
        assert!(view.plan.unwrap().scenes()[0].pending().is_some());
        assert!(matches!(
            app.redraw_scenes_with_old_portraits(project.id, BudgetConsent::Ask),
            Err(SceneError::NothingToGenerate)
        ));
    }

    #[test]
    fn a_portrait_that_will_not_read_is_let_go_and_the_next_run_draws_it_again() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        let portrait = plan.cast()[0].portrait().unwrap().file.clone();
        h.files.remove(project.id, &portrait).unwrap();

        let asked = h.images.requests.lock().unwrap().len();
        let job = wait_done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::PortraitMissing);
        assert!(
            failure
                .detail
                .contains("scene 1 was not drawn, for lack of the portrait of the chef"),
            "{}",
            failure.detail
        );
        assert_eq!(h.images.requests.lock().unwrap().len(), asked, "unpaid");
        let view = app.scenes(project.id).unwrap();
        let plan = view.plan.as_ref().unwrap();
        assert_eq!(
            plan.scenes()[0].failure(),
            Some(JobFailureKind::PortraitMissing)
        );
        assert_eq!(plan.scenes()[0].pending(), None);
        // The person shows as having no portrait, with why, and what
        // drawing the scene costs now counts a new one.
        assert_eq!(plan.cast()[0].portrait(), None);
        assert_eq!(plan.cast()[0].failure(), Some(JobFailureKind::Files));
        assert_eq!(
            view.image_estimates[0],
            app.estimate(&[images_call(2)]).unwrap()
        );

        // A retry draws the portrait again, then the scene with it.
        app.retry_job(job.id()).unwrap();
        done(&app, job.id());
        let requests = h.images.requests.lock().unwrap()[asked..].to_vec();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].shape, ImageShape::Portrait);
        assert_eq!(
            requests[1].references.len(),
            2,
            "the new portrait, and the scene without him for the look"
        );
        let view = app.scenes(project.id).unwrap();
        let plan = view.plan.as_ref().unwrap();
        let portrait = plan.cast()[0].portrait().unwrap().generation.id;
        assert_eq!(plan.cast()[0].failure(), None);
        assert_eq!(plan.scenes()[0].failure(), None);
        assert_eq!(
            plan.scenes()[0].pending_portraits(),
            Some(&vec![(0, Some(portrait))])
        );
        assert_eq!(
            view.old_portraits,
            [2],
            "the other scene shows the lost one"
        );
    }

    #[test]
    fn no_portrait_is_bought_for_a_scene_another_missing_portrait_already_stops() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        h.answer(
            serde_json::json!({
                "cast": [
                    {"name": "the chef", "description": "A man in his fifties, grey beard, white apron"},
                    {"name": "Ana", "description": "A woman in her twenties, black hair in a low bun"},
                ],
                "scenes": [
                    {"first_sentence": 0, "prompt": "The chef and Ana at the launch pad",
                     "cast": [0, 1], "motion": "Slow push in"},
                    {"first_sentence": 1, "prompt": "The probe drifting away from Earth",
                     "cast": [], "motion": "The probe drifts left"},
                ],
            })
            .to_string(),
        );
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        h.images.decline("grey beard");

        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, id);
        assert_eq!(job.state(), JobState::Failed);
        let detail = job.failure().unwrap().detail.clone();
        assert!(
            detail.contains("scene 1 was not drawn, for lack of the portrait of the chef"),
            "{detail}"
        );
        // The chef's portrait is declined, so the scene will not be drawn
        // in this run: Ana's portrait is not bought for it.
        let requests = h.images.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2, "the chef's portrait and scene 2");
        assert!(requests[1].prompt.starts_with("The probe drifting away"));
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert_eq!(plan.cast()[1].portrait(), None);
        assert_eq!(plan.cast()[1].failure(), None);

        // The retry tries the chef's again, then draws Ana's and the scene
        // with both.
        h.images.accept_all();
        app.retry_job(id).unwrap();
        done(&app, id);
        let requests = h.images.requests.lock().unwrap()[2..].to_vec();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests[2].references.len(),
            3,
            "both portraits and the look"
        );
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        let ids: Vec<_> = plan
            .cast()
            .iter()
            .map(|cast| cast.portrait().map(|portrait| portrait.generation.id))
            .collect();
        assert_eq!(
            plan.scenes()[0].image_portraits(),
            Some(&vec![(0, ids[0]), (1, ids[1])])
        );
    }

    #[test]
    fn a_scene_drawn_with_an_older_portrait_is_marked_until_it_is_drawn_again() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert!(app.scenes(project.id).unwrap().old_portraits.is_empty());

        done(
            &app,
            app.redraw_cast_portrait(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.old_portraits, [0, 2], "they show the old face");
        assert_eq!(
            view.old_portraits_estimate,
            Some(app.estimate(&[images_call(2)]).unwrap())
        );
        assert_eq!(view.missing_images(), 0, "not counted as missing");

        done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(app.scenes(project.id).unwrap().old_portraits, [2]);
        app.reject_scene_image(project.id, 0).unwrap();
        assert_eq!(app.scenes(project.id).unwrap().old_portraits, [0, 2]);
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
    fn a_retry_does_not_draw_again_a_scene_whose_image_was_replaced_since() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        h.images.decline("signal");
        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Failed);

        // The user draws scene 2 again and takes the new image, so the
        // one this job drew is gone.
        done(
            &app,
            app.regenerate_scene_image(project.id, 1, BudgetConsent::Ask)
                .unwrap(),
        );
        let replaced = app.accept_scene_image(project.id, 1).unwrap();

        h.images.accept_all();
        let asked = h.images.prompts().len();
        app.retry_job(id).unwrap();
        done(&app, id);

        let retried = &h.images.prompts()[asked..];
        assert_eq!(retried.len(), 1, "only the scene that failed: {retried:?}");
        assert!(retried[0].contains("signal"), "{retried:?}");
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert_eq!(plan.scenes()[1].image(), replaced.scenes()[1].image());
        assert_eq!(plan.scenes()[1].pending(), None, "it was drawn again");
        assert!(plan.scenes()[2].image().is_some());
    }

    #[test]
    fn a_retry_does_not_draw_a_failed_scene_another_job_drew_since() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        h.images.decline("signal");
        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Failed);
        h.images.accept_all();

        // The user draws the declined scene by itself, then retries the
        // first job: there is nothing left for it to draw.
        done(
            &app,
            app.regenerate_scene_image(project.id, 2, BudgetConsent::Ask)
                .unwrap(),
        );
        let theirs = app.scenes(project.id).unwrap().plan.unwrap();
        let asked = h.images.prompts().len();
        app.retry_job(id).unwrap();
        let job = done(&app, id);

        assert_eq!(job.state(), JobState::Done);
        assert_eq!(
            h.images.prompts().len(),
            asked,
            "scene 3 was paid for twice"
        );
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert_eq!(plan.scenes()[2].image(), theirs.scenes()[2].image());
        assert_eq!(plan.scenes()[2].pending(), None);
    }

    #[test]
    fn a_retry_asks_again_about_the_images_it_still_has_to_pay_for() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        h.images.decline("signal");
        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Failed);
        h.images.accept_all();

        // The video's cap is lower now than what it has cost: the retry
        // is asked about like a new job, for the one scene left to draw.
        app.set_spend_cap(project.id, "0.01").unwrap();
        let asked = h.images.prompts().len();
        let refused = app.retry_job(id).unwrap_err();
        let crate::jobs::JobActionError::OverBudget(estimate) = refused else {
            panic!("{refused:?}");
        };
        let one = app.estimate(&[images_call(1)]).unwrap().total();
        assert_eq!(estimate.total(), one);
        assert_eq!(
            estimate.cap.unwrap().cap,
            bardo_domain::Money::from_cents(1)
        );
        let job = app.jobs().into_iter().find(|job| job.id() == id).unwrap();
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(h.images.prompts().len(), asked, "drawn before the answer");
        // The question on screen follows what changes while it waits.
        assert_eq!(app.retry_question(id).unwrap().total(), one);
        app.set_spend_cap(project.id, "50").unwrap();
        assert!(app.retry_question(id).is_none(), "under the cap now");
        assert_eq!(h.images.prompts().len(), asked, "asking draws nothing");
        app.set_spend_cap(project.id, "0.01").unwrap();

        app.retry_job_with(id, BudgetConsent::Confirmed).unwrap();
        assert_eq!(done(&app, id).state(), JobState::Done);
        assert_eq!(h.images.prompts().len(), asked + 1);
        assert!(
            app.retry_question(id).is_none(),
            "a job that ran asks nothing"
        );
    }

    #[test]
    fn a_retry_with_nothing_left_to_pay_for_asks_nothing() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        h.images.decline("signal");
        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Failed);
        h.images.accept_all();
        done(
            &app,
            app.regenerate_scene_image(project.id, 2, BudgetConsent::Ask)
                .unwrap(),
        );

        // Another job drew the scene since: the retry pays for nothing,
        // so a cap already passed is no reason to ask.
        app.set_spend_cap(project.id, "0.01").unwrap();
        let asked = h.images.prompts().len();
        app.retry_job(id).unwrap();
        assert_eq!(done(&app, id).state(), JobState::Done);
        assert_eq!(h.images.prompts().len(), asked);
    }

    #[test]
    fn a_retry_waits_for_another_image_job_of_the_video() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        h.images.decline("signal");
        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Failed);
        h.images.accept_all();

        // The declined scene is being drawn by another job: retried now,
        // the first would not see that image yet and would pay for another.
        *h.images.delay.lock().unwrap() = Duration::from_millis(300);
        let other = app
            .regenerate_scene_image(project.id, 2, BudgetConsent::Ask)
            .unwrap();
        let refused = app.retry_job(id).unwrap_err();
        assert!(matches!(refused, crate::jobs::JobActionError::ScenesTaken));
        assert_eq!(refused.message(), crate::Text::JobScenesTaken);

        *h.images.delay.lock().unwrap() = Duration::ZERO;
        done(&app, other);
        let asked = h.images.prompts().len();
        app.retry_job(id).unwrap();
        assert_eq!(done(&app, id).state(), JobState::Done);
        assert_eq!(h.images.prompts().len(), asked);
    }

    #[test]
    fn a_video_whose_older_image_job_runs_again_takes_no_other_image_job() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        h.images.decline("signal");
        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Failed);
        // A newer job of the video, over by now.
        done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        h.images.accept_all();

        // The first job, retried, is drawing: the newer job being over
        // does not free the video for another.
        *h.images.delay.lock().unwrap() = Duration::from_millis(300);
        app.retry_job(id).unwrap();
        assert!(app.scenes(project.id).unwrap().is_busy());
        assert!(matches!(
            app.regenerate_scene_image(project.id, 2, BudgetConsent::Ask),
            Err(SceneError::Busy)
        ));
        *h.images.delay.lock().unwrap() = Duration::ZERO;
        done(&app, id);
    }

    #[test]
    fn a_retry_does_not_draw_again_a_new_image_the_user_rejected() {
        let h = Harness::new();
        let app = h.start();
        let (project, drawn) = h.drawn_project(&app);
        // Every scene is drawn again in one job, for a new style, and the
        // last is declined.
        let clay = bardo_domain::StyleChoice::Catalog(bardo_domain::VisualStyle::ClayStopMotion);
        app.set_project_style(project.id, Some(clay)).unwrap();
        h.images.decline("signal");
        let id = app
            .redraw_restyled_scenes(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Failed);
        app.reject_scene_image(project.id, 0).unwrap();

        h.images.accept_all();
        let asked = h.images.prompts().len();
        app.retry_job(id).unwrap();
        done(&app, id);

        assert_eq!(h.images.prompts().len() - asked, 1, "only scene 3");
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert_eq!(
            plan.scenes()[0].pending(),
            None,
            "the rejected image is back"
        );
        assert_eq!(plan.scenes()[0].image(), drawn.scenes()[0].image());
        assert!(plan.scenes()[1].pending().is_some());
        assert!(plan.scenes()[2].pending().is_some());
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
            app.generate_narration(project.id, crate::PicturesConsent::Ask, BudgetConsent::Ask)
                .unwrap(),
        );

        let view = app.scenes(project.id).unwrap();
        assert!(view.stale);
        assert!(view.plan.is_some());
    }

    /// Reads the project's script again, as long as `length`, whatever
    /// pictures that leaves behind.
    pub(crate) fn narrate_again(
        h: &Harness,
        app: &Bardo,
        project: &VideoProject,
        length: Duration,
    ) {
        *h.speech.length.lock().unwrap() = length;
        done(
            app,
            app.generate_narration(project.id, agreed(app, project), BudgetConsent::Ask)
                .unwrap(),
        );
    }

    /// The user's yes to what a new narration of the script puts out of
    /// date right now; nothing to agree to when nothing is at stake.
    pub(crate) fn agreed(app: &Bardo, project: &VideoProject) -> crate::PicturesConsent {
        let script = app.scripts.script(project.id).unwrap().unwrap();
        match app
            .pictures_at_stake(project.id, script.text().as_str())
            .unwrap()
        {
            Some(stake) => crate::PicturesConsent::Confirmed(stake),
            None => crate::PicturesConsent::Ask,
        }
    }

    /// A drawn project whose first scene also has a clip in use and a new
    /// image waiting for review.
    fn pictured_project(h: &Harness, app: &Bardo) -> (VideoProject, ScenePlan) {
        let (project, _) = h.drawn_project(app);
        done(
            app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        app.accept_scene_clip(project.id, 0).unwrap();
        done(
            app,
            app.regenerate_scene_image(project.id, 1, BudgetConsent::Ask)
                .unwrap(),
        );
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        (project, plan)
    }

    #[test]
    fn nothing_is_drawn_or_animated_for_a_plan_that_is_out_of_date() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        narrate_again(&h, &app, &project, Duration::from_secs(2));
        assert!(app.scenes(project.id).unwrap().stale);
        // Scenes left in another style are not drawn again either.
        app.set_project_style(
            project.id,
            Some(StyleChoice::Catalog(
                bardo_domain::VisualStyle::ClayStopMotion,
            )),
        )
        .unwrap();
        assert!(!app.scenes(project.id).unwrap().restyled.is_empty());
        let (drawn, sent) = (h.images.prompts().len(), h.clips.submissions().len());

        let refused = [
            app.generate_scene_images(project.id, BudgetConsent::Confirmed),
            app.redraw_restyled_scenes(project.id, BudgetConsent::Confirmed),
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Confirmed),
            app.redraw_cast_portrait(project.id, 0, BudgetConsent::Confirmed),
            app.generate_scene_clip(project.id, 0, BudgetConsent::Confirmed),
            app.generate_missing_clips(project.id, BudgetConsent::Confirmed),
        ];
        for refused in refused {
            let error = refused.unwrap_err();
            assert!(matches!(error, SceneError::PlanOutOfDate), "{error:?}");
            assert_eq!(error.message(), Text::ScenesPlanOutOfDate);
        }
        assert_eq!(h.images.prompts().len(), drawn, "no image was paid for");
        assert_eq!(h.clips.submissions().len(), sent, "no clip was paid for");

        // Free changes still work, and so does what puts the scenes right.
        app.edit_scene_prompt(project.id, 0, "A probe at dawn.")
            .unwrap();
        app.fit_scenes(project.id).unwrap();
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
    }

    #[test]
    fn the_narration_waits_for_scenes_being_planned_drawn_or_animated() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        let again = |app: &Bardo| {
            app.generate_narration(project.id, agreed(app, &project), BudgetConsent::Confirmed)
        };

        // Images being drawn.
        *h.images.delay.lock().unwrap() = Duration::from_millis(200);
        let images = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        let held = again(&app).unwrap_err();
        assert!(matches!(held, NarrationError::ScenesBusy), "{held:?}");
        assert_eq!(held.message(), Text::NarrationScenesBusy);
        *h.images.delay.lock().unwrap() = Duration::ZERO;
        done(&app, images);

        // A clip being made.
        h.clips.hold(true);
        let clip = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Ask)
            .unwrap();
        assert!(matches!(again(&app), Err(NarrationError::ScenesBusy)));
        h.clips.hold(false);
        done(&app, clip);

        // The scenes being planned.
        *h.text.delay.lock().unwrap() = Duration::from_millis(200);
        h.answer(plan_answer());
        let plan = app
            .plan_scenes(project.id, true, BudgetConsent::Ask)
            .unwrap();
        assert!(matches!(again(&app), Err(NarrationError::ScenesBusy)));
        *h.text.delay.lock().unwrap() = Duration::ZERO;
        done(&app, plan);

        // Nothing under way: the narration is read again.
        done(&app, again(&app).unwrap());
    }

    #[test]
    fn scenes_wait_for_a_narration_being_made() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        *h.speech.delay.lock().unwrap() = Duration::from_millis(300);
        let narration = app
            .generate_narration(project.id, agreed(&app, &project), BudgetConsent::Ask)
            .unwrap();
        let (drawn, sent) = (h.images.prompts().len(), h.clips.submissions().len());
        let planned = h.text.requests().len();

        let held = [
            app.plan_scenes(project.id, true, BudgetConsent::Confirmed),
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Confirmed),
            app.generate_scene_clip(project.id, 0, BudgetConsent::Confirmed),
            app.generate_missing_clips(project.id, BudgetConsent::Confirmed),
        ];
        for held in held {
            let error = held.unwrap_err();
            assert!(matches!(error, SceneError::NarrationBusy), "{error:?}");
            assert_eq!(error.message(), Text::ScenesNarrationBusy);
        }
        assert!(matches!(
            app.fit_scenes(project.id),
            Ok(plan) if !plan.is_stale(app.narrations.narration(project.id).unwrap().as_ref())
        ));
        done(&app, narration);
        assert_eq!(h.images.prompts().len(), drawn);
        assert_eq!(h.clips.submissions().len(), sent);
        assert_eq!(h.text.requests().len(), planned);
    }

    #[test]
    fn a_new_narration_over_paid_pictures_asks_first_with_the_counts() {
        use crate::{PicturesAtStake, PicturesConsent};

        let h = Harness::new();
        let app = h.start();
        let (project, _) = pictured_project(&h, &app);
        let ask = |app: &Bardo| {
            app.generate_narration(project.id, PicturesConsent::Ask, BudgetConsent::Confirmed)
        };
        let pictures = bardo_domain::PaidPictures {
            images: 4,
            clips: 1,
        };
        let narrations = h.speech.requests().len();

        // The same script read again: the pictures can be kept.
        let error = ask(&app).unwrap_err();
        assert!(
            matches!(
                error,
                NarrationError::WouldOutdatePictures(stake)
                    if stake == PicturesAtStake { pictures, can_fit: true }
            ),
            "{error:?}"
        );
        assert_eq!(error.message(), Text::NarrationOutdatesPictures);

        // A changed script: the scenes will have to be planned again.
        app.edit_script(project.id, "Era uma vez uma sonda perdida.")
            .unwrap();
        assert!(matches!(
            ask(&app),
            Err(NarrationError::WouldOutdatePictures(stake))
                if stake == PicturesAtStake { pictures, can_fit: false }
        ));
        // A yes given while the pictures could still be kept does not
        // cover losing them: the question comes again, as it is now.
        let seen = PicturesAtStake {
            pictures,
            can_fit: true,
        };
        assert!(matches!(
            app.generate_narration(
                project.id,
                PicturesConsent::Confirmed(seen),
                BudgetConsent::Confirmed
            ),
            Err(NarrationError::WouldOutdatePictures(stake)) if !stake.can_fit
        ));
        assert_eq!(h.speech.requests().len(), narrations, "nothing was read");

        // With the user's yes to that, it is read.
        narrate_again(&h, &app, &project, Duration::from_secs(1));
        let view = app.scenes(project.id).unwrap();
        assert!(view.stale && !view.can_fit);

        // The pictures are now out of date for good: another narration
        // takes nothing more from them, so nothing is asked.
        done(&app, ask(&app).unwrap());
    }

    #[test]
    fn a_recording_or_a_capture_over_paid_pictures_asks_and_waits_too() {
        use crate::captures::{CaptureError, CaptureVoiceChoice};
        use crate::narration_import::Recording;

        let h = Harness::new();
        let app = h.start();
        let (project, _) = pictured_project(&h, &app);
        let folder = tempfile::tempdir().unwrap();
        let recording = Recording {
            path: folder.path().join("voice.mp3"),
            file_name: "voice.mp3".into(),
            format: bardo_media::AudioFormat::Mp3,
            duration: Duration::from_secs(4),
        };
        let capture = crate::captures::ChosenCapture {
            path: folder.path().join("Take 1.MOV"),
            file_name: "Take 1.MOV".into(),
            video: bardo_domain::CaptureVideo::check(
                "Take 1.MOV",
                &bardo_domain::RecordingFacts {
                    duration: Duration::from_secs(4),
                    video: Some((1080, 1920, 29.97)),
                    has_audio: true,
                },
            )
            .unwrap(),
            size: 10,
        };
        let pictures = bardo_domain::PaidPictures {
            images: 4,
            clips: 1,
        };

        // Both read the script, so the plan can be fitted afterwards.
        match app.import_narration(
            project.id,
            &recording,
            crate::PicturesConsent::Ask,
            BudgetConsent::Ask,
        ) {
            Err(NarrationError::WouldOutdatePictures(stake)) => {
                assert_eq!((stake.pictures, stake.can_fit), (pictures, true));
            }
            other => panic!("{other:?}"),
        }
        match app.import_capture(
            project.id,
            &capture,
            CaptureVoiceChoice::Own,
            true,
            crate::PicturesConsent::Ask,
            BudgetConsent::Ask,
        ) {
            Err(CaptureError::WouldOutdatePictures(stake)) => {
                assert_eq!((stake.pictures, stake.can_fit), (pictures, true));
            }
            other => panic!("{other:?}"),
        }

        // And neither starts under a scene being animated, even confirmed.
        h.clips.hold(true);
        let id = app
            .generate_scene_clip(project.id, 2, BudgetConsent::Ask)
            .unwrap();
        let error = app
            .import_narration(
                project.id,
                &recording,
                agreed(&app, &project),
                BudgetConsent::Confirmed,
            )
            .unwrap_err();
        assert!(matches!(error, NarrationError::ScenesBusy));
        assert_eq!(error.message(), Text::NarrationScenesBusy);
        let error = app
            .import_capture(
                project.id,
                &capture,
                CaptureVoiceChoice::Own,
                true,
                agreed(&app, &project),
                BudgetConsent::Confirmed,
            )
            .unwrap_err();
        assert!(matches!(error, CaptureError::ScenesBusy));
        assert_eq!(error.message(), Text::NarrationScenesBusy);
        h.clips.hold(false);
        done(&app, id);
    }

    #[test]
    fn a_narration_over_scenes_without_pictures_asks_nothing() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        done(
            &app,
            app.generate_narration(project.id, crate::PicturesConsent::Ask, BudgetConsent::Ask)
                .unwrap(),
        );
    }

    #[test]
    fn a_narration_of_the_same_text_takes_the_plan_with_every_picture() {
        use bardo_domain::CostRepository;

        let h = Harness::new();
        let app = h.start();
        let (project, plan) = pictured_project(&h, &app);
        let view = app.scenes(project.id).unwrap();
        assert!(!view.stale && !view.can_fit);
        let paid = || {
            let records = h.db.project_costs(project.id).unwrap();
            let narrations = records
                .iter()
                .filter(|record| record.purpose == CostPurpose::Narration)
                .count();
            (narrations, records.len() - narrations)
        };
        let (narrations, rest) = paid();

        // Read again four times slower.
        narrate_again(&h, &app, &project, Duration::from_secs(4));
        let narration = app.narrations.narration(project.id).unwrap().unwrap();
        let view = app.scenes(project.id).unwrap();
        assert!(view.stale && view.can_fit);
        assert_eq!(view.plan.as_ref(), Some(&plan), "untouched until fitted");

        let fitted = app.fit_scenes(project.id).unwrap();

        let view = app.scenes(project.id).unwrap();
        assert!(!view.stale && !view.can_fit);
        let saved = view.plan.unwrap();
        assert_eq!(saved.scenes(), fitted.scenes(), "saved as returned");
        assert_eq!(saved.narration, fitted.narration);
        assert_eq!(fitted.id, plan.id);
        assert_eq!(fitted.narration, narration.id);
        // Every scene is where the new narration says its words.
        let spoken = sentences(&narration);
        let times: Vec<_> = fitted.scenes().iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(
            times,
            [
                (Duration::ZERO, spoken[1].start),
                (spoken[1].start, spoken[2].start),
                (spoken[2].start, narration.duration),
            ]
        );
        assert!(fitted.scenes()[1].start > plan.scenes()[1].start * 3);
        // And holds what it held: prompts, images, the new image waiting,
        // the clip, and their files.
        for (fitted, scene) in fitted.scenes().iter().zip(plan.scenes()) {
            assert_eq!(fitted.text, scene.text);
            assert_eq!(fitted.prompt(), scene.prompt());
            assert_eq!(fitted.image(), scene.image());
            assert_eq!(fitted.pending(), scene.pending());
            assert_eq!(fitted.clip(), scene.clip());
            assert_eq!(fitted.pending_clip(), scene.pending_clip());
        }
        assert_eq!(fitted.paid_pictures(), plan.paid_pictures());
        assert_eq!(plan.files().count(), 5);
        for file in plan.files() {
            assert!(h.files.exists(project.id, file), "{file} is gone");
        }
        // Only the narration was paid for, and paid work is allowed again.
        assert_eq!(paid(), (narrations + 1, rest));
        app.accept_scene_image(project.id, 1).unwrap();
        done(
            &app,
            app.generate_missing_clips(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
    }

    #[test]
    fn a_narration_of_other_text_does_not_take_the_plan() {
        let h = Harness::new();
        let app = h.start();
        let (project, plan) = pictured_project(&h, &app);
        app.edit_script(
            project.id,
            "Era uma vez uma sonda perdida. Ninguém sabe por quê.",
        )
        .unwrap();
        narrate_again(&h, &app, &project, Duration::from_secs(1));
        let view = app.scenes(project.id).unwrap();
        assert!(view.stale && !view.can_fit);

        let error = app.fit_scenes(project.id).unwrap_err();
        assert!(matches!(error, SceneError::OtherText(_)), "{error:?}");
        assert_eq!(error.message(), Text::ScenesOtherText);
        assert_eq!(app.scenes(project.id).unwrap().plan, Some(plan));

        // Planning again is the way, with its own question.
        assert!(matches!(
            app.plan_scenes(project.id, false, BudgetConsent::Ask),
            Err(SceneError::WouldDiscardImages(5))
        ));
    }

    #[test]
    fn fitting_needs_planned_scenes_of_the_users_own_project() {
        let h = Harness::new();
        let app = h.start();
        let bare = project(&app);
        assert!(matches!(app.fit_scenes(bare.id), Err(SceneError::NoPlan)));
        assert!(matches!(
            app.fit_scenes(VideoProjectId::new()),
            Err(SceneError::ProjectNotFound)
        ));
        // Scenes already on the narration come back as they are.
        let (project, plan) = h.planned_project(&app);
        assert_eq!(app.fit_scenes(project.id).unwrap(), plan);
    }

    #[test]
    fn an_image_job_run_again_after_a_new_narration_draws_nothing() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        h.images.decline("control room");
        let images = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, images).state(), JobState::Failed);
        h.images.accept_all();
        narrate_again(&h, &app, &project, Duration::from_secs(2));
        let drawn = h.images.prompts().len();

        // From the jobs panel, which asks the scenes panel nothing.
        app.retry_job(images).unwrap();
        let job = wait_done(&app, images);

        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::OutOfDate);
        assert_eq!(h.images.prompts().len(), drawn, "nothing was paid for");
        assert_eq!(app.scenes(project.id).unwrap().missing_images(), 1);

        // Fitted, the same job draws what it still owed.
        app.fit_scenes(project.id).unwrap();
        app.retry_job(images).unwrap();
        done(&app, images);
        assert_eq!(h.images.prompts().len(), drawn + 1);
        assert_eq!(app.scenes(project.id).unwrap().missing_images(), 0);
    }

    #[test]
    fn jobs_run_again_from_the_panel_wait_for_each_other_too() {
        use crate::jobs::JobActionError;

        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        // A narration job and an image job that both failed.
        *h.speech.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "invalid key",
        ));
        let narration = app
            .generate_narration(project.id, agreed(&app, &project), BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, narration).state(), JobState::Failed);
        *h.speech.failure.lock().unwrap() = None;
        h.images.decline("control room");
        let images = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, images).state(), JobState::Failed);
        h.images.accept_all();

        // The image job runs again: the narration job waits for it.
        *h.images.delay.lock().unwrap() = Duration::from_millis(300);
        app.retry_job(images).unwrap();
        let held = app.retry_job(narration).unwrap_err();
        assert!(matches!(held, JobActionError::ScenesBusy), "{held:?}");
        assert_eq!(held.message(), Text::JobScenesBusy);
        *h.images.delay.lock().unwrap() = Duration::ZERO;
        done(&app, images);

        // The scenes hold images now. The narration job was started before
        // there were any, so running it again would skip the question the
        // Narration stage asks: it starts from there.
        let read = h.speech.requests().len();
        let held = app.retry_job(narration).unwrap_err();
        assert!(matches!(held, JobActionError::AsksFirst), "{held:?}");
        assert_eq!(held.message(), Text::JobAsksFirst);
        assert_eq!(h.speech.requests().len(), read);

        // A narration being made: scene jobs wait for it.
        *h.speech.delay.lock().unwrap() = Duration::from_millis(300);
        let narration = app
            .generate_narration(project.id, agreed(&app, &project), BudgetConsent::Ask)
            .unwrap();
        let held = app.retry_job(images).unwrap_err();
        assert!(matches!(held, JobActionError::NarrationBusy), "{held:?}");
        assert_eq!(held.message(), Text::JobNarrationBusy);
        done(&app, narration);
    }

    #[test]
    fn an_older_job_run_again_holds_the_others_like_a_new_one() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        // An older narration job that failed, and a newer one that is done.
        *h.speech.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "invalid key",
        ));
        let old_narration = app
            .generate_narration(project.id, agreed(&app, &project), BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, old_narration).state(), JobState::Failed);
        *h.speech.failure.lock().unwrap() = None;
        narrate_again(&h, &app, &project, Duration::from_secs(2));
        app.fit_scenes(project.id).unwrap();

        // The older one run again is a narration being made, though the
        // latest narration job is done.
        *h.speech.delay.lock().unwrap() = Duration::from_millis(300);
        app.retry_job(old_narration).unwrap();
        assert!(matches!(
            app.generate_scene_images(project.id, BudgetConsent::Ask),
            Err(SceneError::NarrationBusy)
        ));
        done(&app, old_narration);
        *h.speech.delay.lock().unwrap() = Duration::ZERO;
        app.fit_scenes(project.id).unwrap();

        // An older image job that failed, and a newer one that is done.
        h.images.decline("control room");
        let old_images = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, old_images).state(), JobState::Failed);
        h.images.accept_all();
        done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );

        // The older one run again is scenes being drawn.
        *h.images.delay.lock().unwrap() = Duration::from_millis(300);
        app.retry_job(old_images).unwrap();
        assert!(matches!(
            app.generate_narration(project.id, agreed(&app, &project), BudgetConsent::Ask),
            Err(NarrationError::ScenesBusy)
        ));
        done(&app, old_images);
    }

    #[test]
    fn fitting_waits_for_a_narration_being_made() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        narrate_again(&h, &app, &project, Duration::from_secs(2));
        *h.speech.delay.lock().unwrap() = Duration::from_millis(300);
        let narration = app
            .generate_narration(project.id, agreed(&app, &project), BudgetConsent::Ask)
            .unwrap();

        assert!(matches!(
            app.fit_scenes(project.id),
            Err(SceneError::NarrationBusy)
        ));
        done(&app, narration);
        assert!(!app.fit_scenes(project.id).unwrap().scenes().is_empty());
    }

    #[test]
    fn a_plan_job_run_again_over_paid_pictures_starts_from_its_stage() {
        use crate::jobs::JobActionError;

        let h = Harness::new();
        let app = h.start();
        let (project, planned) = h.planned_project(&app);
        // Planning again fails; then the scenes it would replace are drawn.
        h.answer(r#"{"scenes": []}"#.to_owned());
        let plan = app
            .plan_scenes(project.id, false, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, plan).state(), JobState::Failed);
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let asked = h.text.requests().len();

        // Run again from the panel, it would discard the images unasked.
        h.answer(plan_answer());
        let held = app.retry_job(plan).unwrap_err();
        assert!(matches!(held, JobActionError::AsksFirst), "{held:?}");
        assert_eq!(h.text.requests().len(), asked, "Claude is not asked");
        let kept = app.scenes(project.id).unwrap().plan.unwrap();
        assert_eq!(kept.id, planned.id);
        assert!(kept.scenes().iter().all(|scene| scene.image().is_some()));
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
        assert_eq!(app.scenes(project.id).unwrap().plan, Some(planned.clone()));

        // The request was answered: asked for again, the job sends a new
        // one, and does not take the first for lost.
        let asked = h.text.requests().len();
        h.answer(plan_answer());
        app.retry_job(job.id()).unwrap();
        done(&app, job.id());
        assert_eq!(h.text.requests().len(), asked + 1);
        assert_ne!(app.scenes(project.id).unwrap().plan, Some(planned));
    }

    /// A scene plan whose job is stopped while Claude writes it (#295).
    mod stopped_plans {
        use bardo_domain::{Cost, CostPurpose, CostRecord, CostRepository, Money};

        use super::*;
        use crate::jobs::JobActionError;

        /// Far longer than any of these tests waits: an answer only a stop
        /// ends.
        const LONG: Duration = Duration::from_secs(120);

        fn plan_costs(h: &Harness, project: VideoProjectId) -> Vec<CostRecord> {
            h.db.project_costs(project)
                .unwrap()
                .into_iter()
                .filter(|record| record.purpose == CostPurpose::ScenePlan)
                .collect()
        }

        /// How long a wait may take before the test fails. Generous: a
        /// loaded CI runner can stall the worker threads for over ten
        /// seconds, and a passing test never waits this long.
        const STALLED: Duration = Duration::from_secs(60);

        fn wait_until(what: &str, done: impl Fn() -> bool) {
            let deadline = Instant::now() + STALLED;
            while !done() {
                assert!(Instant::now() < deadline, "never: {what}");
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        /// Starts a plan and waits until its request left.
        fn plan_sent(h: &Harness, app: &Bardo, project: VideoProjectId) -> JobId {
            let asked = h.text.requests().len();
            let id = app
                .plan_scenes(project, false, BudgetConsent::Confirmed)
                .unwrap();
            wait_until("the request left", || h.text.requests().len() > asked);
            id
        }

        /// Starts a plan Claude would write for a long time, and waits
        /// until its request left.
        fn long_plan(h: &Harness, app: &Bardo, project: VideoProjectId) -> JobId {
            *h.text.delay.lock().unwrap() = LONG;
            plan_sent(h, app, project)
        }

        /// Starts a plan whose answer `text` is on its way, whole, and
        /// arrives when the test lets it (`h.text.let_go`), whatever
        /// stopped the job meanwhile. Waits until its request left.
        fn plan_arriving(h: &Harness, app: &Bardo, project: VideoProjectId, text: String) -> JobId {
            *h.text.deaf.lock().unwrap() = true;
            h.text.hold();
            h.answer(text);
            plan_sent(h, app, project)
        }

        /// From here on Claude answers at once, and can be stopped.
        fn answers_at_once(h: &Harness) {
            *h.text.deaf.lock().unwrap() = false;
            *h.text.delay.lock().unwrap() = Duration::ZERO;
            h.text.let_go();
        }

        fn given_up(h: &Harness) -> u32 {
            h.text.given_up.load(std::sync::atomic::Ordering::SeqCst)
        }

        #[test]
        fn cancelling_a_plan_gives_its_request_up_within_moments() {
            let h = Harness::new();
            let app = h.start();
            let project = h.narrated_project(&app);
            let asked = h.text.requests().len();
            let id = long_plan(&h, &app, project.id);

            app.cancel_job(id).unwrap();
            // Long before the answer's end, which is two minutes away: the
            // wait gives up at a small part of that.
            wait_until("the handler let go", || !app.job_runs_here(id));

            assert_eq!(given_up(&h), 1, "the request was given up, not awaited");
            assert_eq!(h.text.unstoppable(), 0, "the job handed its stop over");
            let job = wait_done(&app, id);
            assert_eq!(job.state(), JobState::Cancelled);
            assert_eq!(app.scenes(project.id).unwrap().plan, None);
            // Claude bills what it wrote until then, and the stream never
            // said how much: what such a plan usually takes is in Costs.
            let records = plan_costs(&h, project.id);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].job, Some(id));
            assert!(
                matches!(records[0].cost, Cost::Estimated(amount) if amount > Money::ZERO),
                "{:?}",
                records[0].cost
            );

            // The user asks again: a new request, and its plan.
            answers_at_once(&h);
            h.answer(plan_answer());
            app.retry_job(id).unwrap();
            done(&app, id);
            assert_eq!(h.text.requests().len(), asked + 2);
            assert!(app.scenes(project.id).unwrap().plan.is_some());
            assert_eq!(plan_costs(&h, project.id).len(), 2);
        }

        #[test]
        fn no_second_plan_starts_while_a_cancelled_one_still_stops() {
            let h = Harness::new();
            let app = h.start();
            let project = h.narrated_project(&app);
            // An answer that goes on after the cancel, as one whose
            // connection is slow to close.
            let id = plan_arriving(&h, &app, project.id, plan_answer());
            let asked = h.text.requests().len();

            app.cancel_job(id).unwrap();
            assert_eq!(wait_done(&app, id).state(), JobState::Cancelled);
            assert!(app.job_runs_here(id), "the handler has not returned yet");

            // The job reads cancelled, and its request is still out: a
            // second one beside it would be paid for as well.
            let view = app.scenes(project.id).unwrap();
            assert!(view.stopping && view.is_busy());
            assert!(matches!(
                app.plan_scenes(project.id, false, BudgetConsent::Confirmed),
                Err(SceneError::Busy)
            ));
            assert_eq!(h.text.requests().len(), asked, "nothing was sent");

            h.text.let_go();
            wait_until("the handler let go", || !app.job_runs_here(id));
            let view = app.scenes(project.id).unwrap();
            assert!(!view.stopping && !view.is_busy());
        }

        #[test]
        fn an_answer_that_arrives_as_its_plan_is_cancelled_is_kept_for_a_retry() {
            let h = Harness::new();
            let app = h.start();
            let project = h.narrated_project(&app);
            *h.text.usage.lock().unwrap() = bardo_domain::TokenUsage {
                input_tokens: 900,
                output_tokens: 2_400,
            };
            let id = plan_arriving(&h, &app, project.id, plan_answer());
            let asked = h.text.requests().len();

            app.cancel_job(id).unwrap();
            h.text.let_go();
            wait_until("the handler let go", || !app.job_runs_here(id));

            // The user cancelled: the plan does not appear behind their
            // back. It is paid for, at what Claude counted.
            assert_eq!(wait_done(&app, id).state(), JobState::Cancelled);
            assert_eq!(app.scenes(project.id).unwrap().plan, None);
            let records = plan_costs(&h, project.id);
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].usage.output_tokens, 2_400);

            // Asked for again, the job plans from the answer it kept.
            app.retry_job(id).unwrap();
            done(&app, id);
            let plan = app.scenes(project.id).unwrap().plan.expect("planned");
            assert_eq!(plan.generation.job, Some(id));
            assert_eq!(plan.generation.usage.output_tokens, 2_400);
            assert_eq!(h.text.requests().len(), asked, "Claude is not asked again");
            assert_eq!(plan_costs(&h, project.id).len(), 1, "nor paid again");
        }

        #[test]
        fn planning_again_from_the_stage_uses_the_answer_a_cancelled_plan_kept() {
            let h = Harness::new();
            let app = h.start();
            let project = h.narrated_project(&app);
            let id = plan_arriving(&h, &app, project.id, plan_answer());
            let asked = h.text.requests().len();
            app.cancel_job(id).unwrap();
            h.text.let_go();
            wait_until("the handler let go", || !app.job_runs_here(id));
            assert_eq!(wait_done(&app, id).state(), JobState::Cancelled);

            // Plan scenes, on the stage, asks for the same plan: the job
            // that holds its answer goes on, and Claude is not paid twice.
            let again = app
                .plan_scenes(project.id, false, BudgetConsent::Confirmed)
                .unwrap();
            assert_eq!(again, id);
            done(&app, id);
            assert!(app.scenes(project.id).unwrap().plan.is_some());
            assert_eq!(h.text.requests().len(), asked);
            assert_eq!(plan_costs(&h, project.id).len(), 1);
        }

        #[test]
        fn a_kept_answer_is_not_used_for_another_plan() {
            let h = Harness::new();
            let app = h.start();
            let project = h.narrated_project(&app);
            let id = plan_arriving(&h, &app, project.id, plan_answer());
            app.cancel_job(id).unwrap();
            h.text.let_go();
            wait_until("the handler let go", || !app.job_runs_here(id));
            assert_eq!(wait_done(&app, id).state(), JobState::Cancelled);
            answers_at_once(&h);
            let asked = h.text.requests().len();

            // The prompt changed since: the answer kept is to another
            // question, so Claude is asked this one.
            let template = app.current_template(TemplateKind::ImagePrompt).unwrap();
            app.save_template(
                TemplateKind::ImagePrompt,
                &format!("{} Prefer wide shots.", template.body.instructions()),
                template.body.prompt(),
            )
            .unwrap();
            h.answer(plan_answer());
            let other = app
                .plan_scenes(project.id, false, BudgetConsent::Confirmed)
                .unwrap();
            assert_ne!(other, id);
            done(&app, other);
            assert_eq!(h.text.requests().len(), asked + 1);
            assert_eq!(plan_costs(&h, project.id).len(), 2);
            let plan = app.scenes(project.id).unwrap().plan.expect("planned");
            assert_eq!(plan.generation.job, Some(other));
        }

        /// The scene plans of `inner`, which cannot be saved while `down`.
        struct PlansDown {
            inner: Arc<Database>,
            down: std::sync::atomic::AtomicBool,
        }

        impl ScenePlanRepository for PlansDown {
            fn scene_plan(
                &self,
                project: VideoProjectId,
            ) -> Result<Option<ScenePlan>, RepositoryError> {
                self.inner.scene_plan(project)
            }

            fn save_scene_plan(&self, plan: &ScenePlan) -> Result<(), RepositoryError> {
                if self.down.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(RepositoryError(Box::new(std::io::Error::other(
                        "the disk is full",
                    ))));
                }
                self.inner.save_scene_plan(plan)
            }

            fn save_fitted_plan(&self, plan: &ScenePlan) -> Result<(), RepositoryError> {
                self.inner.save_fitted_plan(plan)
            }

            fn save_scene(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError> {
                self.inner.save_scene(plan, index)
            }

            fn save_cast_member(
                &self,
                plan: &ScenePlan,
                index: usize,
            ) -> Result<(), RepositoryError> {
                self.inner.save_cast_member(plan, index)
            }

            fn save_place(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError> {
                self.inner.save_place(plan, index)
            }

            fn shot_backdrops(
                &self,
                project: VideoProjectId,
            ) -> Result<Vec<bardo_domain::ShotBackdropDrawing>, RepositoryError> {
                self.inner.shot_backdrops(project)
            }

            fn add_shot_backdrop(
                &self,
                backdrop: &bardo_domain::ShotBackdropDrawing,
            ) -> Result<(), RepositoryError> {
                self.inner.add_shot_backdrop(backdrop)
            }

            fn keep_shot_backdrop(
                &self,
                backdrop: bardo_domain::BackdropId,
            ) -> Result<(), RepositoryError> {
                self.inner.keep_shot_backdrop(backdrop)
            }

            fn remove_shot_backdrop(
                &self,
                backdrop: bardo_domain::BackdropId,
            ) -> Result<(), RepositoryError> {
                self.inner.remove_shot_backdrop(backdrop)
            }
        }

        #[test]
        fn a_plan_that_could_not_be_saved_is_made_from_its_answer_when_asked_again() {
            use std::sync::atomic::Ordering;
            let h = Harness::new();
            let plans = Arc::new(PlansDown {
                inner: h.process_db(),
                down: false.into(),
            });
            let app = h.start_from(Repositories {
                scene_plans: Arc::clone(&plans) as _,
                ..Repositories::shared_with_files(
                    h.process_db(),
                    Arc::clone(&h.secrets) as _,
                    Arc::clone(&h.files) as _,
                )
            });
            let project = h.narrated_project(&app);
            let asked = h.text.requests().len();

            // Claude answers, and the plan cannot be saved.
            plans.down.store(true, Ordering::SeqCst);
            h.answer(plan_answer());
            let id = app
                .plan_scenes(project.id, false, BudgetConsent::Confirmed)
                .unwrap();
            assert_eq!(wait_done(&app, id).state(), JobState::Failed);
            assert_eq!(app.scenes(project.id).unwrap().plan, None);
            assert_eq!(h.text.requests().len(), asked + 1);
            assert_eq!(plan_costs(&h, project.id).len(), 1);

            // Asked for again on the stage, where a failed plan has no
            // Retry: the answer is paid for, and its job plans from it.
            plans.down.store(false, Ordering::SeqCst);
            let again = app
                .plan_scenes(project.id, false, BudgetConsent::Confirmed)
                .unwrap();
            assert_eq!(again, id);
            done(&app, id);
            assert!(app.scenes(project.id).unwrap().plan.is_some());
            assert_eq!(
                h.text.requests().len(),
                asked + 1,
                "Claude is not asked again"
            );
            assert_eq!(plan_costs(&h, project.id).len(), 1, "nor paid again");
        }

        #[test]
        fn a_kept_answer_that_is_no_plan_fails_once_and_then_claude_is_asked_again() {
            let h = Harness::new();
            let app = h.start();
            let project = h.narrated_project(&app);
            let no_plan = "{\"cast\":[],\"places\":[],\"scenes\":[]}".to_owned();
            let id = plan_arriving(&h, &app, project.id, no_plan);
            let asked = h.text.requests().len();
            app.cancel_job(id).unwrap();
            h.text.let_go();
            wait_until("the handler let go", || !app.job_runs_here(id));
            assert_eq!(wait_done(&app, id).state(), JobState::Cancelled);

            // The kept answer is no plan: the retry says so, without a
            // request.
            app.retry_job(id).unwrap();
            let job = wait_done(&app, id);
            assert_eq!(
                job.failure().unwrap().kind,
                JobFailureKind::UnexpectedAnswer
            );
            assert_eq!(h.text.requests().len(), asked);

            // It is not kept past that: the next retry asks Claude.
            answers_at_once(&h);
            h.answer(plan_answer());
            app.retry_job(id).unwrap();
            done(&app, id);
            assert_eq!(h.text.requests().len(), asked + 1);
            assert!(app.scenes(project.id).unwrap().plan.is_some());
        }

        #[test]
        fn a_plan_cut_by_closing_the_app_is_not_asked_again_by_itself() {
            use std::sync::atomic::Ordering;
            let mut h = Harness::new();
            // The app does not wait for the answer.
            h.closing_wait = Some(Duration::from_millis(20));
            let app = h.start();
            let project = h.narrated_project(&app);
            let asked = h.text.requests().len();
            h.text.hang.store(true, Ordering::SeqCst);
            let id = app
                .plan_scenes(project.id, false, BudgetConsent::Confirmed)
                .unwrap();
            wait_until("the request is out", || {
                h.text.hanging.load(Ordering::SeqCst)
            });

            // Bardo closes while Claude writes. The request is not given up
            // there: the process ends with it, and it stays noted on the
            // job.
            drop(app);
            assert_eq!(given_up(&h), 0);
            assert!(plan_costs(&h, project.id).is_empty());

            // The next start takes the job up by itself, finds its request
            // noted, and does not send it again: Claude billed what it
            // wrote of the first.
            let app = h.start();
            let job = wait_done(&app, id);
            assert_eq!(job.state(), JobState::Failed);
            let failure = job.failure().unwrap();
            assert_eq!(failure.kind, JobFailureKind::AnswerLost);
            assert!(
                failure.detail.contains("Bardo closed before"),
                "{}",
                failure.detail
            );
            assert_eq!(h.text.requests().len(), asked + 1, "sent once");
            let records = plan_costs(&h, project.id);
            assert_eq!(records.len(), 1, "what it may have cost is in Costs");
            assert!(matches!(records[0].cost, Cost::Estimated(amount) if amount > Money::ZERO));
            assert_eq!(app.scenes(project.id).unwrap().plan, None);

            // Only the user's retry sends it again.
            h.answer(plan_answer());
            app.retry_job(id).unwrap();
            done(&app, id);
            assert_eq!(h.text.requests().len(), asked + 2);
            assert_eq!(plan_costs(&h, project.id).len(), 2);
            assert!(app.scenes(project.id).unwrap().plan.is_some());
            h.text.release.store(true, Ordering::SeqCst);
        }

        #[test]
        fn a_plan_answered_while_the_app_closes_is_saved_and_not_asked_again() {
            let mut h = Harness::new();
            // The app waits for the answer, which comes in time.
            h.closing_wait = Some(PATIENCE);
            let app = h.start();
            let project = h.narrated_project(&app);
            let id = plan_arriving(&h, &app, project.id, plan_answer());
            let asked = h.text.requests().len();
            let probe = app.jobs.probe();

            // The answer arrives once the app has begun to close.
            let text = Arc::clone(&h.text);
            let closing = app.jobs.probe();
            let answer = std::thread::spawn(move || {
                wait_until("the app is closing", || closing.closing());
                assert!(closing.runs(id), "the answer is still on its way");
                text.let_go();
            });
            drop(app);
            answer.join().unwrap();
            assert!(!probe.runs(id), "the app waited for the handler");

            // The answer arrived whole and nobody cancelled: it is the
            // project's plan, and the job finds it there.
            let app = h.start();
            done(&app, id);
            assert_eq!(h.text.requests().len(), asked);
            let plan = app.scenes(project.id).unwrap().plan.expect("planned");
            assert_eq!(plan.generation.job, Some(id));
            assert_eq!(plan_costs(&h, project.id).len(), 1);
        }

        #[test]
        fn a_plan_that_never_left_is_sent_again_by_the_queue() {
            // A failure that says nothing was sent leaves nothing asked:
            // the queue's own retry sends the request.
            let h = Harness::new();
            let app = h.start();
            let project = h.narrated_project(&app);
            let asked = h.text.requests().len();
            *h.text.failure.lock().unwrap() = Some(ProviderFailure::new(
                ProviderFailureKind::Unreachable,
                "host not found",
            ));
            let id = app
                .plan_scenes(project.id, false, BudgetConsent::Confirmed)
                .unwrap();
            let job = wait_done(&app, id);
            assert_eq!(
                job.failure().unwrap().kind,
                JobFailureKind::ProviderUnavailable
            );
            assert_eq!(h.text.requests().len(), asked + 2, "every attempt");
            assert!(plan_costs(&h, project.id).is_empty());
        }

        #[test]
        fn a_scene_job_is_not_retried_beside_another_nor_over_a_newer_plan() {
            let h = Harness::new();
            let app = h.start();
            let project = h.narrated_project(&app);
            // A plan that failed, then one that is still being written.
            *h.text.failure.lock().unwrap() = Some(ProviderFailure::new(
                ProviderFailureKind::Declined,
                "refused",
            ));
            let failed = app
                .plan_scenes(project.id, false, BudgetConsent::Confirmed)
                .unwrap();
            assert_eq!(wait_done(&app, failed).state(), JobState::Failed);
            *h.text.failure.lock().unwrap() = None;
            let asked = h.text.requests().len();
            let newer = plan_arriving(&h, &app, project.id, plan_answer());

            // Retry in Jobs on the older one would send a second request
            // beside the one that is out.
            assert!(matches!(
                app.retry_job(failed),
                Err(JobActionError::ScenesBusy)
            ));
            // And once the newer plan is made, the older job would replace
            // it unasked.
            h.text.let_go();
            done(&app, newer);
            let planned = app.scenes(project.id).unwrap().plan.expect("planned");
            assert!(matches!(
                app.retry_job(failed),
                Err(JobActionError::ScenesMovedOn)
            ));
            assert_eq!(h.text.requests().len(), asked + 1);
            assert_eq!(app.scenes(project.id).unwrap().plan, Some(planned));
        }
    }
}
