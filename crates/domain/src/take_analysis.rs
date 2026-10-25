//! Local analysis of a capture's takes (#190): the mode a take goes to its
//! take model in, the ports of the vision models that run on the
//! computer, the face mesh file (`face_landmarks.json`), the depth
//! normalization and the checks the free review shows.

use std::time::{Duration, SystemTime};

use crate::{CaptureId, JobId, VideoProjectId};

/// What the take model receives for a take (the spec's control video
/// modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TakeMode {
    /// The take as recorded.
    #[default]
    Recording,
    /// The take with the face covered by its drawn mesh.
    Mesh,
    /// The performer as a depth figure over the set.
    Depth,
}

impl TakeMode {
    pub const ALL: [TakeMode; 3] = [TakeMode::Recording, TakeMode::Mesh, TakeMode::Depth];

    pub fn code(self) -> &'static str {
        match self {
            TakeMode::Recording => "recording",
            TakeMode::Mesh => "mesh",
            TakeMode::Depth => "depth",
        }
    }

    pub fn from_code(code: &str) -> Option<TakeMode> {
        TakeMode::ALL.into_iter().find(|mode| mode.code() == code)
    }

    /// The analyses the mode needs: the face mesh always, depth and the
    /// person mask in Depth mode.
    pub fn parts(self) -> &'static [AnalysisPart] {
        match self {
            TakeMode::Recording | TakeMode::Mesh => &[AnalysisPart::Face],
            TakeMode::Depth => &[AnalysisPart::Face, AnalysisPart::Depth],
        }
    }
}

/// One of a take's two analyses, each a job of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AnalysisPart {
    /// The face mesh, with the person mask only where a small face is
    /// searched for or faces are counted.
    Face,
    /// The depth map and the person mask on every frame.
    Depth,
}

impl AnalysisPart {
    pub const ALL: [AnalysisPart; 2] = [AnalysisPart::Face, AnalysisPart::Depth];

    pub fn code(self) -> &'static str {
        match self {
            AnalysisPart::Face => "face",
            AnalysisPart::Depth => "depth",
        }
    }

    pub fn from_code(code: &str) -> Option<AnalysisPart> {
        AnalysisPart::ALL
            .into_iter()
            .find(|part| part.code() == code)
    }
}

/// Points in MediaPipe's face mesh.
pub const FACE_LANDMARKS: usize = 478;

/// MediaPipe Face Landmarker's 52 blendshapes, in the model's order.
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

/// A face shorter than this share of the frame's height is flagged: too
/// small for the mesh to follow the mouth and eyes.
pub const MIN_FACE_HEIGHT: f32 = 0.15;

/// A head turned further than this from the camera, either way, is
/// flagged.
pub const MAX_HEAD_TURN_DEGREES: f32 = 45.0;

/// In Depth mode, a person mask whose area changes by more than this share
/// from one frame to the next is flagged: the figure would jump.
pub const MAX_MASK_CHANGE: f32 = 0.20;

/// How often faces are counted in each person region, so "one person
/// only" also holds for faces too small for the whole-frame detector.
pub const REGION_COUNT_EVERY: Duration = Duration::from_secs(1);

/// Depth is normalized over the whole take between these percentiles
/// (thousandths): far at the low one, near at the high one.
pub const DEPTH_FAR_PERMILLE: u32 = 20;
pub const DEPTH_NEAR_PERMILLE: u32 = 980;

/// A decoded video frame: 8-bit RGB, rows top to bottom, no padding.
#[derive(Clone, PartialEq, Eq)]
pub struct RgbFrame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl std::fmt::Debug for RgbFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RgbFrame")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl RgbFrame {
    /// A black frame.
    pub fn black(width: u32, height: u32) -> RgbFrame {
        RgbFrame {
            width,
            height,
            rgb: vec![0; width as usize * height as usize * 3],
        }
    }
}

/// An upright rectangle in shares of the frame: its center and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Region {
    /// The share of the union the two cover together.
    pub fn overlap(&self, other: &Region) -> f32 {
        let (a0, a1) = (self.x - self.width / 2.0, self.x + self.width / 2.0);
        let (b0, b1) = (other.x - other.width / 2.0, other.x + other.width / 2.0);
        let (c0, c1) = (self.y - self.height / 2.0, self.y + self.height / 2.0);
        let (d0, d1) = (other.y - other.height / 2.0, other.y + other.height / 2.0);
        let w = (a1.min(b1) - a0.max(b0)).max(0.0);
        let h = (c1.min(d1) - c0.max(d0)).max(0.0);
        let inter = w * h;
        let union = self.width * self.height + other.width * other.height - inter;
        if union <= 0.0 { 0.0 } else { inter / union }
    }
}

/// Faces found in several regions of one frame, counted once each: a
/// face seen from two overlapping regions is one face.
pub fn distinct_faces(faces: &[Region]) -> usize {
    let mut kept: Vec<Region> = Vec::new();
    for face in faces {
        if kept.iter().all(|other| other.overlap(face) <= 0.3) {
            kept.push(*face);
        }
    }
    kept.len()
}

/// One face's mesh in one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceMesh {
    /// [`FACE_LANDMARKS`] points: x and y from 0 to 1 across the frame, z
    /// in the units of x (nearer is smaller).
    pub landmarks: Vec<[f32; 3]>,
    /// Scores from 0 to 1, in [`BLENDSHAPE_NAMES`] order.
    pub blendshapes: [f32; 52],
}

/// MediaPipe's mesh points the head turn is read from: the cheeks at the
/// face's outline, left and right on screen.
const LEFT_CHEEK: usize = 234;
const RIGHT_CHEEK: usize = 454;

impl FaceMesh {
    /// The face's height as a share of the frame's.
    pub fn height(&self) -> f32 {
        let (low, high) = self
            .landmarks
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), p| {
                (low.min(p[1]), high.max(p[1]))
            });
        (high - low).max(0.0)
    }

    /// How far the head is turned from the camera, left or right, in
    /// degrees: the angle of the line between the cheeks out of the
    /// picture's plane. x and z are both in shares of the frame's width.
    pub fn turn_degrees(&self) -> f32 {
        let (Some(left), Some(right)) = (
            self.landmarks.get(LEFT_CHEEK),
            self.landmarks.get(RIGHT_CHEEK),
        ) else {
            return 0.0;
        };
        let dx = (right[0] - left[0]).abs();
        let dz = right[2] - left[2];
        dz.abs().atan2(dx).to_degrees()
    }
}

/// What the face tracker saw in one frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FaceObservation {
    /// The face it follows, if any.
    pub mesh: Option<FaceMesh>,
    /// How many faces the detector found, when it ran on the frame.
    pub detected: Option<u32>,
}

/// The person segmenter's classes, by index.
pub const MASK_BACKGROUND: u8 = 0;
pub const MASK_FACE_SKIN: u8 = 3;

/// A patch of face skin with fewer cells is ignored (a stray cell, not a
/// face).
const MIN_SKIN_CELLS: usize = 4;

/// Per cell of a grid stretched over the frame, how likely it is the
/// performer and which class it most likely is.
#[derive(Clone, PartialEq, Eq)]
pub struct PersonMask {
    pub width: u32,
    pub height: u32,
    /// 0 to 255.
    pub person: Vec<u8>,
    /// [`MASK_BACKGROUND`], hair, body skin, [`MASK_FACE_SKIN`], clothes,
    /// other.
    pub class: Vec<u8>,
}

impl std::fmt::Debug for PersonMask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersonMask")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl PersonMask {
    fn is_person(&self, cell: usize) -> bool {
        self.person[cell] >= 128
    }

    /// The share of the cells that are the performer.
    pub fn area(&self) -> f32 {
        let cells = self.person.len().max(1);
        let person = (0..self.person.len())
            .filter(|&c| self.is_person(c))
            .count();
        person as f32 / cells as f32
    }

    /// The person probability at a point given in shares of the frame,
    /// bilinear between cell centers.
    pub fn person_at(&self, x: f32, y: f32) -> f32 {
        let (w, h) = (self.width as usize, self.height as usize);
        if w == 0 || h == 0 {
            return 0.0;
        }
        let fx = (x * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
        let fy = (y * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
        let p = |x: usize, y: usize| f32::from(self.person[y * w + x]) / 255.0;
        (p(x0, y0) * (1.0 - ax) + p(x1, y0) * ax) * (1.0 - ay)
            + (p(x0, y1) * (1.0 - ax) + p(x1, y1) * ax) * ay
    }

    /// The mask resampled to `width` × `height`, person probability 0 to
    /// 255 per pixel: what the Depth mode's mask file holds.
    pub fn resized(&self, width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(width as usize * height as usize);
        for v in 0..height {
            for u in 0..width {
                let x = (u as f32 + 0.5) / width as f32;
                let y = (v as f32 + 0.5) / height as f32;
                out.push((self.person_at(x, y) * 255.0).round() as u8);
            }
        }
        out
    }

    /// Around each patch of face skin, largest first, a square three
    /// times its size, in shares of a `frame_width` × `frame_height`
    /// frame: where to look for a face too small for the detector in the
    /// whole frame, and where to count faces one by one. Two people side by
    /// side make one person region but two patches of face skin.
    pub fn head_regions(&self, frame_width: u32, frame_height: u32) -> Vec<Region> {
        let (w, h) = (self.width as usize, self.height as usize);
        let skin = |cell: usize| self.is_person(cell) && self.class[cell] == MASK_FACE_SKIN;
        let mut seen = vec![false; w * h];
        let mut regions = Vec::new();
        for start in 0..w * h {
            if seen[start] || !skin(start) {
                continue;
            }
            // One 4-connected patch.
            let mut stack = vec![start];
            seen[start] = true;
            let (mut x0, mut y0, mut x1, mut y1, mut size) = (w, h, 0, 0, 0usize);
            while let Some(cell) = stack.pop() {
                size += 1;
                let (x, y) = (cell % w, cell / w);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                let mut visit = |next: usize| {
                    if !seen[next] && skin(next) {
                        seen[next] = true;
                        stack.push(next);
                    }
                };
                if x > 0 {
                    visit(cell - 1);
                }
                if x + 1 < w {
                    visit(cell + 1);
                }
                if y > 0 {
                    visit(cell - w);
                }
                if y + 1 < h {
                    visit(cell + w);
                }
            }
            if size < MIN_SKIN_CELLS {
                continue;
            }
            let cx = (x0 + x1 + 1) as f32 / 2.0 / w as f32;
            let cy = (y0 + y1 + 1) as f32 / 2.0 / h as f32;
            let fw = (x1 + 1 - x0) as f32 / w as f32 * frame_width as f32;
            let fh = (y1 + 1 - y0) as f32 / h as f32 * frame_height as f32;
            let side = fw.max(fh) * 3.0;
            regions.push((
                size,
                Region {
                    x: cx,
                    y: cy,
                    width: side / frame_width as f32,
                    height: side / frame_height as f32,
                },
            ));
        }
        regions.sort_by_key(|region| std::cmp::Reverse(region.0));
        regions.into_iter().map(|(_, region)| region).collect()
    }
}

/// Relative inverse depth for one frame (larger is nearer), at the depth
/// model's size.
#[derive(Clone, PartialEq)]
pub struct DepthMap {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}

impl std::fmt::Debug for DepthMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DepthMap")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// Where a vision model runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Device {
    /// The graphics card, through DirectML.
    Gpu,
    /// The processor; with why, when the graphics card was asked for and
    /// could not run it.
    Cpu { fallback: Option<String> },
}

impl Device {
    /// The slower of two: a run on the graphics card and the processor
    /// counts as the processor's.
    pub fn slower(self, other: Device) -> Device {
        match (self, other) {
            (Device::Gpu, Device::Gpu) => Device::Gpu,
            (Device::Cpu { fallback }, Device::Gpu) | (Device::Gpu, Device::Cpu { fallback }) => {
                Device::Cpu { fallback }
            }
            (Device::Cpu { fallback: a }, Device::Cpu { fallback: b }) => {
                Device::Cpu { fallback: a.or(b) }
            }
        }
    }
}

/// One file of a model, pinned by its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    pub file: String,
    pub sha256: String,
}

/// A model, as `face_landmarks.json` and the analysis record name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub name: String,
    pub version: String,
    pub files: Vec<ModelFile>,
}

impl ModelInfo {
    /// `MediaPipe Face Landmarker float16/1`.
    pub fn label(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

/// Why a vision model did not run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VisionError {
    /// A model file is not where Bardo keeps them, or is not the pinned
    /// one.
    #[error("the vision model {0} is missing or damaged")]
    Missing(String),
    /// ONNX Runtime failed.
    #[error("the vision model failed: {0}")]
    Failed(String),
    /// A saved state from an earlier run does not fit.
    #[error("the saved analysis state does not fit: {0}")]
    State(String),
}

/// Follows one face through a take's frames: the face mesh and its
/// blendshapes, and how many faces the detector finds. Stateful: frames
/// come in order.
pub trait FaceTracker: Send {
    fn model(&self) -> &ModelInfo;
    fn device(&self) -> &Device;

    /// The face in `frame`, `at` from the take's start (for smoothing
    /// over time). When the whole frame shows none and none is followed,
    /// `head` is asked where to look again (around the person mask's
    /// face, for a small face).
    fn track(
        &mut self,
        frame: &RgbFrame,
        at: Duration,
        head: &mut dyn FnMut() -> Result<Option<Region>, VisionError>,
    ) -> Result<FaceObservation, VisionError>;

    /// The faces the detector finds inside `region` of `frame`, in shares
    /// of the frame.
    fn detect_in(&mut self, frame: &RgbFrame, region: &Region) -> Result<Vec<Region>, VisionError>;

    /// What it carries from frame to frame, to resume after the last frame
    /// it saw.
    fn snapshot(&self) -> Vec<u8>;
    fn restore(&mut self, snapshot: &[u8]) -> Result<(), VisionError>;
}

/// Tells the performer from the background, frame by frame.
pub trait PersonSegmenter: Send {
    fn model(&self) -> &ModelInfo;
    fn device(&self) -> &Device;
    fn segment(&mut self, frame: &RgbFrame) -> Result<PersonMask, VisionError>;
}

/// Depth over a take's frames, smoothed in time. Frames come in order; a
/// frame's depth may come out some frames later.
pub trait DepthEstimator: Send {
    fn model(&self) -> &ModelInfo;
    fn device(&self) -> &Device;

    /// The depth maps' size.
    fn size(&self) -> (u32, u32);

    /// Adds a frame; returns the depth of the frames that became final, in
    /// order.
    fn push(&mut self, frame: &RgbFrame) -> Result<Vec<DepthMap>, VisionError>;

    /// The depth of the frames still waiting, after the last frame.
    fn finish(&mut self) -> Result<Vec<DepthMap>, VisionError>;

    fn snapshot(&self) -> Vec<u8>;
    fn restore(&mut self, snapshot: &[u8]) -> Result<(), VisionError>;
}

/// Loads the vision models. Shared with job worker threads.
pub trait VisionModels: Send + Sync {
    fn face_tracker(&self) -> Result<Box<dyn FaceTracker>, VisionError>;
    fn person_segmenter(&self) -> Result<Box<dyn PersonSegmenter>, VisionError>;
    /// For a take taller than wide when `vertical`.
    fn depth_estimator(&self, vertical: bool) -> Result<Box<dyn DepthEstimator>, VisionError>;
}

/// What the preflight checks read from a take's face mesh (#243), counted
/// beside the other checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FaceMeasures {
    /// The face heights of the frames with a face, in thousandths of the
    /// frame's height, summed.
    pub height_sum: u64,
    /// Frames whose head or upper body the frame cuts off: the face within
    /// [`EDGE_MARGIN`] of an edge, or less than its own height of room
    /// below the chin.
    pub cut_off: u32,
    /// Frames whose face moved faster than [`MAX_FACE_SPEED`] since the
    /// frame before.
    pub fast: u32,
}

/// A face this close to an edge of the frame, as a share of it, may be cut
/// off.
pub const EDGE_MARGIN: f32 = 0.02;

/// A face moving faster than this many face heights a second moves too fast
/// for a motion-control model to follow.
pub const MAX_FACE_SPEED: f32 = 2.0;

/// What the free review shows about a take, counted in frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TakeChecks {
    pub frames: u32,
    /// Frames with no face.
    pub no_face: u32,
    /// Frames with more than one face: the take cannot go on.
    pub several_faces: u32,
    /// Frames whose face is under [`MIN_FACE_HEIGHT`].
    pub small_face: u32,
    /// Frames whose head is turned past [`MAX_HEAD_TURN_DEGREES`].
    pub turned: u32,
    /// Depth mode: frames whose person mask changed by more than
    /// [`MAX_MASK_CHANGE`] from the frame before.
    pub mask_jumps: u32,
    /// The face analysis' measures for the preflight checks; `None` for a
    /// take analyzed before they were counted.
    pub measures: Option<FaceMeasures>,
}

/// The face's box in shares of the frame: left, top, right, bottom.
fn face_box(mesh: &FaceMesh) -> Option<[f32; 4]> {
    let mut found = None::<[f32; 4]>;
    for point in mesh
        .landmarks
        .iter()
        .filter(|point| point[0].is_finite() && point[1].is_finite())
    {
        let bounds = found.get_or_insert([point[0], point[1], point[0], point[1]]);
        bounds[0] = bounds[0].min(point[0]);
        bounds[1] = bounds[1].min(point[1]);
        bounds[2] = bounds[2].max(point[0]);
        bounds[3] = bounds[3].max(point[1]);
    }
    found
}

/// Whether the frame cuts off the head or the upper body of `mesh`'s
/// person.
fn cut_off(mesh: &FaceMesh) -> bool {
    let Some([left, top, right, bottom]) = face_box(mesh) else {
        return true;
    };
    let height = bottom - top;
    left < EDGE_MARGIN || top < EDGE_MARGIN || right > 1.0 - EDGE_MARGIN || 1.0 - bottom < height
}

impl TakeChecks {
    /// Counts one frame of the face analysis: what the tracker saw and the
    /// most faces any count found in it (the whole frame's detector, or
    /// the person regions').
    pub fn add_face_frame(&mut self, mesh: Option<&FaceMesh>, faces: usize) {
        // Measures count from a take's first frame, or not at all (a take
        // whose analysis began before they were counted).
        if self.frames == 0 {
            self.measures.get_or_insert_with(FaceMeasures::default);
        }
        if let (Some(measures), Some(mesh)) = (self.measures.as_mut(), mesh) {
            measures.height_sum += (mesh.height().clamp(0.0, 1.0) * 1_000.0).round() as u64;
            if cut_off(mesh) {
                measures.cut_off += 1;
            }
        }
        self.frames += 1;
        match mesh {
            None => self.no_face += 1,
            Some(mesh) => {
                if mesh.height() < MIN_FACE_HEIGHT {
                    self.small_face += 1;
                }
                if mesh.turn_degrees() > MAX_HEAD_TURN_DEGREES {
                    self.turned += 1;
                }
            }
        }
        if faces > 1 {
            self.several_faces += 1;
        }
    }

    /// Counts how fast the face moved from `previous` to `mesh`, the frames
    /// `interval` apart, in a picture `aspect` (width over height) wide:
    /// landmarks are shares of each side, so a move across is scaled to the
    /// height before it is measured in face heights.
    pub fn add_face_motion(
        &mut self,
        previous: &FaceMesh,
        mesh: &FaceMesh,
        interval: Duration,
        aspect: f32,
    ) {
        let Some(measures) = self.measures.as_mut() else {
            return;
        };
        let (Some(before), Some(now)) = (face_box(previous), face_box(mesh)) else {
            return;
        };
        let center = |b: [f32; 4]| ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        let ((x0, y0), (x1, y1)) = (center(before), center(now));
        let height = (now[3] - now[1]).max(before[3] - before[1]);
        let seconds = interval.as_secs_f32();
        if height <= 0.0 || seconds <= 0.0 || !aspect.is_finite() || aspect <= 0.0 {
            return;
        }
        let speed = (((x1 - x0) * aspect).hypot(y1 - y0) / height) / seconds;
        if speed > MAX_FACE_SPEED {
            measures.fast += 1;
        }
    }

    /// The frames with a face.
    pub fn face_frames(&self) -> u32 {
        self.frames.saturating_sub(self.no_face)
    }

    /// The face's average height over the frames with one, as a share of
    /// the frame's; `None` without measures or a face.
    pub fn mean_face_height(&self) -> Option<f32> {
        let measures = self.measures?;
        let faces = self.face_frames();
        (faces > 0).then(|| measures.height_sum as f32 / 1_000.0 / faces as f32)
    }

    /// Counts one frame of the depth analysis by its mask's area and the
    /// one before's.
    pub fn add_mask_frame(&mut self, previous: Option<f32>, area: f32) {
        self.frames += 1;
        if let Some(previous) = previous
            && mask_jumped(previous, area)
        {
            self.mask_jumps += 1;
        }
    }

    /// Whether the take cannot go on.
    pub fn blocked(&self) -> bool {
        self.several_faces > 0
    }

    /// Adds `other`'s counts (a face analysis and a depth analysis of the
    /// same take).
    pub fn merged(mut self, other: &TakeChecks) -> TakeChecks {
        self.frames = self.frames.max(other.frames);
        self.no_face += other.no_face;
        self.several_faces += other.several_faces;
        self.small_face += other.small_face;
        self.turned += other.turned;
        self.mask_jumps += other.mask_jumps;
        self.measures = self.measures.or(other.measures);
        self
    }
}

/// Whether the mask's area moved by more than [`MAX_MASK_CHANGE`] of the
/// larger of the two.
fn mask_jumped(previous: f32, area: f32) -> bool {
    let base = previous.max(area);
    base > 0.0 && (area - previous).abs() / base > MAX_MASK_CHANGE
}

/// Frames a take is analyzed in at a time: a checkpoint after each, so a
/// resumed analysis starts at the last finished chunk.
pub const ANALYSIS_CHUNK_FRAMES: u32 = 96;

/// The frame rate a recording of `fps_milli` is analyzed at: its own, up
/// to 30 frames a second; faster recordings at half (or a quarter) of
/// theirs, so 59.94 is analyzed at 29.97.
pub fn analysis_fps(fps_milli: u32) -> u32 {
    let mut fps = fps_milli.max(1);
    while fps > 30_000 {
        fps /= 2;
    }
    fps
}

/// How many frames a take of `length` has at `fps_milli` thousandths of a
/// frame a second: those whose start falls inside it.
pub fn take_frames(length: Duration, fps_milli: u32) -> u32 {
    let micros = length.as_micros() as u64;
    // ceil(length × fps), in whole frames.
    (micros * u64::from(fps_milli)).div_ceil(1_000_000_000) as u32
}

/// When frame `index` of a take starts, from the take's start.
pub fn take_frame_time(index: u32, fps_milli: u32) -> Duration {
    Duration::from_micros(u64::from(index) * 1_000_000_000 / u64::from(fps_milli.max(1)))
}

/// Raw depth mapped into 16 bits by a fixed scale, set from the first
/// depth the take gives with room on each side, so later frames that
/// reach nearer or farther still fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthScale {
    pub low: f32,
    pub high: f32,
}

impl DepthScale {
    /// The range of `values`, widened by itself on each side.
    pub fn around(values: &[f32]) -> DepthScale {
        let (low, high) = values
            .iter()
            .filter(|v| v.is_finite())
            .fold((f32::MAX, f32::MIN), |(low, high), &v| {
                (low.min(v), high.max(v))
            });
        if low > high {
            return DepthScale {
                low: 0.0,
                high: 1.0,
            };
        }
        let span = (high - low).max(1e-6);
        DepthScale {
            low: low - span,
            high: high + span,
        }
    }

    pub fn quantize(&self, value: f32) -> u16 {
        let share = (value - self.low) / (self.high - self.low);
        if share.is_nan() {
            return 0;
        }
        (share.clamp(0.0, 1.0) * 65535.0).round() as u16
    }
}

/// How often each 16-bit depth value occurs over a take, for its
/// percentiles.
#[derive(Clone, PartialEq, Eq)]
pub struct DepthHistogram(Vec<u32>);

impl std::fmt::Debug for DepthHistogram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DepthHistogram")
            .field("count", &self.count())
            .finish()
    }
}

impl Default for DepthHistogram {
    fn default() -> Self {
        DepthHistogram(vec![0; 65536])
    }
}

impl DepthHistogram {
    pub fn add(&mut self, values: &[u16]) {
        for &value in values {
            self.0[usize::from(value)] = self.0[usize::from(value)].saturating_add(1);
        }
    }

    pub fn count(&self) -> u64 {
        self.0.iter().map(|&n| u64::from(n)).sum()
    }

    /// The smallest value at or under which `permille` thousandths of the
    /// values fall.
    pub fn percentile(&self, permille: u32) -> u16 {
        let total = self.count();
        if total == 0 {
            return 0;
        }
        let wanted = (total * u64::from(permille.min(1000)))
            .div_ceil(1000)
            .max(1);
        let mut seen = 0u64;
        for (value, &n) in self.0.iter().enumerate() {
            seen += u64::from(n);
            if seen >= wanted {
                return value as u16;
            }
        }
        u16::MAX
    }

    /// Little-endian counts, for a saved state.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.iter().flat_map(|n| n.to_le_bytes()).collect()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<DepthHistogram> {
        if bytes.len() != 65536 * 4 {
            return None;
        }
        Some(DepthHistogram(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_le_bytes(*b))
                .collect(),
        ))
    }

    /// The take's near and far, from [`DEPTH_FAR_PERMILLE`] and
    /// [`DEPTH_NEAR_PERMILLE`].
    pub fn range(&self) -> DepthRange {
        DepthRange {
            far: self.percentile(DEPTH_FAR_PERMILLE),
            near: self.percentile(DEPTH_NEAR_PERMILLE),
        }
    }
}

/// A take's depth range: values at `far` and beyond become 0, at `near`
/// and nearer 65535.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthRange {
    pub far: u16,
    pub near: u16,
}

impl DepthRange {
    pub fn normalize(&self, value: u16) -> u16 {
        if self.near <= self.far {
            return if value > self.far { u16::MAX } else { 0 };
        }
        let share =
            (f32::from(value) - f32::from(self.far)) / (f32::from(self.near) - f32::from(self.far));
        (share.clamp(0.0, 1.0) * 65535.0).round() as u16
    }
}

/// One frame of `face_landmarks.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceFrame {
    /// From the take's start.
    pub at: Duration,
    /// The face found, if any (the tracker follows one).
    pub faces: Vec<FaceMesh>,
    /// How many faces the detector found, when it ran.
    pub detected: Option<u32>,
}

/// The recording a face mesh file was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceSource {
    /// The capture's file in the project folder.
    pub file: String,
    pub width: u32,
    pub height: u32,
    pub fps_milli: u32,
    pub frames: u32,
    /// The take's stretch of it.
    pub start: Duration,
    pub end: Duration,
}

/// `face_landmarks.json`'s format version.
pub const FACE_FILE_VERSION: u32 = 1;

/// A take's face mesh: `face_landmarks.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceLandmarks {
    pub source: FaceSource,
    pub model: ModelInfo,
    pub frames: Vec<FaceFrame>,
}

/// Why a face mesh file does not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a face mesh file: {0}")]
pub struct FaceFileError(String);

/// Four decimals: a tenth of a pixel across a 4K frame.
fn number(out: &mut String, value: f32) {
    use std::fmt::Write as _;
    let value = if value.is_finite() { value } else { 0.0 };
    let _ = write!(out, "{:.4}", value);
    // `0.5000` to `0.5`, `-0.0000` to `0`.
    while out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    if out.ends_with("-0") {
        out.truncate(out.len() - 2);
        out.push('0');
    }
}

fn string(out: &mut String, value: &str) {
    out.push_str(&serde_json::Value::String(value.to_owned()).to_string());
}

impl FaceFrame {
    /// The frame as one JSON object, as it sits in the file's `frames`.
    pub fn to_json(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(12_000);
        let _ = write!(out, "{{\"t\":{}", self.at.as_millis());
        if let Some(detected) = self.detected {
            let _ = write!(out, ",\"detected\":{detected}");
        }
        out.push_str(",\"faces\":[");
        for (index, face) in self.faces.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"landmarks\":[");
            for (i, p) in face.landmarks.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('[');
                number(&mut out, p[0]);
                out.push(',');
                number(&mut out, p[1]);
                out.push(',');
                number(&mut out, p[2]);
                out.push(']');
            }
            out.push_str("],\"blendshapes\":{");
            for (i, (name, score)) in BLENDSHAPE_NAMES.iter().zip(face.blendshapes).enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(&mut out, name);
                out.push(':');
                number(&mut out, score);
            }
            out.push_str("}}");
        }
        out.push_str("]}");
        out
    }
}

impl FaceLandmarks {
    /// Where the face is over the take, as fractions of the frame across
    /// and down: the median, over the frames with a face, of the center of
    /// its mesh's bounding box. `None` when no frame has one.
    pub fn face_center(&self) -> Option<(f32, f32)> {
        let (mut xs, mut ys): (Vec<f32>, Vec<f32>) = self
            .frames
            .iter()
            .filter_map(|frame| frame.faces.first())
            .filter_map(|face| {
                let finite = face
                    .landmarks
                    .iter()
                    .filter(|point| point[0].is_finite() && point[1].is_finite());
                let (mut low, mut high) = ([f32::MAX; 2], [f32::MIN; 2]);
                let mut any = false;
                for point in finite {
                    any = true;
                    for axis in 0..2 {
                        low[axis] = low[axis].min(point[axis]);
                        high[axis] = high[axis].max(point[axis]);
                    }
                }
                any.then(|| ((low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0))
            })
            .unzip();
        let median = |values: &mut Vec<f32>| {
            values.sort_by(f32::total_cmp);
            values[values.len() / 2].clamp(0.0, 1.0)
        };
        (!xs.is_empty()).then(|| (median(&mut xs), median(&mut ys)))
    }

    /// The file, with `frames` already written one JSON object each
    /// ([`FaceFrame::to_json`]), so a long take is put together from its
    /// chunks without reading them back.
    pub fn json_from_frames<'a>(
        source: &FaceSource,
        model: &ModelInfo,
        frames: impl IntoIterator<Item = &'a str>,
    ) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = write!(
            out,
            "{{\"version\":{FACE_FILE_VERSION},\"source\":{{\"file\":"
        );
        string(&mut out, &source.file);
        let _ = write!(
            out,
            ",\"width\":{},\"height\":{},\"fps\":",
            source.width, source.height
        );
        number(&mut out, source.fps_milli as f32 / 1000.0);
        let _ = write!(
            out,
            ",\"frames\":{},\"start_ms\":{},\"end_ms\":{}}},\"model\":{{\"name\":",
            source.frames,
            source.start.as_millis(),
            source.end.as_millis()
        );
        string(&mut out, &model.name);
        out.push_str(",\"version\":");
        string(&mut out, &model.version);
        out.push_str(",\"files\":[");
        for (index, file) in model.files.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"file\":");
            string(&mut out, &file.file);
            out.push_str(",\"sha256\":");
            string(&mut out, &file.sha256);
            out.push('}');
        }
        out.push_str("]},\"blendshape_names\":[");
        for (index, name) in BLENDSHAPE_NAMES.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            string(&mut out, name);
        }
        out.push_str("],\"frames\":[\n");
        for (index, frame) in frames.into_iter().enumerate() {
            if index > 0 {
                out.push_str(",\n");
            }
            out.push_str(frame);
        }
        out.push_str("\n]}\n");
        out
    }

    pub fn to_json(&self) -> String {
        let frames: Vec<String> = self.frames.iter().map(FaceFrame::to_json).collect();
        Self::json_from_frames(&self.source, &self.model, frames.iter().map(String::as_str))
    }

    /// Reads a file in this format (version [`FACE_FILE_VERSION`]),
    /// checking every face has [`FACE_LANDMARKS`] points and 52 scores.
    pub fn from_json(json: &str) -> Result<FaceLandmarks, FaceFileError> {
        use serde_json::Value;
        let bad = |what: &str| FaceFileError(what.to_owned());
        let root: Value = serde_json::from_str(json).map_err(|e| FaceFileError(e.to_string()))?;
        if root["version"].as_u64() != Some(u64::from(FACE_FILE_VERSION)) {
            return Err(bad("unknown version"));
        }
        let uint = |value: &Value, what: &str| -> Result<u32, FaceFileError> {
            value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| bad(what))
        };
        let text = |value: &Value, what: &str| -> Result<String, FaceFileError> {
            value.as_str().map(str::to_owned).ok_or_else(|| bad(what))
        };
        let s = &root["source"];
        let source = FaceSource {
            file: text(&s["file"], "source file")?,
            width: uint(&s["width"], "source width")?,
            height: uint(&s["height"], "source height")?,
            fps_milli: s["fps"]
                .as_f64()
                .map(|fps| (fps * 1000.0).round() as u32)
                .ok_or_else(|| bad("source fps"))?,
            frames: uint(&s["frames"], "source frames")?,
            start: Duration::from_millis(u64::from(uint(&s["start_ms"], "source start")?)),
            end: Duration::from_millis(u64::from(uint(&s["end_ms"], "source end")?)),
        };
        let m = &root["model"];
        let model = ModelInfo {
            name: text(&m["name"], "model name")?,
            version: text(&m["version"], "model version")?,
            files: m["files"]
                .as_array()
                .ok_or_else(|| bad("model files"))?
                .iter()
                .map(|file| {
                    Ok(ModelFile {
                        file: text(&file["file"], "model file")?,
                        sha256: text(&file["sha256"], "model sha256")?,
                    })
                })
                .collect::<Result<_, FaceFileError>>()?,
        };
        let mut frames = Vec::new();
        for frame in root["frames"].as_array().ok_or_else(|| bad("frames"))? {
            let mut faces = Vec::new();
            for face in frame["faces"].as_array().ok_or_else(|| bad("faces"))? {
                let points = face["landmarks"]
                    .as_array()
                    .ok_or_else(|| bad("landmarks"))?;
                if points.len() != FACE_LANDMARKS {
                    return Err(bad("landmark count"));
                }
                let mut landmarks = Vec::with_capacity(FACE_LANDMARKS);
                for point in points {
                    let xyz = point.as_array().filter(|p| p.len() == 3);
                    let xyz = xyz.ok_or_else(|| bad("landmark"))?;
                    let coordinate = |i: usize| {
                        xyz[i]
                            .as_f64()
                            .map(|v| v as f32)
                            .ok_or_else(|| bad("landmark"))
                    };
                    landmarks.push([coordinate(0)?, coordinate(1)?, coordinate(2)?]);
                }
                let scores = face["blendshapes"]
                    .as_object()
                    .ok_or_else(|| bad("blendshapes"))?;
                let mut blendshapes = [0.0; 52];
                for (slot, name) in blendshapes.iter_mut().zip(BLENDSHAPE_NAMES) {
                    *slot = scores
                        .get(name)
                        .and_then(Value::as_f64)
                        .ok_or_else(|| bad("blendshape"))? as f32;
                }
                faces.push(FaceMesh {
                    landmarks,
                    blendshapes,
                });
            }
            frames.push(FaceFrame {
                at: Duration::from_millis(frame["t"].as_u64().ok_or_else(|| bad("frame time"))?),
                faces,
                detected: frame["detected"].as_u64().map(|n| n as u32),
            });
        }
        Ok(FaceLandmarks {
            source,
            model,
            frames,
        })
    }
}

/// A finished analysis of one take, kept until the take or its capture
/// changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeAnalysis {
    pub project: VideoProjectId,
    /// The capture and the take's revision it was made from: another one
    /// makes it stale.
    pub capture: CaptureId,
    pub position: u32,
    pub revision: u32,
    pub part: AnalysisPart,
    pub device: Device,
    /// [`ModelInfo::label`] of the models that ran.
    pub model: String,
    pub checks: TakeChecks,
    /// Depth only: the take's range, near and far.
    pub depth_range: Option<DepthRange>,
    pub finished_at: SystemTime,
    pub job: Option<JobId>,
}

impl TakeAnalysis {
    /// Whether it was made from `capture`'s take at `position` as it is
    /// now, at `revision`.
    pub fn fits(&self, capture: CaptureId, position: u32, revision: u32) -> bool {
        self.capture == capture && self.position == position && self.revision == revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(height: f32, turn: f32) -> FaceMesh {
        let mut landmarks = vec![[0.5, 0.5, 0.0]; FACE_LANDMARKS];
        landmarks[0][1] = 0.5 - height / 2.0;
        landmarks[1][1] = 0.5 + height / 2.0;
        // Cheeks 0.2 apart across, turned by `turn` degrees out of plane.
        let dz = 0.2 * turn.to_radians().tan();
        landmarks[LEFT_CHEEK] = [0.4, 0.5, 0.0];
        landmarks[RIGHT_CHEEK] = [0.6, 0.5, dz];
        FaceMesh {
            landmarks,
            blendshapes: [0.0; 52],
        }
    }

    #[test]
    fn fast_recordings_are_analyzed_at_half_their_rate() {
        assert_eq!(analysis_fps(30_000), 30_000);
        assert_eq!(analysis_fps(29_970), 29_970);
        assert_eq!(analysis_fps(24_000), 24_000);
        assert_eq!(analysis_fps(59_940), 29_970);
        assert_eq!(analysis_fps(60_000), 30_000);
        assert_eq!(analysis_fps(120_000), 30_000);
    }

    #[test]
    fn modes_and_parts_round_trip_their_codes() {
        for mode in TakeMode::ALL {
            assert_eq!(TakeMode::from_code(mode.code()), Some(mode));
        }
        for part in AnalysisPart::ALL {
            assert_eq!(AnalysisPart::from_code(part.code()), Some(part));
        }
        assert_eq!(TakeMode::default(), TakeMode::Recording);
        assert_eq!(TakeMode::Mesh.parts(), &[AnalysisPart::Face]);
        assert_eq!(
            TakeMode::Depth.parts(),
            &[AnalysisPart::Face, AnalysisPart::Depth]
        );
    }

    #[test]
    fn face_height_and_turn_come_from_the_mesh() {
        let face = mesh(0.3, 30.0);
        assert!((face.height() - 0.3).abs() < 1e-6);
        assert!((face.turn_degrees() - 30.0).abs() < 0.01);
        // Either way.
        assert!((mesh(0.3, -50.0).turn_degrees() - 50.0).abs() < 0.01);
    }

    #[test]
    fn checks_count_each_condition_per_frame() {
        let mut checks = TakeChecks::default();
        checks.add_face_frame(Some(&mesh(0.3, 10.0)), 1);
        checks.add_face_frame(None, 0);
        checks.add_face_frame(Some(&mesh(0.10, 10.0)), 1);
        checks.add_face_frame(Some(&mesh(0.3, 60.0)), 2);
        assert_eq!(
            checks,
            TakeChecks {
                frames: 4,
                no_face: 1,
                several_faces: 1,
                small_face: 1,
                turned: 1,
                mask_jumps: 0,
                measures: Some(FaceMeasures {
                    height_sum: 700,
                    cut_off: 0,
                    fast: 0,
                }),
            }
        );
        assert!(checks.blocked());
        let mut fine = TakeChecks::default();
        fine.add_face_frame(Some(&mesh(MIN_FACE_HEIGHT + 0.01, 44.0)), 1);
        assert!(!fine.blocked());
        assert_eq!((fine.small_face, fine.turned), (0, 0));
    }

    /// A face `height` tall, centred on `x`, `y`.
    fn mesh_at(x: f32, y: f32, height: f32) -> FaceMesh {
        let mut face = mesh(height, 0.0);
        for point in &mut face.landmarks {
            point[0] += x - 0.5;
            point[1] += y - 0.5;
        }
        face
    }

    #[test]
    fn a_frame_cuts_off_a_face_at_its_edge_or_without_room_for_the_body() {
        let mut checks = TakeChecks::default();
        // Head and shoulders, room below.
        checks.add_face_frame(Some(&mesh_at(0.5, 0.3, 0.2)), 1);
        // At the left edge.
        checks.add_face_frame(Some(&mesh_at(0.105, 0.3, 0.2)), 1);
        // Too low: less than a face's height below the chin.
        checks.add_face_frame(Some(&mesh_at(0.5, 0.7, 0.3)), 1);
        // Touching the top.
        checks.add_face_frame(Some(&mesh_at(0.5, 0.11, 0.2)), 1);
        let measures = checks.measures.unwrap();
        assert_eq!(measures.cut_off, 3);
        assert_eq!(measures.height_sum, 900);
        assert_eq!(checks.mean_face_height(), Some(0.225));
    }

    #[test]
    fn a_face_moving_over_two_heights_a_second_is_fast() {
        let mut checks = TakeChecks::default();
        let a = mesh_at(0.5, 0.3, 0.2);
        checks.add_face_frame(Some(&a), 1);
        // 0.012 in 1/30 s is 0.36 a second: 1.8 face heights.
        let b = mesh_at(0.512, 0.3, 0.2);
        checks.add_face_motion(&a, &b, Duration::from_millis(33), 1.0);
        assert_eq!(checks.measures.unwrap().fast, 0);
        // 0.02 in 1/30 s: 3 face heights a second.
        let c = mesh_at(0.532, 0.3, 0.2);
        checks.add_face_motion(&b, &c, Duration::from_millis(33), 1.0);
        assert_eq!(checks.measures.unwrap().fast, 1);
        checks.add_face_motion(&b, &c, Duration::ZERO, 1.0);
        assert_eq!(checks.measures.unwrap().fast, 1, "no interval, no speed");
    }

    #[test]
    fn a_move_across_is_measured_against_the_height() {
        // 0.012 of the width in 1/30 s: in a 9:16 picture that is 0.2 a
        // second of its height, one face height; in 16:9, 3.2.
        let a = mesh_at(0.5, 0.3, 0.2);
        let b = mesh_at(0.512, 0.3, 0.2);
        let fast = |aspect: f32| {
            let mut checks = TakeChecks::default();
            checks.add_face_frame(Some(&a), 1);
            checks.add_face_motion(&a, &b, Duration::from_millis(33), aspect);
            checks.measures.unwrap().fast
        };
        assert_eq!(fast(9.0 / 16.0), 0);
        assert_eq!(fast(16.0 / 9.0), 1);
    }

    #[test]
    fn measures_count_only_from_a_takes_first_frame() {
        // An analysis resumed from a state saved before measures existed.
        let mut resumed = TakeChecks {
            frames: 96,
            ..TakeChecks::default()
        };
        resumed.add_face_frame(Some(&mesh(0.3, 0.0)), 1);
        resumed.add_face_motion(
            &mesh(0.3, 0.0),
            &mesh(0.3, 0.0),
            Duration::from_millis(33),
            1.0,
        );
        assert_eq!(resumed.measures, None);
        assert_eq!(resumed.mean_face_height(), None);
        let mut none = TakeChecks::default();
        none.add_face_frame(None, 0);
        assert_eq!(none.mean_face_height(), None, "no face to measure");
        assert_eq!(none.face_frames(), 0);
        // A depth analysis merged in keeps the face's measures.
        let mut face = TakeChecks::default();
        face.add_face_frame(Some(&mesh(0.3, 0.0)), 1);
        let depth = TakeChecks {
            frames: 1,
            ..TakeChecks::default()
        };
        assert_eq!(depth.merged(&face).measures, face.measures);
        assert_eq!(face.merged(&depth).measures, face.measures);
    }

    #[test]
    fn a_mask_jump_is_a_change_past_a_fifth_of_the_larger_area() {
        let mut checks = TakeChecks::default();
        checks.add_mask_frame(None, 0.30);
        checks.add_mask_frame(Some(0.30), 0.25);
        checks.add_mask_frame(Some(0.25), 0.19);
        checks.add_mask_frame(Some(0.19), 0.0);
        checks.add_mask_frame(Some(0.0), 0.0);
        assert_eq!((checks.frames, checks.mask_jumps), (5, 2));
        let merged = TakeChecks {
            frames: 5,
            no_face: 1,
            ..TakeChecks::default()
        }
        .merged(&checks);
        assert_eq!(
            (merged.frames, merged.no_face, merged.mask_jumps),
            (5, 1, 2)
        );
    }

    #[test]
    fn faces_seen_from_overlapping_regions_count_once() {
        let face = |x: f32| Region {
            x,
            y: 0.3,
            width: 0.1,
            height: 0.15,
        };
        assert_eq!(distinct_faces(&[]), 0);
        assert_eq!(distinct_faces(&[face(0.3), face(0.31)]), 1);
        assert_eq!(distinct_faces(&[face(0.3), face(0.7)]), 2);
    }

    fn mask(width: u32, height: u32, cells: &[(u32, u32, u8)]) -> PersonMask {
        let mut m = PersonMask {
            width,
            height,
            person: vec![0; (width * height) as usize],
            class: vec![MASK_BACKGROUND; (width * height) as usize],
        };
        for &(x, y, class) in cells {
            let i = (y * width + x) as usize;
            m.person[i] = 255;
            m.class[i] = class;
        }
        m
    }

    fn block(x0: u32, y0: u32, x1: u32, y1: u32, class: u8) -> Vec<(u32, u32, u8)> {
        let mut out = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                out.push((x, y, class));
            }
        }
        out
    }

    #[test]
    fn each_patch_of_face_skin_gives_a_head_region() {
        // Two people on a 40×20 grid: a large one (body 4, face skin
        // 4×4 at the top) and a small one; a hand (body skin) and a stray
        // cell of face skin.
        let mut cells = block(2, 6, 14, 20, 4);
        cells.extend(block(6, 2, 10, 6, MASK_FACE_SKIN));
        cells.extend(block(28, 10, 32, 20, 4));
        cells.extend(block(29, 8, 31, 10, MASK_FACE_SKIN));
        cells.extend(block(20, 15, 23, 18, 2));
        cells.push((36, 2, MASK_FACE_SKIN));
        let m = mask(40, 20, &cells);
        let regions = m.head_regions(400, 200);
        assert_eq!(regions.len(), 2, "{regions:?}");
        let big = regions[0];
        assert!((big.x - 8.0 / 40.0).abs() < 1e-6 && (big.y - 4.0 / 20.0).abs() < 1e-6);
        // Face 40 px wide and 40 px tall in a 400×200 frame: 120 px square.
        assert!((big.width - 120.0 / 400.0).abs() < 1e-6);
        assert!((big.height - 120.0 / 200.0).abs() < 1e-6);
        assert!((regions[1].x - 30.0 / 40.0).abs() < 1e-6);
        assert!((m.area() - (cells.len() as f32 / 800.0)).abs() < 1e-6);
    }

    #[test]
    fn the_mask_resizes_to_the_depth_size() {
        let m = mask(2, 1, &[(1, 0, 4)]);
        let resized = m.resized(4, 2);
        assert_eq!(resized.len(), 8);
        assert_eq!(resized[0], 0);
        assert_eq!(resized[3], 255);
        assert!(resized[1] < resized[2]);
    }

    #[test]
    fn take_frames_count_every_frame_that_starts_inside_the_take() {
        assert_eq!(take_frames(Duration::from_secs(30), 30_000), 900);
        assert_eq!(take_frames(Duration::from_millis(1001), 30_000), 31);
        assert_eq!(take_frames(Duration::from_secs(10), 29_970), 300);
        assert_eq!(take_frame_time(30, 30_000), Duration::from_secs(1));
        assert_eq!(take_frame_time(1, 29_970).as_micros(), 33_366);
    }

    #[test]
    fn depth_is_quantized_with_room_and_normalized_between_percentiles() {
        let scale = DepthScale::around(&[1.0, 2.0, 3.0]);
        assert_eq!((scale.low, scale.high), (-1.0, 5.0));
        assert_eq!(scale.quantize(-1.0), 0);
        assert_eq!(scale.quantize(5.0), 65535);
        assert_eq!(scale.quantize(9.0), 65535);
        assert_eq!(scale.quantize(f32::NAN), 0);
        let mut histogram = DepthHistogram::default();
        let values: Vec<u16> = (0..1000).map(|i| i as u16 * 10).collect();
        histogram.add(&values);
        assert_eq!(histogram.count(), 1000);
        let range = histogram.range();
        assert_eq!(
            range,
            DepthRange {
                far: 190,
                near: 9790
            }
        );
        assert_eq!(range.normalize(0), 0);
        assert_eq!(range.normalize(9790), 65535);
        assert_eq!(range.normalize(65535), 65535);
        assert_eq!(range.normalize((190 + 9790) / 2), 32768);
        let back = DepthHistogram::from_bytes(&histogram.to_bytes()).expect("same size");
        assert_eq!(back, histogram);
        assert!(DepthHistogram::from_bytes(&[0; 3]).is_none());
        // A flat take keeps working.
        let flat = DepthRange { far: 7, near: 7 };
        assert_eq!((flat.normalize(7), flat.normalize(8)), (0, 65535));
    }

    fn landmarks_file() -> FaceLandmarks {
        let mut face = mesh(0.3, 12.0);
        face.landmarks[5] = [0.123456, -0.0000001, 1.5];
        face.blendshapes[25] = 0.75;
        FaceLandmarks {
            source: FaceSource {
                file: "capture-1.mp4".to_owned(),
                width: 1920,
                height: 1080,
                fps_milli: 29_970,
                frames: 2,
                start: Duration::from_millis(1_200),
                end: Duration::from_millis(1_267),
            },
            model: ModelInfo {
                name: "MediaPipe Face Landmarker".to_owned(),
                version: "float16/1".to_owned(),
                files: vec![ModelFile {
                    file: "face_detector.onnx".to_owned(),
                    sha256: "ab".repeat(32),
                }],
            },
            frames: vec![
                FaceFrame {
                    at: Duration::ZERO,
                    faces: vec![face],
                    detected: Some(1),
                },
                FaceFrame {
                    at: Duration::from_millis(33),
                    faces: Vec::new(),
                    detected: None,
                },
            ],
        }
    }

    #[test]
    fn face_landmarks_json_reads_back() {
        let file = landmarks_file();
        let json = file.to_json();
        let back = FaceLandmarks::from_json(&json).expect("reads");
        assert_eq!(back.source, file.source);
        assert_eq!(back.model, file.model);
        assert_eq!(back.frames.len(), 2);
        let (a, b) = (&back.frames[0].faces[0], &file.frames[0].faces[0]);
        assert_eq!(a.landmarks.len(), FACE_LANDMARKS);
        for (p, q) in a.landmarks.iter().zip(&b.landmarks) {
            for i in 0..3 {
                assert!((p[i] - q[i]).abs() <= 0.00005, "{p:?} {q:?}");
            }
        }
        assert_eq!(a.blendshapes[25], 0.75);
        assert_eq!(back.frames[0].detected, Some(1));
        assert_eq!(back.frames[1].detected, None);
        assert!(back.frames[1].faces.is_empty());
        // Four decimals, trailing zeros and negative zero dropped.
        assert!(json.contains("[0.1235,0,1.5]"), "{}", &json[..400]);
        let value: serde_json::Value = serde_json::from_str(&json).expect("plain JSON");
        assert_eq!(value["blendshape_names"][25], "jawOpen");
        assert_eq!(
            value["frames"][0]["faces"][0]["blendshapes"]["jawOpen"],
            0.75
        );
        assert_eq!(value["source"]["fps"], 29.97);
    }

    #[test]
    fn the_face_center_is_the_median_of_its_boxes() {
        let mut file = landmarks_file();
        let shifted = |dx: f32| {
            let mut face = mesh(0.3, 0.0);
            for point in &mut face.landmarks {
                point[0] += dx;
            }
            face
        };
        file.frames = [0.0, -0.3, 0.2, 0.25, 0.3]
            .iter()
            .map(|&dx| FaceFrame {
                at: Duration::ZERO,
                faces: vec![shifted(dx)],
                detected: None,
            })
            .chain([FaceFrame {
                at: Duration::ZERO,
                faces: Vec::new(),
                detected: None,
            }])
            .collect();
        // Boxes centered at 0.5 + dx: the median is 0.7, a jump to 0.2
        // left out; down, the box runs 0.35 to 0.65.
        let (x, y) = file.face_center().expect("a face");
        assert!((x - 0.7).abs() < 1e-5, "{x}");
        assert!((y - 0.5).abs() < 1e-5, "{y}");
        file.frames.retain(|frame| frame.faces.is_empty());
        assert_eq!(file.face_center(), None);
    }

    #[test]
    fn a_face_without_every_landmark_does_not_read() {
        let file = landmarks_file();
        let mut value: serde_json::Value =
            serde_json::from_str(&file.to_json()).expect("plain JSON");
        value["frames"][0]["faces"][0]["landmarks"]
            .as_array_mut()
            .expect("array")
            .pop();
        assert!(FaceLandmarks::from_json(&value.to_string()).is_err());
        let mut value: serde_json::Value =
            serde_json::from_str(&file.to_json()).expect("plain JSON");
        value["version"] = 2.into();
        assert!(FaceLandmarks::from_json(&value.to_string()).is_err());
    }

    #[test]
    fn devices_report_the_slower() {
        let cpu = Device::Cpu {
            fallback: Some("no device".to_owned()),
        };
        assert_eq!(Device::Gpu.slower(Device::Gpu), Device::Gpu);
        assert_eq!(Device::Gpu.slower(cpu.clone()), cpu);
        assert_eq!(cpu.clone().slower(Device::Cpu { fallback: None }), cpu);
    }
}
