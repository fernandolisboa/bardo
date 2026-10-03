//! YouTube owner metrics: the Analytics API's `reports.query` for the
//! connected channel (`ids=channel==MINE`), one quota unit per request.
//!
//! - A video's numbers come from the basic report filtered to it
//!   (`filters=video==ID`, no dimension): `views`, `engagedViews`,
//!   `estimatedMinutesWatched`, `averageViewDuration` (seconds) and
//!   `averageViewPercentage`; with money, `estimatedRevenue`, `cpm` and
//!   `playbackBasedCpm` too, in US dollars.
//! - Money metrics need `yt-analytics-monetary.readonly` and a channel in
//!   the Partner Program; outside it they answer 403.
//! - The retention curve is the audience retention report: dimension
//!   `elapsedVideoTimeRatio` (0.01 to 1) with one video in `filters`,
//!   giving `audienceWatchRatio` and `relativeRetentionPerformance`.
//! - Columns are read by the names in `columnHeaders`, not by position. A
//!   report without rows has no data for the video yet.
//! - No new scope is needed: the method takes `youtube` or
//!   `yt-analytics.readonly`, and money needs
//!   `yt-analytics-monetary.readonly`; Bardo asks for all three at
//!   connection and refuses a connection without them.
//!
//! Docs checked 2026-10-03: <https://developers.google.com/youtube/analytics/reference/reports/query>,
//! <https://developers.google.com/youtube/analytics/channel_reports>,
//! <https://developers.google.com/youtube/analytics/metrics>,
//! <https://developers.google.com/youtube/analytics/revision_history>.

use std::time::Duration;

use bardo_ai::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use bardo_domain::{
    AnalyticsError, AnalyticsErrorKind, MoneyReport, Network, OwnerAnalytics, ReportPeriod,
    RetentionPoint, SecretText, Share, VideoReport, dollars, whole,
};
use serde_json::Value;

use crate::GoogleEndpoints;

const VIDEO_METRICS: &str =
    "views,engagedViews,estimatedMinutesWatched,averageViewDuration,averageViewPercentage";
const MONEY_METRICS: &str = "estimatedRevenue,cpm,playbackBasedCpm";
const RETENTION_METRICS: &str = "audienceWatchRatio,relativeRetentionPerformance";

/// Reads owner reports from YouTube Analytics over HTTPS.
pub struct YouTubeAnalytics<T = UreqTransport> {
    transport: T,
    endpoints: GoogleEndpoints,
}

impl YouTubeAnalytics {
    pub const TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for YouTubeAnalytics {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> YouTubeAnalytics<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: GoogleEndpoints::default(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: GoogleEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The report for `video` over `period`, as a table.
    fn query(
        &self,
        token: &SecretText,
        video: &str,
        period: &ReportPeriod,
        metrics: &str,
        dimension: Option<&str>,
    ) -> Result<Table, AnalyticsError> {
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.extend_pairs([
            ("ids", "channel==MINE"),
            ("startDate", period.start().as_str()),
            ("endDate", period.end().as_str()),
            ("metrics", metrics),
            ("filters", format!("video=={video}").as_str()),
        ]);
        if let Some(dimension) = dimension {
            query.append_pair("dimensions", dimension);
        }
        if metrics.contains("estimatedRevenue") {
            query.append_pair("currency", "USD");
        }
        let request = HttpRequest::get(format!(
            "{}/v2/reports?{}",
            self.endpoints.analytics,
            query.finish()
        ))
        .header("authorization", format!("Bearer {}", token.expose()));
        let response = self
            .transport
            .send(&request)
            .map_err(|error| AnalyticsError::new(AnalyticsErrorKind::Unreachable, error.0))?;
        if !(200..=299).contains(&response.status) {
            return Err(failure(&response));
        }
        Table::read(&response.body)
    }
}

impl<T: Transport> OwnerAnalytics for YouTubeAnalytics<T> {
    fn network(&self) -> Network {
        Network::YouTube
    }

    fn video_report(
        &self,
        token: &SecretText,
        video: &str,
        period: &ReportPeriod,
        money: bool,
    ) -> Result<Option<VideoReport>, AnalyticsError> {
        let metrics = if money {
            format!("{VIDEO_METRICS},{MONEY_METRICS}")
        } else {
            VIDEO_METRICS.to_owned()
        };
        let table = self.query(token, video, period, &metrics, None)?;
        let Some(row) = table.rows.first() else {
            return Ok(None);
        };
        let number = |name: &str| table.number(row, name);
        Ok(Some(VideoReport {
            views: whole(number("views")?),
            engaged_views: whole(number("engagedViews")?),
            minutes_watched: whole(number("estimatedMinutesWatched")?),
            average_view_seconds: whole(number("averageViewDuration")?),
            average_view_share: Share::of_percent(number("averageViewPercentage")?),
            money: if money {
                Some(MoneyReport {
                    revenue: dollars(number("estimatedRevenue")?),
                    cpm: dollars(number("cpm")?),
                    playback_cpm: dollars(number("playbackBasedCpm")?),
                })
            } else {
                None
            },
        }))
    }

    fn retention(
        &self,
        token: &SecretText,
        video: &str,
        period: &ReportPeriod,
    ) -> Result<Vec<RetentionPoint>, AnalyticsError> {
        let table = self.query(
            token,
            video,
            period,
            RETENTION_METRICS,
            Some("elapsedVideoTimeRatio"),
        )?;
        table
            .rows
            .iter()
            .map(|row| {
                Ok(RetentionPoint {
                    elapsed: Share::of_ratio(table.number(row, "elapsedVideoTimeRatio")?),
                    watch: Share::of_ratio(table.number(row, "audienceWatchRatio")?),
                    relative: table
                        .optional(row, "relativeRetentionPerformance")?
                        .map(Share::of_ratio),
                })
            })
            .collect()
    }
}

/// A report's result table: its column names and rows.
struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

impl Table {
    fn read(body: &str) -> Result<Self, AnalyticsError> {
        let body: Value = serde_json::from_str(body)
            .map_err(|error| unexpected(&format!("unreadable report: {error}")))?;
        let columns = body["columnHeaders"]
            .as_array()
            .ok_or_else(|| unexpected("the report has no column headers"))?
            .iter()
            .map(|header| header["name"].as_str().unwrap_or_default().to_owned())
            .collect();
        let rows = match &body["rows"] {
            Value::Null => Vec::new(),
            Value::Array(rows) => rows
                .iter()
                .map(|row| {
                    row.as_array()
                        .cloned()
                        .ok_or_else(|| unexpected("a report row is not a list"))
                })
                .collect::<Result<_, _>>()?,
            _ => return Err(unexpected("the report's rows are not a list")),
        };
        Ok(Self { columns, rows })
    }

    /// The column's value in `row`, when the report has the column and a
    /// number in it.
    fn optional(&self, row: &[Value], name: &str) -> Result<Option<f64>, AnalyticsError> {
        let Some(ix) = self.columns.iter().position(|column| column == name) else {
            return Ok(None);
        };
        match row.get(ix) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_f64()
                .map(Some)
                .ok_or_else(|| unexpected(&format!("{name} is not a number"))),
        }
    }

    fn number(&self, row: &[Value], name: &str) -> Result<f64, AnalyticsError> {
        self.optional(row, name)?
            .ok_or_else(|| unexpected(&format!("the report has no {name}")))
    }
}

fn unexpected(detail: &str) -> AnalyticsError {
    AnalyticsError::new(AnalyticsErrorKind::Unexpected, detail)
}

/// An error answer as a typed failure, with its reason and message.
fn failure(response: &HttpResponse) -> AnalyticsError {
    let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
    let error = &body["error"];
    let message = error["message"].as_str().unwrap_or_default();
    let reasons: Vec<&str> = error["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["reason"].as_str())
        .collect();
    let limited = reasons.iter().any(|reason| {
        matches!(
            *reason,
            "quotaExceeded" | "rateLimitExceeded" | "userRateLimitExceeded" | "dailyLimitExceeded"
        )
    });
    let kind = match response.status {
        401 => AnalyticsErrorKind::SignedOut,
        403 | 429 if limited => AnalyticsErrorKind::LimitReached,
        403 => AnalyticsErrorKind::Forbidden,
        429 => AnalyticsErrorKind::LimitReached,
        400 => AnalyticsErrorKind::Invalid,
        500 | 502 | 503 | 504 => AnalyticsErrorKind::Unreachable,
        _ => AnalyticsErrorKind::Unexpected,
    };
    let reason = reasons.first().copied().unwrap_or_default();
    let detail = match (reason, message) {
        ("", "") => format!("HTTP {}", response.status),
        (reason, "") => format!("HTTP {}: {reason}", response.status),
        ("", message) => format!("HTTP {}: {message}", response.status),
        (reason, message) => format!("HTTP {}: {reason}: {message}", response.status),
    };
    AnalyticsError::new(kind, detail)
}
