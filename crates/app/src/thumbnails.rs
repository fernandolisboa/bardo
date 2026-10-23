//! YouTube thumbnails (#182, ADR-0021): a long video's thumbnail is a
//! background picture with a short text drawn over it. Claude writes the
//! text and the prompt the image provider draws a background from; the user
//! picks a background (one of the scene images or a drawn one), edits the
//! text, picks where it sits and how it looks, and saves the picture. The
//! saved picture goes with the YouTube upload and export; a picture saved
//! before the design changed is outdated and goes nowhere.
//!
//! Writing calls Claude and drawing calls the image provider, so both run
//! as jobs. Saving composes the picture with ffmpeg, off the UI thread
//! (`ThumbnailComposer`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, AspectRatio, BackgroundSource, CostPurpose, DrawnBackground, Generation, GenerationId,
    ImageGenerator, ImageRequest, Job, JobFailure, JobFailureKind, JobId, JobKind, JobState,
    MAX_THUMBNAIL_TEXT, Network, ProfileId, ProjectFiles, Provider, RenderedPrompt,
    RepositoryError, SecretStore, TemplateKind, TemplateUsed, TemplateValues, TemplateVariable,
    TemplateVersion, TemplateVersionId, TextFormat, TextGenerator, TextPlacement, TextRequest,
    Thumbnail, ThumbnailBackground, ThumbnailDesign, ThumbnailFieldError, ThumbnailLook,
    ThumbnailRepository, VideoFormat, VideoProject, VideoProjectId, thumbnail_text,
};
use bardo_media::MediaEngine;
use bardo_media::ffmpeg::{MediaError, ThumbnailSpec};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, KeyState, ScriptError, TemplateError, Text};

#[derive(Debug, thiserror::Error)]
pub enum ThumbnailError {
    #[error("video project not found")]
    ProjectNotFound,
    /// The typed text breaks a rule; the card shows it.
    #[error("invalid thumbnail text: {0:?}")]
    Invalid(ThumbnailFieldError),
    /// Writing or drawing calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// The thumbnail is being written, or a background drawn.
    #[error("the thumbnail is already being worked on")]
    Busy,
    /// The call would reach a provider's budget; the card asks before
    /// starting it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    /// A background is drawn from the background prompt: write one first.
    #[error("no background prompt")]
    NoPrompt,
    /// The picture needs a background: pick one first.
    #[error("no background picked")]
    NoBackground,
    /// The background is not one of the project's pictures, or its file
    /// is gone.
    #[error("the background is missing")]
    BackgroundMissing,
    #[error("the picture could not be made: {0}")]
    Compose(#[from] MediaError),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ThumbnailError {
    /// What the thumbnail card says.
    pub fn message(&self) -> Text {
        match self {
            ThumbnailError::ProjectNotFound => Text::ProjectNotFound,
            ThumbnailError::Invalid(error) => Text::ThumbnailFieldError(*error),
            ThumbnailError::MissingKey(Provider::Claude) => Text::ThumbnailWriteMissingKey,
            ThumbnailError::MissingKey(_) => Text::ThumbnailDrawMissingKey,
            ThumbnailError::Busy => Text::ThumbnailBusy,
            ThumbnailError::OverBudget(_) => Text::BudgetReachedTitle,
            ThumbnailError::NoPrompt => Text::ThumbnailNoPrompt,
            ThumbnailError::NoBackground => Text::ThumbnailNoBackground,
            ThumbnailError::BackgroundMissing => Text::ThumbnailBackgroundMissing,
            ThumbnailError::Compose(_) => Text::ThumbnailNotComposed,
            ThumbnailError::Template(error) => error.message(),
            ThumbnailError::Repository(_) => Text::ThumbnailNotSaved,
        }
    }
}

impl From<ScriptError> for ThumbnailError {
    /// The project lookups are shared with scripts; only these come back.
    fn from(error: ScriptError) -> Self {
        match error {
            ScriptError::Repository(error) => ThumbnailError::Repository(error),
            ScriptError::Template(error) => ThumbnailError::Template(error),
            _ => ThumbnailError::ProjectNotFound,
        }
    }
}

/// A picture the thumbnail can sit on, and where its file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundChoice {
    pub background: ThumbnailBackground,
    pub path: PathBuf,
}

/// A video project's thumbnail card.
#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailView {
    pub project: VideoProjectId,
    /// Only long videos get a thumbnail of their own: YouTube takes a
    /// Short's from its frames.
    pub long: bool,
    /// What the user designed; the channel's look before anything else.
    pub thumbnail: Thumbnail,
    /// Drawn backgrounds, newest first, then the scenes' images in order.
    pub backgrounds: Vec<BackgroundChoice>,
    /// The saved picture when it shows the design as it is.
    pub picture: Option<PathBuf>,
    /// The project's latest writing job.
    pub write_job: Option<Job>,
    /// The project's latest drawing job.
    pub draw_job: Option<Job>,
    /// The template version the next writing uses.
    pub template: TemplateVersion,
    /// What writing the text and prompt would cost.
    pub write_estimate: SpendEstimate,
    /// What drawing a background would cost.
    pub draw_estimate: SpendEstimate,
}

impl ThumbnailView {
    /// Whether Claude is writing the text, or a background is being drawn.
    pub fn is_busy(&self) -> bool {
        [&self.write_job, &self.draw_job]
            .into_iter()
            .flatten()
            .any(|job| job.state().is_active())
    }

    /// The picked background's file, when it is still there.
    pub fn background(&self) -> Option<&BackgroundChoice> {
        let picked = self.thumbnail.design.background.as_ref()?;
        self.backgrounds
            .iter()
            .find(|choice| choice.background == *picked)
    }

    /// Whether a picture was saved and the design changed since.
    pub fn is_outdated(&self) -> bool {
        self.thumbnail.is_outdated()
    }
}

/// What the user changes on the thumbnail card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailEdit {
    pub text: String,
    pub placement: TextPlacement,
    pub look: ThumbnailLook,
    pub background: Option<ThumbnailBackground>,
    /// The prompt the next background is drawn from.
    pub prompt: String,
}

impl ThumbnailEdit {
    /// The thumbnail as it is, to change one thing.
    pub fn of(thumbnail: &Thumbnail) -> Self {
        Self {
            text: thumbnail.design.text.clone(),
            placement: thumbnail.design.placement,
            look: thumbnail.design.look,
            background: thumbnail.design.background.clone(),
            prompt: thumbnail.prompt.clone(),
        }
    }
}

/// The writing job's payload: the rendered prompt, where it came from and
/// the channel's look, for a project with no thumbnail yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WritePayload {
    project: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
    look: String,
}

/// The drawing job's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DrawPayload {
    project: String,
    prompt: String,
}

/// The shape Claude answers in.
const WRITE_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "text": {"type": "string"},
    "background_prompt": {"type": "string"}
  },
  "required": ["text", "background_prompt"],
  "additionalProperties": false
}"#;

#[derive(Deserialize)]
struct Answer {
    text: String,
    background_prompt: String,
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

fn parse<T: serde::de::DeserializeOwned>(payload: &str) -> Result<T, JobFailure> {
    serde_json::from_str(payload)
        .map_err(|e| JobFailure::unexpected(format!("invalid thumbnail payload: {e}")))
}

fn project_id(text: &str) -> Result<VideoProjectId, JobFailure> {
    uuid::Uuid::parse_str(text)
        .map(VideoProjectId::from)
        .map_err(unexpected)
}

/// `text` within the thumbnail's limit: whole words while they fit.
fn fit_text(text: &str) -> String {
    let mut fitted = String::new();
    for word in text.split_whitespace() {
        let longer = if fitted.is_empty() {
            word.to_owned()
        } else {
            format!("{fitted} {word}")
        };
        if longer.chars().count() > MAX_THUMBNAIL_TEXT {
            break;
        }
        fitted = longer;
    }
    fitted
}

/// The drawn background's file in the project folder.
fn background_file(job: JobId, extension: &str) -> String {
    format!("thumbnail-bg-{job}.{extension}")
}

/// Claude's call that writes the thumbnail from `rendered`.
fn write_call(rendered: &RenderedPrompt) -> PlannedCall {
    PlannedCall::new(Provider::Claude, CostPurpose::Thumbnail, 1)
        .with_prompt(&rendered.instructions, &rendered.prompt)
}

/// The image provider's call that draws one background.
fn draw_call() -> PlannedCall {
    PlannedCall::new(Provider::Gemini, CostPurpose::ThumbnailImage, 1)
}

/// Runs the thumbnail's writing and drawing jobs.
pub(crate) struct ThumbnailHandler {
    pub(crate) owner: ProfileId,
    pub(crate) thumbnails: Arc<dyn ThumbnailRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) images: Arc<dyn ImageGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for ThumbnailHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        match cx.kind() {
            JobKind::ThumbnailImage => self.draw(payload, cx.id()),
            _ => self.write(payload, cx.id()),
        }
    }
}

impl ThumbnailHandler {
    fn key(&self, provider: Provider) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, provider)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(
                    JobFailureKind::MissingKey,
                    format!("no {} key is saved", provider.code()),
                )
            })
    }

    /// Writes the text and background prompt `job` asked for, in place of
    /// the thumbnail's.
    pub(crate) fn write(&self, payload: &str, job: JobId) -> Result<(), JobFailure> {
        let payload: WritePayload = parse(payload)?;
        let project = project_id(&payload.project)?;
        let saved = self.thumbnails.thumbnail(project).map_err(unexpected)?;
        // An earlier attempt may have saved it and stopped before the
        // queue recorded the job as done.
        if saved
            .as_ref()
            .and_then(|thumbnail| thumbnail.generation.as_ref())
            .is_some_and(|generation| generation.job == Some(job))
        {
            return Ok(());
        }
        let key = self.key(Provider::Claude)?;
        let request = TextRequest {
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            format: TextFormat::Json {
                schema: WRITE_SCHEMA.to_owned(),
            },
        };
        let generated = self.text.generate(&key, &request).map_err(|failure| {
            JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
        })?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::Thumbnail,
                usage: generated.usage.into(),
                job: Some(job),
                reported: None,
            },
            project,
        );
        let not_usable = |detail: String| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Claude: the thumbnail is not usable ({detail})"),
            )
        };
        let answer: Answer =
            serde_json::from_str(&generated.text).map_err(|e| not_usable(e.to_string()))?;
        let text = fit_text(&answer.text);
        let prompt = answer
            .background_prompt
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() || prompt.is_empty() {
            return Err(not_usable("empty text or prompt".to_owned()));
        }
        let now = SystemTime::now();
        let generation = Generation {
            id: GenerationId::new(),
            owner: self.owner,
            project,
            provider: Provider::Claude,
            model: generated.model,
            template: TemplateUsed {
                id: uuid::Uuid::parse_str(&payload.template)
                    .map(TemplateVersionId::from)
                    .map_err(unexpected)?,
                number: payload.template_number,
            },
            instructions: payload.instructions,
            prompt: payload.prompt,
            output: generated.text,
            usage: generated.usage,
            generated_at: now,
            job: Some(job),
        };
        // Read again: the user may have changed the rest meanwhile.
        let saved = self.thumbnails.thumbnail(project).map_err(unexpected)?;
        let mut thumbnail = saved.unwrap_or_else(|| {
            let look = payload.look.parse().unwrap_or_default();
            Thumbnail::new(project, self.owner, look, now)
        });
        thumbnail.written(text, prompt, generation);
        self.thumbnails
            .save_thumbnail(&thumbnail)
            .map_err(unexpected)
    }

    /// Draws a background from the prompt `job` carries and adds it to the
    /// project's; the first one drawn becomes the background when none is
    /// picked yet.
    pub(crate) fn draw(&self, payload: &str, job: JobId) -> Result<(), JobFailure> {
        let payload: DrawPayload = parse(payload)?;
        let project = project_id(&payload.project)?;
        let drawn = self
            .thumbnails
            .drawn_backgrounds(project)
            .map_err(unexpected)?;
        if drawn.iter().any(|background| background.job == Some(job)) {
            return Ok(());
        }
        let key = self.key(Provider::Gemini)?;
        let request = ImageRequest {
            prompt: payload.prompt.clone(),
            shape: AspectRatio::Landscape.into(),
        };
        let image = self.images.generate(&key, &request).map_err(|failure| {
            JobFailure::new(failure.kind.into(), format!("Gemini: {}", failure.detail))
        })?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Gemini,
                model: &image.model,
                purpose: CostPurpose::ThumbnailImage,
                usage: image.usage,
                job: Some(job),
                reported: None,
            },
            project,
        );
        let file = background_file(job, image.format.extension());
        self.files
            .write(project, &file, &image.bytes)
            .map_err(unexpected)?;
        let now = crate::publications::whole_millis(SystemTime::now());
        let background = DrawnBackground {
            project,
            owner: self.owner,
            file: file.clone(),
            prompt: payload.prompt,
            provider: Provider::Gemini,
            model: image.model,
            job: Some(job),
            drawn_at: now,
        };
        self.thumbnails
            .add_drawn_background(&background)
            .map_err(unexpected)?;
        if let Some(mut thumbnail) = self.thumbnails.thumbnail(project).map_err(unexpected)?
            && thumbnail.design.background.is_none()
        {
            thumbnail.design.background = Some(ThumbnailBackground {
                file,
                source: BackgroundSource::Drawn,
            });
            thumbnail.updated_at = now;
            self.thumbnails
                .save_thumbnail(&thumbnail)
                .map_err(unexpected)?;
        }
        Ok(())
    }
}

/// Composes a thumbnail's picture: slow (ffmpeg), so it runs off the UI
/// thread. `Bardo::thumbnail_composed` records what it made.
pub struct ThumbnailComposer {
    media: Arc<dyn MediaEngine>,
    files: Arc<dyn ProjectFiles>,
    project: VideoProjectId,
    design: ThumbnailDesign,
    spec: ThumbnailSpec,
}

impl ThumbnailComposer {
    /// Makes the picture and saves it in the project folder; returns the
    /// design it shows.
    pub fn run(self) -> Result<ThumbnailDesign, ThumbnailError> {
        let scratch = std::env::temp_dir().join(format!(
            "bardo-thumbnail-{}.jpg",
            uuid::Uuid::new_v4().simple()
        ));
        let composed = self
            .media
            .compose_thumbnail(&self.spec, &scratch)
            .and_then(|()| Ok(std::fs::read(&scratch)?));
        let _ = std::fs::remove_file(&scratch);
        let bytes = composed?;
        self.files
            .write(self.project, &self.design.file_name(), &bytes)
            .map_err(|error| ThumbnailError::Repository(RepositoryError(Box::new(error))))?;
        Ok(self.design)
    }
}

impl Bardo {
    fn thumbnail_project(&self, id: VideoProjectId) -> Result<VideoProject, ThumbnailError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ThumbnailError::ProjectNotFound)
    }

    /// The channel's thumbnail look.
    fn channel_look(&self, project: &VideoProject) -> Result<ThumbnailLook, RepositoryError> {
        Ok(self
            .channels
            .get(project.channel)?
            .map_or(ThumbnailLook::default(), |channel| {
                channel.details.thumbnail_look()
            }))
    }

    /// The project's thumbnail as stored, or a new one in the channel's
    /// look.
    fn stored_thumbnail(&self, project: &VideoProject) -> Result<Thumbnail, ThumbnailError> {
        Ok(match self.thumbnails.thumbnail(project.id)? {
            Some(thumbnail) => thumbnail,
            None => Thumbnail::new(
                project.id,
                self.profile.id,
                self.channel_look(project)?,
                SystemTime::now(),
            ),
        })
    }

    /// The facts the thumbnail template is filled with: the script's, plus
    /// the YouTube title once the metadata is written.
    pub(crate) fn thumbnail_values(
        &self,
        project: &VideoProject,
    ) -> Result<TemplateValues, ThumbnailError> {
        let mut values = self.script_values(project)?;
        let title = self
            .exports
            .video_metadata(project.id)?
            .into_iter()
            .find(|metadata| metadata.network == Network::YouTube)
            .map(|metadata| metadata.title().to_owned())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| project.title.clone());
        values.insert(TemplateVariable::VideoTitle, title);
        Ok(values)
    }

    fn render_thumbnail_prompt(
        &self,
        project: &VideoProject,
        template: &TemplateVersion,
    ) -> Result<RenderedPrompt, ThumbnailError> {
        template
            .body
            .render(&self.thumbnail_values(project)?)
            .map_err(|missing| {
                // Every thumbnail variable has a value above.
                ThumbnailError::Repository(RepositoryError(Box::new(missing)))
            })
    }

    /// The pictures a thumbnail can sit on: drawn backgrounds, newest
    /// first, then the scenes' images in order.
    fn background_choices(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<BackgroundChoice>, RepositoryError> {
        let drawn = self
            .thumbnails
            .drawn_backgrounds(project)?
            .into_iter()
            .map(|drawn| ThumbnailBackground {
                file: drawn.file,
                source: BackgroundSource::Drawn,
            });
        let scenes = self
            .scene_plans
            .scene_plan(project)?
            .map(|plan| {
                plan.scenes()
                    .iter()
                    .filter_map(|scene| scene.image())
                    .map(|image| ThumbnailBackground {
                        file: image.file.clone(),
                        source: BackgroundSource::Scene,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut choices: Vec<BackgroundChoice> = Vec::new();
        for background in drawn.chain(scenes) {
            if self.files.exists(project, &background.file)
                && !choices.iter().any(|c| c.background.file == background.file)
            {
                choices.push(BackgroundChoice {
                    path: self.files.path(project, &background.file),
                    background,
                });
            }
        }
        Ok(choices)
    }

    /// Whether the project makes long videos.
    fn is_long(&self, project: &VideoProject) -> Result<bool, RepositoryError> {
        Ok(self.video_format(project)? == VideoFormat::Long)
    }

    /// The project's thumbnail card.
    pub fn thumbnail(&self, project: VideoProjectId) -> Result<ThumbnailView, ThumbnailError> {
        let project = self.thumbnail_project(project)?;
        let template = self.current_template(TemplateKind::Thumbnail)?;
        let rendered = self.render_thumbnail_prompt(&project, &template)?;
        let thumbnail = self.stored_thumbnail(&project)?;
        let picture = thumbnail
            .picture()
            .filter(|file| self.files.exists(project.id, file))
            .map(|file| self.files.path(project.id, file));
        Ok(ThumbnailView {
            project: project.id,
            long: self.is_long(&project)?,
            backgrounds: self.background_choices(project.id)?,
            picture,
            thumbnail,
            write_job: self.latest_job_of(JobKind::Thumbnail, project.id),
            draw_job: self.latest_job_of(JobKind::ThumbnailImage, project.id),
            write_estimate: self.estimate(&[write_call(&rendered)])?,
            draw_estimate: self.estimate(&[draw_call()])?,
            template,
        })
    }

    fn thumbnail_busy(&self, project: VideoProjectId) -> bool {
        [JobKind::Thumbnail, JobKind::ThumbnailImage]
            .into_iter()
            .filter_map(|kind| self.latest_job_of(kind, project))
            .any(|job| job.state().is_active())
    }

    /// Starts a job in which Claude writes the thumbnail's text and the
    /// prompt of its background from the current thumbnail template. They
    /// replace the current ones. Past Claude's budget it needs `consent`.
    pub fn write_thumbnail(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, ThumbnailError> {
        let project = self.thumbnail_project(project)?;
        if self.thumbnail_busy(project.id) {
            return Err(ThumbnailError::Busy);
        }
        if self.provider_key(Provider::Claude).state == KeyState::NotSet {
            return Err(ThumbnailError::MissingKey(Provider::Claude));
        }
        let template = self.current_template(TemplateKind::Thumbnail)?;
        let rendered = self.render_thumbnail_prompt(&project, &template)?;
        if let Err(estimate) = self.check_budget(&[write_call(&rendered)], consent)? {
            return Err(ThumbnailError::OverBudget(estimate));
        }
        let payload = WritePayload {
            project: project.id.to_string(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
            look: self.channel_look(&project)?.code().to_owned(),
        };
        let payload = serde_json::to_string(&payload).expect("a thumbnail payload serializes");
        let job = Job::new(self.profile.id, JobKind::Thumbnail, payload);
        Ok(self.jobs.enqueue(job)?)
    }

    /// Starts a job that draws a background from the thumbnail's prompt.
    /// Past the image provider's budget it needs `consent`.
    pub fn draw_thumbnail_background(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, ThumbnailError> {
        let project = self.thumbnail_project(project)?;
        if self.thumbnail_busy(project.id) {
            return Err(ThumbnailError::Busy);
        }
        let thumbnail = self.stored_thumbnail(&project)?;
        if thumbnail.prompt.trim().is_empty() {
            return Err(ThumbnailError::NoPrompt);
        }
        if self.provider_key(Provider::Gemini).state == KeyState::NotSet {
            return Err(ThumbnailError::MissingKey(Provider::Gemini));
        }
        if let Err(estimate) = self.check_budget(&[draw_call()], consent)? {
            return Err(ThumbnailError::OverBudget(estimate));
        }
        let payload = DrawPayload {
            project: project.id.to_string(),
            prompt: thumbnail.prompt,
        };
        let payload = serde_json::to_string(&payload).expect("a thumbnail payload serializes");
        let job = Job::new(self.profile.id, JobKind::ThumbnailImage, payload);
        Ok(self.jobs.enqueue(job)?)
    }

    /// Saves the user's changes to the thumbnail's design and background
    /// prompt. The saved picture stays until a new one is saved; it is
    /// outdated once the design changed.
    pub fn edit_thumbnail(
        &self,
        project: VideoProjectId,
        edit: ThumbnailEdit,
    ) -> Result<Thumbnail, ThumbnailError> {
        let project = self.thumbnail_project(project)?;
        let mut thumbnail = self.stored_thumbnail(&project)?;
        let text = if edit.text.trim().is_empty() {
            String::new()
        } else {
            thumbnail_text(&edit.text).map_err(ThumbnailError::Invalid)?
        };
        // A background kept as it was may have left the strip (a scene
        // image drawn again): the composer checks its file is still there.
        if let Some(background) = &edit.background
            && edit.background != thumbnail.design.background
            && !self
                .background_choices(project.id)?
                .iter()
                .any(|choice| choice.background == *background)
        {
            return Err(ThumbnailError::BackgroundMissing);
        }
        let design = ThumbnailDesign {
            text,
            placement: edit.placement,
            look: edit.look,
            background: edit.background,
        };
        let prompt = edit.prompt.split_whitespace().collect::<Vec<_>>().join(" ");
        let stored = self.thumbnails.thumbnail(project.id)?.is_some();
        if stored && design == thumbnail.design && prompt == thumbnail.prompt {
            return Ok(thumbnail);
        }
        thumbnail.design = design;
        thumbnail.prompt = prompt;
        thumbnail.updated_at = crate::publications::whole_millis(SystemTime::now());
        self.thumbnails.save_thumbnail(&thumbnail)?;
        Ok(thumbnail)
    }

    /// What composes the thumbnail's picture as it is designed now.
    pub fn thumbnail_composer(
        &self,
        project: VideoProjectId,
    ) -> Result<ThumbnailComposer, ThumbnailError> {
        let project = self.thumbnail_project(project)?;
        let thumbnail = self.stored_thumbnail(&project)?;
        let background = thumbnail
            .design
            .background
            .as_ref()
            .ok_or(ThumbnailError::NoBackground)?;
        if !self.files.exists(project.id, &background.file) {
            return Err(ThumbnailError::BackgroundMissing);
        }
        Ok(ThumbnailComposer {
            media: Arc::clone(&self.media),
            files: Arc::clone(&self.files),
            project: project.id,
            spec: ThumbnailSpec {
                background: self.files.path(project.id, &background.file),
                text: thumbnail.design.text.clone(),
                placement: thumbnail.design.placement,
                look: thumbnail.design.look,
            },
            design: thumbnail.design,
        })
    }

    /// Records the picture a composer saved for `design`. A design changed
    /// meanwhile keeps the picture it had: the new one is left unused.
    pub fn thumbnail_composed(
        &self,
        project: VideoProjectId,
        design: &ThumbnailDesign,
    ) -> Result<Thumbnail, ThumbnailError> {
        let project = self.thumbnail_project(project)?;
        let mut thumbnail = self.stored_thumbnail(&project)?;
        if thumbnail.design != *design {
            return Ok(thumbnail);
        }
        let file = design.file_name();
        let previous = thumbnail.file.replace(file.clone());
        thumbnail.updated_at = crate::publications::whole_millis(SystemTime::now());
        self.thumbnails.save_thumbnail(&thumbnail)?;
        // A picture an upload or export may still need stays; any other
        // leftover only costs disk space.
        if let Some(previous) = previous.filter(|previous| *previous != file)
            && !self.job_needs(&previous)
        {
            let _ = self.files.remove(project.id, &previous);
        }
        Ok(thumbnail)
    }

    /// The picture YouTube gets for the project: the saved one, when it
    /// shows the design as it is, of a long video.
    pub(crate) fn thumbnail_picture(
        &self,
        project: &VideoProject,
    ) -> Result<Option<String>, RepositoryError> {
        if !self.is_long(project)? {
            return Ok(None);
        }
        Ok(self
            .thumbnails
            .thumbnail(project.id)?
            .and_then(|thumbnail| thumbnail.picture().map(str::to_owned))
            .filter(|file| self.files.exists(project.id, file)))
    }

    /// Whether an upload or export that has not ended for good (waiting,
    /// under way, or failed and so resumable) takes the picture `file`.
    fn job_needs(&self, file: &str) -> bool {
        self.jobs().iter().any(|job| {
            matches!(
                job.state(),
                JobState::Queued | JobState::Running | JobState::Failed
            ) && match job.kind() {
                JobKind::Upload => crate::uploads::sends_thumbnail(job.payload(), file),
                JobKind::Export => crate::export::exports_thumbnail(job.payload(), file),
                _ => false,
            }
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Duration;

    use bardo_domain::{JobState, Meter, Money, TokenUsage};

    use super::*;
    use crate::scenes::tests::{Harness, done, project, wait_done};

    fn answer(text: &str, prompt: &str) -> String {
        serde_json::json!({"text": text, "background_prompt": prompt}).to_string()
    }

    /// Saves a thumbnail on the project's first background; its file.
    pub(crate) fn saved_thumbnail(app: &Bardo, project: VideoProjectId) -> String {
        let view = app.thumbnail(project).unwrap();
        let mut edit = ThumbnailEdit::of(&view.thumbnail);
        edit.text = "Lost".into();
        edit.background = Some(view.backgrounds[0].background.clone());
        app.edit_thumbnail(project, edit).unwrap();
        let design = app.thumbnail_composer(project).unwrap().run().unwrap();
        app.thumbnail_composed(project, &design).unwrap();
        design.file_name()
    }

    fn write(app: &Bardo, project: VideoProjectId) -> Job {
        let id = app.write_thumbnail(project, BudgetConsent::Ask).unwrap();
        wait_done(app, id)
    }

    #[test]
    fn a_new_thumbnail_takes_the_channels_look() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let view = app.thumbnail(project.id).unwrap();
        assert!(view.long);
        assert_eq!(view.thumbnail.design, ThumbnailDesign::default());
        assert_eq!(view.picture, None);
        assert!(view.backgrounds.is_empty());
        assert!(!view.is_busy());

        let channel = app
            .channels()
            .unwrap()
            .into_iter()
            .find(|channel| channel.id == project.channel)
            .unwrap();
        let mut draft = bardo_domain::ChannelDraft::from(&channel.details);
        draft.thumbnail_look = ThumbnailLook::Banner;
        let details = app.update_channel(project.channel, draft).unwrap().details;
        assert_eq!(details.thumbnail_look(), ThumbnailLook::Banner);
        let view = app.thumbnail(project.id).unwrap();
        assert_eq!(view.thumbnail.design.look, ThumbnailLook::Banner);
    }

    #[test]
    fn claude_writes_the_text_and_the_background_prompt() {
        let h = Harness::new();
        h.answer(answer(
            "  Nobody   came back ",
            "A lone probe over a dark planet, empty left side",
        ));
        *h.text.usage.lock().unwrap() = TokenUsage {
            input_tokens: 400,
            output_tokens: 60,
        };
        let app = h.start();
        let project = project(&app);
        let job = write(&app, project.id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(job.kind(), JobKind::Thumbnail);

        let view = app.thumbnail(project.id).unwrap();
        assert_eq!(view.write_job.as_ref().map(Job::id), Some(job.id()));
        let thumbnail = view.thumbnail;
        assert_eq!(thumbnail.design.text, "Nobody came back");
        assert_eq!(
            thumbnail.prompt,
            "A lone probe over a dark planet, empty left side"
        );
        let generation = thumbnail.generation.unwrap();
        let request = h.text.requests().pop().unwrap();
        assert!(matches!(request.format, TextFormat::Json { .. }));
        assert_eq!(generation.prompt, request.prompt);
        assert_eq!(generation.job, Some(job.id()));
        assert_eq!(generation.template.id, view.template.id);
        for fact in [
            "Channel: Space Archives",
            "Video title on YouTube: The probe that never came home",
            "Visual style (aesthetic notes): grainy 1960s film",
        ] {
            assert!(request.prompt.contains(fact), "{fact}: {}", request.prompt);
        }
        assert!(request.instructions.contains("in Portuguese"));
        assert!(!request.prompt.contains("{{"));
    }

    #[test]
    fn every_thumbnail_variable_has_a_value() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let values = app.thumbnail_values(&project).unwrap();
        for variable in TemplateKind::Thumbnail.variables() {
            assert!(values.contains_key(variable), "{variable:?}");
        }
    }

    #[test]
    fn a_long_text_keeps_the_words_that_fit() {
        assert_eq!(
            fit_text("The signal that nobody could explain for fifty years"),
            "The signal that nobody could explain for"
        );
        assert!(fit_text("x".repeat(41).as_str()).is_empty());
        let h = Harness::new();
        h.answer(answer("   ", "A probe"));
        let app = h.start();
        let project = project(&app);
        let job = write(&app, project.id);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::UnexpectedAnswer
        );
    }

    #[test]
    fn a_background_is_drawn_from_the_prompt_and_picked_when_none_is() {
        let h = Harness::new();
        h.answer(answer("Lost signal", "A lone probe, wide"));
        let app = h.start();
        let project = project(&app);
        assert!(matches!(
            app.draw_thumbnail_background(project.id, BudgetConsent::Ask),
            Err(ThumbnailError::NoPrompt)
        ));
        write(&app, project.id);
        let job = done(
            &app,
            app.draw_thumbnail_background(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(job.kind(), JobKind::ThumbnailImage);
        let request = h.images.requests.lock().unwrap()[0].clone();
        assert_eq!(request.prompt, "A lone probe, wide");
        assert_eq!(request.shape, AspectRatio::Landscape.into());

        let view = app.thumbnail(project.id).unwrap();
        assert_eq!(view.backgrounds.len(), 1);
        let drawn = &view.backgrounds[0].background;
        assert_eq!(drawn.source, BackgroundSource::Drawn);
        assert_eq!(drawn.file, format!("thumbnail-bg-{}.png", job.id()));
        assert_eq!(view.thumbnail.design.background.as_ref(), Some(drawn));
        assert_eq!(view.background().map(|c| &c.background), Some(drawn));

        // A second one waits in the strip: the picked one stays.
        let second = done(
            &app,
            app.draw_thumbnail_background(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let view = app.thumbnail(project.id).unwrap();
        assert_eq!(view.backgrounds.len(), 2);
        assert_eq!(
            view.backgrounds[0].background.file,
            format!("thumbnail-bg-{}.png", second.id())
        );
        assert_eq!(view.thumbnail.design.background.as_ref(), Some(drawn));
    }

    #[test]
    fn the_scenes_images_are_backgrounds_too() {
        let h = Harness::new();
        let app = h.start();
        let (project, plan) = h.drawn_project(&app);
        let view = app.thumbnail(project.id).unwrap();
        let files: Vec<_> = view
            .backgrounds
            .iter()
            .map(|choice| choice.background.file.clone())
            .collect();
        let scenes: Vec<_> = plan
            .scenes()
            .iter()
            .map(|scene| scene.image().unwrap().file.clone())
            .collect();
        assert_eq!(files, scenes);
        assert!(
            view.backgrounds
                .iter()
                .all(|choice| choice.background.source == BackgroundSource::Scene)
        );
    }

    #[test]
    fn edits_are_checked_and_saved() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let view = app.thumbnail(project.id).unwrap();
        let scene = view.backgrounds[1].background.clone();
        let mut edit = ThumbnailEdit::of(&view.thumbnail);
        edit.text = "x".repeat(41);
        assert!(matches!(
            app.edit_thumbnail(project.id, edit.clone()),
            Err(ThumbnailError::Invalid(ThumbnailFieldError::TextTooLong))
        ));
        edit.text = "  The   last signal ".into();
        edit.placement = TextPlacement::BottomRight;
        edit.look = ThumbnailLook::Yellow;
        edit.background = Some(ThumbnailBackground {
            file: "elsewhere.png".into(),
            source: BackgroundSource::Scene,
        });
        assert!(matches!(
            app.edit_thumbnail(project.id, edit.clone()),
            Err(ThumbnailError::BackgroundMissing)
        ));
        edit.background = Some(scene.clone());
        edit.prompt = " A probe ".into();
        let saved = app.edit_thumbnail(project.id, edit).unwrap();
        assert_eq!(saved.design.text, "The last signal");
        assert_eq!(saved.prompt, "A probe");
        let stored = app.thumbnail(project.id).unwrap().thumbnail;
        assert_eq!(stored, saved);
        assert_eq!(stored.design.background, Some(scene));
        assert_eq!(stored.design.placement, TextPlacement::BottomRight);
        assert_eq!(stored.design.look, ThumbnailLook::Yellow);
    }

    #[test]
    fn saving_composes_the_picture_and_a_change_outdates_it() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        assert!(matches!(
            app.thumbnail_composer(project.id),
            Err(ThumbnailError::NoBackground)
        ));
        let view = app.thumbnail(project.id).unwrap();
        let mut edit = ThumbnailEdit::of(&view.thumbnail);
        edit.text = "Lost".into();
        edit.background = Some(view.backgrounds[0].background.clone());
        app.edit_thumbnail(project.id, edit.clone()).unwrap();

        let design = app.thumbnail_composer(project.id).unwrap().run().unwrap();
        let (spec, _) = h.media.thumbnails.lock().unwrap()[0].clone();
        assert_eq!(spec.text, "Lost");
        assert_eq!(spec.look, ThumbnailLook::Bold);
        assert_eq!(
            spec.background,
            h.files
                .path(project.id, &view.backgrounds[0].background.file)
        );
        let saved = app.thumbnail_composed(project.id, &design).unwrap();
        let file = design.file_name();
        assert_eq!(saved.picture(), Some(file.as_str()));
        assert!(h.files.exists(project.id, &file));
        let view = app.thumbnail(project.id).unwrap();
        assert_eq!(view.picture, Some(h.files.path(project.id, &file)));
        assert!(!view.is_outdated());
        assert_eq!(app.thumbnail_picture(&project).unwrap(), Some(file.clone()));

        edit.text = "Found".into();
        app.edit_thumbnail(project.id, edit).unwrap();
        let view = app.thumbnail(project.id).unwrap();
        assert!(view.is_outdated());
        assert_eq!(view.picture, None);
        assert_eq!(app.thumbnail_picture(&project).unwrap(), None);
        // A picture made for the old design is not recorded.
        let stale = app.thumbnail_composed(project.id, &design).unwrap();
        assert!(stale.is_outdated());

        let newer = app.thumbnail_composer(project.id).unwrap().run().unwrap();
        app.thumbnail_composed(project.id, &newer).unwrap();
        assert!(!h.files.exists(project.id, &file), "the old picture goes");
        assert!(h.files.exists(project.id, &newer.file_name()));
    }

    #[test]
    fn a_short_gets_no_thumbnail_of_its_own() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let view = app.thumbnail(project.id).unwrap();
        let mut edit = ThumbnailEdit::of(&view.thumbnail);
        edit.background = Some(view.backgrounds[0].background.clone());
        app.edit_thumbnail(project.id, edit).unwrap();
        let design = app.thumbnail_composer(project.id).unwrap().run().unwrap();
        app.thumbnail_composed(project.id, &design).unwrap();
        let short = app
            .set_project_format(project.id, Some(VideoFormat::Short))
            .unwrap();
        assert!(!app.thumbnail(project.id).unwrap().long);
        assert_eq!(app.thumbnail_picture(&short).unwrap(), None);
    }

    #[test]
    fn one_job_at_a_time_and_keys_are_needed() {
        let h = Harness::new();
        h.answer(answer("Lost", "A probe"));
        let app = h.start();
        let project = project(&app);
        *h.text.delay.lock().unwrap() = Duration::from_millis(100);
        let id = app.write_thumbnail(project.id, BudgetConsent::Ask).unwrap();
        assert!(matches!(
            app.write_thumbnail(project.id, BudgetConsent::Ask),
            Err(ThumbnailError::Busy)
        ));
        assert!(matches!(
            app.draw_thumbnail_background(project.id, BudgetConsent::Ask),
            Err(ThumbnailError::Busy)
        ));
        done(&app, id);
        assert!(matches!(
            app.thumbnail(VideoProjectId::new()),
            Err(ThumbnailError::ProjectNotFound)
        ));
        assert_eq!(
            ThumbnailError::MissingKey(Provider::Gemini).message(),
            Text::ThumbnailDrawMissingKey
        );
    }

    #[test]
    fn costs_are_recorded_for_the_video_and_a_resumed_job_pays_once() {
        let h = Harness::new();
        h.answer(answer("Lost", "A probe"));
        let app = h.start();
        app.save_rate(Provider::Claude, "claude-fake", Meter::InputTokens, "4")
            .unwrap();
        *h.text.usage.lock().unwrap() = TokenUsage {
            input_tokens: 500_000,
            output_tokens: 0,
        };
        let project = project(&app);
        assert!(app.thumbnail(project.id).unwrap().write_estimate.total() > Money::ZERO);
        let job = write(&app, project.id);
        let costs = app.costs(app.current_month()).unwrap();
        assert_eq!(costs.videos[0].amount, Money::parse("2").unwrap());

        let handler = ThumbnailHandler {
            owner: app.profile().id,
            thumbnails: Arc::clone(&h.db) as _,
            files: Arc::clone(&h.files) as _,
            text: Arc::clone(&h.text) as _,
            images: Arc::clone(&h.images) as _,
            secrets: Arc::clone(&h.secrets) as _,
            costs: CostBook {
                owner: app.profile().id,
                costs: Arc::clone(&h.db) as _,
                themes: Arc::clone(&h.db) as _,
            },
        };
        let asked = h.text.requests().len();
        handler.write(job.payload(), job.id()).unwrap();
        assert_eq!(h.text.requests().len(), asked, "Claude is asked once");

        let drawn = done(
            &app,
            app.draw_thumbnail_background(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let drawn_before = h.images.prompts().len();
        handler.draw(drawn.payload(), drawn.id()).unwrap();
        assert_eq!(h.images.prompts().len(), drawn_before, "drawn once");
    }
}
