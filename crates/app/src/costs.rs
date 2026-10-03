//! Cost and budget use cases (PRD stories 39, 51-55): every paid provider
//! call records what it cost, priced from the rate table unless the
//! provider reported it; the costs screen adds spend up per provider,
//! channel and video for a month; each paid provider can have a monthly
//! budget.
//!
//! Before a generation starts, `app` estimates it and checks the budgets of
//! the providers it calls: from 80% the screens warn, and a job that would
//! reach 100% (or starts past it) needs the user's confirmation instead of
//! starting.
//!
//! The rate table is data: the built-in prices in `data/rates.toml`, with
//! the user's changes on top.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use bardo_domain::{
    Budget, BudgetLevel, ChannelId, Cost, CostPurpose, CostRecord, CostRecordId, CostRepository,
    JobId, Meter, Metered, Money, MoneyError, Month, ProfileId, Provider, Rate, RateFieldError,
    RateTable, RepositoryError, Spend, ThemeRepository, VideoProjectId,
};

use crate::{Bardo, Text};

/// The built-in prices.
const DEFAULT_RATES: &str = include_str!("../data/rates.toml");

/// Samples an estimate averages, when there are any.
const ESTIMATE_SAMPLES: usize = 5;

/// About four characters make a token in the languages Bardo writes.
const CHARS_PER_TOKEN: u64 = 4;

/// Parses a rate file: `[provider."model"]` tables of `meter = "price"`.
fn parse_rates(text: &str) -> Result<Vec<Rate>, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("{e}"))?;
    let mut rates = Vec::new();
    for (provider, models) in &table {
        let provider: Provider = provider.parse().map_err(|e| format!("{e}"))?;
        let models = models
            .as_table()
            .ok_or_else(|| format!("{provider}: expected a table of models"))?;
        for (model, meters) in models {
            let meters = meters
                .as_table()
                .ok_or_else(|| format!("{provider}.{model}: expected a table of meters"))?;
            for (meter, price) in meters {
                let meter: Meter = meter.parse().map_err(|e| format!("{e}"))?;
                let price = price
                    .as_str()
                    .ok_or_else(|| format!("{provider}.{model}.{meter:?}: price must be text"))?;
                rates.push(
                    Rate::new(provider, model, meter, price)
                        .map_err(|e| format!("{provider}.{model}.{meter:?}: {e}"))?,
                );
            }
        }
    }
    Ok(rates)
}

/// The built-in rates, read once.
pub(crate) fn default_rates() -> &'static [Rate] {
    static RATES: OnceLock<Vec<Rate>> = OnceLock::new();
    RATES.get_or_init(|| parse_rates(DEFAULT_RATES).expect("the built-in rates are valid"))
}

/// What a planned generation would spend with one provider, and where that
/// leaves the provider's budget this month.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEstimate {
    pub provider: Provider,
    /// `None` when no rate covers the model it would call.
    pub amount: Option<Money>,
    /// Spent with the provider this month, before the job.
    pub spent: Money,
    pub budget: Option<Money>,
    /// Where the budget stands once the job runs; `None` without one.
    pub level: Option<BudgetLevel>,
}

/// What a planned generation would spend, per provider it calls.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpendEstimate {
    pub providers: Vec<ProviderEstimate>,
}

impl SpendEstimate {
    /// The priced part of the estimate.
    pub fn total(&self) -> Money {
        self.providers
            .iter()
            .filter_map(|provider| provider.amount)
            .sum()
    }

    /// Whether some call has no rate, so the total is short.
    pub fn is_partial(&self) -> bool {
        self.providers
            .iter()
            .any(|provider| provider.amount.is_none())
    }

    /// The worst budget level among the providers it calls.
    pub fn level(&self) -> BudgetLevel {
        self.providers
            .iter()
            .filter_map(|provider| provider.level)
            .max()
            .unwrap_or(BudgetLevel::Under)
    }

    /// The providers whose budget the job would reach or has reached.
    pub fn over_budget(&self) -> impl Iterator<Item = &ProviderEstimate> {
        self.providers
            .iter()
            .filter(|provider| provider.level == Some(BudgetLevel::Reached))
    }

    /// The providers whose budget the job would take to 80% or more but
    /// not to 100%.
    pub fn near_budget(&self) -> impl Iterator<Item = &ProviderEstimate> {
        self.providers
            .iter()
            .filter(|provider| provider.level == Some(BudgetLevel::Warning))
    }
}

/// Whether the user confirmed starting a job past a budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetConsent {
    /// Start only within budget; past it, return the estimate so the
    /// screen can ask.
    Ask,
    /// The user saw the estimate and chose to go over.
    Confirmed,
}

/// One kind of paid call a generation makes, `calls` times.
#[derive(Debug, Clone)]
pub(crate) struct PlannedCall {
    pub(crate) provider: Provider,
    /// The model the adapter calls.
    pub(crate) model: Cow<'static, str>,
    pub(crate) purpose: CostPurpose,
    pub(crate) calls: u64,
    /// Characters of instructions and prompt, when known before the job.
    pub(crate) prompt_chars: Option<usize>,
    /// Characters read aloud, when known before the job.
    pub(crate) characters: Option<usize>,
    /// Seconds of video, when known before the job.
    pub(crate) video_seconds: Option<u32>,
    /// Seconds of audio, when known before the job.
    pub(crate) audio_seconds: Option<u64>,
}

impl PlannedCall {
    pub(crate) fn new(provider: Provider, purpose: CostPurpose, calls: u64) -> Self {
        let model = match provider {
            Provider::Claude => bardo_ai::claude::MODEL,
            Provider::ElevenLabs => bardo_ai::elevenlabs::SPEECH_MODEL,
            Provider::Gemini => bardo_ai::gemini::IMAGE_MODEL,
            Provider::TypeSafe => bardo_ai::jev::MODEL,
            Provider::Higgsfield => bardo_ai::higgsfield::DEFAULT_MODEL,
            Provider::YouTubeData => "",
        };
        Self {
            provider,
            model: Cow::Borrowed(model),
            purpose,
            calls,
            prompt_chars: None,
            characters: None,
            video_seconds: None,
            audio_seconds: None,
        }
    }

    /// For providers whose model the user picks.
    pub(crate) fn with_model(mut self, model: &str) -> Self {
        self.model = Cow::Owned(model.to_owned());
        self
    }

    pub(crate) fn with_video_seconds(mut self, seconds: u32) -> Self {
        self.video_seconds = Some(seconds);
        self
    }

    pub(crate) fn with_audio_seconds(mut self, seconds: u64) -> Self {
        self.audio_seconds = Some(seconds);
        self
    }

    pub(crate) fn with_prompt(mut self, instructions: &str, prompt: &str) -> Self {
        self.prompt_chars = Some(instructions.chars().count() + prompt.chars().count());
        self
    }

    pub(crate) fn with_characters(mut self, characters: usize) -> Self {
        self.characters = Some(characters);
        self
    }
}

/// What one call of `purpose` uses before Bardo has seen any.
fn first_guess(purpose: CostPurpose) -> Metered {
    let tokens = |input_tokens, output_tokens| Metered {
        input_tokens,
        output_tokens,
        ..Metered::default()
    };
    match purpose {
        // Ten ideas as JSON, with some reasoning.
        CostPurpose::ThemeIdeas => tokens(1_200, 3_000),
        // Ten themes, three questions each (four, with up to 40 past
        // videos in the state, once the channel has history).
        CostPurpose::ThemeRanking => tokens(2_000, 0),
        CostPurpose::Script => tokens(1_000, 4_000),
        CostPurpose::Narration => Metered::characters(5_000),
        CostPurpose::ScenePlan => tokens(2_000, 4_000),
        // A 2K image.
        CostPurpose::SceneImage => Metered {
            input_tokens: 40,
            output_tokens: 250,
            image_tokens: 1_680,
            ..Metered::default()
        },
        CostPurpose::SceneClip => Metered::video_seconds(5),
        // A ten-minute recording.
        CostPurpose::NarrationAlignment => Metered::audio_seconds(600),
        // A short paragraph.
        CostPurpose::MusicPrompt => tokens(500, 200),
        // A post per network for a script of a few minutes.
        CostPurpose::Metadata => tokens(2_500, 1_200),
        // A stretch of script and two questions per point in it.
        CostPurpose::CutSuggestions => tokens(4_000, 0),
    }
}

/// Records paid calls as jobs make them. Shared with the job handlers.
#[derive(Clone)]
pub(crate) struct CostBook {
    pub(crate) owner: ProfileId,
    pub(crate) costs: Arc<dyn CostRepository>,
    /// To find the channel of a video project.
    pub(crate) themes: Arc<dyn ThemeRepository>,
}

/// A paid call that just answered.
pub(crate) struct PaidCall<'a> {
    pub(crate) provider: Provider,
    pub(crate) model: &'a str,
    pub(crate) purpose: CostPurpose,
    pub(crate) usage: Metered,
    pub(crate) job: JobId,
    /// What the provider said it charges, when it says; the rate table
    /// prices the call otherwise.
    pub(crate) reported: Option<Money>,
}

impl CostBook {
    /// The built-in rates with the user's changes.
    pub(crate) fn rates(&self) -> Result<RateTable, RepositoryError> {
        Ok(RateTable::with_changes(
            default_rates(),
            &self.costs.rate_changes(self.owner)?,
        ))
    }

    /// Records a call made for a channel (and a video project of it).
    /// The money is spent whatever happens next, so a record that cannot
    /// be saved is logged and the job goes on.
    pub(crate) fn record(
        &self,
        call: PaidCall<'_>,
        channel: Option<ChannelId>,
        project: Option<VideoProjectId>,
    ) {
        let result = self.rates().and_then(|rates| {
            self.costs.record_cost(&CostRecord {
                id: CostRecordId::new(),
                owner: self.owner,
                provider: call.provider,
                model: call.model.to_owned(),
                purpose: call.purpose,
                usage: call.usage,
                cost: Cost::of(
                    call.reported,
                    &rates,
                    call.provider,
                    call.model,
                    &call.usage,
                ),
                channel,
                project,
                job: Some(call.job),
                at: SystemTime::now(),
            })
        });
        if let Err(error) = result {
            tracing::warn!(
                "could not record the cost of a {} call ({}): {error}",
                call.provider,
                call.purpose
            );
        }
    }

    /// `record` for a call made for a video project, under its channel.
    pub(crate) fn record_for_project(&self, call: PaidCall<'_>, project: VideoProjectId) {
        let channel = match self.themes.project(project) {
            Ok(found) => found.map(|project| project.channel),
            Err(error) => {
                tracing::warn!("could not find the channel of project {project}: {error}");
                None
            }
        };
        self.record(call, channel, Some(project));
    }

    /// What `call` would use, from the last calls for the same purpose or
    /// a first guess, with what is known before the job in place.
    fn typical(&self, call: &PlannedCall) -> Result<Metered, RepositoryError> {
        let samples: Vec<Metered> = self
            .costs
            .recent_costs(self.owner, call.purpose, ESTIMATE_SAMPLES)?
            .into_iter()
            .filter(|record| record.provider == call.provider)
            .map(|record| record.usage)
            .collect();
        let mut usage = Metered::mean(&samples).unwrap_or_else(|| first_guess(call.purpose));
        if let Some(chars) = call.prompt_chars {
            usage.input_tokens = (chars as u64).div_ceil(CHARS_PER_TOKEN);
        }
        if let Some(characters) = call.characters {
            usage.characters = characters as u64;
        }
        if let Some(seconds) = call.video_seconds {
            usage.video_seconds = u64::from(seconds);
        }
        if let Some(seconds) = call.audio_seconds {
            usage.audio_seconds = seconds;
        }
        Ok(usage.times(call.calls))
    }

    /// Spend with each provider in `month`.
    fn month_spend(&self, month: Month) -> Result<Spend, RepositoryError> {
        let rates = self.rates()?;
        let records: Vec<CostRecord> = self
            .costs
            .costs_between(self.owner, month.start(), month.end())?
            .into_iter()
            .map(|record| record.priced_with(&rates))
            .collect();
        Ok(Spend::of(&records))
    }

    /// What `calls` would spend, per provider, against this month's
    /// budgets.
    pub(crate) fn estimate(&self, calls: &[PlannedCall]) -> Result<SpendEstimate, RepositoryError> {
        let rates = self.rates()?;
        let spend = self.month_spend(Month::of(SystemTime::now()))?;
        let budgets = self.costs.budgets(self.owner)?;
        let mut providers: Vec<ProviderEstimate> = Vec::new();
        for call in calls.iter().filter(|call| call.calls > 0) {
            let amount = rates.price(call.provider, &call.model, &self.typical(call)?);
            match providers.iter_mut().find(|p| p.provider == call.provider) {
                Some(estimate) => {
                    estimate.amount = estimate
                        .amount
                        .zip(amount)
                        .map(|(a, b)| a.saturating_add(b));
                }
                None => providers.push(ProviderEstimate {
                    provider: call.provider,
                    amount,
                    spent: spend.provider(call.provider),
                    budget: None,
                    level: None,
                }),
            }
        }
        for estimate in &mut providers {
            if let Some(budget) = budgets.iter().find(|b| b.provider == estimate.provider) {
                estimate.budget = Some(budget.monthly);
                estimate.level =
                    Some(budget.level(estimate.spent, estimate.amount.unwrap_or_default()));
            }
        }
        Ok(SpendEstimate { providers })
    }
}

/// Why a cost action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum CostError {
    /// The typed budget is not an amount.
    #[error("invalid budget: {0}")]
    InvalidBudget(MoneyError),
    #[error("invalid rate: {0}")]
    InvalidRate(RateFieldError),
    /// The provider is free (a quota), so it has no budget.
    #[error("{0} is not a paid provider")]
    NotPaid(Provider),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl CostError {
    /// What the costs screen says.
    pub fn message(&self) -> Text {
        match self {
            CostError::InvalidBudget(error) => Text::MoneyError(*error),
            CostError::InvalidRate(error) => Text::RateFieldError(*error),
            CostError::NotPaid(_) => Text::CostsNotPaid,
            CostError::Repository(_) => Text::CostsNotSaved,
        }
    }
}

/// A paid provider's spend in a month, against its budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpend {
    pub provider: Provider,
    pub spent: Money,
    pub budget: Option<Money>,
    /// `None` without a budget.
    pub level: Option<BudgetLevel>,
    /// Of the budget, in whole percent; `None` without a budget.
    pub percent: Option<u64>,
}

/// Spend on one channel or video. `name` is `None` when what it was for
/// is gone (or, for a channel, when the call was for none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendRow {
    pub name: Option<String>,
    /// For a video, its channel's name.
    pub channel: Option<String>,
    pub amount: Money,
}

/// One line of the rate table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateRow {
    pub rate: Rate,
    /// The built-in price for the same provider, model and meter.
    pub default: Option<Money>,
    /// Whether the user changed or added it.
    pub changed: bool,
}

/// The costs screen for one month.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostsView {
    pub month: Month,
    pub total: Money,
    /// Every paid provider, in settings order.
    pub providers: Vec<ProviderSpend>,
    /// Most spent first.
    pub channels: Vec<SpendRow>,
    /// Most spent first.
    pub videos: Vec<SpendRow>,
    /// Models that ran this month without a rate: their calls count as
    /// nothing until one is added.
    pub unpriced: Vec<(Provider, String)>,
    pub rates: Vec<RateRow>,
}

/// A month at a glance: the spend, how the budgets stand together, and
/// the budgets that call for attention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendSummary {
    pub total: Money,
    /// Paid providers that spent something.
    pub providers_used: usize,
    /// Providers with a budget.
    pub budgets: usize,
    /// What the providers with a budget spent, of their budgets together,
    /// in whole percent; `None` without budgets.
    pub budget_percent: Option<u64>,
    /// The budgets from 80% on, the ones reached first.
    pub alerts: Vec<(Provider, BudgetLevel)>,
}

impl SpendSummary {
    /// The worst level among the budgets.
    pub fn level(&self) -> BudgetLevel {
        self.alerts
            .iter()
            .map(|(_, level)| *level)
            .max()
            .unwrap_or(BudgetLevel::Under)
    }
}

impl CostsView {
    pub fn summary(&self) -> SpendSummary {
        let budgeted = || {
            self.providers
                .iter()
                .filter_map(|spend| spend.budget.map(|budget| (spend, budget)))
        };
        let together = Budget {
            // Which provider does not matter for the percentage.
            provider: Provider::Claude,
            monthly: budgeted().map(|(_, budget)| budget).sum(),
        };
        let spent: Money = budgeted().map(|(spend, _)| spend.spent).sum();
        let mut alerts: Vec<(Provider, BudgetLevel)> = self
            .providers
            .iter()
            .filter_map(|spend| Some((spend.provider, spend.level?)))
            .filter(|(_, level)| *level != BudgetLevel::Under)
            .collect();
        // Stable, so providers keep their settings order within a level.
        alerts.sort_by_key(|(_, level)| std::cmp::Reverse(*level));
        SpendSummary {
            total: self.total,
            providers_used: self
                .providers
                .iter()
                .filter(|spend| !spend.spent.is_zero())
                .count(),
            budgets: budgeted().count(),
            budget_percent: (budgeted().count() > 0).then(|| together.percent_used(spent)),
            alerts,
        }
    }
}

impl Bardo {
    /// The month now, in UTC as providers bill.
    pub fn current_month(&self) -> Month {
        Month::of(SystemTime::now())
    }

    pub(crate) fn estimate(&self, calls: &[PlannedCall]) -> Result<SpendEstimate, RepositoryError> {
        self.cost_book.estimate(calls)
    }

    /// Estimates `calls` and, past a budget, returns the estimate unless
    /// the user confirmed.
    pub(crate) fn check_budget(
        &self,
        calls: &[PlannedCall],
        consent: BudgetConsent,
    ) -> Result<Result<(), SpendEstimate>, RepositoryError> {
        if consent == BudgetConsent::Confirmed {
            return Ok(Ok(()));
        }
        let estimate = self.estimate(calls)?;
        Ok(match estimate.level() {
            BudgetLevel::Reached => Err(estimate),
            _ => Ok(()),
        })
    }

    /// What a video project cost so far.
    pub(crate) fn project_spend(&self, project: VideoProjectId) -> Result<Money, RepositoryError> {
        let rates = self.cost_book.rates()?;
        Ok(self
            .cost_book
            .costs
            .project_costs(project)?
            .into_iter()
            .map(|record| record.priced_with(&rates).cost.amount())
            .sum())
    }

    /// The costs screen for `month`.
    pub fn costs(&self, month: Month) -> Result<CostsView, CostError> {
        let book = &self.cost_book;
        let spend = book.month_spend(month)?;
        let budgets = book.costs.budgets(self.profile.id)?;
        let providers = Provider::paid()
            .map(|provider| {
                let spent = spend.provider(provider);
                let budget = budgets.iter().find(|b| b.provider == provider);
                ProviderSpend {
                    provider,
                    spent,
                    budget: budget.map(|b| b.monthly),
                    level: budget.map(|b| b.level(spent, Money::ZERO)),
                    percent: budget.map(|b| b.percent_used(spent)),
                }
            })
            .collect();

        let mut channels = Vec::new();
        for (channel, amount) in &spend.channels {
            let name = match channel {
                Some(id) => self
                    .channels
                    .get(*id)?
                    .map(|channel| channel.details.name().to_owned()),
                None => None,
            };
            channels.push(SpendRow {
                name,
                channel: None,
                amount: *amount,
            });
        }
        let mut videos = Vec::new();
        for (project, amount) in &spend.projects {
            let project = self.themes.project(*project)?;
            let channel = match &project {
                Some(project) => self
                    .channels
                    .get(project.channel)?
                    .map(|channel| channel.details.name().to_owned()),
                None => None,
            };
            videos.push(SpendRow {
                name: project.map(|project| project.title),
                channel,
                amount: *amount,
            });
        }

        let changes = book.costs.rate_changes(self.profile.id)?;
        let defaults = default_rates();
        let rates = RateTable::with_changes(defaults, &changes)
            .rates()
            .iter()
            .map(|rate| {
                let same = |other: &&Rate| {
                    other.provider == rate.provider
                        && other.model == rate.model
                        && other.meter == rate.meter
                };
                RateRow {
                    default: defaults.iter().find(same).map(|default| default.price),
                    changed: changes.iter().any(|change| same(&change)),
                    rate: rate.clone(),
                }
            })
            .collect();

        Ok(CostsView {
            month,
            total: spend.total,
            providers,
            channels,
            videos,
            unpriced: spend.unpriced,
            rates,
        })
    }

    /// Sets `provider`'s monthly budget from the typed amount.
    pub fn set_budget(&self, provider: Provider, amount: &str) -> Result<Budget, CostError> {
        if !provider.is_paid() {
            return Err(CostError::NotPaid(provider));
        }
        let budget = Budget {
            provider,
            monthly: Money::parse(amount).map_err(CostError::InvalidBudget)?,
        };
        self.cost_book.costs.save_budget(self.profile.id, &budget)?;
        Ok(budget)
    }

    /// Removes `provider`'s budget: its jobs no longer warn or ask.
    pub fn remove_budget(&self, provider: Provider) -> Result<(), CostError> {
        Ok(self
            .cost_book
            .costs
            .remove_budget(self.profile.id, provider)?)
    }

    /// Sets the price of `meter` for `provider`'s models starting with
    /// `model`, replacing the built-in one if there is one. New calls are
    /// priced with it; recorded ones keep their cost.
    pub fn save_rate(
        &self,
        provider: Provider,
        model: &str,
        meter: Meter,
        price: &str,
    ) -> Result<Rate, CostError> {
        let rate = Rate::new(provider, model, meter, price).map_err(CostError::InvalidRate)?;
        self.cost_book.costs.save_rate(self.profile.id, &rate)?;
        Ok(rate)
    }

    /// Drops the user's price: a built-in rate comes back, an added one
    /// goes.
    pub fn reset_rate(
        &self,
        provider: Provider,
        model: &str,
        meter: Meter,
    ) -> Result<(), CostError> {
        Ok(self
            .cost_book
            .costs
            .remove_rate(self.profile.id, provider, model, meter)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price(provider: Provider, model: &str, meter: Meter) -> Option<Money> {
        RateTable::new(default_rates().to_vec())
            .rate(provider, model, meter)
            .map(|rate| rate.price)
    }

    #[test]
    fn the_built_in_rates_price_every_model_bardo_calls() {
        let dollars = |text| Some(Money::parse(text).unwrap());
        let claude = bardo_ai::claude::MODEL;
        assert_eq!(
            price(Provider::Claude, claude, Meter::InputTokens),
            dollars("4")
        );
        assert_eq!(
            price(Provider::Claude, claude, Meter::OutputTokens),
            dollars("20")
        );
        let speech = bardo_ai::elevenlabs::SPEECH_MODEL;
        assert_eq!(
            price(Provider::ElevenLabs, speech, Meter::Characters),
            dollars("0.10")
        );
        let image = bardo_ai::gemini::IMAGE_MODEL;
        for (meter, expected) in [
            (Meter::InputTokens, "0.50"),
            (Meter::OutputTokens, "3"),
            (Meter::ImageTokens, "60"),
        ] {
            assert_eq!(price(Provider::Gemini, image, meter), dollars(expected));
        }
        // JEV answers with its version, e.g. `jev-1.13.0`.
        for model in [bardo_ai::jev::MODEL, "jev-1.13.0"] {
            assert_eq!(
                price(Provider::TypeSafe, model, Meter::InputTokens),
                dollars("0.042")
            );
        }
    }

    #[test]
    fn the_built_in_rates_price_google_clips_by_model_and_resolution() {
        let per_second = |model: &str| price(Provider::Gemini, model, Meter::VideoSeconds);
        let dollars = |text| Some(Money::parse(text).unwrap());
        for (model, expected) in [
            ("veo-3.1-generate-preview/720p", "0.40"),
            ("veo-3.1-generate-preview/1080p", "0.40"),
            ("veo-3.1-fast-generate-preview/720p", "0.10"),
            ("veo-3.1-fast-generate-preview/1080p", "0.12"),
            ("veo-3.1-lite-generate-preview/720p", "0.05"),
            ("veo-3.1-lite-generate-preview/1080p", "0.08"),
            ("gemini-omni-1.1-flash/720p", "0.10136"),
        ] {
            assert_eq!(per_second(model), dollars(expected), "{model}");
        }
        assert_eq!(
            per_second("gemini-omni-1.1-flash/1080p"),
            None,
            "no published price"
        );
        for model in bardo_ai::google_clips::models() {
            let priced = per_second(model.id.model()).is_some();
            assert_eq!(
                priced,
                !model.id.model().ends_with("flash/1080p"),
                "{}",
                model.id
            );
        }
    }

    #[test]
    fn a_rate_file_says_what_is_wrong() {
        assert!(parse_rates("[claude.\"m\"]\ninput_tokens = \"4\"").is_ok());
        for broken in [
            "[nobody.\"m\"]\ninput_tokens = \"1\"",
            "[claude.\"m\"]\ncredits = \"1\"",
            "[claude.\"m\"]\ninput_tokens = 4.0",
            "[claude.\"m\"]\ninput_tokens = \"four\"",
            "[youtube-data.\"m\"]\ninput_tokens = \"1\"",
            "claude = 1",
        ] {
            assert!(parse_rates(broken).is_err(), "{broken}");
        }
    }

    fn spend(provider: Provider, spent: u64, budget: Option<u64>) -> ProviderSpend {
        let budget = budget.map(|cents| Budget {
            provider,
            monthly: Money::from_cents(cents),
        });
        let spent = Money::from_cents(spent);
        ProviderSpend {
            provider,
            spent,
            budget: budget.map(|b| b.monthly),
            level: budget.map(|b| b.level(spent, Money::ZERO)),
            percent: budget.map(|b| b.percent_used(spent)),
        }
    }

    fn view(providers: Vec<ProviderSpend>) -> CostsView {
        CostsView {
            month: Month::new(2026, 10).unwrap(),
            total: providers.iter().map(|p| p.spent).sum(),
            providers,
            channels: Vec::new(),
            videos: Vec::new(),
            unpriced: Vec::new(),
            rates: Vec::new(),
        }
    }

    #[test]
    fn a_summary_adds_the_budgets_up_and_lists_the_ones_to_watch() {
        let summary = view(vec![
            spend(Provider::Claude, 60, Some(2_000)),
            spend(Provider::ElevenLabs, 168, Some(150)),
            spend(Provider::Gemini, 731, Some(800)),
            spend(Provider::Higgsfield, 0, None),
            spend(Provider::TypeSafe, 0, None),
        ])
        .summary();
        assert_eq!(summary.total, Money::from_cents(959));
        assert_eq!(summary.providers_used, 3);
        assert_eq!(summary.budgets, 3);
        assert_eq!(
            summary.budget_percent,
            Some(32),
            "959 of 2950, rounded down"
        );
        assert_eq!(
            summary.alerts,
            [
                (Provider::ElevenLabs, BudgetLevel::Reached),
                (Provider::Gemini, BudgetLevel::Warning),
            ]
        );
        assert_eq!(summary.level(), BudgetLevel::Reached);
    }

    #[test]
    fn a_summary_without_budgets_has_no_percentage_or_alerts() {
        let summary = view(vec![spend(Provider::Claude, 120, None)]).summary();
        assert_eq!(summary.budget_percent, None);
        assert!(summary.alerts.is_empty());
        assert_eq!(summary.level(), BudgetLevel::Under);
        assert_eq!(summary.providers_used, 1);
    }

    #[test]
    fn an_estimate_sums_its_providers_and_takes_the_worst_level() {
        let estimate = SpendEstimate {
            providers: vec![
                ProviderEstimate {
                    provider: Provider::Claude,
                    amount: Some(Money::from_cents(5)),
                    spent: Money::ZERO,
                    budget: Some(Money::from_cents(500)),
                    level: Some(BudgetLevel::Warning),
                },
                ProviderEstimate {
                    provider: Provider::TypeSafe,
                    amount: None,
                    spent: Money::ZERO,
                    budget: None,
                    level: None,
                },
            ],
        };
        assert_eq!(estimate.total(), Money::from_cents(5));
        assert!(estimate.is_partial());
        assert_eq!(estimate.level(), BudgetLevel::Warning);
        assert_eq!(estimate.near_budget().count(), 1);
        assert_eq!(estimate.over_budget().count(), 0);
        assert_eq!(SpendEstimate::default().level(), BudgetLevel::Under);
    }
}
