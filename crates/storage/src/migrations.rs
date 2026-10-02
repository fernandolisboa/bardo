use rusqlite::Connection;

/// Ordered schema migrations. Append only: never edit a released migration.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_user_profile.sql"),
    include_str!("../migrations/0002_channel.sql"),
    include_str!("../migrations/0003_job.sql"),
    include_str!("../migrations/0004_niche_research.sql"),
    include_str!("../migrations/0005_theme.sql"),
    include_str!("../migrations/0006_script.sql"),
    include_str!("../migrations/0007_persona.sql"),
    include_str!("../migrations/0008_narration.sql"),
    include_str!("../migrations/0009_scene.sql"),
];

/// A migration left rows whose foreign keys point nowhere.
#[derive(Debug, thiserror::Error)]
#[error("migration {migration} breaks {rows} foreign key reference(s)")]
pub struct BrokenReferences {
    migration: usize,
    rows: usize,
}

/// Applies every migration newer than the database's `user_version`, each in
/// its own transaction.
///
/// Foreign keys are off while a migration runs, as SQLite's procedure for
/// rebuilding a table requires (dropping a table others refer to would
/// otherwise fail), and every reference is checked before it commits.
pub(crate) fn run(conn: &mut Connection) -> Result<(), MigrationError> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let applied = usize::try_from(current).unwrap_or(0);
    let enforced: bool = conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    conn.pragma_update(None, "foreign_keys", false)?;
    let result = (|| {
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            let broken = {
                let mut check = tx.prepare("PRAGMA foreign_key_check")?;
                let mut rows = check.query([])?;
                let mut count = 0;
                while rows.next()?.is_some() {
                    count += 1;
                }
                count
            };
            if broken > 0 {
                return Err(MigrationError::BrokenReferences(BrokenReferences {
                    migration: index + 1,
                    rows: broken,
                }));
            }
            tx.pragma_update(None, "user_version", index as i64 + 1)?;
            tx.commit()?;
        }
        Ok(())
    })();
    conn.pragma_update(None, "foreign_keys", enforced)?;
    result
}

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    BrokenReferences(BrokenReferences),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(conn: &Connection) -> usize {
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        usize::try_from(version).unwrap()
    }

    #[test]
    fn migrates_a_new_database_to_the_latest_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len());
    }

    #[test]
    fn upgrading_keeps_existing_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO user_profile (id, ui_language) VALUES ('p', 'pt-BR')",
            [],
        )
        .unwrap();

        run(&mut conn).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let language: String = conn
            .query_row("SELECT ui_language FROM user_profile", [], |row| row.get(0))
            .unwrap();
        assert_eq!(language, "pt-BR");
    }

    #[test]
    fn rebuilding_templates_keeps_versions_and_the_generations_that_use_them() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..8] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 8).unwrap();
        conn.execute_batch(
            "INSERT INTO user_profile (id, ui_language) VALUES ('p', 'en-US');
             INSERT INTO channel (id, profile_id, name, niche, aesthetic_notes, language,
                 country)
             VALUES ('c', 'p', 'Space', '', '', 'en', 'US');
             INSERT INTO theme (id, profile_id, channel_id, niche, title, angle, status,
                 suggested_at, position)
             VALUES ('t', 'p', 'c', 'space', 'Probe', '', 'approved', 0, 0);
             INSERT INTO video_project (id, profile_id, channel_id, theme_id, niche, title,
                 created_at)
             VALUES ('v', 'p', 'c', 't', 'space', 'Probe', 0);
             INSERT INTO template_version (id, profile_id, kind, number, instructions,
                 prompt, created_at)
             VALUES ('tv', 'p', 'script', 1, 'Rules.', 'Task.', 0);
             INSERT INTO generation (id, profile_id, project_id, provider, model,
                 template_version_id, instructions, prompt, output, input_tokens,
                 output_tokens, generated_at)
             VALUES ('g', 'p', 'v', 'claude', 'm', 'tv', '', '', 'Draft.', 1, 2, 0);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let template: String = conn
            .query_row(
                "SELECT t.prompt FROM generation g
                 JOIN template_version t ON t.id = g.template_version_id",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(template, "Task.");
        conn.execute(
            "INSERT INTO template_version (id, profile_id, kind, number, instructions,
                 prompt, created_at)
             VALUES ('tv2', 'p', 'image_prompt', 1, '', 'Plan.', 0)",
            [],
        )
        .unwrap();
        let enforced: bool = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert!(enforced, "foreign keys are on again");
        assert!(
            conn.execute("DELETE FROM template_version WHERE id = 'tv'", [])
                .is_err(),
            "the generation still refers to its template"
        );
    }

    #[test]
    fn running_twice_is_a_no_op() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        run(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len());
    }
}
