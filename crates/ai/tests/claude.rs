//! Claude text generation against recorded responses (see
//! `fixtures/claude/README.md`). No test calls Claude.

mod common;

use std::time::Duration;

use bardo_ai::ClaudeTextGenerator;
use bardo_ai::claude::{FALLBACK_BETA, MESSAGES_URL, MODEL};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, ImageFormat, Provider, ProviderFailureKind, TextEffort, TextFormat, TextGenerator,
    TextImage, TextRequest, TokenUsage,
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
fn an_answer_of_a_known_size_gets_room_for_itself_and_for_thought() {
    // The plan of a ten-minute video: the answer and as much again.
    assert_eq!(limit_sent(TextEffort::Careful, 39_000), 78_000);
    assert_eq!(limit_sent(TextEffort::Everyday, 39_000), 78_000);
    // A short answer keeps the usual room, never less.
    assert_eq!(limit_sent(TextEffort::Careful, 2_000), 32_000);
    assert_eq!(limit_sent(TextEffort::Everyday, 2_000), 16_000);
    // Thought has a least room of its own beside a middling answer.
    assert_eq!(limit_sent(TextEffort::Careful, 20_000), 40_000);
    assert_eq!(limit_sent(TextEffort::Careful, 17_000), 34_000);
    assert_eq!(limit_sent(TextEffort::Everyday, 14_000), 28_000);
    // Never past what the model can write.
    assert_eq!(limit_sent(TextEffort::Careful, 80_000), 128_000);
    assert_eq!(limit_sent(TextEffort::Careful, u64::MAX), 128_000);
}

#[test]
fn the_longest_answer_leaves_half_as_much_room_again_for_thought() {
    let longest = bardo_ai::claude::longest_answer();
    let limit = limit_sent(TextEffort::Careful, longest);
    assert_eq!(limit, 128_000);
    assert!(limit - longest >= longest / 2);
    assert!(
        limit - (longest + 2) < (longest + 2) / 2,
        "and no longer one does"
    );
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
    // A proxy that buffers the stream answers with the whole message.
    let generated = generator(&["messages-themes-not-streamed"])
        .generate(&key(), &json_request())
        .unwrap();
    assert!(generated.text.contains("The forgotten first spacewalk"));
    assert_eq!(generated.model, "claude-opus-5-5");
    assert_eq!(generated.usage.output_tokens, 893);
}

#[test]
fn a_connection_that_drops_after_claude_began_is_not_an_outage() {
    let claude = generator(&["messages-dropped-midway"]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    // What was generated is billed: not a call to make again unasked.
    assert_ne!(failure.kind, ProviderFailureKind::Unreachable);
    assert!(
        failure.detail.contains("stopped midway"),
        "{}",
        failure.detail
    );
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn an_overload_after_the_answer_began_is_not_sent_again() {
    let claude = generator(&["messages-overloaded-midway", "messages-themes"]);
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
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
    assert_eq!(
        claude.transport().sent().len(),
        1,
        "the backoff is for answers that generated nothing"
    );
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
fn an_answer_cut_at_the_limit_is_unexpected() {
    let failure = generator(&["messages-max-tokens"])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
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
    let failure = generator(&["messages-overloaded"; 4])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
}

#[test]
fn no_answer_is_unreachable() {
    let claude = ClaudeTextGenerator::with_transport(Scripted::offline());
    let failure = claude.generate(&key(), &json_request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
    assert!(failure.detail.contains("dns"));
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
