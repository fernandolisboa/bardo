//! Theme use cases (PRD stories 25-28): Claude proposes video ideas for a
//! researched niche, the decision engine ranks them with typed reasons
//! (fit, trend, competition), and the user edits, discards or approves
//! each one. Approving starts a video project.
//!
//! Proposing and ranking call providers, so they run as jobs. A suggestion
//! job proposes, saves the ideas and then ranks them; a ranking job only
//! ranks the niche's themes that have no ranking (e.g. after an edit).
//!
//! Once the channel has published videos with a first week of metrics
//! (PRD story 82), the engine also reads how they did and scores a fourth
//! reason, past performance; without history the ranking is the same as
//! before, question for question.

use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, Channel, ChannelId, CostPurpose, DecisionEngine, Decisions, EvidenceScope, Job,
    JobFailure, JobFailureKind, JobId, JobKind, Niche, NicheScores, NicheSeedError,
    PastPerformance, PerformanceEvidence, PerformanceReason, ProfileId, Progress, Provider,
    ProviderFailure, Question, Questions, Reason, RepositoryError, SecretStore, Standing,
    TextFormat, TextGenerator, TextRequest, Theme, ThemeFieldError, ThemeId, ThemeIdea,
    ThemeNotSuggested, ThemeRanking, ThemeRepository, ThemeStatus, UiLanguage, VideoProject,
    rank_themes,
};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, Catalog, KeyState, Text};

/// Ideas one suggestion run asks for.
pub const SUGGESTIONS_PER_RUN: usize = 10;
/// Themes ranked per decision engine call (three questions each).
const RANK_BATCH: usize = 10;
/// Earlier titles shown to Claude so it does not repeat them.
const MAX_TITLES_TO_AVOID: usize = 60;
/// The suggestion job's checkpoint once its ideas are saved.
const PROPOSED: &str = "proposed";
/// The channel's newest published videos the engine reads; older ones
/// still count in the averages.
const MAX_PAST_VIDEOS: usize = 40;

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    /// The typed text breaks one or more rules; the screen shows each one.
    #[error("invalid theme: {0:?}")]
    Invalid(Vec<ThemeFieldError>),
    #[error("invalid niche: {0:?}")]
    InvalidNiche(NicheSeedError),
    #[error("channel not found")]
    ChannelNotFound,
    #[error("theme not found")]
    ThemeNotFound,
    #[error(transparent)]
    NotSuggested(#[from] ThemeNotSuggested),
    /// The work would call this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// A suggestion or ranking job of the channel is still running.
    #[error("theme work is already running for this channel")]
    Busy,
    #[error("no theme is waiting for a ranking")]
    NothingToRank,
    /// The work would reach a provider's budget; the screen asks before
    /// starting it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ThemeError {
    /// What the themes screen says.
    pub fn message(&self) -> Text {
        match self {
            ThemeError::Invalid(errors) => errors
                .first()
                .map_or(Text::ThemeNotSaved, |error| Text::ThemeFieldError(*error)),
            ThemeError::InvalidNiche(_) => Text::ThemesPickNiche,
            ThemeError::ChannelNotFound => Text::ChannelNotFound,
            ThemeError::ThemeNotFound | ThemeError::NotSuggested(_) => Text::ThemeNotFound,
            ThemeError::MissingKey(provider) => Text::ThemesMissingKey(*provider),
            ThemeError::Busy => Text::ThemesBusy,
            ThemeError::NothingToRank => Text::ThemesNothingToRank,
            ThemeError::OverBudget(_) => Text::BudgetReachedTitle,
            ThemeError::Repository(_) => Text::ThemeNotSaved,
        }
    }

    pub fn field_errors(&self) -> &[ThemeFieldError] {
        match self {
            ThemeError::Invalid(errors) => errors,
            _ => &[],
        }
    }
}

/// The themes screen for one channel and niche.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemesView {
    /// The niches ideas can be proposed for: the channel's researched
    /// seeds, or its niche before any research.
    pub niches: Vec<Niche>,
    /// The niche shown, if the channel has any.
    pub niche: Option<Niche>,
    /// The niche's research scores, when researched in the channel's
    /// market.
    pub research: Option<NicheScores>,
    /// Suggested and approved themes of the niche, best first.
    pub themes: Vec<Theme>,
    /// Discarded themes of the niche, hidden from the list.
    pub discarded: usize,
    /// Suggested themes waiting for a ranking.
    pub unranked: usize,
    /// Suggested themes ranked before the channel had history, now that it
    /// has some; ranking again adds past performance to them.
    pub before_history: usize,
    /// The channel's latest suggestion or ranking job.
    pub job: Option<Job>,
    /// The channel's video projects, newest first.
    pub projects: Vec<VideoProject>,
    /// What proposing and ranking new ideas would cost.
    pub suggest_estimate: SpendEstimate,
    /// What ranking the unranked themes (and those ranked before history)
    /// would cost; `None` when none waits.
    pub rank_estimate: Option<SpendEstimate>,
    /// How the channel's published videos did, as a ranking of the niche
    /// would show it now; `None` until one has a first week of metrics.
    pub past: Option<PerformanceEvidence>,
}

/// The paid calls of a suggestion run: Claude proposes, the engine ranks.
fn suggestion_calls() -> [PlannedCall; 2] {
    [
        PlannedCall::new(Provider::Claude, CostPurpose::ThemeIdeas, 1),
        ranking_call(SUGGESTIONS_PER_RUN),
    ]
}

/// The engine's calls to rank `themes` themes.
fn ranking_call(themes: usize) -> PlannedCall {
    PlannedCall::new(
        Provider::TypeSafe,
        CostPurpose::ThemeRanking,
        themes.div_ceil(RANK_BATCH) as u64,
    )
}

/// What providers learn about the channel, fixed when the job starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Brief {
    channel_name: String,
    /// In English, as providers read it best.
    country: String,
    language: String,
    channel_niche: String,
    channel_themes: Vec<String>,
    aesthetic_notes: String,
    research: Option<ResearchFacts>,
    /// The channel's history, when it has some. Absent from payloads
    /// queued before past performance existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    past: Option<PastFacts>,
}

/// How the channel's published videos did, as the engine reads it and as
/// the reason shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PastFacts {
    /// Newest first, at most `MAX_PAST_VIDEOS`.
    videos: Vec<PastVideo>,
    /// Videos left out of the list for being older.
    older: usize,
    /// The channel's usual first week (the median).
    usual: u64,
    evidence: EvidenceFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PastVideo {
    title: String,
    niche: String,
    /// Where it stands against the usual, in the engine's words.
    standing: String,
    views: u64,
    projected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EvidenceFacts {
    /// `EvidenceScope::code`.
    scope: String,
    average_views: u64,
    videos: u32,
    projected: u32,
    basis: u32,
}

impl From<PerformanceEvidence> for EvidenceFacts {
    fn from(evidence: PerformanceEvidence) -> Self {
        Self {
            scope: evidence.scope.code().to_owned(),
            average_views: evidence.average_views,
            videos: evidence.videos,
            projected: evidence.projected,
            basis: evidence.basis,
        }
    }
}

impl EvidenceFacts {
    fn evidence(&self) -> Result<PerformanceEvidence, JobFailure> {
        Ok(PerformanceEvidence {
            scope: EvidenceScope::from_code(&self.scope)
                .ok_or_else(|| JobFailure::unexpected(format!("unknown scope {:?}", self.scope)))?,
            average_views: self.average_views,
            videos: self.videos,
            projected: self.projected,
            basis: self.basis,
        })
    }
}

impl PastFacts {
    /// What the engine reads of `past` when ranking ideas for `niche`;
    /// `None` without history.
    fn of(past: &PastPerformance, niche: &Niche) -> Option<Self> {
        let evidence = past.evidence(niche)?;
        let usual = past.usual().unwrap_or(0);
        let videos = past
            .videos()
            .iter()
            .take(MAX_PAST_VIDEOS)
            .map(|video| PastVideo {
                title: one_line(&video.title),
                niche: video.niche.label().to_owned(),
                standing: describe_standing(Standing::of(video.first_week.views, usual)).to_owned(),
                views: video.first_week.views,
                projected: video.first_week.projected,
            })
            .collect();
        Some(Self {
            videos,
            older: past.videos().len().saturating_sub(MAX_PAST_VIDEOS),
            usual,
            evidence: evidence.into(),
        })
    }

    fn describe(&self) -> String {
        let mut lines = vec![format!(
            "How this channel's own published videos did in their first 7 days, newest first. \
             The channel's usual is its median: {} views.",
            self.usual
        )];
        for video in &self.videos {
            lines.push(format!(
                "- \"{}\" (niche: {}): {}, {} views{}",
                video.title,
                video.niche,
                video.standing,
                video.views,
                if video.projected {
                    ", projected from its first days"
                } else {
                    ""
                }
            ));
        }
        if self.older > 0 {
            lines.push(format!("- and {} older videos", self.older));
        }
        lines.join("\n")
    }
}

/// Where a video stands, as the engine reads it.
fn describe_standing(standing: Standing) -> &'static str {
    match standing {
        Standing::FarBelow => "far below the channel's usual",
        Standing::Below => "below the channel's usual",
        Standing::Usual => "about the channel's usual",
        Standing::Above => "above the channel's usual",
        Standing::FarAbove => "far above the channel's usual",
    }
}

/// `text` on one line with its double quotes made single, so a title cannot
/// break the quoted list it goes into.
fn one_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct ResearchFacts {
    competition: u8,
    trend: u8,
    median_views_per_day: Option<u64>,
    upload_volume: u64,
}

impl Brief {
    /// The channel and niche as the decision engine's state, and the
    /// context Claude writes for.
    fn describe(&self, niche: &str) -> String {
        let mut lines = vec![
            format!("Channel: {}", self.channel_name),
            format!(
                "Audience: viewers in {} watching videos in {}",
                self.country, self.language
            ),
        ];
        if !self.channel_niche.is_empty() {
            lines.push(format!("Channel niche: {}", self.channel_niche));
        }
        lines.push(format!("Niche for these video ideas: {niche}"));
        if !self.channel_themes.is_empty() {
            lines.push(format!(
                "Recurring channel themes: {}",
                self.channel_themes.join("; ")
            ));
        }
        if !self.aesthetic_notes.is_empty() {
            lines.push(format!("Aesthetic notes: {}", self.aesthetic_notes));
        }
        lines.push(match self.research {
            Some(facts) => format!(
                "YouTube research on this niche (uploads of the last 30 days in this market): \
                 competition {}/100 (higher is more crowded), trend {}/100 (higher means views \
                 grow faster), {} median views per day, about {} uploads.",
                facts.competition,
                facts.trend,
                facts
                    .median_views_per_day
                    .map_or_else(|| "unknown".to_owned(), |v| v.to_string()),
                facts.upload_volume,
            ),
            None => "YouTube research on this niche: not run yet.".to_owned(),
        });
        lines.join("\n")
    }

    /// The decision engine's state: the channel and niche, then the
    /// channel's history when it has some. Every question reads it; only
    /// the past performance question asks about the history.
    fn state(&self, niche: &str) -> String {
        let mut state = self.describe(niche);
        if let Some(past) = &self.past {
            state.push_str("\n\n");
            state.push_str(&past.describe());
        }
        state
    }
}

/// The job payload of both theme job kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ThemePayload {
    channel: String,
    niche: String,
    brief: Brief,
}

impl ThemePayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a theme payload serializes")
    }

    fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid theme payload: {e}")))
    }

    fn channel(&self) -> Result<ChannelId, JobFailure> {
        uuid::Uuid::parse_str(&self.channel)
            .map(ChannelId::from)
            .map_err(unexpected)
    }

    fn niche(&self) -> Result<Niche, JobFailure> {
        Niche::new(&self.niche)
            .map_err(|e| JobFailure::unexpected(format!("invalid niche {:?}: {e:?}", self.niche)))
    }
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

fn provider_failure(provider: &str, failure: ProviderFailure) -> JobFailure {
    JobFailure::new(
        failure.kind.into(),
        format!("{provider}: {}", failure.detail),
    )
}

/// The shape Claude answers in.
const PROPOSALS_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "themes": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "title": {"type": "string"},
          "angle": {"type": "string"}
        },
        "required": ["title", "angle"],
        "additionalProperties": false
      }
    }
  },
  "required": ["themes"],
  "additionalProperties": false
}"#;

#[derive(Debug, Deserialize)]
struct Proposals {
    themes: Vec<Proposal>,
}

#[derive(Debug, Deserialize)]
struct Proposal {
    title: String,
    angle: String,
}

fn proposal_request(brief: &Brief, niche: &Niche, avoid: &[&str]) -> TextRequest {
    let instructions = format!(
        "You develop video ideas for a faceless YouTube channel: narration over visuals, no \
         on-camera host. Every idea is one specific story, question or event, not a broad \
         topic. Ideas are original rather than remakes of well-known videos, accurate where \
         they touch real events, and safe for advertisers. Write titles and angles in {}, for \
         viewers in {}.",
        brief.language, brief.country
    );
    let mut prompt = brief.describe(niche.label());
    if !avoid.is_empty() {
        prompt.push_str("\n\nAlready proposed for this channel; do not repeat or rephrase:\n");
        for title in avoid {
            prompt.push_str(&format!("- {title}\n"));
        }
    }
    prompt.push_str(&format!(
        "\n\nPropose {SUGGESTIONS_PER_RUN} video ideas for the niche \"{}\". For each, give a \
         working title under 80 characters and the angle: what the video covers and why \
         viewers would click, in one or two sentences under 300 characters.",
        niche.label()
    ));
    TextRequest {
        instructions,
        prompt,
        format: TextFormat::Json {
            schema: PROPOSALS_SCHEMA.to_owned(),
        },
    }
}

/// The usable ideas of Claude's answer: valid, new to the channel and
/// distinct, at most `SUGGESTIONS_PER_RUN`.
fn usable_ideas(text: &str, existing: &[Theme]) -> Result<Vec<ThemeIdea>, JobFailure> {
    let proposals: Proposals = serde_json::from_str(text).map_err(|e| {
        JobFailure::new(
            JobFailureKind::UnexpectedAnswer,
            format!("Claude: unreadable ideas: {e}"),
        )
    })?;
    let mut ideas: Vec<ThemeIdea> = Vec::new();
    for proposal in proposals.themes {
        let Ok(idea) = ThemeIdea::new(&proposal.title, &proposal.angle) else {
            continue;
        };
        let repeated = existing
            .iter()
            .any(|theme| theme.idea().same_title(idea.title()))
            || ideas.iter().any(|other| other.same_title(idea.title()));
        if !repeated {
            ideas.push(idea);
        }
    }
    ideas.truncate(SUGGESTIONS_PER_RUN);
    if ideas.is_empty() {
        return Err(JobFailure::new(
            JobFailureKind::UnexpectedAnswer,
            "Claude: no usable idea in the answer",
        ));
    }
    Ok(ideas)
}

/// The questions behind a ranking, asked about each theme: three always,
/// past performance when the channel has history.
#[derive(Clone, Copy)]
enum Aspect {
    Fit,
    Trend,
    Competition,
    Performance,
}

impl Aspect {
    const ALL: [Aspect; 4] = [
        Aspect::Fit,
        Aspect::Trend,
        Aspect::Competition,
        Aspect::Performance,
    ];

    /// The questions asked about each theme: past performance last, and
    /// only with history.
    fn asked(with_history: bool) -> &'static [Aspect] {
        if with_history {
            &Self::ALL
        } else {
            &Self::ALL[..3]
        }
    }

    fn id(self, index: usize) -> String {
        let name = match self {
            Aspect::Fit => "fit",
            Aspect::Trend => "trend",
            Aspect::Competition => "competition",
            Aspect::Performance => "performance",
        };
        format!("t{index}_{name}")
    }

    fn question(self, idea: &ThemeIdea) -> Question {
        let (ask, levels) = match self {
            Aspect::Fit => (
                "How well does this video idea fit the channel described in the state: its \
                 niche, recurring themes, aesthetic and audience?",
                [
                    "Does not fit the channel",
                    "Barely related to the channel",
                    "Related but off-center",
                    "Fits the channel well",
                    "A perfect fit for the channel",
                ],
            ),
            Aspect::Trend => (
                "How much demand does this video idea have right now among viewers in the \
                 channel's audience? Use the niche research in the state as context.",
                [
                    "Almost nobody looks for this",
                    "Little interest",
                    "Steady interest",
                    "Strong interest",
                    "Hot right now",
                ],
            ),
            Aspect::Competition => (
                "How crowded is this specific angle on YouTube for the channel's audience: how \
                 many established channels already cover it well? Use the niche research in the \
                 state as context.",
                [
                    "Nobody covers it",
                    "A few small channels cover it",
                    "Several channels cover it",
                    "Many channels cover it",
                    "Saturated by big channels",
                ],
            ),
            Aspect::Performance => (
                "The state lists how this channel's own published videos did in their first 7 \
                 days against the channel's usual. Judging by the videos most like this idea in \
                 topic and angle, how would this idea do on this channel?",
                [
                    "Far below the channel's usual",
                    "Below the channel's usual",
                    "About the channel's usual",
                    "Above the channel's usual",
                    "Far above the channel's usual",
                ],
            ),
        };
        let idea = if idea.angle().is_empty() {
            format!("\"{}\"", idea.title())
        } else {
            format!("\"{}\": {}", idea.title(), idea.angle())
        };
        Question::Score {
            instructions: format!("Video idea: {idea}\n{ask}"),
            levels: levels.map(String::from).to_vec(),
        }
    }
}

fn reason(decisions: &Decisions, id: &str) -> Result<Reason, JobFailure> {
    let answer = decisions.score(id).ok_or_else(|| {
        JobFailure::new(
            JobFailureKind::UnexpectedAnswer,
            format!("decision engine: no answer for {id}"),
        )
    })?;
    Ok(Reason {
        score: answer.normalized(),
        confidence: answer.confidence,
    })
}

/// Whether a ranking job takes `theme`: a suggested theme of `niche` with
/// no ranking, or, `with_history`, one ranked before the channel had any.
fn waits_for_ranking(theme: &Theme, niche: &Niche, with_history: bool) -> bool {
    theme.status() == ThemeStatus::Suggested
        && theme.niche.key() == niche.key()
        && theme
            .ranking()
            .is_none_or(|ranking| with_history && ranking.performance.is_none())
}

/// Runs both theme job kinds.
pub(crate) struct ThemeHandler {
    pub(crate) owner: ProfileId,
    pub(crate) themes: Arc<dyn ThemeRepository>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) decisions: Arc<dyn DecisionEngine>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl ThemeHandler {
    fn key(&self, provider: Provider) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, provider)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(
                    JobFailureKind::MissingKey,
                    format!("no {provider} key is saved"),
                )
            })
    }

    /// Has Claude propose ideas and saves them, unless an earlier attempt
    /// of this job already did (it stopped before its checkpoint).
    fn propose(
        &self,
        payload: &ThemePayload,
        channel: ChannelId,
        niche: &Niche,
        job: JobId,
    ) -> Result<(), JobFailure> {
        let existing = self.themes.themes(channel).map_err(unexpected)?;
        if existing
            .iter()
            .any(|theme| theme.suggestion_job == Some(job))
        {
            return Ok(());
        }
        let mut same_niche: Vec<&Theme> = existing
            .iter()
            .filter(|theme| theme.niche.key() == niche.key())
            .collect();
        same_niche.sort_by_key(|theme| std::cmp::Reverse(theme.suggested_at));
        let avoid: Vec<&str> = same_niche
            .iter()
            .take(MAX_TITLES_TO_AVOID)
            .map(|theme| theme.idea().title())
            .collect();

        let key = self.key(Provider::Claude)?;
        let request = proposal_request(&payload.brief, niche, &avoid);
        let generated = self
            .text
            .generate(&key, &request)
            .map_err(|failure| provider_failure("Claude", failure))?;
        self.costs.record(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::ThemeIdeas,
                usage: generated.usage.into(),
                job: Some(job),
                reported: None,
            },
            Some(channel),
            None,
        );
        let ideas = usable_ideas(&generated.text, &existing)?;

        let now = SystemTime::now();
        let themes: Vec<Theme> = ideas
            .into_iter()
            .enumerate()
            .map(|(position, idea)| {
                Theme::suggested(
                    self.owner,
                    channel,
                    niche.clone(),
                    idea,
                    now,
                    position as u32,
                    Some(job),
                )
            })
            .collect();
        self.themes.save_themes(&themes).map_err(unexpected)
    }

    /// Ranks the niche's suggested themes that have no ranking, a batch per
    /// call; a ranking job with history also ranks again those ranked
    /// before the channel had any. Each batch is saved as it arrives, so a
    /// resumed job ranks only what is left.
    fn rank(
        &self,
        payload: &ThemePayload,
        channel: ChannelId,
        niche: &Niche,
        cx: &JobContext,
        progress_from: u16,
    ) -> Result<(), JobFailure> {
        let evidence = payload
            .brief
            .past
            .as_ref()
            .map(|past| past.evidence.evidence())
            .transpose()?;
        let with_history = cx.kind() == JobKind::ThemeRanking && evidence.is_some();
        let pending: Vec<Theme> = self
            .themes
            .themes(channel)
            .map_err(unexpected)?
            .into_iter()
            .filter(|theme| waits_for_ranking(theme, niche, with_history))
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        let key = self.key(Provider::TypeSafe)?;
        let state = payload.brief.state(niche.label());
        let aspects = Aspect::asked(evidence.is_some());
        let total = pending.len() as u64;
        let mut done = 0;

        for batch in pending.chunks(RANK_BATCH) {
            if cx.should_stop() {
                return Ok(());
            }
            let mut questions = Questions::new();
            for (index, theme) in batch.iter().enumerate() {
                for &aspect in aspects {
                    questions = questions
                        .ask(aspect.id(index), aspect.question(theme.idea()))
                        .map_err(unexpected)?;
                }
            }
            let decisions = self
                .decisions
                .decide(&key, &state, &questions)
                .map_err(|failure| provider_failure("decision engine", failure))?;
            self.costs.record(
                PaidCall {
                    provider: Provider::TypeSafe,
                    model: &decisions.model,
                    purpose: CostPurpose::ThemeRanking,
                    usage: decisions.usage.into(),
                    job: Some(cx.id()),
                    reported: None,
                },
                Some(channel),
                None,
            );
            let ranked_at = SystemTime::now();

            let mut ranked = Vec::new();
            for (index, theme) in batch.iter().enumerate() {
                let ranking = ThemeRanking {
                    fit: reason(&decisions, &Aspect::Fit.id(index))?,
                    trend: reason(&decisions, &Aspect::Trend.id(index))?,
                    competition: reason(&decisions, &Aspect::Competition.id(index))?,
                    performance: evidence
                        .map(|evidence| -> Result<PerformanceReason, JobFailure> {
                            Ok(PerformanceReason {
                                reason: reason(&decisions, &Aspect::Performance.id(index))?,
                                evidence,
                            })
                        })
                        .transpose()?,
                    model: decisions.model.clone(),
                    ranked_at,
                };
                // The user may have edited, discarded or approved it while
                // the engine was answering; their change wins.
                let current = self.themes.theme(theme.id).map_err(unexpected)?;
                if let Some(mut current) = current
                    && current.idea() == theme.idea()
                    && current.ranking() == theme.ranking()
                    && current.rank(ranking).is_ok()
                {
                    ranked.push(current);
                }
            }
            self.themes.save_themes(&ranked).map_err(unexpected)?;
            done += batch.len() as u64;
            let span = u64::from(1000 - progress_from);
            let permille = u64::from(progress_from) + span * done / total;
            cx.report_progress(Progress::from_permille(permille as u16));
        }
        Ok(())
    }
}

impl JobHandler for ThemeHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload = ThemePayload::parse(payload)?;
        let channel = payload.channel()?;
        let niche = payload.niche()?;
        let mut progress_from = 0;
        if cx.kind() == JobKind::ThemeSuggestion {
            progress_from = 500;
            if cx.checkpoint() != Some(PROPOSED) {
                self.propose(&payload, channel, &niche, cx.id())?;
                cx.save_checkpoint(PROPOSED, Progress::from_permille(progress_from))
                    .map_err(unexpected)?;
            }
        }
        if cx.should_stop() {
            return Ok(());
        }
        self.rank(&payload, channel, &niche, cx, progress_from)
    }
}

impl Bardo {
    fn theme_channel(&self, id: ChannelId) -> Result<Channel, ThemeError> {
        self.channels
            .get(id)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(ThemeError::ChannelNotFound)
    }

    fn own_theme(&self, id: ThemeId) -> Result<Theme, ThemeError> {
        self.themes
            .theme(id)?
            .filter(|theme| theme.owner == self.profile.id)
            .ok_or(ThemeError::ThemeNotFound)
    }

    /// The niches themes can be proposed for: the last research run's
    /// seeds, or the channel's niche before any run.
    fn theme_niches(&self, channel: &Channel) -> Result<Vec<Niche>, ThemeError> {
        let mut niches = self.research.seeds(channel.id)?;
        if niches.is_empty()
            && let Ok(niche) = Niche::new(channel.details.niche())
        {
            niches.push(niche);
        }
        Ok(niches)
    }

    fn niche_scores(
        &self,
        channel: &Channel,
        niche: &Niche,
    ) -> Result<Option<NicheScores>, ThemeError> {
        Ok(self
            .research
            .cached(self.profile.id, niche, channel.details.market())?
            .and_then(|research| research.scores()))
    }

    fn latest_theme_job(&self, channel: ChannelId) -> Option<Job> {
        let channel = channel.to_string();
        self.jobs().into_iter().rev().find(|job| {
            matches!(job.kind(), JobKind::ThemeSuggestion | JobKind::ThemeRanking)
                && ThemePayload::parse(job.payload()).is_ok_and(|p| p.channel == channel)
        })
    }

    fn ensure_idle(&self, channel: ChannelId) -> Result<(), ThemeError> {
        match self.latest_theme_job(channel) {
            Some(job) if job.state().is_active() => Err(ThemeError::Busy),
            _ => Ok(()),
        }
    }

    fn ensure_key(&self, provider: Provider) -> Result<(), ThemeError> {
        match self.provider_key(provider).state {
            KeyState::NotSet => Err(ThemeError::MissingKey(provider)),
            _ => Ok(()),
        }
    }

    /// The first weeks of the channel's published video `projects`.
    fn past_performance(
        &self,
        channel: ChannelId,
        projects: &[VideoProject],
    ) -> Result<PastPerformance, ThemeError> {
        Ok(PastPerformance::of_channel(
            projects,
            &self.publications.channel_publications(channel)?,
            &self.publications.channel_snapshots(channel)?,
        ))
    }

    fn brief(&self, channel: &Channel, niche: &Niche) -> Result<Brief, ThemeError> {
        let research = self
            .research
            .cached(self.profile.id, niche, channel.details.market())?;
        let facts = research.as_ref().and_then(|research| {
            let scores = research.scores()?;
            let statistics = research.statistics();
            Some(ResearchFacts {
                competition: scores.competition.value(),
                trend: scores.trend.value(),
                median_views_per_day: statistics.median_views_per_day,
                upload_volume: statistics.upload_volume,
            })
        });
        let english = Catalog::load(UiLanguage::EnUs);
        let details = &channel.details;
        Ok(Brief {
            channel_name: details.name().to_owned(),
            country: english
                .get(Text::CountryName(details.country()))
                .into_owned(),
            language: english
                .get(Text::ContentLanguageName(details.language()))
                .into_owned(),
            channel_niche: details.niche().to_owned(),
            channel_themes: details.themes().to_vec(),
            aesthetic_notes: details.aesthetic_notes().to_owned(),
            research: facts,
            past: PastFacts::of(
                &self.past_performance(channel.id, &self.themes.projects(channel.id)?)?,
                niche,
            ),
        })
    }

    fn enqueue_theme_job(
        &self,
        kind: JobKind,
        channel: &Channel,
        niche: &Niche,
    ) -> Result<JobId, ThemeError> {
        let payload = ThemePayload {
            channel: channel.id.to_string(),
            niche: niche.label().to_owned(),
            brief: self.brief(channel, niche)?,
        };
        let job = Job::new(self.profile.id, kind, payload.to_json());
        Ok(self.jobs.enqueue(job)?)
    }

    /// The themes screen for the channel. `niche` picks the niche shown;
    /// without it, the first of `ThemesView::niches`.
    pub fn themes(
        &self,
        channel: ChannelId,
        niche: Option<&str>,
    ) -> Result<ThemesView, ThemeError> {
        let channel = self.theme_channel(channel)?;
        let niches = self.theme_niches(&channel)?;
        let niche = niche
            .and_then(|label| Niche::new(label).ok())
            .or_else(|| niches.first().cloned());
        let job = self.latest_theme_job(channel.id);
        let projects = self.themes.projects(channel.id)?;
        let suggest_estimate = self.estimate(&suggestion_calls())?;
        let Some(niche) = niche else {
            return Ok(ThemesView {
                niches,
                niche: None,
                research: None,
                themes: Vec::new(),
                discarded: 0,
                unranked: 0,
                before_history: 0,
                job,
                projects,
                suggest_estimate,
                rank_estimate: None,
                past: None,
            });
        };

        let all: Vec<Theme> = self
            .themes
            .themes(channel.id)?
            .into_iter()
            .filter(|theme| theme.niche.key() == niche.key())
            .collect();
        let discarded = all
            .iter()
            .filter(|theme| theme.status() == ThemeStatus::Discarded)
            .count();
        let past = self
            .past_performance(channel.id, &projects)?
            .evidence(&niche);
        let unranked = all
            .iter()
            .filter(|theme| waits_for_ranking(theme, &niche, false))
            .count();
        let waiting = all
            .iter()
            .filter(|theme| waits_for_ranking(theme, &niche, past.is_some()))
            .count();
        let mut themes: Vec<Theme> = all
            .into_iter()
            .filter(|theme| theme.status() != ThemeStatus::Discarded)
            .collect();
        rank_themes(&mut themes);

        Ok(ThemesView {
            research: self.niche_scores(&channel, &niche)?,
            past,
            niches,
            niche: Some(niche),
            themes,
            discarded,
            unranked,
            before_history: waiting - unranked,
            job,
            projects,
            suggest_estimate,
            rank_estimate: match waiting {
                0 => None,
                n => Some(self.estimate(&[ranking_call(n)])?),
            },
        })
    }

    /// Past a budget without `consent`, the estimate to ask about.
    fn theme_budget(
        &self,
        calls: &[PlannedCall],
        consent: BudgetConsent,
    ) -> Result<(), ThemeError> {
        self.check_budget(calls, consent)?
            .map_err(ThemeError::OverBudget)
    }

    /// Starts a job in which Claude proposes ideas for the niche and the
    /// decision engine ranks them. Past Claude's or TypeSafe's budget it
    /// needs `consent`.
    pub fn suggest_themes(
        &self,
        channel: ChannelId,
        niche: &str,
        consent: BudgetConsent,
    ) -> Result<JobId, ThemeError> {
        let channel = self.theme_channel(channel)?;
        let niche = Niche::new(niche).map_err(ThemeError::InvalidNiche)?;
        self.ensure_idle(channel.id)?;
        self.ensure_key(Provider::Claude)?;
        self.ensure_key(Provider::TypeSafe)?;
        self.theme_budget(&suggestion_calls(), consent)?;
        self.enqueue_theme_job(JobKind::ThemeSuggestion, &channel, &niche)
    }

    /// Starts a job ranking the niche's suggested themes that have no
    /// ranking (edited ones, or ones a failed job left behind) and, when
    /// the channel has history, those ranked before it had. Past
    /// TypeSafe's budget it needs `consent`.
    pub fn rank_themes(
        &self,
        channel: ChannelId,
        niche: &str,
        consent: BudgetConsent,
    ) -> Result<JobId, ThemeError> {
        let channel = self.theme_channel(channel)?;
        let niche = Niche::new(niche).map_err(ThemeError::InvalidNiche)?;
        self.ensure_idle(channel.id)?;
        let with_history = self
            .past_performance(channel.id, &self.themes.projects(channel.id)?)?
            .evidence(&niche)
            .is_some();
        let waiting = self
            .themes
            .themes(channel.id)?
            .iter()
            .filter(|theme| waits_for_ranking(theme, &niche, with_history))
            .count();
        if waiting == 0 {
            return Err(ThemeError::NothingToRank);
        }
        self.ensure_key(Provider::TypeSafe)?;
        self.theme_budget(&[ranking_call(waiting)], consent)?;
        self.enqueue_theme_job(JobKind::ThemeRanking, &channel, &niche)
    }

    /// Replaces a suggested theme's text. A changed text loses its ranking
    /// until ranked again.
    pub fn edit_theme(&self, id: ThemeId, title: &str, angle: &str) -> Result<Theme, ThemeError> {
        let mut theme = self.own_theme(id)?;
        let idea = ThemeIdea::new(title, angle).map_err(ThemeError::Invalid)?;
        theme.edit(idea)?;
        self.themes.save_themes(std::slice::from_ref(&theme))?;
        Ok(theme)
    }

    pub fn discard_theme(&self, id: ThemeId) -> Result<(), ThemeError> {
        let mut theme = self.own_theme(id)?;
        theme.discard()?;
        self.themes.save_themes(std::slice::from_ref(&theme))?;
        Ok(())
    }

    /// The numbers behind a past performance reason, as a sentence in the
    /// interface language.
    pub fn performance_evidence(&self, evidence: &PerformanceEvidence) -> String {
        let text = match (evidence.scope, evidence.videos) {
            (EvidenceScope::Niche, 1) => Text::ThemePerformanceNicheOne,
            (EvidenceScope::Niche, _) => Text::ThemePerformanceNiche,
            (EvidenceScope::Channel, 1) => Text::ThemePerformanceChannelOne,
            (EvidenceScope::Channel, _) => Text::ThemePerformanceChannel,
        };
        let mut sentence = self.text_with(
            text,
            &[
                ("views", &self.compact_count(evidence.average_views)),
                ("n", &evidence.videos.to_string()),
            ],
        );
        if evidence.projected > 0 {
            sentence.push(' ');
            sentence.push_str(&self.text_with(
                Text::ThemePerformanceProjected,
                &[("n", &evidence.projected.to_string())],
            ));
        }
        sentence
    }

    /// Approves a suggested theme and starts its video project, linked to
    /// the channel, niche and theme.
    pub fn approve_theme(&self, id: ThemeId) -> Result<VideoProject, ThemeError> {
        let mut theme = self.own_theme(id)?;
        let project = theme.approve(SystemTime::now())?;
        self.themes.start_project(&theme, &project)?;
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use bardo_domain::{
        ChannelDraft, ContentLanguage, Country, JobRepository, JobState, ProviderFailureKind, Score,
    };
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::testing::{FakeDecisionEngine, FakeKeyChecker, FakeMarketData, FakeTextGenerator};
    use crate::{JobSettings, Providers, Repositories};

    const CLAUDE_KEY: &str = "sk-ant-api03-test-key-0001";
    const TYPESAFE_KEY: &str = "ts-test-key-0001-abcdef";
    const PATIENCE: Duration = Duration::from_secs(10);

    struct Harness {
        db: Arc<Database>,
        text: Arc<FakeTextGenerator>,
        decisions: Arc<FakeDecisionEngine>,
        secrets: Arc<MemorySecretStore>,
    }

    impl Harness {
        fn new() -> Self {
            Self::with_db(Arc::new(Database::open_in_memory().unwrap()))
        }

        fn with_db(db: Arc<Database>) -> Self {
            Self {
                db,
                text: Arc::default(),
                decisions: Arc::default(),
                secrets: Arc::default(),
            }
        }

        /// Another app over the same providers and secrets, on `path`.
        fn on_disk(path: &std::path::Path, from: &Harness) -> Self {
            Self {
                db: Arc::new(Database::open(path).unwrap()),
                text: Arc::clone(&from.text),
                decisions: Arc::clone(&from.decisions),
                secrets: Arc::clone(&from.secrets),
            }
        }

        fn start(&self) -> Bardo {
            let repositories = Repositories {
                profiles: Box::new(Arc::clone(&self.db)),
                tours: Box::new(Arc::clone(&self.db)),
                agent_task: Arc::new(bardo_storage::MemoryAgentTask::default()),
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
                connections: Arc::clone(&self.db) as _,
                renders: Arc::clone(&self.db) as _,
                exports: Arc::clone(&self.db) as _,
                publications: Arc::clone(&self.db) as _,
                cut_suggestions: Arc::clone(&self.db) as _,
                export_files: Arc::new(bardo_storage::MemoryExportFiles::default()),
                costs: Arc::clone(&self.db) as _,
                files: Arc::new(bardo_storage::MemoryProjectFiles::default()),
                voice_samples: Arc::new(bardo_storage::MemoryVoiceSamples::default()),
                research: Arc::clone(&self.db) as _,
                secrets: Arc::clone(&self.secrets) as _,
                connection_secrets: Arc::new(MemorySecretStore::default()),
            };
            let providers = Providers {
                key_checker: Arc::new(FakeKeyChecker::default()),
                market_data: Arc::new(FakeMarketData::default()),
                video_stats: Arc::new(crate::testing::FakeVideoStats::default()),
                text: Arc::clone(&self.text) as _,
                decisions: Arc::clone(&self.decisions) as _,
                voices: Arc::new(crate::testing::FakeVoiceLibrary::default()),
                speech: Arc::new(crate::testing::FakeSpeech::default()),
                previews: Arc::new(crate::testing::NoPreviews),
                aligner: Arc::new(crate::narration_import::testing::FakeAligner::default()),
                images: Arc::new(crate::testing::FakeImages::default()),
                clips: vec![Arc::new(crate::testing::FakeClips::default())],
                audio: Arc::new(crate::narrations::testing::FakeAudioOutput::default()),
                media: Arc::new(crate::editor::testing::FakeMedia::default()),
                sign_ins: Vec::new(),
                consent: Arc::new(crate::connections::testing::NoConsent),
                uploaders: Vec::new(),
                analytics: Vec::new(),
                post_insights: Vec::new(),
            };
            Bardo::start_with(
                repositories,
                providers,
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

        /// Started with Claude and TypeSafe keys saved.
        fn start_with_keys(&self) -> Bardo {
            let mut app = self.start();
            app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
            app.save_provider_key(Provider::TypeSafe, TYPESAFE_KEY)
                .unwrap();
            app
        }
    }

    fn channel(app: &Bardo, niche: &str) -> Channel {
        app.create_channel(ChannelDraft {
            name: "Space Archives".into(),
            niche: niche.into(),
            themes: vec!["lost missions".into()],
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

    fn wait_until(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !done() {
            assert!(Instant::now() < deadline, "{what} never happened");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Suggests themes for the channel's niche and waits for the job.
    fn suggest(app: &Bardo, channel: &Channel) -> Job {
        let id = app
            .suggest_themes(channel.id, channel.details.niche(), BudgetConsent::Ask)
            .unwrap();
        wait_done(app, id)
    }

    fn titles(view: &ThemesView) -> Vec<&str> {
        view.themes
            .iter()
            .map(|theme| theme.idea().title())
            .collect()
    }

    fn quoted(title: &str) -> String {
        format!("\"{title}\"")
    }

    #[test]
    fn suggestions_run_as_a_job_and_come_back_ranked_with_reasons() {
        let h = Harness::new();
        h.decisions.set_level(&quoted("Idea 3"), 4.0);
        h.decisions.set_level(&quoted("Idea 7"), 0.0);
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");

        let job = suggest(&app, &channel);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(job.kind(), JobKind::ThemeSuggestion);

        let view = app.themes(channel.id, None).unwrap();
        assert_eq!(view.niche.as_ref().map(Niche::label), Some("space history"));
        assert_eq!(view.themes.len(), SUGGESTIONS_PER_RUN);
        assert_eq!(view.unranked, 0);
        assert_eq!(view.job.as_ref().map(Job::id), Some(job.id()));
        let titles = titles(&view);
        assert_eq!(titles[0], "Idea 3", "the strongest idea leads");
        assert_eq!(titles[9], "Idea 7", "the weakest idea comes last");
        assert_eq!(titles[1], "Idea 1", "ties keep Claude's order");

        let best = view.themes[0].ranking().unwrap();
        assert_eq!(best.fit.score, Score::new(100));
        assert_eq!(best.trend.score, Score::new(100));
        assert_eq!(best.competition.score, Score::new(100));
        assert_eq!(best.priority(), Score::new(75));
        assert_eq!(best.confidence().percent(), 80);
        assert_eq!(best.model, "jev-fake");
        let middle = view.themes[1].ranking().unwrap();
        assert_eq!(middle.priority(), Score::new(50));
        assert_eq!(view.themes[0].idea().angle(), "Why Idea 3 matters.");
        assert!(
            view.themes
                .iter()
                .all(|theme| theme.status() == ThemeStatus::Suggested
                    && theme.channel == channel.id
                    && theme.suggestion_job == Some(job.id()))
        );
    }

    #[test]
    fn suggesting_records_what_claude_and_jev_cost() {
        use bardo_domain::{CostPurpose, CostRepository};

        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        let estimate = app.themes(channel.id, None).unwrap().suggest_estimate;
        let providers: Vec<_> = estimate.providers.iter().map(|p| p.provider).collect();
        assert_eq!(providers, [Provider::Claude, Provider::TypeSafe]);

        let job = suggest(&app, &channel);
        let records =
            h.db.costs_between(app.profile().id, SystemTime::UNIX_EPOCH, SystemTime::now())
                .unwrap();
        let ranking_calls = SUGGESTIONS_PER_RUN.div_ceil(RANK_BATCH);
        assert_eq!(records.len(), 1 + ranking_calls);
        assert_eq!(
            (records[0].provider, records[0].purpose),
            (Provider::Claude, CostPurpose::ThemeIdeas)
        );
        assert!(
            records[1..]
                .iter()
                .all(|record| record.provider == Provider::TypeSafe
                    && record.purpose == CostPurpose::ThemeRanking
                    && record.model == "jev-fake"
                    && record.usage.input_tokens > 0)
        );
        assert!(
            records
                .iter()
                .all(|record| record.channel == Some(channel.id)
                    && record.project.is_none()
                    && record.job == Some(job.id()))
        );
    }

    #[test]
    fn providers_learn_about_the_channel_and_niche() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);

        let requests = h.text.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(matches!(request.format, TextFormat::Json { .. }));
        assert!(
            request.instructions.contains("Portuguese"),
            "{}",
            request.instructions
        );
        for fact in [
            "Space Archives",
            "Brazil",
            "space history",
            "lost missions",
            "not run yet",
        ] {
            assert!(request.prompt.contains(fact), "{fact}: {}", request.prompt);
        }

        let calls = h.decisions.calls();
        assert_eq!(calls.len(), 1, "ten ideas fit one decision call");
        let (state, questions) = &calls[0];
        assert!(state.contains("Space Archives") && state.contains("space history"));
        assert_eq!(questions.len(), 3 * SUGGESTIONS_PER_RUN);
        assert!(questions.get("t0_fit").is_some());
        assert!(questions.get("t9_competition").is_some());
    }

    #[test]
    fn later_runs_avoid_earlier_ideas() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);

        h.text
            .answer_with(&["idea  1", "Fresh idea", "Fresh Idea", ""]);
        let job = suggest(&app, &channel);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());

        let prompt = &h.text.requests()[1].prompt;
        assert!(prompt.contains("- Idea 10"), "{prompt}");
        let view = app.themes(channel.id, None).unwrap();
        assert_eq!(view.themes.len(), SUGGESTIONS_PER_RUN + 1);
        assert!(titles(&view).contains(&"Fresh idea"));
        let calls = h.decisions.calls();
        assert_eq!(calls[1].1.len(), 3, "only the new idea is ranked");
    }

    #[test]
    fn an_answer_without_usable_ideas_fails_the_job() {
        let h = Harness::new();
        h.text.answers.lock().unwrap().push("not json".into());
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");

        let job = suggest(&app, &channel);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::UnexpectedAnswer
        );
        assert!(app.themes(channel.id, None).unwrap().themes.is_empty());
    }

    #[test]
    fn a_declined_request_fails_without_retrying() {
        let h = Harness::new();
        *h.text.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Declined,
            "refused",
        ));
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");

        let job = suggest(&app, &channel);
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::Declined);
        assert_eq!(failure.detail, "Claude: refused");
        assert_eq!(h.text.requests().len(), 1);
    }

    #[test]
    fn a_failed_ranking_keeps_the_ideas_for_a_later_ranking() {
        let h = Harness::new();
        *h.decisions.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::LimitReached,
            "Rate limit exceeded.",
        ));
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");

        let job = suggest(&app, &channel);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::LimitReached);
        let view = app.themes(channel.id, None).unwrap();
        assert_eq!(view.themes.len(), SUGGESTIONS_PER_RUN);
        assert_eq!(view.unranked, SUGGESTIONS_PER_RUN);

        *h.decisions.failure.lock().unwrap() = None;
        let id = app
            .rank_themes(channel.id, "space history", BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(job.kind(), JobKind::ThemeRanking);
        assert_eq!(app.themes(channel.id, None).unwrap().unranked, 0);
        assert_eq!(h.text.requests().len(), 1, "ranking proposes nothing");
    }

    #[test]
    fn an_edit_drops_the_ranking_until_ranked_again() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);
        let theme = app.themes(channel.id, None).unwrap().themes[0].clone();

        let edited = app
            .edit_theme(
                theme.id,
                "  The probe that never came home ",
                "A sharper angle.",
            )
            .unwrap();
        assert_eq!(edited.idea().title(), "The probe that never came home");
        assert_eq!(edited.ranking(), None);
        let view = app.themes(channel.id, None).unwrap();
        assert_eq!(view.unranked, 1);
        assert_eq!(
            titles(&view).last(),
            Some(&"The probe that never came home"),
            "unranked ideas sort last"
        );

        let id = app
            .rank_themes(channel.id, "space history", BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);
        let (_, questions) = h.decisions.calls().pop().unwrap();
        assert_eq!(questions.len(), 3, "only the edited idea is ranked");
        let Some(Question::Score { instructions, .. }) = questions.get("t0_fit") else {
            panic!("fit is a score question");
        };
        assert!(instructions.contains("The probe that never came home"));
        assert_eq!(app.themes(channel.id, None).unwrap().unranked, 0);
        assert!(matches!(
            app.rank_themes(channel.id, "space history", BudgetConsent::Ask),
            Err(ThemeError::NothingToRank)
        ));
    }

    #[test]
    fn saving_the_same_text_keeps_the_ranking() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);
        let theme = app.themes(channel.id, None).unwrap().themes[0].clone();

        let edited = app
            .edit_theme(theme.id, theme.idea().title(), theme.idea().angle())
            .unwrap();
        assert_eq!(edited.ranking(), theme.ranking());
    }

    #[test]
    fn invalid_text_is_rejected_field_by_field() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);
        let theme = app.themes(channel.id, None).unwrap().themes[0].clone();

        let error = app
            .edit_theme(theme.id, " ", &"x".repeat(ThemeIdea::MAX_ANGLE_CHARS + 1))
            .unwrap_err();
        assert_eq!(
            error.field_errors(),
            [
                ThemeFieldError::TitleRequired,
                ThemeFieldError::AngleTooLong
            ]
        );
        assert_eq!(
            error.message(),
            Text::ThemeFieldError(ThemeFieldError::TitleRequired)
        );
        let kept = app.themes(channel.id, None).unwrap().themes[0].clone();
        assert_eq!(kept.idea(), theme.idea());
    }

    #[test]
    fn a_user_edit_during_ranking_wins() {
        let h = Harness::new();
        *h.decisions.delay.lock().unwrap() = Duration::from_millis(150);
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        let id = app
            .suggest_themes(channel.id, "space history", BudgetConsent::Ask)
            .unwrap();
        wait_until("the decision call", || !h.decisions.calls().is_empty());

        let theme = app
            .themes(channel.id, None)
            .unwrap()
            .themes
            .into_iter()
            .find(|theme| theme.idea().title() == "Idea 2")
            .unwrap();
        app.edit_theme(theme.id, "Idea 2, rewritten", "").unwrap();
        let other = app.themes(channel.id, None).unwrap().themes[0].id;
        app.discard_theme(other).unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);

        let view = app.themes(channel.id, None).unwrap();
        let edited = view.themes.iter().find(|t| t.id == theme.id).unwrap();
        assert_eq!(edited.idea().title(), "Idea 2, rewritten");
        assert_eq!(edited.ranking(), None);
        assert_eq!(view.discarded, 1);
        assert_eq!(view.unranked, 1);
    }

    #[test]
    fn discarded_ideas_leave_the_list_and_are_counted() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);
        let theme = app.themes(channel.id, None).unwrap().themes[0].clone();

        app.discard_theme(theme.id).unwrap();
        let view = app.themes(channel.id, None).unwrap();
        assert_eq!(view.themes.len(), SUGGESTIONS_PER_RUN - 1);
        assert!(view.themes.iter().all(|t| t.id != theme.id));
        assert_eq!(view.discarded, 1);

        let again = app.discard_theme(theme.id).unwrap_err();
        assert!(matches!(again, ThemeError::NotSuggested(_)));
        assert_eq!(again.message(), Text::ThemeNotFound);
        assert!(app.approve_theme(theme.id).is_err());
    }

    #[test]
    fn approving_starts_a_video_project_linked_to_channel_niche_and_theme() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);
        let theme = app.themes(channel.id, None).unwrap().themes[3].clone();

        let project = app.approve_theme(theme.id).unwrap();
        assert_eq!(project.channel, channel.id);
        assert_eq!(project.niche.label(), "space history");
        assert_eq!(project.theme, theme.id);
        assert_eq!(project.title, theme.idea().title());
        assert_eq!(project.owner, app.profile().id);

        let view = app.themes(channel.id, None).unwrap();
        let projects: Vec<_> = view.projects.iter().map(|p| (p.id, p.theme)).collect();
        assert_eq!(projects, [(project.id, theme.id)]);
        let approved = view.themes.iter().find(|t| t.id == theme.id).unwrap();
        assert_eq!(approved.status(), ThemeStatus::Approved);
        assert!(approved.ranking().is_some(), "the reasons stay visible");

        assert!(matches!(
            app.approve_theme(theme.id),
            Err(ThemeError::NotSuggested(_))
        ));
        assert!(app.edit_theme(theme.id, "Changed", "").is_err());
        assert!(app.discard_theme(theme.id).is_err());
        assert_eq!(app.themes(channel.id, None).unwrap().projects.len(), 1);
    }

    #[test]
    fn work_needs_the_keys_of_the_providers_it_calls() {
        let h = Harness::new();
        let mut app = h.start();
        let channel = channel(&app, "space history");

        let error = app
            .suggest_themes(channel.id, "space history", BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, ThemeError::MissingKey(Provider::Claude)));
        assert_eq!(error.message(), Text::ThemesMissingKey(Provider::Claude));

        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        let error = app
            .suggest_themes(channel.id, "space history", BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, ThemeError::MissingKey(Provider::TypeSafe)));
        assert!(app.jobs().is_empty(), "no job starts without its keys");
    }

    #[test]
    fn one_theme_job_per_channel_at_a_time() {
        let h = Harness::new();
        *h.decisions.delay.lock().unwrap() = Duration::from_millis(100);
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");

        let id = app
            .suggest_themes(channel.id, "space history", BudgetConsent::Ask)
            .unwrap();
        let again = app
            .suggest_themes(channel.id, "space history", BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(again, ThemeError::Busy));
        assert_eq!(again.message(), Text::ThemesBusy);

        let other = channel_named(&app, "Ocean Files", "deep sea");
        assert!(
            app.suggest_themes(other.id, "deep sea", BudgetConsent::Ask)
                .is_ok()
        );
        wait_done(&app, id);
        assert!(
            app.suggest_themes(channel.id, "space history", BudgetConsent::Ask)
                .is_ok()
        );
    }

    fn channel_named(app: &Bardo, name: &str, niche: &str) -> Channel {
        app.create_channel(ChannelDraft {
            name: name.into(),
            niche: niche.into(),
            ..ChannelDraft::default()
        })
        .unwrap()
    }

    #[test]
    fn niches_come_from_research_or_the_channel() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        let view = app.themes(channel.id, None).unwrap();
        let labels: Vec<_> = view.niches.iter().map(Niche::label).collect();
        assert_eq!(labels, ["space history"]);
        assert_eq!(view.research, None);
        assert!(view.themes.is_empty());

        let blank = channel_named(&app, "Blank", "");
        let view = app.themes(blank.id, None).unwrap();
        assert!(view.niches.is_empty());
        assert_eq!(view.niche, None);
        let error = app
            .suggest_themes(blank.id, " ", BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, ThemeError::InvalidNiche(_)));
        assert_eq!(error.message(), Text::ThemesPickNiche);
    }

    #[test]
    fn each_niche_keeps_its_own_ideas() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);
        h.text.answer_with(&["A deep sea idea"]);
        let id = app
            .suggest_themes(channel.id, "deep sea", BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);

        let deep = app.themes(channel.id, Some("Deep Sea")).unwrap();
        assert_eq!(titles(&deep), ["A deep sea idea"]);
        let space = app.themes(channel.id, Some("space history")).unwrap();
        assert_eq!(space.themes.len(), SUGGESTIONS_PER_RUN);
        assert_eq!(
            space.job.map(|job| job.id()),
            Some(id),
            "the job is per channel"
        );
    }

    #[test]
    fn other_profiles_cannot_reach_channels_or_themes() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);
        let theme = app.themes(channel.id, None).unwrap().themes[0].clone();

        assert!(matches!(
            app.themes(ChannelId::new(), None),
            Err(ThemeError::ChannelNotFound)
        ));
        assert!(matches!(
            app.approve_theme(ThemeId::new()),
            Err(ThemeError::ThemeNotFound)
        ));

        let stranger = bardo_domain::UserProfile::new(UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&*h.db, &stranger).unwrap();
        let mut foreign = theme.clone();
        foreign.id = ThemeId::new();
        foreign.owner = stranger.id;
        h.db.save_themes(&[foreign.clone()]).unwrap();
        assert!(matches!(
            app.discard_theme(foreign.id),
            Err(ThemeError::ThemeNotFound)
        ));
    }

    const DAY: Duration = Duration::from_secs(24 * 3600);

    /// A YouTube post of a new project of the channel: `title` in
    /// `niche`, live `days_ago`, with one snapshot of `views` taken `age`
    /// after it went live (`None`: never synced).
    struct Post<'a> {
        niche: &'a str,
        title: &'a str,
        days_ago: u32,
        age: Option<Duration>,
        views: u64,
    }

    fn published(h: &Harness, app: &Bardo, channel: &Channel, post: Post<'_>) -> VideoProject {
        let Post {
            niche,
            title,
            days_ago,
            age,
            views,
        } = post;
        use bardo_domain::{
            MetricsSnapshot, Network, NetworkAccountId, PostLink, Publication, PublicationId,
            PublicationRepository, RenderId,
        };

        let mut theme = Theme::suggested(
            app.profile().id,
            channel.id,
            Niche::new(niche).unwrap(),
            ThemeIdea::new(title, "").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        h.db.save_themes(std::slice::from_ref(&theme)).unwrap();
        let project = app.approve_theme(theme.id).unwrap();
        theme.approve(SystemTime::now()).unwrap();
        let posted_at = SystemTime::now() - DAY * days_ago;
        let count = h.db.channel_publications(channel.id).unwrap().len();
        let publication = Publication {
            id: PublicationId::new(),
            owner: app.profile().id,
            project: project.id,
            account: NetworkAccountId::new(),
            render: RenderId::new(),
            network: Network::YouTube,
            link: Some(
                PostLink::parse(
                    Network::YouTube,
                    &format!("https://www.youtube.com/watch?v=video{count:06}"),
                )
                .unwrap(),
            ),
            kind: bardo_domain::PublicationKind::Manual,
            posted_at,
            linked_at: posted_at,
            checked_at: None,
            missing_since: None,
            insights_id: None,
        };
        h.db.save_publication(&publication).unwrap();
        if let Some(age) = age {
            let snapshot = MetricsSnapshot {
                publication: publication.id,
                taken_at: posted_at + age,
                views,
                likes: None,
                comments: None,
                owner: None,
                insights: bardo_domain::Insights::default(),
            };
            h.db.save_sync(&[], &[snapshot]).unwrap();
        }
        project
    }

    const PERFORMANCE_ASK: &str = "Judging by the videos most like this idea";

    #[test]
    fn rankings_read_the_channels_published_videos_and_give_a_fourth_reason() {
        let h = Harness::new();
        // Idea 4 looks like what did well; the rest sit at the usual.
        h.decisions.set_level(
            &format!("\"Idea 4\": Why Idea 4 matters.\n{}", "The state lists"),
            4.0,
        );
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        published(
            &h,
            &app,
            &channel,
            Post {
                niche: "space history",
                title: "Lost probes",
                days_ago: 30,
                age: Some(7 * DAY),
                views: 1_000,
            },
        );
        published(
            &h,
            &app,
            &channel,
            Post {
                niche: "space history",
                title: "Moon hoaxes",
                days_ago: 20,
                age: Some(14 * DAY),
                views: 6_000,
            },
        );
        published(
            &h,
            &app,
            &channel,
            Post {
                niche: "deep sea",
                title: "The abyss",
                days_ago: 10,
                age: Some(7 * DAY),
                views: 9_000,
            },
        );
        published(
            &h,
            &app,
            &channel,
            Post {
                niche: "space history",
                title: "Mars dust",
                days_ago: 3,
                age: Some(3 * DAY + DAY / 2),
                views: 500,
            },
        );

        let job = suggest(&app, &channel);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());

        let (state, questions) = h.decisions.calls().pop().unwrap();
        for line in [
            "How this channel's own published videos did in their first 7 days, newest first. \
             The channel's usual is its median: 1000 views.",
            "- \"Mars dust\" (niche: space history): below the channel's usual, 707 views, \
             projected from its first days",
            "- \"The abyss\" (niche: deep sea): far above the channel's usual, 9000 views",
            "- \"Moon hoaxes\" (niche: space history): far above the channel's usual, 4243 views",
            "- \"Lost probes\" (niche: space history): about the channel's usual, 1000 views",
        ] {
            assert!(state.contains(line), "{line}\n---\n{state}");
        }
        assert_eq!(questions.len(), 4 * SUGGESTIONS_PER_RUN);
        let Some(Question::Score {
            instructions,
            levels,
        }) = questions.get("t0_performance")
        else {
            panic!("past performance is a score question");
        };
        assert!(instructions.contains(PERFORMANCE_ASK), "{instructions}");
        assert_eq!(levels.len(), 5);

        let view = app.themes(channel.id, None).unwrap();
        let evidence = PerformanceEvidence {
            scope: EvidenceScope::Niche,
            // (1,000 + 6,000 · √(7/14) + 500 · √(7/3.5)) / 3
            average_views: 1_983,
            videos: 3,
            projected: 1,
            basis: 4,
        };
        assert_eq!(view.past, Some(evidence));
        let suggested: Vec<&Theme> = view
            .themes
            .iter()
            .filter(|theme| theme.status() == ThemeStatus::Suggested)
            .collect();
        assert_eq!(suggested.len(), SUGGESTIONS_PER_RUN);
        assert!(suggested.iter().all(|theme| {
            theme
                .ranking()
                .and_then(|ranking| ranking.performance)
                .is_some_and(|performance| performance.evidence == evidence)
        }));
        assert_eq!(
            suggested[0].idea().title(),
            "Idea 4",
            "what did well before leads"
        );
        let best = suggested[0].ranking().unwrap();
        assert_eq!(
            best.performance.unwrap().reason.score,
            bardo_domain::Score::MAX
        );
        // Four videos weigh 20%: 0.8·50 + 0.2·100.
        assert_eq!(best.priority(), bardo_domain::Score::new(60));
    }

    #[test]
    fn a_channel_without_history_ranks_exactly_as_before() {
        let fresh = Harness::new();
        let app = fresh.start_with_keys();
        let without = channel(&app, "space history");
        suggest(&app, &without);
        let (expected_state, expected_questions) = fresh.decisions.calls().pop().unwrap();
        let expected: Vec<_> = app
            .themes(without.id, None)
            .unwrap()
            .themes
            .iter()
            .map(|theme| {
                (
                    theme.idea().title().to_owned(),
                    theme.ranking().unwrap().priority(),
                )
            })
            .collect();

        // Posts too young to say anything, or never synced, are no history.
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        published(
            &h,
            &app,
            &channel,
            Post {
                niche: "space history",
                title: "Just out",
                days_ago: 1,
                age: Some(DAY),
                views: 400,
            },
        );
        published(
            &h,
            &app,
            &channel,
            Post {
                niche: "space history",
                title: "Never synced",
                days_ago: 30,
                age: None,
                views: 0,
            },
        );
        suggest(&app, &channel);

        let (state, questions) = h.decisions.calls().pop().unwrap();
        assert_eq!(state, expected_state);
        assert_eq!(questions, expected_questions);
        assert!(!state.contains("published videos"));
        let view = app.themes(channel.id, None).unwrap();
        assert_eq!(view.past, None);
        let ranked: Vec<_> = view
            .themes
            .iter()
            .filter(|theme| theme.status() == ThemeStatus::Suggested)
            .map(|theme| {
                let ranking = theme.ranking().unwrap();
                assert_eq!(ranking.performance, None);
                (theme.idea().title().to_owned(), ranking.priority())
            })
            .collect();
        assert_eq!(ranked, expected);
    }

    #[test]
    fn a_niche_without_videos_reads_the_whole_channel() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        published(
            &h,
            &app,
            &channel,
            Post {
                niche: "deep sea",
                title: "The abyss",
                days_ago: 10,
                age: Some(7 * DAY),
                views: 9_000,
            },
        );

        let view = app.themes(channel.id, None).unwrap();
        assert_eq!(
            view.past,
            Some(PerformanceEvidence {
                scope: EvidenceScope::Channel,
                average_views: 9_000,
                videos: 1,
                projected: 0,
                basis: 1,
            })
        );
        suggest(&app, &channel);
        let ranking = app.themes(channel.id, None).unwrap().themes[0]
            .ranking()
            .cloned()
            .unwrap();
        assert_eq!(
            ranking.performance.map(|p| p.evidence.scope),
            Some(EvidenceScope::Channel)
        );
    }

    #[test]
    fn the_engine_reads_only_the_newest_videos() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        for n in 0..(MAX_PAST_VIDEOS as u32 + 2) {
            published(
                &h,
                &app,
                &channel,
                Post {
                    niche: "space history",
                    title: &format!("Video {n}"),
                    days_ago: 100 - n,
                    age: Some(7 * DAY),
                    views: 100,
                },
            );
        }
        suggest(&app, &channel);
        let (state, _) = h.decisions.calls().pop().unwrap();
        assert!(state.contains("\"Video 41\""), "{state}");
        assert!(!state.contains("\"Video 1\""), "{state}");
        assert!(state.contains("- and 2 older videos"), "{state}");
        let ranking = app.themes(channel.id, None).unwrap().themes[0]
            .ranking()
            .cloned()
            .unwrap();
        assert_eq!(ranking.performance.unwrap().evidence.basis, 42);
    }

    #[test]
    fn ideas_ranked_before_history_are_ranked_again_on_request() {
        let h = Harness::new();
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");
        suggest(&app, &channel);
        let view = app.themes(channel.id, None).unwrap();
        assert_eq!((view.unranked, view.before_history), (0, 0));

        published(
            &h,
            &app,
            &channel,
            Post {
                niche: "space history",
                title: "Lost probes",
                days_ago: 10,
                age: Some(7 * DAY),
                views: 1_000,
            },
        );
        let view = app.themes(channel.id, None).unwrap();
        assert_eq!(view.unranked, 0);
        assert_eq!(view.before_history, SUGGESTIONS_PER_RUN);
        assert!(view.rank_estimate.is_some());

        // Suggesting more ranks only the new ideas, as estimated.
        h.text.answer_with(&["A new idea"]);
        suggest(&app, &channel);
        let (_, questions) = h.decisions.calls().pop().unwrap();
        assert_eq!(questions.len(), 4, "only the new idea");

        let id = app
            .rank_themes(channel.id, "space history", BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);
        let (_, questions) = h.decisions.calls().pop().unwrap();
        assert_eq!(questions.len(), 4 * SUGGESTIONS_PER_RUN);
        let view = app.themes(channel.id, None).unwrap();
        assert_eq!((view.unranked, view.before_history), (0, 0));
        assert!(
            view.themes
                .iter()
                .filter(|theme| theme.status() == ThemeStatus::Suggested)
                .all(|theme| theme.ranking().unwrap().performance.is_some())
        );
        assert!(matches!(
            app.rank_themes(channel.id, "space history", BudgetConsent::Ask),
            Err(ThemeError::NothingToRank)
        ));
    }

    #[test]
    fn titles_reach_the_engine_on_one_line() {
        assert_eq!(one_line("The \"lost\"\n  probe"), "The 'lost' probe");
    }

    #[test]
    fn the_evidence_reads_as_a_sentence() {
        let h = Harness::new();
        let app = h.start();
        let evidence = |scope, videos, projected| PerformanceEvidence {
            scope,
            average_views: 12_400,
            videos,
            projected,
            basis: 9,
        };
        assert_eq!(
            app.performance_evidence(&evidence(EvidenceScope::Niche, 3, 0)),
            "Videos in this niche averaged 12K views in their first 7 days on this channel \
             (3 videos)."
        );
        assert_eq!(
            app.performance_evidence(&evidence(EvidenceScope::Channel, 1, 1)),
            "No video in this niche yet; the channel's one video had 12K views in its first 7 \
             days. 1 of them projected from their first days."
        );
    }

    #[test]
    fn a_job_queued_before_past_performance_still_runs() {
        let payload = ThemePayload::parse(
            r#"{"channel":"0b0d7c7e-5b8e-4b8a-9d55-7e3b1c1d2e3f","niche":"space history",
                "brief":{"channel_name":"A","country":"Brazil","language":"Portuguese",
                "channel_niche":"","channel_themes":[],"aesthetic_notes":"","research":null}}"#,
        )
        .unwrap();
        assert_eq!(payload.brief.past, None);
        assert_eq!(
            payload.brief.state("space history"),
            payload.brief.describe("space history")
        );
        assert!(!payload.to_json().contains("past"), "no history, no field");
    }

    #[test]
    fn a_resumed_suggestion_does_not_propose_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let first = Harness::with_db(Arc::new(Database::open(&path).unwrap()));
        *first.decisions.delay.lock().unwrap() = Duration::from_millis(100);
        let app = first.start_with_keys();
        let channel = channel(&app, "space history");
        let id = app
            .suggest_themes(channel.id, "space history", BudgetConsent::Ask)
            .unwrap();
        wait_for(&app, id, |job| job.checkpoint() == Some(PROPOSED));
        drop(app);

        let second = Harness::on_disk(&path, &first);
        drop(first);
        *second.decisions.delay.lock().unwrap() = Duration::ZERO;
        let app = second.start();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);

        assert_eq!(second.text.requests().len(), 1, "Claude is asked once");
        let view = app.themes(channel.id, None).unwrap();
        assert_eq!(view.themes.len(), SUGGESTIONS_PER_RUN);
        assert_eq!(view.unranked, 0);
    }
}
