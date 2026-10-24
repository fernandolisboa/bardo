//! Seedance 2.5 on BytePlus ModelArk (#194) against recorded responses
//! (see `fixtures/byteplus/README.md`). No test calls BytePlus.

mod common;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_ai::BytePlusTakes;
use bardo_ai::byteplus::{API_URL, MODEL, check_key, object_of, presign, take_body};
use bardo_ai::http::Method;
use bardo_domain::{
    ApiKey, BytePlusCredentials, ClipHandle, KeyCheckOutcome, Metered, Provider,
    ProviderFailureKind, StagedFile, TakeGenerator, TakeInput, TakeMode, TakeRequest, TakeStatus,
    TakeStep,
};
use common::{Scripted, fixture, recording_sleeper};

const API_KEY: &str = "0b6c2f6e-5a1d-4f3e-9d2a-7c8b9e0f1a2b";
const ACCESS_KEY: &str = "AKLTtestexample";
const SECRET_KEY: &str = "testSKtestSK";
const DRAFT_ID: &str = "cgt-20261023120000-d7k2p";
const FINAL_ID: &str = "cgt-20261024090000-f1n4l";
const OBJECT: &str = "bardo/takes/0f8e2c1a.mp4";
const HOST: &str = "bardo-takes.tos-ap-southeast-1.bytepluses.com";

/// 2026-10-23T12:00:00Z.
fn noon() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_756_800)
}

fn key_with(secret: &str) -> ApiKey {
    BytePlusCredentials::compose(API_KEY, ACCESS_KEY, secret, "bardo-takes", "ap-southeast-1")
        .unwrap()
}

fn key() -> ApiKey {
    key_with(SECRET_KEY)
}

fn takes(answers: &[&str]) -> BytePlusTakes<Scripted> {
    let answers = answers
        .iter()
        .map(|name| fixture("byteplus", name))
        .collect();
    let (sleeper, _) = recording_sleeper();
    BytePlusTakes::with_transport(Scripted::new(answers))
        .with_sleeper(sleeper)
        .with_clock(noon)
        .with_names(Arc::new(|| "0f8e2c1a".to_owned()))
}

fn link(object: &str) -> String {
    format!("https://{HOST}/{object}?X-Tos-Algorithm=TOS4-HMAC-SHA256&X-Tos-Signature=x")
}

fn draft() -> TakeRequest {
    TakeRequest {
        model: MODEL.into(),
        step: TakeStep::Draft,
        resolution: Some("480p".into()),
        prompt: "Replace the person in @Video 1 with the character in @Image 1.".into(),
        video: StagedFile(link(OBJECT)),
        look: StagedFile(link("bardo/takes/5d2e.jpg")),
        draft_task: None,
    }
}

fn final_of(draft_id: &str) -> TakeRequest {
    TakeRequest {
        step: TakeStep::Final,
        resolution: Some("1080p".into()),
        draft_task: Some(ClipHandle(draft_id.into())),
        ..draft()
    }
}

#[test]
fn seedance_takes_mesh_and_depth_and_its_final_finishes_the_draft_within_seven_days() {
    let models = TakeGenerator::models(&takes(&[]));
    assert_eq!(models.len(), 1);
    let model = &models[0];
    assert_eq!(model.id.provider(), Provider::BytePlus);
    assert_eq!(model.id.model(), MODEL);
    assert_eq!(model.modes, [TakeMode::Mesh, TakeMode::Depth]);
    assert_eq!(model.first_step(), TakeStep::Draft);
    assert!(model.finishes_draft);
    assert!(model.look_assets);
    assert_eq!(
        model.draft_deadline(noon()),
        Some(noon() + Duration::from_secs(7 * 24 * 3600))
    );
    assert_eq!(
        model.priced_model(TakeStep::Final),
        format!("{MODEL}/1080p")
    );
    assert!(model.fits(Duration::from_secs(4)));
    assert!(!model.fits(Duration::from_millis(30_001)));
}

#[test]
fn links_are_signed_as_the_byteplus_sdk_signs_them() {
    let key = ApiKey::parse(
        Provider::BytePlus,
        &[
            API_KEY,
            ACCESS_KEY,
            "testSK",
            "bardo-takes",
            "ap-southeast-1",
        ]
        .join("|"),
    )
    .unwrap();
    let credentials = BytePlusCredentials::of(&key).unwrap();
    let query = |seconds: u64, signature: &str| {
        format!(
            "https://{HOST}/{OBJECT}?X-Tos-Algorithm=TOS4-HMAC-SHA256\
             &X-Tos-Credential=AKLTtestexample%2F20261023%2Fap-southeast-1%2Ftos%2Frequest\
             &X-Tos-Date=20261023T120000Z&X-Tos-Expires={seconds}&X-Tos-SignedHeaders=host\
             &X-Tos-Signature={signature}"
        )
    };
    assert_eq!(
        presign(&credentials, Method::Get, OBJECT, 604_800, noon()),
        query(
            604_800,
            "09019cf9e056d28d86061642dc70461e0471af7fe657353abc6b9cfde3eae5b9"
        )
    );
    assert_eq!(
        presign(&credentials, Method::Put, OBJECT, 900, noon()),
        query(
            900,
            "0a716ccd2c71c2a8c798d296a6ccdcf9b924662cf836da6d5f058fece013d442"
        )
    );
    assert_eq!(
        presign(&credentials, Method::Delete, OBJECT, 900, noon()),
        query(
            900,
            "fe8e514e5beb508c403c3d1882fea2fba1c139b4b4b68811061bc52057a43d85"
        )
    );
}

#[test]
fn staging_uploads_to_the_bucket_and_returns_a_seven_day_link() {
    let takes = takes(&["tos-stored"]);
    let staged = takes
        .stage(
            &key(),
            TakeInput {
                bytes: b"control-video",
                mime: "video/mp4",
            },
        )
        .unwrap();
    let sent = takes.transport().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, Method::Put);
    assert!(
        sent[0]
            .url
            .starts_with(&format!("https://{HOST}/{OBJECT}?"))
    );
    assert!(sent[0].url.contains("X-Tos-Expires=900"));
    assert_eq!(sent[0].header("content-type"), Some("video/mp4"));
    assert_eq!(
        sent[0].header("authorization"),
        None,
        "no key in the upload"
    );
    assert_eq!(sent[0].raw_body, b"control-video");
    assert!(!sent[0].url.contains(SECRET_KEY));
    assert!(staged.0.starts_with(&format!("https://{HOST}/{OBJECT}?")));
    assert!(staged.0.contains("X-Tos-Expires=604800"));
    let key = key();
    let credentials = BytePlusCredentials::of(&key).unwrap();
    assert_eq!(object_of(&credentials, &staged.0).as_deref(), Some(OBJECT));
}

#[test]
fn a_bucket_that_refuses_the_upload_says_so() {
    for (answer, kind) in [
        ("tos-denied", ProviderFailureKind::NotAllowed),
        ("tos-signature", ProviderFailureKind::Rejected),
        ("tos-no-such-bucket", ProviderFailureKind::NotAllowed),
    ] {
        let failure = takes(&[answer])
            .stage(
                &key(),
                TakeInput {
                    bytes: b"x",
                    mime: "image/jpeg",
                },
            )
            .unwrap_err();
        assert_eq!(failure.kind, kind, "{answer}");
        assert!(
            failure.detail.starts_with("the bucket: "),
            "{}",
            failure.detail
        );
    }
}

#[test]
fn a_draft_is_a_480p_video_edit_without_sound_or_watermark() {
    let body = take_body(&draft());
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["draft"], true);
    assert_eq!(body["resolution"], "480p");
    assert_eq!(body["omni_reference_task_type"], "edit");
    assert_eq!(body["ratio"], "adaptive");
    assert_eq!(body["duration"], -1);
    assert_eq!(body["generate_audio"], false);
    assert_eq!(body["watermark"], false);
    assert!(body.get("seed").is_none(), "Seedance 2.5 takes no seed");
    let content = body["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["role"], "reference_video");
    assert_eq!(content[1]["video_url"]["url"], link(OBJECT));
    assert_eq!(content[2]["role"], "reference_image");
    assert_eq!(content[2]["image_url"]["url"], link("bardo/takes/5d2e.jpg"));
}

#[test]
fn a_final_names_only_its_draft_and_1080p() {
    let body = take_body(&final_of(DRAFT_ID));
    assert_eq!(
        body,
        serde_json::json!({
            "model": MODEL,
            "content": [{"type": "draft_task", "draft_task": {"id": DRAFT_ID}}],
            "resolution": "1080p",
            "watermark": false,
        })
    );
}

#[test]
fn a_look_from_the_asset_library_goes_as_its_uri() {
    let request = TakeRequest {
        look: StagedFile("asset://asset-20261020-portrait".into()),
        ..draft()
    };
    assert_eq!(
        take_body(&request)["content"][2]["image_url"]["url"],
        "asset://asset-20261020-portrait"
    );
}

#[test]
fn a_submission_sends_the_api_key_and_returns_the_task() {
    let takes = takes(&["submit-submitted"]);
    let handle = takes.submit(&key(), &draft(), "job-1").unwrap();
    assert_eq!(handle, ClipHandle(DRAFT_ID.into()));
    let sent = takes.transport().sent();
    assert_eq!(sent[0].url, format!("{API_URL}/contents/generations/tasks"));
    assert_eq!(
        sent[0].header("authorization"),
        Some(format!("Bearer {API_KEY}").as_str())
    );
    assert!(
        !String::from_utf8_lossy(&sent[0].raw_body).contains(SECRET_KEY),
        "the storage key never goes to ModelArk"
    );
    assert!(!takes.idempotent());
    assert!(takes.keeps_staged());
}

#[test]
fn refused_submissions_say_why() {
    for (answer, kind, said) in [
        (
            "submit-declined",
            ProviderFailureKind::Declined,
            "InputImageSensitiveContentDetected.PrivacyInformation: The request failed because the input image may contain real person.",
        ),
        (
            "submit-invalid",
            ProviderFailureKind::Unexpected,
            "InvalidParameter: ",
        ),
        (
            "submit-rejected",
            ProviderFailureKind::Rejected,
            "AuthenticationError",
        ),
        (
            "submit-overdue",
            ProviderFailureKind::LimitReached,
            "AccountOverdueError",
        ),
        (
            "submit-not-open",
            ProviderFailureKind::NotAllowed,
            "ModelNotOpen",
        ),
        (
            "submit-limit",
            ProviderFailureKind::LimitReached,
            "SetLimitExceeded",
        ),
    ] {
        let failure = takes(&[answer])
            .submit(&key(), &draft(), "job-1")
            .unwrap_err();
        assert_eq!(failure.kind, kind, "{answer}");
        assert!(
            failure.detail.contains(said),
            "{answer}: {}",
            failure.detail
        );
    }
}

#[test]
fn a_submission_is_never_retried_on_its_own() {
    let overloaded = bardo_ai::http::HttpResponse::new(503, "{}");
    let takes = BytePlusTakes::with_transport(Scripted::new(vec![overloaded])).with_clock(noon);
    let failure = takes.submit(&key(), &draft(), "job-1").unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
    assert_eq!(takes.transport().sent().len(), 1);
}

#[test]
fn a_finished_task_gives_its_video_tokens_and_seed() {
    let takes = takes(&["status-queued", "status-running", "status-succeeded"]);
    let handle = ClipHandle(DRAFT_ID.into());
    assert_eq!(takes.status(&key(), &handle).unwrap(), TakeStatus::Queued);
    assert_eq!(takes.status(&key(), &handle).unwrap(), TakeStatus::Running);
    let TakeStatus::Done(done) = takes.status(&key(), &handle).unwrap() else {
        panic!("not done");
    };
    assert!(done.video.starts_with("https://ark-content-generation"));
    assert_eq!(
        done.usage,
        Some(Metered {
            output_tokens: 384_300,
            ..Metered::default()
        })
    );
    assert_eq!(done.seed, Some(1_867_302_214));
    let sent = takes.transport().sent();
    assert_eq!(
        sent[0].url,
        format!("{API_URL}/contents/generations/tasks/{DRAFT_ID}")
    );
}

#[test]
fn tasks_that_end_without_a_take_say_why() {
    let handle = ClipHandle(DRAFT_ID.into());
    let failed = |answer: &str| match takes(&[answer]).status(&key(), &handle).unwrap() {
        TakeStatus::Failed(failure) => failure,
        other => panic!("{answer}: {other:?}"),
    };
    let refused = failed("status-failed-refused");
    assert_eq!(refused.kind, ProviderFailureKind::Declined);
    assert!(
        refused
            .detail
            .starts_with("OutputVideoSensitiveContentDetected: ")
    );
    assert_eq!(
        failed("status-failed-mismatch").kind,
        ProviderFailureKind::Unexpected
    );
    let down = failed("status-failed");
    assert_eq!(down.kind, ProviderFailureKind::ProviderDown);
    assert!(
        down.detail.contains("InternalServiceError"),
        "{}",
        down.detail
    );
    assert_eq!(
        failed("status-expired").kind,
        ProviderFailureKind::ProviderDown
    );
    assert_eq!(
        failed("status-unknown").kind,
        ProviderFailureKind::Unexpected
    );
    assert_eq!(
        takes(&["status-succeeded-no-video"])
            .status(&key(), &handle)
            .unwrap_err()
            .kind,
        ProviderFailureKind::Unexpected
    );
}

#[test]
fn a_lost_submission_is_found_in_the_task_list_once_and_only_once() {
    let since = noon();
    let found = takes(&["list-one"])
        .find_submitted(&key(), &draft(), since)
        .unwrap();
    assert_eq!(found, Some(ClipHandle(DRAFT_ID.into())));
    let takes_none = takes(&["list-none"]);
    assert_eq!(
        takes_none.find_submitted(&key(), &draft(), since).unwrap(),
        None
    );
    let sent = takes_none.transport().sent();
    assert!(sent[0].url.contains(&format!("filter.model={MODEL}")));
    let several = takes(&["list-several"])
        .find_submitted(&key(), &draft(), since)
        .unwrap_err();
    assert!(several.detail.contains("several"), "{}", several.detail);
    // A final is found by its draft.
    let later = SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_832_400);
    assert_eq!(
        takes(&["list-final"])
            .find_submitted(&key(), &final_of(DRAFT_ID), later)
            .unwrap(),
        Some(ClipHandle(FINAL_ID.into()))
    );
}

#[test]
fn a_take_downloads_without_a_key() {
    let takes = takes(&["download-take", "download-expired"]);
    let clip = takes
        .download(&key(), "https://ark.example/take.mp4")
        .unwrap();
    assert_eq!(&clip.bytes[4..8], b"ftyp");
    assert_eq!(takes.transport().sent()[0].header("authorization"), None);
    assert_eq!(
        takes
            .download(&key(), "https://ark.example/take.mp4")
            .unwrap_err()
            .kind,
        ProviderFailureKind::Unexpected
    );
}

#[test]
fn unstaging_deletes_the_file_and_a_file_already_gone_is_fine() {
    let takes = takes(&["tos-deleted", "tos-no-such-key", "tos-denied"]);
    let file = StagedFile(link(OBJECT));
    takes.unstage(&key(), &file).unwrap();
    takes.unstage(&key(), &file).unwrap();
    assert_eq!(
        takes.unstage(&key(), &file).unwrap_err().kind,
        ProviderFailureKind::NotAllowed
    );
    let sent = takes.transport().sent();
    assert_eq!(sent[0].method, Method::Delete);
    assert!(
        sent[0]
            .url
            .starts_with(&format!("https://{HOST}/{OBJECT}?"))
    );
    // Nothing outside Bardo's own folder of the named bucket is touched.
    for other in [
        format!("https://other-bucket.tos-ap-southeast-1.bytepluses.com/{OBJECT}"),
        format!("https://{HOST}/someone/else.mp4"),
    ] {
        assert_eq!(
            takes.unstage(&key(), &StagedFile(other)).unwrap_err().kind,
            ProviderFailureKind::NotAllowed
        );
    }
    assert_eq!(takes.transport().sent().len(), 3);
}

#[test]
fn the_key_check_asks_modelark_and_the_bucket_for_nothing_that_exists() {
    let answers = |names: &[&str]| {
        Scripted::new(names.iter().map(|name| fixture("byteplus", name)).collect())
    };
    let valid = answers(&["status-unknown", "tos-no-such-key"]);
    assert_eq!(
        check_key(&valid, &key(), noon()).outcome,
        KeyCheckOutcome::Valid
    );
    let sent = valid.sent();
    assert!(
        sent[0]
            .url
            .ends_with("/contents/generations/tasks/cgt-bardo-key-check")
    );
    assert!(
        sent[1]
            .url
            .starts_with(&format!("https://{HOST}/bardo/key-check?"))
    );
    for (names, outcome, part) in [
        (
            &["submit-rejected"][..],
            KeyCheckOutcome::Rejected,
            "ModelArk: ",
        ),
        (
            &["submit-overdue"][..],
            KeyCheckOutcome::LimitReached,
            "ModelArk: ",
        ),
        (
            &["status-unknown", "tos-signature"][..],
            KeyCheckOutcome::Rejected,
            "TOS: ",
        ),
        (
            &["status-unknown", "tos-denied"][..],
            KeyCheckOutcome::NotAllowed,
            "TOS: ",
        ),
        (
            &["status-unknown", "tos-no-such-bucket"][..],
            KeyCheckOutcome::NotAllowed,
            "TOS: ",
        ),
    ] {
        let check = check_key(&answers(names), &key(), noon());
        assert_eq!(check.outcome, outcome, "{names:?}");
        assert!(check.detail.unwrap().starts_with(part));
    }
    assert_eq!(
        check_key(&Scripted::offline(), &key(), noon()).outcome,
        KeyCheckOutcome::Unreachable
    );
}
