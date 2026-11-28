//! Bridges over a cut (#363), waiting on their cut for the user's review.

use std::time::Duration;

use bardo_domain::{
    Bridge, BridgeCut, BridgeId, BridgeRepository, FrameSource, JobId, ProfileId, RepositoryError,
    VideoProjectId,
};
use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const COLUMNS: &str = "id, project_id, profile_id, before_key, after_key, from_frame, to_frame,
    first_file, first_at_ns, last_file, last_at_ns, provider, model, seconds, note, job_id,
    file, asked_at";

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

fn nanos(duration: Duration) -> i64 {
    i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX)
}

fn frame(file: String, at: Option<i64>) -> Result<FrameSource, RepositoryError> {
    Ok(match at {
        Some(at) => FrameSource::Video {
            file,
            at: Duration::from_nanos(u64::try_from(at).map_err(boxed)?),
        },
        None => FrameSource::Still { file },
    })
}

fn frame_at(source: &FrameSource) -> Option<i64> {
    match source {
        FrameSource::Video { at, .. } => Some(nanos(*at)),
        FrameSource::Still { .. } => None,
    }
}

/// A bridge row as stored, before domain validation.
struct BridgeRow {
    id: String,
    project: String,
    profile: String,
    before: String,
    after: String,
    from: i64,
    to: i64,
    first_file: String,
    first_at: Option<i64>,
    last_file: String,
    last_at: Option<i64>,
    provider: String,
    model: String,
    seconds: i64,
    note: String,
    job: String,
    file: Option<String>,
    asked_at: i64,
}

impl BridgeRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<BridgeRow> {
        Ok(BridgeRow {
            id: row.get(0)?,
            project: row.get(1)?,
            profile: row.get(2)?,
            before: row.get(3)?,
            after: row.get(4)?,
            from: row.get(5)?,
            to: row.get(6)?,
            first_file: row.get(7)?,
            first_at: row.get(8)?,
            last_file: row.get(9)?,
            last_at: row.get(10)?,
            provider: row.get(11)?,
            model: row.get(12)?,
            seconds: row.get(13)?,
            note: row.get(14)?,
            job: row.get(15)?,
            file: row.get(16)?,
            asked_at: row.get(17)?,
        })
    }

    fn into_bridge(self) -> Result<Bridge, RepositoryError> {
        Ok(Bridge {
            id: BridgeId::from(uuid(&self.id)?),
            project: VideoProjectId::from(uuid(&self.project)?),
            owner: ProfileId::from(uuid(&self.profile)?),
            cut: BridgeCut {
                before: self.before,
                after: self.after,
            },
            from: u64::try_from(self.from).map_err(boxed)?,
            to: u64::try_from(self.to).map_err(boxed)?,
            first: frame(self.first_file, self.first_at)?,
            last: frame(self.last_file, self.last_at)?,
            provider: self.provider.parse().map_err(boxed)?,
            model: self.model,
            seconds: u32::try_from(self.seconds).map_err(boxed)?,
            note: self.note,
            job: JobId::from(uuid(&self.job)?),
            file: self.file,
            asked_at: from_unix_millis(self.asked_at),
        })
    }
}

impl BridgeRepository for Database {
    fn bridges(&self, project: VideoProjectId) -> Result<Vec<Bridge>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM bridge WHERE project_id = ?1 ORDER BY asked_at, rowid"
            ))
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], BridgeRow::read)
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?;
        rows.into_iter().map(BridgeRow::into_bridge).collect()
    }

    fn bridge(&self, id: BridgeId) -> Result<Option<Bridge>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM bridge WHERE id = ?1"),
                [id.to_string()],
                BridgeRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(BridgeRow::into_bridge).transpose()
    }

    fn save_bridge(&self, bridge: &Bridge) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                &format!(
                    "INSERT INTO bridge ({COLUMNS})
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                             ?16, ?17, ?18)
                     ON CONFLICT (id) DO UPDATE SET file = excluded.file"
                ),
                params![
                    bridge.id.to_string(),
                    bridge.project.to_string(),
                    bridge.owner.to_string(),
                    bridge.cut.before,
                    bridge.cut.after,
                    i64::try_from(bridge.from).map_err(boxed)?,
                    i64::try_from(bridge.to).map_err(boxed)?,
                    bridge.first.file(),
                    frame_at(&bridge.first),
                    bridge.last.file(),
                    frame_at(&bridge.last),
                    bridge.provider.code(),
                    bridge.model,
                    i64::from(bridge.seconds),
                    bridge.note,
                    bridge.job.to_string(),
                    bridge.file,
                    to_unix_millis(bridge.asked_at),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn delete_bridge(&self, id: BridgeId) -> Result<(), RepositoryError> {
        self.conn()
            .execute("DELETE FROM bridge WHERE id = ?1", [id.to_string()])
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Niche, ProfileRepository,
        Provider, Theme, ThemeIdea, ThemeRepository, UiLanguage, UserProfile, VideoProject,
    };

    use super::*;

    fn time(millis: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
    }

    fn project(db: &Database, title: &str) -> VideoProject {
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(db, &profile).unwrap();
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Space Archives".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(
            profile.id,
            crate::project::test_project(db, profile.id),
            details,
        );
        ChannelRepository::save(db, &channel).unwrap();
        let mut theme = Theme::suggested(
            profile.id,
            channel.id,
            Niche::new("space history").unwrap(),
            ThemeIdea::new(title, "").unwrap(),
            time(1_800_000_000_000),
            0,
            None,
        );
        db.save_themes(std::slice::from_ref(&theme)).unwrap();
        let project = theme.approve(time(1_800_000_001_000)).unwrap();
        db.start_project(&theme, &project).unwrap();
        project
    }

    fn bridge(project: &VideoProject, at: u64) -> Bridge {
        Bridge {
            id: BridgeId::new(),
            project: project.id,
            owner: project.owner,
            cut: BridgeCut {
                before: "a.mp4|0|0|60".into(),
                after: "-|0|60|60".into(),
            },
            from: 45,
            to: 75,
            first: FrameSource::Video {
                file: "a.mp4".into(),
                at: Duration::from_nanos(1_466_666_666),
            },
            last: FrameSource::Still {
                file: "b.png".into(),
            },
            provider: Provider::Kling,
            model: "kling-v3".into(),
            seconds: 1,
            note: "a slow pan".into(),
            job: JobId::new(),
            file: None,
            asked_at: time(at),
        }
    }

    #[test]
    fn bridges_round_trip_oldest_first_and_keep_their_clip_once_made() {
        let db = Database::open_in_memory().unwrap();
        let first = project(&db, "The lost probe");
        let other = project(&db, "The far moon");
        let later = bridge(&first, 2_000);
        let mut earlier = bridge(&first, 1_000);
        db.save_bridge(&later).unwrap();
        db.save_bridge(&earlier).unwrap();
        db.save_bridge(&bridge(&other, 500)).unwrap();
        assert_eq!(db.bridges(first.id).unwrap(), vec![earlier.clone(), later]);
        earlier.file = Some(Bridge::file_name(earlier.id));
        db.save_bridge(&earlier).unwrap();
        assert_eq!(db.bridge(earlier.id).unwrap(), Some(earlier.clone()));
        db.delete_bridge(earlier.id).unwrap();
        assert_eq!(db.bridge(earlier.id).unwrap(), None);
        assert_eq!(db.bridges(first.id).unwrap().len(), 1);
    }
}
