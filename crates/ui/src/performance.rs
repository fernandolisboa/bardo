//! Performance screen (#29): a channel's linked posts and how they
//! perform. The figures add up the latest public numbers of its YouTube
//! posts, led by engaged views, watch time and revenue when the channel's
//! account is connected (#79); the chart follows the channel's views over
//! the syncs, and the picked post shows its own numbers, retention and
//! history. Syncing runs as a job
//! in `bardo_app`; this view polls the job revision and re-reads when it
//! moves.

use std::rc::Rc;
use std::time::Duration;

use bardo_app::bardo_domain::{Channel, ChannelId, Job, PublicationId};
use bardo_app::{Bardo, ChannelMetricsView, ChannelPost, Destination, OwnerAccess, Text};
use gpui_kit::component::button::Button;
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, SharedString, Subscription, Task, Window, div, px,
};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::layout;
use crate::metrics;
use crate::parts::{Collection, CollectionKind, Figure, Header, Inspector, ScreenParts, Tile};
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

pub struct PerformanceScreen {
    bardo: Entity<Bardo>,
    channels: Vec<Channel>,
    channel_select: ChannelSelect,
    selected: Option<ChannelId>,
    view: Option<ChannelMetricsView>,
    /// The post open beside the list.
    picked: Option<PublicationId>,
    error: Option<Text>,
    revision: u64,
    /// The metrics sync as last read: the numbers change only with it.
    sync_job: Option<Job>,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl PerformanceScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let channel_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        let subscriptions = vec![cx.subscribe_in(
            &channel_select,
            window,
            |this, _, event: &SelectEvent<SearchableVec<ChannelChoice>>, _, cx| {
                let SelectEvent::Confirm(Some(id)) = event else {
                    return;
                };
                if this.selected != Some(*id) {
                    this.select(*id, cx);
                }
            },
        )];
        let revision = bardo.read(cx).jobs_revision();
        let mut screen = Self {
            bardo,
            channels: Vec::new(),
            channel_select,
            selected: None,
            view: None,
            picked: None,
            error: None,
            revision,
            sync_job: None,
            _poll: poll,
            _subscriptions: subscriptions,
        };
        screen.reload(window, cx);
        screen
    }

    /// Re-reads the channels and the shown one's posts (posts are linked
    /// at a project's Publish stage), keeping the selection when it still
    /// exists.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.channels = self.bardo.read(cx).channels().unwrap_or_default();
        let choices = SearchableVec::new(
            self.channels
                .iter()
                .map(|channel| ChannelChoice {
                    id: channel.id,
                    title: SharedString::from(channel.details.name().to_owned()),
                })
                .collect::<Vec<_>>(),
        );
        let target = self
            .selected
            .filter(|id| self.channels.iter().any(|c| c.id == *id))
            .or_else(|| self.channels.first().map(|c| c.id));
        self.channel_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            if let Some(id) = target {
                select.set_selected_value(&id, window, cx);
            }
        });
        self.selected = target;
        self.load(cx);
        cx.notify();
    }

    fn select(&mut self, id: ChannelId, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.picked = None;
        self.error = None;
        self.load(cx);
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            self.view = None;
            return;
        };
        match self.bardo.read(cx).channel_metrics(id) {
            Ok(view) => self.view = Some(view),
            Err(error) => {
                self.view = None;
                self.error = Some(error.message());
            }
        }
    }

    /// Numbers change while a sync runs; other jobs leave them alone.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let bardo = self.bardo.read(cx);
        let revision = bardo.jobs_revision();
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        let job = bardo.latest_sync_job();
        if job != self.sync_job {
            self.sync_job = job;
            self.load(cx);
            cx.notify();
        }
    }

    fn sync_now(&mut self, cx: &mut Context<Self>) {
        self.error = self
            .bardo
            .read(cx)
            .sync_metrics()
            .err()
            .map(|error| error.message());
        self.load(cx);
        cx.notify();
    }

    fn pick(&mut self, id: PublicationId, cx: &mut Context<Self>) {
        self.picked = Some(id);
        cx.notify();
    }

    /// The post open beside the list: the picked one, else the newest.
    fn shown<'a>(&self, view: &'a ChannelMetricsView) -> Option<&'a ChannelPost> {
        view.posts
            .iter()
            .find(|post| Some(post.post.publication.id) == self.picked)
            .or(view.posts.first())
    }

    /// The channel's totals: views (with their trend), likes, comments
    /// and the posts they come from.
    fn figures(&self, view: &ChannelMetricsView, cx: &App) -> Vec<Figure> {
        let bardo = self.bardo.read(cx);
        let totals = view.totals;
        let some = |n: Option<u64>| n.map_or_else(|| "—".into(), |n| metrics::count(bardo, n));
        let tracked = (totals.posts > 0).then_some(totals.views);
        let mut views = Figure::new(tr(bardo, Text::MetricViews), some(tracked));
        let trend: Vec<u64> = view
            .history
            .iter()
            .map(|point| point.totals.views)
            .collect();
        if trend.len() > 1 {
            views.line = Some(
                metrics::bars("performance-views-trend", &trend, 28., 18., cx).into_any_element(),
            );
        }
        let mut posts = Figure::new(
            tr(bardo, Text::PerformancePosts),
            view.posts.len().to_string(),
        );
        posts.beside = Some(SharedString::from(bardo.text_with(
            Text::PerformanceTracked,
            &[("n", &view.status.tracked.to_string())],
        )));
        // With the owner's numbers, they lead; likes and comments stay on
        // each post.
        if let Some(owner) = totals.owner {
            let mut figures = vec![
                Figure::new(
                    tr(bardo, Text::MetricEngagedViews),
                    metrics::count(bardo, owner.engaged_views),
                ),
                views,
                Figure::new(
                    tr(bardo, Text::MetricWatchTime),
                    bardo.watch_time(owner.minutes_watched),
                ),
            ];
            figures.extend(
                owner.revenue.map(|revenue| {
                    Figure::new(tr(bardo, Text::MetricRevenue), bardo.money(revenue))
                }),
            );
            figures.push(posts);
            return figures;
        }
        vec![
            views,
            Figure::new(tr(bardo, Text::MetricLikes), some(totals.likes)),
            Figure::new(tr(bardo, Text::MetricComments), some(totals.comments)),
            posts,
        ]
    }

    /// Why the owner's numbers are missing, while the channel has YouTube
    /// posts: its account is not connected, or needs to reconnect.
    fn owner_notice(&self, view: &ChannelMetricsView, cx: &App) -> Option<AnyElement> {
        if view.status.tracked == 0 {
            return None;
        }
        let bardo = self.bardo.read(cx);
        let (tone, text) = match view.owner_access {
            OwnerAccess::NotConnected => (Tone::Info, Text::PerformanceOwnerNotConnected),
            OwnerAccess::ReconnectNeeded => (Tone::Warning, Text::PerformanceOwnerReconnect),
            OwnerAccess::NoAccount | OwnerAccess::Connected => return None,
        };
        Some(kit::notice(tone, tr(bardo, text), cx).into_any_element())
    }

    /// Where syncing stands and "Sync now".
    fn toolbar(&self, view: &ChannelMetricsView, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let status = &view.status;
        h_flex()
            .gap_2()
            .items_center()
            .flex_wrap()
            .child(metrics::sync_state(bardo, status, "performance-sync", cx))
            .child(div().flex_1())
            .when(status.can_sync(), |row| {
                row.child(
                    Button::new("performance-sync-now")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::MetricsSyncNow))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.sync_now(cx))),
                )
            })
            .child(kit::info(
                "performance-sync-info",
                None,
                tr(bardo, Text::MetricsSyncHint),
            ))
            .into_any_element()
    }

    /// Every linked post, newest first: its project, network and numbers.
    fn collection(&self, view: &ChannelMetricsView, cx: &mut Context<Self>) -> Collection {
        let shown = self.shown(view).map(|post| post.post.publication.id);
        let mut tiles = Vec::new();
        for (ix, post) in view.posts.iter().enumerate() {
            let id = post.post.publication.id;
            let bardo = self.bardo.read(cx);
            let mut tile = Tile::new(
                ("performance-post", ix),
                Rc::new(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick(id, cx))),
            );
            let publication = &post.post.publication;
            tile.selected = shown == Some(id);
            tile.title = Some(SharedString::from(post.project_title.clone()));
            let network = bardo.text(Text::NetworkName(publication.network()));
            let engaged = post
                .post
                .latest_owner()
                .and_then(|snapshot| snapshot.owner)
                .map(|owner| owner.engaged_views);
            tile.text = Some(SharedString::from(match (engaged, post.post.latest()) {
                (Some(engaged), _) => bardo.text_with(
                    Text::PerformanceTileEngaged,
                    &[
                        ("network", &network),
                        ("views", &bardo.compact_count(engaged)),
                    ],
                ),
                (None, Some(latest)) => bardo.text_with(
                    Text::PerformanceTile,
                    &[
                        ("network", &network),
                        ("views", &bardo.compact_count(latest.views)),
                    ],
                ),
                (None, None) => network.into_owned(),
            }));
            tile.time = Some(SharedString::from(bardo.time_ago(publication.posted_at)));
            if publication.missing_since.is_some() {
                tile.status = Some(
                    kit::status(Tone::Warning, tr(bardo, Text::PublicationMissing), cx)
                        .into_any_element(),
                );
                tile.attention = true;
            }
            tiles.push(tile);
        }
        let bardo = self.bardo.read(cx);
        let mut collection = Collection::new(CollectionKind::List, "performance-posts");
        collection.controls = vec![
            div()
                .text_xs()
                .font_semibold()
                .text_color(look(cx).tokens.text2)
                .child(tr(bardo, Text::PerformancePosts))
                .into_any_element(),
        ];
        collection.tiles = tiles;
        collection.empty = Some(muted(cx, tr(bardo, Text::PerformanceEmpty)));
        collection
    }

    /// The picked post: its state, its link, its numbers and history.
    fn inspector(&self, post: &ChannelPost, cx: &mut Context<Self>) -> Inspector {
        let bardo = self.bardo.read(cx);
        let publication = &post.post.publication;
        let link = publication.link.as_ref().map(|link| {
            let url = link.url().to_owned();
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    kit::well(cx)
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .truncate()
                        .child(SharedString::from(url.clone())),
                )
                .child(
                    Button::new("performance-open-post")
                        .small()
                        .outline()
                        .label(tr(bardo, Text::PublicationOpen))
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
        });
        let mut body =
            vec![metrics::post_state(bardo, &post.post, "performance-post", cx).into_any_element()];
        body.extend(link.map(IntoElement::into_any_element));
        body.extend(metrics::post_metrics(
            bardo,
            &post.post,
            "performance-post",
            cx,
        ));
        body.extend(metrics::history(bardo, &post.post, cx).map(IntoElement::into_any_element));
        let mut inspector = Inspector::new(body);
        inspector.title = Some(
            div()
                .font_semibold()
                .child(SharedString::from(format!(
                    "{} · {}",
                    post.project_title,
                    bardo.text(Text::NetworkName(publication.network()))
                )))
                .into_any_element(),
        );
        inspector
    }

    /// The channel's views over the syncs, as bars.
    fn history_card(&self, view: &ChannelMetricsView, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let t = look(cx).tokens;
        let views: Vec<u64> = view
            .history
            .iter()
            .map(|point| point.totals.views)
            .collect();
        let body: AnyElement = if views.len() > 1 {
            let (first, last) = (view.history[0].at, view.history[views.len() - 1].at);
            v_flex()
                .gap_1()
                .child(metrics::bars("performance-history", &views, 96., 64., cx))
                .child(
                    h_flex()
                        .justify_between()
                        .text_xs()
                        .text_color(t.text2)
                        .child(SharedString::from(bardo.time_ago(first)))
                        .child(SharedString::from(bardo.time_ago(last))),
                )
                .into_any_element()
        } else {
            muted(cx, tr(bardo, Text::PerformanceHistoryEmpty))
        };
        kit::card(cx)
            .p_4()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(tr(bardo, Text::PerformanceHistoryTitle)),
            )
            .child(body)
            .into_any_element()
    }
}

fn muted(cx: &App, text: SharedString) -> AnyElement {
    div()
        .text_sm()
        .text_color(look(cx).tokens.text2)
        .child(text)
        .into_any_element()
}

impl Render for PerformanceScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let mut header = Header::place(bardo, Destination::Performance);
        header.info = Some(
            kit::info("performance-info", None, tr(bardo, Text::PerformanceInfo))
                .into_any_element(),
        );
        if self.channels.is_empty() {
            let mut parts = ScreenParts::new(header);
            parts.content = vec![muted(cx, tr(bardo, Text::PerformanceNoChannels))];
            return layout::screen(parts, cx);
        }
        header.trail.push(
            div()
                .w(px(200.))
                .child(Select::new(&self.channel_select).xsmall())
                .into_any_element(),
        );
        let mut parts = ScreenParts::new(header);
        parts.notices = self
            .error
            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element())
            .into_iter()
            .collect();
        let Some(view) = self.view.clone() else {
            return layout::screen(parts, cx);
        };
        parts.notices.extend(metrics::sync_notices(
            bardo,
            &view.status,
            "performance-sync",
            cx,
        ));
        parts.notices.extend(self.owner_notice(&view, cx));
        parts.summary = self.figures(&view, cx);
        parts.toolbar = Some(self.toolbar(&view, cx));
        parts.collection = Some(self.collection(&view, cx));
        parts.inspector = self.shown(&view).map(|post| self.inspector(post, cx));
        if !view.posts.is_empty() {
            parts.content = vec![self.history_card(&view, cx)];
        }
        layout::screen(parts, cx)
    }
}
