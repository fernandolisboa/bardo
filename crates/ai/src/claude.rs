//! Text generation with Anthropic's Claude API (Messages API).
//!
//! One request per generation, read as Claude streams it. A scene plan of
//! a long video takes minutes to write, so no clock bounds the whole
//! answer: the limit is on silence, and a stream that stops early still
//! says what was generated. JSON output uses structured outputs
//! (`output_config.format`), so the answer always matches the schema. If
//! Claude's safety rules decline a request, the API re-runs it on
//! Anthropic's recommended fallback model in the same call
//! (`fallbacks: "default"`).

use std::io::Read as _;
use std::time::Duration;

use bardo_domain::{
    ApiKey, GeneratedText, Provider, ProviderFailure, ProviderFailureKind, TextFormat,
    TextGenerator, TextRequest, TokenUsage,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::claude_stream::{self, StreamEnd, StreamedAnswer};
use crate::http::{HttpRequest, HttpResponse, StreamedResponse, Transport, UreqTransport};
use crate::idle::Watched;
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

pub const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
pub const MODEL: &str = "claude-opus-5-5";
/// Room for thinking plus the answer; generous so an answer is never cut.
pub const MAX_TOKENS: u32 = 16_000;
/// The model's default made explicit: enough thought for creative work
/// without paying for deep reasoning.
pub const EFFORT: &str = "medium";
/// Gates `fallbacks: "default"`.
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// The Messages API request for `request`. `with_fallback` adds the
/// server-side fallback on a declined request.
pub fn message_request(
    key: &ApiKey,
    request: &TextRequest,
    with_fallback: bool,
) -> Result<HttpRequest, ProviderFailure> {
    let mut output_config = json!({ "effort": EFFORT });
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
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": request.instructions,
        "messages": [{ "role": "user", "content": content }],
        "output_config": output_config,
    });
    if with_fallback {
        body["fallbacks"] = json!("default");
    }
    let http = HttpRequest::post_json(MESSAGES_URL, body.to_string())
        .header("x-api-key", key.expose())
        .header("anthropic-version", "2023-06-01");
    Ok(if with_fallback {
        http.header("anthropic-beta", FALLBACK_BETA)
    } else {
        http
    })
}

/// Reads a successful answer that came whole instead of streamed: the text
/// blocks, the model and the usage. A refusal or an answer cut by the token
/// limit is a failure.
pub fn parse_message(response: &HttpResponse) -> Result<GeneratedText, ProviderFailure> {
    read_answer(parse_whole(response)?)
}

/// A whole answer in the shape a streamed one is read into.
fn parse_whole(response: &HttpResponse) -> Result<StreamedAnswer, ProviderFailure> {
    let body: Value = serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable response: {error}")))?;
    let text: String = body["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    let usage = &body["usage"];
    Ok(StreamedAnswer {
        model: Some(body["model"].as_str().unwrap_or_default().to_owned()),
        text,
        stop_reason: body["stop_reason"].as_str().map(str::to_owned),
        stop_details: body["stop_details"].clone(),
        usage: TokenUsage {
            input_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
        },
        end: StreamEnd::Complete,
    })
}

/// What an answer means, streamed or not: its text when it is whole; a
/// failure when Claude declined, the token limit cut it, it has no text, or
/// the stream stopped before its end.
pub fn read_answer(answer: StreamedAnswer) -> Result<GeneratedText, ProviderFailure> {
    match &answer.end {
        StreamEnd::Complete => {}
        // Nothing was generated: the same as the error's HTTP status.
        StreamEnd::Failed { kind, message } if !answer.started() => {
            return Err(ProviderFailure::new(
                stream_error_kind(kind),
                message.clone(),
            ));
        }
        StreamEnd::Failed { kind, message } => {
            return Err(stopped_midway(format!("{kind}: {message}")));
        }
        StreamEnd::Broken(detail) if !answer.started() => {
            return Err(ProviderFailure::new(
                ProviderFailureKind::Unreachable,
                detail.clone(),
            ));
        }
        StreamEnd::Broken(detail) => return Err(stopped_midway(detail.clone())),
    }
    match answer.stop_reason.as_deref() {
        Some("refusal") => {
            let details = &answer.stop_details;
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
    if answer.text.trim().is_empty() {
        return Err(unexpected("the answer has no text"));
    }
    Ok(GeneratedText {
        text: answer.text,
        model: answer
            .model
            .filter(|model| !model.is_empty())
            .unwrap_or_else(|| MODEL.to_owned()),
        usage: answer.usage,
    })
}

/// A stream that stopped after Claude began to answer. What was generated
/// is billed, so the request is not one to send again unasked.
fn stopped_midway(detail: String) -> ProviderFailure {
    unexpected(format!(
        "the answer stopped midway and what was generated may be billed ({detail})"
    ))
}

/// An `error` event that opens a stream, by its type.
fn stream_error_kind(kind: &str) -> ProviderFailureKind {
    match kind {
        "overloaded_error" | "api_error" | "timeout_error" => ProviderFailureKind::ProviderDown,
        "rate_limit_error" => ProviderFailureKind::LimitReached,
        "authentication_error" => ProviderFailureKind::Rejected,
        "permission_error" => ProviderFailureKind::NotAllowed,
        _ => ProviderFailureKind::Unexpected,
    }
}

/// Whether an `error` event that opens a stream asks to come back shortly,
/// as a 429 or 529 status does.
fn comes_back_shortly(answer: &StreamedAnswer) -> bool {
    !answer.started()
        && matches!(
            &answer.end,
            StreamEnd::Failed { kind, .. }
                if matches!(kind.as_str(), "overloaded_error" | "rate_limit_error")
        )
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

/// How a request was answered.
enum Answered {
    /// Claude took the request; the answer as far as it got.
    Answer(StreamedAnswer),
    /// An error status, with its body.
    Refused(HttpResponse),
}

/// An error answer read whole: it is short, and only its start matters.
fn buffered(response: StreamedResponse) -> HttpResponse {
    let mut bytes = Vec::new();
    // What could not be read still tells the status.
    let _ = response
        .body
        .take(UreqTransport::MAX_BODY_BYTES)
        .read_to_end(&mut bytes);
    HttpResponse {
        status: response.status,
        headers: response.headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

fn is_event_stream(response: &StreamedResponse) -> bool {
    response
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .is_some_and(|(_, value)| {
            value
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("text/event-stream")
        })
}

/// Generates text with Claude over HTTPS.
pub struct ClaudeTextGenerator<T = Watched<UreqTransport>> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl ClaudeTextGenerator {
    /// How long Claude may take to start answering: to connect and to
    /// send the first byte back.
    pub const FIRST_ANSWER: Duration = Duration::from_secs(180);
    /// How long a started answer may say nothing. Claude sends keep-alives
    /// while it thinks, so silence this long is a dead connection.
    pub const SILENCE: Duration = Duration::from_secs(180);
    /// A bound on the whole answer that none reaches: the longest answer
    /// the model can write takes about half of it. It only ends a
    /// connection nothing else ended.
    pub const WHOLE_ANSWER: Duration = Duration::from_secs(60 * 60);

    pub fn new() -> Self {
        Self::with_transport(Watched::new(
            UreqTransport::for_streams(Self::FIRST_ANSWER, Self::WHOLE_ANSWER),
            Self::SILENCE,
        ))
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

    /// Sends the request and reads its answer as it arrives. An answer
    /// that asks to come back shortly, by its status or by an error that
    /// opens the stream, is sent again after a wait; nothing was generated
    /// for it.
    fn send(
        &self,
        key: &ApiKey,
        request: &TextRequest,
        with_fallback: bool,
    ) -> Result<Answered, ProviderFailure> {
        let http = message_request(key, request, with_fallback)?;
        let mut retry = 0;
        loop {
            let response = self
                .transport
                .send_streamed(&http)
                .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
            let (answered, wait) = if !(200..=299).contains(&response.status) {
                let response = buffered(response);
                let wait =
                    Backoff::is_retryable(&response).then(|| self.backoff.delay(retry, &response));
                (Answered::Refused(response), wait)
            } else if is_event_stream(&response) {
                let answer = claude_stream::read(response.body);
                let wait = comes_back_shortly(&answer)
                    .then(|| self.backoff.delay(retry, &HttpResponse::new(529, "")));
                (Answered::Answer(answer), wait)
            } else {
                // A proxy that buffers may hand the answer over whole.
                return parse_whole(&buffered(response)).map(Answered::Answer);
            };
            match wait {
                Some(wait) if retry < self.backoff.retries => {
                    (self.sleep)(wait);
                    retry += 1;
                }
                _ => return Ok(answered),
            }
        }
    }
}

impl<T: Transport> TextGenerator for ClaudeTextGenerator<T> {
    fn generate(
        &self,
        key: &ApiKey,
        request: &TextRequest,
    ) -> Result<GeneratedText, ProviderFailure> {
        let mut answered = self.send(key, request, true)?;
        // An account without access to the fallback beta still gets its
        // text, only without the fallback.
        if let Answered::Refused(response) = &answered
            && response.status == 400
            && error_message(response).contains("anthropic-beta")
        {
            answered = self.send(key, request, false)?;
        }
        let response = match answered {
            Answered::Answer(answer) => return read_answer(answer),
            Answered::Refused(response) => response,
        };
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
