//! Loose videos (issue #341, spec "Loose videos"): videos with no project
//! and no channel, for a quick try. *No project* in the switcher opens
//! them. A loose video starts from an idea the user types and holds itself
//! everything a channel would give it ([`VideoSettings`]), copied from a
//! channel the user picks or a new channel's defaults; only global library
//! items appear in it; it exports files but is never uploaded or
//! scheduled. *Move to a project* gives it a channel, whose network
//! accounts then publish it, and it keeps its settings as its own.
//!
//! Every stage reads a video's settings through [`Bardo::video_details`],
//! never through its channel alone.

use std::path::PathBuf;
use std::time::SystemTime;

use bardo_domain::{
    ChannelDetails, ChannelId, ChannelRepository, GLOBAL_DIR, LOOSE_VIDEOS_DIR, Network,
    NetworkAccount, RepositoryError, SettingDifference, ThemeFieldError, ThemeIdea,
    ThemeRepository, VideoProject, VideoProjectId, VideoSettings, loose_video,
};

use crate::{AppError, Bardo, Text};

/// A loose video as the user asks for it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LooseVideoDraft {
    pub title: String,
    pub angle: String,
    /// The channel whose settings it starts with; `None` for a new
    /// channel's defaults.
    pub copy_from: Option<ChannelId>,
}

#[derive(Debug, thiserror::Error)]
pub enum LooseError {
    /// The typed idea breaks one or more rules; the form shows each one.
    #[error("invalid idea: {0:?}")]
    Invalid(Vec<ThemeFieldError>),
    #[error("channel not found")]
    ChannelNotFound,
    #[error("video not found")]
    VideoNotFound,
    /// Only a loose video moves into a project this way.
    #[error("the video is not loose")]
    NotLoose,
    /// A job for the video is queued or running.
    #[error("a job of the video is under way")]
    Busy,
    /// The channel's project is archived.
    #[error("the project is archived")]
    ProjectArchived,
    /// The loose video itself is archived; it moves once unarchived.
    #[error("the video is archived")]
    VideoArchived,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl From<AppError> for LooseError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Repository(error) => LooseError::Repository(error),
        }
    }
}

impl LooseError {
    /// What the form or the dialog says.
    pub fn message(&self) -> Text {
        match self {
            LooseError::Invalid(errors) => errors
                .first()
                .map_or(Text::LooseNotSaved, |error| Text::ThemeFieldError(*error)),
            LooseError::ChannelNotFound => Text::ChannelNotFound,
            LooseError::VideoNotFound | LooseError::NotLoose => Text::LooseVideoNotFound,
            LooseError::Busy => Text::LooseMoveBusy,
            LooseError::ProjectArchived => Text::LooseMoveArchived,
            LooseError::VideoArchived => Text::LooseMoveVideoArchived,
            LooseError::Repository(_) => Text::LooseNotSaved,
        }
    }
}

/// The settings `video`'s stages read (see [`Bardo::video_details`]), for
/// job handlers that hold the stores themselves.
pub(crate) fn details_of(
    themes: &dyn ThemeRepository,
    channels: &dyn ChannelRepository,
    video: &VideoProject,
) -> Result<Option<ChannelDetails>, RepositoryError> {
    let channel = match video.channel {
        Some(id) => channels.get(id)?,
        None => None,
    };
    Ok(match (themes.video_settings(video.id)?, channel) {
        (Some(own), Some(channel)) => Some(own.under(&channel.details)),
        (Some(own), None) => Some(own.details),
        (None, channel) => channel.map(|channel| channel.details),
    })
}

impl Bardo {
    /// Whether *No project* is open: the loose videos, chosen in the
    /// switcher after every project opened.
    pub fn loose_open(&self) -> bool {
        match self.projects.loose_opened(self.profile.id) {
            Ok(Some(_)) => self.open_project_id().is_ok_and(|open| open.is_none()),
            Ok(None) => false,
            Err(error) => {
                tracing::warn!("could not read whether No project is open: {error}");
                false
            }
        }
    }

    /// Opens *No project*: the work screens show the loose videos from now
    /// on, and from the next start.
    pub fn open_loose_videos(&self) -> Result<(), AppError> {
        Ok(self
            .projects
            .mark_loose_opened(self.profile.id, SystemTime::now())?)
    }

    /// The profile's loose videos, newest first, archived ones too.
    pub fn loose_videos(&self) -> Result<Vec<VideoProject>, AppError> {
        Ok(self.themes.loose_projects(self.profile.id)?)
    }

    /// How many loose videos are not archived, for the switcher.
    pub fn loose_video_count(&self) -> usize {
        let archived = self.archived_videos();
        self.loose_videos()
            .map(|videos| {
                videos
                    .iter()
                    .filter(|video| !archived.contains(&video.id))
                    .count()
            })
            .unwrap_or_default()
    }

    /// Where new loose videos' folders go: `<base folder>\Global\Loose
    /// videos`.
    pub fn loose_videos_folder(&self) -> PathBuf {
        self.base_folder()
            .path
            .join(GLOBAL_DIR)
            .join(LOOSE_VIDEOS_DIR)
    }

    /// Starts a loose video from a typed idea, with the settings of the
    /// channel `copy_from` (its narrator only when global) or a new
    /// channel's defaults.
    pub fn start_loose_video(&self, draft: LooseVideoDraft) -> Result<VideoProject, LooseError> {
        let idea = ThemeIdea::new(&draft.title, &draft.angle).map_err(LooseError::Invalid)?;
        let source = match draft.copy_from {
            Some(id) => Some(
                self.channels
                    .get(id)?
                    .filter(|channel| channel.owner == self.profile.id)
                    .ok_or(LooseError::ChannelNotFound)?
                    .details,
            ),
            None => None,
        };
        let settings = VideoSettings::starting(&idea, source.as_ref(), |persona| {
            // Only global items appear in a loose video.
            self.personas
                .get(persona)
                .ok()
                .flatten()
                .is_some_and(|found| found.owner_project.is_none())
        });
        let video = loose_video(self.profile.id, &idea, &settings, SystemTime::now());
        self.themes
            .start_loose(&video, &settings, &self.loose_videos_folder())?;
        tracing::info!("started a loose video");
        Ok(video)
    }

    /// The channels a loose video can take its settings from or move
    /// into: every active project's, named `Project › Channel`, by name.
    pub fn loose_channel_choices(&self) -> Result<Vec<(ChannelId, String)>, AppError> {
        let projects = self.projects.projects(self.profile.id)?;
        let mut choices: Vec<(ChannelId, String)> = self
            .channels
            .list(self.profile.id)?
            .into_iter()
            .filter_map(|channel| {
                let project = projects
                    .iter()
                    .find(|project| project.id == channel.project && project.is_active())?;
                Some((
                    channel.id,
                    format!("{} › {}", project.name(), channel.details.name()),
                ))
            })
            .collect();
        choices.sort_by_key(|(_, name)| name.to_lowercase());
        Ok(choices)
    }

    /// The settings `video` holds itself: a loose video's, or one that
    /// was loose before it moved; `None` for every other video.
    pub fn own_video_settings(
        &self,
        video: VideoProjectId,
    ) -> Result<Option<VideoSettings>, AppError> {
        Ok(self.themes.video_settings(video)?)
    }

    /// The settings a video's stages read: its own when it holds them
    /// (under its channel's name, niche and themes once it has one), else
    /// its channel's; `None` when the channel is gone.
    pub fn video_details(
        &self,
        video: &VideoProject,
    ) -> Result<Option<ChannelDetails>, RepositoryError> {
        details_of(&*self.themes, &*self.channels, video)
    }

    /// The network accounts that publish `channel`'s videos; none for a
    /// loose video (`None`), which is never published.
    pub(crate) fn channel_accounts(
        &self,
        channel: Option<ChannelId>,
    ) -> Result<Vec<NetworkAccount>, RepositoryError> {
        match channel {
            Some(channel) => self.network_accounts.list(channel),
            None => Ok(Vec::new()),
        }
    }

    /// What `video` renders and exports for: its channel's network
    /// accounts, or for a loose video one target per network that no
    /// account stands behind ([`NetworkAccount::file_target`]).
    pub(crate) fn target_accounts(
        &self,
        video: &VideoProject,
    ) -> Result<Vec<NetworkAccount>, RepositoryError> {
        match video.channel {
            Some(channel) => self.network_accounts.list(channel),
            None => Ok(Network::ALL
                .into_iter()
                .map(|network| NetworkAccount::file_target(video.owner, video.id, network))
                .collect()),
        }
    }

    /// The angle a video takes: its theme's, or the one typed with a
    /// loose video's idea.
    pub(crate) fn video_angle(&self, video: &VideoProject) -> Result<String, RepositoryError> {
        if let Some(theme) = video.theme
            && let Some(theme) = self.themes.theme(theme)?
        {
            return Ok(theme.idea().angle().to_owned());
        }
        Ok(self
            .themes
            .video_settings(video.id)?
            .map(|settings| settings.angle)
            .unwrap_or_default())
    }

    fn own_loose_video(&self, id: VideoProjectId) -> Result<VideoProject, LooseError> {
        let video = self
            .themes
            .project(id)?
            .filter(|video| video.owner == self.profile.id)
            .ok_or(LooseError::VideoNotFound)?;
        if !video.is_loose() {
            return Err(LooseError::NotLoose);
        }
        Ok(video)
    }

    /// What the loose video keeps that differs from `channel`'s audience,
    /// to show before it moves there.
    pub fn loose_move_differences(
        &self,
        video: VideoProjectId,
        channel: ChannelId,
    ) -> Result<Vec<SettingDifference>, LooseError> {
        self.own_loose_video(video)?;
        let channel = self
            .channels
            .get(channel)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(LooseError::ChannelNotFound)?;
        Ok(self
            .themes
            .video_settings(video)?
            .map(|own| own.differences(&channel.details))
            .unwrap_or_default())
    }

    /// Moves a loose video into `channel`'s project: it takes the
    /// channel's network accounts for publishing, keeps its own settings,
    /// and its folder moves into the project's. Refused while one of its
    /// jobs is queued or running.
    pub fn move_loose_video(
        &self,
        video: VideoProjectId,
        channel: ChannelId,
    ) -> Result<(), LooseError> {
        let found = self.own_loose_video(video)?;
        if self.archived_videos().contains(&found.id) {
            return Err(LooseError::VideoArchived);
        }
        let channel = self
            .channels
            .get(channel)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(LooseError::ChannelNotFound)?;
        let project = self
            .projects
            .project(channel.project)?
            .ok_or(LooseError::ChannelNotFound)?;
        if !project.is_active() {
            return Err(LooseError::ProjectArchived);
        }
        let id = found.id.to_string();
        let busy = self.jobs().iter().any(|job| {
            job.state().is_active()
                && serde_json::from_str::<serde_json::Value>(job.payload())
                    .ok()
                    .and_then(|payload| {
                        payload
                            .get("project")
                            .and_then(serde_json::Value::as_str)
                            .map(|named| named == id)
                    })
                    .unwrap_or(false)
        });
        if busy {
            return Err(LooseError::Busy);
        }
        // Its files for the file targets go: the channel's accounts render
        // anew (the rows go with the move).
        let rendered = self.renders.renders(found.id)?;
        self.themes.move_loose(found.id, channel.id)?;
        for render in rendered {
            if let Err(error) = self.files.remove(found.id, &render.file) {
                tracing::warn!("could not remove a loose video's render: {error}");
            }
        }
        tracing::info!("moved a loose video into a project");
        // Its media follows it into the project's folder.
        let _ = self.queue_media_move();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bardo_domain::{ContentLanguage, Country, Network};
    use bardo_storage::MemoryExportFiles;

    use super::*;
    use crate::BudgetConsent;
    use crate::scenes::tests::{Harness, SCRIPT, done, plan_answer, project};
    use crate::{EditAction, PublicationError, UploadReviewError};

    fn draft(copy_from: Option<ChannelId>) -> LooseVideoDraft {
        LooseVideoDraft {
            title: "The probe that never came home".into(),
            angle: "told by the engineers who lost it".into(),
            copy_from,
        }
    }

    #[test]
    fn a_loose_video_goes_from_a_typed_idea_to_exported_files_with_no_channel() {
        let h = Harness::new();
        let exports: Arc<MemoryExportFiles> = Arc::default();
        let app = h.start_with_exports(Arc::clone(&exports) as _);
        let source = project(&app).channel;

        let video = app.start_loose_video(draft(source)).unwrap();
        assert!(video.is_loose());
        assert_eq!((video.channel, video.theme), (None, None));
        assert_eq!(video.title, "The probe that never came home");
        assert_eq!(
            app.loose_videos()
                .unwrap()
                .iter()
                .map(|v| v.id)
                .collect::<Vec<_>>(),
            [video.id]
        );
        assert_eq!(app.loose_video_count(), 1);
        let own = app.own_video_settings(video.id).unwrap().unwrap();
        assert_eq!(own.angle, "told by the engineers who lost it");
        // Copied from the channel, its global narrator included.
        assert_eq!(own.details.language(), ContentLanguage::Portuguese);
        assert_eq!(own.details.country(), Country::Brazil);
        assert!(own.details.default_persona().is_some());
        assert_eq!(own.details.name(), "The probe that never came home");

        // Every stage reads the video's own settings.
        h.answer(SCRIPT.to_owned());
        done(
            &app,
            app.generate_script(video.id, BudgetConsent::Ask).unwrap(),
        );
        let prompt = h.text.requests().last().unwrap().prompt.clone();
        assert!(
            prompt.contains("told by the engineers who lost it"),
            "{prompt}"
        );
        done(
            &app,
            app.generate_narration(video.id, crate::PicturesConsent::Ask, BudgetConsent::Ask)
                .unwrap(),
        );
        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(video.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        done(
            &app,
            app.generate_scene_images(video.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let mut editor = app.open_editor(video.id).unwrap();
        app.edit(&mut editor, EditAction::ShowCaptions(true))
            .unwrap();

        // One file per network, with no account behind any of them.
        let review = app.render_review(video.id).unwrap();
        assert_eq!(
            review.targets.iter().map(|t| t.network).collect::<Vec<_>>(),
            Network::ALL
        );
        assert!(review.targets.iter().all(|t| t.handle.is_empty()));
        crate::render::tests::render_all(&app, video.id);
        assert_eq!(app.renders(video.id).unwrap().len(), Network::ALL.len());

        h.text
            .answers
            .lock()
            .unwrap()
            .push(crate::export::tests::youtube_and_tiktok());
        done(
            &app,
            app.generate_metadata(video.id, BudgetConsent::Ask).unwrap(),
        );
        let view = app.export_view(video.id).unwrap();
        done(&app, app.start_export(&view, &[Network::YouTube]).unwrap());
        let names = exports.names(&view.package, Network::YouTube);
        assert_eq!(names, ["The probe nobody found.mp4", "metadata.txt"]);
        let metadata = String::from_utf8(
            exports
                .read(&view.package, Network::YouTube, "metadata.txt")
                .unwrap(),
        )
        .unwrap();
        assert!(metadata.starts_with("YouTube\r\n"), "{metadata}");

        // Never uploaded, scheduled or marked posted.
        assert!(matches!(
            app.upload_review(video.id, Network::YouTube),
            Err(UploadReviewError::NoAccount)
        ));
        assert!(matches!(
            app.mark_posted(video.id, Network::YouTube, "https://youtu.be/x", false),
            Err(PublicationError::NoAccount)
        ));
        assert!(app.channel_accounts(video.channel).unwrap().is_empty());

        // Moved, its file targets' renders and files go.
        let files: Vec<String> = app
            .renders(video.id)
            .unwrap()
            .into_iter()
            .map(|render| render.file)
            .collect();
        app.move_loose_video(video.id, source.unwrap()).unwrap();
        assert!(app.renders(video.id).unwrap().is_empty());
        assert!(
            files
                .iter()
                .all(|file| bardo_domain::ProjectFiles::read(&*h.files, video.id, file).is_err())
        );
    }

    #[test]
    fn a_loose_video_from_defaults_has_no_narrator_and_writes_its_script() {
        let h = Harness::new();
        let app = h.start();
        let video = app.start_loose_video(draft(None)).unwrap();
        let own = app.own_video_settings(video.id).unwrap().unwrap();
        assert_eq!(own.details.default_persona(), None);
        h.answer(SCRIPT.to_owned());
        done(
            &app,
            app.generate_script(video.id, BudgetConsent::Ask).unwrap(),
        );
        assert!(app.script(video.id).unwrap().script.is_some());
    }

    #[test]
    fn a_typed_idea_breaking_a_rule_or_another_profile_s_channel_is_refused() {
        let h = Harness::new();
        let app = h.start();
        assert!(matches!(
            app.start_loose_video(LooseVideoDraft {
                title: "  ".into(),
                ..draft(None)
            }),
            Err(LooseError::Invalid(_))
        ));
        assert!(matches!(
            app.start_loose_video(draft(Some(ChannelId::new()))),
            Err(LooseError::ChannelNotFound)
        ));
        assert!(app.loose_videos().unwrap().is_empty());
    }

    #[test]
    fn moving_a_loose_video_keeps_its_settings_and_makes_it_publishable() {
        let h = Harness::new();
        let app = h.start();
        let channel = project(&app).channel.unwrap();
        let video = app.start_loose_video(draft(None)).unwrap();
        let own = app.own_video_settings(video.id).unwrap().unwrap();

        let differences = app.loose_move_differences(video.id, channel).unwrap();
        assert_eq!(
            differences,
            [
                SettingDifference::Language {
                    video: own.details.language(),
                    channel: ContentLanguage::Portuguese
                },
                SettingDifference::Country {
                    video: own.details.country(),
                    channel: Country::Brazil
                }
            ]
        );

        app.move_loose_video(video.id, channel).unwrap();
        let moved = app.themes.project(video.id).unwrap().unwrap();
        assert_eq!(moved.channel, Some(channel));
        assert!(!moved.is_loose());
        assert!(app.loose_videos().unwrap().is_empty());
        // Its own settings stay, under the channel's name.
        assert_eq!(app.own_video_settings(video.id).unwrap(), Some(own.clone()));
        let details = app.video_details(&moved).unwrap().unwrap();
        assert_eq!(details.language(), own.details.language());
        assert_eq!(
            details.name(),
            app.channels.get(channel).unwrap().unwrap().details.name()
        );
        // The channel's accounts publish it now.
        let youtube = crate::render::tests::account(
            &app,
            &moved,
            Network::YouTube,
            bardo_domain::NetworkAccountDraft::default(),
        );
        assert_eq!(
            app.target_accounts(&moved)
                .unwrap()
                .iter()
                .map(|a| a.id)
                .collect::<Vec<_>>(),
            [youtube.id]
        );
        assert!(matches!(
            app.move_loose_video(video.id, channel),
            Err(LooseError::NotLoose)
        ));
    }

    #[test]
    fn a_moved_video_offers_versions_by_its_own_language_not_its_channel_s() {
        let h = Harness::new();
        let app = h.start();
        // Portuguese for Brazil.
        let channel = project(&app).channel.unwrap();
        let english = app
            .create_channel(bardo_domain::ChannelDraft {
                name: "Space Archives US".into(),
                niche: "space history".into(),
                language: ContentLanguage::English,
                country: Country::UnitedStates,
                ..bardo_domain::ChannelDraft::default()
            })
            .unwrap();
        let video = app.start_loose_video(draft(Some(english.id))).unwrap();
        app.move_loose_video(video.id, channel).unwrap();
        // The video stays in English for the United States: the English
        // channel is its own audience.
        assert!(matches!(
            app.make_version(video.id, english.id, BudgetConsent::Ask),
            Err(crate::LocalizationError::SameAudience)
        ));
    }

    #[test]
    fn a_narrator_held_by_a_loose_video_is_in_use() {
        let h = Harness::new();
        let app = h.start();
        let source = project(&app).channel;
        let video = app.start_loose_video(draft(source)).unwrap();
        let narrator = app
            .own_video_settings(video.id)
            .unwrap()
            .unwrap()
            .details
            .default_persona()
            .unwrap();
        let uses = app.persona_usage(narrator).unwrap();
        assert!(
            uses.iter()
                .any(|used| used.video.as_deref() == Some(video.title.as_str())),
            "{uses:?}"
        );
    }

    #[test]
    fn an_archived_loose_video_does_not_move() {
        let h = Harness::new();
        let app = h.start();
        let channel = project(&app).channel.unwrap();
        let video = app.start_loose_video(draft(None)).unwrap();
        app.archive_video(video.id).unwrap();
        assert!(matches!(
            app.move_loose_video(video.id, channel),
            Err(LooseError::VideoArchived)
        ));
        assert_eq!(
            LooseError::VideoArchived.message(),
            Text::LooseMoveVideoArchived
        );
        assert!(app.themes.project(video.id).unwrap().unwrap().is_loose());
    }

    #[test]
    fn deleting_says_whether_the_video_has_an_idea_to_give_back() {
        let h = Harness::new();
        let app = h.start();
        let started = project(&app);
        let channel = started.channel.unwrap();
        assert_eq!(app.delete_video_hint(started.id), Text::DeleteVideoHint);
        let video = app.start_loose_video(draft(None)).unwrap();
        assert_eq!(app.delete_video_hint(video.id), Text::DeleteLooseVideoHint);
        app.move_loose_video(video.id, channel).unwrap();
        assert_eq!(app.delete_video_hint(video.id), Text::DeleteVideoNoIdeaHint);
    }

    #[test]
    fn a_new_channel_while_no_project_is_open_is_refused_not_given_a_project() {
        let h = Harness::new();
        let app = h.start();
        project(&app);
        let projects = app.projects().unwrap().len();
        app.open_loose_videos().unwrap();
        let error = app
            .create_channel(bardo_domain::ChannelDraft {
                name: "Deep Sea Files".into(),
                ..bardo_domain::ChannelDraft::default()
            })
            .unwrap_err();
        assert!(matches!(error, crate::ChannelError::NeedsProject));
        assert_eq!(error.form_message(), Some(Text::ChannelNeedsProject));
        assert_eq!(app.projects().unwrap().len(), projects);
        assert!(app.loose_open());
    }

    #[test]
    fn a_moved_video_shows_its_own_format_on_the_overview() {
        let h = Harness::new();
        let app = h.start();
        let channel = project(&app).channel.unwrap();
        let home = app.channels.get(channel).unwrap().unwrap().details.format();
        let other = if home == bardo_domain::VideoFormat::Long {
            bardo_domain::VideoFormat::Short
        } else {
            bardo_domain::VideoFormat::Long
        };
        let long = app
            .create_channel(bardo_domain::ChannelDraft {
                name: "Space Archives Long".into(),
                format: other,
                ..bardo_domain::ChannelDraft::default()
            })
            .unwrap();
        let video = app.start_loose_video(draft(Some(long.id))).unwrap();
        app.move_loose_video(video.id, channel).unwrap();
        let overview = app.overview().unwrap();
        let shown = overview
            .videos
            .iter()
            .find(|shown| shown.id == video.id)
            .expect("the moved video is in progress");
        assert_eq!(shown.format, other);
    }

    #[test]
    fn a_version_of_a_moved_video_keeps_its_own_angle() {
        let h = Harness::new();
        let app = h.start();
        let channel = project(&app).channel.unwrap();
        let video = app.start_loose_video(draft(Some(channel))).unwrap();
        h.answer(SCRIPT.to_owned());
        done(
            &app,
            app.generate_script(video.id, BudgetConsent::Ask).unwrap(),
        );
        done(
            &app,
            app.generate_narration(video.id, crate::PicturesConsent::Ask, BudgetConsent::Ask)
                .unwrap(),
        );
        h.answer(plan_answer());
        done(
            &app,
            app.plan_scenes(video.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        done(
            &app,
            app.generate_scene_images(video.id, BudgetConsent::Ask)
                .unwrap(),
        );
        app.move_loose_video(video.id, channel).unwrap();
        let narrator = app
            .personas()
            .unwrap()
            .into_iter()
            .find(|p| p.details.name() == "Documentary Narrator (en-US)")
            .unwrap();
        let english = app
            .create_channel(bardo_domain::ChannelDraft {
                name: "Lost Signals".into(),
                niche: "space history".into(),
                language: ContentLanguage::English,
                country: Country::UnitedStates,
                default_persona: Some(narrator.id),
                ..bardo_domain::ChannelDraft::default()
            })
            .unwrap();
        h.answer(
            serde_json::json!({
                "title": "The Probe That Never Came Home",
                "sentences": [
                    "Once upon a time, in 1969, there was a probe.",
                    "It flew far away.",
                    "The signal vanished in March.",
                    "Nobody knows why.",
                ],
            })
            .to_string(),
        );
        done(
            &app,
            app.make_version(video.id, english.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let version = app.versions(video.id).unwrap().made[0].project.clone();
        let theme = app.themes.theme(version.theme.unwrap()).unwrap().unwrap();
        assert_eq!(theme.idea().angle(), "told by the engineers who lost it");
    }

    #[test]
    fn a_loose_video_does_not_move_into_an_archived_project() {
        let h = Harness::new();
        let app = h.start();
        let channel = project(&app).channel.unwrap();
        let video = app.start_loose_video(draft(None)).unwrap();
        let home = app.channels.get(channel).unwrap().unwrap().project;
        app.archive_project(home, false).unwrap();
        assert!(matches!(
            app.move_loose_video(video.id, channel),
            Err(LooseError::ProjectArchived)
        ));
        assert!(app.themes.project(video.id).unwrap().unwrap().is_loose());
    }

    #[test]
    fn a_loose_video_does_not_move_while_one_of_its_jobs_runs() {
        let h = Harness::new();
        let app = h.start();
        let channel = project(&app).channel.unwrap();
        let video = app.start_loose_video(draft(None)).unwrap();
        // The answer is held back: the script job is under way.
        h.answer(SCRIPT.to_owned());
        h.text.hold();
        let job = app.generate_script(video.id, BudgetConsent::Ask).unwrap();
        assert!(matches!(
            app.move_loose_video(video.id, channel),
            Err(LooseError::Busy)
        ));
        h.text.let_go();
        crate::scenes::tests::wait_done(&app, job);
    }

    #[test]
    fn no_project_stays_open_until_a_project_opens() {
        let h = Harness::new();
        let app = h.start();
        project(&app);
        let first = app.open_project().unwrap().unwrap();
        assert!(!app.loose_open());

        app.open_loose_videos().unwrap();
        assert!(app.loose_open());
        assert_eq!(app.open_project().unwrap(), None);
        drop(app);
        let app = h.start();
        assert!(app.loose_open(), "kept from the last start");

        app.switch_project(first.id).unwrap();
        assert!(!app.loose_open());
        assert_eq!(app.open_project().unwrap().map(|p| p.id), Some(first.id));
    }

    #[test]
    fn a_loose_video_picks_among_the_active_projects_channels() {
        let h = Harness::new();
        let app = h.start();
        let channel = project(&app).channel.unwrap();
        let choices = app.loose_channel_choices().unwrap();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].0, channel);
        assert!(
            choices[0].1.ends_with("› Space Archives 1"),
            "{}",
            choices[0].1
        );
        let home = app.channels.get(channel).unwrap().unwrap().project;
        app.archive_project(home, false).unwrap();
        assert!(app.loose_channel_choices().unwrap().is_empty());
    }

    #[test]
    fn a_loose_video_folder_is_under_the_global_folder() {
        let h = Harness::new();
        let app = h.start();
        assert!(
            app.loose_videos_folder()
                .ends_with(std::path::Path::new(GLOBAL_DIR).join(LOOSE_VIDEOS_DIR))
        );
    }

    #[test]
    fn an_idea_the_user_writes_joins_the_channel_s_ideas() {
        let h = Harness::new();
        let app = h.start();
        let channel = project(&app).channel.unwrap();
        let theme = app
            .write_theme(
                channel,
                "",
                "Voyager's golden record",
                "who chose the songs",
            )
            .unwrap();
        assert_eq!(theme.idea().title(), "Voyager's golden record");
        assert_eq!(theme.idea().angle(), "who chose the songs");
        assert_eq!(theme.niche.label(), "space history");
        let view = app.themes(channel, None).unwrap();
        assert!(view.themes.iter().any(|t| t.id == theme.id));
        assert!(matches!(
            app.write_theme(channel, "", "", ""),
            Err(crate::ThemeError::Invalid(_))
        ));
    }
}
