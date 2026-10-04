//! Voice samples on disk: `<root>/<key>.mp3`. The root is
//! `%LOCALAPPDATA%\Bardo\voice-samples` on Windows: samples are a cache of
//! this machine, so they stay out of the roaming profile. Only the newest
//! samples are kept; an older one is made again when asked for.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use bardo_domain::{VoiceSampleStore, VoiceSampleStoreError};

use crate::StorageError;

/// `%LOCALAPPDATA%\Bardo\voice-samples` on Windows (the platform's local
/// data directory elsewhere).
pub fn default_voice_samples_dir() -> Result<PathBuf, StorageError> {
    let dir = dirs::data_local_dir().ok_or(StorageError::NoDataDir)?;
    Ok(dir.join("Bardo").join("voice-samples"))
}

fn error(key: &str, source: std::io::Error) -> VoiceSampleStoreError {
    VoiceSampleStoreError {
        key: key.to_owned(),
        source,
    }
}

/// Keys are short hex strings; anything else could name a file elsewhere.
fn checked(key: &str) -> Result<&str, VoiceSampleStoreError> {
    if !key.is_empty() && key.len() <= 64 && key.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(key)
    } else {
        Err(error(
            key,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a sample key"),
        ))
    }
}

/// A partial file older than this belongs to a write that never ended.
const STALE_PARTIAL: Duration = Duration::from_secs(60 * 60);

/// Samples in one folder, at most `limit` of them; the least recently
/// played go first.
#[derive(Debug, Clone)]
pub struct LocalVoiceSamples {
    root: PathBuf,
    limit: usize,
}

impl LocalVoiceSamples {
    /// About 40 KB a sample: a few megabytes at most.
    pub const DEFAULT_LIMIT: usize = 100;

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            limit: Self::DEFAULT_LIMIT,
        }
    }

    /// Keeps at most `limit` samples (at least one).
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn file(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.mp3"))
    }

    /// A file of its own for each write, so two writes of one key never
    /// mix their bytes.
    fn partial(&self, key: &str) -> PathBuf {
        static WRITES: AtomicU64 = AtomicU64::new(0);
        let n = WRITES.fetch_add(1, Ordering::Relaxed);
        self.root
            .join(format!("{key}-{}-{n}.partial", std::process::id()))
    }

    /// Removes the oldest samples past the limit, sparing `kept`, and
    /// partial files a crash left behind. A file that cannot be removed is
    /// left for the next time.
    fn prune(&self, kept: &Path) {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        let now = SystemTime::now();
        let mut samples: Vec<(SystemTime, PathBuf)> = Vec::new();
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            let modified = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            match path.extension().and_then(|ext| ext.to_str()) {
                Some("mp3") => samples.push((modified, path)),
                // Another write may still be filling a recent one.
                Some("partial")
                    if now.duration_since(modified).unwrap_or_default() > STALE_PARTIAL =>
                {
                    let _ = std::fs::remove_file(&path);
                }
                _ => {}
            }
        }
        if samples.len() <= self.limit {
            return;
        }
        // Newest first; the one just kept stays whatever its clock says.
        samples.sort_by_key(|sample| std::cmp::Reverse(sample.0));
        let mut room = self.limit.saturating_sub(1);
        for (_, path) in samples {
            if path == kept {
                continue;
            }
            if room > 0 {
                room -= 1;
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

impl VoiceSampleStore for LocalVoiceSamples {
    fn find(&self, key: &str) -> Option<PathBuf> {
        let key = checked(key).ok()?;
        let file = self.file(key);
        if !file.is_file() {
            return None;
        }
        // Played again: it counts as new, so it outlives unplayed ones.
        if let Ok(handle) = std::fs::File::options().write(true).open(&file) {
            let _ = handle.set_modified(SystemTime::now());
        }
        Some(file)
    }

    fn keep(&self, key: &str, audio: &[u8]) -> Result<PathBuf, VoiceSampleStoreError> {
        let key = checked(key)?;
        std::fs::create_dir_all(&self.root).map_err(|e| error(key, e))?;
        // Written beside it, then renamed over it: never half a file.
        let partial = self.partial(key);
        let mut file = std::fs::File::create(&partial).map_err(|e| error(key, e))?;
        file.write_all(audio)
            .and_then(|()| file.sync_all())
            .map_err(|e| error(key, e))?;
        drop(file);
        let path = self.file(key);
        if let Err(e) = std::fs::rename(&partial, &path) {
            let _ = std::fs::remove_file(&partial);
            return Err(error(key, e));
        }
        self.prune(&path);
        Ok(path)
    }
}

/// Samples in memory, for tests. Paths name no real file.
#[derive(Debug, Default)]
pub struct MemoryVoiceSamples {
    samples: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryVoiceSamples {
    /// The audio kept under `key`.
    pub fn audio(&self, key: &str) -> Option<Vec<u8>> {
        self.samples.lock().expect("samples lock").get(key).cloned()
    }

    pub fn len(&self) -> usize {
        self.samples.lock().expect("samples lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn path(key: &str) -> PathBuf {
        PathBuf::from("memory")
            .join("voice-samples")
            .join(format!("{key}.mp3"))
    }
}

impl VoiceSampleStore for MemoryVoiceSamples {
    fn find(&self, key: &str) -> Option<PathBuf> {
        let key = checked(key).ok()?;
        self.samples
            .lock()
            .expect("samples lock")
            .contains_key(key)
            .then(|| Self::path(key))
    }

    fn keep(&self, key: &str, audio: &[u8]) -> Result<PathBuf, VoiceSampleStoreError> {
        let key = checked(key)?;
        self.samples
            .lock()
            .expect("samples lock")
            .insert(key.to_owned(), audio.to_vec());
        Ok(Self::path(key))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_kept_sample_is_found_by_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let samples = LocalVoiceSamples::new(dir.path().join("voice-samples"));
        assert_eq!(samples.find("00ff"), None);

        let path = samples.keep("00ff", b"ID3 audio").unwrap();
        assert_eq!(path, dir.path().join("voice-samples").join("00ff.mp3"));
        assert_eq!(std::fs::read(&path).unwrap(), b"ID3 audio");
        assert_eq!(samples.find("00ff"), Some(path.clone()));

        // Kept again: replaced, with no partial file left behind.
        samples.keep("00ff", b"ID3 newer").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"ID3 newer");
        let names: Vec<_> = std::fs::read_dir(samples.root())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["00ff.mp3"]);
    }

    #[test]
    fn keys_that_are_not_hex_never_reach_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let samples = LocalVoiceSamples::new(dir.path());
        for bad in ["", "../escape", "a/b", "not-hex", "C:x", &"a".repeat(65)] {
            assert!(samples.keep(bad, b"x").is_err(), "{bad}");
            assert_eq!(samples.find(bad), None, "{bad}");
        }
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn only_the_newest_samples_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let samples = LocalVoiceSamples::new(dir.path()).with_limit(2);
        let keep = |key: &str, age_secs: u64| {
            let path = samples.keep(key, b"ID3").unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(age_secs))
                .unwrap();
        };
        keep("aa", 300);
        keep("bb", 200);
        let kept = |key: &str| dir.path().join(format!("{key}.mp3")).exists();
        assert!(kept("aa") && kept("bb"));

        // The newest stays even if its clock is behind; the oldest goes.
        samples.keep("cc", b"ID3").unwrap();
        assert!(!kept("aa"));
        assert!(kept("bb") && kept("cc"));
    }

    #[test]
    fn a_played_sample_outlives_newer_unplayed_ones() {
        let dir = tempfile::tempdir().unwrap();
        let samples = LocalVoiceSamples::new(dir.path()).with_limit(2);
        let age = |key: &str, secs: u64| {
            let file = std::fs::File::options()
                .write(true)
                .open(dir.path().join(format!("{key}.mp3")))
                .unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(secs))
                .unwrap();
        };
        samples.keep("aa", b"ID3").unwrap();
        age("aa", 300);
        samples.keep("bb", b"ID3").unwrap();
        age("bb", 200);

        assert!(samples.find("aa").is_some(), "played again");
        samples.keep("cc", b"ID3").unwrap();
        assert!(samples.find("aa").is_some());
        assert_eq!(samples.find("bb"), None);
    }

    #[test]
    fn partial_files_left_by_a_crash_are_removed_later() {
        let dir = tempfile::tempdir().unwrap();
        let samples = LocalVoiceSamples::new(dir.path());
        let stale = dir.path().join("aa-1-0.partial");
        let fresh = dir.path().join("bb-1-1.partial");
        std::fs::write(&stale, b"ID").unwrap();
        std::fs::write(&fresh, b"ID").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(SystemTime::now() - STALE_PARTIAL - Duration::from_secs(1))
            .unwrap();

        samples.keep("cc", b"ID3").unwrap();
        assert!(!stale.exists());
        assert!(fresh.exists(), "may still be written");
    }

    #[test]
    fn writes_of_one_key_from_many_threads_leave_one_whole_sample() {
        let dir = tempfile::tempdir().unwrap();
        let samples = LocalVoiceSamples::new(dir.path());
        let audio: Vec<Vec<u8>> = (0..8u8).map(|n| vec![n; 64 * 1024]).collect();
        std::thread::scope(|scope| {
            for bytes in &audio {
                let samples = &samples;
                // Windows may refuse a rename racing another; one wins.
                scope.spawn(move || samples.keep("ab", bytes).ok());
            }
        });
        let kept = std::fs::read(samples.find("ab").unwrap()).unwrap();
        assert!(audio.contains(&kept), "one write, whole");
    }

    #[test]
    fn memory_samples_behave_alike() {
        let samples = MemoryVoiceSamples::default();
        assert!(samples.is_empty());
        assert_eq!(samples.find("ab"), None);
        let path = samples.keep("ab", b"ID3").unwrap();
        assert_eq!(samples.find("ab"), Some(path));
        assert_eq!(samples.audio("ab").unwrap(), b"ID3");
        assert!(samples.keep("../x", b"x").is_err());
    }
}
