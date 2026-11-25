//! Claude text generation against recorded responses (see
//! `fixtures/claude/README.md`). No test calls Claude.

mod common;

use std::time::Duration;

use bardo_ai::ClaudeTextGenerator;
use bardo_ai::claude::{FALLBACK_BETA, MESSAGES_URL, MODEL};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, BilledUse, ImageFormat, Metered, Provider, ProviderFailureKind, TextEffort, TextFormat,
    TextGenerator, TextImage, TextRequest, TokenUsage,
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
    assert!(sent.body.get("thinking").is_none(), "the model decides");
}

#[test]
fn a_careful_request_asks_for_more_thought_and_more_room() {
    let claude = generator(&["messages-themes"]);
    let careful = TextRequest {
        effort: TextEffort::Careful,
        ..request(TextFormat::Prose)
    };
    claude.generate(&key(), &careful).unwrap();
    let body = &claude.transport().sent()[0].body;
    assert_eq!(body["output_config"]["effort"], "high");
    assert_eq!(body["max_tokens"], 32_000);
    assert!(body.get("thinking").is_none(), "the model decides");
    assert_eq!(
        claude.transport().sent()[0].timeout,
        Some(Duration::from_secs(600)),
        "and waits longer for the answer"
    );

    // Everyday requests are as they were.
    let claude = generator(&["messages-themes"]);
    claude
        .generate(&key(), &request(TextFormat::Prose))
        .unwrap();
    let body = &claude.transport().sent()[0].body;
    assert_eq!(body["output_config"]["effort"], "medium");
    assert_eq!(body["max_tokens"], 16_000);
    assert_eq!(claude.transport().sent()[0].timeout, None);
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

#[test]
fn a_refusal_says_what_it_billed_under_the_model_that_refused() {
    let failure = generator(&["messages-refusal"])
        .generate(&key(), &json_request())
        .unwrap_err();
    let billed = failure.billed.expect("the answer reports its usage");
    assert_eq!(billed.model, "claude-opus-4-8");
    assert_eq!(
        (billed.usage.input_tokens, billed.usage.output_tokens),
        (380, 0)
    );
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
    let failure = generator(&["messages-overloaded"; 4])
        .generate(&key(), &json_request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
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
