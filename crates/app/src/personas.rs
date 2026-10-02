//! Persona use cases (PRD stories 12-16): the profile's narrator library.
//! Personas are created, edited and duplicated here; their voice is picked
//! from the user's ElevenLabs voices. A persona is shared by every channel
//! that uses it as default, so saving changes to one asks for those
//! channels to be confirmed first.

use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, Channel, ChannelId, Persona, PersonaDetails, PersonaDraft, PersonaFieldError,
    PersonaId, Provider, ProviderFailure, Redactor, RepositoryError, Voice, VoiceLibrary, VoiceRef,
};

use crate::{AppError, Bardo, ProviderKeyError, Text};

#[derive(Debug, thiserror::Error)]
pub enum PersonaError {
    /// The draft breaks one or more field rules; the form shows each one.
    #[error("invalid persona: {0:?}")]
    Invalid(Vec<PersonaFieldError>),
    #[error("a persona with this name already exists")]
    NameTaken,
    #[error("persona not found")]
    NotFound,
    /// The persona is the default of these channels, and the save did not
    /// confirm all of them. Nothing was saved: show them, then save again
    /// with them confirmed.
    #[error("the persona is used by {} channel(s)", .0.len())]
    UsedByChannels(Vec<Channel>),
    /// Listing voices needs the ElevenLabs key.
    #[error("no ElevenLabs key saved")]
    MissingKey,
    #[error("could not read the ElevenLabs key")]
    KeyUnreadable,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl PersonaError {
    /// Message for the whole form. Field errors are shown next to their
    /// fields, and the channel list in its own panel.
    pub fn form_message(&self) -> Option<Text> {
        match self {
            PersonaError::Invalid(_) | PersonaError::UsedByChannels(_) => None,
            PersonaError::NameTaken => Some(Text::PersonaNameTaken),
            PersonaError::NotFound => Some(Text::PersonaNotFound),
            PersonaError::MissingKey => Some(Text::VoicesMissingKey),
            PersonaError::KeyUnreadable => Some(Text::ProviderKeyStoreFailed),
            PersonaError::Repository(_) => Some(Text::PersonaNotSaved),
        }
    }

    pub fn field_errors(&self) -> &[PersonaFieldError] {
        match self {
            PersonaError::Invalid(errors) => errors,
            _ => &[],
        }
    }
}

/// Whether a voice is among the account's voices, as far as the last
/// listing knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceStatus {
    /// No successful listing this session.
    Unknown,
    Available,
    /// Not in the account: generation with it would fail.
    Missing,
}

/// A voice listing ready to run. `run` calls the provider and blocks, so
/// the UI runs it on a background thread and hands the result to
/// `Bardo::record_voice_list`.
pub struct VoiceListing {
    key: ApiKey,
    library: Arc<dyn VoiceLibrary>,
    redactor: Redactor,
}

impl VoiceListing {
    pub fn run(self) -> VoiceList {
        let voices = self.library.voices(&self.key).map_err(|mut failure| {
            failure.detail = self.redactor.redact(&failure.detail);
            tracing::warn!(
                kind = ?failure.kind,
                detail = %failure.detail,
                "could not list the ElevenLabs voices"
            );
            failure
        });
        if let Ok(voices) = &voices {
            tracing::info!(count = voices.len(), "listed the ElevenLabs voices");
        }
        VoiceList {
            voices,
            listed_at: SystemTime::now(),
        }
    }
}

impl std::fmt::Debug for VoiceListing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceListing").finish_non_exhaustive()
    }
}

/// How a voice listing ended: the account's voices in picker order, or
/// why there are none (already redacted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceList {
    pub voices: Result<Vec<Voice>, ProviderFailure>,
    pub listed_at: SystemTime,
}

impl Bardo {
    /// The profile's personas, ordered by name.
    pub fn personas(&self) -> Result<Vec<Persona>, AppError> {
        Ok(self.personas.list(self.profile.id)?)
    }

    pub(crate) fn own_persona(&self, id: PersonaId) -> Result<Persona, PersonaError> {
        self.personas
            .get(id)?
            .filter(|persona| persona.owner == self.profile.id)
            .ok_or(PersonaError::NotFound)
    }

    /// The channels that use the persona as their default, by name.
    pub fn persona_usage(&self, id: PersonaId) -> Result<Vec<Channel>, PersonaError> {
        let persona = self.own_persona(id)?;
        Ok(self
            .channels
            .list(self.profile.id)?
            .into_iter()
            .filter(|channel| channel.details.default_persona() == Some(persona.id))
            .collect())
    }

    pub fn create_persona(&self, draft: PersonaDraft) -> Result<Persona, PersonaError> {
        let details = PersonaDetails::validate(draft).map_err(PersonaError::Invalid)?;
        self.ensure_persona_name_free(details.name(), None)?;
        let persona = Persona::new(self.profile.id, details);
        self.personas.save(&persona)?;
        Ok(persona)
    }

    /// Saves changes to a persona. When channels use it as their default,
    /// `confirmed` must name every one of them (the user saw the list);
    /// otherwise nothing is saved and the error carries the channels.
    /// Saving without changes needs no confirmation.
    pub fn update_persona(
        &self,
        id: PersonaId,
        draft: PersonaDraft,
        confirmed: &[ChannelId],
    ) -> Result<Persona, PersonaError> {
        let mut persona = self.own_persona(id)?;
        let details = PersonaDetails::validate(draft).map_err(PersonaError::Invalid)?;
        if details == persona.details {
            return Ok(persona);
        }
        self.ensure_persona_name_free(details.name(), Some(id))?;
        let usage = self.persona_usage(id)?;
        if usage.iter().any(|channel| !confirmed.contains(&channel.id)) {
            return Err(PersonaError::UsedByChannels(usage));
        }
        persona.details = details;
        self.personas.save(&persona)?;
        Ok(persona)
    }

    /// A new persona with the same voice, tone, style and presets, named
    /// as a copy, to vary without touching the original.
    pub fn duplicate_persona(&self, id: PersonaId) -> Result<Persona, PersonaError> {
        let original = self.own_persona(id)?;
        let taken = self.personas.list(self.profile.id)?;
        let name = original
            .details
            .copy_name(&self.text(Text::PersonaCopySuffix), |name| {
                taken.iter().any(|other| other.details.same_name(name))
            });
        let details = PersonaDetails::validate(PersonaDraft {
            name,
            ..PersonaDraft::from(&original.details)
        })
        .map_err(PersonaError::Invalid)?;
        let copy = Persona::new(self.profile.id, details);
        self.personas.save(&copy)?;
        Ok(copy)
    }

    fn ensure_persona_name_free(
        &self,
        name: &str,
        editing: Option<PersonaId>,
    ) -> Result<(), PersonaError> {
        let taken = self
            .personas
            .list(self.profile.id)?
            .iter()
            .any(|other| Some(other.id) != editing && other.details.same_name(name));
        if taken {
            Err(PersonaError::NameTaken)
        } else {
            Ok(())
        }
    }

    /// Prepares a listing of the account's ElevenLabs voices with the saved
    /// key, read fresh from the secret store.
    pub fn voice_listing(&self) -> Result<VoiceListing, PersonaError> {
        let key = self
            .provider_keys
            .read(self.profile.id, Provider::ElevenLabs)
            .map_err(|error| match error {
                ProviderKeyError::NotSet => PersonaError::MissingKey,
                _ => PersonaError::KeyUnreadable,
            })?;
        Ok(VoiceListing {
            key,
            library: Arc::clone(&self.voices),
            redactor: self.provider_keys.redactor().clone(),
        })
    }

    /// Keeps a finished listing for the voice picker.
    pub fn record_voice_list(&mut self, list: VoiceList) {
        self.voice_list = Some(list);
    }

    /// The last listing of this session, if any.
    pub fn voice_list(&self) -> Option<&VoiceList> {
        self.voice_list.as_ref()
    }

    /// Whether `voice` is in the account, by the last successful listing.
    pub fn voice_status(&self, voice: &VoiceRef) -> VoiceStatus {
        match self.voice_list.as_ref().map(|list| &list.voices) {
            Some(Ok(voices)) => {
                if voices.iter().any(|v| v.reference.same_voice(voice)) {
                    VoiceStatus::Available
                } else {
                    VoiceStatus::Missing
                }
            }
            _ => VoiceStatus::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        ChannelDraft, GenerationPresets, PersonaRepository, ProviderFailureKind, VoiceCategory,
    };
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::testing::{self, FakeVoiceLibrary};
    use crate::{ChannelError, Providers, Repositories};

    const ELEVENLABS_KEY: &str = "sk_test_elevenlabs_key_0001";

    struct Harness {
        db: Arc<Database>,
        secrets: Arc<MemorySecretStore>,
        voices: Arc<FakeVoiceLibrary>,
    }

    impl Harness {
        fn new() -> Self {
            Self::with_db(Arc::new(Database::open_in_memory().unwrap()))
        }

        fn with_db(db: Arc<Database>) -> Self {
            Self {
                db,
                secrets: Arc::default(),
                voices: Arc::default(),
            }
        }

        fn start(&self) -> Bardo {
            let providers = Providers {
                voices: Arc::clone(&self.voices) as _,
                ..testing::providers()
            };
            Bardo::start(
                Repositories::shared(Arc::clone(&self.db), Arc::clone(&self.secrets) as _),
                providers,
                Some("en-US"),
            )
            .unwrap()
        }

        fn start_with_key(&self) -> Bardo {
            let mut app = self.start();
            app.save_provider_key(Provider::ElevenLabs, ELEVENLABS_KEY)
                .unwrap();
            app
        }
    }

    fn voice(id: &str, name: &str) -> VoiceRef {
        VoiceRef::elevenlabs(id, name).unwrap()
    }

    fn listed(id: &str, name: &str, category: VoiceCategory) -> Voice {
        Voice {
            reference: voice(id, name),
            category,
            description: String::new(),
            labels: vec![],
        }
    }

    fn draft(name: &str) -> PersonaDraft {
        PersonaDraft {
            name: name.into(),
            voice: Some(voice("myClone01", "My voice")),
            tone: "Warm.".into(),
            script_style: "Short sentences.".into(),
            presets: GenerationPresets::default(),
        }
    }

    fn names(personas: &[Persona]) -> Vec<String> {
        personas
            .iter()
            .map(|p| p.details.name().to_owned())
            .collect()
    }

    fn channel_with(app: &Bardo, name: &str, persona: Option<PersonaId>) -> Channel {
        app.create_channel(ChannelDraft {
            name: name.into(),
            default_persona: persona,
            ..ChannelDraft::default()
        })
        .unwrap()
    }

    #[test]
    fn first_start_seeds_the_four_defaults_once() {
        let h = Harness::new();
        let app = h.start();
        let first = app.personas().unwrap();
        assert_eq!(
            names(&first),
            [
                "Contador de Histórias Dramático (pt-BR)",
                "Documentary Narrator (en-US)",
                "Dramatic Storyteller (en-US)",
                "Narradora Documental (pt-BR)",
            ]
        );
        assert!(first.iter().all(|p| p.owner == app.profile().id));

        let restarted = h.start();
        assert_eq!(restarted.personas().unwrap(), first, "not seeded twice");
    }

    #[test]
    fn defaults_can_be_duplicated_and_edited() {
        let app = Harness::new().start();
        let original = app.personas().unwrap().remove(1);
        let copy = app.duplicate_persona(original.id).unwrap();
        assert_ne!(copy.id, original.id);
        assert_eq!(copy.details.name(), "Documentary Narrator (en-US) (copy)");
        assert_eq!(copy.details.voice(), original.details.voice());
        assert_eq!(copy.details.tone(), original.details.tone());
        assert_eq!(copy.details.presets(), original.details.presets());

        let again = app.duplicate_persona(original.id).unwrap();
        assert_eq!(
            again.details.name(),
            "Documentary Narrator (en-US) (copy 2)"
        );

        let edited = app
            .update_persona(
                original.id,
                PersonaDraft {
                    tone: "Even calmer.".into(),
                    ..PersonaDraft::from(&original.details)
                },
                &[],
            )
            .unwrap();
        assert_eq!(edited.details.tone(), "Even calmer.");
        assert_eq!(app.personas().unwrap().len(), 6);
        assert_eq!(
            app.personas
                .get(original.id)
                .unwrap()
                .unwrap()
                .details
                .tone(),
            "Even calmer."
        );
    }

    #[test]
    fn copies_are_named_in_the_interface_language() {
        let mut app = Harness::new().start();
        app.set_ui_language(bardo_domain::UiLanguage::PtBr).unwrap();
        let original = app.personas().unwrap().remove(0);
        let copy = app.duplicate_persona(original.id).unwrap();
        assert_eq!(
            copy.details.name(),
            "Contador de Histórias Dramático (pt-BR) (cópia)"
        );
    }

    #[test]
    fn created_persona_is_listed_with_its_voice_reference() {
        let app = Harness::new().start();
        let created = app.create_persona(draft("My Narrator")).unwrap();
        assert_eq!(created.owner, app.profile().id);
        assert_eq!(created.details.voice(), &voice("myClone01", "My voice"));
        assert!(app.personas().unwrap().contains(&created));
    }

    #[test]
    fn invalid_drafts_and_taken_names_save_nothing() {
        let app = Harness::new().start();
        let before = app.personas().unwrap();

        let error = app
            .create_persona(PersonaDraft {
                voice: None,
                ..draft(" ")
            })
            .unwrap_err();
        assert_eq!(
            error.field_errors(),
            [
                PersonaFieldError::NameRequired,
                PersonaFieldError::VoiceRequired
            ]
        );
        assert_eq!(error.form_message(), None);

        let error = app
            .create_persona(draft(" documentary NARRATOR (en-us) "))
            .unwrap_err();
        assert!(matches!(error, PersonaError::NameTaken));
        assert_eq!(error.form_message(), Some(Text::PersonaNameTaken));
        assert_eq!(app.personas().unwrap(), before);
    }

    #[test]
    fn editing_cannot_take_another_personas_name_but_can_keep_its_own() {
        let app = Harness::new().start();
        let mine = app.create_persona(draft("Mine")).unwrap();
        let error = app
            .update_persona(mine.id, draft("Dramatic Storyteller (en-US)"), &[])
            .unwrap_err();
        assert!(matches!(error, PersonaError::NameTaken));
        let renamed = app.update_persona(mine.id, draft("MINE"), &[]).unwrap();
        assert_eq!(renamed.details.name(), "MINE");
    }

    #[test]
    fn a_channel_can_set_its_default_persona() {
        let app = Harness::new().start();
        let narrator = app.personas().unwrap().remove(1);
        let channel = channel_with(&app, "Space Archives", Some(narrator.id));
        assert_eq!(channel.details.default_persona(), Some(narrator.id));
        assert_eq!(app.channels().unwrap(), std::slice::from_ref(&channel));

        let other = app.personas().unwrap().remove(2);
        let changed = app
            .update_channel(
                channel.id,
                ChannelDraft {
                    default_persona: Some(other.id),
                    ..ChannelDraft::from(&channel.details)
                },
            )
            .unwrap();
        assert_eq!(changed.details.default_persona(), Some(other.id));
    }

    #[test]
    fn a_channel_cannot_use_an_unknown_or_foreign_persona() {
        let h = Harness::new();
        let app = h.start();
        let stranger = bardo_domain::UserProfile::new(bardo_domain::UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&*h.db, &stranger).unwrap();
        let theirs = Persona::defaults(stranger.id).remove(0);
        PersonaRepository::save(&*h.db, &theirs).unwrap();

        for persona in [PersonaId::new(), theirs.id] {
            let error = app
                .create_channel(ChannelDraft {
                    name: "Space Archives".into(),
                    default_persona: Some(persona),
                    ..ChannelDraft::default()
                })
                .unwrap_err();
            assert!(matches!(error, ChannelError::PersonaNotFound));
            assert_eq!(error.form_message(), Some(Text::ChannelPersonaNotFound));
        }
        assert!(app.channels().unwrap().is_empty());
        assert!(matches!(
            app.duplicate_persona(theirs.id),
            Err(PersonaError::NotFound)
        ));
    }

    #[test]
    fn editing_a_used_persona_shows_its_channels_before_saving() {
        let app = Harness::new().start();
        let narrator = app.personas().unwrap().remove(1);
        let space = channel_with(&app, "Space Archives", Some(narrator.id));
        let sea = channel_with(&app, "Deep Sea", Some(narrator.id));
        channel_with(&app, "Unrelated", None);

        let usage = app.persona_usage(narrator.id).unwrap();
        assert_eq!(
            usage.iter().map(|c| c.id).collect::<Vec<_>>(),
            [sea.id, space.id]
        );

        let changed = PersonaDraft {
            tone: "Louder.".into(),
            ..PersonaDraft::from(&narrator.details)
        };
        let error = app
            .update_persona(narrator.id, changed.clone(), &[])
            .unwrap_err();
        let PersonaError::UsedByChannels(channels) = &error else {
            panic!("expected the channels, got {error:?}");
        };
        assert_eq!(channels, &usage);
        assert_eq!(error.form_message(), None);
        assert_eq!(
            app.personas.get(narrator.id).unwrap().unwrap(),
            narrator,
            "nothing saved before confirming"
        );

        // Confirming only some of them is not enough.
        assert!(matches!(
            app.update_persona(narrator.id, changed.clone(), &[space.id]),
            Err(PersonaError::UsedByChannels(_))
        ));

        let saved = app
            .update_persona(narrator.id, changed, &[space.id, sea.id])
            .unwrap();
        assert_eq!(saved.details.tone(), "Louder.");
    }

    #[test]
    fn saving_an_unchanged_used_persona_needs_no_confirmation() {
        let app = Harness::new().start();
        let narrator = app.personas().unwrap().remove(1);
        channel_with(&app, "Space Archives", Some(narrator.id));
        let saved = app
            .update_persona(narrator.id, PersonaDraft::from(&narrator.details), &[])
            .unwrap();
        assert_eq!(saved, narrator);
    }

    #[test]
    fn invalid_edits_report_fields_before_asking_to_confirm() {
        let app = Harness::new().start();
        let narrator = app.personas().unwrap().remove(1);
        channel_with(&app, "Space Archives", Some(narrator.id));
        let error = app
            .update_persona(
                narrator.id,
                PersonaDraft {
                    name: String::new(),
                    ..PersonaDraft::from(&narrator.details)
                },
                &[],
            )
            .unwrap_err();
        assert_eq!(error.field_errors(), [PersonaFieldError::NameRequired]);
    }

    #[test]
    fn the_voice_picker_lists_the_accounts_voices() {
        let h = Harness::new();
        *h.voices.voices.lock().unwrap() = vec![
            listed("myClone01", "My voice", VoiceCategory::Cloned),
            listed("FrS6cKLB1wg4WYgPa9GW", "Wyatt", VoiceCategory::Default),
        ];
        let mut app = h.start_with_key();
        assert!(app.voice_list().is_none());
        let wyatt = voice("FrS6cKLB1wg4WYgPa9GW", "Wyatt");
        assert_eq!(app.voice_status(&wyatt), VoiceStatus::Unknown);

        let list = app.voice_listing().unwrap().run();
        app.record_voice_list(list);

        assert_eq!(*h.voices.calls.lock().unwrap(), [ELEVENLABS_KEY]);
        let voices = app.voice_list().unwrap().voices.as_ref().unwrap();
        assert_eq!(voices.len(), 2);
        assert_eq!(app.voice_status(&wyatt), VoiceStatus::Available);
        assert_eq!(
            app.voice_status(&voice("someoneElse", "Gone")),
            VoiceStatus::Missing
        );
    }

    #[test]
    fn listing_voices_needs_the_elevenlabs_key() {
        let h = Harness::new();
        let app = h.start();
        let error = app.voice_listing().unwrap_err();
        assert!(matches!(error, PersonaError::MissingKey));
        assert_eq!(error.form_message(), Some(Text::VoicesMissingKey));
        assert!(h.voices.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_listing_is_kept_redacted_and_leaves_status_unknown() {
        let h = Harness::new();
        *h.voices.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            format!("bad key {ELEVENLABS_KEY}"),
        ));
        let mut app = h.start_with_key();
        let list = app.voice_listing().unwrap().run();
        app.record_voice_list(list);

        let failure = app.voice_list().unwrap().voices.clone().unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::Rejected);
        assert!(
            !failure.detail.contains(ELEVENLABS_KEY),
            "{}",
            failure.detail
        );
        assert_eq!(
            app.voice_status(&voice("myClone01", "x")),
            VoiceStatus::Unknown
        );
    }

    #[test]
    fn a_persona_never_stores_the_key_or_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let h = Harness::with_db(Arc::new(Database::open(&path).unwrap()));
        *h.voices.voices.lock().unwrap() =
            vec![listed("myClone01", "My voice", VoiceCategory::Cloned)];
        let mut app = h.start_with_key();
        let list = app.voice_listing().unwrap().run();
        app.record_voice_list(list);
        let picked = app.voice_list().unwrap().voices.as_ref().unwrap()[0]
            .reference
            .clone();
        app.create_persona(PersonaDraft {
            voice: Some(picked),
            ..draft("Cloned")
        })
        .unwrap();

        // Everything SQLite wrote: the database file and its write-ahead log.
        let mut written = Vec::new();
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            written.extend(std::fs::read(entry.unwrap().path()).unwrap());
        }
        let contains = |needle: &str| {
            written
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
        };
        assert!(contains("myClone01"), "the voice reference is stored");
        assert!(!contains(ELEVENLABS_KEY), "the key is not");
    }

    #[test]
    fn personas_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let h = Harness::with_db(Arc::new(Database::open(&path).unwrap()));
        let created = h.start().create_persona(draft("Mine")).unwrap();

        let reopened = Harness::with_db(Arc::new(Database::open(&path).unwrap()));
        let app = reopened.start();
        assert!(app.personas().unwrap().contains(&created));
        assert_eq!(app.personas().unwrap().len(), 5);
    }
}
