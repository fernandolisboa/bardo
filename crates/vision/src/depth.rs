//! Depth (ported from the #187 spike): Video Depth Anything Small in
//! windows of 32 frames, or Depth Anything V2 Small one frame at a time
//! smoothed over time where Video Depth Anything's memory is not free.
//! Both give relative inverse depth (larger is nearer); Video Depth
//! Anything aligns its windows itself (ported from
//! video_depth_anything/video_depth.py), V2 is aligned to the frame before
//! and blended.

use std::collections::VecDeque;

use bardo_domain::{DepthEstimator, DepthMap, Device, ModelInfo, RgbFrame, VisionError};

use crate::frame::sample;
use crate::runtime::Model;
use crate::state::{StateReader, StateWriter};

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// How much of each new V2 frame goes into the smoothed depth.
const V2_BLEND: f32 = 0.5;

const INFER_LEN: usize = 32;
const OVERLAP: usize = 10;
const KEYFRAMES: [usize; OVERLAP] = [0, 12, 24, 25, 26, 27, 28, 29, 30, 31];
const INTERP_LEN: usize = 8;

/// Least-squares scale and shift taking `pred` onto `target`
/// (compute_scale_and_shift_full).
fn scale_and_shift(pred: &[f32], target: &[f32]) -> (f32, f32) {
    let (mut a00, mut a01, mut b0, mut b1) = (0f64, 0f64, 0f64, 0f64);
    for (&p, &t) in pred.iter().zip(target) {
        let (p, t) = (f64::from(p), f64::from(t));
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

/// The frame stretched to the model's size, 8-bit RGB: what a window
/// keeps of each frame until it runs.
fn stretched(frame: &RgbFrame, width: usize, height: usize) -> Vec<u8> {
    let (sx, sy) = (
        frame.width as f32 / width as f32,
        frame.height as f32 / height as f32,
    );
    let mut out = Vec::with_capacity(width * height * 3);
    for v in 0..height {
        for u in 0..width {
            let px = sample(frame, (u as f32 + 0.5) * sx, (v as f32 + 0.5) * sy);
            out.extend(px.iter().map(|c| c.round().clamp(0.0, 255.0) as u8));
        }
    }
    out
}

/// Stretched RGB to the model's normalized channels-first input.
fn normalize_into(rgb: &[u8], plane: usize, out: &mut Vec<f32>) {
    let start = out.len();
    out.resize(start + plane * 3, 0.0);
    for (i, px) in rgb.chunks_exact(3).enumerate() {
        for c in 0..3 {
            out[start + c * plane + i] = (f32::from(px[c]) / 255.0 - MEAN[c]) / STD[c];
        }
    }
}

fn write_map(w: &mut StateWriter, map: &DepthMap) {
    w.u32(map.width);
    w.u32(map.height);
    w.floats(&map.data);
}

fn read_map(r: &mut StateReader<'_>, width: u32, height: u32) -> Result<DepthMap, VisionError> {
    let (w, h) = (r.u32()?, r.u32()?);
    let data = r.floats()?;
    if (w, h) != (width, height) || data.len() != (w * h) as usize {
        return Err(VisionError::State("a depth map of another size".to_owned()));
    }
    Ok(DepthMap {
        width: w,
        height: h,
        data,
    })
}

fn read_frame(r: &mut StateReader<'_>, len: usize) -> Result<Vec<u8>, VisionError> {
    let bytes = r.bytes()?;
    if bytes.len() != len {
        return Err(VisionError::State("a frame of another size".to_owned()));
    }
    Ok(bytes.to_vec())
}

/// Depth Anything V2, one frame a run, each frame aligned to the smoothed
/// one before and blended with it.
pub(crate) struct PerFrame {
    model: Model,
    info: ModelInfo,
    width: usize,
    height: usize,
    input: Vec<f32>,
    last: Option<DepthMap>,
}

impl PerFrame {
    pub(crate) fn new(model: Model, info: ModelInfo, width: usize, height: usize) -> PerFrame {
        PerFrame {
            model,
            info,
            width,
            height,
            input: Vec::with_capacity(3 * width * height),
            last: None,
        }
    }
}

impl DepthEstimator for PerFrame {
    fn model(&self) -> &ModelInfo {
        &self.info
    }

    fn device(&self) -> &Device {
        self.model.device()
    }

    fn size(&self) -> (u32, u32) {
        (self.width as u32, self.height as u32)
    }

    fn push(&mut self, frame: &RgbFrame) -> Result<Vec<DepthMap>, VisionError> {
        let plane = self.width * self.height;
        self.input.clear();
        normalize_into(
            &stretched(frame, self.width, self.height),
            plane,
            &mut self.input,
        );
        let out = self
            .model
            .run(&[1, 3, self.height, self.width], &self.input)?;
        let data = out
            .into_iter()
            .next()
            .filter(|d| d.len() == plane)
            .ok_or_else(|| {
                VisionError::Failed("the depth model's output is not its size".to_owned())
            })?;
        let raw = DepthMap {
            width: self.width as u32,
            height: self.height as u32,
            data,
        };
        let smoothed = match self.last.take() {
            None => raw,
            Some(prev) => {
                let (s, b) = scale_and_shift(&raw.data, &prev.data);
                let data = raw
                    .data
                    .iter()
                    .zip(&prev.data)
                    .map(|(&d, &p)| V2_BLEND * (d * s + b) + (1.0 - V2_BLEND) * p)
                    .collect();
                DepthMap { data, ..raw }
            }
        };
        self.last = Some(smoothed.clone());
        Ok(vec![smoothed])
    }

    fn finish(&mut self) -> Result<Vec<DepthMap>, VisionError> {
        Ok(Vec::new())
    }

    fn snapshot(&self) -> Vec<u8> {
        let mut w = StateWriter::new("depth-v2/1");
        w.bool(self.last.is_some());
        if let Some(last) = &self.last {
            write_map(&mut w, last);
        }
        w.finish()
    }

    fn restore(&mut self, snapshot: &[u8]) -> Result<(), VisionError> {
        let mut r = StateReader::new(snapshot, "depth-v2/1")?;
        let last = if r.bool()? {
            Some(read_map(&mut r, self.width as u32, self.height as u32)?)
        } else {
            None
        };
        r.end()?;
        self.last = last;
        Ok(())
    }
}

/// Video Depth Anything over a stream of frames: the first window takes
/// 32 frames, each later one 22 new frames plus ten from the window
/// before (frame 0, frame 12 and the last eight), aligned to the depth
/// already given by a scale and shift fitted on the two shared key
/// frames. The last eight frames given are blended with the next window,
/// so a frame is final once the next window has run.
pub(crate) struct Windowed {
    model: Model,
    info: ModelInfo,
    width: usize,
    height: usize,
    /// The window being filled, stretched.
    window: Vec<Vec<u8>>,
    /// The key frames the next window starts with.
    carry: Option<Vec<Vec<u8>>>,
    /// Aligned depth not yet final (the last INTERP_LEN).
    pending: VecDeque<DepthMap>,
    ref_align: Vec<Vec<f32>>,
    frames_in: u32,
    frames_out: u32,
}

impl Windowed {
    pub(crate) fn new(model: Model, info: ModelInfo, width: usize, height: usize) -> Windowed {
        Windowed {
            model,
            info,
            width,
            height,
            window: Vec::new(),
            carry: None,
            pending: VecDeque::new(),
            ref_align: Vec::new(),
            frames_in: 0,
            frames_out: 0,
        }
    }

    fn map(&self, data: Vec<f32>) -> DepthMap {
        DepthMap {
            width: self.width as u32,
            height: self.height as u32,
            data,
        }
    }

    fn run_window(&mut self) -> Result<Vec<DepthMap>, VisionError> {
        let plane = self.width * self.height;
        let mut batch = Vec::with_capacity(INFER_LEN * 3 * plane);
        for frame in &self.window {
            normalize_into(frame, plane, &mut batch);
        }
        let out = self
            .model
            .run(&[1, INFER_LEN, 3, self.height, self.width], &batch)?;
        let all = out
            .into_iter()
            .next()
            .filter(|d| d.len() == INFER_LEN * plane)
            .ok_or_else(|| {
                VisionError::Failed("the depth model's output is not its size".to_owned())
            })?;
        let depth: Vec<&[f32]> = all.chunks_exact(plane).collect();
        let fit = |d: &[f32], s: f32, b: f32| {
            d.iter().map(|v| (v * s + b).max(0.0)).collect::<Vec<f32>>()
        };
        let mut fresh = Vec::new();
        if self.ref_align.is_empty() {
            self.ref_align = vec![depth[KEYFRAMES[0]].to_vec(), depth[KEYFRAMES[1]].to_vec()];
            fresh.extend(depth.iter().map(|d| self.map(d.to_vec())));
        } else {
            let align_len = OVERLAP - INTERP_LEN;
            let cur: Vec<f32> = depth[..align_len].concat();
            let reference: Vec<f32> = self.ref_align.concat();
            let (s, b) = scale_and_shift(&cur, &reference);
            if self.pending.len() != INTERP_LEN {
                return Err(VisionError::Failed(format!(
                    "expected {INTERP_LEN} frames waiting, have {}",
                    self.pending.len()
                )));
            }
            // Blend the last eight frames given into this window's view.
            for (i, pre) in self.pending.iter_mut().enumerate() {
                let w = i as f32 / (INTERP_LEN - 1) as f32;
                let post = fit(depth[align_len + i], s, b);
                for (p, q) in pre.data.iter_mut().zip(post) {
                    *p = *p * (1.0 - w) + q * w;
                }
            }
            for d in &depth[OVERLAP..] {
                fresh.push(self.map(fit(d, s, b)));
            }
            self.ref_align.truncate(1);
            self.ref_align.push(fit(depth[KEYFRAMES[1]], s, b));
        }
        self.pending.extend(fresh);
        let mut done = Vec::new();
        while self.pending.len() > INTERP_LEN {
            done.extend(self.pending.pop_front());
        }
        let window = std::mem::take(&mut self.window);
        self.carry = Some(KEYFRAMES.iter().map(|&k| window[k].clone()).collect());
        Ok(done)
    }

    /// Keeps only the frames that were pushed: the last window is padded.
    fn count_out(&mut self, maps: Vec<DepthMap>) -> Vec<DepthMap> {
        let left = self.frames_in.saturating_sub(self.frames_out) as usize;
        let maps: Vec<DepthMap> = maps.into_iter().take(left).collect();
        self.frames_out += maps.len() as u32;
        maps
    }
}

impl DepthEstimator for Windowed {
    fn model(&self) -> &ModelInfo {
        &self.info
    }

    fn device(&self) -> &Device {
        self.model.device()
    }

    fn size(&self) -> (u32, u32) {
        (self.width as u32, self.height as u32)
    }

    fn push(&mut self, frame: &RgbFrame) -> Result<Vec<DepthMap>, VisionError> {
        if self.window.is_empty()
            && let Some(carry) = self.carry.take()
        {
            self.window = carry;
        }
        self.window.push(stretched(frame, self.width, self.height));
        self.frames_in += 1;
        if self.window.len() == INFER_LEN {
            let done = self.run_window()?;
            Ok(self.count_out(done))
        } else {
            Ok(Vec::new())
        }
    }

    fn finish(&mut self) -> Result<Vec<DepthMap>, VisionError> {
        let mut out = Vec::new();
        let carried = if self.carry.is_some() || !self.ref_align.is_empty() {
            OVERLAP
        } else {
            0
        };
        if self.window.is_empty()
            && let Some(carry) = self.carry.take()
        {
            self.window = carry;
        }
        if self.window.len() > carried
            && let Some(last) = self.window.last().cloned()
        {
            while self.window.len() < INFER_LEN {
                self.window.push(last.clone());
            }
            out = self.run_window()?;
        }
        out.extend(self.pending.drain(..));
        Ok(self.count_out(out))
    }

    fn snapshot(&self) -> Vec<u8> {
        let mut w = StateWriter::new("depth-video/1");
        w.u32(self.frames_in);
        w.u32(self.frames_out);
        w.u32(self.window.len() as u32);
        for frame in &self.window {
            w.bytes(frame);
        }
        w.bool(self.carry.is_some());
        for frame in self.carry.iter().flatten() {
            w.bytes(frame);
        }
        w.u32(self.pending.len() as u32);
        for map in &self.pending {
            write_map(&mut w, map);
        }
        w.u32(self.ref_align.len() as u32);
        for values in &self.ref_align {
            w.floats(values);
        }
        w.finish()
    }

    fn restore(&mut self, snapshot: &[u8]) -> Result<(), VisionError> {
        let len = self.width * self.height * 3;
        let plane = self.width * self.height;
        let mut r = StateReader::new(snapshot, "depth-video/1")?;
        let frames_in = r.u32()?;
        let frames_out = r.u32()?;
        let n = r.u32()? as usize;
        if n >= INFER_LEN {
            return Err(VisionError::State("a full window".to_owned()));
        }
        let window = (0..n)
            .map(|_| read_frame(&mut r, len))
            .collect::<Result<_, _>>()?;
        let carry = if r.bool()? {
            Some(
                (0..OVERLAP)
                    .map(|_| read_frame(&mut r, len))
                    .collect::<Result<_, _>>()?,
            )
        } else {
            None
        };
        let n = r.u32()? as usize;
        if n > INTERP_LEN {
            return Err(VisionError::State("too many frames waiting".to_owned()));
        }
        let pending = (0..n)
            .map(|_| read_map(&mut r, self.width as u32, self.height as u32))
            .collect::<Result<_, _>>()?;
        let n = r.u32()? as usize;
        let ref_align: Vec<Vec<f32>> = (0..n).map(|_| r.floats()).collect::<Result<_, _>>()?;
        if n > 2 || ref_align.iter().any(|v| v.len() != plane) {
            return Err(VisionError::State("alignment of another size".to_owned()));
        }
        r.end()?;
        self.frames_in = frames_in;
        self.frames_out = frames_out;
        self.window = window;
        self.carry = carry;
        self.pending = pending;
        self.ref_align = ref_align;
        Ok(())
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
        assert_eq!(scale_and_shift(&[1.0, 1.0], &[2.0, 3.0]), (1.0, 0.0));
    }

    #[test]
    fn frames_are_normalized_channels_first() {
        let mut out = Vec::new();
        normalize_into(&[255, 0, 0, 0, 255, 0], 2, &mut out);
        assert_eq!(out.len(), 6);
        assert!((out[0] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-6);
        assert!((out[2] - (0.0 - MEAN[1]) / STD[1]).abs() < 1e-6);
        assert!((out[3] - (1.0 - MEAN[1]) / STD[1]).abs() < 1e-6);
    }
}
