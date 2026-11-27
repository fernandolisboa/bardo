//! Visual style use cases (issue #259): how a video's pictures are
//! rendered. A project follows its channel's style unless it picks its
//! own. Bardo adds the style to every image prompt of the video (scenes,
//! cast portraits, the thumbnail's background) and tells the planner which
//! one it is, so the look does not depend on a prompt repeating it.

use bardo_domain::{RepositoryError, StyleChoice, ThemeRepository, VideoProject, VideoProjectId};

use crate::{Bardo, ScriptError};

/// What the planner is told about the video's style, in English: that
/// one was chosen and the words Bardo adds for it, or that none was. The
/// words name the style themselves, so no interface string goes to Claude.
pub(crate) fn style_for_planner(style: StyleChoice) -> String {
    match style {
        StyleChoice::FromNotes => "not chosen: the look comes from the aesthetic notes".to_owned(),
        StyleChoice::Catalog(style) => format!(
            "chosen. Bardo adds it to every image itself, in these words: {}",
            style.prompt()
        ),
    }
}

impl Bardo {
    /// How `project`'s pictures are rendered: its own style, else its
    /// channel's (from the notes when the channel is gone).
    pub fn video_style(&self, project: &VideoProject) -> Result<StyleChoice, RepositoryError> {
        let details = self.video_details(project)?;
        Ok(project.style(details.map_or(StyleChoice::default(), |details| details.style())))
    }

    /// Sets the project's own style, or goes back to the channel's
    /// (`None`). What was drawn stays as it is: the next scene plan,
    /// image, portrait and thumbnail background follow the new style, and
    /// the scenes panel says which scenes were drawn in another one.
    pub fn set_project_style(
        &self,
        project: VideoProjectId,
        style: Option<StyleChoice>,
    ) -> Result<VideoProject, ScriptError> {
        let mut project = self
            .themes
            .project(project)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ScriptError::ProjectNotFound)?;
        self.themes.set_project_style(project.id, style)?;
        project.style = style;
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{ChannelDraft, ImageShape, ProjectFiles, UiLanguage, VisualStyle};

    use super::*;
    use crate::BudgetConsent;
    use crate::i18n::{Catalog, Text};
    use crate::scenes::tests::{Harness, cast_answer, cast_project, done};

    const MANGA: StyleChoice = StyleChoice::Catalog(VisualStyle::Manga);
    const CLAY: StyleChoice = StyleChoice::Catalog(VisualStyle::ClayStopMotion);
    const STYLE_WORDS: &str = "Render this whole image in this visual style";

    #[test]
    fn every_style_is_named_in_both_languages() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            let mut names: Vec<String> = StyleChoice::all()
                .map(|style| catalog.get(Text::VisualStyleName(style)).into_owned())
                .collect();
            assert!(names.iter().all(|name| !name.trim().is_empty()));
            names.sort();
            names.dedup();
            assert_eq!(names.len(), 27, "{language:?}: each its own name");
        }
        assert!(style_for_planner(StyleChoice::FromNotes).starts_with("not chosen"));
        let told = style_for_planner(MANGA);
        assert!(told.starts_with("chosen. Bardo adds it to every image itself"));
        assert!(told.ends_with(VisualStyle::Manga.prompt()));
    }

    #[test]
    fn with_no_style_no_image_request_carries_one() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        let asked = h.text.requests().pop().unwrap();
        assert!(asked.prompt.contains("Aesthetic notes: grainy 1960s film"));
        assert!(
            asked
                .prompt
                .contains("Visual style: not chosen: the look comes from the aesthetic notes")
        );
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let requests = h.images.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        assert!(requests.iter().all(|r| !r.prompt.contains(STYLE_WORDS)));
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.style, StyleChoice::FromNotes);
        assert!(view.restyled.is_empty());
        assert_eq!(view.restyle_estimate, None);
    }

    #[test]
    fn with_a_style_the_plan_every_image_and_every_portrait_carry_it() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        app.set_project_style(project.id, Some(MANGA)).unwrap();
        h.answer(cast_answer());
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        let asked = h.text.requests().pop().unwrap();
        assert!(asked.prompt.contains(&format!(
            "Visual style: chosen. Bardo adds it to every image itself, in these words: {}",
            VisualStyle::Manga.prompt()
        )));
        assert!(
            asked
                .instructions
                .contains("names a style and says Bardo adds it")
        );

        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let requests = h.images.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4, "the chef, then three scenes");
        let line = MANGA.line().unwrap();
        assert_eq!(requests[0].shape, ImageShape::Portrait);
        assert!(requests[0].prompt.contains(&line), "the portrait too");
        assert!(
            requests[0]
                .prompt
                .ends_with("with the whole face clearly lit.")
        );
        for request in &requests[1..] {
            assert_eq!(request.prompt.matches(STYLE_WORDS).count(), 1);
            assert!(request.prompt.contains(&line));
        }
        // After the people, before what the other shots are for.
        let second = &requests[2].prompt;
        assert!(second.find(&line).unwrap() < second.find("The reference images").unwrap());
        // Each image belongs with the ones before it, all in the style.
        assert_eq!(requests[2].references.len(), 1);
        let view = app.scenes(project.id).unwrap();
        assert_eq!(view.style, MANGA);
        assert!(view.restyled.is_empty());
        assert_eq!(view.missing_images(), 0);
    }

    #[test]
    fn a_project_follows_its_channel_until_it_picks_its_own_style() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        assert_eq!(app.video_style(&project).unwrap(), StyleChoice::FromNotes);

        let channel = app.channels.get(project.channel.unwrap()).unwrap().unwrap();
        app.update_channel(
            channel.id,
            ChannelDraft {
                style: CLAY,
                ..ChannelDraft::from(&channel.details)
            },
        )
        .unwrap();
        assert_eq!(app.scenes(project.id).unwrap().style, CLAY);

        let own = app.set_project_style(project.id, Some(MANGA)).unwrap();
        assert_eq!(own.style, Some(MANGA));
        assert_eq!(app.scenes(project.id).unwrap().style, MANGA);
        // The notes are a choice of its own, not the channel's.
        app.set_project_style(project.id, Some(StyleChoice::FromNotes))
            .unwrap();
        assert_eq!(
            app.scenes(project.id).unwrap().style,
            StyleChoice::FromNotes
        );
        app.set_project_style(project.id, None).unwrap();
        assert_eq!(app.scenes(project.id).unwrap().style, CLAY);
        assert!(matches!(
            app.set_project_style(VideoProjectId::new(), None),
            Err(ScriptError::ProjectNotFound)
        ));
    }

    #[test]
    fn a_new_style_keeps_what_was_drawn_and_draws_the_rest_in_it() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let before = app.scenes(project.id).unwrap().plan.unwrap();
        let files = |plan: &bardo_domain::ScenePlan| -> Vec<String> {
            plan.files().map(str::to_owned).collect()
        };
        assert_eq!(h.images.requests.lock().unwrap().len(), 4);

        // The fake draws every scene alike: mark the ones of the old look,
        // to tell them from the new ones among the references.
        const OLD_LOOK: &[u8] = b"an image of the old look";
        for scene in before.scenes() {
            h.files
                .write(project.id, &scene.image().unwrap().file, OLD_LOOK)
                .unwrap();
        }

        // Changing the style draws nothing and touches nothing.
        app.set_project_style(project.id, Some(CLAY)).unwrap();
        let view = app.scenes(project.id).unwrap();
        let plan = view.plan.clone().unwrap();
        assert_eq!(files(&plan), files(&before));
        assert_eq!(h.images.requests.lock().unwrap().len(), 4);
        assert_eq!(view.restyled, [0, 1, 2]);
        assert_eq!(view.missing_images(), 0, "nothing is missing");
        // Three scenes and the chef's portrait, which is in the old look.
        assert_eq!(
            view.restyle_estimate,
            Some(app.estimate(&[crate::scenes::images_call(4)]).unwrap())
        );
        assert_eq!(
            view.image_estimates[0],
            app.estimate(&[crate::scenes::images_call(2)]).unwrap(),
            "the chef's scene alone draws him again first"
        );

        done(
            &app,
            app.redraw_restyled_scenes(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let requests = h.images.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 8);
        let line = CLAY.line().unwrap();
        assert_eq!(requests[4].shape, ImageShape::Portrait);
        assert!(requests[4].prompt.contains(&line));
        let redrawn = app.scenes(project.id).unwrap();
        let plan = redrawn.plan.clone().unwrap();
        let portrait = plan.cast()[0].portrait().unwrap();
        assert!(CLAY.drew(&portrait.generation.prompt));
        let portrait = h.files.read(project.id, &portrait.file).unwrap();
        // The chef's scenes go with his new portrait, and no image of the
        // old look is a reference. The first scene has nothing else to go
        // by; the next ones go by the new images drawn before them, which
        // wait for review.
        assert_eq!(requests[5].references.len(), 1);
        assert_eq!(requests[5].references[0].bytes, portrait);
        assert!(!requests[5].prompt.contains("another shot of this video"));
        let first = plan.scenes()[0].pending().unwrap();
        let first = h.files.read(project.id, &first.file).unwrap();
        assert_ne!(first, OLD_LOOK);
        assert_eq!(requests[6].references.len(), 1);
        assert_eq!(requests[6].references[0].bytes, first);
        assert_eq!(requests[7].references.len(), 2);
        assert_eq!(requests[7].references[0].bytes, portrait);
        assert_eq!(requests[7].references[1].bytes, first);
        for request in &requests[6..] {
            assert!(request.prompt.contains("another shot of this video"));
        }
        for request in &requests[5..] {
            assert!(request.prompt.contains(&line));
        }
        // Each new image waits beside the one it would replace.
        for (index, scene) in plan.scenes().iter().enumerate() {
            assert_eq!(scene.image(), before.scenes()[index].image());
            assert!(CLAY.drew(&scene.pending().unwrap().generation.prompt));
        }
        assert!(redrawn.restyled.is_empty(), "drawn again, to review");
        assert!(matches!(
            app.redraw_restyled_scenes(project.id, BudgetConsent::Ask),
            Err(crate::SceneError::NothingToGenerate)
        ));

        // Accepted, the first is what the next ones belong with.
        app.accept_scene_image(project.id, 0).unwrap();
        done(
            &app,
            app.regenerate_scene_image(project.id, 1, BudgetConsent::Ask)
                .unwrap(),
        );
        let request = h.images.requests.lock().unwrap().last().cloned().unwrap();
        assert_eq!(request.references.len(), 1);
        assert!(request.prompt.contains("another shot of this video"));
    }

    #[test]
    fn a_job_from_before_the_style_changed_does_not_run_again() {
        let h = Harness::new();
        let app = h.start();
        let project = cast_project(&h, &app);
        h.images.decline("control room");
        let id = app
            .generate_scene_images(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(
            crate::scenes::tests::wait_done(&app, id).state(),
            bardo_domain::JobState::Failed
        );
        h.images.accept_all();
        let asked = h.images.requests.lock().unwrap().len();

        // Run again under another style, it would draw the declined scene
        // from the notes and leave it in another style at once.
        app.set_project_style(project.id, Some(MANGA)).unwrap();
        let refused = app.retry_job(id).unwrap_err();
        assert!(matches!(refused, crate::jobs::JobActionError::Restyled));
        assert_eq!(refused.message(), Text::JobRestyled);
        assert_eq!(h.images.requests.lock().unwrap().len(), asked);

        // Back in the style it was started with, it runs.
        app.set_project_style(project.id, None).unwrap();
        app.retry_job(id).unwrap();
        done(&app, id);
        assert_eq!(h.images.requests.lock().unwrap().len(), asked + 1);
    }
}
