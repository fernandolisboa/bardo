//! Nano Banana images against recorded responses (see
//! `fixtures/gemini/README.md`). No test calls Gemini.

mod common;

use bardo_ai::GeminiImages;
use bardo_ai::gemini::{ASPECT_RATIO, IMAGE_MODEL, IMAGE_SIZE, MODELS_URL};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, ImageFormat, ImageGenerator, ImageRequest, Metered, Provider, ProviderFailureKind,
};
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "AIzaSyTestKeyForBardo0000000000000000";
const PROMPT: &str = "A lighthouse on a cliff at dusk, painterly, wide frame.";
const FINAL_IMAGE: &[u8] = include_bytes!("fixtures/gemini/scene-16x9.png");

fn key() -> ApiKey {
    ApiKey::parse(Provider::Gemini, KEY).unwrap()
}

fn request() -> ImageRequest {
    ImageRequest {
        prompt: PROMPT.into(),
    }
}

fn images(answers: &[&str]) -> GeminiImages<Scripted> {
    let answers = answers.iter().map(|name| fixture("gemini", name)).collect();
    let (sleeper, _) = recording_sleeper();
    GeminiImages::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

#[test]
fn sends_the_prompt_for_one_wide_image_with_the_key_in_a_header() {
    let adapter = images(&["generate-image"]);
    adapter.generate(&key(), &request()).unwrap();

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!(sent.method, Method::Post);
    assert_eq!(
        sent.url,
        format!("{MODELS_URL}/{IMAGE_MODEL}:generateContent")
    );
    assert_eq!(sent.header("x-goog-api-key"), Some(KEY));
    assert!(!sent.url.contains(KEY));
    assert_eq!(sent.body["contents"][0]["parts"][0]["text"], PROMPT);
    let config = &sent.body["generationConfig"];
    assert_eq!(config["responseModalities"], serde_json::json!(["IMAGE"]));
    assert_eq!(config["imageConfig"]["aspectRatio"], ASPECT_RATIO);
    assert_eq!(config["imageConfig"]["imageSize"], IMAGE_SIZE);
    assert_eq!(ASPECT_RATIO, "16:9");
}

#[test]
fn returns_the_final_image_not_the_drafts_with_the_tokens_counted() {
    let image = images(&["generate-image"])
        .generate(&key(), &request())
        .unwrap();
    assert_eq!(image.bytes, FINAL_IMAGE, "the thought image is a draft");
    assert_eq!(image.format, ImageFormat::Png);
    assert_eq!(image.model, "gemini-3.1-flash-image");
    assert_eq!(
        image.usage,
        Metered {
            input_tokens: 14,
            output_tokens: 218,
            image_tokens: 1_680,
            characters: 0,
        },
        "thinking is billed as text output, the image apart"
    );
}

#[test]
fn without_a_breakdown_the_whole_answer_counts_as_image() {
    let image = images(&["generate-image-no-breakdown"])
        .generate(&key(), &request())
        .unwrap();
    assert_eq!(image.usage.image_tokens, 1_680);
    assert_eq!(image.usage.output_tokens, 218);
}

#[test]
fn a_blocked_prompt_or_a_filtered_image_is_declined() {
    for name in ["generate-blocked-prompt", "generate-image-safety"] {
        let failure = images(&[name]).generate(&key(), &request()).unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::Declined, "{name}");
    }
    let safety = images(&["generate-image-safety"])
        .generate(&key(), &request())
        .unwrap_err();
    assert!(safety.detail.contains("IMAGE_SAFETY"), "{}", safety.detail);
    assert!(
        safety.detail.contains("Prohibited Use"),
        "{}",
        safety.detail
    );
}

#[test]
fn errors_are_classified() {
    for (name, kind) in [
        ("generate-rejected", ProviderFailureKind::Rejected),
        ("generate-text-only", ProviderFailureKind::Unexpected),
        ("generate-not-an-image", ProviderFailureKind::Unexpected),
    ] {
        let failure = images(&[name]).generate(&key(), &request()).unwrap_err();
        assert_eq!(failure.kind, kind, "{name}: {}", failure.detail);
    }
    let rejected = images(&["generate-rejected"])
        .generate(&key(), &request())
        .unwrap_err();
    assert_eq!(
        rejected.detail,
        "API key not valid. Please pass a valid API key."
    );
    let text = images(&["generate-text-only"])
        .generate(&key(), &request())
        .unwrap_err();
    assert!(text.detail.contains("description of the lighthouse"));
}

#[test]
fn overload_and_rate_limits_are_retried_before_failing() {
    let adapter = images(&[
        "generate-overloaded",
        "generate-rate-limited",
        "generate-image",
    ]);
    assert!(adapter.generate(&key(), &request()).is_ok());
    assert_eq!(adapter.transport().sent().len(), 3);

    let spent = images(&[
        "generate-rate-limited",
        "generate-rate-limited",
        "generate-rate-limited",
        "generate-rate-limited",
    ])
    .generate(&key(), &request())
    .unwrap_err();
    assert_eq!(spent.kind, ProviderFailureKind::LimitReached);
}

#[test]
fn offline_is_unreachable() {
    let (sleeper, _) = recording_sleeper();
    let failure = GeminiImages::with_transport(Scripted::offline())
        .with_sleeper(sleeper)
        .generate(&key(), &request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
