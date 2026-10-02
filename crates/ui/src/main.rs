//! Bardo desktop entry point: opens the main window over `bardo_app::Bardo`.

// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod appearance;
mod channels;
mod costs;
mod editor;
mod icons;
mod jobs;
mod kit;
mod layout;
mod metrics;
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

use anyhow::Context as _;
use bardo_app::{Bardo, Providers, Repositories};
use bardo_storage::Database;
use gpui_kit::{
    App, AppContext as _, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions, px,
    size,
};

use crate::shell::Shell;

fn main() -> anyhow::Result<()> {
    let db_path = bardo_storage::default_database_path()?;
    let db = Database::open(&db_path)
        .with_context(|| format!("opening the database at {}", db_path.display()))?;
    let secrets = bardo_storage::platform_secret_store()?;
    let files = bardo_storage::LocalProjectFiles::new(bardo_storage::default_projects_dir()?);
    let exports = bardo_storage::LocalExportFiles::new(bardo_storage::default_exports_dir());
    let locale = sys_locale::get_locale();
    let bardo = Bardo::start(
        Repositories::local(db, secrets, Box::new(files), Box::new(exports)),
        Providers::live(),
        locale.as_deref(),
    )?;
    // Started after the app so the saved keys are already masked. Without a
    // log file Bardo still runs; it only loses its diagnostics.
    if let Some(log_path) = bardo_app::logging::default_log_path() {
        let _ = bardo_app::logging::init(&log_path, bardo.redactor());
    }
    // Public post numbers catch up in the background when the profile's
    // setting says they are due.
    bardo.sync_metrics_on_start();

    // The bundled icons (gpui-kit's default set and Bardo's extra ones);
    // without them icons draw nothing.
    gpui_kit::application()
        .with_assets(icons::BardoAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            appearance::init(bardo.ui_theme(), cx);
            layout::show(bardo.ui_layout(), cx);
            let title = SharedString::from(bardo.text(bardo_app::Text::AppName).into_owned());
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1260.), px(780.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some(title),
                    ..Default::default()
                }),
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
