//! A photo into a model's input, and the model's output into an alpha and
//! a cutout. The photo's own pixels are never changed: the cutout is the
//! photo with an alpha channel.

use image::imageops::FilterType;
use image::{GrayImage, ImageBuffer, Luma, RgbImage, RgbaImage};

use crate::models::{Entry, Range, Scale};

/// The photo as the model takes it: stretched to the input size (as every
/// upstream does; none letterboxes), scaled, normalized, channel by
/// channel.
pub fn prepare(photo: &RgbImage, entry: &Entry) -> Vec<f32> {
    let [h, w] = entry.input;
    let small = image::imageops::resize(photo, w as u32, h as u32, FilterType::Triangle);
    let divisor = match entry.scale {
        Scale::Byte => 255.0,
        Scale::Max => f32::from(small.as_raw().iter().copied().max().unwrap_or(255).max(1)),
    };
    let mut chw = vec![0.0f32; 3 * h * w];
    for (i, p) in small.pixels().enumerate() {
        for c in 0..3 {
            chw[c * h * w + i] = (f32::from(p.0[c]) / divisor - entry.mean[c]) / entry.std[c];
        }
    }
    chw
}

/// The model's output as an alpha in 0..=1, with its raw range.
pub fn alpha(raw: &[f32], range: Range) -> (Vec<f32>, (f32, f32)) {
    let lo = raw.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = raw.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let alpha = match range {
        Range::AsIs => raw.iter().map(|v| v.clamp(0.0, 1.0)).collect(),
        Range::MinMax => {
            let span = (hi - lo).max(f32::EPSILON);
            raw.iter().map(|v| (v - lo) / span).collect()
        }
    };
    (alpha, (lo, hi))
}

/// The alpha at the photo's size, in 8 bits.
pub fn to_photo_size(alpha: &[f32], [h, w]: [usize; 2], width: u32, height: u32) -> GrayImage {
    let small: ImageBuffer<Luma<f32>, Vec<f32>> =
        ImageBuffer::from_raw(w as u32, h as u32, alpha.to_vec()).expect("alpha of the input size");
    let big = image::imageops::resize(&small, width, height, FilterType::Triangle);
    GrayImage::from_fn(width, height, |x, y| {
        Luma([(big.get_pixel(x, y).0[0].clamp(0.0, 1.0) * 255.0).round() as u8])
    })
}

/// The photo with the alpha as its fourth channel.
pub fn cutout(photo: &RgbImage, alpha: &GrayImage) -> RgbaImage {
    RgbaImage::from_fn(photo.width(), photo.height(), |x, y| {
        let p = photo.get_pixel(x, y).0;
        image::Rgba([p[0], p[1], p[2], alpha.get_pixel(x, y).0[0]])
    })
}

/// The cutout over a flat backdrop, to look at.
pub fn over(cut: &RgbaImage, backdrop: [u8; 3]) -> RgbImage {
    RgbImage::from_fn(cut.width(), cut.height(), |x, y| {
        let p = cut.get_pixel(x, y).0;
        let a = f32::from(p[3]) / 255.0;
        let mix =
            |c: usize| (f32::from(p[c]) * a + f32::from(backdrop[c]) * (1.0 - a)).round() as u8;
        image::Rgb([mix(0), mix(1), mix(2)])
    })
}

/// The photo no larger than `max_dim` on its longer side.
pub fn limited(photo: RgbImage, max_dim: u32) -> RgbImage {
    let longer = photo.width().max(photo.height());
    if max_dim == 0 || longer <= max_dim {
        return photo;
    }
    let scale = f64::from(max_dim) / f64::from(longer);
    let w = (f64::from(photo.width()) * scale).round().max(1.0) as u32;
    let h = (f64::from(photo.height()) * scale).round().max(1.0) as u32;
    image::imageops::resize(&photo, w, h, FilterType::Lanczos3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(scale: Scale, range: Range) -> Entry {
        Entry {
            file: "m.onnx".to_owned(),
            input: [2, 2],
            scale,
            mean: [0.5, 0.5, 0.5],
            std: [1.0, 1.0, 0.5],
            range,
        }
    }

    #[test]
    fn prepare_scales_normalizes_and_lays_channels_out() {
        let photo = RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 51]));
        let chw = prepare(&photo, &entry(Scale::Byte, Range::AsIs));
        assert_eq!(chw.len(), 12);
        assert!(chw[..4].iter().all(|&v| (v - 0.5).abs() < 1e-6));
        assert!(chw[4..8].iter().all(|&v| (v + 0.5).abs() < 1e-6));
        // (0.2 - 0.5) / 0.5
        assert!(chw[8..].iter().all(|&v| (v + 0.6).abs() < 1e-6));
    }

    #[test]
    fn prepare_by_the_largest_value_ignores_exposure() {
        let dark = RgbImage::from_pixel(2, 2, image::Rgb([100, 50, 0]));
        let chw = prepare(&dark, &entry(Scale::Max, Range::AsIs));
        assert!((chw[0] - 0.5).abs() < 1e-6);
        assert!(chw[4].abs() < 1e-6);
    }

    #[test]
    fn alpha_stretches_only_when_asked() {
        let raw = [0.2, 0.4, 0.6];
        let (a, range) = alpha(&raw, Range::MinMax);
        assert_eq!(range, (0.2, 0.6));
        assert!((a[0]).abs() < 1e-6 && (a[1] - 0.5).abs() < 1e-6 && (a[2] - 1.0).abs() < 1e-6);
        let (a, _) = alpha(&raw, Range::AsIs);
        assert_eq!(a, raw);
    }

    #[test]
    fn a_flat_output_does_not_divide_by_zero() {
        let (a, _) = alpha(&[0.3, 0.3], Range::MinMax);
        assert!(a.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_cutout_keeps_the_photos_pixels() {
        let mut photo = RgbImage::new(4, 2);
        for (i, p) in photo.pixels_mut().enumerate() {
            *p = image::Rgb([i as u8 * 10, 200, 7]);
        }
        let a = to_photo_size(&[0.0, 1.0, 1.0, 0.0], [2, 2], 4, 2);
        let cut = cutout(&photo, &a);
        for (p, c) in photo.pixels().zip(cut.pixels()) {
            assert_eq!(p.0, [c.0[0], c.0[1], c.0[2]]);
        }
        assert_eq!(cut.get_pixel(0, 0).0[3], 0);
        assert_eq!(cut.get_pixel(3, 0).0[3], 255);
    }

    #[test]
    fn over_a_backdrop_mixes_by_alpha() {
        let cut = RgbaImage::from_fn(3, 1, |x, _| {
            image::Rgba([200, 100, 0, [0, 255, 128][x as usize]])
        });
        let shown = over(&cut, [0, 0, 200]);
        assert_eq!(shown.get_pixel(0, 0).0, [0, 0, 200]);
        assert_eq!(shown.get_pixel(1, 0).0, [200, 100, 0]);
        let mid = shown.get_pixel(2, 0).0;
        assert!((i32::from(mid[0]) - 100).abs() <= 1 && (i32::from(mid[2]) - 100).abs() <= 1);
    }

    #[test]
    fn limited_keeps_the_shape_and_small_photos() {
        let big = limited(RgbImage::new(4000, 2000), 2048);
        assert_eq!((big.width(), big.height()), (2048, 1024));
        let small = limited(RgbImage::new(800, 600), 2048);
        assert_eq!((small.width(), small.height()), (800, 600));
    }
}
