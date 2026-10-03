//! Network account use cases (PRD story 10): a channel's accounts, at most
//! one per network, each with its handle, metadata defaults and render
//! preset overrides. Exports and uploads read them later; signing in to a
//! network lives in `connections`.

use bardo_domain::{
    Channel, ChannelId, Network, NetworkAccount, NetworkAccountDetails, NetworkAccountDraft,
    NetworkAccountFieldError, NetworkAccountId, RenderPreset, RepositoryError,
};

use crate::{Bardo, Text};

#[derive(Debug, thiserror::Error)]
pub enum NetworkAccountError {
    /// The draft breaks one or more field rules; the form shows each one.
    #[error("invalid network account: {0:?}")]
    Invalid(Vec<NetworkAccountFieldError>),
    /// The channel already has an account on this network.
    #[error("the channel already has a {0} account")]
    NetworkTaken(Network),
    #[error("channel not found")]
    ChannelNotFound,
    #[error("network account not found")]
    NotFound,
    /// The account is signed in: disconnecting first revokes its tokens.
    #[error("the network account is still connected")]
    StillConnected,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl NetworkAccountError {
    /// Message for the whole form. Field errors are shown next to their
    /// fields instead.
    pub fn form_message(&self) -> Option<Text> {
        match self {
            NetworkAccountError::Invalid(_) => None,
            NetworkAccountError::NetworkTaken(_) => Some(Text::NetworkAccountTaken),
            NetworkAccountError::ChannelNotFound => Some(Text::ChannelNotFound),
            NetworkAccountError::NotFound => Some(Text::NetworkAccountNotFound),
            NetworkAccountError::StillConnected => Some(Text::NetworkAccountStillConnected),
            NetworkAccountError::Repository(_) => Some(Text::NetworkAccountNotSaved),
        }
    }

    pub fn field_errors(&self) -> &[NetworkAccountFieldError] {
        match self {
            NetworkAccountError::Invalid(errors) => errors,
            _ => &[],
        }
    }
}

impl Bardo {
    /// The channel's accounts, in network order.
    pub fn network_accounts(
        &self,
        channel: ChannelId,
    ) -> Result<Vec<NetworkAccount>, NetworkAccountError> {
        let channel = self.accounts_channel(channel)?;
        Ok(self.network_accounts.list(channel.id)?)
    }

    /// The networks the channel has no account on yet, in network order.
    pub fn free_networks(&self, channel: ChannelId) -> Result<Vec<Network>, NetworkAccountError> {
        let taken: Vec<Network> = self
            .network_accounts(channel)?
            .iter()
            .map(|account| account.network)
            .collect();
        Ok(Network::ALL
            .into_iter()
            .filter(|network| !taken.contains(network))
            .collect())
    }

    pub fn add_network_account(
        &self,
        channel: ChannelId,
        network: Network,
        draft: NetworkAccountDraft,
    ) -> Result<NetworkAccount, NetworkAccountError> {
        let channel = self.accounts_channel(channel)?;
        let details = NetworkAccountDetails::validate(network, draft)
            .map_err(NetworkAccountError::Invalid)?;
        let taken = self
            .network_accounts
            .list(channel.id)?
            .iter()
            .any(|account| account.network == network);
        if taken {
            return Err(NetworkAccountError::NetworkTaken(network));
        }
        let account = NetworkAccount::new(self.profile.id, channel.id, network, details);
        self.network_accounts.save(&account)?;
        Ok(account)
    }

    /// Changes everything but the network, which an account keeps for life.
    pub fn update_network_account(
        &self,
        id: NetworkAccountId,
        draft: NetworkAccountDraft,
    ) -> Result<NetworkAccount, NetworkAccountError> {
        let mut account = self.own_network_account(id)?;
        account.details = NetworkAccountDetails::validate(account.network, draft)
            .map_err(NetworkAccountError::Invalid)?;
        self.network_accounts.save(&account)?;
        Ok(account)
    }

    /// Removes the account. A connected one must be disconnected first, so
    /// its tokens are revoked rather than left behind.
    pub fn remove_network_account(&self, id: NetworkAccountId) -> Result<(), NetworkAccountError> {
        let account = self.own_network_account(id)?;
        if self.connection_book.is_connected(account.id)? {
            return Err(NetworkAccountError::StillConnected);
        }
        Ok(self.network_accounts.delete(account.id)?)
    }

    /// One line describing a preset, e.g. `9:16 · 1080×1920 · H.264 ·
    /// 12 Mbps · up to 3:00 · -14 LUFS`.
    pub fn preset_summary(&self, preset: &RenderPreset) -> String {
        let (width, height) = preset.dimensions();
        self.text_with(
            Text::RenderPresetSummary,
            &[
                ("aspect", preset.aspect.code()),
                ("width", &width.to_string()),
                ("height", &height.to_string()),
                ("codec", preset.codec.name()),
                (
                    "bitrate",
                    &self.localized_decimal(&preset.bitrate.to_string()),
                ),
                ("duration", &preset.max_duration.to_string()),
                (
                    "loudness",
                    &self.localized_decimal(&preset.loudness.to_string()),
                ),
            ],
        )
    }

    /// A domain decimal (`7.5`) with the interface language's separator.
    pub fn localized_decimal(&self, value: &str) -> String {
        value.replace('.', &self.text(Text::DecimalSeparator))
    }

    fn accounts_channel(&self, id: ChannelId) -> Result<Channel, NetworkAccountError> {
        self.channels
            .get(id)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(NetworkAccountError::ChannelNotFound)
    }

    fn own_network_account(
        &self,
        id: NetworkAccountId,
    ) -> Result<NetworkAccount, NetworkAccountError> {
        self.network_accounts
            .get(id)?
            .filter(|account| account.owner == self.profile.id)
            .ok_or(NetworkAccountError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bardo_domain::{
        AspectRatio, ChannelDetails, ChannelDraft, ContentLanguage, NetworkAccountRepository,
        ProfileRepository, UiLanguage, UserProfile, Visibility,
    };
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::{Repositories, testing};

    fn start(db: &Arc<Database>) -> Bardo {
        Bardo::start(
            Repositories::shared(Arc::clone(db), Arc::new(MemorySecretStore::default())),
            testing::providers(),
            Some("en-US"),
        )
        .unwrap()
    }

    fn app() -> Bardo {
        start(&Arc::new(Database::open_in_memory().unwrap()))
    }

    fn channel(app: &Bardo, name: &str) -> ChannelId {
        app.create_channel(ChannelDraft {
            name: name.into(),
            language: ContentLanguage::Portuguese,
            ..ChannelDraft::default()
        })
        .unwrap()
        .id
    }

    fn draft(handle: &str) -> NetworkAccountDraft {
        NetworkAccountDraft {
            handle: handle.into(),
            ..NetworkAccountDraft::default()
        }
    }

    #[test]
    fn a_new_channel_has_no_accounts_and_every_network_free() {
        let app = app();
        let channel = channel(&app, "Space Archives");
        assert!(app.network_accounts(channel).unwrap().is_empty());
        assert_eq!(app.free_networks(channel).unwrap(), Network::ALL);
    }

    #[test]
    fn added_account_is_listed_and_owned_by_the_profile() {
        let app = app();
        let channel = channel(&app, "Space Archives");
        let added = app
            .add_network_account(channel, Network::TikTok, draft("@spacearchives"))
            .unwrap();

        assert_eq!(added.owner, app.profile().id);
        assert_eq!(added.channel, channel);
        assert_eq!(added.details.handle(), "spacearchives");
        assert_eq!(app.network_accounts(channel).unwrap(), [added]);
        assert!(
            !app.free_networks(channel)
                .unwrap()
                .contains(&Network::TikTok)
        );
    }

    #[test]
    fn at_most_one_account_per_network() {
        let app = app();
        let channel = channel(&app, "Space Archives");
        app.add_network_account(channel, Network::YouTube, draft("one"))
            .unwrap();

        let error = app
            .add_network_account(channel, Network::YouTube, draft("two"))
            .unwrap_err();
        assert!(matches!(
            error,
            NetworkAccountError::NetworkTaken(Network::YouTube)
        ));
        assert_eq!(error.form_message(), Some(Text::NetworkAccountTaken));
        assert_eq!(app.network_accounts(channel).unwrap().len(), 1);
    }

    #[test]
    fn the_same_network_is_free_on_another_channel() {
        let app = app();
        let first = channel(&app, "Space Archives");
        let second = channel(&app, "Deep Sea");
        app.add_network_account(first, Network::X, draft("space"))
            .unwrap();
        assert!(
            app.add_network_account(second, Network::X, draft("deepsea"))
                .is_ok()
        );
    }

    #[test]
    fn invalid_draft_reports_field_errors_and_saves_nothing() {
        let app = app();
        let channel = channel(&app, "Space Archives");
        let error = app
            .add_network_account(
                channel,
                Network::InstagramReels,
                NetworkAccountDraft {
                    visibility: Visibility::Unlisted,
                    bitrate: "fast".into(),
                    ..draft(" ")
                },
            )
            .unwrap_err();

        assert_eq!(
            error.field_errors(),
            [
                NetworkAccountFieldError::HandleRequired,
                NetworkAccountFieldError::VisibilityNotOffered,
                NetworkAccountFieldError::BitrateInvalid,
            ]
        );
        assert_eq!(error.form_message(), None);
        assert!(app.network_accounts(channel).unwrap().is_empty());
    }

    #[test]
    fn editing_changes_metadata_and_overrides_but_keeps_the_network() {
        let app = app();
        let channel = channel(&app, "Space Archives");
        let added = app
            .add_network_account(channel, Network::YouTube, draft("space"))
            .unwrap();

        let updated = app
            .update_network_account(
                added.id,
                NetworkAccountDraft {
                    tags: vec!["#space".into(), "apollo".into()],
                    visibility: Visibility::Unlisted,
                    aspect: Some(AspectRatio::Landscape),
                    max_duration: "20:00".into(),
                    ..draft("spacearchives")
                },
            )
            .unwrap();

        assert_eq!(updated.id, added.id);
        assert_eq!(updated.network, Network::YouTube);
        assert_eq!(updated.details.metadata().tags(), ["space", "apollo"]);
        assert_eq!(
            updated.render_preset(),
            RenderPreset {
                aspect: AspectRatio::Landscape,
                max_duration: "20:00".parse().unwrap(),
                ..Network::YouTube.render_preset()
            }
        );
        assert_eq!(app.network_accounts(channel).unwrap(), [updated]);
    }

    #[test]
    fn editing_validates_against_the_accounts_own_network() {
        let app = app();
        let channel = channel(&app, "Space Archives");
        let added = app
            .add_network_account(channel, Network::X, draft("space"))
            .unwrap();
        let error = app
            .update_network_account(
                added.id,
                NetworkAccountDraft {
                    visibility: Visibility::Private,
                    ..draft("space")
                },
            )
            .unwrap_err();
        assert_eq!(
            error.field_errors(),
            [NetworkAccountFieldError::VisibilityNotOffered]
        );
    }

    #[test]
    fn removing_frees_the_network() {
        let app = app();
        let channel = channel(&app, "Space Archives");
        let added = app
            .add_network_account(channel, Network::Kick, draft("space"))
            .unwrap();

        app.remove_network_account(added.id).unwrap();

        assert!(app.network_accounts(channel).unwrap().is_empty());
        assert!(
            app.add_network_account(channel, Network::Kick, draft("space"))
                .is_ok()
        );
        assert!(matches!(
            app.remove_network_account(added.id),
            Err(NetworkAccountError::NotFound)
        ));
    }

    #[test]
    fn unknown_channel_or_account_is_not_found() {
        let app = app();
        assert!(matches!(
            app.add_network_account(ChannelId::new(), Network::X, draft("space")),
            Err(NetworkAccountError::ChannelNotFound)
        ));
        assert!(matches!(
            app.network_accounts(ChannelId::new()),
            Err(NetworkAccountError::ChannelNotFound)
        ));
        let error = app
            .update_network_account(NetworkAccountId::new(), draft("space"))
            .unwrap_err();
        assert_eq!(error.form_message(), Some(Text::NetworkAccountNotFound));
    }

    #[test]
    fn another_profiles_accounts_are_out_of_reach() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let stranger = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&*db, &stranger).unwrap();
        let theirs = bardo_domain::Channel::new(
            stranger.id,
            ChannelDetails::validate(ChannelDraft {
                name: "Theirs".into(),
                ..ChannelDraft::default()
            })
            .unwrap(),
        );
        bardo_domain::ChannelRepository::save(&*db, &theirs).unwrap();
        let account = NetworkAccount::new(
            stranger.id,
            theirs.id,
            Network::YouTube,
            NetworkAccountDetails::validate(Network::YouTube, draft("theirs")).unwrap(),
        );
        NetworkAccountRepository::save(&*db, &account).unwrap();

        assert!(matches!(
            app.network_accounts(theirs.id),
            Err(NetworkAccountError::ChannelNotFound)
        ));
        assert!(matches!(
            app.add_network_account(theirs.id, Network::X, draft("mine")),
            Err(NetworkAccountError::ChannelNotFound)
        ));
        assert!(matches!(
            app.update_network_account(account.id, draft("mine")),
            Err(NetworkAccountError::NotFound)
        ));
        assert!(matches!(
            app.remove_network_account(account.id),
            Err(NetworkAccountError::NotFound)
        ));
        assert_eq!(db.get(account.id).unwrap(), Some(account));
    }

    #[test]
    fn accounts_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let open = || Arc::new(Database::open(&path).unwrap());

        let (channel, added) = {
            let app = start(&open());
            let channel = channel(&app, "Space Archives");
            let added = app
                .add_network_account(
                    channel,
                    Network::YouTube,
                    NetworkAccountDraft {
                        language: Some(ContentLanguage::English),
                        description_footer: "Subscribe!".into(),
                        loudness: "-16".into(),
                        ..draft("space")
                    },
                )
                .unwrap();
            (channel, added)
        };

        let restarted = start(&open());
        assert_eq!(restarted.network_accounts(channel).unwrap(), [added]);
    }

    #[test]
    fn storage_failure_is_reported_as_not_saved() {
        struct Broken;
        impl NetworkAccountRepository for Broken {
            fn list(&self, _: ChannelId) -> Result<Vec<NetworkAccount>, RepositoryError> {
                Ok(vec![])
            }
            fn get(&self, _: NetworkAccountId) -> Result<Option<NetworkAccount>, RepositoryError> {
                Ok(None)
            }
            fn save(&self, _: &NetworkAccount) -> Result<(), RepositoryError> {
                Err(RepositoryError("disk full".into()))
            }
            fn delete(&self, _: NetworkAccountId) -> Result<(), RepositoryError> {
                Err(RepositoryError("disk full".into()))
            }
        }

        let db = Arc::new(Database::open_in_memory().unwrap());
        let repositories = Repositories {
            network_accounts: Arc::new(Broken),
            ..Repositories::shared(Arc::clone(&db), Arc::new(MemorySecretStore::default()))
        };
        let app = Bardo::start(repositories, testing::providers(), None).unwrap();
        let channel = channel(&app, "Space Archives");
        let error = app
            .add_network_account(channel, Network::X, draft("space"))
            .unwrap_err();
        assert_eq!(error.form_message(), Some(Text::NetworkAccountNotSaved));
    }

    #[test]
    fn preset_summary_reads_in_the_interface_language() {
        let mut app = app();
        let preset = RenderPreset {
            bitrate: "7.5".parse().unwrap(),
            ..Network::YouTube.render_preset()
        };
        let summary = app.preset_summary(&preset);
        assert!(summary.contains("9:16"), "{summary}");
        assert!(summary.contains("1080×1920"), "{summary}");
        assert!(summary.contains("H.264"), "{summary}");
        assert!(summary.contains("7.5 Mbps"), "{summary}");
        assert!(summary.contains("3:00"), "{summary}");
        assert!(summary.contains("-14 LUFS"), "{summary}");
        assert!(!summary.contains('{'), "{summary}");

        app.set_ui_language(UiLanguage::PtBr).unwrap();
        let summary = app.preset_summary(&preset);
        assert!(summary.contains("7,5 Mbps"), "{summary}");
        assert!(!summary.contains('{'), "{summary}");
    }
}
