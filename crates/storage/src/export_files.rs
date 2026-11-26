//! Export packages on disk: `<root>/<package>/<network>/<file>`. The root
//! is the user's Videos folder (`Videos\Bardo`), where the files are easy
//! to find and drag into a network's uploader; unlike project folders it
//! is meant to be opened by hand.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bardo_domain::{ExportFiles, FrameShape, Network, ProjectFileError};

/// `Videos\Bardo` (the platform's videos folder), else `Documents\Bardo`,
/// else the home folder's.
pub fn default_exports_dir() -> PathBuf {
    dirs::video_dir()
        .or_else(dirs::document_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Bardo")
}

fn error(name: &str, source: std::io::Error) -> ProjectFileError {
    ProjectFileError {
        name: name.to_owned(),
        source,
    }
}

/// Package and file names are plain names; anything that could leave the
/// folder is refused.
fn checked(name: &str) -> Result<&str, ProjectFileError> {
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && Path::new(name).file_name().is_some_and(|n| n == name);
    if plain {
        Ok(name)
    } else {
        Err(error(
            name,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a plain file name"),
        ))
    }
}

/// Export packages under one root directory.
#[derive(Debug, Clone)]
pub struct LocalExportFiles {
    root: PathBuf,
}

impl LocalExportFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The network's folder, created if needed.
    fn ensure(
        &self,
        package: &str,
        network: Network,
        name: &str,
    ) -> Result<PathBuf, ProjectFileError> {
        let folder = self.root.join(checked(package)?).join(network.brand());
        std::fs::create_dir_all(&folder).map_err(|e| error(name, e))?;
        Ok(folder)
    }

    /// The folder of a shape of the export for ads, created if needed.
    fn ensure_ad(
        &self,
        package: &str,
        shape: FrameShape,
        name: &str,
    ) -> Result<PathBuf, ProjectFileError> {
        let folder = self.ad_folder(checked(package)?, shape);
        std::fs::create_dir_all(&folder).map_err(|e| error(name, e))?;
        Ok(folder)
    }
}

/// Writes `name` in `folder` through a file beside it, renamed over the
/// name once whole.
fn write_through(
    folder: &Path,
    name: &str,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<(), ProjectFileError> {
    let partial = folder.join(format!("{name}.partial"));
    let written = std::fs::File::create(&partial)
        .and_then(|mut file| {
            fill(&mut file)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&partial, folder.join(name)));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written.map_err(|e| error(name, e))
}

/// The folder of an export for ads in a package.
const ADS_FOLDER: &str = "Ads";

impl ExportFiles for LocalExportFiles {
    fn write(
        &self,
        package: &str,
        network: Network,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        let folder = self.ensure(package, network, name)?;
        let partial = folder.join(format!("{name}.partial"));
        let written = std::fs::File::create(&partial)
            .and_then(|mut file| {
                file.write_all(bytes)?;
                file.sync_all()
            })
            .and_then(|()| std::fs::rename(&partial, folder.join(name)));
        if written.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        written.map_err(|e| error(name, e))
    }

    fn copy_from(
        &self,
        package: &str,
        network: Network,
        name: &str,
        source: &mut dyn std::io::Read,
    ) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        let folder = self.ensure(package, network, name)?;
        // Copied beside it, then renamed over the name: as project files
        // copy in.
        let partial = folder.join(format!("{name}.partial"));
        let copied = std::fs::File::create(&partial)
            .and_then(|mut copy| {
                std::io::copy(source, &mut copy)?;
                copy.sync_all()
            })
            .and_then(|()| std::fs::rename(&partial, folder.join(name)));
        if copied.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        copied.map_err(|e| error(name, e))
    }

    fn remove(&self, package: &str, network: Network, name: &str) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        let path = self
            .root
            .join(checked(package)?)
            .join(network.brand())
            .join(name);
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(error(name, e)),
            _ => Ok(()),
        }
    }

    fn exists(&self, package: &str, network: Network, name: &str) -> bool {
        checked(package).is_ok()
            && checked(name).is_ok()
            && self
                .root
                .join(package)
                .join(network.brand())
                .join(name)
                .is_file()
    }

    fn folder(&self, package: &str, network: Network) -> PathBuf {
        self.root.join(package).join(network.brand())
    }

    fn copy_ad_from(
        &self,
        package: &str,
        shape: FrameShape,
        name: &str,
        source: &mut dyn std::io::Read,
    ) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        let folder = self.ensure_ad(package, shape, name)?;
        write_through(&folder, name, |file| std::io::copy(source, file).map(drop))
    }

    fn write_ad(
        &self,
        package: &str,
        shape: FrameShape,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        let folder = self.ensure_ad(package, shape, name)?;
        write_through(&folder, name, |file| file.write_all(bytes))
    }

    fn remove_ad(
        &self,
        package: &str,
        shape: FrameShape,
        name: &str,
    ) -> Result<(), ProjectFileError> {
        let name = checked(name)?;
        let path = self.ad_folder(checked(package)?, shape).join(name);
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(error(name, e)),
            _ => Ok(()),
        }
    }

    fn ad_folder(&self, package: &str, shape: FrameShape) -> PathBuf {
        self.root
            .join(package)
            .join(ADS_FOLDER)
            .join(shape.folder())
    }
}

/// Export packages kept in memory, for tests. Paths point nowhere.
#[derive(Debug, Default)]
pub struct MemoryExportFiles {
    files: Mutex<HashMap<(String, Network, String), Vec<u8>>>,
    /// The export for ads, by package, shape and name.
    ads: Mutex<HashMap<(String, FrameShape, String), Vec<u8>>>,
}

impl MemoryExportFiles {
    /// A file's bytes, if written.
    pub fn read(&self, package: &str, network: Network, name: &str) -> Option<Vec<u8>> {
        self.files
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&(package.to_owned(), network, name.to_owned()))
            .cloned()
    }

    /// The names in a network's folder, sorted.
    pub fn names(&self, package: &str, network: Network) -> Vec<String> {
        let files = self.files.lock().unwrap_or_else(|p| p.into_inner());
        let mut names: Vec<_> = files
            .keys()
            .filter(|(p, n, _)| p == package && *n == network)
            .map(|(_, _, name)| name.clone())
            .collect();
        names.sort();
        names
    }
}

impl MemoryExportFiles {
    /// A file of a shape of the export for ads, if written.
    pub fn read_ad(&self, package: &str, shape: FrameShape, name: &str) -> Option<Vec<u8>> {
        self.ads
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&(package.to_owned(), shape, name.to_owned()))
            .cloned()
    }

    /// The names in a shape's folder of the export for ads, sorted.
    pub fn ad_names(&self, package: &str, shape: FrameShape) -> Vec<String> {
        let files = self.ads.lock().unwrap_or_else(|p| p.into_inner());
        let mut names: Vec<_> = files
            .keys()
            .filter(|(p, s, _)| p == package && *s == shape)
            .map(|(_, _, name)| name.clone())
            .collect();
        names.sort();
        names
    }
}

impl ExportFiles for MemoryExportFiles {
    fn write(
        &self,
        package: &str,
        network: Network,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError> {
        let (package, name) = (checked(package)?, checked(name)?);
        self.files.lock().unwrap_or_else(|p| p.into_inner()).insert(
            (package.to_owned(), network, name.to_owned()),
            bytes.to_vec(),
        );
        Ok(())
    }

    fn copy_from(
        &self,
        package: &str,
        network: Network,
        name: &str,
        source: &mut dyn std::io::Read,
    ) -> Result<(), ProjectFileError> {
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).map_err(|e| error(name, e))?;
        self.write(package, network, name, &bytes)
    }

    fn remove(&self, package: &str, network: Network, name: &str) -> Result<(), ProjectFileError> {
        self.files
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&(package.to_owned(), network, name.to_owned()));
        Ok(())
    }

    fn exists(&self, package: &str, network: Network, name: &str) -> bool {
        self.read(package, network, name).is_some()
    }

    fn folder(&self, package: &str, network: Network) -> PathBuf {
        PathBuf::from("memory").join(package).join(network.brand())
    }

    fn copy_ad_from(
        &self,
        package: &str,
        shape: FrameShape,
        name: &str,
        source: &mut dyn std::io::Read,
    ) -> Result<(), ProjectFileError> {
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).map_err(|e| error(name, e))?;
        self.write_ad(package, shape, name, &bytes)
    }

    fn write_ad(
        &self,
        package: &str,
        shape: FrameShape,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError> {
        let (package, name) = (checked(package)?, checked(name)?);
        self.ads
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert((package.to_owned(), shape, name.to_owned()), bytes.to_vec());
        Ok(())
    }

    fn remove_ad(
        &self,
        package: &str,
        shape: FrameShape,
        name: &str,
    ) -> Result<(), ProjectFileError> {
        self.ads.lock().unwrap_or_else(|p| p.into_inner()).remove(&(
            package.to_owned(),
            shape,
            name.to_owned(),
        ));
        Ok(())
    }

    fn ad_folder(&self, package: &str, shape: FrameShape) -> PathBuf {
        PathBuf::from("memory")
            .join(package)
            .join(ADS_FOLDER)
            .join(shape.folder())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_export_for_ads_holds_a_folder_per_shape() {
        let dir = tempfile::tempdir().unwrap();
        let exports = LocalExportFiles::new(dir.path().join("Bardo"));
        let package = "The Lamp (1a2b3c4d)";
        exports
            .copy_ad_from(
                package,
                FrameShape::Portrait,
                "The Lamp.mp4",
                &mut &b"video"[..],
            )
            .unwrap();
        exports
            .write_ad(package, FrameShape::Portrait, "ad.txt", b"Primary text")
            .unwrap();
        let folder = exports.ad_folder(package, FrameShape::Portrait);
        assert_eq!(
            folder,
            dir.path()
                .join("Bardo")
                .join(package)
                .join("Ads")
                .join("4x5")
        );
        assert_eq!(
            std::fs::read(folder.join("ad.txt")).unwrap(),
            b"Primary text"
        );
        assert_eq!(
            std::fs::read(folder.join("The Lamp.mp4")).unwrap(),
            b"video"
        );
        exports
            .remove_ad(package, FrameShape::Portrait, "The Lamp.mp4")
            .unwrap();
        exports
            .remove_ad(package, FrameShape::Portrait, "The Lamp.mp4")
            .unwrap();
        assert!(!folder.join("The Lamp.mp4").exists());
        assert!(
            exports
                .write_ad(package, FrameShape::Square, "../x.txt", b"")
                .is_err()
        );
    }

    #[test]
    fn packages_hold_a_folder_per_network() {
        let dir = tempfile::tempdir().unwrap();
        let exports = LocalExportFiles::new(dir.path().join("Bardo"));
        exports
            .copy_from(
                "The Moon (1a2b3c4d)",
                Network::YouTube,
                "The Moon.mp4",
                &mut &b"video"[..],
            )
            .unwrap();
        exports
            .write(
                "The Moon (1a2b3c4d)",
                Network::YouTube,
                "metadata.txt",
                b"Title",
            )
            .unwrap();
        exports
            .write(
                "The Moon (1a2b3c4d)",
                Network::InstagramReels,
                "metadata.txt",
                b"Caption",
            )
            .unwrap();

        let folder = exports.folder("The Moon (1a2b3c4d)", Network::YouTube);
        assert_eq!(
            folder,
            dir.path()
                .join("Bardo")
                .join("The Moon (1a2b3c4d)")
                .join("YouTube")
        );
        assert_eq!(
            std::fs::read(folder.join("The Moon.mp4")).unwrap(),
            b"video"
        );
        let mut names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["The Moon.mp4", "metadata.txt"],
            "no partial file stays"
        );
        assert!(exports.exists(
            "The Moon (1a2b3c4d)",
            Network::InstagramReels,
            "metadata.txt"
        ));
        assert!(
            exports
                .folder("The Moon (1a2b3c4d)", Network::InstagramReels)
                .ends_with("Instagram Reels")
        );

        exports
            .remove("The Moon (1a2b3c4d)", Network::YouTube, "The Moon.mp4")
            .unwrap();
        exports
            .remove("The Moon (1a2b3c4d)", Network::YouTube, "The Moon.mp4")
            .unwrap();
        assert!(!exports.exists("The Moon (1a2b3c4d)", Network::YouTube, "The Moon.mp4"));
    }

    #[test]
    fn names_that_leave_the_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let exports = LocalExportFiles::new(dir.path());
        assert!(exports.write("..", Network::X, "a.txt", b"").is_err());
        assert!(exports.write("p", Network::X, "../a.txt", b"").is_err());
        assert!(exports.write("p", Network::X, "C:a.txt", b"").is_err());
        assert!(!exports.exists("..", Network::X, "a.txt"));
    }
}
