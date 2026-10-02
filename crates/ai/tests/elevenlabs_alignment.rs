//! ElevenLabs forced alignment against recorded responses (see
//! `fixtures/elevenlabs/README.md`). No test calls ElevenLabs.

mod common;

use std::time::Duration;

use bardo_ai::ElevenLabsAlignment;
use bardo_ai::elevenlabs::{ALIGNMENT_MODEL, ALIGNMENT_URL};
use bardo_ai::http::Method;
use bardo_domain::{AlignmentRequest, ApiKey, Provider, ProviderFailureKind, SpeechAligner};
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "sk_test_elevenlabs_key_0001";
const TEXT: &str = "Hi, you. It is 1969.";
const AUDIO: &[u8] = include_bytes!("../../media/tests/fixtures/tone-1s-raw.mp3");

fn key() -> ApiKey {
    ApiKey::parse(Provider::ElevenLabs, KEY).unwrap()
}

fn request() -> AlignmentRequest<'static> {
    AlignmentRequest {
        audio: AUDIO,
        file_name: "narration.mp3",
        text: TEXT,
    }
}

fn aligner(answers: &[&str]) -> ElevenLabsAlignment<Scripted> {
    let answers = answers
        .iter()
        .map(|name| fixture("elevenlabs", name))
        .collect();
    let (sleeper, _) = recording_sleeper();
    ElevenLabsAlignment::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn sends_the_recording_and_its_text_as_a_form_with_the_key_in_a_header() {
    let adapter = aligner(&["alignment-hello"]);
    adapter.align(&key(), &request()).unwrap();

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!(sent.method, Method::Post);
    assert_eq!(sent.url, ALIGNMENT_URL);
    assert_eq!(sent.header("xi-api-key"), Some(KEY));
    assert!(
        sent.header("content-type")
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    let body = &sent.raw_body;
    assert!(contains(
        body,
        b"name=\"file\"; filename=\"narration.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\n"
    ));
    assert!(contains(body, AUDIO), "the whole file is sent");
    assert!(contains(
        body,
        format!("name=\"text\"\r\n\r\n{TEXT}\r\n").as_bytes()
    ));
    assert!(
        !contains(body, KEY.as_bytes()),
        "the key is not in the form"
    );
}

#[test]
fn wav_recordings_say_so() {
    let adapter = aligner(&["alignment-hello"]);
    adapter
        .align(
            &key(),
            &AlignmentRequest {
                file_name: "narration-1.WAV",
                ..request()
            },
        )
        .unwrap();
    assert!(contains(
        &adapter.transport().sent()[0].raw_body,
        b"Content-Type: audio/wav\r\n"
    ));
}

#[test]
fn reads_the_time_of_every_character() {
    let aligned = aligner(&["alignment-hello"])
        .align(&key(), &request())
        .unwrap();
    assert_eq!(aligned.model, ALIGNMENT_MODEL);
    let chars = &aligned.alignment.chars;
    let text: String = chars.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(text, TEXT);
    assert_eq!(chars[0].start, Duration::from_millis(120));
    assert_eq!(chars[0].end, Duration::from_millis(190));
    assert!(chars.windows(2).all(|pair| pair[0].start <= pair[1].start));
}

#[test]
fn an_answer_without_timings_is_unexpected() {
    let failure = aligner(&["alignment-no-timings"])
        .align(&key(), &request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
}

#[test]
fn a_wrong_key_is_rejected() {
    let failure = aligner(&["alignment-rejected"])
        .align(&key(), &request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
    assert_eq!(failure.detail, "Invalid API key");
}

#[test]
fn a_recording_the_provider_cannot_use_says_why_without_blaming_the_key() {
    let failure = aligner(&["alignment-missing-file"])
        .align(&key(), &request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert_eq!(
        failure.detail,
        "the recording was not accepted: Field required"
    );
}

#[test]
fn a_rate_limit_is_waited_out() {
    let adapter = aligner(&["alignment-rate-limited", "alignment-hello"]);
    assert!(adapter.align(&key(), &request()).is_ok());
    assert_eq!(adapter.transport().sent().len(), 2);
}

#[test]
fn an_unreachable_provider_is_reported() {
    let failure = ElevenLabsAlignment::with_transport(Scripted::offline())
        .align(&key(), &request())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
