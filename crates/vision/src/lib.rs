//! Local vision models for performance capture (ADR-0024): the face mesh,
//! the depth map and the person mask of each take, run on the computer
//! through ONNX Runtime, on DirectML where it starts and on the CPU
//! otherwise.
//!
//! The models are pinned in `models.toml` and mirrored as assets of the
//! release it names; `tools/convert.py` makes them from their sources,
//! `cargo xtask fetch-models` downloads them and the installer bundles
//! them in a `models` folder next to Bardo.

mod depth;
mod face;
mod frame;
mod mask;
mod runtime;
mod state;

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bardo_domain::{
    DepthEstimator, FaceTracker, ModelFile, ModelInfo, PersonSegmenter, VisionError, VisionModels,
};
use sha2::Digest as _;

pub use runtime::Wanted;

/// The manifest the models are checked against.
pub const MANIFEST: &str = include_str!("../models.toml");

/// The environment variable naming the folder that holds the models.
/// `.cargo/config.toml` points it at `.models` for cargo runs.
pub const DIR_VARIABLE: &str = "BARDO_MODELS_DIR";

/// Where the installer puts the models, next to the Bardo executable.
pub const INSTALLED_DIR: &str = "models";

/// The environment variable that keeps every model on the CPU (`cpu`),
/// for checking the fallback on a machine with a graphics card.
pub const DEVICE_VARIABLE: &str = "BARDO_VISION_DEVICE";

/// Video Depth Anything needs about this much free memory on the CPU
/// (the spike measured 4.4 GB); with less, Depth Anything V2 runs instead.
pub const VIDEO_DEPTH_MEMORY: u64 = 4_500_000_000;

/// The depth models' input, landscape (height × width); vertical takes
/// use the same turned.
const DEPTH_SIZE: (usize, usize) = (280, 504);

#[derive(Debug, serde::Deserialize)]
struct Source {
    name: String,
    version: String,
    license: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ModelEntry {
    pub file: String,
    pub from: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, serde::Deserialize)]
struct Manifest {
    mirror: String,
    sources: HashMap<String, Source>,
    model: Vec<ModelEntry>,
}

/// The manifest as Bardo was built with it.
fn manifest() -> Manifest {
    toml::from_str(MANIFEST).expect("models.toml is valid")
}

/// Every file the manifest pins, for the fetch and the installer.
pub fn pinned_files() -> Vec<ModelEntry> {
    manifest().model
}

/// The release the files are downloaded from.
pub fn mirror() -> String {
    manifest().mirror
}

/// What each source is and its license, one line each, for the notices
/// the installer ships with the models.
pub fn notices() -> String {
    let manifest = manifest();
    let mut sources: Vec<_> = manifest.sources.values().collect();
    sources.sort_by(|a, b| a.name.cmp(&b.name));
    sources
        .into_iter()
        .map(|s| format!("{} {} ({})\n", s.name, s.version, s.license))
        .collect()
}

/// SHA-256 of a file, in lowercase hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The folder holding the models: the one [`DIR_VARIABLE`] names, else
/// [`INSTALLED_DIR`] next to the running executable.
pub fn locate() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(DIR_VARIABLE) {
        return Some(PathBuf::from(dir));
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(INSTALLED_DIR)))
}

/// The models in one folder, each checked against its pin the first time
/// it is loaded.
pub struct OnnxVision {
    dir: PathBuf,
    wanted: Wanted,
    manifest: Manifest,
    checked: Mutex<HashMap<String, Result<(), VisionError>>>,
}

impl OnnxVision {
    pub fn new(dir: impl Into<PathBuf>, wanted: Wanted) -> OnnxVision {
        OnnxVision {
            dir: dir.into(),
            wanted,
            manifest: manifest(),
            checked: Mutex::default(),
        }
    }

    /// The models where Bardo keeps them, on the graphics card unless
    /// [`DEVICE_VARIABLE`] says `cpu`.
    pub fn installed() -> OnnxVision {
        let wanted = match std::env::var(DEVICE_VARIABLE).as_deref() {
            Ok("cpu") => Wanted::Cpu,
            _ => Wanted::Gpu,
        };
        OnnxVision::new(
            locate().unwrap_or_else(|| PathBuf::from(INSTALLED_DIR)),
            wanted,
        )
    }

    fn entry(&self, file: &str) -> Result<&ModelEntry, VisionError> {
        self.manifest
            .model
            .iter()
            .find(|entry| entry.file == file)
            .ok_or_else(|| VisionError::Missing(file.to_owned()))
    }

    /// The path of `file`, once its size and SHA-256 are the pinned ones.
    fn checked_path(&self, file: &str) -> Result<PathBuf, VisionError> {
        let entry = self.entry(file)?;
        let path = self.dir.join(file);
        let mut checked = self.checked.lock().unwrap_or_else(|p| p.into_inner());
        let result = checked
            .entry(file.to_owned())
            .or_insert_with(|| {
                let missing = || VisionError::Missing(file.to_owned());
                let size = std::fs::metadata(&path).map_err(|_| missing())?.len();
                if entry.sha256.is_empty() || size != entry.size {
                    return Err(missing());
                }
                let digest = sha256_file(&path).map_err(|_| missing())?;
                if digest != entry.sha256 {
                    tracing::warn!(file, "vision model does not match its pin");
                    return Err(missing());
                }
                Ok(())
            })
            .clone();
        result.map(|()| path)
    }

    fn load(&self, file: &str, threads: usize) -> Result<runtime::Model, VisionError> {
        runtime::Model::load(&self.checked_path(file)?, self.wanted, threads)
    }

    fn info(&self, files: &[&str]) -> Result<ModelInfo, VisionError> {
        let from = &self.entry(files[0])?.from;
        let source = self
            .manifest
            .sources
            .get(from)
            .ok_or_else(|| VisionError::Missing(from.clone()))?;
        Ok(ModelInfo {
            name: source.name.clone(),
            version: source.version.clone(),
            files: files
                .iter()
                .map(|file| {
                    Ok(ModelFile {
                        file: (*file).to_owned(),
                        sha256: self.entry(file)?.sha256.clone(),
                    })
                })
                .collect::<Result<_, VisionError>>()?,
        })
    }
}

/// Which depth model runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthKind {
    /// Video Depth Anything Small, in windows of 32 frames.
    Video,
    /// Depth Anything V2 Small, a frame at a time, smoothed.
    PerFrame,
}

impl OnnxVision {
    /// The depth estimator of `kind` for a vertical take or not.
    pub fn depth_of_kind(
        &self,
        vertical: bool,
        kind: DepthKind,
    ) -> Result<Box<dyn DepthEstimator>, VisionError> {
        let (h, w) = if vertical {
            (DEPTH_SIZE.1, DEPTH_SIZE.0)
        } else {
            DEPTH_SIZE
        };
        match kind {
            DepthKind::Video => {
                let file = format!("video_depth_anything_small_32x{h}x{w}.onnx");
                Ok(Box::new(depth::Windowed::new(
                    self.load(&file, 0)?,
                    self.info(&[&file])?,
                    w,
                    h,
                )))
            }
            DepthKind::PerFrame => {
                let file = format!("depth_anything_v2_small_{h}x{w}.onnx");
                let mut info = self.info(&[&file])?;
                info.version = format!("{} smoothed", info.version);
                Ok(Box::new(depth::PerFrame::new(
                    self.load(&file, 0)?,
                    info,
                    w,
                    h,
                )))
            }
        }
    }
}

/// Free memory, in bytes.
fn free_memory() -> u64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.available_memory()
}

impl VisionModels for OnnxVision {
    fn face_tracker(&self) -> Result<Box<dyn FaceTracker>, VisionError> {
        const FILES: [&str; 3] = [
            "face_detector.onnx",
            "face_landmarks_detector.onnx",
            "face_blendshapes.onnx",
        ];
        // Small models: one thread is faster than several (the spike).
        Ok(Box::new(face::OnnxFaceTracker::new(
            self.load(FILES[0], 1)?,
            self.load(FILES[1], 1)?,
            self.load(FILES[2], 1)?,
            self.info(&FILES)?,
        )))
    }

    fn person_segmenter(&self) -> Result<Box<dyn PersonSegmenter>, VisionError> {
        const FILE: &str = "selfie_multiclass_256x256.onnx";
        Ok(Box::new(mask::OnnxSegmenter::new(
            self.load(FILE, 0)?,
            self.info(&[FILE])?,
        )))
    }

    fn depth_estimator(&self, vertical: bool) -> Result<Box<dyn DepthEstimator>, VisionError> {
        if free_memory() >= VIDEO_DEPTH_MEMORY {
            return self.depth_of_kind(vertical, DepthKind::Video);
        }
        tracing::info!("not enough free memory for Video Depth Anything: Depth Anything V2 runs");
        self.depth_of_kind(vertical, DepthKind::PerFrame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_lists_every_file_the_crate_loads() {
        let files: Vec<String> = pinned_files().into_iter().map(|m| m.file).collect();
        for file in [
            "face_detector.onnx",
            "face_landmarks_detector.onnx",
            "face_blendshapes.onnx",
            "selfie_multiclass_256x256.onnx",
            "depth_anything_v2_small_280x504.onnx",
            "depth_anything_v2_small_504x280.onnx",
            "video_depth_anything_small_32x280x504.onnx",
            "video_depth_anything_small_32x504x280.onnx",
        ] {
            assert!(files.iter().any(|f| f == file), "{file}");
        }
        let manifest = manifest();
        for entry in &manifest.model {
            assert!(manifest.sources.contains_key(&entry.from), "{}", entry.from);
        }
        assert!(notices().contains("Apache-2.0"));
        assert!(mirror().contains("/releases/download/vision-models-"));
    }

    #[test]
    fn a_missing_or_unpinned_file_is_reported_missing() {
        let vision = OnnxVision::new("/nowhere", Wanted::Cpu);
        assert!(matches!(
            vision.face_tracker(),
            Err(VisionError::Missing(_))
        ));
    }
}
