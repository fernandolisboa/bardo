//! Control videos (#191, docs/spec/performance-capture.md): what a take
//! model receives for each take, built on the computer from the take's
//! mode, and the free review that approves it before any video model is
//! called.
//!
//! - Recording mode cuts the take's stretch of the recording.
//! - Mesh mode covers the face, frame by frame, with its drawn mesh.
//! - Depth mode draws the performer as a depth figure over the set: the
//!   room as recorded, or an image (one of the project's scenes, or a
//!   picture the user chose), copied into the project folder. Over an
//!   image, nothing of the recording is decoded.
//!
//! Each is an MP4 with the take's stretch of the narration as its sound,
//! sized and encoded for every take model ([`control_size`],
//! [`control_bitrate`], [`MAX_CONTROL_BYTES`]).
//!
//! A take's control video is built once the analyses its mode needs are
//! finished, and again whenever its mode, set or cut changes. It is named
//! by a fingerprint of what it was made from ([`control_fingerprint`]); an
//! approval holds for that fingerprint only, so any change asks for a new
//! review.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use bardo_domain::{
    ApprovalBlock, CONTROL_KEYFRAME_EVERY, Capture, CaptureId, CaptureRepository, ControlVideo,
    DepthHistogram, FACE_HOLD, FIGURE_FAR_PERMILLE, FIGURE_MASK_FROM, FIGURE_NEAR_PERMILLE, FPS,
    FaceLandmarks, Job, JobFailure, JobFailureKind, JobId, JobKind, JobRepository, JobState,
    MAX_CONTROL_BYTES, Outbound, ProfileId, Progress, ProjectFiles, RgbFrame, SET_EXTENSIONS,
    SetSource, Take, TakeMode, TakeSet, VideoCodec, VideoProjectId, approval_block,
    control_bitrate, control_fingerprint, control_fps, control_size, face_cover, merged_checks,
    needed_analyses, take_frames,
};
use bardo_media::control::Canvas;
use bardo_media::ffmpeg::{
    AudioClip, AudioTrack, ClipSource, ControlOutput, FramePoll, FrameSize, FrameSource,
    FrameStream, Framing, GrayDepth, GraySource, MediaError, Monitor, RenderPlan, VideoClip,
    VideoEncoder, VideoFrame, VoiceCut,
};
use bardo_media::{AudioOutput, MediaEngine, StreamPlayback};
use serde::{Deserialize, Serialize};

use crate::captures::CaptureError;
use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, Text};

/// A control video job's payload: the take, by its place in the capture.
/// The job builds the take as it is when it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ControlPayload {
    pub(crate) project: String,
    pub(crate) capture: String,
    pub(crate) position: u32,
}

impl ControlPayload {
    fn of(project: VideoProjectId, capture: CaptureId, position: u32) -> ControlPayload {
        ControlPayload {
            project: project.to_string(),
            capture: capture.to_string(),
            position,
        }
    }

    pub(crate) fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid control video payload: {e}")))
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a control video payload serializes")
    }
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

fn media_failure(error: MediaError) -> JobFailure {
    JobFailure::new(JobFailureKind::Media, error.to_string())
}

/// The control video's file: the capture, the take (counted from 1), its
/// revision, the fingerprint of what made it and the job making it, so two
/// builds of one take never write the same file.
pub(crate) fn control_file(
    capture: CaptureId,
    position: u32,
    revision: u32,
    fingerprint: &str,
    job: JobId,
) -> String {
    let job = job.to_string();
    format!(
        "control-{capture}-{:03}-r{revision}-{fingerprint}-{}.mp4",
        position + 1,
        &job[..8.min(job.len())]
    )
}

/// A new set image's file, with the picture's extension.
fn set_file(extension: &str) -> String {
    format!("set-{}.{extension}", uuid::Uuid::new_v4())
}

/// Frames between a control video's keyframes at `fps_milli`.
fn keyframe_interval(fps_milli: u32) -> u32 {
    let millis = CONTROL_KEYFRAME_EVERY.as_millis() as u64;
    ((u64::from(fps_milli) * millis + 500_000) / 1_000_000).max(1) as u32
}

/// The control video job of take `position`, unless one already waits to
/// run (a running one may be building the take as it was).
pub(crate) fn control_job(
    owner: ProfileId,
    jobs: &[Job],
    project: VideoProjectId,
    capture: CaptureId,
    position: u32,
) -> Option<Job> {
    let payload = ControlPayload::of(project, capture, position);
    let waiting = jobs.iter().any(|job| {
        job.kind() == JobKind::ControlVideo
            && job.state() == JobState::Queued
            && ControlPayload::parse(job.payload()).is_ok_and(|p| p == payload)
    });
    (!waiting).then(|| Job::new(owner, JobKind::ControlVideo, payload.to_json()))
}

/// Queues take `position`'s control video from a job: once the analyses
/// its mode needs are finished, unless a build already waits.
pub(crate) fn queue_when_ready(
    owner: ProfileId,
    captures: &dyn CaptureRepository,
    jobs: &dyn JobRepository,
    project: VideoProjectId,
    capture: CaptureId,
    position: u32,
) {
    let ready = || -> Result<bool, bardo_domain::RepositoryError> {
        let Some(current) = captures.capture(project)?.filter(|c| c.id == capture) else {
            return Ok(false);
        };
        let Some(take) = current.takes.as_slice().get(position as usize) else {
            return Ok(false);
        };
        let analyses = captures.analyses(project)?;
        Ok(needed_analyses(&analyses, capture, position, take).is_some())
    };
    let queued = ready().and_then(|ready| {
        if !ready {
            return Ok(());
        }
        let existing = jobs.list(owner)?;
        match control_job(owner, &existing, project, capture, position) {
            Some(job) => jobs.save(&job),
            None => Ok(()),
        }
    });
    if let Err(error) = queued {
        // The take shows Build, which starts it.
        tracing::warn!(%error, take = position + 1, "could not queue a take's control video");
    }
}

/// Removes the control videos and set images of `capture`, which is going.
/// `controls` are the project's control videos, read before the capture
/// went (its rows go with it). A file that will not go (open in a player)
/// stays.
pub(crate) fn remove_control_files(
    files: &dyn ProjectFiles,
    capture: &Capture,
    controls: &[ControlVideo],
) {
    for control in controls.iter().filter(|c| c.capture == capture.id) {
        let _ = files.remove(capture.project, &control.file);
    }
    for take in capture.takes.as_slice() {
        if let Some(image) = take.set.image() {
            let _ = files.remove(capture.project, image);
        }
    }
}

/// What a control video is built from: the take as the capture has it now
/// and the analyses it draws on.
struct Build {
    capture: Capture,
    position: u32,
    take: Take,
    fingerprint: String,
}

impl Build {
    fn length(&self) -> Duration {
        self.take.end.saturating_sub(self.take.start)
    }
}

/// The take `position` of the project's capture `capture` as it is now,
/// when the analyses its mode needs are finished.
fn current_build(
    captures: &dyn CaptureRepository,
    project: VideoProjectId,
    capture: CaptureId,
    position: u32,
) -> Result<Option<Build>, bardo_domain::RepositoryError> {
    let Some(current) = captures.capture(project)?.filter(|c| c.id == capture) else {
        return Ok(None);
    };
    let Some(take) = current.takes.as_slice().get(position as usize).cloned() else {
        return Ok(None);
    };
    let analyses = captures.analyses(project)?;
    let Some(needed) = needed_analyses(&analyses, capture, position, &take) else {
        return Ok(None);
    };
    let fingerprint = control_fingerprint(capture, position, &take, current.narration, &needed);
    Ok(Some(Build {
        capture: current,
        position,
        take,
        fingerprint,
    }))
}

/// What building a control video counted.
struct Made {
    frames: u32,
    uncovered: u32,
}

/// Stops ffmpeg when the job is cancelled, and reports its progress.
struct ControlMonitor<'a>(&'a JobContext);

impl Monitor for ControlMonitor<'_> {
    fn should_stop(&self) -> bool {
        self.0.should_stop()
    }

    fn progress(&self, fraction: f32) {
        self.0.report_progress(Progress::from_permille(
            (fraction.clamp(0.0, 1.0) * 1000.0) as u16,
        ));
    }
}

/// A take's depth and person mask, frame by frame; past the last frame
/// (the depth model's rounding), the last one again.
struct DepthPlanes {
    size: (u32, u32),
    range: (u16, u16),
    depth: Box<dyn GraySource>,
    mask: Box<dyn GraySource>,
    depth_now: Vec<u16>,
    mask_now: Vec<u8>,
}

impl DepthPlanes {
    fn advance(&mut self) -> Result<(), MediaError> {
        if let Some(frame) = self.depth.next_frame()? {
            self.depth_now = frame
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
        }
        if let Some(frame) = self.mask.next_frame()? {
            self.mask_now = frame;
        }
        Ok(())
    }
}

/// What the figure is drawn over.
enum Ground {
    /// The recording's own frames.
    Room {
        frames: Box<dyn FrameSource>,
        last: Option<RgbFrame>,
    },
    /// A set image, the same on every frame.
    /// The set image as RGBA, put on every frame.
    Picture(Vec<u8>),
}

/// Runs control video jobs.
pub(crate) struct ControlVideoHandler {
    pub(crate) captures: Arc<dyn CaptureRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) media: Arc<dyn MediaEngine>,
}

impl JobHandler for ControlVideoHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload = ControlPayload::parse(payload)?;
        let id = |text: &str| uuid::Uuid::parse_str(text).map_err(unexpected);
        let project = VideoProjectId::from(id(&payload.project)?);
        let capture = CaptureId::from(id(&payload.capture)?);
        let position = payload.position;
        let Some(build) = current_build(self.captures.as_ref(), project, capture, position)
            .map_err(unexpected)?
        else {
            // The take went, or waits for an analysis that queues it.
            return Ok(());
        };
        let existing = self
            .captures
            .control_videos(project)
            .map_err(unexpected)?
            .into_iter()
            .find(|control| control.position == position);
        if existing.as_ref().is_some_and(|control| {
            control.capture == capture
                && control.fingerprint == build.fingerprint
                && self.files.exists(project, &control.file)
        }) {
            return Ok(());
        }

        let file = control_file(
            capture,
            position,
            build.take.revision,
            &build.fingerprint,
            cx.id(),
        );
        let destination = self.files.path(project, &file);
        let encoders = self.media.encoders().map_err(media_failure)?;
        let best = encoders.best(VideoCodec::H264).map_err(media_failure)?;
        let (made, encoder) = match self.make(&build, best, &destination, cx) {
            Err(MediaError::Cancelled) => return Ok(()),
            Err(error) if best.is_hardware() => {
                // A driver too old, a card busy elsewhere: the software
                // encoder makes it.
                tracing::warn!(%error, encoder = best.name(), "a control video's encoder failed");
                let software = encoders.software(VideoCodec::H264).map_err(media_failure)?;
                match self.make(&build, software, &destination, cx) {
                    Err(MediaError::Cancelled) => return Ok(()),
                    made => (made.map_err(media_failure)?, software),
                }
            }
            made => (made.map_err(media_failure)?, best),
        };

        let bytes = self.files.size(project, &file).map_err(unexpected)?;
        if bytes > MAX_CONTROL_BYTES {
            let _ = self.files.remove(project, &file);
            return Err(JobFailure::new(
                JobFailureKind::Media,
                format!(
                    "the control video came out at {} MB, over the take models' {} MB",
                    bytes / 1_000_000,
                    MAX_CONTROL_BYTES / 1_000_000
                ),
            ));
        }
        // The take may have changed while it was built: a newer job makes
        // that one.
        let still = current_build(self.captures.as_ref(), project, capture, position)
            .map_err(unexpected)?
            .is_some_and(|now| now.fingerprint == build.fingerprint);
        if !still {
            let _ = self.files.remove(project, &file);
            return Ok(());
        }
        // Read again: another build of the take may have finished, or
        // been approved, meanwhile.
        let existing = self
            .captures
            .control_videos(project)
            .map_err(unexpected)?
            .into_iter()
            .find(|control| control.position == position);
        let control = ControlVideo {
            project,
            capture,
            position,
            fingerprint: build.fingerprint.clone(),
            file: file.clone(),
            bytes,
            frames: made.frames,
            uncovered: made.uncovered,
            encoder: encoder.name().to_owned(),
            built_at: SystemTime::now(),
            job: Some(cx.id()),
            // An approval holds for what it saw: the same take built again
            // (its file was gone) keeps it, a changed take loses it.
            approved_at: existing
                .as_ref()
                .filter(|old| old.capture == capture && old.fingerprint == build.fingerprint)
                .and_then(|old| old.approved_at),
            replaced_approval: existing.as_ref().is_some_and(|old| {
                old.fingerprint != build.fingerprint
                    && (old.approved_at.is_some() || old.replaced_approval)
            }),
        };
        if !self
            .captures
            .save_control_video(&control)
            .map_err(unexpected)?
        {
            let _ = self.files.remove(project, &file);
            return Ok(());
        }
        if let Some(old) = existing.filter(|old| old.file != file) {
            let _ = self.files.remove(project, &old.file);
        }
        Ok(())
    }
}

impl ControlVideoHandler {
    fn make(
        &self,
        build: &Build,
        encoder: VideoEncoder,
        destination: &Path,
        cx: &JobContext,
    ) -> Result<Made, MediaError> {
        let capture = &build.capture;
        let length = build.length();
        let fps = control_fps(build.take.mode, capture.video.fps_milli);
        let output = ControlOutput {
            size: control_size(capture.video.width, capture.video.height),
            fps_milli: fps,
            encoder,
            bitrate: control_bitrate(length),
            keyframe_interval: keyframe_interval(fps),
        };
        let voice = VoiceCut {
            path: self.files.path(
                capture.project,
                &crate::narrations::audio_file(capture.narration),
            ),
            from: build.take.start,
            length,
        };
        match build.take.mode {
            TakeMode::Recording => {
                self.media.cut_recording(
                    &self.files.path(capture.project, &capture.video_file),
                    &output,
                    &voice,
                    destination,
                    &ControlMonitor(cx),
                )?;
                Ok(Made {
                    frames: take_frames(length, fps),
                    uncovered: 0,
                })
            }
            TakeMode::Mesh | TakeMode::Depth => self.draw(build, &output, &voice, destination, cx),
        }
    }

    /// Mesh and Depth: each analyzed frame drawn and written.
    fn draw(
        &self,
        build: &Build,
        output: &ControlOutput,
        voice: &VoiceCut,
        destination: &Path,
        cx: &JobContext,
    ) -> Result<Made, MediaError> {
        let capture = &build.capture;
        let project = capture.project;
        let take = &build.take;
        let size = output.size;
        let total = take_frames(build.length(), output.fps_milli);
        let face = self.read(
            project,
            &crate::take_analysis::face_file(capture.id, build.position, take.revision),
        )?;
        let landmarks = FaceLandmarks::from_json(&String::from_utf8_lossy(&face))
            .map_err(|error| MediaError::Parse(error.to_string()))?;
        let found: Vec<bool> = landmarks
            .frames
            .iter()
            .map(|frame| !frame.faces.is_empty())
            .collect();
        let hold =
            (FACE_HOLD.as_millis() as u64 * u64::from(output.fps_milli) / 1_000_000) as usize;
        let cover = face_cover(&found, hold);
        let cover_at = |index: usize| {
            cover
                .get(index.min(cover.len().saturating_sub(1)))
                .copied()?
        };
        let mesh_of = |frame: usize| landmarks.frames[frame].faces.first();

        let mut planes = match take.mode {
            TakeMode::Depth => Some(self.depth_planes(build, cx)?),
            TakeMode::Recording | TakeMode::Mesh => None,
        };
        let mut canvas = Canvas::new(size.0, size.1)
            .ok_or_else(|| MediaError::Parse("a control video with no picture".into()))?;
        let mut ground = match (take.mode, take.set.image()) {
            (TakeMode::Depth, Some(image)) => {
                let picture = self
                    .media
                    .cover_picture(&self.files.path(project, image), size)?;
                canvas.put_rgb(&picture);
                Ground::Picture(canvas.rgba().to_vec())
            }
            _ => Ground::Room {
                frames: self.media.video_frames(
                    &self.files.path(project, &capture.video_file),
                    take.start,
                    build.length(),
                    output.fps_milli,
                    size,
                )?,
                last: None,
            },
        };
        let mut sink = self.media.control_writer(destination, output, voice)?;
        let mut uncovered = 0;
        for index in 0..total {
            if cx.should_stop() {
                // Dropping the sink stops ffmpeg and removes its file.
                return Err(MediaError::Cancelled);
            }
            match &mut ground {
                Ground::Room { frames, last } => {
                    if let Some(frame) = frames.next_frame()? {
                        *last = Some(frame);
                    }
                    canvas.put_rgb(last.get_or_insert_with(|| RgbFrame::black(size.0, size.1)));
                }
                Ground::Picture(picture) => canvas.put_rgba(picture),
            }
            let covering = cover_at(index as usize);
            let mesh = covering.and_then(|c| mesh_of(c.nearest));
            if mesh.is_none() && take.mode == TakeMode::Mesh {
                uncovered += 1;
            }
            match &mut planes {
                Some(planes) => {
                    planes.advance()?;
                    canvas.depth_figure(
                        &planes.depth_now,
                        &planes.mask_now,
                        planes.size,
                        planes.range,
                        mesh,
                    );
                    if let Some(mesh) = mesh {
                        canvas.draw_mesh(mesh);
                    }
                }
                None => {
                    if let (Some(covering), Some(mesh)) = (covering, mesh) {
                        let growth = covering.growth(index as usize, hold);
                        if let Some(other) = covering.other.and_then(mesh_of) {
                            canvas.cover_face(other, growth);
                        }
                        canvas.cover_face(mesh, growth);
                        canvas.draw_mesh(mesh);
                    }
                }
            }
            sink.write(canvas.rgba())?;
            if index % 10 == 9 {
                cx.report_progress(Progress::of(u64::from(index + 1), u64::from(total)));
            }
        }
        sink.finish()?;
        Ok(Made {
            frames: total,
            uncovered,
        })
    }

    fn read(&self, project: VideoProjectId, name: &str) -> Result<Vec<u8>, MediaError> {
        self.files
            .read(project, name)
            .map_err(|error| MediaError::Io(std::io::Error::other(error.to_string())))
    }

    /// The take's depth and mask, with the depth range the figure is
    /// colored over: inside the person mask, over the whole take.
    fn depth_planes(&self, build: &Build, cx: &JobContext) -> Result<DepthPlanes, MediaError> {
        let capture = &build.capture;
        let project = capture.project;
        let revision = build.take.revision;
        let depth_path = self.files.path(
            project,
            &crate::take_analysis::depth_file(capture.id, build.position, revision),
        );
        let mask_path = self.files.path(
            project,
            &crate::take_analysis::mask_file(capture.id, build.position, revision),
        );
        let size = self
            .media
            .probe(&depth_path)?
            .video
            .map(|video| (video.width, video.height))
            .ok_or_else(|| MediaError::Parse("the take's depth has no picture".into()))?;
        let mut histogram = DepthHistogram::default();
        let mut depth = self
            .media
            .gray_frames(&depth_path, size, GrayDepth::Sixteen)?;
        let mut mask = self.media.gray_frames(&mask_path, size, GrayDepth::Eight)?;
        while let (Some(depth), Some(mask)) = (depth.next_frame()?, mask.next_frame()?) {
            if cx.should_stop() {
                return Err(MediaError::Cancelled);
            }
            let inside: Vec<u16> = depth
                .as_chunks::<2>()
                .0
                .iter()
                .zip(&mask)
                .filter(|(_, person)| **person >= FIGURE_MASK_FROM)
                .map(|(pair, _)| u16::from_le_bytes(*pair))
                .collect();
            histogram.add(&inside);
        }
        let range = if histogram.count() == 0 {
            (0, u16::MAX)
        } else {
            (
                histogram.percentile(FIGURE_FAR_PERMILLE),
                histogram.percentile(FIGURE_NEAR_PERMILLE),
            )
        };
        let cells = size.0 as usize * size.1 as usize;
        Ok(DepthPlanes {
            size,
            range,
            depth: self
                .media
                .gray_frames(&depth_path, size, GrayDepth::Sixteen)?,
            mask: self.media.gray_frames(&mask_path, size, GrayDepth::Eight)?,
            depth_now: vec![0; cells],
            mask_now: vec![0; cells],
        })
    }
}

/// Where a take's control video is, for the Capture stage.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlStatus {
    /// The analyses the take's mode needs are not finished.
    Waiting,
    /// Neither built nor queued: Build makes it.
    NotBuilt,
    Queued,
    Building {
        progress: Progress,
    },
    /// Stopped by the user; Build makes it.
    Cancelled,
    /// Failed; Build tries again.
    Failed {
        kind: JobFailureKind,
    },
    /// Built from the take as it is now.
    Ready {
        approved: bool,
    },
}

impl ControlStatus {
    /// Whether it runs or waits to.
    pub fn is_active(&self) -> bool {
        matches!(self, ControlStatus::Queued | ControlStatus::Building { .. })
    }
}

/// A take's control video as the Capture stage shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlVideoView {
    pub mode: TakeMode,
    pub set: TakeSet,
    /// What the control video carries off the computer.
    pub outbound: Outbound,
    pub status: ControlStatus,
    /// The build that is ready, if any.
    pub built: Option<ControlVideo>,
    /// Why it cannot be approved, if anything.
    pub block: Option<ApprovalBlock>,
    /// An approval was given to an earlier build, before the take changed.
    pub approval_lost: bool,
    /// The job building it, to cancel.
    pub job: Option<JobId>,
}

impl ControlVideoView {
    pub fn is_approved(&self) -> bool {
        self.status == ControlStatus::Ready { approved: true }
    }

    /// Whether Approve applies: built, not approved, nothing blocking.
    pub fn can_approve(&self) -> bool {
        self.status == ControlStatus::Ready { approved: false } && self.block.is_none()
    }
}

/// Where a take's set comes from, as the user picks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetChoice {
    /// The recording's own background.
    Room,
    /// A scene's image, by its file in the project folder.
    Scene(String),
    /// A PNG or JPEG of the user's.
    Picture(SetPicture),
}

/// The largest set picture taken, in bytes and on its longer side.
const MAX_SET_BYTES: u64 = 50_000_000;
const MAX_SET_SIDE: u32 = 8_192;

/// A PNG or JPEG of the user's, read and checked, to be a take's set.
#[derive(Clone, PartialEq, Eq)]
pub struct SetPicture {
    bytes: Vec<u8>,
    extension: &'static str,
}

impl std::fmt::Debug for SetPicture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SetPicture({} bytes of {})",
            self.bytes.len(),
            self.extension
        )
    }
}

impl SetPicture {
    /// Reads the picture at `path`: a `.png`, `.jpg` or `.jpeg` file that
    /// is one, of at most 50 MB and 8192 px a side. It reads the whole
    /// file, so it runs off the interface's thread.
    pub fn read(path: &Path) -> Result<SetPicture, CaptureError> {
        use std::io::Read as _;
        path.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .filter(|e| SET_EXTENSIONS.contains(&e.as_str()))
            .ok_or(CaptureError::SetUnreadable)?;
        let file = std::fs::File::open(path).map_err(|_| CaptureError::SetUnreadable)?;
        let mut bytes = Vec::new();
        file.take(MAX_SET_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| CaptureError::SetUnreadable)?;
        if bytes.len() as u64 > MAX_SET_BYTES {
            return Err(CaptureError::SetUnreadable);
        }
        let extension = picture_extension(&bytes)?;
        Ok(SetPicture { bytes, extension })
    }
}

/// A scene image a take's set can be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetScene {
    /// The scene's number, from 1.
    pub number: usize,
    pub file: String,
    pub path: PathBuf,
}

impl Bardo {
    /// The control video jobs of capture `capture`, oldest first, with
    /// their payloads.
    fn control_jobs(&self, capture: CaptureId) -> Vec<(Job, ControlPayload)> {
        let id = capture.to_string();
        self.jobs()
            .into_iter()
            .filter(|job| job.kind() == JobKind::ControlVideo)
            .filter_map(|job| {
                let payload = ControlPayload::parse(job.payload()).ok()?;
                (payload.capture == id).then_some((job, payload))
            })
            .collect()
    }

    /// Each take's control video, in order.
    pub fn take_controls(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<ControlVideoView>, CaptureError> {
        let (capture, _) = self.current_capture(project)?;
        Ok(self.control_views(&capture)?)
    }

    /// Each of `capture`'s takes' control video, in order.
    pub(crate) fn control_views(
        &self,
        capture: &Capture,
    ) -> Result<Vec<ControlVideoView>, bardo_domain::RepositoryError> {
        let project = capture.project;
        let analyses = self.captures.analyses(project)?;
        let controls = self.captures.control_videos(project)?;
        let jobs = self.control_jobs(capture.id);
        Ok(capture
            .takes
            .as_slice()
            .iter()
            .enumerate()
            .map(|(index, take)| {
                let position = index as u32;
                let needed = needed_analyses(&analyses, capture.id, position, take);
                let fingerprint = needed.as_ref().map(|needed| {
                    control_fingerprint(capture.id, position, take, capture.narration, needed)
                });
                let control = controls
                    .iter()
                    .find(|c| c.position == position && c.capture == capture.id);
                let built = control
                    .filter(|c| Some(&c.fingerprint) == fingerprint.as_ref())
                    .filter(|c| self.files.exists(project, &c.file))
                    .cloned();
                let latest = jobs
                    .iter()
                    .rev()
                    .find(|(_, payload)| payload.position == position)
                    .map(|(job, _)| job);
                let status = match (&needed, &built, latest) {
                    (None, _, _) => ControlStatus::Waiting,
                    (Some(_), Some(built), _) => ControlStatus::Ready {
                        approved: built.approved_at.is_some(),
                    },
                    (Some(_), None, Some(job)) => match job.state() {
                        JobState::Queued => ControlStatus::Queued,
                        JobState::Running => ControlStatus::Building {
                            progress: job.progress(),
                        },
                        JobState::Cancelled => ControlStatus::Cancelled,
                        JobState::Failed => ControlStatus::Failed {
                            kind: job
                                .failure()
                                .map_or(JobFailureKind::Unexpected, |failure| failure.kind),
                        },
                        JobState::Done => ControlStatus::NotBuilt,
                    },
                    (Some(_), None, None) => ControlStatus::NotBuilt,
                };
                let checks = needed.as_deref().and_then(merged_checks);
                let block = approval_block(
                    take,
                    checks.as_ref(),
                    built.as_ref().map_or(0, |b| b.uncovered),
                );
                ControlVideoView {
                    mode: take.mode,
                    set: take.set.clone(),
                    outbound: Outbound::of(take.mode, &take.set),
                    job: latest.filter(|job| job.state().is_active()).map(Job::id),
                    status,
                    built,
                    block,
                    approval_lost: control.is_some_and(|c| {
                        let current = Some(&c.fingerprint) == fingerprint.as_ref();
                        if current {
                            c.replaced_approval && c.approved_at.is_none()
                        } else {
                            c.approved_at.is_some() || c.replaced_approval
                        }
                    }),
                }
            })
            .collect())
    }

    /// Queues take `index`'s control video, when its analyses are finished
    /// and no build waits.
    pub(crate) fn queue_control_video(
        &self,
        capture: &Capture,
        index: usize,
    ) -> Result<(), CaptureError> {
        let Some(take) = capture.takes.as_slice().get(index) else {
            return Ok(());
        };
        let analyses = self.captures.analyses(capture.project)?;
        if needed_analyses(&analyses, capture.id, index as u32, take).is_none() {
            return Ok(());
        }
        if let Some(job) = control_job(
            self.profile.id,
            &self.jobs(),
            capture.project,
            capture.id,
            index as u32,
        ) {
            self.jobs.enqueue(job)?;
        }
        Ok(())
    }

    /// Builds take `index`'s control video: again after a failure or a
    /// cancel, or for a take whose build was never queued.
    pub fn build_control_video(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(), CaptureError> {
        let (capture, _) = self.current_capture(project)?;
        if index >= capture.takes.as_slice().len() {
            return Err(CaptureError::Boundary(
                bardo_domain::BoundaryError::NoSuchTake,
            ));
        }
        if self.performed_busy(&capture, index) {
            return Err(CaptureError::TakeBusy);
        }
        self.queue_control_video(&capture, index)
    }

    /// Approves take `index`'s control video as built: the take may go to
    /// its take model. Any later change to the take asks for a new review.
    pub fn approve_control_video(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(), CaptureError> {
        let views = self.take_controls(project)?;
        let view = views.get(index).ok_or(CaptureError::Boundary(
            bardo_domain::BoundaryError::NoSuchTake,
        ))?;
        if let Some(block) = view.block {
            return Err(CaptureError::NotApprovable(block));
        }
        let Some(built) = view.built.as_ref() else {
            return Err(CaptureError::ControlNotReady);
        };
        if !self.captures.approve_control_video(
            project,
            index as u32,
            &built.fingerprint,
            SystemTime::now(),
        )? {
            return Err(CaptureError::ControlNotReady);
        }
        Ok(())
    }

    /// The scene images a take's set can be, in scene order.
    pub fn set_scenes(&self, project: VideoProjectId) -> Result<Vec<SetScene>, CaptureError> {
        let Some(plan) = self.scene_plans.scene_plan(project)? else {
            return Ok(Vec::new());
        };
        Ok(plan
            .scenes()
            .iter()
            .enumerate()
            .filter_map(|(index, scene)| {
                let image = scene.image()?;
                self.files.exists(project, &image.file).then(|| SetScene {
                    number: index + 1,
                    file: image.file.clone(),
                    path: self.files.path(project, &image.file),
                })
            })
            .collect())
    }

    /// Sets where take `index`'s performer appears in Depth mode. An image
    /// is copied into the project folder, so the take keeps it whatever
    /// happens to the scene or the file chosen.
    pub fn set_take_set(
        &self,
        project: VideoProjectId,
        index: usize,
        choice: SetChoice,
    ) -> Result<(), CaptureError> {
        let (mut capture, _) = self.current_capture(project)?;
        if self.performed_busy(&capture, index) {
            return Err(CaptureError::TakeBusy);
        }
        let old = capture
            .takes
            .as_slice()
            .get(index)
            .ok_or(CaptureError::Boundary(
                bardo_domain::BoundaryError::NoSuchTake,
            ))?
            .set
            .clone();
        if old == TakeSet::Room && choice == SetChoice::Room {
            return Ok(());
        }
        let set = match choice {
            SetChoice::Room => TakeSet::Room,
            SetChoice::Scene(file) => {
                if !self
                    .set_scenes(project)?
                    .iter()
                    .any(|scene| scene.file == file)
                {
                    return Err(CaptureError::SetUnreadable);
                }
                let bytes = self
                    .files
                    .read(project, &file)
                    .map_err(|_| CaptureError::SetUnreadable)?;
                let name = set_file(picture_extension(&bytes)?);
                self.files
                    .write(project, &name, &bytes)
                    .map_err(|_| CaptureError::SetUnreadable)?;
                TakeSet::Image {
                    file: name,
                    from: SetSource::Scene,
                }
            }
            SetChoice::Picture(picture) => {
                // The bytes checked are the ones kept.
                let name = set_file(picture.extension);
                self.files
                    .write(project, &name, &picture.bytes)
                    .map_err(|_| CaptureError::SetUnreadable)?;
                TakeSet::Image {
                    file: name,
                    from: SetSource::Imported,
                }
            }
        };
        capture.takes.set_set(index, set)?;
        if let Err(error) = self
            .captures
            .save_takes(capture.project, capture.id, &capture.takes)
        {
            if let Some(image) = capture.takes.as_slice()[index].set.image() {
                let _ = self.files.remove(project, image);
            }
            return Err(error.into());
        }
        if let Some(image) = old.image() {
            let _ = self.files.remove(project, image);
        }
        self.queue_control_video(&capture, index)
    }

    /// Where a control video's file is, for its player.
    pub fn control_video_path(&self, control: &ControlVideo) -> PathBuf {
        self.files.path(control.project, &control.file)
    }

    /// A player of take `index`'s control video as built, playing.
    pub fn control_player(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<ControlPlayer, CaptureError> {
        let (capture, _) = self.current_capture(project)?;
        let views = self.take_controls(project)?;
        let built = views
            .get(index)
            .and_then(|view| view.built.clone())
            .ok_or(CaptureError::ControlNotReady)?;
        let take = capture
            .takes
            .as_slice()
            .get(index)
            .ok_or(CaptureError::ControlNotReady)?;
        let (width, height) = control_size(capture.video.width, capture.video.height);
        Ok(ControlPlayer::new(
            Arc::clone(&self.media),
            Arc::clone(&self.audio),
            self.control_video_path(&built),
            take.end.saturating_sub(take.start),
            (width, height),
        ))
    }
}

/// The extension a set picture is saved with, from its bytes: PNG or JPEG,
/// at most [`MAX_SET_SIDE`] a side.
fn picture_extension(bytes: &[u8]) -> Result<&'static str, CaptureError> {
    let fits = bardo_domain::image_dimensions(bytes)
        .is_some_and(|(width, height)| width.max(height) <= MAX_SET_SIDE);
    if !fits {
        return Err(CaptureError::SetUnreadable);
    }
    if bytes.starts_with(b"\x89PNG") {
        Ok("png")
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Ok("jpg")
    } else {
        Err(CaptureError::SetUnreadable)
    }
}

/// How long playing waits for the sound before going on without it.
const AUDIO_PATIENCE: Duration = Duration::from_secs(1);

/// The largest picture the review player shows a control video at.
const PLAYER_LONG_SIDE: u32 = 540;

struct Playing {
    frames: FrameStream,
    pending: Option<VideoFrame>,
    audio: Option<Box<dyn StreamPlayback>>,
    asked_at: Instant,
    started_at: Option<Instant>,
}

/// Plays a control video with its sound, from the start, for its review:
/// playing as soon as it is made. It also plays a take model's draft or
/// final, with the control video's sound (the take's voice) in place of
/// the provider's.
pub struct ControlPlayer {
    media: Arc<dyn MediaEngine>,
    audio: Arc<dyn AudioOutput>,
    path: PathBuf,
    /// Where the sound comes from, and how long the picture and the sound
    /// are.
    sound: PathBuf,
    picture_duration: Duration,
    sound_duration: Duration,
    duration: Duration,
    size: FrameSize,
    playing: Option<Playing>,
    /// Where playing got to.
    position: Duration,
    error: Option<Text>,
}

impl ControlPlayer {
    fn new(
        media: Arc<dyn MediaEngine>,
        audio: Arc<dyn AudioOutput>,
        path: PathBuf,
        duration: Duration,
        size: (u32, u32),
    ) -> ControlPlayer {
        Self::with_sound(media, audio, path.clone(), path, (duration, duration), size)
    }

    /// Plays the picture of `path` with the sound of `sound`, each as long
    /// as given.
    pub(crate) fn with_sound(
        media: Arc<dyn MediaEngine>,
        audio: Arc<dyn AudioOutput>,
        path: PathBuf,
        sound: PathBuf,
        (picture_duration, sound_duration): (Duration, Duration),
        (width, height): (u32, u32),
    ) -> ControlPlayer {
        let scale = (f64::from(PLAYER_LONG_SIDE) / f64::from(width.max(height).max(1))).min(1.0);
        let side = |side: u32| (((f64::from(side) * scale) as u32) & !1).max(2);
        let mut player = ControlPlayer {
            media,
            audio,
            path,
            sound,
            picture_duration,
            sound_duration,
            duration: picture_duration.max(sound_duration),
            size: FrameSize::new(side(width), side(height)),
            playing: None,
            position: Duration::ZERO,
            error: None,
        };
        player.play();
        player
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size(&self) -> FrameSize {
        self.size
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn position(&self) -> Duration {
        self.position
    }

    pub fn error(&self) -> Option<&Text> {
        self.error.as_ref()
    }

    pub fn is_playing(&self) -> bool {
        self.playing.is_some()
    }

    /// Whether `tick` has work: playing.
    pub fn needs_ticks(&self) -> bool {
        self.playing.is_some()
    }

    /// The control video as a plan: its picture, whole, and its sound.
    fn plan(&self) -> RenderPlan {
        RenderPlan {
            video: vec![VideoClip {
                source: ClipSource::Video(self.path.clone()),
                start: Duration::ZERO,
                duration: self.picture_duration,
                framing: Framing::Fit,
                transition: None,
                motion: None,
            }],
            audio: vec![AudioTrack {
                clips: vec![AudioClip {
                    source: self.sound.clone(),
                    start: Duration::ZERO,
                    duration: self.sound_duration,
                    at: Duration::ZERO,
                    gain_db: 0.0,
                    fade_in: Duration::ZERO,
                    fade_out: Duration::ZERO,
                    skipped: Duration::ZERO,
                }],
                gain_db: 0.0,
                duck: None,
            }],
            captions: None,
            skipped: Duration::ZERO,
        }
    }

    /// Plays from the start.
    pub fn play(&mut self) {
        if self.playing.is_some() {
            return;
        }
        self.error = None;
        self.position = Duration::ZERO;
        let plan = self.plan();
        let frames = match self
            .media
            .preview(&plan, Duration::ZERO, self.size, (FPS, 1))
        {
            Ok(frames) => frames,
            Err(_) => {
                self.error = Some(Text::ControlCannotPlay);
                return;
            }
        };
        // Without sound it still plays, on the wall clock.
        let audio = self
            .media
            .preview_audio(&plan, Duration::ZERO)
            .ok()
            .and_then(|stream| self.audio.stream(stream).ok());
        if audio.is_none() {
            self.error = Some(Text::EditorNoSound);
        }
        self.playing = Some(Playing {
            frames,
            pending: None,
            audio,
            asked_at: Instant::now(),
            started_at: None,
        });
    }

    /// Stops playing; the sound and ffmpeg stop with it.
    pub fn stop(&mut self) {
        self.playing = None;
    }

    pub fn toggle(&mut self) {
        if self.is_playing() {
            self.stop();
        } else {
            self.play();
        }
    }

    /// Moves playing on: call it every frame while `needs_ticks`. Returns
    /// a new picture to show, if there is one.
    pub fn tick(&mut self, now: Instant) -> Option<VideoFrame> {
        let duration = self.duration;
        let playing = self.playing.as_mut()?;
        if playing.pending.is_none() {
            match playing.frames.poll_frame() {
                FramePoll::Ready(frame) => playing.pending = Some(frame),
                FramePoll::Waiting => {}
                FramePoll::Ended if playing.started_at.is_none() => {
                    self.playing = None;
                    self.error = Some(Text::ControlCannotPlay);
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
            Some(audio) if !audio.has_ended() => audio.position(),
            _ => now.duration_since(started_at),
        };
        let time = elapsed.min(duration);
        self.position = time;
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
        if time >= duration {
            self.playing = None;
        }
        newest
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::Ordering;

    use bardo_domain::{BoundaryMove, CONTROL_AUDIO_BITRATE, TakeFlag};

    use super::*;
    use crate::CaptureVoiceChoice;
    use crate::captures::tests::{Setup, setup};
    use crate::narrations::tests::project;

    /// A landscape recording at the smallest size a capture may have, so
    /// drawing its frames stays quick.
    pub(crate) const SIZE: (u32, u32) = (1280, 720);

    /// A capture of two takes, imported.
    pub(crate) fn imported(s: &Setup) -> (VideoProjectId, Capture) {
        // 600 ms a character: long enough for two takes.
        *s.h.aligner.step.lock().unwrap() = Some(Duration::from_millis(600));
        let project = project(&s.app);
        let (_, capture, _) = s.imported_at(project.id, CaptureVoiceChoice::Persona, SIZE);
        assert_eq!(capture.takes.as_slice().len(), 2);
        (project.id, capture)
    }

    /// Waits until no analysis or control video job is under way and every
    /// take's control video is built, or stopped.
    pub(crate) fn settled(app: &Bardo, project: VideoProjectId) -> Vec<ControlVideoView> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let busy = app.jobs().into_iter().any(|job| {
                matches!(job.kind(), JobKind::TakeAnalysis | JobKind::ControlVideo)
                    && job.state().is_active()
            });
            let views = app.take_controls(project).unwrap();
            // A job queues the build: the queue reads it on its next tick.
            let waiting = views.iter().any(|view| {
                view.status.is_active()
                    || matches!(
                        view.status,
                        ControlStatus::Waiting | ControlStatus::NotBuilt
                    )
            });
            if !busy && !waiting {
                return views;
            }
            assert!(
                Instant::now() < deadline,
                "the control videos never settled: {views:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn calls(s: &Setup) -> Vec<bardo_media_fake::ControlCall> {
        s.h.media.controls.lock().unwrap().clone()
    }

    mod bardo_media_fake {
        pub(crate) use crate::editor::testing::ControlCall;
    }

    #[test]
    fn each_take_is_cut_from_the_recording_with_its_voice_once_its_face_is_analyzed() {
        let s = setup(13);
        let (project, capture) = imported(&s);
        let views = settled(&s.app, project);
        let calls = calls(&s);
        let jobs: Vec<_> = s
            .app
            .jobs()
            .into_iter()
            .filter(|job| job.kind() == JobKind::ControlVideo)
            .map(|job| (job.state(), job.failure().cloned()))
            .collect();
        assert_eq!(calls.len(), 2, "{views:?} {jobs:?}");
        for (index, view) in views.iter().enumerate() {
            let take = &capture.takes.as_slice()[index];
            assert_eq!(view.mode, TakeMode::Recording);
            assert_eq!(view.outbound, Outbound::Recording);
            assert_eq!(view.status, ControlStatus::Ready { approved: false });
            assert_eq!(view.block, None);
            let built = view.built.as_ref().unwrap();
            let call = calls
                .iter()
                .find(|call| call.destination == s.app.control_video_path(built))
                .unwrap();
            assert_eq!(
                call.recording.as_deref(),
                Some(s.h.files.path(project, &capture.video_file).as_path())
            );
            // Its stretch of the narration, which the capture's speech
            // became.
            assert_eq!(call.voice.from, take.start);
            assert_eq!(call.voice.length, take.end - take.start);
            assert!(
                call.voice
                    .path
                    .ends_with(crate::narrations::audio_file(capture.narration))
            );
            // As recorded: its own size and frame rate, H.264 in budget.
            assert_eq!(call.output.size, SIZE);
            assert_eq!(call.output.fps_milli, 29_970);
            assert_eq!(call.output.encoder, VideoEncoder::OpenH264);
            assert_eq!(call.output.keyframe_interval, 60);
            let bits = u64::from(call.output.bitrate + CONTROL_AUDIO_BITRATE);
            assert!(bits * (take.end - take.start).as_secs() / 8 < MAX_CONTROL_BYTES);
            assert_eq!(built.encoder, "libopenh264");
            assert_eq!(built.uncovered, 0);
            assert!(
                built
                    .file
                    .starts_with(&format!("control-{}-00{}-r", capture.id, index + 1))
            );
        }
    }

    #[test]
    fn mesh_mode_draws_every_analyzed_frame_over_the_room() {
        let s = setup(13);
        let (project, capture) = imported(&s);
        let recorded = settled(&s.app, project)[0].built.clone().unwrap();
        let before = calls(&s).len();

        // Mesh and Depth are modes of the video edit model.
        s.app
            .set_take_model(project, 0, Some(crate::testing::FakeTakes::edit()))
            .unwrap();
        let views = settled(&s.app, project);
        let calls = calls(&s);
        assert_eq!(calls.len(), before + 1);
        let call = calls.last().unwrap();
        assert_eq!(call.recording, None, "drawn, not cut");
        let take = &capture.takes.as_slice()[0];
        let frames = take_frames(take.end - take.start, 29_970);
        assert_eq!(call.frames, frames);
        assert_eq!(call.output.size, SIZE);
        let built = views[0].built.as_ref().unwrap();
        assert_eq!((built.frames, built.uncovered), (frames, 0));
        assert_eq!(views[0].outbound, Outbound::Mesh);
        // The room was decoded at the drawing's size and rate.
        assert!(s.h.media.control_decoded.lock().unwrap().contains(&(
            take.start,
            take.end - take.start,
            29_970,
            SIZE
        )));
        // The Recording build it replaced is gone.
        assert!(!s.h.files.exists(project, &recorded.file));
        assert!(s.h.files.exists(project, &built.file));
    }

    #[test]
    fn mesh_frames_with_no_face_near_enough_are_counted_and_block_approval() {
        let s = setup(13);
        *s.h.vision.seen.faces.lock().unwrap() = Some(0);
        let (project, _) = imported(&s);
        settled(&s.app, project);
        // Mesh and Depth are modes of the video edit model.
        s.app
            .set_take_model(project, 1, Some(crate::testing::FakeTakes::edit()))
            .unwrap();
        let views = settled(&s.app, project);
        let built = views[1].built.as_ref().unwrap();
        assert!(built.frames > 0);
        assert_eq!(built.uncovered, built.frames);
        // They would show the face as recorded.
        assert_eq!(views[1].block, Some(ApprovalBlock::Uncovered));
        assert!(matches!(
            s.app.approve_control_video(project, 1),
            Err(CaptureError::NotApprovable(ApprovalBlock::Uncovered))
        ));

        // A depth figure shows no face as recorded.
        s.app.set_take_mode(project, 1, TakeMode::Depth).unwrap();
        let views = settled(&s.app, project);
        assert_eq!(views[1].built.as_ref().unwrap().uncovered, 0);
        assert_eq!(views[1].block, None);
    }

    #[test]
    fn an_approval_holds_until_the_take_changes() {
        let s = setup(13);
        let (project, _) = imported(&s);
        settled(&s.app, project);
        s.app.approve_control_video(project, 0).unwrap();
        let views = s.app.take_controls(project).unwrap();
        assert!(views[0].is_approved());
        assert!(!views[1].is_approved());
        assert!(!views[0].can_approve());

        // Moving the cut changes both takes: their analyses and builds run
        // again, and the approval is gone with the build it was for.
        s.app
            .move_take_boundary(project, 0, BoundaryMove::Later)
            .unwrap_or_else(|_| {
                s.app
                    .move_take_boundary(project, 0, BoundaryMove::Earlier)
                    .unwrap()
            });
        let views = settled(&s.app, project);
        assert_eq!(views[0].status, ControlStatus::Ready { approved: false });
        assert!(views[0].approval_lost);
        assert!(views[0].can_approve());

        // So does another mode, and approving the new build holds.
        s.app.approve_control_video(project, 0).unwrap();
        // Mesh and Depth are modes of the video edit model.
        s.app
            .set_take_model(project, 0, Some(crate::testing::FakeTakes::edit()))
            .unwrap();
        let views = s.app.take_controls(project).unwrap();
        assert!(!views[0].is_approved());
        let views = settled(&s.app, project);
        assert_eq!(views[0].status, ControlStatus::Ready { approved: false });
        assert!(views[0].approval_lost);
    }

    #[test]
    fn a_take_with_another_face_or_out_of_length_cannot_be_approved() {
        let s = setup(13);
        *s.h.vision.seen.faces.lock().unwrap() = Some(2);
        let (project, capture) = imported(&s);
        let views = settled(&s.app, project);
        assert_eq!(views[0].block, Some(ApprovalBlock::SeveralFaces));
        assert!(!views[0].can_approve());
        assert!(matches!(
            s.app.approve_control_video(project, 0),
            Err(CaptureError::NotApprovable(ApprovalBlock::SeveralFaces))
        ));

        // A take over the longest a take model takes.
        let take = &capture.takes.as_slice()[1];
        let long = Take {
            end: take.start + Duration::from_secs(31),
            ..take.clone()
        };
        assert_eq!(
            approval_block(&long, None, 0),
            Some(ApprovalBlock::Length(TakeFlag::TooLong))
        );
        assert_eq!(
            CaptureError::NotApprovable(ApprovalBlock::Length(TakeFlag::TooLong)).message(),
            Text::ControlLengthBlocked
        );
    }

    /// A PNG of `width` × `height`, enough for its size to read.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    #[test]
    fn depth_over_an_image_never_decodes_the_recording() {
        let s = setup(13);
        let (project, capture) = imported(&s);
        settled(&s.app, project);
        let picture = s._folders.path().join("Beach.PNG");
        std::fs::write(&picture, png(640, 360)).unwrap();
        s.app
            .set_take_set(
                project,
                0,
                SetChoice::Picture(SetPicture::read(&picture).unwrap()),
            )
            .unwrap();
        let set = s.app.take_controls(project).unwrap()[0].set.clone();
        let TakeSet::Image { file, from } = &set else {
            panic!("{set:?}");
        };
        assert_eq!(*from, SetSource::Imported);
        assert!(file.starts_with("set-") && file.ends_with(".png"), "{file}");
        assert_eq!(s.h.files.read(project, file).unwrap(), png(640, 360));
        // Not in Depth mode, the set changes nothing.
        assert_eq!(
            s.app.take_controls(project).unwrap()[0].outbound,
            Outbound::Recording
        );

        // Depth is a mode of the video edit model.
        s.app
            .set_take_model(project, 0, Some(crate::testing::FakeTakes::edit()))
            .unwrap();
        settled(&s.app, project);
        s.h.media.control_decoded.lock().unwrap().clear();
        s.app.set_take_mode(project, 0, TakeMode::Depth).unwrap();
        let views = settled(&s.app, project);
        assert_eq!(views[0].outbound, Outbound::DepthOverImage);
        assert!(!views[0].outbound.sends_set_frame());
        let built = views[0].built.as_ref().unwrap();
        let take = &capture.takes.as_slice()[0];
        assert_eq!(built.frames, take_frames(take.end - take.start, 29_970));
        assert!(
            s.h.media.control_decoded.lock().unwrap().is_empty(),
            "the recording was decoded"
        );
        assert_eq!(
            s.h.media.covered.lock().unwrap().as_slice(),
            [s.h.files.path(project, file)]
        );
        // The figure is drawn: the last frame is no longer the gray set
        // all over.
        let made = calls(&s);
        let last = &made.last().unwrap().last;
        assert!(last.chunks(4).any(|pixel| pixel[..3] != [128, 128, 128]));

        // Back to the room: the copied image goes, and the room is drawn.
        s.app.set_take_set(project, 0, SetChoice::Room).unwrap();
        assert!(!s.h.files.exists(project, file));
        let views = settled(&s.app, project);
        assert_eq!(views[0].outbound, Outbound::DepthOverRoom);
        assert!(views[0].outbound.sends_set_frame());
        assert!(!s.h.media.control_decoded.lock().unwrap().is_empty());
    }

    #[test]
    fn a_set_can_be_a_scene_image_or_a_picture_that_reads() {
        let s = setup(13);
        let (project, _) = imported(&s);
        settled(&s.app, project);
        let text = s._folders.path().join("notes.png");
        std::fs::write(&text, b"not a picture").unwrap();
        let gif = s._folders.path().join("set.gif");
        std::fs::write(&gif, png(10, 10)).unwrap();
        let huge = s._folders.path().join("huge.png");
        std::fs::write(&huge, png(20_000, 10_000)).unwrap();
        for path in [text, s._folders.path().join("gone.png"), gif, huge] {
            assert!(
                matches!(SetPicture::read(&path), Err(CaptureError::SetUnreadable)),
                "{path:?}"
            );
        }
        assert!(matches!(
            s.app
                .set_take_set(project, 1, SetChoice::Scene("scene-1.png".into())),
            Err(CaptureError::SetUnreadable)
        ));
        assert!(s.app.set_scenes(project).unwrap().is_empty());
        assert_eq!(s.app.take_controls(project).unwrap()[1].set, TakeSet::Room);
    }

    #[test]
    fn removing_the_capture_removes_its_control_videos_and_sets() {
        let s = setup(13);
        let (project, _) = imported(&s);
        let views = settled(&s.app, project);
        let picture = s._folders.path().join("set.jpg");
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        jpeg.extend(360u16.to_be_bytes());
        jpeg.extend(640u16.to_be_bytes());
        std::fs::write(&picture, jpeg).unwrap();
        s.app
            .set_take_set(
                project,
                1,
                SetChoice::Picture(SetPicture::read(&picture).unwrap()),
            )
            .unwrap();
        let set = s.app.take_controls(project).unwrap()[1].set.clone();
        let image = set.image().unwrap().to_owned();
        assert!(image.ends_with(".jpg"));
        let files: Vec<String> = views
            .iter()
            .map(|view| view.built.as_ref().unwrap().file.clone())
            .collect();

        s.app.remove_capture(project).unwrap();
        for file in files.iter().chain([&image]) {
            assert!(!s.h.files.exists(project, file), "{file}");
        }
        assert!(s.h.db.control_videos(project).unwrap().is_empty());
    }

    #[test]
    fn a_stopped_build_keeps_nothing_and_builds_again() {
        let s = setup(13);
        s.h.media.control_hold.store(true, Ordering::SeqCst);
        let (project, _) = imported(&s);
        let deadline = Instant::now() + Duration::from_secs(20);
        let job = loop {
            let views = s.app.take_controls(project).unwrap();
            if let Some(job) = views[0]
                .job
                .filter(|_| matches!(views[0].status, ControlStatus::Building { .. }))
            {
                break job;
            }
            assert!(Instant::now() < deadline, "{views:?}");
            std::thread::sleep(Duration::from_millis(5));
        };
        s.app.cancel_job(job).unwrap();
        while s.app.job_runs_here(job) {
            std::thread::sleep(Duration::from_millis(2));
        }
        s.h.media.control_hold.store(false, Ordering::SeqCst);
        let views = settled(&s.app, project);
        assert_eq!(views[0].status, ControlStatus::Cancelled);
        assert!(views[0].built.is_none());
        assert!(
            s.h.db
                .control_videos(project)
                .unwrap()
                .iter()
                .all(|c| c.position != 0)
        );

        s.app.build_control_video(project, 0).unwrap();
        let views = settled(&s.app, project);
        assert_eq!(views[0].status, ControlStatus::Ready { approved: false });
    }

    #[test]
    fn a_failing_hardware_encoder_falls_back_to_software() {
        let s = setup(13);
        *s.h.media.encoders.lock().unwrap() = Some(bardo_media::ffmpeg::Encoders {
            working: vec![VideoEncoder::Nvenc, VideoEncoder::OpenH264],
        });
        s.h.media
            .broken_encoders
            .lock()
            .unwrap()
            .push(VideoEncoder::Nvenc);
        let (project, _) = imported(&s);
        let views = settled(&s.app, project);
        assert_eq!(views[0].built.as_ref().unwrap().encoder, "libopenh264");
    }

    #[test]
    fn keyframes_come_every_two_seconds() {
        assert_eq!(keyframe_interval(29_970), 60);
        assert_eq!(keyframe_interval(25_000), 50);
        assert_eq!(keyframe_interval(59_940), 120);
        assert_eq!(keyframe_interval(0), 1);
    }
}
