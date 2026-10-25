//! Performed takes (#192, docs/spec/performance-capture.md): what a take
//! model made for a take, and the prompt it was made from.
//!
//! - A take's **prompt** is written by Claude from the take prompt
//!   template, then edited by the user. It holds for the control video and
//!   take model it was written for: a change to either asks for a new one.
//! - A **draft** (where the model has a cheap tier) and a **final** are
//!   each reviewed. A take keeps one of each at a time; a new one replaces
//!   the one before when it arrives.
//! - A used final becomes a **performed clip**: a media file of the
//!   project, linked to its take ([`PerformedClip`]).
//! - A **test** (#243) is a few seconds of one take at the model's
//!   cheapest tier, kept as the capture's one result of step
//!   [`TakeStep::Test`]; an approved test lets the full takes be paid for.

use std::time::{Duration, SystemTime};

use crate::{
    CaptureId, ClipModelRef, JobId, MediaAssetId, Money, NarrationId, TakeStep, TemplateUsed,
    VideoProjectId,
};

/// The longest take prompt, in characters (Kling truncates past it).
pub const MAX_TAKE_PROMPT: usize = 2_500;

/// Why a prompt cannot go to a take model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum TakePromptError {
    #[error("the prompt is empty")]
    Empty,
    #[error("the prompt is over {MAX_TAKE_PROMPT} characters")]
    TooLong,
}

/// `text`, trimmed, if it can be a take's prompt.
pub fn take_prompt_text(text: &str) -> Result<String, TakePromptError> {
    let text = text.trim();
    if text.is_empty() {
        Err(TakePromptError::Empty)
    } else if text.chars().count() > MAX_TAKE_PROMPT {
        Err(TakePromptError::TooLong)
    } else {
        Ok(text.to_owned())
    }
}

/// A take's prompt, as Claude wrote it or the user left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakePrompt {
    pub project: VideoProjectId,
    pub capture: CaptureId,
    pub position: u32,
    /// The fingerprint of the control video it was written for.
    pub control: String,
    /// The take model it was written for: its instruction is the model's.
    pub model: ClipModelRef,
    pub text: String,
    /// The template version Claude wrote it from.
    pub template: Option<TemplateUsed>,
    /// The model that wrote it.
    pub writer: String,
    pub written_at: SystemTime,
    pub job: Option<JobId>,
    /// The user changed Claude's text.
    pub edited: bool,
}

impl TakePrompt {
    /// Whether it was written for the control video `control` and `model`.
    pub fn fits(&self, capture: CaptureId, control: &str, model: &ClipModelRef) -> bool {
        self.capture == capture && self.control == control && self.model == *model
    }
}

/// The user's choice to send a take whose preflight checks failed (#243).
/// It holds for the control video it was given for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightOverride {
    pub project: VideoProjectId,
    pub capture: CaptureId,
    pub position: u32,
    /// The fingerprint of the approved control video it holds for.
    pub control: String,
    pub at: SystemTime,
}

/// What a take model made for a take: a draft or a final, kept in the
/// project folder with how it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeResult {
    pub project: VideoProjectId,
    pub capture: CaptureId,
    pub position: u32,
    pub step: TakeStep,
    /// The fingerprint of the control video it was made from.
    pub control: String,
    pub model: ClipModelRef,
    /// The model and tier as Costs records it (`…/video-edit/480p`).
    pub priced_model: String,
    pub prompt: String,
    pub template: Option<TemplateUsed>,
    /// A fingerprint of the look sent.
    pub look: String,
    /// The look reads as a real person: a performed clip of it needs the
    /// synthetic-content disclosure.
    pub realistic: bool,
    /// The provider's request id.
    pub request: String,
    /// What the provider quoted for it.
    pub cost: Option<Money>,
    /// The MP4 in the project folder.
    pub file: String,
    /// Its real length, which may differ from the take's: models may trim
    /// or pad.
    pub duration: Duration,
    pub made_at: SystemTime,
    pub job: Option<JobId>,
    /// A draft: when the user approved it. A final: when the user last
    /// used it.
    pub accepted_at: Option<SystemTime>,
    /// The seed that made it, when the provider says (ModelArk).
    pub seed: Option<i64>,
    /// A draft whose final finishes it: until when it can be finished.
    pub deadline: Option<SystemTime>,
}

impl TakeResult {
    /// Whether it was made for the control video `control` and `model`.
    pub fn fits(&self, capture: CaptureId, control: &str, model: &ClipModelRef) -> bool {
        self.capture == capture && self.control == control && self.model == *model
    }

    /// Whether it is a draft whose deadline passed at `now`: it can no
    /// longer be finished.
    pub fn expired(&self, now: SystemTime) -> bool {
        self.deadline.is_some_and(|deadline| deadline <= now)
    }

    /// How much longer (positive) or shorter (negative) than `take` it
    /// is, in milliseconds.
    pub fn difference(&self, take: Duration) -> i64 {
        self.duration.as_millis() as i64 - take.as_millis() as i64
    }
}

/// A result this much longer or shorter than its take is pointed out.
pub const LENGTH_TOLERANCE: Duration = Duration::from_millis(100);

/// A used final: the media file it became and the take it performs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformedClip {
    pub asset: MediaAssetId,
    pub project: VideoProjectId,
    pub capture: CaptureId,
    pub position: u32,
    pub model: ClipModelRef,
    /// The look reads as a real person.
    pub realistic: bool,
    pub used_at: SystemTime,
    /// Where it belongs in the narration (#193); `None` for a clip used
    /// before that whose capture was gone.
    pub stretch: Option<TakeStretch>,
    /// Where the performer's face is in its picture, in thousandths
    /// across and down, when its take model keeps the take's framing.
    pub face: Option<(u16, u16)>,
}

/// A take's stretch of the narration it was cut from: a performed clip's
/// first frame belongs to its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeStretch {
    pub narration: NarrationId,
    pub start: Duration,
    pub end: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Provider;

    #[test]
    fn a_prompt_is_trimmed_and_neither_empty_nor_too_long() {
        assert_eq!(take_prompt_text("  Walks in.  "), Ok("Walks in.".into()));
        assert_eq!(take_prompt_text(" \n "), Err(TakePromptError::Empty));
        let long = "é".repeat(MAX_TAKE_PROMPT);
        assert_eq!(take_prompt_text(&long), Ok(long.clone()));
        assert_eq!(
            take_prompt_text(&format!("{long}a")),
            Err(TakePromptError::TooLong)
        );
    }

    #[test]
    fn prompts_and_results_hold_for_their_control_video_and_model() {
        let capture = CaptureId::new();
        let kling = ClipModelRef::new(Provider::Higgsfield, "kling/mc").unwrap();
        let seedance = ClipModelRef::new(Provider::Higgsfield, "seedance/edit").unwrap();
        let prompt = TakePrompt {
            project: VideoProjectId::new(),
            capture,
            position: 0,
            control: "abc".into(),
            model: kling.clone(),
            text: "Walks in.".into(),
            template: None,
            writer: "claude".into(),
            written_at: SystemTime::UNIX_EPOCH,
            job: None,
            edited: false,
        };
        assert!(prompt.fits(capture, "abc", &kling));
        assert!(!prompt.fits(capture, "abd", &kling));
        assert!(!prompt.fits(capture, "abc", &seedance));
        assert!(!prompt.fits(CaptureId::new(), "abc", &kling));

        let result = TakeResult {
            project: prompt.project,
            capture,
            position: 0,
            step: TakeStep::Final,
            control: "abc".into(),
            model: kling.clone(),
            priced_model: "kling/mc".into(),
            prompt: prompt.text.clone(),
            template: None,
            look: "look".into(),
            realistic: false,
            request: "r".into(),
            cost: None,
            file: "take.mp4".into(),
            duration: Duration::from_millis(9_200),
            made_at: SystemTime::UNIX_EPOCH,
            job: None,
            accepted_at: None,
            seed: None,
            deadline: None,
        };
        assert!(result.fits(capture, "abc", &kling));
        assert!(
            !result.expired(SystemTime::now()),
            "no deadline, never expires"
        );
        let draft = TakeResult {
            deadline: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(100)),
            ..result.clone()
        };
        assert!(!draft.expired(SystemTime::UNIX_EPOCH + Duration::from_secs(99)));
        assert!(draft.expired(SystemTime::UNIX_EPOCH + Duration::from_secs(100)));
        assert!(!result.fits(capture, "abc", &seedance));
        assert_eq!(result.difference(Duration::from_secs(9)), 200);
        assert_eq!(result.difference(Duration::from_secs(10)), -800);
    }
}
