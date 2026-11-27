//! Project folders from the app (issue #335, ADR-0047 item 3): the base
//! folder, a folder per project made when the project is, where each
//! video's media is, the job that moves media into its project's folder,
//! and a folder that cannot be reached.
//!
//! **A folder that cannot be reached** (moved, renamed, a drive not
//! plugged in) is one whose `bardo-project.json` does not name its
//! project. Until the user locates it, no job of that project starts, in
//! the app or the background agent (`FolderWatch` gates the job queue):
//! its files would read as missing, and a portrait that will not read is
//! let go and drawn again (ADR-0035). A job held back stays queued and
//! starts once the folder is back.
//!
//! **Moving media** (`JobKind::MoveMedia`) takes the database hold that a
//! restore takes (ADR-0010), so no other job starts here and the agent
//! stops, waits for what still runs, then moves each video's folder: a
//! rename on one volume, a copy checked before the original goes across
//! two. A video moved is recorded at once, so a run cut short resumes
//! with the rest.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ChannelId, ChannelRepository, FolderMoveError, FolderPlaces, FolderRepository, Job, JobFailure,
    JobFailureKind, JobId, JobKind, JobRepository, PROJECT_MANIFEST, Progress, Project,
    ProjectFolders, ProjectId, ProjectManifest, ProjectRepository, RUNNER_ALIVE, RemovalRepository,
    RepositoryError, RunnerRole, Trash, VIDEOS_DIR, VideoProjectId, leaves_room,
    project_folder_name,
};

use crate::jobs::{JobContext, JobGate, JobHandler};
use crate::projects::ProjectError;
use crate::{AppError, Bardo, Text};

/// How long the move's hold on the database lasts without being renewed.
pub const MOVE_HOLD: Duration = Duration::from_secs(2 * 60);

/// A set of projects, and the videos and channels that come with them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Held {
    projects: HashSet<ProjectId>,
    /// Videos whose media or channel is in one of those projects.
    videos: HashSet<VideoProjectId>,
    channels: HashSet<ChannelId>,
}

impl Held {
    /// Whether `job` is for one of these projects, by its origin or by the
    /// video or channel its payload names.
    fn holds(&self, job: &Job) -> bool {
        if self.projects.is_empty() && self.videos.is_empty() {
            return false;
        }
        if job
            .origin()
            .project
            .is_some_and(|project| self.projects.contains(&project))
        {
            return true;
        }
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(job.payload()) else {
            return false;
        };
        let named = |key: &str| {
            payload
                .get(key)
                .and_then(serde_json::Value::as_str)
                .and_then(|id| uuid::Uuid::parse_str(id).ok())
        };
        named("project").is_some_and(|id| self.videos.contains(&VideoProjectId::from(id)))
            || named("channel").is_some_and(|id| self.channels.contains(&ChannelId::from(id)))
            || named("owner_project").is_some_and(|id| self.projects.contains(&ProjectId::from(id)))
    }
}

/// What holds jobs back: projects whose folder cannot be reached, and
/// archived projects and videos (issue #336).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Unreachable {
    projects: HashSet<ProjectId>,
    /// Videos whose media or channel is in one of those projects.
    videos: HashSet<VideoProjectId>,
    channels: HashSet<ChannelId>,
    /// Archived projects, with their videos and channels, and archived
    /// videos.
    archived: Held,
}

impl Unreachable {
    fn unreachable(&self) -> Held {
        Held {
            projects: self.projects.clone(),
            videos: self.videos.clone(),
            channels: self.channels.clone(),
        }
    }
}

/// Whether a job of `kind` waits while its project or video is archived:
/// every one but the metrics sync (published posts go on being read) and
/// the jobs that pack and unpack an archived project's media.
pub(crate) fn waits_while_archived(kind: JobKind) -> bool {
    !matches!(
        kind,
        JobKind::MetricsSync | JobKind::PackMedia | JobKind::UnpackMedia
    )
}

/// Whether each project's folder can be reached, read again every tick of
/// the job queue and whenever the app asks.
pub(crate) struct FolderWatch {
    owner: bardo_domain::ProfileId,
    data_home: String,
    projects: Arc<dyn ProjectRepository>,
    channels: Arc<dyn ChannelRepository>,
    folders: Arc<dyn FolderRepository>,
    removals: Arc<dyn RemovalRepository>,
    disk: Arc<dyn ProjectFolders>,
    state: RwLock<Unreachable>,
}

impl FolderWatch {
    /// Whether `project`'s folder is there and names it. A project with no
    /// folder yet keeps its media in the old projects folder: reachable.
    fn reaches(&self, project: &Project) -> bool {
        project.folder.as_ref().is_none_or(|folder| {
            self.disk
                .manifest(folder)
                .is_some_and(|manifest| manifest.names(project.id, self.owner, &self.data_home))
        })
    }

    /// Reads every project's folder again. True when what is reachable
    /// changed.
    pub(crate) fn check(&self) -> Result<bool, RepositoryError> {
        let projects: HashSet<ProjectId> = self
            .projects
            .projects(self.owner)?
            .iter()
            .filter(|project| !self.reaches(project))
            .map(|project| project.id)
            .collect();
        let mut unreachable = Unreachable::default();
        if !projects.is_empty() {
            let ids: Vec<ProjectId> = projects.iter().copied().collect();
            unreachable.videos = self.folders.videos_in(&ids)?.into_iter().collect();
            unreachable.channels = self
                .channels
                .list(self.owner)?
                .into_iter()
                .filter(|channel| projects.contains(&channel.project))
                .map(|channel| channel.id)
                .collect();
        }
        unreachable.projects = projects;
        let archived: HashSet<ProjectId> = self
            .projects
            .projects(self.owner)?
            .iter()
            .filter(|project| !project.is_active())
            .map(|project| project.id)
            .collect();
        let mut videos = self.removals.archived_videos(self.owner)?;
        if !archived.is_empty() {
            let ids: Vec<ProjectId> = archived.iter().copied().collect();
            videos.extend(self.folders.videos_in(&ids)?);
            unreachable.archived.channels = self
                .channels
                .list(self.owner)?
                .into_iter()
                .filter(|channel| archived.contains(&channel.project))
                .map(|channel| channel.id)
                .collect();
        }
        unreachable.archived.projects = archived;
        unreachable.archived.videos = videos;
        let mut state = self.state.write().unwrap_or_else(|p| p.into_inner());
        let changed = *state != unreachable;
        *state = unreachable;
        Ok(changed)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Unreachable> {
        self.state.read().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn unreachable(&self) -> HashSet<ProjectId> {
        self.read().projects.clone()
    }
}

impl FolderWatch {
    /// Whether `job` waits for a project folder that cannot be reached.
    pub(crate) fn waits_for_folder(&self, job: &Job) -> bool {
        self.read().unreachable().holds(job)
    }

    /// Whether `job` waits because its project or video is archived.
    pub(crate) fn waits_archived(&self, job: &Job) -> bool {
        waits_while_archived(job.kind()) && self.read().archived.holds(job)
    }
}

impl JobGate for FolderWatch {
    fn refresh(&self) {
        if let Err(error) = self.check() {
            tracing::warn!("could not check the project folders: {error}");
        }
    }

    fn waits(&self, job: &Job) -> bool {
        self.waits_for_folder(job) || self.waits_archived(job)
    }
}

/// The base folder, project folders and the media move.
pub(crate) struct FolderBook {
    pub(crate) folders: Arc<dyn FolderRepository>,
    pub(crate) disk: Arc<dyn ProjectFolders>,
    pub(crate) places: FolderPlaces,
    pub(crate) watch: Arc<FolderWatch>,
    /// Archived videos and deletions (issue #336).
    pub(crate) removals: Arc<dyn RemovalRepository>,
    /// Where a deleted folder goes.
    pub(crate) trash: Arc<dyn Trash>,
}

impl FolderBook {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        owner: bardo_domain::ProfileId,
        folders: Arc<dyn FolderRepository>,
        disk: Arc<dyn ProjectFolders>,
        places: FolderPlaces,
        projects: Arc<dyn ProjectRepository>,
        channels: Arc<dyn ChannelRepository>,
        removals: Arc<dyn RemovalRepository>,
        trash: Arc<dyn Trash>,
    ) -> Self {
        let watch = Arc::new(FolderWatch {
            owner,
            data_home: places.data_home.clone(),
            projects,
            channels,
            folders: Arc::clone(&folders),
            removals: Arc::clone(&removals),
            disk: Arc::clone(&disk),
            state: RwLock::default(),
        });
        watch.refresh();
        Self {
            folders,
            disk,
            places,
            watch,
            removals,
            trash,
        }
    }
}

/// The base folder as Settings › Storage shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseFolder {
    pub path: PathBuf,
    /// The user chose it; otherwise it is the default.
    pub chosen: bool,
    /// The default left Documents because a sync client keeps it in the
    /// cloud.
    pub avoided_sync: bool,
    /// It lies in a folder synced to the cloud.
    pub synced: bool,
}

/// Why *Locate…* refused a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LocateError {
    /// It holds no `bardo-project.json` that reads.
    #[error("not a project folder")]
    NotAProjectFolder,
    /// Its `bardo-project.json` names another project.
    #[error("another project's folder")]
    OtherProject,
    /// Its `bardo-project.json` names this project of another profile or
    /// data home (a source build's, or another user's).
    #[error("another copy of Bardo's folder")]
    OtherCopy,
    #[error("project not found")]
    NotFound,
    #[error("could not save")]
    NotSaved,
}

impl LocateError {
    pub const ALL: [LocateError; 5] = [
        LocateError::NotAProjectFolder,
        LocateError::OtherProject,
        LocateError::OtherCopy,
        LocateError::NotFound,
        LocateError::NotSaved,
    ];

    pub fn message(self) -> Text {
        Text::LocateRefused(self)
    }
}

impl Bardo {
    /// Where new projects' folders go.
    pub fn base_folder(&self) -> BaseFolder {
        let book = &self.folder_book;
        let chosen = book
            .folders
            .base_folder(self.profile.id)
            .unwrap_or_else(|error| {
                tracing::warn!("could not read the base folder: {error}");
                None
            });
        let default = book.places.default_base();
        let path = chosen.clone().unwrap_or_else(|| default.path.clone());
        BaseFolder {
            synced: book.places.is_synced(&path),
            avoided_sync: chosen.is_none() && default.avoided_sync,
            chosen: chosen.is_some(),
            path,
        }
    }

    /// Chooses where new projects' folders go; `None` goes back to the
    /// default. Projects made before keep their folders.
    pub fn set_base_folder(&self, folder: Option<PathBuf>) -> Result<(), AppError> {
        Ok(self
            .folder_book
            .folders
            .set_base_folder(self.profile.id, folder.as_deref())?)
    }

    /// The folder a new project called `name` gets in `place` (the base
    /// folder when `None`): its name made safe, with its short id added when
    /// the name is taken. `project` is the id the project will have.
    pub fn new_project_folder(
        &self,
        name: &str,
        project: ProjectId,
        place: Option<&Path>,
    ) -> PathBuf {
        let place = place.map_or_else(|| self.base_folder().path, Path::to_path_buf);
        let recorded: Vec<PathBuf> = self
            .projects
            .projects(self.profile.id)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|project| project.folder)
            .collect();
        let name = project_folder_name(name, project, |name| {
            let path = place.join(name);
            self.folder_book.disk.exists(&path) || recorded.contains(&path)
        });
        place.join(name)
    }

    /// Makes `project`'s folder in `place` (the base folder when `None`)
    /// and points the project at it, before it is saved.
    pub(crate) fn make_project_folder(
        &self,
        project: &mut Project,
        place: Option<&Path>,
    ) -> Result<(), ProjectError> {
        let folder = self.new_project_folder(project.name(), project.id, place);
        if !leaves_room(&folder) {
            return Err(ProjectError::FolderTooDeep);
        }
        let manifest = ProjectManifest {
            project: project.id,
            name: project.name().to_owned(),
            profile: self.profile.id,
            data_home: self.folder_book.places.data_home.clone(),
        };
        self.folder_book
            .disk
            .create(&folder, &manifest)
            .map_err(|error| {
                tracing::warn!("could not make the project's folder: {error}");
                ProjectError::FolderNotMade
            })?;
        project.folder = Some(folder);
        Ok(())
    }

    /// Gives each project without a folder one under the base folder, and
    /// queues the move of media that is not in its project's folder: the
    /// move queued or under way, if any. Runs as the app opens; what fails
    /// is tried again at the next start.
    pub fn prepare_folders(&self) -> Option<JobId> {
        let projects = match self.projects.projects(self.profile.id) {
            Ok(projects) => projects,
            Err(error) => {
                tracing::warn!("could not read the projects: {error}");
                return None;
            }
        };
        for mut project in projects.into_iter().filter(|p| p.folder.is_none()) {
            if self.make_project_folder(&mut project, None).is_err() {
                continue;
            }
            if let Some(folder) = &project.folder
                && let Err(error) = self
                    .folder_book
                    .folders
                    .set_project_folder(project.id, folder)
            {
                tracing::warn!("could not record the project's folder: {error}");
            }
        }
        self.check_folders();
        self.queue_media_move()
    }

    /// Queues the media move when a video's media is not in its project's
    /// folder and no move is queued or running: the move queued or under
    /// way, if any.
    pub(crate) fn queue_media_move(&self) -> Option<JobId> {
        if self.role != RunnerRole::App {
            return None;
        }
        // Only media that can go somewhere now: its project has a folder
        // and both ends are reachable. Else the move would hold the
        // database for nothing.
        let unreachable = self.folder_book.watch.unreachable();
        let waiting = match self.folder_book.folders.videos_to_move(self.profile.id) {
            Ok(videos) => videos.iter().any(|video| {
                !unreachable.contains(&video.to)
                    && video
                        .from
                        .as_ref()
                        .is_none_or(|from| !unreachable.contains(&from.project))
                    && self.project_folder(video.to).is_some()
            }),
            Err(error) => {
                tracing::warn!("could not read where the media is: {error}");
                return None;
            }
        };
        if let Some(job) = self.media_move().filter(|job| job.state().is_active()) {
            return Some(job.id());
        }
        if !waiting {
            return None;
        }
        let job = Job::new(self.profile.id, JobKind::MoveMedia, "{}");
        match self.jobs.enqueue(job) {
            Ok(id) => {
                tracing::info!("moving media into the project folders");
                Some(id)
            }
            Err(error) => {
                tracing::warn!("could not queue the media move: {error}");
                None
            }
        }
    }

    /// The newest media move.
    pub fn media_move(&self) -> Option<Job> {
        self.jobs()
            .into_iter()
            .rfind(|job| job.kind() == JobKind::MoveMedia)
    }

    /// Reads again which project folders can be reached, and starts the
    /// jobs that waited for one that is back.
    pub fn check_folders(&self) {
        match self.folder_book.watch.check() {
            Ok(true) => self.jobs.wake(),
            Ok(false) => {}
            Err(error) => tracing::warn!("could not check the project folders: {error}"),
        }
    }

    /// The projects whose folder cannot be reached, as of the last check.
    pub fn unreachable_projects(&self) -> HashSet<ProjectId> {
        self.folder_book.watch.unreachable()
    }

    /// Whether `job` waits for its project's folder.
    pub fn waits_for_folder(&self, job: &Job) -> bool {
        job.state() == bardo_domain::JobState::Queued
            && self.folder_book.watch.waits_for_folder(job)
    }

    /// Whether `job` waits because its project or video is archived.
    pub fn waits_archived(&self, job: &Job) -> bool {
        job.state() == bardo_domain::JobState::Queued && self.folder_book.watch.waits_archived(job)
    }

    /// The project's folder, for *Show folder*.
    pub fn project_folder(&self, project: ProjectId) -> Option<PathBuf> {
        self.projects.project(project).ok().flatten()?.folder
    }

    /// The folder of a video's media, for *Show folder*: in its project's
    /// folder, or the old projects folder before the move.
    pub fn video_folder(&self, video: VideoProjectId) -> PathBuf {
        match self.folder_book.folders.video_folder(video) {
            Ok(Some(folder)) => folder.path(),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!("could not read where the video's media is: {error}");
                None
            }
        }
        .unwrap_or_else(|| self.folder_book.places.old_media.join(video.to_string()))
    }

    /// Points `project` at `folder`, found again after it moved, once its
    /// `bardo-project.json` names this project of this profile and data
    /// home.
    pub fn locate_project_folder(
        &self,
        project: ProjectId,
        folder: &Path,
    ) -> Result<(), LocateError> {
        let found = self
            .projects
            .project(project)
            .map_err(|_| LocateError::NotSaved)?
            .filter(|found| found.owner == self.profile.id)
            .ok_or(LocateError::NotFound)?;
        // The file itself may be what the user picked.
        let folder = match folder.file_name() {
            Some(name) if name == PROJECT_MANIFEST => folder.parent().unwrap_or(folder),
            _ => folder,
        };
        let manifest = self
            .folder_book
            .disk
            .manifest(folder)
            .ok_or(LocateError::NotAProjectFolder)?;
        if manifest.project != found.id {
            return Err(LocateError::OtherProject);
        }
        if !manifest.names(
            found.id,
            self.profile.id,
            &self.folder_book.places.data_home,
        ) {
            return Err(LocateError::OtherCopy);
        }
        self.folder_book
            .folders
            .set_project_folder(found.id, folder)
            .map_err(|_| LocateError::NotSaved)?;
        tracing::info!("located a project's folder");
        self.check_folders();
        Ok(())
    }

    /// The media move's handler.
    pub(crate) fn move_media_handler(
        owner: bardo_domain::ProfileId,
        book: &FolderBook,
        projects: Arc<dyn ProjectRepository>,
        jobs: Arc<dyn JobRepository>,
        tick: Duration,
    ) -> MoveMediaHandler {
        MoveMediaHandler {
            owner,
            folders: Arc::clone(&book.folders),
            disk: Arc::clone(&book.disk),
            old_media: book.places.old_media.clone(),
            data_home: book.places.data_home.clone(),
            projects,
            jobs,
            tick,
        }
    }
}

/// Moves each video's media into its project's folder
/// (`JobKind::MoveMedia`).
pub(crate) struct MoveMediaHandler {
    owner: bardo_domain::ProfileId,
    folders: Arc<dyn FolderRepository>,
    disk: Arc<dyn ProjectFolders>,
    old_media: PathBuf,
    data_home: String,
    projects: Arc<dyn ProjectRepository>,
    jobs: Arc<dyn JobRepository>,
    /// How often it renews the hold and looks whether what runs ended.
    tick: Duration,
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

impl JobHandler for MoveMediaHandler {
    fn run(&self, _payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let cx = &*cx;
        // Other jobs end before the hold is taken, so a long render does
        // not keep every other job from starting meanwhile.
        loop {
            if !self.others_running(cx)? {
                break;
            }
            if !cx.sleep(self.tick) {
                return Ok(());
            }
        }
        self.renew_hold()?;
        let moved = self.wait_for_others(cx).and_then(|free| match free {
            true => self.move_all(cx),
            false => Ok(()),
        });
        if let Err(error) = self.jobs.release_hold() {
            tracing::warn!("could not release the database hold: {error}");
        }
        moved
    }
}

impl MoveMediaHandler {
    fn renew_hold(&self) -> Result<(), JobFailure> {
        let now = SystemTime::now();
        match self.jobs.hold(now, now + MOVE_HOLD) {
            Ok(true) => Ok(()),
            Ok(false) => Err(unexpected("another Bardo holds the database")),
            Err(error) => Err(unexpected(error)),
        }
    }

    /// Whether another job of this profile runs.
    fn others_running(&self, cx: &JobContext) -> Result<bool, JobFailure> {
        Ok(self
            .jobs
            .list(self.owner)
            .map_err(unexpected)?
            .iter()
            .any(|job| job.id() != cx.id() && job.state() == bardo_domain::JobState::Running))
    }

    /// Waits, under the hold, until no other job runs and the background
    /// agent stopped. False when told to stop.
    fn wait_for_others(&self, cx: &JobContext) -> Result<bool, JobFailure> {
        // Free twice in a row, a tick apart: a job whose start was decided
        // just before the hold was taken has saved itself running by then.
        let mut free_once = false;
        loop {
            self.renew_hold()?;
            let now = SystemTime::now();
            let running = self.others_running(cx)?;
            let agent = self
                .jobs
                .runners(now.checked_sub(RUNNER_ALIVE).unwrap_or(now))
                .map_err(unexpected)?
                .iter()
                .any(|runner| runner.role == RunnerRole::Agent);
            if !running && !agent {
                if free_once {
                    return Ok(true);
                }
                free_once = true;
            } else {
                free_once = false;
            }
            if !cx.sleep(self.tick) {
                return Ok(false);
            }
        }
    }

    /// The project's folder, when it is there and names the project.
    fn reachable(&self, project: ProjectId) -> Result<Option<PathBuf>, JobFailure> {
        let Some(project) = self.projects.project(project).map_err(unexpected)? else {
            return Ok(None);
        };
        Ok(project.folder.filter(|folder| {
            self.disk
                .manifest(folder)
                .is_some_and(|manifest| manifest.names(project.id, self.owner, &self.data_home))
        }))
    }

    fn move_all(&self, cx: &JobContext) -> Result<(), JobFailure> {
        let videos = self
            .folders
            .videos_to_move(self.owner)
            .map_err(unexpected)?;
        let total = videos.len().max(1) as u64;
        let mut renewed = SystemTime::now();
        let mut failed: Vec<String> = Vec::new();
        for (index, video) in videos.iter().enumerate() {
            if cx.should_stop() {
                return Ok(());
            }
            let index = index as u64;
            // A folder that cannot be reached waits for the next move.
            let Some(target) = self.reachable(video.to)? else {
                continue;
            };
            let from = match &video.from {
                None => self.old_media.join(video.video.to_string()),
                Some(from) => match self.reachable(from.project)? {
                    Some(folder) => folder.join(VIDEOS_DIR).join(&from.name),
                    None => continue,
                },
            };
            let name = video.folder_name();
            let to = target.join(VIDEOS_DIR).join(&name);
            if from != to {
                let moved = self.disk.move_folder(&from, &to, &mut |done, all| {
                    if renewed.elapsed().unwrap_or_default() >= self.tick
                        && self.renew_hold().is_ok()
                    {
                        renewed = SystemTime::now();
                    }
                    let within = done.saturating_mul(1000).checked_div(all).unwrap_or(0);
                    let permille = (index * 1000 + within) / total;
                    cx.report_progress(Progress::from_permille(permille.min(999) as u16));
                    !cx.should_stop()
                });
                match moved {
                    Ok(_) => {}
                    Err(FolderMoveError::Stopped) => return Ok(()),
                    Err(error) => {
                        tracing::warn!(video = %video.video, "could not move a video's media: {error}");
                        failed.push(video.title.clone());
                        continue;
                    }
                }
            }
            self.folders
                .set_video_folder(video.video, video.to, &name)
                .map_err(unexpected)?;
            cx.report_progress(Progress::from_permille(
                ((index + 1) * 1000 / total).min(1000) as u16,
            ));
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(JobFailure::new(
                JobFailureKind::Files,
                format!("could not move the media of: {}", failed.join(", ")),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Condvar, Mutex};
    use std::time::Instant;

    use bardo_domain::{
        ChannelDraft, FolderMove, JobState, ProfileId, ProjectColor, ProjectDraft, VideoProject,
    };
    use bardo_storage::{Database, MemoryProjectFolders};

    use super::*;
    use crate::jobs::JobSettings;
    use crate::{Repositories, testing};

    const PATIENCE: Duration = Duration::from_secs(10);

    /// Project folders in memory that can be held inside a move, and that
    /// note whether the database hold was taken whenever a folder moved.
    #[derive(Default)]
    struct Disk {
        inner: MemoryProjectFolders,
        db: Mutex<Option<Arc<Database>>>,
        held_while_moving: Mutex<Vec<bool>>,
        /// While true, a move waits inside, before it copies anything.
        blocked: Mutex<bool>,
        inside: Mutex<bool>,
        changed: Condvar,
    }

    impl Disk {
        fn block(&self, blocked: bool) {
            *self.blocked.lock().unwrap() = blocked;
            self.changed.notify_all();
        }

        fn wait_inside(&self) {
            let deadline = Instant::now() + PATIENCE;
            let mut inside = self.inside.lock().unwrap();
            while !*inside {
                assert!(Instant::now() < deadline, "the move never started");
                inside = self
                    .changed
                    .wait_timeout(inside, Duration::from_millis(10))
                    .unwrap()
                    .0;
            }
        }
    }

    impl ProjectFolders for Disk {
        fn create(&self, folder: &Path, manifest: &ProjectManifest) -> std::io::Result<()> {
            self.inner.create(folder, manifest)
        }

        fn manifest(&self, folder: &Path) -> Option<ProjectManifest> {
            self.inner.manifest(folder)
        }

        fn exists(&self, path: &Path) -> bool {
            self.inner.exists(path)
        }

        fn size(&self, path: &Path) -> std::io::Result<u64> {
            self.inner.size(path)
        }

        fn pack(
            &self,
            videos: &Path,
            archive: &Path,
            progress: &mut dyn FnMut(u64, u64) -> bool,
        ) -> Result<(), FolderMoveError> {
            self.inner.pack(videos, archive, progress)
        }

        fn unpack(
            &self,
            archive: &Path,
            videos: &Path,
            progress: &mut dyn FnMut(u64, u64) -> bool,
        ) -> Result<(), FolderMoveError> {
            self.inner.unpack(archive, videos, progress)
        }

        fn move_folder(
            &self,
            from: &Path,
            to: &Path,
            progress: &mut dyn FnMut(u64, u64) -> bool,
        ) -> Result<FolderMove, FolderMoveError> {
            if let Some(db) = &*self.db.lock().unwrap() {
                let held = JobRepository::holder(&**db, SystemTime::now())
                    .unwrap()
                    .is_some();
                self.held_while_moving.lock().unwrap().push(held);
            }
            *self.inside.lock().unwrap() = true;
            self.changed.notify_all();
            let mut blocked = self.blocked.lock().unwrap();
            while *blocked {
                blocked = self.changed.wait(blocked).unwrap();
            }
            drop(blocked);
            self.inner.move_folder(from, to, progress)
        }
    }

    struct World {
        db: Arc<Database>,
        disk: Arc<Disk>,
    }

    impl World {
        fn new() -> Self {
            let db = Arc::new(Database::open_in_memory().unwrap());
            let disk = Arc::new(Disk::default());
            *disk.db.lock().unwrap() = Some(Arc::clone(&db));
            Self { db, disk }
        }

        fn start(&self) -> Bardo {
            let repositories = Repositories {
                project_folders: Arc::clone(&self.disk) as _,
                ..Repositories::shared(
                    Arc::clone(&self.db),
                    Arc::new(bardo_storage::MemorySecretStore::default()),
                )
            };
            Bardo::start_with(
                repositories,
                testing::providers(),
                Some("en-US"),
                JobSettings {
                    tick: Duration::from_millis(20),
                    ..JobSettings::default()
                },
            )
            .unwrap()
        }
    }

    fn base() -> PathBuf {
        crate::test_places().default_base().path
    }

    fn draft(name: &str) -> ProjectDraft {
        ProjectDraft {
            name: name.into(),
            color: ProjectColor::Teal,
        }
    }

    fn video(app: &Bardo, channel: ChannelId) -> VideoProject {
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel,
            bardo_domain::Niche::new("space history").unwrap(),
            bardo_domain::ThemeIdea::new("Apollo", "The last signal.").unwrap(),
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

    fn channel(app: &Bardo, name: &str) -> bardo_domain::Channel {
        app.create_channel(ChannelDraft {
            name: name.into(),
            ..ChannelDraft::default()
        })
        .unwrap()
    }

    fn wait_for(app: &Bardo, what: &str, matches: impl Fn(&Bardo) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !matches(app) {
            assert!(Instant::now() < deadline, "never: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn the_move(app: &Bardo) -> Job {
        app.media_move().expect("a media move")
    }

    /// A channel and a video made while the project could have no folder
    /// (the base folder was too deep), as before project folders.
    fn legacy_video(world: &World) -> (VideoProject, ProjectId) {
        let app = world.start();
        app.set_base_folder(Some(PathBuf::from(format!("/{}", "deep/".repeat(40)))))
            .unwrap();
        let channel = channel(&app, "Space EN");
        let video = video(&app, channel.id);
        assert_eq!(app.project_folder(channel.project), None);
        app.set_base_folder(None).unwrap();
        world.disk.inner.put(
            &crate::test_places().old_media.join(video.id.to_string()),
            &["script.json", "render-1.mp4"],
        );
        (video, channel.project)
    }

    #[test]
    fn a_new_project_gets_its_folder_named_for_it_with_its_manifest() {
        let world = World::new();
        let app = world.start();
        let project = app
            .create_project(draft("Space: the <final> one."))
            .unwrap();
        let folder = base().join("Space_ the _final_ one_");
        assert_eq!(project.folder.as_ref(), Some(&folder));
        assert_eq!(app.project_folder(project.id), Some(folder.clone()));
        assert!(world.disk.exists(&folder.join(VIDEOS_DIR)));
        assert!(world.disk.exists(&folder.join(bardo_domain::WORLD_DIR)));
        let manifest = world.disk.manifest(&folder).unwrap();
        assert!(manifest.names(project.id, app.profile().id, "Bardo"));
        assert_eq!(manifest.name, "Space: the <final> one.");

        // Renaming never moves it.
        let mut renamed = project.clone();
        renamed.details = bardo_domain::ProjectDetails::validate(draft("Sea")).unwrap();
        app.projects.save_project(&renamed).unwrap();
        assert_eq!(app.project_folder(project.id), Some(folder));
    }

    #[test]
    fn a_taken_folder_name_gets_the_short_id_and_a_deep_place_is_refused() {
        let world = World::new();
        let app = world.start();
        world.disk.inner.put(&base().join("Space"), &[]);
        let project = app.create_project(draft("Space")).unwrap();
        let short = bardo_domain::short_id(project.id.as_uuid());
        assert_eq!(project.folder, Some(base().join(format!("Space-{short}"))));

        let deep = PathBuf::from(format!("/{}", "deep/".repeat(40)));
        let refused = app.create_project_in(draft("Sea"), ProjectId::new(), Some(&deep));
        assert!(matches!(refused, Err(ProjectError::FolderTooDeep)));
        assert!(app.projects().unwrap().iter().all(|p| p.name() != "Sea"));

        let place = PathBuf::from("/media/Other");
        let id = ProjectId::new();
        assert_eq!(
            app.new_project_folder("Sea", id, Some(&place)),
            place.join("Sea")
        );
        let sea = app
            .create_project_in(draft("Sea"), id, Some(&place))
            .unwrap();
        assert_eq!(sea.id, id);
        assert_eq!(sea.folder, Some(place.join("Sea")));
    }

    #[test]
    fn the_base_folder_is_the_default_until_chosen_and_says_when_synced() {
        let world = World::new();
        let app = world.start();
        let shown = app.base_folder();
        assert_eq!(shown.path, base());
        assert!(!shown.chosen && !shown.avoided_sync && !shown.synced);
        app.set_base_folder(Some(PathBuf::from("/media/Bardo")))
            .unwrap();
        let shown = app.base_folder();
        assert_eq!(shown.path, PathBuf::from("/media/Bardo"));
        assert!(shown.chosen);
        assert_eq!(
            app.create_project(draft("Space")).unwrap().folder,
            Some(PathBuf::from("/media/Bardo/Space"))
        );
    }

    #[test]
    fn old_media_moves_into_the_video_folder_holding_the_runner_throughout() {
        let world = World::new();
        let (video, project) = legacy_video(&world);
        let old = crate::test_places().old_media.join(video.id.to_string());
        world.disk.block(true);

        let app = world.start();
        let folder = app.project_folder(project).expect("given at start");
        assert!(world.disk.manifest(&folder).is_some());
        world.disk.wait_inside();
        // Nothing else starts while the media moves.
        let other = app
            .start_test_job(crate::jobs::TestJob {
                steps: 1,
                step: Duration::from_millis(1),
                fail_at_step: None,
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let waiting = app.jobs().into_iter().find(|j| j.id() == other).unwrap();
        assert_eq!(waiting.state(), JobState::Queued);
        world.disk.block(false);

        wait_for(&app, "the move ends", |app| {
            the_move(app).state() == JobState::Done
        });
        let to = folder
            .join(VIDEOS_DIR)
            .join(bardo_domain::video_folder_name("Apollo", video.id));
        assert_eq!(
            world.disk.inner.files(&to),
            Some(vec!["script.json".to_owned(), "render-1.mp4".to_owned()])
        );
        assert_eq!(world.disk.inner.files(&old), None);
        assert_eq!(app.video_folder(video.id), to);
        assert_eq!(world.disk.held_while_moving.lock().unwrap().clone(), [true]);
        wait_for(&app, "the hold is released", |_| {
            JobRepository::holder(&*world.db, SystemTime::now())
                .unwrap()
                .is_none()
        });
        wait_for(&app, "the other job runs", |app| {
            app.jobs()
                .iter()
                .any(|j| j.id() == other && j.state() == JobState::Done)
        });
        // Nothing is left to move: no move is queued at the next start.
        drop(app);
        let app = world.start();
        assert!(the_move(&app).state() == JobState::Done);
    }

    #[test]
    fn a_cancelled_move_keeps_the_media_where_it_was_and_resumes() {
        let world = World::new();
        let (video, _) = legacy_video(&world);
        let old = crate::test_places().old_media.join(video.id.to_string());
        world.disk.block(true);
        let app = world.start();
        world.disk.wait_inside();
        let id = the_move(&app).id();
        app.cancel_job(id).unwrap();
        world.disk.block(false);
        wait_for(&app, "the move stops and lets go", |app| {
            the_move(app).state() == JobState::Cancelled
                && JobRepository::holder(&*world.db, SystemTime::now())
                    .unwrap()
                    .is_none()
        });
        assert_eq!(world.disk.inner.files(&old).map(|f| f.len()), Some(2));
        assert_eq!(app.video_folder(video.id), old);

        app.retry_job(id).unwrap();
        wait_for(&app, "the move ends", |app| {
            the_move(app).state() == JobState::Done
        });
        assert_eq!(world.disk.inner.files(&old), None);
        assert_ne!(app.video_folder(video.id), old);
    }

    #[test]
    fn a_moved_channel_s_media_follows_it_into_the_new_project_folder() {
        let world = World::new();
        let app = world.start();
        let channel = channel(&app, "Space EN");
        let video = video(&app, channel.id);
        let from = app.video_folder(video.id);
        assert!(from.starts_with(app.project_folder(channel.project).unwrap()));
        world.disk.inner.put(&from, &["script.json"]);
        let sea = app.create_project(draft("Sea")).unwrap();
        app.move_channel(channel.id, sea.id).unwrap();
        wait_for(&app, "the move ends", |app| {
            the_move(app).state() == JobState::Done
        });
        let to = sea
            .folder
            .unwrap()
            .join(VIDEOS_DIR)
            .join(from.file_name().unwrap());
        assert_eq!(app.video_folder(video.id), to);
        assert_eq!(
            world.disk.inner.files(&to),
            Some(vec!["script.json".to_owned()])
        );
    }

    #[test]
    fn jobs_of_a_project_whose_folder_is_gone_wait_until_it_is_located() {
        let world = World::new();
        let app = world.start();
        let channel = channel(&app, "Space EN");
        let video = video(&app, channel.id);
        let folder = app.project_folder(channel.project).unwrap();
        let manifest = world.disk.manifest(&folder).unwrap();
        world.disk.inner.remove(&folder);
        app.check_folders();
        assert!(app.unreachable_projects().contains(&channel.project));

        let job = Job::new(
            app.profile().id,
            JobKind::Countdown,
            format!(r#"{{"project":"{}","steps":1,"step_ms":1}}"#, video.id),
        );
        let id = app.jobs.enqueue(job).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        let waiting = app.jobs().into_iter().find(|j| j.id() == id).unwrap();
        assert_eq!(waiting.state(), JobState::Queued);
        assert!(app.waits_for_folder(&waiting));

        // Found elsewhere: only the folder that names it is taken.
        let moved = PathBuf::from("/media/Space");
        let other_project = ProjectManifest {
            project: ProjectId::new(),
            ..manifest.clone()
        };
        world.disk.inner.put_manifest(&moved, other_project);
        assert_eq!(
            app.locate_project_folder(channel.project, &moved),
            Err(LocateError::OtherProject)
        );
        let source_build = ProjectManifest {
            data_home: "Bardo Dev".into(),
            ..manifest.clone()
        };
        world.disk.inner.put_manifest(&moved, source_build);
        assert_eq!(
            app.locate_project_folder(channel.project, &moved),
            Err(LocateError::OtherCopy)
        );
        let other_profile = ProjectManifest {
            profile: ProfileId::new(),
            ..manifest.clone()
        };
        world.disk.inner.put_manifest(&moved, other_profile);
        assert_eq!(
            app.locate_project_folder(channel.project, &moved),
            Err(LocateError::OtherCopy)
        );
        assert_eq!(
            app.locate_project_folder(channel.project, Path::new("/media/Empty")),
            Err(LocateError::NotAProjectFolder)
        );
        world.disk.inner.put_manifest(&moved, manifest);
        app.locate_project_folder(channel.project, &moved.join(PROJECT_MANIFEST))
            .unwrap();
        assert_eq!(app.project_folder(channel.project), Some(moved.clone()));
        assert!(app.unreachable_projects().is_empty());
        assert!(app.video_folder(video.id).starts_with(&moved));
        wait_for(&app, "the job runs", |app| {
            app.jobs()
                .iter()
                .any(|j| j.id() == id && j.state() == JobState::Done)
        });
    }
}
