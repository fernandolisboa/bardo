//! SQLite persistence and migrations, and the secret store for provider
//! keys, network app credentials and OAuth tokens (Windows Credential
//! Manager).

mod channel;
mod connection;
mod cost;
mod cut_suggestion;
mod export;
mod export_files;
mod files;
mod job;
mod media;
mod migrations;
mod narration;
mod network_account;
mod persona;
mod profile;
mod publication;
mod render;
mod research;
mod scene;
mod script;
mod secrets;
mod template;
mod theme;
mod timeline;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use bardo_domain::RepositoryError;
use rusqlite::Connection;

pub use export_files::{LocalExportFiles, MemoryExportFiles, default_exports_dir};
pub use files::{LocalProjectFiles, MemoryProjectFiles, default_projects_dir};
pub use migrations::{BrokenReferences, MigrationError};
#[cfg(windows)]
pub use secrets::CredentialManager;
pub use secrets::{
    MemorySecretStore, app_credentials_target, credential_target, platform_connection_secrets,
    platform_secret_store, tokens_target,
};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Migration(#[from] migrations::MigrationError),
    #[error("could not create the data directory {path}: {source}")]
    DataDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("no per-user data directory is available on this system")]
    NoDataDir,
}

/// The app's database: one SQLite file per installation, migrated on open.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Opens (creating if needed) the database at `path` and migrates it.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| StorageError::DataDir {
                path: dir.to_owned(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, StorageError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrations::run(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock cannot leave SQLite half-written
        // (statements are atomic), so a poisoned lock is safe to reuse.
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Wraps an adapter error for the domain's repository ports.
pub(crate) fn boxed(error: impl std::error::Error + Send + Sync + 'static) -> RepositoryError {
    RepositoryError(Box::new(error))
}

/// Times are stored as Unix milliseconds; times before 1970 store as 0.
pub(crate) fn to_unix_millis(time: SystemTime) -> i64 {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

pub(crate) fn from_unix_millis(millis: i64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_millis(u64::try_from(millis).unwrap_or(0))
}

/// `%APPDATA%\Bardo\bardo.db` on Windows (the platform's per-user data
/// directory elsewhere).
pub fn default_database_path() -> Result<PathBuf, StorageError> {
    let dir = dirs::data_dir().ok_or(StorageError::NoDataDir)?;
    Ok(dir.join("Bardo").join("bardo.db"))
}
