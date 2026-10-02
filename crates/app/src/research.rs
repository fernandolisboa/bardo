//! Niche research use cases: research a channel's seed niches in a job and
//! show the ranked results with the numbers behind each score (ADR-0004).
//!
//! Results are cached per niche and market. A run reuses a result younger
//! than `NicheResearch::CACHE_WINDOW` without calling the provider, unless
//! the user asks for a refresh.

use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, Channel, ChannelId, ContentLanguage, Country, Job, JobFailure, JobFailureKind, JobId,
    JobKind, Market, MarketData, Niche, NicheResearch, NicheResearchRepository, NicheScores,
    NicheSeedError, NicheSeeds, NicheStatistics, ProfileId, Progress, Provider, RepositoryError,
    SecretStore, rank,
};
use serde::{Deserialize, Serialize};

use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, KeyState, Text};

#[derive(Debug, thiserror::Error)]
pub enum ResearchError {
    /// The seeds break one or more rules; the screen shows each one.
    #[error("invalid seeds: {0:?}")]
    Invalid(Vec<NicheSeedError>),
    #[error("channel not found")]
    ChannelNotFound,
    /// Research would call YouTube, and no key is saved for it.
    #[error("no YouTube Data API key saved")]
    MissingKey,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ResearchError {
    /// What the research screen says.
    pub fn message(&self) -> Text {
        match self {
            ResearchError::Invalid(errors) => {
                errors.first().map_or(Text::ResearchNotStarted, |error| {
                    Text::NicheSeedError(*error)
                })
            }
            ResearchError::ChannelNotFound => Text::ChannelNotFound,
            ResearchError::MissingKey => Text::ResearchMissingKey,
            ResearchError::Repository(_) => Text::ResearchNotStarted,
        }
    }

    pub fn seed_errors(&self) -> &[NicheSeedError] {
        match self {
            ResearchError::Invalid(errors) => errors,
            _ => &[],
        }
    }
}

/// A fetched result, with what the screen shows of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicheResult {
    pub fetched_at: SystemTime,
    pub statistics: NicheStatistics,
    /// `None` when no recent upload was found.
    pub scores: Option<NicheScores>,
    /// Whether a run would reuse it instead of fetching again.
    pub fresh: bool,
}

/// One seed niche of the channel and its result, if it was researched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicheRow {
    pub niche: Niche,
    pub result: Option<NicheResult>,
}

/// The research screen for one channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicheResearchView {
    pub market: Market,
    /// The seeds to show in the editor: the last run's, or the channel's
    /// niche before any run.
    pub seeds: Vec<Niche>,
    /// Researched seeds first, best opportunity on top; then seeds without
    /// a result yet, in seed order.
    pub rows: Vec<NicheRow>,
    /// The channel's latest research job, to show its progress or why it
    /// stopped.
    pub job: Option<Job>,
}

/// The research job's payload: the market and niches it fetches, fixed at
/// the time the user started it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ResearchPayload {
    channel: String,
    country: String,
    language: String,
    niches: Vec<String>,
    /// Fetch every niche even when a fresh result is cached.
    refresh: bool,
}

impl ResearchPayload {
    fn new(channel: &Channel, seeds: &NicheSeeds, refresh: bool) -> Self {
        let market = channel.details.market();
        Self {
            channel: channel.id.to_string(),
            country: market.country.code().to_owned(),
            language: market.language.code().to_owned(),
            niches: seeds
                .niches()
                .iter()
                .map(|niche| niche.label().to_owned())
                .collect(),
            refresh,
        }
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a research payload serializes")
    }

    fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid research payload: {e}")))
    }

    fn market(&self) -> Result<Market, JobFailure> {
        let country: Country = self.country.parse().map_err(unexpected)?;
        let language: ContentLanguage = self.language.parse().map_err(unexpected)?;
        Ok(Market::new(country, language))
    }

    fn niches(&self) -> Result<Vec<Niche>, JobFailure> {
        self.niches
            .iter()
            .map(|label| {
                Niche::new(label)
                    .map_err(|e| JobFailure::unexpected(format!("invalid niche {label:?}: {e:?}")))
            })
            .collect()
    }
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// Niches a run would fetch: all of them on refresh, otherwise those
/// without a fresh cached result.
fn to_fetch(
    research: &dyn NicheResearchRepository,
    owner: ProfileId,
    niches: &[Niche],
    market: Market,
    refresh: bool,
    now: SystemTime,
) -> Result<usize, RepositoryError> {
    if refresh {
        return Ok(niches.len());
    }
    let mut count = 0;
    for niche in niches {
        let fresh = research
            .cached(owner, niche, market)?
            .is_some_and(|result| result.is_fresh(now));
        if !fresh {
            count += 1;
        }
    }
    Ok(count)
}

/// Runs research jobs: one checkpoint per niche, so a resumed job skips
/// the niches already fetched.
pub(crate) struct NicheResearchHandler {
    pub(crate) owner: ProfileId,
    pub(crate) research: Arc<dyn NicheResearchRepository>,
    pub(crate) market_data: Arc<dyn MarketData>,
    pub(crate) secrets: Arc<dyn SecretStore>,
}

impl NicheResearchHandler {
    /// Read when the first niche needs fetching, so a run served entirely
    /// from the cache works without a key.
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::YouTubeData)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(
                    JobFailureKind::MissingKey,
                    "no YouTube Data API key is saved",
                )
            })
    }
}

impl JobHandler for NicheResearchHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload = ResearchPayload::parse(payload)?;
        let market = payload.market()?;
        let niches = payload.niches()?;
        let done: usize = match cx.checkpoint() {
            Some(checkpoint) => checkpoint.parse().map_err(|e| {
                JobFailure::unexpected(format!("invalid checkpoint {checkpoint:?}: {e}"))
            })?,
            None => 0,
        };
        let total = niches.len() as u64;
        let mut key = None;

        for (index, niche) in niches.iter().enumerate().skip(done) {
            if cx.should_stop() {
                return Ok(());
            }
            let now = SystemTime::now();
            let cached = self
                .research
                .cached(self.owner, niche, market)
                .map_err(unexpected)?;
            let reuse = !payload.refresh && cached.is_some_and(|result| result.is_fresh(now));
            if !reuse {
                if key.is_none() {
                    key = Some(self.key()?);
                }
                let key = key.as_ref().expect("read above");
                let since = now - NicheResearch::RECENT_WINDOW;
                let sample = self
                    .market_data
                    .recent_uploads(key, niche, market, since)
                    .map_err(|failure| {
                        JobFailure::new(
                            failure.kind.into(),
                            format!("{}: {}", niche.label(), failure.detail),
                        )
                    })?;
                let result = NicheResearch {
                    owner: self.owner,
                    niche: niche.clone(),
                    market,
                    fetched_at: now,
                    sample,
                };
                self.research.save(&result).map_err(unexpected)?;
            }
            let fetched = index as u64 + 1;
            cx.save_checkpoint(fetched.to_string(), Progress::of(fetched, total))
                .map_err(unexpected)?;
        }
        Ok(())
    }
}

impl Bardo {
    fn own_channel(&self, id: ChannelId) -> Result<Channel, ResearchError> {
        self.channels
            .get(id)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(ResearchError::ChannelNotFound)
    }

    /// The channel's seeds and their results, ranked, with its latest
    /// research job.
    pub fn niche_research(&self, channel: ChannelId) -> Result<NicheResearchView, ResearchError> {
        let channel = self.own_channel(channel)?;
        let market = channel.details.market();
        let mut seeds = self.research.seeds(channel.id)?;
        if seeds.is_empty()
            && let Ok(niche) = Niche::new(channel.details.niche())
        {
            seeds.push(niche);
        }

        let now = SystemTime::now();
        let mut researched = Vec::new();
        let mut pending = Vec::new();
        for niche in &seeds {
            match self.research.cached(self.profile.id, niche, market)? {
                Some(result) => researched.push(result),
                None => pending.push(NicheRow {
                    niche: niche.clone(),
                    result: None,
                }),
            }
        }
        rank(&mut researched);
        let mut rows: Vec<NicheRow> = researched
            .into_iter()
            .map(|result| NicheRow {
                result: Some(NicheResult {
                    fetched_at: result.fetched_at,
                    statistics: result.statistics(),
                    scores: result.scores(),
                    fresh: result.is_fresh(now),
                }),
                // The seed as typed now, not as typed when fetched.
                niche: seeds
                    .iter()
                    .find(|seed| seed.key() == result.niche.key())
                    .cloned()
                    .unwrap_or(result.niche),
            })
            .collect();
        rows.extend(pending);

        Ok(NicheResearchView {
            market,
            seeds,
            rows,
            job: self.latest_research_job(channel.id),
        })
    }

    fn latest_research_job(&self, channel: ChannelId) -> Option<Job> {
        let channel = channel.to_string();
        self.jobs().into_iter().rev().find(|job| {
            job.kind() == JobKind::NicheResearch
                && ResearchPayload::parse(job.payload()).is_ok_and(|p| p.channel == channel)
        })
    }

    /// Provider quota units running research on `entries` (one niche per
    /// entry) would spend: only niches without a fresh result cost, unless
    /// `refresh` fetches all.
    pub fn research_cost<S: AsRef<str>>(
        &self,
        channel: ChannelId,
        entries: &[S],
        refresh: bool,
    ) -> Result<u32, ResearchError> {
        let channel = self.own_channel(channel)?;
        let seeds = NicheSeeds::parse(entries).map_err(ResearchError::Invalid)?;
        let fetches = to_fetch(
            &*self.research,
            self.profile.id,
            seeds.niches(),
            channel.details.market(),
            refresh,
            SystemTime::now(),
        )?;
        Ok(fetches as u32 * self.market_data.quota_units_per_niche())
    }

    /// Saves `entries` (one niche per entry) as the channel's seeds and
    /// starts a research job for them in the channel's market. `refresh`
    /// fetches every niche again, ignoring the cache.
    pub fn run_niche_research<S: AsRef<str>>(
        &self,
        channel: ChannelId,
        entries: &[S],
        refresh: bool,
    ) -> Result<JobId, ResearchError> {
        let channel = self.own_channel(channel)?;
        let seeds = NicheSeeds::parse(entries).map_err(ResearchError::Invalid)?;
        let fetches = to_fetch(
            &*self.research,
            self.profile.id,
            seeds.niches(),
            channel.details.market(),
            refresh,
            SystemTime::now(),
        )?;
        if fetches > 0 && self.provider_key(Provider::YouTubeData).state == KeyState::NotSet {
            return Err(ResearchError::MissingKey);
        }
        self.research.set_seeds(channel.id, &seeds)?;
        let payload = ResearchPayload::new(&channel, &seeds, refresh);
        let job = Job::new(self.profile.id, JobKind::NicheResearch, payload.to_json());
        Ok(self.jobs.enqueue(job)?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use bardo_domain::{
        ChannelDraft, JobRepository, JobState, MarketSample, ProviderFailure, ProviderFailureKind,
        Score,
    };
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::testing::{FakeMarketData, providers_with};
    use crate::{JobSettings, Repositories};

    const KEY: &str = "AIzaSyTestKey0001abcdefghij";
    const PATIENCE: Duration = Duration::from_secs(10);

    struct Harness {
        db: Arc<Database>,
        market: Arc<FakeMarketData>,
        secrets: Arc<MemorySecretStore>,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                db: Arc::new(Database::open_in_memory().unwrap()),
                market: Arc::new(FakeMarketData::default()),
                secrets: Arc::new(MemorySecretStore::default()),
            }
        }

        fn on_disk(
            path: &std::path::Path,
            market: Arc<FakeMarketData>,
            secrets: Arc<MemorySecretStore>,
        ) -> Self {
            Self {
                db: Arc::new(Database::open(path).unwrap()),
                market,
                secrets,
            }
        }

        fn start(&self) -> Bardo {
            let repositories = Repositories {
                profiles: Box::new(Arc::clone(&self.db)),
                channels: Box::new(Arc::clone(&self.db)),
                jobs: Arc::clone(&self.db) as Arc<dyn JobRepository>,
                themes: Arc::clone(&self.db) as _,
                templates: Arc::clone(&self.db) as _,
                scripts: Arc::clone(&self.db) as _,
                personas: Arc::clone(&self.db) as _,
                narrations: Arc::clone(&self.db) as _,
                scene_plans: Arc::clone(&self.db) as _,
                timelines: Arc::clone(&self.db) as _,
                media_assets: Arc::clone(&self.db) as _,
                music_prompts: Arc::clone(&self.db) as _,
                network_accounts: Arc::clone(&self.db) as _,
                renders: Arc::clone(&self.db) as _,
                exports: Arc::clone(&self.db) as _,
                publications: Arc::clone(&self.db) as _,
                export_files: Arc::new(bardo_storage::MemoryExportFiles::default()),
                costs: Arc::clone(&self.db) as _,
                files: Arc::new(bardo_storage::MemoryProjectFiles::default()),
                research: Arc::clone(&self.db) as _,
                secrets: Arc::clone(&self.secrets) as _,
            };
            Bardo::start_with(
                repositories,
                providers_with(Arc::clone(&self.market)),
                Some("en-US"),
                JobSettings {
                    retry: bardo_domain::RetryPolicy {
                        max_attempts: 2,
                        first_delay: Duration::from_millis(20),
                        max_delay: Duration::from_millis(20),
                    },
                    ..JobSettings::default()
                },
            )
            .unwrap()
        }

        /// Started with a YouTube key saved.
        fn start_with_key(&self) -> Bardo {
            let mut app = self.start();
            app.save_provider_key(Provider::YouTubeData, KEY).unwrap();
            app
        }
    }

    fn channel(app: &Bardo, niche: &str) -> Channel {
        app.create_channel(ChannelDraft {
            name: "Space Archives".into(),
            niche: niche.into(),
            language: ContentLanguage::Portuguese,
            country: Country::Brazil,
            ..ChannelDraft::default()
        })
        .unwrap()
    }

    fn wait_done(app: &Bardo, id: JobId) -> Job {
        wait_for(app, id, |job| !job.state().is_active())
    }

    fn wait_for(app: &Bardo, id: JobId, matches: impl Fn(&Job) -> bool) -> Job {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(job) = app
                .jobs()
                .into_iter()
                .find(|j| j.id() == id)
                .filter(&matches)
            {
                return job;
            }
            assert!(Instant::now() < deadline, "job {id} never matched");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn labels(view: &NicheResearchView) -> Vec<&str> {
        view.rows.iter().map(|row| row.niche.label()).collect()
    }

    #[test]
    fn before_any_run_the_channel_niche_is_the_only_seed() {
        let h = Harness::new();
        let app = h.start();
        let channel = channel(&app, "space history");

        let view = app.niche_research(channel.id).unwrap();
        assert_eq!(
            view.market,
            Market::new(Country::Brazil, ContentLanguage::Portuguese)
        );
        assert_eq!(labels(&view), ["space history"]);
        assert_eq!(view.rows[0].result, None);
        assert_eq!(view.job, None);

        let blank = app
            .create_channel(ChannelDraft {
                name: "No niche".into(),
                ..ChannelDraft::default()
            })
            .unwrap();
        assert!(app.niche_research(blank.id).unwrap().seeds.is_empty());
    }

    #[test]
    fn research_runs_as_a_job_and_ranks_the_results() {
        let h = Harness::new();
        h.market.set_views("cold niche", 10);
        h.market.set_views("hot niche", 500_000);
        let app = h.start_with_key();
        let channel = channel(&app, "space history");

        let id = app
            .run_niche_research(channel.id, &["cold niche", "hot niche", ""], false)
            .unwrap();
        let job = wait_done(&app, id);
        assert_eq!(job.state(), JobState::Done);
        assert_eq!(job.kind(), JobKind::NicheResearch);

        let view = app.niche_research(channel.id).unwrap();
        assert_eq!(labels(&view), ["hot niche", "cold niche"]);
        let seeds: Vec<_> = view.seeds.iter().map(Niche::label).collect();
        assert_eq!(
            seeds,
            ["cold niche", "hot niche"],
            "seeds keep the typed order"
        );
        let hot = view.rows[0].result.as_ref().unwrap();
        assert!(hot.fresh);
        assert_eq!(hot.statistics.median_views, Some(500_000));
        assert!(hot.scores.unwrap().trend > Score::new(50));
        assert_eq!(view.job.map(|job| job.id()), Some(id));

        let calls = h.market.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].market, channel.details.market());
        assert_eq!(calls[0].key, KEY, "the key comes from the secret store");
        let window = calls[0].since.elapsed().unwrap();
        assert!(
            window >= NicheResearch::RECENT_WINDOW
                && window < NicheResearch::RECENT_WINDOW + PATIENCE
        );
    }

    #[test]
    fn rerunning_within_the_cache_window_makes_no_calls() {
        let h = Harness::new();
        let app = h.start_with_key();
        let channel = channel(&app, "space history");
        let first = app
            .run_niche_research(channel.id, &["space history"], false)
            .unwrap();
        wait_done(&app, first);
        assert_eq!(h.market.calls().len(), 1);

        assert_eq!(
            app.research_cost(channel.id, &["Space  HISTORY"], false)
                .unwrap(),
            0
        );
        let again = app
            .run_niche_research(channel.id, &["Space  HISTORY"], false)
            .unwrap();
        assert_eq!(wait_done(&app, again).state(), JobState::Done);
        assert_eq!(h.market.calls().len(), 1, "served from the cache");
    }

    #[test]
    fn a_cached_run_works_even_without_a_key() {
        let h = Harness::new();
        let mut app = h.start_with_key();
        let channel = channel(&app, "space history");
        wait_done(
            &app,
            app.run_niche_research(channel.id, &["space history"], false)
                .unwrap(),
        );
        app.remove_provider_key(Provider::YouTubeData).unwrap();

        let id = app
            .run_niche_research(channel.id, &["space history"], false)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);
        assert_eq!(h.market.calls().len(), 1);
    }

    #[test]
    fn refresh_bypasses_the_cache() {
        let h = Harness::new();
        let app = h.start_with_key();
        let channel = channel(&app, "space history");
        wait_done(
            &app,
            app.run_niche_research(channel.id, &["a", "b"], false)
                .unwrap(),
        );
        let before = app.niche_research(channel.id).unwrap();

        assert_eq!(
            app.research_cost(channel.id, &["a", "b"], true).unwrap(),
            204
        );
        let id = app
            .run_niche_research(channel.id, &["a", "b"], true)
            .unwrap();
        wait_done(&app, id);

        assert_eq!(h.market.calls().len(), 4);
        let after = app.niche_research(channel.id).unwrap();
        let fetched = |view: &NicheResearchView| view.rows[0].result.as_ref().unwrap().fetched_at;
        assert!(fetched(&after) >= fetched(&before));
    }

    #[test]
    fn a_stale_result_is_fetched_again() {
        let h = Harness::new();
        let app = h.start_with_key();
        let channel = channel(&app, "space history");
        let niche = Niche::new("space history").unwrap();
        NicheResearchRepository::save(
            &*h.db,
            &NicheResearch {
                owner: app.profile().id,
                niche: niche.clone(),
                market: channel.details.market(),
                fetched_at: SystemTime::now()
                    - NicheResearch::CACHE_WINDOW
                    - Duration::from_secs(60),
                sample: MarketSample::default(),
            },
        )
        .unwrap();

        let view = app.niche_research(channel.id).unwrap();
        assert!(!view.rows[0].result.as_ref().unwrap().fresh);
        assert_eq!(
            app.research_cost(channel.id, &["space history"], false)
                .unwrap(),
            102
        );

        wait_done(
            &app,
            app.run_niche_research(channel.id, &["space history"], false)
                .unwrap(),
        );
        assert_eq!(h.market.calls().len(), 1);
        assert!(
            app.niche_research(channel.id).unwrap().rows[0]
                .result
                .as_ref()
                .unwrap()
                .fresh
        );
    }

    #[test]
    fn the_cost_counts_only_niches_to_fetch() {
        let h = Harness::new();
        let app = h.start_with_key();
        let channel = channel(&app, "space history");
        assert_eq!(
            app.research_cost(channel.id, &["a", "b", "A"], false)
                .unwrap(),
            204
        );
        wait_done(
            &app,
            app.run_niche_research(channel.id, &["a"], false).unwrap(),
        );
        assert_eq!(
            app.research_cost(channel.id, &["a", "b"], false).unwrap(),
            102
        );
    }

    #[test]
    fn starting_research_needs_a_key_when_it_would_fetch() {
        let h = Harness::new();
        let app = h.start();
        let channel = channel(&app, "space history");

        let error = app
            .run_niche_research(channel.id, &["space history"], false)
            .unwrap_err();
        assert!(matches!(error, ResearchError::MissingKey));
        assert_eq!(error.message(), Text::ResearchMissingKey);
        assert!(app.jobs().is_empty());
        assert!(h.db.seeds(channel.id).unwrap().is_empty(), "nothing saved");
    }

    #[test]
    fn invalid_seeds_are_reported_and_nothing_starts() {
        let h = Harness::new();
        let app = h.start_with_key();
        let channel = channel(&app, "space history");

        let error = app
            .run_niche_research::<&str>(channel.id, &["", " "], false)
            .unwrap_err();
        assert_eq!(error.seed_errors(), [NicheSeedError::Required]);
        assert_eq!(
            error.message(),
            Text::NicheSeedError(NicheSeedError::Required)
        );
        assert!(app.jobs().is_empty());
        assert!(matches!(
            app.research_cost::<&str>(channel.id, &[], false),
            Err(ResearchError::Invalid(_))
        ));
    }

    #[test]
    fn another_profiles_channel_is_not_found() {
        let h = Harness::new();
        let app = h.start_with_key();
        assert!(matches!(
            app.niche_research(ChannelId::new()),
            Err(ResearchError::ChannelNotFound)
        ));
        assert!(matches!(
            app.run_niche_research(ChannelId::new(), &["x"], false),
            Err(ResearchError::ChannelNotFound)
        ));
    }

    #[test]
    fn a_spent_quota_fails_the_job_without_retries_and_says_why() {
        let h = Harness::new();
        *h.market.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::LimitReached,
            "The request cannot be completed because you have exceeded your quota.",
        ));
        let app = h.start_with_key();
        let channel = channel(&app, "space history");

        let id = app
            .run_niche_research(channel.id, &["space history"], false)
            .unwrap();
        let job = wait_done(&app, id);

        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.attempts(), 1, "a spent quota is not retried");
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::LimitReached);
        assert!(
            failure.detail.starts_with("space history: "),
            "{}",
            failure.detail
        );
        assert_eq!(app.niche_research(channel.id).unwrap().job, Some(job));
    }

    #[test]
    fn an_outage_is_retried() {
        let h = Harness::new();
        *h.market.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Unreachable,
            "timed out",
        ));
        let app = h.start_with_key();
        let channel = channel(&app, "space history");

        let job = wait_done(
            &app,
            app.run_niche_research(channel.id, &["x"], false).unwrap(),
        );
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::ProviderUnavailable
        );
        assert_eq!(job.attempts(), 2);
    }

    #[test]
    fn a_key_removed_before_the_job_runs_fails_it_clearly() {
        let h = Harness::new();
        let app = h.start_with_key();
        let channel = channel(&app, "space history");
        h.secrets
            .delete(app.profile().id, Provider::YouTubeData)
            .unwrap();

        let job = wait_done(
            &app,
            app.run_niche_research(channel.id, &["x"], false).unwrap(),
        );
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::MissingKey);
        assert!(h.market.calls().is_empty());
    }

    #[test]
    fn progress_moves_one_niche_at_a_time() {
        let h = Harness::new();
        *h.market.delay.lock().unwrap() = Duration::from_millis(30);
        let app = h.start_with_key();
        let channel = channel(&app, "space history");

        let id = app
            .run_niche_research(channel.id, &["a", "b", "c", "d"], false)
            .unwrap();
        let midway = wait_for(&app, id, |job| job.progress() > Progress::ZERO);
        assert!(midway.progress() < Progress::DONE);
        assert_eq!(midway.state(), JobState::Running);
        wait_done(&app, id);
    }

    #[test]
    fn closing_mid_run_resumes_without_fetching_done_niches_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let market = Arc::new(FakeMarketData::default());
        *market.delay.lock().unwrap() = Duration::from_millis(40);
        let secrets = Arc::new(MemorySecretStore::default());

        let first = Harness::on_disk(&path, Arc::clone(&market), Arc::clone(&secrets));
        let app = first.start_with_key();
        let channel = channel(&app, "space history");
        let id = app
            .run_niche_research(channel.id, &["a", "b", "c", "d", "e", "f"], true)
            .unwrap();
        wait_for(&app, id, |job| job.checkpoint() == Some("2"));
        drop(app);
        drop(first);
        let fetched_before = market.calls().len();
        assert!(fetched_before < 6);

        let second = Harness::on_disk(&path, Arc::clone(&market), secrets);
        let app = second.start();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);

        let labels: Vec<String> = market.calls().into_iter().map(|call| call.niche).collect();
        for label in ["a", "b", "c", "d", "e", "f"] {
            let times = labels.iter().filter(|l| *l == label).count();
            assert!(times >= 1, "{label} was fetched");
        }
        assert_eq!(
            labels.iter().filter(|l| *l == "a").count(),
            1,
            "done niches are not fetched again"
        );
        assert_eq!(labels.iter().filter(|l| *l == "b").count(), 1);
    }

    #[test]
    fn rows_use_the_seed_as_typed_now() {
        let h = Harness::new();
        let app = h.start_with_key();
        let channel = channel(&app, "space history");
        wait_done(
            &app,
            app.run_niche_research(channel.id, &["space history"], false)
                .unwrap(),
        );
        wait_done(
            &app,
            app.run_niche_research(channel.id, &["Space History"], false)
                .unwrap(),
        );

        let view = app.niche_research(channel.id).unwrap();
        assert_eq!(labels(&view), ["Space History"]);
        assert!(view.rows[0].result.is_some());
    }
}
