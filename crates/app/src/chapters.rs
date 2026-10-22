//! YouTube chapters (#182, ADR-0021): a long video's description can list
//! timestamps that YouTube turns into named parts of the progress bar.
//! Claude splits the narration into chapters, each starting at a sentence;
//! the user renames, removes or moves them to the sentence before or after.
//! A chapter's start is a time in the narration, so it follows the cut: the
//! timestamps are where the cut plays those moments.
//!
//! YouTube ignores chapters that break its rules (the first at 0:00, at
//! least three, each at least ten seconds long). Bardo checks them against
//! the cut and leaves them out of the description, with a warning, until
//! they fit; it never blocks an export or upload over them.
//!
//! Writing calls Claude, so it runs as a job.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, Chapter, ChapterFieldError, ChapterProblem, ChapterRepository, Chapters, CostPurpose,
    Generation, GenerationId, Job, JobFailure, JobFailureKind, JobId, JobKind, Narration,
    NarrationRepository, Network, ProfileId, Provider, RenderedPrompt, RepositoryError,
    SecretStore, TemplateKind, TemplateUsed, TemplateValues, TemplateVariable, TemplateVersion,
    TemplateVersionId, TextFormat, TextGenerator, TextRequest, VideoFormat, VideoProject,
    VideoProjectId, chapter_lines, chapter_problems, chapter_title, fitting_chapters,
    place_chapters, sentences,
};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, KeyState, ScriptError, TemplateError, Text};

#[derive(Debug, thiserror::Error)]
pub enum ChaptersError {
    #[error("video project not found")]
    ProjectNotFound,
    /// Chapters are written from the narration: make it first.
    #[error("the project has no narration yet")]
    NoNarration,
    /// The project has no chapters yet: write them first.
    #[error("the project has no chapters yet")]
    NoChapters,
    /// No chapter at that place in the list.
    #[error("no such chapter")]
    NotFound,
    /// The typed title breaks a rule; the card shows it.
    #[error("invalid chapter title: {0:?}")]
    Invalid(ChapterFieldError),
    /// The chapter cannot move that way: the first one stays at the start,
    /// and a chapter does not pass its neighbours.
    #[error("the chapter cannot move there")]
    CannotMove,
    /// Writing calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    #[error("chapters are already being written for this project")]
    Busy,
    /// The call would reach Claude's budget; the card asks before starting
    /// it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ChaptersError {
    /// What the chapters card says.
    pub fn message(&self) -> Text {
        match self {
            ChaptersError::ProjectNotFound => Text::ProjectNotFound,
            ChaptersError::NoNarration => Text::ChaptersNoNarration,
            ChaptersError::NoChapters | ChaptersError::NotFound => Text::ChaptersMissing,
            ChaptersError::Invalid(error) => Text::ChapterFieldError(*error),
            ChaptersError::CannotMove => Text::ChapterCannotMove,
            ChaptersError::MissingKey(_) => Text::ChaptersMissingKey,
            ChaptersError::Busy => Text::ChaptersBusy,
            ChaptersError::OverBudget(_) => Text::BudgetReachedTitle,
            ChaptersError::Template(error) => error.message(),
            ChaptersError::Repository(_) => Text::ChaptersNotSaved,
        }
    }
}

impl From<ScriptError> for ChaptersError {
    /// The project lookups are shared with scripts; only these come back.
    fn from(error: ScriptError) -> Self {
        match error {
            ScriptError::Repository(error) => ChaptersError::Repository(error),
            ScriptError::Template(error) => ChaptersError::Template(error),
            _ => ChaptersError::ProjectNotFound,
        }
    }
}

/// One chapter on the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterRow {
    pub title: String,
    /// Where it starts in the narration.
    pub start: Duration,
    /// Where the cut plays its start; `None` when the cut left it out (or
    /// there is no cut yet).
    pub at: Option<Duration>,
    pub can_move_earlier: bool,
    pub can_move_later: bool,
}

/// A video project's chapters card.
#[derive(Debug, Clone, PartialEq)]
pub struct ChaptersView {
    pub project: VideoProjectId,
    /// Only long videos get chapters: Shorts have none.
    pub long: bool,
    pub has_narration: bool,
    /// Whether the chapters go in the description.
    pub include: bool,
    pub rows: Vec<ChapterRow>,
    /// What keeps YouTube from taking the chapters, against the cut.
    pub problems: Vec<ChapterProblem>,
    /// The narration changed since the chapters were written: their starts
    /// point at the old one.
    pub stale: bool,
    /// Whether the user changed what Claude wrote.
    pub edited: bool,
    /// The lines the description gets; `None` while left out.
    pub lines: Option<String>,
    /// How long the cut is, which timestamps are written for.
    pub length: Duration,
    /// The project's latest chapters job.
    pub job: Option<Job>,
    /// The template version the next writing uses.
    pub template: TemplateVersion,
    /// What writing the chapters would cost; `None` without a narration.
    pub estimate: Option<SpendEstimate>,
}

impl ChaptersView {
    pub fn is_busy(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }

    /// Whether there are chapters to show.
    pub fn has_chapters(&self) -> bool {
        !self.rows.is_empty()
    }
}

/// The chapters job's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ChaptersPayload {
    project: String,
    /// The narration whose sentences the prompt numbers.
    narration: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
}

/// The shape Claude answers in.
const CHAPTERS_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "chapters": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "sentence": {"type": "integer"},
          "title": {"type": "string"}
        },
        "required": ["sentence", "title"],
        "additionalProperties": false
      }
    }
  },
  "required": ["chapters"],
  "additionalProperties": false
}"#;

#[derive(Deserialize)]
struct Answer {
    chapters: Vec<AnsweredChapter>,
}

#[derive(Deserialize)]
struct AnsweredChapter {
    sentence: i64,
    title: String,
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// Claude's call that writes the chapters from `rendered`.
fn chapters_call(rendered: &RenderedPrompt) -> PlannedCall {
    PlannedCall::new(Provider::Claude, CostPurpose::Chapters, 1)
        .with_prompt(&rendered.instructions, &rendered.prompt)
}

/// When each sentence of the narration starts, in order.
fn sentence_starts(narration: &Narration) -> Vec<Duration> {
    sentences(narration)
        .iter()
        .map(|sentence| sentence.start)
        .collect()
}

/// The sentence a chapter starting at `start` starts at: the last one
/// starting no later.
fn sentence_of(starts: &[Duration], start: Duration) -> usize {
    starts
        .iter()
        .rposition(|&sentence| sentence <= start)
        .unwrap_or(0)
}

/// Where the cut plays a project's chapters (with the cut's length), what
/// keeps YouTube from showing them, and the description's lines.
struct Placed {
    at: Option<(Vec<Option<Duration>>, Duration)>,
    problems: Vec<ChapterProblem>,
    lines: Option<String>,
}

/// Where chapter `index` of `list` lands when it moves one sentence
/// earlier or later; `None` when it cannot.
fn moved(list: &[Chapter], index: usize, starts: &[Duration], later: bool) -> Option<Duration> {
    if index == 0 {
        return None;
    }
    let chapter = list.get(index)?;
    let at = sentence_of(starts, chapter.start);
    let to = if later {
        *starts.get(at + 1)?
    } else if starts.get(at).is_some_and(|&start| start < chapter.start) {
        // Between sentences (the narration changed): back to its own.
        starts[at]
    } else {
        *starts.get(at.checked_sub(1)?)?
    };
    let after_previous = list[index - 1].start < to;
    let before_next = list.get(index + 1).is_none_or(|next| to < next.start);
    (after_previous && before_next).then_some(to)
}

/// Runs chapters jobs.
pub(crate) struct ChaptersHandler {
    pub(crate) owner: ProfileId,
    pub(crate) chapters: Arc<dyn ChapterRepository>,
    pub(crate) narrations: Arc<dyn NarrationRepository>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for ChaptersHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.write(payload, cx.id())
    }
}

impl ChaptersHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::Claude)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no Claude key is saved"))
    }

    /// Writes the chapters `job` asked for, in place of the project's.
    pub(crate) fn write(&self, payload: &str, job: JobId) -> Result<(), JobFailure> {
        let payload: ChaptersPayload = serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid chapters payload: {e}")))?;
        let project: VideoProjectId = crate::scenes::id(&payload.project)?;
        // An earlier attempt may have saved them and stopped before the
        // queue recorded the job as done.
        let saved = self.chapters.chapters(project).map_err(unexpected)?;
        if saved.is_some_and(|chapters| chapters.generation().job == Some(job)) {
            return Ok(());
        }
        let narration = self
            .narrations
            .narration(project)
            .map_err(unexpected)?
            .filter(|narration| narration.id.to_string() == payload.narration)
            .ok_or_else(|| {
                JobFailure::unexpected(
                    "the narration changed since the chapters were asked for: write them again",
                )
            })?;
        let key = self.key()?;
        let request = TextRequest {
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            format: TextFormat::Json {
                schema: CHAPTERS_SCHEMA.to_owned(),
            },
        };
        let generated = self.text.generate(&key, &request).map_err(|failure| {
            JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
        })?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::Chapters,
                usage: generated.usage.into(),
                job: Some(job),
                reported: None,
            },
            project,
        );
        let not_usable = |detail: String| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Claude: the chapters are not usable ({detail})"),
            )
        };
        let answer: Answer =
            serde_json::from_str(&generated.text).map_err(|e| not_usable(e.to_string()))?;
        let starts = sentence_starts(&narration);
        let list: Vec<Chapter> = answer
            .chapters
            .into_iter()
            .filter_map(|chapter| {
                let start = *starts.get(usize::try_from(chapter.sentence).ok()?)?;
                let title = chapter_title(&chapter.title).ok()?;
                Some(Chapter { start, title })
            })
            .collect();
        if list.is_empty() {
            return Err(not_usable(
                "no chapter at a sentence of the narration".into(),
            ));
        }
        let now = SystemTime::now();
        let generation = Generation {
            id: GenerationId::new(),
            owner: self.owner,
            project,
            provider: Provider::Claude,
            model: generated.model,
            template: TemplateUsed {
                id: crate::scenes::id::<TemplateVersionId>(&payload.template)?,
                number: payload.template_number,
            },
            instructions: payload.instructions,
            prompt: payload.prompt,
            output: generated.text,
            usage: generated.usage,
            generated_at: now,
            job: Some(job),
        };
        let chapters = Chapters::generated(narration.id, list, generation, now)
            .map_err(|error| not_usable(format!("{error:?}")))?;
        self.chapters.save_chapters(&chapters).map_err(unexpected)
    }
}

impl Bardo {
    fn chapters_project(&self, id: VideoProjectId) -> Result<VideoProject, ChaptersError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ChaptersError::ProjectNotFound)
    }

    /// The facts the chapters template is filled with.
    fn chapters_values(
        &self,
        project: &VideoProject,
        narration: &Narration,
    ) -> Result<TemplateValues, ChaptersError> {
        let mut values = self.script_values(project)?;
        values.insert(
            TemplateVariable::VideoLength,
            crate::music_prompts::length_text(narration.duration),
        );
        values.insert(
            TemplateVariable::NarrationSentences,
            crate::scenes::sentence_lines(narration),
        );
        Ok(values)
    }

    fn render_chapters_prompt(
        &self,
        project: &VideoProject,
        narration: &Narration,
        template: &TemplateVersion,
    ) -> Result<RenderedPrompt, ChaptersError> {
        template
            .body
            .render(&self.chapters_values(project, narration)?)
            .map_err(|missing| {
                // Every chapters variable has a value above.
                ChaptersError::Repository(RepositoryError(Box::new(missing)))
            })
    }

    /// Where the cut plays each chapter's start, and how long the cut is;
    /// `None` without a cut.
    fn placed_chapters(
        &self,
        project: VideoProjectId,
        chapters: &Chapters,
    ) -> Option<(Vec<Option<Duration>>, Duration)> {
        let timeline = self
            .editor_view(project)
            .ok()?
            .timeline
            .filter(|timeline| !timeline.is_empty())?;
        Some((
            place_chapters(chapters.list(), &timeline),
            timeline.duration(),
        ))
    }

    /// Where the cut plays `chapters`, what keeps YouTube from showing
    /// them, and the lines the description gets: none for a Short, when
    /// left out, out of date, breaking a rule or not fitting.
    fn placed_lines(
        &self,
        project: &VideoProject,
        chapters: &Chapters,
        stale: bool,
    ) -> Result<Placed, ChaptersError> {
        let long = self.video_format(project)? == VideoFormat::Long;
        let placed = self.placed_chapters(project.id, chapters);
        let list = chapters.list();
        let mut problems = placed
            .as_ref()
            .map(|(at, length)| chapter_problems(at, *length))
            .unwrap_or_default();
        let mut lines = None;
        if let Some((at, length)) = &placed
            && long
            && chapters.include
            && !stale
            && problems.is_empty()
        {
            let timed: Vec<(Duration, &str)> = at
                .iter()
                .zip(list)
                .filter_map(|(at, chapter)| Some(((*at)?, chapter.title.as_str())))
                .collect();
            let text = chapter_lines(&timed, *length);
            if self.chapters_fit(project, &text)? {
                lines = Some(text);
            } else {
                problems.push(ChapterProblem::DoesNotFit);
            }
        }
        Ok(Placed {
            at: placed,
            problems,
            lines,
        })
    }

    /// Whether `lines` fit in the YouTube description as saved, with the
    /// account's footer.
    fn chapters_fit(&self, project: &VideoProject, lines: &str) -> Result<bool, RepositoryError> {
        let Some(metadata) = self
            .exports
            .video_metadata(project.id)?
            .into_iter()
            .find(|metadata| metadata.network == Network::YouTube)
        else {
            return Ok(true);
        };
        let footer = self
            .network_accounts
            .list(project.channel)?
            .into_iter()
            .find(|account| account.network == Network::YouTube)
            .map(|account| account.details.metadata().description_footer().to_owned())
            .unwrap_or_default();
        Ok(fitting_chapters(Network::YouTube, &metadata.draft(), Some(lines), &footer).is_some())
    }

    /// The project's chapters card.
    pub fn chapters(&self, project: VideoProjectId) -> Result<ChaptersView, ChaptersError> {
        let project = self.chapters_project(project)?;
        let long = self.video_format(&project)? == VideoFormat::Long;
        let narration = self.narrations.narration(project.id)?;
        let template = self.current_template(TemplateKind::Chapters)?;
        let estimate = match &narration {
            Some(narration) => {
                let rendered = self.render_chapters_prompt(&project, narration, &template)?;
                Some(self.estimate(&[chapters_call(&rendered)])?)
            }
            None => None,
        };
        let stored = self.chapters.chapters(project.id)?;
        let starts = narration.as_ref().map(sentence_starts).unwrap_or_default();
        let mut view = ChaptersView {
            project: project.id,
            long,
            has_narration: narration.is_some(),
            include: stored.as_ref().is_some_and(|chapters| chapters.include),
            rows: Vec::new(),
            problems: Vec::new(),
            stale: false,
            edited: stored.as_ref().is_some_and(Chapters::is_edited),
            lines: None,
            length: Duration::ZERO,
            job: self.latest_job_of(JobKind::Chapters, project.id),
            template,
            estimate,
        };
        let Some(chapters) = stored else {
            return Ok(view);
        };
        view.stale = narration
            .as_ref()
            .is_none_or(|narration| narration.id != chapters.narration);
        let placed = self.placed_lines(&project, &chapters, view.stale)?;
        let list = chapters.list();
        view.rows = list
            .iter()
            .enumerate()
            .map(|(index, chapter)| ChapterRow {
                title: chapter.title.clone(),
                start: chapter.start,
                at: placed.at.as_ref().and_then(|(at, _)| at[index]),
                can_move_earlier: !view.stale && moved(list, index, &starts, false).is_some(),
                can_move_later: !view.stale && moved(list, index, &starts, true).is_some(),
            })
            .collect();
        view.length = placed
            .at
            .as_ref()
            .map_or(Duration::ZERO, |(_, length)| *length);
        view.problems = placed.problems;
        view.lines = placed.lines;
        Ok(view)
    }

    /// The chapter lines YouTube's description gets now, if any: no
    /// template or estimate, as the export and the upload review ask often.
    pub(crate) fn chapter_text(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<String>, ChaptersError> {
        let project = self.chapters_project(project)?;
        let Some(chapters) = self.chapters.chapters(project.id)? else {
            return Ok(None);
        };
        let stale = self
            .narrations
            .narration(project.id)?
            .is_none_or(|narration| narration.id != chapters.narration);
        Ok(self.placed_lines(&project, &chapters, stale)?.lines)
    }

    /// Starts a job in which Claude splits the narration into chapters from
    /// the current chapters template. They replace the current ones, edits
    /// included. Past Claude's budget it needs `consent`.
    pub fn write_chapters(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, ChaptersError> {
        let project = self.chapters_project(project)?;
        if self
            .latest_job_of(JobKind::Chapters, project.id)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(ChaptersError::Busy);
        }
        let narration = self
            .narrations
            .narration(project.id)?
            .ok_or(ChaptersError::NoNarration)?;
        if self.provider_key(Provider::Claude).state == KeyState::NotSet {
            return Err(ChaptersError::MissingKey(Provider::Claude));
        }
        let template = self.current_template(TemplateKind::Chapters)?;
        let rendered = self.render_chapters_prompt(&project, &narration, &template)?;
        if let Err(estimate) = self.check_budget(&[chapters_call(&rendered)], consent)? {
            return Err(ChaptersError::OverBudget(estimate));
        }
        let payload = ChaptersPayload {
            project: project.id.to_string(),
            narration: narration.id.to_string(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
        };
        let payload = serde_json::to_string(&payload).expect("a chapters payload serializes");
        let job = Job::new(self.profile.id, JobKind::Chapters, payload);
        Ok(self.jobs.enqueue(job)?)
    }

    /// Applies `change` to the stored chapters and saves them.
    fn change_chapters(
        &self,
        project: VideoProjectId,
        change: impl FnOnce(&mut Vec<Chapter>, &mut bool) -> Result<(), ChaptersError>,
    ) -> Result<(), ChaptersError> {
        let project = self.chapters_project(project)?;
        let mut chapters = self
            .chapters
            .chapters(project.id)?
            .ok_or(ChaptersError::NoChapters)?;
        let mut list = chapters.list().to_vec();
        let mut include = chapters.include;
        change(&mut list, &mut include)?;
        let changed = chapters
            .edit(list, include, SystemTime::now())
            .map_err(|invalid| ChaptersError::Invalid(invalid.error))?;
        if changed {
            self.chapters.save_chapters(&chapters)?;
        }
        Ok(())
    }

    /// Renames chapter `index` (from 0).
    pub fn rename_chapter(
        &self,
        project: VideoProjectId,
        index: usize,
        title: &str,
    ) -> Result<(), ChaptersError> {
        let title = chapter_title(title).map_err(ChaptersError::Invalid)?;
        self.change_chapters(project, |list, _| {
            list.get_mut(index).ok_or(ChaptersError::NotFound)?.title = title;
            Ok(())
        })
    }

    /// Removes chapter `index` (from 0).
    pub fn remove_chapter(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(), ChaptersError> {
        self.change_chapters(project, |list, _| {
            if index >= list.len() {
                return Err(ChaptersError::NotFound);
            }
            list.remove(index);
            Ok(())
        })
    }

    /// Moves chapter `index` (from 0) to the start of the sentence before
    /// (`later` false) or after it.
    pub fn move_chapter(
        &self,
        project: VideoProjectId,
        index: usize,
        later: bool,
    ) -> Result<(), ChaptersError> {
        let narration = self
            .narrations
            .narration(project)?
            .ok_or(ChaptersError::NoNarration)?;
        let starts = sentence_starts(&narration);
        self.change_chapters(project, |list, _| {
            if index >= list.len() {
                return Err(ChaptersError::NotFound);
            }
            list[index].start =
                moved(list, index, &starts, later).ok_or(ChaptersError::CannotMove)?;
            Ok(())
        })
    }

    /// A chapter problem as one sentence, chapters counted from 1.
    pub fn chapter_problem_text(&self, problem: ChapterProblem) -> String {
        let n = match problem {
            ChapterProblem::TooFew | ChapterProblem::DoesNotFit => 0,
            ChapterProblem::TooShort(index) | ChapterProblem::CutAway(index) => index + 1,
        };
        self.text_with(Text::ChapterProblem(problem), &[("n", &n.to_string())])
    }

    /// Puts the chapters in YouTube's description, or leaves them out.
    pub fn include_chapters(
        &self,
        project: VideoProjectId,
        include: bool,
    ) -> Result<(), ChaptersError> {
        self.change_chapters(project, |_, included| {
            *included = include;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::JobState;

    use super::*;
    use crate::scenes::tests::{Harness, done, project, wait_done};

    /// The test narration has four sentences: 0 and 1 a few seconds apart.
    fn answer(chapters: &[(i64, &str)]) -> String {
        serde_json::json!({
            "chapters": chapters
                .iter()
                .map(|(sentence, title)| serde_json::json!({"sentence": sentence, "title": title}))
                .collect::<Vec<_>>()
        })
        .to_string()
    }

    fn write(h: &Harness, app: &Bardo, project: VideoProjectId, text: String) -> Job {
        h.answer(text);
        let id = app.write_chapters(project, BudgetConsent::Ask).unwrap();
        wait_done(app, id)
    }

    #[test]
    fn claude_splits_the_narration_at_sentences() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        assert!(matches!(
            app.write_chapters(project.id, BudgetConsent::Ask),
            Err(ChaptersError::NoNarration)
        ));
        let view = app.chapters(project.id).unwrap();
        assert!(!view.has_narration && view.estimate.is_none() && view.rows.is_empty());

        let (project, _) = h.drawn_project(&app);
        let narration = app.narrations.narration(project.id).unwrap().unwrap();
        let starts = sentence_starts(&narration);
        let job = write(
            &h,
            &app,
            project.id,
            answer(&[(2, " The  signal "), (0, "Launch"), (9, "Nowhere"), (3, "")]),
        );
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(job.kind(), JobKind::Chapters);
        let request = h.text.requests().pop().unwrap();
        assert!(matches!(request.format, TextFormat::Json { .. }));
        assert!(request.prompt.contains("[0] 0:00-"), "{}", request.prompt);
        assert!(
            request
                .prompt
                .contains("Video title: The probe that never came home")
        );
        assert!(request.instructions.contains("in Portuguese"));

        let view = app.chapters(project.id).unwrap();
        assert!(view.long && view.include && !view.stale && !view.edited);
        let rows: Vec<_> = view
            .rows
            .iter()
            .map(|row| (row.start, row.title.as_str()))
            .collect();
        // Out of range and untitled chapters are dropped.
        assert_eq!(rows, [(starts[0], "Launch"), (starts[2], "The signal")]);
        assert_eq!(view.rows[0].at, Some(Duration::ZERO));
        assert!(view.problems.contains(&ChapterProblem::TooFew));
        assert_eq!(
            app.chapter_problem_text(ChapterProblem::TooShort(1)),
            "Chapter 2 is shorter than 10 seconds."
        );
        assert_eq!(view.lines, None, "too few to go in the description");
    }

    #[test]
    fn every_chapters_variable_has_a_value() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        let narration = app.narrations.narration(project.id).unwrap().unwrap();
        let values = app.chapters_values(&project, &narration).unwrap();
        for variable in TemplateKind::Chapters.variables() {
            assert!(values.contains_key(variable), "{variable:?}");
        }
    }

    #[test]
    fn chapters_follow_the_cut_and_go_in_the_description_when_they_fit() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        write(
            &h,
            &app,
            project.id,
            answer(&[(0, "Launch"), (1, "Away"), (2, "Signal"), (3, "Why")]),
        );
        let view = app.chapters(project.id).unwrap();
        assert_eq!(view.rows.len(), 4);
        assert!(view.rows.iter().all(|row| row.at.is_some()));
        // The test narration is short: chapters under ten seconds.
        assert!(
            view.problems
                .iter()
                .any(|problem| matches!(problem, ChapterProblem::TooShort(_)))
        );
        assert_eq!(view.lines, None);
        assert_eq!(app.chapter_text(project.id).unwrap(), None);

        // Fewer, longer chapters would pass: the lines are the cut's times.
        let placed: Vec<_> = view.rows.iter().map(|row| row.at).collect();
        assert_eq!(placed[0], Some(Duration::ZERO));
        assert_eq!(
            chapter_lines(
                &[
                    (Duration::ZERO, "Launch"),
                    (Duration::from_secs(75), "Away")
                ],
                Duration::from_secs(200)
            ),
            "0:00 Launch\n1:15 Away"
        );
    }

    #[test]
    fn chapters_that_fit_go_in_youtubes_description() {
        use bardo_domain::{Network, NetworkAccountDraft};
        let h = Harness::new();
        // Sentences long enough for chapters of ten seconds and more.
        *h.speech.length.lock().unwrap() = Duration::from_secs(80);
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        write(
            &h,
            &app,
            project.id,
            answer(&[(0, "Launch"), (1, "Away"), (2, "Signal")]),
        );
        let view = app.chapters(project.id).unwrap();
        assert_eq!(view.problems, []);
        let lines = view.lines.clone().unwrap();
        assert!(lines.starts_with("0:00 Launch\n0:"), "{lines}");
        assert_eq!(lines.lines().count(), 3);
        assert_eq!(app.chapter_text(project.id).unwrap(), Some(lines.clone()));

        for network in [Network::YouTube, Network::TikTok] {
            crate::export::tests::add_account(
                &app,
                &project,
                network,
                NetworkAccountDraft {
                    description_footer: "Sources.".into(),
                    ..NetworkAccountDraft::default()
                },
            );
        }
        h.answer(crate::export::tests::youtube_and_tiktok());
        done(
            &app,
            app.generate_metadata(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let text = |network: Network| {
            let view = app.export_view(project.id).unwrap();
            crate::export::tests::target(&view, network)
                .post()
                .unwrap()
                .text
                .unwrap()
        };
        assert_eq!(
            text(Network::YouTube),
            format!("In 1969 a probe went silent.\n\n{lines}\n\nSources.")
        );
        assert!(!text(Network::TikTok).contains("0:00"));
        let review = app.upload_review(project.id, Network::YouTube).unwrap();
        assert!(review.post.unwrap().text.unwrap().contains(&lines));

        app.include_chapters(project.id, false).unwrap();
        assert!(!text(Network::YouTube).contains("0:00"));
    }

    #[test]
    fn chapters_that_would_break_the_description_stay_out_of_it() {
        use bardo_domain::{Network, NetworkAccountDraft, VideoMetadataDraft};
        let h = Harness::new();
        *h.speech.length.lock().unwrap() = Duration::from_secs(80);
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        write(
            &h,
            &app,
            project.id,
            answer(&[(0, "Launch"), (1, "Away"), (2, "Signal")]),
        );
        crate::export::tests::add_account(
            &app,
            &project,
            Network::YouTube,
            NetworkAccountDraft::default(),
        );
        h.answer(crate::export::tests::youtube_and_tiktok());
        done(
            &app,
            app.generate_metadata(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let lines = app.chapters(project.id).unwrap().lines.unwrap();
        let saved = app
            .export_view(project.id)
            .unwrap()
            .targets
            .into_iter()
            .find(|target| target.network == Network::YouTube)
            .unwrap()
            .metadata
            .unwrap();
        // Room for the description, not for the chapters too.
        let long = "a".repeat(5_000 - lines.chars().count());
        app.edit_metadata(
            project.id,
            Network::YouTube,
            VideoMetadataDraft {
                description: long.clone(),
                ..saved.draft()
            },
        )
        .unwrap();
        let view = app.chapters(project.id).unwrap();
        assert_eq!(view.problems, [ChapterProblem::DoesNotFit]);
        assert_eq!(view.lines, None);
        let export = app.export_view(project.id).unwrap();
        let target = crate::export::tests::target(&export, Network::YouTube);
        assert_eq!(target.problems, []);
        assert_eq!(target.post().unwrap().text.unwrap(), long);
        assert_eq!(
            app.metadata_draft_problems(
                Network::YouTube,
                &target.metadata.as_ref().unwrap().draft(),
                target.chapters.as_deref(),
                "",
            ),
            []
        );

        // A title YouTube refuses keeps them out too.
        app.edit_metadata(project.id, Network::YouTube, saved.draft())
            .unwrap();
        app.rename_chapter(project.id, 1, "Away <for good>")
            .unwrap();
        let view = app.chapters(project.id).unwrap();
        assert_eq!(view.problems, [ChapterProblem::DoesNotFit]);
        let export = app.export_view(project.id).unwrap();
        assert_eq!(
            crate::export::tests::target(&export, Network::YouTube).problems,
            []
        );
    }

    #[test]
    fn chapters_are_renamed_removed_moved_and_switched_off() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let narration = app.narrations.narration(project.id).unwrap().unwrap();
        let starts = sentence_starts(&narration);
        write(
            &h,
            &app,
            project.id,
            answer(&[(0, "Launch"), (1, "Away"), (3, "Why")]),
        );
        assert_eq!(
            app.rename_chapter(project.id, 1, "  ")
                .unwrap_err()
                .message(),
            Text::ChapterFieldError(ChapterFieldError::TitleRequired)
        );
        app.rename_chapter(project.id, 1, " Far  away ").unwrap();
        let view = app.chapters(project.id).unwrap();
        assert_eq!(view.rows[1].title, "Far away");
        assert!(view.edited);
        assert!(!view.rows[0].can_move_earlier && !view.rows[0].can_move_later);
        assert!(!view.rows[1].can_move_earlier, "sentence 0 holds the first");
        assert!(view.rows[1].can_move_later);
        assert!(view.rows[2].can_move_earlier && !view.rows[2].can_move_later);

        assert!(matches!(
            app.move_chapter(project.id, 0, true),
            Err(ChaptersError::CannotMove)
        ));
        app.move_chapter(project.id, 1, true).unwrap();
        assert_eq!(app.chapters(project.id).unwrap().rows[1].start, starts[2]);
        assert!(matches!(
            app.move_chapter(project.id, 1, true),
            Err(ChaptersError::CannotMove)
        ));
        app.move_chapter(project.id, 2, false).unwrap_err();
        app.move_chapter(project.id, 1, false).unwrap();
        assert_eq!(app.chapters(project.id).unwrap().rows[1].start, starts[1]);

        app.remove_chapter(project.id, 2).unwrap();
        assert_eq!(app.chapters(project.id).unwrap().rows.len(), 2);
        assert!(matches!(
            app.remove_chapter(project.id, 5),
            Err(ChaptersError::NotFound)
        ));

        app.include_chapters(project.id, false).unwrap();
        let view = app.chapters(project.id).unwrap();
        assert!(!view.include);
        assert_eq!(view.lines, None);

        // Writing again replaces the edits.
        write(&h, &app, project.id, answer(&[(0, "Start"), (2, "End")]));
        let view = app.chapters(project.id).unwrap();
        assert!(view.include && !view.edited);
        assert_eq!(view.rows[0].title, "Start");
    }

    #[test]
    fn a_new_narration_makes_the_chapters_stale() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        write(
            &h,
            &app,
            project.id,
            answer(&[(0, "Launch"), (2, "Signal")]),
        );
        assert!(!app.chapters(project.id).unwrap().stale);
        done(
            &app,
            app.generate_narration(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let view = app.chapters(project.id).unwrap();
        assert!(view.stale);
        assert!(view.rows.iter().all(|row| !row.can_move_later));
        assert_eq!(view.lines, None);
    }

    #[test]
    fn shorts_get_no_chapters_in_the_description() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        app.set_project_format(project.id, Some(VideoFormat::Short))
            .unwrap();
        assert!(!app.chapters(project.id).unwrap().long);
    }

    #[test]
    fn a_resumed_job_does_not_write_twice_and_one_runs_at_a_time() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        let job = write(
            &h,
            &app,
            project.id,
            answer(&[(0, "Launch"), (2, "Signal")]),
        );
        let handler = ChaptersHandler {
            owner: app.profile().id,
            chapters: Arc::clone(&h.db) as _,
            narrations: Arc::clone(&h.db) as _,
            text: Arc::clone(&h.text) as _,
            secrets: Arc::clone(&h.secrets) as _,
            costs: CostBook {
                owner: app.profile().id,
                costs: Arc::clone(&h.db) as _,
                themes: Arc::clone(&h.db) as _,
            },
        };
        let asked = h.text.requests().len();
        handler.write(job.payload(), job.id()).unwrap();
        assert_eq!(h.text.requests().len(), asked);

        h.answer(answer(&[(0, "Launch")]));
        *h.text.delay.lock().unwrap() = Duration::from_millis(100);
        let id = app.write_chapters(project.id, BudgetConsent::Ask).unwrap();
        assert!(matches!(
            app.write_chapters(project.id, BudgetConsent::Ask),
            Err(ChaptersError::Busy)
        ));
        done(&app, id);
    }
}
