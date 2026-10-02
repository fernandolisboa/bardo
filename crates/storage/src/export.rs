use bardo_domain::{
    Export, ExportRepository, Network, NetworkAccountId, ProfileId, RenderId, RepositoryError,
    VideoMetadata, VideoMetadataDraft, VideoProjectId,
};
use rusqlite::params;
use uuid::Uuid;

use crate::script::{generation, insert_generation};
use crate::{Database, boxed, from_unix_millis, to_unix_millis};

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

/// Tags hold no line breaks, so they are kept one per line.
fn tags_text(tags: &[String]) -> String {
    tags.join("\n")
}

fn tags_of(text: &str) -> Vec<String> {
    text.lines()
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

impl ExportRepository for Database {
    fn video_metadata(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<VideoMetadata>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(
                "SELECT network, profile_id, title, description, tags, generation_id, edited,
                        updated_at
                 FROM video_metadata WHERE project_id = ?1",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([project.to_string()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, bool>(6)?,
                            row.get::<_, i64>(7)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(boxed)?;
        let mut metadata = rows
            .into_iter()
            .map(
                |(network, owner, title, description, tags, generation_id, edited, updated_at)| {
                    let network: Network = network.parse().map_err(boxed)?;
                    Ok(VideoMetadata::restore(
                        project,
                        ProfileId::from(uuid(&owner)?),
                        network,
                        VideoMetadataDraft {
                            title,
                            description,
                            tags: tags_of(&tags),
                        },
                        generation(&conn, &generation_id)?,
                        edited,
                        from_unix_millis(updated_at),
                    ))
                },
            )
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        metadata.sort_by_key(|metadata| metadata.network);
        Ok(metadata)
    }

    fn save_video_metadata(&self, metadata: &[VideoMetadata]) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        for one in metadata {
            insert_generation(&tx, one.generation())?;
            tx.execute(
                "INSERT INTO video_metadata (project_id, network, profile_id, title, description,
                                             tags, generation_id, edited, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (project_id, network) DO UPDATE SET
                     title = excluded.title,
                     description = excluded.description,
                     tags = excluded.tags,
                     generation_id = excluded.generation_id,
                     edited = excluded.edited,
                     updated_at = excluded.updated_at",
                params![
                    one.project.to_string(),
                    one.network.code(),
                    one.owner.to_string(),
                    one.title(),
                    one.description(),
                    tags_text(one.tags()),
                    one.generation().id.to_string(),
                    one.is_edited(),
                    to_unix_millis(one.updated_at),
                ],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }

    fn exports(&self, project: VideoProjectId) -> Result<Vec<Export>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(
                "SELECT network, profile_id, account_id, package, video_file, render_id, post,
                        exported_at
                 FROM export WHERE project_id = ?1",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([project.to_string()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, i64>(7)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(boxed)?;
        let mut exports = rows
            .into_iter()
            .map(
                |(network, owner, account, package, video_file, render, post, exported_at)| {
                    Ok(Export {
                        project,
                        owner: ProfileId::from(uuid(&owner)?),
                        network: network.parse().map_err(boxed)?,
                        account: NetworkAccountId::from(uuid(&account)?),
                        package,
                        video_file,
                        render: RenderId::from(uuid(&render)?),
                        post,
                        exported_at: from_unix_millis(exported_at),
                    })
                },
            )
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        exports.sort_by_key(|export| export.network);
        Ok(exports)
    }

    fn save_export(&self, export: &Export) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO export (project_id, network, profile_id, account_id, package,
                                     video_file, render_id, post, exported_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (project_id, network) DO UPDATE SET
                     account_id = excluded.account_id,
                     package = excluded.package,
                     video_file = excluded.video_file,
                     render_id = excluded.render_id,
                     post = excluded.post,
                     exported_at = excluded.exported_at",
                params![
                    export.project.to_string(),
                    export.network.code(),
                    export.owner.to_string(),
                    export.account.to_string(),
                    export.package,
                    export.video_file,
                    export.render.to_string(),
                    export.post,
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

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Generation, GenerationId, Niche,
        ProfileRepository, Provider, TemplateBody, TemplateKind, TemplateRepository, TemplateUsed,
        TemplateVersion, Theme, ThemeIdea, ThemeRepository, TokenUsage, UiLanguage, UserProfile,
        VideoProject,
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

    /// A metadata generation, through a saved metadata template.
    fn generation(db: &Database, project: &VideoProject) -> Generation {
        let body = TemplateBody::new(TemplateKind::Metadata, "", "Posts for {{networks}}").unwrap();
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
            prompt: "Posts for youtube, tiktok".into(),
            output: "{\"posts\":[]}".into(),
            usage: TokenUsage {
                input_tokens: 900,
                output_tokens: 300,
            },
            generated_at: time(5_000),
            job: None,
        }
    }

    fn draft(title: &str, description: &str, tags: &[&str]) -> VideoMetadataDraft {
        VideoMetadataDraft {
            title: title.into(),
            description: description.into(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        }
    }

    #[test]
    fn metadata_round_trips_per_network_and_saving_again_replaces_it() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        assert_eq!(db.video_metadata(project.id).unwrap(), []);
        let generated = generation(&db, &project);
        let tiktok = VideoMetadata::generated(
            Network::TikTok,
            draft("", "The probe nobody found.", &["space", "nasa"]),
            generated.clone(),
            time(6_000),
        );
        let mut youtube = VideoMetadata::generated(
            Network::YouTube,
            draft("The lost probe", "What happened.", &["space history"]),
            generated,
            time(6_000),
        );
        db.save_video_metadata(&[tiktok.clone(), youtube.clone()])
            .unwrap();
        assert_eq!(
            db.video_metadata(project.id).unwrap(),
            [youtube.clone(), tiktok.clone()]
        );

        youtube.edit(draft("The probe", "Edited.", &[]), time(7_000));
        db.save_video_metadata(std::slice::from_ref(&youtube))
            .unwrap();
        assert_eq!(db.video_metadata(project.id).unwrap(), [youtube, tiktok]);
    }

    #[test]
    fn an_export_round_trips_and_a_new_one_replaces_it() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let mut export = Export {
            project: project.id,
            owner: project.owner,
            network: Network::Kick,
            account: NetworkAccountId::new(),
            package: "The lost probe (1a2b3c4d)".into(),
            video_file: "The lost probe.mp4".into(),
            render: RenderId::new(),
            post: "0123456789abcdef".into(),
            exported_at: time(8_000),
        };
        db.save_export(&export).unwrap();
        assert_eq!(db.exports(project.id).unwrap(), [export.clone()]);
        export.video_file = "Renamed.mp4".into();
        export.render = RenderId::new();
        export.exported_at = time(9_000);
        db.save_export(&export).unwrap();
        assert_eq!(db.exports(project.id).unwrap(), [export]);
        assert_eq!(db.exports(VideoProjectId::new()).unwrap(), []);
    }
}
