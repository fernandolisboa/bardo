//! Archived videos and deletions (issue #336). Deleting removes a video's
//! or a project's records through their cascades; jobs and cost records
//! keep the names they were saved with (no foreign key), so Costs goes on
//! naming the work until the user forgets the name.

use std::collections::HashSet;
use std::time::SystemTime;

use bardo_domain::{
    DeletedKind, DeletedWork, ProfileId, ProjectId, RemovalRepository, RepositoryError,
    VideoProjectId,
};
use rusqlite::{Transaction, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

fn record(tx: &Transaction<'_>, deleted: &DeletedWork) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO deleted_work (id, profile_id, kind, name, owner_project_id, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (id) DO UPDATE SET deleted_at = excluded.deleted_at",
        params![
            deleted.id.to_string(),
            deleted.owner.to_string(),
            deleted.kind.code(),
            deleted.name,
            deleted.project.map(|project| project.to_string()),
            to_unix_millis(deleted.deleted_at),
        ],
    )?;
    Ok(())
}

impl RemovalRepository for Database {
    fn archived_videos(
        &self,
        owner: ProfileId,
    ) -> Result<HashSet<VideoProjectId>, RepositoryError> {
        let ids = self
            .conn()
            .prepare(
                "SELECT id FROM video_project WHERE profile_id = ?1 AND archived_at IS NOT NULL",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        ids.iter()
            .map(|id| Uuid::parse_str(id).map(VideoProjectId::from).map_err(boxed))
            .collect()
    }

    fn set_video_archived(
        &self,
        video: VideoProjectId,
        at: Option<SystemTime>,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "UPDATE video_project SET archived_at = ?2 WHERE id = ?1",
                params![video.to_string(), at.map(to_unix_millis)],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn delete_video(
        &self,
        video: VideoProjectId,
        deleted: &DeletedWork,
    ) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        record(&tx, deleted).map_err(boxed)?;
        tx.execute(
            "DELETE FROM video_project WHERE id = ?1",
            [video.to_string()],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)
    }

    fn delete_project(
        &self,
        project: ProjectId,
        deleted: &[DeletedWork],
    ) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        for deleted in deleted {
            record(&tx, deleted).map_err(boxed)?;
        }
        let id = project.to_string();
        // Its videos first: they name their theme and folder project
        // without a cascade.
        tx.execute(
            "DELETE FROM video_project WHERE channel_id IN
                 (SELECT id FROM channel WHERE owner_project_id = ?1)",
            [&id],
        )
        .map_err(boxed)?;
        // A video of another project whose media has not left this folder
        // yet is looked for in the old place from now on.
        tx.execute(
            "UPDATE video_project SET folder_project_id = NULL, folder_name = NULL
             WHERE folder_project_id = ?1",
            [&id],
        )
        .map_err(boxed)?;
        tx.execute("DELETE FROM channel WHERE owner_project_id = ?1", [&id])
            .map_err(boxed)?;
        tx.execute("DELETE FROM project WHERE id = ?1", [&id])
            .map_err(boxed)?;
        tx.commit().map_err(boxed)
    }

    fn deleted_work(&self, owner: ProfileId) -> Result<Vec<DeletedWork>, RepositoryError> {
        type Row = (String, String, Option<String>, Option<String>, i64);
        let rows: Vec<Row> = self
            .conn()
            .prepare(
                "SELECT id, kind, name, owner_project_id, deleted_at FROM deleted_work
                 WHERE profile_id = ?1 ORDER BY deleted_at DESC, rowid DESC",
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
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter()
            .filter_map(|(id, kind, name, project, at)| {
                // A kind of a newer Bardo is left out.
                let kind = DeletedKind::from_code(&kind)?;
                Some((id, kind, name, project, at))
            })
            .map(|(id, kind, name, project, at)| {
                Ok(DeletedWork {
                    id: Uuid::parse_str(&id).map_err(boxed)?,
                    owner,
                    kind,
                    name,
                    project: project
                        .map(|project| Uuid::parse_str(&project).map(ProjectId::from))
                        .transpose()
                        .map_err(boxed)?,
                    deleted_at: from_unix_millis(at),
                })
            })
            .collect()
    }

    fn forget_name(&self, owner: ProfileId, id: Uuid) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let (owner, id) = (owner.to_string(), id.to_string());
        let kind: Option<String> = tx
            .query_row(
                "SELECT kind FROM deleted_work WHERE id = ?1 AND profile_id = ?2",
                [&id, &owner],
                |row| row.get(0),
            )
            .ok();
        match kind.as_deref().and_then(DeletedKind::from_code) {
            Some(DeletedKind::Project) => {
                tx.execute(
                    "UPDATE deleted_work SET name = NULL
                     WHERE profile_id = ?2 AND (id = ?1 OR owner_project_id = ?1)",
                    [&id, &owner],
                )
                .map_err(boxed)?;
                for table in ["cost_record", "job"] {
                    tx.execute(
                        &format!(
                            "UPDATE {table} SET project_name = NULL, channel_name = NULL,
                                                video_title = NULL
                             WHERE profile_id = ?2 AND owner_project_id = ?1"
                        ),
                        [&id, &owner],
                    )
                    .map_err(boxed)?;
                }
            }
            Some(DeletedKind::Video) => {
                tx.execute(
                    "UPDATE deleted_work SET name = NULL WHERE id = ?1 AND profile_id = ?2",
                    [&id, &owner],
                )
                .map_err(boxed)?;
                tx.execute(
                    "UPDATE cost_record SET video_title = NULL
                     WHERE profile_id = ?2 AND project_id = ?1",
                    [&id, &owner],
                )
                .map_err(boxed)?;
                tx.execute(
                    "UPDATE job SET video_title = NULL
                     WHERE profile_id = ?2 AND json_valid(payload)
                           AND json_extract(payload, '$.project') = ?1",
                    [&id, &owner],
                )
                .map_err(boxed)?;
            }
            None => {}
        }
        tx.commit().map_err(boxed)
    }
}
