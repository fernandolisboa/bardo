//! Production modes (issue #248): how a video is produced, chosen before
//! anything is generated so its whole cost can be estimated first. A mode
//! says which scenes become AI clips and with which model; the other
//! scenes stay stills, moved by the channel's Ken Burns motions (#180).
//!
//! A channel has a default mode and a video project may pick another, as
//! with the format. Channels made before modes existed animate every scene
//! with the channel's clip model, as they did; new channels animate only
//! the hook.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use crate::{ClipModelRef, Provider, VideoFormat};

/// How long the hook runs: the scenes that start within it are animated
/// in [`ProductionMode::AnimatedHook`].
pub const HOOK_LENGTH: Duration = Duration::from_secs(10);

/// About how long a scene of a plan runs: the scene template asks for one
/// every 4 to 8 seconds of narration.
pub const TYPICAL_SCENE: Duration = Duration::from_secs(6);

/// About how many characters a second of narration reads: some 150 words a
/// minute of six characters each, the space included.
pub const NARRATION_CHARS_PER_SECOND: u64 = 15;

/// Which scenes a mode animates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimatedScenes {
    /// None: every scene is a still.
    None,
    /// The scenes that start within [`HOOK_LENGTH`].
    Hook,
    /// Every scene.
    All,
}

/// How a video is produced. The list is data: a new mode is a new entry
/// here, with its scenes and model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProductionMode {
    /// Stills with Ken Burns motions; no AI clips.
    Economy,
    /// Stills, with the hook animated by Kling 2.6 at 720p.
    #[default]
    AnimatedHook,
    /// Every scene animated by Kling 2.6 at 720p.
    AllAnimated,
    /// Every scene animated by Kling 3.0 at 1080p.
    Premium,
    /// Every scene animated by the channel's clip model, as before modes.
    ChannelModel,
}

impl ProductionMode {
    pub const ALL: [ProductionMode; 5] = [
        ProductionMode::Economy,
        ProductionMode::AnimatedHook,
        ProductionMode::AllAnimated,
        ProductionMode::Premium,
        ProductionMode::ChannelModel,
    ];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            ProductionMode::Economy => "economy",
            ProductionMode::AnimatedHook => "hook",
            ProductionMode::AllAnimated => "animated",
            ProductionMode::Premium => "premium",
            ProductionMode::ChannelModel => "channel-model",
        }
    }

    /// Which scenes it animates.
    pub fn animated(self) -> AnimatedScenes {
        match self {
            ProductionMode::Economy => AnimatedScenes::None,
            ProductionMode::AnimatedHook => AnimatedScenes::Hook,
            ProductionMode::AllAnimated
            | ProductionMode::Premium
            | ProductionMode::ChannelModel => AnimatedScenes::All,
        }
    }

    /// Whether it animates a scene that starts at `start`.
    pub fn animates(self, start: Duration) -> bool {
        match self.animated() {
            AnimatedScenes::None => false,
            AnimatedScenes::Hook => start < HOOK_LENGTH,
            AnimatedScenes::All => true,
        }
    }

    /// The model its clips use; `None` uses the channel's (and, for
    /// Economy, makes none).
    pub fn clip_model(self) -> Option<ClipModelRef> {
        let model = match self {
            ProductionMode::AnimatedHook | ProductionMode::AllAnimated => "kling-2.6/720p",
            ProductionMode::Premium => "kling-3.0/1080p",
            ProductionMode::Economy | ProductionMode::ChannelModel => return None,
        };
        Some(ClipModelRef::new(Provider::Kling, model).expect("a valid model"))
    }
}

impl fmt::Display for ProductionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown production mode: {0}")]
pub struct UnknownProductionMode(pub String);

impl FromStr for ProductionMode {
    type Err = UnknownProductionMode;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        ProductionMode::ALL
            .into_iter()
            .find(|mode| mode.code() == code)
            .ok_or_else(|| UnknownProductionMode(code.to_owned()))
    }
}

/// How long a video of `format` runs before its narration exists: the
/// figures the estimates quote, a minute for a Short and eight for a long
/// video.
pub fn typical_length(format: VideoFormat) -> Duration {
    match format {
        VideoFormat::Short => Duration::from_secs(60),
        VideoFormat::Long => Duration::from_secs(8 * 60),
    }
}

/// The scenes a plan of a narration `length` long would about have, as
/// (start, length), one every [`TYPICAL_SCENE`]; the last one takes the
/// rest.
pub fn typical_scenes(length: Duration) -> Vec<(Duration, Duration)> {
    let mut scenes = Vec::new();
    let mut start = Duration::ZERO;
    while start < length {
        let left = length - start;
        // A remainder shorter than half a scene joins the one before.
        let take = if left < TYPICAL_SCENE + TYPICAL_SCENE / 2 {
            left
        } else {
            TYPICAL_SCENE
        };
        scenes.push((start, take));
        start += take;
    }
    scenes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for mode in ProductionMode::ALL {
            assert_eq!(mode.code().parse::<ProductionMode>(), Ok(mode));
        }
        assert!("cheap".parse::<ProductionMode>().is_err());
        assert_eq!(ProductionMode::default(), ProductionMode::AnimatedHook);
    }

    #[test]
    fn the_hook_is_the_scenes_that_start_in_the_first_ten_seconds() {
        let hook = ProductionMode::AnimatedHook;
        assert!(hook.animates(Duration::ZERO));
        assert!(hook.animates(Duration::from_millis(9_999)));
        assert!(!hook.animates(Duration::from_secs(10)));
        assert!(!ProductionMode::Economy.animates(Duration::ZERO));
        assert!(ProductionMode::Premium.animates(Duration::from_secs(300)));
    }

    #[test]
    fn modes_pick_kling_direct_or_leave_the_channel_model() {
        let model = |mode: ProductionMode| mode.clip_model().map(|m| m.model().to_owned());
        assert_eq!(model(ProductionMode::Economy), None);
        assert_eq!(model(ProductionMode::ChannelModel), None);
        assert_eq!(
            model(ProductionMode::AnimatedHook).as_deref(),
            Some("kling-2.6/720p")
        );
        assert_eq!(
            model(ProductionMode::AllAnimated).as_deref(),
            Some("kling-2.6/720p")
        );
        assert_eq!(
            model(ProductionMode::Premium).as_deref(),
            Some("kling-3.0/1080p")
        );
        assert!(
            ProductionMode::ALL
                .iter()
                .filter_map(|mode| mode.clip_model())
                .all(|model| model.provider() == Provider::Kling)
        );
    }

    #[test]
    fn a_minute_is_ten_scenes_and_a_remainder_joins_the_last() {
        let scenes = typical_scenes(Duration::from_secs(60));
        assert_eq!(scenes.len(), 10);
        assert_eq!(scenes[1], (Duration::from_secs(6), Duration::from_secs(6)));
        let odd = typical_scenes(Duration::from_secs(14));
        assert_eq!(
            odd,
            [
                (Duration::ZERO, Duration::from_secs(6)),
                (Duration::from_secs(6), Duration::from_secs(8)),
            ]
        );
        assert!(typical_scenes(Duration::ZERO).is_empty());
        assert_eq!(typical_scenes(typical_length(VideoFormat::Long)).len(), 80);
    }
}
