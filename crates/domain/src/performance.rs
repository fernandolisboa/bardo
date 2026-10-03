//! Past performance (CONTEXT.md, PRD story 82): how the channel's own
//! published videos did, as the theme ranking reads it.
//!
//! Videos are compared on their first week, so a video synced a month after
//! going live and one synced on its third day weigh the same: each post's
//! snapshots draw a line of views over its age, from zero when it went
//! live, and the first-week figure is read off that line at seven days. A
//! post younger than a week is projected from its pace so far; one younger
//! than two days says too little and waits.
//!
//! The decision engine judges words, not numbers: each video reaches it as
//! where it stands against the channel's usual (its median first week), and
//! the engine relates the idea it scores to the videos it resembles. The
//! numbers stay in code, for the reason the user reads.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::{
    MetricsSnapshot, Niche, Publication, PublicationId, Reason, VideoProject, VideoProjectId,
};

/// The age at which videos are compared.
pub const FIRST_WEEK: Duration = Duration::from_secs(7 * 24 * 3600);
/// The youngest a post may be for its pace to count.
pub const MIN_AGE: Duration = Duration::from_secs(2 * 24 * 3600);

/// A video's views at seven days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstWeek {
    pub views: u64,
    /// Read from a post younger than a week, at its pace so far.
    pub projected: bool,
}

/// One post's views at seven days, from its snapshots (any order): read off
/// the line through zero at `posted_at` and each snapshot, or, when the
/// newest snapshot is younger than a week, projected at its average pace.
/// `None` until a snapshot at least [`MIN_AGE`] old exists.
pub fn first_week(posted_at: SystemTime, snapshots: &[MetricsSnapshot]) -> Option<FirstWeek> {
    let mut points: Vec<(f64, f64)> = snapshots
        .iter()
        .filter_map(|snapshot| {
            let age = snapshot.taken_at.duration_since(posted_at).ok()?;
            (!age.is_zero()).then_some((age.as_secs_f64(), snapshot.views as f64))
        })
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let &(newest_age, newest_views) = points.last()?;
    if newest_age < MIN_AGE.as_secs_f64() {
        return None;
    }
    let week = FIRST_WEEK.as_secs_f64();
    let (views, projected) = match points.iter().position(|&(age, _)| age >= week) {
        Some(after) => {
            let (age_after, views_after) = points[after];
            let (age_before, views_before) = match after {
                0 => (0.0, 0.0),
                n => points[n - 1],
            };
            let share = (week - age_before) / (age_after - age_before);
            (views_before + (views_after - views_before) * share, false)
        }
        None => (newest_views * week / newest_age, true),
    };
    Some(FirstWeek {
        views: views.max(0.0).round() as u64,
        projected,
    })
}

/// One published video project of the channel and its first week, its
/// posts added up (only YouTube posts have public numbers today).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedVideo {
    pub project: VideoProjectId,
    pub title: String,
    pub niche: Niche,
    /// When its first post went live.
    pub posted_at: SystemTime,
    pub first_week: FirstWeek,
}

/// Where a video stands against the channel's usual first week (its
/// median), in the words the decision engine reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Standing {
    FarBelow,
    Below,
    Usual,
    Above,
    FarAbove,
}

impl Standing {
    /// Bands of the ratio to the usual: under half, under 0.8, up to 1.25,
    /// up to double, beyond.
    pub fn of(views: u64, usual: u64) -> Self {
        if usual == 0 {
            return if views == 0 {
                Standing::Usual
            } else {
                Standing::FarAbove
            };
        }
        let ratio = views as f64 / usual as f64;
        match ratio {
            r if r < 0.5 => Standing::FarBelow,
            r if r < 0.8 => Standing::Below,
            r if r <= 1.25 => Standing::Usual,
            r if r <= 2.0 => Standing::Above,
            _ => Standing::FarAbove,
        }
    }

    /// For the decision engine, which reads English best.
    pub fn describe(self) -> &'static str {
        match self {
            Standing::FarBelow => "far below the channel's usual",
            Standing::Below => "below the channel's usual",
            Standing::Usual => "about the channel's usual",
            Standing::Above => "above the channel's usual",
            Standing::FarAbove => "far above the channel's usual",
        }
    }
}

/// What the figure in a past performance reason averages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidenceScope {
    /// The channel's videos in the theme's niche.
    Niche,
    /// Every video of the channel, when none is in the niche yet.
    Channel,
}

impl EvidenceScope {
    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            EvidenceScope::Niche => "niche",
            EvidenceScope::Channel => "channel",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        [EvidenceScope::Niche, EvidenceScope::Channel]
            .into_iter()
            .find(|scope| scope.code() == code)
    }
}

/// The numbers behind a past performance reason, for the user to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerformanceEvidence {
    pub scope: EvidenceScope,
    /// The average first week of the videos in scope.
    pub average_views: u64,
    /// Videos in scope.
    pub videos: u32,
    /// Of those, projected from younger posts.
    pub projected: u32,
    /// Every video of the channel the engine read; how much the reason
    /// weighs grows with it.
    pub basis: u32,
}

/// The fourth reason of a ranking, when the channel has history: the
/// engine's 0–100 score of how the idea would do next to the channel's
/// own videos, with the numbers it saw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerformanceReason {
    pub reason: Reason,
    pub evidence: PerformanceEvidence,
}

/// A channel's published videos with a first week, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PastPerformance {
    videos: Vec<PublishedVideo>,
}

impl PastPerformance {
    /// The first weeks of the channel's video projects: `publications` and
    /// `snapshots` of the channel, matched to `projects`. Publications of
    /// other projects are left out, as are projects without a first week
    /// yet.
    pub fn of_channel(
        projects: &[VideoProject],
        publications: &[Publication],
        snapshots: &[MetricsSnapshot],
    ) -> Self {
        let mut histories: HashMap<PublicationId, Vec<MetricsSnapshot>> = HashMap::new();
        for snapshot in snapshots {
            histories
                .entry(snapshot.publication)
                .or_default()
                .push(*snapshot);
        }
        let mut videos: Vec<PublishedVideo> = projects
            .iter()
            .filter_map(|project| {
                let mut views = 0u64;
                let mut projected = false;
                let mut posted_at: Option<SystemTime> = None;
                for publication in publications.iter().filter(|p| p.project == project.id) {
                    let history = histories
                        .get(&publication.id)
                        .map_or(&[][..], Vec::as_slice);
                    let Some(week) = first_week(publication.posted_at, history) else {
                        continue;
                    };
                    views = views.saturating_add(week.views);
                    projected |= week.projected;
                    posted_at = Some(posted_at.map_or(publication.posted_at, |earliest| {
                        earliest.min(publication.posted_at)
                    }));
                }
                Some(PublishedVideo {
                    project: project.id,
                    title: project.title.clone(),
                    niche: project.niche.clone(),
                    posted_at: posted_at?,
                    first_week: FirstWeek { views, projected },
                })
            })
            .collect();
        videos.sort_by_key(|video| std::cmp::Reverse(video.posted_at));
        Self { videos }
    }

    /// Newest first.
    pub fn videos(&self) -> &[PublishedVideo] {
        &self.videos
    }

    pub fn is_empty(&self) -> bool {
        self.videos.is_empty()
    }

    /// The channel's usual first week: the median, so one viral video does
    /// not make every other one look weak. The lower middle on an even
    /// count.
    pub fn usual(&self) -> Option<u64> {
        let mut views: Vec<u64> = self.videos.iter().map(|v| v.first_week.views).collect();
        views.sort_unstable();
        views.get(views.len().checked_sub(1)? / 2).copied()
    }

    pub fn standing(&self, video: &PublishedVideo) -> Standing {
        Standing::of(video.first_week.views, self.usual().unwrap_or(0))
    }

    /// The numbers a reason for an idea in `niche` shows: the niche's
    /// videos, or every video when the niche has none. `None` without
    /// history.
    pub fn evidence(&self, niche: &Niche) -> Option<PerformanceEvidence> {
        let in_niche: Vec<&PublishedVideo> = self
            .videos
            .iter()
            .filter(|video| video.niche.key() == niche.key())
            .collect();
        let (scope, videos) = if in_niche.is_empty() {
            (EvidenceScope::Channel, self.videos.iter().collect())
        } else {
            (EvidenceScope::Niche, in_niche)
        };
        let count = videos.len() as u64;
        if count == 0 {
            return None;
        }
        let total: u128 = videos.iter().map(|v| u128::from(v.first_week.views)).sum();
        let average = (total + u128::from(count) / 2) / u128::from(count);
        Some(PerformanceEvidence {
            scope,
            average_views: u64::try_from(average).unwrap_or(u64::MAX),
            videos: count as u32,
            projected: videos.iter().filter(|v| v.first_week.projected).count() as u32,
            basis: self.videos.len() as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChannelId, Network, NetworkAccountId, PostLink, ProfileId, RenderId, ThemeId};

    const DAY: u64 = 24 * 3600;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    fn snap(publication: PublicationId, day: f64, views: u64) -> MetricsSnapshot {
        MetricsSnapshot {
            publication,
            taken_at: at((day * DAY as f64) as u64),
            views,
            likes: None,
            comments: None,
        }
    }

    fn week(posted: SystemTime, snaps: &[MetricsSnapshot]) -> Option<(u64, bool)> {
        first_week(posted, snaps).map(|w| (w.views, w.projected))
    }

    #[test]
    fn a_week_is_read_between_the_snapshots_around_it() {
        let p = PublicationId::new();
        // 1,000 at day 5, 3,000 at day 9: day 7 sits halfway.
        let snaps = [snap(p, 9.0, 3_000), snap(p, 5.0, 1_000)];
        assert_eq!(week(at(0), &snaps), Some((2_000, false)));
        // Exactly at seven days.
        assert_eq!(week(at(0), &[snap(p, 7.0, 4_200)]), Some((4_200, false)));
    }

    #[test]
    fn a_first_snapshot_after_a_week_draws_the_line_from_zero() {
        let p = PublicationId::new();
        // Synced late: 28,000 at day 28 reads as 7,000 at day 7.
        assert_eq!(
            week(at(0), &[snap(p, 28.0, 28_000), snap(p, 60.0, 90_000)]),
            Some((7_000, false))
        );
    }

    #[test]
    fn a_young_post_is_projected_at_its_pace_and_a_very_young_one_waits() {
        let p = PublicationId::new();
        assert_eq!(week(at(0), &[snap(p, 3.5, 1_000)]), Some((2_000, true)));
        assert_eq!(
            week(at(0), &[snap(p, 1.0, 900), snap(p, 1.9, 1_000)]),
            None,
            "under two days says too little"
        );
        assert_eq!(week(at(0), &[]), None);
    }

    #[test]
    fn snapshots_before_the_post_went_live_are_ignored() {
        let p = PublicationId::new();
        let posted = at(10 * DAY);
        let snaps = [snap(p, 9.0, 50), snap(p, 10.0, 60), snap(p, 17.0, 700)];
        assert_eq!(week(posted, &snaps), Some((700, false)));
    }

    #[test]
    fn a_recount_never_reads_below_zero() {
        let p = PublicationId::new();
        let snaps = [snap(p, 6.0, 100), snap(p, 8.0, 0)];
        assert_eq!(week(at(0), &snaps), Some((50, false)));
    }

    #[test]
    fn standing_bands_follow_the_ratio_to_the_usual() {
        assert_eq!(Standing::of(400, 1_000), Standing::FarBelow);
        assert_eq!(Standing::of(500, 1_000), Standing::Below);
        assert_eq!(Standing::of(800, 1_000), Standing::Usual);
        assert_eq!(Standing::of(1_250, 1_000), Standing::Usual);
        assert_eq!(Standing::of(2_000, 1_000), Standing::Above);
        assert_eq!(Standing::of(2_001, 1_000), Standing::FarAbove);
        assert_eq!(Standing::of(0, 0), Standing::Usual);
        assert_eq!(Standing::of(5, 0), Standing::FarAbove);
    }

    #[test]
    fn scope_codes_round_trip() {
        for scope in [EvidenceScope::Niche, EvidenceScope::Channel] {
            assert_eq!(EvidenceScope::from_code(scope.code()), Some(scope));
        }
        assert_eq!(EvidenceScope::from_code("theme"), None);
    }

    struct Channel {
        owner: ProfileId,
        channel: ChannelId,
        projects: Vec<VideoProject>,
        publications: Vec<Publication>,
        snapshots: Vec<MetricsSnapshot>,
    }

    impl Channel {
        fn new() -> Self {
            Self {
                owner: ProfileId::new(),
                channel: ChannelId::new(),
                projects: Vec::new(),
                publications: Vec::new(),
                snapshots: Vec::new(),
            }
        }

        fn project(&mut self, title: &str, niche: &str) -> VideoProjectId {
            let project = VideoProject {
                id: VideoProjectId::new(),
                owner: self.owner,
                channel: self.channel,
                niche: Niche::new(niche).unwrap(),
                theme: ThemeId::new(),
                title: title.into(),
                created_at: at(0),
                persona: None,
            };
            let id = project.id;
            self.projects.push(project);
            id
        }

        /// A post of `project` on `network`, live on `day`, with `views`
        /// on each (day, views).
        fn post(
            &mut self,
            project: VideoProjectId,
            network: Network,
            day: u64,
            views: &[(f64, u64)],
        ) {
            let url = match network {
                Network::YouTube => format!(
                    "https://www.youtube.com/watch?v=vid{:08}",
                    self.publications.len()
                ),
                _ => format!(
                    "https://www.tiktok.com/@a/video/7{:018}",
                    self.publications.len()
                ),
            };
            let publication = Publication {
                id: PublicationId::new(),
                owner: self.owner,
                project,
                account: NetworkAccountId::new(),
                render: RenderId::new(),
                link: PostLink::parse(network, &url).unwrap(),
                posted_at: at(day * DAY),
                linked_at: at(day * DAY),
                checked_at: None,
                missing_since: None,
            };
            for &(age, n) in views {
                self.snapshots
                    .push(snap(publication.id, day as f64 + age, n));
            }
            self.publications.push(publication);
        }

        fn past(&self) -> PastPerformance {
            PastPerformance::of_channel(&self.projects, &self.publications, &self.snapshots)
        }
    }

    #[test]
    fn videos_are_projects_with_a_first_week_newest_first() {
        let mut c = Channel::new();
        let old = c.project("Old", "space history");
        c.post(old, Network::YouTube, 0, &[(7.0, 1_000)]);
        let new = c.project("New", "space history");
        c.post(new, Network::YouTube, 20, &[(3.5, 500)]);
        let fresh = c.project("Too fresh", "space history");
        c.post(fresh, Network::YouTube, 30, &[(1.0, 100)]);
        let unposted = c.project("Not posted", "space history");
        let _ = unposted;
        let tiktok_only = c.project("TikTok only", "space history");
        c.post(tiktok_only, Network::TikTok, 0, &[]);

        let past = c.past();
        let titles: Vec<_> = past.videos().iter().map(|v| v.title.as_str()).collect();
        assert_eq!(titles, ["New", "Old"]);
        assert_eq!(
            past.videos()[0].first_week,
            FirstWeek {
                views: 1_000,
                projected: true
            }
        );
        assert_eq!(past.videos()[0].niche.label(), "space history");
        assert_eq!(past.videos()[1].posted_at, at(0));
    }

    #[test]
    fn a_projects_posts_add_up() {
        let mut c = Channel::new();
        let project = c.project("Twice", "space history");
        c.post(project, Network::YouTube, 2, &[(7.0, 1_000)]);
        c.post(project, Network::TikTok, 1, &[(7.0, 300)]);
        let past = c.past();
        assert_eq!(past.videos().len(), 1);
        assert_eq!(past.videos()[0].first_week.views, 1_300);
        assert_eq!(past.videos()[0].posted_at, at(DAY), "the first post");
    }

    #[test]
    fn posts_of_other_channels_projects_are_left_out() {
        let mut c = Channel::new();
        let mine = c.project("Mine", "space history");
        c.post(mine, Network::YouTube, 0, &[(7.0, 1_000)]);
        c.post(VideoProjectId::new(), Network::YouTube, 0, &[(7.0, 9_000)]);
        assert_eq!(c.past().videos().len(), 1);
    }

    #[test]
    fn the_usual_is_the_median_first_week() {
        let mut c = Channel::new();
        assert_eq!(c.past().usual(), None);
        for (title, views) in [("a", 100), ("b", 1_000), ("c", 50_000)] {
            let p = c.project(title, "space history");
            c.post(p, Network::YouTube, 0, &[(7.0, views)]);
        }
        let past = c.past();
        assert_eq!(
            past.usual(),
            Some(1_000),
            "one viral video does not move it"
        );
        let standings: Vec<_> = past
            .videos()
            .iter()
            .map(|v| (v.title.as_str(), past.standing(v)))
            .collect();
        assert!(standings.contains(&("a", Standing::FarBelow)));
        assert!(standings.contains(&("b", Standing::Usual)));
        assert!(standings.contains(&("c", Standing::FarAbove)));

        let d = c.project("d", "space history");
        c.post(d, Network::YouTube, 0, &[(7.0, 2_000)]);
        assert_eq!(c.past().usual(), Some(1_000), "the lower middle");
    }

    #[test]
    fn evidence_averages_the_niche_or_else_the_channel() {
        let mut c = Channel::new();
        assert_eq!(
            c.past().evidence(&Niche::new("space history").unwrap()),
            None
        );
        for (title, niche, views, age) in [
            ("a", "space history", 1_000, 7.0),
            ("b", "Space  History", 2_001, 3.5),
            ("c", "deep sea", 9_000, 7.0),
        ] {
            let p = c.project(title, niche);
            c.post(p, Network::YouTube, 0, &[(age, views)]);
        }
        let past = c.past();
        assert_eq!(
            past.evidence(&Niche::new("space history").unwrap()),
            Some(PerformanceEvidence {
                scope: EvidenceScope::Niche,
                // (1,000 + 4,002) / 2
                average_views: 2_501,
                videos: 2,
                projected: 1,
                basis: 3,
            })
        );
        assert_eq!(
            past.evidence(&Niche::new("volcanoes").unwrap()),
            Some(PerformanceEvidence {
                scope: EvidenceScope::Channel,
                average_views: 4_667,
                videos: 3,
                projected: 1,
                basis: 3,
            })
        );
    }
}
