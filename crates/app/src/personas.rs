//! Persona use cases (PRD stories 12-19): the profile's narrator library.
//! Personas are created, edited and duplicated here; their voice is picked
//! from the user's ElevenLabs voices. A persona is shared by every channel
//! that uses it as default, so saving changes to one asks for those
//! channels to be confirmed first.
//!
//! Personas travel between machines and profiles as packages
//! (`persona_package`). An imported persona's voice lives in someone
//! else's provider account, so it is flagged until the user's own voice
//! listing shows the voice, and a flagged persona cannot narrate. A video
//! project may also pick its own narrator instead of the channel's.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, Channel, ChannelId, Persona, PersonaDetails, PersonaDraft, PersonaFieldError,
    PersonaId, Provider, ProviderFailure, Redactor, RepositoryError, VideoProject, VideoProjectId,
    Voice, VoiceFlag, VoiceLibrary, VoiceRef,
};

use crate::persona_package::{self, PackageError};
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
    /// The package file could not be read (missing, no permission, too
    /// large to be a package).
    #[error("could not read the package file: {0}")]
    PackageUnreadable(String),
    #[error(transparent)]
    Package(#[from] PackageError),
    /// The package file could not be written where the user chose.
    #[error("could not write the package file: {0}")]
    ExportFailed(String),
    #[error("video project not found")]
    ProjectNotFound,
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
            PersonaError::PackageUnreadable(_) => Some(Text::PersonaPackageUnreadable),
            PersonaError::Package(PackageError::NotAPackage) => Some(Text::PersonaPackageNotOne),
            PersonaError::Package(PackageError::NewerVersion(_)) => Some(Text::PersonaPackageNewer),
            PersonaError::Package(PackageError::InvalidPersona) => {
                Some(Text::PersonaPackageInvalid)
            }
            PersonaError::ExportFailed(_) => Some(Text::PersonaExportFailed),
            PersonaError::ProjectNotFound => Some(Text::ProjectNotFound),
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

/// Where the export and import dialogs open: the user's documents, else
/// their home folder.
pub fn persona_package_folder() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
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
        // A flagged persona given another voice is judged on that voice.
        if persona.voice_flag.is_some() && !details.voice().same_voice(persona.details.voice()) {
            persona.voice_flag = VoiceFlag::from_lookup(self.voice_lookup(details.voice()));
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
        let copy = Persona {
            // Same voice, same flag.
            voice_flag: original.voice_flag,
            ..Persona::new(self.profile.id, details)
        };
        self.personas.save(&copy)?;
        Ok(copy)
    }

    /// The persona's package: what another machine or profile imports.
    /// It holds no keys, audio or ids, only the persona and its voice
    /// reference.
    pub fn export_persona(&self, id: PersonaId) -> Result<String, PersonaError> {
        Ok(persona_package::encode(&self.own_persona(id)?.details))
    }

    /// The file name the save dialog suggests for the persona's package.
    pub fn persona_package_name(&self, id: PersonaId) -> Result<String, PersonaError> {
        Ok(persona_package::file_name(&self.own_persona(id)?.details))
    }

    /// Writes the persona's package to `path`, which the user chose.
    pub fn export_persona_to(&self, id: PersonaId, path: &Path) -> Result<(), PersonaError> {
        let package = self.export_persona(id)?;
        std::fs::write(path, package).map_err(|error| {
            tracing::warn!(%error, "could not write a persona package");
            PersonaError::ExportFailed(error.to_string())
        })?;
        tracing::info!("exported a persona package");
        Ok(())
    }

    /// Adds the packaged persona to the profile's library as a new
    /// persona. A taken name gets an "imported" suffix. The voice is
    /// looked up in the last voice listing: listed clears it, missing
    /// flags it unavailable, and without a listing it stays unchecked
    /// until the next one (`record_voice_list`).
    pub fn import_persona(&self, package: &str) -> Result<Persona, PersonaError> {
        let details = persona_package::decode(package)?;
        let taken = self.personas.list(self.profile.id)?;
        let details = if taken
            .iter()
            .any(|other| other.details.same_name(details.name()))
        {
            let name = details.copy_name(&self.text(Text::PersonaImportedSuffix), |name| {
                taken.iter().any(|other| other.details.same_name(name))
            });
            PersonaDetails::validate(PersonaDraft {
                name,
                ..PersonaDraft::from(&details)
            })
            .map_err(PersonaError::Invalid)?
        } else {
            details
        };
        let persona = Persona {
            voice_flag: VoiceFlag::from_lookup(self.voice_lookup(details.voice())),
            ..Persona::new(self.profile.id, details)
        };
        self.personas.save(&persona)?;
        tracing::info!(flag = ?persona.voice_flag, "imported a persona package");
        Ok(persona)
    }

    /// Reads the package file at `path`, which the user chose, and
    /// imports it.
    pub fn import_persona_from(&self, path: &Path) -> Result<Persona, PersonaError> {
        let unreadable = |error: std::io::Error| {
            tracing::warn!(%error, "could not read a persona package");
            PersonaError::PackageUnreadable(error.to_string())
        };
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(unreadable)?
            // One byte past the limit tells "too large" from "at the limit".
            .take(persona_package::MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(unreadable)?;
        if bytes.len() as u64 > persona_package::MAX_BYTES {
            return Err(PackageError::NotAPackage.into());
        }
        let text = String::from_utf8(bytes).map_err(|_| PackageError::NotAPackage)?;
        self.import_persona(&text)
    }

    /// Sets the persona that narrates the project instead of the
    /// channel's default, or goes back to the channel's (`None`).
    pub fn set_project_persona(
        &self,
        project: VideoProjectId,
        persona: Option<PersonaId>,
    ) -> Result<VideoProject, PersonaError> {
        let mut project = self
            .themes
            .project(project)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(PersonaError::ProjectNotFound)?;
        if let Some(id) = persona {
            self.own_persona(id)?;
        }
        self.themes.set_project_persona(project.id, persona)?;
        project.persona = persona;
        Ok(project)
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

    /// Keeps a finished listing for the voice picker, and checks every
    /// flagged persona's voice against it: listed voices clear the flag,
    /// missing ones are flagged unavailable. A failed listing changes no
    /// flag.
    pub fn record_voice_list(&mut self, list: VoiceList) {
        self.voice_list = Some(list);
        if let Err(error) = self.recheck_flagged_voices() {
            tracing::warn!(%error, "could not update the personas' voice flags");
        }
    }

    fn recheck_flagged_voices(&self) -> Result<(), RepositoryError> {
        let changed: Vec<Persona> = self
            .personas
            .list(self.profile.id)?
            .into_iter()
            .filter(|persona| persona.voice_flag.is_some())
            .filter_map(|mut persona| {
                let listed = self.voice_lookup(persona.details.voice())?;
                let flag = VoiceFlag::from_lookup(Some(listed));
                (flag != persona.voice_flag).then(|| {
                    persona.voice_flag = flag;
                    persona
                })
            })
            .collect();
        if !changed.is_empty() {
            self.personas.insert_all(&changed)?;
        }
        Ok(())
    }

    /// Whether the last successful listing has `voice`; `None` without one.
    fn voice_lookup(&self, voice: &VoiceRef) -> Option<bool> {
        match self.voice_status(voice) {
            VoiceStatus::Available => Some(true),
            VoiceStatus::Missing => Some(false),
            VoiceStatus::Unknown => None,
        }
    }

    /// A new or removed voice key may be another account: its voices are
    /// unknown until listed again.
    pub(crate) fn forget_voice_list(&mut self, provider: Provider) {
        if VoiceRef::PROVIDERS.contains(&provider) {
            self.voice_list = None;
        }
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
        ChannelDraft, GenerationPresets, PersonaRepository, ProviderFailureKind, ThemeRepository,
        VoiceCategory,
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

    /// A second machine: its own database, profile and key.
    fn other_machine() -> Harness {
        Harness::new()
    }

    fn recorded(h: &Harness, app: &mut Bardo) {
        let list = app.voice_listing().unwrap().run();
        app.record_voice_list(list);
        assert!(!h.voices.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn export_and_import_round_trip_a_persona_exactly() {
        let mine = Harness::new().start();
        let original = mine
            .create_persona(PersonaDraft {
                tone: "Calma, \"sem pressa\".\nSegunda linha. 🎙️".into(),
                presets: GenerationPresets {
                    stability: 0,
                    similarity: 100,
                    style: 35,
                    speed: 70,
                },
                ..draft("Minha narradora")
            })
            .unwrap();
        let package = mine.export_persona(original.id).unwrap();

        let theirs = other_machine().start();
        let imported = theirs.import_persona(&package).unwrap();
        assert_eq!(imported.details, original.details, "every field");
        assert_eq!(imported.owner, theirs.profile().id);
        assert_ne!(imported.id, original.id, "a new persona in their library");
        assert!(theirs.personas().unwrap().contains(&imported));
        // And back again, unchanged.
        assert_eq!(theirs.export_persona(imported.id).unwrap(), package);
    }

    #[test]
    fn the_package_contains_no_secrets_or_local_ids() {
        let h = Harness::new();
        let mut app = h.start();
        let keys = [
            (Provider::Claude, "sk-ant-api03-secret-claude-0001"),
            (Provider::ElevenLabs, ELEVENLABS_KEY),
            (Provider::Gemini, "AIzaSyD-secret-gemini-key-000001"),
            (Provider::Higgsfield, "key-id-0001:secret-higgsfield-0001"),
            (Provider::TypeSafe, "ts_secret_typesafe_key_0001"),
            (Provider::YouTubeData, "AIzaSyD-secret-youtube-key-00001"),
        ];
        for (provider, key) in keys {
            app.save_provider_key(provider, key).unwrap();
        }
        *h.voices.voices.lock().unwrap() =
            vec![listed("myClone01", "My voice", VoiceCategory::Cloned)];
        recorded(&h, &mut app);
        let persona = app.create_persona(draft("Cloned")).unwrap();
        let channel = channel_with(&app, "Space Archives", Some(persona.id));

        let package = app.export_persona(persona.id).unwrap();
        for (provider, key) in keys {
            assert!(!package.contains(key), "{provider} key in {package}");
        }
        for id in [
            persona.id.to_string(),
            app.profile().id.to_string(),
            channel.id.to_string(),
        ] {
            assert!(!package.contains(&id), "{id} in {package}");
        }
        assert!(!package.contains("Space Archives"), "no channel data");
        assert!(package.contains("myClone01"), "only a voice reference");
    }

    #[test]
    fn a_package_from_a_newer_bardo_is_refused_clearly() {
        let app = Harness::new().start();
        let before = app.personas().unwrap();
        let newer = app
            .export_persona(before[0].id)
            .unwrap()
            .replace("\"version\": 1", "\"version\": 2");
        let error = app.import_persona(&newer).unwrap_err();
        assert!(matches!(
            error,
            PersonaError::Package(PackageError::NewerVersion(2))
        ));
        assert_eq!(error.form_message(), Some(Text::PersonaPackageNewer));
        assert!(
            app.text(Text::PersonaPackageNewer).contains("Update Bardo"),
            "the message says what to do"
        );

        let error = app.import_persona("{\"hello\": \"world\"}").unwrap_err();
        assert_eq!(error.form_message(), Some(Text::PersonaPackageNotOne));
        assert_eq!(app.personas().unwrap(), before, "nothing imported");
    }

    #[test]
    fn importing_a_taken_name_adds_a_suffix() {
        let app = Harness::new().start();
        let original = app.personas().unwrap().remove(1);
        let package = app.export_persona(original.id).unwrap();
        let first = app.import_persona(&package).unwrap();
        assert_eq!(
            first.details.name(),
            "Documentary Narrator (en-US) (imported)"
        );
        let second = app.import_persona(&package).unwrap();
        assert_eq!(
            second.details.name(),
            "Documentary Narrator (en-US) (imported 2)"
        );
        assert_eq!(second.details.voice(), original.details.voice());
    }

    #[test]
    fn an_imported_voice_is_looked_up_in_the_users_account() {
        let theirs = Harness::new().start();
        let clone = theirs.create_persona(draft("Their clone")).unwrap();
        let clone_package = theirs.export_persona(clone.id).unwrap();
        let wyatt = theirs.personas().unwrap().remove(1);
        let wyatt_package = theirs.export_persona(wyatt.id).unwrap();

        let h = other_machine();
        *h.voices.voices.lock().unwrap() = vec![listed(
            "FrS6cKLB1wg4WYgPa9GW",
            "Wyatt",
            VoiceCategory::Default,
        )];
        let mut app = h.start_with_key();
        recorded(&h, &mut app);

        let missing = app.import_persona(&clone_package).unwrap();
        assert_eq!(missing.voice_flag, Some(VoiceFlag::Unavailable));
        assert!(!missing.can_narrate());
        let listed_voice = app.import_persona(&wyatt_package).unwrap();
        assert_eq!(listed_voice.voice_flag, None);
        assert_eq!(
            app.personas.get(missing.id).unwrap().unwrap().voice_flag,
            Some(VoiceFlag::Unavailable),
            "the flag is stored"
        );
    }

    #[test]
    fn without_a_listing_the_voice_stays_unchecked_until_the_next_one() {
        let theirs = Harness::new().start();
        let clone = theirs.create_persona(draft("Their clone")).unwrap();
        let package = theirs.export_persona(clone.id).unwrap();

        let h = other_machine();
        let mut app = h.start();
        let imported = app.import_persona(&package).unwrap();
        assert_eq!(imported.voice_flag, Some(VoiceFlag::Unchecked));

        // The user adds a key; the account does not have the voice yet.
        app.save_provider_key(Provider::ElevenLabs, ELEVENLABS_KEY)
            .unwrap();
        recorded(&h, &mut app);
        let flag = |app: &Bardo| app.personas.get(imported.id).unwrap().unwrap().voice_flag;
        assert_eq!(flag(&app), Some(VoiceFlag::Unavailable));

        // A failed listing changes nothing.
        *h.voices.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::ProviderDown,
            "down",
        ));
        recorded(&h, &mut app);
        assert_eq!(flag(&app), Some(VoiceFlag::Unavailable));

        // The voice was shared with their account: checking again clears it.
        *h.voices.failure.lock().unwrap() = None;
        *h.voices.voices.lock().unwrap() =
            vec![listed("myClone01", "My voice", VoiceCategory::Other)];
        recorded(&h, &mut app);
        assert_eq!(flag(&app), None);
    }

    #[test]
    fn a_listing_from_another_key_does_not_vouch_for_the_new_one() {
        let theirs = Harness::new().start();
        let clone = theirs.create_persona(draft("Their clone")).unwrap();
        let package = theirs.export_persona(clone.id).unwrap();

        let h = other_machine();
        *h.voices.voices.lock().unwrap() =
            vec![listed("myClone01", "My voice", VoiceCategory::Cloned)];
        let mut app = h.start_with_key();
        recorded(&h, &mut app);
        // Another account's key: the old listing says nothing about it.
        app.save_provider_key(Provider::ElevenLabs, "sk_test_elevenlabs_key_0002")
            .unwrap();
        assert!(app.voice_list().is_none());
        let imported = app.import_persona(&package).unwrap();
        assert_eq!(imported.voice_flag, Some(VoiceFlag::Unchecked));

        recorded(&h, &mut app);
        app.remove_provider_key(Provider::ElevenLabs).unwrap();
        assert!(app.voice_list().is_none());
        // Other providers' keys leave it alone.
        recorded_without_key_check(&mut app);
    }

    /// Records an empty listing and saves an unrelated key after it.
    fn recorded_without_key_check(app: &mut Bardo) {
        app.record_voice_list(VoiceList {
            voices: Ok(vec![]),
            listed_at: SystemTime::now(),
        });
        app.save_provider_key(Provider::Claude, "sk-ant-api03-test-key-0001")
            .unwrap();
        assert!(app.voice_list().is_some());
    }

    #[test]
    fn a_flagged_persona_is_fixed_by_choosing_a_listed_voice() {
        let theirs = Harness::new().start();
        let clone = theirs.create_persona(draft("Their clone")).unwrap();
        let package = theirs.export_persona(clone.id).unwrap();

        let h = other_machine();
        *h.voices.voices.lock().unwrap() = vec![listed("mine02", "Mine", VoiceCategory::Cloned)];
        let mut app = h.start_with_key();
        recorded(&h, &mut app);
        let imported = app.import_persona(&package).unwrap();
        assert_eq!(imported.voice_flag, Some(VoiceFlag::Unavailable));

        // Editing anything but the voice keeps it flagged.
        let kept = app
            .update_persona(
                imported.id,
                PersonaDraft {
                    tone: "Warmer.".into(),
                    ..PersonaDraft::from(&imported.details)
                },
                &[],
            )
            .unwrap();
        assert_eq!(kept.voice_flag, Some(VoiceFlag::Unavailable));
        // So does a copy of it.
        let copy = app.duplicate_persona(imported.id).unwrap();
        assert_eq!(copy.voice_flag, Some(VoiceFlag::Unavailable));

        let fixed = app
            .update_persona(
                imported.id,
                PersonaDraft {
                    voice: Some(voice("mine02", "Mine")),
                    ..PersonaDraft::from(&kept.details)
                },
                &[],
            )
            .unwrap();
        assert_eq!(fixed.voice_flag, None);
        assert_eq!(app.personas.get(imported.id).unwrap().unwrap(), fixed);
    }

    #[test]
    fn packages_are_written_to_and_read_from_files() {
        let dir = tempfile::tempdir().unwrap();
        let app = Harness::new().start();
        let original = app.create_persona(draft("Ação: \"narradora\"?")).unwrap();
        let name = app.persona_package_name(original.id).unwrap();
        assert_eq!(name, "Ação_ _narradora__.bardo-persona");
        let path = dir.path().join(&name);
        app.export_persona_to(original.id, &path).unwrap();

        let theirs = other_machine().start();
        let imported = theirs.import_persona_from(&path).unwrap();
        assert_eq!(imported.details, original.details);

        let error = theirs
            .import_persona_from(&dir.path().join("missing.bardo-persona"))
            .unwrap_err();
        assert!(matches!(error, PersonaError::PackageUnreadable(_)));
        assert_eq!(error.form_message(), Some(Text::PersonaPackageUnreadable));

        let huge = dir.path().join("huge.bardo-persona");
        std::fs::write(&huge, vec![b' '; 300 * 1024]).unwrap();
        assert!(matches!(
            theirs.import_persona_from(&huge),
            Err(PersonaError::Package(PackageError::NotAPackage))
        ));
        let binary = dir.path().join("binary.bardo-persona");
        std::fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
        assert!(matches!(
            theirs.import_persona_from(&binary),
            Err(PersonaError::Package(PackageError::NotAPackage))
        ));

        let error = app
            .export_persona_to(original.id, &dir.path().join("no/such/folder/x"))
            .unwrap_err();
        assert!(matches!(error, PersonaError::ExportFailed(_)));
        assert_eq!(error.form_message(), Some(Text::PersonaExportFailed));
    }

    #[test]
    fn foreign_personas_cannot_be_exported() {
        let h = Harness::new();
        let app = h.start();
        let stranger = bardo_domain::UserProfile::new(bardo_domain::UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&*h.db, &stranger).unwrap();
        let theirs = Persona::defaults(stranger.id).remove(0);
        PersonaRepository::save(&*h.db, &theirs).unwrap();
        assert!(matches!(
            app.export_persona(theirs.id),
            Err(PersonaError::NotFound)
        ));
    }

    #[test]
    fn a_project_can_pick_only_its_owners_personas() {
        let h = Harness::new();
        let app = h.start();
        let channel = channel_with(&app, "Space Archives", None);
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel.id,
            bardo_domain::Niche::new("space history").unwrap(),
            bardo_domain::ThemeIdea::new("The probe", "").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let project = theme.approve(SystemTime::now()).unwrap();
        app.themes.start_project(&theme, &project).unwrap();

        let mine = app.personas().unwrap().remove(0);
        let picked = app.set_project_persona(project.id, Some(mine.id)).unwrap();
        assert_eq!(picked.persona, Some(mine.id));
        assert_eq!(app.themes.project(project.id).unwrap().unwrap(), picked);

        let stranger = bardo_domain::UserProfile::new(bardo_domain::UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&*h.db, &stranger).unwrap();
        let theirs = Persona::defaults(stranger.id).remove(0);
        PersonaRepository::save(&*h.db, &theirs).unwrap();
        assert!(matches!(
            app.set_project_persona(project.id, Some(theirs.id)),
            Err(PersonaError::NotFound)
        ));
        assert!(matches!(
            app.set_project_persona(VideoProjectId::new(), None),
            Err(PersonaError::ProjectNotFound)
        ));

        let cleared = app.set_project_persona(project.id, None).unwrap();
        assert_eq!(cleared.persona, None);
        assert_eq!(
            app.themes.project(project.id).unwrap().unwrap().persona,
            None
        );
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
