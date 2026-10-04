//! Owner metrics (#79; PRD story 90, ADR-0004): what a channel's owner reads
//! of a post through the connected account, beyond the public statistics.
//! On YouTube that is the Analytics API: per video its views, engaged
//! views, watch time, average view duration and percentage, its retention
//! curve, and, for a channel in the Partner Program, revenue, CPM and
//! playback-based CPM. RPM is not a metric there; it is computed here.
//!
//! - Every number covers the post's whole life up to the report's last
//!   day. The data arrive 48 to 72 hours late, so a snapshot's owner numbers
//!   describe an earlier moment than its public ones.
//! - Engaged views are the headline number when they are there: since
//!   2025-03 a Short's `views` count every start or replay, and since
//!   2026-08 every format's count from the first frame, while engaged views
//!   keep the earlier meaning.
//! - Monetary metrics answer 403 for a channel outside the Partner Program:
//!   that is "not monetized", and the other numbers are read without them.

use std::time::{Duration, SystemTime};

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::{Money, Network, SecretText};

/// A non-negative ratio in ten-thousandths: `Share::ONE` is 1 (100%).
/// Retention watch ratios may pass one (rewatched parts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Share(u32);

impl Share {
    pub const ONE: Share = Share(10_000);

    pub const fn from_ten_thousandths(n: u32) -> Self {
        Share(n)
    }

    pub const fn ten_thousandths(self) -> u32 {
        self.0
    }

    /// `ratio` (1.0 = 100%) to the ten-thousandth; negative or not a
    /// number reads as zero.
    pub fn of_ratio(ratio: f64) -> Self {
        Share(fixed(ratio * 10_000.0) as u32)
    }

    /// `percent` (61.2 = 61.2%).
    pub fn of_percent(percent: f64) -> Self {
        Self::of_ratio(percent / 100.0)
    }

    /// As a percentage, to the hundredth: 6123 reads 61.23.
    pub fn percent(self) -> f64 {
        f64::from(self.0) / 100.0
    }
}

/// A float rounded to a non-negative whole number, clamped to `u32`'s
/// range (counts above that go through `whole`).
fn fixed(value: f64) -> u64 {
    if value.is_nan() || value <= 0.0 {
        0
    } else {
        value.round().min(f64::from(u32::MAX)) as u64
    }
}

/// A count from a report, rounded; negative or not a number reads as zero.
pub fn whole(value: f64) -> u64 {
    if value.is_nan() || value <= 0.0 {
        0
    } else if value >= u64::MAX as f64 {
        u64::MAX
    } else {
        value.round() as u64
    }
}

/// Dollars from a report as money, to the millionth.
pub fn dollars(value: f64) -> Money {
    let micros = whole(value * Money::MICROS_PER_DOLLAR as f64);
    Money::from_micros(micros).min(Money::MAX)
}

/// What a video's report says about money, for a monetized channel. CPM is
/// per thousand ad impressions; playback-based CPM per thousand playbacks
/// that showed an ad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoneyReport {
    pub revenue: Money,
    pub cpm: Money,
    pub playback_cpm: Money,
}

/// One video's report as the network answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VideoReport {
    pub views: u64,
    pub engaged_views: u64,
    pub minutes_watched: u64,
    /// The average playback, in whole seconds.
    pub average_view_seconds: u64,
    /// How much of the video an average playback covers.
    pub average_view_share: Share,
    /// Only when the money metrics were asked for and answered.
    pub money: Option<MoneyReport>,
}

/// Whether the channel earns from its videos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Earnings {
    Monetized(MoneyReport),
    /// Outside the Partner Program: the network refuses money metrics.
    NotMonetized,
}

/// A post's owner numbers at one sync (part of its metrics snapshot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerMetrics {
    pub views: u64,
    pub engaged_views: u64,
    pub minutes_watched: u64,
    pub average_view_seconds: u64,
    pub average_view_share: Share,
    pub earnings: Earnings,
}

impl OwnerMetrics {
    /// The owner numbers of a report, or `None` while it holds nothing:
    /// no view and no minute watched yet, which is what a new post reads
    /// until the data arrive.
    pub fn of(report: &VideoReport) -> Option<Self> {
        if report.views == 0 && report.engaged_views == 0 && report.minutes_watched == 0 {
            return None;
        }
        Some(Self {
            views: report.views,
            engaged_views: report.engaged_views,
            minutes_watched: report.minutes_watched,
            average_view_seconds: report.average_view_seconds,
            average_view_share: report.average_view_share,
            earnings: match report.money {
                Some(money) => Earnings::Monetized(money),
                None => Earnings::NotMonetized,
            },
        })
    }

    /// Revenue per thousand views of the same report (RPM). `None` when
    /// the channel is not monetized or nothing was viewed.
    pub fn rpm(&self) -> Option<Money> {
        let Earnings::Monetized(money) = self.earnings else {
            return None;
        };
        rpm(money.revenue, self.views)
    }

    pub fn money(&self) -> Option<&MoneyReport> {
        match &self.earnings {
            Earnings::Monetized(money) => Some(money),
            Earnings::NotMonetized => None,
        }
    }

    pub fn average_view_duration(&self) -> Duration {
        Duration::from_secs(self.average_view_seconds)
    }
}

/// `revenue` per thousand `views`, to the millionth of a dollar (rounded
/// down). `None` without views.
pub fn rpm(revenue: Money, views: u64) -> Option<Money> {
    if views == 0 {
        return None;
    }
    let micros = u128::from(revenue.micros()) * 1000 / u128::from(views);
    Some(Money::from_micros(u64::try_from(micros).unwrap_or(u64::MAX)).min(Money::MAX))
}

/// One point of a retention curve: at `elapsed` of the video (0 to 1),
/// the share of viewers still watching (`watch`, above one where parts
/// are rewatched) and how that compares with videos of similar length
/// (`relative`: 0.5 is the middle, higher keeps viewers better).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPoint {
    pub elapsed: Share,
    pub watch: Share,
    pub relative: Option<Share>,
}

/// A post's retention curve as last read: its points in order of elapsed
/// time, one per moment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetentionCurve {
    points: Vec<RetentionPoint>,
}

impl RetentionCurve {
    /// The points a report gave, in order: points past the end of the
    /// video and repeats of a moment are dropped, and a relative
    /// performance above one is out of its range and dropped too.
    pub fn of(mut points: Vec<RetentionPoint>) -> Self {
        points.retain(|point| point.elapsed <= Share::ONE);
        for point in &mut points {
            point.relative = point.relative.filter(|relative| *relative <= Share::ONE);
        }
        points.sort_by_key(|point| point.elapsed);
        points.dedup_by_key(|point| point.elapsed);
        Self { points }
    }

    pub fn points(&self) -> &[RetentionPoint] {
        &self.points
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The share still watching at the end, the last point.
    pub fn at_end(&self) -> Option<Share> {
        self.points.last().map(|point| point.watch)
    }

    /// At most `n` points, evenly picked (the first and last kept), for a
    /// chart that has room for fewer than the report's hundred.
    pub fn thinned(&self, n: usize) -> Vec<RetentionPoint> {
        let len = self.points.len();
        if n == 0 {
            return Vec::new();
        }
        if len <= n {
            return self.points.clone();
        }
        if n == 1 {
            return vec![self.points[len - 1]];
        }
        (0..n)
            .map(|i| self.points[i * (len - 1) / (n - 1)])
            .collect()
    }
}

/// The days a report covers: from the day before the post went up (the
/// network keeps its days in its own time zone) to today, in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportPeriod {
    start: Date,
    end: Date,
}

impl ReportPeriod {
    pub fn for_post(posted_at: SystemTime, now: SystemTime) -> Self {
        let day = |at: SystemTime| {
            Timestamp::try_from(at)
                .unwrap_or(Timestamp::UNIX_EPOCH)
                .to_zoned(TimeZone::UTC)
                .date()
        };
        let end = day(now);
        let start = day(posted_at).yesterday().unwrap_or(Date::MIN).min(end);
        Self { start, end }
    }

    /// `YYYY-MM-DD`.
    pub fn start(&self) -> String {
        self.start.to_string()
    }

    /// `YYYY-MM-DD`.
    pub fn end(&self) -> String {
        self.end.to_string()
    }
}

/// Why a report could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnalyticsErrorKind {
    /// The network refuses the report: money metrics outside the Partner
    /// Program, or a video that is not the channel's.
    Forbidden,
    /// The network does not have the post (any more).
    NotFound,
    /// The token was refused.
    SignedOut,
    /// The project's quota or rate limit.
    LimitReached,
    /// No connection, or the network failed.
    Unreachable,
    /// The request was refused as malformed.
    Invalid,
    Unexpected,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{detail}")]
pub struct AnalyticsError {
    pub kind: AnalyticsErrorKind,
    pub detail: String,
}

impl AnalyticsError {
    pub fn new(kind: AnalyticsErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// Whether the other posts of a sync would fail the same way: the
    /// token, the quota or the connection, not this one video.
    pub fn stops_the_account(&self) -> bool {
        matches!(
            self.kind,
            AnalyticsErrorKind::SignedOut
                | AnalyticsErrorKind::LimitReached
                | AnalyticsErrorKind::Unreachable
        )
    }
}

/// Reads owner reports of the connected account's videos (YouTube
/// Analytics, `ids=channel==MINE`).
pub trait OwnerAnalytics: Send + Sync {
    fn network(&self) -> Network;

    /// The video's report over `period`, with the money metrics when
    /// `money`. `None` when the report has no row.
    fn video_report(
        &self,
        token: &SecretText,
        video: &str,
        period: &ReportPeriod,
        money: bool,
    ) -> Result<Option<VideoReport>, AnalyticsError>;

    /// The video's retention curve over `period`, as the report gives it.
    fn retention(
        &self,
        token: &SecretText,
        video: &str,
        period: &ReportPeriod,
    ) -> Result<Vec<RetentionPoint>, AnalyticsError>;
}

/// What a sync knows about a channel's money so far: the first refused
/// money report tells it, and its other videos are read without money.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Monetization {
    #[default]
    Unknown,
    Monetized,
    NotMonetized,
}

/// A video's owner numbers and retention curve, as one sync read them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OwnerReading {
    /// `None` until the network has data for the video.
    pub metrics: Option<OwnerMetrics>,
    pub retention: RetentionCurve,
}

/// Reads one video's owner numbers and retention curve.
///
/// The report asks for money unless the channel is known not to be
/// monetized. A 403 to it marks the channel not monetized and asks again
/// without money; a 403 to that means the video is not the channel's. A
/// retention curve the network refuses or does not have reads as empty;
/// only what stops the account (token, quota, connection) fails the read.
pub fn read_owner_metrics(
    analytics: &dyn OwnerAnalytics,
    token: &SecretText,
    video: &str,
    period: &ReportPeriod,
    monetization: &mut Monetization,
) -> Result<OwnerReading, AnalyticsError> {
    let money = *monetization != Monetization::NotMonetized;
    let report = match analytics.video_report(token, video, period, money) {
        Err(error) if money && error.kind == AnalyticsErrorKind::Forbidden => {
            let report = analytics.video_report(token, video, period, false)?;
            *monetization = Monetization::NotMonetized;
            report
        }
        Ok(report) => {
            if money {
                *monetization = Monetization::Monetized;
            }
            report
        }
        Err(error) => return Err(error),
    };
    let Some(metrics) = report.as_ref().and_then(OwnerMetrics::of) else {
        return Ok(OwnerReading::default());
    };
    let retention = match analytics.retention(token, video, period) {
        Ok(points) => RetentionCurve::of(points),
        Err(error) if error.stops_the_account() => return Err(error),
        Err(_) => RetentionCurve::default(),
    };
    Ok(OwnerReading {
        metrics: Some(metrics),
        retention,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn usd(cents: u64) -> Money {
        Money::from_cents(cents)
    }

    fn report(views: u64, money: Option<MoneyReport>) -> VideoReport {
        VideoReport {
            views,
            engaged_views: views * 9 / 10,
            minutes_watched: views / 2,
            average_view_seconds: 31,
            average_view_share: Share::of_percent(64.5),
            money,
        }
    }

    #[test]
    fn rpm_is_revenue_per_thousand_views() {
        let metrics = OwnerMetrics::of(&report(
            20_000,
            Some(MoneyReport {
                revenue: usd(5_000),
                cpm: usd(600),
                playback_cpm: usd(450),
            }),
        ))
        .unwrap();
        assert_eq!(metrics.rpm(), Some(usd(250)), "$50 over 20K views");
        assert_eq!(
            rpm(Money::from_micros(1), 3),
            Some(Money::from_micros(333)),
            "to the millionth, rounded down"
        );
        assert_eq!(rpm(usd(100), 0), None, "nothing viewed");
        assert_eq!(
            rpm(Money::MAX, 1),
            Some(Money::MAX),
            "clamped like every amount"
        );
    }

    #[test]
    fn a_channel_outside_the_partner_program_has_no_rpm() {
        let metrics = OwnerMetrics::of(&report(1_000, None)).unwrap();
        assert_eq!(metrics.earnings, Earnings::NotMonetized);
        assert_eq!(metrics.rpm(), None);
        assert_eq!(metrics.money(), None);
    }

    #[test]
    fn a_report_with_nothing_in_it_yet_is_no_numbers() {
        assert_eq!(OwnerMetrics::of(&VideoReport::default()), None);
        let watched = VideoReport {
            minutes_watched: 3,
            ..VideoReport::default()
        };
        assert!(OwnerMetrics::of(&watched).is_some());
    }

    #[test]
    fn shares_round_from_ratios_and_percentages() {
        assert_eq!(Share::of_ratio(0.01).ten_thousandths(), 100);
        assert_eq!(Share::of_ratio(1.2345).ten_thousandths(), 12_345);
        assert_eq!(Share::of_percent(61.23).ten_thousandths(), 6_123);
        assert_eq!(Share::of_percent(61.23).percent(), 61.23);
        assert_eq!(Share::of_ratio(-0.5), Share::default());
        assert_eq!(Share::of_ratio(f64::NAN), Share::default());
        assert_eq!(dollars(12.345678), Money::from_micros(12_345_678));
        assert_eq!(dollars(-1.0), Money::ZERO);
        assert_eq!(whole(1234.6), 1235);
    }

    fn point(elapsed: u32, watch: u32, relative: Option<u32>) -> RetentionPoint {
        RetentionPoint {
            elapsed: Share::from_ten_thousandths(elapsed),
            watch: Share::from_ten_thousandths(watch),
            relative: relative.map(Share::from_ten_thousandths),
        }
    }

    #[test]
    fn a_retention_curve_is_ordered_with_one_point_per_moment() {
        let curve = RetentionCurve::of(vec![
            point(300, 8_000, Some(5_500)),
            point(100, 11_000, Some(6_000)),
            point(200, 9_000, None),
            point(200, 1, None),
            point(10_100, 10, None),
            point(10_000, 4_000, Some(12_000)),
        ]);
        let elapsed: Vec<u32> = curve
            .points()
            .iter()
            .map(|p| p.elapsed.ten_thousandths())
            .collect();
        assert_eq!(elapsed, [100, 200, 300, 10_000], "past the end dropped");
        assert_eq!(
            curve.points()[0].watch.ten_thousandths(),
            11_000,
            "rewatched"
        );
        assert_eq!(curve.points()[3].relative, None, "out of range dropped");
        assert_eq!(curve.at_end(), Some(Share::from_ten_thousandths(4_000)));
        assert!(RetentionCurve::of(Vec::new()).is_empty());
    }

    #[test]
    fn a_curve_thins_to_the_points_a_chart_has_room_for() {
        let curve = RetentionCurve::of(
            (1..=100)
                .map(|i| point(i * 100, 10_000 - i * 50, None))
                .collect(),
        );
        let thin = curve.thinned(10);
        assert_eq!(thin.len(), 10);
        assert_eq!(thin[0], curve.points()[0]);
        assert_eq!(thin[9], curve.points()[99]);
        assert_eq!(curve.thinned(200).len(), 100);
        assert_eq!(curve.thinned(1), [curve.points()[99]]);
        assert!(curve.thinned(0).is_empty());
    }

    fn at(rfc3339: &str) -> SystemTime {
        SystemTime::from(rfc3339.parse::<Timestamp>().unwrap())
    }

    #[test]
    fn a_report_covers_the_day_before_the_post_to_today() {
        let period = ReportPeriod::for_post(at("2026-10-01T02:00:00Z"), at("2026-10-03T23:30:00Z"));
        assert_eq!(period.start(), "2026-09-30");
        assert_eq!(period.end(), "2026-10-03");
        let future = ReportPeriod::for_post(at("2026-10-09T12:00:00Z"), at("2026-10-03T12:00:00Z"));
        assert_eq!(future.start(), future.end(), "never after its end");
    }

    /// Answers reports as told and remembers what it was asked.
    #[derive(Default)]
    struct Fake {
        /// A 403 to money metrics.
        not_monetized: bool,
        /// A 403 to every report: not the channel's video.
        foreign: bool,
        report: Option<VideoReport>,
        retention: Option<Result<Vec<RetentionPoint>, AnalyticsError>>,
        failure: Option<AnalyticsError>,
        asked: Mutex<Vec<(String, bool)>>,
        curves: Mutex<usize>,
    }

    fn forbidden() -> AnalyticsError {
        AnalyticsError::new(AnalyticsErrorKind::Forbidden, "HTTP 403: forbidden")
    }

    impl OwnerAnalytics for Fake {
        fn network(&self) -> Network {
            Network::YouTube
        }

        fn video_report(
            &self,
            _: &SecretText,
            video: &str,
            _: &ReportPeriod,
            money: bool,
        ) -> Result<Option<VideoReport>, AnalyticsError> {
            self.asked.lock().unwrap().push((video.to_owned(), money));
            if let Some(failure) = &self.failure {
                return Err(failure.clone());
            }
            if self.foreign || (money && self.not_monetized) {
                return Err(forbidden());
            }
            Ok(self.report.map(|report| VideoReport {
                money: if money {
                    Some(MoneyReport {
                        revenue: usd(321),
                        cpm: usd(410),
                        playback_cpm: usd(380),
                    })
                } else {
                    None
                },
                ..report
            }))
        }

        fn retention(
            &self,
            _: &SecretText,
            _: &str,
            _: &ReportPeriod,
        ) -> Result<Vec<RetentionPoint>, AnalyticsError> {
            *self.curves.lock().unwrap() += 1;
            self.retention
                .clone()
                .unwrap_or_else(|| Ok(vec![point(100, 9_000, Some(5_000))]))
        }
    }

    fn read(fake: &Fake, monetization: &mut Monetization) -> Result<OwnerReading, AnalyticsError> {
        let period = ReportPeriod::for_post(SystemTime::UNIX_EPOCH, SystemTime::UNIX_EPOCH);
        read_owner_metrics(
            fake,
            &SecretText::new("ya29.fake"),
            "dQw4w9WgXcQ",
            &period,
            monetization,
        )
    }

    fn reporting() -> Fake {
        Fake {
            report: Some(report(4_000, None)),
            ..Fake::default()
        }
    }

    #[test]
    fn a_monetized_channel_reads_money_with_the_numbers() {
        let fake = reporting();
        let mut monetization = Monetization::Unknown;
        let reading = read(&fake, &mut monetization).unwrap();
        let metrics = reading.metrics.unwrap();
        assert_eq!(metrics.money().unwrap().revenue, usd(321));
        assert_eq!(metrics.rpm(), Some(Money::from_micros(802_500)));
        assert_eq!(metrics.engaged_views, 3_600);
        assert_eq!(monetization, Monetization::Monetized);
        assert_eq!(reading.retention.points().len(), 1);
        assert_eq!(fake.asked.lock().unwrap().len(), 1, "one report");
    }

    #[test]
    fn a_refused_money_report_is_not_monetized_and_the_rest_goes_on() {
        let fake = Fake {
            not_monetized: true,
            ..reporting()
        };
        let mut monetization = Monetization::Unknown;
        let reading = read(&fake, &mut monetization).unwrap();
        let metrics = reading.metrics.unwrap();
        assert_eq!(metrics.earnings, Earnings::NotMonetized);
        assert_eq!(metrics.views, 4_000, "the other numbers are there");
        assert_eq!(monetization, Monetization::NotMonetized);
        assert_eq!(
            *fake.asked.lock().unwrap(),
            [
                ("dQw4w9WgXcQ".to_owned(), true),
                ("dQw4w9WgXcQ".to_owned(), false)
            ]
        );

        // The channel's next video is read without money at once.
        read(&fake, &mut monetization).unwrap();
        assert_eq!(fake.asked.lock().unwrap().len(), 3);
        assert!(!fake.asked.lock().unwrap()[2].1);
    }

    #[test]
    fn a_video_that_is_not_the_channels_is_refused() {
        let fake = Fake {
            foreign: true,
            ..reporting()
        };
        let mut monetization = Monetization::Unknown;
        let error = read(&fake, &mut monetization).unwrap_err();
        assert_eq!(error.kind, AnalyticsErrorKind::Forbidden);
        assert_eq!(
            monetization,
            Monetization::Unknown,
            "says nothing about the channel's money"
        );
    }

    #[test]
    fn an_empty_report_reads_no_numbers_and_skips_the_curve() {
        let fake = Fake::default();
        let reading = read(&fake, &mut Monetization::Unknown).unwrap();
        assert_eq!(reading, OwnerReading::default());
        assert_eq!(*fake.curves.lock().unwrap(), 0);
    }

    #[test]
    fn a_refused_curve_reads_empty_but_the_quota_stops_the_read() {
        let refused = Fake {
            retention: Some(Err(forbidden())),
            ..reporting()
        };
        let reading = read(&refused, &mut Monetization::Unknown).unwrap();
        assert!(reading.metrics.is_some());
        assert!(reading.retention.is_empty());

        let quota = Fake {
            retention: Some(Err(AnalyticsError::new(
                AnalyticsErrorKind::LimitReached,
                "quotaExceeded",
            ))),
            ..reporting()
        };
        let error = read(&quota, &mut Monetization::Unknown).unwrap_err();
        assert!(error.stops_the_account());

        let signed_out = Fake {
            failure: Some(AnalyticsError::new(AnalyticsErrorKind::SignedOut, "401")),
            ..reporting()
        };
        let mut monetization = Monetization::Unknown;
        assert_eq!(
            read(&signed_out, &mut monetization).unwrap_err().kind,
            AnalyticsErrorKind::SignedOut
        );
        assert_eq!(monetization, Monetization::Unknown);
    }
}
