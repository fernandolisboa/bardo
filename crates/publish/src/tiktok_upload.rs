//! TikTok draft upload (ADR-0008): the Content Posting API's upload to
//! the creator's inbox (`video.upload`), which the creator finishes and
//! posts in the TikTok app.
//!
//! - `POST /v2/post/publish/inbox/video/init/` with `source=FILE_UPLOAD`,
//!   the file's size and how it is cut (`ChunkPlan`) answers a
//!   `publish_id` and an `upload_url`, which lasts an hour. At most 6 inits
//!   a minute per token.
//! - The chunks go to the `upload_url` in order, each a `PUT` with
//!   `Content-Range: bytes FIRST-LAST/TOTAL`; TikTok answers 206 for each
//!   but the last, which it answers 201. The upload address carries its own
//!   token, so the user's token never goes there.
//! - TikTok cannot be asked what arrived, so a resumed upload goes on after
//!   the last chunk the run kept as confirmed. An upload address past its
//!   hour, or one TikTok refuses as expired (403) or does not know, starts
//!   over with a new init; a call makes two inits at most, its first one
//!   included.
//! - `POST /v2/post/publish/status/fetch/` says where the draft stands:
//!   `PROCESSING_UPLOAD`, then `SEND_TO_USER_INBOX` once it waits in the
//!   inbox, or `FAILED` with a `fail_reason`. At most 30 a minute per
//!   token; the queue's checks are far apart.
//! - At most 5 shares may wait in an inbox in 24 hours: a sixth init is
//!   refused with `spam_risk_too_many_pending_share` (`UploadLimit`).
//!
//! Docs checked 2026-10-04: <https://developers.tiktok.com/doc/content-posting-api-reference-upload-video>,
//! <https://developers.tiktok.com/doc/content-posting-api-media-transfer-guide>,
//! <https://developers.tiktok.com/doc/content-posting-api-reference-get-video-status>.

use std::time::{Duration, SystemTime};

use bardo_ai::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use bardo_domain::{
    ChunkPlan, Network, SecretText, TIKTOK_CHUNK, TIKTOK_MAX_BYTES, UploadError, UploadErrorKind,
    UploadOutcome, UploadRun, UploadedVideo, VideoState, VideoUpload, VideoUploader,
};
use serde_json::{Value, json};

use crate::TikTokEndpoints;
use crate::text::{SHOWN_TEXT, plain};
use crate::tiktok::detail;

/// How long an upload address lasts once TikTok handed it out.
const UPLOAD_URL_LIFETIME: Duration = Duration::from_secs(60 * 60);

/// How long before its hour an upload address is no longer used to start
/// a chunk: a chunk sent at the last minute could be cut off.
const UPLOAD_URL_MARGIN: Duration = Duration::from_secs(5 * 60);

/// How many inits one call makes at most, its first one included, before
/// it gives up on upload addresses TikTok keeps dropping: each init may
/// hold one of the creator's 5 pending drafts.
const MAX_NEW_SESSIONS: u32 = 2;

/// How many chunks in a row may come back without TikTok taking more of
/// the file, before the call gives up instead of looping.
const MAX_STALLED_CHUNKS: u32 = 3;

/// Bardo renders MP4 files.
const CONTENT_TYPE: &str = "video/mp4";

/// An upload TikTok started: its draft, the address the chunks go to and
/// when TikTok handed it out. Kept by the run as one line of JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    publish_id: String,
    upload_url: String,
    /// Unix milliseconds.
    issued_at: u64,
}

impl Session {
    fn encode(&self) -> String {
        json!({
            "publish_id": self.publish_id,
            "upload_url": self.upload_url,
            "issued_at": self.issued_at,
        })
        .to_string()
    }

    fn decode(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(Self {
            publish_id: value["publish_id"].as_str()?.to_owned(),
            upload_url: value["upload_url"].as_str()?.to_owned(),
            issued_at: value["issued_at"].as_u64()?,
        })
    }

    /// Whether a chunk may still start on its address at `now`.
    fn is_fresh(&self, now: SystemTime) -> bool {
        let issued = SystemTime::UNIX_EPOCH + Duration::from_millis(self.issued_at);
        issued + UPLOAD_URL_LIFETIME > now + UPLOAD_URL_MARGIN && issued <= now + UPLOAD_URL_MARGIN
    }
}

/// How TikTok answered a chunk.
#[derive(Debug, PartialEq, Eq)]
enum Chunk {
    /// Taken; more chunks are due.
    Taken,
    /// The whole file arrived.
    Complete,
    /// TikTok has the file up to `next`, not where the chunk started.
    Elsewhere { next: u64 },
    /// The upload address expired, or TikTok does not know it.
    Dropped,
}

/// Uploads drafts to TikTok over HTTPS.
pub struct TikTokUploader<T = UreqTransport> {
    transport: T,
    endpoints: TikTokEndpoints,
    /// Bytes per chunk (`ChunkPlan`).
    chunk: u64,
}

impl TikTokUploader {
    /// Long enough for one chunk on a slow line (10 MiB at 1 Mbit/s takes
    /// under a minute and a half).
    pub const TIMEOUT: Duration = Duration::from_secs(5 * 60);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::for_uploads(Self::TIMEOUT))
    }
}

impl Default for TikTokUploader {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> TikTokUploader<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: TikTokEndpoints::default(),
            chunk: TIKTOK_CHUNK,
        }
    }

    pub fn with_endpoints(mut self, endpoints: TikTokEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    /// Chunks of `bytes` (within TikTok's 5 to 64 MB, or the plan is
    /// refused).
    pub fn with_chunk(mut self, bytes: u64) -> Self {
        self.chunk = bytes;
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, UploadError> {
        self.transport
            .send(request)
            .map_err(|error| UploadError::new(UploadErrorKind::Unreachable, error.0))
    }

    /// A Content Posting API call as the token's owner, with a JSON body;
    /// its `data`. An answer whose `error.code` is not `ok` is a failure.
    fn api(&self, path: &str, body: Value, token: &SecretText) -> Result<Value, UploadError> {
        let request =
            HttpRequest::post_json(format!("{}{path}", self.endpoints.api), body.to_string())
                .header("authorization", format!("Bearer {}", token.expose()));
        // `post_json` names the type; TikTok's reference adds the charset.
        let request = HttpRequest {
            headers: request
                .headers
                .into_iter()
                .map(|(name, value)| {
                    if name.eq_ignore_ascii_case("content-type") {
                        (name, "application/json; charset=UTF-8".to_owned())
                    } else {
                        (name, value)
                    }
                })
                .collect(),
            ..request
        };
        let response = self.send(&request)?;
        let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
        let code = body["error"]["code"].as_str().unwrap_or_default();
        if !(200..=299).contains(&response.status) || code != "ok" {
            return Err(api_failure(&response, &body));
        }
        Ok(body["data"].clone())
    }

    /// Starts an upload of a file cut as `plan`.
    fn init(&self, plan: &ChunkPlan, token: &SecretText) -> Result<Session, UploadError> {
        let data = self.api(
            "/v2/post/publish/inbox/video/init/",
            json!({
                "source_info": {
                    "source": "FILE_UPLOAD",
                    "video_size": plan.video_size,
                    "chunk_size": plan.chunk_size,
                    "total_chunk_count": plan.total_chunk_count,
                }
            }),
            token,
        )?;
        let publish_id = data["publish_id"]
            .as_str()
            .filter(|id| is_publish_id(id))
            .ok_or_else(|| unexpected("the init answer has no publish id"))?;
        let upload_url = data["upload_url"]
            .as_str()
            .filter(|url| self.is_upload_url(url))
            .ok_or_else(|| unexpected("the init answer has no upload address on TikTok's hosts"))?;
        Ok(Session {
            publish_id: publish_id.to_owned(),
            upload_url: upload_url.to_owned(),
            issued_at: millis(SystemTime::now()),
        })
    }

    /// Whether `url` is an upload address on TikTok's own hosts over
    /// HTTPS (or on the API's address, for a fake server): the file goes
    /// there.
    fn is_upload_url(&self, url: &str) -> bool {
        if url.len() > 512 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return false;
        }
        let api = self.endpoints.api.trim_end_matches('/');
        if url
            .strip_prefix(api)
            .is_some_and(|rest| rest.starts_with('/'))
        {
            return true;
        }
        let Some(rest) = url.strip_prefix("https://") else {
            return false;
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.contains(['@', ':']) {
            return false;
        }
        authority.ends_with(".tiktokapis.com")
    }

    /// Sends `bytes` from `first` of a file of `size` bytes.
    fn send_chunk(
        &self,
        session: &Session,
        first: u64,
        bytes: Vec<u8>,
        size: u64,
    ) -> Result<Chunk, UploadError> {
        let last = first + bytes.len() as u64 - 1;
        let request = HttpRequest::put(session.upload_url.as_str(), bytes)
            .header("content-type", CONTENT_TYPE)
            .header("content-range", format!("bytes {first}-{last}/{size}"));
        // A transport error may quote the upload address, whose token is the
        // upload's own: it stays out of the error and the logs.
        let response = self.transport.send(&request).map_err(|_| {
            UploadError::new(
                UploadErrorKind::Unreachable,
                "TikTok's upload host could not be reached",
            )
        })?;
        Ok(match response.status {
            201 => Chunk::Complete,
            206 => Chunk::Taken,
            200 if last + 1 == size => Chunk::Complete,
            200 => Chunk::Taken,
            // The upload address expired (403), or TikTok does not know it:
            // 404, or 405 from its upload edge (recorded 2026-10-04).
            403..=405 => Chunk::Dropped,
            416 => match uploaded_range(&response, size) {
                Some(next) => Chunk::Elsewhere { next },
                None => Chunk::Dropped,
            },
            429 => return Err(chunk_failure(UploadErrorKind::RateLimited, &response)),
            500..=599 => return Err(chunk_failure(UploadErrorKind::NetworkDown, &response)),
            400 => return Err(chunk_failure(UploadErrorKind::Invalid, &response)),
            _ => return Err(chunk_failure(UploadErrorKind::Unexpected, &response)),
        })
    }
}

impl<T: Transport> VideoUploader for TikTokUploader<T> {
    fn network(&self) -> Network {
        Network::TikTok
    }

    /// Sends the file to a draft in the creator's inbox. The draft takes
    /// none of `video`'s details: the creator writes the caption in the
    /// TikTok app.
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
        if size > TIKTOK_MAX_BYTES {
            return Err(UploadError::new(
                UploadErrorKind::Invalid,
                "the file is over TikTok's limit",
            ));
        }
        let plan = ChunkPlan::new(size, self.chunk)
            .map_err(|error| UploadError::new(UploadErrorKind::Invalid, error.to_string()))?;
        // A saved session is the one started here; check it anyway, since
        // the file goes to its address.
        let mut session = run
            .session()
            .and_then(|text| Session::decode(&text))
            .filter(|session| is_publish_id(&session.publish_id))
            .filter(|session| self.is_upload_url(&session.upload_url));
        let mut confirmed = if session.is_some() { run.resumed() } else { 0 };
        let mut new_sessions = 0;
        let mut stalled = 0;
        loop {
            if run.should_stop() {
                return Ok(UploadOutcome::Stopped);
            }
            if let Some(current) = &session {
                if confirmed >= size {
                    // Every chunk was taken; the status says the rest.
                    return Ok(uploaded(current));
                }
                if !current.is_fresh(SystemTime::now()) {
                    session = None;
                }
            }
            let current = match &session {
                Some(current) => current.clone(),
                None => {
                    if new_sessions == MAX_NEW_SESSIONS {
                        return Err(unexpected("TikTok keeps dropping the upload"));
                    }
                    new_sessions += 1;
                    let started = self.init(&plan, &run.access_token()?)?;
                    run.session_started(&started.encode())?;
                    confirmed = 0;
                    run.confirmed(0)?;
                    session = Some(started.clone());
                    started
                }
            };
            // Kept after a whole chunk only; anything else is not where a
            // chunk starts, and the file goes again.
            let Some((first, len)) = plan.chunk_at(confirmed).and_then(|index| plan.chunk(index))
            else {
                session = None;
                continue;
            };
            let bytes = run.read(first, len as usize)?;
            if bytes.len() as u64 != len {
                return Err(UploadError::new(
                    UploadErrorKind::Local,
                    "the file is shorter than when the upload started",
                ));
            }
            match self.send_chunk(&current, first, bytes, size)? {
                Chunk::Taken => {
                    stalled = 0;
                    confirmed = first + len;
                    run.confirmed(confirmed)?;
                }
                Chunk::Complete => {
                    run.confirmed(size)?;
                    return Ok(uploaded(&current));
                }
                Chunk::Elsewhere { next } => {
                    stalled = if next > confirmed { 0 } else { stalled + 1 };
                    if stalled == MAX_STALLED_CHUNKS {
                        return Err(unexpected("TikTok takes no more of the file"));
                    }
                    if next >= size {
                        run.confirmed(size)?;
                        return Ok(uploaded(&current));
                    }
                    match plan.chunk_at(next) {
                        Some(_) => {
                            confirmed = next;
                            run.confirmed(next)?;
                        }
                        None => session = None,
                    }
                }
                Chunk::Dropped => session = None,
            }
        }
    }

    fn state(&self, access_token: &SecretText, id: &str) -> Result<VideoState, UploadError> {
        if !is_publish_id(id) {
            return Err(unexpected("not a TikTok publish id"));
        }
        let data = match self.api(
            "/v2/post/publish/status/fetch/",
            json!({ "publish_id": id }),
            access_token,
        ) {
            Ok(data) => data,
            // TikTok no longer knows the draft.
            Err(error) if error.kind == UploadErrorKind::NotFound => {
                return Ok(VideoState::Removed);
            }
            Err(error) => return Err(error),
        };
        let reason = || {
            data["fail_reason"]
                .as_str()
                .map(|reason| plain(reason, 100))
                .filter(|reason| !reason.is_empty())
                .unwrap_or_else(|| "unknown".to_owned())
        };
        Ok(match data["status"].as_str() {
            Some("PROCESSING_UPLOAD" | "PROCESSING_DOWNLOAD") => VideoState::Processing,
            // Posted already, should the creator have been quick: the draft
            // went to the inbox all the same, and the creator links the post.
            Some("SEND_TO_USER_INBOX" | "PUBLISH_COMPLETE") => VideoState::InInbox,
            Some("FAILED") => match reason().as_str() {
                // The creator took Bardo's access away meanwhile.
                "auth_removed" => {
                    return Err(UploadError::new(
                        UploadErrorKind::Refused,
                        "TikTok: the creator removed the app's access",
                    ));
                }
                reason if reason.starts_with("spam_risk") || reason == "publish_cancelled" => {
                    VideoState::Rejected(reason.to_owned())
                }
                reason => VideoState::Failed(reason.to_owned()),
            },
            other => {
                return Err(unexpected(&format!(
                    "unknown draft status {:?}",
                    other.map(|status| plain(status, 64))
                )));
            }
        })
    }
}

fn uploaded(session: &Session) -> UploadOutcome {
    UploadOutcome::Uploaded(UploadedVideo {
        id: session.publish_id.clone(),
    })
}

fn millis(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// A publish id as TikTok hands them out (`v_inbox_file~v2.123456789`): at
/// most 64 letters, digits and `_ ~ . -`, so it goes in a request as it is.
fn is_publish_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'~' | b'.' | b'-'))
}

/// Where TikTok's `Content-Range: bytes 0-LAST/TOTAL` says the file goes
/// on (`LAST + 1`), when it names this file.
fn uploaded_range(response: &HttpResponse, size: u64) -> Option<u64> {
    let range = response.header("content-range")?.trim();
    let (span, total) = range.strip_prefix("bytes ")?.split_once('/')?;
    if total.trim().parse::<u64>().ok()? != size {
        return None;
    }
    let (start, last) = span.split_once('-')?;
    if start.trim() != "0" {
        return None;
    }
    last.trim().parse::<u64>().ok()?.checked_add(1)
}

fn unexpected(detail: &str) -> UploadError {
    UploadError::new(UploadErrorKind::Unexpected, detail)
}

/// A chunk TikTok's upload host refused, for the log: its status only,
/// since its body may be an HTML page and its address holds a token.
fn chunk_failure(kind: UploadErrorKind, response: &HttpResponse) -> UploadError {
    UploadError::new(kind, format!("upload host: HTTP {}", response.status))
}

/// Classifies a Content Posting API error answer (`error.code`). The
/// detail keeps TikTok's message and `log_id`, never what Bardo sent.
fn api_failure(response: &HttpResponse, body: &Value) -> UploadError {
    let error = &body["error"];
    let code = error["code"].as_str().unwrap_or_default();
    let kind = match (response.status, code) {
        (_, "access_token_invalid") => UploadErrorKind::Refused,
        (_, "scope_not_authorized" | "scope_permission_missed") => UploadErrorKind::NotAllowed,
        (_, "spam_risk_too_many_pending_share") => UploadErrorKind::UploadLimit,
        (_, "spam_risk_user_banned_from_posting" | "spam_risk_too_many_posts") => {
            UploadErrorKind::NotAllowed
        }
        (_, "token_not_authorized_for_specified_publish_id") => UploadErrorKind::NotAllowed,
        (_, "invalid_publish_id") => UploadErrorKind::NotFound,
        (_, "rate_limit_exceeded") | (429, _) => UploadErrorKind::RateLimited,
        (_, "internal_error") | (500..=599, _) => UploadErrorKind::NetworkDown,
        (401, _) => UploadErrorKind::Refused,
        (403, _) => UploadErrorKind::NotAllowed,
        (_, "invalid_param") | (400, _) => UploadErrorKind::Invalid,
        _ => UploadErrorKind::Unexpected,
    };
    UploadError::new(
        kind,
        detail(
            response.status,
            &plain(code, 64),
            &plain(error["message"].as_str().unwrap_or_default(), SHOWN_TEXT),
            &error["log_id"],
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uploader() -> TikTokUploader<UreqTransport> {
        TikTokUploader::with_transport(UreqTransport::for_uploads(Duration::from_secs(1)))
    }

    #[test]
    fn only_safe_publish_ids_pass() {
        assert!(is_publish_id("v_inbox_file~v2.123456789"));
        assert!(!is_publish_id(""));
        assert!(!is_publish_id("v_inbox/../x"));
        assert!(!is_publish_id("a b"));
        assert!(!is_publish_id(&"a".repeat(65)));
    }

    #[test]
    fn only_tiktoks_upload_hosts_over_https_take_the_file() {
        let u = uploader();
        assert!(
            u.is_upload_url("https://open-upload.tiktokapis.com/video/?upload_id=1&upload_token=x")
        );
        assert!(u.is_upload_url("https://open-upload-i18n.tiktokapis.com/video/?upload_id=1"));
        assert!(!u.is_upload_url("http://open-upload.tiktokapis.com/video/"));
        assert!(!u.is_upload_url("https://open-upload.tiktokapis.com.evil.example/video/"));
        assert!(!u.is_upload_url("https://evil.example/?h=.tiktokapis.com"));
        assert!(!u.is_upload_url("https://user@open-upload.tiktokapis.com/video/"));
        assert!(!u.is_upload_url("https://open-upload.tiktokapis.com:8443/video/"));
        assert!(!u.is_upload_url("https://tiktokapis.com/video/"));
        assert!(!u.is_upload_url("https://open-upload.tiktokapis.com/video/\n"));
    }

    #[test]
    fn a_session_round_trips_and_lasts_under_an_hour() {
        let now = SystemTime::now();
        let session = Session {
            publish_id: "v_inbox_file~v2.1".into(),
            upload_url: "https://open-upload.tiktokapis.com/video/?upload_id=1".into(),
            issued_at: millis(now),
        };
        assert_eq!(Session::decode(&session.encode()), Some(session.clone()));
        assert_eq!(Session::decode("v_inbox_file~v2.1"), None);
        assert!(session.is_fresh(now));
        assert!(session.is_fresh(now + Duration::from_secs(54 * 60)));
        assert!(!session.is_fresh(now + Duration::from_secs(56 * 60)));
        let future = Session {
            issued_at: millis(now + Duration::from_secs(3600)),
            ..session
        };
        assert!(!future.is_fresh(now), "a clock that went back");
    }

    #[test]
    fn tiktoks_range_says_where_the_file_goes_on() {
        let range = |value: &str| {
            let response = HttpResponse::new(416, "").with_header("content-range", value);
            uploaded_range(&response, 30)
        };
        assert_eq!(range("bytes 0-9/30"), Some(10));
        assert_eq!(range("bytes 0-29/30"), Some(30));
        assert_eq!(range("bytes 0-9/31"), None, "another file");
        assert_eq!(range("bytes 5-9/30"), None);
        assert_eq!(range("bytes */30"), None);
        assert_eq!(uploaded_range(&HttpResponse::new(416, ""), 30), None);
    }
}
