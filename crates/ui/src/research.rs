//! Research screen: pick a channel, type seed niches, run research and read
//! the ranked results with the numbers behind each score. Research runs as
//! a job in `bardo_app`; this view polls the job revision and re-reads the
//! results when it moves, so the UI thread never waits on YouTube.

use std::time::Duration;

use bardo_app::bardo_domain::{Channel, ChannelId, JobState, NicheScores, NicheSeedError, Score};
use bardo_app::{Bardo, Destination, NicheResearchView, NicheResult, NicheRow, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
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

/// How often the screen checks the job queue for changes.
const POLL_EVERY: Duration = Duration::from_millis(100);

/// A channel in the picker.
#[derive(Clone)]
struct ChannelChoice {
    id: ChannelId,
    title: SharedString,
}

impl SearchableListItem for ChannelChoice {
    type Value = ChannelId;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &ChannelId {
        &self.id
    }
}

type ChannelSelect = Entity<SelectState<SearchableVec<ChannelChoice>>>;

fn channel_choices(channels: &[Channel]) -> SearchableVec<ChannelChoice> {
    SearchableVec::new(
        channels
            .iter()
            .map(|channel| ChannelChoice {
                id: channel.id,
                title: SharedString::from(channel.details.name().to_owned()),
            })
            .collect::<Vec<_>>(),
    )
}

pub struct ResearchScreen {
    bardo: Entity<Bardo>,
    channels: Vec<Channel>,
    channel_select: ChannelSelect,
    selected: Option<ChannelId>,
    seeds: Entity<TextareaState>,
    view: Option<NicheResearchView>,
    /// Quota a run and a refresh would spend with the typed seeds; `None`
    /// while the seeds are invalid.
    cost: Option<(u32, u32)>,
    seed_errors: Vec<NicheSeedError>,
    error: Option<Text>,
    revision: u64,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl ResearchScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let channel_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let seeds = cx.new(|cx| TextareaState::new(window, cx).auto_grow(6, 14));
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
                |this, _, event: &SelectEvent<SearchableVec<ChannelChoice>>, window, cx| {
                    let SelectEvent::Confirm(Some(id)) = event else {
                        return;
                    };
                    if this.selected != Some(*id) {
                        this.select(*id, window, cx);
                    }
                },
            ),
            cx.subscribe(&seeds, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.seed_errors.clear();
                    this.error = None;
                    this.update_cost(cx);
                    cx.notify();
                }
            }),
            cx.observe_in(&bardo, window, |this, _, window, cx| {
                this.relabel(window, cx)
            }),
        ];
        let revision = bardo.read(cx).jobs_revision();
        let mut screen = Self {
            bardo,
            channels: Vec::new(),
            channel_select,
            selected: None,
            seeds,
            view: None,
            cost: None,
            seed_errors: Vec::new(),
            error: None,
            revision,
            _poll: poll,
            _subscriptions: subscriptions,
        };
        screen.relabel(window, cx);
        screen.reload_channels(window, cx);
        screen
    }

    fn relabel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = tr(self.bardo.read(cx), Text::ResearchSeedsPlaceholder);
        self.seeds.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
        cx.notify();
    }

    /// Re-reads the channels (they change on the channels screen) and keeps
    /// the selection when the channel still exists.
    pub fn reload_channels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A message from the last visit may no longer hold (e.g. a key was
        // saved meanwhile).
        self.error = None;
        self.seed_errors.clear();
        self.channels = self.bardo.read(cx).channels().unwrap_or_default();
        let choices = channel_choices(&self.channels);
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
        match (target, keep) {
            // A kept channel may have a new market: reload its results, but
            // keep what the user typed.
            (Some(_), Some(_)) => {
                self.load(cx);
                self.update_cost(cx);
            }
            (Some(id), None) => self.select(id, window, cx),
            (None, _) => {
                self.selected = None;
                self.view = None;
            }
        }
        cx.notify();
    }

    fn select(&mut self, id: ChannelId, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.seed_errors.clear();
        self.error = None;
        self.load(cx);
        let text = self
            .view
            .as_ref()
            .map(|view| {
                view.seeds
                    .iter()
                    .map(|niche| niche.label())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        self.seeds
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.update_cost(cx);
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        match self.bardo.read(cx).niche_research(id) {
            Ok(view) => self.view = Some(view),
            Err(_) => {
                self.view = None;
                self.error = Some(Text::ResearchNotLoaded);
            }
        }
    }

    fn typed_seeds(&self, cx: &App) -> Vec<String> {
        self.seeds
            .read(cx)
            .value()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn update_cost(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            self.cost = None;
            return;
        };
        let seeds = self.typed_seeds(cx);
        let bardo = self.bardo.read(cx);
        self.cost = bardo
            .research_cost(id, &seeds, false)
            .and_then(|run| Ok((run, bardo.research_cost(id, &seeds, true)?)))
            .ok();
    }

    /// Results and the job's progress change while research runs.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let revision = self.bardo.read(cx).jobs_revision();
        if revision != self.revision {
            self.revision = revision;
            self.load(cx);
            self.update_cost(cx);
            cx.notify();
        }
    }

    fn run(&mut self, refresh: bool, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        let seeds = self.typed_seeds(cx);
        match self.bardo.read(cx).run_niche_research(id, &seeds, refresh) {
            Ok(_) => {
                self.seed_errors.clear();
                self.error = None;
            }
            Err(error) => {
                self.seed_errors = error.seed_errors().to_vec();
                self.error = Some(error.message());
            }
        }
        self.load(cx);
        cx.notify();
    }

    fn render_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let market = self.view.as_ref().map(|view| {
            format!(
                "{} · {}",
                bardo.text(Text::CountryName(view.market.country)),
                bardo.text(Text::ContentLanguageName(view.market.language)),
            )
        });
        let running = self
            .view
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .is_some_and(|job| job.state().is_active());

        let cost = self.cost.map(|(run, _)| match run {
            0 => tr(bardo, Text::ResearchCostNone),
            units => SharedString::from(
                bardo.text_with(Text::ResearchCost, &[("units", &units.to_string())]),
            ),
        });
        let refresh_label = match self.cost {
            Some((_, units)) => SharedString::from(
                bardo.text_with(Text::RefreshResearchCost, &[("units", &units.to_string())]),
            ),
            None => tr(bardo, Text::RefreshResearch),
        };
        let messages: Vec<AnyElement> = self
            .seed_errors
            .iter()
            .map(|error| tr(bardo, Text::NicheSeedError(*error)))
            .chain(
                self.error
                    .filter(|_| self.seed_errors.is_empty())
                    .map(|error| tr(bardo, error)),
            )
            .map(|text| kit::notice(Tone::Danger, text, cx).into_any_element())
            .collect();

        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::ResearchChannel)),
                    )
                    .child(Select::new(&self.channel_select))
                    .children(market.map(|market| {
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(market))
                    })),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(tr(bardo, Text::ResearchSeeds)),
                            )
                            .child(kit::info(
                                "research-seeds-info",
                                None,
                                tr(bardo, Text::ResearchSeedsHint),
                            )),
                    )
                    .child(Textarea::new(&self.seeds))
                    .children(messages),
            )
            // A running job shows its progress instead of the buttons.
            .when(!running, |editor| {
                editor.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new("run-research")
                                .primary()
                                .label(tr(bardo, Text::RunResearch))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.run(false, cx)),
                                ),
                        )
                        .child(
                            Button::new("refresh-research")
                                .outline()
                                .label(refresh_label)
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.run(true, cx)),
                                ),
                        )
                        .children(cost.map(|cost| kit::info("research-cost-info", None, cost))),
                )
            })
            .children(self.render_job(cx))
    }

    /// Progress of a running research job, or why the last one stopped.
    fn render_job(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let job = self.view.as_ref()?.job.as_ref()?;
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        match job.state() {
            JobState::Queued | JobState::Running => {
                let percent = job.progress().percent();
                Some(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().child(tr(bardo, Text::ResearchRunning)))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div().flex_1().child(
                                        Progress::new("research-progress")
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
                            tr(bardo, Text::ResearchStopped),
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
                                    "research-failure-details",
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

    /// The niches, ranked, as a feed of result cards.
    fn collection(&self, cx: &App) -> Collection {
        let bardo = self.bardo.read(cx);
        let mut collection = Collection::new(CollectionKind::Feed, "research-results");
        collection.controls = vec![
            kit::section_heading(tr(bardo, Text::ResearchResultsTitle))
                .child(kit::info(
                    "research-scores-info",
                    None,
                    tr(bardo, Text::ResearchScoresHint),
                ))
                .into_any_element(),
        ];
        collection.cards = self
            .view
            .iter()
            .flat_map(|view| view.rows.iter())
            .enumerate()
            .map(|(ix, row)| self.render_row(ix, row, cx))
            .collect();
        collection
    }

    fn render_row(&self, ix: usize, row: &NicheRow, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let scores = row.result.as_ref().and_then(|result| result.scores);

        let header = h_flex()
            .gap_3()
            .justify_between()
            .items_start()
            .child(
                h_flex()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(format!("{}.", ix + 1))),
                    )
                    .child(
                        div()
                            .font_medium()
                            .child(SharedString::from(row.niche.label().to_owned())),
                    ),
            )
            .children(scores.map(|scores| score_tags(bardo, scores)));

        let body: AnyElement = match &row.result {
            None => muted(cx, tr(bardo, Text::ResearchNotFetched)),
            Some(result) => self.render_result(result, cx),
        };

        kit::card(cx)
            .id(("niche", ix))
            .p_3()
            .gap_2()
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn render_result(&self, result: &NicheResult, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let stats = &result.statistics;
        let count = |n: Option<u64>| n.map_or_else(|| "—".to_owned(), |n| bardo.compact_count(n));

        let mut footer = vec![
            SharedString::from(bardo.text_with(
                Text::ResearchSample,
                &[
                    ("videos", &stats.sample_size.to_string()),
                    ("channels", &stats.channels.to_string()),
                ],
            )),
            SharedString::from(format!(
                "{} {}",
                bardo.text(Text::ResearchFetched),
                bardo.time_ago(result.fetched_at)
            )),
        ];
        if stats.sample_size == 0 {
            footer[0] = tr(bardo, Text::ResearchNoUploads);
        }

        let figures = [
            (
                Text::ResearchUploads,
                format!("≈{}", bardo.compact_count(stats.upload_volume)),
            ),
            (Text::ResearchMedianViews, count(stats.median_views)),
            (Text::ResearchViewsPerDay, count(stats.median_views_per_day)),
            (
                Text::ResearchMedianSubscribers,
                count(stats.median_subscribers),
            ),
            (
                Text::ResearchSmallChannels,
                stats
                    .small_channel_percent
                    .map_or_else(|| "—".to_owned(), |percent| format!("{percent}%")),
            ),
        ];

        v_flex()
            .gap_2()
            .when(stats.sample_size > 0, |body| {
                body.child(
                    h_flex()
                        .flex_wrap()
                        .gap_x_6()
                        .gap_y_2()
                        .children(figures.map(|(label, value)| {
                            v_flex()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(tr(bardo, label)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_medium()
                                        .child(SharedString::from(value)),
                                )
                        })),
                )
            })
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .children(footer.into_iter().map(|text| div().child(text)))
                    .when(!result.fresh, |line| {
                        line.child(kit::status(
                            Tone::Warning,
                            tr(bardo, Text::ResearchStale),
                            cx,
                        ))
                    }),
            )
            .into_any_element()
    }
}

fn muted(cx: &App, text: SharedString) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

/// Opportunity (the ranking key) stands out; competition and trend sit
/// beside it.
fn score_tags(bardo: &Bardo, scores: NicheScores) -> impl IntoElement {
    let tag = |label: Text, score: Score| {
        SharedString::from(format!("{} {}", bardo.text(label), score.value()))
    };
    h_flex()
        .gap_1()
        .flex_none()
        .child(
            Tag::primary()
                .small()
                .child(tag(Text::ResearchOpportunity, scores.opportunity())),
        )
        .child(
            Tag::secondary()
                .small()
                .child(tag(Text::ResearchCompetition, scores.competition)),
        )
        .child(
            Tag::secondary()
                .small()
                .child(tag(Text::ResearchTrend, scores.trend)),
        )
}

impl Render for ResearchScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let header = Header::place(bardo, Destination::Research);
        if self.channels.is_empty() {
            let mut parts = ScreenParts::new(header);
            parts.content = vec![muted(cx, tr(bardo, Text::ResearchNoChannels))];
            return layout::screen(parts, cx);
        }
        let mut parts = ScreenParts::new(header);
        parts.collection = Some(self.collection(cx));
        parts.inspector = Some(Inspector::new(vec![
            self.render_editor(cx).into_any_element(),
        ]));
        layout::screen(parts, cx)
    }
}
