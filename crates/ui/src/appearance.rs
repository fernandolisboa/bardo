//! Applies an interface theme (issue #59): maps Bardo's tokens onto
//! gpui-kit's theme, which every stock component reads, and keeps the
//! tokens gpui-kit lacks (status tints, frames, the accent edge, the editor's
//! palette) in the `Look` global that Bardo's own elements read through
//! [`look`].

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use bardo_app::bardo_domain::{ThemeFamily, ThemeMode, UiTheme, UiThemePreference};
use bardo_app::{EditorPalette, Palette, Rgb, UiFont};
use gpui_kit::component::{Theme, ThemeConfig, ThemeConfigColors};
use gpui_kit::{App, Global, Hsla, Pixels, SharedString, Window, WindowAppearance, px, rgb};

/// JetBrains Mono, the family terminal themes draw their interface in,
/// embedded so it renders the same on every machine (OFL, `fonts/`).
const MONO_FONTS: [&[u8]; 4] = [
    include_bytes!("../fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../fonts/JetBrainsMono-Medium.ttf"),
    include_bytes!("../fonts/JetBrainsMono-SemiBold.ttf"),
    include_bytes!("../fonts/JetBrainsMono-Bold.ttf"),
];

fn color(value: Rgb) -> Hsla {
    rgb(value.0).into()
}

/// A theme's tokens, ready to paint with.
#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub app: Hsla,
    pub surface: Hsla,
    pub raised: Hsla,
    pub sunken: Hsla,
    pub hover: Hsla,
    pub selected: Hsla,
    pub border: Hsla,
    pub frame: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text2: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub accent_text: Hsla,
    pub accent_edge: Hsla,
    pub success: Hsla,
    pub success_bg: Hsla,
    pub warning: Hsla,
    pub warning_bg: Hsla,
    pub danger: Hsla,
    pub danger_bg: Hsla,
    pub info: Hsla,
    pub info_bg: Hsla,
    /// Corner radius of controls and wells.
    pub radius: Pixels,
    /// Corner radius of cards and panels.
    pub radius_lg: Pixels,
    /// Outline width of cards, wells and chips.
    pub border_width: Pixels,
}

impl Tokens {
    fn new(p: &Palette) -> Self {
        let radius = f32::from(p.radius);
        Self {
            app: color(p.app),
            surface: color(p.surface),
            raised: color(p.raised),
            sunken: color(p.sunken),
            hover: color(p.hover),
            selected: color(p.selected),
            border: color(p.border),
            frame: color(p.frame),
            border_strong: color(p.border_strong),
            text: color(p.text),
            text2: color(p.text2),
            accent: color(p.accent),
            on_accent: color(p.on_accent),
            accent_text: color(p.accent_text),
            accent_edge: color(p.accent_edge),
            success: color(p.success),
            success_bg: color(p.success_bg),
            warning: color(p.warning),
            warning_bg: color(p.warning_bg),
            danger: color(p.danger),
            danger_bg: color(p.danger_bg),
            info: color(p.info),
            info_bg: color(p.info_bg),
            radius: px(radius),
            // Cards round a little more than controls; square stays square.
            radius_lg: px(if radius == 0. { 0. } else { radius + 2. }),
            border_width: px(f32::from(p.border_width)),
        }
    }
}

/// The editor's palette (`docs/design/editor.md`), ready to paint with.
#[derive(Debug, Clone, Copy)]
pub struct EditorTokens {
    pub app: Hsla,
    pub panel: Hsla,
    pub raised: Hsla,
    pub raised_hover: Hsla,
    pub hairline: Hsla,
    pub outline: Hsla,
    pub text: Hsla,
    pub text2: Hsla,
    pub text3: Hsla,
    pub accent: Hsla,
    pub error: Hsla,
    pub error_fill: Hsla,
    pub video_fill: Hsla,
    pub video_edge: Hsla,
    pub narration: Hsla,
    pub narration_fill: Hsla,
    pub music: Hsla,
    pub music_fill: Hsla,
    pub sfx_fill: Hsla,
    pub sfx_edge: Hsla,
    pub captions: Hsla,
    pub captions_ink: Hsla,
}

impl EditorTokens {
    fn new(p: &EditorPalette) -> Self {
        let t = &p.tracks;
        Self {
            app: color(p.app),
            panel: color(p.panel),
            raised: color(p.raised),
            raised_hover: color(p.raised_hover),
            hairline: color(p.hairline),
            outline: color(p.outline),
            text: color(p.text),
            text2: color(p.text2),
            text3: color(p.text3),
            accent: color(p.accent),
            error: color(p.error),
            error_fill: color(p.error_fill),
            video_fill: color(t.video_fill),
            video_edge: color(t.video_edge),
            narration: color(t.narration),
            narration_fill: color(t.narration_fill),
            music: color(t.music),
            music_fill: color(t.music_fill),
            sfx_fill: color(t.sfx_fill),
            sfx_edge: color(t.sfx_edge),
            captions: color(t.captions),
            captions_ink: color(t.captions_ink),
        }
    }
}

/// A role in the editor's palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorColor {
    App,
    Panel,
    Raised,
    RaisedHover,
    Hairline,
    Outline,
    Text,
    Text2,
    Text3,
    Accent,
    Error,
    ErrorFill,
    VideoFill,
    VideoEdge,
    Narration,
    NarrationFill,
    Music,
    MusicFill,
    SfxFill,
    SfxEdge,
    Captions,
    CaptionsInk,
}

impl EditorTokens {
    pub fn get(&self, ink: EditorColor) -> Hsla {
        match ink {
            EditorColor::App => self.app,
            EditorColor::Panel => self.panel,
            EditorColor::Raised => self.raised,
            EditorColor::RaisedHover => self.raised_hover,
            EditorColor::Hairline => self.hairline,
            EditorColor::Outline => self.outline,
            EditorColor::Text => self.text,
            EditorColor::Text2 => self.text2,
            EditorColor::Text3 => self.text3,
            EditorColor::Accent => self.accent,
            EditorColor::Error => self.error,
            EditorColor::ErrorFill => self.error_fill,
            EditorColor::VideoFill => self.video_fill,
            EditorColor::VideoEdge => self.video_edge,
            EditorColor::Narration => self.narration,
            EditorColor::NarrationFill => self.narration_fill,
            EditorColor::Music => self.music,
            EditorColor::MusicFill => self.music_fill,
            EditorColor::SfxFill => self.sfx_fill,
            EditorColor::SfxEdge => self.sfx_edge,
            EditorColor::Captions => self.captions,
            EditorColor::CaptionsInk => self.captions_ink,
        }
    }
}

thread_local! {
    /// The editor's palette under the current theme, mirrored from [`Look`]
    /// so the editor's small element helpers can color without an `App`.
    /// Only [`show`] writes it, on the UI thread, before windows refresh.
    static EDITOR: Cell<Option<EditorTokens>> = const { Cell::new(None) };
}

/// An editor palette role in the current theme.
pub fn editor_color(role: EditorColor) -> Hsla {
    let tokens = EDITOR
        .get()
        .unwrap_or_else(|| EditorTokens::new(&EditorPalette::for_theme(UiTheme::Graphite)));
    tokens.get(role)
}

/// The theme on screen and everything Bardo paints with besides gpui-kit.
pub struct Look {
    pub theme: UiTheme,
    pub tokens: Tokens,
    /// The platform's interface and monospace families, which base themes
    /// keep (a terminal theme replaces both while it is on).
    system_font: SharedString,
    system_mono: SharedString,
}

impl Global for Look {}

/// The current look. Every Bardo element that is not a stock component
/// reads its colors here, so a theme change repaints it too.
pub fn look(cx: &App) -> &Look {
    cx.global::<Look>()
}

/// The system's light/dark setting as the window sees it.
pub fn system_mode(window: &Window) -> ThemeMode {
    mode_of(window.appearance())
}

fn mode_of(appearance: WindowAppearance) -> ThemeMode {
    match appearance {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
        WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
    }
}

/// Embeds the terminal font and shows the theme `preference` picks for the
/// system's current appearance. Call once, after `gpui_kit::init`.
pub fn init(preference: UiThemePreference, cx: &mut App) {
    let fonts = MONO_FONTS.iter().map(|&font| Cow::Borrowed(font)).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        // Terminal themes then fall back to the platform's fonts.
        tracing::warn!("could not load the terminal theme font: {error}");
    }
    let theme = Theme::global(cx);
    let system_font = theme.font_family.clone();
    let system_mono = theme.mono_font_family.clone();
    // A first guess before any window exists; the shell confirms it with
    // its window's appearance once it opens.
    let chosen = preference.resolve(mode_of(cx.window_appearance()));
    cx.set_global(Look {
        theme: chosen,
        tokens: Tokens::new(bardo_app::palette(chosen)),
        system_font,
        system_mono,
    });
    show(chosen, cx);
}

/// Shows the theme `preference` picks while the system is in `mode`.
pub fn follow(preference: UiThemePreference, mode: ThemeMode, cx: &mut App) {
    let theme = preference.resolve(mode);
    if theme != look(cx).theme {
        show(theme, cx);
    }
}

/// Repaints every window in `theme`.
pub fn show(theme: UiTheme, cx: &mut App) {
    let look = cx.global_mut::<Look>();
    look.theme = theme;
    look.tokens = Tokens::new(bardo_app::palette(theme));
    EDITOR.set(Some(EditorTokens::new(&EditorPalette::for_theme(theme))));
    let config = Rc::new(config(
        theme,
        look.system_font.clone(),
        look.system_mono.clone(),
    ));
    // `update` re-syncs gpui-kit's token copies and refreshes every window.
    Theme::update(cx, |gpui_theme| gpui_theme.apply_config(&config));
}

fn hex(value: Rgb) -> Option<SharedString> {
    Some(format!("#{:06X}", value.0).into())
}

/// The gpui-kit theme for `theme`: Bardo's tokens on gpui-kit's keys.
fn config(theme: UiTheme, system_font: SharedString, system_mono: SharedString) -> ThemeConfig {
    let p = bardo_app::palette(theme);
    let terminal = theme.family() == ThemeFamily::Terminal;
    let (font, mono) = match &p.font {
        UiFont::Embedded(family) => (SharedString::from(family.clone()), family.clone().into()),
        UiFont::System => (system_font, system_mono),
    };
    let radius = usize::from(p.radius);
    ThemeConfig {
        is_default: false,
        name: SharedString::new_static(theme.code()),
        mode: match theme.mode() {
            ThemeMode::Light => gpui_kit::component::ThemeMode::Light,
            ThemeMode::Dark => gpui_kit::component::ThemeMode::Dark,
        },
        font_size: None,
        font_family: Some(font),
        mono_font_family: Some(mono),
        mono_font_size: None,
        radius: Some(radius),
        radius_lg: Some(if radius == 0 { 0 } else { radius + 2 }),
        // Soft shadows only lift light base cards; terminal and dark
        // themes separate surfaces by tone and frame alone.
        shadow: Some(!terminal && theme.mode() == ThemeMode::Light),
        colors: colors(p),
        highlight: None,
    }
}

fn colors(p: &Palette) -> ThemeConfigColors {
    // Some of its fields are private, so no struct literal.
    let mut c = ThemeConfigColors::default();
    c.background = hex(p.app);
    c.foreground = hex(p.text);
    c.border = hex(p.border);
    c.input = hex(p.border_strong);
    c.ring = hex(p.focus);
    c.caret = hex(p.text);
    c.selection = hex(p.selected);
    c.muted = hex(p.sunken);
    c.muted_foreground = hex(p.text2);
    c.accent = hex(p.hover);
    c.accent_foreground = hex(p.text);
    c.group_box = hex(p.surface);
    c.group_box_foreground = hex(p.text);
    c.group_box_title_foreground = hex(p.text2);
    c.popover = hex(p.raised);
    c.popover_foreground = hex(p.text);
    c.primary = hex(p.accent);
    c.primary_foreground = hex(p.on_accent);
    c.button_primary = hex(p.accent);
    c.button_primary_foreground = hex(p.on_accent);
    c.secondary = hex(p.raised);
    c.secondary_foreground = hex(p.text);
    c.secondary_hover = hex(p.hover);
    c.button = hex(p.raised);
    c.button_foreground = hex(p.text);
    c.button_hover = hex(p.hover);
    // Status colors are inks first (Bardo writes state with them), so
    // a fill takes the surface's color for its text.
    c.success = hex(p.success);
    c.success_foreground = hex(p.surface);
    c.warning = hex(p.warning);
    c.warning_foreground = hex(p.surface);
    c.danger = hex(p.danger);
    c.danger_foreground = hex(p.surface);
    c.info = hex(p.info);
    c.info_foreground = hex(p.surface);
    c.link = hex(p.accent_text);
    c.link_hover = hex(p.accent_text);
    c.link_active = hex(p.accent_text);
    c.list = hex(p.surface);
    c.list_hover = hex(p.hover);
    c.list_active = hex(p.selected);
    c.list_active_border = hex(p.accent_edge);
    c.list_even = hex(p.surface);
    c.list_head = hex(p.sunken);
    c.table = hex(p.surface);
    c.table_hover = hex(p.hover);
    c.table_active = hex(p.selected);
    c.table_active_border = hex(p.accent_edge);
    c.table_even = hex(p.surface);
    c.table_head = hex(p.sunken);
    c.table_head_foreground = hex(p.text2);
    c.table_row_border = hex(p.border);
    c.tab_bar = hex(p.app);
    c.tab = hex(p.app);
    c.tab_active = hex(p.surface);
    c.tab_foreground = hex(p.text2);
    c.tab_active_foreground = hex(p.text);
    c.sidebar = hex(p.surface);
    c.sidebar_foreground = hex(p.text);
    c.sidebar_border = hex(p.border);
    c.sidebar_accent = hex(p.selected);
    c.sidebar_accent_foreground = hex(p.text);
    c.sidebar_primary = hex(p.accent);
    c.sidebar_primary_foreground = hex(p.on_accent);
    c.title_bar = hex(p.surface);
    c.title_bar_border = hex(p.border);
    c.description_list_label = hex(p.sunken);
    c.description_list_label_foreground = hex(p.text2);
    c.progress_bar = hex(p.accent);
    c.slider_bar = hex(p.accent);
    c.slider_thumb = hex(p.raised);
    c.switch = hex(p.accent);
    c.switch_thumb = hex(p.raised);
    c.skeleton = hex(p.hover);
    c.drag_border = hex(p.accent);
    c.drop_target = hex(p.selected);
    c.scrollbar_thumb = hex(p.border_strong);
    c.scrollbar_thumb_hover = hex(p.text3);
    c.window_border = hex(p.frame);
    c
}
