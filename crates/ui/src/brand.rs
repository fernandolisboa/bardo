//! Bardo's mark and wordmark (issue #131): four editor tracks crossed by a
//! gold playhead, and "Bardo" in outlined Geist SemiBold. The source files
//! and their rules live in `assets/brand/`.
//!
//! GPUI paints an SVG in one color, so the mark is drawn as stacked layers,
//! one per color, each a vector at the window's scale: crisp at any size,
//! in full color or in one tint. The layers come from the brand's mark and
//! never touch each other, so tinting them all alike gives the one-color
//! mark.

use std::borrow::Cow;
use std::sync::Arc;

use bardo_app::EditorPalette;
use bardo_app::bardo_domain::{ThemeFamily, ThemeMode, UiTheme};
use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, Pixels, SharedString, Window, div, px, rgb, svg};

/// The embedded brand files, by the asset path GPUI asks for.
const ASSETS: &[(&str, &[u8])] = &[
    (
        "brand/track-1.svg",
        include_bytes!("../assets/mark/track-1.svg"),
    ),
    (
        "brand/track-2.svg",
        include_bytes!("../assets/mark/track-2.svg"),
    ),
    (
        "brand/track-3.svg",
        include_bytes!("../assets/mark/track-3.svg"),
    ),
    (
        "brand/track-4.svg",
        include_bytes!("../assets/mark/track-4.svg"),
    ),
    (
        "brand/playhead.svg",
        include_bytes!("../assets/mark/playhead.svg"),
    ),
    (
        "brand/small-track-1.svg",
        include_bytes!("../assets/mark/small-track-1.svg"),
    ),
    (
        "brand/small-track-2.svg",
        include_bytes!("../assets/mark/small-track-2.svg"),
    ),
    (
        "brand/small-track-3.svg",
        include_bytes!("../assets/mark/small-track-3.svg"),
    ),
    (
        "brand/small-playhead.svg",
        include_bytes!("../assets/mark/small-playhead.svg"),
    ),
    (
        "brand/wordmark.svg",
        include_bytes!("../../../assets/brand/bardo-wordmark.svg"),
    ),
];

/// A brand file, for the app's asset source.
pub fn asset(path: &str) -> Option<Cow<'static, [u8]>> {
    ASSETS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
}

/// Every brand file under `path`, for the app's asset source.
pub fn list(path: &str) -> Vec<SharedString> {
    ASSETS
        .iter()
        .filter(|(name, _)| name.starts_with(path))
        .map(|(name, _)| SharedString::new_static(name))
        .collect()
}

/// The window's icon where the window asks for an image (X11). Windows
/// takes it from `bardo.exe` instead (see `build.rs`).
pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let png = include_bytes!("../../../assets/brand/png/bardo-icon-256.png");
    match image::load_from_memory_with_format(png, image::ImageFormat::Png) {
        Ok(icon) => Some(Arc::new(icon.into_rgba8())),
        Err(error) => {
            tracing::warn!("could not read the window icon: {error}");
            None
        }
    }
}

/// At this size and below the mark loses a track and thickens the rest.
const SMALL: Pixels = px(24.);

/// The ground the full-color mark sits on: each has its own shades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ground {
    Dark,
    Light,
}

/// How the mark is painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ink {
    /// The brand's colors, in the shades for the ground.
    Color(Ground),
    /// One color for every part: the theme's accent (its shade for text,
    /// which reads on the ground) where the theme brings its own colors
    /// (high contrast and terminal themes).
    Tint(Hsla),
}

impl Ink {
    /// The mark over the screens in `theme`.
    pub fn interface(theme: UiTheme, accent: Hsla) -> Self {
        match theme.family() {
            ThemeFamily::Base => Ink::Color(match theme.mode() {
                ThemeMode::Dark => Ground::Dark,
                ThemeMode::Light => Ground::Light,
            }),
            ThemeFamily::HighContrast | ThemeFamily::Terminal => Ink::Tint(accent),
        }
    }

    /// The mark over the editor in `theme`: tinted when the editor wears
    /// the theme's own colors, in full color on the standard dark editor.
    pub fn editor(theme: UiTheme, accent: Hsla) -> Self {
        if EditorPalette::for_theme(theme) == EditorPalette::for_theme(UiTheme::Graphite) {
            Ink::Color(Ground::Dark)
        } else {
            Ink::Tint(accent)
        }
    }
}

/// The brand's colors (Ouro velho): the playhead, then the tracks from the
/// top. The small mark drops the third track.
fn palette(ground: Ground) -> [u32; 5] {
    match ground {
        Ground::Dark => [0xC8962E, 0x5C4A8A, 0x3F7A68, 0xA5455F, 0xC27A55],
        Ground::Light => [0x94680F, 0x463670, 0x2A5E4F, 0x7A2840, 0xA3552D],
    }
}

/// The layers of the mark at `size`, each with its color.
fn layers(size: Pixels, ink: Ink) -> Vec<(&'static str, Hsla)> {
    let color = |index: usize| -> Hsla {
        match ink {
            Ink::Color(ground) => rgb(palette(ground)[index]).into(),
            Ink::Tint(tint) => tint,
        }
    };
    if size <= SMALL {
        vec![
            ("brand/small-track-1.svg", color(1)),
            ("brand/small-track-2.svg", color(2)),
            ("brand/small-track-3.svg", color(4)),
            ("brand/small-playhead.svg", color(0)),
        ]
    } else {
        vec![
            ("brand/track-1.svg", color(1)),
            ("brand/track-2.svg", color(2)),
            ("brand/track-3.svg", color(3)),
            ("brand/track-4.svg", color(4)),
            ("brand/playhead.svg", color(0)),
        ]
    }
}

/// The mark, `size` square.
#[derive(IntoElement)]
pub struct Mark {
    size: Pixels,
    ink: Ink,
}

impl Mark {
    pub fn new(size: Pixels, ink: Ink) -> Self {
        Self { size, ink }
    }
}

impl RenderOnce for Mark {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let size = self.size;
        div()
            .relative()
            .flex_none()
            .size(size)
            .children(layers(size, self.ink).into_iter().map(|(path, color)| {
                svg()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size(size)
                    .path(path)
                    .text_color(color)
            }))
    }
}

/// "Bardo" in the brand's letters, set beside a mark of `mark` size as the
/// lockup sets it: cap height 44 % of the mark, 18/64 of it apart.
#[derive(IntoElement)]
pub struct Lockup {
    mark: Pixels,
    ink: Ink,
    text: Hsla,
}

impl Lockup {
    pub fn new(mark: Pixels, ink: Ink, text: Hsla) -> Self {
        Self { mark, ink, text }
    }
}

/// The wordmark file's view box, 2649 × 722, is 28.47 of the mark's 64
/// units tall in the lockup.
const WORDMARK_HEIGHT: f32 = 28.47 / 64.;
const WORDMARK_ASPECT: f32 = 2649. / 722.;
const GAP: f32 = 18. / 64.;

impl RenderOnce for Lockup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mark = f32::from(self.mark);
        let height = mark * WORDMARK_HEIGHT;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(mark * GAP))
            .child(Mark::new(self.mark, self.ink))
            .child(
                svg()
                    .flex_none()
                    .w(px(height * WORDMARK_ASPECT))
                    .h(px(height))
                    .path("brand/wordmark.svg")
                    .text_color(self.text),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layer_is_embedded() {
        for size in [px(16.), px(24.), px(32.), px(64.)] {
            for (path, _) in layers(size, Ink::Color(Ground::Dark)) {
                assert!(asset(path).is_some(), "{path}");
            }
        }
        assert!(asset("brand/wordmark.svg").is_some());
        if cfg!(target_os = "linux") {
            assert!(window_icon().is_some_and(|icon| icon.width() == 256));
        }
        assert_eq!(list("brand/").len(), ASSETS.len());
    }

    #[test]
    fn small_sizes_draw_the_small_mark() {
        let paths = |size| -> Vec<&str> {
            layers(size, Ink::Color(Ground::Dark))
                .into_iter()
                .map(|(path, _)| path)
                .collect()
        };
        assert!(paths(px(24.)).iter().all(|path| path.contains("small")));
        assert!(paths(px(25.)).iter().all(|path| !path.contains("small")));
        assert_eq!(paths(px(20.)).len(), 4);
        assert_eq!(paths(px(32.)).len(), 5);
    }

    #[test]
    fn base_themes_draw_the_colors_and_the_rest_tint() {
        let accent: Hsla = rgb(0x123456).into();
        for theme in UiTheme::ALL {
            let ink = Ink::interface(theme, accent);
            match (theme.family(), theme.mode()) {
                (ThemeFamily::Base, ThemeMode::Dark) => assert_eq!(ink, Ink::Color(Ground::Dark)),
                (ThemeFamily::Base, ThemeMode::Light) => {
                    assert_eq!(ink, Ink::Color(Ground::Light));
                }
                _ => assert_eq!(ink, Ink::Tint(accent), "{theme}"),
            }
        }
    }

    #[test]
    fn the_editor_tints_only_where_it_wears_the_theme() {
        let accent: Hsla = rgb(0x123456).into();
        for theme in UiTheme::ALL {
            let expected = match (theme.family(), theme.mode()) {
                (ThemeFamily::Terminal, ThemeMode::Dark) | (ThemeFamily::HighContrast, _) => {
                    Ink::Tint(accent)
                }
                _ => Ink::Color(Ground::Dark),
            };
            assert_eq!(Ink::editor(theme, accent), expected, "{theme}");
        }
    }

    #[test]
    fn tinted_layers_share_one_color() {
        let accent: Hsla = rgb(0x123456).into();
        for size in [px(20.), px(32.)] {
            assert!(
                layers(size, Ink::Tint(accent))
                    .iter()
                    .all(|(_, color)| *color == accent)
            );
        }
    }
}
