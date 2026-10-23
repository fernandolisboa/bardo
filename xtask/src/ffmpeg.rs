//! `cargo xtask fetch-ffmpeg`: downloads the ffmpeg build pinned in
//! `crates/media/ffmpeg.toml` for this platform, checks its SHA-256 and
//! unpacks its `bin/` folder (and license) into `.ffmpeg/bin`, where
//! `bardo-media` finds it through `.cargo/config.toml`.
//!
//! `--archive <path>` uses an archive downloaded by other means (offline,
//! or behind a proxy the platform verifier does not trust); the SHA-256
//! check still applies. When the upstream link is gone, the archive comes
//! from Bardo's own mirror (`mirror` in the manifest, issue #135).
//!
//! `cargo xtask mirror-ffmpeg <dir>` downloads both archives and the
//! matching FFmpeg source into `<dir>`, for the release that mirrors them.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

#[derive(Debug, Deserialize)]
pub(crate) struct Manifest {
    pub(crate) version: String,
    /// Where Bardo's own copies of the archives are, by file name.
    pub(crate) mirror: String,
    #[serde(rename = "windows-x86_64")]
    pub(crate) windows: Archive,
    #[serde(rename = "linux-x86_64")]
    pub(crate) linux: Archive,
    pub(crate) source: Source,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Archive {
    pub(crate) url: String,
    pub(crate) sha256: String,
}

impl Archive {
    pub(crate) fn file_name(&self) -> &str {
        self.url.rsplit('/').next().unwrap_or("ffmpeg-archive")
    }
}

/// The FFmpeg source the builds were made from (the LGPL source offer).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Source {
    /// The source at the build's commit.
    pub(crate) url: String,
    /// Its file name in the mirror.
    pub(crate) file: String,
    /// The scripts and configure options the builds were made with.
    pub(crate) build: String,
}

impl Manifest {
    pub(crate) fn read(workspace: &Path) -> anyhow::Result<Manifest> {
        let path = workspace.join("crates/media/ffmpeg.toml");
        toml::from_str(
            &fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
        )
        .context("parsing ffmpeg.toml")
    }

    /// Bardo's copy of `file`.
    pub(crate) fn mirrored(&self, file: &str) -> String {
        format!("{}/{file}", self.mirror.trim_end_matches('/'))
    }
}

/// Files of the archive's `bin/` folder Bardo does not use.
pub(crate) const UNUSED: [&str; 2] = ["ffplay", "ffplay.exe"];

pub fn fetch(workspace: &Path, archive_path: Option<PathBuf>) -> anyhow::Result<()> {
    let manifest = Manifest::read(workspace)?;
    let archive = if cfg!(windows) {
        manifest.windows.clone()
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        manifest.linux.clone()
    } else {
        bail!("no pinned ffmpeg build for this platform");
    };

    let root = workspace.join(".ffmpeg");
    let bin = root.join("bin");
    let stamp = root.join("SHA256");
    if bin.is_dir() && fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == archive.sha256) {
        println!(
            "fetch-ffmpeg: ffmpeg {} already in {}",
            manifest.version,
            bin.display()
        );
        return Ok(());
    }

    fs::create_dir_all(&root)?;
    let file_name = archive.file_name();
    let (download, digest) = match archive_path {
        Some(path) => {
            let digest = sha256_of(&mut fs::File::open(&path)?, None)?;
            (path, digest)
        }
        None => {
            let download = root.join(file_name);
            let digest = download_either(
                &[archive.url.clone(), manifest.mirrored(file_name)],
                &download,
            )?;
            (download, digest)
        }
    };
    let downloaded = download.starts_with(&root);
    if !digest.eq_ignore_ascii_case(&archive.sha256) {
        if downloaded {
            let _ = fs::remove_file(&download);
        }
        bail!(
            "SHA-256 mismatch for {file_name}: expected {}, got {digest}",
            archive.sha256
        );
    }

    let unpack = root.join("unpack");
    if unpack.exists() {
        fs::remove_dir_all(&unpack)?;
    }
    fs::create_dir_all(&unpack)?;
    extract(&download, &unpack)?;
    let top = only_child_dir(&unpack)?;

    if bin.exists() {
        fs::remove_dir_all(&bin)?;
    }
    fs::rename(top.join("bin"), &bin).context("moving bin/ into place")?;
    for unused in UNUSED {
        let _ = fs::remove_file(bin.join(unused));
    }
    fs::copy(top.join("LICENSE.txt"), bin.join("LICENSE.txt")).context("copying LICENSE.txt")?;
    fs::remove_dir_all(&unpack)?;
    if downloaded {
        fs::remove_file(&download)?;
    }
    fs::write(&stamp, &archive.sha256)?;
    println!(
        "fetch-ffmpeg: ffmpeg {} in {}",
        manifest.version,
        bin.display()
    );
    Ok(())
}

/// `cargo xtask mirror-ffmpeg <dir>`: both pinned archives, checked, and
/// the matching source, with release notes, for the mirror release.
pub fn mirror(workspace: &Path, dir: &Path) -> anyhow::Result<()> {
    let manifest = Manifest::read(workspace)?;
    fs::create_dir_all(dir)?;
    for archive in [&manifest.windows, &manifest.linux] {
        let path = dir.join(archive.file_name());
        let digest = download_either(std::slice::from_ref(&archive.url), &path)?;
        if !digest.eq_ignore_ascii_case(&archive.sha256) {
            bail!(
                "SHA-256 mismatch for {}: expected {}, got {digest}",
                archive.file_name(),
                archive.sha256
            );
        }
    }
    let source = dir.join(&manifest.source.file);
    let source_digest = download_either(std::slice::from_ref(&manifest.source.url), &source)?;
    fs::write(
        dir.join("NOTES.md"),
        mirror_notes(&manifest, &source_digest),
    )?;
    println!(
        "mirror-ffmpeg: ffmpeg {} in {}",
        manifest.version,
        dir.display()
    );
    Ok(())
}

/// The mirror release's description.
fn mirror_notes(manifest: &Manifest, source_digest: &str) -> String {
    let version = &manifest.version;
    let windows = &manifest.windows;
    let linux = &manifest.linux;
    let source = &manifest.source;
    format!(
        "Bardo's copy of the FFmpeg {version} build it ships (ADR-0007), \
         and the source it was built from.\n\n\
         FFmpeg is licensed under the GNU Lesser General Public License \
         version 3 (LGPL v3); Bardo runs it as separate programs and ships \
         its libraries unmodified.\n\n\
         | File | From | SHA-256 |\n| --- | --- | --- |\n\
         | `{}` | {} | `{}` |\n\
         | `{}` | {} | `{}` |\n\
         | `{}` (source) | {} | `{source_digest}` |\n\n\
         Build scripts and configure options: {}\n",
        windows.file_name(),
        windows.url,
        windows.sha256,
        linux.file_name(),
        linux.url,
        linux.sha256,
        source.file,
        source.url,
        source.build,
    )
}

/// Downloads the first of `urls` that answers to `path`, and returns its
/// SHA-256 in hex.
fn download_either(urls: &[String], path: &Path) -> anyhow::Result<String> {
    let mut failures = Vec::new();
    for url in urls {
        println!("ffmpeg: downloading {url}");
        match download_to(url, path) {
            Ok(digest) => return Ok(digest),
            Err(error) => {
                let _ = fs::remove_file(path);
                eprintln!("ffmpeg: {error:#}");
                failures.push(url.as_str());
            }
        }
    }
    bail!("could not download from {}", failures.join(" or "))
}

/// Streams the body to `path` and returns its SHA-256 in hex.
pub(crate) fn download_to(url: &str, path: &Path) -> anyhow::Result<String> {
    let response = ureq::get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let mut reader = response.into_body().into_reader();
    let mut file = io::BufWriter::new(fs::File::create(path)?);
    let digest = sha256_of(&mut reader, Some(&mut file))?;
    file.flush()?;
    Ok(digest)
}

/// The SHA-256 of the file at `path`, in hex.
pub(crate) fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    sha256_of(&mut file, None)
}

/// Reads `reader` to the end, copying it to `copy` when given, and returns
/// its SHA-256 in hex.
fn sha256_of(
    reader: &mut dyn io::Read,
    mut copy: Option<&mut dyn io::Write>,
) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        if let Some(copy) = copy.as_mut() {
            copy.write_all(&buffer[..read])?;
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Unpacks a .zip or .tar.xz with the system `tar` (bsdtar on Windows 10+,
/// which reads zip; GNU tar on Linux).
fn extract(archive: &Path, into: &Path) -> anyhow::Result<()> {
    let tar = if cfg!(windows) {
        // Git for Windows puts GNU tar, which cannot read zip, ahead on PATH.
        let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(system_root).join("System32").join("tar.exe")
    } else {
        PathBuf::from("tar")
    };
    let status = Command::new(&tar)
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .status()
        .with_context(|| format!("running {}", tar.display()))?;
    if !status.success() {
        bail!(
            "{} failed on {}: {status}",
            tar.display(),
            archive.display()
        );
    }
    Ok(())
}

fn only_child_dir(dir: &Path) -> anyhow::Result<PathBuf> {
    let mut dirs = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir());
    match (dirs.next(), dirs.next()) {
        (Some(only), None) => Ok(only),
        _ => bail!("expected one top-level folder in the ffmpeg archive"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
    }

    #[test]
    fn the_manifest_names_mirrored_files_and_the_source() {
        let manifest = Manifest::read(workspace()).unwrap();
        let windows = manifest.windows.file_name();
        assert!(windows.ends_with(".zip"), "{windows}");
        assert!(windows.contains("lgpl"), "the LGPL build: {windows}");
        assert_eq!(
            manifest.mirrored(windows),
            format!(
                "https://github.com/fernandolisboa/bardo/releases/download/ffmpeg-{}/{windows}",
                manifest.version
            )
        );
        assert!(manifest.source.url.starts_with("https://"));
        assert!(manifest.source.build.starts_with("https://"));
        let notes = mirror_notes(&manifest, "abc");
        assert!(notes.contains(&manifest.windows.sha256));
        assert!(notes.contains("LGPL v3"));
        assert!(notes.contains("`abc`"));
    }
}
