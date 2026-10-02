use std::time::Duration;

use bardo_domain::{
    GenerationId, JobFailureKind, NarrationId, ProfileId, RepositoryError, Scene, SceneClip,
    SceneImage, ScenePlan, ScenePlanId, ScenePlanRecord, ScenePlanRepository, ScenePrompt,
    SceneRecord, VideoProjectId,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::channel::clip_model;
use crate::script::{generation, insert_generation};
use crate::{Database, boxed, from_unix_millis, to_unix_millis};

#[derive(Debug, thiserror::Error)]
#[error("stored scene plan is invalid: {0}")]
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

fn prompt(text: &str) -> Result<ScenePrompt, RepositoryError> {
    ScenePrompt::new(text).map_err(|error| invalid(format!("{error:?}")))
}

/// A scene row as stored, before domain validation.
struct SceneRow {
    start_ms: i64,
    end_ms: i64,
    text: String,
    generated_prompt: String,
    prompt: String,
    image: (Option<String>, Option<String>),
    pending: (Option<String>, Option<String>),
    failure: Option<String>,
    motion_prompt: Option<String>,
    clip_model: (Option<String>, Option<String>),
    clip_failure: Option<String>,
}

fn failure_kind(code: Option<String>) -> Result<Option<JobFailureKind>, RepositoryError> {
    code.map(|code| code.parse::<JobFailureKind>())
        .transpose()
        .map_err(boxed)
}

/// Clip slots: the clip the cut uses, and a new one waiting for review.
const CURRENT: &str = "current";
const PENDING: &str = "pending";

/// The scene's clip in `slot`, if it has one.
fn clip(
    conn: &Connection,
    plan: &str,
    position: i64,
    slot: &str,
) -> Result<Option<SceneClip>, RepositoryError> {
    let row = conn
        .query_row(
            "SELECT file, seconds, source_image_id, generation_id FROM scene_clip
             WHERE plan_id = ?1 AND position = ?2 AND slot = ?3",
            params![plan, position, slot],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(boxed)?;
    let Some((file, seconds, source, generation_id)) = row else {
        return Ok(None);
    };
    Ok(Some(SceneClip {
        file,
        seconds: u32::try_from(seconds).map_err(boxed)?,
        source_image: GenerationId::from(uuid(&source)?),
        generation: generation(conn, &generation_id)?,
    }))
}

fn image(
    conn: &Connection,
    (file, generation_id): (Option<String>, Option<String>),
) -> Result<Option<SceneImage>, RepositoryError> {
    match (file, generation_id) {
        (Some(file), Some(id)) => Ok(Some(SceneImage {
            file,
            generation: generation(conn, &id)?,
        })),
        (None, None) => Ok(None),
        _ => Err(invalid("an image without its file or generation")),
    }
}

impl SceneRow {
    fn into_scene(
        self,
        conn: &Connection,
        plan: &str,
        position: i64,
    ) -> Result<Scene, RepositoryError> {
        Ok(Scene::restore(SceneRecord {
            start: duration(self.start_ms)?,
            end: duration(self.end_ms)?,
            text: self.text,
            generated_prompt: prompt(&self.generated_prompt)?,
            prompt: prompt(&self.prompt)?,
            image: image(conn, self.image)?,
            pending: image(conn, self.pending)?,
            failure: failure_kind(self.failure)?,
            motion_prompt: self.motion_prompt.as_deref().map(prompt).transpose()?,
            clip_model: clip_model(self.clip_model)?,
            clip: clip(conn, plan, position, CURRENT)?,
            pending_clip: clip(conn, plan, position, PENDING)?,
            clip_failure: failure_kind(self.clip_failure)?,
        }))
    }
}

fn scenes(conn: &Connection, plan: &str) -> Result<Vec<Scene>, RepositoryError> {
    let mut statement = conn
        .prepare(
            "SELECT start_ms, end_ms, text, generated_prompt, prompt, image_file,
                 image_generation_id, pending_file, pending_generation_id, failure,
                 motion_prompt, clip_provider, clip_model, clip_failure, position
             FROM scene WHERE plan_id = ?1 ORDER BY position",
        )
        .map_err(boxed)?;
    let rows = statement
        .query_map([plan], |row| {
            Ok((
                SceneRow {
                    start_ms: row.get(0)?,
                    end_ms: row.get(1)?,
                    text: row.get(2)?,
                    generated_prompt: row.get(3)?,
                    prompt: row.get(4)?,
                    image: (row.get(5)?, row.get(6)?),
                    pending: (row.get(7)?, row.get(8)?),
                    failure: row.get(9)?,
                    motion_prompt: row.get(10)?,
                    clip_model: (row.get(11)?, row.get(12)?),
                    clip_failure: row.get(13)?,
                },
                row.get::<_, i64>(14)?,
            ))
        })
        .map_err(boxed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(boxed)?;
    rows.into_iter()
        .map(|(row, position)| row.into_scene(conn, plan, position))
        .collect()
}

fn position(index: usize) -> Result<i64, RepositoryError> {
    i64::try_from(index).map_err(boxed)
}

/// Saves the generations of the scene's images and clips; they never
/// change.
fn insert_generations(tx: &Transaction<'_>, scene: &Scene) -> Result<(), RepositoryError> {
    for image in scene.image().into_iter().chain(scene.pending()) {
        insert_generation(tx, &image.generation)?;
    }
    for clip in scene.clip().into_iter().chain(scene.pending_clip()) {
        insert_generation(tx, &clip.generation)?;
    }
    Ok(())
}

/// Writes the scene's clips in place of the ones saved.
fn write_clips(
    tx: &Transaction<'_>,
    plan: &str,
    position: i64,
    scene: &Scene,
) -> Result<(), RepositoryError> {
    tx.execute(
        "DELETE FROM scene_clip WHERE plan_id = ?1 AND position = ?2",
        params![plan, position],
    )
    .map_err(boxed)?;
    for (slot, clip) in [(CURRENT, scene.clip()), (PENDING, scene.pending_clip())] {
        let Some(clip) = clip else {
            continue;
        };
        tx.execute(
            "INSERT INTO scene_clip (plan_id, position, slot, file, seconds, source_image_id,
                 generation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                plan,
                position,
                slot,
                clip.file,
                i64::from(clip.seconds),
                clip.source_image.to_string(),
                clip.generation.id.to_string(),
            ],
        )
        .map_err(boxed)?;
    }
    Ok(())
}

fn insert_scene(
    tx: &Transaction<'_>,
    plan: &str,
    index: usize,
    scene: &Scene,
) -> Result<(), RepositoryError> {
    insert_generations(tx, scene)?;
    tx.execute(
        "INSERT INTO scene (plan_id, position, start_ms, end_ms, text, generated_prompt,
             prompt, image_file, image_generation_id, pending_file, pending_generation_id,
             failure, motion_prompt, clip_provider, clip_model, clip_failure)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            plan,
            position(index)?,
            millis(scene.start),
            millis(scene.end),
            scene.text,
            scene.generated_prompt().as_str(),
            scene.prompt().as_str(),
            scene.image().map(|image| image.file.as_str()),
            scene.image().map(|image| image.generation.id.to_string()),
            scene.pending().map(|image| image.file.as_str()),
            scene.pending().map(|image| image.generation.id.to_string()),
            scene.failure().map(|kind| kind.code()),
            scene.own_motion_prompt().map(ScenePrompt::as_str),
            scene.clip_model().map(|model| model.provider().code()),
            scene.clip_model().map(|model| model.model()),
            scene.clip_failure().map(|kind| kind.code()),
        ],
    )
    .map_err(boxed)?;
    write_clips(tx, plan, position(index)?, scene)
}

impl ScenePlanRepository for Database {
    fn scene_plan(&self, project: VideoProjectId) -> Result<Option<ScenePlan>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT id, profile_id, narration_id, generation_id, updated_at
                 FROM scene_plan WHERE project_id = ?1",
                [project.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some((id, owner, narration, generation_id, updated_at)) = row else {
            return Ok(None);
        };
        Ok(Some(ScenePlan::restore(ScenePlanRecord {
            id: ScenePlanId::from(uuid(&id)?),
            project,
            owner: ProfileId::from(uuid(&owner)?),
            narration: NarrationId::from(uuid(&narration)?),
            generation: generation(&conn, &generation_id)?,
            scenes: scenes(&conn, &id)?,
            updated_at: from_unix_millis(updated_at),
        })))
    }

    fn save_scene_plan(&self, plan: &ScenePlan) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id = plan.id.to_string();
        insert_generation(&tx, &plan.generation)?;
        // Another plan of the project goes, its scenes with it (ON DELETE
        // CASCADE); this plan's scenes are written again below.
        tx.execute(
            "DELETE FROM scene_plan WHERE project_id = ?1 AND id <> ?2",
            params![plan.project.to_string(), id],
        )
        .map_err(boxed)?;
        tx.execute(
            "INSERT INTO scene_plan (project_id, id, profile_id, narration_id, generation_id,
                 updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (project_id) DO UPDATE SET updated_at = excluded.updated_at",
            params![
                plan.project.to_string(),
                id,
                plan.owner.to_string(),
                plan.narration.to_string(),
                plan.generation.id.to_string(),
                to_unix_millis(plan.updated_at),
            ],
        )
        .map_err(boxed)?;
        tx.execute("DELETE FROM scene WHERE plan_id = ?1", [&id])
            .map_err(boxed)?;
        for (index, scene) in plan.scenes().iter().enumerate() {
            insert_scene(&tx, &id, index, scene)?;
        }
        tx.commit().map_err(boxed)
    }

    fn save_scene(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError> {
        let scene = plan.scene(index).map_err(boxed)?;
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id = plan.id.to_string();
        let saved = tx
            .execute(
                "UPDATE scene_plan SET updated_at = ?2 WHERE id = ?1",
                params![id, to_unix_millis(plan.updated_at)],
            )
            .map_err(boxed)?;
        if saved == 0 {
            return Err(invalid(format!("plan {id} is not saved")));
        }
        insert_generations(&tx, scene)?;
        let updated = tx
            .execute(
                "UPDATE scene SET prompt = ?3, image_file = ?4, image_generation_id = ?5,
                     pending_file = ?6, pending_generation_id = ?7, failure = ?8,
                     motion_prompt = ?9, clip_provider = ?10, clip_model = ?11,
                     clip_failure = ?12
                 WHERE plan_id = ?1 AND position = ?2",
                params![
                    id,
                    position(index)?,
                    scene.prompt().as_str(),
                    scene.image().map(|image| image.file.as_str()),
                    scene.image().map(|image| image.generation.id.to_string()),
                    scene.pending().map(|image| image.file.as_str()),
                    scene.pending().map(|image| image.generation.id.to_string()),
                    scene.failure().map(|kind| kind.code()),
                    scene.own_motion_prompt().map(ScenePrompt::as_str),
                    scene.clip_model().map(|model| model.provider().code()),
                    scene.clip_model().map(|model| model.model()),
                    scene.clip_failure().map(|kind| kind.code()),
                ],
            )
            .map_err(boxed)?;
        if updated == 0 {
            return Err(invalid(format!("plan {id} has no scene {index}")));
        }
        write_clips(&tx, &id, position(index)?, scene)?;
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use bardo_domain::{
        Alignment, Channel, ChannelDetails, ChannelDraft, ChannelRepository, CharTiming,
        Generation, GenerationId, GenerationPresets, JobId, Narration, Niche, ProfileRepository,
        Provider, SceneDraft, ScriptText, TemplateBody, TemplateKind, TemplateRepository,
        TemplateUsed, TemplateVersion, Theme, ThemeIdea, ThemeRepository, TokenUsage, UiLanguage,
        UserProfile, VideoProject, VoiceRef, WordTimings,
    };

    use super::*;

    const TEXT: &str = "A probe left. It never came back.";

    fn time(secs: u64) -> SystemTime {
        // Whole milliseconds, as stored.
        SystemTime::UNIX_EPOCH + Duration::from_millis(secs * 1000 + 250)
    }

    struct Setup {
        db: Database,
        project: VideoProject,
        template: TemplateVersion,
        narration: Narration,
    }

    fn setup() -> Setup {
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
        let template = TemplateVersion::first(
            profile.id,
            TemplateBody::new(
                TemplateKind::ImagePrompt,
                "Rules.",
                "{{narration_sentences}}",
            )
            .unwrap(),
            time(1_800_000_002),
        );
        db.add_template_version(&template).unwrap();
        let alignment = Alignment {
            chars: TEXT
                .chars()
                .enumerate()
                .map(|(n, c)| CharTiming {
                    text: c.to_string(),
                    start: Duration::from_millis(n as u64 * 80),
                    end: Duration::from_millis(n as u64 * 80 + 80),
                })
                .collect(),
        };
        let narration = Narration {
            id: NarrationId::new(),
            project: project.id,
            owner: profile.id,
            text: ScriptText::new(TEXT).unwrap(),
            voice: VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap(),
            presets: GenerationPresets::default(),
            model: "eleven_multilingual_v2".into(),
            billed_characters: 33,
            audio_file: "narration.mp3".into(),
            duration: Duration::from_millis(3_000),
            words: WordTimings::from_alignment(TEXT, &alignment),
            generated_at: time(1_800_000_003),
            job: None,
        };
        Setup {
            db,
            project,
            template,
            narration,
        }
    }

    fn generation(s: &Setup, provider: Provider, output: &str) -> Generation {
        Generation {
            id: GenerationId::new(),
            owner: s.project.owner,
            project: s.project.id,
            provider,
            model: "model".into(),
            template: TemplateUsed {
                id: s.template.id,
                number: s.template.number,
            },
            instructions: "Rules.".into(),
            prompt: "Plan the scenes, açaí 🚀.".into(),
            output: output.into(),
            usage: TokenUsage {
                input_tokens: 120,
                output_tokens: 1_290,
            },
            generated_at: time(1_800_000_010),
            job: Some(JobId::new()),
        }
    }

    fn plan(s: &Setup) -> ScenePlan {
        ScenePlan::from_drafts(
            generation(s, Provider::Claude, "{\"scenes\": []}"),
            &s.narration,
            vec![
                SceneDraft {
                    first_sentence: 0,
                    prompt: "A probe leaves Earth.".into(),
                },
                SceneDraft {
                    first_sentence: 1,
                    prompt: "Empty space.".into(),
                },
            ],
            time(1_800_000_011),
        )
        .unwrap()
    }

    fn image(s: &Setup, file: &str) -> SceneImage {
        SceneImage {
            file: file.into(),
            generation: generation(s, Provider::Gemini, file),
        }
    }

    #[test]
    fn a_plan_round_trips_with_its_scenes_and_images() {
        let s = setup();
        let mut plan = plan(&s);
        let scene = plan.scene_mut(0, time(1_800_000_012)).unwrap();
        scene.edit_prompt(ScenePrompt::new("A probe leaves Earth at dawn.").unwrap());
        scene.add_image(image(&s, "one.png"));
        scene.add_image(image(&s, "two.png"));
        plan.scene_mut(1, time(1_800_000_013))
            .unwrap()
            .fail(JobFailureKind::Declined);

        s.db.save_scene_plan(&plan).unwrap();

        assert_eq!(s.db.scene_plan(s.project.id).unwrap(), Some(plan));
        assert_eq!(s.db.scene_plan(VideoProjectId::new()).unwrap(), None);
    }

    fn clip(s: &Setup, file: &str, source: &SceneImage) -> SceneClip {
        SceneClip {
            file: file.into(),
            seconds: 6,
            source_image: source.generation.id,
            generation: generation(s, Provider::Higgsfield, file),
        }
    }

    #[test]
    fn clips_round_trip_with_the_scene_s_motion_prompt_and_model() {
        let s = setup();
        let mut plan = plan(&s);
        s.db.save_scene_plan(&plan).unwrap();
        let still = image(&s, "still.png");
        let scene = plan.scene_mut(0, time(1_800_000_030)).unwrap();
        scene.add_image(still.clone());
        scene.set_motion_prompt(Some(ScenePrompt::new("Slow push in.").unwrap()));
        scene.set_clip_model(Some(
            bardo_domain::ClipModelRef::new(Provider::Higgsfield, "minimax/h3/image-to-video")
                .unwrap(),
        ));
        scene.add_clip(clip(&s, "one.mp4", &still));
        scene.accept_clip().unwrap();
        scene.add_clip(clip(&s, "two.mp4", &still));
        scene.fail_clip(JobFailureKind::Declined);
        s.db.save_scene(&plan, 0).unwrap();
        assert_eq!(s.db.scene_plan(s.project.id).unwrap().as_ref(), Some(&plan));

        // Accepting and going back to the still empty the slots again.
        let scene = plan.scene_mut(0, time(1_800_000_031)).unwrap();
        scene.accept_clip().unwrap();
        scene.remove_clip();
        scene.set_motion_prompt(None);
        scene.set_clip_model(None);
        s.db.save_scene(&plan, 0).unwrap();
        assert_eq!(s.db.scene_plan(s.project.id).unwrap().as_ref(), Some(&plan));

        // A whole plan saved with clips reads back the same.
        let scene = plan.scene_mut(1, time(1_800_000_032)).unwrap();
        let other = image(&s, "other.png");
        scene.add_image(other.clone());
        scene.add_clip(clip(&s, "three.mp4", &other));
        s.db.save_scene_plan(&plan).unwrap();
        assert_eq!(s.db.scene_plan(s.project.id).unwrap(), Some(plan));
    }

    #[test]
    fn saving_one_scene_leaves_the_others() {
        let s = setup();
        let mut plan = plan(&s);
        s.db.save_scene_plan(&plan).unwrap();

        // Meanwhile the user edits scene 1.
        let mut edited = plan.clone();
        edited
            .scene_mut(1, time(1_800_000_020))
            .unwrap()
            .edit_prompt(ScenePrompt::new("Stars.").unwrap());
        s.db.save_scene(&edited, 1).unwrap();

        plan.scene_mut(0, time(1_800_000_021))
            .unwrap()
            .add_image(image(&s, "one.png"));
        s.db.save_scene(&plan, 0).unwrap();

        let stored = s.db.scene_plan(s.project.id).unwrap().unwrap();
        assert_eq!(stored.scenes()[0].image().unwrap().file, "one.png");
        assert_eq!(stored.scenes()[1].prompt().as_str(), "Stars.");
        assert_eq!(stored.updated_at, time(1_800_000_021));
        assert!(s.db.save_scene(&plan, 7).is_err());
    }

    #[test]
    fn a_new_plan_replaces_the_old_one_and_generations_stay() {
        let s = setup();
        let mut first = plan(&s);
        first
            .scene_mut(0, time(1_800_000_012))
            .unwrap()
            .add_image(image(&s, "one.png"));
        s.db.save_scene_plan(&first).unwrap();

        let second = plan(&s);
        s.db.save_scene_plan(&second).unwrap();
        assert_eq!(s.db.scene_plan(s.project.id).unwrap(), Some(second));
        assert!(
            s.db.save_scene(&first, 0).is_err(),
            "the replaced plan is gone"
        );

        let conn = s.db.conn();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |row| row.get(0)).unwrap() };
        assert_eq!(count("SELECT COUNT(*) FROM scene"), 2);
        assert_eq!(
            count("SELECT COUNT(*) FROM generation"),
            3,
            "both plans' and the image's generations stay for audit"
        );
    }
}
