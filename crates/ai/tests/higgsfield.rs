//! Higgsfield clips against recorded responses (see
//! `fixtures/higgsfield/README.md`). No test calls Higgsfield.

mod common;

use bardo_ai::HiggsfieldClips;
use bardo_ai::higgsfield::{API_URL, DEFAULT_MODEL};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipRequest, ClipStatus,
    ImageFormat, Money, Provider, ProviderFailureKind, StagedImage,
};
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "3f9e2c1a-7b4d-4e8f-9a21-5c6d7e8f9a0b:hf_secret_test_0001";
const REQUEST_ID: &str = "d7e6c0f3-6699-4f6c-bb45-2ad7fd9158ff";
const PUBLIC_URL: &str = "https://cdn.higgsfield.example/input/3f1c9a52.png";
const VIDEO_URL: &str = "https://cdn.higgsfield.example/output/d7e6c0f3.mp4";
const SUBMISSION: &str = "4b0f6f0e-2d55-4b8e-8b0a-1f2e3d4c5b6a-1";

fn key() -> ApiKey {
    ApiKey::parse(Provider::Higgsfield, KEY).unwrap()
}

fn clips(answers: &[&str]) -> HiggsfieldClips<Scripted> {
    let answers = answers
        .iter()
        .map(|name| fixture("higgsfield", name))
        .collect();
    let (sleeper, _) = recording_sleeper();
    HiggsfieldClips::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

fn request(model: &str) -> ClipRequest {
    ClipRequest {
        model: model.into(),
        prompt: "Slow push in on the launch pad, steam rising.".into(),
        image: StagedImage(PUBLIC_URL.into()),
        seconds: 8,
    }
}

fn handle() -> ClipHandle {
    ClipHandle(REQUEST_ID.into())
}

#[test]
fn the_catalog_starts_with_the_default_and_says_which_lengths_each_takes() {
    let adapter = clips(&[]);
    assert_eq!(adapter.provider(), Provider::Higgsfield);
    let models = adapter.models();
    assert_eq!(models[0].id.model(), DEFAULT_MODEL);
    assert_eq!(models[0].name, "Kling 3.0 Standard");
    assert_eq!(
        models[0].durations,
        ClipDurations::Range { min: 3, max: 15 }
    );
    let kling_26 = models
        .iter()
        .find(|model| model.id.model() == "kling-video/v2.6/pro/image-to-video")
        .unwrap();
    assert_eq!(kling_26.durations, ClipDurations::Choices(vec![5, 10]));
    assert!(
        models
            .iter()
            .all(|model| model.id.provider() == Provider::Higgsfield)
    );
}

#[test]
fn the_image_goes_to_the_presigned_url_without_the_key() {
    let adapter = clips(&["upload-url", "upload-stored"]);
    let image = ClipImage {
        bytes: vec![0x89, b'P', b'N', b'G', 1, 2, 3],
        format: ImageFormat::Png,
    };
    let staged = adapter.stage_image(&key(), &image).unwrap();
    assert_eq!(staged, StagedImage(PUBLIC_URL.into()));

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].method, Method::Post);
    assert_eq!(sent[0].url, format!("{API_URL}/files/generate-upload-url"));
    assert_eq!(
        sent[0].header("authorization"),
        Some(&*format!("Key {KEY}"))
    );
    assert_eq!(sent[0].body["content_type"], "image/png");

    assert_eq!(sent[1].method, Method::Put);
    assert!(
        sent[1]
            .url
            .starts_with("https://storage.higgsfield.example/presigned/")
    );
    assert_eq!(sent[1].header("authorization"), None, "never to storage");
    assert_eq!(sent[1].header("content-type"), Some("image/png"));
    assert_eq!(sent[1].header("x-amz-tagging"), Some("retention=temporary"));
    assert_eq!(sent[1].body, "7 bytes", "the image itself");
}

#[test]
fn a_refused_upload_is_reported() {
    let failure = clips(&["upload-url", "upload-expired"])
        .stage_image(
            &key(),
            &ClipImage {
                bytes: vec![1],
                format: ImageFormat::Jpeg,
            },
        )
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
    assert!(failure.detail.contains("403"));
}

#[test]
fn a_submission_is_quoted_then_sent_once_with_its_idempotency_key() {
    let adapter = clips(&["estimate", "submit-queued"]);
    let submission = adapter
        .submit(&key(), &request(DEFAULT_MODEL), SUBMISSION)
        .unwrap();
    assert_eq!(submission.handle, handle());
    assert_eq!(submission.quote, Some(Money::from_micros(896_000)));

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].url, format!("{API_URL}/estimate/{DEFAULT_MODEL}"));
    assert_eq!(sent[0].header("idempotency-key"), None);
    let submit = &sent[1];
    assert_eq!(submit.method, Method::Post);
    assert_eq!(submit.url, format!("{API_URL}/{DEFAULT_MODEL}"));
    assert_eq!(submit.header("idempotency-key"), Some(SUBMISSION));
    assert_eq!(submit.header("authorization"), Some(&*format!("Key {KEY}")));
    assert_eq!(submit.body["image_url"], PUBLIC_URL);
    assert_eq!(submit.body["duration"], 8);
    assert_eq!(
        submit.body["prompt"],
        "Slow push in on the launch pad, steam rising."
    );
    assert_eq!(
        submit.body["sound"], "off",
        "the narration is the soundtrack"
    );
    assert_eq!(
        sent[0].body, submit.body,
        "the estimate prices this very request"
    );
}

#[test]
fn each_model_gets_its_own_fixed_fields() {
    let adapter = clips(&["estimate", "submit-queued", "estimate", "submit-queued"]);
    adapter
        .submit(
            &key(),
            &request("bytedance/seedance-2.0/image-to-video"),
            SUBMISSION,
        )
        .unwrap();
    adapter
        .submit(
            &key(),
            &request("kling-video/v2.5-turbo/standard/image-to-video"),
            SUBMISSION,
        )
        .unwrap();
    let sent = adapter.transport().sent();
    assert_eq!(sent[1].body["generate_audio"], false);
    assert_eq!(sent[1].body["resolution"], "1080p");
    assert!(sent[3].body.get("sound").is_none());
    assert!(sent[3].body.get("generate_audio").is_none());
}

#[test]
fn a_long_prompt_is_cut_where_the_models_stop_reading() {
    let adapter = clips(&["estimate", "submit-queued"]);
    let mut long = request(DEFAULT_MODEL);
    long.prompt = "é".repeat(3_000);
    adapter.submit(&key(), &long, SUBMISSION).unwrap();
    let prompt = adapter.transport().sent()[1].body["prompt"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(prompt.chars().count(), 2_500);
}

#[test]
fn a_failed_estimate_does_not_stop_the_submission() {
    let adapter = clips(&["submit-overloaded", "submit-queued"]);
    let submission = adapter
        .submit(&key(), &request(DEFAULT_MODEL), SUBMISSION)
        .unwrap();
    assert_eq!(submission.quote, None, "the rate table prices it");
    assert_eq!(submission.handle, handle());
}

#[test]
fn refused_submissions_say_why() {
    let cases = [
        (
            "submit-rejected",
            ProviderFailureKind::Rejected,
            "Invalid credentials",
        ),
        (
            "submit-no-credits",
            ProviderFailureKind::LimitReached,
            "Insufficient credits",
        ),
        (
            "submit-concurrency",
            ProviderFailureKind::ProviderDown,
            "concurrent requests",
        ),
        (
            "submit-invalid",
            ProviderFailureKind::Unexpected,
            "body.duration: Input should be less than or equal to 15",
        ),
        (
            "submit-model-not-found",
            ProviderFailureKind::NotAllowed,
            "Model not found",
        ),
    ];
    for (name, kind, detail) in cases {
        let failure = clips(&["estimate", name])
            .submit(&key(), &request(DEFAULT_MODEL), SUBMISSION)
            .unwrap_err();
        assert_eq!(failure.kind, kind, "{name}");
        assert!(
            failure.detail.contains(detail),
            "{name}: {}",
            failure.detail
        );
    }
}

#[test]
fn a_busy_model_is_retried_with_the_same_key_before_failing() {
    let adapter = clips(&[
        "estimate",
        "submit-overloaded",
        "submit-overloaded",
        "submit-queued",
    ]);
    adapter
        .submit(&key(), &request(DEFAULT_MODEL), SUBMISSION)
        .unwrap();
    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 4);
    assert!(
        sent[1..]
            .iter()
            .all(|request| request.header("idempotency-key") == Some(SUBMISSION))
    );
}

#[test]
fn status_follows_the_request_to_its_clip() {
    let adapter = clips(&["status-queued", "status-in-progress", "status-completed"]);
    assert_eq!(
        adapter.status(&key(), &handle()).unwrap(),
        ClipStatus::Queued
    );
    assert_eq!(
        adapter.status(&key(), &handle()).unwrap(),
        ClipStatus::Running
    );
    assert_eq!(
        adapter.status(&key(), &handle()).unwrap(),
        ClipStatus::Done {
            video: VIDEO_URL.into()
        }
    );
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].method, Method::Get);
    assert_eq!(
        sent[0].url,
        format!("{API_URL}/requests/{REQUEST_ID}/status")
    );
    assert_eq!(
        sent[0].header("authorization"),
        Some(&*format!("Key {KEY}"))
    );
}

#[test]
fn requests_that_end_without_a_clip_are_failures_of_their_kind() {
    let cases = [
        (
            "status-failed",
            ProviderFailureKind::ProviderDown,
            "Generation failed",
        ),
        ("status-nsfw", ProviderFailureKind::Declined, "moderation"),
        (
            "status-canceled",
            ProviderFailureKind::Unexpected,
            "cancelled",
        ),
        (
            "status-not-found",
            ProviderFailureKind::Unexpected,
            "does not know",
        ),
    ];
    for (name, kind, detail) in cases {
        let status = clips(&[name]).status(&key(), &handle()).unwrap();
        let ClipStatus::Failed(failure) = status else {
            panic!("{name}: {status:?}");
        };
        assert_eq!(failure.kind, kind, "{name}");
        assert!(
            failure.detail.contains(detail),
            "{name}: {}",
            failure.detail
        );
    }
    let no_video = clips(&["status-completed-no-video"])
        .status(&key(), &handle())
        .unwrap_err();
    assert_eq!(no_video.kind, ProviderFailureKind::Unexpected);
}

#[test]
fn a_status_check_with_a_bad_key_fails_without_ending_the_request() {
    let failure = clips(&["submit-rejected"])
        .status(&key(), &handle())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
}

#[test]
fn the_clip_downloads_without_the_key() {
    let adapter = clips(&["download-clip"]);
    let clip = adapter.download(VIDEO_URL).unwrap();
    assert_eq!(&clip.bytes[4..8], b"ftyp");
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].url, VIDEO_URL);
    assert_eq!(sent[0].header("authorization"), None);
}

#[test]
fn a_download_that_is_gone_or_not_a_clip_fails() {
    let gone = clips(&["download-expired"])
        .download(VIDEO_URL)
        .unwrap_err();
    assert_eq!(gone.kind, ProviderFailureKind::Unexpected);
    assert!(gone.detail.contains("no longer available"));
    let html = clips(&["status-completed"])
        .download(VIDEO_URL)
        .unwrap_err();
    assert!(html.detail.contains("not an MP4"));
}

#[test]
fn offline_is_unreachable() {
    let (sleeper, _) = recording_sleeper();
    let adapter = HiggsfieldClips::with_transport(Scripted::offline()).with_sleeper(sleeper);
    let failure = adapter.status(&key(), &handle()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
