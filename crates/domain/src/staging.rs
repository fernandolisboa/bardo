//! Files staged for take models in the user's own storage (#194,
//! ADR-0025). ModelArk reads a take's control video and look from links
//! to the user's bucket, and its final reads them again, so they stay
//! there until the draft is finished, replaced, refused or past its
//! deadline. Bardo lists each one it stages and deletes it when it is no
//! longer needed; one whose delete fails stays listed for the next try.

use std::sync::Arc;
use std::time::SystemTime;

use crate::{CaptureId, JobId, ProfileId, Provider, RepositoryError, VideoProjectId};

/// A file Bardo staged for a take job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedUpload {
    /// Where it is, as the provider's adapter names it (its link without
    /// the signature).
    pub file: String,
    pub owner: ProfileId,
    pub provider: Provider,
    pub project: VideoProjectId,
    pub capture: CaptureId,
    pub position: u32,
    /// The job that staged it.
    pub job: JobId,
    pub staged_at: SystemTime,
    /// When its link stops working: it is deleted then at the latest.
    pub expires_at: SystemTime,
    /// When it was given up (no draft will be finished from it); `None`
    /// while a final may still read it.
    pub released_at: Option<SystemTime>,
}

impl StagedUpload {
    /// Whether it should be deleted at `now`.
    pub fn due(&self, now: SystemTime) -> bool {
        self.expires_at <= now || self.released_at.is_some_and(|at| at <= now)
    }
}

/// Where staged files are listed.
pub trait StagingRepository: Send + Sync {
    /// Lists a file just staged.
    fn add_staged(&self, upload: &StagedUpload) -> Result<(), RepositoryError>;

    /// The files `job` staged.
    fn staged_by(&self, job: JobId) -> Result<Vec<StagedUpload>, RepositoryError>;

    /// Gives up the files `job` staged, as of `at`.
    fn release_staged_by(&self, job: JobId, at: SystemTime) -> Result<(), RepositoryError>;

    /// Gives up every file staged for `capture`, as of `at`.
    fn release_staged_for(&self, capture: CaptureId, at: SystemTime)
    -> Result<(), RepositoryError>;

    /// `owner`'s files due for deletion at `now`.
    fn staged_due(
        &self,
        owner: ProfileId,
        now: SystemTime,
    ) -> Result<Vec<StagedUpload>, RepositoryError>;

    /// Forgets a file deleted (or found gone).
    fn remove_staged(&self, file: &str) -> Result<(), RepositoryError>;
}

impl<T: StagingRepository + ?Sized> StagingRepository for Arc<T> {
    fn add_staged(&self, upload: &StagedUpload) -> Result<(), RepositoryError> {
        (**self).add_staged(upload)
    }

    fn staged_by(&self, job: JobId) -> Result<Vec<StagedUpload>, RepositoryError> {
        (**self).staged_by(job)
    }

    fn release_staged_by(&self, job: JobId, at: SystemTime) -> Result<(), RepositoryError> {
        (**self).release_staged_by(job, at)
    }

    fn release_staged_for(
        &self,
        capture: CaptureId,
        at: SystemTime,
    ) -> Result<(), RepositoryError> {
        (**self).release_staged_for(capture, at)
    }

    fn staged_due(
        &self,
        owner: ProfileId,
        now: SystemTime,
    ) -> Result<Vec<StagedUpload>, RepositoryError> {
        (**self).staged_due(owner, now)
    }

    fn remove_staged(&self, file: &str) -> Result<(), RepositoryError> {
        (**self).remove_staged(file)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_file_is_due_once_given_up_or_once_its_link_expires() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let upload = StagedUpload {
            file: "https://bucket/x.mp4".into(),
            owner: ProfileId::new(),
            provider: Provider::BytePlus,
            project: VideoProjectId::new(),
            capture: CaptureId::new(),
            position: 0,
            job: JobId::new(),
            staged_at: now,
            expires_at: now + Duration::from_secs(60),
            released_at: None,
        };
        assert!(!upload.due(now));
        assert!(upload.due(now + Duration::from_secs(60)));
        let released = StagedUpload {
            released_at: Some(now),
            ..upload
        };
        assert!(released.due(now));
    }
}
