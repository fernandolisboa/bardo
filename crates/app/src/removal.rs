//! Archiving and deleting a project or a video (issue #336, ADR-0047 item
//! 7).
//!
//! **Blockers.** Neither may happen while a post waits to go out (in
//! Bardo's schedule, or held on its network for a publish time) or a job
//! for it is queued or running: Bardo lists each, and the user clears it.
//!
//! **Archive** hides a project (from the switcher) or a video (from the
//! Videos list) and holds back every job of it but the metrics sync, so
//! nothing runs or publishes while published posts go on being read. A
//! project archived with *Compress media* packs its video folders into
//! `Videos.zip` (a job that checks the archive before removing them);
//! unarchiving unpacks it first. A starred project keeps its place among
//! the favorites while archived, hidden, so unarchiving gives it back.
//!
//! **Delete** asks for the name, sends the folder to the Recycle Bin (or
//! deletes it at once, as the user chose or as Windows would anyway, which
//! the dialog says beforehand), then removes the records. Deleting a
//! project also disconnects its network accounts. Jobs and cost records
//! keep the names they were saved with, so Costs goes on naming the work
//! until the user forgets the name; amounts never change. A deleted video's
//! theme stays approved, with no video, and may start another.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    DeletedKind, DeletedWork, Discard, Disposal, Job, JobFailure, JobFailureKind, JobId, JobKind,
    PACKED_VIDEOS, Progress, Project, ProjectFolders, ProjectId, ProjectRepository, ProjectState,
    Publication, RemovalBlock, RepositoryError, UploadStatus, VIDEOS_DIR, VideoProject,
    VideoProjectId, confirms_name,
};
use uuid::Uuid;

use crate::connections::Disconnection;
use crate::jobs::{JobContext, JobHandler};
use crate::{AppError, Bardo, Text};

/// What the user archives or deletes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Removable {
    Project(ProjectId),
    Video(VideoProjectId),
}

#[derive(Debug, thiserror::Error)]
pub enum RemovalError {
    #[error("not found")]
    NotFound,
    /// Something must be cleared first; each reason once.
    #[error("blocked: {0:?}")]
    Blocked(Vec<RemovalBlock>),
    /// The name typed is not the name.
    #[error("the name does not match")]
    NameMismatch,
    /// It is archived already, or not archived when unarchiving.
    #[error("not in that state")]
    WrongState,
    /// The project's folder cannot be reached, so its media cannot be
    /// packed or unpacked now.
    #[error("the project's folder cannot be reached")]
    FolderNotFound,
    /// The folder could not go to the bin or be deleted; nothing was
    /// removed.
    #[error("the folder could not be removed: {0}")]
    FolderNotRemoved(std::io::Error),
    /// A network account's tokens could not be removed; the records stay,
    /// so the user can try again.
    #[error("an account could not be disconnected")]
    NotDisconnected,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl From<AppError> for RemovalError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Repository(error) => RemovalError::Repository(error),
        }
    }
}

impl RemovalError {
    /// What the dialog says.
    pub fn message(&self) -> Text {
        match self {
            RemovalError::NotFound => Text::RemovalNotFound,
            RemovalError::Blocked(_) => Text::RemovalBlocked,
            RemovalError::NameMismatch => Text::DeleteNameMismatch,
            RemovalError::WrongState => Text::RemovalWrongState,
            RemovalError::FolderNotFound => Text::FolderNotFound,
            RemovalError::FolderNotRemoved(_) => Text::DeleteFolderNotRemoved,
            RemovalError::NotDisconnected => Text::DeleteNotDisconnected,
            RemovalError::Repository(_) => Text::RemovalNotSaved,
        }
    }
}

/// What the delete dialog shows before the user confirms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePreview {
    /// The name to type.
    pub name: String,
    /// The folder that goes; `None` when there is none, or it cannot be
    /// reached (then only the records go).
    pub folder: Option<PathBuf>,
    /// Its size in bytes.
    pub size: u64,
    /// How it would go to the bin.
    pub disposal: Disposal,
    /// What must be cleared first.
    pub blocks: Vec<RemovalBlock>,
    /// For a project: its videos.
    pub videos: usize,
    /// For a project: its network accounts that are connected, which
    /// deleting disconnects.
    pub connected: usize,
}

/// An archived project as *Archived projects* lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedProject {
    pub project: Project,
    /// Its videos are packed into `Videos.zip`.
    pub packed: bool,
    /// Its media is being packed or unpacked.
    pub busy: bool,
}

/// What the user deleted, as Costs names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedName {
    pub id: Uuid,
    /// `None` once forgotten.
    pub name: Option<String>,
    pub short_id: String,
    pub deleted_at: SystemTime,
}

impl From<&DeletedWork> for DeletedName {
    fn from(deleted: &DeletedWork) -> Self {
        Self {
            id: deleted.id,
            name: deleted.name.clone(),
            short_id: deleted.short_id(),
            deleted_at: deleted.deleted_at,
        }
    }
}

/// The blockers among `publications`.
fn publication_blocks(publications: &[Publication]) -> Vec<RemovalBlock> {
    let mut blocks = Vec::new();
    if publications.iter().any(|publication| {
        publication.due().is_some()
            && publication
                .upload()
                .is_some_and(|upload| matches!(upload.status, UploadStatus::Queued))
    }) {
        blocks.push(RemovalBlock::ScheduledPost);
    }
    if publications.iter().any(Publication::is_scheduled) {
        blocks.push(RemovalBlock::HeldOnNetwork);
    }
    blocks
}

/// The payload's id under `key`, if any.
fn named(job: &Job, key: &str) -> Option<String> {
    let payload = serde_json::from_str::<serde_json::Value>(job.payload()).ok()?;
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// A deletion prepared, for a background thread: the network accounts are
/// disconnected (network calls) and the folder discarded off the UI
/// thread.
pub struct Deletion {
    target: Removable,
    folder: Option<PathBuf>,
    discard: Discard,
    deleted: Vec<DeletedWork>,
    disconnections: Vec<Disconnection>,
    trash: Arc<dyn bardo_domain::Trash>,
    removals: Arc<dyn bardo_domain::RemovalRepository>,
}

impl Deletion {
    pub fn target(&self) -> Removable {
        self.target
    }

    /// Discards the folder, disconnects the accounts and removes the
    /// records, in that order: a folder that will not go stops it all, and
    /// an account whose tokens stay keeps the records, so it can be tried
    /// again.
    pub fn run(self) -> Result<(), RemovalError> {
        if let Some(folder) = &self.folder {
            self.trash
                .discard(folder, self.discard)
                .map_err(RemovalError::FolderNotRemoved)?;
        }
        let mut disconnected = true;
        for disconnection in self.disconnections {
            if let Err(error) = disconnection.run() {
                tracing::warn!("could not disconnect a deleted project's account: {error}");
                disconnected = false;
            }
        }
        if !disconnected {
            return Err(RemovalError::NotDisconnected);
        }
        match self.target {
            Removable::Video(video) => {
                self.removals.delete_video(video, &self.deleted[0])?;
            }
            Removable::Project(project) => {
                self.removals.delete_project(project, &self.deleted)?;
            }
        }
        tracing::info!(
            kind = ?self.target,
            permanent = self.discard == Discard::Permanent,
            "deleted"
        );
        Ok(())
    }
}

impl Bardo {
    fn removable_project(&self, id: ProjectId) -> Result<Project, RemovalError> {
        self.projects
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(RemovalError::NotFound)
    }

    fn removable_video(&self, id: VideoProjectId) -> Result<VideoProject, RemovalError> {
        self.themes
            .project(id)?
            .filter(|video| video.owner == self.profile.id)
            .ok_or(RemovalError::NotFound)
    }

    /// The project's videos, in every one of its channels.
    fn videos_of(&self, project: ProjectId) -> Result<Vec<VideoProject>, RemovalError> {
        let mut videos = Vec::new();
        for channel in self.project_channels(project)? {
            videos.extend(self.themes.projects(channel.id)?);
        }
        Ok(videos)
    }

    /// What stops `target` from being archived or deleted now, each reason
    /// once; empty when nothing does.
    pub fn removal_blocks(&self, target: Removable) -> Result<Vec<RemovalBlock>, RemovalError> {
        let (publications, videos, channels, project) = match target {
            Removable::Video(id) => {
                let video = self.removable_video(id)?;
                (
                    self.publications.publications(id)?,
                    vec![video.id],
                    Vec::new(),
                    None,
                )
            }
            Removable::Project(id) => {
                self.removable_project(id)?;
                let channels = self.project_channels(id)?;
                let mut publications = Vec::new();
                for channel in &channels {
                    publications.extend(self.publications.channel_publications(channel.id)?);
                }
                let videos = self.videos_of(id)?.iter().map(|video| video.id).collect();
                let channels = channels.iter().map(|channel| channel.id).collect();
                (publications, videos, channels, Some(id))
            }
        };
        let mut blocks = publication_blocks(&publications);
        let videos: HashSet<String> = videos.iter().map(ToString::to_string).collect();
        let channels: HashSet<String> = channels
            .iter()
            .map(|id: &bardo_domain::ChannelId| id.to_string())
            .collect();
        let busy = self
            .jobs()
            .iter()
            .filter(|job| job.state().is_active())
            .any(|job| {
                let for_project = project.is_some_and(|project| {
                    job.origin().project == Some(project)
                        || named(job, "owner_project") == Some(project.to_string())
                        // A media move may be carrying this project's media.
                        || job.kind() == JobKind::MoveMedia
                });
                for_project
                    || named(job, "project").is_some_and(|id| videos.contains(&id))
                    || named(job, "channel").is_some_and(|id| channels.contains(&id))
            });
        if busy {
            blocks.push(RemovalBlock::ActiveJob);
        }
        Ok(blocks)
    }

    fn unblocked(&self, target: Removable) -> Result<(), RemovalError> {
        let blocks = self.removal_blocks(target)?;
        if blocks.is_empty() {
            Ok(())
        } else {
            Err(RemovalError::Blocked(blocks))
        }
    }

    /// Archives the project: hidden from the switcher, its jobs held back.
    /// With `compress`, a job packs its videos' folders into `Videos.zip`:
    /// that job, when queued.
    pub fn archive_project(
        &self,
        id: ProjectId,
        compress: bool,
    ) -> Result<Option<JobId>, RemovalError> {
        let mut project = self.removable_project(id)?;
        if !project.is_active() {
            return Err(RemovalError::WrongState);
        }
        self.unblocked(Removable::Project(id))?;
        if compress && self.unreachable_projects().contains(&id) {
            return Err(RemovalError::FolderNotFound);
        }
        project.state = ProjectState::Archived;
        self.projects.save_project(&project)?;
        tracing::info!(compress, "archived a project");
        let job = if compress && project.folder.is_some() {
            let payload = serde_json::json!({ "owner_project": id.to_string() }).to_string();
            Some(
                self.jobs
                    .enqueue(Job::new(self.profile.id, JobKind::PackMedia, payload))?,
            )
        } else {
            None
        };
        self.check_folders();
        Ok(job)
    }

    /// Brings an archived project back, with its star. When its videos
    /// are packed, a job unpacks them first and brings it back once done:
    /// that job.
    pub fn unarchive_project(&self, id: ProjectId) -> Result<Option<JobId>, RemovalError> {
        let mut project = self.removable_project(id)?;
        if project.is_active() {
            return Err(RemovalError::WrongState);
        }
        if self.media_job_for(id).is_some() {
            return Err(RemovalError::Blocked(vec![RemovalBlock::ActiveJob]));
        }
        if self.is_packed(&project) {
            if self.unreachable_projects().contains(&id) {
                return Err(RemovalError::FolderNotFound);
            }
            let payload = serde_json::json!({ "owner_project": id.to_string() }).to_string();
            let job =
                self.jobs
                    .enqueue(Job::new(self.profile.id, JobKind::UnpackMedia, payload))?;
            return Ok(Some(job));
        }
        project.state = ProjectState::Active;
        self.projects.save_project(&project)?;
        tracing::info!("unarchived a project");
        self.check_folders();
        Ok(None)
    }

    /// Whether the project's videos are packed into `Videos.zip`.
    fn is_packed(&self, project: &Project) -> bool {
        project
            .folder
            .as_ref()
            .is_some_and(|folder| self.folder_book.disk.exists(&folder.join(PACKED_VIDEOS)))
    }

    /// The pack or unpack job of the project that is queued or running.
    fn media_job_for(&self, project: ProjectId) -> Option<Job> {
        let id = project.to_string();
        self.jobs().into_iter().find(|job| {
            matches!(job.kind(), JobKind::PackMedia | JobKind::UnpackMedia)
                && job.state().is_active()
                && named(job, "owner_project").as_deref() == Some(id.as_str())
        })
    }

    /// The archived projects, by name.
    pub fn archived_projects(&self) -> Result<Vec<ArchivedProject>, AppError> {
        Ok(self
            .projects()?
            .into_iter()
            .filter(|project| !project.is_active())
            .map(|project| ArchivedProject {
                packed: self.is_packed(&project),
                busy: self.media_job_for(project.id).is_some(),
                project,
            })
            .collect())
    }

    /// Archives the video: hidden from the Videos list, its jobs held
    /// back, nothing of it published.
    pub fn archive_video(&self, id: VideoProjectId) -> Result<(), RemovalError> {
        self.removable_video(id)?;
        if self.archived_videos().contains(&id) {
            return Err(RemovalError::WrongState);
        }
        self.unblocked(Removable::Video(id))?;
        self.folder_book
            .removals
            .set_video_archived(id, Some(SystemTime::now()))?;
        self.check_folders();
        Ok(())
    }

    /// Brings an archived video back to the Videos list.
    pub fn unarchive_video(&self, id: VideoProjectId) -> Result<(), RemovalError> {
        self.removable_video(id)?;
        if !self.archived_videos().contains(&id) {
            return Err(RemovalError::WrongState);
        }
        self.folder_book.removals.set_video_archived(id, None)?;
        self.check_folders();
        Ok(())
    }

    /// The profile's archived videos.
    pub fn archived_videos(&self) -> HashSet<VideoProjectId> {
        self.folder_book
            .removals
            .archived_videos(self.profile.id)
            .unwrap_or_else(|error| {
                tracing::warn!("could not read the archived videos: {error}");
                HashSet::new()
            })
    }

    /// The folder deleting `target` would discard, when it is there: a
    /// project's own folder (only when its `bardo-project.json` names it),
    /// or a video's folder.
    fn folder_to_discard(&self, target: Removable) -> Result<Option<PathBuf>, RemovalError> {
        let disk = &self.folder_book.disk;
        let folder = match target {
            Removable::Project(id) => {
                let project = self.removable_project(id)?;
                self.check_folders();
                if self.unreachable_projects().contains(&id) {
                    None
                } else {
                    project.folder
                }
            }
            Removable::Video(id) => {
                let video = self.removable_video(id)?;
                let reachable = match self.folder_book.folders.video_folder(video.id)? {
                    Some(folder) => !self.unreachable_projects().contains(&folder.project),
                    None => true,
                };
                reachable.then(|| self.video_folder(video.id))
            }
        };
        Ok(folder.filter(|folder| disk.exists(folder)))
    }

    /// What the delete dialog shows for `target`.
    pub fn delete_preview(&self, target: Removable) -> Result<DeletePreview, RemovalError> {
        let (name, videos, connected) = match target {
            Removable::Project(id) => {
                let project = self.removable_project(id)?;
                let videos = self.videos_of(id)?.len();
                (
                    project.name().to_owned(),
                    videos,
                    self.connected_accounts(id)?.len(),
                )
            }
            Removable::Video(id) => (self.removable_video(id)?.title, 0, 0),
        };
        let folder = self.folder_to_discard(target)?;
        let size = match &folder {
            Some(folder) => self.folder_book.disk.size(folder).unwrap_or(0),
            None => 0,
        };
        let disposal = match &folder {
            Some(folder) => self.folder_book.trash.disposal(folder, size),
            None => Disposal::Recycle,
        };
        Ok(DeletePreview {
            name,
            folder,
            size,
            disposal,
            blocks: self.removal_blocks(target)?,
            videos,
            connected,
        })
    }

    /// The project's network accounts that hold a connection.
    fn connected_accounts(
        &self,
        project: ProjectId,
    ) -> Result<Vec<bardo_domain::NetworkAccountId>, RemovalError> {
        let mut accounts = Vec::new();
        for channel in self.project_channels(project)? {
            for account in self.network_accounts.list(channel.id)? {
                if self.has_connection(account.id) {
                    accounts.push(account.id);
                }
            }
        }
        Ok(accounts)
    }

    /// Prepares deleting `target` once `typed` is its name, its folder sent
    /// to the bin (or deleted at once with `permanent`). Run the deletion
    /// off the UI thread.
    pub fn deletion(
        &mut self,
        target: Removable,
        typed: &str,
        permanent: bool,
    ) -> Result<Deletion, RemovalError> {
        let now = SystemTime::now();
        let (name, deleted, accounts) = match target {
            Removable::Project(id) => {
                let project = self.removable_project(id)?;
                let mut deleted = vec![DeletedWork {
                    id: id.as_uuid(),
                    owner: self.profile.id,
                    kind: DeletedKind::Project,
                    name: Some(project.name().to_owned()),
                    project: None,
                    deleted_at: now,
                }];
                deleted.extend(self.videos_of(id)?.into_iter().map(|video| DeletedWork {
                    id: video.id.as_uuid(),
                    owner: self.profile.id,
                    kind: DeletedKind::Video,
                    name: Some(video.title),
                    project: Some(id),
                    deleted_at: now,
                }));
                (
                    project.name().to_owned(),
                    deleted,
                    self.connected_accounts(id)?,
                )
            }
            Removable::Video(id) => {
                let video = self.removable_video(id)?;
                let project = self.project_of_channel(video.channel);
                let deleted = vec![DeletedWork {
                    id: id.as_uuid(),
                    owner: self.profile.id,
                    kind: DeletedKind::Video,
                    name: Some(video.title.clone()),
                    project,
                    deleted_at: now,
                }];
                (video.title, deleted, Vec::new())
            }
        };
        if !confirms_name(typed, &name) {
            return Err(RemovalError::NameMismatch);
        }
        self.unblocked(target)?;
        let folder = self.folder_to_discard(target)?;
        let discard = match (&folder, permanent) {
            (_, true) => Discard::Permanent,
            (Some(folder), false) => {
                let size = self.folder_book.disk.size(folder).unwrap_or(0);
                match self.folder_book.trash.disposal(folder, size) {
                    Disposal::Recycle => Discard::Recycle,
                    Disposal::Permanent(_) => Discard::Permanent,
                }
            }
            (None, false) => Discard::Recycle,
        };
        let disconnections = accounts
            .into_iter()
            .filter_map(|account| self.disconnection(account).ok())
            .collect();
        Ok(Deletion {
            target,
            folder,
            discard,
            deleted,
            disconnections,
            trash: Arc::clone(&self.folder_book.trash),
            removals: Arc::clone(&self.folder_book.removals),
        })
    }

    /// Re-reads what jobs wait on after a deletion ran.
    pub fn after_deletion(&self) {
        self.check_folders();
    }

    fn project_of_channel(&self, channel: bardo_domain::ChannelId) -> Option<ProjectId> {
        self.channels
            .get(channel)
            .ok()
            .flatten()
            .map(|channel| channel.project)
    }

    /// What the user deleted, newest first.
    pub fn deleted_work(&self) -> Vec<DeletedWork> {
        self.folder_book
            .removals
            .deleted_work(self.profile.id)
            .unwrap_or_else(|error| {
                tracing::warn!("could not read the deletions: {error}");
                Vec::new()
            })
    }

    /// How Costs names deleted work: "Space (deleted on October 4, 2026)",
    /// or "Deleted project #1a2b3c4d" once its name is forgotten.
    pub fn deleted_name_text(&self, deleted: &DeletedName, kind: DeletedKind) -> String {
        match &deleted.name {
            Some(name) => {
                let date = self
                    .catalog
                    .date_here(&bardo_domain::local_time(deleted.deleted_at, &self.zone));
                self.text_with(Text::DeletedOn, &[("name", name), ("date", &date)])
            }
            None => self.text_with(
                match kind {
                    DeletedKind::Project => Text::DeletedProjectForgotten,
                    DeletedKind::Video => Text::DeletedVideoForgotten,
                },
                &[("id", &deleted.short_id)],
            ),
        }
    }

    /// Forgets a deleted project's or video's name: Costs names it by its
    /// short id from now on. Amounts never change.
    pub fn forget_deleted_name(&self, id: Uuid) -> Result<(), AppError> {
        self.folder_book.removals.forget_name(self.profile.id, id)?;
        Ok(())
    }

    /// The handler of the pack and unpack jobs.
    pub(crate) fn pack_media_handler(
        book: &crate::folders::FolderBook,
        projects: Arc<dyn ProjectRepository>,
    ) -> PackMediaHandler {
        PackMediaHandler {
            disk: Arc::clone(&book.disk),
            projects,
            unpack: false,
        }
    }
}

/// Packs an archived project's videos into `Videos.zip`
/// (`JobKind::PackMedia`), or unpacks them and brings the project back
/// (`JobKind::UnpackMedia`).
pub(crate) struct PackMediaHandler {
    disk: Arc<dyn ProjectFolders>,
    projects: Arc<dyn ProjectRepository>,
    /// Unpacks rather than packs.
    unpack: bool,
}

impl PackMediaHandler {
    /// The same handler, unpacking.
    pub(crate) fn unpacking(&self) -> Self {
        Self {
            disk: Arc::clone(&self.disk),
            projects: Arc::clone(&self.projects),
            unpack: true,
        }
    }
}

impl JobHandler for PackMediaHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let unexpected = |error: &dyn std::fmt::Display| JobFailure::unexpected(error.to_string());
        let id = serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|payload| {
                payload
                    .get("owner_project")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|id| Uuid::parse_str(id).ok())
            })
            .map(ProjectId::from)
            .ok_or_else(|| unexpected(&"no project in the payload"))?;
        // Deleted, or brought back, since: nothing to do.
        let Some(mut project) = self.projects.project(id).map_err(|e| unexpected(&e))? else {
            return Ok(());
        };
        if project.is_active() {
            return Ok(());
        }
        let Some(folder) = project.folder.clone() else {
            return Ok(());
        };
        let videos = folder.join(VIDEOS_DIR);
        let archive = folder.join(PACKED_VIDEOS);
        let cx = &*cx;
        let mut progress = |done: u64, total: u64| {
            let permille = done.saturating_mul(1000).checked_div(total).unwrap_or(0);
            cx.report_progress(Progress::from_permille(permille.min(999) as u16));
            !cx.should_stop()
        };
        let unpacking = self.unpack;
        let done = if unpacking {
            if self.disk.exists(&archive) {
                self.disk.unpack(&archive, &videos, &mut progress)
            } else {
                Ok(())
            }
        } else {
            self.disk.pack(&videos, &archive, &mut progress)
        };
        match done {
            Ok(()) => {}
            Err(bardo_domain::FolderMoveError::Stopped) => return Ok(()),
            Err(error) => {
                return Err(JobFailure::new(
                    JobFailureKind::Files,
                    format!(
                        "could not {} the videos: {error}",
                        if unpacking { "unpack" } else { "pack" }
                    ),
                ));
            }
        }
        if unpacking {
            project.state = ProjectState::Active;
            self.projects
                .save_project(&project)
                .map_err(|e| unexpected(&e))?;
            tracing::info!("unpacked and unarchived a project");
        }
        cx.report_progress(Progress::from_permille(1000));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bardo_domain::{
        ChannelDraft, CostPurpose, CostRecord, CostRecordId, JobState, Metered, Money,
        PermanentReason, ProjectColor, ProjectDraft, Provider, ThemeStatus,
    };
    use bardo_storage::{Database, MemoryProjectFolders, MemorySecretStore, MemoryTrash};

    use super::*;
    use crate::testing::arriving::ended;
    use crate::{Repositories, testing};

    struct Setup {
        app: Bardo,
        trash: Arc<MemoryTrash>,
        disk: Arc<MemoryProjectFolders>,
    }

    fn setup() -> Setup {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let trash = Arc::new(MemoryTrash::default());
        let repositories = Repositories {
            trash: Arc::clone(&trash) as _,
            ..Repositories::shared(Arc::clone(&db), Arc::new(MemorySecretStore::default()))
        };
        let disk = MemoryProjectFolders::for_database(&db);
        let app = Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap();
        Setup { app, trash, disk }
    }

    fn project(app: &Bardo, name: &str) -> Project {
        app.create_project(ProjectDraft {
            name: name.into(),
            color: ProjectColor::Teal,
        })
        .unwrap()
    }

    fn channel(app: &Bardo, name: &str) -> bardo_domain::Channel {
        app.create_channel(ChannelDraft {
            name: name.into(),
            ..ChannelDraft::default()
        })
        .unwrap()
    }

    /// A video of `channel`, started from an approved theme.
    fn video(app: &Bardo, channel: bardo_domain::ChannelId, title: &str) -> VideoProject {
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel,
            bardo_domain::Niche::new("space history").unwrap(),
            bardo_domain::ThemeIdea::new(title, "The last signal.").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let video = theme.approve(SystemTime::now()).unwrap();
        app.themes.start_project(&theme, &video).unwrap();
        video
    }

    /// A project with a channel and a video whose folder holds a file.
    fn filled(s: &Setup, name: &str) -> (Project, bardo_domain::Channel, VideoProject) {
        let project = project(&s.app, name);
        let channel = channel(&s.app, &format!("{name} channel"));
        let video = video(&s.app, channel.id, "Apollo");
        s.disk.put(&s.app.video_folder(video.id), &["script.json"]);
        (project, channel, video)
    }

    fn cost(app: &Bardo, channel: bardo_domain::ChannelId, video: Option<VideoProjectId>) {
        app.cost_book
            .costs
            .record_cost(&CostRecord {
                id: CostRecordId::new(),
                owner: app.profile().id,
                provider: Provider::Claude,
                model: "claude-test".into(),
                purpose: CostPurpose::Script,
                usage: Metered::default(),
                cost: bardo_domain::Cost::Reported(Money::parse("1.25").unwrap()),
                channel: Some(channel),
                project: video,
                job: None,
                at: SystemTime::now(),
                origin: Default::default(),
            })
            .unwrap();
    }

    fn held_job(app: &Bardo, payload: String) -> JobId {
        app.jobs.pause(true);
        app.jobs
            .enqueue(Job::new(app.profile().id, JobKind::Countdown, payload))
            .unwrap()
    }

    #[test]
    fn an_active_job_refuses_archiving_and_deleting_with_its_reason() {
        let mut s = setup();
        let (project, channel, video) = filled(&s, "Space");
        held_job(&s.app, format!("{{\"project\":\"{}\"}}", video.id));
        for target in [Removable::Project(project.id), Removable::Video(video.id)] {
            assert_eq!(
                s.app.removal_blocks(target).unwrap(),
                [RemovalBlock::ActiveJob]
            );
            let error = s.app.deletion(target, "x", false).err().unwrap();
            assert!(matches!(error, RemovalError::NameMismatch));
        }
        let error = s.app.archive_project(project.id, false).unwrap_err();
        assert!(matches!(&error, RemovalError::Blocked(b) if b == &[RemovalBlock::ActiveJob]));
        assert_eq!(error.message(), Text::RemovalBlocked);
        assert!(matches!(
            s.app.archive_video(video.id),
            Err(RemovalError::Blocked(_))
        ));
        assert!(matches!(
            s.app.deletion(Removable::Video(video.id), "Apollo", false),
            Err(RemovalError::Blocked(_))
        ));
        assert!(matches!(
            s.app
                .deletion(Removable::Project(project.id), "space", false),
            Err(RemovalError::Blocked(_))
        ));
        // A job of the channel blocks the project, not another video.
        let other = self::video(&s.app, channel.id, "Mir");
        assert!(
            s.app
                .removal_blocks(Removable::Video(other.id))
                .unwrap()
                .is_empty()
        );
        assert!(s.app.projects().unwrap().iter().all(Project::is_active));
    }

    #[test]
    fn an_archived_project_leaves_the_switcher_holds_its_jobs_and_comes_back_starred() {
        let s = setup();
        let (space, channel, _) = filled(&s, "Space");
        let cooking = project(&s.app, "Cooking");
        s.app.star_project(space.id, true).unwrap();
        s.app.star_project(cooking.id, true).unwrap();
        s.app.switch_project(space.id).unwrap();

        assert_eq!(s.app.archive_project(space.id, false).unwrap(), None);
        let list = s.app.project_list("").unwrap();
        assert!(
            list.favorites
                .iter()
                .chain(&list.recent)
                .chain(&list.all)
                .all(|p| p.id != space.id)
        );
        assert_eq!(s.app.open_project().unwrap().unwrap().id, cooking.id);
        let archived = s.app.archived_projects().unwrap();
        assert_eq!(archived.len(), 1);
        assert!(!archived[0].packed);
        assert!(matches!(
            s.app.archive_project(space.id, false),
            Err(RemovalError::WrongState)
        ));
        // Reordering the favorites shown leaves the hidden star in place.
        s.app.move_favorite(cooking.id, 0).unwrap();

        // Nothing of it runs; the metrics sync would.
        let id = s
            .app
            .jobs
            .enqueue(Job::new(
                s.app.profile().id,
                JobKind::Countdown,
                format!("{{\"channel\":\"{}\"}}", channel.id),
            ))
            .unwrap();
        let job = s.app.jobs().into_iter().find(|job| job.id() == id).unwrap();
        assert!(s.app.waits_archived(&job));
        assert!(!s.app.waits_for_folder(&job));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            s.app
                .jobs()
                .into_iter()
                .find(|job| job.id() == id)
                .unwrap()
                .state(),
            JobState::Queued
        );

        assert_eq!(s.app.unarchive_project(space.id).unwrap(), None);
        let list = s.app.project_list("").unwrap();
        let favorites: Vec<ProjectId> = list.favorites.iter().map(|p| p.id).collect();
        assert_eq!(favorites, [cooking.id, space.id]);
        ended(&s.app, id);
    }

    #[test]
    fn jobs_wait_while_archived_except_the_metrics_sync_and_the_media_jobs() {
        assert!(crate::folders::waits_while_archived(JobKind::Render));
        assert!(crate::folders::waits_while_archived(JobKind::Upload));
        assert!(!crate::folders::waits_while_archived(JobKind::MetricsSync));
        assert!(!crate::folders::waits_while_archived(JobKind::PackMedia));
        assert!(!crate::folders::waits_while_archived(JobKind::UnpackMedia));
    }

    #[test]
    fn an_archived_video_leaves_the_list_and_holds_its_jobs() {
        let s = setup();
        let (_, _, video) = filled(&s, "Space");
        s.app.archive_video(video.id).unwrap();
        assert!(s.app.archived_videos().contains(&video.id));
        assert!(matches!(
            s.app.archive_video(video.id),
            Err(RemovalError::WrongState)
        ));
        s.app.jobs.pause(true);
        let id = s
            .app
            .jobs
            .enqueue(Job::new(
                s.app.profile().id,
                JobKind::Countdown,
                format!("{{\"project\":\"{}\"}}", video.id),
            ))
            .unwrap();
        let job = s.app.jobs().into_iter().find(|job| job.id() == id).unwrap();
        assert!(s.app.waits_archived(&job));
        s.app.unarchive_video(video.id).unwrap();
        assert!(s.app.archived_videos().is_empty());
        let job = s.app.jobs().into_iter().find(|job| job.id() == id).unwrap();
        assert!(!s.app.waits_archived(&job));
    }

    #[test]
    fn compressed_media_packs_and_unpacks_with_the_check_before_removal() {
        let s = setup();
        let (space, _, video) = filled(&s, "Space");
        let folder = space.folder.clone().unwrap();
        let video_folder = s.app.video_folder(video.id);
        let archive = folder.join(PACKED_VIDEOS);

        // A check that fails removes nothing; the project stays archived.
        s.disk.fail_next_check();
        let job = s.app.archive_project(space.id, true).unwrap().unwrap();
        assert_eq!(ended(&s.app, job).state(), JobState::Failed);
        assert_eq!(s.disk.files(&video_folder).unwrap(), ["script.json"]);
        assert!(!s.disk.exists(&archive));
        s.app.unarchive_project(space.id).unwrap();

        let job = s.app.archive_project(space.id, true).unwrap().unwrap();
        assert_eq!(ended(&s.app, job).state(), JobState::Done);
        assert_eq!(s.disk.archive(&archive).unwrap(), std::slice::from_ref(&video_folder));
        assert_eq!(s.disk.files(&video_folder), None);
        assert!(s.app.archived_projects().unwrap()[0].packed);

        let job = s.app.unarchive_project(space.id).unwrap().unwrap();
        assert_eq!(ended(&s.app, job).state(), JobState::Done);
        assert_eq!(s.disk.files(&video_folder).unwrap(), ["script.json"]);
        assert!(!s.disk.exists(&archive));
        assert!(s.app.projects().unwrap().iter().all(Project::is_active));
    }

    #[test]
    fn a_deleted_video_goes_to_the_bin_and_its_theme_may_start_another() {
        let mut s = setup();
        let (_, channel, video) = filled(&s, "Space");
        let folder = s.app.video_folder(video.id);
        let preview = s.app.delete_preview(Removable::Video(video.id)).unwrap();
        assert_eq!(preview.name, "Apollo");
        assert_eq!(preview.folder.as_deref(), Some(folder.as_path()));
        assert_eq!(preview.disposal, Disposal::Recycle);
        assert!(preview.blocks.is_empty());

        let deletion = s
            .app
            .deletion(Removable::Video(video.id), " apollo ", false)
            .unwrap();
        deletion.run().unwrap();
        assert_eq!(s.trash.recycled(), [folder]);
        assert!(s.app.video_projects(channel.id).unwrap().is_empty());

        let theme = s.app.themes.theme(video.theme).unwrap().unwrap();
        assert_eq!(theme.status(), ThemeStatus::Approved);
        assert!(
            s.app
                .themes_without_video(channel.id)
                .unwrap()
                .contains(&theme.id)
        );
        let again = s.app.restart_theme(theme.id).unwrap();
        assert_eq!(again.theme, theme.id);
        assert!(s.app.themes_without_video(channel.id).unwrap().is_empty());
        assert!(
            s.app.restart_theme(theme.id).is_err(),
            "it has a video again"
        );
    }

    #[test]
    fn the_dialog_says_when_a_folder_would_be_deleted_permanently() {
        let mut s = setup();
        let (space, _, video) = filled(&s, "Space");
        let folder = space.folder.clone().unwrap();
        s.trash.removable(&folder);
        let preview = s.app.delete_preview(Removable::Project(space.id)).unwrap();
        assert_eq!(
            preview.disposal,
            Disposal::Permanent(PermanentReason::RemovableDrive)
        );
        assert_eq!(preview.videos, 1);

        let (other, _, other_video) = filled(&s, "Sea");
        let other_folder = other.folder.clone().unwrap();
        s.disk.set_size(&other_folder, 5_000);
        s.trash.bin_room(4_000);
        let preview = s.app.delete_preview(Removable::Project(other.id)).unwrap();
        assert_eq!(preview.size, 5_000);
        assert_eq!(
            preview.disposal,
            Disposal::Permanent(PermanentReason::LargerThanBin)
        );
        // What Windows would delete at once is deleted at once.
        s.app
            .deletion(Removable::Project(other.id), "Sea", false)
            .unwrap()
            .run()
            .unwrap();
        assert_eq!(s.trash.removed(), [other_folder]);
        assert!(s.trash.recycled().is_empty());

        // And the user may choose it.
        let video_folder = s.app.video_folder(video.id);
        s.app
            .deletion(Removable::Video(video.id), "Apollo", true)
            .unwrap()
            .run()
            .unwrap();
        assert!(s.trash.removed().contains(&video_folder));
        assert!(s.app.themes.project(other_video.id).unwrap().is_none());
    }

    #[test]
    fn a_folder_that_will_not_go_deletes_nothing() {
        let mut s = setup();
        let (space, _, _) = filled(&s, "Space");
        s.trash.refuse(space.folder.as_deref().unwrap());
        let error = s
            .app
            .deletion(Removable::Project(space.id), "Space", false)
            .unwrap()
            .run()
            .unwrap_err();
        assert!(matches!(error, RemovalError::FolderNotRemoved(_)));
        assert_eq!(error.message(), Text::DeleteFolderNotRemoved);
        assert!(s.app.projects().unwrap().iter().any(|p| p.id == space.id));
    }

    #[test]
    fn a_folder_that_cannot_be_reached_is_left_alone() {
        let mut s = setup();
        let (space, _, _) = filled(&s, "Space");
        let folder = space.folder.clone().unwrap();
        s.disk.remove(&folder);
        s.app.check_folders();
        let preview = s.app.delete_preview(Removable::Project(space.id)).unwrap();
        assert_eq!(preview.folder, None);
        s.app
            .deletion(Removable::Project(space.id), "Space", false)
            .unwrap()
            .run()
            .unwrap();
        assert!(s.trash.recycled().is_empty() && s.trash.removed().is_empty());
        assert!(s.app.projects().unwrap().iter().all(|p| p.id != space.id));
        assert!(matches!(
            s.app.archive_project(ProjectId::new(), false),
            Err(RemovalError::NotFound)
        ));
    }

    #[test]
    fn costs_keep_their_totals_and_names_after_a_deletion() {
        let mut s = setup();
        let (space, channel, video) = filled(&s, "Space");
        let (_, sea_channel, sea_video) = filled(&s, "Sea");
        cost(&s.app, channel.id, Some(video.id));
        cost(&s.app, channel.id, None);
        cost(&s.app, sea_channel.id, Some(sea_video.id));
        let month = s.app.current_month();
        let before = s.app.costs(month).unwrap();

        s.app
            .deletion(Removable::Video(sea_video.id), "Apollo", false)
            .unwrap()
            .run()
            .unwrap();
        s.app
            .deletion(Removable::Project(space.id), "Space", false)
            .unwrap()
            .run()
            .unwrap();

        let after = s.app.costs(month).unwrap();
        assert_eq!(after.total, before.total);
        assert_eq!(after.providers, before.providers);
        assert_eq!(after.projects.len(), 1);
        assert_eq!(after.deleted_projects.len(), 1);
        let deleted = &after.deleted_projects[0];
        assert_eq!(deleted.name.as_deref(), Some("Space"));
        assert_eq!(deleted.amount, Money::parse("2.50").unwrap());
        assert!(deleted.deleted.is_some());
        assert_eq!(after.videos.len(), 2);
        assert!(after.videos.iter().all(|row| row.deleted.is_some()));
        assert!(
            after
                .videos
                .iter()
                .all(|row| row.name.as_deref() == Some("Apollo"))
        );
        let space_channel = after
            .channels
            .iter()
            .find(|row| row.amount == Money::parse("2.50").unwrap());
        assert_eq!(
            space_channel.unwrap().name.as_deref(),
            Some("Space channel")
        );

        s.app.forget_deleted_name(space.id.as_uuid()).unwrap();
        let forgotten = s.app.costs(month).unwrap();
        assert_eq!(forgotten.total, before.total);
        let row = &forgotten.deleted_projects[0];
        assert_eq!(row.name, None);
        assert_eq!(
            row.deleted.as_ref().unwrap().short_id,
            bardo_domain::short_id(space.id.as_uuid())
        );
        assert_eq!(row.amount, Money::parse("2.50").unwrap());
        // Its video is forgotten with it; the other project's is not.
        let named: Vec<_> = forgotten
            .videos
            .iter()
            .filter_map(|row| row.name.clone())
            .collect();
        assert_eq!(named, ["Apollo"]);
    }
}
