//! TikTok post numbers (#85): the Display API's video query for the
//! connected creator.
//!
//! - `POST /v2/video/query/?fields=…` with `{"filters":{"video_ids":[…]}}`,
//!   up to 20 ids, answers `data.videos` with each video's `view_count`,
//!   `like_count`, `comment_count`, `share_count` and `create_time` (Unix
//!   seconds). It "verifies that the videos belong to the user": a video
//!   that is not the creator's, removed, or not public is left out.
//! - Errors come as `error.code` (`ok` on success), with a `log_id`.
//! - It takes `video.list`, which Bardo asks for at sign-in.
//!
//! No TikTok API gives watch time or retention.
//!
//! Docs checked 2026-10-04: <https://developers.tiktok.com/doc/tiktok-api-v2-video-query>,
//! <https://developers.tiktok.com/doc/tiktok-api-v2-video-object>,
//! <https://developers.tiktok.com/doc/tiktok-api-v2-error-handling>.

use std::time::{Duration, SystemTime};

use bardo_ai::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use bardo_domain::{
    AnalyticsError, AnalyticsErrorKind, Insights, InsightsReader, Network, PostNumbers, SecretText,
};
use serde_json::{Value, json};

use crate::TikTokEndpoints;
use crate::text::{SHOWN_TEXT, plain};
use crate::tiktok::detail;

const FIELDS: &str = "id,create_time,view_count,like_count,comment_count,share_count";

/// The most ids one query takes.
pub const QUERY_BATCH: usize = 20;

/// Reads TikTok videos' numbers over HTTPS.
pub struct TikTokInsights<T = UreqTransport> {
    transport: T,
    endpoints: TikTokEndpoints,
}

impl TikTokInsights {
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for TikTokInsights {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> TikTokInsights<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: TikTokEndpoints::default(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: TikTokEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T: Transport> InsightsReader for TikTokInsights<T> {
    fn network(&self) -> Network {
        Network::TikTok
    }

    fn batch(&self) -> usize {
        QUERY_BATCH
    }

    fn read(
        &self,
        token: &SecretText,
        posts: &[&str],
    ) -> Result<Vec<(String, PostNumbers)>, AnalyticsError> {
        if posts.is_empty() {
            return Ok(Vec::new());
        }
        if posts.len() > QUERY_BATCH || !posts.iter().all(|id| is_video_id(id)) {
            return Err(AnalyticsError::new(
                AnalyticsErrorKind::Invalid,
                "not up to 20 TikTok video ids",
            ));
        }
        let body = json!({ "filters": { "video_ids": posts } }).to_string();
        let request = HttpRequest::post_json(
            format!("{}/v2/video/query/?fields={FIELDS}", self.endpoints.api),
            body,
        )
        .header("authorization", format!("Bearer {}", token.expose()));
        let response = self
            .transport
            .send(&request)
            .map_err(|error| AnalyticsError::new(AnalyticsErrorKind::Unreachable, error.0))?;
        let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
        let code = body["error"]["code"].as_str().unwrap_or_default();
        if !(200..=299).contains(&response.status) || (!code.is_empty() && code != "ok") {
            return Err(api_failure(&response, &body));
        }
        let videos = body["data"]["videos"]
            .as_array()
            .ok_or_else(|| unexpected("the answer has no video list"))?;
        Ok(videos
            .iter()
            .filter_map(|video| {
                // Ids are 64-bit numbers; TikTok sends them as text.
                let id = match &video["id"] {
                    Value::String(id) => id.clone(),
                    Value::Number(id) => id.to_string(),
                    _ => return None,
                };
                posts.contains(&id.as_str()).then(|| (id, numbers(video)))
            })
            .collect())
    }
}

/// One video's numbers; a count left out is `None`.
fn numbers(video: &Value) -> PostNumbers {
    let count = |name: &str| video[name].as_u64();
    PostNumbers {
        views: count("view_count"),
        likes: count("like_count"),
        comments: count("comment_count"),
        insights: Insights {
            shares: count("share_count"),
            ..Insights::default()
        },
        posted_at: video["create_time"]
            .as_u64()
            .filter(|secs| *secs > 0)
            .map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
    }
}

/// TikTok video ids are numbers of up to 20 digits.
fn is_video_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
}

fn unexpected(detail: &str) -> AnalyticsError {
    AnalyticsError::new(AnalyticsErrorKind::Unexpected, detail)
}

fn api_failure(response: &HttpResponse, body: &Value) -> AnalyticsError {
    let error = &body["error"];
    let code = error["code"].as_str().unwrap_or_default();
    let kind = match (response.status, code) {
        // The code first: TikTok answers a missing scope with a 401 too.
        (_, "access_token_invalid") => AnalyticsErrorKind::SignedOut,
        (_, "scope_not_authorized" | "scope_permission_missed") => AnalyticsErrorKind::Forbidden,
        (_, "rate_limit_exceeded") => AnalyticsErrorKind::LimitReached,
        (_, "internal_error") => AnalyticsErrorKind::Unreachable,
        (_, "invalid_param") => AnalyticsErrorKind::Invalid,
        (401, _) => AnalyticsErrorKind::SignedOut,
        (429, _) => AnalyticsErrorKind::LimitReached,
        (500..=599, _) => AnalyticsErrorKind::Unreachable,
        (403, _) => AnalyticsErrorKind::Forbidden,
        (400, _) => AnalyticsErrorKind::Invalid,
        _ => AnalyticsErrorKind::Unexpected,
    };
    AnalyticsError::new(
        kind,
        detail(
            response.status,
            &plain(code, 64),
            &plain(error["message"].as_str().unwrap_or_default(), SHOWN_TEXT),
            &error["log_id"],
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_numeric_ids_are_queried() {
        assert!(is_video_id("7301234567890123456"));
        assert!(!is_video_id(""));
        assert!(!is_video_id("73012345678901234567890"));
        assert!(!is_video_id("7301\"]}"));
    }

    #[test]
    fn a_count_left_out_is_empty_and_a_zero_is_zero() {
        let numbers = numbers(&json!({
            "id": "7301234567890123456",
            "view_count": 0,
            "like_count": 12,
            "create_time": 1_790_000_000
        }));
        assert_eq!(numbers.views, Some(0));
        assert_eq!(numbers.likes, Some(12));
        assert_eq!(numbers.comments, None);
        assert_eq!(numbers.insights.shares, None);
        assert_eq!(
            numbers.posted_at,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000))
        );
        assert_eq!(numbers.insights.average_watch, None, "TikTok has none");
    }
}
