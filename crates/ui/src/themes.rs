//! Themes screen: pick a channel and niche, have ideas proposed and ranked,
//! then edit, discard or approve each one; approving starts a video
//! project. Proposing and ranking run as jobs in `bardo_app`; this view
//! polls the job revision and re-reads the ideas when it moves.

use std::time::Duration;

use bardo_app::bardo_domain::{
    Channel, ChannelId, JobKind, JobState, NicheScores, Reason, Theme, ThemeFieldError, ThemeId,
    ThemeStatus,
};
use bardo_app::{Bardo, SUGGESTIONS_PER_RUN, Text, ThemesView};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, SharedString, Subscription, Task, Window, div, px,
};

use crate::shell::tr;

/// How often the screen checks the job queue for changes.
const POLL_EVERY: Duration = Duration::from_millis(100);

/// One option of a select.
#[derive(Clone)]
struct Choice<T> {
    value: T,
    title: SharedString,
}

impl<T: Clone + PartialEq> SearchableListItem for Choice<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &T {
        &self.value
    }
}

type ChoiceSelect<T> = Entity<SelectState<SearchableVec<Choice<T>>>>;

/// The idea being edited, and what was wrong with the last save.
struct Editing {
    id: ThemeId,
    errors: Vec<ThemeFieldError>,
}

pub struct ThemesScreen {
    bardo: Entity<Bardo>,
    channels: Vec<Channel>,
    channel_select: ChoiceSelect<ChannelId>,
    niche_select: ChoiceSelect<String>,
    selected: Option<ChannelId>,
    /// The niche label shown; `None` until the view picks one.
    niche: Option<String>,
    view: Option<ThemesView>,
    editing: Option<Editing>,
    title: Entity<InputState>,
    angle: Entity<TextareaState>,
    error: Option<Text>,
    /// The title of the project the last approval started.
    started: Option<String>,
    revision: u64,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl ThemesScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let channel_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let niche_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let title = cx.new(|cx| InputState::new(window, cx));
        let angle = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 6));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &channel_select,
                window,
                |this, _, event: &SelectEvent<SearchableVec<Choice<ChannelId>>>, window, cx| {
                    let SelectEvent::Confirm(Some(id)) = event else {
                        return;
                    };
                    if this.selected != Some(*id) {
                        this.select(*id, window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &niche_select,
                window,
                |this, _, event: &SelectEvent<SearchableVec<Choice<String>>>, _, cx| {
                    let SelectEvent::Confirm(Some(label)) = event else {
                        return;
                    };
                    if this.niche.as_ref() != Some(label) {
                        this.niche = Some(label.clone());
                        this.editing = None;
                        this.error = None;
                        this.load(cx);
                        cx.notify();
                    }
                },
            ),
        ];
        let revision = bardo.read(cx).jobs_revision();
        let mut screen = Self {
            bardo,
            channels: Vec::new(),
            channel_select,
            niche_select,
            selected: None,
            niche: None,
            view: None,
            editing: None,
            title,
            angle,
            error: None,
            started: None,
            revision,
            _poll: poll,
            _subscriptions: subscriptions,
        };
        screen.reload_channels(window, cx);
        screen
    }

    /// Re-reads the channels (they change on the channels screen) and the
    /// niches (they change on the research screen), keeping the selection
    /// when it still exists.
    pub fn reload_channels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.started = None;
        self.channels = self.bardo.read(cx).channels().unwrap_or_default();
        let choices = SearchableVec::new(
            self.channels
                .iter()
                .map(|channel| Choice {
                    value: channel.id,
                    title: SharedString::from(channel.details.name().to_owned()),
                })
                .collect::<Vec<_>>(),
        );
        let keep = self
            .selected
            .filter(|id| self.channels.iter().any(|c| c.id == *id));
        let target = keep.or_else(|| self.channels.first().map(|c| c.id));
        self.channel_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            if let Some(id) = target {
                select.set_selected_value(&id, window, cx);
            }
        });
        match target {
            Some(id) if keep.is_some() => {
                self.selected = Some(id);
                self.load(cx);
                self.sync_niches(window, cx);
            }
            Some(id) => self.select(id, window, cx),
            None => {
                self.selected = None;
                self.view = None;
            }
        }
        cx.notify();
    }

    fn select(&mut self, id: ChannelId, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.niche = None;
        self.editing = None;
        self.error = None;
        self.started = None;
        self.load(cx);
        self.sync_niches(window, cx);
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        match self.bardo.read(cx).themes(id, self.niche.as_deref()) {
            Ok(view) => {
                self.niche = view.niche.as_ref().map(|niche| niche.label().to_owned());
                // The edited idea may have been approved meanwhile, or
                // discarded by another screen.
                if let Some(editing) = &self.editing
                    && !view.themes.iter().any(|theme| {
                        theme.id == editing.id && theme.status() == ThemeStatus::Suggested
                    })
                {
                    self.editing = None;
                }
                self.view = Some(view);
            }
            Err(_) => {
                self.view = None;
                self.error = Some(Text::ThemesNotLoaded);
            }
        }
    }

    fn sync_niches(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let niches: Vec<Choice<String>> = self
            .view
            .iter()
            .flat_map(|view| view.niches.iter())
            .map(|niche| Choice {
                value: niche.label().to_owned(),
                title: SharedString::from(niche.label().to_owned()),
            })
            .collect();
        let current = self.niche.clone();
        self.niche_select.update(cx, |select, cx| {
            select.set_items(SearchableVec::new(niches), window, cx);
            if let Some(label) = &current {
                select.set_selected_value(label, window, cx);
            }
        });
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let revision = self.bardo.read(cx).jobs_revision();
        if revision != self.revision {
            self.revision = revision;
            self.load(cx);
            cx.notify();
        }
    }

    fn running(&self) -> bool {
        self.view
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .is_some_and(|job| job.state().is_active())
    }

    fn suggest(&mut self, rank_only: bool, cx: &mut Context<Self>) {
        let (Some(id), Some(niche)) = (self.selected, self.niche.clone()) else {
            self.error = Some(Text::ThemesPickNiche);
            cx.notify();
            return;
        };
        let bardo = self.bardo.read(cx);
        let result = if rank_only {
            bardo.rank_themes(id, &niche)
        } else {
            bardo.suggest_themes(id, &niche)
        };
        self.error = result.err().map(|error| error.message());
        self.started = None;
        self.load(cx);
        cx.notify();
    }

    fn start_edit(&mut self, theme: &Theme, window: &mut Window, cx: &mut Context<Self>) {
        let (title, angle) = (theme.idea().title(), theme.idea().angle());
        self.title.update(cx, |input, cx| {
            input.set_value(title.to_owned(), window, cx)
        });
        self.angle.update(cx, |input, cx| {
            input.set_value(angle.to_owned(), window, cx)
        });
        self.editing = Some(Editing {
            id: theme.id,
            errors: Vec::new(),
        });
        self.error = None;
        cx.notify();
    }

    fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = &mut self.editing else {
            return;
        };
        let title = self.title.read(cx).value();
        let angle = self.angle.read(cx).value();
        match self.bardo.read(cx).edit_theme(editing.id, &title, &angle) {
            Ok(_) => {
                self.editing = None;
                self.error = None;
            }
            Err(error) if !error.field_errors().is_empty() => {
                editing.errors = error.field_errors().to_vec();
            }
            Err(error) => {
                self.editing = None;
                self.error = Some(error.message());
            }
        }
        self.load(cx);
        cx.notify();
    }

    fn discard(&mut self, id: ThemeId, cx: &mut Context<Self>) {
        self.error = self
            .bardo
            .read(cx)
            .discard_theme(id)
            .err()
            .map(|error| error.message());
        self.started = None;
        self.load(cx);
        cx.notify();
    }

    fn approve(&mut self, id: ThemeId, cx: &mut Context<Self>) {
        match self.bardo.read(cx).approve_theme(id) {
            Ok(project) => {
                self.error = None;
                self.started = Some(project.title);
            }
            Err(error) => self.error = Some(error.message()),
        }
        self.load(cx);
        cx.notify();
    }

    fn render_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let view = self.view.as_ref();
        let has_niche = view.is_some_and(|view| view.niche.is_some());
        let running = self.running();
        let unranked = view.map_or(0, |view| view.unranked);

        v_flex()
            .id("themes-controls")
            .w(px(340.))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_xl()
                    .font_semibold()
                    .child(tr(bardo, Text::ThemesTitle)),
            )
            .child(field(
                tr(bardo, Text::ThemesChannel),
                Select::new(&self.channel_select).into_any_element(),
            ))
            .child(if has_niche {
                field(
                    tr(bardo, Text::ThemesNiche),
                    Select::new(&self.niche_select).into_any_element(),
                )
                .into_any_element()
            } else {
                muted(cx, tr(bardo, Text::ThemesNoNiche))
            })
            .children(
                view.and_then(|view| view.research)
                    .map(|scores| research_tags(bardo, scores)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(SharedString::from(bardo.text_with(
                        Text::SuggestThemesHint,
                        &[("n", &SUGGESTIONS_PER_RUN.to_string())],
                    ))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("suggest-themes")
                            .primary()
                            .label(tr(bardo, Text::SuggestThemes))
                            .disabled(running || !has_niche)
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.suggest(false, cx)),
                            ),
                    )
                    .when(unranked > 0, |row| {
                        row.child(
                            Button::new("rank-themes")
                                .outline()
                                .label(tr(bardo, Text::RankThemes))
                                .disabled(running)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.suggest(true, cx)
                                })),
                        )
                    }),
            )
            .when(unranked > 0, |panel| {
                panel.child(div().text_xs().text_color(theme.muted_foreground).child(
                    SharedString::from(
                        bardo.text_with(Text::ThemesUnranked, &[("n", &unranked.to_string())]),
                    ),
                ))
            })
            .children(self.error.map(|error| {
                div()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(tr(bardo, error))
            }))
            .children(self.render_job(cx))
    }

    /// Progress of a running job, or why the last one stopped.
    fn render_job(&self, cx: &App) -> Option<AnyElement> {
        let job = self.view.as_ref()?.job.as_ref()?;
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        match job.state() {
            JobState::Queued | JobState::Running => {
                let percent = job.progress().percent();
                let label = match job.kind() {
                    JobKind::ThemeRanking => tr(bardo, Text::JobKindName(JobKind::ThemeRanking)),
                    _ => tr(bardo, Text::ThemesRunning),
                };
                Some(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().child(label))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div().flex_1().child(
                                        Progress::new("themes-progress")
                                            .small()
                                            .value(f32::from(percent)),
                                    ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(SharedString::from(format!("{percent}%"))),
                                ),
                        )
                        .into_any_element(),
                )
            }
            JobState::Failed => {
                let failure = job.failure()?;
                Some(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.danger)
                                .child(tr(bardo, Text::ThemesStopped)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(tr(bardo, Text::JobFailureKindName(failure.kind))),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(failure.detail.clone())),
                        )
                        .into_any_element(),
                )
            }
            JobState::Cancelled | JobState::Done => None,
        }
    }

    fn render_ideas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let discarded = self.view.as_ref().map_or(0, |view| view.discarded);
        let themes: Vec<Theme> = self
            .view
            .iter()
            .flat_map(|view| view.themes.iter().cloned())
            .collect();
        let empty = themes.is_empty();
        let cards: Vec<AnyElement> = themes
            .iter()
            .enumerate()
            .map(|(ix, idea)| self.render_theme(ix, idea, cx))
            .collect();

        v_flex()
            .id("themes-ideas")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_baseline()
                            .child(
                                div()
                                    .text_lg()
                                    .font_semibold()
                                    .child(tr(bardo, Text::ThemesListTitle)),
                            )
                            .when(discarded > 0, |row| {
                                row.child(div().text_xs().text_color(theme.muted_foreground).child(
                                    SharedString::from(bardo.text_with(
                                        Text::ThemesDiscarded,
                                        &[("n", &discarded.to_string())],
                                    )),
                                ))
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, Text::ThemesRankingHint)),
                    ),
            )
            .children(self.started.as_ref().map(|title| {
                div()
                    .text_sm()
                    .text_color(theme.success)
                    .child(SharedString::from(
                        bardo.text_with(Text::ProjectStarted, &[("title", title)]),
                    ))
            }))
            .when(empty, |list| {
                list.child(muted(cx, tr(bardo, Text::ThemesEmpty)))
            })
            .children(cards)
            .child(self.render_projects(cx))
    }

    fn render_theme(&self, ix: usize, idea: &Theme, cx: &Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let editing = self
            .editing
            .as_ref()
            .filter(|editing| editing.id == idea.id);
        let approved = idea.status() == ThemeStatus::Approved;

        let tags = h_flex()
            .gap_1()
            .flex_none()
            .when(approved, |tags| {
                tags.child(Tag::success().small().child(tr(bardo, Text::ThemeApproved)))
            })
            .children(idea.ranking().map(|ranking| {
                h_flex()
                    .gap_1()
                    .child(Tag::primary().small().child(SharedString::from(format!(
                        "{} {}",
                        bardo.text(Text::ThemePriority),
                        ranking.priority().value()
                    ))))
                    .child(Tag::secondary().small().child(SharedString::from(format!(
                        "{} {}%",
                        bardo.text(Text::ThemeConfidence),
                        ranking.confidence().percent()
                    ))))
            }));

        let header = h_flex()
            .gap_3()
            .justify_between()
            .items_start()
            .child(
                h_flex()
                    .gap_2()
                    .min_w_0()
                    .items_start()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(format!("{}.", ix + 1))),
                    )
                    .child(
                        v_flex()
                            .gap_0p5()
                            .min_w_0()
                            .child(
                                div()
                                    .font_medium()
                                    .child(SharedString::from(idea.idea().title().to_owned())),
                            )
                            .when(!idea.idea().angle().is_empty(), |text| {
                                text.child(
                                    div()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(SharedString::from(idea.idea().angle().to_owned())),
                                )
                            }),
                    ),
            )
            .child(tags);

        let reasons: AnyElement = match idea.ranking() {
            Some(ranking) => h_flex()
                .flex_wrap()
                .gap_x_6()
                .gap_y_2()
                .child(reason(bardo, cx, Text::ThemeFit, ranking.fit))
                .child(reason(bardo, cx, Text::ThemeTrend, ranking.trend))
                .child(reason(
                    bardo,
                    cx,
                    Text::ThemeCompetition,
                    ranking.competition,
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .self_end()
                        .child(SharedString::from(format!(
                            "{} · {}",
                            bardo.text_with(Text::ThemeRankedBy, &[("model", &ranking.model)]),
                            bardo.time_ago(ranking.ranked_at)
                        ))),
                )
                .into_any_element(),
            None => div()
                .text_xs()
                .text_color(theme.warning)
                .child(tr(bardo, Text::ThemeNotRanked))
                .into_any_element(),
        };

        let body: AnyElement =
            match editing {
                Some(editing) => self.render_editor(editing, cx),
                None if approved => div().into_any_element(),
                None => {
                    let id = idea.id;
                    let edited = idea.clone();
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(("approve-theme", ix))
                                .primary()
                                .small()
                                .label(tr(bardo, Text::ApproveTheme))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.approve(id, cx)
                                })),
                        )
                        .child(
                            Button::new(("edit-theme", ix))
                                .outline()
                                .small()
                                .label(tr(bardo, Text::EditTheme))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.start_edit(&edited, window, cx)
                                })),
                        )
                        .child(
                            Button::new(("discard-theme", ix))
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::DiscardTheme))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.discard(id, cx)
                                })),
                        )
                        .into_any_element()
                }
            };

        v_flex()
            .id(("theme", ix))
            .p_3()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(if approved {
                theme.success
            } else {
                theme.border
            })
            .child(header)
            .child(reasons)
            .child(body)
            .into_any_element()
    }

    fn render_editor(&self, editing: &Editing, cx: &Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let errors = |fields: &[ThemeFieldError]| -> Vec<AnyElement> {
            editing
                .errors
                .iter()
                .filter(|error| fields.contains(error))
                .map(|error| {
                    div()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(tr(bardo, Text::ThemeFieldError(*error)))
                        .into_any_element()
                })
                .collect()
        };

        v_flex()
            .gap_2()
            .child(
                field(
                    tr(bardo, Text::ThemeTitle),
                    Input::new(&self.title).into_any_element(),
                )
                .children(errors(&[
                    ThemeFieldError::TitleRequired,
                    ThemeFieldError::TitleTooLong,
                ])),
            )
            .child(
                field(
                    tr(bardo, Text::ThemeAngle),
                    Textarea::new(&self.angle).into_any_element(),
                )
                .children(errors(&[ThemeFieldError::AngleTooLong])),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("save-theme")
                            .primary()
                            .small()
                            .label(tr(bardo, Text::SaveTheme))
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.save_edit(cx)),
                            ),
                    )
                    .child(
                        Button::new("cancel-theme")
                            .ghost()
                            .small()
                            .label(tr(bardo, Text::CancelThemeEdit))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.editing = None;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_projects(&self, cx: &App) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let projects = self.view.iter().flat_map(|view| view.projects.iter());
        let rows: Vec<AnyElement> = projects
            .map(|project| {
                h_flex()
                    .gap_3()
                    .justify_between()
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .child(SharedString::from(project.title.clone())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(project.niche.label().to_owned())),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(bardo.time_ago(project.created_at))),
                    )
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();

        v_flex()
            .pt_4()
            .mt_2()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .child(tr(bardo, Text::ProjectsTitle)),
            )
            .when(empty, |list| {
                list.child(muted(cx, tr(bardo, Text::ProjectsEmpty)))
            })
            .children(rows)
    }
}

fn field(label: SharedString, input: AnyElement) -> gpui_kit::Div {
    v_flex()
        .gap_1()
        .child(div().text_sm().font_medium().child(label))
        .child(input)
}

fn muted(cx: &App, text: SharedString) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

/// One reason behind a ranking: its score and how sure the engine was.
fn reason(bardo: &Bardo, cx: &App, label: Text, reason: Reason) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .gap_0p5()
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, label)),
        )
        .child(
            h_flex()
                .gap_1()
                .items_baseline()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(SharedString::from(reason.score.value().to_string())),
                )
                .child(div().text_xs().text_color(theme.muted_foreground).child(
                    SharedString::from(format!(
                        "{} {}%",
                        bardo.text(Text::ThemeConfidence),
                        reason.confidence.percent()
                    )),
                )),
        )
}

/// The niche's research scores, as on the research screen.
fn research_tags(bardo: &Bardo, scores: NicheScores) -> impl IntoElement {
    let tag = |label: Text, value: u8| SharedString::from(format!("{} {value}", bardo.text(label)));
    h_flex()
        .gap_1()
        .flex_wrap()
        .child(
            Tag::primary()
                .small()
                .child(tag(Text::ResearchOpportunity, scores.opportunity().value())),
        )
        .child(
            Tag::secondary()
                .small()
                .child(tag(Text::ResearchCompetition, scores.competition.value())),
        )
        .child(
            Tag::secondary()
                .small()
                .child(tag(Text::ResearchTrend, scores.trend.value())),
        )
}

impl Render for ThemesScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.channels.is_empty() {
            let bardo = self.bardo.read(cx);
            return h_flex()
                .size_full()
                .p_6()
                .items_start()
                .child(muted(cx, tr(bardo, Text::ThemesNoChannels)))
                .into_any_element();
        }
        h_flex()
            .size_full()
            .items_start()
            .child(self.render_controls(cx))
            .child(self.render_ideas(cx))
            .into_any_element()
    }
}
