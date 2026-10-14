//! Where an update's installer waits to run (issue #144, ADR-0011):
//! `%LOCALAPPDATA%\Bardo\updates`, and how it is started.
//!
//! A download writes `<name>.<attempt>.part` and renames it once it is
//! whole and checked, so a file with an installer's name is never
//! half-written, and a download cut short that ends late never touches
//! the file a newer one writes.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bardo_domain::{InstallerRunner, InstallerStore};

/// What an unfinished download's name ends with.
pub const PART_SUFFIX: &str = ".part";

/// The updates folder for `local` (`%LOCALAPPDATA%\Bardo`).
pub fn updates_dir(local: &Path) -> PathBuf {
    local.join("updates")
}

/// Installers in a folder on disk.
pub struct LocalInstallers {
    dir: PathBuf,
}

impl LocalInstallers {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn part(&self, name: &str, attempt: &str) -> std::io::Result<PathBuf> {
        Ok(self.dir.join(part_name(checked(name)?, checked(attempt)?)))
    }
}

/// What an attempt at `name` writes until it is whole.
fn part_name(name: &str, attempt: &str) -> String {
    format!("{name}.{attempt}{PART_SUFFIX}")
}

/// Only a plain file name: nothing reaches outside the folder.
fn checked(name: &str) -> std::io::Result<&str> {
    let plain = !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name != "."
        && name != ".."
        && Path::new(name).file_name().is_some_and(|file| file == name);
    if plain {
        Ok(name)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("not a file name: {name}"),
        ))
    }
}

fn removed(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

impl InstallerStore for LocalInstallers {
    fn begin(&self, name: &str, attempt: &str) -> std::io::Result<Box<dyn Write + Send>> {
        let part = self.part(name, attempt)?;
        fs::create_dir_all(&self.dir)?;
        let file = fs::File::create(part)?;
        Ok(Box::new(std::io::BufWriter::with_capacity(
            256 * 1024,
            file,
        )))
    }

    fn finish(&self, name: &str, attempt: &str) -> std::io::Result<()> {
        let part = self.part(name, attempt)?;
        let to = self.dir.join(checked(name)?);
        // Windows does not rename over a file.
        removed(fs::remove_file(&to))?;
        fs::rename(part, to)
    }

    fn abandon(&self, name: &str, attempt: &str) -> std::io::Result<()> {
        removed(fs::remove_file(self.part(name, attempt)?))
    }

    fn discard(&self, name: &str) -> std::io::Result<()> {
        removed(fs::remove_file(self.dir.join(checked(name)?)))
    }

    fn open(&self, name: &str) -> std::io::Result<Option<Box<dyn Read + Send>>> {
        let name = checked(name)?;
        match fs::File::open(self.dir.join(name)) {
            Ok(file) => Ok(Some(Box::new(std::io::BufReader::new(file)))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn exists(&self, name: &str) -> bool {
        checked(name).is_ok_and(|name| self.dir.join(name).is_file())
    }

    fn prune(&self, keep: &dyn Fn(&str) -> bool, keep_unfinished: bool) -> std::io::Result<()> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let mut failed = None;
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let kept = match name.strip_suffix(PART_SUFFIX) {
                Some(_) => keep_unfinished,
                None => keep(&name),
            };
            // One file Windows still holds (the installer that just ran)
            // does not keep the others: it goes another time.
            if !kept && let Err(error) = removed(fs::remove_file(entry.path())) {
                failed.get_or_insert(error);
            }
        }
        failed.map_or(Ok(()), Err)
    }
}

/// Installers in memory, for tests.
#[derive(Default, Clone)]
pub struct MemoryInstallers {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemoryInstallers {
    /// The names held, finished and unfinished (`.part`), sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.lock().keys().cloned().collect();
        names.sort();
        names
    }

    /// Puts a finished file in place, or changes one.
    pub fn put(&self, name: &str, bytes: &[u8]) {
        self.lock().insert(name.to_owned(), bytes.to_vec());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>> {
        self.files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Writes into the memory store as it goes.
struct MemoryWriter {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    name: String,
}

impl Write for MemoryWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(self.name.clone())
            .or_default()
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl InstallerStore for MemoryInstallers {
    fn begin(&self, name: &str, attempt: &str) -> std::io::Result<Box<dyn Write + Send>> {
        let part = part_name(checked(name)?, checked(attempt)?);
        self.lock().insert(part.clone(), Vec::new());
        Ok(Box::new(MemoryWriter {
            files: Arc::clone(&self.files),
            name: part,
        }))
    }

    fn finish(&self, name: &str, attempt: &str) -> std::io::Result<()> {
        let mut files = self.lock();
        let bytes = files
            .remove(&part_name(name, attempt))
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        files.insert(name.to_owned(), bytes);
        Ok(())
    }

    fn abandon(&self, name: &str, attempt: &str) -> std::io::Result<()> {
        self.lock().remove(&part_name(name, attempt));
        Ok(())
    }

    fn discard(&self, name: &str) -> std::io::Result<()> {
        self.lock().remove(name);
        Ok(())
    }

    fn open(&self, name: &str) -> std::io::Result<Option<Box<dyn Read + Send>>> {
        Ok(self
            .lock()
            .get(name)
            .map(|bytes| Box::new(std::io::Cursor::new(bytes.clone())) as Box<dyn Read + Send>))
    }

    fn path(&self, name: &str) -> PathBuf {
        PathBuf::from("memory").join(name)
    }

    fn exists(&self, name: &str) -> bool {
        self.lock().contains_key(name)
    }

    fn prune(&self, keep: &dyn Fn(&str) -> bool, keep_unfinished: bool) -> std::io::Result<()> {
        self.lock()
            .retain(|name, _| match name.strip_suffix(PART_SUFFIX) {
                Some(_) => keep_unfinished,
                None => keep(name),
            });
        Ok(())
    }
}

/// Starts the installer as its own process: it goes on after Bardo
/// quits, which it must for the installer to replace Bardo's files.
pub struct ProcessInstallerRunner;

impl InstallerRunner for ProcessInstallerRunner {
    fn run(&self, installer: &Path, arguments: &[&str]) -> std::io::Result<()> {
        std::process::Command::new(installer)
            .args(arguments)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(drop)
    }
}

/// An installer run: the file and its arguments.
pub type InstallerRun = (PathBuf, Vec<String>);

/// Records what it was asked to run, for tests.
#[derive(Default, Clone)]
pub struct MemoryInstallerRunner {
    runs: Arc<Mutex<Vec<InstallerRun>>>,
}

impl MemoryInstallerRunner {
    /// Each run's installer and arguments, oldest first.
    pub fn runs(&self) -> Vec<InstallerRun> {
        self.runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl InstallerRunner for MemoryInstallerRunner {
    fn run(&self, installer: &Path, arguments: &[&str]) -> std::io::Result<()> {
        self.runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((
                installer.to_owned(),
                arguments.iter().map(|a| (*a).to_owned()).collect(),
            ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(store: &dyn InstallerStore, name: &str) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        store.open(name).unwrap()?.read_to_end(&mut bytes).unwrap();
        Some(bytes)
    }

    const ATTEMPT: &str = "job-1";

    fn write(store: &dyn InstallerStore, name: &str, bytes: &[u8]) {
        let mut file = store.begin(name, ATTEMPT).unwrap();
        file.write_all(bytes).unwrap();
        file.flush().unwrap();
    }

    fn stores(dir: &Path) -> [Box<dyn InstallerStore>; 2] {
        [
            Box::new(LocalInstallers::new(dir.join("updates"))),
            Box::new(MemoryInstallers::default()),
        ]
    }

    #[test]
    fn a_file_has_its_name_only_once_finished() {
        let dir = tempfile::tempdir().unwrap();
        for store in stores(dir.path()) {
            let name = "Bardo-Setup-0.2.0-x64.exe";
            write(&*store, name, b"MZ installer");
            assert_eq!(read(&*store, name), None, "not finished yet");
            assert!(!store.exists(name));
            store.finish(name, ATTEMPT).unwrap();
            assert!(store.exists(name));
            assert_eq!(read(&*store, name).as_deref(), Some(&b"MZ installer"[..]));

            // Downloading it again replaces it only once finished.
            write(&*store, name, b"MZ newer");
            assert_eq!(read(&*store, name).as_deref(), Some(&b"MZ installer"[..]));
            store.finish(name, ATTEMPT).unwrap();
            assert_eq!(read(&*store, name).as_deref(), Some(&b"MZ newer"[..]));

            store.discard(name).unwrap();
            assert_eq!(read(&*store, name), None);
            store.discard(name).unwrap();
        }
    }

    #[test]
    fn an_attempt_abandons_only_what_it_wrote() {
        let dir = tempfile::tempdir().unwrap();
        for store in stores(dir.path()) {
            write(&*store, "a.exe", b"cut");
            let mut newer = store.begin("a.exe", "job-2").unwrap();
            newer.write_all(b"whole").unwrap();
            newer.flush().unwrap();
            drop(newer);
            // The first attempt ends late: the newer one's file stays.
            store.abandon("a.exe", ATTEMPT).unwrap();
            store.finish("a.exe", "job-2").unwrap();
            assert_eq!(read(&*store, "a.exe").as_deref(), Some(&b"whole"[..]));
            store.abandon("a.exe", ATTEMPT).unwrap();
            assert!(read(&*store, "a.exe").is_some(), "never the finished file");
        }
        let store = LocalInstallers::new(dir.path().join("only"));
        write(&store, "b.exe", b"half");
        store.abandon("b.exe", ATTEMPT).unwrap();
        assert_eq!(fs::read_dir(dir.path().join("only")).unwrap().count(), 0);
    }

    #[test]
    fn prune_keeps_what_it_is_told_to() {
        let dir = tempfile::tempdir().unwrap();
        for store in stores(dir.path()) {
            for name in ["old.exe", "new.exe", "cut.exe"] {
                write(&*store, name, b"x");
            }
            store.finish("old.exe", ATTEMPT).unwrap();
            store.finish("new.exe", ATTEMPT).unwrap();
            store.prune(&|name| name == "new.exe", true).unwrap();
            assert_eq!(read(&*store, "old.exe"), None);
            assert!(read(&*store, "new.exe").is_some());
            store
                .finish("cut.exe", ATTEMPT)
                .unwrap_or_else(|_| panic!("kept unfinished"));

            write(&*store, "cut2.exe", b"x");
            store.prune(&|_| true, false).unwrap();
            assert!(
                store.finish("cut2.exe", ATTEMPT).is_err(),
                "unfinished removed"
            );
        }
        // A folder that was never made has nothing to prune.
        LocalInstallers::new(dir.path().join("none"))
            .prune(&|_| false, false)
            .unwrap();
    }

    #[test]
    fn names_never_leave_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalInstallers::new(dir.path().join("updates"));
        for name in ["../evil.exe", "..", "a/b.exe", "a\\b.exe", "C:x.exe", ""] {
            assert!(store.begin(name, ATTEMPT).is_err(), "{name}");
            assert!(store.begin("a.exe", name).is_err(), "{name}");
            assert!(store.discard(name).is_err(), "{name}");
            assert!(store.open(name).is_err(), "{name}");
        }
    }

    #[test]
    fn the_memory_runner_records_runs() {
        let runner = MemoryInstallerRunner::default();
        runner.run(Path::new("setup.exe"), &["/SILENT"]).unwrap();
        assert_eq!(
            runner.runs(),
            vec![(PathBuf::from("setup.exe"), vec!["/SILENT".to_owned()])]
        );
    }
}
