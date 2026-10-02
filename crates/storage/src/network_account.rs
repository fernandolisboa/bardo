use bardo_domain::{
    Bitrate, ChannelId, Loudness, MaxDuration, Network, NetworkAccount, NetworkAccountDetails,
    NetworkAccountDraft, NetworkAccountId, NetworkAccountRepository, ProfileId, RepositoryError,
    Resolution,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, boxed};

const SELECT_ACCOUNT: &str = "SELECT id, profile_id, channel_id, network, handle, language, \
     description_footer, visibility, aspect, resolution, codec, bitrate_kbps, \
     max_duration_secs, loudness_tenths FROM network_account";

/// A row as stored, before its tags are attached and it is validated.
struct AccountRow {
    id: String,
    profile_id: String,
    channel_id: String,
    network: String,
    handle: String,
    language: Option<String>,
    description_footer: String,
    visibility: String,
    aspect: Option<String>,
    resolution: Option<u32>,
    codec: Option<String>,
    bitrate_kbps: Option<u32>,
    max_duration_secs: Option<u32>,
    loudness_tenths: Option<i16>,
}

impl AccountRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            channel_id: row.get(2)?,
            network: row.get(3)?,
            handle: row.get(4)?,
            language: row.get(5)?,
            description_footer: row.get(6)?,
            visibility: row.get(7)?,
            aspect: row.get(8)?,
            resolution: row.get(9)?,
            codec: row.get(10)?,
            bitrate_kbps: row.get(11)?,
            max_duration_secs: row.get(12)?,
            loudness_tenths: row.get(13)?,
        })
    }

    /// Rebuilds the account through domain validation, so a row edited
    /// outside the app cannot smuggle in an invalid account.
    fn into_account(self, conn: &Connection) -> Result<NetworkAccount, RepositoryError> {
        let network: Network = self.network.parse().map_err(boxed)?;
        let invalid = |what: &str| boxed(InvalidRow(what.to_owned()));
        let draft = NetworkAccountDraft {
            handle: self.handle,
            language: self
                .language
                .map(|code| code.parse())
                .transpose()
                .map_err(boxed)?,
            tags: tags(conn, &self.id).map_err(boxed)?,
            description_footer: self.description_footer,
            visibility: self.visibility.parse().map_err(boxed)?,
            aspect: self
                .aspect
                .map(|code| code.parse())
                .transpose()
                .map_err(boxed)?,
            resolution: self
                .resolution
                .map(|pixels| {
                    Resolution::from_short_side(pixels).ok_or_else(|| invalid("resolution"))
                })
                .transpose()?,
            codec: self
                .codec
                .map(|code| code.parse())
                .transpose()
                .map_err(boxed)?,
            bitrate: text(self.bitrate_kbps.map(Bitrate::from_kbps)).map_err(boxed)?,
            max_duration: text(self.max_duration_secs.map(MaxDuration::from_seconds))
                .map_err(boxed)?,
            loudness: text(self.loudness_tenths.map(Loudness::from_tenths)).map_err(boxed)?,
        };
        let details = NetworkAccountDetails::validate(network, draft)
            .map_err(|errors| invalid(&format!("{errors:?}")))?;
        Ok(NetworkAccount {
            id: NetworkAccountId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            owner: ProfileId::from(Uuid::parse_str(&self.profile_id).map_err(boxed)?),
            channel: ChannelId::from(Uuid::parse_str(&self.channel_id).map_err(boxed)?),
            network,
            details,
        })
    }
}

/// A stored preset value as the text a draft carries; none is blank.
fn text<T: ToString, E>(value: Option<Result<T, E>>) -> Result<String, E> {
    Ok(value
        .transpose()?
        .map(|v| v.to_string())
        .unwrap_or_default())
}

#[derive(Debug, thiserror::Error)]
#[error("stored network account is invalid: {0}")]
struct InvalidRow(String);

fn tags(conn: &Connection, account_id: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare_cached(
        "SELECT tag FROM network_account_tag WHERE account_id = ?1 ORDER BY position",
    )?
    .query_map([account_id], |row| row.get(0))?
    .collect()
}

impl NetworkAccountRepository for Database {
    fn list(&self, channel: ChannelId) -> Result<Vec<NetworkAccount>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(&format!("{SELECT_ACCOUNT} WHERE channel_id = ?1"))
            .and_then(|mut statement| {
                statement
                    .query_map([channel.to_string()], AccountRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        let mut accounts = rows
            .into_iter()
            .map(|row| row.into_account(&conn))
            .collect::<Result<Vec<_>, _>>()?;
        accounts.sort_by_key(|account| account.network);
        Ok(accounts)
    }

    fn get(&self, id: NetworkAccountId) -> Result<Option<NetworkAccount>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                &format!("{SELECT_ACCOUNT} WHERE id = ?1"),
                [id.to_string()],
                AccountRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(|row| row.into_account(&conn)).transpose()
    }

    fn save(&self, account: &NetworkAccount) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id = account.id.to_string();
        let details = &account.details;
        let metadata = details.metadata();
        let overrides = details.overrides();
        tx.execute(
            "INSERT INTO network_account (id, profile_id, channel_id, network, handle, language,
                 description_footer, visibility, aspect, resolution, codec, bitrate_kbps,
                 max_duration_secs, loudness_tenths)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (id) DO UPDATE SET
                 handle = excluded.handle,
                 language = excluded.language,
                 description_footer = excluded.description_footer,
                 visibility = excluded.visibility,
                 aspect = excluded.aspect,
                 resolution = excluded.resolution,
                 codec = excluded.codec,
                 bitrate_kbps = excluded.bitrate_kbps,
                 max_duration_secs = excluded.max_duration_secs,
                 loudness_tenths = excluded.loudness_tenths,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                id,
                account.owner.to_string(),
                account.channel.to_string(),
                account.network.code(),
                details.handle(),
                metadata.language().map(|language| language.code()),
                metadata.description_footer(),
                metadata.visibility().code(),
                overrides.aspect.map(|aspect| aspect.code()),
                overrides
                    .resolution
                    .map(|resolution| resolution.short_side()),
                overrides.codec.map(|codec| codec.code()),
                overrides.bitrate.map(Bitrate::kbps),
                overrides.max_duration.map(MaxDuration::seconds),
                overrides.loudness.map(Loudness::tenths),
            ],
        )
        .map_err(boxed)?;
        tx.execute(
            "DELETE FROM network_account_tag WHERE account_id = ?1",
            [&id],
        )
        .map_err(boxed)?;
        for (position, tag) in metadata.tags().iter().enumerate() {
            tx.execute(
                "INSERT INTO network_account_tag (account_id, position, tag) VALUES (?1, ?2, ?3)",
                params![id, position as i64, tag],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }

    fn delete(&self, id: NetworkAccountId) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "DELETE FROM network_account WHERE id = ?1",
                [id.to_string()],
            )
            .map(|_| ())
            .map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        AspectRatio, Channel, ChannelDetails, ChannelDraft, ContentLanguage, UiLanguage,
        UserProfile, VideoCodec, Visibility,
    };

    use super::*;

    fn database_with_channel() -> (Database, ProfileId, ChannelId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&db, &profile).unwrap();
        let channel = channel(&db, profile.id, "Space Archives");
        (db, profile.id, channel)
    }

    fn channel(db: &Database, owner: ProfileId, name: &str) -> ChannelId {
        let details = ChannelDetails::validate(ChannelDraft {
            name: name.into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(owner, details);
        bardo_domain::ChannelRepository::save(db, &channel).unwrap();
        channel.id
    }

    fn account(owner: ProfileId, channel: ChannelId, network: Network) -> NetworkAccount {
        let details = NetworkAccountDetails::validate(
            network,
            NetworkAccountDraft {
                handle: "spacearchives".into(),
                ..NetworkAccountDraft::default()
            },
        )
        .unwrap();
        NetworkAccount::new(owner, channel, network, details)
    }

    fn full_account(owner: ProfileId, channel: ChannelId) -> NetworkAccount {
        let details = NetworkAccountDetails::validate(
            Network::YouTube,
            NetworkAccountDraft {
                handle: "@arquivos".into(),
                language: Some(ContentLanguage::Portuguese),
                tags: vec!["espaço".into(), "Apollo".into()],
                description_footer: "Inscreva-se!".into(),
                visibility: Visibility::Unlisted,
                aspect: Some(AspectRatio::Landscape),
                resolution: Some(Resolution::Qhd1440),
                codec: Some(VideoCodec::Hevc),
                bitrate: "7.5".into(),
                max_duration: "20:00".into(),
                loudness: "-13.5".into(),
            },
        )
        .unwrap();
        NetworkAccount::new(owner, channel, Network::YouTube, details)
    }

    #[test]
    fn saved_account_is_read_back_with_every_field() {
        let (db, owner, channel) = database_with_channel();
        let saved = full_account(owner, channel);
        NetworkAccountRepository::save(&db, &saved).unwrap();

        assert_eq!(db.get(saved.id).unwrap(), Some(saved.clone()));
        assert_eq!(db.list(channel).unwrap(), [saved]);
    }

    #[test]
    fn an_account_without_overrides_stores_no_preset_values() {
        let (db, owner, channel) = database_with_channel();
        let saved = account(owner, channel, Network::TikTok);
        NetworkAccountRepository::save(&db, &saved).unwrap();

        let stored: Option<u32> = db
            .conn()
            .query_row("SELECT bitrate_kbps FROM network_account", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored, None);
        assert_eq!(db.get(saved.id).unwrap(), Some(saved));
    }

    #[test]
    fn unknown_account_is_none() {
        let (db, _, _) = database_with_channel();
        assert_eq!(db.get(NetworkAccountId::new()).unwrap(), None);
    }

    #[test]
    fn saving_again_updates_fields_and_replaces_tags() {
        let (db, owner, channel) = database_with_channel();
        let mut saved = full_account(owner, channel);
        NetworkAccountRepository::save(&db, &saved).unwrap();

        saved.details = NetworkAccountDetails::validate(
            Network::YouTube,
            NetworkAccountDraft {
                handle: "spacefiles".into(),
                tags: vec!["mars".into()],
                bitrate: String::new(),
                ..NetworkAccountDraft::from(&saved.details)
            },
        )
        .unwrap();
        NetworkAccountRepository::save(&db, &saved).unwrap();

        assert_eq!(db.list(channel).unwrap(), [saved]);
    }

    #[test]
    fn list_is_in_network_order_and_scoped_to_the_channel() {
        let (db, owner, channel) = database_with_channel();
        let other = super::tests::channel(&db, owner, "Deep Sea");
        for network in [Network::Kick, Network::YouTube, Network::X] {
            NetworkAccountRepository::save(&db, &account(owner, channel, network)).unwrap();
        }
        NetworkAccountRepository::save(&db, &account(owner, other, Network::TikTok)).unwrap();

        let networks: Vec<_> = db
            .list(channel)
            .unwrap()
            .into_iter()
            .map(|account| account.network)
            .collect();
        assert_eq!(networks, [Network::YouTube, Network::X, Network::Kick]);
    }

    #[test]
    fn a_second_account_on_the_same_network_is_rejected_by_the_schema() {
        let (db, owner, channel) = database_with_channel();
        NetworkAccountRepository::save(&db, &account(owner, channel, Network::YouTube)).unwrap();
        assert!(
            NetworkAccountRepository::save(&db, &account(owner, channel, Network::YouTube))
                .is_err()
        );
        assert_eq!(db.list(channel).unwrap().len(), 1);
    }

    #[test]
    fn an_account_needs_an_existing_channel() {
        let (db, owner, _) = database_with_channel();
        assert!(
            NetworkAccountRepository::save(&db, &account(owner, ChannelId::new(), Network::X))
                .is_err()
        );
    }

    #[test]
    fn delete_removes_the_account_and_its_tags() {
        let (db, owner, channel) = database_with_channel();
        let saved = full_account(owner, channel);
        NetworkAccountRepository::save(&db, &saved).unwrap();

        db.delete(saved.id).unwrap();
        db.delete(saved.id).unwrap();

        assert!(db.list(channel).unwrap().is_empty());
        let tags: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM network_account_tag", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(tags, 0);
    }

    #[test]
    fn accounts_survive_reopening_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let (saved, channel) = {
            let db = Database::open(&path).unwrap();
            let profile = UserProfile::new(UiLanguage::PtBr);
            bardo_domain::ProfileRepository::save(&db, &profile).unwrap();
            let channel = channel(&db, profile.id, "Arquivos do Espaço");
            let saved = full_account(profile.id, channel);
            NetworkAccountRepository::save(&db, &saved).unwrap();
            (saved, channel)
        };

        let reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.list(channel).unwrap(), [saved]);
    }

    #[test]
    fn a_row_edited_out_of_range_fails_to_load_instead_of_guessing() {
        let (db, owner, channel) = database_with_channel();
        let saved = account(owner, channel, Network::X);
        NetworkAccountRepository::save(&db, &saved).unwrap();
        for edit in [
            "UPDATE network_account SET bitrate_kbps = 1",
            "UPDATE network_account SET bitrate_kbps = NULL, resolution = 999",
            "UPDATE network_account SET resolution = NULL, visibility = 'unlisted'",
            "UPDATE network_account SET visibility = 'public', network = 'myspace'",
        ] {
            db.conn().execute(edit, []).unwrap();
            assert!(db.get(saved.id).is_err(), "{edit}");
        }
    }
}
