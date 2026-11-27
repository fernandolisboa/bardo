//! Sharing library items between projects (#340), as the World,
//! Narrators, Products and Templates screens show it alike. The
//! [`SharingPanel`] sits under an edited item: *Shareable*, what a linked
//! copy follows (with *Unlink*), the note once its original is lost,
//! *Export to another project*, *Make global* and *Save to file…*. The
//! [`ImportPanel`] sits above the list: *Import from another project* (as
//! a linked or an independent copy) and *Open item file…*. The rules
//! live in `bardo_app` (`item_sharing`, `import_item`, `export_item`,
//! `unlink_item`, `make_item_global`, `save_item_file`, `open_item_file`).

use std::path::PathBuf;
use std::time::Duration;

use bardo_app::bardo_domain::{
    CopyKind, LibraryItem, LibraryKind, ProjectId, ProjectState, ShareableItem,
};
use bardo_app::{Bardo, ItemSharing, PictureFollowing, SharingError, Text, persona_package_folder};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{
    Disableable as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, Entity, EventEmitter, PathPromptOptions, SharedString,
    Task, Window, div,
};

use crate::appearance::look;
use crate::icons::Lucide;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// How often a linked world item whose pictures are on their way is
/// looked at again.
const FOLLOW_EVERY: Duration = Duration::from_secs(2);

/// What changed, for the screen to re-read its list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharingEvent {
    /// The shown item's sharing changed (marked, unlinked, made global):
    /// the list re-reads, keeping the selection.
    Changed,
    /// A new item arrived (imported or opened from a file): the list
    /// re-reads and shows it.
    Show(LibraryItem),
}

/// A form open under the item.
enum Form {
    Export {
        to: Option<ProjectId>,
        name: Entity<InputState>,
        error: Option<Text>,
    },
    Global {
        name: Entity<InputState>,
        /// The projects that use the global template, once asked.
        users: Vec<String>,
        error: Option<Text>,
    },
}

/// The sharing of the edited item.
pub struct SharingPanel {
    bardo: Entity<Bardo>,
    id: &'static str,
    item: Option<(LibraryItem, String)>,
    state: Option<ItemSharing>,
    form: Option<Form>,
    notice: Option<(Tone, SharedString)>,
    dialog: Option<Task<()>>,
    follow: Option<Task<()>>,
}

impl EventEmitter<SharingEvent> for SharingPanel {}

impl SharingPanel {
    pub fn new(bardo: Entity<Bardo>, id: &'static str) -> Self {
        Self {
            bardo,
            id,
            item: None,
            state: None,
            form: None,
            notice: None,
            dialog: None,
            follow: None,
        }
    }

    /// Shows `item`, named `name` (`None`: nothing is edited).
    pub fn show(&mut self, item: Option<(LibraryItem, String)>, cx: &mut Context<Self>) {
        if self.item.as_ref().map(|(item, _)| *item) != item.as_ref().map(|(item, _)| *item) {
            self.form = None;
            self.notice = None;
        }
        self.item = item;
        self.refresh(cx);
    }

    /// Whether the shown item is a linked copy: its screen keeps it
    /// read-only.
    pub fn is_linked(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.sharing.is_linked())
    }

    /// Reads the shown item's sharing again.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.state = self
            .item
            .as_ref()
            .and_then(|(item, _)| self.bardo.read(cx).item_sharing(*item).ok());
        let waiting = self
            .state
            .as_ref()
            .is_some_and(|state| state.pictures != PictureFollowing::Current);
        if waiting && self.follow.is_none() {
            self.follow = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(FOLLOW_EVERY).await;
                let _ = this.update(cx, |this, cx| {
                    this.follow = None;
                    this.refresh(cx);
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    fn item(&self) -> Option<LibraryItem> {
        self.item.as_ref().map(|(item, _)| *item)
    }

    fn failed(&mut self, error: &SharingError, cx: &mut Context<Self>) {
        let text = error.message();
        match &mut self.form {
            Some(Form::Export { error, .. } | Form::Global { error, .. }) => *error = Some(text),
            None => self.notice = Some((Tone::Danger, tr(self.bardo.read(cx), text))),
        }
    }

    fn set_shareable(&mut self, shareable: bool, cx: &mut Context<Self>) {
        let Some(item) = self.item() else { return };
        self.notice = None;
        if let Err(error) = self.bardo.read(cx).set_item_shareable(item, shareable) {
            self.failed(&error, cx);
        }
        self.refresh(cx);
        cx.emit(SharingEvent::Changed);
    }

    fn unlink(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.item() else { return };
        self.notice = None;
        if let Err(error) = self.bardo.read(cx).unlink_item(item) {
            self.failed(&error, cx);
        }
        self.refresh(cx);
        cx.emit(SharingEvent::Changed);
    }

    fn dismiss_lost(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.item() else { return };
        if let Err(error) = self.bardo.read(cx).dismiss_unlinked(item) {
            self.failed(&error, cx);
        }
        self.refresh(cx);
        cx.emit(SharingEvent::Changed);
    }

    fn named(&self) -> bool {
        !matches!(self.item(), Some(LibraryItem::Template { .. }))
    }

    fn start_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self
            .item
            .as_ref()
            .map(|(_, name)| name.clone())
            .unwrap_or_default();
        let name = cx.new(|cx| InputState::new(window, cx).default_value(value));
        self.notice = None;
        self.form = Some(Form::Export {
            to: None,
            name,
            error: None,
        });
        cx.notify();
    }

    fn start_global(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self
            .item
            .as_ref()
            .map(|(_, name)| name.clone())
            .unwrap_or_default();
        let name = cx.new(|cx| InputState::new(window, cx).default_value(value));
        self.notice = None;
        self.form = Some(Form::Global {
            name,
            users: Vec::new(),
            error: None,
        });
        cx.notify();
    }

    fn typed(&self, name: &Entity<InputState>, cx: &App) -> Option<String> {
        self.named()
            .then(|| name.read(cx).value().trim().to_owned())
    }

    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(item), Some(Form::Export { to, name, .. })) = (self.item(), &self.form) else {
            return;
        };
        let Some(to) = *to else { return };
        let typed = self.typed(name, cx);
        let bardo = self.bardo.read(cx);
        match bardo.export_item(item, to, typed.as_deref()) {
            Ok(_) => {
                let project = project_name(bardo, to);
                let text = bardo.text_with(Text::SharingExportDone, &[("project", &project)]);
                self.form = None;
                self.notice = Some((Tone::Success, text.into()));
            }
            Err(SharingError::NameClash { suggestion }) => {
                if let Some(Form::Export { name, error, .. }) = &mut self.form {
                    name.update(cx, |name, cx| name.set_value(suggestion, window, cx));
                    *error = Some(Text::SharingNameClash);
                }
            }
            Err(error) => self.failed(&error, cx),
        }
        cx.notify();
    }

    fn make_global(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(item), Some(Form::Global { name, users, .. })) = (self.item(), &self.form) else {
            return;
        };
        let confirmed = !users.is_empty();
        let typed = self.typed(name, cx);
        match self
            .bardo
            .read(cx)
            .make_item_global(item, typed.as_deref(), confirmed)
        {
            Ok(()) => {
                self.form = None;
                self.notice = Some((
                    Tone::Success,
                    tr(self.bardo.read(cx), Text::SharingMadeGlobal),
                ));
                self.refresh(cx);
                cx.emit(SharingEvent::Changed);
            }
            Err(SharingError::UsedByProjects(projects)) => {
                if let Some(Form::Global { users, error, .. }) = &mut self.form {
                    *users = projects;
                    *error = None;
                }
            }
            Err(SharingError::NameClash { suggestion }) => {
                if let Some(Form::Global { name, error, .. }) = &mut self.form {
                    name.update(cx, |name, cx| name.set_value(suggestion, window, cx));
                    *error = Some(Text::SharingNameClash);
                }
            }
            Err(error) => self.failed(&error, cx),
        }
        cx.notify();
    }

    /// Asks where to save the item's file, then writes it.
    fn save_file(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.item() else { return };
        let name = match self.bardo.read(cx).item_file_name(item) {
            Ok(name) => name,
            Err(error) => {
                self.failed(&error, cx);
                cx.notify();
                return;
            }
        };
        self.notice = None;
        let chosen = cx.prompt_for_new_path(&persona_package_folder(), Some(&name));
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let chosen = chosen.await;
            let _ = this.update(cx, |this, cx| {
                this.dialog = None;
                match chosen {
                    Ok(Ok(Some(path))) => match this.bardo.read(cx).save_item_file(item, &path) {
                        Ok(()) => {
                            let text = this.bardo.read(cx).text_with(
                                Text::SharingSaved,
                                &[("path", &path.display().to_string())],
                            );
                            this.notice = Some((Tone::Success, text.into()));
                        }
                        Err(error) => this.failed(&error, cx),
                    },
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => {
                        this.notice = Some((
                            Tone::Danger,
                            tr(this.bardo.read(cx), Text::FileDialogFailed),
                        ));
                    }
                }
                cx.notify();
            });
        }));
    }

    fn render_following(&self, state: &ItemSharing, cx: &mut Context<Self>) -> Option<AnyElement> {
        let following = state.sharing.following.as_ref()?;
        let bardo = self.bardo.read(cx);
        let template = following.name.is_empty();
        let muted = look(cx).tokens.text2;
        if state.sharing.lost().is_some() {
            let text = if template {
                bardo.text_with(
                    Text::SharingLostTemplate,
                    &[("project", &following.project)],
                )
            } else {
                bardo.text_with(
                    Text::SharingLost,
                    &[("name", &following.name), ("project", &following.project)],
                )
            };
            let id = self.id;
            return Some(
                v_flex()
                    .gap_2()
                    .child(kit::notice(Tone::Warning, text, cx))
                    .child(
                        h_flex().child(
                            Button::new(ElementId::Name(format!("{id}-lost-ok").into()))
                                .small()
                                .outline()
                                .label(tr(bardo, Text::SharingLostOk))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.dismiss_lost(cx)
                                    }),
                                ),
                        ),
                    )
                    .into_any_element(),
            );
        }
        let follows = if template {
            bardo.text_with(
                Text::SharingFollowsTemplate,
                &[("project", &following.project)],
            )
        } else {
            bardo.text_with(
                Text::SharingFollows,
                &[("name", &following.name), ("project", &following.project)],
            )
        };
        let pictures = match state.pictures {
            PictureFollowing::Current => None,
            PictureFollowing::Updating => Some(kit::status(
                Tone::Info,
                tr(bardo, Text::SharingUpdating),
                cx,
            )),
            PictureFollowing::WaitingForFolder => Some(kit::status(
                Tone::Warning,
                tr(bardo, Text::SharingWaitingForFolder),
                cx,
            )),
        };
        let id = self.id;
        Some(
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(kit::status_with(Tone::Info, Lucide::Link, follows, cx))
                        .children(pictures),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(tr(bardo, Text::SharingLinkedHint)),
                )
                .child(
                    h_flex().child(
                        Button::new(ElementId::Name(format!("{id}-unlink").into()))
                            .small()
                            .outline()
                            .label(tr(bardo, Text::SharingUnlink))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.unlink(cx))),
                    ),
                )
                .into_any_element(),
        )
    }

    fn render_export(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(Form::Export { to, name, error }) = &self.form else {
            return None;
        };
        let bardo = self.bardo.read(cx);
        let tokens = &look(cx).tokens;
        let (muted, danger) = (tokens.text2, tokens.danger);
        let own = self
            .item()
            .and_then(owner_of)
            .or_else(|| crate::library::open_project(bardo).map(|(id, _)| id));
        let projects: Vec<(ProjectId, String)> = bardo
            .projects()
            .unwrap_or_default()
            .into_iter()
            .filter(|project| Some(project.id) != own)
            .map(|project| {
                let name = project.details.name().to_owned();
                let name = if project.state == ProjectState::Archived {
                    bardo.text_with(Text::SharingArchivedProject, &[("project", &name)])
                } else {
                    name
                };
                (project.id, name)
            })
            .collect();
        let id = self.id;
        let chosen = *to;
        let picks = projects.iter().enumerate().map(|(ix, (project, label))| {
            let project = *project;
            let button = Button::new(ElementId::Name(format!("{id}-export-{ix}").into()))
                .small()
                .label(SharedString::from(label.clone()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if let Some(Form::Export { to, error, .. }) = &mut this.form {
                        *to = Some(project);
                        *error = None;
                    }
                    cx.notify();
                }));
            if chosen == Some(project) {
                button.primary()
            } else {
                button.outline()
            }
        });
        let confirm_label = chosen
            .and_then(|chosen| projects.iter().find(|(project, _)| *project == chosen))
            .map(|(_, label)| bardo.text_with(Text::SharingExportTo, &[("project", label)]))
            .map(SharedString::from)
            .unwrap_or_else(|| tr(bardo, Text::SharingExport));
        Some(
            kit::well(cx)
                .p_3()
                .gap_2()
                .child(
                    div()
                        .font_semibold()
                        .child(tr(bardo, Text::SharingExportTitle)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(tr(bardo, Text::SharingExportHint)),
                )
                .child(if projects.is_empty() {
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(tr(bardo, Text::SharingNoOtherProject))
                        .into_any_element()
                } else {
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .children(picks)
                        .into_any_element()
                })
                .when(self.named(), |well| {
                    well.child(kit::field(
                        tr(bardo, Text::SharingNameLabel),
                        None,
                        Input::new(name).into_any_element(),
                        None,
                    ))
                })
                .children(
                    error.map(|error| div().text_xs().text_color(danger).child(tr(bardo, error))),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(ElementId::Name(format!("{id}-export-confirm").into()))
                                .primary()
                                .small()
                                .label(confirm_label)
                                .disabled(chosen.is_none())
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.export(window, cx)
                                })),
                        )
                        .child(cancel_button(id, tr(bardo, Text::LibraryCancel), cx)),
                )
                .into_any_element(),
        )
    }

    fn render_global(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(Form::Global { name, users, error }) = &self.form else {
            return None;
        };
        let bardo = self.bardo.read(cx);
        let tokens = &look(cx).tokens;
        let (muted, danger) = (tokens.text2, tokens.danger);
        let hint = if self.named() {
            Text::SharingMakeGlobalHint
        } else {
            Text::SharingMakeGlobalTemplateHint
        };
        let id = self.id;
        Some(
            kit::well(cx)
                .p_3()
                .gap_2()
                .child(
                    div()
                        .font_semibold()
                        .child(tr(bardo, Text::SharingMakeGlobalTitle)),
                )
                .child(div().text_sm().text_color(muted).child(tr(bardo, hint)))
                .when(self.named(), |well| {
                    well.child(kit::field(
                        tr(bardo, Text::SharingGlobalName),
                        None,
                        Input::new(name).into_any_element(),
                        None,
                    ))
                })
                .when(!users.is_empty(), |well| {
                    well.child(kit::notice(
                        Tone::Warning,
                        tr(bardo, Text::TemplateGlobalUsedBy),
                        cx,
                    ))
                    .children(users.iter().map(|user| {
                        div()
                            .text_sm()
                            .child(SharedString::from(format!("• {user}")))
                    }))
                })
                .children(
                    error.map(|error| div().text_xs().text_color(danger).child(tr(bardo, error))),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child({
                            let button =
                                Button::new(ElementId::Name(format!("{id}-global-confirm").into()))
                                    .small()
                                    .label(tr(bardo, Text::SharingMakeGlobal))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.make_global(window, cx)
                                    }));
                            if users.is_empty() {
                                button.primary()
                            } else {
                                button.warning()
                            }
                        })
                        .child(cancel_button(id, tr(bardo, Text::LibraryCancel), cx)),
                )
                .into_any_element(),
        )
    }
}

fn cancel_button(id: &'static str, label: SharedString, cx: &mut Context<SharingPanel>) -> Button {
    Button::new(ElementId::Name(format!("{id}-form-cancel").into()))
        .ghost()
        .small()
        .label(label)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.form = None;
            cx.notify();
        }))
}

/// The project that owns `item`, as far as its value tells.
fn owner_of(item: LibraryItem) -> Option<ProjectId> {
    match item {
        LibraryItem::Template { project, .. } => project,
        _ => None,
    }
}

fn project_name(bardo: &Bardo, id: ProjectId) -> String {
    bardo
        .projects()
        .unwrap_or_default()
        .into_iter()
        .find(|project| project.id == id)
        .map(|project| project.details.name().to_owned())
        .unwrap_or_default()
}

impl Render for SharingPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.state.clone() else {
            return div().into_any_element();
        };
        let following = self.render_following(&state, cx);
        let export = self.render_export(cx);
        let global = self.render_global(cx);
        let bardo = self.bardo.read(cx);
        let muted = look(cx).tokens.text2;
        let id = self.id;
        let linked = state.sharing.is_linked();
        let own = state.own && !linked;
        let idle = self.form.is_none();
        let notice = self
            .notice
            .clone()
            .map(|(tone, text)| kit::notice(tone, text, cx));
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(kit::section_heading(tr(bardo, Text::SharingTitle)))
            .children(following)
            .when(own, |card| {
                card.child(
                    v_flex()
                        .gap_1()
                        .child(
                            Checkbox::new(ElementId::Name(format!("{id}-shareable").into()))
                                .checked(state.sharing.shareable)
                                .label(tr(bardo, Text::SharingShareable))
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.set_shareable(*checked, cx)
                                })),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(tr(bardo, Text::SharingShareableHint)),
                        ),
                )
            })
            .children(export)
            .children(global)
            .when(idle, |card| {
                card.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .when(own, |row| {
                            row.child(
                                Button::new(ElementId::Name(format!("{id}-export").into()))
                                    .small()
                                    .outline()
                                    .label(tr(bardo, Text::SharingExport))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.start_export(window, cx)
                                    })),
                            )
                            .child(
                                Button::new(ElementId::Name(format!("{id}-global").into()))
                                    .small()
                                    .outline()
                                    .icon(IconName::Globe)
                                    .label(tr(bardo, Text::SharingMakeGlobal))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.start_global(window, cx)
                                    })),
                            )
                        })
                        .child(
                            Button::new(ElementId::Name(format!("{id}-save-file").into()))
                                .small()
                                .outline()
                                .label(tr(bardo, Text::SharingSaveFile))
                                .disabled(self.dialog.is_some())
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.save_file(cx)),
                                ),
                        ),
                )
            })
            .children(notice)
            .into_any_element()
    }
}

/// A row the import list offers, with a name the user may change after
/// a clash.
struct Clash {
    row: usize,
    copy: CopyKind,
    name: Entity<InputState>,
}

/// *Import from another project* and *Open item file…* for a screen.
pub struct ImportPanel {
    bardo: Entity<Bardo>,
    id: &'static str,
    kind: LibraryKind,
    /// Where an item file opens (`None`: the global library), and whether
    /// a project is the screen's owner (import is offered only then).
    into: Option<ProjectId>,
    project_scope: bool,
    open: bool,
    rows: Vec<ShareableItem>,
    clash: Option<Clash>,
    error: Option<Text>,
    notice: Option<SharedString>,
    /// A file holding product pictures, waiting for the user to confirm
    /// they may use them.
    pending: Option<PathBuf>,
    confirmed: bool,
    dialog: Option<Task<()>>,
}

impl EventEmitter<SharingEvent> for ImportPanel {}

impl ImportPanel {
    pub fn new(bardo: Entity<Bardo>, id: &'static str, kind: LibraryKind) -> Self {
        Self {
            bardo,
            id,
            kind,
            into: None,
            project_scope: false,
            open: false,
            rows: Vec::new(),
            clash: None,
            error: None,
            notice: None,
            pending: None,
            confirmed: false,
            dialog: None,
        }
    }

    /// Where items go: the screen's owner, and whether it is a project's
    /// (`into` is `None` for the global library, or a project scope with
    /// no project open, then nothing is offered).
    pub fn set_owner(
        &mut self,
        into: Option<ProjectId>,
        project_scope: bool,
        cx: &mut Context<Self>,
    ) {
        if (self.into, self.project_scope) != (into, project_scope) {
            self.open = false;
            self.clash = None;
            self.error = None;
            self.notice = None;
            self.pending = None;
        }
        self.into = into;
        self.project_scope = project_scope;
        cx.notify();
    }

    /// The kind of item the screen shows now.
    pub fn set_kind(&mut self, kind: LibraryKind, cx: &mut Context<Self>) {
        if self.kind != kind {
            self.kind = kind;
            self.open = false;
            self.clash = None;
            cx.notify();
        }
    }

    fn usable(&self) -> bool {
        !self.project_scope || self.into.is_some()
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        self.clash = None;
        self.error = None;
        self.notice = None;
        if self.open {
            match self.bardo.read(cx).importable_items(self.kind) {
                Ok(rows) => self.rows = rows,
                Err(error) => {
                    self.rows.clear();
                    self.error = Some(error.message());
                }
            }
        }
        cx.notify();
    }

    fn import(&mut self, row: usize, copy: CopyKind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(shared) = self.rows.get(row) else {
            return;
        };
        let item = shared.item;
        let name = self
            .clash
            .as_ref()
            .filter(|clash| clash.row == row)
            .map(|clash| clash.name.read(cx).value().trim().to_owned());
        let bardo = self.bardo.read(cx);
        match bardo.import_item(item, copy, name.as_deref()) {
            Ok(copied) => {
                let label = if shared.name.is_empty() {
                    shared.project_name.clone()
                } else {
                    name.unwrap_or_else(|| shared.name.clone())
                };
                self.notice = Some(
                    bardo
                        .text_with(Text::SharingImported, &[("name", &label)])
                        .into(),
                );
                self.open = false;
                self.clash = None;
                self.error = None;
                cx.emit(SharingEvent::Show(copied));
            }
            Err(SharingError::NameClash { suggestion }) => {
                let input = cx.new(|cx| InputState::new(window, cx).default_value(suggestion));
                self.clash = Some(Clash {
                    row,
                    copy,
                    name: input,
                });
                self.error = Some(Text::SharingNameClash);
            }
            Err(error) => self.error = Some(error.message()),
        }
        cx.notify();
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), Text::SharingOpenFile)),
        });
        self.notice = None;
        self.error = None;
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let chosen = chosen.await;
            let _ = this.update(cx, |this, cx| {
                this.dialog = None;
                match chosen {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.confirmed = false;
                            this.open_file(path, cx);
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => this.error = Some(Text::FileDialogFailed),
                }
                cx.notify();
            });
        }));
    }

    fn open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        match bardo.open_item_file(&path, self.into, self.confirmed) {
            Ok(item) => {
                self.pending = None;
                self.notice = Some(tr(bardo, Text::SharingOpened));
                cx.emit(SharingEvent::Show(item));
            }
            Err(SharingError::NotConfirmed) => {
                self.pending = Some(path);
                self.error = None;
            }
            Err(error) => {
                self.pending = None;
                self.error = Some(error.message());
            }
        }
        cx.notify();
    }

    fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let muted = look(cx).tokens.text2;
        let id = self.id;
        let rows = self.rows.iter().enumerate().map(|(ix, row)| {
            let project = if row.archived_project {
                bardo.text_with(
                    Text::SharingArchivedProject,
                    &[("project", &row.project_name)],
                )
            } else {
                row.project_name.clone()
            };
            let title = if row.name.is_empty() {
                project.clone()
            } else {
                row.name.clone()
            };
            let clash = self.clash.as_ref().filter(|clash| clash.row == ix);
            let button = |copy: CopyKind, label: Text| {
                let primary = clash.is_some_and(|clash| clash.copy == copy);
                let button = Button::new(ElementId::Name(
                    format!("{id}-import-{ix}-{}", copy_code(copy)).into(),
                ))
                .small()
                .label(tr(bardo, label))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.import(ix, copy, window, cx)
                }));
                if primary {
                    button.primary()
                } else {
                    button.outline()
                }
            };
            v_flex()
                .gap_1()
                .py_1()
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .child(div().font_semibold().child(SharedString::from(title)))
                                .when(!row.name.is_empty(), |column| {
                                    column.child(
                                        div()
                                            .text_sm()
                                            .text_color(muted)
                                            .child(SharedString::from(project)),
                                    )
                                }),
                        )
                        .child(button(CopyKind::Linked, Text::SharingImportLinked))
                        .child(button(
                            CopyKind::Independent,
                            Text::SharingImportIndependent,
                        )),
                )
                .children(clash.map(|clash| {
                    kit::field(
                        tr(bardo, Text::SharingNameLabel),
                        None,
                        Input::new(&clash.name).into_any_element(),
                        None,
                    )
                }))
        });
        let empty = self.rows.is_empty().then(|| {
            div()
                .text_sm()
                .text_color(muted)
                .child(tr(bardo, Text::SharingImportEmpty))
        });
        kit::card(cx)
            .p_4()
            .gap_2()
            .child(div().font_semibold().child(tr(bardo, Text::SharingImport)))
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(tr(bardo, Text::SharingImportHint)),
            )
            .children(empty)
            .children(rows)
            .into_any_element()
    }
}

fn copy_code(copy: CopyKind) -> &'static str {
    match copy {
        CopyKind::Linked => "linked",
        CopyKind::Independent => "independent",
    }
}

impl Render for ImportPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.usable() {
            return div().into_any_element();
        }
        let list = self.open.then(|| self.render_list(cx));
        let bardo = self.bardo.read(cx);
        let danger = look(cx).tokens.danger;
        let id = self.id;
        let pending = self.pending.is_some().then(|| {
            v_flex()
                .gap_2()
                .child(kit::notice(
                    Tone::Warning,
                    tr(bardo, Text::SharingConfirmPictures),
                    cx,
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Checkbox::new(ElementId::Name(format!("{id}-confirm-pictures").into()))
                                .checked(self.confirmed)
                                .label(tr(bardo, Text::SharingConfirmPicturesCheck))
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.confirmed = *checked;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(ElementId::Name(format!("{id}-open-confirmed").into()))
                                .small()
                                .primary()
                                .label(tr(bardo, Text::SharingOpenFile))
                                .disabled(!self.confirmed)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    if let Some(path) = this.pending.clone() {
                                        this.open_file(path, cx);
                                    }
                                })),
                        ),
                )
        });
        let error = self
            .error
            .map(|error| div().text_sm().text_color(danger).child(tr(bardo, error)));
        let notice = self
            .notice
            .clone()
            .map(|text| kit::notice(Tone::Success, text, cx));
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .when(self.project_scope, |row| {
                        row.child(
                            Button::new(ElementId::Name(format!("{id}-import").into()))
                                .small()
                                .outline()
                                .icon(IconName::Plus)
                                .label(tr(
                                    bardo,
                                    if self.open {
                                        Text::SharingClose
                                    } else {
                                        Text::SharingImport
                                    },
                                ))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.toggle(cx)),
                                ),
                        )
                    })
                    .child(
                        Button::new(ElementId::Name(format!("{id}-open-file").into()))
                            .small()
                            .outline()
                            .label(tr(bardo, Text::SharingOpenFile))
                            .disabled(self.dialog.is_some())
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.choose_file(cx)),
                            ),
                    ),
            )
            .children(list)
            .children(pending)
            .children(error)
            .children(notice)
            .into_any_element()
    }
}

/// The tag a list shows on a linked copy, or on a shareable item.
pub fn sharing_tag(
    bardo: &Bardo,
    sharing: &bardo_app::bardo_domain::Sharing,
    cx: &App,
) -> Option<AnyElement> {
    if sharing.is_linked() {
        Some(
            kit::status_with(
                Tone::Info,
                Lucide::Link,
                tr(bardo, Text::SharingLinkedTag),
                cx,
            )
            .into_any_element(),
        )
    } else if sharing.shareable {
        Some(
            kit::status_with(
                Tone::Accent,
                Lucide::Share2,
                tr(bardo, Text::SharingShareableTag),
                cx,
            )
            .into_any_element(),
        )
    } else {
        None
    }
}

/// How each item of `kind` that `owner` holds takes part in sharing, for
/// a list's tags; empty when it cannot be read.
pub fn sharing_map(
    bardo: &Bardo,
    owner: Option<ProjectId>,
    kind: LibraryKind,
) -> std::collections::HashMap<LibraryItem, bardo_app::bardo_domain::Sharing> {
    bardo
        .sharing_in(owner, kind)
        .unwrap_or_default()
        .into_iter()
        .collect()
}
