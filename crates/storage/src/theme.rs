use bardo_domain::{
    ChannelId, Confidence, JobId, Niche, ProfileId, Reason, RepositoryError, Score, Theme, ThemeId,
    ThemeIdea, ThemeRanking, ThemeRecord, ThemeRepository, VideoProject, VideoProjectId,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const SELECT_THEME: &str = "SELECT id, profile_id, channel_id, niche, title, angle, status,
        suggested_at, position, suggestion_job,
        fit_score, fit_confidence, trend_score, trend_confidence,
        competition_score, competition_confidence, ranked_by, ranked_at
    FROM theme";

const SELECT_PROJECT: &str =
    "SELECT id, profile_id, channel_id, niche, theme_id, title, created_at FROM video_project";

#[derive(Debug, thiserror::Error)]
#[error("stored theme is invalid: {0}")]
struct InvalidRow(String);

fn invalid(detail: impl Into<String>) -> RepositoryError {
    boxed(InvalidRow(detail.into()))
}

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

fn niche(label: &str) -> Result<Niche, RepositoryError> {
    Niche::new(label).map_err(|error| invalid(format!("{error:?}")))
}

/// A theme row as stored, before domain validation.
struct ThemeRow {
    id: String,
    profile_id: String,
    channel_id: String,
    niche: String,
    title: String,
    angle: String,
    status: String,
    suggested_at: i64,
    position: i64,
    suggestion_job: Option<String>,
    fit: (Option<i64>, Option<f64>),
    trend: (Option<i64>, Option<f64>),
    competition: (Option<i64>, Option<f64>),
    ranked_by: Option<String>,
    ranked_at: Option<i64>,
}

impl ThemeRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            channel_id: row.get(2)?,
            niche: row.get(3)?,
            title: row.get(4)?,
            angle: row.get(5)?,
            status: row.get(6)?,
            suggested_at: row.get(7)?,
            position: row.get(8)?,
            suggestion_job: row.get(9)?,
            fit: (row.get(10)?, row.get(11)?),
            trend: (row.get(12)?, row.get(13)?),
            competition: (row.get(14)?, row.get(15)?),
            ranked_by: row.get(16)?,
            ranked_at: row.get(17)?,
        })
    }

    /// Rebuilds the theme through domain validation, so a row edited
    /// outside the app cannot load an invalid one.
    fn into_theme(self) -> Result<Theme, RepositoryError> {
        let reason = |(score, confidence): (Option<i64>, Option<f64>)| -> Option<Reason> {
            Some(Reason {
                score: Score::new(u8::try_from(score?).ok()?),
                confidence: Confidence::new(confidence?),
            })
        };
        let ranking = match (self.ranked_by, self.ranked_at) {
            (Some(model), Some(ranked_at)) => Some(ThemeRanking {
                fit: reason(self.fit).ok_or_else(|| invalid("incomplete fit"))?,
                trend: reason(self.trend).ok_or_else(|| invalid("incomplete trend"))?,
                competition: reason(self.competition)
                    .ok_or_else(|| invalid("incomplete competition"))?,
                model,
                ranked_at: from_unix_millis(ranked_at),
            }),
            _ => None,
        };
        let idea = ThemeIdea::new(&self.title, &self.angle)
            .map_err(|errors| invalid(format!("{errors:?}")))?;
        Ok(Theme::restore(ThemeRecord {
            id: ThemeId::from(uuid(&self.id)?),
            owner: ProfileId::from(uuid(&self.profile_id)?),
            channel: ChannelId::from(uuid(&self.channel_id)?),
            niche: niche(&self.niche)?,
            idea,
            status: self.status.parse().map_err(boxed)?,
            ranking,
            suggested_at: from_unix_millis(self.suggested_at),
            position: u32::try_from(self.position).map_err(boxed)?,
            suggestion_job: self
                .suggestion_job
                .as_deref()
                .map(uuid)
                .transpose()?
                .map(JobId::from),
        }))
    }
}

fn upsert_theme(tx: &Transaction<'_>, theme: &Theme) -> Result<(), RepositoryError> {
    let ranking = theme.ranking();
    let reason = |pick: fn(&ThemeRanking) -> Reason| {
        ranking.map(|ranking| {
            let reason = pick(ranking);
            (i64::from(reason.score.value()), reason.confidence.value())
        })
    };
    let fit = reason(|r| r.fit);
    let trend = reason(|r| r.trend);
    let competition = reason(|r| r.competition);
    tx.execute(
        "INSERT INTO theme (id, profile_id, channel_id, niche, title, angle, status,
             suggested_at, position, suggestion_job,
             fit_score, fit_confidence, trend_score, trend_confidence,
             competition_score, competition_confidence, ranked_by, ranked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
         ON CONFLICT (id) DO UPDATE SET
             niche = excluded.niche,
             title = excluded.title,
             angle = excluded.angle,
             status = excluded.status,
             fit_score = excluded.fit_score,
             fit_confidence = excluded.fit_confidence,
             trend_score = excluded.trend_score,
             trend_confidence = excluded.trend_confidence,
             competition_score = excluded.competition_score,
             competition_confidence = excluded.competition_confidence,
             ranked_by = excluded.ranked_by,
             ranked_at = excluded.ranked_at",
        params![
            theme.id.to_string(),
            theme.owner.to_string(),
            theme.channel.to_string(),
            theme.niche.label(),
            theme.idea().title(),
            theme.idea().angle(),
            theme.status().code(),
            to_unix_millis(theme.suggested_at),
            i64::from(theme.position),
            theme.suggestion_job.map(|job| job.to_string()),
            fit.map(|(score, _)| score),
            fit.map(|(_, confidence)| confidence),
            trend.map(|(score, _)| score),
            trend.map(|(_, confidence)| confidence),
            competition.map(|(score, _)| score),
            competition.map(|(_, confidence)| confidence),
            ranking.map(|ranking| ranking.model.as_str()),
            ranking.map(|ranking| to_unix_millis(ranking.ranked_at)),
        ],
    )
    .map_err(boxed)?;
    Ok(())
}

fn read_project(row: &Row<'_>) -> rusqlite::Result<[String; 6]> {
    Ok([
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ])
}

impl ThemeRepository for Database {
    fn themes(&self, channel: ChannelId) -> Result<Vec<Theme>, RepositoryError> {
        let rows = self
            .conn()
            .prepare_cached(&format!(
                "{SELECT_THEME} WHERE channel_id = ?1 ORDER BY suggested_at, position"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([channel.to_string()], ThemeRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter().map(ThemeRow::into_theme).collect()
    }

    fn theme(&self, id: ThemeId) -> Result<Option<Theme>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                &format!("{SELECT_THEME} WHERE id = ?1"),
                [id.to_string()],
                ThemeRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(ThemeRow::into_theme).transpose()
    }

    fn save_themes(&self, themes: &[Theme]) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        for theme in themes {
            upsert_theme(&tx, theme)?;
        }
        tx.commit().map_err(boxed)
    }

    fn start_project(&self, theme: &Theme, project: &VideoProject) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        upsert_theme(&tx, theme)?;
        tx.execute(
            "INSERT INTO video_project (id, profile_id, channel_id, niche, theme_id, title, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                project.id.to_string(),
                project.owner.to_string(),
                project.channel.to_string(),
                project.niche.label(),
                project.theme.to_string(),
                project.title,
                to_unix_millis(project.created_at),
            ],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)
    }

    fn projects(&self, channel: ChannelId) -> Result<Vec<VideoProject>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare_cached(&format!(
                "{SELECT_PROJECT} WHERE channel_id = ?1 ORDER BY created_at DESC"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([channel.to_string()], |row| {
                        Ok((read_project(row)?, row.get::<_, i64>(6)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter()
            .map(
                |([id, owner, channel, niche_label, theme, title], created_at)| {
                    Ok(VideoProject {
                        id: VideoProjectId::from(uuid(&id)?),
                        owner: ProfileId::from(uuid(&owner)?),
                        channel: ChannelId::from(uuid(&channel)?),
                        niche: niche(&niche_label)?,
                        theme: ThemeId::from(uuid(&theme)?),
                        title,
                        created_at: from_unix_millis(created_at),
                    })
                },
            )
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, ProfileRepository, ThemeStatus,
        UiLanguage, UserProfile,
    };

    use super::*;

    fn time(secs: u64) -> SystemTime {
        // Whole milliseconds, as stored.
        SystemTime::UNIX_EPOCH + Duration::from_millis(secs * 1000 + 250)
    }

    fn setup() -> (Database, Channel) {
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
        (db, channel)
    }

    fn theme(channel: &Channel, title: &str, position: u32) -> Theme {
        Theme::suggested(
            channel.owner,
            channel.id,
            Niche::new("Space History").unwrap(),
            ThemeIdea::new(title, "An angle with açaí and 🚀.").unwrap(),
            time(1_800_000_000),
            position,
            Some(JobId::new()),
        )
    }

    fn ranking() -> ThemeRanking {
        let reason = |score, confidence| Reason {
            score: Score::new(score),
            confidence: Confidence::new(confidence),
        };
        ThemeRanking {
            fit: reason(80, 0.81),
            trend: reason(55, 0.6),
            competition: reason(30, 0.725),
            model: "jev-1.13.0".into(),
            ranked_at: time(1_800_000_100),
        }
    }

    #[test]
    fn themes_round_trip_with_and_without_a_ranking() {
        let (db, channel) = setup();
        let mut ranked = theme(&channel, "Ranked", 0);
        ranked.rank(ranking()).unwrap();
        let unranked = theme(&channel, "Unranked", 1);

        db.save_themes(&[ranked.clone(), unranked.clone()]).unwrap();

        assert_eq!(db.themes(channel.id).unwrap(), [ranked.clone(), unranked]);
        assert_eq!(db.theme(ranked.id).unwrap(), Some(ranked));
        assert_eq!(db.theme(ThemeId::new()).unwrap(), None);
    }

    #[test]
    fn saving_again_updates_the_theme() {
        let (db, channel) = setup();
        let mut saved = theme(&channel, "Old", 0);
        saved.rank(ranking()).unwrap();
        db.save_themes(std::slice::from_ref(&saved)).unwrap();

        saved.edit(ThemeIdea::new("New", "").unwrap()).unwrap();
        saved.discard().unwrap();
        db.save_themes(std::slice::from_ref(&saved)).unwrap();

        let stored = db.theme(saved.id).unwrap().unwrap();
        assert_eq!(stored.idea().title(), "New");
        assert_eq!(stored.status(), ThemeStatus::Discarded);
        assert_eq!(stored.ranking(), None);
        assert_eq!(db.themes(channel.id).unwrap().len(), 1);
    }

    #[test]
    fn starting_a_project_saves_the_approved_theme_and_the_project() {
        let (db, channel) = setup();
        let mut approved = theme(&channel, "The lost cosmonauts", 0);
        db.save_themes(std::slice::from_ref(&approved)).unwrap();
        let project = approved.approve(time(1_800_000_200)).unwrap();

        db.start_project(&approved, &project).unwrap();

        assert_eq!(
            db.theme(approved.id).unwrap().unwrap().status(),
            ThemeStatus::Approved
        );
        assert_eq!(
            db.projects(channel.id).unwrap(),
            std::slice::from_ref(&project)
        );

        // One project per theme: a second one fails and changes nothing.
        let mut again = project.clone();
        again.id = VideoProjectId::new();
        assert!(db.start_project(&approved, &again).is_err());
        assert_eq!(db.projects(channel.id).unwrap(), [project]);
    }

    #[test]
    fn projects_list_newest_first() {
        let (db, channel) = setup();
        let mut first = theme(&channel, "First", 0);
        let mut second = theme(&channel, "Second", 1);
        db.save_themes(&[first.clone(), second.clone()]).unwrap();
        let older = first.approve(time(1_800_000_000)).unwrap();
        let newer = second.approve(time(1_800_000_500)).unwrap();
        db.start_project(&first, &older).unwrap();
        db.start_project(&second, &newer).unwrap();

        let titles: Vec<_> = db
            .projects(channel.id)
            .unwrap()
            .into_iter()
            .map(|project| project.title)
            .collect();
        assert_eq!(titles, ["Second", "First"]);
    }

    #[test]
    fn a_half_ranked_row_is_rejected() {
        let (db, channel) = setup();
        let saved = theme(&channel, "A", 0);
        db.save_themes(std::slice::from_ref(&saved)).unwrap();
        let result = db.conn().execute(
            "UPDATE theme SET fit_score = 50 WHERE id = ?1",
            [saved.id.to_string()],
        );
        assert!(result.is_err(), "the schema refuses a partial ranking");
    }
}
