//! An ad's export for ads (#283): its texts for an ad manager, the windows
//! the user moved and each shape's last export.

use bardo_domain::{
    AdExport, AdExportRepository, AdTexts, AdWindow, CropPosition, FrameShape, RepositoryError,
    VideoProjectId,
};
use rusqlite::{OptionalExtension, params};

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

fn shape_of(code: &str) -> Result<FrameShape, RepositoryError> {
    code.parse().map_err(boxed)
}

fn step(value: i64) -> u16 {
    u16::try_from(value.max(0)).unwrap_or(u16::MAX)
}

impl Database {
    pub(crate) fn read_ad_texts(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<AdTexts>, RepositoryError> {
        self.conn()
            .query_row(
                "SELECT primary_text, headline FROM ad_texts WHERE project_id = ?1",
                [project.to_string()],
                |row| {
                    Ok(AdTexts::new(
                        &row.get::<_, String>(0)?,
                        &row.get::<_, String>(1)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)
    }

    pub(crate) fn write_ad_texts(
        &self,
        project: VideoProjectId,
        texts: &AdTexts,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO ad_texts (project_id, primary_text, headline) VALUES (?1, ?2, ?3)
                 ON CONFLICT (project_id) DO UPDATE SET
                     primary_text = excluded.primary_text, headline = excluded.headline",
                params![project.to_string(), texts.primary(), texts.headline()],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

impl AdExportRepository for Database {
    fn ad_windows(&self, project: VideoProjectId) -> Result<Vec<AdWindow>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT shape, clip, x, y FROM ad_window WHERE project_id = ?1
                 ORDER BY shape, clip",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?;
        rows.into_iter()
            .map(|(shape, clip, x, y)| {
                Ok(AdWindow {
                    shape: shape_of(&shape)?,
                    clip,
                    position: CropPosition::new(step(x), step(y)),
                })
            })
            .collect()
    }

    fn save_ad_window(
        &self,
        project: VideoProjectId,
        window: &AdWindow,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO ad_window (project_id, shape, clip, x, y) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (project_id, shape, clip) DO UPDATE SET
                     x = excluded.x, y = excluded.y",
                params![
                    project.to_string(),
                    window.shape.code(),
                    window.clip,
                    i64::from(window.position.x()),
                    i64::from(window.position.y()),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn drop_ad_window(
        &self,
        project: VideoProjectId,
        shape: FrameShape,
        clip: &str,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "DELETE FROM ad_window WHERE project_id = ?1 AND shape = ?2 AND clip = ?3",
                params![project.to_string(), shape.code(), clip],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn ad_exports(&self, project: VideoProjectId) -> Result<Vec<AdExport>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT shape, made_from, file, size_bytes, exported_at FROM ad_export
                 WHERE project_id = ?1 ORDER BY shape",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?;
        rows.into_iter()
            .map(|(shape, made_from, file, size, at)| {
                Ok(AdExport {
                    project,
                    shape: shape_of(&shape)?,
                    made_from,
                    file,
                    size_bytes: u64::try_from(size).unwrap_or(0),
                    exported_at: from_unix_millis(at),
                })
            })
            .collect()
    }

    fn save_ad_export(&self, export: &AdExport) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO ad_export
                     (project_id, shape, made_from, file, size_bytes, exported_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (project_id, shape) DO UPDATE SET
                     made_from = excluded.made_from, file = excluded.file,
                     size_bytes = excluded.size_bytes, exported_at = excluded.exported_at",
                params![
                    export.project.to_string(),
                    export.shape.code(),
                    export.made_from,
                    export.file,
                    i64::try_from(export.size_bytes).unwrap_or(i64::MAX),
                    to_unix_millis(export.exported_at),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::AdRepository;

    use super::*;

    fn project(db: &Database) -> VideoProjectId {
        crate::export::tests::project(db).id
    }

    #[test]
    fn an_ads_texts_windows_and_exports_are_kept() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        assert_eq!(db.ad_texts(project).unwrap(), None);
        let texts = AdTexts::new("Folds flat, lights a desk", "The folding lamp");
        db.save_ad_texts(project, &texts).unwrap();
        db.save_ad_texts(project, &texts).unwrap();
        assert_eq!(db.ad_texts(project).unwrap(), Some(texts));

        let window = AdWindow {
            shape: FrameShape::Portrait,
            clip: "scene:2@0".into(),
            position: CropPosition::new(120, 500),
        };
        db.save_ad_window(project, &window).unwrap();
        let moved = AdWindow {
            position: CropPosition::new(900, 500),
            ..window.clone()
        };
        db.save_ad_window(project, &moved).unwrap();
        let square = AdWindow {
            shape: FrameShape::Square,
            ..window.clone()
        };
        db.save_ad_window(project, &square).unwrap();
        assert_eq!(
            db.ad_windows(project).unwrap(),
            vec![square.clone(), moved.clone()]
        );
        db.drop_ad_window(project, FrameShape::Square, "scene:2@0")
            .unwrap();
        assert_eq!(db.ad_windows(project).unwrap(), vec![moved]);

        let at = SystemTime::UNIX_EPOCH + Duration::from_millis(1_800_000_000_000);
        let export = AdExport {
            project,
            shape: FrameShape::Square,
            made_from: "abc".into(),
            file: "The lamp.mp4".into(),
            size_bytes: 42,
            exported_at: at,
        };
        db.save_ad_export(&export).unwrap();
        let again = AdExport {
            made_from: "def".into(),
            ..export
        };
        db.save_ad_export(&again).unwrap();
        assert_eq!(db.ad_exports(project).unwrap(), vec![again]);
    }
}
