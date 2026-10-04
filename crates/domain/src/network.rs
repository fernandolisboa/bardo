//! The networks Bardo makes videos for, and the render preset each one gets
//! unless a network account overrides it.
//!
//! Built-in presets are data, checked against each network's published
//! upload rules on 2026-10-02. The MVP aims at short vertical videos
//! (Shorts, TikTok, Reels), so every network but Kick defaults to 9:16.

use std::fmt;
use std::str::FromStr;

/// A network a channel can have an account on (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Network {
    YouTube,
    TikTok,
    InstagramReels,
    X,
    Kick,
}

impl Network {
    pub const ALL: [Network; 5] = [
        Network::YouTube,
        Network::TikTok,
        Network::InstagramReels,
        Network::X,
        Network::Kick,
    ];

    /// Stable identifier for storage.
    pub fn code(self) -> &'static str {
        match self {
            Network::YouTube => "youtube",
            Network::TikTok => "tiktok",
            Network::InstagramReels => "instagram_reels",
            Network::X => "x",
            Network::Kick => "kick",
        }
    }

    /// Who can see a post, among the options the network offers. The first
    /// one is the default.
    ///
    /// - YouTube: public, unlisted or private.
    /// - TikTok: public or only the creator (`SELF_ONLY` in the Content
    ///   Posting API).
    /// - Instagram Reels, X and Kick: posts are public.
    pub fn visibilities(self) -> &'static [Visibility] {
        match self {
            Network::YouTube => &[
                Visibility::Public,
                Visibility::Unlisted,
                Visibility::Private,
            ],
            Network::TikTok => &[Visibility::Public, Visibility::Private],
            Network::InstagramReels | Network::X | Network::Kick => &[Visibility::Public],
        }
    }

    /// Whether an upload can carry a publish time the network keeps, so
    /// the network publishes the video by itself (YouTube's `publishAt`).
    /// Instagram has no such field: its posts wait for the in-app
    /// scheduler (ADR-0006).
    pub fn schedules_uploads(self) -> bool {
        self == Network::YouTube
    }

    /// Whether an upload declares whether the video is made for kids
    /// (YouTube's `selfDeclaredMadeForKids`).
    pub fn asks_made_for_kids(self) -> bool {
        self == Network::YouTube
    }

    /// Whether an upload is a Reel: it picks its cover frame and whether it
    /// also shows in the feed, and the file must meet the Reel specs.
    pub fn uploads_reels(self) -> bool {
        self == Network::InstagramReels
    }

    /// The preset a render for this network uses unless the account
    /// overrides it.
    ///
    /// - YouTube: a Short, 9:16 1080×1920, up to 3 minutes. 12 Mbps is
    ///   YouTube's recommended upload bitrate for 1080p at 60 fps.
    /// - TikTok: 9:16 1080×1920, up to 10 minutes (the app's limit; the web
    ///   uploader takes longer videos).
    /// - Instagram Reels: 9:16 1080×1920, up to 15 minutes (the Content
    ///   Publishing API's limit; it accepts up to 25 Mbps).
    /// - X: without a paid plan, videos are up to 2:20 and 1200×1900, so a
    ///   vertical video is 720×1280 (X's recommended portrait size).
    /// - Kick: no video upload exists, so the export matches its 16:9
    ///   1080p player and 8 Mbps ingest ceiling, with no limit of its own.
    ///
    /// No network publishes an official loudness target; −14 LUFS
    /// integrated is the level they are widely measured to normalize to.
    pub fn render_preset(self) -> RenderPreset {
        let short = |bitrate_kbps: u32, max_seconds: u32| RenderPreset {
            aspect: AspectRatio::Vertical,
            resolution: Resolution::FullHd1080,
            codec: VideoCodec::H264,
            bitrate: Bitrate(bitrate_kbps),
            max_duration: MaxDuration(max_seconds),
            loudness: Loudness(-140),
        };
        match self {
            Network::YouTube => short(12_000, 3 * 60),
            Network::TikTok => short(10_000, 10 * 60),
            Network::InstagramReels => short(10_000, 15 * 60),
            Network::X => RenderPreset {
                resolution: Resolution::Hd720,
                ..short(6_000, 2 * 60 + 20)
            },
            Network::Kick => RenderPreset {
                aspect: AspectRatio::Landscape,
                ..short(8_000, MaxDuration::MAX_SECONDS)
            },
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown network: {0}")]
pub struct UnknownNetwork(pub String);

impl FromStr for Network {
    type Err = UnknownNetwork;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Network::ALL
            .into_iter()
            .find(|network| network.code() == s)
            .ok_or_else(|| UnknownNetwork(s.to_owned()))
    }
}

/// Who can see a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Visibility {
    #[default]
    Public,
    /// Anyone with the link, not listed anywhere.
    Unlisted,
    /// Only the account owner.
    Private,
}

impl Visibility {
    pub const ALL: [Visibility; 3] = [
        Visibility::Public,
        Visibility::Unlisted,
        Visibility::Private,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Unlisted => "unlisted",
            Visibility::Private => "private",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown visibility: {0}")]
pub struct UnknownVisibility(pub String);

impl FromStr for Visibility {
    type Err = UnknownVisibility;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Visibility::ALL
            .into_iter()
            .find(|visibility| visibility.code() == s)
            .ok_or_else(|| UnknownVisibility(s.to_owned()))
    }
}

/// The shape of the frame. The editor works in 16:9 and 9:16 (see
/// docs/design/editor.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AspectRatio {
    /// 9:16, for Shorts, TikTok and Reels.
    Vertical,
    /// 16:9.
    Landscape,
}

impl AspectRatio {
    pub const ALL: [AspectRatio; 2] = [AspectRatio::Vertical, AspectRatio::Landscape];

    pub fn code(self) -> &'static str {
        match self {
            AspectRatio::Vertical => "9:16",
            AspectRatio::Landscape => "16:9",
        }
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown aspect ratio: {0}")]
pub struct UnknownAspectRatio(pub String);

impl FromStr for AspectRatio {
    type Err = UnknownAspectRatio;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AspectRatio::ALL
            .into_iter()
            .find(|aspect| aspect.code() == s)
            .ok_or_else(|| UnknownAspectRatio(s.to_owned()))
    }
}

/// Output size, named by the frame's short side so it holds for either
/// aspect ratio: 1080p is 1080×1920 vertical and 1920×1080 landscape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resolution {
    Hd720,
    FullHd1080,
    Qhd1440,
    Uhd2160,
}

impl Resolution {
    pub const ALL: [Resolution; 4] = [
        Resolution::Hd720,
        Resolution::FullHd1080,
        Resolution::Qhd1440,
        Resolution::Uhd2160,
    ];

    /// The frame's short side in pixels.
    pub fn short_side(self) -> u32 {
        match self {
            Resolution::Hd720 => 720,
            Resolution::FullHd1080 => 1080,
            Resolution::Qhd1440 => 1440,
            Resolution::Uhd2160 => 2160,
        }
    }

    /// Width and height in pixels for `aspect`.
    pub fn dimensions(self, aspect: AspectRatio) -> (u32, u32) {
        let short = self.short_side();
        let long = short * 16 / 9;
        match aspect {
            AspectRatio::Vertical => (short, long),
            AspectRatio::Landscape => (long, short),
        }
    }

    pub fn from_short_side(pixels: u32) -> Option<Self> {
        Resolution::ALL
            .into_iter()
            .find(|resolution| resolution.short_side() == pixels)
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}p", self.short_side())
    }
}

/// Video codec of the render. Both have NVENC encoders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    H264,
    Hevc,
}

impl VideoCodec {
    pub const ALL: [VideoCodec; 2] = [VideoCodec::H264, VideoCodec::Hevc];

    pub fn code(self) -> &'static str {
        match self {
            VideoCodec::H264 => "h264",
            VideoCodec::Hevc => "hevc",
        }
    }

    /// The name people know the codec by.
    pub fn name(self) -> &'static str {
        match self {
            VideoCodec::H264 => "H.264",
            VideoCodec::Hevc => "HEVC",
        }
    }
}

impl fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown video codec: {0}")]
pub struct UnknownVideoCodec(pub String);

impl FromStr for VideoCodec {
    type Err = UnknownVideoCodec;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        VideoCodec::ALL
            .into_iter()
            .find(|codec| codec.code() == s)
            .ok_or_else(|| UnknownVideoCodec(s.to_owned()))
    }
}

/// A preset value outside the range Bardo accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("preset value out of range")]
pub struct OutOfRange;

/// Video bitrate. Typed and shown in Mbps (`12`, `7.5`), kept in kbps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Bitrate(u32);

impl Bitrate {
    pub const MIN_KBPS: u32 = 500;
    pub const MAX_KBPS: u32 = 100_000;

    pub fn from_kbps(kbps: u32) -> Result<Self, OutOfRange> {
        if (Self::MIN_KBPS..=Self::MAX_KBPS).contains(&kbps) {
            Ok(Self(kbps))
        } else {
            Err(OutOfRange)
        }
    }

    pub fn kbps(self) -> u32 {
        self.0
    }
}

/// Mbps without trailing zeros: `12`, `7.5`, `0.75`.
impl fmt::Display for Bitrate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&decimal(i64::from(self.0), 3))
    }
}

/// Parses Mbps with up to three decimals, with `.` or `,` as separator.
impl FromStr for Bitrate {
    type Err = OutOfRange;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let kbps = parse_decimal(s, 3).ok_or(OutOfRange)?;
        Self::from_kbps(u32::try_from(kbps).map_err(|_| OutOfRange)?)
    }
}

/// The longest video a network takes. Typed and shown as `m:ss` or
/// `h:mm:ss`; a bare number is seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaxDuration(u32);

impl MaxDuration {
    /// Twelve hours, YouTube's own ceiling for a verified account.
    pub const MAX_SECONDS: u32 = 12 * 60 * 60;

    pub fn from_seconds(seconds: u32) -> Result<Self, OutOfRange> {
        if (1..=Self::MAX_SECONDS).contains(&seconds) {
            Ok(Self(seconds))
        } else {
            Err(OutOfRange)
        }
    }

    pub fn seconds(self) -> u32 {
        self.0
    }
}

impl fmt::Display for MaxDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (hours, minutes, seconds) = (self.0 / 3600, self.0 / 60 % 60, self.0 % 60);
        if hours > 0 {
            write!(f, "{hours}:{minutes:02}:{seconds:02}")
        } else {
            write!(f, "{minutes}:{seconds:02}")
        }
    }
}

impl FromStr for MaxDuration {
    type Err = OutOfRange;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.trim().split(':').collect();
        if parts.len() > 3 {
            return Err(OutOfRange);
        }
        let mut seconds: u32 = 0;
        for (index, part) in parts.iter().enumerate() {
            let value = digits(part).ok_or(OutOfRange)?;
            // Every part after the first is minutes or seconds of a clock.
            if index > 0 && (part.len() != 2 || value >= 60) {
                return Err(OutOfRange);
            }
            seconds = seconds
                .checked_mul(60)
                .and_then(|s| s.checked_add(value))
                .ok_or(OutOfRange)?;
        }
        Self::from_seconds(seconds)
    }
}

/// Target integrated loudness. Typed and shown in LUFS (`-14`, `-13.5`),
/// kept in tenths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Loudness(i16);

impl Loudness {
    /// −30 to −5 LUFS.
    pub const MIN_TENTHS: i16 = -300;
    pub const MAX_TENTHS: i16 = -50;

    pub fn from_tenths(tenths: i16) -> Result<Self, OutOfRange> {
        if (Self::MIN_TENTHS..=Self::MAX_TENTHS).contains(&tenths) {
            Ok(Self(tenths))
        } else {
            Err(OutOfRange)
        }
    }

    pub fn tenths(self) -> i16 {
        self.0
    }

    pub fn lufs(self) -> f64 {
        f64::from(self.0) / 10.0
    }
}

impl fmt::Display for Loudness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&decimal(i64::from(self.0), 1))
    }
}

/// Parses LUFS with up to one decimal. Accepts the typographic minus.
impl FromStr for Loudness {
    type Err = OutOfRange;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let tenths = parse_decimal(&s.replace('−', "-"), 1).ok_or(OutOfRange)?;
        Self::from_tenths(i16::try_from(tenths).map_err(|_| OutOfRange)?)
    }
}

/// `value` scaled down by `10^places`, without trailing zeros.
fn decimal(value: i64, places: u32) -> String {
    let scale = 10_i64.pow(places);
    let sign = if value < 0 { "-" } else { "" };
    let (whole, fraction) = (value.abs() / scale, value.abs() % scale);
    if fraction == 0 {
        return format!("{sign}{whole}");
    }
    let fraction = format!("{fraction:0width$}", width = places as usize);
    format!("{sign}{whole}.{}", fraction.trim_end_matches('0'))
}

/// A decimal number scaled up by `10^places`, rejecting more decimals than
/// `places`. Either `.` or `,` separates decimals.
fn parse_decimal(text: &str, places: u32) -> Option<i64> {
    let text = text.trim();
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = match unsigned.split_once(['.', ',']) {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (unsigned, None),
    };
    let whole = i64::from(digits(whole)?);
    let fraction = match fraction {
        None => 0,
        Some(fraction) if fraction.len() <= places as usize => {
            i64::from(digits(fraction)?) * 10_i64.pow(places - fraction.len() as u32)
        }
        Some(_) => return None,
    };
    let value = whole
        .checked_mul(10_i64.pow(places))?
        .checked_add(fraction)?;
    Some(if negative { -value } else { value })
}

/// An unsigned number of ASCII digits only (no sign, no spaces).
fn digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The output format of a render for one network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderPreset {
    pub aspect: AspectRatio,
    pub resolution: Resolution,
    pub codec: VideoCodec,
    pub bitrate: Bitrate,
    pub max_duration: MaxDuration,
    pub loudness: Loudness,
}

impl RenderPreset {
    /// Width and height in pixels.
    pub fn dimensions(&self) -> (u32, u32) {
        self.resolution.dimensions(self.aspect)
    }

    /// This preset with every value the overrides set replaced.
    pub fn with(self, overrides: &PresetOverrides) -> Self {
        Self {
            aspect: overrides.aspect.unwrap_or(self.aspect),
            resolution: overrides.resolution.unwrap_or(self.resolution),
            codec: overrides.codec.unwrap_or(self.codec),
            bitrate: overrides.bitrate.unwrap_or(self.bitrate),
            max_duration: overrides.max_duration.unwrap_or(self.max_duration),
            loudness: overrides.loudness.unwrap_or(self.loudness),
        }
    }
}

/// The preset values a network account sets for itself. `None` keeps the
/// network's built-in value, so a later change to the built-in preset
/// reaches every account that did not override it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PresetOverrides {
    pub aspect: Option<AspectRatio>,
    pub resolution: Option<Resolution>,
    pub codec: Option<VideoCodec>,
    pub bitrate: Option<Bitrate>,
    pub max_duration: Option<MaxDuration>,
    pub loudness: Option<Loudness>,
}

impl PresetOverrides {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn codes_round_trip_and_are_unique() {
        for network in Network::ALL {
            assert_eq!(network.code().parse(), Ok(network));
        }
        for visibility in Visibility::ALL {
            assert_eq!(visibility.code().parse(), Ok(visibility));
        }
        for aspect in AspectRatio::ALL {
            assert_eq!(aspect.code().parse(), Ok(aspect));
        }
        for codec in VideoCodec::ALL {
            assert_eq!(codec.code().parse(), Ok(codec));
        }
        for resolution in Resolution::ALL {
            assert_eq!(
                Resolution::from_short_side(resolution.short_side()),
                Some(resolution)
            );
        }
        let networks: HashSet<_> = Network::ALL.map(Network::code).into();
        assert_eq!(networks.len(), Network::ALL.len());
        assert!("facebook".parse::<Network>().is_err());
    }

    #[test]
    fn short_form_networks_default_to_vertical_and_kick_to_landscape() {
        for network in [
            Network::YouTube,
            Network::TikTok,
            Network::InstagramReels,
            Network::X,
        ] {
            assert_eq!(network.render_preset().aspect, AspectRatio::Vertical);
        }
        assert_eq!(Network::Kick.render_preset().aspect, AspectRatio::Landscape);
    }

    #[test]
    fn built_in_presets_follow_each_networks_upload_rules() {
        let youtube = Network::YouTube.render_preset();
        assert_eq!(youtube.dimensions(), (1080, 1920));
        assert_eq!(
            youtube.max_duration.seconds(),
            180,
            "Shorts are up to 3 min"
        );

        assert_eq!(Network::TikTok.render_preset().max_duration.seconds(), 600);

        let reels = Network::InstagramReels.render_preset();
        assert_eq!(reels.max_duration.seconds(), 900);
        assert!(reels.bitrate.kbps() <= 25_000, "Reels take up to 25 Mbps");

        let x = Network::X.render_preset();
        let (width, height) = x.dimensions();
        assert!(width <= 1200 && height <= 1900, "X takes up to 1200×1900");
        assert_eq!(x.max_duration.seconds(), 140);
        assert!(x.bitrate.kbps() <= 25_000);

        let kick = Network::Kick.render_preset();
        assert_eq!(kick.dimensions(), (1920, 1080));
        assert!(kick.bitrate.kbps() <= 8_000, "Kick ingests up to 8 Mbps");
    }

    #[test]
    fn built_in_presets_are_within_the_accepted_ranges() {
        for network in Network::ALL {
            let preset = network.render_preset();
            assert_eq!(
                Bitrate::from_kbps(preset.bitrate.kbps()),
                Ok(preset.bitrate)
            );
            assert_eq!(
                MaxDuration::from_seconds(preset.max_duration.seconds()),
                Ok(preset.max_duration)
            );
            assert_eq!(
                Loudness::from_tenths(preset.loudness.tenths()),
                Ok(preset.loudness)
            );
            assert_eq!(preset.loudness.lufs(), -14.0);
            assert_eq!(preset.codec, VideoCodec::H264);
        }
    }

    #[test]
    fn every_network_offers_public_first() {
        for network in Network::ALL {
            assert_eq!(network.visibilities()[0], Visibility::Public);
        }
        assert!(
            Network::YouTube
                .visibilities()
                .contains(&Visibility::Unlisted)
        );
        assert!(
            !Network::TikTok
                .visibilities()
                .contains(&Visibility::Unlisted)
        );
        assert_eq!(Network::X.visibilities(), [Visibility::Public]);
    }

    #[test]
    fn resolution_is_the_short_side_for_either_aspect() {
        assert_eq!(
            Resolution::FullHd1080.dimensions(AspectRatio::Vertical),
            (1080, 1920)
        );
        assert_eq!(
            Resolution::FullHd1080.dimensions(AspectRatio::Landscape),
            (1920, 1080)
        );
        assert_eq!(
            Resolution::Hd720.dimensions(AspectRatio::Vertical),
            (720, 1280)
        );
        assert_eq!(
            Resolution::Qhd1440.dimensions(AspectRatio::Landscape),
            (2560, 1440)
        );
        assert_eq!(
            Resolution::Uhd2160.dimensions(AspectRatio::Landscape),
            (3840, 2160)
        );
        assert_eq!(Resolution::FullHd1080.to_string(), "1080p");
    }

    #[test]
    fn no_overrides_keep_the_built_in_preset() {
        for network in Network::ALL {
            let built_in = network.render_preset();
            assert_eq!(built_in.with(&PresetOverrides::default()), built_in);
        }
        assert!(PresetOverrides::default().is_empty());
    }

    #[test]
    fn each_override_replaces_only_its_own_value() {
        let built_in = Network::YouTube.render_preset();
        let cases = [
            PresetOverrides {
                aspect: Some(AspectRatio::Landscape),
                ..PresetOverrides::default()
            },
            PresetOverrides {
                resolution: Some(Resolution::Uhd2160),
                ..PresetOverrides::default()
            },
            PresetOverrides {
                codec: Some(VideoCodec::Hevc),
                ..PresetOverrides::default()
            },
            PresetOverrides {
                bitrate: Some(Bitrate::from_kbps(20_000).unwrap()),
                ..PresetOverrides::default()
            },
            PresetOverrides {
                max_duration: Some(MaxDuration::from_seconds(3600).unwrap()),
                ..PresetOverrides::default()
            },
            PresetOverrides {
                loudness: Some(Loudness::from_tenths(-160).unwrap()),
                ..PresetOverrides::default()
            },
        ];
        let expected = [
            RenderPreset {
                aspect: AspectRatio::Landscape,
                ..built_in
            },
            RenderPreset {
                resolution: Resolution::Uhd2160,
                ..built_in
            },
            RenderPreset {
                codec: VideoCodec::Hevc,
                ..built_in
            },
            RenderPreset {
                bitrate: Bitrate(20_000),
                ..built_in
            },
            RenderPreset {
                max_duration: MaxDuration(3600),
                ..built_in
            },
            RenderPreset {
                loudness: Loudness(-160),
                ..built_in
            },
        ];
        for (overrides, expected) in cases.iter().zip(expected) {
            assert!(!overrides.is_empty());
            assert_eq!(built_in.with(overrides), expected);
        }
    }

    #[test]
    fn overriding_the_aspect_keeps_the_resolution_class() {
        let merged = Network::YouTube.render_preset().with(&PresetOverrides {
            aspect: Some(AspectRatio::Landscape),
            ..PresetOverrides::default()
        });
        assert_eq!(merged.dimensions(), (1920, 1080));
    }

    #[test]
    fn overrides_apply_over_whichever_network_they_belong_to() {
        let overrides = PresetOverrides {
            bitrate: Some(Bitrate(4_000)),
            ..PresetOverrides::default()
        };
        let x = Network::X.render_preset().with(&overrides);
        assert_eq!(x.bitrate, Bitrate(4_000));
        assert_eq!(x.resolution, Resolution::Hd720);
        let kick = Network::Kick.render_preset().with(&overrides);
        assert_eq!(kick.aspect, AspectRatio::Landscape);
    }

    #[test]
    fn bitrate_is_typed_in_mbps() {
        assert_eq!("12".parse(), Ok(Bitrate(12_000)));
        assert_eq!(" 7.5 ".parse(), Ok(Bitrate(7_500)));
        assert_eq!("7,5".parse(), Ok(Bitrate(7_500)));
        assert_eq!("0.75".parse(), Ok(Bitrate(750)));
        assert_eq!("100".parse(), Ok(Bitrate(100_000)));
        for bad in [
            "", "abc", "0.4", "101", "-5", "1.2345", "1.", ".5", "1 000", "1e3",
        ] {
            assert_eq!(bad.parse::<Bitrate>(), Err(OutOfRange), "{bad}");
        }
    }

    #[test]
    fn bitrate_shows_mbps_without_trailing_zeros() {
        for (kbps, shown) in [
            (12_000, "12"),
            (7_500, "7.5"),
            (750, "0.75"),
            (12_345, "12.345"),
        ] {
            let bitrate = Bitrate::from_kbps(kbps).unwrap();
            assert_eq!(bitrate.to_string(), shown);
            assert_eq!(shown.parse(), Ok(bitrate));
        }
    }

    #[test]
    fn duration_is_typed_as_a_clock_or_seconds() {
        assert_eq!("90".parse(), Ok(MaxDuration(90)));
        assert_eq!("3:00".parse(), Ok(MaxDuration(180)));
        assert_eq!(" 2:20 ".parse(), Ok(MaxDuration(140)));
        assert_eq!("1:00:00".parse(), Ok(MaxDuration(3600)));
        assert_eq!("12:00:00".parse(), Ok(MaxDuration(43_200)));
        for bad in [
            "",
            "0",
            "0:00",
            "1:60",
            "1:5",
            "12:00:01",
            "1:00:00:00",
            "-1",
            "1.5",
            "a:00",
            "1::00",
        ] {
            assert_eq!(bad.parse::<MaxDuration>(), Err(OutOfRange), "{bad}");
        }
    }

    #[test]
    fn duration_shows_as_a_clock() {
        for (seconds, shown) in [
            (140, "2:20"),
            (180, "3:00"),
            (59, "0:59"),
            (3_725, "1:02:05"),
        ] {
            let duration = MaxDuration::from_seconds(seconds).unwrap();
            assert_eq!(duration.to_string(), shown);
            assert_eq!(shown.parse(), Ok(duration));
        }
    }

    #[test]
    fn loudness_is_typed_in_lufs() {
        assert_eq!("-14".parse(), Ok(Loudness(-140)));
        assert_eq!("-13.5".parse(), Ok(Loudness(-135)));
        assert_eq!("-13,5".parse(), Ok(Loudness(-135)));
        assert_eq!("−16".parse(), Ok(Loudness(-160)));
        assert_eq!("-30".parse(), Ok(Loudness(-300)));
        assert_eq!("-5".parse(), Ok(Loudness(-50)));
        for bad in ["", "14", "-4", "-31", "-13.55", "0", "loud"] {
            assert_eq!(bad.parse::<Loudness>(), Err(OutOfRange), "{bad}");
        }
    }

    #[test]
    fn loudness_shows_lufs_without_trailing_zeros() {
        for (tenths, shown) in [(-140, "-14"), (-135, "-13.5"), (-50, "-5")] {
            let loudness = Loudness::from_tenths(tenths).unwrap();
            assert_eq!(loudness.to_string(), shown);
            assert_eq!(shown.parse(), Ok(loudness));
        }
    }

    #[test]
    fn values_out_of_range_are_rejected_by_their_constructors() {
        assert!(Bitrate::from_kbps(Bitrate::MIN_KBPS - 1).is_err());
        assert!(Bitrate::from_kbps(Bitrate::MAX_KBPS + 1).is_err());
        assert!(MaxDuration::from_seconds(0).is_err());
        assert!(MaxDuration::from_seconds(MaxDuration::MAX_SECONDS + 1).is_err());
        assert!(Loudness::from_tenths(Loudness::MIN_TENTHS - 1).is_err());
        assert!(Loudness::from_tenths(Loudness::MAX_TENTHS + 1).is_err());
    }
}
