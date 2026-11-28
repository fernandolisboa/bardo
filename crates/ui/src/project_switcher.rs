//! The project switcher (issue #333): in the title bar, beside Bardo's
//! mark, the open project's color and name, and a popover that opens
//! another one. The popover searches the projects and lists the
//! favorites (in the user's order, each with a star and a way up),
//! the recent ones and all the others by name, then *New project…*,
//! whose name and color form opens the new project.
//!
//! For the open project it also offers *Archive project…* and *Delete
//! project…*, and *Archived projects* lists the archived ones to bring
//! back or delete (issue #336, the dialog in `removal`).
//!
//! *Export project…* writes the open project's package, *Import a project
//! package…* brings one in as a new project after a dialog that names it
//! and shows what it holds, and *Open a project from a folder…* relinks a
//! moved project or brings a deleted one back (issue #342).
//!
//! The list and its rules live in `bardo_app` (`project_list`,
//! `switch_project`, `create_project`); this file draws them and tells
//! the shell when the open project changed, so the screens re-read it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bardo_app::bardo_domain::{
    Project, ProjectColor, ProjectDraft, ProjectId, TemplateKind, package_file_name,
};
use bardo_app::{
    ArchivedProject, Bardo, Control, FolderOpened, PackageChoice, PackageOffer, Removable, Text,
    TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Disableable as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, PathPromptOptions, SharedString,
    Subscription, Task, Window, div, px,
};

use crate::appearance::look;
use crate::kit;
use crate::removal::{RemovalEvent, RemovalMode, RemovalPanel};
use crate::shell::tr;

/// Width of the open popover.
const WIDTH: f32 = 320.;

/// The open project changed: the screens re-read what they show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectSwitched;

/// The new project form, while it is open in the popover.
struct NewProject {
    /// The id the project will have, so its folder's name is shown as it
    /// will be made.
    id: ProjectId,
    name: Entity<InputState>,
    color: ProjectColor,
    /// Where its folder goes; the base folder when `None`.
    place: Option<PathBuf>,
    error: Option<Text>,
    _subscription: Subscription,
}

/// The import dialog, once a package was picked and read.
struct ImportForm {
    offer: PackageOffer,
    /// The id the new project will have, so its folder is shown as it
    /// will be made.
    id: ProjectId,
    name: Entity<InputState>,
    color: ProjectColor,
    place: Option<PathBuf>,
    /// The carried global templates ticked to come in as the project's
    /// own.
    templates: Vec<TemplateKind>,
    error: Option<Text>,
    _subscription: Subscription,
}

pub struct ProjectSwitcher {
    bardo: Entity<Bardo>,
    open: bool,
    search: Entity<InputState>,
    query: String,
    form: Option<NewProject>,
    /// The import dialog, while open.
    import: Option<ImportForm>,
    /// What the last export, import or open did, when it went well.
    notice: Option<String>,
    /// What went wrong with the last switch, star, move or locate.
    problem: Option<Text>,
    /// A folder dialog in flight.
    picking: Option<Task<()>>,
    /// *Archived projects* is open instead of the list.
    archived: bool,
    /// The archive or delete dialog, while open.
    removal: Option<(Entity<RemovalPanel>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ProjectSwitched> for ProjectSwitcher {}

impl ProjectSwitcher {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = tr(bardo.read(cx), Text::SwitcherSearch);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscriptions = vec![cx.subscribe_in(
            &search,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.query = input.read(cx).value().to_string();
                    cx.notify();
                }
                // Enter opens the first project that matches.
                InputEvent::PressEnter { .. } => {
                    let first = this.list(cx).and_then(|list| {
                        list.favorites
                            .into_iter()
                            .chain(list.recent)
                            .chain(list.all)
                            .next()
                    });
                    if let Some(project) = first {
                        this.switch(project.id, window, cx);
                    }
                }
                _ => {}
            },
        )];
        Self {
            bardo,
            open: false,
            search,
            query: String::new(),
            form: None,
            import: None,
            notice: None,
            problem: None,
            picking: None,
            archived: false,
            removal: None,
            _subscriptions: subscriptions,
        }
    }

    fn list(&self, cx: &App) -> Option<bardo_app::bardo_domain::ProjectList> {
        self.bardo.read(cx).project_list(&self.query).ok()
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        self.form = None;
        self.import = None;
        self.notice = None;
        self.problem = None;
        self.archived = false;
        self.removal = None;
        if open {
            // A drive plugged in or out since: the list says so.
            self.bardo.read(cx).check_folders();
            self.query.clear();
            let placeholder = tr(self.bardo.read(cx), Text::SwitcherSearch);
            self.search.update(cx, |search, cx| {
                search.set_value("", window, cx);
                search.set_placeholder(placeholder, window, cx);
                search.focus(window, cx);
            });
        }
        cx.notify();
    }

    fn switch(&mut self, id: ProjectId, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.bardo.read(cx).open_project().ok().flatten();
        if open.is_some_and(|open| open.id == id) {
            self.set_open(false, window, cx);
            return;
        }
        match self.bardo.read(cx).switch_project(id) {
            Ok(_) => {
                self.set_open(false, window, cx);
                cx.emit(ProjectSwitched);
            }
            Err(error) => {
                self.problem = Some(error.message());
                cx.notify();
            }
        }
    }

    /// Opens *No project*: the loose videos (#341).
    fn open_loose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bardo.read(cx).loose_open() {
            self.set_open(false, window, cx);
            return;
        }
        match self.bardo.read(cx).open_loose_videos() {
            Ok(()) => {
                self.set_open(false, window, cx);
                cx.emit(ProjectSwitched);
            }
            Err(_) => {
                self.problem = Some(Text::LooseNotSaved);
                cx.notify();
            }
        }
    }

    /// *No project*, after the projects: how many loose videos there are.
    fn loose_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let selected = bardo.loose_open();
        h_flex()
            .id("project-row-loose")
            .px_2()
            .py_1()
            .gap_2()
            .items_center()
            .rounded(t.radius)
            .cursor_pointer()
            .text_sm()
            .when(selected, |row| row.bg(t.selected))
            .when(!selected, |row| row.hover(|row| row.bg(t.hover)))
            .child(Icon::new(IconName::Inbox).size(px(12.)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .when(selected, |name| {
                                name.font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            })
                            .child(tr(bardo, Text::LooseSwitcherRow)),
                    )
                    .child(div().text_xs().text_color(t.text2).truncate().child(
                        SharedString::from(loose_count_text(bardo, bardo.loose_video_count())),
                    )),
            )
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.open_loose(window, cx);
            }))
            .into_any_element()
    }

    fn star(&mut self, id: ProjectId, starred: bool, cx: &mut Context<Self>) {
        self.problem = self
            .bardo
            .read(cx)
            .star_project(id, starred)
            .err()
            .map(|error| error.message());
        cx.notify();
    }

    fn move_up(&mut self, id: ProjectId, to: usize, cx: &mut Context<Self>) {
        self.problem = self
            .bardo
            .read(cx)
            .move_favorite(id, to)
            .err()
            .map(|error| error.message());
        cx.notify();
    }

    fn start_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let placeholder = tr(bardo, Text::ProjectNamePlaceholder);
        let color = bardo.next_project_color();
        // What was typed in the search box is the likeliest name.
        let typed = self.query.trim().to_owned();
        let name = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(placeholder);
            input.set_value(typed, window, cx);
            input
        });
        let subscription = cx.subscribe_in(
            &name,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.create(window, cx),
                InputEvent::Change => {
                    if let Some(form) = &mut this.form {
                        form.error = None;
                    }
                    cx.notify();
                }
                _ => {}
            },
        );
        name.update(cx, |name, cx| name.focus(window, cx));
        self.form = Some(NewProject {
            id: ProjectId::new(),
            name,
            color,
            place: None,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Asks for a folder, then hands it to `chosen`.
    fn ask_folder(
        &mut self,
        prompt: Text,
        window: &mut Window,
        cx: &mut Context<Self>,
        chosen: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.ask_path(false, prompt, window, cx, chosen);
    }

    /// Asks for a file (`file`) or a folder, then hands it to `chosen`.
    fn ask_path(
        &mut self,
        file: bool,
        prompt: Text,
        window: &mut Window,
        cx: &mut Context<Self>,
        chosen: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let asked = cx.prompt_for_paths(PathPromptOptions {
            files: file,
            directories: !file,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), prompt)),
        });
        self.picking = Some(cx.spawn_in(window, async move |this, cx| {
            let picked = match asked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next().map(Ok),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(_)) => Some(Err(())),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.picking = None;
                match picked {
                    Some(Ok(path)) => chosen(this, path, window, cx),
                    Some(Err(())) => this.problem = Some(Text::FileDialogFailed),
                    None => {}
                }
                cx.notify();
            });
        }));
    }

    /// Picks where the new project's folder goes.
    fn choose_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_folder(
            Text::ProjectFolderChoose,
            window,
            cx,
            |this, place, _, _| {
                if let Some(form) = &mut this.form {
                    form.place = Some(place.clone());
                    form.error = None;
                }
                if let Some(form) = &mut this.import {
                    form.place = Some(place);
                    form.error = None;
                }
            },
        );
    }

    /// Points a project whose folder cannot be found at its folder.
    fn locate(&mut self, id: ProjectId, window: &mut Window, cx: &mut Context<Self>) {
        self.problem = None;
        self.ask_folder(
            Text::LocateFolderPrompt,
            window,
            cx,
            move |this, folder, _, cx| {
                let located = this.bardo.update(cx, |bardo, cx| {
                    let located = bardo.locate_project_folder(id, &folder);
                    cx.notify();
                    located
                });
                this.problem = located.err().map(|error| error.message());
            },
        );
    }

    fn pick_color(&mut self, color: ProjectColor, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.form {
            form.color = color;
        }
        cx.notify();
    }

    fn cancel_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
        cx.notify();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else {
            return;
        };
        let draft = ProjectDraft {
            name: form.name.read(cx).value().to_string(),
            color: form.color,
        };
        let (id, place) = (form.id, form.place.clone());
        match self
            .bardo
            .read(cx)
            .create_project_in(draft, id, place.as_deref())
        {
            Ok(_) => {
                self.set_open(false, window, cx);
                cx.emit(ProjectSwitched);
            }
            Err(error) => {
                if let Some(form) = &mut self.form {
                    form.error = Some(error.message());
                }
                cx.notify();
            }
        }
    }

    /// Asks where to save the open project's package, then queues writing
    /// it.
    fn export(&mut self, project: ProjectId, name: &str, cx: &mut Context<Self>) {
        self.problem = None;
        self.notice = None;
        let chosen =
            cx.prompt_for_new_path(&bardo_app::backup_folder(), Some(&package_file_name(name)));
        let name = name.to_owned();
        self.picking = Some(cx.spawn(async move |this, cx| {
            let chosen = chosen.await;
            let _ = this.update(cx, |this, cx| {
                this.picking = None;
                match chosen {
                    Ok(Ok(Some(path))) => {
                        let started = this.bardo.update(cx, |bardo, cx| {
                            let started = bardo.export_project_package(project, path);
                            cx.notify();
                            started
                        });
                        match started {
                            Ok(_) => {
                                let bardo = this.bardo.read(cx);
                                this.notice = Some(
                                    bardo.text_with(Text::PackageExportStarted, &[("name", &name)]),
                                );
                            }
                            Err(error) => this.problem = Some(error.message()),
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(_)) => this.problem = Some(Text::FileDialogFailed),
                }
                cx.notify();
            });
        }));
    }

    /// Asks for a package, reads it, and opens the import dialog.
    fn start_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.problem = None;
        self.notice = None;
        self.ask_path(
            true,
            Text::PackageImportPrompt,
            window,
            cx,
            |this, package, window, cx| {
                let offer = this.bardo.read(cx).inspect_package(&package);
                match offer {
                    Ok(offer) => this.open_import(offer, window, cx),
                    Err(error) => this.problem = Some(error.message()),
                }
            },
        );
    }

    fn open_import(&mut self, offer: PackageOffer, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = tr(self.bardo.read(cx), Text::ProjectNamePlaceholder);
        let typed = offer.name.clone();
        let name = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(placeholder);
            input.set_value(typed, window, cx);
            input
        });
        let subscription = cx.subscribe_in(
            &name,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.import(window, cx),
                InputEvent::Change => {
                    if let Some(form) = &mut this.import {
                        form.error = None;
                    }
                    cx.notify();
                }
                _ => {}
            },
        );
        name.update(cx, |name, cx| name.focus(window, cx));
        self.import = Some(ImportForm {
            color: offer.manifest.color,
            offer,
            id: ProjectId::new(),
            name,
            place: None,
            templates: Vec::new(),
            error: None,
            _subscription: subscription,
        });
    }

    fn tick_template(&mut self, kind: TemplateKind, ticked: bool, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.import {
            form.templates.retain(|other| *other != kind);
            if ticked {
                form.templates.push(kind);
            }
        }
        cx.notify();
    }

    fn cancel_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.import = None;
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
        cx.notify();
    }

    /// Queues the import chosen in the dialog.
    fn import(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.import else {
            return;
        };
        let choice = PackageChoice {
            id: form.id,
            package: form.offer.package.clone(),
            name: form.name.read(cx).value().trim().to_owned(),
            color: form.color,
            place: form.place.clone(),
            templates: form.templates.clone(),
        };
        let name = choice.name.clone();
        let queued = self.bardo.update(cx, |bardo, cx| {
            let queued = bardo.import_package(choice);
            cx.notify();
            queued
        });
        match queued {
            Ok(_) => {
                self.import = None;
                let bardo = self.bardo.read(cx);
                self.notice = Some(bardo.text_with(Text::PackageImportStarted, &[("name", &name)]));
            }
            Err(error) => {
                if let Some(form) = &mut self.import {
                    form.error = Some(error.message());
                }
            }
        }
        cx.notify();
    }

    /// Asks for a project's folder, then relinks it or brings it back,
    /// and opens it.
    fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.problem = None;
        self.notice = None;
        self.ask_folder(Text::OpenFolderPrompt, window, cx, |this, folder, _, cx| {
            let opened = this.bardo.update(cx, |bardo, cx| {
                let opened = bardo.open_project_folder(&folder);
                cx.notify();
                opened
            });
            let (id, said) = match opened {
                Ok(FolderOpened::Relinked(id)) => (id, Text::FolderRelinked),
                Ok(FolderOpened::Imported(imported)) => (imported.project, Text::FolderReopened),
                Err(error) => {
                    this.problem = Some(error.message());
                    return;
                }
            };
            // It opens, and the list stays up to say what happened. An
            // archived one stays archived: Unarchive opens it.
            let bardo = this.bardo.read(cx);
            let Some(project) = bardo
                .projects()
                .ok()
                .and_then(|all| all.into_iter().find(|project| project.id == id))
            else {
                this.problem = Some(Text::ProjectNotFoundAny);
                return;
            };
            let name = project.name().to_owned();
            if !project.is_active() {
                this.notice = Some(bardo.text_with(Text::FolderOpenedArchived, &[("name", &name)]));
                return;
            }
            if let Err(error) = bardo.switch_project(id) {
                this.problem = Some(error.message());
                return;
            }
            this.notice = Some(bardo.text_with(said, &[("name", &name)]));
            cx.emit(ProjectSwitched);
        });
    }

    /// Opens the archive or delete dialog for `project`.
    fn start_removal(
        &mut self,
        project: ProjectId,
        name: String,
        mode: RemovalMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bardo = self.bardo.clone();
        let target = Removable::Project(project);
        let panel = cx.new(|cx| RemovalPanel::new(bardo, target, mode, name, window, cx));
        let subscription = cx.subscribe_in(
            &panel,
            window,
            |this, _, event: &RemovalEvent, window, cx| match event {
                RemovalEvent::Done(_) => {
                    this.removal = None;
                    if this.archived {
                        cx.notify();
                    } else {
                        this.set_open(false, window, cx);
                    }
                    // The open project may have changed.
                    cx.emit(ProjectSwitched);
                }
                RemovalEvent::Cancelled => {
                    this.removal = None;
                    cx.notify();
                }
            },
        );
        self.problem = None;
        self.removal = Some((panel, subscription));
        cx.notify();
    }

    fn show_archived(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.archived = shown;
        self.problem = None;
        cx.notify();
    }

    fn unarchive(&mut self, id: ProjectId, cx: &mut Context<Self>) {
        let unarchived = self.bardo.update(cx, |bardo, cx| {
            let unarchived = bardo.unarchive_project(id);
            cx.notify();
            unarchived
        });
        match unarchived {
            // Back now: it is in the list again.
            Ok(None) => {
                self.problem = None;
                cx.emit(ProjectSwitched);
            }
            // Back once its media is unpacked.
            Ok(Some(_)) => self.problem = Some(Text::UnarchiveUnpacking),
            Err(error) => self.problem = Some(error.message()),
        }
        cx.notify();
    }

    /// The button the popover hangs from: the open project's color and
    /// name, or that there is none yet.
    fn trigger(&self, open: Option<&Project>, cx: &App) -> Button {
        let bardo = self.bardo.read(cx);
        let label = match open {
            Some(project) => SharedString::from(project.name().to_owned()),
            None if bardo.loose_open() => tr(bardo, Text::LooseSwitcherRow),
            None => tr(bardo, Text::SwitcherNoProject),
        };
        Button::new("project-switcher")
            .ghost()
            .xsmall()
            .tooltip(tr(bardo, Text::SwitcherOpen))
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .max_w(px(260.))
                    .children(
                        open.map(|project| kit::project_swatch(project.details.color(), px(10.))),
                    )
                    .child(
                        div()
                            .truncate()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(label),
                    )
                    .child(Icon::new(IconName::ChevronsUpDown).size(px(12.))),
            )
    }

    /// How many channels the open project has, beside the button.
    fn channel_count(&self, cx: &App) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        if bardo.loose_open() {
            return Some(
                div()
                    .text_xs()
                    .text_color(look(cx).tokens.text2)
                    .whitespace_nowrap()
                    .child(SharedString::from(loose_count_text(
                        bardo,
                        bardo.loose_video_count(),
                    )))
                    .into_any_element(),
            );
        }
        let open = bardo.open_project().ok().flatten()?;
        let count = bardo
            .project_channels(open.id)
            .map(|c| c.len())
            .unwrap_or(0);
        Some(
            div()
                .text_xs()
                .text_color(look(cx).tokens.text2)
                .whitespace_nowrap()
                .child(SharedString::from(channel_count_text(bardo, count)))
                .into_any_element(),
        )
    }

    /// One project row: its color, name, channels and when it was opened;
    /// a star on favorites and recent ones, and a way up on favorites.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        project: &Project,
        count: usize,
        open: Option<ProjectId>,
        favorite_at: Option<usize>,
        starrable: bool,
        unreachable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let id = project.id;
        let starred = project.favorite.is_some();
        let selected = open == Some(id);
        let key = id.to_string();
        let star = starrable.then(|| {
            let (icon, label) = if starred {
                (IconName::StarFill, Text::SwitcherUnstar)
            } else {
                (IconName::Star, Text::SwitcherStar)
            };
            Button::new(SharedString::from(format!("project-star-{key}")))
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(tr(bardo, label))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    this.star(id, !starred, cx);
                }))
                .into_any_element()
        });
        let up = favorite_at.filter(|at| *at > 0).map(|at| {
            Button::new(SharedString::from(format!("project-up-{key}")))
                .ghost()
                .xsmall()
                .icon(IconName::ArrowUp)
                .tooltip(tr(bardo, Text::SwitcherMoveUp))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    this.move_up(id, at - 1, cx);
                }))
                .into_any_element()
        });
        let opened = match project.opened_at {
            Some(_) => None,
            None => Some(tr(bardo, Text::ProjectNeverOpened)),
        };
        let lost = unreachable.then(|| {
            h_flex()
                .gap_1()
                .items_center()
                .text_xs()
                .text_color(t.warning)
                .child(Icon::new(IconName::TriangleAlert).size(px(12.)))
                .child(tr(bardo, Text::FolderNotFound))
        });
        h_flex()
            .id(SharedString::from(format!("project-row-{key}")))
            .px_2()
            .py_1()
            .gap_2()
            .items_center()
            .rounded(t.radius)
            .cursor_pointer()
            .text_sm()
            .when(selected, |row| row.bg(t.selected))
            .when(!selected, |row| row.hover(|row| row.bg(t.hover)))
            .child(kit::project_swatch(project.details.color(), px(10.)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .when(selected, |name| {
                                name.font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            })
                            .child(SharedString::from(project.name().to_owned())),
                    )
                    .child(div().text_xs().text_color(t.text2).truncate().child(
                        SharedString::from(match opened {
                            Some(never) => {
                                format!("{} · {never}", channel_count_text(bardo, count))
                            }
                            None => channel_count_text(bardo, count),
                        }),
                    ))
                    .children(lost),
            )
            .children(up)
            .children(star)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.switch(id, window, cx);
            }))
            .into_any_element()
    }

    fn group(&self, title: Text, rows: Vec<AnyElement>, cx: &App) -> Option<AnyElement> {
        if rows.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .px_2()
                        .pt_1()
                        .text_xs()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .text_color(look(cx).tokens.text2)
                        .child(tr(self.bardo.read(cx), title)),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let open = bardo
            .open_project()
            .ok()
            .flatten()
            .map(|project| project.id);
        let list = match bardo.project_list(&self.query) {
            Ok(list) => list,
            Err(_) => {
                return kit::notice(kit::Tone::Danger, tr(bardo, Text::ProjectListNotLoaded), cx)
                    .into_any_element();
            }
        };
        let none_at_all = bardo.projects().map(|all| all.is_empty()).unwrap_or(true);
        // Counted once for every row, rather than reading the channels per row.
        let mut counts: HashMap<ProjectId, usize> = HashMap::new();
        for channel in bardo.all_channels().unwrap_or_default() {
            *counts.entry(channel.project).or_default() += 1;
        }
        let count = |id: ProjectId| counts.get(&id).copied().unwrap_or(0);
        let unreachable: HashSet<ProjectId> = bardo.unreachable_projects();
        let lost = |id: ProjectId| unreachable.contains(&id);
        let favorites: Vec<AnyElement> = list
            .favorites
            .iter()
            .enumerate()
            // Reordering only makes sense on the whole list.
            .map(|(at, project)| {
                let at = self.query.trim().is_empty().then_some(at);
                self.row(
                    project,
                    count(project.id),
                    open,
                    at,
                    true,
                    lost(project.id),
                    cx,
                )
            })
            .collect();
        let recent: Vec<AnyElement> = list
            .recent
            .iter()
            .map(|project| {
                self.row(
                    project,
                    count(project.id),
                    open,
                    None,
                    true,
                    lost(project.id),
                    cx,
                )
            })
            .collect();
        let all: Vec<AnyElement> = list
            .all
            .iter()
            .map(|project| {
                self.row(
                    project,
                    count(project.id),
                    open,
                    None,
                    true,
                    lost(project.id),
                    cx,
                )
            })
            .collect();
        // *No project* closes the list, unless a search narrows it.
        let loose = self.query.trim().is_empty().then(|| self.loose_row(cx));
        let bardo = self.bardo.read(cx);
        let empty = if none_at_all {
            Some(tr(bardo, Text::SwitcherEmpty))
        } else if list.is_empty() {
            Some(tr(bardo, Text::SwitcherNoMatch))
        } else {
            None
        };
        let problem = self
            .problem
            .map(|problem| kit::notice(kit::Tone::Danger, tr(bardo, problem), cx));
        let notice = self
            .notice
            .clone()
            .map(|notice| kit::notice(kit::Tone::Info, notice, cx));
        let new_label = tr(bardo, Text::SwitcherNewProject);
        let folder = open.and_then(|id| Some((id, bardo.project_folder(id)?)));
        let folder_part = folder.map(|(id, folder)| {
            let missing = unreachable.contains(&id);
            let show = folder.clone();
            v_flex()
                .gap_2()
                .when(missing, |part| {
                    part.child(kit::notice(
                        kit::Tone::Warning,
                        bardo.text_with(
                            Text::FolderNotFoundHint,
                            &[("folder", &folder.display().to_string())],
                        ),
                        cx,
                    ))
                })
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .when(missing, |row| {
                            row.child(
                                Button::new("project-locate")
                                    .small()
                                    .label(tr(bardo, Text::LocateFolder))
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, window, cx| {
                                            this.locate(id, window, cx);
                                        },
                                    )),
                            )
                        })
                        .when(!missing, |row| {
                            row.child(
                                Button::new("project-show-folder")
                                    .ghost()
                                    .small()
                                    .icon(IconName::FolderOpen)
                                    .label(tr(bardo, Text::ShowFolder))
                                    .on_click(move |_, _, cx| cx.open_with_system(&show)),
                            )
                        }),
                )
        });
        let open_name = open.and_then(|id| {
            bardo
                .projects()
                .ok()?
                .into_iter()
                .find(|project| project.id == id)
                .map(|project| (id, project.name().to_owned()))
        });
        let removal_part = open_name.map(|(id, name)| {
            let archive_name = name.clone();
            let export_name = name.clone();
            let exporting = bardo.project_export_job(id).is_some();
            h_flex()
                .gap_1()
                .flex_wrap()
                .child(
                    Button::new("project-keys")
                        .ghost()
                        .small()
                        .icon(IconName::Settings)
                        .label(tr(bardo, Text::SwitcherProjectKeys))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.set_open(false, window, cx);
                            crate::guide::go(
                                bardo_app::GuidePlace::Settings(bardo_app::SettingsTab::Project),
                                window,
                                cx,
                            );
                        })),
                )
                .child(
                    Button::new("project-export")
                        .ghost()
                        .small()
                        .icon(IconName::ArrowUp)
                        .label(tr(
                            bardo,
                            if exporting {
                                Text::SwitcherExporting
                            } else {
                                Text::SwitcherExportProject
                            },
                        ))
                        .disabled(exporting || self.picking.is_some())
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.export(id, &export_name, cx);
                        })),
                )
                .child(
                    Button::new("project-archive")
                        .ghost()
                        .small()
                        .icon(IconName::Inbox)
                        .label(tr(bardo, Text::SwitcherArchiveProject))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.start_removal(
                                id,
                                archive_name.clone(),
                                RemovalMode::Archive,
                                window,
                                cx,
                            );
                        })),
                )
                .child(
                    Button::new("project-delete")
                        .ghost()
                        .small()
                        .icon(IconName::Delete)
                        .label(tr(bardo, Text::SwitcherDeleteProject))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.start_removal(id, name.clone(), RemovalMode::Delete, window, cx);
                        })),
                )
        });
        let archived_count = bardo
            .archived_projects()
            .map(|archived| archived.len())
            .unwrap_or(0);
        let archived_label = SharedString::from(bardo.text_with(
            Text::SwitcherArchivedProjects,
            &[("n", &archived_count.to_string())],
        ));
        v_flex()
            .gap_2()
            .child(Input::new(&self.search).small().cleanable(true))
            .child(
                v_flex()
                    .id("project-switcher-list")
                    .max_h(px(380.))
                    .overflow_y_scroll()
                    .gap_2()
                    .children(self.group(Text::SwitcherFavorites, favorites, cx))
                    .children(self.group(Text::SwitcherRecent, recent, cx))
                    .children(self.group(Text::SwitcherAll, all, cx))
                    .children(loose)
                    .children(empty.map(|text| {
                        div()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .text_color(t.text2)
                            .child(text)
                    })),
            )
            .children(problem)
            .children(notice)
            .child(div().h(t.border_width).bg(t.border))
            .children(folder_part)
            .children(removal_part)
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .child(
                        Button::new("project-new")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .label(new_label)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.start_new(window, cx);
                            })),
                    )
                    .child(
                        Button::new("project-archived")
                            .ghost()
                            .small()
                            .icon(IconName::Inbox)
                            .label(archived_label)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.show_archived(true, cx);
                            })),
                    )
                    .child(
                        Button::new("project-import")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowDown)
                            .label(tr(bardo, Text::SwitcherImportPackage))
                            .disabled(self.picking.is_some())
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.start_import(window, cx);
                            })),
                    )
                    .child(
                        Button::new("project-open-folder")
                            .ghost()
                            .small()
                            .icon(IconName::FolderOpen)
                            .label(tr(bardo, Text::SwitcherOpenFolder))
                            .disabled(self.picking.is_some())
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.open_folder(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// *Archived projects*: each with its color and name, whether its
    /// media is compressed, and *Unarchive* and *Delete…*.
    fn render_archived(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let archived = bardo.archived_projects().unwrap_or_default();
        let rows: Vec<AnyElement> = archived
            .into_iter()
            .map(|archived| self.archived_row(archived, cx))
            .collect();
        let bardo = self.bardo.read(cx);
        let empty = rows.is_empty().then(|| {
            div()
                .px_2()
                .text_sm()
                .text_color(t.text2)
                .child(tr(bardo, Text::ArchivedProjectsEmpty))
        });
        let problem = self
            .problem
            .map(|problem| kit::notice(kit::Tone::Info, tr(bardo, problem), cx));
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("project-archived-back")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronLeft)
                            .tooltip(tr(bardo, Text::SwitcherBack))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.show_archived(false, cx);
                            })),
                    )
                    .child(kit::section_heading(tr(bardo, Text::ArchivedProjectsTitle))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(t.text2)
                    .child(tr(bardo, Text::ArchivedProjectsHint)),
            )
            .child(
                v_flex()
                    .id("project-archived-list")
                    .max_h(px(380.))
                    .overflow_y_scroll()
                    .gap_1()
                    .children(rows)
                    .children(empty),
            )
            .children(problem)
            .into_any_element()
    }

    fn archived_row(&self, archived: ArchivedProject, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let ArchivedProject {
            project,
            packed,
            busy,
        } = archived;
        let (id, name) = (project.id, project.name().to_owned());
        let key = id.to_string();
        let state = if busy {
            Some(tr(bardo, Text::ArchivedBusy))
        } else if packed {
            Some(tr(bardo, Text::ArchivedPacked))
        } else {
            None
        };
        h_flex()
            .px_2()
            .py_1()
            .gap_2()
            .items_center()
            .rounded(t.radius)
            .text_sm()
            .child(kit::project_swatch(project.details.color(), px(10.)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(SharedString::from(name.clone())))
                    .children(state.map(|state| div().text_xs().text_color(t.text2).child(state))),
            )
            .child(
                Button::new(SharedString::from(format!("project-unarchive-{key}")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Undo)
                    .label(tr(bardo, Text::Unarchive))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.unarchive(id, cx);
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("project-delete-{key}")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Delete)
                    .tooltip(tr(bardo, Text::DeleteAction))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.start_removal(id, name.clone(), RemovalMode::Delete, window, cx);
                    })),
            )
            .into_any_element()
    }

    fn render_form(&self, form: &NewProject, cx: &mut Context<Self>) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let swatches: Vec<AnyElement> = ProjectColor::ALL
            .into_iter()
            .map(|color| {
                let picked = form.color == color;
                div()
                    .id(SharedString::from(format!(
                        "project-color-{}",
                        color.code()
                    )))
                    .p(px(3.))
                    .rounded_full()
                    .border_2()
                    .border_color(if picked {
                        t.focus
                    } else {
                        gpui_kit::transparent_black()
                    })
                    .cursor_pointer()
                    .tooltip({
                        let name = tr(bardo, Text::ProjectColorName(color));
                        move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(name.clone())
                                .build(window, cx)
                        }
                    })
                    .child(kit::project_swatch(color, px(16.)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.pick_color(color, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let error = form.error.map(|error| {
            div()
                .text_xs()
                .text_color(t.danger)
                .child(tr(bardo, error))
                .into_any_element()
        });
        let typed = form.name.read(cx).value().to_string();
        let folder = bardo.new_project_folder(
            if typed.trim().is_empty() {
                "…"
            } else {
                typed.trim()
            },
            form.id,
            form.place.as_deref(),
        );
        let folder_field = kit::field(
            tr(bardo, Text::ProjectFolderLabel),
            None,
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(t.text2)
                                .truncate()
                                .child(SharedString::from(folder.display().to_string())),
                        )
                        .child(
                            Button::new("project-new-folder")
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::ProjectFolderChoose))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.choose_place(window, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(t.text2)
                        .child(tr(bardo, Text::ProjectFolderHint)),
                )
                .into_any_element(),
            None,
        );
        v_flex()
            .gap_3()
            .child(kit::section_heading(tr(bardo, Text::NewProjectTitle)))
            .child(
                div()
                    .text_xs()
                    .text_color(t.text2)
                    .child(tr(bardo, Text::NewProjectHint)),
            )
            .child(kit::field(
                tr(bardo, Text::ProjectNameLabel),
                None,
                Input::new(&form.name).small().into_any_element(),
                error,
            ))
            .child(kit::field(
                tr(bardo, Text::ProjectColorLabel),
                None,
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .children(swatches)
                    .into_any_element(),
                None,
            ))
            .child(folder_field)
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("project-new-cancel")
                            .ghost()
                            .small()
                            .label(tr(bardo, Text::ProjectFormCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.cancel_new(window, cx);
                            })),
                    )
                    .child(
                        Button::new("project-new-create")
                            .primary()
                            .small()
                            .label(tr(bardo, Text::CreateProject))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.create(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// The import dialog: the new project's name, color and folder, what
    /// the package holds, and which of its global templates come in as
    /// the project's own.
    fn render_import(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = &self.import else {
            return div().into_any_element();
        };
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let manifest = &form.offer.manifest;
        let error = form.error.map(|error| {
            div()
                .text_xs()
                .text_color(t.danger)
                .child(tr(bardo, error))
                .into_any_element()
        });
        let typed = form.name.read(cx).value().to_string();
        let folder = bardo.new_project_folder(
            if typed.trim().is_empty() {
                "…"
            } else {
                typed.trim()
            },
            form.id,
            form.place.as_deref(),
        );
        let swatches: Vec<AnyElement> = ProjectColor::ALL
            .into_iter()
            .map(|color| {
                let picked = form.color == color;
                div()
                    .id(SharedString::from(format!(
                        "project-import-color-{}",
                        color.code()
                    )))
                    .p(px(3.))
                    .rounded_full()
                    .border_2()
                    .border_color(if picked {
                        t.focus
                    } else {
                        gpui_kit::transparent_black()
                    })
                    .cursor_pointer()
                    .child(kit::project_swatch(color, px(16.)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(form) = &mut this.import {
                            form.color = color;
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let templates: Vec<AnyElement> = manifest
            .contents
            .global_templates
            .iter()
            .map(|&kind| {
                Checkbox::new(SharedString::from(format!(
                    "project-import-template-{}",
                    kind.code()
                )))
                .label(tr(bardo, Text::TemplateKindName(kind)))
                .checked(form.templates.contains(&kind))
                .on_click(cx.listener(move |this, ticked: &bool, _, cx| {
                    this.tick_template(kind, *ticked, cx);
                }))
                .into_any_element()
            })
            .collect();
        let templates_field = (!templates.is_empty()).then(|| {
            kit::field(
                tr(bardo, Text::ImportPackageTemplates),
                None,
                v_flex()
                    .gap_1()
                    .children(templates)
                    .child(
                        div()
                            .text_xs()
                            .text_color(t.text2)
                            .child(tr(bardo, Text::ImportPackageTemplatesHint)),
                    )
                    .into_any_element(),
                None,
            )
        });
        let title = bardo.text_with(Text::ImportPackageTitle, &[("name", &manifest.name)]);
        v_flex()
            .id("project-import")
            .gap_3()
            .max_h(px(520.))
            .overflow_y_scroll()
            .child(kit::section_heading(title))
            .child(
                v_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(t.text2)
                    .child(SharedString::from(bardo.package_from_text(manifest)))
                    .child(SharedString::from(bardo.package_contents_text(manifest)))
                    .child(tr(bardo, Text::ImportPackageHint)),
            )
            .child(kit::field(
                tr(bardo, Text::ProjectNameLabel),
                None,
                Input::new(&form.name).small().into_any_element(),
                error,
            ))
            .child(kit::field(
                tr(bardo, Text::ProjectColorLabel),
                None,
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .children(swatches)
                    .into_any_element(),
                None,
            ))
            .child(kit::field(
                tr(bardo, Text::ProjectFolderLabel),
                None,
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(t.text2)
                            .truncate()
                            .child(SharedString::from(folder.display().to_string())),
                    )
                    .child(
                        Button::new("project-import-folder")
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, Text::ProjectFolderChoose))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.choose_place(window, cx);
                            })),
                    )
                    .into_any_element(),
                None,
            ))
            .children(templates_field)
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("project-import-cancel")
                            .ghost()
                            .small()
                            .label(tr(bardo, Text::ProjectFormCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.cancel_import(window, cx);
                            })),
                    )
                    .child(
                        Button::new("project-import-confirm")
                            .primary()
                            .small()
                            .label(tr(bardo, Text::ImportPackageConfirm))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.import(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// The popover's content: the list, or the new project form.
    fn render_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match (&self.removal, &self.form) {
            (Some((panel, _)), _) => panel.clone().into_any_element(),
            (None, Some(form)) => self.render_form(form, cx),
            (None, None) if self.import.is_some() => self.render_import(cx),
            (None, None) if self.archived => self.render_archived(cx),
            (None, None) => self.render_list(cx),
        };
        div().w(px(WIDTH)).child(body).into_any_element()
    }
}

/// "No loose video", "1 loose video", "3 loose videos".
fn loose_count_text(bardo: &Bardo, count: usize) -> String {
    match count {
        0 => bardo.text(Text::LooseCountNone).into_owned(),
        1 => bardo.text(Text::LooseCountOne).into_owned(),
        n => bardo.text_with(Text::LooseCountMany, &[("n", &n.to_string())]),
    }
}

/// "No channel", "1 channel", "3 channels".
fn channel_count_text(bardo: &Bardo, count: usize) -> String {
    match count {
        0 => bardo.text(Text::ProjectChannelsNone).into_owned(),
        1 => bardo.text(Text::ProjectChannelsOne).into_owned(),
        n => bardo.text_with(Text::ProjectChannelsMany, &[("n", &n.to_string())]),
    }
}

impl Render for ProjectSwitcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open_project = self.bardo.read(cx).open_project().ok().flatten();
        let trigger = self.trigger(open_project.as_ref(), cx);
        let count = self.channel_count(cx);
        let this = cx.entity();
        let on_change = cx.entity().downgrade();
        let popover = Popover::new("project-switcher-popover")
            .open(self.open)
            .on_open_change(move |open, window, cx| {
                let open = *open;
                let _ = on_change.update(cx, |this, cx| this.set_open(open, window, cx));
            })
            .trigger(trigger)
            .content(move |_, _, cx| this.update(cx, |this, cx| this.render_panel(cx)));
        kit::anchor(
            TourAnchor::Control(Control::OpenProject),
            h_flex()
                .gap_2()
                .items_center()
                .child(popover)
                .children(count),
        )
    }
}
