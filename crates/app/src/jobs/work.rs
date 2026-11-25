//! What a job that asks Claude for text works on, so that two of them
//! never work on the same thing side by side.

use bardo_domain::{Job, JobId, JobKind};
use serde::Deserialize;

use super::Kept;
use crate::Bardo;

/// What a text job works on: one text of a project, the ideas of a
/// channel, the look of a persona, one take of a capture or the sheet of a
/// product. Two jobs with
/// the same `Work` would pay for the same thing and save over each other,
/// so one is not started, or queued again, while another is at work
/// ([`Bardo::work_busy`]).
///
/// The jobs that draw what such a text describes (a thumbnail's
/// background, a look, a performed take) share the work of their text:
/// its busy check is theirs too. Scenes have their own (`scenes_busy`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Work {
    /// The project's script: written, or adapted again for a version.
    Script(String),
    /// The versions made of the project for other channels.
    Versions(String),
    Chapters(String),
    Thumbnail(String),
    MusicPrompt(String),
    Metadata(String),
    /// The ideas proposed and ranked for the channel.
    Themes(String),
    /// The persona's look.
    Look(String),
    /// One take of a capture: its prompt, test, draft and final.
    Take {
        project: String,
        capture: String,
        position: u32,
    },
    /// The product's sheet: researched on the web, or written from the
    /// brief.
    Sheet(String),
}

/// The fields the payloads of these jobs name what they work on with.
#[derive(Deserialize)]
struct WorksOn {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    persona: Option<String>,
    #[serde(default)]
    capture: Option<String>,
    #[serde(default)]
    position: Option<u32>,
    #[serde(default)]
    product: Option<String>,
}

impl Work {
    /// What `job` works on; `None` for a job of another kind.
    pub(crate) fn of(job: &Job) -> Option<Self> {
        let kind = job.kind();
        if !matches!(
            kind,
            JobKind::ScriptGeneration
                | JobKind::Localization
                | JobKind::Chapters
                | JobKind::Thumbnail
                | JobKind::ThumbnailImage
                | JobKind::MusicPrompt
                | JobKind::Metadata
                | JobKind::ThemeSuggestion
                | JobKind::ThemeRanking
                | JobKind::PersonaLook
                | JobKind::TakePrompt
                | JobKind::PerformedTake
                | JobKind::ProductResearch
        ) {
            return None;
        }
        let on: WorksOn = serde_json::from_str(job.payload()).ok()?;
        Some(match kind {
            JobKind::ScriptGeneration => Self::Script(on.project?),
            // With a channel it makes a version of the project for it;
            // without, it adapts the version's script again.
            JobKind::Localization => match on.channel {
                Some(_) => Self::Versions(on.project?),
                None => Self::Script(on.project?),
            },
            JobKind::Chapters => Self::Chapters(on.project?),
            JobKind::Thumbnail | JobKind::ThumbnailImage => Self::Thumbnail(on.project?),
            JobKind::MusicPrompt => Self::MusicPrompt(on.project?),
            JobKind::Metadata => Self::Metadata(on.project?),
            JobKind::ThemeSuggestion | JobKind::ThemeRanking => Self::Themes(on.channel?),
            JobKind::PersonaLook => Self::Look(on.persona?),
            JobKind::TakePrompt | JobKind::PerformedTake => Self::Take {
                project: on.project?,
                capture: on.capture?,
                position: on.position?,
            },
            JobKind::ProductResearch => Self::Sheet(on.product?),
            _ => return None,
        })
    }
}

impl Bardo {
    /// Whether `job` is at work: queued or running, or no longer reading
    /// so while this process still runs its handler. A cancelled job reads
    /// cancelled at once, and its handler may still be in a paid request
    /// it has yet to give up, or saving what that request brought.
    pub(crate) fn at_work(&self, job: &Job) -> bool {
        job.state().is_active() || self.jobs.runs_here(job.id())
    }

    /// Whether a job is at work on `work`: the latest, an older one
    /// queued again from Jobs, or one that was cancelled and is still
    /// stopping.
    pub(crate) fn work_busy(&self, work: &Work) -> bool {
        self.jobs()
            .iter()
            .any(|job| self.at_work(job) && Work::of(job).as_ref() == Some(work))
    }

    /// A paid answer for `work` that is asked for again: the latest job on
    /// `work`, failed or cancelled, that holds an answer it has yet to
    /// apply ([`Kept`]) and that `same` says asked for the same thing. Its
    /// stage queues that job again instead of starting a new one, so
    /// nothing is asked of Claude, or of the budget.
    pub(crate) fn kept_answer(&self, work: &Work, same: impl Fn(&Job) -> bool) -> Option<JobId> {
        self.jobs()
            .into_iter()
            .rev()
            .find(|job| Work::of(job).as_ref() == Some(work))
            .filter(|job| job.can_retry() && Kept::held_by(job) && same(job))
            .map(|job| job.id())
    }

    /// Whether queueing `job` again would put it beside another job at
    /// work on the same thing, itself included until the run that was
    /// cancelled has returned.
    pub(crate) fn retry_beside_its_work(&self, job: &Job) -> bool {
        Work::of(job).is_some_and(|work| self.work_busy(&work))
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::ProfileId;

    use super::*;

    fn work(kind: JobKind, payload: serde_json::Value) -> Option<Work> {
        Work::of(&Job::new(ProfileId::new(), kind, payload.to_string()))
    }

    #[test]
    fn each_text_job_names_what_it_works_on() {
        let on_project = serde_json::json!({"project": "p", "prompt": "…"});
        for (kind, expected) in [
            (JobKind::ScriptGeneration, Work::Script("p".into())),
            (JobKind::Chapters, Work::Chapters("p".into())),
            (JobKind::Thumbnail, Work::Thumbnail("p".into())),
            (JobKind::ThumbnailImage, Work::Thumbnail("p".into())),
            (JobKind::MusicPrompt, Work::MusicPrompt("p".into())),
            (JobKind::Metadata, Work::Metadata("p".into())),
            // Without a channel, the version's script is adapted again.
            (JobKind::Localization, Work::Script("p".into())),
        ] {
            assert_eq!(work(kind, on_project.clone()), Some(expected), "{kind:?}");
        }
        assert_eq!(
            work(
                JobKind::Localization,
                serde_json::json!({"project": "p", "channel": "c", "version": "v"})
            ),
            Some(Work::Versions("p".into()))
        );
        for kind in [JobKind::ThemeSuggestion, JobKind::ThemeRanking] {
            assert_eq!(
                work(kind, serde_json::json!({"channel": "c", "niche": "n"})),
                Some(Work::Themes("c".into()))
            );
        }
        assert_eq!(
            work(JobKind::PersonaLook, serde_json::json!({"persona": "n"})),
            Some(Work::Look("n".into()))
        );
        let take = Work::Take {
            project: "p".into(),
            capture: "c".into(),
            position: 2,
        };
        for kind in [JobKind::TakePrompt, JobKind::PerformedTake] {
            let payload = serde_json::json!({
                "project": "p", "capture": "c", "position": 2, "persona": "n"
            });
            assert_eq!(work(kind, payload), Some(take.clone()));
        }
        assert_eq!(
            work(
                JobKind::ProductResearch,
                serde_json::json!({"product": "d", "source": "research"})
            ),
            Some(Work::Sheet("d".into()))
        );
    }

    #[test]
    fn other_jobs_and_unreadable_payloads_name_nothing() {
        assert_eq!(
            work(JobKind::Render, serde_json::json!({"project": "p"})),
            None
        );
        assert_eq!(
            work(JobKind::ScenePlan, serde_json::json!({"project": "p"})),
            None
        );
        assert_eq!(
            work(JobKind::Chapters, serde_json::json!({"channel": "c"})),
            None
        );
        assert_eq!(
            Work::of(&Job::new(ProfileId::new(), JobKind::Chapters, "not json")),
            None
        );
    }
}
