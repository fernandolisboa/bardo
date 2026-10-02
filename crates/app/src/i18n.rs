//! UI strings, loaded from one resource file per language in `locales/`.

use std::borrow::Cow;
use std::collections::BTreeMap;

use std::time::Duration;

use bardo_domain::{
    ApiKeyError, ChannelFieldError, ContentLanguage, Country, JobFailureKind, JobKind, JobState,
    KeyCheckOutcome, NicheSeedError, Provider, UiLanguage,
};

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
    JobsTitle,
    JobsEmpty,
    JobsRunning,
    JobsQueued,
    JobsFailed,
    JobsFinished,
    JobKindName(JobKind),
    JobStateName(JobState),
    JobFailureKindName(JobFailureKind),
    /// Placeholders: `{attempt}`, `{max}`.
    JobRetryScheduled,
    /// Placeholder: `{attempts}`.
    JobFailedAfter,
    CancelJob,
    RetryJob,
    JobNotUpdated,
    JobNotStarted,
    TestJobsTitle,
    TestJobsHint,
    StartTestJob,
    StartFailingTestJob,
    SettingsTitle,
    ProviderKeysTitle,
    ProviderKeysHint,
    ProviderName(Provider),
    ProviderPurpose(Provider),
    ProviderKeyPlaceholder(Provider),
    KeyNotSet,
    /// Placeholder: `{hint}`.
    KeySaved,
    KeyUnreadable,
    SaveKey,
    ReplaceKey,
    TestKey,
    TestingKey,
    RemoveKey,
    KeyCheckOutcome(KeyCheckOutcome),
    /// Placeholder: `{detail}`.
    KeyCheckDetail,
    ApiKeyError(ApiKeyError),
    ProviderKeyNotSet,
    ProviderKeyStoreFailed,
    ResearchTitle,
    ResearchNoChannels,
    ResearchChannel,
    ResearchSeeds,
    ResearchSeedsPlaceholder,
    ResearchSeedsHint,
    RunResearch,
    RefreshResearch,
    /// Placeholder: `{units}`.
    RefreshResearchCost,
    /// Placeholder: `{units}`.
    ResearchCost,
    ResearchCostNone,
    ResearchMissingKey,
    ResearchNotStarted,
    ResearchNotLoaded,
    ResearchRunning,
    ResearchStopped,
    NicheSeedError(NicheSeedError),
    ResearchResultsTitle,
    ResearchScoresHint,
    ResearchOpportunity,
    ResearchCompetition,
    ResearchTrend,
    ResearchUploads,
    ResearchMedianViews,
    ResearchViewsPerDay,
    ResearchMedianSubscribers,
    ResearchSmallChannels,
    /// Placeholders: `{videos}`, `{channels}`.
    ResearchSample,
    ResearchFetched,
    ResearchNotFetched,
    ResearchNoUploads,
    ResearchStale,
    FetchedJustNow,
    /// Placeholder: `{n}`.
    FetchedMinutesAgo,
    /// Placeholder: `{n}`.
    FetchedHoursAgo,
    /// Placeholder: `{n}`.
    FetchedDaysAgo,
    /// Placeholder: `{n}`, already formatted with the decimal separator.
    NumberThousands,
    /// Placeholder: `{n}`.
    NumberMillions,
    /// Placeholder: `{n}`.
    NumberBillions,
    DecimalSeparator,
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
            Text::JobsTitle => "jobs.title",
            Text::JobsEmpty => "jobs.empty",
            Text::JobsRunning => "jobs.running",
            Text::JobsQueued => "jobs.queued",
            Text::JobsFailed => "jobs.failed",
            Text::JobsFinished => "jobs.finished",
            Text::JobKindName(kind) => return format!("job.kind.{}", kind.code()).into(),
            Text::JobStateName(state) => return format!("job.state.{}", state.code()).into(),
            Text::JobFailureKindName(kind) => {
                return format!("job.failure.{}", kind.code()).into();
            }
            Text::JobRetryScheduled => "job.retry_scheduled",
            Text::JobFailedAfter => "job.failed_after",
            Text::CancelJob => "job.cancel",
            Text::RetryJob => "job.retry",
            Text::JobNotUpdated => "job.error.not_updated",
            Text::JobNotStarted => "job.error.not_started",
            Text::TestJobsTitle => "jobs.test.title",
            Text::TestJobsHint => "jobs.test.hint",
            Text::StartTestJob => "jobs.test.start",
            Text::StartFailingTestJob => "jobs.test.start_failing",
            Text::SettingsTitle => "settings.title",
            Text::ProviderKeysTitle => "provider_keys.title",
            Text::ProviderKeysHint => "provider_keys.hint",
            Text::ProviderName(provider) => {
                return format!("provider.{}.name", provider.code()).into();
            }
            Text::ProviderPurpose(provider) => {
                return format!("provider.{}.purpose", provider.code()).into();
            }
            Text::ProviderKeyPlaceholder(provider) => {
                return format!("provider.{}.placeholder", provider.code()).into();
            }
            Text::KeyNotSet => "provider_keys.not_set",
            Text::KeySaved => "provider_keys.saved",
            Text::KeyUnreadable => "provider_keys.unreadable",
            Text::SaveKey => "provider_keys.save",
            Text::ReplaceKey => "provider_keys.replace",
            Text::TestKey => "provider_keys.test",
            Text::TestingKey => "provider_keys.testing",
            Text::RemoveKey => "provider_keys.remove",
            Text::KeyCheckOutcome(outcome) => {
                return format!("key_check.{}", outcome.code()).into();
            }
            Text::KeyCheckDetail => "key_check.detail",
            Text::ApiKeyError(error) => match error {
                ApiKeyError::Required => "api_key.error.required",
                ApiKeyError::TooShort => "api_key.error.too_short",
                ApiKeyError::TooLong => "api_key.error.too_long",
                ApiKeyError::InvalidCharacters => "api_key.error.invalid_characters",
                ApiKeyError::NotIdAndSecret => "api_key.error.not_id_and_secret",
            },
            Text::ProviderKeyNotSet => "provider_keys.error.not_set",
            Text::ProviderKeyStoreFailed => "provider_keys.error.store_failed",
            Text::ResearchTitle => "research.title",
            Text::ResearchNoChannels => "research.no_channels",
            Text::ResearchChannel => "research.channel",
            Text::ResearchSeeds => "research.seeds",
            Text::ResearchSeedsPlaceholder => "research.seeds_placeholder",
            Text::ResearchSeedsHint => "research.seeds_hint",
            Text::RunResearch => "research.run",
            Text::RefreshResearch => "research.refresh",
            Text::RefreshResearchCost => "research.refresh_cost",
            Text::ResearchCost => "research.cost",
            Text::ResearchCostNone => "research.cost_none",
            Text::ResearchMissingKey => "research.error.missing_key",
            Text::ResearchNotStarted => "research.error.not_started",
            Text::ResearchNotLoaded => "research.error.not_loaded",
            Text::ResearchRunning => "research.running",
            Text::ResearchStopped => "research.stopped",
            Text::NicheSeedError(error) => match error {
                NicheSeedError::Required => "research.error.seed_required",
                NicheSeedError::TooMany => "research.error.too_many_seeds",
                NicheSeedError::TooLong => "research.error.seed_too_long",
            },
            Text::ResearchResultsTitle => "research.results",
            Text::ResearchScoresHint => "research.scores_hint",
            Text::ResearchOpportunity => "research.opportunity",
            Text::ResearchCompetition => "research.competition",
            Text::ResearchTrend => "research.trend",
            Text::ResearchUploads => "research.uploads",
            Text::ResearchMedianViews => "research.median_views",
            Text::ResearchViewsPerDay => "research.views_per_day",
            Text::ResearchMedianSubscribers => "research.median_subscribers",
            Text::ResearchSmallChannels => "research.small_channels",
            Text::ResearchSample => "research.sample",
            Text::ResearchFetched => "research.fetched",
            Text::ResearchNotFetched => "research.not_fetched",
            Text::ResearchNoUploads => "research.no_uploads",
            Text::ResearchStale => "research.stale",
            Text::FetchedJustNow => "age.just_now",
            Text::FetchedMinutesAgo => "age.minutes",
            Text::FetchedHoursAgo => "age.hours",
            Text::FetchedDaysAgo => "age.days",
            Text::NumberThousands => "number.thousands",
            Text::NumberMillions => "number.millions",
            Text::NumberBillions => "number.billions",
            Text::DecimalSeparator => "number.decimal_separator",
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

    /// The string for `text` with each `{name}` replaced by its value.
    pub fn format(&self, text: Text, args: &[(&str, &str)]) -> String {
        args.iter()
            .fold(self.get(text).into_owned(), |out, (name, value)| {
                out.replace(&format!("{{{name}}}"), value)
            })
    }

    /// A count in a few characters: `950`, `8.7K`, `48K`, `1.8M` in en-US.
    /// One decimal below ten units, cut rather than rounded, so `999,999`
    /// never shows as `1000K`.
    pub fn compact(&self, n: u64) -> String {
        let (unit, text) = match n {
            0..1_000 => return n.to_string(),
            1_000..1_000_000 => (1_000, Text::NumberThousands),
            1_000_000..1_000_000_000 => (1_000_000, Text::NumberMillions),
            _ => (1_000_000_000, Text::NumberBillions),
        };
        let tenths = n / (unit / 10);
        let number = if tenths < 100 && !tenths.is_multiple_of(10) {
            format!(
                "{}{}{}",
                tenths / 10,
                self.get(Text::DecimalSeparator),
                tenths % 10
            )
        } else {
            (tenths / 10).to_string()
        };
        self.format(text, &[("n", &number)])
    }

    /// How long ago something happened, e.g. `3 h ago`.
    pub fn age(&self, elapsed: Duration) -> String {
        let minutes = elapsed.as_secs() / 60;
        let (text, n) = match minutes {
            0 => return self.get(Text::FetchedJustNow).into_owned(),
            1..60 => (Text::FetchedMinutesAgo, minutes),
            60..2_880 => (Text::FetchedHoursAgo, minutes / 60),
            _ => (Text::FetchedDaysAgo, minutes / 1_440),
        };
        self.format(text, &[("n", &n.to_string())])
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
            Text::JobsTitle,
            Text::JobsEmpty,
            Text::JobsRunning,
            Text::JobsQueued,
            Text::JobsFailed,
            Text::JobsFinished,
            Text::JobRetryScheduled,
            Text::JobFailedAfter,
            Text::CancelJob,
            Text::RetryJob,
            Text::JobNotUpdated,
            Text::JobNotStarted,
            Text::TestJobsTitle,
            Text::TestJobsHint,
            Text::StartTestJob,
            Text::StartFailingTestJob,
            Text::SettingsTitle,
            Text::ProviderKeysTitle,
            Text::ProviderKeysHint,
            Text::KeyNotSet,
            Text::KeySaved,
            Text::KeyUnreadable,
            Text::SaveKey,
            Text::ReplaceKey,
            Text::TestKey,
            Text::TestingKey,
            Text::RemoveKey,
            Text::KeyCheckDetail,
            Text::ProviderKeyNotSet,
            Text::ProviderKeyStoreFailed,
            Text::ResearchTitle,
            Text::ResearchNoChannels,
            Text::ResearchChannel,
            Text::ResearchSeeds,
            Text::ResearchSeedsPlaceholder,
            Text::ResearchSeedsHint,
            Text::RunResearch,
            Text::RefreshResearch,
            Text::RefreshResearchCost,
            Text::ResearchCost,
            Text::ResearchCostNone,
            Text::ResearchMissingKey,
            Text::ResearchNotStarted,
            Text::ResearchNotLoaded,
            Text::ResearchRunning,
            Text::ResearchStopped,
            Text::ResearchResultsTitle,
            Text::ResearchScoresHint,
            Text::ResearchOpportunity,
            Text::ResearchCompetition,
            Text::ResearchTrend,
            Text::ResearchUploads,
            Text::ResearchMedianViews,
            Text::ResearchViewsPerDay,
            Text::ResearchMedianSubscribers,
            Text::ResearchSmallChannels,
            Text::ResearchSample,
            Text::ResearchFetched,
            Text::ResearchNotFetched,
            Text::ResearchNoUploads,
            Text::ResearchStale,
            Text::FetchedJustNow,
            Text::FetchedMinutesAgo,
            Text::FetchedHoursAgo,
            Text::FetchedDaysAgo,
            Text::NumberThousands,
            Text::NumberMillions,
            Text::NumberBillions,
            Text::DecimalSeparator,
        ];
        texts.extend(NicheSeedError::ALL.map(Text::NicheSeedError));
        texts.extend(Provider::ALL.map(Text::ProviderName));
        texts.extend(Provider::ALL.map(Text::ProviderPurpose));
        texts.extend(Provider::ALL.map(Text::ProviderKeyPlaceholder));
        texts.extend(KeyCheckOutcome::ALL.map(Text::KeyCheckOutcome));
        texts.extend(ApiKeyError::ALL.map(Text::ApiKeyError));
        texts.extend(UiLanguage::ALL.map(Text::LanguageName));
        texts.extend(JobKind::ALL.map(Text::JobKindName));
        texts.extend(JobState::ALL.map(Text::JobStateName));
        texts.extend(JobFailureKind::ALL.map(Text::JobFailureKindName));
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
    fn placeholders_are_filled_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            let text = catalog.format(Text::JobRetryScheduled, &[("attempt", "2"), ("max", "4")]);
            assert!(text.contains('2') && text.contains('4'), "{text}");
            assert!(!text.contains('{'), "{text}");
            let text = catalog.format(Text::JobFailedAfter, &[("attempts", "4")]);
            assert!(text.contains('4') && !text.contains('{'), "{text}");
            let text = catalog.format(Text::KeySaved, &[("hint", "…abcd")]);
            assert!(text.contains("…abcd") && !text.contains('{'), "{text}");
            let text = catalog.format(Text::KeyCheckDetail, &[("detail", "Invalid API key")]);
            assert!(
                text.contains("Invalid API key") && !text.contains('{'),
                "{text}"
            );
        }
    }

    #[test]
    fn key_length_message_matches_the_domain_limit() {
        let catalog = Catalog::load(UiLanguage::EnUs);
        let message = catalog.get(Text::ApiKeyError(ApiKeyError::TooLong));
        assert!(
            message.contains(&bardo_domain::ApiKey::MAX_CHARS.to_string()),
            "{message}"
        );
    }

    #[test]
    fn seed_limit_messages_match_the_domain_limits() {
        use bardo_domain::{Niche, NicheSeeds};

        let catalog = Catalog::load(UiLanguage::EnUs);
        let too_many = catalog.get(Text::NicheSeedError(NicheSeedError::TooMany));
        assert!(
            too_many.contains(&NicheSeeds::MAX.to_string()),
            "{too_many}"
        );
        let too_long = catalog.get(Text::NicheSeedError(NicheSeedError::TooLong));
        assert!(
            too_long.contains(&Niche::MAX_CHARS.to_string()),
            "{too_long}"
        );
    }

    #[test]
    fn research_placeholders_are_filled_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for (text, args) in [
                (Text::ResearchCost, &[("units", "204")][..]),
                (Text::RefreshResearchCost, &[("units", "510")][..]),
                (
                    Text::ResearchSample,
                    &[("videos", "47"), ("channels", "38")][..],
                ),
            ] {
                let filled = catalog.format(text, args);
                assert!(!filled.contains('{'), "{filled}");
                for (_, value) in args {
                    assert!(filled.contains(value), "{filled}");
                }
            }
        }
    }

    #[test]
    fn counts_are_compact_and_localized() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        let cases = [
            (0, "0", "0"),
            (999, "999", "999"),
            (1_000, "1K", "1 mil"),
            (8_790, "8.7K", "8,7 mil"),
            (48_213, "48K", "48 mil"),
            (999_999, "999K", "999 mil"),
            (1_840_000, "1.8M", "1,8 mi"),
            (12_000_000, "12M", "12 mi"),
            (2_100_000_000, "2.1B", "2,1 bi"),
        ];
        for (n, english, portuguese) in cases {
            assert_eq!(en.compact(n), english);
            assert_eq!(pt.compact(n), portuguese);
        }
    }

    #[test]
    fn ages_pick_a_readable_unit() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        let minutes = |m: u64| Duration::from_secs(m * 60);
        assert_eq!(en.age(Duration::from_secs(30)), "just now");
        assert_eq!(en.age(minutes(5)), "5 min ago");
        assert_eq!(en.age(minutes(90)), "1 h ago");
        assert_eq!(en.age(minutes(47 * 60)), "47 h ago");
        assert_eq!(en.age(minutes(3 * 1_440)), "3 days ago");
        assert_eq!(pt.age(minutes(3 * 1_440)), "há 3 dias");
        assert_eq!(pt.age(minutes(5)), "há 5 min");
    }

    #[test]
    fn strings_differ_between_languages() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        assert_ne!(en.get(Text::AppTagline), pt.get(Text::AppTagline));
    }
}
