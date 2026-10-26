//! Production mode use cases (issue #248): how a video is produced, and
//! what it will about cost in each mode before anything is generated. A
//! project follows its channel's mode unless it picks its own, as with the
//! format. The estimate adds up what is left to make (script, narration,
//! scene plan, images and portraits, the clips the mode animates) from the
//! rate table, the way each step's own estimate does, on top of what the
//! video already cost.
//!
//! A video may have a spend cap: a paid job of the project that would take
//! its spend past the cap asks first, with what the rest of the video would
//! cost in its mode.

use std::time::Duration;

use bardo_domain::{
    ClipModel, CostPurpose, Money, ProductionMode, Provider, RepositoryError, VideoProject,
    VideoProjectId, typical_length, typical_scenes,
};

use crate::costs::{BudgetConsent, CapReached, PlannedCall, SpendEstimate};
use crate::{Bardo, KeyState, ScriptError};

/// About how many characters a second of narration reads, before the
/// narration exists.
const CHARS_PER_SECOND: u64 = bardo_domain::NARRATION_CHARS_PER_SECOND;

/// How many people a plan not made yet is counted with: one portrait.
const TYPICAL_CAST: usize = 1;

/// What the rest of a video would cost in one mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeEstimate {
    pub mode: ProductionMode,
    /// What is left to make, priced.
    pub remaining: Money,
    /// Whether some call has no rate, so `remaining` is short.
    pub partial: bool,
    /// How many clips are left to make.
    pub clips: usize,
    /// A provider the mode's clips call that has no key saved.
    pub missing_key: Option<Provider>,
}

/// A project's production mode, its estimate in every mode and its cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionView {
    /// The mode the project is made in: its own, else its channel's.
    pub mode: ProductionMode,
    /// The project's own mode; `None` follows the channel.
    pub own: Option<ProductionMode>,
    pub channel_mode: ProductionMode,
    /// What the video cost so far.
    pub spent: Money,
    pub cap: Option<Money>,
    /// One per mode, in [`ProductionMode::ALL`] order.
    pub estimates: Vec<ModeEstimate>,
}

impl ProductionView {
    /// The estimate in `mode`.
    pub fn estimate(&self, mode: ProductionMode) -> Option<&ModeEstimate> {
        self.estimates.iter().find(|estimate| estimate.mode == mode)
    }

    /// What the whole video would cost in `mode`: spent so far plus what
    /// is left.
    pub fn total(&self, mode: ProductionMode) -> Option<Money> {
        self.estimate(mode)
            .map(|estimate| self.spent.saturating_add(estimate.remaining))
    }

    /// Whether the whole video, in its mode, would cost more than its cap.
    pub fn over_cap(&self) -> bool {
        self.cap
            .zip(self.total(self.mode))
            .is_some_and(|(cap, total)| total > cap)
    }
}

/// One scene as the estimate sees it: when it starts, how long it runs,
/// whether it still needs an image or a clip, and its own clip model.
struct PlannedScene {
    start: Duration,
    length: Duration,
    needs_image: bool,
    needs_clip: bool,
    own_model: Option<bardo_domain::ClipModelRef>,
}

/// What is left to make before the clips, the same in every mode.
struct VideoWork {
    calls: Vec<PlannedCall>,
    scenes: Vec<PlannedScene>,
}

impl Bardo {
    /// How `project` is produced: its own mode, else its channel's (the
    /// default mode when the channel is gone).
    pub fn production_mode(
        &self,
        project: &VideoProject,
    ) -> Result<ProductionMode, RepositoryError> {
        let channel = self.channels.get(project.channel)?;
        Ok(
            project.production(channel.map_or(ProductionMode::default(), |channel| {
                channel.details.production()
            })),
        )
    }

    /// Sets the project's own mode, or goes back to the channel's
    /// (`None`). What was made stays; the next clips follow the new mode.
    pub fn set_project_production(
        &self,
        project: VideoProjectId,
        mode: Option<ProductionMode>,
    ) -> Result<VideoProject, ScriptError> {
        let mut project = self.production_project(project)?;
        self.themes.set_project_production(project.id, mode)?;
        project.production = mode;
        Ok(project)
    }

    /// Sets what the video may cost before Bardo asks, from the amount the
    /// user typed; nothing typed takes the cap away.
    pub fn set_spend_cap(
        &self,
        project: VideoProjectId,
        typed: &str,
    ) -> Result<VideoProject, ScriptError> {
        let cap = match typed.trim() {
            "" => None,
            typed => Some(Money::parse(typed).map_err(|_| ScriptError::CapInvalid)?),
        };
        if cap.is_some_and(Money::is_zero) {
            return Err(ScriptError::CapInvalid);
        }
        let mut project = self.production_project(project)?;
        self.themes.set_project_spend_cap(project.id, cap)?;
        project.spend_cap = cap;
        Ok(project)
    }

    fn production_project(&self, id: VideoProjectId) -> Result<VideoProject, ScriptError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ScriptError::ProjectNotFound)
    }

    /// The project's mode, its estimate in every mode and its cap.
    pub fn production(&self, project: VideoProjectId) -> Result<ProductionView, ScriptError> {
        let project = self.production_project(project)?;
        let channel_mode = self
            .channels
            .get(project.channel)?
            .map_or(ProductionMode::default(), |channel| {
                channel.details.production()
            });
        let work = self.video_work(&project)?;
        let models = self.clip_models();
        let channel_model = self.channel_clip_model(&models, project.channel)?;
        let mut estimates = Vec::with_capacity(ProductionMode::ALL.len());
        for mode in ProductionMode::ALL {
            estimates.push(self.mode_estimate(&work, mode, &models, channel_model.as_ref())?);
        }
        Ok(ProductionView {
            mode: project.production(channel_mode),
            own: project.production,
            channel_mode,
            spent: self.project_spend(project.id)?,
            cap: project.spend_cap,
            estimates,
        })
    }

    /// What the rest of `project` would cost in its mode.
    fn remaining_in_mode(&self, project: &VideoProject) -> Result<Money, RepositoryError> {
        let work = self.video_work(project)?;
        let models = self.clip_models();
        let channel_model = self.channel_clip_model(&models, project.channel)?;
        let mode = self.production_mode(project)?;
        Ok(self
            .mode_estimate(&work, mode, &models, channel_model.as_ref())?
            .remaining)
    }

    /// What is left to make of `project` before its clips.
    fn video_work(&self, project: &VideoProject) -> Result<VideoWork, RepositoryError> {
        let format = self.video_format(project)?;
        let script = self.scripts.script(project.id)?;
        let narration = self.narrations.narration(project.id)?;
        let plan = self.scene_plans.scene_plan(project.id)?;
        let mut calls = Vec::new();
        if script.is_none() {
            calls.push(PlannedCall::new(Provider::Claude, CostPurpose::Script, 1));
        }
        let length = match (&narration, &script) {
            (Some(narration), _) => narration.duration,
            (None, Some(script)) => Duration::from_secs(
                (script.text().as_str().chars().count() as u64).div_ceil(CHARS_PER_SECOND),
            ),
            (None, None) => typical_length(format),
        };
        if narration.is_none() {
            let characters = match &script {
                Some(script) => script.text().as_str().chars().count(),
                None => (length.as_secs() * CHARS_PER_SECOND) as usize,
            };
            calls.push(
                PlannedCall::new(Provider::ElevenLabs, CostPurpose::Narration, 1)
                    .with_characters(characters),
            );
        }
        let (scenes, portraits) = match &plan {
            Some(plan) => {
                let scenes: Vec<PlannedScene> = plan
                    .scenes()
                    .iter()
                    .map(|scene| PlannedScene {
                        start: scene.start,
                        length: scene.duration(),
                        needs_image: scene.image().is_none(),
                        // A clip waiting for review is already paid for.
                        needs_clip: scene.clip().is_none() && scene.pending_clip().is_none(),
                        own_model: scene.clip_model().cloned(),
                    })
                    .collect();
                let missing = plan.missing_images();
                (scenes, plan.missing_portraits(&missing).len())
            }
            None => {
                calls.push(PlannedCall::new(
                    Provider::Claude,
                    CostPurpose::ScenePlan,
                    1,
                ));
                let scenes = typical_scenes(length)
                    .into_iter()
                    .map(|(start, length)| PlannedScene {
                        start,
                        length,
                        needs_image: true,
                        needs_clip: true,
                        own_model: None,
                    })
                    .collect();
                (scenes, TYPICAL_CAST)
            }
        };
        let images = scenes.iter().filter(|scene| scene.needs_image).count() + portraits;
        if images > 0 {
            calls.push(PlannedCall::new(
                Provider::Gemini,
                CostPurpose::SceneImage,
                images as u64,
            ));
        }
        Ok(VideoWork { calls, scenes })
    }

    /// `work` plus the clips `mode` makes, priced.
    fn mode_estimate(
        &self,
        work: &VideoWork,
        mode: ProductionMode,
        models: &[ClipModel],
        channel_model: Option<&ClipModel>,
    ) -> Result<ModeEstimate, RepositoryError> {
        let default = Self::mode_clip_model(mode, models, channel_model);
        let mut calls = work.calls.clone();
        let mut clips = 0;
        let mut missing_key = None;
        for scene in work
            .scenes
            .iter()
            .filter(|scene| scene.needs_clip && mode.animates(scene.start))
        {
            let model = match &scene.own_model {
                Some(own) => models.iter().find(|model| model.id == *own),
                None => default,
            };
            let Some(model) = model else { continue };
            clips += 1;
            let provider = model.id.provider();
            if missing_key.is_none() && self.provider_key(provider).state == KeyState::NotSet {
                missing_key = Some(provider);
            }
            calls.push(
                PlannedCall::new(provider, CostPurpose::SceneClip, 1)
                    .with_model(model.id.model())
                    .with_video_seconds(model.durations.fit(scene.length)),
            );
        }
        let (remaining, partial) = self.cost_book.price(&calls)?;
        Ok(ModeEstimate {
            mode,
            remaining,
            partial,
            clips,
            missing_key,
        })
    }

    /// The model of `mode`'s clips for scenes without their own: the
    /// mode's, else the channel's.
    pub(crate) fn mode_clip_model<'a>(
        mode: ProductionMode,
        models: &'a [ClipModel],
        channel_model: Option<&'a ClipModel>,
    ) -> Option<&'a ClipModel> {
        match mode.clip_model() {
            Some(wanted) => models
                .iter()
                .find(|model| model.id == wanted)
                .or(channel_model),
            None => channel_model,
        }
    }

    /// `check_budget` for a job of `project`, with the video's cap: a job
    /// that would take the video's spend past it asks too, with what the
    /// rest of the video would cost.
    pub(crate) fn check_video_budget(
        &self,
        project: VideoProjectId,
        calls: &[PlannedCall],
        consent: BudgetConsent,
    ) -> Result<Result<(), SpendEstimate>, RepositoryError> {
        if consent == BudgetConsent::Confirmed {
            return Ok(Ok(()));
        }
        let mut estimate = self.estimate(calls)?;
        if let Some(project) = self.themes.project(project)?
            && let Some(cap) = project.spend_cap
        {
            let spent = self.project_spend(project.id)?;
            // A call with no rate could pass the cap unseen, so it asks too.
            // Jobs still running count once they record what they spent.
            if spent.saturating_add(estimate.total()) > cap || estimate.is_partial() {
                estimate.cap = Some(CapReached {
                    cap,
                    spent,
                    rest: self.remaining_in_mode(&project)?,
                });
            }
        }
        Ok(match (estimate.level(), &estimate.cap) {
            (bardo_domain::BudgetLevel::Reached, _) | (_, Some(_)) => Err(estimate),
            _ => Ok(()),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bardo_domain::{ChannelDraft, VideoFormat};

    use super::*;
    use crate::SceneError;
    use crate::scenes::tests::{Harness, done, project};

    /// A harness whose video providers include Kling, as the app's do.
    fn harness() -> Harness {
        let mut h = Harness::new();
        h.more_clips.push(Arc::new(bardo_ai::KlingClips::new()));
        h
    }

    fn remaining(view: &ProductionView, mode: ProductionMode) -> Money {
        view.estimate(mode).unwrap().remaining
    }

    // Before anything is made, from the rate table and the first guesses:
    // Claude writes the script (1,000 tokens in, 4,000 out at US$ 4 and
    // US$ 20 a million: US$ 0.084) and plans the scenes (2,000 in, 4,000
    // out: US$ 0.088); ElevenLabs reads 15 characters a second at US$ 0.10
    // a thousand; Nano Banana draws a scene every 6 seconds and one
    // portrait at US$ 0.10157 each (1,680 image tokens at US$ 60 a million,
    // 40 in, 250 out).
    const SCRIPT_AND_PLAN: u64 = 84_000 + 88_000;
    const IMAGE: u64 = 101_570;

    #[test]
    fn a_short_and_a_long_video_are_priced_from_the_rate_table_before_anything_is_made() {
        let h = harness();
        let app = h.start();
        let long = project(&app);
        let short = project(&app);
        app.set_project_format(short.id, Some(VideoFormat::Short))
            .unwrap();

        // A minute: 900 characters, ten scenes and a portrait. The hook is
        // the scenes at 0 and 6 seconds, each a 10-second Kling 2.6 clip
        // (it makes 5 or 10) at US$ 0.042 a second; Kling 3.0 makes any
        // length, so premium pays the 60 seconds at US$ 0.112.
        let view = app.production(short.id).unwrap();
        let economy = SCRIPT_AND_PLAN + 90_000 + 11 * IMAGE;
        assert_eq!(view.spent, Money::ZERO);
        assert_eq!(
            remaining(&view, ProductionMode::Economy),
            Money::from_micros(economy)
        );
        assert_eq!(
            remaining(&view, ProductionMode::AnimatedHook),
            Money::from_micros(economy + 20 * 42_000)
        );
        assert_eq!(
            remaining(&view, ProductionMode::AllAnimated),
            Money::from_micros(economy + 100 * 42_000)
        );
        assert_eq!(
            remaining(&view, ProductionMode::Premium),
            Money::from_micros(economy + 60 * 112_000)
        );
        let hook = view.estimate(ProductionMode::AnimatedHook).unwrap();
        assert_eq!(hook.clips, 2);
        assert!(!hook.partial);
        assert_eq!(hook.missing_key, Some(Provider::Kling));
        assert_eq!(view.estimate(ProductionMode::Economy).unwrap().clips, 0);

        // Eight minutes: 7,200 characters, 80 scenes and a portrait.
        let view = app.production(long.id).unwrap();
        let economy = SCRIPT_AND_PLAN + 720_000 + 81 * IMAGE;
        assert_eq!(
            remaining(&view, ProductionMode::Economy),
            Money::from_micros(economy)
        );
        assert_eq!(
            remaining(&view, ProductionMode::AnimatedHook),
            Money::from_micros(economy + 20 * 42_000)
        );
        assert_eq!(
            remaining(&view, ProductionMode::AllAnimated),
            Money::from_micros(economy + 800 * 42_000)
        );
        assert_eq!(
            remaining(&view, ProductionMode::Premium),
            Money::from_micros(economy + 480 * 112_000)
        );
        assert_eq!(view.total(view.mode), Some(remaining(&view, view.mode)));
    }

    #[test]
    fn a_project_follows_its_channel_until_it_picks_its_own_mode() {
        let h = harness();
        let app = h.start();
        let project = project(&app);
        let view = app.production(project.id).unwrap();
        assert_eq!(view.channel_mode, ProductionMode::AnimatedHook);
        assert_eq!((view.mode, view.own), (ProductionMode::AnimatedHook, None));

        let own = app
            .set_project_production(project.id, Some(ProductionMode::Economy))
            .unwrap();
        assert_eq!(own.production, Some(ProductionMode::Economy));
        assert_eq!(
            app.production(project.id).unwrap().mode,
            ProductionMode::Economy
        );
        app.set_project_production(project.id, None).unwrap();
        assert_eq!(
            app.production(project.id).unwrap().mode,
            ProductionMode::AnimatedHook
        );
        assert!(matches!(
            app.set_project_production(VideoProjectId::new(), None),
            Err(ScriptError::ProjectNotFound)
        ));
    }

    #[test]
    fn the_estimate_counts_only_what_is_left_to_make() {
        let h = harness();
        let app = h.start();
        let (project, plan) = h.planned_project(&app);
        app.set_project_production(project.id, Some(ProductionMode::Economy))
            .unwrap();
        let view = app.production(project.id).unwrap();
        // Only the images are left: one per scene, nobody to portray.
        assert_eq!(
            remaining(&view, ProductionMode::Economy),
            Money::from_micros(plan.scenes().len() as u64 * IMAGE)
        );
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let view = app.production(project.id).unwrap();
        assert_eq!(remaining(&view, ProductionMode::Economy), Money::ZERO);
        assert_eq!(view.total(ProductionMode::Economy), Some(view.spent));
    }

    #[test]
    fn a_clip_waiting_for_review_is_not_counted_again() {
        // The harness's own clip model, which needs no key.
        let h = Harness::new();
        let app = h.start();
        let (project, plan) = h.drawn_project(&app);
        app.set_project_production(project.id, Some(ProductionMode::ChannelModel))
            .unwrap();
        let clips = |app: &Bardo| {
            app.production(project.id)
                .unwrap()
                .estimate(ProductionMode::ChannelModel)
                .unwrap()
                .clips
        };
        assert_eq!(clips(&app), plan.scenes().len());
        done(
            &app,
            app.generate_scene_clip(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        assert!(
            app.scenes(project.id).unwrap().plan.unwrap().scenes()[0]
                .pending_clip()
                .is_some()
        );
        assert_eq!(clips(&app), plan.scenes().len() - 1);
    }

    #[test]
    fn animate_missing_follows_the_mode() {
        let h = harness();
        // Thirty seconds of narration over three scenes: the last ones
        // start past the hook.
        *h.speech.length.lock().unwrap() = Duration::from_secs(30);
        let app = h.start();
        let (project, plan) = h.drawn_project(&app);
        let starts: Vec<Duration> = plan.scenes().iter().map(|scene| scene.start).collect();
        let hook: Vec<usize> = (0..starts.len())
            .filter(|&index| starts[index] < bardo_domain::HOOK_LENGTH)
            .collect();
        assert!(!hook.is_empty() && hook.len() < starts.len(), "{starts:?}");

        let view = app.scenes(project.id).unwrap().clips;
        assert_eq!(view.mode, ProductionMode::AnimatedHook);
        assert_eq!(view.missing, hook);
        // Kling 2.6 at 720p animates the hook.
        assert_eq!(
            view.default_model.as_ref().map(|model| model.id.model()),
            Some("kling-2.6/720p")
        );

        app.set_project_production(project.id, Some(ProductionMode::Economy))
            .unwrap();
        assert!(app.scenes(project.id).unwrap().clips.missing.is_empty());
        assert!(matches!(
            app.generate_missing_clips(project.id, BudgetConsent::Ask),
            Err(SceneError::NothingToAnimate)
        ));
        // A scene can still be animated by hand.
        assert!(
            app.scenes(project.id).unwrap().clips.scenes[2]
                .model
                .is_some()
        );

        // Channels from before modes animate every scene with their model.
        let channel = app
            .channels()
            .unwrap()
            .into_iter()
            .find(|channel| channel.id == project.channel)
            .unwrap();
        app.update_channel(
            channel.id,
            ChannelDraft {
                production: ProductionMode::ChannelModel,
                ..ChannelDraft::from(&channel.details)
            },
        )
        .unwrap();
        app.set_project_production(project.id, None).unwrap();
        let view = app.scenes(project.id).unwrap().clips;
        assert_eq!(view.missing, (0..starts.len()).collect::<Vec<_>>());
        assert_eq!(
            view.default_model.map(|model| model.id),
            app.clip_models().first().map(|model| model.id.clone())
        );
    }

    #[test]
    fn the_cap_stops_a_job_that_would_pass_it_and_consent_lets_it_go_on() {
        let h = harness();
        let app = h.start();
        let project = h.narrated_project(&app);
        let spent = app.production(project.id).unwrap().spent;

        for typed in ["lots", "0", "-1"] {
            assert!(matches!(
                app.set_spend_cap(project.id, typed),
                Err(ScriptError::CapInvalid)
            ));
        }
        // A millionth of a dollar more than spent: the plan passes it.
        let typed = spent.saturating_add(Money::from_micros(1)).to_string();
        let capped = app.set_spend_cap(project.id, &typed).unwrap();
        assert!(capped.spend_cap.is_some());
        let cap = capped.spend_cap.unwrap();
        assert!(app.production(project.id).unwrap().over_cap());

        h.answer(crate::scenes::tests::plan_answer());
        let Err(SceneError::OverBudget(estimate)) =
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
        else {
            panic!("the plan would pass the cap");
        };
        let reached = estimate.cap.unwrap();
        assert_eq!((reached.cap, reached.spent), (cap, spent));
        assert!(reached.rest >= estimate.total());
        assert!(estimate.over_budget().next().is_none(), "no budget is set");

        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Confirmed)
                .unwrap(),
        );
        // Without a cap nothing asks.
        assert_eq!(app.set_spend_cap(project.id, " ").unwrap().spend_cap, None);
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
    }
}
