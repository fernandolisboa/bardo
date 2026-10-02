//! The editor (PRD stories 56, 57, 68, 69): opening a video project shows
//! its rough cut, the timeline the scene plan and narration make
//! (`Timeline::rough_cut`), with a preview that plays it from proxies.
//!
//! The timeline is derived, not stored: it changes when the scenes or the
//! narration do, until cut editing (#21) gives the user's own edits a
//! place. Opening or refreshing the editor queues a job for the proxies the
//! timeline lacks; editing never waits on it, clips without a proxy show
//! that they are building, and the preview waits until none is.
//!
//! The preview runs one ffmpeg child for pictures and one for sound, from
//! the playhead on; pausing or seeking stops them and seeking while playing
//! starts new ones (about a tenth of a second, ADR-0007). While it plays,
//! the sound sets the time: a picture is shown when the audio reaches it,
//! so the two never drift apart. Without a sound device the wall clock
//! does.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bardo_domain::{
    FPS, Generation, Job, JobKind, JobState, NarrationRepository, ProjectFiles, RepositoryError,
    Scene, ScenePlanRepository, ThemeRepository, Timeline, VideoProject, VideoProjectId,
    VideoSource, frame_at, frame_time,
};
use bardo_media::ffmpeg::{
    AudioClip, AudioTrack, ClipSource, FrameSize, FrameStream, Framing, MediaError, RenderPlan,
    VideoClip, VideoFrame,
};
use bardo_media::{AudioOutput, MediaEngine, StreamPlayback};
use serde::Deserialize;

use crate::proxies::{Peaks, ProxiesCheckpoint, ProxiesPayload, ProxyKind, ProxyOrder, proxy_name};
use crate::scenes::to_json;
use crate::{Bardo, Text};

/// Preview pictures at the proxies' 540 lines.
pub const PREVIEW_LANDSCAPE: (u32, u32) = (960, 540);
/// A 9:16 window of the 16:9 picture, 540 lines high (even width).
pub const PREVIEW_PORTRAIT: (u32, u32) = (304, 540);

/// How long the preview waits for its sound before playing without it.
const AUDIO_PATIENCE: Duration = Duration::from_secs(1);

#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    #[error("video project not found")]
    ProjectNotFound,
    /// Proxies are still building, or the timeline is empty.
    #[error("the preview is not ready")]
    PreviewNotReady,
    /// Every proxy of the timeline is there or being built.
    #[error("no proxy to build")]
    NothingToRetry,
    #[error("the preview could not start: {0}")]
    Preview(#[from] MediaError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl EditorError {
    /// What the editor says.
    pub fn message(&self) -> Text {
        match self {
            EditorError::ProjectNotFound => Text::ProjectNotFound,
            EditorError::PreviewNotReady => Text::EditorPreviewBuilding,
            EditorError::NothingToRetry => Text::EditorNothingToRetry,
            EditorError::Preview(MediaError::NotFound { .. }) => Text::EditorFfmpegMissing,
            EditorError::Preview(_) => Text::EditorPreviewFailed,
            EditorError::Repository(_) => Text::EditorNotLoaded,
        }
    }
}

/// Where a clip's media stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipMedia {
    /// Its proxy is ready to preview.
    Ready,
    /// Its proxy is waiting or being built.
    Building,
    /// The scene has no image, or its file is gone from the project folder.
    Missing,
    /// Building its proxy failed, with ffmpeg's reason.
    ProxyFailed(String),
    /// Building its proxy was cancelled.
    ProxyCancelled,
}

impl ClipMedia {
    pub fn needs_attention(&self) -> bool {
        matches!(
            self,
            ClipMedia::Missing | ClipMedia::ProxyFailed(_) | ClipMedia::ProxyCancelled
        )
    }
}

/// One clip of the video track as the editor shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipView {
    /// The scene it shows, from 0.
    pub scene: usize,
    /// The file it plays, if any.
    pub file: Option<String>,
    /// Whether it is a video clip (else a still image or nothing).
    pub is_clip: bool,
    pub at: Duration,
    pub duration: Duration,
    pub media: ClipMedia,
    /// The scene's image, for thumbnails.
    pub thumbnail: Option<PathBuf>,
    /// Where the file is.
    pub path: Option<PathBuf>,
    /// The generation of the scene's image, and of its clip when it plays
    /// one: provider and model for the inspector.
    pub image: Option<Generation>,
    pub clip: Option<Generation>,
    /// The scene's narration.
    pub text: String,
}

/// One scene as the bin lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct BinScene {
    pub index: usize,
    pub duration: Duration,
    pub prompt: String,
    pub thumbnail: Option<PathBuf>,
}

/// A narrated word on the narration track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordMark {
    pub text: String,
    pub start: Duration,
    pub end: Duration,
}

/// The narration track (A1).
#[derive(Debug, Clone, PartialEq)]
pub struct NarrationTrack {
    pub file: String,
    pub duration: Duration,
    pub words: Vec<WordMark>,
    /// Peaks of its waveform, once built; `peaks_per_second` of them.
    pub peaks: Option<(u32, Vec<f32>)>,
}

/// A clip that needs the user: what the banner names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipProblem {
    pub scene: usize,
    pub file: Option<String>,
    pub media: ClipMedia,
}

/// What opening the editor shows, read again whenever jobs move.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorView {
    pub project: VideoProject,
    pub channel_name: String,
    /// `None` until the project has a narration and a scene plan.
    pub timeline: Option<Timeline>,
    /// Whether the project has a narration (the empty state says what is
    /// missing).
    pub has_narration: bool,
    /// Whether the narration changed since the scenes were planned.
    pub stale: bool,
    /// One per video item of the timeline, in order.
    pub clips: Vec<ClipView>,
    pub scenes: Vec<BinScene>,
    pub narration: Option<NarrationTrack>,
    /// Proxies ready, of those the timeline plays.
    pub proxies_ready: usize,
    pub proxies_total: usize,
    /// The project's latest proxies job.
    pub job: Option<Job>,
}

impl EditorView {
    pub fn is_empty(&self) -> bool {
        self.timeline.as_ref().is_none_or(Timeline::is_empty)
    }

    pub fn duration(&self) -> Duration {
        self.timeline
            .as_ref()
            .map_or(Duration::ZERO, Timeline::duration)
    }

    /// Whether proxies are still waiting or building.
    pub fn is_building(&self) -> bool {
        self.clips
            .iter()
            .any(|clip| clip.media == ClipMedia::Building)
            || self.job.as_ref().is_some_and(|job| job.state().is_active())
    }

    /// Clips the banner names.
    pub fn problems(&self) -> Vec<ClipProblem> {
        self.clips
            .iter()
            .filter(|clip| clip.media.needs_attention())
            .map(|clip| ClipProblem {
                scene: clip.scene,
                file: clip.file.clone(),
                media: clip.media.clone(),
            })
            .collect()
    }

    /// The clip under `time`.
    pub fn clip_at(&self, time: Duration) -> Option<usize> {
        self.timeline.as_ref()?.video_at(time)
    }
}

/// The preview's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewAspect {
    /// 16:9, each picture fitted to the frame.
    #[default]
    Landscape,
    /// 9:16, a centered window of each picture (per-clip framing is #24).
    Portrait,
}

impl PreviewAspect {
    pub const ALL: [PreviewAspect; 2] = [PreviewAspect::Landscape, PreviewAspect::Portrait];

    pub fn size(self) -> FrameSize {
        let (width, height) = match self {
            PreviewAspect::Landscape => PREVIEW_LANDSCAPE,
            PreviewAspect::Portrait => PREVIEW_PORTRAIT,
        };
        FrameSize::new(width, height)
    }

    fn framing(self) -> Framing {
        match self {
            PreviewAspect::Landscape => Framing::Fit,
            PreviewAspect::Portrait => Framing::Crop { x: 0.5, y: 0.5 },
        }
    }
}

/// A preview playing (or about to: waiting for its first picture and
/// sound).
struct Playing {
    frames: FrameStream,
    /// A decoded picture not yet due.
    pending: Option<VideoFrame>,
    audio: Option<Box<dyn StreamPlayback>>,
    from: Duration,
    asked_at: Instant,
    started_at: Option<Instant>,
}

/// The editor of one project: its view, the playhead, the selection and
/// the preview. Lives with the screen; `Bardo::refresh_editor` brings it up
/// to date when jobs move.
pub struct Editor {
    view: EditorView,
    media: Arc<dyn MediaEngine>,
    audio: Arc<dyn AudioOutput>,
    files: Arc<dyn ProjectFiles>,
    playhead: Duration,
    selection: Option<usize>,
    aspect: PreviewAspect,
    playing: Option<Playing>,
    /// The one picture asked for while paused.
    still: Option<FrameStream>,
    /// The last preview problem, until the next play or seek.
    error: Option<Text>,
}

impl Editor {
    pub fn view(&self) -> &EditorView {
        &self.view
    }

    pub fn project(&self) -> VideoProjectId {
        self.view.project.id
    }

    pub fn playhead(&self) -> Duration {
        self.playhead
    }

    pub fn selection(&self) -> Option<usize> {
        self.selection
    }

    pub fn aspect(&self) -> PreviewAspect {
        self.aspect
    }

    pub fn error(&self) -> Option<Text> {
        self.error
    }

    /// Whether the preview is playing or starting to.
    pub fn is_playing(&self) -> bool {
        self.playing.is_some()
    }

    /// Whether the preview can play: something to play, no proxy building.
    pub fn can_play(&self) -> bool {
        !self.view.is_empty() && !self.view.is_building()
    }

    /// Whether `tick` has work: a preview playing or a picture coming.
    pub fn needs_ticks(&self) -> bool {
        self.playing.is_some() || self.still.is_some()
    }

    /// Selects a clip of the video track, or nothing.
    pub fn select(&mut self, clip: Option<usize>) {
        self.selection = clip.filter(|&index| index < self.view.clips.len());
    }

    /// Switches the preview's shape.
    pub fn set_aspect(&mut self, aspect: PreviewAspect) {
        if aspect != self.aspect {
            self.aspect = aspect;
            self.restart();
        }
    }

    /// The timeline as the preview plays it: proxies for pictures, black
    /// where there is no proxy, the narration as recorded.
    pub fn preview_plan(&self) -> Option<RenderPlan> {
        let timeline = self.view.timeline.as_ref().filter(|t| !t.is_empty())?;
        let project = self.project();
        let framing = self.aspect.framing();
        let video = timeline
            .video()
            .iter()
            .map(|item| {
                let proxy = ProxyKind::of(&item.source).and_then(|kind| {
                    let name = proxy_name(item.source.file()?, kind);
                    self.files
                        .exists(project, &name)
                        .then(|| (kind, self.files.path(project, &name)))
                });
                let source = match proxy {
                    Some((ProxyKind::Clip, path)) => ClipSource::Video(path),
                    Some((_, path)) => ClipSource::Still(path),
                    None => ClipSource::Black,
                };
                VideoClip {
                    source,
                    start: Duration::ZERO,
                    duration: item.duration,
                    framing,
                }
            })
            .collect();
        let narration = timeline
            .narration()
            .iter()
            .filter(|item| self.files.exists(project, &item.file))
            .map(|item| AudioClip {
                source: self.files.path(project, &item.file),
                start: item.start,
                duration: item.duration,
                at: item.at,
                gain_db: 0.0,
            })
            .collect();
        Some(RenderPlan {
            video,
            audio: vec![AudioTrack {
                clips: narration,
                gain_db: 0.0,
            }],
        })
    }

    /// Plays from the playhead (from the start when it is at the end).
    pub fn play(&mut self) -> Result<(), EditorError> {
        if !self.can_play() {
            return Err(EditorError::PreviewNotReady);
        }
        if self.playing.is_some() {
            return Ok(());
        }
        if frame_at(self.playhead) + 1 >= frame_at(self.view.duration()) {
            self.playhead = Duration::ZERO;
        }
        self.start_playing().inspect_err(|error| {
            self.error = Some(error.message());
        })
    }

    fn start_playing(&mut self) -> Result<(), EditorError> {
        self.error = None;
        self.still = None;
        let plan = self.preview_plan().ok_or(EditorError::PreviewNotReady)?;
        let frames = self
            .media
            .preview(&plan, self.playhead, self.aspect.size(), (FPS, 1))?;
        // Without sound the preview still plays, on the wall clock.
        let audio = match self.media.preview_audio(&plan, self.playhead) {
            Ok(stream) => match self.audio.stream(stream) {
                Ok(playback) => Some(playback),
                Err(_) => {
                    self.error = Some(Text::EditorNoSound);
                    None
                }
            },
            Err(_) => {
                self.error = Some(Text::EditorNoSound);
                None
            }
        };
        self.playing = Some(Playing {
            frames,
            pending: None,
            audio,
            from: self.playhead,
            asked_at: Instant::now(),
            started_at: None,
        });
        Ok(())
    }

    pub fn pause(&mut self) {
        // Dropping the streams stops ffmpeg and the sound.
        self.playing = None;
    }

    pub fn toggle_play(&mut self) -> Result<(), EditorError> {
        if self.is_playing() {
            self.pause();
            Ok(())
        } else {
            self.play()
        }
    }

    /// Moves the playhead to the frame at `time` (within the timeline) and
    /// shows it; playing goes on from there.
    pub fn seek(&mut self, time: Duration) {
        let last = frame_at(self.view.duration()).saturating_sub(1);
        self.playhead = frame_time(frame_at(time).min(last));
        self.restart();
    }

    /// Steps `frames` frames back (negative) or forward, pausing first.
    pub fn step(&mut self, frames: i64) {
        self.pause();
        let frame = frame_at(self.playhead) as i64 + frames;
        self.seek(frame_time(frame.max(0) as u64));
    }

    /// Plays again from the playhead if playing, else shows its picture.
    fn restart(&mut self) {
        if self.playing.take().is_some() {
            if let Err(error) = self.start_playing() {
                self.error = Some(error.message());
            }
        } else {
            self.show_still();
        }
    }

    /// Asks for the picture under the playhead.
    fn show_still(&mut self) {
        self.still = None;
        if !self.can_play() {
            return;
        }
        let Some(plan) = self.preview_plan() else {
            return;
        };
        match self
            .media
            .preview(&plan, self.playhead, self.aspect.size(), (FPS, 1))
        {
            Ok(stream) => {
                self.error = None;
                self.still = Some(stream);
            }
            Err(error) => self.error = Some(EditorError::from(error).message()),
        }
    }

    /// Moves the preview on: call it every frame while `needs_ticks`.
    /// Returns a new picture to show, if there is one.
    pub fn tick(&mut self, now: Instant) -> Option<VideoFrame> {
        if let Some(still) = &self.still {
            let frame = still.try_next_frame();
            if frame.is_some() {
                self.still = None;
            }
            return frame;
        }
        let duration = self.view.duration();
        let playing = self.playing.as_mut()?;
        if playing.pending.is_none() {
            playing.pending = playing.frames.try_next_frame();
        }
        let started_at = match playing.started_at {
            Some(at) => at,
            None => {
                let audio_ready = playing.audio.as_ref().is_none_or(|a| a.is_buffered())
                    || now.duration_since(playing.asked_at) > AUDIO_PATIENCE;
                if playing.pending.is_none() || !audio_ready {
                    return None;
                }
                if let Some(audio) = playing.audio.as_mut() {
                    audio.play();
                }
                playing.started_at = Some(now);
                now
            }
        };
        let elapsed = match &playing.audio {
            Some(audio) => audio.position(),
            None => now.duration_since(started_at),
        };
        let time = (playing.from + elapsed).min(duration);
        self.playhead = time;
        let mut newest = None;
        while let Some(frame) = playing
            .pending
            .take()
            .or_else(|| playing.frames.try_next_frame())
        {
            if frame.at <= time {
                newest = Some(frame);
            } else {
                playing.pending = Some(frame);
                break;
            }
        }
        // The sound runs as long as the timeline, so its end is the end
        // (rounding can leave it a hair short of the duration).
        let sound_over = playing.audio.as_ref().is_some_and(|a| a.has_ended());
        if time >= duration || sound_over {
            self.playing = None;
            self.playhead = duration;
        }
        newest
    }

    /// Takes a fresh view, keeping the playhead and selection where they
    /// still fit, and shows the picture again if the timeline changed.
    fn update(&mut self, view: EditorView) {
        let changed =
            view.timeline != self.view.timeline || view.proxies_ready != self.view.proxies_ready;
        let was_unready = !self.can_play();
        self.view = view;
        if self
            .selection
            .is_some_and(|index| index >= self.view.clips.len())
        {
            self.selection = None;
        }
        let end = self.view.duration();
        if self.playhead > end {
            self.playhead = end;
        }
        if !self.can_play() {
            self.playing = None;
            self.still = None;
        } else if changed || was_unready {
            self.restart();
        }
    }
}

/// The project a proxies job serves.
#[derive(Deserialize)]
struct ProjectOf {
    project: String,
}

impl Bardo {
    fn editor_project(&self, project: VideoProjectId) -> Result<VideoProject, EditorError> {
        self.themes
            .project(project)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(EditorError::ProjectNotFound)
    }

    fn latest_proxies_job(&self, project: VideoProjectId) -> Option<Job> {
        let project = project.to_string();
        self.jobs().into_iter().rev().find(|job| {
            job.kind() == JobKind::Proxies
                && serde_json::from_str::<ProjectOf>(job.payload())
                    .is_ok_and(|of| of.project == project)
        })
    }

    /// What the editor shows for `project` now.
    pub fn editor_view(&self, project: VideoProjectId) -> Result<EditorView, EditorError> {
        let project = self.editor_project(project)?;
        let channel_name = self
            .channels
            .get(project.channel)?
            .map(|channel| channel.details.name().to_owned())
            .unwrap_or_default();
        let narration = self.narrations.narration(project.id)?;
        let plan = self.scene_plans.scene_plan(project.id)?;
        let job = self.latest_proxies_job(project.id);
        let problems = held_back(job.as_ref());
        let has = |name: &str| self.files.exists(project.id, name);
        let thumbnail = |scene: &Scene| {
            scene
                .image()
                .map(|image| self.files.path(project.id, &image.file))
        };

        let timeline = match (&plan, &narration) {
            (Some(plan), Some(narration)) => Some(Timeline::rough_cut(plan, narration)),
            _ => None,
        };
        let mut clips = Vec::new();
        let (mut ready, mut total) = (0, 0);
        if let (Some(timeline), Some(plan)) = (&timeline, &plan) {
            for item in timeline.video() {
                let scene = &plan.scenes()[item.scene];
                let media = match (item.source.file(), ProxyKind::of(&item.source)) {
                    (Some(file), Some(kind)) if has(file) => {
                        if has(&proxy_name(file, kind)) {
                            ClipMedia::Ready
                        } else {
                            problems.get(file).cloned().unwrap_or(ClipMedia::Building)
                        }
                    }
                    _ => ClipMedia::Missing,
                };
                let is_clip = matches!(item.source, VideoSource::Clip(_));
                clips.push(ClipView {
                    scene: item.scene,
                    file: item.source.file().map(str::to_owned),
                    is_clip,
                    at: item.at,
                    duration: item.duration,
                    media,
                    thumbnail: thumbnail(scene),
                    path: item
                        .source
                        .file()
                        .map(|file| self.files.path(project.id, file)),
                    image: scene.image().map(|image| image.generation.clone()),
                    clip: is_clip
                        .then(|| scene.clip().map(|clip| clip.generation.clone()))
                        .flatten(),
                    text: scene.text.clone(),
                });
            }
            for (file, kind) in proxy_orders(timeline) {
                if has(&file) {
                    total += 1;
                    if has(&proxy_name(&file, kind)) {
                        ready += 1;
                    }
                }
            }
        }
        let scenes = plan
            .as_ref()
            .map(|plan| {
                plan.scenes()
                    .iter()
                    .enumerate()
                    .map(|(index, scene)| BinScene {
                        index,
                        duration: scene.duration(),
                        prompt: scene.prompt().as_str().to_owned(),
                        thumbnail: thumbnail(scene),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let narration_track = narration.as_ref().map(|narration| {
            let peaks = self
                .files
                .read(
                    project.id,
                    &proxy_name(&narration.audio_file, ProxyKind::Audio),
                )
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Peaks>(&bytes).ok())
                .map(|peaks| (peaks.peaks_per_second, peaks.peaks));
            NarrationTrack {
                file: narration.audio_file.clone(),
                duration: narration.duration,
                words: narration
                    .words()
                    .map(|(text, timing)| WordMark {
                        text: text.to_owned(),
                        start: timing.start,
                        end: timing.end,
                    })
                    .collect(),
                peaks,
            }
        });
        Ok(EditorView {
            channel_name,
            stale: plan
                .as_ref()
                .is_some_and(|plan| plan.is_stale(narration.as_ref())),
            has_narration: narration.is_some(),
            timeline,
            clips,
            scenes,
            narration: narration_track,
            proxies_ready: ready,
            proxies_total: total,
            job,
            project,
        })
    }

    /// Opens the editor on `project` and starts building the proxies its
    /// timeline lacks.
    pub fn open_editor(&self, project: VideoProjectId) -> Result<Editor, EditorError> {
        let view = self.editor_view(project)?;
        let view = self.queue_missing_proxies(view, false)?;
        let mut editor = Editor {
            view,
            media: Arc::clone(&self.media),
            audio: Arc::clone(&self.audio),
            files: Arc::clone(&self.files),
            playhead: Duration::ZERO,
            selection: None,
            aspect: PreviewAspect::default(),
            playing: None,
            still: None,
            error: None,
        };
        editor.show_still();
        Ok(editor)
    }

    /// Reads the editor's view again (after jobs moved), queueing proxies
    /// for media that entered the timeline.
    pub fn refresh_editor(&self, editor: &mut Editor) -> Result<(), EditorError> {
        let view = self.editor_view(editor.project())?;
        let view = self.queue_missing_proxies(view, false)?;
        editor.update(view);
        Ok(())
    }

    /// Builds again the proxies that failed or were cancelled.
    pub fn retry_proxies(&self, editor: &mut Editor) -> Result<(), EditorError> {
        let view = self.editor_view(editor.project())?;
        let before = view.job.as_ref().map(Job::id);
        let view = self.queue_missing_proxies(view, true)?;
        if view.job.as_ref().map(Job::id) == before {
            return Err(EditorError::NothingToRetry);
        }
        editor.update(view);
        Ok(())
    }

    /// Queues a proxies job for the timeline's files that have none, unless
    /// one is running. Files the latest job failed on or was cancelled
    /// over wait for `retry`.
    fn queue_missing_proxies(
        &self,
        view: EditorView,
        retry: bool,
    ) -> Result<EditorView, EditorError> {
        let Some(timeline) = &view.timeline else {
            return Ok(view);
        };
        if view.job.as_ref().is_some_and(|job| job.state().is_active()) {
            return Ok(view);
        }
        let project = view.project.id;
        let held = if retry {
            HashMap::new()
        } else {
            held_back(view.job.as_ref())
        };
        let files: Vec<ProxyOrder> = proxy_orders(timeline)
            .into_iter()
            .filter(|(file, kind)| {
                self.files.exists(project, file)
                    && !self.files.exists(project, &proxy_name(file, *kind))
                    && !held.contains_key(file)
            })
            .map(|(file, kind)| ProxyOrder { file, kind })
            .collect();
        if files.is_empty() {
            return Ok(view);
        }
        let payload = to_json(&ProxiesPayload {
            project: project.to_string(),
            files,
        });
        self.jobs
            .enqueue(Job::new(self.profile.id, JobKind::Proxies, payload))?;
        // Read again, so the clips show as building under the new job.
        self.editor_view(project)
    }
}

/// The files the latest proxies job could not do, and why: a failed job's
/// failures, or every file of a cancelled one. They wait for a retry.
fn held_back(job: Option<&Job>) -> HashMap<String, ClipMedia> {
    let mut held = HashMap::new();
    let Some(job) = job else {
        return held;
    };
    match job.state() {
        JobState::Failed => {
            let checkpoint: ProxiesCheckpoint = job
                .checkpoint()
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or_default();
            for failure in checkpoint.failed {
                held.insert(failure.file, ClipMedia::ProxyFailed(failure.detail));
            }
        }
        JobState::Cancelled => {
            if let Ok(payload) = serde_json::from_str::<ProxiesPayload>(job.payload()) {
                for order in payload.files {
                    held.insert(order.file, ClipMedia::ProxyCancelled);
                }
            }
        }
        JobState::Queued | JobState::Running | JobState::Done => {}
    }
    held
}

/// Every file of the timeline and what it gets: pictures first, in order,
/// then the narration.
fn proxy_orders(timeline: &Timeline) -> Vec<(String, ProxyKind)> {
    let mut orders: Vec<(String, ProxyKind)> = Vec::new();
    let video = timeline
        .video()
        .iter()
        .filter_map(|item| Some((item.source.file()?.to_owned(), ProxyKind::of(&item.source)?)));
    let audio = timeline
        .narration()
        .iter()
        .map(|item| (item.file.clone(), ProxyKind::Audio));
    for order in video.chain(audio) {
        if !orders.contains(&order) {
            orders.push(order);
        }
    }
    orders
}

/// A media engine for tests: proxies are a few bytes, previews a few
/// frames.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use bardo_domain::{ProjectFiles, VideoProjectId};
    use bardo_media::ffmpeg::{
        FrameSize, FrameStream, MediaError, Monitor, RenderPlan, VideoFrame, Waveform,
    };
    use bardo_media::{MediaEngine, PcmStream};
    use bardo_storage::MemoryProjectFiles;

    /// One preview asked for.
    #[derive(Debug, Clone)]
    pub(crate) struct PreviewCall {
        pub(crate) plan: RenderPlan,
        pub(crate) from: Duration,
        pub(crate) size: FrameSize,
    }

    /// Writes every proxy into `files` (memory paths name the project and
    /// file), fails sources named in `failing`, and holds proxy building
    /// while `hold` is set. Previews are three frames from the playhead.
    #[derive(Default)]
    pub(crate) struct FakeMedia {
        files: Option<Arc<MemoryProjectFiles>>,
        pub(crate) failing: Mutex<Vec<String>>,
        pub(crate) hold: AtomicBool,
        pub(crate) not_found: AtomicBool,
        pub(crate) built: Mutex<Vec<PathBuf>>,
        pub(crate) previews: Mutex<Vec<PreviewCall>>,
    }

    impl FakeMedia {
        pub(crate) fn writing_to(files: Arc<MemoryProjectFiles>) -> Self {
            Self {
                files: Some(files),
                ..Self::default()
            }
        }

        pub(crate) fn previews(&self) -> Vec<PreviewCall> {
            self.previews.lock().unwrap().clone()
        }

        fn check(&self, source: &Path) -> Result<(), MediaError> {
            if self.not_found.load(Ordering::SeqCst) {
                return Err(MediaError::NotFound { tried: Vec::new() });
            }
            let name = source.file_name().unwrap().to_string_lossy().into_owned();
            if self.failing.lock().unwrap().contains(&name) {
                return Err(MediaError::Failed {
                    program: "ffmpeg".into(),
                    status: "exit status: 1".into(),
                    log: format!("{name}: Invalid data found when processing input"),
                });
            }
            Ok(())
        }

        fn write(&self, destination: &Path) {
            self.built.lock().unwrap().push(destination.to_owned());
            let Some(files) = &self.files else { return };
            let mut parts = destination.iter().rev();
            let name = parts.next().unwrap().to_string_lossy().into_owned();
            let project: VideoProjectId =
                uuid::Uuid::parse_str(&parts.next().unwrap().to_string_lossy())
                    .unwrap()
                    .into();
            files.write(project, &name, b"proxy").unwrap();
        }
    }

    impl MediaEngine for FakeMedia {
        fn build_proxy(
            &self,
            source: &Path,
            destination: &Path,
            monitor: &dyn Monitor,
        ) -> Result<(), MediaError> {
            while self.hold.load(Ordering::SeqCst) {
                if monitor.should_stop() {
                    return Err(MediaError::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            self.check(source)?;
            monitor.progress(1.0);
            self.write(destination);
            Ok(())
        }

        fn build_still_proxy(&self, source: &Path, destination: &Path) -> Result<(), MediaError> {
            while self.hold.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
            self.check(source)?;
            self.write(destination);
            Ok(())
        }

        fn waveform(&self, path: &Path, peaks_per_second: u32) -> Result<Waveform, MediaError> {
            self.check(path)?;
            Ok(Waveform {
                peaks_per_second,
                peaks: vec![0.25, 0.5, 1.0],
            })
        }

        fn preview(
            &self,
            plan: &RenderPlan,
            from: Duration,
            size: FrameSize,
            _fps: (u32, u32),
        ) -> Result<FrameStream, MediaError> {
            if self.not_found.load(Ordering::SeqCst) {
                return Err(MediaError::NotFound { tried: Vec::new() });
            }
            self.previews.lock().unwrap().push(PreviewCall {
                plan: plan.clone(),
                from,
                size,
            });
            let frames = (0..3)
                .map(|n| VideoFrame {
                    size,
                    at: from + bardo_domain::frame_time(n),
                    bgra: Vec::new(),
                })
                .collect();
            Ok(FrameStream::from_frames(frames))
        }

        fn preview_audio(
            &self,
            _plan: &RenderPlan,
            _from: Duration,
        ) -> Result<PcmStream, MediaError> {
            Ok(PcmStream::from_samples(2, 48_000, vec![0.0; 96]))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use bardo_domain::JobState;
    use bardo_media::ffmpeg::ClipSource;

    use super::*;
    use crate::BudgetConsent;
    use crate::scenes::tests::{Harness, done, project, wait_done};

    const PATIENCE: Duration = Duration::from_secs(10);

    /// Refreshes until the proxies job is over.
    fn settle(app: &Bardo, editor: &mut Editor) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            app.refresh_editor(editor).unwrap();
            if !editor.view().is_building() {
                return;
            }
            assert!(Instant::now() < deadline, "the proxies never settled");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_drawn_project_opens_as_a_rough_cut_at_its_scene_times() {
        let h = Harness::new();
        let app = h.start();
        let (project, plan) = h.drawn_project(&app);
        let editor = app.open_editor(project.id).unwrap();
        let view = editor.view();

        assert_eq!(view.project.id, project.id);
        assert!(view.channel_name.starts_with("Space Archives"));
        assert!(!view.is_empty());
        assert_eq!(view.clips.len(), plan.scenes().len());
        for (clip, scene) in view.clips.iter().zip(plan.scenes()) {
            assert_eq!(
                clip.at,
                frame_time(bardo_domain::nearest_frame(scene.start))
            );
            assert_eq!(
                clip.file.as_deref(),
                Some(scene.image().unwrap().file.as_str())
            );
            assert!(!clip.is_clip);
            assert_eq!(clip.text, scene.text);
        }
        assert_eq!(view.scenes.len(), plan.scenes().len());
        let narration = view.narration.as_ref().unwrap();
        assert!(!narration.words.is_empty());
        assert_eq!(
            view.duration(),
            frame_time(bardo_domain::nearest_frame(narration.duration))
        );
    }

    #[test]
    fn proxies_build_in_the_background_and_clips_show_it() {
        let h = Harness::new();
        let app = h.start();
        let (project, plan) = h.drawn_project(&app);
        h.media.hold.store(true, Ordering::SeqCst);

        let mut editor = app.open_editor(project.id).unwrap();

        let view = editor.view();
        assert!(
            view.clips
                .iter()
                .all(|clip| clip.media == ClipMedia::Building)
        );
        assert!(view.is_building());
        assert!(!editor.can_play());
        assert_eq!(
            view.proxies_total,
            plan.scenes().len() + 1,
            "images and the narration"
        );
        assert_eq!(view.proxies_ready, 0);
        let job = view.job.clone().unwrap();
        assert_eq!(job.kind(), JobKind::Proxies);
        assert!(matches!(editor.play(), Err(EditorError::PreviewNotReady)));
        // Opening again does not queue a second job while one runs.
        app.refresh_editor(&mut editor).unwrap();
        assert_eq!(editor.view().job.as_ref().unwrap().id(), job.id());

        h.media.hold.store(false, Ordering::SeqCst);
        settle(&app, &mut editor);

        let view = editor.view();
        assert!(view.clips.iter().all(|clip| clip.media == ClipMedia::Ready));
        assert_eq!(view.proxies_ready, view.proxies_total);
        assert_eq!(
            view.narration.as_ref().unwrap().peaks,
            Some((50, vec![0.25, 0.5, 1.0]))
        );
        assert!(editor.can_play());
        for scene in plan.scenes() {
            let image = &scene.image().unwrap().file;
            assert!(h.files.exists(project.id, &format!("proxy-{image}.jpg")));
        }
        // All there: reopening queues nothing.
        let again = app.open_editor(project.id).unwrap();
        assert_eq!(again.view().job.as_ref().unwrap().id(), job.id());
    }

    #[test]
    fn the_preview_plays_from_proxies_on_the_sound_clock() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);

        editor.play().unwrap();
        assert!(editor.is_playing());
        let call = h.media.previews().pop().unwrap();
        assert_eq!(call.from, Duration::ZERO);
        assert_eq!(call.size, FrameSize::new(960, 540));
        for (clip, item) in call.plan.video.iter().zip(&editor.view().clips) {
            let ClipSource::Still(path) = &clip.source else {
                panic!("stills play their JPEG proxy: {:?}", clip.source)
            };
            assert!(path.ends_with(format!("proxy-{}.jpg", item.file.as_ref().unwrap())));
            assert_eq!(clip.duration, item.duration);
            assert_eq!(clip.framing, Framing::Fit);
        }
        let narration = &call.plan.audio[0].clips[0];
        assert!(
            narration
                .source
                .ends_with(&editor.view().narration.as_ref().unwrap().file)
        );

        let now = Instant::now();
        let first = editor.tick(now).unwrap();
        assert_eq!(first.at, Duration::ZERO);
        assert!(
            h.audio.stream.lock().unwrap().playing,
            "the sound starts with the picture"
        );
        h.audio.stream.lock().unwrap().position = frame_time(2);
        let later = editor.tick(now).unwrap();
        assert_eq!(later.at, frame_time(2), "frames passed over are dropped");
        assert_eq!(editor.playhead(), frame_time(2));

        editor.pause();
        assert!(!editor.is_playing());
        assert!(!h.audio.stream.lock().unwrap().playing);
        assert_eq!(editor.playhead(), frame_time(2));
    }

    #[test]
    fn playing_waits_for_the_sound_to_buffer() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);
        h.audio.stream.lock().unwrap().buffered = false;

        editor.play().unwrap();
        let now = Instant::now();
        assert!(editor.tick(now).is_none());
        assert!(!h.audio.stream.lock().unwrap().playing);
        // Past the patience it plays anyway.
        assert!(editor.tick(now + AUDIO_PATIENCE * 2).is_some());
    }

    #[test]
    fn seeking_while_paused_shows_that_frame_and_while_playing_restarts_there() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);

        editor.seek(frame_time(10) + Duration::from_millis(5));
        assert_eq!(editor.playhead(), frame_time(10));
        assert!(editor.needs_ticks());
        assert_eq!(h.media.previews().last().unwrap().from, frame_time(10));
        assert_eq!(editor.tick(Instant::now()).unwrap().at, frame_time(10));
        assert!(!editor.needs_ticks(), "one picture, then the stream stops");

        editor.step(2);
        assert_eq!(editor.playhead(), frame_time(12));
        editor.step(-40);
        assert_eq!(editor.playhead(), Duration::ZERO);

        editor.play().unwrap();
        editor.seek(frame_time(20));
        assert!(editor.is_playing());
        assert_eq!(h.media.previews().last().unwrap().from, frame_time(20));
        assert_eq!(h.audio.stream.lock().unwrap().streams, 2);

        let past_end = editor.view().duration() + Duration::from_secs(5);
        editor.seek(past_end);
        assert_eq!(
            editor.playhead(),
            frame_time(frame_at(editor.view().duration()) - 1),
            "the last frame"
        );
    }

    #[test]
    fn playing_reaches_the_end_and_stops_there_then_starts_over() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);
        editor.play().unwrap();
        editor.tick(Instant::now());
        h.audio.stream.lock().unwrap().ended = true;
        editor.tick(Instant::now());
        assert!(!editor.is_playing());
        assert_eq!(editor.playhead(), editor.view().duration());

        editor.play().unwrap();
        assert_eq!(h.media.previews().last().unwrap().from, Duration::ZERO);
    }

    #[test]
    fn portrait_previews_a_centered_window() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);
        editor.set_aspect(PreviewAspect::Portrait);
        let call = h.media.previews().pop().unwrap();
        assert_eq!(call.size, FrameSize::new(304, 540));
        assert!(
            call.plan
                .video
                .iter()
                .all(|clip| clip.framing == Framing::Crop { x: 0.5, y: 0.5 })
        );
    }

    #[test]
    fn a_failed_proxy_is_named_and_built_again_on_retry() {
        let h = Harness::new();
        let app = h.start();
        let (project, plan) = h.drawn_project(&app);
        let broken = plan.scenes()[1].image().unwrap().file.clone();
        h.media.failing.lock().unwrap().push(broken.clone());

        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);

        let view = editor.view();
        let job = view.job.clone().unwrap();
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().unwrap().kind,
            bardo_domain::JobFailureKind::Media
        );
        let ClipMedia::ProxyFailed(reason) = &view.clips[1].media else {
            panic!("{:?}", view.clips[1].media)
        };
        assert!(reason.contains("Invalid data"));
        assert_eq!(
            view.clips[0].media,
            ClipMedia::Ready,
            "the others still build"
        );
        let problems = view.problems();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].file.as_deref(), Some(broken.as_str()));
        assert!(
            editor.can_play(),
            "the preview plays, black where the proxy failed"
        );
        editor.play().unwrap();
        let call = h.media.previews().pop().unwrap();
        assert_eq!(call.plan.video[1].source, ClipSource::Black);

        // Refreshing does not try the failed file again by itself.
        app.refresh_editor(&mut editor).unwrap();
        assert_eq!(editor.view().job.as_ref().unwrap().id(), job.id());

        h.media.failing.lock().unwrap().clear();
        app.retry_proxies(&mut editor).unwrap();
        assert_ne!(editor.view().job.as_ref().unwrap().id(), job.id());
        settle(&app, &mut editor);
        assert_eq!(editor.view().clips[1].media, ClipMedia::Ready);
        assert!(matches!(
            app.retry_proxies(&mut editor),
            Err(EditorError::NothingToRetry)
        ));
    }

    #[test]
    fn a_cancelled_proxies_job_is_not_queued_again_by_itself() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        h.media.hold.store(true, Ordering::SeqCst);
        let mut editor = app.open_editor(project.id).unwrap();
        let job = editor.view().job.clone().unwrap();
        // Wait until it runs, so the cancel stops a build in progress.
        let deadline = Instant::now() + PATIENCE;
        while app
            .jobs()
            .iter()
            .all(|j| j.id() != job.id() || j.state() != JobState::Running)
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        app.cancel_job(job.id()).unwrap();
        assert_eq!(wait_done(&app, job.id()).state(), JobState::Cancelled);
        h.media.hold.store(false, Ordering::SeqCst);

        app.refresh_editor(&mut editor).unwrap();
        let view = editor.view();
        assert_eq!(view.job.as_ref().unwrap().id(), job.id());
        // A still being written when the job stopped may have made it.
        assert!(
            view.clips
                .iter()
                .all(|clip| matches!(clip.media, ClipMedia::ProxyCancelled | ClipMedia::Ready))
        );
        assert!(!view.problems().is_empty());

        app.retry_proxies(&mut editor).unwrap();
        settle(&app, &mut editor);
        assert!(
            editor
                .view()
                .clips
                .iter()
                .all(|clip| clip.media == ClipMedia::Ready)
        );
    }

    #[test]
    fn a_scene_without_an_image_is_missing_media_and_black_in_the_preview() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.planned_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);
        let view = editor.view();
        assert!(
            view.clips
                .iter()
                .all(|clip| clip.media == ClipMedia::Missing)
        );
        assert!(view.clips.iter().all(|clip| clip.file.is_none()));
        assert_eq!(view.proxies_total, 1, "only the narration");
        assert_eq!(view.problems().len(), view.clips.len());
        editor.play().unwrap();
        let call = h.media.previews().pop().unwrap();
        assert!(
            call.plan
                .video
                .iter()
                .all(|clip| clip.source == ClipSource::Black)
        );
    }

    #[test]
    fn a_new_image_enters_the_cut_and_gets_its_proxy() {
        let h = Harness::new();
        let app = h.start();
        let (project, plan) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);
        let old = plan.scenes()[0].image().unwrap().file.clone();
        let old_proxy = format!("proxy-{old}.jpg");
        assert!(h.files.exists(project.id, &old_proxy));

        done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        app.accept_scene_image(project.id, 0).unwrap();
        settle(&app, &mut editor);

        let clip = &editor.view().clips[0];
        assert_ne!(clip.file.as_deref(), Some(old.as_str()));
        assert_eq!(clip.media, ClipMedia::Ready);
        assert!(
            !h.files.exists(project.id, &old_proxy),
            "the old proxy goes with its image"
        );
    }

    #[test]
    fn a_project_without_narration_opens_empty() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        let view = editor.view();
        assert!(view.is_empty());
        assert!(!view.has_narration);
        assert!(view.clips.is_empty());
        assert!(view.job.is_none(), "nothing to build");
        assert!(!editor.can_play());
        assert!(matches!(editor.play(), Err(EditorError::PreviewNotReady)));

        let narrated = h.narrated_project(&app);
        let view = app.open_editor(narrated.id).unwrap();
        assert!(view.view().is_empty());
        assert!(view.view().has_narration);
    }

    #[test]
    fn without_ffmpeg_the_proxies_fail_and_say_so() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        h.media.not_found.store(true, Ordering::SeqCst);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);
        let job = editor.view().job.clone().unwrap();
        assert_eq!(job.state(), JobState::Failed);
        assert!(
            job.failure()
                .unwrap()
                .detail
                .contains("ffmpeg was not found")
        );
        let error = EditorError::Preview(MediaError::NotFound { tried: Vec::new() });
        assert_eq!(error.message(), Text::EditorFfmpegMissing);
    }

    #[test]
    fn someone_elses_project_does_not_open() {
        let h = Harness::new();
        let app = h.start();
        assert!(matches!(
            app.open_editor(VideoProjectId::new()),
            Err(EditorError::ProjectNotFound)
        ));
    }
}
