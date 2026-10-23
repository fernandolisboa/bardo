use bardo_domain::{
    GenerationPresets, JobId, LookAsset, LookAssetKind, LookDrawing, LookImage, LookSource,
    Persona, PersonaDetails,
    PersonaDraft, PersonaId, PersonaLook, PersonaRepository, ProfileId, Provider, RepositoryError,
    TemplateUsed, TemplateVersionId, VoiceFlag, VoiceRef,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const SELECT_PERSONA: &str = "SELECT id, profile_id, name, voice_provider, voice_id, voice_name, \
     tone, script_style, stability, similarity, style, speed, voice_flag, realistic_voice FROM persona";

/// A persona row as stored, before domain validation.
struct PersonaRow {
    id: String,
    profile_id: String,
    name: String,
    voice_provider: String,
    voice_id: String,
    voice_name: String,
    tone: String,
    script_style: String,
    presets: [i64; 4],
    voice_flag: Option<String>,
    realistic_voice: bool,
}

impl PersonaRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            name: row.get(2)?,
            voice_provider: row.get(3)?,
            voice_id: row.get(4)?,
            voice_name: row.get(5)?,
            tone: row.get(6)?,
            script_style: row.get(7)?,
            presets: [row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?],
            voice_flag: row.get(12)?,
            realistic_voice: row.get(13)?,
        })
    }

    /// Rebuilds the persona through domain validation, so a row edited
    /// outside the app cannot smuggle in an invalid persona.
    fn into_persona(self) -> Result<Persona, RepositoryError> {
        let provider: Provider = self.voice_provider.parse().map_err(boxed)?;
        let voice = VoiceRef::new(provider, &self.voice_id, &self.voice_name).map_err(boxed)?;
        let [stability, similarity, style, speed] = self
            .presets
            .map(|value| u8::try_from(value).unwrap_or(u8::MAX));
        let draft = PersonaDraft {
            name: self.name,
            voice: Some(voice),
            tone: self.tone,
            script_style: self.script_style,
            presets: GenerationPresets {
                stability,
                similarity,
                style,
                speed,
            },
            realistic_voice: self.realistic_voice,
        };
        let details = PersonaDetails::validate(draft)
            .map_err(|errors| boxed(InvalidRow(format!("{errors:?}"))))?;
        let voice_flag = match self.voice_flag.as_deref() {
            None => None,
            Some("unchecked") => Some(VoiceFlag::Unchecked),
            Some("unavailable") => Some(VoiceFlag::Unavailable),
            Some(other) => return Err(boxed(InvalidRow(format!("voice flag {other:?}")))),
        };
        Ok(Persona {
            id: PersonaId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            owner: ProfileId::from(Uuid::parse_str(&self.profile_id).map_err(boxed)?),
            details,
            voice_flag,
        })
    }
}

fn flag_code(flag: Option<VoiceFlag>) -> Option<&'static str> {
    flag.map(|flag| match flag {
        VoiceFlag::Unchecked => "unchecked",
        VoiceFlag::Unavailable => "unavailable",
    })
}

#[derive(Debug, thiserror::Error)]
#[error("stored persona is invalid: {0}")]
struct InvalidRow(String);

fn upsert(tx: &Transaction<'_>, persona: &Persona) -> rusqlite::Result<()> {
    let details = &persona.details;
    let voice = details.voice();
    let presets = details.presets();
    tx.execute(
        "INSERT INTO persona (id, profile_id, name, voice_provider, voice_id, voice_name, tone,
                              script_style, stability, similarity, style, speed, voice_flag,
                              realistic_voice)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT (id) DO UPDATE SET
             name = excluded.name,
             voice_provider = excluded.voice_provider,
             voice_id = excluded.voice_id,
             voice_name = excluded.voice_name,
             tone = excluded.tone,
             script_style = excluded.script_style,
             stability = excluded.stability,
             similarity = excluded.similarity,
             style = excluded.style,
             speed = excluded.speed,
             voice_flag = excluded.voice_flag,
             realistic_voice = excluded.realistic_voice,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            persona.id.to_string(),
            persona.owner.to_string(),
            details.name(),
            voice.provider().code(),
            voice.id(),
            voice.name(),
            details.tone(),
            details.script_style(),
            presets.stability,
            presets.similarity,
            presets.style,
            presets.speed,
            flag_code(persona.voice_flag),
            details.realistic_voice(),
        ],
    )?;
    Ok(())
}

/// Where a look sits: the persona's own, or a drawing waiting for review.
#[derive(Clone, Copy)]
enum Slot {
    Current,
    Pending,
}

impl Slot {
    fn code(self) -> &'static str {
        match self {
            Slot::Current => "current",
            Slot::Pending => "pending",
        }
    }
}

const SELECT_LOOK: &str = "SELECT persona_id, image, source, realistic, description, prompt, \
     template_id, template_number, writer_model, image_model, job_id, made_at FROM persona_look";

/// A look row as stored, before domain checks.
struct LookRow {
    persona_id: String,
    image: Vec<u8>,
    source: String,
    realistic: bool,
    description: Option<String>,
    prompt: Option<String>,
    template_id: Option<String>,
    template_number: Option<i64>,
    writer_model: Option<String>,
    image_model: Option<String>,
    job_id: Option<String>,
    made_at: i64,
}

impl LookRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            persona_id: row.get(0)?,
            image: row.get(1)?,
            source: row.get(2)?,
            realistic: row.get(3)?,
            description: row.get(4)?,
            prompt: row.get(5)?,
            template_id: row.get(6)?,
            template_number: row.get(7)?,
            writer_model: row.get(8)?,
            image_model: row.get(9)?,
            job_id: row.get(10)?,
            made_at: row.get(11)?,
        })
    }

    /// Rebuilds the look through the domain's checks, so an image edited
    /// outside the app cannot pass for one a take model reads.
    fn into_look(self) -> Result<PersonaLook, RepositoryError> {
        let image = LookImage::check(self.image).map_err(boxed)?;
        let source: LookSource = self.source.parse().map_err(boxed)?;
        let uuid = |text: &str| Uuid::parse_str(text).map_err(boxed);
        let drawing = match self.prompt {
            None => None,
            Some(prompt) => Some(LookDrawing {
                description: self.description.unwrap_or_default(),
                prompt,
                template: match (self.template_id, self.template_number) {
                    (Some(id), Some(number)) => Some(TemplateUsed {
                        id: TemplateVersionId::from(uuid(&id)?),
                        number: u32::try_from(number).map_err(boxed)?,
                    }),
                    _ => None,
                },
                writer_model: self.writer_model.unwrap_or_default(),
                image_model: self.image_model.unwrap_or_default(),
                job: self
                    .job_id
                    .as_deref()
                    .map(|id| uuid(id).map(JobId::from))
                    .transpose()?,
            }),
        };
        Ok(PersonaLook {
            persona: PersonaId::from(uuid(&self.persona_id)?),
            image,
            source,
            realistic: self.realistic,
            drawing,
            made_at: from_unix_millis(self.made_at),
        })
    }
}

impl Database {
    fn look_in(
        &self,
        persona: PersonaId,
        slot: Slot,
    ) -> Result<Option<PersonaLook>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                &format!("{SELECT_LOOK} WHERE persona_id = ?1 AND slot = ?2"),
                params![persona.to_string(), slot.code()],
                LookRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(LookRow::into_look).transpose()
    }

    fn save_look_in(&self, look: &PersonaLook, slot: Slot) -> Result<(), RepositoryError> {
        let drawing = look.drawing.as_ref();
        self.conn()
            .execute(
                "INSERT INTO persona_look (persona_id, slot, image, format, width, height, source,
                                           realistic, description, prompt, template_id,
                                           template_number, writer_model, image_model, job_id,
                                           made_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT (persona_id, slot) DO UPDATE SET
                     image = excluded.image,
                     format = excluded.format,
                     width = excluded.width,
                     height = excluded.height,
                     source = excluded.source,
                     realistic = excluded.realistic,
                     description = excluded.description,
                     prompt = excluded.prompt,
                     template_id = excluded.template_id,
                     template_number = excluded.template_number,
                     writer_model = excluded.writer_model,
                     image_model = excluded.image_model,
                     job_id = excluded.job_id,
                     made_at = excluded.made_at",
                params![
                    look.persona.to_string(),
                    slot.code(),
                    look.image.bytes(),
                    look.image.format().mime(),
                    look.image.width(),
                    look.image.height(),
                    look.source.code(),
                    look.realistic,
                    drawing.map(|d| d.description.as_str()),
                    drawing.map(|d| d.prompt.as_str()),
                    drawing.and_then(|d| d.template).map(|t| t.id.to_string()),
                    drawing.and_then(|d| d.template).map(|t| t.number),
                    drawing.map(|d| d.writer_model.as_str()),
                    drawing.map(|d| d.image_model.as_str()),
                    drawing.and_then(|d| d.job).map(|job| job.to_string()),
                    to_unix_millis(look.made_at),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn remove_look_in(&self, persona: PersonaId, slot: Slot) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "DELETE FROM persona_look WHERE persona_id = ?1 AND slot = ?2",
                params![persona.to_string(), slot.code()],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

impl PersonaRepository for Database {
    fn list(&self, owner: ProfileId) -> Result<Vec<Persona>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(&format!(
                "{SELECT_PERSONA} WHERE profile_id = ?1 ORDER BY name COLLATE NOCASE, rowid"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], PersonaRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter().map(PersonaRow::into_persona).collect()
    }

    fn get(&self, id: PersonaId) -> Result<Option<Persona>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                &format!("{SELECT_PERSONA} WHERE id = ?1"),
                [id.to_string()],
                PersonaRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(PersonaRow::into_persona).transpose()
    }

    fn save(&self, persona: &Persona) -> Result<(), RepositoryError> {
        self.insert_all(std::slice::from_ref(persona))
    }

    fn insert_all(&self, personas: &[Persona]) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        for persona in personas {
            upsert(&tx, persona).map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }

    fn look(&self, persona: PersonaId) -> Result<Option<PersonaLook>, RepositoryError> {
        self.look_in(persona, Slot::Current)
    }

    fn pending_look(&self, persona: PersonaId) -> Result<Option<PersonaLook>, RepositoryError> {
        self.look_in(persona, Slot::Pending)
    }

    fn save_look(&self, look: &PersonaLook) -> Result<(), RepositoryError> {
        self.save_look_in(look, Slot::Current)
    }

    fn save_pending_look(&self, look: &PersonaLook) -> Result<(), RepositoryError> {
        self.save_look_in(look, Slot::Pending)
    }

    fn keep_pending_look(&self, persona: PersonaId) -> Result<bool, RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id = persona.to_string();
        let waiting: bool = tx
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM persona_look WHERE persona_id = ?1 AND slot = ?2)",
                params![id, Slot::Pending.code()],
                |row| row.get(0),
            )
            .map_err(boxed)?;
        if !waiting {
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM persona_look WHERE persona_id = ?1 AND slot = ?2",
            params![id, Slot::Current.code()],
        )
        .map_err(boxed)?;
        tx.execute(
            "UPDATE persona_look SET slot = ?2 WHERE persona_id = ?1 AND slot = ?3",
            params![id, Slot::Current.code(), Slot::Pending.code()],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)?;
        Ok(true)
    }

    fn remove_look(&self, persona: PersonaId) -> Result<(), RepositoryError> {
        self.remove_look_in(persona, Slot::Current)
    }

    fn remove_pending_look(&self, persona: PersonaId) -> Result<(), RepositoryError> {
        self.remove_look_in(persona, Slot::Pending)
    }

    fn look_asset(
        &self,
        persona: PersonaId,
        provider: Provider,
    ) -> Result<Option<LookAsset>, RepositoryError> {
        let row: Option<(String, String, String)> = self
            .conn()
            .query_row(
                "SELECT kind, asset_id, image FROM persona_look_asset
                 WHERE persona_id = ?1 AND provider = ?2",
                params![persona.to_string(), provider.code()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(boxed)?;
        row.map(|(kind, id, image)| {
            Ok(LookAsset {
                kind: LookAssetKind::from_code(&kind).ok_or_else(|| {
                    boxed(std::io::Error::other(format!("unknown look asset kind {kind}")))
                })?,
                id,
                image,
            })
        })
        .transpose()
    }

    fn save_look_asset(
        &self,
        persona: PersonaId,
        provider: Provider,
        asset: Option<&LookAsset>,
    ) -> Result<(), RepositoryError> {
        let conn = self.conn();
        match asset {
            Some(asset) => conn.execute(
                "INSERT INTO persona_look_asset (persona_id, provider, kind, asset_id, image)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (persona_id, provider) DO UPDATE SET
                     kind = excluded.kind, asset_id = excluded.asset_id, image = excluded.image",
                params![
                    persona.to_string(),
                    provider.code(),
                    asset.kind.code(),
                    asset.id,
                    asset.image
                ],
            ),
            None => conn.execute(
                "DELETE FROM persona_look_asset WHERE persona_id = ?1 AND provider = ?2",
                params![persona.to_string(), provider.code()],
            ),
        }
        .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, ProfileRepository, UiLanguage,
        UserProfile,
    };

    use super::*;

    fn database_with_profile() -> (Database, ProfileId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        (db, profile.id)
    }

    fn persona(owner: ProfileId, name: &str) -> Persona {
        let details = PersonaDetails::validate(PersonaDraft {
            name: name.into(),
            voice: Some(VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap()),
            tone: "Calm.".into(),
            script_style: "Short sentences.".into(),
            presets: GenerationPresets {
                stability: 60,
                similarity: 75,
                style: 10,
                speed: 95,
            },
            realistic_voice: true,
        })
        .unwrap();
        Persona::new(owner, details)
    }

    #[test]
    fn saved_persona_is_read_back_with_every_field() {
        let (db, owner) = database_with_profile();
        let saved = persona(owner, "Narrator");
        PersonaRepository::save(&db, &saved).unwrap();

        assert_eq!(
            PersonaRepository::get(&db, saved.id).unwrap(),
            Some(saved.clone())
        );
        assert_eq!(PersonaRepository::list(&db, owner).unwrap(), [saved]);
    }

    #[test]
    fn a_look_asset_is_kept_per_provider_and_goes_with_its_persona() {
        let (db, owner) = database_with_profile();
        let saved = persona(owner, "Narrator");
        PersonaRepository::save(&db, &saved).unwrap();
        assert_eq!(db.look_asset(saved.id, Provider::BytePlus).unwrap(), None);
        let asset = LookAsset {
            kind: LookAssetKind::VirtualPortrait,
            id: "asset-20261023-7f3a".into(),
            image: "fp-look".into(),
        };
        db.save_look_asset(saved.id, Provider::BytePlus, Some(&asset))
            .unwrap();
        let replaced = LookAsset {
            kind: LookAssetKind::PresetCharacter,
            ..asset
        };
        db.save_look_asset(saved.id, Provider::BytePlus, Some(&replaced))
            .unwrap();
        assert_eq!(
            db.look_asset(saved.id, Provider::BytePlus).unwrap(),
            Some(replaced.clone())
        );
        assert_eq!(db.look_asset(saved.id, Provider::Higgsfield).unwrap(), None);
        db.save_look_asset(saved.id, Provider::BytePlus, None).unwrap();
        assert_eq!(db.look_asset(saved.id, Provider::BytePlus).unwrap(), None);
        db.save_look_asset(saved.id, Provider::BytePlus, Some(&replaced))
            .unwrap();
        let conn = db.conn();
        conn.execute("DELETE FROM persona WHERE id = ?1", [saved.id.to_string()])
            .unwrap();
        let left: i64 = conn
            .query_row("SELECT count(*) FROM persona_look_asset", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn saving_again_updates_in_place() {
        let (db, owner) = database_with_profile();
        let mut saved = persona(owner, "Narrator");
        PersonaRepository::save(&db, &saved).unwrap();
        saved.details = PersonaDetails::validate(PersonaDraft {
            name: "Storyteller".into(),
            voice: Some(VoiceRef::elevenlabs("gOupLcAkjEnguROwi4oS", "Darian").unwrap()),
            ..PersonaDraft::from(&saved.details)
        })
        .unwrap();
        PersonaRepository::save(&db, &saved).unwrap();
        assert_eq!(PersonaRepository::list(&db, owner).unwrap(), [saved]);
    }

    #[test]
    fn list_is_ordered_by_name_and_scoped_to_the_owner() {
        let (db, owner) = database_with_profile();
        let other = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &other).unwrap();
        let mine: Vec<_> = ["beta", "Alpha", "Gamma"]
            .into_iter()
            .map(|name| persona(owner, name))
            .collect();
        db.insert_all(&mine).unwrap();
        PersonaRepository::save(&db, &persona(other.id, "Not mine")).unwrap();

        let names: Vec<_> = PersonaRepository::list(&db, owner)
            .unwrap()
            .into_iter()
            .map(|p| p.details.name().to_owned())
            .collect();
        assert_eq!(names, ["Alpha", "beta", "Gamma"]);
    }

    #[test]
    fn insert_all_saves_all_or_nothing() {
        let (db, owner) = database_with_profile();
        let batch = [persona(owner, "One"), persona(owner, "ONE")];
        assert!(db.insert_all(&batch).is_err());
        assert!(PersonaRepository::list(&db, owner).unwrap().is_empty());
    }

    #[test]
    fn a_persona_stores_only_a_voice_reference() {
        let (db, owner) = database_with_profile();
        PersonaRepository::save(&db, &persona(owner, "Narrator")).unwrap();
        let columns: Vec<(String, String)> = db
            .conn()
            .prepare("SELECT name, type FROM pragma_table_info('persona')")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            columns.iter().all(|(_, kind)| kind != "BLOB"),
            "{columns:?}"
        );
        let names: Vec<_> = columns.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "id",
                "profile_id",
                "name",
                "voice_provider",
                "voice_id",
                "voice_name",
                "tone",
                "script_style",
                "stability",
                "similarity",
                "style",
                "speed",
                "created_at",
                "updated_at",
                "voice_flag",
                "realistic_voice",
            ]
        );
    }

    #[test]
    fn the_voice_flag_is_kept_and_cleared() {
        let (db, owner) = database_with_profile();
        let mut saved = persona(owner, "Imported");
        for flag in [
            Some(VoiceFlag::Unchecked),
            Some(VoiceFlag::Unavailable),
            None,
        ] {
            saved.voice_flag = flag;
            PersonaRepository::save(&db, &saved).unwrap();
            assert_eq!(
                PersonaRepository::get(&db, saved.id)
                    .unwrap()
                    .unwrap()
                    .voice_flag,
                flag
            );
        }
    }

    #[test]
    fn the_schema_refuses_an_unknown_voice_flag() {
        let (db, owner) = database_with_profile();
        let saved = persona(owner, "Narrator");
        PersonaRepository::save(&db, &saved).unwrap();
        assert!(
            db.conn()
                .execute("UPDATE persona SET voice_flag = 'lost'", [])
                .is_err()
        );
    }

    #[test]
    fn a_row_with_an_invalid_voice_id_fails_to_load() {
        let (db, owner) = database_with_profile();
        let saved = persona(owner, "Narrator");
        PersonaRepository::save(&db, &saved).unwrap();
        db.conn()
            .execute("UPDATE persona SET voice_id = 'not an id'", [])
            .unwrap();
        assert!(PersonaRepository::get(&db, saved.id).is_err());
    }

    #[test]
    fn a_channel_keeps_its_default_persona_until_the_persona_is_gone() {
        let (db, owner) = database_with_profile();
        let narrator = persona(owner, "Narrator");
        PersonaRepository::save(&db, &narrator).unwrap();
        let channel = Channel::new(
            owner,
            ChannelDetails::validate(ChannelDraft {
                name: "Space Archives".into(),
                default_persona: Some(narrator.id),
                ..ChannelDraft::default()
            })
            .unwrap(),
        );
        ChannelRepository::save(&db, &channel).unwrap();
        assert_eq!(
            ChannelRepository::get(&db, channel.id).unwrap().unwrap(),
            channel
        );

        db.conn()
            .execute(
                "DELETE FROM persona WHERE id = ?1",
                [narrator.id.to_string()],
            )
            .unwrap();
        let reloaded = ChannelRepository::get(&db, channel.id).unwrap().unwrap();
        assert_eq!(reloaded.details.default_persona(), None);
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0]);
        bytes
    }

    fn drawn(persona: PersonaId, width: u32) -> PersonaLook {
        PersonaLook::drawn(
            persona,
            LookImage::check(png(width, 1248)).unwrap(),
            LookDrawing {
                description: "A lighthouse keeper with a grey beard".into(),
                prompt: "Waist-up portrait of an original character...".into(),
                template: Some(TemplateUsed {
                    id: TemplateVersionId::new(),
                    number: 2,
                }),
                writer_model: "claude-test".into(),
                image_model: "gemini-test".into(),
                job: Some(JobId::new()),
            },
            true,
            from_unix_millis(1_760_000_000_123),
        )
    }

    #[test]
    fn a_look_is_kept_apart_and_read_back_whole() {
        let (db, owner) = database_with_profile();
        let narrator = persona(owner, "Narrator");
        PersonaRepository::save(&db, &narrator).unwrap();
        assert_eq!(db.look(narrator.id).unwrap(), None);

        let look = drawn(narrator.id, 832);
        db.save_look(&look).unwrap();
        assert_eq!(db.look(narrator.id).unwrap(), Some(look.clone()));
        assert_eq!(db.pending_look(narrator.id).unwrap(), None);

        let photo = PersonaLook::imported(
            narrator.id,
            LookImage::check(png(900, 1200)).unwrap(),
            from_unix_millis(1_760_000_000_000),
        );
        db.save_look(&photo).unwrap();
        assert_eq!(db.look(narrator.id).unwrap(), Some(photo));
        db.remove_look(narrator.id).unwrap();
        db.remove_look(narrator.id).unwrap();
        assert_eq!(db.look(narrator.id).unwrap(), None);
    }

    #[test]
    fn a_drawing_waits_beside_the_look_until_kept_or_dropped() {
        let (db, owner) = database_with_profile();
        let narrator = persona(owner, "Narrator");
        PersonaRepository::save(&db, &narrator).unwrap();
        assert!(!db.keep_pending_look(narrator.id).unwrap(), "none waiting");

        let current = drawn(narrator.id, 832);
        let waiting = drawn(narrator.id, 840);
        db.save_look(&current).unwrap();
        db.save_pending_look(&waiting).unwrap();
        assert_eq!(db.look(narrator.id).unwrap(), Some(current.clone()));
        assert_eq!(db.pending_look(narrator.id).unwrap(), Some(waiting.clone()));

        assert!(db.keep_pending_look(narrator.id).unwrap());
        assert_eq!(db.look(narrator.id).unwrap(), Some(waiting));
        assert_eq!(db.pending_look(narrator.id).unwrap(), None);

        db.save_pending_look(&current).unwrap();
        db.remove_pending_look(narrator.id).unwrap();
        assert_eq!(db.pending_look(narrator.id).unwrap(), None);
        assert!(db.look(narrator.id).unwrap().is_some());
    }

    #[test]
    fn a_persona_takes_its_look_with_it() {
        let (db, owner) = database_with_profile();
        let narrator = persona(owner, "Narrator");
        PersonaRepository::save(&db, &narrator).unwrap();
        db.save_look(&drawn(narrator.id, 832)).unwrap();
        db.save_pending_look(&drawn(narrator.id, 840)).unwrap();
        db.conn()
            .execute(
                "DELETE FROM persona WHERE id = ?1",
                [narrator.id.to_string()],
            )
            .unwrap();
        let left: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM persona_look", [], |row| row.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn an_image_edited_outside_bardo_fails_to_load() {
        let (db, owner) = database_with_profile();
        let narrator = persona(owner, "Narrator");
        PersonaRepository::save(&db, &narrator).unwrap();
        db.save_look(&drawn(narrator.id, 832)).unwrap();
        db.conn()
            .execute("UPDATE persona_look SET image = x'474946383961'", [])
            .unwrap();
        assert!(db.look(narrator.id).is_err());
        // A drawing has its prompt; a photo has none.
        assert!(
            db.conn()
                .execute("UPDATE persona_look SET prompt = NULL", [])
                .is_err()
        );
    }

    #[test]
    fn a_channel_cannot_point_at_a_persona_that_does_not_exist() {
        let (db, owner) = database_with_profile();
        let channel = Channel::new(
            owner,
            ChannelDetails::validate(ChannelDraft {
                name: "Space Archives".into(),
                default_persona: Some(PersonaId::new()),
                ..ChannelDraft::default()
            })
            .unwrap(),
        );
        assert!(ChannelRepository::save(&db, &channel).is_err());
    }
}
