//! Persona looks (#188): the character a persona appears as in performed
//! clips. A look is drawn or imported:
//!
//! - **Drawn.** Claude writes an image prompt for an original character
//!   from the user's description, through the look prompt template, and
//!   declines a description of a real, identifiable person; then the image
//!   provider draws it in a tall 2:3 frame. Both calls run in one job, are
//!   estimated first and recorded in Costs. The drawing waits for review
//!   beside the current look until the user keeps or discards it.
//! - **Imported.** A PNG or JPEG photo of the user themselves, which the
//!   user confirms; it becomes the look at once.
//!
//! Every look fits what all take models read (`LookImage`): a drawing that
//! does not is rewritten as a JPEG rather than thrown away, and a photo
//! that does not is refused with the reason.

use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, CostPurpose, ImageGenerator, ImageRequest, ImageShape, Job, JobFailure, JobFailureKind,
    JobId, JobKind, LOOK_MAX_BYTES, LookAsset, LookAssetError, LookAssetKind, LookDescriptionError,
    LookDrawing, LookImage, LookImageError, LookSource, PersonaId, PersonaLook, PersonaRepository,
    ProfileId, Provider, RenderedPrompt, RepositoryError, SecretStore, TemplateKind, TemplateUsed,
    TemplateValues, TemplateVariable, TemplateVersion, TemplateVersionId, TextFormat,
    TextGenerator, TextRequest, look_description,
};
use bardo_media::MediaEngine;
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::scenes::{id, to_json, unexpected};
use crate::{Bardo, KeyState, TemplateError, Text};

#[derive(Debug, thiserror::Error)]
pub enum LookError {
    #[error("persona not found")]
    NotFound,
    /// The description cannot be drawn: empty or too long.
    #[error("invalid description: {0}")]
    Description(LookDescriptionError),
    /// Drawing calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// The persona's look is being drawn.
    #[error("the look is already being drawn")]
    Busy,
    /// The calls would reach a provider's budget; the section asks before
    /// starting them with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    /// No drawing is waiting for review.
    #[error("no drawing waits for review")]
    NothingWaiting,
    /// The persona has no look.
    #[error("the persona has no look")]
    NoLook,
    /// An imported photo must be of the user, and they did not say so.
    #[error("the photo was not confirmed to be of the user")]
    NotConfirmed,
    /// The photo's file could not be read.
    #[error("could not read the photo: {0}")]
    Unreadable(String),
    /// The photo is not one every take model reads.
    #[error("the photo cannot be a look: {0}")]
    Image(LookImageError),
    /// The pasted asset id cannot stand for the look.
    #[error("the asset cannot stand for the look: {0}")]
    Asset(LookAssetError),
    /// No take model of the provider takes looks from its asset library.
    #[error("{0} takes no looks from an asset library")]
    NoAssetLibrary(Provider),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl LookError {
    /// What the Look section says.
    pub fn message(&self) -> Text {
        match self {
            LookError::NotFound => Text::PersonaNotFound,
            LookError::Description(error) => Text::LookDescriptionError(*error),
            LookError::MissingKey(Provider::Claude) => Text::LookMissingClaudeKey,
            LookError::MissingKey(_) => Text::LookMissingImageKey,
            LookError::Busy => Text::LookBusy,
            LookError::OverBudget(_) => Text::BudgetReachedTitle,
            LookError::NothingWaiting => Text::LookNothingWaiting,
            LookError::NoLook => Text::LookNone,
            LookError::NotConfirmed => Text::LookNotConfirmed,
            LookError::Unreadable(_) => Text::LookUnreadable,
            LookError::Image(_) => Text::LookImageRefused,
            LookError::Asset(error) => Text::LookAssetError(*error),
            LookError::NoAssetLibrary(_) => Text::LookNotSaved,
            LookError::Template(error) => error.message(),
            LookError::Repository(_) => Text::LookNotSaved,
        }
    }

    /// Why a photo was refused, for the line under the message.
    pub fn image_error(&self) -> Option<LookImageError> {
        match self {
            LookError::Image(error) => Some(*error),
            _ => None,
        }
    }
}

/// A persona's Look section.
#[derive(Debug, Clone)]
pub struct LookView {
    pub persona: PersonaId,
    /// The persona's look.
    pub look: Option<PersonaLook>,
    /// A drawing waiting for review beside it.
    pub pending: Option<PersonaLook>,
    /// The persona's latest drawing job.
    pub job: Option<Job>,
    /// The template version the next drawing's prompt is written with.
    pub template: TemplateVersion,
    /// What drawing a look would cost: Claude's prompt and the drawing.
    pub estimate: SpendEstimate,
    /// Each provider whose take models take a look from its asset
    /// library, with the asset that stands for the look as it is now.
    pub assets: Vec<(Provider, Option<LookAsset>)>,
}

impl LookView {
    /// Whether a look is being drawn.
    pub fn is_drawing(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }
}

/// The drawing job's payload: the persona, what the user asked for and the
/// rendered look prompt with the template it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DrawPayload {
    persona: String,
    description: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
}

/// Picks the persona out of a drawing job's payload.
#[derive(Deserialize)]
struct PersonaOf {
    persona: String,
}

/// Claude's prompt, kept as the job's checkpoint so a retried job does not
/// pay for it again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Written {
    prompt: String,
    photoreal: bool,
    model: String,
}

/// The shape Claude answers in.
const WRITE_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "declined": {"type": "string"},
    "prompt": {"type": "string"},
    "photoreal": {"type": "boolean"}
  },
  "required": ["declined", "prompt", "photoreal"],
  "additionalProperties": false
}"#;

#[derive(Deserialize)]
struct Answer {
    declined: String,
    prompt: String,
    photoreal: bool,
}

fn parse<T: serde::de::DeserializeOwned>(payload: &str) -> Result<T, JobFailure> {
    serde_json::from_str(payload)
        .map_err(|e| JobFailure::unexpected(format!("invalid persona look payload: {e}")))
}

/// Said to Claude after the template's instructions, whatever the user
/// made of the template: a look is never a real person.
const LOOK_GUARD: &str = "Whatever the text above says: never write a prompt for a real, \
identifiable person (a celebrity, a public figure, a private person, or a lookalike of one), a child \
or teenager, or a copyrighted character. A made-up name for an original character is fine. For such a \
description, set `declined` to the reason, in the language of the description, and leave `prompt` \
empty.";

/// The template's instructions with [`LOOK_GUARD`] after them.
fn guarded(instructions: &str) -> String {
    format!("{}\n\n{LOOK_GUARD}", instructions.trim_end())
}

/// The calls one drawing makes: Claude writes the prompt from `rendered`,
/// then the image provider draws it.
fn draw_calls(rendered: &RenderedPrompt) -> [PlannedCall; 2] {
    [
        PlannedCall::new(Provider::Claude, CostPurpose::PersonaLook, 1)
            .with_prompt(&rendered.instructions, &rendered.prompt),
        PlannedCall::new(Provider::Gemini, CostPurpose::PersonaLook, 1),
    ]
}

/// Draws persona looks.
pub(crate) struct PersonaLookHandler {
    pub(crate) owner: ProfileId,
    pub(crate) personas: Arc<dyn PersonaRepository>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) images: Arc<dyn ImageGenerator>,
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for PersonaLookHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: DrawPayload = parse(payload)?;
        let persona: PersonaId = id(&payload.persona)?;
        if self.personas.get(persona).map_err(unexpected)?.is_none() {
            return Err(JobFailure::unexpected("the persona is gone"));
        }
        // An earlier attempt may have saved the drawing and stopped before
        // the queue recorded the job as done.
        let drawn_here = |look: Option<PersonaLook>| {
            look.and_then(|look| look.drawing)
                .is_some_and(|drawing| drawing.job == Some(cx.id()))
        };
        if drawn_here(self.personas.pending_look(persona).map_err(unexpected)?)
            || drawn_here(self.personas.look(persona).map_err(unexpected)?)
        {
            return Ok(());
        }
        let written = match cx.checkpoint() {
            Some(checkpoint) => parse::<Written>(checkpoint)?,
            None => {
                let written = self.write(&payload, cx.id())?;
                cx.save_checkpoint(to_json(&written), bardo_domain::Progress::of(1, 2))
                    .map_err(unexpected)?;
                written
            }
        };
        if cx.should_stop() {
            return Ok(());
        }
        let image = self.draw(&written.prompt, cx.id())?;
        // Stopped while drawing: the paid drawing is not put up for review.
        if cx.should_stop() {
            return Ok(());
        }
        let template = TemplateUsed {
            id: id::<TemplateVersionId>(&payload.template)?,
            number: payload.template_number,
        };
        let drawing = LookDrawing {
            description: payload.description,
            prompt: written.prompt,
            template: Some(template),
            writer_model: written.model,
            image_model: image.1,
            job: Some(cx.id()),
        };
        let look = PersonaLook::drawn(
            persona,
            image.0,
            drawing,
            written.photoreal,
            crate::publications::whole_millis(SystemTime::now()),
        );
        self.personas.save_pending_look(&look).map_err(unexpected)?;
        Ok(())
    }
}

impl PersonaLookHandler {
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

    /// Claude's prompt for the look, or its reason for declining.
    fn write(&self, payload: &DrawPayload, job: JobId) -> Result<Written, JobFailure> {
        let key = self.key(Provider::Claude)?;
        let request = TextRequest {
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            images: Vec::new(),
            format: TextFormat::Json {
                schema: WRITE_SCHEMA.to_owned(),
            },
        };
        let generated = self.text.generate(&key, &request).map_err(|failure| {
            JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
        })?;
        self.costs.record(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::PersonaLook,
                usage: generated.usage.into(),
                job: Some(job),
                reported: None,
            },
            None,
            None,
        );
        let answer: Answer = serde_json::from_str(&generated.text).map_err(|e| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Claude: the look prompt is not usable ({e})"),
            )
        })?;
        let declined = answer.declined.trim();
        if !declined.is_empty() {
            // Claude's own words: the section shows them.
            return Err(JobFailure::new(JobFailureKind::Declined, declined));
        }
        let prompt = answer
            .prompt
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if prompt.is_empty() {
            return Err(JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                "Claude: the look prompt is empty",
            ));
        }
        Ok(Written {
            prompt,
            photoreal: answer.photoreal,
            model: generated.model,
        })
    }

    /// The image provider's drawing of `prompt`, fitted to the look's
    /// limits, and the model that drew it.
    fn draw(&self, prompt: &str, job: JobId) -> Result<(LookImage, String), JobFailure> {
        let key = self.key(Provider::Gemini)?;
        let request = ImageRequest {
            prompt: prompt.to_owned(),
            shape: ImageShape::Portrait,
        };
        let image = self.images.generate(&key, &request).map_err(|failure| {
            JobFailure::new(failure.kind.into(), format!("Gemini: {}", failure.detail))
        })?;
        self.costs.record(
            PaidCall {
                provider: Provider::Gemini,
                model: &image.model,
                purpose: CostPurpose::PersonaLook,
                usage: image.usage,
                job: Some(job),
                reported: None,
            },
            None,
            None,
        );
        let fitted = match LookImage::check(image.bytes.clone()) {
            Ok(fitted) => fitted,
            Err(LookImageError::FileTooLarge(_) | LookImageError::NotAnImage) => {
                self.fit(&image.bytes, image.format.extension())?
            }
            Err(error) => {
                return Err(JobFailure::new(
                    JobFailureKind::UnexpectedAnswer,
                    format!("Gemini: the drawing cannot be a look ({error})"),
                ));
            }
        };
        Ok((fitted, image.model))
    }

    /// The drawing rewritten as a JPEG of the same size within the look's
    /// limit, so a paid drawing is kept rather than thrown away.
    fn fit(&self, bytes: &[u8], extension: &str) -> Result<LookImage, JobFailure> {
        let stem = format!("bardo-look-{}", uuid::Uuid::new_v4().simple());
        let source = std::env::temp_dir().join(format!("{stem}.{extension}"));
        let destination = std::env::temp_dir().join(format!("{stem}.jpg"));
        let fitted = std::fs::write(&source, bytes)
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))
            .and_then(|()| {
                self.media
                    .fit_picture(&source, &destination, LOOK_MAX_BYTES)
                    .map_err(|e| JobFailure::new(JobFailureKind::Media, e.to_string()))
            })
            .and_then(|()| {
                std::fs::read(&destination)
                    .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))
            });
        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&destination);
        tracing::info!("kept a drawn look as a JPEG within the look's limit");
        LookImage::check(fitted?).map_err(|error| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Gemini: the drawing cannot be a look ({error})"),
            )
        })
    }
}

impl Bardo {
    fn look_persona(&self, persona: PersonaId) -> Result<bardo_domain::Persona, LookError> {
        self.personas
            .get(persona)?
            .filter(|found| found.owner == self.profile.id)
            .ok_or(LookError::NotFound)
    }

    /// The persona's drawing job to show: one still running (a retried
    /// older job counts), else the latest.
    pub fn persona_look_job(&self, persona: PersonaId) -> Option<Job> {
        let persona = persona.to_string();
        let jobs: Vec<Job> = self
            .jobs()
            .into_iter()
            .filter(|job| {
                job.kind() == JobKind::PersonaLook
                    && serde_json::from_str::<PersonaOf>(job.payload())
                        .is_ok_and(|of| of.persona == persona)
            })
            .collect();
        let active = jobs.iter().rev().find(|job| job.state().is_active());
        active.or(jobs.last()).cloned()
    }

    fn render_look_prompt(
        &self,
        persona: &bardo_domain::Persona,
        description: &str,
        template: &TemplateVersion,
    ) -> Result<RenderedPrompt, LookError> {
        let values = TemplateValues::from([
            (TemplateVariable::Persona, persona.details.describe()),
            (TemplateVariable::LookDescription, description.to_owned()),
        ]);
        template
            .body
            .render(&values)
            .map_err(|missing| LookError::Repository(RepositoryError(Box::new(missing))))
    }

    /// A stored look, or none when it no longer reads as one (a damaged
    /// row): the section then offers to draw or import a new one, which
    /// replaces it.
    fn stored_look(
        &self,
        look: Result<Option<PersonaLook>, RepositoryError>,
    ) -> Option<PersonaLook> {
        look.unwrap_or_else(|error| {
            tracing::warn!(%error, "a stored persona look could not be read");
            None
        })
    }

    /// The persona's Look section.
    pub fn persona_look(&self, persona: PersonaId) -> Result<LookView, LookError> {
        let found = self.look_persona(persona)?;
        let template = self.current_template(TemplateKind::LookPrompt)?;
        // The estimate does not depend on the description's few words.
        let rendered = self.render_look_prompt(&found, "", &template)?;
        let look = self.stored_look(self.personas.look(persona));
        let mut assets = Vec::new();
        for provider in self.asset_providers() {
            let held = match &look {
                Some(look) => self.held_asset(persona, provider, look)?,
                None => None,
            };
            assets.push((provider, held));
        }
        Ok(LookView {
            persona,
            look,
            pending: self.stored_look(self.personas.pending_look(persona)),
            job: self.persona_look_job(persona),
            estimate: self.estimate(&draw_calls(&rendered))?,
            template,
            assets,
        })
    }

    /// The providers whose take models take looks from their asset
    /// library, in settings order.
    fn asset_providers(&self) -> Vec<Provider> {
        let mut providers: Vec<Provider> = self
            .take_models()
            .iter()
            .filter(|model| model.look_assets)
            .map(|model| model.id.provider())
            .collect();
        providers.dedup();
        providers
    }

    /// The asset `provider`'s library holds for `look`, when it still
    /// stands for it.
    fn held_asset(
        &self,
        persona: PersonaId,
        provider: Provider,
        look: &PersonaLook,
    ) -> Result<Option<LookAsset>, LookError> {
        let print = crate::performed_takes::look_print(look.image.bytes());
        Ok(self
            .personas
            .look_asset(persona, provider)?
            .filter(|asset| asset.holds_for(look, &print)))
    }

    /// Makes the asset `input` (an id, with or without `asset://`) of
    /// `kind` in `provider`'s library stand for the persona's look as it
    /// is now: take models there send it in place of the image. It is never
    /// exported, and a new look needs its own.
    pub fn set_look_asset(
        &self,
        persona: PersonaId,
        provider: Provider,
        kind: LookAssetKind,
        input: &str,
    ) -> Result<LookAsset, LookError> {
        self.look_persona(persona)?;
        if !self.asset_providers().contains(&provider) {
            return Err(LookError::NoAssetLibrary(provider));
        }
        let look = self.personas.look(persona)?.ok_or(LookError::NoLook)?;
        let print = crate::performed_takes::look_print(look.image.bytes());
        let asset = LookAsset::parse(kind, input, look.source, &print).map_err(LookError::Asset)?;
        self.personas
            .save_look_asset(persona, provider, Some(&asset))?;
        tracing::info!(provider = provider.code(), "saved a look asset");
        Ok(asset)
    }

    /// Forgets the asset that stands for the persona's look in
    /// `provider`'s library: take models there send the image again.
    pub fn remove_look_asset(
        &self,
        persona: PersonaId,
        provider: Provider,
    ) -> Result<(), LookError> {
        self.look_persona(persona)?;
        Ok(self.personas.save_look_asset(persona, provider, None)?)
    }

    /// Starts a job in which Claude writes the look's prompt from
    /// `description`, through the current look prompt template, and the
    /// image provider draws it. The drawing waits for review, in place of
    /// any drawing still waiting. Past a provider's budget it needs
    /// `consent`.
    pub fn draw_look(
        &self,
        persona: PersonaId,
        description: &str,
        consent: BudgetConsent,
    ) -> Result<JobId, LookError> {
        let found = self.look_persona(persona)?;
        let description = look_description(description).map_err(LookError::Description)?;
        if self
            .persona_look_job(persona)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(LookError::Busy);
        }
        for provider in [Provider::Claude, Provider::Gemini] {
            if self.provider_key(provider).state == KeyState::NotSet {
                return Err(LookError::MissingKey(provider));
            }
        }
        let template = self.current_template(TemplateKind::LookPrompt)?;
        let mut rendered = self.render_look_prompt(&found, &description, &template)?;
        rendered.instructions = guarded(&rendered.instructions);
        if let Err(estimate) = self.check_budget(&draw_calls(&rendered), consent)? {
            return Err(LookError::OverBudget(estimate));
        }
        let payload = DrawPayload {
            persona: persona.to_string(),
            description,
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
        };
        let job = Job::new(self.profile.id, JobKind::PersonaLook, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Makes the drawing waiting for review the persona's look.
    pub fn keep_drawn_look(&self, persona: PersonaId) -> Result<PersonaLook, LookError> {
        self.look_persona(persona)?;
        if !self.personas.keep_pending_look(persona)? {
            return Err(LookError::NothingWaiting);
        }
        tracing::info!("kept a drawn persona look");
        self.personas
            .look(persona)?
            .ok_or(LookError::NothingWaiting)
    }

    /// Drops the drawing waiting for review; the look stays as it was.
    pub fn discard_drawn_look(&self, persona: PersonaId) -> Result<(), LookError> {
        self.look_persona(persona)?;
        if self.personas.pending_look(persona)?.is_none() {
            return Err(LookError::NothingWaiting);
        }
        Ok(self.personas.remove_pending_look(persona)?)
    }

    /// Makes the photo at `path`, which the user chose and confirmed shows
    /// them (`shows_me`), the persona's look. A drawing waiting for review
    /// stays.
    pub fn import_look(
        &self,
        persona: PersonaId,
        path: &Path,
        shows_me: bool,
    ) -> Result<PersonaLook, LookError> {
        self.look_persona(persona)?;
        if !shows_me {
            return Err(LookError::NotConfirmed);
        }
        let unreadable = |error: std::io::Error| {
            tracing::warn!(%error, "could not read a look photo");
            LookError::Unreadable(error.to_string())
        };
        let file = std::fs::File::open(path).map_err(unreadable)?;
        let size = file.metadata().map_err(unreadable)?.len();
        if size > LOOK_MAX_BYTES {
            return Err(LookError::Image(LookImageError::FileTooLarge(size)));
        }
        let mut bytes = Vec::new();
        file
            // One byte past the limit tells "too large" from "at the limit",
            // should the file grow meanwhile.
            .take(LOOK_MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(unreadable)?;
        let image = LookImage::check(bytes).map_err(LookError::Image)?;
        let look = PersonaLook::imported(
            persona,
            image,
            crate::publications::whole_millis(SystemTime::now()),
        );
        self.personas.save_look(&look)?;
        tracing::info!("imported a photo as a persona look");
        Ok(look)
    }

    /// Removes the persona's look. A drawing waiting for review stays.
    pub fn remove_look(&self, persona: PersonaId) -> Result<(), LookError> {
        self.look_persona(persona)?;
        // A look that no longer reads is removed all the same.
        if matches!(self.personas.look(persona), Ok(None)) {
            return Err(LookError::NoLook);
        }
        Ok(self.personas.remove_look(persona)?)
    }

    /// Marks the persona's drawn look realistic or not: realistic looks need
    /// the networks' synthetic-content disclosure once they perform. A
    /// photo of the user always is.
    pub fn set_look_realistic(
        &self,
        persona: PersonaId,
        realistic: bool,
    ) -> Result<PersonaLook, LookError> {
        self.look_persona(persona)?;
        let mut look = self.personas.look(persona)?.ok_or(LookError::NoLook)?;
        // A photo of the user is a real person: it stays realistic.
        if look.realistic != realistic && look.source == LookSource::Drawn {
            look.realistic = realistic;
            self.personas.save_look(&look)?;
        }
        Ok(look)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use bardo_domain::{
        CostRepository, ImageFormat, JobState, LookSource, ProviderFailure, ProviderFailureKind,
    };

    use super::*;
    use crate::scenes::tests::{Harness, done, wait_done};
    use crate::testing::LOOK_IMAGE;

    const DESCRIPTION: &str = "A lighthouse keeper in her sixties, grey braid, watercolor";

    fn answer(declined: &str, prompt: &str, photoreal: bool) -> String {
        serde_json::json!({"declined": declined, "prompt": prompt, "photoreal": photoreal})
            .to_string()
    }

    fn narrator(app: &Bardo) -> PersonaId {
        app.personas()
            .unwrap()
            .into_iter()
            .find(|p| p.details.name() == "Documentary Narrator (en-US)")
            .unwrap()
            .id
    }

    fn draw(h: &Harness, app: &Bardo, persona: PersonaId) -> Job {
        h.answer(answer(
            "",
            "  An elderly lighthouse keeper,\n watercolor ",
            false,
        ));
        let job = app
            .draw_look(persona, DESCRIPTION, BudgetConsent::Ask)
            .unwrap();
        done(app, job)
    }

    /// A photo file in its own folder, kept alive by the returned folder.
    fn photo(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("me.png");
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0]);
        bytes
    }

    #[test]
    fn a_drawing_waits_for_review_then_becomes_the_look() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        let view = app.persona_look(persona).unwrap();
        assert!(view.look.is_none() && view.pending.is_none() && view.job.is_none());
        assert_eq!(view.estimate.providers.len(), 2);

        let job = draw(&h, &app, persona);
        assert_eq!(job.kind(), JobKind::PersonaLook);
        let request = h.text.requests().pop().unwrap();
        assert!(request.prompt.contains(DESCRIPTION), "{}", request.prompt);
        assert!(request.prompt.contains("Documentary Narrator"));
        assert!(request.instructions.contains("declined"));
        assert!(
            request.instructions.ends_with(LOOK_GUARD),
            "the guard follows whatever the template says"
        );
        assert!(matches!(request.format, TextFormat::Json { .. }));
        let drawn = h.images.requests.lock().unwrap().pop().unwrap();
        assert_eq!(drawn.shape, ImageShape::Portrait);
        assert_eq!(drawn.prompt, "An elderly lighthouse keeper, watercolor");

        let view = app.persona_look(persona).unwrap();
        assert!(view.look.is_none(), "a drawing waits for review");
        assert_eq!(view.job.as_ref().map(Job::id), Some(job.id()));
        let pending = view.pending.unwrap();
        assert_eq!(pending.source, LookSource::Drawn);
        assert!(!pending.realistic);
        assert_eq!(pending.image.bytes(), LOOK_IMAGE);
        let drawing = pending.drawing.unwrap();
        assert_eq!(drawing.description, DESCRIPTION);
        assert_eq!(drawing.prompt, "An elderly lighthouse keeper, watercolor");
        assert_eq!(drawing.job, Some(job.id()));
        assert_eq!(drawing.template.unwrap().id, view.template.id);
        assert_eq!(drawing.image_model, "nano-banana-fake");

        let kept = app.keep_drawn_look(persona).unwrap();
        let view = app.persona_look(persona).unwrap();
        assert_eq!(view.look, Some(kept));
        assert!(view.pending.is_none());
        assert!(matches!(
            app.keep_drawn_look(persona),
            Err(LookError::NothingWaiting)
        ));

        let costs =
            h.db.recent_costs(app.profile().id, CostPurpose::PersonaLook, 10)
                .unwrap();
        let mut providers: Vec<_> = costs.iter().map(|cost| cost.provider).collect();
        providers.sort_by_key(|provider| provider.code());
        assert_eq!(providers, [Provider::Claude, Provider::Gemini]);
        assert!(
            costs
                .iter()
                .all(|cost| cost.channel.is_none() && cost.project.is_none())
        );
    }

    #[test]
    fn drawing_again_replaces_the_one_waiting_and_discarding_keeps_the_look() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        draw(&h, &app, persona);
        let first = app.keep_drawn_look(persona).unwrap();
        let second = draw(&h, &app, persona);
        let third = draw(&h, &app, persona);
        let pending = app.persona_look(persona).unwrap().pending.unwrap();
        assert_eq!(pending.drawing.unwrap().job, Some(third.id()));
        assert_ne!(Some(second.id()), Some(third.id()));

        app.discard_drawn_look(persona).unwrap();
        let view = app.persona_look(persona).unwrap();
        assert_eq!(view.look, Some(first));
        assert!(view.pending.is_none());
        assert!(matches!(
            app.discard_drawn_look(persona),
            Err(LookError::NothingWaiting)
        ));
    }

    #[test]
    fn claude_declines_a_real_person_in_its_own_words() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        h.answer(answer(
            "This describes a real, identifiable person.",
            "",
            false,
        ));
        let job = app
            .draw_look(persona, "The current president", BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, job);
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::Declined);
        assert_eq!(
            failure.detail,
            "This describes a real, identifiable person."
        );
        assert!(
            h.images.requests.lock().unwrap().is_empty(),
            "nothing drawn"
        );
        assert!(app.persona_look(persona).unwrap().pending.is_none());
        assert_eq!(
            h.db.recent_costs(app.profile().id, CostPurpose::PersonaLook, 10)
                .unwrap()
                .len(),
            1,
            "Claude's answer is paid for"
        );
    }

    #[test]
    fn an_empty_prompt_is_an_unexpected_answer() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        h.answer(answer("  ", " \n ", false));
        let job = app
            .draw_look(persona, DESCRIPTION, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, job);
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::UnexpectedAnswer
        );
    }

    #[test]
    fn a_photoreal_drawing_is_realistic() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        h.answer(answer("", "A photo of an original fisherman", true));
        done(
            &app,
            app.draw_look(persona, "A photoreal fisherman", BudgetConsent::Ask)
                .unwrap(),
        );
        assert!(
            app.persona_look(persona)
                .unwrap()
                .pending
                .unwrap()
                .realistic
        );
    }

    #[test]
    fn a_drawing_over_the_file_limit_is_kept_as_a_jpeg() {
        let h = Harness::new();
        h.images.oversized.store(true, Ordering::SeqCst);
        let app = h.start();
        let persona = narrator(&app);
        draw(&h, &app, persona);
        assert_eq!(*h.media.fitted.lock().unwrap(), [LOOK_MAX_BYTES]);
        let pending = app.persona_look(persona).unwrap().pending.unwrap();
        assert_eq!(pending.image.format(), ImageFormat::Jpeg);
        assert_eq!((pending.image.width(), pending.image.height()), (400, 600));
    }

    #[test]
    fn a_retry_reuses_claudes_prompt() {
        let h = Harness::new();
        *h.images.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Unreachable,
            "offline",
        ));
        let app = h.start();
        let persona = narrator(&app);
        h.answer(answer("", "A keeper", false));
        let job = app
            .draw_look(persona, DESCRIPTION, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, job);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(h.text.requests().len(), 1, "the prompt is paid for once");
        assert_eq!(h.images.requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_drawing_needs_a_description_both_keys_and_no_other_drawing() {
        let h = Harness::new();
        let mut app = h.start();
        let persona = narrator(&app);
        for (description, error) in [
            ("   ", LookDescriptionError::Required),
            (&"x".repeat(1_001)[..], LookDescriptionError::TooLong),
        ] {
            assert!(matches!(
                app.draw_look(persona, description, BudgetConsent::Ask),
                Err(LookError::Description(e)) if e == error
            ));
        }
        assert!(matches!(
            app.draw_look(PersonaId::new(), DESCRIPTION, BudgetConsent::Ask),
            Err(LookError::NotFound)
        ));

        *h.text.delay.lock().unwrap() = Duration::from_millis(300);
        let first = app
            .draw_look(persona, DESCRIPTION, BudgetConsent::Ask)
            .unwrap();
        assert!(matches!(
            app.draw_look(persona, DESCRIPTION, BudgetConsent::Ask),
            Err(LookError::Busy)
        ));
        assert!(app.persona_look(persona).unwrap().is_drawing());
        wait_done(&app, first);
        *h.text.delay.lock().unwrap() = Duration::ZERO;

        app.remove_provider_key(Provider::Gemini).unwrap();
        let error = app
            .draw_look(persona, DESCRIPTION, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, LookError::MissingKey(Provider::Gemini)));
        assert_eq!(error.message(), Text::LookMissingImageKey);
        app.remove_provider_key(Provider::Claude).unwrap();
        assert!(matches!(
            app.draw_look(persona, DESCRIPTION, BudgetConsent::Ask),
            Err(LookError::MissingKey(Provider::Claude))
        ));
    }

    #[test]
    fn a_retried_older_job_still_counts_as_drawing() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        *h.images.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "bad key",
        ));
        h.answer(answer("", "A keeper", false));
        let failed = wait_done(
            &app,
            app.draw_look(persona, DESCRIPTION, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(failed.state(), JobState::Failed);
        *h.images.failure.lock().unwrap() = None;
        draw(&h, &app, persona);
        *h.images.delay.lock().unwrap() = Duration::from_millis(300);
        app.retry_job(failed.id()).unwrap();
        let view = app.persona_look(persona).unwrap();
        assert_eq!(view.job.as_ref().map(Job::id), Some(failed.id()));
        assert!(view.is_drawing());
        assert!(matches!(
            app.draw_look(persona, DESCRIPTION, BudgetConsent::Ask),
            Err(LookError::Busy)
        ));
        wait_done(&app, failed.id());
    }

    #[test]
    fn a_photo_of_the_user_becomes_the_look_once_confirmed() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        draw(&h, &app, persona);
        let (_dir, path) = photo(LOOK_IMAGE);
        assert!(matches!(
            app.import_look(persona, &path, false),
            Err(LookError::NotConfirmed)
        ));
        let look = app.import_look(persona, &path, true).unwrap();
        assert_eq!(look.source, LookSource::Imported);
        assert!(look.realistic, "a photo reads as a real person");
        assert!(look.drawing.is_none());
        let view = app.persona_look(persona).unwrap();
        assert_eq!(view.look, Some(look));
        assert!(view.pending.is_some(), "the drawing still waits");
    }

    #[test]
    fn a_photo_the_take_models_cannot_read_is_refused_with_the_reason() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        for (bytes, expected) in [
            (b"GIF89a".to_vec(), LookImageError::NotAnImage),
            (
                png(200, 300),
                LookImageError::TooSmall {
                    width: 200,
                    height: 300,
                },
            ),
            (
                png(400, 1_200),
                LookImageError::Shape {
                    width: 400,
                    height: 1_200,
                },
            ),
        ] {
            let (_dir, path) = photo(&bytes);
            let error = app.import_look(persona, &path, true).unwrap_err();
            assert_eq!(error.image_error(), Some(expected));
            assert_eq!(error.message(), Text::LookImageRefused);
        }
        let mut huge = LOOK_IMAGE.to_vec();
        huge.resize(LOOK_MAX_BYTES as usize + 5, 0);
        let (_dir, path) = photo(&huge);
        assert_eq!(
            app.import_look(persona, &path, true)
                .unwrap_err()
                .image_error(),
            Some(LookImageError::FileTooLarge(LOOK_MAX_BYTES + 5)),
            "the file's own size"
        );
        let missing = app
            .import_look(persona, Path::new("/no/such/photo.png"), true)
            .unwrap_err();
        assert!(matches!(missing, LookError::Unreadable(_)));
        assert!(app.persona_look(persona).unwrap().look.is_none());
    }

    #[test]
    fn a_look_can_be_marked_realistic_and_removed() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        assert!(matches!(
            app.set_look_realistic(persona, true),
            Err(LookError::NoLook)
        ));
        assert!(matches!(app.remove_look(persona), Err(LookError::NoLook)));
        draw(&h, &app, persona);
        app.keep_drawn_look(persona).unwrap();
        assert!(app.set_look_realistic(persona, true).unwrap().realistic);
        assert!(app.persona_look(persona).unwrap().look.unwrap().realistic);
        app.remove_look(persona).unwrap();
        assert!(app.persona_look(persona).unwrap().look.is_none());

        let (_dir, path) = photo(LOOK_IMAGE);
        app.import_look(persona, &path, true).unwrap();
        assert!(
            app.set_look_realistic(persona, false).unwrap().realistic,
            "a photo of the user stays realistic"
        );
    }

    #[test]
    fn a_drawn_look_travels_with_the_persona_and_a_photo_stays() {
        let h = Harness::new();
        let app = h.start();
        let persona = narrator(&app);
        draw(&h, &app, persona);
        let drawn = app.keep_drawn_look(persona).unwrap();

        let copy = app.duplicate_persona(persona).unwrap();
        let copied = app.persona_look(copy.id).unwrap().look.unwrap();
        assert_eq!(copied, drawn.for_persona(copy.id));

        let package = app.export_persona(persona).unwrap();
        assert!(package.contains("\"version\": 2"));
        let imported = app.import_persona(&package).unwrap();
        let view = app.persona_look(imported.id).unwrap();
        assert!(view.look.is_none(), "a packaged look waits for review");
        let look = view.pending.unwrap();
        assert_eq!(look.image, drawn.image);
        assert_eq!(look.source, LookSource::Drawn);
        let drawing = look.drawing.unwrap();
        assert_eq!(drawing.description, DESCRIPTION);
        assert_eq!(drawing.prompt, drawn.drawing.as_ref().unwrap().prompt);
        assert_eq!(drawing.job, None, "no ids from the other machine");

        let (_dir, path) = photo(LOOK_IMAGE);
        app.import_look(persona, &path, true).unwrap();
        let package = app.export_persona(persona).unwrap();
        assert!(package.contains("\"version\": 1"));
        assert!(!package.contains("\"look\""));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("narrator.bardo-persona");
        let export = app.export_persona_to(persona, &file).unwrap();
        assert!(export.photo_left_out);
        let export = app.export_persona_to(copy.id, &file).unwrap();
        assert!(!export.photo_left_out);
    }
}
