use std::time::Duration;

use bardo_domain::{
    Bitrate, Loudness, MaxDuration, MeasuredLoudness, NetworkAccountId, ProfileId, Render,
    RenderId, RenderPreset, RenderRepository, RenderedCut, RepositoryError, Resolution,
    VideoProjectId,
};
use rusqlite::{OptionalExtension as _, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

/// A value kept in hundredths.
fn cents(value: f64) -> i64 {
    (value * 100.0).round() as i64
}

/// A render row as stored, before domain validation.
struct RenderRow {
    id: String,
    project_id: String,
    profile_id: String,
    account_id: String,
    network: String,
    aspect: String,
    resolution: i64,
    codec: String,
    bitrate_kbps: i64,
    max_duration_secs: i64,
    loudness_tenths: i64,
    file: String,
    encoder: String,
    duration_ns: i64,
    size_bytes: i64,
    integrated_cents: Option<i64>,
    true_peak_cents: Option<i64>,
    cut: String,
    timing: Option<String>,
    rendered_at: i64,
}

impl RenderRow {
    fn into_render(self) -> Result<Render, RepositoryError> {
        let invalid = || boxed(bardo_domain::OutOfRange);
        let resolution = u32::try_from(self.resolution)
            .ok()
            .and_then(Resolution::from_short_side)
            .ok_or_else(invalid)?;
        let preset = RenderPreset {
            aspect: self.aspect.parse().map_err(boxed)?,
            resolution,
            codec: self.codec.parse().map_err(boxed)?,
            bitrate: Bitrate::from_kbps(u32::try_from(self.bitrate_kbps).map_err(boxed)?)
                .map_err(boxed)?,
            max_duration: MaxDuration::from_seconds(
                u32::try_from(self.max_duration_secs).map_err(boxed)?,
            )
            .map_err(boxed)?,
            loudness: Loudness::from_tenths(i16::try_from(self.loudness_tenths).map_err(boxed)?)
                .map_err(boxed)?,
        };
        let loudness = match (self.integrated_cents, self.true_peak_cents) {
            (Some(integrated), Some(true_peak)) => Some(MeasuredLoudness {
                integrated: integrated as f64 / 100.0,
                true_peak: true_peak as f64 / 100.0,
            }),
            _ => None,
        };
        Ok(Render {
            id: RenderId::from(uuid(&self.id)?),
            owner: ProfileId::from(uuid(&self.profile_id)?),
            project: VideoProjectId::from(uuid(&self.project_id)?),
            account: NetworkAccountId::from(uuid(&self.account_id)?),
            network: self.network.parse().map_err(boxed)?,
            preset,
            file: self.file,
            encoder: self.encoder,
            duration: Duration::from_nanos(u64::try_from(self.duration_ns).map_err(boxed)?),
            size_bytes: u64::try_from(self.size_bytes).map_err(boxed)?,
            loudness,
            cut: self.cut,
            timing: self.timing,
            rendered_at: from_unix_millis(self.rendered_at),
        })
    }
}

impl RenderRepository for Database {
    fn renders(&self, project: VideoProjectId) -> Result<Vec<Render>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT render.id, render.project_id, profile_id, account_id, network,
                        render.aspect, resolution, codec, bitrate_kbps, max_duration_secs,
                        loudness_tenths, file, encoder, duration_ns, size_bytes,
                        integrated_cents, true_peak_cents, render.cut, rendered_cut.timing,
                        rendered_at
                 FROM render LEFT JOIN rendered_cut ON rendered_cut.render_id = render.id
                 WHERE render.project_id = ?1",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([project.to_string()], |row| {
                Ok(RenderRow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    profile_id: row.get(2)?,
                    account_id: row.get(3)?,
                    network: row.get(4)?,
                    aspect: row.get(5)?,
                    resolution: row.get(6)?,
                    codec: row.get(7)?,
                    bitrate_kbps: row.get(8)?,
                    max_duration_secs: row.get(9)?,
                    loudness_tenths: row.get(10)?,
                    file: row.get(11)?,
                    encoder: row.get(12)?,
                    duration_ns: row.get(13)?,
                    size_bytes: row.get(14)?,
                    integrated_cents: row.get(15)?,
                    true_peak_cents: row.get(16)?,
                    cut: row.get(17)?,
                    timing: row.get(18)?,
                    rendered_at: row.get(19)?,
                })
            })
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?;
        let mut renders = rows
            .into_iter()
            .map(RenderRow::into_render)
            .collect::<Result<Vec<_>, _>>()?;
        renders.sort_by_key(|render| render.network);
        Ok(renders)
    }

    fn save_render(&self, render: &Render) -> Result<(), RepositoryError> {
        let preset = &render.preset;
        let duration = i64::try_from(render.duration.as_nanos()).map_err(boxed)?;
        let size = i64::try_from(render.size_bytes).map_err(boxed)?;
        // A silent render has no loudness to keep.
        let loudness = render
            .loudness
            .filter(|loudness| loudness.integrated.is_finite() && loudness.true_peak.is_finite());
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        tx.execute(
            "INSERT INTO render (id, project_id, profile_id, account_id, network, aspect,
                                 resolution, codec, bitrate_kbps, max_duration_secs,
                                 loudness_tenths, file, encoder, duration_ns, size_bytes,
                                 integrated_cents, true_peak_cents, cut, rendered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19)
             ON CONFLICT (project_id, account_id) DO UPDATE SET
                 id = excluded.id, network = excluded.network, aspect = excluded.aspect,
                 resolution = excluded.resolution, codec = excluded.codec,
                 bitrate_kbps = excluded.bitrate_kbps,
                 max_duration_secs = excluded.max_duration_secs,
                 loudness_tenths = excluded.loudness_tenths, file = excluded.file,
                 encoder = excluded.encoder, duration_ns = excluded.duration_ns,
                 size_bytes = excluded.size_bytes,
                 integrated_cents = excluded.integrated_cents,
                 true_peak_cents = excluded.true_peak_cents, cut = excluded.cut,
                 rendered_at = excluded.rendered_at",
            params![
                render.id.to_string(),
                render.project.to_string(),
                render.owner.to_string(),
                render.account.to_string(),
                render.network.code(),
                preset.aspect.code(),
                i64::from(preset.resolution.short_side()),
                preset.codec.code(),
                i64::from(preset.bitrate.kbps()),
                i64::from(preset.max_duration.seconds()),
                i64::from(preset.loudness.tenths()),
                render.file,
                render.encoder,
                duration,
                size,
                loudness.map(|loudness| cents(loudness.integrated)),
                loudness.map(|loudness| cents(loudness.true_peak)),
                render.cut,
                to_unix_millis(render.rendered_at),
            ],
        )
        .map_err(boxed)?;
        tx.execute(
            "INSERT INTO rendered_cut (render_id, project_id, length_ns, aspect, cut, timing)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (render_id) DO UPDATE SET
                 length_ns = excluded.length_ns, aspect = excluded.aspect,
                 cut = excluded.cut, timing = excluded.timing",
            params![
                render.id.to_string(),
                render.project.to_string(),
                duration,
                preset.aspect.code(),
                render.cut,
                render.timing,
            ],
        )
        .map_err(boxed)?;
        // What earlier files were cut from goes once nothing points to them.
        tx.execute(
            "DELETE FROM rendered_cut
             WHERE project_id = ?1
               AND render_id NOT IN (SELECT id FROM render WHERE project_id = ?1)
               AND render_id NOT IN (SELECT render_id FROM publication WHERE project_id = ?1)
               AND render_id NOT IN (SELECT render_id FROM export WHERE project_id = ?1)",
            [render.project.to_string()],
        )
        .map_err(boxed)?;
        tx.commit().map_err(boxed)
    }

    fn rendered_cut(&self, render: RenderId) -> Result<Option<RenderedCut>, RepositoryError> {
        let row = self
            .conn()
            .query_row(
                "SELECT length_ns, aspect, cut, timing FROM rendered_cut WHERE render_id = ?1",
                [render.to_string()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        row.map(|(length, aspect, cut, timing)| {
            Ok(RenderedCut {
                render,
                length: Duration::from_nanos(u64::try_from(length).map_err(boxed)?),
                aspect: aspect.parse().map_err(boxed)?,
                cut,
                timing,
            })
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use bardo_domain::{
        AspectRatio, Channel, ChannelDetails, ChannelDraft, ChannelRepository, Network,
        NetworkAccount, NetworkAccountDetails, NetworkAccountDraft, NetworkAccountRepository,
        Niche, PostLink, ProfileRepository, Publication, PublicationId, PublicationKind,
        PublicationRepository, Theme, ThemeIdea, ThemeRepository, UiLanguage, UserProfile,
        VideoCodec, VideoProject,
    };

    use super::*;

    fn time(millis: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
    }

    fn project(db: &Database) -> (VideoProject, Vec<NetworkAccount>) {
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
            time(1_800_000_000_000),
            0,
            None,
        );
        db.save_themes(std::slice::from_ref(&theme)).unwrap();
        let project = theme.approve(time(1_800_000_001_000)).unwrap();
        db.start_project(&theme, &project).unwrap();
        let accounts = [Network::TikTok, Network::YouTube]
            .into_iter()
            .map(|network| {
                let details = NetworkAccountDetails::validate(
                    network,
                    NetworkAccountDraft {
                        handle: "@archives".into(),
                        ..NetworkAccountDraft::default()
                    },
                )
                .unwrap();
                let account = NetworkAccount::new(profile.id, channel.id, network, details);
                NetworkAccountRepository::save(db, &account).unwrap();
                account
            })
            .collect();
        (project, accounts)
    }

    fn render(project: &VideoProject, account: &NetworkAccount, at: u64) -> Render {
        Render {
            id: RenderId::new(),
            owner: project.owner,
            project: project.id,
            account: account.id,
            network: account.network,
            preset: RenderPreset {
                codec: VideoCodec::Hevc,
                aspect: AspectRatio::Landscape,
                ..account.render_preset(bardo_domain::VideoFormat::Short)
            },
            file: format!("render-{}.mp4", account.network.code()),
            encoder: "libopenh264".into(),
            duration: Duration::from_nanos(58_033_333_333),
            size_bytes: 41_234_567,
            loudness: Some(MeasuredLoudness {
                integrated: -14.12,
                true_peak: -1.37,
            }),
            cut: "9f2c1a".into(),
            timing: Some("0011223344556677".into()),
            rendered_at: time(at),
        }
    }

    #[test]
    fn renders_round_trip_in_network_order() {
        let db = Database::open_in_memory().unwrap();
        let (project, accounts) = project(&db);
        let tiktok = render(&project, &accounts[0], 2_000);
        let youtube = render(&project, &accounts[1], 1_000);
        db.save_render(&tiktok).unwrap();
        db.save_render(&youtube).unwrap();
        assert_eq!(db.renders(project.id).unwrap(), [youtube, tiktok]);
        assert_eq!(db.renders(VideoProjectId::new()).unwrap(), []);
    }

    #[test]
    fn a_new_render_for_the_same_account_replaces_the_old_one() {
        let db = Database::open_in_memory().unwrap();
        let (project, accounts) = project(&db);
        db.save_render(&render(&project, &accounts[1], 1_000))
            .unwrap();
        let again = Render {
            loudness: None,
            cut: "77aa00".into(),
            ..render(&project, &accounts[1], 5_000)
        };
        db.save_render(&again).unwrap();
        assert_eq!(db.renders(project.id).unwrap(), [again]);
    }

    #[test]
    fn a_silent_render_keeps_no_loudness() {
        let db = Database::open_in_memory().unwrap();
        let (project, accounts) = project(&db);
        let silent = Render {
            loudness: Some(MeasuredLoudness {
                integrated: f64::NEG_INFINITY,
                true_peak: f64::NEG_INFINITY,
            }),
            ..render(&project, &accounts[0], 1_000)
        };
        db.save_render(&silent).unwrap();
        assert_eq!(db.renders(project.id).unwrap()[0].loudness, None);
    }

    fn cut_of(render: &Render) -> RenderedCut {
        RenderedCut {
            render: render.id,
            length: render.duration,
            aspect: render.preset.aspect,
            cut: render.cut.clone(),
            timing: render.timing.clone(),
        }
    }

    #[test]
    fn a_render_keeps_what_it_was_cut_from() {
        let db = Database::open_in_memory().unwrap();
        let (project, accounts) = project(&db);
        let first = render(&project, &accounts[1], 1_000);
        db.save_render(&first).unwrap();
        assert_eq!(db.rendered_cut(first.id).unwrap(), Some(cut_of(&first)));
        assert_eq!(
            db.renders(project.id).unwrap(),
            std::slice::from_ref(&first)
        );
        let untimed = Render {
            timing: None,
            ..render(&project, &accounts[0], 2_000)
        };
        db.save_render(&untimed).unwrap();
        assert_eq!(db.rendered_cut(untimed.id).unwrap().unwrap().timing, None);
        assert_eq!(db.rendered_cut(RenderId::new()).unwrap(), None);
    }

    #[test]
    fn an_earlier_files_cut_stays_while_a_post_or_an_export_points_to_it() {
        let db = Database::open_in_memory().unwrap();
        let (project, accounts) = project(&db);
        let posted = render(&project, &accounts[1], 1_000);
        db.save_render(&posted).unwrap();
        db.save_publication(&Publication {
            id: PublicationId::new(),
            owner: project.owner,
            project: project.id,
            account: accounts[1].id,
            network: Network::YouTube,
            render: posted.id,
            link: Some(PostLink::parse(Network::YouTube, "https://youtu.be/dQw4w9WgXcQ").unwrap()),
            kind: PublicationKind::Manual,
            posted_at: time(2_000),
            linked_at: time(2_000),
            checked_at: None,
            missing_since: None,
            insights_id: None,
        })
        .unwrap();
        let exported = render(&project, &accounts[0], 1_000);
        db.save_render(&exported).unwrap();
        db.conn()
            .execute(
                "INSERT INTO export (project_id, network, profile_id, account_id, package,
                                     video_file, render_id, post, exported_at)
                 VALUES (?1, 'tiktok', ?2, ?3, 'pkg', 'video.mp4', ?4, '{}', 0)",
                params![
                    project.id.to_string(),
                    project.owner.to_string(),
                    accounts[0].id.to_string(),
                    exported.id.to_string()
                ],
            )
            .unwrap();
        // Both accounts render again: the posted and exported files stay.
        let again = render(&project, &accounts[1], 5_000);
        db.save_render(&again).unwrap();
        db.save_render(&render(&project, &accounts[0], 5_000))
            .unwrap();
        assert_eq!(db.rendered_cut(posted.id).unwrap(), Some(cut_of(&posted)));
        assert_eq!(
            db.rendered_cut(exported.id).unwrap(),
            Some(cut_of(&exported))
        );
        // A file nothing points to is let go at the next render.
        db.save_render(&render(&project, &accounts[1], 9_000))
            .unwrap();
        assert_eq!(db.rendered_cut(again.id).unwrap(), None);
        assert!(db.rendered_cut(posted.id).unwrap().is_some());
    }

    #[test]
    fn upgrading_keeps_what_the_files_rendered_before_were_cut_from() {
        let db = Database::open_in_memory().unwrap();
        let (project, accounts) = project(&db);
        let before = render(&project, &accounts[1], 1_000);
        db.save_render(&before).unwrap();
        {
            // As the database was before migration 49 (and the ones after).
            let mut conn = db.conn();
            conn.execute_batch(
                "DROP TABLE take_staging; DROP TABLE persona_look_asset;
                 DROP TABLE rendered_cut; DROP TABLE adapted_sentence;
                 DROP TABLE localized_audio; DROP TABLE localized_lane;
                 DROP TABLE localized_scene; DROP TABLE localized_sentence;
                 DROP TABLE localization; DROP TABLE persona_look;
                 DROP TABLE control_video; DROP TABLE take_analysis;
                 DROP TABLE take_prompt; DROP TABLE take_result;
                 DROP TABLE performed_clip; DROP TABLE performed_rough_cut;
                 DROP TABLE capture_take; DROP TABLE capture;
                 ALTER TABLE channel DROP COLUMN take_model;
                 ALTER TABLE channel DROP COLUMN take_provider;
                 PRAGMA user_version = 48;",
            )
            .unwrap();
            crate::migrations::run(&mut conn).unwrap();
        }
        assert_eq!(
            db.rendered_cut(before.id).unwrap(),
            Some(RenderedCut {
                timing: None,
                ..cut_of(&before)
            })
        );
        assert_eq!(db.renders(project.id).unwrap()[0].timing, None);
    }
}
