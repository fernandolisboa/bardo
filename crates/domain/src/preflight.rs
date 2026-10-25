//! Preflight checks (#243): free checks of a take against what its take
//! model documents it needs, read from the take's local analysis and the
//! persona's look before anything is paid for.
//!
//! Kling's Motion Control documents its inputs: one person, whose head and
//! upper body stay in the frame, not too small, moving smoothly, and a look
//! framed like the recording (no half-body look driven by full-body
//! motion). A failed check says why and holds the take's paid steps until
//! the user sends it anyway. A check that cannot be read yet (a take
//! analyzed before it was measured, a look not measured yet) says so and
//! holds nothing.

use crate::{TakeChecks, TakeModel};

/// Frames with no face, or with the head or upper body cut off, past this
/// share of the take fail "in frame".
pub const MAX_OUT_OF_FRAME: f32 = 0.10;

/// Frames whose face moved too fast past this share of the frames with a
/// face fail "steady motion".
pub const MAX_FAST_FRAMES: f32 = 0.05;

/// An average face shorter than this share of the frame's height is too
/// small for the model to follow.
pub const MIN_PREFLIGHT_FACE: f32 = 0.08;

/// A face at least this share of the picture's height is framed from the
/// waist (or closer) up; a smaller one shows the whole body.
pub const HALF_BODY_FACE: f32 = 0.16;

/// One preflight check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreflightCheck {
    /// One person in every frame.
    OnePerson,
    /// The head and the upper body stay inside the frame.
    InFrame,
    /// The face is large enough.
    FaceSize,
    /// No very fast motion.
    Motion,
    /// The look is framed like the recording: half body or full body.
    Framing,
}

impl PreflightCheck {
    pub const ALL: [PreflightCheck; 5] = [
        PreflightCheck::OnePerson,
        PreflightCheck::InFrame,
        PreflightCheck::FaceSize,
        PreflightCheck::Motion,
        PreflightCheck::Framing,
    ];

    /// A stable name, for the interface's texts.
    pub fn code(self) -> &'static str {
        match self {
            PreflightCheck::OnePerson => "one_person",
            PreflightCheck::InFrame => "in_frame",
            PreflightCheck::FaceSize => "face_size",
            PreflightCheck::Motion => "motion",
            PreflightCheck::Framing => "framing",
        }
    }
}

/// How much of a person a picture shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyFraming {
    /// From the waist or closer up.
    HalfBody,
    /// The whole body.
    FullBody,
}

impl BodyFraming {
    pub const ALL: [BodyFraming; 2] = [BodyFraming::HalfBody, BodyFraming::FullBody];

    /// A stable name, for the interface's texts.
    pub fn code(self) -> &'static str {
        match self {
            BodyFraming::HalfBody => "half_body",
            BodyFraming::FullBody => "full_body",
        }
    }

    /// The framing of a picture whose face is `face_height` of its height.
    pub fn of(face_height: f32) -> BodyFraming {
        if face_height >= HALF_BODY_FACE {
            BodyFraming::HalfBody
        } else {
            BodyFraming::FullBody
        }
    }
}

/// What a check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightOutcome {
    Passed,
    /// The take does not meet it: its paid steps wait.
    Failed,
    /// It cannot be read yet; it holds nothing.
    Unknown,
}

/// The look's face, as measured on the persona's look.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LookFace {
    /// Not measured yet.
    #[default]
    Unmeasured,
    /// The face tracker found no face in it.
    NoFace,
    /// Its face's height, as a share of the picture's.
    Height(f32),
}

/// One check's result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreflightResult {
    pub check: PreflightCheck,
    pub outcome: PreflightOutcome,
}

/// A take's preflight: the checks its model needs, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Preflight {
    pub results: Vec<PreflightResult>,
    /// The recording's and the look's framing, where known.
    pub capture: Option<BodyFraming>,
    pub look: Option<BodyFraming>,
}

impl Preflight {
    /// The checks it failed.
    pub fn failed(&self) -> impl Iterator<Item = PreflightCheck> + '_ {
        self.results
            .iter()
            .filter(|result| result.outcome == PreflightOutcome::Failed)
            .map(|result| result.check)
    }

    /// Whether a check failed: the take's paid steps wait until the user
    /// sends it anyway.
    pub fn holds(&self) -> bool {
        self.failed().next().is_some()
    }

    /// What `check` found, when the model needs it.
    pub fn outcome(&self, check: PreflightCheck) -> Option<PreflightOutcome> {
        self.results
            .iter()
            .find(|result| result.check == check)
            .map(|result| result.outcome)
    }
}

impl TakeModel {
    /// The preflight checks its takes go through. A model that keeps the
    /// take's framing (a video edit) does not care how the look is framed.
    pub fn preflight_checks(&self) -> Vec<PreflightCheck> {
        PreflightCheck::ALL
            .into_iter()
            .filter(|check| !(self.keeps_framing && *check == PreflightCheck::Framing))
            .collect()
    }
}

/// Whether `part` of `whole` is more than `share` of it.
fn over(part: u32, whole: u32, share: f32) -> bool {
    whole > 0 && part as f32 > share * whole as f32
}

/// The preflight of a take going to `model`, from its face analysis'
/// `checks` (`None` while it has none) and the performer's `look`.
pub fn preflight(model: &TakeModel, checks: Option<&TakeChecks>, look: LookFace) -> Preflight {
    use PreflightOutcome::{Failed, Passed, Unknown};
    let pass = |failed: bool| if failed { Failed } else { Passed };
    let measures = checks.and_then(|checks| checks.measures);
    let capture = checks
        .and_then(TakeChecks::mean_face_height)
        .map(BodyFraming::of);
    let look_framing = match look {
        LookFace::Height(height) => Some(BodyFraming::of(height)),
        _ => None,
    };
    let results = model
        .preflight_checks()
        .into_iter()
        .map(|check| {
            let outcome = match (check, checks) {
                (_, None) => Unknown,
                (PreflightCheck::OnePerson, Some(checks)) => pass(checks.several_faces > 0),
                (PreflightCheck::InFrame, Some(checks)) => {
                    let missing = checks.no_face;
                    match measures {
                        Some(measures) => pass(over(
                            missing + measures.cut_off,
                            checks.frames,
                            MAX_OUT_OF_FRAME,
                        )),
                        // The faces missing fail it on their own.
                        None if over(missing, checks.frames, MAX_OUT_OF_FRAME) => Failed,
                        None => Unknown,
                    }
                }
                (PreflightCheck::FaceSize, Some(checks)) => match checks.mean_face_height() {
                    Some(height) => pass(height < MIN_PREFLIGHT_FACE),
                    None if measures.is_some() => Failed,
                    None => Unknown,
                },
                (PreflightCheck::Motion, Some(checks)) => match measures {
                    Some(measures) => {
                        pass(over(measures.fast, checks.face_frames(), MAX_FAST_FRAMES))
                    }
                    None => Unknown,
                },
                (PreflightCheck::Framing, Some(_)) => match (capture, look) {
                    (_, LookFace::NoFace) => Failed,
                    (Some(capture), LookFace::Height(_)) => pass(Some(capture) != look_framing),
                    _ => Unknown,
                },
            };
            PreflightResult { check, outcome }
        })
        .collect();
    Preflight {
        results,
        capture,
        look: look_framing,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{ClipModelRef, FaceMeasures, Provider, TakeMode, TakeTier};

    fn model(keeps_framing: bool) -> TakeModel {
        TakeModel {
            id: ClipModelRef::new(Provider::Kling, "motion-control/kling-3.0/720p").unwrap(),
            name: "Kling".into(),
            modes: vec![TakeMode::Recording],
            draft: None,
            final_tier: TakeTier {
                resolution: None,
                height: 720,
                model: None,
            },
            finishes_draft: false,
            draft_lifetime: None,
            look_assets: false,
            token_frame_rate: None,
            keeps_framing,
            min_length: Duration::from_secs(3),
            max_length: Duration::from_secs(30),
            max_video_bytes: 100_000_000,
            max_image_bytes: 10_000_000,
        }
    }

    /// A clean 100-frame half-body take: faces of 0.25 of the frame.
    fn clean() -> TakeChecks {
        TakeChecks {
            frames: 100,
            measures: Some(FaceMeasures {
                height_sum: 25_000,
                cut_off: 0,
                fast: 0,
            }),
            ..TakeChecks::default()
        }
    }

    fn outcome(checks: &TakeChecks, look: LookFace, check: PreflightCheck) -> PreflightOutcome {
        preflight(&model(false), Some(checks), look)
            .outcome(check)
            .unwrap()
    }

    #[test]
    fn checks_have_stable_codes_and_a_video_edit_skips_framing() {
        let codes: Vec<_> = PreflightCheck::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            ["one_person", "in_frame", "face_size", "motion", "framing"]
        );
        assert_eq!(model(false).preflight_checks(), PreflightCheck::ALL);
        assert!(
            !model(true)
                .preflight_checks()
                .contains(&PreflightCheck::Framing)
        );
    }

    #[test]
    fn a_clean_take_with_a_matching_look_passes_everything() {
        let found = preflight(&model(false), Some(&clean()), LookFace::Height(0.3));
        assert!(!found.holds());
        assert!(
            found
                .results
                .iter()
                .all(|result| result.outcome == PreflightOutcome::Passed)
        );
        assert_eq!(found.capture, Some(BodyFraming::HalfBody));
        assert_eq!(found.look, Some(BodyFraming::HalfBody));
    }

    #[test]
    fn one_person_fails_on_any_frame_with_another_face() {
        let mut checks = clean();
        checks.several_faces = 1;
        let found = preflight(&model(false), Some(&checks), LookFace::Height(0.3));
        assert_eq!(
            found.failed().collect::<Vec<_>>(),
            [PreflightCheck::OnePerson]
        );
        assert!(found.holds());
    }

    #[test]
    fn in_frame_fails_past_a_tenth_of_frames_cut_off_or_without_a_face() {
        let mut checks = clean();
        checks.no_face = 5;
        checks.measures.as_mut().unwrap().cut_off = 5;
        let look = LookFace::Height(0.3);
        assert_eq!(
            outcome(&checks, look, PreflightCheck::InFrame),
            PreflightOutcome::Passed
        );
        checks.measures.as_mut().unwrap().cut_off = 6;
        assert_eq!(
            outcome(&checks, look, PreflightCheck::InFrame),
            PreflightOutcome::Failed
        );
    }

    #[test]
    fn a_small_or_missing_face_fails_its_size() {
        let mut checks = clean();
        checks.measures.as_mut().unwrap().height_sum = 7_900;
        let look = LookFace::Height(0.05);
        assert_eq!(
            outcome(&checks, look, PreflightCheck::FaceSize),
            PreflightOutcome::Failed
        );
        checks.measures.as_mut().unwrap().height_sum = 8_000;
        assert_eq!(
            outcome(&checks, look, PreflightCheck::FaceSize),
            PreflightOutcome::Passed
        );
        checks.no_face = 100;
        checks.measures.as_mut().unwrap().height_sum = 0;
        assert_eq!(
            outcome(&checks, look, PreflightCheck::FaceSize),
            PreflightOutcome::Failed,
            "no face at all"
        );
    }

    #[test]
    fn motion_fails_past_one_frame_in_twenty_moving_too_fast() {
        let mut checks = clean();
        checks.measures.as_mut().unwrap().fast = 5;
        let look = LookFace::Height(0.3);
        assert_eq!(
            outcome(&checks, look, PreflightCheck::Motion),
            PreflightOutcome::Passed
        );
        checks.measures.as_mut().unwrap().fast = 6;
        assert_eq!(
            outcome(&checks, look, PreflightCheck::Motion),
            PreflightOutcome::Failed
        );
    }

    #[test]
    fn a_look_framed_otherwise_or_without_a_face_fails_framing() {
        let half = clean();
        let framing = PreflightCheck::Framing;
        assert_eq!(
            outcome(&half, LookFace::Height(0.10), framing),
            PreflightOutcome::Failed,
            "a full-body look driven by half-body motion"
        );
        assert_eq!(
            outcome(&half, LookFace::NoFace, framing),
            PreflightOutcome::Failed
        );
        assert_eq!(
            outcome(&half, LookFace::Unmeasured, framing),
            PreflightOutcome::Unknown
        );
        let mut full = clean();
        full.measures.as_mut().unwrap().height_sum = 12_000;
        assert_eq!(
            outcome(&full, LookFace::Height(0.10), framing),
            PreflightOutcome::Passed
        );
        assert_eq!(BodyFraming::of(HALF_BODY_FACE), BodyFraming::HalfBody);
        assert_eq!(
            BodyFraming::of(HALF_BODY_FACE - 0.001),
            BodyFraming::FullBody
        );
    }

    #[test]
    fn what_cannot_be_read_yet_holds_nothing() {
        let found = preflight(&model(false), None, LookFace::Height(0.3));
        assert!(!found.holds());
        assert!(
            found
                .results
                .iter()
                .all(|result| result.outcome == PreflightOutcome::Unknown)
        );
        // Analyzed before the measures were counted.
        let old = TakeChecks {
            frames: 100,
            ..TakeChecks::default()
        };
        let found = preflight(&model(false), Some(&old), LookFace::Height(0.3));
        assert_eq!(
            found.outcome(PreflightCheck::OnePerson),
            Some(PreflightOutcome::Passed)
        );
        for check in [
            PreflightCheck::InFrame,
            PreflightCheck::FaceSize,
            PreflightCheck::Motion,
            PreflightCheck::Framing,
        ] {
            assert_eq!(found.outcome(check), Some(PreflightOutcome::Unknown));
        }
        // Faces missing fail "in frame" even without the measures.
        let gone = TakeChecks {
            frames: 100,
            no_face: 11,
            ..TakeChecks::default()
        };
        let found = preflight(&model(false), Some(&gone), LookFace::Unmeasured);
        assert_eq!(
            found.failed().collect::<Vec<_>>(),
            [PreflightCheck::InFrame]
        );
    }
}
