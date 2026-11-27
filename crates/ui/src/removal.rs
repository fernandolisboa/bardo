//! The archive and delete dialog (issue #336), shared by the project
//! switcher (a project) and the Videos screen (a video): what must be
//! cleared first, *Compress media* when archiving a project, and, when
//! deleting, where the folder goes, the name to type and *Delete
//! permanently*. The rules live in `bardo_app` (`removal_blocks`,
//! `archive_project`, `archive_video`, `delete_preview`, `deletion`); the
//! deletion itself runs off the UI thread.

use bardo_app::bardo_domain::{Disposal, RemovalBlock};
use bardo_app::{Bardo, DeletePreview, Removable, RemovalError, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, SharedString, Subscription, Task, Window,
    div,
};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// What the dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalMode {
    Archive,
    Delete,
}

/// The dialog closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalEvent {
    /// It was archived (or deleted): the screens re-read what they show.
    Done(RemovalMode),
    Cancelled,
}

pub struct RemovalPanel {
    bardo: Entity<Bardo>,
    target: Removable,
    mode: RemovalMode,
    /// The name shown in the title, and typed to delete.
    name: String,
    blocks: Vec<RemovalBlock>,
    /// For a delete: the folder, its size and where it goes.
    preview: Option<DeletePreview>,
    compress: bool,
    permanent: bool,
    typed: Entity<InputState>,
    error: Option<Text>,
    /// The deletion running in the background.
    running: Option<Task<()>>,
    _subscription: Subscription,
}

impl EventEmitter<RemovalEvent> for RemovalPanel {}

impl RemovalPanel {
    /// The dialog for `target`, named `name`.
    pub fn new(
        bardo: Entity<Bardo>,
        target: Removable,
        mode: RemovalMode,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let typed = cx.new(|cx| InputState::new(window, cx).placeholder(name.clone()));
        let subscription = cx.subscribe_in(
            &typed,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.confirm(window, cx),
                _ => {}
            },
        );
        if mode == RemovalMode::Delete {
            typed.update(cx, |typed, cx| typed.focus(window, cx));
        }
        let mut panel = Self {
            bardo,
            target,
            mode,
            name,
            blocks: Vec::new(),
            preview: None,
            compress: false,
            permanent: false,
            typed,
            error: None,
            running: None,
            _subscription: subscription,
        };
        panel.refresh(cx);
        panel
    }

    /// Reads again what blocks it and, for a delete, where its folder goes.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        match self.mode {
            RemovalMode::Archive => match bardo.removal_blocks(self.target) {
                Ok(blocks) => self.blocks = blocks,
                Err(error) => self.error = Some(error.message()),
            },
            RemovalMode::Delete => match bardo.delete_preview(self.target) {
                Ok(preview) => {
                    self.blocks = preview.blocks.clone();
                    self.name = preview.name.clone();
                    self.preview = Some(preview);
                }
                Err(error) => self.error = Some(error.message()),
            },
        }
    }

    fn failed(&mut self, error: RemovalError, cx: &mut Context<Self>) {
        match error {
            RemovalError::Blocked(blocks) => {
                self.blocks = blocks;
                self.error = None;
            }
            error => self.error = Some(error.message()),
        }
        cx.notify();
    }

    fn confirm(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        match self.mode {
            RemovalMode::Archive => self.archive(cx),
            RemovalMode::Delete => self.delete(cx),
        }
    }

    fn archive(&mut self, cx: &mut Context<Self>) {
        let (target, compress) = (self.target, self.compress);
        let archived = self.bardo.update(cx, |bardo, cx| {
            let archived = match target {
                Removable::Project(id) => bardo.archive_project(id, compress).map(|_| ()),
                Removable::Video(id) => bardo.archive_video(id),
            };
            cx.notify();
            archived
        });
        match archived {
            Ok(()) => cx.emit(RemovalEvent::Done(RemovalMode::Archive)),
            Err(error) => self.failed(error, cx),
        }
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let typed = self.typed.read(cx).value().to_string();
        let (target, permanent) = (self.target, self.permanent);
        let deletion = self
            .bardo
            .update(cx, |bardo, _| bardo.deletion(target, &typed, permanent));
        let deletion = match deletion {
            Ok(deletion) => deletion,
            Err(error) => return self.failed(error, cx),
        };
        let bardo = self.bardo.clone();
        self.running = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { deletion.run() })
                .await;
            bardo.update(cx, |bardo, cx| {
                bardo.after_deletion();
                cx.notify();
            });
            let _ = this.update(cx, |this, cx| {
                this.running = None;
                match result {
                    Ok(()) => cx.emit(RemovalEvent::Done(RemovalMode::Delete)),
                    Err(error) => {
                        this.refresh(cx);
                        this.failed(error, cx);
                    }
                }
            });
        }));
        cx.notify();
    }

    fn title(&self, bardo: &Bardo) -> String {
        let text = match (self.mode, self.target) {
            (RemovalMode::Archive, Removable::Project(_)) => Text::ArchiveProjectTitle,
            (RemovalMode::Archive, Removable::Video(_)) => Text::ArchiveVideoTitle,
            (RemovalMode::Delete, Removable::Project(_)) => Text::DeleteProjectTitle,
            (RemovalMode::Delete, Removable::Video(_)) => Text::DeleteVideoTitle,
        };
        bardo.text_with(text, &[("name", &self.name)])
    }

    fn hint(&self, bardo: &Bardo) -> String {
        match (self.mode, self.target) {
            (RemovalMode::Archive, Removable::Project(_)) => {
                bardo.text(Text::ArchiveProjectHint).into_owned()
            }
            (RemovalMode::Archive, Removable::Video(_)) => {
                bardo.text(Text::ArchiveVideoHint).into_owned()
            }
            (RemovalMode::Delete, Removable::Project(_)) => {
                let (videos, connected) = self
                    .preview
                    .as_ref()
                    .map(|preview| (preview.videos, preview.connected))
                    .unwrap_or_default();
                bardo.text_with(
                    Text::DeleteProjectHint,
                    &[
                        ("videos", &videos.to_string()),
                        ("connected", &connected.to_string()),
                    ],
                )
            }
            (RemovalMode::Delete, Removable::Video(_)) => {
                bardo.text(Text::DeleteVideoHint).into_owned()
            }
        }
    }

    /// Where the folder goes, said before the user confirms.
    fn folder_line(&self, bardo: &Bardo, cx: &App) -> Option<AnyElement> {
        let preview = self.preview.as_ref()?;
        let Some(folder) = &preview.folder else {
            return Some(
                kit::notice(Tone::Info, tr(bardo, Text::DeleteNoFolder), cx).into_any_element(),
            );
        };
        let (size, folder) = (bardo.size_text(preview.size), folder.display().to_string());
        let line = match preview.disposal {
            Disposal::Recycle if !self.permanent => kit::notice(
                Tone::Info,
                bardo.text_with(
                    Text::DeleteFolderRecycle,
                    &[("size", &size), ("folder", &folder)],
                ),
                cx,
            ),
            Disposal::Recycle => {
                kit::notice(Tone::Warning, tr(bardo, Text::DeletePermanentlyHint), cx)
            }
            Disposal::Permanent(reason) => kit::notice(
                Tone::Warning,
                bardo.text_with(
                    Text::DeleteFolderPermanent,
                    &[
                        ("size", &size),
                        ("folder", &folder),
                        ("reason", &bardo.text(Text::PermanentReasonText(reason))),
                    ],
                ),
                cx,
            ),
        };
        Some(line.into_any_element())
    }

    fn render_blocks(&self, bardo: &Bardo, cx: &App) -> Option<AnyElement> {
        if self.blocks.is_empty() {
            return None;
        }
        let t = look(cx).tokens;
        Some(
            v_flex()
                .gap_1()
                .child(kit::notice(
                    Tone::Warning,
                    tr(bardo, Text::RemovalBlocked),
                    cx,
                ))
                .children(self.blocks.iter().map(|block| {
                    div()
                        .pl_5()
                        .text_xs()
                        .text_color(t.text2)
                        .child(SharedString::from(format!(
                            "• {}",
                            bardo.text(Text::RemovalBlock(*block))
                        )))
                }))
                .into_any_element(),
        )
    }
}

impl Render for RemovalPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let running = self.running.is_some();
        let delete = self.mode == RemovalMode::Delete;
        let typed = self.typed.read(cx).value().to_string();
        let name_matches = bardo_app::bardo_domain::confirms_name(&typed, &self.name);
        let ready = self.blocks.is_empty() && !running && (!delete || name_matches);
        let compress = (self.mode == RemovalMode::Archive
            && matches!(self.target, Removable::Project(_)))
        .then(|| {
            v_flex()
                .gap_1()
                .child(
                    Checkbox::new("archive-compress")
                        .label(tr(bardo, Text::ArchiveCompress))
                        .checked(self.compress)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.compress = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .pl_6()
                        .text_xs()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::ArchiveCompressHint)),
                )
        });
        // Offered only when the folder would go to the bin.
        let permanent = self
            .preview
            .as_ref()
            .filter(|preview| preview.folder.is_some() && preview.disposal == Disposal::Recycle)
            .map(|_| {
                Checkbox::new("delete-permanently")
                    .label(tr(bardo, Text::DeletePermanently))
                    .checked(self.permanent)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.permanent = *checked;
                        cx.notify();
                    }))
            });
        let name_field = delete.then(|| {
            kit::field(
                SharedString::from(bardo.text_with(Text::DeleteTypeName, &[("name", &self.name)])),
                None,
                Input::new(&self.typed).small().into_any_element(),
                None,
            )
        });
        let error = self
            .error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx));
        let confirm_label = match (self.mode, running) {
            (RemovalMode::Archive, _) => tr(bardo, Text::ArchiveConfirm),
            (RemovalMode::Delete, false) => tr(bardo, Text::DeleteConfirm),
            (RemovalMode::Delete, true) => tr(bardo, Text::DeleteRunning),
        };
        let confirm = Button::new("removal-confirm")
            .small()
            .label(confirm_label)
            .loading(running)
            .disabled(!ready)
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.confirm(window, cx)));
        let confirm = if delete {
            confirm.danger()
        } else {
            confirm.primary()
        };
        v_flex()
            .gap_3()
            .child(kit::section_heading(SharedString::from(self.title(bardo))))
            .child(
                div()
                    .text_xs()
                    .text_color(t.text2)
                    .child(SharedString::from(self.hint(bardo))),
            )
            .children(self.render_blocks(bardo, cx))
            .children(compress)
            .children(self.folder_line(bardo, cx))
            .children(permanent)
            .children(name_field)
            .children(error)
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("removal-cancel")
                            .ghost()
                            .small()
                            .label(tr(bardo, Text::ProjectFormCancel))
                            .disabled(running)
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                                cx.emit(RemovalEvent::Cancelled)
                            })),
                    )
                    .child(confirm),
            )
    }
}
