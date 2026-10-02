//! How the app's own interface looks: the interface theme and how the
//! profile picks it (CONTEXT.md, "Interface theme"). Not to be confused with
//! `Theme`, a video idea.

use std::fmt;
use std::str::FromStr;

/// One of the interface themes Bardo ships. A theme is colors, corner
/// radius, border width and font; it never moves anything on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiTheme {
    Paper,
    Sand,
    Graphite,
    Slate,
    HighContrastLight,
    HighContrastDark,
    BlackGold,
    Brass,
    Phosphor,
    PhosphorLight,
}

/// Light or dark: which slot of "follow the system" a theme fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    Light,
    Dark,
}

/// The theme's family: `Base` themes share the amber accent and rounded
/// corners; `Terminal` themes bring their own accent, square corners,
/// framed panels and a monospace font. High contrast is a base theme with
/// stronger pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeFamily {
    Base,
    HighContrast,
    Terminal,
}

impl UiTheme {
    pub const ALL: [UiTheme; 10] = [
        UiTheme::Paper,
        UiTheme::Sand,
        UiTheme::Graphite,
        UiTheme::Slate,
        UiTheme::HighContrastLight,
        UiTheme::HighContrastDark,
        UiTheme::BlackGold,
        UiTheme::Brass,
        UiTheme::Phosphor,
        UiTheme::PhosphorLight,
    ];

    /// Stable code, used in storage and as the key of the theme's colors.
    pub fn code(self) -> &'static str {
        match self {
            UiTheme::Paper => "paper",
            UiTheme::Sand => "sand",
            UiTheme::Graphite => "graphite",
            UiTheme::Slate => "slate",
            UiTheme::HighContrastLight => "hc-light",
            UiTheme::HighContrastDark => "hc-dark",
            UiTheme::BlackGold => "black-gold",
            UiTheme::Brass => "brass",
            UiTheme::Phosphor => "phosphor",
            UiTheme::PhosphorLight => "phosphor-light",
        }
    }

    pub fn mode(self) -> ThemeMode {
        match self {
            UiTheme::Paper
            | UiTheme::Sand
            | UiTheme::HighContrastLight
            | UiTheme::Brass
            | UiTheme::PhosphorLight => ThemeMode::Light,
            UiTheme::Graphite
            | UiTheme::Slate
            | UiTheme::HighContrastDark
            | UiTheme::BlackGold
            | UiTheme::Phosphor => ThemeMode::Dark,
        }
    }

    pub fn family(self) -> ThemeFamily {
        match self {
            UiTheme::Paper | UiTheme::Sand | UiTheme::Graphite | UiTheme::Slate => {
                ThemeFamily::Base
            }
            UiTheme::HighContrastLight | UiTheme::HighContrastDark => ThemeFamily::HighContrast,
            UiTheme::BlackGold | UiTheme::Brass | UiTheme::Phosphor | UiTheme::PhosphorLight => {
                ThemeFamily::Terminal
            }
        }
    }

    /// The themes that fit one slot of "follow the system".
    pub fn of_mode(mode: ThemeMode) -> impl Iterator<Item = UiTheme> {
        UiTheme::ALL
            .into_iter()
            .filter(move |theme| theme.mode() == mode)
    }
}

impl fmt::Display for UiTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown interface theme: {0}")]
pub struct UnknownUiTheme(pub String);

impl FromStr for UiTheme {
    type Err = UnknownUiTheme;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UiTheme::ALL
            .into_iter()
            .find(|theme| theme.code() == s)
            .ok_or_else(|| UnknownUiTheme(s.to_owned()))
    }
}

/// How a profile picks its theme: follow the system's light/dark setting
/// with a theme for each, or always the same theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiThemePreference {
    FollowSystem { light: UiTheme, dark: UiTheme },
    Fixed(UiTheme),
}

impl Default for UiThemePreference {
    fn default() -> Self {
        UiThemePreference::FollowSystem {
            light: UiTheme::Paper,
            dark: UiTheme::Graphite,
        }
    }
}

impl UiThemePreference {
    /// The theme to show while the system appearance is `system`.
    pub fn resolve(self, system: ThemeMode) -> UiTheme {
        match self {
            UiThemePreference::FollowSystem { light, dark } => match system {
                ThemeMode::Light => light,
                ThemeMode::Dark => dark,
            },
            UiThemePreference::Fixed(theme) => theme,
        }
    }

    /// Stored form: `system:<light>:<dark>` or `fixed:<theme>`.
    pub fn code(self) -> String {
        match self {
            UiThemePreference::FollowSystem { light, dark } => format!("system:{light}:{dark}"),
            UiThemePreference::Fixed(theme) => format!("fixed:{theme}"),
        }
    }

    /// Reads the stored form; anything this version does not know (a theme
    /// removed later, a hand-edited value) gives the default.
    pub fn from_code_or_default(code: &str) -> Self {
        code.parse().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown interface theme preference: {0}")]
pub struct UnknownUiThemePreference(pub String);

impl FromStr for UiThemePreference {
    type Err = UnknownUiThemePreference;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let unknown = || UnknownUiThemePreference(s.to_owned());
        let parts: Vec<&str> = s.split(':').collect();
        match parts.as_slice() {
            ["system", light, dark] => {
                let light: UiTheme = light.parse().map_err(|_| unknown())?;
                let dark: UiTheme = dark.parse().map_err(|_| unknown())?;
                // Each slot takes a theme of its own mode.
                if light.mode() != ThemeMode::Light || dark.mode() != ThemeMode::Dark {
                    return Err(unknown());
                }
                Ok(UiThemePreference::FollowSystem { light, dark })
            }
            ["fixed", theme] => Ok(UiThemePreference::Fixed(
                theme.parse().map_err(|_| unknown())?,
            )),
            _ => Err(unknown()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_codes_round_trip_and_are_distinct() {
        for theme in UiTheme::ALL {
            assert_eq!(theme.code().parse::<UiTheme>(), Ok(theme));
        }
        let mut codes: Vec<_> = UiTheme::ALL.iter().map(|t| t.code()).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), UiTheme::ALL.len());
    }

    #[test]
    fn five_light_and_five_dark_themes() {
        assert_eq!(UiTheme::of_mode(ThemeMode::Light).count(), 5);
        assert_eq!(UiTheme::of_mode(ThemeMode::Dark).count(), 5);
    }

    #[test]
    fn default_follows_the_system_with_paper_and_graphite() {
        let preference = UiThemePreference::default();
        assert_eq!(preference.resolve(ThemeMode::Light), UiTheme::Paper);
        assert_eq!(preference.resolve(ThemeMode::Dark), UiTheme::Graphite);
    }

    #[test]
    fn a_fixed_theme_ignores_the_system() {
        let preference = UiThemePreference::Fixed(UiTheme::BlackGold);
        assert_eq!(preference.resolve(ThemeMode::Light), UiTheme::BlackGold);
        assert_eq!(preference.resolve(ThemeMode::Dark), UiTheme::BlackGold);
    }

    #[test]
    fn preferences_round_trip_through_their_code() {
        let preferences = [
            UiThemePreference::default(),
            UiThemePreference::FollowSystem {
                light: UiTheme::Brass,
                dark: UiTheme::BlackGold,
            },
            UiThemePreference::Fixed(UiTheme::PhosphorLight),
        ];
        for preference in preferences {
            assert_eq!(preference.code().parse(), Ok(preference));
        }
        assert_eq!(UiThemePreference::default().code(), "system:paper:graphite");
        assert_eq!(
            UiThemePreference::Fixed(UiTheme::BlackGold).code(),
            "fixed:black-gold"
        );
    }

    #[test]
    fn unknown_codes_fall_back_to_the_default() {
        for code in [
            "",
            "fixed:neon",
            "system:paper",
            "system:graphite:paper",
            "auto",
            "fixed:paper:extra",
        ] {
            assert_eq!(
                UiThemePreference::from_code_or_default(code),
                UiThemePreference::default(),
                "{code}"
            );
        }
    }
}
