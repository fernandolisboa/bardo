//! Video clips through the Higgsfield API, which serves models from several
//! makers (Kling, Seedance, Wan, MiniMax) behind one asynchronous protocol:
//! a submission to a model's endpoint returns a request id at once; the
//! status endpoint says when the clip is ready and where to download it.
//!
//! The scene's image goes first to Higgsfield's storage (a presigned upload
//! URL) and the model reads it from the public URL it gets. Submissions
//! carry an `Idempotency-Key`, so sending the same request again after a
//! lost answer returns the first request instead of paying for a second.
//! Before submitting, the adapter asks the estimate endpoint what the clip
//! costs, so the cost recorded is Higgsfield's own figure.
//!
//! Credentials are one key `KEY_ID:KEY_SECRET`, sent as
//! `Authorization: Key KEY_ID:KEY_SECRET`.

use std::time::Duration;

use bardo_domain::{
    ApiKey, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipModel, ClipModelRef,
    ClipRequest, ClipStatus, ClipSubmission, GeneratedClip, Money, Provider, ProviderFailure,
    ProviderFailureKind, StagedImage,
};
use serde_json::{Map, Value, json};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

pub const API_URL: &str = "https://api.higgsfield.ai";

/// The model a channel uses until the user picks another: Kling 3.0
/// Standard takes any length from 3 to 15 seconds, so a clip can match its
/// scene.
pub const DEFAULT_MODEL: &str = "kling-video/v3.0/std/image-to-video";

/// A 15-second 1080p clip is tens of megabytes; this leaves room.
pub const MAX_CLIP_BYTES: u64 = 512 * 1024 * 1024;

/// Longest prompt passed on; Kling truncates past 2,500 characters.
const MAX_PROMPT_CHARS: usize = 2_500;

/// An image-to-video model Bardo offers, and the fixed fields its request
/// needs. Narration is the soundtrack, so models that can make sound are
/// told not to; the output stays 16:9 like the image.
struct Spec {
    model: &'static str,
    name: &'static str,
    durations: fn() -> ClipDurations,
    fixed: fn() -> Value,
}

const CATALOG: &[Spec] = &[
    Spec {
        model: DEFAULT_MODEL,
        name: "Kling 3.0 Standard",
        durations: || ClipDurations::Range { min: 3, max: 15 },
        fixed: || json!({ "sound": "off" }),
    },
    Spec {
        model: "kling-video/v2.6/pro/image-to-video",
        name: "Kling 2.6 Pro",
        durations: || ClipDurations::Choices(vec![5, 10]),
        fixed: || json!({ "sound": "off", "aspect_ratio": "16:9" }),
    },
    Spec {
        model: "kling-video/v2.5-turbo/standard/image-to-video",
        name: "Kling 2.5 Turbo Standard",
        durations: || ClipDurations::Choices(vec![5, 10]),
        fixed: || json!({}),
    },
    Spec {
        model: "bytedance/seedance-2.0/image-to-video",
        name: "Seedance 2.0",
        durations: || ClipDurations::Range { min: 4, max: 15 },
        fixed: || json!({ "resolution": "1080p", "generate_audio": false }),
    },
    Spec {
        model: "wan/v2.6/image-to-video",
        name: "Wan 2.6",
        durations: || ClipDurations::Choices(vec![5, 10, 15]),
        fixed: || json!({ "resolution": "1080p" }),
    },
    Spec {
        model: "minimax/h3/image-to-video",
        name: "MiniMax H3",
        durations: || ClipDurations::Range { min: 5, max: 15 },
        fixed: || json!({}),
    },
    Spec {
        model: "minimax/hailuo-2.3/standard/image-to-video",
        name: "Hailuo 2.3 Standard",
        durations: || ClipDurations::Choices(vec![6, 10]),
        fixed: || json!({}),
    },
];

/// The models Bardo offers through Higgsfield, the default first.
pub fn models() -> Vec<ClipModel> {
    CATALOG
        .iter()
        .map(|spec| ClipModel {
            id: ClipModelRef::new(Provider::Higgsfield, spec.model)
                .expect("catalog models are valid"),
            name: spec.name.to_owned(),
            durations: (spec.durations)(),
        })
        .collect()
}

fn auth(request: HttpRequest, key: &ApiKey) -> HttpRequest {
    request.header("authorization", format!("Key {}", key.expose()))
}

/// The request that asks for an upload URL for an image of `mime` type.
pub fn upload_url_request(key: &ApiKey, mime: &str) -> HttpRequest {
    auth(
        HttpRequest::post_json(
            format!("{API_URL}/files/generate-upload-url"),
            json!({ "content_type": mime }).to_string(),
        ),
        key,
    )
}

/// The JSON body a model gets: the request's fields and the model's fixed
/// ones. The same request always makes the same body, as idempotent
/// resubmission requires.
pub fn clip_body(request: &ClipRequest) -> Value {
    let fixed = CATALOG
        .iter()
        .find(|spec| spec.model == request.model)
        .map_or_else(|| json!({}), |spec| (spec.fixed)());
    let mut body: Map<String, Value> = fixed.as_object().cloned().unwrap_or_default();
    let prompt: String = request.prompt.chars().take(MAX_PROMPT_CHARS).collect();
    body.insert("prompt".into(), json!(prompt));
    body.insert("image_url".into(), json!(request.image.0));
    body.insert("duration".into(), json!(request.seconds));
    Value::Object(body)
}

/// The request that starts the clip. `submission` is the idempotency key.
pub fn submit_request(key: &ApiKey, request: &ClipRequest, submission: &str) -> HttpRequest {
    auth(
        HttpRequest::post_json(
            format!("{API_URL}/{}", request.model),
            clip_body(request).to_string(),
        ),
        key,
    )
    .header("idempotency-key", submission.to_owned())
}

/// The request that asks what the clip would cost.
pub fn estimate_request(key: &ApiKey, request: &ClipRequest) -> HttpRequest {
    auth(
        HttpRequest::post_json(
            format!("{API_URL}/estimate/{}", request.model),
            clip_body(request).to_string(),
        ),
        key,
    )
}

pub fn status_request(key: &ApiKey, handle: &ClipHandle) -> HttpRequest {
    auth(
        HttpRequest::get(format!("{API_URL}/requests/{}/status", handle.0)),
        key,
    )
}

fn unexpected(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

fn json_body(response: &HttpResponse) -> Result<Value, ProviderFailure> {
    serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable response: {error}")))
}

/// What a refused call means. Higgsfield answers 400 when the account's
/// concurrent requests are all taken (they free up in minutes), 423 or 503
/// when a model is paused, 404 when the account cannot use the model, and
/// 422 with a list of problems when the request is invalid.
pub fn higgsfield_failure(response: &HttpResponse) -> ProviderFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
    let detail = match &body["detail"] {
        Value::String(text) => Some(text.clone()),
        Value::Array(problems) => Some(
            problems
                .iter()
                .map(|problem| {
                    let place: Vec<String> = problem["loc"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|part| {
                            part.as_str()
                                .map_or_else(|| part.to_string(), str::to_owned)
                        })
                        .collect();
                    let message = problem["msg"].as_str().unwrap_or("invalid");
                    if place.is_empty() {
                        message.to_owned()
                    } else {
                        format!("{}: {message}", place.join("."))
                    }
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        _ => None,
    };
    let with = |kind, fallback: &str| {
        ProviderFailure::new(kind, detail.clone().unwrap_or_else(|| fallback.to_owned()))
    };
    match response.status {
        400 if detail
            .as_deref()
            .is_some_and(|text| text.to_ascii_lowercase().contains("concurrent")) =>
        {
            with(
                ProviderFailureKind::ProviderDown,
                "too many requests at once",
            )
        }
        400 | 422 => with(ProviderFailureKind::Unexpected, "the request is invalid"),
        404 => with(
            ProviderFailureKind::NotAllowed,
            "the model is not available to this account",
        ),
        423 => with(ProviderFailureKind::ProviderDown, "the model is paused"),
        _ => failure(Provider::Higgsfield, response),
    }
}

/// Reads the answer to an upload URL request.
pub fn parse_upload_url(
    response: &HttpResponse,
) -> Result<(String, HttpRequest, String), ProviderFailure> {
    let body = json_body(response)?;
    let field = |name: &str| {
        body[name]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| unexpected(format!("the upload answer has no {name}")))
    };
    let public_url = field("public_url")?;
    let upload_url = field("upload_url")?;
    let content_type = body["content_type"].as_str().unwrap_or_default().to_owned();
    // The upload carries every header Higgsfield asks for, and never the
    // API key.
    let mut upload = HttpRequest::put(upload_url, Vec::new());
    for (name, value) in body["upload_headers"].as_object().into_iter().flatten() {
        let lower = name.to_ascii_lowercase();
        if lower == "authorization" || lower == "host" {
            continue;
        }
        if let Some(value) = value.as_str() {
            upload = upload.header(lower, value.to_owned());
        }
    }
    Ok((public_url, upload, content_type))
}

/// Reads a submission's answer: the request id.
pub fn parse_submission(response: &HttpResponse) -> Result<ClipHandle, ProviderFailure> {
    let body = json_body(response)?;
    body["request_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .map(|id| ClipHandle(id.to_owned()))
        .ok_or_else(|| unexpected("the submission answer has no request id"))
}

/// Reads an estimate's answer: the price in US dollars.
pub fn parse_estimate(response: &HttpResponse) -> Option<Money> {
    let body: Value = serde_json::from_str(&response.body).ok()?;
    match &body["usd"] {
        Value::String(text) => Money::parse(text).ok(),
        Value::Number(number) => Money::parse(&number.to_string()).ok(),
        _ => None,
    }
}

/// Reads a status answer.
pub fn parse_status(response: &HttpResponse) -> Result<ClipStatus, ProviderFailure> {
    let body = json_body(response)?;
    let error = body["error"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(str::trim);
    match body["status"].as_str() {
        Some("queued") => Ok(ClipStatus::Queued),
        Some("in_progress") => Ok(ClipStatus::Running),
        Some("completed") => body["video"]["url"]
            .as_str()
            .filter(|url| url.starts_with("https://"))
            .map(|url| ClipStatus::Done {
                video: url.to_owned(),
            })
            .ok_or_else(|| unexpected("the finished request has no video")),
        Some("failed") => Ok(ClipStatus::Failed(ProviderFailure::new(
            ProviderFailureKind::ProviderDown,
            match error {
                Some(error) => format!("Higgsfield could not make the clip: {error}"),
                None => "Higgsfield could not make the clip".to_owned(),
            },
        ))),
        Some("nsfw") => Ok(ClipStatus::Failed(ProviderFailure::new(
            ProviderFailureKind::Declined,
            "Higgsfield's moderation declined the image or the clip",
        ))),
        Some("canceled") => Ok(ClipStatus::Failed(unexpected(
            "the request was cancelled on Higgsfield",
        ))),
        Some(other) => Err(unexpected(format!("unknown request status {other}"))),
        None => Err(unexpected("the status answer has no status")),
    }
}

/// Whether `bytes` look like an MP4 file (an `ftyp` box first).
fn is_mp4(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp")
}

/// Makes clips with the models Higgsfield serves, over HTTPS.
pub struct HiggsfieldClips<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl HiggsfieldClips {
    /// Uploads and downloads move megabytes.
    pub const TIMEOUT: Duration = Duration::from_secs(300);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for HiggsfieldClips {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> HiggsfieldClips<T> {
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

    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, ProviderFailure> {
        self.backoff
            .send(&self.transport, request, &self.sleep)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))
    }

    fn quote(&self, key: &ApiKey, request: &ClipRequest) -> Option<Money> {
        let response = self.transport.send(&estimate_request(key, request)).ok()?;
        if !(200..=299).contains(&response.status) {
            return None;
        }
        parse_estimate(&response)
    }
}

impl<T: Transport> ClipGenerator for HiggsfieldClips<T> {
    fn provider(&self) -> Provider {
        Provider::Higgsfield
    }

    fn models(&self) -> Vec<ClipModel> {
        models()
    }

    fn stage_image(&self, key: &ApiKey, image: &ClipImage) -> Result<StagedImage, ProviderFailure> {
        let mime = image.format.mime();
        let response = self.send(&upload_url_request(key, mime))?;
        if !(200..=299).contains(&response.status) {
            return Err(higgsfield_failure(&response));
        }
        let (public_url, mut upload, content_type) = parse_upload_url(&response)?;
        if upload.header_value("content-type").is_none() {
            let content_type = if content_type.is_empty() {
                mime.to_owned()
            } else {
                content_type
            };
            upload = upload.header("content-type", content_type);
        }
        upload.body = Some(image.bytes.clone());
        let stored = self
            .transport
            .send(&upload)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        if !(200..=299).contains(&stored.status) {
            return Err(ProviderFailure::new(
                ProviderFailureKind::ProviderDown,
                format!("the image upload failed (HTTP {})", stored.status),
            ));
        }
        Ok(StagedImage(public_url))
    }

    fn submit(
        &self,
        key: &ApiKey,
        request: &ClipRequest,
        submission: &str,
    ) -> Result<ClipSubmission, ProviderFailure> {
        let quote = self.quote(key, request);
        let response = self.send(&submit_request(key, request, submission))?;
        if !(200..=299).contains(&response.status) {
            return Err(higgsfield_failure(&response));
        }
        Ok(ClipSubmission {
            handle: parse_submission(&response)?,
            quote,
        })
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<ClipStatus, ProviderFailure> {
        let response = self.send(&status_request(key, handle))?;
        match response.status {
            200..=299 => parse_status(&response),
            // Gone for good, as far as this request goes.
            404 => Ok(ClipStatus::Failed(unexpected(
                "Higgsfield does not know this request",
            ))),
            _ => Err(higgsfield_failure(&response)),
        }
    }

    fn download(&self, video: &str) -> Result<GeneratedClip, ProviderFailure> {
        let response = self
            .transport
            .send_for_bytes(&HttpRequest::get(video), MAX_CLIP_BYTES)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        match response.status {
            200..=299 if is_mp4(&response.bytes) => Ok(GeneratedClip {
                bytes: response.bytes,
            }),
            200..=299 => Err(unexpected("the download is not an MP4 clip")),
            403 | 404 | 410 => Err(unexpected("the clip is no longer available to download")),
            status => Err(ProviderFailure::new(
                ProviderFailureKind::ProviderDown,
                format!("the download failed (HTTP {status})"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_model_comes_first_and_every_model_is_unique() {
        let models = models();
        assert_eq!(models[0].id.model(), DEFAULT_MODEL);
        let mut ids: Vec<_> = models.iter().map(|model| model.id.model()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        for spec in CATALOG {
            assert!((spec.fixed)().is_object(), "{}", spec.model);
        }
    }

    #[test]
    fn mp4_files_start_with_an_ftyp_box() {
        assert!(is_mp4(b"\0\0\0\x18ftypisom"));
        assert!(!is_mp4(b"<html>"));
        assert!(!is_mp4(b""));
    }
}
