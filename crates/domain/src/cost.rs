//! What generations cost (PRD stories 39, 51-52): every paid provider call
//! records what the provider counted and what that cost, as the provider
//! reported it or else estimated from a rate table. Spend adds the records
//! up per provider, channel and video.
//!
//! Amounts are US dollars, the currency every provider bills in, kept as
//! whole millionths of a dollar so sums never drift.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

use crate::{ChannelId, JobId, ProfileId, Provider, RepositoryError, TokenUsage, VideoProjectId};

uuid_id!(
    /// Identifies one cost record.
    CostRecordId
);

/// An amount of US dollars, never negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Money(u64);

/// Why typed text is not an amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum MoneyError {
    #[error("the amount is empty")]
    Required,
    #[error("the amount is not a number")]
    Invalid,
    #[error("the amount has more than six decimal places")]
    TooPrecise,
    #[error("the amount is too large")]
    TooLarge,
}

impl Money {
    pub const ZERO: Money = Money(0);
    /// Millionths of a dollar in a dollar.
    pub const MICROS_PER_DOLLAR: u64 = 1_000_000;
    /// Nobody budgets or prices beyond this; it keeps sums far from
    /// overflowing.
    pub const MAX: Money = Money(1_000_000_000 * Self::MICROS_PER_DOLLAR);

    pub const fn from_micros(micros: u64) -> Self {
        Money(micros)
    }

    pub const fn from_cents(cents: u64) -> Self {
        Money(cents * 10_000)
    }

    pub const fn micros(self) -> u64 {
        self.0
    }

    /// Whole dollars and the millionths left over.
    pub const fn split(self) -> (u64, u64) {
        (
            self.0 / Self::MICROS_PER_DOLLAR,
            self.0 % Self::MICROS_PER_DOLLAR,
        )
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn saturating_add(self, other: Money) -> Money {
        Money(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(self, other: Money) -> Money {
        Money(self.0.saturating_sub(other.0))
    }

    pub fn times(self, n: u64) -> Money {
        Money(self.0.saturating_mul(n))
    }

    /// Reads an amount as the user typed it: `12`, `0.5`, `3,75`, `$4.20`
    /// or `US$ 4,20`. One `.` or `,` separates the decimals; thousands
    /// separators are not accepted, since `1,000` would be ambiguous.
    pub fn parse(input: &str) -> Result<Money, MoneyError> {
        let text = input.trim();
        let text = text
            .strip_prefix("US$")
            .or_else(|| text.strip_prefix('$'))
            .unwrap_or(text)
            .trim();
        if text.is_empty() {
            return Err(MoneyError::Required);
        }
        let (whole, fraction) = match text.find(['.', ',']) {
            Some(at) => (&text[..at], &text[at + 1..]),
            None => (text, ""),
        };
        let digits = |part: &str| part.chars().all(|c| c.is_ascii_digit());
        if (whole.is_empty() && fraction.is_empty()) || !digits(whole) || !digits(fraction) {
            return Err(MoneyError::Invalid);
        }
        if fraction.len() > 6 {
            return Err(MoneyError::TooPrecise);
        }
        let whole: u64 = if whole.is_empty() {
            0
        } else {
            whole.parse().map_err(|_| MoneyError::TooLarge)?
        };
        let fraction: u64 = format!("{fraction:0<6}")
            .parse()
            .map_err(|_| MoneyError::Invalid)?;
        let micros = whole
            .checked_mul(Self::MICROS_PER_DOLLAR)
            .and_then(|micros| micros.checked_add(fraction))
            .filter(|micros| *micros <= Self::MAX.0)
            .ok_or(MoneyError::TooLarge)?;
        Ok(Money(micros))
    }
}

/// The amount as typed back: dollars with two to six decimals, `12.50`,
/// `0.042`; [`Money::parse`] reads it back.
impl std::fmt::Display for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (whole, fraction) = self.split();
        let mut fraction = format!("{fraction:06}");
        while fraction.len() > 2 && fraction.ends_with('0') {
            fraction.pop();
        }
        write!(f, "{whole}.{fraction}")
    }
}

impl std::iter::Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Self {
        iter.fold(Money::ZERO, Money::saturating_add)
    }
}

/// What a provider counts and charges for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Meter {
    /// Tokens sent: instructions, prompt and inputs.
    InputTokens,
    /// Text tokens written, reasoning included.
    OutputTokens,
    /// Tokens of the images a model drew, priced apart from text.
    ImageTokens,
    /// Characters read aloud.
    Characters,
    /// Seconds of video generated.
    VideoSeconds,
    /// Seconds of audio sent to be timed or transcribed.
    AudioSeconds,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown meter: {0}")]
pub struct UnknownMeter(pub String);

impl Meter {
    pub const ALL: [Meter; 6] = [
        Meter::InputTokens,
        Meter::OutputTokens,
        Meter::ImageTokens,
        Meter::Characters,
        Meter::VideoSeconds,
        Meter::AudioSeconds,
    ];

    /// Stable identifier for storage and the rate file. Never change one.
    pub fn code(self) -> &'static str {
        match self {
            Meter::InputTokens => "input_tokens",
            Meter::OutputTokens => "output_tokens",
            Meter::ImageTokens => "image_tokens",
            Meter::Characters => "characters",
            Meter::VideoSeconds => "video_seconds",
            Meter::AudioSeconds => "audio_seconds",
        }
    }

    /// How many units a price is for, as providers publish them: tokens
    /// per million, characters per thousand, video per second, audio per
    /// hour.
    pub fn units_per_price(self) -> u64 {
        match self {
            Meter::InputTokens | Meter::OutputTokens | Meter::ImageTokens => 1_000_000,
            Meter::Characters => 1_000,
            Meter::VideoSeconds => 1,
            Meter::AudioSeconds => 3_600,
        }
    }
}

impl FromStr for Meter {
    type Err = UnknownMeter;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Meter::ALL
            .into_iter()
            .find(|meter| meter.code() == s)
            .ok_or_else(|| UnknownMeter(s.to_owned()))
    }
}

/// What one provider call used, per meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Metered {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub image_tokens: u64,
    pub characters: u64,
    pub video_seconds: u64,
    pub audio_seconds: u64,
}

impl Metered {
    pub fn characters(characters: u64) -> Self {
        Metered {
            characters,
            ..Metered::default()
        }
    }

    pub fn video_seconds(video_seconds: u64) -> Self {
        Metered {
            video_seconds,
            ..Metered::default()
        }
    }

    pub fn audio_seconds(audio_seconds: u64) -> Self {
        Metered {
            audio_seconds,
            ..Metered::default()
        }
    }

    pub fn get(&self, meter: Meter) -> u64 {
        match meter {
            Meter::InputTokens => self.input_tokens,
            Meter::OutputTokens => self.output_tokens,
            Meter::ImageTokens => self.image_tokens,
            Meter::Characters => self.characters,
            Meter::VideoSeconds => self.video_seconds,
            Meter::AudioSeconds => self.audio_seconds,
        }
    }

    fn set(&mut self, meter: Meter, value: u64) {
        match meter {
            Meter::InputTokens => self.input_tokens = value,
            Meter::OutputTokens => self.output_tokens = value,
            Meter::ImageTokens => self.image_tokens = value,
            Meter::Characters => self.characters = value,
            Meter::VideoSeconds => self.video_seconds = value,
            Meter::AudioSeconds => self.audio_seconds = value,
        }
    }

    /// The tokens in and out, as provenance records them: drawn images
    /// count as output.
    pub fn tokens(&self) -> TokenUsage {
        TokenUsage {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens + self.image_tokens,
        }
    }

    /// Each meter's average over `samples`, rounded up; `None` without
    /// samples.
    pub fn mean(samples: &[Metered]) -> Option<Metered> {
        let n = samples.len() as u64;
        if n == 0 {
            return None;
        }
        let mut mean = Metered::default();
        for meter in Meter::ALL {
            let sum: u64 = samples.iter().map(|sample| sample.get(meter)).sum();
            mean.set(meter, sum.div_ceil(n));
        }
        Some(mean)
    }

    /// This usage `n` times over.
    pub fn times(&self, n: u64) -> Metered {
        let mut times = *self;
        for meter in Meter::ALL {
            times.set(meter, self.get(meter).saturating_mul(n));
        }
        times
    }
}

impl From<TokenUsage> for Metered {
    fn from(usage: TokenUsage) -> Self {
        Metered {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            ..Metered::default()
        }
    }
}

/// Why a rate cannot be saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum RateFieldError {
    #[error("the model name is too long")]
    ModelTooLong,
    #[error("the model name has spaces")]
    ModelHasSpaces,
    #[error("{0}")]
    Price(MoneyError),
    /// The provider is free (a quota, not money).
    #[error("the provider is not paid")]
    NotPaid,
}

/// The price of one meter for a provider's models: every model whose name
/// starts with `model` (an empty `model` covers all the provider's models).
/// The most specific rate wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rate {
    pub provider: Provider,
    pub model: String,
    pub meter: Meter,
    /// For `meter.units_per_price()` units.
    pub price: Money,
}

impl Rate {
    pub const MAX_MODEL_CHARS: usize = 100;

    /// A rate as the user typed it.
    pub fn new(
        provider: Provider,
        model: &str,
        meter: Meter,
        price: &str,
    ) -> Result<Rate, RateFieldError> {
        if !provider.is_paid() {
            return Err(RateFieldError::NotPaid);
        }
        let model = model.trim();
        if model.chars().count() > Self::MAX_MODEL_CHARS {
            return Err(RateFieldError::ModelTooLong);
        }
        if model.chars().any(char::is_whitespace) {
            return Err(RateFieldError::ModelHasSpaces);
        }
        Ok(Rate {
            provider,
            model: model.to_owned(),
            meter,
            price: Money::parse(price).map_err(RateFieldError::Price)?,
        })
    }

    fn covers(&self, provider: Provider, model: &str) -> bool {
        self.provider == provider && model.starts_with(&self.model)
    }

    fn same_key(&self, other: &Rate) -> bool {
        self.provider == other.provider && self.model == other.model && self.meter == other.meter
    }

    /// What `quantity` units cost, rounded to the nearest millionth.
    pub fn charge(&self, quantity: u64) -> Money {
        let units = u128::from(self.meter.units_per_price());
        let micros = (u128::from(quantity) * u128::from(self.price.micros()) + units / 2) / units;
        Money(u64::try_from(micros).unwrap_or(u64::MAX))
    }
}

/// Prices per provider, model and meter.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RateTable {
    rates: Vec<Rate>,
}

impl RateTable {
    pub fn new(rates: Vec<Rate>) -> Self {
        Self { rates }
    }

    /// The built-in `defaults` with the user's `changes` in place: a change
    /// replaces the default for the same provider, model and meter, and
    /// adds a rate where there is none.
    pub fn with_changes(defaults: &[Rate], changes: &[Rate]) -> Self {
        let mut rates: Vec<Rate> = defaults
            .iter()
            .filter(|default| !changes.iter().any(|change| change.same_key(default)))
            .cloned()
            .collect();
        rates.extend(changes.iter().cloned());
        rates.sort_by(|a, b| (a.provider, &a.model, a.meter).cmp(&(b.provider, &b.model, b.meter)));
        Self { rates }
    }

    pub fn rates(&self) -> &[Rate] {
        &self.rates
    }

    /// The rate `model` of `provider` pays for `meter`: the one with the
    /// longest matching model name.
    pub fn rate(&self, provider: Provider, model: &str, meter: Meter) -> Option<&Rate> {
        self.rates
            .iter()
            .filter(|rate| rate.meter == meter && rate.covers(provider, model))
            .max_by_key(|rate| rate.model.len())
    }

    /// What `usage` costs on `model`, or `None` when a meter it used has no
    /// rate.
    pub fn price(&self, provider: Provider, model: &str, usage: &Metered) -> Option<Money> {
        Meter::ALL
            .into_iter()
            .filter(|meter| usage.get(*meter) > 0)
            .map(|meter| {
                self.rate(provider, model, meter)
                    .map(|rate| rate.charge(usage.get(meter)))
            })
            .sum()
    }
}

/// What a call cost, and how that is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cost {
    /// The provider said what it charged.
    Reported(Money),
    /// Priced from the rate table when it happened.
    Estimated(Money),
    /// No rate covered what it used, so it counts as nothing until one is
    /// added.
    Unpriced,
}

impl Cost {
    /// The provider's figure when it gave one, else the rate table's.
    pub fn of(
        reported: Option<Money>,
        rates: &RateTable,
        provider: Provider,
        model: &str,
        usage: &Metered,
    ) -> Cost {
        match reported {
            Some(amount) => Cost::Reported(amount),
            None => rates
                .price(provider, model, usage)
                .map_or(Cost::Unpriced, Cost::Estimated),
        }
    }

    pub fn amount(&self) -> Money {
        match self {
            Cost::Reported(amount) | Cost::Estimated(amount) => *amount,
            Cost::Unpriced => Money::ZERO,
        }
    }
}

/// What a paid call was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CostPurpose {
    /// Claude proposing themes for a niche.
    ThemeIdeas,
    /// The decision engine ranking themes.
    ThemeRanking,
    Script,
    Narration,
    /// Claude splitting the narration into scenes with image prompts.
    ScenePlan,
    SceneImage,
    /// A video provider animating a scene's image.
    SceneClip,
    /// A provider timing the words of a narration the user recorded.
    NarrationAlignment,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown cost purpose: {0}")]
pub struct UnknownCostPurpose(pub String);

impl CostPurpose {
    pub const ALL: [CostPurpose; 8] = [
        CostPurpose::ThemeIdeas,
        CostPurpose::ThemeRanking,
        CostPurpose::Script,
        CostPurpose::Narration,
        CostPurpose::ScenePlan,
        CostPurpose::SceneImage,
        CostPurpose::SceneClip,
        CostPurpose::NarrationAlignment,
    ];

    /// Stable identifier for storage. Never change one.
    pub fn code(self) -> &'static str {
        match self {
            CostPurpose::ThemeIdeas => "theme_ideas",
            CostPurpose::ThemeRanking => "theme_ranking",
            CostPurpose::Script => "script",
            CostPurpose::Narration => "narration",
            CostPurpose::ScenePlan => "scene_plan",
            CostPurpose::SceneImage => "scene_image",
            CostPurpose::SceneClip => "scene_clip",
            CostPurpose::NarrationAlignment => "narration_alignment",
        }
    }
}

impl fmt::Display for CostPurpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl FromStr for CostPurpose {
    type Err = UnknownCostPurpose;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CostPurpose::ALL
            .into_iter()
            .find(|purpose| purpose.code() == s)
            .ok_or_else(|| UnknownCostPurpose(s.to_owned()))
    }
}

/// One paid provider call. Never changes once saved, and outlives the
/// channel or video it was for: the money was spent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostRecord {
    pub id: CostRecordId,
    pub owner: ProfileId,
    pub provider: Provider,
    /// The model that answered, as the provider named it.
    pub model: String,
    pub purpose: CostPurpose,
    pub usage: Metered,
    pub cost: Cost,
    pub channel: Option<ChannelId>,
    pub project: Option<VideoProjectId>,
    /// The job that made the call.
    pub job: Option<JobId>,
    pub at: SystemTime,
}

impl CostRecord {
    /// A call that had no rate, priced with `rates` once one covers it;
    /// a priced call keeps its cost.
    pub fn priced_with(mut self, rates: &RateTable) -> CostRecord {
        if self.cost == Cost::Unpriced {
            self.cost = Cost::of(None, rates, self.provider, &self.model, &self.usage);
        }
        self
    }
}

/// Records added up.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spend {
    pub total: Money,
    pub providers: BTreeMap<Provider, Money>,
    /// Per channel, most spent first; `None` collects records without one.
    pub channels: Vec<(Option<ChannelId>, Money)>,
    /// Per video project, most spent first.
    pub projects: Vec<(VideoProjectId, Money)>,
    /// The provider models that ran without a rate, each once.
    pub unpriced: Vec<(Provider, String)>,
}

impl Spend {
    pub fn of<'a>(records: impl IntoIterator<Item = &'a CostRecord>) -> Self {
        let mut spend = Spend::default();
        let mut channels: BTreeMap<Option<String>, (Option<ChannelId>, Money)> = BTreeMap::new();
        let mut projects: BTreeMap<String, (VideoProjectId, Money)> = BTreeMap::new();
        for record in records {
            let amount = record.cost.amount();
            spend.total = spend.total.saturating_add(amount);
            let provider = spend.providers.entry(record.provider).or_default();
            *provider = provider.saturating_add(amount);
            let channel = channels
                .entry(record.channel.map(|id| id.to_string()))
                .or_insert((record.channel, Money::ZERO));
            channel.1 = channel.1.saturating_add(amount);
            if let Some(project) = record.project {
                let entry = projects
                    .entry(project.to_string())
                    .or_insert((project, Money::ZERO));
                entry.1 = entry.1.saturating_add(amount);
            }
            if record.cost == Cost::Unpriced
                && !spend
                    .unpriced
                    .iter()
                    .any(|(provider, model)| *provider == record.provider && *model == record.model)
            {
                spend.unpriced.push((record.provider, record.model.clone()));
            }
        }
        spend.channels = channels.into_values().collect();
        spend.channels.sort_by_key(|row| std::cmp::Reverse(row.1));
        spend.projects = projects.into_values().collect();
        spend.projects.sort_by_key(|row| std::cmp::Reverse(row.1));
        spend
    }

    pub fn provider(&self, provider: Provider) -> Money {
        self.providers.get(&provider).copied().unwrap_or_default()
    }
}

/// Persistence port for cost records, the user's rate changes and
/// budgets. Shared with job worker threads.
pub trait CostRepository: Send + Sync {
    fn record_cost(&self, record: &CostRecord) -> Result<(), RepositoryError>;

    /// The profile's records from `from` (inclusive) to `to` (exclusive),
    /// oldest first.
    fn costs_between(
        &self,
        owner: ProfileId,
        from: SystemTime,
        to: SystemTime,
    ) -> Result<Vec<CostRecord>, RepositoryError>;

    /// Every record of a video project, oldest first.
    fn project_costs(&self, project: VideoProjectId) -> Result<Vec<CostRecord>, RepositoryError>;

    /// The profile's latest `limit` records for `purpose`, newest first.
    fn recent_costs(
        &self,
        owner: ProfileId,
        purpose: CostPurpose,
        limit: usize,
    ) -> Result<Vec<CostRecord>, RepositoryError>;

    /// The user's changes to the built-in rates.
    fn rate_changes(&self, owner: ProfileId) -> Result<Vec<Rate>, RepositoryError>;

    /// Saves a change, replacing one for the same provider, model and
    /// meter.
    fn save_rate(&self, owner: ProfileId, rate: &Rate) -> Result<(), RepositoryError>;

    fn remove_rate(
        &self,
        owner: ProfileId,
        provider: Provider,
        model: &str,
        meter: Meter,
    ) -> Result<(), RepositoryError>;

    fn budgets(&self, owner: ProfileId) -> Result<Vec<crate::Budget>, RepositoryError>;

    /// Sets the provider's budget, replacing the one it had.
    fn save_budget(&self, owner: ProfileId, budget: &crate::Budget) -> Result<(), RepositoryError>;

    fn remove_budget(&self, owner: ProfileId, provider: Provider) -> Result<(), RepositoryError>;
}

impl<T: CostRepository + ?Sized> CostRepository for Arc<T> {
    fn record_cost(&self, record: &CostRecord) -> Result<(), RepositoryError> {
        (**self).record_cost(record)
    }

    fn costs_between(
        &self,
        owner: ProfileId,
        from: SystemTime,
        to: SystemTime,
    ) -> Result<Vec<CostRecord>, RepositoryError> {
        (**self).costs_between(owner, from, to)
    }

    fn project_costs(&self, project: VideoProjectId) -> Result<Vec<CostRecord>, RepositoryError> {
        (**self).project_costs(project)
    }

    fn recent_costs(
        &self,
        owner: ProfileId,
        purpose: CostPurpose,
        limit: usize,
    ) -> Result<Vec<CostRecord>, RepositoryError> {
        (**self).recent_costs(owner, purpose, limit)
    }

    fn rate_changes(&self, owner: ProfileId) -> Result<Vec<Rate>, RepositoryError> {
        (**self).rate_changes(owner)
    }

    fn save_rate(&self, owner: ProfileId, rate: &Rate) -> Result<(), RepositoryError> {
        (**self).save_rate(owner, rate)
    }

    fn remove_rate(
        &self,
        owner: ProfileId,
        provider: Provider,
        model: &str,
        meter: Meter,
    ) -> Result<(), RepositoryError> {
        (**self).remove_rate(owner, provider, model, meter)
    }

    fn budgets(&self, owner: ProfileId) -> Result<Vec<crate::Budget>, RepositoryError> {
        (**self).budgets(owner)
    }

    fn save_budget(&self, owner: ProfileId, budget: &crate::Budget) -> Result<(), RepositoryError> {
        (**self).save_budget(owner, budget)
    }

    fn remove_budget(&self, owner: ProfileId, provider: Provider) -> Result<(), RepositoryError> {
        (**self).remove_budget(owner, provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dollars(text: &str) -> Money {
        Money::parse(text).unwrap()
    }

    fn rate(provider: Provider, model: &str, meter: Meter, price: &str) -> Rate {
        Rate::new(provider, model, meter, price).unwrap()
    }

    #[test]
    fn amounts_parse_as_typed() {
        assert_eq!(dollars("12"), Money::from_micros(12_000_000));
        assert_eq!(dollars("0.5"), Money::from_micros(500_000));
        assert_eq!(dollars("3,75"), Money::from_cents(375));
        assert_eq!(dollars(" $4.20 "), Money::from_cents(420));
        assert_eq!(dollars("US$ 4,20"), Money::from_cents(420));
        assert_eq!(dollars(".25"), Money::from_cents(25));
        assert_eq!(dollars("7."), Money::from_cents(700));
        assert_eq!(dollars("0.042"), Money::from_micros(42_000));
        assert_eq!(dollars("0.000001"), Money::from_micros(1));
        assert_eq!(dollars("0"), Money::ZERO);
    }

    #[test]
    fn amounts_print_as_they_are_typed() {
        for (amount, text) in [
            ("12.5", "12.50"),
            ("0.042", "0.042"),
            ("0", "0.00"),
            ("7", "7.00"),
        ] {
            let money = dollars(amount);
            assert_eq!(money.to_string(), text);
            assert_eq!(Money::parse(&money.to_string()), Ok(money));
        }
    }

    #[test]
    fn amounts_that_cannot_be_read_say_why() {
        assert_eq!(Money::parse("  "), Err(MoneyError::Required));
        assert_eq!(Money::parse("$"), Err(MoneyError::Required));
        for invalid in ["abc", "-1", "1.2.3", "1,000.00", "1 000", ".", "1e3", "+2"] {
            assert_eq!(Money::parse(invalid), Err(MoneyError::Invalid), "{invalid}");
        }
        assert_eq!(Money::parse("0.0000001"), Err(MoneyError::TooPrecise));
        assert_eq!(Money::parse("1000000001"), Err(MoneyError::TooLarge));
        assert_eq!(
            Money::parse("99999999999999999999999"),
            Err(MoneyError::TooLarge)
        );
        assert_eq!(dollars("1000000000"), Money::MAX);
    }

    #[test]
    fn money_splits_into_dollars_and_millionths() {
        assert_eq!(dollars("12.345678").split(), (12, 345_678));
        let sum: Money = [dollars("1.5"), dollars("2.25")].into_iter().sum();
        assert_eq!(sum, dollars("3.75"));
        assert_eq!(dollars("1").saturating_sub(dollars("2")), Money::ZERO);
        assert_eq!(dollars("0.05").times(3), dollars("0.15"));
    }

    #[test]
    fn meters_round_trip_through_their_codes() {
        for meter in Meter::ALL {
            assert_eq!(meter.code().parse::<Meter>(), Ok(meter));
        }
        assert!("credits".parse::<Meter>().is_err());
        for purpose in CostPurpose::ALL {
            assert_eq!(purpose.code().parse::<CostPurpose>(), Ok(purpose));
        }
    }

    #[test]
    fn a_rate_charges_per_million_tokens_or_thousand_characters() {
        let input = rate(Provider::Claude, "claude-opus-5-5", Meter::InputTokens, "4");
        assert_eq!(input.charge(1_000_000), dollars("4"));
        assert_eq!(input.charge(812), Money::from_micros(3_248));
        let speech = rate(
            Provider::ElevenLabs,
            "eleven_multilingual_v2",
            Meter::Characters,
            "0.10",
        );
        assert_eq!(speech.charge(1_500), dollars("0.15"));
        // Rounded to the nearest millionth.
        let jev = rate(Provider::TypeSafe, "jev", Meter::InputTokens, "0.042");
        assert_eq!(jev.charge(300), Money::from_micros(13));
        assert_eq!(jev.charge(0), Money::ZERO);
    }

    #[test]
    fn rates_are_checked_as_typed() {
        assert_eq!(
            Rate::new(Provider::YouTubeData, "", Meter::InputTokens, "1"),
            Err(RateFieldError::NotPaid)
        );
        assert_eq!(
            Rate::new(Provider::Claude, "claude opus", Meter::InputTokens, "1"),
            Err(RateFieldError::ModelHasSpaces)
        );
        assert_eq!(
            Rate::new(Provider::Claude, &"m".repeat(101), Meter::InputTokens, "1"),
            Err(RateFieldError::ModelTooLong)
        );
        assert_eq!(
            Rate::new(Provider::Claude, "m", Meter::InputTokens, "x"),
            Err(RateFieldError::Price(MoneyError::Invalid))
        );
        let trimmed = rate(Provider::Claude, "  claude-x  ", Meter::InputTokens, "1");
        assert_eq!(trimmed.model, "claude-x");
    }

    fn table() -> RateTable {
        RateTable::new(vec![
            rate(Provider::Claude, "", Meter::InputTokens, "10"),
            rate(Provider::Claude, "claude-opus-5-5", Meter::InputTokens, "4"),
            rate(
                Provider::Claude,
                "claude-opus-5-5",
                Meter::OutputTokens,
                "20",
            ),
            rate(
                Provider::Gemini,
                "gemini-3.1-flash-image",
                Meter::InputTokens,
                "0.5",
            ),
            rate(
                Provider::Gemini,
                "gemini-3.1-flash-image",
                Meter::OutputTokens,
                "3",
            ),
            rate(
                Provider::Gemini,
                "gemini-3.1-flash-image",
                Meter::ImageTokens,
                "60",
            ),
        ])
    }

    #[test]
    fn the_most_specific_model_rate_wins() {
        let table = table();
        let opus = table
            .rate(Provider::Claude, "claude-opus-5-5", Meter::InputTokens)
            .unwrap();
        assert_eq!(opus.price, dollars("4"));
        // A dated or later version of a model matches its prefix.
        let dated = table
            .rate(
                Provider::Claude,
                "claude-opus-5-5-20261001",
                Meter::InputTokens,
            )
            .unwrap();
        assert_eq!(dated.price, dollars("4"));
        let other = table
            .rate(Provider::Claude, "claude-haiku-4-5", Meter::InputTokens)
            .unwrap();
        assert_eq!(other.price, dollars("10"), "the provider-wide rate");
        assert!(
            table
                .rate(Provider::Claude, "claude-haiku-4-5", Meter::OutputTokens)
                .is_none()
        );
        assert!(
            table
                .rate(Provider::ElevenLabs, "x", Meter::Characters)
                .is_none()
        );
    }

    #[test]
    fn a_price_adds_every_used_meter() {
        let table = table();
        let usage = Metered {
            input_tokens: 812,
            output_tokens: 2_431,
            ..Metered::default()
        };
        // 812 × $4/M + 2,431 × $20/M.
        assert_eq!(
            table.price(Provider::Claude, "claude-opus-5-5", &usage),
            Some(Money::from_micros(3_248 + 48_620))
        );
        let image = Metered {
            input_tokens: 14,
            output_tokens: 218,
            image_tokens: 1_680,
            ..Metered::default()
        };
        assert_eq!(
            table.price(Provider::Gemini, "gemini-3.1-flash-image", &image),
            Some(Money::from_micros(7 + 654 + 100_800))
        );
        // An unused meter needs no rate; a used one without a rate leaves
        // the call unpriced.
        let input_only = Metered {
            input_tokens: 10,
            ..Metered::default()
        };
        assert!(
            table
                .price(Provider::Claude, "claude-haiku-4-5", &input_only)
                .is_some()
        );
        assert_eq!(
            table.price(Provider::Claude, "claude-haiku-4-5", &usage),
            None
        );
        assert_eq!(
            table.price(Provider::Claude, "any", &Metered::default()),
            Some(Money::ZERO)
        );
    }

    #[test]
    fn video_is_priced_per_second_of_the_model() {
        let table = RateTable::new(vec![
            rate(
                Provider::Higgsfield,
                "kling-video/",
                Meter::VideoSeconds,
                "0.07",
            ),
            rate(
                Provider::Higgsfield,
                "kling-video/v3.0/",
                Meter::VideoSeconds,
                "0.112",
            ),
        ]);
        let eight = Metered::video_seconds(8);
        assert_eq!(
            table.price(
                Provider::Higgsfield,
                "kling-video/v3.0/std/image-to-video",
                &eight
            ),
            Some(dollars("0.896"))
        );
        assert_eq!(
            table.price(
                Provider::Higgsfield,
                "kling-video/v2.6/pro/image-to-video",
                &eight
            ),
            Some(dollars("0.56")),
            "the most specific rate wins"
        );
        assert_eq!(Meter::VideoSeconds.code().parse(), Ok(Meter::VideoSeconds));
        assert_eq!("scene_clip".parse(), Ok(CostPurpose::SceneClip));
    }

    #[test]
    fn audio_is_priced_per_hour() {
        let table = RateTable::new(vec![rate(
            Provider::ElevenLabs,
            "forced_alignment",
            Meter::AudioSeconds,
            "0.22",
        )]);
        assert_eq!(
            table.price(
                Provider::ElevenLabs,
                "forced_alignment",
                &Metered::audio_seconds(1_800)
            ),
            Some(dollars("0.11"))
        );
        assert_eq!(
            table.price(
                Provider::ElevenLabs,
                "eleven_multilingual_v2",
                &Metered::audio_seconds(60)
            ),
            None,
            "speech models are not aligners"
        );
        assert_eq!(Meter::AudioSeconds.code().parse(), Ok(Meter::AudioSeconds));
        assert_eq!(
            "narration_alignment".parse(),
            Ok(CostPurpose::NarrationAlignment)
        );
    }

    #[test]
    fn changes_replace_or_add_to_the_defaults() {
        let defaults = vec![
            rate(Provider::Claude, "claude-opus-5-5", Meter::InputTokens, "4"),
            rate(
                Provider::Claude,
                "claude-opus-5-5",
                Meter::OutputTokens,
                "20",
            ),
        ];
        let changes = vec![
            rate(
                Provider::Claude,
                "claude-opus-5-5",
                Meter::InputTokens,
                "3.5",
            ),
            rate(
                Provider::Claude,
                "claude-sonnet-5-5",
                Meter::InputTokens,
                "2",
            ),
        ];
        let table = RateTable::with_changes(&defaults, &changes);
        assert_eq!(table.rates().len(), 3);
        let price = |model: &str, meter: Meter| {
            table
                .rate(Provider::Claude, model, meter)
                .map(|rate| rate.price)
        };
        assert_eq!(
            price("claude-opus-5-5", Meter::InputTokens),
            Some(dollars("3.5"))
        );
        assert_eq!(
            price("claude-opus-5-5", Meter::OutputTokens),
            Some(dollars("20"))
        );
        assert_eq!(
            price("claude-sonnet-5-5", Meter::InputTokens),
            Some(dollars("2"))
        );
    }

    #[test]
    fn a_cost_is_reported_estimated_or_unpriced() {
        let table = table();
        let usage = Metered::from(TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
        });
        assert_eq!(
            Cost::of(
                Some(dollars("1")),
                &table,
                Provider::Claude,
                "claude-opus-5-5",
                &usage
            ),
            Cost::Reported(dollars("1"))
        );
        assert_eq!(
            Cost::of(None, &table, Provider::Claude, "claude-opus-5-5", &usage),
            Cost::Estimated(dollars("4"))
        );
        let cost = Cost::of(
            None,
            &table,
            Provider::ElevenLabs,
            "v2",
            &Metered::characters(5),
        );
        assert_eq!(cost, Cost::Unpriced);
        assert_eq!(cost.amount(), Money::ZERO);
    }

    #[test]
    fn usage_averages_and_multiplies() {
        let samples = [
            Metered {
                input_tokens: 100,
                output_tokens: 1_000,
                ..Metered::default()
            },
            Metered {
                input_tokens: 201,
                output_tokens: 3_000,
                ..Metered::default()
            },
        ];
        let mean = Metered::mean(&samples).unwrap();
        assert_eq!(mean.input_tokens, 151, "rounded up");
        assert_eq!(mean.output_tokens, 2_000);
        assert_eq!(Metered::mean(&[]), None);
        assert_eq!(mean.times(3).output_tokens, 6_000);
        let image = Metered {
            input_tokens: 14,
            output_tokens: 218,
            image_tokens: 1_680,
            ..Metered::default()
        };
        assert_eq!(
            image.tokens(),
            TokenUsage {
                input_tokens: 14,
                output_tokens: 1_898
            }
        );
    }

    fn record(
        provider: Provider,
        model: &str,
        cost: Cost,
        channel: Option<ChannelId>,
        project: Option<VideoProjectId>,
    ) -> CostRecord {
        CostRecord {
            id: CostRecordId::new(),
            owner: ProfileId::new(),
            provider,
            model: model.into(),
            purpose: CostPurpose::Script,
            usage: Metered::default(),
            cost,
            channel,
            project,
            job: None,
            at: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn an_unpriced_call_is_priced_once_a_rate_covers_it() {
        let rates = RateTable::new(vec![rate(
            Provider::Claude,
            "claude",
            Meter::OutputTokens,
            "10",
        )]);
        let mut unpriced = record(Provider::Claude, "claude-x", Cost::Unpriced, None, None);
        unpriced.usage.output_tokens = 100_000;
        assert_eq!(
            unpriced.clone().priced_with(&rates).cost,
            Cost::Estimated(dollars("1"))
        );
        let mut elsewhere = unpriced.clone();
        elsewhere.model = "other".into();
        assert_eq!(elsewhere.priced_with(&rates).cost, Cost::Unpriced);
        let mut kept = unpriced;
        kept.cost = Cost::Estimated(dollars("3"));
        assert_eq!(kept.priced_with(&rates).cost, Cost::Estimated(dollars("3")));
    }

    #[test]
    fn spend_adds_up_per_provider_channel_and_project() {
        let (space, cooking) = (ChannelId::new(), ChannelId::new());
        let (probe, launch) = (VideoProjectId::new(), VideoProjectId::new());
        let records = [
            record(
                Provider::Claude,
                "opus",
                Cost::Estimated(dollars("0.05")),
                Some(space),
                Some(probe),
            ),
            record(
                Provider::ElevenLabs,
                "v2",
                Cost::Estimated(dollars("0.30")),
                Some(space),
                Some(probe),
            ),
            record(
                Provider::Claude,
                "opus",
                Cost::Reported(dollars("0.10")),
                Some(space),
                Some(launch),
            ),
            record(
                Provider::Claude,
                "opus",
                Cost::Estimated(dollars("0.02")),
                Some(cooking),
                None,
            ),
            record(Provider::Gemini, "img", Cost::Unpriced, None, None),
            record(Provider::Gemini, "img", Cost::Unpriced, None, None),
        ];
        let spend = Spend::of(&records);
        assert_eq!(spend.total, dollars("0.47"));
        assert_eq!(spend.provider(Provider::Claude), dollars("0.17"));
        assert_eq!(spend.provider(Provider::ElevenLabs), dollars("0.30"));
        assert_eq!(spend.provider(Provider::Gemini), Money::ZERO);
        assert_eq!(spend.provider(Provider::TypeSafe), Money::ZERO);
        assert_eq!(
            spend.channels,
            [
                (Some(space), dollars("0.45")),
                (Some(cooking), dollars("0.02")),
                (None, Money::ZERO)
            ]
        );
        assert_eq!(
            spend.projects,
            [(probe, dollars("0.35")), (launch, dollars("0.10"))]
        );
        assert_eq!(spend.unpriced, [(Provider::Gemini, "img".to_owned())]);
        assert_eq!(Spend::of(&[]), Spend::default());
    }
}
