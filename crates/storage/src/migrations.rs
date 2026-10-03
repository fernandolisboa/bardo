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
    include_str!("../migrations/0010_network_account.sql"),
    include_str!("../migrations/0011_cost.sql"),
    include_str!("../migrations/0012_clip.sql"),
    include_str!("../migrations/0013_persona_sharing.sql"),
    include_str!("../migrations/0014_imported_narration.sql"),
    include_str!("../migrations/0015_timeline.sql"),
    include_str!("../migrations/0016_mix.sql"),
    include_str!("../migrations/0017_captions.sql"),
    include_str!("../migrations/0018_framing.sql"),
    include_str!("../migrations/0019_imported_media.sql"),
    include_str!("../migrations/0020_ui_theme.sql"),
    include_str!("../migrations/0021_ui_layout.sql"),
    include_str!("../migrations/0022_render.sql"),
    include_str!("../migrations/0023_export.sql"),
    include_str!("../migrations/0024_publication.sql"),
    include_str!("../migrations/0025_cut_suggestion.sql"),
    include_str!("../migrations/0026_theme_performance.sql"),
    include_str!("../migrations/0027_network_connection.sql"),
    include_str!("../migrations/0028_uploaded_publication.sql"),
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
    fn rebuilding_narrations_keeps_them_their_words_and_rates() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..13] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 13).unwrap();
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
             INSERT INTO narration (project_id, id, profile_id, text, voice_provider,
                 voice_id, voice_name, stability, similarity, style, speed, model,
                 billed_characters, audio_file, duration_ms, generated_at)
             VALUES ('v', 'n', 'p', 'Hi.', 'elevenlabs', 'voice', 'Wyatt', 50, 75, 0, 100,
                 'eleven_multilingual_v2', 3, 'narration-n.mp3', 900, 0);
             INSERT INTO narration_word (narration_id, position, text_start, text_end,
                 start_ms, end_ms)
             VALUES ('n', 0, 0, 3, 0, 800);
             INSERT INTO rate (profile_id, provider, model, meter, price_micros)
             VALUES ('p', 'higgsfield', 'kling-video/', 'video_seconds', 70000);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let (source, voice): (String, String) = conn
            .query_row("SELECT source, voice_name FROM narration", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!((source.as_str(), voice.as_str()), ("generated", "Wyatt"));
        let words: i64 = conn
            .query_row("SELECT COUNT(*) FROM narration_word", [], |row| row.get(0))
            .unwrap();
        assert_eq!(words, 1, "the words are kept");
        assert!(
            conn.execute("DELETE FROM narration", []).is_ok()
                && conn
                    .query_row("SELECT COUNT(*) FROM narration_word", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap()
                    == 0,
            "words still go with their narration"
        );
        let rates: i64 = conn
            .query_row("SELECT COUNT(*) FROM rate", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rates, 1);
        conn.execute(
            "INSERT INTO rate (profile_id, provider, model, meter, price_micros)
             VALUES ('p', 'elevenlabs', 'forced_alignment', 'audio_seconds', 220000)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn rebuilding_publications_keeps_linked_posts_and_their_snapshots() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..27] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 27).unwrap();
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
             INSERT INTO publication (id, project_id, network, profile_id, account_id,
                 render_id, post_id, url, posted_at, linked_at)
             VALUES ('pub', 'v', 'youtube', 'p', 'a', 'r', 'dQw4w9WgXcQ',
                 'https://www.youtube.com/watch?v=dQw4w9WgXcQ', 1, 1);
             INSERT INTO metrics_snapshot (publication_id, taken_at, views)
             VALUES ('pub', 2, 40);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let (kind, post): (String, String) = conn
            .query_row("SELECT kind, post_id FROM publication", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!((kind.as_str(), post.as_str()), ("manual", "dQw4w9WgXcQ"));
        let views: i64 = conn
            .query_row("SELECT views FROM metrics_snapshot", [], |row| row.get(0))
            .unwrap();
        assert_eq!(views, 40, "the snapshots stay");
        assert!(
            conn.execute(
                "INSERT INTO publication (id, project_id, network, profile_id, account_id,
                     render_id, kind, posted_at, linked_at)
                 VALUES ('m', 'v', 'tiktok', 'p', 'a', 'r', 'manual', 1, 1)",
                [],
            )
            .is_err(),
            "a manual publication needs its post"
        );
        conn.execute(
            "INSERT INTO publication (id, project_id, network, profile_id, account_id,
                 render_id, kind, upload_status, upload_visibility, upload_job, posted_at,
                 linked_at)
             VALUES ('u', 'v', 'tiktok', 'p', 'a', 'r', 'uploaded', 'queued', 'public', 'j',
                 1, 1)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM publication WHERE id = 'pub'", [])
            .unwrap();
        let snapshots: i64 = conn
            .query_row("SELECT COUNT(*) FROM metrics_snapshot", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(snapshots, 0, "snapshots still go with their publication");
    }

    #[test]
    fn running_twice_is_a_no_op() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        run(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len());
    }
}
