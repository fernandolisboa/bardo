//! The manifest of the files `tools/convert.py` writes (`models.toml`):
//! which model each file holds and how a photo goes in and a mask comes
//! out.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Manifest {
    model: Vec<Entry>,
}

/// How pixel values are scaled before the mean and deviation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scale {
    /// Divided by 255.
    Byte,
    /// Divided by the photo's largest value, as U²-Net's own loader does.
    Max,
}

/// What the model's output needs to become an alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Range {
    /// Stretched from its smallest to its largest value, as U²-Net's and
    /// IS-Net's own inference does.
    MinMax,
    /// Already an alpha.
    AsIs,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub file: String,
    /// Height and width of the input.
    pub input: [usize; 2],
    pub scale: Scale,
    pub mean: [f32; 3],
    pub std: [f32; 3],
    pub range: Range,
}

impl Entry {
    /// The file's name without `.onnx`.
    pub fn name(&self) -> &str {
        self.file.strip_suffix(".onnx").unwrap_or(&self.file)
    }
}

pub fn parse(text: &str) -> Result<Vec<Entry>> {
    let manifest: Manifest = toml::from_str(text).context("reading the models manifest")?;
    Ok(manifest.model)
}

/// The manifest next to the converted files, else the probe's own.
pub fn load(models: &Path) -> Result<Vec<Entry>> {
    let beside = models.join("models.toml");
    let path = if beside.exists() {
        beside
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("models.toml")
    };
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probes_own_manifest_parses() {
        let entries = parse(include_str!("../models.toml")).unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.name() == "isnet_general_use_1024x1024")
        );
        for e in &entries {
            assert!(e.file.ends_with(".onnx"), "{}", e.file);
            assert!(e.input[0] > 0 && e.input[1] > 0);
            assert!(e.std.iter().all(|&s| s > 0.0));
        }
    }
}
