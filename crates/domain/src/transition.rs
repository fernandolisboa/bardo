//! Video transitions (issue #167): how the picture goes from one clip of
//! the video track to the next. Without one, a cut: the next clip's first
//! frame follows the last frame of the clip before. With one, the two clips
//! blend, wipe, slide or zoom into each other for a moment.
//!
//! A transition is centered on its cut: the clip before plays on past the
//! cut and the clip after starts before it, for half the transition each.
//! The cut stays where it was, so the video runs as long as it did and
//! stays in sync with the narration. Where a clip's file has no picture
//! past its edge (the start or end of the file), the edge frame holds for
//! that half (ADR-0016).
//!
//! A transition belongs to the clip it leads into, and is kept as the user
//! set it. A clip shorter than its transitions plays them cut short
//! (`crate::Timeline::transition_into`), the way an audio item plays its
//! fades.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use crate::{frame_at, frame_time};

/// How the picture goes from one clip to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TransitionKind {
    /// The next clip fades in over the one before (a dissolve).
    #[default]
    Crossfade,
    /// Down to black, then up into the next clip.
    FadeBlack,
    /// Up to white, then down into the next clip.
    FadeWhite,
    /// The next clip is uncovered by an edge moving to the left.
    WipeLeft,
    /// The next clip is uncovered by an edge moving to the right.
    WipeRight,
    /// The next clip pushes the one before out to the left.
    SlideLeft,
    /// The next clip pushes the one before out to the right.
    SlideRight,
    /// The next clip pushes the one before out at the top.
    SlideUp,
    /// The next clip pushes the one before out at the bottom.
    SlideDown,
    /// The clip before zooms in and gives way to the next.
    ZoomIn,
}

impl TransitionKind {
    pub const ALL: [TransitionKind; 10] = [
        TransitionKind::Crossfade,
        TransitionKind::FadeBlack,
        TransitionKind::FadeWhite,
        TransitionKind::WipeLeft,
        TransitionKind::WipeRight,
        TransitionKind::SlideLeft,
        TransitionKind::SlideRight,
        TransitionKind::SlideUp,
        TransitionKind::SlideDown,
        TransitionKind::ZoomIn,
    ];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            TransitionKind::Crossfade => "crossfade",
            TransitionKind::FadeBlack => "fade_black",
            TransitionKind::FadeWhite => "fade_white",
            TransitionKind::WipeLeft => "wipe_left",
            TransitionKind::WipeRight => "wipe_right",
            TransitionKind::SlideLeft => "slide_left",
            TransitionKind::SlideRight => "slide_right",
            TransitionKind::SlideUp => "slide_up",
            TransitionKind::SlideDown => "slide_down",
            TransitionKind::ZoomIn => "zoom_in",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown transition: {0}")]
pub struct UnknownTransition(pub String);

impl FromStr for TransitionKind {
    type Err = UnknownTransition;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        TransitionKind::ALL
            .into_iter()
            .find(|kind| kind.code() == code)
            .ok_or_else(|| UnknownTransition(code.to_owned()))
    }
}

impl fmt::Display for TransitionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// A transition into a clip from the one before it, and how long it takes:
/// whole frames, within [`Transition::MIN_FRAMES`] and
/// [`Transition::MAX_FRAMES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transition {
    pub kind: TransitionKind,
    frames: u32,
}

impl Transition {
    /// The shortest a transition is set to: 0.1 s.
    pub const MIN_FRAMES: u32 = 3;
    /// The longest: 2 s.
    pub const MAX_FRAMES: u32 = 60;
    /// How far one step of the length moves it: 0.1 s.
    pub const STEP_FRAMES: u32 = 3;
    /// A transition's length unless the user sets another: 0.5 s.
    pub const DEFAULT_FRAMES: u32 = 15;
    /// Cut shorter than this by short clips, a transition plays as a cut:
    /// one frame has nothing to blend.
    pub const PLAYED_MIN_FRAMES: u32 = 2;

    /// `kind` over `frames` frames, kept within the range.
    pub fn new(kind: TransitionKind, frames: u32) -> Transition {
        Transition {
            kind,
            frames: frames.clamp(Self::MIN_FRAMES, Self::MAX_FRAMES),
        }
    }

    /// `kind` over the frames nearest `duration`, kept within the range.
    pub fn lasting(kind: TransitionKind, duration: Duration) -> Transition {
        let frames = u32::try_from(crate::nearest_frame(duration)).unwrap_or(u32::MAX);
        Transition::new(kind, frames)
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn duration(&self) -> Duration {
        frame_time(u64::from(self.frames))
    }

    /// One step longer (`true`) or shorter, within the range.
    pub fn stepped(self, longer: bool) -> Transition {
        let frames = if longer {
            self.frames + Self::STEP_FRAMES
        } else {
            self.frames.saturating_sub(Self::STEP_FRAMES)
        };
        Transition::new(self.kind, frames)
    }

    /// The same length, another kind.
    pub fn with_kind(self, kind: TransitionKind) -> Transition {
        Transition { kind, ..self }
    }
}

impl Default for Transition {
    fn default() -> Self {
        Transition::new(TransitionKind::default(), Self::DEFAULT_FRAMES)
    }
}

/// A transition as it plays between two clips: centered on the cut at
/// `cut`, from `start` to `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayedTransition {
    pub kind: TransitionKind,
    pub start: Duration,
    pub cut: Duration,
    pub end: Duration,
}

impl PlayedTransition {
    /// `frames` of `kind` around the cut at `cut`: the clip after starts
    /// half of them (rounded down) before the cut, the clip before plays on
    /// for the rest.
    pub(crate) fn around(kind: TransitionKind, cut: Duration, frames: u64) -> PlayedTransition {
        let cut_frame = frame_at(cut);
        let before = frames / 2;
        PlayedTransition {
            kind,
            start: frame_time(cut_frame - before),
            cut,
            end: frame_time(cut_frame + frames - before),
        }
    }

    /// How many frames it plays.
    pub fn frames(&self) -> u64 {
        frame_at(self.end) - frame_at(self.start)
    }

    pub fn duration(&self) -> Duration {
        frame_time(self.frames())
    }

    /// Whether `time` falls inside it, past its first frame.
    pub fn covers(&self, time: Duration) -> bool {
        time > self.start && time < self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_go_by_stable_codes() {
        for kind in TransitionKind::ALL {
            assert_eq!(kind.code().parse::<TransitionKind>(), Ok(kind));
        }
        assert!("dissolve".parse::<TransitionKind>().is_err());
    }

    #[test]
    fn a_transition_is_whole_frames_within_its_range() {
        let default = Transition::default();
        assert_eq!(default.kind, TransitionKind::Crossfade);
        assert_eq!(default.frames(), 15);
        assert_eq!(default.duration(), frame_time(15));
        assert_eq!(Transition::new(TransitionKind::ZoomIn, 1).frames(), 3);
        assert_eq!(Transition::new(TransitionKind::ZoomIn, 500).frames(), 60);
        let lasting = Transition::lasting(TransitionKind::WipeLeft, Duration::from_millis(1_010));
        assert_eq!(lasting.frames(), 30);
    }

    #[test]
    fn steps_move_a_tenth_of_a_second_and_stop_at_the_ends() {
        let short = Transition::new(TransitionKind::Crossfade, 3);
        assert_eq!(short.stepped(false), short);
        assert_eq!(short.stepped(true).frames(), 6);
        let long = Transition::new(TransitionKind::Crossfade, 60);
        assert_eq!(long.stepped(true), long);
        assert_eq!(long.stepped(false).frames(), 57);
        assert_eq!(
            long.with_kind(TransitionKind::FadeBlack),
            Transition::new(TransitionKind::FadeBlack, 60)
        );
    }

    #[test]
    fn a_played_transition_straddles_its_cut() {
        let cut = frame_time(90);
        let even = PlayedTransition::around(TransitionKind::Crossfade, cut, 10);
        assert_eq!((even.start, even.end), (frame_time(85), frame_time(95)));
        let odd = PlayedTransition::around(TransitionKind::Crossfade, cut, 15);
        assert_eq!((odd.start, odd.end), (frame_time(83), frame_time(98)));
        assert_eq!(odd.frames(), 15);
        assert_eq!(odd.duration(), frame_time(15));
        assert!(!odd.covers(frame_time(83)));
        assert!(odd.covers(frame_time(84)));
        assert!(odd.covers(frame_time(97)));
        assert!(!odd.covers(frame_time(98)));
    }
}
