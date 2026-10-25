//! Performed takes straight from BytePlus ModelArk (#194, ADR-0025):
//! Seedance 2.5's video edit with its draft mode. A draft is a 480p take
//! made with `draft: true`; its final is a request naming only the draft's
//! task, which ModelArk makes at 1080p from the draft's own prompt,
//! inputs, seed and settings.
//!
//! ModelArk reads reference videos only from a public link (or its asset
//! library), and the final reads the draft's inputs again. So the control
//! video and the look are uploaded to the user's own TOS bucket, and
//! ModelArk gets presigned links to them that last seven days, as long as
//! a draft can be finished. Bardo deletes them once they are no longer
//! needed (`unstage`).
//!
//! The ModelArk API key goes as `Authorization: Bearer`; the TOS access
//! key pair only signs links (TOS's own `TOS4-HMAC-SHA256`, from
//! docs.byteplus.com/en/docs/tos), so it never leaves the computer.
//!
//! ModelArk takes no idempotency key: a submission whose answer was lost
//! is found again through the task list (`find_submitted`).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, BytePlusCredentials, ClipHandle, ClipModelRef, GeneratedClip, KeyCheck,
    KeyCheckOutcome, Metered, Money, Provider, ProviderFailure, ProviderFailureKind, StagedFile,
    TakeDone, TakeGenerator, TakeInput, TakeMode, TakeModel, TakeRequest, TakeStatus, TakeStep,
    TakeTier,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::http::{HttpRequest, HttpResponse, Method, Transport, UreqTransport};
use crate::retry::{Backoff, Sleeper, thread_sleeper};

/// ModelArk's API outside mainland China.
pub const API_URL: &str = "https://ark.ap-southeast.bytepluses.com/api/v3";

/// Seedance 2.5, the model with draft mode and video edit.
pub const MODEL: &str = "dreamina-seedance-2-5-260628";

/// How long a draft can be finished after it is made.
pub const DRAFT_LIFETIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How long the link ModelArk reads a staged file from lasts: TOS's
/// longest, which covers the draft's lifetime.
pub const LINK_SECONDS: u64 = 604_800;

/// How long an upload or delete link lasts.
const SHORT_LINK_SECONDS: u64 = 900;

/// A 30-second 1080p take with 10-bit color is tens of megabytes.
pub const MAX_TAKE_BYTES: u64 = 512 * 1024 * 1024;

/// The longest prompt passed on, as for Higgsfield.
const MAX_PROMPT_CHARS: usize = 2_500;

/// Where Bardo puts what it stages in the user's bucket.
const STAGING_PREFIX: &str = "bardo/takes/";

/// A task id no task has, for the free key check.
pub const NO_SUCH_TASK: &str = "cgt-bardo-key-check";

/// The object the key check writes, reads and deletes, under the prefix
/// the guide's access policy covers.
const KEY_CHECK_OBJECT: &str = "bardo/takes/key-check.txt";

/// How far before the saved send time a task found in the list may have
/// been created: the two clocks differ a little.
const CLOCK_SLACK: Duration = Duration::from_secs(120);

/// The longest provider message passed on.
const MAX_DETAIL_CHARS: usize = 300;

/// The take model Bardo offers on BytePlus, with its limits from
/// ModelArk's Seedance 2.5 docs (checked on 2026-10-23).
pub fn take_models() -> Vec<TakeModel> {
    vec![TakeModel {
        id: ClipModelRef::new(Provider::BytePlus, MODEL).expect("the model id is valid"),
        name: "Seedance 2.5 Video Edit".to_owned(),
        // It refuses real faces, so only control videos that show none.
        modes: vec![TakeMode::Mesh, TakeMode::Depth],
        draft: Some(TakeTier {
            resolution: Some("480p".to_owned()),
            height: 480,
            model: None,
        }),
        final_tier: TakeTier {
            resolution: Some("1080p".to_owned()),
            height: 1080,
            model: None,
        },
        finishes_draft: true,
        draft_lifetime: Some(DRAFT_LIFETIME),
        look_assets: true,
        // Billed by the video's tokens, counted at 24 frames a second.
        token_frame_rate: Some(24),
        // It edits the control video in place.
        keeps_framing: true,
        // Video edit takes 4 to 30 seconds.
        min_length: Duration::from_secs(4),
        max_length: Duration::from_secs(30),
        max_video_bytes: 200_000_000,
        max_image_bytes: 10_000_000,
    }]
}

/// The JSON body of a draft or final. A draft is a video edit of the
/// control video into the look, without sound or watermark; a final names
/// only its draft's task, since ModelArk reuses everything else and
/// refuses it sent again.
pub fn take_body(request: &TakeRequest) -> Value {
    let resolution = request.resolution.clone().unwrap_or_else(|| {
        match request.step {
            // A test is sent as a draft.
            TakeStep::Draft | TakeStep::Test => "480p",
            TakeStep::Final => "1080p",
        }
        .to_owned()
    });
    if let Some(draft) = &request.draft_task {
        return json!({
            "model": request.model,
            "content": [{ "type": "draft_task", "draft_task": { "id": draft.0 } }],
            "resolution": resolution,
            "watermark": false,
        });
    }
    let prompt: String = request.prompt.chars().take(MAX_PROMPT_CHARS).collect();
    let mut body = json!({
        "model": request.model,
        "content": [
            { "type": "text", "text": prompt },
            {
                "type": "video_url",
                "video_url": { "url": request.video.0 },
                "role": "reference_video",
            },
            {
                "type": "image_url",
                "image_url": { "url": request.look.0 },
                "role": "reference_image",
            },
        ],
        "omni_reference_task_type": "edit",
        "ratio": "adaptive",
        "duration": -1,
        "resolution": resolution,
        "generate_audio": false,
        "watermark": false,
    });
    if matches!(request.step, TakeStep::Draft | TakeStep::Test) {
        body["draft"] = json!(true);
    }
    body
}

fn credentials(key: &ApiKey) -> Result<BytePlusCredentials<'_>, ProviderFailure> {
    BytePlusCredentials::of(key).ok_or_else(|| {
        ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "the saved BytePlus key is missing a field",
        )
    })
}

fn bearer(request: HttpRequest, credentials: &BytePlusCredentials<'_>) -> HttpRequest {
    request.header("authorization", format!("Bearer {}", credentials.api_key))
}

/// The request that starts a draft or final.
/// The submission, with each staged file's link signed for ModelArk to
/// read at `at`. Bardo keeps only the unsigned links: a signed one names
/// the access key and opens the file to whoever holds it.
pub fn submit_request(
    credentials: &BytePlusCredentials<'_>,
    request: &TakeRequest,
    at: SystemTime,
) -> HttpRequest {
    let signed = |file: &StagedFile| match object_of(credentials, &file.0) {
        Some(object) => StagedFile(presign(credentials, Method::Get, &object, LINK_SECONDS, at)),
        None => file.clone(),
    };
    let request = TakeRequest {
        video: signed(&request.video),
        look: signed(&request.look),
        ..request.clone()
    };
    bearer(
        HttpRequest::post_json(
            format!("{API_URL}/contents/generations/tasks"),
            take_body(&request).to_string(),
        ),
        credentials,
    )
}

pub fn status_request(credentials: &BytePlusCredentials<'_>, handle: &ClipHandle) -> HttpRequest {
    bearer(
        HttpRequest::get(format!(
            "{API_URL}/contents/generations/tasks/{}",
            percent(&handle.0, false)
        )),
        credentials,
    )
}

/// The model's latest tasks, for finding a submission whose answer was
/// lost.
pub fn list_request(credentials: &BytePlusCredentials<'_>, model: &str) -> HttpRequest {
    bearer(
        HttpRequest::get(format!(
            "{API_URL}/contents/generations/tasks?page_num=1&page_size=500&filter.model={}",
            percent(model, false)
        )),
        credentials,
    )
}

fn unexpected(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

fn cut(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_DETAIL_CHARS {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(MAX_DETAIL_CHARS).collect();
    cut.push('…');
    cut
}

/// ModelArk's error code and message, as `code: message`.
fn error_text(error: &Value) -> Option<(String, String)> {
    let code = error["code"].as_str().unwrap_or_default().trim().to_owned();
    let message = cut(error["message"].as_str().unwrap_or_default());
    (!code.is_empty() || !message.is_empty()).then_some((code, message))
}

fn joined(code: &str, message: &str) -> String {
    match (code.is_empty(), message.is_empty()) {
        (false, false) => format!("{code}: {message}"),
        (false, true) => code.to_owned(),
        _ => message.to_owned(),
    }
}

/// Whether ModelArk's error code is a content refusal (its sensitive
/// content, copyright and real-person checks).
fn refused(code: &str) -> bool {
    code.contains("SensitiveContentDetected")
}

/// What a refused call means, from ModelArk's error codes.
pub fn modelark_failure(response: &HttpResponse) -> ProviderFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
    let (code, message) = error_text(&body["error"]).unwrap_or_default();
    let detail = |fallback: &str| {
        let text = joined(&code, &message);
        if text.is_empty() {
            fallback.to_owned()
        } else {
            text
        }
    };
    let kind = match response.status {
        400 if refused(&code) => ProviderFailureKind::Declined,
        400 | 422 => ProviderFailureKind::Unexpected,
        401 => ProviderFailureKind::Rejected,
        403 if code == "AccountOverdueError" || code.ends_with("ServiceOverdue") => {
            ProviderFailureKind::LimitReached
        }
        403 | 404 => ProviderFailureKind::NotAllowed,
        // ModelArk busy, not a limit of the user's.
        429 if code == "ServerOverloaded" || code == "RequestBurstTooFast" => {
            ProviderFailureKind::ProviderDown
        }
        429 => ProviderFailureKind::LimitReached,
        500..=599 => ProviderFailureKind::ProviderDown,
        _ => ProviderFailureKind::Unexpected,
    };
    ProviderFailure::new(kind, detail(&format!("HTTP {}", response.status)))
}

fn json_body(response: &HttpResponse) -> Result<Value, ProviderFailure> {
    serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable response: {error}")))
}

/// Reads a submission's answer: the task id.
pub fn parse_submission(response: &HttpResponse) -> Result<ClipHandle, ProviderFailure> {
    let body = json_body(response)?;
    body["id"]
        .as_str()
        .filter(|id| is_task_id(id))
        .map(|id| ClipHandle(id.to_owned()))
        .ok_or_else(|| unexpected("the submission answer has no task id"))
}

fn is_task_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Reads a task's status.
pub fn parse_status(response: &HttpResponse) -> Result<TakeStatus, ProviderFailure> {
    let body = json_body(response)?;
    task_status(&body)
}

fn task_status(task: &Value) -> Result<TakeStatus, ProviderFailure> {
    match task["status"].as_str() {
        Some("queued") => Ok(TakeStatus::Queued),
        Some("running") => Ok(TakeStatus::Running),
        Some("succeeded") => {
            let video = task["content"]["video_url"]
                .as_str()
                .filter(|url| url.starts_with("https://"))
                .ok_or_else(|| unexpected("the finished task has no video"))?;
            let usage = task["usage"]["completion_tokens"]
                .as_u64()
                .or_else(|| task["usage"]["total_tokens"].as_u64())
                .map(|tokens| Metered {
                    output_tokens: tokens,
                    ..Metered::default()
                });
            Ok(TakeStatus::Done(TakeDone {
                video: video.to_owned(),
                usage,
                seed: task["seed"].as_i64(),
            }))
        }
        Some("failed") => {
            let (code, message) = error_text(&task["error"]).unwrap_or_default();
            let text = joined(&code, &message);
            let failure = if refused(&code) {
                ProviderFailure::new(ProviderFailureKind::Declined, text)
            } else if code.starts_with("InvalidParameter") {
                unexpected(format!("BytePlus could not make the take: {text}"))
            } else if text.is_empty() {
                ProviderFailure::new(
                    ProviderFailureKind::ProviderDown,
                    "BytePlus could not make the take",
                )
            } else {
                ProviderFailure::new(
                    ProviderFailureKind::ProviderDown,
                    format!("BytePlus could not make the take: {text}"),
                )
            };
            Ok(TakeStatus::Failed(failure))
        }
        Some("expired") => Ok(TakeStatus::Failed(ProviderFailure::new(
            ProviderFailureKind::ProviderDown,
            "the take ran out of time on BytePlus",
        ))),
        Some("cancelled") => Ok(TakeStatus::Failed(unexpected(
            "the task was cancelled on BytePlus",
        ))),
        Some(other) => Err(unexpected(format!("unknown task status {other}"))),
        None => Err(unexpected("the status answer has no status")),
    }
}

/// Reads the task list: the one task of `request`'s kind created since
/// `since`, `None` when there is none.
pub fn parse_list(
    response: &HttpResponse,
    request: &TakeRequest,
    since: SystemTime,
) -> Result<Vec<ClipHandle>, ProviderFailure> {
    let body = json_body(response)?;
    let items = body["items"]
        .as_array()
        .ok_or_else(|| unexpected("the task list has no items"))?;
    let from = since
        .checked_sub(CLOCK_SLACK)
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let found: Vec<&str> = items
        .iter()
        .filter(|item| item["created_at"].as_u64().is_some_and(|at| at >= from))
        .filter(|item| match &request.draft_task {
            Some(draft) => item["draft_task_id"].as_str() == Some(draft.0.as_str()),
            None => {
                item["draft"].as_bool()
                    == Some(matches!(request.step, TakeStep::Draft | TakeStep::Test))
                    && item["draft_task_id"].as_str().is_none_or(str::is_empty)
            }
        })
        .filter_map(|item| item["id"].as_str().filter(|id| is_task_id(id)))
        .collect();
    Ok(found
        .into_iter()
        .map(|id| ClipHandle(id.to_owned()))
        .collect())
}

// TOS, the user's bucket.

/// The bucket's host, e.g. `bardo.tos-ap-southeast-1.bytepluses.com`.
pub fn tos_host(credentials: &BytePlusCredentials<'_>) -> String {
    format!(
        "{}.tos-{}.bytepluses.com",
        credentials.bucket, credentials.region
    )
}

/// Percent-encodes `text` as TOS signs it: letters, digits and `-_.~`
/// stay, and `/` too in a path.
fn percent(text: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (keep_slash && byte == b'/')
        {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// HMAC-SHA256 (RFC 2104).
fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let mut inner = Sha256::new();
    inner.update(pad(0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(pad(0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// `at` as TOS dates a signature: `20220101T000000Z`.
fn tos_date(at: SystemTime) -> String {
    humantime::format_rfc3339_seconds(at)
        .to_string()
        .replace(['-', ':'], "")
}

/// A presigned link for `method` on `object` in the user's bucket, valid
/// for `seconds` from `at` (TOS4-HMAC-SHA256, query signing, only the
/// host signed, `UNSIGNED-PAYLOAD`).
pub fn presign(
    credentials: &BytePlusCredentials<'_>,
    method: Method,
    object: &str,
    seconds: u64,
    at: SystemTime,
) -> String {
    let host = tos_host(credentials);
    let date = tos_date(at);
    let day = &date[..8];
    let scope = format!("{day}/{}/tos/request", credentials.region);
    let path = format!("/{}", percent(object, true));
    let mut params = [
        ("X-Tos-Algorithm", "TOS4-HMAC-SHA256".to_owned()),
        (
            "X-Tos-Credential",
            format!("{}/{scope}", credentials.access_key_id),
        ),
        ("X-Tos-Date", date.clone()),
        ("X-Tos-Expires", seconds.to_string()),
        ("X-Tos-SignedHeaders", "host".to_owned()),
    ];
    params.sort_by(|a, b| a.0.cmp(b.0));
    let query = params
        .iter()
        .map(|(name, value)| format!("{}={}", percent(name, false), percent(value, false)))
        .collect::<Vec<_>>()
        .join("&");
    let verb = match method {
        Method::Get => "GET",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
        Method::Post => "POST",
    };
    let canonical = format!("{verb}\n{path}\n{query}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD");
    let to_sign = format!(
        "TOS4-HMAC-SHA256\n{date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let mut key = hmac(credentials.secret_access_key.as_bytes(), day.as_bytes());
    for part in [credentials.region, "tos", "request"] {
        key = hmac(&key, part.as_bytes());
    }
    let signature = hex(&hmac(&key, to_sign.as_bytes()));
    format!("https://{host}{path}?{query}&X-Tos-Signature={signature}")
}

/// The object a staged link points to in the user's bucket; `None` for a
/// link elsewhere (another bucket, or not a staged file).
pub fn object_of(credentials: &BytePlusCredentials<'_>, link: &str) -> Option<String> {
    let rest = link.strip_prefix("https://")?;
    let (host, rest) = rest.split_once('/')?;
    if host != tos_host(credentials) {
        return None;
    }
    let path = rest.split(['?', '#']).next()?;
    path.starts_with(STAGING_PREFIX)
        .then(|| path.to_owned())
        .filter(|path| {
            path.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
        })
}

/// What a refused storage call means, from TOS's error code (JSON or XML).
pub fn tos_failure(response: &HttpResponse) -> ProviderFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
    let code = body["Code"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| xml_field(&response.body, "Code"))
        .unwrap_or_default();
    let message = body["Message"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| xml_field(&response.body, "Message"))
        .unwrap_or_default();
    let kind = match (response.status, code.as_str()) {
        (403, "SignatureDoesNotMatch" | "InvalidAccessKeyId" | "AccessKeyIdNotFound") => {
            ProviderFailureKind::Rejected
        }
        (401, _) => ProviderFailureKind::Rejected,
        (403 | 404, _) => ProviderFailureKind::NotAllowed,
        (429 | 500..=599, _) => ProviderFailureKind::ProviderDown,
        _ => ProviderFailureKind::Unexpected,
    };
    let text = joined(&code, &cut(&message));
    ProviderFailure::new(
        kind,
        if text.is_empty() {
            format!("the bucket answered HTTP {}", response.status)
        } else {
            format!("the bucket: {text}")
        },
    )
}

fn xml_field(body: &str, name: &str) -> Option<String> {
    let start = body.find(&format!("<{name}>"))? + name.len() + 2;
    let end = body[start..].find(&format!("</{name}>"))? + start;
    Some(body[start..end].trim().to_owned())
}

/// The extension a staged file of `mime` type gets.
fn extension(mime: &str) -> &'static str {
    match mime {
        "video/mp4" => "mp4",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        _ => "bin",
    }
}

/// Whether `bytes` look like an MP4 file (an `ftyp` box first).
fn is_mp4(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp")
}

/// Tests a BytePlus key: ModelArk is asked for a task that cannot exist
/// (free; a valid key gets 404), and the bucket for an object that does
/// not exist (one storage request; valid keys get 404 NoSuchKey).
pub fn check_key(transport: &dyn Transport, key: &ApiKey, at: SystemTime) -> KeyCheck {
    let Some(credentials) = BytePlusCredentials::of(key) else {
        return KeyCheck::new(
            KeyCheckOutcome::Rejected,
            Some("the saved key is missing a field".into()),
        );
    };
    let lookup = status_request(&credentials, &ClipHandle(NO_SUCH_TASK.into()));
    match transport.send(&lookup) {
        Ok(response) if (200..=299).contains(&response.status) || response.status == 404 => {}
        Ok(response) => return failure_check("ModelArk", modelark_failure(&response)),
        Err(error) => {
            return KeyCheck::new(KeyCheckOutcome::Unreachable, Some(error.0));
        }
    }
    // The three things takes do with the bucket, on a file of its own:
    // a few storage requests, worth a fraction of a cent.
    let link = |method| {
        presign(
            &credentials,
            method,
            KEY_CHECK_OBJECT,
            SHORT_LINK_SECONDS,
            at,
        )
    };
    let probes = [
        HttpRequest::put(link(Method::Put), b"bardo".to_vec())
            .header("content-type", "text/plain".to_owned()),
        HttpRequest::get(link(Method::Get)),
        HttpRequest::delete(link(Method::Delete)),
    ];
    for probe in &probes {
        match transport.send(probe) {
            Ok(response) if (200..=299).contains(&response.status) => {}
            Ok(response) => return failure_check("TOS", tos_failure(&response)),
            Err(error) => return KeyCheck::new(KeyCheckOutcome::Unreachable, Some(error.0)),
        }
    }
    KeyCheck::new(KeyCheckOutcome::Valid, None)
}

fn failure_check(part: &str, failure: ProviderFailure) -> KeyCheck {
    let outcome = match failure.kind {
        ProviderFailureKind::Rejected => KeyCheckOutcome::Rejected,
        ProviderFailureKind::NotAllowed => KeyCheckOutcome::NotAllowed,
        ProviderFailureKind::LimitReached => KeyCheckOutcome::LimitReached,
        ProviderFailureKind::ProviderDown => KeyCheckOutcome::ProviderDown,
        ProviderFailureKind::Unreachable => KeyCheckOutcome::Unreachable,
        ProviderFailureKind::Unexpected | ProviderFailureKind::Declined => {
            KeyCheckOutcome::Unexpected
        }
    };
    KeyCheck::new(outcome, Some(format!("{part}: {}", failure.detail)))
}

/// Names a staged file: random, so nothing in it names the user.
pub type Namer = Arc<dyn Fn() -> String + Send + Sync>;

/// Makes takes with Seedance 2.5 on ModelArk, over HTTPS.
pub struct BytePlusTakes<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
    clock: fn() -> SystemTime,
    names: Namer,
}

impl BytePlusTakes {
    /// Uploads and downloads move megabytes.
    pub const TIMEOUT: Duration = Duration::from_secs(300);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for BytePlusTakes {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> BytePlusTakes<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            backoff: Backoff::default(),
            sleep: thread_sleeper(),
            clock: SystemTime::now,
            names: Arc::new(|| uuid::Uuid::new_v4().to_string()),
        }
    }

    /// Replaces how the adapter waits between retries.
    pub fn with_sleeper(mut self, sleep: Sleeper) -> Self {
        self.sleep = sleep;
        self
    }

    /// Replaces the clock links are signed with.
    pub fn with_clock(mut self, clock: fn() -> SystemTime) -> Self {
        self.clock = clock;
        self
    }

    /// Replaces how staged files are named.
    pub fn with_names(mut self, names: Namer) -> Self {
        self.names = names;
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
}

impl<T: Transport> TakeGenerator for BytePlusTakes<T> {
    fn provider(&self) -> Provider {
        Provider::BytePlus
    }

    fn models(&self) -> Vec<TakeModel> {
        take_models()
    }

    /// Uploads to the user's bucket through a short presigned link (the
    /// access key never leaves the computer) and returns the seven-day
    /// link ModelArk reads it from.
    fn stage(&self, key: &ApiKey, input: TakeInput<'_>) -> Result<StagedFile, ProviderFailure> {
        let credentials = credentials(key)?;
        let object = format!(
            "{STAGING_PREFIX}{}.{}",
            (self.names)(),
            extension(input.mime)
        );
        let now = (self.clock)();
        let upload = HttpRequest::put(
            presign(&credentials, Method::Put, &object, SHORT_LINK_SECONDS, now),
            input.bytes.to_vec(),
        )
        .header("content-type", input.mime.to_owned());
        let stored = self.send(&upload)?;
        if !(200..=299).contains(&stored.status) {
            return Err(tos_failure(&stored));
        }
        // Unsigned: the submission signs it (`submit_request`).
        Ok(StagedFile(format!(
            "https://{}/{object}",
            tos_host(&credentials)
        )))
    }

    /// ModelArk quotes nothing; the rate table estimates and the task's
    /// tokens are recorded.
    fn quote(&self, _key: &ApiKey, _request: &TakeRequest) -> Option<Money> {
        None
    }

    /// Sent once, never retried here: ModelArk takes no idempotency key.
    fn submit(
        &self,
        key: &ApiKey,
        request: &TakeRequest,
        _submission: &str,
    ) -> Result<ClipHandle, ProviderFailure> {
        let credentials = credentials(key)?;
        let response = self
            .transport
            .send(&submit_request(&credentials, request, (self.clock)()))
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        if !(200..=299).contains(&response.status) {
            return Err(modelark_failure(&response));
        }
        parse_submission(&response)
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<TakeStatus, ProviderFailure> {
        let credentials = credentials(key)?;
        let response = self.send(&status_request(&credentials, handle))?;
        match response.status {
            200..=299 => parse_status(&response),
            // Tasks are kept seven days.
            404 => Ok(TakeStatus::Failed(unexpected(
                "BytePlus does not know this task",
            ))),
            _ => Err(modelark_failure(&response)),
        }
    }

    /// The video link takes no key.
    fn download(&self, _key: &ApiKey, video: &str) -> Result<GeneratedClip, ProviderFailure> {
        let response = self
            .transport
            .send_for_bytes(&HttpRequest::get(video), MAX_TAKE_BYTES)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        match response.status {
            200..=299 if is_mp4(&response.bytes) => Ok(GeneratedClip {
                bytes: response.bytes,
            }),
            200..=299 => Err(unexpected("the download is not an MP4 video")),
            403 | 404 | 410 => Err(unexpected("the take is no longer available to download")),
            status => Err(ProviderFailure::new(
                ProviderFailureKind::ProviderDown,
                format!("the download failed (HTTP {status})"),
            )),
        }
    }

    fn idempotent(&self) -> bool {
        false
    }

    fn find_submitted(
        &self,
        key: &ApiKey,
        request: &TakeRequest,
        since: SystemTime,
    ) -> Result<Vec<ClipHandle>, ProviderFailure> {
        let credentials = credentials(key)?;
        let response = self.send(&list_request(&credentials, &request.model))?;
        if !(200..=299).contains(&response.status) {
            return Err(modelark_failure(&response));
        }
        parse_list(&response, request, since)
    }

    fn keeps_staged(&self) -> bool {
        true
    }

    /// A seven-day link to a file staged in the user's bucket, signed now
    /// for another provider to read (Kling's takes, #243).
    fn link(&self, key: &ApiKey, file: &StagedFile) -> Result<String, ProviderFailure> {
        let credentials = credentials(key)?;
        let object = object_of(&credentials, &file.0).ok_or_else(|| {
            ProviderFailure::new(
                ProviderFailureKind::NotAllowed,
                "the file is not in the bucket the saved key names",
            )
        })?;
        Ok(presign(
            &credentials,
            Method::Get,
            &object,
            LINK_SECONDS,
            (self.clock)(),
        ))
    }

    fn unstage(&self, key: &ApiKey, file: &StagedFile) -> Result<(), ProviderFailure> {
        let credentials = credentials(key)?;
        let Some(object) = object_of(&credentials, &file.0) else {
            return Err(ProviderFailure::new(
                ProviderFailureKind::NotAllowed,
                "the file is not in the bucket the saved key names",
            ));
        };
        let request = HttpRequest::delete(presign(
            &credentials,
            Method::Delete,
            &object,
            SHORT_LINK_SECONDS,
            (self.clock)(),
        ));
        let response = self.send(&request)?;
        match response.status {
            200..=299 | 404 => Ok(()),
            // The key may not delete there: the file stays listed and is
            // tried again, rather than forgotten as gone.
            403 => Err(ProviderFailure {
                kind: ProviderFailureKind::Rejected,
                ..tos_failure(&response)
            }),
            _ => Err(tos_failure(&response)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc_4231() {
        // Test case 2.
        assert_eq!(
            hex(&hmac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6: a key longer than a block.
        assert_eq!(
            hex(&hmac(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn dates_and_paths_are_written_as_tos_signs_them() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_640_995_200);
        assert_eq!(tos_date(at), "20220101T000000Z");
        assert_eq!(
            percent("bardo/takes/a b.mp4", true),
            "bardo/takes/a%20b.mp4"
        );
        assert_eq!(percent("a/b", false), "a%2Fb");
    }
}
