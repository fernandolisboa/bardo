//! Image generation (PRD story 36): an image provider (Nano Banana, through
//! the Gemini API) draws a scene from its prompt. The interface hides the
//! provider's protocol; callers say what to draw and get the image file
//! back with what the provider counted.

use std::sync::Arc;

use crate::{ApiKey, AspectRatio, Metered, ProviderFailure};

/// The file format of a generated image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

impl ImageFormat {
    /// The format of a MIME type such as `image/png`, ignoring case and
    /// parameters.
    pub fn from_mime(mime: &str) -> Option<Self> {
        let essence = mime.split(';').next().unwrap_or_default().trim();
        match essence.to_ascii_lowercase().as_str() {
            "image/png" => Some(ImageFormat::Png),
            "image/jpeg" | "image/jpg" => Some(ImageFormat::Jpeg),
            "image/webp" => Some(ImageFormat::Webp),
            _ => None,
        }
    }

    /// The MIME type, e.g. `image/png`.
    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Webp => "image/webp",
        }
    }

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Webp => "webp",
        }
    }
}

/// The shape of a drawn image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageShape {
    /// A video frame: the video's format (#179).
    Frame(AspectRatio),
    /// 2:3, taller than wide: a persona's look (#188).
    Portrait,
}

impl ImageShape {
    /// The width-to-height ratio as providers write it, e.g. `2:3`.
    pub fn code(self) -> &'static str {
        match self {
            ImageShape::Frame(aspect) => aspect.code(),
            ImageShape::Portrait => "2:3",
        }
    }
}

impl From<AspectRatio> for ImageShape {
    fn from(aspect: AspectRatio) -> Self {
        ImageShape::Frame(aspect)
    }
}

/// What to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRequest {
    /// The whole description: subject, setting, style. Providers see one
    /// prompt at a time, with no memory of earlier ones.
    pub prompt: String,
    /// The image's shape: a frame of the video's format, or a portrait.
    pub shape: ImageShape,
}

/// The width and height of a PNG, JPEG or WebP file, read from its header;
/// `None` for anything else or a header cut short.
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let be16 = |at: usize| {
        Some(u32::from(u16::from_be_bytes([
            *bytes.get(at)?,
            *bytes.get(at + 1)?,
        ])))
    };
    let le16 = |at: usize| {
        Some(u32::from(u16::from_le_bytes([
            *bytes.get(at)?,
            *bytes.get(at + 1)?,
        ])))
    };
    let le24 = |at: usize| {
        Some(u32::from_le_bytes([
            *bytes.get(at)?,
            *bytes.get(at + 1)?,
            *bytes.get(at + 2)?,
            0,
        ]))
    };
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.get(12..16) == Some(b"IHDR") {
        let be32 = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
        return Some((be32(16)?, be32(20)?));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut at = 2;
        loop {
            if *bytes.get(at)? != 0xFF {
                return None;
            }
            let marker = *bytes.get(at + 1)?;
            match marker {
                // Fill byte before a marker.
                0xFF => at += 1,
                // Markers with no length.
                0x01 | 0xD0..=0xD9 => at += 2,
                // Start of frame: precision, height, width.
                0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                    return Some((be16(at + 7)?, be16(at + 5)?));
                }
                _ => at += 2 + usize::try_from(be16(at + 2)?).ok()?,
            }
        }
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return match bytes.get(12..16)? {
            b"VP8 " => Some((le16(26)? & 0x3FFF, le16(28)? & 0x3FFF)),
            b"VP8L" => {
                let b = bytes.get(21..25)?;
                let (b0, b1, b2, b3) = (
                    u32::from(b[0]),
                    u32::from(b[1]),
                    u32::from(b[2]),
                    u32::from(b[3]),
                );
                let width = 1 + (((b1 & 0x3F) << 8) | b0);
                let height = 1 + (((b3 & 0x0F) << 10) | (b2 << 2) | ((b1 & 0xC0) >> 6));
                Some((width, height))
            }
            b"VP8X" => Some((1 + le24(24)?, 1 + le24(27)?)),
            _ => None,
        };
    }
    None
}

/// The frame shape a picture of these dimensions fits: vertical when it is
/// taller than wide, landscape otherwise.
pub fn shape_of(width: u32, height: u32) -> AspectRatio {
    if height > width {
        AspectRatio::Vertical
    } else {
        AspectRatio::Landscape
    }
}

/// The drawn image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
    /// The model that drew it, as the provider names it.
    pub model: String,
    /// What the provider counted: the prompt in, any reasoning out, and
    /// the image apart, since it is priced apart. Cost tracking builds on
    /// it.
    pub usage: Metered,
}

/// Draws images in the shape asked for. Calls the network and blocks, so
/// it runs inside a job.
pub trait ImageGenerator: Send + Sync {
    fn generate(
        &self,
        key: &ApiKey,
        request: &ImageRequest,
    ) -> Result<GeneratedImage, ProviderFailure>;
}

impl<T: ImageGenerator + ?Sized> ImageGenerator for Arc<T> {
    fn generate(
        &self,
        key: &ApiKey,
        request: &ImageRequest,
    ) -> Result<GeneratedImage, ProviderFailure> {
        (**self).generate(key, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_come_from_mime_types() {
        assert_eq!(ImageFormat::from_mime("image/png"), Some(ImageFormat::Png));
        assert_eq!(
            ImageFormat::from_mime("Image/JPEG; q=1"),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            ImageFormat::from_mime("image/webp"),
            Some(ImageFormat::Webp)
        );
        assert_eq!(ImageFormat::from_mime("text/plain"), None);
        assert_eq!(ImageFormat::Jpeg.extension(), "jpg");
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Webp] {
            assert_eq!(ImageFormat::from_mime(format.mime()), Some(format));
        }
    }

    #[test]
    fn shapes_have_their_ratio() {
        assert_eq!(ImageShape::from(AspectRatio::Vertical).code(), "9:16");
        assert_eq!(ImageShape::from(AspectRatio::Landscape).code(), "16:9");
        assert_eq!(ImageShape::Portrait.code(), "2:3");
    }
}

#[cfg(test)]
mod dimension_tests {
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
        // An APP0 segment to skip, then a fill byte and a baseline frame.
        bytes.extend([0xFF, 0xE0, 0x00, 0x04, 0x4A, 0x46]);
        bytes.extend([0xFF, 0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes
    }

    #[test]
    fn reads_png_and_jpeg_headers() {
        assert_eq!(image_dimensions(&png(1536, 2752)), Some((1536, 2752)));
        assert_eq!(image_dimensions(&jpeg(1920, 1080)), Some((1920, 1080)));
    }

    #[test]
    fn reads_the_three_kinds_of_webp() {
        let riff = |chunk: &[u8], body: &[u8]| {
            let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
            bytes.extend(chunk);
            bytes.extend([0, 0, 0, 0]);
            bytes.extend(body);
            bytes
        };
        // Lossy: frame tag and start code, then 14-bit width and height.
        let mut lossy = vec![0, 0, 0, 0x9D, 0x01, 0x2A];
        lossy.extend(1080u16.to_le_bytes());
        lossy.extend(1920u16.to_le_bytes());
        assert_eq!(image_dimensions(&riff(b"VP8 ", &lossy)), Some((1080, 1920)));
        // Lossless: signature, then width-1 and height-1 in 14 bits each.
        let (w, h) = (1920u32 - 1, 1080u32 - 1);
        let packed = w | (h << 14);
        let mut lossless = vec![0x2F];
        lossless.extend(packed.to_le_bytes());
        assert_eq!(
            image_dimensions(&riff(b"VP8L", &lossless)),
            Some((1920, 1080))
        );
        // Extended: flags, then width-1 and height-1 in 24 bits each.
        let mut extended = vec![0, 0, 0, 0];
        extended.extend(&(1080u32 - 1).to_le_bytes()[..3]);
        extended.extend(&(1920u32 - 1).to_le_bytes()[..3]);
        assert_eq!(
            image_dimensions(&riff(b"VP8X", &extended)),
            Some((1080, 1920))
        );
    }

    #[test]
    fn anything_else_has_no_dimensions() {
        assert_eq!(image_dimensions(b"GIF89a"), None);
        assert_eq!(image_dimensions(&png(10, 10)[..18]), None);
        assert_eq!(image_dimensions(&[0xFF, 0xD8, 0xFF]), None);
    }

    #[test]
    fn taller_than_wide_is_vertical() {
        assert_eq!(shape_of(1080, 1920), AspectRatio::Vertical);
        assert_eq!(shape_of(1920, 1080), AspectRatio::Landscape);
        assert_eq!(shape_of(1024, 1024), AspectRatio::Landscape);
    }
}
