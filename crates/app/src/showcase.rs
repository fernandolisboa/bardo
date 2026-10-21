//! The look job's walk through Bardo's screens (issue #166): every place in
//! both layouts and both languages, the ten themes on a few busy places,
//! and every place again in the narrowest window. A build of `bardo-ui`
//! with its `showcase` feature shows the shots one after another; the
//! job's script takes each screenshot and checks it against what the shot
//! expects. Nothing here runs in a build a user gets.

use bardo_domain::{LayoutId, ThemeMode, UiLanguage, UiTheme, UiThemePreference};
use serde::Serialize;

use crate::{Destination, EditorPalette, GuidePlace, SettingsTab, Stage, palette};

/// A place a shot shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShowcasePlace {
    /// A screen, a project stage or a Settings tab, as guide links name
    /// them. The Edit stage is the editor, over the whole window.
    Place(GuidePlace),
    /// The user guide's own screen.
    Guide,
}

impl ShowcasePlace {
    /// Every place: the screens, the stages and the Settings tabs in the
    /// guide's order, then the Guide screen. The Projects and Settings
    /// screens always show one of their stages or tabs, which stand for
    /// them.
    pub fn all() -> Vec<ShowcasePlace> {
        GuidePlace::all()
            .into_iter()
            .filter(|place| {
                !matches!(
                    place,
                    GuidePlace::Screen(Destination::Projects | Destination::Settings)
                )
            })
            .map(ShowcasePlace::Place)
            .chain([ShowcasePlace::Guide])
            .collect()
    }

    /// Its name in file names: `research`, `projects-script`,
    /// `settings-keys`, `guide`.
    pub fn code(self) -> String {
        match self {
            ShowcasePlace::Place(place) => place.code().replace('/', "-"),
            ShowcasePlace::Guide => Destination::Guide.code().to_owned(),
        }
    }
}

/// The window's inner size in a shot, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct ShotSize {
    pub width: u32,
    pub height: u32,
}

impl ShotSize {
    /// As large as a 1024 × 768 screen holds beside the taskbar: the
    /// Windows runner's screen.
    pub const WIDE: ShotSize = ShotSize {
        width: 1000,
        height: 700,
    };
    /// The smallest Bardo's window goes (issue #151).
    pub const NARROW: ShotSize = ShotSize {
        width: 640,
        height: 600,
    };
}

/// One screenshot: a place in a layout, theme, language and window size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shot {
    pub layout: LayoutId,
    pub theme: UiTheme,
    pub language: UiLanguage,
    pub size: ShotSize,
    pub place: ShowcasePlace,
}

/// The theme the places and narrow shots are taken in: the one a light
/// system shows a new profile.
pub fn base_theme() -> UiTheme {
    UiThemePreference::default().resolve(ThemeMode::Light)
}

/// The places every theme is shown on: the busiest ones, with lists,
/// cards, charts, inputs and the timeline.
pub const THEMED_PLACES: [ShowcasePlace; 4] = [
    ShowcasePlace::Place(GuidePlace::Stage(Stage::Render)),
    ShowcasePlace::Place(GuidePlace::Stage(Stage::Edit)),
    ShowcasePlace::Place(GuidePlace::Screen(Destination::Costs)),
    ShowcasePlace::Place(GuidePlace::Settings(SettingsTab::Appearance)),
];

impl Shot {
    /// The shots in the order they are shown, each once: every place in
    /// both layouts and languages; every theme on [`THEMED_PLACES`] in
    /// both layouts; every place in both layouts at the narrowest size, in
    /// Portuguese, whose words run longest.
    pub fn plan() -> Vec<Shot> {
        let base = base_theme();
        let mut shots = Vec::new();
        for language in UiLanguage::ALL {
            for layout in LayoutId::ALL {
                for place in ShowcasePlace::all() {
                    shots.push(Shot {
                        layout,
                        theme: base,
                        language,
                        size: ShotSize::WIDE,
                        place,
                    });
                }
            }
        }
        for layout in LayoutId::ALL {
            for theme in UiTheme::ALL {
                for place in THEMED_PLACES {
                    shots.push(Shot {
                        layout,
                        theme,
                        language: UiLanguage::EnUs,
                        size: ShotSize::WIDE,
                        place,
                    });
                }
            }
        }
        for layout in LayoutId::ALL {
            for place in ShowcasePlace::all() {
                shots.push(Shot {
                    layout,
                    theme: base,
                    language: UiLanguage::PtBr,
                    size: ShotSize::NARROW,
                    place,
                });
            }
        }
        let mut seen = std::collections::HashSet::new();
        shots.retain(|shot| seen.insert(*shot));
        shots
    }

    /// Its file name, without the extension:
    /// `workspace-paper-en-US-1000x700-settings-keys`.
    pub fn name(&self) -> String {
        format!(
            "{}-{}-{}-{}x{}-{}",
            self.layout.code(),
            self.theme.code(),
            self.language.tag(),
            self.size.width,
            self.size.height,
            self.place.code()
        )
    }

    /// What the window shows once the shot is on screen, for the script to
    /// check the screenshot against.
    pub fn ready(&self, title: &str) -> ShotReady {
        // The editor keeps its own darker grounds in every theme.
        let grounds = if self.place == ShowcasePlace::Place(GuidePlace::Stage(Stage::Edit)) {
            let colors = EditorPalette::for_theme(self.theme);
            [colors.app, colors.panel, colors.raised, colors.hairline]
        } else {
            let colors = palette(self.theme);
            [colors.app, colors.surface, colors.raised, colors.sunken]
        };
        ShotReady {
            name: self.name(),
            layout: self.layout.code(),
            theme: self.theme.code(),
            language: self.language.tag(),
            place: self.place.code(),
            width: self.size.width,
            height: self.size.height,
            title: title.to_owned(),
            grounds: grounds.map(|color| format!("#{:06X}", color.0)).to_vec(),
        }
    }
}

/// A shot on screen, as the window reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShotReady {
    pub name: String,
    pub layout: &'static str,
    pub theme: &'static str,
    pub language: &'static str,
    pub place: String,
    pub width: u32,
    pub height: u32,
    /// The window's title.
    pub title: String,
    /// The theme's grounds (window, cards, controls, wells; the editor's
    /// own in the editor): most of a screenshot is one of them.
    pub grounds: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_place_is_shown_in_both_layouts_and_languages() {
        let plan = Shot::plan();
        for language in UiLanguage::ALL {
            for layout in LayoutId::ALL {
                for place in ShowcasePlace::all() {
                    assert!(
                        plan.iter().any(|shot| shot.language == language
                            && shot.layout == layout
                            && shot.place == place
                            && shot.size == ShotSize::WIDE),
                        "{language:?} {layout:?} {place:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_theme_is_shown_in_both_layouts() {
        let plan = Shot::plan();
        for theme in UiTheme::ALL {
            for layout in LayoutId::ALL {
                for place in THEMED_PLACES {
                    assert!(
                        plan.iter().any(|shot| shot.theme == theme
                            && shot.layout == layout
                            && shot.place == place),
                        "{theme:?} {layout:?} {place:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_place_is_shown_at_the_narrowest_size() {
        let plan = Shot::plan();
        for layout in LayoutId::ALL {
            for place in ShowcasePlace::all() {
                assert!(plan.iter().any(|shot| shot.layout == layout
                    && shot.place == place
                    && shot.size == ShotSize::NARROW));
            }
        }
    }

    #[test]
    fn names_are_unique_and_safe_in_file_names() {
        let plan = Shot::plan();
        let names: std::collections::HashSet<_> = plan.iter().map(Shot::name).collect();
        assert_eq!(names.len(), plan.len());
        for name in names {
            assert!(
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "{name}"
            );
        }
    }

    #[test]
    fn the_editor_and_the_guide_are_places() {
        let places = ShowcasePlace::all();
        assert!(places.contains(&ShowcasePlace::Place(GuidePlace::Stage(Stage::Edit))));
        assert!(places.contains(&ShowcasePlace::Guide));
        assert!(!places.contains(&ShowcasePlace::Place(GuidePlace::Screen(
            Destination::Settings
        ))));
        assert_eq!(
            ShowcasePlace::Place(GuidePlace::Settings(SettingsTab::Keys)).code(),
            "settings-keys"
        );
    }

    #[test]
    fn a_ready_shot_carries_its_themes_grounds() {
        let shot = Shot {
            layout: LayoutId::Studio,
            theme: UiTheme::BlackGold,
            language: UiLanguage::PtBr,
            size: ShotSize::NARROW,
            place: ShowcasePlace::Guide,
        };
        let ready = shot.ready("Bardo");
        assert_eq!(ready.name, "studio-black-gold-pt-BR-640x600-guide");
        assert_eq!(ready.title, "Bardo");
        let app = palette(UiTheme::BlackGold).app;
        assert_eq!(ready.grounds[0], format!("#{:06X}", app.0));
        assert_eq!(ready.grounds.len(), 4);
    }

    /// The screenshot check tells themes apart by their window ground.
    #[test]
    fn every_theme_has_its_own_window_ground() {
        let grounds: std::collections::HashSet<_> = UiTheme::ALL
            .into_iter()
            .map(|theme| palette(theme).app)
            .collect();
        assert_eq!(grounds.len(), UiTheme::ALL.len());
    }
}
