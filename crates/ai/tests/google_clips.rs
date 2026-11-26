//! Google clips (Gemini Omni Flash) against recorded responses (see
//! `fixtures/google-clips/README.md`). No test calls Google.

mod common;

use bardo_ai::GoogleClips;
use bardo_ai::google_clips::{API_URL, DEFAULT_MODEL, omni_body};
use bardo_ai::http::{HttpResponse, Method};
use bardo_domain::{
    ApiKey, AspectRatio, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipRequest,
    ClipStatus, ImageFormat, Provider, ProviderFailureKind, StagedImage,
};
use base64::Engine as _;
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "AIzaSyBardoTestKey00000000000000000000";
/// A Veo operation, as a job started before Google shut Veo down saved it.
const OPERATION: &str = "models/veo-3.1-fast-generate-preview/operations/8x3kq2v9zt1m";
const INTERACTION: &str = "interactions/v1_ChdHb29nbGVPbW5pQmFyZG9UZXN0MDAx";
const VIDEO_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/files/7k2m9q4x1t8v:download?alt=media";
const OMNI_1080P: &str = "gemini-omni-1.2-flash/1080p";
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
        aspect: AspectRatio::Landscape,
    }
}

fn frame_base64() -> String {
    base64::engine::general_purpose::STANDARD.encode(FRAME)
}

#[test]
fn google_offers_omni_flash_at_each_resolution_from_3_to_15_seconds() {
    let adapter = clips(&[]);
    assert_eq!(adapter.provider(), Provider::Gemini);
    let models = adapter.models();
    let offered: Vec<_> = models
        .iter()
        .map(|model| (model.id.model(), model.name.to_string()))
        .collect();
    assert_eq!(
        offered,
        [
            (
                "gemini-omni-1.2-flash/720p",
                "Gemini Omni 1.2 Flash 720p".to_owned()
            ),
            (
                "gemini-omni-1.2-flash/1080p",
                "Gemini Omni 1.2 Flash 1080p".to_owned()
            ),
        ],
        "no model Google has shut down"
    );
    assert_eq!(models[0].id.model(), DEFAULT_MODEL);
    for model in &models {
        assert_eq!(model.id.provider(), Provider::Gemini);
        assert_eq!(model.durations, ClipDurations::Range { min: 3, max: 15 });
    }
}

#[test]
fn every_model_google_shut_down_names_the_omni_flash_of_its_resolution() {
    let adapter = clips(&[]);
    let models = adapter.models();
    let retired = adapter.retired();
    let replaced: Vec<_> = retired
        .iter()
        .map(|gone| {
            assert_eq!(gone.id.provider(), Provider::Gemini);
            (
                gone.id.model(),
                gone.name.as_str(),
                gone.replacement.model(),
            )
        })
        .collect();
    let (hd, full_hd) = (DEFAULT_MODEL, OMNI_1080P);
    assert_eq!(
        replaced,
        [
            (
                "veo-3.1-fast-generate-preview/720p",
                "Veo 3.1 Fast 720p",
                hd
            ),
            (
                "veo-3.1-fast-generate-preview/1080p",
                "Veo 3.1 Fast 1080p",
                full_hd
            ),
            ("veo-3.1-generate-preview/720p", "Veo 3.1 720p", hd),
            ("veo-3.1-generate-preview/1080p", "Veo 3.1 1080p", full_hd),
            (
                "veo-3.1-lite-generate-preview/720p",
                "Veo 3.1 Lite 720p",
                hd
            ),
            (
                "veo-3.1-lite-generate-preview/1080p",
                "Veo 3.1 Lite 1080p",
                full_hd
            ),
            (
                "gemini-omni-1.1-flash/720p",
                "Gemini Omni 1.1 Flash 720p",
                hd
            ),
            (
                "gemini-omni-1.1-flash/1080p",
                "Gemini Omni 1.1 Flash 1080p",
                full_hd
            ),
        ]
    );
    for gone in &retired {
        let current = bardo_domain::current_clip_model(&models, &retired, &gone.id)
            .unwrap_or_else(|| panic!("{} has a replacement on offer", gone.id));
        assert_eq!(current.model.id, gone.replacement);
        assert_eq!(current.replaces, Some(gone));
    }
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
fn a_vertical_first_frame_asks_for_a_vertical_clip() {
    let vertical = ClipRequest {
        aspect: AspectRatio::Vertical,
        ..request(DEFAULT_MODEL, 6)
    };
    assert_eq!(
        omni_body(&vertical, "gemini-omni-1.2-flash", "720p")["response_format"]["aspect_ratio"],
        "9:16"
    );
}

#[test]
fn each_resolution_is_asked_of_the_same_model() {
    let adapter = clips(&["omni-submit", "omni-submit"]);
    adapter
        .submit(&key(), &request(DEFAULT_MODEL, 3), "s")
        .unwrap();
    adapter
        .submit(&key(), &request(OMNI_1080P, 15), "s")
        .unwrap();
    let sent = adapter.transport().sent();
    for submit in &sent {
        assert_eq!(submit.url, format!("{API_URL}/interactions"));
        assert_eq!(submit.body["model"], "gemini-omni-1.2-flash");
    }
    assert_eq!(sent[0].body["response_format"]["resolution"], "720p");
    assert_eq!(sent[0].body["response_format"]["duration"], "3s");
    assert_eq!(sent[1].body["response_format"]["resolution"], "1080p");
    assert_eq!(sent[1].body["response_format"]["duration"], "15s");
}

#[test]
fn an_omni_submission_runs_in_the_background_as_one_shot_from_the_image() {
    let adapter = clips(&["omni-submit"]);
    let submission = adapter
        .submit(&key(), &request(DEFAULT_MODEL, 7), "job-0-1")
        .unwrap();
    assert_eq!(submission.handle, ClipHandle(INTERACTION.into()));
    assert_eq!(submission.quote, None, "the rate table prices it");

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let submit = &sent[0];
    assert_eq!(submit.method, Method::Post);
    assert_eq!(submit.url, format!("{API_URL}/interactions"));
    assert_eq!(submit.header("x-goog-api-key"), Some(KEY));
    assert!(!submit.url.contains(KEY), "the key stays out of the URL");
    assert_eq!(submit.body["model"], "gemini-omni-1.2-flash");
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
fn a_long_prompt_is_cut() {
    let adapter = clips(&["omni-submit"]);
    let mut long = request(DEFAULT_MODEL, 8);
    long.prompt = "é".repeat(3_000);
    adapter.submit(&key(), &long, "s").unwrap();
    let text = adapter.transport().sent()[0].body["input"][1]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(text.chars().filter(|c| *c == 'é').count(), 2_500);
}

#[test]
fn a_model_google_no_longer_serves_or_never_offered_is_not_sent() {
    let adapter = clips(&[]);
    for model in [
        "veo-3.1-fast-generate-preview/720p",
        "gemini-omni-1.1-flash/720p",
        "gemini-omni-1.2-flash/4k",
        "kling-video/v3.0/std/image-to-video",
    ] {
        let failure = adapter.submit(&key(), &request(model, 5), "s").unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::NotAllowed, "{model}");
    }
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn refused_submissions_say_why() {
    let cases = [
        (
            "omni-submit-rejected",
            ProviderFailureKind::Rejected,
            "API key not valid",
        ),
        (
            "omni-submit-model-not-found",
            ProviderFailureKind::NotAllowed,
            "was not found",
        ),
        (
            "omni-submit-invalid",
            ProviderFailureKind::Unexpected,
            "'3s' to '15s'",
        ),
        (
            "omni-submit-no-credits",
            ProviderFailureKind::LimitReached,
            "credit balance",
        ),
    ];
    for (name, kind, detail) in cases {
        let failure = clips(&[name])
            .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
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
fn a_busy_or_throttled_api_is_told_once_and_left_to_the_job() {
    // The job has noted the send: waiting in here would keep that note
    // over a refusal that started nothing.
    let busy = clips(&["omni-submit-overloaded"]);
    let failure = busy
        .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
    assert_eq!(busy.transport().sent().len(), 1);

    let throttled = clips(&["omni-submit-rate-limited"]);
    let failure = throttled
        .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::LimitReached);
    assert_eq!(throttled.transport().sent().len(), 1);
}

#[test]
fn google_neither_recognizes_a_repeated_submission_nor_lists_what_it_started() {
    let adapter = clips(&[]);
    assert!(!adapter.dedupes_submissions(), "no idempotency key");
    assert!(
        !adapter.lists_submissions(),
        "no way to list operations or interactions"
    );
    assert_eq!(
        adapter.find_submitted(&key(), &request(DEFAULT_MODEL, 8)),
        Ok(None)
    );
    assert!(adapter.transport().sent().is_empty(), "nothing to ask");
}

#[test]
fn a_submission_whose_answer_is_lost_may_have_started_and_is_sent_once() {
    let (sleeper, _) = recording_sleeper();
    let timed_out =
        GoogleClips::with_transport(Scripted::losing_the_answer()).with_sleeper(sleeper);
    // A gateway that passed the request on, and an answer that does not
    // say what was started.
    let gateway = clips(&["submit-bad-gateway"]);
    let no_handle = clips(&["submit-no-handle"]);
    for adapter in [timed_out, gateway, no_handle] {
        let failure = adapter
            .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
            .unwrap_err();
        assert_eq!(
            failure.kind,
            ProviderFailureKind::AnswerLost,
            "{}",
            failure.detail
        );
        assert_eq!(adapter.transport().sent().len(), 1);
    }
}

/// The adapter over one answer that is not a recording.
fn answering(status: u16, body: &str) -> GoogleClips<Scripted> {
    let (sleeper, _) = recording_sleeper();
    GoogleClips::with_transport(Scripted::new(vec![HttpResponse::new(status, body)]))
        .with_sleeper(sleeper)
}

#[test]
fn only_googles_own_report_says_a_failed_submission_started_nothing() {
    // The Interactions API's report, wrapped in its one-element array.
    let own = answering(
        503,
        r#"[{"error":{"code":503,"message":"The model is overloaded.","status":"UNAVAILABLE"}}]"#,
    );
    let failure = own
        .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);

    // The same status from something in front of Google says nothing
    // about what Google did with the request.
    for body in [
        "<html><body><h1>503 Service Unavailable</h1></body></html>",
        r#"{"error":"upstream connect error or disconnect/reset before headers"}"#,
        r#"{"message":"Service Unavailable"}"#,
        "",
    ] {
        for status in [500, 503] {
            let adapter = answering(status, body);
            let failure = adapter
                .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
                .unwrap_err();
            assert_eq!(
                failure.kind,
                ProviderFailureKind::AnswerLost,
                "{status} {body}"
            );
            assert_eq!(adapter.transport().sent().len(), 1);
        }
    }
}

#[test]
fn a_submission_that_never_left_or_that_google_failed_started_nothing() {
    let (sleeper, _) = recording_sleeper();
    let offline = GoogleClips::with_transport(Scripted::offline()).with_sleeper(sleeper);
    let failure = offline
        .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);

    // Google failing on its side says so once; the queue goes again.
    let adapter = clips(&["submit-internal"]);
    let failure = adapter
        .submit(&key(), &request(DEFAULT_MODEL, 8), "s")
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
    assert_eq!(adapter.transport().sent().len(), 1);
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
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].method, Method::Get);
    assert_eq!(sent[0].url, format!("{API_URL}/{INTERACTION}"));
    assert_eq!(sent[0].header("x-goog-api-key"), Some(KEY));
}

#[test]
fn a_clip_google_serves_as_a_file_is_followed_to_its_address() {
    let adapter = clips(&["omni-completed-uri"]);
    assert_eq!(
        adapter
            .status(&key(), &ClipHandle(INTERACTION.into()))
            .unwrap(),
        ClipStatus::Done {
            video: VIDEO_URL.into()
        }
    );
}

#[test]
fn a_veo_operation_left_from_before_the_shutdown_is_lost_without_a_call() {
    let adapter = clips(&[]);
    let status = adapter
        .status(&key(), &ClipHandle(OPERATION.into()))
        .unwrap();
    // Google can no longer say whether it made, and charged, the clip.
    let ClipStatus::Lost(failure) = status else {
        panic!("{status:?}");
    };
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert!(failure.detail.contains("no longer serves"), "{failure:?}");
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn work_that_ends_without_a_clip_is_a_failure_of_its_kind() {
    let cases = [
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
        // Blocked by its code alone: not sent, and paid for, again.
        (
            "omni-failed-blocked",
            INTERACTION,
            ProviderFailureKind::Declined,
            "could not be generated",
        ),
        (
            "omni-cancelled",
            INTERACTION,
            ProviderFailureKind::Unexpected,
            "cancelled",
        ),
        (
            "omni-requires-continuation",
            INTERACTION,
            ProviderFailureKind::Unexpected,
            "stopped before the clip was done",
        ),
        (
            "not-found",
            INTERACTION,
            ProviderFailureKind::Unexpected,
            "does not know",
        ),
    ];
    for (name, handle, kind, detail) in cases {
        let status = clips(&[name])
            .status(&key(), &ClipHandle(handle.into()))
            .unwrap();
        let (failure, lost) = match status {
            ClipStatus::Failed(failure) => (failure, false),
            ClipStatus::Lost(failure) => (failure, true),
            other => panic!("{name}: {other:?}"),
        };
        // Only work the provider cannot account for may have been charged.
        assert_eq!(
            lost,
            matches!(
                name,
                "omni-cancelled" | "omni-requires-continuation" | "not-found"
            ),
            "{name}"
        );
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
fn a_clip_file_downloads_with_the_key_from_google_only() {
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
    let json = clips(&["omni-completed-uri"])
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
        .status(&key(), &ClipHandle(INTERACTION.into()))
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}
