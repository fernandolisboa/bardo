use std::time::Duration;

use bardo_domain::{
    GenerationPresets, JobId, Narration, NarrationId, NarrationRepository, NarrationSource,
    ProfileId, Provider, RepositoryError, ScriptText, VideoProjectId, VoiceRef, WordTiming,
    WordTimings,
};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

#[derive(Debug, thiserror::Error)]
#[error("stored narration is invalid: {0}")]
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

/// A narration row as stored, before domain validation.
struct NarrationRow {
    id: String,
    profile_id: String,
    text: String,
    source: String,
    voice: [Option<String>; 3],
    presets: [Option<i64>; 4],
    billed_characters: Option<i64>,
    recording_name: Option<String>,
    aligner: Option<String>,
    model: String,
    audio_file: String,
    duration_ms: i64,
    generated_at: i64,
    job_id: Option<String>,
}

fn words(conn: &Connection, narration: &str) -> Result<Vec<WordTiming>, RepositoryError> {
    let mut statement = conn
        .prepare(
            "SELECT text_start, text_end, start_ms, end_ms FROM narration_word
             WHERE narration_id = ?1 ORDER BY position",
        )
        .map_err(boxed)?;
    let rows = statement
        .query_map([narration], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(boxed)?;
    rows.map(|row| {
        let (text_start, text_end, start, end) = row.map_err(boxed)?;
        Ok(WordTiming {
            text: index(text_start)?..index(text_end)?,
            start: duration(start)?,
            end: duration(end)?,
        })
    })
    .collect()
}

impl NarrationRow {
    fn source(&self) -> Result<NarrationSource, RepositoryError> {
        let missing = |column: &str| invalid(format!("{column} is missing"));
        match self.source.as_str() {
            "generated" => {
                let [provider, voice_id, voice_name] = &self.voice;
                let (Some(provider), Some(voice_id), Some(voice_name)) =
                    (provider, voice_id, voice_name)
                else {
                    return Err(missing("the voice"));
                };
                let provider: Provider = provider.parse().map_err(boxed)?;
                let voice = VoiceRef::new(provider, voice_id, voice_name).map_err(boxed)?;
                let [stability, similarity, style, speed] = self
                    .presets
                    .map(|value| value.map(|value| u8::try_from(value).unwrap_or(u8::MAX)));
                let (Some(stability), Some(similarity), Some(style), Some(speed)) =
                    (stability, similarity, style, speed)
                else {
                    return Err(missing("a preset"));
                };
                let billed = self
                    .billed_characters
                    .ok_or_else(|| missing("billed_characters"))?;
                Ok(NarrationSource::Generated {
                    voice,
                    presets: GenerationPresets {
                        stability,
                        similarity,
                        style,
                        speed,
                    },
                    model: self.model.clone(),
                    billed_characters: u64::try_from(billed).map_err(boxed)?,
                })
            }
            "imported" => Ok(NarrationSource::Imported {
                file_name: self
                    .recording_name
                    .clone()
                    .ok_or_else(|| missing("recording_name"))?,
                aligner: self
                    .aligner
                    .as_deref()
                    .ok_or_else(|| missing("aligner"))?
                    .parse()
                    .map_err(boxed)?,
                model: self.model.clone(),
            }),
            other => Err(invalid(format!("unknown source {other}"))),
        }
    }

    /// Rebuilds the narration through domain validation, so rows edited
    /// outside the app cannot smuggle in timings that do not fit the text.
    fn into_narration(
        self,
        conn: &Connection,
        project: VideoProjectId,
    ) -> Result<Narration, RepositoryError> {
        let text = ScriptText::new(&self.text).map_err(|error| invalid(format!("{error:?}")))?;
        let source = self.source()?;
        let words = WordTimings::restore(text.as_str(), words(conn, &self.id)?).map_err(boxed)?;
        Ok(Narration {
            id: NarrationId::from(uuid(&self.id)?),
            project,
            owner: ProfileId::from(uuid(&self.profile_id)?),
            text,
            source,
            audio_file: self.audio_file,
            duration: duration(self.duration_ms)?,
            words,
            generated_at: from_unix_millis(self.generated_at),
            job: self
                .job_id
                .as_deref()
                .map(uuid)
                .transpose()?
                .map(JobId::from),
        })
    }
}

impl NarrationRepository for Database {
    fn narration(&self, project: VideoProjectId) -> Result<Option<Narration>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT id, profile_id, text, source, voice_provider, voice_id, voice_name,
                     stability, similarity, style, speed, billed_characters, recording_name,
                     aligner, model, audio_file, duration_ms, generated_at, job_id
                 FROM narration WHERE project_id = ?1",
                [project.to_string()],
                |row| {
                    Ok(NarrationRow {
                        id: row.get(0)?,
                        profile_id: row.get(1)?,
                        text: row.get(2)?,
                        source: row.get(3)?,
                        voice: [row.get(4)?, row.get(5)?, row.get(6)?],
                        presets: [row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?],
                        billed_characters: row.get(11)?,
                        recording_name: row.get(12)?,
                        aligner: row.get(13)?,
                        model: row.get(14)?,
                        audio_file: row.get(15)?,
                        duration_ms: row.get(16)?,
                        generated_at: row.get(17)?,
                        job_id: row.get(18)?,
                    })
                },
            )
            .optional()
            .map_err(boxed)?;
        row.map(|row| row.into_narration(&conn, project))
            .transpose()
    }

    fn save_narration(&self, narration: &Narration) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id = narration.id.to_string();
        // Its words go with it (ON DELETE CASCADE).
        tx.execute(
            "DELETE FROM narration WHERE project_id = ?1 OR id = ?2",
            params![narration.project.to_string(), id],
        )
        .map_err(boxed)?;
        let (source, voice, presets, billed, recording, aligner) = match &narration.source {
            NarrationSource::Generated {
                voice,
                presets,
                billed_characters,
                ..
            } => (
                "generated",
                Some(voice),
                Some(presets),
                Some(i64::try_from(*billed_characters).map_err(boxed)?),
                None,
                None,
            ),
            NarrationSource::Imported {
                file_name, aligner, ..
            } => (
                "imported",
                None,
                None,
                None,
                Some(file_name.as_str()),
                Some(aligner.code()),
            ),
        };
        tx.execute(
            "INSERT INTO narration (project_id, id, profile_id, text, source, voice_provider,
                 voice_id, voice_name, stability, similarity, style, speed, billed_characters,
                 recording_name, aligner, model, audio_file, duration_ms, generated_at, job_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19, ?20)",
            params![
                narration.project.to_string(),
                id,
                narration.owner.to_string(),
                narration.text.as_str(),
                source,
                voice.map(|voice| voice.provider().code()),
                voice.map(VoiceRef::id),
                voice.map(VoiceRef::name),
                presets.map(|p| p.stability),
                presets.map(|p| p.similarity),
                presets.map(|p| p.style),
                presets.map(|p| p.speed),
                billed,
                recording,
                aligner,
                narration.source.model(),
                narration.audio_file,
                millis(narration.duration),
                to_unix_millis(narration.generated_at),
                narration.job.map(|job| job.to_string()),
            ],
        )
        .map_err(boxed)?;
        {
            let mut insert = tx
                .prepare(
                    "INSERT INTO narration_word
                         (narration_id, position, text_start, text_end, start_ms, end_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .map_err(boxed)?;
            let count = |n: usize| i64::try_from(n).map_err(boxed);
            for (position, word) in narration.words.as_slice().iter().enumerate() {
                insert
                    .execute(params![
                        id,
                        count(position)?,
                        count(word.text.start)?,
                        count(word.text.end)?,
                        millis(word.start),
                        millis(word.end),
                    ])
                    .map_err(boxed)?;
            }
        }
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use bardo_domain::{
        Alignment, Channel, ChannelDetails, ChannelDraft, ChannelRepository, CharTiming, Niche,
        ProfileRepository, Theme, ThemeIdea, ThemeRepository, UiLanguage, UserProfile,
        VideoProject,
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

    fn narration(project: &VideoProject, text: &str) -> Narration {
        let alignment = Alignment {
            chars: text
                .chars()
                .enumerate()
                .map(|(n, c)| CharTiming {
                    text: c.to_string(),
                    start: Duration::from_millis(n as u64 * 80),
                    end: Duration::from_millis(n as u64 * 80 + 80),
                })
                .collect(),
        };
        Narration {
            id: NarrationId::new(),
            project: project.id,
            owner: project.owner,
            text: ScriptText::new(text).unwrap(),
            source: NarrationSource::Generated {
                voice: VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap(),
                presets: GenerationPresets {
                    stability: 60,
                    similarity: 75,
                    style: 0,
                    speed: 100,
                },
                model: "eleven_multilingual_v2".into(),
                billed_characters: text.chars().count() as u64,
            },
            audio_file: "narration-1.mp3".into(),
            duration: Duration::from_millis(2_350),
            words: WordTimings::from_alignment(text, &alignment),
            generated_at: time(1_800_000_010),
            job: Some(JobId::new()),
        }
    }

    fn count(db: &Database, table: &str) -> i64 {
        db.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn a_narration_round_trips_with_its_word_timings() {
        let (db, project) = setup();
        let saved = narration(&project, "Era uma vez, em 1969, uma sonda — perdida.");

        db.save_narration(&saved).unwrap();

        assert_eq!(db.narration(project.id).unwrap(), Some(saved));
        assert_eq!(db.narration(VideoProjectId::new()).unwrap(), None);
    }

    #[test]
    fn an_imported_narration_round_trips_without_a_voice() {
        let (db, project) = setup();
        let mut imported = narration(&project, "Gravei eu mesmo, em 1969.");
        imported.source = NarrationSource::Imported {
            file_name: "take 3 (final).WAV".into(),
            aligner: Provider::ElevenLabs,
            model: "forced_alignment".into(),
        };
        imported.audio_file = format!("narration-{}.wav", imported.id);

        db.save_narration(&imported).unwrap();

        assert_eq!(db.narration(project.id).unwrap(), Some(imported));
        let voice: Option<String> = db
            .conn()
            .query_row("SELECT voice_id FROM narration", [], |row| row.get(0))
            .unwrap();
        assert_eq!(voice, None);
    }

    #[test]
    fn a_row_mixing_both_sources_is_refused_by_the_schema() {
        let (db, project) = setup();
        db.save_narration(&narration(&project, "One two three."))
            .unwrap();
        let mixed = db.conn().execute(
            "UPDATE narration SET recording_name = 'take.wav', aligner = 'elevenlabs'",
            [],
        );
        assert!(mixed.is_err());
        let half = db
            .conn()
            .execute("UPDATE narration SET source = 'imported'", []);
        assert!(half.is_err());
    }

    #[test]
    fn a_new_narration_replaces_the_old_one_and_its_words() {
        let (db, project) = setup();
        db.save_narration(&narration(&project, "One two three."))
            .unwrap();
        let newer = narration(&project, "Four five.");

        db.save_narration(&newer).unwrap();

        assert_eq!(db.narration(project.id).unwrap(), Some(newer));
        assert_eq!(count(&db, "narration"), 1);
        assert_eq!(count(&db, "narration_word"), 2);
    }

    #[test]
    fn timings_edited_outside_the_app_are_refused() {
        let (db, project) = setup();
        db.save_narration(&narration(&project, "One two three."))
            .unwrap();
        db.conn()
            .execute("UPDATE narration SET text = 'One two.'", [])
            .unwrap();
        assert!(db.narration(project.id).is_err());
    }
}
