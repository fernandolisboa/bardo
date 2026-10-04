//! Manual publications and public YouTube metrics (PRD stories 79-81).
//!
//! After posting an export by hand, the user pastes the post's link at
//! the Publish stage; Bardo checks it belongs to the network
//! (`PostLink::parse`) and keeps it as the project's publication on that
//! network. YouTube posts are then tracked: a metrics sync job reads their
//! public statistics with the Data API key (no OAuth), 50 posts per call
//! for one quota unit, and keeps a snapshot of each. The sync runs when
//! the user asks, when a YouTube post is linked, and on start when the
//! oldest check is older than the profile's setting. The other networks
//! keep their link until publishing brings their metrics.
//!
//! A scheduled upload (#78) is private until its publish time, so public
//! statistics cannot see it: the sync reads it through the owner's
//! connection instead, records when it went live (or that the network kept
//! it private), and from then on tracks it like any other post. A sync of
//! scheduled uploads alone needs no Data API key.
//!
//! When the channel's account is connected, the sync also reads the
//! owner's numbers of each post it found (#79): YouTube Analytics through
//! the account's own token, a report and a retention curve per post. They
//! go on the same snapshot; a channel outside the Partner Program reads as
//! not monetized after its first refused money report. Nothing the owner
//! reads can fail the sync: an account that cannot be read keeps its public
//! numbers, and an unconnected one is never asked.
//!
//! Instagram and TikTok posts are read through their own connected account
//! (#85): Instagram's media insights one post at a time (a linked post's
//! media id found once from its shortcode by listing the account's media),
//! TikTok's video query 20 at a time. They need no Data API key. A post the
//! network does not show to its account is flagged like a YouTube post not
//! found; an account that is not connected keeps its links only, as before.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, ChannelId, ChannelPoint, InsightsReader, Job, JobFailure, JobFailureKind, JobId,
    JobKind, MEDIA_PAGES, MetricsSnapshot, MetricsSyncOnStart, MetricsTotals, Monetization,
    Network, NetworkAccount, NetworkAccountId, NetworkAccountRepository, OwnerAnalytics, PostLink,
    PostLinkError, PostReading, PostRetention, ProfileId, Progress, Provider, Publication,
    PublicationId, PublicationKind, PublicationRepository, ReportPeriod, RepositoryError,
    STATS_BATCH, SecretStore, SecretText, UploadStatus, UserProfile, VideoProjectId, VideoStats,
    VideoUploader, channel_history, find_media, read_insights, read_owner_metrics,
};
use serde::{Deserialize, Serialize};

use crate::connections::{ConnectionError, Connections};
use crate::jobs::{JobContext, JobHandler};
use crate::scenes::{id, parse, to_json, unexpected};
use crate::schedules::read_schedule;
use crate::{AppError, Bardo, KeyState, Text};

#[derive(Debug, thiserror::Error)]
pub enum PublicationError {
    #[error("video project not found")]
    ProjectNotFound,
    /// The channel has no account on the network.
    #[error("the channel has no account on this network")]
    NoAccount,
    /// The network was never exported: there is nothing to have posted.
    #[error("the network has no export yet")]
    NotExported,
    #[error("not a post link: {0:?}")]
    Link(PostLinkError),
    /// The post is already linked to another project.
    #[error("the post is linked to another project")]
    AlreadyLinked,
    #[error("publication not found")]
    NotFound,
    /// The network's publication is an upload; linking a post replaces it
    /// only once the user confirms.
    #[error("linking a post replaces the upload")]
    ReplacesUpload,
    /// The network's upload is running or waiting: stop it first.
    #[error("the network's upload is running")]
    Uploading,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl PublicationError {
    /// What the Publish stage says. A link error names its network
    /// through `Bardo::post_link_problem` instead.
    pub fn message(&self) -> Text {
        match self {
            PublicationError::ProjectNotFound => Text::ProjectNotFound,
            PublicationError::NoAccount => Text::PublicationNoAccount,
            PublicationError::NotExported => Text::PublicationNotExported,
            PublicationError::Link(error) => Text::PostLinkProblem(*error),
            PublicationError::AlreadyLinked => Text::PublicationAlreadyLinked,
            PublicationError::NotFound => Text::PublicationNotFound,
            PublicationError::ReplacesUpload => Text::PublicationReplacesUpload,
            PublicationError::Uploading => Text::PublicationUploading,
            PublicationError::Repository(_) => Text::PublicationNotSaved,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MetricsError {
    /// No YouTube Data API key is saved.
    #[error("no YouTube Data API key saved")]
    MissingKey,
    /// No YouTube publication to sync.
    #[error("nothing to sync")]
    NothingToSync,
    #[error("a metrics sync is already running")]
    AlreadySyncing,
    #[error("channel not found")]
    ChannelNotFound,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl MetricsError {
    pub fn message(&self) -> Text {
        match self {
            MetricsError::MissingKey => Text::MetricsMissingKey,
            MetricsError::NothingToSync => Text::MetricsNothingToSync,
            MetricsError::AlreadySyncing => Text::MetricsAlreadySyncing,
            MetricsError::ChannelNotFound => Text::ChannelNotFound,
            MetricsError::Repository(_) => Text::MetricsNotLoaded,
        }
    }
}

impl From<AppError> for MetricsError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Repository(error) => MetricsError::Repository(error),
        }
    }
}

/// A publication with its snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedPost {
    pub publication: Publication,
    /// Oldest first; empty until a sync found the post.
    pub history: Vec<MetricsSnapshot>,
    /// The retention curve as last read, for a connected channel's post.
    pub retention: Option<PostRetention>,
    /// For an Instagram or TikTok post, whether its account is connected:
    /// syncs read it only then. `None` on the other networks.
    pub access: Option<OwnerAccess>,
}

impl PublishedPost {
    pub fn latest(&self) -> Option<&MetricsSnapshot> {
        self.history.last()
    }

    /// The latest views minus the views one snapshot before, when there
    /// are two.
    pub fn views_change(&self) -> Option<i64> {
        let [.., before, last] = self.history.as_slice() else {
            return None;
        };
        Some(before.views_to(last))
    }

    /// The latest snapshot that has the owner's numbers, which may be
    /// older than the latest one when the network's data had not arrived.
    pub fn latest_owner(&self) -> Option<&MetricsSnapshot> {
        self.history.iter().rev().find(|s| s.owner.is_some())
    }

    /// Whether syncs read the post: YouTube's public numbers, or an
    /// Instagram or TikTok post of a connected account.
    pub fn is_synced(&self) -> bool {
        self.publication.has_public_metrics()
            || (self.publication.reads_insights() && self.access == Some(OwnerAccess::Connected))
    }

    /// The engaged views screens lead with, from the latest owner's
    /// numbers; `None` leaves the public views in the lead.
    pub fn engaged_views(&self) -> Option<u64> {
        self.latest_owner()
            .and_then(|snapshot| snapshot.owner)
            .map(|owner| owner.engaged_views)
    }
}

/// Whether an account's own numbers can be read: the channel's YouTube
/// account for the owner metrics, or a post's Instagram or TikTok account
/// for its insights, and that account's connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerAccess {
    /// The channel has no YouTube account.
    NoAccount,
    NotConnected,
    Connected,
    /// The network refused a refresh: syncs keep the public numbers only.
    ReconnectNeeded,
}

/// Where metrics syncing stands, for the screens that show metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsStatus {
    /// The latest metrics sync job.
    pub job: Option<Job>,
    /// Whether a YouTube Data API key is saved.
    pub key_saved: bool,
    /// Whether a sync needs the key to read anything: every tracked post
    /// is read with it (a scheduled one, and Instagram and TikTok posts,
    /// are read through their account's connection; without a key a sync
    /// reads only those).
    pub needs_key: bool,
    pub on_start: MetricsSyncOnStart,
    /// Publications a sync reads: YouTube's, scheduled ones included, and
    /// the Instagram and TikTok posts of connected accounts.
    pub tracked: usize,
    /// Of those, the YouTube posts (scheduled ones too): their public
    /// numbers need the key.
    pub with_key: usize,
    /// The latest check of any of them.
    pub last_checked: Option<SystemTime>,
}

impl MetricsStatus {
    pub fn is_syncing(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }

    /// Whether "Sync now" may start a sync.
    pub fn can_sync(&self) -> bool {
        (self.key_saved || !self.needs_key) && self.tracked > 0 && !self.is_syncing()
    }
}

/// One post of a channel, for its metrics screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelPost {
    pub project: VideoProjectId,
    pub project_title: String,
    pub post: PublishedPost,
}

/// A channel's posts and how they perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMetricsView {
    pub channel: ChannelId,
    /// Newest post first, every network.
    pub posts: Vec<ChannelPost>,
    /// The latest numbers of every tracked post, added up.
    pub totals: MetricsTotals,
    /// The totals over time, oldest first.
    pub history: Vec<ChannelPoint>,
    pub status: MetricsStatus,
    /// Whether the owner numbers of its YouTube posts can be read.
    pub owner_access: OwnerAccess,
}

/// `time` as storage keeps it, so what `mark_posted` returns equals what
/// is read back.
pub(crate) fn whole_millis(time: SystemTime) -> SystemTime {
    let since = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(since.as_millis() as u64)
}

/// The sync job's payload: the publications it reads, fixed when queued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SyncPayload {
    publications: Vec<String>,
}

/// How many batches of the payload a sync job has saved, and the time
/// its snapshots carry: one per job, so a channel's history gets one
/// point per sync however many batches it reads, a resumed job included.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SyncCheckpoint {
    batches: usize,
    /// Unix time in milliseconds.
    #[serde(default)]
    taken_at: Option<u64>,
}

/// Runs metrics syncs.
pub(crate) struct MetricsSyncHandler {
    pub(crate) owner: ProfileId,
    pub(crate) publications: Arc<dyn PublicationRepository>,
    pub(crate) stats: Arc<dyn VideoStats>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) accounts: Arc<dyn NetworkAccountRepository>,
    pub(crate) connections: Connections,
    pub(crate) uploaders: Vec<Arc<dyn VideoUploader>>,
    pub(crate) analytics: Vec<Arc<dyn OwnerAnalytics>>,
    pub(crate) post_insights: Vec<Arc<dyn InsightsReader>>,
}

/// How one sync reads an account's owner numbers.
enum OwnerAccount {
    /// To read, and what this sync learned of the channel's money.
    Reading {
        account: NetworkAccount,
        monetization: Monetization,
    },
    /// Not connected, or something stopped its reads for this sync.
    Off,
}

impl MetricsSyncHandler {
    fn saved_key(&self) -> Result<Option<ApiKey>, JobFailure> {
        self.secrets
            .get(self.owner, Provider::YouTubeData)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))
    }

    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.saved_key()?.ok_or_else(|| {
            JobFailure::new(
                JobFailureKind::MissingKey,
                "no YouTube Data API key is saved",
            )
        })
    }

    /// The account's owner reads for this sync, until its token is
    /// refused or missing.
    fn owner_account(&self, account: NetworkAccountId) -> OwnerAccount {
        let account = match self.accounts.get(account) {
            Ok(Some(account)) => account,
            Ok(None) => return OwnerAccount::Off,
            Err(error) => {
                tracing::warn!("could not read the account for owner metrics: {error}");
                return OwnerAccount::Off;
            }
        };
        OwnerAccount::Reading {
            account,
            monetization: Monetization::Unknown,
        }
    }

    /// The account's token, renewed when it is close to expiring: a long
    /// sync can outlast the one it started with.
    fn owner_token(&self, account: &NetworkAccount) -> Option<SecretText> {
        match self.connections.access_token(account) {
            Ok(tokens) => Some(SecretText::new(tokens.access_token())),
            Err(ConnectionError::NotConnected) => None,
            Err(error) => {
                tracing::warn!("owner metrics skipped, no sign-in: {error}");
                None
            }
        }
    }

    /// Adds the owner's numbers to the snapshots of `posts` whose account
    /// is connected, and gathers their retention curves. Returns false
    /// when asked to stop.
    fn read_owner(
        &self,
        posts: &[Publication],
        snapshots: &mut [MetricsSnapshot],
        curves: &mut Vec<PostRetention>,
        accounts: &mut HashMap<NetworkAccountId, OwnerAccount>,
        cx: &JobContext,
    ) -> bool {
        for snapshot in snapshots {
            let Some(post) = posts.iter().find(|post| post.id == snapshot.publication) else {
                continue;
            };
            let (Some(video), Some(analytics)) = (
                post.post_id(),
                self.analytics
                    .iter()
                    .find(|analytics| analytics.network() == post.network()),
            ) else {
                continue;
            };
            if cx.should_stop() {
                return false;
            }
            let account = accounts
                .entry(post.account)
                .or_insert_with(|| self.owner_account(post.account));
            let OwnerAccount::Reading {
                account: network_account,
                monetization,
            } = account
            else {
                continue;
            };
            let Some(token) = self.owner_token(network_account) else {
                *account = OwnerAccount::Off;
                continue;
            };
            let period = ReportPeriod::for_post(post.posted_at, snapshot.taken_at);
            match read_owner_metrics(&**analytics, &token, video, &period, monetization) {
                Ok(reading) => {
                    snapshot.owner = reading.metrics;
                    if !reading.retention.is_empty() {
                        curves.push(PostRetention {
                            publication: post.id,
                            read_at: snapshot.taken_at,
                            curve: reading.retention,
                        });
                    }
                }
                Err(error) if error.stops_the_account() => {
                    tracing::warn!("owner metrics stopped for this sync: {error}");
                    *account = OwnerAccount::Off;
                }
                // Not the channel's video, or a report it could not read:
                // the post keeps its public numbers.
                Err(error) => tracing::warn!("could not read a post's owner metrics: {error}"),
            }
        }
        true
    }
}

impl MetricsSyncHandler {
    /// Reads Instagram and TikTok `posts` through their accounts: a
    /// snapshot of each post with numbers into `snapshots`, and the posts
    /// it learned something of (checked, or a media id found) returned.
    /// A post whose account is not connected, or stopped for this sync,
    /// stays as it was. `None` when asked to stop.
    fn read_networked(
        &self,
        posts: Vec<Publication>,
        taken_at: SystemTime,
        snapshots: &mut Vec<MetricsSnapshot>,
        accounts: &mut HashMap<NetworkAccountId, OwnerAccount>,
        cx: &JobContext,
    ) -> Option<Vec<Publication>> {
        let mut by_account: Vec<(NetworkAccountId, Vec<Publication>)> = Vec::new();
        for post in posts {
            match by_account.iter_mut().find(|(id, _)| *id == post.account) {
                Some((_, group)) => group.push(post),
                None => by_account.push((post.account, vec![post])),
            }
        }
        let mut learned = Vec::new();
        for (id, mut group) in by_account {
            if cx.should_stop() {
                return None;
            }
            let Some(reader) = self
                .post_insights
                .iter()
                .find(|reader| reader.network() == group[0].network())
            else {
                continue;
            };
            let account = accounts.entry(id).or_insert_with(|| self.owner_account(id));
            let OwnerAccount::Reading {
                account: network_account,
                ..
            } = account
            else {
                continue;
            };
            let Some(token) = self.owner_token(network_account) else {
                *account = OwnerAccount::Off;
                continue;
            };
            let network_account = network_account.clone();
            match self.read_account(&**reader, &network_account, &token, &mut group, taken_at) {
                Ok(read) => snapshots.extend(read),
                Err(error) => {
                    tracing::warn!("insights stopped for this sync: {error}");
                    *account = OwnerAccount::Off;
                }
            }
            learned.extend(
                group
                    .into_iter()
                    .filter(|post| post.checked_at == Some(taken_at) || post.insights_id.is_some()),
            );
        }
        Some(learned)
    }

    /// Reads one account's `posts`: finds the media of linked Instagram
    /// posts that have none yet, then reads every post with an id. Returns
    /// the snapshots; fails only with what stops the account.
    fn read_account(
        &self,
        reader: &dyn InsightsReader,
        account: &NetworkAccount,
        token: &SecretText,
        posts: &mut [Publication],
        taken_at: SystemTime,
    ) -> Result<Vec<MetricsSnapshot>, bardo_domain::AnalyticsError> {
        let mut listed_at: HashMap<PublicationId, SystemTime> = HashMap::new();
        let codes: Vec<String> = posts
            .iter()
            .filter(|post| post.insights_post().is_none())
            .filter_map(|post| post.post_id().map(str::to_owned))
            .collect();
        if !codes.is_empty() {
            let identity = match self.connections.identity(account) {
                Ok(identity) => identity,
                Err(error) => {
                    return Err(bardo_domain::AnalyticsError::new(
                        bardo_domain::AnalyticsErrorKind::SignedOut,
                        error.to_string(),
                    ));
                }
            };
            let codes: Vec<&str> = codes.iter().map(String::as_str).collect();
            match find_media(reader, token, &identity.id, &codes, MEDIA_PAGES) {
                Ok(lookup) => {
                    for post in posts.iter_mut().filter(|p| p.insights_post().is_none()) {
                        let Some(code) = post.post_id().map(str::to_owned) else {
                            continue;
                        };
                        match lookup.media(&code) {
                            Ok(media) => {
                                post.insights_id = Some(media.id.clone());
                                if let Some(at) = media.posted_at {
                                    listed_at.insert(post.id, at);
                                }
                            }
                            // The whole listing read: not the account's.
                            Err(true) => post.not_seen(taken_at),
                            // Past the pages read: the next sync looks again.
                            Err(false) => {}
                        }
                    }
                }
                Err(error) if error.stops_the_account() => return Err(error),
                Err(error) => tracing::warn!("could not list the account's media: {error}"),
            }
        }
        let ids: Vec<String> = posts
            .iter()
            .filter_map(|post| post.insights_post().map(str::to_owned))
            .collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut snapshots = Vec::new();
        for (id, reading) in read_insights(reader, token, &ids)? {
            let Some(post) = posts
                .iter_mut()
                .find(|post| post.insights_post() == Some(id.as_str()))
            else {
                continue;
            };
            match reading {
                PostReading::Found(numbers) => {
                    let posted_at = numbers.posted_at.or_else(|| {
                        // A linked post went up when Instagram says, not
                        // when it was linked; an upload keeps its own.
                        post.upload()
                            .is_none()
                            .then(|| listed_at.get(&post.id).copied())
                            .flatten()
                    });
                    post.seen(posted_at, taken_at);
                    snapshots.extend(MetricsSnapshot::of_numbers(post.id, &numbers, taken_at));
                }
                PostReading::NotFound => post.not_seen(taken_at),
                PostReading::Unread => {}
            }
        }
        Ok(snapshots)
    }
}

impl JobHandler for MetricsSyncHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: SyncPayload = parse(payload)?;
        let mut checkpoint: SyncCheckpoint = match cx.checkpoint() {
            Some(text) => parse(text)?,
            None => SyncCheckpoint::default(),
        };
        let batches: Vec<&[String]> = payload.publications.chunks(STATS_BATCH).collect();
        let total = batches.len() as u64;
        if checkpoint.batches >= batches.len() {
            return Ok(());
        }
        let mut key = None;
        let mut owners: HashMap<NetworkAccountId, OwnerAccount> = HashMap::new();
        let taken_at = match checkpoint.taken_at {
            Some(millis) => SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(millis),
            None => whole_millis(SystemTime::now()),
        };
        checkpoint.taken_at = Some(
            taken_at
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
        for batch in &batches[checkpoint.batches..] {
            if cx.should_stop() {
                return Ok(());
            }
            // Read again: the user may have removed or relinked a post
            // since the sync was queued.
            let mut posts = Vec::new();
            let mut networked = Vec::new();
            for text in *batch {
                let publication: PublicationId = id(text)?;
                let Some(post) = self
                    .publications
                    .publication(publication)
                    .map_err(unexpected)?
                else {
                    continue;
                };
                if post.reads_insights() {
                    networked.push(post);
                } else if post.is_tracked() {
                    posts.push(post);
                }
            }
            // Scheduled posts first: one that went live joins the
            // statistics read below, when a key is saved.
            let had_public = posts.iter().any(Publication::has_public_metrics);
            let was_scheduled: Vec<PublicationId> = posts
                .iter()
                .filter(|post| post.is_scheduled())
                .map(|post| post.id)
                .collect();
            let mut scheduled = Vec::new();
            for post in posts.iter_mut().filter(|post| post.is_scheduled()) {
                if cx.should_stop() {
                    return Ok(());
                }
                let Some(from) = post.upload().and_then(|upload| upload.publish_at) else {
                    continue;
                };
                match read_schedule(&self.connections, &*self.accounts, &self.uploaders, post) {
                    Ok(Some(reading)) => {
                        post.schedule_seen(reading, SystemTime::now());
                        // A change the user made meanwhile wins over this
                        // read; the next sync reads it again.
                        if self
                            .publications
                            .save_schedule(post, from)
                            .map_err(unexpected)?
                        {
                            scheduled.push(post.clone());
                        }
                    }
                    Ok(None) => {}
                    // The post stays scheduled; the next sync tries again.
                    Err(detail) => tracing::warn!("could not read a scheduled upload: {detail}"),
                }
            }
            // A read that a change made meanwhile overruled is dropped, so
            // its post is not read as live.
            let mut public: Vec<Publication> = posts
                .into_iter()
                .filter(|post| {
                    !was_scheduled.contains(&post.id) || scheduled.iter().any(|p| p.id == post.id)
                })
                .filter(Publication::has_public_metrics)
                .collect();
            if !had_public && !public.is_empty() && key.is_none() && self.saved_key()?.is_none() {
                // Only posts that just went live: their numbers wait for a
                // key, rather than failing the sync of the schedules.
                public.clear();
            }
            let mut snapshots = Vec::new();
            let mut curves = Vec::new();
            if !public.is_empty() {
                let key = match &key {
                    Some(key) => key,
                    None => key.insert(self.key()?),
                };
                let ids: Vec<&str> = public.iter().filter_map(Publication::post_id).collect();
                let found = self.stats.statistics(key, &ids).map_err(|failure| {
                    JobFailure::new(failure.kind.into(), format!("YouTube: {}", failure.detail))
                })?;
                for post in &mut public {
                    let statistics = found
                        .iter()
                        .find(|statistics| Some(statistics.post_id.as_str()) == post.post_id());
                    post.checked(statistics, taken_at);
                    if let Some(statistics) = statistics {
                        snapshots.push(MetricsSnapshot::of(post.id, statistics, taken_at));
                    }
                }
                // Read after the statistics, which set when the post went up.
                if !self.read_owner(&public, &mut snapshots, &mut curves, &mut owners, cx) {
                    return Ok(());
                }
            }
            let Some(networked) =
                self.read_networked(networked, taken_at, &mut snapshots, &mut owners, cx)
            else {
                return Ok(());
            };
            // A post that went live is in both lists; its statistics read
            // is the later word.
            scheduled.retain(|post| public.iter().all(|p| p.id != post.id));
            let checked: Vec<Publication> = scheduled
                .into_iter()
                .chain(public)
                .chain(networked)
                .collect();
            if !checked.is_empty() {
                self.publications
                    .save_sync(&checked, &snapshots)
                    .map_err(unexpected)?;
            }
            if !curves.is_empty() {
                self.publications
                    .save_retention(&curves)
                    .map_err(unexpected)?;
            }
            checkpoint.batches += 1;
            cx.save_checkpoint(
                to_json(&checkpoint),
                Progress::of(checkpoint.batches as u64, total),
            )
            .map_err(unexpected)?;
        }
        Ok(())
    }
}

impl Bardo {
    /// The latest metrics sync job.
    pub fn latest_sync_job(&self) -> Option<Job> {
        self.jobs()
            .into_iter()
            .rev()
            .find(|job| job.kind() == JobKind::MetricsSync)
    }

    fn youtube_key_saved(&self) -> bool {
        matches!(
            self.provider_key(Provider::YouTubeData).state,
            KeyState::Saved { .. }
        )
    }

    /// Where metrics syncing stands for `publications`.
    pub(crate) fn metrics_status(&self, publications: &[Publication]) -> MetricsStatus {
        let access = self.insights_access(publications);
        let tracked: Vec<&Publication> = publications
            .iter()
            .filter(|publication| is_synced(publication, &access))
            .collect();
        MetricsStatus {
            job: self.latest_sync_job(),
            key_saved: self.youtube_key_saved(),
            needs_key: !tracked
                .iter()
                .any(|p| p.is_scheduled() || p.reads_insights()),
            on_start: self.profile.metrics_sync,
            tracked: tracked.len(),
            with_key: tracked.iter().filter(|p| !p.reads_insights()).count(),
            last_checked: tracked.iter().filter_map(|p| p.checked_at).max(),
        }
    }

    /// The connection of every account whose posts read insights, by id.
    pub(crate) fn insights_access(
        &self,
        publications: &[Publication],
    ) -> HashMap<NetworkAccountId, OwnerAccess> {
        let mut access = HashMap::new();
        for publication in publications.iter().filter(|p| p.network().reads_insights()) {
            access
                .entry(publication.account)
                .or_insert_with(|| self.account_access(publication.account));
        }
        access
    }

    /// Whether the account is connected, as the owner's reads see it.
    fn account_access(&self, id: NetworkAccountId) -> OwnerAccess {
        match self.network_accounts.get(id) {
            Ok(Some(account)) => self.connection_access(&account),
            Ok(None) => OwnerAccess::NoAccount,
            Err(error) => {
                tracing::warn!("could not read the account: {error}");
                OwnerAccess::NotConnected
            }
        }
    }

    fn connection_access(&self, account: &NetworkAccount) -> OwnerAccess {
        match self.connection_state(account) {
            Ok(crate::ConnectionState::Connected { .. }) => OwnerAccess::Connected,
            Ok(crate::ConnectionState::ReconnectNeeded { .. }) => OwnerAccess::ReconnectNeeded,
            Ok(_) => OwnerAccess::NotConnected,
            Err(error) => {
                tracing::warn!("could not read the connection: {error}");
                OwnerAccess::NotConnected
            }
        }
    }

    /// The project's publications with their snapshots, in `Network::ALL`
    /// order.
    pub(crate) fn published_posts(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<PublishedPost>, RepositoryError> {
        let publications = self.publications.publications(project)?;
        let access = self.insights_access(&publications);
        publications
            .into_iter()
            .map(|publication| {
                Ok(PublishedPost {
                    history: self.publications.snapshots(publication.id)?,
                    retention: self.publications.retention(publication.id)?,
                    access: access.get(&publication.account).copied(),
                    publication,
                })
            })
            .collect()
    }

    /// Where metrics syncing stands for every publication of the profile.
    pub fn metrics_sync_status(&self) -> Result<MetricsStatus, MetricsError> {
        let publications = self.publications.all_publications(self.profile.id)?;
        Ok(self.metrics_status(&publications))
    }

    /// Links the post the user made of the project's export on `network`,
    /// from its address. Linking the same post again keeps its metrics;
    /// another post replaces the publication and its metrics. Replacing an
    /// upload needs `replace` (the user confirmed it), and waits until the
    /// upload stops. A TikTok draft Bardo sent to the inbox becomes the post
    /// the creator made of it: no export and no confirmation needed. A
    /// YouTube post is synced right away when the key is saved.
    pub fn mark_posted(
        &self,
        project: VideoProjectId,
        network: Network,
        link: &str,
        replace: bool,
    ) -> Result<Publication, PublicationError> {
        let project = self
            .themes
            .project(project)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(PublicationError::ProjectNotFound)?;
        let account = self
            .network_accounts
            .list(project.channel)?
            .into_iter()
            .find(|account| account.network == network)
            .ok_or(PublicationError::NoAccount)?;
        let current = self
            .publications
            .publications(project.id)?
            .into_iter()
            .find(|publication| publication.network() == network);
        // A TikTok upload is the video the creator posts from the inbox:
        // its render, with no export. One that reached the inbox is replaced
        // without asking again; another (say, still processing when Bardo
        // stopped checking) only once the user confirms.
        let sent = current
            .as_ref()
            .filter(|publication| network.uploads_drafts() && publication.upload().is_some());
        let draft = sent.filter(|publication| {
            publication
                .upload()
                .is_some_and(|upload| upload.status == UploadStatus::DraftSent)
        });
        let render = match sent {
            Some(sent) => sent.render,
            None => {
                self.exports
                    .exports(project.id)?
                    .into_iter()
                    .find(|export| export.network == network)
                    .ok_or(PublicationError::NotExported)?
                    .render
            }
        };
        let replace = replace || draft.is_some();
        let link = PostLink::parse(network, link).map_err(PublicationError::Link)?;
        let elsewhere = self
            .publications
            .all_publications(self.profile.id)?
            .into_iter()
            .any(|other| {
                other.project != project.id
                    && other.network() == network
                    && other.post_id() == Some(link.post_id())
            });
        if elsewhere {
            return Err(PublicationError::AlreadyLinked);
        }
        let now = whole_millis(SystemTime::now());
        if let Some(upload) = current.as_ref().and_then(Publication::upload) {
            if self.job_active(upload.job) {
                return Err(PublicationError::Uploading);
            }
            if !replace {
                return Err(PublicationError::ReplacesUpload);
            }
        }
        let publication = match current {
            Some(current) if current.post_id() == Some(link.post_id()) => Publication {
                link: Some(link),
                // A Reel Bardo published keeps its media id for insights.
                insights_id: current.insights_post().map(str::to_owned).filter(|_| {
                    current.network() == Network::InstagramReels && account.id == current.account
                }),
                account: account.id,
                kind: PublicationKind::Manual,
                ..current
            },
            _ => Publication {
                id: PublicationId::new(),
                owner: self.profile.id,
                project: project.id,
                account: account.id,
                network,
                render,
                link: Some(link),
                kind: PublicationKind::Manual,
                posted_at: now,
                linked_at: now,
                checked_at: None,
                missing_since: None,
                insights_id: None,
            },
        };
        self.publications.save_publication(&publication)?;
        let readable = if publication.reads_insights() {
            self.account_access(publication.account) == OwnerAccess::Connected
        } else {
            publication.has_public_metrics() && self.youtube_key_saved()
        };
        if readable
            && publication.checked_at.is_none()
            && !self
                .latest_sync_job()
                .is_some_and(|job| job.state().is_active())
        {
            // The link stands even if the sync cannot be queued; the next
            // sync reads it.
            if let Err(error) = self.queue_sync(vec![publication.id]) {
                tracing::warn!("could not queue a metrics sync: {error}");
            }
        }
        Ok(publication)
    }

    /// Unlinks a post: its publication and metrics go; the post stays on
    /// the network. An upload is unlinked only once it stopped.
    pub fn remove_publication(&self, id: PublicationId) -> Result<(), PublicationError> {
        let publication = self
            .publications
            .publication(id)?
            .filter(|publication| publication.owner == self.profile.id)
            .ok_or(PublicationError::NotFound)?;
        if publication
            .upload()
            .is_some_and(|upload| self.job_active(upload.job))
        {
            return Err(PublicationError::Uploading);
        }
        Ok(self.publications.remove_publication(publication.id)?)
    }

    /// The problem with a pasted link, as the Publish stage says it.
    pub fn post_link_problem(&self, error: PostLinkError, network: Network) -> String {
        let other = match error {
            PostLinkError::OtherSite(Some(other)) => self.text(Text::NetworkName(other)),
            _ => self.text(Text::NetworkName(network)),
        };
        self.text_with(
            Text::PostLinkProblem(error),
            &[
                ("network", &self.text(Text::NetworkName(network))),
                ("other", &other),
            ],
        )
    }

    fn queue_sync(&self, publications: Vec<PublicationId>) -> Result<JobId, AppError> {
        let payload = SyncPayload {
            publications: publications.iter().map(ToString::to_string).collect(),
        };
        let job = Job::new(self.profile.id, JobKind::MetricsSync, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// The publications a sync reads (YouTube's, and the Instagram and
    /// TikTok posts of connected accounts), never checked first, then the
    /// longest unchecked.
    fn tracked_publications(&self) -> Result<Vec<Publication>, RepositoryError> {
        let publications = self.publications.all_publications(self.profile.id)?;
        let access = self.insights_access(&publications);
        let mut tracked: Vec<Publication> = publications
            .into_iter()
            .filter(|publication| is_synced(publication, &access))
            .collect();
        tracked.sort_by_key(|publication| publication.checked_at);
        Ok(tracked)
    }

    /// Starts a sync of every YouTube publication's public statistics, of
    /// where scheduled uploads stand, and of the Instagram and TikTok posts
    /// of connected accounts. Without a key it reads only the scheduled
    /// uploads and the posts read through their account.
    pub fn sync_metrics(&self) -> Result<JobId, MetricsError> {
        if self
            .latest_sync_job()
            .is_some_and(|job| job.state().is_active())
        {
            return Err(MetricsError::AlreadySyncing);
        }
        let mut tracked = self.tracked_publications()?;
        if tracked.is_empty() {
            return Err(MetricsError::NothingToSync);
        }
        if !self.youtube_key_saved() {
            tracked.retain(|p| p.is_scheduled() || p.reads_insights());
            if tracked.is_empty() {
                return Err(MetricsError::MissingKey);
            }
        }
        Ok(self.queue_sync(tracked.iter().map(|p| p.id).collect())?)
    }

    /// Starts a sync when the app opens, if the profile's setting says
    /// it is due and it can run (a key is saved, none is running). Never
    /// fails: a start that cannot sync just does not.
    pub fn sync_metrics_on_start(&self) -> Option<JobId> {
        let tracked = self.tracked_publications().ok()?;
        if !self
            .profile
            .metrics_sync
            .is_due(&tracked, SystemTime::now())
        {
            return None;
        }
        self.sync_metrics().ok()
    }

    /// When a start syncs metrics by itself.
    pub fn metrics_sync_setting(&self) -> MetricsSyncOnStart {
        self.profile.metrics_sync
    }

    /// Changes when a start syncs metrics and remembers it. On failure
    /// the current setting stays.
    pub fn set_metrics_sync_on_start(
        &mut self,
        setting: MetricsSyncOnStart,
    ) -> Result<(), AppError> {
        if setting == self.profile.metrics_sync {
            return Ok(());
        }
        let updated = UserProfile {
            metrics_sync: setting,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        Ok(())
    }

    /// The channel's posts and how they perform: the latest numbers of
    /// each, their totals and the totals over time.
    pub fn channel_metrics(&self, channel: ChannelId) -> Result<ChannelMetricsView, MetricsError> {
        let channel = self
            .channels
            .get(channel)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(MetricsError::ChannelNotFound)?;
        let projects = self.themes.projects(channel.id)?;
        let publications = self.publications.channel_publications(channel.id)?;
        let snapshots = self.publications.channel_snapshots(channel.id)?;
        let mut curves = self.publications.channel_retention(channel.id)?;
        let status = self.metrics_status(&publications);
        let access = self.insights_access(&publications);
        let owner_access = self.owner_access(channel.id)?;
        let mut histories: HashMap<PublicationId, Vec<MetricsSnapshot>> = HashMap::new();
        for snapshot in &snapshots {
            histories
                .entry(snapshot.publication)
                .or_default()
                .push(*snapshot);
        }
        // An upload is a post once the network processed the video.
        let posts = publications
            .into_iter()
            .filter(Publication::is_posted)
            .map(|publication| {
                let history = histories.remove(&publication.id).unwrap_or_default();
                let retention = curves
                    .iter()
                    .position(|curve| curve.publication == publication.id)
                    .map(|ix| curves.swap_remove(ix));
                ChannelPost {
                    project: publication.project,
                    project_title: projects
                        .iter()
                        .find(|project| project.id == publication.project)
                        .map(|project| project.title.clone())
                        .unwrap_or_default(),
                    post: PublishedPost {
                        access: access.get(&publication.account).copied(),
                        publication,
                        history,
                        retention,
                    },
                }
            })
            .collect();
        Ok(ChannelMetricsView {
            channel: channel.id,
            posts,
            totals: MetricsTotals::latest(&snapshots),
            history: channel_history(&snapshots),
            status,
            owner_access,
        })
    }

    /// Whether the channel's YouTube posts get the owner's numbers.
    fn owner_access(&self, channel: ChannelId) -> Result<OwnerAccess, MetricsError> {
        let Some(account) = self
            .network_accounts
            .list(channel)?
            .into_iter()
            .find(|account| account.network == Network::YouTube)
        else {
            return Ok(OwnerAccess::NoAccount);
        };
        Ok(self.connection_access(&account))
    }
}

/// Whether a sync reads `publication`: YouTube's own rules, or an Instagram
/// or TikTok post whose account `access` says is connected.
fn is_synced(publication: &Publication, access: &HashMap<NetworkAccountId, OwnerAccess>) -> bool {
    publication.is_tracked()
        || (publication.reads_insights()
            && access.get(&publication.account) == Some(&OwnerAccess::Connected))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use bardo_domain::{JobState, ProviderFailure, ProviderFailureKind};

    use super::*;
    use crate::export::tests::{Setup, generated, rendered, target};
    use crate::scenes::tests::{done, wait_done};

    const YOUTUBE_KEY: &str = "AIzaSyTestKey0001abcdefghij";
    const SHORT: &str = "https://youtube.com/shorts/dQw4w9WgXcQ?si=shared";
    const TIKTOK: &str = "https://www.tiktok.com/@archives/video/7301234567890123456";

    /// A project exported for YouTube and TikTok.
    fn exported() -> Setup {
        let s = rendered();
        let view = generated(&s);
        done(
            &s.app,
            s.app
                .start_export(&view, &[Network::YouTube, Network::TikTok])
                .unwrap(),
        );
        s
    }

    fn with_key(s: &mut Setup) {
        s.app
            .save_provider_key(Provider::YouTubeData, YOUTUBE_KEY)
            .unwrap();
    }

    fn sync_jobs(app: &Bardo) -> Vec<Job> {
        app.jobs()
            .into_iter()
            .filter(|job| job.kind() == JobKind::MetricsSync)
            .collect()
    }

    fn posted(s: &Setup, network: Network) -> Option<PublishedPost> {
        let view = s.app.export_view(s.project.id).unwrap();
        target(&view, network).posted.clone()
    }

    #[test]
    fn marking_an_export_as_posted_keeps_the_networks_address_and_id() {
        let s = exported();
        s.h.stats.set("dQw4w9WgXcQ", 1_200, Some(80));
        let before = s.app.export_view(s.project.id).unwrap();
        assert!(target(&before, Network::YouTube).can_mark_posted());
        assert_eq!(target(&before, Network::YouTube).posted, None);

        let publication = s
            .app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        assert_eq!(publication.post_id().unwrap(), "dQw4w9WgXcQ");
        assert_eq!(
            publication.link.as_ref().unwrap().url(),
            "https://www.youtube.com/shorts/dQw4w9WgXcQ"
        );
        let view = s.app.export_view(s.project.id).unwrap();
        let youtube = target(&view, Network::YouTube);
        assert_eq!(youtube.posted.as_ref().unwrap().publication, publication);
        assert_eq!(
            publication.render,
            youtube.last.as_ref().unwrap().render,
            "the render its export copied"
        );
        // No key: linked, nothing synced.
        assert!(sync_jobs(&s.app).is_empty());
        assert!(!view.metrics.key_saved);
        assert_eq!(view.metrics.tracked, 1);
    }

    #[test]
    fn a_bad_link_is_refused_with_why() {
        let s = exported();
        let refuse = |network, link| {
            s.app
                .mark_posted(s.project.id, network, link, false)
                .unwrap_err()
        };
        assert!(matches!(
            refuse(Network::YouTube, TIKTOK),
            PublicationError::Link(PostLinkError::OtherSite(Some(Network::TikTok)))
        ));
        assert!(matches!(
            refuse(Network::TikTok, "https://vm.tiktok.com/ZMabc/"),
            PublicationError::Link(PostLinkError::ShortLink)
        ));
        assert!(matches!(
            refuse(Network::YouTube, ""),
            PublicationError::Link(PostLinkError::Empty)
        ));
        assert_eq!(posted(&s, Network::YouTube), None);
        assert_eq!(
            s.app.post_link_problem(
                PostLinkError::OtherSite(Some(Network::TikTok)),
                Network::YouTube
            ),
            "That's a TikTok link. Paste the YouTube post's link."
        );
    }

    #[test]
    fn only_an_exported_network_of_the_channel_can_be_marked() {
        let s = rendered();
        generated(&s);
        assert!(matches!(
            s.app
                .mark_posted(s.project.id, Network::YouTube, SHORT, false),
            Err(PublicationError::NotExported)
        ));
        assert!(matches!(
            s.app.mark_posted(
                s.project.id,
                Network::X,
                "https://x.com/a/status/1840000000000000001",
                false
            ),
            Err(PublicationError::NoAccount)
        ));
        assert!(matches!(
            s.app
                .mark_posted(VideoProjectId::new(), Network::YouTube, SHORT, false),
            Err(PublicationError::ProjectNotFound)
        ));
    }

    #[test]
    fn linking_a_youtube_post_syncs_it_when_the_key_is_saved() {
        let mut s = exported();
        with_key(&mut s);
        s.h.stats.set("dQw4w9WgXcQ", 1_200, Some(80));
        s.app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        let job = sync_jobs(&s.app).pop().expect("a sync was queued");
        done(&s.app, job.id());

        let post = posted(&s, Network::YouTube).unwrap();
        let latest = post.latest().unwrap();
        assert_eq!(latest.views, 1_200);
        assert_eq!(latest.likes, Some(80));
        assert_eq!(latest.comments, Some(12));
        assert!(post.publication.checked_at.is_some());
        assert_eq!(
            post.publication.posted_at,
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000),
            "YouTube's publish time"
        );
        assert_eq!(s.h.stats.calls(), [vec!["dQw4w9WgXcQ".to_owned()]]);

        // A TikTok post keeps its link and syncs nothing.
        s.app
            .mark_posted(s.project.id, Network::TikTok, TIKTOK, false)
            .unwrap();
        assert_eq!(sync_jobs(&s.app).len(), 1);
        assert!(posted(&s, Network::TikTok).unwrap().history.is_empty());
    }

    #[test]
    fn each_sync_keeps_a_snapshot_and_the_history_grows() {
        let mut s = exported();
        s.app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        s.app
            .mark_posted(s.project.id, Network::TikTok, TIKTOK, false)
            .unwrap();
        assert!(matches!(
            s.app.sync_metrics(),
            Err(MetricsError::MissingKey)
        ));
        with_key(&mut s);
        for views in [100, 250, 400] {
            s.h.stats.set("dQw4w9WgXcQ", views, None);
            done(&s.app, s.app.sync_metrics().unwrap());
            // Snapshots are keyed by the millisecond they were taken.
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        let post = posted(&s, Network::YouTube).unwrap();
        let views: Vec<u64> = post.history.iter().map(|snap| snap.views).collect();
        assert_eq!(views, [100, 250, 400]);
        assert_eq!(post.views_change(), Some(150));
        assert_eq!(post.latest().unwrap().likes, None, "hidden likes");
        // Only the YouTube post is asked for, once per sync.
        assert!(
            s.h.stats
                .calls()
                .iter()
                .all(|ids| ids == &["dQw4w9WgXcQ".to_owned()])
        );
        let status = s.app.metrics_sync_status().unwrap();
        assert_eq!(status.tracked, 1);
        assert!(status.last_checked.is_some());
        assert_eq!(
            status.job.unwrap().state(),
            JobState::Done,
            "the latest sync"
        );
    }

    #[test]
    fn a_post_no_longer_on_youtube_is_flagged_and_keeps_its_numbers() {
        let mut s = exported();
        with_key(&mut s);
        s.h.stats.set("dQw4w9WgXcQ", 900, Some(9));
        s.app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        done(&s.app, sync_jobs(&s.app).pop().unwrap().id());

        s.h.stats.found.lock().unwrap().clear();
        done(&s.app, s.app.sync_metrics().unwrap());
        let post = posted(&s, Network::YouTube).unwrap();
        assert!(post.publication.missing_since.is_some());
        assert_eq!(post.latest().unwrap().views, 900);
        assert_eq!(post.history.len(), 1);
    }

    #[test]
    fn relinking_the_same_post_keeps_its_metrics_and_another_post_replaces_them() {
        let mut s = exported();
        with_key(&mut s);
        s.h.stats.set("dQw4w9WgXcQ", 500, None);
        let first = s
            .app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        done(&s.app, sync_jobs(&s.app).pop().unwrap().id());

        let again = s
            .app
            .mark_posted(
                s.project.id,
                Network::YouTube,
                "https://youtu.be/dQw4w9WgXcQ",
                false,
            )
            .unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(
            again.link.as_ref().unwrap().url(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        assert_eq!(posted(&s, Network::YouTube).unwrap().history.len(), 1);

        let other = s
            .app
            .mark_posted(
                s.project.id,
                Network::YouTube,
                "https://www.youtube.com/shorts/aaaaaaaaaaa",
                false,
            )
            .unwrap();
        assert_ne!(other.id, first.id);
        let post = posted(&s, Network::YouTube).unwrap();
        assert_eq!(post.publication.id, other.id);
        // Its own sync was queued; the old post's numbers are gone.
        let job = sync_jobs(&s.app).pop().unwrap();
        wait_done(&s.app, job.id());
        assert!(
            posted(&s, Network::YouTube)
                .unwrap()
                .history
                .iter()
                .all(|snap| snap.publication == other.id)
        );
    }

    #[test]
    fn a_post_links_to_one_project() {
        let s = exported();
        s.app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        // A second project of the same channel, exported too.
        let (second, _) = s.h.drawn_project(&s.app);
        crate::export::tests::add_account(
            &s.app,
            &second,
            Network::YouTube,
            bardo_domain::NetworkAccountDraft::default(),
        );
        crate::render::tests::render_all(&s.app, second.id);
        s.h.answer(crate::export::tests::youtube_and_tiktok());
        done(
            &s.app,
            s.app
                .generate_metadata(second.id, crate::BudgetConsent::Ask)
                .unwrap(),
        );
        let view = s.app.export_view(second.id).unwrap();
        done(
            &s.app,
            s.app.start_export(&view, &[Network::YouTube]).unwrap(),
        );
        assert!(matches!(
            s.app.mark_posted(second.id, Network::YouTube, SHORT, false),
            Err(PublicationError::AlreadyLinked)
        ));
    }

    #[test]
    fn unlinking_removes_the_publication_and_its_metrics() {
        let mut s = exported();
        with_key(&mut s);
        s.h.stats.set("dQw4w9WgXcQ", 10, None);
        let publication = s
            .app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        done(&s.app, sync_jobs(&s.app).pop().unwrap().id());
        s.app.remove_publication(publication.id).unwrap();
        assert_eq!(posted(&s, Network::YouTube), None);
        assert!(matches!(
            s.app.remove_publication(publication.id),
            Err(PublicationError::NotFound)
        ));
        assert!(matches!(
            s.app.sync_metrics(),
            Err(MetricsError::NothingToSync)
        ));
    }

    #[test]
    fn a_running_sync_refuses_another_and_can_be_cancelled() {
        let mut s = exported();
        with_key(&mut s);
        s.app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        let first = sync_jobs(&s.app).pop().unwrap();
        wait_done(&s.app, first.id());
        s.h.stats.hold.store(true, Ordering::SeqCst);
        let id = s.app.sync_metrics().unwrap();
        assert!(matches!(
            s.app.sync_metrics(),
            Err(MetricsError::AlreadySyncing)
        ));
        assert!(s.app.metrics_sync_status().unwrap().is_syncing());
        s.app.cancel_job(id).unwrap();
        s.h.stats.hold.store(false, Ordering::SeqCst);
        assert_eq!(wait_done(&s.app, id).state(), JobState::Cancelled);
    }

    #[test]
    fn a_quota_failure_stops_the_sync_with_its_kind() {
        let mut s = exported();
        with_key(&mut s);
        s.app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        wait_done(&s.app, sync_jobs(&s.app).pop().unwrap().id());
        *s.h.stats.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::LimitReached,
            "quotaExceeded",
        ));
        let job = wait_done(&s.app, s.app.sync_metrics().unwrap());
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::LimitReached);
    }

    #[test]
    fn a_start_syncs_only_when_due_by_the_profiles_setting() {
        let mut s = exported();
        assert_eq!(s.app.sync_metrics_on_start(), None, "nothing linked");
        s.app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        assert_eq!(s.app.sync_metrics_on_start(), None, "no key");
        with_key(&mut s);
        s.app
            .set_metrics_sync_on_start(MetricsSyncOnStart::Off)
            .unwrap();
        assert_eq!(s.app.sync_metrics_on_start(), None, "turned off");

        s.app
            .set_metrics_sync_on_start(MetricsSyncOnStart::Daily)
            .unwrap();
        let id = s.app.sync_metrics_on_start().expect("never checked");
        done(&s.app, id);
        assert_eq!(s.app.sync_metrics_on_start(), None, "checked just now");
        assert_eq!(s.app.metrics_sync_setting(), MetricsSyncOnStart::Daily);
    }

    #[test]
    fn the_channel_view_lists_its_posts_with_totals_and_history() {
        let mut s = exported();
        with_key(&mut s);
        s.h.stats.set("dQw4w9WgXcQ", 300, Some(30));
        s.app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        done(&s.app, sync_jobs(&s.app).pop().unwrap().id());
        std::thread::sleep(std::time::Duration::from_millis(3));
        s.h.stats.set("dQw4w9WgXcQ", 700, Some(50));
        done(&s.app, s.app.sync_metrics().unwrap());
        s.app
            .mark_posted(s.project.id, Network::TikTok, TIKTOK, false)
            .unwrap();

        let view = s.app.channel_metrics(s.project.channel).unwrap();
        assert_eq!(view.posts.len(), 2);
        assert!(
            view.posts
                .iter()
                .all(|post| post.project_title == s.project.title)
        );
        assert_eq!(view.totals.views, 700);
        assert_eq!(view.totals.likes, Some(50));
        assert_eq!(view.totals.posts, 1);
        let history: Vec<u64> = view.history.iter().map(|p| p.totals.views).collect();
        assert_eq!(history, [300, 700]);
        assert_eq!(view.status.tracked, 1);
        assert!(view.status.can_sync());
        assert!(matches!(
            s.app.channel_metrics(ChannelId::new()),
            Err(MetricsError::ChannelNotFound)
        ));
    }

    /// A project exported for YouTube and TikTok, its YouTube post linked
    /// and the key saved, with the channel's YouTube account connected.
    fn connected() -> Setup {
        let mut s = exported();
        with_key(&mut s);
        crate::uploads::tests::connect(&s);
        s.h.stats.set("dQw4w9WgXcQ", 50_000, Some(900));
        s
    }

    fn link_and_sync(s: &Setup) {
        s.app
            .mark_posted(s.project.id, Network::YouTube, SHORT, false)
            .unwrap();
        done(&s.app, sync_jobs(&s.app).pop().unwrap().id());
    }

    #[test]
    fn a_connected_channel_reads_the_owners_numbers_with_its_own_token() {
        let s = connected();
        s.h.analytics.set("dQw4w9WgXcQ", 48_000);
        link_and_sync(&s);

        let post = posted(&s, Network::YouTube).unwrap();
        let latest = post.latest().unwrap();
        assert_eq!(latest.views, 50_000, "public views as before");
        let owner = latest.owner.expect("owner numbers on the snapshot");
        assert_eq!(owner.views, 48_000);
        assert_eq!(owner.engaged_views, 33_600);
        assert_eq!(post.engaged_views(), Some(33_600));
        let money = owner.money().expect("monetized");
        assert_eq!(money.revenue, bardo_domain::Money::from_micros(96_000_000));
        assert_eq!(owner.rpm(), Some(bardo_domain::Money::from_cents(200)));
        assert_eq!(post.latest_owner(), Some(latest));
        let retention = post.retention.as_ref().expect("a retention curve");
        assert_eq!(retention.curve.points().len(), 100);
        assert_eq!(retention.read_at, latest.taken_at);

        assert_eq!(
            s.h.analytics.calls(),
            [("dQw4w9WgXcQ".to_owned(), true)],
            "one report with money"
        );
        assert!(
            s.h.analytics
                .tokens
                .lock()
                .unwrap()
                .iter()
                .all(|token| token == crate::uploads::tests::ACCESS)
        );

        let view = s.app.channel_metrics(s.project.channel).unwrap();
        assert_eq!(view.owner_access, OwnerAccess::Connected);
        let totals = view.totals.owner.unwrap();
        assert_eq!(totals.engaged_views, 33_600);
        assert_eq!(
            totals.revenue,
            Some(bardo_domain::Money::from_micros(96_000_000))
        );
        assert!(view.posts[0].post.retention.is_some());
    }

    #[test]
    fn a_channel_outside_the_partner_program_reads_not_monetized() {
        let s = connected();
        s.h.analytics.set("dQw4w9WgXcQ", 1_000);
        s.h.analytics.not_monetized.store(true, Ordering::SeqCst);
        link_and_sync(&s);

        let owner = posted(&s, Network::YouTube)
            .unwrap()
            .latest()
            .unwrap()
            .owner
            .unwrap();
        assert_eq!(owner.earnings, bardo_domain::Earnings::NotMonetized);
        assert_eq!(owner.rpm(), None);
        assert_eq!(owner.views, 1_000, "the rest is read");
        assert_eq!(
            s.h.analytics.calls(),
            [
                ("dQw4w9WgXcQ".to_owned(), true),
                ("dQw4w9WgXcQ".to_owned(), false)
            ]
        );
        let job = s.app.latest_sync_job().unwrap();
        assert_eq!(job.state(), JobState::Done);
    }

    #[test]
    fn an_unconnected_channel_keeps_exactly_its_public_metrics() {
        let mut s = exported();
        with_key(&mut s);
        s.h.stats.set("dQw4w9WgXcQ", 1_200, Some(80));
        s.h.analytics.set("dQw4w9WgXcQ", 9_999);
        link_and_sync(&s);

        assert!(s.h.analytics.calls().is_empty(), "never asked");
        let post = posted(&s, Network::YouTube).unwrap();
        let latest = *post.latest().unwrap();
        assert_eq!(
            latest,
            MetricsSnapshot {
                publication: post.publication.id,
                taken_at: latest.taken_at,
                views: 1_200,
                likes: Some(80),
                comments: Some(12),
                owner: None,
                insights: bardo_domain::Insights::default(),
            }
        );
        assert_eq!(post.engaged_views(), None, "the public views lead");
        assert_eq!(post.retention, None);
        let view = s.app.channel_metrics(s.project.channel).unwrap();
        assert_eq!(view.owner_access, OwnerAccess::NotConnected);
        assert_eq!(view.totals.owner, None);
    }

    #[test]
    fn a_post_without_data_yet_keeps_its_public_numbers_only() {
        let s = connected();
        // No report for the video: the network's data have not arrived.
        link_and_sync(&s);
        let post = posted(&s, Network::YouTube).unwrap();
        assert_eq!(post.latest().unwrap().owner, None);
        assert_eq!(post.latest().unwrap().views, 50_000);
        assert_eq!(post.retention, None);
        assert!(s.h.analytics.curves.lock().unwrap().is_empty());
    }

    #[test]
    fn an_owner_read_that_fails_never_fails_the_sync() {
        let s = connected();
        s.h.analytics.set("dQw4w9WgXcQ", 1_000);
        *s.h.analytics.failure.lock().unwrap() = Some(bardo_domain::AnalyticsError::new(
            bardo_domain::AnalyticsErrorKind::LimitReached,
            "quotaExceeded",
        ));
        link_and_sync(&s);
        assert_eq!(s.app.latest_sync_job().unwrap().state(), JobState::Done);
        let latest = *posted(&s, Network::YouTube).unwrap().latest().unwrap();
        assert_eq!((latest.views, latest.owner), (50_000, None));
    }

    #[test]
    fn a_channel_that_needs_to_reconnect_is_not_read() {
        let s = connected();
        s.h.analytics.set("dQw4w9WgXcQ", 1_000);
        let account = s
            .app
            .network_accounts
            .list(s.project.channel)
            .unwrap()
            .into_iter()
            .find(|account| account.network == Network::YouTube)
            .unwrap();
        let mut state = bardo_domain::NetworkConnectionRepository::get(&*s.h.db, account.id)
            .unwrap()
            .unwrap();
        state.status = bardo_domain::ConnectionStatus::ReconnectNeeded;
        bardo_domain::NetworkConnectionRepository::save(&*s.h.db, &state).unwrap();

        link_and_sync(&s);
        assert!(s.h.analytics.calls().is_empty());
        assert_eq!(
            posted(&s, Network::YouTube)
                .unwrap()
                .latest()
                .unwrap()
                .owner,
            None
        );
        assert_eq!(
            s.app
                .channel_metrics(s.project.channel)
                .unwrap()
                .owner_access,
            OwnerAccess::ReconnectNeeded
        );
    }

    const REEL: &str = "https://www.instagram.com/reel/C9xYz12AbCd/?igsh=shared";
    const MEDIA: &str = "17912345678901234";
    const TIKTOK_ID: &str = "7301234567890123456";
    const PAGE_TOKEN: &str = "EAAG-insights-page-token";
    const TIKTOK_TOKEN: &str = "act.insights-access-token";

    /// A project exported for YouTube, TikTok and Instagram Reels.
    fn exported_everywhere() -> Setup {
        let s = crate::export::tests::rendered_with(&[Network::InstagramReels]);
        crate::uploads::reel_tests::generate(&s);
        let view = s.app.export_view(s.project.id).unwrap();
        done(
            &s.app,
            s.app
                .start_export(
                    &view,
                    &[Network::YouTube, Network::TikTok, Network::InstagramReels],
                )
                .unwrap(),
        );
        s
    }

    fn account_on(s: &Setup, network: Network) -> NetworkAccount {
        s.app
            .network_accounts
            .list(s.project.channel)
            .unwrap()
            .into_iter()
            .find(|account| account.network == network)
            .unwrap()
    }

    /// Connects the channel's `network` account as `identity`.
    fn connect_on(s: &Setup, network: Network, identity: &str, token: &str) {
        let account = account_on(s, network);
        let now = SystemTime::now();
        let tokens = bardo_domain::TokenSet::granted(
            &bardo_domain::TokenGrant {
                access_token: SecretText::new(token),
                refresh_token: Some(SecretText::new("refresh")),
                expires_in: std::time::Duration::from_secs(30 * 24 * 60 * 60),
                scopes: Vec::new(),
            },
            now,
        );
        bardo_domain::ConnectionSecrets::set_tokens(
            &*s.h.connection_secrets,
            s.app.profile().id,
            account.id,
            &tokens,
        )
        .unwrap();
        bardo_domain::NetworkConnectionRepository::save(
            &*s.h.db,
            &bardo_domain::NetworkConnection {
                account: account.id,
                owner: s.app.profile().id,
                status: bardo_domain::ConnectionStatus::Connected,
                identity: bardo_domain::ConnectedIdentity {
                    id: identity.into(),
                    name: "archives".into(),
                },
                scopes: Vec::new(),
                expires_at: tokens.expires_at(),
                connected_at: now,
                refreshed_at: None,
            },
        )
        .unwrap();
    }

    /// Both accounts connected, the Reel listed and both posts with
    /// numbers.
    fn connected_everywhere() -> Setup {
        let s = exported_everywhere();
        connect_on(&s, Network::InstagramReels, "17841400000000001", PAGE_TOKEN);
        connect_on(&s, Network::TikTok, "open-id-archives", TIKTOK_TOKEN);
        s.h.instagram_insights
            .list(&[("17900000000000009", "OtherReel01"), (MEDIA, "C9xYz12AbCd")]);
        s
    }

    fn mark_and_sync(s: &Setup, network: Network, link: &str) {
        s.app
            .mark_posted(s.project.id, network, link, false)
            .unwrap();
        let job = sync_jobs(&s.app).pop().expect("a sync was queued");
        done(&s.app, job.id());
    }

    fn sync_now(s: &Setup) {
        let job = s.app.sync_metrics().unwrap();
        done(&s.app, job);
    }

    #[test]
    fn a_linked_reel_is_found_once_in_the_accounts_media_and_read_with_its_insights() {
        let s = connected_everywhere();
        s.h.instagram_insights.set_numbers(
            MEDIA,
            bardo_domain::PostNumbers {
                views: Some(3_400),
                likes: Some(210),
                comments: Some(0),
                insights: bardo_domain::Insights {
                    shares: Some(31),
                    saves: Some(12),
                    reach: Some(2_900),
                    interactions: Some(253),
                    average_watch: Some(std::time::Duration::from_millis(7_300)),
                    watch_time: Some(std::time::Duration::from_secs(24_820)),
                },
                posted_at: None,
            },
        );
        // No YouTube key: the account's own connection reads it.
        mark_and_sync(&s, Network::InstagramReels, REEL);

        let post = posted(&s, Network::InstagramReels).unwrap();
        assert_eq!(post.publication.insights_id.as_deref(), Some(MEDIA));
        assert!(post.publication.checked_at.is_some());
        assert_eq!(post.access, Some(OwnerAccess::Connected));
        let latest = *post.latest().unwrap();
        assert_eq!(latest.views, 3_400);
        assert_eq!(latest.likes, Some(210));
        assert_eq!(latest.comments, Some(0), "a zero stays a zero");
        assert_eq!(latest.owner, None, "no YouTube owner numbers");
        assert_eq!(latest.insights.reach, Some(2_900));
        assert_eq!(latest.insights.shares, Some(31));
        assert_eq!(
            latest.insights.average_watch,
            Some(std::time::Duration::from_millis(7_300))
        );
        assert_eq!(s.h.instagram_insights.pages(), [None]);
        assert_eq!(s.h.instagram_insights.reads(), [vec![MEDIA.to_owned()]]);

        // The media id is kept: the next sync reads it without listing.
        sync_now(&s);
        assert_eq!(s.h.instagram_insights.pages().len(), 1);
        assert_eq!(s.h.instagram_insights.reads().len(), 2);
        assert_eq!(
            posted(&s, Network::InstagramReels).unwrap().history.len(),
            2
        );
        assert!(
            s.h.instagram_insights
                .tokens
                .lock()
                .unwrap()
                .iter()
                .all(|token| token == PAGE_TOKEN)
        );
        let view = s.app.channel_metrics(s.project.channel).unwrap();
        assert_eq!(view.totals.views, 3_400);
        assert_eq!(view.totals.shares, Some(31));
    }

    #[test]
    fn a_reel_without_data_yet_has_no_snapshot_and_is_not_missing() {
        let s = connected_everywhere();
        // Instagram answers an empty set while the data have not arrived.
        s.h.instagram_insights
            .set_numbers(MEDIA, bardo_domain::PostNumbers::default());
        mark_and_sync(&s, Network::InstagramReels, REEL);

        let post = posted(&s, Network::InstagramReels).unwrap();
        assert!(post.history.is_empty(), "empty is not zero");
        assert!(post.publication.checked_at.is_some());
        assert_eq!(post.publication.missing_since, None);
        assert_eq!(post.publication.insights_id.as_deref(), Some(MEDIA));
    }

    #[test]
    fn a_reel_the_account_does_not_have_is_flagged_without_reading_insights() {
        let s = exported_everywhere();
        connect_on(&s, Network::InstagramReels, "17841400000000001", PAGE_TOKEN);
        s.h.instagram_insights
            .list(&[("17900000000000009", "OtherReel01")]);
        mark_and_sync(&s, Network::InstagramReels, REEL);

        let post = posted(&s, Network::InstagramReels).unwrap();
        assert!(post.publication.missing_since.is_some());
        assert_eq!(post.publication.insights_id, None);
        assert!(s.h.instagram_insights.reads().is_empty());
    }

    #[test]
    fn a_tiktok_post_is_read_by_its_video_id_and_flagged_once_it_is_gone() {
        let s = connected_everywhere();
        s.h.tiktok_insights.set(TIKTOK_ID, 10_000);
        mark_and_sync(&s, Network::TikTok, TIKTOK);

        let post = posted(&s, Network::TikTok).unwrap();
        let latest = *post.latest().unwrap();
        assert_eq!(
            (latest.views, latest.likes, latest.comments),
            (10_000, Some(1_000), Some(100))
        );
        assert_eq!(latest.insights.shares, Some(200));
        assert_eq!(latest.insights.average_watch, None, "TikTok has none");
        assert_eq!(s.h.tiktok_insights.reads(), [vec![TIKTOK_ID.to_owned()]]);
        assert!(
            s.h.tiktok_insights.pages().is_empty(),
            "the link has the id"
        );
        assert_eq!(post.publication.missing_since, None);

        // Deleted or made private: TikTok leaves it out.
        s.h.tiktok_insights.posts.lock().unwrap().clear();
        sync_now(&s);
        let post = posted(&s, Network::TikTok).unwrap();
        assert!(post.publication.missing_since.is_some());
        assert_eq!(post.history.len(), 1, "it keeps its numbers");
    }

    #[test]
    fn unconnected_instagram_and_tiktok_accounts_keep_their_link_only() {
        let mut s = exported_everywhere();
        with_key(&mut s);
        s.h.instagram_insights.list(&[(MEDIA, "C9xYz12AbCd")]);
        s.h.instagram_insights.set(MEDIA, 3_400);
        s.h.tiktok_insights.set(TIKTOK_ID, 10_000);
        s.app
            .mark_posted(s.project.id, Network::InstagramReels, REEL, false)
            .unwrap();
        s.app
            .mark_posted(s.project.id, Network::TikTok, TIKTOK, false)
            .unwrap();
        assert!(sync_jobs(&s.app).is_empty(), "nothing to read");
        assert!(matches!(
            s.app.sync_metrics(),
            Err(MetricsError::NothingToSync)
        ));

        // A YouTube post syncs; the others are never asked.
        s.h.stats.set("dQw4w9WgXcQ", 1_200, Some(80));
        mark_and_sync(&s, Network::YouTube, SHORT);
        sync_now(&s);
        assert!(s.h.instagram_insights.reads().is_empty());
        assert!(s.h.instagram_insights.pages().is_empty());
        assert!(s.h.tiktok_insights.reads().is_empty());
        for network in [Network::InstagramReels, Network::TikTok] {
            let post = posted(&s, network).unwrap();
            assert!(post.history.is_empty());
            assert_eq!(post.publication.checked_at, None);
            assert_eq!(post.access, Some(OwnerAccess::NotConnected));
            assert!(!post.is_synced());
        }
        let status = s.app.metrics_sync_status().unwrap();
        assert_eq!((status.tracked, status.with_key), (1, 1));
        assert!(status.needs_key);
    }

    #[test]
    fn an_account_that_needs_to_reconnect_is_not_read() {
        let s = connected_everywhere();
        s.h.tiktok_insights.set(TIKTOK_ID, 10_000);
        let account = account_on(&s, Network::TikTok);
        let mut state = bardo_domain::NetworkConnectionRepository::get(&*s.h.db, account.id)
            .unwrap()
            .unwrap();
        state.status = bardo_domain::ConnectionStatus::ReconnectNeeded;
        bardo_domain::NetworkConnectionRepository::save(&*s.h.db, &state).unwrap();

        s.app
            .mark_posted(s.project.id, Network::TikTok, TIKTOK, false)
            .unwrap();
        assert!(sync_jobs(&s.app).is_empty());
        let post = posted(&s, Network::TikTok).unwrap();
        assert_eq!(post.access, Some(OwnerAccess::ReconnectNeeded));
        assert!(s.h.tiktok_insights.reads().is_empty());
    }

    #[test]
    fn a_read_that_stops_the_account_never_fails_the_sync() {
        let s = connected_everywhere();
        s.h.tiktok_insights.set(TIKTOK_ID, 10_000);
        s.h.instagram_insights.set(MEDIA, 3_400);
        *s.h.tiktok_insights.failure.lock().unwrap() = Some(bardo_domain::AnalyticsError::new(
            bardo_domain::AnalyticsErrorKind::LimitReached,
            "rate_limit_exceeded",
        ));
        s.app
            .mark_posted(s.project.id, Network::TikTok, TIKTOK, false)
            .unwrap();
        s.app
            .mark_posted(s.project.id, Network::InstagramReels, REEL, false)
            .unwrap();
        // The first link queued a sync; wait for it, then sync both.
        wait_done(&s.app, sync_jobs(&s.app).pop().unwrap().id());
        sync_now(&s);

        assert_eq!(s.app.latest_sync_job().unwrap().state(), JobState::Done);
        let tiktok = posted(&s, Network::TikTok).unwrap();
        assert!(tiktok.history.is_empty());
        assert_eq!(tiktok.publication.missing_since, None, "unread, not gone");
        assert_eq!(
            posted(&s, Network::InstagramReels)
                .unwrap()
                .latest()
                .unwrap()
                .views,
            3_400,
            "the other account is read"
        );
    }

    #[test]
    fn the_status_counts_connected_posts_and_needs_no_key_for_them() {
        let s = connected_everywhere();
        s.h.tiktok_insights.set(TIKTOK_ID, 10_000);
        mark_and_sync(&s, Network::TikTok, TIKTOK);
        let status = s.app.metrics_sync_status().unwrap();
        assert_eq!((status.tracked, status.with_key), (1, 0));
        assert!(!status.needs_key);
        assert!(!status.key_saved);
        assert!(status.can_sync());
        assert!(status.last_checked.is_some());
    }
}
