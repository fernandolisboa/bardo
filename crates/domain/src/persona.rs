//! Personas (ADR-0005, CONTEXT.md): reusable narrator identities owned by
//! the user profile, independent of channels. A persona holds a voice
//! reference, a tone, a script style and generation presets; a channel
//! points at one as its default.

use std::sync::Arc;

use crate::{ProfileId, RepositoryError, VoiceRef};

uuid_id!(
    /// Identifies a persona.
    PersonaId
);

/// How the persona's voice is generated (ElevenLabs voice settings), in
/// whole percent so the screen and storage need no floats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GenerationPresets {
    /// Higher is steadier and more monotone; lower is more expressive.
    pub stability: u8,
    /// How closely the output sticks to the original voice.
    pub similarity: u8,
    /// Exaggerates the voice's style; 0 is off.
    pub style: u8,
    /// Speaking rate, 100 is normal.
    pub speed: u8,
}

impl GenerationPresets {
    pub const PERCENT: std::ops::RangeInclusive<u8> = 0..=100;
    /// What the provider accepts (0.7x to 1.2x).
    pub const SPEED: std::ops::RangeInclusive<u8> = 70..=120;
}

impl Default for GenerationPresets {
    /// The provider's own defaults.
    fn default() -> Self {
        Self {
            stability: 50,
            similarity: 75,
            style: 0,
            speed: 100,
        }
    }
}

/// Raw persona fields as the user typed them, before validation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersonaDraft {
    pub name: String,
    pub voice: Option<VoiceRef>,
    pub tone: String,
    pub script_style: String,
    pub presets: GenerationPresets,
    /// The voice is a clone of a real person or a realistic synthetic
    /// voice: posts made with it need the network's synthetic-content
    /// disclosure.
    pub realistic_voice: bool,
}

/// Why a draft is not a valid persona. One entry per offending field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PersonaFieldError {
    NameRequired,
    NameTooLong,
    VoiceRequired,
    ToneTooLong,
    ScriptStyleTooLong,
    StabilityOutOfRange,
    SimilarityOutOfRange,
    StyleOutOfRange,
    SpeedOutOfRange,
}

impl PersonaFieldError {
    pub const ALL: [PersonaFieldError; 9] = [
        PersonaFieldError::NameRequired,
        PersonaFieldError::NameTooLong,
        PersonaFieldError::VoiceRequired,
        PersonaFieldError::ToneTooLong,
        PersonaFieldError::ScriptStyleTooLong,
        PersonaFieldError::StabilityOutOfRange,
        PersonaFieldError::SimilarityOutOfRange,
        PersonaFieldError::StyleOutOfRange,
        PersonaFieldError::SpeedOutOfRange,
    ];
}

/// The user-editable part of a persona, always valid: a name, a voice,
/// text within limits and presets within the provider's ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaDetails {
    name: String,
    voice: VoiceRef,
    tone: String,
    script_style: String,
    presets: GenerationPresets,
    realistic_voice: bool,
}

impl PersonaDetails {
    /// Limits are in characters, not bytes.
    pub const MAX_NAME_CHARS: usize = 80;
    pub const MAX_TONE_CHARS: usize = 1000;
    pub const MAX_SCRIPT_STYLE_CHARS: usize = 2000;

    /// Validates and normalizes a draft; surrounding whitespace is trimmed.
    pub fn validate(draft: PersonaDraft) -> Result<Self, Vec<PersonaFieldError>> {
        let mut errors = Vec::new();

        let name = draft.name.trim().to_owned();
        if name.is_empty() {
            errors.push(PersonaFieldError::NameRequired);
        } else if too_long(&name, Self::MAX_NAME_CHARS) {
            errors.push(PersonaFieldError::NameTooLong);
        }
        if draft.voice.is_none() {
            errors.push(PersonaFieldError::VoiceRequired);
        }
        let tone = draft.tone.trim().to_owned();
        if too_long(&tone, Self::MAX_TONE_CHARS) {
            errors.push(PersonaFieldError::ToneTooLong);
        }
        let script_style = draft.script_style.trim().to_owned();
        if too_long(&script_style, Self::MAX_SCRIPT_STYLE_CHARS) {
            errors.push(PersonaFieldError::ScriptStyleTooLong);
        }
        let presets = draft.presets;
        for (value, range, error) in [
            (
                presets.stability,
                GenerationPresets::PERCENT,
                PersonaFieldError::StabilityOutOfRange,
            ),
            (
                presets.similarity,
                GenerationPresets::PERCENT,
                PersonaFieldError::SimilarityOutOfRange,
            ),
            (
                presets.style,
                GenerationPresets::PERCENT,
                PersonaFieldError::StyleOutOfRange,
            ),
            (
                presets.speed,
                GenerationPresets::SPEED,
                PersonaFieldError::SpeedOutOfRange,
            ),
        ] {
            if !range.contains(&value) {
                errors.push(error);
            }
        }

        match draft.voice {
            Some(voice) if errors.is_empty() => Ok(Self {
                name,
                voice,
                tone,
                script_style,
                presets,
                realistic_voice: draft.realistic_voice,
            }),
            _ => Err(errors),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn voice(&self) -> &VoiceRef {
        &self.voice
    }

    pub fn tone(&self) -> &str {
        &self.tone
    }

    pub fn script_style(&self) -> &str {
        &self.script_style
    }

    pub fn presets(&self) -> GenerationPresets {
        self.presets
    }

    /// Whether posts narrated by this voice need a synthetic-content
    /// disclosure (a clone of a real person, or a realistic synthetic
    /// voice).
    pub fn realistic_voice(&self) -> bool {
        self.realistic_voice
    }

    /// Whether two persona names would read as the same to the user.
    pub fn same_name(&self, other: &str) -> bool {
        self.name.to_lowercase() == other.trim().to_lowercase()
    }

    /// The narrator as a script prompt describes it: name, voice, tone and
    /// script style, skipping what is blank.
    pub fn describe(&self) -> String {
        let mut parts = vec![self.name.clone(), format!("Voice: {}", self.voice.name())];
        if !self.tone.is_empty() {
            parts.push(format!("Tone: {}", self.tone));
        }
        if !self.script_style.is_empty() {
            parts.push(format!("Script style: {}", self.script_style));
        }
        parts
            .into_iter()
            .map(|part| {
                let part = part.trim_end();
                if part.ends_with(['.', '!', '?']) {
                    part.to_owned()
                } else {
                    format!("{part}.")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A name for a copy of this persona that none of `taken` uses:
    /// `Name (copy)`, then `Name (copy 2)` and so on, in the user's
    /// language through `suffix`. The original is cut to fit the limit.
    pub fn copy_name(&self, suffix: &str, taken: impl Fn(&str) -> bool) -> String {
        (1..)
            .map(|n| {
                let tag = if n == 1 {
                    format!(" ({suffix})")
                } else {
                    format!(" ({suffix} {n})")
                };
                let room = Self::MAX_NAME_CHARS.saturating_sub(tag.chars().count());
                let base: String = self.name.chars().take(room).collect();
                format!("{}{tag}", base.trim_end())
            })
            .find(|name| !taken(name))
            .expect("some numbered name is free")
    }
}

impl From<&PersonaDetails> for PersonaDraft {
    fn from(details: &PersonaDetails) -> Self {
        Self {
            name: details.name.clone(),
            voice: Some(details.voice.clone()),
            tone: details.tone.clone(),
            script_style: details.script_style.clone(),
            presets: details.presets,
            realistic_voice: details.realistic_voice,
        }
    }
}

fn too_long(text: &str, max_chars: usize) -> bool {
    text.chars().count() > max_chars
}

/// Why a persona's voice may not work for narration. Imported personas
/// point at a voice in someone else's provider account; until the user's
/// own account is seen to have it, the persona is flagged and cannot
/// narrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoiceFlag {
    /// The voice has not been looked up in the user's account yet (no key,
    /// or the listing failed).
    Unchecked,
    /// The user's account does not have the voice: add it there (share or
    /// clone it with the voice owner's consent) or pick another voice.
    Unavailable,
}

impl VoiceFlag {
    /// The flag a persona's voice gets from a lookup in the user's
    /// account: `Some(true)` if listed, `Some(false)` if not, `None` if
    /// the account could not be listed.
    pub fn from_lookup(listed: Option<bool>) -> Option<VoiceFlag> {
        match listed {
            Some(true) => None,
            Some(false) => Some(VoiceFlag::Unavailable),
            None => Some(VoiceFlag::Unchecked),
        }
    }
}

/// A narrator identity in the user's library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persona {
    pub id: PersonaId,
    pub owner: ProfileId,
    pub details: PersonaDetails,
    /// Set while the voice is not known to be in the user's account; a
    /// flagged persona cannot narrate.
    pub voice_flag: Option<VoiceFlag>,
}

impl Persona {
    pub fn new(owner: ProfileId, details: PersonaDetails) -> Self {
        Self {
            id: PersonaId::new(),
            owner,
            details,
            voice_flag: None,
        }
    }

    /// Whether it can read a narration now.
    pub fn can_narrate(&self) -> bool {
        self.voice_flag.is_none()
    }

    /// The personas a new profile starts with (PRD story 12): a sober
    /// documentary narrator and a dramatic storyteller in en-US and pt-BR,
    /// one male and one female voice in each language. The voices are
    /// ElevenLabs default voices every account has, from the set that does
    /// not expire; each persona's text is in its own language.
    pub fn defaults(owner: ProfileId) -> Vec<Persona> {
        DEFAULTS
            .iter()
            .map(|default| {
                let draft = PersonaDraft {
                    name: default.name.to_owned(),
                    voice: Some(
                        VoiceRef::elevenlabs(default.voice_id, default.voice_name)
                            .expect("default voice ids are valid"),
                    ),
                    tone: default.tone.to_owned(),
                    script_style: default.script_style.to_owned(),
                    presets: default.presets,
                    // The provider's stock voices are of no real person.
                    realistic_voice: false,
                };
                let details = PersonaDetails::validate(draft).expect("default personas are valid");
                Persona::new(owner, details)
            })
            .collect()
    }
}

struct DefaultPersona {
    name: &'static str,
    voice_id: &'static str,
    voice_name: &'static str,
    tone: &'static str,
    script_style: &'static str,
    presets: GenerationPresets,
}

/// Steady and plain for documentaries.
const SOBER: GenerationPresets = GenerationPresets {
    stability: 60,
    similarity: 75,
    style: 0,
    speed: 100,
};

/// Freer and more expressive for storytelling, a little slower.
const DRAMATIC: GenerationPresets = GenerationPresets {
    stability: 40,
    similarity: 75,
    style: 35,
    speed: 95,
};

const DEFAULTS: [DefaultPersona; 4] = [
    DefaultPersona {
        name: "Documentary Narrator (en-US)",
        voice_id: "FrS6cKLB1wg4WYgPa9GW",
        voice_name: "Wyatt",
        tone: "Sober, measured and authoritative. Calm pacing and no hype: the facts carry the weight.",
        script_style: "Clear chronological structure. Short declarative sentences with concrete dates, names and numbers. Open with a factual hook and close with a quiet reflection. No exclamation marks or clickbait phrasing.",
        presets: SOBER,
    },
    DefaultPersona {
        name: "Dramatic Storyteller (en-US)",
        voice_id: "22N9cF8z0o7y23njdyaY",
        voice_name: "Florence",
        tone: "Dramatic and immersive. Builds tension, varies the pace and leans into suspense and emotion.",
        script_style: "A story arc: cold open, rising tension, a reveal. Vivid sensory detail, moments that speak to the viewer directly and cliffhanger transitions between sections. End on a line that stays with the viewer.",
        presets: DRAMATIC,
    },
    DefaultPersona {
        name: "Narradora Documental (pt-BR)",
        voice_id: "WQP7cQUF5aAS6Axh5yaa",
        voice_name: "Elara",
        tone: "Sóbria, ponderada e confiável. Ritmo calmo e sem exageros: os fatos falam por si.",
        script_style: "Estrutura cronológica clara. Frases curtas e afirmativas, com datas, nomes e números concretos. Abra com um gancho factual e feche com uma reflexão serena. Sem pontos de exclamação nem frases caça-clique.",
        presets: SOBER,
    },
    DefaultPersona {
        name: "Contador de Histórias Dramático (pt-BR)",
        voice_id: "gOupLcAkjEnguROwi4oS",
        voice_name: "Darian",
        tone: "Dramático e envolvente. Constrói tensão, varia o ritmo e aposta no suspense e na emoção.",
        script_style: "Arco narrativo: abertura impactante, tensão crescente e uma revelação. Detalhes sensoriais vívidos, momentos que falam direto com o espectador e transições com gancho entre as partes. Termine com uma frase marcante.",
        presets: DRAMATIC,
    },
];

/// Persistence port for personas. Shared with job worker threads.
pub trait PersonaRepository: Send + Sync {
    /// The owner's personas, ordered by name.
    fn list(&self, owner: ProfileId) -> Result<Vec<Persona>, RepositoryError>;

    fn get(&self, id: PersonaId) -> Result<Option<Persona>, RepositoryError>;

    /// Inserts or updates the persona.
    fn save(&self, persona: &Persona) -> Result<(), RepositoryError>;

    /// Inserts every persona, or none of them.
    fn insert_all(&self, personas: &[Persona]) -> Result<(), RepositoryError>;
}

impl<T: PersonaRepository + ?Sized> PersonaRepository for Arc<T> {
    fn list(&self, owner: ProfileId) -> Result<Vec<Persona>, RepositoryError> {
        (**self).list(owner)
    }

    fn get(&self, id: PersonaId) -> Result<Option<Persona>, RepositoryError> {
        (**self).get(id)
    }

    fn save(&self, persona: &Persona) -> Result<(), RepositoryError> {
        (**self).save(persona)
    }

    fn insert_all(&self, personas: &[Persona]) -> Result<(), RepositoryError> {
        (**self).insert_all(personas)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::Provider;

    fn voice() -> VoiceRef {
        VoiceRef::elevenlabs("abc123", "Wyatt").unwrap()
    }

    fn draft(name: &str) -> PersonaDraft {
        PersonaDraft {
            name: name.into(),
            voice: Some(voice()),
            ..PersonaDraft::default()
        }
    }

    fn errors(draft: PersonaDraft) -> Vec<PersonaFieldError> {
        PersonaDetails::validate(draft).unwrap_err()
    }

    #[test]
    fn a_name_and_a_voice_are_enough() {
        let details = PersonaDetails::validate(draft("Narrator")).unwrap();
        assert_eq!(details.name(), "Narrator");
        assert_eq!(details.voice(), &voice());
        assert_eq!(details.presets(), GenerationPresets::default());
    }

    #[test]
    fn name_and_voice_are_required() {
        assert_eq!(
            errors(PersonaDraft::default()),
            [
                PersonaFieldError::NameRequired,
                PersonaFieldError::VoiceRequired
            ]
        );
    }

    #[test]
    fn text_is_trimmed_and_limited_in_characters() {
        let details = PersonaDetails::validate(PersonaDraft {
            tone: "  calm \n".into(),
            script_style: "\n short sentences ".into(),
            ..draft("  Narrator ")
        })
        .unwrap();
        assert_eq!(details.name(), "Narrator");
        assert_eq!(details.tone(), "calm");
        assert_eq!(details.script_style(), "short sentences");

        let at_limit = "é".repeat(PersonaDetails::MAX_NAME_CHARS);
        assert!(PersonaDetails::validate(draft(&at_limit)).is_ok());
        let long = |n: usize| "x".repeat(n + 1);
        assert_eq!(
            errors(PersonaDraft {
                tone: long(PersonaDetails::MAX_TONE_CHARS),
                script_style: long(PersonaDetails::MAX_SCRIPT_STYLE_CHARS),
                ..draft(&long(PersonaDetails::MAX_NAME_CHARS))
            }),
            [
                PersonaFieldError::NameTooLong,
                PersonaFieldError::ToneTooLong,
                PersonaFieldError::ScriptStyleTooLong,
            ]
        );
    }

    #[test]
    fn presets_stay_within_the_providers_ranges() {
        let presets = GenerationPresets {
            stability: 101,
            similarity: 200,
            style: 101,
            speed: 69,
        };
        assert_eq!(
            errors(PersonaDraft {
                presets,
                ..draft("Narrator")
            }),
            [
                PersonaFieldError::StabilityOutOfRange,
                PersonaFieldError::SimilarityOutOfRange,
                PersonaFieldError::StyleOutOfRange,
                PersonaFieldError::SpeedOutOfRange,
            ]
        );
        let edges = GenerationPresets {
            stability: 0,
            similarity: 100,
            style: 100,
            speed: 120,
        };
        assert!(
            PersonaDetails::validate(PersonaDraft {
                presets: edges,
                ..draft("Narrator")
            })
            .is_ok()
        );
        assert_eq!(
            errors(PersonaDraft {
                presets: GenerationPresets {
                    speed: 121,
                    ..edges
                },
                ..draft("Narrator")
            }),
            [PersonaFieldError::SpeedOutOfRange]
        );
    }

    #[test]
    fn a_valid_persona_round_trips_through_a_draft() {
        let details = PersonaDetails::validate(PersonaDraft {
            tone: "calm".into(),
            script_style: "short".into(),
            presets: GenerationPresets {
                stability: 30,
                similarity: 80,
                style: 10,
                speed: 90,
            },
            ..draft("Narrator")
        })
        .unwrap();
        assert_eq!(
            PersonaDetails::validate(PersonaDraft::from(&details)),
            Ok(details)
        );
    }

    #[test]
    fn the_description_names_voice_tone_and_style() {
        let details = PersonaDetails::validate(PersonaDraft {
            tone: "Calm and measured".into(),
            script_style: "Short sentences.".into(),
            ..draft("Documentary Narrator")
        })
        .unwrap();
        assert_eq!(
            details.describe(),
            "Documentary Narrator. Voice: Wyatt. Tone: Calm and measured. Script style: Short sentences."
        );
        let bare = PersonaDetails::validate(draft("Narrator")).unwrap();
        assert_eq!(bare.describe(), "Narrator. Voice: Wyatt.");
    }

    #[test]
    fn copies_get_a_free_numbered_name() {
        let details = PersonaDetails::validate(draft("Narrator")).unwrap();
        let taken: HashSet<String> = ["narrator (copy)".to_owned(), "narrator (copy 2)".into()]
            .into_iter()
            .collect();
        let name = details.copy_name("copy", |name| taken.contains(&name.to_lowercase()));
        assert_eq!(name, "Narrator (copy 3)");
        assert_eq!(details.copy_name("cópia", |_| false), "Narrator (cópia)");
    }

    #[test]
    fn a_copy_of_a_long_name_still_fits() {
        let long = "x".repeat(PersonaDetails::MAX_NAME_CHARS);
        let details = PersonaDetails::validate(draft(&long)).unwrap();
        let name = details.copy_name("copy", |_| false);
        assert_eq!(name.chars().count(), PersonaDetails::MAX_NAME_CHARS);
        assert!(name.ends_with(" (copy)"));
        assert!(PersonaDetails::validate(draft(&name)).is_ok());
    }

    #[test]
    fn a_voice_lookup_sets_or_clears_the_flag() {
        assert_eq!(VoiceFlag::from_lookup(Some(true)), None);
        assert_eq!(
            VoiceFlag::from_lookup(Some(false)),
            Some(VoiceFlag::Unavailable)
        );
        assert_eq!(VoiceFlag::from_lookup(None), Some(VoiceFlag::Unchecked));

        let mut persona = Persona::new(
            ProfileId::new(),
            PersonaDetails::validate(draft("Narrator")).unwrap(),
        );
        assert!(persona.can_narrate(), "a persona made here is not flagged");
        persona.voice_flag = Some(VoiceFlag::Unchecked);
        assert!(!persona.can_narrate());
    }

    #[test]
    fn four_defaults_two_per_language_one_male_and_one_female_voice_each() {
        let owner = ProfileId::new();
        let defaults = Persona::defaults(owner);
        assert_eq!(defaults.len(), 4);
        assert!(defaults.iter().all(|p| p.owner == owner));
        assert!(
            defaults
                .iter()
                .all(|p| p.details.voice().provider() == Provider::ElevenLabs)
        );
        let names: HashSet<_> = defaults
            .iter()
            .map(|p| p.details.name().to_lowercase())
            .collect();
        assert_eq!(names.len(), 4, "distinct names");
        let voices: HashSet<_> = defaults.iter().map(|p| p.details.voice().id()).collect();
        assert_eq!(voices.len(), 4, "distinct voices");
        for language in ["(en-US)", "(pt-BR)"] {
            let count = defaults
                .iter()
                .filter(|p| p.details.name().ends_with(language))
                .count();
            assert_eq!(count, 2, "{language}");
        }
        let ids: HashSet<_> = defaults.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), 4);
    }
}
