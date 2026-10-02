//! The editor (PRD stories 56, 57, 68, 69): opening a video project shows
//! its rough cut, the timeline the scene plan and narration make
//! (`Timeline::rough_cut`), with a preview that plays it from proxies.
//!
//! Cut editing (stories 58, 59, 70): split, trim, move, reorder and delete
//! are domain edits (`bardo_domain::Edit`) the editor makes on its timeline
//! and saves at once, so the cut survives closing the project; undo and
//! redo walk the session's history. Cuts snap to the narration's words when
//! snapping is on. The saved cut belongs to the scene plan and narration it
//! was made on: a new plan or narration starts the editor over from the
//! rough cut, and says so. A scene's new image takes its place in the cut.
//!
//! Opening or refreshing the editor queues a job for the proxies the
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
use std::time::{Duration, Instant, SystemTime};

use bardo_domain::{
    Edge, Edit, EditError, FPS, Generation, History, ItemRef, Job, JobKind, JobState, NarrationId,
    NarrationRepository, ProjectFiles, RepositoryError, Scene, ScenePlanId, ScenePlanRepository,
    Shift, ThemeRepository, Timeline, TimelineRepository, Track, VideoProject, VideoProjectId,
    VideoSource, frame_at, frame_time, nearest_frame, snap,
};
use bardo_media::ffmpeg::{
    AudioClip, AudioTrack, ClipSource, FramePoll, FrameSize, FrameStream, Framing, MediaError,
    RenderPlan, VideoClip, VideoFrame,
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
/// How far short of the end the sound may stop and still count as done.
const SOUND_END_SLACK: Duration = Duration::from_millis(100);

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
    /// Nothing under the playhead to split, or nothing selected to delete.
    #[error("nothing to cut there")]
    NothingToCut,
    #[error("the edit could not be made: {0}")]
    Edit(#[from] EditError),
    /// The edit was made but could not be saved; the cut is as it was.
    #[error("the edit could not be saved: {0}")]
    NotSaved(RepositoryError),
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
            EditorError::NothingToCut => Text::EditorNothingToCut,
            EditorError::Edit(_) => Text::EditorCannotEdit,
            EditorError::NotSaved(_) => Text::EditorEditNotSaved,
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
    /// Where in the clip it starts (zero for a still).
    pub start: Duration,
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

/// A narrated word on the narration track, where the cut plays it.
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
    /// How long the narration file is.
    pub duration: Duration,
    /// The words the cut keeps, at their timeline times.
    pub words: Vec<WordMark>,
    /// Peaks of its waveform, once built; `peaks_per_second` of them, over
    /// the file's own time.
    pub peaks: Option<(u32, Vec<f32>)>,
}

/// The scene plan and narration a cut is made on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutBasis {
    pub scene_plan: ScenePlanId,
    pub narration: NarrationId,
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
    /// What the timeline is cut on, when there is one.
    pub basis: Option<CutBasis>,
    /// Whether a saved cut was left behind because the scenes or narration
    /// changed since it was made.
    pub cut_outdated: bool,
    /// Where the narration's words start and end on the timeline: what
    /// cuts snap to.
    pub word_boundaries: Vec<Duration>,
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
    /// Whether there is nothing to play: no timeline yet, or every item cut
    /// away.
    pub fn is_empty(&self) -> bool {
        self.timeline.as_ref().is_none_or(Timeline::is_empty)
    }

    /// Where `item` sits on the timeline and how long it is.
    pub fn span(&self, item: ItemRef) -> Option<(Duration, Duration)> {
        self.timeline.as_ref()?.span(item)
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

    /// The narration item under `time`.
    pub fn narration_at(&self, time: Duration) -> Option<usize> {
        self.timeline.as_ref()?.narration_at(time)
    }
}

/// What the user asks of the timeline. `reach` is how far a cut may move to
/// land on a word (zero, or snapping off, keeps it where it is).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditAction {
    /// Splits the selected item if the playhead is inside it, else the
    /// video clip under the playhead.
    SplitAtPlayhead {
        reach: Duration,
    },
    /// Moves an edge of `item` to `to`.
    Trim {
        item: ItemRef,
        edge: Edge,
        to: Duration,
        reach: Duration,
    },
    /// Moves a narration item to start at `to`.
    Move {
        item: ItemRef,
        to: Duration,
    },
    /// Takes video clip `from` to position `to`.
    Reorder {
        from: usize,
        to: usize,
    },
    /// Removes the selected item.
    DeleteSelection,
    Undo,
    Redo,
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
    selection: Option<ItemRef>,
    aspect: PreviewAspect,
    playing: Option<Playing>,
    /// The one picture asked for while paused.
    still: Option<FrameStream>,
    /// The last preview problem, until the next play or seek.
    error: Option<Text>,
    /// The edits of this session, to undo and redo.
    history: History,
    /// Whether cuts snap to the narration's words.
    snapping: bool,
    /// Whether opening found the saved cut left behind, until the next
    /// edit.
    cut_reset: bool,
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

    pub fn selection(&self) -> Option<ItemRef> {
        self.selection
    }

    /// The selected video clip, if a clip is selected.
    pub fn selected_clip(&self) -> Option<usize> {
        self.selection
            .filter(|item| item.track == Track::Video)
            .map(|item| item.index)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn snapping(&self) -> bool {
        self.snapping
    }

    pub fn set_snapping(&mut self, on: bool) {
        self.snapping = on;
    }

    /// Whether the saved cut was left behind on opening because the scenes
    /// or narration changed; until the next edit.
    pub fn cut_reset(&self) -> bool {
        self.cut_reset
    }

    /// Where a cut aimed at `time` lands: on the nearest word boundary
    /// within `reach` when snapping is on, else on the nearest frame. The
    /// flag says it found a word.
    pub fn snapped(&self, time: Duration, reach: Duration) -> (Duration, bool) {
        let word = self
            .snapping
            .then(|| snap(time, &self.view.word_boundaries, reach))
            .flatten();
        (
            frame_time(nearest_frame(word.unwrap_or(time))),
            word.is_some(),
        )
    }

    /// How far a trim of `item`'s `edge` toward `to` moves it, within what
    /// the item allows.
    fn trim_shift(
        &self,
        item: ItemRef,
        edge: Edge,
        to: Duration,
        reach: Duration,
    ) -> Option<Shift> {
        let timeline = self.view.timeline.as_ref()?;
        let (at, duration) = timeline.span(item)?;
        let from = match edge {
            Edge::Start => at,
            Edge::End => at + duration,
        };
        let (earliest, latest) = timeline.trim_limits(item, edge).ok()?;
        let (to, _) = self.snapped(to, reach);
        Some(Shift::between(from, to).clamp(earliest, latest))
    }

    /// Where `item` would sit while its `edge` is dragged to `to`: its new
    /// start and length, before the clips after it follow.
    pub fn trim_preview(
        &self,
        item: ItemRef,
        edge: Edge,
        to: Duration,
        reach: Duration,
    ) -> Option<(Duration, Duration)> {
        let (at, duration) = self.view.span(item)?;
        let shift = self.trim_shift(item, edge, to, reach)?;
        let end = at + duration;
        Some(match edge {
            Edge::Start => {
                let start = shift.move_time(at).unwrap_or(Duration::ZERO);
                (start, end.saturating_sub(start))
            }
            Edge::End => (at, shift.move_time(end).unwrap_or(end).saturating_sub(at)),
        })
    }

    /// Where a narration item dragged to start at `to` would go: on a
    /// frame, short of the items beside it.
    pub fn move_preview(&self, item: ItemRef, to: Duration) -> Option<Duration> {
        let (earliest, latest) = self.view.timeline.as_ref()?.move_limits(item).ok()?;
        let to = frame_time(nearest_frame(to));
        Some(latest.map_or(to, |latest| to.min(latest)).max(earliest))
    }

    /// The position video clip `from` takes when dropped at `time`: before
    /// the clip whose middle is past `time`.
    pub fn reorder_target(&self, from: usize, time: Duration) -> Option<usize> {
        let clips = &self.view.clips;
        if from >= clips.len() {
            return None;
        }
        let slot = clips
            .iter()
            .position(|clip| time < clip.at + clip.duration / 2)
            .unwrap_or(clips.len());
        Some(if slot > from { slot - 1 } else { slot })
    }

    /// The edit an action makes, and what is selected after it.
    fn edit_for(&self, action: EditAction) -> Result<(Edit, Option<ItemRef>), EditorError> {
        let timeline = self
            .view
            .timeline
            .as_ref()
            .ok_or(EditorError::NothingToCut)?;
        Ok(match action {
            EditAction::SplitAtPlayhead { reach } => {
                let (at, _) = self.snapped(self.playhead, reach);
                let inside = |item: &ItemRef| {
                    timeline
                        .span(*item)
                        .is_some_and(|(start, length)| start < at && at < start + length)
                };
                let item = self
                    .selection
                    .filter(inside)
                    .or_else(|| timeline.video_at(at).map(ItemRef::video))
                    .filter(inside)
                    .ok_or(EditorError::NothingToCut)?;
                let edit = Edit::Split {
                    track: item.track,
                    index: item.index,
                    at,
                };
                // The part after the cut, where the playhead goes on.
                (
                    edit,
                    Some(ItemRef {
                        index: item.index + 1,
                        ..item
                    }),
                )
            }
            EditAction::Trim {
                item,
                edge,
                to,
                reach,
            } => {
                let by = self
                    .trim_shift(item, edge, to, reach)
                    .ok_or(EditError::NoSuchItem)?;
                (
                    Edit::Trim {
                        track: item.track,
                        index: item.index,
                        edge,
                        by,
                    },
                    Some(item),
                )
            }
            EditAction::Move { item, to } => {
                let to = self.move_preview(item, to).ok_or(EditError::WrongTrack)?;
                (
                    Edit::Move {
                        track: item.track,
                        index: item.index,
                        to,
                    },
                    Some(item),
                )
            }
            EditAction::Reorder { from, to } => {
                (Edit::Reorder { from, to }, Some(ItemRef::video(to)))
            }
            EditAction::DeleteSelection => {
                let item = self.selection.ok_or(EditorError::NothingToCut)?;
                (
                    Edit::Delete {
                        track: item.track,
                        index: item.index,
                    },
                    None,
                )
            }
            EditAction::Undo | EditAction::Redo => return Err(EditorError::NothingToCut),
        })
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

    /// Selects an item of the timeline, or nothing.
    pub fn select(&mut self, item: Option<ItemRef>) {
        self.selection = item.filter(|&item| self.view.span(item).is_some());
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
        let mut video: Vec<VideoClip> = timeline
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
                    start: item.start,
                    duration: item.duration,
                    framing,
                }
            })
            .collect();
        // Black after the last clip while the narration runs on.
        let end = timeline.duration();
        if end > timeline.video_end() {
            video.push(VideoClip {
                source: ClipSource::Black,
                start: Duration::ZERO,
                duration: end - timeline.video_end(),
                framing,
            });
        }
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
        if self.playing.take().is_some() {
            // On a frame, so playing again starts on a whole one.
            self.playhead = frame_time(frame_at(self.playhead));
        }
    }

    /// The last frame's time: where playing to the end leaves the playhead.
    fn last_frame(&self) -> Duration {
        frame_time(frame_at(self.view.duration()).saturating_sub(1))
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
            return match still.poll_frame() {
                FramePoll::Ready(frame) => {
                    self.still = None;
                    Some(frame)
                }
                FramePoll::Waiting => None,
                FramePoll::Ended => {
                    // ffmpeg stopped without a picture: a broken proxy.
                    let failure = self.still.take().map(FrameStream::finish);
                    self.error = Some(preview_failure(failure));
                    None
                }
            };
        }
        let duration = self.view.duration();
        let last_frame = self.last_frame();
        let playing = self.playing.as_mut()?;
        if playing.pending.is_none() {
            match playing.frames.poll_frame() {
                FramePoll::Ready(frame) => playing.pending = Some(frame),
                FramePoll::Waiting => {}
                FramePoll::Ended if playing.started_at.is_none() => {
                    // Not one picture: ffmpeg failed before playing began.
                    let failure = self.playing.take().map(|playing| playing.frames.finish());
                    self.error = Some(preview_failure(failure));
                    return None;
                }
                FramePoll::Ended => {}
            }
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
        // The sound runs as long as the timeline, so its end is the end
        // (rounding can leave it a hair short). Ending well before means
        // it failed: play on without it, on the wall clock.
        if playing.audio.as_ref().is_some_and(|a| a.has_ended())
            && playing.from + elapsed + SOUND_END_SLACK < duration
        {
            playing.audio = None;
            if let Some(at) = now.checked_sub(elapsed) {
                playing.started_at = Some(at);
            }
            self.error = Some(Text::EditorNoSound);
        }
        let time = (playing.from + elapsed).min(duration);
        self.playhead = time;
        let mut newest = None;
        while let Some(frame) =
            playing
                .pending
                .take()
                .or_else(|| match playing.frames.poll_frame() {
                    FramePoll::Ready(frame) => Some(frame),
                    FramePoll::Waiting | FramePoll::Ended => None,
                })
        {
            if frame.at <= time {
                newest = Some(frame);
            } else {
                playing.pending = Some(frame);
                break;
            }
        }
        let sound_over = playing.audio.as_ref().is_some_and(|a| a.has_ended());
        if time >= duration || sound_over {
            self.playing = None;
            self.playhead = last_frame;
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
            .is_some_and(|item| self.view.span(item).is_none())
        {
            self.selection = None;
        }
        let last = self.last_frame();
        if self.playhead > last {
            self.playhead = last;
        }
        if !self.can_play() {
            self.playing = None;
            self.still = None;
        } else if changed || was_unready {
            self.restart();
        }
    }
}

/// What the preview says when ffmpeg stopped before giving a picture.
fn preview_failure(finished: Option<Result<(), MediaError>>) -> Text {
    match finished {
        Some(Err(error)) => EditorError::from(error).message(),
        _ => Text::EditorPreviewFailed,
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

        let saved = self.timelines.saved_timeline(project.id)?;
        let (timeline, basis, cut_outdated) = match (&plan, &narration) {
            (Some(plan), Some(narration)) => {
                let restored = saved
                    .as_ref()
                    .and_then(|saved| Timeline::restore(saved, plan, narration));
                let outdated = saved.is_some() && restored.is_none();
                let basis = CutBasis {
                    scene_plan: plan.id,
                    narration: narration.id,
                };
                (
                    Some(restored.unwrap_or_else(|| Timeline::rough_cut(plan, narration))),
                    Some(basis),
                    outdated,
                )
            }
            _ => (None, None, false),
        };
        let mut clips = Vec::new();
        let (mut ready, mut total) = (0, 0);
        if let (Some(timeline), Some(plan)) = (&timeline, &plan) {
            for item in timeline.video() {
                let scene = plan.scenes().get(item.scene);
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
                    start: item.start,
                    media,
                    thumbnail: scene.and_then(thumbnail),
                    path: item
                        .source
                        .file()
                        .map(|file| self.files.path(project.id, file)),
                    image: scene
                        .and_then(|scene| scene.image().map(|image| image.generation.clone())),
                    clip: scene
                        .filter(|_| is_clip)
                        .and_then(|scene| scene.clip().map(|clip| clip.generation.clone())),
                    text: scene.map(|scene| scene.text.clone()).unwrap_or_default(),
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
            let words = timeline.as_ref().map_or_else(Vec::new, |timeline| {
                narration
                    .words()
                    .filter_map(|(text, timing)| {
                        let start = timeline.on_narration(timing.start)?;
                        // A word cut short ends where its piece does.
                        let end = timeline
                            .on_narration(timing.end)
                            .filter(|end| *end >= start)
                            .unwrap_or(start);
                        Some(WordMark {
                            text: text.to_owned(),
                            start,
                            end,
                        })
                    })
                    .collect()
            });
            NarrationTrack {
                file: narration.audio_file.clone(),
                duration: narration.duration,
                words,
                peaks,
            }
        });
        let word_boundaries = match (&timeline, &narration) {
            (Some(timeline), Some(narration)) => timeline.word_boundaries(
                narration
                    .words()
                    .map(|(_, timing)| (timing.start, timing.end)),
            ),
            _ => Vec::new(),
        };
        Ok(EditorView {
            channel_name,
            stale: plan
                .as_ref()
                .is_some_and(|plan| plan.is_stale(narration.as_ref())),
            has_narration: narration.is_some(),
            timeline,
            basis,
            cut_outdated,
            word_boundaries,
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
    /// timeline lacks. A saved cut the scenes or narration left behind is
    /// replaced by the rough cut, once, and the editor says so.
    pub fn open_editor(&self, project: VideoProjectId) -> Result<Editor, EditorError> {
        let view = self.editor_view(project)?;
        let cut_reset = view.cut_outdated;
        if let (true, Some(timeline), Some(basis)) = (cut_reset, &view.timeline, view.basis) {
            self.save_cut(project, timeline, basis)?;
        }
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
            history: History::default(),
            snapping: true,
            cut_reset,
        };
        editor.show_still();
        Ok(editor)
    }

    /// Reads the editor's view again (after jobs moved), queueing proxies
    /// for media that entered the timeline. A timeline changed from outside
    /// (new scenes or narration) can no longer be undone.
    pub fn refresh_editor(&self, editor: &mut Editor) -> Result<(), EditorError> {
        let view = self.editor_view(editor.project())?;
        let view = self.queue_missing_proxies(view, false)?;
        if view.timeline != editor.view.timeline {
            editor.history.clear();
        }
        editor.update(view);
        Ok(())
    }

    fn save_cut(
        &self,
        project: VideoProjectId,
        timeline: &Timeline,
        basis: CutBasis,
    ) -> Result<(), RepositoryError> {
        self.timelines.save_timeline(&timeline.to_saved(
            project,
            self.profile.id,
            basis.scene_plan,
            basis.narration,
            SystemTime::now(),
        ))
    }

    /// Makes `action` on the editor's timeline and saves the cut. An edit
    /// that changes nothing (a trim already at its limit) is no error.
    pub fn edit(&self, editor: &mut Editor, action: EditAction) -> Result<(), EditorError> {
        let (Some(mut timeline), Some(basis)) = (editor.view.timeline.clone(), editor.view.basis)
        else {
            return Err(EditorError::NothingToCut);
        };
        let mut history = editor.history.clone();
        let made = match action {
            EditAction::Undo => history.undo(&mut timeline).map(|done| (done, None)),
            EditAction::Redo => history.redo(&mut timeline).map(|done| (done, None)),
            action => {
                let (edit, selection) = editor.edit_for(action)?;
                history
                    .apply(&mut timeline, &edit)
                    .map(|()| (true, selection))
            }
        };
        let selection = match made {
            Ok((true, selection)) => selection,
            Ok((false, _)) | Err(EditError::NoChange) => return Ok(()),
            Err(error) => {
                // A failed undo leaves a history that no longer fits.
                editor.history = history;
                return Err(error.into());
            }
        };
        let project = editor.project();
        self.save_cut(project, &timeline, basis)
            .map_err(EditorError::NotSaved)?;
        let view = self.editor_view(project)?;
        editor.history = history;
        editor.cut_reset = false;
        editor.update(view);
        editor.select(selection);
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
        /// Previews end without a picture, as ffmpeg does on a broken file.
        pub(crate) blank: AtomicBool,
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
            let count = if self.blank.load(Ordering::SeqCst) {
                0
            } else {
                3
            };
            let frames = (0..count)
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
        {
            let mut sound = h.audio.stream.lock().unwrap();
            sound.position = editor.view().duration();
            sound.ended = true;
        }
        editor.tick(Instant::now());
        assert!(!editor.is_playing());
        let last = frame_time(frame_at(editor.view().duration()) - 1);
        assert_eq!(
            editor.playhead(),
            last,
            "on the last frame, which can be shown"
        );

        editor.play().unwrap();
        assert_eq!(h.media.previews().last().unwrap().from, Duration::ZERO);
    }

    #[test]
    fn sound_that_stops_early_leaves_the_picture_playing_on_the_wall_clock() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);
        editor.play().unwrap();
        let start = Instant::now();
        editor.tick(start);
        h.audio.stream.lock().unwrap().ended = true;
        editor.tick(start + Duration::from_millis(100));
        assert!(editor.is_playing(), "a failed sound is not the end");
        assert_eq!(editor.error(), Some(Text::EditorNoSound));
        editor.tick(start + Duration::from_millis(300));
        assert_eq!(
            editor.playhead(),
            Duration::from_millis(200),
            "timed from the switch"
        );
    }

    #[test]
    fn a_preview_that_ends_without_a_picture_says_so_and_stops_asking() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);
        h.media.blank.store(true, Ordering::SeqCst);

        editor.seek(frame_time(10));
        assert!(editor.tick(Instant::now()).is_none());
        assert_eq!(editor.error(), Some(Text::EditorPreviewFailed));
        assert!(!editor.needs_ticks());

        editor.play().unwrap();
        editor.tick(Instant::now());
        assert!(!editor.is_playing());
        assert_eq!(editor.error(), Some(Text::EditorPreviewFailed));
    }

    #[test]
    fn pausing_leaves_the_playhead_on_a_frame() {
        let h = Harness::new();
        let app = h.start();
        let (project, _) = h.drawn_project(&app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(&app, &mut editor);
        editor.play().unwrap();
        editor.tick(Instant::now());
        h.audio.stream.lock().unwrap().position = Duration::from_millis(110);
        editor.tick(Instant::now());
        editor.pause();
        assert_eq!(editor.playhead(), frame_time(3));
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

    /// A drawn project open in the editor with its proxies built.
    fn opened(h: &Harness, app: &Bardo) -> (VideoProject, Editor) {
        let (project, _) = h.drawn_project(app);
        let mut editor = app.open_editor(project.id).unwrap();
        settle(app, &mut editor);
        (project, editor)
    }

    fn spans(editor: &Editor) -> Vec<(Duration, Duration)> {
        editor
            .view()
            .clips
            .iter()
            .map(|clip| (clip.at, clip.duration))
            .collect()
    }

    const REACH: Duration = Duration::from_millis(150);

    /// The test narration says a word every few milliseconds, closer than a
    /// frame; these tests put one word boundary where they need it.
    fn one_word_at(editor: &mut Editor, at: Duration) {
        editor.view.word_boundaries = vec![at];
    }

    #[test]
    fn cuts_snap_to_the_words_the_cut_keeps() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        let timeline = editor.view().timeline.clone().unwrap();
        let words: Vec<(Duration, Duration)> = app
            .narrations
            .narration(editor.project())
            .unwrap()
            .unwrap()
            .words()
            .map(|(_, timing)| (timing.start, timing.end))
            .collect();
        assert!(!editor.view().word_boundaries.is_empty());
        assert_eq!(
            editor.view().word_boundaries,
            timeline.word_boundaries(words.iter().copied())
        );

        // Cutting the narration's first half away leaves its words out.
        editor.set_snapping(false);
        let middle = frame_time(frame_at(editor.view().duration()) / 2);
        editor.seek(middle);
        editor.select(Some(ItemRef::narration(0)));
        app.edit(&mut editor, EditAction::SplitAtPlayhead { reach: REACH })
            .unwrap();
        editor.select(Some(ItemRef::narration(0)));
        app.edit(&mut editor, EditAction::DeleteSelection).unwrap();
        assert!(editor.view().word_boundaries.iter().all(|at| *at >= middle));
    }

    #[test]
    fn splitting_cuts_the_clip_under_the_playhead_on_the_nearest_word() {
        let h = Harness::new();
        let app = h.start();
        let (project, mut editor) = opened(&h, &app);
        let before = spans(&editor);
        let word = Duration::from_millis(140);
        assert!(word > before[0].0 + frame_time(1) && word + frame_time(1) < before[0].1);
        one_word_at(&mut editor, word);
        editor.seek(word + Duration::from_millis(80));

        app.edit(&mut editor, EditAction::SplitAtPlayhead { reach: REACH })
            .unwrap();

        let cut = frame_time(nearest_frame(word));
        let after = spans(&editor);
        assert_eq!(after.len(), before.len() + 1);
        assert_eq!(after[0], (before[0].0, cut - before[0].0));
        assert_eq!(after[1], (cut, before[0].1 - (cut - before[0].0)));
        assert_eq!(after[2..], before[1..]);
        assert_eq!(editor.view().clips[1].scene, 0, "both halves show scene 1");
        assert_eq!(
            editor.selection(),
            Some(ItemRef::video(1)),
            "the part after the cut"
        );
        assert!(editor.can_undo());

        // Saved: the cut comes back when the project opens again.
        let reopened = app.open_editor(project.id).unwrap();
        assert_eq!(spans(&reopened), after);
        assert!(!reopened.can_undo(), "the history is the session's");
        assert!(!reopened.cut_reset());
    }

    #[test]
    fn without_snapping_the_cut_stays_on_the_playhead() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        let word = Duration::from_millis(140);
        one_word_at(&mut editor, word);
        let playhead = frame_time(nearest_frame(word) + 2);
        editor.seek(playhead);
        editor.set_snapping(false);
        assert_eq!(editor.snapped(playhead, REACH), (playhead, false));

        app.edit(&mut editor, EditAction::SplitAtPlayhead { reach: REACH })
            .unwrap();
        assert_eq!(editor.view().clips[1].at, playhead);

        // A zero reach (Alt held) does the same with snapping on.
        editor.set_snapping(true);
        one_word_at(&mut editor, word);
        assert_eq!(editor.snapped(playhead, Duration::ZERO), (playhead, false));
        assert_eq!(
            editor.snapped(playhead, REACH),
            (frame_time(nearest_frame(word)), true)
        );
    }

    #[test]
    fn a_split_needs_the_playhead_inside_an_item() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        editor.set_snapping(false);
        let second = editor.view().clips[1].at;
        editor.seek(second);
        assert!(matches!(
            app.edit(&mut editor, EditAction::SplitAtPlayhead { reach: REACH }),
            Err(EditorError::NothingToCut)
        ));
        assert!(!editor.can_undo());
        assert_eq!(
            EditorError::NothingToCut.message(),
            Text::EditorNothingToCut
        );

        // A selected narration item is split before the clip under it.
        editor.seek(second + frame_time(3));
        editor.select(Some(ItemRef::narration(0)));
        app.edit(&mut editor, EditAction::SplitAtPlayhead { reach: REACH })
            .unwrap();
        let timeline = editor.view().timeline.as_ref().unwrap();
        assert_eq!(timeline.narration().len(), 2);
        assert_eq!(timeline.narration()[1].at, second + frame_time(3));
        assert_eq!(editor.selection(), Some(ItemRef::narration(1)));
    }

    #[test]
    fn undo_and_redo_walk_every_edit_and_save_each_step() {
        let h = Harness::new();
        let app = h.start();
        let (project, mut editor) = opened(&h, &app);
        let original = editor.view().timeline.clone();
        editor.select(Some(ItemRef::video(0)));
        app.edit(&mut editor, EditAction::DeleteSelection).unwrap();
        app.edit(&mut editor, EditAction::Reorder { from: 0, to: 1 })
            .unwrap();
        let edited = editor.view().timeline.clone();

        app.edit(&mut editor, EditAction::Undo).unwrap();
        app.edit(&mut editor, EditAction::Undo).unwrap();
        assert_eq!(editor.view().timeline, original);
        assert!(!editor.can_undo() && editor.can_redo());
        assert_eq!(
            app.open_editor(project.id).unwrap().view().timeline,
            original,
            "undoing saves too"
        );
        // Nothing left to undo: nothing happens.
        app.edit(&mut editor, EditAction::Undo).unwrap();

        app.edit(&mut editor, EditAction::Redo).unwrap();
        app.edit(&mut editor, EditAction::Redo).unwrap();
        assert_eq!(editor.view().timeline, edited);
        assert!(!editor.can_redo());
    }

    #[test]
    fn trimming_a_clip_end_moves_the_clips_after_it_and_snaps_to_a_word() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        let before = spans(&editor);
        let word = Duration::from_millis(140);
        one_word_at(&mut editor, word);
        let aim = word + Duration::from_millis(60);
        let end = frame_time(nearest_frame(word));
        let item = ItemRef::video(0);

        assert_eq!(
            editor.trim_preview(item, Edge::End, aim, REACH),
            Some((before[0].0, end - before[0].0)),
            "the dragged clip, snapped"
        );
        app.edit(
            &mut editor,
            EditAction::Trim {
                item,
                edge: Edge::End,
                to: aim,
                reach: REACH,
            },
        )
        .unwrap();
        let after = spans(&editor);
        assert_eq!(after[0].1, end);
        let shift = before[0].1 - end;
        assert_eq!(after[1], (before[1].0 - shift, before[1].1));
        // The narration stays: it now runs past the video.
        assert_eq!(
            editor.view().duration(),
            before.last().map(|(a, d)| *a + *d).unwrap()
        );

        // Dragged past the clip's start, it keeps a frame.
        assert_eq!(
            editor.trim_preview(item, Edge::End, Duration::ZERO, REACH),
            Some((Duration::ZERO, frame_time(1)))
        );
    }

    #[test]
    fn trimming_a_still_start_keeps_it_in_place_and_lengthens_it() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        editor.set_snapping(false);
        let before = spans(&editor);
        let item = ItemRef::video(1);
        let aim = before[1].0 - frame_time(6);
        assert_eq!(
            editor.trim_preview(item, Edge::Start, aim, REACH),
            Some((aim, before[1].1 + frame_time(6)))
        );
        app.edit(
            &mut editor,
            EditAction::Trim {
                item,
                edge: Edge::Start,
                to: aim,
                reach: REACH,
            },
        )
        .unwrap();
        let after = spans(&editor);
        assert_eq!(after[1].0, before[1].0, "the track is magnetic");
        assert_eq!(after[1].1, before[1].1 + frame_time(6));
        assert_eq!(after[2].0, before[2].0 + frame_time(6));
    }

    #[test]
    fn reordering_drops_a_clip_between_two_others() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        let clips = editor.view().clips.clone();
        let last = clips.len() - 1;
        let middle = |index: usize| clips[index].at + clips[index].duration / 2;
        let past_end = clips[last].at + clips[last].duration + Duration::from_secs(1);
        assert_eq!(editor.reorder_target(0, Duration::ZERO), Some(0));
        assert_eq!(editor.reorder_target(0, middle(1) + frame_time(1)), Some(1));
        assert_eq!(editor.reorder_target(0, past_end), Some(last));
        assert_eq!(editor.reorder_target(last, Duration::ZERO), Some(0));
        assert_eq!(editor.reorder_target(9, Duration::ZERO), None);

        app.edit(&mut editor, EditAction::Reorder { from: 0, to: last })
            .unwrap();
        let scenes: Vec<usize> = editor.view().clips.iter().map(|clip| clip.scene).collect();
        assert_eq!(scenes.last(), Some(&0));
        assert_eq!(scenes[0], 1);
        assert_eq!(editor.selection(), Some(ItemRef::video(last)));
    }

    #[test]
    fn deleting_narration_leaves_silence_and_moving_it_stops_at_its_neighbour() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        editor.set_snapping(false);
        let middle = frame_time(frame_at(editor.view().duration()) / 2);
        editor.seek(middle);
        editor.select(Some(ItemRef::narration(0)));
        app.edit(&mut editor, EditAction::SplitAtPlayhead { reach: REACH })
            .unwrap();
        editor.select(Some(ItemRef::narration(0)));
        app.edit(&mut editor, EditAction::DeleteSelection).unwrap();
        assert_eq!(editor.selection(), None);
        let timeline = editor.view().timeline.clone().unwrap();
        assert_eq!(timeline.narration().len(), 1);
        assert_eq!(timeline.narration()[0].at, middle, "silence before it");
        assert!(matches!(
            app.edit(&mut editor, EditAction::DeleteSelection),
            Err(EditorError::NothingToCut)
        ));

        // The narration's words follow the cut: those cut away are gone.
        let words = &editor.view().narration.as_ref().unwrap().words;
        assert!(words.iter().all(|word| word.start >= middle));

        let item = ItemRef::narration(0);
        assert_eq!(
            editor.move_preview(item, Duration::ZERO),
            Some(Duration::ZERO)
        );
        app.edit(
            &mut editor,
            EditAction::Move {
                item,
                to: Duration::from_millis(10),
            },
        )
        .unwrap();
        assert_eq!(
            editor.view().timeline.as_ref().unwrap().narration()[0].at,
            Duration::ZERO,
            "on a frame"
        );
        // Video items do not move; they reorder.
        assert!(matches!(
            app.edit(
                &mut editor,
                EditAction::Move {
                    item: ItemRef::video(0),
                    to: Duration::ZERO,
                },
            ),
            Err(EditorError::Edit(EditError::WrongTrack))
        ));
    }

    #[test]
    fn the_preview_plays_the_edited_cut() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        editor.set_snapping(false);
        let clip = editor.view().clips[2].clone();
        let duration = editor.view().duration();
        // Shorten the narration so the video outlasts it, then cut the
        // first clip's end.
        editor.select(Some(ItemRef::narration(0)));
        app.edit(
            &mut editor,
            EditAction::Trim {
                item: ItemRef::narration(0),
                edge: Edge::Start,
                to: frame_time(15),
                reach: REACH,
            },
        )
        .unwrap();
        app.edit(
            &mut editor,
            EditAction::Trim {
                item: ItemRef::video(2),
                edge: Edge::End,
                to: clip.at + clip.duration - frame_time(5),
                reach: REACH,
            },
        )
        .unwrap();
        let calls = h.media.previews().len();
        editor.play().unwrap();
        assert_eq!(h.media.previews().len(), calls + 1);
        let plan = h.media.previews().pop().unwrap().plan;
        let end = frame_time(frame_at(clip.at + clip.duration) - 5);
        assert_eq!(plan.video[2].duration, end - clip.at);
        // The narration ends later than the video now: black fills in.
        assert_eq!(plan.video.last().unwrap().source, ClipSource::Black);
        assert_eq!(plan.duration(), duration);
        let narration = &plan.audio[0].clips[0];
        assert_eq!(
            (narration.at, narration.start),
            (frame_time(15), frame_time(15))
        );
    }

    #[test]
    fn an_edit_shows_its_picture_again_while_paused() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        editor.tick(Instant::now());
        let calls = h.media.previews().len();
        app.edit(&mut editor, EditAction::Reorder { from: 0, to: 1 })
            .unwrap();
        assert_eq!(h.media.previews().len(), calls + 1, "the new cut's frame");
        assert!(editor.needs_ticks());
    }

    #[test]
    fn a_new_scene_plan_starts_the_cut_over_and_says_so_once() {
        let h = Harness::new();
        let app = h.start();
        let (project, mut editor) = opened(&h, &app);
        editor.select(Some(ItemRef::video(0)));
        app.edit(&mut editor, EditAction::DeleteSelection).unwrap();

        h.answer(crate::scenes::tests::plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, true, BudgetConsent::Ask)
                .unwrap(),
        );
        let editor = app.open_editor(project.id).unwrap();
        assert!(editor.cut_reset());
        let plan = app.scenes(project.id).unwrap().plan.unwrap();
        assert_eq!(
            editor.view().clips.len(),
            plan.scenes().len(),
            "the rough cut"
        );
        assert!(
            !app.open_editor(project.id).unwrap().cut_reset(),
            "said once"
        );
    }

    #[test]
    fn a_new_image_takes_its_scene_s_place_in_the_cut_and_ends_the_history() {
        let h = Harness::new();
        let app = h.start();
        let (project, mut editor) = opened(&h, &app);
        app.edit(&mut editor, EditAction::Reorder { from: 0, to: 2 })
            .unwrap();
        assert!(editor.can_undo());

        done(
            &app,
            app.regenerate_scene_image(project.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        app.accept_scene_image(project.id, 0).unwrap();
        settle(&app, &mut editor);

        let moved = &editor.view().clips[2];
        assert_eq!(moved.scene, 0, "still where it was cut to");
        let image = app.scenes(project.id).unwrap().plan.unwrap().scenes()[0]
            .image()
            .unwrap()
            .file
            .clone();
        assert_eq!(moved.file.as_deref(), Some(image.as_str()));
        assert!(!editor.can_undo(), "the timeline changed under the history");
    }

    #[test]
    fn an_edit_that_cannot_be_saved_leaves_the_cut_as_it_was() {
        struct Broken;
        impl TimelineRepository for Broken {
            fn saved_timeline(
                &self,
                _: VideoProjectId,
            ) -> Result<Option<bardo_domain::SavedTimeline>, RepositoryError> {
                Ok(None)
            }
            fn save_timeline(
                &self,
                _: &bardo_domain::SavedTimeline,
            ) -> Result<(), RepositoryError> {
                Err(RepositoryError("disk full".into()))
            }
        }
        let h = Harness::new();
        let app = h.start_with_timelines(Arc::new(Broken));
        let (_, mut editor) = opened(&h, &app);
        let before = editor.view().timeline.clone();
        editor.select(Some(ItemRef::video(0)));
        let error = app
            .edit(&mut editor, EditAction::DeleteSelection)
            .unwrap_err();
        assert_eq!(error.message(), Text::EditorEditNotSaved);
        assert_eq!(editor.view().timeline, before);
        assert!(!editor.can_undo());
    }

    #[test]
    fn a_trim_at_its_limit_changes_nothing_and_is_no_error() {
        let h = Harness::new();
        let app = h.start();
        let (_, mut editor) = opened(&h, &app);
        let item = ItemRef::narration(0);
        app.edit(
            &mut editor,
            EditAction::Trim {
                item,
                edge: Edge::Start,
                to: Duration::ZERO,
                reach: REACH,
            },
        )
        .unwrap();
        assert!(!editor.can_undo());
    }
}
