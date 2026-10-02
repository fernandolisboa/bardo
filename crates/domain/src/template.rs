//! Templates (CONTEXT.md): versioned, editable prompts that generation
//! fills with a project's facts (PRD story 45). A template's text holds
//! `{{variable}}` placeholders; each kind of template has its own fixed set
//! of variables. Editing never changes a saved version: saving creates the
//! next one, so every generation can name the exact text it used.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

use crate::{ProfileId, RepositoryError};

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
}

impl TemplateKind {
    pub const ALL: [TemplateKind; 3] = [
        TemplateKind::Script,
        TemplateKind::ImagePrompt,
        TemplateKind::MusicPrompt,
    ];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            TemplateKind::Script => "script",
            TemplateKind::ImagePrompt => "image_prompt",
            TemplateKind::MusicPrompt => "music_prompt",
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
            ],
            TemplateKind::ImagePrompt => &[
                TemplateVariable::ChannelName,
                TemplateVariable::ChannelNiche,
                TemplateVariable::AestheticNotes,
                TemplateVariable::Language,
                TemplateVariable::Niche,
                TemplateVariable::ThemeTitle,
                TemplateVariable::ThemeAngle,
                TemplateVariable::NarrationSentences,
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
}

impl TemplateVariable {
    pub const ALL: [TemplateVariable; 12] = [
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
    /// 1 for the first version, then one more per save.
    pub number: u32,
    pub body: TemplateBody,
    pub created_at: SystemTime,
}

impl TemplateVersion {
    /// The first version of the owner's template.
    pub fn first(owner: ProfileId, body: TemplateBody, now: SystemTime) -> Self {
        Self {
            id: TemplateVersionId::new(),
            owner,
            number: 1,
            body,
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
            number: self.number + 1,
            body,
            created_at: now,
        })
    }
}

/// Persistence port for template versions. Shared with job worker threads.
pub trait TemplateRepository: Send + Sync {
    /// Every version of the owner's template of `kind`, oldest first.
    fn template_versions(
        &self,
        owner: ProfileId,
        kind: TemplateKind,
    ) -> Result<Vec<TemplateVersion>, RepositoryError>;

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
        kind: TemplateKind,
    ) -> Result<Vec<TemplateVersion>, RepositoryError> {
        (**self).template_versions(owner, kind)
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
}
