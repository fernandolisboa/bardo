//! Images with Nano Banana through Google's Gemini API
//! (`models.generateContent`).
//!
//! One request per image, answered with the image inline (base64), so a
//! large answer is allowed. Images come back 16:9 at 2K, enough for a
//! 1080p frame with room for a slow zoom or a 9:16 crop. Every image the
//! model draws carries Google's SynthID watermark.

use std::time::Duration;

use bardo_domain::{
    ApiKey, GeneratedImage, ImageFormat, ImageGenerator, ImageRequest, Provider, ProviderFailure,
    ProviderFailureKind, TokenUsage,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

pub const MODELS_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
/// Nano Banana 2: Google's stable image model for everyday generation.
pub const IMAGE_MODEL: &str = "gemini-3.1-flash-image";
pub const ASPECT_RATIO: &str = "16:9";
pub const IMAGE_SIZE: &str = "2K";

/// The request that draws `request.prompt`.
pub fn image_request(key: &ApiKey, request: &ImageRequest) -> HttpRequest {
    let body = json!({
        "contents": [{ "role": "user", "parts": [{ "text": request.prompt }] }],
        "generationConfig": {
            "responseModalities": ["IMAGE"],
            "imageConfig": { "aspectRatio": ASPECT_RATIO, "imageSize": IMAGE_SIZE },
        },
    });
    HttpRequest::post_json(
        format!("{MODELS_URL}/{IMAGE_MODEL}:generateContent"),
        body.to_string(),
    )
    .header("x-goog-api-key", key.expose())
}

fn unexpected(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

fn declined(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Declined, detail)
}

/// Reads a successful answer: the first image the model output (images it
/// drew while thinking are drafts), the model and the tokens counted. A
/// blocked prompt or an answer without an image is a failure.
pub fn parse_image(response: &HttpResponse) -> Result<GeneratedImage, ProviderFailure> {
    let body: Value = serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable response: {error}")))?;
    if let Some(reason) = body["promptFeedback"]["blockReason"].as_str() {
        return Err(declined(format!("Gemini blocked the prompt ({reason})")));
    }
    let candidate = &body["candidates"][0];
    let parts = candidate["content"]["parts"].as_array();
    let image = parts.into_iter().flatten().find_map(|part| {
        let inline = part.get("inlineData").or_else(|| part.get("inline_data"))?;
        let thought = part["thought"].as_bool().unwrap_or(false);
        let mime = inline
            .get("mimeType")
            .or_else(|| inline.get("mime_type"))?
            .as_str()?;
        (!thought).then(|| (mime, inline["data"].as_str()))
    });
    let Some((mime, data)) = image else {
        let said: String = parts
            .into_iter()
            .flatten()
            .filter(|part| !part["thought"].as_bool().unwrap_or(false))
            .filter_map(|part| part["text"].as_str())
            .collect();
        let finish = candidate["finishReason"].as_str().unwrap_or_default();
        let detail = candidate["finishMessage"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| (!said.trim().is_empty()).then(|| said.trim().to_owned()));
        return Err(match finish {
            "" | "STOP" | "MAX_TOKENS" | "FINISH_REASON_UNSPECIFIED" | "OTHER" => unexpected(
                detail.map_or("the answer has no image".to_owned(), |detail| {
                    format!("the answer has no image: {detail}")
                }),
            ),
            reason => declined(detail.map_or_else(
                || format!("Gemini drew no image ({reason})"),
                |detail| format!("Gemini drew no image ({reason}): {detail}"),
            )),
        });
    };
    let format = ImageFormat::from_mime(mime)
        .ok_or_else(|| unexpected(format!("the image is not a usable format ({mime})")))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.unwrap_or_default())
        .map_err(|error| unexpected(format!("unreadable image: {error}")))?;
    if bytes.is_empty() {
        return Err(unexpected("the image is empty"));
    }
    let usage = &body["usageMetadata"];
    let count = |name: &str| usage[name].as_u64().unwrap_or(0);
    Ok(GeneratedImage {
        bytes,
        format,
        model: body["modelVersion"]
            .as_str()
            .unwrap_or(IMAGE_MODEL)
            .to_owned(),
        usage: TokenUsage {
            input_tokens: count("promptTokenCount"),
            // Reasoning is billed as output, like the image.
            output_tokens: count("candidatesTokenCount") + count("thoughtsTokenCount"),
        },
    })
}

/// Draws images with Nano Banana over HTTPS.
pub struct GeminiImages<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl GeminiImages {
    /// The model thinks before drawing; a 2K image takes up to a minute.
    pub const TIMEOUT: Duration = Duration::from_secs(180);
    /// A 2K image as base64 and headroom.
    pub const MAX_BODY_BYTES: u64 = 32 * 1024 * 1024;

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT).with_max_body(Self::MAX_BODY_BYTES))
    }
}

impl Default for GeminiImages {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> GeminiImages<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            backoff: Backoff::default(),
            sleep: thread_sleeper(),
        }
    }

    /// Replaces how the adapter waits between retries (tests record the
    /// waits instead).
    pub fn with_sleeper(mut self, sleep: Sleeper) -> Self {
        self.sleep = sleep;
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T: Transport> ImageGenerator for GeminiImages<T> {
    fn generate(
        &self,
        key: &ApiKey,
        request: &ImageRequest,
    ) -> Result<GeneratedImage, ProviderFailure> {
        let response = self
            .backoff
            .send(&self.transport, &image_request(key, request), &self.sleep)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        if !(200..=299).contains(&response.status) {
            return Err(failure(Provider::Gemini, &response));
        }
        parse_image(&response)
    }
}
