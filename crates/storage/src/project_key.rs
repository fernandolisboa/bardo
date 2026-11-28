//! Which projects keep their own API keys (#343). The keys are in the
//! secret store; a row here only says one is there, so a job asks the
//! store for a project's key only when the project has one.

use bardo_domain::{
    ProfileId, ProjectId, ProjectKey, ProjectKeyRepository, Provider, RepositoryError,
};
use rusqlite::params;
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

impl ProjectKeyRepository for Database {
    fn project_keys(&self, owner: ProfileId) -> Result<Vec<ProjectKey>, RepositoryError> {
        let rows: Vec<(String, String, i64)> = self
            .conn()
            .prepare(
                "SELECT k.project_id, k.provider, k.saved_at FROM project_key k
                 JOIN project p ON p.id = k.project_id WHERE p.profile_id = ?1
                 ORDER BY k.project_id, k.provider",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([owner.to_string()], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter()
            .map(|(project, provider, saved_at)| {
                Ok(ProjectKey {
                    project: ProjectId::from(Uuid::parse_str(&project).map_err(boxed)?),
                    provider: provider.parse::<Provider>().map_err(boxed)?,
                    saved_at: from_unix_millis(saved_at),
                })
            })
            .collect()
    }

    fn has_project_key(
        &self,
        project: ProjectId,
        provider: Provider,
    ) -> Result<bool, RepositoryError> {
        self.conn()
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM project_key
                                WHERE project_id = ?1 AND provider = ?2)",
                params![project.to_string(), provider.code()],
                |row| row.get(0),
            )
            .map_err(boxed)
    }

    fn save_project_key(&self, key: &ProjectKey) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO project_key (project_id, provider, saved_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (project_id, provider) DO UPDATE SET saved_at = excluded.saved_at",
                params![
                    key.project.to_string(),
                    key.provider.code(),
                    to_unix_millis(key.saved_at),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }

    fn remove_project_key(
        &self,
        project: ProjectId,
        provider: Provider,
    ) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let (project, provider) = (project.to_string(), provider.code());
        tx.execute(
            "DELETE FROM project_key WHERE project_id = ?1 AND provider = ?2",
            params![project, provider],
        )
        .map_err(boxed)?;
        // Its budget counted only what this key paid.
        tx.execute(
            "DELETE FROM project_budget WHERE project_id = ?1 AND provider = ?2",
            params![project, provider],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        Budget, CostRepository as _, Money, ProfileRepository as _, ProjectBudget,
        RemovalRepository as _, UiLanguage, UserProfile,
    };

    use super::*;
    use crate::project::test_project;

    /// Now, kept to the millisecond as stored.
    fn now() -> std::time::SystemTime {
        from_unix_millis(to_unix_millis(std::time::SystemTime::now()))
    }

    #[test]
    fn a_project_notes_its_keys_and_forgets_them_with_their_budgets() {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        db.save(&profile).unwrap();
        let (brand, other) = (test_project(&db, profile.id), test_project(&db, profile.id));
        let key = ProjectKey {
            project: brand,
            provider: Provider::Claude,
            saved_at: now(),
        };
        db.save_project_key(&key).unwrap();
        // Saved again (a replaced key): still one.
        db.save_project_key(&key).unwrap();
        assert_eq!(db.project_keys(profile.id).unwrap(), [key]);
        assert!(db.has_project_key(brand, Provider::Claude).unwrap());
        assert!(!db.has_project_key(brand, Provider::Gemini).unwrap());
        assert!(!db.has_project_key(other, Provider::Claude).unwrap());

        db.save_project_budget(&ProjectBudget {
            project: brand,
            budget: Budget {
                provider: Provider::Claude,
                monthly: Money::from_cents(500),
            },
        })
        .unwrap();
        db.remove_project_key(brand, Provider::Claude).unwrap();
        assert!(db.project_keys(profile.id).unwrap().is_empty());
        assert!(db.project_budgets(profile.id).unwrap().is_empty());
        db.remove_project_key(brand, Provider::Claude).unwrap();

        // Deleting the project takes its notes along.
        db.save_project_key(&key).unwrap();
        db.delete_project(brand, &[]).unwrap();
        assert!(db.project_keys(profile.id).unwrap().is_empty());
    }
}
