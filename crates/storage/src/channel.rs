use bardo_domain::{
    Channel, ChannelDetails, ChannelDraft, ChannelId, ChannelRepository, ProfileId, RepositoryError,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, boxed};

const SELECT_CHANNEL: &str =
    "SELECT id, profile_id, name, niche, aesthetic_notes, language, country FROM channel";

/// A channel row before its themes are attached.
struct ChannelRow {
    id: String,
    profile_id: String,
    draft: ChannelDraft,
}

impl ChannelRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<(Self, String, String)> {
        Ok((
            Self {
                id: row.get(0)?,
                profile_id: row.get(1)?,
                draft: ChannelDraft {
                    name: row.get(2)?,
                    niche: row.get(3)?,
                    aesthetic_notes: row.get(4)?,
                    ..ChannelDraft::default()
                },
            },
            row.get(5)?,
            row.get(6)?,
        ))
    }

    /// Rebuilds the channel through domain validation, so a row edited
    /// outside the app cannot smuggle in an invalid channel.
    fn into_channel(
        self,
        conn: &Connection,
        language: &str,
        country: &str,
    ) -> Result<Channel, RepositoryError> {
        let mut draft = self.draft;
        draft.language = language.parse().map_err(boxed)?;
        draft.country = country.parse().map_err(boxed)?;
        draft.themes = themes(conn, &self.id).map_err(boxed)?;
        let details = ChannelDetails::validate(draft)
            .map_err(|errors| boxed(InvalidRow(format!("{errors:?}"))))?;
        Ok(Channel {
            id: ChannelId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            owner: ProfileId::from(Uuid::parse_str(&self.profile_id).map_err(boxed)?),
            details,
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("stored channel is invalid: {0}")]
struct InvalidRow(String);

fn themes(conn: &Connection, channel_id: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare_cached("SELECT theme FROM channel_theme WHERE channel_id = ?1 ORDER BY position")?
        .query_map([channel_id], |row| row.get(0))?
        .collect()
}

impl ChannelRepository for Database {
    fn list(&self, owner: ProfileId) -> Result<Vec<Channel>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(&format!(
                "{SELECT_CHANNEL} WHERE profile_id = ?1 ORDER BY name COLLATE NOCASE, rowid"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], ChannelRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter()
            .map(|(row, language, country)| row.into_channel(&conn, &language, &country))
            .collect()
    }

    fn get(&self, id: ChannelId) -> Result<Option<Channel>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                &format!("{SELECT_CHANNEL} WHERE id = ?1"),
                [id.to_string()],
                ChannelRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(|(row, language, country)| row.into_channel(&conn, &language, &country))
            .transpose()
    }

    fn save(&self, channel: &Channel) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id = channel.id.to_string();
        let details = &channel.details;
        tx.execute(
            "INSERT INTO channel (id, profile_id, name, niche, aesthetic_notes, language, country)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 niche = excluded.niche,
                 aesthetic_notes = excluded.aesthetic_notes,
                 language = excluded.language,
                 country = excluded.country,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                id,
                channel.owner.to_string(),
                details.name(),
                details.niche(),
                details.aesthetic_notes(),
                details.language().code(),
                details.country().code(),
            ],
        )
        .map_err(boxed)?;
        tx.execute("DELETE FROM channel_theme WHERE channel_id = ?1", [&id])
            .map_err(boxed)?;
        for (position, theme) in details.themes().iter().enumerate() {
            tx.execute(
                "INSERT INTO channel_theme (channel_id, position, theme) VALUES (?1, ?2, ?3)",
                params![id, position as i64, theme],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{ContentLanguage, Country, ProfileRepository, UiLanguage, UserProfile};

    use super::*;

    fn database_with_profile() -> (Database, ProfileId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        (db, profile.id)
    }

    fn channel(owner: ProfileId, name: &str) -> Channel {
        let details = ChannelDetails::validate(ChannelDraft {
            name: name.into(),
            niche: "space history".into(),
            themes: vec!["Apollo".into(), "Cold War".into()],
            aesthetic_notes: "dark, archival".into(),
            language: ContentLanguage::Portuguese,
            country: Country::Brazil,
        })
        .unwrap();
        Channel::new(owner, details)
    }

    #[test]
    fn saved_channel_is_read_back_with_every_field() {
        let (db, owner) = database_with_profile();
        let saved = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &saved).unwrap();

        assert_eq!(db.get(saved.id).unwrap(), Some(saved.clone()));
        assert_eq!(db.list(owner).unwrap(), [saved]);
    }

    #[test]
    fn unknown_channel_is_none() {
        let (db, _) = database_with_profile();
        assert_eq!(db.get(ChannelId::new()).unwrap(), None);
    }

    #[test]
    fn saving_again_updates_fields_and_replaces_themes() {
        let (db, owner) = database_with_profile();
        let mut saved = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &saved).unwrap();

        saved.details = ChannelDetails::validate(ChannelDraft {
            name: "Space Files".into(),
            themes: vec!["Mars".into()],
            ..ChannelDraft::from(&saved.details)
        })
        .unwrap();
        ChannelRepository::save(&db, &saved).unwrap();

        assert_eq!(db.list(owner).unwrap(), [saved]);
    }

    #[test]
    fn list_is_ordered_by_name_ignoring_case_and_scoped_to_the_owner() {
        let (db, owner) = database_with_profile();
        let other = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &other).unwrap();

        for name in ["beta", "Alpha", "Gamma"] {
            ChannelRepository::save(&db, &channel(owner, name)).unwrap();
        }
        ChannelRepository::save(&db, &channel(other.id, "Not mine")).unwrap();

        let names: Vec<_> = db
            .list(owner)
            .unwrap()
            .into_iter()
            .map(|c| c.details.name().to_owned())
            .collect();
        assert_eq!(names, ["Alpha", "beta", "Gamma"]);
    }

    #[test]
    fn every_row_carries_its_owner() {
        let (db, owner) = database_with_profile();
        let saved = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &saved).unwrap();

        let stored: String = db
            .conn()
            .query_row(
                "SELECT profile_id FROM channel WHERE id = ?1",
                [saved.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, owner.to_string());
    }

    #[test]
    fn a_channel_needs_an_existing_profile() {
        let db = Database::open_in_memory().unwrap();
        assert!(ChannelRepository::save(&db, &channel(ProfileId::new(), "Orphan")).is_err());
    }

    #[test]
    fn duplicate_names_for_one_owner_are_rejected_by_the_schema() {
        let (db, owner) = database_with_profile();
        ChannelRepository::save(&db, &channel(owner, "Space Archives")).unwrap();
        assert!(ChannelRepository::save(&db, &channel(owner, "SPACE ARCHIVES")).is_err());
    }

    #[test]
    fn a_failed_save_leaves_the_previous_version() {
        let (db, owner) = database_with_profile();
        let original = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &original).unwrap();
        ChannelRepository::save(&db, &channel(owner, "Taken")).unwrap();

        let mut renamed = original.clone();
        renamed.details = ChannelDetails::validate(ChannelDraft {
            name: "Taken".into(),
            themes: vec![],
            ..ChannelDraft::from(&original.details)
        })
        .unwrap();
        assert!(ChannelRepository::save(&db, &renamed).is_err());

        assert_eq!(db.get(original.id).unwrap(), Some(original));
    }

    #[test]
    fn channels_survive_reopening_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let profile = UserProfile::new(UiLanguage::PtBr);
        let saved = channel(profile.id, "Arquivos do Espaço");
        {
            let db = Database::open(&path).unwrap();
            ProfileRepository::save(&db, &profile).unwrap();
            ChannelRepository::save(&db, &saved).unwrap();
        }

        let reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.list(profile.id).unwrap(), [saved]);
    }

    #[test]
    fn a_row_with_an_unknown_country_fails_to_load_instead_of_guessing() {
        let (db, owner) = database_with_profile();
        let saved = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &saved).unwrap();
        db.conn()
            .execute("UPDATE channel SET country = 'ZZ'", [])
            .unwrap();

        assert!(db.get(saved.id).is_err());
    }
}
