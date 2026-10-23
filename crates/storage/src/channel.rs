use bardo_domain::{
    Channel, ChannelDetails, ChannelDraft, ChannelId, ChannelRepository, ClipModelRef,
    MotionDefault, PersonaId, ProfileId, Provider, RepositoryError, Transition, TransitionKind,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, boxed};

const SELECT_CHANNEL: &str = "SELECT id, profile_id, name, niche, aesthetic_notes, language, \
     country, default_persona_id, clip_provider, clip_model, caption_style, transition, \
     transition_frames, video_format, motion, motion_strength, caption_highlight, thumbnail_look, \
     take_provider, take_model \
     FROM channel";

/// A channel row before its themes are attached.
struct ChannelRow {
    id: String,
    profile_id: String,
    default_persona: Option<String>,
    clip_model: (Option<String>, Option<String>),
    take_model: (Option<String>, Option<String>),
    caption_style: String,
    caption_highlight: String,
    thumbnail_look: String,
    transition: (String, i64),
    format: String,
    motion: (String, String),
    draft: ChannelDraft,
}

impl ChannelRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<(Self, String, String)> {
        Ok((
            Self {
                id: row.get(0)?,
                profile_id: row.get(1)?,
                default_persona: row.get(7)?,
                clip_model: (row.get(8)?, row.get(9)?),
                take_model: (row.get(18)?, row.get(19)?),
                caption_style: row.get(10)?,
                caption_highlight: row.get(16)?,
                thumbnail_look: row.get(17)?,
                transition: (row.get(11)?, row.get(12)?),
                format: row.get(13)?,
                motion: (row.get(14)?, row.get(15)?),
                draft: ChannelDraft {
                    name: row.get(2)?,
                    niche: row.get(3)?,
                    aesthetic_notes: row.get(4)?,
                    ..ChannelDraft::default()
                },
            },
            row.get(5)?,
            row.get(6)?,
        ))
    }

    /// Rebuilds the channel through domain validation, so a row edited
    /// outside the app cannot smuggle in an invalid channel.
    fn into_channel(
        self,
        conn: &Connection,
        language: &str,
        country: &str,
    ) -> Result<Channel, RepositoryError> {
        let mut draft = self.draft;
        draft.language = language.parse().map_err(boxed)?;
        draft.country = country.parse().map_err(boxed)?;
        draft.themes = themes(conn, &self.id).map_err(boxed)?;
        draft.default_persona = self
            .default_persona
            .map(|id| Uuid::parse_str(&id).map(PersonaId::from))
            .transpose()
            .map_err(boxed)?;
        draft.clip_model = clip_model(self.clip_model)?;
        draft.take_model = clip_model(self.take_model)?;
        draft.caption_style = self.caption_style.parse().map_err(boxed)?;
        draft.caption_highlight = self.caption_highlight.parse().map_err(boxed)?;
        draft.thumbnail_look = self.thumbnail_look.parse().map_err(boxed)?;
        draft.format = self.format.parse().map_err(boxed)?;
        let (pattern, strength) = &self.motion;
        draft.motion = MotionDefault::new(
            pattern.parse().map_err(boxed)?,
            strength.parse().map_err(boxed)?,
        );
        let (kind, frames) = self.transition;
        draft.transition = Transition::new(
            kind.parse::<TransitionKind>().map_err(boxed)?,
            u32::try_from(frames).map_err(boxed)?,
        );
        let details = ChannelDetails::validate(draft)
            .map_err(|errors| boxed(InvalidRow(format!("{errors:?}"))))?;
        Ok(Channel {
            id: ChannelId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            owner: ProfileId::from(Uuid::parse_str(&self.profile_id).map_err(boxed)?),
            details,
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("stored channel is invalid: {0}")]
struct InvalidRow(String);

/// A video model as stored: provider and model, both or neither.
pub(crate) fn clip_model(
    (provider, model): (Option<String>, Option<String>),
) -> Result<Option<ClipModelRef>, RepositoryError> {
    match (provider, model) {
        (Some(provider), Some(model)) => {
            let provider: Provider = provider.parse().map_err(boxed)?;
            ClipModelRef::new(provider, &model).map(Some).map_err(boxed)
        }
        (None, None) => Ok(None),
        _ => Err(boxed(InvalidRow(
            "a video model without its provider".into(),
        ))),
    }
}

fn themes(conn: &Connection, channel_id: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare_cached("SELECT theme FROM channel_theme WHERE channel_id = ?1 ORDER BY position")?
        .query_map([channel_id], |row| row.get(0))?
        .collect()
}

impl ChannelRepository for Database {
    fn list(&self, owner: ProfileId) -> Result<Vec<Channel>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(&format!(
                "{SELECT_CHANNEL} WHERE profile_id = ?1 ORDER BY name COLLATE NOCASE, rowid"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], ChannelRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter()
            .map(|(row, language, country)| row.into_channel(&conn, &language, &country))
            .collect()
    }

    fn get(&self, id: ChannelId) -> Result<Option<Channel>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                &format!("{SELECT_CHANNEL} WHERE id = ?1"),
                [id.to_string()],
                ChannelRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(|(row, language, country)| row.into_channel(&conn, &language, &country))
            .transpose()
    }

    fn save(&self, channel: &Channel) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id = channel.id.to_string();
        let details = &channel.details;
        tx.execute(
            "INSERT INTO channel (id, profile_id, name, niche, aesthetic_notes, language, country,
                                  default_persona_id, clip_provider, clip_model, caption_style,
                                  transition, transition_frames, video_format, motion,
                                  motion_strength, caption_highlight, thumbnail_look,
                                  take_provider, take_model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19, ?20)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 niche = excluded.niche,
                 aesthetic_notes = excluded.aesthetic_notes,
                 language = excluded.language,
                 country = excluded.country,
                 default_persona_id = excluded.default_persona_id,
                 clip_provider = excluded.clip_provider,
                 clip_model = excluded.clip_model,
                 caption_style = excluded.caption_style,
                 transition = excluded.transition,
                 transition_frames = excluded.transition_frames,
                 video_format = excluded.video_format,
                 motion = excluded.motion,
                 motion_strength = excluded.motion_strength,
                 caption_highlight = excluded.caption_highlight,
                 thumbnail_look = excluded.thumbnail_look,
                 take_provider = excluded.take_provider,
                 take_model = excluded.take_model,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                id,
                channel.owner.to_string(),
                details.name(),
                details.niche(),
                details.aesthetic_notes(),
                details.language().code(),
                details.country().code(),
                details.default_persona().map(|id| id.to_string()),
                details.clip_model().map(|model| model.provider().code()),
                details.clip_model().map(ClipModelRef::model),
                details.caption_style().code(),
                details.transition().kind.code(),
                details.transition().frames(),
                details.format().code(),
                details.motion().pattern.code(),
                details.motion().strength.code(),
                details.caption_highlight().code(),
                details.thumbnail_look().code(),
                details.take_model().map(|model| model.provider().code()),
                details.take_model().map(ClipModelRef::model),
            ],
        )
        .map_err(boxed)?;
        tx.execute("DELETE FROM channel_theme WHERE channel_id = ?1", [&id])
            .map_err(boxed)?;
        for (position, theme) in details.themes().iter().enumerate() {
            tx.execute(
                "INSERT INTO channel_theme (channel_id, position, theme) VALUES (?1, ?2, ?3)",
                params![id, position as i64, theme],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        CaptionStyle, ContentLanguage, Country, ProfileRepository, UiLanguage, UserProfile,
    };

    use super::*;

    fn database_with_profile() -> (Database, ProfileId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        (db, profile.id)
    }

    fn channel(owner: ProfileId, name: &str) -> Channel {
        let details = ChannelDetails::validate(ChannelDraft {
            name: name.into(),
            niche: "space history".into(),
            themes: vec!["Apollo".into(), "Cold War".into()],
            aesthetic_notes: "dark, archival".into(),
            language: ContentLanguage::Portuguese,
            country: Country::Brazil,
            default_persona: None,
            clip_model: Some(
                ClipModelRef::new(Provider::Higgsfield, "kling-video/v2.6/pro/image-to-video")
                    .unwrap(),
            ),
            take_model: Some(
                ClipModelRef::new(Provider::Higgsfield, "bytedance/seedance-2.5/video-edit")
                    .unwrap(),
            ),
            caption_style: CaptionStyle::Boxed,
            caption_highlight: bardo_domain::CaptionHighlight::Green,
            thumbnail_look: bardo_domain::ThumbnailLook::Yellow,
            transition: Transition::new(TransitionKind::SlideLeft, 24),
            format: bardo_domain::VideoFormat::Long,
            motion: MotionDefault::new(
                bardo_domain::MotionPattern::Same(bardo_domain::MotionKind::ZoomOut),
                bardo_domain::MotionStrength::Subtle,
            ),
        })
        .unwrap();
        Channel::new(owner, details)
    }

    #[test]
    fn saved_channel_is_read_back_with_every_field() {
        let (db, owner) = database_with_profile();
        let saved = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &saved).unwrap();

        assert_eq!(db.get(saved.id).unwrap(), Some(saved.clone()));
        assert_eq!(db.list(owner).unwrap(), [saved]);
    }

    #[test]
    fn a_channel_from_before_formats_makes_long_videos() {
        let (db, owner) = database_with_profile();
        let short = Channel::new(
            owner,
            ChannelDetails::validate(ChannelDraft {
                name: "Shorts".into(),
                ..ChannelDraft::default()
            })
            .unwrap(),
        );
        ChannelRepository::save(&db, &short).unwrap();
        assert_eq!(
            db.get(short.id).unwrap().unwrap().details.format(),
            bardo_domain::VideoFormat::Short
        );

        // A row as an older version wrote it, with no format of its own.
        let old = ChannelId::new();
        db.conn()
            .execute(
                "INSERT INTO channel (id, profile_id, name, niche, aesthetic_notes, language, country)
                 VALUES (?1, ?2, 'Old', '', '', ?3, ?4)",
                params![
                    old.to_string(),
                    owner.to_string(),
                    ContentLanguage::English.code(),
                    Country::UnitedStates.code(),
                ],
            )
            .unwrap();
        assert_eq!(
            db.get(old).unwrap().unwrap().details.format(),
            bardo_domain::VideoFormat::Long
        );
        // And with still stills, as its cuts had.
        assert_eq!(
            db.get(old).unwrap().unwrap().details.motion(),
            MotionDefault::still()
        );
        // And captions that light no word, as its cuts had.
        assert_eq!(
            db.get(old).unwrap().unwrap().details.caption_highlight(),
            bardo_domain::CaptionHighlight::Off
        );
        assert_eq!(
            db.get(short.id)
                .unwrap()
                .unwrap()
                .details
                .caption_highlight(),
            bardo_domain::CaptionHighlight::Yellow
        );
    }

    #[test]
    fn unknown_channel_is_none() {
        let (db, _) = database_with_profile();
        assert_eq!(db.get(ChannelId::new()).unwrap(), None);
    }

    #[test]
    fn saving_again_updates_fields_and_replaces_themes() {
        let (db, owner) = database_with_profile();
        let mut saved = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &saved).unwrap();

        saved.details = ChannelDetails::validate(ChannelDraft {
            name: "Space Files".into(),
            themes: vec!["Mars".into()],
            ..ChannelDraft::from(&saved.details)
        })
        .unwrap();
        ChannelRepository::save(&db, &saved).unwrap();

        assert_eq!(db.list(owner).unwrap(), [saved]);
    }

    #[test]
    fn list_is_ordered_by_name_ignoring_case_and_scoped_to_the_owner() {
        let (db, owner) = database_with_profile();
        let other = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &other).unwrap();

        for name in ["beta", "Alpha", "Gamma"] {
            ChannelRepository::save(&db, &channel(owner, name)).unwrap();
        }
        ChannelRepository::save(&db, &channel(other.id, "Not mine")).unwrap();

        let names: Vec<_> = db
            .list(owner)
            .unwrap()
            .into_iter()
            .map(|c| c.details.name().to_owned())
            .collect();
        assert_eq!(names, ["Alpha", "beta", "Gamma"]);
    }

    #[test]
    fn every_row_carries_its_owner() {
        let (db, owner) = database_with_profile();
        let saved = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &saved).unwrap();

        let stored: String = db
            .conn()
            .query_row(
                "SELECT profile_id FROM channel WHERE id = ?1",
                [saved.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, owner.to_string());
    }

    #[test]
    fn a_channel_needs_an_existing_profile() {
        let db = Database::open_in_memory().unwrap();
        assert!(ChannelRepository::save(&db, &channel(ProfileId::new(), "Orphan")).is_err());
    }

    #[test]
    fn duplicate_names_for_one_owner_are_rejected_by_the_schema() {
        let (db, owner) = database_with_profile();
        ChannelRepository::save(&db, &channel(owner, "Space Archives")).unwrap();
        assert!(ChannelRepository::save(&db, &channel(owner, "SPACE ARCHIVES")).is_err());
    }

    #[test]
    fn a_failed_save_leaves_the_previous_version() {
        let (db, owner) = database_with_profile();
        let original = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &original).unwrap();
        ChannelRepository::save(&db, &channel(owner, "Taken")).unwrap();

        let mut renamed = original.clone();
        renamed.details = ChannelDetails::validate(ChannelDraft {
            name: "Taken".into(),
            themes: vec![],
            ..ChannelDraft::from(&original.details)
        })
        .unwrap();
        assert!(ChannelRepository::save(&db, &renamed).is_err());

        assert_eq!(db.get(original.id).unwrap(), Some(original));
    }

    #[test]
    fn channels_survive_reopening_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let profile = UserProfile::new(UiLanguage::PtBr);
        let saved = channel(profile.id, "Arquivos do Espaço");
        {
            let db = Database::open(&path).unwrap();
            ProfileRepository::save(&db, &profile).unwrap();
            ChannelRepository::save(&db, &saved).unwrap();
        }

        let reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.list(profile.id).unwrap(), [saved]);
    }

    #[test]
    fn a_row_with_an_unknown_country_fails_to_load_instead_of_guessing() {
        let (db, owner) = database_with_profile();
        let saved = channel(owner, "Space Archives");
        ChannelRepository::save(&db, &saved).unwrap();
        db.conn()
            .execute("UPDATE channel SET country = 'ZZ'", [])
            .unwrap();

        assert!(db.get(saved.id).is_err());
    }
}
