//! Archiving and deleting a project or a video (issue #336, ADR-0047 item
//! 7): what blocks both, how Windows takes a deleted folder, and the names
//! a deletion leaves behind for Costs.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use uuid::Uuid;

use crate::{ProfileId, ProjectId, RepositoryError, VideoProjectId};

/// The file a project's videos are packed into when it is archived with
/// *Compress media*, in the project's folder beside `Videos\`.
pub const PACKED_VIDEOS: &str = "Videos.zip";

/// What stops a project or a video from being archived or deleted now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemovalBlock {
    /// A post Bardo publishes at its time (the in-app schedule) waits.
    ScheduledPost,
    /// A post waits on its network to go public at a publish time.
    HeldOnNetwork,
    /// A job for it is queued or running.
    ActiveJob,
}

impl RemovalBlock {
    pub const ALL: [RemovalBlock; 3] = [
        RemovalBlock::ScheduledPost,
        RemovalBlock::HeldOnNetwork,
        RemovalBlock::ActiveJob,
    ];
}

/// Why Windows would delete a folder at once rather than keep it in the
/// Recycle Bin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermanentReason {
    /// The folder is on a removable drive, which has no Recycle Bin.
    RemovableDrive,
    /// The folder is on a network drive, which has no Recycle Bin.
    NetworkDrive,
    /// The folder is larger than the room Windows gives that drive's bin.
    LargerThanBin,
    /// The drive's bin is turned off (files are removed at once).
    BinOff,
}

/// How a deleted folder would go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Disposal {
    /// To the Recycle Bin, where the user can restore it.
    Recycle,
    /// Deleted at once, for this reason.
    Permanent(PermanentReason),
}

/// How Bardo asks a folder to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Discard {
    Recycle,
    Permanent,
}

/// The Recycle Bin of this computer.
pub trait Trash: Send + Sync {
    /// How a folder (or file) of `size` bytes at `path` would go if sent to
    /// the bin.
    fn disposal(&self, path: &Path, size: u64) -> Disposal;

    /// Sends `path` to the bin, or deletes it at once. A path that is not
    /// there is nothing to do.
    fn discard(&self, path: &Path, how: Discard) -> std::io::Result<()>;
}

impl<T: Trash + ?Sized> Trash for Arc<T> {
    fn disposal(&self, path: &Path, size: u64) -> Disposal {
        (**self).disposal(path, size)
    }

    fn discard(&self, path: &Path, how: Discard) -> std::io::Result<()> {
        (**self).discard(path, how)
    }
}

/// What a deletion removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeletedKind {
    Project,
    Video,
}

impl DeletedKind {
    pub fn code(self) -> &'static str {
        match self {
            DeletedKind::Project => "project",
            DeletedKind::Video => "video",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "project" => Some(DeletedKind::Project),
            "video" => Some(DeletedKind::Video),
            _ => None,
        }
    }
}

/// A project or video the user deleted, as Costs goes on naming it: its
/// name until the user forgets it, and when it went. Amounts are never
/// touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedWork {
    /// The deleted project's or video's id.
    pub id: Uuid,
    pub owner: ProfileId,
    pub kind: DeletedKind,
    /// `None` once the user chose *Forget the name*.
    pub name: Option<String>,
    /// For a video, the project it belonged to.
    pub project: Option<ProjectId>,
    pub deleted_at: SystemTime,
}

impl DeletedWork {
    /// The first eight hex digits of its id, as Costs names it once the
    /// name is forgotten.
    pub fn short_id(&self) -> String {
        crate::short_id(self.id)
    }
}

/// Persistence port for archived videos and deletions.
pub trait RemovalRepository: Send + Sync {
    /// The owner's archived videos.
    fn archived_videos(&self, owner: ProfileId)
    -> Result<HashSet<VideoProjectId>, RepositoryError>;

    /// Archives the video at `at`, or brings it back with `None`.
    fn set_video_archived(
        &self,
        video: VideoProjectId,
        at: Option<SystemTime>,
    ) -> Result<(), RepositoryError>;

    /// Removes the video's records, with everything that is only its, and
    /// records `deleted`, in one transaction. Its theme stays, approved,
    /// with no video.
    fn delete_video(
        &self,
        video: VideoProjectId,
        deleted: &DeletedWork,
    ) -> Result<(), RepositoryError>;

    /// Removes the project's records (its channels, their themes, videos
    /// and network accounts) and records `deleted` (the project and each
    /// of its videos), in one transaction.
    fn delete_project(
        &self,
        project: ProjectId,
        deleted: &[DeletedWork],
    ) -> Result<(), RepositoryError>;

    /// The owner's deletions, newest first.
    fn deleted_work(&self, owner: ProfileId) -> Result<Vec<DeletedWork>, RepositoryError>;

    /// Forgets the deleted project's or video's name: on its deletion, and
    /// on the jobs and cost records that kept it (for a project, the
    /// names of its channels and videos too). Amounts stay.
    fn forget_name(&self, owner: ProfileId, id: Uuid) -> Result<(), RepositoryError>;
}

impl<T: RemovalRepository + ?Sized> RemovalRepository for Arc<T> {
    fn archived_videos(
        &self,
        owner: ProfileId,
    ) -> Result<HashSet<VideoProjectId>, RepositoryError> {
        (**self).archived_videos(owner)
    }

    fn set_video_archived(
        &self,
        video: VideoProjectId,
        at: Option<SystemTime>,
    ) -> Result<(), RepositoryError> {
        (**self).set_video_archived(video, at)
    }

    fn delete_video(
        &self,
        video: VideoProjectId,
        deleted: &DeletedWork,
    ) -> Result<(), RepositoryError> {
        (**self).delete_video(video, deleted)
    }

    fn delete_project(
        &self,
        project: ProjectId,
        deleted: &[DeletedWork],
    ) -> Result<(), RepositoryError> {
        (**self).delete_project(project, deleted)
    }

    fn deleted_work(&self, owner: ProfileId) -> Result<Vec<DeletedWork>, RepositoryError> {
        (**self).deleted_work(owner)
    }

    fn forget_name(&self, owner: ProfileId, id: Uuid) -> Result<(), RepositoryError> {
        (**self).forget_name(owner, id)
    }
}

/// Whether what the user typed confirms deleting `name`: the same name,
/// ignoring case and the spaces around it.
pub fn confirms_name(typed: &str, name: &str) -> bool {
    let typed = typed.trim();
    !typed.is_empty() && typed.to_lowercase() == name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_must_be_typed_as_it_is_ignoring_case() {
        assert!(confirms_name(" space archives ", "Space Archives"));
        assert!(!confirms_name("Space", "Space Archives"));
        assert!(!confirms_name("", ""));
        assert!(!confirms_name("   ", "Space"));
    }

    #[test]
    fn kinds_round_trip() {
        for kind in [DeletedKind::Project, DeletedKind::Video] {
            assert_eq!(DeletedKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(DeletedKind::from_code("channel"), None);
    }

    #[test]
    fn a_forgotten_name_is_told_by_its_short_id() {
        let deleted = DeletedWork {
            id: Uuid::parse_str("0123abcd-0000-4000-8000-000000000000").unwrap(),
            owner: ProfileId::new(),
            kind: DeletedKind::Project,
            name: None,
            project: None,
            deleted_at: SystemTime::UNIX_EPOCH,
        };
        assert_eq!(deleted.short_id(), "0123abcd");
    }
}
