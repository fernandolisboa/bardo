//! The media job's commands (issue #166): what a person does in the
//! editor, the Render stage and the Publish stage, on the project
//! `seed-showcase` made, with the bundled ffmpeg and no provider. Each
//! prints what Bardo shows and what is on disk; the job's script checks it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow, bail};
use bardo_app::{Bardo, ClipMedia, EditAction};
use bardo_domain::{AudioLane, JobId, LaneMix, Network, VideoProjectId};
use serde_json::{Value, json};

use crate::seed::PROJECT_TITLE;
use crate::{JOB_PATIENCE, wait_for};

/// The seeded project.
fn project(bardo: &Bardo) -> anyhow::Result<VideoProjectId> {
    for channel in bardo.channels()? {
        let projects = bardo
            .video_projects(channel.id)
            .map_err(|error| anyhow!("listing projects: {error:?}"))?;
        if let Some(project) = projects.iter().find(|p| p.title == PROJECT_TITLE) {
            return Ok(project.id);
        }
    }
    bail!("no project \"{PROJECT_TITLE}\": run seed-showcase first")
}

/// `youtube,x` or `all`.
fn networks(list: &str) -> anyhow::Result<Vec<Network>> {
    if list == "all" {
        return Ok(Network::ALL.to_vec());
    }
    list.split(',')
        .map(|code| {
            Network::ALL
                .into_iter()
                .find(|network| network.code() == code)
                .with_context(|| format!("no network {code}"))
        })
        .collect()
}

/// The files in the project folder, by name.
fn folder_files(bardo: &Bardo, project: VideoProjectId) -> anyhow::Result<Vec<String>> {
    let folder = bardo_storage::default_projects_dir()?.join(project.to_string());
    let mut names: Vec<String> = std::fs::read_dir(&folder)?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let _ = bardo;
    Ok(names)
}

/// Opens the project in the editor and waits for its proxies (S4.02).
pub fn proxies(bardo: &Bardo) -> anyhow::Result<Value> {
    let project = project(bardo)?;
    let started = Instant::now();
    let mut editor = bardo
        .open_editor(project)
        .map_err(|error| anyhow!("opening the editor: {error:?}"))?;
    while editor.view().is_building() {
        if started.elapsed() > JOB_PATIENCE {
            bail!("the proxies did not finish within {JOB_PATIENCE:?}");
        }
        std::thread::sleep(Duration::from_millis(250));
        bardo
            .refresh_editor(&mut editor)
            .map_err(|error| anyhow!("{error:?}"))?;
    }
    let view = editor.view();
    Ok(json!({
        "ready": view.proxies_ready,
        "total": view.proxies_total,
        "job": view.job.as_ref().map(|job| json!({
            "state": job.state().code(),
            "failure": job.failure().map(|failure| format!("{failure:?}")),
        })),
        "problems": view.problems().iter().map(|problem| format!("{problem:?}")).collect::<Vec<_>>(),
        "seconds": started.elapsed().as_secs_f64(),
        "files": folder_files(bardo, project)?,
    }))
}

/// The cut's clips in order, by the file each plays.
fn clip_order(bardo: &Bardo, project: VideoProjectId) -> anyhow::Result<Vec<String>> {
    let view = bardo
        .editor_view(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    Ok(view
        .clips
        .iter()
        .map(|clip| clip.file.clone().unwrap_or_default())
        .collect())
}

/// Whether the sound effects lane is muted, as the mixer shows it.
fn sfx_muted(bardo: &Bardo, project: VideoProjectId) -> anyhow::Result<bool> {
    let view = bardo
        .editor_view(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    let timeline = view.timeline.context("no cut")?;
    Ok(timeline.mix().lane(AudioLane::Sfx).muted)
}

/// Swaps the first two clips of the cut, as dragging one does, and mutes
/// the sound effects lane in the mixer (S8.06, S8.11).
pub fn edit_cut(bardo: &Bardo) -> anyhow::Result<Value> {
    let project = project(bardo)?;
    let before = clip_order(bardo, project)?;
    let muted_before = sfx_muted(bardo, project)?;
    let mut editor = bardo
        .open_editor(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    bardo
        .edit(&mut editor, EditAction::Reorder { from: 0, to: 1 })
        .map_err(|error| anyhow!("reordering: {error:?}"))?;
    let lane = editor
        .view()
        .timeline
        .as_ref()
        .context("no cut")?
        .mix()
        .lane(AudioLane::Sfx);
    bardo
        .edit(
            &mut editor,
            EditAction::SetLane {
                lane: AudioLane::Sfx,
                mix: LaneMix {
                    muted: true,
                    ..lane
                },
            },
        )
        .map_err(|error| anyhow!("muting: {error:?}"))?;
    Ok(json!({
        "before": before,
        "after": clip_order(bardo, project)?,
        "sfx_muted_before": muted_before,
        "sfx_muted": sfx_muted(bardo, project)?,
        "can_undo": editor.can_undo(),
    }))
}

/// The cut as a new start of Bardo finds it, how many clips lost their
/// file, and whether the renders still match it (S8.06, S8.11, S8.36,
/// S9.06).
pub fn cut_state(bardo: &Bardo) -> anyhow::Result<Value> {
    let project = project(bardo)?;
    let editor = bardo
        .open_editor(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    let review = bardo
        .render_review(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    let missing = editor
        .view()
        .clips
        .iter()
        .filter(|clip| clip.media == ClipMedia::Missing)
        .count();
    Ok(json!({
        "clips": clip_order(bardo, project)?,
        "sfx_muted": sfx_muted(bardo, project)?,
        "media_missing": missing,
        "can_undo": editor.can_undo(),
        "targets": review.targets.iter().map(|target| json!({
            "network": target.network.code(),
            "rendered": target.last.is_some(),
            "current": target.last_current,
        })).collect::<Vec<_>>(),
    }))
}

/// The Render stage: checks the cut, then renders the chosen networks
/// (S4.08–S4.10). With `cancel_second`, cancels as the second file starts,
/// then resumes, as Retry does (S9.03).
pub fn render(bardo: &Bardo, list: &str, cancel_second: bool) -> anyhow::Result<Value> {
    let project = project(bardo)?;
    let chosen = networks(list)?;
    let review = bardo
        .render_review(project)
        .map_err(|error| anyhow!("the render review: {error:?}"))?;
    let found = bardo
        .render_checks(&review)
        .run()
        .map_err(|error| anyhow!("checking the cut: {error:?}"))?;
    let review = review.checked(&found);
    let targets: Vec<_> = review
        .targets
        .iter()
        .filter(|target| chosen.contains(&target.network))
        .collect();
    let accounts: Vec<_> = targets.iter().map(|target| target.account).collect();
    let reviewed: Vec<Value> = targets
        .iter()
        .map(|target| {
            json!({
                "network": target.network.code(),
                "size": format!("{}x{}", target.size().0, target.size().1),
                "encoder": target.encoder.map(|encoder| bardo.encoder_text(encoder)),
                "can_render": target.can_render(),
            })
        })
        .collect();
    let mix = review
        .cut
        .mix
        .map(|mix| json!({ "integrated": mix.integrated, "true_peak": mix.true_peak }));
    // While a render runs, an export is refused (S9.13).
    let started = std::time::SystemTime::now();
    let job = bardo
        .start_render(&review, &accounts)
        .map_err(|error| anyhow!("starting the render: {error:?}"))?;
    let view = bardo
        .export_view(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    let every: Vec<_> = view.targets.iter().map(|target| target.network).collect();
    let export_while_rendering = match bardo.start_export(&view, &every) {
        Ok(_) => "started".to_owned(),
        Err(error) => format!("{error:?}"),
    };
    let mut cancelled = Value::Null;
    if cancel_second {
        cancelled = cancel_during_second(bardo, project, job, started)?;
    }
    let finished = wait_for(bardo, job)?;
    Ok(json!({
        "review": reviewed,
        "mix": mix,
        "export_while_rendering": export_while_rendering,
        "cancelled": cancelled,
        "job": finished,
        "renders": renders(bardo, project)?,
        "files": folder_files(bardo, project)?,
    }))
}

/// Waits for the first file of the render started at `started`, cancels the job, notes what is on disk, then
/// resumes it.
fn cancel_during_second(
    bardo: &Bardo,
    project: VideoProjectId,
    job: JobId,
    started: std::time::SystemTime,
) -> anyhow::Result<Value> {
    let deadline = Instant::now() + JOB_PATIENCE;
    while !bardo
        .renders(project)
        .map_err(|error| anyhow!("{error:?}"))?
        .iter()
        .any(|render| render.rendered_at >= started)
    {
        if Instant::now() > deadline {
            bail!("the first file was not rendered within {JOB_PATIENCE:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Into the second file.
    std::thread::sleep(Duration::from_millis(1500));
    bardo
        .cancel_job(job)
        .map_err(|error| anyhow!("cancelling: {error:?}"))?;
    let stopped = wait_for(bardo, job)?;
    let at_cancel = renders(bardo, project)?;
    let files = folder_files(bardo, project)?;
    bardo
        .retry_job(job)
        .map_err(|error| anyhow!("resuming: {error:?}"))?;
    Ok(json!({ "job": stopped, "renders": at_cancel, "files": files }))
}

/// The project's rendered files as Bardo recorded them and as ffprobe
/// reads them.
fn renders(bardo: &Bardo, project: VideoProjectId) -> anyhow::Result<Vec<Value>> {
    let ffmpeg = bardo_media::ffmpeg::Ffmpeg::locate().map_err(|error| anyhow!("{error:?}"))?;
    let renders = bardo
        .renders(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    renders
        .iter()
        .map(|render| {
            let path = bardo.render_path(render);
            let probed = ffmpeg.probe(&path).map_err(|error| anyhow!("{error:?}"));
            Ok(json!({
                "network": render.network.code(),
                "file": path,
                "encoder": render.encoder,
                "seconds": render.duration.as_secs_f64(),
                "bytes": render.size_bytes,
                "rendered_at": render.rendered_at.duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64,
                "loudness": render.loudness.map(|l| l.integrated),
                "target": render.preset.loudness.lufs(),
                "on_target": render.loudness_on_target(),
                "probe": match probed {
                    Ok(info) => json!({
                        "seconds": info.duration.as_secs_f64(),
                        "video": info.video.map(|video| json!({
                            "codec": video.codec,
                            "width": video.width,
                            "height": video.height,
                        })),
                        "audio": info.audio.is_some(),
                    }),
                    Err(error) => json!({ "error": error.to_string() }),
                },
            }))
        })
        .collect()
}

/// After a start that found a render job left running: waits for every
/// job, then lists the renders (S9.04).
pub fn wait_jobs(bardo: &Bardo) -> anyhow::Result<Value> {
    let project = project(bardo)?;
    let deadline = Instant::now() + JOB_PATIENCE;
    while bardo.jobs().iter().any(|job| job.state().is_active()) {
        if Instant::now() > deadline {
            bail!("jobs still running after {JOB_PATIENCE:?}");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let job = bardo.render_job(project).map(|job| {
        json!({
            "state": job.state().code(),
            "failure": job.failure().map(|failure| format!("{failure:?}")),
        })
    });
    Ok(json!({
        "job": job,
        "renders": renders(bardo, project)?,
        "files": folder_files(bardo, project)?,
    }))
}

/// Files to import, made with the bundled ffmpeg in `folder`: what the
/// editor takes (mp3, wav, m4a, a landscape mp4, a portrait mov) and what
/// it refuses (a text file named .mp3, a cut-off mp4, a still image).
pub fn media_fixtures(folder: &Path) -> anyhow::Result<Value> {
    let ffmpeg = bardo_media::ffmpeg::Ffmpeg::locate()
        .map_err(|error| anyhow!("the bundled ffmpeg: {error:?}"))?
        .ffmpeg_path()
        .to_path_buf();
    std::fs::create_dir_all(folder)?;
    let tone = [
        "-f",
        "lavfi",
        "-i",
        "sine=f=440:d=3",
        "-ac",
        "2",
        "-ar",
        "48000",
    ];
    let picture = |size: &str| {
        vec![
            "-f".to_owned(),
            "lavfi".to_owned(),
            "-i".to_owned(),
            format!("testsrc2=s={size}:d=3:r=30"),
            "-f".to_owned(),
            "lavfi".to_owned(),
            "-i".to_owned(),
            "sine=f=330:d=3".to_owned(),
            "-shortest".to_owned(),
            "-pix_fmt".to_owned(),
            "yuv420p".to_owned(),
            "-c:v".to_owned(),
            "mpeg4".to_owned(),
            "-c:a".to_owned(),
            "aac".to_owned(),
        ]
    };
    let made = [
        (
            "tom de teste.mp3",
            tone.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        ),
        (
            "tom de teste.wav",
            tone.iter().map(|s| s.to_string()).collect(),
        ),
        (
            "tom de teste.m4a",
            tone.iter()
                .map(|s| s.to_string())
                .chain(["-c:a".into(), "aac".into()])
                .collect(),
        ),
        ("paisagem ação.mp4", picture("1280x720")),
        ("retrato.mov", picture("720x1280")),
        (
            "imagem.jpg",
            [
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=640x360:d=1",
                "-frames:v",
                "1",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        ),
    ];
    for (name, args) in &made {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        crate::seed::make(&ffmpeg, &args, &folder.join(name))?;
    }
    std::fs::write(
        folder.join("notes.mp3"),
        "Not audio: notes renamed to .mp3.\n",
    )?;
    let whole = std::fs::read(folder.join("paisagem ação.mp4"))?;
    std::fs::write(folder.join("cortado.mp4"), &whole[..whole.len() / 3])?;
    Ok(json!({
        "takes": ["tom de teste.mp3", "tom de teste.wav", "tom de teste.m4a", "paisagem ação.mp4", "retrato.mov"],
        "refuses": ["notes.mp3", "cortado.mp4", "imagem.jpg"],
    }))
}

/// Imports each file into the project, as the editor's Import media does
/// (S8.13–S8.15).
pub fn import_media(bardo: &Bardo, files: &[String]) -> anyhow::Result<Value> {
    let project = project(bardo)?;
    let import = bardo
        .media_import(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    let before = folder_files(bardo, project)?;
    let results: Vec<Value> = files
        .iter()
        .map(|file| match import.run(Path::new(file)) {
            Ok(asset) => json!({
                "file": file,
                "imported": true,
                "kind": format!("{:?}", asset.kind),
                "picture": asset.picture.map(|p| format!("{}x{}", p.width, p.height)),
                "seconds": asset.duration.as_secs_f64(),
                "copy": asset.file,
            }),
            Err(error) => json!({
                "file": file,
                "imported": false,
                "message": bardo.text(error.message()).into_owned(),
            }),
        })
        .collect();
    Ok(json!({
        "results": results,
        "before": before,
        "after": folder_files(bardo, project)?,
    }))
}

/// The Publish stage's Export for the chosen networks (S9.08–S9.12). With
/// `title`, the posts get that title first, as an edit does.
pub fn export(bardo: &Bardo, list: &str, title: Option<&str>) -> anyhow::Result<Value> {
    let project = project(bardo)?;
    let chosen = networks(list)?;
    if let Some(title) = title {
        for network in &chosen {
            let view = bardo
                .export_view(project)
                .map_err(|error| anyhow!("{error:?}"))?;
            let Some(metadata) = view
                .targets
                .iter()
                .find(|target| target.network == *network)
                .and_then(|target| target.metadata.clone())
            else {
                continue;
            };
            bardo
                .edit_metadata(
                    project,
                    *network,
                    bardo_domain::VideoMetadataDraft {
                        title: title.to_owned(),
                        description: metadata.description().to_owned(),
                        tags: metadata.tags().to_vec(),
                    },
                )
                .map_err(|error| anyhow!("editing the title: {error:?}"))?;
        }
    }
    let view = bardo
        .export_view(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    let blocked: Vec<Value> = view
        .targets
        .iter()
        .filter(|target| chosen.contains(&target.network) && !target.can_export())
        .map(|target| json!({ "network": target.network.code(), "block": format!("{:?}", target.block()) }))
        .collect();
    let job = bardo
        .start_export(&view, &chosen)
        .map_err(|error| anyhow!("starting the export: {error:?} (blocked: {blocked:?})"))?;
    let finished = wait_for(bardo, job)?;
    let view = bardo
        .export_view(project)
        .map_err(|error| anyhow!("{error:?}"))?;
    let exports: Vec<Value> = view
        .targets
        .iter()
        .filter(|target| chosen.contains(&target.network))
        .filter_map(|target| target.last.as_ref())
        .map(|export| {
            let folder = bardo.export_folder(export);
            json!({
                "network": export.network.code(),
                "folder": folder,
                "video_file": export.video_file,
                "files": list_files(&folder),
                // The line metadata.txt carries when the voice needs it.
                "disclosure": view.disclosure.then(|| bardo.disclosure_text(export.network)),
            })
        })
        .collect();
    Ok(json!({ "job": finished, "exports": exports }))
}

fn list_files(folder: &PathBuf) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}
