use std::time::Duration;

use std::time::SystemTime;

use bardo_domain::{
    AnalysisPart, Capture, CaptureConfirmation, CaptureId, CaptureRepository, CaptureVideo,
    CaptureVoice, ClipModelRef, ControlVideo, DepthRange, Device, JobId, Money, NarrationId,
    ProfileId, Provider, RepositoryError, SetSource, Take, TakeAnalysis, TakeChecks, TakeMode,
    TakePrompt, TakeResult, TakeSet, TakeStep, Takes, TemplateUsed, TemplateVersionId,
    VideoProjectId, VoiceRef,
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

pub(crate) fn model_ref(provider: &str, model: &str) -> Result<ClipModelRef, RepositoryError> {
    let provider: Provider = provider.parse().map_err(boxed)?;
    ClipModelRef::new(provider, model).map_err(boxed)
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
            "SELECT first_word, end_word, start_ms, end_ms, revision, mode, set_file, set_source,
                 take_provider, take_model
             FROM capture_take WHERE project_id = ?1 ORDER BY position",
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
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                (
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ),
            ))
        })
        .map_err(boxed)?;
    rows.map(|row| {
        let (first, end, start_ms, end_ms, revision, mode, set_file, set_source, model) =
            row.map_err(boxed)?;
        let set = match (set_file, set_source) {
            (None, None) => TakeSet::Room,
            (Some(file), Some(source)) => TakeSet::Image {
                file,
                from: SetSource::from_code(&source)
                    .ok_or_else(|| invalid(format!("unknown set source {source}")))?,
            },
            _ => return Err(invalid("a set image without its source")),
        };
        Ok(Take {
            words: index(first)?..index(end)?,
            start: duration(start_ms)?,
            end: duration(end_ms)?,
            revision: u32::try_from(revision).map_err(boxed)?,
            mode: TakeMode::from_code(&mode)
                .ok_or_else(|| invalid(format!("unknown take mode {mode}")))?,
            set,
            model: match model {
                (Some(provider), Some(model)) => Some(model_ref(&provider, &model)?),
                _ => None,
            },
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
                 (project_id, position, first_word, end_word, start_ms, end_ms, revision, mode,
                  set_file, set_source, take_provider, take_model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                take.mode.code(),
                take.set.image(),
                match &take.set {
                    TakeSet::Room => None,
                    TakeSet::Image { from, .. } => Some(from.code()),
                },
                take.model.as_ref().map(|model| model.provider().code()),
                take.model.as_ref().map(ClipModelRef::model),
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

    fn save_takes(
        &self,
        project: VideoProjectId,
        capture: CaptureId,
        takes: &Takes,
    ) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM capture WHERE project_id = ?1 AND id = ?2)",
                [project.to_string(), capture.to_string()],
                |row| row.get(0),
            )
            .map_err(boxed)?;
        if !exists {
            return Err(invalid("the project's capture is not this one"));
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

    fn analyses(&self, project: VideoProjectId) -> Result<Vec<TakeAnalysis>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT capture_id, position, part, revision, device, fallback, model, frames,
                     no_face, several_faces, small_face, turned, mask_jumps, depth_far,
                     depth_near, finished_at, job_id
                 FROM take_analysis WHERE project_id = ?1
                 ORDER BY position, part = 'depth'",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok(AnalysisRow {
                    capture: row.get(0)?,
                    position: row.get(1)?,
                    part: row.get(2)?,
                    revision: row.get(3)?,
                    device: row.get(4)?,
                    fallback: row.get(5)?,
                    model: row.get(6)?,
                    checks: [
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                    ],
                    depth: [row.get(13)?, row.get(14)?],
                    finished_at: row.get(15)?,
                    job: row.get(16)?,
                })
            })
            .map_err(boxed)?;
        rows.map(|row| row.map_err(boxed)?.into_analysis(project))
            .collect()
    }

    fn save_analysis(&self, analysis: &TakeAnalysis) -> Result<bool, RepositoryError> {
        let (device, fallback) = match &analysis.device {
            Device::Gpu => ("gpu", None),
            Device::Cpu { fallback } => ("cpu", fallback.as_deref()),
        };
        let checks = &analysis.checks;
        // Kept only while the take is still the one analyzed.
        let saved = self
            .conn()
            .execute(
                "INSERT OR REPLACE INTO take_analysis (project_id, capture_id, position, part,
                     revision, device, fallback, model, frames, no_face, several_faces,
                     small_face, turned, mask_jumps, depth_far, depth_near, finished_at, job_id)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18
                 WHERE EXISTS (
                     SELECT 1 FROM capture c JOIN capture_take t ON t.project_id = c.project_id
                     WHERE c.project_id = ?1 AND c.id = ?2 AND t.position = ?3
                         AND t.revision = ?5)",
                params![
                    analysis.project.to_string(),
                    analysis.capture.to_string(),
                    analysis.position,
                    analysis.part.code(),
                    analysis.revision,
                    device,
                    fallback,
                    analysis.model,
                    checks.frames,
                    checks.no_face,
                    checks.several_faces,
                    checks.small_face,
                    checks.turned,
                    checks.mask_jumps,
                    analysis.depth_range.map(|range| range.far),
                    analysis.depth_range.map(|range| range.near),
                    to_unix_millis(analysis.finished_at),
                    analysis.job.map(|job| job.to_string()),
                ],
            )
            .map_err(boxed)?;
        Ok(saved == 1)
    }

    fn control_videos(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<ControlVideo>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT capture_id, position, fingerprint, file, bytes, frames, uncovered,
                     encoder, built_at, job_id, approved_at, replaced_approval
                 FROM control_video WHERE project_id = ?1 ORDER BY position",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    [row.get::<_, i64>(4)?, row.get(5)?, row.get(6)?],
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    (row.get::<_, Option<i64>>(10)?, row.get::<_, bool>(11)?),
                ))
            })
            .map_err(boxed)?;
        rows.map(|row| {
            let (capture, position, fingerprint, file, sizes, encoder, built_at, job, approval) =
                row.map_err(boxed)?;
            let (approved_at, replaced_approval) = approval;
            let [bytes, frames, uncovered] = sizes;
            Ok(ControlVideo {
                project,
                capture: CaptureId::from(uuid(&capture)?),
                position: u32::try_from(position).map_err(boxed)?,
                fingerprint,
                file,
                bytes: u64::try_from(bytes).map_err(boxed)?,
                frames: u32::try_from(frames).map_err(boxed)?,
                uncovered: u32::try_from(uncovered).map_err(boxed)?,
                encoder,
                built_at: from_unix_millis(built_at),
                job: job.as_deref().map(uuid).transpose()?.map(JobId::from),
                approved_at: approved_at.map(from_unix_millis),
                replaced_approval,
            })
        })
        .collect()
    }

    fn save_control_video(&self, video: &ControlVideo) -> Result<bool, RepositoryError> {
        // Kept only while the take is still there on the project's capture.
        // An approval given meanwhile to a build of the same take, as it
        // is, carries over.
        let saved = self
            .conn()
            .execute(
                "INSERT OR REPLACE INTO control_video (project_id, capture_id, position,
                     fingerprint, file, bytes, frames, uncovered, encoder, built_at, job_id,
                     approved_at, replaced_approval)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                     COALESCE(?12, (SELECT approved_at FROM control_video
                         WHERE project_id = ?1 AND capture_id = ?2 AND position = ?3
                             AND fingerprint = ?4)),
                     ?13
                 WHERE EXISTS (
                     SELECT 1 FROM capture c JOIN capture_take t ON t.project_id = c.project_id
                     WHERE c.project_id = ?1 AND c.id = ?2 AND t.position = ?3)",
                params![
                    video.project.to_string(),
                    video.capture.to_string(),
                    video.position,
                    video.fingerprint,
                    video.file,
                    i64::try_from(video.bytes).unwrap_or(i64::MAX),
                    video.frames,
                    video.uncovered,
                    video.encoder,
                    to_unix_millis(video.built_at),
                    video.job.map(|job| job.to_string()),
                    video.approved_at.map(to_unix_millis),
                    video.replaced_approval,
                ],
            )
            .map_err(boxed)?;
        Ok(saved == 1)
    }

    fn approve_control_video(
        &self,
        project: VideoProjectId,
        position: u32,
        fingerprint: &str,
        at: SystemTime,
    ) -> Result<bool, RepositoryError> {
        let approved = self
            .conn()
            .execute(
                "UPDATE control_video SET approved_at = ?4, replaced_approval = 0
                 WHERE project_id = ?1 AND position = ?2 AND fingerprint = ?3",
                params![
                    project.to_string(),
                    position,
                    fingerprint,
                    to_unix_millis(at)
                ],
            )
            .map_err(boxed)?;
        Ok(approved == 1)
    }

    fn take_prompts(&self, project: VideoProjectId) -> Result<Vec<TakePrompt>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT capture_id, position, control, take_provider, take_model, text,
                     template_id, template_number, writer, written_at, job_id, edited
                 FROM take_prompt WHERE project_id = ?1 ORDER BY position",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok((
                    [
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(8)?,
                    ],
                    row.get::<_, i64>(1)?,
                    (
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ),
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, bool>(11)?,
                ))
            })
            .map_err(boxed)?;
        rows.map(|row| {
            let (texts, position, template, written_at, job, edited) = row.map_err(boxed)?;
            let [capture, control, provider, model, text, writer] = texts;
            Ok(TakePrompt {
                project,
                capture: CaptureId::from(uuid(&capture)?),
                position: u32::try_from(position).map_err(boxed)?,
                control,
                model: model_ref(&provider, &model)?,
                text,
                template: template_used(template)?,
                writer,
                written_at: from_unix_millis(written_at),
                job: job.as_deref().map(uuid).transpose()?.map(JobId::from),
                edited,
            })
        })
        .collect()
    }

    fn save_take_prompt(&self, prompt: &TakePrompt) -> Result<bool, RepositoryError> {
        // Kept only while the take is still there on the project's capture.
        let saved = self
            .conn()
            .execute(
                "INSERT OR REPLACE INTO take_prompt (project_id, capture_id, position, control,
                     take_provider, take_model, text, template_id, template_number, writer,
                     written_at, job_id, edited)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
                 WHERE EXISTS (
                     SELECT 1 FROM capture c JOIN capture_take t ON t.project_id = c.project_id
                     WHERE c.project_id = ?1 AND c.id = ?2 AND t.position = ?3)",
                params![
                    prompt.project.to_string(),
                    prompt.capture.to_string(),
                    prompt.position,
                    prompt.control,
                    prompt.model.provider().code(),
                    prompt.model.model(),
                    prompt.text,
                    prompt.template.map(|t| t.id.to_string()),
                    prompt.template.map(|t| t.number),
                    prompt.writer,
                    to_unix_millis(prompt.written_at),
                    prompt.job.map(|job| job.to_string()),
                    prompt.edited,
                ],
            )
            .map_err(boxed)?;
        Ok(saved == 1)
    }

    fn take_results(&self, project: VideoProjectId) -> Result<Vec<TakeResult>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT capture_id, position, step, control, take_provider, take_model,
                     priced_model, prompt, template_id, template_number, look, realistic,
                     request, cost_micros, file, duration_ms, made_at, job_id, accepted_at
                 FROM take_result WHERE project_id = ?1 ORDER BY position, step = 'final'",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok(ResultRow {
                    texts: [
                        row.get(0)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(10)?,
                        row.get(12)?,
                        row.get(14)?,
                    ],
                    position: row.get(1)?,
                    template: (row.get(8)?, row.get(9)?),
                    realistic: row.get(11)?,
                    cost_micros: row.get(13)?,
                    duration_ms: row.get(15)?,
                    made_at: row.get(16)?,
                    job: row.get(17)?,
                    accepted_at: row.get(18)?,
                })
            })
            .map_err(boxed)?;
        rows.map(|row| row.map_err(boxed)?.into_result(project))
            .collect()
    }

    fn save_take_result(&self, result: &TakeResult) -> Result<bool, RepositoryError> {
        // Kept only while the take is still there on the project's capture.
        let saved = self
            .conn()
            .execute(
                "INSERT OR REPLACE INTO take_result (project_id, capture_id, position, step,
                     control, take_provider, take_model, priced_model, prompt, template_id,
                     template_number, look, realistic, request, cost_micros, file, duration_ms,
                     made_at, job_id, accepted_at)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19, ?20
                 WHERE EXISTS (
                     SELECT 1 FROM capture c JOIN capture_take t ON t.project_id = c.project_id
                     WHERE c.project_id = ?1 AND c.id = ?2 AND t.position = ?3)",
                params![
                    result.project.to_string(),
                    result.capture.to_string(),
                    result.position,
                    result.step.code(),
                    result.control,
                    result.model.provider().code(),
                    result.model.model(),
                    result.priced_model,
                    result.prompt,
                    result.template.map(|t| t.id.to_string()),
                    result.template.map(|t| t.number),
                    result.look,
                    result.realistic,
                    result.request,
                    result
                        .cost
                        .map(|cost| i64::try_from(cost.micros()).unwrap_or(i64::MAX)),
                    result.file,
                    millis(result.duration).max(1),
                    to_unix_millis(result.made_at),
                    result.job.map(|job| job.to_string()),
                    result.accepted_at.map(to_unix_millis),
                ],
            )
            .map_err(boxed)?;
        Ok(saved == 1)
    }

    fn accept_take_result(
        &self,
        project: VideoProjectId,
        position: u32,
        step: TakeStep,
        control: &str,
        at: Option<SystemTime>,
    ) -> Result<bool, RepositoryError> {
        let changed = self
            .conn()
            .execute(
                "UPDATE take_result SET accepted_at = ?5
                 WHERE project_id = ?1 AND position = ?2 AND step = ?3 AND control = ?4",
                params![
                    project.to_string(),
                    position,
                    step.code(),
                    control,
                    at.map(to_unix_millis)
                ],
            )
            .map_err(boxed)?;
        Ok(changed == 1)
    }
}

/// The template version of a prompt or result, as stored.
fn template_used(
    (id, number): (Option<String>, Option<i64>),
) -> Result<Option<TemplateUsed>, RepositoryError> {
    match (id, number) {
        (Some(id), Some(number)) => Ok(Some(TemplateUsed {
            id: TemplateVersionId::from(uuid(&id)?),
            number: u32::try_from(number).map_err(boxed)?,
        })),
        (None, None) => Ok(None),
        _ => Err(invalid("a template without its number")),
    }
}

/// A take result row as stored.
struct ResultRow {
    /// capture, step, control, provider, model, priced model, prompt,
    /// look, request, file.
    texts: [String; 10],
    position: i64,
    template: (Option<String>, Option<i64>),
    realistic: bool,
    cost_micros: Option<i64>,
    duration_ms: i64,
    made_at: i64,
    job: Option<String>,
    accepted_at: Option<i64>,
}

impl ResultRow {
    fn into_result(self, project: VideoProjectId) -> Result<TakeResult, RepositoryError> {
        let [
            capture,
            step,
            control,
            provider,
            model,
            priced_model,
            prompt,
            look,
            request,
            file,
        ] = self.texts;
        Ok(TakeResult {
            project,
            capture: CaptureId::from(uuid(&capture)?),
            position: u32::try_from(self.position).map_err(boxed)?,
            step: TakeStep::from_code(&step)
                .ok_or_else(|| invalid(format!("unknown take step {step}")))?,
            control,
            model: model_ref(&provider, &model)?,
            priced_model,
            prompt,
            template: template_used(self.template)?,
            look,
            realistic: self.realistic,
            request,
            cost: self
                .cost_micros
                .map(|micros| u64::try_from(micros).map(Money::from_micros))
                .transpose()
                .map_err(boxed)?,
            file,
            duration: duration(self.duration_ms)?,
            made_at: from_unix_millis(self.made_at),
            job: self.job.as_deref().map(uuid).transpose()?.map(JobId::from),
            accepted_at: self.accepted_at.map(from_unix_millis),
        })
    }
}

/// A take analysis row as stored.
struct AnalysisRow {
    capture: String,
    position: i64,
    part: String,
    revision: i64,
    device: String,
    fallback: Option<String>,
    model: String,
    checks: [i64; 6],
    depth: [Option<i64>; 2],
    finished_at: i64,
    job: Option<String>,
}

impl AnalysisRow {
    fn into_analysis(self, project: VideoProjectId) -> Result<TakeAnalysis, RepositoryError> {
        let frames = |value: i64| u32::try_from(value).map_err(boxed);
        let [all, no_face, several_faces, small_face, turned, mask_jumps] = self.checks;
        let depth_range = match self.depth {
            [Some(far), Some(near)] => Some(DepthRange {
                far: u16::try_from(far).map_err(boxed)?,
                near: u16::try_from(near).map_err(boxed)?,
            }),
            _ => None,
        };
        Ok(TakeAnalysis {
            project,
            capture: CaptureId::from(uuid(&self.capture)?),
            position: frames(self.position)?,
            revision: frames(self.revision)?,
            part: AnalysisPart::from_code(&self.part)
                .ok_or_else(|| invalid(format!("unknown analysis part {}", self.part)))?,
            device: match self.device.as_str() {
                "gpu" => Device::Gpu,
                "cpu" => Device::Cpu {
                    fallback: self.fallback,
                },
                other => return Err(invalid(format!("unknown device {other}"))),
            },
            model: self.model,
            checks: TakeChecks {
                frames: frames(all)?,
                no_face: frames(no_face)?,
                several_faces: frames(several_faces)?,
                small_face: frames(small_face)?,
                turned: frames(turned)?,
                mask_jumps: frames(mask_jumps)?,
            },
            depth_range,
            finished_at: from_unix_millis(self.finished_at),
            job: self.job.as_deref().map(uuid).transpose()?.map(JobId::from),
        })
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
        db.save_takes(project.id, saved.id, &takes).unwrap();
        assert_eq!(db.capture(project.id).unwrap().unwrap().takes, takes);
        assert!(
            db.save_takes(VideoProjectId::new(), saved.id, &takes)
                .is_err()
        );
        // Nor onto a capture that replaced the one they were cut from.
        assert!(db.save_takes(project.id, CaptureId::new(), &takes).is_err());

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

    fn analysis(saved: &Capture, position: u32, part: AnalysisPart) -> TakeAnalysis {
        TakeAnalysis {
            project: saved.project,
            capture: saved.id,
            position,
            revision: saved.takes.as_slice()[position as usize].revision,
            part,
            device: Device::Cpu {
                fallback: Some("No devices detected".into()),
            },
            model: "MediaPipe Face Landmarker float16/1".into(),
            checks: TakeChecks {
                frames: 90,
                no_face: 2,
                several_faces: 1,
                small_face: 3,
                turned: 4,
                mask_jumps: 5,
            },
            depth_range: (part == AnalysisPart::Depth).then_some(DepthRange {
                far: 1_200,
                near: 60_000,
            }),
            finished_at: time(1_800_000_020),
            job: Some(JobId::new()),
        }
    }

    #[test]
    fn take_modes_and_analyses_round_trip_while_the_take_is_the_one_analyzed() {
        let (db, project) = setup();
        let narration = narration(&project);
        db.save_narration(&narration).unwrap();
        let saved = capture(&project, &narration, CaptureVoice::Own);
        db.save_capture(&saved).unwrap();

        let mut takes = saved.takes.clone();
        takes.set_mode(0, TakeMode::Depth).unwrap();
        db.save_takes(project.id, saved.id, &takes).unwrap();
        let loaded = db.capture(project.id).unwrap().unwrap();
        assert_eq!(loaded.takes.as_slice()[0].mode, TakeMode::Depth);

        let face = analysis(&saved, 0, AnalysisPart::Face);
        let depth = analysis(&saved, 0, AnalysisPart::Depth);
        assert!(db.save_analysis(&face).unwrap());
        assert!(db.save_analysis(&depth).unwrap());
        assert_eq!(
            db.analyses(project.id).unwrap(),
            [face.clone(), depth.clone()]
        );
        // On the graphics card, and done again: replaced.
        let again = TakeAnalysis {
            device: Device::Gpu,
            ..face.clone()
        };
        assert!(db.save_analysis(&again).unwrap());
        assert_eq!(db.analyses(project.id).unwrap(), [again, depth]);

        // Not kept for a revision the take moved past, nor another capture.
        let old = TakeAnalysis {
            revision: face.revision + 1,
            ..face.clone()
        };
        assert!(!db.save_analysis(&old).unwrap());
        let other = TakeAnalysis {
            capture: CaptureId::new(),
            ..face.clone()
        };
        assert!(!db.save_analysis(&other).unwrap());

        // Moving cuts keeps them (stale by revision); a new capture or
        // deleting it removes them.
        db.save_takes(project.id, saved.id, &takes).unwrap();
        assert_eq!(db.analyses(project.id).unwrap().len(), 2);
        db.save_capture(&capture(&project, &narration, CaptureVoice::Own))
            .unwrap();
        assert!(db.analyses(project.id).unwrap().is_empty());
        let saved = db.capture(project.id).unwrap().unwrap();
        assert!(
            db.save_analysis(&analysis(&saved, 0, AnalysisPart::Face))
                .unwrap()
        );
        db.delete_capture(project.id).unwrap();
        assert!(db.analyses(project.id).unwrap().is_empty());
    }

    fn control(saved: &Capture, position: u32, fingerprint: &str) -> ControlVideo {
        ControlVideo {
            project: saved.project,
            capture: saved.id,
            position,
            fingerprint: fingerprint.into(),
            file: format!("control-{fingerprint}.mp4"),
            bytes: 45_000_000,
            frames: 900,
            uncovered: 3,
            encoder: "h264_nvenc".into(),
            built_at: time(1_800_000_030),
            job: Some(JobId::new()),
            approved_at: None,
            replaced_approval: false,
        }
    }

    #[test]
    fn take_sets_round_trip_and_must_name_their_source() {
        let (db, project) = setup();
        let narration = narration(&project);
        db.save_narration(&narration).unwrap();
        let saved = capture(&project, &narration, CaptureVoice::Own);
        db.save_capture(&saved).unwrap();
        assert_eq!(saved.takes.as_slice()[0].set, TakeSet::Room);

        let mut takes = saved.takes.clone();
        let set = TakeSet::Image {
            file: "set-1.png".into(),
            from: SetSource::Scene,
        };
        takes.set_set(0, set.clone()).unwrap();
        db.save_takes(project.id, saved.id, &takes).unwrap();
        let loaded = db.capture(project.id).unwrap().unwrap();
        assert_eq!(loaded.takes.as_slice()[0].set, set);

        // A file without where it came from is refused by the schema.
        assert!(
            db.conn()
                .execute("UPDATE capture_take SET set_source = NULL", [])
                .is_err()
        );
        assert!(
            db.conn()
                .execute("UPDATE capture_take SET set_source = 'elsewhere'", [])
                .is_err()
        );
        assert!(
            db.conn()
                .execute("UPDATE capture_take SET set_file = NULL", [])
                .is_err()
        );
    }

    #[test]
    fn a_control_video_is_approved_only_for_its_fingerprint_and_goes_with_its_capture() {
        let (db, project) = setup();
        let narration = narration(&project);
        db.save_narration(&narration).unwrap();
        let saved = capture(&project, &narration, CaptureVoice::Own);
        db.save_capture(&saved).unwrap();

        let first = control(&saved, 0, "aaaa");
        assert!(db.save_control_video(&first).unwrap());
        assert_eq!(
            db.control_videos(project.id).unwrap(),
            std::slice::from_ref(&first)
        );
        // Not for a fingerprint it was not built from.
        assert!(
            !db.approve_control_video(project.id, 0, "bbbb", time(1_800_000_040))
                .unwrap()
        );
        assert!(
            db.approve_control_video(project.id, 0, "aaaa", time(1_800_000_040))
                .unwrap()
        );
        let approved = db.control_videos(project.id).unwrap().remove(0);
        assert_eq!(approved.approved_at, Some(time(1_800_000_040)));
        assert!(approved.approved_for("aaaa"));

        // Another build of the same take, started before the approval,
        // keeps it.
        let again = ControlVideo {
            file: "again.mp4".into(),
            ..first.clone()
        };
        assert!(db.save_control_video(&again).unwrap());
        let kept = db.control_videos(project.id).unwrap().remove(0);
        assert_eq!(kept.file, "again.mp4");
        assert_eq!(kept.approved_at, Some(time(1_800_000_040)));

        // A new build replaces it, as the app says: unapproved, its
        // approval lost with the take it was for.
        let second = ControlVideo {
            replaced_approval: true,
            ..control(&saved, 0, "bbbb")
        };
        assert!(db.save_control_video(&second).unwrap());
        assert_eq!(db.control_videos(project.id).unwrap(), [second]);
        // Approving it clears that.
        assert!(
            db.approve_control_video(project.id, 0, "bbbb", time(1_800_000_050))
                .unwrap()
        );
        let approved = db.control_videos(project.id).unwrap().remove(0);
        assert!(!approved.replaced_approval);
        assert_eq!(approved.approved_at, Some(time(1_800_000_050)));

        // Not kept for a take or capture that is not there.
        assert!(!db.save_control_video(&control(&saved, 99, "cccc")).unwrap());
        let other = ControlVideo {
            capture: CaptureId::new(),
            ..control(&saved, 0, "dddd")
        };
        assert!(!db.save_control_video(&other).unwrap());

        // Moving cuts keeps it (stale by fingerprint); deleting the capture
        // removes it.
        db.save_takes(project.id, saved.id, &saved.takes).unwrap();
        assert_eq!(db.control_videos(project.id).unwrap().len(), 1);
        db.delete_capture(project.id).unwrap();
        assert!(db.control_videos(project.id).unwrap().is_empty());
    }

    fn seedance() -> ClipModelRef {
        ClipModelRef::new(Provider::Higgsfield, "bytedance/seedance-2.5/video-edit").unwrap()
    }

    fn prompt(saved: &Capture, position: u32, control: &str) -> TakePrompt {
        TakePrompt {
            project: saved.project,
            capture: saved.id,
            position,
            control: control.into(),
            model: seedance(),
            text: "The character in the image talks to the camera.".into(),
            template: None,
            writer: "claude-opus-5-5".into(),
            written_at: time(1_800_000_060),
            job: Some(JobId::new()),
            edited: false,
        }
    }

    fn result(saved: &Capture, position: u32, step: TakeStep, control: &str) -> TakeResult {
        TakeResult {
            project: saved.project,
            capture: saved.id,
            position,
            step,
            control: control.into(),
            model: seedance(),
            priced_model: format!("bytedance/seedance-2.5/video-edit/{}", step.code()),
            prompt: "The character in the image talks to the camera.".into(),
            template: None,
            look: "look-1.png".into(),
            realistic: true,
            request: "d7e6c0f3".into(),
            cost: Some(Money::from_micros(2_460_000)),
            file: format!("take-{}.mp4", step.code()),
            duration: Duration::from_millis(3_010),
            made_at: time(1_800_000_070),
            job: Some(JobId::new()),
            accepted_at: None,
        }
    }

    #[test]
    fn a_takes_model_prompt_and_results_round_trip_and_go_with_its_capture() {
        let (db, project) = setup();
        let narration = narration(&project);
        db.save_narration(&narration).unwrap();
        let mut saved = capture(&project, &narration, CaptureVoice::Own);
        saved
            .takes
            .set_model(0, Some(seedance()), TakeMode::Mesh)
            .unwrap();
        db.save_capture(&saved).unwrap();
        let loaded = db.capture(project.id).unwrap().unwrap();
        assert_eq!(loaded.takes.as_slice()[0].model, Some(seedance()));

        let written = prompt(&saved, 0, "aaaa");
        assert!(db.save_take_prompt(&written).unwrap());
        assert_eq!(
            db.take_prompts(project.id).unwrap(),
            std::slice::from_ref(&written)
        );
        // Writing again replaces it.
        let edited = TakePrompt {
            text: "Edited.".into(),
            edited: true,
            ..written
        };
        assert!(db.save_take_prompt(&edited).unwrap());
        assert_eq!(db.take_prompts(project.id).unwrap(), [edited]);

        let draft = result(&saved, 0, TakeStep::Draft, "aaaa");
        let final_take = result(&saved, 0, TakeStep::Final, "aaaa");
        assert!(db.save_take_result(&final_take).unwrap());
        assert!(db.save_take_result(&draft).unwrap());
        assert_eq!(
            db.take_results(project.id).unwrap(),
            [draft.clone(), final_take.clone()],
            "the draft first"
        );
        // Approved only for the control video it was made from.
        assert!(
            !db.accept_take_result(
                project.id,
                0,
                TakeStep::Draft,
                "bbbb",
                Some(time(1_800_000_080))
            )
            .unwrap()
        );
        assert!(
            db.accept_take_result(
                project.id,
                0,
                TakeStep::Draft,
                "aaaa",
                Some(time(1_800_000_080))
            )
            .unwrap()
        );
        assert_eq!(
            db.take_results(project.id).unwrap()[0].accepted_at,
            Some(time(1_800_000_080))
        );
        // A new draft replaces the approved one.
        assert!(db.save_take_result(&draft).unwrap());
        assert_eq!(db.take_results(project.id).unwrap()[0].accepted_at, None);

        // Not kept for a take or capture that is not there.
        assert!(!db.save_take_prompt(&prompt(&saved, 99, "aaaa")).unwrap());
        assert!(
            !db.save_take_result(&result(&saved, 99, TakeStep::Draft, "aaaa"))
                .unwrap()
        );
        let other = TakeResult {
            capture: CaptureId::new(),
            ..draft
        };
        assert!(!db.save_take_result(&other).unwrap());

        // A take model without its provider is refused by the schema.
        assert!(
            db.conn()
                .execute(
                    "UPDATE capture_take SET take_provider = NULL WHERE position = 0",
                    []
                )
                .is_err()
        );

        db.delete_capture(project.id).unwrap();
        assert!(db.take_prompts(project.id).unwrap().is_empty());
        assert!(db.take_results(project.id).unwrap().is_empty());
    }

    #[test]
    fn clips_used_before_their_stretch_was_kept_take_it_from_their_capture() {
        use bardo_domain::{
            AssetSource, MediaAsset, MediaAssetId, MediaAssetRepository, MediaKind, PerformedClip,
            PictureSize,
        };
        let (db, project) = setup();
        let narration = narration(&project);
        db.save_narration(&narration).unwrap();
        let saved = capture(&project, &narration, CaptureVoice::Own);
        db.save_capture(&saved).unwrap();
        let used = |position: u32, capture: CaptureId| {
            let asset = MediaAsset {
                id: MediaAssetId::new(),
                project: project.id,
                owner: project.owner,
                kind: MediaKind::Video,
                source: AssetSource::Performed,
                file: format!("media-{position}-{capture}.mp4"),
                name: "Performed clip".into(),
                duration: Duration::from_secs(3),
                picture: Some(PictureSize::new(1920, 1080)),
                imported_at: time(1_800_000_020),
            };
            let clip = PerformedClip {
                asset: asset.id,
                project: project.id,
                capture,
                position,
                model: seedance(),
                realistic: false,
                used_at: time(1_800_000_020 + u64::from(position)),
                stretch: None,
                face: None,
            };
            db.save_performed_clip(&asset, &clip).unwrap();
        };
        // Its final, made from the take's control video as it is now.
        db.save_control_video(&control(&saved, 0, "fp-1")).unwrap();
        db.save_take_result(&result(&saved, 0, TakeStep::Final, "fp-1"))
            .unwrap();
        used(0, saved.id);
        // One of a capture that is gone.
        used(3, CaptureId::new());
        // As the database was before migration 56, then migrated.
        let migrate = |before: &str| {
            let mut conn = db.conn();
            conn.execute_batch(&format!(
                "{before}
                 ALTER TABLE performed_clip DROP COLUMN face_y;
                 ALTER TABLE performed_clip DROP COLUMN face_x;
                 ALTER TABLE performed_clip DROP COLUMN take_end_ms;
                 ALTER TABLE performed_clip DROP COLUMN take_start_ms;
                 ALTER TABLE performed_clip DROP COLUMN narration_id;
                 DROP TABLE performed_rough_cut;
                 PRAGMA user_version = 55;"
            ))
            .unwrap();
            crate::migrations::run(&mut conn).unwrap();
        };
        migrate("");
        let clips = db.performed_clips(project.id).unwrap();
        let take = &saved.takes.as_slice()[0];
        assert_eq!(
            clips[0].stretch,
            Some(bardo_domain::TakeStretch {
                narration: narration.id,
                start: take.start,
                end: take.end,
            })
        );
        assert_eq!(clips[1].stretch, None, "its capture is gone");
        // A project never edited keeps its cut without the clips.
        assert!(!db.performed_in_rough_cut(project.id).unwrap());
        // A take whose control video changed after its final was made
        // may have other bounds now: no stretch.
        migrate("UPDATE control_video SET fingerprint = 'fp-2';");
        assert_eq!(db.performed_clips(project.id).unwrap()[0].stretch, None);
    }
}
