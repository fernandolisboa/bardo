use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use uuid::Uuid;

use crate::{RepositoryError, UiThemePreference};

/// Identifies a user profile. Every entity carries the owning profile so more
/// profiles can exist later without a migration (ADR-0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProfileId(Uuid);

impl ProfileId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for ProfileId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Language of the app's own interface. Content language belongs to channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UiLanguage {
    #[default]
    EnUs,
    PtBr,
}

impl UiLanguage {
    pub const ALL: [UiLanguage; 2] = [UiLanguage::PtBr, UiLanguage::EnUs];

    /// BCP 47 tag, also the name of the language's resource file.
    pub fn tag(self) -> &'static str {
        match self {
            UiLanguage::EnUs => "en-US",
            UiLanguage::PtBr => "pt-BR",
        }
    }
}

impl fmt::Display for UiLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported UI language: {0}")]
pub struct UnsupportedLanguage(pub String);

impl FromStr for UiLanguage {
    type Err = UnsupportedLanguage;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UiLanguage::ALL
            .into_iter()
            .find(|language| language.tag().eq_ignore_ascii_case(s))
            .ok_or_else(|| UnsupportedLanguage(s.to_owned()))
    }
}

/// The local owner of all data. Single user today, no login (CONTEXT.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserProfile {
    pub id: ProfileId,
    pub ui_language: UiLanguage,
    pub ui_theme: UiThemePreference,
}

impl UserProfile {
    pub fn new(ui_language: UiLanguage) -> Self {
        Self {
            id: ProfileId::new(),
            ui_language,
            ui_theme: UiThemePreference::default(),
        }
    }
}

/// Persistence port for user profiles.
pub trait ProfileRepository {
    /// The profile the app starts with, if one was created before.
    fn load_default(&self) -> Result<Option<UserProfile>, RepositoryError>;

    /// Inserts or updates the profile.
    fn save(&self, profile: &UserProfile) -> Result<(), RepositoryError>;
}

/// One storage adapter can serve several ports through a shared handle.
impl<T: ProfileRepository + ?Sized> ProfileRepository for Arc<T> {
    fn load_default(&self) -> Result<Option<UserProfile>, RepositoryError> {
        (**self).load_default()
    }

    fn save(&self, profile: &UserProfile) -> Result<(), RepositoryError> {
        (**self).save(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tags_round_trip() {
        for language in UiLanguage::ALL {
            assert_eq!(language.tag().parse::<UiLanguage>(), Ok(language));
        }
    }

    #[test]
    fn language_parsing_ignores_case() {
        assert_eq!("PT-br".parse::<UiLanguage>(), Ok(UiLanguage::PtBr));
    }

    #[test]
    fn unknown_language_is_rejected() {
        assert_eq!(
            "fr-FR".parse::<UiLanguage>(),
            Err(UnsupportedLanguage("fr-FR".into()))
        );
    }

    #[test]
    fn new_profiles_get_distinct_ids() {
        let a = UserProfile::new(UiLanguage::EnUs);
        let b = UserProfile::new(UiLanguage::EnUs);
        assert_ne!(a.id, b.id);
    }
}
