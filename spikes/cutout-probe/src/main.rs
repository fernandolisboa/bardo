//! cutout-probe: cuts a product out of its photo with salient-object
//! models (issue #277), from Rust through ONNX Runtime, and measures them.
//!
//!   cutout-probe info --models DIR [--ep cpu|directml] [--placement]
//!       [--out OUT.json]
//!   cutout-probe cut --models DIR --photos DIR_OR_FILE --out DIR
//!       [--model NAME[,NAME…]] [--runs 3] [--max-dim 2048]
//!       [--backdrop RRGGBB] [--report OUT.json]
//!
//! `--models` holds tools/convert.py's output. `cut` writes, for every
//! photo and model, the cutout (`PHOTO.MODEL.png`, the photo's own pixels
//! with an alpha) and the cutout over a flat backdrop
//! (`PHOTO.MODEL.jpg`), and reports the time of each step, the memory and
//! what the mask says about itself (src/stats.rs). Memory is the
//! process's: for a model's own number, run it alone with `--model`.
//! Common options: --ep cpu|directml (default cpu), --threads N (default:
//! ONNX Runtime's choice), --no-arena (the CPU's memory arena off, to
//! measure what it costs in memory and time).

mod cutout;
mod memory;
mod models;
mod runtime;
mod stats;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use image::{DynamicImage, ImageDecoder, ImageReader, RgbImage};
use serde::Serialize;

use crate::runtime::{Ep, Model, Placement};
use crate::stats::MaskStats;

struct Args {
    command: String,
    opts: HashMap<String, String>,
    flags: Vec<String>,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut it = std::env::args().skip(1);
        let command = it
            .next()
            .context("usage: cutout-probe <info|cut> [options]")?;
        let (mut opts, mut flags) = (HashMap::new(), Vec::new());
        let rest: Vec<String> = it.collect();
        let mut i = 0;
        while i < rest.len() {
            let key = rest[i]
                .strip_prefix("--")
                .with_context(|| format!("unexpected {}", rest[i]))?;
            match rest.get(i + 1) {
                Some(v) if !v.starts_with("--") => {
                    opts.insert(key.to_owned(), v.clone());
                    i += 2;
                }
                _ => {
                    flags.push(key.to_owned());
                    i += 1;
                }
            }
        }
        Ok(Self {
            command,
            opts,
            flags,
        })
    }

    fn get(&self, k: &str) -> Option<&str> {
        self.opts.get(k).map(String::as_str)
    }

    fn need(&self, k: &str) -> Result<&str> {
        self.get(k).with_context(|| format!("--{k} is required"))
    }

    fn flag(&self, k: &str) -> bool {
        self.flags.iter().any(|f| f == k)
    }

    fn number(&self, k: &str) -> Result<Option<usize>> {
        self.get(k)
            .map(|v| v.parse().with_context(|| format!("--{k} {v}")))
            .transpose()
    }
}

fn ms(since: Instant) -> f64 {
    (since.elapsed().as_secs_f64() * 1e4).round() / 10.0
}

fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => 0.0,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

/// The manifest's entries, or those named (by file name without `.onnx`).
fn chosen(models: &Path, names: Option<&str>) -> Result<Vec<models::Entry>> {
    let all = models::load(models)?;
    let Some(names) = names else { return Ok(all) };
    names
        .split(',')
        .map(|name| {
            all.iter()
                .find(|e| e.name() == name)
                .cloned()
                .with_context(|| format!("no model {name} in the manifest"))
        })
        .collect()
}

/// Where and how a model runs.
#[derive(Clone, Copy)]
struct On {
    ep: Ep,
    threads: usize,
    arena: bool,
}

/// Loads a model and checks that the file takes what the manifest says.
fn load(models: &Path, entry: &models::Entry, on: On, profile: Option<&Path>) -> Result<Model> {
    let model = Model::load(
        &models.join(&entry.file),
        on.ep,
        on.threads,
        on.arena,
        profile,
    )?;
    let [h, w] = entry.input;
    let shapes = model.input_shapes();
    if shapes != [vec![1, 3, h, w]] {
        bail!(
            "{} takes {shapes:?}, the manifest says 1 × 3 × {h} × {w}",
            entry.file
        );
    }
    Ok(model)
}

#[derive(Serialize)]
struct Info {
    name: String,
    bytes: u64,
    input: [usize; 2],
    ep: Ep,
    fallback_reason: Option<String>,
    load_ms: f64,
    first_run_ms: f64,
    placement: Option<Placement>,
}

/// Loads every model, runs it once on a flat input and says where it ran.
fn info(models: &Path, names: Option<&str>, on: On, placement: bool) -> Result<Vec<Info>> {
    let mut all = Vec::new();
    for entry in chosen(models, names)? {
        let path = models.join(&entry.file);
        let profile = std::env::temp_dir().join(format!("cutout-probe-{}", entry.name()));
        let t = Instant::now();
        let mut model = load(models, &entry, on, placement.then_some(profile.as_path()))?;
        let load_ms = ms(t);
        let [h, w] = entry.input;
        let t = Instant::now();
        model.run(&[(&[1, 3, h, w], &vec![0.0f32; 3 * h * w])])?;
        let first_run_ms = ms(t);
        println!("{}", model.describe());
        let placement = if placement {
            let p = model.placement()?;
            println!("  nodes {:?}, on the CPU {:?}", p.nodes, p.on_cpu);
            Some(p)
        } else {
            None
        };
        all.push(Info {
            name: entry.name().to_owned(),
            bytes: std::fs::metadata(&path)?.len(),
            input: entry.input,
            ep: model.ep,
            fallback_reason: model.fallback_reason.clone(),
            load_ms,
            first_run_ms,
            placement,
        });
    }
    Ok(all)
}

/// A photo upright (phones store the turn beside the pixels), without any
/// transparency it has, no larger than `max_dim`.
fn read_photo(path: &Path, max_dim: u32) -> Result<RgbImage> {
    let mut decoder = ImageReader::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .with_guessed_format()?
        .into_decoder()
        .with_context(|| format!("reading {}", path.display()))?;
    let orientation = decoder.orientation()?;
    let mut photo = DynamicImage::from_decoder(decoder)
        .with_context(|| format!("decoding {}", path.display()))?;
    photo.apply_orientation(orientation);
    Ok(cutout::limited(photo.to_rgb8(), max_dim))
}

fn photos(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_owned()]);
    }
    let mut all: Vec<PathBuf> = std::fs::read_dir(path)
        .with_context(|| format!("listing {}", path.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            matches!(
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("jpg" | "jpeg" | "png")
            )
        })
        .collect();
    all.sort();
    if all.is_empty() {
        bail!("no .jpg or .png in {}", path.display());
    }
    Ok(all)
}

fn backdrop(hex: &str) -> Result<[u8; 3]> {
    let value = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6);
    let v = value.with_context(|| format!("--backdrop {hex}: six hex digits"))?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

#[derive(Serialize)]
struct PhotoReport {
    photo: String,
    width: u32,
    height: u32,
    /// Resizing and normalizing the photo for the model.
    prepare_ms: f64,
    /// Every run of the model; the first of a session is the slowest.
    run_ms: Vec<f64>,
    run_ms_median: f64,
    /// The alpha at the photo's size and the cutout, without writing it.
    finish_ms: f64,
    stats: MaskStats,
}

#[derive(Serialize)]
struct ModelReport {
    name: String,
    bytes: u64,
    input: [usize; 2],
    ep: Ep,
    fallback_reason: Option<String>,
    load_ms: f64,
    /// The process's resident memory before the model loaded and its peak
    /// until the model's last photo.
    memory_before_mb: f64,
    memory_peak_mb: f64,
    photos: Vec<PhotoReport>,
}

#[derive(Serialize)]
struct Report {
    threads: usize,
    arena: bool,
    max_dim: u32,
    models: Vec<ModelReport>,
}

struct Cut<'a> {
    models: &'a Path,
    names: Option<&'a str>,
    photos: &'a Path,
    out: &'a Path,
    on: On,
    runs: usize,
    max_dim: u32,
    backdrop: [u8; 3],
}

fn cut(c: &Cut) -> Result<Report> {
    std::fs::create_dir_all(c.out)?;
    let files = photos(c.photos)?;
    let mut report = Report {
        threads: c.on.threads,
        arena: c.on.arena,
        max_dim: c.max_dim,
        models: Vec::new(),
    };
    for entry in chosen(c.models, c.names)? {
        let path = c.models.join(&entry.file);
        let memory_before_mb = memory::Sampler::start().finish();
        let sampler = memory::Sampler::start();
        let t = Instant::now();
        let mut model = load(c.models, &entry, c.on, None)?;
        let load_ms = ms(t);
        println!("{}", model.describe());
        let [h, w] = entry.input;
        let mut rows = Vec::new();
        for file in &files {
            let photo = read_photo(file, c.max_dim)?;
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("photo");

            let t = Instant::now();
            let input = cutout::prepare(&photo, &entry);
            let prepare_ms = ms(t);

            let mut run_ms = Vec::new();
            let mut raw = Vec::new();
            for _ in 0..c.runs.max(1) {
                let t = Instant::now();
                let mut outputs = model.run(&[(&[1, 3, h, w], &input)])?;
                run_ms.push(ms(t));
                raw = outputs.swap_remove(0);
            }
            if raw.len() != h * w {
                bail!(
                    "{}: {} values out, expected {h} × {w}",
                    entry.file,
                    raw.len()
                );
            }

            let t = Instant::now();
            let (alpha, range) = cutout::alpha(&raw, entry.range);
            let alpha8 = cutout::to_photo_size(&alpha, entry.input, photo.width(), photo.height());
            let cutout = cutout::cutout(&photo, &alpha8);
            let finish_ms = ms(t);

            let stats = stats::mask_stats(&alpha, w, h, range);
            cutout.save(c.out.join(format!("{stem}.{}.png", entry.name())))?;
            cutout::over(&cutout, c.backdrop)
                .save(c.out.join(format!("{stem}.{}.jpg", entry.name())))?;
            println!(
                "  {stem}: {:.0} ms, area {:.2}, unsure/area {:.2}, border {:.2}, pieces {}, raw max {:.2}",
                median(&run_ms),
                stats.area,
                stats.unsure_to_area,
                stats.border,
                stats.pieces,
                stats.raw_max
            );
            rows.push(PhotoReport {
                photo: stem.to_owned(),
                width: photo.width(),
                height: photo.height(),
                prepare_ms,
                run_ms_median: median(&run_ms),
                run_ms,
                finish_ms,
                stats,
            });
        }
        report.models.push(ModelReport {
            name: entry.name().to_owned(),
            bytes: std::fs::metadata(&path)?.len(),
            input: entry.input,
            ep: model.ep,
            fallback_reason: model.fallback_reason.clone(),
            load_ms,
            memory_before_mb,
            memory_peak_mb: sampler.finish(),
            photos: rows,
        });
    }
    Ok(report)
}

fn main() -> Result<()> {
    let args = Args::parse()?;
    let ep = Ep::parse(args.get("ep").unwrap_or("cpu"))?;
    let on = On {
        ep,
        threads: args.number("threads")?.unwrap_or(0),
        arena: !args.flag("no-arena"),
    };
    let models = Path::new(args.need("models")?);
    match args.command.as_str() {
        "info" => {
            let all = info(models, args.get("model"), on, args.flag("placement"))?;
            if let Some(out) = args.get("out") {
                std::fs::write(out, serde_json::to_string_pretty(&all)?)?;
            }
        }
        "cut" => {
            let report = cut(&Cut {
                models,
                names: args.get("model"),
                photos: Path::new(args.need("photos")?),
                out: Path::new(args.need("out")?),
                on,
                runs: args.number("runs")?.unwrap_or(3),
                max_dim: args.number("max-dim")?.unwrap_or(2048) as u32,
                backdrop: backdrop(args.get("backdrop").unwrap_or("2a7f8f"))?,
            })?;
            if let Some(out) = args.get("report") {
                std::fs::write(out, serde_json::to_string_pretty(&report)?)?;
            }
        }
        other => bail!("unknown command {other} (info or cut)"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_of_odd_and_even_counts() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn backdrop_reads_six_hex_digits_only() {
        assert_eq!(backdrop("2a7f8f").unwrap(), [0x2a, 0x7f, 0x8f]);
        assert!(backdrop("fff").is_err());
        assert!(backdrop("zzzzzz").is_err());
    }
}
