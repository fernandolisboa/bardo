//! The files staged in the user's own storage for take models (#194).

use std::time::SystemTime;

use bardo_domain::{
    CaptureId, JobId, ProfileId, Provider, RepositoryError, StagedUpload, StagingRepository,
};
use rusqlite::{Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const SELECT: &str = "SELECT file, profile_id, provider, project_id, capture_id, position, \
     job_id, staged_at, expires_at, released_at FROM take_staging";

/// A row as read: its texts, its position, staged and expiry times and
/// release time.
type StagedRow = (Vec<String>, i64, [i64; 2], Option<i64>);

fn read(row: &Row<'_>) -> rusqlite::Result<StagedRow> {
    Ok((
        vec![
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(6)?,
        ],
        row.get(5)?,
        [row.get(7)?, row.get(8)?],
        row.get(9)?,
    ))
}

fn id(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

fn upload(
    (texts, position, [staged, expires], released): StagedRow,
) -> Result<StagedUpload, RepositoryError> {
    let [file, owner, provider, project, capture, job] = <[String; 6]>::try_from(texts)
        .map_err(|_| boxed(std::io::Error::other("a staged file row is short")))?;
    Ok(StagedUpload {
        file,
        owner: ProfileId::from(id(&owner)?),
        provider: provider.parse::<Provider>().map_err(boxed)?,
        project: id(&project)?.into(),
        capture: CaptureId::from(id(&capture)?),
        position: u32::try_from(position).map_err(boxed)?,
        job: JobId::from(id(&job)?),
        staged_at: from_unix_millis(staged),
        expires_at: from_unix_millis(expires),
        released_at: released.map(from_unix_millis),
    })
}

impl Database {
    fn staged_where(
        &self,
        clause: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<StagedUpload>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(&format!("{SELECT} WHERE {clause} ORDER BY staged_at, file"))
            .map_err(boxed)?;
        let rows = statement
            .query_map(params, read)
            .map_err(boxed)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(boxed)?;
        rows.into_iter().map(upload).collect()
    }
}

impl StagingRepository for Database {
    fn add_staged(&self, upload: &StagedUpload) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO take_staging (file, profile_id, provider, project_id,
                     capture_id, position, job_id, staged_at, expires_at, released_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    upload.file,
                    upload.owner.to_string(),
                    upload.provider.code(),
                    upload.project.to_string(),
                    upload.capture.to_string(),
                    upload.position,
                    upload.job.to_string(),
                    to_unix_millis(upload.staged_at),
                    to_unix_millis(upload.expires_at),
                    upload.released_at.map(to_unix_millis),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn staged_by(&self, job: JobId) -> Result<Vec<StagedUpload>, RepositoryError> {
        self.staged_where("job_id = ?1", [job.to_string()])
    }

    fn release_staged_by(&self, job: JobId, at: SystemTime) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "UPDATE take_staging SET released_at = ?2
                 WHERE job_id = ?1 AND (released_at IS NULL OR released_at > ?2)",
                params![job.to_string(), to_unix_millis(at)],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn release_staged_for(
        &self,
        capture: CaptureId,
        at: SystemTime,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "UPDATE take_staging SET released_at = ?2
                 WHERE capture_id = ?1 AND (released_at IS NULL OR released_at > ?2)",
                params![capture.to_string(), to_unix_millis(at)],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn staged_due(
        &self,
        owner: ProfileId,
        now: SystemTime,
    ) -> Result<Vec<StagedUpload>, RepositoryError> {
        self.staged_where(
            "profile_id = ?1 AND (expires_at <= ?2 OR released_at <= ?2)",
            params![owner.to_string(), to_unix_millis(now)],
        )
    }

    fn remove_staged(&self, file: &str) -> Result<(), RepositoryError> {
        self.conn()
            .execute("DELETE FROM take_staging WHERE file = ?1", [file])
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bardo_domain::{ProfileRepository, UiLanguage, UserProfile, VideoProjectId};

    use super::*;

    #[test]
    fn staged_files_are_due_once_given_up_or_expired_and_go_once_deleted() {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let (capture, job, other_job) = (CaptureId::new(), JobId::new(), JobId::new());
        let staged = |file: &str, job, expires| StagedUpload {
            file: file.into(),
            owner: profile.id,
            provider: Provider::BytePlus,
            project: VideoProjectId::new(),
            capture,
            position: 1,
            job,
            staged_at: now,
            expires_at: now + expires,
            released_at: None,
        };
        let week = Duration::from_secs(7 * 24 * 3600);
        db.add_staged(&staged("video", job, week)).unwrap();
        db.add_staged(&staged("look", job, week)).unwrap();
        db.add_staged(&staged("old", other_job, Duration::ZERO))
            .unwrap();
        assert_eq!(db.staged_by(job).unwrap().len(), 2);
        let due = |at| -> Vec<String> {
            db.staged_due(profile.id, at)
                .unwrap()
                .into_iter()
                .map(|upload| upload.file)
                .collect()
        };
        assert_eq!(due(now), ["old"]);
        db.release_staged_by(job, now + Duration::from_secs(5))
            .unwrap();
        assert_eq!(due(now), ["old"]);
        assert_eq!(due(now + Duration::from_secs(5)), ["look", "old", "video"]);
        db.remove_staged("old").unwrap();
        assert_eq!(db.staged_by(other_job).unwrap(), []);
        // A capture removed gives up all of its files at once.
        db.add_staged(&staged("later", JobId::new(), week)).unwrap();
        db.release_staged_for(capture, now).unwrap();
        assert_eq!(due(now), ["later", "look", "video"]);
        let kept = &db.staged_by(job).unwrap()[0];
        assert_eq!(kept.released_at, Some(now));
        assert_eq!(kept.position, 1);
        assert_eq!(kept.provider, Provider::BytePlus);
    }
}
