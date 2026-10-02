//! Google clips (Veo, Gemini Omni Flash) against recorded responses (see
//! `fixtures/google-clips/README.md`). No test calls Google.

mod common;

use bardo_ai::GoogleClips;
use bardo_ai::google_clips::{API_URL, DEFAULT_MODEL};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipRequest, ClipStatus,
    ImageFormat, Provider, ProviderFailureKind, StagedImage,
};
use base64::Engine as _;
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "AIzaSyBardoTestKey00000000000000000000";
const OPERATION: &str = "models/veo-3.1-fast-generate-preview/operations/8x3kq2v9zt1m";
const INTERACTION: &str = "interactions/v1_ChdHb29nbGVPbW5pQmFyZG9UZXN0MDAx";
const VIDEO_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/files/7k2m9q4x1t8v:download?alt=media";
const OMNI_720P: &str = "gemini-omni-1.1-flash/720p";
const FRAME: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2, 3];

fn key() -> ApiKey {
    ApiKey::parse(Provider::Gemini, KEY).unwrap()
}

fn clips(answers: &[&str]) -> GoogleClips<Scripted> {
    let answers = answers
        .iter()
        .map(|name| fixture("google-clips", name))
        .collect();
    let (sleeper, _) = recording_sleeper();
    GoogleClips::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

fn request(model: &str, seconds: u32) -> ClipRequest {
    ClipRequest {
        model: model.into(),
        prompt: "Slow push in on the launch pad, steam rising.".into(),
        image: StagedImage("inline".into()),
        first_frame: ClipImage {
            bytes: FRAME.to_vec(),
            format: ImageFormat::Png,
        },
        seconds,
    }
}

fn frame_base64() -> String {
    base64::engine::general_purpose::STANDARD.encode(FRAME)
}

#[test]
fn google_offers_veo_and_omni_at_each_resolution_with_their_lengths() {
    let adapter = clips(&[]);
    assert_eq!(adapter.provider(), Provider::Gemini);
    let models = adapter.models();
    assert_eq!(models[0].id.model(), DEFAULT_MODEL);
    assert_eq!(models[0].name, "Veo 3.1 Fast 720p");
    let durations = |id: &str| {
        models
            .iter()
            .find(|model| model.id.model() == id)
            .unwrap_or_else(|| panic!("{id} is offered"))
            .durations
            .clone()
    };
    for veo in ["veo-3.1", "veo-3.1-fast", "veo-3.1-lite"] {
        assert_eq!(
            durations(&format!("{veo}-generate-preview/720p")),
            ClipDurations::Choices(vec![4, 6, 8])
        );
        assert_eq!(
            durations(&format!("{veo}-generate-preview/1080p")),
            ClipDurations::Choices(vec![8]),
            "1080p is 8 seconds only"
        );
    }
    for omni in [OMNI_720P, "gemini-omni-1.1-flash/1080p"] {
        assert_eq!(durations(omni), ClipDurations::Range { min: 3, max: 10 });
    }
    assert!(
        models
            .iter()
            .all(|model| model.id.provider() == Provider::Gemini)
    );
}

#[test]
fn nothing_is_staged_ahead_of_the_submission() {
    let adapter = clips(&[]);
    let staged = adapter
        .stage_image(
            &key(),
            &ClipImage {
                bytes: FRAME.to_vec(),
                format: ImageFormat::Png,
            },
        )
        .unwrap();
    assert_eq!(staged, StagedImage("inline".into()));
    assert!(adapter.transport().sent().is_empty(), "no call");
}

#[test]
fn a_veo_submission_carries_the_image_inline_and_returns_the_operation() {
    let adapter = clips(&["veo-submit"]);
    let submission = adapter
        .submit(&key(), &request(DEFAULT_MODEL, 6), "job-0-1")
        .unwrap();
    assert_eq!(submission.handle, ClipHandle(OPERATION.into()));
    assert_eq!(submission.quote, None, "the rate table prices it");

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let submit = &sent[0];
    assert_eq!(submit.method, Method::Post);
    assert_eq!(
        submit.url,
        format!("{API_URL}/models/veo-3.1-fast-generate-preview:predictLongRunning")
    );
    assert_eq!(submit.header("x-goog-api-key"), Some(KEY));
    assert!(!submit.url.contains(KEY), "the key stays out of the URL");
    let instance = &submit.body["instances"][0];
    assert_eq!(
        instance["prompt"],
        "Slow push in on the launch pad, steam rising."
    );
    assert_eq!(instance["image"]["inlineData"]["mimeType"], "image/png");
    assert_eq!(instance["image"]["inlineData"]["data"], frame_base64());
    let parameters = &submit.body["parameters"];
    assert_eq!(parameters["aspectRatio"], "16:9");
    assert_eq!(parameters["resolution"], "720p");
    assert_eq!(parameters["durationSeconds"], 6);
}

#[test]
fn each_veo_model_and_resolution_goes_to_its_own_endpoint() {
    let adapter = clips(&["veo-submit", "veo-submit"]);
    adapter
        .submit(&key(), &request("veo-3.1-generate-preview/1080p", 8), "s")
        .unwrap();
    adapter
        .submit(
            &key(),
            &request("veo-3.1-lite-generate-preview/720p", 4),
            "s",
        )
        .unwrap();
    let sent = adapter.transport().sent();
    assert_eq!(
        sent[0].url,
        format!("{API_URL}/models/veo-3.1-generate-preview:predictLongRunning")
    );
    assert_eq!(sent[0].body["parameters"]["resolution"], "1080p");
    assert_eq!(
        sent[1].url,
        format!("{API_URL}/models/veo-3.1-lite-generate-preview:predictLongRunning")
    );
    assert_eq!(sent[1].body["parameters"]["durationSeconds"], 4);
}

#[test]
fn an_omni_submission_runs_in_the_background_as_one_shot_from_the_image() {
    let adapter = clips(&["omni-submit"]);
    let submission = adapter
        .submit(&key(), &request(OMNI_720P, 7), "job-0-1")
        .unwrap();
    assert_eq!(submission.handle, ClipHandle(INTERACTION.into()));

    let submit = &adapter.transport().sent()[0];
    assert_eq!(submit.url, format!("{API_URL}/interactions"));
    assert_eq!(submit.header("x-goog-api-key"), Some(KEY));
    assert_eq!(submit.body["model"], "gemini-omni-1.1-flash");
    assert_eq!(submit.body["background"], true);
    let input = submit.body["input"].as_array().unwrap();
    assert_eq!(input[0]["type"], "image");
    assert_eq!(input[0]["mime_type"], "image/png");
    assert_eq!(input[0]["data"], frame_base64());
    let text = input[1]["text"].as_str().unwrap();
    assert!(text.contains("first frame") && text.contains("no scene cuts"));
    assert!(text.ends_with("Slow push in on the launch pad, steam rising."));
    let format = &submit.body["response_format"];
    assert_eq!(format["type"], "video");
    assert_eq!(format["aspect_ratio"], "16:9");
    assert_eq!(format["resolution"], "720p");
    assert_eq!(format["duration"], "7s");
}

#[test]
fn a_long_prompt_is_cut_where_veo_stops_reading() {
    let adapter = clips(&["veo-submit"]);
    let mut long = request(DEFAULT_MODEL, 8);
    long.prompt = "é".repeat(3_000);
    adapter.submit(&key(), &long, "s").unwrap();
    let prompt = adapter.transport().sent()[0].body["instances"][0]["prompt"]
        .as_str()
        .unwrap()
        .chars()
        .count();
    assert_eq!(prompt, 2_500);
}

#[test]
fn a_model_google_does_not_offer_is_not_sent() {
    let adapter = clips(&[]);
    let failure = adapter
        .submit(
            &key(),
            &request("kling-video/v3.0/std/image-to-video", 5),
            "s",
        )
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::NotAllowed);
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn refused_submissions_say_why() {
    let cases = [
        (
            "veo-submit-rejected",
            DEFAULT_MODEL,
            ProviderFailureKind::Rejected,
            "API key not valid",
        ),
        (
            "omni-submit-rejected",
            OMNI_720P,
            ProviderFailureKind::Rejected,
            "API key not valid",
        ),
        (
            "veo-submit-model-not-found",
            DEFAULT_MODEL,
            ProviderFailureKind::NotAllowed,
            "is not found",
        ),
        (
            "veo-submit-invalid",
            DEFAULT_MODEL,
            ProviderFailureKind::Unexpected,
            "must be 8 seconds",
        ),
    ];
    for (name, model, kind, detail) in cases {
        let failure = clips(&[name])
            .submit(&key(), &request(model, 8), "s")
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
fn a_busy_or_throttled_api_is_retried_before_failing() {
    let adapter = clips(&[
        "veo-submit-overloaded",
        "veo-submit-rate-limited",
        "veo-submit",
    ]);
    let submission = adapter
        .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
        .unwrap();
    assert_eq!(submission.handle, ClipHandle(OPERATION.into()));
    assert_eq!(adapter.transport().sent().len(), 3);

    let throttled = clips(&["veo-submit-rate-limited"; 4])
        .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
        .unwrap_err();
    assert_eq!(throttled.kind, ProviderFailureKind::LimitReached);
}

#[test]
fn a_veo_operation_is_followed_to_its_clip() {
    let adapter = clips(&["veo-running", "veo-done"]);
    let handle = ClipHandle(OPERATION.into());
    assert_eq!(
        adapter.status(&key(), &handle).unwrap(),
        ClipStatus::Running
    );
    assert_eq!(
        adapter.status(&key(), &handle).unwrap(),
        ClipStatus::Done {
            video: VIDEO_URL.into()
        }
    );
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].method, Method::Get);
    assert_eq!(sent[0].url, format!("{API_URL}/{OPERATION}"));
    assert_eq!(sent[0].header("x-goog-api-key"), Some(KEY));
}

#[test]
fn an_omni_interaction_is_followed_to_its_clip() {
    let adapter = clips(&["omni-queued", "omni-in-progress", "omni-completed"]);
    let handle = ClipHandle(INTERACTION.into());
    assert_eq!(adapter.status(&key(), &handle).unwrap(), ClipStatus::Queued);
    assert_eq!(
        adapter.status(&key(), &handle).unwrap(),
        ClipStatus::Running
    );
    assert_eq!(
        adapter.status(&key(), &handle).unwrap(),
        ClipStatus::Done {
            video: INTERACTION.into()
        },
        "the clip is inline: the download reads the interaction again"
    );
    assert_eq!(
        adapter.transport().sent()[0].url,
        format!("{API_URL}/{INTERACTION}")
    );
}

#[test]
fn work_that_ends_without_a_clip_is_a_failure_of_its_kind() {
    let cases = [
        (
            "veo-filtered",
            OPERATION,
            ProviderFailureKind::Declined,
            "issue with the audio",
        ),
        (
            "veo-error",
            OPERATION,
            ProviderFailureKind::ProviderDown,
            "internal error",
        ),
        (
            "veo-done-empty",
            OPERATION,
            ProviderFailureKind::Unexpected,
            "without a clip",
        ),
        (
            "omni-completed-text",
            INTERACTION,
            ProviderFailureKind::Declined,
            "usage policies",
        ),
        (
            "omni-failed",
            INTERACTION,
            ProviderFailureKind::ProviderDown,
            "Please try again",
        ),
        (
            "omni-cancelled",
            INTERACTION,
            ProviderFailureKind::Unexpected,
            "cancelled",
        ),
        (
            "not-found",
            OPERATION,
            ProviderFailureKind::Unexpected,
            "does not know",
        ),
    ];
    for (name, handle, kind, detail) in cases {
        let status = clips(&[name])
            .status(&key(), &ClipHandle(handle.into()))
            .unwrap();
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
}

#[test]
fn a_status_check_with_a_bad_key_fails_without_ending_the_work() {
    let failure = clips(&["omni-submit-rejected"])
        .status(&key(), &ClipHandle(INTERACTION.into()))
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
}

#[test]
fn a_handle_that_could_leave_the_api_is_never_requested() {
    let adapter = clips(&[]);
    for handle in ["models/../../v1/files", "https://example.com/x", ""] {
        assert!(adapter.status(&key(), &ClipHandle(handle.into())).is_err());
    }
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn a_veo_clip_downloads_with_the_key_from_google_only() {
    let adapter = clips(&["download-clip", "download-clip"]);
    let clip = adapter.download(&key(), VIDEO_URL).unwrap();
    assert_eq!(&clip.bytes[4..8], b"ftyp");
    adapter
        .download(&key(), "https://storage.example.com/clip.mp4")
        .unwrap();
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].url, VIDEO_URL);
    assert_eq!(sent[0].header("x-goog-api-key"), Some(KEY));
    assert_eq!(sent[1].header("x-goog-api-key"), None, "not Google's API");
}

#[test]
fn an_omni_clip_is_read_from_its_interaction() {
    let adapter = clips(&["omni-completed"]);
    let clip = adapter.download(&key(), INTERACTION).unwrap();
    assert_eq!(clip.bytes, b"\0\0\0\x18ftypisomclip-bytes");
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].url, format!("{API_URL}/{INTERACTION}"));
    assert_eq!(sent[0].header("x-goog-api-key"), Some(KEY));
}

#[test]
fn a_download_that_is_gone_or_not_a_clip_fails() {
    let gone = clips(&["download-expired"])
        .download(&key(), VIDEO_URL)
        .unwrap_err();
    assert_eq!(gone.kind, ProviderFailureKind::Unexpected);
    assert!(gone.detail.contains("no longer available"));
    let json = clips(&["veo-done"])
        .download(&key(), VIDEO_URL)
        .unwrap_err();
    assert!(json.detail.contains("not an MP4"));
    let no_clip = clips(&["omni-in-progress"])
        .download(&key(), INTERACTION)
        .unwrap_err();
    assert!(no_clip.detail.contains("no clip"));
    let operation = clips(&[]).download(&key(), OPERATION).unwrap_err();
    assert_eq!(operation.kind, ProviderFailureKind::Unexpected);
}

#[test]
fn offline_is_unreachable() {
    let (sleeper, _) = recording_sleeper();
    let adapter = GoogleClips::with_transport(Scripted::offline()).with_sleeper(sleeper);
    let failure = adapter
        .status(&key(), &ClipHandle(OPERATION.into()))
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
