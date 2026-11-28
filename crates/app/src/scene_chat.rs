//! Scene chat use cases (#368, ADR-0049): the user asks, in their own
//! words, for a change to a scene's image; Claude answers with what it
//! understood (a question, or a proposal of options with the English
//! instruction each would send); applying an option starts a paid job
//! whose result waits for review like every new image.
//!
//! Sending pays for Claude's reply only. The reply is kept on its job from
//! the moment it is paid for until it is saved in the chat (`Kept`), so a
//! cancel or a close never pays for it twice (ADR-0036). One reply is
//! written at a time per scene (`Work::SceneChat`).

use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, CHAT_HISTORY_MESSAGES, ChatApplyError, ChatOption, ChatOptionKind, ChatOutcome,
    ChatReply, ChatRequestError, ChatTarget, CostPurpose, GeneratedText, GenerationId, ImageFormat,
    Job, JobFailure, JobFailureKind, JobId, JobKind, JobState, ProfileId, ProjectFiles, Proposal,
    Provider, RenderedPrompt, RepositoryError, SceneChatMessage, SceneChatMessageId,
    SceneChatRepository, ScenePlan, ScenePlanId, ScenePlanRepository, SecretStore, TemplateKind,
    TemplateValues, TemplateVariable, TemplateVersion, TextEffort, TextFormat, TextGenerator,
    TextImage, TextRequest, UiLanguage, VideoProject, VideoProjectId,
};
use bardo_media::MediaEngine;
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler, Kept, Work};
use crate::scenes::{clock, id, images_call, parse, to_json, unexpected};
use crate::{Bardo, KeyState, SceneError, ScriptError, TemplateError, Text};

/// What Claude answers with: a reply for the user, and a question or the
/// options it proposes.
const ANSWER_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "reply": {"type": "string"},
    "kind": {"type": "string", "enum": ["question", "proposal"]},
    "options": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "kind": {"type": "string", "enum": ["edit_image"]},
          "instruction": {"type": "string"},
          "prompt": {"type": "string"}
        },
        "required": ["kind", "instruction", "prompt"],
        "additionalProperties": false
      }
    },
    "recommended": {"type": "integer"}
  },
  "required": ["reply", "kind", "options", "recommended"],
  "additionalProperties": false
}"#;

#[derive(Deserialize)]
struct Answer {
    reply: String,
    kind: String,
    options: Vec<AnswerOption>,
    recommended: i64,
}

#[derive(Deserialize)]
struct AnswerOption {
    kind: String,
    instruction: String,
    prompt: String,
}

/// The longest a reply shown in the chat may be; a longer one is cut.
const MAX_REPLY_CHARS: usize = 1_000;

/// The longest an option's instruction or prompt may be: what the image
/// model is sent, as a scene's prompt may be (`ScenePrompt`).
const MAX_OPTION_CHARS: usize = 4_000;

#[derive(Debug, thiserror::Error)]
pub enum SceneChatError {
    #[error(transparent)]
    Scenes(#[from] SceneError),
    /// The typed request breaks a rule; the chat shows it.
    #[error("invalid request: {0:?}")]
    Invalid(ChatRequestError),
    /// A reply is already being written for this scene.
    #[error("a reply is being written for this scene")]
    Busy,
    /// The scene has no image to adjust yet.
    #[error("the scene has no image to adjust")]
    NoImage,
    /// A composed scene's picture never goes to a model (ADR-0040).
    #[error("a composed scene is not adjusted by a model")]
    Composed,
    #[error(transparent)]
    Apply(#[from] ChatApplyError),
    /// The message is gone: the scenes were planned again.
    #[error("message not found")]
    MessageNotFound,
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// Sending would reach a budget; the chat asks before sending with
    /// `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl SceneChatError {
    /// What the chat says.
    pub fn message(&self) -> Text {
        match self {
            SceneChatError::Scenes(error) => error.message(),
            SceneChatError::Invalid(ChatRequestError::Empty) => Text::SceneChatEmpty,
            SceneChatError::Invalid(ChatRequestError::TooLong) => Text::SceneChatTooLong,
            SceneChatError::Busy => Text::SceneChatBusy,
            SceneChatError::NoImage => Text::SceneChatNoImage,
            SceneChatError::Composed => Text::SceneChatComposed,
            SceneChatError::Apply(ChatApplyError::OutOfDate) => Text::SceneChatOutOfDate,
            SceneChatError::Apply(ChatApplyError::AlreadyUsed) => Text::SceneChatAlreadyUsed,
            SceneChatError::Apply(_) | SceneChatError::MessageNotFound => Text::SceneChatNoOption,
            SceneChatError::MissingKey(_) => Text::SceneChatMissingKey,
            SceneChatError::OverBudget(_) => Text::BudgetReachedTitle,
            SceneChatError::Template(error) => error.message(),
            SceneChatError::Repository(_) => Text::SceneChatNotSaved,
        }
    }

    /// The estimate to ask about, for a request past a budget.
    pub fn over_budget(&self) -> Option<&SpendEstimate> {
        match self {
            SceneChatError::OverBudget(estimate)
            | SceneChatError::Scenes(SceneError::OverBudget(estimate)) => Some(estimate),
            _ => None,
        }
    }
}

/// Whether a scene's chat takes requests now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAvailability {
    Open,
    /// It has no image yet.
    NoImage,
    /// Bardo composes its picture (ADR-0040).
    Composed,
    /// The plan is out of date (#262): nothing paid is made for it.
    OutOfDate,
}

/// Where an applied option stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppliedState {
    /// Its job is queued or drawing.
    Working,
    /// The new image waits for review.
    Review,
    Used,
    Kept,
    /// Its job failed or was cancelled, with why when it said.
    Failed(Option<JobFailure>),
    /// Its job ended and the image it made is not waiting: another drawing
    /// took its place, or it drew nothing.
    Ended,
}

/// One message of the chat, as the screen shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatLine {
    pub message: SceneChatMessage,
    /// Where the option the user applied stands.
    pub applied: Option<AppliedState>,
    /// Whether its options still apply: proposed for the scene's current
    /// image, and not used already.
    pub applies: bool,
}

/// A scene's chat.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneChatView {
    pub scene: usize,
    pub lines: Vec<ChatLine>,
    /// The latest scene chat job of this scene: a reply on its way, or why
    /// the last one failed.
    pub job: Option<Job>,
    pub availability: ChatAvailability,
    /// What sending a request costs: Claude's reply.
    pub send_estimate: SpendEstimate,
    /// What applying *Edit this image* costs.
    pub edit_estimate: SpendEstimate,
    /// The template version the next reply is written with.
    pub template: u32,
}

impl SceneChatView {
    /// Whether a reply is on its way.
    pub fn is_answering(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }

    /// Why the last reply did not come, when it failed and nothing came
    /// after.
    pub fn failure(&self) -> Option<&JobFailure> {
        self.job
            .as_ref()
            .filter(|job| matches!(job.state(), JobState::Failed))
            .and_then(|job| job.failure())
            .filter(|_| {
                // A request answered after the failed job (a retry) is not
                // failed any more.
                let job = self.job.as_ref().map(Job::id);
                !self
                    .lines
                    .iter()
                    .any(|line| !line.message.is_request() && line.message.job == job)
            })
    }
}

/// The scene chat job's payload: the request it answers, and the template
/// filled when the request was sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ChatPayload {
    project: String,
    plan: String,
    scene: usize,
    request: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
}

/// The Claude call of a reply.
pub(crate) fn chat_call() -> PlannedCall {
    PlannedCall::new(Provider::Claude, CostPurpose::SceneChat, 1)
}

/// The language a reply is written in.
fn language_name(language: UiLanguage) -> &'static str {
    match language {
        UiLanguage::PtBr => "Brazilian Portuguese",
        UiLanguage::EnUs => "American English",
    }
}

/// The scene, as Claude reads it.
fn scene_text(plan: &ScenePlan, index: usize) -> Result<String, SceneError> {
    let scene = plan.scene(index)?;
    let mut text = format!(
        "Scene {} of {}, from {} to {}.\nNarration over it: {}\nImage prompt: {}",
        index + 1,
        plan.scenes().len(),
        clock(scene.start),
        clock(scene.end),
        scene.text.trim(),
        scene.prompt().as_str(),
    );
    if let Some(place) = plan
        .scene_place(index)
        .and_then(|place| plan.place(place).ok())
    {
        text.push_str(&format!("\nPlace: {}", place.setting()));
    }
    let people: Vec<String> = plan
        .scene_cast(index)
        .into_iter()
        .filter_map(|member| plan.cast_member(member).ok())
        .map(|member| format!("- {}: {}", member.name, member.description().as_str()))
        .collect();
    if !people.is_empty() {
        text.push_str("\nPeople in it:\n");
        text.push_str(&people.join("\n"));
    }
    Ok(text)
}

/// The chat's last messages, oldest first, as Claude reads them.
fn history_text(messages: &[SceneChatMessage]) -> String {
    let recent = &messages[messages.len().saturating_sub(CHAT_HISTORY_MESSAGES)..];
    if recent.is_empty() {
        return "none yet".to_owned();
    }
    recent
        .iter()
        .map(|message| {
            let who = if message.is_request() { "User" } else { "You" };
            let mut line = format!("{who}: {}", message.text);
            if let Some(applied) = message.applied() {
                let outcome = match applied.outcome {
                    Some(ChatOutcome::Used) => "the user applied it and used the new image",
                    Some(ChatOutcome::Kept) => "the user applied it and kept the old image",
                    None => "the user applied it",
                };
                line.push_str(&format!(" ({outcome})"));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The options a request may be answered with, each with its price.
fn options_text(edit_price: &str) -> String {
    format!(
        "- `edit_image` (Edit this image, {edit_price}): the image model changes the current \
         image as the instruction says and keeps the rest. Fits any change to what the picture \
         shows: a color, an object, a person's expression or clothes, the light, a detail."
    )
}

/// The project's image `file` as Claude takes it: a JPEG small copy when
/// it is large, as the image requests send it.
fn chat_image(
    files: &dyn ProjectFiles,
    media: &dyn MediaEngine,
    project: VideoProjectId,
    file: &str,
) -> Result<TextImage, JobFailure> {
    if let Some(bytes) = crate::scenes::small_copy(files, media, project, file) {
        return Ok(TextImage {
            bytes,
            format: ImageFormat::Jpeg,
        });
    }
    let format = crate::scenes::saved_format(file).ok_or_else(|| {
        JobFailure::new(
            JobFailureKind::Files,
            "the scene's image is not one Bardo saved",
        )
    })?;
    let bytes = files.read(project, file).map_err(|error| {
        JobFailure::new(
            JobFailureKind::Files,
            format!("the scene's image will not read: {error}"),
        )
    })?;
    Ok(TextImage { bytes, format })
}

/// Cuts `text` to `max` characters.
fn cut(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

/// The reply Claude's answer makes for the scene's image `image`.
fn reply_of(text: &str, image: GenerationId) -> Result<(String, ChatReply), JobFailure> {
    let unusable = |why: String| {
        JobFailure::new(
            JobFailureKind::UnexpectedAnswer,
            format!("Claude: the reply is not usable ({why})"),
        )
    };
    let answer: Answer = serde_json::from_str(text).map_err(|error| unusable(error.to_string()))?;
    let reply_text = cut(&answer.reply, MAX_REPLY_CHARS);
    if reply_text.is_empty() {
        return Err(unusable("it says nothing".to_owned()));
    }
    let reply = match answer.kind.as_str() {
        "question" => ChatReply::Question,
        "proposal" => {
            let mut options: Vec<ChatOption> = Vec::new();
            let mut recommended = 0;
            for (index, option) in answer.options.iter().enumerate() {
                let kind = ChatOptionKind::from_code(&option.kind)
                    .ok_or_else(|| unusable(format!("unknown option {}", option.kind)))?;
                // One option per way: a second one of a kind already
                // offered is left out rather than failing a paid reply,
                // and recommending it recommends the first.
                let at = match options.iter().position(|kept| kept.kind == kind) {
                    Some(at) => at,
                    None => {
                        options.push(ChatOption {
                            kind,
                            instruction: cut(&option.instruction, MAX_OPTION_CHARS),
                            prompt: cut(&option.prompt, MAX_OPTION_CHARS),
                        });
                        options.len() - 1
                    }
                };
                if usize::try_from(answer.recommended).ok() == Some(index) {
                    recommended = at;
                }
            }
            ChatReply::Proposal(
                Proposal::new(options, recommended, image)
                    .map_err(|error| unusable(error.to_string()))?,
            )
        }
        other => return Err(unusable(format!("unknown kind {other}"))),
    };
    Ok((reply_text, reply))
}

/// Runs scene chat jobs.
pub(crate) struct SceneChatHandler {
    pub(crate) owner: ProfileId,
    pub(crate) plans: Arc<dyn ScenePlanRepository>,
    pub(crate) chats: Arc<dyn SceneChatRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    /// Makes the small copy of a large image sent to Claude.
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for SceneChatHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: ChatPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let plan_id: ScenePlanId = id(&payload.plan)?;
        // An earlier run may have saved the reply and stopped before the
        // queue recorded the job as done.
        let chat = self
            .chats
            .scene_chat(plan_id, payload.scene)
            .map_err(unexpected)?;
        if chat
            .iter()
            .any(|message| !message.is_request() && message.job == Some(cx.id()))
        {
            Kept::settle(cx);
            return Ok(());
        }
        // The scenes were planned again: the request went with them.
        let Some(plan) = self
            .plans
            .scene_plan(project)
            .map_err(unexpected)?
            .filter(|plan| plan.id == plan_id)
        else {
            Kept::settle(cx);
            return Ok(());
        };
        let Some(image) = plan
            .scene(payload.scene)
            .ok()
            .and_then(|scene| scene.image().cloned())
        else {
            Kept::settle(cx);
            return Err(JobFailure::new(
                JobFailureKind::Unexpected,
                "the scene has no image any more",
            ));
        };
        let generated = match Kept::answer(cx) {
            // Paid for and recorded when it arrived.
            Some(answer) => answer,
            None => {
                let picture = chat_image(
                    self.files.as_ref(),
                    self.media.as_ref(),
                    project,
                    &image.file,
                )?;
                match self.ask(&payload, project, picture, cx)? {
                    Some(generated) => generated,
                    // Stopped before anything was sent.
                    None => return Ok(()),
                }
            }
        };
        // A reply that arrived as the job was cancelled is paid for: it is
        // shown all the same.
        let (text, reply) = match reply_of(&generated.text, image.generation.id) {
            Ok(reply) => reply,
            Err(failure) => {
                // Asked again on a retry rather than failing on it for good.
                Kept::settle(cx);
                return Err(failure);
            }
        };
        let message = SceneChatMessage::reply(
            plan_id,
            payload.scene,
            &text,
            reply,
            cx.id(),
            SystemTime::now(),
        );
        self.chats.save_chat_message(&message).map_err(unexpected)?;
        Kept::settle(cx);
        Ok(())
    }
}

impl SceneChatHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::Claude)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no Claude key is saved"))
    }

    /// Sends the request to Claude with the scene's image and waits for
    /// the answer, given up as soon as the job is cancelled. The answer is
    /// recorded in Costs and kept on the job before anything is made of
    /// it. `None` when the job was told to stop before anything was sent.
    fn ask(
        &self,
        payload: &ChatPayload,
        project: VideoProjectId,
        picture: TextImage,
        cx: &mut JobContext,
    ) -> Result<Option<GeneratedText>, JobFailure> {
        let key = self.key()?;
        if cx.should_stop() {
            return Ok(None);
        }
        let request = TextRequest {
            effort: TextEffort::Everyday,
            answer_tokens: None,
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            images: vec![picture],
            format: TextFormat::Json {
                schema: ANSWER_SCHEMA.to_owned(),
            },
        };
        let paid_call = PlannedCall::text(CostPurpose::SceneChat, &request);
        let generated = {
            let _paying = self
                .costs
                .paying_for_project(cx.id(), &paid_call, project)?;
            self.text
                .generate_until(&key, &request, &cx.stop())
                .map_err(|failure| {
                    // An answer that was lost or cut may still have been billed.
                    self.costs.record_failed_for_project(
                        &failure,
                        &paid_call,
                        Some(cx.id()),
                        project,
                    );
                    JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
                })
        }?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::SceneChat,
                usage: generated.usage.into(),
                job: Some(cx.id()),
                reported: None,
            },
            project,
        );
        Kept::keep(cx, &generated);
        Ok(Some(generated))
    }
}

impl Bardo {
    fn chat_jobs(&self, project: VideoProjectId, scene: usize) -> Vec<Job> {
        let work = Work::SceneChat {
            project: project.to_string(),
            scene,
        };
        self.jobs()
            .into_iter()
            .filter(|job| Work::of(job).as_ref() == Some(&work))
            .collect()
    }

    fn chat_availability(&self, plan: &ScenePlan, index: usize) -> ChatAvailability {
        let Ok(scene) = plan.scene(index) else {
            return ChatAvailability::NoImage;
        };
        if scene.is_composed() {
            ChatAvailability::Composed
        } else if self.scenes_current(plan).is_err() {
            ChatAvailability::OutOfDate
        } else if scene.image().is_none() {
            ChatAvailability::NoImage
        } else {
            ChatAvailability::Open
        }
    }

    /// Why scene `index` takes no request now, as an error.
    fn chat_open(&self, plan: &ScenePlan, index: usize) -> Result<(), SceneChatError> {
        plan.scene(index).map_err(SceneError::from)?;
        match self.chat_availability(plan, index) {
            ChatAvailability::Open => Ok(()),
            ChatAvailability::NoImage => Err(SceneChatError::NoImage),
            ChatAvailability::Composed => Err(SceneChatError::Composed),
            ChatAvailability::OutOfDate => Err(SceneError::PlanOutOfDate.into()),
        }
    }

    /// The scene chat template filled for a request of scene `index`.
    fn render_chat(
        &self,
        project: &VideoProject,
        plan: &ScenePlan,
        index: usize,
        template: &TemplateVersion,
        request: &str,
    ) -> Result<RenderedPrompt, SceneChatError> {
        let mut values: TemplateValues =
            self.script_values(project).map_err(|error| match error {
                ScriptError::Repository(error) => SceneChatError::Repository(error),
                ScriptError::Template(error) => SceneChatError::Template(error),
                _ => SceneChatError::Scenes(SceneError::ProjectNotFound),
            })?;
        let chat = self.scene_chats.scene_chat(plan.id, index)?;
        let edit = self.estimate(&[images_call(1)])?;
        values.insert(TemplateVariable::Scene, scene_text(plan, index)?);
        values.insert(TemplateVariable::SceneRequest, request.trim().to_owned());
        values.insert(TemplateVariable::SceneChatHistory, history_text(&chat));
        values.insert(
            TemplateVariable::ChatOptions,
            options_text(&self.money(edit.total())),
        );
        values.insert(
            TemplateVariable::ReplyLanguage,
            language_name(self.ui_language()).to_owned(),
        );
        template
            .body
            .render(&values)
            // Every scene chat variable has a value above.
            .map_err(|missing| SceneChatError::Repository(RepositoryError(Box::new(missing))))
    }

    /// Scene `index`'s chat.
    pub fn scene_chat(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<SceneChatView, SceneChatError> {
        let video = self.scenes_project(project)?;
        let plan = self.own_plan(project)?;
        let scene = plan.scene(index).map_err(SceneError::from)?;
        let jobs = self.jobs();
        let image = scene.image().map(|image| image.generation.id);
        let pending_job = scene.pending().and_then(|image| image.generation.job);
        let lines = self
            .scene_chats
            .scene_chat(plan.id, index)?
            .into_iter()
            .map(|message| {
                let applied = message.applied().map(|applied| match applied.outcome {
                    Some(ChatOutcome::Used) => AppliedState::Used,
                    Some(ChatOutcome::Kept) => AppliedState::Kept,
                    None => match jobs.iter().find(|job| job.id() == applied.job) {
                        Some(job) if self.at_work(job) => AppliedState::Working,
                        _ if pending_job == Some(applied.job) => AppliedState::Review,
                        Some(job) if matches!(job.state(), JobState::Failed) => {
                            AppliedState::Failed(job.failure().cloned())
                        }
                        Some(job) if matches!(job.state(), JobState::Cancelled) => {
                            AppliedState::Failed(None)
                        }
                        _ => AppliedState::Ended,
                    },
                });
                // An option whose image is being drawn or waits for review
                // is not applied twice.
                let applies = message
                    .proposal()
                    .is_some_and(|_| message.option_for(0, image).is_ok())
                    && !matches!(applied, Some(AppliedState::Working | AppliedState::Review));
                ChatLine {
                    message,
                    applied,
                    applies,
                }
            })
            .collect();
        let template = self.channel_template(TemplateKind::SceneChat, video.channel)?;
        Ok(SceneChatView {
            scene: index,
            lines,
            job: self.chat_jobs(project, index).into_iter().last(),
            availability: self.chat_availability(&plan, index),
            send_estimate: self.estimate(&[chat_call()])?,
            edit_estimate: self.estimate(&[images_call(1)])?,
            template: template.number,
        })
    }

    /// Sends the user's request about scene `index`'s image: Claude answers
    /// in a job, with a question or a proposal. Past Claude's budget it
    /// needs `consent`.
    pub fn send_scene_chat(
        &self,
        project: VideoProjectId,
        index: usize,
        text: &str,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneChatError> {
        let video = self.scenes_project(project)?;
        let plan = self.own_plan(project)?;
        self.chat_open(&plan, index)?;
        let mut request =
            SceneChatMessage::request(plan.id, index, ChatTarget::Image, text, SystemTime::now())
                .map_err(SceneChatError::Invalid)?;
        let work = Work::SceneChat {
            project: project.to_string(),
            scene: index,
        };
        if self.work_busy(&work) {
            return Err(SceneChatError::Busy);
        }
        if self.work_key_state(Provider::Claude) == KeyState::NotSet {
            return Err(SceneChatError::MissingKey(Provider::Claude));
        }
        let template = self.channel_template(TemplateKind::SceneChat, video.channel)?;
        let rendered = self.render_chat(&video, &plan, index, &template, &request.text)?;
        if let Err(estimate) = self.check_video_budget(project, &[chat_call()], consent)? {
            return Err(SceneChatError::OverBudget(estimate));
        }
        let payload = ChatPayload {
            project: project.to_string(),
            plan: plan.id.to_string(),
            scene: index,
            request: request.id.to_string(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
        };
        let job = Job::new(self.profile.id, JobKind::SceneChat, to_json(&payload));
        request.job = Some(job.id());
        self.scene_chats.save_chat_message(&request)?;
        Ok(self.jobs.enqueue(job)?)
    }

    /// Applies option `option` of the reply `message`: starts the job that
    /// makes it, whose result waits for review. Past Gemini's budget it
    /// needs `consent`.
    pub fn apply_scene_chat_option(
        &self,
        project: VideoProjectId,
        message: SceneChatMessageId,
        option: usize,
        consent: BudgetConsent,
    ) -> Result<JobId, SceneChatError> {
        let plan = self.own_plan(project)?;
        let mut reply = self
            .scene_chats
            .chat_message(message)?
            .filter(|reply| reply.plan == plan.id)
            .ok_or(SceneChatError::MessageNotFound)?;
        let index = reply.scene;
        self.chat_open(&plan, index)?;
        let image = plan
            .scene(index)
            .map_err(SceneError::from)?
            .image()
            .map(|image| image.generation.id);
        let chosen = reply.option_for(option, image)?.clone();
        let from = image.ok_or(SceneChatError::NoImage)?;
        let job = match chosen.kind {
            ChatOptionKind::EditImage => self.edit_scene_image(
                &plan,
                index,
                crate::scenes::ImageEdit {
                    instruction: chosen.instruction,
                    prompt: chosen.prompt,
                    from: from.to_string(),
                },
                consent,
            )?,
        };
        reply.apply(option, job);
        self.scene_chats.save_chat_message(&reply)?;
        Ok(job)
    }

    /// Notes how the review of scene `index`'s new image, made by `job`,
    /// ended, in the reply that applied it; a used image's option makes
    /// its prompt the scene's. Nothing happens for an image no reply
    /// applied.
    pub(crate) fn settle_scene_chat(
        &self,
        plan: &mut ScenePlan,
        index: usize,
        job: Option<JobId>,
        outcome: ChatOutcome,
    ) -> Result<(), SceneError> {
        let Some(job) = job else {
            return Ok(());
        };
        let chat = self.scene_chats.scene_chat(plan.id, index)?;
        for mut reply in chat {
            let Some(applied) = reply.applied() else {
                continue;
            };
            if !reply.settle(job, outcome) {
                continue;
            }
            if outcome == ChatOutcome::Used {
                let prompt = reply
                    .proposal()
                    .and_then(|proposal| proposal.options().get(applied.option))
                    .and_then(|option| bardo_domain::ScenePrompt::new(&option.prompt).ok());
                if let Some(prompt) = prompt
                    && plan
                        .scene_mut(index, SystemTime::now())?
                        .edit_prompt(prompt)
                {
                    self.scene_plans.save_scene(plan, index)?;
                }
            }
            self.scene_chats.save_chat_message(&reply)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
