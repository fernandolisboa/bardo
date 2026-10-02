//! The interface themes' colors (issue #59) and the editor's palette under
//! each of them. Plain data so contrast is checked without a window; the UI
//! maps it onto its toolkit.

use std::collections::HashMap;
use std::sync::OnceLock;

use bardo_domain::{ThemeFamily, ThemeMode, UiTheme};
use serde::{Deserialize, Deserializer};

/// Every theme's tokens, keyed by `UiTheme::code`.
const THEMES: &str = include_str!("../data/themes.json");

/// An sRGB color, `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u32);

impl Rgb {
    fn channel(self, shift: u32) -> f64 {
        let value = f64::from((self.0 >> shift) & 0xFF) / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    /// WCAG 2.x relative luminance.
    pub fn luminance(self) -> f64 {
        0.2126 * self.channel(16) + 0.7152 * self.channel(8) + 0.0722 * self.channel(0)
    }

    /// WCAG 2.x contrast ratio between two colors (1 to 21).
    pub fn contrast(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.strip_prefix('#')
            .filter(|hex| hex.len() == 6)
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .map(Rgb)
            .ok_or_else(|| serde::de::Error::custom(format!("not a #RRGGBB color: {text}")))
    }
}

/// The font a theme draws its interface in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiFont {
    /// The platform's interface font (Segoe UI on Windows).
    System,
    /// A family Bardo embeds, by name.
    Embedded(String),
}

impl<'de> Deserialize<'de> for UiFont {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(if name == "system" {
            UiFont::System
        } else {
            UiFont::Embedded(name)
        })
    }
}

/// One theme's semantic tokens. Surfaces go from darkest to lightest in
/// dark themes (`sunken` < `app` < `surface` < `raised`) and the other way
/// in light ones, so cards stand off the ground and wells sink into cards.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Palette {
    /// The window's ground.
    pub app: Rgb,
    /// Cards and panels.
    pub surface: Rgb,
    /// Controls and menus above a surface.
    pub raised: Rgb,
    /// Wells: prompts, read-only text, inputs.
    pub sunken: Rgb,
    pub hover: Rgb,
    /// A selected row or option (accent-tinted).
    pub selected: Rgb,
    /// Dividers.
    pub border: Rgb,
    /// The outline of panels and cards (accent-tinted in terminal themes).
    pub frame: Rgb,
    /// The outline of controls.
    pub border_strong: Rgb,
    pub text: Rgb,
    pub text2: Rgb,
    pub text3: Rgb,
    pub accent: Rgb,
    /// Text and icons on an accent fill.
    pub on_accent: Rgb,
    /// Accent-colored text and links.
    pub accent_text: Rgb,
    /// The outline of accent-filled buttons.
    pub accent_edge: Rgb,
    pub focus: Rgb,
    pub success: Rgb,
    pub success_bg: Rgb,
    pub warning: Rgb,
    pub warning_bg: Rgb,
    /// Meters and bars that warn.
    pub warning_fill: Rgb,
    pub danger: Rgb,
    pub danger_bg: Rgb,
    pub info: Rgb,
    pub info_bg: Rgb,
    /// Corner radius of controls, in px (0 in terminal themes).
    pub radius: u8,
    /// Outline width in px (2 in high contrast).
    pub border_width: u8,
    pub font: UiFont,
}

/// The tokens of `theme`.
pub fn palette(theme: UiTheme) -> &'static Palette {
    static PALETTES: OnceLock<HashMap<String, Palette>> = OnceLock::new();
    let palettes = PALETTES.get_or_init(|| {
        serde_json::from_str(THEMES).expect("the embedded themes are valid (tested)")
    });
    &palettes[theme.code()]
}

/// The editor's colors (docs/design/editor.md). Under light and base dark
/// themes the editor keeps its approved Graphite look; dark terminal themes
/// lend it their ground and accent, and high contrast its own pairs. The
/// track colors stay apart from each other and from the accent everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPalette {
    pub app: Rgb,
    pub panel: Rgb,
    pub raised: Rgb,
    pub raised_hover: Rgb,
    pub hairline: Rgb,
    pub outline: Rgb,
    pub text: Rgb,
    pub text2: Rgb,
    pub text3: Rgb,
    pub accent: Rgb,
    pub accent_ink: Rgb,
    pub error: Rgb,
    pub error_fill: Rgb,
    pub tracks: TrackColors,
}

/// The timeline's lane colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackColors {
    pub video_fill: Rgb,
    pub video_edge: Rgb,
    pub narration: Rgb,
    pub narration_fill: Rgb,
    pub music: Rgb,
    pub music_fill: Rgb,
    pub sfx_fill: Rgb,
    pub sfx_edge: Rgb,
    /// Caption chips, with `captions_ink` for their text.
    pub captions: Rgb,
    pub captions_ink: Rgb,
}

/// The approved editor design.
const GRAPHITE_EDITOR: EditorPalette = EditorPalette {
    app: Rgb(0x0F1113),
    panel: Rgb(0x16191C),
    raised: Rgb(0x1E2226),
    raised_hover: Rgb(0x262B30),
    hairline: Rgb(0x2A2F35),
    outline: Rgb(0x3A4048),
    text: Rgb(0xE7E9EC),
    text2: Rgb(0xA3AAB3),
    text3: Rgb(0x8A929C),
    accent: Rgb(0xF2A33A),
    accent_ink: Rgb(0x1A1206),
    error: Rgb(0xE5534B),
    error_fill: Rgb(0x2A1416),
    tracks: STANDARD_TRACKS,
};

const STANDARD_TRACKS: TrackColors = TrackColors {
    video_fill: Rgb(0x2B3D54),
    video_edge: Rgb(0x4F6E94),
    narration: Rgb(0x2FA295),
    narration_fill: Rgb(0x14302D),
    music: Rgb(0xBBAEF7),
    music_fill: Rgb(0x231F36),
    sfx_fill: Rgb(0x7A4230),
    sfx_edge: Rgb(0xC8664A),
    captions: Rgb(0xD2D6DC),
    captions_ink: Rgb(0x121417),
};

/// Brighter edges and deeper fills, for a black ground.
const HIGH_CONTRAST_DARK_TRACKS: TrackColors = TrackColors {
    video_fill: Rgb(0x16304F),
    video_edge: Rgb(0x8CCBFF),
    narration: Rgb(0x4FE0C8),
    narration_fill: Rgb(0x06302B),
    music: Rgb(0xD0C4FF),
    music_fill: Rgb(0x2A2050),
    sfx_fill: Rgb(0x5A2414),
    sfx_edge: Rgb(0xFF9E7A),
    captions: Rgb(0xFFFFFF),
    captions_ink: Rgb(0x000000),
};

/// Dark edges and pale fills, for a white ground.
const HIGH_CONTRAST_LIGHT_TRACKS: TrackColors = TrackColors {
    video_fill: Rgb(0xD6E6FA),
    video_edge: Rgb(0x0A3F8C),
    narration: Rgb(0x0B6158),
    narration_fill: Rgb(0xD4F0EC),
    music: Rgb(0x4B2A99),
    music_fill: Rgb(0xE6E0FA),
    sfx_fill: Rgb(0xFADBD0),
    sfx_edge: Rgb(0x9A3412),
    captions: Rgb(0x000000),
    captions_ink: Rgb(0xFFFFFF),
};

impl EditorPalette {
    pub fn for_theme(theme: UiTheme) -> EditorPalette {
        let p = palette(theme);
        match (theme.family(), theme.mode()) {
            (ThemeFamily::Terminal, ThemeMode::Dark) => EditorPalette {
                app: p.app,
                panel: p.surface,
                raised: p.raised,
                raised_hover: p.hover,
                hairline: p.border,
                outline: p.frame,
                text: p.text,
                text2: p.text2,
                text3: p.text3,
                accent: p.accent,
                accent_ink: p.on_accent,
                error: p.danger,
                error_fill: p.danger_bg,
                tracks: STANDARD_TRACKS,
            },
            (ThemeFamily::HighContrast, mode) => EditorPalette {
                app: p.app,
                panel: p.surface,
                raised: p.raised,
                raised_hover: p.hover,
                hairline: p.border,
                outline: p.border_strong,
                text: p.text,
                text2: p.text2,
                text3: p.text3,
                accent: p.accent,
                accent_ink: p.on_accent,
                error: p.danger,
                error_fill: p.danger_bg,
                tracks: match mode {
                    ThemeMode::Dark => HIGH_CONTRAST_DARK_TRACKS,
                    ThemeMode::Light => HIGH_CONTRAST_LIGHT_TRACKS,
                },
            },
            _ => GRAPHITE_EDITOR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text must reach this against every ground it sits on.
    fn text_minimum(theme: UiTheme) -> f64 {
        match theme.family() {
            ThemeFamily::HighContrast => 7.0,
            _ => 4.5,
        }
    }

    fn assert_contrast(theme: UiTheme, what: &str, fg: Rgb, bg: Rgb, minimum: f64) {
        let ratio = fg.contrast(bg);
        assert!(
            ratio >= minimum,
            "{theme}: {what} is {ratio:.2}:1, needs {minimum}:1"
        );
    }

    #[test]
    fn contrast_of_known_pairs() {
        assert!((Rgb(0x000000).contrast(Rgb(0xFFFFFF)) - 21.0).abs() < 1e-9);
        assert!((Rgb(0x777777).contrast(Rgb(0x777777)) - 1.0).abs() < 1e-9);
        // #767676 on white is the classic 4.54:1.
        assert!((Rgb(0x767676).contrast(Rgb(0xFFFFFF)) - 4.54).abs() < 0.01);
    }

    #[test]
    fn every_theme_is_embedded_with_its_mode() {
        for theme in UiTheme::ALL {
            let p = palette(theme);
            let dark = p.text.luminance() > p.surface.luminance();
            assert_eq!(dark, theme.mode() == ThemeMode::Dark, "{theme}");
            let terminal = theme.family() == ThemeFamily::Terminal;
            assert_eq!(p.radius == 0, terminal, "{theme}");
            assert_eq!(matches!(p.font, UiFont::Embedded(_)), terminal, "{theme}");
        }
        let embedded: HashMap<String, serde_json::Value> = serde_json::from_str(THEMES).unwrap();
        assert_eq!(embedded.len(), UiTheme::ALL.len());
    }

    #[test]
    fn every_theme_meets_wcag_contrast() {
        for theme in UiTheme::ALL {
            let p = palette(theme);
            let text = text_minimum(theme);
            let grounds = [("surface", p.surface), ("app", p.app), ("sunken", p.sunken)];
            let inks = [
                ("text", p.text),
                ("text2", p.text2),
                ("text3", p.text3),
                ("accent text", p.accent_text),
            ];
            for (ink_name, ink) in inks {
                for (ground_name, ground) in grounds {
                    let what = format!("{ink_name} on {ground_name}");
                    assert_contrast(theme, &what, ink, ground, text);
                }
            }
            for (name, ink) in [("text", p.text), ("text2", p.text2)] {
                assert_contrast(theme, &format!("{name} on selected"), ink, p.selected, text);
                assert_contrast(theme, &format!("{name} on hover"), ink, p.hover, text);
            }
            let statuses = [
                ("success", p.success, p.success_bg),
                ("warning", p.warning, p.warning_bg),
                ("danger", p.danger, p.danger_bg),
                ("info", p.info, p.info_bg),
            ];
            for (name, ink, tint) in statuses {
                assert_contrast(theme, &format!("{name} on surface"), ink, p.surface, text);
                assert_contrast(theme, &format!("{name} on its tint"), ink, tint, text);
            }
            assert_contrast(theme, "accent ink on accent", p.on_accent, p.accent, text);
            for (ground_name, ground) in [("surface", p.surface), ("app", p.app)] {
                let control = format!("control outline on {ground_name}");
                assert_contrast(theme, &control, p.border_strong, ground, 3.0);
                let ring = format!("focus ring on {ground_name}");
                assert_contrast(theme, &ring, p.focus, ground, 3.0);
            }
        }
    }

    #[test]
    fn editor_keeps_graphite_under_light_and_base_dark_themes() {
        for theme in [
            UiTheme::Paper,
            UiTheme::Sand,
            UiTheme::Graphite,
            UiTheme::Slate,
            UiTheme::Brass,
            UiTheme::PhosphorLight,
        ] {
            assert_eq!(EditorPalette::for_theme(theme), GRAPHITE_EDITOR, "{theme}");
        }
    }

    #[test]
    fn editor_takes_the_ground_and_accent_of_a_dark_terminal_theme() {
        let editor = EditorPalette::for_theme(UiTheme::BlackGold);
        let theme = palette(UiTheme::BlackGold);
        assert_eq!(editor.app, theme.app);
        assert_eq!(editor.accent, Rgb(0xD4AF37));
        assert_eq!(editor.accent_ink, theme.on_accent);
        assert_eq!(
            EditorPalette::for_theme(UiTheme::Phosphor).accent,
            Rgb(0x3DF57A)
        );
    }

    #[test]
    fn editor_uses_high_contrast_pairs_under_high_contrast() {
        for theme in [UiTheme::HighContrastLight, UiTheme::HighContrastDark] {
            let editor = EditorPalette::for_theme(theme);
            let p = palette(theme);
            assert_eq!(editor.app, p.app, "{theme}");
            assert_eq!(editor.text, p.text, "{theme}");
            for (what, ink) in [("text2", editor.text2), ("text3", editor.text3)] {
                assert_contrast(theme, &format!("editor {what}"), ink, editor.panel, 7.0);
            }
        }
    }

    #[test]
    fn editor_readable_and_tracks_distinguishable_under_every_theme() {
        for theme in UiTheme::ALL {
            let editor = EditorPalette::for_theme(theme);
            let minimum = text_minimum(theme);
            for (what, ink) in [("text", editor.text), ("text2", editor.text2)] {
                for (ground_name, ground) in [("app", editor.app), ("panel", editor.panel)] {
                    let what = format!("editor {what} on {ground_name}");
                    assert_contrast(theme, &what, ink, ground, minimum);
                }
            }
            assert_contrast(
                theme,
                "editor accent ink",
                editor.accent_ink,
                editor.accent,
                4.5,
            );
            let t = editor.tracks;
            assert_contrast(theme, "caption ink", t.captions_ink, t.captions, 4.5);
            // Lane marks stand off the editor's ground…
            let marks = [
                ("video", t.video_edge),
                ("narration", t.narration),
                ("music", t.music),
                ("sfx", t.sfx_edge),
                ("captions", t.captions),
            ];
            for (name, mark) in marks {
                assert_contrast(theme, &format!("{name} lane mark"), mark, editor.app, 3.0);
            }
            // …and from each other, so no two tracks read the same.
            for (i, (a_name, a)) in marks.iter().enumerate() {
                for (b_name, b) in &marks[i + 1..] {
                    assert_ne!(a, b, "{theme}: {a_name} and {b_name} share a color");
                }
                assert_ne!(*a, editor.accent, "{theme}: {a_name} looks like the accent");
            }
        }
    }
}
