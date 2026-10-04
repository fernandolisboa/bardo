//! Shared pieces that carry the presentation rules (issue #59): tonal
//! surfaces, state as icon + label + color, explanations and provenance
//! behind a click. Colors come from the current [`look`], so every piece
//! repaints with the theme.

use bardo_app::bardo_domain::ThemeFamily;
use bardo_app::{Side, TourAnchor};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Div, ElementId, Hsla, SharedString, Stateful, div, px};

use crate::appearance::look;
use crate::tour::Anchored as _;

/// A state's kind; picks the chip's colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Success,
    Warning,
    Danger,
    Info,
    /// Selected, chosen, in review.
    Accent,
    /// Present but nothing to act on.
    Neutral,
}

impl Tone {
    /// (ink, tint) of the tone.
    fn colors(self, cx: &App) -> (Hsla, Hsla) {
        let t = &look(cx).tokens;
        match self {
            Tone::Success => (t.success, t.success_bg),
            Tone::Warning => (t.warning, t.warning_bg),
            Tone::Danger => (t.danger, t.danger_bg),
            Tone::Info => (t.info, t.info_bg),
            Tone::Accent => (t.accent_text, t.selected),
            Tone::Neutral => (t.text2, t.sunken),
        }
    }

    /// The icon a state of this tone shows by default.
    pub fn icon(self) -> IconName {
        match self {
            Tone::Success => IconName::CircleCheck,
            Tone::Warning => IconName::TriangleAlert,
            Tone::Danger => IconName::CircleX,
            Tone::Info | Tone::Neutral => IconName::Info,
            Tone::Accent => IconName::Star,
        }
    }

    /// Ink only, for text that states something without a chip.
    pub fn ink(self, cx: &App) -> Hsla {
        self.colors(cx).0
    }
}

/// A state: icon, short label and color on its tint, never color alone.
pub fn status(tone: Tone, label: impl Into<SharedString>, cx: &App) -> Div {
    status_with(tone, tone.icon(), label, cx)
}

/// [`status`] with an icon of its own.
pub fn status_with(
    tone: Tone,
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Div {
    let (ink, tint) = tone.colors(cx);
    let look = look(cx);
    // High contrast outlines chips so they hold without their tint.
    let outlined = look.theme.family() == ThemeFamily::HighContrast;
    h_flex()
        .flex_none()
        .gap_1()
        .h(px(22.))
        .pl(px(6.))
        .pr(px(8.))
        .rounded(if look.tokens.radius == px(0.) {
            px(2.)
        } else {
            px(999.)
        })
        .bg(tint)
        .text_color(ink)
        .text_xs()
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .when(outlined, |chip| {
            chip.border(look.tokens.border_width).border_color(ink)
        })
        .child(icon.into().size(px(13.)).text_color(ink))
        .child(div().whitespace_nowrap().child(label.into()))
}

/// A message after an action (saved, failed, exported to …): icon and
/// text in the tone's ink; wraps, unlike a [`status`] chip.
pub fn notice(tone: Tone, text: impl Into<SharedString>, cx: &App) -> Div {
    let ink = tone.ink(cx);
    h_flex()
        .min_w_0()
        .gap_1p5()
        .items_start()
        .text_sm()
        .text_color(ink)
        .child(
            div()
                .flex_none()
                .pt(px(2.))
                .child(Icon::new(tone.icon()).size(px(14.)).text_color(ink)),
        )
        .child(div().min_w_0().child(text.into()))
}

/// A card: a surface a step off the app ground, framed.
pub fn card(cx: &App) -> Div {
    let t = &look(cx).tokens;
    v_flex()
        .bg(t.surface)
        .border(t.border_width)
        .border_color(t.frame)
        .rounded(t.radius_lg)
}

/// A well: read-only text or a prompt, a step below its card.
pub fn well(cx: &App) -> Div {
    let t = &look(cx).tokens;
    div()
        .bg(t.sunken)
        .border(t.border_width)
        .border_color(t.border)
        .rounded(t.radius)
        .px_2p5()
        .py_1p5()
        .text_sm()
        .text_color(t.text)
}

/// An ⓘ button whose popover holds the how-it-works or billing text, so
/// the screen keeps at most one sentence of explanation.
pub fn info(id: impl Into<ElementId>, label: Option<SharedString>, text: SharedString) -> Popover {
    let id = id.into();
    let mut trigger = Button::new(id.clone())
        .ghost()
        .xsmall()
        .icon(IconName::Info);
    if let Some(label) = label {
        trigger = trigger.label(label);
    }
    Popover::new(id).trigger(trigger).content(move |_, _, cx| {
        div()
            .max_w(px(360.))
            .text_sm()
            .text_color(look(cx).tokens.text)
            .child(text.clone())
    })
}

/// A "Details" button whose popover lists provenance: model, tokens,
/// template version.
pub fn details(id: impl Into<ElementId>, label: SharedString, lines: Vec<SharedString>) -> Popover {
    let id = id.into();
    Popover::new(id.clone())
        .trigger(
            Button::new(id)
                .ghost()
                .xsmall()
                .dropdown_caret(true)
                .label(label),
        )
        .content(move |_, _, cx| {
            let t = &look(cx).tokens;
            v_flex()
                .gap_1()
                .max_w(px(420.))
                .text_xs()
                .text_color(t.text2)
                .children(lines.iter().cloned().map(|line| div().child(line)))
        })
}

/// A screen's title line.
pub fn title(text: impl Into<SharedString>) -> Div {
    div()
        .text_xl()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .child(text.into())
}

/// A section heading, with optional things beside it (a count, an ⓘ).
pub fn section_heading(text: impl Into<SharedString>) -> Div {
    h_flex().gap_2().items_center().child(
        div()
            .text_lg()
            .font_weight(gpui_kit::FontWeight::MEDIUM)
            .child(text.into()),
    )
}

/// A row of a list of records; the selected one is tinted and edged.
pub fn list_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let t = look(cx).tokens;
    v_flex()
        .id(id)
        .px_3()
        .py_2()
        .gap_0p5()
        .rounded(t.radius)
        .cursor_pointer()
        .border(t.border_width)
        .map(|row| {
            if selected {
                row.bg(t.selected).border_color(t.accent_edge)
            } else {
                row.border_color(gpui_kit::transparent_black())
                    .hover(|row| row.bg(t.hover))
            }
        })
}

/// A form field: its label (with an ⓘ when it needs explaining), the
/// control, and an error or a one-line hint below.
pub fn field(
    label: SharedString,
    info: Option<Popover>,
    control: AnyElement,
    below: Option<AnyElement>,
) -> Div {
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(label),
                )
                .children(info),
        )
        .child(control)
        .children(below)
}

/// `element` tagged with the component a tour step points at (issue
/// #105): a screen's own control, where the layouts tag its parts.
pub fn anchor(anchor: TourAnchor, element: impl IntoElement) -> Div {
    div()
        .relative()
        .child(element)
        .tour_anchor(anchor, Side::Below, None)
}
