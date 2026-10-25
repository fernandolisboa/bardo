//! A look animated by a take: LivePortrait's pipeline for a still source
//! and a driving video, with the face found by Bardo's MediaPipe tracker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use bardo_domain::{FaceTracker, RgbFrame, VisionModels};

use crate::crop::{self, Affine, FacePoints};
use crate::image::{self, Image, PasteMask};
use crate::motion::{self, Keypoints, Motion};
use crate::runtime::{Ep, Model};
use crate::video::{self, Probe, Reader, Writer};

/// The networks of one run, by file stem.
pub struct Networks {
    pub appearance: Model,
    pub motion: Model,
    pub warping: Model,
    pub generator: Model,
    pub stitching: Model,
    pub lip: Model,
}

pub struct Choice<'a> {
    pub ep: Ep,
    pub threads: usize,
    /// The warping module's file stem: `warping_module` (Gather),
    /// `warping_module_gridsample` (GridSample), or either's `_fp16`.
    pub warping: &'a str,
    pub generator: &'a str,
}

impl Networks {
    pub fn load(dir: &Path, choice: &Choice) -> Result<Networks> {
        let load = |stem: &str| {
            Model::load(
                &dir.join(format!("{stem}.onnx")),
                choice.ep,
                choice.threads,
                None,
            )
        };
        Ok(Networks {
            appearance: load("appearance_feature_extractor")?,
            motion: load("motion_extractor")?,
            warping: load(choice.warping)?,
            generator: load(choice.generator)?,
            stitching: load("stitching")?,
            lip: load("retarget_lip")?,
        })
    }

    pub fn all(&self) -> [&Model; 6] {
        [
            &self.appearance,
            &self.motion,
            &self.warping,
            &self.generator,
            &self.stitching,
            &self.lip,
        ]
    }
}

const CROP: usize = 256;
const OUT: usize = 512;

fn run_motion(net: &mut Model, crop: &Image) -> Result<Motion> {
    let input = crop.to_chw();
    let out = net.run(&[(&[1, 3, CROP, CROP], &input)])?;
    let [pitch, yaw, roll, t, exp, scale, kp] = out.as_slice() else {
        bail!("the motion extractor gave {} outputs, not 7", out.len());
    };
    Ok(Motion {
        pitch: motion::degrees(pitch),
        yaw: motion::degrees(yaw),
        roll: motion::degrees(roll),
        t: t.as_slice().try_into().context("t")?,
        exp: exp.as_slice().try_into().context("exp")?,
        scale: scale[0],
        kp: kp.as_slice().try_into().context("kp")?,
    })
}

fn keypoints(v: &[f32]) -> Result<Keypoints> {
    v.try_into().context("63 coordinates")
}

/// Milliseconds spent per step, and how many times each ran.
#[derive(Default, serde::Serialize)]
pub struct Timings(pub BTreeMap<&'static str, (f64, usize)>);

impl Timings {
    pub fn time<T>(&mut self, step: &'static str, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let out = f();
        let e = self.0.entry(step).or_default();
        e.0 += start.elapsed().as_secs_f64() * 1000.0;
        e.1 += 1;
        out
    }

    /// Mean milliseconds a time, per step.
    pub fn means(&self) -> BTreeMap<&'static str, f64> {
        self.0
            .iter()
            .map(|(k, (ms, n))| (*k, ms / (*n).max(1) as f64))
            .collect()
    }
}

/// The face's mesh in pixels, or `None` when the tracker sees no face.
fn mesh(tracker: &mut dyn FaceTracker, img: &Image, at: Duration) -> Result<Option<Vec<[f32; 2]>>> {
    let frame = RgbFrame {
        width: img.width as u32,
        height: img.height as u32,
        rgb: img.rgb.clone(),
    };
    let seen = tracker
        .track(&frame, at, &mut || Ok(None))
        .map_err(|e| anyhow::anyhow!("face tracker: {e}"))?;
    Ok(seen.mesh.map(|m| {
        m.landmarks
            .iter()
            .map(|p| [p[0] * img.width as f32, p[1] * img.height as f32])
            .collect()
    }))
}

pub struct Inputs<'a> {
    pub models: &'a Path,
    pub vision: &'a Path,
    /// The look; `None` takes the take's first frame (self-reenactment).
    pub look: Option<&'a Path>,
    pub take: &'a Path,
    pub out: &'a Path,
    pub side_by_side: Option<&'a Path>,
    pub frames: Option<usize>,
    pub max_dim: usize,
}

#[derive(serde::Serialize)]
pub struct Report {
    pub frames: usize,
    pub fps: f64,
    pub look: (usize, usize),
    pub devices: BTreeMap<String, String>,
    /// Mean milliseconds a frame, per step.
    pub ms_per_frame: BTreeMap<&'static str, f64>,
    /// Milliseconds a frame for everything after the face pass.
    pub ms_per_frame_total: f64,
    /// Seconds a 30 s take at the take's frame rate would take, face pass
    /// and setup included.
    pub projected_30s_take_s: f64,
    pub wall_s: f64,
    /// Frames of the take where the tracker saw no face.
    pub frames_without_face: usize,
    pub source_lip_ratio: f32,
    pub lip_closed: bool,
    pub source_crop: Affine,
    /// Pixels of the look each generated pixel covers once pasted back:
    /// above 1 the face is stretched from the 512 square, and softer.
    pub paste_upscale: f32,
    pub driving_box: [f32; 4],
    /// Only when the look is the take's first frame: how close each frame
    /// comes to the take's own frame.
    pub self_psnr: Option<Psnr>,
}

#[derive(serde::Serialize)]
pub struct Psnr {
    /// Over the paste-back mask (where the face is generated), weighted by it.
    pub face_db: f64,
    pub whole_frame_db: f64,
}

/// Scales `img` to `width` × `height`, bilinear (for the side-by-side).
fn scaled(img: &Image, width: usize, height: usize) -> Image {
    let (sx, sy) = (
        width as f32 / img.width as f32,
        height as f32 / img.height as f32,
    );
    let m = Affine([[sx, 0.0, 0.5 * sx - 0.5], [0.0, sy, 0.5 * sy - 0.5]]);
    image::warp(img, &m, width, height)
}

fn side_by_side(left: &Image, right: &Image) -> Image {
    let l = scaled(
        left,
        left.width * right.height / left.height / 2 * 2,
        right.height,
    );
    let mut out = Image::new(l.width + right.width, right.height);
    for y in 0..out.height {
        let row = y * out.width * 3;
        out.rgb[row..row + l.width * 3]
            .copy_from_slice(&l.rgb[y * l.width * 3..(y + 1) * l.width * 3]);
        out.rgb[row + l.width * 3..row + out.width * 3]
            .copy_from_slice(&right.rgb[y * right.width * 3..(y + 1) * right.width * 3]);
    }
    out
}

/// Squared error sums: over the mask (weighted) and over the frame.
#[derive(Default)]
struct ErrorSums {
    face: (f64, f64),
    whole: (f64, f64),
}

impl ErrorSums {
    fn add(&mut self, got: &Image, want: &Image, mask: &PasteMask) {
        for (i, (g, w)) in got.rgb.iter().zip(&want.rgb).enumerate() {
            let e = (f64::from(*g) - f64::from(*w)).powi(2);
            let m = f64::from(mask.weight[i]);
            self.face.0 += m * e;
            self.face.1 += m;
            self.whole.0 += e;
            self.whole.1 += 1.0;
        }
    }

    fn psnr(&self) -> Psnr {
        let db = |(sum, n): (f64, f64)| {
            10.0 * (255.0f64.powi(2) / (sum / n.max(1.0)).max(1e-12)).log10()
        };
        Psnr {
            face_db: db(self.face),
            whole_frame_db: db(self.whole),
        }
    }
}

pub fn animate(inputs: &Inputs, choice: &Choice) -> Result<Report> {
    let wall = Instant::now();
    let mut t = Timings::default();
    let mut nets = t.time("load_models", || Networks::load(inputs.models, choice))?;
    let vision = bardo_vision::OnnxVision::new(
        inputs.vision,
        match choice.ep {
            Ep::Cpu => bardo_vision::Wanted::Cpu,
            Ep::DirectMl => bardo_vision::Wanted::Gpu,
        },
    );
    let mut devices: BTreeMap<String, String> = nets
        .all()
        .iter()
        .map(|m| {
            let note = m
                .fallback_reason
                .as_deref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default();
            (m.name.clone(), format!("{:?}{note}", m.ep))
        })
        .collect();

    // The look: cropped, its appearance and keypoints read once.
    let look = match inputs.look {
        Some(path) => video::read_image(path, inputs.max_dim)?,
        None => video::read_image(inputs.take, inputs.max_dim)?,
    };
    let mut tracker = vision
        .face_tracker()
        .map_err(|e| anyhow::anyhow!("face tracker: {e}"))?;
    devices.insert("face_tracker".into(), format!("{:?}", tracker.device()));
    let look_mesh = t
        .time("face_look", || {
            mesh(tracker.as_mut(), &look, Duration::ZERO)
        })?
        .context("no face in the look")?;
    let o2c = crop::source_transform(&FacePoints::from_mediapipe(&look_mesh));
    let c2o = o2c.inverse();
    let source = image::half(&image::warp(&look, &o2c, OUT, OUT));
    let input = source.to_chw();
    let feature = t.time("appearance", || {
        nets.appearance.run(&[(&[1, 3, CROP, CROP], &input)])
    })?;
    let feature = feature.into_iter().next().context("no feature volume")?;
    let src = t.time("motion", || run_motion(&mut nets.motion, &source))?;
    let x_s = src.keypoints();
    let lip_ratio = crop::lip_ratio_mediapipe(&look_mesh);
    let lip_delta = if lip_ratio >= motion::LIP_NORMALIZE_THRESHOLD {
        let input = motion::lip_input(&x_s, lip_ratio);
        let out = nets.lip.run(&[(&[1, input.len()], &input)])?;
        Some(keypoints(&out[0])?)
    } else {
        None
    };
    let template = video::read_image(&inputs.models.join("mask_template.png"), 0)?;
    let mask = PasteMask::new(&template, &c2o, look.width, look.height);

    // Pass 1: the face in every frame of the take, for the one box the
    // whole take is cropped to (upstream averages them the same way).
    let p: Probe = video::probe(inputs.take)?;
    let mut boxes = Vec::new();
    let mut missing = 0;
    let mut driving_tracker = vision
        .face_tracker()
        .map_err(|e| anyhow::anyhow!("face tracker: {e}"))?;
    let mut reader = Reader::open(inputs.take, &p, inputs.frames)?;
    let mut n = 0usize;
    while let Some(frame) = reader.next_frame()? {
        let at = Duration::from_secs_f64(n as f64 / p.fps);
        match t.time("face_take", || mesh(driving_tracker.as_mut(), &frame, at))? {
            Some(m) => boxes.push(crop::driving_box(&FacePoints::from_mediapipe(&m))),
            None => missing += 1,
        }
        n += 1;
    }
    drop(reader);
    let global = crop::mean_box(&boxes).context("no face anywhere in the take")?;
    let d_o2c = crop::box_transform(&global, OUT as f32);

    // Pass 2: every frame's motion moves the look's keypoints.
    let audio = p.has_audio.then_some(inputs.take);
    let mut writer = Writer::create(inputs.out, look.width, look.height, p.fps, audio)?;
    let mut sbs = match inputs.side_by_side {
        Some(path) => {
            let w = p.width * look.height / p.height / 2 * 2 + look.width;
            Some(Writer::create(path, w, look.height, p.fps, audio)?)
        }
        None => None,
    };
    let mut truth = match inputs.look {
        None => Some(Reader::open_scaled(
            inputs.take,
            look.width,
            look.height,
            inputs.frames,
        )?),
        Some(_) => None,
    };
    let mut errors = ErrorSums::default();
    let mut reader = Reader::open(inputs.take, &p, inputs.frames)?;
    let mut first: Option<Motion> = None;
    let mut frames = 0usize;
    let shape_f = [1usize, 32, 16, 64, 64];
    let shape_kp = [1usize, motion::KP, 3];
    while let Some(frame) = reader.next_frame()? {
        let driving = t.time("crop_take", || {
            image::half(&image::warp(&frame, &d_o2c, OUT, OUT))
        });
        let m = t.time("motion", || run_motion(&mut nets.motion, &driving))?;
        let first = first.get_or_insert_with(|| m.clone());
        let mut x_d = src.driven(first, &m);
        let input = motion::stitching_input(&x_s, &x_d);
        let delta = t.time("stitching", || {
            nets.stitching.run(&[(&[1, input.len()], &input)])
        })?;
        x_d = motion::stitch(&x_d, &delta[0]);
        if let Some(lip) = &lip_delta {
            for (k, d) in x_d.iter_mut().zip(lip) {
                *k += d;
            }
        }
        let warped = t.time("warping", || {
            nets.warping
                .run(&[(&shape_f, &feature), (&shape_kp, &x_d), (&shape_kp, &x_s)])
        })?;
        let generated = t.time("generator", || {
            nets.generator.run(&[(&[1, 256, 64, 64], &warped[0])])
        })?;
        let face = Image::from_chw(OUT, OUT, &generated[0]);
        let out = t.time("paste_back", || {
            image::paste_back(&face, &c2o, &look, &mask)
        });
        t.time("encode", || writer.write(&out))?;
        if let Some(w) = sbs.as_mut() {
            w.write(&side_by_side(&frame, &out))?;
        }
        if let Some(truth) = truth.as_mut() {
            let want = truth.next_frame()?.context("the take ended early")?;
            errors.add(&out, &want, &mask);
        }
        frames += 1;
    }
    writer.finish()?;
    if let Some(w) = sbs {
        w.finish()?;
    }

    let means = t.means();
    let per_frame: f64 = [
        "crop_take",
        "motion",
        "stitching",
        "warping",
        "generator",
        "paste_back",
        "encode",
    ]
    .iter()
    .filter_map(|k| means.get(k))
    .sum();
    let face_ms = means.get("face_take").copied().unwrap_or_default();
    let setup_ms: f64 = ["load_models", "face_look", "appearance"]
        .iter()
        .filter_map(|k| t.0.get(k).map(|v| v.0))
        .sum();
    let take_frames = 30.0 * p.fps;
    Ok(Report {
        frames,
        fps: p.fps,
        look: (look.width, look.height),
        devices,
        ms_per_frame: means,
        ms_per_frame_total: per_frame,
        projected_30s_take_s: (setup_ms + take_frames * (per_frame + face_ms)) / 1000.0,
        wall_s: wall.elapsed().as_secs_f64(),
        frames_without_face: missing,
        source_lip_ratio: lip_ratio,
        lip_closed: lip_delta.is_some(),
        source_crop: o2c,
        paste_upscale: 1.0 / o2c.0[0][0].hypot(o2c.0[0][1]),
        driving_box: global,
        self_psnr: inputs.look.is_none().then(|| errors.psnr()),
    })
}

/// Loads every model of `dir` on `ep`, runs each once on zeros and says
/// where its nodes ran.
pub fn info(dir: &Path, ep: Ep, placement: bool) -> Result<serde_json::Value> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "onnx"))
        .collect();
    files.sort();
    let profile_dir = std::env::temp_dir().join("liveportrait-probe-profile");
    std::fs::create_dir_all(&profile_dir)?;
    let mut all = serde_json::Map::new();
    for file in files {
        let profile = placement.then(|| profile_dir.join(file.file_stem().unwrap_or_default()));
        let mut model = match Model::load(&file, ep, 0, profile.as_deref()) {
            Ok(m) => m,
            Err(e) => {
                println!("{}: {e:#}", file.display());
                all.insert(
                    file.display().to_string(),
                    serde_json::json!({ "error": format!("{e:#}") }),
                );
                continue;
            }
        };
        println!("{}", model.describe());
        let shapes = model.input_shapes();
        let data: Vec<Vec<f32>> = shapes
            .iter()
            .map(|s| vec![0.0; s.iter().product()])
            .collect();
        let inputs: Vec<(&[usize], &[f32])> = shapes
            .iter()
            .map(Vec::as_slice)
            .zip(data.iter().map(Vec::as_slice))
            .collect();
        let start = Instant::now();
        let ran = model.run(&inputs);
        let first_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut entry = serde_json::json!({
            "ep": model.ep,
            "fallback": model.fallback_reason,
            "first_run_ms": first_ms,
        });
        if let Err(e) = ran {
            println!("  run failed: {e:#}");
            entry["error"] = format!("{e:#}").into();
        }
        if placement {
            let p = model.placement()?;
            println!("  nodes by provider: {:?}", p.nodes);
            if !p.on_cpu.is_empty() {
                println!("  on the CPU: {:?}", p.on_cpu);
            }
            entry["placement"] = serde_json::to_value(&p)?;
        }
        all.insert(model.name.clone(), entry);
    }
    Ok(serde_json::Value::Object(all))
}
