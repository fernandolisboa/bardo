//! `bardo --showcase <folder>`, in a build with the `showcase` feature
//! only: the look job's walk through the screens (issue #166, see
//! e2e/README.md). The window shows each shot of [`Shot::plan`] in turn,
//! the way a person would get there (Settings' own calls for the layout,
//! theme and language, guide links for the place), then hands over to the
//! job's script through files in `folder`:
//!
//! 1. Bardo writes `<shot>.json` ([`bardo_app::ShotReady`]) once the shot
//!    has drawn and its jobs have settled;
//! 2. the script takes the screenshot and writes `<shot>.png`;
//! 3. Bardo shows the next shot. After the last it writes `finished.json`
//!    and quits.
//!
//! A screenshot that never comes ends the walk (`stopped.json`), so a
//! broken script never leaves Bardo open.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bardo_app::bardo_domain::UiThemePreference;
use bardo_app::{Bardo, Shot, Text};
use gpui_kit::{App, AsyncWindowContext, Entity, Window, px, size};

use crate::appearance;
use crate::layout;
use crate::shell::Shell;

/// The argument that starts the walk, followed by the folder.
const ARGUMENT: &str = "--showcase";

/// How long a shot draws before its jobs are looked at: a few frames,
/// and images read from disk.
const SETTLE: Duration = Duration::from_millis(700);
/// The longest a shot waits for its jobs (the editor's proxies).
const SETTLE_AT_MOST: Duration = Duration::from_secs(20);
/// How long the script may take over one screenshot.
const SCREENSHOT_AT_MOST: Duration = Duration::from_secs(120);
const POLL_EVERY: Duration = Duration::from_millis(100);

/// The folder the walk hands its shots over in, when Bardo was started
/// for it.
pub fn folder() -> Option<PathBuf> {
    let mut args = std::env::args().skip_while(|arg| arg != ARGUMENT);
    args.next()?;
    args.next().map(PathBuf::from)
}

/// Starts the walk in `window`, over `shell`.
pub fn start(folder: PathBuf, shell: Entity<Shell>, window: &mut Window, cx: &mut App) {
    if let Err(error) = std::fs::create_dir_all(&folder) {
        tracing::error!("the showcase folder: {error}");
        cx.quit();
        return;
    }
    // Nothing over the screens but what a shot shows.
    shell.update(cx, |shell, cx| shell.prepare_showcase(cx));
    window
        .spawn(cx, async move |cx| {
            let outcome = walk(&folder, &shell, cx).await;
            let (file, body) = match outcome {
                Ok(shots) => ("finished.json", serde_json::json!({ "shots": shots })),
                Err(error) => ("stopped.json", serde_json::json!({ "error": error })),
            };
            let _ = write(&folder.join(file), &body);
            let _ = cx.update(|_, cx| cx.quit());
        })
        .detach();
}

/// Shows every shot and waits for its screenshot; how many were taken.
async fn walk(
    folder: &Path,
    shell: &Entity<Shell>,
    cx: &mut AsyncWindowContext,
) -> Result<usize, String> {
    let plan = Shot::plan();
    for shot in &plan {
        cx.update(|window, cx| show(shot, shell, window, cx))
            .map_err(|error| error.to_string())?;
        settle(shell, cx).await;
        let title = cx
            .update(|_, cx| {
                shell
                    .read(cx)
                    .bardo()
                    .read(cx)
                    .text(Text::AppName)
                    .into_owned()
            })
            .map_err(|error| error.to_string())?;
        let name = shot.name();
        let screenshot = folder.join(format!("{name}.png"));
        let _ = std::fs::remove_file(&screenshot);
        write(&folder.join(format!("{name}.json")), &shot.ready(&title))
            .map_err(|error| format!("{name}: {error}"))?;
        let deadline = Instant::now() + SCREENSHOT_AT_MOST;
        while !screenshot.exists() {
            if Instant::now() > deadline {
                return Err(format!("no screenshot of {name}"));
            }
            cx.background_executor().timer(POLL_EVERY).await;
        }
    }
    Ok(plan.len())
}

/// Puts `shot` on screen as Settings and the guide's links would.
fn show(shot: &Shot, shell: &Entity<Shell>, window: &mut Window, cx: &mut App) {
    let bardo = shell.read(cx).bardo();
    let saved = bardo.update(cx, |bardo: &mut Bardo, cx| {
        let saved = bardo
            .set_ui_language(shot.language)
            .and_then(|()| bardo.set_ui_theme(UiThemePreference::Fixed(shot.theme)))
            .and_then(|()| bardo.set_ui_layout(shot.layout));
        cx.notify();
        saved
    });
    if let Err(error) = saved {
        tracing::warn!("the showcase could not save the shot's appearance: {error}");
    }
    appearance::show(shot.theme, cx);
    layout::show(shot.layout, cx);
    let wanted = size(px(shot.size.width as f32), px(shot.size.height as f32));
    if window.viewport_size() != wanted {
        window.resize(wanted);
    }
    shell.update(cx, |shell, cx| shell.show_place(shot.place, window, cx));
}

/// Waits for the shot to draw, then for its jobs (the editor's proxies,
/// a stage's checks) to end.
async fn settle(shell: &Entity<Shell>, cx: &mut AsyncWindowContext) {
    cx.background_executor().timer(SETTLE).await;
    let deadline = Instant::now() + SETTLE_AT_MOST;
    while Instant::now() < deadline {
        let busy = cx
            .update(|_, cx| shell.read(cx).showcase_busy(cx))
            .unwrap_or(false);
        if !busy {
            break;
        }
        cx.background_executor().timer(POLL_EVERY).await;
    }
    // A frame after the last change.
    cx.background_executor().timer(POLL_EVERY * 3).await;
}

/// Writes `value` as JSON, whole or not at all: the script never reads
/// half a file.
fn write(path: &Path, value: &impl serde::Serialize) -> std::io::Result<()> {
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&partial, path)
}
