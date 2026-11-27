//! Projects (ADR-0047), and the project, channel and video jobs and cost
//! records remember they were for.

use std::path::PathBuf;
use std::time::SystemTime;

use bardo_domain::{
    ChannelId, ProfileId, Project, ProjectColor, ProjectDetails, ProjectDraft, ProjectId,
    ProjectRepository, RepositoryError, WorkOrigin,
};
use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::folders::path_text;
use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const SELECT_PROJECT: &str = "SELECT id, profile_id, name, color, state, favorite_position, \
     opened_at, created_at, folder FROM project";

fn read(row: &Row<'_>) -> rusqlite::Result<ProjectRow> {
    Ok(ProjectRow {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        name: row.get(2)?,
        color: row.get(3)?,
        state: row.get(4)?,
        favorite: row.get(5)?,
        opened_at: row.get(6)?,
        created_at: row.get(7)?,
        folder: row.get(8)?,
    })
}

struct ProjectRow {
    id: String,
    profile_id: String,
    name: String,
    color: String,
    state: String,
    favorite: Option<i64>,
    opened_at: Option<i64>,
    created_at: i64,
    folder: Option<String>,
}

impl ProjectRow {
    /// Rebuilds the project through domain validation, so a row edited
    /// outside the app cannot load an invalid name.
    fn into_project(self) -> Result<Project, RepositoryError> {
        let details = ProjectDetails::validate(ProjectDraft {
            name: self.name,
            color: ProjectColor::from_code(&self.color),
        })
        .map_err(|error| boxed(InvalidRow(format!("{error:?}"))))?;
        Ok(Project {
            id: ProjectId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            owner: ProfileId::from(Uuid::parse_str(&self.profile_id).map_err(boxed)?),
            details,
            state: self.state.parse().map_err(boxed)?,
            favorite: self
                .favorite
                .map(u32::try_from)
                .transpose()
                .map_err(boxed)?,
            opened_at: self.opened_at.map(from_unix_millis),
            created_at: from_unix_millis(self.created_at),
            folder: self.folder.map(PathBuf::from),
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("stored project is invalid: {0}")]
struct InvalidRow(String);

impl ProjectRepository for Database {
    fn projects(&self, owner: ProfileId) -> Result<Vec<Project>, RepositoryError> {
        let rows = self
            .conn()
            .prepare(&format!(
                "{SELECT_PROJECT} WHERE profile_id = ?1 ORDER BY name COLLATE NOCASE, rowid"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter().map(ProjectRow::into_project).collect()
    }

    fn project(&self, id: ProjectId) -> Result<Option<Project>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                &format!("{SELECT_PROJECT} WHERE id = ?1"),
                [id.to_string()],
                read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(ProjectRow::into_project).transpose()
    }

    fn save_project(&self, project: &Project) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO project (id, profile_id, name, color, state, created_at, folder)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (id) DO UPDATE SET
                     name = excluded.name,
                     color = excluded.color,
                     state = excluded.state",
                params![
                    project.id.to_string(),
                    project.owner.to_string(),
                    project.details.name(),
                    project.details.color().code(),
                    project.state.code(),
                    to_unix_millis(project.created_at),
                    project.folder.as_deref().map(path_text),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn mark_opened(&self, id: ProjectId, at: SystemTime) -> Result<(), RepositoryError> {
        // Later than any project opened before, even within the same
        // millisecond, so the last one opened is always the open one.
        self.conn()
            .execute(
                "UPDATE project SET opened_at = MAX(?2,
                     (SELECT COALESCE(MAX(other.opened_at), 0) + 1 FROM project other
                      WHERE other.profile_id = project.profile_id),
                     (SELECT COALESCE(u.loose_opened_at, 0) + 1 FROM user_profile u
                      WHERE u.id = project.profile_id))
                 WHERE id = ?1",
                params![id.to_string(), to_unix_millis(at)],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn set_favorites(&self, owner: ProfileId, order: &[ProjectId]) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        tx.execute(
            "UPDATE project SET favorite_position = NULL WHERE profile_id = ?1",
            [owner.to_string()],
        )
        .map_err(boxed)?;
        for (position, id) in order.iter().enumerate() {
            tx.execute(
                "UPDATE project SET favorite_position = ?3 WHERE id = ?1 AND profile_id = ?2",
                params![id.to_string(), owner.to_string(), position as i64],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }

    fn loose_opened(&self, owner: ProfileId) -> Result<Option<SystemTime>, RepositoryError> {
        let at: Option<Option<i64>> = self
            .conn()
            .query_row(
                "SELECT loose_opened_at FROM user_profile WHERE id = ?1",
                [owner.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(boxed)?;
        Ok(at.flatten().map(from_unix_millis))
    }

    fn mark_loose_opened(&self, owner: ProfileId, at: SystemTime) -> Result<(), RepositoryError> {
        // Later than any project opened before, as `mark_opened` is.
        self.conn()
            .execute(
                "UPDATE user_profile SET loose_opened_at = MAX(?2,
                     (SELECT COALESCE(MAX(p.opened_at), 0) + 1 FROM project p
                      WHERE p.profile_id = user_profile.id))
                 WHERE id = ?1",
                params![owner.to_string(), to_unix_millis(at)],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn move_channel(&self, channel: ChannelId, to: ProjectId) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "UPDATE channel SET owner_project_id = ?2,
                                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![channel.to_string(), to.to_string()],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

/// A subquery, to join as `origin`, that finds the project and channel of
/// the channel id `channel` (an SQL expression) or, without one, of the
/// video id `video`'s channel, with that video's title either way (a
/// version names its source video and the channel it is made for):
/// columns `project_id`, `project_name`, `channel_name` and `video_title`.
/// A loose video (#341) has a title and no project or channel. No row when
/// neither names a channel or a video.
pub(crate) fn origin_of(video: &str, channel: &str) -> String {
    format!(
        "(SELECT p.id AS project_id, p.name AS project_name, c.name AS channel_name,
                 v.title AS video_title
          FROM (SELECT 1)
          LEFT JOIN video_project v ON v.id = ({video})
          LEFT JOIN channel c ON c.id = COALESCE(({channel}), v.channel_id)
          LEFT JOIN project p ON p.id = c.owner_project_id
          WHERE c.id IS NOT NULL OR v.id IS NOT NULL)"
    )
}

/// The origin columns of a job or cost record row.
pub(crate) struct OriginRow {
    project: Option<String>,
    project_name: Option<String>,
    channel_name: Option<String>,
    video_title: Option<String>,
}

impl OriginRow {
    /// Reads the four columns from `first` on: the project's id and name,
    /// the channel's name and the video's title.
    pub(crate) fn read(row: &Row<'_>, first: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            project: row.get(first)?,
            project_name: row.get(first + 1)?,
            channel_name: row.get(first + 2)?,
            video_title: row.get(first + 3)?,
        })
    }

    pub(crate) fn into_origin(self) -> Result<WorkOrigin, RepositoryError> {
        Ok(WorkOrigin {
            project: self
                .project
                .map(|id| Uuid::parse_str(&id).map(ProjectId::from))
                .transpose()
                .map_err(boxed)?,
            project_name: self.project_name,
            channel_name: self.channel_name,
            video_title: self.video_title,
        })
    }
}

/// Saves a project for tests that need a channel: every channel belongs
/// to one.
#[cfg(test)]
pub(crate) fn test_project(db: &Database, owner: ProfileId) -> ProjectId {
    let project = Project::new(
        owner,
        ProjectDetails::validate(ProjectDraft {
            name: format!("Project {}", Uuid::new_v4()),
            color: ProjectColor::Amber,
        })
        .unwrap(),
        SystemTime::UNIX_EPOCH,
    );
    db.save_project(&project).unwrap();
    project.id
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, ProfileRepository, ProjectState,
        UiLanguage, UserProfile,
    };

    use super::*;

    fn database_with_profile() -> (Database, ProfileId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        (db, profile.id)
    }

    fn project(owner: ProfileId, name: &str) -> Project {
        Project::new(
            owner,
            ProjectDetails::validate(ProjectDraft {
                name: name.into(),
                color: ProjectColor::Violet,
            })
            .unwrap(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_000),
        )
    }

    #[test]
    fn a_project_round_trips_and_updates_in_place() {
        let (db, owner) = database_with_profile();
        let mut saved = project(owner, "Space Archives");
        db.save_project(&saved).unwrap();
        assert_eq!(db.project(saved.id).unwrap(), Some(saved.clone()));

        saved.details = ProjectDetails::validate(ProjectDraft {
            name: "Arquivos".into(),
            color: ProjectColor::Lime,
        })
        .unwrap();
        saved.state = ProjectState::Archived;
        db.save_project(&saved).unwrap();
        assert_eq!(db.projects(owner).unwrap(), [saved]);
    }

    #[test]
    fn names_are_unique_per_profile_ignoring_case() {
        let (db, owner) = database_with_profile();
        db.save_project(&project(owner, "Space")).unwrap();
        assert!(db.save_project(&project(owner, "SPACE")).is_err());
    }

    #[test]
    fn opening_and_starring_are_kept() {
        let (db, owner) = database_with_profile();
        let a = project(owner, "A");
        let b = project(owner, "B");
        db.save_project(&a).unwrap();
        db.save_project(&b).unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_millis(5_000);
        db.mark_opened(a.id, at).unwrap();
        // Opened again at the same time, B still comes after A.
        db.mark_opened(b.id, at).unwrap();
        let b_opened = db.project(b.id).unwrap().unwrap().opened_at.unwrap();
        assert!(b_opened > at);
        db.set_favorites(owner, &[b.id, a.id]).unwrap();
        let read = db.projects(owner).unwrap();
        assert_eq!(read[0].opened_at, Some(at));
        assert_eq!((read[0].favorite, read[1].favorite), (Some(1), Some(0)));

        db.set_favorites(owner, &[a.id]).unwrap();
        let read = db.projects(owner).unwrap();
        assert_eq!((read[0].favorite, read[1].favorite), (Some(0), None));
        // Saving the details keeps the star and the time.
        db.save_project(&read[0]).unwrap();
        assert_eq!(db.project(a.id).unwrap().unwrap().favorite, Some(0));
    }

    #[test]
    fn a_channel_belongs_to_its_project_and_moves_to_another() {
        let (db, owner) = database_with_profile();
        let first = project(owner, "First");
        let second = project(owner, "Second");
        db.save_project(&first).unwrap();
        db.save_project(&second).unwrap();
        let channel = Channel::new(
            owner,
            first.id,
            ChannelDetails::validate(ChannelDraft {
                name: "Space".into(),
                ..ChannelDraft::default()
            })
            .unwrap(),
        );
        ChannelRepository::save(&db, &channel).unwrap();
        assert_eq!(db.get(channel.id).unwrap().unwrap().project, first.id);

        db.move_channel(channel.id, second.id).unwrap();
        assert_eq!(db.get(channel.id).unwrap().unwrap().project, second.id);
    }

    #[test]
    fn a_new_job_or_cost_remembers_its_project_with_the_names_of_then() {
        use bardo_domain::{
            Cost, CostPurpose, CostRecord, CostRecordId, CostRepository, Job, JobKind,
            JobRepository, Metered, Provider, VideoProjectId,
        };

        let (db, owner) = database_with_profile();
        let space = project(owner, "Space");
        db.save_project(&space).unwrap();
        let channel = Channel::new(
            owner,
            space.id,
            ChannelDetails::validate(ChannelDraft {
                name: "Space EN".into(),
                ..ChannelDraft::default()
            })
            .unwrap(),
        );
        ChannelRepository::save(&db, &channel).unwrap();
        let video = VideoProjectId::new();
        db.conn()
            .execute_batch(&format!(
                "INSERT INTO theme (id, profile_id, channel_id, niche, title, angle, status,
                     suggested_at, position)
                 VALUES ('t', '{owner}', '{channel}', 'space', 'Apollo', '', 'approved', 0, 0);
                 INSERT INTO video_project (id, profile_id, channel_id, theme_id, niche, title,
                     created_at)
                 VALUES ('{video}', '{owner}', '{channel}', 't', 'space', 'Apollo', 1);",
                channel = channel.id,
            ))
            .unwrap();

        let job = Job::new(
            owner,
            JobKind::Narration,
            format!("{{\"project\":\"{video}\"}}"),
        );
        JobRepository::save(&db, &job).unwrap();
        let plain = Job::new(owner, JobKind::Countdown, "not json");
        JobRepository::save(&db, &plain).unwrap();
        let cost = CostRecord {
            id: CostRecordId::new(),
            owner,
            provider: Provider::Claude,
            model: "claude".into(),
            purpose: CostPurpose::Script,
            usage: Metered::default(),
            cost: Cost::Unpriced,
            channel: Some(channel.id),
            project: None,
            job: None,
            at: SystemTime::UNIX_EPOCH,
            origin: WorkOrigin::default(),
            paid_by: bardo_domain::KeyPayer::App,
        };
        db.record_cost(&cost).unwrap();

        // A rename later leaves the names they were saved with.
        let mut renamed = space.clone();
        renamed.details = ProjectDetails::validate(ProjectDraft {
            name: "Renamed".into(),
            color: ProjectColor::Sky,
        })
        .unwrap();
        db.save_project(&renamed).unwrap();
        // Saving the job again keeps what it was for.
        JobRepository::save(&db, &job).unwrap();

        let jobs = JobRepository::list(&db, owner).unwrap();
        assert_eq!(
            jobs[0].origin(),
            &WorkOrigin {
                project: Some(space.id),
                project_name: Some("Space".into()),
                channel_name: Some("Space EN".into()),
                video_title: Some("Apollo".into()),
            }
        );
        assert_eq!(jobs[1].origin(), &WorkOrigin::default());
        let costs = db
            .costs_between(owner, SystemTime::UNIX_EPOCH, SystemTime::now())
            .unwrap();
        assert_eq!(
            costs[0].origin,
            WorkOrigin {
                project: Some(space.id),
                project_name: Some("Space".into()),
                channel_name: Some("Space EN".into()),
                video_title: None,
            }
        );
    }
    #[test]
    fn a_job_naming_a_channel_is_that_channels_even_with_another_channels_video() {
        use bardo_domain::{Job, JobKind, JobRepository, VideoProjectId};

        let (db, owner) = database_with_profile();
        let space = project(owner, "Space");
        db.save_project(&space).unwrap();
        let channel = |name: &str| {
            let channel = Channel::new(
                owner,
                space.id,
                ChannelDetails::validate(ChannelDraft {
                    name: name.into(),
                    ..ChannelDraft::default()
                })
                .unwrap(),
            );
            ChannelRepository::save(&db, &channel).unwrap();
            channel
        };
        let english = channel("Space EN");
        let portuguese = channel("Space PT");
        let video = VideoProjectId::new();
        db.conn()
            .execute_batch(&format!(
                "INSERT INTO theme (id, profile_id, channel_id, niche, title, angle, status,
                     suggested_at, position)
                 VALUES ('t', '{owner}', '{english}', 'space', 'Apollo', '', 'approved', 0, 0);
                 INSERT INTO video_project (id, profile_id, channel_id, theme_id, niche, title,
                     created_at)
                 VALUES ('{video}', '{owner}', '{english}', 't', 'space', 'Apollo', 1);",
                english = english.id,
            ))
            .unwrap();

        // A version of the English video, made for the Portuguese channel.
        let job = Job::new(
            owner,
            JobKind::Localization,
            format!(
                "{{\"project\":\"{video}\",\"channel\":\"{}\"}}",
                portuguese.id
            ),
        );
        JobRepository::save(&db, &job).unwrap();

        let jobs = JobRepository::list(&db, owner).unwrap();
        assert_eq!(
            jobs[0].origin(),
            &WorkOrigin {
                project: Some(space.id),
                project_name: Some("Space".into()),
                channel_name: Some("Space PT".into()),
                video_title: Some("Apollo".into()),
            }
        );
    }
}
