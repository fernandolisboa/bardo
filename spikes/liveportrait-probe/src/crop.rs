//! Where LivePortrait crops a face, from its landmarks (upstream
//! `src/utils/crop.py`), with MediaPipe's mesh standing in for upstream's
//! points: InsightFace's 106 for a still source, its own landmark model's
//! 203 for a driving video.

/// A 2 × 3 affine transform, `[x', y'] = m · [x, y, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Affine(pub [[f32; 3]; 2]);

impl Affine {
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.0;
        (
            m[0][0] * x + m[0][1] * y + m[0][2],
            m[1][0] * x + m[1][1] * y + m[1][2],
        )
    }

    pub fn inverse(&self) -> Affine {
        let [[a, b, c], [d, e, f]] = self.0;
        let det = a * e - b * d;
        let (ia, ib, id, ie) = (e / det, -b / det, -d / det, a / det);
        Affine([[ia, ib, -(ia * c + ib * f)], [id, ie, -(id * c + ie * f)]])
    }
}

/// The landmarks a crop is measured from: every point whose extent sets
/// its size, and the two that set its tilt.
#[derive(Debug, Clone)]
pub struct FacePoints {
    pub points: Vec<[f32; 2]>,
    /// Midway between the eyes' centers.
    pub eyes: [f32; 2],
    /// Midway between the mouth's corners.
    pub mouth: [f32; 2],
}

fn mean(points: &[[f32; 2]], indices: &[usize]) -> [f32; 2] {
    let n = indices.len() as f32;
    let (x, y) = indices.iter().fold((0.0, 0.0), |(x, y), &i| {
        (x + points[i][0], y + points[i][1])
    });
    [x / n, y / n]
}

fn midway(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

impl FacePoints {
    /// Upstream's 203 landmarks (`parse_pt2_from_pt203`).
    #[cfg(test)]
    pub fn from_203(points: &[[f32; 2]]) -> FacePoints {
        let left = mean(points, &[0, 6, 12, 18]);
        let right = mean(points, &[24, 30, 36, 42]);
        FacePoints {
            points: points.to_vec(),
            eyes: midway(left, right),
            mouth: midway(points[48], points[66]),
        }
    }

    /// MediaPipe's 478-point mesh, in pixels. The points kept are the ones
    /// that match what upstream's 106 and 203 cover: the face's outline from
    /// ear to ear through the chin (not the forehead, which both leave out),
    /// the brows, the eyes and the mouth's corners.
    pub fn from_mediapipe(mesh: &[[f32; 2]]) -> FacePoints {
        let left = mean(mesh, &MP_LEFT_EYE);
        let right = mean(mesh, &MP_RIGHT_EYE);
        FacePoints {
            points: MP_EXTENT.iter().map(|&i| mesh[i]).collect(),
            eyes: midway(left, right),
            mouth: midway(mesh[MP_MOUTH_CORNERS[0]], mesh[MP_MOUTH_CORNERS[1]]),
        }
    }
}

/// MediaPipe mesh points, by index. Eyes as seen on screen: outer corner,
/// inner corner, top, bottom (upstream averages four points the same way).
const MP_LEFT_EYE: [usize; 4] = [33, 133, 159, 145];
const MP_RIGHT_EYE: [usize; 4] = [263, 362, 386, 374];
const MP_MOUTH_CORNERS: [usize; 2] = [61, 291];
/// The inner lips' middle, top and bottom.
const MP_INNER_LIPS: [usize; 2] = [13, 14];
const MP_EXTENT: [usize; 53] = [
    // Outline, ear to ear through the chin.
    127, 234, 93, 132, 58, 172, 136, 150, 149, 176, 148, 152, 377, 400, 378, 379, 365, 397, 288,
    361, 323, 454, 356, // Brows, upper and lower rows.
    70, 63, 105, 66, 107, 46, 53, 52, 65, 55, 300, 293, 334, 296, 336, 276, 283, 282, 295, 285,
    // Eyes and the mouth's corners.
    33, 133, 159, 145, 263, 362, 386, 374, 61, 291,
];

/// How open the mouth is: the inner lips' gap over the mouth's width
/// (upstream's `calc_lip_close_ratio`, points 90, 102 over 48, 66).
#[cfg(test)]
pub fn lip_ratio_203(points: &[[f32; 2]]) -> f32 {
    distance(points[90], points[102]) / (distance(points[48], points[66]) + 1e-6)
}

/// The same from MediaPipe's mesh.
pub fn lip_ratio_mediapipe(mesh: &[[f32; 2]]) -> f32 {
    distance(mesh[MP_INNER_LIPS[0]], mesh[MP_INNER_LIPS[1]])
        / (distance(mesh[MP_MOUTH_CORNERS[0]], mesh[MP_MOUTH_CORNERS[1]]) + 1e-6)
}

/// A square around the face, turned with it (`parse_rect_from_landmark`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub center: [f32; 2],
    pub size: f32,
    /// Radians, counterclockwise.
    pub angle: f32,
}

/// The face's square: its landmarks' extent along the axes from the eyes
/// to the mouth, its longer side times `scale`, moved by `vx` and `vy`
/// times its size along those axes (negative `vy` takes in more forehead).
pub fn rect(face: &FacePoints, scale: f32, vx: f32, vy: f32) -> Rect {
    let mut uy = [face.mouth[0] - face.eyes[0], face.mouth[1] - face.eyes[1]];
    let l = (uy[0] * uy[0] + uy[1] * uy[1]).sqrt();
    if l <= 1e-3 {
        uy = [0.0, 1.0];
    } else {
        uy = [uy[0] / l, uy[1] / l];
    }
    let ux = [uy[1], -uy[0]];
    let mut angle = ux[0].clamp(-1.0, 1.0).acos();
    if ux[1] < 0.0 {
        angle = -angle;
    }
    let n = face.points.len() as f32;
    let c0 = face
        .points
        .iter()
        .fold([0.0, 0.0], |c, p| [c[0] + p[0] / n, c[1] + p[1] / n]);
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in &face.points {
        let d = [p[0] - c0[0], p[1] - c0[1]];
        let r = [d[0] * ux[0] + d[1] * ux[1], d[0] * uy[0] + d[1] * uy[1]];
        for k in 0..2 {
            lo[k] = lo[k].min(r[k]);
            hi[k] = hi[k].max(r[k]);
        }
    }
    let c1 = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]) * scale;
    let center = [
        c0[0] + ux[0] * c1[0] + uy[0] * c1[1] + ux[0] * vx * size + uy[0] * vy * size,
        c0[1] + ux[1] * c1[0] + uy[1] * c1[1] + ux[1] * vx * size + uy[1] * vy * size,
    ];
    Rect {
        center,
        size,
        angle,
    }
}

/// From the frame to a `dsize` square crop of `r`, turned upright
/// (`_estimate_similar_transform_from_pts` with rotation).
pub fn upright(r: &Rect, dsize: f32) -> Affine {
    let s = dsize / r.size;
    let (cos, sin) = (r.angle.cos(), r.angle.sin());
    let [cx, cy] = r.center;
    let t = dsize / 2.0;
    Affine([
        [s * cos, s * sin, t - s * (cos * cx + sin * cy)],
        [-s * sin, s * cos, t - s * (-sin * cx + cos * cy)],
    ])
}

/// Upstream's source crop: 512 square, 2.3 times the face, an eighth of
/// that higher (`CropConfig`).
pub const SOURCE_DSIZE: f32 = 512.0;
pub const SOURCE_SCALE: f32 = 2.3;
pub const SOURCE_VY: f32 = -0.125;
/// Upstream's driving crop: 2.2 times the face, a tenth higher, upright.
pub const DRIVING_SCALE: f32 = 2.2;
pub const DRIVING_VY: f32 = -0.1;

/// The crop of a still source: frame to the 512 square.
pub fn source_transform(face: &FacePoints) -> Affine {
    upright(&rect(face, SOURCE_SCALE, 0.0, SOURCE_VY), SOURCE_DSIZE)
}

/// One driving frame's box, left, top, right, bottom: the face's square
/// without its tilt (`parse_bbox_from_landmark`).
pub fn driving_box(face: &FacePoints) -> [f32; 4] {
    let r = rect(face, DRIVING_SCALE, 0.0, DRIVING_VY);
    let h = r.size / 2.0;
    [
        r.center[0] - h,
        r.center[1] - h,
        r.center[0] + h,
        r.center[1] + h,
    ]
}

/// The mean of every frame's box: the driving video is cropped in one
/// place for the whole take (`average_bbox_lst`).
pub fn mean_box(boxes: &[[f32; 4]]) -> Option<[f32; 4]> {
    if boxes.is_empty() {
        return None;
    }
    let n = boxes.len() as f64;
    let mut sum = [0.0f64; 4];
    for b in boxes {
        for k in 0..4 {
            sum[k] += f64::from(b[k]);
        }
    }
    Some(sum.map(|s| (s / n) as f32))
}

/// From the frame to a `dsize` square of `b`, upright
/// (`crop_image_by_bbox` without rotation).
pub fn box_transform(b: &[f32; 4], dsize: f32) -> Affine {
    let s = dsize / (b[2] - b[0]);
    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let t = dsize / 2.0;
    Affine([[s, 0.0, t - s * cx], [0.0, s, t - s * cy]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{self, close};

    fn points(v: &serde_json::Value) -> Vec<[f32; 2]> {
        fixtures::floats(v)
            .chunks(2)
            .map(|p| [p[0], p[1]])
            .collect()
    }

    fn same(got: &Affine, want: &[f32]) {
        for (g, w) in got.0.iter().flatten().zip(want) {
            close(*g, *w, 1e-4);
        }
    }

    #[test]
    fn the_source_crop_matches_upstream() {
        let fx = &fixtures::upstream()["source_crop"];
        let face = FacePoints::from_203(&points(&fx["points"]));
        let f = |k: &str| fx[k].as_f64().unwrap() as f32;
        assert_eq!(
            (f("dsize"), f("scale"), f("vx_ratio"), f("vy_ratio")),
            (SOURCE_DSIZE, SOURCE_SCALE, 0.0, SOURCE_VY)
        );
        same(&source_transform(&face), &fixtures::floats(&fx["m_o2c"]));
    }

    #[test]
    fn the_driving_crop_is_the_mean_box_as_upstream_takes_it() {
        let all = fixtures::upstream();
        let base = points(&all["source_crop"]["points"]);
        let fx = &all["driving_crop"];
        assert_eq!(
            (
                fx["scale"].as_f64().unwrap() as f32,
                fx["vy_ratio"].as_f64().unwrap() as f32
            ),
            (DRIVING_SCALE, DRIVING_VY)
        );
        let boxes: Vec<[f32; 4]> = fixtures::floats(&fx["offsets"])
            .chunks(2)
            .map(|o| {
                let moved: Vec<[f32; 2]> =
                    base.iter().map(|p| [p[0] + o[0], p[1] + o[1]]).collect();
                driving_box(&FacePoints::from_203(&moved))
            })
            .collect();
        let b = mean_box(&boxes).unwrap();
        for (g, w) in b.iter().zip(fixtures::floats(&fx["box"])) {
            close(*g, w, 1e-4);
        }
        same(&box_transform(&b, 512.0), &fixtures::floats(&fx["m_o2c"]));
        assert_eq!(mean_box(&[]), None);
    }

    #[test]
    fn the_lip_ratio_is_upstreams() {
        let fx = &fixtures::upstream()["lip_ratio"];
        close(
            lip_ratio_203(&points(&fx["points"])),
            fx["ratio"].as_f64().unwrap() as f32,
            1e-5,
        );
    }

    #[test]
    fn an_affine_transform_undoes_itself() {
        let m = Affine([[1.2, -0.3, 40.0], [0.25, 0.9, -7.0]]);
        let (x, y) = m.apply(13.0, -4.5);
        let (bx, by) = m.inverse().apply(x, y);
        close(bx, 13.0, 1e-5);
        close(by, -4.5, 1e-5);
    }

    #[test]
    fn mediapipe_points_set_the_tilt_from_eyes_and_mouth() {
        // A mesh of 478 points all at the origin, but for the eyes level
        // at y = 100 and the mouth's corners below them: an upright face.
        let mut mesh = vec![[50.0, 150.0]; 478];
        for &i in &MP_LEFT_EYE {
            mesh[i] = [30.0, 100.0];
        }
        for &i in &MP_RIGHT_EYE {
            mesh[i] = [70.0, 100.0];
        }
        mesh[61] = [40.0, 140.0];
        mesh[291] = [60.0, 140.0];
        let face = FacePoints::from_mediapipe(&mesh);
        assert_eq!((face.eyes, face.mouth), ([50.0, 100.0], [50.0, 140.0]));
        let r = rect(&face, 1.0, 0.0, 0.0);
        close(r.angle, 0.0, 1e-6);
        // Turn the face 90° on screen: the angle follows.
        let turned: Vec<[f32; 2]> = mesh.iter().map(|p| [-p[1], p[0]]).collect();
        let r = rect(&FacePoints::from_mediapipe(&turned), 1.0, 0.0, 0.0);
        close(r.angle.abs(), std::f32::consts::FRAC_PI_2, 1e-5);
        mesh[13] = [50.0, 130.0];
        mesh[14] = [50.0, 135.0];
        close(lip_ratio_mediapipe(&mesh), 0.25, 1e-5);
    }
}
