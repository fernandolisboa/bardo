use bardo_domain::{
    Job, JobFailure, JobId, JobRecord, JobRepository, ProfileId, Progress, RepositoryError,
};
use rusqlite::{Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

/// Column values of a job row, before parsing.
struct JobRow {
    id: String,
    profile_id: String,
    kind: String,
    payload: String,
    state: String,
    progress: i64,
    attempts: i64,
    checkpoint: Option<String>,
    external_handle: Option<String>,
    failure_kind: Option<String>,
    failure_detail: Option<String>,
    retry_at: Option<i64>,
    run_at: Option<i64>,
}

impl JobRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            kind: row.get(2)?,
            payload: row.get(3)?,
            state: row.get(4)?,
            progress: row.get(5)?,
            attempts: row.get(6)?,
            checkpoint: row.get(7)?,
            external_handle: row.get(8)?,
            failure_kind: row.get(9)?,
            failure_detail: row.get(10)?,
            retry_at: row.get(11)?,
            run_at: row.get(12)?,
        })
    }

    /// Rebuilds the job through the domain's consistency check, so a row
    /// edited outside the app cannot load a state no transition produces.
    fn into_job(self) -> Result<Job, RepositoryError> {
        let failure = match (self.failure_kind, self.failure_detail) {
            (Some(kind), Some(detail)) => {
                Some(JobFailure::new(kind.parse().map_err(boxed)?, detail))
            }
            _ => None,
        };
        let record = JobRecord {
            id: JobId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            owner: ProfileId::from(Uuid::parse_str(&self.profile_id).map_err(boxed)?),
            kind: self.kind.parse().map_err(boxed)?,
            payload: self.payload,
            state: self.state.parse().map_err(boxed)?,
            progress: Progress::from_permille(u16::try_from(self.progress).map_err(boxed)?),
            attempts: u32::try_from(self.attempts).map_err(boxed)?,
            checkpoint: self.checkpoint,
            external_handle: self.external_handle,
            failure,
            retry_at: self.retry_at.map(from_unix_millis),
            run_at: self.run_at.map(from_unix_millis),
        };
        Job::restore(record).map_err(boxed)
    }
}

impl JobRepository for Database {
    fn list(&self, owner: ProfileId) -> Result<Vec<Job>, RepositoryError> {
        let rows = self
            .conn()
            .prepare(
                "SELECT id, profile_id, kind, payload, state, progress, attempts, checkpoint,
                        external_handle, failure_kind, failure_detail, retry_at, run_at
                 FROM job WHERE profile_id = ?1 ORDER BY rowid",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], JobRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter().map(JobRow::into_job).collect()
    }

    fn save(&self, job: &Job) -> Result<(), RepositoryError> {
        let failure = job.failure();
        self.conn()
            .execute(
                "INSERT INTO job (id, profile_id, kind, payload, state, progress, attempts,
                                  checkpoint, external_handle, failure_kind, failure_detail,
                                  retry_at, run_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT (id) DO UPDATE SET
                     state = excluded.state,
                     progress = excluded.progress,
                     attempts = excluded.attempts,
                     checkpoint = excluded.checkpoint,
                     external_handle = excluded.external_handle,
                     failure_kind = excluded.failure_kind,
                     failure_detail = excluded.failure_detail,
                     retry_at = excluded.retry_at,
                     run_at = excluded.run_at,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    job.id().to_string(),
                    job.owner().to_string(),
                    job.kind().code(),
                    job.payload(),
                    job.state().code(),
                    job.progress().permille(),
                    job.attempts(),
                    job.checkpoint(),
                    job.external_handle(),
                    failure.map(|f| f.kind.code()),
                    failure.map(|f| f.detail.as_str()),
                    job.retry_at().map(to_unix_millis),
                    job.run_at().map(to_unix_millis),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        JobFailureKind, JobKind, JobState, ProfileRepository, RetryPolicy, UiLanguage, UserProfile,
    };

    use super::*;

    fn database_with_profile() -> (Database, ProfileId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        (db, profile.id)
    }

    fn job(owner: ProfileId) -> Job {
        Job::new(owner, JobKind::Countdown, r#"{"steps":3}"#)
    }

    /// A job that has been through most transitions, so every column holds
    /// a value.
    fn busy_job(owner: ProfileId) -> Job {
        let now = SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        let mut job = job(owner);
        job.start(now).unwrap();
        job.record_progress(Progress::of(1, 3), Some("1".into()))
            .unwrap();
        job.record_external_handle("provider-job-7").unwrap();
        job.fail_attempt(
            JobFailure::new(JobFailureKind::Simulated, "timeout"),
            now,
            &RetryPolicy::default(),
        )
        .unwrap();
        job
    }

    #[test]
    fn saved_job_is_read_back_with_every_field() {
        let (db, owner) = database_with_profile();
        let saved = busy_job(owner);
        JobRepository::save(&db, &saved).unwrap();
        assert_eq!(JobRepository::list(&db, owner).unwrap(), [saved]);
    }

    #[test]
    fn a_waiting_job_keeps_its_time() {
        let (db, owner) = database_with_profile();
        let at = SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_600_456);
        let saved = Job::scheduled(owner, JobKind::Countdown, "{}", at);
        JobRepository::save(&db, &saved).unwrap();
        let listed = JobRepository::list(&db, owner).unwrap();
        assert_eq!(listed, [saved]);
        assert_eq!(listed[0].run_at(), Some(at));
    }

    #[test]
    fn saving_again_updates_the_job() {
        let (db, owner) = database_with_profile();
        let mut saved = job(owner);
        JobRepository::save(&db, &saved).unwrap();
        saved.cancel().unwrap();
        JobRepository::save(&db, &saved).unwrap();

        let listed = JobRepository::list(&db, owner).unwrap();
        assert_eq!(listed, [saved]);
        assert_eq!(listed[0].state(), JobState::Cancelled);
    }

    #[test]
    fn list_is_oldest_first_and_scoped_to_the_owner() {
        let (db, owner) = database_with_profile();
        let other = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &other).unwrap();

        let first = job(owner);
        let second = job(owner);
        JobRepository::save(&db, &first).unwrap();
        JobRepository::save(&db, &job(other.id)).unwrap();
        JobRepository::save(&db, &second).unwrap();
        // Updating does not move a job in the list.
        JobRepository::save(&db, &first).unwrap();

        let ids: Vec<_> = JobRepository::list(&db, owner)
            .unwrap()
            .iter()
            .map(Job::id)
            .collect();
        assert_eq!(ids, [first.id(), second.id()]);
    }

    #[test]
    fn a_job_needs_an_existing_profile() {
        let db = Database::open_in_memory().unwrap();
        assert!(JobRepository::save(&db, &job(ProfileId::new())).is_err());
    }

    #[test]
    fn jobs_survive_reopening_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let profile = UserProfile::new(UiLanguage::EnUs);
        let saved = busy_job(profile.id);
        {
            let db = Database::open(&path).unwrap();
            ProfileRepository::save(&db, &profile).unwrap();
            JobRepository::save(&db, &saved).unwrap();
        }

        let reopened = Database::open(&path).unwrap();
        assert_eq!(JobRepository::list(&reopened, profile.id).unwrap(), [saved]);
    }

    #[test]
    fn an_inconsistent_row_fails_to_load_instead_of_guessing() {
        let (db, owner) = database_with_profile();
        JobRepository::save(&db, &job(owner)).unwrap();
        db.conn()
            .execute("UPDATE job SET state = 'failed'", [])
            .unwrap();
        assert!(JobRepository::list(&db, owner).is_err());
    }

    #[test]
    fn an_unknown_kind_fails_to_load() {
        let (db, owner) = database_with_profile();
        JobRepository::save(&db, &job(owner)).unwrap();
        db.conn()
            .execute("UPDATE job SET kind = 'teleport'", [])
            .unwrap();
        assert!(JobRepository::list(&db, owner).is_err());
    }
}
