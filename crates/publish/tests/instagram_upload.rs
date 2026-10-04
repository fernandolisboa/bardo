//! Instagram Reels upload against recorded responses (see
//! `fixtures/instagram-upload/README.md`). No test calls Meta.

mod common;

use std::time::Duration;

use bardo_ai::http::Method;
use bardo_domain::{
    Network, PostLink, PublishingLimit, SecretText, UploadError, UploadErrorKind, UploadOutcome,
    UploadRun, UploadedVideo, VideoState, VideoUpload, VideoUploader, Visibility,
};
use bardo_publish::InstagramUploader;
use common::{Scripted, fixture};

const ACCOUNT: &str = "17841400008460056";
const CONTAINER: &str = "17895695668004550";
const NEW_CONTAINER: &str = "17895695668004551";
const SIZE: usize = 10;
const GRAPH: &str = "https://graph.facebook.com/v25.0";
const RUPLOAD: &str = "https://rupload.facebook.com/ig-api-upload/v25.0";

fn uploader(names: &[&str]) -> InstagramUploader<Scripted> {
    InstagramUploader::with_transport(Scripted::new(
        names
            .iter()
            .map(|name| fixture("instagram-upload", name))
            .collect(),
    ))
}

fn video() -> VideoUpload {
    VideoUpload {
        title: String::new(),
        description: "Fifty years out and still sending data home. #space #voyager".into(),
        tags: Vec::new(),
        visibility: Visibility::Public,
        made_for_kids: false,
        synthetic: true,
        publish_at: None,
        share_to_feed: false,
        cover: Duration::from_millis(2_500),
    }
}

fn token() -> SecretText {
    SecretText::new("EAAG-fixture-page-token")
}

/// A run over a file in memory that remembers what it was told to keep.
struct FakeRun {
    file: Vec<u8>,
    session: Option<String>,
    confirmed: Vec<u64>,
    account: String,
    stop: bool,
}

impl FakeRun {
    fn new() -> Self {
        Self {
            file: (0..SIZE as u8).collect(),
            session: None,
            confirmed: Vec::new(),
            account: ACCOUNT.to_owned(),
            stop: false,
        }
    }

    fn resuming(container: &str) -> Self {
        Self {
            session: Some(container.to_owned()),
            ..Self::new()
        }
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

    fn should_stop(&self) -> bool {
        self.stop
    }

    fn account(&self) -> &str {
        &self.account
    }
}

fn uploaded(id: &str) -> UploadOutcome {
    UploadOutcome::Uploaded(UploadedVideo { id: id.into() })
}

#[test]
fn a_new_upload_makes_a_reel_container_with_the_review_and_sends_the_whole_file() {
    let uploader = uploader(&["container-created", "rupload-ok"]);
    let mut run = FakeRun::new();

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(CONTAINER));
    assert_eq!(run.session.as_deref(), Some(CONTAINER));
    assert_eq!(run.confirmed, [0, SIZE as u64]);
    let sent = uploader.transport().sent();
    let create = &sent[0];
    assert_eq!(create.method, Method::Post);
    assert_eq!(create.url, format!("{GRAPH}/{ACCOUNT}/media"));
    assert_eq!(
        create.header("authorization"),
        Some("Bearer EAAG-fixture-page-token")
    );
    assert_eq!(create.field("media_type").as_deref(), Some("REELS"));
    assert_eq!(create.field("upload_type").as_deref(), Some("resumable"));
    assert_eq!(create.field("caption"), Some(video().description));
    assert_eq!(create.field("share_to_feed").as_deref(), Some("false"));
    assert_eq!(create.field("thumb_offset").as_deref(), Some("2500"));
    assert_eq!(create.field("is_ai_generated").as_deref(), Some("true"));
    assert!(
        create.field("access_token").is_none(),
        "the token goes in a header"
    );

    let file = &sent[1];
    assert_eq!(file.method, Method::Post);
    assert_eq!(file.url, format!("{RUPLOAD}/{CONTAINER}"));
    assert_eq!(
        file.header("authorization"),
        Some("OAuth EAAG-fixture-page-token")
    );
    assert_eq!(file.header("offset"), Some("0"));
    assert_eq!(file.header("file_size"), Some("10"));
    assert_eq!(file.bytes, run.file);
}

#[test]
fn a_reel_without_the_ai_label_does_not_send_one() {
    let uploader = uploader(&["container-created", "rupload-ok"]);
    let mut run = FakeRun::new();
    let video = VideoUpload {
        synthetic: false,
        share_to_feed: true,
        ..video()
    };

    uploader.upload(&video, &mut run).unwrap();

    let create = &uploader.transport().sent()[0];
    assert!(create.field("is_ai_generated").is_none());
    assert_eq!(create.field("share_to_feed").as_deref(), Some("true"));
}

#[test]
fn a_resumed_upload_sends_only_what_did_not_arrive() {
    let uploader = uploader(&["status-partial", "rupload-ok"]);
    let mut run = FakeRun::resuming(CONTAINER);

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(CONTAINER));
    assert_eq!(run.confirmed, [4, SIZE as u64]);
    let sent = uploader.transport().sent();
    assert_eq!(sent.len(), 2, "no new container");
    assert_eq!(sent[0].method, Method::Get);
    assert!(
        sent[0]
            .url
            .starts_with(&format!("{GRAPH}/{CONTAINER}?fields=")),
        "{}",
        sent[0].url
    );
    assert!(sent[0].url.contains("video_status"));
    assert_eq!(sent[1].header("offset"), Some("4"));
    assert_eq!(sent[1].header("file_size"), Some("10"));
    assert_eq!(sent[1].bytes, run.file[4..]);
}

#[test]
fn a_resumed_upload_nothing_of_which_arrived_sends_it_all_to_the_same_container() {
    let uploader = uploader(&["status-nothing-yet", "rupload-ok"]);
    let mut run = FakeRun::resuming(CONTAINER);

    uploader.upload(&video(), &mut run).unwrap();

    let sent = uploader.transport().sent();
    assert_eq!(sent[1].url, format!("{RUPLOAD}/{CONTAINER}"));
    assert_eq!(sent[1].header("offset"), Some("0"));
    assert_eq!(sent[1].bytes, run.file);
}

#[test]
fn a_container_that_has_the_whole_file_is_not_sent_again() {
    for status in [
        "status-processing",
        "status-finished",
        "status-finished-bare",
        "status-error",
    ] {
        let uploader = uploader(&[status]);
        let mut run = FakeRun::resuming(CONTAINER);

        assert_eq!(
            uploader.upload(&video(), &mut run).unwrap(),
            uploaded(CONTAINER),
            "{status}"
        );
        assert_eq!(uploader.transport().sent().len(), 1, "{status}");
    }
}

#[test]
fn an_expired_container_starts_over_in_a_new_one() {
    let uploader = uploader(&["status-expired", "container-recreated", "rupload-ok"]);
    let mut run = FakeRun::resuming(CONTAINER);

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(outcome, uploaded(NEW_CONTAINER));
    assert_eq!(run.session.as_deref(), Some(NEW_CONTAINER));
    let sent = uploader.transport().sent();
    assert_eq!(sent[1].url, format!("{GRAPH}/{ACCOUNT}/media"));
    assert_eq!(sent[2].url, format!("{RUPLOAD}/{NEW_CONTAINER}"));
    assert_eq!(sent[2].header("offset"), Some("0"));
    assert_eq!(sent[2].bytes, run.file);
}

#[test]
fn a_container_meta_no_longer_has_starts_over_in_a_new_one() {
    let uploader = uploader(&["status-not-found", "container-recreated", "rupload-ok"]);
    let mut run = FakeRun::resuming(CONTAINER);

    assert_eq!(
        uploader.upload(&video(), &mut run).unwrap(),
        uploaded(NEW_CONTAINER)
    );
}

#[test]
fn a_saved_session_that_is_not_a_container_id_is_never_put_in_an_address() {
    let uploader = uploader(&["container-created", "rupload-ok"]);
    let mut run = FakeRun::resuming("../me/accounts");

    uploader.upload(&video(), &mut run).unwrap();

    let sent = uploader.transport().sent();
    assert_eq!(sent[0].url, format!("{GRAPH}/{ACCOUNT}/media"));
}

#[test]
fn a_run_asked_to_stop_sends_nothing() {
    let uploader = uploader(&["status-partial"]);
    let mut run = FakeRun {
        stop: true,
        ..FakeRun::resuming(CONTAINER)
    };

    assert_eq!(
        uploader.upload(&video(), &mut run).unwrap(),
        UploadOutcome::Stopped
    );
    assert_eq!(uploader.transport().sent().len(), 1);
}

#[test]
fn an_upload_without_an_account_id_asks_to_reconnect() {
    let uploader = uploader(&[]);
    let mut run = FakeRun {
        account: String::new(),
        ..FakeRun::new()
    };

    let error = uploader.upload(&video(), &mut run).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::SignedOut);
    assert!(uploader.transport().sent().is_empty());
}

#[test]
fn a_refused_token_asks_to_reconnect_on_either_host() {
    let error = uploader(&["container-invalid-token"])
        .upload(&video(), &mut FakeRun::new())
        .unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Refused);
    assert!(!error.detail.contains("EAAG"), "no token in the log");

    let error = uploader(&["container-created", "rupload-not-authorized"])
        .upload(&video(), &mut FakeRun::new())
        .unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Refused, "{error}");
}

#[test]
fn a_retriable_upload_host_failure_is_worth_another_attempt() {
    let mut run = FakeRun::new();
    let error = uploader(&["container-created", "rupload-retriable"])
        .upload(&video(), &mut run)
        .unwrap_err();

    assert!(error.kind.is_transient(), "{error}");
    assert_eq!(run.session.as_deref(), Some(CONTAINER), "kept to resume");
}

#[test]
fn offline_is_unreachable() {
    let uploader = InstagramUploader::with_transport(Scripted::offline());
    let error = uploader.upload(&video(), &mut FakeRun::new()).unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unreachable);
}

#[test]
fn a_containers_status_code_says_where_the_reel_stands() {
    let state = |name: &str| uploader(&[name]).state(&token(), CONTAINER).unwrap();
    assert_eq!(state("status-processing"), VideoState::Processing);
    assert_eq!(state("status-finished"), VideoState::Processed);
    assert_eq!(state("status-expired"), VideoState::Expired);
    assert_eq!(
        state("status-error"),
        VideoState::Failed("Error: 2207026".into())
    );
    assert_eq!(
        state("status-published"),
        VideoState::Ready {
            visibility: Visibility::Public,
            publish_at: None,
            published_at: None,
        }
    );
    assert_eq!(state("status-not-found"), VideoState::Expired);
}

#[test]
fn the_publishing_limit_is_read_as_meta_reports_it() {
    let uploader = uploader(&["limit-room", "limit-full"]);

    let room = uploader.publishing_limit(&token(), ACCOUNT).unwrap();
    assert_eq!(
        room,
        Some(PublishingLimit {
            used: 2,
            total: 50,
            window: Duration::from_secs(24 * 60 * 60),
        })
    );
    assert!(room.unwrap().has_room());
    let full = uploader
        .publishing_limit(&token(), ACCOUNT)
        .unwrap()
        .unwrap();
    assert!(!full.has_room());

    let sent = uploader.transport().sent();
    assert!(
        sent[0]
            .url
            .starts_with(&format!("{GRAPH}/{ACCOUNT}/content_publishing_limit?")),
        "{}",
        sent[0].url
    );
}

#[test]
fn a_limit_window_out_of_reason_is_not_trusted() {
    let error = uploader(&["limit-odd-window"])
        .publishing_limit(&token(), ACCOUNT)
        .unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::Unexpected);
}

#[test]
fn a_limit_read_without_permission_is_not_allowed() {
    let error = uploader(&["limit-no-permission"])
        .publishing_limit(&token(), ACCOUNT)
        .unwrap_err();
    assert_eq!(error.kind, UploadErrorKind::NotAllowed);
}

#[test]
fn publishing_a_finished_container_gives_the_reels_link() {
    let uploader = uploader(&["publish-ok", "permalink"]);

    let post = uploader.publish(&token(), ACCOUNT, CONTAINER).unwrap();

    assert_eq!(
        post.link,
        Some(
            PostLink::parse(
                Network::InstagramReels,
                "https://www.instagram.com/reel/C1aBcDeFgHi/"
            )
            .unwrap()
        )
    );
    assert_eq!(post.issue, None);
    let sent = uploader.transport().sent();
    assert_eq!(sent[0].url, format!("{GRAPH}/{ACCOUNT}/media_publish"));
    assert_eq!(sent[0].field("creation_id").as_deref(), Some(CONTAINER));
    assert!(
        sent[1]
            .url
            .starts_with(&format!("{GRAPH}/17920238422030506?fields="))
    );
}

#[test]
fn what_instagram_published_without_comes_back_with_the_post() {
    let post = uploader(&["publish-config-issue", "permalink"])
        .publish(&token(), ACCOUNT, CONTAINER)
        .unwrap();

    assert_eq!(
        post.issue.as_deref(),
        Some("The audio could not be added to the reel")
    );
    assert!(post.link.is_some());
}

#[test]
fn a_post_whose_link_cannot_be_read_is_published_all_the_same() {
    let post = uploader(&["publish-ok", "rate-limited"])
        .publish(&token(), ACCOUNT, CONTAINER)
        .unwrap();
    assert_eq!(post.link, None);
}

#[test]
fn publish_refusals_say_why() {
    let refusal = |name: &str| {
        uploader(&[name])
            .publish(&token(), ACCOUNT, CONTAINER)
            .unwrap_err()
            .kind
    };
    assert_eq!(refusal("publish-not-ready"), UploadErrorKind::NotReady);
    assert_eq!(refusal("publish-limit"), UploadErrorKind::UploadLimit);
    assert_eq!(refusal("publish-expired"), UploadErrorKind::Expired);
    assert_eq!(refusal("rate-limited"), UploadErrorKind::RateLimited);
    assert_eq!(refusal("container-invalid-token"), UploadErrorKind::Refused);
}

#[test]
fn ids_that_are_not_meta_ids_never_reach_an_address() {
    let uploader = uploader(&[]);
    assert!(uploader.publish(&token(), ACCOUNT, "1/../2").is_err());
    assert!(uploader.publish(&token(), "me", CONTAINER).is_err());
    assert!(uploader.state(&token(), "abc").is_err());
    assert!(uploader.publishing_limit(&token(), "").is_err());
    assert!(uploader.transport().sent().is_empty());
}

#[test]
fn reels_are_checked_once_a_minute_for_a_quarter_of_an_hour() {
    let uploader = uploader(&[]);
    assert_eq!(uploader.poll_delay(0), Duration::from_secs(60));
    assert_eq!(uploader.poll_delay(9), Duration::from_secs(60));
    assert_eq!(uploader.processing_polls(), 15);
}
