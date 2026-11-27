//! Video folders on disk. A video's media lives in its project's folder,
//! `<project folder>\Videos\<slug>-<short id>` (ADR-0047), as the
//! database says; until it is moved there, in the old projects folder,
//! `<root>\<video id>` (`%LOCALAPPDATA%\Bardo\projects` on Windows).

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bardo_domain::{ProjectFileError, ProjectFiles, VideoFolders, VideoProjectId};

use crate::StorageError;

/// `%LOCALAPPDATA%\Bardo\projects` on Windows (the platform's local data
/// directory elsewhere), under `Bardo Dev` for a copy built from source.
pub fn default_projects_dir() -> Result<PathBuf, StorageError> {
    Ok(crate::data_home().local()?.join("projects"))
}

/// Video folders: each where the database says, else under one root.
#[derive(Clone)]
pub struct LocalProjectFiles {
    root: PathBuf,
    folders: Option<Arc<dyn VideoFolders>>,
}

impl std::fmt::Debug for LocalProjectFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalProjectFiles")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl LocalProjectFiles {
    /// Every video's folder under `root`, as before project folders.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            folders: None,
        }
    }

    /// Each video's folder where `folders` says, else under `root`.
    pub fn with_folders(root: impl Into<PathBuf>, folders: Arc<dyn VideoFolders>) -> Self {
        Self {
            root: root.into(),
            folders: Some(folders),
        }
    }

    /// The old projects folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a video's media is in the old projects folder.
    pub fn old_folder(&self, project: VideoProjectId) -> PathBuf {
        self.root.join(project.to_string())
    }

    /// Where a video's media is now.
    pub fn folder(&self, project: VideoProjectId) -> PathBuf {
        self.folders
            .as_ref()
            .and_then(|folders| folders.video_folder_path(project))
            .unwrap_or_else(|| self.old_folder(project))
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

    fn open(
        &self,
        project: VideoProjectId,
        name: &str,
    ) -> Result<Box<dyn std::io::Read + Send>, ProjectFileError> {
        let name = checked(name)?;
        let file =
            std::fs::File::open(self.folder(project).join(name)).map_err(|e| error(name, e))?;
        Ok(Box::new(file))
    }

    fn copy_in(
        &self,
        project: VideoProjectId,
        name: &str,
        source: &Path,
    ) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        let folder = self.folder(project);
        std::fs::create_dir_all(&folder).map_err(|e| error(name, e))?;
        // Copied beside it, then renamed over it, as `write` does. Bytes
        // only, through a handle of its own: `fs::copy` would carry over a
        // read-only flag, and Windows syncs only a handle open for writing.
        let partial = folder.join(format!("{name}.partial"));
        let copied = std::fs::File::open(source)
            .and_then(|mut original| {
                let mut copy = std::fs::File::create(&partial)?;
                std::io::copy(&mut original, &mut copy)?;
                copy.sync_all()
            })
            .and_then(|()| std::fs::rename(&partial, folder.join(name)));
        if copied.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        copied.map_err(|e| error(name, e))
    }

    fn copy_from(
        &self,
        from: VideoProjectId,
        name: &str,
        project: VideoProjectId,
    ) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        self.copy_in(project, name, &self.folder(from).join(name))
    }

    fn exists(&self, project: VideoProjectId, name: &str) -> bool {
        checked(name).is_ok_and(|name| self.folder(project).join(name).is_file())
    }

    fn size(&self, project: VideoProjectId, name: &str) -> Result<u64, ProjectFileError> {
        let name = checked(name)?;
        std::fs::metadata(self.folder(project).join(name))
            .map(|metadata| metadata.len())
            .map_err(|e| error(name, e))
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

    fn open(
        &self,
        project: VideoProjectId,
        name: &str,
    ) -> Result<Box<dyn std::io::Read + Send>, ProjectFileError> {
        Ok(Box::new(std::io::Cursor::new(self.read(project, name)?)))
    }

    /// Reads the source from disk.
    fn copy_in(
        &self,
        project: VideoProjectId,
        name: &str,
        source: &Path,
    ) -> Result<(), ProjectFileError> {
        let bytes = std::fs::read(source).map_err(|e| error(name, e))?;
        self.write(project, name, &bytes)
    }

    fn copy_from(
        &self,
        from: VideoProjectId,
        name: &str,
        project: VideoProjectId,
    ) -> Result<(), ProjectFileError> {
        let bytes = self.read(from, name)?;
        self.write(project, name, &bytes)
    }

    fn size(&self, project: VideoProjectId, name: &str) -> Result<u64, ProjectFileError> {
        self.read(project, name).map(|bytes| bytes.len() as u64)
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

    struct OneMoved(VideoProjectId, PathBuf);

    impl VideoFolders for OneMoved {
        fn video_folder_path(&self, video: VideoProjectId) -> Option<PathBuf> {
            (video == self.0).then(|| self.1.clone())
        }
    }

    #[test]
    fn a_moved_video_is_read_from_its_project_folder_and_the_rest_from_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let moved = VideoProjectId::new();
        let other = VideoProjectId::new();
        let folder = dir
            .path()
            .join("Space")
            .join("Videos")
            .join("Apollo-01234567");
        let files = LocalProjectFiles::with_folders(
            dir.path().join("projects"),
            Arc::new(OneMoved(moved, folder.clone())),
        );
        files.write(moved, "script.json", b"moved").unwrap();
        files.write(other, "script.json", b"old").unwrap();
        assert_eq!(std::fs::read(folder.join("script.json")).unwrap(), b"moved");
        assert_eq!(
            std::fs::read(
                dir.path()
                    .join("projects")
                    .join(other.to_string())
                    .join("script.json")
            )
            .unwrap(),
            b"old"
        );
        assert_eq!(files.path(moved, "a.mp4"), folder.join("a.mp4"));
        assert_eq!(
            files.old_folder(moved),
            dir.path().join("projects").join(moved.to_string())
        );
    }

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
        assert_eq!(files.size(project, "narration.mp3").unwrap(), 6);
        assert!(files.size(other, "narration.mp3").is_err());
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
            assert!(files.size(project, name).is_err(), "{name}");
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
        assert_eq!(files.size(project, "a.mp3").unwrap(), 1);
        files.remove(project, "a.mp3").unwrap();
        assert!(!files.exists(project, "a.mp3"));
        assert!(files.write(project, "../a.mp3", b"a").is_err());
    }

    #[test]
    fn copying_in_leaves_the_original_and_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("My Song.mp3");
        std::fs::write(&original, b"tune").unwrap();
        let files = LocalProjectFiles::new(dir.path().join("projects"));
        let project = VideoProjectId::new();

        files.copy_in(project, "media-1.mp3", &original).unwrap();
        assert_eq!(files.read(project, "media-1.mp3").unwrap(), b"tune");
        assert_eq!(std::fs::read(&original).unwrap(), b"tune");
        let names: Vec<_> = std::fs::read_dir(files.path(project, "x").parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["media-1.mp3"]);

        // A read-only original gives a copy Bardo can still replace.
        let locked = dir.path().join("Locked.wav");
        std::fs::write(&locked, b"riff").unwrap();
        let mut permissions = std::fs::metadata(&locked).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&locked, permissions).unwrap();
        files.copy_in(project, "media-3.wav", &locked).unwrap();
        let copy = std::fs::metadata(files.path(project, "media-3.wav")).unwrap();
        assert!(!copy.permissions().readonly());
        assert_eq!(std::fs::read(&locked).unwrap(), b"riff");
        // Writable again, so the temporary folder goes away on Windows.
        let mut permissions = std::fs::metadata(&locked).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        std::fs::set_permissions(&locked, permissions).unwrap();

        let missing = dir.path().join("gone.mp3");
        assert!(files.copy_in(project, "media-2.mp3", &missing).is_err());
        assert!(!files.exists(project, "media-2.mp3"));
        assert!(files.copy_in(project, "../escape.mp3", &original).is_err());
    }
}
