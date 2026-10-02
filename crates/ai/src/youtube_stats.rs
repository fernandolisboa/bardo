//! Public statistics of the user's own YouTube posts, from the YouTube
//! Data API v3 with the API key alone (no OAuth): `videos.list` with
//! `part=statistics,snippet`, up to 50 ids per call for one quota unit.
//! Only the fields Bardo reads are asked for.
//!
//! A video the API does not return (removed, private) is left out; likes
//! and comments the owner hides or turns off are absent from `statistics`
//! and come back as `None`.

use std::time::Duration;

use bardo_domain::{ApiKey, ProviderFailure, STATS_BATCH, VideoStatistics, VideoStats};

use crate::http::{HttpRequest, Transport, UreqTransport};
use crate::market::{call, count, items, request, unexpected};

/// The `videos.list` request for the statistics and publish time of
/// `ids`. `maxResults` does not apply to id lookups, so it is not sent.
pub fn statistics_request(key: &ApiKey, ids: &[&str]) -> HttpRequest {
    request(
        key,
        "videos",
        &[
            ("part", "statistics,snippet"),
            ("id", &ids.join(",")),
            (
                "fields",
                "items(id,snippet/publishedAt,statistics(viewCount,likeCount,commentCount))",
            ),
        ],
    )
}

/// Reads YouTube statistics over HTTPS.
pub struct YouTubeStats<T = UreqTransport> {
    transport: T,
}

impl YouTubeStats {
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for YouTubeStats {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> YouTubeStats<T> {
    pub fn with_transport(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T: Transport> VideoStats for YouTubeStats<T> {
    fn statistics(
        &self,
        key: &ApiKey,
        ids: &[&str],
    ) -> Result<Vec<VideoStatistics>, ProviderFailure> {
        let mut found = Vec::new();
        for batch in ids.chunks(STATS_BATCH) {
            let body = call(&self.transport, &statistics_request(key, batch))?;
            if !body.is_object() {
                return Err(unexpected("videos response is not an object"));
            }
            for item in items(&body) {
                let Some(id) = item["id"].as_str() else {
                    continue;
                };
                // Only the ids asked for: an answer about another video
                // would credit a stranger's numbers to the user's post.
                if !batch.contains(&id) {
                    continue;
                }
                let statistics = &item["statistics"];
                let Some(views) = count(&statistics["viewCount"]) else {
                    return Err(unexpected(&format!("video {id} without a view count")));
                };
                found.push(VideoStatistics {
                    post_id: id.to_owned(),
                    published_at: item["snippet"]["publishedAt"]
                        .as_str()
                        .and_then(|text| humantime::parse_rfc3339_weak(text).ok()),
                    views,
                    likes: count(&statistics["likeCount"]),
                    comments: count(&statistics["commentCount"]),
                });
            }
        }
        Ok(found)
    }
}
