//! Ad angles and the briefs of the ad projects they start (#275). An
//! approved angle, its theme, its project and the project's brief are
//! saved in one transaction; a brief is saved whole with its claims. The
//! claims check of an ad script (#276) is saved whole with its
//! statements, one per script of a project.

use bardo_domain::{
    AdAngle, AdAngleId, AdAngleIdea, AdAngleRecord, AdBrief, AdBriefRecord, AdFramework, AdRanking,
    AdRepository, AdStart, BriefClaim, BriefText, ChannelId, CheckedScript, CheckedStatement,
    ClaimId, ClaimKind, ClaimSeen, ClaimsCheck, Confidence, JobId, ProductId, ProfileId, Reason,
    RepositoryError, Score, ThemeId, ThemeStatus, VideoProjectId,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use uuid::Uuid;

use crate::theme::insert_project;
use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const SELECT_ANGLE: &str = "SELECT id, profile_id, product_id, channel_id, title, angle, framework,
        segment, hook, status, suggested_at, position, suggestion_job,
        fit_score, fit_confidence, hook_score, hook_confidence,
        clarity_score, clarity_confidence, ranked_by, ranked_at, theme_id
    FROM ad_angle";

const SELECT_BRIEF: &str = "SELECT project_id, product_id, angle_id, offer, call_to_action, link,
        confirmed_at, changed_at, script_kept_at
    FROM ad_brief";

#[derive(Debug, thiserror::Error)]
#[error("stored ad angle is invalid: {0}")]
struct InvalidRow(String);

fn invalid(detail: impl Into<String>) -> RepositoryError {
    boxed(InvalidRow(detail.into()))
}

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

fn position(index: usize) -> i64 {
    i64::try_from(index).unwrap_or(i64::MAX)
}

/// An ad angle row as stored, before domain validation.
struct AngleRow {
    id: String,
    profile_id: String,
    product_id: String,
    channel_id: String,
    title: String,
    angle: String,
    framework: String,
    segment: String,
    hook: String,
    status: String,
    suggested_at: i64,
    position: i64,
    suggestion_job: Option<String>,
    fit: (Option<i64>, Option<f64>),
    hook_reason: (Option<i64>, Option<f64>),
    clarity: (Option<i64>, Option<f64>),
    ranked_by: Option<String>,
    ranked_at: Option<i64>,
    theme_id: Option<String>,
}

impl AngleRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            product_id: row.get(2)?,
            channel_id: row.get(3)?,
            title: row.get(4)?,
            angle: row.get(5)?,
            framework: row.get(6)?,
            segment: row.get(7)?,
            hook: row.get(8)?,
            status: row.get(9)?,
            suggested_at: row.get(10)?,
            position: row.get(11)?,
            suggestion_job: row.get(12)?,
            fit: (row.get(13)?, row.get(14)?),
            hook_reason: (row.get(15)?, row.get(16)?),
            clarity: (row.get(17)?, row.get(18)?),
            ranked_by: row.get(19)?,
            ranked_at: row.get(20)?,
            theme_id: row.get(21)?,
        })
    }

    /// Rebuilds the angle through domain validation, so a row edited
    /// outside the app cannot load an invalid one.
    fn into_angle(self, claims: Vec<ClaimId>) -> Result<AdAngle, RepositoryError> {
        let reason = |(score, confidence): (Option<i64>, Option<f64>)| -> Option<Reason> {
            Some(Reason {
                score: Score::new(u8::try_from(score?).ok()?),
                confidence: Confidence::new(confidence?),
            })
        };
        let ranking = match (self.ranked_by, self.ranked_at) {
            (Some(model), Some(ranked_at)) => Some(AdRanking {
                audience_fit: reason(self.fit).ok_or_else(|| invalid("incomplete audience fit"))?,
                hook: reason(self.hook_reason).ok_or_else(|| invalid("incomplete hook"))?,
                clarity: reason(self.clarity).ok_or_else(|| invalid("incomplete clarity"))?,
                model,
                ranked_at: from_unix_millis(ranked_at),
            }),
            _ => None,
        };
        let framework = AdFramework::from_code(&self.framework)
            .ok_or_else(|| invalid(format!("framework {:?}", self.framework)))?;
        let idea = AdAngleIdea::new(
            &self.title,
            &self.angle,
            framework,
            &self.segment,
            &self.hook,
            &claims,
        )
        .map_err(|errors| invalid(format!("{errors:?}")))?;
        Ok(AdAngle::restore(AdAngleRecord {
            id: AdAngleId::from(uuid(&self.id)?),
            owner: ProfileId::from(uuid(&self.profile_id)?),
            product: ProductId::from(uuid(&self.product_id)?),
            channel: ChannelId::from(uuid(&self.channel_id)?),
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
            theme: self
                .theme_id
                .as_deref()
                .map(uuid)
                .transpose()?
                .map(ThemeId::from),
        }))
    }
}

/// The claims an angle leans on, in their order.
fn angle_claims(conn: &Connection, angle: &str) -> Result<Vec<ClaimId>, RepositoryError> {
    let ids: Vec<String> = conn
        .prepare_cached("SELECT claim_id FROM ad_angle_claim WHERE angle_id = ?1 ORDER BY position")
        .and_then(|mut statement| {
            statement
                .query_map([angle], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(boxed)?;
    ids.iter().map(|id| uuid(id).map(ClaimId::from)).collect()
}

fn read_angles(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<AdAngle>, RepositoryError> {
    let rows = conn
        .prepare_cached(sql)
        .and_then(|mut statement| {
            statement
                .query_map(params, AngleRow::read)?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(boxed)?;
    rows.into_iter()
        .map(|row| {
            let claims = angle_claims(conn, &row.id)?;
            row.into_angle(claims)
        })
        .collect()
}

fn upsert_angle(tx: &Transaction<'_>, angle: &AdAngle) -> Result<(), RepositoryError> {
    let ranking = angle.ranking();
    let reason = |pick: fn(&AdRanking) -> Reason| {
        ranking.map(|ranking| {
            let reason = pick(ranking);
            (i64::from(reason.score.value()), reason.confidence.value())
        })
    };
    let fit = reason(|r| r.audience_fit);
    let hook = reason(|r| r.hook);
    let clarity = reason(|r| r.clarity);
    let idea = angle.idea();
    let id = angle.id.to_string();
    tx.execute(
        "INSERT INTO ad_angle (id, profile_id, product_id, channel_id, title, angle, framework,
             segment, hook, status, suggested_at, position, suggestion_job,
             fit_score, fit_confidence, hook_score, hook_confidence,
             clarity_score, clarity_confidence, ranked_by, ranked_at, theme_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
             ?19, ?20, ?21, ?22)
         ON CONFLICT (id) DO UPDATE SET
             title = excluded.title,
             angle = excluded.angle,
             framework = excluded.framework,
             segment = excluded.segment,
             hook = excluded.hook,
             status = excluded.status,
             fit_score = excluded.fit_score,
             fit_confidence = excluded.fit_confidence,
             hook_score = excluded.hook_score,
             hook_confidence = excluded.hook_confidence,
             clarity_score = excluded.clarity_score,
             clarity_confidence = excluded.clarity_confidence,
             ranked_by = excluded.ranked_by,
             ranked_at = excluded.ranked_at,
             theme_id = excluded.theme_id",
        params![
            id,
            angle.owner.to_string(),
            angle.product.to_string(),
            angle.channel.to_string(),
            idea.title(),
            idea.angle(),
            idea.framework().code(),
            idea.segment(),
            idea.hook(),
            angle.status().code(),
            to_unix_millis(angle.suggested_at),
            i64::from(angle.position),
            angle.suggestion_job.map(|job| job.to_string()),
            fit.map(|(score, _)| score),
            fit.map(|(_, confidence)| confidence),
            hook.map(|(score, _)| score),
            hook.map(|(_, confidence)| confidence),
            clarity.map(|(score, _)| score),
            clarity.map(|(_, confidence)| confidence),
            ranking.map(|ranking| ranking.model.as_str()),
            ranking.map(|ranking| to_unix_millis(ranking.ranked_at)),
            angle.theme().map(|theme| theme.to_string()),
        ],
    )
    .map_err(boxed)?;
    tx.execute("DELETE FROM ad_angle_claim WHERE angle_id = ?1", [&id])
        .map_err(boxed)?;
    for (index, claim) in idea.claims().iter().enumerate() {
        tx.execute(
            "INSERT INTO ad_angle_claim (angle_id, position, claim_id) VALUES (?1, ?2, ?3)",
            params![id, position(index), claim.to_string()],
        )
        .map_err(boxed)?;
    }
    Ok(())
}

/// A brief row as stored.
struct BriefRow {
    project: String,
    product: String,
    angle: String,
    offer: String,
    call_to_action: String,
    link: String,
    confirmed_at: Option<i64>,
    changed_at: i64,
    script_kept_at: Option<i64>,
}

impl BriefRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            project: row.get(0)?,
            product: row.get(1)?,
            angle: row.get(2)?,
            offer: row.get(3)?,
            call_to_action: row.get(4)?,
            link: row.get(5)?,
            confirmed_at: row.get(6)?,
            changed_at: row.get(7)?,
            script_kept_at: row.get(8)?,
        })
    }

    fn into_brief(self, conn: &Connection) -> Result<AdBrief, RepositoryError> {
        type ClaimRow = (String, String, String, bool, Option<String>, Option<bool>);
        let rows: Vec<ClaimRow> = conn
            .prepare_cached(
                "SELECT claim_id, text, kind, sensitive, kept_text, kept_confirmed
                 FROM ad_brief_claim WHERE project_id = ?1 ORDER BY position",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([&self.project], |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        let claims = rows
            .into_iter()
            .map(
                |(claim, text, kind, sensitive, kept_text, kept_confirmed)| {
                    Ok(BriefClaim {
                        claim: ClaimId::from(uuid(&claim)?),
                        text,
                        kind: ClaimKind::from_code(&kind)
                            .ok_or_else(|| invalid(format!("claim kind {kind:?}")))?,
                        sensitive,
                        kept: match (kept_text, kept_confirmed) {
                            (Some(text), Some(confirmed)) => Some(ClaimSeen { text, confirmed }),
                            _ => None,
                        },
                    })
                },
            )
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        Ok(AdBrief::restore(AdBriefRecord {
            project: VideoProjectId::from(uuid(&self.project)?),
            product: ProductId::from(uuid(&self.product)?),
            angle: AdAngleId::from(uuid(&self.angle)?),
            text: BriefText::new(&self.offer, &self.call_to_action, &self.link)
                .map_err(|errors| invalid(format!("{errors:?}")))?,
            claims,
            confirmed_at: self.confirmed_at.map(from_unix_millis),
            changed_at: from_unix_millis(self.changed_at),
            script_kept_at: self.script_kept_at.map(from_unix_millis),
        }))
    }
}

/// Writes the brief and its claims, in place of what was stored.
fn write_brief(tx: &Transaction<'_>, brief: &AdBrief) -> Result<(), RepositoryError> {
    let project = brief.project.to_string();
    let text = brief.text();
    tx.execute(
        "INSERT INTO ad_brief (project_id, product_id, angle_id, offer, call_to_action, link,
             confirmed_at, changed_at, script_kept_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (project_id) DO UPDATE SET
             offer = excluded.offer,
             call_to_action = excluded.call_to_action,
             link = excluded.link,
             confirmed_at = excluded.confirmed_at,
             changed_at = excluded.changed_at,
             script_kept_at = excluded.script_kept_at",
        params![
            project,
            brief.product.to_string(),
            brief.angle.to_string(),
            text.offer(),
            text.call_to_action(),
            text.link(),
            brief.confirmed_at().map(to_unix_millis),
            to_unix_millis(brief.changed_at()),
            brief.script_kept_at().map(to_unix_millis),
        ],
    )
    .map_err(boxed)?;
    tx.execute(
        "DELETE FROM ad_brief_claim WHERE project_id = ?1",
        [&project],
    )
    .map_err(boxed)?;
    for (index, copy) in brief.claims().iter().enumerate() {
        tx.execute(
            "INSERT INTO ad_brief_claim (project_id, position, claim_id, text, kind, sensitive,
                 kept_text, kept_confirmed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                project,
                position(index),
                copy.claim.to_string(),
                copy.text,
                copy.kind.code(),
                copy.sensitive,
                copy.kept.as_ref().map(|seen| seen.text.as_str()),
                copy.kept.as_ref().map(|seen| seen.confirmed),
            ],
        )
        .map_err(boxed)?;
    }
    Ok(())
}

fn read_briefs(conn: &Connection, sql: &str, id: &str) -> Result<Vec<AdBrief>, RepositoryError> {
    let rows = conn
        .prepare_cached(sql)
        .and_then(|mut statement| {
            statement
                .query_map([id], BriefRow::read)?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(boxed)?;
    rows.into_iter().map(|row| row.into_brief(conn)).collect()
}

/// The statements of a stored check, in order.
fn check_statements(
    conn: &Connection,
    project: &str,
    script: &str,
) -> Result<Vec<CheckedStatement>, RepositoryError> {
    let mut statement = conn
        .prepare(
            "SELECT sentence, says, claim_id FROM claims_check_statement
             WHERE project_id = ?1 AND script = ?2 ORDER BY position",
        )
        .map_err(boxed)?;
    let rows = statement
        .query_map(params![project, script], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(boxed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(boxed)?;
    rows.into_iter()
        .map(|(sentence, says, claim)| {
            Ok(CheckedStatement {
                sentence: sentence
                    .map(|sentence| usize::try_from(sentence).map_err(boxed))
                    .transpose()?,
                says,
                claim: claim
                    .map(|claim| uuid(&claim).map(ClaimId::from))
                    .transpose()?,
            })
        })
        .collect()
}

impl AdRepository for Database {
    fn angles(
        &self,
        product: ProductId,
        channel: ChannelId,
    ) -> Result<Vec<AdAngle>, RepositoryError> {
        read_angles(
            &self.conn(),
            &format!(
                "{SELECT_ANGLE} WHERE product_id = ?1 AND channel_id = ?2
                 ORDER BY suggested_at, position"
            ),
            [product.to_string(), channel.to_string()],
        )
    }

    fn angle(&self, id: AdAngleId) -> Result<Option<AdAngle>, RepositoryError> {
        Ok(read_angles(
            &self.conn(),
            &format!("{SELECT_ANGLE} WHERE id = ?1"),
            [id.to_string()],
        )?
        .pop())
    }

    fn save_angles(&self, angles: &[AdAngle]) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        for angle in angles {
            upsert_angle(&tx, angle)?;
        }
        tx.commit().map_err(boxed)
    }

    fn save_rankings(&self, ranked: &[AdAngle]) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        for angle in ranked {
            let Some(ranking) = angle.ranking() else {
                continue;
            };
            let idea = angle.idea();
            // Only the ranking, and only onto what was ranked: an edit, a
            // discard or an approval made meanwhile stays as it is.
            tx.execute(
                "UPDATE ad_angle SET
                     fit_score = ?1, fit_confidence = ?2,
                     hook_score = ?3, hook_confidence = ?4,
                     clarity_score = ?5, clarity_confidence = ?6,
                     ranked_by = ?7, ranked_at = ?8
                 WHERE id = ?9 AND status = ?10 AND theme_id IS NULL AND ranked_at IS NULL
                     AND title = ?11 AND angle = ?12 AND framework = ?13 AND segment = ?14
                     AND hook = ?15",
                params![
                    i64::from(ranking.audience_fit.score.value()),
                    ranking.audience_fit.confidence.value(),
                    i64::from(ranking.hook.score.value()),
                    ranking.hook.confidence.value(),
                    i64::from(ranking.clarity.score.value()),
                    ranking.clarity.confidence.value(),
                    ranking.model.as_str(),
                    to_unix_millis(ranking.ranked_at),
                    angle.id.to_string(),
                    ThemeStatus::Suggested.code(),
                    idea.title(),
                    idea.angle(),
                    idea.framework().code(),
                    idea.segment(),
                    idea.hook(),
                ],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }

    fn start_ad_project(
        &self,
        angle: &AdAngle,
        start: &AdStart,
        brief: &AdBrief,
    ) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        // An angle starts one project: a second approval, from another
        // window or beside a job, finds it taken.
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM ad_angle WHERE id = ?1",
                [angle.id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(boxed)?;
        if status.as_deref() != Some(ThemeStatus::Suggested.code()) {
            return Err(boxed(std::io::Error::other(
                "the ad angle is no longer a suggestion",
            )));
        }
        // The theme first: the angle and the project both name it.
        insert_project(&tx, &start.theme, &start.project)?;
        upsert_angle(&tx, angle)?;
        write_brief(&tx, brief)?;
        tx.commit().map_err(boxed)
    }

    fn brief(&self, project: VideoProjectId) -> Result<Option<AdBrief>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                &format!("{SELECT_BRIEF} WHERE project_id = ?1"),
                [project.to_string()],
                BriefRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(|row| row.into_brief(&conn)).transpose()
    }

    fn save_brief(&self, brief: &AdBrief) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        write_brief(&tx, brief)?;
        tx.commit().map_err(boxed)
    }

    fn briefs_of(&self, product: ProductId) -> Result<Vec<AdBrief>, RepositoryError> {
        read_briefs(
            &self.conn(),
            &format!("{SELECT_BRIEF} WHERE product_id = ?1 ORDER BY project_id"),
            &product.to_string(),
        )
    }

    fn claims_check(
        &self,
        project: VideoProjectId,
        of: CheckedScript,
    ) -> Result<Option<ClaimsCheck>, RepositoryError> {
        let conn = self.conn();
        let id = project.to_string();
        let row = conn
            .query_row(
                "SELECT text, brief_at, checked_at, model, job_id FROM claims_check
                 WHERE project_id = ?1 AND script = ?2",
                params![id, of.code()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some((text, brief_at, checked_at, model, job)) = row else {
            return Ok(None);
        };
        Ok(Some(ClaimsCheck {
            project,
            of,
            text,
            brief_at: from_unix_millis(brief_at),
            statements: check_statements(&conn, &id, of.code())?,
            checked_at: from_unix_millis(checked_at),
            model,
            job: job.map(|job| uuid(&job).map(JobId::from)).transpose()?,
        }))
    }

    fn save_claims_check(&self, check: &ClaimsCheck) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let (project, script) = (check.project.to_string(), check.of.code());
        // The statements go with the check they belonged to.
        tx.execute(
            "DELETE FROM claims_check WHERE project_id = ?1 AND script = ?2",
            params![project, script],
        )
        .map_err(boxed)?;
        tx.execute(
            "INSERT INTO claims_check
                (project_id, script, text, brief_at, checked_at, model, job_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                project,
                script,
                check.text,
                to_unix_millis(check.brief_at),
                to_unix_millis(check.checked_at),
                check.model,
                check.job.map(|job| job.to_string()),
            ],
        )
        .map_err(boxed)?;
        for (index, statement) in check.statements.iter().enumerate() {
            tx.execute(
                "INSERT INTO claims_check_statement
                    (project_id, script, position, sentence, says, claim_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    project,
                    script,
                    position(index),
                    statement.sentence.map(position),
                    statement.says,
                    statement.claim.map(|claim| claim.to_string()),
                ],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }

    fn drop_claims_check(
        &self,
        project: VideoProjectId,
        of: CheckedScript,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "DELETE FROM claims_check WHERE project_id = ?1 AND script = ?2",
                params![project.to_string(), of.code()],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn accept_claims_check(&self, project: VideoProjectId) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let project = project.to_string();
        tx.execute(
            "DELETE FROM claims_check WHERE project_id = ?1 AND script = ?2",
            params![project, CheckedScript::Current.code()],
        )
        .map_err(boxed)?;
        // Its statements follow it (ON UPDATE CASCADE).
        tx.execute(
            "UPDATE claims_check SET script = ?2 WHERE project_id = ?1 AND script = ?3",
            params![
                project,
                CheckedScript::Current.code(),
                CheckedScript::Pending.code()
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
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, Claim, Niche, Product,
        ProductBrief, ProductDraft, ProductRepository, ProductSheet, ProfileRepository,
        ThemeRepository, ThemeStatus, UiLanguage, UserProfile, VideoLength,
    };

    use super::*;

    fn time(secs: u64) -> SystemTime {
        // Whole milliseconds, as stored.
        SystemTime::UNIX_EPOCH + Duration::from_millis(secs * 1000 + 250)
    }

    struct Setup {
        db: Database,
        channel: Channel,
        product: Product,
        sheet: ProductSheet,
        claims: [ClaimId; 2],
    }

    fn setup() -> Setup {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Desk Life".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(profile.id, details);
        ChannelRepository::save(&db, &channel).unwrap();
        let brief = ProductBrief::validate(ProductDraft {
            name: "Lumen lamp".into(),
            description: "A desk lamp that folds flat.".into(),
            call_to_action: "Order yours today".into(),
            ..ProductDraft::default()
        })
        .unwrap();
        let product = Product::new(profile.id, brief);
        ProductRepository::save(&db, &product).unwrap();
        let mut sheet = ProductSheet::empty(product.id, time(1));
        let folds = sheet
            .add_claim("Folds flat", ClaimKind::Feature, time(1))
            .unwrap();
        let lasts = sheet
            .add_claim("Lasts 40 hours on a charge", ClaimKind::Proof, time(1))
            .unwrap();
        db.save_sheet(&sheet).unwrap();
        Setup {
            db,
            channel,
            product,
            sheet,
            claims: [folds, lasts],
        }
    }

    fn angle(s: &Setup, title: &str, framework: AdFramework, position: u32) -> AdAngle {
        let idea = AdAngleIdea::new(
            title,
            "An angle with açaí and 🚀.",
            framework,
            "People who work late",
            "Still squinting at your desk?",
            &s.claims,
        )
        .unwrap();
        AdAngle::suggested(
            s.product.owner,
            s.product.id,
            s.channel.id,
            idea,
            time(1_800_000_000),
            position,
            Some(JobId::new()),
        )
    }

    fn ranking() -> AdRanking {
        let reason = |score, confidence| Reason {
            score: Score::new(score),
            confidence: Confidence::new(confidence),
        };
        AdRanking {
            audience_fit: reason(81, 0.75),
            hook: reason(64, 0.5),
            clarity: reason(92, 0.875),
            model: "engine-1".into(),
            ranked_at: time(1_800_000_100),
        }
    }

    #[test]
    fn every_framework_round_trips_with_the_angle_that_names_it() {
        let s = setup();
        let angles: Vec<AdAngle> = AdFramework::ALL
            .into_iter()
            .enumerate()
            .map(|(index, framework)| angle(&s, framework.name(), framework, index as u32))
            .collect();
        s.db.save_angles(&angles).unwrap();

        let stored = s.db.angles(s.product.id, s.channel.id).unwrap();
        assert_eq!(stored, angles);
        for (stored, framework) in stored.iter().zip(AdFramework::ALL) {
            assert_eq!(stored.idea().framework(), framework);
            // The code is what is stored: its name and steps come back with it.
            assert_eq!(stored.idea().title(), framework.name());
            assert!(!stored.idea().framework().steps().is_empty());
        }
    }

    #[test]
    fn an_angle_keeps_its_claims_its_ranking_and_what_the_user_made_of_it() {
        let s = setup();
        let mut ranked = angle(&s, "A desk you can see", AdFramework::BeforeAfter, 0);
        ranked.rank(ranking()).unwrap();
        let mut discarded = angle(&s, "Work after dark", AdFramework::Offer, 1);
        discarded.discard().unwrap();
        s.db.save_angles(&[ranked.clone(), discarded.clone()])
            .unwrap();

        assert_eq!(s.db.angle(ranked.id).unwrap(), Some(ranked.clone()));
        assert_eq!(s.db.angle(discarded.id).unwrap(), Some(discarded));
        assert_eq!(s.db.angle(AdAngleId::new()).unwrap(), None);
        assert_eq!(ranked.idea().claims(), s.claims);

        // An edit leans on one claim only and loses the ranking.
        let edited = AdAngleIdea::new(
            "A desk you can see",
            "",
            AdFramework::Demonstration,
            "",
            "Lights on.",
            &s.claims[1..],
        )
        .unwrap();
        ranked.edit(edited).unwrap();
        s.db.save_angles(std::slice::from_ref(&ranked)).unwrap();
        let stored = s.db.angle(ranked.id).unwrap().unwrap();
        assert_eq!(stored, ranked);
        assert_eq!(stored.ranking(), None);
        assert_eq!(stored.idea().claims(), &s.claims[1..]);
    }

    #[test]
    fn angles_belong_to_their_product_and_channel() {
        let s = setup();
        s.db.save_angles(&[angle(&s, "One", AdFramework::Offer, 0)])
            .unwrap();
        assert_eq!(s.db.angles(s.product.id, s.channel.id).unwrap().len(), 1);
        assert!(
            s.db.angles(ProductId::new(), s.channel.id)
                .unwrap()
                .is_empty()
        );
        assert!(
            s.db.angles(s.product.id, ChannelId::new())
                .unwrap()
                .is_empty()
        );
    }

    /// Approves a saved angle and saves what it starts.
    fn started(s: &Setup, title: &str) -> (AdAngle, AdStart, AdBrief) {
        let mut angle = angle(s, title, AdFramework::BeforeAfter, 0);
        s.db.save_angles(std::slice::from_ref(&angle)).unwrap();
        let start = angle
            .approve(Niche::new("Desk lamps").unwrap(), time(1_800_000_200))
            .unwrap();
        let brief = AdBrief::start(
            start.project.id,
            &s.product,
            angle.id,
            &s.sheet,
            time(1_800_000_200),
        );
        s.db.start_ad_project(&angle, &start, &brief).unwrap();
        (angle, start, brief)
    }

    #[test]
    fn approving_saves_the_theme_the_ad_project_and_its_brief_together() {
        let s = setup();
        let (angle, start, brief) = started(&s, "A desk you can see");

        let stored = s.db.angle(angle.id).unwrap().unwrap();
        assert_eq!(stored.status(), ThemeStatus::Approved);
        assert_eq!(stored.theme(), Some(start.theme.id));

        let project = s.db.project(start.project.id).unwrap().unwrap();
        assert_eq!(project, start.project);
        assert_eq!(project.product, Some(s.product.id));
        assert_eq!(project.length, Some(VideoLength::Seconds30));
        assert_eq!(s.db.projects(s.channel.id).unwrap(), [project]);

        let theme = s.db.theme(start.theme.id).unwrap().unwrap();
        assert_eq!(theme.idea().title(), "A desk you can see");
        assert_eq!(theme.niche.label(), "Desk lamps");

        assert_eq!(s.db.brief(start.project.id).unwrap(), Some(brief.clone()));
        assert_eq!(s.db.briefs_of(s.product.id).unwrap(), [brief]);
        assert_eq!(s.db.brief(VideoProjectId::new()).unwrap(), None);
    }

    #[test]
    fn an_angle_starts_one_project_at_most() {
        let s = setup();
        let suggested = angle(&s, "A desk you can see", AdFramework::BeforeAfter, 0);
        s.db.save_angles(std::slice::from_ref(&suggested)).unwrap();
        // Two approvals of the same suggestion, each read before the
        // other was saved.
        let approve = |at: u64| {
            let mut angle = suggested.clone();
            let start = angle
                .approve(Niche::new("Desk lamps").unwrap(), time(at))
                .unwrap();
            let brief = AdBrief::start(start.project.id, &s.product, angle.id, &s.sheet, time(at));
            (angle, start, brief)
        };
        let (first, first_start, first_brief) = approve(1_800_000_200);
        let (second, second_start, second_brief) = approve(1_800_000_300);

        s.db.start_ad_project(&first, &first_start, &first_brief)
            .unwrap();
        assert!(
            s.db.start_ad_project(&second, &second_start, &second_brief)
                .is_err()
        );
        assert_eq!(
            s.db.projects(s.channel.id).unwrap(),
            std::slice::from_ref(&first_start.project)
        );
        assert_eq!(s.db.project(second_start.project.id).unwrap(), None);
        assert_eq!(s.db.angle(suggested.id).unwrap(), Some(first));

        // So is an angle that was never saved.
        let mut unknown = angle(&s, "Never saved", AdFramework::Offer, 1);
        let start = unknown
            .approve(Niche::new("Desk lamps").unwrap(), time(1_800_000_300))
            .unwrap();
        let brief = AdBrief::start(
            start.project.id,
            &s.product,
            unknown.id,
            &s.sheet,
            time(1_800_000_300),
        );
        assert!(s.db.start_ad_project(&unknown, &start, &brief).is_err());
    }

    #[test]
    fn a_ranking_is_written_only_onto_the_angle_that_was_ranked() {
        let s = setup();
        let waiting = angle(&s, "A desk you can see", AdFramework::BeforeAfter, 0);
        let edited = angle(&s, "Work after dark", AdFramework::Offer, 1);
        let discarded = angle(&s, "One lamp, two moods", AdFramework::Demonstration, 2);
        let approved = angle(&s, "The lamp that stays", AdFramework::ThreeReasons, 3);
        s.db.save_angles(&[
            waiting.clone(),
            edited.clone(),
            discarded.clone(),
            approved.clone(),
        ])
        .unwrap();

        // The engine answers about all four as they were read.
        let ranked: Vec<AdAngle> = [&waiting, &edited, &discarded, &approved]
            .into_iter()
            .map(|angle| {
                let mut angle = angle.clone();
                angle.rank(ranking()).unwrap();
                angle
            })
            .collect();

        // Meanwhile the user edits one, discards one and approves one.
        let mut edited_now = edited.clone();
        edited_now
            .edit(
                AdAngleIdea::new(
                    "Work after dark",
                    "",
                    AdFramework::Offer,
                    "",
                    "Another first line.",
                    &s.claims,
                )
                .unwrap(),
            )
            .unwrap();
        let mut discarded_now = discarded.clone();
        discarded_now.discard().unwrap();
        s.db.save_angles(&[edited_now.clone(), discarded_now.clone()])
            .unwrap();
        let mut approved_now = approved.clone();
        let start = approved_now
            .approve(Niche::new("Desk lamps").unwrap(), time(1_800_000_200))
            .unwrap();
        let brief = AdBrief::start(
            start.project.id,
            &s.product,
            approved_now.id,
            &s.sheet,
            time(1_800_000_200),
        );
        s.db.start_ad_project(&approved_now, &start, &brief)
            .unwrap();

        s.db.save_rankings(&ranked).unwrap();

        assert_eq!(s.db.angle(waiting.id).unwrap().as_ref(), Some(&ranked[0]));
        assert_eq!(s.db.angle(edited.id).unwrap(), Some(edited_now));
        assert_eq!(s.db.angle(discarded.id).unwrap(), Some(discarded_now));
        // Still approved, with its theme: the ad's theme stays off Themes.
        assert_eq!(s.db.angle(approved.id).unwrap(), Some(approved_now));
        assert!(s.db.themes(s.channel.id).unwrap().is_empty());

        // A ranking already there is not written over.
        let mut later = waiting.clone();
        later
            .rank(AdRanking {
                model: "another".to_owned(),
                ..ranking()
            })
            .unwrap();
        s.db.save_rankings(&[later]).unwrap();
        assert_eq!(s.db.angle(waiting.id).unwrap().as_ref(), Some(&ranked[0]));
    }

    #[test]
    fn the_theme_of_an_ad_is_not_among_the_channels_themes() {
        let s = setup();
        let (_, start, _) = started(&s, "A desk you can see");
        assert!(s.db.themes(s.channel.id).unwrap().is_empty());
        assert!(s.db.theme(start.theme.id).unwrap().is_some());
    }

    #[test]
    fn a_product_with_an_ad_project_counts_it_and_stays() {
        let s = setup();
        assert_eq!(s.db.projects_using(s.product.id).unwrap(), 0);
        started(&s, "A desk you can see");
        started(&s, "Work after dark");
        assert_eq!(s.db.projects_using(s.product.id).unwrap(), 2);
        assert!(
            ProductRepository::delete(&s.db, s.product.id).is_err(),
            "its projects still name it"
        );
    }

    #[test]
    fn a_product_without_projects_takes_its_angles_along() {
        let s = setup();
        s.db.save_angles(&[angle(&s, "One", AdFramework::Offer, 0)])
            .unwrap();
        ProductRepository::delete(&s.db, s.product.id).unwrap();
        let left: i64 = s
            .db
            .conn()
            .query_row(
                "SELECT (SELECT count(*) FROM ad_angle) + (SELECT count(*) FROM ad_angle_claim)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn a_brief_is_saved_whole_with_its_copies_and_what_the_user_kept() {
        let mut s = setup();
        let (_, start, mut brief) = started(&s, "A desk you can see");
        let [folds, lasts] = s.claims;

        brief.confirm(&s.sheet, time(1_800_000_300)).unwrap();
        brief.set_text(
            BriefText::new("Two for one", "Get yours", "@lumen").unwrap(),
            time(1_800_000_400),
        );
        s.sheet
            .change_claim(folds, time(1_800_000_500), Claim::reject)
            .unwrap();
        brief.keep_claim(folds, &s.sheet).unwrap();
        brief.drop_claim(lasts, time(1_800_000_600)).unwrap();
        brief.keep_script(time(1_800_000_700));
        s.db.save_brief(&brief).unwrap();

        let stored = s.db.brief(start.project.id).unwrap().unwrap();
        assert_eq!(stored, brief);
        assert_eq!(stored.claims().len(), 1);
        assert_eq!(
            stored.claims()[0].kept,
            Some(ClaimSeen {
                text: "Folds flat".into(),
                confirmed: false
            })
        );
        assert_eq!(stored.text().link(), "@lumen");
        assert!(stored.is_confirmed());
    }

    fn check(s: &Setup, project: VideoProjectId, of: CheckedScript, text: &str) -> ClaimsCheck {
        ClaimsCheck {
            project,
            of,
            text: text.to_owned(),
            brief_at: time(1_800_000_800),
            statements: vec![
                CheckedStatement {
                    sentence: Some(1),
                    says: "The lamp folds flat".into(),
                    claim: Some(s.claims[0]),
                },
                CheckedStatement {
                    sentence: None,
                    says: "It is the brightest lamp ever made".into(),
                    claim: None,
                },
            ],
            checked_at: time(1_800_000_900),
            model: "claude-test".into(),
            job: Some(JobId::new()),
        }
    }

    #[test]
    fn a_claims_check_is_saved_whole_and_replaced_whole() {
        let s = setup();
        let (_, start, _) = started(&s, "A desk you can see");
        let project = start.project.id;
        assert_eq!(
            s.db.claims_check(project, CheckedScript::Current).unwrap(),
            None
        );

        let first = check(&s, project, CheckedScript::Current, "It folds flat.");
        s.db.save_claims_check(&first).unwrap();
        assert_eq!(
            s.db.claims_check(project, CheckedScript::Current).unwrap(),
            Some(first)
        );
        assert_eq!(
            s.db.claims_check(project, CheckedScript::Pending).unwrap(),
            None
        );

        let mut again = check(&s, project, CheckedScript::Current, "It folds.");
        again.statements.truncate(1);
        again.job = None;
        s.db.save_claims_check(&again).unwrap();
        assert_eq!(
            s.db.claims_check(project, CheckedScript::Current).unwrap(),
            Some(again)
        );

        s.db.drop_claims_check(project, CheckedScript::Current)
            .unwrap();
        assert_eq!(
            s.db.claims_check(project, CheckedScript::Current).unwrap(),
            None
        );
        let left: i64 =
            s.db.conn()
                .query_row("SELECT COUNT(*) FROM claims_check_statement", [], |row| {
                    row.get(0)
                })
                .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn accepting_gives_the_new_scripts_check_to_the_current_script() {
        let s = setup();
        let (_, start, _) = started(&s, "A desk you can see");
        let project = start.project.id;
        let old = check(&s, project, CheckedScript::Current, "The old script.");
        let new = check(&s, project, CheckedScript::Pending, "The new script.");
        s.db.save_claims_check(&old).unwrap();
        s.db.save_claims_check(&new).unwrap();

        s.db.accept_claims_check(project).unwrap();
        assert_eq!(
            s.db.claims_check(project, CheckedScript::Current).unwrap(),
            Some(new.accepted())
        );
        assert_eq!(
            s.db.claims_check(project, CheckedScript::Pending).unwrap(),
            None
        );

        // With no check of the new script, the old script's still goes:
        // it read another text.
        s.db.accept_claims_check(project).unwrap();
        assert_eq!(
            s.db.claims_check(project, CheckedScript::Current).unwrap(),
            None
        );
    }

    #[test]
    fn a_projects_checks_go_with_it() {
        let s = setup();
        let (_, start, _) = started(&s, "A desk you can see");
        let project = start.project.id;
        s.db.save_claims_check(&check(&s, project, CheckedScript::Current, "It folds."))
            .unwrap();
        s.db.conn()
            .execute(
                "DELETE FROM video_project WHERE id = ?1",
                [project.to_string()],
            )
            .unwrap();
        let left: i64 =
            s.db.conn()
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM claims_check)
                      + (SELECT COUNT(*) FROM claims_check_statement)",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(left, 0);
    }
}
