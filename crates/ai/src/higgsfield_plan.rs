//! Video clips paid from the credits of the user's higgsfield.ai plan,
//! through Higgsfield's official MCP server (ADR-0028). Higgsfield's API
//! (`higgsfield.rs`) is a separate product with its own dollar balance;
//! plan credits can only be spent on the website or through this server,
//! which Higgsfield's terms open to third-party MCP clients (§11.13).
//!
//! **Sign-in** is OAuth 2.1 as MCP asks: Bardo registers itself as a public
//! client (dynamic client registration) that returns to the loopback
//! listener, the user consents in the browser, and the code is traded for
//! tokens with PKCE. Access tokens are refreshed with the refresh token
//! (`offline_access`).
//!
//! **Clips** go through the server's tools: the scene's image (and the
//! frame a clip ends on, for the models that take one) is uploaded
//! (`media_upload`, a presigned PUT, `media_confirm`), the clip is started
//! with `generate_video_batch` (one request, no widget), and its state is
//! read with `jobs_wait` without waiting. `generate_video_batch` takes no
//! idempotency key, so a submission is never sent twice by the adapter:
//! before submitting, it looks for a generation already made from the same
//! uploaded image (the job keeps that image across attempts), and after an
//! answer that may have been lost it reports a lost answer so the job
//! looks again instead of paying twice.
//!
//! **Credits.** Before starting, the free `estimate_video_cost` and
//! `balance` tools tell whether the plan can pay; the clip is refused
//! with "not enough credits" when it cannot. The plan is a subscription,
//! so the rate table prices clips in dollars (`rates.toml`).

use std::time::Duration;

use bardo_domain::{
    ApiKey, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipModel, ClipModelRef,
    ClipRequest, ClipStatus, ClipSubmission, ConsentRequest, Credits, GeneratedClip, PlanAccount,
    PlanSignIn, Provider, ProviderFailure, ProviderFailureKind, SecretText, SignInFailure,
    SignInFailureKind, StagedImage, TokenGrant,
};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::mcp::{CallFailure, McpClient, McpError};

/// Higgsfield's MCP server.
pub const MCP_URL: &str = "https://mcp.higgsfield.ai/mcp";
/// Where its OAuth endpoints are, as its authorization server metadata
/// (`/.well-known/oauth-authorization-server`) lists them.
pub const AUTH_URL: &str = "https://mcp.higgsfield.ai/oauth2";
/// `offline_access` brings a refresh token.
pub const SCOPES: &str = "openid email offline_access";

/// The model a channel uses until the user picks another, as in the API
/// adapter: Kling 3.0 Standard takes any length from 3 to 15 seconds.
pub const DEFAULT_MODEL: &str = "kling3_0/std";

/// A 15-second 1080p clip is tens of megabytes; this leaves room.
pub const MAX_CLIP_BYTES: u64 = 512 * 1024 * 1024;

/// Longest prompt passed on, as for the API's models.
const MAX_PROMPT_CHARS: usize = 2_500;

/// How many generations a page of the lookup for a lost submission
/// reads, and how many pages: the newest 150.
const PAGE: u32 = 30;
const PAGES: usize = 5;

/// A model Bardo offers through the plan, as `{model}` or
/// `{model}/{variant}`, and the settings its request needs besides the
/// prompt, image, length and frame shape. Narration is the soundtrack, so
/// models that can make sound are told not to.
struct Spec {
    id: &'static str,
    model: &'static str,
    /// Model, version and variant.
    name: [&'static str; 3],
    durations: fn() -> ClipDurations,
    fixed: fn() -> Value,
    /// Whether it takes an `end_image`, the frame the clip ends on (the
    /// server's `models_explore`, checked on 2026-11-28).
    ends_on_frame: bool,
}

const CATALOG: &[Spec] = &[
    Spec {
        id: DEFAULT_MODEL,
        model: "kling3_0",
        name: ["Kling", "3.0", "Standard"],
        durations: || ClipDurations::Range { min: 3, max: 15 },
        fixed: || json!({ "mode": "std", "sound": "off" }),
        ends_on_frame: true,
    },
    Spec {
        id: "kling3_0/pro",
        model: "kling3_0",
        name: ["Kling", "3.0", "Pro"],
        durations: || ClipDurations::Range { min: 3, max: 15 },
        fixed: || json!({ "mode": "pro", "sound": "off" }),
        ends_on_frame: true,
    },
    Spec {
        id: "kling2_6",
        model: "kling2_6",
        name: ["Kling", "2.6", ""],
        durations: || ClipDurations::Choices(vec![5, 10]),
        fixed: || json!({ "sound": false }),
        ends_on_frame: false,
    },
    Spec {
        id: "seedance_2_0/1080p",
        model: "seedance_2_0",
        name: ["Seedance", "2.0", ""],
        durations: || ClipDurations::Range { min: 4, max: 15 },
        fixed: || json!({ "resolution": "1080p", "generate_audio": false }),
        ends_on_frame: true,
    },
    Spec {
        id: "wan2_7/1080p",
        model: "wan2_7",
        name: ["Wan", "2.7", ""],
        durations: || ClipDurations::Range { min: 2, max: 15 },
        fixed: || json!({ "resolution": "1080p" }),
        ends_on_frame: true,
    },
    Spec {
        id: "minimax_hailuo/2.3",
        model: "minimax_hailuo",
        name: ["Hailuo", "2.3", ""],
        durations: || ClipDurations::Choices(vec![6, 10]),
        fixed: || json!({ "variant": "minimax-2.3", "resolution": "768" }),
        ends_on_frame: true,
    },
];

fn spec(id: &str) -> Option<&'static Spec> {
    CATALOG.iter().find(|spec| spec.id == id)
}

/// The models Bardo offers through the plan, the default first.
pub fn models() -> Vec<ClipModel> {
    CATALOG
        .iter()
        .map(|spec| ClipModel {
            id: ClipModelRef::new(Provider::HiggsfieldPlan, spec.id)
                .expect("catalog models are valid"),
            name: bardo_domain::ModelName::new(spec.name[0], spec.name[1], spec.name[2]),
            durations: (spec.durations)(),
            ends_on_frame: spec.ends_on_frame,
        })
        .collect()
}

/// The `params` of a generation: the request's fields and the model's
/// fixed ones. Credits only: the plan's free-trial allowance is never
/// spent without the user asking, and leaving the field out makes the
/// server ask instead of starting. `None` for a model the plan does not
/// offer here, or a last frame to a model that takes none.
pub fn clip_params(request: &ClipRequest) -> Option<Value> {
    let spec = spec(&request.model)?;
    if request.last.is_some() && !spec.ends_on_frame {
        return None;
    }
    let mut params: Map<String, Value> = (spec.fixed)().as_object().cloned().unwrap_or_default();
    let prompt: String = request.prompt.chars().take(MAX_PROMPT_CHARS).collect();
    params.insert("model".into(), json!(spec.model));
    params.insert("prompt".into(), json!(prompt));
    params.insert("duration".into(), json!(request.seconds));
    params.insert("aspect_ratio".into(), json!(request.aspect.code()));
    let mut medias = vec![json!({ "role": "start_image", "value": request.image.0 })];
    if let Some(last) = &request.last {
        medias.push(json!({ "role": "end_image", "value": last.image.0 }));
    }
    params.insert("medias".into(), Value::Array(medias));
    params.insert("use_unlim".into(), json!(false));
    Some(Value::Object(params))
}

/// Job and media ids are UUIDs; nothing else goes into a request.
fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn unexpected(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

/// What an MCP failure means for a clip.
fn clip_failure(error: McpError) -> ProviderFailure {
    let kind = match &error {
        McpError::Unauthorized => {
            return ProviderFailure::new(
                ProviderFailureKind::Rejected,
                "Higgsfield refused the sign-in; connect the plan again",
            );
        }
        McpError::Forbidden(_) => ProviderFailureKind::NotAllowed,
        McpError::Unreachable(_) => ProviderFailureKind::Unreachable,
        McpError::RateLimited | McpError::ServerDown(_) => ProviderFailureKind::ProviderDown,
        McpError::Tool(text) => tool_failure_kind(text),
        McpError::Protocol(_) => ProviderFailureKind::Unexpected,
    };
    ProviderFailure::new(kind, error.to_string())
}

/// What a failed `generate_video_batch` means. Refusals (sign-in,
/// permission, rate limit, the tool's own error) started nothing; a call
/// never sent neither. Anything else may have started a clip, so it is
/// `AnswerLost`: the job looks for the clip instead of paying again.
fn submit_failure(failure: CallFailure) -> ProviderFailure {
    match failure.error {
        McpError::RateLimited => ProviderFailure::new(
            ProviderFailureKind::ProviderDown,
            "too many calls; nothing was started",
        ),
        error @ (McpError::Unauthorized | McpError::Forbidden(_) | McpError::Tool(_)) => {
            clip_failure(error)
        }
        error if !failure.sent => ProviderFailure::new(
            ProviderFailureKind::ProviderDown,
            format!("{error}; nothing was sent"),
        ),
        error => ProviderFailure::new(
            ProviderFailureKind::AnswerLost,
            format!("{error}; the clip may have started"),
        ),
    }
}

/// The parameters of a request, or why it cannot be sent.
fn submit_params(request: &ClipRequest) -> Result<Value, ProviderFailure> {
    if !is_uuid(&request.image.0) {
        return Err(unexpected("the scene's image was not uploaded"));
    }
    let params = clip_params(request).ok_or_else(|| match spec(&request.model) {
        Some(_) => bardo_domain::no_last_frame(&request.model),
        None => ProviderFailure::new(
            ProviderFailureKind::NotAllowed,
            format!("{} is not a model of the Higgsfield plan", request.model),
        ),
    })?;
    if request
        .last
        .as_ref()
        .is_some_and(|last| !is_uuid(&last.image.0))
    {
        return Err(unexpected("the last frame was not uploaded"));
    }
    Ok(params)
}

/// What a tool's own failure message is about.
fn tool_failure_kind(text: &str) -> ProviderFailureKind {
    let text = text.to_ascii_lowercase();
    // Moderation first: "credits were not charged for flagged content" is
    // about the content, not the balance.
    if ["nsfw", "moderation", "policy", "ip_detected", "safety"]
        .iter()
        .any(|word| text.contains(word))
    {
        ProviderFailureKind::Declined
    } else if text.contains("credit") || text.contains("balance") || text.contains("upgrade") {
        ProviderFailureKind::LimitReached
    } else if text.contains("unknown model") || text.contains("not available") {
        ProviderFailureKind::NotAllowed
    } else {
        ProviderFailureKind::Unexpected
    }
}

/// What an MCP failure means for the sign-in.
fn sign_in_failure(error: McpError) -> SignInFailure {
    let kind = match &error {
        McpError::Unauthorized => SignInFailureKind::Refused,
        McpError::Forbidden(_) => SignInFailureKind::NotAllowed,
        McpError::Unreachable(_) => SignInFailureKind::Unreachable,
        McpError::RateLimited => SignInFailureKind::LimitReached,
        McpError::ServerDown(_) => SignInFailureKind::NetworkDown,
        McpError::Tool(_) | McpError::Protocol(_) => SignInFailureKind::Unexpected,
    };
    SignInFailure::new(kind, error.to_string())
}

/// The credits in a tool's answer: `credits` or `cost.credits`.
fn credits_in(value: &Value) -> Option<Credits> {
    value["credits"]
        .as_f64()
        .or_else(|| value["cost"]["credits_exact"].as_f64())
        .or_else(|| value["cost"]["credits"].as_f64())
        .and_then(Credits::from_number)
}

/// Reads `balance`'s answer.
pub fn parse_account(value: &Value) -> Option<PlanAccount> {
    Some(PlanAccount {
        plan: value["subscription_plan_type"]
            .as_str()
            .or_else(|| value["plan"].as_str())
            .unwrap_or("free")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "-_ ".contains(*c))
            .take(40)
            .collect(),
        credits: credits_in(value)?,
    })
}

/// The id of the job a batch submission started: the entry with index 0,
/// or the only one.
pub fn parse_submitted_job(value: &Value) -> Option<String> {
    let jobs = value["jobs"].as_array()?;
    let job = jobs
        .iter()
        .find(|job| job["index"].as_u64() == Some(0))
        .or_else(|| (jobs.len() == 1).then(|| &jobs[0]))?;
    job["job_id"]
        .as_str()
        .or_else(|| job["id"].as_str())
        .filter(|id| is_uuid(id))
        .map(str::to_owned)
}

/// The generation in one page of `show_generations` whose parameters
/// name `media` (the first frame uploaded for this submission alone):
/// the job a lost answer started. Nothing else identifies it; a model and
/// prompt match could be an older clip of the same scene. A generation
/// that only ends on `media` (its `end_image`) did not start from it, so
/// it is not the one.
pub fn find_generation(value: &Value, media: &str) -> Option<String> {
    fn names(value: &Value, media: &str) -> bool {
        match value {
            Value::String(text) => text == media,
            Value::Array(items) => items.iter().any(|item| names(item, media)),
            Value::Object(fields) if fields.get("role") == Some(&json!("end_image")) => false,
            Value::Object(fields) => fields.values().any(|field| names(field, media)),
            _ => false,
        }
    }
    value["items"]
        .as_array()
        .or_else(|| value["generations"].as_array())?
        .iter()
        .find(|item| names(&item["params"], media) || names(&item["medias"], media))
        .and_then(|item| {
            item["id"]
                .as_str()
                .or_else(|| item["job_id"].as_str())
                .filter(|id| is_uuid(id))
                .map(str::to_owned)
        })
}

/// The cursor of the next page of `show_generations`, if any.
fn next_cursor(value: &Value) -> Option<String> {
    value["next_cursor"]
        .as_str()
        .filter(|cursor| !cursor.is_empty() && cursor.len() <= 512)
        .map(str::to_owned)
}

/// The first HTTPS address in a finished job: under a key naming a URL,
/// an MP4 first.
fn result_url(job: &Value) -> Option<String> {
    fn collect<'a>(value: &'a Value, key: &str, found: &mut Vec<&'a str>) {
        match value {
            Value::String(text) if key.contains("url") && text.starts_with("https://") => {
                found.push(text)
            }
            Value::Array(items) => items.iter().for_each(|item| collect(item, key, found)),
            Value::Object(fields) => fields.iter().for_each(|(name, field)| {
                let key = name.to_ascii_lowercase();
                // An entry of a `results` list counts as a result address.
                let key = if key == "results" || key == "result" {
                    "url".to_owned()
                } else {
                    key
                };
                collect(field, &key, found)
            }),
            _ => {}
        }
    }
    let mut found = Vec::new();
    collect(job, "", &mut found);
    let mp4 = |url: &&&str| {
        url.split('?')
            .next()
            .is_some_and(|path| path.ends_with(".mp4"))
    };
    found
        .iter()
        .find(mp4)
        .or_else(|| found.first())
        .map(|url| (*url).to_owned())
}

/// Reads a `jobs_wait` answer for one job.
pub fn parse_status(value: &Value) -> Result<ClipStatus, ProviderFailure> {
    let job = value["jobs"]
        .as_array()
        .and_then(|jobs| jobs.first())
        .ok_or_else(|| unexpected("the status answer has no job"))?;
    let reason = job["error"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(300).collect::<String>());
    let status = job["status"]
        .as_str()
        .ok_or_else(|| unexpected("the status answer has no status"))?;
    let ended = |kind, what: &str| {
        ProviderFailure::new(
            kind,
            match &reason {
                Some(reason) => format!("{what}: {reason}"),
                None => what.to_owned(),
            },
        )
    };
    let failed = |kind, what: &str| ClipStatus::Failed(ended(kind, what));
    // No clip, and no word that none was made: its credits may be spent.
    let lost = |what: &str| ClipStatus::Lost(ended(ProviderFailureKind::Unexpected, what));
    Ok(match status.to_ascii_lowercase().as_str() {
        "completed" | "succeeded" | "done" => match result_url(job) {
            Some(video) => ClipStatus::Done { video },
            None => return Err(unexpected("the finished clip has no address")),
        },
        "nsfw" | "ip_detected" | "moderated" => failed(
            ProviderFailureKind::Declined,
            "Higgsfield's content rules declined the clip",
        ),
        "failed" | "error" | "cancelled" | "canceled" => failed(
            ProviderFailureKind::ProviderDown,
            "Higgsfield could not make the clip",
        ),
        "lookup_failed" | "not_found" => lost("Higgsfield does not know this clip"),
        "queued" | "pending" | "waiting" | "submitted" | "created" => ClipStatus::Queued,
        "in_progress" | "running" | "processing" | "generating" | "started" => ClipStatus::Running,
        // A status this adapter does not know: over when the server says
        // every job is, else still going.
        _ => match value["all_terminal"].as_bool() {
            Some(true) => lost(&format!(
                "the clip ended as \"{}\"",
                status.chars().take(40).collect::<String>()
            )),
            Some(false) => ClipStatus::Running,
            None => {
                return Err(unexpected(format!(
                    "unknown clip status \"{}\"",
                    status.chars().take(40).collect::<String>()
                )));
            }
        },
    })
}

/// Whether `bytes` look like an MP4 file (an `ftyp` box first).
fn is_mp4(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp")
}

fn random_text<const N: usize>() -> SecretText {
    let mut bytes = [0; N];
    // Without the operating system's random generator no sign-in can be
    // safe, and Windows always has one.
    getrandom::fill(&mut bytes).expect("the operating system's random generator works");
    SecretText::new(URL_SAFE_NO_PAD.encode(bytes))
}

fn sign_in_unexpected(detail: impl Into<String>) -> SignInFailure {
    SignInFailure::new(SignInFailureKind::Unexpected, detail)
}

/// What a token endpoint's refusal means.
fn token_failure(response: &HttpResponse) -> SignInFailure {
    let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
    let code = body["error"].as_str().unwrap_or_default();
    let detail = body["error_description"]
        .as_str()
        .unwrap_or(code)
        .chars()
        .take(300)
        .collect::<String>();
    let kind = match (response.status, code) {
        (_, "invalid_grant") => SignInFailureKind::Refused,
        (_, "invalid_client" | "unauthorized_client") => SignInFailureKind::ClientRejected,
        (429, _) => SignInFailureKind::LimitReached,
        (500..=599, _) => SignInFailureKind::NetworkDown,
        (401, _) => SignInFailureKind::ClientRejected,
        _ => SignInFailureKind::Unexpected,
    };
    SignInFailure::new(kind, format!("HTTP {}: {detail}", response.status))
}

/// The tokens a token endpoint granted.
pub fn parse_grant(body: &str) -> Result<TokenGrant, SignInFailure> {
    let body: Value = serde_json::from_str(body)
        .map_err(|error| sign_in_unexpected(format!("unreadable token answer: {error}")))?;
    let token = |name: &str| {
        body[name]
            .as_str()
            .filter(|token| !token.is_empty() && token.chars().all(|c| c.is_ascii_graphic()))
            .map(SecretText::new)
    };
    let access_token = token("access_token")
        .ok_or_else(|| sign_in_unexpected("the token answer has no access token"))?;
    if !body["token_type"]
        .as_str()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"))
    {
        return Err(sign_in_unexpected("the token answer is not a bearer token"));
    }
    let expires_in = body["expires_in"]
        .as_u64()
        .filter(|secs| *secs > 0)
        .ok_or_else(|| sign_in_unexpected("the token answer has no lifetime"))?;
    Ok(TokenGrant {
        access_token,
        refresh_token: token("refresh_token"),
        expires_in: Duration::from_secs(expires_in),
        scopes: body["scope"]
            .as_str()
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
    })
}

/// The Higgsfield plan over its MCP server: sign-in and clips.
pub struct HiggsfieldPlan<T = UreqTransport> {
    mcp: McpClient<T>,
}

impl HiggsfieldPlan {
    /// Uploads carry the image and downloads the clip: megabytes.
    pub const TIMEOUT: Duration = Duration::from_secs(300);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for HiggsfieldPlan {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> HiggsfieldPlan<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            mcp: McpClient::new(MCP_URL, transport),
        }
    }

    pub fn transport(&self) -> &T {
        self.mcp.transport()
    }

    fn tool(&self, key: &ApiKey, name: &str, arguments: Value) -> Result<Value, ProviderFailure> {
        self.mcp
            .call_tool(key.expose(), name, arguments)
            .map_err(clip_failure)
    }

    fn token_request(&self, fields: &[(&str, &str)]) -> Result<TokenGrant, SignInFailure> {
        let response = self
            .transport()
            .send(&HttpRequest::post_form(format!("{AUTH_URL}/token"), fields))
            .map_err(|error| SignInFailure::new(SignInFailureKind::Unreachable, error.0))?;
        if (200..=299).contains(&response.status) {
            parse_grant(&response.body)
        } else {
            Err(token_failure(&response))
        }
    }

    /// Refuses a clip the plan's credits cannot pay for.
    fn check_credits(&self, key: &ApiKey, params: &Value) -> Result<(), ProviderFailure> {
        let estimate = self.tool(key, "estimate_video_cost", json!({ "params": params }))?;
        let needed =
            credits_in(&estimate).ok_or_else(|| unexpected("the estimate has no credits"))?;
        let balance = self.tool(key, "balance", json!({}))?;
        let left = credits_in(&balance).ok_or_else(|| unexpected("the balance has no credits"))?;
        if left < needed {
            return Err(ProviderFailure::new(
                ProviderFailureKind::LimitReached,
                format!("not enough credits: the clip needs {needed} and the plan has {left}"),
            ));
        }
        Ok(())
    }
}

impl<T: Transport> PlanSignIn for HiggsfieldPlan<T> {
    fn provider(&self) -> Provider {
        Provider::HiggsfieldPlan
    }

    fn register(&self, redirect_uri: &str) -> Result<String, SignInFailure> {
        let body = json!({
            "client_name": "Bardo",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
            "scope": SCOPES,
        });
        let response = self
            .transport()
            .send(&HttpRequest::post_json(
                format!("{AUTH_URL}/register"),
                body.to_string(),
            ))
            .map_err(|error| SignInFailure::new(SignInFailureKind::Unreachable, error.0))?;
        if !(200..=299).contains(&response.status) {
            return Err(token_failure(&response));
        }
        let answer: Value = serde_json::from_str(&response.body)
            .map_err(|error| sign_in_unexpected(format!("unreadable registration: {error}")))?;
        answer["client_id"]
            .as_str()
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 256
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-._~:".contains(c))
            })
            .map(str::to_owned)
            .ok_or_else(|| sign_in_unexpected("the registration has no client id"))
    }

    fn consent_request(&self, client_id: &str, redirect_uri: &str) -> ConsentRequest {
        // 48 random bytes make a 64-character verifier (RFC 7636).
        let verifier = random_text::<48>();
        let state = random_text::<32>();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.expose().as_bytes()));
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", SCOPES)
            .append_pair("state", state.expose())
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", MCP_URL)
            .finish();
        ConsentRequest {
            url: format!("{AUTH_URL}/authorize?{query}"),
            state,
            verifier,
        }
    }

    fn exchange(
        &self,
        client_id: &str,
        code: &SecretText,
        verifier: &SecretText,
        redirect_uri: &str,
    ) -> Result<TokenGrant, SignInFailure> {
        self.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code.expose()),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("code_verifier", verifier.expose()),
            ("resource", MCP_URL),
        ])
    }

    fn refresh(&self, client_id: &str, refresh_token: &str) -> Result<TokenGrant, SignInFailure> {
        self.token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("resource", MCP_URL),
        ])
    }

    fn account(&self, access_token: &str) -> Result<PlanAccount, SignInFailure> {
        let balance = self
            .mcp
            .call_tool(access_token, "balance", json!({}))
            .map_err(sign_in_failure)?;
        parse_account(&balance).ok_or_else(|| sign_in_unexpected("the balance has no credits"))
    }
}

impl<T: Transport> ClipGenerator for HiggsfieldPlan<T> {
    fn provider(&self) -> Provider {
        Provider::HiggsfieldPlan
    }

    fn models(&self) -> Vec<ClipModel> {
        models()
    }

    /// Uploads the image to the account's media and gives its id.
    fn stage_image(&self, key: &ApiKey, image: &ClipImage) -> Result<StagedImage, ProviderFailure> {
        let mime = image.format.mime();
        let upload = self.tool(
            key,
            "media_upload",
            json!({ "filename": format!("scene.{}", image.format.extension()), "content_type": mime }),
        )?;
        let slot = &upload["uploads"][0];
        let url = slot["upload_url"]
            .as_str()
            .filter(|url| url.starts_with("https://"))
            .ok_or_else(|| unexpected("the upload answer has no HTTPS address"))?;
        let media = slot["media_id"]
            .as_str()
            .filter(|id| is_uuid(id))
            .ok_or_else(|| unexpected("the upload answer has no media id"))?
            .to_owned();
        // The address is signed for these two headers.
        let put = HttpRequest::put(url, image.bytes.clone())
            .header("content-type", mime)
            .header("if-none-match", "*");
        let response = self
            .transport()
            .send(&put)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        if !(200..=299).contains(&response.status) {
            return Err(ProviderFailure::new(
                ProviderFailureKind::ProviderDown,
                format!("the image upload failed (HTTP {})", response.status),
            ));
        }
        self.tool(
            key,
            "media_confirm",
            json!({ "type": "image", "media_id": media }),
        )?;
        Ok(StagedImage(media))
    }

    /// Higgsfield takes no idempotency key: the job writes down that it
    /// sent a submission, and looks for it after a lost answer.
    fn dedupes_submissions(&self) -> bool {
        false
    }

    /// The library lists every generation, the newest first.
    fn lists_submissions(&self) -> bool {
        true
    }

    fn check_submit(&self, key: &ApiKey, request: &ClipRequest) -> Result<(), ProviderFailure> {
        let params = submit_params(request)?;
        self.check_credits(key, &params)
    }

    /// The generation made from this submission's first frame, in the
    /// newest pages of the library. Each submission uploads its frame
    /// again, so the frame's id names one submission; a generation that
    /// only ends on it is another one.
    fn find_submitted(
        &self,
        key: &ApiKey,
        request: &ClipRequest,
    ) -> Result<Option<ClipHandle>, ProviderFailure> {
        if !is_uuid(&request.image.0) {
            return Ok(None);
        }
        let mut cursor: Option<String> = None;
        for _ in 0..PAGES {
            let mut arguments = json!({ "only_completed": false, "size": PAGE, "type": "video" });
            if let Some(cursor) = &cursor {
                arguments["cursor"] = json!(cursor);
            }
            let page = self.tool(key, "show_generations", arguments)?;
            if let Some(job) = find_generation(&page, &request.image.0) {
                return Ok(Some(ClipHandle(job)));
            }
            match next_cursor(&page) {
                Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
                _ => break,
            }
        }
        Ok(None)
    }

    /// Starts the clip. A failure of kind `AnswerLost` means the request
    /// may have reached Higgsfield (the job then looks for it); any other
    /// means it did not start.
    fn submit(
        &self,
        key: &ApiKey,
        request: &ClipRequest,
        _submission: &str,
    ) -> Result<ClipSubmission, ProviderFailure> {
        let params = submit_params(request)?;
        let started = self
            .mcp
            .try_call_tool(
                key.expose(),
                "generate_video_batch",
                json!({ "requests": [{ "index": 0, "params": params }] }),
            )
            .map_err(submit_failure)?;
        let handle = parse_submitted_job(&started).ok_or_else(|| {
            ProviderFailure::new(
                ProviderFailureKind::AnswerLost,
                "the submission's answer has no job id; the clip may have started",
            )
        })?;
        Ok(ClipSubmission {
            handle: ClipHandle(handle),
            quote: None,
        })
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<ClipStatus, ProviderFailure> {
        if !is_uuid(&handle.0) {
            return Err(unexpected(format!("{} is not a Higgsfield job", handle.0)));
        }
        let answer = self.tool(
            key,
            "jobs_wait",
            json!({ "jobs": [{ "index": 0, "job_id": handle.0 }], "timeout_seconds": 0 }),
        )?;
        parse_status(&answer)
    }

    /// The CDN gets no token.
    fn download(&self, _key: &ApiKey, video: &str) -> Result<GeneratedClip, ProviderFailure> {
        if !video.starts_with("https://") {
            return Err(unexpected("the clip's address is not HTTPS"));
        }
        let response = self
            .transport()
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
        let mut ids: Vec<_> = CATALOG.iter().map(|spec| spec.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        assert!(CATALOG.iter().all(|spec| (spec.fixed)().is_object()));
    }

    #[test]
    fn ids_are_uuids() {
        assert!(is_uuid("2428803a-33a6-42e4-9627-6133b90b02ef"));
        assert!(!is_uuid("2428803a-33a6-42e4-9627-6133b90b02e"));
        assert!(!is_uuid("2428803a_33a6-42e4-9627-6133b90b02ef"));
        assert!(!is_uuid("../../2428803a-33a6-42e4-9627-6133b90"));
    }

    #[test]
    fn a_result_address_prefers_an_mp4_under_a_url_key() {
        let job = json!({
            "status": "completed",
            "thumbnail_url": "https://cdn.example/t.webp",
            "results": [{ "url": "https://cdn.example/v.mp4?x=1" }],
            "prompt": "https://not-an-address-field.example",
        });
        assert_eq!(
            result_url(&job).as_deref(),
            Some("https://cdn.example/v.mp4?x=1")
        );
        assert_eq!(result_url(&json!({ "status": "completed" })), None);
    }

    #[test]
    fn tool_failures_name_credits_and_moderation() {
        assert_eq!(
            tool_failure_kind("Insufficient credits for this generation"),
            ProviderFailureKind::LimitReached
        );
        assert_eq!(
            tool_failure_kind("Flagged as NSFW"),
            ProviderFailureKind::Declined
        );
        assert_eq!(
            tool_failure_kind("Flagged by moderation; your credits were not charged"),
            ProviderFailureKind::Declined,
            "about the content, not the balance"
        );
        assert_eq!(
            tool_failure_kind("unknown model \"x\""),
            ProviderFailureKind::NotAllowed
        );
        assert_eq!(tool_failure_kind("oops"), ProviderFailureKind::Unexpected);
    }
}
