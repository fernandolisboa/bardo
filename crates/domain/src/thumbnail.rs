//! YouTube thumbnails (#182, ADR-0021): the picture a long video shows
//! before anyone plays it. Bardo composes it from a background and a short
//! text: the background is a scene image of the project or one the image
//! provider drew for it, and the text is drawn by Bardo with the fonts it
//! ships, never by the image model. Claude can write the text and the
//! background's prompt; the user edits both and picks where the text goes
//! and how it looks.
//!
//! What the user picked is the thumbnail's design; saving composes the
//! picture, named after the design, so a design changed since shows as not
//! saved and an upload always sends the picture the user saw.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

use crate::{Generation, JobId, ProfileId, Provider, RepositoryError, VideoProjectId};

/// The thumbnail's size: YouTube's 16:9 frame at the width it shows.
pub const THUMBNAIL_WIDTH: u32 = 1280;
pub const THUMBNAIL_HEIGHT: u32 = 720;

/// The largest thumbnail Bardo makes: YouTube's limit on mobile, the
/// smallest of its limits.
pub const MAX_THUMBNAIL_BYTES: u64 = 2 * 1024 * 1024;

/// The longest text, in characters: a thumbnail reads at a glance.
pub const MAX_THUMBNAIL_TEXT: usize = 40;

/// How the text looks. Each channel picks one for its thumbnails; a
/// thumbnail can change its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThumbnailLook {
    /// Big condensed letters, white with a heavy black outline.
    #[default]
    Bold,
    /// The same letters in yellow.
    Yellow,
    /// Rounded bold letters, white on a dark band.
    Banner,
}

impl ThumbnailLook {
    pub const ALL: [ThumbnailLook; 3] = [
        ThumbnailLook::Bold,
        ThumbnailLook::Yellow,
        ThumbnailLook::Banner,
    ];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            ThumbnailLook::Bold => "bold",
            ThumbnailLook::Yellow => "yellow",
            ThumbnailLook::Banner => "banner",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown thumbnail look: {0}")]
pub struct UnknownThumbnailLook(pub String);

impl FromStr for ThumbnailLook {
    type Err = UnknownThumbnailLook;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        ThumbnailLook::ALL
            .into_iter()
            .find(|look| look.code() == code)
            .ok_or_else(|| UnknownThumbnailLook(code.to_owned()))
    }
}

impl fmt::Display for ThumbnailLook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Where the text sits: a corner or the middle of the top or bottom edge.
/// The middle of the frame stays the background's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextPlacement {
    #[default]
    TopLeft,
    Top,
    TopRight,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl TextPlacement {
    pub const ALL: [TextPlacement; 6] = [
        TextPlacement::TopLeft,
        TextPlacement::Top,
        TextPlacement::TopRight,
        TextPlacement::BottomLeft,
        TextPlacement::Bottom,
        TextPlacement::BottomRight,
    ];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            TextPlacement::TopLeft => "top_left",
            TextPlacement::Top => "top",
            TextPlacement::TopRight => "top_right",
            TextPlacement::BottomLeft => "bottom_left",
            TextPlacement::Bottom => "bottom",
            TextPlacement::BottomRight => "bottom_right",
        }
    }

    pub fn is_top(self) -> bool {
        matches!(
            self,
            TextPlacement::TopLeft | TextPlacement::Top | TextPlacement::TopRight
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown text placement: {0}")]
pub struct UnknownTextPlacement(pub String);

impl FromStr for TextPlacement {
    type Err = UnknownTextPlacement;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        TextPlacement::ALL
            .into_iter()
            .find(|placement| placement.code() == code)
            .ok_or_else(|| UnknownTextPlacement(code.to_owned()))
    }
}

/// Where a background came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackgroundSource {
    /// A scene image of the project.
    Scene,
    /// Drawn for the thumbnail by the image provider ([`DrawnBackground`]).
    Drawn,
}

impl BackgroundSource {
    pub fn code(self) -> &'static str {
        match self {
            BackgroundSource::Scene => "scene",
            BackgroundSource::Drawn => "drawn",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown background source: {0}")]
pub struct UnknownBackgroundSource(pub String);

impl FromStr for BackgroundSource {
    type Err = UnknownBackgroundSource;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        match code {
            "scene" => Ok(BackgroundSource::Scene),
            "drawn" => Ok(BackgroundSource::Drawn),
            _ => Err(UnknownBackgroundSource(code.to_owned())),
        }
    }
}

/// The picture behind the text: an image file in the project folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThumbnailBackground {
    pub file: String,
    pub source: BackgroundSource,
}

/// Why thumbnail text is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbnailFieldError {
    TextTooLong,
}

/// The text as a thumbnail holds it: one line, spaces collapsed, within
/// [`MAX_THUMBNAIL_TEXT`]. Empty is fine: a thumbnail may be all picture.
pub fn thumbnail_text(text: &str) -> Result<String, ThumbnailFieldError> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > MAX_THUMBNAIL_TEXT {
        Err(ThumbnailFieldError::TextTooLong)
    } else {
        Ok(text)
    }
}

/// What the user picked: the text, where it goes, how it looks and the
/// background. Saving composes it into a picture.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ThumbnailDesign {
    pub text: String,
    pub placement: TextPlacement,
    pub look: ThumbnailLook,
    pub background: Option<ThumbnailBackground>,
}

impl ThumbnailDesign {
    /// A stable fingerprint of the design (FNV-1a over its fields).
    pub fn fingerprint(&self) -> String {
        let background = self.background.as_ref();
        let fields = [
            self.text.as_str(),
            self.placement.code(),
            self.look.code(),
            background.map_or("", |background| background.source.code()),
            background.map_or("", |background| background.file.as_str()),
        ];
        let hash = fields
            .iter()
            .flat_map(|field| field.bytes().chain(std::iter::once(0)))
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        format!("{hash:016x}")
    }

    /// The composed picture's name in the project folder: one per design,
    /// so a picture never changes once made.
    pub fn file_name(&self) -> String {
        format!("thumbnail-{}.jpg", self.fingerprint())
    }
}

/// A video project's thumbnail: the design, the background prompt, what
/// Claude wrote last, and the picture saving made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    pub design: ThumbnailDesign,
    /// The prompt the next background is drawn from, as the user left it.
    pub prompt: String,
    /// The call that last wrote the text and the prompt; `None` when the
    /// user wrote them.
    pub generation: Option<Generation>,
    /// The picture composed when the design was last saved.
    pub file: Option<String>,
    pub updated_at: SystemTime,
}

impl Thumbnail {
    /// A project's thumbnail before anything was picked: the channel's look.
    pub fn new(
        project: VideoProjectId,
        owner: ProfileId,
        look: ThumbnailLook,
        now: SystemTime,
    ) -> Self {
        Self {
            project,
            owner,
            design: ThumbnailDesign {
                look,
                ..ThumbnailDesign::default()
            },
            prompt: String::new(),
            generation: None,
            file: None,
            updated_at: now,
        }
    }

    /// The saved picture, when it shows the design as it is: what an upload
    /// sends and an export copies.
    pub fn picture(&self) -> Option<&str> {
        self.file
            .as_deref()
            .filter(|file| *file == self.design.file_name())
    }

    /// Whether a picture was saved and the design changed since.
    pub fn is_outdated(&self) -> bool {
        self.file.is_some() && self.picture().is_none()
    }

    /// What Claude wrote replaces the text and the background prompt.
    pub fn written(&mut self, text: String, prompt: String, generation: Generation) {
        self.updated_at = generation.generated_at;
        self.design.text = text;
        self.prompt = prompt;
        self.generation = Some(generation);
    }
}

/// A background the image provider drew for a project's thumbnail. Every
/// one stays to pick from; drawing again never replaces the one in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawnBackground {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    /// The image file in the project folder.
    pub file: String,
    pub prompt: String,
    pub provider: Provider,
    pub model: String,
    /// The job that drew it.
    pub job: Option<JobId>,
    pub drawn_at: SystemTime,
}

/// Persistence port for thumbnails. Shared with job worker threads.
pub trait ThumbnailRepository: Send + Sync {
    fn thumbnail(&self, project: VideoProjectId) -> Result<Option<Thumbnail>, RepositoryError>;

    /// Saves the thumbnail, and its generation when it is new.
    fn save_thumbnail(&self, thumbnail: &Thumbnail) -> Result<(), RepositoryError>;

    /// The project's drawn backgrounds, newest first.
    fn drawn_backgrounds(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<DrawnBackground>, RepositoryError>;

    fn add_drawn_background(&self, background: &DrawnBackground) -> Result<(), RepositoryError>;
}

impl<T: ThumbnailRepository + ?Sized> ThumbnailRepository for Arc<T> {
    fn thumbnail(&self, project: VideoProjectId) -> Result<Option<Thumbnail>, RepositoryError> {
        (**self).thumbnail(project)
    }

    fn save_thumbnail(&self, thumbnail: &Thumbnail) -> Result<(), RepositoryError> {
        (**self).save_thumbnail(thumbnail)
    }

    fn drawn_backgrounds(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<DrawnBackground>, RepositoryError> {
        (**self).drawn_backgrounds(project)
    }

    fn add_drawn_background(&self, background: &DrawnBackground) -> Result<(), RepositoryError> {
        (**self).add_drawn_background(background)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::generation;

    fn design(text: &str) -> ThumbnailDesign {
        ThumbnailDesign {
            text: text.into(),
            placement: TextPlacement::Bottom,
            look: ThumbnailLook::Yellow,
            background: Some(ThumbnailBackground {
                file: "scene-1.png".into(),
                source: BackgroundSource::Scene,
            }),
        }
    }

    #[test]
    fn text_is_one_line_within_the_limit() {
        assert_eq!(
            thumbnail_text("  It  was\nNEVER   found "),
            Ok("It was NEVER found".to_owned())
        );
        assert_eq!(thumbnail_text(""), Ok(String::new()));
        assert_eq!(thumbnail_text(&"é".repeat(40)), Ok("é".repeat(40)));
        assert_eq!(
            thumbnail_text(&"é".repeat(41)),
            Err(ThumbnailFieldError::TextTooLong)
        );
    }

    #[test]
    fn each_design_has_its_own_picture() {
        let base = design("Lost");
        assert_eq!(base.file_name(), design("Lost").file_name());
        assert!(base.file_name().starts_with("thumbnail-"));
        assert!(base.file_name().ends_with(".jpg"));
        let mut moved = base.clone();
        moved.placement = TextPlacement::TopRight;
        let mut looked = base.clone();
        looked.look = ThumbnailLook::Banner;
        let mut drawn = base.clone();
        drawn.background.as_mut().unwrap().source = BackgroundSource::Drawn;
        let mut bare = base.clone();
        bare.background = None;
        let names: std::collections::HashSet<String> = [
            &base,
            &design("Found"),
            &moved,
            &looked,
            &drawn,
            &bare,
        ]
        .iter()
        .map(|design| design.file_name())
        .collect();
        assert_eq!(names.len(), 6);
    }

    #[test]
    fn a_picture_counts_only_while_it_shows_the_design() {
        let now = SystemTime::UNIX_EPOCH;
        let mut thumbnail = Thumbnail::new(
            VideoProjectId::new(),
            ProfileId::new(),
            ThumbnailLook::Banner,
            now,
        );
        assert_eq!(thumbnail.design.look, ThumbnailLook::Banner);
        assert_eq!(thumbnail.picture(), None);
        assert!(!thumbnail.is_outdated());
        thumbnail.design = design("Lost");
        thumbnail.file = Some(thumbnail.design.file_name());
        assert_eq!(thumbnail.picture(), thumbnail.file.as_deref());
        thumbnail.written("Found".into(), "A cave".into(), generation());
        assert_eq!(thumbnail.design.text, "Found");
        assert_eq!(thumbnail.prompt, "A cave");
        assert_eq!(thumbnail.picture(), None);
        assert!(thumbnail.is_outdated());
    }

    #[test]
    fn codes_read_back() {
        for look in ThumbnailLook::ALL {
            assert_eq!(look.code().parse(), Ok(look));
        }
        for placement in TextPlacement::ALL {
            assert_eq!(placement.code().parse(), Ok(placement));
        }
        for source in [BackgroundSource::Scene, BackgroundSource::Drawn] {
            assert_eq!(source.code().parse(), Ok(source));
        }
        assert!("huge".parse::<ThumbnailLook>().is_err());
        assert_eq!(ThumbnailLook::default(), ThumbnailLook::Bold);
    }
}
