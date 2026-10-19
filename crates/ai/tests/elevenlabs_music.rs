//! ElevenLabs Music against recorded responses (see
//! `fixtures/elevenlabs/README.md`). No test calls ElevenLabs.

mod common;

use std::time::Duration;

use bardo_ai::ElevenLabsMusic;
use bardo_ai::elevenlabs::{MUSIC_MODEL, MUSIC_URL, OUTPUT_FORMAT};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, MusicGenerator, MusicRequest, Provider, ProviderFailureKind, music_length,
};
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "sk_test_elevenlabs_key_0001";

fn key() -> ApiKey {
    ApiKey::parse(Provider::ElevenLabs, KEY).unwrap()
}

fn request() -> MusicRequest {
    MusicRequest {
        prompt: "Slow ambient synth pads, 70 BPM, hopeful, no drums.".into(),
        length: music_length(Duration::from_nanos(95_123_456_789)),
    }
}

fn music(answers: &[&str]) -> ElevenLabsMusic<Scripted> {
    let answers = answers
        .iter()
        .map(|name| fixture("elevenlabs", name))
        .collect();
    let (sleeper, _) = recording_sleeper();
    ElevenLabsMusic::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

#[test]
fn composes_an_instrumental_signed_track_as_long_as_asked() {
    let adapter = music(&["music-track"]);
    assert_eq!(adapter.provider(), Provider::ElevenLabs);
    let track = adapter.compose(&key(), &request()).unwrap();
    assert!(track.audio.starts_with(b"ID3"));
    assert_eq!(track.model, MUSIC_MODEL);

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!(sent.method, Method::Post);
    assert_eq!(
        sent.url,
        format!("{MUSIC_URL}?output_format={OUTPUT_FORMAT}")
    );
    assert_eq!(sent.header("xi-api-key"), Some(KEY));
    assert_eq!(
        sent.body,
        serde_json::json!({
            "prompt": "Slow ambient synth pads, 70 BPM, hopeful, no drums.",
            "music_length_ms": 95_123,
            "model_id": "music_v1",
            "force_instrumental": true,
            "sign_with_c2pa": true,
        })
    );
}

#[test]
fn a_prompt_naming_an_artist_is_declined_with_elevenlabs_suggestion() {
    let failure = music(&["music-bad-prompt"])
        .compose(&key(), &request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Declined);
    assert!(
        failure.detail.ends_with(
            "ElevenLabs suggests: A calm, cinematic piano piece with soft strings and a slow build."
        ),
        "{}",
        failure.detail
    );
}

#[test]
fn a_key_without_music_or_without_credits_says_so() {
    let failure = music(&["music-missing-permission"])
        .compose(&key(), &request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::NotAllowed);
    assert!(
        failure.detail.contains("music_generation"),
        "{}",
        failure.detail
    );

    let failure = music(&["music-quota-exceeded"])
        .compose(&key(), &request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::LimitReached);
}

#[test]
fn a_busy_service_is_tried_again_before_failing() {
    let adapter = music(&["music-rate-limited", "music-track"]);
    adapter.compose(&key(), &request()).unwrap();
    assert_eq!(adapter.transport().sent().len(), 2);

    let failure = music(&[
        "music-rate-limited",
        "music-rate-limited",
        "music-rate-limited",
        "music-rate-limited",
    ])
    .compose(&key(), &request())
    .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::LimitReached);
}

#[test]
fn an_unreachable_host_is_reported_as_such() {
    let adapter = ElevenLabsMusic::with_transport(Scripted::offline());
    let failure = adapter.compose(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
