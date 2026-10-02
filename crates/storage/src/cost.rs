use std::time::SystemTime;

use bardo_domain::{
    Budget, ChannelId, Cost, CostPurpose, CostRecord, CostRecordId, CostRepository, JobId, Meter,
    Metered, Money, ProfileId, Provider, Rate, RepositoryError, VideoProjectId,
};
use rusqlite::{Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const SELECT_RECORD: &str = "SELECT id, profile_id, provider, model, purpose, input_tokens, \
     output_tokens, image_tokens, characters, basis, amount_micros, channel_id, project_id, \
     job_id, at FROM cost_record";

#[derive(Debug, thiserror::Error)]
#[error("stored cost is invalid: {0}")]
struct InvalidRow(String);

/// A record row as stored, before it is checked.
struct RecordRow {
    id: String,
    profile_id: String,
    provider: String,
    model: String,
    purpose: String,
    usage: [i64; 4],
    basis: String,
    amount_micros: i64,
    channel_id: Option<String>,
    project_id: Option<String>,
    job_id: Option<String>,
    at: i64,
}

impl RecordRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            provider: row.get(2)?,
            model: row.get(3)?,
            purpose: row.get(4)?,
            usage: [row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?],
            basis: row.get(9)?,
            amount_micros: row.get(10)?,
            channel_id: row.get(11)?,
            project_id: row.get(12)?,
            job_id: row.get(13)?,
            at: row.get(14)?,
        })
    }

    fn into_record(self) -> Result<CostRecord, RepositoryError> {
        let count = |value: i64| u64::try_from(value).map_err(boxed);
        let [input, output, image, characters] = self.usage;
        let amount = Money::from_micros(count(self.amount_micros)?);
        let cost = match self.basis.as_str() {
            "reported" => Cost::Reported(amount),
            "estimated" => Cost::Estimated(amount),
            "unpriced" => Cost::Unpriced,
            other => return Err(boxed(InvalidRow(format!("basis {other}")))),
        };
        let id = |text: &str| Uuid::parse_str(text).map_err(boxed);
        Ok(CostRecord {
            id: CostRecordId::from(id(&self.id)?),
            owner: ProfileId::from(id(&self.profile_id)?),
            provider: self.provider.parse().map_err(boxed)?,
            model: self.model,
            purpose: self.purpose.parse().map_err(boxed)?,
            usage: Metered {
                input_tokens: count(input)?,
                output_tokens: count(output)?,
                image_tokens: count(image)?,
                characters: count(characters)?,
            },
            cost,
            channel: self
                .channel_id
                .map(|text| id(&text).map(ChannelId::from))
                .transpose()?,
            project: self
                .project_id
                .map(|text| id(&text).map(VideoProjectId::from))
                .transpose()?,
            job: self
                .job_id
                .map(|text| id(&text).map(JobId::from))
                .transpose()?,
            at: from_unix_millis(self.at),
        })
    }
}

fn basis(cost: &Cost) -> &'static str {
    match cost {
        Cost::Reported(_) => "reported",
        Cost::Estimated(_) => "estimated",
        Cost::Unpriced => "unpriced",
    }
}

fn sql_count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl Database {
    fn query_records(
        &self,
        filter: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<CostRecord>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(&format!("{SELECT_RECORD} {filter}"))
            .and_then(|mut statement| {
                statement
                    .query_map(params, RecordRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter().map(RecordRow::into_record).collect()
    }
}

impl CostRepository for Database {
    fn record_cost(&self, record: &CostRecord) -> Result<(), RepositoryError> {
        let usage = &record.usage;
        self.conn()
            .execute(
                "INSERT INTO cost_record (id, profile_id, provider, model, purpose, \
                 input_tokens, output_tokens, image_tokens, characters, basis, amount_micros, \
                 channel_id, project_id, job_id, at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    record.id.to_string(),
                    record.owner.to_string(),
                    record.provider.code(),
                    record.model,
                    record.purpose.code(),
                    sql_count(usage.input_tokens),
                    sql_count(usage.output_tokens),
                    sql_count(usage.image_tokens),
                    sql_count(usage.characters),
                    basis(&record.cost),
                    sql_count(record.cost.amount().micros()),
                    record.channel.map(|id| id.to_string()),
                    record.project.map(|id| id.to_string()),
                    record.job.map(|id| id.to_string()),
                    to_unix_millis(record.at),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn costs_between(
        &self,
        owner: ProfileId,
        from: SystemTime,
        to: SystemTime,
    ) -> Result<Vec<CostRecord>, RepositoryError> {
        self.query_records(
            "WHERE profile_id = ?1 AND at >= ?2 AND at < ?3 ORDER BY at, rowid",
            params![owner.to_string(), to_unix_millis(from), to_unix_millis(to)],
        )
    }

    fn project_costs(&self, project: VideoProjectId) -> Result<Vec<CostRecord>, RepositoryError> {
        self.query_records(
            "WHERE project_id = ?1 ORDER BY at, rowid",
            [project.to_string()],
        )
    }

    fn recent_costs(
        &self,
        owner: ProfileId,
        purpose: CostPurpose,
        limit: usize,
    ) -> Result<Vec<CostRecord>, RepositoryError> {
        self.query_records(
            "WHERE profile_id = ?1 AND purpose = ?2 ORDER BY at DESC, rowid DESC LIMIT ?3",
            params![
                owner.to_string(),
                purpose.code(),
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
        )
    }

    fn rate_changes(&self, owner: ProfileId) -> Result<Vec<Rate>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare(
                "SELECT provider, model, meter, price_micros FROM rate WHERE profile_id = ?1 \
                 ORDER BY provider, model, meter",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter()
            .map(|(provider, model, meter, price)| {
                Ok(Rate {
                    provider: provider.parse().map_err(boxed)?,
                    model,
                    meter: meter.parse().map_err(boxed)?,
                    price: Money::from_micros(u64::try_from(price).map_err(boxed)?),
                })
            })
            .collect()
    }

    fn save_rate(&self, owner: ProfileId, rate: &Rate) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO rate (profile_id, provider, model, meter, price_micros) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT (profile_id, provider, model, meter) \
                 DO UPDATE SET price_micros = excluded.price_micros",
                params![
                    owner.to_string(),
                    rate.provider.code(),
                    rate.model,
                    rate.meter.code(),
                    sql_count(rate.price.micros()),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn remove_rate(
        &self,
        owner: ProfileId,
        provider: Provider,
        model: &str,
        meter: Meter,
    ) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "DELETE FROM rate WHERE profile_id = ?1 AND provider = ?2 AND model = ?3 \
                 AND meter = ?4",
                params![owner.to_string(), provider.code(), model, meter.code()],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn budgets(&self, owner: ProfileId) -> Result<Vec<Budget>, RepositoryError> {
        let conn = self.conn();
        let rows = conn
            .prepare("SELECT provider, monthly_micros FROM budget WHERE profile_id = ?1")
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        let mut budgets = rows
            .into_iter()
            .map(|(provider, monthly)| {
                Ok(Budget {
                    provider: provider.parse().map_err(boxed)?,
                    monthly: Money::from_micros(u64::try_from(monthly).map_err(boxed)?),
                })
            })
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        budgets.sort_by_key(|budget| budget.provider);
        Ok(budgets)
    }

    fn save_budget(&self, owner: ProfileId, budget: &Budget) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO budget (profile_id, provider, monthly_micros) VALUES (?1, ?2, ?3) \
                 ON CONFLICT (profile_id, provider) \
                 DO UPDATE SET monthly_micros = excluded.monthly_micros",
                params![
                    owner.to_string(),
                    budget.provider.code(),
                    sql_count(budget.monthly.micros()),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn remove_budget(&self, owner: ProfileId, provider: Provider) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "DELETE FROM budget WHERE profile_id = ?1 AND provider = ?2",
                params![owner.to_string(), provider.code()],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bardo_domain::{Month, ProfileRepository, UiLanguage, UserProfile};

    use super::*;

    fn database() -> (Database, ProfileId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        db.save(&profile).unwrap();
        (db, profile.id)
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn record(owner: ProfileId, purpose: CostPurpose, cost: Cost, at: SystemTime) -> CostRecord {
        CostRecord {
            id: CostRecordId::new(),
            owner,
            provider: Provider::Claude,
            model: "claude-opus-5-5".into(),
            purpose,
            usage: Metered {
                input_tokens: 812,
                output_tokens: 2_431,
                image_tokens: 0,
                characters: 0,
            },
            cost,
            channel: Some(ChannelId::new()),
            project: Some(VideoProjectId::new()),
            job: Some(JobId::new()),
            at,
        }
    }

    #[test]
    fn records_round_trip() {
        let (db, owner) = database();
        let estimated = record(
            owner,
            CostPurpose::Script,
            Cost::Estimated(Money::from_micros(51_868)),
            at(1_790_916_201),
        );
        let reported = CostRecord {
            channel: None,
            project: None,
            job: None,
            usage: Metered::characters(1_500),
            ..record(
                owner,
                CostPurpose::Narration,
                Cost::Reported(Money::from_cents(15)),
                at(1_790_916_202),
            )
        };
        let unpriced = record(
            owner,
            CostPurpose::SceneImage,
            Cost::Unpriced,
            at(1_790_916_203),
        );
        for record in [&estimated, &reported, &unpriced] {
            db.record_cost(record).unwrap();
        }
        let october = Month::of(at(1_790_916_201));
        let stored = db
            .costs_between(owner, october.start(), october.end())
            .unwrap();
        assert_eq!(stored, [estimated.clone(), reported, unpriced]);
        assert_eq!(
            db.project_costs(estimated.project.unwrap()).unwrap(),
            [estimated]
        );
    }

    #[test]
    fn a_month_holds_only_its_own_records() {
        let (db, owner) = database();
        let october = Month::new(2026, 10).unwrap();
        let cost = Cost::Estimated(Money::from_cents(1));
        let before = record(
            owner,
            CostPurpose::Script,
            cost,
            october.start() - Duration::from_millis(1),
        );
        let first = record(owner, CostPurpose::Script, cost, october.start());
        let last = record(
            owner,
            CostPurpose::Script,
            cost,
            october.end() - Duration::from_millis(1),
        );
        let after = record(owner, CostPurpose::Script, cost, october.end());
        let other_profile = {
            let profile = UserProfile::new(UiLanguage::EnUs);
            db.save(&profile).unwrap();
            record(profile.id, CostPurpose::Script, cost, october.start())
        };
        for record in [&before, &first, &last, &after, &other_profile] {
            db.record_cost(record).unwrap();
        }
        let ids: Vec<_> = db
            .costs_between(owner, october.start(), october.end())
            .unwrap()
            .iter()
            .map(|record| record.id)
            .collect();
        assert_eq!(ids, [first.id, last.id]);
    }

    #[test]
    fn recent_records_come_newest_first_per_purpose() {
        let (db, owner) = database();
        let cost = Cost::Estimated(Money::from_cents(1));
        let records: Vec<_> = (0..4)
            .map(|n| record(owner, CostPurpose::Script, cost, at(1_000 + n)))
            .collect();
        for record in &records {
            db.record_cost(record).unwrap();
        }
        db.record_cost(&record(owner, CostPurpose::ScenePlan, cost, at(2_000)))
            .unwrap();
        let ids: Vec<_> = db
            .recent_costs(owner, CostPurpose::Script, 3)
            .unwrap()
            .iter()
            .map(|record| record.id)
            .collect();
        assert_eq!(ids, [records[3].id, records[2].id, records[1].id]);
        assert!(
            db.recent_costs(owner, CostPurpose::Narration, 5)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rate_changes_are_saved_replaced_and_removed() {
        let (db, owner) = database();
        let rate = |price: &str| {
            Rate::new(
                Provider::Claude,
                "claude-opus-5-5",
                Meter::InputTokens,
                price,
            )
            .unwrap()
        };
        db.save_rate(owner, &rate("4")).unwrap();
        db.save_rate(owner, &rate("3.5")).unwrap();
        let other = Rate::new(Provider::Gemini, "", Meter::ImageTokens, "60").unwrap();
        db.save_rate(owner, &other).unwrap();
        assert_eq!(
            db.rate_changes(owner).unwrap(),
            [rate("3.5"), other.clone()]
        );

        db.remove_rate(
            owner,
            Provider::Claude,
            "claude-opus-5-5",
            Meter::InputTokens,
        )
        .unwrap();
        assert_eq!(db.rate_changes(owner).unwrap(), [other]);
    }

    #[test]
    fn budgets_are_set_changed_and_removed() {
        let (db, owner) = database();
        let budget = |provider, cents| Budget {
            provider,
            monthly: Money::from_cents(cents),
        };
        db.save_budget(owner, &budget(Provider::Gemini, 500))
            .unwrap();
        db.save_budget(owner, &budget(Provider::Claude, 1_000))
            .unwrap();
        db.save_budget(owner, &budget(Provider::Claude, 2_000))
            .unwrap();
        assert_eq!(
            db.budgets(owner).unwrap(),
            [
                budget(Provider::Claude, 2_000),
                budget(Provider::Gemini, 500)
            ]
        );
        db.remove_budget(owner, Provider::Claude).unwrap();
        assert_eq!(db.budgets(owner).unwrap(), [budget(Provider::Gemini, 500)]);
    }
}
