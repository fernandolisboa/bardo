//! UI strings, loaded from one resource file per language in `locales/`.

use std::borrow::Cow;
use std::collections::BTreeMap;

use bardo_domain::{ChannelFieldError, ContentLanguage, Country, UiLanguage};

/// Every string the UI shows. Adding a variant without adding its key to all
/// resource files fails the catalog tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    AppName,
    AppTagline,
    UiLanguageLabel,
    LanguageName(UiLanguage),
    LanguageNotSaved,
    ContentLanguageName(ContentLanguage),
    CountryName(Country),
    ChannelsTitle,
    ChannelsEmpty,
    ChannelsNotLoaded,
    NewChannel,
    NewChannelTitle,
    EditChannelTitle,
    ChannelName,
    ChannelNamePlaceholder,
    ChannelNiche,
    ChannelNichePlaceholder,
    ChannelThemes,
    ChannelThemesPlaceholder,
    ChannelThemesHint,
    ChannelAestheticNotes,
    ChannelAestheticNotesPlaceholder,
    ChannelLanguage,
    ChannelCountry,
    ChannelCountrySearch,
    CreateChannel,
    SaveChannel,
    ChannelSaved,
    ChannelNameTaken,
    ChannelNotFound,
    ChannelNotSaved,
    ChannelFieldError(ChannelFieldError),
}

impl Text {
    fn key(self) -> Cow<'static, str> {
        let key = match self {
            Text::AppName => "app.name",
            Text::AppTagline => "app.tagline",
            Text::UiLanguageLabel => "settings.ui_language",
            Text::LanguageName(UiLanguage::EnUs) => "language.en-US",
            Text::LanguageName(UiLanguage::PtBr) => "language.pt-BR",
            Text::LanguageNotSaved => "error.language_not_saved",
            Text::ContentLanguageName(language) => {
                return format!("content_language.{}", language.code()).into();
            }
            Text::CountryName(country) => return format!("country.{}", country.code()).into(),
            Text::ChannelsTitle => "channels.title",
            Text::ChannelsEmpty => "channels.empty",
            Text::ChannelsNotLoaded => "channels.not_loaded",
            Text::NewChannel => "channels.new",
            Text::NewChannelTitle => "channel.new_title",
            Text::EditChannelTitle => "channel.edit_title",
            Text::ChannelName => "channel.name",
            Text::ChannelNamePlaceholder => "channel.name_placeholder",
            Text::ChannelNiche => "channel.niche",
            Text::ChannelNichePlaceholder => "channel.niche_placeholder",
            Text::ChannelThemes => "channel.themes",
            Text::ChannelThemesPlaceholder => "channel.themes_placeholder",
            Text::ChannelThemesHint => "channel.themes_hint",
            Text::ChannelAestheticNotes => "channel.aesthetic_notes",
            Text::ChannelAestheticNotesPlaceholder => "channel.aesthetic_notes_placeholder",
            Text::ChannelLanguage => "channel.language",
            Text::ChannelCountry => "channel.country",
            Text::ChannelCountrySearch => "channel.country_search",
            Text::CreateChannel => "channel.create",
            Text::SaveChannel => "channel.save",
            Text::ChannelSaved => "channel.saved",
            Text::ChannelNameTaken => "channel.error.name_taken",
            Text::ChannelNotFound => "channel.error.not_found",
            Text::ChannelNotSaved => "channel.error.not_saved",
            Text::ChannelFieldError(error) => match error {
                ChannelFieldError::NameRequired => "channel.error.name_required",
                ChannelFieldError::NameTooLong => "channel.error.name_too_long",
                ChannelFieldError::NicheTooLong => "channel.error.niche_too_long",
                ChannelFieldError::TooManyThemes => "channel.error.too_many_themes",
                ChannelFieldError::ThemeTooLong => "channel.error.theme_too_long",
                ChannelFieldError::AestheticNotesTooLong => {
                    "channel.error.aesthetic_notes_too_long"
                }
            },
        };
        Cow::Borrowed(key)
    }
}

fn source(language: UiLanguage) -> &'static str {
    match language {
        UiLanguage::EnUs => include_str!("../locales/en-US.toml"),
        UiLanguage::PtBr => include_str!("../locales/pt-BR.toml"),
    }
}

/// Strings of one language, keyed by dotted path (`app.name`).
#[derive(Debug, Clone)]
pub struct Catalog {
    language: UiLanguage,
    strings: BTreeMap<String, String>,
}

impl Catalog {
    /// Loads the embedded resource file. The files ship inside the binary and
    /// are checked by tests, so a parse failure is a build defect.
    pub fn load(language: UiLanguage) -> Self {
        let table: toml::Table = source(language)
            .parse()
            .unwrap_or_else(|e| panic!("locales/{language}.toml is invalid: {e}"));
        let mut strings = BTreeMap::new();
        flatten("", &table, &mut strings);
        Self { language, strings }
    }

    pub fn language(&self) -> UiLanguage {
        self.language
    }

    /// The string for `text`, or its key when missing so a gap is visible
    /// instead of blank.
    pub fn get(&self, text: Text) -> Cow<'_, str> {
        let key = text.key();
        match self.strings.get(key.as_ref()) {
            Some(text) => Cow::Borrowed(text.as_str()),
            None => Cow::Owned(key.into_owned()),
        }
    }
}

fn flatten(prefix: &str, table: &toml::Table, out: &mut BTreeMap<String, String>) {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::Table(nested) => flatten(&key, nested, out),
            toml::Value::String(text) => {
                out.insert(key, text.clone());
            }
            other => panic!(
                "locale key {key} must be a string, found {}",
                other.type_str()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_texts() -> Vec<Text> {
        let mut texts = vec![
            Text::AppName,
            Text::AppTagline,
            Text::UiLanguageLabel,
            Text::LanguageNotSaved,
            Text::ChannelsTitle,
            Text::ChannelsEmpty,
            Text::ChannelsNotLoaded,
            Text::NewChannel,
            Text::NewChannelTitle,
            Text::EditChannelTitle,
            Text::ChannelName,
            Text::ChannelNamePlaceholder,
            Text::ChannelNiche,
            Text::ChannelNichePlaceholder,
            Text::ChannelThemes,
            Text::ChannelThemesPlaceholder,
            Text::ChannelThemesHint,
            Text::ChannelAestheticNotes,
            Text::ChannelAestheticNotesPlaceholder,
            Text::ChannelLanguage,
            Text::ChannelCountry,
            Text::ChannelCountrySearch,
            Text::CreateChannel,
            Text::SaveChannel,
            Text::ChannelSaved,
            Text::ChannelNameTaken,
            Text::ChannelNotFound,
            Text::ChannelNotSaved,
        ];
        texts.extend(UiLanguage::ALL.map(Text::LanguageName));
        texts.extend(ContentLanguage::ALL.map(Text::ContentLanguageName));
        texts.extend(Country::ALL.map(Text::CountryName));
        texts.extend(ChannelFieldError::ALL.map(Text::ChannelFieldError));
        texts
    }

    #[test]
    fn every_text_exists_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for text in all_texts() {
                assert!(
                    catalog.strings.contains_key(text.key().as_ref()),
                    "{language} is missing {}",
                    text.key()
                );
            }
        }
    }

    #[test]
    fn all_languages_have_the_same_keys() {
        let reference = Catalog::load(UiLanguage::EnUs);
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            let expected: Vec<_> = reference.strings.keys().collect();
            let actual: Vec<_> = catalog.strings.keys().collect();
            assert_eq!(actual, expected, "{language} keys differ from en-US");
        }
    }

    #[test]
    fn limit_messages_match_the_domain_limits() {
        use bardo_domain::ChannelDetails;

        let catalog = Catalog::load(UiLanguage::EnUs);
        for (error, limit) in [
            (
                ChannelFieldError::NameTooLong,
                ChannelDetails::MAX_NAME_CHARS,
            ),
            (
                ChannelFieldError::NicheTooLong,
                ChannelDetails::MAX_NICHE_CHARS,
            ),
            (ChannelFieldError::TooManyThemes, ChannelDetails::MAX_THEMES),
            (
                ChannelFieldError::ThemeTooLong,
                ChannelDetails::MAX_THEME_CHARS,
            ),
        ] {
            let message = catalog.get(Text::ChannelFieldError(error));
            assert!(message.contains(&limit.to_string()), "{message}");
        }
    }

    #[test]
    fn strings_differ_between_languages() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        assert_ne!(en.get(Text::AppTagline), pt.get(Text::AppTagline));
    }
}
