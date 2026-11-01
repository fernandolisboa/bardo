//! Video format (issue #179): whether a video is a Short or a long video.
//! A channel has a default format and a video project may pick the other.
//! The format says how long the script runs, which shape the pictures are
//! drawn in, the shape the rough cut starts in and which YouTube preset a
//! render uses (ADR-0018).
//!
//! A project may also pick a length (issue #253), which overrides only how
//! long its script runs: the format keeps the shape, the cut and the
//! preset.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

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

/// How long a video runs, when its project picks a length instead of its
/// format's. Every length stays under the 3:00 that YouTube Shorts and
/// Instagram Reels take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoLength {
    /// A quick Reel or Short.
    Seconds30,
    /// A Short or Reel that fits every network.
    Minute1,
    /// The longest Short or Reel, or a short landscape video.
    Minutes3,
}

impl VideoLength {
    pub const ALL: [VideoLength; 3] = [
        VideoLength::Seconds30,
        VideoLength::Minute1,
        VideoLength::Minutes3,
    ];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            VideoLength::Seconds30 => "30s",
            VideoLength::Minute1 => "1m",
            VideoLength::Minutes3 => "3m",
        }
    }

    /// How long it runs at most.
    pub fn duration(self) -> Duration {
        Duration::from_secs(match self {
            VideoLength::Seconds30 => 30,
            VideoLength::Minute1 => 60,
            VideoLength::Minutes3 => 180,
        })
    }

    /// How long its script runs, as the script template's
    /// `{{target_length}}` says it to the model: about 150 spoken words a
    /// minute, a little under the length so the narration fits.
    pub fn target_length(self) -> &'static str {
        match self {
            VideoLength::Seconds30 => "about 60 to 75 words (25 to 30 seconds spoken)",
            VideoLength::Minute1 => "about 130 to 150 words (50 to 60 seconds spoken)",
            VideoLength::Minutes3 => {
                "about 400 to 440 words (2 minutes 40 seconds to just under 3 minutes spoken)"
            }
        }
    }
}

impl fmt::Display for VideoLength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown video length: {0}")]
pub struct UnknownVideoLength(pub String);

impl FromStr for VideoLength {
    type Err = UnknownVideoLength;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        VideoLength::ALL
            .into_iter()
            .find(|length| length.code() == code)
            .ok_or_else(|| UnknownVideoLength(code.to_owned()))
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

    #[test]
    fn lengths_round_trip_and_stay_under_three_minutes() {
        for length in VideoLength::ALL {
            assert_eq!(length.code().parse::<VideoLength>(), Ok(length));
            assert!(length.duration() <= Duration::from_secs(180));
        }
        assert!("2m".parse::<VideoLength>().is_err());
        assert!(
            VideoLength::Seconds30
                .target_length()
                .contains("30 seconds")
        );
        assert!(
            VideoLength::Minutes3
                .target_length()
                .contains("under 3 minutes")
        );
    }
}
