//! X video upload and post (#147, ADR-0008): the media upload of the X API
//! v2 in chunks, then the post Bardo makes with it, at once or at its due
//! time (the in-app scheduler, ADR-0006).
//!
//! - `POST /2/media/upload/initialize` with the file's type, size and
//!   `tweet_video` answers the media id and how long X keeps the upload
//!   (`expires_after_secs`, 24 hours in the reference's example).
//! - Each chunk goes to `POST /2/media/upload/{id}/append` as a multipart
//!   form with `segment_index` (from 0) and `media`; X recommends chunks of
//!   at most 5 MB. X cannot be asked what arrived, so a resumed upload goes
//!   on after the last chunk the run kept as confirmed; an upload past its
//!   time, or one X no longer knows, starts over with a new one.
//! - `POST /2/media/upload/{id}/finalize` closes it. X then processes the
//!   video: `GET /2/media/upload?command=STATUS&media_id={id}` says
//!   `pending`, `in_progress`, `succeeded` or `failed`, with
//!   `check_after_secs`. No `processing_info` means it is ready.
//! - `POST /2/tweets` with the text, `media.media_ids` and, for synthetic
//!   content, `made_with_ai` makes the post. X bills it (US$ 0.015, or
//!   US$ 0.20 with a link in the text); the upload itself is not billed.
//!   A video over the account's length limit is refused there with 403,
//!   even after a finalize that went through.
//! - The post is the one request that cannot be made twice. One that was
//!   sent and got no answer may be live: Bardo does not send it again by
//!   itself (ADR-0030). `GET /2/users/{id}/tweets` with
//!   `tweet.fields=attachments` lists the account's own posts with their
//!   media keys, so the post made with an upload can be found. X's
//!   examples write a key as a kind, an underscore and the media's id
//!   (`7_1880…`); the rule is not spelled out, so only the id is matched.
//!   X bills each post it returns as an owned read (US$ 0.001), up to 100
//!   an answer.
//!
//! Docs checked 2026-10-14:
//! <https://docs.x.com/x-api/media/quickstart/media-upload-chunked>,
//! <https://docs.x.com/x-api/media/initialize-media-upload>,
//! <https://docs.x.com/x-api/media/append-media-upload>,
//! <https://docs.x.com/x-api/media/finalize-media-upload>,
//! <https://docs.x.com/x-api/media/get-media-upload-status>,
//! <https://docs.x.com/x-api/posts/create-post>,
//! <https://docs.x.com/x-api/getting-started/pricing>.
//! Checked 2026-11-25: <https://docs.x.com/x-api/users/get-posts>,
//! <https://docs.x.com/x-api/fundamentals/data-dictionary> (media keys),
//! and the owned reads of the pricing page.

use std::time::{Duration, SystemTime};

use bardo_ai::http::{FormPart, HttpRequest, HttpResponse, Method, Transport, UreqTransport};
use bardo_domain::{
    Network, NetworkPost, PostLink, Rfc3339, SecretText, UploadError, UploadErrorKind,
    UploadOutcome, UploadRun, UploadedVideo, VideoState, VideoUpload, VideoUploader,
};
use serde_json::{Value, json};

use crate::x::{XEndpoints, api_detail, is_user_id, problem_type};

/// Bytes per chunk: within the 5 MB X recommends.
pub const X_CHUNK: u64 = 4 * 1024 * 1024;

/// The most `segment_index` takes is 9999, so a file of more chunks than
/// that cannot go; at 4 MiB that is about 39 GiB, past X's 16 GB anyway.
const MAX_SEGMENTS: u64 = 10_000;

/// How long X keeps an upload when its answer does not say: a day, as in
/// the reference's example.
const DEFAULT_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// How long before its end an upload is no longer resumed: a chunk sent at
/// the last minute could be cut off, and the post needs it still there.
const LIFETIME_MARGIN: Duration = Duration::from_secs(10 * 60);

/// How far before the time it was asked for a lost post is looked for: X
/// goes by its own clock, and the computer's may be off by hours (a wrong
/// time zone set by hand).
const LOOKBACK: Duration = Duration::from_secs(24 * 60 * 60);

/// The most posts X lists in one answer.
const LISTED_POSTS: u32 = 100;

/// How many answers a look reads at most. An account with more posts of
/// its own than that since the lost one is not looked through: the look
/// fails, and nothing is posted on a guess.
const LISTED_PAGES: u32 = 5;

/// How many uploads one call starts at most, its first one included,
/// before it gives up on uploads X keeps dropping.
const MAX_NEW_SESSIONS: u32 = 2;

/// How long between checks on a video X is processing: X suggests a few
/// seconds (`check_after_secs`), and a short video is usually ready by the
/// first check.
const POLL_DELAY: Duration = Duration::from_secs(60);

/// Checks before the post shows "still processing": about a quarter of an
/// hour.
const PROCESSING_POLLS: u32 = 15;

/// Bardo renders MP4 files.
const CONTENT_TYPE: &str = "video/mp4";

/// An upload X started: its media id and until when X keeps it. Kept by
/// the run as one line of JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    media_id: String,
    /// Unix seconds.
    expires_at: u64,
}

impl Session {
    fn encode(&self) -> String {
        json!({ "media_id": self.media_id, "expires_at": self.expires_at }).to_string()
    }

    fn decode(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(Self {
            media_id: value["media_id"].as_str()?.to_owned(),
            expires_at: value["expires_at"].as_u64()?,
        })
        .filter(|session| is_media_id(&session.media_id))
    }

    /// Whether a chunk may still go to it at `now`.
    fn is_fresh(&self, now: SystemTime) -> bool {
        let end = SystemTime::UNIX_EPOCH + Duration::from_secs(self.expires_at);
        now + LIFETIME_MARGIN < end
    }
}

/// Uploads videos to X and posts them, over HTTPS.
pub struct XUploader<T = UreqTransport> {
    transport: T,
    /// What the post, and a look for it, go through when not `transport`:
    /// a new connection for every request, and a short wait. A kept
    /// connection that died unnoticed takes the request before it fails,
    /// which would make a post that never arrived look like one that may
    /// be live.
    poster: Option<T>,
    endpoints: XEndpoints,
    /// Bytes per chunk.
    chunk: u64,
}

impl XUploader {
    /// Long enough for one chunk on a slow line (4 MiB at 1 Mbit/s takes
    /// about half a minute).
    pub const TIMEOUT: Duration = Duration::from_secs(5 * 60);

    /// How long the post, and a look for it, wait for an answer: they
    /// carry a text, not the file.
    pub const POST_TIMEOUT: Duration = Duration::from_secs(60);

    pub fn new() -> Self {
        Self {
            poster: Some(UreqTransport::for_posts(Self::POST_TIMEOUT)),
            ..Self::with_transport(UreqTransport::for_uploads(Self::TIMEOUT))
        }
    }
}

impl Default for XUploader {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> XUploader<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            poster: None,
            endpoints: XEndpoints::default(),
            chunk: X_CHUNK,
        }
    }

    /// The post, and a look for it, through `poster` instead of the
    /// transport of the upload.
    pub fn with_poster(mut self, poster: T) -> Self {
        self.poster = Some(poster);
        self
    }

    pub fn with_endpoints(mut self, endpoints: XEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    /// Chunks of `bytes`, for tests with small files.
    pub fn with_chunk(mut self, bytes: u64) -> Self {
        self.chunk = bytes.max(1);
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// An API call as the token's owner; its JSON answer. A request that
    /// was sent, or may have been, and got no answer is `AnswerLost`; one
    /// that never left is `Unreachable`.
    fn call(
        &self,
        transport: &T,
        request: HttpRequest,
        token: &SecretText,
    ) -> Result<Value, UploadError> {
        let request = request.header("authorization", format!("Bearer {}", token.expose()));
        let response = transport.send(&request).map_err(|error| {
            let kind = if error.never_left() {
                UploadErrorKind::Unreachable
            } else {
                UploadErrorKind::AnswerLost
            };
            UploadError::new(kind, error.0)
        })?;
        let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
        if !(200..=299).contains(&response.status) {
            return Err(api_failure(&response, &body));
        }
        Ok(body)
    }

    /// `call` for a request X takes again without harm (every one but the
    /// post): no answer is an outage, whether the request left or not.
    fn api(&self, request: HttpRequest, token: &SecretText) -> Result<Value, UploadError> {
        self.call(&self.transport, request, token)
            .map_err(may_go_again)
    }

    /// What the post goes through.
    fn poster(&self) -> &T {
        self.poster.as_ref().unwrap_or(&self.transport)
    }

    fn media_url(&self, rest: &str) -> String {
        format!("{}/2/media/upload{rest}", self.endpoints.api)
    }

    /// Starts an upload of `size` bytes.
    fn initialize(&self, size: u64, token: &SecretText) -> Result<Session, UploadError> {
        let body = self.api(
            HttpRequest::post_json(
                self.media_url("/initialize"),
                json!({
                    "media_type": CONTENT_TYPE,
                    "total_bytes": size,
                    "media_category": "tweet_video",
                })
                .to_string(),
            ),
            token,
        )?;
        let data = &body["data"];
        let media_id = data["id"]
            .as_str()
            .filter(|id| is_media_id(id))
            .ok_or_else(|| unexpected("the initialize answer has no media id"))?;
        let lifetime = data["expires_after_secs"]
            .as_u64()
            .filter(|secs| *secs > 0)
            .map_or(DEFAULT_LIFETIME, Duration::from_secs);
        let expires_at = (SystemTime::now() + lifetime)
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Ok(Session {
            media_id: media_id.to_owned(),
            expires_at,
        })
    }

    /// Sends chunk number `index` of the upload. `Ok(false)` when X no
    /// longer knows the upload.
    fn append(
        &self,
        session: &Session,
        index: u64,
        bytes: &[u8],
        token: &SecretText,
    ) -> Result<bool, UploadError> {
        let index = index.to_string();
        let request = HttpRequest::post_multipart(
            self.media_url(&format!("/{}/append", session.media_id)),
            &[
                FormPart::Text {
                    name: "segment_index",
                    value: &index,
                },
                FormPart::File {
                    name: "media",
                    file_name: "video.mp4",
                    content_type: "application/octet-stream",
                    bytes,
                },
            ],
        );
        match self.api(request, token) {
            Ok(_) => Ok(true),
            Err(error) if error.kind == UploadErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Closes the upload once every chunk arrived. `Ok(false)` when X no
    /// longer knows the upload.
    fn finalize(&self, session: &Session, token: &SecretText) -> Result<bool, UploadError> {
        let request = HttpRequest {
            method: Method::Post,
            url: self.media_url(&format!("/{}/finalize", session.media_id)),
            headers: Vec::new(),
            body: None,
        };
        match self.api(request, token) {
            Ok(_) => Ok(true),
            Err(error) if error.kind == UploadErrorKind::NotFound => Ok(false),
            // Finalized already, by a run cut short before it kept that:
            // the status says whether X has the whole file.
            Err(error) if error.kind == UploadErrorKind::Invalid => {
                match self.state(token, &session.media_id)? {
                    VideoState::Expired => Ok(false),
                    _ => Ok(true),
                }
            }
            Err(error) => Err(error),
        }
    }
}

impl<T: Transport> VideoUploader for XUploader<T> {
    fn network(&self) -> Network {
        Network::X
    }

    /// Sends the file. The post's text goes with the post, not the upload
    /// (`publish`).
    fn upload(
        &self,
        _video: &VideoUpload,
        run: &mut dyn UploadRun,
    ) -> Result<UploadOutcome, UploadError> {
        let size = run.size();
        if size == 0 {
            return Err(UploadError::new(
                UploadErrorKind::Local,
                "the file is empty",
            ));
        }
        if size.div_ceil(self.chunk) > MAX_SEGMENTS {
            return Err(UploadError::new(
                UploadErrorKind::Invalid,
                "the file has more chunks than X takes",
            ));
        }
        let mut session = run.session().and_then(|text| Session::decode(&text));
        // Kept after a whole chunk only; anything else starts over.
        let mut confirmed = match &session {
            Some(_) => run.resumed(),
            None => 0,
        };
        if confirmed % self.chunk != 0 && confirmed != size {
            session = None;
            confirmed = 0;
        }
        let mut new_sessions = 0;
        loop {
            if run.should_stop() {
                return Ok(UploadOutcome::Stopped);
            }
            if session
                .as_ref()
                .is_some_and(|current| !current.is_fresh(SystemTime::now()))
            {
                session = None;
            }
            let current = match &session {
                Some(current) => current.clone(),
                None => {
                    if new_sessions == MAX_NEW_SESSIONS {
                        return Err(unexpected("X keeps dropping the upload"));
                    }
                    new_sessions += 1;
                    let started = self.initialize(size, &run.access_token()?)?;
                    run.session_started(&started.encode())?;
                    confirmed = 0;
                    run.confirmed(0)?;
                    session = Some(started.clone());
                    started
                }
            };
            if confirmed >= size {
                if self.finalize(&current, &run.access_token()?)? {
                    return Ok(UploadOutcome::Uploaded(UploadedVideo {
                        id: current.media_id,
                    }));
                }
                session = None;
                continue;
            }
            let len = self.chunk.min(size - confirmed);
            let bytes = run.read(confirmed, len as usize)?;
            if bytes.len() as u64 != len {
                return Err(UploadError::new(
                    UploadErrorKind::Local,
                    "the file is shorter than when the upload started",
                ));
            }
            let index = confirmed / self.chunk;
            if self.append(&current, index, &bytes, &run.access_token()?)? {
                confirmed += len;
                run.confirmed(confirmed)?;
            } else {
                session = None;
            }
        }
    }

    fn state(&self, access_token: &SecretText, id: &str) -> Result<VideoState, UploadError> {
        if !is_media_id(id) {
            return Err(unexpected("not an X media id"));
        }
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("command", "STATUS")
            .append_pair("media_id", id)
            .finish();
        let body = match self.api(
            HttpRequest::get(self.media_url(&format!("?{query}"))),
            access_token,
        ) {
            Ok(body) => body,
            // Gone before it was posted (X keeps an upload a day): the
            // file goes again.
            Err(error) if error.kind == UploadErrorKind::NotFound => {
                return Ok(VideoState::Expired);
            }
            Err(error) => return Err(error),
        };
        let processing = &body["data"]["processing_info"];
        if processing.is_null() {
            return Ok(VideoState::Processed);
        }
        Ok(match processing["state"].as_str().unwrap_or_default() {
            "pending" | "in_progress" => VideoState::Processing,
            "succeeded" => VideoState::Processed,
            "failed" => VideoState::Failed(
                processing["error"]["message"]
                    .as_str()
                    .or_else(|| processing["error"]["name"].as_str())
                    .map(|reason| crate::text::plain(reason, 100))
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or_else(|| "failed".to_owned()),
            ),
            other => {
                return Err(unexpected(&format!(
                    "unknown processing state {:?}",
                    crate::text::plain(other, 40)
                )));
            }
        })
    }

    /// Makes the post with the processed video `id`: `video`'s text, and
    /// X's AI label when it holds synthetic content.
    fn publish(
        &self,
        access_token: &SecretText,
        _account: &str,
        id: &str,
        video: &VideoUpload,
    ) -> Result<NetworkPost, UploadError> {
        if !is_media_id(id) {
            return Err(unexpected("not an X media id"));
        }
        let mut post = json!({
            "text": video.description,
            "media": { "media_ids": [id] },
        });
        if video.synthetic {
            post["made_with_ai"] = Value::Bool(true);
        }
        let request =
            HttpRequest::post_json(format!("{}/2/tweets", self.endpoints.api), post.to_string());
        let body = match self.call(self.poster(), request, access_token) {
            Ok(body) => body,
            // The media id is not usable any more (or not yet): ask where
            // it stands, so an expired one goes again and one still
            // processing waits.
            Err(error) if error.kind == UploadErrorKind::Invalid && mentions_media(&error) => {
                return Err(match self.state(access_token, id)? {
                    VideoState::Processing => {
                        UploadError::new(UploadErrorKind::NotReady, error.detail)
                    }
                    VideoState::Expired => UploadError::new(UploadErrorKind::Expired, error.detail),
                    _ => error,
                });
            }
            Err(error) => return Err(error),
        };
        // X took the post and its answer does not say which one it is.
        let post_id = body["data"]["id"]
            .as_str()
            .filter(|id| is_media_id(id))
            .ok_or_else(|| {
                UploadError::new(UploadErrorKind::AnswerLost, "the post answer has no id")
            })?;
        Ok(posted(post_id))
    }

    /// The account's newest post with the uploaded video `id`, among its
    /// own posts (no replies or reposts) from a day before `since` on.
    fn find_post(
        &self,
        access_token: &SecretText,
        account: &str,
        id: &str,
        since: SystemTime,
    ) -> Result<Option<NetworkPost>, UploadError> {
        if !is_media_id(id) {
            return Err(unexpected("not an X media id"));
        }
        if !is_user_id(account) {
            return Err(unexpected("not an X user id"));
        }
        let from = since
            .checked_sub(LOOKBACK)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mut page: Option<String> = None;
        for _ in 0..LISTED_PAGES {
            let mut query = form_urlencoded::Serializer::new(String::new());
            query
                .append_pair("max_results", &LISTED_POSTS.to_string())
                // Replies stay in: X may keep a post that opens with a
                // mention as one.
                .append_pair("exclude", "retweets")
                .append_pair("start_time", &Rfc3339(from).to_string())
                .append_pair("tweet.fields", "attachments");
            if let Some(token) = &page {
                query.append_pair("pagination_token", token);
            }
            // With the post's short wait: it runs right after a post that
            // got no answer, when the line may still be down.
            let request = HttpRequest::get(format!(
                "{}/2/users/{account}/tweets?{}",
                self.endpoints.api,
                query.finish()
            ));
            let body = self
                .call(self.poster(), request, access_token)
                .map_err(may_go_again)?;
            // Only X's own list says the post is not there: any other
            // answer (a page from something in between, a list of errors)
            // says nothing, and "none" would let the post go again.
            let posts = match body["data"].as_array() {
                Some(posts) => posts.as_slice(),
                None if body["meta"]["result_count"].as_u64() == Some(0) => &[],
                None => return Err(unexpected("the posts answer is not a list of posts")),
            };
            let found = posts.iter().find(|post| {
                post["attachments"]["media_keys"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .any(|key| is_key_of(key, id))
            });
            if let Some(post) = found {
                let post_id = post["id"]
                    .as_str()
                    .filter(|post_id| is_media_id(post_id))
                    .ok_or_else(|| unexpected("the post with the video has no id"))?;
                return Ok(Some(posted(post_id)));
            }
            page = body["meta"]["next_token"]
                .as_str()
                .filter(|token| !token.is_empty())
                .map(str::to_owned);
            if page.is_none() {
                return Ok(None);
            }
        }
        Err(unexpected("too many posts to look for the one made"))
    }

    fn processing_polls(&self) -> u32 {
        PROCESSING_POLLS
    }

    fn poll_delay(&self, _polls: u32) -> Duration {
        POLL_DELAY
    }
}

/// The failure of a request X takes again without harm: one that got no
/// answer is an outage like one that never left.
fn may_go_again(error: UploadError) -> UploadError {
    match error.kind {
        UploadErrorKind::AnswerLost => UploadError::new(UploadErrorKind::Unreachable, error.detail),
        _ => error,
    }
}

/// The post `id`, with its address.
fn posted(id: &str) -> NetworkPost {
    NetworkPost {
        id: id.to_owned(),
        link: PostLink::parse(Network::X, &format!("https://x.com/i/status/{id}")).ok(),
        issue: None,
    }
}

/// Whether a post's media key names the uploaded media `id`: X writes a
/// key as a kind, an underscore and the media's id (`7_1880…` for a video).
fn is_key_of(key: &str, id: &str) -> bool {
    key.rsplit_once('_').map_or(key, |(_, media)| media) == id
}

/// A media or post id as X hands them out: 1 to 19 digits.
fn is_media_id(id: &str) -> bool {
    crate::x::is_user_id(id)
}

/// Whether X's refusal is about the media it was given.
fn mentions_media(error: &UploadError) -> bool {
    error.detail.to_ascii_lowercase().contains("media")
}

fn unexpected(detail: &str) -> UploadError {
    UploadError::new(UploadErrorKind::Unexpected, detail)
}

/// Classifies an API error answer.
fn api_failure(response: &HttpResponse, body: &Value) -> UploadError {
    let problem = problem_type(body);
    let kind = match (response.status, problem) {
        (401, _) | (403, "unsupported-authentication") => UploadErrorKind::Refused,
        // The app's credits ran out (or its spending limit was reached).
        (402, _) | (_, "credits" | "usage-capped") => UploadErrorKind::QuotaExceeded,
        (404, _) | (_, "resource-not-found") => UploadErrorKind::NotFound,
        (403, "client-forbidden" | "client-not-enrolled") => UploadErrorKind::NotAllowed,
        (429, _) => UploadErrorKind::RateLimited,
        (500..=599, _) => UploadErrorKind::NetworkDown,
        // Refused as sent: a video over the account's length, a text X
        // does not take, an upload already finalized.
        (400 | 403, _) => UploadErrorKind::Invalid,
        _ => UploadErrorKind::Unexpected,
    };
    let detail = api_detail(response.status, body);
    // X has no problem type for a text the account already posted: it
    // says so in words ("... duplicate content.").
    let kind = match kind {
        UploadErrorKind::Invalid
            if response.status == 403 && detail.to_ascii_lowercase().contains("duplicate") =>
        {
            UploadErrorKind::Duplicate
        }
        kind => kind,
    };
    UploadError::new(kind, detail)
}
