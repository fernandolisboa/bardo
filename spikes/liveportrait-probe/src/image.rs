//! RGB images and the three pixel operations LivePortrait's crop and
//! paste-back need, as OpenCV does them upstream.

use crate::crop::Affine;

/// 8-bit RGB, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

impl Image {
    pub fn new(width: usize, height: usize) -> Image {
        Image {
            width,
            height,
            rgb: vec![0; width * height * 3],
        }
    }

    /// The image as a 1 × 3 × H × W tensor from 0 to 1.
    pub fn to_chw(&self) -> Vec<f32> {
        let n = self.width * self.height;
        let mut out = vec![0.0; 3 * n];
        for (i, px) in self.rgb.as_chunks::<3>().0.iter().enumerate() {
            for c in 0..3 {
                out[c * n + i] = f32::from(px[c]) / 255.0;
            }
        }
        out
    }

    /// From a 1 × 3 × H × W tensor from 0 to 1 (upstream's
    /// `parse_output`: clipped, times 255, truncated).
    pub fn from_chw(width: usize, height: usize, chw: &[f32]) -> Image {
        let n = width * height;
        let mut img = Image::new(width, height);
        for i in 0..n {
            for c in 0..3 {
                img.rgb[i * 3 + c] = (chw[c * n + i].clamp(0.0, 1.0) * 255.0) as u8;
            }
        }
        img
    }
}

/// `cv2.warpAffine(src, m, (width, height))`: every output pixel reads the
/// source where `m`'s inverse takes it, bilinear, black outside.
pub fn warp(src: &Image, m: &Affine, width: usize, height: usize) -> Image {
    let inv = m.inverse();
    let mut out = Image::new(width, height);
    let (w, h) = (src.width as i64, src.height as i64);
    let px = |x: i64, y: i64, c: usize| -> f32 {
        if x < 0 || y < 0 || x >= w || y >= h {
            0.0
        } else {
            f32::from(src.rgb[(y as usize * src.width + x as usize) * 3 + c])
        }
    };
    for y in 0..height {
        for x in 0..width {
            let (sx, sy) = inv.apply(x as f32, y as f32);
            if !(sx > -1.0 && sy > -1.0 && sx < w as f32 && sy < h as f32) {
                continue;
            }
            let (x0, y0) = (sx.floor(), sy.floor());
            let (fx, fy) = (sx - x0, sy - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let o = (y * width + x) * 3;
            for c in 0..3 {
                let top = px(x0, y0, c) * (1.0 - fx) + px(x0 + 1, y0, c) * fx;
                let bottom = px(x0, y0 + 1, c) * (1.0 - fx) + px(x0 + 1, y0 + 1, c) * fx;
                out.rgb[o + c] = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Half the size, each pixel the mean of a 2 × 2 block
/// (`cv2.resize(..., INTER_AREA)` at exactly half).
pub fn half(src: &Image) -> Image {
    let (w, h) = (src.width / 2, src.height / 2);
    let mut out = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let at = |xx: usize, yy: usize| u32::from(src.rgb[(yy * src.width + xx) * 3 + c]);
                let sum = at(2 * x, 2 * y)
                    + at(2 * x + 1, 2 * y)
                    + at(2 * x, 2 * y + 1)
                    + at(2 * x + 1, 2 * y + 1);
                out.rgb[(y * w + x) * 3 + c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    out
}

/// The paste-back mask in the frame: upstream's mask template taken from
/// the crop to the frame, 0 to 1 per channel (`prepare_paste_back`).
pub struct PasteMask {
    pub weight: Vec<f32>,
    /// The rows that have any of the crop in them, to skip the rest.
    rows: std::ops::Range<usize>,
}

impl PasteMask {
    pub fn new(template: &Image, crop_to_frame: &Affine, width: usize, height: usize) -> PasteMask {
        let warped = warp(template, crop_to_frame, width, height);
        let weight: Vec<f32> = warped.rgb.iter().map(|&v| f32::from(v) / 255.0).collect();
        let row_used = |y: usize| {
            weight[y * width * 3..(y + 1) * width * 3]
                .iter()
                .any(|&v| v > 0.0)
        };
        let first = (0..height).find(|&y| row_used(y)).unwrap_or(height);
        let last = (0..height)
            .rev()
            .find(|&y| row_used(y))
            .map_or(first, |y| y + 1);
        PasteMask {
            weight,
            rows: first..last,
        }
    }
}

/// The generated crop put back into the frame through the mask
/// (`paste_back`: blended, clipped, truncated to 8 bits).
pub fn paste_back(crop: &Image, crop_to_frame: &Affine, frame: &Image, mask: &PasteMask) -> Image {
    let mut out = frame.clone();
    let rows = mask.rows.clone();
    if rows.is_empty() {
        return out;
    }
    // Only the rows the mask covers: the warp of the whole frame is most of
    // the cost, and the face is a small part of it.
    let band = Affine([
        crop_to_frame.0[0],
        [
            crop_to_frame.0[1][0],
            crop_to_frame.0[1][1],
            crop_to_frame.0[1][2] - rows.start as f32,
        ],
    ]);
    let warped = warp(crop, &band, frame.width, rows.len());
    for y in rows.clone() {
        for i in y * frame.width * 3..(y + 1) * frame.width * 3 {
            let m = mask.weight[i];
            if m == 0.0 {
                continue;
            }
            let g = f32::from(warped.rgb[i - rows.start * frame.width * 3]);
            let v = m * g + (1.0 - m) * f32::from(frame.rgb[i]);
            out.rgb[i] = v.clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{self, bytes};

    fn image(v: &serde_json::Value, w: usize, h: usize) -> Image {
        Image {
            width: w,
            height: h,
            rgb: bytes(v),
        }
    }

    fn affine(v: &serde_json::Value) -> Affine {
        let f = fixtures::floats(v);
        Affine([[f[0], f[1], f[2]], [f[3], f[4], f[5]]])
    }

    /// OpenCV interpolates in fixed point (1/32 of a pixel, 15-bit
    /// weights): a level or two apart from floating point.
    fn near(got: &[u8], want: &[u8], tolerance: u8) {
        assert_eq!(got.len(), want.len());
        let worst = got
            .iter()
            .zip(want)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= tolerance, "worst difference {worst}");
    }

    #[test]
    fn warp_matches_opencv() {
        let fx = &fixtures::upstream()["pixels"];
        let src = image(&fx["image"], 32, 24);
        let got = warp(&src, &affine(&fx["affine"]), 20, 18);
        near(&got.rgb, &bytes(&fx["warped"]), 2);
    }

    #[test]
    fn half_matches_opencvs_area_resize() {
        let fx = &fixtures::upstream()["pixels"];
        let got = half(&image(&fx["image"], 32, 24));
        near(&got.rgb, &bytes(&fx["half"]), 1);
    }

    #[test]
    fn paste_back_matches_upstream() {
        let fx = &fixtures::upstream()["pixels"];
        let frame = image(&fx["image"], 32, 24);
        let m = affine(&fx["m_c2o"]);
        let mask = PasteMask::new(&image(&fx["mask"], 20, 18), &m, 32, 24);
        let got = paste_back(&image(&fx["crop"], 20, 18), &m, &frame, &mask);
        near(&got.rgb, &bytes(&fx["pasted"]), 2);
    }

    #[test]
    fn tensors_go_there_and_back() {
        let img = Image {
            width: 2,
            height: 1,
            rgb: vec![0, 128, 255, 10, 20, 30],
        };
        let t = img.to_chw();
        assert_eq!(t[0], 0.0);
        assert_eq!(t[1], 10.0 / 255.0);
        assert_eq!(t[4], 1.0);
        assert_eq!(Image::from_chw(2, 1, &t), img);
    }
}
