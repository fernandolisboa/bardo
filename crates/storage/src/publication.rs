use std::time::{Duration, SystemTime};

use bardo_domain::{
    ChannelId, Earnings, Insights, JobId, MetricsSnapshot, Money, MoneyReport, Network,
    NetworkAccountId, OwnerMetrics, PostLink, PostRetention, ProfileId, Publication, PublicationId,
    PublicationKind, PublicationRepository, RenderId, RepositoryError, RetentionCurve,
    RetentionPoint, Share, Upload, UploadStatus, VideoProjectId, Visibility,
};
use rusqlite::{Connection, Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

const PUBLICATION_COLUMNS: &str = "publication.id, publication.project_id, publication.network,
     publication.profile_id, publication.account_id, publication.render_id,
     publication.post_id, publication.url, publication.posted_at, publication.linked_at,
     publication.checked_at, publication.missing_since, publication.kind,
     publication.upload_status, publication.upload_failure, publication.upload_visibility,
     publication.upload_job, publication.upload_publish_at, publication.upload_issue,
     publication.upload_network_id, publication.upload_claimed_at, publication.insights_id";

/// A publication row as SQLite returns it.
struct PublicationRow {
    id: String,
    project: String,
    network: String,
    owner: String,
    account: String,
    render: String,
    post_id: Option<String>,
    url: Option<String>,
    posted_at: i64,
    linked_at: i64,
    checked_at: Option<i64>,
    missing_since: Option<i64>,
    kind: String,
    upload_status: Option<String>,
    upload_failure: Option<String>,
    upload_visibility: Option<String>,
    upload_job: Option<String>,
    upload_publish_at: Option<i64>,
    upload_issue: Option<String>,
    upload_network_id: Option<String>,
    upload_claimed_at: Option<i64>,
    insights_id: Option<String>,
}

impl PublicationRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            project: row.get(1)?,
            network: row.get(2)?,
            owner: row.get(3)?,
            account: row.get(4)?,
            render: row.get(5)?,
            post_id: row.get(6)?,
            url: row.get(7)?,
            posted_at: row.get(8)?,
            linked_at: row.get(9)?,
            checked_at: row.get(10)?,
            missing_since: row.get(11)?,
            kind: row.get(12)?,
            upload_status: row.get(13)?,
            upload_failure: row.get(14)?,
            upload_visibility: row.get(15)?,
            upload_job: row.get(16)?,
            upload_publish_at: row.get(17)?,
            upload_issue: row.get(18)?,
            upload_network_id: row.get(19)?,
            upload_claimed_at: row.get(20)?,
            insights_id: row.get(21)?,
        })
    }

    fn publication(self) -> Result<Publication, RepositoryError> {
        let network: Network = self.network.parse().map_err(boxed)?;
        let broken =
            |what: &str| RepositoryError(format!("publication {}: {what}", self.id).into());
        let kind = match self.kind.as_str() {
            "manual" => PublicationKind::Manual,
            "uploaded" => PublicationKind::Uploaded(Upload {
                status: self
                    .upload_status
                    .as_deref()
                    .and_then(|status| {
                        UploadStatus::from_code(status, self.upload_failure.as_deref())
                    })
                    .ok_or_else(|| broken("unknown upload status"))?,
                visibility: self
                    .upload_visibility
                    .as_deref()
                    .ok_or_else(|| broken("no upload visibility"))?
                    .parse::<Visibility>()
                    .map_err(boxed)?,
                publish_at: self.upload_publish_at.map(from_unix_millis),
                job: JobId::from(uuid(
                    self.upload_job
                        .as_deref()
                        .ok_or_else(|| broken("no upload job"))?,
                )?),
                issue: self.upload_issue,
                network_id: self.upload_network_id,
                claimed_at: self.upload_claimed_at.map(from_unix_millis),
            }),
            other => return Err(broken(&format!("unknown kind {other}"))),
        };
        let link = match (self.post_id, self.url) {
            (Some(post_id), Some(url)) => Some(PostLink::restore(network, post_id, url)),
            _ => None,
        };
        Ok(Publication {
            id: PublicationId::from(uuid(&self.id)?),
            owner: ProfileId::from(uuid(&self.owner)?),
            project: VideoProjectId::from(uuid(&self.project)?),
            account: NetworkAccountId::from(uuid(&self.account)?),
            network,
            render: RenderId::from(uuid(&self.render)?),
            link,
            kind,
            posted_at: from_unix_millis(self.posted_at),
            linked_at: from_unix_millis(self.linked_at),
            checked_at: self.checked_at.map(from_unix_millis),
            missing_since: self.missing_since.map(from_unix_millis),
            insights_id: self.insights_id,
        })
    }
}

fn query_publications(
    conn: &Connection,
    sql: &str,
    param: String,
) -> Result<Vec<Publication>, RepositoryError> {
    let rows = conn
        .prepare(sql)
        .and_then(|mut statement| {
            statement
                .query_map([param], PublicationRow::read)?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(boxed)?;
    rows.into_iter().map(PublicationRow::publication).collect()
}

/// The snapshot columns `read_snapshot` reads, in order.
const SNAPSHOT_COLUMNS: &str = "s.publication_id, s.taken_at, s.views, s.likes, s.comments,
     s.owner_views, s.engaged_views, s.minutes_watched, s.average_view_seconds,
     s.average_view_share, s.revenue_micros, s.cpm_micros, s.playback_cpm_micros, s.monetized,
     s.shares, s.saves, s.reach, s.interactions, s.average_watch_ms, s.watch_time_ms";

/// A snapshot row as SQLite returns it.
struct SnapshotRow {
    publication: String,
    taken_at: i64,
    counts: [Option<i64>; 3],
    /// Owner views, engaged views, minutes, seconds, share.
    owner: [Option<i64>; 5],
    /// Revenue, CPM, playback-based CPM.
    money: [Option<i64>; 3],
    monetized: Option<i64>,
    /// Shares, saves, reach, interactions, average and total watch time.
    insights: [Option<i64>; 6],
}

impl SnapshotRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            publication: row.get(0)?,
            taken_at: row.get(1)?,
            counts: [row.get(2)?, row.get(3)?, row.get(4)?],
            owner: [
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
            ],
            money: [row.get(10)?, row.get(11)?, row.get(12)?],
            monetized: row.get(13)?,
            insights: [
                row.get(14)?,
                row.get(15)?,
                row.get(16)?,
                row.get(17)?,
                row.get(18)?,
                row.get(19)?,
            ],
        })
    }

    fn snapshot(self) -> Result<MetricsSnapshot, RepositoryError> {
        let count = |n: i64| u64::try_from(n).unwrap_or(0);
        let [views, likes, comments] = self.counts;
        let owner = match (self.owner, self.monetized) {
            (
                [
                    Some(views),
                    Some(engaged),
                    Some(minutes),
                    Some(seconds),
                    Some(share),
                ],
                Some(m),
            ) => {
                let earnings = match (m, self.money) {
                    (1, [Some(revenue), Some(cpm), Some(playback)]) => {
                        let money = |n: i64| Money::from_micros(count(n));
                        Earnings::Monetized(MoneyReport {
                            revenue: money(revenue),
                            cpm: money(cpm),
                            playback_cpm: money(playback),
                        })
                    }
                    _ => Earnings::NotMonetized,
                };
                Some(OwnerMetrics {
                    views: count(views),
                    engaged_views: count(engaged),
                    minutes_watched: count(minutes),
                    average_view_seconds: count(seconds),
                    average_view_share: Share::from_ten_thousandths(
                        u32::try_from(share).unwrap_or(0),
                    ),
                    earnings,
                })
            }
            _ => None,
        };
        let [
            shares,
            saves,
            reach,
            interactions,
            average_watch,
            watch_time,
        ] = self.insights;
        let millis = |n: i64| Duration::from_millis(count(n));
        Ok(MetricsSnapshot {
            publication: PublicationId::from(uuid(&self.publication)?),
            taken_at: from_unix_millis(self.taken_at),
            views: count(views.unwrap_or(0)),
            likes: likes.map(count),
            comments: comments.map(count),
            owner,
            insights: Insights {
                shares: shares.map(count),
                saves: saves.map(count),
                reach: reach.map(count),
                interactions: interactions.map(count),
                average_watch: average_watch.map(millis),
                watch_time: watch_time.map(millis),
            },
        })
    }
}

fn query_snapshots(
    conn: &Connection,
    sql: &str,
    param: String,
) -> Result<Vec<MetricsSnapshot>, RepositoryError> {
    let rows = conn
        .prepare(sql)
        .and_then(|mut statement| {
            statement
                .query_map([param], SnapshotRow::read)?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(boxed)?;
    rows.into_iter().map(SnapshotRow::snapshot).collect()
}

/// Retention points of one or more publications, gathered into a curve
/// each.
fn query_retention(
    conn: &Connection,
    sql: &str,
    param: String,
) -> Result<Vec<PostRetention>, RepositoryError> {
    let rows = conn
        .prepare(sql)
        .and_then(|mut statement| {
            statement
                .query_map([param], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(boxed)?;
    let share = |n: i64| Share::from_ten_thousandths(u32::try_from(n).unwrap_or(0));
    let mut curves: Vec<(PublicationId, SystemTime, Vec<RetentionPoint>)> = Vec::new();
    for (publication, elapsed, watch, relative, read_at) in rows {
        let publication = PublicationId::from(uuid(&publication)?);
        let point = RetentionPoint {
            elapsed: share(elapsed),
            watch: share(watch),
            relative: relative.map(share),
        };
        match curves.last_mut() {
            Some((id, _, points)) if *id == publication => points.push(point),
            _ => curves.push((publication, from_unix_millis(read_at), vec![point])),
        }
    }
    Ok(curves
        .into_iter()
        .map(|(publication, read_at, points)| PostRetention {
            publication,
            read_at,
            curve: RetentionCurve::of(points),
        })
        .collect())
}

/// A snapshot's owner columns, in `SNAPSHOT_COLUMNS` order from
/// `owner_views`.
fn owner_columns(owner: Option<&OwnerMetrics>) -> [Option<i64>; 9] {
    let Some(owner) = owner else {
        return [None; 9];
    };
    let money = owner.money();
    let micros = |pick: fn(&MoneyReport) -> Money| money.map(|m| stored(pick(m).micros()));
    [
        Some(stored(owner.views)),
        Some(stored(owner.engaged_views)),
        Some(stored(owner.minutes_watched)),
        Some(stored(owner.average_view_seconds)),
        Some(i64::from(owner.average_view_share.ten_thousandths())),
        micros(|m| m.revenue),
        micros(|m| m.cpm),
        micros(|m| m.playback_cpm),
        Some(i64::from(money.is_some())),
    ]
}

/// A snapshot's insights columns, in `SNAPSHOT_COLUMNS` order from
/// `shares`.
fn insights_columns(insights: &Insights) -> [Option<i64>; 6] {
    let millis = |d: Duration| stored(u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    [
        insights.shares.map(stored),
        insights.saves.map(stored),
        insights.reach.map(stored),
        insights.interactions.map(stored),
        insights.average_watch.map(millis),
        insights.watch_time.map(millis),
    ]
}

/// Counts beyond SQLite's integers are not real view counts; they clamp.
fn stored(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The statuses whose upload sets `posted_at`: it went live, waits for
/// its publish time, or reached the creator's inbox as a draft.
const SETS_POSTED_AT: &str = "('published', 'restricted', 'scheduled', 'draft_sent')";

/// The statuses after which a later save keeps `posted_at`: it went live.
const LIVE: &str = "('published', 'restricted')";

/// Saves a publication. One saved before keeps its dates and what syncs
/// found (they belong to `save_sync`); its account, render, post and
/// upload change. An upload that went live, or was scheduled, sets when.
fn upsert(conn: &Connection, publication: &Publication) -> Result<(), RepositoryError> {
    // Another post linked for the same project and network goes, with its
    // snapshots.
    conn.execute(
        "DELETE FROM publication WHERE project_id = ?1 AND network = ?2 AND id <> ?3",
        params![
            publication.project.to_string(),
            publication.network().code(),
            publication.id.to_string()
        ],
    )
    .map_err(boxed)?;
    let upload = publication.upload();
    conn.execute(
        &format!(
            "INSERT INTO publication (id, project_id, network, profile_id, account_id, render_id,
                                      post_id, url, posted_at, linked_at, checked_at,
                                      missing_since, kind, upload_status, upload_failure,
                                      upload_visibility, upload_job, upload_publish_at,
                                      upload_issue, upload_network_id, upload_claimed_at,
                                      insights_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18, ?19, ?20, ?21, ?22)
             ON CONFLICT (id) DO UPDATE SET
                 account_id = excluded.account_id,
                 render_id = excluded.render_id,
                 post_id = excluded.post_id,
                 url = excluded.url,
                 kind = excluded.kind,
                 upload_status = excluded.upload_status,
                 upload_failure = excluded.upload_failure,
                 upload_visibility = excluded.upload_visibility,
                 upload_job = excluded.upload_job,
                 upload_publish_at = excluded.upload_publish_at,
                 upload_issue = excluded.upload_issue,
                 upload_network_id = excluded.upload_network_id,
                 upload_claimed_at = excluded.upload_claimed_at,
                 insights_id = COALESCE(excluded.insights_id, publication.insights_id),
                 posted_at = CASE WHEN excluded.upload_status IN {SETS_POSTED_AT}
                                       AND publication.upload_status NOT IN {LIVE}
                                  THEN excluded.posted_at ELSE publication.posted_at END"
        ),
        params![
            publication.id.to_string(),
            publication.project.to_string(),
            publication.network().code(),
            publication.owner.to_string(),
            publication.account.to_string(),
            publication.render.to_string(),
            publication.link.as_ref().map(PostLink::post_id),
            publication.link.as_ref().map(PostLink::url),
            to_unix_millis(publication.posted_at),
            to_unix_millis(publication.linked_at),
            publication.checked_at.map(to_unix_millis),
            publication.missing_since.map(to_unix_millis),
            publication.kind.code(),
            upload.map(|upload| upload.status.code()),
            upload.and_then(|upload| upload.status.failure().map(|failure| failure.code())),
            upload.map(|upload| upload.visibility.code()),
            upload.map(|upload| upload.job.to_string()),
            upload.and_then(|upload| upload.publish_at.map(to_unix_millis)),
            upload.and_then(|upload| upload.issue.as_deref()),
            upload.and_then(|upload| upload.network_id.as_deref()),
            upload.and_then(|upload| upload.claimed_at.map(to_unix_millis)),
            publication.insights_id.as_deref(),
        ],
    )
    .map_err(boxed)?;
    Ok(())
}

impl PublicationRepository for Database {
    fn publications(&self, project: VideoProjectId) -> Result<Vec<Publication>, RepositoryError> {
        let mut publications = query_publications(
            &self.conn(),
            &format!("SELECT {PUBLICATION_COLUMNS} FROM publication WHERE project_id = ?1"),
            project.to_string(),
        )?;
        publications.sort_by_key(Publication::network);
        Ok(publications)
    }

    fn channel_publications(
        &self,
        channel: ChannelId,
    ) -> Result<Vec<Publication>, RepositoryError> {
        query_publications(
            &self.conn(),
            &format!(
                "SELECT {PUBLICATION_COLUMNS} FROM publication
                 JOIN video_project ON video_project.id = publication.project_id
                 WHERE video_project.channel_id = ?1
                 ORDER BY publication.posted_at DESC, publication.rowid DESC"
            ),
            channel.to_string(),
        )
    }

    fn all_publications(&self, owner: ProfileId) -> Result<Vec<Publication>, RepositoryError> {
        query_publications(
            &self.conn(),
            &format!(
                "SELECT {PUBLICATION_COLUMNS} FROM publication WHERE profile_id = ?1
                 ORDER BY publication.posted_at DESC, publication.rowid DESC"
            ),
            owner.to_string(),
        )
    }

    fn publication(&self, id: PublicationId) -> Result<Option<Publication>, RepositoryError> {
        Ok(query_publications(
            &self.conn(),
            &format!("SELECT {PUBLICATION_COLUMNS} FROM publication WHERE id = ?1"),
            id.to_string(),
        )?
        .pop())
    }

    fn save_publication(&self, publication: &Publication) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        upsert(&tx, publication)?;
        tx.commit().map_err(boxed)
    }

    fn save_upload(&self, publication: &Publication) -> Result<bool, RepositoryError> {
        update_upload(&self.conn(), publication, None)
    }

    fn save_schedule(
        &self,
        publication: &Publication,
        from: SystemTime,
    ) -> Result<bool, RepositoryError> {
        update_upload(&self.conn(), publication, Some(from))
    }

    fn claim_upload(
        &self,
        publication: &Publication,
        now: SystemTime,
    ) -> Result<bool, RepositoryError> {
        let Some(upload) = publication.upload() else {
            return Ok(false);
        };
        // One statement, so two runners racing for it cannot both win: the
        // second finds the row claimed by then. The job claims it again on
        // resume, keeping the first claim's time.
        let now = to_unix_millis(now);
        let claimed = self
            .conn()
            .execute(
                "UPDATE publication
                 SET upload_claimed_at = coalesce(upload_claimed_at, ?3)
                 WHERE id = ?1 AND upload_job = ?2
                   AND (upload_status IN ('queued', 'uploading', 'processing')
                        OR (upload_status = 'failed'
                            AND coalesce(upload_failure, '') <> 'schedule_missed'))
                   AND upload_publish_at IS NOT NULL AND upload_publish_at <= ?3",
                params![publication.id.to_string(), upload.job.to_string(), now],
            )
            .map_err(boxed)?;
        Ok(claimed > 0)
    }

    fn remove_publication(&self, id: PublicationId) -> Result<(), RepositoryError> {
        self.conn()
            .execute("DELETE FROM publication WHERE id = ?1", [id.to_string()])
            .map_err(boxed)?;
        Ok(())
    }

    fn save_sync(
        &self,
        checked: &[Publication],
        snapshots: &[MetricsSnapshot],
    ) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        for publication in checked {
            // Only what a sync learns; a publication removed meanwhile
            // stays removed.
            tx.execute(
                "UPDATE publication SET posted_at = ?2, checked_at = ?3, missing_since = ?4,
                     insights_id = COALESCE(?5, insights_id)
                 WHERE id = ?1",
                params![
                    publication.id.to_string(),
                    to_unix_millis(publication.posted_at),
                    publication.checked_at.map(to_unix_millis),
                    publication.missing_since.map(to_unix_millis),
                    publication.insights_id.as_deref(),
                ],
            )
            .map_err(boxed)?;
        }
        for snapshot in snapshots {
            let [
                owner_views,
                engaged,
                minutes,
                seconds,
                share,
                revenue,
                cpm,
                playback,
                monetized,
            ] = owner_columns(snapshot.owner.as_ref());
            let [
                shares,
                saves,
                reach,
                interactions,
                average_watch,
                watch_time,
            ] = insights_columns(&snapshot.insights);
            tx.execute(
                "INSERT INTO metrics_snapshot (publication_id, taken_at, views, likes, comments,
                     owner_views, engaged_views, minutes_watched, average_view_seconds,
                     average_view_share, revenue_micros, cpm_micros, playback_cpm_micros,
                     monetized, shares, saves, reach, interactions, average_watch_ms,
                     watch_time_ms)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                        ?17, ?18, ?19, ?20
                 WHERE EXISTS (SELECT 1 FROM publication WHERE id = ?1)
                 ON CONFLICT (publication_id, taken_at) DO UPDATE SET
                     views = excluded.views,
                     likes = excluded.likes,
                     comments = excluded.comments,
                     owner_views = excluded.owner_views,
                     engaged_views = excluded.engaged_views,
                     minutes_watched = excluded.minutes_watched,
                     average_view_seconds = excluded.average_view_seconds,
                     average_view_share = excluded.average_view_share,
                     revenue_micros = excluded.revenue_micros,
                     cpm_micros = excluded.cpm_micros,
                     playback_cpm_micros = excluded.playback_cpm_micros,
                     monetized = excluded.monetized,
                     shares = excluded.shares,
                     saves = excluded.saves,
                     reach = excluded.reach,
                     interactions = excluded.interactions,
                     average_watch_ms = excluded.average_watch_ms,
                     watch_time_ms = excluded.watch_time_ms",
                params![
                    snapshot.publication.to_string(),
                    to_unix_millis(snapshot.taken_at),
                    stored(snapshot.views),
                    snapshot.likes.map(stored),
                    snapshot.comments.map(stored),
                    owner_views,
                    engaged,
                    minutes,
                    seconds,
                    share,
                    revenue,
                    cpm,
                    playback,
                    monetized,
                    shares,
                    saves,
                    reach,
                    interactions,
                    average_watch,
                    watch_time,
                ],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }

    fn snapshots(
        &self,
        publication: PublicationId,
    ) -> Result<Vec<MetricsSnapshot>, RepositoryError> {
        query_snapshots(
            &self.conn(),
            &format!(
                "SELECT {SNAPSHOT_COLUMNS} FROM metrics_snapshot s
                 WHERE s.publication_id = ?1 ORDER BY s.taken_at"
            ),
            publication.to_string(),
        )
    }

    fn channel_snapshots(
        &self,
        channel: ChannelId,
    ) -> Result<Vec<MetricsSnapshot>, RepositoryError> {
        query_snapshots(
            &self.conn(),
            &format!(
                "SELECT {SNAPSHOT_COLUMNS}
                 FROM metrics_snapshot s
                 JOIN publication p ON p.id = s.publication_id
                 JOIN video_project v ON v.id = p.project_id
                 WHERE v.channel_id = ?1
                 ORDER BY s.taken_at"
            ),
            channel.to_string(),
        )
    }

    fn save_retention(&self, curves: &[PostRetention]) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        for retention in curves {
            let publication = retention.publication.to_string();
            tx.execute(
                "DELETE FROM retention_point WHERE publication_id = ?1",
                [&publication],
            )
            .map_err(boxed)?;
            for point in retention.curve.points() {
                tx.execute(
                    "INSERT INTO retention_point (publication_id, elapsed, watch, relative, read_at)
                     SELECT ?1, ?2, ?3, ?4, ?5
                     WHERE EXISTS (SELECT 1 FROM publication WHERE id = ?1)",
                    params![
                        publication,
                        i64::from(point.elapsed.ten_thousandths()),
                        i64::from(point.watch.ten_thousandths()),
                        point.relative.map(|r| i64::from(r.ten_thousandths())),
                        to_unix_millis(retention.read_at),
                    ],
                )
                .map_err(boxed)?;
            }
        }
        tx.commit().map_err(boxed)
    }

    fn retention(
        &self,
        publication: PublicationId,
    ) -> Result<Option<PostRetention>, RepositoryError> {
        Ok(query_retention(
            &self.conn(),
            "SELECT publication_id, elapsed, watch, relative, read_at FROM retention_point
             WHERE publication_id = ?1 ORDER BY elapsed",
            publication.to_string(),
        )?
        .pop())
    }

    fn channel_retention(&self, channel: ChannelId) -> Result<Vec<PostRetention>, RepositoryError> {
        query_retention(
            &self.conn(),
            "SELECT r.publication_id, r.elapsed, r.watch, r.relative, r.read_at
             FROM retention_point r
             JOIN publication p ON p.id = r.publication_id
             JOIN video_project v ON v.id = p.project_id
             WHERE v.channel_id = ?1
             ORDER BY r.publication_id, r.elapsed",
            channel.to_string(),
        )
    }
}

/// Saves an upload's progress on the row of the same publication and
/// upload job; with `scheduled_at`, only while the row is still scheduled
/// to go public then. Returns whether the row changed.
fn update_upload(
    conn: &Connection,
    publication: &Publication,
    scheduled_at: Option<SystemTime>,
) -> Result<bool, RepositoryError> {
    let Some(upload) = publication.upload() else {
        return Ok(false);
    };
    let changed = conn
        .execute(
            &format!(
                "UPDATE publication SET
                     post_id = ?3,
                     url = ?4,
                     upload_status = ?5,
                     upload_failure = ?6,
                     upload_visibility = ?8,
                     upload_publish_at = ?9,
                     upload_issue = ?11,
                     upload_network_id = ?12,
                     posted_at = CASE WHEN ?5 IN {SETS_POSTED_AT}
                                           AND upload_status NOT IN {LIVE}
                                      THEN ?7 ELSE posted_at END
                 WHERE id = ?1 AND upload_job = ?2
                   AND (?10 IS NULL
                        OR (upload_status = 'scheduled' AND upload_publish_at = ?10))"
            ),
            params![
                publication.id.to_string(),
                upload.job.to_string(),
                publication.link.as_ref().map(PostLink::post_id),
                publication.link.as_ref().map(PostLink::url),
                upload.status.code(),
                upload.status.failure().map(|failure| failure.code()),
                to_unix_millis(publication.posted_at),
                upload.visibility.code(),
                upload.publish_at.map(to_unix_millis),
                scheduled_at.map(to_unix_millis),
                upload.issue.as_deref(),
                upload.network_id.as_deref(),
            ],
        )
        .map_err(boxed)?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Niche, ProfileRepository, Theme,
        ThemeIdea, ThemeRepository, UiLanguage, UserProfile, VideoProject,
    };

    use super::*;

    fn time(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    fn channel(db: &Database, profile: &UserProfile) -> Channel {
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Space Archives".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(profile.id, details);
        ChannelRepository::save(db, &channel).unwrap();
        channel
    }

    fn project(db: &Database, channel: &Channel, title: &str) -> VideoProject {
        let mut theme = Theme::suggested(
            channel.owner,
            channel.id,
            Niche::new("space history").unwrap(),
            ThemeIdea::new(title, "").unwrap(),
            time(0),
            0,
            None,
        );
        db.save_themes(std::slice::from_ref(&theme)).unwrap();
        let project = theme.approve(time(1)).unwrap();
        db.start_project(&theme, &project).unwrap();
        project
    }

    fn setup() -> (Database, Channel, VideoProject) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        let channel = channel(&db, &profile);
        let project = project(&db, &channel, "The lost probe");
        (db, channel, project)
    }

    fn publication(project: &VideoProject, network: Network, link: &str) -> Publication {
        Publication {
            id: PublicationId::new(),
            owner: project.owner,
            project: project.id,
            account: NetworkAccountId::new(),
            network,
            render: RenderId::new(),
            link: Some(PostLink::parse(network, link).unwrap()),
            kind: PublicationKind::Manual,
            posted_at: time(100),
            linked_at: time(100),
            checked_at: None,
            missing_since: None,
            insights_id: None,
        }
    }

    fn youtube(project: &VideoProject, id: &str) -> Publication {
        publication(project, Network::YouTube, &format!("https://youtu.be/{id}"))
    }

    fn snapshot(publication: &Publication, secs: u64, views: u64) -> MetricsSnapshot {
        MetricsSnapshot {
            publication: publication.id,
            taken_at: time(secs),
            views,
            likes: Some(views / 10),
            comments: None,
            owner: None,
            insights: bardo_domain::Insights::default(),
        }
    }

    #[test]
    fn publications_round_trip_per_project_in_network_order() {
        let (db, _, project) = setup();
        assert_eq!(db.publications(project.id).unwrap(), []);
        let tiktok = publication(
            &project,
            Network::TikTok,
            "https://www.tiktok.com/@a/video/7301234567890123456",
        );
        let mut yt = youtube(&project, "dQw4w9WgXcQ");
        yt.checked_at = Some(time(200));
        yt.missing_since = Some(time(200));
        db.save_publication(&tiktok).unwrap();
        db.save_publication(&yt).unwrap();
        assert_eq!(
            db.publications(project.id).unwrap(),
            [yt.clone(), tiktok.clone()]
        );
        assert_eq!(db.publication(yt.id).unwrap(), Some(yt));
        assert_eq!(db.publication(PublicationId::new()).unwrap(), None);
    }

    fn uploading(project: &VideoProject) -> Publication {
        Publication {
            link: None,
            kind: PublicationKind::Uploaded(Upload::queued(Visibility::Unlisted, JobId::new())),
            ..youtube(project, "dQw4w9WgXcQ")
        }
    }

    #[test]
    fn uploads_round_trip_through_every_status() {
        let (db, _, project) = setup();
        let mut upload = uploading(&project);
        db.save_publication(&upload).unwrap();
        assert_eq!(db.publication(upload.id).unwrap(), Some(upload.clone()));

        upload.upload_mut().unwrap().start().unwrap();
        db.save_publication(&upload).unwrap();
        upload
            .sent(Some(
                PostLink::parse(Network::YouTube, "https://youtu.be/Xb7kQ2mN9pA").unwrap(),
            ))
            .unwrap();
        db.save_publication(&upload).unwrap();
        assert_eq!(db.publication(upload.id).unwrap(), Some(upload.clone()));

        upload
            .processed(Visibility::Private, None, time(400))
            .unwrap();
        db.save_publication(&upload).unwrap();
        let read = db.publication(upload.id).unwrap().unwrap();
        assert_eq!(read.upload().unwrap().status, UploadStatus::Restricted);
        assert_eq!(read.posted_at, time(400), "went live when processed");
        assert_eq!(read, upload);

        let mut failed = uploading(&project);
        failed.upload_mut().unwrap().start().unwrap();
        failed
            .upload_mut()
            .unwrap()
            .fail(bardo_domain::UploadFailure::Rejected("duplicate".into()))
            .unwrap();
        db.save_publication(&failed).unwrap();
        assert_eq!(db.publications(project.id).unwrap(), [failed], "replaced");
    }

    /// A Reel scheduled in the app for `time(1000)`, saved.
    fn due_reel(db: &Database, project: &VideoProject) -> Publication {
        let reel = Publication {
            network: Network::InstagramReels,
            link: None,
            kind: PublicationKind::Uploaded(Upload::scheduled(time(1000), JobId::new())),
            ..youtube(project, "dQw4w9WgXcQ")
        };
        db.save_publication(&reel).unwrap();
        reel
    }

    fn claimed_at(db: &Database, reel: &Publication) -> Option<SystemTime> {
        db.publication(reel.id)
            .unwrap()
            .unwrap()
            .upload()
            .unwrap()
            .claimed_at
    }

    #[test]
    fn a_due_upload_is_claimed_once_and_keeps_its_first_claim() {
        let (db, _, project) = setup();
        let reel = due_reel(&db, &project);
        assert!(!db.claim_upload(&reel, time(999)).unwrap(), "not due yet");
        assert_eq!(claimed_at(&db, &reel), None);

        assert!(db.claim_upload(&reel, time(1000)).unwrap());
        assert_eq!(claimed_at(&db, &reel), Some(time(1000)));
        assert!(
            db.claim_upload(&reel, time(1300)).unwrap(),
            "its own job resumes"
        );
        assert_eq!(claimed_at(&db, &reel), Some(time(1000)), "the first claim");

        // A run of another job never takes it.
        let mut other = reel.clone();
        other.upload_mut().unwrap().job = JobId::new();
        assert!(!db.claim_upload(&other, time(1300)).unwrap());

        // Progress saved by the run keeps the claim.
        let mut read = db.publication(reel.id).unwrap().unwrap();
        read.upload_mut().unwrap().start().unwrap();
        let mut stale = read.clone();
        stale.upload_mut().unwrap().claimed_at = None;
        assert!(db.save_upload(&stale).unwrap());
        assert_eq!(claimed_at(&db, &reel), Some(time(1000)));
    }

    #[test]
    fn a_missed_upload_is_not_claimed_and_a_new_time_clears_the_claim() {
        let (db, _, project) = setup();
        let reel = due_reel(&db, &project);
        assert!(db.claim_upload(&reel, time(1000)).unwrap());
        let mut missed = db.publication(reel.id).unwrap().unwrap();
        missed.upload_mut().unwrap().miss().unwrap();
        assert!(db.save_upload(&missed).unwrap());
        assert!(
            !db.claim_upload(&missed, time(2000)).unwrap(),
            "it waits for the user"
        );
        assert!(db.publication(reel.id).unwrap().unwrap().is_missed());

        missed.upload_mut().unwrap().due_again(time(5000)).unwrap();
        db.save_publication(&missed).unwrap();
        let read = db.publication(reel.id).unwrap().unwrap();
        assert_eq!(read.due(), Some(time(5000)));
        assert_eq!(read.upload().unwrap().claimed_at, None);
        assert_eq!(read, missed);

        // Sent now, it has no due time, and nothing claims it.
        let mut now = read;
        now.upload_mut().unwrap().miss().unwrap();
        now.upload_mut().unwrap().send_now().unwrap();
        db.save_publication(&now).unwrap();
        assert!(!db.claim_upload(&now, time(9000)).unwrap());
    }

    #[test]
    fn an_upload_that_failed_on_the_way_is_claimed_when_tried_again() {
        let (db, _, project) = setup();
        let mut reel = due_reel(&db, &project);
        reel.upload_mut()
            .unwrap()
            .fail(bardo_domain::UploadFailure::ReconnectNeeded)
            .unwrap();
        assert!(db.save_upload(&reel).unwrap());
        assert!(db.claim_upload(&reel, time(1000)).unwrap());
    }

    #[test]
    fn only_an_upload_with_a_due_time_holds_a_claim() {
        let (db, _, project) = setup();
        let reel = due_reel(&db, &project);
        let refused = db.conn().execute(
            "UPDATE publication SET upload_publish_at = NULL, upload_claimed_at = 5
             WHERE id = ?1",
            [reel.id.to_string()],
        );
        assert!(refused.is_err());
    }

    #[test]
    fn a_published_reel_keeps_its_link_and_what_instagram_left_out() {
        let (db, _, project) = setup();
        let mut reel = Publication {
            network: Network::InstagramReels,
            ..uploading(&project)
        };
        db.save_publication(&reel).unwrap();
        reel.upload_mut().unwrap().start().unwrap();
        reel.sent(None).unwrap();
        assert!(db.save_upload(&reel).unwrap());
        assert_eq!(db.publication(reel.id).unwrap().unwrap().link, None);

        reel.published(
            bardo_domain::NetworkPost {
                id: "17900000000000001".into(),
                link: Some(
                    PostLink::parse(
                        Network::InstagramReels,
                        "https://www.instagram.com/reel/C9xYz12AbCd/",
                    )
                    .unwrap(),
                ),
                issue: Some("User tags could not be added".into()),
            },
            time(500),
        )
        .unwrap();
        assert!(db.save_upload(&reel).unwrap());
        let read = db.publication(reel.id).unwrap().unwrap();
        assert_eq!(read, reel);
        assert_eq!(
            read.upload().unwrap().issue.as_deref(),
            Some("User tags could not be added")
        );
        assert_eq!(read.posted_at, time(500));
        assert_eq!(read.post_id(), Some("C9xYz12AbCd"));
    }

    #[test]
    fn an_upload_saves_its_progress_only_while_it_is_the_publication() {
        let (db, _, project) = setup();
        let mut upload = uploading(&project);
        db.save_publication(&upload).unwrap();
        upload.upload_mut().unwrap().start().unwrap();
        upload
            .sent(Some(
                PostLink::parse(Network::YouTube, "https://youtu.be/Xb7kQ2mN9pA").unwrap(),
            ))
            .unwrap();
        assert!(db.save_upload(&upload).unwrap());
        assert_eq!(db.publication(upload.id).unwrap(), Some(upload.clone()));
        upload
            .processed(Visibility::Unlisted, None, time(400))
            .unwrap();
        assert!(db.save_upload(&upload).unwrap());
        let read = db.publication(upload.id).unwrap().unwrap();
        assert_eq!(read.posted_at, time(400), "went live when processed");
        assert_eq!(read, upload);

        // The user linked a post meanwhile: the upload's run no longer
        // touches it.
        let manual = youtube(&project, "dQw4w9WgXcQ");
        db.save_publication(&manual).unwrap();
        assert!(!db.save_upload(&upload).unwrap());
        assert_eq!(db.publications(project.id).unwrap(), [manual]);

        // Neither does a run of another upload job on the same row.
        let mut other = uploading(&project);
        db.save_publication(&other).unwrap();
        let stale = Publication {
            kind: PublicationKind::Uploaded(Upload::queued(Visibility::Public, JobId::new())),
            ..other.clone()
        };
        assert!(!db.save_upload(&stale).unwrap());
        other.upload_mut().unwrap().start().unwrap();
        assert!(db.save_upload(&other).unwrap());
        assert_eq!(db.publications(project.id).unwrap(), [other]);
    }

    #[test]
    fn a_schedule_read_saves_only_over_the_schedule_it_started_from() {
        let (db, _, project) = setup();
        let mut upload = Publication {
            kind: PublicationKind::Uploaded(Upload::scheduled(time(5_000), JobId::new())),
            ..uploading(&project)
        };
        upload.upload_mut().unwrap().start().unwrap();
        upload
            .sent(Some(
                PostLink::parse(Network::YouTube, "https://youtu.be/Xb7kQ2mN9pA").unwrap(),
            ))
            .unwrap();
        upload
            .processed(Visibility::Private, Some(time(5_000)), time(400))
            .unwrap();
        db.save_publication(&upload).unwrap();

        // The user moved it to 6,000 while a sync read 5,000 and found it
        // live: the sync's word does not overwrite the change.
        let mut changed = upload.clone();
        changed.rescheduled(time(6_000)).unwrap();
        assert!(db.save_schedule(&changed, time(5_000)).unwrap());
        let mut stale = upload.clone();
        stale.schedule_seen(
            bardo_domain::ScheduleReading::Live { published_at: None },
            time(5_100),
        );
        assert!(!db.save_schedule(&stale, time(5_000)).unwrap());
        assert_eq!(db.publication(upload.id).unwrap(), Some(changed.clone()));

        // A read from the schedule as it stands is saved.
        let mut live = changed.clone();
        live.schedule_seen(
            bardo_domain::ScheduleReading::Live {
                published_at: Some(time(6_001)),
            },
            time(6_100),
        );
        assert!(db.save_schedule(&live, time(6_000)).unwrap());
        let read = db.publication(upload.id).unwrap().unwrap();
        assert_eq!(read.upload().unwrap().status, UploadStatus::Published);
        assert_eq!(read.posted_at, time(6_001));
        assert!(
            !db.save_schedule(&live, time(6_000)).unwrap(),
            "not scheduled any more"
        );
    }

    #[test]
    fn a_scheduled_upload_keeps_its_publish_time_until_it_goes_live() {
        let (db, _, project) = setup();
        let mut upload = Publication {
            kind: PublicationKind::Uploaded(Upload::scheduled(time(5_000), JobId::new())),
            ..uploading(&project)
        };
        db.save_publication(&upload).unwrap();
        assert_eq!(db.publication(upload.id).unwrap(), Some(upload.clone()));
        upload.upload_mut().unwrap().start().unwrap();
        upload
            .sent(Some(
                PostLink::parse(Network::YouTube, "https://youtu.be/Xb7kQ2mN9pA").unwrap(),
            ))
            .unwrap();
        upload
            .processed(Visibility::Private, Some(time(5_000)), time(400))
            .unwrap();
        assert!(db.save_upload(&upload).unwrap());
        let read = db.publication(upload.id).unwrap().unwrap();
        assert_eq!(read.upload().unwrap().status, UploadStatus::Scheduled);
        assert_eq!(read.posted_at, time(5_000), "goes live at its time");
        assert_eq!(read, upload);

        upload.rescheduled(time(6_000)).unwrap();
        assert!(db.save_upload(&upload).unwrap());
        assert_eq!(db.publication(upload.id).unwrap(), Some(upload.clone()));

        upload.schedule_seen(
            bardo_domain::ScheduleReading::Live {
                published_at: Some(time(6_002)),
            },
            time(7_000),
        );
        assert!(db.save_upload(&upload).unwrap());
        db.save_sync(std::slice::from_ref(&upload), &[]).unwrap();
        let read = db.publication(upload.id).unwrap().unwrap();
        assert_eq!(read.posted_at, time(6_002), "YouTube's publish time");
        assert_eq!(read.checked_at, Some(time(7_000)));
        assert_eq!(read, upload);

        let mut cancelled = Publication {
            kind: PublicationKind::Uploaded(Upload::scheduled(time(5_000), JobId::new())),
            ..uploading(&project)
        };
        cancelled.upload_mut().unwrap().start().unwrap();
        cancelled
            .sent(Some(
                PostLink::parse(Network::YouTube, "https://youtu.be/Xb7kQ2mN9pB").unwrap(),
            ))
            .unwrap();
        cancelled
            .processed(Visibility::Private, Some(time(5_000)), time(400))
            .unwrap();
        db.save_publication(&cancelled).unwrap();
        cancelled.unscheduled().unwrap();
        assert!(db.save_upload(&cancelled).unwrap());
        let read = db.publication(cancelled.id).unwrap().unwrap();
        assert_eq!(read.upload().unwrap().visibility, Visibility::Private);
        assert_eq!(read.upload().unwrap().publish_at, None);
    }

    #[test]
    fn a_tiktok_draft_round_trips_with_no_post_and_when_it_reached_the_inbox() {
        let (db, _, project) = setup();
        let mut draft = Publication {
            network: Network::TikTok,
            link: None,
            kind: PublicationKind::Uploaded(Upload::queued(Visibility::Private, JobId::new())),
            ..uploading(&project)
        };
        db.save_publication(&draft).unwrap();
        draft.upload_mut().unwrap().start().unwrap();
        draft.sent(None).unwrap();
        assert!(db.save_upload(&draft).unwrap());
        draft.drafted(time(900)).unwrap();
        assert!(db.save_upload(&draft).unwrap());
        let read = db.publication(draft.id).unwrap().unwrap();
        assert_eq!(read.upload().unwrap().status, UploadStatus::DraftSent);
        assert_eq!(read.link, None);
        assert_eq!(read.posted_at, time(900), "reached the inbox");
        assert_eq!(read, draft);
    }

    #[test]
    fn an_upload_replaces_a_linked_post_and_its_snapshots() {
        let (db, _, project) = setup();
        let manual = youtube(&project, "dQw4w9WgXcQ");
        db.save_publication(&manual).unwrap();
        db.save_sync(&[], &[snapshot(&manual, 200, 50)]).unwrap();

        let upload = uploading(&project);
        db.save_publication(&upload).unwrap();
        assert_eq!(db.publications(project.id).unwrap(), [upload]);
        assert_eq!(db.snapshots(manual.id).unwrap(), []);
    }

    #[test]
    fn linking_another_post_replaces_the_publication_and_its_snapshots() {
        let (db, channel, project) = setup();
        let first = youtube(&project, "dQw4w9WgXcQ");
        db.save_publication(&first).unwrap();
        db.save_sync(&[], &[snapshot(&first, 200, 50)]).unwrap();

        let second = youtube(&project, "aaaaaaaaaaa");
        db.save_publication(&second).unwrap();
        assert_eq!(db.publications(project.id).unwrap(), [second]);
        assert_eq!(db.snapshots(first.id).unwrap(), []);
        assert_eq!(db.channel_snapshots(channel.id).unwrap(), []);
    }

    #[test]
    fn a_post_links_to_one_project_only() {
        let (db, channel, project) = setup();
        let other = super::tests::project(&db, &channel, "Another");
        db.save_publication(&youtube(&project, "dQw4w9WgXcQ"))
            .unwrap();
        assert!(
            db.save_publication(&youtube(&other, "dQw4w9WgXcQ"))
                .is_err()
        );
    }

    #[test]
    fn a_sync_saves_checks_and_snapshots_and_skips_removed_publications() {
        let (db, channel, project) = setup();
        let mut yt = youtube(&project, "dQw4w9WgXcQ");
        db.save_publication(&yt).unwrap();
        let gone = youtube(&super::tests::project(&db, &channel, "Gone"), "bbbbbbbbbbb");
        db.save_publication(&gone).unwrap();
        db.remove_publication(gone.id).unwrap();

        yt.checked_at = Some(time(300));
        yt.posted_at = time(50);
        db.save_sync(
            &[yt.clone(), gone.clone()],
            &[
                snapshot(&yt, 300, 120),
                snapshot(&yt, 200, 80),
                snapshot(&gone, 300, 9),
            ],
        )
        .unwrap();
        assert_eq!(db.publication(yt.id).unwrap(), Some(yt.clone()));
        assert_eq!(db.publication(gone.id).unwrap(), None);
        assert_eq!(
            db.snapshots(yt.id).unwrap(),
            [snapshot(&yt, 200, 80), snapshot(&yt, 300, 120)],
            "oldest first"
        );
        assert_eq!(db.snapshots(gone.id).unwrap(), []);

        db.remove_publication(yt.id).unwrap();
        assert_eq!(db.snapshots(yt.id).unwrap(), []);
    }

    #[test]
    fn insights_ride_on_the_snapshot_and_keep_empty_apart_from_zero() {
        let (db, _, project) = setup();
        let reel = publication(
            &project,
            Network::InstagramReels,
            "https://www.instagram.com/reel/C9xYz12AbCd/",
        );
        db.save_publication(&reel).unwrap();
        let read = MetricsSnapshot {
            likes: Some(0),
            comments: None,
            insights: Insights {
                shares: Some(0),
                saves: Some(12),
                reach: Some(3_400),
                interactions: None,
                average_watch: Some(Duration::from_millis(5_250)),
                watch_time: Some(Duration::from_millis(9_876_543)),
            },
            ..snapshot(&reel, 100, 4_100)
        };
        db.save_sync(std::slice::from_ref(&reel), &[read]).unwrap();
        assert_eq!(db.snapshots(reel.id).unwrap(), [read]);

        // A YouTube snapshot has none of them.
        let yt = youtube(&project, "dQw4w9WgXcQ");
        db.save_publication(&yt).unwrap();
        db.save_sync(std::slice::from_ref(&yt), &[snapshot(&yt, 100, 7)])
            .unwrap();
        assert!(db.snapshots(yt.id).unwrap()[0].insights.is_empty());
    }

    #[test]
    fn a_linked_reels_media_id_is_kept_once_a_sync_found_it() {
        let (db, _, project) = setup();
        let mut reel = publication(
            &project,
            Network::InstagramReels,
            "https://www.instagram.com/reel/C9xYz12AbCd/",
        );
        db.save_publication(&reel).unwrap();
        assert_eq!(db.publication(reel.id).unwrap().unwrap().insights_id, None);

        reel.insights_id = Some("17900000000000001".into());
        reel.checked_at = Some(time(60));
        db.save_sync(std::slice::from_ref(&reel), &[]).unwrap();
        assert_eq!(db.publication(reel.id).unwrap(), Some(reel.clone()));

        // A later save that does not know it (the link saved again) and a
        // sync that did not look it up keep it.
        let stale = Publication {
            insights_id: None,
            ..reel.clone()
        };
        db.save_publication(&stale).unwrap();
        db.save_sync(std::slice::from_ref(&stale), &[]).unwrap();
        assert_eq!(
            db.publication(reel.id)
                .unwrap()
                .unwrap()
                .insights_id
                .as_deref(),
            Some("17900000000000001")
        );
    }

    fn owner(engaged: u64, money: Option<MoneyReport>) -> OwnerMetrics {
        OwnerMetrics {
            views: engaged + 100,
            engaged_views: engaged,
            minutes_watched: engaged / 3,
            average_view_seconds: 42,
            average_view_share: Share::from_ten_thousandths(6_123),
            earnings: money.map_or(Earnings::NotMonetized, Earnings::Monetized),
        }
    }

    #[test]
    fn owner_numbers_ride_on_the_snapshot_monetized_or_not() {
        let (db, channel, project) = setup();
        let yt = youtube(&project, "dQw4w9WgXcQ");
        db.save_publication(&yt).unwrap();
        let money = MoneyReport {
            revenue: Money::from_micros(61_873_000),
            cpm: Money::from_micros(7_412_000),
            playback_cpm: Money::from_micros(5_880_000),
        };
        let public = snapshot(&yt, 100, 50);
        let paid = MetricsSnapshot {
            owner: Some(owner(900, Some(money))),
            ..snapshot(&yt, 200, 1_000)
        };
        let unpaid = MetricsSnapshot {
            owner: Some(owner(950, None)),
            ..snapshot(&yt, 300, 1_100)
        };
        db.save_sync(&[], &[public, paid, unpaid]).unwrap();
        assert_eq!(db.snapshots(yt.id).unwrap(), [public, paid, unpaid]);
        assert_eq!(
            db.channel_snapshots(channel.id).unwrap(),
            [public, paid, unpaid]
        );

        // A resumed sync writes the same moment again, owner numbers too.
        let again = MetricsSnapshot {
            owner: None,
            ..paid
        };
        db.save_sync(&[], &[again]).unwrap();
        assert_eq!(db.snapshots(yt.id).unwrap()[1], again);
    }

    #[test]
    fn half_an_owner_row_is_refused() {
        let (db, _, project) = setup();
        let yt = youtube(&project, "dQw4w9WgXcQ");
        db.save_publication(&yt).unwrap();
        let conn = db.conn();
        let insert = |columns: &str, values: &str| {
            conn.execute(
                &format!(
                    "INSERT INTO metrics_snapshot (publication_id, taken_at, views{columns})
                     VALUES ('{}', 1, 5{values})",
                    yt.id
                ),
                [],
            )
        };
        assert!(
            insert(", engaged_views", ", 3").is_err(),
            "no monetized flag"
        );
        assert!(
            insert(
                ", owner_views, engaged_views, minutes_watched, average_view_seconds, \
                 average_view_share, monetized",
                ", 1, 1, 1, 1, 1, 1"
            )
            .is_err(),
            "monetized without money"
        );
        assert!(
            insert(
                ", owner_views, engaged_views, minutes_watched, average_view_seconds, \
                 average_view_share, monetized",
                ", 1, 1, 1, 1, 1, 0"
            )
            .is_ok()
        );
    }

    fn curve(watch: &[u32]) -> RetentionCurve {
        RetentionCurve::of(
            watch
                .iter()
                .enumerate()
                .map(|(ix, watch)| RetentionPoint {
                    elapsed: Share::from_ten_thousandths((ix as u32 + 1) * 100),
                    watch: Share::from_ten_thousandths(*watch),
                    relative: (ix % 2 == 0).then_some(Share::from_ten_thousandths(5_000)),
                })
                .collect(),
        )
    }

    #[test]
    fn a_retention_curve_is_replaced_by_the_next_one_and_goes_with_its_post() {
        let (db, channel, project) = setup();
        let yt = youtube(&project, "dQw4w9WgXcQ");
        db.save_publication(&yt).unwrap();
        let second = youtube(
            &super::tests::project(&db, &channel, "Second"),
            "aaaaaaaaaaa",
        );
        db.save_publication(&second).unwrap();
        assert_eq!(db.retention(yt.id).unwrap(), None);

        let first = PostRetention {
            publication: yt.id,
            read_at: time(100),
            curve: curve(&[12_000, 9_000, 8_000]),
        };
        db.save_retention(std::slice::from_ref(&first)).unwrap();
        assert_eq!(db.retention(yt.id).unwrap(), Some(first));

        let next = PostRetention {
            publication: yt.id,
            read_at: time(200),
            curve: curve(&[11_000, 7_000]),
        };
        let other = PostRetention {
            publication: second.id,
            read_at: time(200),
            curve: curve(&[10_000]),
        };
        db.save_retention(&[next.clone(), other.clone()]).unwrap();
        assert_eq!(db.retention(yt.id).unwrap(), Some(next.clone()));
        let mut both = db.channel_retention(channel.id).unwrap();
        both.sort_by_key(|r| r.curve.points().len());
        assert_eq!(both, [other, next]);

        db.remove_publication(yt.id).unwrap();
        assert_eq!(db.retention(yt.id).unwrap(), None);
        db.save_retention(&[PostRetention {
            publication: yt.id,
            read_at: time(300),
            curve: curve(&[1]),
        }])
        .unwrap();
        assert_eq!(db.retention(yt.id).unwrap(), None, "removed meanwhile");
    }

    #[test]
    fn channel_queries_cover_every_project_of_the_channel_only() {
        let (db, channel, project) = setup();
        let second = super::tests::project(&db, &channel, "Second");
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        let elsewhere = super::tests::project(&db, &super::tests::channel(&db, &profile), "Else");

        let mut older = youtube(&project, "dQw4w9WgXcQ");
        older.posted_at = time(10);
        let newer = youtube(&second, "aaaaaaaaaaa");
        let foreign = youtube(&elsewhere, "ccccccccccc");
        for p in [&older, &newer, &foreign] {
            db.save_publication(p).unwrap();
        }
        db.save_sync(
            &[],
            &[
                snapshot(&newer, 400, 7),
                snapshot(&older, 300, 5),
                snapshot(&foreign, 300, 1),
            ],
        )
        .unwrap();

        assert_eq!(
            db.channel_publications(channel.id).unwrap(),
            [newer.clone(), older.clone()],
            "newest post first"
        );
        assert_eq!(
            db.channel_snapshots(channel.id).unwrap(),
            [snapshot(&older, 300, 5), snapshot(&newer, 400, 7)]
        );
        assert_eq!(db.all_publications(project.owner).unwrap().len(), 2);
        assert_eq!(db.all_publications(profile.id).unwrap(), [foreign]);
    }
}
