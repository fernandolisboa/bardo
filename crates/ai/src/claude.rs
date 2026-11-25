//! Text generation with Anthropic's Claude API (Messages API).
//!
//! One request per generation, not streamed: the outputs Bardo asks for
//! (ideas, titles, short scripts) fit well within the timeout. JSON output
//! uses structured outputs (`output_config.format`), so the answer always
//! matches the schema, and its fields come in the schema's order: the
//! schema is sent as written (`serde_json` keeps the order of keys, see
//! the workspace manifest), so a schema can make Claude write one field
//! before another. If Claude's safety rules decline a request, the API
//! re-runs it on Anthropic's recommended fallback model in the same call
//! (`fallbacks: "default"`).

use std::time::Duration;

use bardo_domain::{
    ApiKey, GeneratedText, Provider, ProviderFailure, ProviderFailureKind, TextEffort, TextFormat,
    TextGenerator, TextRequest, TokenUsage,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

pub const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
pub const MODEL: &str = "claude-opus-5-5";
/// Room for thinking plus the answer; generous so an answer is never cut.
pub const MAX_TOKENS: u32 = 16_000;
/// The model's default made explicit: enough thought for creative work
/// without paying for deep reasoning.
pub const EFFORT: &str = "medium";
/// For the requests a whole video is built on (its script and its scene
/// plan): more thought before answering.
pub const CAREFUL_EFFORT: &str = "high";
/// Room for that thought on top of a long plan.
pub const CAREFUL_MAX_TOKENS: u32 = 32_000;
/// How long a careful request waits for its answer: thought and a long
/// plan take minutes when the answer is not streamed, and an answer given
/// up on is still billed. Past ten minutes the API expects a streamed
/// request, so the wait stops there.
pub const CAREFUL_TIMEOUT: Duration = Duration::from_secs(600);

/// The effort and the token limit a request of `effort` is sent with.
pub fn effort_settings(effort: TextEffort) -> (&'static str, u32) {
    match effort {
        TextEffort::Everyday => (EFFORT, MAX_TOKENS),
        TextEffort::Careful => (CAREFUL_EFFORT, CAREFUL_MAX_TOKENS),
    }
}
/// Gates `fallbacks: "default"`.
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// The Messages API request for `request`. `with_fallback` adds the
/// server-side fallback on a declined request.
pub fn message_request(
    key: &ApiKey,
    request: &TextRequest,
    with_fallback: bool,
) -> Result<HttpRequest, ProviderFailure> {
    let (effort, max_tokens) = effort_settings(request.effort);
    let mut output_config = json!({ "effort": effort });
    if let TextFormat::Json { schema } = &request.format {
        let schema: Value = serde_json::from_str(schema).map_err(|error| {
            ProviderFailure::new(
                ProviderFailureKind::Unexpected,
                format!("invalid output schema: {error}"),
            )
        })?;
        output_config["format"] = json!({ "type": "json_schema", "schema": schema });
    }
    let content = if request.images.is_empty() {
        json!(request.prompt)
    } else {
        // Pictures first, then the task about them.
        let mut blocks: Vec<Value> = request
            .images
            .iter()
            .map(|image| {
                json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": image.format.mime(),
                        "data": base64::engine::general_purpose::STANDARD.encode(&image.bytes),
                    },
                })
            })
            .collect();
        blocks.push(json!({ "type": "text", "text": request.prompt }));
        Value::Array(blocks)
    };
    let mut body = json!({
        "model": MODEL,
        "max_tokens": max_tokens,
        "system": request.instructions,
        "messages": [{ "role": "user", "content": content }],
        "output_config": output_config,
    });
    if with_fallback {
        body["fallbacks"] = json!("default");
    }
    let mut http = HttpRequest::post_json(MESSAGES_URL, body.to_string())
        .header("x-api-key", key.expose())
        .header("anthropic-version", "2023-06-01");
    if request.effort == TextEffort::Careful {
        http = http.timeout(CAREFUL_TIMEOUT);
    }
    Ok(if with_fallback {
        http.header("anthropic-beta", FALLBACK_BETA)
    } else {
        http
    })
}

/// Reads a successful answer: the text blocks, the model and the usage. A
/// refusal or an answer cut by the token limit is a failure.
pub fn parse_message(response: &HttpResponse) -> Result<GeneratedText, ProviderFailure> {
    let body: Value = serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable response: {error}")))?;
    match body["stop_reason"].as_str() {
        Some("refusal") => {
            let details = &body["stop_details"];
            let reason = details["explanation"]
                .as_str()
                .or(details["category"].as_str())
                .unwrap_or("no reason given");
            return Err(ProviderFailure::new(
                ProviderFailureKind::Declined,
                format!("Claude declined the request: {reason}"),
            ));
        }
        Some("max_tokens") => {
            return Err(unexpected("the answer was cut at the token limit"));
        }
        _ => {}
    }
    let text: String = body["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    if text.trim().is_empty() {
        return Err(unexpected("the answer has no text"));
    }
    let usage = &body["usage"];
    Ok(GeneratedText {
        text,
        model: body["model"].as_str().unwrap_or(MODEL).to_owned(),
        usage: TokenUsage {
            input_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
        },
    })
}

fn unexpected(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

fn error_message(response: &HttpResponse) -> String {
    serde_json::from_str::<Value>(&response.body)
        .ok()
        .and_then(|body| body["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Generates text with Claude over HTTPS.
pub struct ClaudeTextGenerator<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl ClaudeTextGenerator {
    /// A non-streamed answer with thinking can take a minute or two. A
    /// careful request asks for its own, longer wait ([`CAREFUL_TIMEOUT`]).
    pub const TIMEOUT: Duration = Duration::from_secs(180);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for ClaudeTextGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> ClaudeTextGenerator<T> {
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

    fn send(
        &self,
        key: &ApiKey,
        request: &TextRequest,
        with_fallback: bool,
    ) -> Result<HttpResponse, ProviderFailure> {
        let http = message_request(key, request, with_fallback)?;
        self.backoff
            .send(&self.transport, &http, &self.sleep)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))
    }
}

impl<T: Transport> TextGenerator for ClaudeTextGenerator<T> {
    fn generate(
        &self,
        key: &ApiKey,
        request: &TextRequest,
    ) -> Result<GeneratedText, ProviderFailure> {
        let mut response = self.send(key, request, true)?;
        // An account without access to the fallback beta still gets its
        // text, only without the fallback.
        if response.status == 400 && error_message(&response).contains("anthropic-beta") {
            response = self.send(key, request, false)?;
        }
        if (200..=299).contains(&response.status) {
            return parse_message(&response);
        }
        // An empty balance is reported as a bad request.
        let message = error_message(&response);
        if response.status == 400 && message.contains("credit balance") {
            return Err(ProviderFailure::new(
                ProviderFailureKind::LimitReached,
                message,
            ));
        }
        Err(failure(Provider::Claude, &response))
    }
}
