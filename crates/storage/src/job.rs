use std::collections::HashSet;
use std::time::SystemTime;

use bardo_domain::{
    Job, JobFailure, JobHeldElsewhere, JobId, JobRecord, JobRepository, ProfileId, Progress,
    RepositoryError, RunnerRole, RunnerSeen,
};
use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

/// The columns `JobRow::read` reads, in its order.
const JOB_COLUMNS: &str = "id, profile_id, kind, payload, state, progress, attempts, checkpoint,
     external_handle, failure_kind, failure_detail, retry_at, run_at";

/// How long a runner that stopped saying it is up keeps its row.
const FORGET_RUNNER_AFTER_MILLIS: i64 = 24 * 60 * 60 * 1000;

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
            .prepare(&format!(
                "SELECT {JOB_COLUMNS} FROM job WHERE profile_id = ?1 ORDER BY rowid"
            ))
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
        // A job another runner's live lease holds is that runner's: the
        // update is skipped, and the caller told.
        let saved = self
            .conn()
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
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE job.leased_by IS NULL OR job.leased_by = ?14
                       OR job.leased_until <= ?15",
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
                    self.runner(),
                    to_unix_millis(SystemTime::now()),
                ],
            )
            .map_err(boxed)?;
        if saved == 0 {
            return Err(JobHeldElsewhere.into());
        }
        Ok(())
    }

    fn job(&self, id: JobId) -> Result<Option<Job>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM job WHERE id = ?1"),
                [id.to_string()],
                JobRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(JobRow::into_job).transpose()
    }

    fn lease(&self, id: JobId, now: SystemTime, until: SystemTime) -> Result<bool, RepositoryError> {
        // One statement, so two runners racing for a job cannot both win.
        let leased = self
            .conn()
            .execute(
                "UPDATE job SET leased_by = ?2, leased_until = ?3
                 WHERE id = ?1
                   AND (leased_by IS NULL OR leased_by = ?2 OR leased_until <= ?4)",
                params![
                    id.to_string(),
                    self.runner(),
                    to_unix_millis(until),
                    to_unix_millis(now),
                ],
            )
            .map_err(boxed)?;
        Ok(leased > 0)
    }

    fn renew(&self, ids: &[JobId], until: SystemTime) -> Result<Vec<JobId>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare("UPDATE job SET leased_until = ?3 WHERE id = ?1 AND leased_by = ?2")
            .map_err(boxed)?;
        let mut lost = Vec::new();
        for id in ids {
            let renewed = statement
                .execute(params![id.to_string(), self.runner(), to_unix_millis(until)])
                .map_err(boxed)?;
            if renewed == 0 {
                lost.push(*id);
            }
        }
        Ok(lost)
    }

    fn release(&self, id: JobId) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "UPDATE job SET leased_by = NULL, leased_until = NULL
                 WHERE id = ?1 AND leased_by = ?2",
                params![id.to_string(), self.runner()],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn held_elsewhere(
        &self,
        owner: ProfileId,
        now: SystemTime,
    ) -> Result<HashSet<JobId>, RepositoryError> {
        let ids = self
            .conn()
            .prepare(
                "SELECT id FROM job
                 WHERE profile_id = ?1 AND leased_by IS NOT NULL AND leased_by <> ?2
                   AND leased_until > ?3",
            )
            .and_then(|mut statement| {
                statement
                    .query_map(
                        params![owner.to_string(), self.runner(), to_unix_millis(now)],
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        ids.iter()
            .map(|id| Ok(JobId::from(Uuid::parse_str(id).map_err(boxed)?)))
            .collect()
    }

    fn heartbeat(
        &self,
        role: RunnerRole,
        started_at: SystemTime,
        now: SystemTime,
    ) -> Result<(), RepositoryError> {
        let now = to_unix_millis(now);
        let conn = self.conn();
        conn.execute(
            "INSERT INTO job_runner (id, role, started_at, seen_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET seen_at = excluded.seen_at",
            params![self.runner(), role.code(), to_unix_millis(started_at), now],
        )
        .map_err(boxed)?;
        // Runners that died without leaving.
        conn.execute(
            "DELETE FROM job_runner WHERE seen_at < ?1",
            [now.saturating_sub(FORGET_RUNNER_AFTER_MILLIS)],
        )
        .map_err(boxed)?;
        Ok(())
    }

    fn runners(&self, since: SystemTime) -> Result<Vec<RunnerSeen>, RepositoryError> {
        let rows = self
            .conn()
            .prepare(
                "SELECT role, started_at, seen_at FROM job_runner
                 WHERE id <> ?1 AND seen_at >= ?2 ORDER BY started_at",
            )
            .and_then(|mut statement| {
                statement
                    .query_map(params![self.runner(), to_unix_millis(since)], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter()
            .map(|(role, started_at, seen_at)| {
                Ok(RunnerSeen {
                    role: role.parse().map_err(boxed)?,
                    started_at: from_unix_millis(started_at),
                    seen_at: from_unix_millis(seen_at),
                })
            })
            .collect()
    }

    fn leave(&self) -> Result<(), RepositoryError> {
        self.conn()
            .execute("DELETE FROM job_runner WHERE id = ?1", [self.runner()])
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bardo_domain::{
        JobFailureKind, JobKind, JobState, ProfileRepository, RetryPolicy, RunnerRole, RunnerSeen,
        UiLanguage, UserProfile,
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

    fn at(millis: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_000 + millis)
    }

    #[test]
    fn only_one_runner_holds_a_job_until_its_lease_runs_out() {
        let (app, owner) = database_with_profile();
        let agent = app.other_runner();
        let saved = job(owner);
        JobRepository::save(&app, &saved).unwrap();
        let id = saved.id();

        assert!(app.lease(id, at(0), at(30_000)).unwrap());
        assert!(!agent.lease(id, at(1_000), at(31_000)).unwrap(), "held");
        assert!(app.lease(id, at(2_000), at(32_000)).unwrap(), "its own");
        assert_eq!(agent.held_elsewhere(owner, at(3_000)).unwrap(), [id].into());
        assert!(app.held_elsewhere(owner, at(3_000)).unwrap().is_empty());

        // The app stops renewing (it closed or slept): the lease runs out.
        assert!(agent.lease(id, at(32_000), at(62_000)).unwrap());
        assert_eq!(app.renew(&[id], at(70_000)).unwrap(), [id], "lost");
        assert!(agent.renew(&[id], at(70_000)).unwrap().is_empty());
    }

    #[test]
    fn a_released_job_is_free_for_the_other_runner_at_once() {
        let (app, owner) = database_with_profile();
        let agent = app.other_runner();
        let saved = job(owner);
        JobRepository::save(&app, &saved).unwrap();
        assert!(app.lease(saved.id(), at(0), at(30_000)).unwrap());
        // Releasing someone else's lease does nothing.
        agent.release(saved.id()).unwrap();
        assert!(!agent.lease(saved.id(), at(1), at(30_001)).unwrap());
        app.release(saved.id()).unwrap();
        assert!(agent.lease(saved.id(), at(2), at(30_002)).unwrap());
    }

    #[test]
    fn a_job_another_runner_holds_is_not_saved_over() {
        let (app, owner) = database_with_profile();
        let agent = app.other_runner();
        let mut saved = job(owner);
        JobRepository::save(&app, &saved).unwrap();
        let now = SystemTime::now();
        assert!(agent.lease(saved.id(), now, now + Duration::from_secs(60)).unwrap());

        saved.cancel().unwrap();
        let refused = JobRepository::save(&app, &saved).unwrap_err();
        assert!(refused.is_held_elsewhere());
        assert_eq!(
            JobRepository::job(&app, saved.id()).unwrap().unwrap().state(),
            JobState::Queued
        );
        // The holder saves as usual, and once it lets go so does the app.
        JobRepository::save(&agent, &saved).unwrap();
        agent.release(saved.id()).unwrap();
        saved.retry().unwrap();
        JobRepository::save(&app, &saved).unwrap();
        assert_eq!(JobRepository::job(&app, saved.id()).unwrap(), Some(saved));
    }

    #[test]
    fn runners_see_each_other_while_they_say_they_are_up() {
        let db = Database::open_in_memory().unwrap();
        let agent = db.other_runner();
        assert!(db.runners(at(0)).unwrap().is_empty());
        agent
            .heartbeat(RunnerRole::Agent, at(0), at(5_000))
            .unwrap();
        db.heartbeat(RunnerRole::App, at(4_000), at(5_000)).unwrap();

        let seen = db.runners(at(1_000)).unwrap();
        assert_eq!(
            seen,
            [RunnerSeen {
                role: RunnerRole::Agent,
                started_at: at(0),
                seen_at: at(5_000),
            }]
        );
        assert!(db.runners(at(6_000)).unwrap().is_empty(), "not seen since");
        agent.heartbeat(RunnerRole::Agent, at(0), at(9_000)).unwrap();
        assert_eq!(db.runners(at(6_000)).unwrap()[0].seen_at, at(9_000));
        assert_eq!(agent.runners(at(0)).unwrap()[0].role, RunnerRole::App);

        agent.leave().unwrap();
        assert!(db.runners(at(0)).unwrap().is_empty());
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
