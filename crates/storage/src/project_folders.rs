//! Project folders on this computer (issue #335): made with their
//! `bardo-project.json`, recognized by it, and a video's folder moved
//! between them, renamed on one volume and copied, checked and removed
//! across two.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use bardo_domain::{
    FolderMove, FolderMoveError, FolderPlaces, PROJECT_MANIFEST, ProjectFolders, ProjectManifest,
    VIDEOS_DIR, WORLD_DIR,
};

/// How much is copied between two progress reports (and checks for stop).
const CHUNK: usize = 1024 * 1024;

/// A file being copied in, until it is whole and checked.
const COPYING: &str = ".moving";

/// The environment variables OneDrive sets to the folders it syncs.
const ONEDRIVE_VARIABLES: [&str; 3] = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"];

/// This computer's folders for `home`'s media; `old_media` is where each
/// video's media was before project folders (`default_projects_dir`).
pub fn folder_places(home: crate::DataHome, old_media: PathBuf) -> FolderPlaces {
    FolderPlaces {
        documents: dirs::document_dir(),
        home: dirs::home_dir().unwrap_or_default(),
        synced: ONEDRIVE_VARIABLES
            .iter()
            .filter_map(std::env::var_os)
            .filter(|folder| !folder.is_empty())
            .map(PathBuf::from)
            .collect(),
        data_home: home.folder().to_owned(),
        old_media,
    }
}

/// Project folders on this computer's disks.
#[derive(Debug, Clone, Default)]
pub struct LocalProjectFolders {
    /// Copies even where a rename would do, as across two volumes (tests).
    force_copy: bool,
}

impl LocalProjectFolders {
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves always copy, as between two volumes.
    pub fn copying() -> Self {
        Self { force_copy: true }
    }
}

/// Every file under `folder`, as paths relative to it, with their sizes.
fn files_under(folder: &Path) -> io::Result<Vec<(PathBuf, u64)>> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        for entry in fs::read_dir(folder.join(&relative))? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = relative.join(entry.file_name());
            if kind.is_dir() {
                pending.push(path);
            } else {
                found.push((path, entry.metadata()?.len()));
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Copies `from` to `to` in chunks, synced; false when told to stop.
fn copy_file(
    from: &Path,
    to: &Path,
    copied: &mut u64,
    total: u64,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> io::Result<bool> {
    let mut source = File::open(from)?;
    let mut copy = File::create(to)?;
    let mut buffer = vec![0; CHUNK];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copy.write_all(&buffer[..read])?;
        *copied += read as u64;
        if !progress(*copied, total) {
            return Ok(false);
        }
    }
    copy.sync_all()?;
    Ok(true)
}

impl ProjectFolders for LocalProjectFolders {
    fn create(&self, folder: &Path, manifest: &ProjectManifest) -> io::Result<()> {
        fs::create_dir_all(folder.join(VIDEOS_DIR))?;
        fs::create_dir_all(folder.join(WORLD_DIR))?;
        let partial = folder.join(format!("{PROJECT_MANIFEST}.partial"));
        fs::write(&partial, manifest.to_json())?;
        fs::rename(&partial, folder.join(PROJECT_MANIFEST))
    }

    fn manifest(&self, folder: &Path) -> Option<ProjectManifest> {
        let text = fs::read_to_string(folder.join(PROJECT_MANIFEST)).ok()?;
        ProjectManifest::parse(&text)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn move_folder(
        &self,
        from: &Path,
        to: &Path,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<FolderMove, FolderMoveError> {
        if !from.exists() {
            fs::create_dir_all(to)?;
            return Ok(FolderMove::Nothing);
        }
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        // A rename moves it whole on one volume. It fails across volumes,
        // and onto a folder a copy cut short left: then it is copied.
        if !self.force_copy && !to.exists() {
            match fs::rename(from, to) {
                Ok(()) => return Ok(FolderMove::Renamed),
                // A file in it is open (a preview playing, an antivirus
                // scan): copying would duplicate it all and still fail to
                // remove the original. Tried again at the next move.
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    return Err(error.into());
                }
                Err(_) => {}
            }
        }
        let files = files_under(from)?;
        let total: u64 = files.iter().map(|(_, size)| size).sum();
        let mut copied = 0;
        for (relative, size) in &files {
            let target = to.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            // Whole from a run cut short: copied before, never half, and
            // not changed since.
            let source_changed = fs::metadata(from.join(relative)).and_then(|meta| meta.modified());
            if fs::metadata(&target).is_ok_and(|meta| {
                meta.len() == *size
                    && matches!((meta.modified(), &source_changed), (Ok(copied), Ok(changed)) if copied >= *changed)
            }) {
                copied += size;
                if !progress(copied, total) {
                    return Err(FolderMoveError::Stopped);
                }
                continue;
            }
            let mut name = target.as_os_str().to_owned();
            name.push(COPYING);
            let partial = PathBuf::from(name);
            if !copy_file(&from.join(relative), &partial, &mut copied, total, progress)? {
                let _ = fs::remove_file(&partial);
                return Err(FolderMoveError::Stopped);
            }
            if fs::metadata(&partial)?.len() != *size {
                let _ = fs::remove_file(&partial);
                return Err(FolderMoveError::Mismatch(relative.display().to_string()));
            }
            fs::rename(&partial, &target)?;
        }
        // Every file is there and checked: the original can go.
        for (relative, size) in &files {
            if fs::metadata(to.join(relative))?.len() != *size {
                return Err(FolderMoveError::Mismatch(relative.display().to_string()));
            }
        }
        // The copy is whole: it is the media now, even if a file of the
        // original cannot go yet (one left open); what is left is only
        // a leftover in the old place.
        if let Err(error) = fs::remove_dir_all(from) {
            tracing::warn!("could not remove all of {}: {error}", from.display());
        }
        Ok(FolderMove::Copied)
    }

    fn size(&self, path: &Path) -> io::Result<u64> {
        match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => Ok(files_under(path)?.iter().map(|(_, size)| size).sum()),
            Ok(meta) => Ok(meta.len()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(error),
        }
    }

    fn pack(
        &self,
        videos: &Path,
        archive: &Path,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<(), FolderMoveError> {
        let mut packed: Vec<(String, PathBuf, u64)> = Vec::new();
        let mut folders = Vec::new();
        if videos.exists() {
            for entry in fs::read_dir(videos)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let folder = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                for (relative, size) in files_under(&folder)? {
                    packed.push((zip_entry(&name, &relative), folder.join(relative), size));
                }
                folders.push(folder);
            }
        }
        packed.sort();
        // Read twice: once written, once checked.
        let total: u64 = packed.iter().map(|(_, _, size)| size).sum::<u64>() * 2;
        let mut done = 0;
        let partial = with_suffix(archive, ".partial");
        let written = write_archive(&partial, &packed, &mut done, total, progress)
            .and_then(|()| check_archive(&partial, &packed, &mut done, total, progress));
        if let Err(error) = written {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
        fs::rename(&partial, archive)?;
        // Every file is in the archive and read back whole: the folders can
        // go. One that cannot (a file left open) stays beside the archive,
        // and unpacking writes over it.
        for folder in folders {
            if let Err(error) = fs::remove_dir_all(&folder) {
                tracing::warn!("could not remove all of {}: {error}", folder.display());
            }
        }
        Ok(())
    }

    fn unpack(
        &self,
        archive: &Path,
        videos: &Path,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<(), FolderMoveError> {
        let mut zip =
            zip::ZipArchive::new(io::BufReader::new(File::open(archive)?)).map_err(zip_failure)?;
        let mut entries: Vec<(usize, PathBuf, u64)> = Vec::new();
        for index in 0..zip.len() {
            let entry = zip.by_index_raw(index).map_err(zip_failure)?;
            if entry.is_dir() {
                continue;
            }
            let relative = entry.enclosed_name().ok_or_else(|| {
                FolderMoveError::Mismatch(
                    entry
                        .name()
                        .map(|name| name.into_owned())
                        .unwrap_or_default(),
                )
            })?;
            entries.push((index, relative, entry.size()));
        }
        let total: u64 = entries.iter().map(|(_, _, size)| size).sum();
        let mut done = 0;
        for (index, relative, size) in &entries {
            let target = videos.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let partial = with_suffix(&target, COPYING);
            let mut entry = zip.by_index(*index).map_err(zip_failure)?;
            let mut out = File::create(&partial)?;
            // Reading an entry to its end checks its CRC.
            let copied = copy_reader(&mut entry, &mut out, &mut done, total, progress);
            match copied {
                Ok(true) => {}
                Ok(false) => {
                    let _ = fs::remove_file(&partial);
                    return Err(FolderMoveError::Stopped);
                }
                Err(error) => {
                    let _ = fs::remove_file(&partial);
                    return Err(error.into());
                }
            }
            out.sync_all()?;
            drop(out);
            if fs::metadata(&partial)?.len() != *size {
                let _ = fs::remove_file(&partial);
                return Err(FolderMoveError::Mismatch(relative.display().to_string()));
            }
            fs::rename(&partial, &target)?;
        }
        drop(zip);
        fs::remove_file(archive)?;
        Ok(())
    }
}

/// `path` with `suffix` added to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The archive entry of a file in a video's folder: `<folder>/<path>`,
/// with forward slashes as zip wants.
fn zip_entry(folder: &str, relative: &Path) -> String {
    let parts: Vec<String> = std::iter::once(folder.to_owned())
        .chain(
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned()),
        )
        .collect();
    parts.join("/")
}

fn zip_failure(error: zip::result::ZipError) -> FolderMoveError {
    match error {
        zip::result::ZipError::Io(error) => FolderMoveError::Io(error),
        other => FolderMoveError::Io(io::Error::new(io::ErrorKind::InvalidData, other)),
    }
}

/// Copies `from` into `to` in chunks: false when told to stop.
fn copy_reader(
    from: &mut dyn Read,
    to: &mut dyn Write,
    done: &mut u64,
    total: u64,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> io::Result<bool> {
    let mut buffer = vec![0; CHUNK];
    loop {
        let read = from.read(&mut buffer)?;
        if read == 0 {
            return Ok(true);
        }
        to.write_all(&buffer[..read])?;
        *done += read as u64;
        if !progress(*done, total) {
            return Ok(false);
        }
    }
}

/// Writes `files` (entry, path, size) into a new zip at `path`, stored:
/// media is compressed already.
fn write_archive(
    path: &Path,
    files: &[(String, PathBuf, u64)],
    done: &mut u64,
    total: u64,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<(), FolderMoveError> {
    use zip::write::SimpleFileOptions;
    let mut zip = zip::ZipWriter::new(io::BufWriter::new(File::create(path)?));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (entry, file, size) in files {
        // Zip64 for anything near the 4 GB a plain entry holds.
        zip.start_file(entry.as_str(), stored.large_file(*size >= 1 << 31))
            .map_err(zip_failure)?;
        if !copy_reader(&mut File::open(file)?, &mut zip, done, total, progress)? {
            return Err(FolderMoveError::Stopped);
        }
    }
    let out = zip.finish().map_err(zip_failure)?;
    out.into_inner()
        .map_err(|error| error.into_error())?
        .sync_all()?;
    Ok(())
}

/// Reads every entry of the zip at `path` back (which checks its CRC) and
/// checks it holds each of `files` at its size.
fn check_archive(
    path: &Path,
    files: &[(String, PathBuf, u64)],
    done: &mut u64,
    total: u64,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<(), FolderMoveError> {
    let mut zip =
        zip::ZipArchive::new(io::BufReader::new(File::open(path)?)).map_err(zip_failure)?;
    for (entry, _, size) in files {
        let mut read = zip.by_name(entry).map_err(|error| match error {
            zip::result::ZipError::FileNotFound => FolderMoveError::Mismatch(entry.clone()),
            other => zip_failure(other),
        })?;
        let before = *done;
        if !copy_reader(&mut read, &mut io::sink(), done, total, progress)? {
            return Err(FolderMoveError::Stopped);
        }
        if *done - before != *size {
            return Err(FolderMoveError::Mismatch(entry.clone()));
        }
    }
    Ok(())
}

/// Project folders in memory, for tests: a folder is a set of file names.
#[derive(Debug, Default)]
pub struct MemoryProjectFolders {
    state: Mutex<MemoryFolders>,
}

#[derive(Debug, Default)]
struct MemoryFolders {
    folders: HashSet<PathBuf>,
    manifests: HashMap<PathBuf, ProjectManifest>,
    /// Files of a video folder, by folder.
    files: HashMap<PathBuf, Vec<String>>,
    /// What a packed archive holds: each video folder's path with its
    /// files.
    archives: HashMap<PathBuf, Vec<(PathBuf, Vec<String>)>>,
    /// Sizes set by a test; otherwise a folder weighs a byte per file.
    sizes: HashMap<PathBuf, u64>,
    /// The next pack or unpack fails its check.
    fail_check: bool,
}

/// Which database a set of memory folders goes with: a file by its path,
/// an in-memory database by its connection. The weak handle keeps a
/// dropped connection's address from being reused while it is listed.
enum DatabaseKey {
    File(PathBuf),
    Memory(Weak<Mutex<rusqlite::Connection>>),
}

static REGISTRY: Mutex<Vec<(DatabaseKey, Arc<MemoryProjectFolders>)>> = Mutex::new(Vec::new());

impl MemoryProjectFolders {
    /// The memory folders that go with `db`: the same ones each time the
    /// database is opened again, or seen by another runner, so a test that
    /// reopens Bardo finds the folders it made.
    pub fn for_database(db: &crate::Database) -> Arc<Self> {
        Self::listed(db, None)
    }

    /// Lets `to` find the memory folders of `from`, as when Bardo opens a
    /// restored database on the same computer: the folders stayed.
    pub fn keep_for(from: &crate::Database, to: &crate::Database) {
        let folders = Self::for_database(from);
        Self::listed(to, Some(folders));
    }

    fn listed(db: &crate::Database, given: Option<Arc<Self>>) -> Arc<Self> {
        let path = db.file_path();
        let conn = db.connection_handle();
        let mut registry = REGISTRY.lock().unwrap_or_else(|p| p.into_inner());
        registry.retain(|(key, _)| match key {
            DatabaseKey::File(_) => true,
            DatabaseKey::Memory(weak) => weak.strong_count() > 0,
        });
        let same = |key: &DatabaseKey| match (key, &path) {
            (DatabaseKey::File(listed), Some(path)) => listed == path,
            (DatabaseKey::Memory(weak), None) => std::ptr::eq(weak.as_ptr(), Arc::as_ptr(&conn)),
            _ => false,
        };
        if let Some(given) = given {
            registry.retain(|(key, _)| !same(key));
            registry.push((Self::key(path, &conn), Arc::clone(&given)));
            return given;
        }
        if let Some((_, folders)) = registry.iter().find(|(key, _)| same(key)) {
            return Arc::clone(folders);
        }
        let folders = Arc::new(Self::default());
        registry.push((Self::key(path, &conn), Arc::clone(&folders)));
        folders
    }

    fn key(path: Option<PathBuf>, conn: &Arc<Mutex<rusqlite::Connection>>) -> DatabaseKey {
        match path {
            Some(path) => DatabaseKey::File(path),
            None => DatabaseKey::Memory(Arc::downgrade(conn)),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, MemoryFolders> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Puts a video folder with these files in place.
    pub fn put(&self, folder: &Path, files: &[&str]) {
        let mut state = self.state();
        state.folders.insert(folder.to_owned());
        state.files.insert(
            folder.to_owned(),
            files.iter().map(|f| (*f).to_owned()).collect(),
        );
    }

    /// The files in a folder, or `None` without the folder.
    pub fn files(&self, folder: &Path) -> Option<Vec<String>> {
        let state = self.state();
        state
            .folders
            .contains(folder)
            .then(|| state.files.get(folder).cloned().unwrap_or_default())
    }

    /// The folder goes away, as a drive unplugged.
    pub fn remove(&self, folder: &Path) {
        let mut state = self.state();
        state.folders.retain(|path| !path.starts_with(folder));
        state.manifests.retain(|path, _| !path.starts_with(folder));
        state.files.retain(|path, _| !path.starts_with(folder));
    }

    /// Writes a manifest in a folder, as another Bardo would have.
    pub fn put_manifest(&self, folder: &Path, manifest: ProjectManifest) {
        let mut state = self.state();
        state.folders.insert(folder.to_owned());
        state.manifests.insert(folder.to_owned(), manifest);
    }

    /// `size` reports `bytes` for `path`.
    pub fn set_size(&self, path: &Path, bytes: u64) {
        self.state().sizes.insert(path.to_owned(), bytes);
    }

    /// The next pack or unpack finds a file that does not match: nothing
    /// is removed.
    pub fn fail_next_check(&self) {
        self.state().fail_check = true;
    }

    /// The video folders a packed archive holds, by path.
    pub fn archive(&self, archive: &Path) -> Option<Vec<PathBuf>> {
        self.state()
            .archives
            .get(archive)
            .map(|folders| folders.iter().map(|(path, _)| path.clone()).collect())
    }
}

impl ProjectFolders for MemoryProjectFolders {
    fn create(&self, folder: &Path, manifest: &ProjectManifest) -> io::Result<()> {
        let mut state = self.state();
        state.folders.insert(folder.to_owned());
        state.folders.insert(folder.join(VIDEOS_DIR));
        state.folders.insert(folder.join(WORLD_DIR));
        state.manifests.insert(folder.to_owned(), manifest.clone());
        Ok(())
    }

    fn manifest(&self, folder: &Path) -> Option<ProjectManifest> {
        self.state().manifests.get(folder).cloned()
    }

    fn exists(&self, path: &Path) -> bool {
        self.state().folders.contains(path)
    }

    fn move_folder(
        &self,
        from: &Path,
        to: &Path,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<FolderMove, FolderMoveError> {
        let mut state = self.state();
        if !state.folders.remove(from) {
            state.folders.insert(to.to_owned());
            return Ok(FolderMove::Nothing);
        }
        if !progress(1, 1) {
            state.folders.insert(from.to_owned());
            return Err(FolderMoveError::Stopped);
        }
        let files = state.files.remove(from).unwrap_or_default();
        state.folders.insert(to.to_owned());
        state.files.insert(to.to_owned(), files);
        Ok(FolderMove::Renamed)
    }

    fn size(&self, path: &Path) -> io::Result<u64> {
        let state = self.state();
        if let Some(bytes) = state.sizes.get(path) {
            return Ok(*bytes);
        }
        Ok(state
            .files
            .iter()
            .filter(|(folder, _)| folder.starts_with(path))
            .map(|(_, files)| files.len() as u64)
            .sum())
    }

    fn pack(
        &self,
        videos: &Path,
        archive: &Path,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<(), FolderMoveError> {
        let mut state = self.state();
        if !progress(1, 2) {
            return Err(FolderMoveError::Stopped);
        }
        if std::mem::take(&mut state.fail_check) {
            return Err(FolderMoveError::Mismatch("a packed file".into()));
        }
        let packed: Vec<PathBuf> = state
            .folders
            .iter()
            .filter(|folder| folder.parent() == Some(videos))
            .cloned()
            .collect();
        let held = packed
            .into_iter()
            .map(|folder| {
                state.folders.remove(&folder);
                let files = state.files.remove(&folder).unwrap_or_default();
                (folder, files)
            })
            .collect();
        state.archives.insert(archive.to_owned(), held);
        state.folders.insert(archive.to_owned());
        progress(2, 2);
        Ok(())
    }

    fn unpack(
        &self,
        archive: &Path,
        _videos: &Path,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<(), FolderMoveError> {
        let mut state = self.state();
        if !state.archives.contains_key(archive) {
            return Err(io::Error::from(io::ErrorKind::NotFound).into());
        }
        if !progress(1, 2) {
            return Err(FolderMoveError::Stopped);
        }
        if std::mem::take(&mut state.fail_check) {
            return Err(FolderMoveError::Mismatch("an unpacked file".into()));
        }
        for (folder, files) in state.archives.remove(archive).unwrap_or_default() {
            state.folders.insert(folder.clone());
            state.files.insert(folder, files);
        }
        state.folders.remove(archive);
        progress(2, 2);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{ProfileId, ProjectId};

    use super::*;

    fn manifest() -> ProjectManifest {
        ProjectManifest {
            project: ProjectId::new(),
            name: "Space".into(),
            profile: ProfileId::new(),
            data_home: "Bardo".into(),
        }
    }

    fn video_folder(root: &Path) -> PathBuf {
        let from = root.join("projects").join("video");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("script.json"), b"script").unwrap();
        fs::write(from.join("render-1.mp4"), vec![7u8; 3 * CHUNK + 5]).unwrap();
        from
    }

    #[test]
    fn a_project_folder_is_made_and_recognized() {
        let dir = tempfile::tempdir().unwrap();
        let folders = LocalProjectFolders::new();
        let folder = dir.path().join("Space");
        let manifest = manifest();
        folders.create(&folder, &manifest).unwrap();
        assert!(folder.join("Videos").is_dir() && folder.join("World").is_dir());
        assert_eq!(folders.manifest(&folder), Some(manifest));
        assert_eq!(folders.manifest(&dir.path().join("elsewhere")), None);
        fs::write(folder.join(PROJECT_MANIFEST), "not json").unwrap();
        assert_eq!(folders.manifest(&folder), None);
    }

    #[test]
    fn on_one_volume_a_video_folder_is_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let from = video_folder(dir.path());
        let to = dir
            .path()
            .join("Space")
            .join("Videos")
            .join("Apollo-01234567");
        let moved = LocalProjectFolders::new()
            .move_folder(&from, &to, &mut |_, _| true)
            .unwrap();
        assert_eq!(moved, FolderMove::Renamed);
        assert!(!from.exists());
        assert_eq!(fs::read(to.join("script.json")).unwrap(), b"script");
    }

    #[test]
    fn across_volumes_a_video_folder_is_copied_checked_then_removed() {
        let dir = tempfile::tempdir().unwrap();
        let from = video_folder(dir.path());
        let to = dir
            .path()
            .join("Other")
            .join("Videos")
            .join("Apollo-01234567");
        let mut reports = Vec::new();
        let moved = LocalProjectFolders::copying()
            .move_folder(&from, &to, &mut |done, total| {
                reports.push((done, total));
                true
            })
            .unwrap();
        assert_eq!(moved, FolderMove::Copied);
        assert!(!from.exists(), "the original goes once the copy is checked");
        assert_eq!(
            fs::read(to.join("render-1.mp4")).unwrap().len(),
            3 * CHUNK + 5
        );
        let total = 3 * CHUNK as u64 + 5 + 6;
        assert_eq!(reports.last(), Some(&(total, total)));
        assert!(reports.len() > 2, "progress as it copies");
        assert!(fs::read_dir(&to).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(COPYING)
        }));
    }

    #[test]
    fn a_copy_stopped_keeps_the_original_and_resumes_without_copying_twice() {
        let dir = tempfile::tempdir().unwrap();
        let from = video_folder(dir.path());
        let to = dir
            .path()
            .join("Other")
            .join("Videos")
            .join("Apollo-01234567");
        let folders = LocalProjectFolders::copying();
        // Stops in the middle of the big file (the files go by name).
        let stopped = folders.move_folder(&from, &to, &mut |done, _| done < 2 * CHUNK as u64);
        assert!(matches!(stopped, Err(FolderMoveError::Stopped)));
        assert!(from.join("render-1.mp4").exists() && from.join("script.json").exists());
        assert!(!to.join("render-1.mp4").exists(), "never half a file");

        let mut first = None;
        let moved = folders
            .move_folder(&from, &to, &mut |done, _| {
                first.get_or_insert(done);
                true
            })
            .unwrap();
        assert_eq!(moved, FolderMove::Copied);
        assert!(!from.exists());
        assert_eq!(fs::read(to.join("script.json")).unwrap(), b"script");
        assert_eq!(
            fs::read(to.join("render-1.mp4")).unwrap().len(),
            3 * CHUNK + 5
        );
    }

    #[test]
    fn a_video_without_media_moves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let to = dir
            .path()
            .join("Space")
            .join("Videos")
            .join("Apollo-01234567");
        let moved = LocalProjectFolders::new()
            .move_folder(&dir.path().join("missing"), &to, &mut |_, _| true)
            .unwrap();
        assert_eq!(moved, FolderMove::Nothing);
        assert!(to.is_dir());
    }

    #[test]
    fn a_rename_onto_a_partial_copy_copies_the_rest_instead() {
        let dir = tempfile::tempdir().unwrap();
        let from = video_folder(dir.path());
        let to = dir
            .path()
            .join("Space")
            .join("Videos")
            .join("Apollo-01234567");
        fs::create_dir_all(&to).unwrap();
        fs::write(to.join("script.json"), b"script").unwrap();
        let moved = LocalProjectFolders::new()
            .move_folder(&from, &to, &mut |_, _| true)
            .unwrap();
        assert_eq!(moved, FolderMove::Copied);
        assert!(!from.exists());
        assert!(to.join("render-1.mp4").exists());
    }

    /// A project's `Videos` folder with two videos' media.
    fn videos(root: &Path) -> PathBuf {
        let videos = root.join("Space").join("Videos");
        for (name, bytes) in [("Apollo-01234567", 3 * CHUNK + 5), ("Mir-89abcdef", 10)] {
            let folder = videos.join(name);
            fs::create_dir_all(folder.join("clips")).unwrap();
            fs::write(folder.join("script.json"), name.as_bytes()).unwrap();
            fs::write(folder.join("clips").join("1.mp4"), vec![3u8; bytes]).unwrap();
        }
        videos
    }

    #[test]
    fn packed_videos_unpack_as_they_were() {
        let dir = tempfile::tempdir().unwrap();
        let videos = videos(dir.path());
        let archive = dir.path().join("Space").join("Videos.zip");
        let folders = LocalProjectFolders::new();
        let before = folders.size(&videos).unwrap();
        assert_eq!(before, 3 * CHUNK as u64 + 5 + 10 + 15 + 12);
        let mut last = (0, 0);
        folders
            .pack(&videos, &archive, &mut |done, total| {
                last = (done, total);
                true
            })
            .unwrap();
        assert_eq!(last.0, last.1);
        assert!(archive.is_file());
        assert!(!videos.join("Apollo-01234567").exists());
        assert!(!videos.join("Mir-89abcdef").exists());
        folders.unpack(&archive, &videos, &mut |_, _| true).unwrap();
        assert!(!archive.exists());
        assert_eq!(folders.size(&videos).unwrap(), before);
        assert_eq!(
            fs::read(videos.join("Mir-89abcdef").join("script.json")).unwrap(),
            b"Mir-89abcdef"
        );
        assert_eq!(
            fs::read(videos.join("Apollo-01234567").join("clips").join("1.mp4"))
                .unwrap()
                .len(),
            3 * CHUNK + 5
        );
    }

    #[test]
    fn a_pack_stopped_or_failing_its_check_removes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let videos = videos(dir.path());
        let archive = dir.path().join("Space").join("Videos.zip");
        let folders = LocalProjectFolders::new();
        let stopped = folders.pack(&videos, &archive, &mut |done, total| done < total / 2);
        assert!(matches!(stopped, Err(FolderMoveError::Stopped)));
        assert!(!archive.exists());
        assert!(!with_suffix(&archive, ".partial").exists());
        assert!(videos.join("Apollo-01234567").join("script.json").exists());

        // An archive that does not hold a file whole fails the check.
        let partial = dir.path().join("check.zip");
        let mut files = Vec::new();
        for (relative, size) in files_under(&videos).unwrap() {
            let name = relative.to_string_lossy().replace('\\', "/");
            files.push((name, videos.join(&relative), size));
        }
        let mut done = 0;
        write_archive(&partial, &files, &mut done, u64::MAX, &mut |_, _| true).unwrap();
        files[0].2 += 1;
        let mut done = 0;
        let checked = check_archive(&partial, &files, &mut done, u64::MAX, &mut |_, _| true);
        assert!(matches!(checked, Err(FolderMoveError::Mismatch(_))));
        files.push(("missing/file".into(), videos.clone(), 1));
        files[0].2 -= 1;
        let mut done = 0;
        let checked = check_archive(&partial, &files, &mut done, u64::MAX, &mut |_, _| true);
        assert!(matches!(checked, Err(FolderMoveError::Mismatch(name)) if name == "missing/file"));
    }

    #[test]
    fn an_unpack_cut_short_keeps_the_archive_and_starts_over() {
        let dir = tempfile::tempdir().unwrap();
        let videos = videos(dir.path());
        let archive = dir.path().join("Space").join("Videos.zip");
        let folders = LocalProjectFolders::new();
        folders.pack(&videos, &archive, &mut |_, _| true).unwrap();
        let stopped = folders.unpack(&archive, &videos, &mut |done, _| done < CHUNK as u64);
        assert!(matches!(stopped, Err(FolderMoveError::Stopped)));
        assert!(archive.exists());
        folders.unpack(&archive, &videos, &mut |_, _| true).unwrap();
        assert!(!archive.exists());
        assert_eq!(
            fs::read(videos.join("Apollo-01234567").join("script.json")).unwrap(),
            b"Apollo-01234567"
        );
    }
}
