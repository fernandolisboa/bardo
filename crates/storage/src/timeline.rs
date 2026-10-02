use std::time::Duration;

use bardo_domain::{
    AudioLane, Caption, CaptionStyle, Decibels, Ducking, LaneMix, Mix, NarrationId, ProfileId,
    RepositoryError, SavedAudioItem, SavedCaptions, SavedTimeline, SavedVideoItem, ScenePlanId,
    TimelineRepository, VideoProjectId,
};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::{Database, boxed, from_unix_millis, to_unix_millis};

#[derive(Debug, thiserror::Error)]
#[error("stored timeline is invalid: {0}")]
struct InvalidRow(String);

fn invalid(detail: impl Into<String>) -> RepositoryError {
    boxed(InvalidRow(detail.into()))
}

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

const VIDEO: &str = "video";
const NARRATION: &str = "narration";

fn lane_name(lane: AudioLane) -> &'static str {
    match lane {
        AudioLane::Narration => "narration",
        AudioLane::Music => "music",
        AudioLane::Sfx => "sfx",
    }
}

fn lane_named(name: &str) -> Result<AudioLane, RepositoryError> {
    AudioLane::ALL
        .into_iter()
        .find(|lane| lane_name(*lane) == name)
        .ok_or_else(|| invalid(format!("an audio lane named {name}")))
}

fn tenths(value: i64) -> Result<Decibels, RepositoryError> {
    i16::try_from(value)
        .map(Decibels::from_tenths)
        .map_err(boxed)
}

/// An item row as stored.
struct ItemRow {
    track: String,
    scene: Option<i64>,
    file: Option<String>,
    start: i64,
    at: i64,
    duration: i64,
    fade_in: i64,
    fade_out: i64,
}

impl TimelineRepository for Database {
    fn saved_timeline(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<SavedTimeline>, RepositoryError> {
        let conn = self.conn();
        let project_id = project.to_string();
        let head = conn
            .query_row(
                "SELECT profile_id, scene_plan_id, narration_id, updated_at, duck_music,
                        duck_depth_tenths, has_captions, captions_shown, caption_style
                 FROM timeline WHERE project_id = ?1",
                [&project_id],
                |row| {
                    Ok((
                        (
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                        ),
                        (row.get::<_, bool>(4)?, row.get::<_, i64>(5)?),
                        (
                            row.get::<_, bool>(6)?,
                            row.get::<_, bool>(7)?,
                            row.get::<_, String>(8)?,
                        ),
                    ))
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some((
            (owner, plan, narration, updated_at),
            (duck, depth),
            (has_captions, captions_shown, caption_style),
        )) = head
        else {
            return Ok(None);
        };
        let mut mix = Mix::default();
        mix.ducking = Ducking {
            on: duck,
            depth: tenths(depth)?,
        };
        let mut lanes = conn
            .prepare(
                "SELECT lane, gain_tenths, muted, solo FROM timeline_lane WHERE project_id = ?1",
            )
            .map_err(boxed)?;
        let lanes = lanes
            .query_map([&project_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?;
        for (lane, gain, muted, solo) in lanes {
            mix.set_lane(
                lane_named(&lane)?,
                LaneMix {
                    gain: tenths(gain)?,
                    muted,
                    solo,
                },
            );
        }
        let mut statement = conn
            .prepare(
                "SELECT track, scene, file, start_ns, at_ns, duration_ns, fade_in_ns, fade_out_ns
                 FROM timeline_item
                 WHERE project_id = ?1 ORDER BY track, position",
            )
            .map_err(boxed)?;
        let rows = statement
            .query_map([&project_id], |row| {
                Ok(ItemRow {
                    track: row.get(0)?,
                    scene: row.get(1)?,
                    file: row.get(2)?,
                    start: row.get(3)?,
                    at: row.get(4)?,
                    duration: row.get(5)?,
                    fade_in: row.get(6)?,
                    fade_out: row.get(7)?,
                })
            })
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?;
        let captions = if has_captions {
            let mut statement = conn
                .prepare(
                    "SELECT text, start_ns, end_ns FROM timeline_caption
                     WHERE project_id = ?1 ORDER BY position",
                )
                .map_err(boxed)?;
            let rows = statement
                .query_map([&project_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .map_err(boxed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(boxed)?;
            let lines = rows
                .into_iter()
                .map(|(text, start, end)| {
                    Ok(Caption {
                        text,
                        start: duration(start)?,
                        end: duration(end)?,
                    })
                })
                .collect::<Result<Vec<_>, RepositoryError>>()?;
            Some(SavedCaptions {
                lines,
                shown: captions_shown,
                style: caption_style.parse::<CaptionStyle>().map_err(boxed)?,
            })
        } else {
            None
        };
        let mut saved = SavedTimeline {
            project,
            owner: ProfileId::from(uuid(&owner)?),
            scene_plan: ScenePlanId::from(uuid(&plan)?),
            narration: NarrationId::from(uuid(&narration)?),
            video: Vec::new(),
            narration_items: Vec::new(),
            mix,
            captions,
            updated_at: from_unix_millis(updated_at),
        };
        for row in rows {
            match (row.track.as_str(), row.scene, row.file) {
                (VIDEO, Some(scene), None) => saved.video.push(SavedVideoItem {
                    scene: usize::try_from(scene).map_err(boxed)?,
                    start: duration(row.start)?,
                    duration: duration(row.duration)?,
                }),
                (NARRATION, None, Some(file)) => saved.narration_items.push(SavedAudioItem {
                    file,
                    start: duration(row.start)?,
                    at: duration(row.at)?,
                    duration: duration(row.duration)?,
                    fade_in: duration(row.fade_in)?,
                    fade_out: duration(row.fade_out)?,
                }),
                (track, ..) => return Err(invalid(format!("an item of track {track}"))),
            }
        }
        Ok(Some(saved))
    }

    fn save_timeline(&self, timeline: &SavedTimeline) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        let project = timeline.project.to_string();
        // Its items go with it (ON DELETE CASCADE).
        tx.execute("DELETE FROM timeline WHERE project_id = ?1", [&project])
            .map_err(boxed)?;
        tx.execute(
            "INSERT INTO timeline (project_id, profile_id, scene_plan_id, narration_id, updated_at,
                                   duck_music, duck_depth_tenths, has_captions, captions_shown,
                                   caption_style)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                project,
                timeline.owner.to_string(),
                timeline.scene_plan.to_string(),
                timeline.narration.to_string(),
                to_unix_millis(timeline.updated_at),
                timeline.mix.ducking.on,
                timeline.mix.ducking.depth.tenths(),
                timeline.captions.is_some(),
                timeline
                    .captions
                    .as_ref()
                    .is_none_or(|captions| captions.shown),
                timeline
                    .captions
                    .as_ref()
                    .map_or(CaptionStyle::default(), |captions| captions.style)
                    .code(),
            ],
        )
        .map_err(boxed)?;
        if let Some(captions) = &timeline.captions {
            let mut insert = tx
                .prepare(
                    "INSERT INTO timeline_caption (project_id, position, text, start_ns, end_ns)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(boxed)?;
            for (position, caption) in captions.lines.iter().enumerate() {
                insert
                    .execute(params![
                        project,
                        i64::try_from(position).map_err(boxed)?,
                        caption.text,
                        nanos(caption.start)?,
                        nanos(caption.end)?,
                    ])
                    .map_err(boxed)?;
            }
        }
        for lane in AudioLane::ALL {
            let mix = timeline.mix.lane(lane);
            tx.execute(
                "INSERT INTO timeline_lane (project_id, lane, gain_tenths, muted, solo)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    project,
                    lane_name(lane),
                    mix.gain.tenths(),
                    mix.muted,
                    mix.solo
                ],
            )
            .map_err(boxed)?;
        }
        {
            let mut insert = tx
                .prepare(
                    "INSERT INTO timeline_item
                         (project_id, track, position, scene, file, start_ns, at_ns, duration_ns,
                          fade_in_ns, fade_out_ns)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )
                .map_err(boxed)?;
            let mut at = Duration::ZERO;
            for (position, item) in timeline.video.iter().enumerate() {
                insert
                    .execute(params![
                        project,
                        VIDEO,
                        i64::try_from(position).map_err(boxed)?,
                        Some(i64::try_from(item.scene).map_err(boxed)?),
                        None::<String>,
                        nanos(item.start)?,
                        nanos(at)?,
                        nanos(item.duration)?,
                        0i64,
                        0i64,
                    ])
                    .map_err(boxed)?;
                at += item.duration;
            }
            for (position, item) in timeline.narration_items.iter().enumerate() {
                insert
                    .execute(params![
                        project,
                        NARRATION,
                        i64::try_from(position).map_err(boxed)?,
                        None::<i64>,
                        Some(&item.file),
                        nanos(item.start)?,
                        nanos(item.at)?,
                        nanos(item.duration)?,
                        nanos(item.fade_in)?,
                        nanos(item.fade_out)?,
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
        ThemeIdea, ThemeRepository, UiLanguage, UserProfile, VideoProject,
    };

    use super::*;

    fn time(millis: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
    }

    fn project(db: &Database) -> VideoProject {
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
        project
    }

    fn cut(project: &VideoProject) -> SavedTimeline {
        SavedTimeline {
            project: project.id,
            owner: project.owner,
            scene_plan: ScenePlanId::new(),
            narration: NarrationId::new(),
            video: vec![
                SavedVideoItem {
                    scene: 2,
                    start: Duration::from_nanos(1_500_000_001),
                    duration: Duration::from_nanos(966_666_666),
                },
                SavedVideoItem {
                    scene: 0,
                    start: Duration::ZERO,
                    duration: Duration::from_secs(2),
                },
            ],
            narration_items: vec![
                SavedAudioItem {
                    file: "narration-1.mp3".into(),
                    start: Duration::ZERO,
                    at: Duration::ZERO,
                    duration: Duration::from_millis(1_000),
                    fade_in: Duration::from_nanos(266_666_666),
                    fade_out: Duration::ZERO,
                },
                SavedAudioItem {
                    file: "narration-1.mp3".into(),
                    start: Duration::from_millis(1_400),
                    at: Duration::from_nanos(1_033_333_333),
                    duration: Duration::from_millis(2_123),
                    fade_in: Duration::ZERO,
                    fade_out: Duration::from_millis(500),
                },
            ],
            mix: {
                let mut mix = Mix::default();
                mix.set_lane(
                    AudioLane::Music,
                    LaneMix {
                        gain: Decibels::from_tenths(-65),
                        muted: false,
                        solo: true,
                    },
                );
                mix.set_lane(
                    AudioLane::Narration,
                    LaneMix {
                        gain: Decibels::from_tenths(15),
                        muted: true,
                        solo: false,
                    },
                );
                mix.ducking = Ducking {
                    on: false,
                    depth: Decibels::from_tenths(185),
                };
                mix
            },
            captions: Some(SavedCaptions {
                lines: vec![
                    Caption {
                        text: "The probe went quiet.".into(),
                        start: Duration::from_nanos(120_000_001),
                        end: Duration::from_millis(1_400),
                    },
                    Caption {
                        text: "Nobody knows why — até hoje.".into(),
                        start: Duration::from_millis(1_500),
                        end: Duration::from_millis(3_000),
                    },
                ],
                shown: false,
                style: CaptionStyle::Punch,
            }),
            updated_at: time(1_800_000_002_000),
        }
    }

    #[test]
    fn a_project_never_edited_has_no_saved_timeline() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        assert_eq!(db.saved_timeline(project.id).unwrap(), None);
    }

    #[test]
    fn a_saved_timeline_round_trips_to_the_nanosecond() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let saved = cut(&project);
        db.save_timeline(&saved).unwrap();
        assert_eq!(db.saved_timeline(project.id).unwrap(), Some(saved));
    }

    #[test]
    fn saving_again_replaces_every_item() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        db.save_timeline(&cut(&project)).unwrap();
        let mut shorter = cut(&project);
        shorter.video.truncate(1);
        shorter.narration_items.clear();
        shorter.captions.as_mut().unwrap().lines.truncate(1);
        db.save_timeline(&shorter).unwrap();
        assert_eq!(db.saved_timeline(project.id).unwrap(), Some(shorter));
    }

    #[test]
    fn a_timeline_needs_its_project() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let mut orphan = cut(&project);
        orphan.project = VideoProjectId::new();
        assert!(db.save_timeline(&orphan).is_err());
        assert_eq!(db.saved_timeline(orphan.project).unwrap(), None);
    }

    #[test]
    fn a_cut_saved_before_the_mix_existed_plays_the_default_mix() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let mut saved = cut(&project);
        saved.mix = Mix::default();
        db.save_timeline(&saved).unwrap();
        // As a cut saved by an older Bardo: no lane rows.
        db.conn().execute("DELETE FROM timeline_lane", []).unwrap();
        assert_eq!(db.saved_timeline(project.id).unwrap(), Some(saved));
    }

    #[test]
    fn a_cut_saved_before_captions_existed_has_none() {
        let db = Database::open_in_memory().unwrap();
        let project = project(&db);
        let mut saved = cut(&project);
        db.save_timeline(&saved).unwrap();
        // As a cut saved by an older Bardo: the migration's defaults.
        db.conn()
            .execute(
                "UPDATE timeline SET has_captions = 0, captions_shown = 1,
                     caption_style = 'clean'",
                [],
            )
            .unwrap();
        db.conn()
            .execute("DELETE FROM timeline_caption", [])
            .unwrap();
        saved.captions = None;
        assert_eq!(db.saved_timeline(project.id).unwrap(), Some(saved.clone()));
        // Saving it so keeps it so.
        db.save_timeline(&saved).unwrap();
        assert_eq!(db.saved_timeline(project.id).unwrap(), Some(saved));
    }
}
