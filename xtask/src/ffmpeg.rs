//! `cargo xtask fetch-ffmpeg`: downloads the ffmpeg build pinned in
//! `crates/media/ffmpeg.toml` for this platform, checks its SHA-256 and
//! unpacks its `bin/` folder (and license) into `.ffmpeg/bin`, where
//! `bardo-media` finds it through `.cargo/config.toml`.
//!
//! `--archive <path>` uses an archive downloaded by other means (offline,
//! or behind a proxy the platform verifier does not trust); the SHA-256
//! check still applies.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    #[serde(rename = "windows-x86_64")]
    windows: Archive,
    #[serde(rename = "linux-x86_64")]
    linux: Archive,
}

#[derive(Debug, Clone, Deserialize)]
struct Archive {
    url: String,
    sha256: String,
}

/// Files of the archive's `bin/` folder Bardo does not use.
const UNUSED: [&str; 2] = ["ffplay", "ffplay.exe"];

pub fn fetch(workspace: &Path, archive_path: Option<PathBuf>) -> anyhow::Result<()> {
    let manifest_path = workspace.join("crates/media/ffmpeg.toml");
    let manifest: Manifest = toml::from_str(
        &fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?,
    )
    .context("parsing ffmpeg.toml")?;
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
    let file_name = archive.url.rsplit('/').next().unwrap_or("ffmpeg-archive");
    let (download, digest) = match archive_path {
        Some(path) => {
            let digest = sha256_of(&mut fs::File::open(&path)?, None)?;
            (path, digest)
        }
        None => {
            let download = root.join(file_name);
            println!("fetch-ffmpeg: downloading {}", archive.url);
            let digest = download_to(&archive.url, &download)?;
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

/// Streams the body to `path` and returns its SHA-256 in hex.
fn download_to(url: &str, path: &Path) -> anyhow::Result<String> {
    let response = ureq::get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let mut reader = response.into_body().into_reader();
    let mut file = io::BufWriter::new(fs::File::create(path)?);
    let digest = sha256_of(&mut reader, Some(&mut file))?;
    file.flush()?;
    Ok(digest)
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
