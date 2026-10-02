//! A video project's stages, from script to publishing, and where each
//! stands: done, partly done, waiting on the user, open, or locked until
//! an earlier stage gives it something to work on. Every layout shows the
//! same stages and states; this decides them from the project's views.

use std::time::Duration;

use bardo_domain::Scene;

use bardo_domain::JobState;

use crate::{Bardo, NarrationView, RenderSummary, ScenesView, ScriptView, Text};

/// A step of making a video, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Script,
    Narration,
    Scenes,
    Clips,
    Edit,
    Render,
    Publish,
}

impl Stage {
    pub const ALL: [Stage; 7] = [
        Stage::Script,
        Stage::Narration,
        Stage::Scenes,
        Stage::Clips,
        Stage::Edit,
        Stage::Render,
        Stage::Publish,
    ];

    /// Whether the stage is worked on in the projects screen; Edit opens
    /// the editor instead.
    pub fn is_page(self) -> bool {
        matches!(
            self,
            Stage::Script | Stage::Narration | Stage::Scenes | Stage::Clips | Stage::Render
        )
    }
}

/// Where a stage stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageState {
    Done,
    /// A job of the stage is running.
    Working,
    /// Something waits on the user: a new version to review, or work made
    /// from an older version of an earlier stage.
    Attention,
    /// Partly done.
    Partial,
    /// Nothing done yet, and nothing in the way.
    Open,
    /// Waits on an earlier stage, or is not available yet.
    Locked,
}

/// The stage's status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageNote {
    ScriptMissing,
    ScriptToReview,
    ScriptWords(usize),
    NarrationMissing,
    NarrationStale,
    NarrationLength(Duration),
    ScenesMissing,
    ScenesStale,
    ImagesToReview(usize),
    Images {
        done: usize,
        total: usize,
    },
    ClipsToReview(usize),
    Clips {
        done: usize,
        total: usize,
    },
    EditorReady,
    /// The cut can be reviewed and rendered; nothing rendered yet.
    RenderReady,
    /// Files rendered from the cut as it is now.
    Rendered(usize),
    /// Files rendered from an earlier cut or preset.
    RenderOutdated(usize),
    /// A render job is running.
    Rendering,
    /// The last render failed or was cancelled; it can resume.
    RenderStopped,
    Working,
    /// Locked until that stage has something to give.
    After(Stage),
    /// Locked: Bardo does not do it yet.
    NotYet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageStatus {
    pub stage: Stage,
    pub state: StageState,
    pub note: StageNote,
}

impl StageStatus {
    fn new(stage: Stage, state: StageState, note: StageNote) -> Self {
        Self { stage, state, note }
    }

    fn locked(stage: Stage, note: StageNote) -> Self {
        Self::new(stage, StageState::Locked, note)
    }

    fn working(stage: Stage) -> Self {
        Self::new(stage, StageState::Working, StageNote::Working)
    }
}

/// Every stage of a project, in order, from its script, narration, scenes
/// and renders as the projects screen loads them.
pub fn project_stages(
    script: &ScriptView,
    narration: &NarrationView,
    scenes: &ScenesView,
    renders: &RenderSummary,
) -> Vec<StageStatus> {
    let running = |job: Option<&bardo_domain::Job>| job.is_some_and(|job| job.state().is_active());

    let script_stage = if running(script.job.as_ref()) {
        StageStatus::working(Stage::Script)
    } else {
        match &script.script {
            None => StageStatus::new(Stage::Script, StageState::Open, StageNote::ScriptMissing),
            Some(script) if script.pending().is_some() => StageStatus::new(
                Stage::Script,
                StageState::Attention,
                StageNote::ScriptToReview,
            ),
            Some(script) => StageStatus::new(
                Stage::Script,
                StageState::Done,
                StageNote::ScriptWords(script.text().word_count()),
            ),
        }
    };

    let narration_stage = if narration.script.is_none() {
        StageStatus::locked(Stage::Narration, StageNote::After(Stage::Script))
    } else if running(narration.job.as_ref()) {
        StageStatus::working(Stage::Narration)
    } else {
        match &narration.narration {
            None => StageStatus::new(
                Stage::Narration,
                StageState::Open,
                StageNote::NarrationMissing,
            ),
            Some(_) if narration.stale => StageStatus::new(
                Stage::Narration,
                StageState::Attention,
                StageNote::NarrationStale,
            ),
            Some(recorded) => StageStatus::new(
                Stage::Narration,
                StageState::Done,
                StageNote::NarrationLength(recorded.duration),
            ),
        }
    };

    let plan = scenes.plan.as_ref();
    let scene_list = plan.map_or(&[][..], |plan| plan.scenes());
    let total = scene_list.len();
    let count = |test: fn(&Scene) -> bool| scene_list.iter().filter(|s| test(s)).count();

    let scenes_stage = if scenes.narration.is_none() {
        StageStatus::locked(Stage::Scenes, StageNote::After(Stage::Narration))
    } else if running(scenes.job.as_ref()) {
        StageStatus::working(Stage::Scenes)
    } else if plan.is_none() {
        StageStatus::new(Stage::Scenes, StageState::Open, StageNote::ScenesMissing)
    } else if scenes.stale {
        StageStatus::new(Stage::Scenes, StageState::Attention, StageNote::ScenesStale)
    } else {
        let to_review = count(|scene| scene.pending().is_some());
        let drawn = count(|scene| scene.image().is_some());
        if to_review > 0 {
            StageStatus::new(
                Stage::Scenes,
                StageState::Attention,
                StageNote::ImagesToReview(to_review),
            )
        } else {
            let state = if drawn == total {
                StageState::Done
            } else {
                StageState::Partial
            };
            StageStatus::new(
                Stage::Scenes,
                state,
                StageNote::Images { done: drawn, total },
            )
        }
    };

    let clips_stage = if !scene_list.iter().any(Scene::can_animate) {
        StageStatus::locked(Stage::Clips, StageNote::After(Stage::Scenes))
    } else if scenes.clips.scenes.iter().any(|clip| clip.is_busy()) {
        StageStatus::working(Stage::Clips)
    } else {
        let to_review = count(|scene| scene.pending_clip().is_some());
        let animated = count(|scene| scene.clip().is_some());
        if to_review > 0 {
            StageStatus::new(
                Stage::Clips,
                StageState::Attention,
                StageNote::ClipsToReview(to_review),
            )
        } else {
            let state = match animated {
                0 => StageState::Open,
                done if done == total => StageState::Done,
                _ => StageState::Partial,
            };
            StageStatus::new(
                Stage::Clips,
                state,
                StageNote::Clips {
                    done: animated,
                    total,
                },
            )
        }
    };

    // The rough cut is built from the planned scenes over the narration.
    let edit_stage = if plan.is_none() {
        StageStatus::locked(Stage::Edit, StageNote::After(Stage::Scenes))
    } else {
        StageStatus::new(Stage::Edit, StageState::Open, StageNote::EditorReady)
    };

    let render_job = renders.job.as_ref().map(|job| job.state());
    let render_stage = if !renders.has_cut {
        StageStatus::locked(Stage::Render, StageNote::After(Stage::Edit))
    } else if render_job.is_some_and(JobState::is_active) {
        StageStatus::new(Stage::Render, StageState::Working, StageNote::Rendering)
    } else if matches!(render_job, Some(JobState::Failed | JobState::Cancelled)) {
        StageStatus::new(
            Stage::Render,
            StageState::Attention,
            StageNote::RenderStopped,
        )
    } else if renders.outdated > 0 {
        StageStatus::new(
            Stage::Render,
            StageState::Attention,
            StageNote::RenderOutdated(renders.outdated),
        )
    } else if renders.current > 0 {
        StageStatus::new(
            Stage::Render,
            StageState::Done,
            StageNote::Rendered(renders.current),
        )
    } else {
        StageStatus::new(Stage::Render, StageState::Open, StageNote::RenderReady)
    };

    vec![
        script_stage,
        narration_stage,
        scenes_stage,
        clips_stage,
        edit_stage,
        render_stage,
        StageStatus::locked(Stage::Publish, StageNote::NotYet),
    ]
}

impl Bardo {
    /// A stage's status line in the interface language.
    pub fn stage_note(&self, note: StageNote) -> String {
        let n = |text: Text, n: usize| self.text_with(text, &[("n", &n.to_string())]);
        let of = |text: Text, done: usize, total: usize| {
            self.text_with(
                text,
                &[("done", &done.to_string()), ("total", &total.to_string())],
            )
        };
        match note {
            StageNote::ScriptMissing => self.text(Text::StageScriptMissing).into_owned(),
            StageNote::ScriptToReview => self.text(Text::StageScriptToReview).into_owned(),
            StageNote::ScriptWords(words) => n(Text::StageScriptWords, words),
            StageNote::NarrationMissing => self.text(Text::StageNarrationMissing).into_owned(),
            StageNote::NarrationStale => self.text(Text::StageNarrationStale).into_owned(),
            StageNote::NarrationLength(length) => {
                let seconds = length.as_secs();
                format!("{}:{:02}", seconds / 60, seconds % 60)
            }
            StageNote::ScenesMissing => self.text(Text::StageScenesMissing).into_owned(),
            StageNote::ScenesStale => self.text(Text::StageScenesStale).into_owned(),
            StageNote::ImagesToReview(count) | StageNote::ClipsToReview(count) => {
                n(Text::StageToReview, count)
            }
            StageNote::Images { done, total } => of(Text::StageImages, done, total),
            StageNote::Clips { done, total } => of(Text::StageClips, done, total),
            StageNote::EditorReady => self.text(Text::StageEditorReady).into_owned(),
            StageNote::RenderReady => self.text(Text::StageRenderReady).into_owned(),
            StageNote::Rendered(files) => n(Text::StageRendered, files),
            StageNote::RenderOutdated(files) => n(Text::StageRenderOutdated, files),
            StageNote::Rendering => self.text(Text::StageRendering).into_owned(),
            StageNote::RenderStopped => self.text(Text::StageRenderStopped).into_owned(),
            StageNote::Working => self.text(Text::StageWorking).into_owned(),
            StageNote::After(stage) => self.text(Text::StageAfter(stage)).into_owned(),
            StageNote::NotYet => self.text(Text::StageNotYet).into_owned(),
        }
    }
}

/// The stage a project opens on: the first page stage with work left,
/// else the last one that can be opened.
pub fn opening_stage(stages: &[StageStatus]) -> Stage {
    let pages = || stages.iter().filter(|status| status.stage.is_page());
    pages()
        .find(|status| !matches!(status.state, StageState::Done | StageState::Locked))
        .or_else(|| {
            pages()
                .rev()
                .find(|status| status.state != StageState::Locked)
        })
        .map_or(Stage::Script, |status| status.stage)
}

/// Where one scene stands at the Scenes or Clips stage: what its card
/// says, and whether the "Pending" filter keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneState {
    Done,
    /// Has no image yet.
    ToDraw,
    /// Has an image and no clip yet.
    ToAnimate,
    /// Its clip is being made.
    Animating,
    /// A new image or clip waits for the user to keep it or not.
    Review,
    /// Its last image or clip failed.
    Failed,
}

impl SceneState {
    /// Whether the scene still needs something at that stage.
    pub fn is_pending(self) -> bool {
        self != SceneState::Done
    }
}

/// Every planned scene's state at `stage` (Scenes or Clips), in order;
/// empty without a plan.
pub fn scene_states(scenes: &ScenesView, stage: Stage) -> Vec<SceneState> {
    let Some(plan) = &scenes.plan else {
        return Vec::new();
    };
    plan.scenes()
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            if stage == Stage::Clips {
                let busy = scenes
                    .clips
                    .scenes
                    .get(index)
                    .is_some_and(|clip| clip.is_busy());
                if busy {
                    SceneState::Animating
                } else if scene.pending_clip().is_some() {
                    SceneState::Review
                } else if scene.clip_failure().is_some() {
                    SceneState::Failed
                } else if !scene.can_animate() {
                    SceneState::ToDraw
                } else if scene.clip().is_none() {
                    SceneState::ToAnimate
                } else {
                    SceneState::Done
                }
            } else if scene.pending().is_some() {
                SceneState::Review
            } else if scene.failure().is_some() {
                SceneState::Failed
            } else if scene.image().is_none() {
                SceneState::ToDraw
            } else {
                SceneState::Done
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use bardo_domain::{Job, JobKind, ProfileId, VideoProject};

    use super::*;
    use crate::scenes::tests::{Harness, done, project};
    use crate::{Bardo, BudgetConsent};

    fn stages(app: &Bardo, project: &VideoProject) -> Vec<StageStatus> {
        project_stages(
            &app.script(project.id).unwrap(),
            &app.narration(project.id).unwrap(),
            &app.scenes(project.id).unwrap(),
            &app.render_summary(project.id).unwrap(),
        )
    }

    fn of(stages: &[StageStatus], stage: Stage) -> (StageState, StageNote) {
        let status = stages.iter().find(|s| s.stage == stage).unwrap();
        (status.state, status.note)
    }

    #[test]
    fn a_new_project_starts_at_its_script_with_the_rest_locked_and_saying_why() {
        let h = Harness::new();
        let app = h.start();
        let stages = stages(&app, &project(&app));

        assert_eq!(
            stages.iter().map(|s| s.stage).collect::<Vec<_>>(),
            Stage::ALL
        );
        assert_eq!(
            of(&stages, Stage::Script),
            (StageState::Open, StageNote::ScriptMissing)
        );
        assert_eq!(
            of(&stages, Stage::Narration),
            (StageState::Locked, StageNote::After(Stage::Script))
        );
        assert_eq!(
            of(&stages, Stage::Scenes),
            (StageState::Locked, StageNote::After(Stage::Narration))
        );
        for stage in [Stage::Clips, Stage::Edit] {
            assert_eq!(
                of(&stages, stage),
                (StageState::Locked, StageNote::After(Stage::Scenes)),
                "{stage:?}"
            );
        }
        assert_eq!(
            of(&stages, Stage::Render),
            (StageState::Locked, StageNote::After(Stage::Edit))
        );
        assert_eq!(
            of(&stages, Stage::Publish),
            (StageState::Locked, StageNote::NotYet)
        );
        assert_eq!(opening_stage(&stages), Stage::Script);
    }

    #[test]
    fn a_narrated_project_opens_on_its_scenes() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        let stages = stages(&app, &project);

        let words = app
            .script(project.id)
            .unwrap()
            .script
            .unwrap()
            .text()
            .word_count();
        assert_eq!(
            of(&stages, Stage::Script),
            (StageState::Done, StageNote::ScriptWords(words))
        );
        let length = app
            .narration(project.id)
            .unwrap()
            .narration
            .unwrap()
            .duration;
        assert_eq!(
            of(&stages, Stage::Narration),
            (StageState::Done, StageNote::NarrationLength(length))
        );
        assert_eq!(
            of(&stages, Stage::Scenes),
            (StageState::Open, StageNote::ScenesMissing)
        );
        assert_eq!(opening_stage(&stages), Stage::Scenes);
    }

    #[test]
    fn planned_scenes_count_their_images_and_open_the_editor() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        let stages = stages(&app, &project);

        assert_eq!(
            of(&stages, Stage::Scenes),
            (StageState::Partial, StageNote::Images { done: 0, total: 3 })
        );
        assert_eq!(
            of(&stages, Stage::Clips),
            (StageState::Locked, StageNote::After(Stage::Scenes)),
            "a clip starts from an image"
        );
        assert_eq!(
            of(&stages, Stage::Edit),
            (StageState::Open, StageNote::EditorReady)
        );
        assert_eq!(
            of(&stages, Stage::Render),
            (StageState::Open, StageNote::RenderReady)
        );
        assert_eq!(opening_stage(&stages), Stage::Scenes);
    }

    #[test]
    fn drawn_scenes_hand_over_to_clips_and_new_versions_wait_for_review() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let drawn = stages(&app, &project);
        assert_eq!(
            of(&drawn, Stage::Scenes),
            (StageState::Done, StageNote::Images { done: 3, total: 3 })
        );
        assert_eq!(
            of(&drawn, Stage::Clips),
            (StageState::Open, StageNote::Clips { done: 0, total: 3 })
        );
        assert_eq!(opening_stage(&drawn), Stage::Clips);

        done(
            &app,
            app.regenerate_scene_image(project.id, 1, BudgetConsent::Ask)
                .unwrap(),
        );
        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        let review = stages(&app, &project);
        assert_eq!(
            of(&review, Stage::Scenes),
            (StageState::Attention, StageNote::ImagesToReview(1))
        );
        assert_eq!(
            of(&review, Stage::Clips),
            (StageState::Attention, StageNote::ClipsToReview(1))
        );
        assert_eq!(opening_stage(&review), Stage::Scenes);

        app.accept_scene_image(project.id, 1).unwrap();
        app.accept_scene_clip(project.id, 0).unwrap();
        let accepted = stages(&app, &project);
        assert_eq!(of(&accepted, Stage::Scenes).0, StageState::Done);
        assert_eq!(
            of(&accepted, Stage::Clips),
            (StageState::Partial, StageNote::Clips { done: 1, total: 3 })
        );
    }

    #[test]
    fn an_edited_script_puts_its_narration_out_of_date() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        app.edit_script(project.id, "A different script, read again.")
            .unwrap();
        let stages = stages(&app, &project);
        assert_eq!(
            of(&stages, Stage::Narration),
            (StageState::Attention, StageNote::NarrationStale)
        );
        assert_eq!(opening_stage(&stages), Stage::Narration);
    }

    #[test]
    fn status_lines_read_in_the_interface_language() {
        let h = Harness::new();
        let app = h.start();
        assert_eq!(
            app.stage_note(StageNote::Images { done: 4, total: 6 }),
            "4 of 6 images"
        );
        assert_eq!(
            app.stage_note(StageNote::NarrationLength(Duration::from_secs(96))),
            "1:36"
        );
        assert_eq!(
            app.stage_note(StageNote::After(Stage::Scenes)),
            "After the scenes"
        );
    }

    #[test]
    fn each_scene_says_what_it_still_needs_at_its_stage() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        let planned = app.scenes(project.id).unwrap();
        assert_eq!(
            scene_states(&planned, Stage::Scenes),
            vec![SceneState::ToDraw; 3]
        );
        assert_eq!(
            scene_states(&planned, Stage::Clips),
            vec![SceneState::ToDraw; 3],
            "a clip starts from an image"
        );

        let (project, _) = h.drawn_project(&app);
        done(
            &app,
            app.regenerate_scene_image(project.id, 1, BudgetConsent::Ask)
                .unwrap(),
        );
        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        let view = app.scenes(project.id).unwrap();
        assert_eq!(
            scene_states(&view, Stage::Scenes),
            vec![SceneState::Done, SceneState::Review, SceneState::Done]
        );
        let clips = scene_states(&view, Stage::Clips);
        assert_eq!(
            clips,
            vec![
                SceneState::Review,
                SceneState::ToAnimate,
                SceneState::ToAnimate
            ]
        );
        assert_eq!(clips.iter().filter(|s| s.is_pending()).count(), 3);
        assert!(!SceneState::Done.is_pending());
    }

    #[test]
    fn with_every_page_done_the_project_opens_on_its_last_page() {
        let done = |stage| StageStatus::new(stage, StageState::Done, StageNote::Working);
        let mut stages: Vec<_> = Stage::ALL.into_iter().map(done).collect();
        assert_eq!(opening_stage(&stages), Stage::Render);
        stages[5] = StageStatus::locked(Stage::Render, StageNote::After(Stage::Edit));
        assert_eq!(opening_stage(&stages), Stage::Clips);
    }

    /// The Render stage of a project with a cut, from its summary.
    fn render_stage(summary: RenderSummary) -> (StageState, StageNote) {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        let stages = project_stages(
            &app.script(project.id).unwrap(),
            &app.narration(project.id).unwrap(),
            &app.scenes(project.id).unwrap(),
            &RenderSummary {
                has_cut: true,
                ..summary
            },
        );
        of(&stages, Stage::Render)
    }

    #[test]
    fn the_render_stage_follows_its_files_and_its_last_job() {
        let job = |state: Option<JobState>| {
            let mut job = Job::new(ProfileId::new(), JobKind::Render, "{}");
            match state {
                Some(JobState::Cancelled) => job.cancel().unwrap(),
                Some(JobState::Done) => {
                    job.start(SystemTime::now()).unwrap();
                    job.complete().unwrap();
                }
                _ => {}
            }
            Some(job)
        };
        assert_eq!(
            render_stage(RenderSummary {
                current: 2,
                ..RenderSummary::default()
            }),
            (StageState::Done, StageNote::Rendered(2))
        );
        assert_eq!(
            render_stage(RenderSummary {
                current: 1,
                outdated: 1,
                job: job(Some(JobState::Done)),
                ..RenderSummary::default()
            }),
            (StageState::Attention, StageNote::RenderOutdated(1))
        );
        assert_eq!(
            render_stage(RenderSummary {
                current: 2,
                job: job(None),
                ..RenderSummary::default()
            }),
            (StageState::Working, StageNote::Rendering)
        );
        assert_eq!(
            render_stage(RenderSummary {
                current: 1,
                job: job(Some(JobState::Cancelled)),
                ..RenderSummary::default()
            }),
            (StageState::Attention, StageNote::RenderStopped)
        );
    }
}
