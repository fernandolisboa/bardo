use bardo_domain::{
    ChannelId, Market, MarketSample, Niche, NicheResearch, NicheResearchRepository, NicheSeeds,
    ProfileId, RepositoryError, UploadSample,
};
use rusqlite::{OptionalExtension, params};

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

#[derive(Debug, thiserror::Error)]
#[error("stored niche research is invalid: {0}")]
struct InvalidRow(String);

fn invalid(detail: impl Into<String>) -> RepositoryError {
    boxed(InvalidRow(detail.into()))
}

fn to_count(value: u64) -> Result<i64, RepositoryError> {
    i64::try_from(value).map_err(boxed)
}

fn from_count(value: i64) -> Result<u64, RepositoryError> {
    u64::try_from(value).map_err(boxed)
}

impl NicheResearchRepository for Database {
    fn cached(
        &self,
        owner: ProfileId,
        niche: &Niche,
        market: Market,
    ) -> Result<Option<NicheResearch>, RepositoryError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT id, niche, fetched_at, upload_volume FROM niche_research
                 WHERE profile_id = ?1 AND niche_key = ?2 AND country = ?3 AND language = ?4",
                params![
                    owner.to_string(),
                    niche.key(),
                    market.country.code(),
                    market.language.code(),
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some((id, label, fetched_at, upload_volume)) = row else {
            return Ok(None);
        };

        let rows = conn
            .prepare_cached(
                "SELECT channel_id, published_at, views, channel_subscribers
                 FROM niche_research_upload WHERE research_id = ?1 ORDER BY position",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        let uploads = rows
            .into_iter()
            .map(|(channel_id, published_at, views, subscribers)| {
                Ok(UploadSample {
                    channel_id,
                    published_at: from_unix_millis(published_at),
                    views: from_count(views)?,
                    channel_subscribers: subscribers.map(from_count).transpose()?,
                })
            })
            .collect::<Result<Vec<_>, RepositoryError>>()?;

        // The label goes through domain validation like any typed niche, so
        // a row edited outside the app cannot load an invalid one.
        let stored = Niche::new(&label).map_err(|error| invalid(format!("{error:?}")))?;
        if stored.key() != niche.key() {
            return Err(invalid("niche does not match its key"));
        }
        Ok(Some(NicheResearch {
            owner,
            niche: stored,
            market,
            fetched_at: from_unix_millis(fetched_at),
            sample: MarketSample {
                upload_volume: from_count(upload_volume)?,
                uploads,
            },
        }))
    }

    fn save(&self, research: &NicheResearch) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id: i64 = tx
            .query_row(
                "INSERT INTO niche_research
                     (profile_id, niche_key, niche, country, language, fetched_at, upload_volume)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (profile_id, niche_key, country, language) DO UPDATE SET
                     niche = excluded.niche,
                     fetched_at = excluded.fetched_at,
                     upload_volume = excluded.upload_volume
                 RETURNING id",
                params![
                    research.owner.to_string(),
                    research.niche.key(),
                    research.niche.label(),
                    research.market.country.code(),
                    research.market.language.code(),
                    to_unix_millis(research.fetched_at),
                    to_count(research.sample.upload_volume)?,
                ],
                |row| row.get(0),
            )
            .map_err(boxed)?;
        tx.execute(
            "DELETE FROM niche_research_upload WHERE research_id = ?1",
            [id],
        )
        .map_err(boxed)?;
        for (position, upload) in research.sample.uploads.iter().enumerate() {
            tx.execute(
                "INSERT INTO niche_research_upload
                     (research_id, position, channel_id, published_at, views, channel_subscribers)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    position as i64,
                    upload.channel_id,
                    to_unix_millis(upload.published_at),
                    to_count(upload.views)?,
                    upload.channel_subscribers.map(to_count).transpose()?,
                ],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }

    fn seeds(&self, channel: ChannelId) -> Result<Vec<Niche>, RepositoryError> {
        let labels = self
            .conn()
            .prepare_cached("SELECT niche FROM niche_seed WHERE channel_id = ?1 ORDER BY position")
            .and_then(|mut statement| {
                statement
                    .query_map([channel.to_string()], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        labels
            .iter()
            .map(|label| Niche::new(label).map_err(|error| invalid(format!("{error:?}"))))
            .collect()
    }

    fn set_seeds(&self, channel: ChannelId, seeds: &NicheSeeds) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let id = channel.to_string();
        tx.execute("DELETE FROM niche_seed WHERE channel_id = ?1", [&id])
            .map_err(boxed)?;
        for (position, niche) in seeds.niches().iter().enumerate() {
            tx.execute(
                "INSERT INTO niche_seed (channel_id, position, niche) VALUES (?1, ?2, ?3)",
                params![id, position as i64, niche.label()],
            )
            .map_err(boxed)?;
        }
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        Channel, ChannelDetails, ChannelDraft, ChannelRepository, ContentLanguage, Country,
        ProfileRepository, UiLanguage, UserProfile,
    };

    use super::*;

    fn database_with_profile() -> (Database, ProfileId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        (db, profile.id)
    }

    fn us() -> Market {
        Market::new(Country::UnitedStates, ContentLanguage::English)
    }

    fn time(secs: u64) -> SystemTime {
        // Whole milliseconds, as stored.
        SystemTime::UNIX_EPOCH + Duration::from_millis(secs * 1000 + 250)
    }

    fn research(owner: ProfileId, label: &str, market: Market) -> NicheResearch {
        NicheResearch {
            owner,
            niche: Niche::new(label).unwrap(),
            market,
            fetched_at: time(1_800_000_000),
            sample: MarketSample {
                upload_volume: 48_213,
                uploads: vec![
                    UploadSample {
                        channel_id: "UCa".into(),
                        published_at: time(1_799_000_000),
                        views: 125_431,
                        channel_subscribers: Some(1_840_000),
                    },
                    UploadSample {
                        channel_id: "UCb".into(),
                        published_at: time(1_799_500_000),
                        views: 0,
                        channel_subscribers: None,
                    },
                ],
            },
        }
    }

    #[test]
    fn a_saved_result_is_read_back_with_every_upload() {
        let (db, owner) = database_with_profile();
        let saved = research(owner, "Space History", us());
        NicheResearchRepository::save(&db, &saved).unwrap();

        assert_eq!(db.cached(owner, &saved.niche, us()).unwrap(), Some(saved));
    }

    #[test]
    fn the_cache_matches_a_niche_ignoring_case_and_spacing() {
        let (db, owner) = database_with_profile();
        NicheResearchRepository::save(&db, &research(owner, "Space History", us())).unwrap();

        let found = db
            .cached(owner, &Niche::new("space   history").unwrap(), us())
            .unwrap()
            .unwrap();
        assert_eq!(found.niche.label(), "Space History");
    }

    #[test]
    fn results_are_kept_per_market_and_owner() {
        let (db, owner) = database_with_profile();
        let other = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &other).unwrap();
        NicheResearchRepository::save(&db, &research(owner, "space", us())).unwrap();

        let niche = Niche::new("space").unwrap();
        let brazil = Market::new(Country::Brazil, ContentLanguage::English);
        let portuguese = Market::new(Country::UnitedStates, ContentLanguage::Portuguese);
        assert_eq!(db.cached(owner, &niche, brazil).unwrap(), None);
        assert_eq!(db.cached(owner, &niche, portuguese).unwrap(), None);
        assert_eq!(db.cached(other.id, &niche, us()).unwrap(), None);
    }

    #[test]
    fn saving_again_replaces_the_result_and_its_uploads() {
        let (db, owner) = database_with_profile();
        NicheResearchRepository::save(&db, &research(owner, "space", us())).unwrap();

        let mut newer = research(owner, "SPACE", us());
        newer.fetched_at = time(1_800_100_000);
        newer.sample.uploads.truncate(1);
        newer.sample.upload_volume = 7;
        NicheResearchRepository::save(&db, &newer).unwrap();

        assert_eq!(db.cached(owner, &newer.niche, us()).unwrap(), Some(newer));
        let rows: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM niche_research_upload", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn a_result_without_uploads_round_trips() {
        let (db, owner) = database_with_profile();
        let mut empty = research(owner, "nothing here", us());
        empty.sample = MarketSample::default();
        NicheResearchRepository::save(&db, &empty).unwrap();
        assert_eq!(db.cached(owner, &empty.niche, us()).unwrap(), Some(empty));
    }

    #[test]
    fn a_result_needs_an_existing_profile() {
        let db = Database::open_in_memory().unwrap();
        assert!(
            NicheResearchRepository::save(&db, &research(ProfileId::new(), "x", us())).is_err()
        );
    }

    fn channel(db: &Database, owner: ProfileId) -> ChannelId {
        let details = ChannelDetails::validate(ChannelDraft {
            name: "Space Archives".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(owner, details);
        ChannelRepository::save(db, &channel).unwrap();
        channel.id
    }

    #[test]
    fn seeds_are_kept_in_order_and_replaced_as_a_whole() {
        let (db, owner) = database_with_profile();
        let id = channel(&db, owner);
        assert!(db.seeds(id).unwrap().is_empty());

        let first = NicheSeeds::parse(&["space history", "true crime", "cozy games"]).unwrap();
        db.set_seeds(id, &first).unwrap();
        assert_eq!(db.seeds(id).unwrap(), first.niches());

        let second = NicheSeeds::parse(&["deep sea"]).unwrap();
        db.set_seeds(id, &second).unwrap();
        assert_eq!(db.seeds(id).unwrap(), second.niches());
    }

    #[test]
    fn seeds_belong_to_an_existing_channel() {
        let (db, _) = database_with_profile();
        let seeds = NicheSeeds::parse(&["space"]).unwrap();
        assert!(db.set_seeds(ChannelId::new(), &seeds).is_err());
    }

    #[test]
    fn results_survive_reopening_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let profile = UserProfile::new(UiLanguage::PtBr);
        let brazil = Market::new(Country::Brazil, ContentLanguage::Portuguese);
        let saved = research(profile.id, "história do espaço", brazil);
        {
            let db = Database::open(&path).unwrap();
            ProfileRepository::save(&db, &profile).unwrap();
            NicheResearchRepository::save(&db, &saved).unwrap();
        }

        let reopened = Database::open(&path).unwrap();
        assert_eq!(
            reopened.cached(profile.id, &saved.niche, brazil).unwrap(),
            Some(saved)
        );
    }
}
