//! YouTube owner metrics against recorded responses (see
//! `fixtures/youtube-analytics/README.md`). No test calls Google.

mod common;

use std::time::{Duration, SystemTime};

use bardo_domain::{
    AnalyticsErrorKind, Earnings, Monetization, Money, OwnerAnalytics, ReportPeriod, SecretText,
    Share, read_owner_metrics,
};
use bardo_publish::YouTubeAnalytics;
use common::{Scripted, fixture};

const VIDEO: &str = "dQw4w9WgXcQ";

fn analytics(names: &[&str]) -> YouTubeAnalytics<Scripted> {
    YouTubeAnalytics::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("youtube-analytics", name))
            .collect(),
    ))
}

fn token() -> SecretText {
    SecretText::new("ya29.fixture-access")
}

/// From 2026-09-30 (the day before the post) to 2026-10-03.
fn period() -> ReportPeriod {
    let day = |n: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(n * 86_400);
    // 2026-10-01 is day 20727 since the epoch.
    ReportPeriod::for_post(day(20_727) + Duration::from_secs(3600), day(20_729))
}

fn param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

#[test]
fn a_monetized_videos_report_reads_its_numbers_and_money() {
    let yt = analytics(&["video-monetized"]);
    let report = yt
        .video_report(&token(), VIDEO, &period(), true)
        .unwrap()
        .unwrap();
    assert_eq!(report.views, 48_210);
    assert_eq!(report.engaged_views, 31_544);
    assert_eq!(report.minutes_watched, 18_021);
    assert_eq!(report.average_view_seconds, 34);
    assert_eq!(report.average_view_share, Share::of_percent(71.62));
    let money = report.money.unwrap();
    assert_eq!(money.revenue, Money::from_micros(61_873_000));
    assert_eq!(money.cpm, Money::from_micros(7_412_000));
    assert_eq!(money.playback_cpm, Money::from_micros(5_880_000));

    let sent = yt.transport().sent();
    assert_eq!(sent.len(), 1);
    let url = &sent[0].url;
    assert!(
        url.starts_with("https://youtubeanalytics.googleapis.com/v2/reports?"),
        "{url}"
    );
    assert_eq!(param(url, "ids").as_deref(), Some("channel==MINE"));
    assert_eq!(param(url, "filters").as_deref(), Some("video==dQw4w9WgXcQ"));
    assert_eq!(param(url, "startDate").as_deref(), Some("2026-09-30"));
    assert_eq!(param(url, "endDate").as_deref(), Some("2026-10-03"));
    assert_eq!(
        param(url, "metrics").as_deref(),
        Some(
            "views,engagedViews,estimatedMinutesWatched,averageViewDuration,\
             averageViewPercentage,estimatedRevenue,cpm,playbackBasedCpm"
        )
    );
    assert_eq!(param(url, "currency").as_deref(), Some("USD"));
    assert_eq!(param(url, "dimensions"), None);
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer ya29.fixture-access")
    );
}

#[test]
fn a_report_without_money_asks_for_none() {
    let yt = analytics(&["video-numbers"]);
    let report = yt
        .video_report(&token(), VIDEO, &period(), false)
        .unwrap()
        .unwrap();
    assert_eq!(report.views, 12_045);
    assert_eq!(report.money, None);
    let url = &yt.transport().sent()[0].url;
    assert!(!param(url, "metrics").unwrap().contains("estimatedRevenue"));
    assert_eq!(param(url, "currency"), None);
}

#[test]
fn a_report_without_rows_has_no_data_yet() {
    let yt = analytics(&["video-empty"]);
    assert_eq!(
        yt.video_report(&token(), VIDEO, &period(), true).unwrap(),
        None
    );
}

#[test]
fn a_channel_outside_the_partner_program_reads_not_monetized() {
    // The money report answers 403; the same report without money follows.
    let yt = analytics(&["money-forbidden", "video-numbers", "retention"]);
    let mut monetization = Monetization::Unknown;
    let reading = read_owner_metrics(&yt, &token(), VIDEO, &period(), &mut monetization).unwrap();
    let metrics = reading.metrics.unwrap();
    assert_eq!(metrics.earnings, Earnings::NotMonetized);
    assert_eq!(metrics.engaged_views, 8_710);
    assert_eq!(metrics.rpm(), None);
    assert_eq!(monetization, Monetization::NotMonetized);
    assert_eq!(reading.retention.points().len(), 100);
    assert_eq!(yt.transport().sent().len(), 3);
}

#[test]
fn a_monetized_reading_computes_rpm() {
    let yt = analytics(&["video-monetized", "retention"]);
    let mut monetization = Monetization::Unknown;
    let metrics = read_owner_metrics(&yt, &token(), VIDEO, &period(), &mut monetization)
        .unwrap()
        .metrics
        .unwrap();
    // $61.873 over 48,210 views.
    assert_eq!(metrics.rpm(), Some(Money::from_micros(1_283_405)));
    assert_eq!(monetization, Monetization::Monetized);
}

#[test]
fn the_retention_report_reads_a_hundred_points_in_order() {
    let yt = analytics(&["retention"]);
    let points = yt.retention(&token(), VIDEO, &period()).unwrap();
    assert_eq!(points.len(), 100);
    assert_eq!(points[0].elapsed, Share::of_ratio(0.01));
    assert_eq!(
        points[0].watch,
        Share::of_ratio(1.18),
        "the start rewatched"
    );
    assert_eq!(points[99].elapsed, Share::ONE);
    assert_eq!(points[0].relative, Some(Share::of_ratio(0.619)));
    let url = &yt.transport().sent()[0].url;
    assert_eq!(
        param(url, "dimensions").as_deref(),
        Some("elapsedVideoTimeRatio")
    );
    assert_eq!(
        param(url, "metrics").as_deref(),
        Some("audienceWatchRatio,relativeRetentionPerformance")
    );
    assert_eq!(param(url, "filters").as_deref(), Some("video==dQw4w9WgXcQ"));
}

#[test]
fn errors_are_typed() {
    let kind = |name: &str| {
        analytics(&[name])
            .video_report(&token(), VIDEO, &period(), false)
            .unwrap_err()
            .kind
    };
    assert_eq!(kind("invalid-token"), AnalyticsErrorKind::SignedOut);
    assert_eq!(kind("quota-exceeded"), AnalyticsErrorKind::LimitReached);
    assert_eq!(kind("money-forbidden"), AnalyticsErrorKind::Forbidden);
    let offline = YouTubeAnalytics::with_transport(Scripted::offline());
    assert_eq!(
        offline
            .video_report(&token(), VIDEO, &period(), false)
            .unwrap_err()
            .kind,
        AnalyticsErrorKind::Unreachable
    );
    let detail = analytics(&["invalid-token"])
        .video_report(&token(), VIDEO, &period(), false)
        .unwrap_err()
        .detail;
    assert!(detail.starts_with("HTTP 401: authError"), "{detail}");
    assert!(!detail.contains("ya29"), "never the token");
}
