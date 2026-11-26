//! The project switcher (issue #333): in the title bar, beside Bardo's
//! mark, the open project's color and name, and a popover that opens
//! another one. The popover searches the projects and lists the
//! favorites (in the user's order, each with a star and a way up),
//! the recent ones and all the others by name, then *New project…*,
//! whose name and color form opens the new project.
//!
//! The list and its rules live in `bardo_app` (`project_list`,
//! `switch_project`, `create_project`); this file draws them and tells
//! the shell when the open project changed, so the screens re-read it.

use bardo_app::bardo_domain::{Project, ProjectColor, ProjectDraft, ProjectId};
use bardo_app::{Bardo, Control, Text, TourAnchor};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, SharedString, Subscription, Window, div, px,
};

use crate::appearance::look;
use crate::kit;
use crate::shell::tr;

/// Width of the open popover.
const WIDTH: f32 = 320.;

/// The open project changed: the screens re-read what they show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectSwitched;

/// The new project form, while it is open in the popover.
struct NewProject {
    name: Entity<InputState>,
    color: ProjectColor,
    error: Option<Text>,
    _subscription: Subscription,
}

pub struct ProjectSwitcher {
    bardo: Entity<Bardo>,
    open: bool,
    search: Entity<InputState>,
    query: String,
    form: Option<NewProject>,
    /// What went wrong with the last switch, star or move.
    problem: Option<Text>,
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
            problem: None,
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
        self.problem = None;
        if open {
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
            name,
            color,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
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
        match self.bardo.read(cx).create_project(draft) {
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

    /// The button the popover hangs from: the open project's color and
    /// name, or that there is none yet.
    fn trigger(&self, open: Option<&Project>, cx: &App) -> Button {
        let bardo = self.bardo.read(cx);
        let label = match open {
            Some(project) => SharedString::from(project.name().to_owned()),
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
    fn row(
        &self,
        project: &Project,
        open: Option<ProjectId>,
        favorite_at: Option<usize>,
        starrable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = look(cx).tokens;
        let bardo = self.bardo.read(cx);
        let id = project.id;
        let count = bardo.project_channels(id).map(|c| c.len()).unwrap_or(0);
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
                    )),
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
        let favorites: Vec<AnyElement> = list
            .favorites
            .iter()
            .enumerate()
            // Reordering only makes sense on the whole list.
            .map(|(at, project)| {
                let at = self.query.trim().is_empty().then_some(at);
                self.row(project, open, at, true, cx)
            })
            .collect();
        let recent: Vec<AnyElement> = list
            .recent
            .iter()
            .map(|project| self.row(project, open, None, true, cx))
            .collect();
        let all: Vec<AnyElement> = list
            .all
            .iter()
            .map(|project| self.row(project, open, None, true, cx))
            .collect();
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
        let new_label = tr(bardo, Text::SwitcherNewProject);
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
            .child(div().h(t.border_width).bg(t.border))
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

    /// The popover's content: the list, or the new project form.
    fn render_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match &self.form {
            Some(form) => self.render_form(form, cx),
            None => self.render_list(cx),
        };
        div().w(px(WIDTH)).child(body).into_any_element()
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
