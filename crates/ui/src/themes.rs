//! Themes screen: pick a channel and niche, have ideas proposed and ranked,
//! then edit, discard or approve each one; approving starts a video
//! project. Proposing and ranking run as jobs in `bardo_app`; this view
//! polls the job revision and re-reads the ideas when it moves.

use std::time::Duration;

use bardo_app::bardo_domain::{
    Channel, ChannelId, JobKind, JobState, NicheScores, PerformanceEvidence, Reason, Theme,
    ThemeFieldError, ThemeId, ThemeStatus,
};
use bardo_app::{
    Bardo, BudgetConsent, Destination, SUGGESTIONS_PER_RUN, SpendEstimate, Text, ThemeError,
    ThemesView,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, SharedString, Subscription, Task, Window, div,
};

use crate::kit::{self, Tone};
use crate::layout;
use crate::parts::{Collection, CollectionKind, Header, Inspector, ScreenParts};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

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
    /// A run held back at a budget: whether it only ranks, and what it
    /// would cost.
    budget_ask: Option<(bool, SpendEstimate)>,
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
            budget_ask: None,
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
        self.budget_ask = None;
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

    fn suggest(&mut self, rank_only: bool, consent: BudgetConsent, cx: &mut Context<Self>) {
        self.budget_ask = None;
        let (Some(id), Some(niche)) = (self.selected, self.niche.clone()) else {
            self.error = Some(Text::ThemesPickNiche);
            cx.notify();
            return;
        };
        let bardo = self.bardo.read(cx);
        let result = if rank_only {
            bardo.rank_themes(id, &niche, consent)
        } else {
            bardo.suggest_themes(id, &niche, consent)
        };
        self.error = match result {
            Ok(_) => None,
            Err(ThemeError::OverBudget(estimate)) => {
                self.budget_ask = Some((rank_only, estimate));
                None
            }
            Err(error) => Some(error.message()),
        };
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
        let view = self.view.as_ref();
        let has_niche = view.is_some_and(|view| view.niche.is_some());
        let running = self.running();
        let unranked = view.map_or(0, |view| view.unranked);

        v_flex()
            .gap_3()
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
            .when(has_niche, |panel| {
                panel.child(past_performance(
                    bardo,
                    cx,
                    view.and_then(|view| view.past.as_ref()),
                ))
            })
            // Nothing to suggest for without a niche; a running job shows
            // its progress instead of the buttons.
            .when(has_niche && !running, |panel| {
                panel.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new("suggest-themes")
                                .primary()
                                .label(tr(bardo, Text::SuggestThemes))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.suggest(false, BudgetConsent::Ask, cx)
                                })),
                        )
                        .when(unranked > 0, |row| {
                            row.child(
                                Button::new("rank-themes")
                                    .outline()
                                    .label(tr(bardo, Text::RankThemes))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.suggest(true, BudgetConsent::Ask, cx)
                                    })),
                            )
                        })
                        .child(kit::info(
                            "suggest-themes-info",
                            None,
                            SharedString::from(bardo.text_with(
                                Text::SuggestThemesHint,
                                &[("n", &SUGGESTIONS_PER_RUN.to_string())],
                            )),
                        )),
                )
            })
            .children(view.filter(|_| has_niche && !running).and_then(|view| {
                estimate_note(bardo, &view.suggest_estimate, Text::EstimateCost, cx)
            }))
            .when(unranked > 0, |panel| {
                panel.child(kit::notice(
                    Tone::Warning,
                    bardo.text_with(Text::ThemesUnranked, &[("n", &unranked.to_string())]),
                    cx,
                ))
            })
            .children(
                view.and_then(|view| view.rank_estimate.as_ref())
                    .filter(|_| unranked > 0 && !running)
                    .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx)),
            )
            .children(self.budget_ask.as_ref().map(|(rank_only, estimate)| {
                let rank_only = *rank_only;
                budget_question(
                    "themes-budget",
                    bardo,
                    estimate,
                    cx,
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.suggest(rank_only, BudgetConsent::Confirmed, cx)
                    }),
                    cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.budget_ask = None;
                        cx.notify();
                    }),
                )
            }))
            .children(
                self.error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
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
                        .child(kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::ThemesStopped),
                            cx,
                        ))
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .child(tr(bardo, Text::JobFailureKindName(failure.kind))),
                                )
                                .child(kit::details(
                                    "themes-failure-details",
                                    tr(bardo, Text::Details),
                                    vec![SharedString::from(failure.detail.clone())],
                                )),
                        )
                        .into_any_element(),
                )
            }
            JobState::Cancelled | JobState::Done => None,
        }
    }

    /// The channel's ideas, ranked, as a feed of cards.
    fn collection(&self, cx: &mut Context<Self>) -> Collection {
        let themes: Vec<Theme> = self
            .view
            .iter()
            .flat_map(|view| view.themes.iter().cloned())
            .collect();
        let cards: Vec<AnyElement> = themes
            .iter()
            .enumerate()
            .map(|(ix, idea)| self.render_theme(ix, idea, cx))
            .collect();
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let discarded = self.view.as_ref().map_or(0, |view| view.discarded);
        let mut collection = Collection::new(CollectionKind::Feed, "themes-ideas");
        collection.controls =
            vec![
                kit::section_heading(tr(bardo, Text::ThemesListTitle))
                    .child(kit::info(
                        "themes-ranking-info",
                        None,
                        tr(bardo, Text::ThemesRankingHint),
                    ))
                    .when(discarded > 0, |row| {
                        row.child(div().text_xs().text_color(theme.muted_foreground).child(
                            SharedString::from(bardo.text_with(
                                Text::ThemesDiscarded,
                                &[("n", &discarded.to_string())],
                            )),
                        ))
                    })
                    .into_any_element(),
            ];
        collection.empty = Some(muted(cx, tr(bardo, Text::ThemesEmpty)));
        collection.cards = cards;
        collection
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
                tags.child(kit::status(
                    Tone::Success,
                    tr(bardo, Text::ThemeApproved),
                    cx,
                ))
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
                .children(ranking.performance.map(|performance| {
                    reason(bardo, cx, Text::ThemePerformance, performance.reason)
                }))
                .child(
                    div().self_end().child(kit::details(
                        ("theme-ranking-details", ix),
                        tr(bardo, Text::Details),
                        // The numbers past performance was judged on, as they
                        // were then, and who ranked it when.
                        ranking
                            .performance
                            .map(|performance| {
                                SharedString::from(
                                    bardo.performance_evidence(&performance.evidence),
                                )
                            })
                            .into_iter()
                            .chain([SharedString::from(format!(
                                "{} · {}",
                                bardo.text_with(Text::ThemeRankedBy, &[("model", &ranking.model)]),
                                bardo.time_ago(ranking.ranked_at)
                            ))])
                            .collect(),
                    )),
                )
                .into_any_element(),
            None => h_flex()
                .child(kit::status(
                    Tone::Warning,
                    tr(bardo, Text::ThemeNotRanked),
                    cx,
                ))
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

        kit::card(cx)
            .id(("theme", ix))
            .p_3()
            .gap_2()
            .child(header)
            .child(reasons)
            .child(body)
            .into_any_element()
    }

    fn render_editor(&self, editing: &Editing, cx: &Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let errors = |fields: &[ThemeFieldError]| -> Vec<AnyElement> {
            editing
                .errors
                .iter()
                .filter(|error| fields.contains(error))
                .map(|error| {
                    kit::notice(Tone::Danger, tr(bardo, Text::ThemeFieldError(*error)), cx)
                        .text_xs()
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
            .pt_3()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .child(kit::section_heading(tr(bardo, Text::ProjectsTitle)))
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

/// What the channel's published videos say about the niche, or when they
/// will start to.
fn past_performance(
    bardo: &Bardo,
    cx: &App,
    evidence: Option<&PerformanceEvidence>,
) -> impl IntoElement {
    let theme = cx.theme();
    let line = match evidence {
        Some(evidence) => div()
            .text_sm()
            .child(SharedString::from(bardo.performance_evidence(evidence))),
        None => div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(tr(bardo, Text::ThemesNoHistory)),
    };
    v_flex()
        .gap_0p5()
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(tr(bardo, Text::ThemePerformance)),
        )
        .child(line)
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
        let bardo = self.bardo.read(cx);
        let header = Header::place(bardo, Destination::Themes);
        let mut parts = ScreenParts::new(header);
        if self.channels.is_empty() {
            parts.content = vec![muted(cx, tr(bardo, Text::ThemesNoChannels))];
            return layout::screen(parts, cx);
        }
        parts.notices = self
            .started
            .as_ref()
            .map(|title| {
                kit::notice(
                    Tone::Success,
                    bardo.text_with(Text::ProjectStarted, &[("title", title)]),
                    cx,
                )
                .into_any_element()
            })
            .into_iter()
            .collect();
        parts.collection = Some(self.collection(cx));
        parts.inspector = Some(Inspector::new(vec![
            self.render_controls(cx).into_any_element(),
            self.render_projects(cx).into_any_element(),
        ]));
        layout::screen(parts, cx)
    }
}
