//! Pieces that show a post's numbers (#29): its figures, its views over
//! time as bars, its syncs as a list, and where syncing stands. With the
//! channel's account connected (#79) the owner's numbers join them: engaged
//! views lead, then watch time, average view, money (or "not monetized")
//! and the retention curve, with how late YouTube Analytics runs. The
//! Publish stage and the Performance screen share them; what they show
//! comes from `bardo_app`.

use bardo_app::bardo_domain::{JobState, MetricsSnapshot, OwnerMetrics, PostRetention};
use bardo_app::{Bardo, MetricsStatus, PublishedPost, Text, UploadState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Div, ElementId, SharedString, Stateful, div, px};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::shell::tr;

/// The most bars a chart draws: the latest syncs.
const MAX_BARS: usize = 30;

/// The most syncs a history list shows.
const MAX_ROWS: usize = 8;

/// The most bars a retention curve draws, of the report's hundred.
const RETENTION_BARS: usize = 50;

/// A count in a few characters: `12.3K`.
pub fn count(bardo: &Bardo, n: u64) -> SharedString {
    SharedString::from(bardo.compact_count(n))
}

/// A change, signed: `+1.2K`, `−300`, `0`.
pub fn change(bardo: &Bardo, change: i64) -> String {
    let sign = match change {
        ..0 => "\u{2212}",
        0 => "",
        _ => "+",
    };
    format!("{sign}{}", bardo.compact_count(change.unsigned_abs()))
}

/// `values` (oldest first) as bars up to `bar` wide, the latest in the
/// accent. Bars rise from the lowest value so growth shows even on large
/// totals.
pub fn bars(
    id: impl Into<ElementId>,
    values: &[u64],
    height: f32,
    bar: f32,
    cx: &App,
) -> Stateful<Div> {
    let t = look(cx).tokens;
    let shown = &values[values.len().saturating_sub(MAX_BARS)..];
    let low = shown.iter().copied().min().unwrap_or(0);
    let high = shown.iter().copied().max().unwrap_or(0);
    let last = shown.len().saturating_sub(1);
    h_flex()
        .id(id)
        .h(px(height))
        .w_full()
        .items_end()
        .gap(px(2.))
        .children(shown.iter().enumerate().map(|(ix, value)| {
            // A fifth of the height for the lowest, the rest by growth.
            let share = if high == low {
                0.6
            } else {
                0.2 + 0.8 * (value - low) as f32 / (high - low) as f32
            };
            div()
                .flex_1()
                .max_w(px(bar))
                .h(px((height * share).max(2.)))
                .rounded_t(t.radius.min(px(2.)))
                .bg(if ix == last { t.accent } else { t.accent_edge })
        }))
}

/// A figure in a row of them: its label over its value.
fn stat(label: SharedString, value: AnyElement, cx: &App) -> Div {
    stat_with(label, None, value, cx)
}

/// A figure whose label has what it means behind an ⓘ.
fn stat_with(
    label: SharedString,
    hint: Option<(ElementId, SharedString)>,
    value: AnyElement,
    cx: &App,
) -> Div {
    let t = look(cx).tokens;
    v_flex()
        .gap_0p5()
        .min_w(px(72.))
        .child(
            h_flex()
                .gap_0p5()
                .items_center()
                .child(div().text_xs().text_color(t.text2).child(label))
                .children(hint.map(|(id, hint)| kit::info(id, None, hint))),
        )
        .child(value)
}

fn hint_id(id: &str, suffix: &str) -> ElementId {
    ElementId::Name(format!("{id}-{suffix}").into())
}

/// Values beside each other in a wrapping row.
fn stat_row(stats: Vec<Div>) -> Div {
    h_flex().gap_6().flex_wrap().items_start().children(stats)
}

fn figure_value(text: SharedString, cx: &App) -> AnyElement {
    div()
        .text_lg()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .font_family(
            gpui_kit::component::ActiveTheme::theme(cx)
                .mono_font_family
                .clone(),
        )
        .child(text)
        .into_any_element()
}

/// "Hidden", with why behind an ⓘ.
fn hidden(bardo: &Bardo, id: ElementId, cx: &App) -> AnyElement {
    h_flex()
        .gap_1()
        .items_center()
        .child(
            div()
                .text_sm()
                .text_color(look(cx).tokens.text2)
                .child(tr(bardo, Text::MetricHidden)),
        )
        .child(kit::info(id, None, tr(bardo, Text::MetricHiddenHint)))
        .into_any_element()
}

/// The post's latest views, likes and comments, the views' change since
/// the sync before, and its views over time; engaged views first and the
/// owner's numbers under them when there are some. Nothing before a sync
/// found it.
pub fn post_numbers(bardo: &Bardo, post: &PublishedPost, id: &str, cx: &App) -> Option<Div> {
    let latest = post.latest()?;
    let owned = post.latest_owner();
    let optional = |value: Option<u64>, suffix: &str| match value {
        Some(n) => figure_value(count(bardo, n), cx),
        None => hidden(bardo, hint_id(id, suffix), cx),
    };
    let mut stats = Vec::new();
    let owner = owned.and_then(|snapshot| snapshot.owner);
    // With engaged views first, the change line names the views it counts.
    let engaged = owner.is_some();
    if let Some(owner) = owner {
        stats.push(stat_with(
            tr(bardo, Text::MetricEngagedViews),
            Some((
                hint_id(id, "engaged"),
                tr(bardo, Text::MetricEngagedViewsHint),
            )),
            figure_value(count(bardo, owner.engaged_views), cx),
            cx,
        ));
    }
    stats.push(stat(
        tr(bardo, Text::MetricViews),
        figure_value(count(bardo, latest.views), cx),
        cx,
    ));
    stats.push(stat(
        tr(bardo, Text::MetricLikes),
        optional(latest.likes, "likes"),
        cx,
    ));
    stats.push(stat(
        tr(bardo, Text::MetricComments),
        optional(latest.comments, "comments"),
        cx,
    ));
    let change_line = post.views_change().map(|n| {
        div()
            .text_xs()
            .text_color(look(cx).tokens.text2)
            .child(SharedString::from(bardo.text_with(
                if engaged {
                    Text::MetricsViewsChange
                } else {
                    Text::MetricsChange
                },
                &[("change", &change(bardo, n))],
            )))
    });
    let views_over_time: Vec<u64> = post.history.iter().map(|s| s.views).collect();
    Some(
        v_flex()
            .gap_2()
            .child(stat_row(stats))
            .children(change_line)
            .when(views_over_time.len() > 1, |numbers| {
                numbers.child(bars(
                    ElementId::Name(format!("{id}-bars").into()),
                    &views_over_time,
                    40.,
                    18.,
                    cx,
                ))
            })
            .children(owned.map(|snapshot| owner_numbers(bardo, snapshot, id, cx))),
    )
}

/// The owner's numbers of a snapshot: watch time, the average view, money
/// or "not monetized", and when YouTube Analytics was read and how late
/// it runs.
fn owner_numbers(bardo: &Bardo, snapshot: &MetricsSnapshot, id: &str, cx: &App) -> Div {
    let t = look(cx).tokens;
    let Some(owner) = snapshot.owner else {
        return div();
    };
    let watch = stat_row(vec![
        stat(
            tr(bardo, Text::MetricWatchTime),
            figure_value(bardo.watch_time(owner.minutes_watched).into(), cx),
            cx,
        ),
        stat(
            tr(bardo, Text::MetricAverageView),
            figure_value(bardo.clock(owner.average_view_seconds).into(), cx),
            cx,
        ),
        stat(
            tr(bardo, Text::MetricAverageViewed),
            figure_value(bardo.percent(owner.average_view_share).into(), cx),
            cx,
        ),
    ]);
    v_flex()
        .gap_2()
        .pt_2()
        .border_t(t.border_width)
        .border_color(t.border)
        .child(watch)
        .child(money(bardo, &owner, id, cx))
        .child(
            div()
                .text_xs()
                .text_color(t.text2)
                .child(SharedString::from(bardo.text_with(
                    Text::MetricsOwnerLine,
                    &[("when", &bardo.time_ago(snapshot.taken_at))],
                ))),
        )
}

/// Revenue, RPM, CPM and playback-based CPM; "not monetized" outside the
/// Partner Program.
fn money(bardo: &Bardo, owner: &OwnerMetrics, id: &str, cx: &App) -> Div {
    let Some(money) = owner.money() else {
        return stat_row(vec![stat_with(
            tr(bardo, Text::MetricRevenue),
            Some((
                hint_id(id, "unpaid"),
                tr(bardo, Text::MetricNotMonetizedHint),
            )),
            div()
                .text_sm()
                .text_color(look(cx).tokens.text2)
                .child(tr(bardo, Text::MetricNotMonetized))
                .into_any_element(),
            cx,
        )]);
    };
    let amount = |amount| figure_value(bardo.money(amount).into(), cx);
    let rpm = owner
        .rpm()
        .map_or_else(|| figure_value("—".into(), cx), amount);
    stat_row(vec![
        stat(tr(bardo, Text::MetricRevenue), amount(money.revenue), cx),
        stat_with(
            tr(bardo, Text::MetricRpm),
            Some((hint_id(id, "rpm"), tr(bardo, Text::MetricRpmHint))),
            rpm,
            cx,
        ),
        stat_with(
            tr(bardo, Text::MetricCpm),
            Some((hint_id(id, "cpm"), tr(bardo, Text::MetricCpmHint))),
            amount(money.cpm),
            cx,
        ),
        stat_with(
            tr(bardo, Text::MetricPlaybackCpm),
            Some((
                hint_id(id, "playback-cpm"),
                tr(bardo, Text::MetricPlaybackCpmHint),
            )),
            amount(money.playback_cpm),
            cx,
        ),
    ])
}

/// The post's retention curve as bars from zero, the tallest point at the
/// top, with the share still watching at the end.
pub fn retention(bardo: &Bardo, retention: &PostRetention, id: &str, cx: &App) -> Option<Div> {
    let points = retention.curve.thinned(RETENTION_BARS);
    if points.is_empty() {
        return None;
    }
    let t = look(cx).tokens;
    let high = points
        .iter()
        .map(|point| point.watch.ten_thousandths())
        .max()
        .unwrap_or(0)
        .max(1);
    let height = 56.;
    let chart = h_flex()
        .id(hint_id(id, "retention-bars"))
        .h(px(height))
        .w_full()
        .items_end()
        .gap(px(1.))
        .children(points.iter().map(|point| {
            let share = point.watch.ten_thousandths() as f32 / high as f32;
            div()
                .flex_1()
                .h(px((height * share).max(1.)))
                .rounded_t(t.radius.min(px(1.)))
                .bg(t.accent_edge)
        }));
    let end = retention.curve.at_end().map(|share| {
        bardo.text_with(
            Text::MetricsRetentionEnd,
            &[("percent", &bardo.percent(share))],
        )
    });
    Some(
        v_flex()
            .gap_1()
            .max_w(px(420.))
            .child(
                h_flex()
                    .gap_0p5()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(tr(bardo, Text::MetricsRetention)),
                    )
                    .child(kit::info(
                        hint_id(id, "retention"),
                        None,
                        tr(bardo, Text::MetricsRetentionHint),
                    )),
            )
            .child(chart)
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(t.text2)
                    .child(tr(bardo, Text::MetricsRetentionStart))
                    .children(end.map(SharedString::from)),
            ),
    )
}

/// The post's syncs, newest first: when, views, and the change.
pub fn history(bardo: &Bardo, post: &PublishedPost, cx: &App) -> Option<Div> {
    if post.history.len() < 2 {
        return None;
    }
    let t = look(cx).tokens;
    let rows = post
        .history
        .iter()
        .enumerate()
        .rev()
        .take(MAX_ROWS)
        .map(|(ix, snapshot)| {
            let delta = ix
                .checked_sub(1)
                .map(|before| post.history[before].views_to(snapshot));
            h_flex()
                .gap_3()
                .text_xs()
                .child(
                    div()
                        .flex_1()
                        .text_color(t.text2)
                        .child(SharedString::from(bardo.time_ago(snapshot.taken_at))),
                )
                .child(
                    div()
                        .w(px(80.))
                        .text_right()
                        .whitespace_nowrap()
                        .child(count(bardo, snapshot.views)),
                )
                .child(
                    div()
                        .w(px(80.))
                        .text_right()
                        .whitespace_nowrap()
                        .text_color(t.text2)
                        .child(SharedString::from(
                            delta.map_or_else(String::new, |n| change(bardo, n)),
                        )),
                )
        });
    Some(
        v_flex()
            .gap_1()
            .max_w(px(420.))
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(tr(bardo, Text::MetricsHistory)),
            )
            .children(rows),
    )
}

/// Where syncing stands, in a line: its progress while it runs, else
/// when the numbers were read.
pub fn sync_state(bardo: &Bardo, status: &MetricsStatus, id: &str, cx: &App) -> Div {
    let t = look(cx).tokens;
    if let Some(job) = status.job.as_ref().filter(|_| status.is_syncing()) {
        return h_flex()
            .gap_2()
            .items_center()
            .child(div().text_sm().child(tr(bardo, Text::MetricsSyncing)))
            .child(
                div().w(px(120.)).child(
                    Progress::new(ElementId::Name(format!("{id}-progress").into()))
                        .small()
                        .value(job.progress().permille() as f32 / 10.0),
                ),
            );
    }
    let line = match status.last_checked {
        Some(at) => bardo.text_with(Text::MetricsSyncedAgo, &[("when", &bardo.time_ago(at))]),
        None => bardo.text(Text::MetricsNotSynced).into_owned(),
    };
    div()
        .text_sm()
        .text_color(t.text2)
        .child(SharedString::from(line))
}

/// What keeps metrics from syncing: no key, or the last sync stopped
/// (with its reason behind "Details").
pub fn sync_notices(bardo: &Bardo, status: &MetricsStatus, id: &str, cx: &App) -> Vec<AnyElement> {
    let mut notices = Vec::new();
    if status.tracked == 0 {
        return notices;
    }
    if !status.key_saved {
        notices.push(
            kit::notice(Tone::Warning, tr(bardo, Text::MetricsMissingKey), cx).into_any_element(),
        );
    }
    if let Some(job) = status
        .job
        .as_ref()
        .filter(|job| job.state() == JobState::Failed)
    {
        let detail = job
            .failure()
            .map(|failure| SharedString::from(failure.detail.clone()));
        notices.push(
            h_flex()
                .gap_2()
                .items_center()
                .flex_wrap()
                .child(kit::notice(
                    Tone::Danger,
                    tr(bardo, Text::MetricsSyncStopped),
                    cx,
                ))
                .children(job.failure().map(|failure| {
                    div()
                        .text_xs()
                        .child(tr(bardo, Text::JobFailureKindName(failure.kind)))
                }))
                .children(detail.map(|detail| {
                    kit::details(
                        ElementId::Name(format!("{id}-failure").into()),
                        tr(bardo, Text::Details),
                        vec![detail],
                    )
                }))
                .into_any_element(),
        );
    }
    notices
}

/// The post's state (posted, or not found on the last sync) and when it
/// went up.
pub fn post_state(bardo: &Bardo, post: &PublishedPost, id: &str, cx: &App) -> Div {
    let publication = &post.publication;
    if let Some(state) = bardo.upload_state(publication) {
        return upload_state(bardo, post, &state, id, cx);
    }
    let chip = if publication.missing_since.is_some() {
        kit::status(Tone::Warning, tr(bardo, Text::PublicationMissing), cx)
    } else {
        kit::status(Tone::Success, tr(bardo, Text::PublicationPosted), cx)
    };
    h_flex()
        .gap_2()
        .items_center()
        .flex_wrap()
        .child(chip)
        .when(publication.missing_since.is_some(), |row| {
            row.child(kit::info(
                ElementId::Name(format!("{id}-missing").into()),
                None,
                tr(bardo, Text::PublicationMissingHint),
            ))
        })
        .child(when_line(
            bardo,
            Text::PublicationPostedAt,
            publication.posted_at,
            cx,
        ))
}

fn when_line(bardo: &Bardo, text: Text, at: std::time::SystemTime, cx: &App) -> Div {
    div()
        .text_sm()
        .text_color(look(cx).tokens.text2)
        .child(SharedString::from(
            bardo.text_with(text, &[("when", &bardo.time_ago(at))]),
        ))
}

/// An upload's state in a few words.
pub fn upload_label(bardo: &Bardo, state: &UploadState) -> SharedString {
    match state {
        UploadState::Waiting => tr(bardo, Text::UploadStateWaiting),
        UploadState::Uploading(progress) => bardo
            .text_with(
                Text::UploadStateUploading,
                &[("percent", &format!("{}%", progress.permille() / 10))],
            )
            .into(),
        UploadState::Retrying => tr(bardo, Text::UploadStateRetrying),
        UploadState::Processing => tr(bardo, Text::UploadStateProcessing),
        UploadState::StillProcessing => tr(bardo, Text::UploadStateStillProcessing),
        UploadState::Scheduled(_) => tr(bardo, Text::UploadStateScheduled),
        UploadState::Published => tr(bardo, Text::UploadStatePublished),
        UploadState::Restricted => tr(bardo, Text::UploadStateRestricted),
        UploadState::Stopped => tr(bardo, Text::UploadStateStopped),
        UploadState::Failed { .. } => tr(bardo, Text::UploadStateFailed),
    }
}

/// Where an uploaded post stands: its chip, a hint where it waits on the
/// network, and why it failed.
fn upload_state(
    bardo: &Bardo,
    post: &PublishedPost,
    state: &UploadState,
    id: &str,
    cx: &App,
) -> Div {
    let publication = &post.publication;
    let network = bardo
        .text(Text::NetworkName(publication.network()))
        .into_owned();
    let with_network = |text: Text| bardo.text_with(text, &[("network", &network)]);
    let missing = publication.missing_since.is_some();
    let (tone, hint): (Tone, Option<SharedString>) = match state {
        UploadState::Waiting | UploadState::Uploading(_) => (Tone::Info, None),
        UploadState::Retrying => (
            Tone::Warning,
            Some(with_network(Text::UploadRetryingHint).into()),
        ),
        UploadState::Processing => (
            Tone::Info,
            Some(with_network(Text::UploadProcessingHint).into()),
        ),
        UploadState::StillProcessing => (
            Tone::Warning,
            Some(with_network(Text::UploadStillProcessingHint).into()),
        ),
        UploadState::Scheduled(_) => (
            Tone::Info,
            Some(with_network(Text::UploadScheduledHint).into()),
        ),
        UploadState::Published if missing => {
            (Tone::Warning, Some(tr(bardo, Text::PublicationMissingHint)))
        }
        UploadState::Published => (Tone::Success, None),
        UploadState::Restricted => (Tone::Warning, None),
        UploadState::Stopped => (
            Tone::Neutral,
            Some(with_network(Text::UploadStoppedHint).into()),
        ),
        UploadState::Failed { .. } => (Tone::Danger, None),
    };
    let label = if missing && matches!(state, UploadState::Published) {
        tr(bardo, Text::PublicationMissing)
    } else {
        upload_label(bardo, state)
    };
    let chip = kit::status(tone, label, cx);
    let done = matches!(state, UploadState::Published | UploadState::Restricted);
    let row = h_flex()
        .gap_2()
        .items_center()
        .flex_wrap()
        .child(chip)
        .children(
            hint.map(|hint| kit::info(ElementId::Name(format!("{id}-upload").into()), None, hint)),
        )
        .when(done, |row| {
            row.child(when_line(
                bardo,
                Text::UploadSentAt,
                publication.posted_at,
                cx,
            ))
        });
    let row = match state {
        UploadState::Scheduled(at) => row.child(
            div()
                .text_sm()
                .text_color(look(cx).tokens.text2)
                .child(SharedString::from(bardo.text_with(
                    Text::UploadScheduledAt,
                    &[("when", &bardo.publish_time_text(*at))],
                ))),
        ),
        _ => row,
    };
    let mut column = v_flex().gap_2().child(row);
    if let UploadState::Uploading(progress) = state {
        column = column.child(
            Progress::new(ElementId::Name(format!("{id}-upload-progress").into()))
                .small()
                .value(progress.permille() as f32 / 10.0),
        );
    }
    if matches!(state, UploadState::Restricted) {
        // A scheduled one was kept private at its publish time; before it,
        // YouTube kept it private while processing it.
        let now = std::time::SystemTime::now();
        let scheduled = publication
            .upload()
            .and_then(|upload| upload.publish_at)
            .is_some_and(|at| at <= now);
        let hint = if scheduled {
            Text::UploadRestrictedScheduledHint
        } else {
            Text::UploadRestrictedHint
        };
        column = column.child(kit::notice(Tone::Warning, tr(bardo, hint), cx));
    }
    if let UploadState::Failed { failure, .. } = state {
        column = column.child(kit::notice(
            Tone::Danger,
            bardo.upload_failure_text(failure, publication.network()),
            cx,
        ));
    }
    column
}

/// The post's numbers on YouTube; on another network, that only the link
/// is kept.
pub fn post_metrics(bardo: &Bardo, post: &PublishedPost, id: &str, cx: &App) -> Vec<AnyElement> {
    let network = post.publication.network();
    if post.publication.upload().is_some() && !post.publication.has_public_metrics() {
        return Vec::new();
    }
    if !post.publication.has_public_metrics() {
        return vec![
            div()
                .text_sm()
                .text_color(look(cx).tokens.text2)
                .child(SharedString::from(bardo.text_with(
                    Text::PublicationNoMetrics,
                    &[("network", &bardo.text(Text::NetworkName(network)))],
                )))
                .into_any_element(),
        ];
    }
    let mut shown: Vec<AnyElement> = post_numbers(bardo, post, id, cx)
        .into_iter()
        .map(IntoElement::into_any_element)
        .collect();
    shown.extend(
        post.retention
            .as_ref()
            .and_then(|curve| retention(bardo, curve, id, cx))
            .map(IntoElement::into_any_element),
    );
    shown
}
