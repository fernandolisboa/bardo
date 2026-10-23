//! `cargo xtask fetch-models`: downloads the vision models pinned in
//! `crates/vision/models.toml` from Bardo's mirror into `.models`, where
//! `bardo-vision` finds them through `.cargo/config.toml`, and checks each
//! against its SHA-256 and size (ADR-0024). A file already there and
//! matching its pin is kept. `--dir <path>` puts them elsewhere.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use serde::Deserialize;

use crate::ffmpeg::{download_to, sha256_file};

#[derive(Debug, Deserialize)]
pub(crate) struct Manifest {
    pub(crate) mirror: String,
    pub(crate) model: Vec<Model>,
    sources: std::collections::BTreeMap<String, Source>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Model {
    pub(crate) file: String,
    pub(crate) sha256: String,
    pub(crate) size: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct Source {
    name: String,
    version: String,
    url: String,
    license: String,
    license_url: String,
}

impl Manifest {
    pub(crate) fn read(workspace: &Path) -> anyhow::Result<Manifest> {
        let path = workspace.join("crates/vision/models.toml");
        toml::from_str(
            &fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
        )
        .context("parsing models.toml")
    }

    fn mirrored(&self, file: &str) -> String {
        format!("{}/{file}", self.mirror.trim_end_matches('/'))
    }

    /// What the installer ships beside the models: each source, where it
    /// came from and its license.
    pub(crate) fn notices(&self) -> String {
        let mut out = String::from(
            "Bardo's local vision models (ADR-0024), converted to ONNX from these sources.\n\
             Each is under its own license, linked below.\n\n",
        );
        for source in self.sources.values() {
            out.push_str(&format!(
                "{} {}\n  Source: {}\n  License: {} ({})\n\n",
                source.name, source.version, source.url, source.license, source.license_url
            ));
        }
        out
    }
}

/// Whether `path` is the pinned file.
pub(crate) fn matches(path: &Path, model: &Model) -> bool {
    fs::metadata(path).is_ok_and(|m| m.len() == model.size)
        && sha256_file(path).is_ok_and(|digest| digest == model.sha256)
}

pub fn fetch(workspace: &Path, dir: Option<PathBuf>) -> anyhow::Result<()> {
    let manifest = Manifest::read(workspace)?;
    let dir = dir.unwrap_or_else(|| workspace.join(".models"));
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    for model in &manifest.model {
        if model.sha256.is_empty() {
            bail!("{} is not pinned yet in models.toml", model.file);
        }
        let path = dir.join(&model.file);
        if matches(&path, model) {
            println!("fetch-models: {} already in {}", model.file, dir.display());
            continue;
        }
        let url = manifest.mirrored(&model.file);
        println!("fetch-models: downloading {url}");
        let partial = dir.join(format!("{}.partial", model.file));
        let digest = download_to(&url, &partial)?;
        let size = fs::metadata(&partial)?.len();
        if digest != model.sha256 || size != model.size {
            let _ = fs::remove_file(&partial);
            bail!(
                "{}: SHA-256 {digest} ({size} bytes), expected {} ({} bytes)",
                model.file,
                model.sha256,
                model.size
            );
        }
        fs::rename(&partial, &path)?;
    }
    println!(
        "fetch-models: {} models in {}",
        manifest.model.len(),
        dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
    }

    #[test]
    fn every_model_is_pinned_and_mirrored() {
        let manifest = Manifest::read(workspace()).unwrap();
        assert_eq!(manifest.model.len(), 8);
        for model in &manifest.model {
            assert_eq!(model.sha256.len(), 64, "{}", model.file);
            assert!(model.size > 0, "{}", model.file);
        }
        assert!(manifest.mirrored("face_detector.onnx").starts_with(
            "https://github.com/fernandolisboa/bardo/releases/download/vision-models-"
        ));
        let notices = manifest.notices();
        assert!(notices.contains("Apache-2.0"));
        assert!(notices.contains("Video Depth Anything"));
    }

    #[test]
    fn a_file_matches_only_its_pin() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.onnx");
        fs::write(&path, b"abc").unwrap();
        let pin = Model {
            file: "model.onnx".into(),
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            size: 3,
        };
        assert!(matches(&path, &pin));
        assert!(!matches(
            &path,
            &Model {
                size: 4,
                ..pin.clone()
            }
        ));
        fs::write(&path, b"abd").unwrap();
        assert!(!matches(&path, &pin));
    }
}
