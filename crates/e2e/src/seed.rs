//! `seed-showcase`: the data the look and media jobs show and work on,
//! written as the providers' answers would have left it (recorded
//! fixtures), with media made by the bundled ffmpeg. No provider is called.
//!
//! - "Space Archives" (English, United States): YouTube, TikTok, X and
//!   Instagram accounts; four published videos with their numbers; ranked
//!   theme ideas; niche research for four seeds; a project ready to render
//!   and publish ("The probe that never came back"): a 24 s narration
//!   captured from a recording of the user and voiced by the persona's
//!   realistic voice, with word timings and analyzed takes, five scenes with
//!   stills, imported music, a sound effect and portrait footage on the
//!   cut, a music prompt and every network's title and text. X renders
//!   landscape at 720p.
//! - "Vozes do Mar" (Portuguese, Brazil): one project with a script.
//! - A persona, this month's spend against two budgets, and a finished and
//!   a failed job.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, anyhow, bail};
use bardo_app::Bardo;
use bardo_domain::*;
use bardo_storage::Database;
use serde_json::{Value, json};

/// Fixture values are valid by construction; a refusal still names itself.
trait Valid<T> {
    fn valid(self) -> anyhow::Result<T>;
}

impl<T, E: std::fmt::Debug> Valid<T> for Result<T, E> {
    fn valid(self) -> anyhow::Result<T> {
        self.map_err(|error| anyhow!("{error:?}"))
    }
}

const DAY: Duration = Duration::from_secs(24 * 3600);

/// The project the media job renders and exports.
pub const PROJECT_TITLE: &str = "The probe that never came back";

/// The narration's words, read over [`NARRATION_MS`].
const NARRATION: &str = "In 1977 a small probe left Earth on a mission nobody expected to last. \
    It crossed the orbit of Mars within a year. Then the signal grew faint. \
    Engineers listened every night for a voice from the dark. \
    Decades later, one quiet morning, the antennas heard it again.";
const NARRATION_MS: u64 = 24_000;
/// The recording the narration was captured from, as the user named it.
const CAPTURE_NAME: &str = "IMG_4412.MOV";
/// The persona's made-up voice.
const VOICE_ID: &str = "e2eVoiceArchivist01";
const VOICE_NAME: &str = "Archivist (made up)";

pub fn seed_showcase(bardo: &mut Bardo) -> anyhow::Result<Value> {
    if !bardo.channels()?.is_empty() {
        bail!("seed-showcase needs a Bardo with no channels yet");
    }
    let ffmpeg = bardo_media::ffmpeg::Ffmpeg::locate()
        .map_err(|error| anyhow!("the bundled ffmpeg: {error:?}"))?
        .ffmpeg_path()
        .to_path_buf();
    let profile = bardo.profile().id;
    let db = Database::open(&bardo_storage::default_database_path()?)?;
    let projects_dir = bardo_storage::default_projects_dir()?;
    let now = SystemTime::now();

    let templates = Templates::first(&db, profile, now)?;
    let persona = persona(&db, profile)?;
    let space = channel(
        &db,
        profile,
        ChannelDraft {
            name: "Space Archives".into(),
            niche: "space history".into(),
            themes: vec!["lost missions".into()],
            aesthetic_notes: "archival photos, cold blues".into(),
            language: ContentLanguage::English,
            country: Country::UnitedStates,
            default_persona: Some(persona),
            ..ChannelDraft::default()
        },
    )?;
    let sea = channel(
        &db,
        profile,
        ChannelDraft {
            name: "Vozes do Mar".into(),
            niche: "naufrágios".into(),
            themes: vec!["navios perdidos".into()],
            language: ContentLanguage::Portuguese,
            country: Country::Brazil,
            ..ChannelDraft::default()
        },
    )?;
    // X renders landscape at 720p: the media job checks both shapes.
    for (network, handle, aspect, resolution) in [
        (Network::YouTube, "@spacearchives", None, None),
        (Network::TikTok, "@spacearchives", None, None),
        (Network::InstagramReels, "spacearchives", None, None),
        (
            Network::X,
            "@space_archives",
            Some(AspectRatio::Landscape),
            Some(Resolution::Hd720),
        ),
    ] {
        let details = NetworkAccountDetails::validate(
            network,
            NetworkAccountDraft {
                handle: handle.into(),
                aspect,
                resolution,
                ..NetworkAccountDraft::default()
            },
        )
        .map_err(|errors| anyhow!("the {network:?} account: {errors:?}"))?;
        NetworkAccountRepository::save(
            &db,
            &NetworkAccount::new(profile, space.id, network, details),
        )?;
    }

    let niche = Niche::new("space history").valid()?;
    past_posts(&db, profile, &space, now)?;
    ranked_ideas(&db, profile, &space, &niche, now)?;
    research(&db, profile, &space, now)?;
    let ship = start_project(
        &db,
        profile,
        &sea,
        "naufrágios",
        "O navio que sumiu duas vezes",
        now,
    )?;
    script(
        &db,
        &templates,
        &ship,
        "Em 1872 um veleiro foi encontrado à deriva, intacto e sem ninguém a bordo. \
         Dez anos depois, um navio com o mesmo nome desapareceu de novo.",
        now,
    )?;
    let probe = render_ready(&db, &templates, &space, &projects_dir, &ffmpeg, now)?;
    spend(&db, profile, &space, &sea, &probe, &ship, now)?;
    jobs(&db, profile, now)?;

    Ok(json!({
        "profile": profile.to_string(),
        "project": probe.id.to_string(),
        "project_folder": projects_dir.join(probe.id.to_string()),
        "channels": [space.details.name(), sea.details.name()],
    }))
}

/// The templates the recorded generations name, as a first generation
/// saves them.
struct Templates {
    script: TemplateVersion,
    image: TemplateVersion,
    music: TemplateVersion,
    metadata: TemplateVersion,
}

impl Templates {
    fn first(db: &Database, profile: ProfileId, now: SystemTime) -> anyhow::Result<Templates> {
        let first = |kind| -> anyhow::Result<TemplateVersion> {
            let version = TemplateVersion::first(profile, bardo_app::default_template(kind), now);
            db.add_template_version(&version)?;
            Ok(version)
        };
        Ok(Templates {
            script: first(TemplateKind::Script)?,
            image: first(TemplateKind::ImagePrompt)?,
            music: first(TemplateKind::MusicPrompt)?,
            metadata: first(TemplateKind::Metadata)?,
        })
    }
}

fn persona(db: &Database, profile: ProfileId) -> anyhow::Result<PersonaId> {
    let details = PersonaDetails::validate(PersonaDraft {
        name: "Archivist".into(),
        voice: Some(VoiceRef::new(Provider::ElevenLabs, VOICE_ID, VOICE_NAME)?),
        tone: "Calm, curious, a little wry.".into(),
        script_style: "Short sentences, one surprising fact per scene.".into(),
        presets: GenerationPresets::default(),
        realistic_voice: true,
    })
    .map_err(|errors| anyhow!("the persona: {errors:?}"))?;
    let persona = Persona::new(profile, details);
    PersonaRepository::save(db, &persona)?;
    Ok(persona.id)
}

fn channel(db: &Database, profile: ProfileId, draft: ChannelDraft) -> anyhow::Result<Channel> {
    let details =
        ChannelDetails::validate(draft).map_err(|errors| anyhow!("a channel: {errors:?}"))?;
    let channel = Channel::new(profile, details);
    ChannelRepository::save(db, &channel)?;
    Ok(channel)
}

fn start_project(
    db: &Database,
    profile: ProfileId,
    channel: &Channel,
    niche: &str,
    idea: &str,
    at: SystemTime,
) -> anyhow::Result<VideoProject> {
    let mut theme = Theme::suggested(
        profile,
        channel.id,
        Niche::new(niche).valid()?,
        ThemeIdea::new(idea, "").valid()?,
        at,
        0,
        None,
    );
    db.save_themes(std::slice::from_ref(&theme))?;
    let mut project = theme.approve(at)?;
    project.created_at = at;
    db.start_project(&theme, &project)?;
    Ok(project)
}

/// The script Claude would have written for `project`.
fn script(
    db: &Database,
    templates: &Templates,
    project: &VideoProject,
    text: &str,
    now: SystemTime,
) -> anyhow::Result<()> {
    let template = &templates.script;
    let profile = template.owner;
    let generation = Generation {
        id: GenerationId::new(),
        owner: profile,
        project: project.id,
        provider: Provider::Claude,
        model: "claude-fixture".into(),
        template: TemplateUsed {
            id: template.id,
            number: 1,
        },
        instructions: String::new(),
        prompt: "recorded fixture".into(),
        output: text.into(),
        usage: TokenUsage::default(),
        generated_at: now,
        job: None,
    };
    let generated =
        GeneratedScript::new(generation).map_err(|error| anyhow!("the script: {error:?}"))?;
    db.save_script(&Script::first(generated, now))?;
    Ok(())
}

/// Four videos posted on YouTube over the last six weeks, with views
/// taken a few times since.
fn past_posts(
    db: &Database,
    profile: ProfileId,
    channel: &Channel,
    now: SystemTime,
) -> anyhow::Result<()> {
    // Title, days since posted, and views taken (days after posting, views).
    type Post<'a> = (&'a str, u32, &'a [(f64, u64)]);
    let posts: [Post; 4] = [
        (
            "Who really built the first rocket",
            40,
            &[(3.0, 2_100), (9.0, 5_400), (30.0, 8_900)],
        ),
        (
            "The cosmonaut who was erased",
            25,
            &[(7.0, 1_300), (20.0, 2_000)],
        ),
        (
            "The station that fell on Australia",
            15,
            &[(2.5, 4_700), (10.0, 13_800)],
        ),
        ("Voices from Apollo 13", 4, &[(1.0, 900), (3.5, 2_600)]),
    ];
    for (n, (title, days_ago, views)) in posts.into_iter().enumerate() {
        let project = start_project(
            db,
            profile,
            channel,
            "space history",
            title,
            now - DAY * (days_ago + 2),
        )?;
        let posted_at = now - DAY * days_ago;
        let publication = Publication {
            id: PublicationId::new(),
            owner: profile,
            project: project.id,
            account: NetworkAccountId::new(),
            network: Network::YouTube,
            render: RenderId::new(),
            link: Some(
                PostLink::parse(
                    Network::YouTube,
                    &format!("https://www.youtube.com/watch?v=e2eArchive{n}"),
                )
                .valid()?,
            ),
            kind: PublicationKind::Manual,
            insights_id: None,
            posted_at,
            linked_at: posted_at,
            checked_at: Some(now),
            missing_since: None,
        };
        db.save_publication(&publication)?;
        let snapshots: Vec<_> = views
            .iter()
            .map(|&(age, views)| MetricsSnapshot {
                publication: publication.id,
                taken_at: posted_at + Duration::from_secs_f64(age * DAY.as_secs_f64()),
                views,
                likes: None,
                comments: None,
                owner: None,
                insights: Insights::default(),
            })
            .collect();
        db.save_sync(&[], &snapshots)?;
    }
    Ok(())
}

/// Six ideas the decision engine ranked, five with the channel's own
/// numbers as evidence.
fn ranked_ideas(
    db: &Database,
    profile: ProfileId,
    channel: &Channel,
    niche: &Niche,
    now: SystemTime,
) -> anyhow::Result<()> {
    let past = PastPerformance::of_channel(
        &db.projects(channel.id)?,
        &db.channel_publications(channel.id)?,
        &db.channel_snapshots(channel.id)?,
    );
    let evidence = past.evidence(niche);
    let reason = |score: u8, confidence: f64| Reason {
        score: Score::new(score),
        confidence: Confidence::new(confidence),
    };
    let ideas: [(&str, &str, [u8; 4]); 6] = [
        (
            "The satellite that fell on Canada",
            "How a reactor scattered over the tundra in 1978, and the cleanup nobody remembers.",
            [80, 60, 35, 85],
        ),
        (
            "The last message from Mars 3",
            "Twenty seconds of signal, then silence: what the first lander on Mars sent home.",
            [85, 55, 40, 70],
        ),
        (
            "Why Skylab came down early",
            "The sun, not the engineers, decided when the first station would fall.",
            [70, 70, 55, 45],
        ),
        (
            "The thirteen who never flew",
            "Thirteen pilots passed the astronaut tests in 1961, and none of them flew.",
            [75, 65, 60, 50],
        ),
        (
            "A capsule and no way back",
            "An early flight, told by the engineers who knew it would not return.",
            [65, 50, 75, 25],
        ),
        ("The shuttle that flew once", "", [60, 45, 30, 0]),
    ];
    let mut themes = Vec::new();
    for (position, (title, angle, scores)) in ideas.into_iter().enumerate() {
        let mut theme = Theme::suggested(
            profile,
            channel.id,
            niche.clone(),
            ThemeIdea::new(title, angle).valid()?,
            now - Duration::from_secs(600),
            position as u32,
            None,
        );
        // The last one was ranked before the channel had any history.
        let performance = evidence
            .filter(|_| position < 5)
            .map(|evidence| PerformanceReason {
                reason: reason(scores[3], 0.62 + position as f64 * 0.03),
                evidence,
            });
        theme.rank(ThemeRanking {
            fit: reason(scores[0], 0.84),
            trend: reason(scores[1], 0.71),
            competition: reason(scores[2], 0.77),
            performance,
            model: "jev-e2e-fixture".into(),
            ranked_at: now - Duration::from_secs(540),
        })?;
        themes.push(theme);
    }
    db.save_themes(&themes)?;
    Ok(())
}

/// Market numbers for four seed niches, as the YouTube Data API returned
/// them.
fn research(
    db: &Database,
    profile: ProfileId,
    channel: &Channel,
    now: SystemTime,
) -> anyhow::Result<()> {
    let seeds = [
        "space history",
        "lost missions",
        "cold war science",
        "rocket failures",
    ];
    let parsed = NicheSeeds::parse(&seeds).valid()?;
    NicheResearchRepository::set_seeds(db, channel.id, &parsed)?;
    let samples: [(u64, u64, u64); 4] = [
        (4_200, 38_000, 12_000),
        (1_100, 61_000, 4_500),
        (2_600, 22_000, 48_000),
        (7_800, 15_000, 160_000),
    ];
    for (niche, (volume, views, subs)) in parsed.niches().iter().zip(samples) {
        let uploads = (0..12u64)
            .map(|i| UploadSample {
                channel_id: format!("UCe2e{i:03}"),
                published_at: now - DAY * (2 + i as u32 * 2),
                views: views / 2 + i * views / 12,
                channel_subscribers: Some(subs / 3 + i * subs / 8),
            })
            .collect();
        NicheResearchRepository::save(
            db,
            &NicheResearch {
                owner: profile,
                niche: niche.clone(),
                market: channel.details.market(),
                fetched_at: now - Duration::from_secs(3_600),
                sample: MarketSample {
                    upload_volume: volume,
                    uploads,
                },
            },
        )?;
    }
    Ok(())
}

/// Runs the bundled ffmpeg to make a fixture file.
pub(crate) fn make(ffmpeg: &Path, args: &[&str], out: &Path) -> anyhow::Result<()> {
    let output = Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .arg(out)
        .output()
        .with_context(|| format!("running {}", ffmpeg.display()))?;
    if !output.status.success() {
        bail!(
            "ffmpeg could not make {}: {}",
            out.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// The recording of the user the narration was captured from, as the
/// capture job would have left it: its copy in the project folder (a
/// small stand-in, picture and sound) and the takes cut from the
/// narration's words.
#[allow(clippy::too_many_arguments)]
fn capture(
    db: &Database,
    ffmpeg: &Path,
    dir: &Path,
    profile: ProfileId,
    project: &VideoProject,
    narration: &Narration,
    voice: CaptureVoice,
    now: SystemTime,
) -> anyhow::Result<()> {
    let id = CaptureId::new();
    let video_file = format!("capture-{id}.mov");
    let seconds = (NARRATION_MS / 1000).to_string();
    make(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=0x2a3a4a:s=720x1280:d={seconds}:r=30"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=f=220:d={seconds}"),
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "mpeg4",
            "-q:v",
            "31",
            "-g",
            "300",
            "-c:a",
            "aac",
            "-shortest",
        ],
        &dir.join(&video_file),
    )?;
    let capture = Capture {
        id,
        project: project.id,
        owner: profile,
        file_name: CAPTURE_NAME.into(),
        video_file,
        video: CaptureVideo {
            width: 720,
            height: 1280,
            fps_milli: 30_000,
            duration: Duration::from_millis(NARRATION_MS),
        },
        confirmation: CaptureConfirmation {
            text: "This recording shows only me.".into(),
            at: now,
        },
        voice,
        narration: narration.id,
        coverage: 1000,
        takes: Takes::split(&bardo_app::take_source(narration)),
        imported_at: now,
        job: None,
    };
    db.save_capture(&capture)?;
    // Each take's face mesh, as the analysis on the graphics card would
    // have left it; the second take turns away for a moment.
    for (index, take) in capture.takes.as_slice().iter().enumerate() {
        let frames = ((take.end - take.start).as_secs_f64() * 30.0).round() as u32;
        db.save_analysis(&TakeAnalysis {
            project: project.id,
            capture: capture.id,
            position: index as u32,
            revision: take.revision,
            part: AnalysisPart::Face,
            device: Device::Gpu,
            model: "MediaPipe Face Landmarker float16 v1".into(),
            checks: TakeChecks {
                frames,
                turned: if index == 1 { 12 } else { 0 },
                ..TakeChecks::default()
            },
            depth_range: None,
            finished_at: now,
            job: None,
        })?;
    }
    Ok(())
}

/// The project ready to render: what the script, narration, scene and
/// image steps would have saved, and imported media on the cut.
fn render_ready(
    db: &Database,
    templates: &Templates,
    channel: &Channel,
    projects_dir: &Path,
    ffmpeg: &Path,
    now: SystemTime,
) -> anyhow::Result<VideoProject> {
    let profile = templates.script.owner;
    let project = start_project(db, profile, channel, "space history", PROJECT_TITLE, now)?;
    let template = &templates.image;
    let generation = |provider: Provider, model: &str, template: &TemplateVersion| Generation {
        id: GenerationId::new(),
        owner: profile,
        project: project.id,
        provider,
        model: model.into(),
        template: TemplateUsed {
            id: template.id,
            number: 1,
        },
        instructions: String::new(),
        prompt: "recorded fixture".into(),
        output: String::new(),
        usage: TokenUsage::default(),
        generated_at: now,
        job: None,
    };

    script(db, templates, &project, NARRATION, now)?;

    let dir = projects_dir.join(project.id.to_string());
    std::fs::create_dir_all(&dir)?;
    // A voice-like level: pink noise swelling five times a second.
    make(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            &format!(
                "anoisesrc=d={}:c=pink:a=0.4,volume='0.2+0.8*abs(sin(t*5))':eval=frame",
                NARRATION_MS / 1000
            ),
            "-ac",
            "2",
            "-ar",
            "48000",
        ],
        &dir.join("narration-1.mp3"),
    )?;
    let chars: Vec<char> = NARRATION.chars().collect();
    let step = NARRATION_MS / chars.len() as u64;
    let alignment = Alignment {
        chars: chars
            .iter()
            .enumerate()
            .map(|(n, c)| CharTiming {
                text: c.to_string(),
                start: Duration::from_millis(n as u64 * step),
                end: Duration::from_millis((n as u64 + 1) * step),
            })
            .collect(),
    };
    // The user's capture, voiced by the persona's realistic voice: posts
    // need the synthetic-content disclosure.
    let voice = CaptureVoice::Persona {
        voice: VoiceRef::new(Provider::ElevenLabs, VOICE_ID, VOICE_NAME)?,
        presets: GenerationPresets::default(),
    };
    let narration = Narration {
        id: NarrationId::new(),
        project: project.id,
        owner: profile,
        text: ScriptText::new(NARRATION).valid()?,
        source: NarrationSource::Captured {
            file_name: CAPTURE_NAME.into(),
            voice: voice.clone(),
            model: "eleven_multilingual_sts_v2".into(),
            aligner: Provider::ElevenLabs,
        },
        audio_file: "narration-1.mp3".into(),
        duration: Duration::from_millis(NARRATION_MS),
        words: WordTimings::from_alignment(NARRATION, &alignment),
        generated_at: now,
        job: None,
    };
    db.save_narration(&narration)?;
    capture(db, ffmpeg, &dir, profile, &project, &narration, voice, now)?;

    let prompts = [
        "A small probe on a launch pad, archival photo, 1977",
        "The probe crossing the orbit of Mars, red planet glowing",
        "A fading radio signal on an old oscilloscope",
        "Engineers in a dark control room at night",
        "Giant radio antennas at dawn under a pale sky",
    ];
    let drafts = (0..prompts.len())
        .map(|n| SceneDraft {
            first_sentence: n,
            prompt: prompts[n].into(),
        })
        .collect();
    let mut plan = ScenePlan::from_drafts(
        generation(Provider::Claude, "claude-fixture", template),
        &narration,
        drafts,
        now,
    )?;
    let colors = ["0x1d3557", "0x7a2e1f", "0x1f4d3a", "0x3a2a5c", "0x5c4a1f"];
    for (index, color) in colors.iter().enumerate().take(plan.scenes().len()) {
        let file = format!("scene-{}.png", index + 1);
        make(
            ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                &format!(
                    "gradients=s=1280x720:c0={color}:c1=0x101418:x0=0:y0=0:x1=1280:y1=720:d=1"
                ),
                "-frames:v",
                "1",
            ],
            &dir.join(&file),
        )?;
        let scene = plan.scene_mut(index, now)?;
        scene.add_image(SceneImage {
            file,
            generation: generation(Provider::Gemini, "image-fixture", template),
        });
        let _ = scene.accept_image();
    }
    db.save_scene_plan(&plan)?;

    let import = |name: &str, args: &[&str], kind, secs, picture| -> anyhow::Result<MediaAsset> {
        let id = MediaAssetId::new();
        let file = MediaAsset::file_name(id, name);
        make(ffmpeg, args, &dir.join(&file))?;
        let asset = MediaAsset {
            id,
            project: project.id,
            owner: profile,
            kind,
            source: AssetSource::Imported,
            file,
            name: name.into(),
            duration: Duration::from_secs(secs),
            picture,
            imported_at: now,
        };
        db.save_media_asset(&asset)?;
        Ok(asset)
    };
    let music = import(
        "Night Drive (bed).mp3",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=f=220:d=20,volume='3+2*sin(t*2)':eval=frame",
            "-ac",
            "2",
            "-ar",
            "48000",
        ],
        MediaKind::Audio,
        20,
        None,
    )?;
    let hit = import(
        "whoosh.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "anoisesrc=d=2:c=white:a=0.5,afade=t=out:st=0.3:d=1.7",
            "-ac",
            "2",
            "-ar",
            "48000",
        ],
        MediaKind::Audio,
        2,
        None,
    )?;
    let footage = import(
        "drone over antennas.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=1080x1920:d=4:r=30",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "mpeg4",
            "-q:v",
            "4",
        ],
        MediaKind::Video,
        4,
        Some(PictureSize::new(1080, 1920)),
    )?;
    // Its stills move as a new channel's do, so the render draws them.
    let mut timeline = Timeline::rough_cut(&plan, &narration, AspectRatio::Landscape)
        .with_motion(MotionDefault::default());
    for (asset, track, at) in [
        (&music, Track::Music, Duration::from_secs(2)),
        (&hit, Track::Sfx, Duration::from_millis(9_600)),
        (&footage, Track::Video, Duration::from_secs(10)),
    ] {
        let (edit, _) = timeline
            .place(asset, track, at)
            .map_err(|error| anyhow!("placing {}: {error:?}", asset.name))?;
        timeline
            .apply(&edit)
            .map_err(|error| anyhow!("placing {}: {error:?}", asset.name))?;
    }
    db.save_timeline(&timeline.to_saved(project.id, profile, plan.id, narration.id, now))?;

    let mut music_prompt = generation(Provider::Claude, "claude-fixture", &templates.music);
    music_prompt.output = "Slow cinematic ambient bed at 70 BPM in D minor: warm analog pads, \
        a soft sub pulse and distant glassy plucks. Instrumental, no vocals, gentle dynamics \
        that leave room for a narrator."
        .into();
    db.save_music_prompt(&MusicPrompt::generated(music_prompt, now).valid()?)?;

    // Every network's title, text and tags, with accents for the export's
    // UTF-8.
    let written = generation(Provider::Claude, "claude-fixture", &templates.metadata);
    let metadata: Vec<_> = Network::ALL
        .into_iter()
        .map(|network| {
            VideoMetadata::generated(
                network,
                VideoMetadataDraft {
                    title: PROJECT_TITLE.into(),
                    description: "In 1977 a probe left Earth and went silent. Decades later, \
                        the antennas heard it again. Narração e música: Bardo e2e."
                        .into(),
                    tags: vec!["space".into(), "history".into(), "missões".into()],
                },
                written.clone(),
                now,
            )
        })
        .collect();
    db.save_video_metadata(&metadata)?;
    Ok(project)
}

/// This month's spend: two budgets, one near and one reached, and a call
/// without a price.
fn spend(
    db: &Database,
    profile: ProfileId,
    space: &Channel,
    sea: &Channel,
    probe: &VideoProject,
    ship: &VideoProject,
    now: SystemTime,
) -> anyhow::Result<()> {
    use CostPurpose::*;
    use Provider::*;
    let record =
        |provider, model: &str, purpose, cents, channel: &Channel, project: &VideoProject| {
            CostRecord {
                id: CostRecordId::new(),
                owner: profile,
                provider,
                model: model.into(),
                purpose,
                usage: Metered {
                    input_tokens: 812,
                    output_tokens: 2_431,
                    ..Metered::default()
                },
                cost: if cents == 0 {
                    Cost::Unpriced
                } else {
                    Cost::Estimated(Money::from_cents(cents))
                },
                channel: Some(channel.id),
                project: Some(project.id),
                job: None,
                at: now - Duration::from_secs(3600),
            }
        };
    for cost in [
        record(Claude, "claude-fixture", Script, 312, space, probe),
        record(Claude, "claude-fixture", ScenePlan, 188, space, probe),
        record(Gemini, "image-fixture", SceneImage, 1_240, space, probe),
        record(Gemini, "video-fixture", SceneClip, 1_620, space, probe),
        record(ElevenLabs, "voice-fixture", Narration, 420, sea, ship),
        record(Claude, "claude-fixture", Script, 290, sea, ship),
        record(Higgsfield, "clip-fixture", SceneClip, 0, space, probe),
    ] {
        db.record_cost(&cost)?;
    }
    for (provider, cents) in [(Gemini, 3_200), (Claude, 1_000)] {
        db.save_budget(
            profile,
            &Budget {
                provider,
                monthly: Money::from_cents(cents),
            },
        )?;
    }
    Ok(())
}

/// A finished job and one that failed for good, for the Jobs panel.
fn jobs(db: &Database, profile: ProfileId, now: SystemTime) -> anyhow::Result<()> {
    let mut done = Job::new(profile, JobKind::MetricsSync, "");
    done.start(now)?;
    done.complete()?;
    JobRepository::save(db, &done)?;
    let mut failed = Job::new(profile, JobKind::NicheResearch, "");
    failed.start(now)?;
    failed.fail_attempt(
        JobFailure::new(JobFailureKind::MissingKey, "no YouTube Data API key saved"),
        now,
        &RetryPolicy::default(),
    )?;
    JobRepository::save(db, &failed)?;
    Ok(())
}
