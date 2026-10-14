//! Deleting stored YouTube data (issue #145, ADR-0012): the retention
//! rule's upkeep, a disconnection and the user's own request.

use std::time::SystemTime;

use bardo_domain::{
    Forgotten, NetworkAccountId, PRIVACY_POLICY_VERSION, ProfileId, RepositoryError,
    YouTubeDataStore, YouTubeRetention,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension as _, Transaction, params, params_from_iter};

use crate::{Database, boxed, to_unix_millis};

/// Every ranking column of a theme, cleared together (the table's checks
/// hold them all set or all empty).
const CLEAR_RANKING: &str = "fit_score = NULL, fit_confidence = NULL, trend_score = NULL,
     trend_confidence = NULL, competition_score = NULL, competition_confidence = NULL,
     ranked_by = NULL, ranked_at = NULL, performance_score = NULL,
     performance_confidence = NULL, performance_scope = NULL, performance_views = NULL,
     performance_videos = NULL, performance_projected = NULL, performance_basis = NULL";

/// Every owner column of a snapshot, cleared together.
const CLEAR_OWNER: &str = "owner_views = NULL, engaged_views = NULL, minutes_watched = NULL,
     average_view_seconds = NULL, average_view_share = NULL, revenue_micros = NULL,
     cpm_micros = NULL, playback_cpm_micros = NULL, monetized = NULL";

/// The owner's YouTube posts whose account is not one of `authorized`:
/// a subquery on `?1` (the owner) and `?2…` (the accounts).
fn unauthorized_posts(authorized: usize) -> String {
    let mut query = "SELECT id FROM publication
         WHERE profile_id = ?1 AND network = 'youtube'"
        .to_owned();
    if authorized > 0 {
        let accounts: Vec<String> = (2..authorized + 2).map(|n| format!("?{n}")).collect();
        query.push_str(&format!(" AND account_id NOT IN ({})", accounts.join(", ")));
    }
    query
}

fn forget_research_and_rankings(
    tx: &Transaction<'_>,
    owner: &str,
    before: Option<i64>,
) -> Result<Forgotten, RepositoryError> {
    let before = before.unwrap_or(i64::MAX);
    let research = tx
        .execute(
            "DELETE FROM niche_research WHERE profile_id = ?1 AND fetched_at < ?2",
            params![owner, before],
        )
        .map_err(boxed)?;
    let rankings = tx
        .execute(
            &format!(
                "UPDATE theme SET {CLEAR_RANKING}
                 WHERE profile_id = ?1 AND ranked_at IS NOT NULL AND ranked_at < ?2"
            ),
            params![owner, before],
        )
        .map_err(boxed)?;
    Ok(Forgotten {
        research,
        rankings,
        ..Forgotten::default()
    })
}

/// Leaves a backup copy without the YouTube data it may not keep
/// (ADR-0012). A backup can outlast any 30 days, so it holds none of the
/// data that expires: niche research, theme rankings, and the numbers of
/// YouTube posts whose channel is not connected. A connected channel's
/// own numbers stay; restored, the upkeep confirms that connection before
/// anything else and deletes them with it if it lapsed.
pub(crate) fn strip_for_backup(copy: &Connection) -> rusqlite::Result<()> {
    let unconnected = "SELECT id FROM publication
         WHERE network = 'youtube'
           AND account_id NOT IN (SELECT account_id FROM network_connection)";
    // The copy's connection does not enforce foreign keys: children first.
    copy.execute_batch(&format!(
        "DELETE FROM niche_research_upload;
         DELETE FROM niche_research;
         UPDATE theme SET {CLEAR_RANKING} WHERE ranked_at IS NOT NULL;
         DELETE FROM retention_point WHERE publication_id IN ({unconnected});
         DELETE FROM metrics_snapshot WHERE publication_id IN ({unconnected});"
    ))
}

impl YouTubeDataStore for Database {
    fn forget_expired(
        &self,
        owner: ProfileId,
        rule: &YouTubeRetention,
    ) -> Result<Forgotten, RepositoryError> {
        let owner = owner.to_string();
        let before = to_unix_millis(rule.keep_since);
        let accounts: Vec<Value> = rule
            .authorized
            .iter()
            .map(|account| Value::Text(account.to_string()))
            .collect();
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let mut forgotten = forget_research_and_rankings(&tx, &owner, Some(before))?;

        // `?1` the owner, `?2…` the authorized accounts, then the cutoff.
        let unauthorized = unauthorized_posts(accounts.len());
        let cutoff = format!("?{}", accounts.len() + 2);
        let only_owner = || std::iter::once(Value::Text(owner.clone())).chain(accounts.clone());
        let with_cutoff = || only_owner().chain(std::iter::once(Value::Integer(before)));
        // Read without the channel's authorization: no older than the
        // cutoff (III.E.4.b, the last paragraph).
        forgotten.snapshots += tx
            .execute(
                &format!(
                    "DELETE FROM metrics_snapshot
                     WHERE publication_id IN ({unauthorized})
                       AND taken_at < {cutoff}"
                ),
                params_from_iter(with_cutoff()),
            )
            .map_err(boxed)?;
        // What only the channel's authorization gave: not without it.
        forgotten.owner_numbers += tx
            .execute(
                &format!(
                    "UPDATE metrics_snapshot SET {CLEAR_OWNER}
                     WHERE publication_id IN ({unauthorized}) AND owner_views IS NOT NULL"
                ),
                params_from_iter(only_owner()),
            )
            .map_err(boxed)?;
        forgotten.curves += tx
            .query_row(
                &format!(
                    "SELECT COUNT(DISTINCT publication_id) FROM retention_point
                     WHERE publication_id IN ({unauthorized})"
                ),
                params_from_iter(only_owner()),
                |row| row.get::<_, i64>(0),
            )
            .map_err(boxed)? as usize;
        tx.execute(
            &format!("DELETE FROM retention_point WHERE publication_id IN ({unauthorized})"),
            params_from_iter(only_owner()),
        )
        .map_err(boxed)?;

        // A post YouTube stopped showing before the cutoff: deleted, or no
        // longer the channel's (III.E.4.b asks to check every 30 days).
        let gone = "SELECT id FROM publication
                    WHERE profile_id = ?1 AND network = 'youtube'
                      AND missing_since IS NOT NULL AND missing_since < ?2";
        forgotten.snapshots += tx
            .execute(
                &format!("DELETE FROM metrics_snapshot WHERE publication_id IN ({gone})"),
                params![owner, before],
            )
            .map_err(boxed)?;
        forgotten.curves += tx
            .query_row(
                &format!(
                    "SELECT COUNT(DISTINCT publication_id) FROM retention_point
                     WHERE publication_id IN ({gone})"
                ),
                params![owner, before],
                |row| row.get::<_, i64>(0),
            )
            .map_err(boxed)? as usize;
        tx.execute(
            &format!("DELETE FROM retention_point WHERE publication_id IN ({gone})"),
            params![owner, before],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)?;
        Ok(forgotten)
    }

    fn forget_account(
        &self,
        account: NetworkAccountId,
        keep_since: SystemTime,
    ) -> Result<Forgotten, RepositoryError> {
        let account = account.to_string();
        let before = to_unix_millis(keep_since);
        let posts = "SELECT id FROM publication WHERE account_id = ?1 AND network = 'youtube'";
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let snapshots = tx
            .execute(
                &format!(
                    "DELETE FROM metrics_snapshot
                     WHERE publication_id IN ({posts}) AND taken_at < ?2"
                ),
                params![account, before],
            )
            .map_err(boxed)?;
        let owner_numbers = tx
            .execute(
                &format!(
                    "UPDATE metrics_snapshot SET {CLEAR_OWNER}
                     WHERE publication_id IN ({posts}) AND owner_views IS NOT NULL"
                ),
                params![account],
            )
            .map_err(boxed)?;
        let curves = tx
            .query_row(
                &format!(
                    "SELECT COUNT(DISTINCT publication_id) FROM retention_point
                     WHERE publication_id IN ({posts})"
                ),
                params![account],
                |row| row.get::<_, i64>(0),
            )
            .map_err(boxed)? as usize;
        tx.execute(
            &format!("DELETE FROM retention_point WHERE publication_id IN ({posts})"),
            params![account],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)?;
        Ok(Forgotten {
            snapshots,
            owner_numbers,
            curves,
            ..Forgotten::default()
        })
    }

    fn forget_all(&self, owner: ProfileId) -> Result<Forgotten, RepositoryError> {
        let owner = owner.to_string();
        let posts = "SELECT id FROM publication WHERE profile_id = ?1 AND network = 'youtube'";
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let mut forgotten = forget_research_and_rankings(&tx, &owner, None)?;
        forgotten.snapshots = tx
            .execute(
                &format!("DELETE FROM metrics_snapshot WHERE publication_id IN ({posts})"),
                params![owner],
            )
            .map_err(boxed)?;
        forgotten.curves = tx
            .query_row(
                &format!(
                    "SELECT COUNT(DISTINCT publication_id) FROM retention_point
                     WHERE publication_id IN ({posts})"
                ),
                params![owner],
                |row| row.get::<_, i64>(0),
            )
            .map_err(boxed)? as usize;
        tx.execute(
            &format!("DELETE FROM retention_point WHERE publication_id IN ({posts})"),
            params![owner],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)?;
        Ok(forgotten)
    }

    fn privacy_accepted(&self, owner: ProfileId) -> Result<bool, RepositoryError> {
        let version: Option<i64> = self
            .conn()
            .query_row(
                "SELECT privacy_version FROM user_profile WHERE id = ?1",
                [owner.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(boxed)?
            .flatten();
        Ok(version == Some(i64::from(PRIVACY_POLICY_VERSION)))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Insights, MetricsSnapshot,
        Network, Niche, PostLink, ProfileRepository, Publication, PublicationId, PublicationKind,
        PublicationRepository, RenderId, Theme, ThemeIdea, ThemeRepository, UiLanguage,
        UserProfile, VideoProject,
    };

    use super::*;

    const DAY: u64 = 86_400;

    fn time(days: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + days * DAY)
    }

    struct Setup {
        db: Database,
        profile: UserProfile,
        project: VideoProject,
    }

    fn setup() -> Setup {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Space Archives".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(profile.id, details);
        ChannelRepository::save(&db, &channel).unwrap();
        let mut theme = Theme::suggested(
            profile.id,
            channel.id,
            Niche::new("space history").unwrap(),
            ThemeIdea::new("The lost probe", "").unwrap(),
            time(0),
            0,
            None,
        );
        db.save_themes(std::slice::from_ref(&theme)).unwrap();
        let project = theme.approve(time(0)).unwrap();
        db.start_project(&theme, &project).unwrap();
        Setup {
            db,
            profile,
            project,
        }
    }

    fn post(s: &Setup, network: Network, account: NetworkAccountId) -> Publication {
        let link = match network {
            Network::YouTube => format!(
                "https://youtu.be/{}",
                &PublicationId::new().to_string()[..11]
            ),
            _ => "https://www.tiktok.com/@a/video/7301234567890123456".to_owned(),
        };
        let post = Publication {
            id: PublicationId::new(),
            owner: s.profile.id,
            project: s.project.id,
            account,
            network,
            render: RenderId::new(),
            link: Some(PostLink::parse(network, &link).unwrap()),
            kind: PublicationKind::Manual,
            posted_at: time(0),
            linked_at: time(0),
            checked_at: None,
            missing_since: None,
            insights_id: None,
        };
        // One post per project and network: each test post its own project.
        s.db.conn()
            .execute(
                "DELETE FROM publication WHERE network = ?1",
                [network.code()],
            )
            .ok();
        s.db.save_publication(&post).unwrap();
        post
    }

    /// Snapshots of `post` taken on `days`, the owner's numbers on each.
    fn snapshots(s: &Setup, post: &Publication, days: &[u64]) {
        let snapshots: Vec<MetricsSnapshot> = days
            .iter()
            .map(|day| MetricsSnapshot {
                publication: post.id,
                taken_at: time(*day),
                views: 100 * day,
                likes: None,
                comments: None,
                owner: None,
                insights: Insights::default(),
            })
            .collect();
        s.db.save_sync(&[], &snapshots).unwrap();
        s.db.conn()
            .execute(
                "UPDATE metrics_snapshot SET owner_views = views, engaged_views = 1,
                     minutes_watched = 1, average_view_seconds = 1, average_view_share = 1,
                     monetized = 0
                 WHERE publication_id = ?1",
                [post.id.to_string()],
            )
            .unwrap();
        s.db.conn()
            .execute(
                "INSERT OR REPLACE INTO retention_point (publication_id, elapsed, watch, relative, read_at)
                 VALUES (?1, 0, 10000, NULL, ?2)",
                params![post.id.to_string(), to_unix_millis(time(*days.last().unwrap()))],
            )
            .unwrap();
    }

    fn count(s: &Setup, sql: &str, post: &Publication) -> i64 {
        s.db.conn()
            .query_row(sql, [post.id.to_string()], |row| row.get(0))
            .unwrap()
    }

    fn kept_days(s: &Setup, post: &Publication) -> Vec<u64> {
        let conn = s.db.conn();
        let mut statement = conn
            .prepare(
                "SELECT views FROM metrics_snapshot WHERE publication_id = ?1 ORDER BY taken_at",
            )
            .unwrap();
        statement
            .query_map([post.id.to_string()], |row| row.get::<_, i64>(0))
            .unwrap()
            .map(|views| views.unwrap() as u64 / 100)
            .collect()
    }

    fn owner_numbers(s: &Setup, post: &Publication) -> i64 {
        count(
            s,
            "SELECT COUNT(*) FROM metrics_snapshot WHERE publication_id = ?1 AND owner_views IS NOT NULL",
            post,
        )
    }

    fn curve(s: &Setup, post: &Publication) -> i64 {
        count(
            s,
            "SELECT COUNT(*) FROM retention_point WHERE publication_id = ?1",
            post,
        )
    }

    fn research(s: &Setup, niche: &str, fetched: SystemTime) {
        s.db.conn()
            .execute(
                "INSERT INTO niche_research (profile_id, niche_key, niche, country, language,
                                             fetched_at, upload_volume)
                 VALUES (?1, ?2, ?2, 'US', 'en', ?3, 10)",
                params![s.profile.id.to_string(), niche, to_unix_millis(fetched)],
            )
            .unwrap();
        s.db.conn()
            .execute(
                "INSERT INTO niche_research_upload (research_id, position, channel_id,
                                                    published_at, views)
                 VALUES (last_insert_rowid(), 0, 'UCx', 0, 5)",
                [],
            )
            .unwrap();
    }

    fn researched(s: &Setup) -> Vec<String> {
        let conn = s.db.conn();
        let mut statement = conn
            .prepare("SELECT niche FROM niche_research ORDER BY niche")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn rank_every_theme(s: &Setup, at: SystemTime) {
        s.db.conn()
            .execute(
                "UPDATE theme SET fit_score = 50, fit_confidence = 0.5, trend_score = 50,
                     trend_confidence = 0.5, competition_score = 50,
                     competition_confidence = 0.5, ranked_by = 'engine', ranked_at = ?1,
                     performance_score = 40, performance_confidence = 0.4,
                     performance_scope = 'channel', performance_views = 900,
                     performance_videos = 1, performance_projected = 0, performance_basis = 1",
                [to_unix_millis(at)],
            )
            .unwrap();
    }

    fn ranked(s: &Setup) -> i64 {
        s.db.conn()
            .query_row(
                "SELECT COUNT(*) FROM theme WHERE ranked_at IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn research_and_rankings_go_once_older_than_the_cutoff() {
        let s = setup();
        research(&s, "old", time(9));
        research(&s, "edge", time(10));
        research(&s, "new", time(20));
        rank_every_theme(&s, time(10));
        let rule = YouTubeRetention {
            keep_since: time(10),
            authorized: Vec::new(),
        };

        let forgotten = s.db.forget_expired(s.profile.id, &rule).unwrap();
        assert_eq!(researched(&s), ["edge", "new"]);
        assert_eq!((forgotten.research, forgotten.rankings), (1, 0));
        let orphans: i64 =
            s.db.conn()
                .query_row("SELECT COUNT(*) FROM niche_research_upload", [], |row| {
                    row.get(0)
                })
                .unwrap();
        assert_eq!(orphans, 2, "the uploads go with their result");

        let later = YouTubeRetention {
            keep_since: time(10) + Duration::from_millis(1),
            ..rule
        };
        let forgotten = s.db.forget_expired(s.profile.id, &later).unwrap();
        assert_eq!(ranked(&s), 0, "every ranking column cleared together");
        assert_eq!((forgotten.research, forgotten.rankings), (1, 1));
    }

    #[test]
    fn numbers_without_the_channels_authorization_last_until_the_cutoff() {
        let s = setup();
        let account = NetworkAccountId::new();
        let yt = post(&s, Network::YouTube, account);
        snapshots(&s, &yt, &[5, 10, 15]);
        let rule = YouTubeRetention {
            keep_since: time(10),
            authorized: vec![NetworkAccountId::new()],
        };

        let forgotten = s.db.forget_expired(s.profile.id, &rule).unwrap();
        assert_eq!(kept_days(&s, &yt), [10, 15]);
        assert_eq!(
            owner_numbers(&s, &yt),
            0,
            "the owner's numbers need the channel"
        );
        assert_eq!(curve(&s, &yt), 0);
        assert_eq!(
            forgotten,
            Forgotten {
                snapshots: 1,
                owner_numbers: 2,
                curves: 1,
                ..Forgotten::default()
            }
        );
        assert!(s.db.forget_expired(s.profile.id, &rule).unwrap().is_empty());
    }

    #[test]
    fn an_authorized_channel_keeps_its_posts_numbers_whatever_their_age() {
        let s = setup();
        let account = NetworkAccountId::new();
        let yt = post(&s, Network::YouTube, account);
        snapshots(&s, &yt, &[1, 5, 15]);
        let rule = YouTubeRetention {
            keep_since: time(10),
            authorized: vec![NetworkAccountId::new(), account],
        };

        assert!(s.db.forget_expired(s.profile.id, &rule).unwrap().is_empty());
        assert_eq!(kept_days(&s, &yt), [1, 5, 15]);
        assert_eq!(owner_numbers(&s, &yt), 3);
        assert_eq!(curve(&s, &yt), 1);
    }

    #[test]
    fn a_post_youtube_stopped_showing_before_the_cutoff_loses_its_numbers() {
        let s = setup();
        let account = NetworkAccountId::new();
        let mut yt = post(&s, Network::YouTube, account);
        snapshots(&s, &yt, &[1, 5]);
        let rule = YouTubeRetention {
            keep_since: time(10),
            authorized: vec![account],
        };
        yt.checked_at = Some(time(12));
        yt.missing_since = Some(time(10));
        s.db.save_sync(std::slice::from_ref(&yt), &[]).unwrap();
        assert!(s.db.forget_expired(s.profile.id, &rule).unwrap().is_empty());

        yt.missing_since = Some(time(9));
        s.db.save_sync(std::slice::from_ref(&yt), &[]).unwrap();
        let forgotten = s.db.forget_expired(s.profile.id, &rule).unwrap();
        assert_eq!(kept_days(&s, &yt), Vec::<u64>::new());
        assert_eq!(curve(&s, &yt), 0);
        assert_eq!((forgotten.snapshots, forgotten.curves), (2, 1));
    }

    #[test]
    fn other_networks_and_other_profiles_are_left_alone() {
        let s = setup();
        let tiktok = post(&s, Network::TikTok, NetworkAccountId::new());
        snapshots(&s, &tiktok, &[1]);
        research(&s, "old", time(1));
        let rule = YouTubeRetention {
            keep_since: time(10),
            authorized: Vec::new(),
        };

        let other = ProfileId::new();
        assert!(s.db.forget_expired(other, &rule).unwrap().is_empty());
        assert!(s.db.forget_all(other).unwrap().is_empty());
        assert_eq!(researched(&s), ["old"]);
        s.db.forget_expired(s.profile.id, &rule).unwrap();
        s.db.forget_all(s.profile.id).unwrap();
        assert_eq!(kept_days(&s, &tiktok), [1]);
        assert_eq!(owner_numbers(&s, &tiktok), 1);
        assert_eq!(curve(&s, &tiktok), 1);
    }

    #[test]
    fn a_disconnection_takes_what_the_authorization_gave() {
        let s = setup();
        let account = NetworkAccountId::new();
        let yt = post(&s, Network::YouTube, account);
        snapshots(&s, &yt, &[5, 15]);

        assert!(
            s.db.forget_account(NetworkAccountId::new(), time(10))
                .unwrap()
                .is_empty()
        );
        let forgotten = s.db.forget_account(account, time(10)).unwrap();
        assert_eq!(kept_days(&s, &yt), [15], "the public count, still young");
        assert_eq!(owner_numbers(&s, &yt), 0);
        assert_eq!(curve(&s, &yt), 0);
        assert_eq!(
            forgotten,
            Forgotten {
                snapshots: 1,
                owner_numbers: 1,
                curves: 1,
                ..Forgotten::default()
            }
        );
    }

    #[test]
    fn deleting_everything_keeps_the_users_own_records() {
        let s = setup();
        let yt = post(&s, Network::YouTube, NetworkAccountId::new());
        snapshots(&s, &yt, &[5, 15]);
        research(&s, "new", time(20));
        rank_every_theme(&s, time(20));

        let forgotten = s.db.forget_all(s.profile.id).unwrap();
        assert_eq!(
            forgotten,
            Forgotten {
                research: 1,
                rankings: 1,
                snapshots: 2,
                curves: 1,
                ..Forgotten::default()
            }
        );
        assert_eq!(researched(&s), Vec::<String>::new());
        assert_eq!(ranked(&s), 0);
        assert_eq!(kept_days(&s, &yt), Vec::<u64>::new());
        assert_eq!(s.db.publication(yt.id).unwrap(), Some(yt), "the link stays");
        let themes: i64 =
            s.db.conn()
                .query_row("SELECT COUNT(*) FROM theme", [], |row| row.get(0))
                .unwrap();
        assert_eq!(themes, 1, "the theme stays, unranked");
    }

    #[test]
    fn a_backup_keeps_only_a_connected_channels_own_numbers() {
        let s = setup();
        research(&s, "new", time(20));
        rank_every_theme(&s, time(20));
        let unconnected = post(&s, Network::YouTube, NetworkAccountId::new());
        snapshots(&s, &unconnected, &[5, 15]);
        strip_for_backup(&s.db.conn()).unwrap();
        assert_eq!(researched(&s), Vec::<String>::new());
        assert_eq!(ranked(&s), 0);
        assert_eq!(kept_days(&s, &unconnected), Vec::<u64>::new());
        assert_eq!(curve(&s, &unconnected), 0);

        let account = NetworkAccountId::new();
        let connected = post(&s, Network::YouTube, account);
        snapshots(&s, &connected, &[5, 15]);
        {
            let conn = s.db.conn();
            // The account itself is beside the point here.
            conn.pragma_update(None, "foreign_keys", false).unwrap();
            conn.execute(
                "INSERT INTO network_connection
                 VALUES (?1, ?2, 'connected', 'UC1', 'Channel', '', 0, 0, NULL)",
                params![account.to_string(), s.profile.id.to_string()],
            )
            .unwrap();
            strip_for_backup(&conn).unwrap();
            conn.pragma_update(None, "foreign_keys", true).unwrap();
        }
        assert_eq!(kept_days(&s, &connected), [5, 15]);
        assert_eq!(owner_numbers(&s, &connected), 2);
        assert_eq!(curve(&s, &connected), 1);
    }

    #[test]
    fn the_privacy_policy_is_read_fresh_from_the_profile() {
        let s = setup();
        assert!(!s.db.privacy_accepted(s.profile.id).unwrap());
        let accepted = UserProfile {
            privacy: Some(bardo_domain::PrivacyConsent::current(time(1))),
            ..s.profile.clone()
        };
        ProfileRepository::save(&s.db, &accepted).unwrap();
        assert!(s.db.privacy_accepted(s.profile.id).unwrap());
        assert!(
            !s.db
                .privacy_accepted(bardo_domain::ProfileId::new())
                .unwrap()
        );
    }
}
