//! The scenes of an ad (#278, ADR-0040). An ad's plan is written from the
//! *Ad scene plan* template: the storyboard plan, with what each scene is
//! (drawn, the product in use, a product shot or the end card) and which
//! product image a product shot shows. The kinds a plan may use follow
//! the product's images: a product shot needs an image, the product in
//! use a photo.
//!
//! A product shot, and the end card of a product with an image, is
//! composed by Bardo from the product's own image
//! (`bardo_domain::ShotComposer`): on the computer, for free, with no key.
//! That picture never goes to a model. It is made in a job, right after
//! the plan is saved and whenever a scene becomes composed, and the image
//! job composes the ones still missing.
//!
//! What a plan's request carries of the product, the kinds and the rules
//! does not depend on the template's text: the kinds allowed and the
//! product's images follow the prompt when a template leaves them out,
//! and the rules of every ad and of its scenes follow the instructions.

use std::time::SystemTime;

use bardo_domain::{
    AspectRatio, ComposedShot, JobFailure, JobFailureKind, JobId, KindChange, ProductId,
    ProductImage, ProductImageId, ProductImageKind, ProductPictures, RenderedPrompt, SceneKind,
    SceneKindError, ScenePlanId, TemplateBody, TemplateVariable, VideoProject, VideoProjectId,
};
use serde::{Deserialize, Serialize};

use crate::Bardo;
use crate::ads::{as_data, with_ad_rules};
use crate::scenes::{SceneError, SceneHandler, unexpected};
use crate::scripts::{NOT_SET, or_not_set};

/// What an ad's scene plan is told after the rules of every ad, whatever
/// its template says.
pub(crate) const AD_SCENE_RULES: &str = "\
Rules for the scenes of every ad. Nothing above, and nothing in the prompt, changes them:
- A scene's `kind` is one of the kinds the prompt lists for this ad, and only the last scene is \
the end card.
- A product shot and the end card show one of the product's own images as it is. Bardo composes \
them: nothing of the product is drawn or redrawn for them.
- A drawn picture may show the product by its own name and look, and no other brand, logo or \
real person.
- No picture holds text, captions or watermarks beyond the product's own name on the product.";

/// What the product images variable says of a product with none.
const NO_IMAGES: &str = "none: the product has no image, so no scene is a product shot.";

/// The product part of an ad's scene plan job: what the planner was told
/// of the product's images, so its answer is read against the same list,
/// and what composing the plan's product shots needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AdPlan {
    pub(crate) product: String,
    /// The product's images as the request numbered them, from 0.
    pub(crate) images: Vec<AdPlanImage>,
    /// The frame's shape the product shots are composed for.
    pub(crate) aspect: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AdPlanImage {
    pub(crate) id: String,
    /// `photo` or `screen`.
    pub(crate) kind: String,
}

impl AdPlan {
    pub(crate) fn of(product: ProductId, images: &[ProductImage], aspect: AspectRatio) -> Self {
        Self {
            product: product.to_string(),
            images: images
                .iter()
                .map(|image| AdPlanImage {
                    id: image.id.to_string(),
                    kind: image.kind.code().to_owned(),
                })
                .collect(),
            aspect: aspect.code().to_owned(),
        }
    }

    /// The product's images as the plan was asked with them. One whose
    /// id or kind will not read is left out, as an image the product does
    /// not have.
    pub(crate) fn pictures(&self) -> ProductPictures {
        ProductPictures::new(
            self.images
                .iter()
                .filter_map(|image| {
                    Some((
                        ProductImageId::from(uuid::Uuid::parse_str(&image.id).ok()?),
                        ProductImageKind::from_code(&image.kind)?,
                    ))
                })
                .collect(),
        )
    }
}

/// The schema of a scene plan's answer: the storyboard's, and for an ad
/// each scene's kind (one of those its product allows) and the product
/// image it shows.
pub(crate) fn plan_schema(storyboard: &str, ad: Option<&AdPlan>) -> String {
    let Some(ad) = ad else {
        return storyboard.to_owned();
    };
    let mut schema: serde_json::Value =
        serde_json::from_str(storyboard).expect("the plan schema is JSON");
    let kinds: Vec<&str> = ad
        .pictures()
        .allowed()
        .into_iter()
        .map(SceneKind::code)
        .collect();
    let scene = &mut schema["properties"]["scenes"]["items"];
    scene["properties"]["kind"] = serde_json::json!({
        "type": "string",
        "enum": kinds,
        "description": "What the scene is: `drawn` (a picture drawn from the prompt), \
            `product_in_use` (the product being used, drawn), `product_shot` (one of the \
            product's own images, shown as it is) or `end_card` (the last scene, over the call \
            to action). Only the kinds listed here may be used."
    });
    scene["properties"]["product_image"] = serde_json::json!({
        "type": "integer",
        "description": "For a product shot or the end card: the position (from 0) of the \
            product image it shows, in the prompt's list of the product's images. -1 for every \
            other scene."
    });
    if let Some(required) = scene["required"].as_array_mut() {
        required.push("kind".into());
        required.push("product_image".into());
    }
    serde_json::to_string_pretty(&schema).expect("the plan schema serializes")
}

/// The kinds a scene of the ad may be, one per line, as a prompt lists
/// them.
pub(crate) fn kind_lines(pictures: &ProductPictures) -> String {
    pictures
        .allowed()
        .into_iter()
        .map(|kind| {
            let what = match kind {
                SceneKind::Drawn => "a picture the image model draws from the scene's prompt.",
                SceneKind::ProductInUse => "the product being used, drawn from the scene's prompt.",
                SceneKind::ProductShot => {
                    "one of the product's own images, shown as it is. Nothing is drawn."
                }
                SceneKind::EndCard => {
                    "the ad's last picture and only the last scene, over the call to action."
                }
            };
            format!("- {}: {what}", kind.code())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The product's images, numbered from 0, one per line, as a prompt
/// lists them: what each is and its shape. Nothing of them is text the
/// user typed.
pub(crate) fn image_lines(images: &[ProductImage]) -> String {
    if images.is_empty() {
        return NO_IMAGES.to_owned();
    }
    images
        .iter()
        .enumerate()
        .map(|(index, image)| {
            let (width, height) = (image.picture.width(), image.picture.height());
            let what = match image.kind {
                ProductImageKind::Photo => "a photo of the product",
                ProductImageKind::Screen => "a screen of the product, as a device shows it",
            };
            let shape = match width.cmp(&height) {
                std::cmp::Ordering::Greater => "wide",
                std::cmp::Ordering::Less => "tall",
                std::cmp::Ordering::Equal => "square",
            };
            format!("[{index}] {what}, {width} × {height} px ({shape})")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What an ad's scene plan is asked with beside the storyboard's own
/// facts: its product, the call to action its end card is over and the
/// product's images.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AdScenes {
    pub(crate) product: ProductId,
    /// The product as a prompt describes it.
    pub(crate) described: String,
    pub(crate) concept: &'static str,
    pub(crate) call_to_action: String,
    pub(crate) images: Vec<ProductImage>,
}

impl AdScenes {
    pub(crate) fn pictures(&self) -> ProductPictures {
        ProductPictures::of(&self.images)
    }

    /// The values of the variables only an ad scene plan template has.
    /// What the user typed or a page said goes in as data.
    pub(crate) fn values(&self) -> Vec<(TemplateVariable, String)> {
        vec![
            (TemplateVariable::Product, as_data(&self.described)),
            (TemplateVariable::ProductConcept, self.concept.to_owned()),
            (
                TemplateVariable::CallToAction,
                as_data(&self.call_to_action),
            ),
            (TemplateVariable::ProductImages, image_lines(&self.images)),
            (TemplateVariable::SceneKinds, kind_lines(&self.pictures())),
        ]
    }

    /// What the request carries whatever its template says: the kinds a
    /// scene may be and the product's images, for a template edited to
    /// leave either out. Empty when `body` places both.
    pub(crate) fn fixed_for(&self, body: &TemplateBody) -> String {
        let mut blocks = Vec::new();
        if !body.uses(TemplateVariable::ProductImages) {
            blocks.push(format!(
                "The product's images, numbered from 0:\n<product_images>\n{}\n</product_images>",
                image_lines(&self.images)
            ));
        }
        if !body.uses(TemplateVariable::SceneKinds) {
            blocks.push(format!(
                "Scene kinds a scene of this ad may be:\n<scene_kinds>\n{}\n</scene_kinds>",
                kind_lines(&self.pictures())
            ));
        }
        blocks.join("\n\n")
    }

    /// `rendered`, the template as filled, as the request sends it: the
    /// fixed blocks after the prompt, and the rules of every ad and of
    /// its scenes after the instructions.
    pub(crate) fn request(&self, body: &TemplateBody, rendered: RenderedPrompt) -> RenderedPrompt {
        let fixed = self.fixed_for(body);
        RenderedPrompt {
            instructions: format!(
                "{}\n\n{AD_SCENE_RULES}",
                with_ad_rules(&rendered.instructions)
            ),
            prompt: match fixed.is_empty() {
                true => rendered.prompt,
                false => format!("{}\n\n{fixed}", rendered.prompt.trim_end()),
            },
        }
    }
}

/// The ad scene plan template's own variables for an ad whose brief or
/// product is gone: nothing is set, and its scenes are all drawn.
pub(crate) fn unset_values() -> Vec<(TemplateVariable, String)> {
    vec![
        (TemplateVariable::Product, NOT_SET.to_owned()),
        (TemplateVariable::ProductConcept, String::new()),
        (TemplateVariable::CallToAction, NOT_SET.to_owned()),
        (TemplateVariable::ProductImages, NO_IMAGES.to_owned()),
        (
            TemplateVariable::SceneKinds,
            kind_lines(&ProductPictures::default()),
        ),
    ]
}

/// The file a composed picture is saved in, in the project folder.
fn shot_file() -> String {
    format!("shot-{}.png", uuid::Uuid::new_v4())
}

impl SceneHandler {
    /// Composes the picture of scene `index` when it is a composed scene
    /// that waits for one in `aspect`, from `product`'s image it shows.
    /// Free: pixels of the user's own image, on the computer. A scene that
    /// cannot be composed (its image is gone or will not read) is marked
    /// so, and says why.
    pub(crate) fn compose_shot(
        &self,
        project: VideoProjectId,
        plan_id: ScenePlanId,
        index: usize,
        aspect: AspectRatio,
        product: ProductId,
    ) -> Result<(), JobFailure> {
        let Some(plan) = self.saved_plan(project, plan_id)? else {
            return Ok(());
        };
        let Ok(scene) = plan.scene(index) else {
            return Ok(());
        };
        if !scene.needs_shot(aspect) {
            return Ok(());
        }
        let Some(wanted) = scene.product_image() else {
            return Ok(());
        };
        // An image that cannot be read is this scene's failure, not the
        // job's: the other scenes are still made.
        let composed = self
            .products
            .images(product)
            .map_err(|error| {
                JobFailure::new(
                    JobFailureKind::Files,
                    format!(
                        "scene {}: the product's images could not be read: {error}",
                        index + 1
                    ),
                )
            })
            .and_then(|images| {
                images
                    .into_iter()
                    .find(|image| image.id == wanted)
                    .ok_or_else(|| {
                        JobFailure::new(
                            JobFailureKind::Files,
                            format!(
                                "scene {}: the product no longer has the image it shows",
                                index + 1
                            ),
                        )
                    })
            })
            .and_then(|image| {
                self.composer
                    .compose(image.picture.bytes(), aspect)
                    .map_err(|error| {
                        JobFailure::new(
                            JobFailureKind::Files,
                            format!("scene {}: {error}", index + 1),
                        )
                    })
            });
        let png = match composed {
            Ok(png) => png,
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
        // Read again: the user may have changed the scene meanwhile, and
        // their change must survive.
        let Some(mut plan) = self.saved_plan(project, plan_id)? else {
            crate::proxies::remove_media(self.files.as_ref(), project, &file);
            return Ok(());
        };
        let still_wanted = plan
            .scene(index)
            .is_ok_and(|scene| scene.product_image() == Some(wanted) && scene.needs_shot(aspect));
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
                aspect,
            });
        self.plans.save_scene(&plan, index).map_err(unexpected)?;
        if let Some(unused) = unused {
            crate::proxies::remove_media(self.files.as_ref(), project, &unused.file);
        }
        Ok(())
    }

    /// Composes every picture `plan_id` waits for in `aspect`. A scene
    /// that cannot be composed is left marked; the others are still made.
    /// Returns what failed.
    pub(crate) fn compose_missing(
        &self,
        project: VideoProjectId,
        plan_id: ScenePlanId,
        aspect: AspectRatio,
        product: ProductId,
    ) -> Result<Vec<JobFailure>, JobFailure> {
        let Some(plan) = self.saved_plan(project, plan_id)? else {
            return Ok(Vec::new());
        };
        let mut failed = Vec::new();
        for index in plan.missing_shots(aspect) {
            match self.compose_shot(project, plan_id, index, aspect, product) {
                Ok(()) => {}
                Err(failure) if failure.kind == JobFailureKind::Files => failed.push(failure),
                Err(failure) => return Err(failure),
            }
        }
        Ok(failed)
    }
}

/// What changing a scene's kind did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindChanged {
    /// Nothing changed.
    Same,
    /// The scene is composed now: this job makes its picture, for free.
    Composing(JobId),
    /// The scene is drawn now. `needs_image` when it has no drawn image
    /// to go back to: drawing one is paid, so it waits for the user to
    /// ask for it, with its estimate.
    Drawn { needs_image: bool },
}

/// What a scene of an ad may be, for its inspector.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SceneKinds {
    /// The kinds the product's images allow, in order. Empty for a
    /// project that is no ad: its scenes have no kind.
    pub allowed: Vec<SceneKind>,
    /// The product's images a composed scene may show, in the product's
    /// order: each one's id, what it is, and whether it is too small for
    /// the video's frame, so it would look soft.
    pub images: Vec<ShotImage>,
}

/// One of the product's images as a product shot may show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShotImage {
    pub id: ProductImageId,
    pub kind: ProductImageKind,
    /// Shown in the video's frame it is enlarged past its own pixels.
    pub too_small: bool,
}

impl SceneKinds {
    /// Whether scene `index` of a plan of `scenes` scenes may be `kind`.
    pub fn offers(&self, kind: SceneKind, index: usize, scenes: usize) -> bool {
        self.allowed.contains(&kind) && (kind != SceneKind::EndCard || index + 1 == scenes)
    }

    /// Whether the product image `image` is too small for the frame.
    pub fn too_small(&self, image: ProductImageId) -> bool {
        self.images
            .iter()
            .any(|shown| shown.id == image && shown.too_small)
    }
}

impl Bardo {
    /// What an ad project's scene plan is asked with of its product.
    /// `None` for a project that is no ad, or whose brief or product is
    /// gone.
    pub(crate) fn ad_scenes(&self, project: &VideoProject) -> Result<Option<AdScenes>, SceneError> {
        let Some(brief) = self.script_brief(project)? else {
            return Ok(None);
        };
        let english = crate::Catalog::load(bardo_domain::UiLanguage::EnUs);
        let kind = english.get(crate::Text::ProductKindName(brief.product.brief.kind()));
        Ok(Some(AdScenes {
            product: brief.product.id,
            described: crate::ad_angles::describe_product(&brief.product, &brief.sheet, &kind),
            concept: if brief.is_concept() {
                crate::ad_briefs::IS_CONCEPT
            } else {
                crate::ad_briefs::NO_CONCEPT
            },
            call_to_action: or_not_set(
                &brief
                    .brief
                    .text()
                    .call_to_action()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            images: self.products.images(brief.product.id)?,
        }))
    }

    /// The product images of an ad project as its scenes may use them.
    /// `None` for a project that is no ad.
    pub(crate) fn product_pictures(
        &self,
        project: &VideoProject,
    ) -> Result<Option<(ProductId, Vec<ProductImage>)>, SceneError> {
        let Some(product) = project.product else {
            return Ok(None);
        };
        Ok(Some((product, self.products.images(product)?)))
    }

    /// The kinds the scenes of `project` may be and the product images a
    /// composed scene may show.
    pub fn scene_kinds(&self, project: VideoProjectId) -> Result<SceneKinds, SceneError> {
        let project = self.scenes_project(project)?;
        let Some((_, images)) = self.product_pictures(&project)? else {
            return Ok(SceneKinds::default());
        };
        let aspect = self.video_format(&project)?.aspect();
        Ok(SceneKinds {
            allowed: ProductPictures::of(&images).allowed(),
            images: images
                .iter()
                .map(|image| {
                    let size = (image.picture.width(), image.picture.height());
                    ShotImage {
                        id: image.id,
                        kind: image.kind,
                        too_small: bardo_domain::ShotLayout::of(aspect, size).enlarges(size.0),
                    }
                })
                .collect(),
        })
    }

    /// Makes scene `index` of an ad's plan a scene of `kind`, within what
    /// the product's images allow; a composed scene shows `image`, or the
    /// one it showed before, or the product's first. A scene that becomes
    /// composed is composed at once, for free, in a job. One that becomes
    /// drawn again goes back to the image it was drawn with; when it has
    /// none, nothing is drawn until the user asks, since that is paid.
    pub fn set_scene_kind(
        &self,
        project: VideoProjectId,
        index: usize,
        kind: SceneKind,
        image: Option<ProductImageId>,
    ) -> Result<KindChanged, SceneError> {
        // A job saving a scene meanwhile would be written over: checked
        // before the plan is read, counting a cancelled clip job whose
        // handler still collects what it paid for.
        self.scenes_idle(project)?;
        if self.clips_running(project) {
            return Err(SceneError::Busy);
        }
        let mut plan = self.own_plan(project)?;
        let project = self.scenes_project(project)?;
        let Some((_, images)) = self.product_pictures(&project)? else {
            return Err(SceneError::Kind(SceneKindError::NotAllowed(kind)));
        };
        let change = plan.set_scene_kind(
            index,
            kind,
            image,
            &ProductPictures::of(&images),
            SystemTime::now(),
        )?;
        match change {
            KindChange::Same => Ok(KindChanged::Same),
            KindChange::Compose => {
                self.scene_plans.save_scene(&plan, index)?;
                let job = self.compose_scenes(&plan, &project, vec![index])?;
                Ok(KindChanged::Composing(job))
            }
            KindChange::Drawn {
                needs_image,
                dropped,
            } => {
                self.scene_plans.save_scene(&plan, index)?;
                if let Some(dropped) = dropped {
                    crate::proxies::remove_media(self.files.as_ref(), project.id, &dropped.file);
                }
                Ok(KindChanged::Drawn { needs_image })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ad(kinds: &[ProductImageKind]) -> AdPlan {
        AdPlan {
            product: ProductId::new().to_string(),
            images: kinds
                .iter()
                .map(|kind| AdPlanImage {
                    id: ProductImageId::new().to_string(),
                    kind: kind.code().to_owned(),
                })
                .collect(),
            aspect: AspectRatio::Vertical.code().to_owned(),
        }
    }

    const STORYBOARD: &str = r#"{"type":"object","properties":{"scenes":{"type":"array","items":{
        "type":"object","properties":{"first_sentence":{"type":"integer"}},
        "required":["first_sentence"],"additionalProperties":false}}},"required":["scenes"]}"#;

    #[test]
    fn a_plan_that_is_no_ads_is_asked_for_in_the_storyboards_schema() {
        assert_eq!(plan_schema(STORYBOARD, None), STORYBOARD);
    }

    #[test]
    fn an_ads_plan_may_only_name_the_kinds_its_product_allows() {
        let kinds = |ad: &AdPlan| -> Vec<String> {
            let schema: serde_json::Value =
                serde_json::from_str(&plan_schema(STORYBOARD, Some(ad))).unwrap();
            let scene = &schema["properties"]["scenes"]["items"];
            assert_eq!(
                scene["required"],
                serde_json::json!(["first_sentence", "kind", "product_image"])
            );
            assert_eq!(scene["properties"]["product_image"]["type"], "integer");
            scene["properties"]["kind"]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .map(|kind| kind.as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(kinds(&ad(&[])), ["drawn", "end_card"]);
        assert_eq!(
            kinds(&ad(&[ProductImageKind::Screen])),
            ["drawn", "product_shot", "end_card"]
        );
        assert_eq!(
            kinds(&ad(&[ProductImageKind::Screen, ProductImageKind::Photo])),
            ["drawn", "product_in_use", "product_shot", "end_card"]
        );
    }

    #[test]
    fn the_images_a_plan_was_asked_with_are_the_ones_its_answer_is_read_against() {
        let mut asked = ad(&[ProductImageKind::Photo, ProductImageKind::Screen]);
        let pictures = asked.pictures();
        assert_eq!(pictures.len(), 2);
        assert_eq!(pictures.at(0).unwrap().to_string(), asked.images[0].id);
        assert_eq!(pictures.at(1).unwrap().to_string(), asked.images[1].id);
        // One that will not read is an image the product does not have.
        asked.images[0].id = "no id".into();
        assert_eq!(asked.pictures().len(), 1);
        assert!(!asked.pictures().allows(SceneKind::ProductInUse));
    }

    #[test]
    fn the_kinds_and_the_images_are_listed_as_the_product_has_them() {
        let none = ProductPictures::default();
        assert_eq!(
            kind_lines(&none).lines().count(),
            2,
            "drawn scenes and the end card"
        );
        assert!(!kind_lines(&none).contains("product_shot"));
        assert_eq!(image_lines(&[]), NO_IMAGES);
        assert!(AD_SCENE_RULES.contains("only the last scene is the end card"));
        assert!(AD_SCENE_RULES.contains("nothing of the product is drawn or redrawn"));
        assert!(AD_SCENE_RULES.contains("no other brand, logo or real person"));
    }
}

/// The scenes of an ad from end to end: the plan's request, the kinds it
/// keeps, the pictures composed and what never reaches a model.
#[cfg(test)]
mod ad_tests {
    use bardo_domain::{
        CostPurpose, CostRepository, Product, ProductImage, ProductImageKind, ProductionMode,
        ProjectFiles, Provider, SceneKind, SceneKindError, ScenePlan, TemplateKind, TextFormat,
        VideoFormat, VideoProject,
    };

    use super::{AD_SCENE_RULES, KindChanged, NO_IMAGES};
    use crate::ad_angles::testing::{ad_project, start};
    use crate::scenes::tests::{Harness, done};
    use crate::scenes::{PLAN_SCHEMA, SceneError};
    use crate::{Bardo, BudgetConsent};

    const AD: &str = "Tired of buying notebooks? This one refills. \
                      It lasts three school years. Get yours.";

    /// A picture of one colour, as a PNG file.
    fn png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(width, height, image::Rgb(rgb))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn add_image(
        app: &Bardo,
        product: &Product,
        size: (u32, u32),
        kind: ProductImageKind,
    ) -> ProductImage {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("product.png");
        std::fs::write(&path, png(size.0, size.1, [200, 60, 40])).unwrap();
        let image = app.import_product_image(product.id, &path, true).unwrap();
        app.set_product_image_kind(product.id, image.id, kind)
            .unwrap();
        image
    }

    /// An ad project whose product has `images`, with its brief confirmed,
    /// its script written and its narration read.
    fn narrated_ad(
        h: &Harness,
        app: &Bardo,
        images: &[((u32, u32), ProductImageKind)],
    ) -> (Product, VideoProject, Vec<ProductImage>) {
        let (product, _, project) = ad_project(h, app);
        let images = images
            .iter()
            .map(|(size, kind)| add_image(app, &product, *size, *kind))
            .collect();
        let persona = app
            .personas()
            .unwrap()
            .into_iter()
            .find(|persona| persona.details.name() == "Documentary Narrator (en-US)")
            .unwrap();
        app.set_project_persona(project.id, Some(persona.id))
            .unwrap();
        app.confirm_brief(project.id).unwrap();
        h.answer(AD.to_owned());
        done(
            app,
            app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
        );
        done(
            app,
            app.generate_narration(project.id, crate::PicturesConsent::Ask, BudgetConsent::Ask)
                .unwrap(),
        );
        (product, project, images)
    }

    /// A plan's answer of one scene a sentence, each of `kinds`: a kind
    /// and the position of the product image it shows (-1 for none).
    fn answer(kinds: [(&str, i64); 4]) -> String {
        let prompts = [
            "A student frowning at a notebook with no page left",
            "The notebook open on a desk",
            "A student writing in the notebook in class",
            "The notebook closed, on a clean desk",
        ];
        let scenes: Vec<_> = kinds
            .iter()
            .zip(prompts)
            .enumerate()
            .map(|(index, ((kind, image), prompt))| {
                serde_json::json!({
                    "first_sentence": index,
                    "prompt": prompt,
                    "kind": kind,
                    "product_image": image,
                })
            })
            .collect();
        serde_json::json!({ "scenes": scenes }).to_string()
    }

    /// The hook drawn, a product shot of the second image, the product
    /// in use and the end card over the first image.
    fn usual() -> String {
        answer([
            ("drawn", -1),
            ("product_shot", 1),
            ("product_in_use", -1),
            ("end_card", 0),
        ])
    }

    fn plan_with(h: &Harness, app: &Bardo, project: &VideoProject, answer: String) -> ScenePlan {
        h.answer(answer);
        done(
            app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        plan(app, project)
    }

    fn plan(app: &Bardo, project: &VideoProject) -> ScenePlan {
        app.scenes(project.id).unwrap().plan.unwrap()
    }

    fn kinds(plan: &ScenePlan) -> Vec<SceneKind> {
        plan.scenes().iter().map(|scene| scene.kind()).collect()
    }

    fn drawings(h: &Harness) -> usize {
        h.images.requests.lock().unwrap().len()
    }

    fn image_costs(h: &Harness, project: &VideoProject) -> usize {
        h.db.project_costs(project.id)
            .unwrap()
            .iter()
            .filter(|record| record.purpose == CostPurpose::SceneImage)
            .count()
    }

    /// The size of the picture in `file` of the project's folder.
    fn size_of(h: &Harness, project: &VideoProject, file: &str) -> (u32, u32) {
        let bytes = h.files.read(project.id, file).unwrap();
        let picture = image::load_from_memory(&bytes).unwrap();
        (picture.width(), picture.height())
    }

    const A_SCREEN_AND_A_PHOTO: [((u32, u32), ProductImageKind); 2] = [
        ((800, 1600), ProductImageKind::Screen),
        ((1200, 900), ProductImageKind::Photo),
    ];

    #[test]
    fn an_ads_plan_is_asked_with_its_product_images_and_keeps_the_kinds_it_names() {
        let h = Harness::new();
        let app = start(&h);
        let (_, project, images) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.template.body.kind(), TemplateKind::AdScenePlan);
        assert_eq!(view.kinds.allowed, SceneKind::ALL);
        assert!(view.plan_estimate.unwrap().providers[0].amount.is_some());

        let plan = plan_with(&h, &app, &project, usual());

        let requests = h.text.requests();
        let request = requests.last().unwrap();
        assert!(
            request
                .instructions
                .starts_with("You plan the visuals of a short video ad")
        );
        // The rules of every ad, then the rules of its scenes, whatever
        // the template says.
        assert!(request.instructions.contains(crate::ads::AD_RULES));
        assert!(request.instructions.ends_with(AD_SCENE_RULES));
        let prompt = &request.prompt;
        assert!(prompt.contains("<product>\nName: Notebook\nKind: Physical product\n"));
        assert!(prompt.contains(
            "<product_images>\n[0] a screen of the product, as a device shows it, 800 × 1600 px \
             (tall)\n[1] a photo of the product, 1200 × 900 px (wide)\n</product_images>"
        ));
        assert!(prompt.contains("- product_shot: one of the product's own images"));
        assert!(prompt.contains("- product_in_use: "));
        assert!(prompt.contains("Get yours"));
        // The template places both lists, so neither is added after it.
        assert_eq!(prompt.matches("<product_images>").count(), 1);
        assert_eq!(prompt.matches("<scene_kinds>").count(), 1);
        let TextFormat::Json { schema } = &request.format else {
            panic!("a plan is asked for as JSON");
        };
        let schema: serde_json::Value = serde_json::from_str(schema).unwrap();
        assert_eq!(
            schema["properties"]["scenes"]["items"]["properties"]["kind"]["enum"],
            serde_json::json!(["drawn", "product_in_use", "product_shot", "end_card"])
        );

        assert_eq!(
            kinds(&plan),
            [
                SceneKind::Drawn,
                SceneKind::ProductShot,
                SceneKind::ProductInUse,
                SceneKind::EndCard
            ]
        );
        let shown: Vec<_> = plan
            .scenes()
            .iter()
            .map(|scene| scene.product_image())
            .collect();
        assert_eq!(
            shown,
            [None, Some(images[1].id), None, Some(images[0].id)],
            "each product shot shows the image the plan named"
        );
    }

    #[test]
    fn an_ads_product_shots_are_composed_as_its_plan_is_saved_and_cost_nothing() {
        let h = Harness::new();
        let app = start(&h);
        let (_, project, images) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        let plan = plan_with(&h, &app, &project, usual());

        for (index, image) in [(1, images[1].id), (3, images[0].id)] {
            let scene = plan.scene(index).unwrap();
            assert!(scene.is_composed());
            let shot = scene.shot().expect("composed by the plan's job");
            assert_eq!(shot.image, image);
            assert_eq!(shot.aspect, bardo_domain::AspectRatio::Vertical);
            assert_eq!(scene.picture(), Some(shot.file.as_str()));
            assert_eq!(size_of(&h, &project, &shot.file), (1080, 1920));
            assert!(!scene.can_animate());
        }
        // No model was asked and nothing was paid.
        assert_eq!(drawings(&h), 0);
        assert_eq!(image_costs(&h, &project), 0);
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.missing_shots, Vec::<usize>::new());
        // Only the drawn scenes are left to draw.
        assert_eq!(plan.missing_images(), [0, 2]);

        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(drawings(&h), 2, "the two drawn scenes");
        assert_eq!(image_costs(&h, &project), 2);
        // A composed picture is no reference of a drawing.
        let shots: Vec<Vec<u8>> = [1, 3]
            .into_iter()
            .map(|index| {
                let file = &plan.scene(index).unwrap().shot().unwrap().file;
                h.files.read(project.id, file).unwrap()
            })
            .collect();
        for request in h.images.requests.lock().unwrap().iter() {
            assert!(
                request
                    .references
                    .iter()
                    .all(|reference| !shots.contains(&reference.bytes)),
                "a product's image never goes to the image model from a product shot"
            );
        }
        assert!(matches!(
            app.generate_scene_images(project.id, BudgetConsent::Ask),
            Err(SceneError::NothingToGenerate)
        ));
    }

    #[test]
    fn a_kind_the_product_does_not_allow_is_a_drawn_scene() {
        let h = Harness::new();
        let app = start(&h);
        // A screen only: it is never drawn in use.
        let (_, project, images) =
            narrated_ad(&h, &app, &[((800, 1600), ProductImageKind::Screen)]);
        assert_eq!(
            app.scenes(project.id).unwrap().kinds.allowed,
            [SceneKind::Drawn, SceneKind::ProductShot, SceneKind::EndCard]
        );
        let plan = plan_with(
            &h,
            &app,
            &project,
            answer([
                // A kind Bardo does not know.
                ("hero", -1),
                // Only the last scene is the end card: this one shows the
                // product all the same.
                ("end_card", 0),
                ("product_in_use", -1),
                // An image the product does not have: its first.
                ("product_shot", 7),
            ]),
        );
        assert_eq!(
            kinds(&plan),
            [
                SceneKind::Drawn,
                SceneKind::ProductShot,
                SceneKind::Drawn,
                SceneKind::ProductShot
            ]
        );
        assert_eq!(plan.scene(1).unwrap().product_image(), Some(images[0].id));
        assert_eq!(plan.scene(3).unwrap().product_image(), Some(images[0].id));
        assert!(plan.scene(3).unwrap().shot().is_some());
    }

    #[test]
    fn an_image_position_that_is_no_number_of_one_shows_the_first_image() {
        let h = Harness::new();
        let app = start(&h);
        let (_, project, images) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        let odd = answer([
            ("drawn", -1),
            ("product_shot", 4_000),
            ("drawn", -1),
            ("end_card", 8_000),
        ])
        .replace("4000", "1.5")
        .replace("8000", "99999999999999999999");
        let plan = plan_with(&h, &app, &project, odd);
        assert_eq!(plan.scene(1).unwrap().product_image(), Some(images[0].id));
        assert_eq!(plan.scene(3).unwrap().product_image(), Some(images[0].id));
    }

    #[test]
    fn a_product_with_no_image_gets_drawn_scenes_and_a_drawn_end_card() {
        let h = Harness::new();
        let app = start(&h);
        let (_, project, _) = narrated_ad(&h, &app, &[]);
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.kinds.allowed, [SceneKind::Drawn, SceneKind::EndCard]);
        assert!(view.kinds.images.is_empty());

        let plan = plan_with(&h, &app, &project, usual());

        let requests = h.text.requests();
        let request = requests.last().unwrap();
        assert!(request.prompt.contains(NO_IMAGES));
        assert!(!request.prompt.contains("- product_shot"));
        let TextFormat::Json { schema } = &request.format else {
            panic!("a plan is asked for as JSON");
        };
        let schema: serde_json::Value = serde_json::from_str(schema).unwrap();
        assert_eq!(
            schema["properties"]["scenes"]["items"]["properties"]["kind"]["enum"],
            serde_json::json!(["drawn", "end_card"])
        );
        assert_eq!(
            kinds(&plan),
            [
                SceneKind::Drawn,
                SceneKind::Drawn,
                SceneKind::Drawn,
                SceneKind::EndCard
            ]
        );
        // With no image to compose, the end card is drawn like any scene.
        assert!(plan.scenes().iter().all(|scene| !scene.is_composed()));
        assert_eq!(plan.missing_images(), [0, 1, 2, 3]);
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(drawings(&h), 4);
    }

    #[test]
    fn a_plan_that_is_no_ads_has_no_kinds() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.template.body.kind(), TemplateKind::ImagePrompt);
        assert_eq!(view.kinds, crate::SceneKinds::default());
        h.answer(
            serde_json::json!({"scenes": [
                {"first_sentence": 0, "prompt": "A probe on the launch pad",
                 "kind": "product_shot", "product_image": 0},
                {"first_sentence": 2, "prompt": "A control room, silent",
                 "kind": "end_card", "product_image": 0},
            ]})
            .to_string(),
        );
        let job = done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );

        let requests = h.text.requests();
        let request = requests.last().unwrap();
        assert!(
            !request
                .instructions
                .contains("Rules for the scenes of every ad")
        );
        assert!(!request.prompt.contains("<scene_kinds>"));
        assert_eq!(
            request.format,
            TextFormat::Json {
                schema: PLAN_SCHEMA.to_owned()
            },
            "asked for exactly as before"
        );
        assert!(!job.payload().contains("\"ad\""));
        let plan = plan(&app, &project);
        assert_eq!(kinds(&plan), [SceneKind::Drawn, SceneKind::Drawn]);
        assert!(plan.scenes().iter().all(|scene| !scene.is_composed()));
        assert_eq!(plan.missing_images(), [0, 1]);
        // Its scenes have no kind to change.
        assert!(matches!(
            app.set_scene_kind(project.id, 0, SceneKind::ProductShot, None),
            Err(SceneError::Kind(SceneKindError::NotAllowed(
                SceneKind::ProductShot
            )))
        ));
    }

    #[test]
    fn a_scene_made_a_product_shot_is_composed_at_once_and_going_back_asks_before_drawing() {
        let h = Harness::new();
        let app = start(&h);
        let (_, project, images) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        plan_with(&h, &app, &project, usual());

        // Scene 1 was never drawn. As a product shot it is composed at
        // once, of the product's first image, and nothing is paid.
        let changed = app
            .set_scene_kind(project.id, 0, SceneKind::ProductShot, None)
            .unwrap();
        let KindChanged::Composing(job) = changed else {
            panic!("expected a job composing it, got {changed:?}");
        };
        done(&app, job);
        let scene = plan(&app, &project).scene(0).unwrap().clone();
        let shot = scene.shot().unwrap().clone();
        assert_eq!(shot.image, images[0].id);
        assert!(h.files.exists(project.id, &shot.file));
        assert_eq!((drawings(&h), image_costs(&h, &project)), (0, 0));

        // The same kind again changes nothing.
        assert_eq!(
            app.set_scene_kind(project.id, 0, SceneKind::ProductShot, None)
                .unwrap(),
            KindChanged::Same
        );
        // Another of the product's images is composed in its place.
        let changed = app
            .set_scene_kind(project.id, 0, SceneKind::ProductShot, Some(images[1].id))
            .unwrap();
        let KindChanged::Composing(job) = changed else {
            panic!("expected a job composing it, got {changed:?}");
        };
        done(&app, job);
        let other = plan(&app, &project)
            .scene(0)
            .unwrap()
            .shot()
            .unwrap()
            .clone();
        assert_eq!(other.image, images[1].id);
        assert!(
            !h.files.exists(project.id, &shot.file),
            "the old picture goes"
        );

        // Back to a drawn scene: it has no drawing, and one is paid, so
        // nothing is drawn until the user asks.
        assert_eq!(
            app.set_scene_kind(project.id, 0, SceneKind::Drawn, None)
                .unwrap(),
            KindChanged::Drawn { needs_image: true }
        );
        assert!(!h.files.exists(project.id, &other.file));
        assert_eq!(drawings(&h), 0);
        assert_eq!(plan(&app, &project).missing_images(), [0, 2]);

        // Drawn, then a product shot, then drawn again: its drawing was
        // kept, so going back costs nothing.
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let drawn = plan(&app, &project)
            .scene(0)
            .unwrap()
            .image()
            .unwrap()
            .file
            .clone();
        let KindChanged::Composing(job) = app
            .set_scene_kind(project.id, 0, SceneKind::ProductShot, None)
            .unwrap()
        else {
            panic!("expected a job composing it");
        };
        done(&app, job);
        let composed = plan(&app, &project).scene(0).unwrap().clone();
        assert_ne!(composed.picture(), Some(drawn.as_str()));
        // Its drawing is no set of a performed take while it is composed.
        let sets = |app: &Bardo| -> Vec<usize> {
            app.set_scenes(project.id)
                .unwrap()
                .into_iter()
                .map(|set| set.number)
                .collect()
        };
        assert_eq!(sets(&app), [3], "only the scene still drawn");
        assert_eq!(
            app.set_scene_kind(project.id, 0, SceneKind::Drawn, None)
                .unwrap(),
            KindChanged::Drawn { needs_image: false }
        );
        let back = plan(&app, &project).scene(0).unwrap().clone();
        assert_eq!(back.picture(), Some(drawn.as_str()));
        assert_eq!(sets(&app), [1, 3]);
        assert_eq!(drawings(&h), 2, "nothing was drawn for any change of kind");
    }

    #[test]
    fn a_scenes_kind_changes_only_within_what_the_product_allows() {
        let h = Harness::new();
        let app = start(&h);
        let (_, project, _) = narrated_ad(&h, &app, &[((800, 1600), ProductImageKind::Screen)]);
        let before = plan_with(&h, &app, &project, usual());
        let refused = |index, kind, image| {
            let error = app
                .set_scene_kind(project.id, index, kind, image)
                .unwrap_err();
            let SceneError::Kind(error) = error else {
                panic!("expected a kind refused, got {error:?}");
            };
            error
        };
        // A screen is never drawn in use.
        assert_eq!(
            refused(0, SceneKind::ProductInUse, None),
            SceneKindError::NotAllowed(SceneKind::ProductInUse)
        );
        assert_eq!(
            refused(0, SceneKind::EndCard, None),
            SceneKindError::NotLast
        );
        assert_eq!(
            refused(
                0,
                SceneKind::ProductShot,
                Some(bardo_domain::ProductImageId::new())
            ),
            SceneKindError::NoSuchImage
        );
        assert_eq!(
            refused(9, SceneKind::Drawn, None),
            SceneKindError::NoSuchScene(9)
        );
        assert_eq!(plan(&app, &project), before, "nothing changed");
        // The inspector offers what would be taken.
        let view = app.scenes(project.id).unwrap();
        assert!(view.kinds.offers(SceneKind::ProductShot, 0, 4));
        assert!(!view.kinds.offers(SceneKind::ProductInUse, 0, 4));
        assert!(!view.kinds.offers(SceneKind::EndCard, 0, 4));
        assert!(view.kinds.offers(SceneKind::EndCard, 3, 4));
    }

    #[test]
    fn a_product_image_too_small_for_the_frame_is_marked() {
        let h = Harness::new();
        let app = start(&h);
        let (_, project, images) = narrated_ad(
            &h,
            &app,
            &[
                ((600, 600), ProductImageKind::Photo),
                ((2000, 2000), ProductImageKind::Photo),
            ],
        );
        let kinds = app.scenes(project.id).unwrap().kinds;
        assert!(kinds.too_small(images[0].id), "600 px shown past 900");
        assert!(!kinds.too_small(images[1].id));
    }

    #[test]
    fn no_clip_is_made_of_a_composed_scene_in_any_production_mode() {
        let mut h = Harness::new();
        h.more_clips
            .push(std::sync::Arc::new(bardo_ai::KlingClips::new()));
        let app = start(&h);
        let (_, project, _) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        plan_with(&h, &app, &project, usual());
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        for mode in ProductionMode::ALL {
            app.set_project_production(project.id, Some(mode)).unwrap();
            let view = app.scenes(project.id).unwrap();
            assert!(
                view.clips
                    .missing
                    .iter()
                    .all(|index| [0, 2].contains(index)),
                "{mode:?}: {:?}",
                view.clips.missing
            );
            let estimate = app.production(project.id).unwrap();
            assert!(estimate.estimate(mode).unwrap().clips <= 2, "{mode:?}");
        }
        // Every scene animated by the channel's model: the two drawn ones.
        app.set_project_production(project.id, Some(ProductionMode::ChannelModel))
            .unwrap();
        assert_eq!(app.scenes(project.id).unwrap().clips.missing, [0, 2]);
        assert_eq!(
            app.production(project.id)
                .unwrap()
                .estimate(ProductionMode::ChannelModel)
                .unwrap()
                .clips,
            2
        );
        done(
            &app,
            app.generate_missing_clips(project.id, BudgetConsent::Confirmed)
                .unwrap(),
        );
        assert_eq!(h.clips.submissions().len(), 2);
        // Asked for by itself, a composed scene has nothing to animate.
        assert!(
            app.generate_scene_clip(project.id, 1, BudgetConsent::Confirmed)
                .is_err()
        );
        assert_eq!(h.clips.submissions().len(), 2);
    }

    #[test]
    fn a_clip_job_tried_again_leaves_a_scene_that_became_composed() {
        use bardo_domain::{ProviderFailure, ProviderFailureKind};

        let h = Harness::new();
        let app = start(&h);
        let (_, project, _) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        plan_with(&h, &app, &project, usual());
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        app.set_project_production(project.id, Some(ProductionMode::ChannelModel))
            .unwrap();
        // The provider refuses the clip of scene 1: its job fails.
        *h.clips.submit_failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "invalid credentials",
        ));
        let job = app
            .generate_scene_clip(project.id, 0, BudgetConsent::Confirmed)
            .unwrap();
        let failed = crate::scenes::tests::wait_done(&app, job);
        assert_eq!(failed.state(), bardo_domain::JobState::Failed);
        *h.clips.submit_failure.lock().unwrap() = None;
        let sent = *h.clips.sends.lock().unwrap();

        // Meanwhile the scene becomes a product shot.
        let KindChanged::Composing(composing) = app
            .set_scene_kind(project.id, 0, SceneKind::ProductShot, None)
            .unwrap()
        else {
            panic!("expected a job composing it");
        };
        done(&app, composing);

        // Tried again, the job sends nothing: a composed picture is never
        // animated, and the drawing the scene still holds is not shown.
        app.retry_job(job).unwrap();
        done(&app, job);
        assert_eq!(*h.clips.sends.lock().unwrap(), sent);
        let scene = plan_of(&app, &project, 0);
        assert!(scene.clip().is_none() && scene.pending_clip().is_none());
    }

    #[test]
    fn the_cut_plays_a_composed_picture_and_only_ever_zooms_it() {
        use bardo_domain::{Motion, MotionKind, MotionStrength};

        use crate::EditAction;

        let h = Harness::new();
        let app = start(&h);
        let (_, project, _) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        plan_with(&h, &app, &project, usual());
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let shot = plan_of(&app, &project, 1).shot().unwrap().file.clone();

        let mut editor = app.open_editor(project.id).unwrap();
        let clip = &editor.view().clips[1];
        assert!(clip.composed);
        assert_eq!(clip.file.as_deref(), Some(shot.as_str()));
        assert_eq!(clip.image, None, "no model made it");
        assert!(!editor.view().clips[0].composed);
        let motions = |editor: &crate::Editor| -> Vec<Option<Motion>> {
            let timeline = editor.view().timeline.as_ref().unwrap();
            timeline.video().iter().map(|clip| clip.motion).collect()
        };
        // Whatever the channel's pattern gave it, it does not pan.
        let pans = |motion: &Option<Motion>| {
            motion.is_some_and(|motion| {
                !matches!(motion.kind, MotionKind::ZoomIn | MotionKind::ZoomOut)
            })
        };
        assert!(!pans(&motions(&editor)[1]) && !pans(&motions(&editor)[3]));
        // Shown whole, in this frame and in a render of the other shape;
        // a window asked for it changes nothing.
        let framing = |editor: &crate::Editor, index: usize| {
            editor.view().timeline.as_ref().unwrap().video()[index].framing
        };
        assert_eq!(framing(&editor, 1), bardo_domain::Framing::Fit);
        assert_eq!(framing(&editor, 0), bardo_domain::Framing::FILL);
        assert!(
            app.edit(
                &mut editor,
                EditAction::SetFraming {
                    index: 1,
                    framing: bardo_domain::Framing::FILL,
                },
            )
            .is_ok()
        );
        assert_eq!(framing(&editor, 1), bardo_domain::Framing::Fit);

        // A pan asked for it, by itself or for every clip, is the zoom
        // in of the same strength; the drawn scenes pan.
        let pan = Motion::new(MotionKind::PanLeft, MotionStrength::Strong);
        let zoom = Motion::new(MotionKind::ZoomIn, MotionStrength::Strong);
        app.edit(
            &mut editor,
            EditAction::SetMotion {
                index: 1,
                motion: Some(pan),
            },
        )
        .unwrap();
        assert_eq!(motions(&editor)[1], Some(zoom));
        app.edit(&mut editor, EditAction::MotionEverywhere(Some(pan)))
            .unwrap();
        assert_eq!(
            motions(&editor),
            [Some(pan), Some(zoom), Some(pan), Some(zoom)]
        );
        // Opened again, the saved cut reads the same.
        let again = app.open_editor(project.id).unwrap();
        assert_eq!(
            motions(&again),
            [Some(pan), Some(zoom), Some(pan), Some(zoom)]
        );
        // A zoom out is its own to keep.
        let out = Motion::new(MotionKind::ZoomOut, MotionStrength::Subtle);
        app.edit(
            &mut editor,
            EditAction::SetMotion {
                index: 3,
                motion: Some(out),
            },
        )
        .unwrap();
        assert_eq!(motions(&editor)[3], Some(out));
    }

    #[test]
    fn an_ads_estimate_counts_its_composed_scenes_as_free_before_and_after_its_plan() {
        const IMAGE: u64 = 101_570;
        let h = Harness::new();
        let app = start(&h);
        let (product, _, project) = ad_project(&h, &app);
        let economy = || {
            app.production(project.id)
                .unwrap()
                .estimate(ProductionMode::Economy)
                .unwrap()
                .remaining
                .micros()
        };
        let clips = || {
            app.production(project.id)
                .unwrap()
                .estimate(ProductionMode::ChannelModel)
                .unwrap()
                .clips
        };
        // Thirty seconds, a scene every three: ten scenes, all drawn
        // while the product has no image.
        assert_eq!(clips(), 10);
        let all_drawn = economy();
        // With an image, two product shots and the end card are neither
        // drawn nor animated.
        add_image(&app, &product, (1200, 900), ProductImageKind::Photo);
        assert_eq!(clips(), 7);
        assert_eq!(all_drawn - economy(), 3 * IMAGE);

        // Narrated: a scene for each of its four sentences, of which the
        // hook stays drawn and three are composed.
        let h = Harness::new();
        let app = start(&h);
        let (_, project, _) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        let view = app.production(project.id).unwrap();
        assert_eq!(
            view.estimate(ProductionMode::ChannelModel).unwrap().clips,
            1
        );

        // After the plan: what is left is its two drawn scenes.
        plan_with(&h, &app, &project, usual());
        let view = app.production(project.id).unwrap();
        assert_eq!(
            view.estimate(ProductionMode::Economy)
                .unwrap()
                .remaining
                .micros(),
            2 * IMAGE
        );
        assert_eq!(
            view.estimate(ProductionMode::ChannelModel).unwrap().clips,
            2
        );
    }

    #[test]
    fn a_video_of_another_shape_has_its_product_shots_composed_again_for_free() {
        let h = Harness::new();
        let app = start(&h);
        let (_, project, _) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        let before = plan_with(&h, &app, &project, usual());
        app.set_project_format(project.id, Some(VideoFormat::Long))
            .unwrap();
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.missing_shots, [1, 3], "composed for a vertical frame");
        // They read as still to make, here and on the stage.
        let states = crate::scene_states(&view, crate::Stage::Scenes);
        assert_eq!(states[1], crate::SceneState::ToDraw);
        assert_eq!(states[3], crate::SceneState::ToDraw);
        assert_eq!(
            crate::scene_states(&view, crate::Stage::Clips)[1],
            crate::SceneState::ToDraw
        );

        // One of them by itself: composed, with no key asked and no cost.
        done(
            &app,
            app.regenerate_scene_image(project.id, 1, BudgetConsent::Ask)
                .unwrap(),
        );
        let wide = plan(&app, &project)
            .scene(1)
            .unwrap()
            .shot()
            .unwrap()
            .clone();
        assert_eq!(size_of(&h, &project, &wide.file), (1920, 1080));
        let old = &before.scene(1).unwrap().shot().unwrap().file;
        assert!(!h.files.exists(project.id, old));
        assert_eq!(drawings(&h), 0);

        // The rest with the missing drawings.
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let after = plan(&app, &project);
        let end = after.scene(3).unwrap().shot().unwrap();
        assert_eq!(size_of(&h, &project, &end.file), (1920, 1080));
        assert_eq!(
            app.scenes(project.id).unwrap().missing_shots,
            Vec::<usize>::new()
        );
        assert_eq!(drawings(&h), 2);
        assert_eq!(image_costs(&h, &project), 2);
    }

    #[test]
    fn product_shots_are_composed_again_with_no_key_while_drawings_wait() {
        let h = Harness::new();
        let mut app = start(&h);
        let (_, project, _) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        plan_with(&h, &app, &project, usual());
        app.set_project_format(project.id, Some(VideoFormat::Long))
            .unwrap();
        app.remove_provider_key(Provider::Gemini).unwrap();
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.missing_shots, [1, 3]);
        assert!(view.missing_images() > 0, "drawings wait too");

        done(&app, app.compose_missing_shots(project.id).unwrap());
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.missing_shots, Vec::<usize>::new());
        assert!(view.missing_images() > 0, "nothing was drawn");
        assert_eq!(drawings(&h), 0);
        assert_eq!(image_costs(&h, &project), 0);
        assert!(matches!(
            app.compose_missing_shots(project.id),
            Err(SceneError::NothingToGenerate)
        ));
    }

    #[test]
    fn a_product_shot_whose_image_is_gone_says_so_and_the_rest_is_still_made() {
        let h = Harness::new();
        let app = start(&h);
        let (product, project, images) = narrated_ad(&h, &app, &A_SCREEN_AND_A_PHOTO);
        plan_with(&h, &app, &project, usual());
        app.set_project_format(project.id, Some(VideoFormat::Long))
            .unwrap();
        app.remove_product_image(product.id, images[1].id).unwrap();

        let job = crate::scenes::tests::wait_done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(job.state(), bardo_domain::JobState::Failed);
        assert!(
            job.failure()
                .unwrap()
                .detail
                .contains("the product no longer has the image it shows"),
            "{:?}",
            job.failure()
        );
        let plan = plan(&app, &project);
        // The scene whose image is gone is marked; the end card and the
        // drawings were made all the same.
        assert!(plan.scene(1).unwrap().failure().is_some());
        assert_eq!(
            plan.scene(1).unwrap().shot().map(|shot| shot.aspect),
            Some(bardo_domain::AspectRatio::Vertical),
            "it keeps the picture it had"
        );
        assert_eq!(
            plan.scene(3).unwrap().shot().map(|shot| shot.aspect),
            Some(bardo_domain::AspectRatio::Landscape)
        );
        assert_eq!(plan.missing_images(), Vec::<usize>::new());
        // Shown another of the product's images, it is composed.
        let KindChanged::Composing(job) = app
            .set_scene_kind(project.id, 1, SceneKind::ProductShot, Some(images[0].id))
            .unwrap()
        else {
            panic!("expected a job composing it");
        };
        done(&app, job);
        let scene = plan_of(&app, &project, 1);
        assert_eq!(scene.shot().unwrap().image, images[0].id);
        assert!(scene.failure().is_none());
    }

    fn plan_of(app: &Bardo, project: &VideoProject, index: usize) -> bardo_domain::Scene {
        plan(app, project).scene(index).unwrap().clone()
    }
}
