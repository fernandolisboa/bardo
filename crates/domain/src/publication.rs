//! Manual publications and their metrics (PRD stories 79-81): after
//! posting an export by hand, the user pastes the post's link; Bardo checks
//! it belongs to the network, keeps the post's id, and from then on tracks
//! the post. On YouTube a metrics sync reads the public statistics (views,
//! likes, comments) with the Data API key and keeps a snapshot per sync;
//! the other networks keep the link until publishing brings their metrics.
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
    ApiKey, ChannelId, Network, NetworkAccountId, ProfileId, ProviderFailure, RenderId,
    RepositoryError, VideoProjectId,
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

/// A post the user made of a video project's export on one network
/// account, linked to Bardo by its address. One per project and network:
/// linking again replaces the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publication {
    pub id: PublicationId,
    pub owner: ProfileId,
    pub project: VideoProjectId,
    pub account: NetworkAccountId,
    /// The render the export it was posted from copied.
    pub render: RenderId,
    pub link: PostLink,
    /// When it went live: when the user linked it, until the network says
    /// (YouTube's publish time, after a sync).
    pub posted_at: SystemTime,
    /// When the user linked it.
    pub linked_at: SystemTime,
    /// The last sync that looked for the post, found or not.
    pub checked_at: Option<SystemTime>,
    /// Since when syncs have not found the post (removed or made private).
    pub missing_since: Option<SystemTime>,
}

impl Publication {
    pub fn network(&self) -> Network {
        self.link.network()
    }

    /// Whether a metrics sync reads this post: public statistics exist
    /// for YouTube only until publishing brings the others' (ADR-0004).
    pub fn has_public_metrics(&self) -> bool {
        self.network() == Network::YouTube
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
    pub views: u64,
    pub likes: Option<u64>,
    pub comments: Option<u64>,
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
        }
    }
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

    /// Whether a start at `now` syncs `publications`: some has public
    /// metrics and was never checked, or the oldest check is older than
    /// the interval. Off never syncs.
    pub fn is_due(self, publications: &[Publication], now: SystemTime) -> bool {
        let Some(interval) = self.interval() else {
            return false;
        };
        let mut checks = publications
            .iter()
            .filter(|publication| publication.has_public_metrics())
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

    /// The latest snapshot of each post added up.
    pub fn latest(snapshots: &[MetricsSnapshot]) -> Self {
        let mut totals = Self::default();
        for snapshot in latest_of_each(snapshots).values() {
            totals.add(snapshot);
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
/// at that moment counts with its previous numbers). Oldest first.
pub fn channel_history(snapshots: &[MetricsSnapshot]) -> Vec<ChannelPoint> {
    let mut ordered: Vec<&MetricsSnapshot> = snapshots.iter().collect();
    ordered.sort_by_key(|snapshot| snapshot.taken_at);
    let mut latest: HashMap<PublicationId, MetricsSnapshot> = HashMap::new();
    let mut points: Vec<ChannelPoint> = Vec::new();
    // A sync stamps every post with the same time: one point per sync.
    for (ix, snapshot) in ordered.iter().enumerate() {
        latest.insert(snapshot.publication, **snapshot);
        let last_of_its_time = ordered
            .get(ix + 1)
            .is_none_or(|next| next.taken_at != snapshot.taken_at);
        if last_of_its_time {
            let mut totals = MetricsTotals::default();
            for kept in latest.values() {
                totals.add(kept);
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
            _ => "https://www.tiktok.com/@a/video/7301234567890123456",
        };
        Publication {
            id: PublicationId::new(),
            owner: ProfileId::new(),
            project: VideoProjectId::new(),
            account: NetworkAccountId::new(),
            render: RenderId::new(),
            link: PostLink::parse(network, link).unwrap(),
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
        assert!(publication(Network::YouTube, None).has_public_metrics());
        assert!(!publication(Network::TikTok, None).has_public_metrics());
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
