//! ElevenLabs speech-to-text, Voice Changer and Voice Isolator, which
//! import a capture (#189), against recorded responses (see
//! `fixtures/elevenlabs/README.md`). No test calls ElevenLabs.

mod common;

use std::time::Duration;

use bardo_ai::elevenlabs::{
    ISOLATION_MODEL, ISOLATION_URL, OUTPUT_FORMAT, TRANSCRIPTION_MODEL, TRANSCRIPTION_URL,
    VOICE_CHANGER_MAX, VOICE_CHANGER_MODEL, VOICE_CHANGER_URL,
};
use bardo_ai::http::Method;
use bardo_ai::{ElevenLabsTranscription, ElevenLabsVoiceChanger, ElevenLabsVoiceIsolator};
use bardo_domain::{
    ApiKey, GenerationPresets, HeardWord, Provider, ProviderFailureKind, SpeechTranscriber,
    TranscriptionRequest, VoiceChangeRequest, VoiceChanger, VoiceIsolator, VoiceRef,
};
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "sk_test_elevenlabs_key_0001";
const AUDIO: &[u8] = b"RIFF-stand-in-for-the-capture-sound";

fn key() -> ApiKey {
    ApiKey::parse(Provider::ElevenLabs, KEY).unwrap()
}

fn scripted(answers: &[&str]) -> Scripted {
    Scripted::new(
        answers
            .iter()
            .map(|name| fixture("elevenlabs", name))
            .collect(),
    )
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn field(name: &str, value: &str) -> Vec<u8> {
    format!("name=\"{name}\"\r\n\r\n{value}\r\n").into_bytes()
}

fn transcriber(answers: &[&str]) -> ElevenLabsTranscription<Scripted> {
    let (sleeper, _) = recording_sleeper();
    ElevenLabsTranscription::with_transport(scripted(answers)).with_sleeper(sleeper)
}

fn transcription() -> TranscriptionRequest<'static> {
    TranscriptionRequest {
        audio: AUDIO,
        file_name: "capture.wav",
        language: Some("en"),
    }
}

#[test]
fn transcribes_the_sound_word_by_word_in_the_channels_language() {
    let adapter = transcriber(&["stt-words"]);
    let transcript = adapter.transcribe(&key(), &transcription()).unwrap();

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!(sent.method, Method::Post);
    assert_eq!(sent.url, TRANSCRIPTION_URL);
    assert_eq!(sent.header("xi-api-key"), Some(KEY));
    let body = &sent.raw_body;
    assert!(contains(body, &field("model_id", TRANSCRIPTION_MODEL)));
    assert!(contains(
        body,
        b"name=\"file\"; filename=\"capture.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    ));
    assert!(contains(body, AUDIO));
    assert!(contains(body, &field("timestamps_granularity", "word")));
    assert!(contains(body, &field("tag_audio_events", "false")));
    assert!(contains(body, &field("diarize", "false")));
    assert!(contains(body, &field("language_code", "en")));
    assert!(!contains(body, KEY.as_bytes()));

    // Spacing and sound events are not words.
    let ms = Duration::from_millis;
    assert_eq!(transcript.model, "scribe_v2");
    assert_eq!(
        transcript.words,
        [
            ("Hi,", 120, 330),
            ("you.", 380, 660),
            ("It", 1200, 1300),
            ("is", 1350, 1450),
            ("1969.", 1500, 2100)
        ]
        .map(|(text, start, end)| HeardWord {
            text: text.into(),
            start: ms(start),
            end: ms(end),
        })
    );
}

#[test]
fn an_unknown_language_is_left_to_the_provider() {
    let adapter = transcriber(&["stt-words"]);
    adapter
        .transcribe(
            &key(),
            &TranscriptionRequest {
                language: None,
                ..transcription()
            },
        )
        .unwrap();
    assert!(!contains(
        &adapter.transport().sent()[0].raw_body,
        b"language_code"
    ));
}

#[test]
fn transcription_failures_say_why() {
    for (name, kind) in [
        ("stt-rejected", ProviderFailureKind::Rejected),
        ("stt-no-words", ProviderFailureKind::Unexpected),
        ("isolation-missing-file", ProviderFailureKind::Unexpected),
    ] {
        let failure = transcriber(&[name])
            .transcribe(&key(), &transcription())
            .unwrap_err();
        assert_eq!(failure.kind, kind, "{name}: {}", failure.detail);
    }
    let refused = transcriber(&["isolation-missing-file"])
        .transcribe(&key(), &transcription())
        .unwrap_err();
    assert_eq!(
        refused.detail,
        "the recording was not accepted: Field required"
    );
    let adapter = transcriber(&["stt-rate-limited", "stt-words"]);
    assert!(adapter.transcribe(&key(), &transcription()).is_ok());
    assert_eq!(adapter.transport().sent().len(), 2);
}

fn changer(answers: &[&str]) -> ElevenLabsVoiceChanger<Scripted> {
    let (sleeper, _) = recording_sleeper();
    ElevenLabsVoiceChanger::with_transport(scripted(answers)).with_sleeper(sleeper)
}

fn voice() -> VoiceRef {
    VoiceRef::elevenlabs("21m00Tcm4TlvDq8ikWAM", "Rachel").unwrap()
}

fn part() -> VoiceChangeRequest<'static> {
    VoiceChangeRequest {
        audio: AUDIO,
        file_name: "capture.part001.wav",
    }
}

#[test]
fn changes_the_voice_with_the_personas_presets_but_never_its_speed() {
    let adapter = changer(&["voice-changer-audio"]);
    assert_eq!(adapter.max_length(), VOICE_CHANGER_MAX);
    assert!(VOICE_CHANGER_MAX <= Duration::from_secs(300));
    let presets = GenerationPresets {
        stability: 40,
        similarity: 80,
        style: 10,
        speed: 115,
    };
    let changed = adapter.change(&key(), &voice(), presets, &part()).unwrap();
    assert!(changed.audio.starts_with(b"ID3"));
    assert_eq!(changed.model, VOICE_CHANGER_MODEL);

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!(sent.method, Method::Post);
    assert_eq!(
        sent.url,
        format!("{VOICE_CHANGER_URL}/21m00Tcm4TlvDq8ikWAM?output_format={OUTPUT_FORMAT}")
    );
    assert_eq!(sent.header("xi-api-key"), Some(KEY));
    let body = &sent.raw_body;
    assert!(contains(
        body,
        b"name=\"audio\"; filename=\"capture.part001.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    ));
    assert!(contains(body, AUDIO));
    assert!(contains(
        body,
        &field("model_id", "eleven_multilingual_sts_v2")
    ));
    assert!(contains(body, &field("remove_background_noise", "true")));
    let settings = String::from_utf8_lossy(body);
    let settings = settings
        .split("name=\"voice_settings\"\r\n\r\n")
        .nth(1)
        .and_then(|rest| rest.split("\r\n").next())
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(settings).unwrap(),
        serde_json::json!({
            "stability": 0.4,
            "similarity_boost": 0.8,
            "style": 0.1,
            "use_speaker_boost": true,
        }),
        "no speed: it would move the words off the performer's lips"
    );
}

#[test]
fn voice_change_failures_say_why() {
    for (name, kind) in [
        ("voice-changer-rejected", ProviderFailureKind::Rejected),
        (
            "voice-changer-voice-not-found",
            ProviderFailureKind::Unexpected,
        ),
        ("voice-changer-empty", ProviderFailureKind::Unexpected),
        ("isolation-missing-file", ProviderFailureKind::Unexpected),
    ] {
        let failure = changer(&[name])
            .change(&key(), &voice(), GenerationPresets::default(), &part())
            .unwrap_err();
        assert_eq!(failure.kind, kind, "{name}: {}", failure.detail);
    }
    let adapter = changer(&["voice-changer-rate-limited", "voice-changer-audio"]);
    assert!(
        adapter
            .change(&key(), &voice(), GenerationPresets::default(), &part())
            .is_ok()
    );
    assert_eq!(adapter.transport().sent().len(), 2);
    let (sleeper, _) = recording_sleeper();
    let offline = ElevenLabsVoiceChanger::with_transport(Scripted::offline())
        .with_sleeper(sleeper)
        .change(&key(), &voice(), GenerationPresets::default(), &part())
        .unwrap_err();
    assert_eq!(offline.kind, ProviderFailureKind::Unreachable);
}

fn isolator(answers: &[&str]) -> ElevenLabsVoiceIsolator<Scripted> {
    let (sleeper, _) = recording_sleeper();
    ElevenLabsVoiceIsolator::with_transport(scripted(answers)).with_sleeper(sleeper)
}

#[test]
fn isolates_the_voice_in_one_call() {
    let adapter = isolator(&["isolation-audio"]);
    let cleaned = adapter
        .isolate(
            &key(),
            &VoiceChangeRequest {
                audio: AUDIO,
                file_name: "capture.wav",
            },
        )
        .unwrap();
    assert!(cleaned.audio.starts_with(b"ID3"));
    assert_eq!(cleaned.model, ISOLATION_MODEL);
    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].url, ISOLATION_URL);
    assert_eq!(sent[0].header("xi-api-key"), Some(KEY));
    assert!(contains(
        &sent[0].raw_body,
        b"name=\"audio\"; filename=\"capture.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    ));
    assert!(contains(&sent[0].raw_body, &field("file_format", "other")));

    for (name, kind) in [
        ("isolation-rejected", ProviderFailureKind::Rejected),
        ("isolation-missing-file", ProviderFailureKind::Unexpected),
    ] {
        let failure = isolator(&[name])
            .isolate(
                &key(),
                &VoiceChangeRequest {
                    audio: AUDIO,
                    file_name: "capture.wav",
                },
            )
            .unwrap_err();
        assert_eq!(failure.kind, kind, "{name}: {}", failure.detail);
    }
}
