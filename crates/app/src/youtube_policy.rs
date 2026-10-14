//! YouTube API Services policies (issue #145, ADR-0012): the privacy
//! policy the user accepts before Bardo uses YouTube, the links Bardo shows
//! beside YouTube, and deleting stored YouTube data on request. The daily
//! upkeep that applies the retention rule is `Bardo::youtube_upkeep`.

use std::time::SystemTime;

use bardo_domain::{Forgotten, Network, PrivacyConsent, Provider, UiLanguage, UserProfile};

use crate::{AppError, Bardo, KeyState};

/// YouTube's Terms of Service, which Bardo's users agree to (policy
/// III.A.1).
pub const YOUTUBE_TERMS_URL: &str = "https://www.youtube.com/t/terms";
/// Google's Privacy Policy, which Bardo's links to (III.A.2.c).
pub const GOOGLE_PRIVACY_URL: &str = "https://www.google.com/policies/privacy";
/// Where a Google account revokes an app's access (III.A.2.h).
pub const GOOGLE_PERMISSIONS_URL: &str =
    "https://security.google.com/settings/security/permissions";

/// The privacy policy on the guide's website, in `language`: the address
/// Google's forms ask for.
pub fn privacy_policy_url(language: UiLanguage) -> String {
    format!(
        "https://fernandolisboa.github.io/bardo/{}/privacy.html",
        language.tag()
    )
}

/// The guide page that holds the privacy policy.
pub const PRIVACY_PAGE: &str = "privacy";

impl Bardo {
    /// The privacy policy the user accepted, if any (an older version
    /// included).
    pub fn privacy_consent(&self) -> Option<PrivacyConsent> {
        self.profile.privacy
    }

    /// Whether Bardo may use YouTube: the current privacy policy is
    /// accepted.
    pub fn privacy_accepted(&self) -> bool {
        self.profile.accepted_privacy()
    }

    /// Accepts the current privacy policy, now, and remembers it.
    pub fn accept_privacy_policy(&mut self) -> Result<(), AppError> {
        if self.privacy_accepted() {
            return Ok(());
        }
        let updated = UserProfile {
            privacy: Some(PrivacyConsent::current(SystemTime::now())),
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        tracing::info!("accepted the privacy policy");
        Ok(())
    }

    /// Whether YouTube is set up (its key, its app credentials or a
    /// connection) while the current privacy policy is not accepted: its
    /// features wait, and a bar over the screens says why.
    pub fn youtube_waits_for_privacy(&self) -> bool {
        if self.privacy_accepted() {
            return false;
        }
        let key = self.provider_key(Provider::YouTubeData).state != KeyState::NotSet;
        let app = self
            .app_credentials()
            .iter()
            .any(|status| status.network == Network::YouTube && status.state != KeyState::NotSet);
        let connected = self
            .connection_book
            .connections()
            .has_any(Network::YouTube)
            .unwrap_or(false);
        key || app || connected
    }

    /// Deletes every YouTube datum Bardo stored, at the user's request
    /// (policy III.E.4.g): niche research, theme rankings and the numbers
    /// and retention curves of YouTube posts. Nothing on YouTube changes,
    /// and the channels, themes, projects, links and connections stay.
    pub fn forget_youtube_data(&self) -> Result<Forgotten, AppError> {
        let forgotten = self.connection_book.connections().forget_all_youtube()?;
        tracing::info!(?forgotten, "deleted the YouTube data at the user's request");
        Ok(forgotten)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use bardo_domain::{PRIVACY_POLICY_VERSION, ProfileRepository};
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::testing::providers;
    use crate::{JobSettings, Repositories};

    fn start(db: &Arc<Database>) -> Bardo {
        Bardo::start_with(
            Repositories::shared(Arc::clone(db), Arc::new(MemorySecretStore::default())),
            providers(),
            Some("en-US"),
            JobSettings::default(),
        )
        .unwrap()
    }

    #[test]
    fn youtube_waits_for_the_privacy_policy_and_the_acceptance_is_kept() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let mut app = start(&db);
        assert!(!app.privacy_accepted());
        assert_eq!(app.privacy_consent(), None);
        assert!(!app.youtube_waits_for_privacy(), "nothing set up yet");

        assert!(matches!(
            app.save_provider_key(Provider::YouTubeData, "AIzaSyTestKey0001abcdefghij"),
            Err(crate::ProviderKeyError::PrivacyNotAccepted)
        ));
        assert!(matches!(
            app.save_app_credentials(Network::YouTube, "id.apps.googleusercontent.com", "s"),
            Err(crate::ConnectionError::PrivacyNotAccepted)
        ));
        // Other providers do not wait.
        app.save_provider_key(Provider::Claude, "sk-ant-api03-test-key-0001")
            .unwrap();

        let before = SystemTime::now();
        app.accept_privacy_policy().unwrap();
        let consent = app.privacy_consent().unwrap();
        assert_eq!(consent.version, PRIVACY_POLICY_VERSION);
        assert!(consent.accepted_at >= before - Duration::from_millis(1));
        app.save_provider_key(Provider::YouTubeData, "AIzaSyTestKey0001abcdefghij")
            .unwrap();
        drop(app);

        let reopened = start(&db);
        assert!(reopened.privacy_accepted());
        assert!(!reopened.youtube_waits_for_privacy());
    }

    #[test]
    fn a_youtube_set_up_before_the_policy_waits_with_a_bar() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let secrets = Arc::new(MemorySecretStore::default());
        let open = || {
            Bardo::start_with(
                Repositories::shared(Arc::clone(&db), secrets.clone()),
                providers(),
                Some("en-US"),
                JobSettings::default(),
            )
            .unwrap()
        };
        let mut app = open();
        app.accept_privacy_policy().unwrap();
        app.save_provider_key(Provider::YouTubeData, "AIzaSyTestKey0001abcdefghij")
            .unwrap();
        // A database from before the policy.
        let mut profile = app.profile().clone();
        profile.privacy = None;
        ProfileRepository::save(&*db, &profile).unwrap();
        drop(app);

        let app = open();
        assert!(!app.privacy_accepted());
        assert!(app.youtube_waits_for_privacy());
        assert!(matches!(
            app.key_test(Provider::YouTubeData),
            Err(crate::ProviderKeyError::PrivacyNotAccepted)
        ));
    }

    #[test]
    fn the_policy_lives_on_the_site_in_each_language() {
        assert_eq!(
            privacy_policy_url(UiLanguage::PtBr),
            "https://fernandolisboa.github.io/bardo/pt-BR/privacy.html"
        );
        for language in UiLanguage::ALL {
            assert!(crate::Guide::load(language).page(PRIVACY_PAGE).is_some());
        }
    }
}
