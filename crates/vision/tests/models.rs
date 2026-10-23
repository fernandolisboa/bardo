//! The vision models on short public-domain clips (`fixtures/README.md`),
//! decoded by the pinned ffmpeg. Needs the models where `BARDO_MODELS_DIR`
//! points (`cargo xtask fetch-models`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use bardo_domain::{
    BLENDSHAPE_NAMES, DepthEstimator, FACE_LANDMARKS, FaceObservation, FaceTracker, RgbFrame,
    VisionError, VisionModels, distinct_faces,
};
use bardo_media::ffmpeg::Ffmpeg;
use bardo_vision::{DepthKind, OnnxVision, Wanted};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn ffmpeg() -> Ffmpeg {
    Ffmpeg::locate().expect("the pinned ffmpeg (cargo xtask fetch-ffmpeg)")
}

/// Every frame of a 640×360 fixture at 15 fps.
fn frames(name: &str, seconds: u64) -> Vec<RgbFrame> {
    let mut source = ffmpeg()
        .rgb_frames(
            &fixture(name),
            Duration::ZERO,
            Duration::from_secs(seconds),
            15_000,
            (640, 360),
        )
        .expect("decodes");
    let mut out = Vec::new();
    while let Some(frame) = source.next_frame().expect("a frame") {
        out.push(frame);
    }
    out
}

fn vision() -> OnnxVision {
    OnnxVision::new(
        bardo_vision::locate().expect("BARDO_MODELS_DIR"),
        Wanted::Gpu,
    )
}

fn at(index: usize) -> Duration {
    Duration::from_micros(index as u64 * 1_000_000 / 15)
}

fn no_head() -> impl FnMut() -> Result<Option<bardo_domain::Region>, VisionError> {
    || Ok(None)
}

fn track_all(
    tracker: &mut dyn FaceTracker,
    frames: &[RgbFrame],
    from: usize,
) -> Vec<FaceObservation> {
    frames
        .iter()
        .enumerate()
        .skip(from)
        .map(|(i, frame)| tracker.track(frame, at(i), &mut no_head()).expect("tracks"))
        .collect()
}

#[test]
fn a_talking_head_has_its_mesh_in_every_frame_and_resumes_the_same() {
    let frames = frames("head-3s.mp4", 3);
    assert_eq!(frames.len(), 45);
    let vision = vision();
    let mut tracker = vision.face_tracker().expect("loads");
    assert_eq!(tracker.model().name, "MediaPipe Face Landmarker");
    assert_eq!(tracker.model().files.len(), 3);
    let straight = track_all(tracker.as_mut(), &frames, 0);
    for seen in &straight {
        let mesh = seen.mesh.as_ref().expect("a face in every frame");
        assert_eq!(mesh.landmarks.len(), FACE_LANDMARKS);
        assert!(
            mesh.landmarks
                .iter()
                .all(|p| (0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1]))
        );
        assert!(mesh.blendshapes.iter().all(|s| (-0.01..=1.01).contains(s)));
        assert_eq!(seen.detected, Some(1));
        // A face about a third of the frame, looking at the camera.
        assert!(mesh.height() > 0.2, "{}", mesh.height());
        assert!(mesh.turn_degrees() < 30.0, "{}", mesh.turn_degrees());
    }
    assert_eq!(BLENDSHAPE_NAMES.len(), 52);

    // Stopped after frame 20 and started again from its state: the same.
    let mut first = vision.face_tracker().expect("loads");
    track_all(first.as_mut(), &frames[..20], 0);
    let state = first.snapshot();
    let mut second = vision.face_tracker().expect("loads");
    second.restore(&state).expect("restores");
    let resumed = track_all(second.as_mut(), &frames, 20);
    for (a, b) in straight[20..].iter().zip(&resumed) {
        let (a, b) = (a.mesh.as_ref().unwrap(), b.mesh.as_ref().unwrap());
        for (p, q) in a.landmarks.iter().zip(&b.landmarks) {
            assert!((p[0] - q[0]).abs() < 1e-5 && (p[1] - q[1]).abs() < 1e-5);
        }
    }
    assert!(second.restore(b"not a state").is_err());
}

#[test]
fn two_people_are_found_in_their_person_regions() {
    let frames = frames("two-people-1s.mp4", 1);
    let vision = vision();
    let mut tracker = vision.face_tracker().expect("loads");
    let mut segmenter = vision.person_segmenter().expect("loads");
    let frame = &frames[0];
    let mask = segmenter.segment(frame).expect("segments");
    let regions = mask.head_regions(frame.width, frame.height);
    assert_eq!(regions.len(), 2, "{regions:?}");
    let mut faces = Vec::new();
    for region in &regions {
        faces.extend(tracker.detect_in(frame, region).expect("detects"));
    }
    assert_eq!(distinct_faces(&faces), 2, "{faces:?}");
    let seen = tracker
        .track(frame, Duration::ZERO, &mut no_head())
        .expect("tracks");
    assert!(seen.mesh.is_some());
}

#[test]
fn a_small_standing_face_is_found_around_the_person_mask() {
    let frames = frames("standing-1s.mp4", 1);
    let vision = vision();
    let mut tracker = vision.face_tracker().expect("loads");
    let mut segmenter = vision.person_segmenter().expect("loads");
    let mut found = 0;
    for (i, frame) in frames.iter().enumerate() {
        let mut head = || {
            let mask = segmenter.segment(frame)?;
            Ok(mask
                .head_regions(frame.width, frame.height)
                .first()
                .copied())
        };
        let seen = tracker.track(frame, at(i), &mut head).expect("tracks");
        found += usize::from(seen.mesh.is_some());
    }
    assert!(found + 1 >= frames.len(), "{found} of {}", frames.len());
}

fn depth_all(estimator: &mut dyn DepthEstimator, frames: &[RgbFrame]) -> Vec<Vec<f32>> {
    let mut out = Vec::new();
    for frame in frames {
        out.extend(
            estimator
                .push(frame)
                .expect("pushes")
                .into_iter()
                .map(|d| d.data),
        );
    }
    out
}

fn check_depth(kind: DepthKind) {
    let frames = frames("head-3s.mp4", 3);
    let vision = vision();
    let mut estimator = vision.depth_of_kind(false, kind).expect("loads");
    assert_eq!(estimator.size(), (504, 280));
    let mut straight = depth_all(estimator.as_mut(), &frames);
    straight.extend(
        estimator
            .finish()
            .expect("finishes")
            .into_iter()
            .map(|d| d.data),
    );
    assert_eq!(straight.len(), frames.len());
    // The speaker is nearer (larger) than the screen behind her.
    let d = &straight[0];
    let near = d[150 * 504 + 360];
    let far = d[100 * 504 + 120];
    assert!(near > far, "{near} {far}");

    let mut first = vision.depth_of_kind(false, kind).expect("loads");
    let mut resumed = depth_all(first.as_mut(), &frames[..37]);
    let state = first.snapshot();
    let mut second = vision.depth_of_kind(false, kind).expect("loads");
    second.restore(&state).expect("restores");
    resumed.extend(depth_all(second.as_mut(), &frames[37..]));
    resumed.extend(
        second
            .finish()
            .expect("finishes")
            .into_iter()
            .map(|d| d.data),
    );
    assert_eq!(resumed.len(), straight.len());
    for (a, b) in straight.iter().zip(&resumed) {
        let worst = a
            .iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max);
        assert!(worst < 1e-3, "{worst}");
    }
}

#[test]
fn video_depth_anything_gives_every_frame_and_resumes_the_same() {
    check_depth(DepthKind::Video);
}

#[test]
fn depth_anything_v2_gives_every_frame_and_resumes_the_same() {
    check_depth(DepthKind::PerFrame);
}

/// Mesh mode (#191) on a real face: no pixel inside the face's outline
/// still passes for skin once the face is covered and its mesh drawn.
#[test]
fn a_covered_face_shows_no_skin_inside_its_outline() {
    use bardo_media::control::{Canvas, inside, looks_like_skin};

    let frames = frames("head-3s.mp4", 3);
    let vision = vision();
    let mut tracker = vision.face_tracker().expect("loads");
    let seen = track_all(tracker.as_mut(), &frames, 0);
    let mut skin_before = 0;
    for (frame, seen) in frames.iter().zip(&seen).step_by(5) {
        let mesh = seen.mesh.as_ref().expect("a face in every frame");
        let mut canvas = Canvas::new(frame.width, frame.height).expect("a canvas");
        canvas.put_rgb(frame);
        let outline = canvas.face_outline(mesh);
        let pixels_inside = |picture: &RgbFrame| {
            let mut skin = 0;
            for y in 0..picture.height {
                for x in 0..picture.width {
                    let point = (x as f32 + 0.5, y as f32 + 0.5);
                    let at = ((y * picture.width + x) * 3) as usize;
                    let rgb = [picture.rgb[at], picture.rgb[at + 1], picture.rgb[at + 2]];
                    if inside(&outline, point) && looks_like_skin(rgb) {
                        skin += 1;
                    }
                }
            }
            skin
        };
        skin_before += pixels_inside(frame);
        canvas.cover_face(mesh);
        canvas.draw_mesh(mesh);
        assert_eq!(
            pixels_inside(&canvas.rgb()),
            0,
            "skin shows inside the outline"
        );
    }
    // The clip's face does read as skin before it is covered.
    assert!(skin_before > 1_000, "{skin_before}");
}
