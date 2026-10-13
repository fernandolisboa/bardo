//! Bardo desktop entry point: opens the main window over `bardo_app::Bardo`,
//! or, with `--agent`, runs the background publishing agent without one.
//! The uninstaller runs it with `--uninstall` (issue #135).

// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod appearance;
mod brand;
mod channels;
mod costs;
mod editor;
mod guide;
mod guide_screen;
mod icons;
mod jobs;
mod kit;
mod layout;
mod metrics;
mod missed;
mod network_accounts;
mod parts;
mod performance;
mod personas;
mod projects;
mod research;
mod settings;
mod shell;
mod spend;
mod templates;
mod themes;
mod title_bar;
mod tour;

use anyhow::Context as _;
use bardo_app::{Bardo, Providers, Repositories};
use bardo_storage::Database;
use gpui_kit::{
    App, AppContext as _, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions, px,
    size,
};

use crate::shell::Shell;

/// Bardo's local storage, as both the app and the agent open it.
fn repositories() -> anyhow::Result<Repositories> {
    let db_path = bardo_storage::default_database_path()?;
    let db = Database::open(&db_path)
        .with_context(|| format!("opening the database at {}", db_path.display()))?;
    let secrets = bardo_storage::platform_secret_store()?;
    let connection_secrets = bardo_storage::platform_connection_secrets()?;
    let files = bardo_storage::LocalProjectFiles::new(bardo_storage::default_projects_dir()?);
    let exports = bardo_storage::LocalExportFiles::new(bardo_storage::default_exports_dir());
    let voice_samples =
        bardo_storage::LocalVoiceSamples::new(bardo_storage::default_voice_samples_dir()?);
    Ok(Repositories::local(
        db,
        secrets,
        connection_secrets,
        Box::new(files),
        Box::new(exports),
        Box::new(voice_samples),
    ))
}

/// The background agent: sends scheduled posts until the user turns it
/// off. It exits at once when it is off, or another agent runs.
fn run_agent() -> anyhow::Result<()> {
    let Some(agent) = Bardo::start_agent(repositories()?, Providers::live())? else {
        return Ok(());
    };
    if let Some(log_path) = bardo_app::logging::agent_log_path() {
        let _ = bardo_app::logging::init(&log_path, agent.redactor());
    }
    tracing::info!("the background agent starts");
    agent.run_agent();
    Ok(())
}

/// The smallest the main window goes, in logical pixels.
const MIN_WIDTH: f32 = 960.;
const MIN_HEIGHT: f32 = 600.;

/// The argument the uninstaller runs `bardo` with.
const UNINSTALL_ARGUMENT: &str = "--uninstall";

/// Before the uninstaller removes the program: the background agent is
/// turned off, its task removed, and a running agent has stopped. A
/// machine where Bardo never opened gets no database from it.
fn uninstall() -> anyhow::Result<()> {
    if !bardo_storage::default_database_path()?.exists() {
        let task = bardo_storage::platform_agent_task();
        if task.is_registered()? {
            task.remove()?;
        }
        return Ok(());
    }
    Bardo::uninstall(repositories()?)?;
    Ok(())
}

/// What the installer asked on a first install, read once and deleted.
fn apply_setup_choices(bardo: &mut Bardo) {
    let Ok(path) = bardo_storage::default_setup_choices_path() else {
        return;
    };
    match bardo_storage::take_setup_choices(&path) {
        Ok(Some(choices)) => {
            if let Err(error) = bardo.apply_setup_choices(&choices) {
                tracing::warn!("the installer's choices were not saved: {error}");
            }
        }
        Ok(None) => {}
        Err(error) => tracing::warn!("{error}"),
    }
}

fn main() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == bardo_storage::AGENT_ARGUMENT) {
        return run_agent();
    }
    if std::env::args().any(|arg| arg == UNINSTALL_ARGUMENT) {
        return uninstall();
    }
    let locale = sys_locale::get_locale();
    let mut bardo = Bardo::start(repositories()?, Providers::live(), locale.as_deref())?;
    // Started after the app so the saved keys are already masked. Without a
    // log file Bardo still runs; it only loses its diagnostics.
    if let Some(log_path) = bardo_app::logging::default_log_path() {
        let _ = bardo_app::logging::init(&log_path, bardo.redactor());
    }
    // Before the agent's upkeep below, which sets the task up when the
    // installer's answer turned it on.
    apply_setup_choices(&mut bardo);
    // Public post numbers catch up in the background when the profile's
    // setting says they are due.
    bardo.sync_metrics_on_start();
    // Connections close to expiring are renewed off the UI thread.
    let renewal = bardo.connection_renewal();
    std::thread::spawn(move || renewal.run());
    // The background agent's task follows the setting: set up again when
    // it is on and not running, removed when it is off but left over.
    let upkeep = bardo.agent_upkeep();
    std::thread::spawn(move || {
        if let Err(error) = upkeep.run() {
            tracing::warn!("{error}");
        }
    });

    // The bundled icons (gpui-kit's default set and Bardo's extra ones);
    // without them icons draw nothing.
    gpui_kit::application()
        .with_assets(icons::BardoAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            appearance::init(bardo.ui_theme(), cx);
            editor::init(cx);
            guide_screen::init(cx);
            layout::show(bardo.ui_layout(), cx);
            let title = SharedString::from(bardo.text(bardo_app::Text::AppName).into_owned());
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1260.), px(780.)),
                    cx,
                ))),
                // Bardo draws the title bar in the interface theme
                // (`title_bar`); the system keeps the title for the taskbar.
                titlebar: Some(TitlebarOptions {
                    title: Some(title),
                    appears_transparent: true,
                    ..Default::default()
                }),
                app_owns_titlebar_drag: true,
                // The narrowest window every screen is checked at in both
                // layouts: half of a 1920-pixel-wide screen at 100 % scale,
                // where Windows snaps two windows side by side (issue #151).
                // Laptop snap halves are narrower; issue #153 lowers it.
                window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
                icon: brand::window_icon(),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Shell::new(bardo, window, cx))
            })
            .expect("failed to open the main window");
            cx.activate(true);
        });
    Ok(())
}
