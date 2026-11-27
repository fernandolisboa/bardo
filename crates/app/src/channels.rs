//! Channel use cases: list, create and edit the open project's channels,
//! each with an optional default persona from the profile's library and an
//! optional video model for its clips. Channel names stay unique across
//! the profile.

use bardo_domain::{
    Channel, ChannelDetails, ChannelDraft, ChannelFieldError, ChannelId, ProjectId, RepositoryError,
};

use crate::{AppError, Bardo, PersonaError, Text};

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// The draft breaks one or more field rules; the form shows each one.
    #[error("invalid channel: {0:?}")]
    Invalid(Vec<ChannelFieldError>),
    #[error("a channel with this name already exists")]
    NameTaken,
    #[error("channel not found")]
    NotFound,
    /// The chosen default persona is not one of the profile's.
    #[error("persona not found")]
    PersonaNotFound,
    /// The chosen video model is not one the providers offer.
    #[error("video model not offered")]
    ClipModelNotOffered,
    /// The chosen take model is not one the providers offer.
    #[error("take model not offered")]
    TakeModelNotOffered,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ChannelError {
    /// Message for the whole form. Field errors are shown next to their
    /// fields instead.
    pub fn form_message(&self) -> Option<Text> {
        match self {
            ChannelError::Invalid(_) => None,
            ChannelError::NameTaken => Some(Text::ChannelNameTaken),
            ChannelError::NotFound => Some(Text::ChannelNotFound),
            ChannelError::PersonaNotFound => Some(Text::ChannelPersonaNotFound),
            ChannelError::ClipModelNotOffered => Some(Text::ChannelClipModelNotOffered),
            ChannelError::TakeModelNotOffered => Some(Text::ChannelTakeModelNotOffered),
            ChannelError::Repository(_) => Some(Text::ChannelNotSaved),
        }
    }

    pub fn field_errors(&self) -> &[ChannelFieldError] {
        match self {
            ChannelError::Invalid(errors) => errors,
            _ => &[],
        }
    }
}

impl Bardo {
    /// The open project's channels, ordered by name: what every work
    /// screen lists. Empty while no project is open. `all_channels` spans
    /// the profile.
    pub fn channels(&self) -> Result<Vec<Channel>, AppError> {
        let Some(project) = self.open_project_id()? else {
            return Ok(Vec::new());
        };
        Ok(self
            .channels
            .list(self.profile.id)?
            .into_iter()
            .filter(|channel| channel.project == project)
            .collect())
    }

    /// `draft` with the video model its choice names today: a form or a
    /// settings file may still name one its provider has since retired.
    fn with_current_clip_model(&self, mut draft: ChannelDraft) -> ChannelDraft {
        draft.clip_model = draft.clip_model.map(|model| self.clip_model_now(&model));
        draft
    }

    /// Creates a channel in the open project (in a new one of the
    /// channel's name when none is open yet).
    pub fn create_channel(&self, draft: ChannelDraft) -> Result<Channel, ChannelError> {
        self.create_channel_in(None, draft)
    }

    /// Creates a channel in the project called `project`, or in the open
    /// one when `project` is `None`.
    pub(crate) fn create_channel_in(
        &self,
        project: Option<&str>,
        draft: ChannelDraft,
    ) -> Result<Channel, ChannelError> {
        let details = self.new_channel_details(draft)?;
        let project = match project {
            Some(name) => self.project_named(name),
            None => self.project_for_new_channel(details.name()),
        }
        .map_err(|error| match error {
            crate::ProjectError::Repository(error) => ChannelError::Repository(error),
            other => ChannelError::Repository(RepositoryError(other.to_string().into())),
        })?;
        self.save_new_channel(project, details)
    }

    /// A new channel in `project`, which the caller already found.
    pub(crate) fn create_channel_at(
        &self,
        project: ProjectId,
        draft: ChannelDraft,
    ) -> Result<Channel, ChannelError> {
        let details = self.new_channel_details(draft)?;
        self.save_new_channel(project, details)
    }

    fn new_channel_details(&self, draft: ChannelDraft) -> Result<ChannelDetails, ChannelError> {
        let draft = self.with_current_clip_model(draft);
        let details = ChannelDetails::validate(draft).map_err(ChannelError::Invalid)?;
        self.ensure_name_free(&details, None)?;
        self.ensure_offered_clip_model(&details)?;
        Ok(details)
    }

    fn save_new_channel(
        &self,
        project: ProjectId,
        details: ChannelDetails,
    ) -> Result<Channel, ChannelError> {
        self.ensure_own_persona(&details, project)?;
        let channel = Channel::new(self.profile.id, project, details);
        self.channels.save(&channel)?;
        Ok(channel)
    }

    pub fn update_channel(
        &self,
        id: ChannelId,
        draft: ChannelDraft,
    ) -> Result<Channel, ChannelError> {
        let mut channel = self
            .channels
            .get(id)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(ChannelError::NotFound)?;
        let draft = self.with_current_clip_model(draft);
        let details = ChannelDetails::validate(draft).map_err(ChannelError::Invalid)?;
        self.ensure_name_free(&details, Some(id))?;
        self.ensure_own_persona(&details, channel.project)?;
        self.ensure_offered_clip_model(&details)?;
        channel.details = details;
        self.channels.save(&channel)?;
        Ok(channel)
    }

    /// Fails unless the default narrator is the profile's, and global or
    /// `project`'s own.
    fn ensure_own_persona(
        &self,
        details: &ChannelDetails,
        project: ProjectId,
    ) -> Result<(), ChannelError> {
        let Some(id) = details.default_persona() else {
            return Ok(());
        };
        match self.own_persona(id) {
            Ok(persona) if crate::usable_in(persona.owner_project, Some(project)) => Ok(()),
            Ok(_) => Err(ChannelError::PersonaNotFound),
            Err(PersonaError::Repository(error)) => Err(ChannelError::Repository(error)),
            Err(_) => Err(ChannelError::PersonaNotFound),
        }
    }

    fn ensure_offered_clip_model(&self, details: &ChannelDetails) -> Result<(), ChannelError> {
        match details.clip_model() {
            Some(model) if !self.offers_clip_model(model) => Err(ChannelError::ClipModelNotOffered),
            _ => self.ensure_offered_take_model(details),
        }
    }

    fn ensure_offered_take_model(&self, details: &ChannelDetails) -> Result<(), ChannelError> {
        match details.take_model() {
            Some(model) if !self.offers_take_model(model) => Err(ChannelError::TakeModelNotOffered),
            _ => Ok(()),
        }
    }

    fn ensure_name_free(
        &self,
        details: &ChannelDetails,
        editing: Option<ChannelId>,
    ) -> Result<(), ChannelError> {
        let taken = self
            .channels
            .list(self.profile.id)?
            .iter()
            .any(|other| Some(other.id) != editing && other.details.same_name(details.name()));
        if taken {
            Err(ChannelError::NameTaken)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bardo_domain::{ChannelRepository, ContentLanguage, Country, ProfileId};
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::{Repositories, testing};

    fn start(db: &Arc<Database>) -> Bardo {
        let repositories = Repositories {
            backups: Arc::new(bardo_storage::MemoryBackups::default()),
            installers: Arc::new(bardo_storage::MemoryInstallers::default()),
            installer_runner: Arc::new(bardo_storage::MemoryInstallerRunner::default()),
            youtube_data: Arc::new(crate::testing::FakeYouTubeData::default()),
            profiles: Box::new(Arc::clone(db)),
            tours: Box::new(Arc::clone(db)),
            agent_task: Arc::new(bardo_storage::MemoryAgentTask::default()),
            channels: Arc::clone(db) as _,
            projects: Arc::clone(db) as _,
            jobs: Arc::clone(db) as _,
            themes: Arc::clone(db) as _,
            templates: Arc::clone(db) as _,
            scripts: Arc::clone(db) as _,
            personas: Arc::clone(db) as _,
            products: Arc::clone(db) as _,
            world: Arc::clone(db) as _,
            world_files: Arc::new(bardo_storage::MemoryWorldFiles::default()),
            ads: Arc::clone(db) as _,
            end_cards: Arc::clone(db) as _,
            ad_exports: Arc::clone(db) as _,
            narrations: Arc::clone(db) as _,
            captures: Arc::clone(db) as _,
            staging: Arc::clone(db) as _,
            scene_plans: Arc::clone(db) as _,
            timelines: Arc::clone(db) as _,
            media_assets: Arc::clone(db) as _,
            music_prompts: Arc::clone(db) as _,
            music_tracks: Arc::clone(db) as _,
            thumbnails: Arc::clone(db) as _,
            chapters: Arc::clone(db) as _,
            network_accounts: Arc::clone(db) as _,
            connections: Arc::clone(db) as _,
            renders: Arc::clone(db) as _,
            exports: Arc::clone(db) as _,
            publications: Arc::clone(db) as _,
            cut_suggestions: Arc::clone(db) as _,
            localizations: Arc::clone(db) as _,
            export_files: Arc::new(bardo_storage::MemoryExportFiles::default()),
            costs: Arc::clone(db) as _,
            files: Arc::new(bardo_storage::MemoryProjectFiles::default()),
            folders: Arc::clone(db) as _,
            project_folders: bardo_storage::MemoryProjectFolders::for_database(db),
            removals: Arc::clone(db) as _,
            trash: Arc::new(bardo_storage::MemoryTrash::default()),
            places: crate::test_places(),
            voice_samples: Arc::new(bardo_storage::MemoryVoiceSamples::default()),
            research: Arc::clone(db) as _,
            secrets: Arc::new(MemorySecretStore::default()),
            connection_secrets: Arc::new(MemorySecretStore::default()),
        };
        Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap()
    }

    fn app() -> Bardo {
        start(&Arc::new(Database::open_in_memory().unwrap()))
    }

    fn draft(name: &str) -> ChannelDraft {
        ChannelDraft {
            name: name.into(),
            niche: "space history".into(),
            themes: vec!["Apollo".into()],
            aesthetic_notes: "archival".into(),
            language: ContentLanguage::English,
            country: Country::UnitedStates,
            default_persona: None,
            clip_model: None,
            take_model: None,
            caption_style: Default::default(),
            caption_highlight: Default::default(),
            transition: Default::default(),
            format: Default::default(),
            motion: Default::default(),
            thumbnail_look: Default::default(),
            production: Default::default(),
            style: Default::default(),
            clip_fit: Default::default(),
            stretch_limit: Default::default(),
        }
    }

    #[test]
    fn a_new_profile_has_no_channels() {
        assert!(app().channels().unwrap().is_empty());
    }

    #[test]
    fn created_channel_is_listed_and_owned_by_the_profile() {
        let app = app();
        let created = app.create_channel(draft("Space Archives")).unwrap();

        assert_eq!(created.owner, app.profile().id);
        assert_eq!(app.channels().unwrap(), [created]);
    }

    #[test]
    fn invalid_draft_reports_field_errors_and_saves_nothing() {
        let app = app();
        let error = app.create_channel(draft("  ")).unwrap_err();

        assert_eq!(error.field_errors(), [ChannelFieldError::NameRequired]);
        assert_eq!(error.form_message(), None);
        assert!(app.channels().unwrap().is_empty());
    }

    #[test]
    fn names_must_be_unique_ignoring_case() {
        let app = app();
        app.create_channel(draft("Space Archives")).unwrap();

        let error = app.create_channel(draft(" space ARCHIVES ")).unwrap_err();
        assert!(matches!(error, ChannelError::NameTaken));
        assert_eq!(error.form_message(), Some(Text::ChannelNameTaken));
        assert_eq!(app.channels().unwrap().len(), 1);
    }

    #[test]
    fn editing_changes_the_channel_in_place() {
        let app = app();
        let created = app.create_channel(draft("Space Archives")).unwrap();

        let updated = app
            .update_channel(
                created.id,
                ChannelDraft {
                    country: Country::Brazil,
                    language: ContentLanguage::Portuguese,
                    ..draft("Arquivos do Espaço")
                },
            )
            .unwrap();

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.details.name(), "Arquivos do Espaço");
        assert_eq!(updated.details.country(), Country::Brazil);
        assert_eq!(app.channels().unwrap(), [updated]);
    }

    #[test]
    fn a_channel_picks_one_of_the_offered_video_models() {
        use bardo_domain::{ClipModelRef, Provider};

        let app = app();
        let offered = app.clip_models()[1].id.clone();
        let created = app
            .create_channel(ChannelDraft {
                clip_model: Some(offered.clone()),
                ..draft("Space Archives")
            })
            .unwrap();
        assert_eq!(created.details.clip_model(), Some(&offered));

        let unknown = ClipModelRef::new(Provider::Higgsfield, "retired/model").unwrap();
        let error = app
            .update_channel(
                created.id,
                ChannelDraft {
                    clip_model: Some(unknown),
                    ..draft("Space Archives")
                },
            )
            .unwrap_err();
        assert!(matches!(error, ChannelError::ClipModelNotOffered));
        assert_eq!(error.form_message(), Some(Text::ChannelClipModelNotOffered));
        assert_eq!(app.channels().unwrap(), [created]);
    }

    #[test]
    fn a_channel_can_keep_its_own_name_when_edited() {
        let app = app();
        let created = app.create_channel(draft("Space Archives")).unwrap();
        let updated = app
            .update_channel(created.id, draft("SPACE archives"))
            .unwrap();
        assert_eq!(updated.details.name(), "SPACE archives");
    }

    #[test]
    fn editing_cannot_take_another_channels_name() {
        let app = app();
        app.create_channel(draft("Space Archives")).unwrap();
        let other = app.create_channel(draft("Deep Sea")).unwrap();

        let error = app
            .update_channel(other.id, draft("Space Archives"))
            .unwrap_err();
        assert!(matches!(error, ChannelError::NameTaken));
    }

    #[test]
    fn editing_an_unknown_channel_is_not_found() {
        let error = app()
            .update_channel(ChannelId::new(), draft("Ghost"))
            .unwrap_err();
        assert!(matches!(error, ChannelError::NotFound));
        assert_eq!(error.form_message(), Some(Text::ChannelNotFound));
    }

    #[test]
    fn another_profiles_channel_cannot_be_edited() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let stranger = bardo_domain::UserProfile::new(bardo_domain::UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&*db, &stranger).unwrap();
        let project = bardo_domain::Project::new(
            stranger.id,
            bardo_domain::ProjectDetails::validate(bardo_domain::ProjectDraft {
                name: "Theirs".into(),
                ..Default::default()
            })
            .unwrap(),
            std::time::SystemTime::now(),
        );
        bardo_domain::ProjectRepository::save_project(&*db, &project).unwrap();
        let theirs = Channel::new(
            stranger.id,
            project.id,
            ChannelDetails::validate(draft("Theirs")).unwrap(),
        );
        ChannelRepository::save(&*db, &theirs).unwrap();

        assert!(matches!(
            app.update_channel(theirs.id, draft("Mine now")),
            Err(ChannelError::NotFound)
        ));
        assert!(app.channels().unwrap().is_empty());
    }

    #[test]
    fn channels_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let open = || Arc::new(Database::open(&path).unwrap());

        let created = start(&open())
            .create_channel(draft("Space Archives"))
            .unwrap();

        let restarted = start(&open());
        assert_eq!(restarted.channels().unwrap(), [created]);
    }

    #[test]
    fn storage_failure_is_reported_as_not_saved() {
        struct Broken;
        impl ChannelRepository for Broken {
            fn list(&self, _: ProfileId) -> Result<Vec<Channel>, RepositoryError> {
                Ok(vec![])
            }
            fn get(&self, _: ChannelId) -> Result<Option<Channel>, RepositoryError> {
                Ok(None)
            }
            fn save(&self, _: &Channel) -> Result<(), RepositoryError> {
                Err(RepositoryError("disk full".into()))
            }
        }

        let db = Arc::new(Database::open_in_memory().unwrap());
        let repositories = Repositories {
            backups: Arc::new(bardo_storage::MemoryBackups::default()),
            installers: Arc::new(bardo_storage::MemoryInstallers::default()),
            installer_runner: Arc::new(bardo_storage::MemoryInstallerRunner::default()),
            youtube_data: Arc::new(crate::testing::FakeYouTubeData::default()),
            profiles: Box::new(Arc::clone(&db)),
            tours: Box::new(Arc::clone(&db)),
            agent_task: Arc::new(bardo_storage::MemoryAgentTask::default()),
            channels: Arc::new(Broken),
            projects: Arc::clone(&db) as _,
            jobs: Arc::clone(&db) as _,
            themes: Arc::clone(&db) as _,
            templates: Arc::clone(&db) as _,
            scripts: Arc::clone(&db) as _,
            personas: Arc::clone(&db) as _,
            products: Arc::clone(&db) as _,
            world: Arc::clone(&db) as _,
            world_files: Arc::new(bardo_storage::MemoryWorldFiles::default()),
            ads: Arc::clone(&db) as _,
            end_cards: Arc::clone(&db) as _,
            ad_exports: Arc::clone(&db) as _,
            narrations: Arc::clone(&db) as _,
            captures: Arc::clone(&db) as _,
            staging: Arc::clone(&db) as _,
            scene_plans: Arc::clone(&db) as _,
            timelines: Arc::clone(&db) as _,
            media_assets: Arc::clone(&db) as _,
            music_prompts: Arc::clone(&db) as _,
            music_tracks: Arc::clone(&db) as _,
            thumbnails: Arc::clone(&db) as _,
            chapters: Arc::clone(&db) as _,
            network_accounts: Arc::clone(&db) as _,
            connections: Arc::clone(&db) as _,
            renders: Arc::clone(&db) as _,
            exports: Arc::clone(&db) as _,
            publications: Arc::clone(&db) as _,
            cut_suggestions: Arc::clone(&db) as _,
            localizations: Arc::clone(&db) as _,
            export_files: Arc::new(bardo_storage::MemoryExportFiles::default()),
            costs: Arc::clone(&db) as _,
            files: Arc::new(bardo_storage::MemoryProjectFiles::default()),
            folders: Arc::clone(&db) as _,
            project_folders: bardo_storage::MemoryProjectFolders::for_database(&db),
            removals: Arc::clone(&db) as _,
            trash: Arc::new(bardo_storage::MemoryTrash::default()),
            places: crate::test_places(),
            voice_samples: Arc::new(bardo_storage::MemoryVoiceSamples::default()),
            research: db,
            secrets: Arc::new(MemorySecretStore::default()),
            connection_secrets: Arc::new(MemorySecretStore::default()),
        };
        let app = Bardo::start(repositories, testing::providers(), None).unwrap();
        let error = app.create_channel(draft("Space Archives")).unwrap_err();
        assert_eq!(error.form_message(), Some(Text::ChannelNotSaved));
    }
}
