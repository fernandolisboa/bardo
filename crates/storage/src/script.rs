use bardo_domain::{
    GeneratedScript, Generation, GenerationId, JobId, ProfileId, RepositoryError, Script,
    ScriptRecord, ScriptRepository, ScriptText, TemplateUsed, TemplateVersionId, TokenUsage,
    VideoProjectId,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const SELECT_GENERATION: &str = "SELECT g.id, g.profile_id, g.project_id, g.provider, g.model,
        g.template_version_id, t.number, g.instructions, g.prompt, g.output,
        g.input_tokens, g.output_tokens, g.generated_at, g.job_id
    FROM generation g JOIN template_version t ON t.id = g.template_version_id";

#[derive(Debug, thiserror::Error)]
#[error("stored script is invalid: {0}")]
struct InvalidRow(String);

fn invalid(detail: impl Into<String>) -> RepositoryError {
    boxed(InvalidRow(detail.into()))
}

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

fn tokens(value: i64) -> Result<u64, RepositoryError> {
    u64::try_from(value).map_err(boxed)
}

/// A generation row as stored, before domain validation.
struct GenerationRow {
    id: String,
    profile_id: String,
    project_id: String,
    provider: String,
    model: String,
    template_id: String,
    template_number: i64,
    instructions: String,
    prompt: String,
    output: String,
    input_tokens: i64,
    output_tokens: i64,
    generated_at: i64,
    job_id: Option<String>,
}

impl GenerationRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            project_id: row.get(2)?,
            provider: row.get(3)?,
            model: row.get(4)?,
            template_id: row.get(5)?,
            template_number: row.get(6)?,
            instructions: row.get(7)?,
            prompt: row.get(8)?,
            output: row.get(9)?,
            input_tokens: row.get(10)?,
            output_tokens: row.get(11)?,
            generated_at: row.get(12)?,
            job_id: row.get(13)?,
        })
    }

    fn into_generation(self) -> Result<Generation, RepositoryError> {
        Ok(Generation {
            id: GenerationId::from(uuid(&self.id)?),
            owner: ProfileId::from(uuid(&self.profile_id)?),
            project: VideoProjectId::from(uuid(&self.project_id)?),
            provider: self.provider.parse().map_err(boxed)?,
            model: self.model,
            template: TemplateUsed {
                id: TemplateVersionId::from(uuid(&self.template_id)?),
                number: u32::try_from(self.template_number).map_err(boxed)?,
            },
            instructions: self.instructions,
            prompt: self.prompt,
            output: self.output,
            usage: TokenUsage {
                input_tokens: tokens(self.input_tokens)?,
                output_tokens: tokens(self.output_tokens)?,
            },
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

/// The saved generation `id`.
pub(crate) fn generation(conn: &Connection, id: &str) -> Result<Generation, RepositoryError> {
    conn.query_row(
        &format!("{SELECT_GENERATION} WHERE g.id = ?1"),
        [id],
        GenerationRow::read,
    )
    .map_err(boxed)?
    .into_generation()
}

fn generated_script(conn: &Connection, id: &str) -> Result<GeneratedScript, RepositoryError> {
    GeneratedScript::new(generation(conn, id)?).map_err(|error| invalid(format!("{error:?}")))
}

/// Saves the generation unless it already is: generations never change.
pub(crate) fn insert_generation(
    tx: &Transaction<'_>,
    generation: &Generation,
) -> Result<(), RepositoryError> {
    let count = |n: u64| i64::try_from(n).map_err(boxed);
    tx.execute(
        "INSERT INTO generation (id, profile_id, project_id, provider, model,
             template_version_id, instructions, prompt, output,
             input_tokens, output_tokens, generated_at, job_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT (id) DO NOTHING",
        params![
            generation.id.to_string(),
            generation.owner.to_string(),
            generation.project.to_string(),
            generation.provider.code(),
            generation.model,
            generation.template.id.to_string(),
            generation.instructions,
            generation.prompt,
            generation.output,
            count(generation.usage.input_tokens)?,
            count(generation.usage.output_tokens)?,
            to_unix_millis(generation.generated_at),
            generation.job.map(|job| job.to_string()),
        ],
    )
    .map_err(boxed)?;
    Ok(())
}

impl ScriptRepository for Database {
    fn script(&self, project: VideoProjectId) -> Result<Option<Script>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT profile_id, text, source_generation_id, pending_generation_id, updated_at
                 FROM script WHERE project_id = ?1",
                [project.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some((owner, text, source, pending, updated_at)) = row else {
            return Ok(None);
        };
        Ok(Some(Script::restore(ScriptRecord {
            project,
            owner: ProfileId::from(uuid(&owner)?),
            text: ScriptText::new(&text).map_err(|error| invalid(format!("{error:?}")))?,
            source: generated_script(&conn, &source)?,
            pending: pending.map(|id| generated_script(&conn, &id)).transpose()?,
            updated_at: from_unix_millis(updated_at),
        })))
    }

    fn save_script(&self, script: &Script) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        insert_generation(&tx, script.source().generation())?;
        if let Some(pending) = script.pending() {
            insert_generation(&tx, pending.generation())?;
        }
        tx.execute(
            "INSERT INTO script (project_id, profile_id, text, source_generation_id,
                 pending_generation_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (project_id) DO UPDATE SET
                 text = excluded.text,
                 source_generation_id = excluded.source_generation_id,
                 pending_generation_id = excluded.pending_generation_id,
                 updated_at = excluded.updated_at",
            params![
                script.project.to_string(),
                script.owner.to_string(),
                script.text().as_str(),
                script.source().generation().id.to_string(),
                script
                    .pending()
                    .map(|pending| pending.generation().id.to_string()),
                to_unix_millis(script.updated_at),
            ],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Niche, ProfileRepository,
        Provider, TemplateBody, TemplateKind, TemplateRepository, TemplateVersion, Theme,
        ThemeIdea, ThemeRepository, UiLanguage, UserProfile, VideoProject,
    };

    use super::*;

    fn time(secs: u64) -> SystemTime {
        // Whole milliseconds, as stored.
        SystemTime::UNIX_EPOCH + Duration::from_millis(secs * 1000 + 250)
    }

    struct Setup {
        db: Database,
        project: VideoProject,
        template: TemplateVersion,
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
            TemplateBody::new(TemplateKind::Script, "Rules.", "About {{niche}}.").unwrap(),
            time(1_800_000_002),
        );
        db.add_template_version(&template).unwrap();
        Setup {
            db,
            project,
            template,
        }
    }

    fn generated(setup: &Setup, output: &str, at: u64) -> GeneratedScript {
        GeneratedScript::new(Generation {
            id: GenerationId::new(),
            owner: setup.project.owner,
            project: setup.project.id,
            provider: Provider::Claude,
            model: "claude-opus-5-5".into(),
            template: TemplateUsed {
                id: setup.template.id,
                number: setup.template.number,
            },
            instructions: "Rules.".into(),
            prompt: "About space history, açaí 🚀.".into(),
            output: output.into(),
            usage: TokenUsage {
                input_tokens: 812,
                output_tokens: 2_431,
            },
            generated_at: time(at),
            job: Some(JobId::new()),
        })
        .unwrap()
    }

    #[test]
    fn a_script_round_trips_with_its_provenance() {
        let s = setup();
        let mut script = Script::first(
            generated(&s, "Draft one.", 1_800_000_010),
            time(1_800_000_010),
        );
        script.edit(
            ScriptText::new("Draft one, edited.").unwrap(),
            time(1_800_000_020),
        );

        s.db.save_script(&script).unwrap();

        assert_eq!(s.db.script(s.project.id).unwrap(), Some(script));
        assert_eq!(s.db.script(VideoProjectId::new()).unwrap(), None);
    }

    #[test]
    fn a_pending_script_is_kept_until_accepted_and_every_generation_stays() {
        let s = setup();
        let first = generated(&s, "Draft one.", 1_800_000_010);
        let mut script = Script::first(first.clone(), time(1_800_000_010));
        s.db.save_script(&script).unwrap();

        script.offer(
            generated(&s, "Draft two.", 1_800_000_030),
            time(1_800_000_030),
        );
        s.db.save_script(&script).unwrap();
        assert_eq!(s.db.script(s.project.id).unwrap().as_ref(), Some(&script));

        script.accept(time(1_800_000_040)).unwrap();
        s.db.save_script(&script).unwrap();
        let stored = s.db.script(s.project.id).unwrap().unwrap();
        assert_eq!(stored.text().as_str(), "Draft two.");
        assert_eq!(stored.pending(), None);

        let generations: i64 =
            s.db.conn()
                .query_row("SELECT COUNT(*) FROM generation", [], |row| row.get(0))
                .unwrap();
        assert_eq!(generations, 2, "the replaced generation stays for audit");
        assert_eq!(
            generated_script(&s.db.conn(), &first.generation().id.to_string()).unwrap(),
            first
        );
    }
}
