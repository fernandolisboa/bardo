//! Project packages (issue #342, ADR-0047 item 4, ADR-0048): *Export
//! project* writes the open project to a `.bardo-project` file, *Import a
//! project package* brings one in as a new project, and *Open a project
//! from a folder* relinks a project Bardo knows or brings back a deleted
//! one from the records it left in its folder.
//!
//! Export and import are jobs, with progress and cancel: a package holds
//! the media, gigabytes at times. The records go in last, in one
//! transaction, so a cut-short import leaves no project behind.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bardo_domain::{
    FolderImport, FolderPlaces, FolderRepository, GLOBAL_DIR, Job, JobFailure, JobFailureKind,
    JobId, JobKind, PROJECT_MANIFEST, PackageError, PackageExport, PackageImport, PackageImported,
    PackageManifest, ProfileId, Progress, ProjectColor, ProjectDetails, ProjectDraft, ProjectId,
    ProjectPackages, ProjectRepository, TemplateKind, WORLD_DIR, free_name, leaves_room,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::folders::LocateError;
use crate::jobs::{JobContext, JobHandler};
use crate::projects::ProjectError;
use crate::{Bardo, Text};

/// Why a package cannot be written or brought in now.
#[derive(Debug, thiserror::Error)]
pub enum PackageProblem {
    #[error("project not found")]
    NotFound,
    /// The project's folder cannot be reached: its media cannot be read.
    #[error("the project's folder cannot be reached")]
    FolderNotFound,
    /// A package of it is being written already.
    #[error("an export is under way")]
    Busy,
    /// The name or the folder of the new project will not do.
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error(transparent)]
    Package(#[from] PackageError),
}

impl PackageProblem {
    /// What the dialog says.
    pub fn message(&self) -> Text {
        match self {
            PackageProblem::NotFound => Text::ProjectNotFoundAny,
            PackageProblem::FolderNotFound => Text::FolderNotFound,
            PackageProblem::Busy => Text::PackageExportBusy,
            PackageProblem::Project(error) => error.message(),
            PackageProblem::Package(error) => package_text(error),
        }
    }
}

/// What the import dialog says for a package that will not come in.
pub fn package_text(error: &PackageError) -> Text {
    match error {
        PackageError::NotAPackage => Text::PackageNotAPackage,
        PackageError::Newer => Text::PackageNewer,
        PackageError::Damaged(_) => Text::PackageDamaged,
        PackageError::FolderNotEmpty => Text::PackageFolderNotEmpty,
        PackageError::Stopped | PackageError::Database(_) | PackageError::Io(_) => {
            Text::PackageNotRead
        }
    }
}

/// A package the user picked, as the import dialog shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOffer {
    pub package: PathBuf,
    pub manifest: PackageManifest,
    /// The name it would get: its own, or numbered when the profile has a
    /// project of that name.
    pub name: String,
}

/// What the user chose in the import dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageChoice {
    /// The id the new project gets, so the dialog shows its folder as it
    /// will be made.
    pub id: ProjectId,
    pub package: PathBuf,
    pub name: String,
    pub color: ProjectColor,
    /// Where the new project's folder goes; the base folder when `None`.
    pub place: Option<PathBuf>,
    /// The carried global templates that come in as the project's own.
    pub templates: Vec<TemplateKind>,
}

/// What *Open a project from a folder* did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderOpened {
    /// A project Bardo knows, moved by the user, points at it now.
    Relinked(ProjectId),
    /// A deleted project's folder came back as a new project.
    Imported(PackageImported),
}

/// Why a folder does not open as a project.
#[derive(Debug, thiserror::Error)]
pub enum OpenFolderError {
    /// It has no `bardo-project.json` that reads.
    #[error("not a project folder")]
    NotAProjectFolder,
    /// It belongs to another data home or profile: its package is the way
    /// across.
    #[error("another copy's folder")]
    OtherCopy,
    /// It names a project Bardo does not know and holds no records to
    /// bring it back.
    #[error("no records in the folder")]
    NoRecords,
    #[error(transparent)]
    Locate(#[from] LocateError),
    #[error(transparent)]
    Package(#[from] PackageError),
}

impl OpenFolderError {
    pub fn message(&self) -> Text {
        match self {
            OpenFolderError::NotAProjectFolder => Text::OpenFolderNotAProject,
            OpenFolderError::OtherCopy => Text::OpenFolderOtherCopy,
            OpenFolderError::NoRecords => Text::OpenFolderNoRecords,
            OpenFolderError::Locate(error) => error.message(),
            OpenFolderError::Package(error) => package_text(error),
        }
    }
}

fn named(job: &Job, key: &str) -> Option<String> {
    serde_json::from_str::<Value>(job.payload())
        .ok()?
        .get(key)?
        .as_str()
        .map(str::to_owned)
}

impl Bardo {
    /// The export of `project` queued or running, if any.
    pub fn project_export_job(&self, project: ProjectId) -> Option<Job> {
        let id = project.to_string();
        self.jobs().into_iter().find(|job| {
            job.kind() == JobKind::ProjectExport
                && job.state().is_active()
                && named(job, "owner_project").as_deref() == Some(id.as_str())
        })
    }

    /// The imports queued or running.
    pub fn project_import_jobs(&self) -> Vec<Job> {
        self.jobs()
            .into_iter()
            .filter(|job| job.kind() == JobKind::ProjectImport && job.state().is_active())
            .collect()
    }

    /// Queues writing `project`'s package to `to`: the job.
    pub fn export_project_package(
        &self,
        project: ProjectId,
        to: PathBuf,
    ) -> Result<JobId, PackageProblem> {
        let found = self
            .projects
            .project(project)
            .map_err(ProjectError::from)?
            .filter(|found| found.owner == self.profile.id && found.is_active())
            .ok_or(PackageProblem::NotFound)?;
        self.check_folders();
        if self.unreachable_projects().contains(&found.id) {
            return Err(PackageProblem::FolderNotFound);
        }
        if self.project_export_job(project).is_some() {
            return Err(PackageProblem::Busy);
        }
        let payload = json!({
            "owner_project": project.to_string(),
            "to": to.to_string_lossy(),
        })
        .to_string();
        let job = self
            .jobs
            .enqueue(Job::new(self.profile.id, JobKind::ProjectExport, payload))
            .map_err(ProjectError::from)?;
        tracing::info!("queued a project export");
        Ok(job)
    }

    /// What the package at `path` holds, with the name it would come in
    /// as.
    pub fn inspect_package(&self, path: &Path) -> Result<PackageOffer, PackageProblem> {
        let manifest = self.folder_book.packages.inspect(path)?;
        let taken = self.project_names()?;
        let name = free_name(manifest.name.trim(), |candidate| {
            taken.contains(&candidate.to_lowercase())
        });
        Ok(PackageOffer {
            package: path.to_owned(),
            manifest,
            name,
        })
    }

    /// "Exported by Bardo 0.4.0 on November 27, 2026."
    pub fn package_from_text(&self, manifest: &PackageManifest) -> String {
        let date = self
            .catalog
            .date_here(&bardo_domain::local_time(manifest.exported_at, &self.zone));
        self.text_with(
            Text::ImportPackageFrom,
            &[("version", &manifest.app_version), ("date", &date)],
        )
    }

    /// What the package holds, in one line.
    pub fn package_contents_text(&self, manifest: &PackageManifest) -> String {
        let contents = &manifest.contents;
        self.text_with(
            Text::ImportPackageContents,
            &[
                ("channels", &contents.channels.to_string()),
                ("videos", &contents.videos.to_string()),
                ("world", &contents.world_items.to_string()),
                ("narrators", &contents.narrators.to_string()),
                ("products", &contents.products.to_string()),
                ("size", &self.size_text(contents.media_bytes)),
            ],
        )
    }

    fn project_names(&self) -> Result<Vec<String>, ProjectError> {
        Ok(self
            .projects()?
            .iter()
            .map(|project| project.name().to_lowercase())
            .collect())
    }

    /// Queues bringing the package in as a new project, under the name and
    /// in the folder chosen: the new project's id, which it has once the
    /// job is done.
    pub fn import_package(&self, choice: PackageChoice) -> Result<ProjectId, PackageProblem> {
        let details = ProjectDetails::validate(ProjectDraft {
            name: choice.name,
            color: choice.color,
        })
        .map_err(ProjectError::Invalid)?;
        if self
            .projects()
            .map_err(ProjectError::from)?
            .iter()
            .any(|other| other.details.same_name(details.name()))
        {
            return Err(ProjectError::NameTaken.into());
        }
        // A name another import queued already is taken too.
        let queued = self.project_import_jobs();
        if queued.iter().any(|job| {
            named(job, "name")
                .is_some_and(|name| name.to_lowercase() == details.name().to_lowercase())
        }) {
            return Err(ProjectError::NameTaken.into());
        }
        let project = choice.id;
        let folder = self.new_project_folder(details.name(), project, choice.place.as_deref());
        let queued_folder = queued
            .iter()
            .any(|job| named(job, "folder").as_deref() == Some(&*folder.to_string_lossy()));
        if queued_folder || self.folder_book.disk.exists(&folder) {
            return Err(ProjectError::FolderNotMade.into());
        }
        if !leaves_room(&folder) {
            return Err(ProjectError::FolderTooDeep.into());
        }
        // Read now, so a package that will not come in says so at once.
        self.folder_book.packages.inspect(&choice.package)?;
        let payload = json!({
            "package": choice.package.to_string_lossy(),
            "project": project.to_string(),
            "name": details.name(),
            "color": details.color().code(),
            "folder": folder.to_string_lossy(),
            "templates": choice.templates.iter().map(|kind| kind.code()).collect::<Vec<_>>(),
        })
        .to_string();
        self.jobs
            .enqueue(Job::new(self.profile.id, JobKind::ProjectImport, payload))
            .map_err(ProjectError::from)?;
        tracing::info!("queued a project import");
        Ok(project)
    }

    /// Opens the project of `folder`: relinks the project Bardo knows it
    /// for, or brings a deleted project back from the records it left.
    pub fn open_project_folder(&self, folder: &Path) -> Result<FolderOpened, OpenFolderError> {
        // The file itself may be what the user picked.
        let folder = match folder.file_name() {
            Some(name) if name == PROJECT_MANIFEST => folder.parent().unwrap_or(folder),
            _ => folder,
        };
        let manifest = self
            .folder_book
            .disk
            .manifest(folder)
            .ok_or(OpenFolderError::NotAProjectFolder)?;
        if manifest.profile != self.profile.id
            || manifest.data_home != self.folder_book.places.data_home
        {
            return Err(OpenFolderError::OtherCopy);
        }
        let known = self
            .projects
            .project(manifest.project)
            .map_err(|_| LocateError::NotSaved)?
            .filter(|project| project.owner == self.profile.id);
        if let Some(project) = known {
            self.locate_project_folder(project.id, folder)?;
            return Ok(FolderOpened::Relinked(project.id));
        }
        let records = self
            .folder_book
            .packages
            .folder_records(folder)
            .ok_or(OpenFolderError::NoRecords)?;
        let imported = self.folder_book.packages.import_folder(&FolderImport {
            folder: folder.to_owned(),
            owner: self.profile.id,
            project: ProjectId::new(),
            name: records.name,
            data_home: self.folder_book.places.data_home.clone(),
        })?;
        self.check_folders();
        Ok(FolderOpened::Imported(imported))
    }

    /// The handlers of the export and import jobs.
    pub(crate) fn package_handlers(
        owner: ProfileId,
        book: &crate::folders::FolderBook,
        projects: Arc<dyn ProjectRepository>,
    ) -> (ProjectExportHandler, ProjectImportHandler) {
        (
            ProjectExportHandler {
                owner,
                packages: Arc::clone(&book.packages),
                projects,
                folders: Arc::clone(&book.folders),
                places: book.places.clone(),
            },
            ProjectImportHandler {
                owner,
                packages: Arc::clone(&book.packages),
                data_home: book.places.data_home.clone(),
            },
        )
    }
}

fn unexpected(error: &dyn std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// Reports bytes done as the job's progress; stops when told.
fn progress_of(cx: &JobContext) -> impl FnMut(u64, u64) -> bool + '_ {
    move |done: u64, total: u64| {
        let permille = done.saturating_mul(1000).checked_div(total).unwrap_or(0);
        cx.report_progress(Progress::from_permille(permille.min(999) as u16));
        !cx.should_stop()
    }
}

fn failure(error: PackageError, what: &str) -> JobFailure {
    JobFailure::new(
        JobFailureKind::Files,
        format!("could not {what} the project package: {error}"),
    )
}

/// Writes a project package (`JobKind::ProjectExport`).
pub(crate) struct ProjectExportHandler {
    owner: ProfileId,
    packages: Arc<dyn ProjectPackages>,
    projects: Arc<dyn ProjectRepository>,
    folders: Arc<dyn FolderRepository>,
    places: FolderPlaces,
}

impl JobHandler for ProjectExportHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: Value = serde_json::from_str(payload).map_err(|e| unexpected(&e))?;
        let id = payload
            .get("owner_project")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .map(ProjectId::from)
            .ok_or_else(|| unexpected(&"no project in the payload"))?;
        let to = payload
            .get("to")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| unexpected(&"no file in the payload"))?;
        // Deleted since: nothing to write.
        let Some(project) = self.projects.project(id).map_err(|e| unexpected(&e))? else {
            return Ok(());
        };
        let mut videos = Vec::new();
        for video in self.folders.videos_in(&[id]).map_err(|e| unexpected(&e))? {
            if let Some(path) = self
                .folders
                .video_folder(video)
                .map_err(|e| unexpected(&e))?
                .and_then(|folder| folder.path())
            {
                videos.push((video, path));
            }
        }
        let chosen = self
            .folders
            .base_folder(self.owner)
            .map_err(|e| unexpected(&e))?;
        let request = PackageExport {
            project: id,
            to,
            videos,
            world: project.folder.as_ref().map(|folder| folder.join(WORLD_DIR)),
            global_world: self
                .places
                .base(chosen.as_deref())
                .join(GLOBAL_DIR)
                .join(WORLD_DIR),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        let cx = &*cx;
        match self.packages.export(&request, &mut progress_of(cx)) {
            Ok(_) => {
                cx.report_progress(Progress::DONE);
                Ok(())
            }
            Err(PackageError::Stopped) => Ok(()),
            Err(error) => Err(failure(error, "write")),
        }
    }
}

/// Brings a project package in (`JobKind::ProjectImport`).
pub(crate) struct ProjectImportHandler {
    owner: ProfileId,
    packages: Arc<dyn ProjectPackages>,
    data_home: String,
}

impl JobHandler for ProjectImportHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: Value = serde_json::from_str(payload).map_err(|e| unexpected(&e))?;
        let text = |key: &str| {
            payload
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| unexpected(&format!("no {key} in the payload")))
        };
        let project = Uuid::parse_str(&text("project")?)
            .map(ProjectId::from)
            .map_err(|e| unexpected(&e))?;
        let request = PackageImport {
            package: PathBuf::from(text("package")?),
            owner: self.owner,
            project,
            name: text("name")?,
            color: ProjectColor::from_code(&text("color")?),
            folder: PathBuf::from(text("folder")?),
            data_home: self.data_home.clone(),
            templates: payload
                .get("templates")
                .and_then(Value::as_array)
                .map(|kinds| {
                    kinds
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(|code| code.parse().ok())
                        .collect()
                })
                .unwrap_or_default(),
        };
        let cx = &*cx;
        match self.packages.import(&request, &mut progress_of(cx)) {
            Ok(_) => {
                cx.report_progress(Progress::DONE);
                Ok(())
            }
            Err(PackageError::Stopped) => Ok(()),
            Err(error) => Err(failure(error, "import")),
        }
    }
}

#[cfg(test)]
mod tests;
