//! Video format use cases (issue #179): what kind of video a project makes,
//! Short or Long. A project follows its channel's format unless it picks
//! its own. The format reaches the script and scene templates, the shape
//! images and clips are drawn in, the shape a new cut starts in and the
//! YouTube preset a render uses (ADR-0018).

use bardo_domain::{RepositoryError, ThemeRepository, VideoFormat, VideoProject, VideoProjectId};

use crate::{Bardo, ScriptError};

impl Bardo {
    /// What kind of video `project` makes: its own format, else its
    /// channel's (a Short when the channel is gone).
    pub fn video_format(&self, project: &VideoProject) -> Result<VideoFormat, RepositoryError> {
        let channel = self.channels.get(project.channel)?;
        Ok(project
            .format(channel.map_or(VideoFormat::default(), |channel| channel.details.format())))
    }

    /// Sets the project's own format, or goes back to the channel's
    /// (`None`). What was made before stays as it is: the next script,
    /// scene plan, image, clip, new cut and render follow the new format.
    pub fn set_project_format(
        &self,
        project: VideoProjectId,
        format: Option<VideoFormat>,
    ) -> Result<VideoProject, ScriptError> {
        let mut project = self
            .themes
            .project(project)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ScriptError::ProjectNotFound)?;
        self.themes.set_project_format(project.id, format)?;
        project.format = format;
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{AspectRatio, Network, NetworkAccountDraft};

    use super::*;
    use crate::BudgetConsent;
    use crate::scenes::tests::{Harness, SCRIPT, done, plan_answer, project};

    #[test]
    fn a_project_follows_its_channel_until_it_picks_its_own_format() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        assert_eq!(project.format, None);
        assert_eq!(app.video_format(&project).unwrap(), VideoFormat::Long);

        let short = app
            .set_project_format(project.id, Some(VideoFormat::Short))
            .unwrap();
        assert_eq!(short.format, Some(VideoFormat::Short));
        let stored = app.themes.project(project.id).unwrap().unwrap();
        assert_eq!(app.video_format(&stored).unwrap(), VideoFormat::Short);

        let back = app.set_project_format(project.id, None).unwrap();
        assert_eq!(app.video_format(&back).unwrap(), VideoFormat::Long);
        assert!(matches!(
            app.set_project_format(VideoProjectId::new(), None),
            Err(ScriptError::ProjectNotFound)
        ));
    }

    #[test]
    fn a_short_gets_a_short_script_tall_pictures_a_vertical_cut_and_the_shorts_preset() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        app.set_project_format(project.id, Some(VideoFormat::Short))
            .unwrap();

        h.answer(SCRIPT.to_owned());
        done(
            &app,
            app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
        );
        let script = h.text.requests().pop().unwrap();
        assert!(
            script.prompt.contains("45 to 60 seconds"),
            "{}",
            script.prompt
        );
        done(
            &app,
            app.generate_narration(project.id, BudgetConsent::Ask)
                .unwrap(),
        );

        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        let plan = h.text.requests().pop().unwrap();
        assert!(
            plan.instructions.contains("tall (9:16"),
            "{}",
            plan.instructions
        );

        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let drawn = h.images.requests.lock().unwrap().clone();
        assert!(!drawn.is_empty());
        assert!(
            drawn
                .iter()
                .all(|request| request.aspect == AspectRatio::Vertical)
        );

        assert_eq!(
            app.open_editor(project.id).unwrap().aspect(),
            AspectRatio::Vertical
        );

        let youtube = app
            .add_network_account(
                project.channel,
                Network::YouTube,
                NetworkAccountDraft {
                    handle: "archives".into(),
                    ..NetworkAccountDraft::default()
                },
            )
            .unwrap();
        let review = app.render_review(project.id).unwrap();
        assert_eq!(review.targets[0].account, youtube.id);
        assert_eq!(
            review.targets[0].preset,
            Network::YouTube.render_preset(VideoFormat::Short)
        );
    }

    #[test]
    fn a_long_video_gets_a_long_script_wide_pictures_and_the_long_youtube_preset() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let requests = h.text.requests();
        assert!(requests[0].prompt.contains("8 to 10 minutes"));
        assert!(
            requests
                .last()
                .unwrap()
                .instructions
                .contains("wide (16:9)")
        );
        let drawn = h.images.requests.lock().unwrap().clone();
        assert!(
            drawn
                .iter()
                .all(|request| request.aspect == AspectRatio::Landscape)
        );

        app.add_network_account(
            project.channel,
            Network::YouTube,
            NetworkAccountDraft {
                handle: "archives".into(),
                ..NetworkAccountDraft::default()
            },
        )
        .unwrap();
        let review = app.render_review(project.id).unwrap();
        assert_eq!(
            review.targets[0].preset,
            Network::YouTube.render_preset(VideoFormat::Long)
        );
        assert_eq!(review.cut.aspect, AspectRatio::Landscape);
    }
}
