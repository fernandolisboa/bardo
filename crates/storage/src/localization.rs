use std::time::Duration;

use bardo_domain::{
    AudioLane, CarriedAudio, CarriedCut, Ducking, GenerationId, LaneMix, Localization,
    LocalizationRepository, Mix, NewVersion, ProfileId, RepositoryError, SavedTimeline, SceneLook,
    ScenePlan, ScenePlanId, Script, SourceSentence, VideoProject, VideoProjectId,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::media::{insert_asset, insert_track};
use crate::scene::write_scene_plan;
use crate::script::write_script;
use crate::theme::{ProjectRow, SELECT_PROJECT, insert_project};
use crate::timeline::{
    duration, framing, framing_columns, lane_name, lane_named, motion, motion_columns, nanos,
    tenths, transition, transition_columns, write_timeline,
};
use crate::{Database, boxed, from_unix_millis, to_unix_millis};

fn uuid(text: &str) -> Result<Uuid, RepositoryError> {
    Uuid::parse_str(text).map_err(boxed)
}

fn millis(value: Duration) -> Result<i64, RepositoryError> {
    i64::try_from(value.as_millis()).map_err(boxed)
}

fn from_millis(value: i64) -> Result<Duration, RepositoryError> {
    u64::try_from(value)
        .map(Duration::from_millis)
        .map_err(boxed)
}

fn index(value: i64) -> Result<usize, RepositoryError> {
    usize::try_from(value).map_err(boxed)
}

fn position(value: usize) -> Result<i64, RepositoryError> {
    i64::try_from(value).map_err(boxed)
}

/// The version's row, before its sentences, scenes, lanes and sounds.
struct LocalizationRow {
    owner: String,
    source: Option<String>,
    title: String,
    language: String,
    country: String,
    length_ms: i64,
    plan: String,
    aspect: String,
    caption_style: String,
    caption_highlight: String,
    captions_shown: bool,
    duck_music: bool,
    duck_depth: i64,
    created_at: i64,
}

fn sentences(conn: &Connection, project: &str) -> Result<Vec<SourceSentence>, RepositoryError> {
    let mut statement = conn
        .prepare(
            "SELECT text, start_ms FROM localized_sentence WHERE project_id = ?1
             ORDER BY position",
        )
        .map_err(boxed)?;
    let rows = statement
        .query_map([project], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(boxed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(boxed)?;
    rows.into_iter()
        .map(|(text, start)| {
            Ok(SourceSentence {
                text,
                start: from_millis(start)?,
            })
        })
        .collect()
}

/// Per scene: the sentence it starts at and how the source cut showed it.
type SceneRow = (
    i64,
    bool,
    String,
    i64,
    i64,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
);

fn scenes(
    conn: &Connection,
    project: &str,
) -> Result<(Vec<usize>, Vec<Option<SceneLook>>), RepositoryError> {
    let mut statement = conn
        .prepare(
            "SELECT sentence, shown, framing, crop_x, crop_y, transition, transition_frames,
                 motion, motion_strength
             FROM localized_scene WHERE project_id = ?1 ORDER BY position",
        )
        .map_err(boxed)?;
    let rows: Vec<SceneRow> = statement
        .query_map([project], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        })
        .map_err(boxed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(boxed)?;
    let mut starts = Vec::with_capacity(rows.len());
    let mut looks = Vec::with_capacity(rows.len());
    for (sentence, shown, kind, x, y, transition_kind, frames, motion_kind, strength) in rows {
        starts.push(index(sentence)?);
        looks.push(if shown {
            Some(SceneLook {
                framing: framing(&kind, x, y)?,
                motion: motion(motion_kind, strength)?,
                transition: transition(transition_kind, frames)?,
            })
        } else {
            None
        });
    }
    Ok((starts, looks))
}

fn lanes(conn: &Connection, project: &str, mix: &mut Mix) -> Result<(), RepositoryError> {
    let mut statement = conn
        .prepare("SELECT lane, gain_tenths, muted, solo FROM localized_lane WHERE project_id = ?1")
        .map_err(boxed)?;
    let rows = statement
        .query_map([project], |row| {
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
    for (lane, gain, muted, solo) in rows {
        mix.set_lane(
            lane_named(&lane)?,
            LaneMix {
                gain: tenths(gain)?,
                muted,
                solo,
            },
        );
    }
    Ok(())
}

fn audio(conn: &Connection, project: &str) -> Result<Vec<CarriedAudio>, RepositoryError> {
    let mut statement = conn
        .prepare(
            "SELECT track, file, start_ns, at_ns, until_ns, fade_in_ns, fade_out_ns
             FROM localized_audio WHERE project_id = ?1 ORDER BY track, position",
        )
        .map_err(boxed)?;
    let rows = statement
        .query_map([project], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(boxed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(boxed)?;
    rows.into_iter()
        .map(|(track, file, start, at, until, fade_in, fade_out)| {
            Ok(CarriedAudio {
                lane: lane_named(&track)?,
                file,
                start: duration(start)?,
                at: duration(at)?,
                until: duration(until)?,
                fade_in: duration(fade_in)?,
                fade_out: duration(fade_out)?,
            })
        })
        .collect()
}

fn insert_localization(
    tx: &Transaction<'_>,
    localization: &Localization,
) -> Result<(), RepositoryError> {
    let project = localization.project.to_string();
    let cut = &localization.cut;
    tx.execute(
        "INSERT INTO localization (project_id, profile_id, source_project_id, source_title,
             source_language, source_country, source_length_ms, plan_id, aspect,
             caption_style, caption_highlight, captions_shown, duck_music, duck_depth_tenths,
             created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            project,
            localization.owner.to_string(),
            localization.source.map(|id| id.to_string()),
            localization.source_title,
            localization.source_language.code(),
            localization.source_country.code(),
            millis(localization.source_length)?,
            localization.plan.to_string(),
            cut.aspect.code(),
            cut.caption_style.code(),
            cut.caption_highlight.code(),
            cut.captions_shown,
            cut.mix.ducking.on,
            cut.mix.ducking.depth.tenths(),
            to_unix_millis(localization.created_at),
        ],
    )
    .map_err(boxed)?;
    for (at, sentence) in localization.sentences.iter().enumerate() {
        tx.execute(
            "INSERT INTO localized_sentence (project_id, position, text, start_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                project,
                position(at)?,
                sentence.text,
                millis(sentence.start)?
            ],
        )
        .map_err(boxed)?;
    }
    for (at, sentence) in localization.scene_sentences.iter().enumerate() {
        let look = cut.scenes.get(at).copied().flatten();
        let (kind, x, y) = framing_columns(look.map_or_else(Default::default, |look| look.framing));
        let (transition, frames) = transition_columns(look.and_then(|look| look.transition));
        let (motion, strength) = motion_columns(look.and_then(|look| look.motion));
        tx.execute(
            "INSERT INTO localized_scene (project_id, position, sentence, shown, framing, crop_x,
                 crop_y, transition, transition_frames, motion, motion_strength)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                project,
                position(at)?,
                position(*sentence)?,
                look.is_some(),
                kind,
                x,
                y,
                transition,
                frames,
                motion,
                strength,
            ],
        )
        .map_err(boxed)?;
    }
    for lane in AudioLane::ALL {
        let mix = cut.mix.lane(lane);
        tx.execute(
            "INSERT INTO localized_lane (project_id, lane, gain_tenths, muted, solo)
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
    for lane in [AudioLane::Music, AudioLane::Sfx] {
        for (at, item) in cut
            .audio
            .iter()
            .filter(|item| item.lane == lane)
            .enumerate()
        {
            tx.execute(
                "INSERT INTO localized_audio (project_id, track, position, file, start_ns, at_ns,
                     until_ns, fade_in_ns, fade_out_ns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    project,
                    lane_name(lane),
                    position(at)?,
                    item.file,
                    nanos(item.start)?,
                    nanos(item.at)?,
                    nanos(item.until)?,
                    nanos(item.fade_in)?,
                    nanos(item.fade_out)?,
                ],
            )
            .map_err(boxed)?;
        }
    }
    Ok(())
}

fn insert_starts(
    tx: &Transaction<'_>,
    generation: GenerationId,
    starts: &[usize],
) -> Result<(), RepositoryError> {
    let generation = generation.to_string();
    tx.execute(
        "DELETE FROM adapted_sentence WHERE generation_id = ?1",
        [&generation],
    )
    .map_err(boxed)?;
    for (at, start) in starts.iter().enumerate() {
        tx.execute(
            "INSERT INTO adapted_sentence (generation_id, position, text_start)
             VALUES (?1, ?2, ?3)",
            params![generation, position(at)?, position(*start)?],
        )
        .map_err(boxed)?;
    }
    Ok(())
}

impl LocalizationRepository for Database {
    fn localization(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<Localization>, RepositoryError> {
        let conn = self.conn();
        let id = project.to_string();
        let row = conn
            .query_row(
                "SELECT profile_id, source_project_id, source_title, source_language,
                     source_country, source_length_ms, plan_id, aspect, caption_style,
                     caption_highlight, captions_shown, duck_music, duck_depth_tenths, created_at
                 FROM localization WHERE project_id = ?1",
                [&id],
                |row| {
                    Ok(LocalizationRow {
                        owner: row.get(0)?,
                        source: row.get(1)?,
                        title: row.get(2)?,
                        language: row.get(3)?,
                        country: row.get(4)?,
                        length_ms: row.get(5)?,
                        plan: row.get(6)?,
                        aspect: row.get(7)?,
                        caption_style: row.get(8)?,
                        caption_highlight: row.get(9)?,
                        captions_shown: row.get(10)?,
                        duck_music: row.get(11)?,
                        duck_depth: row.get(12)?,
                        created_at: row.get(13)?,
                    })
                },
            )
            .optional()
            .map_err(boxed)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let (scene_sentences, looks) = scenes(&conn, &id)?;
        let mut mix = Mix::default();
        mix.ducking = Ducking {
            on: row.duck_music,
            depth: tenths(row.duck_depth)?,
        };
        lanes(&conn, &id, &mut mix)?;
        Ok(Some(Localization {
            project,
            owner: ProfileId::from(uuid(&row.owner)?),
            source: row
                .source
                .as_deref()
                .map(|id| uuid(id).map(VideoProjectId::from))
                .transpose()?,
            source_title: row.title,
            source_language: row.language.parse().map_err(boxed)?,
            source_country: row.country.parse().map_err(boxed)?,
            source_length: from_millis(row.length_ms)?,
            sentences: sentences(&conn, &id)?,
            plan: ScenePlanId::from(uuid(&row.plan)?),
            scene_sentences,
            cut: CarriedCut {
                aspect: row.aspect.parse().map_err(boxed)?,
                scenes: looks,
                audio: audio(&conn, &id)?,
                mix,
                caption_style: row.caption_style.parse().map_err(boxed)?,
                caption_highlight: row.caption_highlight.parse().map_err(boxed)?,
                captions_shown: row.captions_shown,
            },
            created_at: from_unix_millis(row.created_at),
        }))
    }

    fn versions(&self, source: VideoProjectId) -> Result<Vec<VideoProject>, RepositoryError> {
        let rows = self
            .conn()
            .prepare_cached(&format!(
                "{SELECT_PROJECT} WHERE id IN
                     (SELECT project_id FROM localization WHERE source_project_id = ?1)
                 ORDER BY created_at"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([source.to_string()], ProjectRow::read)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(boxed)?;
        rows.into_iter().map(ProjectRow::into_project).collect()
    }

    fn adapted_starts(
        &self,
        generation: GenerationId,
    ) -> Result<Option<Vec<usize>>, RepositoryError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT text_start FROM adapted_sentence WHERE generation_id = ?1
                 ORDER BY position",
            )
            .map_err(boxed)?;
        let starts = statement
            .query_map([generation.to_string()], |row| row.get::<_, i64>(0))
            .map_err(boxed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(boxed)?
            .into_iter()
            .map(index)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((!starts.is_empty()).then_some(starts))
    }

    fn start_version(&self, version: &NewVersion) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        insert_project(&tx, &version.theme, &version.project)?;
        write_script(&tx, &version.script)?;
        insert_starts(
            &tx,
            version.script.source().generation().id,
            &version.starts,
        )?;
        write_scene_plan(&tx, &version.plan)?;
        for asset in &version.media {
            insert_asset(&tx, asset)?;
        }
        for track in &version.tracks {
            insert_track(&tx, track)?;
        }
        insert_localization(&tx, &version.localization)?;
        tx.commit().map_err(boxed)
    }

    fn save_adaptation(
        &self,
        script: &Script,
        generation: GenerationId,
        starts: &[usize],
    ) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        write_script(&tx, script)?;
        insert_starts(&tx, generation, starts)?;
        tx.commit().map_err(boxed)
    }

    fn place_scenes(&self, plan: &ScenePlan, cut: &SavedTimeline) -> Result<(), RepositoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(boxed)?;
        write_scene_plan(&tx, plan)?;
        write_timeline(&tx, cut)?;
        tx.commit().map_err(boxed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use bardo_domain::{
        AspectRatio, AssetSource, CaptionHighlight, CaptionStyle, Channel, ChannelDetails,
        ChannelDraft, ChannelRepository, ContentLanguage, Country, CropPosition, Framing,
        GeneratedScript, Generation, LocalizationRepository, MediaAsset, MediaAssetId,
        MediaAssetRepository, MediaKind, Motion, MotionKind, MotionStrength, MusicTrack,
        MusicTrackRepository, NarrationId, Niche, ProfileRepository, Provider, Scene, SceneImage,
        ScenePlanRecord, ScenePlanRepository, ScenePrompt, SceneRecord, ScriptRepository,
        TemplateBody, TemplateKind, TemplateRepository, TemplateUsed, TemplateVersion, Theme,
        ThemeIdea, ThemeRepository, TimelineRepository, TokenUsage, Transition, TransitionKind,
        UiLanguage, UserProfile,
    };

    use super::*;

    fn time(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(secs * 1000 + 250)
    }

    struct Setup {
        db: Database,
        source: VideoProject,
        target: Channel,
        template: TemplateVersion,
    }

    fn channel(db: &Database, owner: ProfileId, name: &str) -> Channel {
        let details = ChannelDetails::validate(ChannelDraft {
            name: name.into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        let channel = Channel::new(owner, details);
        ChannelRepository::save(db, &channel).unwrap();
        channel
    }

    fn approved(owner: ProfileId, channel: &Channel, title: &str) -> (Theme, VideoProject) {
        let mut theme = Theme::suggested(
            owner,
            channel.id,
            Niche::new("lighthouses").unwrap(),
            ThemeIdea::new(title, "Why they went dark").unwrap(),
            time(1_800_000_000),
            0,
            None,
        );
        let project = theme.approve(time(1_800_000_001)).unwrap();
        (theme, project)
    }

    fn setup() -> Setup {
        let db = Database::open_in_memory().unwrap();
        let profile = UserProfile::new(UiLanguage::EnUs);
        ProfileRepository::save(&db, &profile).unwrap();
        let source_channel = channel(&db, profile.id, "Lighthouse Tales");
        let target = channel(&db, profile.id, "Contos de Farol");
        let (theme, source) = approved(profile.id, &source_channel, "The dark lighthouses");
        db.start_project(&theme, &source).unwrap();
        let template = TemplateVersion::first(
            profile.id,
            TemplateBody::new(
                TemplateKind::Localization,
                "Adapt.",
                "{{narration_sentences}}",
            )
            .unwrap(),
            time(1_800_000_002),
        );
        db.add_template_version(&template).unwrap();
        Setup {
            db,
            source,
            target,
            template,
        }
    }

    fn generation(setup: &Setup, project: &VideoProject, output: &str) -> Generation {
        Generation {
            id: GenerationId::new(),
            owner: project.owner,
            project: project.id,
            provider: Provider::Claude,
            model: "claude-opus-5-5".into(),
            template: TemplateUsed {
                id: setup.template.id,
                number: setup.template.number,
            },
            instructions: "Adapt.".into(),
            prompt: "[0] 0:00 One. [1] 0:01 Two.".into(),
            output: output.into(),
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
            },
            generated_at: time(1_800_000_010),
            job: None,
        }
    }

    fn version(setup: &Setup) -> NewVersion {
        let (theme, mut project) =
            approved(setup.source.owner, &setup.target, "Os faróis apagados");
        project.format = Some(bardo_domain::VideoFormat::Long);
        let script = Script::first(
            GeneratedScript::new(generation(setup, &project, "Um. Dois.")).unwrap(),
            time(1_800_000_011),
        );
        let prompt = ScenePrompt::new("a lighthouse at dusk").unwrap();
        let scene = |start: u64, end: u64, file: &str| {
            Scene::restore(SceneRecord {
                start: Duration::from_millis(start),
                end: Duration::from_millis(end),
                text: "One.".into(),
                generated_prompt: prompt.clone(),
                prompt: prompt.clone(),
                image: Some(SceneImage {
                    file: file.into(),
                    generation: generation(setup, &project, ""),
                }),
                pending: None,
                failure: None,
                motion_prompt: None,
                clip_model: None,
                clip: None,
                pending_clip: None,
                clip_failure: None,
            })
        };
        let plan = ScenePlan::restore(ScenePlanRecord {
            id: ScenePlanId::new(),
            project: project.id,
            owner: project.owner,
            narration: NarrationId::new(),
            generation: generation(setup, &project, "{}"),
            scenes: vec![
                scene(0, 1_000, "scene-a.png"),
                scene(1_000, 2_000, "scene-b.png"),
            ],
            updated_at: time(1_800_000_012),
        });
        let music = MediaAsset {
            id: MediaAssetId::new(),
            project: project.id,
            owner: project.owner,
            kind: MediaKind::Audio,
            source: AssetSource::Generated,
            file: "media-music.mp3".into(),
            name: "Music.mp3".into(),
            duration: Duration::from_secs(60),
            picture: None,
            imported_at: time(1_800_000_013),
        };
        let track = MusicTrack {
            asset: music.id,
            project: project.id,
            owner: project.owner,
            provider: Provider::ElevenLabs,
            model: "music_v1".into(),
            prompt: "calm strings".into(),
            length: Duration::from_secs(60),
            job: None,
            generated_at: time(1_800_000_013),
            chosen_at: Some(time(1_800_000_014)),
        };
        let mut mix = Mix::default();
        mix.ducking = Ducking {
            on: false,
            depth: bardo_domain::Decibels::from_tenths(90),
        };
        mix.set_lane(
            AudioLane::Sfx,
            LaneMix {
                gain: bardo_domain::Decibels::from_tenths(-30),
                muted: true,
                solo: false,
            },
        );
        let localization = Localization {
            project: project.id,
            owner: project.owner,
            source: Some(setup.source.id),
            source_title: setup.source.title.clone(),
            source_language: ContentLanguage::English,
            source_country: Country::UnitedStates,
            source_length: Duration::from_millis(2_000),
            sentences: vec![
                SourceSentence {
                    text: "One.".into(),
                    start: Duration::ZERO,
                },
                SourceSentence {
                    text: "Two.".into(),
                    start: Duration::from_millis(1_000),
                },
            ],
            plan: plan.id,
            scene_sentences: vec![0, 1],
            cut: CarriedCut {
                aspect: AspectRatio::Vertical,
                scenes: vec![
                    Some(SceneLook {
                        framing: Framing::Crop(CropPosition::new(10, 990)),
                        motion: Some(Motion::new(MotionKind::PanLeft, MotionStrength::Subtle)),
                        transition: None,
                    }),
                    None,
                ],
                audio: vec![
                    CarriedAudio {
                        lane: AudioLane::Music,
                        file: "media-music.mp3".into(),
                        start: Duration::from_millis(250),
                        at: Duration::ZERO,
                        until: Duration::from_millis(2_000),
                        fade_in: Duration::from_millis(500),
                        fade_out: Duration::from_millis(900),
                    },
                    CarriedAudio {
                        lane: AudioLane::Sfx,
                        file: "media-music.mp3".into(),
                        start: Duration::ZERO,
                        at: Duration::from_millis(700),
                        until: Duration::from_millis(900),
                        fade_in: Duration::ZERO,
                        fade_out: Duration::ZERO,
                    },
                ],
                mix,
                caption_style: CaptionStyle::Boxed,
                caption_highlight: CaptionHighlight::default(),
                captions_shown: false,
            },
            created_at: time(1_800_000_015),
        };
        NewVersion {
            theme,
            project,
            script,
            starts: vec![0, 4],
            plan,
            media: vec![music],
            tracks: vec![track],
            localization,
        }
    }

    #[test]
    fn a_version_is_saved_with_everything_it_starts_with() {
        let setup = setup();
        let version = version(&setup);
        setup.db.start_version(&version).unwrap();
        let db = &setup.db;
        let id = version.project.id;
        assert_eq!(db.project(id).unwrap().as_ref(), Some(&version.project));
        assert_eq!(db.script(id).unwrap().as_ref(), Some(&version.script));
        assert_eq!(db.scene_plan(id).unwrap().as_ref(), Some(&version.plan));
        assert_eq!(db.media_assets(id).unwrap(), version.media);
        assert_eq!(
            db.music_tracks(id).unwrap().all(),
            version.tracks.as_slice()
        );
        assert_eq!(
            db.localization(id).unwrap().as_ref(),
            Some(&version.localization)
        );
        assert_eq!(
            db.adapted_starts(version.script.source().generation().id)
                .unwrap(),
            Some(vec![0, 4])
        );
        assert_eq!(
            db.versions(setup.source.id).unwrap(),
            [version.project.clone()]
        );
        assert_eq!(db.localization(setup.source.id).unwrap(), None);
    }

    #[test]
    fn a_version_that_cannot_be_saved_leaves_nothing() {
        let setup = setup();
        let mut version = version(&setup);
        // A track whose asset is not among the copies breaks the save.
        version.media.clear();
        assert!(setup.db.start_version(&version).is_err());
        assert_eq!(setup.db.project(version.project.id).unwrap(), None);
        assert!(setup.db.versions(setup.source.id).unwrap().is_empty());
    }

    #[test]
    fn a_version_outlives_its_source() {
        let setup = setup();
        let version = version(&setup);
        setup.db.start_version(&version).unwrap();
        setup
            .db
            .conn()
            .execute(
                "DELETE FROM video_project WHERE id = ?1",
                [setup.source.id.to_string()],
            )
            .unwrap();
        let kept = setup.db.localization(version.project.id).unwrap().unwrap();
        assert_eq!(kept.source, None);
        assert_eq!(kept.source_title, "The dark lighthouses");
    }

    #[test]
    fn an_adaptation_again_is_saved_with_its_sentence_starts() {
        let setup = setup();
        let version = version(&setup);
        setup.db.start_version(&version).unwrap();
        let mut script = version.script.clone();
        let again = generation(&setup, &version.project, "Um! Dois!");
        script.offer(
            GeneratedScript::new(again.clone()).unwrap(),
            time(1_800_000_020),
        );
        setup
            .db
            .save_adaptation(&script, again.id, &[0, 4])
            .unwrap();
        assert_eq!(setup.db.script(version.project.id).unwrap(), Some(script));
        assert_eq!(setup.db.adapted_starts(again.id).unwrap(), Some(vec![0, 4]));
        assert_eq!(setup.db.adapted_starts(GenerationId::new()).unwrap(), None);
    }

    #[test]
    fn placing_scenes_saves_the_plan_on_its_new_narration_and_the_cut() {
        let setup = setup();
        let version = version(&setup);
        setup.db.start_version(&version).unwrap();
        let mut record = ScenePlanRecord {
            id: version.plan.id,
            project: version.plan.project,
            owner: version.plan.owner,
            narration: NarrationId::new(),
            generation: version.plan.generation.clone(),
            scenes: version.plan.scenes().to_vec(),
            updated_at: time(1_800_000_030),
        };
        record.scenes[1].start = Duration::from_millis(1_500);
        let placed = ScenePlan::restore(record);
        let cut = SavedTimeline {
            project: placed.project,
            owner: placed.owner,
            scene_plan: placed.id,
            narration: placed.narration,
            video: vec![bardo_domain::SavedVideoItem {
                picture: bardo_domain::Picture::Scene(0),
                start: Duration::ZERO,
                duration: Duration::from_millis(1_500),
                framing: Framing::Fit,
                transition: Some(Transition::new(TransitionKind::Crossfade, 12)),
                motion: None,
            }],
            narration_items: Vec::new(),
            music_items: Vec::new(),
            sfx_items: Vec::new(),
            mix: Mix::default(),
            captions: None,
            aspect: AspectRatio::Vertical,
            updated_at: time(1_800_000_030),
        };
        setup.db.place_scenes(&placed, &cut).unwrap();
        assert_eq!(
            setup.db.scene_plan(placed.project).unwrap(),
            Some(placed.clone())
        );
        assert_eq!(
            setup
                .db
                .saved_timeline(placed.project)
                .unwrap()
                .map(|saved| saved.scene_plan),
            Some(placed.id)
        );
    }
}
