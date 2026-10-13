//! What the installer asked the user and leaves for Bardo (issue #135): a
//! small TOML file next to the database, which Bardo reads once at its next
//! start and deletes.
//!
//! ```toml
//! background_agent = true
//! ```
//!
//! The installer writes it only on a first install. A file Bardo cannot
//! read is still deleted, so a bad one never comes back at every start.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::StorageError;

/// The answers the installer collected. A missing key means the installer
/// did not ask.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct SetupChoices {
    /// "Publish even when Bardo is closed?" (ADR-0006).
    pub background_agent: Option<bool>,
}

/// `%APPDATA%\Bardo\setup-choices.toml` on Windows, next to the database
/// (the platform's per-user data directory elsewhere).
pub fn default_setup_choices_path() -> Result<PathBuf, StorageError> {
    let dir = dirs::data_dir().ok_or(StorageError::NoDataDir)?;
    Ok(dir.join("Bardo").join(SETUP_CHOICES_FILE))
}

/// The file name the installer writes.
pub const SETUP_CHOICES_FILE: &str = "setup-choices.toml";

/// Why the choices could not be read. The file is gone either way.
#[derive(Debug, thiserror::Error)]
pub enum SetupChoicesError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not valid: {source}")]
    Invalid {
        path: PathBuf,
        source: toml::de::Error,
    },
}

/// Reads the installer's choices at `path` and deletes the file: `None`
/// when there is none.
pub fn take_setup_choices(path: &Path) -> Result<Option<SetupChoices>, SetupChoicesError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            let _ = std::fs::remove_file(path);
            return Err(SetupChoicesError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    let _ = std::fs::remove_file(path);
    // Inno Setup may write a byte order mark.
    let text = text.trim_start_matches('\u{feff}');
    toml::from_str(text)
        .map(Some)
        .map_err(|source| SetupChoicesError::Invalid {
            path: path.to_owned(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_file_means_no_choices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETUP_CHOICES_FILE);
        assert_eq!(take_setup_choices(&path).unwrap(), None);
    }

    #[test]
    fn the_choices_are_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETUP_CHOICES_FILE);
        std::fs::write(&path, "background_agent = true\r\n").unwrap();
        assert_eq!(
            take_setup_choices(&path).unwrap(),
            Some(SetupChoices {
                background_agent: Some(true)
            })
        );
        assert!(!path.exists(), "deleted after reading");
        assert_eq!(take_setup_choices(&path).unwrap(), None);
    }

    #[test]
    fn a_byte_order_mark_and_unknown_keys_are_fine() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETUP_CHOICES_FILE);
        std::fs::write(
            &path,
            "\u{feff}background_agent = false\nlanguage = \"pt-BR\"\n",
        )
        .unwrap();
        assert_eq!(
            take_setup_choices(&path).unwrap(),
            Some(SetupChoices {
                background_agent: Some(false)
            })
        );
    }

    #[test]
    fn an_empty_file_asks_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETUP_CHOICES_FILE);
        std::fs::write(&path, "").unwrap();
        assert_eq!(
            take_setup_choices(&path).unwrap(),
            Some(SetupChoices::default())
        );
    }

    #[test]
    fn a_bad_file_is_reported_and_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETUP_CHOICES_FILE);
        std::fs::write(&path, "background_agent = maybe").unwrap();
        assert!(matches!(
            take_setup_choices(&path),
            Err(SetupChoicesError::Invalid { .. })
        ));
        assert!(!path.exists());
    }
}
