//! MediaPipe Face Landmarker, rebuilt around its three converted models
//! (ported from the #187 spike): BlazeFace short range finds faces, the
//! landmark model gives 478 points in a rotated square around the face
//! (tracked from frame to frame), the blendshape model turns 146 of them
//! into 52 expression scores, and a one-euro filter smooths the points
//! over time as MediaPipe's video mode does. Constants follow
//! mediapipe/tasks/cc/vision/face_detector and face_landmarker.

use std::f32::consts::PI;
use std::time::Duration;

use bardo_domain::{
    Device, FACE_LANDMARKS, FaceMesh, FaceObservation, FaceTracker, ModelInfo, Region, RgbFrame,
    VisionError,
};

use crate::frame::sample;
use crate::runtime::Model;
use crate::state::{StateReader, StateWriter};

const DETECTOR_SIZE: usize = 128;
const LANDMARK_SIZE: usize = 256;
const MIN_SCORE: f32 = 0.5;
const NMS_IOU: f32 = 0.3;
const MIN_PRESENCE: f32 = 0.5;

/// The one-euro filter MediaPipe's face landmarker smooths with.
const MIN_CUTOFF: f64 = 0.05;
const BETA: f64 = 80.0;
const DERIVATE_CUTOFF: f64 = 1.0;
/// Its first frequency, before two frames give one.
const START_FREQUENCY: f64 = 30.0;

/// face_blendshapes_graph.cc, kLandmarksSubsetIdxs.
const BLENDSHAPE_SUBSET: [usize; 146] = [
    0, 1, 4, 5, 6, 7, 8, 10, 13, 14, 17, 21, 33, 37, 39, 40, 46, 52, 53, 54, 55, 58, 61, 63, 65,
    66, 67, 70, 78, 80, 81, 82, 84, 87, 88, 91, 93, 95, 103, 105, 107, 109, 127, 132, 133, 136,
    144, 145, 146, 148, 149, 150, 152, 153, 154, 155, 157, 158, 159, 160, 161, 162, 163, 168, 172,
    173, 176, 178, 181, 185, 191, 195, 197, 234, 246, 249, 251, 263, 267, 269, 270, 276, 282, 283,
    284, 285, 288, 291, 293, 295, 296, 297, 300, 308, 310, 311, 312, 314, 317, 318, 321, 323, 324,
    332, 334, 336, 338, 356, 361, 362, 365, 373, 374, 375, 377, 378, 379, 380, 381, 382, 384, 385,
    386, 387, 388, 389, 390, 397, 398, 400, 402, 405, 409, 415, 454, 466, 468, 469, 470, 471, 472,
    473, 474, 475, 476, 477,
];

/// A rectangle in shares of the image, turned by `rotation` radians
/// around its center (MediaPipe's NormalizedRect).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rotation: f32,
}

#[derive(Clone, Debug)]
struct Detection {
    score: f32,
    /// Center x, y and size w, h in shares of the image.
    bbox: [f32; 4],
    /// Eyes, nose tip, mouth, ear tragions.
    keypoints: [[f32; 2]; 6],
}

#[derive(Clone, Copy)]
struct Anchor {
    x: f32,
    y: f32,
}

/// SsdAnchorsCalculator for BlazeFace short range: strides 8, 16, 16, 16,
/// two anchors per layer; layers with the same stride share a grid.
/// 16×16×2 + 8×8×6 = 896.
fn anchors() -> Vec<Anchor> {
    let strides = [8usize, 16, 16, 16];
    let mut out = Vec::with_capacity(896);
    let mut layer = 0;
    while layer < strides.len() {
        let stride = strides[layer];
        let mut per_cell = 0;
        while layer < strides.len() && strides[layer] == stride {
            per_cell += 2;
            layer += 1;
        }
        let cells = DETECTOR_SIZE.div_ceil(stride);
        for y in 0..cells {
            for x in 0..cells {
                for _ in 0..per_cell {
                    out.push(Anchor {
                        x: (x as f32 + 0.5) / cells as f32,
                        y: (y as f32 + 0.5) / cells as f32,
                    });
                }
            }
        }
    }
    out
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x.clamp(-100.0, 100.0)).exp())
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let region = |r: &[f32; 4]| Region {
        x: r[0],
        y: r[1],
        width: r[2],
        height: r[3],
    };
    region(a).overlap(&region(b))
}

/// NonMaxSuppressionCalculator, WEIGHTED: each face kept is the
/// score-weighted mean of the boxes overlapping the best one.
fn weighted_nms(mut dets: Vec<Detection>) -> Vec<Detection> {
    dets.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut out = Vec::new();
    while !dets.is_empty() {
        let best = dets[0].clone();
        let (cluster, rest): (Vec<_>, Vec<_>) = dets
            .into_iter()
            .partition(|d| iou(&d.bbox, &best.bbox) > NMS_IOU);
        dets = rest;
        let total: f32 = cluster.iter().map(|d| d.score).sum();
        let mut merged = best.clone();
        merged.bbox = [0.0; 4];
        merged.keypoints = [[0.0; 2]; 6];
        for d in &cluster {
            let w = d.score / total;
            for i in 0..4 {
                merged.bbox[i] += d.bbox[i] * w;
            }
            for k in 0..6 {
                merged.keypoints[k][0] += d.keypoints[k][0] * w;
                merged.keypoints[k][1] += d.keypoints[k][1] * w;
            }
        }
        out.push(merged);
    }
    out
}

fn normalize_radians(a: f32) -> f32 {
    a - 2.0 * PI * ((a + PI) / (2.0 * PI)).floor()
}

/// DetectionsToRectsCalculator's rotation: the angle that levels the line
/// from `start` to `end` (shares of the image), in pixels.
fn rotation(start: [f32; 2], end: [f32; 2], width: u32, height: u32) -> f32 {
    let dx = (end[0] - start[0]) * width as f32;
    let dy = (end[1] - start[1]) * height as f32;
    normalize_radians(0.0 - (-dy).atan2(dx))
}

/// RectTransformationCalculator with no shift: squared on its long side,
/// then scaled.
fn transform(mut r: Rect, scale: f32, width: u32, height: u32) -> Rect {
    let long = (r.w * width as f32).max(r.h * height as f32);
    r.w = long / width as f32 * scale;
    r.h = long / height as f32 * scale;
    r
}

/// Where a pixel of a `size`×`size` tensor cut from a rect (letterboxed
/// to keep its aspect) lands in the image, in pixels.
struct Crop {
    cx: f32,
    cy: f32,
    cos: f32,
    sin: f32,
    scale: f32,
    half_w: f32,
    half_h: f32,
    size: f32,
}

impl Crop {
    fn new(rect: &Rect, size: usize, width: u32, height: u32) -> Self {
        let (rw, rh) = (rect.w * width as f32, rect.h * height as f32);
        let scale = (size as f32 / rw).min(size as f32 / rh);
        Crop {
            cx: rect.x * width as f32,
            cy: rect.y * height as f32,
            cos: rect.rotation.cos(),
            sin: rect.rotation.sin(),
            scale,
            half_w: rw / 2.0,
            half_h: rh / 2.0,
            size: size as f32,
        }
    }

    /// Tensor position (in tensor pixels) to image pixels, and whether it
    /// is inside the rect (not letterbox).
    fn to_image(&self, u: f32, v: f32) -> ([f32; 2], bool) {
        let dx = (u - self.size / 2.0) / self.scale;
        let dy = (v - self.size / 2.0) / self.scale;
        let inside = dx.abs() <= self.half_w && dy.abs() <= self.half_h;
        (
            [
                self.cx + dx * self.cos - dy * self.sin,
                self.cy + dx * self.sin + dy * self.cos,
            ],
            inside,
        )
    }
}

/// Fills an NHWC tensor from `rect`, mapping 0..255 to `lo..hi`; the
/// letterbox is `lo`.
fn crop_tensor(
    frame: &RgbFrame,
    rect: &Rect,
    size: usize,
    lo: f32,
    hi: f32,
    out: &mut [f32],
) -> Crop {
    let crop = Crop::new(rect, size, frame.width, frame.height);
    let k = (hi - lo) / 255.0;
    for v in 0..size {
        for u in 0..size {
            let ([x, y], inside) = crop.to_image(u as f32 + 0.5, v as f32 + 0.5);
            let i = (v * size + u) * 3;
            if inside {
                let px = sample(frame, x, y);
                for c in 0..3 {
                    out[i + c] = lo + px[c] * k;
                }
            } else {
                out[i..i + 3].fill(lo);
            }
        }
    }
    crop
}

/// A first-order low-pass filter (MediaPipe's LowPassFilter).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct LowPass {
    raw: f64,
    stored: f64,
    started: bool,
}

impl LowPass {
    fn apply(&mut self, value: f64, alpha: f64) -> f64 {
        let out = if self.started {
            alpha * value + (1.0 - alpha) * self.stored
        } else {
            value
        };
        self.raw = value;
        self.stored = out;
        self.started = true;
        out
    }
}

/// MediaPipe's OneEuroFilter over every coordinate of the mesh, in
/// pixels, with the velocity scaled by the face's size.
#[derive(Clone, Debug, PartialEq)]
struct Smoother {
    values: Vec<LowPass>,
    rates: Vec<LowPass>,
    frequency: f64,
    last: Option<f64>,
}

impl Smoother {
    fn new() -> Smoother {
        Smoother {
            values: vec![LowPass::default(); FACE_LANDMARKS * 3],
            rates: vec![LowPass::default(); FACE_LANDMARKS * 3],
            frequency: START_FREQUENCY,
            last: None,
        }
    }

    fn alpha(&self, cutoff: f64) -> f64 {
        let te = 1.0 / self.frequency;
        let tau = 1.0 / (2.0 * std::f64::consts::PI * cutoff);
        1.0 / (1.0 + tau / te)
    }

    /// Smooths `points` (shares of a `width` × `height` image) seen at
    /// `at` seconds.
    fn apply(&mut self, points: &mut [[f32; 3]], at: f64, width: u32, height: u32) {
        if let Some(last) = self.last {
            if at <= last {
                return;
            }
            self.frequency = 1.0 / (at - last);
        }
        self.last = Some(at);
        let (w, h) = (f64::from(width), f64::from(height));
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in points.iter() {
            x0 = x0.min(f64::from(p[0]) * w);
            x1 = x1.max(f64::from(p[0]) * w);
            y0 = y0.min(f64::from(p[1]) * h);
            y1 = y1.max(f64::from(p[1]) * h);
        }
        let scale = ((x1 - x0) + (y1 - y0)) / 2.0;
        let value_scale = if scale > 1e-6 { 1.0 / scale } else { 1.0 };
        let rate_alpha = self.alpha(DERIVATE_CUTOFF);
        for (i, p) in points.iter_mut().enumerate() {
            // x and z in pixels of the width, y of the height.
            for (c, size) in [(0, w), (1, h), (2, w)] {
                let k = i * 3 + c;
                let value = f64::from(p[c]) * size;
                let filter = &mut self.values[k];
                let rate = if filter.started {
                    (value - filter.raw) * value_scale * self.frequency
                } else {
                    0.0
                };
                let smooth_rate = self.rates[k].apply(rate, rate_alpha);
                let cutoff = MIN_CUTOFF + BETA * smooth_rate.abs();
                let alpha = self.alpha(cutoff);
                p[c] = (self.values[k].apply(value, alpha) / size) as f32;
            }
        }
    }
}

/// The face pipeline with its three sessions.
pub(crate) struct OnnxFaceTracker {
    detector: Model,
    landmarks: Model,
    blendshapes: Model,
    info: ModelInfo,
    device: Device,
    anchors: Vec<Anchor>,
    track: Option<Rect>,
    smoother: Smoother,
    det_in: Vec<f32>,
    lm_in: Vec<f32>,
}

impl OnnxFaceTracker {
    pub(crate) fn new(
        detector: Model,
        landmarks: Model,
        blendshapes: Model,
        info: ModelInfo,
    ) -> OnnxFaceTracker {
        let device = detector
            .device()
            .clone()
            .slower(landmarks.device().clone())
            .slower(blendshapes.device().clone());
        OnnxFaceTracker {
            detector,
            landmarks,
            blendshapes,
            info,
            device,
            anchors: anchors(),
            track: None,
            smoother: Smoother::new(),
            det_in: vec![0.0; DETECTOR_SIZE * DETECTOR_SIZE * 3],
            lm_in: vec![0.0; LANDMARK_SIZE * LANDMARK_SIZE * 3],
        }
    }

    fn refresh_device(&mut self) {
        self.device = self
            .detector
            .device()
            .clone()
            .slower(self.landmarks.device().clone())
            .slower(self.blendshapes.device().clone());
    }

    fn detect(&mut self, frame: &RgbFrame) -> Result<Vec<Detection>, VisionError> {
        let full = Rect {
            x: 0.5,
            y: 0.5,
            w: 1.0,
            h: 1.0,
            rotation: 0.0,
        };
        self.detect_rect(frame, &full)
    }

    /// Faces BlazeFace finds in an upright rect of the frame, in shares of
    /// the whole frame.
    fn detect_rect(
        &mut self,
        frame: &RgbFrame,
        region: &Rect,
    ) -> Result<Vec<Detection>, VisionError> {
        let crop = crop_tensor(frame, region, DETECTOR_SIZE, -1.0, 1.0, &mut self.det_in);
        let out = self
            .detector
            .run(&[1, DETECTOR_SIZE, DETECTOR_SIZE, 3], &self.det_in)?;
        let (Some(regressors), Some(scores)) = (out.first(), out.get(1)) else {
            return Err(VisionError::Failed(
                "the face detector gave no output".to_owned(),
            ));
        };
        if regressors.len() < self.anchors.len() * 16 || scores.len() < self.anchors.len() {
            return Err(VisionError::Failed(
                "the face detector's output is short".to_owned(),
            ));
        }
        let n = DETECTOR_SIZE as f32;
        let (w, h) = (frame.width as f32, frame.height as f32);
        let to_frame = |x: f32, y: f32| {
            let ([px, py], _) = crop.to_image(x * n, y * n);
            [px / w, py / h]
        };
        let mut dets = Vec::new();
        for (i, a) in self.anchors.iter().enumerate() {
            let score = sigmoid(scores[i]);
            if score < MIN_SCORE {
                continue;
            }
            let r = &regressors[i * 16..i * 16 + 16];
            let [x, y] = to_frame(r[0] / n + a.x, r[1] / n + a.y);
            let mut keypoints = [[0.0; 2]; 6];
            for (k, kp) in keypoints.iter_mut().enumerate() {
                *kp = to_frame(r[4 + 2 * k] / n + a.x, r[5 + 2 * k] / n + a.y);
            }
            dets.push(Detection {
                score,
                bbox: [x, y, r[2] / crop.scale / w, r[3] / crop.scale / h],
                keypoints,
            });
        }
        Ok(weighted_nms(dets))
    }

    fn rect_from_detection(d: &Detection, width: u32, height: u32) -> Rect {
        let r = Rect {
            x: d.bbox[0],
            y: d.bbox[1],
            w: d.bbox[2],
            h: d.bbox[3],
            rotation: rotation(d.keypoints[0], d.keypoints[1], width, height),
        };
        transform(r, 1.5, width, height)
    }

    fn rect_from_landmarks(lm: &[[f32; 3]], width: u32, height: u32) -> Rect {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in lm {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        let r = Rect {
            x: (x0 + x1) / 2.0,
            y: (y0 + y1) / 2.0,
            w: x1 - x0,
            h: y1 - y0,
            rotation: rotation(
                [lm[33][0], lm[33][1]],
                [lm[263][0], lm[263][1]],
                width,
                height,
            ),
        };
        transform(r, 1.5, width, height)
    }

    fn landmarks_in(
        &mut self,
        frame: &RgbFrame,
        rect: &Rect,
    ) -> Result<Option<FaceMesh>, VisionError> {
        let crop = crop_tensor(frame, rect, LANDMARK_SIZE, 0.0, 1.0, &mut self.lm_in);
        let out = self
            .landmarks
            .run(&[1, LANDMARK_SIZE, LANDMARK_SIZE, 3], &self.lm_in)?;
        let (Some(raw), Some(presence)) = (out.first(), out.get(1).and_then(|p| p.first())) else {
            return Err(VisionError::Failed(
                "the landmark model gave no output".to_owned(),
            ));
        };
        if sigmoid(*presence) < MIN_PRESENCE {
            return Ok(None);
        }
        if raw.len() < FACE_LANDMARKS * 3 {
            return Err(VisionError::Failed(
                "the landmark model's output is short".to_owned(),
            ));
        }
        let (w, h) = (frame.width as f32, frame.height as f32);
        let landmarks: Vec<[f32; 3]> = raw[..FACE_LANDMARKS * 3]
            .chunks_exact(3)
            .map(|p| {
                let ([x, y], _) = crop.to_image(p[0], p[1]);
                [x / w, y / h, p[2] / crop.scale / w]
            })
            .collect();
        Ok(Some(FaceMesh {
            landmarks,
            blendshapes: [0.0; 52],
        }))
    }

    fn blendshapes_of(
        &mut self,
        mesh: &mut FaceMesh,
        width: u32,
        height: u32,
    ) -> Result<(), VisionError> {
        let (w, h) = (width as f32, height as f32);
        let mut points = Vec::with_capacity(146 * 2);
        for &i in &BLENDSHAPE_SUBSET {
            points.push(mesh.landmarks[i][0] * w);
            points.push(mesh.landmarks[i][1] * h);
        }
        let scores = self.blendshapes.run(&[1, 146, 2], &points)?;
        let scores = scores.first().filter(|s| s.len() >= 52).ok_or_else(|| {
            VisionError::Failed("the blendshape model's output is short".to_owned())
        })?;
        mesh.blendshapes.copy_from_slice(&scores[..52]);
        Ok(())
    }
}

fn region_of(d: &Detection) -> Region {
    Region {
        x: d.bbox[0],
        y: d.bbox[1],
        width: d.bbox[2],
        height: d.bbox[3],
    }
}

fn rect_of(region: &Region) -> Rect {
    Rect {
        x: region.x,
        y: region.y,
        w: region.width,
        h: region.height,
        rotation: 0.0,
    }
}

impl FaceTracker for OnnxFaceTracker {
    fn model(&self) -> &ModelInfo {
        &self.info
    }

    fn device(&self) -> &Device {
        &self.device
    }

    fn track(
        &mut self,
        frame: &RgbFrame,
        at: Duration,
        head: &mut dyn FnMut() -> Result<Option<Region>, VisionError>,
    ) -> Result<FaceObservation, VisionError> {
        // The detector runs on every frame, to count faces.
        let mut detections = self.detect(frame)?;
        let mut detected = detections.len();
        if detections.is_empty()
            && self.track.is_none()
            && let Some(region) = head()?
        {
            detections = self.detect_rect(frame, &rect_of(&region))?;
            detected = detected.max(detections.len());
        }
        let rect = match (self.track, detections.first()) {
            (Some(r), _) => Some(r),
            (None, Some(d)) => Some(Self::rect_from_detection(d, frame.width, frame.height)),
            (None, None) => None,
        };
        let mut mesh = match rect {
            Some(rect) => self.landmarks_in(frame, &rect)?,
            None => None,
        };
        if mesh.is_none() && self.track.is_some() {
            // Lost the track: start again from the detector.
            self.track = None;
            if detections.is_empty()
                && let Some(region) = head()?
            {
                detections = self.detect_rect(frame, &rect_of(&region))?;
                detected = detected.max(detections.len());
            }
            if let Some(d) = detections.first() {
                let rect = Self::rect_from_detection(d, frame.width, frame.height);
                mesh = self.landmarks_in(frame, &rect)?;
            }
        }
        match mesh.as_mut() {
            Some(mesh) => {
                // The next frame's crop follows the raw points, as MediaPipe
                // tracks; the points given out are smoothed.
                self.track = Some(Self::rect_from_landmarks(
                    &mesh.landmarks,
                    frame.width,
                    frame.height,
                ));
                self.blendshapes_of(mesh, frame.width, frame.height)?;
                self.smoother.apply(
                    &mut mesh.landmarks,
                    at.as_secs_f64(),
                    frame.width,
                    frame.height,
                );
            }
            None => {
                self.track = None;
                self.smoother = Smoother::new();
            }
        }
        self.refresh_device();
        Ok(FaceObservation {
            mesh,
            detected: Some(detected as u32),
        })
    }

    fn detect_in(&mut self, frame: &RgbFrame, region: &Region) -> Result<Vec<Region>, VisionError> {
        let found = self.detect_rect(frame, &rect_of(region))?;
        self.refresh_device();
        Ok(found.iter().map(region_of).collect())
    }

    fn snapshot(&self) -> Vec<u8> {
        let mut w = StateWriter::new("face/1");
        w.bool(self.track.is_some());
        if let Some(r) = self.track {
            for v in [r.x, r.y, r.w, r.h, r.rotation] {
                w.f32(v);
            }
        }
        let s = &self.smoother;
        w.f64(s.frequency);
        w.bool(s.last.is_some());
        w.f64(s.last.unwrap_or(0.0));
        for filter in s.values.iter().chain(&s.rates) {
            w.f64(filter.raw);
            w.f64(filter.stored);
            w.bool(filter.started);
        }
        w.finish()
    }

    fn restore(&mut self, snapshot: &[u8]) -> Result<(), VisionError> {
        let mut r = StateReader::new(snapshot, "face/1")?;
        let track = if r.bool()? {
            Some(Rect {
                x: r.f32()?,
                y: r.f32()?,
                w: r.f32()?,
                h: r.f32()?,
                rotation: r.f32()?,
            })
        } else {
            None
        };
        let mut smoother = Smoother::new();
        smoother.frequency = r.f64()?;
        let has_last = r.bool()?;
        let last = r.f64()?;
        smoother.last = has_last.then_some(last);
        for filter in smoother.values.iter_mut().chain(smoother.rates.iter_mut()) {
            filter.raw = r.f64()?;
            filter.stored = r.f64()?;
            filter.started = r.bool()?;
        }
        r.end()?;
        self.track = track;
        self.smoother = smoother;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blazeface_short_range_has_896_anchors() {
        let a = anchors();
        assert_eq!(a.len(), 896);
        assert_eq!((a[0].x, a[0].y), (0.5 / 16.0, 0.5 / 16.0));
        assert_eq!(a[1].x, a[0].x);
        assert_eq!((a[512].x, a[512].y), (0.5 / 8.0, 0.5 / 8.0));
        assert_eq!(a[517].x, a[512].x);
        assert_eq!(a[518].x, 1.5 / 8.0);
    }

    #[test]
    fn rotation_is_zero_for_level_eyes_and_follows_a_tilt() {
        assert!(rotation([0.4, 0.5], [0.6, 0.5], 100, 100).abs() < 1e-6);
        let r = rotation([0.4, 0.5], [0.6, 0.7], 100, 100);
        assert!((r - PI / 4.0).abs() < 1e-5, "{r}");
    }

    #[test]
    fn rects_are_squared_on_the_long_side_in_pixels() {
        let r = Rect {
            x: 0.5,
            y: 0.5,
            w: 0.1,
            h: 0.1,
            rotation: 0.0,
        };
        let t = transform(r, 1.5, 1920, 1080);
        assert!((t.w * 1920.0 - 288.0).abs() < 1e-3);
        assert!((t.h * 1080.0 - 288.0).abs() < 1e-3);
    }

    #[test]
    fn crop_maps_the_center_letterboxes_and_turns() {
        let full = Rect {
            x: 0.5,
            y: 0.5,
            w: 1.0,
            h: 1.0,
            rotation: 0.0,
        };
        let crop = Crop::new(&full, 128, 1920, 1080);
        let ([x, y], inside) = crop.to_image(64.0, 64.0);
        assert!((x - 960.0).abs() < 1e-3 && (y - 540.0).abs() < 1e-3 && inside);
        assert!(!crop.to_image(64.0, 27.0).1);
        assert!(crop.to_image(64.0, 29.0).1);
        let turned = Rect {
            rotation: PI / 2.0,
            w: 0.5,
            h: 0.5,
            ..full
        };
        let crop = Crop::new(&turned, 100, 200, 200);
        let ([x0, y0], _) = crop.to_image(50.0, 50.0);
        let ([x1, y1], _) = crop.to_image(60.0, 50.0);
        assert!((x1 - x0).abs() < 1e-3 && y1 > y0);
    }

    #[test]
    fn weighted_nms_merges_overlaps_and_keeps_apart_faces() {
        let d = |score, x| Detection {
            score,
            bbox: [x, 0.5, 0.2, 0.2],
            keypoints: [[x, 0.5]; 6],
        };
        let out = weighted_nms(vec![d(0.9, 0.30), d(0.6, 0.32), d(0.8, 0.8)]);
        assert_eq!(out.len(), 2);
        assert!((out[0].bbox[0] - (0.30 * 0.9 + 0.32 * 0.6) / 1.5).abs() < 1e-6);
        assert_eq!(out[1].bbox[0], 0.8);
    }

    #[test]
    fn blendshape_subset_is_sorted_and_in_range() {
        assert!(BLENDSHAPE_SUBSET.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(*BLENDSHAPE_SUBSET.last().unwrap(), 477);
    }

    fn face_at(x: f32) -> Vec<[f32; 3]> {
        (0..FACE_LANDMARKS)
            .map(|i| {
                [
                    x + (i % 10) as f32 * 0.01,
                    0.4 + (i / 50) as f32 * 0.02,
                    0.0,
                ]
            })
            .collect()
    }

    #[test]
    fn the_one_euro_filter_holds_a_still_face_and_follows_a_move() {
        let mut s = Smoother::new();
        let mut first = face_at(0.4);
        s.apply(&mut first, 0.0, 1920, 1080);
        assert_eq!(first, face_at(0.4), "the first frame passes as it is");
        // A pixel of jitter is mostly held back.
        let mut jitter = face_at(0.4 + 1.0 / 1920.0);
        s.apply(&mut jitter, 1.0 / 30.0, 1920, 1080);
        let moved = (jitter[0][0] - 0.4) * 1920.0;
        assert!(moved < 0.5, "{moved} px");
        // A fast move of 100 px gets most of the way at once.
        let mut far = face_at(0.4 + 100.0 / 1920.0);
        s.apply(&mut far, 2.0 / 30.0, 1920, 1080);
        let moved = (far[0][0] - 0.4) * 1920.0;
        assert!(moved > 80.0, "{moved} px");
        // A frame from the past changes nothing.
        let mut late = face_at(0.9);
        s.apply(&mut late, 0.0, 1920, 1080);
        assert_eq!(late, face_at(0.9));
    }
}
