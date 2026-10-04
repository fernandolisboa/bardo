//! ElevenLabs stock previews against recorded responses (see
//! `fixtures/elevenlabs/README.md`). No test calls ElevenLabs.

mod common;

use bardo_ai::ElevenLabsPreviews;
use bardo_ai::http::Method;
use bardo_domain::{ProviderFailureKind, VoicePreviews};
use common::{Scripted, fixture};

const URL: &str = "https://storage.googleapis.com/eleven-public-prod/preview.mp3";

fn previews(answers: &[&str]) -> ElevenLabsPreviews<Scripted> {
    let answers = answers
        .iter()
        .map(|name| fixture("elevenlabs", name))
        .collect();
    ElevenLabsPreviews::with_transport(Scripted::new(answers))
}

#[test]
fn downloads_the_preview_without_any_key() {
    let adapter = previews(&["preview-ok"]);
    let audio = adapter.download(URL).unwrap();
    assert!(audio.starts_with(b"ID3"));

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, Method::Get);
    assert_eq!(sent[0].url, URL);
    assert_eq!(sent[0].header("xi-api-key"), None, "a public link");
    assert_eq!(sent[0].header("authorization"), None);
}

#[test]
fn only_https_links_are_followed() {
    let adapter = previews(&[]);
    for bad in [
        "http://example.com/preview.mp3",
        "file:///etc/passwd",
        "https://",
        "",
    ] {
        let failure = adapter.download(bad).unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::Unexpected, "{bad}");
    }
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn a_preview_that_is_gone_says_to_list_the_voices_again() {
    let failure = previews(&["preview-gone"]).download(URL).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert!(
        failure.detail.contains("list the voices again"),
        "{}",
        failure.detail
    );
}

#[test]
fn an_outage_or_an_empty_answer_is_a_failure() {
    let failure = previews(&["preview-down"]).download(URL).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
    assert!(failure.detail.contains("503"), "{}", failure.detail);

    let failure = previews(&["preview-empty"]).download(URL).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
}

#[test]
fn an_unreachable_host_is_reported_as_such() {
    let adapter = ElevenLabsPreviews::with_transport(Scripted::offline());
    let failure = adapter.download(URL).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
