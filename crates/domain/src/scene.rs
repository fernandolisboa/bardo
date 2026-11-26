//! Scenes (PRD stories 34-36, 42, 44): the narration split into scenes,
//! each with the image prompt that draws it and the image drawn. Claude
//! plans the scenes on sentence boundaries of the narration, so every scene
//! starts and ends where the word timings say; the user edits any prompt;
//! the image provider draws one image per scene. Regenerating one scene's
//! image leaves the others alone and keeps the current image until the user
//! accepts the new one.
//!
//! A plan is a storyboard (#258): it names the video's recurring people
//! (the cast, #246) and places once, each with one fixed description, and
//! every scene says who it shows and where it happens. A scene's image is
//! drawn with the cast's portraits and with earlier scenes of the video as
//! references, so the place, the light and the look hold from one scene to
//! the next.

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{
    AspectRatio, BackdropId, ClipModelRef, ComposedShot, Generation, GenerationId, JobFailureKind,
    KindChange, Narration, NarrationId, ProductImageId, ProductPictures, ProfileId,
    RepositoryError, SceneKind, SceneKindError, ShotBackdropDrawing, ShotLook, StretchLimit,
    StyleChoice, VideoProjectId, spoken_words,
};

uuid_id!(
    /// Identifies one scene plan of a video project.
    ScenePlanId
);

/// The longest sentence a scene can start inside of, in words. A run-on
/// paragraph without punctuation is cut here, so a scene can still change
/// within it.
pub const MAX_SENTENCE_WORDS: usize = 40;

/// A sentence of the narration: where scenes may start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    /// Indexes of its words in the narration's word timings.
    pub words: Range<usize>,
    /// Byte range in the narration's text.
    pub text: Range<usize>,
    /// When its first word starts and its last word ends.
    pub start: Duration,
    pub end: Duration,
}

/// The sentences of the narration, in order: a word ending with `.`, `!`,
/// `?` or `…` (closing quotes and brackets aside) or followed by a line
/// break ends one, and none runs longer than `MAX_SENTENCE_WORDS`.
pub fn sentences(narration: &Narration) -> Vec<Sentence> {
    let words = narration.words.as_slice();
    let ranges: Vec<Range<usize>> = words.iter().map(|word| word.text.clone()).collect();
    sentence_spans(narration.text.as_str(), &ranges)
        .into_iter()
        .map(|span| {
            let (opening, closing) = (&words[span.start], &words[span.end - 1]);
            Sentence {
                text: opening.text.start..closing.text.end,
                start: opening.start,
                end: closing.end.max(opening.start),
                words: span,
            }
        })
        .collect()
}

/// The sentences of `text` as runs of its `words` (their places in
/// `text`, in order), by the rules of [`sentences`]. They depend on the
/// text alone, not on when it is spoken.
pub(crate) fn sentence_spans(text: &str, words: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut first = 0;
    for (index, word) in words.iter().enumerate() {
        let next = words.get(index + 1);
        let ends = next.is_none_or(|next| {
            ends_sentence(&text[word.clone()])
                || text[word.end..next.start].contains('\n')
                || index + 1 - first >= MAX_SENTENCE_WORDS
        });
        if ends {
            spans.push(first..index + 1);
            first = index + 1;
        }
    }
    spans
}

/// A word that ends a sentence: `.`, `!`, `?` or `…`, past closing quotes
/// and brackets.
pub(crate) fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '”', '’', '»', ')', ']', '*'])
        .ends_with(['.', '!', '?', '…'])
}

/// Why prompt text is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SceneFieldError {
    PromptRequired,
    PromptTooLong,
}

impl SceneFieldError {
    pub const ALL: [SceneFieldError; 2] = [
        SceneFieldError::PromptRequired,
        SceneFieldError::PromptTooLong,
    ];
}

/// A scene's image prompt. Always valid: present, ends trimmed, within
/// limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePrompt(String);

impl ScenePrompt {
    /// In characters. Image models read a paragraph; more is a mistake.
    pub const MAX_CHARS: usize = 4_000;

    pub fn new(text: &str) -> Result<Self, SceneFieldError> {
        let text = text.trim();
        if text.is_empty() {
            Err(SceneFieldError::PromptRequired)
        } else if text.chars().count() > Self::MAX_CHARS {
            Err(SceneFieldError::PromptTooLong)
        } else {
            Ok(Self(text.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An image drawn for a scene: the file in the project folder and the
/// generation that drew it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneImage {
    /// The file's name in the project folder.
    pub file: String,
    pub generation: Generation,
}

/// A user action on a scene's image that does not apply right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no new image of this scene is waiting for review")]
pub struct NoPendingImage;

/// Who an image of a scene was drawn showing (#269): each cast member, by
/// their index in the plan's cast, with the generation of the portrait
/// sent for them, or `None` for one drawn from the description alone.
pub type PortraitsUsed = Vec<(usize, Option<GenerationId>)>;

/// The most recurring characters a plan keeps, and so the most reference
/// portraits sent with one scene's image.
pub const MAX_CAST: usize = 3;

/// The longest name of a cast member, in characters.
pub const MAX_CAST_NAME_CHARS: usize = 60;

/// One of the video's recurring characters (#246) as the planner proposed
/// them: what the scene prompts call them and their fixed appearance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastDraft {
    pub name: String,
    pub description: String,
}

/// A recurring character of the video: the planner names them once with a
/// fixed appearance, the image provider draws their reference portrait,
/// and every scene that shows them is drawn with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastMember {
    /// What the scene prompts call them, e.g. "the chef".
    pub name: String,
    /// The appearance as planned. The user's edits change `description`.
    generated_description: ScenePrompt,
    description: ScenePrompt,
    /// The reference portrait, once drawn.
    portrait: Option<SceneImage>,
    /// Why the last attempt to draw the portrait failed, until one
    /// succeeds.
    failure: Option<JobFailureKind>,
    /// The user chose to draw this person from the description alone
    /// (#269): no portrait is drawn or sent for them, and their face may
    /// change from scene to scene.
    described_only: bool,
}

/// Every stored field of a cast member, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastRecord {
    pub name: String,
    pub generated_description: ScenePrompt,
    pub description: ScenePrompt,
    pub portrait: Option<SceneImage>,
    pub failure: Option<JobFailureKind>,
}

impl CastMember {
    pub fn restore(record: CastRecord) -> Self {
        Self {
            name: record.name,
            generated_description: record.generated_description,
            description: record.description,
            portrait: record.portrait,
            failure: record.failure,
            described_only: false,
        }
    }

    /// The member drawn from the description alone or not, as an adapter
    /// rebuilds it.
    pub fn with_described_only(mut self, described_only: bool) -> Self {
        self.described_only = described_only;
        self
    }

    /// A planned member, or `None` when the name or the description is not
    /// usable.
    fn planned(draft: &CastDraft) -> Option<Self> {
        let name: String = draft
            .name
            .trim()
            .chars()
            .take(MAX_CAST_NAME_CHARS)
            .collect();
        let description = ScenePrompt::new(&draft.description).ok()?;
        (!name.trim().is_empty()).then(|| Self {
            name: name.trim().to_owned(),
            generated_description: description.clone(),
            description,
            portrait: None,
            failure: None,
            described_only: false,
        })
    }

    pub fn description(&self) -> &ScenePrompt {
        &self.description
    }

    /// The name without a leading article ("the chef" is "chef"), as an
    /// edited prompt may still call them.
    fn called(&self) -> &str {
        without_article(&self.name)
    }

    pub fn generated_description(&self) -> &ScenePrompt {
        &self.generated_description
    }

    /// Whether the user changed the description since it was planned.
    pub fn is_edited(&self) -> bool {
        self.description != self.generated_description
    }

    pub fn portrait(&self) -> Option<&SceneImage> {
        self.portrait.as_ref()
    }

    pub fn failure(&self) -> Option<JobFailureKind> {
        self.failure
    }

    /// Replaces the description with the user's. The portrait no longer
    /// shows it, so it goes: the next image drawn with this member draws
    /// a new one first. Returns whether the description changed and the
    /// portrait it dropped, whose file can go.
    pub fn edit_description(&mut self, description: ScenePrompt) -> (bool, Option<SceneImage>) {
        if description == self.description {
            return (false, None);
        }
        self.description = description;
        // A failure was about the old description.
        self.failure = None;
        (true, self.portrait.take())
    }

    /// Takes a newly drawn portrait in place of the current one. Returns
    /// the replaced one, whose file can go.
    pub fn set_portrait(&mut self, portrait: SceneImage) -> Option<SceneImage> {
        self.failure = None;
        self.portrait.replace(portrait)
    }

    /// Records that drawing the portrait failed. The portrait it has stays.
    pub fn fail(&mut self, kind: JobFailureKind) {
        self.failure = Some(kind);
    }

    /// Whether the user chose to draw this person from the description
    /// alone, without a portrait.
    pub fn is_described_only(&self) -> bool {
        self.described_only
    }

    /// Chooses to draw this person from the description alone, or with a
    /// portrait again. Returns whether the choice changed.
    pub fn set_described_only(&mut self, described_only: bool) -> bool {
        let changed = self.described_only != described_only;
        self.described_only = described_only;
        if described_only {
            // A portrait that failed is no longer asked for.
            self.failure = None;
        }
        changed
    }

    /// Lets go of a portrait whose file is gone or will not read, noting
    /// why: the person has none again, so the next image drawn with them
    /// draws one first. Returns the portrait, whose file can go.
    pub fn drop_portrait(&mut self, kind: JobFailureKind) -> Option<SceneImage> {
        self.failure = Some(kind);
        self.portrait.take()
    }

    /// The portrait, when it was drawn in `style`: a portrait in another
    /// look would pull the scenes drawn with it back to that look.
    pub fn portrait_in(&self, style: StyleChoice) -> Option<&SceneImage> {
        self.portrait
            .as_ref()
            .filter(|portrait| style.drew(&portrait.generation.prompt))
    }

    /// What the image model draws the reference portrait from, in the
    /// video's `style`. A style may ask for hard shadows, haze, a set or an
    /// angle, and the portrait is what every scene with the person is
    /// drawn from, so after the style it says again what a portrait keeps.
    pub fn portrait_prompt(&self, style: StyleChoice) -> String {
        let portrait = style.applied(&format!(
            "A reference portrait of one character of a video, {name}: {description}\n\n\
             Exactly one person, from the waist up, the whole head in frame, facing the \
             camera with a neutral expression, against a plain, evenly lit background. \
             Nothing covers the face. No other people, no text, logos or watermarks.",
            name = self.name,
            description = self.description.as_str(),
        ));
        match style {
            StyleChoice::FromNotes => portrait,
            StyleChoice::Catalog(_) => format!("{portrait} {PORTRAIT_IN_A_STYLE}"),
        }
    }
}

/// What a portrait drawn in a catalog style ends with.
const PORTRAIT_IN_A_STYLE: &str = "In this portrait the style decides only the medium, the line, \
    the texture and the palette: keep the framing, the neutral expression and the plain, evenly \
    lit background asked for above, with the whole face clearly lit.";

/// The prompt an image of a scene is drawn from: the scene's prompt, who
/// each cast member it shows is, where it happens, and what each other
/// scene sent with it is for. `referenced` are the members whose portrait
/// goes with the request, in that order; the others are only described.
/// The other scenes' images follow the portraits, in `setting.shots`
/// order.
pub fn scene_image_prompt(
    prompt: &str,
    shown: &[&CastMember],
    referenced: &[bool],
    setting: SceneSetting<'_>,
) -> String {
    let mut text = prompt.to_owned();
    let with: Vec<&CastMember> = shown
        .iter()
        .zip(referenced.iter().chain(std::iter::repeat(&false)))
        .filter(|(_, referenced)| **referenced)
        .map(|(member, _)| *member)
        .collect();
    if !with.is_empty() {
        text.push_str(
            "\n\nThe reference images show the people in this scene, in this order. Each \
             person must look exactly as in their reference: the same face, skin tone, age, \
             hair, build and clothes. Do not copy the reference's pose, framing or \
             background.",
        );
        for (number, member) in with.iter().enumerate() {
            text.push_str(&format!(
                "\nReference {}: {}, {}",
                number + 1,
                member.name,
                member.description.as_str()
            ));
        }
    }
    let described: Vec<&&CastMember> = shown
        .iter()
        .zip(referenced.iter().chain(std::iter::repeat(&false)))
        .filter(|(_, referenced)| !**referenced)
        .map(|(member, _)| member)
        .collect();
    if !described.is_empty() {
        text.push_str("\n\nThe people in this scene look like this:");
        for member in described {
            text.push_str(&format!(
                "\n{}: {}",
                member.name,
                member.description.as_str()
            ));
        }
    }
    if let Some(looks) = setting.product {
        text.push_str(&format!(
            "\n\nReference {}: {PRODUCT_REFERENCE_RULE}",
            with.len() + 1
        ));
        if !looks.trim().is_empty() {
            text.push_str("\nHow the product looks: ");
            text.push_str(looks.trim());
        }
    }
    if let Some(place) = setting.place {
        text.push_str("\n\n");
        text.push_str(&place.setting());
    }
    if let Some(style) = setting.style.line() {
        text.push_str("\n\n");
        text.push_str(&style);
    }
    if !setting.shots.is_empty() {
        text.push_str(if with.is_empty() && setting.product.is_none() {
            "\n\nThe reference images are other shots of the same video, so that this image \
             belongs with them."
        } else {
            "\n\nThe reference images after those are other shots of the same video, so that \
             this image belongs with them."
        });
    }
    let product = usize::from(setting.product.is_some());
    for (offset, shot) in setting.shots.iter().enumerate() {
        let number = with.len() + product + offset + 1;
        text.push_str(&format!("\nReference {number}: "));
        text.push_str(match shot {
            OtherShot::SamePlace => {
                "another shot in this same place. Keep the place as it is there: its \
                 architecture, objects, colors, light and time of day, and the same rendering \
                 style. People in it are in this scene only if this prompt names them. Draw \
                 a new shot, framed as this prompt says; do not repeat the reference's \
                 composition."
            }
            OtherShot::Before => {
                "the shot right before this one, in this same place. This scene continues \
                 from it: keep the place, the light, the time of day, what people wear and \
                 where things are, and the same rendering style. People in it are in this \
                 scene only if this prompt names them. Draw the next moment as a new shot, \
                 framed as this prompt says; do not repeat the reference's composition."
            }
            OtherShot::Look => {
                "another shot of this video. Match only how it is rendered: the medium, the \
                 line, the texture and the level of detail. Its place, its light and colors, \
                 its people and its composition are not part of this scene."
            }
        });
    }
    text
}

/// A user action on a cast member that does not apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the plan has no cast member {0}")]
pub struct NoSuchCastMember(pub usize);

/// The most recurring places a plan keeps.
pub const MAX_PLACES: usize = 8;

/// One of the video's recurring places (#258) as the planner proposed it:
/// what the scene prompts call it and its fixed description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceDraft {
    pub name: String,
    pub description: String,
}

/// A recurring place of the video: the planner names it once with one
/// fixed description (the place, its period, time of day, light and
/// colors), and every scene that happens there is drawn with it and with
/// an earlier scene of the place as a reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// What the scene prompts call it, e.g. "the kitchen".
    pub name: String,
    /// The description as planned. The user's edits change `description`.
    generated_description: ScenePrompt,
    description: ScenePrompt,
}

/// Every stored field of a place, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceRecord {
    pub name: String,
    pub generated_description: ScenePrompt,
    pub description: ScenePrompt,
}

impl Place {
    pub fn restore(record: PlaceRecord) -> Self {
        Self {
            name: record.name,
            generated_description: record.generated_description,
            description: record.description,
        }
    }

    /// A planned place, or `None` when the name or the description is not
    /// usable.
    fn planned(draft: &PlaceDraft) -> Option<Self> {
        let name: String = draft
            .name
            .trim()
            .chars()
            .take(MAX_CAST_NAME_CHARS)
            .collect();
        let description = ScenePrompt::new(&draft.description).ok()?;
        (!name.trim().is_empty()).then(|| Self {
            name: name.trim().to_owned(),
            generated_description: description.clone(),
            description,
        })
    }

    pub fn description(&self) -> &ScenePrompt {
        &self.description
    }

    pub fn generated_description(&self) -> &ScenePrompt {
        &self.generated_description
    }

    /// Whether the user changed the description since it was planned.
    pub fn is_edited(&self) -> bool {
        self.description != self.generated_description
    }

    /// Replaces the description with the user's; the next images drawn in
    /// the place follow it. Returns whether it changed.
    pub fn edit_description(&mut self, description: ScenePrompt) -> bool {
        if description == self.description {
            return false;
        }
        self.description = description;
        true
    }

    /// The name without a leading article ("the kitchen" is "kitchen"),
    /// as an edited prompt may still call it.
    fn called(&self) -> &str {
        without_article(&self.name)
    }

    /// What an image prompt says about a scene that happens here.
    pub fn setting(&self) -> String {
        format!(
            "The scene happens in {}: {}",
            self.name,
            self.description.as_str()
        )
    }
}

/// Whether `text` names `name` as whole words, whatever the case: "the
/// bar" is not named by "barn".
fn names(text: &str, name: &str) -> bool {
    let text = text.to_lowercase();
    let name = name.to_lowercase();
    !name.is_empty()
        && text.match_indices(&name).any(|(at, found)| {
            let before = text[..at].chars().next_back();
            let after = text[at + found.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
}

/// A user action on a place that does not apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the plan has no place {0}")]
pub struct NoSuchPlace(pub usize);

/// `name` without a leading English article.
fn without_article(name: &str) -> &str {
    let lower = name.to_lowercase();
    ["the ", "a ", "an "]
        .iter()
        .find(|article| lower.starts_with(**article))
        .map_or(name, |article| &name[article.len()..])
}

/// What another scene of the video is to a scene drawn with its image as
/// a reference (#258).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OtherShot {
    /// Another scene in the same place: the place holds.
    SamePlace,
    /// The scene right before, in the same place: the scene continues
    /// from it.
    Before,
    /// A scene of another place, for a scene whose place has no image
    /// yet: only the video's look holds.
    Look,
}

/// Where a scene happens and which other scenes go with its image
/// request, in the order of their reference images (after the cast's
/// portraits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneSetting<'a> {
    /// The scene's place, described in the prompt.
    pub place: Option<&'a Place>,
    /// What each other scene sent as a reference is to this one.
    pub shots: &'a [OtherShot],
    /// The video's visual style (#259), added after the people and the
    /// place.
    pub style: StyleChoice,
    /// For a scene of the product in use (#279): the product's photo is
    /// the reference right after the people's, and this is how the
    /// product looks.
    pub product: Option<&'a str>,
}

/// What a product in use scene's image request says of the product's
/// reference (#279), before how it looks.
pub const PRODUCT_REFERENCE_RULE: &str = "the product itself. Keep the product's shape, colors, \
label and proportions exactly as in this reference; show it being used as this prompt says, \
not the reference's framing or background.";

/// A clip made from a scene's image: the file in the project folder, its
/// length, the image it animated and the generation that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneClip {
    /// The file's name in the project folder.
    pub file: String,
    /// The length asked for, in seconds.
    pub seconds: u32,
    /// Set when it was asked for shorter than its scene on purpose
    /// (`ClipLength`): it plays slowed, to end when the scene does, within
    /// the limit it was made under.
    pub stretch: Option<StretchLimit>,
    /// The generation of the image it starts from.
    pub source_image: GenerationId,
    pub generation: Generation,
}

impl SceneClip {
    /// How long the clip runs at the speed it was made.
    pub fn recorded(&self) -> Duration {
        Duration::from_secs(self.seconds.into())
    }

    /// What a stretched clip plays past its scene's end when its limit
    /// allows: the picture a transition out of it shows, which would
    /// otherwise be its last frame held.
    pub const STRETCH_HANDLE: Duration = Duration::from_millis(500);

    /// How long it plays over a scene of `scene`: its own length, or for
    /// a stretched clip the scene's and a handle after it, as far as its
    /// limit goes. Past that it holds its last frame, as any clip shorter
    /// than its scene does.
    pub fn played_over(&self, scene: Duration) -> Duration {
        match self.stretch {
            Some(limit) if scene > self.recorded() => {
                (scene + Self::STRETCH_HANDLE).min(limit.longest(self.seconds))
            }
            _ => self.recorded(),
        }
    }
}

/// A user action on a scene's clip that does not apply right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no new clip of this scene is waiting for review")]
pub struct NoPendingClip;

/// One scene: a stretch of the narration and the image shown over it, or
/// a clip animating that image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    pub start: Duration,
    pub end: Duration,
    /// The narration spoken over it.
    pub text: String,
    /// The prompt as planned. The user's edits change `prompt` only.
    generated_prompt: ScenePrompt,
    prompt: ScenePrompt,
    image: Option<SceneImage>,
    /// A regenerated image waiting for the user to accept or reject it.
    pending: Option<SceneImage>,
    /// Why the last attempt to draw it failed, until one succeeds.
    failure: Option<JobFailureKind>,
    /// How the clip moves, when the user wrote it; the image prompt
    /// otherwise.
    motion_prompt: Option<ScenePrompt>,
    /// The video model for this scene, instead of the channel's.
    clip_model: Option<ClipModelRef>,
    /// The clip the rough cut uses instead of the still image.
    clip: Option<SceneClip>,
    /// A new clip waiting for the user to accept or reject it.
    pending_clip: Option<SceneClip>,
    /// Why the last attempt to make a clip failed, until one succeeds.
    clip_failure: Option<JobFailureKind>,
    /// The cast members it shows, as indexes into the plan's cast.
    cast: Vec<usize>,
    /// How the clip moves, as planned; plans made before #246 have none.
    planned_motion: Option<ScenePrompt>,
    /// The recurring place it happens in, as an index into the plan's
    /// places; `None` for a place seen once or a plan made before #258.
    place: Option<usize>,
    /// The portraits `image` was drawn with; `None` for an image from
    /// before they were recorded (#269).
    image_portraits: Option<PortraitsUsed>,
    /// The portraits `pending` was drawn with.
    pending_portraits: Option<PortraitsUsed>,
    /// What the scene is, in an ad's plan (#278); a drawn scene in every
    /// other.
    kind: SceneKind,
    /// The product image a composed scene shows.
    product_image: Option<ProductImageId>,
    /// The picture Bardo composed for it, while it is a composed scene.
    shot: Option<ComposedShot>,
    /// How its product shot looks (#285): layout, place and backdrop.
    look: ShotLook,
    /// What an end card (#281) shows besides its image and look, as the
    /// fingerprint of the card's design (`crate::card_fingerprint`):
    /// `None` until Bardo first composes it.
    card: Option<String>,
    /// Which of its images were drawn with the product as a reference,
    /// and which picture and clip the user checked against the product's
    /// own image (#279).
    checks: ProductChecks,
}

/// What a scene of the product in use (#279) holds about its product:
/// whether its image and the image waiting for review were drawn with
/// the product's photo as a reference, and the image and the clip the
/// user checked against the real product. A check names what it was
/// made on, so a new image or clip is not checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProductChecks {
    pub image_drawn: bool,
    pub pending_drawn: bool,
    /// The image the user checked, by its generation.
    pub picture: Option<GenerationId>,
    /// The clip the user checked, by its generation.
    pub clip: Option<GenerationId>,
}

/// The sentence a product in use scene's clip prompt starts with (#279).
pub const PRODUCT_MOTION_RULE: &str = "The product stays rigid and unchanged: its shape, \
colors, label and proportions never change. Only the camera or the person moves.";

/// A product check that does not apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProductCheckError {
    #[error("the scene does not show the product in use")]
    NotInUse,
    /// Its image was not drawn with the product as a reference.
    #[error("the scene's image was not drawn with the product")]
    NotDrawn,
    #[error("the scene has no clip to check")]
    NoClip,
    /// Its picture is not checked, or its clip was made of another one.
    #[error("the scene's picture is not checked, or its clip is of another picture")]
    PictureUnchecked,
}

/// Every stored field of a scene, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneRecord {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
    pub generated_prompt: ScenePrompt,
    pub prompt: ScenePrompt,
    pub image: Option<SceneImage>,
    pub pending: Option<SceneImage>,
    pub failure: Option<JobFailureKind>,
    pub motion_prompt: Option<ScenePrompt>,
    pub clip_model: Option<ClipModelRef>,
    pub clip: Option<SceneClip>,
    pub pending_clip: Option<SceneClip>,
    pub clip_failure: Option<JobFailureKind>,
}

impl Scene {
    pub fn restore(record: SceneRecord) -> Self {
        Self {
            start: record.start,
            end: record.end,
            text: record.text,
            generated_prompt: record.generated_prompt,
            prompt: record.prompt,
            image: record.image,
            pending: record.pending,
            failure: record.failure,
            motion_prompt: record.motion_prompt,
            clip_model: record.clip_model,
            clip: record.clip,
            pending_clip: record.pending_clip,
            clip_failure: record.clip_failure,
            cast: Vec::new(),
            planned_motion: None,
            place: None,
            image_portraits: None,
            pending_portraits: None,
            kind: SceneKind::Drawn,
            product_image: None,
            shot: None,
            look: ShotLook::default(),
            card: None,
            checks: ProductChecks::default(),
        }
    }

    /// The scene showing `cast` (indexes into its plan's cast), as an
    /// adapter rebuilds it.
    pub fn with_cast(mut self, cast: Vec<usize>) -> Self {
        self.cast = cast;
        self
    }

    /// The scene with its planned motion prompt, as an adapter rebuilds it.
    pub fn with_planned_motion(mut self, motion: Option<ScenePrompt>) -> Self {
        self.planned_motion = motion;
        self
    }

    /// The scene happening in `place` (an index into its plan's places),
    /// as an adapter rebuilds it.
    pub fn with_place(mut self, place: Option<usize>) -> Self {
        self.place = place;
        self
    }

    /// The scene with the portraits its image and the image waiting for
    /// review were drawn with, as an adapter rebuilds it. One without its
    /// image is dropped.
    pub fn with_portraits(
        mut self,
        image: Option<PortraitsUsed>,
        pending: Option<PortraitsUsed>,
    ) -> Self {
        self.image_portraits = image.filter(|_| self.image.is_some());
        self.pending_portraits = pending.filter(|_| self.pending.is_some());
        self
    }

    /// A scene of a new plan: no image or clip yet.
    fn planned(start: Duration, end: Duration, text: String, prompt: ScenePrompt) -> Self {
        Self {
            start,
            end,
            text,
            generated_prompt: prompt.clone(),
            prompt,
            image: None,
            pending: None,
            failure: None,
            motion_prompt: None,
            clip_model: None,
            clip: None,
            pending_clip: None,
            clip_failure: None,
            cast: Vec::new(),
            planned_motion: None,
            place: None,
            image_portraits: None,
            pending_portraits: None,
            kind: SceneKind::Drawn,
            product_image: None,
            shot: None,
            look: ShotLook::default(),
            card: None,
            checks: ProductChecks::default(),
        }
    }

    /// The scene of an ad's plan with its kind, the product image it
    /// shows and the picture composed for it, as an adapter rebuilds it.
    /// A composed picture is kept only while the scene is composed.
    pub fn with_kind(
        mut self,
        kind: SceneKind,
        product_image: Option<ProductImageId>,
        shot: Option<ComposedShot>,
    ) -> Self {
        self.kind = kind;
        self.product_image = product_image;
        self.shot = shot.filter(|_| self.is_composed());
        self
    }

    /// The scene with how its product shot looks, as an adapter rebuilds
    /// it.
    pub fn with_look(mut self, look: ShotLook) -> Self {
        self.look = look;
        self
    }

    /// How its product shot looks: kept while the scene is drawn, for
    /// when it is composed again.
    pub fn look(&self) -> ShotLook {
        self.look
    }

    /// The scene with the end card design it shows, as an adapter
    /// rebuilds it.
    pub fn with_card(mut self, card: Option<String>) -> Self {
        self.card = card;
        self
    }

    /// The end card design the scene shows, by its fingerprint (#281).
    pub fn card(&self) -> Option<&str> {
        self.card.as_deref()
    }

    /// Whether the scene is the end card (#281).
    pub fn is_end_card(&self) -> bool {
        self.kind == SceneKind::EndCard
    }

    /// The scene with what it holds about its product, as an adapter
    /// rebuilds it. A flag or check about an image or clip it does not
    /// have is dropped.
    pub fn with_product_checks(mut self, checks: ProductChecks) -> Self {
        self.checks = ProductChecks {
            image_drawn: checks.image_drawn && self.image.is_some(),
            pending_drawn: checks.pending_drawn && self.pending.is_some(),
            picture: checks
                .picture
                .filter(|id| self.image.as_ref().is_some_and(|i| i.generation.id == *id)),
            clip: checks
                .clip
                .filter(|id| self.clip.as_ref().is_some_and(|c| c.generation.id == *id)),
        };
        self
    }

    /// What it holds about its product, for adapters.
    pub fn product_checks(&self) -> ProductChecks {
        self.checks
    }

    /// Whether it shows the product in use (#279).
    pub fn shows_product_in_use(&self) -> bool {
        self.kind == SceneKind::ProductInUse
    }

    /// Whether its image was drawn with the product as a reference.
    pub fn product_drawn(&self) -> bool {
        self.image.is_some() && self.checks.image_drawn
    }

    /// Whether the image waiting for review was drawn with the product.
    pub fn pending_product_drawn(&self) -> bool {
        self.pending.is_some() && self.checks.pending_drawn
    }

    /// Whether its picture may be shown and animated: always, but for a
    /// scene of the product in use, whose image drawn with the product
    /// the user checked against the real product.
    pub fn product_checked(&self) -> bool {
        !self.shows_product_in_use()
            || (self.product_drawn()
                && self.checks.picture.is_some()
                && self.checks.picture == self.image.as_ref().map(|i| i.generation.id))
    }

    /// Whether its picture waits for the user's check: drawn with the
    /// product and not checked yet.
    pub fn needs_product_check(&self) -> bool {
        self.shows_product_in_use() && self.product_drawn() && !self.product_checked()
    }

    /// Whether the cut may use its clip: always, but for a scene of the
    /// product in use, whose clip the user checked.
    pub fn clip_checked(&self) -> bool {
        !self.shows_product_in_use()
            || (self.product_checked()
                && !self.is_clip_stale()
                && self
                    .clip
                    .as_ref()
                    .is_some_and(|clip| self.checks.clip == Some(clip.generation.id)))
    }

    /// Whether its clip may be checked: the product in use whose picture
    /// is checked, with a clip made of that picture.
    pub fn may_check_clip(&self) -> bool {
        self.shows_product_in_use()
            && self.product_checked()
            && self.clip.is_some()
            && !self.is_clip_stale()
    }

    /// Checks its picture: the product in it matches the real one.
    /// Returns whether that changed anything.
    pub fn check_product(&mut self) -> Result<bool, ProductCheckError> {
        if !self.shows_product_in_use() {
            return Err(ProductCheckError::NotInUse);
        }
        if !self.product_drawn() {
            return Err(ProductCheckError::NotDrawn);
        }
        let image = self.image.as_ref().map(|image| image.generation.id);
        Ok(self.checks.picture.replace(image.expect("drawn")) != image)
    }

    /// Checks its clip: the product stays as it is all through it.
    pub fn check_product_clip(&mut self) -> Result<bool, ProductCheckError> {
        if !self.shows_product_in_use() {
            return Err(ProductCheckError::NotInUse);
        }
        if !self.product_checked() || self.is_clip_stale() {
            return Err(ProductCheckError::PictureUnchecked);
        }
        let clip = self
            .clip
            .as_ref()
            .map(|clip| clip.generation.id)
            .ok_or(ProductCheckError::NoClip)?;
        Ok(self.checks.clip.replace(clip) != Some(clip))
    }

    /// What its clip is asked to do: the motion prompt, and for the
    /// product in use, first that the product stays as it is (first, so a
    /// provider that keeps only the start of a long prompt keeps it).
    pub fn clip_prompt(&self) -> String {
        let motion = self.motion_prompt().as_str();
        match self.shows_product_in_use() {
            true => format!("{PRODUCT_MOTION_RULE} {}", motion.trim_start()),
            false => motion.to_owned(),
        }
    }

    /// Whether the scene has yet to be drawn: it has no image, or it shows
    /// the product in use (#279) and neither its image nor the one waiting
    /// for review was drawn with the product. A composed scene is never
    /// drawn.
    pub fn needs_drawing(&self) -> bool {
        !self.is_composed()
            && (self.image.is_none()
                || (self.shows_product_in_use()
                    && !self.product_drawn()
                    && !self.pending_product_drawn()))
    }

    /// What the scene is: a drawn scene, unless an ad's plan says
    /// otherwise.
    pub fn kind(&self) -> SceneKind {
        self.kind
    }

    /// The product image a composed scene shows.
    pub fn product_image(&self) -> Option<ProductImageId> {
        self.product_image
    }

    /// Whether Bardo composes the scene's picture instead of having it
    /// drawn: a product shot, from a product image, and the end card,
    /// with or without one (#281). Such a picture is free, is never
    /// animated and is never a reference for another scene.
    pub fn is_composed(&self) -> bool {
        match self.kind {
            SceneKind::ProductShot => self.product_image.is_some(),
            SceneKind::EndCard => true,
            SceneKind::Drawn | SceneKind::ProductInUse => false,
        }
    }

    /// The picture composed for it.
    pub fn shot(&self) -> Option<&ComposedShot> {
        self.shot.as_ref()
    }

    /// Whether a composed scene still waits for its picture in `aspect`:
    /// it has none, or the one it has shows another image, was made for
    /// another shape or in another look, or (an end card) shows another
    /// design or none was set yet.
    pub fn needs_shot(&self, aspect: AspectRatio) -> bool {
        self.is_composed()
            && !self.shot.as_ref().is_some_and(|shot| {
                shot.image == self.product_image
                    && shot.aspect == aspect
                    && shot.look == Some(self.look)
                    && (!self.is_end_card() || (self.card.is_some() && shot.card == self.card))
            })
    }

    /// Makes `card` the end card design the scene shows. Returns whether
    /// it changed: its picture is then composed again, for free.
    pub fn set_card(&mut self, card: String) -> bool {
        if self.card.as_deref() == Some(card.as_str()) {
            return false;
        }
        self.card = Some(card);
        true
    }

    /// Makes `shot` the scene's composed picture. Returns the picture
    /// that is no longer used, whose file can go: the one replaced (unless
    /// it is the same file), or `shot` itself when the scene is no longer
    /// composed and takes none.
    pub fn add_shot(&mut self, shot: ComposedShot) -> Option<ComposedShot> {
        if !self.is_composed() {
            return Some(shot);
        }
        self.failure = None;
        self.shot
            .replace(shot)
            .filter(|old| self.shot.as_ref().is_some_and(|new| new.file != old.file))
    }

    /// The still picture the scene shows, by its file in the project
    /// folder: for a composed scene the picture composed from the image
    /// it shows (none while another image's picture waits to be
    /// replaced), the drawn image for every other.
    pub fn picture(&self) -> Option<&str> {
        match self.is_composed() {
            true => self
                .shot
                .as_ref()
                .filter(|shot| shot.image == self.product_image)
                .map(|shot| shot.file.as_str()),
            false => self.image.as_ref().map(|image| image.file.as_str()),
        }
    }

    /// The cast members it shows, as indexes into the plan's cast.
    pub fn cast(&self) -> &[usize] {
        &self.cast
    }

    /// The recurring place it was planned in, as an index into the plan's
    /// places.
    pub fn place(&self) -> Option<usize> {
        self.place
    }

    pub fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }

    pub fn prompt(&self) -> &ScenePrompt {
        &self.prompt
    }

    pub fn generated_prompt(&self) -> &ScenePrompt {
        &self.generated_prompt
    }

    /// Whether the user changed the prompt since it was planned.
    pub fn is_edited(&self) -> bool {
        self.prompt != self.generated_prompt
    }

    pub fn image(&self) -> Option<&SceneImage> {
        self.image.as_ref()
    }

    pub fn pending(&self) -> Option<&SceneImage> {
        self.pending.as_ref()
    }

    pub fn failure(&self) -> Option<JobFailureKind> {
        self.failure
    }

    /// Replaces the prompt with the user's. Returns whether it changed.
    pub fn edit_prompt(&mut self, prompt: ScenePrompt) -> bool {
        if prompt == self.prompt {
            return false;
        }
        self.prompt = prompt;
        true
    }

    /// The portraits the scene's image was drawn with; `None` when that
    /// was not recorded.
    pub fn image_portraits(&self) -> Option<&PortraitsUsed> {
        self.image_portraits.as_ref()
    }

    /// The portraits the image waiting for review was drawn with.
    pub fn pending_portraits(&self) -> Option<&PortraitsUsed> {
        self.pending_portraits.as_ref()
    }

    /// Takes a newly drawn image: it becomes the scene's image when there
    /// is none, else it waits for review beside the current one. Returns
    /// the image it pushed out (an earlier one waiting), whose file can go.
    pub fn add_image(&mut self, image: SceneImage) -> Option<SceneImage> {
        self.add_image_drawn_with(image, None)
    }

    /// `add_image`, recording the `portraits` the image was drawn with.
    pub fn add_image_drawn_with(
        &mut self,
        image: SceneImage,
        portraits: Option<PortraitsUsed>,
    ) -> Option<SceneImage> {
        self.add_drawing(image, portraits, false)
    }

    /// `add_image_drawn_with`, recording whether the image was drawn with
    /// the product's photo as a reference (#279).
    pub fn add_drawing(
        &mut self,
        image: SceneImage,
        portraits: Option<PortraitsUsed>,
        with_product: bool,
    ) -> Option<SceneImage> {
        self.failure = None;
        if self.image.is_none() {
            self.image = Some(image);
            self.image_portraits = portraits;
            self.checks.image_drawn = with_product;
            None
        } else {
            self.pending_portraits = portraits;
            self.checks.pending_drawn = with_product;
            self.pending.replace(image)
        }
    }

    /// Records that drawing it failed. The images it has stay.
    pub fn fail(&mut self, kind: JobFailureKind) {
        self.failure = Some(kind);
    }

    /// Makes the image waiting for review the scene's image. Returns the
    /// replaced one, whose file can go.
    pub fn accept_image(&mut self) -> Result<Option<SceneImage>, NoPendingImage> {
        let pending = self.pending.take().ok_or(NoPendingImage)?;
        self.image_portraits = self.pending_portraits.take();
        // The new image is not the one checked (#279).
        self.checks.image_drawn = std::mem::take(&mut self.checks.pending_drawn);
        self.checks.picture = None;
        Ok(self.image.replace(pending))
    }

    /// Drops the image waiting for review and keeps the current one.
    /// Returns the dropped one, whose file can go.
    pub fn reject_image(&mut self) -> Result<SceneImage, NoPendingImage> {
        let pending = self.pending.take().ok_or(NoPendingImage)?;
        self.pending_portraits = None;
        self.checks.pending_drawn = false;
        Ok(pending)
    }

    /// What moves the clip: the user's motion prompt, else the planned
    /// one, else (plans made before #246) the image prompt.
    pub fn motion_prompt(&self) -> &ScenePrompt {
        self.motion_prompt
            .as_ref()
            .unwrap_or_else(|| self.default_motion())
    }

    /// The motion prompt the scene follows without one of the user's.
    fn default_motion(&self) -> &ScenePrompt {
        self.planned_motion.as_ref().unwrap_or(&self.prompt)
    }

    /// The motion prompt Claude planned, if the plan has one.
    pub fn planned_motion(&self) -> Option<&ScenePrompt> {
        self.planned_motion.as_ref()
    }

    /// The motion prompt the user wrote, if any.
    pub fn own_motion_prompt(&self) -> Option<&ScenePrompt> {
        self.motion_prompt.as_ref()
    }

    /// Sets the user's motion prompt; `None` goes back to the planned one
    /// (or the image prompt). Returns whether it changed.
    pub fn set_motion_prompt(&mut self, prompt: Option<ScenePrompt>) -> bool {
        let prompt = prompt.filter(|prompt| prompt != self.default_motion());
        if prompt == self.motion_prompt {
            return false;
        }
        self.motion_prompt = prompt;
        true
    }

    /// The video model the scene picked over its channel's.
    pub fn clip_model(&self) -> Option<&ClipModelRef> {
        self.clip_model.as_ref()
    }

    /// Picks the scene's video model; `None` follows the channel. Returns
    /// whether it changed.
    pub fn set_clip_model(&mut self, model: Option<ClipModelRef>) -> bool {
        if model == self.clip_model {
            return false;
        }
        self.clip_model = model;
        true
    }

    pub fn clip(&self) -> Option<&SceneClip> {
        self.clip.as_ref()
    }

    pub fn pending_clip(&self) -> Option<&SceneClip> {
        self.pending_clip.as_ref()
    }

    pub fn clip_failure(&self) -> Option<JobFailureKind> {
        self.clip_failure
    }

    /// Whether the clip animates an image the scene no longer shows.
    pub fn is_clip_stale(&self) -> bool {
        self.clip.as_ref().is_some_and(|clip| {
            self.image
                .as_ref()
                .is_none_or(|image| image.generation.id != clip.source_image)
        })
    }

    /// Takes a new clip. Clips cost money and replace the still image in
    /// the cut, so every one waits for review, beside the current clip if
    /// there is one. Returns the clip it pushed out (an earlier one
    /// waiting), whose file can go.
    pub fn add_clip(&mut self, clip: SceneClip) -> Option<SceneClip> {
        self.clip_failure = None;
        self.pending_clip.replace(clip)
    }

    /// Records that making a clip failed. The clips it has stay.
    pub fn fail_clip(&mut self, kind: JobFailureKind) {
        self.clip_failure = Some(kind);
    }

    /// Makes the clip waiting for review the scene's clip. Returns the
    /// replaced one, whose file can go.
    pub fn accept_clip(&mut self) -> Result<Option<SceneClip>, NoPendingClip> {
        let pending = self.pending_clip.take().ok_or(NoPendingClip)?;
        self.checks.clip = None;
        Ok(self.clip.replace(pending))
    }

    /// Drops the clip waiting for review and keeps what the scene had.
    /// Returns the dropped one, whose file can go.
    pub fn reject_clip(&mut self) -> Result<SceneClip, NoPendingClip> {
        self.pending_clip.take().ok_or(NoPendingClip)
    }

    /// Goes back to the still image. Returns the clip it dropped, whose
    /// file can go.
    pub fn remove_clip(&mut self) -> Option<SceneClip> {
        self.checks.clip = None;
        self.clip.take()
    }

    /// Whether a clip can be made: it needs the image to start from, and
    /// a picture Bardo composed is never animated.
    pub fn can_animate(&self) -> bool {
        self.image.is_some() && !self.is_composed()
    }

    /// Every image and clip file the scene holds.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.image
            .iter()
            .chain(&self.pending)
            .map(|image| image.file.as_str())
            .chain(
                self.clip
                    .iter()
                    .chain(&self.pending_clip)
                    .map(|clip| clip.file.as_str()),
            )
            .chain(self.shot.iter().map(|shot| shot.file.as_str()))
    }
}

/// One scene as the planner proposed it: the sentence it starts at (an
/// index into `sentences`), its image prompt, the cast members it shows
/// (indexes into the drafted cast), how its clip moves and the recurring
/// place it happens in (an index into the drafted places).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SceneDraft {
    pub first_sentence: usize,
    pub prompt: String,
    pub cast: Vec<usize>,
    pub motion: String,
    pub place: Option<usize>,
    /// What the scene is, by a [`SceneKind`]'s code; only an ad's plan
    /// reads it.
    pub kind: String,
    /// For a product shot or an end card: which of the product's images
    /// it shows, by its position from 0.
    pub product_image: Option<usize>,
}

/// A whole plan as the planner proposed it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanDraft {
    pub cast: Vec<CastDraft>,
    pub places: Vec<PlaceDraft>,
    pub scenes: Vec<SceneDraft>,
    /// For an ad's plan: its product's images, which decide the kinds
    /// its scenes may be. `None` for every other plan, whose scenes are
    /// all drawn whatever the answer says.
    pub product: Option<ProductPictures>,
}

/// The planner's answer has no usable scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the scene plan has no usable scene")]
pub struct NoScenes;

/// A scene plan that does not apply right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the plan has no scene {0}")]
pub struct NoSuchScene(pub usize);

/// A narration that reads other text than a plan's scenes were written
/// over, so the plan cannot be fitted to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the narration reads other text than the scenes were planned on")]
pub struct OtherText;

/// The pictures a plan holds that were paid for: what a narration that no
/// longer fits the plan puts out of date.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaidPictures {
    /// Scene images, the new ones waiting for review and the cast's
    /// portraits.
    pub images: usize,
    /// Clips, the new ones waiting for review included.
    pub clips: usize,
}

impl PaidPictures {
    pub fn is_empty(&self) -> bool {
        self.images == 0 && self.clips == 0
    }
}

/// The scenes of a video project, planned on its narration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePlan {
    pub id: ScenePlanId,
    pub project: VideoProjectId,
    pub owner: ProfileId,
    /// The narration the scenes are timed on.
    pub narration: NarrationId,
    /// The planner's generation; never changes.
    pub generation: Generation,
    scenes: Vec<Scene>,
    /// The recurring characters the scenes show (#246).
    cast: Vec<CastMember>,
    /// The recurring places the scenes happen in (#258).
    places: Vec<Place>,
    pub updated_at: SystemTime,
}

/// Every stored field of a scene plan, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePlanRecord {
    pub id: ScenePlanId,
    pub project: VideoProjectId,
    pub owner: ProfileId,
    pub narration: NarrationId,
    pub generation: Generation,
    pub scenes: Vec<Scene>,
    pub updated_at: SystemTime,
}

/// A usable scene of a draft: the sentence it starts at and what it shows.
struct PlannedStart {
    first: usize,
    prompt: ScenePrompt,
    cast: Vec<usize>,
    motion: Option<ScenePrompt>,
    place: Option<usize>,
    kind: String,
    product_image: Option<usize>,
}

/// What a scene of an ad's plan is, from what the planner asked for: a
/// kind Bardo does not know, or one the product's images do not allow, is
/// a drawn scene; only the `last` scene is the end card; and a composed
/// scene naming an image the product does not have shows its first.
fn planned_kind(
    kind: &str,
    image: Option<usize>,
    last: bool,
    pictures: &ProductPictures,
) -> (SceneKind, Option<ProductImageId>) {
    let kind = SceneKind::from_code(kind)
        .filter(|kind| pictures.allows(*kind))
        .unwrap_or_default();
    let kind = match kind {
        SceneKind::EndCard if !last && pictures.allows(SceneKind::ProductShot) => {
            SceneKind::ProductShot
        }
        SceneKind::EndCard if !last => SceneKind::Drawn,
        kind => kind,
    };
    let image = match kind {
        SceneKind::ProductShot | SceneKind::EndCard => image
            .and_then(|position| pictures.at(position))
            .or_else(|| pictures.first()),
        SceneKind::Drawn | SceneKind::ProductInUse => None,
    };
    (kind, image)
}

impl ScenePlan {
    /// The plan the planner proposed, made to fit the narration: scenes in
    /// the order of their first sentence, one per sentence at most, the
    /// first starting at the narration's start and each running until the
    /// next starts, so together they cover all of it. Drafts with a
    /// sentence the narration does not have or an unusable prompt are
    /// dropped. Cast members without a name or a usable description, past
    /// `MAX_CAST`, or shown in no scene are dropped too, and each scene
    /// keeps the members it names that are left, each once. Places follow
    /// the same rule, up to `MAX_PLACES`.
    pub fn from_draft(
        generation: Generation,
        narration: &Narration,
        draft: PlanDraft,
        now: SystemTime,
    ) -> Result<Self, NoScenes> {
        let PlanDraft {
            cast,
            places,
            scenes: mut drafts,
            product,
        } = draft;
        let sentences = sentences(narration);
        drafts.sort_by_key(|draft| draft.first_sentence);
        let mut starts: Vec<PlannedStart> = Vec::new();
        for draft in drafts {
            let Ok(prompt) = ScenePrompt::new(&draft.prompt) else {
                continue;
            };
            let taken = starts
                .last()
                .is_some_and(|start| start.first == draft.first_sentence);
            if draft.first_sentence < sentences.len() && !taken {
                starts.push(PlannedStart {
                    first: draft.first_sentence,
                    prompt,
                    cast: draft.cast,
                    motion: ScenePrompt::new(&draft.motion).ok(),
                    place: draft.place,
                    kind: draft.kind,
                    product_image: draft.product_image,
                });
            }
        }
        // The usable members, by their drafted index.
        let mut usable: Vec<(usize, CastMember)> = cast
            .iter()
            .enumerate()
            .filter_map(|(index, draft)| Some((index, CastMember::planned(draft)?)))
            .collect();
        usable.retain(|(index, _)| starts.iter().any(|start| start.cast.contains(index)));
        usable.truncate(MAX_CAST);
        // The usable places, likewise.
        let mut somewhere: Vec<(usize, Place)> = places
            .iter()
            .enumerate()
            .filter_map(|(index, draft)| Some((index, Place::planned(draft)?)))
            .collect();
        somewhere.retain(|(index, _)| starts.iter().any(|start| start.place == Some(*index)));
        // Past the limit, the places with the most scenes stay, and a
        // scene of another keeps its description in its own prompt.
        let mut crowded: Vec<usize> = somewhere.iter().map(|(index, _)| *index).collect();
        crowded.sort_by_key(|index| {
            std::cmp::Reverse(
                starts
                    .iter()
                    .filter(|start| start.place == Some(*index))
                    .count(),
            )
        });
        crowded.truncate(MAX_PLACES);
        let (somewhere, dropped): (Vec<_>, Vec<_>) = somewhere
            .into_iter()
            .partition(|(index, _)| crowded.contains(index));
        for start in &mut starts {
            let described = dropped
                .iter()
                .find(|(index, _)| start.place == Some(*index))
                .and_then(|(_, place)| {
                    ScenePrompt::new(&format!("{} {}", start.prompt.as_str(), place.setting())).ok()
                });
            if let Some(prompt) = described {
                start.prompt = prompt;
            }
        }
        for start in &mut starts {
            let mut kept: Vec<usize> = Vec::new();
            for drafted in &start.cast {
                if let Some(at) = usable.iter().position(|(index, _)| index == drafted)
                    && !kept.contains(&at)
                {
                    kept.push(at);
                }
            }
            start.cast = kept;
            start.place = start
                .place
                .and_then(|drafted| somewhere.iter().position(|(index, _)| *index == drafted));
        }
        // The opening sentences belong to the first scene.
        match starts.first_mut() {
            Some(start) => start.first = 0,
            None => return Err(NoScenes),
        }
        let text = narration.text.as_str();
        let end = narration
            .duration
            .max(sentences.last().map_or(Duration::ZERO, |s| s.end));
        let scenes = starts
            .iter()
            .enumerate()
            .map(|(index, start)| {
                let next = starts.get(index + 1).map(|next| next.first);
                let last = &sentences[next.unwrap_or(sentences.len()) - 1];
                let (kind, product_image) = match &product {
                    Some(pictures) => planned_kind(
                        &start.kind,
                        start.product_image,
                        index + 1 == starts.len(),
                        pictures,
                    ),
                    None => (SceneKind::Drawn, None),
                };
                Scene::planned(
                    if index == 0 {
                        Duration::ZERO
                    } else {
                        sentences[start.first].start
                    },
                    next.map_or(end, |next| sentences[next].start),
                    text[sentences[start.first].text.start..last.text.end].to_owned(),
                    start.prompt.clone(),
                )
                .with_cast(start.cast.clone())
                .with_planned_motion(start.motion.clone())
                .with_place(start.place)
                .with_kind(kind, product_image, None)
            })
            .collect();
        Ok(Self {
            id: ScenePlanId::new(),
            project: generation.project,
            owner: generation.owner,
            narration: narration.id,
            generation,
            scenes,
            cast: usable.into_iter().map(|(_, member)| member).collect(),
            places: somewhere.into_iter().map(|(_, place)| place).collect(),
            updated_at: now,
        })
    }

    /// [`ScenePlan::from_draft`] for a plan without recurring places.
    pub fn from_drafts(
        generation: Generation,
        narration: &Narration,
        cast: Vec<CastDraft>,
        drafts: Vec<SceneDraft>,
        now: SystemTime,
    ) -> Result<Self, NoScenes> {
        Self::from_draft(
            generation,
            narration,
            PlanDraft {
                cast,
                places: Vec::new(),
                scenes: drafts,
                product: None,
            },
            now,
        )
    }

    pub fn restore(record: ScenePlanRecord) -> Self {
        Self {
            id: record.id,
            project: record.project,
            owner: record.owner,
            narration: record.narration,
            generation: record.generation,
            scenes: record.scenes,
            cast: Vec::new(),
            places: Vec::new(),
            updated_at: record.updated_at,
        }
    }

    /// The plan with its cast, as an adapter rebuilds it.
    pub fn with_cast(mut self, cast: Vec<CastMember>) -> Self {
        self.cast = cast;
        self
    }

    /// The plan with its places, as an adapter rebuilds it.
    pub fn with_places(mut self, places: Vec<Place>) -> Self {
        self.places = places;
        self
    }

    /// The recurring places the scenes happen in.
    pub fn places(&self) -> &[Place] {
        &self.places
    }

    pub fn place(&self, index: usize) -> Result<&Place, NoSuchPlace> {
        self.places.get(index).ok_or(NoSuchPlace(index))
    }

    /// The place at `index`, to change; the plan's time moves to `now`.
    pub fn place_mut(&mut self, index: usize, now: SystemTime) -> Result<&mut Place, NoSuchPlace> {
        let place = self.places.get_mut(index).ok_or(NoSuchPlace(index))?;
        self.updated_at = now;
        Ok(place)
    }

    /// The place scene `index` is drawn in, by its index in the places:
    /// the one it was planned in, and once the user edited its prompt,
    /// only while the prompt still names it, so writing a scene somewhere
    /// else takes the place out of its image.
    pub fn scene_place(&self, index: usize) -> Option<usize> {
        let scene = self.scenes.get(index)?;
        // A picture composed from a product image happens nowhere.
        if scene.is_composed() {
            return None;
        }
        let place = self.places.get(scene.place?)?;
        let named = || names(scene.prompt.as_str(), place.called());
        (!scene.is_edited() || named()).then_some(scene.place?)
    }

    /// The other scenes whose images go with scene `index`'s image
    /// request, so the video holds together from scene to scene, with what
    /// each one is to it. Only scenes that have an image count, never the
    /// scene itself:
    ///
    /// - the first other scene of the same place, as the place;
    /// - the scene right before it, when it is in the same place, as what
    ///   it continues from (alone when it is also the first of the place);
    /// - for a scene whose place has no image yet, or with no place: the
    ///   video's first other image, for its look only.
    ///
    /// An image shows a place only when it was drawn from the place as it
    /// is described now: once the user describes a place again, the images
    /// of how it was no longer hold the next ones to it. And only images
    /// drawn in the video's `style` count (#259): one drawn in another
    /// look would pull this scene back to it (see [`ScenePlan::shot_in`]).
    pub fn reference_shots(&self, index: usize, style: StyleChoice) -> Vec<(usize, OtherShot)> {
        let in_style = |other: usize| self.shot_in(other, style);
        let drawn = |other: usize| other != index && in_style(other).is_some();
        if index >= self.scenes.len() {
            return Vec::new();
        }
        let place = self.scene_place(index);
        let setting = place
            .and_then(|place| self.places.get(place))
            .map(Place::setting);
        let same_place = |other: usize| {
            place.is_some()
                && other != index
                && self.scene_place(other) == place
                && in_style(other).is_some_and(|image| {
                    setting
                        .as_deref()
                        .is_some_and(|setting| image.generation.prompt.contains(setting))
                })
        };
        // A composed scene is no shot to continue from: the one before it
        // is.
        let before = (0..index)
            .rev()
            .find(|&before| !self.scenes[before].is_composed())
            .filter(|&before| same_place(before))
            .map(|before| (before, OtherShot::Before));
        let anchor = (0..self.scenes.len())
            .find(|&other| same_place(other))
            .filter(|&anchor| before.is_none_or(|(before, _)| before != anchor))
            .map(|anchor| (anchor, OtherShot::SamePlace));
        if anchor.is_some() || before.is_some() {
            return anchor.into_iter().chain(before).collect();
        }
        (0..self.scenes.len())
            .find(|&other| drawn(other))
            .map(|look| (look, OtherShot::Look))
            .into_iter()
            .collect()
    }

    /// Scene `index`'s image as other scenes drawn in `style` see it: the
    /// image it uses when that was drawn in the style, else the one that
    /// waits for review when that was. A video drawn again in a new style
    /// has every new image waiting beside one of the old look, and those
    /// new images are what hold it together from scene to scene until the
    /// user accepts them. The same goes for an image that shows a person
    /// as an older portrait did, or without one (#269): beside the portrait
    /// it would pull the next image back to the face being replaced.
    pub fn shot_in(&self, index: usize, style: StyleChoice) -> Option<&SceneImage> {
        // A composed scene shows a product image, never a drawing: whatever
        // drawn image it still holds is no reference for another scene.
        let scene = self
            .scenes
            .get(index)
            .filter(|scene| !scene.is_composed())?;
        let fits = |image: &SceneImage, portraits: Option<&PortraitsUsed>| {
            style.drew(&image.generation.prompt)
                && !self.shows_old_portrait(index, image, portraits)
        };
        scene
            .image
            .as_ref()
            .filter(|image| fits(image, scene.image_portraits.as_ref()))
            .or_else(|| {
                scene
                    .pending
                    .as_ref()
                    .filter(|image| fits(image, scene.pending_portraits.as_ref()))
            })
    }

    /// The recurring characters the scenes show.
    pub fn cast(&self) -> &[CastMember] {
        &self.cast
    }

    pub fn cast_member(&self, index: usize) -> Result<&CastMember, NoSuchCastMember> {
        self.cast.get(index).ok_or(NoSuchCastMember(index))
    }

    /// The cast member at `index`, to change; the plan's time moves to
    /// `now`.
    pub fn cast_mut(
        &mut self,
        index: usize,
        now: SystemTime,
    ) -> Result<&mut CastMember, NoSuchCastMember> {
        let member = self.cast.get_mut(index).ok_or(NoSuchCastMember(index))?;
        self.updated_at = now;
        Ok(member)
    }

    /// The cast members scene `index` is drawn with, by their index in
    /// the cast: the ones it shows, and once the user edited its prompt,
    /// only those the prompt still names, so writing a person out of a
    /// scene takes them out of its image.
    pub fn scene_cast(&self, index: usize) -> Vec<usize> {
        let drawn = self.scenes.get(index).filter(|scene| !scene.is_composed());
        drawn.map_or_else(Vec::new, |scene| {
            let prompt = scene.prompt.as_str().to_lowercase();
            scene
                .cast
                .iter()
                .copied()
                .filter(|&member| {
                    self.cast.get(member).is_some_and(|cast| {
                        !scene.is_edited() || prompt.contains(&cast.called().to_lowercase())
                    })
                })
                .collect()
        })
    }

    /// Cast members without a portrait in the video's `style` that drawing
    /// `scenes` would draw first, each once: the ones with none, and the
    /// ones whose portrait was drawn in another style. One the user chose
    /// to draw from the description alone gets none.
    pub fn missing_portraits(&self, scenes: &[usize], style: StyleChoice) -> Vec<usize> {
        let mut missing: Vec<usize> = Vec::new();
        for &scene in scenes {
            for member in self.scene_cast(scene) {
                let cast = &self.cast[member];
                if !cast.described_only
                    && cast.portrait_in(style).is_none()
                    && !missing.contains(&member)
                {
                    missing.push(member);
                }
            }
        }
        missing
    }

    /// Takes the "no portrait" failure off the scenes that no longer wait
    /// for one in `style`: everyone they show has a portrait now, or is
    /// drawn from the description by the user's choice. Returns the scenes
    /// it changed, to save. A scene that has an image goes back to it, and
    /// one without is to draw.
    pub fn settle_portrait_waits(&mut self, style: StyleChoice) -> Vec<usize> {
        let settled: Vec<usize> = (0..self.scenes.len())
            .filter(|&index| {
                self.scenes[index].failure == Some(JobFailureKind::PortraitMissing)
                    && self.missing_portraits(&[index], style).is_empty()
            })
            .collect();
        for &index in &settled {
            self.scenes[index].failure = None;
        }
        settled
    }

    /// Whether `image` of scene `index`, drawn with `portraits`, shows
    /// someone other than as their portrait shows them now: it was drawn
    /// with an older portrait of theirs, or without one. A person drawn
    /// from the description alone by the user's choice has no portrait to
    /// match. A person the record does not list was not in the image when
    /// it was drawn (the prompt had them written out), so it does not show
    /// their portrait either. For an image from before portraits were
    /// recorded, a portrait drawn after it tells, and so does having none
    /// now: the image cannot show the one that will be drawn.
    fn shows_old_portrait(
        &self,
        index: usize,
        image: &SceneImage,
        portraits: Option<&PortraitsUsed>,
    ) -> bool {
        self.scene_cast(index).into_iter().any(|member| {
            let cast = &self.cast[member];
            if cast.described_only {
                return false;
            }
            let current = cast.portrait.as_ref();
            match portraits {
                Some(portraits) => portraits
                    .iter()
                    .find(|(drawn, _)| *drawn == member)
                    .is_none_or(|(_, used)| {
                        used.is_none() || *used != current.map(|portrait| portrait.generation.id)
                    }),
                None => current.is_none_or(|portrait| {
                    portrait.generation.generated_at > image.generation.generated_at
                }),
            }
        })
    }

    /// The scenes whose image shows a person as an older portrait of
    /// theirs did, or was drawn without their portrait (#269): they stay
    /// as they are until the user draws them again. One with a new image
    /// waiting for review that matches the portraits is not counted: it
    /// was drawn again already.
    pub fn with_old_portraits(&self) -> Vec<usize> {
        self.scenes
            .iter()
            .enumerate()
            .filter(|(index, scene)| {
                let old = |image: &SceneImage, portraits: Option<&PortraitsUsed>| {
                    self.shows_old_portrait(*index, image, portraits)
                };
                // A composed scene does not show the drawing it holds.
                !scene.is_composed()
                    && scene
                        .image
                        .as_ref()
                        .is_some_and(|image| old(image, scene.image_portraits.as_ref()))
                    && !scene
                        .pending
                        .as_ref()
                        .is_some_and(|image| !old(image, scene.pending_portraits.as_ref()))
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// The scenes whose image was drawn in another style than `style`,
    /// the video's: they stay as they are until the user draws them again.
    /// One with a new image in `style` waiting for review is not counted:
    /// it was drawn again already.
    pub fn in_another_style(&self, style: StyleChoice) -> Vec<usize> {
        let drew = |image: &SceneImage| style.drew(&image.generation.prompt);
        self.scenes
            .iter()
            .enumerate()
            .filter(|(_, scene)| {
                // A composed scene does not show the drawing it holds.
                !scene.is_composed()
                    && scene.image.as_ref().is_some_and(|image| !drew(image))
                    && !scene.pending.as_ref().is_some_and(drew)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// The scenes drawn with cast member `member`.
    pub fn scenes_with(&self, member: usize) -> Vec<usize> {
        (0..self.scenes.len())
            .filter(|&index| self.scene_cast(index).contains(&member))
            .collect()
    }

    pub fn scenes(&self) -> &[Scene] {
        &self.scenes
    }

    pub fn scene(&self, index: usize) -> Result<&Scene, NoSuchScene> {
        self.scenes.get(index).ok_or(NoSuchScene(index))
    }

    /// The scene at `index`, to change; the plan's time moves to `now`.
    pub fn scene_mut(&mut self, index: usize, now: SystemTime) -> Result<&mut Scene, NoSuchScene> {
        let scene = self.scenes.get_mut(index).ok_or(NoSuchScene(index))?;
        self.updated_at = now;
        Ok(scene)
    }

    /// Whether the narration was generated again after this plan, so the
    /// scenes' times no longer match what is heard.
    pub fn is_stale(&self, narration: Option<&Narration>) -> bool {
        narration.is_none_or(|narration| narration.id != self.narration)
    }

    /// Where each scene's words start in `text` (byte offsets), when the
    /// scenes' words, in order, are all of `text` (nothing before, between
    /// or after them but white space) and every scene with words starts a
    /// sentence of `text`, as planned scenes do. `None` when `text` reads
    /// anything else, or breaks into sentences elsewhere.
    fn scene_offsets(&self, text: &str) -> Option<Vec<usize>> {
        let words = spoken_words(text);
        let sentences: Vec<usize> = sentence_spans(text, &words)
            .iter()
            .map(|span| words[span.start].start)
            .collect();
        let mut offsets = Vec::with_capacity(self.scenes.len());
        let mut cursor = 0;
        for scene in &self.scenes {
            let rest = &text[cursor..];
            let at = cursor + (rest.len() - rest.trim_start().len());
            if !text[at..].starts_with(scene.text.as_str()) {
                return None;
            }
            if !scene.text.is_empty() && sentences.binary_search(&at).is_err() {
                return None;
            }
            offsets.push(at);
            cursor = at + scene.text.len();
        }
        text[cursor..].trim().is_empty().then_some(offsets)
    }

    /// Whether `text` is what the scenes were written over: a narration
    /// that reads it can take this plan with [`ScenePlan::fitted_to`].
    pub fn reads(&self, text: &str) -> bool {
        self.scene_offsets(text).is_some()
    }

    /// The plan fitted to `narration`, a new reading of the text it was
    /// made for: every scene keeps its words, prompts, people, place,
    /// images and clips, and only when it starts and ends is read again
    /// from the new word timings, the way [`ScenePlan::from_draft`] times
    /// a new plan (the first scene from the start, each until the next
    /// starts, the last to the narration's end). It is the same plan, by
    /// its id, on the new narration. A narration that reads other text
    /// does not fit: those scenes are planned again.
    pub fn fitted_to(&self, narration: &Narration, now: SystemTime) -> Result<Self, OtherText> {
        if narration.id == self.narration {
            return Ok(self.clone());
        }
        let offsets = self
            .scene_offsets(narration.text.as_str())
            .ok_or(OtherText)?;
        let words = narration.words.as_slice();
        let end = narration
            .duration
            .max(words.last().map_or(Duration::ZERO, |word| word.end));
        let mut starts: Vec<Duration> = Vec::with_capacity(offsets.len());
        for (index, (scene, offset)) in self.scenes.iter().zip(&offsets).enumerate() {
            let word = words.get(words.partition_point(|word| word.text.start < *offset));
            let start = match word {
                _ if index == 0 => Duration::ZERO,
                // A scene with words starts at a word; one without (a
                // version's scene whose sentences were all dropped) where
                // the next words are said.
                Some(word) if scene.text.is_empty() || word.text.start == *offset => {
                    word.start.min(end)
                }
                None if scene.text.is_empty() => end,
                _ => return Err(OtherText),
            };
            starts.push(start.max(starts.last().copied().unwrap_or(Duration::ZERO)));
        }
        let scenes = self
            .scenes
            .iter()
            .enumerate()
            .map(|(index, scene)| Scene {
                start: starts[index],
                end: starts
                    .get(index + 1)
                    .copied()
                    .unwrap_or(end)
                    .max(starts[index]),
                ..scene.clone()
            })
            .collect();
        Ok(Self {
            narration: narration.id,
            scenes,
            updated_at: now,
            ..self.clone()
        })
    }

    /// The images and clips the plan holds.
    pub fn paid_pictures(&self) -> PaidPictures {
        let portraits = self
            .cast
            .iter()
            .filter(|member| member.portrait.is_some())
            .count();
        let count = |held: fn(&Scene) -> usize| self.scenes.iter().map(held).sum::<usize>();
        PaidPictures {
            images: portraits
                + count(|scene| {
                    usize::from(scene.image.is_some()) + usize::from(scene.pending.is_some())
                }),
            clips: count(|scene| {
                usize::from(scene.clip.is_some()) + usize::from(scene.pending_clip.is_some())
            }),
        }
    }

    /// Scenes without an image, which a new image job draws. A composed
    /// scene is never drawn.
    pub fn missing_images(&self) -> Vec<usize> {
        (0..self.scenes.len())
            .filter(|&index| self.scenes[index].needs_drawing())
            .collect()
    }

    /// The end card (#281): the last scene, when it is of that kind.
    pub fn end_card(&self) -> Option<usize> {
        let last = self.scenes.len().checked_sub(1)?;
        self.scenes[last].is_end_card().then_some(last)
    }

    /// Makes `card` the design the plan's end card shows. Returns the
    /// card's index when that changed: its picture is then out of date
    /// and is composed again, for free.
    pub fn set_end_card(&mut self, card: String, now: SystemTime) -> Option<usize> {
        let index = self.end_card()?;
        if !self.scenes[index].set_card(card) {
            return None;
        }
        self.updated_at = now;
        Some(index)
    }

    /// Composed scenes that wait for their picture in `aspect`, which
    /// Bardo makes for free.
    pub fn missing_shots(&self, aspect: AspectRatio) -> Vec<usize> {
        (0..self.scenes.len())
            .filter(|&index| self.scenes[index].needs_shot(aspect))
            .collect()
    }

    /// Makes scene `index` of an ad's plan a scene of `kind`, within what
    /// the product's `pictures` allow; a composed scene shows `image`,
    /// else the image it showed before, else the product's first. The
    /// drawn image and the clip of a scene that becomes composed are kept
    /// unused, so going back costs nothing.
    pub fn set_scene_kind(
        &mut self,
        index: usize,
        kind: SceneKind,
        image: Option<ProductImageId>,
        pictures: &ProductPictures,
        now: SystemTime,
    ) -> Result<KindChange, SceneKindError> {
        let last = index + 1 == self.scenes.len();
        let scene = self
            .scenes
            .get_mut(index)
            .ok_or(SceneKindError::NoSuchScene(index))?;
        if !pictures.allows(kind) {
            return Err(SceneKindError::NotAllowed(kind));
        }
        if kind == SceneKind::EndCard && !last {
            return Err(SceneKindError::NotLast);
        }
        let image = match kind {
            SceneKind::ProductShot | SceneKind::EndCard => match image {
                Some(image) if !pictures.has(image) => return Err(SceneKindError::NoSuchImage),
                Some(image) => Some(image),
                None => scene
                    .product_image
                    .filter(|image| pictures.has(*image))
                    .or_else(|| pictures.first()),
            },
            SceneKind::Drawn | SceneKind::ProductInUse => None,
        };
        if scene.kind == kind && scene.product_image == image {
            return Ok(KindChange::Same);
        }
        scene.kind = kind;
        scene.product_image = image;
        // A failure was about making the picture it showed before: a
        // drawing that failed, or a picture that could not be composed.
        scene.failure = None;
        self.updated_at = now;
        Ok(match scene.is_composed() {
            true => KindChange::Compose,
            false => KindChange::Drawn {
                // The product in use needs an image drawn with the
                // product (#279).
                needs_image: scene.needs_drawing(),
                dropped: scene.shot.take(),
            },
        })
    }

    /// Makes the product shot of scene `index` look as `look` says.
    /// Returns whether it changed: a changed look is composed again, for
    /// free. Only a composed scene has a look to change.
    pub fn set_shot_look(
        &mut self,
        index: usize,
        look: ShotLook,
        now: SystemTime,
    ) -> Result<bool, SceneKindError> {
        let scene = self
            .scenes
            .get_mut(index)
            .ok_or(SceneKindError::NoSuchScene(index))?;
        if !scene.is_composed() {
            return Err(SceneKindError::NotComposed);
        }
        if scene.look == look {
            return Ok(false);
        }
        scene.look = look;
        scene.failure = None;
        self.updated_at = now;
        Ok(true)
    }

    /// Scenes of the product in use whose picture waits for the user's
    /// check, or for a drawing with the product (#279): the render waits
    /// for them.
    pub fn unchecked_products(&self) -> Vec<usize> {
        (0..self.scenes.len())
            .filter(|&index| !self.scenes[index].product_checked())
            .collect()
    }

    /// Scenes a clip can be made for that have none, accepted or waiting:
    /// what a new clip run animates.
    pub fn missing_clips(&self) -> Vec<usize> {
        (0..self.scenes.len())
            .filter(|&index| {
                let scene = &self.scenes[index];
                scene.can_animate()
                    && scene.product_checked()
                    && scene.clip.is_none()
                    && scene.pending_clip.is_none()
            })
            .collect()
    }

    /// Every image and clip file of every scene, and every portrait.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.scenes.iter().flat_map(Scene::files).chain(
            self.cast
                .iter()
                .filter_map(|member| member.portrait.as_ref())
                .map(|portrait| portrait.file.as_str()),
        )
    }
}

/// Persistence port for scene plans. Shared with job worker threads.
pub trait ScenePlanRepository: Send + Sync {
    /// The project's current plan.
    fn scene_plan(&self, project: VideoProjectId) -> Result<Option<ScenePlan>, RepositoryError>;

    /// Makes `plan` the project's plan with all its scenes, replacing any
    /// other, and saves the generations it refers to; all or none.
    fn save_scene_plan(&self, plan: &ScenePlan) -> Result<(), RepositoryError>;

    /// Saves `plan` fitted to a new narration ([`ScenePlan::fitted_to`]):
    /// the project's plan with its scenes at their new times. The
    /// project's saved cut goes with the old times, since it was laid out
    /// on them: the editor builds the rough cut again. All or none.
    fn save_fitted_plan(&self, plan: &ScenePlan) -> Result<(), RepositoryError>;

    /// Saves scene `index` of the saved plan `plan` (prompts, model,
    /// images, clips, failures) and the generations of its images and
    /// clips, leaving the other scenes as they are. Fails when the plan is
    /// no longer saved.
    fn save_scene(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError>;

    /// Saves cast member `index` of the saved plan `plan` (description,
    /// portrait, failure) and its portrait's generation, leaving the rest
    /// as it is. Fails when the plan is no longer saved.
    fn save_cast_member(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError>;

    /// Saves place `index` of the saved plan `plan` (its description),
    /// leaving the rest as it is. Fails when the plan is no longer saved.
    fn save_place(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError>;

    /// The backdrops drawn for the project's product shots (#285), newest
    /// first.
    fn shot_backdrops(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<ShotBackdropDrawing>, RepositoryError>;

    fn add_shot_backdrop(&self, backdrop: &ShotBackdropDrawing) -> Result<(), RepositoryError>;

    /// Marks the backdrop kept: the user looked at it and wants it.
    fn keep_shot_backdrop(&self, backdrop: BackdropId) -> Result<(), RepositoryError>;

    fn remove_shot_backdrop(&self, backdrop: BackdropId) -> Result<(), RepositoryError>;
}

impl<T: ScenePlanRepository + ?Sized> ScenePlanRepository for Arc<T> {
    fn scene_plan(&self, project: VideoProjectId) -> Result<Option<ScenePlan>, RepositoryError> {
        (**self).scene_plan(project)
    }

    fn save_scene_plan(&self, plan: &ScenePlan) -> Result<(), RepositoryError> {
        (**self).save_scene_plan(plan)
    }

    fn save_fitted_plan(&self, plan: &ScenePlan) -> Result<(), RepositoryError> {
        (**self).save_fitted_plan(plan)
    }

    fn save_scene(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError> {
        (**self).save_scene(plan, index)
    }

    fn save_cast_member(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError> {
        (**self).save_cast_member(plan, index)
    }

    fn save_place(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError> {
        (**self).save_place(plan, index)
    }

    fn shot_backdrops(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<ShotBackdropDrawing>, RepositoryError> {
        (**self).shot_backdrops(project)
    }

    fn add_shot_backdrop(&self, backdrop: &ShotBackdropDrawing) -> Result<(), RepositoryError> {
        (**self).add_shot_backdrop(backdrop)
    }

    fn keep_shot_backdrop(&self, backdrop: BackdropId) -> Result<(), RepositoryError> {
        (**self).keep_shot_backdrop(backdrop)
    }

    fn remove_shot_backdrop(&self, backdrop: BackdropId) -> Result<(), RepositoryError> {
        (**self).remove_shot_backdrop(backdrop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Alignment, CharTiming, GenerationId, Provider, ScriptText, TemplateUsed, TemplateVersionId,
        TokenUsage, WordTimings,
    };

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A narration of `text` with every character 100 ms long, lasting
    /// `tail` ms past its last character.
    fn narration(text: &str, tail: u64) -> Narration {
        let alignment = Alignment {
            chars: text
                .chars()
                .enumerate()
                .map(|(n, c)| CharTiming {
                    text: c.to_string(),
                    start: ms(n as u64 * 100),
                    end: ms(n as u64 * 100 + 100),
                })
                .collect(),
        };
        Narration {
            id: NarrationId::new(),
            project: VideoProjectId::new(),
            owner: ProfileId::new(),
            text: ScriptText::new(text).unwrap(),
            // Scenes follow the words, whoever spoke them.
            source: crate::NarrationSource::Imported {
                file_name: "take 3.wav".into(),
                aligner: Provider::ElevenLabs,
                model: "forced_alignment".into(),
            },
            audio_file: "narration.wav".into(),
            duration: alignment.end() + ms(tail),
            words: WordTimings::from_alignment(text, &alignment),
            generated_at: SystemTime::UNIX_EPOCH,
            job: None,
        }
    }

    fn generation(narration: &Narration, provider: Provider, output: &str) -> Generation {
        Generation {
            id: GenerationId::new(),
            owner: narration.owner,
            project: narration.project,
            provider,
            model: "model".into(),
            template: TemplateUsed {
                id: TemplateVersionId::new(),
                number: 1,
            },
            instructions: String::new(),
            prompt: "Plan.".into(),
            output: output.into(),
            usage: TokenUsage::default(),
            generated_at: SystemTime::UNIX_EPOCH,
            job: None,
        }
    }

    fn draft(first_sentence: usize, prompt: &str) -> SceneDraft {
        SceneDraft {
            first_sentence,
            prompt: prompt.into(),
            ..SceneDraft::default()
        }
    }

    fn plan(narration: &Narration, drafts: Vec<SceneDraft>) -> ScenePlan {
        ScenePlan::from_drafts(
            generation(narration, Provider::Claude, "{}"),
            narration,
            Vec::new(),
            drafts,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap()
    }

    fn image(narration: &Narration, file: &str) -> SceneImage {
        SceneImage {
            file: file.into(),
            generation: generation(narration, Provider::Gemini, file),
        }
    }

    const STORY: &str = "Hi there. Who are you?\nI am \"the probe.\" It ends";

    #[test]
    fn sentences_end_at_marks_and_line_breaks() {
        let n = narration(STORY, 0);
        let texts: Vec<_> = sentences(&n)
            .iter()
            .map(|s| &STORY[s.text.clone()])
            .collect();
        assert_eq!(
            texts,
            [
                "Hi there.",
                "Who are you?",
                "I am \"the probe.\"",
                "It ends"
            ]
        );
        let first = &sentences(&n)[0];
        assert_eq!(first.words, 0..2);
        assert_eq!(
            (first.start, first.end),
            (ms(0), ms(800)),
            "'Hi' to 'there'"
        );
        let second = &sentences(&n)[1];
        assert_eq!(second.start, ms(1000));
    }

    #[test]
    fn a_run_on_sentence_is_cut_so_scenes_can_change_inside_it() {
        let text = vec!["word"; MAX_SENTENCE_WORDS + 5].join(" ");
        let n = narration(&text, 0);
        let lengths: Vec<_> = sentences(&n).iter().map(|s| s.words.len()).collect();
        assert_eq!(lengths, [MAX_SENTENCE_WORDS, 5]);
    }

    #[test]
    fn scenes_cover_the_whole_narration_back_to_back() {
        let n = narration(STORY, 500);
        let sentences = sentences(&n);
        let plan = plan(&n, vec![draft(2, "A probe."), draft(0, "A greeting.")]);
        let scenes = plan.scenes();
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].start, Duration::ZERO);
        assert_eq!(scenes[0].end, sentences[2].start);
        assert_eq!(scenes[1].start, sentences[2].start);
        assert_eq!(scenes[1].end, n.duration, "to the end, silence included");
        assert_eq!(scenes[0].text, "Hi there. Who are you?");
        assert_eq!(scenes[1].text, "I am \"the probe.\" It ends");
        assert_eq!(scenes[0].prompt().as_str(), "A greeting.");
        assert!(!scenes[0].is_edited());
        assert_eq!(plan.narration, n.id);
        assert_eq!(plan.project, n.project);
        assert_eq!(plan.missing_images(), [0, 1]);
    }

    #[test]
    fn the_first_scene_starts_at_the_beginning_whatever_the_planner_said() {
        let n = narration(STORY, 0);
        let plan = plan(&n, vec![draft(1, "Who."), draft(3, "End.")]);
        assert_eq!(plan.scenes()[0].start, Duration::ZERO);
        assert_eq!(
            plan.scenes()[0].text,
            "Hi there. Who are you?\nI am \"the probe.\""
        );
    }

    #[test]
    fn unusable_drafts_are_dropped_and_none_left_is_an_error() {
        let n = narration(STORY, 0);
        let plan = plan(
            &n,
            vec![
                draft(0, "Opening."),
                draft(0, "Same sentence again."),
                draft(2, "  "),
                draft(9, "No such sentence."),
                draft(3, "End."),
            ],
        );
        let prompts: Vec<_> = plan.scenes().iter().map(|s| s.prompt().as_str()).collect();
        assert_eq!(prompts, ["Opening.", "End."]);

        let none = ScenePlan::from_drafts(
            generation(&n, Provider::Claude, "{}"),
            &n,
            Vec::new(),
            vec![draft(7, "Out of range."), draft(1, "")],
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(none, Err(NoScenes));
    }

    #[test]
    fn prompts_are_trimmed_required_and_limited() {
        assert_eq!(ScenePrompt::new("  A ship. ").unwrap().as_str(), "A ship.");
        assert_eq!(ScenePrompt::new(" "), Err(SceneFieldError::PromptRequired));
        let longest = "é".repeat(ScenePrompt::MAX_CHARS);
        assert!(ScenePrompt::new(&longest).is_ok());
        assert_eq!(
            ScenePrompt::new(&format!("{longest}é")),
            Err(SceneFieldError::PromptTooLong)
        );
    }

    #[test]
    fn editing_a_prompt_keeps_the_planned_one() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "Planned.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert!(!scene.edit_prompt(ScenePrompt::new("Planned.").unwrap()));
        assert!(scene.edit_prompt(ScenePrompt::new("Mine.").unwrap()));
        assert_eq!(scene.prompt().as_str(), "Mine.");
        assert_eq!(scene.generated_prompt().as_str(), "Planned.");
        assert!(scene.is_edited());
        assert_eq!(plan.scene(3), Err(NoSuchScene(3)));
    }

    #[test]
    fn a_new_image_waits_for_review_beside_the_current_one() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A."), draft(2, "B.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        scene.fail(JobFailureKind::Declined);
        assert_eq!(scene.add_image(image(&n, "one.png")), None);
        assert_eq!(scene.failure(), None, "a success clears the failure");
        assert_eq!(scene.image().unwrap().file, "one.png");

        assert_eq!(scene.add_image(image(&n, "two.png")), None);
        assert_eq!(
            scene.image().unwrap().file,
            "one.png",
            "kept until accepted"
        );
        assert_eq!(scene.pending().unwrap().file, "two.png");
        let pushed = scene.add_image(image(&n, "three.png")).unwrap();
        assert_eq!(
            pushed.file, "two.png",
            "a newer one replaces the one waiting"
        );

        let replaced = scene.accept_image().unwrap().unwrap();
        assert_eq!(replaced.file, "one.png");
        assert_eq!(scene.image().unwrap().file, "three.png");
        assert_eq!(scene.pending(), None);
        assert_eq!(scene.accept_image(), Err(NoPendingImage));

        scene.add_image(image(&n, "four.png"));
        assert_eq!(scene.reject_image().unwrap().file, "four.png");
        assert_eq!(scene.image().unwrap().file, "three.png");
        assert_eq!(scene.reject_image(), Err(NoPendingImage));
        assert_eq!(plan.missing_images(), [1], "the other scene is untouched");
        assert_eq!(plan.files().collect::<Vec<_>>(), ["three.png"]);
    }

    fn clip(narration: &Narration, file: &str, source: &SceneImage) -> SceneClip {
        SceneClip {
            file: file.into(),
            seconds: 5,
            stretch: None,
            source_image: source.generation.id,
            generation: generation(narration, Provider::Higgsfield, file),
        }
    }

    #[test]
    fn every_new_clip_waits_for_review_and_the_still_can_come_back() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A."), draft(2, "B.")]);
        assert_eq!(plan.missing_clips(), Vec::<usize>::new(), "no image yet");
        let still = image(&n, "still.png");
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert!(!scene.can_animate());
        scene.add_image(still.clone());
        assert_eq!(plan.missing_clips(), [0]);

        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        scene.fail_clip(JobFailureKind::Declined);
        assert_eq!(scene.add_clip(clip(&n, "one.mp4", &still)), None);
        assert_eq!(scene.clip_failure(), None, "a success clears the failure");
        assert_eq!(scene.clip(), None, "even the first clip is reviewed");
        assert_eq!(scene.pending_clip().unwrap().file, "one.mp4");
        assert_eq!(plan.missing_clips(), Vec::<usize>::new(), "one is waiting");

        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        let pushed = scene.add_clip(clip(&n, "two.mp4", &still)).unwrap();
        assert_eq!(
            pushed.file, "one.mp4",
            "a newer one replaces the one waiting"
        );
        assert_eq!(scene.accept_clip().unwrap(), None);
        assert_eq!(scene.clip().unwrap().file, "two.mp4");
        assert_eq!(scene.accept_clip(), Err(NoPendingClip));

        scene.add_clip(clip(&n, "three.mp4", &still));
        assert_eq!(scene.reject_clip().unwrap().file, "three.mp4");
        assert_eq!(scene.clip().unwrap().file, "two.mp4", "kept");
        assert_eq!(scene.reject_clip(), Err(NoPendingClip));
        assert_eq!(
            plan.files().collect::<Vec<_>>(),
            ["still.png", "two.mp4"],
            "images and clips"
        );

        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(scene.remove_clip().unwrap().file, "two.mp4");
        assert_eq!(scene.clip(), None, "back to the still image");
        assert_eq!(scene.image().unwrap().file, "still.png");
    }

    #[test]
    fn a_clip_goes_stale_when_the_scene_takes_another_image() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        let first = image(&n, "first.png");
        scene.add_image(first.clone());
        scene.add_clip(clip(&n, "clip.mp4", &first));
        scene.accept_clip().unwrap();
        assert!(!scene.is_clip_stale());
        scene.add_image(image(&n, "second.png"));
        assert!(!scene.is_clip_stale(), "the new image is only waiting");
        scene.accept_image().unwrap();
        assert!(scene.is_clip_stale());
    }

    #[test]
    fn the_motion_prompt_follows_the_image_prompt_until_the_user_writes_one() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A ship at dawn.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(scene.motion_prompt().as_str(), "A ship at dawn.");
        assert_eq!(scene.own_motion_prompt(), None);
        let slow = ScenePrompt::new("Slow push in, waves rolling.").unwrap();
        assert!(scene.set_motion_prompt(Some(slow.clone())));
        assert!(!scene.set_motion_prompt(Some(slow)));
        assert_eq!(
            scene.motion_prompt().as_str(),
            "Slow push in, waves rolling."
        );
        scene.edit_prompt(ScenePrompt::new("A ship at dusk.").unwrap());
        assert_eq!(
            scene.motion_prompt().as_str(),
            "Slow push in, waves rolling.",
            "the user's motion prompt stays"
        );
        assert!(scene.set_motion_prompt(None));
        assert_eq!(scene.motion_prompt().as_str(), "A ship at dusk.");
        let same = ScenePrompt::new("A ship at dusk.").unwrap();
        assert!(
            !scene.set_motion_prompt(Some(same)),
            "same as the image prompt"
        );
        assert_eq!(scene.own_motion_prompt(), None);
    }

    #[test]
    fn a_scene_can_pick_its_own_video_model() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(scene.clip_model(), None, "follows the channel");
        let model = ClipModelRef::new(Provider::Higgsfield, "kling").unwrap();
        assert!(scene.set_clip_model(Some(model.clone())));
        assert!(!scene.set_clip_model(Some(model.clone())));
        assert_eq!(scene.clip_model(), Some(&model));
        assert!(scene.set_clip_model(None));
    }

    fn member(name: &str, description: &str) -> CastDraft {
        CastDraft {
            name: name.into(),
            description: description.into(),
        }
    }

    fn cast_draft(first_sentence: usize, prompt: &str, cast: Vec<usize>) -> SceneDraft {
        SceneDraft {
            first_sentence,
            prompt: prompt.into(),
            cast,
            motion: "Slow push in.".into(),
            place: None,
            kind: String::new(),
            product_image: None,
        }
    }

    #[test]
    fn the_cast_is_planned_once_and_scenes_point_at_it() {
        let n = narration(STORY, 0);
        let plan = ScenePlan::from_drafts(
            generation(&n, Provider::Claude, "{}"),
            &n,
            vec![
                member("the chef", "A man in his fifties, brown skin, white apron."),
                member("  ", "No name."),
                member("the critic", "  "),
                member("the waiter", "Never shown."),
                member("the sous-chef", "A woman in her twenties, red hair."),
            ],
            vec![
                cast_draft(0, "The chef tastes a sauce.", vec![0, 0, 1, 9]),
                cast_draft(2, "The chef and the sous-chef plate.", vec![4, 0, 2]),
                cast_draft(3, "An empty kitchen.", Vec::new()),
            ],
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let names: Vec<_> = plan.cast().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["the chef", "the sous-chef"],
            "unusable and unshown members are dropped"
        );
        assert_eq!(plan.scenes()[0].cast(), [0], "each once, dropped ones gone");
        assert_eq!(plan.scenes()[1].cast(), [1, 0], "in the scene's order");
        assert!(plan.scenes()[2].cast().is_empty());
        assert_eq!(plan.scenes_with(0), [0, 1]);
        assert_eq!(
            plan.missing_portraits(&[2, 1, 0], StyleChoice::FromNotes),
            [1, 0]
        );
        assert_eq!(
            plan.scenes()[0].planned_motion().map(ScenePrompt::as_str),
            Some("Slow push in.")
        );
    }

    #[test]
    fn a_long_cast_is_cut_and_names_are_limited() {
        let n = narration(STORY, 0);
        let cast: Vec<_> = (0..MAX_CAST + 2)
            .map(|i| member(&format!("person {i}"), "A person."))
            .collect();
        let all: Vec<usize> = (0..MAX_CAST + 2).collect();
        let plan = ScenePlan::from_drafts(
            generation(&n, Provider::Claude, "{}"),
            &n,
            cast,
            vec![cast_draft(0, "A crowd.", all)],
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(plan.cast().len(), MAX_CAST);
        assert_eq!(plan.scenes()[0].cast().len(), MAX_CAST);
        assert_eq!(plan.scene_cast(0).len(), MAX_CAST);
        let long = "n".repeat(MAX_CAST_NAME_CHARS + 10);
        let plan = ScenePlan::from_drafts(
            generation(&n, Provider::Claude, "{}"),
            &n,
            vec![member(&long, "A person.")],
            vec![cast_draft(0, "A person.", vec![0])],
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(plan.cast()[0].name.chars().count(), MAX_CAST_NAME_CHARS);
    }

    #[test]
    fn a_portrait_is_kept_until_the_description_changes() {
        let n = narration(STORY, 0);
        let mut plan = ScenePlan::from_drafts(
            generation(&n, Provider::Claude, "{}"),
            &n,
            vec![member("the chef", "A man, brown skin.")],
            vec![cast_draft(0, "The chef cooks.", vec![0])],
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let chef = plan.cast_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        chef.fail(JobFailureKind::Declined);
        assert_eq!(chef.set_portrait(image(&n, "chef-1.png")), None);
        assert_eq!(chef.failure(), None, "a success clears the failure");
        let replaced = chef.set_portrait(image(&n, "chef-2.png")).unwrap();
        assert_eq!(replaced.file, "chef-1.png");
        assert_eq!(
            plan.missing_portraits(&[0], StyleChoice::FromNotes),
            Vec::<usize>::new()
        );
        assert_eq!(plan.files().collect::<Vec<_>>(), ["chef-2.png"]);

        let chef = plan.cast_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        let same = ScenePrompt::new("A man, brown skin.").unwrap();
        assert_eq!(chef.edit_description(same), (false, None));
        let (changed, dropped) =
            chef.edit_description(ScenePrompt::new("A man, brown skin, mustache.").unwrap());
        assert!(changed);
        assert_eq!(dropped.unwrap().file, "chef-2.png");
        assert!(chef.is_edited());
        assert_eq!(chef.generated_description().as_str(), "A man, brown skin.");
        assert_eq!(
            plan.missing_portraits(&[0], StyleChoice::FromNotes),
            [0],
            "drawn again next time"
        );
        assert_eq!(plan.cast_member(3), Err(NoSuchCastMember(3)));
    }

    /// A plan of two scenes with the chef and one without.
    fn chef_plan(n: &Narration) -> ScenePlan {
        ScenePlan::from_drafts(
            generation(n, Provider::Claude, "{}"),
            n,
            vec![member("the chef", "A man, brown skin.")],
            vec![
                cast_draft(0, "The chef cooks.", vec![0]),
                cast_draft(2, "The chef plates.", vec![0]),
                cast_draft(3, "An empty kitchen.", Vec::new()),
            ],
            SystemTime::UNIX_EPOCH,
        )
        .unwrap()
    }

    /// The generation of the chef's portrait.
    fn chef_portrait(plan: &ScenePlan) -> Option<GenerationId> {
        plan.cast()[0].portrait().map(|p| p.generation.id)
    }

    #[test]
    fn an_image_drawn_with_an_older_portrait_or_none_is_out_of_date() {
        let n = narration(STORY, 0);
        let mut plan = chef_plan(&n);
        let at = SystemTime::UNIX_EPOCH;
        plan.cast_mut(0, at)
            .unwrap()
            .set_portrait(image(&n, "chef-1.png"));
        let first = chef_portrait(&plan);
        plan.scene_mut(0, at)
            .unwrap()
            .add_image_drawn_with(image(&n, "scene-0.png"), Some(vec![(0, first)]));
        // Drawn without the chef's portrait, and with nobody to match.
        plan.scene_mut(1, at)
            .unwrap()
            .add_image_drawn_with(image(&n, "scene-1.png"), Some(vec![(0, None)]));
        plan.scene_mut(2, at)
            .unwrap()
            .add_image_drawn_with(image(&n, "scene-2.png"), Some(Vec::new()));
        assert_eq!(plan.with_old_portraits(), [1], "only the one drawn without");

        // A new portrait: the scene drawn with the first one shows the old
        // face.
        plan.cast_mut(0, at)
            .unwrap()
            .set_portrait(image(&n, "chef-2.png"));
        assert_eq!(plan.with_old_portraits(), [0, 1]);

        // Drawn again with the new portrait: the new image waits, and the
        // scene is no longer counted; rejected, it is again.
        let second = chef_portrait(&plan);
        let scene = plan.scene_mut(0, at).unwrap();
        scene.add_image_drawn_with(image(&n, "scene-0b.png"), Some(vec![(0, second)]));
        assert_eq!(plan.with_old_portraits(), [1]);
        plan.scene_mut(0, at).unwrap().reject_image().unwrap();
        assert_eq!(plan.with_old_portraits(), [0, 1]);
        let scene = plan.scene_mut(0, at).unwrap();
        scene.add_image_drawn_with(image(&n, "scene-0c.png"), Some(vec![(0, second)]));
        scene.accept_image().unwrap();
        assert_eq!(scene.image_portraits(), Some(&vec![(0, second)]));
        assert_eq!(scene.pending_portraits(), None);
        assert_eq!(plan.with_old_portraits(), [1]);

        // A new look drops the portrait: every image of the chef shows the
        // old one.
        plan.cast_mut(0, at)
            .unwrap()
            .edit_description(ScenePrompt::new("A man, brown skin, mustache.").unwrap());
        assert_eq!(plan.with_old_portraits(), [0, 1]);
    }

    #[test]
    fn a_person_drawn_from_the_description_by_choice_has_no_portrait_to_match() {
        let n = narration(STORY, 0);
        let mut plan = chef_plan(&n);
        let at = SystemTime::UNIX_EPOCH;
        plan.scene_mut(0, at)
            .unwrap()
            .add_image_drawn_with(image(&n, "scene-0.png"), Some(vec![(0, None)]));
        assert_eq!(plan.with_old_portraits(), [0]);
        assert_eq!(plan.missing_portraits(&[0, 1], StyleChoice::FromNotes), [0]);

        let chef = plan.cast_mut(0, at).unwrap();
        assert!(chef.set_described_only(true));
        assert!(!chef.set_described_only(true), "already chosen");
        assert!(plan.cast()[0].is_described_only());
        assert!(plan.with_old_portraits().is_empty());
        assert!(
            plan.missing_portraits(&[0, 1], StyleChoice::FromNotes)
                .is_empty(),
            "no portrait is drawn for them"
        );

        plan.cast_mut(0, at).unwrap().set_described_only(false);
        assert_eq!(plan.with_old_portraits(), [0]);
    }

    #[test]
    fn an_image_from_before_portraits_were_recorded_is_told_by_the_portrait_there_is_now() {
        let n = narration(STORY, 0);
        let mut plan = chef_plan(&n);
        let at = SystemTime::UNIX_EPOCH;
        let later = |file: &str, seconds: u64| {
            let mut image = image(&n, file);
            image.generation.generated_at = at + Duration::from_secs(seconds);
            image
        };
        // No portrait (it was declined, and the scene drawn anyway as
        // Bardo once did): the image cannot show the one to come.
        plan.scene_mut(0, at)
            .unwrap()
            .add_image(later("scene-0.png", 20));
        plan.scene_mut(2, at)
            .unwrap()
            .add_image(later("scene-2.png", 20));
        assert_eq!(
            plan.with_old_portraits(),
            [0],
            "the empty kitchen shows nobody"
        );

        let mut plan = chef_plan(&n);
        plan.cast_mut(0, at)
            .unwrap()
            .set_portrait(later("chef-1.png", 10));
        plan.scene_mut(0, at)
            .unwrap()
            .add_image(later("scene-0.png", 20));
        assert!(plan.with_old_portraits().is_empty(), "nothing tells");

        plan.cast_mut(0, at)
            .unwrap()
            .set_portrait(later("chef-2.png", 30));
        assert_eq!(plan.with_old_portraits(), [0], "it cannot show that one");

        // A new look drops the portrait: the same.
        plan.cast_mut(0, at)
            .unwrap()
            .edit_description(ScenePrompt::new("A man, brown skin, mustache.").unwrap());
        assert_eq!(plan.with_old_portraits(), [0]);
    }

    #[test]
    fn an_image_drawn_while_a_person_was_written_out_does_not_show_their_portrait() {
        let n = narration(STORY, 0);
        let mut plan = chef_plan(&n);
        let at = SystemTime::UNIX_EPOCH;
        plan.cast_mut(0, at)
            .unwrap()
            .set_portrait(image(&n, "chef-1.png"));
        let planned = plan.scenes()[0].prompt().clone();
        let scene = plan.scene_mut(0, at).unwrap();
        scene.edit_prompt(ScenePrompt::new("An empty stove.").unwrap());
        scene.add_image_drawn_with(image(&n, "scene-0.png"), Some(Vec::new()));
        assert!(plan.with_old_portraits().is_empty(), "nobody is in it");

        // Written back in: the image was drawn without them.
        plan.scene_mut(0, at).unwrap().edit_prompt(planned);
        assert_eq!(plan.scene_cast(0), [0]);
        assert_eq!(plan.with_old_portraits(), [0]);
    }

    #[test]
    fn an_image_showing_an_older_face_is_no_reference_for_the_next_ones() {
        let n = narration(STORY, 0);
        let mut plan = chef_plan(&n);
        let at = SystemTime::UNIX_EPOCH;
        let style = StyleChoice::FromNotes;
        plan.cast_mut(0, at)
            .unwrap()
            .set_portrait(image(&n, "chef-1.png"));
        let first = chef_portrait(&plan);
        plan.scene_mut(0, at)
            .unwrap()
            .add_image_drawn_with(image(&n, "scene-0.png"), Some(vec![(0, first)]));
        assert_eq!(
            plan.shot_in(0, style).map(|image| image.file.as_str()),
            Some("scene-0.png")
        );
        assert_eq!(plan.reference_shots(1, style), [(0, OtherShot::Look)]);

        // A new portrait: the image of the old face would pull the next
        // one back to it.
        plan.cast_mut(0, at)
            .unwrap()
            .set_portrait(image(&n, "chef-2.png"));
        assert_eq!(plan.shot_in(0, style), None);
        assert!(plan.reference_shots(1, style).is_empty());

        // Drawn again, the image that waits is the one the others follow.
        let second = chef_portrait(&plan);
        plan.scene_mut(0, at)
            .unwrap()
            .add_image_drawn_with(image(&n, "scene-0b.png"), Some(vec![(0, second)]));
        assert_eq!(
            plan.shot_in(0, style).map(|image| image.file.as_str()),
            Some("scene-0b.png")
        );
        assert_eq!(plan.reference_shots(1, style), [(0, OtherShot::Look)]);
    }

    #[test]
    fn a_scene_stops_waiting_for_a_portrait_once_nobody_in_it_lacks_one() {
        let n = narration(STORY, 0);
        let mut plan = chef_plan(&n);
        let at = SystemTime::UNIX_EPOCH;
        let style = StyleChoice::FromNotes;
        for index in [0, 1] {
            plan.scene_mut(index, at)
                .unwrap()
                .fail(JobFailureKind::PortraitMissing);
        }
        // Another failure is not about the portrait.
        plan.scene_mut(2, at)
            .unwrap()
            .fail(JobFailureKind::Declined);
        assert!(plan.settle_portrait_waits(style).is_empty(), "still none");

        let chef = plan.cast_mut(0, at).unwrap();
        chef.fail(JobFailureKind::Declined);
        chef.set_described_only(true);
        assert_eq!(chef.failure(), None, "the portrait is no longer asked for");
        assert_eq!(plan.settle_portrait_waits(style), [0, 1]);
        assert_eq!(plan.scenes()[0].failure(), None);
        assert_eq!(plan.scenes()[2].failure(), Some(JobFailureKind::Declined));
        assert!(plan.settle_portrait_waits(style).is_empty());

        // A portrait that will not read is let go, and noted.
        let chef = plan.cast_mut(0, at).unwrap();
        chef.set_described_only(false);
        chef.set_portrait(image(&n, "chef-1.png"));
        let dropped = chef.drop_portrait(JobFailureKind::Files);
        assert_eq!(
            dropped.map(|portrait| portrait.file),
            Some("chef-1.png".into())
        );
        assert_eq!(chef.portrait(), None);
        assert_eq!(chef.failure(), Some(JobFailureKind::Files));
    }

    #[test]
    fn an_edited_prompt_is_drawn_only_with_the_people_it_still_names() {
        let n = narration(STORY, 0);
        let mut plan = ScenePlan::from_drafts(
            generation(&n, Provider::Claude, "{}"),
            &n,
            vec![member("the chef", "A man."), member("Ana", "A woman.")],
            vec![cast_draft(0, "The chef and Ana cook.", vec![0, 1])],
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(plan.scene_cast(0), [0, 1]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        scene.edit_prompt(ScenePrompt::new("Our Chef alone at the stove.").unwrap());
        assert_eq!(plan.scene_cast(0), [0], "Ana was written out");
        assert_eq!(plan.scenes_with(1), Vec::<usize>::new());
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        scene.edit_prompt(ScenePrompt::new("An empty kitchen.").unwrap());
        assert!(plan.scene_cast(0).is_empty());
        assert_eq!(
            plan.missing_portraits(&[0], StyleChoice::FromNotes),
            Vec::<usize>::new()
        );

        let ana = plan.cast_mut(1, SystemTime::UNIX_EPOCH).unwrap();
        ana.fail(JobFailureKind::Declined);
        ana.edit_description(ScenePrompt::new("A woman, red hair.").unwrap());
        assert_eq!(ana.failure(), None, "the failure was about the old look");
    }

    #[test]
    fn a_scene_prompt_names_its_people_and_their_references() {
        let n = narration(STORY, 0);
        let plan = ScenePlan::from_drafts(
            generation(&n, Provider::Claude, "{}"),
            &n,
            vec![
                member("the chef", "A man in his fifties, brown skin."),
                member("the critic", "A woman in her forties, grey bob."),
            ],
            vec![cast_draft(0, "The chef serves the critic.", vec![0, 1])],
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let shown: Vec<&CastMember> = plan.cast().iter().collect();
        let prompt = scene_image_prompt(
            "The chef serves the critic.",
            &shown,
            &[true, false],
            SceneSetting::default(),
        );
        assert!(prompt.starts_with("The chef serves the critic.\n\n"));
        assert!(prompt.contains("Reference 1: the chef, A man in his fifties, brown skin."));
        assert!(!prompt.contains("Reference 2"));
        assert!(prompt.contains("the critic: A woman in her forties, grey bob."));
        assert_eq!(
            scene_image_prompt("Stars.", &[], &[], SceneSetting::default()),
            "Stars."
        );
        assert!(
            plan.cast()[0]
                .portrait_prompt(StyleChoice::FromNotes)
                .contains("the chef: A man in his fifties")
        );
    }

    fn somewhere(name: &str, description: &str) -> PlaceDraft {
        PlaceDraft {
            name: name.into(),
            description: description.into(),
        }
    }

    fn placed(first_sentence: usize, prompt: &str, place: Option<usize>) -> SceneDraft {
        SceneDraft {
            first_sentence,
            prompt: prompt.into(),
            place,
            ..SceneDraft::default()
        }
    }

    /// Four scenes: the kitchen, the street, nowhere that recurs, and the
    /// kitchen again.
    fn storyboard(n: &Narration) -> ScenePlan {
        ScenePlan::from_draft(
            generation(n, Provider::Claude, "{}"),
            n,
            PlanDraft {
                product: None,
                cast: Vec::new(),
                places: vec![
                    somewhere(
                        "the kitchen",
                        "A small tiled kitchen at night, one warm lamp.",
                    ),
                    somewhere("  ", "No name."),
                    somewhere("the cellar", "Never shown."),
                    somewhere("The Street", "A wet cobbled street at dawn."),
                    somewhere("the attic", "  "),
                ],
                scenes: vec![
                    placed(0, "Wide shot of the kitchen.", Some(0)),
                    placed(1, "The street, empty.", Some(3)),
                    placed(2, "A map of the city.", Some(9)),
                    placed(3, "Close on the stove in the kitchen.", Some(0)),
                ],
            },
            SystemTime::UNIX_EPOCH,
        )
        .unwrap()
    }

    /// Gives scene `index` an image drawn from its prompt and its place as
    /// they are now.
    fn draw(plan: &mut ScenePlan, n: &Narration, index: usize) {
        draw_in(plan, n, index, StyleChoice::FromNotes);
    }

    /// The same, drawn in `style`.
    fn draw_in(plan: &mut ScenePlan, n: &Narration, index: usize, style: StyleChoice) {
        let mut image = image(n, &format!("scene-{index}.png"));
        image.generation.prompt = scene_image_prompt(
            plan.scenes()[index].prompt().as_str(),
            &[],
            &[],
            SceneSetting {
                place: plan
                    .scene_place(index)
                    .and_then(|place| plan.place(place).ok()),
                shots: &[],
                style,
                product: None,
            },
        );
        plan.scene_mut(index, SystemTime::UNIX_EPOCH)
            .unwrap()
            .add_image(image);
    }

    #[test]
    fn places_are_planned_once_and_scenes_point_at_them() {
        let n = narration(STORY, 0);
        let plan = storyboard(&n);
        let names: Vec<&str> = plan
            .places()
            .iter()
            .map(|place| place.name.as_str())
            .collect();
        assert_eq!(names, ["the kitchen", "The Street"], "usable and shown");
        let places: Vec<Option<usize>> = plan.scenes().iter().map(Scene::place).collect();
        assert_eq!(places, [Some(0), Some(1), None, Some(0)]);
        assert_eq!(
            plan.place(0).unwrap().description().as_str(),
            "A small tiled kitchen at night, one warm lamp."
        );
        assert_eq!(plan.place(2), Err(NoSuchPlace(2)));
        assert!(plan.scenes().iter().all(|scene| scene.cast().is_empty()));

        // A plan without places is the plan it always was.
        let plain = self::plan(&n, vec![draft(0, "A."), draft(2, "B.")]);
        assert!(plain.places().is_empty());
        assert_eq!(plain.scene_place(0), None);
    }

    #[test]
    fn no_more_places_than_the_limit_are_kept() {
        let text = ["Go."; MAX_PLACES + 2].join(" ");
        let n = narration(&text, 0);
        let plan = ScenePlan::from_draft(
            generation(&n, Provider::Claude, "{}"),
            &n,
            PlanDraft {
                product: None,
                cast: Vec::new(),
                places: (0..MAX_PLACES + 2)
                    .map(|index| somewhere(&format!("room {index}"), "A room."))
                    .collect(),
                scenes: (0..MAX_PLACES + 2)
                    .map(|index| placed(index, "A room.", Some(index)))
                    .collect(),
            },
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(plan.places().len(), MAX_PLACES);
        assert_eq!(plan.scenes()[MAX_PLACES - 1].place(), Some(MAX_PLACES - 1));
        assert_eq!(plan.scenes()[MAX_PLACES].place(), None, "past the limit");
        assert_eq!(
            plan.scenes()[MAX_PLACES].prompt().as_str(),
            format!("A room. The scene happens in room {MAX_PLACES}: A room."),
            "its description stays in its own prompt"
        );
        assert!(!plan.scenes()[MAX_PLACES].is_edited());
        assert_eq!(plan.scenes()[0].prompt().as_str(), "A room.");
    }

    #[test]
    fn past_the_limit_the_places_with_the_most_scenes_stay() {
        let text = ["Go."; MAX_PLACES + 3].join(" ");
        let n = narration(&text, 0);
        // One scene in each of nine rooms, then two more in the last room.
        let last = MAX_PLACES;
        let plan = ScenePlan::from_draft(
            generation(&n, Provider::Claude, "{}"),
            &n,
            PlanDraft {
                product: None,
                cast: Vec::new(),
                places: (0..=last)
                    .map(|index| somewhere(&format!("room {index}"), "A room."))
                    .collect(),
                scenes: (0..MAX_PLACES + 3)
                    .map(|index| placed(index, "A room.", Some(index.min(last))))
                    .collect(),
            },
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let names: Vec<&str> = plan
            .places()
            .iter()
            .map(|place| place.name.as_str())
            .collect();
        assert_eq!(names.len(), MAX_PLACES);
        assert_eq!(names.last(), Some(&"room 8"), "three scenes happen there");
        assert!(!names.contains(&"room 7"), "the last listed with one scene");
        assert_eq!(plan.scenes()[7].place(), None);
        assert_eq!(plan.scenes()[MAX_PLACES + 2].place(), Some(MAX_PLACES - 1));
    }

    #[test]
    fn a_scene_is_drawn_with_the_shots_it_belongs_with() {
        let n = narration(STORY, 0);
        let mut plan = storyboard(&n);
        for index in 0..4 {
            assert!(
                plan.reference_shots(index, StyleChoice::FromNotes)
                    .is_empty(),
                "nothing is drawn"
            );
        }

        draw(&mut plan, &n, 0);
        assert!(
            plan.reference_shots(0, StyleChoice::FromNotes).is_empty(),
            "never itself"
        );
        assert_eq!(
            plan.reference_shots(1, StyleChoice::FromNotes),
            [(0, OtherShot::Look)],
            "a new place takes only the video's look"
        );
        assert_eq!(
            plan.reference_shots(2, StyleChoice::FromNotes),
            [(0, OtherShot::Look)],
            "and so does a scene with no place"
        );
        assert_eq!(
            plan.reference_shots(3, StyleChoice::FromNotes),
            [(0, OtherShot::SamePlace)],
            "back in the kitchen"
        );

        // A second scene in the street, right after the first.
        let street = Scene::planned(
            ms(0),
            ms(1),
            String::new(),
            ScenePrompt::new("A cat crosses the street.").unwrap(),
        )
        .with_place(Some(1));
        plan.scenes.insert(2, street);
        draw(&mut plan, &n, 1);
        assert_eq!(
            plan.reference_shots(2, StyleChoice::FromNotes),
            [(1, OtherShot::Before)],
            "the scene before is also the place's first"
        );
        draw(&mut plan, &n, 2);
        let third = Scene::planned(
            ms(1),
            ms(2),
            String::new(),
            ScenePrompt::new("The cat on a wall of the street.").unwrap(),
        )
        .with_place(Some(1));
        plan.scenes.insert(3, third);
        assert_eq!(
            plan.reference_shots(3, StyleChoice::FromNotes),
            [(1, OtherShot::SamePlace), (2, OtherShot::Before)],
            "the place, then what it continues from"
        );
        assert_eq!(
            plan.reference_shots(1, StyleChoice::FromNotes),
            [(2, OtherShot::SamePlace)],
            "drawn again, the first of a place follows the others"
        );
        assert!(plan.reference_shots(99, StyleChoice::FromNotes).is_empty());
    }

    #[test]
    fn a_plan_without_places_still_shares_its_look() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A."), draft(1, "B."), draft(2, "C.")]);
        draw(&mut plan, &n, 1);
        assert_eq!(
            plan.reference_shots(0, StyleChoice::FromNotes),
            [(1, OtherShot::Look)]
        );
        assert!(plan.reference_shots(1, StyleChoice::FromNotes).is_empty());
        assert_eq!(
            plan.reference_shots(2, StyleChoice::FromNotes),
            [(1, OtherShot::Look)]
        );
    }

    #[test]
    fn an_edited_prompt_keeps_its_place_only_while_it_names_it() {
        let n = narration(STORY, 0);
        let mut plan = storyboard(&n);
        draw(&mut plan, &n, 0);
        assert_eq!(plan.scene_place(3), Some(0));
        let scene = plan.scene_mut(3, SystemTime::UNIX_EPOCH).unwrap();
        scene.edit_prompt(ScenePrompt::new("The stove, cold, in the Kitchen.").unwrap());
        assert_eq!(plan.scene_place(3), Some(0), "still there");
        let scene = plan.scene_mut(3, SystemTime::UNIX_EPOCH).unwrap();
        scene.edit_prompt(ScenePrompt::new("A train leaving the city.").unwrap());
        assert_eq!(plan.scene_place(3), None, "written somewhere else");
        assert_eq!(
            plan.reference_shots(3, StyleChoice::FromNotes),
            [(0, OtherShot::Look)],
            "so the kitchen is no longer its place"
        );
        assert_eq!(plan.scenes()[3].place(), Some(0), "as planned");
    }

    #[test]
    fn a_place_is_named_by_whole_words_only() {
        assert!(names("Close on the stove in the Kitchen.", "kitchen"));
        assert!(names("The kitchen's window, open.", "kitchen"));
        assert!(names("launch pad at dawn", "launch pad"));
        assert!(!names("A red barn in a field.", "bar"));
        assert!(!names("She ties her scarf.", "car"));
        assert!(!names("A streetlamp flickers.", "street"));
        assert!(!names("Anything.", ""));
    }

    #[test]
    fn an_image_of_a_place_as_it_was_no_longer_holds_the_next_ones_to_it() {
        let n = narration(STORY, 0);
        let mut plan = storyboard(&n);
        draw(&mut plan, &n, 0);
        assert_eq!(
            plan.reference_shots(3, StyleChoice::FromNotes),
            [(0, OtherShot::SamePlace)]
        );

        let noon = ScenePrompt::new("A wide farmhouse kitchen at noon.").unwrap();
        plan.place_mut(0, SystemTime::UNIX_EPOCH)
            .unwrap()
            .edit_description(noon);
        assert_eq!(
            plan.reference_shots(3, StyleChoice::FromNotes),
            [(0, OtherShot::Look)],
            "the old kitchen only lends its look"
        );
        // Drawn again from the new description, it only counts once the
        // user took the new image.
        draw(&mut plan, &n, 0);
        assert_eq!(
            plan.reference_shots(3, StyleChoice::FromNotes),
            [(0, OtherShot::Look)]
        );
        plan.scene_mut(0, SystemTime::UNIX_EPOCH)
            .unwrap()
            .accept_image()
            .unwrap();
        assert_eq!(
            plan.reference_shots(3, StyleChoice::FromNotes),
            [(0, OtherShot::SamePlace)]
        );
    }

    #[test]
    fn a_place_can_be_described_again() {
        let n = narration(STORY, 0);
        let mut plan = storyboard(&n);
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(5);
        let kitchen = plan.place_mut(0, later).unwrap();
        assert!(!kitchen.is_edited());
        let same = kitchen.description().clone();
        assert!(!kitchen.edit_description(same));
        assert!(kitchen.edit_description(ScenePrompt::new("A steel kitchen at noon.").unwrap()));
        assert!(kitchen.is_edited());
        assert_eq!(
            kitchen.generated_description().as_str(),
            "A small tiled kitchen at night, one warm lamp."
        );
        assert_eq!(plan.updated_at, later);
        assert_eq!(plan.place_mut(7, later).map(|_| ()), Err(NoSuchPlace(7)));
    }

    #[test]
    fn a_scene_prompt_says_where_it_happens_and_what_each_shot_is_for() {
        let n = narration(STORY, 0);
        let plan = ScenePlan::from_draft(
            generation(&n, Provider::Claude, "{}"),
            &n,
            PlanDraft {
                product: None,
                cast: vec![member("the chef", "A man in his fifties.")],
                places: vec![somewhere("the kitchen", "A small tiled kitchen at night.")],
                scenes: vec![SceneDraft {
                    place: Some(0),
                    ..cast_draft(0, "The chef plates a dish in the kitchen.", vec![0])
                }],
            },
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let shown: Vec<&CastMember> = plan.cast().iter().collect();
        let kitchen = plan.place(0).ok();

        // Described, with nothing drawn yet to follow.
        let alone = scene_image_prompt(
            "The chef plates a dish.",
            &shown,
            &[false],
            SceneSetting {
                place: kitchen,
                shots: &[],
                ..SceneSetting::default()
            },
        );
        assert!(
            alone
                .ends_with("\n\nThe scene happens in the kitchen: A small tiled kitchen at night.")
        );
        assert!(!alone.contains("Reference"));

        // Portraits first, then the other shots, numbered on.
        let prompt = scene_image_prompt(
            "The chef plates a dish.",
            &shown,
            &[true],
            SceneSetting {
                place: kitchen,
                shots: &[OtherShot::SamePlace, OtherShot::Before],
                ..SceneSetting::default()
            },
        );
        assert!(prompt.contains("Reference 1: the chef, A man in his fifties."));
        assert!(prompt.contains("\n\nThe reference images after those are other shots"));
        assert!(prompt.contains("Reference 2: another shot in this same place."));
        assert!(prompt.contains("only if this prompt names them"));
        assert!(prompt.contains("Reference 3: the shot right before this one"));
        assert!(!prompt.contains("Reference 4"));
        let place = prompt.find("The scene happens in the kitchen").unwrap();
        assert!(place < prompt.find("Reference 2").unwrap());

        // The look of another place is all a new place takes.
        let look = scene_image_prompt(
            "A wet street at dawn.",
            &[],
            &[],
            SceneSetting {
                place: None,
                shots: &[OtherShot::Look],
                ..SceneSetting::default()
            },
        );
        assert!(look.contains("\n\nThe reference images are other shots of the same video"));
        assert!(look.contains("Reference 1: another shot of this video. Match only how"));
        assert!(look.contains("its people and its composition are not part of this scene."));
        assert!(!look.contains("The scene happens in"));
    }

    #[test]
    fn the_planned_motion_prompt_comes_before_the_image_prompt() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![cast_draft(0, "A ship at dawn.", Vec::new())]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(scene.motion_prompt().as_str(), "Slow push in.");
        assert_eq!(scene.own_motion_prompt(), None);
        let same = ScenePrompt::new("Slow push in.").unwrap();
        assert!(!scene.set_motion_prompt(Some(same)), "same as planned");
        let mine = ScenePrompt::new("Pan left.").unwrap();
        assert!(scene.set_motion_prompt(Some(mine)));
        assert_eq!(scene.motion_prompt().as_str(), "Pan left.");
        assert!(scene.set_motion_prompt(None));
        assert_eq!(scene.motion_prompt().as_str(), "Slow push in.");
    }

    /// `narration`, read again: the same text, every character `char_ms`
    /// long, lasting `tail` ms past its last character.
    fn read_again(narration: &Narration, char_ms: u64, tail: u64) -> Narration {
        read(narration, narration.text.as_str(), char_ms, tail)
    }

    /// Another narration of `narration`'s project, reading `text`.
    fn read(narration: &Narration, text: &str, char_ms: u64, tail: u64) -> Narration {
        let alignment = Alignment {
            chars: text
                .chars()
                .enumerate()
                .map(|(n, c)| CharTiming {
                    text: c.to_string(),
                    start: ms(n as u64 * char_ms),
                    end: ms((n as u64 + 1) * char_ms),
                })
                .collect(),
        };
        Narration {
            id: NarrationId::new(),
            text: ScriptText::new(text).unwrap(),
            duration: alignment.end() + ms(tail),
            words: WordTimings::from_alignment(text, &alignment),
            ..narration.clone()
        }
    }

    /// A storyboard of `STORY` with a person, a place and every kind of
    /// picture: an image, a new one waiting, a clip, a new one waiting and
    /// a portrait.
    fn pictured_plan(n: &Narration) -> ScenePlan {
        let mut plan = ScenePlan::from_draft(
            generation(n, Provider::Claude, "{}"),
            n,
            PlanDraft {
                product: None,
                cast: vec![member("the probe", "A small silver probe with one dish.")],
                places: vec![somewhere(
                    "the hangar",
                    "A dim hangar with a concrete floor.",
                )],
                scenes: vec![
                    SceneDraft {
                        place: Some(0),
                        ..cast_draft(0, "The probe in the hangar.", vec![0])
                    },
                    cast_draft(1, "A voice from the dark.", Vec::new()),
                    SceneDraft {
                        place: Some(0),
                        ..cast_draft(2, "The probe answers in the hangar.", vec![0])
                    },
                ],
            },
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let first = image(n, "one.png");
        let moving = clip(n, "one.mp4", &first);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        scene.add_image(first.clone());
        scene.add_image(image(n, "one-again.png"));
        scene.add_clip(moving);
        scene.accept_clip().unwrap();
        scene.add_clip(clip(n, "one-again.mp4", &first));
        scene.edit_prompt(ScenePrompt::new("The probe in the hangar, at night.").unwrap());
        plan.scene_mut(2, SystemTime::UNIX_EPOCH)
            .unwrap()
            .add_image(image(n, "three.png"));
        plan.cast_mut(0, SystemTime::UNIX_EPOCH)
            .unwrap()
            .set_portrait(image(n, "probe.png"));
        plan
    }

    #[test]
    fn a_plan_counts_the_pictures_paid_for() {
        let n = narration(STORY, 0);
        assert_eq!(
            plan(&n, vec![draft(0, "A.")]).paid_pictures(),
            PaidPictures::default()
        );
        assert!(PaidPictures::default().is_empty());
        let pictures = pictured_plan(&n).paid_pictures();
        assert_eq!(
            pictures,
            PaidPictures {
                images: 4,
                clips: 2
            },
            "two images of scene 1, one of scene 3 and a portrait; two clips"
        );
        assert!(!pictures.is_empty());
    }

    #[test]
    fn a_new_narration_of_the_same_text_fits_the_plan_and_keeps_every_picture() {
        let n = narration(STORY, 0);
        let plan = pictured_plan(&n);
        // Read again, slower, with a pause at the end.
        let again = read_again(&n, 250, 700);
        assert!(plan.is_stale(Some(&again)));
        assert!(plan.reads(again.text.as_str()));

        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(60);
        let fitted = plan.fitted_to(&again, later).unwrap();

        assert!(!fitted.is_stale(Some(&again)));
        assert_eq!(fitted.id, plan.id, "the same plan: its jobs still find it");
        assert_eq!(fitted.narration, again.id);
        assert_eq!(fitted.generation, plan.generation);
        assert_eq!(fitted.updated_at, later);
        assert_eq!(fitted.cast(), plan.cast());
        assert_eq!(fitted.places(), plan.places());
        // Each scene starts where the new narration says its first word.
        let spoken = sentences(&again);
        let times: Vec<_> = fitted.scenes().iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(
            times,
            [
                (Duration::ZERO, spoken[1].start),
                (spoken[1].start, spoken[2].start),
                (spoken[2].start, again.duration),
            ]
        );
        assert_eq!(spoken[1].start, ms(10 * 250), "'Who' is the 11th character");
        // Nothing else about a scene changed.
        for (fitted, scene) in fitted.scenes().iter().zip(plan.scenes()) {
            let timed_as_before = Scene {
                start: scene.start,
                end: scene.end,
                ..fitted.clone()
            };
            assert_eq!(&timed_as_before, scene);
        }
        assert_eq!(fitted.paid_pictures(), plan.paid_pictures());
        assert_eq!(
            fitted.files().collect::<Vec<_>>(),
            plan.files().collect::<Vec<_>>()
        );
        // It is the plan the planner's answer would give on this narration.
        let planned = ScenePlan::from_drafts(
            generation(&again, Provider::Claude, "{}"),
            &again,
            Vec::new(),
            vec![draft(0, "A."), draft(1, "B."), draft(2, "C.")],
            later,
        )
        .unwrap();
        let planned: Vec<_> = planned
            .scenes()
            .iter()
            .map(|s| (s.start, s.end, s.text.clone()))
            .collect();
        let fitted_scenes: Vec<_> = fitted
            .scenes()
            .iter()
            .map(|s| (s.start, s.end, s.text.clone()))
            .collect();
        assert_eq!(fitted_scenes, planned);
    }

    #[test]
    fn fitting_a_plan_to_its_own_narration_changes_nothing() {
        let n = narration(STORY, 300);
        let plan = pictured_plan(&n);
        let fitted = plan.fitted_to(&n, plan.updated_at).unwrap();
        assert_eq!(fitted, plan);
    }

    #[test]
    fn a_narration_read_the_very_same_way_gives_the_plan_its_own_times() {
        // Accents, an opening quote, a dash on its own and a mark before
        // the first word; the last word ends past the narration's length.
        let text = "— Era uma vez, em março. \"Não sei\", disse ela.\nÉ só — nada mais.";
        for tail in [0, 300] {
            let mut n = narration(text, tail);
            if tail == 0 {
                n.duration -= ms(250);
            }
            for firsts in [vec![0], vec![0, 1, 2], vec![0, 2]] {
                let drafts = firsts.iter().map(|&first| draft(first, "A shot")).collect();
                let planned = plan(&n, drafts);
                let same = Narration {
                    id: NarrationId::new(),
                    ..n.clone()
                };
                assert!(planned.reads(text));
                let fitted = planned.fitted_to(&same, planned.updated_at).unwrap();
                assert_eq!(fitted.scenes(), planned.scenes(), "{firsts:?}");
                assert_eq!(fitted.narration, same.id);
            }
        }
    }

    #[test]
    fn a_scene_that_would_no_longer_start_a_sentence_does_not_fit() {
        let n = narration("Hi there.\nWho are you\nI am the probe.", 0);
        let planned = plan(
            &n,
            vec![draft(0, "One"), draft(1, "Two"), draft(2, "Three")],
        );
        for text in [
            // The space between two scenes is gone: one word holds both.
            "Hi there.Who are you\nI am the probe.",
            // The line break that ended a sentence is a space now.
            "Hi there.\nWho are you I am the probe.",
        ] {
            let other = read(&n, text, 100, 0);
            assert!(!planned.reads(text), "{text:?}");
            assert_eq!(
                planned.fitted_to(&other, SystemTime::UNIX_EPOCH),
                Err(OtherText),
                "{text:?}"
            );
        }
        // Other white space where a sentence still ends is the same text.
        let text = "Hi there.  Who are you\n\nI am the probe.\n";
        assert!(planned.reads(text));
        let fitted = planned
            .fitted_to(&read(&n, text, 100, 0), SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(fitted.scenes().len(), 3);
    }

    #[test]
    fn a_narration_of_other_text_does_not_fit_the_plan() {
        let n = narration(STORY, 0);
        let plan = pictured_plan(&n);
        for text in [
            // A word changed inside a scene.
            "Hi there. Who are they?\nI am \"the probe.\" It ends",
            // A sentence more at the end.
            "Hi there. Who are you?\nI am \"the probe.\" It ends here. Or not.",
            // A sentence less.
            "Hi there. Who are you?",
            // A sentence more at the start.
            "Listen. Hi there. Who are you?\nI am \"the probe.\" It ends",
            // A scene's last sentence moved into the next line break.
            "Hi there. Who are you?\nI am \"the probe.\"\nIt ends",
        ] {
            let other = read(&n, text, 100, 0);
            assert!(!plan.reads(text), "{text:?}");
            assert_eq!(
                plan.fitted_to(&other, SystemTime::UNIX_EPOCH),
                Err(OtherText),
                "{text:?}"
            );
        }
    }

    #[test]
    fn white_space_between_scenes_does_not_stop_a_plan_from_fitting() {
        let n = narration(STORY, 0);
        let plan = pictured_plan(&n);
        // The second scene now starts on a new line.
        let text = "Hi there.\n\nWho are you?\nI am \"the probe.\" It ends";
        let again = read(&n, text, 100, 0);
        let fitted = plan.fitted_to(&again, SystemTime::UNIX_EPOCH).unwrap();
        let spoken = sentences(&again);
        assert_eq!(fitted.scenes()[1].start, spoken[1].start);
        assert_eq!(fitted.scenes()[1].text, "Who are you?");
    }

    #[test]
    fn a_scene_without_words_keeps_its_place_in_a_fitted_plan() {
        let n = narration(STORY, 0);
        let plan = pictured_plan(&n);
        // As a version's placed plan can be: its middle scene lost every
        // sentence, and the first one took them.
        let mut scenes = plan.scenes().to_vec();
        scenes[0].text = "Hi there. Who are you?".into();
        scenes[0].end = scenes[2].start;
        scenes[1].text = String::new();
        scenes[1].start = scenes[2].start;
        scenes[1].end = scenes[2].start;
        let placed = ScenePlan::restore(ScenePlanRecord {
            id: plan.id,
            project: plan.project,
            owner: plan.owner,
            narration: plan.narration,
            generation: plan.generation.clone(),
            scenes,
            updated_at: plan.updated_at,
        });
        let again = read_again(&n, 200, 0);
        let fitted = placed.fitted_to(&again, SystemTime::UNIX_EPOCH).unwrap();
        let third = sentences(&again)[2].start;
        let times: Vec<_> = fitted.scenes().iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(
            times,
            [
                (Duration::ZERO, third),
                (third, third),
                (third, again.duration)
            ]
        );
    }

    #[test]
    fn a_plan_goes_stale_when_the_narration_is_generated_again() {
        let n = narration(STORY, 0);
        let plan = plan(&n, vec![draft(0, "A.")]);
        assert!(!plan.is_stale(Some(&n)));
        let again = Narration {
            id: NarrationId::new(),
            ..n.clone()
        };
        assert!(plan.is_stale(Some(&again)));
        assert!(plan.is_stale(None));
    }

    #[test]
    fn the_style_goes_after_the_people_and_the_place_and_before_the_other_shots() {
        let n = narration(STORY, 0);
        let plan = ScenePlan::from_draft(
            generation(&n, Provider::Claude, "{}"),
            &n,
            PlanDraft {
                product: None,
                cast: vec![member("the chef", "A man in his fifties.")],
                places: vec![somewhere("the kitchen", "A small tiled kitchen at night.")],
                scenes: vec![
                    SceneDraft {
                        cast: vec![0],
                        ..placed(0, "The chef plates a dish in the kitchen.", Some(0))
                    },
                    SceneDraft {
                        cast: vec![0],
                        ..placed(1, "The chef tastes it in the kitchen.", Some(0))
                    },
                ],
            },
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let shown: Vec<&CastMember> = plan.cast().iter().collect();
        let manga = StyleChoice::Catalog(crate::VisualStyle::Manga);
        let setting = SceneSetting {
            place: plan.place(0).ok(),
            shots: &[OtherShot::Before],
            style: manga,
            product: None,
        };
        let prompt = scene_image_prompt("The chef plates a dish.", &shown, &[true], setting);
        let line = manga.line().unwrap();
        let style = prompt
            .find(&line)
            .expect("the style's text is in the prompt");
        assert!(prompt.find("The scene happens in the kitchen").unwrap() < style);
        assert!(style < prompt.find("The reference images after those").unwrap());
        assert!(manga.drew(&prompt));

        // With no style chosen the prompt is what it was before styles.
        let plain = scene_image_prompt(
            "The chef plates a dish.",
            &shown,
            &[true],
            SceneSetting {
                style: StyleChoice::FromNotes,
                ..setting
            },
        );
        assert_eq!(plain, prompt.replace(&format!("\n\n{line}"), ""));
        assert!(StyleChoice::FromNotes.drew(&plain));
    }

    #[test]
    fn a_portrait_is_drawn_in_the_videos_style() {
        let n = narration(STORY, 0);
        let mut plan = ScenePlan::from_drafts(
            generation(&n, Provider::Claude, "{}"),
            &n,
            vec![member("the chef", "A man, brown skin.")],
            vec![cast_draft(0, "The chef cooks.", vec![0])],
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let notes = StyleChoice::FromNotes;
        let clay = StyleChoice::Catalog(crate::VisualStyle::ClayStopMotion);
        let anime = StyleChoice::Catalog(crate::VisualStyle::Anime);
        let chef = plan.cast_member(0).unwrap();
        let plain = chef.portrait_prompt(notes);
        assert!(plain.ends_with("no text, logos or watermarks."));
        let styled = chef.portrait_prompt(clay);
        let rest = styled
            .strip_prefix(&clay.applied(&plain))
            .expect("the portrait, then the style");
        assert!(
            rest.starts_with(" In this portrait the style decides only the medium")
                && rest.ends_with("with the whole face clearly lit."),
            "{rest}"
        );
        assert!(clay.drew(&styled));

        // A portrait from before the style was chosen is drawn again
        // first, and is no reference until then.
        let mut portrait = image(&n, "chef-1.png");
        portrait.generation.prompt = plain;
        plan.cast_mut(0, SystemTime::UNIX_EPOCH)
            .unwrap()
            .set_portrait(portrait);
        assert!(plan.missing_portraits(&[0], notes).is_empty());
        assert_eq!(plan.missing_portraits(&[0], clay), [0]);
        assert!(plan.cast_member(0).unwrap().portrait_in(clay).is_none());
        assert!(
            plan.cast_member(0).unwrap().portrait().is_some(),
            "it stays"
        );

        let mut portrait = image(&n, "chef-2.png");
        portrait.generation.prompt = plan.cast_member(0).unwrap().portrait_prompt(clay);
        plan.cast_mut(0, SystemTime::UNIX_EPOCH)
            .unwrap()
            .set_portrait(portrait);
        assert!(plan.missing_portraits(&[0], clay).is_empty());
        assert_eq!(plan.missing_portraits(&[0], anime), [0], "another style");
        assert_eq!(plan.missing_portraits(&[0], notes), [0], "and back to none");
    }

    #[test]
    fn an_image_in_another_style_holds_nothing_to_it() {
        let n = narration(STORY, 0);
        let mut plan = storyboard(&n);
        let notes = StyleChoice::FromNotes;
        let manga = StyleChoice::Catalog(crate::VisualStyle::Manga);
        draw(&mut plan, &n, 0);
        draw(&mut plan, &n, 1);
        assert_eq!(plan.reference_shots(3, notes), [(0, OtherShot::SamePlace)]);
        assert!(plan.in_another_style(notes).is_empty());

        // The video takes a style: what was drawn stays, and is no
        // reference for what is drawn next.
        assert_eq!(plan.in_another_style(manga), [0, 1]);
        for index in 0..4 {
            assert!(plan.reference_shots(index, manga).is_empty(), "{index}");
        }

        // The street is drawn in it: the look of the video is its image,
        // not the kitchen's from before.
        draw_in(&mut plan, &n, 1, manga);
        assert_eq!(plan.in_another_style(manga), [0], "drawn again, to review");
        let waiting = plan.scenes()[1].pending().unwrap().file.clone();
        assert_eq!(plan.shot_in(1, manga).unwrap().file, waiting);
        assert_eq!(
            plan.reference_shots(3, manga),
            [(1, OtherShot::Look)],
            "the new image holds the next ones before it is accepted"
        );
        assert_eq!(
            plan.shot_in(1, notes),
            plan.scenes()[1].image(),
            "and the one in use still is the notes' image"
        );
        plan.scene_mut(1, SystemTime::UNIX_EPOCH)
            .unwrap()
            .accept_image()
            .unwrap();
        assert_eq!(plan.in_another_style(manga), [0]);
        assert_eq!(plan.reference_shots(3, manga), [(1, OtherShot::Look)]);
        assert_eq!(plan.reference_shots(2, manga), [(1, OtherShot::Look)]);

        // Then the kitchen: the next scene there holds to it.
        draw_in(&mut plan, &n, 0, manga);
        plan.scene_mut(0, SystemTime::UNIX_EPOCH)
            .unwrap()
            .accept_image()
            .unwrap();
        assert!(plan.in_another_style(manga).is_empty());
        assert_eq!(plan.reference_shots(3, manga), [(0, OtherShot::SamePlace)]);
        assert_eq!(plan.in_another_style(notes), [0, 1]);
    }

    /// The pictures of a product with `kinds` of images, in order.
    fn pictures(kinds: &[crate::ProductImageKind]) -> ProductPictures {
        ProductPictures::new(
            kinds
                .iter()
                .map(|kind| (ProductImageId::new(), *kind))
                .collect(),
        )
    }

    fn kinded(first_sentence: usize, kind: &str, image: Option<usize>) -> SceneDraft {
        SceneDraft {
            first_sentence,
            prompt: format!("Scene {first_sentence}."),
            kind: kind.into(),
            product_image: image,
            ..SceneDraft::default()
        }
    }

    /// An ad's plan of `drafts` for a product with `pictures`.
    fn ad_plan(n: &Narration, pictures: &ProductPictures, drafts: Vec<SceneDraft>) -> ScenePlan {
        ScenePlan::from_draft(
            generation(n, Provider::Claude, "{}"),
            n,
            PlanDraft {
                scenes: drafts,
                product: Some(pictures.clone()),
                ..PlanDraft::default()
            },
            SystemTime::UNIX_EPOCH,
        )
        .unwrap()
    }

    fn kinds(plan: &ScenePlan) -> Vec<(SceneKind, Option<ProductImageId>)> {
        plan.scenes()
            .iter()
            .map(|scene| (scene.kind(), scene.product_image()))
            .collect()
    }

    fn shot(image: ProductImageId, file: &str) -> ComposedShot {
        ComposedShot {
            file: file.into(),
            image: Some(image),
            card: None,
            aspect: AspectRatio::Vertical,
            look: Some(ShotLook::default()),
            tones: Vec::new(),
        }
    }

    #[test]
    fn an_ads_plan_keeps_the_kinds_its_product_allows() {
        use crate::ProductImageKind::{Photo, Screen};
        let n = narration(STORY, 0);
        let both = pictures(&[Screen, Photo]);
        let plan = ad_plan(
            &n,
            &both,
            vec![
                kinded(0, "product_in_use", None),
                kinded(1, "product_shot", Some(1)),
                kinded(2, "hero", None),
                kinded(3, "end_card", None),
            ],
        );
        assert_eq!(
            kinds(&plan),
            [
                (SceneKind::ProductInUse, None),
                (SceneKind::ProductShot, both.at(1)),
                (SceneKind::Drawn, None),
                (SceneKind::EndCard, both.first()),
            ],
            "a kind Bardo does not know is a drawn scene"
        );
        assert!(plan.scenes()[1].is_composed() && plan.scenes()[3].is_composed());
        assert!(
            !plan.scenes()[0].is_composed(),
            "the product in use is drawn"
        );

        // A product shot naming an image the product does not have shows
        // its first, and an end card before the last scene is a product
        // shot.
        let plan = ad_plan(
            &n,
            &both,
            vec![
                kinded(0, "end_card", Some(1)),
                kinded(1, "product_shot", Some(7)),
                kinded(2, "drawn", Some(0)),
            ],
        );
        assert_eq!(
            kinds(&plan),
            [
                (SceneKind::ProductShot, both.at(1)),
                (SceneKind::ProductShot, both.first()),
                (SceneKind::Drawn, None),
            ]
        );

        // Screens only: the product is never drawn in use.
        let screens = pictures(&[Screen]);
        let plan = ad_plan(
            &n,
            &screens,
            vec![
                kinded(0, "product_in_use", None),
                kinded(1, "product_shot", None),
            ],
        );
        assert_eq!(
            kinds(&plan),
            [
                (SceneKind::Drawn, None),
                (SceneKind::ProductShot, screens.first())
            ]
        );
    }

    #[test]
    fn a_product_without_an_image_gets_drawn_scenes_and_an_end_card_of_text() {
        let n = narration(STORY, 0);
        let plan = ad_plan(
            &n,
            &ProductPictures::default(),
            vec![
                kinded(0, "product_shot", Some(0)),
                kinded(1, "product_in_use", None),
                kinded(2, "end_card", Some(0)),
            ],
        );
        assert_eq!(
            kinds(&plan),
            [
                (SceneKind::Drawn, None),
                (SceneKind::Drawn, None),
                (SceneKind::EndCard, None),
            ]
        );
        // The end card is composed with no image (#281): free, never
        // drawn.
        let composed: Vec<bool> = plan.scenes().iter().map(Scene::is_composed).collect();
        assert_eq!(composed, [false, false, true]);
        assert_eq!(plan.missing_images(), [0, 1], "the others are drawn");
        assert_eq!(plan.missing_shots(AspectRatio::Vertical), [2]);
    }

    #[test]
    fn a_plan_that_is_no_ads_has_drawn_scenes_whatever_the_answer_says() {
        let n = narration(STORY, 0);
        let plan = plan(
            &n,
            vec![
                kinded(0, "product_shot", Some(0)),
                kinded(1, "end_card", None),
            ],
        );
        assert_eq!(
            kinds(&plan),
            [(SceneKind::Drawn, None), (SceneKind::Drawn, None)]
        );
        assert_eq!(plan.missing_images(), [0, 1]);
    }

    #[test]
    fn a_composed_scene_is_neither_drawn_nor_animated_and_shows_its_own_picture() {
        let n = narration(STORY, 0);
        let photo = pictures(&[crate::ProductImageKind::Photo]);
        let image_id = photo.first().unwrap();
        let mut plan = ad_plan(
            &n,
            &photo,
            vec![kinded(0, "drawn", None), kinded(1, "product_shot", None)],
        );
        assert_eq!(plan.missing_images(), [0]);
        assert_eq!(plan.missing_shots(AspectRatio::Vertical), [1]);
        assert_eq!(plan.scenes()[1].picture(), None);

        let scene = plan.scene_mut(1, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(scene.add_shot(shot(image_id, "shot-a.png")), None);
        assert_eq!(scene.picture(), Some("shot-a.png"));
        assert!(!scene.can_animate());
        assert!(scene.files().any(|file| file == "shot-a.png"));
        assert!(plan.missing_shots(AspectRatio::Vertical).is_empty());
        assert_eq!(
            plan.missing_shots(AspectRatio::Landscape),
            [1],
            "made for another shape"
        );
        assert!(plan.missing_clips().is_empty());

        // Composed again under another name, the first file can go; under
        // the same name nothing does.
        let scene = plan.scene_mut(1, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(
            scene.add_shot(shot(image_id, "shot-b.png")),
            Some(shot(image_id, "shot-a.png"))
        );
        assert_eq!(scene.add_shot(shot(image_id, "shot-b.png")), None);

        // A drawn scene takes no composed picture.
        let drawn = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(
            drawn.add_shot(shot(image_id, "stray.png")),
            Some(shot(image_id, "stray.png"))
        );
        assert_eq!(drawn.picture(), None);
        draw(&mut plan, &n, 0);
        assert_eq!(plan.scenes()[0].picture(), Some("scene-0.png"));
        assert!(plan.scenes()[0].can_animate());
    }

    #[test]
    fn a_new_look_is_composed_again_and_keeps_its_picture_meanwhile() {
        let n = narration(STORY, 0);
        let photo = pictures(&[crate::ProductImageKind::Photo]);
        let image_id = photo.first().unwrap();
        let mut plan = ad_plan(
            &n,
            &photo,
            vec![kinded(0, "drawn", None), kinded(1, "product_shot", None)],
        );
        plan.scene_mut(1, SystemTime::UNIX_EPOCH)
            .unwrap()
            .add_shot(shot(image_id, "shot-a.png"));
        let look = ShotLook {
            placement: crate::ShotPlacement::Left,
            ..ShotLook::default()
        };
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(5);
        assert_eq!(plan.set_shot_look(1, look, later), Ok(true));
        assert_eq!(plan.updated_at, later);
        assert_eq!(plan.scenes()[1].look(), look);
        assert_eq!(plan.missing_shots(AspectRatio::Vertical), [1]);
        assert_eq!(plan.scenes()[1].picture(), Some("shot-a.png"));
        assert_eq!(plan.set_shot_look(1, look, later), Ok(false));
        assert_eq!(
            plan.set_shot_look(0, look, later),
            Err(SceneKindError::NotComposed)
        );
        assert_eq!(
            plan.set_shot_look(5, look, later),
            Err(SceneKindError::NoSuchScene(5))
        );

        // A picture made before looks is composed again.
        let mut old = shot(image_id, "shot-b.png");
        old.look = None;
        let scene = plan.scene_mut(1, SystemTime::UNIX_EPOCH).unwrap();
        scene.add_shot(old);
        assert!(scene.needs_shot(AspectRatio::Vertical));
        let mut new = shot(image_id, "shot-c.png");
        new.look = Some(look);
        scene.add_shot(new);
        assert!(!scene.needs_shot(AspectRatio::Vertical));
    }

    #[test]
    fn a_composed_scene_is_not_among_the_scenes_to_draw_again() {
        let n = narration(STORY, 0);
        let mut plan = storyboard(&n);
        let photo = pictures(&[crate::ProductImageKind::Photo]);
        for index in 0..2 {
            draw(&mut plan, &n, index);
        }
        let style = StyleChoice::Catalog(crate::VisualStyle::ALL[0]);
        assert_eq!(plan.in_another_style(style), [0, 1]);
        plan.set_scene_kind(
            1,
            SceneKind::ProductShot,
            None,
            &photo,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(
            plan.in_another_style(style),
            [0],
            "its drawing is not shown, so it is not drawn again in the new style"
        );
        assert!(!plan.with_old_portraits().contains(&1));
    }

    #[test]
    fn a_composed_scene_is_no_reference_for_another() {
        let n = narration(STORY, 0);
        let mut plan = storyboard(&n);
        let photo = pictures(&[crate::ProductImageKind::Photo]);
        // The street scene was drawn, then made a product shot: it keeps
        // the drawing, which no other scene is drawn from.
        draw(&mut plan, &n, 1);
        plan.set_scene_kind(
            1,
            SceneKind::ProductShot,
            None,
            &photo,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert!(plan.scenes()[1].image().is_some());
        assert_eq!(plan.scene_place(1), None, "it happens nowhere");
        assert!(plan.shot_in(1, StyleChoice::FromNotes).is_none());
        assert!(
            plan.reference_shots(0, StyleChoice::FromNotes).is_empty(),
            "not even for the look"
        );
        // The look reference is the first drawn picture.
        draw(&mut plan, &n, 3);
        assert_eq!(
            plan.reference_shots(2, StyleChoice::FromNotes),
            [(3, OtherShot::Look)]
        );

        // A scene continues from the drawn scene before it, over a
        // composed one in between.
        let mut plan = storyboard(&n);
        plan.scenes[1].place = Some(0);
        plan.scenes[2].place = Some(0);
        for index in 0..3 {
            draw(&mut plan, &n, index);
        }
        assert_eq!(
            plan.reference_shots(3, StyleChoice::FromNotes),
            [(0, OtherShot::SamePlace), (2, OtherShot::Before)]
        );
        plan.set_scene_kind(
            2,
            SceneKind::ProductShot,
            None,
            &photo,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(
            plan.reference_shots(3, StyleChoice::FromNotes),
            [(0, OtherShot::SamePlace), (1, OtherShot::Before)],
            "the composed scene is skipped as the shot before"
        );
        assert!(plan.scene_cast(2).is_empty());
    }

    #[test]
    fn the_product_in_use_is_drawn_with_the_product_and_checked_before_it_moves() {
        use crate::ProductImageKind::Photo;
        let n = narration(STORY, 0);
        let now = SystemTime::UNIX_EPOCH;
        let photo = pictures(&[Photo]);
        let mut plan = ad_plan(
            &n,
            &photo,
            vec![kinded(0, "drawn", None), kinded(1, "drawn", None)],
        );
        // A drawing made without the product does not count for it.
        plan.scene_mut(0, now)
            .unwrap()
            .add_image(image(&n, "old.png"));
        assert_eq!(
            plan.set_scene_kind(0, SceneKind::ProductInUse, None, &photo, now),
            Ok(KindChange::Drawn {
                needs_image: true,
                dropped: None,
            })
        );
        assert!(plan.missing_images().contains(&0));
        assert_eq!(plan.unchecked_products(), [0]);
        let scene = plan.scene_mut(0, now).unwrap();
        assert_eq!(scene.check_product(), Err(ProductCheckError::NotDrawn));
        assert!(!scene.needs_product_check());

        // Drawn with it, it waits for review, then for the check.
        scene.add_drawing(image(&n, "with.png"), None, true);
        assert!(scene.pending_product_drawn());
        assert!(!plan.missing_images().contains(&0));
        let scene = plan.scene_mut(0, now).unwrap();
        scene.accept_image().unwrap();
        assert!(scene.needs_product_check());
        assert!(!scene.product_checked());
        assert!(!plan.missing_clips().contains(&0), "not animated unchecked");
        let scene = plan.scene_mut(0, now).unwrap();
        assert_eq!(scene.check_product(), Ok(true));
        assert_eq!(scene.check_product(), Ok(false));
        assert!(scene.product_checked() && !scene.needs_product_check());
        assert!(plan.unchecked_products().is_empty());
        assert!(plan.missing_clips().contains(&0));

        // Its clip keeps the product still, and plays once checked.
        let scene = plan.scene_mut(0, now).unwrap();
        assert!(scene.clip_prompt().starts_with(PRODUCT_MOTION_RULE));
        let source = scene.image().unwrap().clone();
        scene.add_clip(clip(&n, "moves.mp4", &source));
        scene.accept_clip().unwrap();
        assert!(!scene.clip_checked());
        assert!(matches!(
            crate::VideoSource::of(scene),
            crate::VideoSource::Still(file) if file == "with.png"
        ));
        assert_eq!(scene.check_product_clip(), Ok(true));
        assert!(matches!(
            crate::VideoSource::of(scene),
            crate::VideoSource::Clip { .. }
        ));

        // Drawn again and accepted, it is not checked any more.
        scene.add_drawing(image(&n, "again.png"), None, true);
        scene.accept_image().unwrap();
        assert!(scene.needs_product_check());
        // Its clip, of the earlier picture, plays no more and cannot be
        // checked until the new picture is.
        assert!(!scene.clip_checked() && !scene.may_check_clip());
        assert_eq!(
            scene.check_product_clip(),
            Err(ProductCheckError::PictureUnchecked)
        );
        assert!(matches!(
            crate::VideoSource::of(scene),
            crate::VideoSource::Still(_)
        ));
        assert_eq!(plan.unchecked_products(), [0]);

        // Other scenes need no check, and their clip prompt is the motion.
        let other = plan.scene_mut(1, now).unwrap();
        assert!(other.product_checked() && other.clip_checked());
        assert_eq!(other.clip_prompt(), other.motion_prompt().as_str());
        assert_eq!(other.check_product(), Err(ProductCheckError::NotInUse));
    }

    #[test]
    fn a_scenes_kind_changes_within_what_the_product_allows() {
        use crate::ProductImageKind::{Photo, Screen};
        let n = narration(STORY, 0);
        let both = pictures(&[Screen, Photo]);
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(9);
        let mut plan = ad_plan(
            &n,
            &both,
            vec![
                kinded(0, "drawn", None),
                kinded(1, "drawn", None),
                kinded(2, "drawn", None),
            ],
        );
        draw(&mut plan, &n, 0);

        // To a product shot: composed at once, from the first image unless
        // another is named, and the drawing is kept.
        assert_eq!(
            plan.set_scene_kind(0, SceneKind::ProductShot, None, &both, later),
            Ok(KindChange::Compose)
        );
        assert_eq!(plan.updated_at, later);
        assert_eq!(plan.scenes()[0].product_image(), both.first());
        assert!(plan.scenes()[0].image().is_some());
        assert_eq!(
            plan.set_scene_kind(0, SceneKind::ProductShot, None, &both, later),
            Ok(KindChange::Same)
        );
        assert_eq!(
            plan.set_scene_kind(0, SceneKind::ProductShot, both.at(1), &both, later),
            Ok(KindChange::Compose),
            "another image is another picture"
        );
        assert_eq!(
            plan.scenes()[0].picture(),
            None,
            "no picture of another image stands in for it"
        );
        plan.scene_mut(0, later)
            .unwrap()
            .add_shot(shot(both.at(1).unwrap(), "shot.png"));
        assert_eq!(plan.scenes()[0].picture(), Some("shot.png"));
        // A picture that could not be composed is not held against the
        // drawing it goes back to.
        plan.scene_mut(0, later)
            .unwrap()
            .fail(JobFailureKind::Files);

        // Away from it: the drawing it kept is there, so nothing is paid,
        // and the composed picture goes.
        assert_eq!(
            plan.set_scene_kind(0, SceneKind::Drawn, None, &both, later),
            Ok(KindChange::Drawn {
                needs_image: false,
                dropped: Some(shot(both.at(1).unwrap(), "shot.png")),
            })
        );
        assert_eq!(plan.scenes()[0].picture(), Some("scene-0.png"));
        assert_eq!(plan.scenes()[0].shot(), None);
        assert_eq!(plan.scenes()[0].failure(), None);

        // A scene that was never drawn needs a drawing, which is the
        // user's to ask for.
        plan.set_scene_kind(1, SceneKind::ProductShot, None, &both, later)
            .unwrap();
        assert_eq!(
            plan.set_scene_kind(1, SceneKind::Drawn, None, &both, later),
            Ok(KindChange::Drawn {
                needs_image: true,
                dropped: None,
            })
        );
        assert_eq!(plan.missing_images(), [1, 2]);

        // What is not allowed changes nothing.
        let before = plan.clone();
        let screens = pictures(&[Screen]);
        assert_eq!(
            plan.set_scene_kind(1, SceneKind::ProductInUse, None, &screens, later),
            Err(SceneKindError::NotAllowed(SceneKind::ProductInUse))
        );
        assert_eq!(
            plan.set_scene_kind(
                1,
                SceneKind::ProductShot,
                None,
                &ProductPictures::default(),
                later
            ),
            Err(SceneKindError::NotAllowed(SceneKind::ProductShot))
        );
        assert_eq!(
            plan.set_scene_kind(1, SceneKind::EndCard, None, &both, later),
            Err(SceneKindError::NotLast)
        );
        assert_eq!(
            plan.set_scene_kind(
                1,
                SceneKind::ProductShot,
                Some(ProductImageId::new()),
                &both,
                later
            ),
            Err(SceneKindError::NoSuchImage)
        );
        assert_eq!(
            plan.set_scene_kind(9, SceneKind::Drawn, None, &both, later),
            Err(SceneKindError::NoSuchScene(9))
        );
        assert_eq!(plan, before);

        // The last scene can be the end card: composed, with the
        // product's image when it has one and as a card of text when not.
        assert_eq!(
            plan.set_scene_kind(2, SceneKind::EndCard, None, &both, later),
            Ok(KindChange::Compose)
        );
        let mut bare = ad_plan(
            &n,
            &ProductPictures::default(),
            vec![kinded(0, "drawn", None)],
        );
        assert_eq!(
            bare.set_scene_kind(
                0,
                SceneKind::EndCard,
                None,
                &ProductPictures::default(),
                later
            ),
            Ok(KindChange::Compose)
        );
        assert!(bare.scene(0).unwrap().is_composed());
        assert_eq!(bare.scene(0).unwrap().product_image(), None);
    }

    #[test]
    fn a_fitted_plan_keeps_its_kinds_and_composed_pictures() {
        let n = narration(STORY, 0);
        let photo = pictures(&[crate::ProductImageKind::Photo]);
        let mut plan = ad_plan(
            &n,
            &photo,
            vec![kinded(0, "drawn", None), kinded(1, "product_shot", None)],
        );
        plan.scene_mut(1, SystemTime::UNIX_EPOCH)
            .unwrap()
            .add_shot(shot(photo.first().unwrap(), "shot.png"));
        let again = Narration {
            id: NarrationId::new(),
            ..narration(STORY, 500)
        };
        let fitted = plan.fitted_to(&again, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(kinds(&fitted), kinds(&plan));
        assert_eq!(fitted.scenes()[1].picture(), Some("shot.png"));
    }
}
