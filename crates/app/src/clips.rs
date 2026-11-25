//! Video clip use cases (PRD stories 9, 37-39): a video provider animates a
//! scene's image into a short clip, moved by the scene's motion prompt (the
//! image prompt unless the user writes one). A channel picks the model its
//! clips use; any scene can pick another. The clip is as long as the scene
//! is narrated, within what the model allows. A scene without a clip keeps
//! its still image.
//!
//! Clips are made by a job per run. It hands each scene's image to the
//! provider, submits all of them, then polls them together and saves each
//! clip as it is ready. The provider's request ids are saved before any
//! waiting, so a restart (or a retry of a cancelled job) polls the same
//! requests instead of paying for them twice. A scene that fails does not
//! stop the others, and retrying the job submits only the scenes without a
//! clip from it. A new clip waits beside the current one until the user
//! accepts it, like a new image.
//!
//! Cancelling a run stops the waiting, not the provider: a request already
//! submitted may still finish and be billed. Retrying the cancelled job
//! picks it up.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, ChannelId, ClipGenerator, ClipHandle, ClipImage, ClipModel, ClipModelRef, ClipRequest,
    ClipStatus, ClipSubmission, CostPurpose, Generation, GenerationId, ImageFormat, Job,
    JobFailure, JobFailureKind, JobId, JobKind, Metered, Money, ProductionMode, ProfileId,
    Progress, ProjectFiles, Provider, ProviderFailure, ProviderFailureKind, RepositoryError, Scene,
    SceneClip, ScenePlan, ScenePlanId, ScenePlanRepository, ScenePrompt, SecretStore, StagedImage,
    TokenUsage, VideoProject, VideoProjectId,
};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::scenes::{id, parse, to_json, unexpected};
use crate::{Bardo, KeyState, SceneError};

/// How many rounds of status checks in a row may all fail on the
/// provider's side before the job gives up for now (the queue retries it
/// later, polling the same requests).
const MAX_HICCUPS: u32 = 10;

/// How long after sending a submission whose answer was lost the job
/// keeps looking for it before saying it may have started unseen: the
/// provider's library can take a moment to list a new clip.
/// Tests look at once.
fn lookup_grace_secs() -> u64 {
    if cfg!(test) { 0 } else { 120 }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The clips part of a project's scenes panel.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipsView {
    /// Every model the user can pick, each provider's default first.
    pub models: Vec<ClipModel>,
    /// The model of scenes without their own: the production mode's, else
    /// the channel's, else the provider's default. `None` when no provider
    /// is set up.
    pub default_model: Option<ClipModel>,
    /// The video's production mode (#248), which says which scenes
    /// "animate missing" animates.
    pub mode: ProductionMode,
    /// One per scene of the plan, in order.
    pub scenes: Vec<SceneClipView>,
    /// The scenes "animate missing" would animate: the ones the mode
    /// animates with an image, no clip and no clip being made.
    pub missing: Vec<usize>,
    /// What animating them would cost; `None` when none is missing.
    pub missing_estimate: Option<SpendEstimate>,
}

/// One scene's clip settings and work.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneClipView {
    /// The model its next clip uses; `None` when the model it picked is no
    /// longer offered.
    pub model: Option<ClipModel>,
    /// How long its next clip would be.
    pub seconds: Option<u32>,
    /// What its next clip would cost at the rate table's price; `None`
    /// when no rate covers the model.
    pub price: Option<Money>,
    /// The latest clip job that animates it.
    pub job: Option<Job>,
}

impl SceneClipView {
    /// Whether a clip of the scene is being made.
    pub fn is_busy(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }
}

/// Which scenes a clip job animates, with the model each uses, fixed when
/// the job is queued (the budget was checked against it). The motion prompt
/// and length are read when each scene is submitted, so an edit made before
/// a retry is what gets animated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ClipsPayload {
    project: String,
    plan: String,
    scenes: Vec<ClipOrder>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ClipOrder {
    scene: usize,
    provider: String,
    model: String,
}

/// What a clip job sent, saved as the job's external handle after every
/// change: a restart reads it to poll instead of submitting again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ClipsState {
    scenes: Vec<ClipRun>,
}

/// One scene's submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ClipRun {
    scene: usize,
    /// Counts the scene's submissions in this job, from 1. Part of the
    /// idempotency key: resending the same submission (after a crash right
    /// after sending it) returns the first one instead of a second clip,
    /// and a failed submission gets a new number.
    submission: u32,
    prompt: String,
    seconds: u32,
    /// The image animated: its generation and file.
    source_image: String,
    image_file: String,
    /// The image as the provider refers to it, once handed over.
    staged: Option<String>,
    /// The provider's request id, once submitted.
    request: Option<String>,
    /// What the provider said the clip costs, in micro-dollars.
    quote_micros: Option<u64>,
    /// For a provider that would start a second clip if sent the same
    /// submission again: when it was sent without an answer yet (Unix
    /// seconds). Written before sending, so after a lost answer or a
    /// crash the job looks for the clip instead of paying twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sent_at: Option<u64>,
}

impl ClipRun {
    /// Forgets the submission, so the next attempt starts a new one.
    fn restart(&mut self) {
        self.submission += 1;
        self.staged = None;
        self.request = None;
        self.quote_micros = None;
        self.sent_at = None;
    }
}

/// Where one scene stands after a step.
enum Step {
    /// Submitted, not ready yet.
    Waiting,
    /// Submitted, but the provider did not say how it is going.
    Hiccup(JobFailure),
    /// Could not be submitted for now (the provider is busy or down).
    NotSubmitted(JobFailure),
    /// The clip is saved.
    Saved,
    /// It failed; the others go on.
    Failed(JobFailure),
    /// The job was told to stop before the scene was sent: nothing was
    /// started, and what the scene had reached is saved.
    Stopped,
}

/// The clip file's name in the project folder.
fn clip_file(generation: GenerationId) -> String {
    format!("clip-{generation}.mp4")
}

/// The format of a scene image, from its file name.
fn image_format(file: &str) -> Option<ImageFormat> {
    let extension = Path::new(file).extension()?.to_str()?;
    [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Webp]
        .into_iter()
        .find(|format| format.extension().eq_ignore_ascii_case(extension))
}

/// Failures about one scene (its prompt, its image, its request): the job
/// goes on with the others. Any other failure (key, credits) would hit
/// every scene, so the job stops.
fn is_about_the_scene(kind: JobFailureKind) -> bool {
    matches!(
        kind,
        JobFailureKind::Declined | JobFailureKind::UnexpectedAnswer | JobFailureKind::NotAllowed
    )
}

/// Whether the scene has a clip, current or waiting, made by `job`.
fn animated_by(scene: &Scene, job: JobId) -> bool {
    scene
        .clip()
        .into_iter()
        .chain(scene.pending_clip())
        .any(|clip| clip.generation.job == Some(job))
}

/// Why a plan gave no access token, as a job failure: signed out or
/// refused is a missing key, the provider unreachable or busy passes.
fn plan_failure(provider: Provider, error: &crate::PlanError) -> JobFailure {
    use crate::PlanError;
    let kind = match error {
        PlanError::NotConnected(_) | PlanError::ReconnectNeeded(_) => JobFailureKind::MissingKey,
        error if error.is_transient() => JobFailureKind::ProviderUnavailable,
        _ => JobFailureKind::Unexpected,
    };
    let detail = match error {
        PlanError::NotConnected(_) => format!("not signed in to the {provider} plan"),
        PlanError::ReconnectNeeded(_) => format!("sign in to the {provider} plan again"),
        error => format!("could not renew the {provider} plan's sign-in: {error}"),
    };
    JobFailure::new(kind, detail)
}

/// Runs clip jobs.
pub(crate) struct ClipHandler {
    pub(crate) owner: ProfileId,
    pub(crate) plans: Arc<dyn ScenePlanRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) generators: Vec<Arc<dyn ClipGenerator>>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    /// Plans signed in to instead of a key (ADR-0028).
    pub(crate) plan_sessions: crate::plans::PlanSessions,
    pub(crate) costs: CostBook,
}

/// One clip job's run.
struct ClipJob<'a> {
    handler: &'a ClipHandler,
    job: JobId,
    project: VideoProjectId,
    plan: ScenePlanId,
    state: ClipsState,
    keys: Vec<(Provider, ApiKey)>,
    /// A plan's token was renewed after the provider refused it, and no
    /// call has worked since: a second refusal is final.
    renewed: bool,
}

impl JobHandler for ClipHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: ClipsPayload = parse(payload)?;
        let state = match cx.external_handle() {
            Some(saved) => parse(saved)?,
            None => ClipsState::default(),
        };
        let mut run = ClipJob {
            handler: self,
            job: cx.id(),
            project: id(&payload.project)?,
            plan: id(&payload.plan)?,
            state,
            keys: Vec::new(),
            renewed: false,
        };
        run.run(&payload.scenes, cx)
    }
}

impl ClipHandler {
    fn generator(&self, provider: Provider) -> Option<&Arc<dyn ClipGenerator>> {
        self.generators
            .iter()
            .find(|generator| generator.provider() == provider)
    }
}

impl ClipJob<'_> {
    fn run(&mut self, orders: &[ClipOrder], cx: &mut JobContext) -> Result<(), JobFailure> {
        let total = orders.len() as u64;
        let mut failed: Vec<(usize, JobFailure)> = Vec::new();
        let mut polls = 0;
        let mut hiccups = 0;
        loop {
            if cx.should_stop() {
                return Ok(());
            }
            // A new plan replaced this one: its scenes are gone.
            let Some(plan) = self.saved_plan()? else {
                return Ok(());
            };
            let (mut done, mut waiting) = (0, 0);
            let (mut answered, mut hiccup) = (false, None);
            let mut not_submitted = None;
            let mut delay = None;
            for order in orders {
                if failed.iter().any(|(scene, _)| *scene == order.scene) {
                    continue;
                }
                let Ok(scene) = plan.scene(order.scene) else {
                    done += 1;
                    continue;
                };
                if animated_by(scene, self.job) {
                    done += 1;
                    continue;
                }
                if cx.should_stop() {
                    return Ok(());
                }
                let generator = self.generator(order)?;
                delay.get_or_insert_with(|| generator.poll_delay(polls));
                match self.step(order, scene, &generator, cx)? {
                    Step::Waiting => {
                        waiting += 1;
                        answered = true;
                    }
                    Step::Hiccup(failure) => {
                        waiting += 1;
                        hiccup = Some(failure);
                    }
                    Step::NotSubmitted(failure) => not_submitted = Some(failure),
                    Step::Saved => {
                        answered = true;
                        done += 1;
                        cx.save_checkpoint(
                            format!("{done} of {total} clips"),
                            Progress::of(done, total),
                        )
                        .map_err(unexpected)?;
                    }
                    Step::Failed(failure) => {
                        answered = true;
                        failed.push((order.scene, failure));
                    }
                    Step::Stopped => return Ok(()),
                }
            }
            if waiting == 0 {
                // Nothing in flight: a scene the provider would not take
                // yet waits for the queue's retry.
                if let Some(failure) = not_submitted {
                    return Err(failure);
                }
                break;
            }
            match hiccup {
                Some(failure) if !answered => {
                    hiccups += 1;
                    if hiccups >= MAX_HICCUPS {
                        return Err(failure);
                    }
                }
                _ => hiccups = 0,
            }
            if !cx.sleep(delay.unwrap_or_default()) {
                return Ok(());
            }
            polls += 1;
        }
        match failed.first() {
            None => Ok(()),
            Some((_, first)) => Err(JobFailure::new(
                first.kind,
                format!(
                    "{} of {} clips were not made. {}",
                    failed.len(),
                    total,
                    failed
                        .iter()
                        .map(|(_, failure)| failure.detail.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
            )),
        }
    }

    fn generator(&self, order: &ClipOrder) -> Result<Arc<dyn ClipGenerator>, JobFailure> {
        let provider: Provider = order.provider.parse().map_err(unexpected)?;
        self.handler
            .generator(provider)
            .cloned()
            .ok_or_else(|| JobFailure::unexpected(format!("{provider} does not make clips")))
    }

    /// The provider's key. A plan's access token is asked for on every
    /// step instead of kept: it expires within the hour and is renewed
    /// while clips run.
    fn key(&mut self, provider: Provider) -> Result<ApiKey, JobFailure> {
        if provider.signs_in() {
            return self
                .handler
                .plan_sessions
                .access_token(provider)
                .map_err(|error| plan_failure(provider, &error));
        }
        if let Some((_, key)) = self.keys.iter().find(|(p, _)| *p == provider) {
            return Ok(key.clone());
        }
        let key = self
            .handler
            .secrets
            .get(self.handler.owner, provider)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(
                    JobFailureKind::MissingKey,
                    format!("no {provider} key is saved"),
                )
            })?;
        self.keys.push((provider, key.clone()));
        Ok(key)
    }

    /// The saved plan, if it is still this job's.
    fn saved_plan(&self) -> Result<Option<ScenePlan>, JobFailure> {
        Ok(self
            .handler
            .plans
            .scene_plan(self.project)
            .map_err(unexpected)?
            .filter(|saved| saved.id == self.plan))
    }

    fn save_state(&self, cx: &mut JobContext) -> Result<(), JobFailure> {
        cx.save_external_handle(to_json(&self.state))
            .map_err(unexpected)
    }

    fn run_of(&mut self, scene: usize) -> Option<&mut ClipRun> {
        self.state.scenes.iter_mut().find(|run| run.scene == scene)
    }

    /// Moves one scene along: hands its image over and submits it, or asks
    /// how its request is going and saves the clip when it is ready.
    /// `Err` stops the whole job.
    fn step(
        &mut self,
        order: &ClipOrder,
        scene: &Scene,
        generator: &Arc<dyn ClipGenerator>,
        cx: &mut JobContext,
    ) -> Result<Step, JobFailure> {
        let mut key = self.key(generator.provider())?;
        let submitted = self.run_of(order.scene).and_then(|run| run.request.clone());
        let Some(request) = submitted else {
            return self.submit(order, scene, generator, &mut key, cx);
        };
        let handle = ClipHandle(request);
        match self.call(generator, &mut key, |key| generator.status(key, &handle)) {
            Ok(ClipStatus::Queued | ClipStatus::Running) => Ok(Step::Waiting),
            Ok(ClipStatus::Done { video }) => {
                match self.call(generator, &mut key, |key| generator.download(key, &video)) {
                    Ok(clip) => {
                        self.save_clip(order, &clip.bytes)?;
                        Ok(Step::Saved)
                    }
                    Err(failure) if JobFailureKind::from(failure.kind).is_transient() => {
                        Ok(Step::Hiccup(self.failure(order, failure)))
                    }
                    Err(failure) => self.scene_failed(order, failure, cx),
                }
            }
            Ok(ClipStatus::Failed(failure)) => self.scene_failed(order, failure, cx),
            Err(failure) if JobFailureKind::from(failure.kind).is_transient() => {
                Ok(Step::Hiccup(self.failure(order, failure)))
            }
            Err(failure) => Err(self.failure(order, failure)),
        }
    }

    /// Calls the provider. A token a plan's provider refuses is renewed
    /// and the call made once more: a refusal started nothing.
    fn call<R>(
        &mut self,
        generator: &Arc<dyn ClipGenerator>,
        key: &mut ApiKey,
        call: impl Fn(&ApiKey) -> Result<R, ProviderFailure>,
    ) -> Result<R, ProviderFailure> {
        let provider = generator.provider();
        let answer = match call(key) {
            Err(failure)
                if failure.kind == ProviderFailureKind::Rejected
                    && provider.signs_in()
                    && !self.renewed =>
            {
                self.renewed = true;
                self.handler.plan_sessions.renew(provider);
                match self.key(provider) {
                    Ok(fresh) => {
                        *key = fresh;
                        call(key)
                    }
                    Err(_) => Err(failure),
                }
            }
            answer => answer,
        };
        if answer.is_ok() {
            self.renewed = false;
        }
        answer
    }

    /// Hands the scene's image over (once per submission) and submits it.
    /// The job may be told to stop while a call is on its way (cancelled,
    /// or the app closing): it looks again right before the call that
    /// starts, and charges for, the clip.
    fn submit(
        &mut self,
        order: &ClipOrder,
        scene: &Scene,
        generator: &Arc<dyn ClipGenerator>,
        key: &mut ApiKey,
        cx: &mut JobContext,
    ) -> Result<Step, JobFailure> {
        let staged = self.run_of(order.scene).and_then(|run| run.staged.clone());
        let staged = match staged {
            Some(staged) => staged,
            None => match self.stage(order, scene, generator, key, cx)? {
                Ok(staged) => staged,
                Err(step) => return Ok(step),
            },
        };
        let run = self
            .run_of(order.scene)
            .expect("staging adds the scene's run")
            .clone();
        let format = image_format(&run.image_file)
            .ok_or_else(|| unexpected(format!("{} is not an image", run.image_file)))?;
        let bytes = self
            .handler
            .files
            .read(self.project, &run.image_file)
            .map_err(unexpected)?;
        let first_frame = ClipImage { bytes, format };
        let request = ClipRequest {
            model: order.model.clone(),
            prompt: run.prompt.clone(),
            image: StagedImage(staged),
            aspect: first_frame.aspect(),
            first_frame,
            seconds: run.seconds,
        };
        let submission = format!("{}-{}-{}", self.job, order.scene, run.submission);
        if !generator.dedupes_submissions() {
            return self.submit_once(order, generator, key, &request, &submission, cx);
        }
        if cx.should_stop() {
            return Ok(Step::Stopped);
        }
        match self.call(generator, key, |key| {
            generator.submit(key, &request, &submission)
        }) {
            Ok(submitted) => self.submitted(order, submitted, cx),
            Err(failure) => self.submit_failed(order, failure, cx),
        }
    }

    /// Submits to a provider that would start (and charge for) a second
    /// clip if sent the same submission again. The job writes down that it
    /// sends before sending; with that note and no answer it looks for the
    /// clip instead, and never sends again without knowing.
    fn submit_once(
        &mut self,
        order: &ClipOrder,
        generator: &Arc<dyn ClipGenerator>,
        key: &mut ApiKey,
        request: &ClipRequest,
        submission: &str,
        cx: &mut JobContext,
    ) -> Result<Step, JobFailure> {
        let provider = generator.provider();
        if let Some(sent_at) = self.run_of(order.scene).and_then(|run| run.sent_at) {
            return match self.call(generator, key, |key| generator.find_submitted(key, request)) {
                Ok(Some(handle)) => {
                    tracing::info!(scene = order.scene, "found the clip a lost answer started");
                    self.submitted(
                        order,
                        ClipSubmission {
                            handle,
                            quote: None,
                        },
                        cx,
                    )
                }
                Ok(None) if now_secs().saturating_sub(sent_at) < lookup_grace_secs() => {
                    Ok(Step::NotSubmitted(self.failure(
                        order,
                        ProviderFailure::new(
                            ProviderFailureKind::ProviderDown,
                            format!(
                                "waiting for {provider} to list the clip sent without an answer"
                            ),
                        ),
                    )))
                }
                Ok(None) => self.scene_failed(
                    order,
                    ProviderFailure::new(
                        ProviderFailureKind::Unexpected,
                        format!(
                            "{provider} did not answer the submission and does not list it; \
                             it may still have started the clip, so check your {provider} \
                             library before animating this scene again"
                        ),
                    ),
                    cx,
                ),
                Err(failure) if JobFailureKind::from(failure.kind).is_transient() => {
                    Ok(Step::NotSubmitted(self.failure(order, failure)))
                }
                // Stops with the note kept: the next attempt looks again.
                Err(failure) => Err(self.failure(order, failure)),
            };
        }
        if let Err(failure) = self.call(generator, key, |key| generator.check_submit(key, request))
        {
            return self.submit_failed(order, failure, cx);
        }
        if cx.should_stop() {
            return Ok(Step::Stopped);
        }
        self.run_of(order.scene)
            .expect("staging adds the scene's run")
            .sent_at = Some(now_secs());
        self.save_state(cx)?;
        match self.call(generator, key, |key| {
            generator.submit(key, request, submission)
        }) {
            Ok(submitted) => self.submitted(order, submitted, cx),
            // It may have started: the note stays for the lookup.
            Err(failure) if failure.kind == ProviderFailureKind::Unreachable => {
                Ok(Step::NotSubmitted(self.failure(order, failure)))
            }
            Err(failure) => {
                self.run_of(order.scene)
                    .expect("the scene's run exists")
                    .sent_at = None;
                self.save_state(cx)?;
                self.submit_failed(order, failure, cx)
            }
        }
    }

    /// Keeps the provider's handle: from now on the job asks how it goes.
    fn submitted(
        &mut self,
        order: &ClipOrder,
        submitted: ClipSubmission,
        cx: &mut JobContext,
    ) -> Result<Step, JobFailure> {
        let run = self.run_of(order.scene).expect("the scene's run exists");
        run.request = Some(submitted.handle.0);
        run.quote_micros = submitted.quote.map(Money::micros);
        run.sent_at = None;
        self.save_state(cx)?;
        Ok(Step::Waiting)
    }

    /// Reads the scene as it is now (image, motion prompt, length) and
    /// hands its image to the provider. The inner `Err` is the scene's
    /// step when that did not work.
    fn stage(
        &mut self,
        order: &ClipOrder,
        scene: &Scene,
        generator: &Arc<dyn ClipGenerator>,
        key: &mut ApiKey,
        cx: &mut JobContext,
    ) -> Result<Result<String, Step>, JobFailure> {
        let Some(image) = scene.image() else {
            let failure = ProviderFailure::new(
                ProviderFailureKind::Unexpected,
                "the scene has no image to animate",
            );
            return self.scene_failed(order, failure, cx).map(Err);
        };
        let model = generator
            .models()
            .into_iter()
            .find(|model| model.id.model() == order.model);
        let Some(model) = model else {
            let failure = ProviderFailure::new(
                ProviderFailureKind::NotAllowed,
                format!("the model {} is no longer offered", order.model),
            );
            return self.scene_failed(order, failure, cx).map(Err);
        };
        let format = image_format(&image.file)
            .ok_or_else(|| unexpected(format!("{} is not an image", image.file)))?;
        let bytes = self
            .handler
            .files
            .read(self.project, &image.file)
            .map_err(unexpected)?;
        let first_frame = ClipImage { bytes, format };
        let staged = match self.call(generator, key, |key| {
            generator.stage_image(key, &first_frame)
        }) {
            Ok(staged) => staged.0,
            Err(failure) => return self.submit_failed(order, failure, cx).map(Err),
        };
        let prompt = scene.motion_prompt().as_str().to_owned();
        let seconds = model.durations.fit(scene.duration());
        let (source_image, image_file) = (image.generation.id.to_string(), image.file.clone());
        match self.run_of(order.scene) {
            Some(run) => {
                run.prompt = prompt;
                run.seconds = seconds;
                run.source_image = source_image;
                run.image_file = image_file;
                run.staged = Some(staged.clone());
            }
            None => self.state.scenes.push(ClipRun {
                scene: order.scene,
                submission: 1,
                prompt,
                seconds,
                source_image,
                image_file,
                staged: Some(staged.clone()),
                request: None,
                quote_micros: None,
                sent_at: None,
            }),
        }
        self.save_state(cx)?;
        Ok(Ok(staged))
    }

    fn failure(&self, order: &ClipOrder, failure: ProviderFailure) -> JobFailure {
        JobFailure::new(
            failure.kind.into(),
            format!("Scene {}: {}", order.scene + 1, failure.detail),
        )
    }

    /// A submission that did not go through: the provider is busy (try
    /// again later), the scene is at fault (the others go on), or the key
    /// or credits are (stop).
    fn submit_failed(
        &mut self,
        order: &ClipOrder,
        failure: ProviderFailure,
        cx: &mut JobContext,
    ) -> Result<Step, JobFailure> {
        let kind = JobFailureKind::from(failure.kind);
        if kind.is_transient() {
            return Ok(Step::NotSubmitted(self.failure(order, failure)));
        }
        if !is_about_the_scene(kind) {
            return Err(self.failure(order, failure));
        }
        self.scene_failed(order, failure, cx)
    }

    /// The scene's request ended without a clip: the scene shows why, and
    /// the next attempt submits it again.
    fn scene_failed(
        &mut self,
        order: &ClipOrder,
        failure: ProviderFailure,
        cx: &mut JobContext,
    ) -> Result<Step, JobFailure> {
        let failure = self.failure(order, failure);
        if let Some(run) = self.run_of(order.scene) {
            run.restart();
            self.save_state(cx)?;
        }
        if let Some(mut plan) = self.saved_plan()? {
            plan.scene_mut(order.scene, SystemTime::now())
                .map_err(unexpected)?
                .fail_clip(failure.kind);
            self.handler
                .plans
                .save_scene(&plan, order.scene)
                .map_err(unexpected)?;
        }
        Ok(Step::Failed(failure))
    }

    /// Records the clip's cost and saves it beside the scene's current
    /// clip, for the user to review.
    fn save_clip(&mut self, order: &ClipOrder, bytes: &[u8]) -> Result<(), JobFailure> {
        let provider: Provider = order.provider.parse().map_err(unexpected)?;
        let run = self
            .run_of(order.scene)
            .expect("a submitted scene has its run")
            .clone();
        self.handler.costs.record_for_project(
            PaidCall {
                provider,
                model: &order.model,
                purpose: CostPurpose::SceneClip,
                usage: Metered::video_seconds(u64::from(run.seconds)),
                job: Some(self.job),
                reported: run.quote_micros.map(Money::from_micros),
            },
            self.project,
        );
        let Some(mut plan) = self.saved_plan()? else {
            return Ok(());
        };
        let generation = GenerationId::new();
        let file = clip_file(generation);
        self.handler
            .files
            .write(self.project, &file, bytes)
            .map_err(unexpected)?;
        let clip = SceneClip {
            file: file.clone(),
            seconds: run.seconds,
            source_image: id(&run.source_image)?,
            generation: Generation {
                id: generation,
                owner: self.handler.owner,
                project: self.project,
                provider,
                model: order.model.clone(),
                template: plan.generation.template,
                instructions: format!("Image to video, {} s, from {}", run.seconds, run.image_file),
                prompt: run.prompt,
                output: file,
                usage: TokenUsage::default(),
                generated_at: SystemTime::now(),
                job: Some(self.job),
            },
        };
        let pushed = plan
            .scene_mut(order.scene, SystemTime::now())
            .map_err(unexpected)?
            .add_clip(clip);
        self.handler
            .plans
            .save_scene(&plan, order.scene)
            .map_err(unexpected)?;
        if let Some(pushed) = pushed {
            crate::proxies::remove_media(self.handler.files.as_ref(), self.project, &pushed.file);
        }
        Ok(())
    }
}

/// A clip job's project and scenes, to find a project's clip jobs.
fn clip_payload(job: &Job) -> Option<ClipsPayload> {
    (job.kind() == JobKind::SceneClips)
        .then(|| serde_json::from_str(job.payload()).ok())
        .flatten()
}

impl Bardo {
    /// Every model the video providers offer, each provider's default
    /// first, and the providers with a key saved before the others: the
    /// first model is the default of channels that keep it, so it is one
    /// that can run (#251).
    pub fn clip_models(&self) -> Vec<ClipModel> {
        let (keyed, rest): (Vec<_>, Vec<_>) = self.clips.iter().partition(|generator| {
            self.provider_key(generator.provider()).state != KeyState::NotSet
        });
        keyed
            .into_iter()
            .chain(rest)
            .flat_map(|generator| generator.models())
            .collect()
    }

    /// Whether `model` is offered.
    pub(crate) fn offers_clip_model(&self, model: &ClipModelRef) -> bool {
        self.clip_models()
            .iter()
            .any(|offered| offered.id == *model)
    }

    /// The model of a channel's scenes without their own: the channel's
    /// when still offered, else the first provider's default.
    pub(crate) fn channel_clip_model(
        &self,
        models: &[ClipModel],
        channel: ChannelId,
    ) -> Result<Option<ClipModel>, RepositoryError> {
        let chosen = self
            .channels
            .get(channel)?
            .and_then(|channel| channel.details.clip_model().cloned());
        Ok(chosen
            .and_then(|chosen| models.iter().find(|model| model.id == chosen))
            .or_else(|| models.first())
            .cloned())
    }

    /// The model a scene's next clip uses.
    fn scene_clip_model(
        models: &[ClipModel],
        default_model: Option<&ClipModel>,
        scene: &Scene,
    ) -> Option<ClipModel> {
        match scene.clip_model() {
            Some(own) => models.iter().find(|model| model.id == *own).cloned(),
            None => default_model.cloned(),
        }
    }

    /// The project's clip jobs, oldest first.
    fn clip_jobs(&self, project: VideoProjectId) -> Vec<(Job, ClipsPayload)> {
        let project = project.to_string();
        self.jobs()
            .into_iter()
            .filter_map(|job| clip_payload(&job).map(|payload| (job, payload)))
            .filter(|(_, payload)| payload.project == project)
            .collect()
    }

    /// Whether a clip job of the project is running or waiting.
    pub(crate) fn clips_busy(&self, project: VideoProjectId) -> bool {
        self.clip_jobs(project)
            .iter()
            .any(|(job, _)| job.state().is_active())
    }

    /// The clips part of the scenes panel for `plan`.
    pub(crate) fn clips_view(
        &self,
        project: &VideoProject,
        plan: Option<&ScenePlan>,
    ) -> Result<ClipsView, SceneError> {
        let models = self.clip_models();
        let channel_model = self.channel_clip_model(&models, project.channel)?;
        let mode = self.production_mode(project)?;
        let default_model = Self::mode_clip_model(mode, &models, channel_model.as_ref()).cloned();
        let Some(plan) = plan else {
            return Ok(ClipsView {
                models,
                default_model,
                mode,
                ..ClipsView::default()
            });
        };
        let rates = self.cost_book.rates()?;
        let jobs = self.clip_jobs(plan.project);
        let scenes: Vec<SceneClipView> = plan
            .scenes()
            .iter()
            .enumerate()
            .map(|(index, scene)| {
                let model = Self::scene_clip_model(&models, default_model.as_ref(), scene);
                let seconds = model
                    .as_ref()
                    .map(|model| model.durations.fit(scene.duration()));
                let price = model.as_ref().zip(seconds).and_then(|(model, seconds)| {
                    rates.price(
                        model.id.provider(),
                        model.id.model(),
                        &Metered::video_seconds(u64::from(seconds)),
                    )
                });
                let job = jobs
                    .iter()
                    .rev()
                    .find(|(_, payload)| {
                        payload.plan == plan.id.to_string()
                            && payload.scenes.iter().any(|order| order.scene == index)
                    })
                    .map(|(job, _)| job.clone());
                SceneClipView {
                    model,
                    seconds,
                    price,
                    job,
                }
            })
            .collect();
        let missing: Vec<usize> = plan
            .missing_clips()
            .into_iter()
            .filter(|&index| {
                mode.animates(plan.scenes()[index].start)
                    && !scenes[index].is_busy()
                    && scenes[index].model.is_some()
            })
            .collect();
        let missing_estimate = match missing.is_empty() {
            true => None,
            false => Some(self.estimate(&clip_calls(&scenes, &missing))?),
        };
        Ok(ClipsView {
            models,
            default_model,
            mode,
            scenes,
            missing,
            missing_estimate,
        })
    }

    /// Starts a job that animates scene `index`'s image into a clip with
    /// the scene's model. Its current clip stays until the user accepts the
    /// new one. Past the provider's budget it needs `consent`.
    pub fn generate_scene_clip(
        &self,
        project: VideoProjectId,
        index: usize,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let (plan, view) = self.clips_of(project)?;
        let scene = plan.scene(index)?;
        if !scene.can_animate() {
            return Err(SceneError::NoImageToAnimate);
        }
        if view.scenes[index].is_busy() {
            return Err(SceneError::Busy);
        }
        self.animate(&plan, &view.scenes, vec![index], consent)
    }

    /// Starts a job that animates every scene with an image and no clip.
    /// Past the provider's budget it needs `consent`.
    pub fn generate_missing_clips(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let (plan, view) = self.clips_of(project)?;
        if view.missing.is_empty() {
            return Err(SceneError::NothingToAnimate);
        }
        self.animate(&plan, &view.scenes, view.missing, consent)
    }

    fn clips_of(&self, project: VideoProjectId) -> Result<(ScenePlan, ClipsView), SceneError> {
        let project = self.scenes_project(project)?;
        let plan = self
            .scene_plans
            .scene_plan(project.id)?
            .ok_or(SceneError::NoPlan)?;
        let view = self.clips_view(&project, Some(&plan))?;
        Ok((plan, view))
    }

    fn animate(
        &self,
        plan: &ScenePlan,
        views: &[SceneClipView],
        scenes: Vec<usize>,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneError> {
        let mut orders = Vec::with_capacity(scenes.len());
        for &index in &scenes {
            let model = views[index]
                .model
                .as_ref()
                .ok_or(SceneError::ClipModelNotOffered)?;
            self.require_key(model.id.provider())?;
            orders.push(ClipOrder {
                scene: index,
                provider: model.id.provider().code().to_owned(),
                model: model.id.model().to_owned(),
            });
        }
        self.check_video_budget(plan.project, &clip_calls(views, &scenes), consent)?
            .map_err(SceneError::OverBudget)?;
        let payload = ClipsPayload {
            project: plan.project.to_string(),
            plan: plan.id.to_string(),
            scenes: orders,
        };
        let job = Job::new(self.profile.id, JobKind::SceneClips, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Makes scene `index`'s clips use `model`, or the channel's model
    /// with `None`.
    pub fn set_scene_clip_model(
        &self,
        project: VideoProjectId,
        index: usize,
        model: Option<ClipModelRef>,
    ) -> Result<ScenePlan, SceneError> {
        if model
            .as_ref()
            .is_some_and(|model| !self.offers_clip_model(model))
        {
            return Err(SceneError::ClipModelNotOffered);
        }
        let mut plan = self.own_plan(project)?;
        if plan
            .scene_mut(index, SystemTime::now())?
            .set_clip_model(model)
        {
            self.scene_plans.save_scene(&plan, index)?;
        }
        Ok(plan)
    }

    /// Replaces scene `index`'s motion prompt with the user's; an empty one
    /// goes back to the image prompt.
    pub fn edit_scene_motion_prompt(
        &self,
        project: VideoProjectId,
        index: usize,
        prompt: &str,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        let prompt = match prompt.trim() {
            "" => None,
            text => Some(ScenePrompt::new(text).map_err(SceneError::Invalid)?),
        };
        if plan
            .scene_mut(index, SystemTime::now())?
            .set_motion_prompt(prompt)
        {
            self.scene_plans.save_scene(&plan, index)?;
        }
        Ok(plan)
    }

    /// Makes scene `index`'s new clip its clip; the replaced file goes.
    pub fn accept_scene_clip(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        let replaced = plan.scene_mut(index, SystemTime::now())?.accept_clip()?;
        self.scene_plans.save_scene(&plan, index)?;
        if let Some(replaced) = replaced {
            crate::proxies::remove_media(self.files.as_ref(), plan.project, &replaced.file);
        }
        Ok(plan)
    }

    /// Drops scene `index`'s new clip and keeps the current one (or the
    /// still image).
    pub fn reject_scene_clip(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        let dropped = plan.scene_mut(index, SystemTime::now())?.reject_clip()?;
        self.scene_plans.save_scene(&plan, index)?;
        crate::proxies::remove_media(self.files.as_ref(), plan.project, &dropped.file);
        Ok(plan)
    }

    /// Drops scene `index`'s clip: the scene shows its still image again.
    pub fn use_scene_still(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<ScenePlan, SceneError> {
        let mut plan = self.own_plan(project)?;
        if let Some(removed) = plan.scene_mut(index, SystemTime::now())?.remove_clip() {
            self.scene_plans.save_scene(&plan, index)?;
            crate::proxies::remove_media(self.files.as_ref(), plan.project, &removed.file);
        }
        Ok(plan)
    }

    /// Where a scene clip is on disk, for the screen to play it.
    pub fn scene_clip_path(&self, clip: &SceneClip) -> PathBuf {
        self.files.path(clip.generation.project, &clip.file)
    }
}

/// The paid calls that animate `scenes`, one per scene at its model and
/// length.
fn clip_calls(views: &[SceneClipView], scenes: &[usize]) -> Vec<PlannedCall> {
    scenes
        .iter()
        .filter_map(|&index| {
            let view = &views[index];
            let model = view.model.as_ref()?;
            Some(
                PlannedCall::new(model.id.provider(), CostPurpose::SceneClip, 1)
                    .with_model(model.id.model())
                    .with_video_seconds(view.seconds?),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use bardo_domain::{
        ChannelDraft, ClipDurations, Cost, CostRepository, JobState, Meter, ProviderFailure,
        VideoProject,
    };

    use super::*;
    use crate::scenes::tests::{Harness, done, wait_done};
    use crate::testing::{CLIP, ClipCall, SCENE_IMAGE};
    use crate::{Text, testing::FakeClips};

    const PATIENCE: Duration = Duration::from_secs(10);

    fn model(name: &str) -> ClipModelRef {
        ClipModelRef::new(Provider::Higgsfield, name).unwrap()
    }

    fn plan_of(app: &Bardo, project: &VideoProject) -> ScenePlan {
        app.scenes(project.id).unwrap().plan.unwrap()
    }

    /// Waits until `ready` holds for the fake provider.
    fn wait_for(clips: &FakeClips, ready: impl Fn(&FakeClips) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !ready(clips) {
            assert!(Instant::now() < deadline, "the provider never got there");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Gives the project's channel `model` for its clips, in the mode that
    /// animates every scene with it.
    fn channel_uses(app: &Bardo, project: &VideoProject, model: Option<ClipModelRef>) {
        let channel = app
            .channels()
            .unwrap()
            .into_iter()
            .find(|channel| channel.id == project.channel)
            .unwrap();
        app.update_channel(
            channel.id,
            ChannelDraft {
                clip_model: model,
                production: ProductionMode::ChannelModel,
                ..ChannelDraft::from(&channel.details)
            },
        )
        .unwrap();
    }

    #[test]
    fn a_clip_animates_the_scene_image_and_waits_for_review() {
        let h = Harness::new();
        let app = h.start();
        let (project, drawn) = h.drawn_project(&app);
        *h.clips.quote.lock().unwrap() = Some(Money::from_cents(56));
        let scene = &drawn.scenes()[0];
        let view = app.scenes(project.id).unwrap().clips;
        assert_eq!(view.default_model.as_ref().unwrap().id, model("fake/range"));
        let seconds = view.scenes[0].seconds.unwrap();
        assert_eq!(
            seconds,
            ClipDurations::Range { min: 3, max: 15 }.fit(scene.duration())
        );

        let job = done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );

        let staged = h.clips.staged.lock().unwrap().clone();
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].bytes, SCENE_IMAGE);
        assert_eq!(staged[0].format, ImageFormat::Png);
        let sent = h.clips.submissions();
        assert_eq!(
            sent,
            [ClipRequest {
                model: "fake/range".into(),
                prompt: scene.prompt().as_str().into(),
                image: StagedImage("https://fake/image-1".into()),
                first_frame: ClipImage {
                    bytes: SCENE_IMAGE.to_vec(),
                    format: ImageFormat::Png,
                },
                seconds,
                aspect: bardo_domain::AspectRatio::Landscape,
            }],
            "the image prompt moves the image until a motion prompt is written; \
             the image also goes inline for providers that take it so"
        );

        let plan = plan_of(&app, &project);
        let animated = &plan.scenes()[0];
        assert_eq!(animated.clip(), None, "a new clip waits for review");
        let clip = animated.pending_clip().unwrap().clone();
        assert_eq!(h.files.read(project.id, &clip.file).unwrap(), CLIP);
        assert!(clip.file.starts_with("clip-") && clip.file.ends_with(".mp4"));
        assert_eq!(clip.seconds, seconds);
        assert_eq!(clip.source_image, scene.image().unwrap().generation.id);
        let generation = &clip.generation;
        assert_eq!(generation.provider, Provider::Higgsfield);
        assert_eq!(generation.model, "fake/range");
        assert_eq!(generation.prompt, scene.prompt().as_str());
        assert!(
            generation
                .instructions
                .contains(&scene.image().unwrap().file)
        );
        assert_eq!(generation.job, Some(job.id()));
        assert!(
            plan.scenes()[1..]
                .iter()
                .all(|s| s.pending_clip().is_none())
        );

        let record =
            h.db.project_costs(project.id)
                .unwrap()
                .into_iter()
                .find(|record| record.purpose == CostPurpose::SceneClip)
                .unwrap();
        assert_eq!(record.model, "fake/range");
        assert_eq!(record.usage.video_seconds, u64::from(seconds));
        assert_eq!(record.cost, Cost::Reported(Money::from_cents(56)));
        assert_eq!(record.job, Some(job.id()));

        let plan = app.accept_scene_clip(project.id, 0).unwrap();
        assert_eq!(plan.scenes()[0].clip(), Some(&clip));
        assert_eq!(plan.scenes()[0].pending_clip(), None);
    }

    /// One request Google got: URL, key header and body.
    type Sent = (String, Option<String>, Vec<u8>);

    /// Google's API as recorded: a Veo operation that is done on the
    /// first check, and its clip.
    #[derive(Default, Clone)]
    struct GoogleApi {
        sent: Arc<std::sync::Mutex<Vec<Sent>>>,
    }

    impl bardo_ai::http::Transport for GoogleApi {
        fn send(
            &self,
            request: &bardo_ai::http::HttpRequest,
        ) -> Result<bardo_ai::http::HttpResponse, bardo_ai::http::TransportError> {
            self.sent.lock().unwrap().push((
                request.url.clone(),
                request.header_value("x-goog-api-key").map(str::to_owned),
                request.body.clone().unwrap_or_default(),
            ));
            const OPERATION: &str = "models/veo-3.1-lite-generate-preview/operations/op1";
            const VIDEO: &str =
                "https://generativelanguage.googleapis.com/v1beta/files/f1:download?alt=media";
            let body = if request.url.ends_with(":predictLongRunning") {
                serde_json::json!({ "name": OPERATION }).to_string()
            } else if request.url.ends_with(OPERATION) {
                serde_json::json!({
                    "name": OPERATION,
                    "done": true,
                    "response": { "generateVideoResponse": {
                        "generatedSamples": [{ "video": { "uri": VIDEO } }],
                    } },
                })
                .to_string()
            } else if request.url == VIDEO {
                String::from_utf8(CLIP.to_vec()).unwrap()
            } else {
                return Ok(bardo_ai::http::HttpResponse::new(404, "{}"));
            };
            Ok(bardo_ai::http::HttpResponse::new(200, body))
        }
    }

    #[test]
    fn a_scene_animated_with_veo_goes_through_google_on_the_gemini_key() {
        let mut h = Harness::new();
        let google = GoogleApi::default();
        h.more_clips.push(Arc::new(
            bardo_ai::GoogleClips::with_transport(google.clone()).with_sleeper(Arc::new(|_| {})),
        ));
        let app = h.start();
        let (project, drawn) = h.drawn_project(&app);
        let veo =
            ClipModelRef::new(Provider::Gemini, "veo-3.1-lite-generate-preview/720p").unwrap();
        let models = app.clip_models();
        assert!(
            models.iter().any(|offered| offered.id == veo),
            "Google's models are offered"
        );
        assert_eq!(
            models[0].id,
            model("fake/range"),
            "the first provider's default leads"
        );
        channel_uses(&app, &project, Some(veo.clone()));

        let view = app.scenes(project.id).unwrap().clips;
        let scene = &view.scenes[0];
        assert_eq!(scene.model.as_ref().unwrap().id, veo);
        let seconds = scene.seconds.unwrap();
        assert!([4, 6, 8].contains(&seconds));
        let price = Money::from_micros(50_000 * u64::from(seconds));
        assert_eq!(
            scene.price,
            Some(price),
            "Veo 3.1 Lite 720p at $0.05 a second"
        );

        let job = done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );

        assert!(h.clips.submissions().is_empty(), "not the other provider");
        let sent = google.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 3, "submit, one status check, download");
        assert!(
            sent.iter()
                .all(|(_, key, _)| key.as_deref() == Some(crate::scenes::tests::GEMINI_KEY))
        );
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        let image = &body["instances"][0]["image"]["inlineData"];
        assert_eq!(image["mimeType"], "image/png");
        assert!(
            !image["data"].as_str().unwrap().is_empty(),
            "the scene image, inline"
        );
        assert_eq!(body["parameters"]["durationSeconds"], seconds);
        assert_eq!(
            body["instances"][0]["prompt"],
            drawn.scenes()[0].prompt().as_str()
        );

        let plan = plan_of(&app, &project);
        let clip = plan.scenes()[0].pending_clip().unwrap().clone();
        assert_eq!(h.files.read(project.id, &clip.file).unwrap(), CLIP);
        assert_eq!(clip.generation.provider, Provider::Gemini);
        assert_eq!(clip.generation.model, veo.model());
        let record =
            h.db.project_costs(project.id)
                .unwrap()
                .into_iter()
                .find(|record| record.purpose == CostPurpose::SceneClip)
                .unwrap();
        assert_eq!(record.provider, Provider::Gemini);
        assert_eq!(record.cost, Cost::Estimated(price), "Google quotes nothing");
        assert_eq!(record.job, Some(job.id()));
    }

    /// Kling's API as documented: a task that succeeded on the first
    /// check, and its clip.
    #[derive(Default, Clone)]
    struct KlingApi {
        sent: Arc<std::sync::Mutex<Vec<Sent>>>,
    }

    impl bardo_ai::http::Transport for KlingApi {
        fn send(
            &self,
            request: &bardo_ai::http::HttpRequest,
        ) -> Result<bardo_ai::http::HttpResponse, bardo_ai::http::TransportError> {
            self.sent.lock().unwrap().push((
                request.url.clone(),
                request.header_value("authorization").map(str::to_owned),
                request.body.clone().unwrap_or_default(),
            ));
            const VIDEO: &str = "https://cdn.kling.example/output/7001.mp4";
            let body = if request.url.ends_with("/image-to-video/kling-3.0") {
                serde_json::json!({ "code": 0, "data": { "id": "7001", "status": "submitted" } })
                    .to_string()
            } else if request.url.ends_with("/tasks?task_ids=7001") {
                serde_json::json!({ "code": 0, "data": [{
                    "id": "7001",
                    "status": "succeeded",
                    "outputs": [{ "type": "video", "url": VIDEO }],
                }] })
                .to_string()
            } else if request.url == VIDEO {
                String::from_utf8(CLIP.to_vec()).unwrap()
            } else {
                return Ok(bardo_ai::http::HttpResponse::new(404, "{}"));
            };
            Ok(bardo_ai::http::HttpResponse::new(200, body))
        }
    }

    #[test]
    fn a_channel_on_the_default_model_follows_a_provider_whose_key_is_saved() {
        let mut h = Harness::new();
        h.more_clips.push(Arc::new(bardo_ai::KlingClips::new()));
        h.more_clips.push(Arc::new(bardo_ai::GoogleClips::new()));
        let mut app = h.start();
        let (project, _) = h.drawn_project(&app);
        channel_uses(&app, &project, None);
        let default = |app: &Bardo| {
            let model = app
                .channel_clip_model(&app.clip_models(), project.channel)
                .unwrap()
                .unwrap();
            (model.id.provider(), model.id.model().to_owned())
        };
        let missing_key = |app: &Bardo| {
            app.production(project.id)
                .unwrap()
                .estimate(ProductionMode::ChannelModel)
                .unwrap()
                .missing_key
        };

        // The harness saves Higgsfield's key and Gemini's: Higgsfield, the
        // first provider, stays the default, as before.
        assert_eq!(default(&app).0, Provider::Higgsfield);
        assert_eq!(missing_key(&app), None);

        // Without Higgsfield's key, Veo through the Gemini key already
        // saved for images.
        app.remove_provider_key(Provider::Higgsfield).unwrap();
        assert_eq!(
            default(&app),
            (
                Provider::Gemini,
                bardo_ai::google_clips::DEFAULT_MODEL.to_owned()
            )
        );
        assert_eq!(missing_key(&app), None);

        // Kling comes before Google, as on the list.
        app.save_provider_key(Provider::Kling, "kling-test-key-0001")
            .unwrap();
        assert_eq!(
            default(&app),
            (Provider::Kling, bardo_ai::kling::DEFAULT_MODEL.to_owned())
        );
        assert_eq!(
            app.clip_models().first().map(|model| model.id.provider()),
            Some(Provider::Kling),
            "the menus list a provider with a key first"
        );

        // With no video key at all, the first provider again: it asks for
        // its key.
        app.remove_provider_key(Provider::Kling).unwrap();
        app.remove_provider_key(Provider::Gemini).unwrap();
        assert_eq!(default(&app).0, Provider::Higgsfield);
        assert_eq!(missing_key(&app), Some(Provider::Higgsfield));

        // A channel's own pick stays, whatever keys are saved.
        let veo =
            ClipModelRef::new(Provider::Gemini, bardo_ai::google_clips::DEFAULT_MODEL).unwrap();
        channel_uses(&app, &project, Some(veo.clone()));
        app.save_provider_key(Provider::Kling, "kling-test-key-0001")
            .unwrap();
        assert_eq!(default(&app).0, Provider::Gemini);
    }

    #[test]
    fn a_scene_animated_on_the_higgsfield_plan_asks_for_a_fresh_sign_in_each_step() {
        let mut h = Harness::new();
        let plan = Arc::new(crate::plans::testing::FakePlan::default());
        // Every token is close to expiring: each use renews it.
        *plan.expires_in.lock().unwrap() = Duration::from_secs(60);
        h.plans.push(Arc::clone(&plan) as _);
        let on_plan = Arc::new(FakeClips {
            provider: Some(Provider::HiggsfieldPlan),
            ..FakeClips::default()
        });
        h.more_clips.push(Arc::clone(&on_plan) as _);
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let model = ClipModelRef::new(Provider::HiggsfieldPlan, "fake/range").unwrap();
        channel_uses(&app, &project, Some(model));

        let error = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
            .unwrap_err();
        assert!(matches!(
            error,
            SceneError::MissingKey(Provider::HiggsfieldPlan)
        ));
        assert_eq!(error.message(), Text::ScenesMissingHiggsfieldPlan);

        let tokens = bardo_domain::TokenSet::granted(
            &bardo_domain::TokenGrant {
                access_token: bardo_domain::SecretText::new("stored-access"),
                refresh_token: Some(bardo_domain::SecretText::new("stored-refresh")),
                expires_in: Duration::from_secs(60),
                scopes: Vec::new(),
            },
            std::time::SystemTime::now(),
        );
        bardo_domain::ConnectionSecrets::set_plan_session(
            &*h.connection_secrets,
            app.profile().id,
            Provider::HiggsfieldPlan,
            &bardo_domain::PlanSession::new("client-0001", tokens).unwrap(),
        )
        .unwrap();
        drop(app);
        let app = h.start();
        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
                .unwrap(),
        );

        assert!(h.clips.submissions().is_empty(), "not the API's provider");
        assert_eq!(on_plan.submissions().len(), 1);
        let keys = on_plan.keys.lock().unwrap().clone();
        assert_eq!(
            keys,
            [
                "plan-access-0001",
                "plan-access-0001",
                "plan-access-0002",
                "plan-access-0002"
            ],
            "staging and submitting share a step; the status check and the \
             download the next one, on a renewed token"
        );
        let pending = plan_of(&app, &project).scenes()[0].pending_clip().cloned();
        assert_eq!(
            pending.unwrap().generation.provider,
            Provider::HiggsfieldPlan
        );
    }

    /// The app over a plan signed in with a fresh token
    /// (`stored-access`), animating with `clips` on the plan.
    fn on_the_plan(
        h: &mut Harness,
        clips: FakeClips,
    ) -> (
        Arc<crate::plans::testing::FakePlan>,
        Arc<FakeClips>,
        Bardo,
        VideoProject,
    ) {
        let plan = Arc::new(crate::plans::testing::FakePlan::default());
        h.plans.push(Arc::clone(&plan) as _);
        let clips = Arc::new(FakeClips {
            provider: Some(Provider::HiggsfieldPlan),
            ..clips
        });
        h.more_clips.push(Arc::clone(&clips) as _);
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let model = ClipModelRef::new(Provider::HiggsfieldPlan, "fake/range").unwrap();
        channel_uses(&app, &project, Some(model));
        let tokens = bardo_domain::TokenSet::granted(
            &bardo_domain::TokenGrant {
                access_token: bardo_domain::SecretText::new("stored-access"),
                refresh_token: Some(bardo_domain::SecretText::new("stored-refresh")),
                expires_in: Duration::from_secs(3_600),
                scopes: Vec::new(),
            },
            std::time::SystemTime::now(),
        );
        bardo_domain::ConnectionSecrets::set_plan_session(
            &*h.connection_secrets,
            app.profile().id,
            Provider::HiggsfieldPlan,
            &bardo_domain::PlanSession::new("client-0001", tokens).unwrap(),
        )
        .unwrap();
        drop(app);
        let app = h.start();
        (plan, clips, app, project)
    }

    #[test]
    fn a_plan_clip_whose_answer_was_lost_is_found_instead_of_paid_for_twice() {
        let mut h = Harness::new();
        let (_, clips, app, project) = on_the_plan(
            &mut h,
            FakeClips {
                no_idempotency: true,
                lost: Mutex::new(1),
                ..FakeClips::default()
            },
        );
        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
                .unwrap(),
        );
        assert_eq!(*clips.sends.lock().unwrap(), 1, "sent once");
        assert_eq!(*clips.checks.lock().unwrap(), 1, "credits checked first");
        assert_eq!(*clips.lookups.lock().unwrap(), 1);
        assert_eq!(clips.submissions().len(), 1);
        assert!(plan_of(&app, &project).scenes()[0].pending_clip().is_some());
    }

    #[test]
    fn a_lost_plan_submission_that_is_not_listed_fails_the_scene_unsent_again() {
        let mut h = Harness::new();
        let (_, clips, app, project) = on_the_plan(
            &mut h,
            FakeClips {
                no_idempotency: true,
                unseen: Mutex::new(1),
                ..FakeClips::default()
            },
        );
        let job = wait_done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
                .unwrap(),
        );
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert!(failure.detail.contains("library"), "{failure:?}");
        assert_eq!(*clips.sends.lock().unwrap(), 1, "never sent again");
        assert_eq!(*clips.lookups.lock().unwrap(), 1);
        let plan = plan_of(&app, &project);
        assert!(plan.scenes()[0].clip_failure().is_some());
        assert!(plan.scenes()[0].pending_clip().is_none());
    }

    #[test]
    fn a_refused_plan_token_is_renewed_once_and_the_call_made_again() {
        let mut h = Harness::new();
        let (plan, clips, app, project) = on_the_plan(
            &mut h,
            FakeClips {
                no_idempotency: true,
                refusals: Mutex::new(1),
                ..FakeClips::default()
            },
        );
        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
                .unwrap(),
        );
        let refreshes = plan
            .calls()
            .into_iter()
            .filter(|call| matches!(call, crate::plans::testing::PlanCall::Refresh { .. }))
            .count();
        assert_eq!(refreshes, 1);
        let keys = clips.keys.lock().unwrap().clone();
        // Staged, checked, refused on submit; submitted again renewed.
        assert_eq!(keys[..3], ["stored-access"; 3]);
        assert!(
            keys[3..].iter().all(|key| key == "plan-access-0001"),
            "{keys:?}"
        );
        assert_eq!(*clips.sends.lock().unwrap(), 1);
    }

    #[test]
    fn a_plan_that_refuses_its_renewed_token_too_stops_the_run() {
        let mut h = Harness::new();
        let (plan, clips, app, project) = on_the_plan(
            &mut h,
            FakeClips {
                no_idempotency: true,
                refusals: Mutex::new(2),
                ..FakeClips::default()
            },
        );
        let job = wait_done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
                .unwrap(),
        );
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::KeyRejected);
        assert_eq!(plan.calls().len(), 1, "one renewal");
        assert_eq!(*clips.sends.lock().unwrap(), 0);
    }

    #[test]
    fn a_scene_animated_with_kling_goes_straight_to_kling_on_its_own_key() {
        const KLING_KEY: &str = "kling-test-key-0001";
        let mut h = Harness::new();
        let kling = KlingApi::default();
        h.more_clips.push(Arc::new(
            bardo_ai::KlingClips::with_transport(kling.clone()).with_sleeper(Arc::new(|_| {})),
        ));
        let mut app = h.start();
        let (project, drawn) = h.drawn_project(&app);
        let kling_3 = ClipModelRef::new(Provider::Kling, bardo_ai::kling::DEFAULT_MODEL).unwrap();
        assert!(
            app.clip_models()
                .iter()
                .any(|offered| offered.id == kling_3),
            "Kling's models are offered"
        );
        channel_uses(&app, &project, Some(kling_3.clone()));

        let error = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
            .unwrap_err();
        assert!(matches!(error, SceneError::MissingKey(Provider::Kling)));
        assert_eq!(error.message(), Text::ScenesMissingKlingKey);
        app.save_provider_key(Provider::Kling, KLING_KEY).unwrap();

        let view = app.scenes(project.id).unwrap().clips;
        let seconds = view.scenes[0].seconds.unwrap();
        let price = Money::from_micros(84_000 * u64::from(seconds));
        assert_eq!(
            view.scenes[0].price,
            Some(price),
            "Kling 3.0 720p at $0.084 a second"
        );

        let job = done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );

        assert!(h.clips.submissions().is_empty(), "not the other provider");
        let sent = kling.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 3, "submit, one status check, download");
        let bearer = format!("Bearer {KLING_KEY}");
        assert!(
            sent[..2]
                .iter()
                .all(|(_, key, _)| key.as_deref() == Some(bearer.as_str()))
        );
        assert_eq!(sent[2].1, None, "the CDN gets no key");
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(
            body["contents"][0]["text"],
            drawn.scenes()[0].prompt().as_str()
        );
        assert!(
            !body["contents"][1]["url"].as_str().unwrap().is_empty(),
            "the scene image, inline"
        );
        assert_eq!(body["settings"]["duration"], seconds);
        assert!(
            body["options"]["external_task_id"]
                .as_str()
                .unwrap()
                .starts_with("bardo-")
        );

        let plan = plan_of(&app, &project);
        let clip = plan.scenes()[0].pending_clip().unwrap().clone();
        assert_eq!(h.files.read(project.id, &clip.file).unwrap(), CLIP);
        assert_eq!(clip.generation.provider, Provider::Kling);
        assert_eq!(clip.generation.model, kling_3.model());
        let record =
            h.db.project_costs(project.id)
                .unwrap()
                .into_iter()
                .find(|record| record.purpose == CostPurpose::SceneClip)
                .unwrap();
        assert_eq!(record.provider, Provider::Kling);
        assert_eq!(record.cost, Cost::Estimated(price), "Kling quotes nothing");
        assert_eq!(record.job, Some(job.id()));
    }

    #[test]
    fn the_channel_model_applies_unless_the_scene_picks_its_own() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        channel_uses(&app, &project, Some(model("fake/choices")));
        app.set_scene_clip_model(project.id, 1, Some(model("fake/range")))
            .unwrap();
        assert!(matches!(
            app.set_scene_clip_model(project.id, 2, Some(model("retired/model"))),
            Err(SceneError::ClipModelNotOffered)
        ));

        let view = app.scenes(project.id).unwrap().clips;
        let models: Vec<_> = view
            .scenes
            .iter()
            .map(|scene| scene.model.as_ref().unwrap().id.model().to_owned())
            .collect();
        assert_eq!(models, ["fake/choices", "fake/range", "fake/choices"]);
        assert_eq!(view.missing, [0, 1, 2]);
        done(
            &app,
            app.generate_missing_clips(project.id, BudgetConsent::Ask)
                .unwrap(),
        );

        let sent = h.clips.submissions();
        let sent: Vec<_> = sent.iter().map(|r| (r.model.as_str(), r.seconds)).collect();
        assert!(matches!(sent[0], ("fake/choices", 5 | 10)));
        assert_eq!(sent[1].0, "fake/range");
        assert!(matches!(sent[2], ("fake/choices", 5 | 10)));
        let plan = plan_of(&app, &project);
        assert!(plan.scenes().iter().all(|s| s.pending_clip().is_some()));
        assert!(matches!(
            app.generate_missing_clips(project.id, BudgetConsent::Ask),
            Err(SceneError::NothingToAnimate)
        ));

        // Back to the channel's model.
        let plan = app.set_scene_clip_model(project.id, 1, None).unwrap();
        assert_eq!(plan.scenes()[1].clip_model(), None);
    }

    #[test]
    fn a_restart_while_a_clip_is_made_polls_the_same_request() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        h.clips.hold(true);
        let id = app
            .generate_scene_clip(project.id, 2, BudgetConsent::Ask)
            .unwrap();
        wait_for(&h.clips, |clips| !clips.polls.lock().unwrap().is_empty());
        drop(app);

        h.clips.hold(false);
        let app = h.start();
        done(&app, id);

        assert_eq!(h.clips.submissions().len(), 1, "submitted once");
        assert_eq!(h.clips.staged.lock().unwrap().len(), 1, "staged once");
        let polls = h.clips.polls.lock().unwrap().clone();
        assert!(polls.iter().all(|request| request == "req-0"));
        assert!(plan_of(&app, &project).scenes()[2].pending_clip().is_some());
    }

    /// The app with `clips` as a second video provider (in Kling's place)
    /// that starts, and charges for, a new clip every time it is sent a
    /// submission, and a drawn project whose channel animates with it.
    fn charged_on_every_send(
        h: &mut Harness,
        clips: FakeClips,
    ) -> (Arc<FakeClips>, Bardo, VideoProject) {
        let clips = Arc::new(FakeClips {
            provider: Some(Provider::Kling),
            no_idempotency: true,
            ..clips
        });
        h.more_clips.push(Arc::clone(&clips) as _);
        let mut app = h.start();
        app.save_provider_key(Provider::Kling, "kling-test-key-0001")
            .unwrap();
        let (project, _) = h.drawn_project(&app);
        let model = ClipModelRef::new(Provider::Kling, "fake/range").unwrap();
        channel_uses(&app, &project, Some(model));
        (clips, app, project)
    }

    /// Waits for a cancelled job's handler to return.
    fn returned(app: &Bardo, id: JobId) {
        let deadline = Instant::now() + PATIENCE;
        while app.job_runs_here(id) {
            assert!(Instant::now() < deadline, "the handler never returned");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn sends(clips: &FakeClips) -> u32 {
        *clips.sends.lock().unwrap()
    }

    fn clips_in_costs(h: &Harness, project: &VideoProject) -> usize {
        h.db.project_costs(project.id)
            .unwrap()
            .into_iter()
            .filter(|record| record.purpose == CostPurpose::SceneClip)
            .count()
    }

    /// A harness whose app, when closing, waits for its handlers however
    /// slow the machine is.
    fn closing_patiently() -> Harness {
        Harness {
            closing_wait: Some(PATIENCE),
            ..Harness::new()
        }
    }

    /// Closes the app, and lets the call held open go on once the app has
    /// told its jobs to stop.
    fn close_with_a_call_open(app: Bardo, clips: &Arc<FakeClips>) {
        let resumer = {
            let (clips, queue) = (Arc::clone(clips), app.queue_probe());
            std::thread::spawn(move || {
                while !queue.closing() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                clips.resume();
            })
        };
        drop(app);
        resumer.join().unwrap();
    }

    #[test]
    fn a_job_cancelled_while_an_image_is_handed_over_sends_no_clip() {
        let mut h = Harness::new();
        let (clips, app, project) = charged_on_every_send(&mut h, FakeClips::default());
        clips.pause_at(ClipCall::StageImage);
        let id = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
            .unwrap();
        wait_for(&clips, FakeClips::paused);

        app.cancel_job(id).unwrap();
        clips.resume();
        returned(&app, id);

        assert_eq!(sends(&clips), 0, "a cancelled job still sent a clip");
        assert_eq!(clips_in_costs(&h, &project), 0);

        // Started again by hand, it sends the clip once, with the image the
        // cancelled run handed over.
        app.retry_job(id).unwrap();
        done(&app, id);
        assert_eq!(sends(&clips), 1);
        assert_eq!(clips.staged.lock().unwrap().len(), 1, "handed over once");
        assert_eq!(clips_in_costs(&h, &project), 1);
        assert!(plan_of(&app, &project).scenes()[0].pending_clip().is_some());
    }

    #[test]
    fn a_job_cancelled_while_an_image_is_handed_over_sends_no_clip_with_a_submission_id_either() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        h.clips.pause_at(ClipCall::StageImage);
        let id = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Ask)
            .unwrap();
        wait_for(&h.clips, FakeClips::paused);

        app.cancel_job(id).unwrap();
        h.clips.resume();
        returned(&app, id);
        assert_eq!(sends(&h.clips), 0, "a cancelled job still sent a clip");

        app.retry_job(id).unwrap();
        done(&app, id);
        assert_eq!(sends(&h.clips), 1);
        assert_eq!(h.clips.staged.lock().unwrap().len(), 1, "handed over once");
        assert!(plan_of(&app, &project).scenes()[0].pending_clip().is_some());
    }

    #[test]
    fn a_clip_sent_as_its_job_is_cancelled_is_kept_for_the_retry() {
        let mut h = Harness::new();
        let (clips, app, project) = charged_on_every_send(&mut h, FakeClips::default());
        clips.pause_at(ClipCall::Submit);
        let id = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
            .unwrap();
        wait_for(&clips, FakeClips::paused);

        // The submission is on its way: the provider starts the clip.
        app.cancel_job(id).unwrap();
        clips.resume();
        returned(&app, id);

        let cancelled = wait_done(&app, id);
        assert_eq!(cancelled.state(), JobState::Cancelled);
        let kept = cancelled.external_handle().expect("the request is kept");
        assert!(kept.contains("req-0"), "{kept}");

        // The retry asks for that clip instead of paying for another.
        app.retry_job(id).unwrap();
        done(&app, id);
        assert_eq!(sends(&clips), 1, "the scene was sent, and paid for, twice");
        assert_eq!(clips.submissions().len(), 1);
        assert_eq!(*clips.lookups.lock().unwrap(), 0, "nothing to look for");
        assert_eq!(clips_in_costs(&h, &project), 1);
        assert!(plan_of(&app, &project).scenes()[0].pending_clip().is_some());
    }

    #[test]
    fn closing_while_an_image_is_handed_over_sends_the_clip_once_after_the_restart() {
        let mut h = closing_patiently();
        let (clips, app, project) = charged_on_every_send(&mut h, FakeClips::default());
        clips.pause_at(ClipCall::StageImage);
        let id = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
            .unwrap();
        wait_for(&clips, FakeClips::paused);

        close_with_a_call_open(app, &clips);
        assert_eq!(sends(&clips), 0, "a closing app still sent a clip");

        let app = h.start();
        done(&app, id);
        assert_eq!(sends(&clips), 1);
        assert_eq!(clips.staged.lock().unwrap().len(), 1, "handed over once");
        assert_eq!(clips_in_costs(&h, &project), 1);
        assert!(plan_of(&app, &project).scenes()[0].pending_clip().is_some());
    }

    #[test]
    fn a_clip_sent_as_the_app_closes_is_kept_for_the_next_start() {
        let mut h = closing_patiently();
        let (clips, app, project) = charged_on_every_send(&mut h, FakeClips::default());
        clips.pause_at(ClipCall::Submit);
        let id = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
            .unwrap();
        wait_for(&clips, FakeClips::paused);

        close_with_a_call_open(app, &clips);

        let app = h.start();
        done(&app, id);
        assert_eq!(sends(&clips), 1, "the scene was sent, and paid for, twice");
        assert_eq!(clips.submissions().len(), 1);
        assert_eq!(*clips.lookups.lock().unwrap(), 0, "nothing to look for");
        assert_eq!(clips_in_costs(&h, &project), 1);
        assert!(plan_of(&app, &project).scenes()[0].pending_clip().is_some());
    }

    #[test]
    fn a_run_refused_its_save_after_the_app_closed_sends_no_clip() {
        let mut h = Harness {
            closing_wait: Some(Duration::from_millis(20)),
            ..Harness::new()
        };
        let (clips, app, project) = charged_on_every_send(&mut h, FakeClips::default());
        clips.pause_at(ClipCall::StageImage);
        let id = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
            .unwrap();
        wait_for(&clips, FakeClips::paused);

        // The app closes without the run, whose upload ends only later:
        // its note of the staged image is refused, so it stops there.
        let closed = app.queue_probe();
        drop(app);
        clips.resume();
        wait_for(&clips, |_| !closed.runs(id));
        assert_eq!(sends(&clips), 0, "the run that lost its job still paid");

        // The next start runs the job from what was saved: nothing staged.
        let app = h.start();
        done(&app, id);
        assert_eq!(sends(&clips), 1);
        assert_eq!(clips.staged.lock().unwrap().len(), 2, "handed over again");
        assert_eq!(clips_in_costs(&h, &project), 1);
    }

    #[test]
    fn a_failed_clip_shows_on_its_scene_and_a_retry_submits_only_it() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        h.clips.fail("launch pad");

        let id = app
            .generate_missing_clips(project.id, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, id);

        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::UnexpectedAnswer);
        assert!(
            failure.detail.contains("1 of 3 clips"),
            "{}",
            failure.detail
        );
        assert!(failure.detail.contains("launch pad"), "{}", failure.detail);
        let plan = plan_of(&app, &project);
        assert_eq!(
            plan.scenes()[0].clip_failure(),
            Some(JobFailureKind::UnexpectedAnswer)
        );
        assert!(plan.scenes()[0].pending_clip().is_none());
        assert!(
            plan.scenes()[1..]
                .iter()
                .all(|s| s.pending_clip().is_some())
        );
        let view = app.scenes(project.id).unwrap().clips;
        assert_eq!(view.scenes[0].job.as_ref().map(Job::id), Some(id));
        assert_eq!(view.missing, [0]);

        // A softer motion prompt, and the retry submits only that scene.
        app.edit_scene_motion_prompt(project.id, 0, "Slow push in on the rocket")
            .unwrap();
        h.clips.succeed_all();
        app.retry_job(id).unwrap();
        done(&app, id);

        let sent = h.clips.submissions();
        assert_eq!(sent.len(), 4);
        assert_eq!(sent[3].prompt, "Slow push in on the rocket");
        let plan = plan_of(&app, &project);
        assert_eq!(plan.scenes()[0].clip_failure(), None);
        assert!(plan.scenes().iter().all(|s| s.pending_clip().is_some()));
    }

    #[test]
    fn provider_outages_while_polling_wait_instead_of_resubmitting() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        // Short outages are waited out; a long one ends the attempt, and the
        // queue's retry polls the same request again.
        *h.clips.status_outages.lock().unwrap() = MAX_HICCUPS + 2;

        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );

        assert_eq!(h.clips.submissions().len(), 1);
        assert!(plan_of(&app, &project).scenes()[0].pending_clip().is_some());
    }

    #[test]
    fn a_rejected_key_stops_the_run_and_keeps_its_requests() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        *h.clips.submit_failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "invalid credentials",
        ));

        let job = wait_done(
            &app,
            app.generate_missing_clips(project.id, BudgetConsent::Ask)
                .unwrap(),
        );

        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::KeyRejected);
        assert_eq!(h.clips.staged.lock().unwrap().len(), 1, "stopped at once");
        let plan = plan_of(&app, &project);
        assert!(plan.scenes().iter().all(|s| s.clip_failure().is_none()));
    }

    #[test]
    fn animating_waits_for_an_image_a_key_and_the_budget() {
        let h = Harness::new();
        let mut app = h.start();
        let (project, _) = h.planned_project(&app);
        assert!(matches!(
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask),
            Err(SceneError::NoImageToAnimate)
        ));
        assert_eq!(
            SceneError::NoImageToAnimate.message(),
            Text::SceneNoImageToAnimate
        );
        assert!(matches!(
            app.generate_missing_clips(project.id, BudgetConsent::Ask),
            Err(SceneError::NothingToAnimate)
        ));

        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        app.save_rate(Provider::Higgsfield, "fake/", Meter::VideoSeconds, "0.10")
            .unwrap();
        let view = app.scenes(project.id).unwrap().clips;
        let seconds = view.scenes[0].seconds.unwrap();
        assert_eq!(
            view.scenes[0].price,
            Some(Money::from_cents(10).times(u64::from(seconds)))
        );
        let all: u64 = view
            .scenes
            .iter()
            .map(|s| u64::from(s.seconds.unwrap()))
            .sum();
        assert_eq!(
            view.missing_estimate.unwrap().total(),
            Money::from_cents(10).times(all)
        );

        app.set_budget(Provider::Higgsfield, "0.20").unwrap();
        let error = app
            .generate_missing_clips(project.id, BudgetConsent::Ask)
            .unwrap_err();
        let SceneError::OverBudget(estimate) = error else {
            panic!("expected the budget question, got {error:?}");
        };
        assert_eq!(estimate.over_budget().count(), 1);
        assert!(h.clips.submissions().is_empty());
        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
                .unwrap(),
        );

        app.remove_provider_key(Provider::Higgsfield).unwrap();
        let error = app
            .generate_scene_clip(project.id, 1, BudgetConsent::Confirmed)
            .unwrap_err();
        assert!(matches!(
            error,
            SceneError::MissingKey(Provider::Higgsfield)
        ));
        assert_eq!(error.message(), Text::ScenesMissingHiggsfieldKey);
    }

    #[test]
    fn a_scene_being_animated_holds_its_clip_and_the_plan() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        h.clips.hold(true);
        let id = app
            .generate_scene_clip(project.id, 1, BudgetConsent::Ask)
            .unwrap();

        let view = app.scenes(project.id).unwrap();
        assert!(view.is_animating());
        assert!(view.clips.scenes[1].is_busy());
        assert_eq!(view.clips.missing, [0, 2], "the busy scene is left out");
        assert!(matches!(
            app.generate_scene_clip(project.id, 1, BudgetConsent::Ask),
            Err(SceneError::Busy)
        ));
        assert!(matches!(
            app.plan_scenes(project.id, true, BudgetConsent::Ask),
            Err(SceneError::Busy)
        ));
        app.cancel_job(id).unwrap();
        wait_done(&app, id);
        h.clips.hold(false);

        // Retrying the cancelled job picks up the request it sent.
        app.retry_job(id).unwrap();
        done(&app, id);
        assert_eq!(h.clips.submissions().len(), 1);
    }

    #[test]
    fn the_still_image_can_replace_a_clip_and_a_new_clip_can_be_discarded() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        let first = app.accept_scene_clip(project.id, 0).unwrap().scenes()[0]
            .clip()
            .unwrap()
            .clone();
        assert!(matches!(
            app.accept_scene_clip(project.id, 0),
            Err(SceneError::NoClipToReview(_))
        ));

        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        let second = plan_of(&app, &project).scenes()[0]
            .pending_clip()
            .unwrap()
            .clone();
        let plan = app.reject_scene_clip(project.id, 0).unwrap();
        assert_eq!(plan.scenes()[0].clip(), Some(&first));
        assert!(!h.files.exists(project.id, &second.file));

        let plan = app.use_scene_still(project.id, 0).unwrap();
        assert_eq!(plan.scenes()[0].clip(), None);
        assert!(plan.scenes()[0].image().is_some());
        assert!(!h.files.exists(project.id, &first.file));
        assert_eq!(plan_of(&app, &project).scenes()[0].clip(), None);
    }

    #[test]
    fn a_new_image_marks_the_clip_stale() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        app.accept_scene_clip(project.id, 0).unwrap();
        done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        assert!(!plan_of(&app, &project).scenes()[0].is_clip_stale());

        let plan = app.accept_scene_image(project.id, 0).unwrap();

        assert!(plan.scenes()[0].is_clip_stale());
    }

    #[test]
    fn motion_prompts_fall_back_to_the_image_prompt() {
        let h = Harness::new();
        let app = h.start();
        let (project, planned) = h.drawn_project(&app);
        let image_prompt = planned.scenes()[0].prompt().clone();

        let plan = app
            .edit_scene_motion_prompt(project.id, 0, "  Slow push in  ")
            .unwrap();
        assert_eq!(plan.scenes()[0].motion_prompt().as_str(), "Slow push in");
        let plan = app.edit_scene_motion_prompt(project.id, 0, "   ").unwrap();
        assert_eq!(plan.scenes()[0].own_motion_prompt(), None);
        assert_eq!(plan.scenes()[0].motion_prompt(), &image_prompt);
        let too_long = "x".repeat(10_000);
        assert!(matches!(
            app.edit_scene_motion_prompt(project.id, 0, &too_long),
            Err(SceneError::Invalid(_))
        ));
    }
}
