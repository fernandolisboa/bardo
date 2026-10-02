//! Arrangements: where a layout places the parts of [`crate::parts`].
//! They place and frame, and decide nothing: every click, state and text
//! comes in with the parts. Workspace is the only layout so far; a second
//! one adds its own module and a switch here.

mod workspace;

use gpui_kit::{AnyElement, App};

use crate::parts::{Navigation, ScreenParts};

/// The window: the navigation around the current screen, and the jobs
/// panel beside it when open.
pub fn shell(
    navigation: Navigation,
    screen: AnyElement,
    jobs: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    workspace::shell(navigation, screen, jobs, cx)
}

/// A screen from its parts.
pub fn screen(parts: ScreenParts, cx: &App) -> AnyElement {
    workspace::screen(parts, cx)
}
