//! The window's title bar (issue #104), drawn by Bardo in the interface
//! theme in place of the native caption: the app mark and name, a drag
//! area, and the minimize / maximize / close buttons.
//!
//! On Windows every part declares what it is ([`WindowControlArea`]) and
//! the system does the rest, as it does for its own caption: dragging,
//! double click to maximize, snapping, the snap layouts flyout on the
//! maximize button and the system menu. Elsewhere the bar drags and
//! zooms the window itself, and leaves the buttons to the window manager
//! when it draws its own decorations.

use gpui_kit::component::{Icon, IconName, InteractiveElementExt as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Decorations, Hsla, MouseButton, Pixels, SharedString, Window,
    WindowControlArea, div, px,
};

use crate::appearance::{EditorColor, editor_color, look};

/// Height of the bar.
const HEIGHT: Pixels = px(32.);
/// Width of each window button, as Windows draws its own.
const BUTTON_WIDTH: Pixels = px(46.);

/// What the bar is painted with.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub background: Hsla,
    pub border: Hsla,
    pub border_width: Pixels,
    pub text: Hsla,
    pub mark: Hsla,
    pub mark_radius: Pixels,
    pub hover: Hsla,
    pub close: Hsla,
    pub on_close: Hsla,
}

impl Colors {
    /// The interface theme's colors, over the screens.
    pub fn interface(cx: &App) -> Self {
        let t = look(cx).tokens;
        Self {
            background: t.surface,
            border: t.border,
            border_width: t.border_width,
            text: t.text,
            mark: t.accent,
            mark_radius: if t.radius == px(0.) { px(0.) } else { px(3.) },
            hover: t.hover,
            close: t.danger,
            on_close: t.surface,
        }
    }

    /// The editor's palette, over the editor.
    pub fn editor() -> Self {
        Self {
            background: editor_color(EditorColor::Panel),
            border: editor_color(EditorColor::Hairline),
            border_width: px(1.),
            text: editor_color(EditorColor::Text),
            mark: editor_color(EditorColor::Accent),
            mark_radius: px(3.),
            hover: editor_color(EditorColor::RaisedHover),
            close: editor_color(EditorColor::Error),
            on_close: editor_color(EditorColor::App),
        }
    }
}

/// The bar, with the app's name in the interface language.
#[derive(IntoElement)]
pub struct TitleBar {
    name: SharedString,
    colors: Colors,
}

impl TitleBar {
    pub fn new(name: SharedString, colors: Colors) -> Self {
        Self { name, colors }
    }
}

/// Whether a press on the drag area may still turn into a window move.
struct Pressed(bool);

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = self.colors;
        let windows = cfg!(target_os = "windows");
        let brand = h_flex()
            .gap_2()
            .pl_3()
            // Room for the traffic lights, which macOS keeps drawing.
            .when(cfg!(target_os = "macos"), |brand| brand.pl(px(80.)))
            .items_center()
            .child(
                div()
                    .size(px(12.))
                    .rounded(colors.mark_radius)
                    .bg(colors.mark),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(self.name),
            );
        let drag = h_flex()
            .id("title-bar-drag")
            .flex_1()
            .h_full()
            .min_w_0()
            .items_center()
            .child(brand);
        let drag = if windows {
            drag.window_control_area(WindowControlArea::Drag)
        } else {
            // Without the system's caption the bar moves the window itself,
            // once the pointer moves, so a double click still reaches it.
            let pressed = window.use_keyed_state("title-bar-pressed", cx, |_, _| Pressed(false));
            let down = pressed.clone();
            let up = pressed.clone();
            let out = pressed.clone();
            drag.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                down.update(cx, |pressed, _| pressed.0 = true);
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                up.update(cx, |pressed, _| pressed.0 = false);
            })
            .on_mouse_down_out(move |_, _, cx| {
                out.update(cx, |pressed, _| pressed.0 = false);
            })
            .on_mouse_move(move |_, window, cx| {
                if pressed.update(cx, |pressed, _| std::mem::take(&mut pressed.0)) {
                    window.start_window_move();
                }
            })
            .on_double_click(|_, window, _| window.zoom_window())
            .on_mouse_down(MouseButton::Right, |event, window, _| {
                window.show_window_menu(event.position);
            })
        };
        h_flex()
            .id("title-bar")
            .flex_none()
            .w_full()
            .h(HEIGHT)
            .bg(colors.background)
            .border_b(colors.border_width)
            .border_color(colors.border)
            .text_color(colors.text)
            .child(drag)
            .when(draws_buttons(window), |bar| {
                bar.child(buttons(colors, window))
            })
    }
}

/// Whether the window's buttons are Bardo's to draw: always on Windows,
/// on Linux only when the window manager leaves decorations to the app.
fn draws_buttons(window: &Window) -> bool {
    if cfg!(target_os = "macos") {
        return false;
    }
    !cfg!(target_os = "linux") || matches!(window.window_decorations(), Decorations::Client { .. })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl Button {
    fn id(self) -> &'static str {
        match self {
            Button::Minimize => "window-minimize",
            Button::Maximize => "window-maximize",
            Button::Restore => "window-restore",
            Button::Close => "window-close",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Button::Minimize => IconName::WindowMinimize,
            Button::Maximize => IconName::WindowMaximize,
            Button::Restore => IconName::WindowRestore,
            Button::Close => IconName::WindowClose,
        }
    }

    fn area(self) -> WindowControlArea {
        match self {
            Button::Minimize => WindowControlArea::Min,
            Button::Maximize | Button::Restore => WindowControlArea::Max,
            Button::Close => WindowControlArea::Close,
        }
    }
}

fn buttons(colors: Colors, window: &Window) -> AnyElement {
    let supported = window.window_controls();
    let size = if window.is_maximized() {
        Button::Restore
    } else {
        Button::Maximize
    };
    h_flex()
        .h_full()
        .flex_none()
        .when(supported.minimize, |row| {
            row.child(button(Button::Minimize, colors))
        })
        .when(supported.maximize, |row| row.child(button(size, colors)))
        .child(button(Button::Close, colors))
        .into_any_element()
}

fn button(which: Button, colors: Colors) -> AnyElement {
    let (hover, ink) = if which == Button::Close {
        (colors.close, colors.on_close)
    } else {
        (colors.hover, colors.text)
    };
    div()
        .id(which.id())
        .flex()
        .flex_none()
        .w(BUTTON_WIDTH)
        .h_full()
        .items_center()
        .justify_center()
        .hover(move |style| style.bg(hover).text_color(ink))
        .child(Icon::new(which.icon()).size(px(14.)))
        // Windows presses the button itself, from the area it reports.
        .when(cfg!(target_os = "windows"), |button| {
            button.window_control_area(which.area())
        })
        .when(!cfg!(target_os = "windows"), |button| {
            button
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(move |_, window, _| match which {
                    Button::Minimize => window.minimize_window(),
                    Button::Maximize | Button::Restore => window.zoom_window(),
                    Button::Close => window.remove_window(),
                })
        })
        .into_any_element()
}
