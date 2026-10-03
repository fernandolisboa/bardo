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

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, CostPurpose, Generation, GenerationId, ImageGenerator, ImageRequest, Job, JobFailure,
    JobFailureKind, JobId, JobKind, Narration, NarrationId, NarrationRepository, NoPendingClip,
    NoPendingImage, NoSuchScene, ProfileId, Progress, ProjectFiles, Provider, RenderedPrompt,
    RepositoryError, SceneDraft, SceneFieldError, SceneImage, ScenePlan, ScenePlanId,
    ScenePlanRepository, ScenePrompt, SecretStore, TemplateKind, TemplateUsed, TemplateVariable,
    TemplateVersion, TemplateVersionId, TextFormat, TextGenerator, TextRequest, VideoProject,
    VideoProjectId, sentences,
};
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
    /// Generation calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// Scenes or images of the project are being generated.
    #[error("scenes or images are already being generated for this project")]
    Busy,
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
            SceneError::SceneNotFound(_) => Text::SceneNotFound,
            SceneError::MissingKey(Provider::Claude) => Text::ScenesMissingClaudeKey,
            SceneError::MissingKey(Provider::Higgsfield) => Text::ScenesMissingHiggsfieldKey,
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
    /// What drawing one scene again would cost; `None` without a plan.
    pub image_estimate: Option<SpendEstimate>,
    /// The scenes' clips: models, lengths, prices and jobs.
    pub clips: ClipsView,
}

/// Claude's call that plans scenes from `rendered`.
fn plan_call(rendered: &RenderedPrompt) -> PlannedCall {
    PlannedCall::new(Provider::Claude, CostPurpose::ScenePlan, 1)
        .with_prompt(&rendered.instructions, &rendered.prompt)
}

/// Nano Banana's calls that draw `images` images.
fn images_call(images: usize) -> PlannedCall {
    PlannedCall::new(Provider::Gemini, CostPurpose::SceneImage, images as u64)
}

impl ScenesView {
    /// How many images "generate images" draws: the scenes without one.
    pub fn missing_images(&self) -> usize {
        self.plan
            .as_ref()
            .map_or(0, |plan| plan.missing_images().len())
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

/// The shape Claude answers in.
const PLAN_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "scenes": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "first_sentence": {"type": "integer"},
          "prompt": {"type": "string"}
        },
        "required": ["first_sentence", "prompt"],
        "additionalProperties": false
      }
    }
  },
  "required": ["scenes"],
  "additionalProperties": false
}"#;

#[derive(Debug, Deserialize)]
struct PlannedScenes {
    scenes: Vec<PlannedScene>,
}

#[derive(Debug, Deserialize)]
struct PlannedScene {
    first_sentence: i64,
    prompt: String,
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
fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The narration as the planner reads it: one sentence per line, with its
/// number and when it is spoken.
fn sentence_lines(narration: &Narration) -> String {
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
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
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
                job,
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
        let drafts = planned
            .scenes
            .into_iter()
            .filter_map(|scene| {
                Some(SceneDraft {
                    first_sentence: usize::try_from(scene.first_sentence).ok()?,
                    prompt: scene.prompt,
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
        let plan = ScenePlan::from_drafts(generation, &narration, drafts, now)
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

    /// Draws the scenes `job` asked for, saving each image as it arrives.
    /// Without a context (tests replaying a job) nothing reports progress
    /// or stops it.
    fn draw(
        &self,
        payload: &str,
        job: JobId,
        mut cx: Option<&mut JobContext>,
    ) -> Result<(), JobFailure> {
        let payload: ImagesPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let plan_id: ScenePlanId = id(&payload.plan)?;
        let total = payload.scenes.len();
        let mut failed: Vec<JobFailure> = Vec::new();
        let mut key = None;
        for (done, &index) in payload.scenes.iter().enumerate() {
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
            let prompt = scene.prompt().as_str().to_owned();
            let request = ImageRequest {
                prompt: prompt.clone(),
            };
            match self.images.generate(key, &request) {
                Ok(image) => {
                    self.costs.record_for_project(
                        PaidCall {
                            provider: Provider::Gemini,
                            model: &image.model,
                            purpose: CostPurpose::SceneImage,
                            usage: image.usage,
                            job,
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
            if let Some(cx) = cx.as_mut() {
                cx.save_checkpoint(
                    format!("{} of {} scenes", done + 1, total),
                    Progress::of(done as u64 + 1, total as u64),
                )
                .map_err(unexpected)?;
            }
        }
        match failed.first() {
            None => Ok(()),
            Some(first) => Err(JobFailure::new(
                first.kind,
                format!(
                    "{} of {} scenes were not drawn. {}",
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
}

impl Bardo {
    pub(crate) fn scenes_project(&self, id: VideoProjectId) -> Result<VideoProject, SceneError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(SceneError::ProjectNotFound)
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
                Some(self.estimate(&[plan_call(&rendered)])?)
            }
            None => None,
        };
        let missing = plan.as_ref().map_or(0, |plan| plan.missing_images().len());
        let clips = self.clips_view(project.channel, plan.as_ref())?;
        Ok(ScenesView {
            clips,
            narration: narration.as_ref().map(|narration| narration.id),
            stale: plan
                .as_ref()
                .is_some_and(|plan| plan.is_stale(narration.as_ref())),
            images_estimate: match missing {
                0 => None,
                n => Some(self.estimate(&[images_call(n)])?),
            },
            image_estimate: match &plan {
                Some(_) => Some(self.estimate(&[images_call(1)])?),
                None => None,
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
    fn scenes_budget(&self, call: PlannedCall, consent: BudgetConsent) -> Result<(), SceneError> {
        self.check_budget(&[call], consent)?
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
        let template = self.current_template(TemplateKind::ImagePrompt)?;
        let rendered = self.render_plan(&project, &narration, &template)?;
        self.scenes_budget(plan_call(&rendered), consent)?;
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
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        self.scenes_idle(plan.project)?;
        self.require_key(Provider::Gemini)?;
        self.scenes_budget(images_call(scenes.len()), consent)?;
        let payload = ImagesPayload {
            project: plan.project.to_string(),
            plan: plan.id.to_string(),
            scenes,
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
        self.draw_scenes(&plan, missing, consent)
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
        self.draw_scenes(&plan, vec![index], consent)
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
        ChannelDraft, ContentLanguage, Country, JobState, ProviderFailure, ProviderFailureKind,
    };
    use bardo_storage::{Database, MemoryProjectFiles, MemorySecretStore};

    use super::*;
    use crate::testing::{
        FakeClips, FakeDecisionEngine, FakeImages, FakeKeyChecker, FakeMarketData, FakeSpeech,
        FakeTextGenerator, FakeVoiceLibrary, SCENE_IMAGE,
    };
    use crate::{JobSettings, Providers, Repositories};

    const CLAUDE_KEY: &str = "sk-ant-api03-test-key-0001";
    const ELEVENLABS_KEY: &str = "sk_test_elevenlabs_key_0001";
    pub(crate) const GEMINI_KEY: &str = "AIzaSyTest-gemini-key-0001";
    const HIGGSFIELD_KEY: &str = "hf-key-id-0001:hf-key-secret-0001";
    const PATIENCE: Duration = Duration::from_secs(10);
    const SCRIPT: &str = "Era uma vez, em 1969, uma sonda. Ela partiu para longe. \
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
        pub(crate) files: Arc<MemoryProjectFiles>,
        pub(crate) text: Arc<FakeTextGenerator>,
        pub(crate) images: Arc<FakeImages>,
        pub(crate) clips: Arc<FakeClips>,
        /// Video providers after the fake one.
        pub(crate) more_clips: Vec<Arc<dyn bardo_domain::ClipGenerator>>,
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
        /// App credentials and network tokens.
        pub(crate) connection_secrets: Arc<MemorySecretStore>,
    }

    impl Harness {
        pub(crate) fn new() -> Self {
            let files: Arc<MemoryProjectFiles> = Arc::default();
            Self {
                db: Arc::new(Database::open_in_memory().unwrap()),
                media: Arc::new(crate::editor::testing::FakeMedia::writing_to(Arc::clone(
                    &files,
                ))),
                audio: Arc::default(),
                files,
                text: Arc::default(),
                images: Arc::default(),
                clips: Arc::default(),
                more_clips: Vec::new(),
                secrets: Arc::default(),
                stats: Arc::default(),
                decisions: Arc::default(),
                speech: Arc::default(),
                uploader: Arc::default(),
                connection_secrets: Arc::default(),
            }
        }

        pub(crate) fn start(&self) -> Bardo {
            self.start_from(Repositories::shared_with_files(
                Arc::clone(&self.db),
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
                    Arc::clone(&self.db),
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
                    Arc::clone(&self.db),
                    Arc::clone(&self.secrets) as _,
                    Arc::clone(&self.files) as _,
                )
            })
        }

        fn start_from(&self, repositories: Repositories) -> Bardo {
            let repositories = Repositories {
                connection_secrets: Arc::clone(&self.connection_secrets) as _,
                ..repositories
            };
            let providers = Providers {
                key_checker: Arc::new(FakeKeyChecker::default()),
                market_data: Arc::new(FakeMarketData::default()),
                video_stats: Arc::clone(&self.stats) as _,
                text: Arc::clone(&self.text) as _,
                decisions: Arc::clone(&self.decisions) as _,
                voices: Arc::new(FakeVoiceLibrary::default()),
                speech: Arc::clone(&self.speech) as _,
                aligner: Arc::new(crate::narration_import::testing::FakeAligner::default()),
                images: Arc::clone(&self.images) as _,
                clips: std::iter::once(Arc::clone(&self.clips) as _)
                    .chain(self.more_clips.iter().cloned())
                    .collect(),
                audio: Arc::clone(&self.audio) as _,
                media: Arc::clone(&self.media) as _,
                sign_ins: Vec::new(),
                consent: Arc::new(crate::connections::testing::NoConsent),
                uploaders: vec![Arc::clone(&self.uploader) as _],
            };
            let mut app = Bardo::start_with(
                repositories,
                providers,
                Some("en-US"),
                JobSettings {
                    retry: bardo_domain::RetryPolicy {
                        max_attempts: 2,
                        first_delay: Duration::from_millis(20),
                        max_delay: Duration::from_millis(20),
                    },
                    ..JobSettings::default()
                },
            )
            .unwrap();
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

    /// A channel whose default persona is the documentary narrator, and a
    /// project on one of its themes.
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

        let prompts: Vec<_> = planned
            .scenes()
            .iter()
            .map(|scene| scene.prompt().as_str().to_owned())
            .collect();
        assert_eq!(h.images.prompts(), prompts);
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        for scene in plan.scenes() {
            let image = scene.image().unwrap();
            assert_eq!(scene.pending(), None);
            assert_eq!(scene.failure(), None);
            assert!(image.file.starts_with("scene-") && image.file.ends_with(".png"));
            assert_eq!(h.files.read(project.id, &image.file).unwrap(), SCENE_IMAGE);
            let generation = &image.generation;
            assert_eq!(generation.provider, Provider::Gemini);
            assert_eq!(generation.model, "nano-banana-fake");
            assert_eq!(generation.prompt, scene.prompt().as_str());
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
            failure.detail.contains("1 of 3 scenes"),
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

        assert_eq!(
            h.images.prompts()[asked..],
            ["An empty control room at night"]
        );
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
