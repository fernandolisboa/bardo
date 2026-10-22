//! Video format (issue #179): whether a video is a Short or a long video.
//! A channel has a default format and a video project may pick the other.
//! The format says how long the script runs, which shape the pictures are
//! drawn in, the shape the rough cut starts in and which YouTube preset a
//! render uses (ADR-0018).

use std::fmt;
use std::str::FromStr;

use crate::AspectRatio;

/// What kind of video a project makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VideoFormat {
    /// A vertical video of under a minute or so: a YouTube Short, a TikTok,
    /// a Reel. Fits every network.
    #[default]
    Short,
    /// A landscape video of several minutes, for YouTube (and Kick).
    Long,
}

impl VideoFormat {
    pub const ALL: [VideoFormat; 2] = [VideoFormat::Short, VideoFormat::Long];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            VideoFormat::Short => "short",
            VideoFormat::Long => "long",
        }
    }

    /// The frame shape its pictures are drawn in and its cut starts in.
    pub fn aspect(self) -> AspectRatio {
        match self {
            VideoFormat::Short => AspectRatio::Vertical,
            VideoFormat::Long => AspectRatio::Landscape,
        }
    }

    /// How long its script runs, as the script template's
    /// `{{target_length}}` says it to the model. About 150 spoken words a
    /// minute.
    pub fn target_length(self) -> &'static str {
        match self {
            VideoFormat::Short => "about 110 to 150 words (45 to 60 seconds spoken)",
            VideoFormat::Long => "about 1,200 to 1,500 words (8 to 10 minutes spoken)",
        }
    }

    /// The shape of its pictures, as the scene template's `{{frame_shape}}`
    /// says it to the model.
    pub fn frame_shape(self) -> &'static str {
        match self {
            VideoFormat::Short => "tall (9:16, vertical)",
            VideoFormat::Long => "wide (16:9)",
        }
    }
}

impl fmt::Display for VideoFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown video format: {0}")]
pub struct UnknownVideoFormat(pub String);

impl FromStr for VideoFormat {
    type Err = UnknownVideoFormat;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        VideoFormat::ALL
            .into_iter()
            .find(|format| format.code() == code)
            .ok_or_else(|| UnknownVideoFormat(code.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for format in VideoFormat::ALL {
            assert_eq!(format.code().parse::<VideoFormat>(), Ok(format));
        }
        assert!("reel".parse::<VideoFormat>().is_err());
    }

    #[test]
    fn a_short_is_vertical_and_a_long_video_landscape() {
        assert_eq!(VideoFormat::Short.aspect(), AspectRatio::Vertical);
        assert_eq!(VideoFormat::Long.aspect(), AspectRatio::Landscape);
    }

    #[test]
    fn a_short_script_runs_under_a_minute_and_a_long_one_minutes() {
        assert!(VideoFormat::Short.target_length().contains("60 seconds"));
        assert!(VideoFormat::Long.target_length().contains("10 minutes"));
        assert!(VideoFormat::Short.frame_shape().contains("9:16"));
        assert!(VideoFormat::Long.frame_shape().contains("16:9"));
    }
}
