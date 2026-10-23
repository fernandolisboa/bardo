use std::time::{Duration, SystemTime};

use bardo_domain::{
    JobId, MediaAsset, MediaAssetId, MediaAssetRepository, MusicPrompt, MusicPromptRepository,
    MusicTrack, MusicTrackRepository, MusicTracks, PictureSize, ProfileId, RepositoryError,
    VideoProjectId,
};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::script::{generation, insert_generation};
use crate::{Database, boxed, from_unix_millis, to_unix_millis};

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

/// A media asset row as stored, before domain validation.
struct AssetRow {
    id: String,
    project_id: String,
    profile_id: String,
    kind: String,
    source: String,
    file: String,
    name: String,
    duration_ns: i64,
    width: Option<i64>,
    height: Option<i64>,
    imported_at: i64,
}

impl AssetRow {
    fn into_asset(self) -> Result<MediaAsset, RepositoryError> {
        let side = |value: i64| u32::try_from(value).map_err(boxed);
        let picture = match (self.width, self.height) {
            (Some(width), Some(height)) => Some(PictureSize::new(side(width)?, side(height)?)),
            _ => None,
        };
        Ok(MediaAsset {
            id: MediaAssetId::from(uuid(&self.id)?),
            project: VideoProjectId::from(uuid(&self.project_id)?),
            owner: ProfileId::from(uuid(&self.profile_id)?),
            kind: self.kind.parse().map_err(boxed)?,
            source: self.source.parse().map_err(boxed)?,
            file: self.file,
            name: self.name,
            duration: Duration::from_nanos(u64::try_from(self.duration_ns).map_err(boxed)?),
            picture,
            imported_at: from_unix_millis(self.imported_at),
        })
    }
}

impl MediaAssetRepository for Database {
    fn media_assets(&self, project: VideoProjectId) -> Result<Vec<MediaAsset>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT id, project_id, profile_id, kind, source, file, name, duration_ns, width,
                        height, imported_at
                 FROM media_asset WHERE project_id = ?1
                 ORDER BY imported_at, rowid",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok(AssetRow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    profile_id: row.get(2)?,
                    kind: row.get(3)?,
                    source: row.get(4)?,
                    file: row.get(5)?,
                    name: row.get(6)?,
                    duration_ns: row.get(7)?,
                    width: row.get(8)?,
                    height: row.get(9)?,
                    imported_at: row.get(10)?,
                })
            })
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?;
        rows.into_iter().map(AssetRow::into_asset).collect()
    }

    fn save_media_asset(&self, asset: &MediaAsset) -> Result<(), RepositoryError> {
        insert_asset(&self.conn(), asset)
    }
}

pub(crate) fn insert_asset(
    conn: &rusqlite::Connection,
    asset: &MediaAsset,
) -> Result<(), RepositoryError> {
    let duration = i64::try_from(asset.duration.as_nanos()).map_err(boxed)?;
    conn.execute(
        "INSERT INTO media_asset (id, project_id, profile_id, kind, source, file, name,
                                  duration_ns, width, height, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT (id) DO UPDATE SET name = excluded.name",
        params![
            asset.id.to_string(),
            asset.project.to_string(),
            asset.owner.to_string(),
            asset.kind.code(),
            asset.source.code(),
            asset.file,
            asset.name,
            duration,
            asset.picture.map(|picture| i64::from(picture.width)),
            asset.picture.map(|picture| i64::from(picture.height)),
            to_unix_millis(asset.imported_at),
        ],
    )
    .map_err(boxed)?;
    Ok(())
}

/// A music track row as stored, before domain validation.
struct TrackRow {
    asset_id: String,
    project_id: String,
    profile_id: String,
    provider: String,
    model: String,
    prompt: String,
    length_ms: i64,
    job_id: Option<String>,
    generated_at: i64,
    chosen_at: Option<i64>,
}

impl TrackRow {
    fn into_track(self) -> Result<MusicTrack, RepositoryError> {
        Ok(MusicTrack {
            asset: MediaAssetId::from(uuid(&self.asset_id)?),
            project: VideoProjectId::from(uuid(&self.project_id)?),
            owner: ProfileId::from(uuid(&self.profile_id)?),
            provider: self.provider.parse().map_err(boxed)?,
            model: self.model,
            prompt: self.prompt,
            length: Duration::from_millis(u64::try_from(self.length_ms).map_err(boxed)?),
            job: self
                .job_id
                .as_deref()
                .map(|job| uuid(job).map(JobId::from))
                .transpose()?,
            generated_at: from_unix_millis(self.generated_at),
            chosen_at: self.chosen_at.map(from_unix_millis),
        })
    }
}

/// Saves `track`, whose media asset is already saved.
pub(crate) fn insert_track(
    conn: &rusqlite::Connection,
    track: &MusicTrack,
) -> Result<(), RepositoryError> {
    conn.execute(
        "INSERT INTO music_track (asset_id, project_id, profile_id, provider, model, prompt,
                                  length_ms, job_id, generated_at, chosen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            track.asset.to_string(),
            track.project.to_string(),
            track.owner.to_string(),
            track.provider.code(),
            track.model,
            track.prompt,
            i64::try_from(track.length.as_millis()).map_err(boxed)?,
            track.job.map(|job| job.to_string()),
            to_unix_millis(track.generated_at),
            track.chosen_at.map(to_unix_millis),
        ],
    )
    .map_err(boxed)?;
    Ok(())
}

impl MusicTrackRepository for Database {
    fn music_tracks(&self, project: VideoProjectId) -> Result<MusicTracks, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT asset_id, project_id, profile_id, provider, model, prompt, length_ms,
                        job_id, generated_at, chosen_at
                 FROM music_track WHERE project_id = ?1
                 ORDER BY generated_at, rowid",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok(TrackRow {
                    asset_id: row.get(0)?,
                    project_id: row.get(1)?,
                    profile_id: row.get(2)?,
                    provider: row.get(3)?,
                    model: row.get(4)?,
                    prompt: row.get(5)?,
                    length_ms: row.get(6)?,
                    job_id: row.get(7)?,
                    generated_at: row.get(8)?,
                    chosen_at: row.get(9)?,
                })
            })
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?;
        let tracks = rows
            .into_iter()
            .map(TrackRow::into_track)
            .collect::<Result<_, _>>()?;
        Ok(MusicTracks::new(tracks))
    }

    fn add_music_track(
        &self,
        track: &MusicTrack,
        asset: &MediaAsset,
    ) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        insert_asset(&tx, asset)?;
        insert_track(&tx, track)?;
        tx.commit().map_err(boxed)
    }

    fn choose_music_track(
        &self,
        asset: MediaAssetId,
        at: SystemTime,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "UPDATE music_track SET chosen_at = ?2 WHERE asset_id = ?1",
                params![asset.to_string(), to_unix_millis(at)],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn remove_music_track(&self, asset: MediaAssetId) -> Result<(), RepositoryError> {
        // The track goes with its asset (ON DELETE CASCADE).
        self.conn()
            .execute("DELETE FROM media_asset WHERE id = ?1", [asset.to_string()])
            .map_err(boxed)?;
        Ok(())
    }
}

impl MusicPromptRepository for Database {
    fn music_prompt(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<MusicPrompt>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT profile_id, text, generation_id, updated_at
                 FROM music_prompt WHERE project_id = ?1",
                [project.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some((owner, text, generation_id, updated_at)) = row else {
            return Ok(None);
        };
        Ok(Some(MusicPrompt::restore(
            project,
            ProfileId::from(uuid(&owner)?),
            text,
            generation(&conn, &generation_id)?,
            from_unix_millis(updated_at),
        )))
    }

    fn save_music_prompt(&self, prompt: &MusicPrompt) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        insert_generation(&tx, prompt.generation())?;
        tx.execute(
            "INSERT INTO music_prompt (project_id, profile_id, text, generation_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (project_id) DO UPDATE SET
                 text = excluded.text,
                 generation_id = excluded.generation_id,
                 updated_at = excluded.updated_at",
            params![
                prompt.project.to_string(),
                prompt.owner.to_string(),
                prompt.text(),
                prompt.generation().id.to_string(),
                to_unix_millis(prompt.updated_at),
            ],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        AssetSource, Channel, ChannelDetails, ChannelDraft, ChannelRepository, Generation,
        GenerationId, MediaKind, Niche, ProfileRepository, Provider, TemplateBody, TemplateKind,
        TemplateRepository, TemplateUsed, TemplateVersion, Theme, ThemeIdea, ThemeRepository,
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

    fn asset(project: &VideoProject, kind: MediaKind, file: &str, at: u64) -> MediaAsset {
        MediaAsset {
            id: MediaAssetId::new(),
            project: project.id,
            owner: project.owner,
            kind,
            source: AssetSource::Imported,
            file: file.into(),
            name: format!("My {file}"),
            duration: Duration::from_nanos(12_345_678_901),
            picture: (kind == MediaKind::Video).then(|| PictureSize::new(1080, 1920)),
            imported_at: time(at),
        }
    }

    #[test]
    fn assets_round_trip_oldest_first_per_project() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let footage = asset(&project, MediaKind::Video, "media-b.mov", 2_000);
        let music = asset(&project, MediaKind::Audio, "media-a.mp3", 1_000);
        db.save_media_asset(&footage).unwrap();
        db.save_media_asset(&music).unwrap();
        assert_eq!(db.media_assets(project.id).unwrap(), [music, footage]);
        assert_eq!(db.media_assets(VideoProjectId::new()).unwrap(), []);
    }

    #[test]
    fn an_asset_needs_its_project_and_its_own_file() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let music = asset(&project, MediaKind::Audio, "media-a.mp3", 1_000);
        db.save_media_asset(&music).unwrap();
        let mut same_file = asset(&project, MediaKind::Audio, "media-a.mp3", 2_000);
        assert!(db.save_media_asset(&same_file).is_err());
        same_file.file = "media-c.mp3".into();
        same_file.project = VideoProjectId::new();
        assert!(db.save_media_asset(&same_file).is_err());
        // A video without its picture's size is refused.
        let mut video = asset(&project, MediaKind::Video, "media-d.mp4", 3_000);
        video.picture = None;
        assert!(db.save_media_asset(&video).is_err());
    }

    fn generation(db: &Database, project: &VideoProject, output: &str) -> Generation {
        let body =
            TemplateBody::new(TemplateKind::MusicPrompt, "", "Music for {{theme_title}}").unwrap();
        let version = match db
            .template_versions(project.owner, TemplateKind::MusicPrompt)
            .unwrap()
            .pop()
        {
            Some(version) => version,
            None => {
                let version = TemplateVersion::first(project.owner, body, time(1));
                db.add_template_version(&version).unwrap();
                version
            }
        };
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
            prompt: "Music for The lost probe".into(),
            output: output.into(),
            usage: TokenUsage {
                input_tokens: 120,
                output_tokens: 80,
            },
            generated_at: time(5_000),
            job: None,
        }
    }

    #[test]
    fn a_music_prompt_round_trips_and_generating_again_replaces_it() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        assert_eq!(db.music_prompt(project.id).unwrap(), None);
        let mut prompt = MusicPrompt::generated(
            generation(&db, &project, "Slow ambient synth, 70 BPM, no vocals."),
            time(6_000),
        )
        .unwrap();
        prompt
            .edit("Slow ambient synth, 60 BPM.", time(7_000))
            .unwrap();
        db.save_music_prompt(&prompt).unwrap();
        assert_eq!(db.music_prompt(project.id).unwrap(), Some(prompt));

        let again = MusicPrompt::generated(
            generation(&db, &project, "Tense strings, rising."),
            time(8_000),
        )
        .unwrap();
        db.save_music_prompt(&again).unwrap();
        assert_eq!(db.music_prompt(project.id).unwrap(), Some(again));
    }

    fn track(asset: &MediaAsset, generated: u64) -> MusicTrack {
        MusicTrack {
            asset: asset.id,
            project: asset.project,
            owner: asset.owner,
            provider: Provider::ElevenLabs,
            model: "music_v1".into(),
            prompt: "Calm piano, 70 BPM".into(),
            length: Duration::from_millis(95_123),
            job: Some(JobId::new()),
            generated_at: time(generated),
            chosen_at: None,
        }
    }

    #[test]
    fn a_track_is_saved_with_its_asset_chosen_and_removed_with_it() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let mut first_asset = asset(&project, MediaKind::Audio, "media-first.mp3", 1_000);
        first_asset.source = AssetSource::Generated;
        let mut second_asset = asset(&project, MediaKind::Audio, "media-second.mp3", 2_000);
        second_asset.source = AssetSource::Generated;
        let first = track(&first_asset, 1_000);
        let second = MusicTrack {
            job: None,
            ..track(&second_asset, 2_000)
        };
        db.add_music_track(&first, &first_asset).unwrap();
        db.add_music_track(&second, &second_asset).unwrap();
        assert_eq!(
            db.music_tracks(project.id).unwrap().all(),
            [first.clone(), second.clone()]
        );
        assert_eq!(
            db.media_assets(project.id).unwrap(),
            [first_asset.clone(), second_asset.clone()]
        );

        db.choose_music_track(first.asset, time(3_000)).unwrap();
        let tracks = db.music_tracks(project.id).unwrap();
        assert_eq!(tracks.in_use().map(|track| track.asset), Some(first.asset));
        assert_eq!(tracks.in_review(), Some(&second));

        db.remove_music_track(second.asset).unwrap();
        assert_eq!(db.music_tracks(project.id).unwrap().all().len(), 1);
        assert_eq!(db.media_assets(project.id).unwrap(), [first_asset]);
        assert_eq!(db.music_tracks(VideoProjectId::new()).unwrap().all(), []);
    }

    #[test]
    fn a_track_whose_asset_cannot_be_saved_leaves_nothing() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let music = asset(&project, MediaKind::Audio, "media-a.mp3", 1_000);
        db.save_media_asset(&music).unwrap();
        // Another asset with the same file name is refused, and its track
        // with it.
        let mut clash = asset(&project, MediaKind::Audio, "media-a.mp3", 2_000);
        clash.source = AssetSource::Generated;
        assert!(db.add_music_track(&track(&clash, 2_000), &clash).is_err());
        assert_eq!(db.music_tracks(project.id).unwrap().all(), []);
        assert_eq!(db.media_assets(project.id).unwrap(), [music]);
    }
}
