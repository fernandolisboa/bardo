//! Instagram post insights against recorded responses (see
//! `fixtures/instagram-insights/README.md`). No test calls Meta.

mod common;

use std::time::Duration;

use bardo_domain::{
    AnalyticsErrorKind, InsightsReader, MEDIA_PAGES, PostReading, SecretText, find_media,
    read_insights,
};
use bardo_publish::InstagramInsights;
use common::{Scripted, fixture};

const REEL: &str = "17900000000000001";
const ACCOUNT: &str = "17841400000000001";

fn insights(names: &[&str]) -> InstagramInsights<Scripted> {
    InstagramInsights::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("instagram-insights", name))
            .collect(),
    ))
}

fn token() -> SecretText {
    SecretText::new("EAAG.fixture-page-token")
}

fn param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

#[test]
fn a_reels_insights_read_every_metric_with_the_page_token() {
    let ig = insights(&["reel-insights"]);
    let read = ig.read(&token(), &[REEL]).unwrap();
    assert_eq!(read.len(), 1);
    let (id, numbers) = &read[0];
    assert_eq!(id, REEL);
    assert_eq!(numbers.views, Some(48_210));
    assert_eq!(numbers.likes, Some(2_190));
    assert_eq!(numbers.comments, Some(87));
    assert_eq!(numbers.insights.reach, Some(31_544));
    assert_eq!(numbers.insights.shares, Some(412));
    assert_eq!(numbers.insights.saves, Some(0), "a reported zero");
    assert_eq!(numbers.insights.interactions, Some(2_689));
    assert_eq!(
        numbers.insights.average_watch,
        Some(Duration::from_millis(7_342))
    );
    assert_eq!(
        numbers.insights.watch_time,
        Some(Duration::from_millis(353_957_820))
    );

    let sent = ig.transport().sent();
    assert_eq!(sent.len(), 1);
    let url = &sent[0].url;
    assert!(
        url.starts_with("https://graph.facebook.com/v25.0/17900000000000001/insights?"),
        "{url}"
    );
    let metrics = param(url, "metric").unwrap();
    assert_eq!(
        metrics,
        "views,reach,likes,comments,shares,saved,total_interactions,\
         ig_reels_avg_watch_time,ig_reels_video_view_total_time"
    );
    for gone in ["plays", "impressions", "video_views"] {
        assert!(!metrics.split(',').any(|m| m == gone), "{gone}");
    }
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer EAAG.fixture-page-token")
    );
    assert!(!url.contains("access_token"), "the token stays in a header");
}

#[test]
fn metrics_not_arrived_yet_are_empty_not_zero() {
    let ig = insights(&["reel-insights-partial"]);
    let (_, numbers) = ig.read(&token(), &[REEL]).unwrap().remove(0);
    assert_eq!(numbers.views, Some(1_204));
    assert_eq!(numbers.likes, Some(0));
    assert_eq!(numbers.insights.shares, None);
    assert_eq!(numbers.insights.reach, None);
    assert_eq!(numbers.insights.average_watch, None);

    let ig = insights(&["insights-empty"]);
    let (_, numbers) = ig.read(&token(), &[REEL]).unwrap().remove(0);
    assert!(numbers.is_empty(), "no data yet");
}

#[test]
fn a_feed_post_is_asked_again_without_the_reel_only_metrics() {
    let ig = insights(&["feed-metric-refused", "feed-insights"]);
    let (_, numbers) = ig.read(&token(), &["17900000000000002"]).unwrap().remove(0);
    assert_eq!(numbers.views, Some(950));
    assert_eq!(numbers.insights.saves, Some(11));
    assert_eq!(numbers.insights.average_watch, None);
    let sent = ig.transport().sent();
    assert_eq!(
        param(&sent[1].url, "metric").as_deref(),
        Some("views,reach,likes,comments,shares,saved,total_interactions")
    );
}

#[test]
fn a_media_meta_does_not_have_is_not_found_and_a_refused_one_is_unread() {
    let ig = insights(&["media-gone", "not-enough-viewers", "reel-insights"]);
    let read = read_insights(
        &ig,
        &token(),
        &["17900000000000009", "17900000000000008", REEL],
    )
    .unwrap();
    assert_eq!(read[0].1, PostReading::NotFound);
    assert_eq!(read[1].1, PostReading::Unread);
    assert!(matches!(read[2].1, PostReading::Found(_)));
}

#[test]
fn a_refused_token_or_the_limit_stops_the_account() {
    let ig = insights(&["invalid-token"]);
    let error = read_insights(&ig, &token(), &[REEL, "17900000000000002"]).unwrap_err();
    assert_eq!(error.kind, AnalyticsErrorKind::SignedOut);
    assert!(error.detail.contains("190"), "{}", error.detail);
    assert_eq!(ig.transport().sent().len(), 1);

    let ig = insights(&["rate-limited"]);
    let error = read_insights(&ig, &token(), &[REEL]).unwrap_err();
    assert_eq!(error.kind, AnalyticsErrorKind::LimitReached);
}

#[test]
fn an_id_that_is_not_digits_never_reaches_an_address() {
    let ig = insights(&[]);
    let error = ig.read(&token(), &["1789/../me"]).unwrap_err();
    assert_eq!(error.kind, AnalyticsErrorKind::Invalid);
    assert!(ig.transport().sent().is_empty());
    assert!(ig.media_page(&token(), "me", None).is_err());
}

#[test]
fn a_linked_posts_shortcode_maps_to_its_media_over_the_listing() {
    let ig = insights(&["media-page-1", "media-page-2"]);
    let lookup = find_media(&ig, &token(), ACCOUNT, &["C9xYz12AbCd"], MEDIA_PAGES).unwrap();
    let media = lookup.media("C9xYz12AbCd").unwrap();
    assert_eq!(media.id, "17900000000000002");
    let posted = jiff::Timestamp::try_from(media.posted_at.unwrap()).unwrap();
    assert_eq!(posted.to_string(), "2026-09-28T09:00:00Z");

    let sent = ig.transport().sent();
    assert_eq!(sent.len(), 2);
    assert!(
        sent[0]
            .url
            .starts_with("https://graph.facebook.com/v25.0/17841400000000001/media?"),
        "{}",
        sent[0].url
    );
    assert_eq!(
        param(&sent[0].url, "fields").as_deref(),
        Some("id,permalink,shortcode,timestamp")
    );
    assert_eq!(param(&sent[0].url, "limit").as_deref(), Some("100"));
    assert_eq!(param(&sent[0].url, "after"), None);
    assert_eq!(
        param(&sent[1].url, "after").as_deref(),
        Some("QVFIUjBfMzJpZAkx0ZAzZA"),
        "the cursor, not the next address Meta sent"
    );
    assert_eq!(
        sent[1].header("authorization"),
        Some("Bearer EAAG.fixture-page-token")
    );
}

#[test]
fn a_shortcode_the_whole_listing_lacks_is_not_the_accounts() {
    let ig = insights(&["media-page-1", "media-page-2"]);
    let lookup = find_media(&ig, &token(), ACCOUNT, &["ZZZZZZZZZZZ"], MEDIA_PAGES).unwrap();
    assert!(lookup.complete);
    assert_eq!(lookup.media("ZZZZZZZZZZZ"), Err(true));

    let ig = insights(&["invalid-token"]);
    let error = find_media(&ig, &token(), ACCOUNT, &["ZZZZZZZZZZZ"], MEDIA_PAGES).unwrap_err();
    assert_eq!(error.kind, AnalyticsErrorKind::SignedOut);
}
