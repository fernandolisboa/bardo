//! AI cut suggestions (PRD story 60): on the timeline, the user asks where
//! the picture could cut. Code finds the candidate points in the narration
//! (`bardo_domain::cut_candidates`: sentence ends, pauses, scene changes)
//! that the cut still lacks, and the decision engine scores each one for
//! pacing and says whether the topic moves on there. The user accepts or
//! rejects each suggestion; accepting splits the clip under it through the
//! editor's own edit path, so it saves and undoes like a manual cut.
//!
//! Scoring calls the engine, so it runs as a job: a stretch of the script
//! per call (`bardo_domain::cut_chunks`), sized well inside the engine's
//! context, with a checkpoint after each so a resumed job asks only for
//! what is left. A finished job replaces the project's suggestions;
//! a cancelled or failed one leaves the earlier ones as they were.
//!
//! Suggestions are kept with the project, in narration time, so they
//! survive closing the editor (they were paid for) and keep their place as
//! the cut moves the narration. A new narration leaves them behind.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, ChunkLimits, Confidence, CostPurpose, CutCandidate, CutChunk, CutPlace, CutReasons,
    CutRules, CutSuggestion, CutSuggestionRepository, CutSuggestions, DEFAULT_CUT_FLOOR,
    DecisionEngine, Decisions, Job, JobFailure, JobFailureKind, JobId, JobKind, Narration,
    NarrationId, NarrationRepository, ProfileId, Progress, Provider, Question, Questions,
    RepositoryError, STRONG_CUT, ScenePlan, Score, SecretStore, SuggestionStatus, Timeline,
    UserProfile, VideoProjectId, cut_candidates, cut_chunks, cut_marker, open_candidates,
    place_cut,
};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::editor::{EditAction, Editor, EditorError};
use crate::export::unix_millis;
use crate::jobs::{JobContext, JobHandler};
use crate::{AppError, Bardo, KeyState, Text};

/// What one engine call reads. The engine takes 32k tokens of state plus
/// its longest question, and 64k with every question; its answers lose
/// accuracy as the state fills with text a question does not need. About
/// a thousand tokens of script and up to thirty points (sixty short
/// questions) stay far inside both.
pub const CHUNK_LIMITS: ChunkLimits = ChunkLimits {
    max_chars: 4_000,
    max_cuts: 30,
};

/// How likely a topic shift must be to show as a reason.
const TOPIC_SHIFT_FROM: f64 = 0.5;

#[derive(Debug, thiserror::Error)]
pub enum CutSuggestionError {
    /// No cut to suggest on: the project has no timeline yet.
    #[error("nothing to cut")]
    NothingToCut,
    /// Every sentence end, pause and scene change has a cut, or is too
    /// close to one.
    #[error("no point to suggest")]
    NoCandidates,
    #[error("no TypeSafe key saved")]
    MissingKey,
    /// The project's cut points are being scored.
    #[error("cut points are already being scored")]
    Busy,
    /// Scoring would reach a budget; the screen asks before starting it
    /// with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    /// The suggestion is not on the cut (any more).
    #[error("no such suggestion")]
    NoSuchSuggestion,
    #[error(transparent)]
    Editor(#[from] EditorError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl CutSuggestionError {
    /// What the editor says.
    pub fn message(&self) -> Text {
        match self {
            CutSuggestionError::NothingToCut => Text::EditorNothingToCut,
            CutSuggestionError::NoCandidates => Text::CutsNoPoints,
            CutSuggestionError::MissingKey => Text::CutsMissingKey,
            CutSuggestionError::Busy => Text::CutsBusy,
            CutSuggestionError::OverBudget(_) => Text::BudgetReachedTitle,
            CutSuggestionError::NoSuchSuggestion => Text::CutsGone,
            CutSuggestionError::Editor(error) => error.message(),
            CutSuggestionError::Repository(_) => Text::CutsNotSaved,
        }
    }
}

/// Where a suggestion stands with the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionState {
    Pending,
    /// The picture cuts at its point.
    Accepted,
    Rejected,
}

/// One suggestion as the timeline shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SuggestionView {
    /// Its place among the project's suggestions, to accept or reject it.
    pub index: usize,
    /// Where on the timeline the cut goes.
    pub at: Duration,
    pub score: Score,
    pub confidence: Confidence,
    pub reasons: CutReasons,
    pub state: SuggestionState,
}

impl SuggestionView {
    /// Whether the score reaches `floor`, so it shows without asking.
    pub fn reaches(&self, floor: Score) -> bool {
        self.score >= floor
    }
}

/// The suggestions part of the editor.
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestionsView {
    /// Those the cut plays with room for a shot each side, in timeline
    /// order.
    pub items: Vec<SuggestionView>,
    /// The job that made them.
    pub set: Option<JobId>,
    /// The project's latest scoring job.
    pub job: Option<Job>,
    /// The score a suggestion needs to show unless the user asks for all.
    pub floor: Score,
    /// Points the cut lacks that asking now would score.
    pub open_points: usize,
    /// What asking now would cost; `None` with nothing to ask about.
    pub estimate: Option<SpendEstimate>,
}

impl Default for SuggestionsView {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            set: None,
            job: None,
            floor: DEFAULT_CUT_FLOOR,
            open_points: 0,
            estimate: None,
        }
    }
}

impl SuggestionsView {
    pub fn is_running(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }

    /// Pending suggestions that reach `floor`: what the toolbar counts.
    pub fn pending(&self, floor: Score) -> impl Iterator<Item = &SuggestionView> {
        self.items
            .iter()
            .filter(move |item| item.state == SuggestionState::Pending && item.reaches(floor))
    }

    /// The score "accept all" takes: [`STRONG_CUT`], or the floor when
    /// that is higher, so it never accepts what does not show.
    pub fn strong(&self) -> Score {
        self.floor.max(STRONG_CUT)
    }

    /// Pending suggestions below the floor, hidden unless asked for.
    pub fn hidden(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.state == SuggestionState::Pending && !item.reaches(self.floor))
            .count()
    }
}

/// One candidate as the job's payload carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Point {
    word: usize,
    source_ns: u64,
    sentence_end: bool,
    pause_ns: Option<u64>,
    scene_change: bool,
}

impl Point {
    fn of(candidate: &CutCandidate) -> Self {
        Self {
            word: candidate.word,
            source_ns: nanos(candidate.source),
            sentence_end: candidate.reasons.sentence_end,
            pause_ns: candidate.reasons.pause.map(nanos),
            scene_change: candidate.reasons.scene_change,
        }
    }

    fn candidate(self) -> CutCandidate {
        CutCandidate {
            word: self.word,
            source: Duration::from_nanos(self.source_ns),
            reasons: CutReasons {
                sentence_end: self.sentence_end,
                pause: self.pause_ns.map(Duration::from_nanos),
                scene_change: self.scene_change,
                topic_shift: false,
            },
        }
    }
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// The scoring job's payload: the points the cut lacked when asked, on the
/// narration they point into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CutPayload {
    project: String,
    narration: String,
    /// When the job was queued, in Unix milliseconds: suggestions saved
    /// after it (by a newer job) are not replaced when this one is retried.
    queued_at: u64,
    points: Vec<Point>,
}

impl CutPayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a cut suggestion payload serializes")
    }

    fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid cut suggestion payload: {e}")))
    }

    fn ids(&self) -> Result<(VideoProjectId, NarrationId), JobFailure> {
        let id = |text: &str| uuid::Uuid::parse_str(text).map_err(unexpected);
        Ok((
            VideoProjectId::from(id(&self.project)?),
            NarrationId::from(id(&self.narration)?),
        ))
    }
}

/// One point's answer, as the checkpoint keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Scored {
    point: usize,
    score: u8,
    confidence: f64,
    topic_shift: bool,
}

/// Where a scoring job resumes: the chunks answered and their answers.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
struct CutCheckpoint {
    chunks: usize,
    scored: Vec<Scored>,
    model: String,
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// The question ids of point `index`.
fn pace_id(index: usize) -> String {
    format!("p{index}_pace")
}

fn topic_id(index: usize) -> String {
    format!("p{index}_topic")
}

/// The state one call reads: a stretch of the script with its points
/// marked.
fn chunk_state(chunk: &CutChunk) -> String {
    format!(
        "An excerpt of a video's narration script. Markers like {} are points where the \
         picture could cut to a new shot; they are not part of the script.\n\n{}",
        cut_marker(0),
        chunk.text
    )
}

/// The two questions asked about point `index`: how good a cut it makes
/// for pacing, and whether the narration moves on there. What code knows
/// (a pause) is told in words; the engine judges words better than
/// numbers.
fn point_questions(
    questions: Questions,
    index: usize,
    candidate: &CutCandidate,
) -> Result<Questions, JobFailure> {
    let marker = cut_marker(index);
    let pause = if candidate.reasons.pause.is_some() {
        " The narrator pauses there."
    } else {
        ""
    };
    questions
        .ask(
            pace_id(index),
            Question::Score {
                instructions: format!(
                    "In the excerpt, {marker} marks a point between two words of the \
                     narration.{pause} How good is this point for cutting the picture to a new \
                     shot, for the video's pacing?"
                ),
                levels: [
                    "A bad cut: it splits a phrase or an idea in the middle",
                    "A weak cut: the same thought goes straight on",
                    "An acceptable cut: a small break inside the same thought",
                    "A good cut: one thought ends and the next begins",
                    "An ideal cut: the narration moves to a new subject, place, time or step",
                ]
                .map(String::from)
                .to_vec(),
            },
        )
        .and_then(|questions| {
            questions.ask(
                topic_id(index),
                Question::YesNo {
                    instructions: format!(
                        "In the excerpt, does the narration move on to a new topic, place, \
                         time or step at {marker}?"
                    ),
                    yes: Some(format!(
                        "The words after {marker} start something new compared to the words \
                         before it"
                    )),
                    no: Some(format!(
                        "The words after {marker} go on with the same topic as the words \
                         before it"
                    )),
                },
            )
        })
        .map_err(unexpected)
}

fn answer(decisions: &Decisions, point: usize) -> Result<Scored, JobFailure> {
    let missing = |id: String| {
        JobFailure::new(
            JobFailureKind::UnexpectedAnswer,
            format!("decision engine: no answer for {id}"),
        )
    };
    let pace = decisions
        .score(&pace_id(point))
        .ok_or_else(|| missing(pace_id(point)))?;
    let topic = decisions
        .yes_no(&topic_id(point))
        .ok_or_else(|| missing(topic_id(point)))?;
    Ok(Scored {
        point,
        score: pace.normalized().value(),
        confidence: pace.confidence.value(),
        topic_shift: topic.probability_yes >= TOPIC_SHIFT_FROM,
    })
}

/// Runs cut suggestion jobs.
pub(crate) struct CutSuggestionHandler {
    pub(crate) owner: ProfileId,
    pub(crate) suggestions: Arc<dyn CutSuggestionRepository>,
    pub(crate) narrations: Arc<dyn NarrationRepository>,
    pub(crate) decisions: Arc<dyn DecisionEngine>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl CutSuggestionHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::TypeSafe)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no TypeSafe key is saved"))
    }
}

impl CutSuggestionHandler {
    /// Whether the project's saved suggestions are this job's, or newer
    /// than it: then it has nothing left to save.
    fn superseded(
        &self,
        project: VideoProjectId,
        payload: &CutPayload,
        cx: &JobContext,
    ) -> Result<bool, JobFailure> {
        let saved = self
            .suggestions
            .cut_suggestions(project)
            .map_err(unexpected)?;
        Ok(saved.is_some_and(|saved| {
            saved.job == cx.id() || unix_millis(saved.made_at) > payload.queued_at
        }))
    }
}

impl JobHandler for CutSuggestionHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload = CutPayload::parse(payload)?;
        let (project, narration_id) = payload.ids()?;
        // An earlier attempt may have saved them and stopped before the
        // queue recorded it as done, or a newer job replaced them.
        if self.superseded(project, &payload, cx)? {
            return Ok(());
        }
        let narration = self
            .narrations
            .narration(project)
            .map_err(unexpected)?
            .filter(|narration| narration.id == narration_id)
            .ok_or_else(|| {
                JobFailure::unexpected("the narration changed since the points were found")
            })?;
        let candidates: Vec<CutCandidate> = payload
            .points
            .iter()
            .map(|point| point.candidate())
            .collect();
        let chunks = cut_chunks(&narration, &candidates, &CHUNK_LIMITS);
        let mut checkpoint: CutCheckpoint = match cx.checkpoint() {
            Some(text) => serde_json::from_str(text)
                .map_err(|e| JobFailure::unexpected(format!("invalid checkpoint: {e}")))?,
            None => CutCheckpoint::default(),
        };
        let total = chunks.len().max(1) as u64;

        if checkpoint.chunks < chunks.len() {
            let key = self.key()?;
            for chunk in &chunks[checkpoint.chunks..] {
                if cx.should_stop() {
                    return Ok(());
                }
                let mut questions = Questions::new();
                for &point in &chunk.cuts {
                    questions = point_questions(questions, point, &candidates[point])?;
                }
                let decisions = self
                    .decisions
                    .decide(&key, &chunk_state(chunk), &questions)
                    .map_err(|failure| {
                        JobFailure::new(
                            failure.kind.into(),
                            format!("decision engine: {}", failure.detail),
                        )
                    })?;
                self.costs.record_for_project(
                    PaidCall {
                        provider: Provider::TypeSafe,
                        model: &decisions.model,
                        purpose: CostPurpose::CutSuggestions,
                        usage: decisions.usage.into(),
                        job: Some(cx.id()),
                        reported: None,
                    },
                    project,
                );
                for &point in &chunk.cuts {
                    checkpoint.scored.push(answer(&decisions, point)?);
                }
                checkpoint.chunks += 1;
                checkpoint.model = decisions.model;
                let permille = 1000 * checkpoint.chunks as u64 / total;
                cx.save_checkpoint(
                    serde_json::to_string(&checkpoint).map_err(unexpected)?,
                    Progress::from_permille(permille as u16),
                )
                .map_err(unexpected)?;
            }
        }
        if cx.should_stop() || self.superseded(project, &payload, cx)? {
            return Ok(());
        }

        let mut scored = checkpoint.scored;
        scored.sort_by_key(|scored| scored.point);
        let suggestions = scored
            .into_iter()
            .filter_map(|scored| {
                let candidate = candidates.get(scored.point)?;
                Some(CutSuggestion {
                    source: candidate.source,
                    reasons: CutReasons {
                        topic_shift: scored.topic_shift,
                        ..candidate.reasons
                    },
                    score: Score::new(scored.score),
                    confidence: Confidence::new(scored.confidence),
                    status: SuggestionStatus::Pending,
                })
            })
            .collect();
        self.suggestions
            .save_cut_suggestions(&CutSuggestions {
                project,
                owner: self.owner,
                narration: narration.id,
                job: cx.id(),
                model: checkpoint.model,
                made_at: SystemTime::now(),
                suggestions,
            })
            .map_err(unexpected)
    }
}

/// The engine calls that score `chunks` chunks.
fn scoring_call(chunks: usize) -> PlannedCall {
    PlannedCall::new(
        Provider::TypeSafe,
        CostPurpose::CutSuggestions,
        chunks as u64,
    )
}

/// The points of `narration` the cut lacks, with room for a shot.
fn open_points(
    timeline: &Timeline,
    narration: &Narration,
    plan: Option<&ScenePlan>,
) -> Vec<CutCandidate> {
    let rules = CutRules::default();
    open_candidates(timeline, cut_candidates(narration, plan, &rules), &rules)
}

#[derive(Deserialize)]
struct ProjectOf {
    project: String,
}

impl Bardo {
    /// The project's scoring jobs, oldest first.
    fn cut_jobs(&self, project: VideoProjectId) -> impl DoubleEndedIterator<Item = Job> {
        let project = project.to_string();
        self.jobs().into_iter().filter(move |job| {
            job.kind() == JobKind::CutSuggestions
                && serde_json::from_str::<ProjectOf>(job.payload())
                    .is_ok_and(|of| of.project == project)
        })
    }

    /// The project's running scoring job, else its latest.
    fn latest_cut_job(&self, project: VideoProjectId) -> Option<Job> {
        let mut jobs: Vec<Job> = self.cut_jobs(project).collect();
        let running = jobs.iter().rposition(|job| job.state().is_active());
        match running {
            Some(index) => Some(jobs.swap_remove(index)),
            None => jobs.pop(),
        }
    }

    /// The suggestions part of the editor of `project`, on `timeline` as
    /// it is now.
    pub(crate) fn suggestions_view(
        &self,
        project: VideoProjectId,
        timeline: Option<&Timeline>,
        narration: Option<&Narration>,
        plan: Option<&ScenePlan>,
    ) -> Result<SuggestionsView, RepositoryError> {
        let mut view = SuggestionsView {
            job: self.latest_cut_job(project),
            floor: self.profile.cut_suggestion_floor,
            ..SuggestionsView::default()
        };
        let (Some(timeline), Some(narration)) = (timeline, narration) else {
            return Ok(view);
        };
        let saved = self
            .cut_suggestions
            .cut_suggestions(project)?
            .filter(|saved| saved.narration == narration.id);
        if let Some(saved) = saved {
            view.set = Some(saved.job);
            view.items = saved
                .suggestions
                .iter()
                .enumerate()
                .filter_map(|(index, suggestion)| {
                    let place = place_cut(timeline, suggestion.source)?;
                    // A point a cut made since leaves too short a shot.
                    if matches!(place, CutPlace::Open { .. })
                        && !place.has_room(timeline, &CutRules::default())
                    {
                        return None;
                    }
                    let state = match (place, suggestion.status) {
                        (CutPlace::Made { .. }, _) => SuggestionState::Accepted,
                        (CutPlace::Open { .. }, SuggestionStatus::Pending) => {
                            SuggestionState::Pending
                        }
                        (CutPlace::Open { .. }, SuggestionStatus::Rejected) => {
                            SuggestionState::Rejected
                        }
                    };
                    Some(SuggestionView {
                        index,
                        at: place.at(),
                        score: suggestion.score,
                        confidence: suggestion.confidence,
                        reasons: suggestion.reasons,
                        state,
                    })
                })
                .collect();
            view.items.sort_by_key(|item| (item.at, item.index));
        }
        let points = open_points(timeline, narration, plan);
        view.open_points = points.len();
        if !points.is_empty() {
            let chunks = cut_chunks(narration, &points, &CHUNK_LIMITS).len();
            view.estimate = Some(self.estimate(&[scoring_call(chunks)])?);
        }
        Ok(view)
    }

    /// Starts scoring the points the editor's cut lacks. The new
    /// suggestions replace the project's earlier ones, rejected ones
    /// included. Past TypeSafe's budget it needs `consent`.
    pub fn suggest_cuts(
        &self,
        editor: &mut Editor,
        consent: BudgetConsent,
    ) -> Result<JobId, CutSuggestionError> {
        self.refresh_editor(editor)?;
        let project = editor.project();
        let timeline = editor
            .view()
            .timeline
            .clone()
            .ok_or(CutSuggestionError::NothingToCut)?;
        let narration = self
            .narrations
            .narration(project)?
            .ok_or(CutSuggestionError::NothingToCut)?;
        if self.cut_jobs(project).any(|job| job.state().is_active()) {
            return Err(CutSuggestionError::Busy);
        }
        if self.provider_key(Provider::TypeSafe).state == KeyState::NotSet {
            return Err(CutSuggestionError::MissingKey);
        }
        let plan = self.scene_plans.scene_plan(project)?;
        let points = open_points(&timeline, &narration, plan.as_ref());
        if points.is_empty() {
            return Err(CutSuggestionError::NoCandidates);
        }
        let chunks = cut_chunks(&narration, &points, &CHUNK_LIMITS).len();
        if let Err(estimate) = self.check_budget(&[scoring_call(chunks)], consent)? {
            return Err(CutSuggestionError::OverBudget(estimate));
        }
        let payload = CutPayload {
            project: project.to_string(),
            narration: narration.id.to_string(),
            queued_at: unix_millis(SystemTime::now()),
            points: points.iter().map(Point::of).collect(),
        };
        let job = self.jobs.enqueue(Job::new(
            self.profile.id,
            JobKind::CutSuggestions,
            payload.to_json(),
        ))?;
        self.refresh_editor(editor)?;
        Ok(job)
    }

    /// Splits the clip under suggestion `index`, as a manual cut: saved at
    /// once and undoable.
    pub fn accept_suggestion(
        &self,
        editor: &mut Editor,
        index: usize,
    ) -> Result<(), CutSuggestionError> {
        let at = editor
            .view()
            .suggestions
            .items
            .iter()
            .find(|item| item.index == index && item.state != SuggestionState::Accepted)
            .map(|item| item.at)
            .ok_or(CutSuggestionError::NoSuchSuggestion)?;
        self.edit(editor, EditAction::CutPicture { at })?;
        Ok(())
    }

    /// Accepts every pending suggestion that scores
    /// [`SuggestionsView::strong`] or more, each as its own cut, earliest
    /// first; one a cut before it left without room is skipped. The number
    /// accepted.
    pub fn accept_strong_suggestions(
        &self,
        editor: &mut Editor,
    ) -> Result<usize, CutSuggestionError> {
        let mut accepted = 0;
        loop {
            let suggestions = &editor.view().suggestions;
            let strong = suggestions.strong();
            let Some(index) = suggestions
                .items
                .iter()
                .find(|item| item.state == SuggestionState::Pending && item.reaches(strong))
                .map(|item| item.index)
            else {
                return Ok(accepted);
            };
            self.accept_suggestion(editor, index)?;
            accepted += 1;
        }
    }

    /// Turns suggestion `index` down: it leaves the timeline and stays in
    /// the list, struck through, until the user asks again.
    pub fn reject_suggestion(
        &self,
        editor: &mut Editor,
        index: usize,
    ) -> Result<(), CutSuggestionError> {
        self.set_suggestion_status(editor, index, SuggestionStatus::Rejected)
    }

    /// Takes a rejection back.
    pub fn restore_suggestion(
        &self,
        editor: &mut Editor,
        index: usize,
    ) -> Result<(), CutSuggestionError> {
        self.set_suggestion_status(editor, index, SuggestionStatus::Pending)
    }

    fn set_suggestion_status(
        &self,
        editor: &mut Editor,
        index: usize,
        status: SuggestionStatus,
    ) -> Result<(), CutSuggestionError> {
        let project = editor.project();
        let mut saved = self
            .cut_suggestions
            .cut_suggestions(project)?
            .ok_or(CutSuggestionError::NoSuchSuggestion)?;
        // The editor may show an earlier set than the one saved now.
        let shown = &editor.view().suggestions;
        if shown.set != Some(saved.job) || !shown.items.iter().any(|item| item.index == index) {
            return Err(CutSuggestionError::NoSuchSuggestion);
        }
        if saved.set_status(index, status) {
            self.cut_suggestions.save_cut_suggestions(&saved)?;
        }
        self.refresh_editor(editor)?;
        Ok(())
    }

    /// The score a suggestion needs to show unless the user asks for all.
    pub fn cut_suggestion_floor(&self) -> Score {
        self.profile.cut_suggestion_floor
    }

    /// Changes that score and remembers it. On failure it stays as it was.
    pub fn set_cut_suggestion_floor(&mut self, floor: Score) -> Result<(), AppError> {
        if floor == self.profile.cut_suggestion_floor {
            return Ok(());
        }
        let updated = UserProfile {
            cut_suggestion_floor: floor,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{JobState, ProviderFailure, ProviderFailureKind, VideoProject};

    use super::*;
    use crate::scenes::tests::{Harness, done, wait_done};

    const TYPESAFE_KEY: &str = "ts_secret_typesafe_key_0001";

    /// A drawn project whose narration reads for 20 s: "Era uma vez, em
    /// 1969, uma sonda. | Ela partiu para longe. | O sinal sumiu em março.
    /// Ninguém sabe por quê." in three scenes. Its cut lacks three points:
    /// the two commas of the first sentence (pauses) and the end of the
    /// third (a sentence end and a pause); the other sentence ends start
    /// scenes, so the cut has them.
    fn drawn(h: &Harness) -> (Bardo, VideoProject) {
        *h.speech.length.lock().unwrap() = Duration::from_secs(20);
        let mut app = h.start();
        app.save_provider_key(Provider::TypeSafe, TYPESAFE_KEY)
            .unwrap();
        let (project, _) = h.drawn_project(&app);
        (app, project)
    }

    fn suggested(h: &Harness) -> (Bardo, VideoProject, Editor) {
        let (app, project) = drawn(h);
        let mut editor = app.open_editor(project.id).unwrap();
        done(
            &app,
            app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap(),
        );
        app.refresh_editor(&mut editor).unwrap();
        (app, project, editor)
    }

    fn states(editor: &Editor) -> Vec<SuggestionState> {
        let items = &editor.view().suggestions.items;
        items.iter().map(|item| item.state).collect()
    }

    #[test]
    fn asking_scores_the_points_the_cut_lacks_and_changes_nothing() {
        let h = Harness::new();
        let (app, project) = drawn(&h);
        h.decisions.set_yes("[CUT 3]", 0.9);
        let mut editor = app.open_editor(project.id).unwrap();
        let before = editor.view().timeline.clone();
        assert_eq!(editor.view().suggestions.open_points, 3);
        assert!(editor.view().suggestions.estimate.is_some());

        let job = app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap();
        assert_eq!(
            editor.view().suggestions.job.as_ref().map(Job::id),
            Some(job)
        );
        done(&app, job);
        app.refresh_editor(&mut editor).unwrap();

        let view = &editor.view().suggestions;
        assert_eq!(states(&editor), [SuggestionState::Pending; 3]);
        assert!(view.items.windows(2).all(|pair| pair[0].at < pair[1].at));
        let (first, last) = (&view.items[0], &view.items[2]);
        assert!(first.reasons.pause.is_some());
        assert!(!first.reasons.sentence_end && !first.reasons.topic_shift);
        assert!(last.reasons.sentence_end && last.reasons.pause.is_some());
        assert!(last.reasons.topic_shift, "the engine said yes there");
        assert_eq!(first.score, Score::new(50), "the fake's middle level");
        assert_eq!(first.confidence, Confidence::new(0.8));
        assert_eq!(editor.view().timeline, before, "nothing cut yet");
        assert!(!editor.can_undo());

        let calls = h.decisions.calls();
        assert_eq!(calls.len(), 1);
        let (state, questions) = &calls[0];
        assert!(
            state.contains("vez, [CUT 1] em 1969, [CUT 2] uma sonda."),
            "{state}"
        );
        assert!(state.contains("março. [CUT 3] Ninguém"), "{state}");
        assert_eq!(questions.len(), 6, "two per point");
        assert!(questions.get("p0_pace").is_some() && questions.get("p2_topic").is_some());
        let Some(Question::Score { instructions, .. }) = questions.get("p0_pace") else {
            panic!("pacing is a score");
        };
        assert!(instructions.contains("[CUT 1]") && instructions.contains("pauses"));
    }

    #[test]
    fn accepting_cuts_the_clip_like_a_manual_split_and_undo_brings_it_back() {
        let h = Harness::new();
        let (app, project, mut editor) = suggested(&h);
        let clips = editor.view().clips.len();
        let target = editor.view().suggestions.items[2];

        app.accept_suggestion(&mut editor, target.index).unwrap();

        assert_eq!(editor.view().clips.len(), clips + 1);
        assert_eq!(
            editor.view().clips[3].at,
            target.at,
            "the new clip starts at the point"
        );
        assert_eq!(states(&editor)[2], SuggestionState::Accepted);
        assert!(editor.can_undo());
        // Saved like any edit.
        let reopened = app.open_editor(project.id).unwrap();
        assert_eq!(reopened.view().clips.len(), clips + 1);
        assert_eq!(states(&reopened)[2], SuggestionState::Accepted);
        assert!(matches!(
            app.accept_suggestion(&mut editor, target.index),
            Err(CutSuggestionError::NoSuchSuggestion)
        ));

        app.edit(&mut editor, EditAction::Undo).unwrap();
        assert_eq!(editor.view().clips.len(), clips);
        assert_eq!(states(&editor)[2], SuggestionState::Pending);
        app.edit(&mut editor, EditAction::Redo).unwrap();
        assert_eq!(states(&editor)[2], SuggestionState::Accepted);
    }

    #[test]
    fn rejecting_keeps_it_turned_down_until_asked_again() {
        let h = Harness::new();
        let (app, project, mut editor) = suggested(&h);
        let first = editor.view().suggestions.items[0].index;

        app.reject_suggestion(&mut editor, first).unwrap();
        assert_eq!(states(&editor)[0], SuggestionState::Rejected);
        let reopened = app.open_editor(project.id).unwrap();
        assert_eq!(states(&reopened)[0], SuggestionState::Rejected);

        app.restore_suggestion(&mut editor, first).unwrap();
        assert_eq!(states(&editor)[0], SuggestionState::Pending);
        app.reject_suggestion(&mut editor, first).unwrap();

        // Asking again replaces the set, the rejection with it.
        let earlier = editor.view().suggestions.job.as_ref().map(Job::id);
        done(
            &app,
            app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap(),
        );
        app.refresh_editor(&mut editor).unwrap();
        assert_ne!(editor.view().suggestions.job.as_ref().map(Job::id), earlier);
        assert_eq!(states(&editor), [SuggestionState::Pending; 3]);
    }

    #[test]
    fn asking_again_after_a_cut_scores_only_the_points_still_open() {
        let h = Harness::new();
        let (app, _, mut editor) = suggested(&h);
        let last = editor.view().suggestions.items[2].index;
        app.accept_suggestion(&mut editor, last).unwrap();
        assert_eq!(editor.view().suggestions.open_points, 2);

        done(
            &app,
            app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap(),
        );
        app.refresh_editor(&mut editor).unwrap();

        assert_eq!(h.decisions.calls()[1].1.len(), 4);
        assert_eq!(editor.view().suggestions.items.len(), 2);
    }

    #[test]
    fn the_floor_hides_weak_suggestions_and_strong_ones_are_accepted_together() {
        let h = Harness::new();
        let (mut app, _, mut editor) = {
            let (app, project) = drawn(&h);
            h.decisions.set_level("[CUT 2]", 4.0);
            let mut editor = app.open_editor(project.id).unwrap();
            done(
                &app,
                app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap(),
            );
            app.refresh_editor(&mut editor).unwrap();
            (app, project, editor)
        };
        let view = &editor.view().suggestions;
        assert_eq!(view.floor, bardo_domain::DEFAULT_CUT_FLOOR);
        assert_eq!(view.pending(view.floor).count(), 3);
        assert_eq!(view.items[1].score, Score::MAX);

        app.set_cut_suggestion_floor(Score::new(60)).unwrap();
        app.refresh_editor(&mut editor).unwrap();
        let view = &editor.view().suggestions;
        assert_eq!(view.pending(view.floor).count(), 1);
        assert_eq!(view.hidden(), 2);
        drop(app);
        let app = h.start();
        assert_eq!(app.cut_suggestion_floor(), Score::new(60), "remembered");

        let clips = editor.view().clips.len();
        assert_eq!(app.accept_strong_suggestions(&mut editor).unwrap(), 1);
        assert_eq!(editor.view().clips.len(), clips + 1);
        assert_eq!(
            states(&editor),
            [
                SuggestionState::Pending,
                SuggestionState::Accepted,
                SuggestionState::Pending
            ]
        );
    }

    /// A project narrated for 5 minutes, 120 sentences in scenes of a
    /// few: over a hundred points, several stretches.
    fn long(h: &Harness) -> (Bardo, VideoProject, Editor) {
        let sentences: Vec<String> = (0..120)
            .map(|n| format!("The probe sent its report number {n} home."))
            .collect();
        *h.speech.length.lock().unwrap() = Duration::from_secs(300);
        let mut app = h.start();
        app.save_provider_key(Provider::TypeSafe, TYPESAFE_KEY)
            .unwrap();
        h.answer(sentences.join(" "));
        let project = crate::scenes::tests::project(&app);
        done(
            &app,
            app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
        );
        done(
            &app,
            app.generate_narration(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        h.answer(crate::scenes::tests::plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        let editor = app.open_editor(project.id).unwrap();
        (app, project, editor)
    }

    #[test]
    fn a_long_script_is_scored_a_stretch_at_a_time() {
        let h = Harness::new();
        let (app, _, mut editor) = long(&h);
        let points = editor.view().suggestions.open_points;
        assert!(points > 100, "{points}");

        done(
            &app,
            app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap(),
        );
        app.refresh_editor(&mut editor).unwrap();

        let calls = h.decisions.calls();
        assert!(calls.len() > 1, "one call per stretch");
        let mut asked = 0;
        for (state, questions) in &calls {
            assert!(
                state.chars().count() < CHUNK_LIMITS.max_chars + 400,
                "{}",
                state.len()
            );
            assert!(questions.len() <= 2 * CHUNK_LIMITS.max_cuts);
            asked += questions.len();
        }
        assert_eq!(asked, 2 * points, "every point once");
        assert_eq!(editor.view().suggestions.items.len(), points);
    }

    #[test]
    fn scoring_is_a_job_that_resumes_where_it_stopped() {
        let h = Harness::new();
        let (app, project, mut editor) = long(&h);
        let narration = app.narrations.narration(project.id).unwrap().unwrap();
        let plan = app.scene_plans.scene_plan(project.id).unwrap();
        let timeline = editor.view().timeline.clone().unwrap();
        let points = open_points(&timeline, &narration, plan.as_ref());
        let chunks = cut_chunks(&narration, &points, &CHUNK_LIMITS).len();
        assert!(chunks > 2, "{chunks}");

        *h.decisions.delay.lock().unwrap() = Duration::from_millis(100);
        let job = app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app
            .jobs()
            .iter()
            .any(|j| j.id() == job && j.progress().permille() == 0)
        {
            assert!(std::time::Instant::now() < deadline, "no stretch scored");
            std::thread::sleep(Duration::from_millis(2));
        }
        app.cancel_job(job).unwrap();
        let cancelled = wait_done(&app, job);
        assert_eq!(cancelled.state(), JobState::Cancelled);
        let kept: CutCheckpoint = serde_json::from_str(cancelled.checkpoint().unwrap()).unwrap();
        assert!(kept.chunks > 0 && kept.chunks < chunks, "{}", kept.chunks);
        // The call in flight when it stopped still ends.
        std::thread::sleep(Duration::from_millis(150));
        let before = h.decisions.calls().len();
        assert_eq!(
            app.cut_suggestions.cut_suggestions(project.id).unwrap(),
            None
        );

        *h.decisions.delay.lock().unwrap() = Duration::ZERO;
        app.retry_job(job).unwrap();
        assert_eq!(wait_done(&app, job).state(), JobState::Done);

        assert_eq!(
            h.decisions.calls().len() - before,
            chunks - kept.chunks,
            "only the stretches not scored yet"
        );
        app.refresh_editor(&mut editor).unwrap();
        assert_eq!(editor.view().suggestions.set, Some(job));
        assert_eq!(editor.view().suggestions.items.len(), points.len());
    }

    #[test]
    fn an_older_job_never_replaces_newer_suggestions() {
        let h = Harness::new();
        let (app, project) = drawn(&h);
        let mut editor = app.open_editor(project.id).unwrap();
        *h.decisions.delay.lock().unwrap() = Duration::from_millis(300);
        let older = app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap();
        assert!(matches!(
            app.suggest_cuts(&mut editor, BudgetConsent::Ask),
            Err(CutSuggestionError::Busy)
        ));
        app.cancel_job(older).unwrap();
        assert_eq!(wait_done(&app, older).state(), JobState::Cancelled);
        std::thread::sleep(Duration::from_millis(350));

        *h.decisions.delay.lock().unwrap() = Duration::ZERO;
        let newer = app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap();
        done(&app, newer);
        app.refresh_editor(&mut editor).unwrap();
        let first = editor.view().suggestions.items[0].index;
        app.reject_suggestion(&mut editor, first).unwrap();
        let calls = h.decisions.calls().len();

        app.retry_job(older).unwrap();
        assert_eq!(wait_done(&app, older).state(), JobState::Done);
        assert_eq!(h.decisions.calls().len(), calls, "nothing asked again");
        app.refresh_editor(&mut editor).unwrap();
        assert_eq!(editor.view().suggestions.set, Some(newer));
        assert_eq!(states(&editor)[0], SuggestionState::Rejected);
    }

    #[test]
    fn a_suggestion_of_an_earlier_set_is_not_changed() {
        let h = Harness::new();
        let (app, _, mut editor) = suggested(&h);
        let mut stale = app.open_editor(editor.project()).unwrap();
        done(
            &app,
            app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap(),
        );
        let index = stale.view().suggestions.items[0].index;
        assert!(matches!(
            app.reject_suggestion(&mut stale, index),
            Err(CutSuggestionError::NoSuchSuggestion)
        ));
        app.refresh_editor(&mut editor).unwrap();
        assert!(
            states(&editor)
                .iter()
                .all(|state| *state == SuggestionState::Pending)
        );
    }

    #[test]
    fn engine_failures_fail_the_job_with_their_kind() {
        let h = Harness::new();
        let (app, project) = drawn(&h);
        *h.decisions.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "invalid key",
        ));
        let mut editor = app.open_editor(project.id).unwrap();
        let job = wait_done(
            &app,
            app.suggest_cuts(&mut editor, BudgetConsent::Ask).unwrap(),
        );
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().map(|failure| failure.kind),
            Some(JobFailureKind::KeyRejected)
        );
        app.refresh_editor(&mut editor).unwrap();
        assert!(editor.view().suggestions.items.is_empty());
    }

    #[test]
    fn asking_needs_a_key_points_and_room_in_the_budget() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        assert!(matches!(
            app.suggest_cuts(&mut editor, BudgetConsent::Ask),
            Err(CutSuggestionError::MissingKey)
        ));
        assert_eq!(
            editor.view().suggestions.open_points,
            0,
            "a one-second narration has no room for a shot"
        );

        let h = Harness::new();
        let (app, project) = drawn(&h);
        let mut editor = app.open_editor(project.id).unwrap();
        app.set_budget(Provider::TypeSafe, "0.0001").unwrap();
        let Err(CutSuggestionError::OverBudget(estimate)) =
            app.suggest_cuts(&mut editor, BudgetConsent::Ask)
        else {
            panic!("over budget");
        };
        assert_eq!(estimate.providers[0].provider, Provider::TypeSafe);
        done(
            &app,
            app.suggest_cuts(&mut editor, BudgetConsent::Confirmed)
                .unwrap(),
        );
    }

    #[test]
    fn a_project_without_a_cut_has_nothing_to_suggest() {
        let h = Harness::new();
        let mut app = h.start();
        app.save_provider_key(Provider::TypeSafe, TYPESAFE_KEY)
            .unwrap();
        let project = h.narrated_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        assert!(matches!(
            app.suggest_cuts(&mut editor, BudgetConsent::Ask),
            Err(CutSuggestionError::NothingToCut)
        ));
    }
}
