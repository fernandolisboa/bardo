//! Where media lives (issue #335, ADR-0047 item 3): the base folder, each
//! project's folder and, per video, the project folder holding its media
//! and its folder's name there.

use std::path::{Path, PathBuf};

use bardo_domain::{
    FolderRepository, ProfileId, ProjectId, RepositoryError, VideoFolder, VideoFolders,
    VideoProjectId, VideoToMove,
};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::{Database, boxed};

/// A path as the database keeps it.
pub(crate) fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

/// The project folder holding a video's media, and its name there.
const SELECT_FOLDER: &str = "SELECT v.folder_project_id, p.folder, v.folder_name
     FROM video_project v JOIN project p ON p.id = v.folder_project_id
     WHERE v.id = ?1 AND v.folder_name IS NOT NULL";

fn video_folder(
    project: String,
    folder: Option<String>,
    name: String,
) -> Result<VideoFolder, RepositoryError> {
    Ok(VideoFolder {
        project: ProjectId::from(uuid(&project)?),
        project_folder: folder.map(PathBuf::from),
        name,
    })
}

impl FolderRepository for Database {
    fn base_folder(&self, owner: ProfileId) -> Result<Option<PathBuf>, RepositoryError> {
        let folder: Option<Option<String>> = self
            .conn()
            .query_row(
                "SELECT base_folder FROM user_profile WHERE id = ?1",
                [owner.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(boxed)?;
        Ok(folder.flatten().map(PathBuf::from))
    }

    fn set_base_folder(
        &self,
        owner: ProfileId,
        folder: Option<&Path>,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "UPDATE user_profile SET base_folder = ?2 WHERE id = ?1",
                params![owner.to_string(), folder.map(path_text)],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn set_project_folder(&self, project: ProjectId, folder: &Path) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "UPDATE project SET folder = ?2 WHERE id = ?1",
                params![project.to_string(), path_text(folder)],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn video_folder(&self, video: VideoProjectId) -> Result<Option<VideoFolder>, RepositoryError> {
        let row: Option<(String, Option<String>, String)> = self
            .conn()
            .query_row(SELECT_FOLDER, [video.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()
            .map_err(boxed)?;
        row.map(|(project, folder, name)| video_folder(project, folder, name))
            .transpose()
    }

    fn loose_folder(&self, video: VideoProjectId) -> Result<Option<PathBuf>, RepositoryError> {
        let row: Option<(String, String)> = self
            .conn()
            .query_row(
                "SELECT loose_folder, folder_name FROM video_project
                 WHERE id = ?1 AND loose_folder IS NOT NULL AND folder_name IS NOT NULL",
                [video.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(boxed)?;
        Ok(row.map(|(folder, name)| PathBuf::from(folder).join(name)))
    }

    fn videos_to_move(&self, owner: ProfileId) -> Result<Vec<VideoToMove>, RepositoryError> {
        type Row = (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let rows: Vec<Row> = self
            .conn()
            .prepare(
                "SELECT v.id, v.title, c.owner_project_id, v.folder_project_id, f.folder,
                        v.folder_name, v.loose_folder
                 FROM video_project v
                 JOIN channel c ON c.id = v.channel_id
                 LEFT JOIN project f ON f.id = v.folder_project_id
                 WHERE v.profile_id = ?1
                   AND (v.folder_name IS NULL OR v.folder_project_id IS NULL
                        OR v.folder_project_id <> c.owner_project_id)
                 ORDER BY v.created_at, v.rowid",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                        ))
                    })?
                    .collect()
            })
            .map_err(boxed)?;
        rows.into_iter()
            .map(
                |(video, title, to, from_project, from_folder, name, loose)| {
                    // A loose video that moved into a project comes from the
                    // loose videos' folder.
                    let from_loose = match (&loose, &name) {
                        (Some(loose), Some(name)) => Some(PathBuf::from(loose).join(name)),
                        _ => None,
                    };
                    let from = match (from_project, name) {
                        (Some(project), Some(name)) => {
                            Some(video_folder(project, from_folder, name)?)
                        }
                        _ => None,
                    };
                    Ok(VideoToMove {
                        video: VideoProjectId::from(uuid(&video)?),
                        title,
                        to: ProjectId::from(uuid(&to)?),
                        from,
                        from_loose,
                    })
                },
            )
            .collect()
    }

    fn videos_in(&self, projects: &[ProjectId]) -> Result<Vec<VideoProjectId>, RepositoryError> {
        if projects.is_empty() {
            return Ok(Vec::new());
        }
        // Ids are UUIDs: a JSON array of them needs no escaping.
        let ids = format!(
            "[{}]",
            projects
                .iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        let rows: Vec<String> = self
            .conn()
            .prepare(
                "SELECT v.id FROM video_project v JOIN channel c ON c.id = v.channel_id
                 WHERE v.folder_project_id IN (SELECT value FROM json_each(?1))
                    OR c.owner_project_id IN (SELECT value FROM json_each(?1))",
            )
            .and_then(|mut statement| statement.query_map([ids], |row| row.get(0))?.collect())
            .map_err(boxed)?;
        rows.iter()
            .map(|id| Ok(VideoProjectId::from(uuid(id)?)))
            .collect()
    }

    fn set_video_folder(
        &self,
        video: VideoProjectId,
        project: ProjectId,
        name: &str,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "UPDATE video_project SET folder_project_id = ?2, folder_name = ?3, loose_folder = NULL
                 WHERE id = ?1",
                params![video.to_string(), project.to_string(), name],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

impl VideoFolders for Database {
    fn video_folder_path(&self, video: VideoProjectId) -> Option<PathBuf> {
        let found = FolderRepository::video_folder(self, video).and_then(|folder| {
            match folder.and_then(|folder| folder.path()) {
                Some(path) => Ok(Some(path)),
                None => self.loose_folder(video),
            }
        });
        match found {
            Ok(folder) => folder,
            Err(error) => {
                tracing::warn!("could not read where video {video}'s media is: {error}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Project, ProjectColor,
        ProjectDetails, ProjectDraft, ProjectRepository, UiLanguage, UserProfile,
        video_folder_name,
    };

    use super::*;

    struct World {
        db: Database,
        owner: ProfileId,
        space: Project,
        sea: Project,
        channel: Channel,
    }

    fn project(db: &Database, owner: ProfileId, name: &str, folder: Option<&str>) -> Project {
        let mut project = Project::new(
            owner,
            ProjectDetails::validate(ProjectDraft {
                name: name.into(),
                color: ProjectColor::Teal,
            })
            .unwrap(),
            std::time::SystemTime::UNIX_EPOCH,
        );
        project.folder = folder.map(PathBuf::from);
        db.save_project(&project).unwrap();
        project
    }

    fn world() -> World {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&db, &profile).unwrap();
        let owner = profile.id;
        let space = project(&db, owner, "Space", Some("/base/Space"));
        let sea = project(&db, owner, "Sea", None);
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
        World {
            db,
            owner,
            space,
            sea,
            channel,
        }
    }

    fn insert_video(world: &World, title: &str) -> VideoProjectId {
        let video = VideoProjectId::new();
        world
            .db
            .conn()
            .execute_batch(&format!(
                "INSERT OR IGNORE INTO theme (id, profile_id, channel_id, niche, title, angle,
                     status, suggested_at, position)
                 VALUES ('t', '{owner}', '{channel}', 'space', 'Apollo', '', 'approved', 0, 0);
                 INSERT INTO video_project (id, profile_id, channel_id, theme_id, niche, title,
                     created_at)
                 VALUES ('{video}', '{owner}', '{channel}', 't', 'space', '{title}', 1);",
                owner = world.owner,
                channel = world.channel.id,
            ))
            .unwrap();
        video
    }

    #[test]
    fn the_base_folder_is_the_default_until_chosen() {
        let world = world();
        assert_eq!(world.db.base_folder(world.owner).unwrap(), None);
        world
            .db
            .set_base_folder(world.owner, Some(Path::new("/media/Bardo")))
            .unwrap();
        assert_eq!(
            world.db.base_folder(world.owner).unwrap(),
            Some(PathBuf::from("/media/Bardo"))
        );
        world.db.set_base_folder(world.owner, None).unwrap();
        assert_eq!(world.db.base_folder(world.owner).unwrap(), None);
    }

    #[test]
    fn a_project_keeps_its_folder_and_can_be_pointed_elsewhere() {
        let world = world();
        let read = world.db.project(world.space.id).unwrap().unwrap();
        assert_eq!(read.folder, Some(PathBuf::from("/base/Space")));
        assert_eq!(
            world.db.project(world.sea.id).unwrap().unwrap().folder,
            None
        );
        // Saving it again (a rename) leaves its folder where it is.
        world.db.save_project(&world.sea).unwrap();
        world
            .db
            .set_project_folder(world.sea.id, Path::new("/elsewhere/Sea"))
            .unwrap();
        let mut renamed = world.db.project(world.sea.id).unwrap().unwrap();
        renamed.folder = None;
        world.db.save_project(&renamed).unwrap();
        assert_eq!(
            world.db.project(world.sea.id).unwrap().unwrap().folder,
            Some(PathBuf::from("/elsewhere/Sea"))
        );
    }

    #[test]
    fn a_video_in_the_old_folder_moves_then_follows_its_channel() {
        let world = world();
        let video = insert_video(&world, "Apollo");
        assert_eq!(world.db.video_folder(video).unwrap(), None);
        assert_eq!(world.db.video_folder_path(video), None);
        let to_move = world.db.videos_to_move(world.owner).unwrap();
        assert_eq!(
            to_move,
            vec![VideoToMove {
                video,
                title: "Apollo".into(),
                to: world.space.id,
                from: None,
                from_loose: None,
            }]
        );
        let name = to_move[0].folder_name();
        assert_eq!(name, video_folder_name("Apollo", video));

        world
            .db
            .set_video_folder(video, world.space.id, &name)
            .unwrap();
        assert!(world.db.videos_to_move(world.owner).unwrap().is_empty());
        assert_eq!(
            world.db.video_folder_path(video),
            Some(PathBuf::from("/base/Space").join("Videos").join(&name))
        );

        // The channel moves: the media stays where it is until it follows.
        world
            .db
            .move_channel(world.channel.id, world.sea.id)
            .unwrap();
        let to_move = world.db.videos_to_move(world.owner).unwrap();
        assert_eq!(to_move.len(), 1);
        assert_eq!(to_move[0].to, world.sea.id);
        let from = to_move[0].from.clone().unwrap();
        assert_eq!(from.project, world.space.id);
        assert_eq!(to_move[0].folder_name(), name, "the name stays");
        assert_eq!(
            world.db.video_folder_path(video),
            Some(PathBuf::from("/base/Space").join("Videos").join(&name))
        );
        // Both projects hold it: Space has its media, Sea its channel.
        assert_eq!(world.db.videos_in(&[world.space.id]).unwrap(), vec![video]);
        assert_eq!(world.db.videos_in(&[world.sea.id]).unwrap(), vec![video]);
        assert!(world.db.videos_in(&[]).unwrap().is_empty());
    }
}
