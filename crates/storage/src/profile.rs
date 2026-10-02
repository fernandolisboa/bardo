use bardo_domain::{
    LayoutId, MetricsSyncOnStart, ProfileId, ProfileRepository, RepositoryError, UiLanguage,
    UiThemePreference, UserProfile,
};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::{Database, boxed};

impl ProfileRepository for Database {
    fn load_default(&self) -> Result<Option<UserProfile>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                "SELECT id, ui_language, ui_theme, ui_layout, metrics_sync FROM user_profile
                 ORDER BY created_at, rowid LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;

        let Some((id, ui_language, ui_theme, ui_layout, metrics_sync)) = row else {
            return Ok(None);
        };
        Ok(Some(UserProfile {
            id: ProfileId::from(Uuid::parse_str(&id).map_err(boxed)?),
            ui_language: ui_language.parse::<UiLanguage>().map_err(boxed)?,
            ui_theme: UiThemePreference::from_code_or_default(&ui_theme),
            ui_layout: LayoutId::from_code_or_default(&ui_layout),
            metrics_sync: MetricsSyncOnStart::from_code_or_default(&metrics_sync),
        }))
    }

    fn save(&self, profile: &UserProfile) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO user_profile (id, ui_language, ui_theme, ui_layout, metrics_sync)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (id) DO UPDATE SET
                     ui_language = excluded.ui_language,
                     ui_theme = excluded.ui_theme,
                     ui_layout = excluded.ui_layout,
                     metrics_sync = excluded.metrics_sync",
                params![
                    profile.id.to_string(),
                    profile.ui_language.tag(),
                    profile.ui_theme.code(),
                    profile.ui_layout.code(),
                    profile.metrics_sync.code()
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::UiTheme;

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
    fn theme_preference_is_saved_and_loaded_back() {
        let db = Database::open_in_memory().unwrap();
        let mut profile = UserProfile::new(UiLanguage::EnUs);
        db.save(&profile).unwrap();
        assert_eq!(
            db.load_default().unwrap().unwrap().ui_theme,
            UiThemePreference::default()
        );

        profile.ui_theme = UiThemePreference::Fixed(UiTheme::BlackGold);
        db.save(&profile).unwrap();
        assert_eq!(db.load_default().unwrap(), Some(profile));
    }

    #[test]
    fn unknown_stored_theme_reads_as_the_default() {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        db.save(&profile).unwrap();
        db.conn()
            .execute("UPDATE user_profile SET ui_theme = 'fixed:neon'", [])
            .unwrap();

        assert_eq!(
            db.load_default().unwrap().unwrap().ui_theme,
            UiThemePreference::default()
        );
    }

    #[test]
    fn layout_is_saved_and_loaded_back() {
        let db = Database::open_in_memory().unwrap();
        let mut profile = UserProfile::new(UiLanguage::EnUs);
        db.save(&profile).unwrap();
        assert_eq!(
            db.load_default().unwrap().unwrap().ui_layout,
            LayoutId::Workspace
        );

        profile.ui_layout = LayoutId::Studio;
        db.save(&profile).unwrap();
        assert_eq!(db.load_default().unwrap(), Some(profile));
    }

    #[test]
    fn unknown_stored_layout_reads_as_workspace() {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        db.save(&profile).unwrap();
        db.conn()
            .execute("UPDATE user_profile SET ui_layout = 'dashboard'", [])
            .unwrap();

        assert_eq!(
            db.load_default().unwrap().unwrap().ui_layout,
            LayoutId::Workspace
        );
    }

    #[test]
    fn metrics_sync_setting_is_saved_and_unknown_values_read_as_the_default() {
        let db = Database::open_in_memory().unwrap();
        let mut profile = UserProfile::new(UiLanguage::EnUs);
        db.save(&profile).unwrap();
        assert_eq!(
            db.load_default().unwrap().unwrap().metrics_sync,
            MetricsSyncOnStart::Every6Hours
        );

        profile.metrics_sync = MetricsSyncOnStart::Off;
        db.save(&profile).unwrap();
        assert_eq!(db.load_default().unwrap(), Some(profile));

        db.conn()
            .execute("UPDATE user_profile SET metrics_sync = 'weekly'", [])
            .unwrap();
        assert_eq!(
            db.load_default().unwrap().unwrap().metrics_sync,
            MetricsSyncOnStart::Every6Hours
        );
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
