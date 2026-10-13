//! Arrangements: where a layout places the parts of [`crate::parts`].
//! They place and frame, and decide nothing: every click, state and text
//! comes in with the parts. The profile's [`LayoutId`] picks one; a new
//! layout is a variant there, a module here and an arm in each switch.

mod studio;
mod workspace;

use bardo_app::Step;
use bardo_app::bardo_domain::LayoutId;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Div, Global, KeyDownEvent, Stateful, div, px};

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

/// The jobs panel's width.
pub const JOBS_WIDTH: f32 = 360.;
/// The narrowest a screen gets with the jobs panel beside it: in a
/// narrower window the panel lies over the screen's right side instead of
/// squeezing it (issue #153).
const SCREEN_MIN_BESIDE_JOBS: f32 = 640.;

/// What a layout fits its columns to: the window's width, in logical
/// pixels, and whether the jobs panel is open. The shell sets it as it
/// draws, before the screens draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub window: f32,
    pub jobs_open: bool,
}

impl Global for Room {}

impl Default for Room {
    /// The window Bardo opens in.
    fn default() -> Self {
        Self {
            window: 1260.,
            jobs_open: false,
        }
    }
}

impl Room {
    /// Whether the jobs panel lies over the screen rather than beside it,
    /// with `nav` taken by the navigation beside the screen.
    pub fn jobs_over(self, nav: f32) -> bool {
        self.jobs_open && self.window - nav - JOBS_WIDTH < SCREEN_MIN_BESIDE_JOBS
    }

    /// The screen's width, with `nav` taken by the navigation beside it.
    pub fn screen(self, nav: f32) -> f32 {
        let jobs = if self.jobs_open && !self.jobs_over(nav) {
            JOBS_WIDTH
        } else {
            0.
        };
        self.window - nav - jobs
    }
}

/// Fits the screens to `room` from this frame on.
pub fn set_room(room: Room, cx: &mut App) {
    if cx.try_global::<Room>() != Some(&room) {
        cx.set_global(room);
    }
}

pub(crate) fn room(cx: &App) -> Room {
    cx.try_global::<Room>().copied().unwrap_or_default()
}

/// The screen's area, with the jobs panel over its right side when `over`;
/// otherwise the panel comes back to go beside it.
pub fn screen_area(
    screen: AnyElement,
    jobs: Option<AnyElement>,
    over: bool,
) -> (Div, Option<AnyElement>) {
    let (over, beside) = if over { (jobs, None) } else { (None, jobs) };
    let area = div()
        .relative()
        .flex_1()
        .h_full()
        .min_w_0()
        .child(screen)
        .children(over.map(|jobs| {
            div()
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(px(JOBS_WIDTH))
                .shadow_lg()
                .child(jobs)
        }));
    (area, beside)
}

/// The window: the navigation around the current screen, and the jobs
/// panel beside it when open (over it when the window is narrow).
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

#[cfg(test)]
mod tests {
    use super::*;

    fn room(window: f32, jobs_open: bool) -> Room {
        Room { window, jobs_open }
    }

    #[test]
    fn the_jobs_panel_goes_beside_the_screen_while_the_screen_keeps_its_room() {
        let open = room(1260., true);
        assert!(!open.jobs_over(216.));
        assert_eq!(open.screen(216.), 1260. - 216. - JOBS_WIDTH);
    }

    #[test]
    fn the_jobs_panel_lies_over_a_screen_that_would_get_too_narrow() {
        let narrow = room(960., true);
        assert!(narrow.jobs_over(216.));
        assert_eq!(narrow.screen(216.), 960. - 216.);
    }

    #[test]
    fn a_closed_jobs_panel_takes_no_room() {
        let closed = room(640., false);
        assert!(!closed.jobs_over(0.));
        assert_eq!(closed.screen(0.), 640.);
    }
}
