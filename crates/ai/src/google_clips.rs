//! Video clips through Google's Gemini API, on the same key as Nano Banana:
//! Veo 3.1 (Standard, Fast and Lite) and Gemini Omni Flash.
//!
//! Both work asynchronously, through two protocols:
//! - Veo: `models/{model}:predictLongRunning` returns an operation name at
//!   once; `GET /{operation}` says when it is done and where the clip is.
//!   The clip downloads from the Files API with the key; Google keeps it
//!   for two days.
//! - Omni: `POST /interactions` with `background: true` returns an
//!   interaction id at once; `GET /interactions/{id}` says how it is going
//!   and, once completed, carries the clip inline (base64).
//!
//! Neither has a staging step: the scene's image goes inline with the
//! submission. Neither takes an idempotency key either, so a submission
//! whose answer is lost (the connection drops after Google took it) may be
//! made, and billed, twice when the job sends it again. Refusals (429,
//! 5xx) are safe to resend, as Google started nothing.
//!
//! Google quotes no price before starting; the rate table prices each clip.
//! Every clip carries Google's SynthID watermark and a soundtrack the
//! models always make; the narration replaces it in the edit.

use std::time::Duration;

use bardo_domain::{
    ApiKey, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipModel, ClipModelRef,
    ClipRequest, ClipStatus, ClipSubmission, GeneratedClip, Provider, ProviderFailure,
    ProviderFailureKind, StagedImage,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

pub const API_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Only Google's own API host ever gets the key.
const API_HOST: &str = "https://generativelanguage.googleapis.com/";

/// The model a channel uses until the user picks another: Veo 3.1 Fast at
/// 720p takes 4, 6 or 8 seconds, so a clip can come close to its scene.
pub const DEFAULT_MODEL: &str = "veo-3.1-fast-generate-preview/720p";

/// An 8-second 1080p clip is tens of megabytes; this leaves room.
pub const MAX_CLIP_BYTES: u64 = 512 * 1024 * 1024;

/// A clip inline as base64 is a third bigger than the file.
const MAX_ANSWER_BYTES: u64 = MAX_CLIP_BYTES / 3 * 4 + 1024 * 1024;

/// Longest prompt passed on; Veo reads up to 1,024 tokens.
const MAX_PROMPT_CHARS: usize = 2_500;

/// What `stage_image` returns: the image travels with the submission.
const INLINE: &str = "inline";

/// Omni makes a few shots and lets people talk unless told otherwise; a
/// scene needs one shot that starts on its image, under the narration.
const OMNI_FRAMING: &str = "Animate this image, starting on it as the first frame, \
    in a single continuous shot with no scene cuts and no dialogue.";

/// How a model is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Veo,
    Omni,
}

/// A model at a resolution, as Bardo offers it. The id is the API model
/// and the resolution, `{model}/{resolution}`, so each resolution has its
/// own price in the rate table.
struct Spec {
    id: &'static str,
    name: &'static str,
    family: Family,
    durations: fn() -> ClipDurations,
}

/// Veo makes 4, 6 or 8 seconds at 720p, and only 8 at 1080p.
fn veo_720p() -> ClipDurations {
    ClipDurations::Choices(vec![4, 6, 8])
}

fn veo_1080p() -> ClipDurations {
    ClipDurations::Choices(vec![8])
}

fn omni() -> ClipDurations {
    ClipDurations::Range { min: 3, max: 10 }
}

const CATALOG: &[Spec] = &[
    Spec {
        id: DEFAULT_MODEL,
        name: "Veo 3.1 Fast 720p",
        family: Family::Veo,
        durations: veo_720p,
    },
    Spec {
        id: "veo-3.1-fast-generate-preview/1080p",
        name: "Veo 3.1 Fast 1080p",
        family: Family::Veo,
        durations: veo_1080p,
    },
    Spec {
        id: "veo-3.1-generate-preview/720p",
        name: "Veo 3.1 720p",
        family: Family::Veo,
        durations: veo_720p,
    },
    Spec {
        id: "veo-3.1-generate-preview/1080p",
        name: "Veo 3.1 1080p",
        family: Family::Veo,
        durations: veo_1080p,
    },
    Spec {
        id: "veo-3.1-lite-generate-preview/720p",
        name: "Veo 3.1 Lite 720p",
        family: Family::Veo,
        durations: veo_720p,
    },
    Spec {
        id: "veo-3.1-lite-generate-preview/1080p",
        name: "Veo 3.1 Lite 1080p",
        family: Family::Veo,
        durations: veo_1080p,
    },
    Spec {
        id: "gemini-omni-1.1-flash/720p",
        name: "Gemini Omni Flash 720p",
        family: Family::Omni,
        durations: omni,
    },
    Spec {
        id: "gemini-omni-1.1-flash/1080p",
        name: "Gemini Omni Flash 1080p",
        family: Family::Omni,
        durations: omni,
    },
];

/// The models Bardo offers through Google, the default first.
pub fn models() -> Vec<ClipModel> {
    CATALOG
        .iter()
        .map(|spec| ClipModel {
            id: ClipModelRef::new(Provider::Gemini, spec.id).expect("catalog models are valid"),
            name: spec.name.to_owned(),
            durations: (spec.durations)(),
        })
        .collect()
}

/// The catalog entry for `id`, with its API model and resolution.
fn spec(id: &str) -> Option<(&'static Spec, &'static str, &'static str)> {
    let spec = CATALOG.iter().find(|spec| spec.id == id)?;
    let (model, resolution) = spec.id.split_once('/')?;
    Some((spec, model, resolution))
}

fn with_key(request: HttpRequest, key: &ApiKey) -> HttpRequest {
    request.header("x-goog-api-key", key.expose())
}

fn unexpected(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

fn prompt(request: &ClipRequest) -> String {
    request.prompt.chars().take(MAX_PROMPT_CHARS).collect()
}

fn base64(image: &ClipImage) -> String {
    base64::engine::general_purpose::STANDARD.encode(&image.bytes)
}

/// The Veo request body: the image as the first frame, 16:9 like it.
pub fn veo_body(request: &ClipRequest, resolution: &str) -> Value {
    json!({
        "instances": [{
            "prompt": prompt(request),
            "image": {
                "inlineData": {
                    "mimeType": request.first_frame.format.mime(),
                    "data": base64(&request.first_frame),
                },
            },
        }],
        "parameters": {
            "aspectRatio": "16:9",
            "resolution": resolution,
            "durationSeconds": request.seconds,
        },
    })
}

/// The Omni request body, run in the background so the answer comes at
/// once with an id to poll.
pub fn omni_body(request: &ClipRequest, model: &str, resolution: &str) -> Value {
    json!({
        "model": model,
        "input": [
            {
                "type": "image",
                "data": base64(&request.first_frame),
                "mime_type": request.first_frame.format.mime(),
            },
            { "type": "text", "text": format!("{OMNI_FRAMING} {}", prompt(request)) },
        ],
        "response_format": {
            "type": "video",
            "aspect_ratio": "16:9",
            "resolution": resolution,
            "duration": format!("{}s", request.seconds),
        },
        "background": true,
    })
}

/// The request that starts the clip; `None` when the model is not one
/// Bardo offers through Google.
pub fn submit_request(key: &ApiKey, request: &ClipRequest) -> Option<HttpRequest> {
    let (spec, model, resolution) = spec(&request.model)?;
    let http = match spec.family {
        Family::Veo => HttpRequest::post_json(
            format!("{API_URL}/models/{model}:predictLongRunning"),
            veo_body(request, resolution).to_string(),
        ),
        Family::Omni => HttpRequest::post_json(
            format!("{API_URL}/interactions"),
            omni_body(request, model, resolution).to_string(),
        ),
    };
    Some(with_key(http, key))
}

/// Whether `name` is safe to put in a URL path: Google's resource name
/// characters (ids may be base64url, padding included), no way up, no
/// query.
fn is_resource_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '=' | '~'))
}

/// Omni's work is an interaction; everything else is a Veo operation.
fn is_interaction(handle: &ClipHandle) -> bool {
    handle.0.starts_with("interactions/")
}

pub fn status_request(key: &ApiKey, handle: &ClipHandle) -> Option<HttpRequest> {
    is_resource_name(&handle.0)
        .then(|| with_key(HttpRequest::get(format!("{API_URL}/{}", handle.0)), key))
}

fn json_body(response: &HttpResponse) -> Result<Value, ProviderFailure> {
    serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable response: {error}")))
}

/// What a refused call means: Google's usual errors, and 404 when the key
/// cannot use the model. The Interactions API wraps its error in a
/// one-element array.
pub fn google_failure(response: &HttpResponse) -> ProviderFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
    let body = match body {
        Value::Array(mut errors) if !errors.is_empty() => errors.swap_remove(0),
        body => body,
    };
    if response.status == 404 {
        return ProviderFailure::new(
            ProviderFailureKind::NotAllowed,
            body["error"]["message"]
                .as_str()
                .unwrap_or("the model is not available to this key")
                .to_owned(),
        );
    }
    let mut unwrapped = HttpResponse::new(response.status, body.to_string());
    unwrapped.headers = response.headers.clone();
    failure(Provider::Gemini, &unwrapped)
}

/// Reads a submission's answer: the operation (Veo) or interaction (Omni)
/// to poll.
pub fn parse_submission(response: &HttpResponse) -> Result<ClipHandle, ProviderFailure> {
    let body = json_body(response)?;
    let handle = match (body["name"].as_str(), body["id"].as_str()) {
        (Some(operation), _) => operation.to_owned(),
        (None, Some(id)) if !id.contains('/') => format!("interactions/{id}"),
        _ => return Err(unexpected("the submission answer has no operation")),
    };
    if is_resource_name(&handle) {
        Ok(ClipHandle(handle))
    } else {
        Err(unexpected(
            "the submission answer has an unusable operation",
        ))
    }
}

/// Whether a refusal reads as a safety or policy block.
fn is_safety(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "safety",
        "policy",
        "policies",
        "blocked",
        "responsible ai",
        "filtered",
    ]
    .iter()
    .any(|word| text.contains(word))
}

/// Work that ended in an error: declined when the error says safety.
fn ended_with(model: &str, error: &Value) -> ClipStatus {
    let message = error["message"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let kind = match message {
        Some(text) if is_safety(text) => ProviderFailureKind::Declined,
        _ => ProviderFailureKind::ProviderDown,
    };
    ClipStatus::Failed(ProviderFailure::new(
        kind,
        match message {
            Some(text) => format!("{model} could not make the clip: {text}"),
            None => format!("{model} could not make the clip"),
        },
    ))
}

/// Reads a Veo operation.
pub fn parse_veo_status(response: &HttpResponse) -> Result<ClipStatus, ProviderFailure> {
    let body = json_body(response)?;
    if !body["done"].as_bool().unwrap_or(false) {
        return Ok(ClipStatus::Running);
    }
    if body.get("error").is_some_and(Value::is_object) {
        return Ok(ended_with("Veo", &body["error"]));
    }
    let answer = &body["response"]["generateVideoResponse"];
    let video = answer["generatedSamples"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|sample| sample["video"]["uri"].as_str())
        .filter(|uri| uri.starts_with("https://"));
    if let Some(uri) = video {
        return Ok(ClipStatus::Done {
            video: uri.to_owned(),
        });
    }
    let reasons: Vec<&str> = answer["raiMediaFilteredReasons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let filtered = answer["raiMediaFilteredCount"].as_u64().unwrap_or(0) > 0;
    Ok(ClipStatus::Failed(if filtered || !reasons.is_empty() {
        ProviderFailure::new(
            ProviderFailureKind::Declined,
            match reasons.is_empty() {
                true => "Google's safety filters declined the clip".to_owned(),
                false => format!(
                    "Google's safety filters declined the clip: {}",
                    reasons.join(" ")
                ),
            },
        )
    } else {
        unexpected("Veo finished without a clip")
    }))
}

/// Every content block of an interaction's output: the model output steps,
/// and `outputs` as answers before the steps shape had it.
fn output_blocks(body: &Value) -> impl Iterator<Item = &Value> {
    let steps = body["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|step| step["type"].as_str() == Some("model_output"))
        .flat_map(|step| step["content"].as_array().into_iter().flatten());
    let outputs = body["outputs"].as_array().into_iter().flatten();
    steps.chain(outputs)
}

fn omni_video(body: &Value) -> Option<&Value> {
    output_blocks(body).find(|block| block["type"].as_str() == Some("video"))
}

/// Reads an Omni interaction. A finished clip comes inline, so `Done`
/// points back at the interaction for the download to read it again.
pub fn parse_omni_status(
    response: &HttpResponse,
    handle: &ClipHandle,
) -> Result<ClipStatus, ProviderFailure> {
    let body = json_body(response)?;
    match body["status"].as_str() {
        Some("queued") => Ok(ClipStatus::Queued),
        Some("in_progress") => Ok(ClipStatus::Running),
        Some("completed") => Ok(match omni_video(&body) {
            Some(video) if video["data"].as_str().is_some_and(|data| !data.is_empty()) => {
                ClipStatus::Done {
                    video: handle.0.clone(),
                }
            }
            Some(video)
                if video["uri"]
                    .as_str()
                    .is_some_and(|uri| uri.starts_with("https://")) =>
            {
                ClipStatus::Done {
                    video: video["uri"].as_str().unwrap_or_default().to_owned(),
                }
            }
            // A refusal comes back as text instead of a clip.
            _ => {
                let said: Vec<&str> = output_blocks(&body)
                    .filter(|block| block["type"].as_str() == Some("text"))
                    .filter_map(|block| block["text"].as_str())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .collect();
                ClipStatus::Failed(ProviderFailure::new(
                    ProviderFailureKind::Declined,
                    match said.is_empty() {
                        true => "Gemini Omni Flash made no clip".to_owned(),
                        false => format!("Gemini Omni Flash made no clip: {}", said.join(" ")),
                    },
                ))
            }
        }),
        Some("failed") => Ok(ended_with("Gemini Omni Flash", &body["error"])),
        Some("cancelled") => Ok(ClipStatus::Failed(unexpected(
            "the request was cancelled on Google",
        ))),
        Some("incomplete" | "budget_exceeded") => Ok(ClipStatus::Failed(unexpected(
            "Gemini Omni Flash stopped before the clip was done",
        ))),
        Some("requires_action") => Ok(ClipStatus::Failed(unexpected(
            "Gemini Omni Flash asked for more input",
        ))),
        Some(other) => Err(unexpected(format!("unknown interaction status {other}"))),
        None => Err(unexpected("the status answer has no status")),
    }
}

/// The clip inside a completed interaction.
pub fn parse_omni_clip(response: &HttpResponse) -> Result<GeneratedClip, ProviderFailure> {
    let body = json_body(response)?;
    let data = omni_video(&body)
        .and_then(|video| video["data"].as_str())
        .ok_or_else(|| unexpected("the interaction has no clip"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| unexpected(format!("unreadable clip: {error}")))?;
    mp4(bytes)
}

/// Whether `bytes` look like an MP4 file (an `ftyp` box first).
fn is_mp4(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp")
}

fn mp4(bytes: Vec<u8>) -> Result<GeneratedClip, ProviderFailure> {
    if is_mp4(&bytes) {
        Ok(GeneratedClip { bytes })
    } else {
        Err(unexpected("the download is not an MP4 clip"))
    }
}

/// Makes clips with Veo and Gemini Omni Flash, over HTTPS.
pub struct GoogleClips<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl GoogleClips {
    /// Submissions carry the image and downloads the clip: megabytes.
    pub const TIMEOUT: Duration = Duration::from_secs(300);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT).with_max_body(MAX_ANSWER_BYTES))
    }
}

impl Default for GoogleClips {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> GoogleClips<T> {
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

    /// Downloads a file Google serves (the key goes only to its API host).
    fn download_file(&self, key: &ApiKey, url: &str) -> Result<GeneratedClip, ProviderFailure> {
        let mut request = HttpRequest::get(url);
        if url.starts_with(API_HOST) {
            request = with_key(request, key);
        }
        let response = self
            .transport
            .send_for_bytes(&request, MAX_CLIP_BYTES)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        match response.status {
            200..=299 => mp4(response.bytes),
            403 | 404 | 410 => Err(unexpected(
                "the clip is no longer available to download (Google keeps it for two days)",
            )),
            status => Err(ProviderFailure::new(
                ProviderFailureKind::ProviderDown,
                format!("the download failed (HTTP {status})"),
            )),
        }
    }
}

impl<T: Transport> ClipGenerator for GoogleClips<T> {
    fn provider(&self) -> Provider {
        Provider::Gemini
    }

    fn models(&self) -> Vec<ClipModel> {
        models()
    }

    /// Nothing to hand over ahead: the image goes with the submission.
    fn stage_image(
        &self,
        _key: &ApiKey,
        _image: &ClipImage,
    ) -> Result<StagedImage, ProviderFailure> {
        Ok(StagedImage(INLINE.to_owned()))
    }

    /// Google has no idempotency key, so `submission` is not sent.
    fn submit(
        &self,
        key: &ApiKey,
        request: &ClipRequest,
        _submission: &str,
    ) -> Result<ClipSubmission, ProviderFailure> {
        let http = submit_request(key, request).ok_or_else(|| {
            ProviderFailure::new(
                ProviderFailureKind::NotAllowed,
                format!("{} is not a Google video model", request.model),
            )
        })?;
        let response = self.send(&http)?;
        if !(200..=299).contains(&response.status) {
            return Err(google_failure(&response));
        }
        Ok(ClipSubmission {
            handle: parse_submission(&response)?,
            quote: None,
        })
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<ClipStatus, ProviderFailure> {
        let http = status_request(key, handle)
            .ok_or_else(|| unexpected(format!("{} is not a Google request", handle.0)))?;
        let response = self.send(&http)?;
        match response.status {
            200..=299 if is_interaction(handle) => parse_omni_status(&response, handle),
            200..=299 => parse_veo_status(&response),
            // Gone for good: Google keeps work for two days.
            404 => Ok(ClipStatus::Failed(unexpected(
                "Google does not know this request",
            ))),
            _ => Err(google_failure(&response)),
        }
    }

    fn download(&self, key: &ApiKey, video: &str) -> Result<GeneratedClip, ProviderFailure> {
        if video.starts_with("https://") {
            return self.download_file(key, video);
        }
        let handle = ClipHandle(video.to_owned());
        let http = status_request(key, &handle)
            .filter(|_| is_interaction(&handle))
            .ok_or_else(|| unexpected(format!("{video} is not a Google clip")))?;
        let response = self.send(&http)?;
        match response.status {
            200..=299 => parse_omni_clip(&response),
            404 => Err(unexpected("the clip is no longer available to download")),
            _ => Err(google_failure(&response)),
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
            let (_, model, resolution) = super::spec(spec.id).unwrap();
            assert!(!model.is_empty() && !model.contains('/'), "{}", spec.id);
            assert!(["720p", "1080p"].contains(&resolution), "{}", spec.id);
        }
    }

    #[test]
    fn resource_names_cannot_leave_the_api() {
        assert!(is_resource_name(
            "models/veo-3.1-generate-preview/operations/a1b2"
        ));
        assert!(is_resource_name("interactions/v1_ChdBYmNk"));
        assert!(is_resource_name("interactions/v1_ChdBYmNk=="));
        assert!(!is_resource_name("models/../../evil"));
        assert!(!is_resource_name("models/x?key=1"));
        assert!(!is_resource_name("https://example.com/x"));
        assert!(!is_resource_name(""));
    }

    #[test]
    fn mp4_files_start_with_an_ftyp_box() {
        assert!(is_mp4(b"\0\0\0\x18ftypisom"));
        assert!(!is_mp4(b"<html>"));
    }
}
