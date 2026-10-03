//! YouTube resumable upload against recorded responses (see
//! `fixtures/youtube-upload/README.md`). No test calls Google.

mod common;

use bardo_ai::http::Method;
use bardo_domain::{
    SecretText, UploadError, UploadErrorKind, UploadOutcome, UploadRun, UploadedVideo, VideoState,
    VideoUpload, VideoUploader, Visibility,
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
        "https://www.googleapis.com/youtube/v3/videos?part=status&id=Xb7kQ2mN9pA"
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
            visibility: Visibility::Public
        })
    );
    assert_eq!(
        state("videos-processed-private"),
        Ok(VideoState::Ready {
            visibility: Visibility::Private
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
