//! MediaPipe's multiclass selfie segmenter: the whole frame stretched to
//! 256×256, values -1..1, six class logits per pixel (background, hair,
//! body skin, face skin, clothes, other). The person is everything but
//! background.

use std::path::Path;

use anyhow::Result;

use crate::face::{Rect, ms};
use crate::runtime::{Ep, Model, load};
use crate::video::Frame;

pub const SIZE: usize = 256;
pub const CLASSES: usize = 6;

pub struct Segmenter {
    pub model: Model,
    input: Vec<f32>,
    pub run_ms: f64,
    pub preprocess_ms: f64,
    pub runs: usize,
}

/// Per pixel of the 256×256 grid, the probability of each class.
pub struct Mask {
    pub probs: Vec<[f32; CLASSES]>,
}

impl Mask {
    pub fn person(&self, i: usize) -> f32 {
        1.0 - self.probs[i][0]
    }

    /// Person probability at a normalized position, bilinear.
    pub fn person_at(&self, x: f32, y: f32) -> f32 {
        let fx = (x * SIZE as f32 - 0.5).clamp(0.0, (SIZE - 1) as f32);
        let fy = (y * SIZE as f32 - 0.5).clamp(0.0, (SIZE - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(SIZE - 1), (y0 + 1).min(SIZE - 1));
        let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
        let p = |x: usize, y: usize| self.person(y * SIZE + x);
        (p(x0, y0) * (1.0 - ax) + p(x1, y0) * ax) * (1.0 - ay)
            + (p(x0, y1) * (1.0 - ax) + p(x1, y1) * ax) * ay
    }

    pub fn class_at(&self, i: usize) -> usize {
        let p = &self.probs[i];
        (0..CLASSES)
            .max_by(|&a, &b| p[a].total_cmp(&p[b]))
            .unwrap_or(0)
    }

    /// A square around the face-skin pixels, three times their size, for
    /// finding a face too small to detect in the whole frame.
    pub fn head_region(&self, width: usize, height: usize) -> Option<Rect> {
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (SIZE, SIZE, 0, 0, 0);
        for y in 0..SIZE {
            for x in 0..SIZE {
                if self.class_at(y * SIZE + x) == 3 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                    n += 1;
                }
            }
        }
        if n < 4 {
            return None;
        }
        let cx = (x0 + x1 + 1) as f32 / 2.0 / SIZE as f32;
        let cy = (y0 + y1 + 1) as f32 / 2.0 / SIZE as f32;
        let fw = (x1 + 1 - x0) as f32 / SIZE as f32 * width as f32;
        let fh = (y1 + 1 - y0) as f32 / SIZE as f32 * height as f32;
        let side = fw.max(fh) * 3.0;
        Some(Rect {
            x: cx,
            y: cy,
            w: side / width as f32,
            h: side / height as f32,
            rotation: 0.0,
        })
    }

    /// Intersection over union of the person pixels of two masks.
    pub fn iou(&self, other: &Mask) -> f32 {
        let (mut inter, mut union) = (0usize, 0usize);
        for i in 0..SIZE * SIZE {
            let (a, b) = (self.person(i) >= 0.5, other.person(i) >= 0.5);
            inter += usize::from(a && b);
            union += usize::from(a || b);
        }
        if union == 0 {
            1.0
        } else {
            inter as f32 / union as f32
        }
    }
}

impl Segmenter {
    pub fn load(dir: &Path, ep: Ep, threads: usize) -> Result<Self> {
        Ok(Self {
            model: load(&dir.join("selfie_multiclass_256x256.onnx"), ep, threads)?,
            input: vec![0.0; SIZE * SIZE * 3],
            run_ms: 0.0,
            preprocess_ms: 0.0,
            runs: 0,
        })
    }

    pub fn segment(&mut self, frame: &Frame) -> Result<Mask> {
        let t = std::time::Instant::now();
        let (sx, sy) = (
            frame.width as f32 / SIZE as f32,
            frame.height as f32 / SIZE as f32,
        );
        for v in 0..SIZE {
            for u in 0..SIZE {
                let px = frame.sample((u as f32 + 0.5) * sx, (v as f32 + 0.5) * sy);
                let i = (v * SIZE + u) * 3;
                for c in 0..3 {
                    self.input[i + c] = px[c] / 127.5 - 1.0;
                }
            }
        }
        self.preprocess_ms += ms(t);
        let t = std::time::Instant::now();
        let out = self.model.run(&[1, SIZE, SIZE, 3], &self.input)?;
        self.run_ms += ms(t);
        self.runs += 1;
        let probs = out[0]
            .1
            .as_chunks::<CLASSES>()
            .0
            .iter()
            .map(|l| {
                let max = l.iter().copied().fold(f32::MIN, f32::max);
                let mut p = [0.0; CLASSES];
                let mut sum = 0.0;
                for c in 0..CLASSES {
                    p[c] = (l[c] - max).exp();
                    sum += p[c];
                }
                p.iter_mut().for_each(|v| *v /= sum);
                p
            })
            .collect();
        Ok(Mask { probs })
    }
}
