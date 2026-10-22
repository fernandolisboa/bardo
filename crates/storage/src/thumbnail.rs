//! YouTube thumbnails, their drawn backgrounds and chapters (#182).

use std::time::Duration;

use bardo_domain::{
    BackgroundSource, Chapter, ChapterRepository, Chapters, DrawnBackground, JobId, NarrationId,
    ProfileId, RepositoryError, Thumbnail, ThumbnailBackground, ThumbnailDesign,
    ThumbnailRepository, VideoProjectId,
};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::script::{generation, insert_generation};
use crate::{Database, boxed, from_unix_millis, to_unix_millis};

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

/// A thumbnail row as stored, before domain validation.
struct ThumbnailRow {
    owner: String,
    text: String,
    placement: String,
    look: String,
    background_file: Option<String>,
    background_source: Option<String>,
    prompt: String,
    generation_id: Option<String>,
    file: Option<String>,
    updated_at: i64,
}

impl ThumbnailRepository for Database {
    fn thumbnail(&self, project: VideoProjectId) -> Result<Option<Thumbnail>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT profile_id, text, placement, look, background_file, background_source,
                        prompt, generation_id, file, updated_at
                 FROM thumbnail WHERE project_id = ?1",
                [project.to_string()],
                |row| {
                    Ok(ThumbnailRow {
                        owner: row.get(0)?,
                        text: row.get(1)?,
                        placement: row.get(2)?,
                        look: row.get(3)?,
                        background_file: row.get(4)?,
                        background_source: row.get(5)?,
                        prompt: row.get(6)?,
                        generation_id: row.get(7)?,
                        file: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let background = match (row.background_file, row.background_source) {
            (Some(file), Some(source)) => Some(ThumbnailBackground {
                file,
                source: source.parse::<BackgroundSource>().map_err(boxed)?,
            }),
            _ => None,
        };
        Ok(Some(Thumbnail {
            project,
            owner: ProfileId::from(uuid(&row.owner)?),
            design: ThumbnailDesign {
                text: row.text,
                placement: row.placement.parse().map_err(boxed)?,
                look: row.look.parse().map_err(boxed)?,
                background,
            },
            prompt: row.prompt,
            generation: row
                .generation_id
                .map(|id| generation(&conn, &id))
                .transpose()?,
            file: row.file,
            updated_at: from_unix_millis(row.updated_at),
        }))
    }

    fn save_thumbnail(&self, thumbnail: &Thumbnail) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        if let Some(generation) = &thumbnail.generation {
            insert_generation(&tx, generation)?;
        }
        let design = &thumbnail.design;
        let background = design.background.as_ref();
        tx.execute(
            "INSERT INTO thumbnail (project_id, profile_id, text, placement, look,
                                    background_file, background_source, prompt, generation_id,
                                    file, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (project_id) DO UPDATE SET
                 text = excluded.text,
                 placement = excluded.placement,
                 look = excluded.look,
                 background_file = excluded.background_file,
                 background_source = excluded.background_source,
                 prompt = excluded.prompt,
                 generation_id = excluded.generation_id,
                 file = excluded.file,
                 updated_at = excluded.updated_at",
            params![
                thumbnail.project.to_string(),
                thumbnail.owner.to_string(),
                design.text,
                design.placement.code(),
                design.look.code(),
                background.map(|background| background.file.as_str()),
                background.map(|background| background.source.code()),
                thumbnail.prompt,
                thumbnail
                    .generation
                    .as_ref()
                    .map(|generation| generation.id.to_string()),
                thumbnail.file,
                to_unix_millis(thumbnail.updated_at),
            ],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)
    }

    fn drawn_backgrounds(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<DrawnBackground>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT profile_id, file, prompt, provider, model, job_id, drawn_at
                 FROM drawn_background WHERE project_id = ?1
                 ORDER BY drawn_at DESC, file DESC",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .map_err(boxed)?;
        rows.map(|row| {
            let (owner, file, prompt, provider, model, job, drawn_at) = row.map_err(boxed)?;
            Ok(DrawnBackground {
                project,
                owner: ProfileId::from(uuid(&owner)?),
                file,
                prompt,
                provider: provider.parse().map_err(boxed)?,
                model,
                job: job.as_deref().map(uuid).transpose()?.map(JobId::from),
                drawn_at: from_unix_millis(drawn_at),
            })
        })
        .collect()
    }

    fn add_drawn_background(&self, background: &DrawnBackground) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO drawn_background (project_id, file, profile_id, prompt, provider,
                                               model, job_id, drawn_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (project_id, file) DO NOTHING",
                params![
                    background.project.to_string(),
                    background.file,
                    background.owner.to_string(),
                    background.prompt,
                    background.provider.code(),
                    background.model,
                    background.job.map(|job| job.to_string()),
                    to_unix_millis(background.drawn_at),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

impl ChapterRepository for Database {
    fn chapters(&self, project: VideoProjectId) -> Result<Option<Chapters>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT profile_id, include, narration_id, generation_id, edited, updated_at
                 FROM chapters WHERE project_id = ?1",
                [project.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some((owner, include, narration, generation_id, edited, updated_at)) = row else {
            return Ok(None);
        };
        let mut statement = conn
            .prepare("SELECT start_ms, title FROM chapter WHERE project_id = ?1 ORDER BY position")
            .map_err(boxed)?;
        let list = statement
            .query_map([project.to_string()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(boxed)?
            .map(|row| {
                let (start, title) = row.map_err(boxed)?;
                Ok(Chapter {
                    start: Duration::from_millis(u64::try_from(start).map_err(boxed)?),
                    title,
                })
            })
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        Ok(Some(Chapters::restore(
            project,
            ProfileId::from(uuid(&owner)?),
            list,
            include != 0,
            NarrationId::from(uuid(&narration)?),
            generation(&conn, &generation_id)?,
            edited != 0,
            from_unix_millis(updated_at),
        )))
    }

    fn save_chapters(&self, chapters: &Chapters) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        insert_generation(&tx, chapters.generation())?;
        let project = chapters.project.to_string();
        tx.execute(
            "INSERT INTO chapters (project_id, profile_id, include, narration_id, generation_id,
                                   edited, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (project_id) DO UPDATE SET
                 include = excluded.include,
                 narration_id = excluded.narration_id,
                 generation_id = excluded.generation_id,
                 edited = excluded.edited,
                 updated_at = excluded.updated_at",
            params![
                project,
                chapters.owner.to_string(),
                i64::from(chapters.include),
                chapters.narration.to_string(),
                chapters.generation().id.to_string(),
                i64::from(chapters.is_edited()),
                to_unix_millis(chapters.updated_at),
            ],
        )
        .map_err(boxed)?;
        tx.execute("DELETE FROM chapter WHERE project_id = ?1", [&project])
            .map_err(boxed)?;
        for (position, chapter) in chapters.list().iter().enumerate() {
            tx.execute(
                "INSERT INTO chapter (project_id, position, start_ms, title)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    project,
                    i64::try_from(position).map_err(boxed)?,
                    i64::try_from(chapter.start.as_millis()).map_err(boxed)?,
                    chapter.title,
                ],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Generation, GenerationId, Niche,
        ProfileRepository, Provider, TemplateBody, TemplateKind, TemplateRepository, TemplateUsed,
        TemplateVersion, TextPlacement, Theme, ThemeIdea, ThemeRepository, ThumbnailLook,
        TokenUsage, UiLanguage, UserProfile, VideoProject,
    };

    use super::*;

    fn time(millis: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
    }

    fn project(db: &Database) -> VideoProject {
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(db, &profile).unwrap();
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Space Archives".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(profile.id, details);
        ChannelRepository::save(db, &channel).unwrap();
        let mut theme = Theme::suggested(
            profile.id,
            channel.id,
            Niche::new("space history").unwrap(),
            ThemeIdea::new("The lost probe", "").unwrap(),
            time(1_800_000_000_000),
            0,
            None,
        );
        db.save_themes(std::slice::from_ref(&theme)).unwrap();
        let project = theme.approve(time(1_800_000_001_000)).unwrap();
        db.start_project(&theme, &project).unwrap();
        project
    }

    fn generation(db: &Database, project: &VideoProject, kind: TemplateKind) -> Generation {
        let body = TemplateBody::new(kind, "", "For {{theme_title}}").unwrap();
        let version = TemplateVersion::first(project.owner, body, time(1));
        db.add_template_version(&version).unwrap();
        Generation {
            id: GenerationId::new(),
            owner: project.owner,
            project: project.id,
            provider: Provider::Claude,
            model: "claude-opus-5-5".into(),
            template: TemplateUsed {
                id: version.id,
                number: version.number,
            },
            instructions: String::new(),
            prompt: "For The lost probe".into(),
            output: "{}".into(),
            usage: TokenUsage {
                input_tokens: 120,
                output_tokens: 80,
            },
            generated_at: time(5_000),
            job: None,
        }
    }

    #[test]
    fn a_thumbnail_round_trips_with_and_without_its_writing() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        assert_eq!(db.thumbnail(project.id).unwrap(), None);
        let mut thumbnail =
            Thumbnail::new(project.id, project.owner, ThumbnailLook::Banner, time(10));
        db.save_thumbnail(&thumbnail).unwrap();
        assert_eq!(db.thumbnail(project.id).unwrap(), Some(thumbnail.clone()));

        thumbnail.written(
            "Never found".into(),
            "A probe drifting in the dark".into(),
            generation(&db, &project, TemplateKind::Thumbnail),
        );
        thumbnail.design.placement = TextPlacement::BottomRight;
        thumbnail.design.background = Some(ThumbnailBackground {
            file: "thumbnail-bg-1.png".into(),
            source: BackgroundSource::Drawn,
        });
        thumbnail.file = Some(thumbnail.design.file_name());
        db.save_thumbnail(&thumbnail).unwrap();
        assert_eq!(db.thumbnail(project.id).unwrap(), Some(thumbnail));
    }

    #[test]
    fn drawn_backgrounds_list_newest_first_and_save_once() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let drawn = |file: &str, at: u64| DrawnBackground {
            project: project.id,
            owner: project.owner,
            file: file.into(),
            prompt: "A probe".into(),
            provider: Provider::Gemini,
            model: "gemini-3-pro-image".into(),
            job: Some(JobId::new()),
            drawn_at: time(at),
        };
        let first = drawn("thumbnail-bg-a.png", 1_000);
        let second = drawn("thumbnail-bg-b.png", 2_000);
        db.add_drawn_background(&first).unwrap();
        db.add_drawn_background(&second).unwrap();
        db.add_drawn_background(&first).unwrap();
        assert_eq!(db.drawn_backgrounds(project.id).unwrap(), [second, first]);
        assert_eq!(db.drawn_backgrounds(VideoProjectId::new()).unwrap(), []);
    }

    #[test]
    fn chapters_round_trip_in_order_and_saving_again_replaces_them() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        assert_eq!(db.chapters(project.id).unwrap(), None);
        let chapter = |ms: u64, title: &str| Chapter {
            start: Duration::from_millis(ms),
            title: title.into(),
        };
        let mut chapters = Chapters::generated(
            NarrationId::new(),
            vec![
                chapter(0, "Start"),
                chapter(41_250, "The signal"),
                chapter(90_000, "Silence"),
            ],
            generation(&db, &project, TemplateKind::Chapters),
            time(6_000),
        )
        .unwrap();
        db.save_chapters(&chapters).unwrap();
        assert_eq!(db.chapters(project.id).unwrap(), Some(chapters.clone()));
        chapters
            .edit(
                vec![chapter(0, "Opening"), chapter(90_000, "Silence")],
                false,
                time(7_000),
            )
            .unwrap();
        db.save_chapters(&chapters).unwrap();
        assert_eq!(db.chapters(project.id).unwrap(), Some(chapters));
    }
}
