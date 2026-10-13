//! Channel use cases: list, create and edit the profile's channels, each
//! with an optional default persona from the profile's library and an
//! optional video model for its clips.

use bardo_domain::{
    Channel, ChannelDetails, ChannelDraft, ChannelFieldError, ChannelId, RepositoryError,
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
    /// The profile's channels, ordered by name.
    pub fn channels(&self) -> Result<Vec<Channel>, AppError> {
        Ok(self.channels.list(self.profile.id)?)
    }

    pub fn create_channel(&self, draft: ChannelDraft) -> Result<Channel, ChannelError> {
        let details = ChannelDetails::validate(draft).map_err(ChannelError::Invalid)?;
        self.ensure_name_free(&details, None)?;
        self.ensure_own_persona(&details)?;
        self.ensure_offered_clip_model(&details)?;
        let channel = Channel::new(self.profile.id, details);
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
        let details = ChannelDetails::validate(draft).map_err(ChannelError::Invalid)?;
        self.ensure_name_free(&details, Some(id))?;
        self.ensure_own_persona(&details)?;
        self.ensure_offered_clip_model(&details)?;
        channel.details = details;
        self.channels.save(&channel)?;
        Ok(channel)
    }

    fn ensure_own_persona(&self, details: &ChannelDetails) -> Result<(), ChannelError> {
        let Some(id) = details.default_persona() else {
            return Ok(());
        };
        match self.own_persona(id) {
            Ok(_) => Ok(()),
            Err(PersonaError::Repository(error)) => Err(ChannelError::Repository(error)),
            Err(_) => Err(ChannelError::PersonaNotFound),
        }
    }

    fn ensure_offered_clip_model(&self, details: &ChannelDetails) -> Result<(), ChannelError> {
        match details.clip_model() {
            Some(model) if !self.offers_clip_model(model) => Err(ChannelError::ClipModelNotOffered),
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
            profiles: Box::new(Arc::clone(db)),
            tours: Box::new(Arc::clone(db)),
            agent_task: Arc::new(bardo_storage::MemoryAgentTask::default()),
            channels: Box::new(Arc::clone(db)),
            jobs: Arc::clone(db) as _,
            themes: Arc::clone(db) as _,
            templates: Arc::clone(db) as _,
            scripts: Arc::clone(db) as _,
            personas: Arc::clone(db) as _,
            narrations: Arc::clone(db) as _,
            scene_plans: Arc::clone(db) as _,
            timelines: Arc::clone(db) as _,
            media_assets: Arc::clone(db) as _,
            music_prompts: Arc::clone(db) as _,
            network_accounts: Arc::clone(db) as _,
            connections: Arc::clone(db) as _,
            renders: Arc::clone(db) as _,
            exports: Arc::clone(db) as _,
            publications: Arc::clone(db) as _,
            cut_suggestions: Arc::clone(db) as _,
            export_files: Arc::new(bardo_storage::MemoryExportFiles::default()),
            costs: Arc::clone(db) as _,
            files: Arc::new(bardo_storage::MemoryProjectFiles::default()),
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
            caption_style: Default::default(),
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
        let theirs = Channel::new(
            stranger.id,
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
            profiles: Box::new(Arc::clone(&db)),
            tours: Box::new(Arc::clone(&db)),
            agent_task: Arc::new(bardo_storage::MemoryAgentTask::default()),
            channels: Box::new(Broken),
            jobs: Arc::clone(&db) as _,
            themes: Arc::clone(&db) as _,
            templates: Arc::clone(&db) as _,
            scripts: Arc::clone(&db) as _,
            personas: Arc::clone(&db) as _,
            narrations: Arc::clone(&db) as _,
            scene_plans: Arc::clone(&db) as _,
            timelines: Arc::clone(&db) as _,
            media_assets: Arc::clone(&db) as _,
            music_prompts: Arc::clone(&db) as _,
            network_accounts: Arc::clone(&db) as _,
            connections: Arc::clone(&db) as _,
            renders: Arc::clone(&db) as _,
            exports: Arc::clone(&db) as _,
            publications: Arc::clone(&db) as _,
            cut_suggestions: Arc::clone(&db) as _,
            export_files: Arc::new(bardo_storage::MemoryExportFiles::default()),
            costs: Arc::clone(&db) as _,
            files: Arc::new(bardo_storage::MemoryProjectFiles::default()),
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
