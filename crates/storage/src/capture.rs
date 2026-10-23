use std::time::Duration;

use bardo_domain::{
    Capture, CaptureConfirmation, CaptureId, CaptureRepository, CaptureVideo, CaptureVoice, JobId,
    NarrationId, ProfileId, Provider, RepositoryError, Take, Takes, VideoProjectId, VoiceRef,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::narration::stored_presets;
use crate::{Database, boxed, from_unix_millis, to_unix_millis};

#[derive(Debug, thiserror::Error)]
#[error("stored capture is invalid: {0}")]
struct InvalidRow(String);

fn invalid(detail: impl Into<String>) -> RepositoryError {
    boxed(InvalidRow(detail.into()))
}

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn duration(millis: i64) -> Result<Duration, RepositoryError> {
    u64::try_from(millis)
        .map(Duration::from_millis)
        .map_err(boxed)
}

fn index(value: i64) -> Result<usize, RepositoryError> {
    usize::try_from(value).map_err(boxed)
}

fn count(value: usize) -> Result<i64, RepositoryError> {
    i64::try_from(value).map_err(boxed)
}

/// A capture row as stored, before domain validation.
struct CaptureRow {
    id: String,
    profile_id: String,
    file_name: String,
    video_file: String,
    size: [i64; 3],
    duration_ms: i64,
    confirmation: String,
    confirmed_at: i64,
    voice: String,
    voice_ref: [Option<String>; 3],
    presets: [Option<i64>; 4],
    narration_id: String,
    coverage: i64,
    imported_at: i64,
    job_id: Option<String>,
}

fn takes(conn: &Connection, project: VideoProjectId) -> Result<Vec<Take>, RepositoryError> {
    let mut statement = conn
        .prepare(
            "SELECT first_word, end_word, start_ms, end_ms, revision FROM capture_take
             WHERE project_id = ?1 ORDER BY position",
        )
        .map_err(boxed)?;
    let rows = statement
        .query_map([project.to_string()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(boxed)?;
    rows.map(|row| {
        let (first, end, start_ms, end_ms, revision) = row.map_err(boxed)?;
        Ok(Take {
            words: index(first)?..index(end)?,
            start: duration(start_ms)?,
            end: duration(end_ms)?,
            revision: u32::try_from(revision).map_err(boxed)?,
        })
    })
    .collect()
}

/// The narration's word count, which the takes must cover.
fn narration_words(conn: &Connection, narration: &str) -> Result<usize, RepositoryError> {
    let words: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM narration_word WHERE narration_id = ?1",
            [narration],
            |row| row.get(0),
        )
        .map_err(boxed)?;
    index(words)
}

impl CaptureRow {
    fn voice(&self) -> Result<CaptureVoice, RepositoryError> {
        match self.voice.as_str() {
            "own" => Ok(CaptureVoice::Own),
            "persona" => {
                let [provider, id, name] = &self.voice_ref;
                let (Some(provider), Some(id), Some(name)) = (provider, id, name) else {
                    return Err(invalid("the voice is missing"));
                };
                let provider: Provider = provider.parse().map_err(boxed)?;
                Ok(CaptureVoice::Persona {
                    voice: VoiceRef::new(provider, id, name).map_err(boxed)?,
                    presets: stored_presets(self.presets)
                        .ok_or_else(|| invalid("a preset is missing"))?,
                })
            }
            other => Err(invalid(format!("unknown voice {other}"))),
        }
    }

    /// Rebuilds the capture through domain validation, so rows edited
    /// outside the app cannot smuggle in takes that do not cover the
    /// narration. Takes of a narration that is gone (replaced) load as
    /// they are; the capture is no longer the project's narration then.
    fn into_capture(
        self,
        conn: &Connection,
        project: VideoProjectId,
    ) -> Result<Capture, RepositoryError> {
        let stored = takes(conn, project)?;
        let words = if narration_exists(conn, &self.narration_id)? {
            narration_words(conn, &self.narration_id)?
        } else {
            stored.last().map_or(0, |take| take.words.end)
        };
        let takes = Takes::restore(words, stored).map_err(boxed)?;
        let [width, height, fps_milli] = self.size.map(|value| u32::try_from(value).unwrap_or(0));
        Ok(Capture {
            id: CaptureId::from(uuid(&self.id)?),
            project,
            owner: ProfileId::from(uuid(&self.profile_id)?),
            voice: self.voice()?,
            file_name: self.file_name,
            video_file: self.video_file,
            video: CaptureVideo {
                width,
                height,
                fps_milli,
                duration: duration(self.duration_ms)?,
            },
            confirmation: CaptureConfirmation {
                text: self.confirmation,
                at: from_unix_millis(self.confirmed_at),
            },
            narration: NarrationId::from(uuid(&self.narration_id)?),
            coverage: u16::try_from(self.coverage).map_err(boxed)?,
            takes,
            imported_at: from_unix_millis(self.imported_at),
            job: self
                .job_id
                .as_deref()
                .map(uuid)
                .transpose()?
                .map(JobId::from),
        })
    }
}

fn narration_exists(conn: &Connection, narration: &str) -> Result<bool, RepositoryError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM narration WHERE id = ?1)",
        [narration],
        |row| row.get(0),
    )
    .map_err(boxed)
}

fn insert_takes(
    tx: &Transaction<'_>,
    project: VideoProjectId,
    takes: &Takes,
) -> Result<(), RepositoryError> {
    tx.execute(
        "DELETE FROM capture_take WHERE project_id = ?1",
        [project.to_string()],
    )
    .map_err(boxed)?;
    let mut insert = tx
        .prepare(
            "INSERT INTO capture_take
                 (project_id, position, first_word, end_word, start_ms, end_ms, revision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .map_err(boxed)?;
    for (position, take) in takes.as_slice().iter().enumerate() {
        insert
            .execute(params![
                project.to_string(),
                count(position)?,
                count(take.words.start)?,
                count(take.words.end)?,
                millis(take.start),
                millis(take.end),
                take.revision,
            ])
            .map_err(boxed)?;
    }
    Ok(())
}

impl CaptureRepository for Database {
    fn capture(&self, project: VideoProjectId) -> Result<Option<Capture>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT id, profile_id, file_name, video_file, width, height, fps_milli,
                     duration_ms, confirmation, confirmed_at, voice, voice_provider, voice_id,
                     voice_name, stability, similarity, style, speed, narration_id, coverage,
                     imported_at, job_id
                 FROM capture WHERE project_id = ?1",
                [project.to_string()],
                |row| {
                    Ok(CaptureRow {
                        id: row.get(0)?,
                        profile_id: row.get(1)?,
                        file_name: row.get(2)?,
                        video_file: row.get(3)?,
                        size: [row.get(4)?, row.get(5)?, row.get(6)?],
                        duration_ms: row.get(7)?,
                        confirmation: row.get(8)?,
                        confirmed_at: row.get(9)?,
                        voice: row.get(10)?,
                        voice_ref: [row.get(11)?, row.get(12)?, row.get(13)?],
                        presets: [row.get(14)?, row.get(15)?, row.get(16)?, row.get(17)?],
                        narration_id: row.get(18)?,
                        coverage: row.get(19)?,
                        imported_at: row.get(20)?,
                        job_id: row.get(21)?,
                    })
                },
            )
            .optional()
            .map_err(boxed)?;
        row.map(|row| row.into_capture(&conn, project)).transpose()
    }

    fn save_capture(&self, capture: &Capture) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        // Its takes go with it (ON DELETE CASCADE).
        tx.execute(
            "DELETE FROM capture WHERE project_id = ?1 OR id = ?2",
            params![capture.project.to_string(), capture.id.to_string()],
        )
        .map_err(boxed)?;
        let (voice, presets) = match &capture.voice {
            CaptureVoice::Persona { voice, presets } => (Some(voice), Some(presets)),
            CaptureVoice::Own => (None, None),
        };
        tx.execute(
            "INSERT INTO capture (project_id, id, profile_id, file_name, video_file, width,
                 height, fps_milli, duration_ms, confirmation, confirmed_at, voice,
                 voice_provider, voice_id, voice_name, stability, similarity, style, speed,
                 narration_id, coverage, imported_at, job_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19, ?20, ?21, ?22, ?23)",
            params![
                capture.project.to_string(),
                capture.id.to_string(),
                capture.owner.to_string(),
                capture.file_name,
                capture.video_file,
                capture.video.width,
                capture.video.height,
                capture.video.fps_milli,
                millis(capture.video.duration),
                capture.confirmation.text,
                to_unix_millis(capture.confirmation.at),
                capture.voice.code(),
                voice.map(|voice| voice.provider().code()),
                voice.map(VoiceRef::id),
                voice.map(VoiceRef::name),
                presets.map(|p| p.stability),
                presets.map(|p| p.similarity),
                presets.map(|p| p.style),
                presets.map(|p| p.speed),
                capture.narration.to_string(),
                capture.coverage,
                to_unix_millis(capture.imported_at),
                capture.job.map(|job| job.to_string()),
            ],
        )
        .map_err(boxed)?;
        insert_takes(&tx, capture.project, &capture.takes)?;
        tx.commit().map_err(boxed)
    }

    fn save_takes(&self, project: VideoProjectId, takes: &Takes) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM capture WHERE project_id = ?1)",
                [project.to_string()],
                |row| row.get(0),
            )
            .map_err(boxed)?;
        if !exists {
            return Err(invalid("the project has no capture"));
        }
        insert_takes(&tx, project, takes)?;
        tx.commit().map_err(boxed)
    }

    fn delete_capture(&self, project: VideoProjectId) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "DELETE FROM capture WHERE project_id = ?1",
                [project.to_string()],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use bardo_domain::{
        Alignment, Channel, ChannelDetails, ChannelDraft, ChannelRepository, CharTiming,
        GenerationPresets, Narration, NarrationRepository, NarrationSource, Niche,
        ProfileRepository, ScriptText, TakeSource, Theme, ThemeIdea, ThemeRepository, UiLanguage,
        UserProfile, VideoProject, WordTimings,
    };

    use super::*;

    fn time(secs: u64) -> SystemTime {
        // Whole milliseconds, as stored.
        SystemTime::UNIX_EPOCH + Duration::from_millis(secs * 1000 + 250)
    }

    fn setup() -> (Database, VideoProject) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Space Archives".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(profile.id, details);
        ChannelRepository::save(&db, &channel).unwrap();
        let mut theme = Theme::suggested(
            profile.id,
            channel.id,
            Niche::new("space history").unwrap(),
            ThemeIdea::new("The lost probe", "").unwrap(),
            time(1_800_000_000),
            0,
            None,
        );
        db.save_themes(std::slice::from_ref(&theme)).unwrap();
        let project = theme.approve(time(1_800_000_001)).unwrap();
        db.start_project(&theme, &project).unwrap();
        (db, project)
    }

    const TEXT: &str = "One two three. Four five six. Seven eight.";

    fn narration(project: &VideoProject) -> Narration {
        let alignment = Alignment {
            chars: TEXT
                .chars()
                .enumerate()
                .map(|(n, c)| CharTiming {
                    text: c.to_string(),
                    start: Duration::from_millis(n as u64 * 200),
                    end: Duration::from_millis(n as u64 * 200 + 200),
                })
                .collect(),
        };
        Narration {
            id: NarrationId::new(),
            project: project.id,
            owner: project.owner,
            text: ScriptText::new(TEXT).unwrap(),
            source: NarrationSource::Captured {
                file_name: "IMG_0042.MOV".into(),
                voice: CaptureVoice::Own,
                model: "audio_isolation".into(),
                aligner: Provider::ElevenLabs,
            },
            audio_file: "narration-1.mp3".into(),
            duration: Duration::from_millis(8_600),
            words: WordTimings::from_alignment(TEXT, &alignment),
            generated_at: time(1_800_000_010),
            job: Some(JobId::new()),
        }
    }

    fn capture(project: &VideoProject, narration: &Narration, voice: CaptureVoice) -> Capture {
        let takes = Takes::split(&TakeSource {
            text: TEXT,
            words: narration.words.as_slice(),
            duration: narration.duration,
        });
        Capture {
            id: CaptureId::new(),
            project: project.id,
            owner: project.owner,
            file_name: "IMG_0042.MOV".into(),
            video_file: "capture-1.mov".into(),
            video: CaptureVideo {
                width: 1920,
                height: 1080,
                fps_milli: 29_970,
                duration: Duration::from_millis(9_120),
            },
            confirmation: CaptureConfirmation {
                text: "This recording shows only me.".into(),
                at: time(1_800_000_005),
            },
            voice,
            narration: narration.id,
            coverage: 950,
            takes,
            imported_at: time(1_800_000_011),
            job: Some(JobId::new()),
        }
    }

    #[test]
    fn a_capture_round_trips_with_its_takes_and_voice() {
        let (db, project) = setup();
        let narration = narration(&project);
        db.save_narration(&narration).unwrap();
        let persona = CaptureVoice::Persona {
            voice: VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap(),
            presets: GenerationPresets {
                stability: 55,
                similarity: 80,
                style: 5,
                speed: 100,
            },
        };
        let saved = capture(&project, &narration, persona);
        assert!(!saved.takes.is_empty());

        db.save_capture(&saved).unwrap();
        assert_eq!(db.capture(project.id).unwrap(), Some(saved.clone()));
        assert_eq!(db.capture(VideoProjectId::new()).unwrap(), None);

        // A new capture replaces it and its takes.
        let own = capture(&project, &narration, CaptureVoice::Own);
        db.save_capture(&own).unwrap();
        assert_eq!(db.capture(project.id).unwrap(), Some(own));
        let rows: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM capture", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn takes_are_replaced_and_must_cover_the_narration() {
        let (db, project) = setup();
        let narration = narration(&project);
        db.save_narration(&narration).unwrap();
        let saved = capture(&project, &narration, CaptureVoice::Own);
        db.save_capture(&saved).unwrap();

        let mut takes = saved.takes.as_slice().to_vec();
        takes[0].revision = 7;
        let takes = Takes::restore(narration.words.len(), takes).unwrap();
        db.save_takes(project.id, &takes).unwrap();
        assert_eq!(db.capture(project.id).unwrap().unwrap().takes, takes);
        assert!(db.save_takes(VideoProjectId::new(), &takes).is_err());

        // Takes edited outside the app to leave words out are refused.
        db.conn()
            .execute(
                "DELETE FROM capture_take WHERE position = (SELECT MAX(position) FROM capture_take)",
                [],
            )
            .unwrap();
        assert!(db.capture(project.id).is_err());
    }

    #[test]
    fn a_capture_is_deleted_with_its_takes_and_outlives_a_replaced_narration() {
        let (db, project) = setup();
        let narration = narration(&project);
        db.save_narration(&narration).unwrap();
        let saved = capture(&project, &narration, CaptureVoice::Own);
        db.save_capture(&saved).unwrap();

        // Another narration replaces the capture's: the capture still loads.
        let mut other = narration.clone();
        other.id = NarrationId::new();
        db.save_narration(&other).unwrap();
        assert_eq!(db.capture(project.id).unwrap(), Some(saved));

        db.delete_capture(project.id).unwrap();
        assert_eq!(db.capture(project.id).unwrap(), None);
        let takes: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM capture_take", [], |row| row.get(0))
            .unwrap();
        assert_eq!(takes, 0);
    }

    #[test]
    fn a_persona_voice_without_its_presets_is_refused_by_the_schema() {
        let (db, project) = setup();
        let narration = narration(&project);
        db.save_narration(&narration).unwrap();
        db.save_capture(&capture(&project, &narration, CaptureVoice::Own))
            .unwrap();
        assert!(
            db.conn()
                .execute("UPDATE capture SET voice = 'persona'", [])
                .is_err()
        );
    }
}
