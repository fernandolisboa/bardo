//! Bardo desktop entry point: opens the main window over `bardo_app::Bardo`.

// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod channels;
mod jobs;
mod personas;
mod projects;
mod research;
mod settings;
mod shell;
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
    let locale = sys_locale::get_locale();
    let bardo = Bardo::start(
        Repositories::local(db, secrets, Box::new(files)),
        Providers::live(),
        locale.as_deref(),
    )?;
    // Started after the app so the saved keys are already masked. Without a
    // log file Bardo still runs; it only loses its diagnostics.
    if let Some(log_path) = bardo_app::logging::default_log_path() {
        let _ = bardo_app::logging::init(&log_path, bardo.redactor());
    }

    gpui_kit::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        let title = SharedString::from(bardo.text(bardo_app::Text::AppName).into_owned());
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1120.), px(760.)),
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
