//! Niche research (ADR-0004): what market data says about a niche, the
//! competition and trend scores computed from it, and how long a result
//! stays fresh in the cache.
//!
//! Adapters return raw samples; everything derived from them (medians,
//! scores, ranking) is computed here, from the sample and its fetch time,
//! so a stored result always shows the same numbers.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{ApiKey, ChannelId, Market, ProfileId, ProviderFailure, RepositoryError};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// A niche or keyword to research, as the user typed it (trimmed). It is
/// the search query sent to the provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Niche {
    label: String,
}

impl Niche {
    pub const MAX_CHARS: usize = 100;

    pub fn new(label: &str) -> Result<Self, NicheSeedError> {
        let label = label.trim();
        if label.is_empty() {
            return Err(NicheSeedError::Required);
        }
        if label.chars().count() > Self::MAX_CHARS {
            return Err(NicheSeedError::TooLong);
        }
        Ok(Self {
            label: label.to_owned(),
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// The cache identity: lowercase with inner whitespace collapsed, so
    /// "Space  History" and "space history" share one result.
    pub fn key(&self) -> String {
        self.label
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
}

/// Why a list of seeds cannot be researched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NicheSeedError {
    Required,
    TooMany,
    TooLong,
}

impl NicheSeedError {
    pub const ALL: [NicheSeedError; 3] = [
        NicheSeedError::Required,
        NicheSeedError::TooMany,
        NicheSeedError::TooLong,
    ];
}

/// The niches a channel researches: at least one, at most `MAX`, without
/// repeats (same `Niche::key`), in the order typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicheSeeds(Vec<Niche>);

impl NicheSeeds {
    /// Each niche costs provider quota (about 1% of YouTube's default
    /// daily quota), so one run is capped.
    pub const MAX: usize = 20;

    /// One niche per entry. Blank entries are dropped and repeats are kept
    /// once, in their first position. Every problem is reported at once.
    pub fn parse<S: AsRef<str>>(entries: &[S]) -> Result<Self, Vec<NicheSeedError>> {
        let mut errors = Vec::new();
        let mut seen = HashSet::new();
        let mut niches = Vec::new();
        for entry in entries {
            match Niche::new(entry.as_ref()) {
                Ok(niche) => {
                    if seen.insert(niche.key()) {
                        niches.push(niche);
                    }
                }
                Err(NicheSeedError::Required) => {}
                Err(error) => {
                    if !errors.contains(&error) {
                        errors.push(error);
                    }
                }
            }
        }
        if niches.is_empty() && errors.is_empty() {
            errors.push(NicheSeedError::Required);
        }
        if niches.len() > Self::MAX {
            errors.push(NicheSeedError::TooMany);
        }
        if errors.is_empty() {
            Ok(Self(niches))
        } else {
            Err(errors)
        }
    }

    pub fn niches(&self) -> &[Niche] {
        &self.0
    }
}

/// One recent upload found for a niche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadSample {
    /// The provider's id of the uploading channel.
    pub channel_id: String,
    pub published_at: SystemTime,
    pub views: u64,
    /// `None` when the channel hides its subscriber count.
    pub channel_subscribers: Option<u64>,
}

/// What market data returned for a niche in a market.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MarketSample {
    /// Uploads matching the niche in the research window, as the provider
    /// estimates it.
    pub upload_volume: u64,
    /// The provider's most relevant uploads of the window, with views and
    /// channel size.
    pub uploads: Vec<UploadSample>,
}

/// Statistics behind the scores, as the screen shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NicheStatistics {
    /// Uploads in the research window (provider estimate).
    pub upload_volume: u64,
    /// Uploads the other numbers are computed from.
    pub sample_size: usize,
    /// Median views of the sampled uploads.
    pub median_views: Option<u64>,
    /// Distinct channels behind the sampled uploads.
    pub channels: usize,
    /// Median subscribers of those channels, among those that show it.
    pub median_subscribers: Option<u64>,
    /// Percent of those channels below `SMALL_CHANNEL_SUBSCRIBERS`.
    pub small_channel_percent: Option<u8>,
    /// View velocity: median views per day since upload.
    pub median_views_per_day: Option<u64>,
}

impl NicheStatistics {
    /// Channels below this size count as small: newcomers who still get
    /// surfaced make a niche easier to enter.
    pub const SMALL_CHANNEL_SUBSCRIBERS: u64 = 10_000;

    /// Computes the statistics of `sample` as of `fetched_at`. An upload
    /// younger than a day counts as one day old, so fresh uploads do not
    /// inflate the velocity.
    pub fn from_sample(sample: &MarketSample, fetched_at: SystemTime) -> Self {
        let views: Vec<u64> = sample.uploads.iter().map(|upload| upload.views).collect();
        let views_per_day: Vec<u64> = sample
            .uploads
            .iter()
            .map(|upload| {
                let age = fetched_at
                    .duration_since(upload.published_at)
                    .unwrap_or_default();
                let days = (age.as_secs_f64() / DAY.as_secs_f64()).max(1.0);
                (upload.views as f64 / days) as u64
            })
            .collect();

        let mut subscribers_by_channel: HashMap<&str, Option<u64>> = HashMap::new();
        for upload in &sample.uploads {
            subscribers_by_channel
                .entry(upload.channel_id.as_str())
                .or_insert(upload.channel_subscribers);
        }
        let subscribers: Vec<u64> = subscribers_by_channel.values().flatten().copied().collect();
        let small_channel_percent = (!subscribers.is_empty()).then(|| {
            let small = subscribers
                .iter()
                .filter(|&&count| count < Self::SMALL_CHANNEL_SUBSCRIBERS)
                .count();
            (small * 100 / subscribers.len()) as u8
        });

        Self {
            upload_volume: sample.upload_volume,
            sample_size: sample.uploads.len(),
            median_views: median(views),
            channels: subscribers_by_channel.len(),
            median_subscribers: median(subscribers),
            small_channel_percent,
            median_views_per_day: median(views_per_day),
        }
    }
}

/// The lower middle value for an even count, so the median is always one
/// of the observed numbers.
fn median(mut values: Vec<u64>) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[(values.len() - 1) / 2])
}

/// A score from 0 to 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Score(u8);

impl Score {
    pub const MAX: Score = Score(100);

    /// Values above 100 are clamped.
    pub fn new(value: u8) -> Self {
        Self(value.min(100))
    }

    pub fn value(self) -> u8 {
        self.0
    }

    /// From a fraction between 0 and 1, rounded to the nearest point.
    fn from_fraction(fraction: f64) -> Self {
        Self((fraction.clamp(0.0, 1.0) * 100.0).round() as u8)
    }
}

/// Competition and trend of a niche, and the opportunity they rank by.
///
/// Each input goes through a log scale between two anchors (0 at the low
/// anchor or below, 1 at the high anchor or above), because these numbers
/// span orders of magnitude: 1,000 versus 10,000 uploads matters as much as
/// 10,000 versus 100,000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NicheScores {
    /// How hard it is to stand out: many uploads, big channels and few
    /// views per upload push it up. Higher is harder.
    pub competition: Score,
    /// How fast recent uploads gather views. Higher is hotter.
    pub trend: Score,
}

impl NicheScores {
    /// Upload volume anchors (uploads in the window).
    pub const SUPPLY_ANCHORS: (u64, u64) = (10, 100_000);
    /// Median subscriber anchors of the uploading channels.
    pub const CHANNEL_SIZE_ANCHORS: (u64, u64) = (1_000, 10_000_000);
    /// Median views anchors; more views per upload means room for more.
    pub const DEMAND_ANCHORS: (u64, u64) = (100, 1_000_000);
    /// Median views-per-day anchors.
    pub const VELOCITY_ANCHORS: (u64, u64) = (1, 100_000);

    /// Competition weights: supply, incumbency (channel size and how few
    /// small channels get surfaced) and saturation (low demand).
    const SUPPLY_WEIGHT: f64 = 0.4;
    const INCUMBENCY_WEIGHT: f64 = 0.4;
    const SATURATION_WEIGHT: f64 = 0.2;

    /// Scores the statistics, or `None` when the sample is empty: with no
    /// upload to look at, a score would be invented.
    pub fn from_statistics(stats: &NicheStatistics) -> Option<Self> {
        if stats.sample_size == 0 {
            return None;
        }
        let supply = log_scale(stats.upload_volume, Self::SUPPLY_ANCHORS);
        // Unknown channel sizes (all hidden) count as middling.
        let size = stats.median_subscribers.map_or(0.5, |subscribers| {
            log_scale(subscribers, Self::CHANNEL_SIZE_ANCHORS)
        });
        let closed = stats
            .small_channel_percent
            .map_or(0.5, |percent| 1.0 - f64::from(percent) / 100.0);
        let incumbency = (size + closed) / 2.0;
        let saturation = 1.0 - log_scale(stats.median_views.unwrap_or(0), Self::DEMAND_ANCHORS);
        let competition = Self::SUPPLY_WEIGHT * supply
            + Self::INCUMBENCY_WEIGHT * incumbency
            + Self::SATURATION_WEIGHT * saturation;

        let trend = log_scale(
            stats.median_views_per_day.unwrap_or(0),
            Self::VELOCITY_ANCHORS,
        );

        Some(Self {
            competition: Score::from_fraction(competition),
            trend: Score::from_fraction(trend),
        })
    }

    /// The ranking key: the average of trend and lack of competition.
    pub fn opportunity(&self) -> Score {
        let open = 100 - u16::from(self.competition.value());
        let sum = u16::from(self.trend.value()) + open;
        Score::new(sum.div_ceil(2) as u8)
    }
}

/// Where `value` sits between the anchors on a log scale, from 0 to 1.
fn log_scale(value: u64, (low, high): (u64, u64)) -> f64 {
    let value = (value.max(1) as f64).log10();
    let (low, high) = ((low as f64).log10(), (high as f64).log10());
    ((value - low) / (high - low)).clamp(0.0, 1.0)
}

/// Market data fetched for a niche: the cache entry, and what the research
/// screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicheResearch {
    pub owner: ProfileId,
    pub niche: Niche,
    pub market: Market,
    pub fetched_at: SystemTime,
    pub sample: MarketSample,
}

impl NicheResearch {
    /// How far back "recent uploads" reach.
    pub const RECENT_WINDOW: Duration = Duration::from_secs(30 * 24 * 60 * 60);
    /// How long a result is reused before research fetches it again. The
    /// numbers cover a month of uploads, so a week-old result still tells
    /// the story, and the user can refresh any time.
    pub const CACHE_WINDOW: Duration = Duration::from_secs(7 * 24 * 60 * 60);

    /// Whether research may reuse this result at `now` instead of calling
    /// the provider. A fetch time in the future (the clock moved back)
    /// counts as fresh.
    pub fn is_fresh(&self, now: SystemTime) -> bool {
        match now.duration_since(self.fetched_at) {
            Ok(age) => age < Self::CACHE_WINDOW,
            Err(_) => true,
        }
    }

    pub fn statistics(&self) -> NicheStatistics {
        NicheStatistics::from_sample(&self.sample, self.fetched_at)
    }

    pub fn scores(&self) -> Option<NicheScores> {
        NicheScores::from_statistics(&self.statistics())
    }
}

/// Orders research results for the screen: highest opportunity first,
/// results without scores last, ties by niche name.
pub fn rank(results: &mut [NicheResearch]) {
    results.sort_by_cached_key(|result| {
        let opportunity = result.scores().map(|scores| scores.opportunity());
        (
            std::cmp::Reverse(opportunity),
            result.niche.label().to_lowercase(),
        )
    });
}

/// Finds recent uploads for a niche. Calls the network and blocks, so it
/// runs inside a job.
pub trait MarketData: Send + Sync {
    /// Uploads matching `niche` in `market`, published since `since`.
    fn recent_uploads(
        &self,
        key: &ApiKey,
        niche: &Niche,
        market: Market,
        since: SystemTime,
    ) -> Result<MarketSample, ProviderFailure>;

    /// Provider quota one `recent_uploads` call spends, so the screen can
    /// say what a run costs.
    fn quota_units_per_niche(&self) -> u32;
}

impl<T: MarketData + ?Sized> MarketData for Arc<T> {
    fn recent_uploads(
        &self,
        key: &ApiKey,
        niche: &Niche,
        market: Market,
        since: SystemTime,
    ) -> Result<MarketSample, ProviderFailure> {
        (**self).recent_uploads(key, niche, market, since)
    }

    fn quota_units_per_niche(&self) -> u32 {
        (**self).quota_units_per_niche()
    }
}

/// Persistence port for research results (the cache) and each channel's
/// seed niches. Shared with job worker threads.
pub trait NicheResearchRepository: Send + Sync {
    /// The stored result for the niche (by `Niche::key`) in the market.
    fn cached(
        &self,
        owner: ProfileId,
        niche: &Niche,
        market: Market,
    ) -> Result<Option<NicheResearch>, RepositoryError>;

    /// Stores the result, replacing any earlier one for the same niche and
    /// market.
    fn save(&self, research: &NicheResearch) -> Result<(), RepositoryError>;

    /// The channel's seeds from its last research run, in order.
    fn seeds(&self, channel: ChannelId) -> Result<Vec<Niche>, RepositoryError>;

    fn set_seeds(&self, channel: ChannelId, seeds: &NicheSeeds) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentLanguage, Country};

    fn fetched_at() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    fn upload(channel: &str, days_old: u64, views: u64, subscribers: Option<u64>) -> UploadSample {
        UploadSample {
            channel_id: channel.into(),
            published_at: fetched_at() - DAY * days_old as u32,
            views,
            channel_subscribers: subscribers,
        }
    }

    fn stats() -> NicheStatistics {
        NicheStatistics {
            upload_volume: 1_000,
            sample_size: 50,
            median_views: Some(10_000),
            channels: 40,
            median_subscribers: Some(100_000),
            small_channel_percent: Some(50),
            median_views_per_day: Some(1_000),
        }
    }

    fn scores(stats: NicheStatistics) -> NicheScores {
        NicheScores::from_statistics(&stats).unwrap()
    }

    #[test]
    fn a_niche_is_trimmed_and_keyed_ignoring_case_and_spacing() {
        let niche = Niche::new("  Space   History\t").unwrap();
        assert_eq!(niche.label(), "Space   History");
        assert_eq!(niche.key(), "space history");
        assert_eq!(Niche::new("space history").unwrap().key(), niche.key());
    }

    #[test]
    fn a_niche_needs_text_within_the_limit_in_characters() {
        assert_eq!(Niche::new("  "), Err(NicheSeedError::Required));
        assert!(Niche::new(&"é".repeat(Niche::MAX_CHARS)).is_ok());
        assert_eq!(
            Niche::new(&"é".repeat(Niche::MAX_CHARS + 1)),
            Err(NicheSeedError::TooLong)
        );
    }

    #[test]
    fn seeds_drop_blank_lines_and_repeats_keeping_the_first() {
        let seeds = NicheSeeds::parse(&["Space history", "", "  ", "true crime", "SPACE  history"])
            .unwrap();
        let labels: Vec<_> = seeds.niches().iter().map(Niche::label).collect();
        assert_eq!(labels, ["Space history", "true crime"]);
    }

    #[test]
    fn seeds_need_at_least_one_niche() {
        assert_eq!(
            NicheSeeds::parse::<&str>(&[]),
            Err(vec![NicheSeedError::Required])
        );
        assert_eq!(
            NicheSeeds::parse(&["", " "]),
            Err(vec![NicheSeedError::Required])
        );
    }

    #[test]
    fn seeds_are_capped_after_removing_repeats() {
        let distinct: Vec<String> = (0..=NicheSeeds::MAX)
            .map(|i| format!("niche {i}"))
            .collect();
        assert_eq!(
            NicheSeeds::parse(&distinct),
            Err(vec![NicheSeedError::TooMany])
        );
        let repeated = vec!["same"; NicheSeeds::MAX + 5];
        assert!(NicheSeeds::parse(&repeated).is_ok());
    }

    #[test]
    fn every_seed_problem_is_reported_once() {
        let long = "x".repeat(Niche::MAX_CHARS + 1);
        let mut entries: Vec<String> = (0..=NicheSeeds::MAX).map(|i| format!("n{i}")).collect();
        entries.push(long.clone());
        entries.push(long);
        assert_eq!(
            NicheSeeds::parse(&entries),
            Err(vec![NicheSeedError::TooLong, NicheSeedError::TooMany])
        );
    }

    #[test]
    fn statistics_take_medians_of_the_sample() {
        let sample = MarketSample {
            upload_volume: 1_234,
            uploads: vec![
                upload("a", 10, 1_000, Some(500)),
                upload("b", 10, 3_000, Some(50_000)),
                upload("c", 10, 2_000, Some(2_000_000)),
                upload("d", 10, 9_000, Some(8_000)),
            ],
        };
        let stats = NicheStatistics::from_sample(&sample, fetched_at());
        assert_eq!(stats.upload_volume, 1_234);
        assert_eq!(stats.sample_size, 4);
        assert_eq!(stats.median_views, Some(2_000), "lower middle of 4");
        assert_eq!(stats.channels, 4);
        assert_eq!(stats.median_subscribers, Some(8_000));
        assert_eq!(stats.small_channel_percent, Some(50));
        assert_eq!(stats.median_views_per_day, Some(200));
    }

    #[test]
    fn a_channel_with_several_uploads_counts_once_for_its_size() {
        let sample = MarketSample {
            upload_volume: 3,
            uploads: vec![
                upload("big", 5, 100, Some(5_000_000)),
                upload("big", 6, 100, Some(5_000_000)),
                upload("small", 7, 100, Some(1_000)),
            ],
        };
        let stats = NicheStatistics::from_sample(&sample, fetched_at());
        assert_eq!(stats.channels, 2);
        assert_eq!(stats.small_channel_percent, Some(50));
        assert_eq!(stats.median_subscribers, Some(1_000));
    }

    #[test]
    fn hidden_subscriber_counts_are_left_out_of_channel_size() {
        let sample = MarketSample {
            upload_volume: 2,
            uploads: vec![
                upload("hidden", 3, 100, None),
                upload("known", 3, 100, Some(20_000)),
            ],
        };
        let stats = NicheStatistics::from_sample(&sample, fetched_at());
        assert_eq!(stats.channels, 2);
        assert_eq!(stats.median_subscribers, Some(20_000));
        assert_eq!(stats.small_channel_percent, Some(0));

        let all_hidden = MarketSample {
            upload_volume: 1,
            uploads: vec![upload("hidden", 3, 100, None)],
        };
        let stats = NicheStatistics::from_sample(&all_hidden, fetched_at());
        assert_eq!(stats.median_subscribers, None);
        assert_eq!(stats.small_channel_percent, None);
    }

    #[test]
    fn velocity_counts_uploads_younger_than_a_day_as_one_day_old() {
        let mut fresh = upload("a", 0, 5_000, None);
        fresh.published_at = fetched_at() - Duration::from_secs(3600);
        let sample = MarketSample {
            upload_volume: 1,
            uploads: vec![fresh],
        };
        let stats = NicheStatistics::from_sample(&sample, fetched_at());
        assert_eq!(stats.median_views_per_day, Some(5_000));
    }

    #[test]
    fn statistics_are_relative_to_the_fetch_time_not_the_clock() {
        let sample = MarketSample {
            upload_volume: 1,
            uploads: vec![upload("a", 4, 400, None)],
        };
        let at_fetch = NicheStatistics::from_sample(&sample, fetched_at());
        let later = NicheStatistics::from_sample(&sample, fetched_at() + DAY * 4);
        assert_eq!(at_fetch.median_views_per_day, Some(100));
        assert_eq!(later.median_views_per_day, Some(50));
    }

    #[test]
    fn an_empty_sample_has_no_scores() {
        let stats = NicheStatistics::from_sample(&MarketSample::default(), fetched_at());
        assert_eq!(stats.sample_size, 0);
        assert_eq!(stats.median_views, None);
        assert_eq!(NicheScores::from_statistics(&stats), None);
    }

    #[test]
    fn scores_for_fixed_statistics() {
        // supply log10(1000)=3 → (3-1)/4 = 0.5
        // size log10(100k)=5 → (5-3)/4 = 0.5; closed 1-0.5 = 0.5; incumbency 0.5
        // demand log10(10k)=4 → (4-2)/4 = 0.5; saturation 0.5
        // velocity log10(1000)=3 → 3/5 = 0.6
        assert_eq!(
            scores(stats()),
            NicheScores {
                competition: Score::new(50),
                trend: Score::new(60),
            }
        );
    }

    #[test]
    fn the_emptiest_niche_scores_lowest_competition() {
        let open = NicheStatistics {
            upload_volume: 3,
            median_views: Some(5_000_000),
            median_subscribers: Some(10),
            small_channel_percent: Some(100),
            ..stats()
        };
        assert_eq!(scores(open).competition, Score::new(0));
    }

    #[test]
    fn the_most_crowded_niche_scores_highest_competition() {
        let crowded = NicheStatistics {
            upload_volume: 1_000_000,
            median_views: Some(3),
            median_subscribers: Some(50_000_000),
            small_channel_percent: Some(0),
            ..stats()
        };
        assert_eq!(scores(crowded).competition, Score::MAX);
    }

    #[test]
    fn competition_grows_with_uploads_and_channel_size() {
        let base = scores(stats()).competition;
        let more_uploads = NicheStatistics {
            upload_volume: 50_000,
            ..stats()
        };
        let bigger_channels = NicheStatistics {
            median_subscribers: Some(2_000_000),
            ..stats()
        };
        let fewer_small = NicheStatistics {
            small_channel_percent: Some(10),
            ..stats()
        };
        assert!(scores(more_uploads).competition > base);
        assert!(scores(bigger_channels).competition > base);
        assert!(scores(fewer_small).competition > base);
    }

    #[test]
    fn more_views_per_upload_lowers_competition() {
        let base = scores(stats()).competition;
        let more_demand = NicheStatistics {
            median_views: Some(500_000),
            ..stats()
        };
        assert!(scores(more_demand).competition < base);
    }

    #[test]
    fn unknown_channel_sizes_count_as_middling() {
        let hidden = NicheStatistics {
            median_subscribers: None,
            small_channel_percent: None,
            ..stats()
        };
        assert_eq!(scores(hidden), scores(stats()));
    }

    #[test]
    fn trend_follows_view_velocity() {
        let cold = NicheStatistics {
            median_views_per_day: Some(0),
            ..stats()
        };
        let hot = NicheStatistics {
            median_views_per_day: Some(250_000),
            ..stats()
        };
        assert_eq!(scores(cold).trend, Score::new(0));
        assert_eq!(scores(hot).trend, Score::MAX);
        assert!(scores(stats()).trend > scores(cold).trend);
    }

    #[test]
    fn opportunity_averages_trend_and_open_space() {
        let s = |competition, trend| NicheScores {
            competition: Score::new(competition),
            trend: Score::new(trend),
        };
        assert_eq!(s(50, 60).opportunity(), Score::new(55));
        assert_eq!(s(0, 100).opportunity(), Score::MAX);
        assert_eq!(s(100, 0).opportunity(), Score::new(0));
        assert_eq!(s(40, 61).opportunity(), Score::new(61), "rounds up");
    }

    #[test]
    fn scores_never_exceed_100() {
        assert_eq!(Score::new(250), Score::MAX);
    }

    fn research(label: &str, uploads: Vec<UploadSample>) -> NicheResearch {
        NicheResearch {
            owner: ProfileId::new(),
            niche: Niche::new(label).unwrap(),
            market: Market::new(Country::UnitedStates, ContentLanguage::English),
            fetched_at: fetched_at(),
            sample: MarketSample {
                upload_volume: 100,
                uploads,
            },
        }
    }

    #[test]
    fn results_rank_by_opportunity_with_unscored_last() {
        let hot = research("b hot", vec![upload("a", 2, 400_000, Some(500))]);
        let cold = research("a cold", vec![upload("a", 20, 20, Some(5_000_000))]);
        let empty = research("c empty", vec![]);
        let tie = research("A hot twin", vec![upload("a", 2, 400_000, Some(500))]);
        let mut results = vec![empty, cold, hot, tie];

        rank(&mut results);

        let order: Vec<_> = results.iter().map(|r| r.niche.label()).collect();
        assert_eq!(order, ["A hot twin", "b hot", "a cold", "c empty"]);
    }

    #[test]
    fn a_result_is_fresh_within_the_cache_window() {
        let result = research("space", vec![]);
        let at = result.fetched_at;
        assert!(result.is_fresh(at));
        assert!(result.is_fresh(at + NicheResearch::CACHE_WINDOW - Duration::from_secs(1)));
        assert!(!result.is_fresh(at + NicheResearch::CACHE_WINDOW));
        assert!(
            result.is_fresh(at - DAY),
            "a clock moved back does not refetch"
        );
    }
}
