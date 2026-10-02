//! Image generation (PRD story 36): an image provider (Nano Banana, through
//! the Gemini API) draws a scene from its prompt. The interface hides the
//! provider's protocol; callers say what to draw and get the image file
//! back with what the provider counted.

use std::sync::Arc;

use crate::{ApiKey, Metered, ProviderFailure};

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

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Webp => "webp",
        }
    }
}

/// What to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRequest {
    /// The whole description: subject, setting, style. Providers see one
    /// prompt at a time, with no memory of earlier ones.
    pub prompt: String,
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

/// Draws images. Every image is a wide 16:9 frame, the source every video
/// format is cropped from. Calls the network and blocks, so it runs inside
/// a job.
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
    }
}
