//! LivePortrait's implicit keypoints: the motion extractor's outputs, the
//! keypoints in a pose, and how a driving frame moves the source's
//! (upstream `live_portrait_pipeline.py` for a still source and a driving
//! video, relative motion, "pose-friendly", animation region "all").

/// Implicit keypoints.
pub const KP: usize = 21;
/// Their coordinates, three a keypoint.
pub const COORDS: usize = KP * 3;

pub type Keypoints = [f32; COORDS];

/// What the motion extractor reads from one 256 × 256 face crop.
#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    /// Head angles, in degrees.
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub t: [f32; 3],
    pub exp: Keypoints,
    pub scale: f32,
    /// The canonical keypoints.
    pub kp: Keypoints,
}

/// A head angle from the motion extractor's 66 bins of 3°: the softmax's
/// expected bin, from −97.5° to 97.5°.
pub fn degrees(logits: &[f32]) -> f32 {
    let max = logits.iter().copied().fold(f32::MIN, f32::max);
    let (mut sum, mut weighted) = (0.0f64, 0.0f64);
    for (i, &l) in logits.iter().enumerate() {
        let e = f64::from(l - max).exp();
        sum += e;
        weighted += e * i as f64;
    }
    (weighted / sum * 3.0 - 97.5) as f32
}

pub type Matrix = [[f32; 3]; 3];

fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

fn transpose(a: &Matrix) -> Matrix {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = a[j][i];
        }
    }
    m
}

/// Upstream's `get_rotation_matrix`: `(Rz Ry Rx)ᵀ`, for keypoints as row
/// vectors (`kp · R`).
pub fn rotation(pitch: f32, yaw: f32, roll: f32) -> Matrix {
    let (x, y, z) = (pitch.to_radians(), yaw.to_radians(), roll.to_radians());
    let rx = [
        [1.0, 0.0, 0.0],
        [0.0, x.cos(), -x.sin()],
        [0.0, x.sin(), x.cos()],
    ];
    let ry = [
        [y.cos(), 0.0, y.sin()],
        [0.0, 1.0, 0.0],
        [-y.sin(), 0.0, y.cos()],
    ];
    let rz = [
        [z.cos(), -z.sin(), 0.0],
        [z.sin(), z.cos(), 0.0],
        [0.0, 0.0, 1.0],
    ];
    transpose(&mul(&mul(&rz, &ry), &rx))
}

/// `scale · (kp · R + exp) + t`, every keypoint as a row.
fn pose(kp: &Keypoints, r: &Matrix, exp: &Keypoints, scale: f32, t: [f32; 3]) -> Keypoints {
    let mut out = [0.0; COORDS];
    for k in 0..KP {
        for j in 0..3 {
            let rotated: f32 = (0..3).map(|i| kp[k * 3 + i] * r[i][j]).sum();
            out[k * 3 + j] = scale * (rotated + exp[k * 3 + j]) + t[j];
        }
    }
    out
}

impl Motion {
    pub fn rotation(&self) -> Matrix {
        rotation(self.pitch, self.yaw, self.roll)
    }

    /// The keypoints in this pose, moved in x and y only (upstream's
    /// `transform_keypoint`): the source's `x_s`.
    pub fn keypoints(&self) -> Keypoints {
        pose(
            &self.kp,
            &self.rotation(),
            &self.exp,
            self.scale,
            [self.t[0], self.t[1], 0.0],
        )
    }

    /// The source's keypoints moved as the driving video moved from its
    /// `first` frame to `frame`: rotation, expression, scale and x/y shift
    /// relative to the first frame, on top of the source's own.
    pub fn driven(&self, first: &Motion, frame: &Motion) -> Keypoints {
        let r = mul(
            &mul(&frame.rotation(), &transpose(&first.rotation())),
            &self.rotation(),
        );
        let mut exp = self.exp;
        for (e, (f, f0)) in exp.iter_mut().zip(frame.exp.iter().zip(&first.exp)) {
            *e += f - f0;
        }
        let scale = self.scale * (frame.scale / first.scale);
        let t = [
            self.t[0] + frame.t[0] - first.t[0],
            self.t[1] + frame.t[1] - first.t[1],
            0.0,
        ];
        pose(&self.kp, &r, &exp, scale, t)
    }
}

/// The stitching network's input: source and driving keypoints, side by side.
pub fn stitching_input(source: &Keypoints, driving: &Keypoints) -> [f32; 2 * COORDS] {
    let mut v = [0.0; 2 * COORDS];
    v[..COORDS].copy_from_slice(source);
    v[COORDS..].copy_from_slice(driving);
    v
}

/// Applies the stitching network's 65 outputs: the first 63 move every
/// coordinate, the last two shift every keypoint in x and y.
pub fn stitch(driving: &Keypoints, delta: &[f32]) -> Keypoints {
    let mut out = *driving;
    for (o, d) in out.iter_mut().zip(&delta[..COORDS]) {
        *o += d;
    }
    for k in 0..KP {
        out[k * 3] += delta[COORDS];
        out[k * 3 + 1] += delta[COORDS + 1];
    }
    out
}

/// The lip retargeting network's input to close the source's mouth: the
/// source keypoints, its lip-open ratio and the wanted one (closed, 0).
pub fn lip_input(source: &Keypoints, source_lip_ratio: f32) -> [f32; COORDS + 2] {
    let mut v = [0.0; COORDS + 2];
    v[..COORDS].copy_from_slice(source);
    v[COORDS] = source_lip_ratio;
    v
}

/// Below this lip-open ratio the source's mouth counts as closed and is
/// left alone (upstream's `lip_normalize_threshold`).
pub const LIP_NORMALIZE_THRESHOLD: f32 = 0.03;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{self, close};

    fn motion(v: &serde_json::Value, kp: &[f32]) -> Motion {
        let f = |k: &str| v[k].as_f64().unwrap() as f32;
        Motion {
            pitch: f("pitch"),
            yaw: f("yaw"),
            roll: f("roll"),
            t: fixtures::floats(&v["t"]).try_into().unwrap(),
            exp: fixtures::floats(&v["exp"]).try_into().unwrap(),
            scale: f("scale"),
            kp: kp.try_into().unwrap(),
        }
    }

    #[test]
    fn head_angles_are_the_expected_bin_as_upstream_reads_them() {
        let fx = &fixtures::upstream()["headpose"];
        let logits = fixtures::floats(&fx["logits"]);
        let want = fixtures::floats(&fx["degrees"]);
        for (row, want) in logits.chunks(66).zip(want) {
            close(degrees(row), want, 1e-4);
        }
    }

    #[test]
    fn rotation_matches_upstream() {
        let fx = &fixtures::upstream()["rotation"];
        let f = |k: &str| fx[k].as_f64().unwrap() as f32;
        let r = rotation(f("pitch"), f("yaw"), f("roll"));
        let want = fixtures::floats(&fx["matrix"]);
        for (got, want) in r.iter().flatten().zip(want) {
            close(*got, want, 1e-5);
        }
    }

    #[test]
    fn source_keypoints_match_upstreams_transform() {
        let fx = &fixtures::upstream()["transform_keypoint"];
        let m = motion(fx, &fixtures::floats(&fx["kp"]));
        for (got, want) in m.keypoints().iter().zip(fixtures::floats(&fx["x_s"])) {
            close(*got, want, 1e-5);
        }
    }

    #[test]
    fn a_driving_frame_moves_the_source_as_upstream_does() {
        let all = fixtures::upstream();
        let src = &all["transform_keypoint"];
        let kp = fixtures::floats(&src["kp"]);
        let source = motion(src, &kp);
        let fx = &all["relative_motion"];
        let first = motion(&fx["first"], &kp);
        let frame = motion(&fx["frame"], &kp);
        let got = source.driven(&first, &frame);
        for (got, want) in got.iter().zip(fixtures::floats(&fx["x_d"])) {
            close(*got, want, 1e-5);
        }
    }

    #[test]
    fn the_first_driving_frame_leaves_the_source_where_it_is() {
        let all = fixtures::upstream();
        let src = &all["transform_keypoint"];
        let source = motion(src, &fixtures::floats(&src["kp"]));
        let first = motion(&all["relative_motion"]["first"], &source.kp);
        let got = source.driven(&first, &first);
        for (got, want) in got.iter().zip(source.keypoints()) {
            close(*got, want, 1e-5);
        }
    }

    #[test]
    fn stitching_applies_like_upstream() {
        let fx = &fixtures::upstream()["stitching"];
        let kp: Keypoints = fixtures::floats(&fx["kp_driving"]).try_into().unwrap();
        let got = stitch(&kp, &fixtures::floats(&fx["delta"]));
        // The fixture keeps six decimals of each number.
        for (got, want) in got.iter().zip(fixtures::floats(&fx["stitched"])) {
            close(*got, want, 1e-5);
        }
    }

    #[test]
    fn network_inputs_put_source_first() {
        let a = [1.0; COORDS];
        let b = [2.0; COORDS];
        let s = stitching_input(&a, &b);
        assert_eq!(
            (s[0], s[COORDS - 1], s[COORDS], s[2 * COORDS - 1]),
            (1.0, 1.0, 2.0, 2.0)
        );
        let l = lip_input(&a, 0.2);
        assert_eq!((l[COORDS - 1], l[COORDS], l[COORDS + 1]), (1.0, 0.2, 0.0));
    }

    #[test]
    fn the_threshold_is_upstreams() {
        let fx = &fixtures::upstream()["config"];
        close(
            LIP_NORMALIZE_THRESHOLD,
            fx["lip_normalize_threshold"].as_f64().unwrap() as f32,
            0.0,
        );
    }
}
