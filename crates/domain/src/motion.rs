//! Motion on stills (issue #180), the Ken Burns effect: a slow zoom or pan
//! across a still picture while it shows, so a video made mostly of stills
//! does not play as a slideshow of frozen frames.
//!
//! A motion moves a view over the clip's picture as framed (`crate::Framing`):
//! the view is a window of the frame's shape, as large as the frame at zoom
//! 1 and smaller as it zooms in, placed across the room the zoom leaves. A
//! zoom keeps the view in the middle; a pan holds the zoom of its strength
//! and travels from one edge of the picture to the other. The view moves
//! over the clip's whole time on screen, transitions included, so a clip
//! never stops moving while it shows.
//!
//! A motion belongs to its clip and plays on stills only: a clip of a scene
//! that gets animated keeps its motion unplayed, and footage has none. A
//! channel's default ([`MotionDefault`]) gives each new rough cut its
//! motions, varied by default so consecutive scenes move differently.

use std::fmt;
use std::str::FromStr;

/// Which way a view moves over a still.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionKind {
    /// From the whole picture in to its middle.
    ZoomIn,
    /// From the middle out to the whole picture.
    ZoomOut,
    /// The view travels to the left, from the right edge to the left one.
    PanLeft,
    /// The view travels to the right.
    PanRight,
    /// The view travels up, from the bottom edge to the top one.
    PanUp,
    /// The view travels down.
    PanDown,
}

impl MotionKind {
    pub const ALL: [MotionKind; 6] = [
        MotionKind::ZoomIn,
        MotionKind::ZoomOut,
        MotionKind::PanLeft,
        MotionKind::PanRight,
        MotionKind::PanUp,
        MotionKind::PanDown,
    ];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            MotionKind::ZoomIn => "zoom_in",
            MotionKind::ZoomOut => "zoom_out",
            MotionKind::PanLeft => "pan_left",
            MotionKind::PanRight => "pan_right",
            MotionKind::PanUp => "pan_up",
            MotionKind::PanDown => "pan_down",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown motion: {0}")]
pub struct UnknownMotion(pub String);

impl FromStr for MotionKind {
    type Err = UnknownMotion;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        MotionKind::ALL
            .into_iter()
            .find(|kind| kind.code() == code)
            .ok_or_else(|| UnknownMotion(code.to_owned()))
    }
}

impl fmt::Display for MotionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// How far a motion goes: how much it zooms, and so how much room a pan
/// has to travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MotionStrength {
    /// 8% closer: barely there.
    Subtle,
    /// 15% closer.
    #[default]
    Medium,
    /// 25% closer.
    Strong,
}

impl MotionStrength {
    pub const ALL: [MotionStrength; 3] = [
        MotionStrength::Subtle,
        MotionStrength::Medium,
        MotionStrength::Strong,
    ];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            MotionStrength::Subtle => "subtle",
            MotionStrength::Medium => "medium",
            MotionStrength::Strong => "strong",
        }
    }

    /// How much closer the view gets than the whole picture: 1.15 shows
    /// the picture 15% larger.
    pub fn zoom(self) -> f64 {
        match self {
            MotionStrength::Subtle => 1.08,
            MotionStrength::Medium => 1.15,
            MotionStrength::Strong => 1.25,
        }
    }
}

impl FromStr for MotionStrength {
    type Err = UnknownMotion;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        MotionStrength::ALL
            .into_iter()
            .find(|strength| strength.code() == code)
            .ok_or_else(|| UnknownMotion(code.to_owned()))
    }
}

/// A clip's motion: which way, and how far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Motion {
    pub kind: MotionKind,
    pub strength: MotionStrength,
}

impl Motion {
    pub fn new(kind: MotionKind, strength: MotionStrength) -> Motion {
        Motion { kind, strength }
    }

    /// Where the view starts and where it ends.
    pub fn views(self) -> (MotionView, MotionView) {
        let zoom = self.strength.zoom();
        let whole = MotionView::new(1.0, 0.5, 0.5);
        let close = |x: f64, y: f64| MotionView::new(zoom, x, y);
        match self.kind {
            MotionKind::ZoomIn => (whole, close(0.5, 0.5)),
            MotionKind::ZoomOut => (close(0.5, 0.5), whole),
            MotionKind::PanLeft => (close(1.0, 0.5), close(0.0, 0.5)),
            MotionKind::PanRight => (close(0.0, 0.5), close(1.0, 0.5)),
            MotionKind::PanUp => (close(0.5, 1.0), close(0.5, 0.0)),
            MotionKind::PanDown => (close(0.5, 0.0), close(0.5, 1.0)),
        }
    }

    /// The view `progress` of the way through (0.0 at the start, 1.0 at
    /// the end; kept within).
    pub fn view_at(self, progress: f64) -> MotionView {
        let (from, to) = self.views();
        from.toward(to, progress)
    }

    /// The same motion, another way.
    pub fn with_kind(self, kind: MotionKind) -> Motion {
        Motion { kind, ..self }
    }

    /// The same way, another strength.
    pub fn with_strength(self, strength: MotionStrength) -> Motion {
        Motion { strength, ..self }
    }
}

/// What a motion shows at one moment: a window of the frame's shape,
/// `zoom` times smaller than the framed picture, placed `x` across (0.0 at
/// the left edge, 1.0 at the right) and `y` down the room it leaves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionView {
    pub zoom: f64,
    pub x: f64,
    pub y: f64,
}

impl MotionView {
    pub const fn new(zoom: f64, x: f64, y: f64) -> MotionView {
        MotionView { zoom, x, y }
    }

    /// The view `progress` of the way to `to`: the zoom changes by the same
    /// ratio each frame, so a zoom looks as fast at its end as at its
    /// start, and the place moves at an even pace.
    pub fn toward(self, to: MotionView, progress: f64) -> MotionView {
        let progress = progress.clamp(0.0, 1.0);
        MotionView {
            zoom: self.zoom * (to.zoom / self.zoom).powf(progress),
            x: self.x + (to.x - self.x) * progress,
            y: self.y + (to.y - self.y) * progress,
        }
    }
}

/// Which motions a new rough cut gives its scenes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MotionPattern {
    /// None: stills stay still.
    None,
    /// A different motion from one scene to the next ([`MOTION_VARIED`]).
    #[default]
    Varied,
    /// The same motion on every scene.
    Same(MotionKind),
}

/// The motions a varied pattern goes through, in turn: a zoom, then a pan,
/// each the other way from the last of its kind.
pub const MOTION_VARIED: [MotionKind; 4] = [
    MotionKind::ZoomIn,
    MotionKind::PanRight,
    MotionKind::ZoomOut,
    MotionKind::PanLeft,
];

impl MotionPattern {
    /// Every pattern, as a channel offers them.
    pub fn all() -> Vec<MotionPattern> {
        [MotionPattern::None, MotionPattern::Varied]
            .into_iter()
            .chain(MotionKind::ALL.map(MotionPattern::Same))
            .collect()
    }

    /// The stable code it is stored under: a kind's own code for the same
    /// motion everywhere.
    pub fn code(self) -> &'static str {
        match self {
            MotionPattern::None => "none",
            MotionPattern::Varied => "varied",
            MotionPattern::Same(kind) => kind.code(),
        }
    }

    /// The motion the scene at `position` of a cut gets.
    pub fn kind_at(self, position: usize) -> Option<MotionKind> {
        match self {
            MotionPattern::None => None,
            MotionPattern::Varied => Some(MOTION_VARIED[position % MOTION_VARIED.len()]),
            MotionPattern::Same(kind) => Some(kind),
        }
    }
}

impl FromStr for MotionPattern {
    type Err = UnknownMotion;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        match code {
            "none" => Ok(MotionPattern::None),
            "varied" => Ok(MotionPattern::Varied),
            kind => kind.parse().map(MotionPattern::Same),
        }
    }
}

/// A channel's motions for new rough cuts: the pattern, at one strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MotionDefault {
    pub pattern: MotionPattern,
    pub strength: MotionStrength,
}

impl MotionDefault {
    pub fn new(pattern: MotionPattern, strength: MotionStrength) -> MotionDefault {
        MotionDefault { pattern, strength }
    }

    /// The motion the scene at `position` of a new cut gets.
    pub fn at(self, position: usize) -> Option<Motion> {
        self.pattern
            .kind_at(position)
            .map(|kind| Motion::new(kind, self.strength))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn kinds_strengths_and_patterns_go_by_stable_codes() {
        for kind in MotionKind::ALL {
            assert_eq!(kind.code().parse::<MotionKind>(), Ok(kind));
        }
        for strength in MotionStrength::ALL {
            assert_eq!(strength.code().parse::<MotionStrength>(), Ok(strength));
        }
        for pattern in MotionPattern::all() {
            assert_eq!(pattern.code().parse::<MotionPattern>(), Ok(pattern));
        }
        assert_eq!(MotionPattern::all().len(), 8);
        assert!("spin".parse::<MotionKind>().is_err());
        assert!("spin".parse::<MotionPattern>().is_err());
        assert!("wild".parse::<MotionStrength>().is_err());
    }

    #[test]
    fn a_zoom_stays_in_the_middle_and_a_pan_crosses_at_its_zoom() {
        let zoom_in = Motion::new(MotionKind::ZoomIn, MotionStrength::Medium);
        assert_eq!(
            zoom_in.views(),
            (
                MotionView::new(1.0, 0.5, 0.5),
                MotionView::new(1.15, 0.5, 0.5)
            )
        );
        let (from, to) = zoom_in.with_kind(MotionKind::ZoomOut).views();
        assert_eq!((from.zoom, to.zoom), (1.15, 1.0));
        let pan = Motion::new(MotionKind::PanLeft, MotionStrength::Strong);
        assert_eq!(
            pan.views(),
            (
                MotionView::new(1.25, 1.0, 0.5),
                MotionView::new(1.25, 0.0, 0.5)
            )
        );
        let (from, to) = pan.with_kind(MotionKind::PanDown).views();
        assert_eq!((from.y, to.y, from.x), (0.0, 1.0, 0.5));
    }

    #[test]
    fn a_zoom_changes_by_the_same_ratio_each_step_and_a_pan_by_the_same_distance() {
        let zoom_in = Motion::new(MotionKind::ZoomIn, MotionStrength::Strong);
        let half = zoom_in.view_at(0.5);
        assert!(close(half.zoom, 1.25_f64.sqrt()), "{half:?}");
        assert!(close(zoom_in.view_at(1.0).zoom, 1.25));
        assert!(close(zoom_in.view_at(4.0).zoom, 1.25), "kept within");
        assert!(close(zoom_in.view_at(-1.0).zoom, 1.0));
        let pan = Motion::new(MotionKind::PanRight, MotionStrength::Subtle);
        assert!(close(pan.view_at(0.25).x, 0.25));
        assert!(close(pan.view_at(0.25).zoom, 1.08));
    }

    #[test]
    fn a_varied_pattern_never_repeats_a_motion_on_the_next_scene() {
        let default = MotionDefault::default();
        assert_eq!(default.pattern, MotionPattern::Varied);
        assert_eq!(default.strength, MotionStrength::Medium);
        let kinds: Vec<MotionKind> = (0..9).map(|n| default.at(n).unwrap().kind).collect();
        assert!(kinds.windows(2).all(|pair| pair[0] != pair[1]), "{kinds:?}");
        assert_eq!(kinds[0], MotionKind::ZoomIn);
        assert_eq!(kinds[4], MotionKind::ZoomIn);
        let same = MotionDefault::new(
            MotionPattern::Same(MotionKind::PanUp),
            MotionStrength::Subtle,
        );
        assert_eq!(
            same.at(7),
            Some(Motion::new(MotionKind::PanUp, MotionStrength::Subtle))
        );
        let none = MotionDefault::new(MotionPattern::None, MotionStrength::Strong);
        assert_eq!(none.at(0), None);
    }
}
