//! The Higgsfield plan over its MCP server, against recorded responses
//! (see `fixtures/higgsfield-plan/README.md`). No test calls Higgsfield.

mod common;

use bardo_ai::HiggsfieldPlan;
use bardo_ai::higgsfield_plan::{AUTH_URL, DEFAULT_MODEL, MCP_URL, SCOPES};
use bardo_ai::http::{HttpResponse, Method, TransportError};
use bardo_ai::mcp::PROTOCOL_VERSION;
use bardo_domain::{
    ApiKey, AspectRatio, ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipRequest,
    ClipStatus, Credits, ImageFormat, LastFrame, PlanSignIn, Provider, ProviderFailureKind,
    SecretText, SignInFailureKind, StagedImage,
};
use common::{Scripted, fixture};

const TOKEN: &str = "hf-access-test-0001";
const MEDIA: &str = "2428803a-33a6-42e4-9627-6133b90b02ef";
const JOB: &str = "7c1e9a52-0f4d-4f7b-9d3e-5a6b2c8d1e40";
const SESSION: &str = "mcp-session-7f3a9c";
const VIDEO_URL: &str = "https://cdn.higgsfield.example/7c1e9a52-0f4d-4f7b-9d3e-5a6b2c8d1e40.mp4";
const IMAGE: [u8; 7] = [0x89, b'P', b'N', b'G', 1, 2, 3];
const PROMPT: &str = "Slow push in on the launch pad, steam rising.";

fn key() -> ApiKey {
    ApiKey::access_token(TOKEN).unwrap()
}

fn answers(names: &[&str]) -> Vec<HttpResponse> {
    names
        .iter()
        .map(|name| fixture("higgsfield-plan", name))
        .collect()
}

/// The adapter, with a session opened first (`initialize`, then the
/// `initialized` notice) and then `names` in order.
fn plan(names: &[&str]) -> HiggsfieldPlan<Scripted> {
    let mut all = answers(&["initialize", "initialized"]);
    all.extend(answers(names));
    HiggsfieldPlan::with_transport(Scripted::new(all))
}

/// The adapter without a session first: for the OAuth endpoints.
fn bare(names: &[&str]) -> HiggsfieldPlan<Scripted> {
    HiggsfieldPlan::with_transport(Scripted::new(answers(names)))
}

fn request(model: &str) -> ClipRequest {
    ClipRequest {
        model: model.into(),
        prompt: PROMPT.into(),
        image: StagedImage(MEDIA.into()),
        first_frame: ClipImage {
            bytes: IMAGE.to_vec(),
            format: ImageFormat::Png,
        },
        seconds: 8,
        aspect: AspectRatio::Vertical,
        last: None,
    }
}

fn tool_calls(adapter: &HiggsfieldPlan<Scripted>) -> Vec<(String, serde_json::Value)> {
    adapter
        .transport()
        .sent()
        .into_iter()
        .filter(|sent| sent.body["method"] == "tools/call")
        .map(|sent| {
            (
                sent.body["params"]["name"].as_str().unwrap().to_owned(),
                sent.body["params"]["arguments"].clone(),
            )
        })
        .collect()
}

#[test]
fn the_catalog_starts_with_kling_3_standard_and_names_the_plan() {
    let adapter = plan(&[]);
    assert_eq!(ClipGenerator::provider(&adapter), Provider::HiggsfieldPlan);
    let models = adapter.models();
    assert_eq!(models[0].id.model(), DEFAULT_MODEL);
    assert_eq!(models[0].name, "Kling 3.0 Standard");
    assert_eq!(
        models[0].durations,
        ClipDurations::Range { min: 3, max: 15 }
    );
    assert_eq!(models.len(), 6);
    assert!(
        models
            .iter()
            .all(|model| model.id.provider() == Provider::HiggsfieldPlan)
    );
}

#[test]
fn a_session_opens_once_and_every_request_carries_its_id_and_version() {
    let adapter = plan(&["balance", "balance-plus"]);
    let first = adapter.account(TOKEN).unwrap();
    assert_eq!(first.plan, "free");
    assert_eq!(first.credits, Credits::from_hundredths(1_000));
    let second = adapter.account(TOKEN).unwrap();
    assert_eq!(second.plan, "plus");
    assert_eq!(second.credits.to_string(), "982.5");

    let sent = adapter.transport().sent();
    assert_eq!(sent.len(), 4, "initialize, initialized, then two calls");
    assert!(sent.iter().all(|request| request.url == MCP_URL));
    assert!(sent.iter().all(|request| request.method == Method::Post));
    assert!(
        sent.iter()
            .all(|request| request.header("authorization") == Some(&*format!("Bearer {TOKEN}")))
    );
    assert!(sent.iter().all(|request| {
        request
            .header("accept")
            .is_some_and(|accept| accept.contains("text/event-stream"))
    }));
    assert_eq!(sent[0].body["method"], "initialize");
    assert_eq!(sent[0].body["params"]["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(sent[0].header("mcp-session-id"), None);
    assert_eq!(sent[1].body["method"], "notifications/initialized");
    assert!(sent[1].body.get("id").is_none(), "a notice has no id");
    for later in &sent[1..] {
        assert_eq!(later.header("mcp-session-id"), Some(SESSION));
        assert_eq!(later.header("mcp-protocol-version"), Some(PROTOCOL_VERSION));
    }
    assert_eq!(sent[2].body["params"]["name"], "balance");
}

#[test]
fn a_plain_json_answer_works_like_an_event_stream() {
    let all = answers(&["initialize-json", "initialized", "balance"]);
    let adapter = HiggsfieldPlan::with_transport(Scripted::new(all));
    assert_eq!(adapter.account(TOKEN).unwrap().plan, "free");
}

#[test]
fn a_dropped_session_is_opened_again_and_the_call_sent_once_more() {
    let adapter = plan(&["session-gone", "initialize", "initialized", "balance"]);
    assert_eq!(adapter.account(TOKEN).unwrap().plan, "free");
    let methods: Vec<_> = adapter
        .transport()
        .sent()
        .iter()
        .map(|sent| sent.body["method"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "notifications/initialized",
            "tools/call",
            "initialize",
            "notifications/initialized",
            "tools/call"
        ]
    );
}

#[test]
fn a_refused_token_is_a_refused_sign_in_and_drops_the_session() {
    let adapter = plan(&["unauthorized", "initialize", "initialized", "balance"]);
    let failure = adapter.account(TOKEN).unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Refused);
    // A renewed token opens a new session.
    assert_eq!(adapter.account("hf-access-test-0002").unwrap().plan, "free");
    let sent = adapter.transport().sent();
    assert_eq!(sent[3].body["method"], "initialize");
    assert_eq!(
        sent[3].header("authorization"),
        Some("Bearer hf-access-test-0002")
    );
}

#[test]
fn registration_asks_for_a_public_client_that_returns_to_the_loopback() {
    let adapter = bare(&["register"]);
    let client = adapter.register("http://127.0.0.1:53682/callback").unwrap();
    assert_eq!(client, "bardoTestClient1");
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].url, format!("{AUTH_URL}/register"));
    let body = &sent[0].body;
    assert_eq!(body["redirect_uris"][0], "http://127.0.0.1:53682/callback");
    assert_eq!(body["token_endpoint_auth_method"], "none");
    assert_eq!(body["grant_types"][1], "refresh_token");
    assert_eq!(body["scope"], SCOPES);
}

#[test]
fn the_consent_address_carries_pkce_state_and_the_resource() {
    let adapter = bare(&[]);
    let consent = adapter.consent_request("bardoTestClient1", "http://127.0.0.1:53682/callback");
    let (address, query) = consent.url.split_once('?').unwrap();
    assert_eq!(address, format!("{AUTH_URL}/authorize"));
    let fields: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    };
    assert_eq!(field("response_type"), Some("code"));
    assert_eq!(field("client_id"), Some("bardoTestClient1"));
    assert_eq!(
        field("redirect_uri"),
        Some("http://127.0.0.1:53682/callback")
    );
    assert_eq!(field("scope"), Some(SCOPES));
    assert_eq!(field("code_challenge_method"), Some("S256"));
    assert_eq!(field("resource"), Some(MCP_URL));
    assert_eq!(field("state"), Some(consent.state.expose()));
    assert_eq!(field("code_challenge").unwrap().len(), 43);
    assert!(consent.verifier.expose().len() >= 43);
    let again = adapter.consent_request("bardoTestClient1", "http://127.0.0.1:53682/callback");
    assert_ne!(again.state, consent.state);
    assert_ne!(again.verifier, consent.verifier);
}

#[test]
fn the_code_and_the_refresh_token_are_traded_at_the_token_endpoint() {
    let adapter = bare(&["token", "token-refreshed"]);
    let grant = adapter
        .exchange(
            "bardoTestClient1",
            &SecretText::new("code-0001"),
            &SecretText::new("verifier-0001"),
            "http://127.0.0.1:53682/callback",
        )
        .unwrap();
    assert_eq!(grant.access_token.expose(), "hf-access-test-0001");
    assert_eq!(
        grant.refresh_token.as_ref().map(SecretText::expose),
        Some("hf-refresh-test-0001")
    );
    let renewed = adapter
        .refresh("bardoTestClient1", "hf-refresh-test-0001")
        .unwrap();
    assert_eq!(renewed.access_token.expose(), "hf-access-test-0002");
    assert_eq!(renewed.refresh_token, None, "the old one is kept");

    let sent = adapter.transport().sent();
    assert!(sent.iter().all(|s| s.url == format!("{AUTH_URL}/token")));
    let form = |index: usize| -> Vec<(String, String)> {
        form_urlencoded::parse(&sent[index].raw_body)
            .into_owned()
            .collect()
    };
    let exchange = form(0);
    for expected in [
        ("grant_type", "authorization_code"),
        ("code", "code-0001"),
        ("code_verifier", "verifier-0001"),
        ("client_id", "bardoTestClient1"),
        ("redirect_uri", "http://127.0.0.1:53682/callback"),
        ("resource", MCP_URL),
    ] {
        assert!(
            exchange.contains(&(expected.0.to_owned(), expected.1.to_owned())),
            "{expected:?}"
        );
    }
    let refresh = form(1);
    assert!(refresh.contains(&("grant_type".into(), "refresh_token".into())));
    assert!(refresh.contains(&("refresh_token".into(), "hf-refresh-test-0001".into())));
}

#[test]
fn a_refused_refresh_says_to_sign_in_again() {
    let failure = bare(&["token-invalid-grant"])
        .refresh("bardoTestClient1", "hf-refresh-test-0001")
        .unwrap_err();
    assert_eq!(failure.kind, SignInFailureKind::Refused);
    let offline = HiggsfieldPlan::with_transport(Scripted::offline())
        .refresh("bardoTestClient1", "x")
        .unwrap_err();
    assert_eq!(offline.kind, SignInFailureKind::Unreachable);
}

#[test]
fn the_image_is_uploaded_to_the_presigned_address_and_confirmed() {
    let adapter = plan(&["media-upload", "put-ok", "media-confirm"]);
    let staged = adapter
        .stage_image(
            &key(),
            &ClipImage {
                bytes: IMAGE.to_vec(),
                format: ImageFormat::Png,
            },
        )
        .unwrap();
    assert_eq!(staged, StagedImage(MEDIA.into()));
    let sent = adapter.transport().sent();
    let calls = tool_calls(&adapter);
    assert_eq!(calls[0].0, "media_upload");
    assert_eq!(calls[0].1["filename"], "scene.png");
    assert_eq!(calls[0].1["content_type"], "image/png");
    let put = &sent[3];
    assert_eq!(put.method, Method::Put);
    assert!(put.url.starts_with("https://upload.higgsfield.ai/"));
    assert_eq!(put.header("content-type"), Some("image/png"));
    assert_eq!(put.header("if-none-match"), Some("*"));
    assert_eq!(put.header("authorization"), None, "the store gets no token");
    assert_eq!(put.raw_body, IMAGE);
    assert_eq!(calls[1].0, "media_confirm");
    assert_eq!(calls[1].1["media_id"], MEDIA);
    assert_eq!(calls[1].1["type"], "image");
}

#[test]
fn a_refused_upload_fails_without_confirming() {
    let adapter = plan(&["media-upload", "put-refused"]);
    let failure = adapter
        .stage_image(
            &key(),
            &ClipImage {
                bytes: IMAGE.to_vec(),
                format: ImageFormat::Png,
            },
        )
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
    assert_eq!(tool_calls(&adapter).len(), 1);
}

#[test]
fn a_submission_starts_one_job_with_credits_only() {
    let adapter = plan(&["submitted"]);
    assert!(!adapter.dedupes_submissions(), "no idempotency key");
    assert!(
        adapter.lists_submissions(),
        "the library shows what started"
    );
    let submission = adapter
        .submit(&key(), &request(DEFAULT_MODEL), "41-2-1")
        .unwrap();
    assert_eq!(submission.handle, ClipHandle(JOB.into()));
    assert_eq!(submission.quote, None, "the rate table prices it");

    let calls = tool_calls(&adapter);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "generate_video_batch");
    let params = &calls[0].1["requests"][0]["params"];
    assert_eq!(calls[0].1["requests"][0]["index"], 0);
    assert_eq!(calls[0].1["requests"].as_array().unwrap().len(), 1);
    assert_eq!(params["model"], "kling3_0");
    assert_eq!(params["mode"], "std");
    assert_eq!(params["sound"], "off", "the narration is the soundtrack");
    assert_eq!(params["prompt"], PROMPT);
    assert_eq!(params["duration"], 8);
    assert_eq!(params["aspect_ratio"], "9:16");
    assert_eq!(params["medias"][0]["role"], "start_image");
    assert_eq!(params["medias"][0]["value"], MEDIA);
    assert_eq!(
        params["medias"].as_array().unwrap().len(),
        1,
        "no end image"
    );
    assert_eq!(params["use_unlim"], false, "credits only, never asked");
}

#[test]
fn the_credit_check_prices_the_same_request_and_starts_nothing() {
    let adapter = plan(&["estimate", "balance-plus", "submitted"]);
    adapter
        .check_submit(&key(), &request(DEFAULT_MODEL))
        .unwrap();
    adapter
        .submit(&key(), &request(DEFAULT_MODEL), "41-2-1")
        .unwrap();
    let calls = tool_calls(&adapter);
    let names: Vec<_> = calls.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["estimate_video_cost", "balance", "generate_video_batch"]
    );
    assert_eq!(
        calls[0].1["params"], calls[2].1["requests"][0]["params"],
        "the estimate prices the same"
    );
}

#[test]
fn a_plan_without_enough_credits_is_refused_before_starting() {
    let adapter = plan(&["estimate", "balance"]);
    let mut costly = request(DEFAULT_MODEL);
    costly.seconds = 15;
    // The estimate says 15 credits and the free plan has 10.
    let failure = adapter.check_submit(&key(), &costly).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::LimitReached);
    assert!(failure.detail.contains("needs 15"), "{}", failure.detail);
    assert!(failure.detail.contains("has 10"), "{}", failure.detail);
}

#[test]
fn a_lost_submission_is_found_by_its_first_frame_on_a_later_page() {
    let adapter = plan(&["generations-page-1", "generations-with-job"]);
    let found = adapter
        .find_submitted(&key(), &request(DEFAULT_MODEL))
        .unwrap();
    assert_eq!(found, Some(ClipHandle(JOB.into())));
    let calls = tool_calls(&adapter);
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|(name, _)| name == "show_generations"));
    assert_eq!(calls[0].1["only_completed"], false);
    assert_eq!(calls[0].1.get("cursor"), None);
    assert_eq!(calls[1].1["cursor"], "page-2");
}

#[test]
fn an_older_clip_of_the_same_scene_is_not_taken_for_a_lost_one() {
    // Same model and prompt, another first frame: an earlier submission.
    for page in ["generations-same-prompt", "generations-empty"] {
        let found = plan(&[page])
            .find_submitted(&key(), &request(DEFAULT_MODEL))
            .unwrap();
        assert_eq!(found, None, "{page}");
    }
}

#[test]
fn a_cursor_that_does_not_move_ends_the_lookup() {
    let adapter = plan(&["generations-page-1", "generations-page-1"]);
    let found = adapter
        .find_submitted(&key(), &request(DEFAULT_MODEL))
        .unwrap();
    assert_eq!(found, None);
    assert_eq!(tool_calls(&adapter).len(), 2);
}

#[test]
fn a_failed_lookup_is_a_failure_not_a_missing_clip() {
    let failure = plan(&["rate-limited"])
        .find_submitted(&key(), &request(DEFAULT_MODEL))
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::ProviderDown);
}

/// The adapter with a session open, then a lost or cut answer.
fn losing(last: Result<HttpResponse, TransportError>) -> HiggsfieldPlan<Scripted> {
    let mut all: Vec<_> = answers(&["initialize", "initialized"])
        .into_iter()
        .map(Ok)
        .collect();
    all.push(last);
    HiggsfieldPlan::with_transport(Scripted::with_answers(all))
}

#[test]
fn an_answer_lost_after_sending_may_have_started_a_clip() {
    let timed_out = losing(Err(TransportError::unanswered(
        "timed out reading the answer",
    )));
    let cut_off = losing(Ok(fixture("higgsfield-plan", "answer-cut-off")));
    let no_id = plan(&["submitted-no-id"]);
    for adapter in [timed_out, cut_off, no_id] {
        let failure = adapter
            .submit(&key(), &request(DEFAULT_MODEL), "41-2-1")
            .unwrap_err();
        assert_eq!(
            failure.kind,
            ProviderFailureKind::AnswerLost,
            "{}",
            failure.detail
        );
    }
}

#[test]
fn a_call_never_sent_or_refused_started_nothing() {
    let offline = HiggsfieldPlan::with_transport(Scripted::offline());
    let unsent = offline
        .submit(&key(), &request(DEFAULT_MODEL), "41-2-1")
        .unwrap_err();
    assert_eq!(unsent.kind, ProviderFailureKind::ProviderDown);
    assert!(tool_calls(&offline).is_empty());

    let busy = plan(&["rate-limited"])
        .submit(&key(), &request(DEFAULT_MODEL), "41-2-1")
        .unwrap_err();
    assert_eq!(busy.kind, ProviderFailureKind::ProviderDown);
    let no_credits = plan(&["submit-no-credits"])
        .submit(&key(), &request(DEFAULT_MODEL), "41-2-1")
        .unwrap_err();
    assert_eq!(no_credits.kind, ProviderFailureKind::LimitReached);
    let unknown = plan(&["submit-unknown-model"])
        .submit(&key(), &request(DEFAULT_MODEL), "41-2-1")
        .unwrap_err();
    assert_eq!(unknown.kind, ProviderFailureKind::NotAllowed);
    let refused = plan(&["unauthorized"])
        .submit(&key(), &request(DEFAULT_MODEL), "41-2-1")
        .unwrap_err();
    assert_eq!(refused.kind, ProviderFailureKind::Rejected);
}

#[test]
fn a_model_outside_the_catalog_or_an_image_not_uploaded_is_never_sent() {
    let adapter = plan(&[]);
    let other = adapter
        .submit(&key(), &request("kling-video/v3.0/std/image-to-video"), "1")
        .unwrap_err();
    assert_eq!(other.kind, ProviderFailureKind::NotAllowed);
    let mut inline = request(DEFAULT_MODEL);
    inline.image = StagedImage("inline".into());
    assert!(adapter.submit(&key(), &inline, "1").is_err());
    assert!(adapter.check_submit(&key(), &inline).is_err());
    assert_eq!(adapter.find_submitted(&key(), &inline).unwrap(), None);
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn a_status_this_adapter_does_not_know_follows_whether_the_job_is_over() {
    let adapter = plan(&["wait-unknown-over", "wait-unknown-going"]);
    let handle = ClipHandle(JOB.into());
    match adapter.status(&key(), &handle).unwrap() {
        // Over, and not said to have made nothing: it may have been
        // charged.
        ClipStatus::Lost(failure) => {
            assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
            assert!(failure.detail.contains("expired"), "{}", failure.detail);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        adapter.status(&key(), &handle).unwrap(),
        ClipStatus::Running
    );
}

#[test]
fn the_status_reads_every_state_without_waiting() {
    let adapter = plan(&[
        "wait-queued",
        "wait-running",
        "wait-completed",
        "wait-failed",
        "wait-nsfw",
        "wait-lookup-failed",
    ]);
    let handle = ClipHandle(JOB.into());
    assert_eq!(adapter.status(&key(), &handle).unwrap(), ClipStatus::Queued);
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
    let failed = |status: ClipStatus| match status {
        ClipStatus::Failed(failure) => failure,
        other => panic!("{other:?}"),
    };
    let broke = failed(adapter.status(&key(), &handle).unwrap());
    assert_eq!(broke.kind, ProviderFailureKind::ProviderDown);
    assert!(broke.detail.contains("refunded"), "{}", broke.detail);
    let declined = failed(adapter.status(&key(), &handle).unwrap());
    assert_eq!(declined.kind, ProviderFailureKind::Declined);
    // A job the plan cannot find may have spent its credits.
    let ClipStatus::Lost(unknown) = adapter.status(&key(), &handle).unwrap() else {
        panic!("a job the plan does not know is lost, not failed");
    };
    assert_eq!(unknown.kind, ProviderFailureKind::Unexpected);

    let calls = tool_calls(&adapter);
    assert!(calls.iter().all(|(name, _)| name == "jobs_wait"));
    assert_eq!(calls[0].1["timeout_seconds"], 0);
    assert_eq!(calls[0].1["jobs"][0]["job_id"], JOB);
}

#[test]
fn a_handle_that_is_not_a_job_id_is_never_sent() {
    let adapter = plan(&[]);
    assert!(
        adapter
            .status(&key(), &ClipHandle("../balance".into()))
            .is_err()
    );
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn the_clip_downloads_without_the_token() {
    let adapter = bare(&["download-clip"]);
    let clip = adapter.download(&key(), VIDEO_URL).unwrap();
    assert_eq!(&clip.bytes[4..8], b"ftyp");
    let sent = adapter.transport().sent();
    assert_eq!(sent[0].url, VIDEO_URL);
    assert_eq!(sent[0].header("authorization"), None);
    assert!(
        bare(&[])
            .download(&key(), "http://cdn.higgsfield.example/x.mp4")
            .is_err()
    );
}

const LAST_MEDIA: &str = "5b1d9e0a-77c2-4f3e-8a1b-0c9d8e7f6a5b";

/// `request` ending on another uploaded image.
fn bridge(model: &str) -> ClipRequest {
    ClipRequest {
        last: Some(LastFrame {
            image: StagedImage(LAST_MEDIA.into()),
            frame: ClipImage {
                bytes: IMAGE.to_vec(),
                format: ImageFormat::Png,
            },
        }),
        ..request(model)
    }
}

#[test]
fn every_plan_model_but_kling_2_6_ends_on_a_given_frame() {
    let ends: Vec<(String, bool)> = plan(&[])
        .models()
        .into_iter()
        .map(|model| (model.id.model().to_owned(), model.ends_on_frame))
        .collect();
    assert_eq!(
        ends,
        [
            (DEFAULT_MODEL.to_owned(), true),
            ("kling3_0/pro".to_owned(), true),
            ("kling2_6".to_owned(), false),
            ("seedance_2_0/1080p".to_owned(), true),
            ("wan2_7/1080p".to_owned(), true),
            ("minimax_hailuo/2.3".to_owned(), true),
        ]
    );
}

#[test]
fn the_last_frame_is_the_end_image_media_and_is_priced_too() {
    let adapter = plan(&["estimate", "balance-plus", "submitted"]);
    adapter
        .check_submit(&key(), &bridge(DEFAULT_MODEL))
        .unwrap();
    adapter
        .submit(&key(), &bridge(DEFAULT_MODEL), "41-2-1")
        .unwrap();
    let calls = tool_calls(&adapter);
    let params = &calls[2].1["requests"][0]["params"];
    let medias = params["medias"].as_array().unwrap();
    assert_eq!(medias.len(), 2);
    assert_eq!(medias[0]["role"], "start_image");
    assert_eq!(medias[0]["value"], MEDIA);
    assert_eq!(medias[1]["role"], "end_image");
    assert_eq!(medias[1]["value"], LAST_MEDIA);
    assert_eq!(
        calls[0].1["params"], *params,
        "the estimate prices the bridge"
    );
}

#[test]
fn a_last_frame_not_uploaded_or_to_a_model_that_takes_none_is_never_sent() {
    let adapter = plan(&[]);
    let refused = adapter
        .submit(&key(), &bridge("kling2_6"), "1")
        .unwrap_err();
    assert_eq!(refused.kind, ProviderFailureKind::NotAllowed);
    assert!(refused.detail.contains("cannot end a clip"), "{refused:?}");
    let checked = adapter
        .check_submit(&key(), &bridge("kling2_6"))
        .unwrap_err();
    assert_eq!(checked.kind, ProviderFailureKind::NotAllowed);
    let mut inline = bridge(DEFAULT_MODEL);
    inline.last.as_mut().unwrap().image = StagedImage("inline".into());
    assert!(adapter.submit(&key(), &inline, "1").is_err());
    let mut inline_to_kling_2_6 = bridge("kling2_6");
    inline_to_kling_2_6.last.as_mut().unwrap().image = StagedImage("inline".into());
    let refused = adapter
        .submit(&key(), &inline_to_kling_2_6, "1")
        .unwrap_err();
    assert_eq!(
        refused.kind,
        ProviderFailureKind::NotAllowed,
        "the model's refusal comes first"
    );
    assert!(adapter.transport().sent().is_empty());
}

#[test]
fn a_generation_that_only_ends_on_the_frame_is_not_taken_for_a_lost_one() {
    let page = serde_json::json!({
        "items": [
            {
                "id": "11111111-2222-4333-8444-555555555555",
                "params": { "medias": [
                    { "role": "start_image", "value": LAST_MEDIA },
                    { "role": "end_image", "value": MEDIA },
                ] },
            },
            {
                "id": JOB,
                "params": { "medias": [
                    { "role": "start_image", "value": MEDIA },
                    { "role": "end_image", "value": LAST_MEDIA },
                ] },
            },
        ],
    });
    assert_eq!(
        bardo_ai::higgsfield_plan::find_generation(&page, MEDIA),
        Some(JOB.to_owned()),
        "the clip that starts on the frame, not the one that ends on it"
    );
}
