//! Depth Anything V2 Small (one frame at a time) and Video Depth Anything
//! Small (windows of 32 frames), both exported at a fixed input size. Both
//! give relative inverse depth: larger is nearer, with an unknown scale and
//! shift per frame (V2) or per window (Video Depth Anything, which aligns
//! its windows itself; ported from video_depth_anything/video_depth.py).

use std::collections::VecDeque;
use std::path::Path;

use anyhow::{Result, ensure};

use crate::face::ms;
use crate::runtime::{Ep, Model, load};
use crate::video::Frame;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

#[derive(Clone)]
pub struct DepthMap {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl DepthMap {
    /// Value at a normalized position, nearest.
    pub fn at(&self, x: f32, y: f32) -> f32 {
        let xi = ((x * self.width as f32) as usize).min(self.width - 1);
        let yi = ((y * self.height as f32) as usize).min(self.height - 1);
        self.data[yi * self.width + xi]
    }

    /// Mapped to 0..1 between its 2nd and 98th percentiles, as a renderer
    /// would before coloring it.
    pub fn normalized(&self) -> Vec<f32> {
        let mut sorted = self.data.clone();
        sorted.sort_by(f32::total_cmp);
        let lo = sorted[sorted.len() * 2 / 100];
        let hi = sorted[sorted.len() * 98 / 100];
        let span = (hi - lo).max(1e-6);
        self.data
            .iter()
            .map(|v| ((v - lo) / span).clamp(0.0, 1.0))
            .collect()
    }
}

/// Least-squares scale and shift taking `pred` onto `target`
/// (compute_scale_and_shift_full).
pub fn scale_and_shift(pred: &[f32], target: &[f32]) -> (f32, f32) {
    let (mut a00, mut a01, mut b0, mut b1) = (0f64, 0f64, 0f64, 0f64);
    for (&p, &t) in pred.iter().zip(target) {
        let (p, t) = (p as f64, t as f64);
        a00 += p * p;
        a01 += p;
        b0 += p * t;
        b1 += t;
    }
    let a11 = pred.len() as f64;
    let det = a00 * a11 - a01 * a01;
    if det == 0.0 {
        return (1.0, 0.0);
    }
    (
        ((a11 * b0 - a01 * b1) / det) as f32,
        ((-a01 * b0 + a00 * b1) / det) as f32,
    )
}

fn fill_chw(frame: &Frame, width: usize, height: usize, out: &mut [f32]) {
    let (sx, sy) = (
        frame.width as f32 / width as f32,
        frame.height as f32 / height as f32,
    );
    let plane = width * height;
    for v in 0..height {
        for u in 0..width {
            let px = frame.sample((u as f32 + 0.5) * sx, (v as f32 + 0.5) * sy);
            for c in 0..3 {
                out[c * plane + v * width + u] = (px[c] / 255.0 - MEAN[c]) / STD[c];
            }
        }
    }
}

#[derive(Default, Debug, serde::Serialize)]
pub struct DepthTimes {
    pub preprocess_ms: f64,
    pub run_ms: f64,
    pub runs: usize,
    pub align_ms: f64,
}

/// Depth Anything V2, one frame per run; optionally smoothed over time by
/// aligning each frame to the last smoothed one and blending.
pub struct PerFrame {
    model: Model,
    pub width: usize,
    pub height: usize,
    input: Vec<f32>,
    pub times: DepthTimes,
    smooth: Option<(f32, Option<DepthMap>)>,
}

impl PerFrame {
    pub fn load(
        path: &Path,
        width: usize,
        height: usize,
        ep: Ep,
        threads: usize,
        ema: Option<f32>,
    ) -> Result<Self> {
        Ok(Self {
            model: load(path, ep, threads)?,
            width,
            height,
            input: vec![0.0; 3 * width * height],
            times: DepthTimes::default(),
            smooth: ema.map(|a| (a, None)),
        })
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn infer(&mut self, frame: &Frame) -> Result<DepthMap> {
        let t = std::time::Instant::now();
        fill_chw(frame, self.width, self.height, &mut self.input);
        self.times.preprocess_ms += ms(t);
        let t = std::time::Instant::now();
        let out = self
            .model
            .run(&[1, 3, self.height, self.width], &self.input)?;
        self.times.run_ms += ms(t);
        self.times.runs += 1;
        let raw = DepthMap {
            width: self.width,
            height: self.height,
            data: out[0].1.clone(),
        };
        let Some((alpha, last)) = self.smooth.as_mut() else {
            return Ok(raw);
        };
        let t = std::time::Instant::now();
        let smoothed = match last.take() {
            None => raw,
            Some(prev) => {
                let (s, b) = scale_and_shift(&raw.data, &prev.data);
                let data = raw
                    .data
                    .iter()
                    .zip(&prev.data)
                    .map(|(&d, &p)| *alpha * (d * s + b) + (1.0 - *alpha) * p)
                    .collect();
                DepthMap { data, ..raw }
            }
        };
        *last = Some(smoothed.clone());
        self.times.align_ms += ms(t);
        Ok(smoothed)
    }
}

const INFER_LEN: usize = 32;
const OVERLAP: usize = 10;
const KEYFRAMES: [usize; OVERLAP] = [0, 12, 24, 25, 26, 27, 28, 29, 30, 31];
const INTERP_LEN: usize = 8;

/// Video Depth Anything over a stream of frames: the first window takes
/// 32 frames, each later one 22 new frames plus ten from the window before
/// (frame 0, frame 12, and the last eight), and is aligned to the depth
/// already given by a scale and shift fitted on the two shared key frames.
/// The last eight frames given are blended with the next window, so a
/// frame is final once eight more have come.
pub struct Windowed {
    model: Model,
    pub width: usize,
    pub height: usize,
    plane: usize,
    pub times: DepthTimes,
    /// Inputs of the window being filled and of the one before.
    window: Vec<Vec<f32>>,
    prev_window: Option<Vec<Vec<f32>>>,
    /// Aligned depth not yet final (the last INTERP_LEN), plus its index.
    pending: VecDeque<DepthMap>,
    next_index: usize,
    ref_align: Vec<Vec<f32>>,
    frames_in: usize,
}

impl Windowed {
    pub fn load(path: &Path, width: usize, height: usize, ep: Ep, threads: usize) -> Result<Self> {
        Ok(Self {
            model: load(path, ep, threads)?,
            width,
            height,
            plane: 3 * width * height,
            times: DepthTimes::default(),
            window: Vec::new(),
            prev_window: None,
            pending: VecDeque::new(),
            next_index: 0,
            ref_align: Vec::new(),
            frames_in: 0,
        })
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    fn window_len_needed(&self) -> usize {
        INFER_LEN
    }

    /// Adds a frame; returns the depth of every frame that became final, in
    /// order, with its index.
    pub fn push(&mut self, frame: &Frame) -> Result<Vec<(usize, DepthMap)>> {
        let t = std::time::Instant::now();
        if self.window.is_empty()
            && let Some(prev) = &self.prev_window
        {
            for &k in &KEYFRAMES {
                self.window.push(prev[k].clone());
            }
        }
        let mut input = vec![0.0; self.plane];
        fill_chw(frame, self.width, self.height, &mut input);
        self.window.push(input);
        self.frames_in += 1;
        self.times.preprocess_ms += ms(t);
        if self.window.len() == self.window_len_needed() {
            self.run_window()
        } else {
            Ok(Vec::new())
        }
    }

    /// Pads the last window with copies of the last frame and returns the
    /// rest, cut to the number of frames given.
    pub fn finish(&mut self) -> Result<Vec<(usize, DepthMap)>> {
        let mut out = Vec::new();
        let has_new = self.window.len()
            > if self.prev_window.is_some() {
                OVERLAP
            } else {
                0
            };
        if has_new {
            let last = self.window.last().cloned().unwrap_or_default();
            while self.window.len() < INFER_LEN {
                self.window.push(last.clone());
            }
            out = self.run_window()?;
        }
        while let Some(d) = self.pending.pop_front() {
            out.push((self.next_index, d));
            self.next_index += 1;
        }
        out.retain(|(i, _)| *i < self.frames_in);
        Ok(out)
    }

    fn run_window(&mut self) -> Result<Vec<(usize, DepthMap)>> {
        ensure!(self.window.len() == INFER_LEN);
        let mut batch = Vec::with_capacity(INFER_LEN * self.plane);
        for f in &self.window {
            batch.extend_from_slice(f);
        }
        let t = std::time::Instant::now();
        let out = self
            .model
            .run(&[1, INFER_LEN, 3, self.height, self.width], &batch)?;
        self.times.run_ms += ms(t);
        self.times.runs += 1;
        let t = std::time::Instant::now();
        let n = self.width * self.height;
        let depth: Vec<&[f32]> = out[0].1.chunks_exact(n).collect();
        let map = |d: Vec<f32>| DepthMap {
            width: self.width,
            height: self.height,
            data: d,
        };
        let fit = |d: &[f32], s: f32, b: f32| {
            d.iter().map(|v| (v * s + b).max(0.0)).collect::<Vec<f32>>()
        };
        let mut fresh = Vec::new();
        if self.ref_align.is_empty() {
            self.ref_align = vec![depth[KEYFRAMES[0]].to_vec(), depth[KEYFRAMES[1]].to_vec()];
            fresh.extend(depth.iter().map(|d| map(d.to_vec())));
        } else {
            let align_len = OVERLAP - INTERP_LEN;
            let cur: Vec<f32> = depth[..align_len].concat();
            let reference: Vec<f32> = self.ref_align.concat();
            let (s, b) = scale_and_shift(&cur, &reference);
            // Blend the last eight given frames into this window's view of them.
            let k = self.pending.len();
            ensure!(
                k == INTERP_LEN,
                "expected {INTERP_LEN} pending frames, have {k}"
            );
            for (i, pre) in self.pending.iter_mut().enumerate() {
                let w = i as f32 / (INTERP_LEN - 1) as f32;
                let post = fit(depth[align_len + i], s, b);
                for (p, q) in pre.data.iter_mut().zip(post) {
                    *p = *p * (1.0 - w) + q * w;
                }
            }
            fresh.extend(depth[OVERLAP..].iter().map(|d| map(fit(d, s, b))));
            self.ref_align.truncate(1);
            self.ref_align.push(fit(depth[KEYFRAMES[1]], s, b));
        }
        self.pending.extend(fresh);
        let mut done = Vec::new();
        while self.pending.len() > INTERP_LEN {
            let d = self.pending.pop_front().expect("pending");
            done.push((self.next_index, d));
            self.next_index += 1;
        }
        self.prev_window = Some(std::mem::take(&mut self.window));
        self.times.align_ms += ms(t);
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_and_shift_recovers_an_affine_map() {
        let pred = [1.0, 2.0, 3.0, 4.0];
        let target: Vec<f32> = pred.iter().map(|p| p * 2.5 - 1.0).collect();
        let (s, b) = scale_and_shift(&pred, &target);
        assert!((s - 2.5).abs() < 1e-5 && (b + 1.0).abs() < 1e-5);
    }

    #[test]
    fn normalized_spans_zero_to_one() {
        let d = DepthMap {
            width: 100,
            height: 1,
            data: (0..100).map(|i| i as f32).collect(),
        };
        let n = d.normalized();
        assert_eq!(n[0], 0.0);
        assert_eq!(n[99], 1.0);
    }
}
