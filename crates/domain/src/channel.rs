use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use uuid::Uuid;

use crate::{ContentLanguage, Country, ProfileId, RepositoryError};

/// Identifies a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelId(Uuid);

impl ChannelId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ChannelId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for ChannelId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Raw channel fields as the user typed them, before validation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChannelDraft {
    pub name: String,
    pub niche: String,
    pub themes: Vec<String>,
    pub aesthetic_notes: String,
    pub language: ContentLanguage,
    pub country: Country,
}

/// Why a draft is not a valid channel. One entry per offending field, so a
/// form can show every problem at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelFieldError {
    NameRequired,
    NameTooLong,
    NicheTooLong,
    TooManyThemes,
    ThemeTooLong,
    AestheticNotesTooLong,
}

impl ChannelFieldError {
    pub const ALL: [ChannelFieldError; 6] = [
        ChannelFieldError::NameRequired,
        ChannelFieldError::NameTooLong,
        ChannelFieldError::NicheTooLong,
        ChannelFieldError::TooManyThemes,
        ChannelFieldError::ThemeTooLong,
        ChannelFieldError::AestheticNotesTooLong,
    ];
}

/// The user-editable part of a channel, always valid: name present, text
/// within limits, themes trimmed and without duplicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelDetails {
    name: String,
    niche: String,
    themes: Vec<String>,
    aesthetic_notes: String,
    language: ContentLanguage,
    country: Country,
}

impl ChannelDetails {
    /// Limits are in characters, not bytes, so accents count once.
    pub const MAX_NAME_CHARS: usize = 100;
    pub const MAX_NICHE_CHARS: usize = 100;
    pub const MAX_THEMES: usize = 30;
    pub const MAX_THEME_CHARS: usize = 100;
    pub const MAX_AESTHETIC_NOTES_CHARS: usize = 2000;

    /// Validates and normalizes a draft. Surrounding whitespace is trimmed,
    /// blank themes are dropped and repeated themes (ignoring case) are kept
    /// once, in their first position.
    pub fn validate(draft: ChannelDraft) -> Result<Self, Vec<ChannelFieldError>> {
        let mut errors = Vec::new();

        let name = draft.name.trim().to_owned();
        if name.is_empty() {
            errors.push(ChannelFieldError::NameRequired);
        } else if too_long(&name, Self::MAX_NAME_CHARS) {
            errors.push(ChannelFieldError::NameTooLong);
        }

        let niche = draft.niche.trim().to_owned();
        if too_long(&niche, Self::MAX_NICHE_CHARS) {
            errors.push(ChannelFieldError::NicheTooLong);
        }

        let themes = normalize_themes(draft.themes);
        if themes.len() > Self::MAX_THEMES {
            errors.push(ChannelFieldError::TooManyThemes);
        }
        if themes
            .iter()
            .any(|theme| too_long(theme, Self::MAX_THEME_CHARS))
        {
            errors.push(ChannelFieldError::ThemeTooLong);
        }

        let aesthetic_notes = draft.aesthetic_notes.trim().to_owned();
        if too_long(&aesthetic_notes, Self::MAX_AESTHETIC_NOTES_CHARS) {
            errors.push(ChannelFieldError::AestheticNotesTooLong);
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            name,
            niche,
            themes,
            aesthetic_notes,
            language: draft.language,
            country: draft.country,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn niche(&self) -> &str {
        &self.niche
    }

    pub fn themes(&self) -> &[String] {
        &self.themes
    }

    pub fn aesthetic_notes(&self) -> &str {
        &self.aesthetic_notes
    }

    pub fn language(&self) -> ContentLanguage {
        self.language
    }

    pub fn country(&self) -> Country {
        self.country
    }

    /// Whether two channel names would read as the same to the user.
    pub fn same_name(&self, other: &str) -> bool {
        self.name.to_lowercase() == other.trim().to_lowercase()
    }
}

impl From<&ChannelDetails> for ChannelDraft {
    fn from(details: &ChannelDetails) -> Self {
        Self {
            name: details.name.clone(),
            niche: details.niche.clone(),
            themes: details.themes.clone(),
            aesthetic_notes: details.aesthetic_notes.clone(),
            language: details.language,
            country: details.country,
        }
    }
}

fn too_long(text: &str, max_chars: usize) -> bool {
    text.chars().count() > max_chars
}

fn normalize_themes(themes: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    themes
        .into_iter()
        .map(|theme| theme.trim().to_owned())
        .filter(|theme| !theme.is_empty() && seen.insert(theme.to_lowercase()))
        .collect()
}

/// A brand the user runs on one or more networks (CONTEXT.md). Default
/// persona and network accounts arrive with their own slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: ChannelId,
    pub owner: ProfileId,
    pub details: ChannelDetails,
}

impl Channel {
    pub fn new(owner: ProfileId, details: ChannelDetails) -> Self {
        Self {
            id: ChannelId::new(),
            owner,
            details,
        }
    }
}

/// Persistence port for channels.
pub trait ChannelRepository {
    /// The owner's channels, ordered by name.
    fn list(&self, owner: ProfileId) -> Result<Vec<Channel>, RepositoryError>;

    fn get(&self, id: ChannelId) -> Result<Option<Channel>, RepositoryError>;

    /// Inserts or updates the channel.
    fn save(&self, channel: &Channel) -> Result<(), RepositoryError>;
}

impl<T: ChannelRepository + ?Sized> ChannelRepository for Arc<T> {
    fn list(&self, owner: ProfileId) -> Result<Vec<Channel>, RepositoryError> {
        (**self).list(owner)
    }

    fn get(&self, id: ChannelId) -> Result<Option<Channel>, RepositoryError> {
        (**self).get(id)
    }

    fn save(&self, channel: &Channel) -> Result<(), RepositoryError> {
        (**self).save(channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(name: &str) -> ChannelDraft {
        ChannelDraft {
            name: name.into(),
            ..ChannelDraft::default()
        }
    }

    fn errors(draft: ChannelDraft) -> Vec<ChannelFieldError> {
        ChannelDetails::validate(draft).unwrap_err()
    }

    #[test]
    fn a_name_is_enough_for_a_channel() {
        let details = ChannelDetails::validate(draft("Space Archives")).unwrap();
        assert_eq!(details.name(), "Space Archives");
        assert_eq!(details.niche(), "");
        assert!(details.themes().is_empty());
        assert_eq!(details.language(), ContentLanguage::English);
        assert_eq!(details.country(), Country::UnitedStates);
    }

    #[test]
    fn name_is_required() {
        assert_eq!(errors(draft("")), [ChannelFieldError::NameRequired]);
        assert_eq!(errors(draft("  \t ")), [ChannelFieldError::NameRequired]);
    }

    #[test]
    fn text_fields_are_trimmed() {
        let details = ChannelDetails::validate(ChannelDraft {
            name: "  Space Archives ".into(),
            niche: " space history\n".into(),
            aesthetic_notes: "\n dark, archival footage \n".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        assert_eq!(details.name(), "Space Archives");
        assert_eq!(details.niche(), "space history");
        assert_eq!(details.aesthetic_notes(), "dark, archival footage");
    }

    #[test]
    fn length_limits_count_characters_not_bytes() {
        let at_limit = "é".repeat(ChannelDetails::MAX_NAME_CHARS);
        assert!(ChannelDetails::validate(draft(&at_limit)).is_ok());

        let over = "é".repeat(ChannelDetails::MAX_NAME_CHARS + 1);
        assert_eq!(errors(draft(&over)), [ChannelFieldError::NameTooLong]);
    }

    #[test]
    fn every_invalid_field_is_reported() {
        let long = |n: usize| "x".repeat(n + 1);
        let found = errors(ChannelDraft {
            name: String::new(),
            niche: long(ChannelDetails::MAX_NICHE_CHARS),
            themes: vec![long(ChannelDetails::MAX_THEME_CHARS)],
            aesthetic_notes: long(ChannelDetails::MAX_AESTHETIC_NOTES_CHARS),
            ..ChannelDraft::default()
        });
        assert_eq!(
            found,
            [
                ChannelFieldError::NameRequired,
                ChannelFieldError::NicheTooLong,
                ChannelFieldError::ThemeTooLong,
                ChannelFieldError::AestheticNotesTooLong,
            ]
        );
    }

    #[test]
    fn themes_are_trimmed_deduplicated_and_blank_ones_dropped() {
        let details = ChannelDetails::validate(ChannelDraft {
            themes: vec![
                " Apollo program ".into(),
                "".into(),
                "Cold War".into(),
                "apollo PROGRAM".into(),
                "   ".into(),
            ],
            ..draft("Space Archives")
        })
        .unwrap();
        assert_eq!(details.themes(), ["Apollo program", "Cold War"]);
    }

    #[test]
    fn theme_count_is_limited_after_deduplication() {
        let distinct: Vec<String> = (0..=ChannelDetails::MAX_THEMES)
            .map(|i| format!("theme {i}"))
            .collect();
        assert_eq!(
            errors(ChannelDraft {
                themes: distinct,
                ..draft("Space Archives")
            }),
            [ChannelFieldError::TooManyThemes]
        );

        let repeated = vec!["same".to_owned(); ChannelDetails::MAX_THEMES + 5];
        assert!(
            ChannelDetails::validate(ChannelDraft {
                themes: repeated,
                ..draft("Space Archives")
            })
            .is_ok()
        );
    }

    #[test]
    fn a_valid_channel_round_trips_through_a_draft() {
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Arquivos do Espaço".into(),
            niche: "história espacial".into(),
            themes: vec!["Apollo".into(), "Guerra Fria".into()],
            aesthetic_notes: "escuro, imagens de arquivo".into(),
            language: ContentLanguage::Portuguese,
            country: Country::Brazil,
        })
        .unwrap();
        assert_eq!(
            ChannelDetails::validate(ChannelDraft::from(&details)),
            Ok(details)
        );
    }

    #[test]
    fn same_name_ignores_case_and_surrounding_spaces() {
        let details = ChannelDetails::validate(draft("Arquivos do Espaço")).unwrap();
        assert!(details.same_name("  arquivos do ESPAÇO "));
        assert!(!details.same_name("Arquivos do Espaço 2"));
    }

    #[test]
    fn new_channels_get_distinct_ids_and_keep_their_owner() {
        let owner = ProfileId::new();
        let details = ChannelDetails::validate(draft("A")).unwrap();
        let a = Channel::new(owner, details.clone());
        let b = Channel::new(owner, details);
        assert_ne!(a.id, b.id);
        assert_eq!(a.owner, owner);
    }
}
