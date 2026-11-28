//! Concept products (#280, ADR-0043): a product of kind *idea* is a
//! concept, whatever pictures it has. With no image of its own it can get a
//! **concept image**:
//!
//! - Claude writes an image prompt from the product's brief, through the
//!   concept prompt template, with a fixed instruction after the template's
//!   text that no edit removes: an original product, with no existing
//!   brand, logo, trade dress or text.
//! - The image provider draws it in a tall 2:3 frame.
//!
//! Both calls run in one job, are estimated and checked against the
//! providers' budgets first, and are recorded in Costs under no channel. The
//! drawing waits for the user's review; the kept one stands where a photo
//! would in the product's ads (`bardo_domain::ad_images`), marked as drawn.

use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, ConceptDrawing, CostPurpose, ImageGenerator, ImageRequest, ImageShape, Job, JobFailure,
    JobFailureKind, JobId, JobKind, PRODUCT_IMAGE_MAX_BYTES, Product, ProductConcept, ProductId,
    ProductImageError, ProductImageId, ProductPicture, ProductRepository, ProfileId, Provider,
    RenderedPrompt, RepositoryError, SecretStore, Stop, TemplateKind, TemplateUsed, TemplateValues,
    TemplateVariable, TemplateVersion, TemplateVersionId, TextEffort, TextFormat, TextGenerator,
    TextRequest, may_draw_concept,
};
use bardo_media::MediaEngine;
use serde::{Deserialize, Serialize};

use crate::ads::as_data;
use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler, Work};
use crate::scenes::{id, to_json, unexpected};
use crate::{Bardo, KeyState, TemplateError, Text};

#[derive(Debug, thiserror::Error)]
pub enum ConceptError {
    #[error("product not found")]
    NotFound,
    /// A linked copy is read-only (#340).
    #[error("a linked copy")]
    Linked,
    /// Only a product of kind *idea* with no image of its own gets one.
    #[error("a concept image is only drawn for an idea with no image")]
    NotOffered,
    /// Drawing calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// A concept image is being drawn for the product.
    #[error("a concept image is already being drawn")]
    Busy,
    /// The calls would reach a provider's budget; the section asks before
    /// starting them with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    /// No drawing waits for review.
    #[error("no drawing waits for review")]
    NothingWaiting,
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ConceptError {
    /// What the Concept image section says.
    pub fn message(&self) -> Text {
        match self {
            ConceptError::NotFound => Text::ProductNotFound,
            ConceptError::Linked => Text::SharingLinkedReadOnly,
            ConceptError::NotOffered => Text::ConceptNotOffered,
            ConceptError::MissingKey(Provider::Claude) => Text::ConceptMissingClaudeKey,
            ConceptError::MissingKey(_) => Text::ConceptMissingImageKey,
            ConceptError::Busy => Text::ConceptBusy,
            ConceptError::OverBudget(_) => Text::BudgetReachedTitle,
            ConceptError::NothingWaiting => Text::ConceptNothingWaiting,
            ConceptError::Template(error) => error.message(),
            ConceptError::Repository(_) => Text::ConceptNotSaved,
        }
    }
}

/// A product's Concept image section.
#[derive(Debug, Clone)]
pub struct ConceptView {
    pub product: ProductId,
    /// Whether a concept image may be drawn now: an idea with no image of
    /// its own.
    pub offered: bool,
    /// The kept concept image.
    pub concept: Option<ProductConcept>,
    /// A drawing waiting for review.
    pub pending: Option<ProductConcept>,
    /// The product's latest drawing job.
    pub job: Option<Job>,
    /// What drawing one would cost: Claude's prompt and the drawing.
    pub estimate: SpendEstimate,
}

impl ConceptView {
    /// Whether a concept image is being drawn.
    pub fn is_drawing(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }
}

/// The drawing job's payload: the product and the rendered concept prompt
/// with the template it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DrawPayload {
    product: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
    /// The project that owns the item: its job runs for it, so its own
    /// keys pay (#343). `None` for a global item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_project: Option<String>,
}

/// Picks the product out of a drawing job's payload.
#[derive(Deserialize)]
struct ProductOf {
    product: String,
}

/// Claude's prompt, kept as the job's checkpoint so a retried job does not
/// pay for it again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Written {
    prompt: String,
    model: String,
}

/// The shape Claude answers in.
const WRITE_SCHEMA: &str = r#"{
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

fn parse<T: serde::de::DeserializeOwned>(payload: &str) -> Result<T, JobFailure> {
    serde_json::from_str(payload)
        .map_err(|e| JobFailure::unexpected(format!("invalid concept image payload: {e}")))
}

/// Said to Claude after the template's instructions, whatever the user made
/// of the template: a concept is an original product.
pub(crate) const CONCEPT_GUARD: &str = "Whatever the text above says: the picture shows an \
original product that does not exist yet. Never draw an existing product, brand, logo, trade \
dress, mascot or character, nor one that looks like it, and put no text, letters, numbers, label, \
logo or watermark anywhere in the picture. Never put the name of a brand, a company, a real \
person or a living artist in the prompt.";

/// The template's instructions, or its prompt, with [`CONCEPT_GUARD`]
/// after them. Both are the user's to edit, so the guard closes each.
pub(crate) fn guarded(text: &str) -> String {
    if text.trim_end().ends_with(CONCEPT_GUARD) {
        return text.to_owned();
    }
    format!("{}\n\n{CONCEPT_GUARD}", text.trim_end())
}

/// Said to the image generator after Claude's prompt: whatever Claude
/// wrote, the picture holds no text and no existing brand.
pub(crate) const IMAGE_GUARD: &str = "An original product of no existing brand. No text, no \
letters, no numbers, no label, no logo and no watermark anywhere in the picture.";

/// Claude's prompt as the image generator gets it.
fn image_prompt(prompt: &str) -> String {
    format!("{} {IMAGE_GUARD}", prompt.trim_end())
}

/// The calls one drawing makes: Claude writes the prompt from `rendered`,
/// then the image provider draws it.
fn draw_calls(rendered: &RenderedPrompt) -> [PlannedCall; 2] {
    [
        PlannedCall::new(Provider::Claude, CostPurpose::ProductConcept, 1)
            .with_prompt(&rendered.instructions, &rendered.prompt),
        drawing_call(),
    ]
}

fn drawing_call() -> PlannedCall {
    PlannedCall::new(Provider::Gemini, CostPurpose::ProductConcept, 1)
}

/// What the concept prompt template reads of the product: its brief as
/// data, never as instructions.
fn concept_values(product: &Product) -> TemplateValues {
    let brief = &product.brief;
    let mut described = format!(
        "Name: {}\nDescription: {}",
        brief.name(),
        brief.description()
    );
    for fact in brief.facts() {
        described.push_str(&format!("\n- {fact}"));
    }
    TemplateValues::from([
        (TemplateVariable::Product, as_data(&described)),
        (TemplateVariable::ProductAudience, as_data(brief.audience())),
    ])
}

/// Draws concept images.
pub(crate) struct ProductConceptHandler {
    pub(crate) owner: ProfileId,
    pub(crate) products: Arc<dyn ProductRepository>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) images: Arc<dyn ImageGenerator>,
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for ProductConceptHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: DrawPayload = parse(payload)?;
        let product: ProductId = id(&payload.product)?;
        // An earlier attempt may have saved the drawing and stopped before
        // the queue recorded the job as done.
        let drawn_here = |concept: Option<ProductConcept>| {
            concept.is_some_and(|concept| concept.drawing.job == Some(cx.id()))
        };
        if drawn_here(self.products.pending_concept(product).map_err(unexpected)?)
            || drawn_here(self.products.concept(product).map_err(unexpected)?)
        {
            return Ok(());
        }
        let written = match cx.checkpoint() {
            Some(checkpoint) => parse::<Written>(checkpoint)?,
            None => {
                self.still_offered(product)?;
                let written = self.write(&payload, cx.id(), &cx.stop())?;
                cx.save_checkpoint(to_json(&written), bardo_domain::Progress::of(1, 2))
                    .map_err(unexpected)?;
                written
            }
        };
        if cx.should_stop() {
            return Ok(());
        }
        // The product may have gained an image, or changed kind, since:
        // nothing more is paid for a drawing that would not be used.
        self.still_offered(product)?;
        let (picture, image_model) = self.draw(&written.prompt, cx.id())?;
        // Stopped while drawing: the paid drawing is not put up for review.
        if cx.should_stop() {
            return Ok(());
        }
        let concept = ProductConcept {
            id: ProductImageId::new(),
            product,
            picture,
            drawing: ConceptDrawing {
                prompt: written.prompt,
                template: Some(TemplateUsed {
                    id: id::<TemplateVersionId>(&payload.template)?,
                    number: payload.template_number,
                }),
                writer_model: written.model,
                image_model,
                job: Some(cx.id()),
            },
            drawn_at: crate::publications::whole_millis(SystemTime::now()),
        };
        self.products
            .save_pending_concept(&concept)
            .map_err(unexpected)?;
        tracing::info!("drew a concept image, waiting for review");
        Ok(())
    }
}

impl ProductConceptHandler {
    /// Fails, before anything is paid, when the product is gone or is no
    /// longer offered a concept image (it has an image, or is no idea).
    fn still_offered(&self, product: ProductId) -> Result<(), JobFailure> {
        let found = self
            .products
            .get(product)
            .map_err(unexpected)?
            .filter(|found| found.owner == self.owner)
            .ok_or_else(|| JobFailure::unexpected("the product is gone"))?;
        let images = self.products.image_count(product).map_err(unexpected)?;
        if !may_draw_concept(found.brief.kind(), images) {
            return Err(JobFailure::unexpected(
                "the product has an image of its own or is no longer an idea",
            ));
        }
        Ok(())
    }

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

    /// Claude's prompt for the concept image.
    fn write(&self, payload: &DrawPayload, job: JobId, stop: &Stop) -> Result<Written, JobFailure> {
        let key = self.key(Provider::Claude)?;
        let request = TextRequest {
            effort: TextEffort::Everyday,
            answer_tokens: None,
            // The fixed instruction travels in the payload, written when
            // the job started; it is added again here so a payload written
            // some other way cannot leave it out.
            instructions: guarded(&payload.instructions),
            prompt: guarded(&payload.prompt),
            images: Vec::new(),
            format: TextFormat::Json {
                schema: WRITE_SCHEMA.to_owned(),
            },
        };
        let paid_call = PlannedCall::text(CostPurpose::ProductConcept, &request);
        let generated = {
            let _paying = self.costs.paying(job, &paid_call, None, None)?;
            self.text
                .generate_until(&key, &request, stop)
                .map_err(|failure| {
                    // An answer that was lost or cut may still have been billed.
                    self.costs
                        .record_failed(&failure, &paid_call, Some(job), None, None);
                    JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
                })
        }?;
        self.costs.record(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::ProductConcept,
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
                format!("Claude: the concept prompt is not usable ({e})"),
            )
        })?;
        let prompt = answer
            .prompt
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if prompt.is_empty() {
            return Err(JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                "Claude: the concept prompt is empty",
            ));
        }
        Ok(Written {
            prompt,
            model: generated.model,
        })
    }

    /// The image provider's drawing of `prompt` as a product picture, and
    /// the model that drew it. No reference image goes with it.
    fn draw(&self, prompt: &str, job: JobId) -> Result<(ProductPicture, String), JobFailure> {
        let key = self.key(Provider::Gemini)?;
        let request = ImageRequest {
            prompt: image_prompt(prompt),
            shape: ImageShape::Portrait,
            references: Vec::new(),
        };
        let paid_call = drawing_call();
        let image = {
            let _paying = self.costs.paying(job, &paid_call, None, None)?;
            self.images.generate(&key, &request).map_err(|failure| {
                // An answer that was lost may still have been billed.
                self.costs
                    .record_failed(&failure, &paid_call, Some(job), None, None);
                JobFailure::new(failure.kind.into(), format!("Gemini: {}", failure.detail))
            })
        }?;
        self.costs.record(
            PaidCall {
                provider: Provider::Gemini,
                model: &image.model,
                purpose: CostPurpose::ProductConcept,
                usage: image.usage,
                job: Some(job),
                reported: None,
            },
            None,
            None,
        );
        let picture = match ProductPicture::image(image.bytes.clone()) {
            Ok(picture) => picture,
            // A paid drawing over the file limit is kept as a JPEG rather
            // than thrown away.
            Err(ProductImageError::FileTooLarge(_)) => {
                self.fit(&image.bytes, image.format.extension())?
            }
            Err(error) => {
                return Err(JobFailure::new(
                    JobFailureKind::UnexpectedAnswer,
                    format!("Gemini: the drawing cannot be a product image ({error})"),
                ));
            }
        };
        Ok((picture, image.model))
    }

    fn fit(&self, bytes: &[u8], extension: &str) -> Result<ProductPicture, JobFailure> {
        let stem = format!("bardo-concept-{}", uuid::Uuid::new_v4().simple());
        let source = std::env::temp_dir().join(format!("{stem}.{extension}"));
        let destination = std::env::temp_dir().join(format!("{stem}.jpg"));
        let fitted = std::fs::write(&source, bytes)
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))
            .and_then(|()| {
                self.media
                    .fit_picture(&source, &destination, PRODUCT_IMAGE_MAX_BYTES)
                    .map_err(|e| JobFailure::new(JobFailureKind::Media, e.to_string()))
            })
            .and_then(|()| {
                std::fs::read(&destination)
                    .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))
            });
        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&destination);
        ProductPicture::image(fitted?).map_err(|error| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Gemini: the drawing cannot be a product image ({error})"),
            )
        })
    }
}

/// The product a concept job draws for, and whether it still has to pay
/// for Claude's prompt (none is in its checkpoint).
fn job_of(job: &Job) -> Option<(String, bool)> {
    let of: ProductOf = serde_json::from_str(job.payload()).ok()?;
    let written = job
        .checkpoint()
        .is_some_and(|checkpoint| serde_json::from_str::<Written>(checkpoint).is_ok());
    Some((of.product, !written))
}

impl Bardo {
    /// The product, refused when it is a linked copy: what changes its
    /// concept image.
    fn concept_to_change(&self, product: ProductId) -> Result<Product, ConceptError> {
        let found = self.concept_product(product)?;
        if self.is_linked_copy(bardo_domain::LibraryItem::Product(product)) {
            return Err(ConceptError::Linked);
        }
        Ok(found)
    }

    fn concept_product(&self, product: ProductId) -> Result<Product, ConceptError> {
        self.products
            .get(product)?
            .filter(|found| found.owner == self.profile.id)
            .ok_or(ConceptError::NotFound)
    }

    /// The product's drawing job to show: one still running (a retried
    /// older job counts), else the latest.
    pub fn product_concept_job(&self, product: ProductId) -> Option<Job> {
        let product = product.to_string();
        let jobs: Vec<Job> = self
            .jobs()
            .into_iter()
            .filter(|job| {
                job.kind() == JobKind::ProductConcept
                    && job_of(job).is_some_and(|(of, _)| of == product)
            })
            .collect();
        let active = jobs.iter().rev().find(|job| job.state().is_active());
        active.or(jobs.last()).cloned()
    }

    /// What a failed or cancelled concept job still has to pay for when run
    /// again: the drawing, and Claude's prompt when it has none yet. It
    /// belongs to no video, so only the providers' budgets apply.
    pub(crate) fn concept_calls_left(&self, job: &Job) -> Option<Vec<PlannedCall>> {
        if job.kind() != JobKind::ProductConcept {
            return None;
        }
        let (_, prompt_left) = job_of(job)?;
        let payload: DrawPayload = serde_json::from_str(job.payload()).ok()?;
        let rendered = RenderedPrompt {
            instructions: payload.instructions,
            prompt: payload.prompt,
        };
        let [write, draw] = draw_calls(&rendered);
        Some(match prompt_left {
            true => vec![write, draw],
            false => vec![draw],
        })
    }

    fn render_concept_prompt(
        &self,
        product: &Product,
        template: &TemplateVersion,
    ) -> Result<RenderedPrompt, ConceptError> {
        let mut rendered = template
            .body
            .render(&concept_values(product))
            .map_err(|missing| ConceptError::Repository(RepositoryError(Box::new(missing))))?;
        rendered.instructions = guarded(&rendered.instructions);
        rendered.prompt = guarded(&rendered.prompt);
        Ok(rendered)
    }

    /// A stored concept, or none when it no longer reads as one (a damaged
    /// row): the section then offers to draw a new one, which replaces it.
    fn stored_concept(
        &self,
        concept: Result<Option<ProductConcept>, RepositoryError>,
    ) -> Option<ProductConcept> {
        concept.unwrap_or_else(|error| {
            tracing::warn!(%error, "a stored concept image could not be read");
            None
        })
    }

    /// The product's Concept image section.
    pub fn product_concept(&self, product: ProductId) -> Result<ConceptView, ConceptError> {
        let found = self.concept_product(product)?;
        let template =
            self.current_template_in(TemplateKind::ConceptPrompt, found.owner_project)?;
        let rendered = self.render_concept_prompt(&found, &template)?;
        let images = self.products.image_count(product)?;
        Ok(ConceptView {
            product,
            offered: may_draw_concept(found.brief.kind(), images),
            concept: self.stored_concept(self.products.concept(product)),
            pending: self.stored_concept(self.products.pending_concept(product)),
            job: self.product_concept_job(product),
            estimate: self.estimate_for(found.owner_project, &draw_calls(&rendered))?,
        })
    }

    /// Starts a job in which Claude writes a concept image's prompt from
    /// the product's brief, through the current concept prompt template and
    /// the fixed instruction after it, and the image provider draws it. The
    /// drawing waits for review, in place of any drawing still waiting.
    /// Only for a product of kind *idea* with no image of its own. Past a
    /// provider's budget it needs `consent`.
    pub fn draw_product_concept(
        &self,
        product: ProductId,
        consent: BudgetConsent,
    ) -> Result<JobId, ConceptError> {
        let found = self.concept_to_change(product)?;
        if !may_draw_concept(found.brief.kind(), self.products.image_count(product)?) {
            return Err(ConceptError::NotOffered);
        }
        if self.work_busy(&Work::Concept(product.to_string())) {
            return Err(ConceptError::Busy);
        }
        for provider in [Provider::Claude, Provider::Gemini] {
            if self.key_state_for(found.owner_project, provider) == KeyState::NotSet {
                return Err(ConceptError::MissingKey(provider));
            }
        }
        let template =
            self.current_template_in(TemplateKind::ConceptPrompt, found.owner_project)?;
        let rendered = self.render_concept_prompt(&found, &template)?;
        if let Err(estimate) =
            self.check_budget_for(found.owner_project, &draw_calls(&rendered), consent)?
        {
            return Err(ConceptError::OverBudget(estimate));
        }
        let payload = DrawPayload {
            product: product.to_string(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
            owner_project: found.owner_project.map(|id| id.to_string()),
        };
        let job = Job::new(self.profile.id, JobKind::ProductConcept, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Makes the drawing waiting for review the product's concept image,
    /// in place of the one kept before.
    pub fn keep_product_concept(&self, product: ProductId) -> Result<ProductConcept, ConceptError> {
        self.concept_to_change(product)?;
        if !self.products.keep_pending_concept(product)? {
            return Err(ConceptError::NothingWaiting);
        }
        tracing::info!("kept a concept image");
        self.products
            .concept(product)?
            .ok_or(ConceptError::NothingWaiting)
    }

    /// Whether `project` is a concept (#280): an ad whose product was of
    /// kind *idea* when it started. Its header and the project list show a
    /// *Concept* badge, whatever the product became since.
    pub fn is_concept_project(&self, project: &bardo_domain::VideoProject) -> bool {
        if !project.is_ad() {
            return false;
        }
        match self.ads.brief(project.id) {
            Ok(brief) => brief.is_some_and(|brief| brief.is_concept()),
            Err(error) => {
                tracing::warn!(%error, "could not read whether a project is a concept");
                false
            }
        }
    }

    /// Drops the drawing waiting for review; the kept one stays.
    pub fn discard_product_concept(&self, product: ProductId) -> Result<(), ConceptError> {
        self.concept_to_change(product)?;
        if !self.products.remove_pending_concept(product)? {
            return Err(ConceptError::NothingWaiting);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        CostRepository, ImageFormat, JobState, ProductDraft, ProductImage, ProductImageKind,
        ProductKind, ProductOwnership, ProviderFailure, ProviderFailureKind,
    };

    use super::*;
    use crate::scenes::tests::{Harness, done, wait_done};

    /// A 832 × 1248 PNG header, as the image provider draws a tall frame.
    fn drawing() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(832_u32.to_be_bytes());
        bytes.extend(1248_u32.to_be_bytes());
        bytes.extend([8, 2, 0, 0, 0]);
        bytes
    }

    fn idea(app: &Bardo, kind: ProductKind) -> Product {
        app.create_product(ProductDraft {
            name: "Pocket loom".into(),
            kind,
            description: "A loom that folds into a pocket.".into(),
            audience: "People who knit on the train".into(),
            facts: "Folds to 10 cm\n".into(),
            offer: String::new(),
            link: String::new(),
            call_to_action: "Join the list".into(),
            ownership: ProductOwnership::Mine,
            must_say: String::new(),
            never_say: String::new(),
        })
        .unwrap()
    }

    fn harness() -> Harness {
        let h = Harness::new();
        *h.images.drawing.lock().unwrap() = Some(drawing());
        h
    }

    fn draw(h: &Harness, app: &Bardo, product: ProductId) -> Job {
        h.answer(
            serde_json::json!({"prompt": "  A folding wooden loom,\n studio light "}).to_string(),
        );
        let job = app
            .draw_product_concept(product, BudgetConsent::Ask)
            .unwrap();
        done(app, job)
    }

    #[test]
    fn a_concept_is_drawn_for_an_idea_reviewed_then_stands_for_a_photo() {
        let h = harness();
        let app = h.start();
        let product = idea(&app, ProductKind::Idea);
        let view = app.product_concept(product.id).unwrap();
        assert!(view.offered);
        assert!(view.concept.is_none() && view.pending.is_none() && view.job.is_none());
        assert_eq!(view.estimate.providers.len(), 2);

        let job = draw(&h, &app, product.id);
        assert_eq!(job.kind(), JobKind::ProductConcept);
        let request = h.text.requests().pop().unwrap();
        assert!(request.prompt.contains("Pocket loom"), "{}", request.prompt);
        assert!(request.prompt.contains("People who knit on the train"));
        assert!(request.instructions.ends_with(CONCEPT_GUARD));
        let drawn = h.images.requests.lock().unwrap().pop().unwrap();
        assert_eq!(drawn.shape, ImageShape::Portrait);
        assert!(drawn.references.is_empty(), "nothing is sent with it");
        assert_eq!(
            drawn.prompt,
            format!("A folding wooden loom, studio light {IMAGE_GUARD}")
        );

        let view = app.product_concept(product.id).unwrap();
        assert!(view.concept.is_none(), "a drawing waits for review");
        let pending = view.pending.unwrap();
        assert_eq!(pending.drawing.job, Some(job.id()));
        assert_eq!(
            pending.drawing.prompt,
            "A folding wooden loom, studio light"
        );
        assert_eq!(pending.drawing.image_model, "nano-banana-fake");
        // Not used while it waits.
        assert!(
            crate::ad_scenes::ad_product_images(h.db.as_ref(), product.id)
                .unwrap()
                .is_empty()
        );

        let kept = app.keep_product_concept(product.id).unwrap();
        assert_eq!(kept.id, pending.id);
        assert!(matches!(
            app.keep_product_concept(product.id),
            Err(ConceptError::NothingWaiting)
        ));
        let used = crate::ad_scenes::ad_product_images(h.db.as_ref(), product.id).unwrap();
        assert_eq!(used.len(), 1);
        assert_eq!(used[0].id, kept.id);
        assert_eq!(used[0].kind, ProductImageKind::Concept, "marked as drawn");

        // Both calls are in Costs, under no channel and no video.
        let costs =
            h.db.recent_costs(app.profile().id, CostPurpose::ProductConcept, 10)
                .unwrap();
        let mut providers: Vec<_> = costs.iter().map(|cost| cost.provider).collect();
        providers.sort_by_key(|provider| provider.code());
        assert_eq!(providers, [Provider::Claude, Provider::Gemini]);
        assert!(
            costs
                .iter()
                .all(|cost| cost.channel.is_none() && cost.project.is_none())
        );

        // An image of its own takes its place, and no other is offered.
        let photo = ProductImage::new(
            product.id,
            ProductPicture::image(drawing()).unwrap(),
            ProductImageKind::Photo,
            SystemTime::now(),
        );
        assert!(h.db.add_image(&photo).unwrap());
        let used = crate::ad_scenes::ad_product_images(h.db.as_ref(), product.id).unwrap();
        assert_eq!(
            used.iter().map(|image| image.id).collect::<Vec<_>>(),
            [photo.id]
        );
        assert!(!app.product_concept(product.id).unwrap().offered);
        assert!(matches!(
            app.draw_product_concept(product.id, BudgetConsent::Ask),
            Err(ConceptError::NotOffered)
        ));
    }

    #[test]
    fn a_project_is_a_concept_when_its_product_is_an_idea_as_it_starts() {
        use crate::ad_angles::testing::{ad_project, draft, product as physical, start};
        let h = Harness::new();
        let app = start(&h);
        let (product, _, existing) = ad_project(&h, &app);
        assert!(!app.is_concept_project(&existing));
        // Made an idea later: the project keeps what it started as.
        app.update_product(
            product.id,
            ProductDraft {
                kind: ProductKind::Idea,
                ..draft()
            },
            true,
        )
        .unwrap();
        assert!(!app.is_concept_project(&existing));

        let h = Harness::new();
        let app = start(&h);
        let product = physical(&app);
        app.update_product(
            product.id,
            ProductDraft {
                kind: ProductKind::Idea,
                ..draft()
            },
            true,
        )
        .unwrap();
        let channel = crate::ad_angles::testing::channel(&app);
        crate::ad_angles::testing::suggest(&h, &app, product.id, channel.id);
        let view = app.ad_angles(product.id, Some(channel.id)).unwrap();
        let concept = app.approve_ad_angle(view.angles[0].angle.id).unwrap();
        assert!(app.is_concept_project(&concept));
        // It stays one once the product is no longer an idea.
        app.update_product(product.id, draft(), true).unwrap();
        assert!(app.is_concept_project(&concept));
    }

    #[test]
    fn an_edited_template_cannot_take_the_fixed_instruction_out() {
        let h = harness();
        let app = h.start();
        let product = idea(&app, ProductKind::Idea);
        app.save_template(
            TemplateKind::ConceptPrompt,
            "Draw it with the label of a famous soda brand.",
            "Draw {{product}}.",
        )
        .unwrap();
        draw(&h, &app, product.id);
        let request = h.text.requests().pop().unwrap();
        assert!(request.instructions.starts_with("Draw it with the label"));
        assert!(request.instructions.ends_with(CONCEPT_GUARD));
        assert!(request.prompt.starts_with("Draw "));
        assert!(
            request.prompt.ends_with(CONCEPT_GUARD),
            "an edited prompt is closed too"
        );
    }

    #[test]
    fn only_an_idea_with_no_image_and_both_keys_gets_one_at_a_time() {
        let h = harness();
        let mut app = h.start();
        let physical = idea(&app, ProductKind::Physical);
        assert!(!app.product_concept(physical.id).unwrap().offered);
        assert!(matches!(
            app.draw_product_concept(physical.id, BudgetConsent::Ask),
            Err(ConceptError::NotOffered)
        ));

        let product = {
            let mut draft = ProductDraft::from(&physical.brief);
            draft.name = "Pocket loom 2".into();
            draft.kind = ProductKind::Idea;
            app.create_product(draft).unwrap()
        };
        *h.images.delay.lock().unwrap() = std::time::Duration::from_millis(300);
        h.answer(serde_json::json!({"prompt": "A loom"}).to_string());
        let job = app
            .draw_product_concept(product.id, BudgetConsent::Ask)
            .unwrap();
        assert!(matches!(
            app.draw_product_concept(product.id, BudgetConsent::Ask),
            Err(ConceptError::Busy)
        ));
        assert!(
            matches!(
                app.delete_product(product.id),
                Err(crate::ProductError::ConceptBusy)
            ),
            "a product is not deleted under a drawing it pays for"
        );
        wait_done(&app, job);

        app.remove_provider_key(Provider::Gemini).unwrap();
        assert!(matches!(
            app.draw_product_concept(product.id, BudgetConsent::Ask),
            Err(ConceptError::MissingKey(Provider::Gemini))
        ));
    }

    #[test]
    fn a_retry_pays_for_the_drawing_alone_and_reuses_claudes_prompt() {
        let h = harness();
        *h.images.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Unreachable,
            "offline",
        ));
        let app = h.start();
        let product = idea(&app, ProductKind::Idea);
        h.answer(serde_json::json!({"prompt": "A loom"}).to_string());
        let job = app
            .draw_product_concept(product.id, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, job);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(h.text.requests().len(), 1, "the prompt is paid for once");
        let left = app.concept_calls_left(&job).unwrap();
        assert_eq!(left.len(), 1, "only the drawing is left to pay for");
        assert_eq!(left[0].provider, Provider::Gemini);

        *h.images.failure.lock().unwrap() = None;
        app.retry_job(job.id()).unwrap();
        let job = wait_done(&app, job.id());
        assert_eq!(job.state(), JobState::Done);
        assert_eq!(h.text.requests().len(), 1);
        assert!(app.product_concept(product.id).unwrap().pending.is_some());
    }

    #[test]
    fn a_drawing_no_longer_offered_stops_before_its_next_paid_call() {
        let h = harness();
        *h.images.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Unreachable,
            "offline",
        ));
        let app = h.start();
        let product = idea(&app, ProductKind::Idea);
        h.answer(serde_json::json!({"prompt": "A loom"}).to_string());
        let job = app
            .draw_product_concept(product.id, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, job);
        assert_eq!(job.state(), JobState::Failed);
        let drawn = h.images.requests.lock().unwrap().len();

        // The idea became a physical product before the retry.
        let mut draft = ProductDraft::from(&product.brief);
        draft.kind = ProductKind::Physical;
        app.update_product(product.id, draft, true).unwrap();
        *h.images.failure.lock().unwrap() = None;
        app.retry_job(job.id()).unwrap();
        let job = wait_done(&app, job.id());
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            h.images.requests.lock().unwrap().len(),
            drawn,
            "nothing more is drawn"
        );
        assert_eq!(h.text.requests().len(), 1);
        assert!(app.product_concept(product.id).unwrap().pending.is_none());
    }

    #[test]
    fn a_drawing_over_the_file_limit_is_kept_as_a_jpeg_and_a_duplicate_keeps_the_concept() {
        let h = harness();
        let mut big = drawing();
        big.resize(PRODUCT_IMAGE_MAX_BYTES as usize + 1, 0);
        *h.images.drawing.lock().unwrap() = Some(big);
        let app = h.start();
        let product = idea(&app, ProductKind::Idea);
        let job = draw(&h, &app, product.id);
        assert_eq!(job.state(), JobState::Done);
        let pending = app.product_concept(product.id).unwrap().pending.unwrap();
        assert_eq!(pending.picture.format(), ImageFormat::Jpeg);

        app.keep_product_concept(product.id).unwrap();
        let copy = app.duplicate_product(product.id).unwrap();
        let copied = app.product_concept(copy.id).unwrap().concept.unwrap();
        assert_ne!(copied.id, pending.id);
        assert_eq!(copied.picture, pending.picture);
    }
}
