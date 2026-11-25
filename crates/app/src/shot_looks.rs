//! How an ad's product shots look (#285, ADR-0041): the layout the
//! product's image is set in, where it sits and what is behind it. Every
//! change is composed again by Bardo, on the computer, for free.
//!
//! The only paid part is a drawn backdrop: the image generator draws an
//! empty surface or set in the video's style, with no product, text or
//! logo and no reference image (the product's images never go to a
//! model). It costs one scene image, is estimated and checked against the
//! video's budget before it starts, and waits for the user's review. A
//! kept backdrop may then be shown by every composed scene of the
//! project, at no cost.

use std::time::SystemTime;

use bardo_domain::{
    ApiKey, BackdropId, CostPurpose, ImageRequest, Job, JobFailure, JobFailureKind, JobId, JobKind,
    Provider, ShotBackdropDrawing, ShotLook, VideoProjectId,
};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, PaidCall, PlannedCall};
use crate::scenes::{SceneError, SceneHandler, unexpected};
use crate::{Bardo, KeyState, Text};

/// The image provider's call that draws one backdrop.
pub(crate) fn backdrop_call() -> PlannedCall {
    PlannedCall::new(Provider::Gemini, CostPurpose::ShotBackdrop, 1)
}

/// What every backdrop is asked to be, whatever the product. The product
/// is named only by its kind, so nothing the user typed about it reaches
/// the image generator.
fn backdrop_prompt(kind: &str) -> String {
    format!(
        "An empty surface or set for a product photograph of {kind}, as the backdrop of an ad. \
The middle and the lower half are open and uncluttered, with soft, even light. Show no \
product, no object in the foreground, no person, no text, no letters, no logo and no \
watermark."
    )
}

/// The drawn backdrop's file in the project folder.
fn backdrop_file(job: JobId, extension: &str) -> String {
    format!("backdrop-{job}.{extension}")
}

/// What a backdrop job draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BackdropPayload {
    pub(crate) project: String,
    /// The prompt, in the video's style.
    pub(crate) prompt: String,
    /// The frame's shape it is drawn in.
    pub(crate) aspect: String,
}

impl SceneHandler {
    fn gemini_key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::Gemini)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no gemini key is saved"))
    }

    /// Draws the backdrop `job` asks for and adds it to the project's,
    /// waiting for the user's review. Run again, a job that already drew
    /// its backdrop draws nothing.
    pub(crate) fn draw_backdrop(&self, payload: &str, job: JobId) -> Result<(), JobFailure> {
        let payload: BackdropPayload = serde_json::from_str(payload)
            .map_err(|e| unexpected(format!("bad backdrop payload: {e}")))?;
        let project = VideoProjectId::from(
            uuid::Uuid::parse_str(&payload.project)
                .map_err(|_| unexpected(format!("bad project id {}", payload.project)))?,
        );
        let aspect = payload
            .aspect
            .parse::<bardo_domain::AspectRatio>()
            .map_err(|_| unexpected(format!("unknown frame shape {}", payload.aspect)))?;
        let drawn = self.plans.shot_backdrops(project).map_err(unexpected)?;
        if drawn.iter().any(|backdrop| backdrop.job == Some(job)) {
            return Ok(());
        }
        let key = self.gemini_key()?;
        // No reference: no image of the product goes to a model.
        let request = ImageRequest {
            prompt: payload.prompt.clone(),
            shape: aspect.into(),
            references: Vec::new(),
        };
        let drawn = {
            let _paying = self
                .costs
                .paying_for_project(job, &backdrop_call(), project)?;
            self.images.generate(&key, &request)
        };
        let image = drawn.map_err(|failure| {
            // An answer that was lost may still have been billed.
            self.costs
                .record_failed_for_project(&failure, &backdrop_call(), Some(job), project);
            JobFailure::new(failure.kind.into(), format!("Gemini: {}", failure.detail))
        })?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Gemini,
                model: &image.model,
                purpose: CostPurpose::ShotBackdrop,
                usage: image.usage,
                job: Some(job),
                reported: None,
            },
            project,
        );
        let file = backdrop_file(job, image.format.extension());
        self.files
            .write(project, &file, &image.bytes)
            .map_err(unexpected)?;
        self.plans
            .add_shot_backdrop(&ShotBackdropDrawing {
                id: BackdropId::new(),
                project,
                file,
                prompt: payload.prompt,
                provider: Provider::Gemini,
                model: image.model,
                job: Some(job),
                drawn_at: crate::publications::whole_millis(SystemTime::now()),
                kept: false,
            })
            .map_err(unexpected)
    }
}

impl Bardo {
    /// Whether a backdrop is being drawn for the project, counting a
    /// cancelled job that is still stopping.
    pub(crate) fn drawing_backdrop(&self, project: VideoProjectId) -> bool {
        self.work_busy(&crate::jobs::Work::Backdrop(project.to_string()))
    }

    /// What drawing a backdrop for `project` would spend.
    pub(crate) fn backdrop_estimate(
        &self,
    ) -> Result<crate::SpendEstimate, bardo_domain::RepositoryError> {
        self.estimate(&[backdrop_call()])
    }

    /// Makes the product shot of scene `index` look as `look` says, and
    /// composes it again in a job, for free. A drawn backdrop must be one
    /// of the project's that the user kept. Returns the job, or `None`
    /// when nothing changed.
    pub fn set_shot_look(
        &self,
        project: VideoProjectId,
        index: usize,
        look: ShotLook,
    ) -> Result<Option<JobId>, SceneError> {
        // A job saving a scene meanwhile would be written over.
        self.scenes_idle(project)?;
        if self.clips_running(project) {
            return Err(SceneError::Busy);
        }
        if let Some(drawn) = look.drawn_backdrop() {
            let kept = self
                .scene_plans
                .shot_backdrops(project)?
                .into_iter()
                .any(|backdrop| backdrop.id == drawn && backdrop.kept);
            if !kept {
                return Err(SceneError::NoSuchBackdrop);
            }
        }
        let mut plan = self.own_plan(project)?;
        let project = self.scenes_project(project)?;
        if !plan.set_shot_look(index, look, SystemTime::now())? {
            return Ok(None);
        }
        self.scene_plans.save_scene(&plan, index)?;
        Ok(Some(self.compose_scenes(&plan, &project, vec![index])?))
    }

    /// Starts a job in which the image generator draws a backdrop for the
    /// project's product shots, in the video's style and shape. It costs
    /// one scene image; past a budget or the video's cap it needs
    /// `consent`. The backdrop waits for the user's review.
    pub fn draw_shot_backdrop(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let project = self.scenes_project(project)?;
        let Some(brief) = self.script_brief(&project)? else {
            return Err(SceneError::Kind(bardo_domain::SceneKindError::NotComposed));
        };
        if self.drawing_backdrop(project.id) {
            return Err(SceneError::Busy);
        }
        if self.provider_key(Provider::Gemini).state == KeyState::NotSet {
            return Err(SceneError::MissingKey(Provider::Gemini));
        }
        if let Err(estimate) = self.check_video_budget(project.id, &[backdrop_call()], consent)? {
            return Err(SceneError::OverBudget(estimate));
        }
        let english = crate::Catalog::load(bardo_domain::UiLanguage::EnUs);
        let kind = english.get(Text::ProductKindName(brief.product.brief.kind()));
        let payload = BackdropPayload {
            project: project.id.to_string(),
            // In the video's style (#259), as its scenes are drawn.
            prompt: self
                .video_style(&project)?
                .applied(&backdrop_prompt(&kind.to_lowercase())),
            aspect: self.video_format(&project)?.aspect().code().to_owned(),
        };
        let payload = serde_json::to_string(&payload).expect("a backdrop payload serializes");
        let job = Job::new(self.profile.id, JobKind::ShotBackdrop, payload);
        Ok(self.jobs.enqueue(job)?)
    }

    /// Keeps a drawn backdrop the user looked at, and shows it behind the
    /// product shot of scene `index` when one is named (composed again,
    /// for free).
    pub fn keep_shot_backdrop(
        &self,
        project: VideoProjectId,
        backdrop: BackdropId,
        index: Option<usize>,
    ) -> Result<Option<JobId>, SceneError> {
        let project = self.scenes_project(project)?;
        let found = self
            .scene_plans
            .shot_backdrops(project.id)?
            .into_iter()
            .any(|drawn| drawn.id == backdrop);
        if !found {
            return Err(SceneError::NoSuchBackdrop);
        }
        self.scene_plans.keep_shot_backdrop(backdrop)?;
        let Some(index) = index else {
            return Ok(None);
        };
        let plan = self.own_plan(project.id)?;
        let look = plan.scene(index)?.look();
        self.set_shot_look(
            project.id,
            index,
            ShotLook {
                backdrop: bardo_domain::ShotBackdrop::Drawn(backdrop),
                ..look
            },
        )
    }

    /// Discards a drawn backdrop the user does not want. A kept backdrop
    /// that a scene still shows stays.
    pub fn discard_shot_backdrop(
        &self,
        project: VideoProjectId,
        backdrop: BackdropId,
    ) -> Result<(), SceneError> {
        let project = self.scenes_project(project)?;
        let Some(drawn) = self
            .scene_plans
            .shot_backdrops(project.id)?
            .into_iter()
            .find(|drawn| drawn.id == backdrop)
        else {
            return Err(SceneError::NoSuchBackdrop);
        };
        let shown = self
            .scene_plans
            .scene_plan(project.id)?
            .is_some_and(|plan| {
                plan.scenes()
                    .iter()
                    .any(|scene| scene.look().drawn_backdrop() == Some(backdrop))
            });
        if shown {
            return Err(SceneError::BackdropInUse);
        }
        self.scene_plans.remove_shot_backdrop(backdrop)?;
        crate::proxies::remove_media(self.files.as_ref(), project.id, &drawn.file);
        Ok(())
    }

    /// The backdrop a failed or cancelled backdrop job still has to pay
    /// for when run again: none once it drew one.
    pub(crate) fn backdrop_left(&self, job: &Job) -> Option<(VideoProjectId, Vec<PlannedCall>)> {
        if job.kind() != JobKind::ShotBackdrop {
            return None;
        }
        let payload: BackdropPayload = serde_json::from_str(job.payload()).ok()?;
        let project = VideoProjectId::from(uuid::Uuid::parse_str(&payload.project).ok()?);
        let drawn = self
            .scene_plans
            .shot_backdrops(project)
            .ok()?
            .iter()
            .any(|backdrop| backdrop.job == Some(job.id()));
        Some((
            project,
            if drawn {
                Vec::new()
            } else {
                vec![backdrop_call()]
            },
        ))
    }
}

impl Bardo {
    /// Where a drawn backdrop's file is, to show it.
    pub fn shot_backdrop_path(
        &self,
        project: VideoProjectId,
        backdrop: &ShotBackdropDrawing,
    ) -> std::path::PathBuf {
        self.files.path(project, &backdrop.file)
    }
}
