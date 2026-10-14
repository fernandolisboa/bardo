//! Backups of the profile's data (issue #143, ADR-0010): one `.zip` with
//! the database and, when the user asks, the project folders. Keys,
//! credentials and sign-ins never go in: they live in the system's secret
//! store, not in the database or the folders.
//!
//! A restore is unpacked beside the data first (**staged**) and swapped in
//! at the next start, before the database opens.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::{Network, Progress, Provider};

/// How often Bardo backs up the database by itself, when the profile has
/// automatic backups on.
pub const AUTOMATIC_BACKUP_EVERY: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How many automatic backups are kept; older ones are deleted.
pub const AUTOMATIC_BACKUPS_KEPT: usize = 4;

/// What a backup holds, as its manifest says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    /// When the database was copied.
    pub created_at: SystemTime,
    /// The Bardo that made it, e.g. `0.1.0`.
    pub bardo_version: String,
    /// Its database's schema version.
    pub schema_version: u32,
    /// Whether the project folders are in it.
    pub media: bool,
    /// Made by Bardo on its own, weekly.
    pub automatic: bool,
    /// The database's size, uncompressed.
    pub database_bytes: u64,
    /// The media's size and file count, uncompressed (zero without media).
    pub media_bytes: u64,
    pub media_files: u64,
    /// The providers whose keys were saved where it was made, and the
    /// networks whose app credentials were. Their names only: the secrets
    /// stay in that computer's secret store. After a restore, the ones
    /// missing here are listed to save again.
    pub saved_keys: Vec<Provider>,
    pub saved_credentials: Vec<Network>,
}

/// What a backup would carry of the project folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MediaSize {
    pub bytes: u64,
    pub files: u64,
}

/// A backup Bardo made by itself, in its automatic backups folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedBackup {
    pub path: PathBuf,
    pub created_at: SystemTime,
    /// The file's size on disk.
    pub bytes: u64,
}

/// How the restore applied at the last start went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    /// When the restored backup was made.
    pub backup_created_at: SystemTime,
    /// Whether the backup had media, which replaced the project folders.
    pub media: bool,
    /// When the files were swapped.
    pub restored_at: SystemTime,
    /// `None` when the restored data is in place; otherwise why not, for
    /// logs. The data is then as it was before, unless `kept_aside`.
    pub failure: Option<String>,
    /// When a failed swap could not put every file back: the folder that
    /// holds the previous data, which nothing deletes.
    pub kept_aside: Option<PathBuf>,
    /// As the backup's manifest names them (`BackupInfo`).
    pub saved_keys: Vec<Provider>,
    pub saved_credentials: Vec<Network>,
}

impl RestoreOutcome {
    pub fn succeeded(&self) -> bool {
        self.failure.is_none()
    }
}

/// Why a backup or restore did not happen.
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    /// Not a Bardo backup: not a zip, or no manifest.
    #[error("not a Bardo backup")]
    NotABackup,
    /// A Bardo backup that cannot be used: a part is missing or broken.
    #[error("the backup is damaged: {0}")]
    Damaged(String),
    /// Made by a newer Bardo, whose database this one cannot open.
    #[error("the backup was made by a newer Bardo ({bardo_version})")]
    Newer { bardo_version: String },
    /// The user cancelled, or Bardo is closing.
    #[error("stopped")]
    Stopped,
    /// Reading or writing a file failed (a full disk, a missing folder).
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The database copy failed.
    #[error("the database: {0}")]
    Database(String),
}

/// What a backup is asked to carry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BackupOptions {
    pub media: bool,
    pub automatic: bool,
    /// Written to the manifest as they are (`BackupInfo::saved_keys`).
    pub saved_keys: Vec<Provider>,
    pub saved_credentials: Vec<Network>,
}

/// Called with how far a backup or restore got; returns false to stop it.
pub type ProgressReport<'a> = dyn FnMut(Progress) -> bool + 'a;

/// Backs up the data, and unpacks a backup to replace it at the next
/// start. Blocks on the disk: run off the UI thread (in a job). Shared
/// with the job queue's worker threads.
pub trait BackupStore: Send + Sync {
    /// What a backup with media would carry now.
    fn media_size(&self) -> Result<MediaSize, BackupError>;

    /// Writes a backup to `to`, replacing it. A failed or stopped backup
    /// leaves no file.
    fn write(
        &self,
        to: &Path,
        options: BackupOptions,
        progress: &mut ProgressReport<'_>,
    ) -> Result<BackupInfo, BackupError>;

    /// What the backup at `from` holds, refusing one this Bardo cannot
    /// restore.
    fn inspect(&self, from: &Path) -> Result<BackupInfo, BackupError>;

    /// Unpacks and checks the backup at `from` beside the data, to replace
    /// it at the next start. A failed or stopped one leaves nothing staged.
    fn stage(
        &self,
        from: &Path,
        progress: &mut ProgressReport<'_>,
    ) -> Result<BackupInfo, BackupError>;

    /// The backup staged to replace the data at the next start.
    fn staged(&self) -> Option<BackupInfo>;

    /// Removes a staged backup, so the next start keeps the data.
    fn discard_staged(&self) -> Result<(), BackupError>;

    /// Where the next automatic backup made at `at` goes.
    fn automatic_path(&self, at: SystemTime) -> PathBuf;

    /// The automatic backups kept, newest first.
    fn automatic(&self) -> Result<Vec<SavedBackup>, BackupError>;

    /// Deletes automatic backups past the newest `keep`.
    fn prune_automatic(&self, keep: usize) -> Result<(), BackupError>;

    /// How the restore applied at the last start went, until forgotten.
    fn last_restore(&self) -> Option<RestoreOutcome>;

    fn forget_last_restore(&self) -> Result<(), BackupError>;
}
