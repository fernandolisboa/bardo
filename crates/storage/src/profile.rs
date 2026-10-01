use bardo_domain::{ProfileId, ProfileRepository, RepositoryError, UiLanguage, UserProfile};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::Database;

fn boxed(error: impl std::error::Error + Send + Sync + 'static) -> RepositoryError {
    RepositoryError(Box::new(error))
}

impl ProfileRepository for Database {
    fn load_default(&self) -> Result<Option<UserProfile>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                "SELECT id, ui_language FROM user_profile ORDER BY created_at, rowid LIMIT 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(boxed)?;

        let Some((id, ui_language)) = row else {
            return Ok(None);
        };
        Ok(Some(UserProfile {
            id: ProfileId::from(Uuid::parse_str(&id).map_err(boxed)?),
            ui_language: ui_language.parse::<UiLanguage>().map_err(boxed)?,
        }))
    }

    fn save(&self, profile: &UserProfile) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO user_profile (id, ui_language) VALUES (?1, ?2)
                 ON CONFLICT (id) DO UPDATE SET ui_language = excluded.ui_language",
                params![profile.id.to_string(), profile.ui_language.tag()],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_database_has_no_profile() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.load_default().unwrap(), None);
    }

    #[test]
    fn saved_profile_is_loaded_back() {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::PtBr);
        db.save(&profile).unwrap();
        assert_eq!(db.load_default().unwrap(), Some(profile));
    }

    #[test]
    fn saving_again_updates_instead_of_duplicating() {
        let db = Database::open_in_memory().unwrap();
        let mut profile = UserProfile::new(UiLanguage::EnUs);
        db.save(&profile).unwrap();
        profile.ui_language = UiLanguage::PtBr;
        db.save(&profile).unwrap();

        assert_eq!(db.load_default().unwrap(), Some(profile));
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM user_profile", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn profile_survives_reopening_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("bardo.db");
        let profile = UserProfile::new(UiLanguage::PtBr);

        Database::open(&path).unwrap().save(&profile).unwrap();

        let reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.load_default().unwrap(), Some(profile));
    }
}
