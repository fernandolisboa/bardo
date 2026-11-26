//! X video upload and post against recorded responses (see
//! `fixtures/x-upload/README.md`). No test calls X.

mod common;

use std::time::{Duration, SystemTime};

use bardo_ai::http::{Method, TransportError};
use bardo_domain::{
    SecretText, UploadError, UploadErrorKind, UploadOutcome, UploadRun, UploadedVideo, VideoState,
    VideoUpload, VideoUploader, Visibility,
};
use bardo_publish::XUploader;
use common::{Scripted, Sent, fixture};
use serde_json::{Value, json};

const API: &str = "https://api.x.com";
const MEDIA_ID: &str = "1880028106020515840";
const NEW_MEDIA_ID: &str = "1880028106020515841";
const POST_ID: &str = "1880028106020515999";
const USER_ID: &str = "1840000000000000001";
/// Small chunks so the file in memory stays small.
const CHUNK: u64 = 10;
/// Two whole chunks and a short last one.
const SIZE: u64 = 25;

fn uploader(names: &[&str]) -> XUploader<Scripted> {
    XUploader::with_transport(Scripted::new(
        names.iter().map(|name| fixture("x-upload", name)).collect(),
    ))
    .with_chunk(CHUNK)
}

fn video() -> VideoUpload {
    VideoUpload {
        title: String::new(),
        description: "Por que Plutão deixou de ser planeta? #espaço".into(),
        tags: Vec::new(),
        visibility: Visibility::Public,
        made_for_kids: false,
        synthetic: true,
        publish_at: None,
        share_to_feed: true,
        cover: Duration::ZERO,
        paid_partnership: false,
        sponsors: Vec::new(),
    }
}

fn token() -> SecretText {
    SecretText::new("fixture-access-token")
}

fn unix(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
}

fn session(media_id: &str, expires_at: SystemTime) -> String {
    json!({"media_id": media_id, "expires_at": unix(expires_at)}).to_string()
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
            file: (0..SIZE).map(|n| b'a' + (n % 26) as u8).collect(),
            session: None,
            resumed: 0,
            confirmed: Vec::new(),
            stop: false,
        }
    }

    /// Resuming the first upload after `resumed` bytes; X keeps it for
    /// `left` more.
    fn resuming(resumed: u64, left: Duration) -> Self {
        Self {
            session: Some(session(MEDIA_ID, SystemTime::now() + left)),
            resumed,
            ..Self::new()
        }
    }

    fn media_id(&self) -> Option<String> {
        let session: Value = serde_json::from_str(self.session.as_deref()?).ok()?;
        session["media_id"].as_str().map(str::to_owned)
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

fn body(sent: &Sent) -> Value {
    serde_json::from_str(&sent.body).unwrap()
}

/// The `segment_index` field of a chunk's multipart form.
fn segment(sent: &Sent) -> String {
    let (_, after) = sent
        .body
        .split_once("name=\"segment_index\"\r\n\r\n")
        .expect("a segment_index field");
    after.split("\r\n").next().unwrap().to_owned()
}

/// The `media` part of a chunk's multipart form.
fn media(sent: &Sent) -> String {
    let (_, after) = sent
        .body
        .split_once("Content-Type: application/octet-stream\r\n\r\n")
        .expect("a media part");
    after.split("\r\n").next().unwrap().to_owned()
}

#[test]
fn a_new_upload_initializes_sends_numbered_chunks_and_finalizes() {
    let uploader = uploader(&[
        "initialize-ok",
        "append-ok",
        "append-ok",
        "append-ok",
        "finalize-processing",
    ]);
    let mut run = FakeRun::new();

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(MEDIA_ID));
    assert_eq!(run.media_id().as_deref(), Some(MEDIA_ID));
    assert_eq!(run.confirmed, [0, 10, 20, 25]);
    let sent = uploader.transport().sent();
    assert_eq!(sent.len(), 5);
    let init = &sent[0];
    assert_eq!(init.method, Method::Post);
    assert_eq!(init.url, format!("{API}/2/media/upload/initialize"));
    assert_eq!(
        init.header("authorization"),
        Some("Bearer fixture-access-token")
    );
    assert_eq!(
        body(init),
        json!({"media_type": "video/mp4", "total_bytes": SIZE, "media_category": "tweet_video"}),
        "the text goes with the post, not the upload"
    );
    for (index, chunk) in sent[1..4].iter().enumerate() {
        assert_eq!(chunk.method, Method::Post);
        assert_eq!(chunk.url, format!("{API}/2/media/upload/{MEDIA_ID}/append"));
        assert!(
            chunk
                .header("content-type")
                .is_some_and(|value| value.starts_with("multipart/form-data; boundary=")),
            "{:?}",
            chunk.header("content-type")
        );
        assert_eq!(segment(chunk), index.to_string());
        assert_eq!(
            chunk.header("authorization"),
            Some("Bearer fixture-access-token")
        );
    }
    assert_eq!(media(&sent[1]), "abcdefghij");
    assert_eq!(media(&sent[3]), "uvwxy", "the last chunk takes the rest");
    let finalize = &sent[4];
    assert_eq!(finalize.method, Method::Post);
    assert_eq!(
        finalize.url,
        format!("{API}/2/media/upload/{MEDIA_ID}/finalize")
    );
}

#[test]
fn the_kept_session_holds_the_media_id_and_xs_deadline() {
    let uploader = uploader(&[
        "initialize-ok",
        "append-ok",
        "append-ok",
        "append-ok",
        "finalize-processing",
    ]);
    let mut run = FakeRun::new();
    let before = unix(SystemTime::now());
    uploader.upload(&video(), &mut run).unwrap();
    let kept: Value = serde_json::from_str(run.session.as_deref().unwrap()).unwrap();
    let expires_at = kept["expires_at"].as_u64().unwrap();
    // `expires_after_secs` is 86400 in the answer.
    assert!(
        (before + 86_400..=before + 86_402).contains(&expires_at),
        "{expires_at}"
    );
}

#[test]
fn a_resumed_upload_goes_on_after_the_last_confirmed_chunk() {
    let uploader = uploader(&["append-ok", "finalize-processing"]);
    let mut run = FakeRun::resuming(20, Duration::from_secs(60 * 60));

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(MEDIA_ID));
    let sent = uploader.transport().sent();
    assert_eq!(sent.len(), 2, "no new initialize, no chunk sent again");
    assert_eq!(segment(&sent[0]), "2");
    assert_eq!(media(&sent[0]), "uvwxy");
    assert_eq!(run.confirmed, [25]);
}

#[test]
fn a_resumed_upload_whose_chunks_all_went_only_finalizes() {
    let uploader = uploader(&["finalize-processing"]);
    let mut run = FakeRun::resuming(SIZE, Duration::from_secs(60 * 60));
    assert_eq!(
        uploader.upload(&video(), &mut run).unwrap(),
        uploaded(MEDIA_ID)
    );
    assert_eq!(uploader.transport().sent().len(), 1);
}

#[test]
fn a_finalize_a_cut_short_run_already_sent_asks_the_status() {
    let uploader = uploader(&["finalize-already", "status-succeeded"]);
    let mut run = FakeRun::resuming(SIZE, Duration::from_secs(60 * 60));
    assert_eq!(
        uploader.upload(&video(), &mut run).unwrap(),
        uploaded(MEDIA_ID)
    );
    let sent = uploader.transport().sent();
    assert_eq!(sent[1].method, Method::Get);
    assert_eq!(
        sent[1].url,
        format!("{API}/2/media/upload?command=STATUS&media_id={MEDIA_ID}")
    );
}

#[test]
fn a_resume_off_a_chunk_boundary_starts_over() {
    let uploader = uploader(&[
        "initialize-again",
        "append-ok",
        "append-ok",
        "append-ok",
        "finalize-processing",
    ]);
    let mut run = FakeRun::resuming(7, Duration::from_secs(60 * 60));
    assert_eq!(
        uploader.upload(&video(), &mut run).unwrap(),
        uploaded(NEW_MEDIA_ID)
    );
    assert_eq!(run.confirmed, [0, 10, 20, 25]);
}

#[test]
fn an_upload_near_its_end_starts_over_with_a_new_one() {
    let uploader = uploader(&[
        "initialize-again",
        "append-ok",
        "append-ok",
        "append-ok",
        "finalize-processing",
    ]);
    // Five minutes left: too close to X dropping it.
    let mut run = FakeRun::resuming(10, Duration::from_secs(5 * 60));

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(NEW_MEDIA_ID));
    assert_eq!(run.media_id().as_deref(), Some(NEW_MEDIA_ID));
    assert_eq!(run.confirmed, [0, 10, 20, 25], "from the first byte");
    let sent = uploader.transport().sent();
    assert!(sent[0].url.ends_with("/initialize"));
    assert_eq!(segment(&sent[1]), "0");
}

#[test]
fn an_upload_x_no_longer_knows_starts_over() {
    let uploader = uploader(&[
        "append-unknown-upload",
        "initialize-again",
        "append-ok",
        "append-ok",
        "append-ok",
        "finalize-processing",
    ]);
    let mut run = FakeRun::resuming(10, Duration::from_secs(60 * 60));

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(NEW_MEDIA_ID));
    let sent = uploader.transport().sent();
    assert_eq!(
        sent[0].url,
        format!("{API}/2/media/upload/{MEDIA_ID}/append")
    );
    assert_eq!(
        sent[2].url,
        format!("{API}/2/media/upload/{NEW_MEDIA_ID}/append")
    );
}

#[test]
fn an_upload_x_keeps_dropping_gives_up_after_two_new_ones() {
    let uploader = uploader(&[
        "initialize-ok",
        "append-unknown-upload",
        "initialize-again",
        "append-unknown-upload",
    ]);
    let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unexpected);
    assert_eq!(uploader.transport().sent().len(), 4, "no third initialize");
}

#[test]
fn a_saved_session_that_is_not_xs_is_not_resumed() {
    let uploader = uploader(&[
        "initialize-ok",
        "append-ok",
        "append-ok",
        "append-ok",
        "finalize-processing",
    ]);
    let mut run = FakeRun {
        session: Some(session(
            "../../evil",
            SystemTime::now() + Duration::from_secs(3600),
        )),
        resumed: 10,
        ..FakeRun::new()
    };
    uploader.upload(&video(), &mut run).unwrap();
    let sent = uploader.transport().sent();
    assert!(sent.iter().all(|request| !request.url.contains("evil")));
    assert_eq!(run.confirmed, [0, 10, 20, 25]);
}

#[test]
fn a_failing_chunk_keeps_what_arrived_for_the_next_try() {
    let uploader = uploader(&["initialize-ok", "append-ok", "rate-limited"]);
    let mut run = FakeRun::new();
    let error = uploader.upload(&video(), &mut run).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::RateLimited);
    assert!(error.kind.is_transient());
    assert_eq!(run.confirmed, [0, 10]);
}

#[test]
fn upload_errors_are_classified() {
    for (name, kind) in [
        ("not-a-user-token", UploadErrorKind::Refused),
        ("credits-depleted", UploadErrorKind::QuotaExceeded),
        ("rate-limited", UploadErrorKind::RateLimited),
    ] {
        let uploader = uploader(&[name]);
        let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
        assert_eq!(error.kind, kind, "{name}");
        assert!(!error.detail.contains("fixture-access-token"), "{name}");
    }
}

#[test]
fn a_run_that_stops_sends_nothing_more() {
    let uploader = uploader(&[]);
    let mut run = FakeRun {
        stop: true,
        ..FakeRun::resuming(10, Duration::from_secs(3600))
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
    assert_eq!(
        uploader.upload(&video(), &mut run).unwrap_err().kind,
        UploadErrorKind::Local
    );
}

fn state(name: &str) -> Result<VideoState, UploadError> {
    uploader(&[name]).state(&token(), MEDIA_ID)
}

#[test]
fn the_status_says_where_processing_stands() {
    assert_eq!(state("status-in-progress").unwrap(), VideoState::Processing);
    assert_eq!(
        state("finalize-processing").unwrap(),
        VideoState::Processing
    );
    assert_eq!(state("status-succeeded").unwrap(), VideoState::Processed);
    assert_eq!(
        state("status-no-processing").unwrap(),
        VideoState::Processed,
        "no processing info means ready"
    );
    assert_eq!(
        state("status-failed").unwrap(),
        VideoState::Failed("Unsupported video codec".into())
    );
    assert_eq!(state("status-not-found").unwrap(), VideoState::Expired);
    assert_eq!(
        state("not-a-user-token").unwrap_err().kind,
        UploadErrorKind::Refused
    );
}

#[test]
fn the_status_takes_only_x_media_ids() {
    let uploader = uploader(&[]);
    assert!(uploader.state(&token(), "abc&command=x").is_err());
    assert!(uploader.transport().sent().is_empty());
}

fn publish(
    names: &[&str],
    video: &VideoUpload,
) -> (Result<bardo_domain::NetworkPost, UploadError>, Vec<Sent>) {
    let uploader = uploader(names);
    let post = uploader.publish(&token(), USER_ID, MEDIA_ID, video);
    (post, uploader.transport().sent())
}

#[test]
fn the_post_carries_the_text_the_video_and_the_ai_label() {
    let (post, sent) = publish(&["post-created"], &video());
    let post = post.unwrap();
    assert_eq!(post.id, POST_ID);
    assert_eq!(
        post.link.unwrap().url(),
        format!("https://x.com/i/status/{POST_ID}")
    );
    assert_eq!(sent[0].method, Method::Post);
    assert_eq!(sent[0].url, format!("{API}/2/tweets"));
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer fixture-access-token")
    );
    assert_eq!(
        body(&sent[0]),
        json!({
            "text": "Por que Plutão deixou de ser planeta? #espaço",
            "media": {"media_ids": [MEDIA_ID]},
            "made_with_ai": true,
        })
    );
}

#[test]
fn a_video_without_synthetic_content_goes_without_the_label() {
    let video = VideoUpload {
        synthetic: false,
        ..video()
    };
    let (_, sent) = publish(&["post-created"], &video);
    assert_eq!(body(&sent[0]).get("made_with_ai"), None);
}

#[test]
fn a_paid_partnership_post_carries_its_label_and_others_do_not() {
    let (_, sent) = publish(&["post-created"], &video());
    assert_eq!(body(&sent[0]).get("paid_partnership"), None);
    let video = VideoUpload {
        paid_partnership: true,
        ..video()
    };
    let (_, sent) = publish(&["post-created"], &video);
    assert_eq!(
        body(&sent[0]).get("paid_partnership"),
        Some(&Value::Bool(true))
    );
}

#[test]
fn a_media_id_x_refuses_is_asked_about_before_saying_why() {
    let (post, sent) = publish(&["post-media-invalid", "status-not-found"], &video());
    assert_eq!(post.unwrap_err().kind, UploadErrorKind::Expired);
    assert_eq!(sent.len(), 2);
    let (post, _) = publish(&["post-media-invalid", "status-in-progress"], &video());
    assert_eq!(post.unwrap_err().kind, UploadErrorKind::NotReady);
    let (post, _) = publish(&["post-media-invalid", "status-succeeded"], &video());
    assert_eq!(post.unwrap_err().kind, UploadErrorKind::Invalid);
}

#[test]
fn post_errors_say_what_x_refused() {
    let (post, _) = publish(&["post-video-too-long"], &video());
    let error = post.unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Invalid);
    assert!(
        error.detail.contains("longer than 20 minutes"),
        "{}",
        error.detail
    );
    // A text the account already posted: its own kind, since after a
    // request whose end is not known it means that request made the post.
    let (post, _) = publish(&["post-duplicate"], &video());
    let error = post.unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Duplicate);
    assert!(error.detail.contains("duplicate content"));
    let (post, _) = publish(&["credits-depleted"], &video());
    assert_eq!(post.unwrap_err().kind, UploadErrorKind::QuotaExceeded);
    let (post, _) = publish(&["rate-limited"], &video());
    assert_eq!(post.unwrap_err().kind, UploadErrorKind::RateLimited);
}

/// An uploader whose transport fails with `error` on its first request.
fn failing(error: TransportError, then: &[&str]) -> XUploader<Scripted> {
    let mut answers = vec![Err(error)];
    answers.extend(then.iter().map(|name| Ok(fixture("x-upload", name))));
    XUploader::with_transport(Scripted::script(answers)).with_chunk(CHUNK)
}

#[test]
fn a_post_that_was_sent_and_got_no_answer_may_be_live_and_is_not_an_outage() {
    let uploader = failing(TransportError::unanswered("timeout: no answer"), &[]);
    let error = uploader
        .publish(&token(), USER_ID, MEDIA_ID, &video())
        .unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::AnswerLost);
    assert!(!error.kind.is_transient(), "the queue would post it again");
    assert_eq!(uploader.transport().sent().len(), 1, "asked once");
}

#[test]
fn a_post_that_never_left_is_an_outage_the_queue_retries() {
    let uploader = failing(TransportError::not_sent("dns error: no such host"), &[]);
    let error = uploader
        .publish(&token(), USER_ID, MEDIA_ID, &video())
        .unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unreachable);
    assert!(error.kind.is_transient());
}

#[test]
fn a_post_x_made_whose_answer_cannot_be_read_may_be_live() {
    let (post, sent) = publish(&["post-created-unreadable"], &video());
    assert_eq!(post.unwrap_err().kind, UploadErrorKind::AnswerLost);
    assert_eq!(sent.len(), 1);
}

#[test]
fn a_post_x_failed_on_is_a_failure_on_its_side() {
    let (post, _) = publish(&["post-bad-gateway"], &video());
    assert_eq!(post.unwrap_err().kind, UploadErrorKind::NetworkDown);
}

#[test]
fn every_other_step_without_an_answer_is_an_outage_to_retry() {
    let lost = || TransportError::unanswered("connection reset");
    let script = |answers: Vec<Result<bardo_ai::http::HttpResponse, TransportError>>| {
        XUploader::with_transport(Scripted::script(answers)).with_chunk(CHUNK)
    };
    let ok = |name: &str| Ok(fixture("x-upload", name));
    // The start of an upload: at worst X keeps one nobody finishes.
    let uploader = script(vec![Err(lost())]);
    let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unreachable);
    // A chunk X may have taken goes again with the same number.
    let uploader = script(vec![ok("initialize-ok"), Err(lost())]);
    let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unreachable);
    assert_eq!(uploader.transport().sent().len(), 2);
    // A finalize sent twice is asked about.
    let uploader = script(vec![Err(lost())]);
    let mut run = FakeRun::resuming(SIZE, Duration::from_secs(60 * 60));
    let error = uploader.upload(&video(), &mut run).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unreachable);
    // A status and a look for a post only read.
    let uploader = script(vec![Err(lost())]);
    let error = uploader.state(&token(), MEDIA_ID).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unreachable);
    let uploader = script(vec![Err(lost())]);
    let error = uploader
        .find_post(&token(), USER_ID, MEDIA_ID, SystemTime::now())
        .unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unreachable);
}

#[test]
fn the_post_and_the_look_for_it_go_through_their_own_connection() {
    let poster = Scripted::new(vec![
        fixture("x-upload", "post-created"),
        fixture("x-upload", "posts-none"),
    ]);
    let uploader = uploader(&["status-succeeded"]).with_poster(poster);
    uploader.state(&token(), MEDIA_ID).unwrap();
    uploader
        .publish(&token(), USER_ID, MEDIA_ID, &video())
        .unwrap();
    uploader
        .find_post(&token(), USER_ID, MEDIA_ID, noon())
        .unwrap();
    // The scripted transports fail the test on a request they do not
    // expect: the status went through the upload's, the rest did not.
    assert_eq!(uploader.transport().sent().len(), 1);
}

#[test]
fn a_post_x_made_whose_answer_has_no_id_may_be_live() {
    for body in ["{}", r#"{"data":{"id":1880028106020515999}}"#, ""] {
        let answer = bardo_ai::http::HttpResponse::from_recording(&format!(
            "HTTP/2 201 \ncontent-type: application/json\n\n{body}"
        ))
        .unwrap();
        let uploader = XUploader::with_transport(Scripted::new(vec![answer]));
        let error = uploader
            .publish(&token(), USER_ID, MEDIA_ID, &video())
            .unwrap_err();
        assert_eq!(error.kind, UploadErrorKind::AnswerLost, "{body:?}");
    }
}

/// 2026-11-25 12:00:00 UTC.
fn noon() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_608_000)
}

#[test]
fn a_lost_post_is_found_among_the_accounts_posts_by_its_video() {
    let uploader = uploader(&["posts-with-video"]);
    let post = uploader
        .find_post(&token(), USER_ID, MEDIA_ID, noon())
        .unwrap()
        .expect("the post with the video");
    assert_eq!(post.id, POST_ID);
    assert_eq!(
        post.link.unwrap().url(),
        format!("https://x.com/i/status/{POST_ID}")
    );
    assert_eq!(post.issue, None);

    let sent = uploader.transport().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, Method::Get);
    let (path, query) = sent[0].url.split_once('?').unwrap();
    assert_eq!(path, format!("{API}/2/users/{USER_ID}/tweets"));
    let query: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    assert_eq!(
        query,
        [
            pair("max_results", "100"),
            // Replies stay in: a post that opens with a mention may be one.
            pair("exclude", "retweets"),
            // A day before the post was asked for, by X's clock.
            pair("start_time", "2026-11-24T12:00:00Z"),
            pair("tweet.fields", "attachments"),
        ]
    );
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer fixture-access-token")
    );
}

#[test]
fn a_post_with_another_video_or_none_is_not_the_lost_one() {
    let uploader = uploader(&["posts-with-video", "posts-none"]);
    // No post carries this upload.
    let found = uploader.find_post(&token(), USER_ID, "1880028106020515", noon());
    assert_eq!(found.unwrap(), None);
    let found = uploader.find_post(&token(), USER_ID, MEDIA_ID, noon());
    assert_eq!(found.unwrap(), None);
}

#[test]
fn the_newest_post_with_the_video_is_the_one_whatever_kind_its_key_names() {
    let uploader = uploader(&["posts-same-video-twice"]);
    let post = uploader
        .find_post(&token(), USER_ID, MEDIA_ID, noon())
        .unwrap()
        .unwrap();
    assert_eq!(post.id, "1880028106020516030");
}

#[test]
fn a_look_reads_on_while_x_has_more_posts() {
    let uploader = uploader(&["posts-more-to-come", "posts-with-video"]);
    let post = uploader
        .find_post(&token(), USER_ID, MEDIA_ID, noon())
        .unwrap()
        .unwrap();
    assert_eq!(post.id, POST_ID);
    let sent = uploader.transport().sent();
    assert_eq!(sent.len(), 2);
    let token_of = |sent: &Sent| {
        form_urlencoded::parse(sent.url.split_once('?').unwrap().1.as_bytes())
            .find(|(name, _)| name == "pagination_token")
            .map(|(_, value)| value.into_owned())
    };
    assert_eq!(token_of(&sent[0]), None);
    assert_eq!(
        token_of(&sent[1]).as_deref(),
        Some("7140dibdnow9c7btw482gs8uvjq5c0lp26b8ipt9kkl2")
    );

    // The last page without it: the post is not there.
    let uploader = self::uploader(&["posts-more-to-come", "posts-none"]);
    let found = uploader.find_post(&token(), USER_ID, MEDIA_ID, noon());
    assert_eq!(found.unwrap(), None);
}

#[test]
fn a_look_that_cannot_read_every_post_does_not_say_none() {
    let uploader = uploader(&["posts-more-to-come"; 5]);
    let error = uploader
        .find_post(&token(), USER_ID, MEDIA_ID, noon())
        .unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unexpected);
    assert_eq!(uploader.transport().sent().len(), 5);
}

#[test]
fn an_answer_that_is_not_xs_list_does_not_say_none() {
    // "None" lets the post go again, so only X's own list may say it.
    let recorded = |raw: &str| bardo_ai::http::HttpResponse::from_recording(raw).unwrap();
    for (what, answer) in [
        ("only errors", fixture("x-upload", "posts-errors-only")),
        (
            "a page from something in between",
            recorded("HTTP/1.1 200 OK\ncontent-type: text/html\n\n<html>Sign in</html>"),
        ),
        (
            "nothing",
            recorded("HTTP/2 200 \ncontent-type: application/json\n\n"),
        ),
        (
            "an empty object",
            recorded("HTTP/2 200 \ncontent-type: application/json\n\n{}"),
        ),
        (
            "a post without a usable id",
            recorded(
                "HTTP/2 200 \ncontent-type: application/json\n\n\
                 {\"data\":[{\"id\":\"x\",\"attachments\":{\"media_keys\":[\"7_1880028106020515840\"]}}],\
                 \"meta\":{\"result_count\":1}}",
            ),
        ),
    ] {
        let uploader = XUploader::with_transport(Scripted::new(vec![answer]));
        let error = uploader
            .find_post(&token(), USER_ID, MEDIA_ID, noon())
            .unwrap_err();
        assert_eq!(error.kind, UploadErrorKind::Unexpected, "{what}");
    }
}

#[test]
fn looking_for_a_post_takes_only_x_ids_and_says_why_it_failed() {
    let uploader = uploader(&[]);
    assert!(
        uploader
            .find_post(&token(), "me/../x", MEDIA_ID, noon())
            .is_err()
    );
    assert!(
        uploader
            .find_post(&token(), USER_ID, "7_1880028106020515840", noon())
            .is_err()
    );
    assert!(uploader.transport().sent().is_empty());

    for (name, kind) in [
        ("rate-limited", UploadErrorKind::RateLimited),
        ("not-a-user-token", UploadErrorKind::Refused),
        ("post-bad-gateway", UploadErrorKind::NetworkDown),
    ] {
        let uploader = self::uploader(&[name]);
        let error = uploader
            .find_post(&token(), USER_ID, MEDIA_ID, noon())
            .unwrap_err();
        assert_eq!(error.kind, kind, "{name}");
    }
}

#[test]
fn processing_is_checked_once_a_minute_for_about_a_quarter_of_an_hour() {
    let uploader = uploader(&[]);
    assert_eq!(uploader.poll_delay(0), Duration::from_secs(60));
    assert_eq!(uploader.processing_polls(), 15);
}
