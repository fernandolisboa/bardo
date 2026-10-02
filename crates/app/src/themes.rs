//! Theme use cases (PRD stories 25-28): Claude proposes video ideas for a
//! researched niche, the decision engine ranks them with typed reasons
//! (fit, trend, competition), and the user edits, discards or approves
//! each one. Approving starts a video project.
//!
//! Proposing and ranking call providers, so they run as jobs. A suggestion
//! job proposes, saves the ideas and then ranks them; a ranking job only
//! ranks the niche's themes that have no ranking (e.g. after an edit).

use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, Channel, ChannelId, DecisionEngine, Decisions, Job, JobFailure, JobFailureKind, JobId,
    JobKind, Niche, NicheScores, NicheSeedError, ProfileId, Progress, Provider, ProviderFailure,
    Question, Questions, Reason, RepositoryError, SecretStore, TextFormat, TextGenerator,
    TextRequest, Theme, ThemeFieldError, ThemeId, ThemeIdea, ThemeNotSuggested, ThemeRanking,
    ThemeRepository, ThemeStatus, UiLanguage, VideoProject, rank_themes,
};
use serde::{Deserialize, Serialize};

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
    /// The channel's latest suggestion or ranking job.
    pub job: Option<Job>,
    /// The channel's video projects, newest first.
    pub projects: Vec<VideoProject>,
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

/// The three questions behind a ranking, asked about each theme.
#[derive(Clone, Copy)]
enum Aspect {
    Fit,
    Trend,
    Competition,
}

impl Aspect {
    const ALL: [Aspect; 3] = [Aspect::Fit, Aspect::Trend, Aspect::Competition];

    fn id(self, index: usize) -> String {
        let name = match self {
            Aspect::Fit => "fit",
            Aspect::Trend => "trend",
            Aspect::Competition => "competition",
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

/// Runs both theme job kinds.
pub(crate) struct ThemeHandler {
    pub(crate) owner: ProfileId,
    pub(crate) themes: Arc<dyn ThemeRepository>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) decisions: Arc<dyn DecisionEngine>,
    pub(crate) secrets: Arc<dyn SecretStore>,
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
    /// call. Each batch is saved as it arrives, so a resumed job ranks only
    /// what is left.
    fn rank(
        &self,
        payload: &ThemePayload,
        channel: ChannelId,
        niche: &Niche,
        cx: &JobContext,
        progress_from: u16,
    ) -> Result<(), JobFailure> {
        let pending: Vec<Theme> = self
            .themes
            .themes(channel)
            .map_err(unexpected)?
            .into_iter()
            .filter(|theme| {
                theme.status() == ThemeStatus::Suggested
                    && theme.ranking().is_none()
                    && theme.niche.key() == niche.key()
            })
            .collect();
        if pending.is_empty() {
            return Ok(());
        }
        let key = self.key(Provider::TypeSafe)?;
        let state = payload.brief.describe(niche.label());
        let total = pending.len() as u64;
        let mut done = 0;

        for batch in pending.chunks(RANK_BATCH) {
            if cx.should_stop() {
                return Ok(());
            }
            let mut questions = Questions::new();
            for (index, theme) in batch.iter().enumerate() {
                for aspect in Aspect::ALL {
                    questions = questions
                        .ask(aspect.id(index), aspect.question(theme.idea()))
                        .map_err(unexpected)?;
                }
            }
            let decisions = self
                .decisions
                .decide(&key, &state, &questions)
                .map_err(|failure| provider_failure("decision engine", failure))?;
            let ranked_at = SystemTime::now();

            let mut ranked = Vec::new();
            for (index, theme) in batch.iter().enumerate() {
                let ranking = ThemeRanking {
                    fit: reason(&decisions, &Aspect::Fit.id(index))?,
                    trend: reason(&decisions, &Aspect::Trend.id(index))?,
                    competition: reason(&decisions, &Aspect::Competition.id(index))?,
                    model: decisions.model.clone(),
                    ranked_at,
                };
                // The user may have edited, discarded or approved it while
                // the engine was answering; their change wins.
                let current = self.themes.theme(theme.id).map_err(unexpected)?;
                if let Some(mut current) = current
                    && current.idea() == theme.idea()
                    && current.ranking().is_none()
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
        let Some(niche) = niche else {
            return Ok(ThemesView {
                niches,
                niche: None,
                research: None,
                themes: Vec::new(),
                discarded: 0,
                unranked: 0,
                job,
                projects,
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
        let unranked = all
            .iter()
            .filter(|theme| theme.status() == ThemeStatus::Suggested && theme.ranking().is_none())
            .count();
        let mut themes: Vec<Theme> = all
            .into_iter()
            .filter(|theme| theme.status() != ThemeStatus::Discarded)
            .collect();
        rank_themes(&mut themes);

        Ok(ThemesView {
            research: self.niche_scores(&channel, &niche)?,
            niches,
            niche: Some(niche),
            themes,
            discarded,
            unranked,
            job,
            projects,
        })
    }

    /// Starts a job in which Claude proposes ideas for the niche and the
    /// decision engine ranks them.
    pub fn suggest_themes(&self, channel: ChannelId, niche: &str) -> Result<JobId, ThemeError> {
        let channel = self.theme_channel(channel)?;
        let niche = Niche::new(niche).map_err(ThemeError::InvalidNiche)?;
        self.ensure_idle(channel.id)?;
        self.ensure_key(Provider::Claude)?;
        self.ensure_key(Provider::TypeSafe)?;
        self.enqueue_theme_job(JobKind::ThemeSuggestion, &channel, &niche)
    }

    /// Starts a job ranking the niche's suggested themes that have no
    /// ranking (edited ones, or ones a failed job left behind).
    pub fn rank_themes(&self, channel: ChannelId, niche: &str) -> Result<JobId, ThemeError> {
        let channel = self.theme_channel(channel)?;
        let niche = Niche::new(niche).map_err(ThemeError::InvalidNiche)?;
        self.ensure_idle(channel.id)?;
        let waiting = self.themes.themes(channel.id)?.iter().any(|theme| {
            theme.status() == ThemeStatus::Suggested
                && theme.ranking().is_none()
                && theme.niche.key() == niche.key()
        });
        if !waiting {
            return Err(ThemeError::NothingToRank);
        }
        self.ensure_key(Provider::TypeSafe)?;
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
                channels: Box::new(Arc::clone(&self.db)),
                jobs: Arc::clone(&self.db) as Arc<dyn JobRepository>,
                themes: Arc::clone(&self.db) as _,
                templates: Arc::clone(&self.db) as _,
                scripts: Arc::clone(&self.db) as _,
                personas: Arc::clone(&self.db) as _,
                narrations: Arc::clone(&self.db) as _,
                scene_plans: Arc::clone(&self.db) as _,
                network_accounts: Arc::clone(&self.db) as _,
                files: Arc::new(bardo_storage::MemoryProjectFiles::default()),
                research: Arc::clone(&self.db) as _,
                secrets: Arc::clone(&self.secrets) as _,
            };
            let providers = Providers {
                key_checker: Arc::new(FakeKeyChecker::default()),
                market_data: Arc::new(FakeMarketData::default()),
                text: Arc::clone(&self.text) as _,
                decisions: Arc::clone(&self.decisions) as _,
                voices: Arc::new(crate::testing::FakeVoiceLibrary::default()),
                speech: Arc::new(crate::testing::FakeSpeech::default()),
                images: Arc::new(crate::testing::FakeImages::default()),
                audio: Arc::new(crate::narrations::testing::FakeAudioOutput::default()),
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
            .suggest_themes(channel.id, channel.details.niche())
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
        let id = app.rank_themes(channel.id, "space history").unwrap();
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

        let id = app.rank_themes(channel.id, "space history").unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);
        let (_, questions) = h.decisions.calls().pop().unwrap();
        assert_eq!(questions.len(), 3, "only the edited idea is ranked");
        let Some(Question::Score { instructions, .. }) = questions.get("t0_fit") else {
            panic!("fit is a score question");
        };
        assert!(instructions.contains("The probe that never came home"));
        assert_eq!(app.themes(channel.id, None).unwrap().unranked, 0);
        assert!(matches!(
            app.rank_themes(channel.id, "space history"),
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
        let id = app.suggest_themes(channel.id, "space history").unwrap();
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

        let error = app.suggest_themes(channel.id, "space history").unwrap_err();
        assert!(matches!(error, ThemeError::MissingKey(Provider::Claude)));
        assert_eq!(error.message(), Text::ThemesMissingKey(Provider::Claude));

        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        let error = app.suggest_themes(channel.id, "space history").unwrap_err();
        assert!(matches!(error, ThemeError::MissingKey(Provider::TypeSafe)));
        assert!(app.jobs().is_empty(), "no job starts without its keys");
    }

    #[test]
    fn one_theme_job_per_channel_at_a_time() {
        let h = Harness::new();
        *h.decisions.delay.lock().unwrap() = Duration::from_millis(100);
        let app = h.start_with_keys();
        let channel = channel(&app, "space history");

        let id = app.suggest_themes(channel.id, "space history").unwrap();
        let again = app.suggest_themes(channel.id, "space history").unwrap_err();
        assert!(matches!(again, ThemeError::Busy));
        assert_eq!(again.message(), Text::ThemesBusy);

        let other = channel_named(&app, "Ocean Files", "deep sea");
        assert!(app.suggest_themes(other.id, "deep sea").is_ok());
        wait_done(&app, id);
        assert!(app.suggest_themes(channel.id, "space history").is_ok());
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
        let error = app.suggest_themes(blank.id, " ").unwrap_err();
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
        let id = app.suggest_themes(channel.id, "deep sea").unwrap();
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

    #[test]
    fn a_resumed_suggestion_does_not_propose_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let first = Harness::with_db(Arc::new(Database::open(&path).unwrap()));
        *first.decisions.delay.lock().unwrap() = Duration::from_millis(100);
        let app = first.start_with_keys();
        let channel = channel(&app, "space history");
        let id = app.suggest_themes(channel.id, "space history").unwrap();
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
