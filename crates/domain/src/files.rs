//! Project files: the folder where a video project's media lives
//! (narration audio now; images, clips and renders later). The database
//! keeps file names; the files stay on disk where the user and other tools
//! can reach them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::VideoProjectId;

/// A file operation failed. The cause is kept for logs.
#[derive(Debug, thiserror::Error)]
#[error("project file {name}: {source}")]
pub struct ProjectFileError {
    pub name: String,
    pub source: std::io::Error,
}

/// The folder of each video project. Names are plain file names inside
/// it, never paths. Shared with job worker threads.
pub trait ProjectFiles: Send + Sync {
    /// Writes the whole file, replacing any file of that name. A reader
    /// never sees it half written.
    fn write(
        &self,
        project: VideoProjectId,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError>;

    fn read(&self, project: VideoProjectId, name: &str) -> Result<Vec<u8>, ProjectFileError>;

    /// Copies the file at `source`, outside the project, in as `name`,
    /// replacing any file of that name. The source is left as it was, and
    /// a reader never sees the copy half written.
    fn copy_in(
        &self,
        project: VideoProjectId,
        name: &str,
        source: &Path,
    ) -> Result<(), ProjectFileError>;

    fn exists(&self, project: VideoProjectId, name: &str) -> bool;

    /// Removes the file; a file already gone is not an error.
    fn remove(&self, project: VideoProjectId, name: &str) -> Result<(), ProjectFileError>;

    /// Where the file is, for players and tools that open it themselves.
    fn path(&self, project: VideoProjectId, name: &str) -> PathBuf;
}

impl<T: ProjectFiles + ?Sized> ProjectFiles for Arc<T> {
    fn write(
        &self,
        project: VideoProjectId,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError> {
        (**self).write(project, name, bytes)
    }

    fn read(&self, project: VideoProjectId, name: &str) -> Result<Vec<u8>, ProjectFileError> {
        (**self).read(project, name)
    }

    fn copy_in(
        &self,
        project: VideoProjectId,
        name: &str,
        source: &Path,
    ) -> Result<(), ProjectFileError> {
        (**self).copy_in(project, name, source)
    }

    fn exists(&self, project: VideoProjectId, name: &str) -> bool {
        (**self).exists(project, name)
    }

    fn remove(&self, project: VideoProjectId, name: &str) -> Result<(), ProjectFileError> {
        (**self).remove(project, name)
    }

    fn path(&self, project: VideoProjectId, name: &str) -> PathBuf {
        (**self).path(project, name)
    }
}
