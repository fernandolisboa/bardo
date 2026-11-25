//! Text generation with Anthropic's Claude API (Messages API).
//!
//! One request per generation, read as Claude streams it. A scene plan of
//! a long video takes minutes to write, so no clock bounds the whole
//! answer: the limit is on silence, and a stream that stops early still
//! says how far it got. Claude bills what it writes, read or not, so a
//! request that left is never sent again by itself (ADR-0030): one whose
//! answer never arrives, stops midway or cannot be read is a lost answer,
//! not an outage. A request whose job was cancelled, or whose app is
//! closing, is given up as soon as the adapter is told: the stream is
//! dropped, which closes its connection, and what Claude wrote until then
//! is a lost answer too. JSON output uses structured outputs
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
    ApiKey, GeneratedText, Provider, ProviderFailure, ProviderFailureKind, Stop, TextEffort,
    TextFormat, TextGenerator, TextRequest, TokenUsage,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::claude_stream::{self, StreamEnd, StreamedAnswer};
use crate::http::{HttpRequest, HttpResponse, StreamedResponse, Transport, UreqTransport};
use crate::idle::{STOP_POLL, Watched};
use crate::key_check::failure;
use crate::retry::{
    Backoff, PaidAnswer, Sleeper, lost_upstream, thread_sleeper, unanswered, unreadable,
};

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
/// The most a model writes in one answer, thought included: the limit of
/// the model Bardo asks and of the fallback Anthropic documents for it.
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

/// Whether an error body is the Messages API's own: `{"type": "error",
/// "error": {"type": ...}}`.
fn own_words(body: &Value) -> bool {
    body["type"] == "error" && body["error"]["type"].is_string()
}

/// A successful answer that came whole instead of streamed, in the shape
/// a streamed one is read into. One that cannot be read is a lost answer:
/// Claude wrote it.
fn parse_whole(response: &HttpResponse) -> Result<StreamedAnswer, ProviderFailure> {
    let body: Value = serde_json::from_str(response.body.trim_start_matches('\u{feff}'))
        .map_err(|_| unreadable(response.status))?;
    // An error in the place of the answer, as an `error` event that opens
    // a stream.
    if own_words(&body) {
        let error = &body["error"];
        return Ok(StreamedAnswer::unstarted(StreamEnd::Failed {
            kind: error["type"].as_str().unwrap_or("error").to_owned(),
            message: error["message"].as_str().unwrap_or_default().to_owned(),
        }));
    }
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
        began: true,
        text,
        stop_reason: body["stop_reason"].as_str().map(str::to_owned),
        stop_details: body["stop_details"].clone(),
        usage,
        end: StreamEnd::Complete,
    })
}

/// What an answer means, streamed or not: its text when it is whole. An
/// answer cut by the token limit or one without text is a failure that
/// says what the answer used: Claude bills an answer as it wrote it,
/// usable or not. A refusal says it when Claude bills it. A stream that
/// stopped before the answer's end is a lost answer.
pub fn read_answer(answer: StreamedAnswer) -> Result<GeneratedText, ProviderFailure> {
    match &answer.end {
        StreamEnd::Complete => {}
        // Nothing was generated: the same as the error's HTTP status.
        StreamEnd::Failed { kind, message } if !answer.started() => {
            return Err(if kind == TIMEOUT_ERROR {
                timed_out(message)
            } else {
                ProviderFailure::new(stream_error_kind(kind), message.clone())
            });
        }
        StreamEnd::Failed { kind, message } => {
            return Err(stopped_midway(&format!("{kind}: {message}")));
        }
        // Claude took the request (the stream opened) and then nothing
        // readable came. A path that holds the answer back looks the same
        // as one that lost it, and the answer may be written and billed all
        // the same.
        StreamEnd::Broken(detail) if !answer.started() => {
            return Err(ProviderFailure::new(
                ProviderFailureKind::AnswerLost,
                format!(
                    "Claude took the request and no answer arrived, so it may have written and billed one ({detail})"
                ),
            ));
        }
        StreamEnd::Broken(detail) => return Err(stopped_midway(detail)),
    }
    let model = answer
        .model
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| MODEL.to_owned());
    let usage = answer.usage;
    let failed = |failure: ProviderFailure| failure.billed(model.clone(), usage.into());
    match answer.stop_reason.as_deref() {
        Some("refusal") => {
            let details = &answer.stop_details;
            let reason = details["explanation"]
                .as_str()
                .or(details["category"].as_str())
                .unwrap_or("no reason given");
            let declined = ProviderFailure::new(
                ProviderFailureKind::Declined,
                format!("Claude declined the request: {reason}"),
            );
            // A refusal before anything was written is billed only for
            // some reasons; its usage is reported either way. One that
            // comes midway bills the input and what was written.
            let wrote = usage.output_tokens > 0 || !answer.text.is_empty();
            let billed = wrote
                || details["category"]
                    .as_str()
                    .is_some_and(|category| BILLED_REFUSALS.contains(&category));
            return Err(if billed { failed(declined) } else { declined });
        }
        Some("max_tokens") => {
            return Err(failed(unexpected("the answer was cut at the token limit")));
        }
        Some("model_context_window_exceeded") => {
            return Err(failed(unexpected(
                "the answer was cut at the end of the model's context window",
            )));
        }
        // Nothing a message says: whatever answered, what Claude may
        // have written did not arrive.
        None if answer.text.trim().is_empty() => {
            return Err(ProviderFailure::new(
                ProviderFailureKind::AnswerLost,
                "Claude took the request and what came back is not an answer, so it may have written and billed one",
            ));
        }
        _ => {}
    }
    if answer.text.trim().is_empty() {
        return Err(failed(unexpected("the answer has no text")));
    }
    Ok(GeneratedText {
        text: answer.text,
        model,
        usage,
    })
}

/// A stream that stopped after Claude began to answer: a lost answer. It
/// carries no count of what it used, because a stream counts its output
/// only at its end (what it says before is the input and a placeholder),
/// so Costs records what such a call usually uses instead of a figure that
/// leaves the writing out.
fn stopped_midway(detail: &str) -> ProviderFailure {
    ProviderFailure::new(
        ProviderFailureKind::AnswerLost,
        format!("the answer stopped midway, and Claude bills what it wrote of it ({detail})"),
    )
}

/// The error of a request Claude took and ran out of time on, whatever
/// status it comes with.
const TIMEOUT_ERROR: &str = "timeout_error";

/// The reasons a refusal is billed for although nothing was written
/// (Anthropic's list as of September 2026).
const BILLED_REFUSALS: [&str; 3] = ["bio", "frontier_llm", "reasoning_extraction"];

/// Claude's own word that it ran out of time on the request: it worked on
/// it, so this is a lost answer and not an outage to send it again after.
fn timed_out(message: &str) -> ProviderFailure {
    ProviderFailure::new(
        ProviderFailureKind::AnswerLost,
        format!(
            "Claude ran out of time on the request, so it may have written and billed part of an answer ({message})"
        ),
    )
}

/// An `error` event that opens a stream, by its type. A timeout is not
/// among them: see `timed_out`.
fn stream_error_kind(kind: &str) -> ProviderFailureKind {
    match kind {
        "overloaded_error" | "api_error" => ProviderFailureKind::ProviderDown,
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

/// A request given up before it was sent: nothing is billed for it.
fn not_sent() -> ProviderFailure {
    ProviderFailure::new(
        ProviderFailureKind::Unreachable,
        "given up before the request was sent",
    )
}

/// An answer given up after its request left, as far as it had got: a
/// lost answer, like any stream that stops before its end. Claude bills
/// what it wrote until the connection closed.
fn given_up(answer: &StreamedAnswer) -> ProviderFailure {
    if answer.started() {
        stopped_midway("given up while Claude was writing it")
    } else {
        ProviderFailure::new(
            ProviderFailureKind::AnswerLost,
            "Claude took the request and its answer was given up before it began, so it may have written and billed part of one",
        )
    }
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

/// Whether an error status is Claude's own word that it ran out of time.
fn is_timeout(response: &HttpResponse) -> bool {
    serde_json::from_str::<Value>(&response.body)
        .is_ok_and(|body| own_words(&body) && body["error"]["type"] == TIMEOUT_ERROR)
}

/// The message of a request Claude calls invalid, by its status or by an
/// error in the place of the answer.
fn bad_request(answered: &Answered) -> Option<String> {
    match answered {
        Answered::Refused(response) if response.status == 400 => Some(error_message(response)),
        Answered::Answer(answer) if !answer.started() => match &answer.end {
            StreamEnd::Failed { kind, message } if kind == "invalid_request_error" => {
                Some(message.clone())
            }
            _ => None,
        },
        _ => None,
    }
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
/// events, by how it begins. White space and a byte order mark before it
/// are read past and dropped: neither form needs them. The headers are not
/// asked: a path that rewrites them does not change what the body is. An
/// answer that fails before it begins says why.
fn comes_whole(body: &mut impl BufRead) -> std::io::Result<bool> {
    /// More of nothing than this is not the start of an answer.
    const MOST_BLANK: usize = 64 * 1024;
    let mut dropped = 0;
    while dropped <= MOST_BLANK {
        let start = body.fill_buf()?;
        if start.is_empty() {
            break;
        }
        // The bytes of a byte order mark begin neither form.
        let blank = start
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace() || matches!(byte, 0xEF | 0xBB | 0xBF))
            .count();
        let begins = start.get(blank).copied();
        body.consume(blank);
        dropped += blank;
        if let Some(byte) = begins {
            return Ok(byte == b'{');
        }
    }
    Ok(false)
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
    /// the wait as long as the answer; past this the request is a lost
    /// answer, not one to send again.
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

    /// Sends the request and reads its answer as it arrives. It goes
    /// again, after a wait, only when Claude says nothing was started: a
    /// rate limit or an overload, by the status or by an error in the
    /// place of the answer, and never when the answer's `x-should-retry`
    /// header says not to. An error status that is not Claude speaking is
    /// a lost answer, as for every paid call; so is Claude's own word that
    /// it ran out of time, or a failure it asks not to be sent again. Once
    /// `stop` asks, nothing more is sent, and an answer on its way is
    /// given up where it is.
    fn send(
        &self,
        key: &ApiKey,
        request: &TextRequest,
        with_fallback: bool,
        stop: &Stop,
    ) -> Result<Answered, ProviderFailure> {
        let http = message_request(key, request, with_fallback)?;
        let mut retry = 0;
        loop {
            if stop.asked() {
                return Err(not_sent());
            }
            // Billed for what it writes, read or not: a request that left
            // and got no answer is a lost answer, never sent again by
            // itself.
            let StreamedResponse {
                status,
                headers,
                body,
            } = self
                .transport
                .send_streamed_until(&http, stop)
                .map_err(unanswered)?;
            let mut body = BufReader::new(body);
            // What the answer's headers ask of a client that sends again.
            let asked = HttpResponse {
                status,
                headers: headers.clone(),
                body: String::new(),
            };
            let may_resend = asked.header("x-should-retry") != Some("false");
            let (answered, wait) = if (200..=299).contains(&status) {
                let answer = match comes_whole(&mut body) {
                    // A server that does not stream hands the answer over
                    // whole.
                    Ok(true) => parse_whole(&buffered(status, headers, body))?,
                    Ok(false) => claude_stream::read(body),
                    Err(error) => StreamedAnswer::unstarted(StreamEnd::Broken(error.to_string())),
                };
                // An answer that arrived whole all the same is an answer:
                // it is paid for, and the caller decides what to do with
                // it.
                if stop.asked() && answer.end != StreamEnd::Complete {
                    return Err(given_up(&answer));
                }
                let wait = (comes_back_shortly(&answer) && may_resend).then(|| {
                    let mut busy = asked;
                    busy.status = 529;
                    self.backoff.delay(retry, &busy)
                });
                (Answered::Answer(answer), wait)
            } else {
                let response = buffered(status, headers, body);
                // Claude worked on the request until its time ran out.
                if status >= 500 && is_timeout(&response) {
                    return Err(timed_out(&error_message(&response)));
                }
                let wait = match PaidAnswer::of(status, response.body.as_bytes(), own_words) {
                    PaidAnswer::Lost => return Err(lost_upstream(status)),
                    // Claude failed and says not to send the request again:
                    // not an outage to send it again after.
                    _ if status >= 500 && !may_resend => {
                        return Err(ProviderFailure::new(
                            ProviderFailureKind::AnswerLost,
                            format!(
                                "Claude failed and asks not to be sent the request again, so it may have written and billed part of an answer (HTTP {status}: {})",
                                error_message(&response)
                            ),
                        ));
                    }
                    PaidAnswer::StartedNothing if may_resend => {
                        Some(self.backoff.delay(retry, &response))
                    }
                    PaidAnswer::StartedNothing | PaidAnswer::Told => None,
                };
                (Answered::Refused(response), wait)
            };
            match wait {
                Some(wait) if retry < self.backoff.retries => {
                    if !self.pause(wait, stop) {
                        return Err(not_sent());
                    }
                    retry += 1;
                }
                _ => return Ok(answered),
            }
        }
    }

    /// Waits before sending again, in short steps when somebody may stop
    /// the wait. False once `stop` asked.
    fn pause(&self, wait: Duration, stop: &Stop) -> bool {
        if !stop.may_ask() {
            (self.sleep)(wait);
            return true;
        }
        let mut left = wait;
        while !left.is_zero() {
            if stop.asked() {
                return false;
            }
            let step = left.min(STOP_POLL);
            (self.sleep)(step);
            left -= step;
        }
        !stop.asked()
    }
}

impl<T: Transport> TextGenerator for ClaudeTextGenerator<T> {
    fn generate_until(
        &self,
        key: &ApiKey,
        request: &TextRequest,
        stop: &Stop,
    ) -> Result<GeneratedText, ProviderFailure> {
        let mut answered = self.send(key, request, true, stop)?;
        // An account without access to the fallback beta still gets its
        // text, only without the fallback.
        if bad_request(&answered).is_some_and(|message| message.contains("anthropic-beta")) {
            answered = self.send(key, request, false, stop)?;
        }
        // An empty balance is reported as a bad request.
        if let Some(message) = bad_request(&answered)
            && message.contains("credit balance")
        {
            return Err(ProviderFailure::new(
                ProviderFailureKind::LimitReached,
                message,
            ));
        }
        let response = match answered {
            Answered::Answer(answer) => return read_answer(answer),
            Answered::Refused(response) => response,
        };
        Err(failure(Provider::Claude, &response))
    }
}
