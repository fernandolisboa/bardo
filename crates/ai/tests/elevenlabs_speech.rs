//! ElevenLabs narration against recorded responses (see
//! `fixtures/elevenlabs/README.md`). No test calls ElevenLabs.

mod common;

use std::time::Duration;

use bardo_ai::ElevenLabsSpeech;
use bardo_ai::elevenlabs::{
    CONTEXT_CHARS, MAX_SPEECH_CHARS, OUTPUT_FORMAT, SPEECH_MODEL, SPEECH_URL,
};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, GenerationPresets, Provider, ProviderFailureKind, SpeechRequest, SpeechSynthesizer,
    VoiceRef,
};
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "sk_test_elevenlabs_key_0001";
const TEXT: &str = "Hello, world. It is 1969.";
const WYATT: &str = "FrS6cKLB1wg4WYgPa9GW";

fn key() -> ApiKey {
    ApiKey::parse(Provider::ElevenLabs, KEY).unwrap()
}

fn request() -> SpeechRequest {
    SpeechRequest {
        voice: VoiceRef::elevenlabs(WYATT, "Wyatt").unwrap(),
        presets: GenerationPresets {
            stability: 60,
            similarity: 75,
            style: 35,
            speed: 95,
        },
        text: TEXT.into(),
        previous_text: None,
        next_text: None,
    }
}

fn speech(answers: &[&str]) -> ElevenLabsSpeech<Scripted> {
    let answers = answers
        .iter()
        .map(|name| fixture("elevenlabs", name))
        .collect();
    let (sleeper, _) = recording_sleeper();
    ElevenLabsSpeech::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

#[test]
fn sends_the_text_voice_and_presets_with_the_key_in_a_header() {
    let adapter = speech(&["tts-hello"]);
    adapter.synthesize(&key(), &request()).unwrap();

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!(sent.method, Method::Post);
    assert_eq!(
        sent.url,
        format!("{SPEECH_URL}/{WYATT}/with-timestamps?output_format={OUTPUT_FORMAT}")
    );
    assert_eq!(sent.header("xi-api-key"), Some(KEY));
    assert!(!sent.url.contains(KEY));
    assert_eq!(sent.body["text"], TEXT);
    assert_eq!(sent.body["model_id"], SPEECH_MODEL);
    let settings = &sent.body["voice_settings"];
    assert_eq!(settings["stability"], 0.6);
    assert_eq!(settings["similarity_boost"], 0.75);
    assert_eq!(settings["style"], 0.35);
    assert_eq!(settings["speed"], 0.95);
    assert!(sent.body.get("previous_text").is_none());
    assert!(sent.body.get("next_text").is_none());
}

#[test]
fn neighbouring_parts_go_as_short_context() {
    let adapter = speech(&["tts-hello"]);
    let previous = format!("{}End of the part before.", "x".repeat(1_000));
    let next = format!("Start of the next part.{}", "y".repeat(1_000));
    adapter
        .synthesize(
            &key(),
            &SpeechRequest {
                previous_text: Some(previous),
                next_text: Some(next),
                ..request()
            },
        )
        .unwrap();
    let body = &adapter.transport().sent()[0].body;
    let previous = body["previous_text"].as_str().unwrap();
    let next = body["next_text"].as_str().unwrap();
    assert_eq!(previous.chars().count(), CONTEXT_CHARS);
    assert!(previous.ends_with("End of the part before."));
    assert_eq!(next.chars().count(), CONTEXT_CHARS);
    assert!(next.starts_with("Start of the next part."));
}

#[test]
fn returns_the_audio_timings_of_the_text_as_sent_and_the_billed_cost() {
    let result = speech(&["tts-hello"])
        .synthesize(&key(), &request())
        .unwrap();

    assert!(result.audio.starts_with(&[0xFF, 0xFB]), "MP3 frames");
    assert_eq!(result.model, SPEECH_MODEL);
    assert_eq!(result.billed_characters, 25);
    let text: String = result
        .alignment
        .chars
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(text, TEXT, "not the normalized text");
    let first = &result.alignment.chars[0];
    assert_eq!(first.start, Duration::from_millis(50));
    assert_eq!(first.end, Duration::from_millis(130));
}

#[test]
fn falls_back_to_normalized_timings_and_the_text_length() {
    let result = speech(&["tts-normalized-only"])
        .synthesize(&key(), &request())
        .unwrap();
    let text: String = result
        .alignment
        .chars
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(text, "Hello, world. It is nineteen sixty-nine.");
    assert_eq!(result.billed_characters, TEXT.chars().count() as u64);
}

#[test]
fn a_part_is_at_most_half_the_models_limit() {
    const { assert!(MAX_SPEECH_CHARS <= 5_000) };
    assert_eq!(speech(&[]).max_chars(), MAX_SPEECH_CHARS);
}

#[test]
fn errors_are_classified() {
    for (name, kind) in [
        ("tts-rejected", ProviderFailureKind::Rejected),
        ("tts-quota-exceeded", ProviderFailureKind::LimitReached),
        ("tts-voice-not-found", ProviderFailureKind::Unexpected),
        ("tts-no-audio", ProviderFailureKind::Unexpected),
        ("tts-misaligned", ProviderFailureKind::Unexpected),
    ] {
        let failure = speech(&[name]).synthesize(&key(), &request()).unwrap_err();
        assert_eq!(failure.kind, kind, "{name}: {}", failure.detail);
    }
    let quota = speech(&["tts-quota-exceeded"])
        .synthesize(&key(), &request())
        .unwrap_err();
    assert!(quota.detail.contains("credits"), "{}", quota.detail);
}

#[test]
fn rate_limits_are_retried_before_failing() {
    let adapter = speech(&["tts-rate-limited", "tts-hello"]);
    assert!(adapter.synthesize(&key(), &request()).is_ok());
    assert_eq!(adapter.transport().sent().len(), 2);
}

#[test]
fn offline_is_unreachable() {
    let (sleeper, _) = recording_sleeper();
    let failure = ElevenLabsSpeech::with_transport(Scripted::offline())
        .with_sleeper(sleeper)
        .synthesize(&key(), &request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
