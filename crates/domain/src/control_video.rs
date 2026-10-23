//! Control videos (#191, docs/spec/performance-capture.md): what a take
//! model receives for a take, built on the computer in the take's mode,
//! and the free review that approves it before any video model is called.
//!
//! Here: a take's set (where the performer appears in Depth mode), the
//! size, frame rate and bitrate a control video is made at, what leaves
//! the computer in each mode, which frames borrow a nearby frame's face,
//! and the fingerprint an approval keeps.

use std::time::{Duration, SystemTime};

use crate::{
    CaptureId, JobId, NarrationId, Take, TakeAnalysis, TakeChecks, TakeFlag, TakeMode,
    VideoProjectId, analysis_fps,
};

/// Where the performer appears in Depth mode.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TakeSet {
    /// The recording's own background: the room.
    #[default]
    Room,
    /// An image copied into the project folder as `file`.
    Image { file: String, from: SetSource },
}

/// Where a set image came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SetSource {
    /// One of the project's scene images.
    Scene,
    /// A picture the user chose.
    Imported,
}

impl SetSource {
    pub fn code(self) -> &'static str {
        match self {
            SetSource::Scene => "scene",
            SetSource::Imported => "imported",
        }
    }

    pub fn from_code(code: &str) -> Option<SetSource> {
        [SetSource::Scene, SetSource::Imported]
            .into_iter()
            .find(|source| source.code() == code)
    }
}

impl TakeSet {
    /// The set image's file in the project folder, if the set is one.
    pub fn image(&self) -> Option<&str> {
        match self {
            TakeSet::Room => None,
            TakeSet::Image { file, .. } => Some(file),
        }
    }
}

/// The file extensions a set image may have, lowercase.
pub const SET_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// The largest file the take models take (the strictest's limit).
pub const MAX_CONTROL_BYTES: u64 = 100_000_000;
/// What a control video's bitrate is chosen to fill at most, leaving room
/// under [`MAX_CONTROL_BYTES`] for the container and the encoder's swings.
pub const CONTROL_BUDGET_BYTES: u64 = 90_000_000;
/// The highest video bitrate a control video gets, in bits a second:
/// plenty for 1080p, and a 30-second take stays near 45 MB.
pub const MAX_CONTROL_BITRATE: u32 = 12_000_000;
/// The lowest, for a take far over [`crate::MAX_TAKE`].
pub const MIN_CONTROL_BITRATE: u32 = 1_000_000;
/// The voice inside a control video: AAC at this many bits a second.
pub const CONTROL_AUDIO_BITRATE: u32 = 128_000;
/// How far apart a control video's keyframes are.
pub const CONTROL_KEYFRAME_EVERY: Duration = Duration::from_secs(2);
/// A control video fits inside 1920 × 1080, or 1080 × 1920 upright.
pub const CONTROL_LONG_SIDE: u32 = 1920;
pub const CONTROL_SHORT_SIDE: u32 = 1080;
/// Depth mode colors the figure between these points of the depth inside
/// the person mask, over the whole take, in thousandths: the room behind
/// leaves the scale to the performer.
pub const FIGURE_FAR_PERMILLE: u32 = 20;
pub const FIGURE_NEAR_PERMILLE: u32 = 980;
/// Where the person mask counts as the performer, from 0 to 255.
pub const FIGURE_MASK_FROM: u8 = 128;
/// How long a frame where no face was found borrows the covering of the
/// nearest frame with one, before or after it.
pub const FACE_HOLD: Duration = Duration::from_millis(500);

/// The picture size a control video of a `width` × `height` recording is
/// made at: inside [`CONTROL_LONG_SIDE`] × [`CONTROL_SHORT_SIDE`] the way
/// the recording stands, the aspect kept, even sides, never larger than
/// the recording.
pub fn control_size(width: u32, height: u32) -> (u32, u32) {
    let (long, short) = (width.max(height).max(1), width.min(height).max(1));
    let scale = (f64::from(CONTROL_LONG_SIDE) / f64::from(long))
        .min(f64::from(CONTROL_SHORT_SIDE) / f64::from(short))
        .min(1.0);
    let even = |side: u32| (((f64::from(side) * scale) as u32) & !1).max(2);
    (even(width), even(height))
}

/// The frame rate a take's control video is made at, in thousandths:
/// Recording mode keeps the recording's; Mesh and Depth draw on every
/// analyzed frame, so they run at the analysis rate.
pub fn control_fps(mode: TakeMode, fps_milli: u32) -> u32 {
    match mode {
        TakeMode::Recording => fps_milli,
        TakeMode::Mesh | TakeMode::Depth => analysis_fps(fps_milli),
    }
}

/// The video bitrate a control video of `length` gets: the most that keeps
/// it, with its voice, inside [`CONTROL_BUDGET_BYTES`], up to
/// [`MAX_CONTROL_BITRATE`].
pub fn control_bitrate(length: Duration) -> u32 {
    let seconds = length.as_secs_f64().max(1.0);
    let room = CONTROL_BUDGET_BYTES as f64 * 8.0 / seconds - f64::from(CONTROL_AUDIO_BITRATE);
    room.clamp(
        f64::from(MIN_CONTROL_BITRATE),
        f64::from(MAX_CONTROL_BITRATE),
    ) as u32
}

/// What a take's control video carries off the computer, as the review
/// says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outbound {
    /// The performer's face, body and room.
    Recording,
    /// Body, room and facial geometry, not the face.
    Mesh,
    /// Build and facial geometry as a depth figure, over the room.
    DepthOverRoom,
    /// Build and facial geometry as a depth figure, over a set image:
    /// nothing of the recording.
    DepthOverImage,
}

impl Outbound {
    pub const ALL: [Outbound; 4] = [
        Outbound::Recording,
        Outbound::Mesh,
        Outbound::DepthOverRoom,
        Outbound::DepthOverImage,
    ];

    /// A stable name, for the interface's texts.
    pub fn code(self) -> &'static str {
        match self {
            Outbound::Recording => "recording",
            Outbound::Mesh => "mesh",
            Outbound::DepthOverRoom => "depth_over_room",
            Outbound::DepthOverImage => "depth_over_image",
        }
    }

    pub fn of(mode: TakeMode, set: &TakeSet) -> Outbound {
        match (mode, set) {
            (TakeMode::Recording, _) => Outbound::Recording,
            (TakeMode::Mesh, _) => Outbound::Mesh,
            (TakeMode::Depth, TakeSet::Room) => Outbound::DepthOverRoom,
            (TakeMode::Depth, TakeSet::Image { .. }) => Outbound::DepthOverImage,
        }
    }

    /// Whether the set is described from the recording's background, so
    /// one frame of the control video goes to Claude (#192).
    pub fn sends_set_frame(self) -> bool {
        self != Outbound::DepthOverImage
    }
}

/// A borrowed face's cover grows by up to this much more of its size, in
/// proportion to how far it was borrowed from: the head may have moved.
pub const HELD_GROWTH: f32 = 0.5;

/// Which found faces cover a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceCover {
    /// The frame's own face, else the nearest one with a face no more
    /// than the hold away (the earlier when two are as near).
    pub nearest: usize,
    /// For a frame between two faces within the hold, the one on the
    /// other side: both are covered, so a head that moved stays under one.
    pub other: Option<usize>,
}

impl FaceCover {
    /// How much more than its own the cover of frame `index` grows, out
    /// of `hold` frames: none for a frame's own face, [`HELD_GROWTH`] at
    /// the full hold.
    pub fn growth(self, index: usize, hold: usize) -> f32 {
        if hold == 0 {
            return 0.0;
        }
        HELD_GROWTH * index.abs_diff(self.nearest) as f32 / hold as f32
    }
}

/// For each frame, the found faces that cover it ([`FaceCover`]), or none
/// when no face is within `hold` frames.
pub fn face_cover(found: &[bool], hold: usize) -> Vec<Option<FaceCover>> {
    let mut before = None;
    let mut earlier = Vec::with_capacity(found.len());
    for (index, &here) in found.iter().enumerate() {
        if here {
            before = Some(index);
        }
        earlier.push(before.filter(|&at| index - at <= hold));
    }
    let mut cover = vec![None; found.len()];
    let mut after = None;
    for index in (0..found.len()).rev() {
        if found[index] {
            after = Some(index);
        }
        let later = after.filter(|&at| at - index <= hold);
        cover[index] = match (earlier[index], later) {
            _ if found[index] => Some(FaceCover {
                nearest: index,
                other: None,
            }),
            (Some(earlier), Some(later)) => Some(if later - index < index - earlier {
                FaceCover {
                    nearest: later,
                    other: Some(earlier),
                }
            } else {
                FaceCover {
                    nearest: earlier,
                    other: Some(later),
                }
            }),
            (Some(nearest), None) | (None, Some(nearest)) => Some(FaceCover {
                nearest,
                other: None,
            }),
            (None, None) => None,
        };
    }
    cover
}

/// The finished analyses take `position` of `capture` needs for its mode,
/// as the take is now; `None` while one is missing.
pub fn needed_analyses<'a>(
    analyses: &'a [TakeAnalysis],
    capture: CaptureId,
    position: u32,
    take: &Take,
) -> Option<Vec<&'a TakeAnalysis>> {
    take.mode
        .parts()
        .iter()
        .map(|part| {
            analyses.iter().find(|analysis| {
                analysis.part == *part && analysis.fits(capture, position, take.revision)
            })
        })
        .collect()
}

/// What `analyses` found, merged.
pub fn merged_checks(analyses: &[&TakeAnalysis]) -> Option<TakeChecks> {
    analyses
        .iter()
        .map(|analysis| analysis.checks)
        .reduce(|merged, checks| merged.merged(&checks))
}

/// FNV-1a over `bytes`: stable across machines and releases.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// What a take's control video is made from, as an approval keeps it: the
/// capture, the take's bounds and revision, its mode, its set (in Depth
/// mode), the narration that is its voice and the analyses it draws on.
/// Any change to one gives another fingerprint, which unapproves it.
pub fn control_fingerprint(
    capture: CaptureId,
    position: u32,
    take: &Take,
    narration: NarrationId,
    analyses: &[&TakeAnalysis],
) -> String {
    let set = match (take.mode, &take.set) {
        (TakeMode::Depth, TakeSet::Image { file, .. }) => file.as_str(),
        _ => "room",
    };
    let mut text = format!(
        "control/1|{capture}|{position}|{}|{}|{}|{}|{set}|{narration}",
        take.revision,
        take.start.as_millis(),
        take.end.as_millis(),
        take.mode.code(),
    );
    let mut parts: Vec<&&TakeAnalysis> = analyses.iter().collect();
    parts.sort_by_key(|analysis| analysis.part);
    for analysis in parts {
        let finished = analysis
            .finished_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        text.push_str(&format!(
            "|{}:{}:{finished}:{}",
            analysis.part.code(),
            analysis.revision,
            analysis.model
        ));
    }
    format!("{:016x}", fnv1a(text.as_bytes()))
}

/// Why a take's control video cannot be approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApprovalBlock {
    /// More than one face in some frame: only the user may perform.
    SeveralFaces,
    /// Mesh mode: some frames had no face near enough to cover, so they
    /// would show the performer's face.
    Uncovered,
    /// No take model takes it at this length.
    Length(TakeFlag),
}

/// Why `take`, with what its analyses found and the `uncovered` frames of
/// its control video, cannot be approved, if anything.
pub fn approval_block(
    take: &Take,
    checks: Option<&TakeChecks>,
    uncovered: u32,
) -> Option<ApprovalBlock> {
    if checks.is_some_and(TakeChecks::blocked) {
        return Some(ApprovalBlock::SeveralFaces);
    }
    if take.mode == TakeMode::Mesh && uncovered > 0 {
        return Some(ApprovalBlock::Uncovered);
    }
    take.flag().map(ApprovalBlock::Length)
}

/// A take's control video as built: the file and what made it. Kept until
/// a newer build of the take replaces it or the capture goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlVideo {
    pub project: VideoProjectId,
    pub capture: CaptureId,
    pub position: u32,
    /// [`control_fingerprint`] of what it was made from.
    pub fingerprint: String,
    /// The MP4 in the project folder.
    pub file: String,
    pub bytes: u64,
    pub frames: u32,
    /// Mesh: frames with no face near enough to cover, which would go as
    /// recorded; such a build cannot be approved.
    pub uncovered: u32,
    /// The encoder's ffmpeg name.
    pub encoder: String,
    pub built_at: SystemTime,
    pub job: Option<JobId>,
    /// When the user approved it.
    pub approved_at: Option<SystemTime>,
    /// The build it replaced was approved, for the take as it was then:
    /// the review says so until this one is approved.
    pub replaced_approval: bool,
}

impl ControlVideo {
    /// Whether it was made from what `fingerprint` names and approved.
    pub fn approved_for(&self, fingerprint: &str) -> bool {
        self.fingerprint == fingerprint && self.approved_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisPart, Device};

    #[test]
    fn control_videos_fit_inside_1080p_the_way_the_recording_stands() {
        assert_eq!(control_size(1920, 1080), (1920, 1080));
        assert_eq!(control_size(3840, 2160), (1920, 1080));
        assert_eq!(control_size(2160, 3840), (1080, 1920));
        assert_eq!(control_size(1280, 720), (1280, 720));
        // 4:3 at 4K: the short side is the limit.
        assert_eq!(control_size(2880, 2160), (1440, 1080));
        // Odd sides come out even.
        assert_eq!(control_size(1281, 721), (1280, 720));
        let (w, h) = control_size(2001, 1333);
        assert!(
            w % 2 == 0 && h % 2 == 0 && w <= 1920 && h <= 1080,
            "{w}×{h}"
        );
    }

    #[test]
    fn mesh_and_depth_run_at_the_analysis_rate() {
        assert_eq!(control_fps(TakeMode::Recording, 59_940), 59_940);
        assert_eq!(control_fps(TakeMode::Mesh, 59_940), 29_970);
        assert_eq!(control_fps(TakeMode::Depth, 25_000), 25_000);
    }

    #[test]
    fn a_thirty_second_take_stays_far_under_the_limit() {
        let bits = control_bitrate(Duration::from_secs(30));
        assert_eq!(bits, MAX_CONTROL_BITRATE);
        // Even if the encoder ran at its 1.5× peak the whole time.
        let worst = (f64::from(bits) * 1.5 + f64::from(CONTROL_AUDIO_BITRATE)) * 30.0 / 8.0;
        assert!(worst < MAX_CONTROL_BYTES as f64, "{worst}");
        // A long flagged take gets less, to stay inside the budget.
        let long = control_bitrate(Duration::from_secs(120));
        assert!(long < MAX_CONTROL_BITRATE);
        let bytes = (f64::from(long) + f64::from(CONTROL_AUDIO_BITRATE)) * 120.0 / 8.0;
        assert!(bytes <= CONTROL_BUDGET_BYTES as f64 + 1.0, "{bytes}");
        assert_eq!(
            control_bitrate(Duration::from_secs(15 * 60)),
            MIN_CONTROL_BITRATE
        );
    }

    #[test]
    fn what_leaves_the_computer_follows_the_mode_and_set() {
        let image = TakeSet::Image {
            file: "set.png".into(),
            from: SetSource::Imported,
        };
        assert_eq!(
            Outbound::of(TakeMode::Recording, &image),
            Outbound::Recording
        );
        assert_eq!(Outbound::of(TakeMode::Mesh, &image), Outbound::Mesh);
        assert_eq!(
            Outbound::of(TakeMode::Depth, &TakeSet::Room),
            Outbound::DepthOverRoom
        );
        assert_eq!(
            Outbound::of(TakeMode::Depth, &image),
            Outbound::DepthOverImage
        );
        assert!(Outbound::Mesh.sends_set_frame());
        assert!(Outbound::DepthOverRoom.sends_set_frame());
        assert!(!Outbound::DepthOverImage.sends_set_frame());
        for source in [SetSource::Scene, SetSource::Imported] {
            assert_eq!(SetSource::from_code(source.code()), Some(source));
        }
    }

    #[test]
    fn frames_without_a_face_borrow_the_nearest_within_the_hold() {
        let found = |pattern: &str| pattern.chars().map(|c| c == 'F').collect::<Vec<_>>();
        let nearest = |pattern: &str, hold: usize| {
            face_cover(&found(pattern), hold)
                .iter()
                .map(|cover| cover.map(|c| c.nearest))
                .collect::<Vec<_>>()
        };
        assert_eq!(nearest("F..F", 2), [Some(0), Some(0), Some(3), Some(3)]);
        // Equally near: the earlier.
        assert_eq!(nearest("F.F", 1), [Some(0), Some(0), Some(2)]);
        // Past the hold, nothing covers.
        assert_eq!(
            nearest("F....F", 1),
            [Some(0), Some(0), None, None, Some(5), Some(5)]
        );
        assert_eq!(nearest("...", 5), [None, None, None]);
        assert_eq!(nearest(".F", 0), [None, Some(1)]);
        // Between two faces within the hold, both cover; a frame's own
        // face alone covers it.
        let covers = face_cover(&found("F..F."), 2);
        assert_eq!(covers[0].unwrap().other, None);
        assert_eq!(covers[1].unwrap().other, Some(3));
        assert_eq!(covers[2].unwrap().other, Some(0));
        assert_eq!(covers[4].unwrap().other, None);
        // A borrowed cover grows with the distance it was borrowed from.
        assert_eq!(covers[0].unwrap().growth(0, 2), 0.0);
        assert_eq!(covers[4].unwrap().growth(4, 2), HELD_GROWTH / 2.0);
        assert_eq!(covers[4].unwrap().growth(4, 0), 0.0);
        assert!(face_cover(&[], 3).is_empty());
    }

    fn take(mode: TakeMode, set: TakeSet) -> Take {
        Take {
            words: 0..4,
            start: Duration::from_millis(1_000),
            end: Duration::from_millis(9_000),
            revision: 2,
            mode,
            set,
            model: None,
        }
    }

    fn analysis(capture: CaptureId, part: AnalysisPart, revision: u32) -> TakeAnalysis {
        TakeAnalysis {
            project: VideoProjectId::new(),
            capture,
            position: 1,
            revision,
            part,
            device: Device::Gpu,
            model: "m 1".into(),
            checks: TakeChecks {
                frames: 240,
                ..TakeChecks::default()
            },
            depth_range: None,
            finished_at: SystemTime::UNIX_EPOCH + Duration::from_secs(100),
            job: None,
        }
    }

    #[test]
    fn the_fingerprint_changes_with_everything_the_video_is_made_from() {
        let capture = CaptureId::new();
        let narration = NarrationId::new();
        let face = analysis(capture, AnalysisPart::Face, 2);
        let depth = analysis(capture, AnalysisPart::Depth, 2);
        let base = take(TakeMode::Mesh, TakeSet::Room);
        let print = |take: &Take, analyses: &[&TakeAnalysis]| {
            control_fingerprint(capture, 1, take, narration, analyses)
        };
        let first = print(&base, &[&face]);
        assert_eq!(first.len(), 16);
        assert_eq!(first, print(&base.clone(), &[&face]), "stable");

        let image = TakeSet::Image {
            file: "set-1.png".into(),
            from: SetSource::Scene,
        };
        // Outside Depth mode the set plays no part.
        assert_eq!(print(&take(TakeMode::Mesh, image.clone()), &[&face]), first);
        let depth_room = print(&take(TakeMode::Depth, TakeSet::Room), &[&face, &depth]);
        let depth_image = print(&take(TakeMode::Depth, image), &[&face, &depth]);
        assert_ne!(depth_room, depth_image);
        // The analyses' order does not matter.
        assert_eq!(
            depth_room,
            print(&take(TakeMode::Depth, TakeSet::Room), &[&depth, &face])
        );

        let mut moved = base.clone();
        moved.end = Duration::from_millis(9_500);
        let mut revised = base.clone();
        revised.revision = 3;
        let mut redone = face.clone();
        redone.finished_at += Duration::from_secs(1);
        for other in [
            print(&take(TakeMode::Recording, TakeSet::Room), &[&face]),
            print(&moved, &[&face]),
            print(&revised, &[&face]),
            print(&base, &[&redone]),
            control_fingerprint(capture, 1, &base, NarrationId::new(), &[&face]),
            control_fingerprint(capture, 0, &base, narration, &[&face]),
            control_fingerprint(CaptureId::new(), 1, &base, narration, &[&face]),
        ] {
            assert_ne!(other, first);
        }
    }

    #[test]
    fn a_take_needs_the_analyses_of_its_mode_at_its_revision() {
        let capture = CaptureId::new();
        let analyses = vec![
            analysis(capture, AnalysisPart::Face, 2),
            analysis(capture, AnalysisPart::Depth, 1),
        ];
        let mesh = take(TakeMode::Mesh, TakeSet::Room);
        assert_eq!(
            needed_analyses(&analyses, capture, 1, &mesh).map(|found| found.len()),
            Some(1)
        );
        // The depth analysis is of an older revision.
        let depth = take(TakeMode::Depth, TakeSet::Room);
        assert!(needed_analyses(&analyses, capture, 1, &depth).is_none());
        assert!(needed_analyses(&analyses, capture, 0, &mesh).is_none());
        assert!(needed_analyses(&analyses, CaptureId::new(), 1, &mesh).is_none());
    }

    #[test]
    fn several_faces_an_uncovered_face_or_a_length_no_model_takes_block_approval() {
        let ok = take(TakeMode::Mesh, TakeSet::Room);
        let fine = TakeChecks {
            frames: 10,
            no_face: 3,
            ..TakeChecks::default()
        };
        assert_eq!(approval_block(&ok, Some(&fine), 0), None);
        assert_eq!(approval_block(&ok, None, 0), None);
        // Mesh frames left uncovered would show the face; elsewhere they
        // show none.
        assert_eq!(
            approval_block(&ok, Some(&fine), 2),
            Some(ApprovalBlock::Uncovered)
        );
        let depth = take(TakeMode::Depth, TakeSet::Room);
        assert_eq!(approval_block(&depth, Some(&fine), 2), None);
        let crowded = TakeChecks {
            several_faces: 1,
            ..fine
        };
        assert_eq!(
            approval_block(&ok, Some(&crowded), 0),
            Some(ApprovalBlock::SeveralFaces)
        );
        let mut long = ok.clone();
        long.end = long.start + Duration::from_secs(31);
        assert_eq!(
            approval_block(&long, Some(&fine), 0),
            Some(ApprovalBlock::Length(TakeFlag::TooLong))
        );
    }

    #[test]
    fn an_approval_holds_for_its_fingerprint_only() {
        let video = ControlVideo {
            project: VideoProjectId::new(),
            capture: CaptureId::new(),
            position: 0,
            fingerprint: "abc".into(),
            file: "control.mp4".into(),
            bytes: 10,
            frames: 3,
            uncovered: 0,
            encoder: "libopenh264".into(),
            built_at: SystemTime::UNIX_EPOCH,
            job: None,
            approved_at: None,
            replaced_approval: false,
        };
        assert!(!video.approved_for("abc"));
        let approved = ControlVideo {
            approved_at: Some(SystemTime::UNIX_EPOCH),
            ..video
        };
        assert!(approved.approved_for("abc"));
        assert!(!approved.approved_for("abd"));
    }
}
