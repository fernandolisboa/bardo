//! Claude text generation against recorded responses (see
//! `fixtures/claude/README.md`). No test calls Claude.

mod common;

use std::collections::VecDeque;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bardo_ai::ClaudeTextGenerator;
use bardo_ai::claude::{FALLBACK_BETA, MESSAGES_URL, MODEL};
use bardo_ai::http::{
    HttpRequest, HttpResponse, Method, StreamedResponse, Transport, TransportError,
};
use bardo_ai::idle::Watched;
use bardo_domain::{
    ApiKey, BilledUse, ImageFormat, Metered, Provider, ProviderFailureKind, Stop, TextEffort,
    TextFormat, TextGenerator, TextImage, TextRequest, TokenUsage,
};
use common::{Scripted, fixture, recording_sleeper};
use serde_json::json;

const KEY: &str = "sk-ant-api03-test-key-0001";
const SCHEMA: &str = r#"{"type":"object","properties":{"themes":{"type":"array","items":{"type":"string"}}},"required":["themes"],"additionalProperties":false}"#;

fn key() -> ApiKey {
    ApiKey::parse(Provider::Claude, KEY).unwrap()
}

fn request(format: TextFormat) -> TextRequest {
    TextRequest {
        effort: TextEffort::Everyday,
        answer_tokens: None,
        instructions: "You plan videos for a faceless channel.".into(),
        prompt: "Propose three themes about space history.".into(),
        images: Vec::new(),
        format,
    }
}

fn json_request() -> TextRequest {
    request(TextFormat::Json {
        schema: SCHEMA.into(),
    })
}

fn generator(answers: &[&str]) -> ClaudeTextGenerator<Scripted> {
    let answers = answers.iter().map(|name| fixture("claude", name)).collect();
    let (sleeper, _) = recording_sleeper();
    ClaudeTextGenerator::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

#[test]
fn sends_a_messages_request_with_the_schema_and_the_fallback() {
    let claude = generator(&["messages-themes"]);
    claude.generate(&key(), &json_request()).unwrap();

    let sent = claude.transport().sent();
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!(sent.method, Method::Post);
    assert_eq!(sent.url, MESSAGES_URL);
    assert_eq!(sent.header("x-api-key"), Some(KEY));
    assert_eq!(sent.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(sent.header("anthropic-beta"), Some(FALLBACK_BETA));
    assert_eq!(sent.header("content-type"), Some("application/json"));
    assert_eq!(sent.body["model"], MODEL);
    assert_eq!(
        sent.body["system"],
        "You plan videos for a faceless channel."
    );
    assert_eq!(
        sent.body["messages"],
        json!([{"role": "user", "content": "Propose three themes about space history."}])
    );
    assert_eq!(sent.body["fallbacks"], "default");
    assert_eq!(sent.body["output_config"]["effort"], "medium");
    assert_eq!(sent.body["output_config"]["format"]["type"], "json_schema");
    assert_eq!(
        sent.body["output_config"]["format"]["schema"]["required"],
        json!(["themes"])
    );
    assert!(sent.body["max_tokens"].as_u64().unwrap() >= 4_000);
    assert_eq!(sent.body["stream"], true, "read as it is written");
    assert!(sent.body.get("thinking").is_none(), "the model decides");
}

#[test]
fn a_careful_request_asks_for_more_thought_and_more_room() {
    let claude = generator(&["messages-themes"]);
    let careful = TextRequest {
        effort: TextEffort::Careful,
        answer_tokens: None,
        ..request(TextFormat::Prose)
    };
    claude.generate(&key(), &careful).unwrap();
    let body = &claude.transport().sent()[0].body;
    assert_eq!(body["output_config"]["effort"], "high");
    assert_eq!(body["max_tokens"], 32_000);
    assert!(body.get("thinking").is_none(), "the model decides");

    // Everyday requests are as they were.
    let claude = generator(&["messages-themes"]);
    claude
        .generate(&key(), &request(TextFormat::Prose))
        .unwrap();
    let body = &claude.transport().sent()[0].body;
    assert_eq!(body["output_config"]["effort"], "medium");
    assert_eq!(body["max_tokens"], 16_000);
}

/// The token limit sent for an answer of `answer_tokens` at `effort`.
fn limit_sent(effort: TextEffort, answer_tokens: u64) -> u64 {
    let claude = generator(&["messages-themes"]);
    let sized = TextRequest {
        effort,
        answer_tokens: Some(answer_tokens),
        ..request(TextFormat::Prose)
    };
    claude.generate(&key(), &sized).unwrap();
    claude.transport().sent()[0].body["max_tokens"]
        .as_u64()
        .unwrap()
}

#[test]
fn an_answer_of_a_known_size_gets_four_times_its_room() {
    // The plan of a three-minute video: its own size, the thought before
    // it, and prompts that run longer than they were counted.
    assert_eq!(limit_sent(TextEffort::Careful, 13_750), 55_000);
    assert_eq!(limit_sent(TextEffort::Everyday, 13_750), 55_000);
    // A short answer keeps the usual room, never less.
    assert_eq!(limit_sent(TextEffort::Careful, 2_750), 32_000);
    assert_eq!(limit_sent(TextEffort::Everyday, 2_750), 16_000);
    // The plan of a ten-minute video gets all the model can write, and
    // nothing gets more.
    assert_eq!(limit_sent(TextEffort::Careful, 40_000), 128_000);
    assert_eq!(limit_sent(TextEffort::Careful, u64::MAX), 128_000);
}

#[test]
fn the_longest_answer_leaves_as_much_room_again() {
    let longest = bardo_ai::claude::longest_answer();
    assert_eq!(longest, 64_000);
    assert_eq!(limit_sent(TextEffort::Careful, longest), 128_000);
}

#[test]
fn the_schema_is_sent_in_the_order_it_was_written() {
    // Claude answers in the schema's order, so a field written first in
    // the schema is thought through first.
    let schema = r#"{"type":"object","properties":{"story":{"type":"string"},"cast":{"type":"string"},"answer":{"type":"string"}},"required":["story","cast","answer"],"additionalProperties":false}"#;
    let claude = generator(&["messages-themes"]);
    claude
        .generate(
            &key(),
            &request(TextFormat::Json {
                schema: schema.into(),
            }),
        )
        .unwrap();
    let sent = String::from_utf8(claude.transport().sent()[0].raw_body.clone()).unwrap();
    let at = |name: &str| sent.find(&format!("\"{name}\":{{")).unwrap();
    assert!(at("story") < at("cast"));
    assert!(at("cast") < at("answer"), "not sorted by name");
}

#[test]
fn prose_requests_have_no_format() {
    let claude = generator(&["messages-themes"]);
    claude
        .generate(&key(), &request(TextFormat::Prose))
        .unwrap();
    let body = &claude.transport().sent()[0].body;
    assert!(body["output_config"].get("format").is_none());
}

#[test]
fn pictures_go_before_the_task_as_base64_blocks() {
    let claude = generator(&["messages-themes"]);
    let mut asked = request(TextFormat::Prose);
    asked.images = vec![TextImage {
        bytes: vec![0xFF, 0xD8, 0xFF, 0xE0],
        format: ImageFormat::Jpeg,
    }];
    claude.generate(&key(), &asked).unwrap();
    let body = &claude.transport().sent()[0].body;
    assert_eq!(
        body["messages"][0]["content"],
        json!([
            {
                "type": "image",
                "source": { "type": "base64", "media_type": "image/jpeg", "data": "/9j/4A==" },
            },
            { "type": "text", "text": "Propose three themes about space history." },
        ])
    );
    // Without pictures the task stays plain text.
    let claude = generator(&["messages-themes"]);
    claude
        .generate(&key(), &request(TextFormat::Prose))
        .unwrap();
    assert_eq!(
        claude.transport().sent()[0].body["messages"][0]["content"],
        "Propose three themes about space history."
    );
}

#[test]
fn reads_the_text_blocks_model_and_usage() {
    let generated = generator(&["messages-themes"])
        .generate(&key(), &json_request())
        .unwrap();
    let themes: serde_json::Value = serde_json::from_str(&generated.text).unwrap();
    assert_eq!(themes["themes"].as_array().unwrap().len(), 3);
    assert_eq!(
        themes["themes"][0]["title"],
        "The cosmonauts the Soviet Union erased"
    );
    assert_eq!(generated.model, "claude-opus-5-5");
    assert_eq!(
        generated.usage,
        TokenUsage {
            input_tokens: 412,
            output_tokens: 893
        }
    );
}

#[test]
fn an_answer_handed_over_whole_is_still_read() {
    // A server that does not stream answers with the whole message.
    let generated = generator(&["messages-themes-not-streamed"])
        .generate(&key(), &json_request())
        .unwrap();
    assert!(generated.text.contains("The forgotten first spacewalk"));
    assert_eq!(generated.model, "claude-opus-5-5");
    assert_eq!(generated.usage.output_tokens, 893);
}

#[test]
fn a_stream_is_read_as_one_whatever_its_headers_say() {
    let mut relabeled = fixture("claude", "messages-themes");
    relabeled
        .headers
        .retain(|(name, _)| !name.eq_ignore_ascii_case("content-type"));
    let relabeled = relabeled.with_header("content-type", "application/json");
    let (sleeper, _) = recording_sleeper();
    let generated = ClaudeTextGenerator::with_transport(Scripted::new(vec![relabeled]))
        .with_sleeper(sleeper)
        .generate(&key(), &json_request())
        .unwrap();
    assert!(generated.text.contains("The forgotten first spacewalk"));
    assert_eq!(generated.usage.output_tokens, 893);
}

#[test]
fn a_stream_that_opens_and_brings_no_answer_is_a_lost_answer_sent_once() {
    // Claude took the request, and a path that holds the answer back or
    // loses it brought only this. The answer may be written and billed all
    // the same: the queue must not take it for an outage and send it again.
    let silent = HttpResponse::new(200, "event: ping\ndata: {\"type\": \"ping\"}\n\n")
        .with_header("content-type", "text/event-stream; charset=utf-8");
    let (sleeper, _) = recording_sleeper();
    let claude = ClaudeTextGenerator::with_transport(Scripted::new(vec![
        silent,
        fixture("claude", "messages-themes"),
    ]))
    .with_sleeper(sleeper);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert!(
        failure.detail.contains("no answer arrived"),
        "{}",
        failure.detail
    );
    assert_eq!(failure.billed, None, "nothing says what it used");
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn a_connection_that_drops_after_claude_began_is_a_lost_answer_sent_once() {
    let claude = generator(&["messages-dropped-midway", "messages-themes"]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    // What was written is billed: not a call to make again unasked.
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert!(
        failure.detail.contains("stopped midway"),
        "{}",
        failure.detail
    );
    // The stream counted the input and nothing of what it wrote: Costs
    // records what such a call usually uses instead.
    assert_eq!(failure.billed, None);
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn an_overload_after_the_answer_began_is_a_lost_answer_sent_once() {
    let claude = generator(&["messages-overloaded-midway", "messages-themes"]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert!(
        failure.detail.contains("overloaded_error"),
        "{}",
        failure.detail
    );
    assert!(
        failure.detail.contains("stopped midway"),
        "{}",
        failure.detail
    );
    assert_eq!(failure.billed, None);
    assert_eq!(
        claude.transport().sent().len(),
        1,
        "the backoff is for answers that generated nothing"
    );
}

/// Hands over a successful answer in the pieces given, one per read; an
/// `Err` is a read that fails.
struct Pieces(Mutex<Vec<Vec<std::io::Result<Vec<u8>>>>>, Mutex<u32>);

impl Pieces {
    fn new(answers: Vec<Vec<std::io::Result<Vec<u8>>>>) -> Self {
        Self(Mutex::new(answers), Mutex::new(0))
    }

    fn sent(&self) -> u32 {
        *self.1.lock().unwrap()
    }
}

struct PieceReader(VecDeque<std::io::Result<Vec<u8>>>);

impl Read for PieceReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self.0.pop_front() {
            None => Ok(0),
            Some(Err(error)) => Err(error),
            Some(Ok(piece)) => {
                let count = piece.len().min(buffer.len());
                buffer[..count].copy_from_slice(&piece[..count]);
                if count < piece.len() {
                    self.0.push_front(Ok(piece[count..].to_vec()));
                }
                Ok(count)
            }
        }
    }
}

impl Transport for Pieces {
    fn send(&self, _: &HttpRequest) -> Result<HttpResponse, TransportError> {
        unreachable!("the adapter streams every request")
    }

    fn send_streamed(&self, _: &HttpRequest) -> Result<StreamedResponse, TransportError> {
        *self.1.lock().unwrap() += 1;
        let pieces = self.0.lock().unwrap().remove(0);
        Ok(StreamedResponse {
            status: 200,
            headers: Vec::new(),
            body: Box::new(PieceReader(pieces.into())),
        })
    }
}

/// An answer that begins and then goes on for as long as the test holds
/// it: `begun` arrives at once, and each read after it waits for the feed.
struct Held {
    begun: Vec<u8>,
    feed: Mutex<Option<Receiver<Vec<u8>>>>,
    sent: AtomicU32,
    /// Set when the answer's body is dropped, which is what closes a
    /// real connection.
    dropped: Arc<AtomicBool>,
}

impl Held {
    fn new(begun: &str) -> (Self, Sender<Vec<u8>>) {
        let (feed, held) = channel();
        let transport = Self {
            begun: begun.as_bytes().to_vec(),
            feed: Mutex::new(Some(held)),
            sent: AtomicU32::new(0),
            dropped: Arc::default(),
        };
        (transport, feed)
    }
}

struct HeldReader {
    begun: Option<Vec<u8>>,
    feed: Receiver<Vec<u8>>,
    dropped: Arc<AtomicBool>,
}

impl Read for HeldReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let piece = match self.begun.take() {
            Some(begun) => begun,
            None => match self.feed.recv() {
                Ok(piece) => piece,
                Err(_) => return Ok(0),
            },
        };
        buffer[..piece.len()].copy_from_slice(&piece);
        Ok(piece.len())
    }
}

impl Drop for HeldReader {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

impl Transport for Held {
    fn send(&self, _: &HttpRequest) -> Result<HttpResponse, TransportError> {
        unreachable!("the adapter streams every request")
    }

    fn send_streamed(&self, _: &HttpRequest) -> Result<StreamedResponse, TransportError> {
        self.sent.fetch_add(1, Ordering::SeqCst);
        let feed = self.feed.lock().unwrap().take().expect("one request");
        Ok(StreamedResponse {
            status: 200,
            headers: Vec::new(),
            body: Box::new(HeldReader {
                begun: Some(self.begun.clone()),
                feed,
                dropped: Arc::clone(&self.dropped),
            }),
        })
    }
}

/// A stop that asks once `flag` is set.
fn stop_on(flag: &Arc<AtomicBool>) -> Stop {
    let flag = Arc::clone(flag);
    Stop::when(move || flag.load(Ordering::SeqCst))
}

const BEGUN: &str = "event: message_start\n\
    data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\",\
    \"usage\":{\"input_tokens\":412,\"output_tokens\":1}}}\n\n\
    event: content_block_start\n\
    data: {\"type\":\"content_block_start\",\"index\":0,\
    \"content_block\":{\"type\":\"text\",\"text\":\"{\\\"themes\\\":[\"}}\n\n";

#[test]
fn an_answer_given_up_midway_is_a_lost_answer_and_its_stream_is_dropped() {
    // Claude began to answer and would go on for minutes: the test holds
    // the stream open the whole time.
    let (held, feed) = Held::new(BEGUN);
    let dropped = Arc::clone(&held.dropped);
    let claude = ClaudeTextGenerator::with_transport(Watched::new(held, Duration::from_secs(600)));
    let asked = Arc::<AtomicBool>::default();
    let cancel = Arc::clone(&asked);
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        cancel.store(true, Ordering::SeqCst);
    });

    let started = Instant::now();
    let failure = claude
        .generate_until(&key(), &json_request(), &stop_on(&asked))
        .unwrap_err();

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "within seconds, not at the answer's end"
    );
    // What Claude wrote until then is billed: a lost answer, not one to
    // send again unasked, and with no count of what it used.
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert!(failure.detail.contains("given up"), "{}", failure.detail);
    assert_eq!(failure.billed, None);
    canceller.join().unwrap();
    // The body is dropped at the next byte that arrives, which closes its
    // connection.
    assert!(!dropped.load(Ordering::SeqCst), "still waiting for a byte");
    feed.send(b"event: ping\ndata: {\"type\": \"ping\"}\n\n".to_vec())
        .unwrap();
    let waited = Instant::now();
    while !dropped.load(Ordering::SeqCst) {
        assert!(waited.elapsed() < Duration::from_secs(5), "never dropped");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn nothing_is_sent_once_the_stop_asked() {
    let claude = generator(&["messages-themes"]);
    let failure = claude
        .generate_until(&key(), &json_request(), &Stop::when(|| true))
        .unwrap_err();
    // Nothing left, so nothing is billed and the queue may send it later.
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
    assert_eq!(failure.billed, None);
    assert!(claude.transport().sent().is_empty());
}

#[test]
fn a_wait_before_sending_again_is_given_up() {
    // Claude is overloaded and the adapter waits to send again; the job is
    // cancelled during that wait.
    let asked = Arc::<AtomicBool>::default();
    let during_the_wait = Arc::clone(&asked);
    let claude = ClaudeTextGenerator::with_transport(Scripted::new(vec![
        fixture("claude", "messages-overloaded"),
        fixture("claude", "messages-themes"),
    ]))
    .with_sleeper(Arc::new(move |_| {
        during_the_wait.store(true, Ordering::SeqCst)
    }));
    let failure = claude
        .generate_until(&key(), &json_request(), &stop_on(&asked))
        .unwrap_err();
    // The overload started nothing, and nothing was sent after it.
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn a_wait_nobody_stops_is_one_wait() {
    // With a stop that never asks, the wait is as long as without one.
    let (sleeper, waits) = recording_sleeper();
    let claude = ClaudeTextGenerator::with_transport(Scripted::new(vec![
        fixture("claude", "messages-overloaded"),
        fixture("claude", "messages-themes"),
    ]))
    .with_sleeper(sleeper);
    claude
        .generate_until(&key(), &json_request(), &Stop::when(|| false))
        .unwrap();
    let waits = waits.lock().unwrap();
    assert_eq!(waits.iter().sum::<Duration>(), Duration::from_secs(1));
}

#[test]
fn an_answer_that_arrived_whole_is_an_answer_though_the_stop_asked() {
    // The stop comes as the last of the answer is read: the answer is paid
    // for and whole, so the caller gets it and decides.
    let asks = AtomicU32::new(0);
    let stop = Stop::when(move || asks.fetch_add(1, Ordering::SeqCst) >= 1);
    let claude = generator(&["messages-themes"]);
    let generated = claude
        .generate_until(&key(), &json_request(), &stop)
        .unwrap();
    assert!(generated.text.contains("themes"));
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn an_answer_that_fails_before_its_first_byte_is_a_lost_answer_that_says_why() {
    for (kind, reason) in [
        (std::io::ErrorKind::ConnectionReset, "connection reset"),
        (std::io::ErrorKind::TimedOut, "no data for 600 seconds"),
    ] {
        let claude = ClaudeTextGenerator::with_transport(Pieces::new(vec![vec![Err(
            std::io::Error::new(kind, reason),
        )]]));
        let failure = claude.generate(&key(), &json_request()).unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::AnswerLost, "{reason}");
        assert!(failure.detail.contains(reason), "{}", failure.detail);
        assert_eq!(claude.transport().sent(), 1, "{reason}");
    }
}

#[test]
fn a_whole_answer_is_told_from_a_stream_past_white_space_and_a_byte_order_mark() {
    let whole = fixture("claude", "messages-themes-not-streamed").body;
    let stream = fixture("claude", "messages-themes").body;
    let claude = ClaudeTextGenerator::with_transport(Pieces::new(vec![
        vec![Ok(b"\r\n \n".to_vec()), Ok(whole.clone().into_bytes())],
        vec![Ok(format!("\u{feff}{whole}").into_bytes())],
        // The mark in two pieces, and after white space.
        vec![
            Ok(vec![0xEF]),
            Ok([&[0xBB, 0xBF][..], whole.as_bytes()].concat()),
        ],
        vec![Ok(format!("\n\u{feff}{whole}").into_bytes())],
        vec![Ok(b"\n\n".to_vec()), Ok(stream.clone().into_bytes())],
        vec![Ok(format!("\u{feff}{stream}").into_bytes())],
    ]));
    for _ in 0..6 {
        let generated = claude.generate(&key(), &json_request()).unwrap();
        assert!(generated.text.contains("The forgotten first spacewalk"));
        assert_eq!(generated.usage.output_tokens, 893);
    }
}

#[test]
fn a_whole_answer_that_cannot_be_read_is_a_lost_answer() {
    // Cut before its end: Claude wrote it, and what came of it is gone.
    let (sleeper, _) = recording_sleeper();
    let claude = ClaudeTextGenerator::with_transport(Scripted::new(vec![HttpResponse::new(
        200,
        r#"{"id":"msg_01","type":"message","content":[{"type":"te"#,
    )]))
    .with_sleeper(sleeper);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert!(
        failure.detail.contains("could not be read"),
        "{}",
        failure.detail
    );
    assert_eq!(failure.billed, None);
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn an_error_in_the_place_of_a_whole_answer_is_told_as_the_error() {
    let overloaded = fixture("claude", "messages-overloaded").body;
    let (sleeper, waits) = recording_sleeper();
    let claude = ClaudeTextGenerator::with_transport(Scripted::new(vec![
        HttpResponse::new(200, overloaded),
        fixture("claude", "messages-themes"),
    ]))
    .with_sleeper(sleeper);
    claude.generate(&key(), &json_request()).unwrap();
    assert_eq!(claude.transport().sent().len(), 2);
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(1)]);

    let rejected = fixture("claude", "messages-rejected").body;
    let claude =
        ClaudeTextGenerator::with_transport(Scripted::new(vec![HttpResponse::new(200, rejected)]));
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
    assert_eq!(failure.billed, None);
}

#[test]
fn a_server_error_that_is_not_claude_speaking_is_a_lost_answer_sent_once() {
    // A proxy in front answers after Claude took the request: its 503 or
    // 529 says nothing about the answer behind it.
    for (status, body) in [
        (
            503,
            "upstream connect error or disconnect/reset before headers",
        ),
        (529, ""),
        (500, r#"{"message":"Internal Server Error"}"#),
    ] {
        let (sleeper, _) = recording_sleeper();
        let claude = ClaudeTextGenerator::with_transport(Scripted::new(vec![
            HttpResponse::new(status, body),
            fixture("claude", "messages-themes"),
        ]))
        .with_sleeper(sleeper);
        let failure = claude.generate(&key(), &json_request()).unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::AnswerLost, "{status}");
        assert_eq!(claude.transport().sent().len(), 1, "{status}");
    }
}

#[test]
fn an_overload_that_opens_the_stream_is_retried_like_its_status() {
    let (sleeper, waits) = recording_sleeper();
    let answers = ["messages-overloaded-at-start", "messages-themes"]
        .iter()
        .map(|name| fixture("claude", name))
        .collect();
    let claude = ClaudeTextGenerator::with_transport(Scripted::new(answers)).with_sleeper(sleeper);

    claude.generate(&key(), &json_request()).unwrap();

    assert_eq!(claude.transport().sent().len(), 2);
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(1)]);
}

#[test]
fn a_lasting_overload_at_the_start_of_the_stream_is_provider_down() {
    let claude = generator(&["messages-overloaded-at-start"; 4]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
    assert_eq!(failure.detail, "Overloaded");
    assert_eq!(claude.transport().sent().len(), 4);
}

#[test]
fn a_rejected_key_fails_with_the_providers_message() {
    let failure = generator(&["messages-rejected"])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
    assert_eq!(failure.detail, "API key is invalid.");
}

#[test]
fn a_refusal_is_declined_with_the_reason() {
    let failure = generator(&["messages-refusal"])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Declined);
    assert!(
        failure.detail.contains("does not write"),
        "{}",
        failure.detail
    );
}

#[test]
fn an_answer_cut_at_the_limit_is_unexpected_and_says_what_it_billed() {
    let failure = generator(&["messages-max-tokens"])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    // Claude bills every token of an answer it cuts.
    assert_eq!(
        failure.billed,
        Some(BilledUse {
            model: "claude-opus-5-5".into(),
            usage: Metered {
                input_tokens: 412,
                output_tokens: 16_000,
                ..Metered::default()
            },
        })
    );
}

#[test]
fn an_answer_without_text_says_what_it_billed() {
    let failure = generator(&["messages-no-text"])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    let billed = failure.billed.expect("the answer reports its usage");
    assert_eq!(
        (billed.usage.input_tokens, billed.usage.output_tokens),
        (412, 950)
    );
}

/// The recorded refusal with another reason, or with something written
/// before it.
fn refusal(category: &str, output_tokens: u64) -> ClaudeTextGenerator<Scripted> {
    let mut refused = fixture("claude", "messages-refusal");
    refused.body = refused.body.replace("general_harms", category).replace(
        r#""usage":{"output_tokens":0}"#,
        &format!(r#""usage":{{"output_tokens":{output_tokens}}}"#),
    );
    ClaudeTextGenerator::with_transport(Scripted::new(vec![refused]))
}

#[test]
fn a_refusal_says_what_it_billed_only_when_claude_bills_it() {
    // Before anything was written, a refusal is billed for some reasons
    // only; its usage is reported either way.
    let failure = generator(&["messages-refusal"])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Declined);
    assert_eq!(failure.billed, None, "general_harms is not billed");

    for category in ["bio", "frontier_llm", "reasoning_extraction"] {
        let failure = refusal(category, 0)
            .generate(&key(), &json_request())
            .unwrap_err();
        let billed = failure.billed.expect(category);
        assert_eq!(billed.model, "claude-opus-4-8", "the model that refused");
        assert_eq!(
            (billed.usage.input_tokens, billed.usage.output_tokens),
            (380, 0)
        );
    }

    // A refusal midway bills the input and what was written, whatever
    // its reason.
    let failure = refusal("general_harms", 120)
        .generate(&key(), &json_request())
        .unwrap_err();
    let billed = failure.billed.expect("what was written is billed");
    assert_eq!(
        (billed.usage.input_tokens, billed.usage.output_tokens),
        (380, 120)
    );
}

fn error_body(kind: &str, message: &str) -> String {
    json!({ "type": "error", "error": { "type": kind, "message": message } }).to_string()
}

fn error_event(kind: &str, message: &str) -> String {
    format!("event: error\ndata: {}\n\n", error_body(kind, message))
}

fn scripted(answers: Vec<HttpResponse>) -> ClaudeTextGenerator<Scripted> {
    let (sleeper, _) = recording_sleeper();
    ClaudeTextGenerator::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

#[test]
fn claude_running_out_of_time_is_a_lost_answer_whatever_its_status() {
    // Claude took the request and worked on it: not an outage to send it
    // again after, as a status or in the place of the answer.
    let timeout = error_body("timeout_error", "Request timed out");
    for answer in [
        HttpResponse::new(500, timeout.clone()),
        HttpResponse::new(504, timeout.clone()),
        HttpResponse::new(200, timeout),
        HttpResponse::new(200, error_event("timeout_error", "Request timed out")),
    ] {
        let status = answer.status;
        let claude = scripted(vec![answer, fixture("claude", "messages-themes")]);
        let failure = claude.generate(&key(), &json_request()).unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::AnswerLost, "{status}");
        assert_eq!(failure.billed, None);
        assert_eq!(claude.transport().sent().len(), 1, "{status}");
    }
}

#[test]
fn a_failure_claude_asks_not_to_be_sent_again_is_not_sent_again() {
    // `x-should-retry: false` on Claude's own server error: neither the
    // adapter nor the queue sends it again.
    for name in ["messages-api-error", "messages-overloaded"] {
        let told = fixture("claude", name).with_header("x-should-retry", "false");
        let claude = scripted(vec![told, fixture("claude", "messages-themes")]);
        let failure = claude.generate(&key(), &json_request()).unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::AnswerLost, "{name}");
        assert_eq!(claude.transport().sent().len(), 1, "{name}");
    }
    // A rate limit started nothing: it is told as it is, once.
    let told = fixture("claude", "messages-rate-limited").with_header("x-should-retry", "false");
    let claude = scripted(vec![told, fixture("claude", "messages-themes")]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::LimitReached);
    assert_eq!(claude.transport().sent().len(), 1);
    // The header saying yes changes nothing.
    let told = fixture("claude", "messages-overloaded").with_header("x-should-retry", "true");
    let claude = scripted(vec![told, fixture("claude", "messages-themes")]);
    claude.generate(&key(), &json_request()).unwrap();
    assert_eq!(claude.transport().sent().len(), 2);
}

#[test]
fn a_success_that_is_not_an_answer_is_a_lost_answer() {
    // Something answered 200 in JSON, or with a stream that only ends:
    // what Claude may have written did not arrive.
    for body in [
        "{}",
        r#"{"error":{"message":"upstream closed"}}"#,
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    ] {
        let claude = scripted(vec![
            HttpResponse::new(200, body),
            fixture("claude", "messages-themes"),
        ]);
        let failure = claude.generate(&key(), &json_request()).unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::AnswerLost, "{body}");
        assert_eq!(failure.billed, None, "{body}");
        assert_eq!(claude.transport().sent().len(), 1, "{body}");
    }
}

#[test]
fn a_bad_request_in_the_place_of_the_answer_is_read_like_its_status() {
    // Without the fallback beta: asked again without it.
    let beta = fixture("claude", "messages-beta-not-enabled").body;
    let beta: serde_json::Value = serde_json::from_str(&beta).unwrap();
    let event = error_event(
        "invalid_request_error",
        beta["error"]["message"].as_str().unwrap(),
    );
    let claude = scripted(vec![
        HttpResponse::new(200, event),
        fixture("claude", "messages-themes"),
    ]);
    claude.generate(&key(), &json_request()).unwrap();
    let sent = claude.transport().sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].header("anthropic-beta"), None);

    // An empty balance is a limit.
    let event = error_event(
        "invalid_request_error",
        "Your credit balance is too low to access the Anthropic API.",
    );
    let failure = scripted(vec![HttpResponse::new(200, event)])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::LimitReached);
}

#[test]
fn a_rate_limit_that_opens_the_stream_waits_as_long_as_it_asks() {
    let (sleeper, waits) = recording_sleeper();
    let limited = HttpResponse::new(200, error_event("rate_limit_error", "Too many requests"))
        .with_header("retry-after", "7");
    let claude = ClaudeTextGenerator::with_transport(Scripted::new(vec![
        limited,
        fixture("claude", "messages-themes"),
    ]))
    .with_sleeper(sleeper);
    claude.generate(&key(), &json_request()).unwrap();
    assert_eq!(claude.transport().sent().len(), 2);
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(7)]);
}

#[test]
fn claudes_own_word_that_it_has_no_room_is_sent_again() {
    // A 503 in the API's own error report started nothing.
    let (sleeper, waits) = recording_sleeper();
    let busy = HttpResponse::new(503, error_body("overloaded_error", "Overloaded"))
        .with_header("retry-after", "3");
    let claude = ClaudeTextGenerator::with_transport(Scripted::new(vec![
        busy,
        fixture("claude", "messages-themes"),
    ]))
    .with_sleeper(sleeper);
    claude.generate(&key(), &json_request()).unwrap();
    assert_eq!(claude.transport().sent().len(), 2);
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(3)]);
}

#[test]
fn a_failure_that_started_nothing_bills_nothing() {
    for name in ["messages-rejected", "messages-credit-balance"] {
        let failure = generator(&[name])
            .generate(&key(), &json_request())
            .unwrap_err();
        assert_eq!(failure.billed, None, "{name}");
    }
}

#[test]
fn an_empty_balance_is_a_limit() {
    let failure = generator(&["messages-credit-balance"])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::LimitReached);
    assert!(failure.detail.contains("credit balance"));
}

#[test]
fn without_the_fallback_beta_it_asks_again_without_it() {
    let claude = generator(&["messages-beta-not-enabled", "messages-themes"]);
    let generated = claude.generate(&key(), &json_request()).unwrap();
    assert!(generated.text.contains("themes"));

    let sent = claude.transport().sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].header("anthropic-beta"), None);
    assert!(sent[1].body.get("fallbacks").is_none());
}

#[test]
fn overload_and_rate_limits_are_retried_with_backoff() {
    let (sleeper, waits) = recording_sleeper();
    let answers = [
        "messages-overloaded",
        "messages-rate-limited",
        "messages-themes",
    ]
    .iter()
    .map(|name| fixture("claude", name))
    .collect();
    let claude = ClaudeTextGenerator::with_transport(Scripted::new(answers)).with_sleeper(sleeper);

    claude.generate(&key(), &json_request()).unwrap();

    assert_eq!(claude.transport().sent().len(), 3);
    assert_eq!(
        *waits.lock().unwrap(),
        [Duration::from_secs(1), Duration::from_secs(2)],
        "doubling, or the retry-after the server asked"
    );
}

#[test]
fn a_lasting_overload_is_reported_as_provider_down() {
    let claude = generator(&["messages-overloaded"; 4]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
    assert_eq!(claude.transport().sent().len(), 4, "three more, no more");

    let claude = generator(&["messages-rate-limited"; 4]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::LimitReached);
    assert_eq!(claude.transport().sent().len(), 4);
}

#[test]
fn a_request_that_never_left_is_unreachable() {
    let claude = ClaudeTextGenerator::with_transport(Scripted::offline());
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
    assert!(failure.detail.contains("dns"));
}

#[test]
fn a_request_that_was_sent_and_got_no_answer_is_a_lost_answer_sent_once() {
    let claude = ClaudeTextGenerator::with_transport(Scripted::losing_the_answer());
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert!(failure.detail.contains("timeout"), "{}", failure.detail);
    assert_eq!(failure.billed, None, "nothing says what it used");
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn a_gateway_that_gave_up_on_claude_is_a_lost_answer_sent_once() {
    let claude = generator(&["messages-gateway-timeout"]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert!(failure.detail.contains("504"), "{}", failure.detail);
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn claude_failing_on_its_side_is_told_once_and_left_to_the_queue() {
    let claude = generator(&["messages-api-error"]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
    assert_eq!(failure.detail, "Internal server error");
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn an_invalid_schema_fails_before_calling() {
    let claude = generator(&[]);
    let failure = claude
        .generate(
            &key(),
            &request(TextFormat::Json {
                schema: "{not json".into(),
            }),
        )
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert!(claude.transport().sent().is_empty());
}
