//! Higgsfield take models (#192) against recorded responses (see
//! `fixtures/higgsfield/README.md`). No test calls Higgsfield.

mod common;

use std::time::Duration;

use bardo_ai::HiggsfieldTakes;
use bardo_ai::higgsfield::{API_URL, DEFAULT_TAKE_MODEL, SEEDANCE_VIDEO_EDIT};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, ClipHandle, Money, Provider, ProviderFailureKind, StagedFile, TakeDone, TakeGenerator,
    TakeInput, TakeMode, TakeRequest, TakeStatus, TakeStep,
};
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "3f9e2c1a-7b4d-4e8f-9a21-5c6d7e8f9a0b:hf_secret_test_0001";
const REQUEST_ID: &str = "d7e6c0f3-6699-4f6c-bb45-2ad7fd9158ff";
const LOOK_URL: &str = "https://cdn.higgsfield.example/input/3f1c9a52.png";
const CONTROL_URL: &str = "https://cdn.higgsfield.example/input/9a7e2b10.mp4";
const VIDEO_URL: &str = "https://cdn.higgsfield.example/output/d7e6c0f3.mp4";
const SUBMISSION: &str = "4b0f6f0e-2d55-4b8e-8b0a-1f2e3d4c5b6a-0-final-1";

fn key() -> ApiKey {
    ApiKey::parse(Provider::Higgsfield, KEY).unwrap()
}

fn takes(answers: &[&str]) -> HiggsfieldTakes<Scripted> {
    let answers = answers
        .iter()
        .map(|name| fixture("higgsfield", name))
        .collect();
    let (sleeper, _) = recording_sleeper();
    HiggsfieldTakes::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

fn request(model: &str, step: TakeStep, resolution: Option<&str>) -> TakeRequest {
    TakeRequest {
        model: model.into(),
        step,
        resolution: resolution.map(str::to_owned),
        prompt: "The character in the image talks to the camera in a dim study.".into(),
        video: StagedFile(CONTROL_URL.into()),
        look: StagedFile(LOOK_URL.into()),
        draft_task: None,
    }
}

#[test]
fn the_take_models_are_kling_motion_control_then_seedance_video_edit() {
    let models = TakeGenerator::models(&takes(&[]));
    assert_eq!(models.len(), 2);
    let kling = &models[0];
    assert_eq!(kling.id.model(), DEFAULT_TAKE_MODEL);
    assert_eq!(kling.id.provider(), Provider::Higgsfield);
    assert_eq!(kling.name, "Kling 3.0 Motion Control");
    assert_eq!(kling.modes, [TakeMode::Recording]);
    assert_eq!(kling.draft, None, "its cheap tier costs three quarters");
    assert_eq!(kling.final_tier.height, 1080);
    assert_eq!(kling.first_step(), TakeStep::Final);
    assert!(!kling.keeps_framing, "its picture follows the look");
    assert!(kling.fits(Duration::from_secs(30)));
    assert!(!kling.fits(Duration::from_secs(31)));
    assert_eq!(kling.max_image_bytes, 10_000_000);

    let seedance = &models[1];
    assert_eq!(seedance.id.model(), SEEDANCE_VIDEO_EDIT);
    assert_eq!(seedance.modes, [TakeMode::Mesh, TakeMode::Depth]);
    assert_eq!(seedance.default_mode(), TakeMode::Mesh);
    assert_eq!(seedance.draft.as_ref().unwrap().height, 480);
    assert_eq!(
        seedance.priced_model(TakeStep::Draft),
        format!("{SEEDANCE_VIDEO_EDIT}/480p")
    );
    assert_eq!(
        seedance.priced_model(TakeStep::Final),
        format!("{SEEDANCE_VIDEO_EDIT}/1080p")
    );
    assert!(!seedance.finishes_draft, "Higgsfield has no draft task");
    assert!(
        seedance.keeps_framing,
        "a video edit keeps the take's framing"
    );
    assert!(!seedance.fits(Duration::from_millis(3_900)));
}

#[test]
fn a_control_video_goes_to_the_presigned_url_as_mp4_without_the_key() {
    let adapter = takes(&["upload-url-video", "upload-stored"]);
    let bytes = b"\0\0\0\x18ftypisom-control";
    let staged = adapter
        .stage(
            &key(),
            TakeInput {
                bytes,
                mime: "video/mp4",
            },
        )
        .unwrap();
    assert_eq!(staged, StagedFile(CONTROL_URL.into()));
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].url, format!("{API_URL}/files/generate-upload-url"));
    assert_eq!(sent[0].body["content_type"], "video/mp4");
    assert_eq!(
        sent[0].header("authorization"),
        Some(&*format!("Key {KEY}"))
    );
    assert_eq!(sent[1].method, Method::Put);
    assert_eq!(sent[1].header("authorization"), None, "never to storage");
    assert_eq!(sent[1].header("content-type"), Some("video/mp4"));
    assert_eq!(sent[1].raw_body, bytes, "the control video itself");
}

#[test]
fn kling_motion_control_gets_the_look_as_character_and_the_take_as_motion() {
    let adapter = takes(&["estimate-take", "submit-queued"]);
    let request = request(DEFAULT_TAKE_MODEL, TakeStep::Final, None);
    assert_eq!(
        adapter.quote(&key(), &request),
        Some(Money::from_micros(5_040_000))
    );
    let handle = adapter.submit(&key(), &request, SUBMISSION).unwrap();
    assert_eq!(handle, ClipHandle(REQUEST_ID.into()));

    let sent = adapter.transport().sent();
    assert_eq!(
        sent[0].url,
        format!("{API_URL}/estimate/{DEFAULT_TAKE_MODEL}")
    );
    assert_eq!(sent[0].header("idempotency-key"), None);
    let submit = &sent[1];
    assert_eq!(submit.url, format!("{API_URL}/{DEFAULT_TAKE_MODEL}"));
    assert_eq!(submit.header("idempotency-key"), Some(SUBMISSION));
    assert_eq!(submit.body["image_url"], LOOK_URL);
    assert_eq!(submit.body["video_url"], CONTROL_URL);
    assert_eq!(submit.body["character_orientation"], "video");
    assert_eq!(
        submit.body["keep_original_sound"], "no",
        "the take's voice is the only sound"
    );
    assert_eq!(submit.body["prompt"], request.prompt);
    assert!(submit.body.get("duration").is_none(), "the take sets it");
    assert_eq!(
        sent[0].body, submit.body,
        "the estimate prices this request"
    );
}

#[test]
fn seedance_video_edit_gets_the_tier_and_never_makes_sound() {
    let adapter = takes(&["submit-queued", "submit-queued"]);
    for (step, resolution) in [(TakeStep::Draft, "480p"), (TakeStep::Final, "1080p")] {
        adapter
            .submit(
                &key(),
                &request(SEEDANCE_VIDEO_EDIT, step, Some(resolution)),
                SUBMISSION,
            )
            .unwrap();
    }
    let sent = adapter.transport().sent();
    for (body, resolution) in [(&sent[0].body, "480p"), (&sent[1].body, "1080p")] {
        assert_eq!(body["video_url"], CONTROL_URL, "the voice is inside it");
        assert_eq!(body["image_urls"], serde_json::json!([LOOK_URL]));
        assert_eq!(body["resolution"], resolution);
        assert_eq!(body["generate_audio"], false);
        assert!(body.get("audio_urls").is_none());
        assert!(body.get("image_url").is_none());
        assert!(body.get("duration").is_none());
    }
    assert_eq!(sent[0].url, format!("{API_URL}/{SEEDANCE_VIDEO_EDIT}"));
}

#[test]
fn a_take_prompt_is_cut_where_kling_stops_reading() {
    let adapter = takes(&["submit-queued"]);
    let mut long = request(DEFAULT_TAKE_MODEL, TakeStep::Final, None);
    long.prompt = "é".repeat(3_000);
    adapter.submit(&key(), &long, SUBMISSION).unwrap();
    let prompt = adapter.transport().sent()[0].body["prompt"]
        .as_str()
        .unwrap()
        .chars()
        .count();
    assert_eq!(prompt, 2_500);
}

#[test]
fn no_quote_when_the_estimate_fails() {
    let adapter = takes(&["submit-overloaded"]);
    let request = request(DEFAULT_TAKE_MODEL, TakeStep::Final, None);
    assert_eq!(adapter.quote(&key(), &request), None);
}

#[test]
fn a_refused_take_submission_says_why() {
    for (answer, kind) in [
        ("submit-no-credits", ProviderFailureKind::LimitReached),
        ("submit-invalid", ProviderFailureKind::Unexpected),
        ("submit-model-not-found", ProviderFailureKind::NotAllowed),
    ] {
        let failure = takes(&[answer])
            .submit(
                &key(),
                &request(DEFAULT_TAKE_MODEL, TakeStep::Final, None),
                SUBMISSION,
            )
            .unwrap_err();
        assert_eq!(failure.kind, kind, "{answer}");
    }
}

#[test]
fn a_moderated_take_is_declined_and_a_finished_one_downloads_without_the_key() {
    let adapter = takes(&[
        "status-nsfw",
        "status-failed",
        "status-completed",
        "download-clip",
    ]);
    let handle = ClipHandle(REQUEST_ID.into());
    match adapter.status(&key(), &handle).unwrap() {
        TakeStatus::Failed(failure) => assert_eq!(failure.kind, ProviderFailureKind::Declined),
        other => panic!("{other:?}"),
    }
    match adapter.status(&key(), &handle).unwrap() {
        TakeStatus::Failed(failure) => {
            assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
            assert!(failure.detail.contains("Generation failed"), "its reason");
        }
        other => panic!("{other:?}"),
    }
    let TakeStatus::Done(TakeDone { video, .. }) = adapter.status(&key(), &handle).unwrap() else {
        panic!("done");
    };
    assert_eq!(video, VIDEO_URL);
    let clip = adapter.download(&key(), &video).unwrap();
    assert_eq!(&clip.bytes[4..8], b"ftyp");
    let sent = adapter.transport().sent();
    assert_eq!(sent[3].url, VIDEO_URL);
    assert_eq!(sent[3].header("authorization"), None, "the CDN gets no key");
}
