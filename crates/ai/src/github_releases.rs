//! Bardo's own releases, from the GitHub REST API (issue #144,
//! ADR-0011): `GET /repos/{repo}/releases/latest` without a token, which
//! answers with the newest published release that is neither a draft nor
//! a pre-release (so the `ffmpeg-*` mirror releases never show).
//!
//! Without a token GitHub allows 60 calls an hour from one address; over
//! it, it answers 403 or 429 with `x-ratelimit-remaining: 0` and the time
//! the window resets in `x-ratelimit-reset`. A check runs once a day, far
//! below that.
//!
//! Files are fetched only from the repository's own release downloads,
//! over HTTPS to the end (GitHub redirects them to its file servers).

use std::io::{Read, Write};
use std::time::{Duration, SystemTime};

use bardo_domain::{DownloadError, Release, ReleaseAsset, ReleaseFeed, UpdateCheckError};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};

/// Bardo's repository on GitHub.
pub const REPOSITORY: &str = "fernandolisboa/bardo";

/// GitHub's REST API.
pub const API: &str = "https://api.github.com";

/// The releases of Bardo's repository, over HTTPS.
pub struct GitHubReleases<T = UreqTransport, D = UreqTransport> {
    /// For the API and the small checksum file.
    transport: T,
    /// For the installer, which may take minutes on a slow line.
    downloads: D,
    api: String,
    repository: String,
}

impl GitHubReleases {
    pub const TIMEOUT: Duration = Duration::from_secs(30);
    /// How long the installer may take to arrive.
    pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);

    pub fn new() -> Self {
        Self::with_transports(
            UreqTransport::https_only(Self::TIMEOUT),
            UreqTransport::for_downloads(Self::TIMEOUT, Self::DOWNLOAD_TIMEOUT),
        )
    }
}

impl Default for GitHubReleases {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport, D: Transport> GitHubReleases<T, D> {
    pub fn with_transports(transport: T, downloads: D) -> Self {
        Self {
            transport,
            downloads,
            api: API.to_owned(),
            repository: REPOSITORY.to_owned(),
        }
    }

    /// Asks another API address (a local test server) for the same
    /// repository's releases.
    pub fn at(mut self, api: impl Into<String>) -> Self {
        self.api = api.into().trim_end_matches('/').to_owned();
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn downloads(&self) -> &D {
        &self.downloads
    }

    /// The only place release files come from:
    /// `https://github.com/<repo>/releases/download/`.
    pub fn downloads_prefix(&self) -> String {
        format!("https://github.com/{}/releases/download/", self.repository)
    }

    fn release_file(&self, url: &str) -> Result<(), DownloadError> {
        let prefix = self.downloads_prefix();
        let rest = url.strip_prefix(&prefix).unwrap_or_default();
        // `<tag>/<file>`, nothing that climbs out or asks a query.
        let parts: Vec<&str> = rest.split('/').collect();
        let plain = parts.len() == 2
            && parts
                .iter()
                .all(|part| !part.is_empty() && *part != ".." && *part != ".")
            && !rest.contains(['?', '#', '\\']);
        if plain {
            Ok(())
        } else {
            Err(DownloadError::NotAReleaseFile(url.to_owned()))
        }
    }
}

/// When to ask again: after `retry-after` (seconds from now) when GitHub
/// gives it, which a secondary limit may set later than the hourly
/// window, else when that window resets (`x-ratelimit-reset`, Unix
/// seconds).
fn rate_limit_reset(response: &HttpResponse) -> Option<SystemTime> {
    if let Some(seconds) = response
        .header("retry-after")
        .and_then(|value| value.trim().parse::<u64>().ok())
    {
        return Some(SystemTime::now() + Duration::from_secs(seconds));
    }
    response
        .header("x-ratelimit-reset")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|reset| SystemTime::UNIX_EPOCH + Duration::from_secs(reset))
}

fn is_rate_limited(response: &HttpResponse) -> bool {
    response.status == 429
        || (response.status == 403
            && (response
                .header("x-ratelimit-remaining")
                .is_some_and(|remaining| remaining.trim() == "0")
                || response.header("retry-after").is_some()))
}

/// The release in a `releases/latest` answer.
pub fn parse_release(body: &str) -> Result<Release, UpdateCheckError> {
    let unexpected = |why: &str| UpdateCheckError::Unexpected(why.to_owned());
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|_| unexpected("the release is not JSON"))?;
    if json["draft"].as_bool() == Some(true) || json["prerelease"].as_bool() == Some(true) {
        return Err(unexpected("the latest release is a draft or a pre-release"));
    }
    let tag = json["tag_name"]
        .as_str()
        .ok_or_else(|| unexpected("the release has no tag"))?;
    let version = tag
        .parse()
        .map_err(|_| UpdateCheckError::Unexpected(format!("the tag {tag} is not a version")))?;
    let assets = json["assets"]
        .as_array()
        .map(|assets| {
            assets
                .iter()
                .filter(|asset| {
                    asset["state"]
                        .as_str()
                        .is_none_or(|state| state == "uploaded")
                })
                .filter_map(|asset| {
                    Some(ReleaseAsset {
                        name: asset["name"].as_str()?.to_owned(),
                        url: asset["browser_download_url"].as_str()?.to_owned(),
                        size: asset["size"].as_u64()?,
                        sha256: asset["digest"]
                            .as_str()
                            .and_then(|digest| digest.strip_prefix("sha256:"))
                            .filter(|hex| bardo_domain::is_sha256(hex))
                            .map(str::to_ascii_lowercase),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release {
        version,
        page: json["html_url"].as_str().unwrap_or_default().to_owned(),
        notes: json["body"].as_str().unwrap_or_default().to_owned(),
        published_at: json["published_at"]
            .as_str()
            .and_then(|at| humantime::parse_rfc3339_weak(at).ok()),
        assets,
    })
}

impl<T: Transport, D: Transport> ReleaseFeed for GitHubReleases<T, D> {
    fn latest(&self) -> Result<Release, UpdateCheckError> {
        let request = HttpRequest::get(format!(
            "{}/repos/{}/releases/latest",
            self.api, self.repository
        ))
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2022-11-28");
        let response = self
            .transport
            .send(&request)
            .map_err(|error| UpdateCheckError::Offline(error.0))?;
        match response.status {
            200 => parse_release(&response.body),
            404 => Err(UpdateCheckError::NoRelease),
            _ if is_rate_limited(&response) => Err(UpdateCheckError::RateLimited {
                until: rate_limit_reset(&response),
            }),
            status @ 500..=599 => Err(UpdateCheckError::Offline(format!(
                "GitHub answered {status}"
            ))),
            status => Err(UpdateCheckError::Unexpected(format!(
                "GitHub answered {status}"
            ))),
        }
    }

    fn text(&self, url: &str, max_bytes: u64) -> Result<String, DownloadError> {
        self.release_file(url)?;
        let response = self
            .transport
            .send_for_bytes(&HttpRequest::get(url), max_bytes)
            .map_err(|error| DownloadError::Unreachable(error.0))?;
        match response.status {
            200 => String::from_utf8(response.bytes)
                .map_err(|_| DownloadError::Unexpected("the file is not text".into())),
            status @ 500..=599 => Err(DownloadError::Unreachable(format!("answered {status}"))),
            status => Err(DownloadError::Unexpected(format!("answered {status}"))),
        }
    }

    fn download(
        &self,
        url: &str,
        to: &mut dyn Write,
        max_bytes: u64,
        progress: &mut dyn FnMut(u64) -> bool,
    ) -> Result<u64, DownloadError> {
        self.release_file(url)?;
        let response = self
            .downloads
            .send_streamed(&HttpRequest::get(url))
            .map_err(|error| DownloadError::Unreachable(error.0))?;
        match response.status {
            200 => {}
            status @ 500..=599 => {
                return Err(DownloadError::Unreachable(format!("answered {status}")));
            }
            status => return Err(DownloadError::Unexpected(format!("answered {status}"))),
        }
        let mut body = response.body;
        let mut buffer = vec![0; 64 * 1024];
        let mut written = 0u64;
        loop {
            if !progress(written) {
                return Err(DownloadError::Stopped);
            }
            let read = match body.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(DownloadError::Unreachable(error.to_string())),
            };
            written += read as u64;
            if written > max_bytes {
                return Err(DownloadError::TooBig(max_bytes));
            }
            to.write_all(&buffer[..read])
                .map_err(|error| DownloadError::Write(error.to_string()))?;
        }
        to.flush()
            .map_err(|error| DownloadError::Write(error.to_string()))?;
        let _ = progress(written);
        Ok(written)
    }
}
