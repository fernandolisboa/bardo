//! Kling Motion Control takes (#243) against recorded responses (see
//! `fixtures/kling/README.md`). No test calls Kling.

mod common;

use bardo_ai::KlingTakes;
use bardo_ai::http::Method;
use bardo_ai::kling::API_URL;
use bardo_ai::kling_takes::{DRAFT_MODEL, TAKE_MODEL_720P, TAKE_MODEL_1080P, external_task_id};
use bardo_domain::{
    ApiKey, ClipHandle, Metered, Provider, ProviderFailureKind, StagedFile, TakeGenerator,
    TakeInput, TakeMode, TakeRequest, TakeStatus, TakeStep,
};
use base64::Engine as _;
use common::{Scripted, fixture, recording_sleeper};

const KEY: &str = "kling-test-key-0001-abcdef";
const TASK: &str = "893605946402811985";
const VIDEO_URL: &str = "https://cdn.kling.example/output/893605946402811985.mp4";
const SUBMISSION: &str = "57-1";
const STAGED: &str = "https://bardo-takes.tos-ap-southeast-1.bytepluses.com/bardo/takes/0f8e.mp4";
const LOOK: [u8; 6] = [0xff, 0xd8, 0xff, 0xe0, 1, 2];

fn key() -> ApiKey {
    ApiKey::parse(Provider::Kling, KEY).unwrap()
}

fn takes(answers: &[&str]) -> KlingTakes<Scripted> {
    let answers = answers.iter().map(|name| fixture("kling", name)).collect();
    let (sleeper, _) = recording_sleeper();
    KlingTakes::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

fn signed(signature: &str) -> String {
    format!("{STAGED}?X-Tos-Signature={signature}")
}

fn request(model: &str, step: TakeStep) -> TakeRequest {
    TakeRequest {
        model: model.into(),
        step,
        resolution: None,
        prompt: "The character in the image talks to the camera in a dim study.".into(),
        video: StagedFile(STAGED.into()),
        look: StagedFile("inline:5d2e8c1a".into()),
        draft_task: None,
        video_link: Some(signed("a1")),
        look_bytes: Some(LOOK.to_vec()),
    }
}

#[test]
fn kling_offers_3_0_motion_control_at_720p_and_1080p_with_drafts_on_2_6() {
    let models = TakeGenerator::models(&takes(&[]));
    let ids: Vec<_> = models.iter().map(|model| model.id.model()).collect();
    assert_eq!(ids, [TAKE_MODEL_720P, TAKE_MODEL_1080P]);
    for model in &models {
        assert_eq!(model.id.provider(), Provider::Kling);
        assert_eq!(model.modes, [TakeMode::Recording]);
        assert!(model.draft_optional(), "the final is a new take");
        assert_eq!(model.priced_model(TakeStep::Draft), DRAFT_MODEL);
        assert_eq!(model.priced_model(TakeStep::Test), DRAFT_MODEL);
        assert_eq!(model.priced_model(TakeStep::Final), model.id.model());
        assert!(!model.keeps_framing);
    }
    assert_eq!(models[0].final_tier.height, 720);
    assert_eq!(models[1].final_tier.height, 1080);
    let takes = takes(&[]);
    assert!(!takes.stages_video(), "the video is staged elsewhere");
    assert!(takes.inline_look());
    assert_eq!(
        takes.quote(&key(), &request(TAKE_MODEL_720P, TakeStep::Final)),
        None
    );
}

#[test]
fn nothing_is_staged_with_kling() {
    let failure = takes(&[])
        .stage(
            &key(),
            TakeInput {
                bytes: &LOOK,
                mime: "image/jpeg",
            },
        )
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::NotAllowed);
}

#[test]
fn a_final_sends_the_look_inline_and_the_video_by_its_link() {
    let adapter = takes(&["submit-submitted"]);
    let handle = adapter
        .submit(
            &key(),
            &request(TAKE_MODEL_1080P, TakeStep::Final),
            SUBMISSION,
        )
        .unwrap();
    assert_eq!(handle, ClipHandle(TASK.into()));

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 1);
    let submit = &sent[0];
    assert_eq!(submit.method, Method::Post);
    assert_eq!(submit.url, format!("{API_URL}/motion-control/kling-3.0"));
    assert_eq!(
        submit.header("authorization"),
        Some(&*format!("Bearer {KEY}"))
    );
    let contents = submit.body["contents"].as_array().unwrap();
    assert_eq!(contents[0]["type"], "prompt");
    assert_eq!(contents[1]["type"], "image");
    assert_eq!(
        contents[1]["url"],
        base64::engine::general_purpose::STANDARD.encode(LOOK),
        "raw base64, no data: prefix"
    );
    assert_eq!(contents[2]["type"], "video");
    assert_eq!(contents[2]["url"], signed("a1"));
    let settings = &submit.body["settings"];
    assert_eq!(settings["character_orientation"], "video");
    assert_eq!(
        settings["audio"], "off",
        "the take's voice is the only sound"
    );
    assert_eq!(settings["resolution"], "1080p");
    let options = &submit.body["options"];
    assert_eq!(options["watermark_info"]["enabled"], false);
    let external =
        external_task_id(&request(TAKE_MODEL_1080P, TakeStep::Final), SUBMISSION).unwrap();
    assert_eq!(options["external_task_id"], external.as_str());
}

#[test]
fn a_draft_is_made_with_2_6_at_720p() {
    let adapter = takes(&["submit-submitted"]);
    adapter
        .submit(
            &key(),
            &request(TAKE_MODEL_1080P, TakeStep::Draft),
            SUBMISSION,
        )
        .unwrap();
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].url, format!("{API_URL}/motion-control/kling-2.6"));
    assert_eq!(sent[0].body["settings"]["resolution"], "720p");
}

#[test]
fn the_custom_task_id_ignores_the_signature_but_follows_the_inputs() {
    let id = |request: &TakeRequest| external_task_id(request, SUBMISSION).unwrap();
    let base = request(TAKE_MODEL_720P, TakeStep::Final);
    let resigned = TakeRequest {
        video_link: Some(signed("b2")),
        ..base.clone()
    };
    assert_eq!(
        id(&base),
        id(&resigned),
        "a link signed again is the same take"
    );
    let encoded_again = TakeRequest {
        look_bytes: Some(vec![0xff, 0xd8, 9]),
        ..base.clone()
    };
    assert_eq!(
        id(&base),
        id(&encoded_again),
        "the look is named by its print"
    );
    let other_look = TakeRequest {
        look: StagedFile("inline:77aa0b3c".into()),
        ..base.clone()
    };
    assert_ne!(id(&base), id(&other_look));
    let other_video = TakeRequest {
        video: StagedFile(format!("{STAGED}x")),
        ..base.clone()
    };
    assert_ne!(id(&base), id(&other_video));
    let draft = TakeRequest {
        step: TakeStep::Draft,
        ..base.clone()
    };
    assert_ne!(id(&base), id(&draft));
    assert_ne!(
        id(&base),
        external_task_id(&base, "57-2").unwrap(),
        "another submission is another take"
    );
}

#[test]
fn a_resent_submission_finds_the_take_kling_already_has() {
    let adapter = takes(&["submit-duplicate", "lookup-found"]);
    let request = request(TAKE_MODEL_720P, TakeStep::Final);
    let handle = adapter.submit(&key(), &request, SUBMISSION).unwrap();
    assert_eq!(handle, ClipHandle(TASK.into()));
    let external = external_task_id(&request, SUBMISSION).unwrap();
    assert_eq!(
        adapter.transport().sent()[1].url,
        format!("{API_URL}/tasks?external_task_ids={external}")
    );
}

#[test]
fn refusals_say_why() {
    // A refusal that may be a repeated submission looks for the first one.
    let refused = |fixture: &[&str]| {
        takes(fixture)
            .submit(
                &key(),
                &request(TAKE_MODEL_720P, TakeStep::Final),
                SUBMISSION,
            )
            .unwrap_err()
            .kind
    };
    assert_eq!(
        refused(&["submit-declined", "lookup-empty"]),
        ProviderFailureKind::Declined
    );
    assert_eq!(
        refused(&["submit-model-not-allowed", "lookup-empty"]),
        ProviderFailureKind::NotAllowed
    );
    assert_eq!(refused(&["submit-rejected"]), ProviderFailureKind::Rejected);
}

#[test]
fn a_request_without_the_look_or_of_another_model_is_not_sent() {
    let adapter = takes(&[]);
    let without_look = TakeRequest {
        look_bytes: None,
        ..request(TAKE_MODEL_720P, TakeStep::Final)
    };
    assert!(adapter.submit(&key(), &without_look, SUBMISSION).is_err());
    let failure = adapter
        .submit(
            &key(),
            &request("kling-3.0/720p", TakeStep::Final),
            SUBMISSION,
        )
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::NotAllowed);
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn a_finished_take_is_billed_by_its_own_seconds_rounded_to_the_nearest() {
    let adapter = takes(&["status-processing", "take-status-succeeded"]);
    let handle = ClipHandle(TASK.into());
    assert_eq!(
        adapter.status(&key(), &handle).unwrap(),
        TakeStatus::Running
    );
    let TakeStatus::Done(done) = adapter.status(&key(), &handle).unwrap() else {
        panic!("not done");
    };
    assert_eq!(done.video, VIDEO_URL);
    assert_eq!(done.usage, Some(Metered::video_seconds(8)));
    assert_eq!(
        adapter.transport().sent()[1].url,
        format!("{API_URL}/tasks?task_ids={TASK}")
    );
}

#[test]
fn a_finished_take_with_its_length_as_text_is_read_too() {
    let TakeStatus::Done(done) = takes(&["status-succeeded"])
        .status(&key(), &ClipHandle(TASK.into()))
        .unwrap()
    else {
        panic!("not done");
    };
    assert_eq!(done.usage, Some(Metered::video_seconds(8)));
}

#[test]
fn a_failed_take_says_why() {
    let status = takes(&["status-failed-moderation"])
        .status(&key(), &ClipHandle(TASK.into()))
        .unwrap();
    let TakeStatus::Failed(failure) = status else {
        panic!("not failed");
    };
    assert_eq!(failure.kind, ProviderFailureKind::Declined);
}

#[test]
fn the_take_is_downloaded_without_the_key() {
    let adapter = takes(&["download-clip"]);
    let take = TakeGenerator::download(&adapter, &key(), VIDEO_URL).unwrap();
    assert!(!take.bytes.is_empty());
    assert_eq!(adapter.transport().sent()[0].header("authorization"), None);
}
