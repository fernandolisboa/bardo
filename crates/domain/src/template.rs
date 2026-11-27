//! Templates (CONTEXT.md): versioned, editable prompts that generation
//! fills with a project's facts (PRD story 45). A template's text holds
//! `{{variable}}` placeholders; each kind of template has its own fixed set
//! of variables. Editing never changes a saved version: saving creates the
//! next one, so every generation can name the exact text it used.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

use crate::{ProfileId, ProjectId, RepositoryError, VideoProjectId};

uuid_id!(
    /// Identifies one saved version of a template.
    TemplateVersionId
);

/// What a template generates. Titles, descriptions and the other media
/// prompts reuse the same mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateKind {
    /// The narration script of a video project.
    Script,
    /// The scene plan of a narrated video: where each scene starts and the
    /// image prompt for each one.
    ImagePrompt,
    /// The prompt for a video's music, which the user takes to the music
    /// tool of their choice.
    MusicPrompt,
    /// Each network's title, description and tags for a video.
    Metadata,
    /// The text of a long video's YouTube thumbnail and the prompt its
    /// background is drawn from (#182).
    Thumbnail,
    /// Where a long video's YouTube chapters start and their titles (#182).
    Chapters,
    /// A video's script adapted, sentence by sentence, for another
    /// channel's language and country (#184).
    Localization,
    /// The prompt a persona's look is drawn from, written from the user's
    /// description (#188).
    LookPrompt,
    /// The prompt a take model makes a take from: the look, the set, the
    /// take's words and the model's instruction (#192).
    TakePrompt,
    /// The ad angles Claude proposes for a product and a channel, from the
    /// product sheet's confirmed claims and the ad frameworks (#275).
    AdAngles,
    /// The narration script of an ad project, written from its brief:
    /// the product, the approved angle and the claims the video uses
    /// (#276). Every other project is written from `Script`.
    AdScript,
    /// The scene plan of an ad project (#278): the storyboard plan, with
    /// what each scene is (drawn, the product in use, a product shot or
    /// the end card) and which product image a product shot shows. Every
    /// other project is planned from `ImagePrompt`.
    AdScenePlan,
    /// The prompt a concept image is drawn from, written from the brief of
    /// a product of kind *idea* (#280).
    ConceptPrompt,
    /// Each network's title, caption and tags for an ad project (#282):
    /// opened by the hook, from the Brief stage's claims only, ending on
    /// the call to action. Every other project's come from `Metadata`.
    AdMetadata,
}

impl TemplateKind {
    pub const ALL: [TemplateKind; 14] = [
        TemplateKind::Script,
        TemplateKind::ImagePrompt,
        TemplateKind::MusicPrompt,
        TemplateKind::Metadata,
        TemplateKind::Thumbnail,
        TemplateKind::Chapters,
        TemplateKind::Localization,
        TemplateKind::LookPrompt,
        TemplateKind::TakePrompt,
        TemplateKind::AdAngles,
        TemplateKind::AdScript,
        TemplateKind::AdScenePlan,
        TemplateKind::ConceptPrompt,
        TemplateKind::AdMetadata,
    ];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            TemplateKind::Script => "script",
            TemplateKind::ImagePrompt => "image_prompt",
            TemplateKind::MusicPrompt => "music_prompt",
            TemplateKind::Metadata => "metadata",
            TemplateKind::Thumbnail => "thumbnail",
            TemplateKind::Chapters => "chapters",
            TemplateKind::Localization => "localization",
            TemplateKind::LookPrompt => "look_prompt",
            TemplateKind::TakePrompt => "take_prompt",
            TemplateKind::AdAngles => "ad_angles",
            TemplateKind::AdScript => "ad_script",
            TemplateKind::AdScenePlan => "ad_scene_plan",
            TemplateKind::ConceptPrompt => "concept_prompt",
            TemplateKind::AdMetadata => "ad_metadata",
        }
    }

    /// The variables its text may use, in the order the editor lists them.
    pub fn variables(self) -> &'static [TemplateVariable] {
        match self {
            TemplateKind::Script => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::ChannelThemes,
                TemplateVariable::AestheticNotes,
                TemplateVariable::Language,
                TemplateVariable::Country,
                TemplateVariable::Persona,
                TemplateVariable::Niche,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::TargetLength,
            ],
            TemplateKind::ImagePrompt => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::AestheticNotes,
                TemplateVariable::VisualStyle,
                TemplateVariable::Language,
                TemplateVariable::Niche,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::NarrationSentences,
                TemplateVariable::FrameShape,
                TemplateVariable::FixedCast,
                TemplateVariable::FixedPlaces,
                TemplateVariable::FixedObjects,
            ],
            TemplateKind::MusicPrompt => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::AestheticNotes,
                TemplateVariable::Country,
                TemplateVariable::Niche,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::VideoLength,
            ],
            TemplateKind::Metadata => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::Language,
                TemplateVariable::Country,
                TemplateVariable::Niche,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::Script,
                TemplateVariable::Networks,
            ],
            TemplateKind::Thumbnail => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::AestheticNotes,
                TemplateVariable::VisualStyle,
                TemplateVariable::Language,
                TemplateVariable::Niche,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::VideoTitle,
            ],
            TemplateKind::Chapters => &[
                TemplateVariable::Language,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::VideoLength,
                TemplateVariable::NarrationSentences,
            ],
            TemplateKind::Localization => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::Language,
                TemplateVariable::Country,
                TemplateVariable::Persona,
                TemplateVariable::SourceLanguage,
                TemplateVariable::SourceCountry,
                TemplateVariable::ThemeTitle,
                TemplateVariable::NarrationSentences,
            ],
            TemplateKind::LookPrompt => {
                &[TemplateVariable::Persona, TemplateVariable::LookDescription]
            }
            TemplateKind::TakePrompt => &[
                TemplateVariable::Look,
                TemplateVariable::Set,
                TemplateVariable::Words,
                TemplateVariable::Instruction,
            ],
            TemplateKind::AdAngles => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::Language,
                TemplateVariable::Country,
                TemplateVariable::Persona,
                TemplateVariable::Product,
                TemplateVariable::ProductAudience,
                TemplateVariable::ProductClaims,
                TemplateVariable::ProductNotes,
                TemplateVariable::AdFrameworks,
                TemplateVariable::AnglesToAvoid,
                TemplateVariable::AngleCount,
            ],
            TemplateKind::AdScript => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::Language,
                TemplateVariable::Country,
                TemplateVariable::Persona,
                TemplateVariable::Product,
                TemplateVariable::ProductConcept,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::AdSegment,
                TemplateVariable::AdHook,
                TemplateVariable::AdFramework,
                TemplateVariable::BriefClaims,
                TemplateVariable::Offer,
                TemplateVariable::CallToAction,
                TemplateVariable::MustSay,
                TemplateVariable::NeverSay,
                TemplateVariable::TargetLength,
            ],
            TemplateKind::AdScenePlan => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::AestheticNotes,
                TemplateVariable::VisualStyle,
                TemplateVariable::Language,
                TemplateVariable::Product,
                TemplateVariable::ProductConcept,
                TemplateVariable::ProductImages,
                TemplateVariable::SceneKinds,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::CallToAction,
                TemplateVariable::NarrationSentences,
                TemplateVariable::FrameShape,
                TemplateVariable::FixedCast,
                TemplateVariable::FixedPlaces,
                TemplateVariable::FixedObjects,
            ],
            TemplateKind::ConceptPrompt => {
                &[TemplateVariable::Product, TemplateVariable::ProductAudience]
            }
            TemplateKind::AdMetadata => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::Language,
                TemplateVariable::Country,
                TemplateVariable::Product,
                TemplateVariable::ProductConcept,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::AdHook,
                TemplateVariable::BriefClaims,
                TemplateVariable::Offer,
                TemplateVariable::CallToAction,
                TemplateVariable::Link,
                TemplateVariable::Script,
                TemplateVariable::Networks,
            ],
        }
    }
}

impl fmt::Display for TemplateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown template kind: {0}")]
pub struct UnknownTemplateKind(pub String);

impl std::str::FromStr for TemplateKind {
    type Err = UnknownTemplateKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TemplateKind::ALL
            .into_iter()
            .find(|kind| kind.code() == s)
            .ok_or_else(|| UnknownTemplateKind(s.to_owned()))
    }
}

/// A fact a template can place in its text, written `{{name}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateVariable {
    ChannelName,
    /// The channel's own niche.
    ChannelNiche,
    /// The channel's recurring themes.
    ChannelThemes,
    AestheticNotes,
    /// The language the video is made in.
    Language,
    /// The country of the audience.
    Country,
    /// Who narrates: voice, tone and script style.
    Persona,
    /// The niche the video project belongs to.
    Niche,
    /// The video's working title.
    ThemeTitle,
    /// The approved theme's angle.
    ThemeAngle,
    /// The narration, one numbered sentence per line with when it is
    /// spoken.
    NarrationSentences,
    /// How long the video runs: its narration's length.
    VideoLength,
    /// The video's script as the user left it.
    Script,
    /// The networks to write for, one per line: each one's fields and
    /// limits, its language and the account's default tags.
    Networks,
    /// How long the script runs, from the video's format (Short or Long).
    TargetLength,
    /// The shape of the pictures, from the video's format.
    FrameShape,
    /// The video's title on YouTube, as its metadata has it (the working
    /// title until there is one).
    VideoTitle,
    /// The language a localized version is adapted from.
    SourceLanguage,
    /// The audience country a localized version is adapted from.
    SourceCountry,
    /// What the user wrote about the character a persona's look shows.
    LookDescription,
    /// Who the take's persona looks like: its look's description.
    Look,
    /// Where the take is performed: the room as one frame shows it, or the
    /// set image.
    Set,
    /// What the take says.
    Words,
    /// What the take model must do with its inputs, fixed per model.
    Instruction,
    /// The video's visual style (#259): that one was chosen and the text
    /// Bardo adds to every image for it, or that none was chosen.
    VisualStyle,
    /// The product an ad is for: its name, what kind of thing it is, what
    /// it is and what its sheet says of it.
    Product,
    /// Who the product is for: the brief's audience and the sheet's
    /// audience segments.
    ProductAudience,
    /// The claims an ad may state: the sheet's confirmed claims, numbered.
    ProductClaims,
    /// What an ad for the product must say and must never say.
    ProductNotes,
    /// The ad frameworks to pick from: each one's code, name and steps.
    AdFrameworks,
    /// The ad angles already proposed for the product and the channel.
    AnglesToAvoid,
    /// How many ad angles to propose.
    AngleCount,
    /// Whether the product of an ad is a concept: an idea that does not
    /// exist yet.
    ProductConcept,
    /// The part of the audience an ad speaks to, from its angle.
    AdSegment,
    /// A draft of an ad's first words, from its angle.
    AdHook,
    /// The framework an ad follows: its name and its steps in order.
    AdFramework,
    /// The claims an ad's video uses, numbered: the Brief stage's own
    /// copies.
    BriefClaims,
    /// What the viewer of an ad gets, as its Brief stage has it.
    Offer,
    /// What the viewer of an ad is asked to do, in the user's words.
    CallToAction,
    /// What every ad for the product must say.
    MustSay,
    /// What no ad for the product may say.
    NeverSay,
    /// The images of an ad's product, numbered from 0: what each one is
    /// (a photo or a screen) and its shape.
    ProductImages,
    /// The kinds the scenes of an ad may be, by what its product's images
    /// allow.
    SceneKinds,
    /// The link or handle an ad sends viewers to, as its Brief stage has
    /// it.
    Link,
    /// The characters the video picked from the world (#337), the fixed
    /// cast the scene planner must use as given: each one's name, fixed
    /// appearance, always and never notes and the video's instruction.
    FixedCast,
    /// The places the video picked from the world (#338): each one's name,
    /// fixed description, notes and the video's instruction.
    FixedPlaces,
    /// The objects the video picked from the world (#338), numbered as a
    /// scene's `objects` names them, with the character each belongs to.
    FixedObjects,
}

impl TemplateVariable {
    pub const ALL: [TemplateVariable; 47] = [
        TemplateVariable::ChannelName,
        TemplateVariable::ChannelNiche,
        TemplateVariable::ChannelThemes,
        TemplateVariable::AestheticNotes,
        TemplateVariable::Language,
        TemplateVariable::Country,
        TemplateVariable::Persona,
        TemplateVariable::Niche,
        TemplateVariable::ThemeTitle,
        TemplateVariable::ThemeAngle,
        TemplateVariable::NarrationSentences,
        TemplateVariable::VideoLength,
        TemplateVariable::Script,
        TemplateVariable::Networks,
        TemplateVariable::TargetLength,
        TemplateVariable::FrameShape,
        TemplateVariable::VideoTitle,
        TemplateVariable::SourceLanguage,
        TemplateVariable::SourceCountry,
        TemplateVariable::LookDescription,
        TemplateVariable::Look,
        TemplateVariable::Set,
        TemplateVariable::Words,
        TemplateVariable::Instruction,
        TemplateVariable::VisualStyle,
        TemplateVariable::Product,
        TemplateVariable::ProductAudience,
        TemplateVariable::ProductClaims,
        TemplateVariable::ProductNotes,
        TemplateVariable::AdFrameworks,
        TemplateVariable::AnglesToAvoid,
        TemplateVariable::AngleCount,
        TemplateVariable::ProductConcept,
        TemplateVariable::AdSegment,
        TemplateVariable::AdHook,
        TemplateVariable::AdFramework,
        TemplateVariable::BriefClaims,
        TemplateVariable::Offer,
        TemplateVariable::CallToAction,
        TemplateVariable::MustSay,
        TemplateVariable::NeverSay,
        TemplateVariable::ProductImages,
        TemplateVariable::SceneKinds,
        TemplateVariable::Link,
        TemplateVariable::FixedCast,
        TemplateVariable::FixedPlaces,
        TemplateVariable::FixedObjects,
    ];

    /// The name written between the braces.
    pub fn name(self) -> &'static str {
        match self {
            TemplateVariable::ChannelName => "channel_name",
            TemplateVariable::ChannelNiche => "channel_niche",
            TemplateVariable::ChannelThemes => "channel_themes",
            TemplateVariable::AestheticNotes => "aesthetic_notes",
            TemplateVariable::Language => "language",
            TemplateVariable::Country => "country",
            TemplateVariable::Persona => "persona",
            TemplateVariable::Niche => "niche",
            TemplateVariable::ThemeTitle => "theme_title",
            TemplateVariable::ThemeAngle => "theme_angle",
            TemplateVariable::NarrationSentences => "narration_sentences",
            TemplateVariable::VideoLength => "video_length",
            TemplateVariable::Script => "script",
            TemplateVariable::Networks => "networks",
            TemplateVariable::TargetLength => "target_length",
            TemplateVariable::FrameShape => "frame_shape",
            TemplateVariable::VideoTitle => "video_title",
            TemplateVariable::SourceLanguage => "source_language",
            TemplateVariable::SourceCountry => "source_country",
            TemplateVariable::LookDescription => "look_description",
            TemplateVariable::Look => "look",
            TemplateVariable::Set => "set",
            TemplateVariable::Words => "words",
            TemplateVariable::Instruction => "instruction",
            TemplateVariable::VisualStyle => "visual_style",
            TemplateVariable::Product => "product",
            TemplateVariable::ProductAudience => "product_audience",
            TemplateVariable::ProductClaims => "product_claims",
            TemplateVariable::ProductNotes => "product_notes",
            TemplateVariable::AdFrameworks => "ad_frameworks",
            TemplateVariable::AnglesToAvoid => "angles_to_avoid",
            TemplateVariable::AngleCount => "angle_count",
            TemplateVariable::ProductConcept => "product_concept",
            TemplateVariable::AdSegment => "ad_segment",
            TemplateVariable::AdHook => "ad_hook",
            TemplateVariable::AdFramework => "ad_framework",
            TemplateVariable::BriefClaims => "brief_claims",
            TemplateVariable::Offer => "offer",
            TemplateVariable::CallToAction => "call_to_action",
            TemplateVariable::MustSay => "must_say",
            TemplateVariable::NeverSay => "never_say",
            TemplateVariable::ProductImages => "product_images",
            TemplateVariable::SceneKinds => "scene_kinds",
            TemplateVariable::Link => "link",
            TemplateVariable::FixedCast => "fixed_cast",
            TemplateVariable::FixedPlaces => "fixed_places",
            TemplateVariable::FixedObjects => "fixed_objects",
        }
    }

    /// The placeholder as written in a template: `{{name}}`.
    pub fn placeholder(self) -> String {
        format!("{{{{{}}}}}", self.name())
    }
}

/// The two texts of a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateField {
    /// Standing instructions: role, rules, tone.
    Instructions,
    /// The task, with the project's facts.
    Prompt,
}

/// What is wrong with one field's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateProblem {
    Required,
    TooLong,
    /// A `{{name}}` that is not a variable of the template's kind.
    UnknownVariable,
    /// A `{{` with no `}}` after it.
    UnclosedVariable,
}

impl TemplateProblem {
    pub const ALL: [TemplateProblem; 4] = [
        TemplateProblem::Required,
        TemplateProblem::TooLong,
        TemplateProblem::UnknownVariable,
        TemplateProblem::UnclosedVariable,
    ];
}

/// Why typed template text is not valid. One entry per field and problem,
/// so a form can show each under its field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TemplateFieldError {
    pub field: TemplateField,
    pub problem: TemplateProblem,
}

/// A template text split into literal text and variables.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Text(String),
    Variable(TemplateVariable),
}

fn parse(kind: TemplateKind, text: &str) -> Result<Vec<Segment>, TemplateProblem> {
    let mut segments = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        if open > 0 {
            segments.push(Segment::Text(rest[..open].to_owned()));
        }
        let after = &rest[open + 2..];
        let close = after.find("}}").ok_or(TemplateProblem::UnclosedVariable)?;
        let name = after[..close].trim();
        let variable = kind
            .variables()
            .iter()
            .find(|variable| variable.name() == name)
            .ok_or(TemplateProblem::UnknownVariable)?;
        segments.push(Segment::Variable(*variable));
        rest = &after[close + 2..];
    }
    if !rest.is_empty() {
        segments.push(Segment::Text(rest.to_owned()));
    }
    Ok(segments)
}

/// The values to fill a template with. A value is placed as written: a
/// value that looks like a placeholder is not expanded again.
pub type TemplateValues = HashMap<TemplateVariable, String>;

/// A template filled in: what is sent to the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPrompt {
    pub instructions: String,
    pub prompt: String,
}

/// Rendering needs a value for every variable the text uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no value for {{{{{}}}}}", .0.name())]
pub struct MissingValue(pub TemplateVariable);

/// A template's text, valid for its kind: the prompt is present, both
/// fields are within limits and use only the kind's variables. Ends are
/// trimmed; the text inside is kept as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateBody {
    kind: TemplateKind,
    instructions: String,
    prompt: String,
}

impl TemplateBody {
    /// Limits are in characters, not bytes, so accents count once.
    pub const MAX_CHARS: usize = 8_000;

    pub fn new(
        kind: TemplateKind,
        instructions: &str,
        prompt: &str,
    ) -> Result<Self, Vec<TemplateFieldError>> {
        let instructions = instructions.trim();
        let prompt = prompt.trim();
        let mut errors = Vec::new();
        for (field, text) in [
            (TemplateField::Instructions, instructions),
            (TemplateField::Prompt, prompt),
        ] {
            let problem = if text.is_empty() && field == TemplateField::Prompt {
                Some(TemplateProblem::Required)
            } else if text.chars().count() > Self::MAX_CHARS {
                Some(TemplateProblem::TooLong)
            } else {
                parse(kind, text).err()
            };
            if let Some(problem) = problem {
                errors.push(TemplateFieldError { field, problem });
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            kind,
            instructions: instructions.to_owned(),
            prompt: prompt.to_owned(),
        })
    }

    pub fn kind(&self) -> TemplateKind {
        self.kind
    }

    /// May be empty.
    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// Whether the instructions or the prompt place `variable`.
    pub fn uses(&self, variable: TemplateVariable) -> bool {
        [&self.instructions, &self.prompt].iter().any(|text| {
            // The text was validated, so it parses.
            parse(self.kind, text)
                .unwrap_or_default()
                .iter()
                .any(|segment| matches!(segment, Segment::Variable(used) if *used == variable))
        })
    }

    /// Places each value in its placeholder.
    pub fn render(&self, values: &TemplateValues) -> Result<RenderedPrompt, MissingValue> {
        let fill = |text: &str| -> Result<String, MissingValue> {
            // The text was validated, so it parses.
            let segments = parse(self.kind, text).unwrap_or_default();
            let mut out = String::with_capacity(text.len());
            for segment in segments {
                match segment {
                    Segment::Text(text) => out.push_str(&text),
                    Segment::Variable(variable) => out.push_str(
                        values
                            .get(&variable)
                            .ok_or(MissingValue(variable))?
                            .as_str(),
                    ),
                }
            }
            Ok(out)
        };
        Ok(RenderedPrompt {
            instructions: fill(&self.instructions)?,
            prompt: fill(&self.prompt)?,
        })
    }
}

/// One saved version of a template. Never changes once saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateVersion {
    pub id: TemplateVersionId,
    pub owner: ProfileId,
    /// The project whose own template it is, or `None` for the global one.
    pub owner_project: Option<ProjectId>,
    /// 1 for the first version, then one more per save.
    pub number: u32,
    pub body: TemplateBody,
    pub created_at: SystemTime,
}

impl TemplateVersion {
    /// The first version of the owner's global template.
    pub fn first(owner: ProfileId, body: TemplateBody, now: SystemTime) -> Self {
        Self {
            id: TemplateVersionId::new(),
            owner,
            owner_project: None,
            number: 1,
            body,
            created_at: now,
        }
    }

    /// The first version of `project`'s own template: a copy of `from`'s
    /// text (the global template's latest version), numbered 1.
    pub fn project_copy(from: &TemplateVersion, project: ProjectId, now: SystemTime) -> Self {
        Self {
            id: TemplateVersionId::new(),
            owner: from.owner,
            owner_project: Some(project),
            number: 1,
            body: from.body.clone(),
            created_at: now,
        }
    }

    pub fn kind(&self) -> TemplateKind {
        self.body.kind()
    }

    /// The next version with `body`, or `None` when the text did not
    /// change (saving it again adds nothing to the history).
    pub fn revise(&self, body: TemplateBody, now: SystemTime) -> Option<Self> {
        (body != self.body).then(|| Self {
            id: TemplateVersionId::new(),
            owner: self.owner,
            owner_project: self.owner_project,
            number: self.number + 1,
            body,
            created_at: now,
        })
    }
}

/// Whose template of a kind a project uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateSource {
    /// The project's own, which wins inside the project.
    Project,
    /// The global template, which every project without its own uses.
    Global,
}

/// The versions of the template a project uses for one kind: its own when
/// it has one (`project`, oldest first), else the global one's. Generations
/// keep pointing at the version they used, so neither list is ever cut.
pub fn template_in_use(
    project: Vec<TemplateVersion>,
    global: Vec<TemplateVersion>,
) -> (TemplateSource, Vec<TemplateVersion>) {
    if project.is_empty() {
        (TemplateSource::Global, global)
    } else {
        (TemplateSource::Project, project)
    }
}

/// Persistence port for template versions. Shared with job worker threads.
pub trait TemplateRepository: Send + Sync {
    /// Every version of the template of `kind` that `project` owns, or of
    /// the global one for `None`, oldest first.
    fn template_versions(
        &self,
        owner: ProfileId,
        project: Option<ProjectId>,
        kind: TemplateKind,
    ) -> Result<Vec<TemplateVersion>, RepositoryError>;

    /// The projects that have their own template of `kind`.
    fn projects_with_template(
        &self,
        owner: ProfileId,
        kind: TemplateKind,
    ) -> Result<Vec<ProjectId>, RepositoryError>;

    /// The videos with a generation made from one of `project`'s own
    /// templates.
    fn videos_using_templates_of(
        &self,
        project: ProjectId,
    ) -> Result<Vec<VideoProjectId>, RepositoryError>;

    fn template_version(
        &self,
        id: TemplateVersionId,
    ) -> Result<Option<TemplateVersion>, RepositoryError>;

    /// Adds a version. Fails when its number is already taken, so two
    /// saves racing cannot both become the same version.
    fn add_template_version(&self, version: &TemplateVersion) -> Result<(), RepositoryError>;
}

impl<T: TemplateRepository + ?Sized> TemplateRepository for Arc<T> {
    fn template_versions(
        &self,
        owner: ProfileId,
        project: Option<ProjectId>,
        kind: TemplateKind,
    ) -> Result<Vec<TemplateVersion>, RepositoryError> {
        (**self).template_versions(owner, project, kind)
    }

    fn projects_with_template(
        &self,
        owner: ProfileId,
        kind: TemplateKind,
    ) -> Result<Vec<ProjectId>, RepositoryError> {
        (**self).projects_with_template(owner, kind)
    }

    fn videos_using_templates_of(
        &self,
        project: ProjectId,
    ) -> Result<Vec<VideoProjectId>, RepositoryError> {
        (**self).videos_using_templates_of(project)
    }

    fn template_version(
        &self,
        id: TemplateVersionId,
    ) -> Result<Option<TemplateVersion>, RepositoryError> {
        (**self).template_version(id)
    }

    fn add_template_version(&self, version: &TemplateVersion) -> Result<(), RepositoryError> {
        (**self).add_template_version(version)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    fn body(instructions: &str, prompt: &str) -> TemplateBody {
        TemplateBody::new(TemplateKind::Script, instructions, prompt).unwrap()
    }

    fn values(pairs: &[(TemplateVariable, &str)]) -> TemplateValues {
        pairs
            .iter()
            .map(|(variable, value)| (*variable, (*value).to_owned()))
            .collect()
    }

    fn errors(instructions: &str, prompt: &str) -> Vec<TemplateFieldError> {
        TemplateBody::new(TemplateKind::Script, instructions, prompt).unwrap_err()
    }

    fn error(field: TemplateField, problem: TemplateProblem) -> TemplateFieldError {
        TemplateFieldError { field, problem }
    }

    #[test]
    fn rendering_places_each_value_in_its_placeholders() {
        let template = body(
            "Write in {{language}}.",
            "Title: {{theme_title}}\nAgain: {{ theme_title }}. Niche: {{niche}}",
        );
        let rendered = template
            .render(&values(&[
                (TemplateVariable::Language, "Portuguese"),
                (TemplateVariable::ThemeTitle, "The lost probe"),
                (TemplateVariable::Niche, "space history"),
            ]))
            .unwrap();
        assert_eq!(rendered.instructions, "Write in Portuguese.");
        assert_eq!(
            rendered.prompt,
            "Title: The lost probe\nAgain: The lost probe. Niche: space history"
        );
    }

    #[test]
    fn a_body_tells_which_variables_it_places() {
        let body = body("Write in {{ language }}.", "About {{theme_title}}.");
        assert!(body.uses(TemplateVariable::Language));
        assert!(body.uses(TemplateVariable::ThemeTitle));
        assert!(!body.uses(TemplateVariable::Persona));
    }

    #[test]
    fn rendering_keeps_single_braces_and_does_not_expand_values() {
        let template = body("", "JSON like {\"a\": 1} and {{persona}}}");
        let rendered = template
            .render(&values(&[(TemplateVariable::Persona, "{{niche}}")]))
            .unwrap();
        assert_eq!(rendered.instructions, "");
        assert_eq!(rendered.prompt, "JSON like {\"a\": 1} and {{niche}}}");
    }

    #[test]
    fn rendering_needs_every_value_the_text_uses() {
        let template = body("", "About {{niche}}.");
        assert_eq!(
            template.render(&TemplateValues::new()),
            Err(MissingValue(TemplateVariable::Niche))
        );
        assert!(
            template
                .render(&values(&[(TemplateVariable::Niche, "")]))
                .is_ok(),
            "an empty value is still a value"
        );
    }

    #[test]
    fn text_is_trimmed_and_the_prompt_is_required() {
        let template = body("  Rules.\n", "\n Task. ");
        assert_eq!(template.instructions(), "Rules.");
        assert_eq!(template.prompt(), "Task.");
        assert!(TemplateBody::new(TemplateKind::Script, "", "Task").is_ok());
        assert_eq!(
            errors("Rules.", "  "),
            [error(TemplateField::Prompt, TemplateProblem::Required)]
        );
    }

    #[test]
    fn only_the_kinds_variables_are_accepted() {
        assert_eq!(
            errors("Use {{voice}}.", "{{niche}} and {{ NICHE }}"),
            [
                error(
                    TemplateField::Instructions,
                    TemplateProblem::UnknownVariable
                ),
                error(TemplateField::Prompt, TemplateProblem::UnknownVariable),
            ]
        );
        for variable in TemplateKind::Script.variables() {
            assert!(
                TemplateBody::new(TemplateKind::Script, "", &variable.placeholder()).is_ok(),
                "{variable:?}"
            );
        }
    }

    #[test]
    fn an_unclosed_placeholder_is_rejected() {
        assert_eq!(
            errors("", "About {{niche} and more"),
            [error(
                TemplateField::Prompt,
                TemplateProblem::UnclosedVariable
            )]
        );
    }

    #[test]
    fn limits_count_characters() {
        let longest = "é".repeat(TemplateBody::MAX_CHARS);
        assert!(TemplateBody::new(TemplateKind::Script, &longest, &longest).is_ok());
        assert_eq!(
            errors(&format!("{longest}é"), &format!("{longest}é")),
            [
                error(TemplateField::Instructions, TemplateProblem::TooLong),
                error(TemplateField::Prompt, TemplateProblem::TooLong),
            ]
        );
    }

    #[test]
    fn every_variable_has_a_distinct_name_and_kind_codes_round_trip() {
        let mut names: Vec<_> = TemplateVariable::ALL.iter().map(|v| v.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TemplateVariable::ALL.len());
        assert_eq!(
            TemplateVariable::ThemeTitle.placeholder(),
            "{{theme_title}}"
        );
        for kind in TemplateKind::ALL {
            assert_eq!(kind.code().parse::<TemplateKind>(), Ok(kind));
        }
        assert!("title".parse::<TemplateKind>().is_err());
    }

    #[test]
    fn each_kind_accepts_only_its_own_variables() {
        assert!(
            TemplateBody::new(TemplateKind::ImagePrompt, "", "{{narration_sentences}}").is_ok()
        );
        assert_eq!(
            TemplateBody::new(TemplateKind::Script, "", "{{narration_sentences}}").unwrap_err(),
            [error(
                TemplateField::Prompt,
                TemplateProblem::UnknownVariable
            )]
        );
        assert_eq!(
            TemplateBody::new(TemplateKind::ImagePrompt, "", "{{persona}}").unwrap_err(),
            [error(
                TemplateField::Prompt,
                TemplateProblem::UnknownVariable
            )]
        );
    }

    #[test]
    fn saving_creates_the_next_version_and_keeps_the_old_one() {
        let owner = ProfileId::new();
        let first = TemplateVersion::first(owner, body("A", "B"), at(0));
        assert_eq!(first.number, 1);

        let second = first.revise(body("A", "B, sharper"), at(5)).unwrap();
        assert_eq!(second.number, 2);
        assert_ne!(second.id, first.id);
        assert_eq!(second.owner, owner);
        assert_eq!(second.created_at, at(5));
        assert_eq!(first.body.prompt(), "B", "the old version is untouched");
        assert_eq!(second.kind(), TemplateKind::Script);
    }

    #[test]
    fn saving_the_same_text_adds_no_version() {
        let first = TemplateVersion::first(ProfileId::new(), body("A", "B"), at(0));
        assert_eq!(first.revise(body(" A ", "B\n"), at(5)), None);
    }

    fn version_of(
        kind: TemplateKind,
        project: Option<ProjectId>,
        number: u32,
        text: &str,
    ) -> TemplateVersion {
        TemplateVersion {
            number,
            owner_project: project,
            ..TemplateVersion::first(
                ProfileId::new(),
                TemplateBody::new(kind, text, text).unwrap(),
                at(u64::from(number)),
            )
        }
    }

    #[test]
    fn a_project_uses_its_own_template_of_a_kind_else_the_global_one() {
        assert!(TemplateKind::ALL.contains(&TemplateKind::AdAngles));
        assert!(TemplateKind::ALL.contains(&TemplateKind::AdScript));
        assert!(TemplateKind::ALL.contains(&TemplateKind::AdMetadata));
        let project = ProjectId::new();
        for kind in TemplateKind::ALL {
            let global = vec![
                version_of(kind, None, 1, "global one"),
                version_of(kind, None, 2, "global two"),
            ];

            let (source, versions) = template_in_use(Vec::new(), global.clone());
            assert_eq!(source, TemplateSource::Global, "{kind:?} without its own");
            assert_eq!(versions, global, "{kind:?}");

            let own = vec![version_of(kind, Some(project), 1, "own")];
            let (source, versions) = template_in_use(own.clone(), global);
            assert_eq!(source, TemplateSource::Project, "{kind:?} with its own");
            assert_eq!(versions, own, "{kind:?}: the project's own wins");
        }
    }

    #[test]
    fn a_project_template_starts_as_a_copy_of_the_latest_global_version() {
        let project = ProjectId::new();
        let global = version_of(TemplateKind::AdScript, None, 4, "latest");
        let copy = TemplateVersion::project_copy(&global, project, at(9));
        assert_eq!(copy.number, 1, "its own history starts at 1");
        assert_eq!(copy.owner_project, Some(project));
        assert_eq!(copy.body, global.body);
        assert_ne!(copy.id, global.id, "generations keep the global version");

        let next = copy
            .revise(
                TemplateBody::new(TemplateKind::AdScript, "changed", "changed").unwrap(),
                at(10),
            )
            .unwrap();
        assert_eq!(
            next.owner_project,
            Some(project),
            "a revision stays the project's"
        );
        assert_eq!(next.number, 2);
    }
}
