use bardo_domain::{
    ProfileId, RepositoryError, TourId, TourProgress, TourProgressRepository, TourState,
};
use rusqlite::params;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

impl TourProgressRepository for Database {
    fn tour_progress(&self, profile: ProfileId) -> Result<Vec<TourProgress>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT tour, content_version, state, last_step, updated_at
                 FROM tour_progress WHERE profile_id = ?1",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map(params![profile.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(boxed)?;
        let mut progress = Vec::new();
        for row in rows {
            let (tour, version, state, last_step, updated_at) = row.map_err(boxed)?;
            // A tour or state this version does not know reads as no
            // progress: the tour is offered as if new.
            let (Some(tour), Some(state)) =
                (TourId::from_code(&tour), TourState::from_code(&state))
            else {
                continue;
            };
            progress.push(TourProgress {
                profile,
                tour,
                version: u32::try_from(version).map_err(boxed)?,
                state,
                last_step: u32::try_from(last_step).map_err(boxed)?,
                updated_at: from_unix_millis(updated_at),
            });
        }
        Ok(progress)
    }

    fn save_tour_progress(&self, progress: &TourProgress) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO tour_progress
                     (profile_id, tour, content_version, state, last_step, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (profile_id, tour) DO UPDATE SET
                     content_version = excluded.content_version,
                     state = excluded.state,
                     last_step = excluded.last_step,
                     updated_at = excluded.updated_at",
                params![
                    progress.profile.to_string(),
                    progress.tour.code(),
                    progress.version,
                    progress.state.code(),
                    progress.last_step,
                    to_unix_millis(progress.updated_at),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn reset_tour_progress(&self, profile: ProfileId) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "DELETE FROM tour_progress WHERE profile_id = ?1",
                params![profile.to_string()],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{ProfileRepository, UiLanguage, UserProfile};

    use super::*;

    fn profile(db: &Database) -> ProfileId {
        let profile = UserProfile::new(UiLanguage::EnUs);
        db.save(&profile).unwrap();
        profile.id
    }

    fn progress(profile: ProfileId, state: TourState, last_step: u32) -> TourProgress {
        TourProgress {
            profile,
            tour: TourId::Welcome,
            version: 1,
            state,
            last_step,
            updated_at: SystemTime::UNIX_EPOCH + Duration::from_millis(1_790_000_000_123),
        }
    }

    #[test]
    fn a_new_profile_has_no_progress() {
        let db = Database::open_in_memory().unwrap();
        let owner = profile(&db);
        assert_eq!(db.tour_progress(owner).unwrap(), vec![]);
    }

    #[test]
    fn saved_progress_is_read_back() {
        let db = Database::open_in_memory().unwrap();
        let owner = profile(&db);
        let saved = progress(owner, TourState::InProgress, 3);
        db.save_tour_progress(&saved).unwrap();
        assert_eq!(db.tour_progress(owner).unwrap(), vec![saved]);
    }

    #[test]
    fn saving_again_replaces_the_tour_row() {
        let db = Database::open_in_memory().unwrap();
        let owner = profile(&db);
        db.save_tour_progress(&progress(owner, TourState::InProgress, 3))
            .unwrap();
        let finished = TourProgress {
            version: 2,
            ..progress(owner, TourState::Completed, 7)
        };
        db.save_tour_progress(&finished).unwrap();
        assert_eq!(db.tour_progress(owner).unwrap(), vec![finished]);
    }

    #[test]
    fn progress_is_kept_per_profile() {
        let db = Database::open_in_memory().unwrap();
        let first = profile(&db);
        let second = profile(&db);
        db.save_tour_progress(&progress(first, TourState::Dismissed, 0))
            .unwrap();
        assert_eq!(db.tour_progress(second).unwrap(), vec![]);
        assert_eq!(db.tour_progress(first).unwrap().len(), 1);
    }

    #[test]
    fn reset_forgets_only_that_profile() {
        let db = Database::open_in_memory().unwrap();
        let first = profile(&db);
        let second = profile(&db);
        db.save_tour_progress(&progress(first, TourState::Completed, 7))
            .unwrap();
        db.save_tour_progress(&progress(second, TourState::Completed, 7))
            .unwrap();

        db.reset_tour_progress(first).unwrap();

        assert_eq!(db.tour_progress(first).unwrap(), vec![]);
        assert_eq!(db.tour_progress(second).unwrap().len(), 1);
    }

    #[test]
    fn unknown_tours_and_states_read_as_no_progress() {
        let db = Database::open_in_memory().unwrap();
        let owner = profile(&db);
        db.conn()
            .execute(
                "INSERT INTO tour_progress
                     (profile_id, tour, content_version, state, last_step, updated_at)
                 VALUES (?1, 'editor-2030', 1, 'completed', 0, 0)",
                params![owner.to_string()],
            )
            .unwrap();
        assert_eq!(db.tour_progress(owner).unwrap(), vec![]);
    }

    #[test]
    fn the_table_refuses_states_it_does_not_know() {
        let db = Database::open_in_memory().unwrap();
        let owner = profile(&db);
        let refused = db.conn().execute(
            "INSERT INTO tour_progress
                 (profile_id, tour, content_version, state, last_step, updated_at)
             VALUES (?1, 'welcome', 1, 'paused', 0, 0)",
            params![owner.to_string()],
        );
        assert!(refused.is_err());
    }
}
