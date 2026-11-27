//! The Recycle Bin (issue #336): a deleted project's or video's folder goes
//! there unless Windows would delete it at once (a removable or network
//! drive, a bin turned off, or a folder larger than the drive's bin), which
//! Bardo says before the user confirms.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bardo_domain::{Discard, Disposal, PermanentReason, Trash};

/// This computer's Recycle Bin.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTrash;

impl Trash for SystemTrash {
    fn disposal(&self, path: &Path, size: u64) -> Disposal {
        platform::disposal(path, size)
    }

    fn discard(&self, path: &Path, how: Discard) -> io::Result<()> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        match how {
            Discard::Recycle => trash::delete(path).map_err(io::Error::other),
            Discard::Permanent if meta.is_dir() => fs::remove_dir_all(path),
            Discard::Permanent => fs::remove_file(path),
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::path::{Component, Path, Prefix};

    use bardo_domain::{Disposal, PermanentReason};

    /// The share of a drive Windows gives its bin when the user never set
    /// one: about 5%.
    const DEFAULT_BIN_SHARE: u64 = 20;

    pub(super) fn disposal(path: &Path, size: u64) -> Disposal {
        let root = match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    format!("{}:\\", char::from(letter).to_ascii_uppercase())
                }
                // `\\server\share`: a network folder.
                Prefix::UNC(..) | Prefix::VerbatimUNC(..) => {
                    return Disposal::Permanent(PermanentReason::NetworkDrive);
                }
                _ => return Disposal::Recycle,
            },
            _ => return Disposal::Recycle,
        };
        // Only fixed and removable drives are listed: a mapped network drive
        // is not.
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let Some(disk) = disks.list().iter().find(|disk| {
            disk.mount_point()
                .to_string_lossy()
                .eq_ignore_ascii_case(&root)
        }) else {
            return Disposal::Permanent(PermanentReason::NetworkDrive);
        };
        if disk.is_removable() {
            return Disposal::Permanent(PermanentReason::RemovableDrive);
        }
        let settings = volume_id(&root).and_then(|id| bin_settings(&id));
        if settings.is_some_and(|settings| settings.off) {
            return Disposal::Permanent(PermanentReason::BinOff);
        }
        let room = settings
            .and_then(|settings| settings.max_megabytes)
            .map(|megabytes| u64::from(megabytes) * 1024 * 1024)
            .unwrap_or(disk.total_space() / DEFAULT_BIN_SHARE);
        if size > room {
            Disposal::Permanent(PermanentReason::LargerThanBin)
        } else {
            Disposal::Recycle
        }
    }

    /// The `{guid}` of the volume mounted at `root` (`C:\`), as `mountvol`
    /// lists it.
    fn volume_id(root: &str) -> Option<String> {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = std::process::Command::new("mountvol")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        volume_of(&String::from_utf8_lossy(&output.stdout), root)
    }

    /// In `mountvol`'s listing, the volume `root` is a mount point of.
    pub(super) fn volume_of(listing: &str, root: &str) -> Option<String> {
        let mut volume: Option<&str> = None;
        for line in listing.lines().map(str::trim) {
            if line.starts_with(r"\\?\Volume{") {
                volume = Some(line);
            } else if line.eq_ignore_ascii_case(root) {
                let line = volume?;
                let start = line.find('{')?;
                let end = line.find('}')?;
                return Some(line[start..=end].to_owned());
            }
        }
        None
    }

    #[derive(Debug, Clone, Copy)]
    struct BinSettings {
        off: bool,
        max_megabytes: Option<u32>,
    }

    /// The bin's settings for the volume, as Windows keeps them per user.
    fn bin_settings(volume: &str) -> Option<BinSettings> {
        let key = windows_registry::CURRENT_USER
            .open(format!(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume\{volume}"
            ))
            .ok()?;
        Some(BinSettings {
            off: key.get_u32("NukeOnDelete").is_ok_and(|value| value != 0),
            max_megabytes: key.get_u32("MaxCapacity").ok(),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_drive_is_found_in_mountvols_listing() {
            let listing = "Possible values for VolumeName along with current mount points are:\r\n\r\n    \\\\?\\Volume{1111-aaaa}\\\r\n        C:\\\r\n\r\n    \\\\?\\Volume{2222-bbbb}\\\r\n        *** NO MOUNT POINTS ***\r\n\r\n    \\\\?\\Volume{3333-cccc}\\\r\n        D:\\\r\n";
            assert_eq!(volume_of(listing, "C:\\").as_deref(), Some("{1111-aaaa}"));
            assert_eq!(volume_of(listing, "D:\\").as_deref(), Some("{3333-cccc}"));
            assert_eq!(volume_of(listing, "E:\\"), None);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    use bardo_domain::Disposal;

    /// A desktop's trash (where a source build runs off Windows) takes
    /// anything.
    pub(super) fn disposal(_: &Path, _: u64) -> Disposal {
        Disposal::Recycle
    }
}

/// A Recycle Bin in memory, for tests: drives marked removable or on the
/// network, a bin of a set room, and what was sent where.
#[derive(Debug, Default)]
pub struct MemoryTrash {
    state: Mutex<MemoryBin>,
}

#[derive(Debug, Default)]
struct MemoryBin {
    removable: Vec<PathBuf>,
    network: Vec<PathBuf>,
    /// The bin's room; unlimited when `None`.
    room: Option<u64>,
    recycled: Vec<PathBuf>,
    removed: Vec<PathBuf>,
    /// Paths that refuse to go, as a file left open.
    refusing: Vec<PathBuf>,
}

impl MemoryTrash {
    fn state(&self) -> std::sync::MutexGuard<'_, MemoryBin> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Everything under `root` is on a removable drive.
    pub fn removable(&self, root: &Path) {
        self.state().removable.push(root.to_owned());
    }

    /// Everything under `root` is on a network drive.
    pub fn network(&self, root: &Path) {
        self.state().network.push(root.to_owned());
    }

    /// The bin holds at most `bytes` at once.
    pub fn bin_room(&self, bytes: u64) {
        self.state().room = Some(bytes);
    }

    /// `path` refuses to go.
    pub fn refuse(&self, path: &Path) {
        self.state().refusing.push(path.to_owned());
    }

    /// What went to the bin.
    pub fn recycled(&self) -> Vec<PathBuf> {
        self.state().recycled.clone()
    }

    /// What was deleted at once.
    pub fn removed(&self) -> Vec<PathBuf> {
        self.state().removed.clone()
    }
}

impl Trash for MemoryTrash {
    fn disposal(&self, path: &Path, size: u64) -> Disposal {
        let state = self.state();
        if state.removable.iter().any(|root| path.starts_with(root)) {
            Disposal::Permanent(PermanentReason::RemovableDrive)
        } else if state.network.iter().any(|root| path.starts_with(root)) {
            Disposal::Permanent(PermanentReason::NetworkDrive)
        } else if state.room.is_some_and(|room| size > room) {
            Disposal::Permanent(PermanentReason::LargerThanBin)
        } else {
            Disposal::Recycle
        }
    }

    fn discard(&self, path: &Path, how: Discard) -> io::Result<()> {
        let mut state = self.state();
        if state.refusing.iter().any(|refusing| path == refusing) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        match how {
            Discard::Recycle => state.recycled.push(path.to_owned()),
            Discard::Permanent => state.removed.push(path.to_owned()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_path_is_nothing_to_discard() {
        let dir = tempfile::tempdir().unwrap();
        SystemTrash
            .discard(&dir.path().join("gone"), Discard::Permanent)
            .unwrap();
    }

    #[test]
    fn a_permanent_discard_removes_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Space");
        fs::create_dir_all(folder.join("Videos").join("Apollo")).unwrap();
        fs::write(folder.join("Videos").join("Apollo").join("a.mp4"), b"x").unwrap();
        SystemTrash.discard(&folder, Discard::Permanent).unwrap();
        assert!(!folder.exists());
    }

    #[test]
    fn the_memory_bin_tells_each_reason() {
        let bin = MemoryTrash::default();
        bin.removable(Path::new("/usb"));
        bin.network(Path::new("/net"));
        bin.bin_room(100);
        assert_eq!(
            bin.disposal(Path::new("/usb/Space"), 1),
            Disposal::Permanent(PermanentReason::RemovableDrive)
        );
        assert_eq!(
            bin.disposal(Path::new("/net/Space"), 1),
            Disposal::Permanent(PermanentReason::NetworkDrive)
        );
        assert_eq!(
            bin.disposal(Path::new("/disk/Space"), 101),
            Disposal::Permanent(PermanentReason::LargerThanBin)
        );
        assert_eq!(
            bin.disposal(Path::new("/disk/Space"), 100),
            Disposal::Recycle
        );
    }
}
