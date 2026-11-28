//! The video's summary under its title (#366): who narrates it, its
//! format, length and visual style, and how it is produced, each as the
//! video makes it (its own choice, else its channel's), on one line. Its
//! settings change in a popup over the screen, whose open state lives
//! here, and the user may collapse the summary for every video.

use bardo_domain::{
    ProductionMode, StyleChoice, UserProfile, VideoFormat, VideoLength, VideoProjectId,
};

use crate::{AppError, Bardo, ScriptError};

/// What an open video is, as its summary shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSummary {
    /// Who reads the narration: the video's own narrator, else its
    /// channel's default; `None` with nobody.
    pub narrator: Option<String>,
    pub format: VideoFormat,
    /// The video's own length; `None` follows its format's.
    pub length: Option<VideoLength>,
    pub style: StyleChoice,
    pub mode: ProductionMode,
}

impl Bardo {
    /// The open video's summary.
    pub fn video_summary(&self, project: VideoProjectId) -> Result<VideoSummary, ScriptError> {
        let project = self
            .themes
            .project(project)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ScriptError::ProjectNotFound)?;
        let narrator = self.narrator(&project)?;
        Ok(VideoSummary {
            narrator: narrator.map(|persona| persona.details.name().to_owned()),
            format: self.video_format(&project)?,
            length: project.length,
            style: self.video_style(&project)?,
            mode: self.production_mode(&project)?,
        })
    }

    /// Opens `project`'s settings popup over its screen.
    pub fn open_video_settings(&mut self, project: VideoProjectId) {
        self.video_settings_open = Some(project);
    }

    /// Closes the video settings popup.
    pub fn close_video_settings(&mut self) {
        self.video_settings_open = None;
    }

    /// Whether `project`'s settings popup is open: never for another
    /// video, so opening one closes it.
    pub fn video_settings_open(&self, project: Option<VideoProjectId>) -> bool {
        project.is_some() && self.video_settings_open == project
    }

    /// Whether an open video shows its summary under its title.
    pub fn video_summary_shown(&self) -> bool {
        self.profile.video_summary
    }

    /// Shows or collapses the summary of every video, and remembers it.
    /// On failure it stays as it was.
    pub fn set_video_summary_shown(&mut self, shown: bool) -> Result<(), AppError> {
        if shown == self.profile.video_summary {
            return Ok(());
        }
        let updated = UserProfile {
            video_summary: shown,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        ChannelDraft, ContentLanguage, Country, Niche, Theme, ThemeIdea, ThemeRepository,
        VisualStyle,
    };

    use super::*;
    use crate::scenes::tests::{Harness, project};

    #[test]
    fn a_video_that_chose_nothing_shows_its_channels_settings() {
        let h = Harness::new();
        let app = h.start();
        let video = project(&app);

        let summary = app.video_summary(video.id).unwrap();
        assert_eq!(
            summary,
            VideoSummary {
                narrator: Some("Documentary Narrator (en-US)".to_owned()),
                format: VideoFormat::Long,
                length: None,
                style: StyleChoice::FromNotes,
                mode: ProductionMode::default(),
            }
        );
    }

    #[test]
    fn the_summary_follows_every_setting_the_video_changes() {
        let h = Harness::new();
        let app = h.start();
        let video = project(&app);
        let other = app
            .personas()
            .unwrap()
            .into_iter()
            .find(|persona| persona.details.name() != "Documentary Narrator (en-US)")
            .unwrap();
        let manga = StyleChoice::Catalog(VisualStyle::Manga);
        let mode = ProductionMode::ALL
            .into_iter()
            .find(|mode| *mode != ProductionMode::default())
            .unwrap();
        let length = VideoLength::ALL[0];

        app.set_project_persona(video.id, Some(other.id)).unwrap();
        app.set_project_format(video.id, Some(VideoFormat::Short))
            .unwrap();
        app.set_project_length(video.id, Some(length)).unwrap();
        app.set_project_style(video.id, Some(manga)).unwrap();
        app.set_project_production(video.id, Some(mode)).unwrap();

        let summary = app.video_summary(video.id).unwrap();
        assert_eq!(
            summary,
            VideoSummary {
                narrator: Some(other.details.name().to_owned()),
                format: VideoFormat::Short,
                length: Some(length),
                style: manga,
                mode,
            }
        );
    }

    #[test]
    fn a_video_with_nobody_to_narrate_says_so() {
        let h = Harness::new();
        let app = h.start();
        let channel = app
            .create_channel(ChannelDraft {
                name: "No voice".into(),
                niche: "space history".into(),
                language: ContentLanguage::Portuguese,
                country: Country::Brazil,
                default_persona: None,
                ..ChannelDraft::default()
            })
            .unwrap();
        let mut theme = Theme::suggested(
            app.profile().id,
            channel.id,
            Niche::new("space history").unwrap(),
            ThemeIdea::new("Quiet probe", "").unwrap(),
            std::time::SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let video = theme.approve(std::time::SystemTime::now()).unwrap();
        app.themes.start_project(&theme, &video).unwrap();

        let summary = app.video_summary(video.id).unwrap();
        assert_eq!(summary.narrator, None);
    }

    #[test]
    fn the_settings_popup_is_open_for_one_video_until_closed() {
        let h = Harness::new();
        let mut app = h.start();
        let video = project(&app).id;
        let other = project(&app).id;
        assert!(!app.video_settings_open(Some(video)));

        app.open_video_settings(video);
        assert!(app.video_settings_open(Some(video)));
        assert!(!app.video_settings_open(Some(other)), "another video");
        assert!(!app.video_settings_open(None), "no video open");

        app.open_video_settings(other);
        assert!(!app.video_settings_open(Some(video)));
        assert!(app.video_settings_open(Some(other)));

        app.close_video_settings();
        assert!(!app.video_settings_open(Some(other)));

        // A tour lights the screen: the popup closes.
        app.open_video_settings(video);
        app.start_tour(
            bardo_domain::TourId::Projects,
            crate::Destination::Videos,
            false,
        )
        .unwrap();
        assert!(!app.video_settings_open(Some(video)));
    }

    #[test]
    fn the_summary_shows_until_collapsed_and_stays_collapsed_after_a_restart() {
        let h = Harness::new();
        let mut app = h.start();
        assert!(app.video_summary_shown());

        app.set_video_summary_shown(false).unwrap();
        assert!(!app.video_summary_shown());
        drop(app);

        let mut app = h.start();
        assert!(!app.video_summary_shown(), "remembered");
        app.set_video_summary_shown(true).unwrap();
        assert!(app.video_summary_shown());
    }
}
