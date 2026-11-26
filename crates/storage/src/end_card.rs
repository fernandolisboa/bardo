//! An ad's end card design (#281, ADR-0044).

use std::time::SystemTime;

use bardo_domain::{
    CardDesign, CardLogoPlace, CardTextPlace, EndCard, EndCardRepository, RepositoryError,
    ThumbnailLook, VideoProjectId,
};
use rusqlite::{OptionalExtension, params};

use crate::{Database, boxed, to_unix_millis};

fn invalid(detail: impl Into<String>) -> RepositoryError {
    RepositoryError(detail.into().into())
}

/// An end card row as stored, before domain validation.
struct EndCardRow {
    headline: String,
    second_line: String,
    offer: bool,
    look: String,
    text_place: String,
    logo_place: String,
}

impl EndCardRepository for Database {
    fn end_card(&self, project: VideoProjectId) -> Result<Option<EndCard>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT headline, second_line, offer, look, text_place, logo_place
                 FROM end_card WHERE project_id = ?1",
                [project.to_string()],
                |row| {
                    Ok(EndCardRow {
                        headline: row.get(0)?,
                        second_line: row.get(1)?,
                        offer: row.get(2)?,
                        look: row.get(3)?,
                        text_place: row.get(4)?,
                        logo_place: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some(row) = row else {
            return Ok(None);
        };
        // A look or place this version does not know is the first one.
        let design = CardDesign::new(
            &row.headline,
            &row.second_line,
            row.offer,
            row.look.parse::<ThumbnailLook>().unwrap_or_default(),
            CardTextPlace::from_code(&row.text_place).unwrap_or_default(),
            CardLogoPlace::from_code(&row.logo_place).unwrap_or_default(),
        )
        .map_err(|errors| invalid(format!("end card of {project}: {errors:?}")))?;
        Ok(Some(EndCard { project, design }))
    }

    fn save_end_card(&self, card: &EndCard) -> Result<(), RepositoryError> {
        let design = &card.design;
        self.conn()
            .execute(
                "INSERT INTO end_card (project_id, headline, second_line, offer, look,
                                       text_place, logo_place, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (project_id) DO UPDATE SET
                     headline = excluded.headline,
                     second_line = excluded.second_line,
                     offer = excluded.offer,
                     look = excluded.look,
                     text_place = excluded.text_place,
                     logo_place = excluded.logo_place,
                     updated_at = excluded.updated_at",
                params![
                    card.project.to_string(),
                    design.headline(),
                    design.second_line(),
                    design.offer,
                    design.look.code(),
                    design.text_place.code(),
                    design.logo_place.code(),
                    to_unix_millis(SystemTime::now()),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Niche, ProfileRepository, Theme,
        ThemeIdea, ThemeRepository, UiLanguage, UserProfile, VideoProject,
    };

    use super::*;

    fn time(millis: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
    }

    fn project(db: &Database) -> VideoProject {
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(db, &profile).unwrap();
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Looms".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(profile.id, details);
        ChannelRepository::save(db, &channel).unwrap();
        let mut theme = Theme::suggested(
            profile.id,
            channel.id,
            Niche::new("crafts").unwrap(),
            ThemeIdea::new("The pocket loom", "").unwrap(),
            time(1_800_000_000_000),
            0,
            None,
        );
        db.save_themes(std::slice::from_ref(&theme)).unwrap();
        let project = theme.approve(time(1_800_000_001_000)).unwrap();
        db.start_project(&theme, &project).unwrap();
        project
    }

    #[test]
    fn a_card_design_is_saved_whole_and_replaced_and_goes_with_its_project() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        assert_eq!(db.end_card(project.id).unwrap(), None);
        let card = EndCard {
            project: project.id,
            design: CardDesign::new(
                "Get yours",
                "loom.com",
                true,
                ThumbnailLook::Banner,
                CardTextPlace::Below,
                CardLogoPlace::WithText,
            )
            .unwrap(),
        };
        db.save_end_card(&card).unwrap();
        assert_eq!(db.end_card(project.id).unwrap(), Some(card.clone()));

        let changed = EndCard {
            design: CardDesign::new(
                "Order now",
                "",
                false,
                ThumbnailLook::Yellow,
                CardTextPlace::Above,
                CardLogoPlace::Corner,
            )
            .unwrap(),
            ..card
        };
        db.save_end_card(&changed).unwrap();
        assert_eq!(db.end_card(project.id).unwrap(), Some(changed));

        db.conn()
            .execute(
                "DELETE FROM video_project WHERE id = ?1",
                [project.id.to_string()],
            )
            .unwrap();
        assert_eq!(db.end_card(project.id).unwrap(), None);
    }
}
