//! TikTok post numbers against recorded responses (see
//! `fixtures/tiktok-video-query/README.md`). No test calls TikTok.

mod common;

use std::time::{Duration, SystemTime};

use bardo_domain::{AnalyticsErrorKind, InsightsReader, PostReading, SecretText, read_insights};
use bardo_publish::TikTokInsights;
use common::{Scripted, fixture};
use serde_json::Value;

const FIRST: &str = "7301234567890123456";
const SECOND: &str = "7301234567890123999";
const GONE: &str = "7301234567890120000";

fn tiktok(names: &[&str]) -> TikTokInsights<Scripted> {
    TikTokInsights::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("tiktok-video-query", name))
            .collect(),
    ))
}

fn token() -> SecretText {
    SecretText::new("act.fixture-access")
}

#[test]
fn the_creators_videos_read_their_counts_and_one_left_out_is_not_found() {
    let tt = tiktok(&["query-ok"]);
    let read = read_insights(&tt, &token(), &[FIRST, GONE, SECOND]).unwrap();
    let PostReading::Found(first) = read[0].1 else {
        panic!("{:?}", read[0]);
    };
    assert_eq!(first.views, Some(125_400));
    assert_eq!(first.likes, Some(9_810));
    assert_eq!(first.comments, Some(312));
    assert_eq!(first.insights.shares, Some(0), "a reported zero");
    assert_eq!(
        first.posted_at,
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000))
    );
    assert_eq!(
        first.insights.average_watch, None,
        "no watch time on TikTok"
    );
    assert_eq!(read[1], (GONE.to_owned(), PostReading::NotFound));
    assert!(matches!(read[2].1, PostReading::Found(n) if n.views == Some(880)));

    let sent = tt.transport().sent();
    assert_eq!(sent.len(), 1, "one query for up to 20");
    assert_eq!(
        sent[0].url,
        "https://open.tiktokapis.com/v2/video/query/\
         ?fields=id,create_time,view_count,like_count,comment_count,share_count"
    );
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer act.fixture-access")
    );
    assert_eq!(sent[0].header("content-type"), Some("application/json"));
    let body: Value = serde_json::from_str(&sent[0].body).unwrap();
    assert_eq!(
        body["filters"]["video_ids"],
        serde_json::json!([FIRST, GONE, SECOND])
    );
}

#[test]
fn more_than_twenty_videos_go_in_several_queries() {
    let ids: Vec<String> = (0..21).map(|n| format!("73012345678901{n:05}")).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let tt = tiktok(&["query-none", "query-none"]);
    let read = read_insights(&tt, &token(), &ids).unwrap();
    assert_eq!(read.len(), 21);
    assert!(read.iter().all(|(_, r)| *r == PostReading::NotFound));
    let sent = tt.transport().sent();
    assert_eq!(sent.len(), 2);
    let second: Value = serde_json::from_str(&sent[1].body).unwrap();
    assert_eq!(second["filters"]["video_ids"].as_array().unwrap().len(), 1);
}

#[test]
fn a_refused_token_or_the_limit_stops_the_account() {
    let tt = tiktok(&["invalid-token"]);
    let error = read_insights(&tt, &token(), &[FIRST]).unwrap_err();
    assert_eq!(error.kind, AnalyticsErrorKind::SignedOut);
    assert!(error.detail.contains("access_token_invalid"));
    assert!(error.detail.contains("log_id"));

    let tt = tiktok(&["rate-limited"]);
    let error = read_insights(&tt, &token(), &[FIRST]).unwrap_err();
    assert_eq!(error.kind, AnalyticsErrorKind::LimitReached);
}

#[test]
fn a_token_without_the_video_list_scope_reads_nothing_and_flags_nothing() {
    let tt = tiktok(&["scope-not-authorized"]);
    let read = read_insights(&tt, &token(), &[FIRST]).unwrap();
    assert_eq!(read[0].1, PostReading::Unread);
}

#[test]
fn ids_that_are_not_numbers_never_leave() {
    let tt = tiktok(&[]);
    let error = tt.read(&token(), &["7301\"]}"]).unwrap_err();
    assert_eq!(error.kind, AnalyticsErrorKind::Invalid);
    assert!(tt.transport().sent().is_empty());
}
