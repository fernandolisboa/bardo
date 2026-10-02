//! Claude text generation against recorded responses (see
//! `fixtures/claude/README.md`). No test calls Claude.

mod common;

use std::time::Duration;

use bardo_ai::ClaudeTextGenerator;
use bardo_ai::claude::{FALLBACK_BETA, MESSAGES_URL, MODEL};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, Provider, ProviderFailureKind, TextFormat, TextGenerator, TextRequest, TokenUsage,
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
        instructions: "You plan videos for a faceless channel.".into(),
        prompt: "Propose three themes about space history.".into(),
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
fn prose_requests_have_no_format() {
    let claude = generator(&["messages-themes"]);
    claude
        .generate(&key(), &request(TextFormat::Prose))
        .unwrap();
    let body = &claude.transport().sent()[0].body;
    assert!(body["output_config"].get("format").is_none());
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
