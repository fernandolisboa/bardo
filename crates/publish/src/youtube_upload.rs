//! YouTube upload: `videos.insert` over the resumable upload protocol, and
//! the uploaded video's status from `videos.list`.
//!
//! - A session starts with the video's details and the file's size
//!   (`X-Upload-Content-Length`); YouTube answers with the session's address
//!   in `Location`.
//! - The file goes in chunks that are multiples of 256 KiB (the last one
//!   excepted), each with `Content-Range: bytes FIRST-LAST/TOTAL`. YouTube
//!   answers 308 with the bytes it has (`Range: bytes=0-N`) until the last
//!   one, which it answers 200 or 201 with the video.
//! - To resume, an empty `PUT` with `Content-Range: bytes */TOTAL` asks what
//!   YouTube has. A session it no longer knows answers 404: a new one
//!   starts from the first byte.
//! - 500, 502, 503 and 504 are worth resuming after a wait; the job queue
//!   waits and resumes. `quotaExceeded` (the project's 100 uploads a day)
//!   is not.
//!
//! Docs checked 2026-10-03: <https://developers.google.com/youtube/v3/guides/using_resumable_upload_protocol>,
//! <https://developers.google.com/youtube/v3/docs/videos/insert>.

use std::time::Duration;

use bardo_ai::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use bardo_domain::{
    Network, SecretText, UPLOAD_CHUNK, UPLOAD_CHUNK_UNIT, UploadError, UploadErrorKind,
    UploadOutcome, UploadRun, UploadedVideo, VideoState, VideoUpload, VideoUploader, Visibility,
};
use serde_json::{Value, json};

use crate::GoogleEndpoints;

/// How many times one call starts a new session after YouTube forgot the
/// last one, before it gives up.
const MAX_NEW_SESSIONS: u32 = 2;

/// How many chunks in a row may come back without YouTube confirming more
/// bytes, before the call gives up instead of looping.
const MAX_STALLED_CHUNKS: u32 = 3;

/// What YouTube says about a session.
#[derive(Debug, PartialEq, Eq)]
enum Session {
    /// It has the first `confirmed` bytes.
    Incomplete { confirmed: u64 },
    /// It has the whole file and made the video.
    Complete(UploadedVideo),
    /// It no longer knows the session.
    Gone,
}

/// Uploads to YouTube over HTTPS.
pub struct YouTubeUploader<T = UreqTransport> {
    transport: T,
    endpoints: GoogleEndpoints,
    /// Bytes per chunk: a multiple of 256 KiB.
    chunk: u64,
}

impl YouTubeUploader {
    /// Long enough for one chunk on a slow line (8 MiB at 1 Mbit/s takes a
    /// little over a minute).
    pub const TIMEOUT: Duration = Duration::from_secs(5 * 60);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::for_uploads(Self::TIMEOUT))
    }
}

impl Default for YouTubeUploader {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> YouTubeUploader<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: GoogleEndpoints::default(),
            chunk: UPLOAD_CHUNK,
        }
    }

    pub fn with_endpoints(mut self, endpoints: GoogleEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }

    /// Chunks of `units` × 256 KiB (at least one).
    pub fn with_chunk_units(mut self, units: u64) -> Self {
        self.chunk = units.max(1) * UPLOAD_CHUNK_UNIT;
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

    /// Starts a session for a file of `size` bytes; its address.
    fn start(
        &self,
        video: &VideoUpload,
        size: u64,
        token: &SecretText,
    ) -> Result<String, UploadError> {
        let url = format!(
            "{}/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status",
            self.endpoints.api
        );
        let request = HttpRequest::post_json(url, resource(video).to_string())
            .header("authorization", format!("Bearer {}", token.expose()))
            .header("x-upload-content-length", size.to_string())
            .header("x-upload-content-type", "video/mp4");
        // `post_json` names the type; YouTube's guide adds the charset.
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
        if !(200..=299).contains(&response.status) {
            return Err(failure(&response));
        }
        response
            .header("location")
            .filter(|location| self.is_session(location))
            .map(str::to_owned)
            .ok_or_else(|| unexpected("the session answer has no address on the API's host"))
    }

    /// Whether `address` is on the API's own scheme and host: every chunk
    /// carries the token, so it never goes in clear text or elsewhere.
    fn is_session(&self, address: &str) -> bool {
        let api = self.endpoints.api.trim_end_matches('/');
        address
            .strip_prefix(api)
            .is_some_and(|rest| rest.starts_with('/'))
    }

    /// Asks what YouTube has of the session.
    fn query(&self, session: &str, size: u64, token: &SecretText) -> Result<Session, UploadError> {
        let request = HttpRequest::put(session, Vec::new())
            .header("authorization", format!("Bearer {}", token.expose()))
            .header("content-range", format!("bytes */{size}"));
        session_answer(&self.send(&request)?, size)
    }

    /// Sends `bytes` from `offset` of a file of `size` bytes.
    fn send_chunk(
        &self,
        session: &str,
        offset: u64,
        bytes: Vec<u8>,
        size: u64,
        token: &SecretText,
    ) -> Result<Session, UploadError> {
        let last = offset + bytes.len() as u64 - 1;
        let request = HttpRequest::put(session, bytes)
            .header("authorization", format!("Bearer {}", token.expose()))
            .header("content-type", "video/mp4")
            .header("content-range", format!("bytes {offset}-{last}/{size}"));
        session_answer(&self.send(&request)?, size)
    }
}

impl<T: Transport> VideoUploader for YouTubeUploader<T> {
    fn network(&self) -> Network {
        Network::YouTube
    }

    fn upload(
        &self,
        video: &VideoUpload,
        run: &mut dyn UploadRun,
    ) -> Result<UploadOutcome, UploadError> {
        let size = run.size();
        if size == 0 {
            return Err(UploadError::new(
                UploadErrorKind::Local,
                "the file is empty",
            ));
        }
        // A saved session is the one started here; check it anyway.
        let mut session = run.session().filter(|address| self.is_session(address));
        let mut confirmed = 0;
        if let Some(address) = &session {
            match self.query(address, size, &run.access_token()?)? {
                Session::Incomplete { confirmed: bytes } => {
                    confirmed = bytes;
                    run.confirmed(bytes)?;
                }
                Session::Complete(video) => return Ok(UploadOutcome::Uploaded(video)),
                Session::Gone => session = None,
            }
        }
        let mut new_sessions = 0;
        let mut stalled = 0;
        loop {
            if run.should_stop() {
                return Ok(UploadOutcome::Stopped);
            }
            let address = match &session {
                Some(address) => address.clone(),
                None => {
                    if new_sessions == MAX_NEW_SESSIONS {
                        return Err(unexpected("YouTube keeps forgetting the upload session"));
                    }
                    new_sessions += 1;
                    let address = self.start(video, size, &run.access_token()?)?;
                    run.session_started(&address)?;
                    confirmed = 0;
                    run.confirmed(0)?;
                    session = Some(address.clone());
                    address
                }
            };
            let token = run.access_token()?;
            let answer = if confirmed >= size {
                // YouTube has every byte but has not said so with the video.
                self.query(&address, size, &token)?
            } else {
                let len = self.chunk.min(size - confirmed);
                let bytes = run.read(confirmed, len as usize)?;
                if bytes.len() as u64 != len {
                    return Err(UploadError::new(
                        UploadErrorKind::Local,
                        "the file is shorter than when the upload started",
                    ));
                }
                self.send_chunk(&address, confirmed, bytes, size, &token)?
            };
            match answer {
                Session::Incomplete { confirmed: bytes } => {
                    stalled = if bytes > confirmed { 0 } else { stalled + 1 };
                    if stalled == MAX_STALLED_CHUNKS {
                        return Err(unexpected("YouTube confirms no more of the file"));
                    }
                    confirmed = bytes;
                    run.confirmed(bytes)?;
                }
                Session::Complete(video) => return Ok(UploadOutcome::Uploaded(video)),
                Session::Gone => session = None,
            }
        }
    }

    fn state(&self, access_token: &SecretText, id: &str) -> Result<VideoState, UploadError> {
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs([("part", "status"), ("id", id)])
            .finish();
        let request = HttpRequest::get(format!("{}/youtube/v3/videos?{query}", self.endpoints.api))
            .header("authorization", format!("Bearer {}", access_token.expose()));
        let response = self.send(&request)?;
        if !(200..=299).contains(&response.status) {
            return Err(failure(&response));
        }
        let body: Value = serde_json::from_str(&response.body)
            .map_err(|error| unexpected(&format!("unreadable videos answer: {error}")))?;
        let Some(status) = body["items"]
            .as_array()
            .and_then(|items| items.first())
            .map(|item| &item["status"])
        else {
            return Ok(VideoState::Removed);
        };
        let reason = |name: &str| status[name].as_str().unwrap_or("unknown").to_owned();
        Ok(match status["uploadStatus"].as_str() {
            Some("uploaded") => VideoState::Processing,
            Some("processed") => VideoState::Ready {
                visibility: match status["privacyStatus"].as_str() {
                    Some("public") => Visibility::Public,
                    Some("unlisted") => Visibility::Unlisted,
                    Some("private") => Visibility::Private,
                    other => {
                        return Err(unexpected(&format!("unknown privacy status {other:?}")));
                    }
                },
            },
            Some("failed") => VideoState::Failed(reason("failureReason")),
            Some("rejected") => VideoState::Rejected(reason("rejectionReason")),
            Some("deleted") => VideoState::Removed,
            other => return Err(unexpected(&format!("unknown upload status {other:?}"))),
        })
    }
}

/// The video resource `videos.insert` takes.
fn resource(video: &VideoUpload) -> Value {
    json!({
        "snippet": {
            "title": video.title,
            "description": video.description,
            "tags": video.tags,
        },
        "status": {
            "privacyStatus": video.visibility.code(),
            "selfDeclaredMadeForKids": video.made_for_kids,
            "containsSyntheticMedia": video.synthetic,
        },
    })
}

fn unexpected(detail: &str) -> UploadError {
    UploadError::new(UploadErrorKind::Unexpected, detail)
}

/// Reads an answer to a chunk or a status query.
fn session_answer(response: &HttpResponse, size: u64) -> Result<Session, UploadError> {
    match response.status {
        308 => {
            let confirmed = match response.header("range") {
                // Nothing received yet.
                None => 0,
                Some(range) => confirmed_bytes(range)
                    .filter(|bytes| *bytes <= size)
                    .ok_or_else(|| unexpected(&format!("unreadable range {range:?}")))?,
            };
            Ok(Session::Incomplete { confirmed })
        }
        200 | 201 => {
            let body: Value = serde_json::from_str(&response.body)
                .map_err(|error| unexpected(&format!("unreadable video answer: {error}")))?;
            match body["id"].as_str() {
                Some(id) if !id.is_empty() => {
                    Ok(Session::Complete(UploadedVideo { id: id.to_owned() }))
                }
                _ => Err(unexpected("the finished upload has no video id")),
            }
        }
        404 | 410 => Ok(Session::Gone),
        _ => Err(failure(response)),
    }
}

/// `bytes=0-N` as the count of bytes it covers (N + 1).
fn confirmed_bytes(range: &str) -> Option<u64> {
    let (first, last) = range.trim().strip_prefix("bytes=")?.split_once('-')?;
    if first != "0" {
        return None;
    }
    last.parse::<u64>().ok()?.checked_add(1)
}

/// Classifies an error answer of the upload or the Data API.
fn failure(response: &HttpResponse) -> UploadError {
    let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
    let error = &body["error"];
    let message = error["message"].as_str().unwrap_or_default();
    let reasons: Vec<&str> = error["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["reason"].as_str())
        .collect();
    let has = |reason: &str| reasons.contains(&reason);
    let kind = match response.status {
        401 => UploadErrorKind::Refused,
        403 | 429 if has("quotaExceeded") => UploadErrorKind::QuotaExceeded,
        403 if has("uploadLimitExceeded") => UploadErrorKind::UploadLimit,
        403 | 429 if has("rateLimitExceeded") || has("userRateLimitExceeded") => {
            UploadErrorKind::RateLimited
        }
        403 => UploadErrorKind::NotAllowed,
        429 => UploadErrorKind::RateLimited,
        400 => UploadErrorKind::Invalid,
        500 | 502 | 503 | 504 => UploadErrorKind::NetworkDown,
        _ => UploadErrorKind::Unexpected,
    };
    let reason = reasons.first().copied().unwrap_or_default();
    let detail = match (reason, message) {
        ("", "") => format!("HTTP {}", response.status),
        (reason, "") => format!("HTTP {}: {reason}", response.status),
        ("", message) => format!("HTTP {}: {message}", response.status),
        (reason, message) => format!("HTTP {}: {reason}: {message}", response.status),
    };
    UploadError::new(kind, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_counts_the_bytes_it_covers() {
        assert_eq!(confirmed_bytes("bytes=0-262143"), Some(262_144));
        assert_eq!(confirmed_bytes(" bytes=0-0"), Some(1));
        assert_eq!(confirmed_bytes("bytes=5-10"), None, "not from the start");
        assert_eq!(confirmed_bytes("0-10"), None);
        assert_eq!(confirmed_bytes("bytes=0-x"), None);
    }

    #[test]
    fn a_range_past_the_file_is_not_believed() {
        let response = HttpResponse::new(308, "").with_header("Range", "bytes=0-99");
        assert!(session_answer(&response, 50).is_err());
        assert_eq!(
            session_answer(&response, 100),
            Ok(Session::Incomplete { confirmed: 100 })
        );
    }
}
