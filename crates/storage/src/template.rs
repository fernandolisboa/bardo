use bardo_domain::{
    ProfileId, RepositoryError, TemplateBody, TemplateKind, TemplateRepository, TemplateVersion,
    TemplateVersionId,
};
use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

const SELECT_VERSION: &str =
    "SELECT id, profile_id, kind, number, instructions, prompt, created_at FROM template_version";

#[derive(Debug, thiserror::Error)]
#[error("stored template is invalid: {0}")]
struct InvalidRow(String);

/// A template version row as stored, before domain validation.
struct VersionRow {
    id: String,
    profile_id: String,
    kind: String,
    number: i64,
    instructions: String,
    prompt: String,
    created_at: i64,
}

impl VersionRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            kind: row.get(2)?,
            number: row.get(3)?,
            instructions: row.get(4)?,
            prompt: row.get(5)?,
            created_at: row.get(6)?,
        })
    }

    /// Rebuilds the version through domain validation, so a row edited
    /// outside the app cannot load an invalid template.
    fn into_version(self) -> Result<TemplateVersion, RepositoryError> {
        let kind: TemplateKind = self.kind.parse().map_err(boxed)?;
        let body = TemplateBody::new(kind, &self.instructions, &self.prompt)
            .map_err(|errors| boxed(InvalidRow(format!("{errors:?}"))))?;
        Ok(TemplateVersion {
            id: TemplateVersionId::from(Uuid::parse_str(&self.id).map_err(boxed)?),
            owner: ProfileId::from(Uuid::parse_str(&self.profile_id).map_err(boxed)?),
            number: u32::try_from(self.number).map_err(boxed)?,
            body,
            created_at: from_unix_millis(self.created_at),
        })
    }
}

impl TemplateRepository for Database {
    fn template_versions(
        &self,
        owner: ProfileId,
        kind: TemplateKind,
    ) -> Result<Vec<TemplateVersion>, RepositoryError> {
        let rows = self
            .conn()
            .prepare_cached(&format!(
                "{SELECT_VERSION} WHERE profile_id = ?1 AND kind = ?2 ORDER BY number"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map(params![owner.to_string(), kind.code()], VersionRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter().map(VersionRow::into_version).collect()
    }

    fn template_version(
        &self,
        id: TemplateVersionId,
    ) -> Result<Option<TemplateVersion>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                &format!("{SELECT_VERSION} WHERE id = ?1"),
                [id.to_string()],
                VersionRow::read,
            )
            .optional()
            .map_err(boxed)?;
        row.map(VersionRow::into_version).transpose()
    }

    fn add_template_version(&self, version: &TemplateVersion) -> Result<(), RepositoryError> {
        self.conn()
            .execute(
                "INSERT INTO template_version
                     (id, profile_id, kind, number, instructions, prompt, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    version.id.to_string(),
                    version.owner.to_string(),
                    version.kind().code(),
                    i64::from(version.number),
                    version.body.instructions(),
                    version.body.prompt(),
                    to_unix_millis(version.created_at),
                ],
            )
            .map_err(boxed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bardo_domain::{ProfileRepository, UiLanguage, UserProfile};

    use super::*;

    fn time(secs: u64) -> SystemTime {
        // Whole milliseconds, as stored.
        SystemTime::UNIX_EPOCH + Duration::from_millis(secs * 1000 + 250)
    }

    fn setup() -> (Database, ProfileId) {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        (db, profile.id)
    }

    fn body(prompt: &str) -> TemplateBody {
        TemplateBody::new(
            TemplateKind::Script,
            "Write in {{language}}, açaí 🚀.",
            prompt,
        )
        .unwrap()
    }

    #[test]
    fn versions_round_trip_oldest_first() {
        let (db, owner) = setup();
        let first = TemplateVersion::first(owner, body("About {{niche}}."), time(1_800_000_000));
        let second = first
            .revise(body("About {{niche}}, sharper."), time(1_800_000_100))
            .unwrap();
        db.add_template_version(&first).unwrap();
        db.add_template_version(&second).unwrap();

        assert_eq!(
            db.template_versions(owner, TemplateKind::Script).unwrap(),
            [first.clone(), second]
        );
        assert_eq!(db.template_version(first.id).unwrap(), Some(first));
        assert_eq!(db.template_version(TemplateVersionId::new()).unwrap(), None);
        assert!(
            db.template_versions(ProfileId::new(), TemplateKind::Script)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_version_number_is_taken_once() {
        let (db, owner) = setup();
        let first = TemplateVersion::first(owner, body("A"), time(1_800_000_000));
        db.add_template_version(&first).unwrap();
        let mut rival = first.revise(body("B"), time(1_800_000_001)).unwrap();
        rival.number = 1;
        assert!(db.add_template_version(&rival).is_err());
        assert_eq!(
            db.template_versions(owner, TemplateKind::Script)
                .unwrap()
                .len(),
            1
        );
    }
}
