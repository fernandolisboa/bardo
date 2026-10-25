//! Persona looks (#188, CONTEXT.md): the image of the character a persona
//! appears as in performed clips. A look is an original character the
//! image generator drew from the user's description, or a photo of the
//! user. Every take model must be able to read it, so a look keeps within
//! the limits they all share.

use std::time::SystemTime;

use crate::{ImageFormat, JobId, PersonaId, TemplateUsed, image_dimensions};

/// The shortest side a take model reads, in pixels.
pub const LOOK_MIN_SIDE: u32 = 300;
/// The longest side a take model reads, in pixels.
pub const LOOK_MAX_SIDE: u32 = 6_000;
/// The narrowest width-to-height ratio a take model reads, as a fraction
/// of ten (0.4).
pub const LOOK_MIN_RATIO_TENTHS: u32 = 4;
/// The widest width-to-height ratio a take model reads, as a fraction of
/// ten (2.5).
pub const LOOK_MAX_RATIO_TENTHS: u32 = 25;
/// The largest file a take model reads.
pub const LOOK_MAX_BYTES: u64 = 10_000_000;
/// The longest description a look is drawn from, in characters.
pub const MAX_LOOK_DESCRIPTION: usize = 1_000;
/// The longest image prompt a look keeps, in characters.
pub const MAX_LOOK_PROMPT: usize = 4_000;

/// Why an image cannot be a look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum LookImageError {
    /// Not a PNG or JPEG file, or one whose size cannot be read.
    #[error("not a PNG or JPEG image")]
    NotAnImage,
    /// A side under [`LOOK_MIN_SIDE`].
    #[error("{width}×{height} px is too small")]
    TooSmall { width: u32, height: u32 },
    /// A side over [`LOOK_MAX_SIDE`].
    #[error("{width}×{height} px is too large")]
    TooLarge { width: u32, height: u32 },
    /// Too narrow or too wide for the take models.
    #[error("{width}×{height} px is too narrow or too wide")]
    Shape { width: u32, height: u32 },
    /// A file over [`LOOK_MAX_BYTES`].
    #[error("the file is {0} bytes")]
    FileTooLarge(u64),
}

/// A look's picture, checked against the take models' limits: a PNG or
/// JPEG file of a size and shape they all read.
#[derive(Clone, PartialEq, Eq)]
pub struct LookImage {
    bytes: Vec<u8>,
    format: ImageFormat,
    width: u32,
    height: u32,
}

impl std::fmt::Debug for LookImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LookImage")
            .field("bytes", &self.bytes.len())
            .field("format", &self.format)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

impl LookImage {
    /// The image in `bytes`, if it can be a look.
    pub fn check(bytes: Vec<u8>) -> Result<Self, LookImageError> {
        let format = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            ImageFormat::Png
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            ImageFormat::Jpeg
        } else {
            return Err(LookImageError::NotAnImage);
        };
        let (width, height) = image_dimensions(&bytes).ok_or(LookImageError::NotAnImage)?;
        if width == 0 || height == 0 {
            return Err(LookImageError::NotAnImage);
        }
        if width.min(height) < LOOK_MIN_SIDE {
            return Err(LookImageError::TooSmall { width, height });
        }
        if width.max(height) > LOOK_MAX_SIDE {
            return Err(LookImageError::TooLarge { width, height });
        }
        // width / height within [0.4, 2.5], in whole numbers.
        let (w, h) = (u64::from(width) * 10, u64::from(height));
        if w < h * u64::from(LOOK_MIN_RATIO_TENTHS) || w > h * u64::from(LOOK_MAX_RATIO_TENTHS) {
            return Err(LookImageError::Shape { width, height });
        }
        let size = bytes.len() as u64;
        if size > LOOK_MAX_BYTES {
            return Err(LookImageError::FileTooLarge(size));
        }
        Ok(Self {
            bytes,
            format,
            width,
            height,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Where a look came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookSource {
    /// Drawn by the image generator as an original character.
    Drawn,
    /// A photo of the user, imported with their confirmation. It is
    /// someone's likeness, so it never leaves in a persona package.
    Imported,
}

impl LookSource {
    pub const ALL: [LookSource; 2] = [LookSource::Drawn, LookSource::Imported];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            LookSource::Drawn => "drawn",
            LookSource::Imported => "imported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown look source: {0}")]
pub struct UnknownLookSource(pub String);

impl std::str::FromStr for LookSource {
    type Err = UnknownLookSource;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LookSource::ALL
            .into_iter()
            .find(|source| source.code() == s)
            .ok_or_else(|| UnknownLookSource(s.to_owned()))
    }
}

/// How a drawn look was made: what the user asked for, the prompt the
/// image generator drew, and who wrote and drew it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LookDrawing {
    /// The user's description of the character.
    pub description: String,
    /// The prompt the image generator drew.
    pub prompt: String,
    /// The look prompt template version Claude wrote the prompt with;
    /// `None` for a look that came in a persona package.
    pub template: Option<TemplateUsed>,
    /// The model that wrote the prompt, as the provider named it; empty
    /// when not known.
    pub writer_model: String,
    /// The model that drew the image, as the provider named it; empty when
    /// not known.
    pub image_model: String,
    /// The job that drew it, so a resumed job does not draw twice.
    pub job: Option<JobId>,
}

/// The character a persona appears as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaLook {
    pub persona: PersonaId,
    pub image: LookImage,
    pub source: LookSource,
    /// The look shows what reads as a real person (a photo, or a
    /// photoreal drawing): posts with performed clips of it need the
    /// network's synthetic-content disclosure.
    pub realistic: bool,
    /// How a drawn look was made; `None` for an imported photo.
    pub drawing: Option<LookDrawing>,
    pub made_at: SystemTime,
}

impl PersonaLook {
    /// A look drawn by the image generator, realistic when the drawing is
    /// photoreal.
    pub fn drawn(
        persona: PersonaId,
        image: LookImage,
        drawing: LookDrawing,
        photoreal: bool,
        made_at: SystemTime,
    ) -> Self {
        Self {
            persona,
            image,
            source: LookSource::Drawn,
            realistic: photoreal,
            drawing: Some(drawing),
            made_at,
        }
    }

    /// A photo of the user: always realistic until the user says
    /// otherwise.
    pub fn imported(persona: PersonaId, image: LookImage, made_at: SystemTime) -> Self {
        Self {
            persona,
            image,
            source: LookSource::Imported,
            realistic: true,
            drawing: None,
            made_at,
        }
    }

    /// The same look on another persona (a duplicate, or an import).
    pub fn for_persona(&self, persona: PersonaId) -> Self {
        Self {
            persona,
            ..self.clone()
        }
    }

    /// Whether it may leave in a persona package: only a drawing may. An
    /// imported photo is someone's likeness.
    pub fn is_shareable(&self) -> bool {
        self.source == LookSource::Drawn
    }
}

/// Which entry of the provider's asset library a look stands for (#194).
/// ModelArk refuses real faces in uploaded pictures and offers these
/// instead; which ones a look may use depends on where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookAssetKind {
    /// One of ModelArk's preset digital characters.
    PresetCharacter,
    /// A private virtual portrait the user made from the drawn look, which
    /// ModelArk requires not to resemble any real person.
    VirtualPortrait,
    /// The user's own likeness, added through ModelArk's real-person
    /// verification.
    VerifiedLikeness,
}

impl LookAssetKind {
    pub const ALL: [LookAssetKind; 3] = [
        LookAssetKind::PresetCharacter,
        LookAssetKind::VirtualPortrait,
        LookAssetKind::VerifiedLikeness,
    ];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            LookAssetKind::PresetCharacter => "preset_character",
            LookAssetKind::VirtualPortrait => "virtual_portrait",
            LookAssetKind::VerifiedLikeness => "verified_likeness",
        }
    }

    pub fn from_code(code: &str) -> Option<LookAssetKind> {
        LookAssetKind::ALL
            .into_iter()
            .find(|kind| kind.code() == code)
    }

    /// The kinds a look from `source` may use: a drawn look a preset
    /// character or a virtual portrait, a photo of the user only their
    /// verified likeness.
    pub fn for_source(source: LookSource) -> &'static [LookAssetKind] {
        match source {
            LookSource::Drawn => &[
                LookAssetKind::PresetCharacter,
                LookAssetKind::VirtualPortrait,
            ],
            LookSource::Imported => &[LookAssetKind::VerifiedLikeness],
        }
    }
}

/// Why a pasted asset id is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum LookAssetError {
    #[error("the asset id is empty")]
    Empty,
    /// Not letters, digits, `-`, `_` or `.`, or over 128 characters.
    #[error("the asset id is not one")]
    Invalid,
    /// The kind is not one the look may use.
    #[error("this look cannot use that kind of asset")]
    KindNotForLook,
}

impl LookAssetError {
    pub const ALL: [LookAssetError; 3] = [
        LookAssetError::Empty,
        LookAssetError::Invalid,
        LookAssetError::KindNotForLook,
    ];
}

/// How tall the face in a look's image is (#243), measured on the computer
/// by the face tracker for the preflight checks: a full-body look should
/// not be driven by half-body motion. It holds for the image it was
/// measured on.
#[derive(Debug, Clone, PartialEq)]
pub struct LookFaceMeasure {
    /// The fingerprint of the look's image it was measured on.
    pub image: String,
    /// The face's height as a share of the image's; `None` when the face
    /// tracker found no face.
    pub face_height: Option<f32>,
}

/// A look's entry in the provider's asset library, used in place of the
/// image. It belongs to the user's account, so it is never exported, and
/// it holds only for the image it was given for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookAsset {
    pub kind: LookAssetKind,
    /// The id, without `asset://`.
    pub id: String,
    /// A fingerprint of the look's image it was given for.
    pub image: String,
}

impl LookAsset {
    pub const MAX_ID_CHARS: usize = 128;
    pub const PREFIX: &str = "asset://";

    /// Checks a pasted id (with or without `asset://`) for a look from
    /// `source` whose image has fingerprint `image`.
    pub fn parse(
        kind: LookAssetKind,
        input: &str,
        source: LookSource,
        image: &str,
    ) -> Result<Self, LookAssetError> {
        let input = input.trim();
        let id = input.strip_prefix(Self::PREFIX).unwrap_or(input).trim();
        if id.is_empty() {
            return Err(LookAssetError::Empty);
        }
        if id.len() > Self::MAX_ID_CHARS
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            return Err(LookAssetError::Invalid);
        }
        if !LookAssetKind::for_source(source).contains(&kind) {
            return Err(LookAssetError::KindNotForLook);
        }
        Ok(Self {
            kind,
            id: id.to_owned(),
            image: image.to_owned(),
        })
    }

    /// How the provider is told to use it.
    pub fn uri(&self) -> String {
        format!("{}{}", Self::PREFIX, self.id)
    }

    /// Whether it still stands for `look`: given for the image `look` has
    /// now (fingerprint `image`), of a kind its source may use.
    pub fn holds_for(&self, look: &PersonaLook, image: &str) -> bool {
        self.image == image && LookAssetKind::for_source(look.source).contains(&self.kind)
    }
}

/// The user's description of a look to draw: trimmed, not empty, within
/// [`MAX_LOOK_DESCRIPTION`] characters.
pub fn look_description(text: &str) -> Result<String, LookDescriptionError> {
    let text = text.trim();
    if text.is_empty() {
        Err(LookDescriptionError::Required)
    } else if text.chars().count() > MAX_LOOK_DESCRIPTION {
        Err(LookDescriptionError::TooLong)
    } else {
        Ok(text.to_owned())
    }
}

/// Why a look's description cannot be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum LookDescriptionError {
    #[error("describe the look")]
    Required,
    #[error("the description is too long")]
    TooLong,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0]);
        bytes
    }

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes
    }

    #[test]
    fn asset_kinds_have_stable_codes_and_follow_the_looks_source() {
        for kind in LookAssetKind::ALL {
            assert_eq!(LookAssetKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(
            LookAssetKind::for_source(LookSource::Imported),
            [LookAssetKind::VerifiedLikeness]
        );
        assert!(
            !LookAssetKind::for_source(LookSource::Drawn)
                .contains(&LookAssetKind::VerifiedLikeness)
        );
    }

    #[test]
    fn a_pasted_asset_id_loses_its_prefix_and_must_fit_the_look() {
        let asset = LookAsset::parse(
            LookAssetKind::VirtualPortrait,
            " asset://asset-2026abc_1.x ",
            LookSource::Drawn,
            "print",
        )
        .unwrap();
        assert_eq!(asset.id, "asset-2026abc_1.x");
        assert_eq!(asset.uri(), "asset://asset-2026abc_1.x");
        let parse = |kind, input: &str, source| LookAsset::parse(kind, input, source, "print");
        assert_eq!(
            parse(
                LookAssetKind::PresetCharacter,
                "asset://",
                LookSource::Drawn
            ),
            Err(LookAssetError::Empty)
        );
        assert_eq!(
            parse(LookAssetKind::PresetCharacter, "a b", LookSource::Drawn),
            Err(LookAssetError::Invalid)
        );
        assert_eq!(
            parse(
                LookAssetKind::PresetCharacter,
                &"a".repeat(129),
                LookSource::Drawn
            ),
            Err(LookAssetError::Invalid)
        );
        assert_eq!(
            parse(LookAssetKind::VerifiedLikeness, "me-1", LookSource::Drawn),
            Err(LookAssetError::KindNotForLook)
        );
        assert_eq!(
            parse(
                LookAssetKind::PresetCharacter,
                "char-1",
                LookSource::Imported
            ),
            Err(LookAssetError::KindNotForLook)
        );
        assert!(
            parse(
                LookAssetKind::VerifiedLikeness,
                "me-1",
                LookSource::Imported
            )
            .is_ok()
        );
    }

    #[test]
    fn a_portrait_png_or_jpeg_within_the_limits_is_a_look() {
        let image = LookImage::check(png(1696, 2528)).unwrap();
        assert_eq!(image.format(), ImageFormat::Png);
        assert_eq!((image.width(), image.height()), (1696, 2528));
        let image = LookImage::check(jpeg(1080, 1350)).unwrap();
        assert_eq!(image.format(), ImageFormat::Jpeg);
        // The edges are inside.
        assert!(LookImage::check(png(300, 750)).is_ok(), "0.4");
        assert!(LookImage::check(png(750, 300)).is_ok(), "2.5");
        assert!(LookImage::check(png(6000, 6000)).is_ok());
    }

    #[test]
    fn other_files_are_not_looks() {
        let mut webp = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        webp.extend([0; 16]);
        for bytes in [
            b"GIF89a".to_vec(),
            webp,
            Vec::new(),
            png(10, 10)[..18].to_vec(),
        ] {
            assert_eq!(LookImage::check(bytes), Err(LookImageError::NotAnImage));
        }
    }

    #[test]
    fn sizes_and_shapes_take_models_refuse_are_refused_with_the_reason() {
        assert_eq!(
            LookImage::check(png(299, 400)),
            Err(LookImageError::TooSmall {
                width: 299,
                height: 400
            })
        );
        assert_eq!(
            LookImage::check(png(4000, 6001)),
            Err(LookImageError::TooLarge {
                width: 4000,
                height: 6001
            })
        );
        assert_eq!(
            LookImage::check(png(300, 751)),
            Err(LookImageError::Shape {
                width: 300,
                height: 751
            })
        );
        assert_eq!(
            LookImage::check(png(751, 300)),
            Err(LookImageError::Shape {
                width: 751,
                height: 300
            })
        );
        let mut big = png(1000, 1500);
        big.resize(LOOK_MAX_BYTES as usize + 1, 0);
        assert_eq!(
            LookImage::check(big),
            Err(LookImageError::FileTooLarge(LOOK_MAX_BYTES + 1))
        );
        let mut at_limit = png(1000, 1500);
        at_limit.resize(LOOK_MAX_BYTES as usize, 0);
        assert!(LookImage::check(at_limit).is_ok());
    }

    #[test]
    fn a_drawing_is_realistic_when_photoreal_and_a_photo_always_is() {
        let persona = PersonaId::new();
        let image = LookImage::check(png(832, 1248)).unwrap();
        let at = SystemTime::UNIX_EPOCH;
        let drawn = PersonaLook::drawn(persona, image.clone(), LookDrawing::default(), false, at);
        assert_eq!(drawn.source, LookSource::Drawn);
        assert!(!drawn.realistic);
        assert!(drawn.is_shareable());
        assert!(
            PersonaLook::drawn(persona, image.clone(), LookDrawing::default(), true, at).realistic
        );
        let photo = PersonaLook::imported(persona, image, at);
        assert!(photo.realistic);
        assert!(!photo.is_shareable(), "a photo is someone's likeness");
        assert_eq!(photo.drawing, None);
        let other = PersonaId::new();
        assert_eq!(photo.for_persona(other).persona, other);
    }

    #[test]
    fn sources_have_stable_codes() {
        for source in LookSource::ALL {
            assert_eq!(source.code().parse(), Ok(source));
        }
        assert!("photo".parse::<LookSource>().is_err());
    }

    #[test]
    fn a_description_is_trimmed_and_limited() {
        assert_eq!(
            look_description("  a lighthouse keeper \n").as_deref(),
            Ok("a lighthouse keeper")
        );
        assert_eq!(look_description(" \n"), Err(LookDescriptionError::Required));
        assert!(look_description(&"é".repeat(MAX_LOOK_DESCRIPTION)).is_ok());
        assert_eq!(
            look_description(&"x".repeat(MAX_LOOK_DESCRIPTION + 1)),
            Err(LookDescriptionError::TooLong)
        );
    }
}
