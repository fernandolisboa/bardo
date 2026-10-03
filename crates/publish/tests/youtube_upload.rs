//! YouTube resumable upload against recorded responses (see
//! `fixtures/youtube-upload/README.md`). No test calls Google.

mod common;

use bardo_ai::http::Method;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    Rfc3339, ScheduleChange, ScheduleOutcome, SecretText, UploadError, UploadErrorKind,
    UploadOutcome, UploadRun, UploadedVideo, VideoState, VideoUpload, VideoUploader, Visibility,
    parse_rfc3339,
};
use bardo_publish::YouTubeUploader;
use common::{Scripted, fixture};

const KIB_256: usize = 256 * 1024;
/// Two full chunks and a short last one.
const SIZE: usize = 2 * KIB_256 + 75_712;
const SESSION: &str = "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status&upload_id=FIXTURE-upload-id-1";
const NEW_SESSION: &str = "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status&upload_id=FIXTURE-upload-id-2";

fn uploader(names: &[&str]) -> YouTubeUploader<Scripted> {
    YouTubeUploader::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("youtube-upload", name))
            .collect(),
    ))
    .with_chunk_units(1)
}

fn video() -> VideoUpload {
    VideoUpload {
        title: "Why the Voyager probes are still talking".into(),
        description: "Fifty years out and still sending data home.".into(),
        tags: vec!["space".into(), "voyager probes".into()],
        visibility: Visibility::Public,
        made_for_kids: false,
        synthetic: true,
        publish_at: None,
    }
}

/// A run over a file in memory that remembers what it was told to keep.
struct FakeRun {
    file: Vec<u8>,
    session: Option<String>,
    confirmed: Vec<u64>,
    /// Asks to stop once this many confirmations came in.
    stop_after: Option<usize>,
    signed_out: bool,
}

impl FakeRun {
    fn new() -> Self {
        Self {
            file: (0..SIZE).map(|i| (i % 251) as u8).collect(),
            session: None,
            confirmed: Vec::new(),
            stop_after: None,
            signed_out: false,
        }
    }

    fn resuming(session: &str) -> Self {
        Self {
            session: Some(session.to_owned()),
            ..Self::new()
        }
    }
}

impl UploadRun for FakeRun {
    fn access_token(&mut self) -> Result<SecretText, UploadError> {
        if self.signed_out {
            return Err(UploadError::new(UploadErrorKind::SignedOut, "reconnect"));
        }
        Ok(SecretText::new("ya29.fixture-access"))
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

    fn should_stop(&self) -> bool {
        self.stop_after
            .is_some_and(|after| self.confirmed.len() >= after)
    }
}

fn uploaded() -> UploadOutcome {
    UploadOutcome::Uploaded(UploadedVideo {
        id: "Xb7kQ2mN9pA".into(),
    })
}

#[test]
fn a_new_upload_starts_a_session_and_sends_the_file_in_chunks() {
    let uploader = uploader(&[
        "session-started",
        "chunk-incomplete-1",
        "chunk-incomplete-2",
        "upload-complete",
    ]);
    let mut run = FakeRun::new();

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded());
    assert_eq!(
        run.session.as_deref(),
        Some(SESSION),
        "kept before any byte"
    );
    assert_eq!(run.confirmed, [0, 262_144, 524_288]);
    let sent = uploader.transport().sent();
    assert_eq!(sent.len(), 4);

    let start = &sent[0];
    assert_eq!(start.method, Method::Post);
    assert_eq!(
        start.url,
        "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status"
    );
    assert_eq!(
        start.header("authorization"),
        Some("Bearer ya29.fixture-access")
    );
    assert_eq!(
        start.header("x-upload-content-length"),
        Some(SIZE.to_string().as_str())
    );
    assert_eq!(start.header("x-upload-content-type"), Some("video/mp4"));
    assert_eq!(
        start.header("content-type"),
        Some("application/json; charset=UTF-8")
    );
    let body: serde_json::Value = serde_json::from_str(&start.body).unwrap();
    assert_eq!(
        body["snippet"]["title"],
        "Why the Voyager probes are still talking"
    );
    assert_eq!(
        body["snippet"]["tags"],
        serde_json::json!(["space", "voyager probes"])
    );
    assert_eq!(body["status"]["privacyStatus"], "public");
    assert_eq!(body["status"]["selfDeclaredMadeForKids"], false);
    assert_eq!(body["status"]["containsSyntheticMedia"], true);

    let ranges: Vec<_> = sent[1..]
        .iter()
        .map(|chunk| {
            assert_eq!(chunk.method, Method::Put);
            assert_eq!(chunk.url, SESSION);
            assert_eq!(
                chunk.bytes.len() % KIB_256 == 0,
                chunk.bytes.len() == KIB_256
            );
            chunk.header("content-range").unwrap().to_owned()
        })
        .collect();
    assert_eq!(
        ranges,
        [
            format!("bytes 0-262143/{SIZE}"),
            format!("bytes 262144-524287/{SIZE}"),
            format!("bytes 524288-{}/{SIZE}", SIZE - 1),
        ]
    );
    let all: Vec<u8> = sent[1..]
        .iter()
        .flat_map(|chunk| chunk.bytes.clone())
        .collect();
    assert_eq!(all, run.file, "every byte once, in order");
}

#[test]
fn an_interrupted_upload_resumes_from_the_confirmed_bytes() {
    let uploader = uploader(&[
        "status-first-chunk",
        "chunk-incomplete-2",
        "upload-complete",
    ]);
    let mut run = FakeRun::resuming(SESSION);

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded());
    let sent = uploader.transport().sent();
    let query = &sent[0];
    assert_eq!(query.method, Method::Put);
    assert_eq!(query.url, SESSION);
    assert!(query.bytes.is_empty());
    assert_eq!(
        query.header("content-range"),
        Some(format!("bytes */{SIZE}").as_str())
    );
    assert_eq!(
        sent[1].header("content-range"),
        Some(format!("bytes 262144-524287/{SIZE}").as_str()),
        "the confirmed first chunk is not sent again"
    );
    assert_eq!(sent[1].bytes, run.file[KIB_256..2 * KIB_256]);
    assert_eq!(run.confirmed, [262_144, 524_288]);
}

#[test]
fn a_session_with_nothing_received_resumes_from_the_first_byte() {
    let uploader = uploader(&[
        "status-nothing-yet",
        "chunk-incomplete-1",
        "chunk-incomplete-2",
        "upload-complete",
    ]);
    let mut run = FakeRun::resuming(SESSION);

    assert_eq!(uploader.upload(&video(), &mut run).unwrap(), uploaded());
    let sent = uploader.transport().sent();
    assert_eq!(
        sent[1].header("content-range"),
        Some(format!("bytes 0-262143/{SIZE}").as_str())
    );
    assert_eq!(run.session.as_deref(), Some(SESSION), "the same session");
}

#[test]
fn an_expired_session_restarts_cleanly_with_a_new_one() {
    let uploader = uploader(&[
        "session-gone",
        "session-restarted",
        "chunk-incomplete-1",
        "chunk-incomplete-2",
        "upload-complete",
    ]);
    let mut run = FakeRun::resuming(SESSION);

    assert_eq!(uploader.upload(&video(), &mut run).unwrap(), uploaded());
    assert_eq!(run.session.as_deref(), Some(NEW_SESSION));
    assert_eq!(run.confirmed, [0, 262_144, 524_288], "starts over at 0");
    let sent = uploader.transport().sent();
    assert_eq!(sent[1].method, Method::Post, "a new session");
    assert_eq!(sent[2].url, NEW_SESSION);
    assert_eq!(
        sent[2].header("content-range"),
        Some(format!("bytes 0-262143/{SIZE}").as_str())
    );
}

#[test]
fn a_session_lost_mid_upload_also_restarts() {
    let uploader = uploader(&[
        "session-started",
        "chunk-incomplete-1",
        "session-gone",
        "session-restarted",
        "chunk-incomplete-1",
        "chunk-incomplete-2",
        "upload-complete",
    ]);
    let mut run = FakeRun::new();

    assert_eq!(uploader.upload(&video(), &mut run).unwrap(), uploaded());
    assert_eq!(run.session.as_deref(), Some(NEW_SESSION));
}

#[test]
fn a_session_forgotten_again_and_again_gives_up() {
    let uploader = uploader(&[
        "session-started",
        "session-gone",
        "session-restarted",
        "session-gone",
    ]);
    let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unexpected);
    assert_eq!(uploader.transport().sent().len(), 4, "no endless loop");
}

#[test]
fn a_session_on_another_host_gets_no_token() {
    let elsewhere = uploader(&["session-elsewhere"]);
    let mut run = FakeRun::new();
    let error = elsewhere.upload(&video(), &mut run).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unexpected);
    assert_eq!(run.session, None, "not kept");
    assert_eq!(elsewhere.transport().sent().len(), 1, "no chunk sent there");

    // Nor does a saved one: a new session starts on the API's host.
    let fresh = uploader(&[
        "session-started",
        "chunk-incomplete-1",
        "chunk-incomplete-2",
        "upload-complete",
    ]);
    let mut run =
        FakeRun::resuming("http://www.googleapis.com/upload/youtube/v3/videos?upload_id=x");
    assert_eq!(fresh.upload(&video(), &mut run).unwrap(), uploaded());
    let sent = fresh.transport().sent();
    assert!(
        sent.iter()
            .all(|request| request.url.starts_with("https://www.googleapis.com/"))
    );
    assert_eq!(run.session.as_deref(), Some(SESSION));
}

#[test]
fn a_stop_keeps_the_session_for_the_next_run() {
    let uploader = uploader(&["session-started", "chunk-incomplete-1"]);
    let mut run = FakeRun::new();
    run.stop_after = Some(2);

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, UploadOutcome::Stopped);
    assert_eq!(run.session.as_deref(), Some(SESSION));
    assert_eq!(run.confirmed.last(), Some(&262_144));
}

#[test]
fn server_errors_are_transient_and_quota_is_not() {
    let cases = [
        ("server-unavailable", UploadErrorKind::NetworkDown, true),
        ("not-implemented", UploadErrorKind::Unexpected, false),
        ("quota-exceeded", UploadErrorKind::QuotaExceeded, false),
        ("upload-limit", UploadErrorKind::UploadLimit, false),
        ("invalid-title", UploadErrorKind::Invalid, false),
        ("videos-invalid-token", UploadErrorKind::Refused, false),
    ];
    for (name, kind, transient) in cases {
        // At the start of a session.
        let uploader = uploader(&[name]);
        let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
        assert_eq!(error.kind, kind, "{name}");
        assert_eq!(error.kind.is_transient(), transient, "{name}");
        assert_eq!(
            uploader.transport().sent().len(),
            1,
            "{name}: no retry loop"
        );
    }
    // In the middle of the file: the session stays for the retry.
    let uploader = uploader(&[
        "session-started",
        "chunk-incomplete-1",
        "server-unavailable",
    ]);
    let mut run = FakeRun::new();
    let error = uploader.upload(&video(), &mut run).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::NetworkDown);
    assert!(error.detail.contains("503"));
    assert!(!error.detail.contains("upload_id"), "no session in the log");
    assert_eq!(run.session.as_deref(), Some(SESSION));
    assert_eq!(run.confirmed.last(), Some(&262_144));
}

#[test]
fn quota_names_its_reason_in_the_detail() {
    let uploader = uploader(&["quota-exceeded"]);
    let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
    assert!(
        error.detail.starts_with("HTTP 403: quotaExceeded"),
        "{}",
        error.detail
    );
}

#[test]
fn no_token_means_no_request() {
    let uploader = uploader(&[]);
    let mut run = FakeRun::new();
    run.signed_out = true;
    let error = uploader.upload(&video(), &mut run).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::SignedOut);
    assert!(uploader.transport().sent().is_empty());
}

#[test]
fn an_empty_file_is_not_uploaded() {
    let uploader = uploader(&[]);
    let mut run = FakeRun::new();
    run.file.clear();
    assert_eq!(
        uploader.upload(&video(), &mut run).unwrap_err().kind,
        UploadErrorKind::Local
    );
}

#[test]
fn offline_is_unreachable_and_transient() {
    let uploader = YouTubeUploader::with_transport(Scripted::offline());
    let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unreachable);
    assert!(error.kind.is_transient());
}

fn state(name: &str) -> Result<VideoState, UploadError> {
    let uploader = uploader(&[name]);
    let state = uploader.state(&SecretText::new("ya29.fixture-access"), "Xb7kQ2mN9pA");
    let sent = uploader.transport().sent();
    assert_eq!(
        sent[0].url,
        "https://www.googleapis.com/youtube/v3/videos?part=snippet%2Cstatus&id=Xb7kQ2mN9pA"
    );
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer ya29.fixture-access")
    );
    state
}

#[test]
fn the_videos_state_reads_processing_and_its_outcome() {
    assert_eq!(state("videos-processing"), Ok(VideoState::Processing));
    assert_eq!(
        state("videos-processed-public"),
        Ok(VideoState::Ready {
            visibility: Visibility::Public,
            publish_at: None,
            published_at: None,
        })
    );
    assert_eq!(
        state("videos-processed-private"),
        Ok(VideoState::Ready {
            visibility: Visibility::Private,
            publish_at: None,
            published_at: None,
        })
    );
    assert_eq!(
        state("videos-scheduled"),
        Ok(VideoState::Ready {
            visibility: Visibility::Private,
            publish_at: parse_rfc3339("2026-10-04T22:00:00Z"),
            published_at: parse_rfc3339("2026-10-03T21:50:12Z"),
        })
    );
    assert_eq!(
        state("videos-live"),
        Ok(VideoState::Ready {
            visibility: Visibility::Public,
            publish_at: None,
            published_at: parse_rfc3339("2026-10-04T22:00:03Z"),
        })
    );
    assert_eq!(
        state("videos-rejected"),
        Ok(VideoState::Rejected("duplicate".into()))
    );
    assert_eq!(
        state("videos-failed"),
        Ok(VideoState::Failed("codec".into()))
    );
    assert_eq!(state("videos-none"), Ok(VideoState::Removed));
    assert_eq!(
        state("videos-invalid-token").unwrap_err().kind,
        UploadErrorKind::Refused
    );
}

fn tomorrow() -> SystemTime {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    SystemTime::UNIX_EPOCH + Duration::from_secs(now - now % 60 + 24 * 3600)
}

#[test]
fn a_scheduled_upload_goes_private_with_its_publish_time() {
    let uploader = uploader(&[
        "session-started",
        "chunk-incomplete-1",
        "chunk-incomplete-2",
        "upload-complete",
    ]);
    let at = tomorrow();
    let scheduled = VideoUpload {
        publish_at: Some(at),
        ..video()
    };
    uploader.upload(&scheduled, &mut FakeRun::new()).unwrap();
    let body: serde_json::Value =
        serde_json::from_str(&uploader.transport().sent()[0].body).unwrap();
    assert_eq!(
        body["status"],
        serde_json::json!({
            "privacyStatus": "private",
            "publishAt": Rfc3339(at).to_string(),
            "selfDeclaredMadeForKids": false,
            "containsSyntheticMedia": true,
        })
    );
    assert!(
        Rfc3339(at).to_string().ends_with(":00Z"),
        "UTC, to the second"
    );
}

#[test]
fn a_scheduled_upload_whose_time_passed_starts_no_session() {
    let uploader = uploader(&[]);
    let late = VideoUpload {
        publish_at: Some(SystemTime::now() - Duration::from_secs(60)),
        ..video()
    };
    let error = uploader.upload(&late, &mut FakeRun::new()).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Late);
    assert!(!error.kind.is_transient());
    assert!(uploader.transport().sent().is_empty(), "nothing sent");
}

fn change(publish_at: Option<SystemTime>) -> ScheduleChange {
    ScheduleChange {
        publish_at,
        made_for_kids: false,
        synthetic: false,
    }
}

fn reschedule(
    names: &[&str],
    change: &ScheduleChange,
) -> (Result<ScheduleOutcome, UploadError>, Vec<common::Sent>) {
    let uploader = uploader(names);
    let outcome = uploader.reschedule(
        &SecretText::new("ya29.fixture-access"),
        "Xb7kQ2mN9pA",
        change,
    );
    (outcome, uploader.transport().sent())
}

#[test]
fn changing_the_time_sends_the_whole_status_back_with_the_new_time() {
    let at = tomorrow();
    let (outcome, sent) = reschedule(&["videos-scheduled", "update-scheduled"], &change(Some(at)));
    assert_eq!(outcome, Ok(ScheduleOutcome::Changed));
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].method, Method::Get, "reads the status first");
    let update = &sent[1];
    assert_eq!(update.method, Method::Put);
    assert_eq!(
        update.url,
        "https://www.googleapis.com/youtube/v3/videos?part=status"
    );
    assert_eq!(
        update.header("authorization"),
        Some("Bearer ya29.fixture-access")
    );
    assert_eq!(update.header("content-type"), Some("application/json"));
    let body: serde_json::Value = serde_json::from_str(&update.body).unwrap();
    assert_eq!(
        body,
        serde_json::json!({
            "id": "Xb7kQ2mN9pA",
            "status": {
                "privacyStatus": "private",
                "publishAt": Rfc3339(at).to_string(),
                "license": "creativeCommon",
                "embeddable": false,
                "publicStatsViewable": false,
                "selfDeclaredMadeForKids": true,
                "containsSyntheticMedia": true,
            },
        }),
        "every settable property, as the video has it; only the time changes"
    );
}

#[test]
fn cancelling_leaves_a_private_video_with_no_publish_time() {
    let (outcome, sent) = reschedule(&["videos-scheduled", "update-scheduled"], &change(None));
    assert_eq!(outcome, Ok(ScheduleOutcome::Changed));
    let body: serde_json::Value = serde_json::from_str(&sent[1].body).unwrap();
    assert_eq!(body["status"]["privacyStatus"], "private");
    assert!(body["status"].get("publishAt").is_none());
    assert_eq!(body["status"]["selfDeclaredMadeForKids"], true, "kept");
    assert_eq!(body["status"]["containsSyntheticMedia"], true, "kept");
}

#[test]
fn what_youtube_does_not_say_comes_from_the_upload() {
    let declared = ScheduleChange {
        publish_at: Some(tomorrow()),
        made_for_kids: true,
        synthetic: true,
    };
    let (outcome, sent) = reschedule(&["videos-scheduled-sparse", "update-scheduled"], &declared);
    assert_eq!(outcome, Ok(ScheduleOutcome::Changed));
    let body: serde_json::Value = serde_json::from_str(&sent[1].body).unwrap();
    let status = body["status"].as_object().unwrap();
    assert_eq!(status["selfDeclaredMadeForKids"], true);
    assert_eq!(status["containsSyntheticMedia"], true);
    assert!(
        !status.contains_key("license") && !status.contains_key("embeddable"),
        "nothing made up"
    );
}

#[test]
fn a_video_already_live_is_not_changed() {
    let (outcome, sent) = reschedule(&["videos-live"], &change(Some(tomorrow())));
    assert_eq!(
        outcome,
        Ok(ScheduleOutcome::Live {
            visibility: Visibility::Public,
            published_at: parse_rfc3339("2026-10-04T22:00:03Z"),
        })
    );
    assert_eq!(sent.len(), 1, "no update sent");
}

#[test]
fn a_new_time_that_is_not_ahead_is_not_sent() {
    let past = SystemTime::now() - Duration::from_secs(60);
    let (outcome, sent) = reschedule(&["videos-scheduled"], &change(Some(past)));
    assert_eq!(outcome.unwrap_err().kind, UploadErrorKind::Late);
    assert_eq!(sent.len(), 1, "no update sent");
}

#[test]
fn schedule_changes_name_what_youtube_refused() {
    let (outcome, _) = reschedule(&["videos-none"], &change(None));
    assert_eq!(outcome.unwrap_err().kind, UploadErrorKind::NotFound);
    let (outcome, _) = reschedule(&["videos-scheduled", "update-not-found"], &change(None));
    assert_eq!(outcome.unwrap_err().kind, UploadErrorKind::NotFound);
    let (outcome, _) = reschedule(
        &["videos-scheduled", "update-invalid-publish-at"],
        &change(Some(tomorrow())),
    );
    let error = outcome.unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Invalid);
    assert!(
        error.detail.contains("invalidPublishAt"),
        "{}",
        error.detail
    );
    let (outcome, _) = reschedule(&["videos-invalid-token"], &change(None));
    assert_eq!(outcome.unwrap_err().kind, UploadErrorKind::Refused);
}
