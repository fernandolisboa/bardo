//! The open project at a glance (Overview): its videos in progress, each
//! with the stage it is at and the step that moves it on, the ideas that
//! wait for the user, its channels, this month's spending and narrators.
//!
//! The next step is derived from the stages ([`project_stages`]) and the
//! video's publications, in one function, [`next_step`].

use std::collections::HashSet;
use std::time::SystemTime;

use bardo_domain::{
    Channel, ChannelId, Money, Publication, ThemeStatus, UploadStatus, VideoFormat, VideoProjectId,
};

use crate::{
    AppError, Bardo, SpendEstimate, Stage, StageNote, StageState, StageStatus, Text, project_stages,
};

/// What moves a video on, or why nothing does yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextStep {
    /// Something for the user to do.
    Do(NextAction),
    /// A job of the stage runs (or the upload goes to the network): nothing
    /// to do until it ends.
    Working(Stage),
    /// Exported or uploaded, and waiting to go out: Bardo publishes it at
    /// `at`, or the network makes it public then. `None` when the time is
    /// not known (a post the network holds for its own review).
    WaitingOnPost { at: Option<SystemTime> },
    /// Posted: no longer in progress.
    Finished,
}

impl NextStep {
    /// Whether the video still counts as in progress.
    pub fn in_progress(&self) -> bool {
        *self != NextStep::Finished
    }

    /// The stage the step is at; `None` once posted or while waiting.
    pub fn stage(&self) -> Option<Stage> {
        match self {
            NextStep::Do(action) => Some(action.stage),
            NextStep::Working(stage) => Some(*stage),
            NextStep::WaitingOnPost { .. } | NextStep::Finished => None,
        }
    }
}

/// A step for the user, where it is done and what it would cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextAction {
    /// The stage to open; Edit opens the editor.
    pub stage: Stage,
    pub step: StepKind,
    /// What the step would spend, when it calls a paid provider.
    pub estimate: Option<SpendEstimate>,
}

/// What the user does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    ConfirmBrief,
    /// Keep or drop the claims that changed in the product.
    DecideClaims(usize),
    CompleteBrief,
    WriteScript,
    ReviewScript,
    /// The brief changed after the script was written.
    RewriteScript,
    RecordNarration,
    RecordAgain,
    FixTakes(usize),
    PlanScenes,
    PlaceScenes,
    PlanAgain,
    ReviewImages(usize),
    DrawImages(usize),
    ReviewClips(usize),
    AnimateClips(usize),
    /// Open the editor: the render waits for a cut.
    BuildCut,
    Render,
    RenderAgain,
    ResumeRender,
    Export,
    ExportAgain,
    ExportRest,
    ResumeExport,
    /// Exported and nothing posted yet: upload it, or link the post.
    Post,
}

/// What generating the next step would cost, from the stages' own views;
/// `None` where a step calls nothing paid or its view had no estimate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepEstimates {
    pub script: Option<SpendEstimate>,
    pub narration: Option<SpendEstimate>,
    pub plan: Option<SpendEstimate>,
    pub images: Option<SpendEstimate>,
    pub clips: Option<SpendEstimate>,
}

/// Where a video's posts stand, from its publications.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Posts {
    /// An upload on its way: queued, uploading or processing.
    pub sending: bool,
    /// A post that waits to go out, with when, if known.
    pub waiting: Option<Option<SystemTime>>,
    /// A post on the network.
    pub posted: bool,
}

impl Posts {
    pub fn of(publications: &[Publication]) -> Self {
        let mut posts = Posts::default();
        for publication in publications {
            let Some(upload) = publication.upload() else {
                posts.posted |= publication.is_posted();
                continue;
            };
            match upload.status {
                UploadStatus::Queued if publication.due().is_some() => {
                    let at = publication.due();
                    posts.waiting = Some(earliest(posts.waiting.flatten(), at));
                }
                UploadStatus::Queued | UploadStatus::Uploading | UploadStatus::Processing => {
                    posts.sending = true;
                }
                UploadStatus::Scheduled => {
                    posts.waiting = Some(earliest(posts.waiting.flatten(), upload.publish_at));
                }
                _ => posts.posted |= publication.is_posted(),
            }
        }
        posts
    }
}

fn earliest(a: Option<SystemTime>, b: Option<SystemTime>) -> Option<SystemTime> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// The step that moves a video on: its posts first (sending, waiting to go
/// out, or posted and done), then the first stage, in order, that waits on
/// the user; a stage at work holds the video until it ends. The editor is a
/// step only while the render waits for a cut, and images or clips left to
/// make stop being one once the render has run. With every stage done and
/// nothing posted, the step is to post it.
pub fn next_step(stages: &[StageStatus], posts: Posts, estimates: &StepEstimates) -> NextStep {
    if posts.sending {
        return NextStep::Working(Stage::Publish);
    }
    if let Some(at) = posts.waiting {
        return NextStep::WaitingOnPost { at };
    }
    if posts.posted {
        return NextStep::Finished;
    }
    let edit_open = stages
        .iter()
        .any(|status| status.stage == Stage::Edit && status.state != StageState::Locked);
    let moved_past_gaps = stages.iter().any(|status| {
        status.stage == Stage::Render
            && !matches!(status.state, StageState::Locked | StageState::Open)
    });
    for status in stages {
        let action = |step: StepKind| {
            let estimate = match step {
                StepKind::WriteScript | StepKind::RewriteScript => estimates.script.clone(),
                StepKind::RecordNarration | StepKind::RecordAgain => estimates.narration.clone(),
                StepKind::PlanScenes | StepKind::PlanAgain => estimates.plan.clone(),
                StepKind::DrawImages(_) => estimates.images.clone(),
                StepKind::AnimateClips(_) => estimates.clips.clone(),
                _ => None,
            };
            NextStep::Do(NextAction {
                stage: status.stage,
                step,
                estimate,
            })
        };
        match status.state {
            StageState::Working => return NextStep::Working(status.stage),
            StageState::Done => continue,
            StageState::Locked => {
                if status.stage == Stage::Render
                    && status.note == StageNote::After(Stage::Edit)
                    && edit_open
                {
                    return NextStep::Do(NextAction {
                        stage: Stage::Edit,
                        step: StepKind::BuildCut,
                        estimate: None,
                    });
                }
                continue;
            }
            StageState::Attention | StageState::Open | StageState::Partial => {}
        }
        if status.stage == Stage::Edit {
            continue;
        }
        // Images or clips left to make once a cut went to the render (built
        // with stills in their place) are optional: the video moved on.
        if moved_past_gaps
            && matches!(
                status.note,
                StageNote::Images { .. } | StageNote::Clips { .. }
            )
        {
            continue;
        }
        let step = match status.note {
            StageNote::BriefToConfirm => StepKind::ConfirmBrief,
            StageNote::BriefClaimsChanged(n) => StepKind::DecideClaims(n),
            StageNote::BriefIncomplete => StepKind::CompleteBrief,
            StageNote::ScriptMissing => StepKind::WriteScript,
            StageNote::ScriptToReview => StepKind::ReviewScript,
            StageNote::ScriptBriefChanged => StepKind::RewriteScript,
            StageNote::NarrationMissing => StepKind::RecordNarration,
            StageNote::NarrationStale => StepKind::RecordAgain,
            StageNote::TakesFlagged(n) => StepKind::FixTakes(n),
            StageNote::ScenesMissing => StepKind::PlanScenes,
            StageNote::ScenesToPlace => StepKind::PlaceScenes,
            StageNote::ScenesStale => StepKind::PlanAgain,
            StageNote::ImagesToReview(n) => StepKind::ReviewImages(n),
            StageNote::Images { done, total } => StepKind::DrawImages(total.saturating_sub(done)),
            StageNote::ClipsToReview(n) => StepKind::ReviewClips(n),
            StageNote::Clips { done, total } => StepKind::AnimateClips(total.saturating_sub(done)),
            StageNote::RenderReady => StepKind::Render,
            StageNote::RenderOutdated(_) => StepKind::RenderAgain,
            StageNote::RenderStopped => StepKind::ResumeRender,
            StageNote::ExportReady => StepKind::Export,
            StageNote::ExportOutdated(_) => StepKind::ExportAgain,
            StageNote::Exported(_) => StepKind::ExportRest,
            StageNote::ExportStopped => StepKind::ResumeExport,
            // Notes of a stage at work, done or locked: never reached here.
            _ => continue,
        };
        return action(step);
    }
    NextStep::Do(NextAction {
        stage: Stage::Publish,
        step: StepKind::Post,
        estimate: None,
    })
}

/// A video of the open project that is not posted yet.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInProgress {
    pub id: VideoProjectId,
    pub title: String,
    pub channel: ChannelId,
    pub channel_name: String,
    pub format: VideoFormat,
    pub is_ad: bool,
    /// Where the stage of its next step stands ("Clips, 7 of 14").
    pub status: Option<StageStatus>,
    pub next: NextStep,
    pub created_at: SystemTime,
}

/// The ideas of the open project that wait for the user.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdeasWaiting {
    /// Suggested themes no ranking has scored yet.
    pub unranked: usize,
    /// Suggested themes ranked and waiting to be approved or discarded.
    pub ranked: usize,
    /// Approved themes whose video is gone.
    pub approved_without_video: usize,
}

impl IdeasWaiting {
    pub fn is_empty(&self) -> bool {
        self.unranked == 0 && self.ranked == 0 && self.approved_without_video == 0
    }
}

/// Everything Overview shows of the open project.
#[derive(Debug, Clone, PartialEq)]
pub struct OverviewView {
    /// Newest first.
    pub videos: Vec<VideoInProgress>,
    pub ideas: IdeasWaiting,
    pub channels: Vec<Channel>,
    /// What the project's work spent this month.
    pub spent: Money,
    /// The narrators the project's channels speak with by default, each
    /// once, by name.
    pub narrators: Vec<String>,
}

impl Bardo {
    /// The open project's overview: empty without a project.
    pub fn overview(&self) -> Result<OverviewView, AppError> {
        let channels = self.channels()?;
        let mut videos = Vec::new();
        let mut ideas = IdeasWaiting::default();
        // An archived video is put away: it is not in progress (#336).
        let archived = self.archived_videos();
        for channel in &channels {
            let projects = self.themes.projects(channel.id)?;
            let started: HashSet<_> = projects.iter().filter_map(|video| video.theme).collect();
            for theme in self.themes.themes(channel.id)? {
                match theme.status() {
                    ThemeStatus::Suggested if theme.ranking().is_some() => ideas.ranked += 1,
                    ThemeStatus::Suggested => ideas.unranked += 1,
                    ThemeStatus::Approved if !started.contains(&theme.id) => {
                        ideas.approved_without_video += 1;
                    }
                    _ => {}
                }
            }
            for video in projects {
                if archived.contains(&video.id) {
                    continue;
                }
                // A video already posted is left out before its stages are
                // read: those are the costly part.
                let posts = Posts::of(&self.publications.publications(video.id)?);
                if posts.posted && !posts.sending && posts.waiting.is_none() {
                    continue;
                }
                let Some((stages, estimates)) = self.stages_of(video.id) else {
                    continue;
                };
                let next = next_step(&stages, posts, &estimates);
                if !next.in_progress() {
                    continue;
                }
                let status = next
                    .stage()
                    .and_then(|stage| stages.iter().find(|status| status.stage == stage))
                    .copied();
                videos.push(VideoInProgress {
                    id: video.id,
                    title: video.title.clone(),
                    channel: channel.id,
                    channel_name: channel.details.name().to_owned(),
                    // A video moved in from the loose ones keeps its own
                    // settings (#341).
                    format: video.format(
                        crate::loose::details_of(&*self.themes, &*self.channels, &video)?
                            .map_or(channel.details.format(), |details| details.format()),
                    ),
                    is_ad: video.is_ad(),
                    status,
                    next,
                    created_at: video.created_at,
                });
            }
        }
        videos.sort_by_key(|video| std::cmp::Reverse(video.created_at));
        let spent = match self.open_project_id()? {
            Some(project) => self
                .costs(self.current_month())
                .ok()
                .and_then(|view| {
                    view.projects
                        .iter()
                        .find(|row| row.project == Some(project))
                        .map(|row| row.amount)
                })
                .unwrap_or_default(),
            None => Money::default(),
        };
        let personas = self.personas().unwrap_or_default();
        let mut narrators = Vec::new();
        for channel in &channels {
            let name = channel
                .details
                .default_persona()
                .and_then(|id| personas.iter().find(|persona| persona.id == id))
                .map(|persona| persona.details.name().to_owned());
            if let Some(name) = name
                && !narrators.contains(&name)
            {
                narrators.push(name);
            }
        }
        Ok(OverviewView {
            videos,
            ideas,
            channels,
            spent,
            narrators,
        })
    }

    /// The step as Overview words it: "Next step: animate the scenes
    /// without a clip (7), about $1.40", or why there is nothing to do.
    pub fn next_step_text(&self, next: &NextStep) -> String {
        match next {
            NextStep::Do(action) => {
                let step = self.step_text(action.step);
                match &action.estimate {
                    Some(estimate) if estimate.total() > Money::default() => self.text_with(
                        Text::OverviewNextWithCost,
                        &[("step", &step), ("amount", &self.money(estimate.total()))],
                    ),
                    _ => self.text_with(Text::OverviewNextLabel, &[("step", &step)]),
                }
            }
            NextStep::Working(stage) => self.text_with(
                Text::OverviewWorking,
                &[("stage", &self.text(Text::StageName(*stage)))],
            ),
            NextStep::WaitingOnPost { at: Some(at) } => self.text_with(
                Text::OverviewWaitingAt,
                &[(
                    "when",
                    &self
                        .catalog
                        .date_time_here(&bardo_domain::local_time(*at, &self.zone)),
                )],
            ),
            NextStep::WaitingOnPost { at: None } | NextStep::Finished => {
                self.text(Text::OverviewWaiting).into_owned()
            }
        }
    }

    fn step_text(&self, step: StepKind) -> String {
        let n = |text: Text, n: usize| self.text_with(text, &[("n", &n.to_string())]);
        let plain = |text: Text| self.text(text).into_owned();
        match step {
            StepKind::ConfirmBrief => plain(Text::StepConfirmBrief),
            StepKind::DecideClaims(count) => n(Text::StepDecideClaims, count),
            StepKind::CompleteBrief => plain(Text::StepCompleteBrief),
            StepKind::WriteScript => plain(Text::StepWriteScript),
            StepKind::ReviewScript => plain(Text::StepReviewScript),
            StepKind::RewriteScript => plain(Text::StepRewriteScript),
            StepKind::RecordNarration => plain(Text::StepRecordNarration),
            StepKind::RecordAgain => plain(Text::StepRecordAgain),
            StepKind::FixTakes(count) => n(Text::StepFixTakes, count),
            StepKind::PlanScenes => plain(Text::StepPlanScenes),
            StepKind::PlaceScenes => plain(Text::StepPlaceScenes),
            StepKind::PlanAgain => plain(Text::StepPlanAgain),
            StepKind::ReviewImages(count) => n(Text::StepReviewImages, count),
            StepKind::DrawImages(count) => n(Text::StepDrawImages, count),
            StepKind::ReviewClips(count) => n(Text::StepReviewClips, count),
            StepKind::AnimateClips(count) => n(Text::StepAnimateClips, count),
            StepKind::BuildCut => plain(Text::StepBuildCut),
            StepKind::Render => plain(Text::StepRender),
            StepKind::RenderAgain => plain(Text::StepRenderAgain),
            StepKind::ResumeRender => plain(Text::StepResumeRender),
            StepKind::Export => plain(Text::StepExport),
            StepKind::ExportAgain => plain(Text::StepExportAgain),
            StepKind::ExportRest => plain(Text::StepExportRest),
            StepKind::ResumeExport => plain(Text::StepResumeExport),
            StepKind::Post => plain(Text::StepPost),
        }
    }

    /// A video's stages and what generating its next steps would cost;
    /// `None` when a view does not load.
    fn stages_of(&self, video: VideoProjectId) -> Option<(Vec<StageStatus>, StepEstimates)> {
        let script = self.script(video).ok()?;
        let narration = self.narration(video).ok()?;
        let scenes = self.scenes(video).ok()?;
        let renders = self.render_summary(video).ok()?;
        let exports = self.export_summary(video).ok()?;
        let stages = project_stages(&script, &narration, &scenes, &renders, &exports);
        let estimates = StepEstimates {
            script: Some(script.estimate.clone()),
            narration: narration.estimate.clone(),
            plan: scenes.plan_estimate.clone(),
            images: scenes.images_estimate.clone(),
            clips: scenes.clips.missing_estimate.clone(),
        };
        Some((stages, estimates))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(stage: Stage, state: StageState, note: StageNote) -> StageStatus {
        StageStatus { stage, state, note }
    }

    fn done(stage: Stage) -> StageStatus {
        status(stage, StageState::Done, StageNote::Working)
    }

    fn locked(stage: Stage, after: Stage) -> StageStatus {
        status(stage, StageState::Locked, StageNote::After(after))
    }

    /// A video at its very start: a script to write, the rest locked.
    fn fresh() -> Vec<StageStatus> {
        vec![
            status(Stage::Script, StageState::Open, StageNote::ScriptMissing),
            locked(Stage::Narration, Stage::Script),
            locked(Stage::Scenes, Stage::Narration),
            locked(Stage::Clips, Stage::Scenes),
            locked(Stage::Edit, Stage::Scenes),
            locked(Stage::Render, Stage::Edit),
            locked(Stage::Publish, Stage::Render),
        ]
    }

    /// Every stage done, with `last` in place of the stage it names.
    fn all_done_but(last: StageStatus) -> Vec<StageStatus> {
        [
            Stage::Script,
            Stage::Narration,
            Stage::Scenes,
            Stage::Clips,
            Stage::Edit,
            Stage::Render,
            Stage::Publish,
        ]
        .into_iter()
        .map(|stage| {
            if stage == last.stage {
                last
            } else if stage == Stage::Edit {
                status(Stage::Edit, StageState::Open, StageNote::EditorReady)
            } else {
                done(stage)
            }
        })
        .collect()
    }

    /// `stages` with the cut built but not rendered yet.
    fn unrendered(mut stages: Vec<StageStatus>) -> Vec<StageStatus> {
        for status in &mut stages {
            if status.stage == Stage::Render {
                *status = StageStatus {
                    stage: Stage::Render,
                    state: StageState::Open,
                    note: StageNote::RenderReady,
                };
            }
        }
        stages
    }

    fn step(stages: &[StageStatus]) -> NextStep {
        next_step(stages, Posts::default(), &StepEstimates::default())
    }

    fn doing(stage: Stage, step: StepKind) -> NextStep {
        NextStep::Do(NextAction {
            stage,
            step,
            estimate: None,
        })
    }

    fn priced(cents: u64) -> SpendEstimate {
        SpendEstimate {
            providers: vec![crate::ProviderEstimate {
                provider: bardo_domain::Provider::Claude,
                amount: Some(Money::from_cents(cents)),
                spent: Money::default(),
                budget: None,
                level: None,
                paid_by: bardo_domain::KeyPayer::App,
            }],
            ..SpendEstimate::default()
        }
    }

    #[test]
    fn a_new_video_writes_its_script_with_the_scripts_estimate() {
        let estimates = StepEstimates {
            script: Some(priced(3)),
            ..StepEstimates::default()
        };
        assert_eq!(
            next_step(&fresh(), Posts::default(), &estimates),
            NextStep::Do(NextAction {
                stage: Stage::Script,
                step: StepKind::WriteScript,
                estimate: Some(priced(3)),
            })
        );
    }

    #[test]
    fn an_ad_starts_by_confirming_its_brief() {
        let mut stages = fresh();
        stages[0] = locked(Stage::Script, Stage::Brief);
        stages.insert(
            0,
            status(Stage::Brief, StageState::Open, StageNote::BriefToConfirm),
        );
        assert_eq!(step(&stages), doing(Stage::Brief, StepKind::ConfirmBrief));
        stages[0] = status(
            Stage::Brief,
            StageState::Attention,
            StageNote::BriefClaimsChanged(2),
        );
        assert_eq!(
            step(&stages),
            doing(Stage::Brief, StepKind::DecideClaims(2))
        );
    }

    #[test]
    fn a_script_to_review_comes_before_anything_later() {
        let mut stages = fresh();
        stages[0] = status(
            Stage::Script,
            StageState::Attention,
            StageNote::ScriptToReview,
        );
        stages[1] = status(
            Stage::Narration,
            StageState::Open,
            StageNote::NarrationMissing,
        );
        assert_eq!(step(&stages), doing(Stage::Script, StepKind::ReviewScript));
    }

    #[test]
    fn the_narration_is_recorded_then_recorded_again_when_stale() {
        let estimates = StepEstimates {
            narration: Some(priced(40)),
            ..StepEstimates::default()
        };
        let mut stages = fresh();
        stages[0] = done(Stage::Script);
        stages[1] = status(
            Stage::Narration,
            StageState::Open,
            StageNote::NarrationMissing,
        );
        assert_eq!(
            next_step(&stages, Posts::default(), &estimates),
            NextStep::Do(NextAction {
                stage: Stage::Narration,
                step: StepKind::RecordNarration,
                estimate: Some(priced(40)),
            })
        );
        stages[1] = status(
            Stage::Narration,
            StageState::Attention,
            StageNote::NarrationStale,
        );
        assert_eq!(
            step(&stages),
            doing(Stage::Narration, StepKind::RecordAgain)
        );
    }

    #[test]
    fn a_capture_with_takes_to_fix_asks_for_them() {
        let mut stages = all_done_but(done(Stage::Script));
        stages.insert(
            2,
            status(
                Stage::Capture,
                StageState::Attention,
                StageNote::TakesFlagged(3),
            ),
        );
        assert_eq!(step(&stages), doing(Stage::Capture, StepKind::FixTakes(3)));
    }

    #[test]
    fn scenes_are_planned_placed_reviewed_and_drawn() {
        let estimates = StepEstimates {
            plan: Some(priced(5)),
            images: Some(priced(80)),
            ..StepEstimates::default()
        };
        let at = |state, note| {
            next_step(
                &unrendered(all_done_but(status(Stage::Scenes, state, note))),
                Posts::default(),
                &estimates,
            )
        };
        assert_eq!(
            at(StageState::Open, StageNote::ScenesMissing),
            NextStep::Do(NextAction {
                stage: Stage::Scenes,
                step: StepKind::PlanScenes,
                estimate: Some(priced(5)),
            })
        );
        assert_eq!(
            at(StageState::Attention, StageNote::ScenesToPlace),
            doing(Stage::Scenes, StepKind::PlaceScenes)
        );
        assert_eq!(
            at(StageState::Attention, StageNote::ImagesToReview(2)),
            doing(Stage::Scenes, StepKind::ReviewImages(2))
        );
        assert_eq!(
            at(StageState::Partial, StageNote::Images { done: 4, total: 9 }),
            NextStep::Do(NextAction {
                stage: Stage::Scenes,
                step: StepKind::DrawImages(5),
                estimate: Some(priced(80)),
            })
        );
    }

    #[test]
    fn gaps_left_in_images_or_clips_wait_no_longer_once_the_video_is_rendered() {
        // Built with stills where clips are missing, then rendered: the
        // video is ready to post, not waiting on paid gap filling.
        let mut stages = all_done_but(status(
            Stage::Clips,
            StageState::Partial,
            StageNote::Clips { done: 5, total: 8 },
        ));
        stages[2] = status(
            Stage::Scenes,
            StageState::Partial,
            StageNote::Images { done: 6, total: 8 },
        );
        assert_eq!(step(&stages), doing(Stage::Publish, StepKind::Post));

        // Before any render, the gaps are still the next thing to do.
        let stages = unrendered(stages);
        assert_eq!(step(&stages), doing(Stage::Scenes, StepKind::DrawImages(2)));

        // Clips to review still ask for a decision after a render.
        let mut stages = all_done_but(status(
            Stage::Clips,
            StageState::Attention,
            StageNote::ClipsToReview(2),
        ));
        stages[2] = done(Stage::Scenes);
        assert_eq!(step(&stages), doing(Stage::Clips, StepKind::ReviewClips(2)));
    }

    #[test]
    fn clips_left_to_animate_carry_their_estimate() {
        let estimates = StepEstimates {
            clips: Some(priced(140)),
            ..StepEstimates::default()
        };
        let stages = unrendered(all_done_but(status(
            Stage::Clips,
            StageState::Partial,
            StageNote::Clips { done: 7, total: 14 },
        )));
        assert_eq!(
            next_step(&stages, Posts::default(), &estimates),
            NextStep::Do(NextAction {
                stage: Stage::Clips,
                step: StepKind::AnimateClips(7),
                estimate: Some(priced(140)),
            })
        );
        let review = all_done_but(status(
            Stage::Clips,
            StageState::Attention,
            StageNote::ClipsToReview(1),
        ));
        assert_eq!(step(&review), doing(Stage::Clips, StepKind::ReviewClips(1)));
    }

    #[test]
    fn without_a_cut_the_editor_is_the_step() {
        let stages = all_done_but(locked(Stage::Render, Stage::Edit));
        assert_eq!(step(&stages), doing(Stage::Edit, StepKind::BuildCut));
    }

    #[test]
    fn a_cut_is_rendered_again_when_it_changed_or_resumed_when_it_stopped() {
        let at = |state, note| step(&all_done_but(status(Stage::Render, state, note)));
        assert_eq!(
            at(StageState::Open, StageNote::RenderReady),
            doing(Stage::Render, StepKind::Render)
        );
        assert_eq!(
            at(StageState::Attention, StageNote::RenderOutdated(1)),
            doing(Stage::Render, StepKind::RenderAgain)
        );
        assert_eq!(
            at(StageState::Attention, StageNote::RenderStopped),
            doing(Stage::Render, StepKind::ResumeRender)
        );
        assert_eq!(
            at(StageState::Working, StageNote::Rendering),
            NextStep::Working(Stage::Render)
        );
    }

    #[test]
    fn a_render_is_exported_then_posted() {
        let at = |state, note| step(&all_done_but(status(Stage::Publish, state, note)));
        assert_eq!(
            at(StageState::Open, StageNote::ExportReady),
            doing(Stage::Publish, StepKind::Export)
        );
        assert_eq!(
            at(StageState::Partial, StageNote::Exported(1)),
            doing(Stage::Publish, StepKind::ExportRest)
        );
        assert_eq!(
            at(StageState::Attention, StageNote::ExportOutdated(2)),
            doing(Stage::Publish, StepKind::ExportAgain)
        );
        assert_eq!(
            at(StageState::Attention, StageNote::ExportStopped),
            doing(Stage::Publish, StepKind::ResumeExport)
        );
        assert_eq!(
            at(StageState::Done, StageNote::Exported(2)),
            doing(Stage::Publish, StepKind::Post)
        );
    }

    #[test]
    fn a_stage_at_work_holds_the_video_until_it_ends() {
        let mut stages = fresh();
        stages[0] = status(Stage::Script, StageState::Working, StageNote::Working);
        assert_eq!(step(&stages), NextStep::Working(Stage::Script));
    }

    #[test]
    fn a_video_waiting_on_a_publication_has_nothing_to_do() {
        let at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000);
        let stages = all_done_but(done(Stage::Publish));
        let waiting = Posts {
            waiting: Some(Some(at)),
            ..Posts::default()
        };
        assert_eq!(
            next_step(&stages, waiting, &StepEstimates::default()),
            NextStep::WaitingOnPost { at: Some(at) }
        );
        let sending = Posts {
            sending: true,
            ..Posts::default()
        };
        assert_eq!(
            next_step(&stages, sending, &StepEstimates::default()),
            NextStep::Working(Stage::Publish)
        );
        let posted = Posts {
            posted: true,
            ..Posts::default()
        };
        assert_eq!(
            next_step(&stages, posted, &StepEstimates::default()),
            NextStep::Finished
        );
        assert!(!NextStep::Finished.in_progress());
    }
    mod with_a_project {
        use std::sync::Arc;

        use bardo_domain::{ChannelDraft, Niche, Theme, ThemeIdea, ThemeRepository};
        use bardo_storage::Database;

        use super::*;
        use crate::{Repositories, testing};

        fn app() -> (Bardo, Arc<Database>) {
            let db = Arc::new(Database::open_in_memory().unwrap());
            let repositories = Repositories::shared(
                Arc::clone(&db),
                Arc::new(bardo_storage::MemorySecretStore::default()),
            );
            let app = Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap();
            (app, db)
        }

        fn theme(app: &Bardo, channel: ChannelId, title: &str) -> Theme {
            let theme = Theme::suggested(
                app.profile().id,
                channel,
                Niche::new("space history").unwrap(),
                ThemeIdea::new(title, "The last signal.").unwrap(),
                SystemTime::now(),
                0,
                None,
            );
            app.themes
                .save_themes(std::slice::from_ref(&theme))
                .unwrap();
            theme
        }

        #[test]
        fn a_new_project_shows_nothing_and_a_started_video_its_first_step() {
            let (app, db) = app();
            let empty = app.overview().unwrap();
            assert!(empty.videos.is_empty() && empty.channels.is_empty());

            let space = app
                .create_channel(ChannelDraft {
                    name: "Space Archives".into(),
                    ..ChannelDraft::default()
                })
                .unwrap();
            let mut approved = theme(&app, space.id, "Apollo");
            theme(&app, space.id, "Voyager");
            let video = approved.approve(SystemTime::now()).unwrap();
            app.themes.start_project(&approved, &video).unwrap();

            use bardo_domain::CostRepository;
            db.record_cost(&bardo_domain::CostRecord {
                id: bardo_domain::CostRecordId::new(),
                owner: app.profile().id,
                provider: bardo_domain::Provider::Claude,
                model: "claude".into(),
                purpose: bardo_domain::CostPurpose::Script,
                usage: bardo_domain::Metered::default(),
                cost: bardo_domain::Cost::Estimated(Money::from_cents(250)),
                channel: Some(space.id),
                project: Some(video.id),
                job: None,
                at: SystemTime::now(),
                origin: bardo_domain::WorkOrigin::default(),
                paid_by: bardo_domain::KeyPayer::App,
            })
            .unwrap();

            let view = app.overview().unwrap();
            assert_eq!(view.channels.len(), 1);
            assert_eq!(view.spent, Money::from_cents(250));
            assert_eq!(
                view.ideas,
                IdeasWaiting {
                    unranked: 1,
                    ..IdeasWaiting::default()
                }
            );
            let [shown] = view.videos.as_slice() else {
                panic!("one video in progress: {:?}", view.videos);
            };
            assert_eq!(shown.title, "Apollo");
            assert_eq!(shown.channel_name, "Space Archives");
            let NextStep::Do(action) = &shown.next else {
                panic!("a step to do: {:?}", shown.next);
            };
            assert_eq!(
                (action.stage, action.step),
                (Stage::Script, StepKind::WriteScript)
            );
            assert_eq!(shown.status.map(|status| status.stage), Some(Stage::Script));
            assert!(
                app.next_step_text(&shown.next)
                    .starts_with("Next step: write the script"),
                "{}",
                app.next_step_text(&shown.next)
            );

            // Another project shows none of it.
            app.create_project(bardo_domain::ProjectDraft {
                name: "Cooking".into(),
                ..Default::default()
            })
            .unwrap();
            let other = app.overview().unwrap();
            assert!(other.videos.is_empty() && other.ideas.is_empty());
            assert_eq!(other.spent, Money::default());
        }
    }
}
