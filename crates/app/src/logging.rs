//! The app's log file. Every line passes through the `Redactor` on its way
//! to disk, so a known key never reaches the file, whoever logged it
//! (PRD story 2).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bardo_domain::Redactor;
use tracing::Subscriber;
use tracing_subscriber::fmt::MakeWriter;

/// A log file bigger than this is moved to `bardo.log.1` (replacing the
/// previous one) when the app starts, so logs stay bounded.
pub const ROTATE_AT_BYTES: u64 = 5 * 1024 * 1024;

/// `%LOCALAPPDATA%\Bardo\logs\bardo.log` on Windows (the platform's local
/// data directory elsewhere). Local, not roaming: logs belong to this PC.
pub fn default_log_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("Bardo").join("logs").join("bardo.log"))
}

/// Logs INFO and above to `path` for the rest of the process.
pub fn init(path: &Path, redactor: Redactor) -> io::Result<()> {
    let subscriber = file_subscriber(path, redactor)?;
    tracing::subscriber::set_global_default(subscriber).map_err(io::Error::other)
}

/// The subscriber `init` installs, for scoped use in tests.
pub fn file_subscriber(
    path: &Path,
    redactor: Redactor,
) -> io::Result<impl Subscriber + Send + Sync + 'static> {
    let writer = RedactingFile::open(path, redactor)?;
    Ok(tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish())
}

/// Opens the log for appending, rotating it first when too big.
fn open_log(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > ROTATE_AT_BYTES) {
        let mut previous = path.as_os_str().to_owned();
        previous.push(".1");
        std::fs::rename(path, previous)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Hands each log event a buffer that is redacted and written in one piece
/// when the event is done, so a key can never be split across writes.
#[derive(Clone)]
struct RedactingFile {
    file: Arc<Mutex<File>>,
    redactor: Redactor,
}

impl RedactingFile {
    fn open(path: &Path, redactor: Redactor) -> io::Result<Self> {
        Ok(Self {
            file: Arc::new(Mutex::new(open_log(path)?)),
            redactor,
        })
    }
}

impl<'a> MakeWriter<'a> for RedactingFile {
    type Writer = RedactedEvent;

    fn make_writer(&'a self) -> Self::Writer {
        RedactedEvent {
            buffer: Vec::new(),
            target: self.clone(),
        }
    }
}

struct RedactedEvent {
    buffer: Vec<u8>,
    target: RedactingFile,
}

impl Write for RedactedEvent {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for RedactedEvent {
    fn drop(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&self.buffer);
        let redacted = self.target.redactor.redact(&text);
        let mut file = self
            .target
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Logging must never take the app down; a lost line is acceptable.
        let _ = file.write_all(redacted.as_bytes());
        let _ = file.flush();
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{ApiKey, Provider};

    use super::*;

    const SECRET: &str = "sk-ant-log-secret-0001";

    fn log_with(path: &Path, redactor: Redactor, body: impl FnOnce()) {
        let subscriber = file_subscriber(path, redactor).unwrap();
        tracing::subscriber::with_default(subscriber, body);
    }

    #[test]
    fn known_keys_never_reach_the_file_however_they_are_logged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("bardo.log");
        let redactor = Redactor::new();
        redactor.add(&ApiKey::parse(Provider::Claude, SECRET).unwrap());

        log_with(&path, redactor, || {
            tracing::info!("message with {SECRET}");
            tracing::warn!(header = SECRET, "as a field");
            tracing::error!(error = %format!("wrapped: {SECRET}!"), "as an error");
        });

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(!written.contains(SECRET), "{written}");
        assert_eq!(written.matches(Redactor::MASK).count(), 3, "{written}");
        assert!(written.contains("as an error"), "{written}");
    }

    #[test]
    fn a_key_saved_after_logging_started_is_masked_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.log");
        let redactor = Redactor::new();

        log_with(&path, redactor.clone(), || {
            redactor.add(&ApiKey::parse(Provider::Claude, SECRET).unwrap());
            tracing::info!("later: {SECRET}");
        });

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(!written.contains(SECRET), "{written}");
    }

    #[test]
    fn a_big_log_is_rotated_on_open_and_a_small_one_appended() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.log");
        std::fs::write(&path, "small\n").unwrap();
        log_with(&path, Redactor::new(), || tracing::info!("appended"));
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("small\n") && written.contains("appended"));

        let big = vec![b'x'; ROTATE_AT_BYTES as usize + 1];
        std::fs::write(&path, &big).unwrap();
        log_with(&path, Redactor::new(), || tracing::info!("fresh"));
        assert!(std::fs::read_to_string(&path).unwrap().contains("fresh"));
        assert_eq!(
            std::fs::metadata(dir.path().join("bardo.log.1"))
                .unwrap()
                .len(),
            big.len() as u64
        );
    }
}
