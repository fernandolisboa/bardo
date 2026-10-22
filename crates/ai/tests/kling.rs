//! Kling clips against recorded responses (see `fixtures/kling/README.md`).
//! No test calls Kling.

mod common;

use bardo_ai::KlingClips;
use bardo_ai::http::Method;
use bardo_ai::kling::{API_URL, DEFAULT_MODEL, external_task_id};
use bardo_domain::{
    ApiKey, AspectRatio, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipRequest,
    ClipStatus, ImageFormat, Provider, ProviderFailureKind, StagedImage,
};
use base64::Engine as _;
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "kling-test-key-0001-abcdef";
const TASK: &str = "893605946402811985";
const VIDEO_URL: &str = "https://cdn.kling.example/output/893605946402811985.mp4";
const SUBMISSION: &str = "41-2-1";
const IMAGE: [u8; 7] = [0x89, b'P', b'N', b'G', 1, 2, 3];

fn key() -> ApiKey {
    ApiKey::parse(Provider::Kling, KEY).unwrap()
}

fn clips(answers: &[&str]) -> KlingClips<Scripted> {
    let answers = answers.iter().map(|name| fixture("kling", name)).collect();
    let (sleeper, _) = recording_sleeper();
    KlingClips::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

fn request(model: &str) -> ClipRequest {
    ClipRequest {
        model: model.into(),
        prompt: "Slow push in on the launch pad, steam rising.".into(),
        image: StagedImage("inline".into()),
        first_frame: ClipImage {
            bytes: IMAGE.to_vec(),
            format: ImageFormat::Png,
        },
        seconds: 8,
        aspect: AspectRatio::Landscape,
    }
}

fn handle() -> ClipHandle {
    ClipHandle(TASK.into())
}

#[test]
fn the_catalog_starts_with_the_default_and_says_which_lengths_each_takes() {
    let adapter = clips(&[]);
    assert_eq!(adapter.provider(), Provider::Kling);
    let models = adapter.models();
    assert_eq!(models[0].id.model(), DEFAULT_MODEL);
    assert_eq!(models[0].name, "Kling 3.0 720p");
    assert_eq!(
        models[0].durations,
        ClipDurations::Range { min: 3, max: 15 }
    );
    let kling_26 = models
        .iter()
        .find(|model| model.id.model() == "kling-2.6/1080p")
        .unwrap();
    assert_eq!(kling_26.durations, ClipDurations::Choices(vec![5, 10]));
    assert_eq!(models.len(), 6);
    assert!(
        models
            .iter()
            .all(|model| model.id.provider() == Provider::Kling)
    );
}

#[test]
fn nothing_is_staged_because_the_image_goes_with_the_submission() {
    let adapter = clips(&[]);
    let staged = adapter
        .stage_image(
            &key(),
            &ClipImage {
                bytes: IMAGE.to_vec(),
                format: ImageFormat::Png,
            },
        )
        .unwrap();
    assert_eq!(staged, StagedImage("inline".into()));
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn a_submission_sends_the_image_inline_with_a_custom_task_id() {
    let adapter = clips(&["submit-submitted"]);
    let submission = adapter
        .submit(&key(), &request(DEFAULT_MODEL), SUBMISSION)
        .unwrap();
    assert_eq!(submission.handle, handle());
    assert_eq!(submission.quote, None, "the rate table prices it");

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let submit = &sent[0];
    assert_eq!(submit.method, Method::Post);
    assert_eq!(submit.url, format!("{API_URL}/image-to-video/kling-3.0"));
    assert_eq!(
        submit.header("authorization"),
        Some(&*format!("Bearer {KEY}"))
    );
    let contents = submit.body["contents"].as_array().unwrap();
    assert_eq!(contents[0]["type"], "prompt");
    assert_eq!(
        contents[0]["text"],
        "Slow push in on the launch pad, steam rising."
    );
    assert_eq!(contents[1]["type"], "first_frame");
    assert_eq!(
        contents[1]["url"],
        base64::engine::general_purpose::STANDARD.encode(IMAGE),
        "raw base64, no data: prefix"
    );
    let settings = &submit.body["settings"];
    assert_eq!(settings["resolution"], "720p");
    assert_eq!(settings["duration"], 8);
    assert_eq!(settings["audio"], "off", "the narration is the soundtrack");
    assert_eq!(settings["multi_shot"], false, "a scene is one shot");
    let options = &submit.body["options"];
    assert_eq!(options["watermark_info"]["enabled"], false);
    let external = external_task_id(&request(DEFAULT_MODEL), SUBMISSION).unwrap();
    assert_eq!(options["external_task_id"], external.as_str());
}

#[test]
fn the_custom_task_id_follows_the_submission_and_the_request() {
    let id =
        |request: &ClipRequest, submission: &str| external_task_id(request, submission).unwrap();
    let base = request(DEFAULT_MODEL);
    let first = id(&base, SUBMISSION);
    assert!(first.starts_with("bardo-") && first.len() == 38, "{first}");
    assert_eq!(first, id(&base, SUBMISSION), "the same submission");
    assert_ne!(first, id(&base, "41-2-2"), "a new submission");
    let mut edited = base.clone();
    edited.prompt.push_str(" Dusk.");
    assert_ne!(first, id(&edited, SUBMISSION), "another request");
    assert_ne!(first, id(&request("kling-3.0/1080p"), SUBMISSION));
}

#[test]
fn each_model_gets_its_own_settings() {
    let adapter = clips(&["submit-submitted", "submit-submitted"]);
    adapter
        .submit(&key(), &request("kling-2.6/1080p"), SUBMISSION)
        .unwrap();
    adapter
        .submit(&key(), &request("kling-2.5-turbo/720p"), SUBMISSION)
        .unwrap();
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].url, format!("{API_URL}/image-to-video/kling-2.6"));
    assert_eq!(sent[0].body["settings"]["audio"], "off");
    assert_eq!(sent[0].body["settings"]["resolution"], "1080p");
    assert!(sent[0].body["settings"].get("multi_shot").is_none());
    assert_eq!(
        sent[1].url,
        format!("{API_URL}/image-to-video/kling-2.5-turbo")
    );
    assert!(sent[1].body["settings"].get("audio").is_none());
    assert_eq!(sent[1].body["settings"]["resolution"], "720p");
}

#[test]
fn a_model_kling_does_not_offer_is_not_sent() {
    let adapter = clips(&[]);
    let failure = adapter
        .submit(&key(), &request("kling-9.0/720p"), SUBMISSION)
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::NotAllowed);
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn a_long_prompt_is_cut_where_kling_advises() {
    let adapter = clips(&["submit-submitted"]);
    let mut long = request(DEFAULT_MODEL);
    long.prompt = "é".repeat(3_000);
    adapter.submit(&key(), &long, SUBMISSION).unwrap();
    let prompt = adapter.transport().sent()[0].body["contents"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(prompt.chars().count(), 2_500);
}

#[test]
fn a_resent_submission_finds_the_task_kling_already_has() {
    let adapter = clips(&["submit-duplicate", "lookup-found"]);
    let submission = adapter
        .submit(&key(), &request(DEFAULT_MODEL), SUBMISSION)
        .unwrap();
    assert_eq!(submission.handle, handle());
    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 2);
    let external = external_task_id(&request(DEFAULT_MODEL), SUBMISSION).unwrap();
    assert_eq!(sent[1].method, Method::Get);
    assert_eq!(
        sent[1].url,
        format!("{API_URL}/tasks?external_task_ids={external}")
    );
    assert_eq!(
        sent[1].header("authorization"),
        Some(&*format!("Bearer {KEY}"))
    );
}

#[test]
fn an_invalid_request_with_no_task_behind_it_says_why() {
    let failure = clips(&["submit-invalid", "lookup-empty"])
        .submit(&key(), &request(DEFAULT_MODEL), SUBMISSION)
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert!(
        failure.detail.contains("settings.duration"),
        "{}",
        failure.detail
    );
}

#[test]
fn refused_submissions_say_why() {
    let retried = |name| vec![name; 4];
    let cases = [
        (
            vec!["submit-rejected"],
            ProviderFailureKind::Rejected,
            "create API key",
        ),
        (
            retried("submit-no-units"),
            ProviderFailureKind::LimitReached,
            "resource pack",
        ),
        (
            retried("submit-concurrency"),
            ProviderFailureKind::ProviderDown,
            "parallel task",
        ),
        (
            vec!["submit-declined"],
            ProviderFailureKind::Declined,
            "risk control",
        ),
        (
            vec!["submit-model-not-allowed"],
            ProviderFailureKind::NotAllowed,
            "no permission",
        ),
        (
            retried("submit-overloaded"),
            ProviderFailureKind::ProviderDown,
            "unavailable",
        ),
    ];
    for (answers, kind, detail) in cases {
        let adapter = clips(&answers);
        let failure = adapter
            .submit(&key(), &request(DEFAULT_MODEL), SUBMISSION)
            .unwrap_err();
        assert_eq!(failure.kind, kind, "{answers:?}");
        assert!(
            failure.detail.contains(detail),
            "{answers:?}: {}",
            failure.detail
        );
        assert_eq!(
            adapter.transport().sent().len(),
            answers.len(),
            "no lookup for a refusal that is not about the request"
        );
    }
}

#[test]
fn a_busy_kling_is_retried_with_the_same_task_id_before_failing() {
    let adapter = clips(&[
        "submit-overloaded",
        "submit-concurrency",
        "submit-submitted",
    ]);
    adapter
        .submit(&key(), &request(DEFAULT_MODEL), SUBMISSION)
        .unwrap();
    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 3);
    assert!(sent.iter().all(|request| request.body == sent[0].body));
}

#[test]
fn status_follows_the_task_to_its_clip() {
    let adapter = clips(&["status-submitted", "status-processing", "status-succeeded"]);
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
        },
        "the clip without a watermark"
    );
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].method, Method::Get);
    assert_eq!(sent[0].url, format!("{API_URL}/tasks?task_ids={TASK}"));
    assert_eq!(
        sent[0].header("authorization"),
        Some(&*format!("Bearer {KEY}"))
    );
}

#[test]
fn tasks_that_end_without_a_clip_are_failures_of_their_kind() {
    let cases = [
        (
            "status-failed",
            ProviderFailureKind::ProviderDown,
            "internal error",
        ),
        (
            "status-failed-moderation",
            ProviderFailureKind::Declined,
            "risk control",
        ),
        (
            "status-unknown-task",
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
    let no_video = clips(&["status-succeeded-no-video"])
        .status(&key(), &handle())
        .unwrap_err();
    assert_eq!(no_video.kind, ProviderFailureKind::Unexpected);
}

#[test]
fn a_status_check_with_a_bad_key_fails_without_ending_the_task() {
    let failure = clips(&["submit-rejected"])
        .status(&key(), &handle())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
}

#[test]
fn a_handle_kling_did_not_give_is_never_sent() {
    let adapter = clips(&[]);
    let failure = adapter
        .status(&key(), &ClipHandle("1&task_ids=2".into()))
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn the_clip_downloads_without_the_key() {
    let adapter = clips(&["download-clip"]);
    let clip = adapter.download(&key(), VIDEO_URL).unwrap();
    assert_eq!(&clip.bytes[4..8], b"ftyp");
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].url, VIDEO_URL);
    assert_eq!(sent[0].header("authorization"), None);
}

#[test]
fn a_download_that_is_gone_or_not_a_clip_fails() {
    let gone = clips(&["download-expired"])
        .download(&key(), VIDEO_URL)
        .unwrap_err();
    assert_eq!(gone.kind, ProviderFailureKind::Unexpected);
    assert!(gone.detail.contains("no longer available"));
    let html = clips(&["status-succeeded"])
        .download(&key(), VIDEO_URL)
        .unwrap_err();
    assert!(html.detail.contains("not an MP4"));
    let plain = clips(&[])
        .download(&key(), "http://cdn.kling.example/x.mp4")
        .unwrap_err();
    assert_eq!(plain.kind, ProviderFailureKind::Unexpected);
}

#[test]
fn offline_is_unreachable() {
    let (sleeper, _) = recording_sleeper();
    let adapter = KlingClips::with_transport(Scripted::offline()).with_sleeper(sleeper);
    let failure = adapter.status(&key(), &handle()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
