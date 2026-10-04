//! Instagram Reels upload (ADR-0008): a resumable container on the
//! Instagram API with Facebook Login, the file sent to Meta's upload host,
//! then the Reel published once Instagram processed it.
//!
//! - `POST /<IG_USER_ID>/media` with `media_type=REELS` and
//!   `upload_type=resumable` makes a container with the caption, the cover
//!   frame (`thumb_offset`, in milliseconds), `share_to_feed` and the AI
//!   label (`is_ai_generated`); Meta answers with its id.
//! - The file goes to `rupload.facebook.com/ig-api-upload/<version>/<id>`
//!   in one `POST` with `Authorization: OAuth <token>`, `offset` and
//!   `file_size`. After an interruption the container's
//!   `video_status.uploading_phase.bytes_transferred` says what arrived,
//!   and the next `POST` starts there. The body is the rest of the file in
//!   memory: at most 300 MB, the Reel limit.
//! - `GET /<container>?fields=status_code,…` says where it stands:
//!   `IN_PROGRESS`, `FINISHED` (ready to publish), `PUBLISHED`, `ERROR`
//!   (`status` has the reason) or `EXPIRED` (not published within 24
//!   hours). Meta asks to poll about once a minute.
//! - `POST /<IG_USER_ID>/media_publish` with `creation_id` makes the Reel;
//!   its permalink comes from `GET /<media>?fields=permalink`.
//! - `GET /<IG_USER_ID>/content_publishing_limit` says how many posts the
//!   account published through the API in the moving window and how many
//!   it allows. Bardo reads the numbers each time and never assumes one.
//!
//! Docs checked 2026-10-04: <https://developers.facebook.com/docs/instagram-platform/content-publishing>,
//! <https://developers.facebook.com/docs/instagram-platform/content-publishing/resumable-uploads>,
//! <https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/ig-user/media>,
//! <https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/ig-container>,
//! <https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/ig-user/content_publishing_limit>,
//! <https://developers.facebook.com/docs/instagram-platform/instagram-graph-api/reference/error-codes>.

use std::time::Duration;

use bardo_ai::http::{HttpRequest, HttpResponse, Method, Transport, UreqTransport};
use bardo_domain::{
    Network, NetworkPost, PostLink, PublishingLimit, REEL_MAX_BYTES, SecretText, UploadError,
    UploadErrorKind, UploadOutcome, UploadRun, UploadedVideo, VideoState, VideoUpload,
    VideoUploader, Visibility,
};
use serde_json::Value;

use crate::MetaEndpoints;
use crate::text::{SHOWN_TEXT, plain};

/// The window `content_publishing_limit` counts over when its answer
/// leaves `quota_duration` out: 24 hours, as documented.
const LIMIT_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);

/// Meta asks to check a container about once a minute.
const POLL_DELAY: Duration = Duration::from_secs(60);

/// About a quarter of an hour of checks before the job stops waiting and
/// the user checks again later: Reels are mostly processed within minutes.
const PROCESSING_POLLS: u32 = 15;

/// Where a container stands, as `GET /<container>` says.
#[derive(Debug, PartialEq, Eq)]
enum Container {
    /// Meta still knows it: its `status_code`, its `status` (the reason of
    /// an `ERROR`), and the bytes of the file that arrived.
    Known {
        code: String,
        status: Option<String>,
        transferred: u64,
        upload_complete: bool,
    },
    /// Meta does not know it (any more).
    Gone,
}

/// Uploads Reels to Instagram over HTTPS.
pub struct InstagramUploader<T = UreqTransport> {
    transport: T,
    endpoints: MetaEndpoints,
}

impl InstagramUploader {
    /// Long enough for the whole file on a slow line: 300 MB at 1 Mbit/s
    /// takes about 40 minutes.
    pub const TIMEOUT: Duration = Duration::from_secs(60 * 60);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::for_uploads(Self::TIMEOUT))
    }
}

impl Default for InstagramUploader {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> InstagramUploader<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            endpoints: MetaEndpoints::default(),
        }
    }

    pub fn with_endpoints(mut self, endpoints: MetaEndpoints) -> Self {
        self.endpoints = endpoints;
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

    /// A Graph API call as the token's owner, answered in JSON.
    fn graph(&self, request: HttpRequest, token: &SecretText) -> Result<Value, UploadError> {
        let request = request.header("authorization", format!("Bearer {}", token.expose()));
        let response = self.send(&request)?;
        if !(200..=299).contains(&response.status) {
            return Err(graph_failure(&response));
        }
        serde_json::from_str(&response.body)
            .map_err(|error| unexpected(&format!("unreadable Graph API answer: {error}")))
    }

    /// Makes a resumable Reel container on `account`; its id.
    fn create(
        &self,
        video: &VideoUpload,
        account: &str,
        token: &SecretText,
    ) -> Result<String, UploadError> {
        let cover = video.cover.as_millis().to_string();
        let mut fields = vec![
            ("media_type", "REELS"),
            ("upload_type", "resumable"),
            ("caption", video.description.as_str()),
            (
                "share_to_feed",
                if video.share_to_feed { "true" } else { "false" },
            ),
            ("thumb_offset", cover.as_str()),
        ];
        if video.synthetic {
            fields.push(("is_ai_generated", "true"));
        }
        let url = format!("{}/{account}/media", self.endpoints.graph);
        let body = self.graph(HttpRequest::post_form(url, &fields), token)?;
        body["id"]
            .as_str()
            .filter(|id| is_id(id))
            .map(str::to_owned)
            .ok_or_else(|| unexpected("the container answer has no id"))
    }

    /// Where the container `id` stands.
    fn container(&self, id: &str, token: &SecretText) -> Result<Container, UploadError> {
        let request = HttpRequest::get(format!(
            "{}/{id}?fields=id%2Cstatus%2Cstatus_code%2Cvideo_status",
            self.endpoints.graph
        ))
        .header("authorization", format!("Bearer {}", token.expose()));
        let response = self.send(&request)?;
        if !(200..=299).contains(&response.status) {
            let error = graph_failure(&response);
            return match error.kind {
                UploadErrorKind::NotFound => Ok(Container::Gone),
                _ => Err(error),
            };
        }
        let body: Value = serde_json::from_str(&response.body)
            .map_err(|error| unexpected(&format!("unreadable container answer: {error}")))?;
        let code = body["status_code"]
            .as_str()
            .filter(|code| !code.is_empty())
            .ok_or_else(|| unexpected("the container answer has no status code"))?;
        let uploading = &body["video_status"]["uploading_phase"];
        let transferred = match &uploading["bytes_transferred"] {
            Value::Null => 0,
            value => value
                .as_u64()
                .ok_or_else(|| unexpected("unreadable bytes_transferred"))?,
        };
        Ok(Container::Known {
            code: code.to_owned(),
            status: body["status"]
                .as_str()
                .map(|status| plain(status, SHOWN_TEXT))
                .filter(|status| !status.is_empty()),
            transferred,
            upload_complete: uploading["status"].as_str() == Some("complete"),
        })
    }

    /// Sends the file from `offset`, the bytes Meta has, to the container.
    fn send_file(
        &self,
        id: &str,
        offset: u64,
        bytes: Vec<u8>,
        size: u64,
        token: &SecretText,
    ) -> Result<(), UploadError> {
        let request = HttpRequest {
            method: Method::Post,
            url: format!("{}/{id}", self.endpoints.rupload),
            headers: Vec::new(),
            body: Some(bytes),
        }
        .header("authorization", format!("OAuth {}", token.expose()))
        .header("offset", offset.to_string())
        .header("file_size", size.to_string());
        let response = self.send(&request)?;
        if !(200..=299).contains(&response.status) {
            return Err(rupload_failure(&response));
        }
        let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
        if body["success"].as_bool() == Some(false) {
            return Err(rupload_failure(&response));
        }
        Ok(())
    }

    /// The post's address, read after it was published. `None` when Meta
    /// does not say, or says something Bardo does not take for a Reel: the
    /// post exists either way.
    fn permalink(&self, media: &str, token: &SecretText) -> Option<PostLink> {
        let request = HttpRequest::get(format!(
            "{}/{media}?fields=permalink%2Cshortcode",
            self.endpoints.graph
        ));
        let body = self.graph(request, token).ok()?;
        PostLink::parse(Network::InstagramReels, body["permalink"].as_str()?).ok()
    }
}

impl<T: Transport> VideoUploader for InstagramUploader<T> {
    fn network(&self) -> Network {
        Network::InstagramReels
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
        if size > REEL_MAX_BYTES {
            return Err(UploadError::new(
                UploadErrorKind::Invalid,
                "the file is over the Reel limit",
            ));
        }
        let account = run.account().to_owned();
        if !is_id(&account) {
            return Err(UploadError::new(
                UploadErrorKind::SignedOut,
                "no Instagram account to upload to",
            ));
        }
        // A saved container is the one made here; check it anyway, since
        // its id goes in addresses.
        let mut container = run.session().filter(|id| is_id(id));
        let mut confirmed = 0;
        if let Some(id) = &container {
            match self.container(id, &run.access_token()?)? {
                // Dropped (gone, or expired unpublished): a new one starts.
                Container::Gone => container = None,
                Container::Known { code, .. } if code == "EXPIRED" => container = None,
                Container::Known {
                    code,
                    transferred,
                    upload_complete,
                    ..
                } => {
                    let done = matches!(code.as_str(), "FINISHED" | "PUBLISHED");
                    if done || upload_complete || transferred >= size {
                        // Meta has the file; `state` says the rest, a
                        // processing error included.
                        return Ok(UploadOutcome::Uploaded(UploadedVideo { id: id.clone() }));
                    }
                    if code == "ERROR" {
                        container = None;
                    } else {
                        confirmed = transferred;
                        run.confirmed(transferred)?;
                    }
                }
            }
        }
        if run.should_stop() {
            return Ok(UploadOutcome::Stopped);
        }
        let id = match container {
            Some(id) => id,
            None => {
                let id = self.create(video, &account, &run.access_token()?)?;
                run.session_started(&id)?;
                confirmed = 0;
                run.confirmed(0)?;
                id
            }
        };
        let len = size - confirmed;
        let bytes = run.read(confirmed, len as usize)?;
        if bytes.len() as u64 != len {
            return Err(UploadError::new(
                UploadErrorKind::Local,
                "the file is shorter than when the upload started",
            ));
        }
        if run.should_stop() {
            return Ok(UploadOutcome::Stopped);
        }
        self.send_file(&id, confirmed, bytes, size, &run.access_token()?)?;
        run.confirmed(size)?;
        Ok(UploadOutcome::Uploaded(UploadedVideo { id }))
    }

    fn state(&self, access_token: &SecretText, id: &str) -> Result<VideoState, UploadError> {
        if !is_id(id) {
            return Err(unexpected("not an Instagram container id"));
        }
        let (code, status) = match self.container(id, access_token)? {
            // A container is not the post: one Meta no longer has can't be
            // published, so it reads as expired and a new one starts.
            Container::Gone => return Ok(VideoState::Expired),
            Container::Known { code, status, .. } => (code, status),
        };
        Ok(match code.as_str() {
            "IN_PROGRESS" => VideoState::Processing,
            "FINISHED" => VideoState::Processed,
            "EXPIRED" => VideoState::Expired,
            "ERROR" => VideoState::Failed(status.unwrap_or_else(|| "ERROR".to_owned())),
            // Published already (by an earlier attempt that lost its
            // answer): public, without a known address.
            "PUBLISHED" => VideoState::Ready {
                visibility: Visibility::Public,
                publish_at: None,
                published_at: None,
            },
            other => return Err(unexpected(&format!("unknown container status {other:?}"))),
        })
    }

    fn publishing_limit(
        &self,
        access_token: &SecretText,
        account: &str,
    ) -> Result<Option<PublishingLimit>, UploadError> {
        if !is_id(account) {
            return Err(UploadError::new(
                UploadErrorKind::SignedOut,
                "no Instagram account to read the limit of",
            ));
        }
        let url = format!(
            "{}/{account}/content_publishing_limit?fields=config%2Cquota_usage",
            self.endpoints.graph
        );
        let body = self.graph(HttpRequest::get(url), access_token)?;
        let item = &body["data"][0];
        let number = |value: &Value| value.as_u64().and_then(|n| u32::try_from(n).ok());
        let (Some(used), Some(total)) = (
            number(&item["quota_usage"]),
            number(&item["config"]["quota_total"]),
        ) else {
            return Err(unexpected("the publishing limit answer has no numbers"));
        };
        let window = match &item["config"]["quota_duration"] {
            Value::Null => LIMIT_WINDOW,
            value => value
                .as_u64()
                .filter(|secs| (60..=7 * 24 * 60 * 60).contains(secs))
                .map(Duration::from_secs)
                .ok_or_else(|| unexpected("unreadable quota_duration"))?,
        };
        Ok(Some(PublishingLimit {
            used,
            total,
            window,
        }))
    }

    fn publish(
        &self,
        access_token: &SecretText,
        account: &str,
        id: &str,
    ) -> Result<NetworkPost, UploadError> {
        if !is_id(account) || !is_id(id) {
            return Err(unexpected("not an Instagram account or container id"));
        }
        let url = format!("{}/{account}/media_publish", self.endpoints.graph);
        let body = self.graph(
            HttpRequest::post_form(url, &[("creation_id", id)]),
            access_token,
        )?;
        let media = body["id"]
            .as_str()
            .filter(|media| is_id(media))
            .ok_or_else(|| unexpected("the publish answer has no media id"))?;
        Ok(NetworkPost {
            id: media.to_owned(),
            link: self.permalink(media, access_token),
            issue: config_issue(&body["config_issue"]),
        })
    }

    fn processing_polls(&self) -> u32 {
        PROCESSING_POLLS
    }

    fn poll_delay(&self, _polls: u32) -> Duration {
        POLL_DELAY
    }
}

/// An id as Meta hands them out: digits only, so it goes in an address as
/// it is.
fn is_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_digit())
}

/// What Instagram published the Reel without, in its words. The field is
/// not in the reference: it is read as text, as an object with a message,
/// or as a list of either.
fn config_issue(value: &Value) -> Option<String> {
    let text = |value: &Value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(fields) => ["message", "description", "error_user_msg", "code"]
            .iter()
            .find_map(|name| match &fields.get(*name)? {
                Value::String(text) => Some(text.clone()),
                Value::Number(number) => Some(number.to_string()),
                _ => None,
            }),
        Value::Null | Value::Bool(false) => None,
        other => Some(other.to_string()),
    };
    let issues: Vec<String> = match value {
        Value::Array(items) => items.iter().filter_map(text).collect(),
        other => text(other).into_iter().collect(),
    };
    let issues: Vec<String> = issues
        .iter()
        .map(|issue| plain(issue, SHOWN_TEXT))
        .filter(|issue| !issue.is_empty())
        .collect();
    let issues = plain(&issues.join("; "), SHOWN_TEXT);
    (!issues.is_empty()).then_some(issues)
}

fn unexpected(detail: &str) -> UploadError {
    UploadError::new(UploadErrorKind::Unexpected, detail)
}

/// The `error` object of a Graph API answer.
struct GraphError {
    code: i64,
    subcode: i64,
    message: String,
    transient: bool,
}

fn graph_error(body: &str) -> Option<GraphError> {
    let body: Value = serde_json::from_str(body).ok()?;
    let error = body.get("error")?;
    Some(GraphError {
        code: error["code"].as_i64().unwrap_or_default(),
        subcode: error["error_subcode"].as_i64().unwrap_or_default(),
        message: plain(error["message"].as_str().unwrap_or_default(), SHOWN_TEXT),
        transient: error["is_transient"].as_bool().unwrap_or_default(),
    })
}

/// Classifies a Graph API error answer by its documented codes.
fn graph_failure(response: &HttpResponse) -> UploadError {
    let error = graph_error(&response.body);
    let (code, subcode, message, transient) = error.as_ref().map_or((0, 0, "", false), |error| {
        (
            error.code,
            error.subcode,
            error.message.as_str(),
            error.transient,
        )
    });
    let kind = match (response.status, code, subcode) {
        // 190: expired, revoked or otherwise invalid; 102: session.
        (_, 190 | 102, _) => UploadErrorKind::Refused,
        // "The account has reached its daily publishing limit".
        (_, _, 2_207_042) => UploadErrorKind::UploadLimit,
        // "Media is not ready for publishing, please wait for a moment".
        (_, _, 2_207_027) | (_, 9007, _) => UploadErrorKind::NotReady,
        // The container expired: not published within 24 hours.
        (_, _, 2_207_020) => UploadErrorKind::Expired,
        // "Media ID is not available", or an object Meta does not have.
        (_, _, 2_207_006) | (_, 24, _) | (_, 100, 33) | (404, _, _) => UploadErrorKind::NotFound,
        (_, 4 | 17 | 32 | 613, _) | (429, _, _) => UploadErrorKind::RateLimited,
        // 3: capability; 10 and 200-299: a permission not granted.
        (_, 3 | 10 | 200..=299, _) => UploadErrorKind::NotAllowed,
        (_, 1 | 2, _) | (500..=599, _, _) => UploadErrorKind::NetworkDown,
        _ if transient => UploadErrorKind::NetworkDown,
        (401, _, _) => UploadErrorKind::Refused,
        (400, _, _) => UploadErrorKind::Invalid,
        _ => UploadErrorKind::Unexpected,
    };
    let detail = match (code, subcode, message) {
        (0, _, "") => format!("HTTP {}", response.status),
        (code, 0, message) => format!("HTTP {}: {code}: {message}", response.status),
        (code, subcode, message) => {
            format!("HTTP {}: {code}/{subcode}: {message}", response.status)
        }
    };
    UploadError::new(kind, detail)
}

/// Classifies an error answer of the upload host, which answers with
/// `debug_info` (`retriable`, `type`, `message`) instead of a Graph error.
fn rupload_failure(response: &HttpResponse) -> UploadError {
    if graph_error(&response.body).is_some() {
        return graph_failure(response);
    }
    let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
    let info = &body["debug_info"];
    let kind_name = plain(info["type"].as_str().unwrap_or_default(), 64);
    let message = plain(info["message"].as_str().unwrap_or_default(), SHOWN_TEXT);
    let kind = match (response.status, kind_name.as_str()) {
        (_, "NotAuthorizedError") | (401, _) => UploadErrorKind::Refused,
        _ if info["retriable"].as_bool() == Some(true) => UploadErrorKind::NetworkDown,
        (429, _) => UploadErrorKind::RateLimited,
        (500..=599, _) => UploadErrorKind::NetworkDown,
        (400, _) => UploadErrorKind::Invalid,
        _ => UploadErrorKind::Unexpected,
    };
    let detail = match (kind_name.as_str(), message.as_str()) {
        ("", "") => format!("HTTP {}", response.status),
        (name, message) => format!("HTTP {}: {name}: {message}", response.status),
    };
    UploadError::new(kind, detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_digits_pass_for_ids() {
        assert!(is_id("17895695668004550"));
        assert!(!is_id(""));
        assert!(!is_id("1789/../me"));
        assert!(!is_id("12a"));
        assert!(!is_id(&"1".repeat(33)));
    }

    #[test]
    fn a_config_issue_is_read_whatever_its_shape() {
        assert_eq!(config_issue(&Value::Null), None);
        assert_eq!(config_issue(&json!("")), None);
        assert_eq!(
            config_issue(&json!("The music was removed")),
            Some("The music was removed".into())
        );
        assert_eq!(
            config_issue(&json!({"message": "Cover not used"})),
            Some("Cover not used".into())
        );
        assert_eq!(
            config_issue(&json!(["Shared to Reels only", {"code": 42}])),
            Some("Shared to Reels only; 42".into())
        );
    }

    #[test]
    fn metas_words_are_kept_as_one_short_plain_line() {
        assert_eq!(
            plain("  Audio\n\tremoved\u{202e}gpj.exe ", 100),
            "Audio removedgpj.exe"
        );
        assert_eq!(plain(&"a".repeat(600), 500).chars().count(), 500);
        let many: Vec<Value> = (0..1000).map(|n| json!(format!("issue {n}"))).collect();
        let joined = config_issue(&Value::Array(many)).unwrap();
        assert_eq!(joined.chars().count(), SHOWN_TEXT);
        assert!(joined.starts_with("issue 0; issue 1; "));
    }

    #[test]
    fn upload_host_answers_are_classified() {
        let answer = |status, body: &str| rupload_failure(&HttpResponse::new(status, body)).kind;
        assert_eq!(
            answer(
                400,
                r#"{"debug_info":{"retriable":false,"type":"NotAuthorizedError","message":"x"}}"#
            ),
            UploadErrorKind::Refused
        );
        assert_eq!(
            answer(
                400,
                r#"{"debug_info":{"retriable":true,"type":"ProcessingFailedError","message":"x"}}"#
            ),
            UploadErrorKind::NetworkDown
        );
        assert_eq!(answer(503, ""), UploadErrorKind::NetworkDown);
        assert_eq!(
            answer(400, r#"{"error":{"code":190,"message":"x"}}"#),
            UploadErrorKind::Refused
        );
    }
}
