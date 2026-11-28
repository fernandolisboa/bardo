use rusqlite::{Connection, TransactionBehavior};

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
    include_str!("../migrations/0029_scheduled_upload.sql"),
    include_str!("../migrations/0030_owner_metrics.sql"),
    include_str!("../migrations/0031_reel_uploads.sql"),
    include_str!("../migrations/0032_in_app_schedule.sql"),
    include_str!("../migrations/0033_tiktok_drafts.sql"),
    include_str!("../migrations/0034_post_insights.sql"),
    include_str!("../migrations/0035_tour_progress.sql"),
    include_str!("../migrations/0036_offer_screen_tours.sql"),
    include_str!("../migrations/0037_background_agent.sql"),
    include_str!("../migrations/0038_backup.sql"),
    include_str!("../migrations/0039_update_check.sql"),
    include_str!("../migrations/0040_youtube_privacy.sql"),
    include_str!("../migrations/0041_x_posts.sql"),
    include_str!("../migrations/0042_generated_music.sql"),
    include_str!("../migrations/0043_video_transitions.sql"),
    include_str!("../migrations/0044_decision_engine.sql"),
    include_str!("../migrations/0045_video_format.sql"),
    include_str!("../migrations/0046_still_motion.sql"),
    include_str!("../migrations/0047_caption_highlight.sql"),
    include_str!("../migrations/0048_youtube_thumbnails_and_chapters.sql"),
    include_str!("../migrations/0049_rendered_cut.sql"),
    include_str!("../migrations/0050_localized_version.sql"),
    include_str!("../migrations/0051_persona_look.sql"),
    include_str!("../migrations/0052_capture.sql"),
    include_str!("../migrations/0053_take_analysis.sql"),
    include_str!("../migrations/0054_control_video.sql"),
    include_str!("../migrations/0055_performed_takes.sql"),
    include_str!("../migrations/0056_performed_cut.sql"),
    include_str!("../migrations/0057_byteplus_takes.sql"),
    include_str!("../migrations/0058_cheaper_takes.sql"),
    include_str!("../migrations/0059_scene_cast.sql"),
    include_str!("../migrations/0060_production_mode.sql"),
    include_str!("../migrations/0061_video_length.sql"),
    include_str!("../migrations/0062_scene_place.sql"),
    include_str!("../migrations/0063_visual_style.sql"),
    include_str!("../migrations/0064_clip_fit.sql"),
    include_str!("../migrations/0065_job_paid_call.sql"),
    include_str!("../migrations/0066_product.sql"),
    include_str!("../migrations/0067_scene_portraits.sql"),
    include_str!("../migrations/0068_spend_cap_kind.sql"),
    include_str!("../migrations/0069_product_sheet.sql"),
    include_str!("../migrations/0070_ad_angle.sql"),
    include_str!("../migrations/0071_claims_check.sql"),
    include_str!("../migrations/0072_scene_kind.sql"),
    include_str!("../migrations/0073_shot_look.sql"),
    include_str!("../migrations/0074_product_check.sql"),
    include_str!("../migrations/0075_product_concept.sql"),
    include_str!("../migrations/0076_end_card.sql"),
    include_str!("../migrations/0077_ad_post_review.sql"),
    include_str!("../migrations/0078_ad_export.sql"),
    include_str!("../migrations/0079_project.sql"),
    include_str!("../migrations/0080_project_folders.sql"),
    include_str!("../migrations/0081_archive_and_delete.sql"),
    include_str!("../migrations/0082_world_characters.sql"),
    include_str!("../migrations/0083_world_places_and_objects.sql"),
    include_str!("../migrations/0084_library_owners.sql"),
    include_str!("../migrations/0085_library_sharing.sql"),
    include_str!("../migrations/0086_loose_videos.sql"),
    include_str!("../migrations/0087_project_keys.sql"),
    include_str!("../migrations/0088_render_settings.sql"),
    include_str!("../migrations/0089_video_summary.sql"),
    include_str!("../migrations/0090_recurring_objects.sql"),
];

/// The schema version this build migrates every database to (SQLite's
/// `user_version` after `run`), shown in an exported diagnostic.
pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

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
            // The background agent may open the same file at the same
            // time: the write lock comes first and the version is read again
            // under it, so each migration runs once.
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let now: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if usize::try_from(now).unwrap_or(0) > index {
                continue;
            }
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
             VALUES ('tv2', 'p', 'image_prompt', 1, '', 'Plan.', 0),
                    ('tv3', 'p', 'ad_angles', 1, '', 'Angles.', 0)",
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
    fn rebuilding_publications_for_drafts_keeps_every_upload_column() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..32] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 32).unwrap();
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
                 render_id, kind, post_id, url, upload_status, upload_visibility,
                 upload_publish_at, upload_job, posted_at, linked_at, checked_at,
                 missing_since, upload_issue, upload_network_id, upload_claimed_at)
             VALUES ('reel', 'v', 'instagram_reels', 'p', 'a', 'r', 'uploaded', 'C1aBcDeFgHi',
                 'https://www.instagram.com/reel/C1aBcDeFgHi/', 'published', 'public', 5, 'j',
                 6, 1, 7, 8, 'no audio', '17900000000000001', 4);
             INSERT INTO metrics_snapshot (publication_id, taken_at, views)
             VALUES ('reel', 2, 40);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        type Row = (
            String,
            Option<i64>,
            i64,
            Option<i64>,
            Option<i64>,
            String,
            String,
            i64,
        );
        let row: Row = conn
            .query_row(
                "SELECT upload_status, upload_publish_at, posted_at, checked_at, missing_since,
                        upload_issue, upload_network_id, upload_claimed_at
                 FROM publication",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "published".into(),
                Some(5),
                6,
                Some(7),
                Some(8),
                "no audio".into(),
                "17900000000000001".into(),
                4
            )
        );
        let snapshots: i64 = conn
            .query_row("SELECT COUNT(*) FROM metrics_snapshot", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(snapshots, 1, "the snapshots stay");
        let draft = |id: &str, post: &str| {
            conn.execute(
                &format!(
                    "INSERT INTO publication (id, project_id, network, profile_id, account_id,
                         render_id, kind, post_id, url, upload_status, upload_visibility,
                         upload_job, posted_at, linked_at)
                     VALUES ('{id}', 'v', 'tiktok', 'p', 'a', 'r', 'uploaded', {post},
                         {post}, 'draft_sent', 'private', 'j2', 1, 1)"
                ),
                [],
            )
        };
        assert!(
            draft("x", "'7301234567890123456'").is_err(),
            "a draft has no post"
        );
        draft("d", "NULL").unwrap();
    }

    #[test]
    fn rebuilding_publications_for_schedules_keeps_uploads_and_snapshots() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..28] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 28).unwrap();
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
                 render_id, kind, post_id, url, upload_status, upload_visibility,
                 upload_job, posted_at, linked_at, checked_at)
             VALUES ('up', 'v', 'youtube', 'p', 'a', 'r', 'uploaded', 'dQw4w9WgXcQ',
                 'https://www.youtube.com/watch?v=dQw4w9WgXcQ', 'restricted', 'public',
                 'j', 1, 1, 3);
             INSERT INTO metrics_snapshot (publication_id, taken_at, views)
             VALUES ('up', 2, 40);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let row: (String, String, Option<i64>, String, Option<i64>) = conn
            .query_row(
                "SELECT upload_status, upload_visibility, upload_publish_at, upload_job,
                        checked_at
                 FROM publication",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "restricted".into(),
                "public".into(),
                None,
                "j".into(),
                Some(3)
            )
        );
        let snapshots: i64 = conn
            .query_row("SELECT COUNT(*) FROM metrics_snapshot", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(snapshots, 1, "the snapshots stay");
        let scheduled = |publish_at: &str| {
            conn.execute(
                &format!(
                    "INSERT INTO publication (id, project_id, network, profile_id, account_id,
                         render_id, kind, upload_status, upload_visibility, upload_publish_at,
                         upload_job, posted_at, linked_at)
                     VALUES ('s', 'v', 'tiktok', 'p', 'a', 'r', 'uploaded', 'scheduled',
                         'public', {publish_at}, 'j2', 1, 1)"
                ),
                [],
            )
        };
        assert!(scheduled("NULL").is_err(), "a schedule needs its time");
        scheduled("5").unwrap();
    }

    #[test]
    fn a_job_waiting_for_a_limit_keeps_its_time_apart_from_retries() {
        let mut conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..31] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 31).unwrap();
        conn.execute_batch(
            "INSERT INTO user_profile (id, ui_language) VALUES ('p', 'en-US');
             INSERT INTO job (id, profile_id, kind, payload, state, progress, attempts,
                              failure_kind, failure_detail, retry_at)
             VALUES ('held', 'p', 'upload', '{}', 'queued', 900, 0, NULL, NULL, 7),
                    ('backoff', 'p', 'upload', '{}', 'queued', 0, 1, 'provider_unavailable',
                     'down', 8),
                    ('ready', 'p', 'upload', '{}', 'queued', 0, 0, NULL, NULL, NULL);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let times = |id: &str| -> (Option<i64>, Option<i64>) {
            conn.query_row(
                "SELECT retry_at, run_at FROM job WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(times("held"), (None, Some(7)), "a wait is not a backoff");
        assert_eq!(times("backoff"), (Some(8), None));
        assert_eq!(times("ready"), (None, None));
    }

    #[test]
    fn rebuilding_rates_for_posts_keeps_the_users_prices() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..40] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 40).unwrap();
        conn.execute_batch(
            "INSERT INTO user_profile (id, ui_language) VALUES ('p', 'en-US');
             INSERT INTO rate (profile_id, provider, model, meter, price_micros)
             VALUES ('p', 'elevenlabs', 'forced_alignment', 'audio_seconds', 220000);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let kept: i64 = conn
            .query_row("SELECT price_micros FROM rate", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kept, 220_000);
        conn.execute(
            "INSERT INTO rate (profile_id, provider, model, meter, price_micros)
             VALUES ('p', 'x', 'post', 'posts', 15000)",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO rate (profile_id, provider, model, meter, price_micros)
                 VALUES ('p', 'x', 'post', 'likes', 1)",
                [],
            )
            .is_err()
        );
    }

    #[test]
    fn rebuilding_rates_for_searches_and_the_cache_keeps_the_users_prices() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        let before = MIGRATIONS
            .iter()
            .position(|sql| sql.contains("CREATE TABLE product_sheet ("))
            .unwrap();
        for sql in &MIGRATIONS[..before] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", i64::try_from(before).unwrap())
            .unwrap();
        conn.execute_batch(
            "INSERT INTO user_profile (id, ui_language) VALUES ('p', 'en-US');
             INSERT INTO rate (profile_id, provider, model, meter, price_micros)
             VALUES ('p', 'x', 'post', 'posts', 15000);
             INSERT INTO cost_record (id, profile_id, provider, model, purpose, input_tokens,
                                      output_tokens, image_tokens, characters, basis,
                                      amount_micros, at)
             VALUES ('c', 'p', 'claude', 'claude-opus-5-5', 'script', 800, 2400, 0, 0,
                     'estimated', 51200, 1790916201000);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let kept: i64 = conn
            .query_row("SELECT price_micros FROM rate", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kept, 15_000);
        let used: (i64, i64, i64) = conn
            .query_row(
                "SELECT searches, cache_write_tokens, cache_read_tokens FROM cost_record",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(used, (0, 0, 0), "an earlier record used none of them");
        for meter in ["searches", "cache_write_tokens", "cache_read_tokens"] {
            conn.execute(
                "INSERT INTO rate (profile_id, provider, model, meter, price_micros)
                 VALUES ('p', 'claude', 'claude-opus-5-5', ?1, 1)",
                [meter],
            )
            .unwrap();
        }
        assert!(
            conn.execute(
                "INSERT INTO rate (profile_id, provider, model, meter, price_micros)
                 VALUES ('p', 'claude', 'claude-opus-5-5', 'fetches', 1)",
                [],
            )
            .is_err()
        );
    }

    #[test]
    fn projects_from_before_default_caps_keep_the_cap_they_had() {
        let before = MIGRATIONS
            .iter()
            .position(|sql| sql.contains("ADD COLUMN spend_cap_kind"))
            .expect("the migration that adds the kind of cap");
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..before] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", before as i64)
            .unwrap();
        // One with a cap its user typed, one without.
        conn.execute_batch(
            "INSERT INTO user_profile (id, ui_language) VALUES ('p', 'en-US');
             INSERT INTO channel (id, profile_id, name, niche, aesthetic_notes, language,
                 country)
             VALUES ('c', 'p', 'Space', '', '', 'en', 'US');
             INSERT INTO theme (id, profile_id, channel_id, niche, title, angle, status,
                 suggested_at, position)
             VALUES ('t1', 'p', 'c', 'space', 'One', '', 'approved', 0, 0),
                    ('t2', 'p', 'c', 'space', 'Two', '', 'approved', 0, 1);
             INSERT INTO video_project (id, profile_id, channel_id, theme_id, niche, title,
                 created_at, spend_cap_micros)
             VALUES ('typed', 'p', 'c', 't1', 'space', 'One', 1, 2500000),
                    ('none', 'p', 'c', 't2', 'space', 'Two', 2, NULL);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let cap = |id: &str| -> (String, Option<i64>) {
            conn.query_row(
                "SELECT spend_cap_kind, spend_cap_micros FROM video_project WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(cap("typed"), ("own".to_owned(), Some(2_500_000)));
        assert_eq!(
            cap("none"),
            ("not_offered".to_owned(), None),
            "offered the default once, not given it"
        );
    }

    #[test]
    fn projects_from_before_ad_angles_advertise_no_product() {
        let before = MIGRATIONS
            .iter()
            .position(|sql| sql.contains("CREATE TABLE ad_angle"))
            .expect("the migration that adds ad angles");
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..before] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", before as i64)
            .unwrap();
        conn.execute_batch(
            "INSERT INTO user_profile (id, ui_language) VALUES ('p', 'en-US');
             INSERT INTO channel (id, profile_id, name, niche, aesthetic_notes, language,
                 country)
             VALUES ('c', 'p', 'Space', '', '', 'en', 'US');
             INSERT INTO theme (id, profile_id, channel_id, niche, title, angle, status,
                 suggested_at, position)
             VALUES ('t', 'p', 'c', 'space', 'One', '', 'approved', 0, 0);
             INSERT INTO video_project (id, profile_id, channel_id, theme_id, niche, title,
                 created_at)
             VALUES ('v', 'p', 'c', 't', 'space', 'One', 1);
             INSERT INTO product (id, profile_id, name, kind, description, audience, facts,
                 offer, link, call_to_action, ownership, must_say, never_say)
             VALUES ('lamp', 'p', 'Lamp', 'physical', 'A lamp.', '', '', '', '', '', 'mine',
                 '', '');",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let product: Option<String> = conn
            .query_row(
                "SELECT product_id FROM video_project WHERE id = 'v'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(product, None);
        // A project that names a product holds it in place.
        conn.execute(
            "UPDATE video_project SET product_id = 'lamp' WHERE id = 'v'",
            [],
        )
        .unwrap();
        assert!(
            conn.execute("DELETE FROM product WHERE id = 'lamp'", [])
                .is_err(),
            "the project still names its product"
        );
        assert!(
            conn.execute(
                "UPDATE video_project SET product_id = 'no-such' WHERE id = 'v'",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn narrators_products_and_templates_from_before_owners_are_global() {
        let before = MIGRATIONS
            .iter()
            .position(|sql| sql.contains("ALTER TABLE persona ADD COLUMN owner_project_id"))
            .expect("the migration that gives library items an owner");
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..before] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", before as i64)
            .unwrap();
        conn.execute_batch(
            "INSERT INTO user_profile (id, ui_language) VALUES ('p', 'en-US');
             INSERT INTO project (id, profile_id, name, color, created_at)
             VALUES ('space', 'p', 'Space', 'gold', 1);
             INSERT INTO persona (id, profile_id, name, voice_provider, voice_id, voice_name,
                 tone, script_style, stability, similarity, style, speed)
             VALUES ('ana', 'p', 'Ana', 'elevenlabs', 'v1', 'Voice', '', '', 50, 75, 0, 100);
             INSERT INTO channel (id, profile_id, owner_project_id, name, niche,
                 aesthetic_notes, language, country, default_persona_id)
             VALUES ('c', 'p', 'space', 'Space', '', '', 'en', 'US', 'ana');
             INSERT INTO product (id, profile_id, name, kind, description, audience, facts,
                 offer, link, call_to_action, ownership, must_say, never_say)
             VALUES ('lamp', 'p', 'Lamp', 'physical', 'A lamp.', '', '', '', '', '', 'mine',
                 '', '');
             INSERT INTO template_version (id, profile_id, kind, number, instructions, prompt,
                 created_at)
             VALUES ('t1', 'p', 'script', 1, 'A', 'B', 1), ('t2', 'p', 'script', 2, 'A', 'C', 2);",
        )
        .unwrap();

        run(&mut conn).unwrap();

        for table in ["persona", "product", "template_version"] {
            let owned: i64 = conn
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE owner_project_id IS NOT NULL"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(owned, 0, "every {table} row is global");
        }
        let narrator: String = conn
            .query_row(
                "SELECT default_persona_id FROM channel WHERE id = 'c'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(narrator, "ana", "the channel keeps its narrator");
        let versions: Vec<(String, i64)> = conn
            .prepare("SELECT id, number FROM template_version ORDER BY number")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(versions, [("t1".into(), 1), ("t2".into(), 2)]);

        // Names and numbers are unique per owner.
        let persona = |id: &str, project: &str| {
            conn.execute(
                &format!(
                    "INSERT INTO persona (id, profile_id, owner_project_id, name, voice_provider,
                         voice_id, voice_name, tone, script_style, stability, similarity, style,
                         speed)
                     VALUES ('{id}', 'p', {project}, 'ANA', 'elevenlabs', 'v1', 'Voice', '', '',
                         50, 75, 0, 100)"
                ),
                [],
            )
        };
        assert!(
            persona("ana-2", "NULL").is_err(),
            "taken in the global library"
        );
        assert!(persona("ana-3", "'space'").is_ok(), "free in the project");
        assert!(persona("ana-4", "'space'").is_err(), "taken in the project");
        assert!(
            conn.execute(
                "INSERT INTO template_version (id, profile_id, owner_project_id, kind, number,
                     instructions, prompt, created_at)
                 VALUES ('own', 'p', 'space', 'script', 1, 'A', 'B', 3)",
                [],
            )
            .is_ok(),
            "a project's own template starts at 1"
        );
        conn.execute("DELETE FROM channel WHERE id = 'c'", [])
            .unwrap();
        conn.execute("DELETE FROM project WHERE id = 'space'", [])
            .unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT (SELECT count(*) FROM persona) + (SELECT count(*) FROM template_version)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(left, 3, "the project's own items went with it");
    }

    /// A database one step before projects, holding `rows`.
    fn before_projects(rows: &str) -> Connection {
        let before = MIGRATIONS
            .iter()
            .position(|sql| sql.contains("CREATE TABLE project ("))
            .expect("the migration that adds projects");
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..before] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", before as i64)
            .unwrap();
        conn.execute_batch(rows).unwrap();
        run(&mut conn).unwrap();
        conn
    }

    /// `(channel, its project, the project's name)`, by channel.
    fn channel_projects(conn: &Connection) -> Vec<(String, String, String)> {
        conn.prepare(
            "SELECT c.id, p.id, p.name FROM channel c
             JOIN project p ON p.id = c.owner_project_id ORDER BY c.id",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    const CHANNELS: &str = "
        INSERT INTO user_profile (id, ui_language) VALUES ('p', 'en-US');
        INSERT INTO channel (id, profile_id, name, niche, aesthetic_notes, language, country,
            created_at)
        VALUES ('en', 'p', 'Space EN', '', '', 'en', 'US', '2026-02-01T00:00:00.000Z'),
               ('pt', 'p', 'Space PT', '', '', 'pt', 'BR', '2026-03-01T00:00:00.000Z'),
               ('es', 'p', 'Space ES', '', '', 'es', 'ES', '2026-01-01T00:00:00.000Z'),
               ('sea', 'p', 'Deep Sea', '', '', 'en', 'US', '2026-04-01T00:00:00.000Z');
        INSERT INTO theme (id, profile_id, channel_id, niche, title, angle, status,
            suggested_at, position)
        VALUES ('t-en', 'p', 'en', 'space', 'Apollo', '', 'approved', 0, 0),
               ('t-pt', 'p', 'pt', 'space', 'Apolo', '', 'approved', 0, 0),
               ('t-pt2', 'p', 'pt', 'space', 'Gemini', '', 'approved', 0, 1),
               ('t-es', 'p', 'es', 'space', 'Géminis', '', 'approved', 0, 0);
        INSERT INTO video_project (id, profile_id, channel_id, theme_id, niche, title,
            created_at)
        VALUES ('v-en', 'p', 'en', 't-en', 'space', 'Apollo', 1),
               ('v-pt', 'p', 'pt', 't-pt', 'space', 'Apolo', 1),
               ('v-pt2', 'p', 'pt', 't-pt2', 'space', 'Gemini', 1),
               ('v-es', 'p', 'es', 't-es', 'space', 'Géminis', 1);";

    /// A localized version `version` made from `source`.
    fn version(version: &str, source: &str) -> String {
        format!(
            "INSERT INTO localization (project_id, profile_id, source_project_id, source_title,
                 source_language, source_country, source_length_ms, plan_id, aspect,
                 caption_style, caption_highlight, captions_shown, duck_music,
                 duck_depth_tenths, created_at)
             VALUES ('{version}', 'p', '{source}', '', 'en', 'US', 1, 'plan', '9:16', 'clean',
                 'off', 1, 1, 60, 1);"
        )
    }

    #[test]
    fn every_channel_becomes_a_project_of_its_own_name() {
        let conn = before_projects(CHANNELS);
        assert_eq!(
            channel_projects(&conn),
            [
                ("en".into(), "en".into(), "Space EN".into()),
                ("es".into(), "es".into(), "Space ES".into()),
                ("pt".into(), "pt".into(), "Space PT".into()),
                ("sea".into(), "sea".into(), "Deep Sea".into()),
            ]
        );
        // Each starts active, unstarred and never opened, with its own
        // color and its channel's creation time.
        let (state, favorite, opened, created, colors): (
            String,
            Option<i64>,
            Option<i64>,
            i64,
            i64,
        ) = conn
            .query_row(
                "SELECT state, favorite_position, opened_at, created_at,
                        (SELECT COUNT(DISTINCT color) FROM project)
                 FROM project WHERE id = 'es'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!((state.as_str(), favorite, opened), ("active", None, None));
        assert_eq!(created, 1_767_225_600_000, "2026-01-01 in milliseconds");
        assert_eq!(colors, 4);
    }

    #[test]
    fn channels_linked_by_a_localized_version_share_the_oldest_ones_project() {
        // pt is a version of en, and es of pt's other video: one chain.
        let conn = before_projects(&format!(
            "{CHANNELS}{}{}",
            version("v-pt", "v-en"),
            version("v-es", "v-pt2")
        ));
        assert_eq!(
            channel_projects(&conn),
            [
                ("en".into(), "es".into(), "Space ES".into()),
                ("es".into(), "es".into(), "Space ES".into()),
                ("pt".into(), "es".into(), "Space ES".into()),
                ("sea".into(), "sea".into(), "Deep Sea".into()),
            ]
        );
        let projects: i64 = conn
            .query_row("SELECT COUNT(*) FROM project", [], |row| row.get(0))
            .unwrap();
        assert_eq!(projects, 2);
    }

    #[test]
    fn jobs_and_costs_remember_their_project_channel_and_video() {
        let conn = before_projects(&format!(
            "{CHANNELS}{}
             INSERT INTO job (id, profile_id, kind, payload, state, progress, attempts)
             VALUES ('video', 'p', 'narration', '{{\"project\":\"v-pt\"}}', 'done', 1000, 1),
                    ('channel', 'p', 'theme-suggestion', '{{\"channel\":\"sea\"}}', 'done',
                        1000, 1),
                    ('plain', 'p', 'countdown', 'plain', 'done', 1000, 1);
             INSERT INTO cost_record (id, profile_id, provider, model, purpose, input_tokens,
                 output_tokens, image_tokens, characters, basis, amount_micros, channel_id,
                 project_id, at)
             VALUES ('video', 'p', 'claude', 'm', 'script', 0, 0, 0, 0, 'estimated', 5, 'en',
                        'v-en', 0),
                    ('channel', 'p', 'claude', 'm', 'script', 0, 0, 0, 0, 'estimated', 5, 'sea',
                        NULL, 0),
                    ('none', 'p', 'claude', 'm', 'script', 0, 0, 0, 0, 'estimated', 5, NULL,
                        NULL, 0);",
            version("v-pt", "v-en")
        ));
        let origins = |table: &str| -> Vec<[Option<String>; 5]> {
            conn.prepare(&format!(
                "SELECT id, owner_project_id, project_name, channel_name, video_title
                 FROM {table} ORDER BY id"
            ))
            .unwrap()
            .query_map([], |row| {
                Ok([
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ])
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
        };
        let some = |text: &str| Some(text.to_owned());
        assert_eq!(
            origins("job"),
            [
                [
                    some("channel"),
                    some("sea"),
                    some("Deep Sea"),
                    some("Deep Sea"),
                    None
                ],
                [some("plain"), None, None, None, None],
                [
                    some("video"),
                    some("en"),
                    some("Space EN"),
                    some("Space PT"),
                    some("Apolo")
                ],
            ]
        );
        assert_eq!(
            origins("cost_record"),
            [
                [
                    some("channel"),
                    some("sea"),
                    some("Deep Sea"),
                    some("Deep Sea"),
                    None
                ],
                [some("none"), None, None, None, None],
                [
                    some("video"),
                    some("en"),
                    some("Space EN"),
                    some("Space EN"),
                    some("Apollo")
                ],
            ]
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
