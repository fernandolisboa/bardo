//! An ad's end card (#281, ADR-0044): the last scene, composed on the
//! computer like a thumbnail from the product's image, the logo and the
//! lines the user typed, in Bardo's fonts. Free: no model is asked, and
//! nothing here is sent anywhere.

use std::time::SystemTime;

use bardo_domain::{
    AdBrief, AdRepository, AspectRatio, CardDesign, CardFieldError, CardLayout, CardLogoPlace,
    CardRecipe, CardTextPlace, ComposedShot, EndCard, EndCardRepository, JobFailure,
    JobFailureKind, JobId, ProductId, ProductLogo, ProductRepository, RepositoryError, ScenePlanId,
    ThumbnailLook, VideoProjectId, card_fingerprint,
};

use crate::Bardo;
use crate::ad_scenes::{ad_product_images, backdrop_paint, shot_file};
use crate::scenes::{SceneError, SceneHandler, unexpected};

/// What an end card is made from: its design (the stored one, or the one
/// it starts with), the brief and the product's logo.
pub(crate) struct CardInputs {
    pub(crate) design: CardDesign,
    pub(crate) brief: AdBrief,
    pub(crate) logo: Option<ProductLogo>,
}

impl CardInputs {
    /// What `project`'s end card is made from; `None` for a project that
    /// is no ad.
    pub(crate) fn of(
        ads: &dyn AdRepository,
        end_cards: &dyn EndCardRepository,
        products: &dyn ProductRepository,
        project: VideoProjectId,
        product: Option<ProductId>,
    ) -> Result<Option<Self>, RepositoryError> {
        let Some(brief) = ads.brief(project)? else {
            return Ok(None);
        };
        let design = match end_cards.end_card(project)? {
            Some(card) => card.design,
            None => CardDesign::start(brief.text()),
        };
        let logo = match product {
            Some(product) => products.logo(product)?,
            None => None,
        };
        Ok(Some(Self {
            design,
            brief,
            logo,
        }))
    }

    /// The fingerprint of what the card shows.
    pub(crate) fn fingerprint(&self) -> String {
        card_fingerprint(
            &self.design,
            self.brief.text(),
            self.logo.as_ref().map(|logo| logo.picture.bytes()),
        )
    }
}

/// An ad's end card, for its editor in the Scenes stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndCardView {
    /// The design: the one saved, or the one the card starts with.
    pub design: CardDesign,
    /// The Brief stage's offer, which the offer line shows; empty when
    /// the brief has none.
    pub offer: String,
    /// That offer fits the card: one too long is left out whole.
    pub offer_fits: bool,
    /// The product has a logo, which the card shows.
    pub has_logo: bool,
    /// The card scene, when the plan's last scene is one.
    pub scene: Option<usize>,
    /// The card's picture shows another design than this one, or there
    /// is none yet: it is composed again before the video is rendered.
    pub out_of_date: bool,
}

/// What the user typed and chose for the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardDraft {
    pub headline: String,
    pub second_line: String,
    pub offer: bool,
    pub look: ThumbnailLook,
    pub text_place: CardTextPlace,
    pub logo_place: CardLogoPlace,
}

impl From<&CardDesign> for CardDraft {
    fn from(design: &CardDesign) -> Self {
        Self {
            headline: design.headline().to_owned(),
            second_line: design.second_line().to_owned(),
            offer: design.offer,
            look: design.look,
            text_place: design.text_place,
            logo_place: design.logo_place,
        }
    }
}

impl SceneHandler {
    /// Composes the end card of `plan_id`, scene `index`, in `aspect`:
    /// the backdrop, the product's image when the scene shows one and the
    /// logo, set on the computer, then the lines drawn in Bardo's fonts.
    pub(crate) fn compose_end_card(
        &self,
        project: VideoProjectId,
        plan_id: ScenePlanId,
        index: usize,
        aspect: AspectRatio,
        product: ProductId,
    ) -> Result<(), JobFailure> {
        let Some(inputs) = CardInputs::of(
            self.ads.as_ref(),
            self.end_cards.as_ref(),
            self.products.as_ref(),
            project,
            Some(product),
        )
        .map_err(unexpected)?
        else {
            return Ok(());
        };
        let card = inputs.fingerprint();
        let Some(mut plan) = self.saved_plan(project, plan_id)? else {
            return Ok(());
        };
        // The scene waits for this design's picture from now on.
        if plan.set_end_card(card.clone(), SystemTime::now()) == Some(index) {
            self.plans.save_scene(&plan, index).map_err(unexpected)?;
        }
        let Ok(scene) = plan.scene(index) else {
            return Ok(());
        };
        if !scene.is_end_card() || !scene.needs_shot(aspect) {
            return Ok(());
        }
        let wanted = scene.product_image();
        let look = scene.look();
        let composed = self.compose_card_picture(
            project,
            index,
            &inputs,
            wanted,
            look,
            product,
            aspect.into(),
        );
        let (png, tones) = match composed {
            Ok(composed) => composed,
            Err(failure) => {
                if let Some(mut plan) = self.saved_plan(project, plan_id)? {
                    plan.scene_mut(index, SystemTime::now())
                        .map_err(unexpected)?
                        .fail(failure.kind);
                    self.plans.save_scene(&plan, index).map_err(unexpected)?;
                }
                return Err(failure);
            }
        };
        let file = shot_file();
        self.files.write(project, &file, &png).map_err(unexpected)?;
        // Read again: the user may have changed the scene or the design
        // meanwhile, and their change must survive.
        let Some(mut plan) = self.saved_plan(project, plan_id)? else {
            crate::proxies::remove_media(self.files.as_ref(), project, &file);
            return Ok(());
        };
        let still_wanted = plan.scene(index).is_ok_and(|scene| {
            scene.is_end_card()
                && scene.product_image() == wanted
                && scene.look() == look
                && scene.card() == Some(card.as_str())
                && scene.needs_shot(aspect)
        });
        if !still_wanted {
            crate::proxies::remove_media(self.files.as_ref(), project, &file);
            return Ok(());
        }
        let unused = plan
            .scene_mut(index, SystemTime::now())
            .map_err(unexpected)?
            .add_shot(ComposedShot {
                file,
                image: wanted,
                card: Some(card),
                aspect,
                look: Some(look),
                tones,
            });
        self.plans.save_scene(&plan, index).map_err(unexpected)?;
        if let Some(unused) = unused {
            crate::proxies::remove_media(self.files.as_ref(), project, &unused.file);
        }
        Ok(())
    }

    /// The picture of the end card, scene `index`, from `inputs`, with
    /// `product`'s image `wanted` when it shows one, in `look`, composed
    /// in `shape` with its text drawn: a cut's own shape, or one of an
    /// export for ads (#283). Free, on the computer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compose_card_picture(
        &self,
        project: VideoProjectId,
        index: usize,
        inputs: &CardInputs,
        wanted: Option<bardo_domain::ProductImageId>,
        look: bardo_domain::ShotLook,
        product: ProductId,
        shape: bardo_domain::FrameShape,
    ) -> Result<(Vec<u8>, Vec<bardo_domain::ShotColor>), JobFailure> {
        let failure = |why: String| {
            JobFailure::new(JobFailureKind::Files, format!("scene {}: {why}", index + 1))
        };

        let image = match wanted {
            None => None,
            Some(wanted) => Some(
                ad_product_images(self.products.as_ref(), product)
                    .map_err(|error| {
                        failure(format!("the product's images could not be read: {error}"))
                    })?
                    .into_iter()
                    .find(|image| image.id == wanted)
                    .ok_or_else(|| {
                        failure("the product no longer has the image it shows".to_owned())
                    })?,
            ),
        };
        let backdrop = self
            .backdrop_bytes(project, look.backdrop)
            .map_err(failure)?;
        let layout = CardLayout::of(
            shape,
            inputs.design.text_place,
            inputs.design.logo_place,
            image.is_some(),
            inputs.logo.is_some(),
        );
        let recipe = CardRecipe {
            shape,
            layout,
            backdrop: backdrop_paint(look.backdrop, backdrop.as_deref()),
            product: image.as_ref().map(|image| {
                (
                    image.picture.bytes(),
                    look.layout_for(image.kind, image.cut_out()),
                )
            }),
            logo: inputs.logo.as_ref().map(|logo| logo.picture.bytes()),
        };
        let picture = self
            .composer
            .compose_card(&recipe)
            .map_err(|error| failure(error.to_string()))?;
        let png = self
            .card_text(
                &picture.png,
                &layout,
                &inputs.design.text(inputs.brief.text()),
            )
            .map_err(failure)?;
        Ok((png, picture.tones))
    }

    /// The card picture `picture` with `text` drawn in `layout`'s text
    /// box by the media engine, as a PNG file's bytes.
    fn card_text(
        &self,
        picture: &[u8],
        layout: &CardLayout,
        text: &bardo_domain::CardText,
    ) -> Result<Vec<u8>, String> {
        let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
        let source = dir.path().join("card.png");
        let destination = dir.path().join("card-text.png");
        std::fs::write(&source, picture).map_err(|error| error.to_string())?;
        self.media
            .draw_card_text(&source, layout, text, &destination)
            .map_err(|error| format!("the card's text could not be drawn: {error}"))?;
        std::fs::read(&destination).map_err(|error| error.to_string())
    }
}

impl Bardo {
    /// What `project`'s end card is made from; `None` for a project that
    /// is no ad.
    fn card_inputs(
        &self,
        project: &bardo_domain::VideoProject,
    ) -> Result<Option<CardInputs>, RepositoryError> {
        CardInputs::of(
            self.ads.as_ref(),
            self.end_cards.as_ref(),
            self.products.as_ref(),
            project.id,
            project.product,
        )
    }

    /// The composed scenes of `plan` that wait for their picture in
    /// `aspect`, counting the end card when its picture shows another
    /// design than the one saved, brief and logo as they are now.
    pub(crate) fn missing_shots(
        &self,
        project: &bardo_domain::VideoProject,
        plan: &bardo_domain::ScenePlan,
        aspect: AspectRatio,
    ) -> Result<Vec<usize>, RepositoryError> {
        let mut missing = plan.missing_shots(aspect);
        let Some(index) = plan.end_card() else {
            return Ok(missing);
        };
        if missing.contains(&index) {
            return Ok(missing);
        }
        if let Some(inputs) = self.card_inputs(project)?
            && plan
                .scene(index)
                .is_ok_and(|scene| scene.card() != Some(inputs.fingerprint().as_str()))
        {
            missing.push(index);
        }
        Ok(missing)
    }

    /// Whether `project`'s end card waits to be composed for the video's
    /// shape: its picture is missing or shows another design than the
    /// one saved, brief and logo as they are now. The video is not
    /// rendered until it is composed (#281).
    pub(crate) fn end_card_waits(&self, project: VideoProjectId) -> Result<bool, RepositoryError> {
        let Some(video) = self.themes.project(project)? else {
            return Ok(false);
        };
        let Some(plan) = self.scene_plans.scene_plan(project)? else {
            return Ok(false);
        };
        let Some(index) = plan.end_card() else {
            return Ok(false);
        };
        let aspect = self.video_format(&video)?.aspect();
        Ok(self.missing_shots(&video, &plan, aspect)?.contains(&index))
    }

    /// The end card of an ad project, for its editor; `None` for a
    /// project that is no ad.
    pub fn end_card(&self, project: VideoProjectId) -> Result<Option<EndCardView>, SceneError> {
        let project = self.scenes_project(project)?;
        let Some(inputs) = self.card_inputs(&project)? else {
            return Ok(None);
        };
        let aspect = self.video_format(&project)?.aspect();
        let plan = self.scene_plans.scene_plan(project.id)?;
        let scene = plan.as_ref().and_then(|plan| plan.end_card());
        let card = inputs.fingerprint();
        let out_of_date = match (plan.as_ref(), scene) {
            (Some(plan), Some(index)) => plan
                .scene(index)
                .is_ok_and(|scene| scene.card() != Some(card.as_str()) || scene.needs_shot(aspect)),
            _ => false,
        };
        Ok(Some(EndCardView {
            offer: inputs.brief.text().offer().to_owned(),
            offer_fits: CardDesign::offer_fits(inputs.brief.text()),
            has_logo: inputs.logo.is_some(),
            design: inputs.design,
            scene,
            out_of_date,
        }))
    }

    /// Saves the end card's design as `draft` says, and composes its
    /// picture again in a job, for free. Returns the job, or `None` when
    /// the plan has no end card to compose (the design is kept for when
    /// it has).
    pub fn save_end_card(
        &self,
        project: VideoProjectId,
        draft: &CardDraft,
    ) -> Result<Option<JobId>, SceneError> {
        let design = CardDesign::new(
            &draft.headline,
            &draft.second_line,
            draft.offer,
            draft.look,
            draft.text_place,
            draft.logo_place,
        )
        .map_err(SceneError::Card)?;
        let video = self.scenes_project(project)?;
        if self.ads.brief(project)?.is_none() {
            return Err(SceneError::Kind(bardo_domain::SceneKindError::NotAllowed(
                bardo_domain::SceneKind::EndCard,
            )));
        }
        // A job saving a scene meanwhile would be written over.
        self.scenes_idle(project)?;
        if self.clips_running(project) {
            return Err(SceneError::Busy);
        }
        self.end_cards.save_end_card(&EndCard { project, design })?;
        self.refresh_end_card(&video)
    }

    /// Brings the end card's wanted design up to date after what it shows
    /// changed (its design, the brief's offer, the product's logo), and
    /// composes its picture again in a job, for free. Returns the job;
    /// `None` when the plan has no end card or it was up to date. When
    /// the scenes are busy the card stays out of date, and is composed
    /// with the other missing pictures.
    pub(crate) fn refresh_end_card(
        &self,
        project: &bardo_domain::VideoProject,
    ) -> Result<Option<JobId>, SceneError> {
        let Some(inputs) = self.card_inputs(project)? else {
            return Ok(None);
        };
        let Some(mut plan) = self.scene_plans.scene_plan(project.id)? else {
            return Ok(None);
        };
        if plan.owner != self.profile.id {
            return Ok(None);
        }
        if self.scenes_busy(project.id) || self.clips_running(project.id) {
            return Ok(None);
        }
        let Some(index) = plan.set_end_card(inputs.fingerprint(), SystemTime::now()) else {
            return Ok(None);
        };
        self.scene_plans.save_scene(&plan, index)?;
        match self.compose_scenes(&plan, project, vec![index]) {
            Ok(job) => Ok(Some(job)),
            Err(SceneError::Busy) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Refreshes the end cards of the ads of `product`, after its logo
    /// changed. A card that cannot be refreshed now stays as it was, and
    /// shows out of date when it is next read.
    pub(crate) fn refresh_end_cards_of(&self, product: ProductId) {
        let Ok(briefs) = self.ads.briefs_of(product) else {
            return;
        };
        for brief in briefs {
            let Ok(Some(project)) = self.themes.project(brief.project) else {
                continue;
            };
            if let Err(error) = self.refresh_end_card(&project) {
                tracing::warn!(%error, "an end card could not be brought up to date");
            }
        }
    }
}

/// Why a card's lines cannot be saved, for the editor.
pub fn card_field_errors(error: &SceneError) -> &[CardFieldError] {
    match error {
        SceneError::Card(errors) => errors,
        _ => &[],
    }
}
