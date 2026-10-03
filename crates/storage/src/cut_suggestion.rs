use std::time::Duration;

use bardo_domain::{
    Confidence, CutReasons, CutSuggestion, CutSuggestionRepository, CutSuggestions, JobId,
    NarrationId, ProfileId, RepositoryError, Score, SuggestionStatus, VideoProjectId,
};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

fn nanos(duration: Duration) -> Result<i64, RepositoryError> {
    i64::try_from(duration.as_nanos()).map_err(boxed)
}

fn duration(nanos: i64) -> Result<Duration, RepositoryError> {
    u64::try_from(nanos)
        .map(Duration::from_nanos)
        .map_err(boxed)
}

impl CutSuggestionRepository for Database {
    fn cut_suggestions(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<CutSuggestions>, RepositoryError> {
        let conn = self.conn();
        let project_id = project.to_string();
        let set = conn
            .query_row(
                "SELECT profile_id, narration_id, job_id, model, made_at
                 FROM cut_suggestion_set WHERE project_id = ?1",
                [&project_id],
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
        let Some((owner, narration, job, model, made_at)) = set else {
            return Ok(None);
        };
        let mut select = conn
            .prepare(
                "SELECT source_ns, sentence_end, pause_ns, scene_change, topic_shift, score,
                        confidence, status
                 FROM cut_suggestion WHERE project_id = ?1 ORDER BY position",
            )
            .map_err(boxed)?;
        let rows = select
            .query_map([&project_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, f64>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .map_err(boxed)?;
        let mut suggestions = Vec::new();
        for row in rows {
            let (source, sentence_end, pause, scene_change, topic_shift, score, confidence, status) =
                row.map_err(boxed)?;
            suggestions.push(CutSuggestion {
                source: duration(source)?,
                reasons: CutReasons {
                    sentence_end,
                    pause: pause.map(duration).transpose()?,
                    scene_change,
                    topic_shift,
                },
                score: Score::new(u8::try_from(score).map_err(boxed)?),
                confidence: Confidence::new(confidence),
                status: SuggestionStatus::from_code_or_default(&status),
            });
        }
        Ok(Some(CutSuggestions {
            project,
            owner: ProfileId::from(uuid(&owner)?),
            narration: NarrationId::from(uuid(&narration)?),
            job: JobId::from(uuid(&job)?),
            model,
            made_at: from_unix_millis(made_at),
            suggestions,
        }))
    }

    fn save_cut_suggestions(&self, set: &CutSuggestions) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let project = set.project.to_string();
        // Its suggestions go with it (ON DELETE CASCADE).
        tx.execute(
            "DELETE FROM cut_suggestion_set WHERE project_id = ?1",
            [&project],
        )
        .map_err(boxed)?;
        tx.execute(
            "INSERT INTO cut_suggestion_set (project_id, profile_id, narration_id, job_id, model,
                                             made_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project,
                set.owner.to_string(),
                set.narration.to_string(),
                set.job.to_string(),
                set.model,
                to_unix_millis(set.made_at),
            ],
        )
        .map_err(boxed)?;
        {
            let mut insert = tx
                .prepare(
                    "INSERT INTO cut_suggestion (project_id, position, source_ns, sentence_end,
                                                 pause_ns, scene_change, topic_shift, score,
                                                 confidence, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )
                .map_err(boxed)?;
            for (position, suggestion) in set.suggestions.iter().enumerate() {
                let reasons = suggestion.reasons;
                insert
                    .execute(params![
                        project,
                        i64::try_from(position).map_err(boxed)?,
                        nanos(suggestion.source)?,
                        reasons.sentence_end,
                        reasons.pause.map(nanos).transpose()?,
                        reasons.scene_change,
                        reasons.topic_shift,
                        suggestion.score.value(),
                        suggestion.confidence.value(),
                        suggestion.status.code(),
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
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Niche, ProfileRepository, Theme,
        ThemeIdea, ThemeRepository, UiLanguage, UserProfile,
    };

    use super::*;

    fn project(db: &Database) -> (ProfileId, VideoProjectId) {
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(db, &profile).unwrap();
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Space Archives".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(profile.id, details);
        ChannelRepository::save(db, &channel).unwrap();
        let mut theme = Theme::suggested(
            profile.id,
            channel.id,
            Niche::new("space history").unwrap(),
            ThemeIdea::new("The lost probe", "").unwrap(),
            SystemTime::UNIX_EPOCH,
            0,
            None,
        );
        db.save_themes(std::slice::from_ref(&theme)).unwrap();
        let project = theme.approve(SystemTime::UNIX_EPOCH).unwrap();
        db.start_project(&theme, &project).unwrap();
        (profile.id, project.id)
    }

    fn set(owner: ProfileId, project: VideoProjectId) -> CutSuggestions {
        CutSuggestions {
            project,
            owner,
            narration: NarrationId::new(),
            job: JobId::new(),
            model: "jev-1.13".into(),
            made_at: SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_000),
            suggestions: vec![
                CutSuggestion {
                    source: Duration::from_nanos(1_500_000_001),
                    reasons: CutReasons {
                        sentence_end: true,
                        pause: Some(Duration::from_millis(420)),
                        scene_change: false,
                        topic_shift: true,
                    },
                    score: Score::new(82),
                    confidence: Confidence::new(0.75),
                    status: SuggestionStatus::Pending,
                },
                CutSuggestion {
                    source: Duration::from_secs(4),
                    reasons: CutReasons {
                        scene_change: true,
                        ..CutReasons::default()
                    },
                    score: Score::new(31),
                    confidence: Confidence::new(0.5),
                    status: SuggestionStatus::Rejected,
                },
            ],
        }
    }

    #[test]
    fn suggestions_round_trip_and_a_new_set_replaces_the_old() {
        let db = Database::open_in_memory().unwrap();
        let (owner, project) = project(&db);
        assert_eq!(db.cut_suggestions(project).unwrap(), None);

        let first = set(owner, project);
        db.save_cut_suggestions(&first).unwrap();
        assert_eq!(db.cut_suggestions(project).unwrap(), Some(first));

        let mut second = set(owner, project);
        second.suggestions.truncate(1);
        db.save_cut_suggestions(&second).unwrap();
        assert_eq!(db.cut_suggestions(project).unwrap(), Some(second));
    }
}
