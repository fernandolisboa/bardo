//! Project folders on disk: `<root>/<project id>/<file name>`. The root is
//! `%LOCALAPPDATA%\Bardo\projects` on Windows: media is large and belongs
//! to this machine, so it stays out of the roaming profile.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bardo_domain::{ProjectFileError, ProjectFiles, VideoProjectId};

use crate::StorageError;

/// `%LOCALAPPDATA%\Bardo\projects` on Windows (the platform's local data
/// directory elsewhere).
pub fn default_projects_dir() -> Result<PathBuf, StorageError> {
    let dir = dirs::data_local_dir().ok_or(StorageError::NoDataDir)?;
    Ok(dir.join("Bardo").join("projects"))
}

/// Project folders under one root directory.
#[derive(Debug, Clone)]
pub struct LocalProjectFiles {
    root: PathBuf,
}

impl LocalProjectFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn folder(&self, project: VideoProjectId) -> PathBuf {
        self.root.join(project.to_string())
    }
}

fn error(name: &str, source: std::io::Error) -> ProjectFileError {
    ProjectFileError {
        name: name.to_owned(),
        source,
    }
}

/// Names are plain file names; anything that could leave the folder is
/// refused.
fn checked(name: &str) -> Result<&str, ProjectFileError> {
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && Path::new(name).file_name().is_some_and(|n| n == name);
    if plain {
        Ok(name)
    } else {
        Err(error(
            name,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a plain file name"),
        ))
    }
}

impl ProjectFiles for LocalProjectFiles {
    fn write(
        &self,
        project: VideoProjectId,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        let folder = self.folder(project);
        std::fs::create_dir_all(&folder).map_err(|e| error(name, e))?;
        // Written beside it, then renamed over it: never half a file.
        let partial = folder.join(format!("{name}.partial"));
        let mut file = std::fs::File::create(&partial).map_err(|e| error(name, e))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| error(name, e))?;
        drop(file);
        std::fs::rename(&partial, folder.join(name)).map_err(|e| error(name, e))
    }

    fn read(&self, project: VideoProjectId, name: &str) -> Result<Vec<u8>, ProjectFileError> {
        let name = checked(name)?;
        std::fs::read(self.folder(project).join(name)).map_err(|e| error(name, e))
    }

    fn exists(&self, project: VideoProjectId, name: &str) -> bool {
        checked(name).is_ok_and(|name| self.folder(project).join(name).is_file())
    }

    fn remove(&self, project: VideoProjectId, name: &str) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        match std::fs::remove_file(self.folder(project).join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(error(name, e)),
            _ => Ok(()),
        }
    }

    fn path(&self, project: VideoProjectId, name: &str) -> PathBuf {
        self.folder(project).join(name)
    }
}

/// Project files kept in memory, for tests. Paths point nowhere.
#[derive(Debug, Default)]
pub struct MemoryProjectFiles {
    files: Mutex<HashMap<(VideoProjectId, String), Vec<u8>>>,
}

impl MemoryProjectFiles {
    /// The names of a project's files, sorted.
    pub fn names(&self, project: VideoProjectId) -> Vec<String> {
        let files = self.files.lock().unwrap_or_else(|p| p.into_inner());
        let mut names: Vec<_> = files
            .keys()
            .filter(|(p, _)| *p == project)
            .map(|(_, name)| name.clone())
            .collect();
        names.sort();
        names
    }
}

impl ProjectFiles for MemoryProjectFiles {
    fn write(
        &self,
        project: VideoProjectId,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        self.files
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert((project, name.to_owned()), bytes.to_vec());
        Ok(())
    }

    fn read(&self, project: VideoProjectId, name: &str) -> Result<Vec<u8>, ProjectFileError> {
        self.files
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&(project, name.to_owned()))
            .cloned()
            .ok_or_else(|| error(name, std::io::ErrorKind::NotFound.into()))
    }

    fn exists(&self, project: VideoProjectId, name: &str) -> bool {
        self.files
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&(project, name.to_owned()))
    }

    fn remove(&self, project: VideoProjectId, name: &str) -> Result<(), ProjectFileError> {
        self.files
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&(project, name.to_owned()));
        Ok(())
    }

    fn path(&self, project: VideoProjectId, name: &str) -> PathBuf {
        PathBuf::from("memory").join(project.to_string()).join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_live_in_a_folder_per_project() {
        let dir = tempfile::tempdir().unwrap();
        let files = LocalProjectFiles::new(dir.path().join("projects"));
        let project = VideoProjectId::new();
        let other = VideoProjectId::new();

        files.write(project, "narration.mp3", b"first").unwrap();
        files.write(project, "narration.mp3", b"second").unwrap();

        let path = files.path(project, "narration.mp3");
        assert_eq!(
            path,
            dir.path()
                .join("projects")
                .join(project.to_string())
                .join("narration.mp3")
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(files.read(project, "narration.mp3").unwrap(), b"second");
        assert!(files.exists(project, "narration.mp3"));
        assert!(!files.exists(other, "narration.mp3"));
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["narration.mp3"], "no partial file stays");

        files.remove(project, "narration.mp3").unwrap();
        assert!(!files.exists(project, "narration.mp3"));
        files.remove(project, "narration.mp3").unwrap();
        assert!(files.read(project, "narration.mp3").is_err());
    }

    #[test]
    fn names_cannot_leave_the_project_folder() {
        let dir = tempfile::tempdir().unwrap();
        let files = LocalProjectFiles::new(dir.path());
        let project = VideoProjectId::new();
        for name in ["", ".", "..", "../x.mp3", "a/b.mp3", "a\\b.mp3", "C:x.mp3"] {
            assert!(files.write(project, name, b"x").is_err(), "{name}");
            assert!(files.read(project, name).is_err(), "{name}");
            assert!(!files.exists(project, name), "{name}");
        }
    }

    #[test]
    fn memory_files_behave_alike() {
        let files = MemoryProjectFiles::default();
        let project = VideoProjectId::new();
        files.write(project, "b.mp3", b"b").unwrap();
        files.write(project, "a.mp3", b"a").unwrap();
        assert_eq!(files.names(project), ["a.mp3", "b.mp3"]);
        assert_eq!(files.read(project, "a.mp3").unwrap(), b"a");
        files.remove(project, "a.mp3").unwrap();
        assert!(!files.exists(project, "a.mp3"));
        assert!(files.write(project, "../a.mp3", b"a").is_err());
    }
}
