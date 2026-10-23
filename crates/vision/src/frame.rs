//! Reading pixels of a decoded frame.

use bardo_domain::RgbFrame;

/// Bilinear sample at a pixel position (pixel centers at .5), clamped to
/// the edge: the three channels as 0..255.
pub(crate) fn sample(frame: &RgbFrame, x: f32, y: f32) -> [f32; 3] {
    let (w, h) = (frame.width as usize, frame.height as usize);
    if w == 0 || h == 0 {
        return [0.0; 3];
    }
    let x = (x - 0.5).clamp(0.0, (w - 1) as f32);
    let y = (y - 0.5).clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |xx: usize, yy: usize, c: usize| f32::from(frame.rgb[(yy * w + xx) * 3 + c]);
    let mut out = [0.0; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let top = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
        let bottom = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
        *o = top * (1.0 - fy) + bottom * fy;
    }
    out
}

/// The frame stretched to `width` × `height`, channels last (NHWC).
pub(crate) fn stretch_hwc(
    frame: &RgbFrame,
    width: usize,
    height: usize,
    out: &mut [f32],
    map: impl Fn(f32) -> f32,
) {
    let (sx, sy) = (
        frame.width as f32 / width as f32,
        frame.height as f32 / height as f32,
    );
    for v in 0..height {
        for u in 0..width {
            let px = sample(frame, (u as f32 + 0.5) * sx, (v as f32 + 0.5) * sy);
            let i = (v * width + u) * 3;
            for c in 0..3 {
                out[i + c] = map(px[c]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_between_pixels_and_clamps_at_the_edge() {
        let frame = RgbFrame {
            width: 2,
            height: 1,
            rgb: vec![0, 0, 0, 200, 100, 50],
        };
        assert_eq!(sample(&frame, 0.5, 0.5), [0.0; 3]);
        assert_eq!(sample(&frame, 1.0, 0.5), [100.0, 50.0, 25.0]);
        assert_eq!(sample(&frame, 9.0, 9.0), [200.0, 100.0, 50.0]);
        let mut out = vec![0.0; 6];
        stretch_hwc(&frame, 2, 1, &mut out, |v| v / 255.0);
        assert!((out[3] - 200.0 / 255.0).abs() < 1e-6);
        assert!((out[4] - 100.0 / 255.0).abs() < 1e-6);
    }
}
