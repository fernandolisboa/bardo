//! Instagram post insights (#85): the Instagram API with Facebook Login
//! reads, for the connected professional account, each post's insights and
//! the account's media.
//!
//! - `GET /<IG_MEDIA_ID>/insights?metric=…` answers one entry per metric
//!   with its lifetime value (`values[0].value`). Bardo asks for `views`,
//!   `reach`, `likes`, `comments`, `shares`, `saved`, `total_interactions`,
//!   `ig_reels_avg_watch_time` and `ig_reels_video_view_total_time`; never
//!   `plays`, `impressions` or `video_views`, which are gone (changelog,
//!   2025-01-21). The two watch metrics exist for Reels only, so a linked
//!   feed post the request is refused for is asked again without them.
//! - "If insights data you are requesting does not exist or is currently
//!   unavailable, the API returns an empty data set instead of 0 for
//!   individual metrics", and the data can be 48 hours late: a metric left
//!   out reads as `None`, an answer with none as no data yet.
//! - The watch metrics are read as milliseconds, as Instagram's own
//!   reports show them. The reference gives no unit: an assumption until a
//!   real account confirms it.
//! - `GET /<IG_USER_ID>/media?fields=id,permalink,shortcode,timestamp`
//!   lists the account's media, newest first, a page at a time with the
//!   `after` cursor while `paging.next` is there. A linked post carries a
//!   shortcode; its media id comes from this listing.
//!
//! Insights take `instagram_manage_insights`, which Bardo asks for at sign
//! in, and the Page token the connection keeps.
//!
//! Docs checked 2026-10-04: <https://developers.facebook.com/docs/instagram-platform/reference/instagram-media/insights>,
//! <https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/ig-user/media>,
//! <https://developers.facebook.com/docs/instagram-platform/changelog>,
//! <https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/error-codes>.

use std::time::{Duration, SystemTime};

use bardo_ai::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use bardo_domain::{
    AnalyticsError, AnalyticsErrorKind, Insights, InsightsReader, MediaItem, MediaPage, Network,
    PostNumbers, SecretText, whole,
};
use serde_json::Value;

use crate::MetaEndpoints;
use crate::text::{SHOWN_TEXT, plain};

/// A Reel's metrics.
const REEL_METRICS: &str = "views,reach,likes,comments,shares,saved,total_interactions,\
                            ig_reels_avg_watch_time,ig_reels_video_view_total_time";

/// A feed post's: the Reel's without the watch times.
const POST_METRICS: &str = "views,reach,likes,comments,shares,saved,total_interactions";

/// Media a listing page asks for.
const PAGE_SIZE: u32 = 100;

/// Reads Instagram insights over HTTPS.
pub struct InstagramInsights<T = UreqTransport> {
    transport: T,
    endpoints: MetaEndpoints,
}

impl InstagramInsights {
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for InstagramInsights {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> InstagramInsights<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: MetaEndpoints::default(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: MetaEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// A Graph API read as the token's owner, answered in JSON.
    fn get(&self, url: String, token: &SecretText) -> Result<Value, AnalyticsError> {
        let request =
            HttpRequest::get(url).header("authorization", format!("Bearer {}", token.expose()));
        let response = self
            .transport
            .send(&request)
            .map_err(|error| AnalyticsError::new(AnalyticsErrorKind::Unreachable, error.0))?;
        if !(200..=299).contains(&response.status) {
            return Err(graph_failure(&response));
        }
        serde_json::from_str(&response.body)
            .map_err(|error| unexpected(&format!("unreadable Graph API answer: {error}")))
    }

    fn insights(
        &self,
        token: &SecretText,
        media: &str,
        metrics: &str,
    ) -> Result<PostNumbers, AnalyticsError> {
        let url = format!(
            "{}/{media}/insights?metric={}",
            self.endpoints.graph,
            metrics.replace(',', "%2C")
        );
        read_numbers(&self.get(url, token)?)
    }

    /// One media's numbers: as a Reel, else as a feed post when the Reel's
    /// metrics are refused for it.
    fn media_numbers(
        &self,
        token: &SecretText,
        media: &str,
    ) -> Result<PostNumbers, AnalyticsError> {
        if !is_id(media) {
            return Err(AnalyticsError::new(
                AnalyticsErrorKind::Invalid,
                "not an Instagram media id",
            ));
        }
        match self.insights(token, media, REEL_METRICS) {
            Err(error) if error.kind == AnalyticsErrorKind::Invalid => {
                self.insights(token, media, POST_METRICS)
            }
            other => other,
        }
    }
}

impl<T: Transport> InsightsReader for InstagramInsights<T> {
    fn network(&self) -> Network {
        Network::InstagramReels
    }

    /// Insights are read one media at a time.
    fn batch(&self) -> usize {
        1
    }

    fn read(
        &self,
        token: &SecretText,
        posts: &[&str],
    ) -> Result<Vec<(String, PostNumbers)>, AnalyticsError> {
        posts
            .iter()
            .map(|media| Ok(((*media).to_owned(), self.media_numbers(token, media)?)))
            .collect()
    }

    fn media_page(
        &self,
        token: &SecretText,
        account: &str,
        after: Option<&str>,
    ) -> Result<MediaPage, AnalyticsError> {
        if !is_id(account) {
            return Err(AnalyticsError::new(
                AnalyticsErrorKind::Invalid,
                "not an Instagram account id",
            ));
        }
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.extend_pairs([
            ("fields", "id,permalink,shortcode,timestamp"),
            ("limit", PAGE_SIZE.to_string().as_str()),
        ]);
        if let Some(after) = after {
            query.append_pair("after", after);
        }
        let body = self.get(
            format!(
                "{}/{account}/media?{}",
                self.endpoints.graph,
                query.finish()
            ),
            token,
        )?;
        read_page(&body)
    }
}

/// The numbers of an insights answer. A metric left out, or without a
/// number, is `None`.
fn read_numbers(body: &Value) -> Result<PostNumbers, AnalyticsError> {
    let data = body["data"]
        .as_array()
        .ok_or_else(|| unexpected("the insights answer has no data list"))?;
    let value = |name: &str| -> Option<f64> {
        let entry = data.iter().find(|entry| entry["name"] == name)?;
        entry["values"]
            .get(0)
            .map(|first| &first["value"])
            .filter(|value| !value.is_null())
            .or_else(|| entry.get("total_value").map(|total| &total["value"]))
            .and_then(Value::as_f64)
    };
    let count = |name: &str| value(name).map(whole);
    let millis = |name: &str| value(name).map(|ms| Duration::from_millis(whole(ms)));
    Ok(PostNumbers {
        views: count("views"),
        likes: count("likes"),
        comments: count("comments"),
        insights: Insights {
            shares: count("shares"),
            saves: count("saved"),
            reach: count("reach"),
            interactions: count("total_interactions"),
            average_watch: millis("ig_reels_avg_watch_time"),
            watch_time: millis("ig_reels_video_view_total_time"),
        },
        posted_at: None,
    })
}

/// A media listing page. An item without a usable id is skipped.
fn read_page(body: &Value) -> Result<MediaPage, AnalyticsError> {
    let data = body["data"]
        .as_array()
        .ok_or_else(|| unexpected("the media listing has no data list"))?;
    let text = |value: &Value| value.as_str().map(|text| plain(text, SHOWN_TEXT));
    let items = data
        .iter()
        .filter_map(|item| {
            let id = item["id"].as_str().filter(|id| is_id(id))?;
            Some(MediaItem {
                id: id.to_owned(),
                permalink: text(&item["permalink"]),
                shortcode: text(&item["shortcode"]),
                posted_at: item["timestamp"].as_str().and_then(timestamp),
            })
        })
        .collect();
    let next = body["paging"]["next"]
        .as_str()
        .and(body["paging"]["cursors"]["after"].as_str())
        .map(|cursor| plain(cursor, SHOWN_TEXT))
        .filter(|cursor| !cursor.is_empty());
    Ok(MediaPage { items, next })
}

/// A Graph API time, `2026-10-01T18:04:12+0000`.
fn timestamp(text: &str) -> Option<SystemTime> {
    let mut text = text.to_owned();
    // The offset comes without its colon, which RFC 3339 wants.
    let split = text.len().checked_sub(2)?;
    if text.is_char_boundary(split) && text[..split].ends_with(|c: char| c.is_ascii_digit()) {
        let sign = text.len().checked_sub(5)?;
        if text.is_char_boundary(sign) && matches!(&text[sign..sign + 1], "+" | "-") {
            text.insert(split, ':');
        }
    }
    text.parse::<jiff::Timestamp>().ok().map(SystemTime::from)
}

/// An id as Meta hands them out: digits only, so it goes in an address as
/// it is.
fn is_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_digit())
}

fn unexpected(detail: &str) -> AnalyticsError {
    AnalyticsError::new(AnalyticsErrorKind::Unexpected, detail)
}

/// Classifies a Graph API error answer by its documented codes.
fn graph_failure(response: &HttpResponse) -> AnalyticsError {
    let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
    let error = &body["error"];
    let code = error["code"].as_i64().unwrap_or_default();
    let subcode = error["error_subcode"].as_i64().unwrap_or_default();
    let message = plain(error["message"].as_str().unwrap_or_default(), SHOWN_TEXT);
    let transient = error["is_transient"].as_bool().unwrap_or_default();
    let kind = match (response.status, code, subcode) {
        // 190: expired, revoked or otherwise invalid; 102: session.
        (_, 190 | 102, _) => AnalyticsErrorKind::SignedOut,
        (_, 4 | 17 | 32 | 613, _) | (429, _, _) => AnalyticsErrorKind::LimitReached,
        // An object Meta does not have (any more).
        (_, 100, 33) | (_, 24, _) | (404, _, _) => AnalyticsErrorKind::NotFound,
        // 10 and 200-299: a permission not granted, or "not enough viewers
        // for the media to show insights"; 3: capability.
        (_, 3 | 10 | 200..=299, _) => AnalyticsErrorKind::Forbidden,
        (_, 1 | 2, _) | (500..=599, _, _) => AnalyticsErrorKind::Unreachable,
        _ if transient => AnalyticsErrorKind::Unreachable,
        (401, _, _) => AnalyticsErrorKind::SignedOut,
        (_, 100, _) | (400, _, _) => AnalyticsErrorKind::Invalid,
        _ => AnalyticsErrorKind::Unexpected,
    };
    let detail = match (code, subcode, message.as_str()) {
        (0, _, "") => format!("HTTP {}", response.status),
        (code, 0, message) => format!("HTTP {}: {code}: {message}", response.status),
        (code, subcode, message) => {
            format!("HTTP {}: {code}/{subcode}: {message}", response.status)
        }
    };
    AnalyticsError::new(kind, detail)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn graph_times_read_with_or_without_the_offsets_colon() {
        let at = |text| timestamp(text).map(|t| jiff::Timestamp::try_from(t).unwrap().to_string());
        assert_eq!(
            at("2026-10-01T18:04:12+0000").as_deref(),
            Some("2026-10-01T18:04:12Z")
        );
        assert_eq!(
            at("2026-10-01T15:04:12-0300").as_deref(),
            Some("2026-10-01T18:04:12Z")
        );
        assert_eq!(
            at("2026-10-01T18:04:12+00:00").as_deref(),
            Some("2026-10-01T18:04:12Z")
        );
        assert_eq!(at("yesterday"), None);
        assert_eq!(at(""), None);
    }

    #[test]
    fn a_metric_left_out_is_empty_and_a_zero_is_zero() {
        let numbers = read_numbers(&json!({ "data": [
            { "name": "views", "period": "lifetime", "values": [{ "value": 0 }] },
            { "name": "likes", "period": "lifetime", "values": [{ "value": 7 }] },
            { "name": "ig_reels_avg_watch_time", "values": [{ "value": 4_520.4 }] },
            { "name": "shares", "total_value": { "value": 2 } },
            { "name": "comments", "values": [] },
            { "name": "saved", "values": [{ "value": null }] }
        ]}))
        .unwrap();
        assert_eq!(numbers.views, Some(0));
        assert_eq!(numbers.likes, Some(7));
        assert_eq!(numbers.comments, None, "no value is no number");
        assert_eq!(numbers.insights.saves, None);
        assert_eq!(numbers.insights.shares, Some(2));
        assert_eq!(numbers.insights.reach, None, "left out");
        assert_eq!(
            numbers.insights.average_watch,
            Some(Duration::from_millis(4_520))
        );
        let empty = read_numbers(&json!({ "data": [] })).unwrap();
        assert!(empty.is_empty());
        assert!(read_numbers(&json!({ "error": {} })).is_err());
    }

    #[test]
    fn only_digit_ids_go_into_an_address() {
        assert!(is_id("17895695668004550"));
        assert!(!is_id("1789/../me"));
        assert!(!is_id(""));
    }
}
