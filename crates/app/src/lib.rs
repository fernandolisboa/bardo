//! Application state and use cases. The UI talks to Bardo only through this
//! crate, so behavior is tested here instead of through pixels (ADR-0001).

mod channels;
pub mod i18n;
mod jobs;

use std::borrow::Cow;
use std::sync::Arc;

use bardo_domain::{
    ChannelRepository, JobRepository, ProfileRepository, RepositoryError, UiLanguage, UserProfile,
};
use bardo_storage::Database;

pub use bardo_domain;
pub use channels::ChannelError;
pub use i18n::{Catalog, Text};
pub use jobs::{JobActionError, JobContext, JobGroups, JobHandler, JobSettings, TestJob};

use crate::jobs::JobQueue;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

/// The storage ports the app runs on. Each slice that adds a repository adds
/// a field here; tests swap any of them for a fake.
pub struct Repositories {
    pub profiles: Box<dyn ProfileRepository>,
    pub channels: Box<dyn ChannelRepository>,
    /// Shared with the job queue's worker threads.
    pub jobs: Arc<dyn JobRepository>,
}

impl Repositories {
    /// Every port served by one SQLite database.
    pub fn sqlite(db: Database) -> Self {
        let db = Arc::new(db);
        Self {
            profiles: Box::new(Arc::clone(&db)),
            channels: Box::new(Arc::clone(&db)),
            jobs: db,
        }
    }
}

/// The running app for the local user profile.
pub struct Bardo {
    profiles: Box<dyn ProfileRepository>,
    channels: Box<dyn ChannelRepository>,
    jobs: JobQueue,
    profile: UserProfile,
    catalog: Catalog,
}

impl Bardo {
    /// Loads the local profile, creating it on first start (no login), and
    /// starts the job queue, resuming jobs the last session left running.
    /// `system_locale` (e.g. `pt-BR`) picks the language of a new profile.
    pub fn start(
        repositories: Repositories,
        system_locale: Option<&str>,
    ) -> Result<Self, AppError> {
        Self::start_with(repositories, system_locale, JobSettings::default())
    }

    /// `start` with explicit job queue settings.
    pub fn start_with(
        repositories: Repositories,
        system_locale: Option<&str>,
        job_settings: JobSettings,
    ) -> Result<Self, AppError> {
        let Repositories {
            profiles,
            channels,
            jobs,
        } = repositories;
        let profile = match profiles.load_default()? {
            Some(profile) => profile,
            None => {
                let profile = UserProfile::new(language_for_locale(system_locale));
                profiles.save(&profile)?;
                profile
            }
        };
        let catalog = Catalog::load(profile.ui_language);
        let jobs = JobQueue::start(
            jobs,
            profile.id,
            crate::jobs::built_in_handlers(),
            job_settings,
        )?;
        Ok(Self {
            profiles,
            channels,
            jobs,
            profile,
            catalog,
        })
    }

    pub fn profile(&self) -> &UserProfile {
        &self.profile
    }

    pub fn ui_language(&self) -> UiLanguage {
        self.profile.ui_language
    }

    /// Switches the interface language and remembers it. On failure the
    /// current language stays.
    pub fn set_ui_language(&mut self, language: UiLanguage) -> Result<(), AppError> {
        if language == self.profile.ui_language {
            return Ok(());
        }
        let updated = UserProfile {
            ui_language: language,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        self.catalog = Catalog::load(language);
        Ok(())
    }

    pub fn text(&self, text: Text) -> Cow<'_, str> {
        self.catalog.get(text)
    }

    /// `text` with its `{name}` placeholders filled in.
    pub fn text_with(&self, text: Text, args: &[(&str, &str)]) -> String {
        self.catalog.format(text, args)
    }
}

/// Portuguese system locales (`pt-BR`, `pt_BR`, `pt`) start in pt-BR;
/// everything else starts in en-US.
fn language_for_locale(locale: Option<&str>) -> UiLanguage {
    match locale {
        Some(tag) if tag.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("pt")) => UiLanguage::PtBr,
        _ => UiLanguage::EnUs,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    /// In-memory repository that shares its state with the test, so a second
    /// `Bardo::start` sees what the first one saved (a restart).
    #[derive(Clone, Default)]
    struct FakeProfiles {
        saved: Rc<RefCell<Option<UserProfile>>>,
        fail_saves: Rc<Cell<bool>>,
    }

    impl ProfileRepository for FakeProfiles {
        fn load_default(&self) -> Result<Option<UserProfile>, RepositoryError> {
            Ok(self.saved.borrow().clone())
        }

        fn save(&self, profile: &UserProfile) -> Result<(), RepositoryError> {
            if self.fail_saves.get() {
                return Err(RepositoryError("disk full".into()));
            }
            *self.saved.borrow_mut() = Some(profile.clone());
            Ok(())
        }
    }

    fn start(profiles: &FakeProfiles, locale: Option<&str>) -> Bardo {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let repositories = Repositories {
            profiles: Box::new(profiles.clone()),
            channels: Box::new(Arc::clone(&db)),
            jobs: db,
        };
        Bardo::start(repositories, locale).unwrap()
    }

    #[test]
    fn first_start_creates_and_saves_a_profile() {
        let profiles = FakeProfiles::default();
        let app = start(&profiles, None);
        assert_eq!(profiles.saved.borrow().as_ref(), Some(app.profile()));
    }

    #[test]
    fn later_starts_reuse_the_saved_profile() {
        let profiles = FakeProfiles::default();
        let first = start(&profiles, None).profile().id;
        let second = start(&profiles, Some("pt-BR")).profile().id;
        assert_eq!(first, second);
    }

    #[test]
    fn first_start_follows_a_portuguese_system_locale() {
        for locale in ["pt-BR", "pt_BR", "pt", "PT-pt"] {
            let app = start(&FakeProfiles::default(), Some(locale));
            assert_eq!(app.ui_language(), UiLanguage::PtBr, "{locale}");
        }
    }

    #[test]
    fn first_start_defaults_to_english() {
        for locale in [None, Some("en-GB"), Some("fr-FR"), Some("")] {
            let app = start(&FakeProfiles::default(), locale);
            assert_eq!(app.ui_language(), UiLanguage::EnUs, "{locale:?}");
        }
    }

    #[test]
    fn switching_language_changes_every_string_without_restart() {
        let mut app = start(&FakeProfiles::default(), Some("en-US"));
        let english = app.text(Text::AppTagline).into_owned();

        app.set_ui_language(UiLanguage::PtBr).unwrap();

        assert_eq!(app.ui_language(), UiLanguage::PtBr);
        assert_eq!(
            app.text(Text::AppTagline),
            Catalog::load(UiLanguage::PtBr).get(Text::AppTagline)
        );
        assert_ne!(app.text(Text::AppTagline), english);
    }

    #[test]
    fn language_choice_survives_a_restart() {
        let profiles = FakeProfiles::default();
        start(&profiles, Some("en-US"))
            .set_ui_language(UiLanguage::PtBr)
            .unwrap();
        assert_eq!(
            start(&profiles, Some("en-US")).ui_language(),
            UiLanguage::PtBr
        );
    }

    #[test]
    fn failed_save_keeps_the_current_language() {
        let profiles = FakeProfiles::default();
        let mut app = start(&profiles, Some("en-US"));
        profiles.fail_saves.set(true);

        assert!(app.set_ui_language(UiLanguage::PtBr).is_err());
        assert_eq!(app.ui_language(), UiLanguage::EnUs);
        assert_eq!(
            app.text(Text::AppTagline),
            Catalog::load(UiLanguage::EnUs).get(Text::AppTagline)
        );
    }

    #[test]
    fn works_against_real_sqlite() {
        let db = Database::open_in_memory().unwrap();
        let mut app = Bardo::start(Repositories::sqlite(db), Some("en-US")).unwrap();
        app.set_ui_language(UiLanguage::PtBr).unwrap();
        assert_eq!(app.ui_language(), UiLanguage::PtBr);
    }
}
