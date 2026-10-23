//! Drawing for the look-at outputs: the mesh, filled face triangles, and
//! colored depth. These are rough versions of the Mesh and Depth control
//! videos, to judge the models by eye.

use crate::edges;
use crate::video::Frame;

/// The frame scaled to `width`×`height`.
pub fn scaled(frame: &Frame, width: usize, height: usize) -> Frame {
    let mut out = Frame::new(width, height);
    let (sx, sy) = (
        frame.width as f32 / width as f32,
        frame.height as f32 / height as f32,
    );
    for y in 0..height {
        for x in 0..width {
            let p = frame.sample((x as f32 + 0.5) * sx, (y as f32 + 0.5) * sy);
            let i = (y * width + x) * 3;
            for c in 0..3 {
                out.rgb[i + c] = p[c] as u8;
            }
        }
    }
    out
}

pub fn line(f: &mut Frame, a: [f32; 2], b: [f32; 2], rgb: [u8; 3]) {
    let steps = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as usize).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        f.put(
            (a[0] + (b[0] - a[0]) * t).round() as i64,
            (a[1] + (b[1] - a[1]) * t).round() as i64,
            rgb,
        );
    }
}

pub fn triangle(f: &mut Frame, p: [[f32; 2]; 3], rgb: [u8; 3]) {
    let x0 = p
        .iter()
        .map(|q| q[0])
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as i64;
    let x1 = p
        .iter()
        .map(|q| q[0])
        .fold(f32::MIN, f32::max)
        .ceil()
        .min(f.width as f32 - 1.0) as i64;
    let y0 = p
        .iter()
        .map(|q| q[1])
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as i64;
    let y1 = p
        .iter()
        .map(|q| q[1])
        .fold(f32::MIN, f32::max)
        .ceil()
        .min(f.height as f32 - 1.0) as i64;
    let edge = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let area = edge(p[0], p[1], p[2]);
    if area.abs() < 1e-6 {
        return;
    }
    for y in y0..=y1 {
        for x in x0..=x1 {
            let c = [x as f32 + 0.5, y as f32 + 0.5];
            let (w0, w1, w2) = (
                edge(p[1], p[2], c),
                edge(p[2], p[0], c),
                edge(p[0], p[1], c),
            );
            let inside = if area > 0.0 {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };
            if inside {
                f.put(x, y, rgb);
            }
        }
    }
}

/// Covers the face: every mesh triangle filled, then the tessellation,
/// contours and irises drawn over it.
pub fn mesh(
    f: &mut Frame,
    landmarks: &[[f32; 3]],
    fill: [u8; 3],
    lines: [u8; 3],
    contour: [u8; 3],
) {
    let (w, h) = (f.width as f32, f.height as f32);
    let pt = |i: u16| [landmarks[i as usize][0] * w, landmarks[i as usize][1] * h];
    for t in &edges::TRIANGLES {
        triangle(f, [pt(t[0]), pt(t[1]), pt(t[2])], fill);
    }
    for t in &edges::TRIANGLES {
        line(f, pt(t[0]), pt(t[1]), lines);
        line(f, pt(t[1]), pt(t[2]), lines);
        line(f, pt(t[2]), pt(t[0]), lines);
    }
    for e in edges::CONTOURS
        .iter()
        .chain(&edges::LEFT_IRIS)
        .chain(&edges::RIGHT_IRIS)
    {
        line(f, pt(e[0]), pt(e[1]), contour);
    }
}

/// Near is warm, far is cool (a short Turbo-like ramp), for 0..1.
pub fn depth_color(v: f32) -> [u8; 3] {
    const STOPS: [[f32; 3]; 5] = [
        [48.0, 18.0, 59.0],
        [40.0, 120.0, 230.0],
        [60.0, 200.0, 130.0],
        [245.0, 190.0, 40.0],
        [200.0, 40.0, 20.0],
    ];
    let x = v.clamp(0.0, 1.0) * (STOPS.len() - 1) as f32;
    let i = (x.floor() as usize).min(STOPS.len() - 2);
    let t = x - i as f32;
    let mut out = [0u8; 3];
    for c in 0..3 {
        out[c] = (STOPS[i][c] * (1.0 - t) + STOPS[i + 1][c] * t) as u8;
    }
    out
}
