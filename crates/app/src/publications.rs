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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, ChannelId, ChannelPoint, Job, JobFailure, JobFailureKind, JobId, JobKind,
    MetricsSnapshot, MetricsSyncOnStart, MetricsTotals, Network, PostLink, PostLinkError,
    ProfileId, Progress, Provider, Publication, PublicationId, PublicationKind,
    PublicationRepository, RepositoryError, STATS_BATCH, SecretStore, UserProfile, VideoProjectId,
    VideoStats, channel_history,
};
use serde::{Deserialize, Serialize};

use crate::jobs::{JobContext, JobHandler};
use crate::scenes::{id, parse, to_json, unexpected};
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
}

/// Where metrics syncing stands, for the screens that show metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsStatus {
    /// The latest metrics sync job.
    pub job: Option<Job>,
    /// Whether a YouTube Data API key is saved.
    pub key_saved: bool,
    pub on_start: MetricsSyncOnStart,
    /// YouTube publications a sync reads.
    pub tracked: usize,
    /// The latest check of any of them.
    pub last_checked: Option<SystemTime>,
}

impl MetricsStatus {
    pub fn is_syncing(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }

    /// Whether "Sync now" may start a sync.
    pub fn can_sync(&self) -> bool {
        self.key_saved && self.tracked > 0 && !self.is_syncing()
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
}

impl MetricsSyncHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::YouTubeData)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(
                    JobFailureKind::MissingKey,
                    "no YouTube Data API key is saved",
                )
            })
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
        let key = self.key()?;
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
            for text in *batch {
                let publication: PublicationId = id(text)?;
                if let Some(post) = self
                    .publications
                    .publication(publication)
                    .map_err(unexpected)?
                    .filter(Publication::has_public_metrics)
                {
                    posts.push(post);
                }
            }
            if !posts.is_empty() {
                let ids: Vec<&str> = posts.iter().filter_map(Publication::post_id).collect();
                let found = self.stats.statistics(&key, &ids).map_err(|failure| {
                    JobFailure::new(failure.kind.into(), format!("YouTube: {}", failure.detail))
                })?;
                let mut snapshots = Vec::new();
                for post in &mut posts {
                    let statistics = found
                        .iter()
                        .find(|statistics| Some(statistics.post_id.as_str()) == post.post_id());
                    post.checked(statistics, taken_at);
                    if let Some(statistics) = statistics {
                        snapshots.push(MetricsSnapshot::of(post.id, statistics, taken_at));
                    }
                }
                self.publications
                    .save_sync(&posts, &snapshots)
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
        let tracked: Vec<&Publication> = publications
            .iter()
            .filter(|publication| publication.has_public_metrics())
            .collect();
        MetricsStatus {
            job: self.latest_sync_job(),
            key_saved: self.youtube_key_saved(),
            on_start: self.profile.metrics_sync,
            tracked: tracked.len(),
            last_checked: tracked.iter().filter_map(|p| p.checked_at).max(),
        }
    }

    /// The project's publications with their snapshots, in `Network::ALL`
    /// order.
    pub(crate) fn published_posts(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<PublishedPost>, RepositoryError> {
        self.publications
            .publications(project)?
            .into_iter()
            .map(|publication| {
                Ok(PublishedPost {
                    history: self.publications.snapshots(publication.id)?,
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
    /// upload stops. A YouTube post is synced right away when the key is
    /// saved.
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
        let export = self
            .exports
            .exports(project.id)?
            .into_iter()
            .find(|export| export.network == network)
            .ok_or(PublicationError::NotExported)?;
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
        let current = self
            .publications
            .publications(project.id)?
            .into_iter()
            .find(|publication| publication.network() == network);
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
                render: export.render,
                link: Some(link),
                kind: PublicationKind::Manual,
                posted_at: now,
                linked_at: now,
                checked_at: None,
                missing_since: None,
            },
        };
        self.publications.save_publication(&publication)?;
        if publication.has_public_metrics()
            && publication.checked_at.is_none()
            && self.youtube_key_saved()
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

    /// The YouTube publications a sync reads, never checked first, then
    /// the longest unchecked.
    fn tracked_publications(&self) -> Result<Vec<Publication>, RepositoryError> {
        let mut tracked: Vec<Publication> = self
            .publications
            .all_publications(self.profile.id)?
            .into_iter()
            .filter(Publication::has_public_metrics)
            .collect();
        tracked.sort_by_key(|publication| publication.checked_at);
        Ok(tracked)
    }

    /// Starts a sync of every YouTube publication's public statistics.
    pub fn sync_metrics(&self) -> Result<JobId, MetricsError> {
        if self
            .latest_sync_job()
            .is_some_and(|job| job.state().is_active())
        {
            return Err(MetricsError::AlreadySyncing);
        }
        let tracked = self.tracked_publications()?;
        if tracked.is_empty() {
            return Err(MetricsError::NothingToSync);
        }
        if !self.youtube_key_saved() {
            return Err(MetricsError::MissingKey);
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
        let status = self.metrics_status(&publications);
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
                ChannelPost {
                    project: publication.project,
                    project_title: projects
                        .iter()
                        .find(|project| project.id == publication.project)
                        .map(|project| project.title.clone())
                        .unwrap_or_default(),
                    post: PublishedPost {
                        publication,
                        history,
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
        })
    }
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
}
