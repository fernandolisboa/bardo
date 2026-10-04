//! TikTok draft upload against recorded responses (see
//! `fixtures/tiktok-upload/README.md`). No test calls TikTok.

mod common;

use std::time::{Duration, SystemTime};

use bardo_ai::http::Method;
use bardo_domain::{
    SecretText, TIKTOK_MIN_CHUNK, UploadError, UploadErrorKind, UploadOutcome, UploadRun,
    UploadedVideo, VideoState, VideoUpload, VideoUploader, Visibility,
};
use bardo_publish::TikTokUploader;
use common::{Scripted, fixture};
use serde_json::{Value, json};

const API: &str = "https://open.tiktokapis.com";
const PUBLISH_ID: &str = "v_inbox_file~v2.7301234567890123456";
const NEW_PUBLISH_ID: &str = "v_inbox_file~v2.7301234567890123999";
const UPLOAD_URL: &str = "https://open-upload.tiktokapis.com/video/?upload_id=7301234567890123456&upload_token=fixture-upload-token-1";
const NEW_UPLOAD_URL: &str = "https://open-upload.tiktokapis.com/video/?upload_id=7301234567890123999&upload_token=fixture-upload-token-2";
/// Chunks of 5 MiB, TikTok's smallest.
const CHUNK: u64 = TIKTOK_MIN_CHUNK;
/// Two and a half chunks: two chunks, the last one taking the rest.
const SIZE: u64 = 13_107_200;

fn uploader(names: &[&str]) -> TikTokUploader<Scripted> {
    TikTokUploader::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("tiktok-upload", name))
            .collect(),
    ))
    .with_chunk(CHUNK)
}

fn video() -> VideoUpload {
    VideoUpload {
        title: String::new(),
        description: "Fifty years out and still sending data home. #space".into(),
        tags: Vec::new(),
        visibility: Visibility::Public,
        made_for_kids: false,
        synthetic: true,
        publish_at: None,
        share_to_feed: true,
        cover: Duration::ZERO,
    }
}

fn token() -> SecretText {
    SecretText::new("act.fixture-access-token")
}

fn millis(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn session(publish_id: &str, upload_url: &str, issued: SystemTime) -> String {
    json!({"publish_id": publish_id, "upload_url": upload_url, "issued_at": millis(issued)})
        .to_string()
}

/// A run over a file in memory that remembers what it was told to keep.
struct FakeRun {
    file: Vec<u8>,
    session: Option<String>,
    resumed: u64,
    confirmed: Vec<u64>,
    stop: bool,
}

impl FakeRun {
    fn new() -> Self {
        Self {
            file: (0..SIZE).map(|n| (n % 251) as u8).collect(),
            session: None,
            resumed: 0,
            confirmed: Vec::new(),
            stop: false,
        }
    }

    /// Resuming the first upload after `resumed` bytes, its address handed
    /// out `age` ago.
    fn resuming(resumed: u64, age: Duration) -> Self {
        Self {
            session: Some(session(PUBLISH_ID, UPLOAD_URL, SystemTime::now() - age)),
            resumed,
            ..Self::new()
        }
    }

    fn publish_id(&self) -> Option<String> {
        let session: Value = serde_json::from_str(self.session.as_deref()?).ok()?;
        session["publish_id"].as_str().map(str::to_owned)
    }
}

impl UploadRun for FakeRun {
    fn access_token(&mut self) -> Result<SecretText, UploadError> {
        Ok(token())
    }

    fn size(&self) -> u64 {
        self.file.len() as u64
    }

    fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, UploadError> {
        let start = offset as usize;
        Ok(self.file[start..(start + len).min(self.file.len())].to_vec())
    }

    fn session(&self) -> Option<String> {
        self.session.clone()
    }

    fn session_started(&mut self, session: &str) -> Result<(), UploadError> {
        self.session = Some(session.to_owned());
        Ok(())
    }

    fn confirmed(&mut self, bytes: u64) -> Result<(), UploadError> {
        self.confirmed.push(bytes);
        Ok(())
    }

    fn resumed(&self) -> u64 {
        self.resumed
    }

    fn should_stop(&self) -> bool {
        self.stop
    }
}

fn uploaded(id: &str) -> UploadOutcome {
    UploadOutcome::Uploaded(UploadedVideo { id: id.into() })
}

fn body(sent: &common::Sent) -> Value {
    serde_json::from_str(&sent.body).unwrap()
}

#[test]
fn a_new_upload_inits_a_draft_and_sends_the_chunks_in_order() {
    let uploader = uploader(&["init-ok", "chunk-partial", "chunk-complete"]);
    let mut run = FakeRun::new();

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(PUBLISH_ID));
    assert_eq!(run.publish_id().as_deref(), Some(PUBLISH_ID));
    assert_eq!(run.confirmed, [0, CHUNK, SIZE]);
    let sent = uploader.transport().sent();
    assert_eq!(sent.len(), 3);
    let init = &sent[0];
    assert_eq!(init.method, Method::Post);
    assert_eq!(init.url, format!("{API}/v2/post/publish/inbox/video/init/"));
    assert_eq!(
        init.header("authorization"),
        Some("Bearer act.fixture-access-token")
    );
    assert_eq!(
        init.header("content-type"),
        Some("application/json; charset=UTF-8")
    );
    assert_eq!(
        body(init),
        json!({"source_info": {
            "source": "FILE_UPLOAD",
            "video_size": SIZE,
            "chunk_size": CHUNK,
            "total_chunk_count": 2,
        }}),
        "the draft takes no caption: the creator writes it in TikTok"
    );
    let first = &sent[1];
    assert_eq!(first.method, Method::Put);
    assert_eq!(first.url, UPLOAD_URL);
    assert_eq!(first.header("content-type"), Some("video/mp4"));
    assert_eq!(
        first.header("content-range"),
        Some(format!("bytes 0-{}/{SIZE}", CHUNK - 1).as_str())
    );
    assert_eq!(first.bytes, run.file[..CHUNK as usize]);
    let last = &sent[2];
    assert_eq!(
        last.header("content-range"),
        Some(format!("bytes {CHUNK}-{}/{SIZE}", SIZE - 1).as_str()),
        "the last chunk takes the rest of the file"
    );
    assert_eq!(last.bytes, run.file[CHUNK as usize..]);
    for put in &sent[1..] {
        assert_eq!(
            put.header("authorization"),
            None,
            "the upload address carries its own token"
        );
    }
}

#[test]
fn a_resumed_upload_goes_on_after_the_last_confirmed_chunk() {
    let uploader = uploader(&["chunk-complete"]);
    let mut run = FakeRun::resuming(CHUNK, Duration::from_secs(10 * 60));

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(PUBLISH_ID));
    let sent = uploader.transport().sent();
    assert_eq!(
        sent.len(),
        1,
        "no new init, the first chunk is not sent again"
    );
    assert_eq!(sent[0].url, UPLOAD_URL);
    assert_eq!(
        sent[0].header("content-range"),
        Some(format!("bytes {CHUNK}-{}/{SIZE}", SIZE - 1).as_str())
    );
    assert_eq!(run.confirmed, [SIZE]);
}

#[test]
fn a_resumed_upload_whose_chunks_all_went_asks_nothing_more() {
    let uploader = uploader(&[]);
    let mut run = FakeRun::resuming(SIZE, Duration::from_secs(60));
    assert_eq!(
        uploader.upload(&video(), &mut run).unwrap(),
        uploaded(PUBLISH_ID)
    );
    assert!(uploader.transport().sent().is_empty());
}

#[test]
fn an_upload_address_past_its_hour_starts_over_with_a_new_init() {
    let uploader = uploader(&["init-again", "chunk-partial", "chunk-complete"]);
    let mut run = FakeRun::resuming(CHUNK, Duration::from_secs(70 * 60));

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(NEW_PUBLISH_ID));
    assert_eq!(run.publish_id().as_deref(), Some(NEW_PUBLISH_ID));
    assert_eq!(run.confirmed, [0, CHUNK, SIZE], "from the first byte");
    let sent = uploader.transport().sent();
    assert!(sent[0].url.ends_with("/inbox/video/init/"));
    assert_eq!(sent[1].url, NEW_UPLOAD_URL);
    assert_eq!(
        sent[1].header("content-range"),
        Some(format!("bytes 0-{}/{SIZE}", CHUNK - 1).as_str())
    );
}

#[test]
fn an_address_tiktok_refuses_as_expired_or_unknown_starts_over() {
    for refused in ["chunk-expired", "chunk-unknown-slot"] {
        let uploader = uploader(&[refused, "init-again", "chunk-partial", "chunk-complete"]);
        let mut run = FakeRun::resuming(CHUNK, Duration::from_secs(20 * 60));

        let outcome = uploader.upload(&video(), &mut run).unwrap();

        assert_eq!(outcome, uploaded(NEW_PUBLISH_ID), "{refused}");
        assert_eq!(run.confirmed, [0, CHUNK, SIZE], "{refused}");
        let sent = uploader.transport().sent();
        assert_eq!(sent[0].url, UPLOAD_URL);
        assert_eq!(sent[2].url, NEW_UPLOAD_URL);
    }
}

#[test]
fn an_upload_tiktok_keeps_dropping_gives_up_after_two_new_inits() {
    let uploader = uploader(&["init-ok", "chunk-expired", "init-again", "chunk-expired"]);
    let mut run = FakeRun::new();

    let error = uploader.upload(&video(), &mut run).unwrap_err();

    assert_eq!(error.kind, UploadErrorKind::Unexpected);
    assert_eq!(uploader.transport().sent().len(), 4, "no third init");
}

#[test]
fn a_chunk_out_of_range_goes_on_from_where_tiktok_says() {
    let uploader = uploader(&["chunk-out-of-range", "chunk-complete"]);
    // The run kept nothing, though TikTok took the first chunk.
    let mut run = FakeRun::resuming(0, Duration::from_secs(60));

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(PUBLISH_ID));
    assert_eq!(run.confirmed, [CHUNK, SIZE]);
    let sent = uploader.transport().sent();
    assert_eq!(
        sent[1].header("content-range"),
        Some(format!("bytes {CHUNK}-{}/{SIZE}", SIZE - 1).as_str())
    );
}

#[test]
fn a_failing_upload_host_keeps_what_arrived_for_the_next_try() {
    let uploader = uploader(&["init-ok", "chunk-partial", "chunk-unavailable"]);
    let mut run = FakeRun::new();

    let error = uploader.upload(&video(), &mut run).unwrap_err();

    assert_eq!(error.kind, UploadErrorKind::NetworkDown);
    assert!(error.kind.is_transient());
    assert_eq!(run.confirmed, [0, CHUNK]);
    assert!(
        !error.detail.contains("upload_token"),
        "the upload address stays out of the log: {}",
        error.detail
    );
}

#[test]
fn a_sixth_pending_draft_is_refused_at_init() {
    let uploader = uploader(&["init-pending-cap"]);
    let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::UploadLimit);
    assert!(error.detail.contains("spam_risk_too_many_pending_share"));
    assert!(error.detail.contains("log_id"));
}

#[test]
fn init_errors_are_classified() {
    for (name, kind) in [
        ("init-invalid-token", UploadErrorKind::Refused),
        ("init-scope-not-authorized", UploadErrorKind::NotAllowed),
        ("init-rate-limited", UploadErrorKind::RateLimited),
    ] {
        let uploader = uploader(&[name]);
        let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
        assert_eq!(error.kind, kind, "{name}");
        let sent = uploader.transport().sent();
        assert!(!sent[0].url.contains("fixture-access-token"));
        assert!(!sent[0].body.contains("fixture-access-token"));
        assert!(!error.detail.contains("fixture-access-token"));
    }
}

#[test]
fn an_upload_address_off_tiktoks_hosts_is_refused() {
    let uploader = uploader(&["init-elsewhere"]);
    let mut run = FakeRun::new();
    let error = uploader.upload(&video(), &mut run).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unexpected);
    assert_eq!(uploader.transport().sent().len(), 1, "no byte sent there");
    assert_eq!(run.session, None);
}

#[test]
fn a_saved_session_off_tiktoks_hosts_is_not_resumed() {
    let uploader = uploader(&["init-ok", "chunk-partial", "chunk-complete"]);
    let mut run = FakeRun {
        session: Some(session(
            PUBLISH_ID,
            "https://evil.example/video/",
            SystemTime::now(),
        )),
        resumed: CHUNK,
        ..FakeRun::new()
    };
    uploader.upload(&video(), &mut run).unwrap();
    let sent = uploader.transport().sent();
    assert!(sent.iter().all(|request| !request.url.contains("evil")));
    assert_eq!(run.confirmed, [0, CHUNK, SIZE]);
}

#[test]
fn a_run_that_stops_sends_nothing_more() {
    let uploader = uploader(&[]);
    let mut run = FakeRun {
        stop: true,
        ..FakeRun::resuming(CHUNK, Duration::from_secs(60))
    };
    assert_eq!(
        uploader.upload(&video(), &mut run).unwrap(),
        UploadOutcome::Stopped
    );
}

#[test]
fn an_empty_file_is_refused_before_any_call() {
    let uploader = uploader(&[]);
    let mut run = FakeRun {
        file: Vec::new(),
        ..FakeRun::new()
    };
    let error = uploader.upload(&video(), &mut run).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Local);
}

fn state(name: &str) -> (Result<VideoState, UploadError>, Vec<common::Sent>) {
    let uploader = uploader(&[name]);
    let state = uploader.state(&token(), PUBLISH_ID);
    (state, uploader.transport().sent())
}

#[test]
fn the_draft_status_says_where_it_stands() {
    let (processing, sent) = state("status-processing");
    assert_eq!(processing.unwrap(), VideoState::Processing);
    assert_eq!(sent[0].method, Method::Post);
    assert_eq!(sent[0].url, format!("{API}/v2/post/publish/status/fetch/"));
    assert_eq!(body(&sent[0]), json!({"publish_id": PUBLISH_ID}));
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer act.fixture-access-token")
    );

    assert_eq!(state("status-inbox").0.unwrap(), VideoState::InInbox);
    assert_eq!(
        state("status-complete").0.unwrap(),
        VideoState::InInbox,
        "posted already: it went to the inbox all the same"
    );
    assert_eq!(
        state("status-failed").0.unwrap(),
        VideoState::Failed("frame_rate_check_failed".into())
    );
    assert_eq!(
        state("status-spam").0.unwrap(),
        VideoState::Rejected("spam_risk_too_many_posts".into())
    );
    assert_eq!(
        state("status-invalid-publish-id").0.unwrap(),
        VideoState::Removed
    );
}

#[test]
fn a_draft_whose_access_was_removed_or_token_refused_asks_to_reconnect() {
    for name in ["status-auth-removed", "status-invalid-token"] {
        assert_eq!(
            state(name).0.unwrap_err().kind,
            UploadErrorKind::Refused,
            "{name}"
        );
    }
}

#[test]
fn an_unsafe_publish_id_is_never_sent() {
    let uploader = uploader(&[]);
    let error = uploader.state(&token(), "v_inbox\"}, {\"x").unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unexpected);
    assert!(uploader.transport().sent().is_empty());
}
