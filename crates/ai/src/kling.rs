//! Video clips straight from Kling's own API, with a Kling API key: Kling
//! 3.0, 2.6 and 2.5 Turbo at 720p or 1080p.
//!
//! The API works asynchronously: `POST /image-to-video/{model}` returns a
//! task id at once; `GET /tasks?task_ids={id}` says how it is going and,
//! once it succeeded, where to download the clip. Kling deletes results
//! after 30 days; Bardo downloads each clip as soon as it is ready.
//!
//! There is no staging step: the scene's image goes inline (raw base64)
//! with the submission. Every submission carries a custom task id
//! (`external_task_id`) made from the job's submission id and the request
//! itself; Kling keeps those unique per account, so when the answer to a
//! submission is lost and the job sends it again, Kling refuses the second
//! one and the adapter finds the first by its custom id instead of paying
//! for another clip.
//!
//! Kling quotes no price before starting; the rate table prices each clip.
//! The key goes as `Authorization: Bearer {key}`.

use std::time::Duration;

use bardo_domain::{
    ApiKey, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipModel, ClipModelRef,
    ClipRequest, ClipStatus, ClipSubmission, GeneratedClip, Provider, ProviderFailure,
    ProviderFailureKind, StagedImage,
};
use base64::Engine as _;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

pub const API_URL: &str = "https://api-singapore.klingai.com";

/// The model a channel uses until the user picks another: Kling 3.0 takes
/// any length from 3 to 15 seconds, so a clip can match its scene, and at
/// 720p it costs a quarter less than at 1080p.
pub const DEFAULT_MODEL: &str = "kling-3.0/720p";

/// A 15-second 1080p clip is tens of megabytes; this leaves room.
pub const MAX_CLIP_BYTES: u64 = 512 * 1024 * 1024;

/// Longest prompt passed on: Kling takes 3,072 characters and advises
/// staying within 2,500.
const MAX_PROMPT_CHARS: usize = 2_500;

/// What `stage_image` returns: the image travels with the submission.
const INLINE: &str = "inline";

/// An image-to-video model Bardo offers, as `{model}/{resolution}`, and
/// the settings its request needs besides resolution and length. Narration
/// is the soundtrack, so models that can make sound are told not to, and a
/// scene is one shot.
struct Spec {
    id: &'static str,
    name: &'static str,
    durations: fn() -> ClipDurations,
    fixed: fn() -> Value,
}

fn kling_3() -> ClipDurations {
    ClipDurations::Range { min: 3, max: 15 }
}

fn five_or_ten() -> ClipDurations {
    ClipDurations::Choices(vec![5, 10])
}

fn kling_3_settings() -> Value {
    json!({ "audio": "off", "multi_shot": false })
}

fn kling_2_6_settings() -> Value {
    json!({ "audio": "off" })
}

fn no_settings() -> Value {
    json!({})
}

const CATALOG: &[Spec] = &[
    Spec {
        id: DEFAULT_MODEL,
        name: "Kling 3.0 720p",
        durations: kling_3,
        fixed: kling_3_settings,
    },
    Spec {
        id: "kling-3.0/1080p",
        name: "Kling 3.0 1080p",
        durations: kling_3,
        fixed: kling_3_settings,
    },
    Spec {
        id: "kling-2.6/720p",
        name: "Kling 2.6 720p",
        durations: five_or_ten,
        fixed: kling_2_6_settings,
    },
    Spec {
        id: "kling-2.6/1080p",
        name: "Kling 2.6 1080p",
        durations: five_or_ten,
        fixed: kling_2_6_settings,
    },
    Spec {
        id: "kling-2.5-turbo/720p",
        name: "Kling 2.5 Turbo 720p",
        durations: five_or_ten,
        fixed: no_settings,
    },
    Spec {
        id: "kling-2.5-turbo/1080p",
        name: "Kling 2.5 Turbo 1080p",
        durations: five_or_ten,
        fixed: no_settings,
    },
];

/// The models Bardo offers through Kling, the default first.
pub fn models() -> Vec<ClipModel> {
    CATALOG
        .iter()
        .map(|spec| ClipModel {
            id: ClipModelRef::new(Provider::Kling, spec.id).expect("catalog models are valid"),
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

pub(crate) fn auth(request: HttpRequest, key: &ApiKey) -> HttpRequest {
    request.header("authorization", format!("Bearer {}", key.expose()))
}

pub(crate) fn unexpected(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

/// The submission without its custom task id; `None` for a model Kling
/// does not offer here.
fn task_body(request: &ClipRequest) -> Option<Value> {
    let (spec, _, resolution) = spec(&request.model)?;
    let prompt: String = request.prompt.chars().take(MAX_PROMPT_CHARS).collect();
    let image = base64::engine::general_purpose::STANDARD.encode(&request.first_frame.bytes);
    let mut settings: Map<String, Value> = (spec.fixed)().as_object().cloned().unwrap_or_default();
    settings.insert("resolution".into(), json!(resolution));
    settings.insert("duration".into(), json!(request.seconds));
    Some(json!({
        "contents": [
            { "type": "prompt", "text": prompt },
            { "type": "first_frame", "url": image },
        ],
        "settings": settings,
    }))
}

/// The custom task id of a submission: the same submission of the same
/// request always gets the same id, and anything else (another job, another
/// install on the same Kling account, an edited prompt) gets another.
pub fn external_task_id(request: &ClipRequest, submission: &str) -> Option<String> {
    Some(task_id_for(&task_body(request)?, submission))
}

pub(crate) fn task_id_for(body: &Value, submission: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(submission.as_bytes());
    hash.update([0]);
    hash.update(body.to_string().as_bytes());
    let digest = hash.finalize();
    let hex: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("bardo-{hex}")
}

/// The JSON body a submission sends; `None` for a model Kling does not
/// offer here.
pub fn clip_body(request: &ClipRequest, submission: &str) -> Option<Value> {
    let mut body = task_body(request)?;
    body["options"] = json!({
        "external_task_id": task_id_for(&body, submission),
        "watermark_info": { "enabled": false },
    });
    Some(body)
}

/// The request that starts the clip.
pub fn submit_request(
    key: &ApiKey,
    request: &ClipRequest,
    submission: &str,
) -> Option<HttpRequest> {
    let (_, model, _) = spec(&request.model)?;
    let body = clip_body(request, submission)?;
    Some(auth(
        HttpRequest::post_json(
            format!("{API_URL}/image-to-video/{model}"),
            body.to_string(),
        ),
        key,
    ))
}

/// Task ids are numbers, and custom ids are Bardo's own; anything else
/// would change the query.
fn is_task_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The request that asks how the task is going; `None` for a handle Kling
/// did not give.
pub fn status_request(key: &ApiKey, handle: &ClipHandle) -> Option<HttpRequest> {
    is_task_id(&handle.0).then(|| {
        auth(
            HttpRequest::get(format!("{API_URL}/tasks?task_ids={}", handle.0)),
            key,
        )
    })
}

/// The request that finds a task by its custom id.
pub fn lookup_request(key: &ApiKey, external_id: &str) -> Option<HttpRequest> {
    is_task_id(external_id).then(|| {
        auth(
            HttpRequest::get(format!("{API_URL}/tasks?external_task_ids={external_id}")),
            key,
        )
    })
}

pub(crate) fn json_body(response: &HttpResponse) -> Result<Value, ProviderFailure> {
    serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable response: {error}")))
}

/// What a refused call means. Kling names the cause in its service code
/// (`code`), with the reason in `message`: 1000-1004 the key, 1100-1102 the
/// account or its resource packages, 1103 a model the account may not use,
/// 1200-1203 the request, 1301 the content rules, 1302-1303 too many
/// requests or tasks at once (they free up as tasks end), 5000-5002 Kling
/// itself.
pub fn kling_failure(response: &HttpResponse) -> ProviderFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
    let message = body["message"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned);
    let with = |kind, fallback: &str| {
        ProviderFailure::new(kind, message.clone().unwrap_or_else(|| fallback.to_owned()))
    };
    match body["code"].as_u64() {
        Some(1000..=1004) => with(ProviderFailureKind::Rejected, "the key was not accepted"),
        Some(1100) => with(
            ProviderFailureKind::LimitReached,
            "the account is not usable",
        ),
        Some(1101 | 1102) => with(
            ProviderFailureKind::LimitReached,
            "the account has no resource package left",
        ),
        Some(1103) => with(
            ProviderFailureKind::NotAllowed,
            "the account may not use this model",
        ),
        Some(1203) => with(
            ProviderFailureKind::NotAllowed,
            "the model is not available to this account",
        ),
        Some(1200..=1202) => with(ProviderFailureKind::Unexpected, "the request is invalid"),
        Some(1300 | 1301) => with(
            ProviderFailureKind::Declined,
            "Kling's content rules declined the image or the prompt",
        ),
        Some(1302 | 1303) => with(ProviderFailureKind::ProviderDown, "too many tasks at once"),
        Some(1304) => with(
            ProviderFailureKind::NotAllowed,
            "Kling refused this network address",
        ),
        Some(5000..=5999) => with(ProviderFailureKind::ProviderDown, "Kling is failing"),
        _ => failure(Provider::Kling, response),
    }
}

/// Whether an answer is a success: HTTP 2xx and service code 0.
pub(crate) fn succeeded(response: &HttpResponse) -> bool {
    (200..=299).contains(&response.status)
        && serde_json::from_str::<Value>(&response.body)
            .ok()
            .is_none_or(|body| body["code"].as_u64().is_none_or(|code| code == 0))
}

fn task_handle(task: &Value) -> Result<ClipHandle, ProviderFailure> {
    task["id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| task["id"].as_u64().map(|id| id.to_string()))
        .filter(|id| is_task_id(id))
        .map(ClipHandle)
        .ok_or_else(|| unexpected("the task has no id"))
}

/// Reads a submission's answer: the task id.
pub fn parse_submission(response: &HttpResponse) -> Result<ClipHandle, ProviderFailure> {
    task_handle(&json_body(response)?["data"])
}

/// Reads a lookup's answer: the task, when Kling has one by that id.
pub fn parse_lookup(response: &HttpResponse) -> Result<Option<ClipHandle>, ProviderFailure> {
    let body = json_body(response)?;
    match body["data"].as_array().and_then(|tasks| tasks.first()) {
        Some(task) => task_handle(task).map(Some),
        None => Ok(None),
    }
}

/// Whether Kling's reason for a failed task is its content rules.
fn is_moderation(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "risk",
        "sensitive",
        "safety",
        "security",
        "moderation",
        "policy",
    ]
    .iter()
    .any(|word| message.contains(word))
}

/// Reads a status answer.
pub fn parse_status(response: &HttpResponse) -> Result<ClipStatus, ProviderFailure> {
    let body = json_body(response)?;
    let Some(task) = body["data"].as_array().and_then(|tasks| tasks.first()) else {
        return Ok(ClipStatus::Failed(unexpected(
            "Kling does not know this task",
        )));
    };
    let message = task["message"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    match task["status"].as_str() {
        Some("submitted") => Ok(ClipStatus::Queued),
        Some("processing") => Ok(ClipStatus::Running),
        Some("succeeded" | "succeed") => task["outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|output| output["type"].as_str() == Some("video"))
            .find_map(|output| output["url"].as_str())
            .filter(|url| url.starts_with("https://"))
            .map(|url| ClipStatus::Done {
                video: url.to_owned(),
            })
            .ok_or_else(|| unexpected("the finished task has no video")),
        Some("failed") => Ok(ClipStatus::Failed(match message {
            Some(message) if is_moderation(message) => ProviderFailure::new(
                ProviderFailureKind::Declined,
                format!("Kling's content rules declined the clip: {message}"),
            ),
            Some(message) => ProviderFailure::new(
                ProviderFailureKind::ProviderDown,
                format!("Kling could not make the clip: {message}"),
            ),
            None => ProviderFailure::new(
                ProviderFailureKind::ProviderDown,
                "Kling could not make the clip",
            ),
        })),
        Some(other) => Err(unexpected(format!("unknown task status {other}"))),
        None => Err(unexpected("the status answer has no status")),
    }
}

/// Whether `bytes` look like an MP4 file (an `ftyp` box first).
fn is_mp4(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp")
}

/// Makes clips with Kling's models, over HTTPS.
pub struct KlingClips<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl KlingClips {
    /// Submissions carry the image and downloads the clip: megabytes.
    pub const TIMEOUT: Duration = Duration::from_secs(300);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for KlingClips {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> KlingClips<T> {
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

    pub(crate) fn send(&self, request: &HttpRequest) -> Result<HttpResponse, ProviderFailure> {
        self.backoff
            .send(&self.transport, request, &self.sleep)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))
    }

    /// The task Kling already has under this custom id, if any.
    pub(crate) fn find(&self, key: &ApiKey, external_id: &str) -> Option<ClipHandle> {
        let response = self.send(&lookup_request(key, external_id)?).ok()?;
        if !succeeded(&response) {
            return None;
        }
        parse_lookup(&response).ok().flatten()
    }
}

impl<T: Transport> ClipGenerator for KlingClips<T> {
    fn provider(&self) -> Provider {
        Provider::Kling
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

    fn submit(
        &self,
        key: &ApiKey,
        request: &ClipRequest,
        submission: &str,
    ) -> Result<ClipSubmission, ProviderFailure> {
        let not_offered = || {
            ProviderFailure::new(
                ProviderFailureKind::NotAllowed,
                format!("{} is not a Kling video model", request.model),
            )
        };
        let http = submit_request(key, request, submission).ok_or_else(not_offered)?;
        let response = self.send(&http)?;
        if succeeded(&response) {
            return Ok(ClipSubmission {
                handle: parse_submission(&response)?,
                quote: None,
            });
        }
        let refused = kling_failure(&response);
        // A refused request may be this very submission sent a second time
        // (Kling took the first, its answer was lost): find the first.
        if refused.kind == ProviderFailureKind::Unexpected {
            let external = external_task_id(request, submission).ok_or_else(not_offered)?;
            if let Some(handle) = self.find(key, &external) {
                return Ok(ClipSubmission {
                    handle,
                    quote: None,
                });
            }
        }
        Err(refused)
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<ClipStatus, ProviderFailure> {
        let http = status_request(key, handle)
            .ok_or_else(|| unexpected(format!("{} is not a Kling task", handle.0)))?;
        let response = self.send(&http)?;
        if succeeded(&response) {
            parse_status(&response)
        } else {
            Err(kling_failure(&response))
        }
    }

    /// The CDN gets no key.
    fn download(&self, _key: &ApiKey, video: &str) -> Result<GeneratedClip, ProviderFailure> {
        if !video.starts_with("https://") {
            return Err(unexpected("the clip's address is not HTTPS"));
        }
        let response = self
            .transport
            .send_for_bytes(&HttpRequest::get(video), MAX_CLIP_BYTES)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        match response.status {
            200..=299 if is_mp4(&response.bytes) => Ok(GeneratedClip {
                bytes: response.bytes,
            }),
            200..=299 => Err(unexpected("the download is not an MP4 clip")),
            403 | 404 | 410 => Err(unexpected(
                "the clip is no longer available to download (Kling keeps it for 30 days)",
            )),
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
            let (_, model, resolution) = super::spec(spec.id).unwrap();
            assert!(
                model.starts_with("kling-") && !model.contains('/'),
                "{}",
                spec.id
            );
            assert!(["720p", "1080p"].contains(&resolution), "{}", spec.id);
            assert!((spec.fixed)().is_object(), "{}", spec.id);
        }
    }

    #[test]
    fn task_ids_cannot_change_the_query() {
        assert!(is_task_id("893605946402811985"));
        assert!(is_task_id("bardo-0123abcd"));
        assert!(!is_task_id(""));
        assert!(!is_task_id("1&external_task_ids=2"));
        assert!(!is_task_id("../account"));
        assert!(!is_task_id(&"9".repeat(129)));
    }

    #[test]
    fn failed_tasks_name_moderation_apart() {
        assert!(is_moderation("Failure to pass the risk control system"));
        assert!(is_moderation("Content SECURITY check failed"));
        assert!(!is_moderation("internal error"));
    }

    #[test]
    fn mp4_files_start_with_an_ftyp_box() {
        assert!(is_mp4(b"\0\0\0\x18ftypisom"));
        assert!(!is_mp4(b"<html>"));
        assert!(!is_mp4(b""));
    }
}
