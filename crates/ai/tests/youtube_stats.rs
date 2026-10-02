//! YouTube statistics against recorded responses (see
//! `fixtures/youtube-stats/README.md`). No test calls YouTube.

mod common;

use std::collections::HashMap;

use bardo_ai::YouTubeStats;
use bardo_domain::{ApiKey, Provider, ProviderFailureKind, VideoStatistics, VideoStats};
use common::{Scripted, fixture};

const KEY: &str = "AIzaSyTestKey0001abcdefghijklmnopqrstu";

fn key() -> ApiKey {
    ApiKey::parse(Provider::YouTubeData, KEY).unwrap()
}

fn answers(names: &[&str]) -> YouTubeStats<Scripted> {
    YouTubeStats::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("youtube-stats", name))
            .collect(),
    ))
}

fn query(url: &str) -> HashMap<String, String> {
    let (_, query) = url.split_once('?').unwrap();
    form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

#[test]
fn statistics_come_back_for_the_videos_found() {
    let stats = answers(&["videos-two-of-three"]);
    let found = stats
        .statistics(&key(), &["Qm4f1rT8vXa", "Lp2c9Wk3sYb", "Hy6j8Pm1gFd"])
        .unwrap();
    assert_eq!(
        found,
        [
            VideoStatistics {
                post_id: "Qm4f1rT8vXa".into(),
                published_at: Some(humantime::parse_rfc3339("2026-09-27T14:00:08Z").unwrap()),
                views: 125_431,
                likes: Some(8_812),
                comments: Some(214),
            },
            // Likes hidden and comments off: absent, not zero.
            VideoStatistics {
                post_id: "Hy6j8Pm1gFd".into(),
                published_at: Some(humantime::parse_rfc3339("2026-09-30T09:15:42Z").unwrap()),
                views: 2_210,
                likes: None,
                comments: None,
            },
        ]
    );
}

#[test]
fn one_call_asks_for_every_id_and_only_the_fields_read() {
    let stats = answers(&["videos-two-of-three"]);
    stats
        .statistics(&key(), &["Qm4f1rT8vXa", "Lp2c9Wk3sYb", "Hy6j8Pm1gFd"])
        .unwrap();
    let sent = stats.transport().sent();
    assert_eq!(sent.len(), 1);
    let url = &sent[0].url;
    assert!(url.starts_with("https://www.googleapis.com/youtube/v3/videos?"));
    let query = query(url);
    assert_eq!(query["id"], "Qm4f1rT8vXa,Lp2c9Wk3sYb,Hy6j8Pm1gFd");
    assert_eq!(query["part"], "statistics,snippet");
    assert_eq!(
        query["fields"],
        "items(id,snippet/publishedAt,statistics(viewCount,likeCount,commentCount))"
    );
    assert!(
        !query.contains_key("maxResults"),
        "not supported with id lookups"
    );
    // The key travels in a header, never in the URL.
    assert_eq!(sent[0].header("x-goog-api-key"), Some(KEY));
    assert!(!url.contains(KEY));
}

#[test]
fn more_than_fifty_ids_go_in_batches_of_fifty() {
    let ids: Vec<String> = (0..120).map(|n| format!("vid{n:08}")).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let stats = answers(&["videos-none", "videos-none", "videos-none"]);
    assert_eq!(stats.statistics(&key(), &ids).unwrap(), []);
    let batches: Vec<usize> = stats
        .transport()
        .sent()
        .iter()
        .map(|sent| query(&sent.url)["id"].split(',').count())
        .collect();
    assert_eq!(batches, [50, 50, 20]);
}

#[test]
fn ids_nobody_asked_for_are_ignored() {
    let stats = answers(&["videos-stranger"]);
    assert_eq!(stats.statistics(&key(), &["Qm4f1rT8vXa"]).unwrap(), []);
}

#[test]
fn a_video_without_a_view_count_is_an_unexpected_answer() {
    let stats = answers(&["videos-no-views"]);
    let failure = stats.statistics(&key(), &["Qm4f1rT8vXa"]).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
}

#[test]
fn key_and_quota_failures_are_classified() {
    let rejected = answers(&["videos-key-rejected"])
        .statistics(&key(), &["Qm4f1rT8vXa"])
        .unwrap_err();
    assert_eq!(rejected.kind, ProviderFailureKind::Rejected);
    let quota = answers(&["videos-quota"])
        .statistics(&key(), &["Qm4f1rT8vXa"])
        .unwrap_err();
    assert_eq!(quota.kind, ProviderFailureKind::LimitReached);
    let offline = YouTubeStats::with_transport(Scripted::offline())
        .statistics(&key(), &["Qm4f1rT8vXa"])
        .unwrap_err();
    assert_eq!(offline.kind, ProviderFailureKind::Unreachable);
}

#[test]
fn no_ids_make_no_call() {
    let stats = answers(&[]);
    assert_eq!(stats.statistics(&key(), &[]).unwrap(), []);
    assert!(stats.transport().sent().is_empty());
}
