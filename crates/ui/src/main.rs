//! Bardo desktop entry point: opens the main window over `bardo_app::Bardo`.

// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod shell;

use anyhow::Context as _;
use bardo_app::Bardo;
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
    let locale = sys_locale::get_locale();
    let bardo = Bardo::start(Box::new(db), locale.as_deref())?;

    gpui_kit::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        let title = SharedString::from(bardo.text(bardo_app::Text::AppName).to_owned());
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(960.), px(640.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some(title),
                ..Default::default()
            }),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Shell::new(bardo)))
            .expect("failed to open the main window");
        cx.activate(true);
    });
    Ok(())
}
