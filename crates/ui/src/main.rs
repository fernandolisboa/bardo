//! Bardo desktop entry point: opens the main window over `bardo_app::Bardo`.

// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod appearance;
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

fn main() -> anyhow::Result<()> {
    let db_path = bardo_storage::default_database_path()?;
    let db = Database::open(&db_path)
        .with_context(|| format!("opening the database at {}", db_path.display()))?;
    let secrets = bardo_storage::platform_secret_store()?;
    let connection_secrets = bardo_storage::platform_connection_secrets()?;
    let files = bardo_storage::LocalProjectFiles::new(bardo_storage::default_projects_dir()?);
    let exports = bardo_storage::LocalExportFiles::new(bardo_storage::default_exports_dir());
    let voice_samples =
        bardo_storage::LocalVoiceSamples::new(bardo_storage::default_voice_samples_dir()?);
    let locale = sys_locale::get_locale();
    let bardo = Bardo::start(
        Repositories::local(
            db,
            secrets,
            connection_secrets,
            Box::new(files),
            Box::new(exports),
            Box::new(voice_samples),
        ),
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
    // Connections close to expiring are renewed off the UI thread.
    let renewal = bardo.connection_renewal();
    std::thread::spawn(move || renewal.run());

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
