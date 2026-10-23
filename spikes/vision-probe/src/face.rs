//! MediaPipe Face Landmarker, rebuilt around the three converted models:
//! BlazeFace short range finds faces, the landmark model gives 478 points in
//! a rotated square around the face (tracked from frame to frame), and the
//! blendshape model turns 146 of them into 52 expression scores. Constants
//! follow mediapipe/tasks/cc/vision/face_detector and face_landmarker.

use std::f32::consts::PI;
use std::path::Path;

use anyhow::Result;

use crate::runtime::{Ep, Model, load};
use crate::video::Frame;

const DETECTOR_SIZE: usize = 128;
const LANDMARK_SIZE: usize = 256;
const MIN_SCORE: f32 = 0.5;
const NMS_IOU: f32 = 0.3;
const MIN_PRESENCE: f32 = 0.5;

pub const BLENDSHAPE_NAMES: [&str; 52] = [
    "_neutral",
    "browDownLeft",
    "browDownRight",
    "browInnerUp",
    "browOuterUpLeft",
    "browOuterUpRight",
    "cheekPuff",
    "cheekSquintLeft",
    "cheekSquintRight",
    "eyeBlinkLeft",
    "eyeBlinkRight",
    "eyeLookDownLeft",
    "eyeLookDownRight",
    "eyeLookInLeft",
    "eyeLookInRight",
    "eyeLookOutLeft",
    "eyeLookOutRight",
    "eyeLookUpLeft",
    "eyeLookUpRight",
    "eyeSquintLeft",
    "eyeSquintRight",
    "eyeWideLeft",
    "eyeWideRight",
    "jawForward",
    "jawLeft",
    "jawOpen",
    "jawRight",
    "mouthClose",
    "mouthDimpleLeft",
    "mouthDimpleRight",
    "mouthFrownLeft",
    "mouthFrownRight",
    "mouthFunnel",
    "mouthLeft",
    "mouthLowerDownLeft",
    "mouthLowerDownRight",
    "mouthPressLeft",
    "mouthPressRight",
    "mouthPucker",
    "mouthRight",
    "mouthRollLower",
    "mouthRollUpper",
    "mouthShrugLower",
    "mouthShrugUpper",
    "mouthSmileLeft",
    "mouthSmileRight",
    "mouthStretchLeft",
    "mouthStretchRight",
    "mouthUpperUpLeft",
    "mouthUpperUpRight",
    "noseSneerLeft",
    "noseSneerRight",
];

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

/// A rectangle in normalized image coordinates, turned by `rotation`
/// radians around its center (MediaPipe's NormalizedRect).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rotation: f32,
}

#[derive(Clone, Debug)]
pub struct Detection {
    pub score: f32,
    /// x, y, w, h normalized to the image; x and y are the center.
    pub bbox: [f32; 4],
    /// Six keypoints: eyes, nose tip, mouth, ear tragions.
    pub keypoints: [[f32; 2]; 6],
}

#[derive(Clone, Copy)]
struct Anchor {
    x: f32,
    y: f32,
}

/// SsdAnchorsCalculator for BlazeFace short range: strides 8, 16, 16, 16,
/// two anchors per layer, fixed size; layers with the same stride share a
/// grid. 16×16×2 + 8×8×6 = 896.
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
    let (ax0, ay0, ax1, ay1) = (
        a[0] - a[2] / 2.0,
        a[1] - a[3] / 2.0,
        a[0] + a[2] / 2.0,
        a[1] + a[3] / 2.0,
    );
    let (bx0, by0, bx1, by1) = (
        b[0] - b[2] / 2.0,
        b[1] - b[3] / 2.0,
        b[0] + b[2] / 2.0,
        b[1] + b[3] / 2.0,
    );
    let iw = (ax1.min(bx1) - ax0.max(bx0)).max(0.0);
    let ih = (ay1.min(by1) - ay0.max(by0)).max(0.0);
    let inter = iw * ih;
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// NonMaxSuppressionCalculator, WEIGHTED: each kept face is the
/// score-weighted mean of the boxes that overlap the best one.
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

/// DetectionsToRectsCalculator's rotation: the angle that turns the line
/// from `start` to `end` (normalized points) level, in pixels.
fn rotation(start: [f32; 2], end: [f32; 2], width: usize, height: usize) -> f32 {
    let dx = (end[0] - start[0]) * width as f32;
    let dy = (end[1] - start[1]) * height as f32;
    normalize_radians(0.0 - (-dy).atan2(dx))
}

/// RectTransformationCalculator with no shift.
fn transform(mut r: Rect, scale: f32, square_long: bool, width: usize, height: usize) -> Rect {
    if square_long {
        let long = (r.w * width as f32).max(r.h * height as f32);
        r.w = long / width as f32;
        r.h = long / height as f32;
    }
    r.w *= scale;
    r.h *= scale;
    r
}

/// Where a pixel of a `size`×`size` tensor cut from `rect` (letterboxed to
/// keep the rect's aspect) lands in the image, in pixels; `None` in the
/// letterbox.
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
    fn new(rect: &Rect, size: usize, width: usize, height: usize) -> Self {
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

    /// Tensor position (in tensor pixels) to image pixels.
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
/// letterbox is `lo` (black).
fn crop_tensor(frame: &Frame, rect: &Rect, size: usize, lo: f32, hi: f32, out: &mut [f32]) -> Crop {
    let crop = Crop::new(rect, size, frame.width, frame.height);
    let k = (hi - lo) / 255.0;
    for v in 0..size {
        for u in 0..size {
            let ([x, y], inside) = crop.to_image(u as f32 + 0.5, v as f32 + 0.5);
            let i = (v * size + u) * 3;
            if inside {
                let px = frame.sample(x, y);
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

pub struct FaceResult {
    /// 478 points: x and y normalized to the frame, z in the same units as x.
    pub landmarks: Vec<[f32; 3]>,
    pub blendshapes: [f32; 52],
}

#[derive(Default, Debug, serde::Serialize)]
pub struct FaceTimes {
    pub detector_ms: f64,
    pub detector_runs: usize,
    pub landmarks_ms: f64,
    pub landmark_runs: usize,
    pub blendshapes_ms: f64,
    pub preprocess_ms: f64,
    /// Frames where the detector ran again in the head region.
    pub head_searches: usize,
}

pub struct FaceTracker {
    detector: Model,
    landmarks: Model,
    blendshapes: Model,
    anchors: Vec<Anchor>,
    track: Option<Rect>,
    /// Run the detector on every frame, to count faces, even while tracking.
    pub detect_every_frame: bool,
    pub times: FaceTimes,
    det_in: Vec<f32>,
    lm_in: Vec<f32>,
}

impl FaceTracker {
    pub fn load(dir: &Path, ep: Ep, threads: usize) -> Result<Self> {
        Ok(Self {
            detector: load(&dir.join("face_detector.onnx"), ep, threads)?,
            landmarks: load(&dir.join("face_landmarks_detector.onnx"), ep, threads)?,
            blendshapes: load(&dir.join("face_blendshapes.onnx"), ep, threads)?,
            anchors: anchors(),
            track: None,
            detect_every_frame: true,
            times: FaceTimes::default(),
            det_in: vec![0.0; DETECTOR_SIZE * DETECTOR_SIZE * 3],
            lm_in: vec![0.0; LANDMARK_SIZE * LANDMARK_SIZE * 3],
        })
    }

    pub fn models(&self) -> [&Model; 3] {
        [&self.detector, &self.landmarks, &self.blendshapes]
    }

    /// Faces found by BlazeFace in the whole frame (letterboxed to 128).
    pub fn detect(&mut self, frame: &Frame) -> Result<Vec<Detection>> {
        let full = Rect {
            x: 0.5,
            y: 0.5,
            w: 1.0,
            h: 1.0,
            rotation: 0.0,
        };
        self.detect_in(frame, &full)
    }

    /// Faces found by BlazeFace in an upright region of the frame, in
    /// coordinates of the whole frame. A face as small as in a standing
    /// shot (13 % of the frame's height in the spike's take) is missed by the
    /// short-range model in the whole frame; in a region around the head it
    /// is found.
    pub fn detect_in(&mut self, frame: &Frame, region: &Rect) -> Result<Vec<Detection>> {
        let t = std::time::Instant::now();
        let crop = crop_tensor(frame, region, DETECTOR_SIZE, -1.0, 1.0, &mut self.det_in);
        self.times.preprocess_ms += ms(t);
        let t = std::time::Instant::now();
        let out = self
            .detector
            .run(&[1, DETECTOR_SIZE, DETECTOR_SIZE, 3], &self.det_in)?;
        self.times.detector_ms += ms(t);
        self.times.detector_runs += 1;
        let (regressors, scores) = (&out[0].1, &out[1].1);
        let n = DETECTOR_SIZE as f32;
        let (w, h) = (frame.width as f32, frame.height as f32);
        // Tensor-normalized point to frame-normalized point.
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

    fn rect_from_detection(d: &Detection, width: usize, height: usize) -> Rect {
        let r = Rect {
            x: d.bbox[0],
            y: d.bbox[1],
            w: d.bbox[2],
            h: d.bbox[3],
            rotation: rotation(d.keypoints[0], d.keypoints[1], width, height),
        };
        transform(r, 1.5, false, width, height)
    }

    fn rect_from_landmarks(lm: &[[f32; 3]], width: usize, height: usize) -> Rect {
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
        transform(r, 1.5, true, width, height)
    }

    /// The face in this frame (tracked from the last one when possible) and
    /// how many faces the detector saw (when it ran). `head` is called for a
    /// region to search when the whole frame shows no face and none is
    /// tracked (the person mask's face and hair, for a small face).
    pub fn process(
        &mut self,
        frame: &Frame,
        head: &mut dyn FnMut(&Frame) -> Result<Option<Rect>>,
    ) -> Result<(Option<FaceResult>, Option<usize>)> {
        let mut faces_seen = None;
        let mut detections = Vec::new();
        if self.detect_every_frame || self.track.is_none() {
            detections = self.detect(frame)?;
            faces_seen = Some(detections.len());
        }
        if detections.is_empty()
            && self.track.is_none()
            && let Some(region) = head(frame)?
        {
            detections = self.detect_in(frame, &region)?;
            self.times.head_searches += 1;
            faces_seen = Some(detections.len());
        }
        let rect = match (self.track, detections.first()) {
            (Some(r), _) => r,
            (None, Some(d)) => Self::rect_from_detection(d, frame.width, frame.height),
            (None, None) => return Ok((None, faces_seen)),
        };
        let mut result = self.landmarks_in(frame, &rect)?;
        if result.is_none() && self.track.is_some() {
            // Lost the track: start again from the detector.
            self.track = None;
            if faces_seen.is_none() {
                detections = self.detect(frame)?;
                faces_seen = Some(detections.len());
            }
            if let Some(d) = detections.first() {
                let rect = Self::rect_from_detection(d, frame.width, frame.height);
                result = self.landmarks_in(frame, &rect)?;
            }
        }
        self.track = result
            .as_ref()
            .map(|r| Self::rect_from_landmarks(&r.landmarks, frame.width, frame.height));
        Ok((result, faces_seen))
    }

    fn landmarks_in(&mut self, frame: &Frame, rect: &Rect) -> Result<Option<FaceResult>> {
        let t = std::time::Instant::now();
        let crop = crop_tensor(frame, rect, LANDMARK_SIZE, 0.0, 1.0, &mut self.lm_in);
        self.times.preprocess_ms += ms(t);
        let t = std::time::Instant::now();
        let out = self
            .landmarks
            .run(&[1, LANDMARK_SIZE, LANDMARK_SIZE, 3], &self.lm_in)?;
        self.times.landmarks_ms += ms(t);
        self.times.landmark_runs += 1;
        let presence = sigmoid(out[1].1[0]);
        if presence < MIN_PRESENCE {
            return Ok(None);
        }
        let raw = &out[0].1;
        let (w, h) = (frame.width as f32, frame.height as f32);
        let landmarks: Vec<[f32; 3]> = raw
            .chunks_exact(3)
            .map(|p| {
                let ([x, y], _) = crop.to_image(p[0], p[1]);
                [x / w, y / h, p[2] / crop.scale / w]
            })
            .collect();
        let t = std::time::Instant::now();
        let mut pts = Vec::with_capacity(146 * 2);
        for &i in &BLENDSHAPE_SUBSET {
            pts.push(landmarks[i][0] * w);
            pts.push(landmarks[i][1] * h);
        }
        let scores = self.blendshapes.run(&[1, 146, 2], &pts)?;
        self.times.blendshapes_ms += ms(t);
        let mut blendshapes = [0.0; 52];
        blendshapes.copy_from_slice(&scores[0].1[..52]);
        Ok(Some(FaceResult {
            landmarks,
            blendshapes,
        }))
    }
}

pub fn ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blazeface_short_range_has_896_anchors() {
        let a = anchors();
        assert_eq!(a.len(), 896);
        // First grid is 16×16 with two anchors per cell, then 8×8 with six.
        assert_eq!((a[0].x, a[0].y), (0.5 / 16.0, 0.5 / 16.0));
        assert_eq!(a[1].x, a[0].x);
        assert_eq!((a[512].x, a[512].y), (0.5 / 8.0, 0.5 / 8.0));
        assert_eq!(a[517].x, a[512].x);
        assert_eq!(a[518].x, 1.5 / 8.0);
    }

    #[test]
    fn rotation_is_zero_for_level_eyes_and_follows_a_tilt() {
        assert!(rotation([0.4, 0.5], [0.6, 0.5], 100, 100).abs() < 1e-6);
        // Right eye lower than left on screen: the face is turned clockwise.
        let r = rotation([0.4, 0.5], [0.6, 0.7], 100, 100);
        assert!((r - PI / 4.0).abs() < 1e-5, "{r}");
    }

    #[test]
    fn square_long_uses_the_long_side_in_pixels() {
        let r = Rect {
            x: 0.5,
            y: 0.5,
            w: 0.1,
            h: 0.1,
            rotation: 0.0,
        };
        // 0.1 of 1920 is 192 px, of 1080 is 108 px: square of 192 px, ×1.5.
        let t = transform(r, 1.5, true, 1920, 1080);
        assert!((t.w * 1920.0 - 288.0).abs() < 1e-3);
        assert!((t.h * 1080.0 - 288.0).abs() < 1e-3);
    }

    #[test]
    fn crop_maps_the_tensor_center_to_the_rect_center_and_letterboxes() {
        let rect = Rect {
            x: 0.5,
            y: 0.5,
            w: 1.0,
            h: 1.0,
            rotation: 0.0,
        };
        let crop = Crop::new(&rect, 128, 1920, 1080);
        let ([x, y], inside) = crop.to_image(64.0, 64.0);
        assert!((x - 960.0).abs() < 1e-3 && (y - 540.0).abs() < 1e-3 && inside);
        // 1080 of 1920 is 72 of 128 rows: rows above 28 are letterbox.
        assert!(!crop.to_image(64.0, 27.0).1);
        assert!(crop.to_image(64.0, 29.0).1);
    }

    #[test]
    fn crop_turns_with_the_rect() {
        let rect = Rect {
            x: 0.5,
            y: 0.5,
            w: 0.5,
            h: 0.5,
            rotation: PI / 2.0,
        };
        let crop = Crop::new(&rect, 100, 200, 200);
        // A step right in the tensor is a step down in the image.
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
        assert_eq!(out[0].score, 0.9);
        assert_eq!(out[1].bbox[0], 0.8);
    }

    #[test]
    fn blendshape_subset_is_sorted_and_in_range() {
        assert!(BLENDSHAPE_SUBSET.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(*BLENDSHAPE_SUBSET.last().unwrap(), 477);
    }
}
