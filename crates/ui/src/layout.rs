//! Arrangements: where a layout places the parts of [`crate::parts`].
//! They place and frame, and decide nothing: every click, state and text
//! comes in with the parts. The profile's [`LayoutId`] picks one; a new
//! layout is a variant there, a module here and an arm in each switch.

mod studio;
mod workspace;

use bardo_app::Step;
use bardo_app::bardo_domain::LayoutId;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Div, Global, KeyDownEvent, Stateful};

use crate::parts::{CollectionKeys, Navigation, ScreenParts};

/// The layout on screen.
struct Current(LayoutId);

impl Global for Current {}

/// The layout every screen is arranged in.
pub fn current(cx: &App) -> LayoutId {
    cx.try_global::<Current>()
        .map_or_else(LayoutId::default, |current| current.0)
}

/// Arranges every window in `layout` from the next frame on. Screens keep
/// their state (the place, the selection), so only where things go changes.
pub fn show(layout: LayoutId, cx: &mut App) {
    if cx
        .try_global::<Current>()
        .is_some_and(|current| current.0 == layout)
    {
        return;
    }
    cx.set_global(Current(layout));
    cx.refresh_windows();
}

/// The window: the navigation around the current screen, and the jobs
/// panel beside it when open.
pub fn shell(
    navigation: Navigation,
    screen: AnyElement,
    jobs: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    match current(cx) {
        LayoutId::Workspace => workspace::shell(navigation, screen, jobs, cx),
        LayoutId::Studio => studio::shell(navigation, screen, jobs, cx),
    }
}

/// A screen from its parts.
pub fn screen(parts: ScreenParts, cx: &App) -> AnyElement {
    match current(cx) {
        LayoutId::Workspace => workspace::screen(parts, cx),
        LayoutId::Studio => studio::screen(parts, cx),
    }
}

/// `items` taking ↑/↓ and Enter while focused; a click on them focuses
/// them.
fn take_keys(items: Stateful<Div>, keys: Option<CollectionKeys>) -> Stateful<Div> {
    let Some(keys) = keys else {
        return items;
    };
    items
        .track_focus(&keys.focus)
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            let stroke = &event.keystroke;
            if stroke.modifiers.modified() {
                return;
            }
            match stroke.key.as_str() {
                "up" => (keys.on_step)(Step::Previous, window, cx),
                "down" => (keys.on_step)(Step::Next, window, cx),
                "enter" => match &keys.on_enter {
                    Some(enter) => enter(window, cx),
                    None => return,
                },
                _ => return,
            }
            cx.stop_propagation();
        })
}
