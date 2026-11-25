//! Text generation with Anthropic's Claude API (Messages API).
//!
//! One request per generation, read as Claude streams it. A scene plan of
//! a long video takes minutes to write, so no clock bounds the whole
//! answer: the limit is on silence, and a stream that stops early still
//! says what was generated. JSON output uses structured outputs
//! (`output_config.format`), so the answer always matches the schema, and
//! its fields come in the schema's order: the schema is sent as written
//! (`serde_json` keeps the order of keys, see the workspace manifest), so a
//! schema can make Claude write one field before another. If Claude's
//! safety rules decline a request, the API re-runs it on Anthropic's
//! recommended fallback model in the same call
//! (`fallbacks: "default"`).

use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

use bardo_domain::{
    ApiKey, GeneratedText, Provider, ProviderFailure, ProviderFailureKind, TextEffort, TextFormat,
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
/// For the requests a whole video is built on (its script and its scene
/// plan): more thought before answering.
pub const CAREFUL_EFFORT: &str = "high";
/// Room for that thought on top of the answer.
pub const CAREFUL_MAX_TOKENS: u32 = 32_000;
/// The most a model writes in one answer, thought included. Every model
/// Bardo's requests may be answered by (the fallback's too) has this limit.
pub const MAX_OUTPUT_TOKENS: u32 = 128_000;

/// The effort and the usual token limit a request of `effort` is sent
/// with.
pub fn effort_settings(effort: TextEffort) -> (&'static str, u32) {
    match effort {
        TextEffort::Everyday => (EFFORT, MAX_TOKENS),
        TextEffort::Careful => (CAREFUL_EFFORT, CAREFUL_MAX_TOKENS),
    }
}

/// The longest answer, in tokens, a request can say it needs: half of what
/// the model can write. The other half is for the thought before the
/// answer and for an answer that runs longer than it was sized. An answer
/// cut at the limit is paid for and lost, so a longer one is not worth
/// sending.
pub fn longest_answer() -> u64 {
    u64::from(MAX_OUTPUT_TOKENS) / 2
}

/// The token limit of a request: the usual one, or, for an answer of a
/// known size, four times that size (the thought before it, and an answer
/// that runs longer than it was sized), never less than the usual limit
/// nor more than the model can write. Room that is not used is not billed,
/// and an answer cut at the limit is billed whole and lost.
pub fn max_tokens(effort: TextEffort, answer_tokens: Option<u64>) -> u32 {
    let (_, usual) = effort_settings(effort);
    let Some(answer) = answer_tokens else {
        return usual;
    };
    u32::try_from(answer.saturating_mul(4))
        .unwrap_or(MAX_OUTPUT_TOKENS)
        .clamp(usual, MAX_OUTPUT_TOKENS)
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
    let (effort, _) = effort_settings(request.effort);
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
        "max_tokens": max_tokens(request.effort, request.answer_tokens),
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

/// A successful answer that came whole instead of streamed, in the shape
/// a streamed one is read into.
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
    let mut usage = TokenUsage::default();
    claude_stream::read_usage(&body["usage"], &mut usage);
    Ok(StreamedAnswer {
        model: Some(body["model"].as_str().unwrap_or_default().to_owned()),
        text,
        stop_reason: body["stop_reason"].as_str().map(str::to_owned),
        stop_details: body["stop_details"].clone(),
        usage,
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
        // Claude took the request (the stream opened) and then nothing
        // readable came. A path that holds the answer back looks the same
        // as one that lost it, and the answer may be written and billed all
        // the same, so this is not an outage to send again after.
        StreamEnd::Broken(detail) if !answer.started() => {
            return Err(unexpected(format!(
                "Claude took the request and no answer arrived; it may be billed ({detail})"
            )));
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
        Some("model_context_window_exceeded") => {
            return Err(unexpected(
                "the answer was cut at the end of the model's context window",
            ));
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
        "rate_limit_error" | "billing_error" => ProviderFailureKind::LimitReached,
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

/// An answer read whole: an error, which is short, or an answer that was
/// not streamed.
fn buffered(status: u16, headers: Vec<(String, String)>, body: impl Read) -> HttpResponse {
    let mut bytes = Vec::new();
    // What could not be read still tells the status.
    let _ = body
        .take(UreqTransport::MAX_BODY_BYTES)
        .read_to_end(&mut bytes);
    HttpResponse {
        status,
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

/// Whether a successful answer is one JSON message instead of a stream of
/// events, by how it begins. The headers are not asked: a path that
/// rewrites them does not change what the body is.
fn comes_whole(body: &mut impl BufRead) -> bool {
    body.fill_buf().is_ok_and(|start| {
        start
            .iter()
            .find(|byte| !byte.is_ascii_whitespace())
            .is_some_and(|byte| *byte == b'{')
    })
}

/// Generates text with Claude over HTTPS.
pub struct ClaudeTextGenerator<T = Watched<UreqTransport>> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl ClaudeTextGenerator {
    /// How long connecting may take.
    pub const CONNECT: Duration = Duration::from_secs(30);
    /// How long a request may take to be sent, and Claude to start
    /// answering it. Claude starts a stream at once, but a path that holds
    /// the answer back until it is whole (a proxy that inspects it) makes
    /// the wait as long as the answer.
    pub const FIRST_ANSWER: Duration = Duration::from_secs(600);
    /// How long a started answer may say nothing. Claude sends keep-alives
    /// while it thinks, but says nowhere how often, and the two mistakes
    /// do not cost the same: waiting this long on a dead connection costs
    /// minutes, and giving up on a live one costs the whole answer, which
    /// is billed.
    pub const SILENCE: Duration = Duration::from_secs(600);
    /// A bound on the whole answer that none reaches: the longest answer
    /// the model can write takes a fraction of it. It only ends a
    /// connection nothing else ended.
    pub const WHOLE_ANSWER: Duration = Duration::from_secs(2 * 60 * 60);

    pub fn new() -> Self {
        Self::with_transport(Watched::new(
            UreqTransport::for_streams(Self::CONNECT, Self::FIRST_ANSWER, Self::WHOLE_ANSWER),
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
            let StreamedResponse {
                status,
                headers,
                body,
            } = response;
            let mut body = BufReader::new(body);
            let (answered, wait) = if !(200..=299).contains(&status) {
                let response = buffered(status, headers, body);
                let wait =
                    Backoff::is_retryable(&response).then(|| self.backoff.delay(retry, &response));
                (Answered::Refused(response), wait)
            } else if comes_whole(&mut body) {
                // A server that does not stream hands the answer over whole.
                return parse_whole(&buffered(status, headers, body)).map(Answered::Answer);
            } else {
                let answer = claude_stream::read(body);
                let wait = comes_back_shortly(&answer)
                    .then(|| self.backoff.delay(retry, &HttpResponse::new(529, "")));
                (Answered::Answer(answer), wait)
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
