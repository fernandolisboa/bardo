use bardo_domain::{
    ChannelId, JobId, MetricsSnapshot, Network, NetworkAccountId, PostLink, ProfileId, Publication,
    PublicationId, PublicationKind, PublicationRepository, RenderId, RepositoryError, Upload,
    UploadStatus, VideoProjectId, Visibility,
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
     publication.upload_job";

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
                job: JobId::from(uuid(
                    self.upload_job
                        .as_deref()
                        .ok_or_else(|| broken("no upload job"))?,
                )?),
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

fn query_snapshots(
    conn: &Connection,
    sql: &str,
    param: String,
) -> Result<Vec<MetricsSnapshot>, RepositoryError> {
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
                        row.get::<_, Option<i64>>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(boxed)?;
    let count = |n: i64| u64::try_from(n).unwrap_or(0);
    rows.into_iter()
        .map(|(publication, taken_at, views, likes, comments)| {
            Ok(MetricsSnapshot {
                publication: PublicationId::from(uuid(&publication)?),
                taken_at: from_unix_millis(taken_at),
                views: count(views),
                likes: likes.map(count),
                comments: comments.map(count),
            })
        })
        .collect()
}

/// Counts beyond SQLite's integers are not real view counts; they clamp.
fn stored(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Saves a publication. One saved before keeps its dates and what syncs
/// found (they belong to `save_sync`); its account, render, post and
/// upload change. An upload that went live sets when it did.
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
        "INSERT INTO publication (id, project_id, network, profile_id, account_id, render_id,
                                  post_id, url, posted_at, linked_at, checked_at, missing_since,
                                  kind, upload_status, upload_failure, upload_visibility,
                                  upload_job)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
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
             posted_at = CASE WHEN excluded.upload_status IN ('published', 'restricted')
                                   AND publication.upload_status
                                       NOT IN ('published', 'restricted')
                              THEN excluded.posted_at ELSE publication.posted_at END",
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
                "UPDATE publication SET posted_at = ?2, checked_at = ?3, missing_since = ?4
                 WHERE id = ?1",
                params![
                    publication.id.to_string(),
                    to_unix_millis(publication.posted_at),
                    publication.checked_at.map(to_unix_millis),
                    publication.missing_since.map(to_unix_millis),
                ],
            )
            .map_err(boxed)?;
        }
        for snapshot in snapshots {
            tx.execute(
                "INSERT INTO metrics_snapshot (publication_id, taken_at, views, likes, comments)
                 SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM publication WHERE id = ?1)
                 ON CONFLICT (publication_id, taken_at) DO UPDATE SET
                     views = excluded.views,
                     likes = excluded.likes,
                     comments = excluded.comments",
                params![
                    snapshot.publication.to_string(),
                    to_unix_millis(snapshot.taken_at),
                    stored(snapshot.views),
                    snapshot.likes.map(stored),
                    snapshot.comments.map(stored),
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
            "SELECT publication_id, taken_at, views, likes, comments FROM metrics_snapshot
             WHERE publication_id = ?1 ORDER BY taken_at",
            publication.to_string(),
        )
    }

    fn channel_snapshots(
        &self,
        channel: ChannelId,
    ) -> Result<Vec<MetricsSnapshot>, RepositoryError> {
        query_snapshots(
            &self.conn(),
            "SELECT s.publication_id, s.taken_at, s.views, s.likes, s.comments
             FROM metrics_snapshot s
             JOIN publication p ON p.id = s.publication_id
             JOIN video_project v ON v.id = p.project_id
             WHERE v.channel_id = ?1
             ORDER BY s.taken_at",
            channel.to_string(),
        )
    }
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
            .sent(PostLink::parse(Network::YouTube, "https://youtu.be/Xb7kQ2mN9pA").unwrap())
            .unwrap();
        db.save_publication(&upload).unwrap();
        assert_eq!(db.publication(upload.id).unwrap(), Some(upload.clone()));

        upload.processed(Visibility::Private, time(400)).unwrap();
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
