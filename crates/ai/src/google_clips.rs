//! Video clips through Google's Gemini API, on the same key as Nano Banana:
//! Gemini Omni Flash, through the Interactions API.
//!
//! It works asynchronously: `POST /interactions` with `background: true`
//! returns an interaction id at once (Google documents background runs for
//! its text models and agents, and Bardo has used them for Omni since 1.1;
//! a real-key check confirms it for 1.2); `GET /interactions/{id}` says how it
//! is going and, once completed, carries the clip inline (base64). Google
//! keeps an interaction for 55 days unless the project asks for less.
//!
//! There is no staging step: the scene's image goes inline with the
//! submission. The API takes no idempotency key either, and Google offers
//! no way to list the interactions a key started. So a submission is a paid
//! call sent once (ADR-0030), without the adapter coming back by itself: a
//! refusal in Google's words (429, its own 500 or 503) started nothing and
//! is told as such, for the job and the queue to go again; a submission
//! that was sent and got no answer, or whose answer does not name what it
//! started, is a lost answer, which the job never sends again by itself.
//!
//! Google quotes no price before starting; the rate table prices each clip.
//! Every clip carries Google's SynthID watermark and a soundtrack the
//! model always makes; the narration replaces it in the edit.
//!
//! Google retires models: it shut down Veo 3.1 Preview in this API on
//! 2026-10-22 (its other Veo models answer on another platform, to another
//! kind of key) and serves Omni 1.1 Flash as 1.2 from 2026-12-03. `RETIRED`
//! names each model Bardo offered once with the one that takes its place,
//! so a channel or scene saved with it goes on making clips.

use std::time::Duration;

use bardo_domain::{
    ApiKey, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipModel, ClipModelRef,
    ClipRequest, ClipStatus, ClipSubmission, GeneratedClip, Provider, ProviderFailure,
    ProviderFailureKind, RetiredClipModel, StagedImage,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

pub const API_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Only Google's own API host ever gets the key.
const API_HOST: &str = "https://generativelanguage.googleapis.com/";

/// The model a channel uses until the user picks another: Omni 1.2 Flash
/// at 720p, the resolution it renders (1080p is the same clip, upscaled).
pub const DEFAULT_MODEL: &str = "gemini-omni-1.2-flash/720p";

/// The same model at 1080p.
const MODEL_1080P: &str = "gemini-omni-1.2-flash/1080p";

/// A 15-second 1080p clip is tens of megabytes; this leaves room.
pub const MAX_CLIP_BYTES: u64 = 512 * 1024 * 1024;

/// A clip inline as base64 is a third bigger than the file.
const MAX_ANSWER_BYTES: u64 = MAX_CLIP_BYTES / 3 * 4 + 1024 * 1024;

/// Longest prompt passed on: a motion prompt is a few sentences.
const MAX_PROMPT_CHARS: usize = 2_500;

/// What `stage_image` returns: the image travels with the submission.
const INLINE: &str = "inline";

/// Omni makes a few shots and lets people talk unless told otherwise; a
/// scene needs one shot that starts on its image, under the narration.
const OMNI_FRAMING: &str = "Animate this image, starting on it as the first frame, \
    in a single continuous shot with no scene cuts and no dialogue.";

/// A model at a resolution, as Bardo offers it. The id is the API model
/// and the resolution, `{model}/{resolution}`, so each resolution has its
/// own price in the rate table.
struct Spec {
    id: &'static str,
    name: &'static str,
}

/// Omni Flash makes any whole number of seconds from 3 to 15.
fn durations() -> ClipDurations {
    ClipDurations::Range { min: 3, max: 15 }
}

/// Left out: 360p (too small to publish) and 4K (upscaled, at three times
/// the price of 720p).
const CATALOG: &[Spec] = &[
    Spec {
        id: DEFAULT_MODEL,
        name: "Gemini Omni 1.2 Flash 720p",
    },
    Spec {
        id: MODEL_1080P,
        name: "Gemini Omni 1.2 Flash 1080p",
    },
];

/// A model Google no longer serves, as Bardo offered it, and the catalog
/// model of the same resolution that takes its place.
const RETIRED: &[(&str, &str, &str)] = &[
    (
        "veo-3.1-fast-generate-preview/720p",
        "Veo 3.1 Fast 720p",
        DEFAULT_MODEL,
    ),
    (
        "veo-3.1-fast-generate-preview/1080p",
        "Veo 3.1 Fast 1080p",
        MODEL_1080P,
    ),
    (
        "veo-3.1-generate-preview/720p",
        "Veo 3.1 720p",
        DEFAULT_MODEL,
    ),
    (
        "veo-3.1-generate-preview/1080p",
        "Veo 3.1 1080p",
        MODEL_1080P,
    ),
    (
        "veo-3.1-lite-generate-preview/720p",
        "Veo 3.1 Lite 720p",
        DEFAULT_MODEL,
    ),
    (
        "veo-3.1-lite-generate-preview/1080p",
        "Veo 3.1 Lite 1080p",
        MODEL_1080P,
    ),
    (
        "gemini-omni-1.1-flash/720p",
        "Gemini Omni 1.1 Flash 720p",
        DEFAULT_MODEL,
    ),
    (
        "gemini-omni-1.1-flash/1080p",
        "Gemini Omni 1.1 Flash 1080p",
        MODEL_1080P,
    ),
];

fn model_ref(id: &str) -> ClipModelRef {
    ClipModelRef::new(Provider::Gemini, id).expect("catalog models are valid")
}

/// The models Bardo offers through Google, the default first.
pub fn models() -> Vec<ClipModel> {
    CATALOG
        .iter()
        .map(|spec| ClipModel {
            id: model_ref(spec.id),
            name: spec.name.to_owned(),
            durations: durations(),
        })
        .collect()
}

/// The models Bardo offered through Google and Google no longer serves.
pub fn retired() -> Vec<RetiredClipModel> {
    RETIRED
        .iter()
        .map(|&(id, name, replacement)| RetiredClipModel {
            id: model_ref(id),
            name: name.to_owned(),
            replacement: model_ref(replacement),
        })
        .collect()
}

/// The API model and resolution of the catalog entry `id`.
fn spec(id: &str) -> Option<(&'static str, &'static str)> {
    let spec = CATALOG.iter().find(|spec| spec.id == id)?;
    spec.id.split_once('/')
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

/// The request body: the image, then what to do with it, run in the
/// background so the answer comes at once with an id to poll.
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
            "aspect_ratio": request.aspect.code(),
            "resolution": resolution,
            "duration": format!("{}s", request.seconds),
        },
        "background": true,
    })
}

/// The request that starts the clip; `None` when the model is not one
/// Bardo offers through Google.
pub fn submit_request(key: &ApiKey, request: &ClipRequest) -> Option<HttpRequest> {
    let (model, resolution) = spec(&request.model)?;
    let http = HttpRequest::post_json(
        format!("{API_URL}/interactions"),
        omni_body(request, model, resolution).to_string(),
    );
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

/// Omni's work is an interaction; anything else is an operation of a model
/// Google shut down, saved by a job from before.
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

/// Whether an error body is the Gemini API's own: an `error` with a
/// message and either a `status` (`UNAVAILABLE`) or a code in snake case
/// (`service_unavailable`, as the Interactions API documents), which that
/// API may wrap in a one-element array.
fn own_words(body: &Value) -> bool {
    let error = &body.get(0).unwrap_or(body)["error"];
    let snake_case =
        |code: &str| !code.is_empty() && code.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    error["message"].is_string()
        && (error["status"].is_string() || error["code"].as_str().is_some_and(snake_case))
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

/// Reads a submission's answer: the interaction to poll.
pub fn parse_submission(response: &HttpResponse) -> Result<ClipHandle, ProviderFailure> {
    let body = json_body(response)?;
    let handle = match body["id"].as_str() {
        Some(id) if !id.contains('/') => format!("interactions/{id}"),
        _ => return Err(unexpected("the submission answer has no interaction")),
    };
    if is_resource_name(&handle) {
        Ok(ClipHandle(handle))
    } else {
        Err(unexpected(
            "the submission answer has an unusable interaction",
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

/// The error codes the API gives when policy, safety or content rules
/// blocked the output (the API errors page, "Generation blocked codes").
const BLOCKED_CODES: &[&str] = &[
    "safety",
    "recitation",
    "language",
    "prohibited_content",
    "spii",
    "blocklist",
    "image_safety",
    "image_prohibited_content",
    "image_recitation",
    "image_other",
    "content_blocked",
];

/// Work that ended in an error: declined when its code says it was
/// blocked or its message says safety, so the queue does not send (and
/// pay for) it again.
fn ended_with(model: &str, error: &Value) -> ClipStatus {
    let message = error["message"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let blocked = error["code"]
        .as_str()
        .is_some_and(|code| BLOCKED_CODES.contains(&code));
    let kind = match message {
        _ if blocked => ProviderFailureKind::Declined,
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
        // Google does not say whether work stopped in these ways is
        // charged.
        Some("cancelled") => Ok(ClipStatus::Lost(unexpected(
            "the request was cancelled on Google",
        ))),
        Some("incomplete" | "requires_continuation" | "budget_exceeded") => Ok(ClipStatus::Lost(
            unexpected("Gemini Omni Flash stopped before the clip was done"),
        )),
        Some("requires_action") => Ok(ClipStatus::Lost(unexpected(
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

/// Makes clips with Gemini Omni Flash, over HTTPS.
pub struct GoogleClips<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl GoogleClips {
    /// Submissions carry the image and downloads the clip: megabytes.
    pub const TIMEOUT: Duration = Duration::from_secs(300);

    /// A new connection for every request: a submission that fails on a
    /// kept connection that had died would look like one Google may have
    /// taken.
    pub fn new() -> Self {
        Self::with_transport(
            UreqTransport::for_paid_calls(Self::TIMEOUT).with_max_body(MAX_ANSWER_BYTES),
        )
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

    /// Sends a submission: the one call here that starts paid work. It
    /// goes once. The job has noted that it is being sent, and that note
    /// must not outlive a refusal by the seconds of a wait in here: coming
    /// back after "busy" is the job's and the queue's, with the note
    /// cleared.
    fn send_submission(&self, request: &HttpRequest) -> Result<HttpResponse, ProviderFailure> {
        let once = Backoff {
            retries: 0,
            ..self.backoff
        };
        once.send_paid(&self.transport, request, own_words, &self.sleep)
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
                "the clip is no longer available to download (Google keeps it for a while only)",
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

    fn retired(&self) -> Vec<RetiredClipModel> {
        retired()
    }

    /// Nothing to hand over ahead: the image goes with the submission.
    fn stage_image(
        &self,
        _key: &ApiKey,
        _image: &ClipImage,
    ) -> Result<StagedImage, ProviderFailure> {
        Ok(StagedImage(INLINE.to_owned()))
    }

    /// Google has no idempotency key, so `submission` is not sent. A
    /// failure of kind `AnswerLost` means Google may have started the
    /// clip; any other means it did not.
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
        let response = self.send_submission(&http)?;
        if !(200..=299).contains(&response.status) {
            return Err(google_failure(&response));
        }
        // Google took it; without the handle nobody can follow the clip.
        let handle = parse_submission(&response).map_err(|failure| {
            ProviderFailure::new(
                ProviderFailureKind::AnswerLost,
                format!(
                    "Google took the submission and its answer does not say what it started, \
                     so the clip may have been made and billed ({})",
                    failure.detail
                ),
            )
        })?;
        Ok(ClipSubmission {
            handle,
            quote: None,
        })
    }

    /// A repeated submission starts, and bills, a second clip.
    fn dedupes_submissions(&self) -> bool {
        false
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<ClipStatus, ProviderFailure> {
        let http = status_request(key, handle)
            .ok_or_else(|| unexpected(format!("{} is not a Google request", handle.0)))?;
        if !is_interaction(handle) {
            // Nothing is left to ask: the clip may have been made, and
            // charged, before the model went away.
            return Ok(ClipStatus::Lost(unexpected(
                "Google no longer serves the model that was making this clip",
            )));
        }
        let response = self.send(&http)?;
        match response.status {
            200..=299 => parse_omni_status(&response, handle),
            // Gone for good: Google keeps work for a while only.
            404 => Ok(ClipStatus::Lost(unexpected(
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
    fn googles_error_reports_are_told_from_what_stands_in_front_of_it() {
        for own in [
            json!({ "error": { "code": 503, "message": "overloaded", "status": "UNAVAILABLE" } }),
            json!([{ "error": { "code": 503, "message": "overloaded", "status": "UNAVAILABLE" } }]),
            json!({ "error": { "code": "service_unavailable", "message": "overloaded" } }),
        ] {
            assert!(own_words(&own), "{own}");
        }
        for other in [
            json!({ "error": "upstream connect error" }),
            json!({ "error": { "code": 503 } }),
            json!({ "error": { "code": 503, "message": "Service Unavailable" } }),
            json!({ "error": { "code": "E503", "message": "upstream", "type": "proxy" } }),
            json!({ "message": "Service Unavailable" }),
            json!([]),
            json!("Bad Gateway"),
        ] {
            assert!(!own_words(&other), "{other}");
        }
    }

    #[test]
    fn the_default_model_comes_first_and_every_model_is_unique() {
        let models = models();
        assert_eq!(models[0].id.model(), DEFAULT_MODEL);
        let mut ids: Vec<_> = models.iter().map(|model| model.id.model()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        for spec in CATALOG {
            let (model, resolution) = super::spec(spec.id).unwrap();
            assert!(!model.is_empty() && !model.contains('/'), "{}", spec.id);
            assert!(["720p", "1080p"].contains(&resolution), "{}", spec.id);
        }
    }

    #[test]
    fn a_retired_model_is_not_offered_and_its_replacement_is() {
        for &(id, name, replacement) in RETIRED {
            assert!(super::spec(id).is_none(), "{id} is still offered");
            assert!(super::spec(replacement).is_some(), "{id}: {replacement}");
            assert!(!name.is_empty(), "{id}");
            assert_eq!(
                id.rsplit_once('/').map(|(_, resolution)| resolution),
                replacement
                    .rsplit_once('/')
                    .map(|(_, resolution)| resolution),
                "{id} keeps its resolution"
            );
        }
        let mut ids: Vec<_> = RETIRED.iter().map(|(id, ..)| id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), RETIRED.len());
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
