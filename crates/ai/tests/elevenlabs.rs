//! ElevenLabs voice listing against recorded responses (see
//! `fixtures/elevenlabs/README.md`). No test calls ElevenLabs.

mod common;

use std::time::Duration;

use bardo_ai::ElevenLabsVoices;
use bardo_ai::elevenlabs::{PAGE_SIZE, VOICES_URL};
use bardo_ai::http::Method;
use bardo_domain::{ApiKey, Provider, ProviderFailureKind, VoiceCategory, VoiceLibrary};
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "sk_test_elevenlabs_key_0001";

fn key() -> ApiKey {
    ApiKey::parse(Provider::ElevenLabs, KEY).unwrap()
}

fn library(answers: &[&str]) -> ElevenLabsVoices<Scripted> {
    let answers = answers
        .iter()
        .map(|name| fixture("elevenlabs", name))
        .collect();
    let (sleeper, _) = recording_sleeper();
    ElevenLabsVoices::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

#[test]
fn lists_every_page_with_the_key_in_a_header() {
    let voices = library(&["voices-page-1", "voices-page-2"]);
    voices.voices(&key()).unwrap();

    let sent = voices.transport().sent();
    assert_eq!(sent.len(), 2);
    for request in &sent {
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.header("xi-api-key"), Some(KEY));
        assert!(request.url.starts_with(VOICES_URL), "{}", request.url);
        assert!(
            request.url.contains(&format!("page_size={PAGE_SIZE}")),
            "{}",
            request.url
        );
        assert!(!request.url.contains(KEY), "the key stays out of the URL");
    }
    assert!(!sent[0].url.contains("next_page_token"));
    assert!(
        sent[1].url.contains("next_page_token=page-2-token"),
        "{}",
        sent[1].url
    );
}

#[test]
fn keeps_the_reference_category_description_and_labels() {
    let voices = library(&["voices-page-1", "voices-page-2"])
        .voices(&key())
        .unwrap();

    let names: Vec<_> = voices.iter().map(|v| v.reference.name()).collect();
    assert_eq!(
        names,
        ["Minha voz", "Narrador Épico", "Florence", "Wyatt"],
        "own voices first, repeats and unusable ids dropped"
    );

    let wyatt = voices
        .iter()
        .find(|v| v.reference.name() == "Wyatt")
        .unwrap();
    assert_eq!(wyatt.reference.provider(), Provider::ElevenLabs);
    assert_eq!(wyatt.reference.id(), "FrS6cKLB1wg4WYgPa9GW");
    assert_eq!(wyatt.category, VoiceCategory::Default);
    assert!(wyatt.description.starts_with("Measured and thoughtful"));
    assert_eq!(
        wyatt.labels,
        ["male", "middle aged", "american", "deep", "narrative story"]
    );

    assert_eq!(
        wyatt.preview_url.as_deref(),
        Some("https://storage.googleapis.com/eleven-public-prod/preview.mp3")
    );

    let clone = &voices[0];
    assert_eq!(clone.category, VoiceCategory::Cloned);
    assert_eq!(clone.description, "");
    assert!(clone.labels.is_empty());
    assert_eq!(clone.preview_url, None, "no preview listed");
    assert_eq!(voices[1].preview_url, None, "a plain HTTP link is dropped");
    assert_eq!(voices[1].category, VoiceCategory::Generated);
    assert_eq!(voices[1].labels, ["male", "old", "narrative story"]);
}

#[test]
fn an_invalid_key_is_rejected_with_the_providers_message() {
    let failure = library(&["voices-rejected"]).voices(&key()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
    assert_eq!(failure.detail, "Invalid API key");
}

#[test]
fn a_key_without_voice_permission_is_not_allowed() {
    let failure = library(&["voices-missing-permission"])
        .voices(&key())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::NotAllowed);
    assert!(failure.detail.contains("voices_read"), "{}", failure.detail);
}

#[test]
fn a_rate_limit_is_retried_after_the_asked_wait() {
    let (sleeper, waits) = recording_sleeper();
    let answers = ["voices-rate-limited", "voices-page-2"]
        .iter()
        .map(|name| fixture("elevenlabs", name))
        .collect();
    let voices = ElevenLabsVoices::with_transport(Scripted::new(answers)).with_sleeper(sleeper);
    assert_eq!(voices.voices(&key()).unwrap().len(), 3);
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(2)]);
}

#[test]
fn an_unreadable_answer_is_unexpected() {
    let failure = library(&["voices-not-json"]).voices(&key()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
}

#[test]
fn no_network_is_unreachable() {
    let failure = ElevenLabsVoices::with_transport(Scripted::offline())
        .voices(&key())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
