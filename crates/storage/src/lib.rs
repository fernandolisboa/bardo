//! SQLite persistence and migrations. Secrets access (Windows Credential
//! Manager) lands with the provider keys slice.

mod channel;
mod migrations;
mod profile;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use bardo_domain::RepositoryError;
use rusqlite::Connection;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
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

/// `%APPDATA%\Bardo\bardo.db` on Windows (the platform's per-user data
/// directory elsewhere).
pub fn default_database_path() -> Result<PathBuf, StorageError> {
    let dir = dirs::data_dir().ok_or(StorageError::NoDataDir)?;
    Ok(dir.join("Bardo").join("bardo.db"))
}
