//! The audience a channel targets: content language and country.
//!
//! Both are closed lists so every value has a translated name in the UI and
//! maps onto what the YouTube Data API accepts (`relevanceLanguage` takes
//! ISO 639-1, `regionCode` takes ISO 3166-1 alpha-2). Adding a market is
//! adding a variant here and its name to the locale files.

use std::fmt;
use std::str::FromStr;

/// Language a channel's videos are made in (ISO 639-1). Independent of the
/// app's own interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ContentLanguage {
    #[default]
    English,
    Portuguese,
    Spanish,
    German,
    French,
}

impl ContentLanguage {
    pub const ALL: [ContentLanguage; 5] = [
        ContentLanguage::English,
        ContentLanguage::Portuguese,
        ContentLanguage::Spanish,
        ContentLanguage::German,
        ContentLanguage::French,
    ];

    /// ISO 639-1 code, lowercase.
    pub fn code(self) -> &'static str {
        match self {
            ContentLanguage::English => "en",
            ContentLanguage::Portuguese => "pt",
            ContentLanguage::Spanish => "es",
            ContentLanguage::German => "de",
            ContentLanguage::French => "fr",
        }
    }
}

impl fmt::Display for ContentLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported content language: {0}")]
pub struct UnsupportedContentLanguage(pub String);

impl FromStr for ContentLanguage {
    type Err = UnsupportedContentLanguage;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ContentLanguage::ALL
            .into_iter()
            .find(|language| language.code().eq_ignore_ascii_case(s))
            .ok_or_else(|| UnsupportedContentLanguage(s.to_owned()))
    }
}

/// Country a channel targets (ISO 3166-1 alpha-2). The list starts with the
/// launch markets (US, then BR) and other high-CPM markets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Country {
    #[default]
    UnitedStates,
    Brazil,
    Canada,
    UnitedKingdom,
    Australia,
    NewZealand,
    Ireland,
    Germany,
    France,
    Spain,
    Mexico,
    Portugal,
}

impl Country {
    pub const ALL: [Country; 12] = [
        Country::UnitedStates,
        Country::Brazil,
        Country::Canada,
        Country::UnitedKingdom,
        Country::Australia,
        Country::NewZealand,
        Country::Ireland,
        Country::Germany,
        Country::France,
        Country::Spain,
        Country::Mexico,
        Country::Portugal,
    ];

    /// ISO 3166-1 alpha-2 code, uppercase.
    pub fn code(self) -> &'static str {
        match self {
            Country::UnitedStates => "US",
            Country::Brazil => "BR",
            Country::Canada => "CA",
            Country::UnitedKingdom => "GB",
            Country::Australia => "AU",
            Country::NewZealand => "NZ",
            Country::Ireland => "IE",
            Country::Germany => "DE",
            Country::France => "FR",
            Country::Spain => "ES",
            Country::Mexico => "MX",
            Country::Portugal => "PT",
        }
    }
}

impl fmt::Display for Country {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported country: {0}")]
pub struct UnsupportedCountry(pub String);

impl FromStr for Country {
    type Err = UnsupportedCountry;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Country::ALL
            .into_iter()
            .find(|country| country.code().eq_ignore_ascii_case(s))
            .ok_or_else(|| UnsupportedCountry(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn language_codes_round_trip() {
        for language in ContentLanguage::ALL {
            assert_eq!(language.code().parse(), Ok(language));
        }
    }

    #[test]
    fn language_codes_are_iso_639_1() {
        for language in ContentLanguage::ALL {
            let code = language.code();
            assert!(
                code.len() == 2 && code.chars().all(|c| c.is_ascii_lowercase()),
                "{code}"
            );
        }
    }

    #[test]
    fn unknown_language_code_is_rejected() {
        assert_eq!(
            "xx".parse::<ContentLanguage>(),
            Err(UnsupportedContentLanguage("xx".into()))
        );
        assert!("en-US".parse::<ContentLanguage>().is_err());
    }

    #[test]
    fn country_codes_round_trip_ignoring_case() {
        for country in Country::ALL {
            assert_eq!(country.code().parse(), Ok(country));
            assert_eq!(country.code().to_lowercase().parse(), Ok(country));
        }
    }

    #[test]
    fn country_codes_are_iso_3166_alpha_2() {
        for country in Country::ALL {
            let code = country.code();
            assert!(
                code.len() == 2 && code.chars().all(|c| c.is_ascii_uppercase()),
                "{code}"
            );
        }
    }

    #[test]
    fn unknown_country_code_is_rejected() {
        assert_eq!(
            "ZZ".parse::<Country>(),
            Err(UnsupportedCountry("ZZ".into()))
        );
        assert!("USA".parse::<Country>().is_err());
    }

    #[test]
    fn codes_are_unique() {
        let languages: HashSet<_> = ContentLanguage::ALL.map(ContentLanguage::code).into();
        assert_eq!(languages.len(), ContentLanguage::ALL.len());
        let countries: HashSet<_> = Country::ALL.map(Country::code).into();
        assert_eq!(countries.len(), Country::ALL.len());
    }

    #[test]
    fn defaults_are_the_first_launch_market() {
        assert_eq!(ContentLanguage::default(), ContentLanguage::English);
        assert_eq!(Country::default(), Country::UnitedStates);
    }
}
