//! Which folders this copy of Bardo keeps its data in (issue #162,
//! ADR-0014). A copy built from source keeps its own, `Bardo Dev`, so
//! trying a newer build never opens, migrates or changes the data of the
//! Bardo the installer put in place, and an older installed Bardo never
//! opens a database a newer build migrated.
//!
//! The choice is made once, as the process starts (`use_data_home`),
//! before anything opens the database; every path below follows it.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::StorageError;

/// Whose folders this copy uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataHome {
    /// Installed by Bardo's installer (or the Store): `Bardo`.
    #[default]
    Installed,
    /// Built from source: `Bardo Dev`.
    Development,
}

impl DataHome {
    /// The folder's name under the user's data folders.
    pub fn folder(self) -> &'static str {
        match self {
            DataHome::Installed => "Bardo",
            DataHome::Development => "Bardo Dev",
        }
    }

    /// `%APPDATA%\<folder>` on Windows (the platform's per-user data
    /// directory elsewhere): the database and the installer's choices.
    pub fn roaming(self) -> Result<PathBuf, StorageError> {
        let dir = dirs::data_dir().ok_or(StorageError::NoDataDir)?;
        Ok(dir.join(self.folder()))
    }

    /// `%LOCALAPPDATA%\<folder>` on Windows (the platform's local data
    /// directory elsewhere): media, logs, backups and updates, which belong
    /// to this computer.
    pub fn local(self) -> Result<PathBuf, StorageError> {
        let dir = dirs::data_local_dir().ok_or(StorageError::NoDataDir)?;
        Ok(dir.join(self.folder()))
    }
}

static DATA_HOME: OnceLock<DataHome> = OnceLock::new();

/// Sets whose folders this process uses. Called once, first thing at
/// start; a later call keeps the first choice and says so.
pub fn use_data_home(home: DataHome) -> bool {
    DATA_HOME.set(home).is_ok() || data_home() == home
}

/// Whose folders this process uses: the installed copy's unless
/// `use_data_home` said otherwise.
pub fn data_home() -> DataHome {
    DATA_HOME.get().copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_copy_has_its_own_folders() {
        assert_eq!(DataHome::Installed.folder(), "Bardo");
        assert_eq!(DataHome::Development.folder(), "Bardo Dev");
        let installed = DataHome::Installed.roaming().unwrap();
        let development = DataHome::Development.roaming().unwrap();
        assert_eq!(installed.parent(), development.parent());
        assert_ne!(installed, development);
        assert_ne!(
            DataHome::Installed.local().unwrap(),
            DataHome::Development.local().unwrap()
        );
    }
}
