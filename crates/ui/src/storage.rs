//! Settings › Storage (issue #335): the base folder new projects' folders
//! go in, why the default left Documents when OneDrive syncs it, a warning
//! for a synced folder the user picked, and how existing media moves into
//! its project's folder.
//!
//! The rules live in `bardo_app` (`base_folder`, `set_base_folder`); this
//! file draws them and asks for a folder.

use bardo_app::{Bardo, Control, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Entity, PathPromptOptions, ScrollHandle, SharedString, Task, Window, div, px,
};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::shell::tr;

pub struct StoragePanel {
    bardo: Entity<Bardo>,
    /// The Settings screen's scroll, so the tour brings parts into view.
    scroll: ScrollHandle,
    /// Why the last change was not saved.
    error: Option<Text>,
    /// The folder dialog in flight.
    picking: Option<Task<()>>,
}

impl StoragePanel {
    pub fn new(bardo: Entity<Bardo>, scroll: ScrollHandle, cx: &mut Context<Self>) -> Self {
        cx.observe(&bardo, |_, _, cx| cx.notify()).detach();
        Self {
            bardo,
            scroll,
            error: None,
            picking: None,
        }
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), Text::BaseFolderPrompt)),
        });
        self.error = None;
        self.picking = Some(cx.spawn_in(window, async move |this, cx| {
            let picked = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next().map(Ok),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(_)) => Some(Err(())),
            };
            let _ = this.update(cx, |this, cx| {
                this.picking = None;
                match picked {
                    Some(Ok(folder)) => this.save(Some(folder), cx),
                    Some(Err(())) => this.error = Some(Text::FileDialogFailed),
                    None => {}
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn save(&mut self, folder: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        let saved = self.bardo.update(cx, |bardo, cx| {
            let saved = bardo.set_base_folder(folder);
            cx.notify();
            saved
        });
        self.error = saved.err().map(|_| Text::BaseFolderNotSaved);
        cx.notify();
    }
}

impl Render for StoragePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let scroll = Some(&self.scroll);
        let base = bardo.base_folder();
        let path = SharedString::from(base.path.display().to_string());
        let folder = kit::anchor_in(
            TourAnchor::Control(Control::StorageBaseFolder),
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(tr(bardo, Text::BaseFolderLabel)),
                )
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .min_w_0()
                                .px_2()
                                .py_1()
                                .rounded(t.radius)
                                .bg(t.sunken)
                                .border_1()
                                .border_color(t.border)
                                .text_sm()
                                .child(path),
                        )
                        .when(!base.chosen, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(t.text2)
                                    .child(tr(bardo, Text::BaseFolderIsDefault)),
                            )
                        }),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::BaseFolderHint)),
                )
                .when(base.avoided_sync, |part| {
                    part.child(kit::notice(
                        Tone::Info,
                        tr(bardo, Text::BaseFolderAvoidedSync),
                        cx,
                    ))
                })
                .when(base.synced, |part| {
                    part.child(kit::notice(
                        Tone::Warning,
                        tr(bardo, Text::BaseFolderSynced),
                        cx,
                    ))
                }),
            scroll,
        );
        let change = kit::anchor_in(
            TourAnchor::Control(Control::StorageChange),
            h_flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Button::new("storage-change")
                        .small()
                        .label(tr(bardo, Text::BaseFolderChange))
                        .disabled(self.picking.is_some())
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.choose(window, cx);
                        })),
                )
                .when(base.chosen, |row| {
                    row.child(
                        Button::new("storage-default")
                            .small()
                            .ghost()
                            .label(tr(bardo, Text::BaseFolderUseDefault))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.save(None, cx);
                            })),
                    )
                }),
            scroll,
        );
        let moving = kit::anchor_in(
            TourAnchor::Control(Control::StorageMove),
            div()
                .text_sm()
                .text_color(t.text2)
                .child(tr(bardo, Text::StorageMoveHint)),
            scroll,
        );
        v_flex()
            .max_w(px(720.))
            .gap_3()
            .children(
                self.error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .child(kit::section_heading(tr(bardo, Text::SettingsStorageTab)))
            .child(folder)
            .child(change)
            .child(moving)
            .into_any_element()
    }
}
