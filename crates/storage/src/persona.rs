use bardo_domain::{
    GenerationPresets, Persona, PersonaDetails, PersonaDraft, PersonaId, PersonaRepository,
    ProfileId, Provider, RepositoryError, VoiceFlag, VoiceRef,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use uuid::Uuid;

use crate::{Database, boxed};

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
