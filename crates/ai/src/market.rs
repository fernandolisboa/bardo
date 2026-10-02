//! Market data from the YouTube Data API v3 (ADR-0004): recent uploads for
//! a niche in a country and language, with their views and channel sizes.
//!
//! One niche costs three calls: `search.list` (100 quota units) for the
//! uploads and their estimated count, then `videos.list` and
//! `channels.list` (1 unit each) for views and subscribers. Each call asks
//! only for the fields Bardo reads.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, Market, MarketData, MarketSample, Niche, Provider, ProviderFailure,
    ProviderFailureKind, UploadSample,
};
use serde_json::Value;

use crate::http::{HttpRequest, Transport, UreqTransport};
use crate::key_check::failure;

const API: &str = "https://www.googleapis.com/youtube/v3";

/// The most results one search page returns, and the most ids one
/// `videos.list` or `channels.list` call accepts.
pub const MAX_RESULTS: usize = 50;

/// Quota units one niche costs: a search and two id lookups.
pub const QUOTA_UNITS_PER_NICHE: u32 = 100 + 1 + 1;

/// The `search.list` request for recent uploads of `niche` in `market`.
/// Ordered by relevance, so the sample is what a viewer searching the niche
/// would see, not only the newest or the biggest videos.
pub fn search_request(
    key: &ApiKey,
    niche: &Niche,
    market: Market,
    since: SystemTime,
) -> HttpRequest {
    let published_after = humantime::format_rfc3339_seconds(since).to_string();
    let max_results = MAX_RESULTS.to_string();
    request(
        key,
        "search",
        &[
            ("part", "snippet"),
            ("type", "video"),
            ("order", "relevance"),
            ("maxResults", &max_results),
            ("publishedAfter", &published_after),
            ("regionCode", market.country.code()),
            ("relevanceLanguage", market.language.code()),
            ("q", niche.label()),
            (
                "fields",
                "pageInfo/totalResults,items(id/videoId,snippet(channelId,publishedAt))",
            ),
        ],
    )
}

/// The `videos.list` request for the view counts of `ids`.
pub fn videos_request(key: &ApiKey, ids: &[&str]) -> HttpRequest {
    request(
        key,
        "videos",
        &[
            ("part", "statistics"),
            ("id", &ids.join(",")),
            ("maxResults", &MAX_RESULTS.to_string()),
            ("fields", "items(id,statistics/viewCount)"),
        ],
    )
}

/// The `channels.list` request for the subscriber counts of `ids`.
pub fn channels_request(key: &ApiKey, ids: &[&str]) -> HttpRequest {
    request(
        key,
        "channels",
        &[
            ("part", "statistics"),
            ("id", &ids.join(",")),
            ("maxResults", &MAX_RESULTS.to_string()),
            (
                "fields",
                "items(id,statistics(subscriberCount,hiddenSubscriberCount))",
            ),
        ],
    )
}

/// The key goes in a header, keeping it out of the URL and so out of any
/// error that quotes the URL.
fn request(key: &ApiKey, resource: &str, params: &[(&str, &str)]) -> HttpRequest {
    let query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    HttpRequest::get(format!("{API}/{resource}?{query}")).header("x-goog-api-key", key.expose())
}

/// A search hit before its numbers are looked up.
struct Hit {
    video_id: String,
    channel_id: String,
    published_at: SystemTime,
}

fn parse_search(body: &Value) -> Result<(u64, Vec<Hit>), ProviderFailure> {
    let upload_volume = body["pageInfo"]["totalResults"]
        .as_u64()
        .ok_or_else(|| unexpected("search response without pageInfo.totalResults"))?;
    let hits = items(body)
        .filter_map(|item| {
            let snippet = &item["snippet"];
            Some(Hit {
                video_id: item["id"]["videoId"].as_str()?.to_owned(),
                channel_id: snippet["channelId"].as_str()?.to_owned(),
                published_at: humantime::parse_rfc3339_weak(snippet["publishedAt"].as_str()?)
                    .ok()?,
            })
        })
        .collect();
    Ok((upload_volume, hits))
}

/// `items[]`, empty when absent (the API leaves it out when nothing
/// matches).
fn items(body: &Value) -> impl Iterator<Item = &Value> {
    body["items"].as_array().into_iter().flatten()
}

/// YouTube sends counts as decimal strings.
fn count(value: &Value) -> Option<u64> {
    value.as_str()?.parse().ok()
}

fn unexpected(detail: &str) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

/// Fetches market data over HTTPS.
pub struct YouTubeMarketData<T = UreqTransport> {
    transport: T,
}

impl YouTubeMarketData {
    /// A search can take a few seconds on a slow connection.
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for YouTubeMarketData {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> YouTubeMarketData<T> {
    pub fn with_transport(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Sends the request and reads the JSON body of a successful answer.
    /// Failed answers are classified like key checks, since YouTube reports
    /// a bad key, a disabled API or a spent quota the same way everywhere.
    fn call(&self, request: &HttpRequest) -> Result<Value, ProviderFailure> {
        let response = self
            .transport
            .send(request)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        if !(200..=299).contains(&response.status) {
            return Err(failure(Provider::YouTubeData, &response));
        }
        serde_json::from_str(&response.body)
            .map_err(|error| unexpected(&format!("unreadable response: {error}")))
    }
}

impl<T: Transport> MarketData for YouTubeMarketData<T> {
    fn recent_uploads(
        &self,
        key: &ApiKey,
        niche: &Niche,
        market: Market,
        since: SystemTime,
    ) -> Result<MarketSample, ProviderFailure> {
        let search = self.call(&search_request(key, niche, market, since))?;
        let (upload_volume, hits) = parse_search(&search)?;
        if hits.is_empty() {
            return Ok(MarketSample {
                upload_volume,
                uploads: Vec::new(),
            });
        }

        let video_ids: Vec<&str> = hits.iter().map(|hit| hit.video_id.as_str()).collect();
        let videos = self.call(&videos_request(key, &video_ids))?;
        let views: HashMap<&str, u64> = items(&videos)
            .filter_map(|item| {
                Some((
                    item["id"].as_str()?,
                    count(&item["statistics"]["viewCount"])?,
                ))
            })
            .collect();

        let mut seen = HashSet::new();
        let channel_ids: Vec<&str> = hits
            .iter()
            .map(|hit| hit.channel_id.as_str())
            .filter(|id| seen.insert(*id))
            .collect();
        let channels = self.call(&channels_request(key, &channel_ids))?;
        let subscribers: HashMap<&str, u64> = items(&channels)
            .filter(|item| item["statistics"]["hiddenSubscriberCount"].as_bool() != Some(true))
            .filter_map(|item| {
                Some((
                    item["id"].as_str()?,
                    count(&item["statistics"]["subscriberCount"])?,
                ))
            })
            .collect();

        // A video without statistics was removed or made private between
        // the calls; it is left out rather than counted as unwatched.
        let uploads = hits
            .iter()
            .filter_map(|hit| {
                Some(UploadSample {
                    channel_id: hit.channel_id.clone(),
                    published_at: hit.published_at,
                    views: *views.get(hit.video_id.as_str())?,
                    channel_subscribers: subscribers.get(hit.channel_id.as_str()).copied(),
                })
            })
            .collect();
        Ok(MarketSample {
            upload_volume,
            uploads,
        })
    }

    fn quota_units_per_niche(&self) -> u32 {
        QUOTA_UNITS_PER_NICHE
    }
}
