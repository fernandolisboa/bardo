//! Network connection state (ADR-0008). Tokens never come here: they live
//! in the secret store.

use bardo_domain::{
    ConnectedIdentity, ConnectionStatus, Network, NetworkAccountId, NetworkConnection,
    NetworkConnectionRepository, ProfileId, RepositoryError,
};
use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

#[derive(Debug, thiserror::Error)]
#[error("invalid stored connection: {0}")]
struct InvalidRow(String);

struct ConnectionRow {
    account: String,
    owner: String,
    status: String,
    identity_id: String,
    identity_name: String,
    scopes: String,
    expires_at: i64,
    connected_at: i64,
    refreshed_at: Option<i64>,
}

impl ConnectionRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            account: row.get(0)?,
            owner: row.get(1)?,
            status: row.get(2)?,
            identity_id: row.get(3)?,
            identity_name: row.get(4)?,
            scopes: row.get(5)?,
            expires_at: row.get(6)?,
            connected_at: row.get(7)?,
            refreshed_at: row.get(8)?,
        })
    }

    fn connection(self) -> Result<NetworkConnection, RepositoryError> {
        let uuid = |text: &str| Uuid::parse_str(text).map_err(boxed);
        Ok(NetworkConnection {
            account: NetworkAccountId::from(uuid(&self.account)?),
            owner: ProfileId::from(uuid(&self.owner)?),
            status: ConnectionStatus::from_code(&self.status)
                .ok_or_else(|| boxed(InvalidRow(format!("status {}", self.status))))?,
            identity: ConnectedIdentity {
                id: self.identity_id,
                name: self.identity_name,
            },
            scopes: self.scopes.split_whitespace().map(str::to_owned).collect(),
            expires_at: from_unix_millis(self.expires_at),
            connected_at: from_unix_millis(self.connected_at),
            refreshed_at: self.refreshed_at.map(from_unix_millis),
        })
    }
}

impl NetworkConnectionRepository for Database {
    fn get(&self, account: NetworkAccountId) -> Result<Option<NetworkConnection>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                "SELECT account_id, profile_id, status, identity_id, identity_name, scopes,
                        expires_at, connected_at, refreshed_at
                 FROM network_connection WHERE account_id = ?1",
                [account.to_string()],
                ConnectionRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(ConnectionRow::connection).transpose()
    }

    fn save(&self, connection: &NetworkConnection) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO network_connection (account_id, profile_id, status, identity_id,
                     identity_name, scopes, expires_at, connected_at, refreshed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (account_id) DO UPDATE SET
                     status = excluded.status,
                     identity_id = excluded.identity_id,
                     identity_name = excluded.identity_name,
                     scopes = excluded.scopes,
                     expires_at = excluded.expires_at,
                     connected_at = excluded.connected_at,
                     refreshed_at = excluded.refreshed_at",
                params![
                    connection.account.to_string(),
                    connection.owner.to_string(),
                    connection.status.code(),
                    connection.identity.id,
                    connection.identity.name,
                    connection.scopes.join(" "),
                    to_unix_millis(connection.expires_at),
                    to_unix_millis(connection.connected_at),
                    connection.refreshed_at.map(to_unix_millis),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn delete(&self, account: NetworkAccountId) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "DELETE FROM network_connection WHERE account_id = ?1",
                [account.to_string()],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn connected_on(
        &self,
        owner: ProfileId,
        network: Network,
    ) -> Result<Vec<NetworkAccountId>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT c.account_id FROM network_connection c
                 JOIN network_account a ON a.id = c.account_id
                 WHERE c.profile_id = ?1 AND a.network = ?2
                 ORDER BY c.connected_at, c.account_id",
            )
            .map_err(boxed)?;
        let ids = statement
            .query_map(params![owner.to_string(), network.code()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(boxed)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(boxed)?;
        ids.iter()
            .map(|id| {
                Uuid::parse_str(id)
                    .map(NetworkAccountId::from)
                    .map_err(boxed)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, Network, NetworkAccount, NetworkAccountDetails,
        NetworkAccountDraft, NetworkAccountRepository, UiLanguage, UserProfile,
    };

    use super::*;

    fn time(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    fn database_with_account() -> (Database, NetworkAccount) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&db, &profile).unwrap();
        let channel = Channel::new(
            profile.id,
            ChannelDetails::validate(ChannelDraft {
                name: "Space Archives".into(),
                ..ChannelDraft::default()
            })
            .unwrap(),
        );
        bardo_domain::ChannelRepository::save(&db, &channel).unwrap();
        let details = NetworkAccountDetails::validate(
            Network::YouTube,
            NetworkAccountDraft {
                handle: "spacearchives".into(),
                ..NetworkAccountDraft::default()
            },
        )
        .unwrap();
        let account = NetworkAccount::new(profile.id, channel.id, Network::YouTube, details);
        NetworkAccountRepository::save(&db, &account).unwrap();
        (db, account)
    }

    fn connection(account: &NetworkAccount) -> NetworkConnection {
        NetworkConnection {
            account: account.id,
            owner: account.owner,
            status: ConnectionStatus::Connected,
            identity: ConnectedIdentity {
                id: "UCabcdefghijklmnopqrstuv".into(),
                name: "Arquivos do Espaço".into(),
            },
            scopes: vec![
                "https://www.googleapis.com/auth/youtube.upload".into(),
                "https://www.googleapis.com/auth/youtube".into(),
            ],
            expires_at: time(3600),
            connected_at: time(0),
            refreshed_at: None,
        }
    }

    #[test]
    fn a_connection_round_trips() {
        let (db, account) = database_with_account();
        assert_eq!(
            NetworkConnectionRepository::get(&db, account.id).unwrap(),
            None
        );
        let saved = connection(&account);
        NetworkConnectionRepository::save(&db, &saved).unwrap();
        assert_eq!(
            NetworkConnectionRepository::get(&db, account.id).unwrap(),
            Some(saved)
        );
    }

    #[test]
    fn saving_again_replaces_the_state() {
        let (db, account) = database_with_account();
        NetworkConnectionRepository::save(&db, &connection(&account)).unwrap();
        let later = NetworkConnection {
            status: ConnectionStatus::ReconnectNeeded,
            expires_at: time(9000),
            refreshed_at: Some(time(5400)),
            ..connection(&account)
        };
        NetworkConnectionRepository::save(&db, &later).unwrap();
        assert_eq!(
            NetworkConnectionRepository::get(&db, account.id).unwrap(),
            Some(later)
        );
    }

    #[test]
    fn deleting_is_idempotent() {
        let (db, account) = database_with_account();
        NetworkConnectionRepository::save(&db, &connection(&account)).unwrap();
        NetworkConnectionRepository::delete(&db, account.id).unwrap();
        NetworkConnectionRepository::delete(&db, account.id).unwrap();
        assert_eq!(
            NetworkConnectionRepository::get(&db, account.id).unwrap(),
            None
        );
    }

    #[test]
    fn removing_the_account_removes_its_connection() {
        let (db, account) = database_with_account();
        NetworkConnectionRepository::save(&db, &connection(&account)).unwrap();
        NetworkAccountRepository::delete(&db, account.id).unwrap();
        assert_eq!(
            NetworkConnectionRepository::get(&db, account.id).unwrap(),
            None
        );
    }

    /// An account on `network` in a new channel of the account's owner.
    fn another_account(db: &Database, owner: ProfileId, network: Network) -> NetworkAccount {
        let channel = Channel::new(
            owner,
            ChannelDetails::validate(ChannelDraft {
                name: format!("Channel {}", Uuid::new_v4()),
                ..ChannelDraft::default()
            })
            .unwrap(),
        );
        bardo_domain::ChannelRepository::save(db, &channel).unwrap();
        let details = NetworkAccountDetails::validate(
            network,
            NetworkAccountDraft {
                handle: "another".into(),
                ..NetworkAccountDraft::default()
            },
        )
        .unwrap();
        let account = NetworkAccount::new(owner, channel.id, network, details);
        NetworkAccountRepository::save(db, &account).unwrap();
        account
    }

    #[test]
    fn connected_accounts_are_listed_per_network() {
        let (db, youtube) = database_with_account();
        let first = another_account(&db, youtube.owner, Network::InstagramReels);
        let second = another_account(&db, youtube.owner, Network::InstagramReels);
        let unconnected = another_account(&db, youtube.owner, Network::InstagramReels);
        for (account, at) in [(&youtube, 0), (&second, 20), (&first, 10)] {
            NetworkConnectionRepository::save(
                &db,
                &NetworkConnection {
                    connected_at: time(at),
                    status: if at == 20 {
                        ConnectionStatus::ReconnectNeeded
                    } else {
                        ConnectionStatus::Connected
                    },
                    ..connection(account)
                },
            )
            .unwrap();
        }
        let listed = |network| {
            NetworkConnectionRepository::connected_on(&db, youtube.owner, network).unwrap()
        };
        assert_eq!(listed(Network::InstagramReels), [first.id, second.id]);
        assert!(!listed(Network::InstagramReels).contains(&unconnected.id));
        assert_eq!(listed(Network::YouTube), [youtube.id]);
        assert_eq!(listed(Network::TikTok), []);
        let stranger = UserProfile::new(UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&db, &stranger).unwrap();
        assert_eq!(
            NetworkConnectionRepository::connected_on(&db, stranger.id, Network::YouTube).unwrap(),
            []
        );
    }
}
