//! Publications and their metrics (PRD stories 79-81, 84, 85): after
//! posting an export by hand, the user pastes the post's link; Bardo checks
//! it belongs to the network, keeps the post's id, and from then on tracks
//! the post. An upload (ADR-0008) becomes a publication of its own kind,
//! with a status, and gets its link once the network made the video. On
//! YouTube a metrics sync reads the public statistics (views, likes,
//! comments) with the Data API key and keeps a snapshot per sync; the other
//! networks keep the link until publishing brings their metrics. A
//! scheduled upload (YouTube's `publishAt`) waits private on the network;
//! the sync reads it back through the owner's connection until it goes
//! live, and then tracks it like any other post. When the channel's account
//! is connected, a sync also reads the owner's numbers of each YouTube post
//! it found (#79, `owner_metrics`): they ride on the same snapshot, and the
//! retention curve is kept as last read.
//!
//! Links are read here, without a network call: the post's id must be in
//! the link itself. Short links (`vm.tiktok.com/…`) hide it, so they are
//! refused with a hint to open them first.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{
    ApiKey, ChannelId, InvalidUploadTransition, Money, Network, NetworkAccountId, NetworkPost,
    OwnerMetrics, ProfileId, ProviderFailure, RenderId, RepositoryError, RetentionCurve,
    SCHEDULE_GRACE, Upload, UploadStatus, VideoProjectId, Visibility,
};

uuid_id!(
    /// Identifies a publication.
    PublicationId
);

/// Why a pasted link is not a post of the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PostLinkError {
    /// Nothing was pasted.
    Empty,
    /// The text is not a web address.
    NotALink,
    /// A link to another site: another network's (named) or none of them.
    OtherSite(Option<Network>),
    /// A short link, which hides the post's id until it is opened.
    ShortLink,
    /// The network's link, but not to one post (a profile, a feed).
    NotAPost,
}

/// A post's link as Bardo keeps it: the network's own address for the
/// post, rebuilt from its id (tracking parameters dropped), and the id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PostLink {
    network: Network,
    post_id: String,
    url: String,
}

/// The parts of a web address Bardo reads.
struct Address<'a> {
    host: String,
    segments: Vec<&'a str>,
    query: &'a str,
}

impl<'a> Address<'a> {
    fn parse(text: &'a str) -> Option<Self> {
        if text.chars().any(char::is_whitespace) {
            return None;
        }
        let rest = match text.find("://") {
            Some(at) => {
                let scheme = &text[..at];
                if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
                    return None;
                }
                &text[at + 3..]
            }
            None => text,
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, rest) = rest.split_at(end);
        // A user name or a port has no place in a post's link.
        if authority.contains(['@', ':']) {
            return None;
        }
        let host = authority.trim_end_matches('.').to_ascii_lowercase();
        if !host.contains('.') || host.starts_with('.') {
            return None;
        }
        let rest = rest.split('#').next().unwrap_or_default();
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        Some(Self {
            host,
            segments: path.split('/').filter(|s| !s.is_empty()).collect(),
            query,
        })
    }

    /// The site, without the prefixes networks use for the same pages.
    fn site(&self) -> &str {
        let mut host = self.host.as_str();
        for prefix in ["www.", "m.", "mobile."] {
            host = host.strip_prefix(prefix).unwrap_or(host);
        }
        host
    }

    fn param(&self, name: &str) -> Option<&'a str> {
        self.query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then_some(value)
        })
    }
}

/// The network a site belongs to, if any.
fn network_of(site: &str) -> Option<Network> {
    match site {
        "youtube.com" | "youtu.be" | "music.youtube.com" => Some(Network::YouTube),
        "tiktok.com" | "vm.tiktok.com" | "vt.tiktok.com" => Some(Network::TikTok),
        "instagram.com" | "instagr.am" => Some(Network::InstagramReels),
        "x.com" | "twitter.com" => Some(Network::X),
        "kick.com" => Some(Network::Kick),
        _ => None,
    }
}

fn charset(text: &str, min: usize, max: usize, allowed: impl Fn(char) -> bool) -> bool {
    (min..=max).contains(&text.chars().count()) && text.chars().all(allowed)
}

fn url_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// YouTube ids are 11 characters of the URL-safe alphabet.
fn youtube_id(text: &str) -> bool {
    charset(text, 11, 11, url_safe)
}

/// TikTok and X ids are numbers (snowflakes, up to 20 digits).
fn numeric_id(text: &str) -> bool {
    charset(text, 1, 20, |c| c.is_ascii_digit())
}

fn instagram_code(text: &str) -> bool {
    charset(text, 5, 64, url_safe)
}

/// A handle as the networks print it in links.
fn handle(text: &str) -> bool {
    charset(text, 1, 50, |c| url_safe(c) || c == '.')
}

fn kick_video_id(text: &str) -> bool {
    text.len() == 36
        && text.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn kick_clip_id(text: &str) -> bool {
    text.strip_prefix("clip_")
        .is_some_and(|rest| charset(rest, 1, 64, |c| c.is_ascii_alphanumeric()))
}

impl PostLink {
    /// Reads `text` as a link to one post on `network`. Accepted links:
    ///
    /// - YouTube: `youtube.com/shorts/ID`, `youtube.com/watch?v=ID`,
    ///   `youtu.be/ID`, `youtube.com/live/ID`, `youtube.com/embed/ID`.
    /// - TikTok: `tiktok.com/@handle/video/ID`.
    /// - Instagram: `instagram.com/reel/CODE`, `/reels/CODE`, `/p/CODE`,
    ///   also after a handle (`/handle/reel/CODE`).
    /// - X: `x.com/handle/status/ID` or `twitter.com/…`, `/i/status/ID`.
    /// - Kick: `kick.com/channel/videos/ID`, `kick.com/video/ID`,
    ///   `kick.com/channel/clips/clip_ID` or `?clip=clip_ID`.
    ///
    /// The scheme may be left out; `www.`, `m.` and `mobile.` are the same
    /// site; query strings and fragments other than the id are dropped.
    pub fn parse(network: Network, text: &str) -> Result<Self, PostLinkError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(PostLinkError::Empty);
        }
        let address = Address::parse(text).ok_or(PostLinkError::NotALink)?;
        let site = address.site();
        match network_of(site) {
            Some(found) if found == network => {}
            other => return Err(PostLinkError::OtherSite(other)),
        }
        let link = |post_id: &str, url: String| Self {
            network,
            post_id: post_id.to_owned(),
            url,
        };
        let segments = address.segments.as_slice();
        match network {
            Network::YouTube => {
                let (id, short) = match (site, segments) {
                    ("youtu.be", [id, ..]) => (*id, false),
                    (_, ["shorts", id, ..]) => (*id, true),
                    (_, ["live" | "embed" | "v", id, ..]) => (*id, false),
                    (_, ["watch"]) => (address.param("v").unwrap_or_default(), false),
                    _ => return Err(PostLinkError::NotAPost),
                };
                if !youtube_id(id) {
                    return Err(PostLinkError::NotAPost);
                }
                let url = if short {
                    format!("https://www.youtube.com/shorts/{id}")
                } else {
                    format!("https://www.youtube.com/watch?v={id}")
                };
                Ok(link(id, url))
            }
            Network::TikTok => {
                if site != "tiktok.com" || matches!(segments, ["t", ..]) {
                    return Err(PostLinkError::ShortLink);
                }
                match segments {
                    [user, "video", id, ..]
                        if numeric_id(id) && user.strip_prefix('@').is_some_and(handle) =>
                    {
                        Ok(link(
                            id,
                            format!("https://www.tiktok.com/{user}/video/{id}"),
                        ))
                    }
                    _ => Err(PostLinkError::NotAPost),
                }
            }
            Network::InstagramReels => {
                let code = match segments {
                    ["reel" | "reels" | "p" | "tv", code, ..] => *code,
                    [user, "reel" | "reels" | "p", code, ..] if handle(user) => *code,
                    _ => return Err(PostLinkError::NotAPost),
                };
                if !instagram_code(code) {
                    return Err(PostLinkError::NotAPost);
                }
                Ok(link(
                    code,
                    format!("https://www.instagram.com/reel/{code}/"),
                ))
            }
            Network::X => {
                let (user, id) = match segments {
                    ["i", "web", "status", id, ..] => ("i", *id),
                    [user, "status" | "statuses", id, ..] if handle(user) => (*user, *id),
                    _ => return Err(PostLinkError::NotAPost),
                };
                if !numeric_id(id) {
                    return Err(PostLinkError::NotAPost);
                }
                Ok(link(id, format!("https://x.com/{user}/status/{id}")))
            }
            Network::Kick => match segments {
                ["video", id] if kick_video_id(id) => {
                    Ok(link(id, format!("https://kick.com/video/{id}")))
                }
                [channel, "videos", id] if handle(channel) && kick_video_id(id) => {
                    Ok(link(id, format!("https://kick.com/{channel}/videos/{id}")))
                }
                [channel, "clips", id] if handle(channel) && kick_clip_id(id) => {
                    Ok(link(id, format!("https://kick.com/{channel}/clips/{id}")))
                }
                [channel, ..] if handle(channel) => match address.param("clip") {
                    Some(id) if kick_clip_id(id) => {
                        Ok(link(id, format!("https://kick.com/{channel}/clips/{id}")))
                    }
                    _ => Err(PostLinkError::NotAPost),
                },
                _ => Err(PostLinkError::NotAPost),
            },
        }
    }

    /// A link as stored, trusted to have been parsed before.
    pub fn restore(network: Network, post_id: String, url: String) -> Self {
        Self {
            network,
            post_id,
            url,
        }
    }

    pub fn network(&self) -> Network {
        self.network
    }

    /// The post's id on its network (a YouTube video id, an Instagram
    /// shortcode).
    pub fn post_id(&self) -> &str {
        &self.post_id
    }

    /// The network's address for the post.
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// How a publication got to the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicationKind {
    /// The user posted an export by hand and linked the post.
    Manual,
    /// Bardo uploaded it after the upload review (ADR-0008).
    Uploaded(Upload),
}

impl PublicationKind {
    /// Stable name for storage.
    pub fn code(&self) -> &'static str {
        match self {
            PublicationKind::Manual => "manual",
            PublicationKind::Uploaded(_) => "uploaded",
        }
    }
}

/// A video project's post on one network account: posted by hand from an
/// export and linked by its address, or uploaded by Bardo. One per project
/// and network: a new one replaces the earlier one, whatever its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publication {
    pub id: PublicationId,
    pub owner: ProfileId,
    pub project: VideoProjectId,
    pub account: NetworkAccountId,
    pub network: Network,
    /// The render it was posted from: the one the export copied, or the
    /// one uploaded.
    pub render: RenderId,
    /// The post on the network. Always there for a manual publication; an
    /// upload has it once the network made the video.
    pub link: Option<PostLink>,
    pub kind: PublicationKind,
    /// When it went live: when the user linked it or the upload was
    /// reviewed, until the network says (YouTube's publish time, after a
    /// sync).
    pub posted_at: SystemTime,
    /// When the user linked it, or reviewed the upload.
    pub linked_at: SystemTime,
    /// The last sync that looked for the post, found or not.
    pub checked_at: Option<SystemTime>,
    /// Since when syncs have not found the post (removed or made private).
    pub missing_since: Option<SystemTime>,
}

impl Publication {
    pub fn network(&self) -> Network {
        self.network
    }

    /// The post's id on the network, once there is a post.
    pub fn post_id(&self) -> Option<&str> {
        self.link.as_ref().map(PostLink::post_id)
    }

    pub fn upload(&self) -> Option<&Upload> {
        match &self.kind {
            PublicationKind::Manual => None,
            PublicationKind::Uploaded(upload) => Some(upload),
        }
    }

    pub fn upload_mut(&mut self) -> Option<&mut Upload> {
        match &mut self.kind {
            PublicationKind::Manual => None,
            PublicationKind::Uploaded(upload) => Some(upload),
        }
    }

    /// Whether the post is on the network: a linked post, or an upload
    /// the network finished processing (published or restricted). An
    /// upload still on its way, or failed, is not a post yet.
    pub fn is_posted(&self) -> bool {
        self.link.is_some()
            && match &self.kind {
                PublicationKind::Manual => true,
                PublicationKind::Uploaded(upload) => upload.status.is_final(),
            }
    }

    /// Whether a metrics sync reads this post: public statistics exist
    /// for YouTube only until publishing brings the others' (ADR-0004),
    /// and only for a post anyone can open by its id (an upload published
    /// as public or unlisted; a private or restricted one is hidden).
    pub fn has_public_metrics(&self) -> bool {
        if self.network != Network::YouTube || self.link.is_none() {
            return false;
        }
        match &self.kind {
            PublicationKind::Manual => true,
            PublicationKind::Uploaded(upload) => {
                upload.status == UploadStatus::Published && upload.visibility != Visibility::Private
            }
        }
    }

    /// Whether the post waits on the network for its publish time.
    pub fn is_scheduled(&self) -> bool {
        self.upload()
            .is_some_and(|upload| upload.status == UploadStatus::Scheduled)
    }

    /// Whether a metrics sync reads this post: for its public statistics,
    /// or, while scheduled, to learn whether it went live.
    pub fn is_tracked(&self) -> bool {
        self.has_public_metrics() || (self.is_scheduled() && self.link.is_some())
    }

    /// What a sync read of a scheduled post, at `now`:
    ///
    /// - Live: it went public; it went live when the network says.
    /// - Private with a publish time: still waiting, at that time (changed
    ///   on the network or not), unless the time is more than
    ///   `SCHEDULE_GRACE` past: then the network kept it private.
    /// - Private without one: the schedule was cancelled on the network
    ///   before its time (a private video now), or the network kept it
    ///   private once its time passed.
    /// - Missing: not found, like a post a sync no longer finds.
    ///
    /// A post that is not scheduled is left as it is.
    pub fn schedule_seen(&mut self, reading: ScheduleReading, now: SystemTime) {
        // A video that stays private counts as posted from its upload.
        let arrived = self.linked_at;
        let Some(upload) = self
            .upload_mut()
            .filter(|u| u.status == UploadStatus::Scheduled)
        else {
            return;
        };
        let due = |at: SystemTime| at + SCHEDULE_GRACE <= now;
        let step = match reading {
            ScheduleReading::Live { published_at } => {
                let fallback = upload.publish_at.unwrap_or(now);
                upload
                    .went_live()
                    .map(|()| Some(published_at.unwrap_or(fallback)))
            }
            ScheduleReading::Private {
                publish_at: Some(at),
            } if due(at) => upload.kept_private().map(|()| Some(arrived)),
            ScheduleReading::Private {
                publish_at: Some(at),
            } => upload.reschedule(at).map(|()| Some(at)),
            ScheduleReading::Private { publish_at: None } => {
                if upload.publish_at.is_some_and(due) {
                    upload.kept_private()
                } else {
                    upload.unschedule()
                }
                .map(|()| Some(arrived))
            }
            ScheduleReading::Missing => {
                self.checked(None, now);
                return;
            }
        };
        if let Ok(Some(posted_at)) = step {
            self.posted_at = posted_at;
        }
        self.checked_at = Some(now);
        self.missing_since = None;
    }

    /// The user changed the scheduled post's publish time on the network.
    pub fn rescheduled(&mut self, publish_at: SystemTime) -> Result<(), InvalidUploadTransition> {
        self.scheduled_upload("reschedule")?
            .reschedule(publish_at)?;
        self.posted_at = publish_at;
        Ok(())
    }

    /// The user cancelled the scheduled post's publish time on the network:
    /// it stays private.
    pub fn unscheduled(&mut self) -> Result<(), InvalidUploadTransition> {
        self.scheduled_upload("cancel the schedule of")?
            .unschedule()?;
        self.posted_at = self.linked_at;
        Ok(())
    }

    fn scheduled_upload(
        &mut self,
        action: &'static str,
    ) -> Result<&mut Upload, InvalidUploadTransition> {
        self.upload_mut().ok_or(InvalidUploadTransition {
            from: "manual",
            action,
        })
    }

    /// What a sync learned: the post with its statistics, or that it was
    /// not found.
    pub fn checked(&mut self, found: Option<&VideoStatistics>, at: SystemTime) {
        self.checked_at = Some(at);
        match found {
            Some(statistics) => {
                self.missing_since = None;
                if let Some(published) = statistics.published_at {
                    self.posted_at = published;
                }
            }
            None => {
                self.missing_since.get_or_insert(at);
            }
        }
    }

    /// The upload sent the whole file and the network made `video` of it:
    /// its post, where the network makes one at once (YouTube). Where the
    /// post only comes when Bardo publishes it (Instagram), `None` keeps
    /// the publication without one until then.
    pub fn sent(&mut self, video: Option<PostLink>) -> Result<(), InvalidUploadTransition> {
        let upload = self.upload_mut().ok_or(InvalidUploadTransition {
            from: "manual",
            action: "finish sending",
        })?;
        upload.sent()?;
        if video.is_some() {
            self.link = video;
        }
        Ok(())
    }

    /// Bardo published the processed upload and the network made `post`
    /// of it at `at`, public, with what it said it left out.
    pub fn published(
        &mut self,
        post: NetworkPost,
        at: SystemTime,
    ) -> Result<(), InvalidUploadTransition> {
        let upload = self.scheduled_upload("publish")?;
        upload.processed(Visibility::Public, None)?;
        upload.issue = post.issue;
        upload.network_id = Some(post.id);
        if post.link.is_some() {
            self.link = post.link;
        }
        self.posted_at = at;
        Ok(())
    }

    /// The network processed the uploaded video: it went live `at`, or,
    /// scheduled, goes live at its publish time.
    pub fn processed(
        &mut self,
        visibility: Visibility,
        publish_at: Option<SystemTime>,
        at: SystemTime,
    ) -> Result<(), InvalidUploadTransition> {
        let upload = self.scheduled_upload("finish processing")?;
        upload.processed(visibility, publish_at)?;
        self.posted_at = match (&upload.status, upload.publish_at) {
            (UploadStatus::Scheduled, Some(publish_at)) => publish_at,
            _ => at,
        };
        Ok(())
    }
}

/// What a sync read of a scheduled post (`Publication::schedule_seen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleReading {
    /// The video is no longer private, public since `published_at` (the
    /// network's time) when it says.
    Live { published_at: Option<SystemTime> },
    /// Still private, with the publish time the network holds.
    Private { publish_at: Option<SystemTime> },
    /// The network no longer has the video.
    Missing,
}

/// A post's public statistics as the network reports them now. Likes and
/// comments are `None` when the owner hides or turns them off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoStatistics {
    pub post_id: String,
    pub published_at: Option<SystemTime>,
    pub views: u64,
    pub likes: Option<u64>,
    pub comments: Option<u64>,
}

/// One publication's statistics at one sync (CONTEXT.md: metrics
/// snapshot). Never changes once taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub publication: PublicationId,
    pub taken_at: SystemTime,
    /// The public view count.
    pub views: u64,
    pub likes: Option<u64>,
    pub comments: Option<u64>,
    /// The owner's numbers, when the channel's account is connected and
    /// the network had data for the post (48 to 72 hours late).
    pub owner: Option<OwnerMetrics>,
}

impl MetricsSnapshot {
    /// Views gained (or lost, when YouTube recounts) from this snapshot
    /// to `later`.
    pub fn views_to(&self, later: &MetricsSnapshot) -> i64 {
        let signed = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        signed(later.views) - signed(self.views)
    }

    pub fn of(publication: PublicationId, statistics: &VideoStatistics, at: SystemTime) -> Self {
        Self {
            publication,
            taken_at: at,
            views: statistics.views,
            likes: statistics.likes,
            comments: statistics.comments,
            owner: None,
        }
    }
}

/// A post's retention curve as a sync last read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostRetention {
    pub publication: PublicationId,
    pub read_at: SystemTime,
    pub curve: RetentionCurve,
}

/// Reads public statistics of posts (YouTube Data API, key only).
pub trait VideoStats: Send + Sync {
    /// The statistics of `ids`, at most [`STATS_BATCH`] of them. Posts the
    /// network does not show (removed, private) are left out.
    fn statistics(
        &self,
        key: &ApiKey,
        ids: &[&str],
    ) -> Result<Vec<VideoStatistics>, ProviderFailure>;
}

/// The most posts one statistics call reads (`videos.list` takes 50 ids
/// for one quota unit).
pub const STATS_BATCH: usize = 50;

/// Quota units a sync of `posts` costs: one per batch.
pub fn sync_quota_units(posts: usize) -> u32 {
    u32::try_from(posts.div_ceil(STATS_BATCH)).unwrap_or(u32::MAX)
}

/// When the app syncs metrics by itself on start: never, or when the
/// oldest check is older than an interval. A per-profile setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MetricsSyncOnStart {
    Off,
    Hourly,
    #[default]
    Every6Hours,
    Every12Hours,
    Daily,
}

impl MetricsSyncOnStart {
    pub const ALL: [MetricsSyncOnStart; 5] = [
        MetricsSyncOnStart::Off,
        MetricsSyncOnStart::Hourly,
        MetricsSyncOnStart::Every6Hours,
        MetricsSyncOnStart::Every12Hours,
        MetricsSyncOnStart::Daily,
    ];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            MetricsSyncOnStart::Off => "off",
            MetricsSyncOnStart::Hourly => "1h",
            MetricsSyncOnStart::Every6Hours => "6h",
            MetricsSyncOnStart::Every12Hours => "12h",
            MetricsSyncOnStart::Daily => "24h",
        }
    }

    /// A stored value this version does not know reads as the default.
    pub fn from_code_or_default(code: &str) -> Self {
        code.parse().unwrap_or_default()
    }

    /// How old the oldest check may be before a start syncs again.
    pub fn interval(self) -> Option<Duration> {
        let hours = |n: u64| Some(Duration::from_secs(n * 3600));
        match self {
            MetricsSyncOnStart::Off => None,
            MetricsSyncOnStart::Hourly => hours(1),
            MetricsSyncOnStart::Every6Hours => hours(6),
            MetricsSyncOnStart::Every12Hours => hours(12),
            MetricsSyncOnStart::Daily => hours(24),
        }
    }

    /// Whether a start at `now` syncs `publications`: some tracked post
    /// was never checked, the oldest check is older than the interval, or
    /// a scheduled post's publish time passed since its last check. Off
    /// never syncs.
    pub fn is_due(self, publications: &[Publication], now: SystemTime) -> bool {
        let Some(interval) = self.interval() else {
            return false;
        };
        // A scheduled post past its publish time, not checked since: did it
        // go live?
        let went_live = publications.iter().any(|publication| {
            let passed = publication
                .upload()
                .and_then(|upload| upload.publish_at)
                .map(|at| at + SCHEDULE_GRACE)
                .filter(|passed| *passed <= now);
            publication.is_tracked()
                && publication.is_scheduled()
                && passed.is_some_and(|passed| publication.checked_at.is_none_or(|c| c < passed))
        });
        if went_live {
            return true;
        }
        let mut checks = publications
            .iter()
            .filter(|publication| publication.is_tracked())
            .map(|publication| publication.checked_at)
            .peekable();
        if checks.peek().is_none() {
            return false;
        }
        match checks.min() {
            Some(Some(oldest)) => now.duration_since(oldest).is_ok_and(|age| age >= interval),
            _ => true,
        }
    }
}

impl fmt::Display for MetricsSyncOnStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown metrics sync setting: {0}")]
pub struct UnknownMetricsSync(pub String);

impl FromStr for MetricsSyncOnStart {
    type Err = UnknownMetricsSync;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MetricsSyncOnStart::ALL
            .into_iter()
            .find(|setting| setting.code() == s)
            .ok_or_else(|| UnknownMetricsSync(s.to_owned()))
    }
}

/// Views, likes and comments added up. A count no post reports stays
/// `None`; posts that hide one add nothing to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetricsTotals {
    pub views: u64,
    pub likes: Option<u64>,
    pub comments: Option<u64>,
    /// How many posts the totals add up.
    pub posts: usize,
    /// The owner's numbers of the posts that have them; `None` when none
    /// has.
    pub owner: Option<OwnerTotals>,
}

/// The owner's numbers added up, over the posts that have them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OwnerTotals {
    pub engaged_views: u64,
    pub minutes_watched: u64,
    /// The revenue of the monetized posts; `None` when none is.
    pub revenue: Option<Money>,
    pub posts: usize,
}

impl MetricsTotals {
    fn add(&mut self, snapshot: &MetricsSnapshot) {
        let sum = |total: Option<u64>, n: Option<u64>| match (total, n) {
            (total, None) => total,
            (total, Some(n)) => Some(total.unwrap_or(0).saturating_add(n)),
        };
        self.views = self.views.saturating_add(snapshot.views);
        self.likes = sum(self.likes, snapshot.likes);
        self.comments = sum(self.comments, snapshot.comments);
        self.posts += 1;
    }

    fn add_owner(&mut self, snapshot: &MetricsSnapshot) {
        if let Some(owner) = &snapshot.owner {
            let totals = self.owner.get_or_insert_with(OwnerTotals::default);
            totals.engaged_views = totals.engaged_views.saturating_add(owner.engaged_views);
            totals.minutes_watched = totals.minutes_watched.saturating_add(owner.minutes_watched);
            if let Some(money) = owner.money() {
                totals.revenue = Some(
                    totals
                        .revenue
                        .unwrap_or(Money::ZERO)
                        .saturating_add(money.revenue)
                        .min(Money::MAX),
                );
            }
            totals.posts += 1;
        }
    }

    /// The latest snapshot of each post added up, and the owner's numbers
    /// of each post's latest snapshot that has them: a sync that could not
    /// read them keeps the ones read before.
    pub fn latest(snapshots: &[MetricsSnapshot]) -> Self {
        let mut totals = Self::default();
        for snapshot in latest_of_each(snapshots).values() {
            totals.add(snapshot);
        }
        let owned: Vec<MetricsSnapshot> = snapshots
            .iter()
            .filter(|snapshot| snapshot.owner.is_some())
            .copied()
            .collect();
        for snapshot in latest_of_each(&owned).values() {
            totals.add_owner(snapshot);
        }
        totals
    }
}

/// Each publication's latest snapshot.
pub fn latest_of_each(snapshots: &[MetricsSnapshot]) -> HashMap<PublicationId, MetricsSnapshot> {
    let mut latest: HashMap<PublicationId, MetricsSnapshot> = HashMap::new();
    for snapshot in snapshots {
        latest
            .entry(snapshot.publication)
            .and_modify(|kept| {
                if snapshot.taken_at >= kept.taken_at {
                    *kept = *snapshot;
                }
            })
            .or_insert(*snapshot);
    }
    latest
}

/// A channel's totals at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelPoint {
    pub at: SystemTime,
    pub totals: MetricsTotals,
}

/// A channel's totals over time: at each moment a snapshot was taken, the
/// latest snapshot of every post up to then, added up (a post not synced
/// at that moment counts with its previous numbers, and its owner's
/// numbers with the last ones read). Oldest first.
pub fn channel_history(snapshots: &[MetricsSnapshot]) -> Vec<ChannelPoint> {
    let mut ordered: Vec<&MetricsSnapshot> = snapshots.iter().collect();
    ordered.sort_by_key(|snapshot| snapshot.taken_at);
    let mut latest: HashMap<PublicationId, MetricsSnapshot> = HashMap::new();
    let mut latest_owned: HashMap<PublicationId, MetricsSnapshot> = HashMap::new();
    let mut points: Vec<ChannelPoint> = Vec::new();
    // A sync stamps every post with the same time: one point per sync.
    for (ix, snapshot) in ordered.iter().enumerate() {
        latest.insert(snapshot.publication, **snapshot);
        if snapshot.owner.is_some() {
            latest_owned.insert(snapshot.publication, **snapshot);
        }
        let last_of_its_time = ordered
            .get(ix + 1)
            .is_none_or(|next| next.taken_at != snapshot.taken_at);
        if last_of_its_time {
            let mut totals = MetricsTotals::default();
            for kept in latest.values() {
                totals.add(kept);
            }
            for kept in latest_owned.values() {
                totals.add_owner(kept);
            }
            points.push(ChannelPoint {
                at: snapshot.taken_at,
                totals,
            });
        }
    }
    points
}

/// Persistence port for publications and their snapshots.
pub trait PublicationRepository: Send + Sync {
    /// The project's publications, in `Network::ALL` order.
    fn publications(&self, project: VideoProjectId) -> Result<Vec<Publication>, RepositoryError>;

    /// The publications of every project of the channel, newest post
    /// first.
    fn channel_publications(&self, channel: ChannelId)
    -> Result<Vec<Publication>, RepositoryError>;

    /// Every publication of the profile.
    fn all_publications(&self, owner: ProfileId) -> Result<Vec<Publication>, RepositoryError>;

    fn publication(&self, id: PublicationId) -> Result<Option<Publication>, RepositoryError>;

    /// Saves a publication, replacing the project's earlier one for the
    /// same network (and its snapshots, when it is another post).
    fn save_publication(&self, publication: &Publication) -> Result<(), RepositoryError>;

    /// Saves an upload's progress: its status, visibility and publish time,
    /// and the post once the network has it. Only the row of the same
    /// publication and upload job changes, and nothing else goes, so a run
    /// that ends after the upload was replaced leaves the replacement
    /// alone. Returns whether the row was still there.
    fn save_upload(&self, publication: &Publication) -> Result<bool, RepositoryError>;

    /// Saves what was learned about a scheduled upload (a sync's read-back
    /// or the user's change) as `save_upload` does, but only while the row
    /// still holds the schedule it started from: scheduled to go public at
    /// `from`. A change saved meanwhile wins. Returns whether it was saved.
    fn save_schedule(
        &self,
        publication: &Publication,
        from: SystemTime,
    ) -> Result<bool, RepositoryError>;

    /// Removes a publication and its snapshots.
    fn remove_publication(&self, id: PublicationId) -> Result<(), RepositoryError>;

    /// Saves a sync's outcome in one transaction: the publications it
    /// checked and the snapshots it took.
    fn save_sync(
        &self,
        checked: &[Publication],
        snapshots: &[MetricsSnapshot],
    ) -> Result<(), RepositoryError>;

    /// The publication's snapshots, oldest first.
    fn snapshots(
        &self,
        publication: PublicationId,
    ) -> Result<Vec<MetricsSnapshot>, RepositoryError>;

    /// The snapshots of every publication of the channel, oldest first.
    fn channel_snapshots(
        &self,
        channel: ChannelId,
    ) -> Result<Vec<MetricsSnapshot>, RepositoryError>;

    /// Replaces each post's retention curve with the one given; a post
    /// removed meanwhile is skipped.
    fn save_retention(&self, curves: &[PostRetention]) -> Result<(), RepositoryError>;

    /// The post's retention curve as last read.
    fn retention(
        &self,
        publication: PublicationId,
    ) -> Result<Option<PostRetention>, RepositoryError>;

    /// The retention curves of every publication of the channel.
    fn channel_retention(&self, channel: ChannelId) -> Result<Vec<PostRetention>, RepositoryError>;
}

impl<T: PublicationRepository + ?Sized> PublicationRepository for Arc<T> {
    fn publications(&self, project: VideoProjectId) -> Result<Vec<Publication>, RepositoryError> {
        (**self).publications(project)
    }

    fn channel_publications(
        &self,
        channel: ChannelId,
    ) -> Result<Vec<Publication>, RepositoryError> {
        (**self).channel_publications(channel)
    }

    fn all_publications(&self, owner: ProfileId) -> Result<Vec<Publication>, RepositoryError> {
        (**self).all_publications(owner)
    }

    fn publication(&self, id: PublicationId) -> Result<Option<Publication>, RepositoryError> {
        (**self).publication(id)
    }

    fn save_publication(&self, publication: &Publication) -> Result<(), RepositoryError> {
        (**self).save_publication(publication)
    }

    fn save_upload(&self, publication: &Publication) -> Result<bool, RepositoryError> {
        (**self).save_upload(publication)
    }

    fn save_schedule(
        &self,
        publication: &Publication,
        from: SystemTime,
    ) -> Result<bool, RepositoryError> {
        (**self).save_schedule(publication, from)
    }

    fn remove_publication(&self, id: PublicationId) -> Result<(), RepositoryError> {
        (**self).remove_publication(id)
    }

    fn save_sync(
        &self,
        checked: &[Publication],
        snapshots: &[MetricsSnapshot],
    ) -> Result<(), RepositoryError> {
        (**self).save_sync(checked, snapshots)
    }

    fn snapshots(
        &self,
        publication: PublicationId,
    ) -> Result<Vec<MetricsSnapshot>, RepositoryError> {
        (**self).snapshots(publication)
    }

    fn channel_snapshots(
        &self,
        channel: ChannelId,
    ) -> Result<Vec<MetricsSnapshot>, RepositoryError> {
        (**self).channel_snapshots(channel)
    }

    fn save_retention(&self, curves: &[PostRetention]) -> Result<(), RepositoryError> {
        (**self).save_retention(curves)
    }

    fn retention(
        &self,
        publication: PublicationId,
    ) -> Result<Option<PostRetention>, RepositoryError> {
        (**self).retention(publication)
    }

    fn channel_retention(&self, channel: ChannelId) -> Result<Vec<PostRetention>, RepositoryError> {
        (**self).channel_retention(channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(network: Network, text: &str) -> Result<(String, String), PostLinkError> {
        PostLink::parse(network, text)
            .map(|link| (link.post_id().to_owned(), link.url().to_owned()))
    }

    fn id(network: Network, text: &str) -> String {
        parse(network, text)
            .unwrap_or_else(|e| panic!("{text}: {e:?}"))
            .0
    }

    #[test]
    fn youtube_links_in_every_shape_give_the_video_id() {
        let y = Network::YouTube;
        for text in [
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://youtube.com/shorts/dQw4w9WgXcQ?si=AbCdEf123",
            "youtube.com/shorts/dQw4w9WgXcQ/",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ&t=42s",
            "http://www.youtube.com/watch?feature=share&v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?si=x",
            "https://www.youtube.com/live/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "  HTTPS://WWW.YOUTUBE.COM/shorts/dQw4w9WgXcQ#comments  ",
        ] {
            assert_eq!(id(y, text), "dQw4w9WgXcQ", "{text}");
        }
    }

    #[test]
    fn youtube_links_are_kept_as_the_networks_own_address() {
        let y = Network::YouTube;
        assert_eq!(
            parse(y, "youtube.com/shorts/dQw4w9WgXcQ?si=track")
                .unwrap()
                .1,
            "https://www.youtube.com/shorts/dQw4w9WgXcQ"
        );
        assert_eq!(
            parse(y, "https://youtu.be/dQw4w9WgXcQ?si=track").unwrap().1,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
    }

    #[test]
    fn youtube_links_that_are_not_one_video_are_refused() {
        let y = Network::YouTube;
        for text in [
            "https://www.youtube.com/@archives",
            "https://www.youtube.com/watch?list=PL123",
            "https://www.youtube.com/watch?v=short",
            "https://www.youtube.com/shorts/",
            "https://www.youtube.com/shorts/dQw4w9WgXc!",
            "https://www.youtube.com/",
        ] {
            assert_eq!(parse(y, text), Err(PostLinkError::NotAPost), "{text}");
        }
    }

    #[test]
    fn tiktok_links_need_the_full_address() {
        let t = Network::TikTok;
        assert_eq!(
            parse(
                t,
                "https://www.tiktok.com/@space.archives/video/7301234567890123456?lang=en"
            ),
            Ok((
                "7301234567890123456".to_owned(),
                "https://www.tiktok.com/@space.archives/video/7301234567890123456".to_owned()
            ))
        );
        for text in [
            "https://vm.tiktok.com/ZMabc123/",
            "https://vt.tiktok.com/ZSabc123/",
            "https://www.tiktok.com/t/ZTabc123/",
        ] {
            assert_eq!(parse(t, text), Err(PostLinkError::ShortLink), "{text}");
        }
        for text in [
            "https://www.tiktok.com/@space.archives",
            "https://www.tiktok.com/@space.archives/photo/7301234567890123456",
            "https://www.tiktok.com/space/video/7301234567890123456",
        ] {
            assert_eq!(parse(t, text), Err(PostLinkError::NotAPost), "{text}");
        }
    }

    #[test]
    fn instagram_reels_and_posts_give_the_shortcode() {
        let i = Network::InstagramReels;
        for text in [
            "https://www.instagram.com/reel/C9xYz_AbC-1/",
            "https://instagram.com/reels/C9xYz_AbC-1?igsh=abc",
            "https://www.instagram.com/p/C9xYz_AbC-1/",
            "https://www.instagram.com/space.archives/reel/C9xYz_AbC-1/",
        ] {
            assert_eq!(
                parse(i, text),
                Ok((
                    "C9xYz_AbC-1".to_owned(),
                    "https://www.instagram.com/reel/C9xYz_AbC-1/".to_owned()
                )),
                "{text}"
            );
        }
        assert_eq!(
            parse(i, "https://www.instagram.com/space.archives/"),
            Err(PostLinkError::NotAPost)
        );
        assert_eq!(
            parse(i, "https://www.instagram.com/reels/"),
            Err(PostLinkError::NotAPost)
        );
    }

    #[test]
    fn x_and_twitter_links_give_the_post_id() {
        let x = Network::X;
        for (text, url) in [
            (
                "https://x.com/archives/status/1840000000000000001",
                "https://x.com/archives/status/1840000000000000001",
            ),
            (
                "https://twitter.com/archives/status/1840000000000000001/video/1",
                "https://x.com/archives/status/1840000000000000001",
            ),
            (
                "https://mobile.twitter.com/archives/status/1840000000000000001?s=20",
                "https://x.com/archives/status/1840000000000000001",
            ),
            (
                "https://x.com/i/web/status/1840000000000000001",
                "https://x.com/i/status/1840000000000000001",
            ),
            (
                "https://x.com/i/status/1840000000000000001",
                "https://x.com/i/status/1840000000000000001",
            ),
        ] {
            assert_eq!(
                parse(x, text),
                Ok(("1840000000000000001".to_owned(), url.to_owned())),
                "{text}"
            );
        }
        assert_eq!(
            parse(x, "https://x.com/archives"),
            Err(PostLinkError::NotAPost)
        );
    }

    #[test]
    fn kick_videos_and_clips_give_their_ids() {
        let k = Network::Kick;
        let video = "1f2e3d4c-5b6a-4789-8abc-def012345678";
        assert_eq!(
            id(k, &format!("https://kick.com/archives/videos/{video}")),
            video
        );
        assert_eq!(id(k, &format!("https://kick.com/video/{video}")), video);
        assert_eq!(
            parse(k, "https://kick.com/archives/clips/clip_01J9ABCDEF"),
            Ok((
                "clip_01J9ABCDEF".to_owned(),
                "https://kick.com/archives/clips/clip_01J9ABCDEF".to_owned()
            ))
        );
        assert_eq!(
            id(k, "https://kick.com/archives?clip=clip_01J9ABCDEF"),
            "clip_01J9ABCDEF"
        );
        assert_eq!(
            parse(k, "https://kick.com/archives"),
            Err(PostLinkError::NotAPost)
        );
        assert_eq!(
            parse(k, "https://kick.com/archives/videos/not-a-uuid"),
            Err(PostLinkError::NotAPost)
        );
    }

    #[test]
    fn a_link_to_another_network_names_it() {
        assert_eq!(
            parse(
                Network::YouTube,
                "https://www.tiktok.com/@a/video/7301234567890123456"
            ),
            Err(PostLinkError::OtherSite(Some(Network::TikTok)))
        );
        assert_eq!(
            parse(Network::TikTok, "https://youtu.be/dQw4w9WgXcQ"),
            Err(PostLinkError::OtherSite(Some(Network::YouTube)))
        );
        assert_eq!(
            parse(Network::YouTube, "https://example.com/shorts/dQw4w9WgXcQ"),
            Err(PostLinkError::OtherSite(None))
        );
        // A look-alike host is another site.
        assert_eq!(
            parse(
                Network::YouTube,
                "https://youtube.com.evil.io/shorts/dQw4w9WgXcQ"
            ),
            Err(PostLinkError::OtherSite(None))
        );
    }

    #[test]
    fn text_that_is_not_a_link_is_refused() {
        let y = Network::YouTube;
        assert_eq!(parse(y, "   "), Err(PostLinkError::Empty));
        for text in [
            "dQw4w9WgXcQ",
            "my video",
            "ftp://youtube.com/shorts/dQw4w9WgXcQ",
            "javascript:alert(1)",
            "https://user@youtube.com/shorts/dQw4w9WgXcQ",
            "https://youtube.com:8080/shorts/dQw4w9WgXcQ",
            "https://youtube.com/shorts/dQw4w9WgXcQ extra",
        ] {
            assert_eq!(parse(y, text), Err(PostLinkError::NotALink), "{text}");
        }
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    fn publication(network: Network, checked_at: Option<SystemTime>) -> Publication {
        let link = match network {
            Network::YouTube => "https://youtu.be/dQw4w9WgXcQ",
            Network::InstagramReels => "https://www.instagram.com/reel/C1aBcDeFgHi/",
            _ => "https://www.tiktok.com/@a/video/7301234567890123456",
        };
        Publication {
            id: PublicationId::new(),
            owner: ProfileId::new(),
            project: VideoProjectId::new(),
            account: NetworkAccountId::new(),
            network,
            render: RenderId::new(),
            link: Some(PostLink::parse(network, link).unwrap()),
            kind: PublicationKind::Manual,
            posted_at: at(0),
            linked_at: at(0),
            checked_at,
            missing_since: None,
        }
    }

    fn statistics(views: u64) -> VideoStatistics {
        VideoStatistics {
            post_id: "dQw4w9WgXcQ".to_owned(),
            published_at: Some(at(10)),
            views,
            likes: Some(views / 10),
            comments: None,
        }
    }

    #[test]
    fn a_sync_marks_a_post_missing_until_it_is_found_again() {
        let mut p = publication(Network::YouTube, None);
        p.checked(None, at(100));
        assert_eq!(p.missing_since, Some(at(100)));
        p.checked(None, at(200));
        assert_eq!(p.missing_since, Some(at(100)), "missing since the first");
        assert_eq!(p.checked_at, Some(at(200)));

        p.checked(Some(&statistics(5)), at(300));
        assert_eq!(p.missing_since, None);
        assert_eq!(p.posted_at, at(10), "the network's publish time");
    }

    #[test]
    fn only_youtube_posts_have_public_metrics() {
        assert!(publication(Network::TikTok, None).is_posted());
        assert!(publication(Network::YouTube, None).has_public_metrics());
        assert!(!publication(Network::TikTok, None).has_public_metrics());
    }

    fn uploading(visibility: Visibility) -> Publication {
        let mut p = publication(Network::YouTube, None);
        p.link = None;
        p.kind = PublicationKind::Uploaded(Upload::queued(visibility, crate::JobId::new()));
        p.upload_mut().unwrap().start().unwrap();
        p
    }

    fn video() -> PostLink {
        PostLink::parse(
            Network::YouTube,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        )
        .unwrap()
    }

    #[test]
    fn an_upload_gets_its_link_when_sent_and_goes_live_when_processed() {
        let mut p = uploading(Visibility::Public);
        assert_eq!(p.post_id(), None);
        assert!(!p.has_public_metrics(), "no video yet");
        assert!(!p.is_posted());

        p.sent(Some(video())).unwrap();
        assert_eq!(p.post_id(), Some("dQw4w9WgXcQ"));
        assert_eq!(p.upload().unwrap().status, UploadStatus::Processing);
        assert!(!p.has_public_metrics(), "still processing");
        assert!(!p.is_posted(), "still processing");

        p.processed(Visibility::Public, None, at(50)).unwrap();
        assert_eq!(p.upload().unwrap().status, UploadStatus::Published);
        assert_eq!(p.posted_at, at(50));
        assert!(p.has_public_metrics(), "joins the metrics sync");
        assert!(p.is_posted());

        let mut failed = uploading(Visibility::Public);
        failed.sent(Some(video())).unwrap();
        failed
            .upload_mut()
            .unwrap()
            .fail(crate::UploadFailure::Rejected("duplicate".into()))
            .unwrap();
        assert!(!failed.is_posted(), "rejected by the network");
    }

    #[test]
    fn a_reel_gets_its_link_and_the_networks_issue_when_bardo_publishes_it() {
        let mut p = publication(Network::InstagramReels, None);
        p.link = None;
        p.kind = PublicationKind::Uploaded(Upload::queued(Visibility::Public, crate::JobId::new()));
        p.upload_mut().unwrap().start().unwrap();
        p.sent(None).unwrap();
        assert_eq!(p.upload().unwrap().status, UploadStatus::Processing);
        assert_eq!(p.link, None, "no post until it is published");
        assert!(!p.is_posted());

        let reel = PostLink::parse(
            Network::InstagramReels,
            "https://www.instagram.com/reel/C9xYz12AbCd/",
        )
        .unwrap();
        p.published(
            NetworkPost {
                id: "17900000000000001".into(),
                link: Some(reel.clone()),
                issue: Some("Caption not attached".into()),
            },
            at(70),
        )
        .unwrap();
        assert_eq!(p.upload().unwrap().status, UploadStatus::Published);
        assert_eq!(
            p.upload().unwrap().issue.as_deref(),
            Some("Caption not attached")
        );
        assert_eq!(p.link, Some(reel));
        assert_eq!(p.posted_at, at(70));
        assert!(p.is_posted());
        assert!(!p.has_public_metrics(), "no Instagram statistics yet");
        assert!(
            p.published(
                NetworkPost {
                    id: "17900000000000001".into(),
                    link: None,
                    issue: None
                },
                at(80)
            )
            .is_err(),
            "once"
        );

        let mut manual = publication(Network::InstagramReels, None);
        assert!(
            manual
                .published(
                    NetworkPost {
                        id: "17900000000000001".into(),
                        link: None,
                        issue: None
                    },
                    at(1)
                )
                .is_err()
        );
    }

    #[test]
    fn private_and_restricted_uploads_have_no_public_metrics() {
        let mut restricted = uploading(Visibility::Unlisted);
        restricted.sent(Some(video())).unwrap();
        restricted
            .processed(Visibility::Private, None, at(5))
            .unwrap();
        assert_eq!(
            restricted.upload().unwrap().status,
            UploadStatus::Restricted
        );
        assert!(!restricted.has_public_metrics());
        assert!(restricted.is_posted(), "on the channel, private");

        let mut private = uploading(Visibility::Private);
        private.sent(Some(video())).unwrap();
        private.processed(Visibility::Private, None, at(5)).unwrap();
        assert_eq!(private.upload().unwrap().status, UploadStatus::Published);
        assert!(!private.has_public_metrics());
    }

    /// A YouTube upload scheduled for `at(1000)`, processed and waiting.
    fn scheduled() -> Publication {
        let mut p = publication(Network::YouTube, None);
        p.link = None;
        p.kind = PublicationKind::Uploaded(Upload::scheduled(at(1000), crate::JobId::new()));
        p.upload_mut().unwrap().start().unwrap();
        p.sent(Some(video())).unwrap();
        p.processed(Visibility::Private, Some(at(1000)), at(5))
            .unwrap();
        p
    }

    const GRACE: u64 = SCHEDULE_GRACE.as_secs();

    #[test]
    fn a_scheduled_upload_waits_for_its_time_and_is_read_by_the_sync() {
        let p = scheduled();
        assert!(p.is_scheduled());
        assert_eq!(p.posted_at, at(1000), "goes live at its publish time");
        assert!(!p.is_posted(), "not live yet");
        assert!(!p.has_public_metrics(), "private until then");
        assert!(p.is_tracked(), "the sync reads it back");
    }

    #[test]
    fn a_sync_that_finds_it_public_makes_it_published_at_the_networks_time() {
        let mut p = scheduled();
        p.schedule_seen(
            ScheduleReading::Live {
                published_at: Some(at(1003)),
            },
            at(2000),
        );
        assert_eq!(p.upload().unwrap().status, UploadStatus::Published);
        assert_eq!(p.posted_at, at(1003), "YouTube's own publish time");
        assert_eq!(p.checked_at, Some(at(2000)));
        assert!(p.has_public_metrics(), "now tracked for its statistics");
        assert!(p.is_posted());

        let mut no_time = scheduled();
        no_time.schedule_seen(ScheduleReading::Live { published_at: None }, at(2000));
        assert_eq!(no_time.posted_at, at(1000), "the publish time asked");
    }

    #[test]
    fn a_sync_reads_back_a_time_changed_on_the_network() {
        let mut p = scheduled();
        p.schedule_seen(
            ScheduleReading::Private {
                publish_at: Some(at(5000)),
            },
            at(900),
        );
        assert!(p.is_scheduled());
        assert_eq!(p.upload().unwrap().publish_at, Some(at(5000)));
        assert_eq!(p.posted_at, at(5000));
        assert_eq!(p.checked_at, Some(at(900)));
    }

    #[test]
    fn a_video_still_private_past_its_time_was_kept_private_by_the_network() {
        // Shortly after the time, YouTube may not have flipped it yet.
        let mut soon = scheduled();
        let reading = ScheduleReading::Private {
            publish_at: Some(at(1000)),
        };
        soon.schedule_seen(reading, at(1000 + GRACE - 1));
        assert!(soon.is_scheduled());

        let mut kept = scheduled();
        kept.schedule_seen(reading, at(1000 + GRACE));
        assert_eq!(kept.upload().unwrap().status, UploadStatus::Restricted);
        assert!(!kept.has_public_metrics());
        assert_eq!(kept.posted_at, kept.linked_at, "posted from its upload");

        let mut dropped = scheduled();
        dropped.schedule_seen(
            ScheduleReading::Private { publish_at: None },
            at(1000 + GRACE),
        );
        assert_eq!(
            dropped.upload().unwrap().status,
            UploadStatus::Restricted,
            "no publish time any more, past it"
        );
    }

    #[test]
    fn a_schedule_cancelled_on_the_network_before_its_time_leaves_a_private_video() {
        let mut p = scheduled();
        p.schedule_seen(ScheduleReading::Private { publish_at: None }, at(500));
        let upload = p.upload().unwrap();
        assert_eq!(upload.status, UploadStatus::Published);
        assert_eq!(upload.visibility, Visibility::Private);
        assert_eq!(upload.publish_at, None);
        assert_eq!(p.posted_at, p.linked_at, "not the cancelled time");
        assert!(!p.is_tracked(), "a private video has no public metrics");
    }

    #[test]
    fn a_scheduled_video_no_longer_found_is_missing() {
        let mut p = scheduled();
        p.schedule_seen(ScheduleReading::Missing, at(600));
        assert!(p.is_scheduled());
        assert_eq!(p.missing_since, Some(at(600)));
        p.schedule_seen(
            ScheduleReading::Private {
                publish_at: Some(at(1000)),
            },
            at(700),
        );
        assert_eq!(p.missing_since, None, "found again");
    }

    #[test]
    fn only_a_scheduled_post_takes_schedule_readings() {
        let mut manual = publication(Network::YouTube, None);
        let before = manual.clone();
        manual.schedule_seen(ScheduleReading::Live { published_at: None }, at(9));
        assert_eq!(manual, before);
        assert!(manual.rescheduled(at(9)).is_err());
        assert!(manual.unscheduled().is_err());

        let mut p = scheduled();
        p.rescheduled(at(3000)).unwrap();
        assert_eq!(
            (p.upload().unwrap().publish_at, p.posted_at),
            (Some(at(3000)), at(3000))
        );
        p.unscheduled().unwrap();
        assert_eq!(p.upload().unwrap().visibility, Visibility::Private);
        assert_eq!(p.posted_at, p.linked_at, "posted from its upload");
        assert!(p.rescheduled(at(4000)).is_err(), "not scheduled any more");
    }

    #[test]
    fn a_start_syncs_when_a_scheduled_post_passed_its_time() {
        let six = MetricsSyncOnStart::Every6Hours;
        let mut p = scheduled();
        p.checked_at = Some(at(900));
        assert!(!six.is_due(std::slice::from_ref(&p), at(1000)), "not yet");
        assert!(six.is_due(std::slice::from_ref(&p), at(1000 + GRACE)));
        p.checked_at = Some(at(1000 + GRACE));
        assert!(
            !six.is_due(std::slice::from_ref(&p), at(1000 + GRACE + 60)),
            "checked since"
        );
        assert!(!MetricsSyncOnStart::Off.is_due(&[scheduled()], at(9999)));
    }

    #[test]
    fn a_manual_publication_takes_no_upload_steps() {
        let mut p = publication(Network::YouTube, None);
        assert!(p.sent(Some(video())).is_err());
        assert!(p.processed(Visibility::Public, None, at(1)).is_err());
        assert_eq!(p.kind.code(), "manual");
    }

    #[test]
    fn a_start_syncs_when_the_oldest_check_is_older_than_the_interval() {
        let six = MetricsSyncOnStart::Every6Hours;
        let now = at(10 * 3600);
        assert!(!six.is_due(&[], now), "nothing to sync");
        assert!(
            !six.is_due(&[publication(Network::TikTok, None)], now),
            "no public metrics"
        );
        assert!(six.is_due(&[publication(Network::YouTube, None)], now));
        let fresh = publication(Network::YouTube, Some(at(9 * 3600)));
        let old = publication(Network::YouTube, Some(at(3 * 3600)));
        assert!(!six.is_due(std::slice::from_ref(&fresh), now));
        assert!(six.is_due(&[fresh.clone(), old.clone()], now));
        assert!(
            six.is_due(&[fresh.clone(), publication(Network::YouTube, None)], now),
            "a post never checked"
        );
        assert!(!MetricsSyncOnStart::Off.is_due(std::slice::from_ref(&old), now));
        assert!(!MetricsSyncOnStart::Daily.is_due(&[old], now));
        assert!(MetricsSyncOnStart::Hourly.is_due(&[fresh], now));
    }

    #[test]
    fn the_setting_round_trips_and_unknown_values_read_as_six_hours() {
        for setting in MetricsSyncOnStart::ALL {
            assert_eq!(setting.code().parse(), Ok(setting));
        }
        assert_eq!(
            MetricsSyncOnStart::from_code_or_default("7d"),
            MetricsSyncOnStart::Every6Hours
        );
        assert_eq!(
            MetricsSyncOnStart::default().interval(),
            Some(Duration::from_secs(6 * 3600))
        );
    }

    #[test]
    fn quota_is_one_unit_per_fifty_posts() {
        assert_eq!(sync_quota_units(0), 0);
        assert_eq!(sync_quota_units(1), 1);
        assert_eq!(sync_quota_units(50), 1);
        assert_eq!(sync_quota_units(51), 2);
    }

    fn snap(p: PublicationId, secs: u64, views: u64, likes: Option<u64>) -> MetricsSnapshot {
        MetricsSnapshot {
            publication: p,
            taken_at: at(secs),
            views,
            likes,
            comments: Some(1),
            owner: None,
        }
    }

    #[test]
    fn totals_add_the_latest_snapshot_of_each_post() {
        let (a, b) = (PublicationId::new(), PublicationId::new());
        let snapshots = [
            snap(a, 0, 100, Some(10)),
            snap(a, 60, 150, Some(12)),
            snap(b, 30, 40, None),
        ];
        let totals = MetricsTotals::latest(&snapshots);
        assert_eq!(totals.views, 190);
        assert_eq!(totals.likes, Some(12), "hidden likes add nothing");
        assert_eq!(totals.comments, Some(2));
        assert_eq!(totals.posts, 2);
        assert_eq!(MetricsTotals::latest(&[]), MetricsTotals::default());
        assert_eq!(MetricsTotals::latest(&[snap(b, 0, 1, None)]).likes, None);
    }

    fn owned(snapshot: MetricsSnapshot, engaged: u64, revenue: Option<Money>) -> MetricsSnapshot {
        MetricsSnapshot {
            owner: Some(OwnerMetrics {
                views: snapshot.views,
                engaged_views: engaged,
                minutes_watched: engaged / 2,
                average_view_seconds: 20,
                average_view_share: crate::Share::of_percent(50.0),
                earnings: match revenue {
                    Some(revenue) => crate::Earnings::Monetized(crate::MoneyReport {
                        revenue,
                        ..crate::MoneyReport::default()
                    }),
                    None => crate::Earnings::NotMonetized,
                },
            }),
            ..snapshot
        }
    }

    #[test]
    fn owner_totals_keep_each_posts_last_owner_numbers() {
        let (a, b) = (PublicationId::new(), PublicationId::new());
        // The latest sync could not read the owner numbers (quota, or the
        // data had not arrived): the earlier ones still count.
        let snapshots = [
            owned(snap(a, 0, 100, None), 80, Some(Money::from_cents(150))),
            snap(a, 60, 150, None),
            owned(snap(b, 0, 300, None), 200, None),
            snap(b, 60, 320, None),
        ];
        let totals = MetricsTotals::latest(&snapshots);
        assert_eq!(totals.views, 470, "the latest public views");
        let owner = totals.owner.unwrap();
        assert_eq!(owner.engaged_views, 280);
        assert_eq!(owner.revenue, Some(Money::from_cents(150)));
        assert_eq!(owner.posts, 2);
        let history = channel_history(&snapshots);
        assert_eq!(history[1].totals.views, 470);
        assert_eq!(history[1].totals.owner.unwrap().engaged_views, 280);
    }

    #[test]
    fn owner_totals_add_the_posts_that_have_them() {
        let (a, b, c) = (
            PublicationId::new(),
            PublicationId::new(),
            PublicationId::new(),
        );
        let snapshots = [
            owned(snap(a, 0, 100, None), 80, Some(Money::from_cents(150))),
            owned(snap(b, 0, 300, None), 200, None),
            snap(c, 0, 50, None),
        ];
        let totals = MetricsTotals::latest(&snapshots);
        assert_eq!(totals.views, 450, "public views of every post");
        let owner = totals.owner.unwrap();
        assert_eq!(owner.engaged_views, 280);
        assert_eq!(owner.minutes_watched, 140);
        assert_eq!(
            owner.revenue,
            Some(Money::from_cents(150)),
            "monetized only"
        );
        assert_eq!(owner.posts, 2);
        assert_eq!(MetricsTotals::latest(&[snap(c, 0, 5, None)]).owner, None);
        let unpaid = MetricsTotals::latest(&[owned(snap(b, 0, 9, None), 9, None)]);
        assert_eq!(unpaid.owner.unwrap().revenue, None);
    }

    #[test]
    fn channel_history_carries_each_posts_last_numbers_forward() {
        let (a, b) = (PublicationId::new(), PublicationId::new());
        let snapshots = [
            snap(b, 100, 40, Some(4)),
            snap(a, 0, 100, Some(10)),
            snap(a, 100, 150, Some(15)),
            snap(a, 200, 200, Some(20)),
        ];
        let history = channel_history(&snapshots);
        let views: Vec<(SystemTime, u64, usize)> = history
            .iter()
            .map(|point| (point.at, point.totals.views, point.totals.posts))
            .collect();
        assert_eq!(
            views,
            [(at(0), 100, 1), (at(100), 190, 2), (at(200), 240, 2)]
        );
        assert_eq!(history[2].totals.likes, Some(24));
        assert!(channel_history(&[]).is_empty());
    }
}
