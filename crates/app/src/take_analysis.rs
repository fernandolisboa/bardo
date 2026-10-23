//! Take analysis (#190, docs/spec/performance-capture.md, ADR-0024): the
//! vision models run on the computer over each take of a capture. The face
//! mesh runs on every take as soon as the takes exist, and the depth map
//! and person mask once the user switches a take to Depth mode. It costs
//! nothing and nothing leaves the computer.
//!
//! Each part is a job of its own per take. ffmpeg decodes the take's frames
//! at [`analysis_fps`], and the job works through them a chunk of
//! [`ANALYSIS_CHUNK_FRAMES`] at a time: after each chunk its output and the
//! models' state are written in the project folder, so a cancelled or
//! interrupted job resumes at the last finished chunk.
//!
//! - The face mesh ends as the take's `face_landmarks.json`.
//! - The depth is mapped to 16 bits with a scale set from the take's first
//!   depth, then, once every frame is in, normalized over the whole take
//!   between its percentiles. It is saved with the person mask as FFV1 in
//!   Matroska. One depth job runs at a time: Video Depth Anything needs
//!   about 4.5 GB of memory.
//!
//! An analysis names the capture and the take's revision it was made from.
//! Moving a cut gives both takes a new revision, which makes theirs stale
//! and queues them again; a job whose take moved on, or whose part the
//! mode no longer needs, stops at its next chunk and removes what it made.

use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant, SystemTime};

use bardo_domain::{
    ANALYSIS_CHUNK_FRAMES, AnalysisPart, Capture, CaptureId, CaptureRepository, DepthHistogram,
    DepthMap, DepthScale, Device, FaceFrame, FaceLandmarks, FaceSource, Job, JobFailure,
    JobFailureKind, JobKind, JobRepository, JobState, PersonMask, ProfileId, Progress,
    ProjectFiles, REGION_COUNT_EVERY, RgbFrame, TakeAnalysis, TakeChecks, TakeMode, VideoProjectId,
    VisionError, VisionModels, analysis_fps, distinct_faces, take_frame_time, take_frames,
};
use bardo_media::MediaEngine;
use bardo_media::ffmpeg::{GrayDepth, GraySink, MediaError, analysis_size};
use serde::{Deserialize, Serialize};

use crate::Bardo;
use crate::captures::CaptureError;
use crate::jobs::{JobContext, JobHandler};

/// How long a depth analysis waits while another one runs.
const DEPTH_WAIT: Duration = Duration::from_secs(5);

/// The share of a depth job's progress its first pass takes; joining the
/// chunks into the take's files is the rest.
const DEPTH_FIRST_PASS_PERMILLE: u64 = 900;

/// A take analysis job's payload: the take as it was when it was queued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AnalysisPayload {
    pub(crate) project: String,
    pub(crate) capture: String,
    pub(crate) position: u32,
    pub(crate) revision: u32,
    /// [`AnalysisPart::code`].
    pub(crate) part: String,
    /// The take's frames at the analysis frame rate, so what an unfinished
    /// job left is known without the take.
    pub(crate) frames: u32,
}

impl AnalysisPayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("an analysis payload serializes")
    }

    pub(crate) fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid take analysis payload: {e}")))
    }

    fn work(&self) -> Result<Work, JobFailure> {
        let id = |text: &str| uuid::Uuid::parse_str(text).map_err(unexpected);
        Ok(Work {
            project: VideoProjectId::from(id(&self.project)?),
            capture: CaptureId::from(id(&self.capture)?),
            position: self.position,
            revision: self.revision,
            part: AnalysisPart::from_code(&self.part)
                .ok_or_else(|| unexpected(format!("unknown analysis part {}", self.part)))?,
            frames: self.frames,
        })
    }
}

/// What a running analysis saves as its checkpoint, for the Capture stage:
/// where it runs and how fast.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AnalysisCheckpoint {
    /// On the graphics card; otherwise the processor.
    pub(crate) gpu: bool,
    /// Frames finished.
    pub(crate) done: u32,
    /// Milliseconds a frame took on this run, once one finished.
    pub(crate) ms_per_frame: u32,
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

fn vision_failure(error: VisionError) -> JobFailure {
    match error {
        VisionError::Missing(_) | VisionError::Failed(_) => {
            JobFailure::new(JobFailureKind::VisionModels, error.to_string())
        }
        VisionError::State(_) => unexpected(error),
    }
}

fn media_failure(error: MediaError) -> JobFailure {
    JobFailure::new(JobFailureKind::Media, error.to_string())
}

/// One take's part, as a job works on it.
#[derive(Debug, Clone, Copy)]
struct Work {
    project: VideoProjectId,
    capture: CaptureId,
    position: u32,
    revision: u32,
    part: AnalysisPart,
    frames: u32,
}

impl Work {
    fn chunks(&self) -> u32 {
        self.frames.div_ceil(ANALYSIS_CHUNK_FRAMES)
    }

    fn base(&self) -> String {
        file_base(self.capture, self.position, self.revision)
    }
}

/// The take's files are named after the capture, the take (counted from 1)
/// and its revision.
fn file_base(capture: CaptureId, position: u32, revision: u32) -> String {
    format!("take-{capture}-{:03}-r{revision}", position + 1)
}

/// The take's `face_landmarks.json`.
pub(crate) fn face_file(capture: CaptureId, position: u32, revision: u32) -> String {
    format!(
        "{}.face_landmarks.json",
        file_base(capture, position, revision)
    )
}

/// The take's depth, 16-bit gray, near light.
pub(crate) fn depth_file(capture: CaptureId, position: u32, revision: u32) -> String {
    format!("{}.depth.mkv", file_base(capture, position, revision))
}

/// The take's person mask, 8-bit gray.
pub(crate) fn mask_file(capture: CaptureId, position: u32, revision: u32) -> String {
    format!("{}.mask.mkv", file_base(capture, position, revision))
}

fn state_file(work: &Work) -> String {
    format!("{}.{}.state", work.base(), work.part.code())
}

fn face_chunk(work: &Work, chunk: u32) -> String {
    format!("{}.face.{chunk:04}.jsonl", work.base())
}

fn depth_chunk(work: &Work, chunk: u32) -> String {
    format!("{}.depth.{chunk:04}.mkv", work.base())
}

fn mask_chunk(work: &Work, chunk: u32) -> String {
    format!("{}.mask.{chunk:04}.mkv", work.base())
}

/// What an unfinished job of `work` keeps between attempts.
fn leftovers(work: &Work) -> Vec<String> {
    let mut files = vec![state_file(work)];
    for chunk in 0..work.chunks() {
        match work.part {
            AnalysisPart::Face => files.push(face_chunk(work, chunk)),
            AnalysisPart::Depth => {
                files.push(depth_chunk(work, chunk));
                files.push(mask_chunk(work, chunk));
            }
        }
    }
    files
}

/// The finished files of a take's part at `revision`.
fn finished_files(
    capture: CaptureId,
    position: u32,
    revision: u32,
    part: AnalysisPart,
) -> Vec<String> {
    match part {
        AnalysisPart::Face => vec![face_file(capture, position, revision)],
        AnalysisPart::Depth => vec![
            depth_file(capture, position, revision),
            mask_file(capture, position, revision),
        ],
    }
}

/// Removes every analysis file of `capture`'s takes, at any revision they
/// had: the capture is going. One that will not go (open in a player)
/// stays.
pub(crate) fn remove_analysis_files(files: &dyn ProjectFiles, capture: &Capture) {
    for (position, take) in capture.takes.as_slice().iter().enumerate() {
        for revision in 1..=take.revision {
            for part in AnalysisPart::ALL {
                for file in finished_files(capture.id, position as u32, revision, part) {
                    let _ = files.remove(capture.project, &file);
                }
            }
        }
    }
}

/// The job that analyzes `part` of `capture`'s take at `index` as it is now.
pub(crate) fn analysis_job(
    owner: ProfileId,
    capture: &Capture,
    index: usize,
    part: AnalysisPart,
) -> Option<Job> {
    let take = capture.takes.as_slice().get(index)?;
    let fps = analysis_fps(capture.video.fps_milli);
    let payload = AnalysisPayload {
        project: capture.project.to_string(),
        capture: capture.id.to_string(),
        position: index as u32,
        revision: take.revision,
        part: part.code().to_owned(),
        frames: take_frames(take.end.saturating_sub(take.start), fps),
    };
    Some(Job::new(owner, JobKind::TakeAnalysis, payload.to_json()))
}

/// The face mesh jobs of every take of a capture just imported.
pub(crate) fn queue_face_analyses(owner: ProfileId, capture: &Capture, jobs: &dyn JobRepository) {
    for index in 0..capture.takes.as_slice().len() {
        let Some(job) = analysis_job(owner, capture, index, AnalysisPart::Face) else {
            continue;
        };
        if let Err(error) = jobs.save(&job) {
            // The take shows no analysis, and Analyze starts it.
            tracing::warn!(%error, take = index + 1, "could not queue a take's face mesh");
        }
    }
}

/// The take a job analyzes, as the capture has it now.
struct Current {
    project: VideoProjectId,
    video_file: String,
    width: u32,
    height: u32,
    /// The analysis frame rate.
    fps: u32,
    start: Duration,
    end: Duration,
}

/// A run's state, written after each chunk: a JSON header, then the
/// models' snapshots.
const STATE_MAGIC: &[u8] = b"bardo-take-analysis/1\n";

fn pack(header: &impl Serialize, blobs: &[&[u8]]) -> Vec<u8> {
    let header = serde_json::to_vec(header).expect("a state header serializes");
    let mut out = STATE_MAGIC.to_vec();
    for section in std::iter::once(header.as_slice()).chain(blobs.iter().copied()) {
        out.extend_from_slice(&(section.len() as u64).to_le_bytes());
        out.extend_from_slice(section);
    }
    out
}

fn unpack(bytes: &[u8]) -> Option<Vec<&[u8]>> {
    let mut rest = bytes.strip_prefix(STATE_MAGIC)?;
    let mut sections = Vec::new();
    while !rest.is_empty() {
        let (length, tail) = rest.split_first_chunk::<8>()?;
        let length = usize::try_from(u64::from_le_bytes(*length)).ok()?;
        if tail.len() < length {
            return None;
        }
        let (section, tail) = tail.split_at(length);
        sections.push(section);
        rest = tail;
    }
    Some(sections)
}

/// [`TakeChecks`] as a state file keeps them.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct ChecksRecord([u32; 6]);

impl From<TakeChecks> for ChecksRecord {
    fn from(c: TakeChecks) -> Self {
        ChecksRecord([
            c.frames,
            c.no_face,
            c.several_faces,
            c.small_face,
            c.turned,
            c.mask_jumps,
        ])
    }
}

impl From<ChecksRecord> for TakeChecks {
    fn from(ChecksRecord(c): ChecksRecord) -> Self {
        TakeChecks {
            frames: c[0],
            no_face: c[1],
            several_faces: c[2],
            small_face: c[3],
            turned: c[4],
            mask_jumps: c[5],
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct FaceState {
    /// Chunks finished.
    chunks: u32,
    checks: ChecksRecord,
}

#[derive(Debug, Serialize, Deserialize)]
struct DepthState {
    chunks: u32,
    checks: ChecksRecord,
    /// The 16-bit scale, from the first depth.
    scale: Option<[f32; 2]>,
    /// The last mask's area, for the next frame's check.
    area: Option<f32>,
    /// Depth frames each finished chunk holds (a frame's depth may come
    /// out in a later chunk).
    depth_frames: Vec<u32>,
}

/// The frames of a take from one frame on, one at a time; past the last
/// frame ffmpeg gives (rounding at the end of the take), the last one again.
struct Frames {
    source: Box<dyn bardo_media::ffmpeg::FrameSource>,
    last: Option<RgbFrame>,
    size: (u32, u32),
}

impl Frames {
    fn next(&mut self) -> Result<&RgbFrame, JobFailure> {
        if let Some(frame) = self.source.next_frame().map_err(media_failure)? {
            self.last = Some(frame);
        }
        Ok(self
            .last
            .get_or_insert_with(|| RgbFrame::black(self.size.0, self.size.1)))
    }
}

/// Runs take analysis jobs.
pub(crate) struct TakeAnalysisHandler {
    pub(crate) captures: Arc<dyn CaptureRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) vision: Arc<dyn VisionModels>,
    /// Held while a depth analysis runs: one at a time.
    pub(crate) depth_slot: Mutex<()>,
}

impl JobHandler for TakeAnalysisHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let work = AnalysisPayload::parse(payload)?.work()?;
        let Some(take) = self.current(&work)? else {
            self.remove_leftovers(&work);
            return Ok(());
        };
        match work.part {
            AnalysisPart::Face => self.face(&work, &take, cx),
            AnalysisPart::Depth => {
                let _slot = match self.depth_slot.try_lock() {
                    Ok(slot) => slot,
                    Err(TryLockError::Poisoned(slot)) => slot.into_inner(),
                    Err(TryLockError::WouldBlock) => {
                        cx.defer(SystemTime::now() + DEPTH_WAIT);
                        return Ok(());
                    }
                };
                self.depth(&work, &take, cx)
            }
        }
    }
}

/// Measures a run's speed for its checkpoint.
struct Pace {
    started: Instant,
    from: u32,
    gpu: bool,
}

impl Pace {
    fn checkpoint(&self, done: u32) -> String {
        let frames = done.saturating_sub(self.from);
        let ms_per_frame = if frames == 0 {
            0
        } else {
            (self.started.elapsed().as_millis() / u128::from(frames)) as u32
        };
        serde_json::to_string(&AnalysisCheckpoint {
            gpu: self.gpu,
            done,
            ms_per_frame,
        })
        .expect("a checkpoint serializes")
    }
}

impl TakeAnalysisHandler {
    /// The take `work` analyzes, while the project's capture, the take's
    /// revision and its mode still call for it.
    fn current(&self, work: &Work) -> Result<Option<Current>, JobFailure> {
        let Some(capture) = self
            .captures
            .capture(work.project)
            .map_err(unexpected)?
            .filter(|capture| capture.id == work.capture)
        else {
            return Ok(None);
        };
        let Some(take) = capture
            .takes
            .as_slice()
            .get(work.position as usize)
            .filter(|take| {
                take.revision == work.revision && take.mode.parts().contains(&work.part)
            })
        else {
            return Ok(None);
        };
        Ok(Some(Current {
            project: work.project,
            video_file: capture.video_file.clone(),
            width: capture.video.width,
            height: capture.video.height,
            fps: analysis_fps(capture.video.fps_milli),
            start: take.start,
            end: take.end,
        }))
    }

    fn remove_leftovers(&self, work: &Work) {
        for file in leftovers(work) {
            let _ = self.files.remove(work.project, &file);
        }
    }

    /// The take's frames from frame `from`.
    fn frames(&self, take: &Current, from: u32) -> Result<Frames, JobFailure> {
        let start = take.start + take_frame_time(from, take.fps);
        let source = self
            .media
            .rgb_frames(
                &self.files.path(take.project, &take.video_file),
                start,
                take.end.saturating_sub(start),
                take.fps,
                (take.width, take.height),
            )
            .map_err(media_failure)?;
        Ok(Frames {
            source,
            last: None,
            size: analysis_size(take.width, take.height),
        })
    }

    fn checkpoint(
        cx: &mut JobContext,
        pace: &Pace,
        done: u32,
        progress: Progress,
    ) -> Result<(), JobFailure> {
        cx.save_checkpoint(pace.checkpoint(done), progress)
            .map_err(unexpected)
    }

    /// Keeps the finished analysis and removes what the run and the take's
    /// earlier revisions left; when the take moved on meanwhile, removes
    /// the finished files too.
    fn finish(&self, work: &Work, analysis: TakeAnalysis) -> Result<(), JobFailure> {
        let saved = self.captures.save_analysis(&analysis).map_err(unexpected)?;
        self.remove_leftovers(work);
        let revisions = if saved {
            1..work.revision
        } else {
            work.revision..work.revision + 1
        };
        for revision in revisions {
            for file in finished_files(work.capture, work.position, revision, work.part) {
                let _ = self.files.remove(work.project, &file);
            }
        }
        Ok(())
    }

    fn face(&self, work: &Work, take: &Current, cx: &mut JobContext) -> Result<(), JobFailure> {
        let project = work.project;
        let mut tracker = self.vision.face_tracker().map_err(vision_failure)?;
        let mut segmenter = self.vision.person_segmenter().map_err(vision_failure)?;
        let mut checks = TakeChecks::default();
        let mut chunks_done = 0;
        if let Some((state, snapshot)) = self.read_state::<FaceState>(work, 1) {
            if tracker.restore(&snapshot[0]).is_ok() && state.chunks <= work.chunks() {
                chunks_done = state.chunks;
                checks = state.checks.into();
            } else {
                tracing::info!("a take's saved face state does not fit: starting it over");
                tracker = self.vision.face_tracker().map_err(vision_failure)?;
            }
        }
        let pace = Pace {
            started: Instant::now(),
            from: chunks_done * ANALYSIS_CHUNK_FRAMES,
            gpu: tracker.device().clone().slower(segmenter.device().clone()) == Device::Gpu,
        };
        let total = work.frames;
        Self::checkpoint(
            cx,
            &pace,
            pace.from.min(total),
            Progress::of(u64::from(pace.from.min(total)), u64::from(total)),
        )?;
        if chunks_done < work.chunks() {
            let mut frames = self.frames(take, chunks_done * ANALYSIS_CHUNK_FRAMES)?;
            // Faces are counted in each person region about once a second.
            let every = (take.fps.div_ceil(1_000) * REGION_COUNT_EVERY.as_secs() as u32).max(1);
            for chunk in chunks_done..work.chunks() {
                let first = chunk * ANALYSIS_CHUNK_FRAMES;
                let end = (first + ANALYSIS_CHUNK_FRAMES).min(total);
                let mut lines = Vec::with_capacity((end - first) as usize);
                for index in first..end {
                    if cx.should_stop() {
                        return Ok(());
                    }
                    let frame = frames.next()?;
                    let at = take_frame_time(index, take.fps);
                    let mut mask: Option<PersonMask> = None;
                    let seen = {
                        let mut head = || {
                            let found = segmenter.segment(frame)?;
                            let region = found
                                .head_regions(frame.width, frame.height)
                                .first()
                                .copied();
                            mask = Some(found);
                            Ok(region)
                        };
                        tracker
                            .track(frame, at, &mut head)
                            .map_err(vision_failure)?
                    };
                    let mut faces = seen.detected.unwrap_or(0) as usize;
                    if index % every == 0 {
                        let mask = match mask {
                            Some(mask) => mask,
                            None => segmenter.segment(frame).map_err(vision_failure)?,
                        };
                        let mut found = Vec::new();
                        for region in mask.head_regions(frame.width, frame.height) {
                            found
                                .extend(tracker.detect_in(frame, &region).map_err(vision_failure)?);
                        }
                        faces = faces.max(distinct_faces(&found));
                    }
                    checks.add_face_frame(seen.mesh.as_ref(), faces);
                    lines.push(
                        FaceFrame {
                            at,
                            faces: seen.mesh.into_iter().collect(),
                            detected: Some(faces as u32),
                        }
                        .to_json(),
                    );
                }
                self.files
                    .write(
                        project,
                        &face_chunk(work, chunk),
                        lines.join("\n").as_bytes(),
                    )
                    .map_err(unexpected)?;
                let state = FaceState {
                    chunks: chunk + 1,
                    checks: checks.into(),
                };
                self.files
                    .write(
                        project,
                        &state_file(work),
                        &pack(&state, &[&tracker.snapshot()]),
                    )
                    .map_err(unexpected)?;
                Self::checkpoint(
                    cx,
                    &pace,
                    end,
                    Progress::of(u64::from(end), u64::from(total)),
                )?;
                if self.current(work)?.is_none() {
                    self.remove_leftovers(work);
                    return Ok(());
                }
            }
        }

        let mut text = Vec::with_capacity(work.chunks() as usize);
        for chunk in 0..work.chunks() {
            let bytes = self
                .files
                .read(project, &face_chunk(work, chunk))
                .map_err(unexpected)?;
            text.push(String::from_utf8(bytes).map_err(unexpected)?);
        }
        let source = FaceSource {
            file: take.video_file.clone(),
            width: take.width,
            height: take.height,
            fps_milli: take.fps,
            frames: total,
            start: take.start,
            end: take.end,
        };
        let json = FaceLandmarks::json_from_frames(
            &source,
            tracker.model(),
            text.iter()
                .flat_map(|chunk| chunk.split('\n'))
                .filter(|line| !line.is_empty()),
        );
        self.files
            .write(
                project,
                &face_file(work.capture, work.position, work.revision),
                json.as_bytes(),
            )
            .map_err(unexpected)?;
        let device = tracker.device().clone().slower(segmenter.device().clone());
        self.finish(
            work,
            TakeAnalysis {
                project,
                capture: work.capture,
                position: work.position,
                revision: work.revision,
                part: AnalysisPart::Face,
                device,
                model: tracker.model().label(),
                checks,
                depth_range: None,
                finished_at: SystemTime::now(),
                job: Some(cx.id()),
            },
        )
    }

    fn depth(&self, work: &Work, take: &Current, cx: &mut JobContext) -> Result<(), JobFailure> {
        let project = work.project;
        let vertical = take.height > take.width;
        let mut estimator = self
            .vision
            .depth_estimator(vertical)
            .map_err(vision_failure)?;
        let mut segmenter = self.vision.person_segmenter().map_err(vision_failure)?;
        let size = estimator.size();
        let mut checks = TakeChecks::default();
        let mut histogram = DepthHistogram::default();
        let mut state = DepthState {
            chunks: 0,
            checks: checks.into(),
            scale: None,
            area: None,
            depth_frames: Vec::new(),
        };
        if let Some((saved, blobs)) = self.read_state::<DepthState>(work, 2) {
            let restored = saved.chunks <= work.chunks()
                && saved.depth_frames.len() == saved.chunks as usize
                && estimator.restore(&blobs[0]).is_ok();
            match DepthHistogram::from_bytes(&blobs[1]).filter(|_| restored) {
                Some(saved_histogram) => {
                    histogram = saved_histogram;
                    checks = saved.checks.into();
                    state = saved;
                }
                None => {
                    tracing::info!("a take's saved depth state does not fit: starting it over");
                    estimator = self
                        .vision
                        .depth_estimator(vertical)
                        .map_err(vision_failure)?;
                }
            }
        }
        let gpu = estimator
            .device()
            .clone()
            .slower(segmenter.device().clone())
            == Device::Gpu;
        let pace = Pace {
            started: Instant::now(),
            from: state.chunks * ANALYSIS_CHUNK_FRAMES,
            gpu,
        };
        let total = work.frames;
        let first_pass = |done: u32| {
            Progress::from_permille(
                (u64::from(done) * DEPTH_FIRST_PASS_PERMILLE / u64::from(total.max(1))) as u16,
            )
        };
        Self::checkpoint(
            cx,
            &pace,
            pace.from.min(total),
            first_pass(pace.from.min(total)),
        )?;

        // The first pass: each chunk's depth in 16 bits by a fixed scale,
        // and its masks.
        if state.chunks < work.chunks() {
            let mut frames = self.frames(take, state.chunks * ANALYSIS_CHUNK_FRAMES)?;
            for chunk in state.chunks..work.chunks() {
                let first = chunk * ANALYSIS_CHUNK_FRAMES;
                let end = (first + ANALYSIS_CHUNK_FRAMES).min(total);
                let mut depth_sink: Option<Box<dyn GraySink>> = None;
                let mut depth_count = 0;
                let mut mask_sink = self
                    .media
                    .gray_writer(
                        &self.files.path(project, &mask_chunk(work, chunk)),
                        size,
                        take.fps,
                        GrayDepth::Eight,
                    )
                    .map_err(media_failure)?;
                let mut write_depth = |maps: Vec<DepthMap>,
                                       sink: &mut Option<Box<dyn GraySink>>|
                 -> Result<(), JobFailure> {
                    for map in maps {
                        let [low, high] = *state.scale.get_or_insert_with(|| {
                            let scale = DepthScale::around(&map.data);
                            [scale.low, scale.high]
                        });
                        let scale = DepthScale { low, high };
                        let quantized: Vec<u16> = map
                            .data
                            .iter()
                            .map(|&value| scale.quantize(value))
                            .collect();
                        histogram.add(&quantized);
                        let bytes: Vec<u8> = quantized
                            .iter()
                            .flat_map(|value| value.to_le_bytes())
                            .collect();
                        if sink.is_none() {
                            *sink = Some(
                                self.media
                                    .gray_writer(
                                        &self.files.path(project, &depth_chunk(work, chunk)),
                                        size,
                                        take.fps,
                                        GrayDepth::Sixteen,
                                    )
                                    .map_err(media_failure)?,
                            );
                        }
                        sink.as_mut()
                            .expect("the depth sink was just made")
                            .write(&bytes)
                            .map_err(media_failure)?;
                        depth_count += 1;
                    }
                    Ok(())
                };
                for _ in first..end {
                    if cx.should_stop() {
                        return Ok(());
                    }
                    let frame = frames.next()?;
                    let maps = estimator.push(frame).map_err(vision_failure)?;
                    write_depth(maps, &mut depth_sink)?;
                    let mask = segmenter.segment(frame).map_err(vision_failure)?;
                    let area = mask.area();
                    checks.add_mask_frame(state.area, area);
                    state.area = Some(area);
                    mask_sink
                        .write(&mask.resized(size.0, size.1))
                        .map_err(media_failure)?;
                }
                if chunk + 1 == work.chunks() {
                    let maps = estimator.finish().map_err(vision_failure)?;
                    write_depth(maps, &mut depth_sink)?;
                }
                if let Some(sink) = depth_sink {
                    sink.finish().map_err(media_failure)?;
                }
                mask_sink.finish().map_err(media_failure)?;
                state.chunks = chunk + 1;
                state.checks = checks.into();
                state.depth_frames.push(depth_count);
                self.files
                    .write(
                        project,
                        &state_file(work),
                        &pack(&state, &[&estimator.snapshot(), &histogram.to_bytes()]),
                    )
                    .map_err(unexpected)?;
                Self::checkpoint(cx, &pace, end, first_pass(end))?;
                if self.current(work)?.is_none() {
                    self.remove_leftovers(work);
                    return Ok(());
                }
            }
        }

        // The second pass: the chunks joined into the take's two files,
        // the depth normalized over the whole take.
        let range = histogram.range();
        let depth_name = depth_file(work.capture, work.position, work.revision);
        let mask_name = mask_file(work.capture, work.position, work.revision);
        let mut depth_out = self
            .media
            .gray_writer(
                &self.files.path(project, &depth_name),
                size,
                take.fps,
                GrayDepth::Sixteen,
            )
            .map_err(media_failure)?;
        let mut mask_out = self
            .media
            .gray_writer(
                &self.files.path(project, &mask_name),
                size,
                take.fps,
                GrayDepth::Eight,
            )
            .map_err(media_failure)?;
        let mut written = 0;
        for chunk in 0..work.chunks() {
            if cx.should_stop() {
                return Ok(());
            }
            if state.depth_frames.get(chunk as usize).copied().unwrap_or(0) > 0 {
                let mut source = self
                    .media
                    .gray_frames(
                        &self.files.path(project, &depth_chunk(work, chunk)),
                        size,
                        GrayDepth::Sixteen,
                    )
                    .map_err(media_failure)?;
                while let Some(frame) = source.next_frame().map_err(media_failure)? {
                    let normalized: Vec<u8> = frame
                        .chunks_exact(2)
                        .flat_map(|pair| {
                            range
                                .normalize(u16::from_le_bytes([pair[0], pair[1]]))
                                .to_le_bytes()
                        })
                        .collect();
                    depth_out.write(&normalized).map_err(media_failure)?;
                    written += 1;
                }
            }
            let mut source = self
                .media
                .gray_frames(
                    &self.files.path(project, &mask_chunk(work, chunk)),
                    size,
                    GrayDepth::Eight,
                )
                .map_err(media_failure)?;
            while let Some(frame) = source.next_frame().map_err(media_failure)? {
                mask_out.write(&frame).map_err(media_failure)?;
            }
            let joined = DEPTH_FIRST_PASS_PERMILLE
                + u64::from(chunk + 1) * (1_000 - DEPTH_FIRST_PASS_PERMILLE)
                    / u64::from(work.chunks());
            cx.report_progress(Progress::from_permille(joined as u16));
        }
        if written != total {
            tracing::warn!(
                written,
                total,
                "a take's depth has another number of frames"
            );
        }
        depth_out.finish().map_err(media_failure)?;
        mask_out.finish().map_err(media_failure)?;
        let device = estimator
            .device()
            .clone()
            .slower(segmenter.device().clone());
        self.finish(
            work,
            TakeAnalysis {
                project,
                capture: work.capture,
                position: work.position,
                revision: work.revision,
                part: AnalysisPart::Depth,
                device,
                model: format!(
                    "{}, {}",
                    estimator.model().label(),
                    segmenter.model().label()
                ),
                checks,
                depth_range: Some(range),
                finished_at: SystemTime::now(),
                job: Some(cx.id()),
            },
        )
    }

    /// The state an earlier attempt saved, with its `blobs` snapshots.
    fn read_state<T: for<'de> Deserialize<'de>>(
        &self,
        work: &Work,
        blobs: usize,
    ) -> Option<(T, Vec<Vec<u8>>)> {
        let bytes = self.files.read(work.project, &state_file(work)).ok()?;
        let sections = unpack(&bytes)?;
        if sections.len() != blobs + 1 {
            return None;
        }
        let header = serde_json::from_slice(sections[0]).ok()?;
        Some((header, sections[1..].iter().map(|s| s.to_vec()).collect()))
    }
}

/// Where one part of a take's analysis is, for the Capture stage.
#[derive(Debug, Clone, PartialEq)]
pub enum AnalysisStatus {
    /// Neither analyzed nor queued: Analyze starts it.
    NotStarted,
    /// Waiting for its turn.
    Queued,
    Running {
        progress: Progress,
        /// On the graphics card, once the run says where it runs.
        gpu: Option<bool>,
        /// About how long the rest takes, once a chunk finished.
        left: Option<Duration>,
    },
    /// Stopped by the user; Analyze resumes it.
    Cancelled,
    /// Failed; Analyze tries again from where it stopped.
    Failed {
        kind: JobFailureKind,
    },
    Done {
        device: Device,
    },
}

impl AnalysisStatus {
    fn of_job(job: &Job, total: u32) -> AnalysisStatus {
        match job.state() {
            JobState::Queued => AnalysisStatus::Queued,
            JobState::Running => {
                let checkpoint = job
                    .checkpoint()
                    .and_then(|text| serde_json::from_str::<AnalysisCheckpoint>(text).ok());
                AnalysisStatus::Running {
                    progress: job.progress(),
                    gpu: checkpoint.map(|c| c.gpu),
                    left: checkpoint.filter(|c| c.ms_per_frame > 0).map(|c| {
                        Duration::from_millis(
                            u64::from(total.saturating_sub(c.done)) * u64::from(c.ms_per_frame),
                        )
                    }),
                }
            }
            JobState::Cancelled => AnalysisStatus::Cancelled,
            JobState::Failed => AnalysisStatus::Failed {
                kind: job
                    .failure()
                    .map_or(JobFailureKind::Unexpected, |failure| failure.kind),
            },
            // It stopped without an analysis: its take moved on.
            JobState::Done => AnalysisStatus::NotStarted,
        }
    }

    /// Whether it runs or waits to.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            AnalysisStatus::Queued | AnalysisStatus::Running { .. }
        )
    }
}

/// A take's analysis as the Capture stage shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeAnalysisView {
    pub mode: TakeMode,
    /// The face mesh.
    pub face: AnalysisStatus,
    /// The depth map and person mask, in Depth mode.
    pub depth: Option<AnalysisStatus>,
    /// What the finished parts found.
    pub checks: Option<TakeChecks>,
    /// The take had an analysis before its cut moved.
    pub redone: bool,
}

impl TakeAnalysisView {
    /// Whether the take cannot go on: more than one face in its frames.
    pub fn blocked(&self) -> bool {
        self.checks.is_some_and(|checks| checks.blocked())
    }
}

impl Bardo {
    /// The analysis jobs of the project's capture `capture`, oldest first,
    /// with their payloads.
    fn analysis_jobs(&self, capture: CaptureId) -> Vec<(Job, AnalysisPayload)> {
        let id = capture.to_string();
        self.jobs()
            .into_iter()
            .filter(|job| job.kind() == JobKind::TakeAnalysis)
            .filter_map(|job| {
                let payload = AnalysisPayload::parse(job.payload()).ok()?;
                (payload.capture == id).then_some((job, payload))
            })
            .collect()
    }

    /// Each take's analysis, in order.
    pub fn take_analyses(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<TakeAnalysisView>, CaptureError> {
        let (capture, _) = self.current_capture(project)?;
        Ok(self.analysis_views(&capture)?)
    }

    /// Each of `capture`'s takes' analysis, in order.
    pub(crate) fn analysis_views(
        &self,
        capture: &Capture,
    ) -> Result<Vec<TakeAnalysisView>, bardo_domain::RepositoryError> {
        let analyses = self.captures.analyses(capture.project)?;
        let jobs = self.analysis_jobs(capture.id);
        Ok(capture
            .takes
            .as_slice()
            .iter()
            .enumerate()
            .map(|(index, take)| {
                let position = index as u32;
                let status = |part: AnalysisPart| {
                    if let Some(done) = analyses.iter().find(|analysis| {
                        analysis.part == part && analysis.fits(capture.id, position, take.revision)
                    }) {
                        return (
                            AnalysisStatus::Done {
                                device: done.device.clone(),
                            },
                            Some(done.checks),
                        );
                    }
                    let latest = jobs.iter().rev().find(|(_, payload)| {
                        payload.position == position
                            && payload.revision == take.revision
                            && payload.part == part.code()
                    });
                    let status = latest.map_or(AnalysisStatus::NotStarted, |(job, payload)| {
                        AnalysisStatus::of_job(job, payload.frames)
                    });
                    (status, None)
                };
                let (face, face_checks) = status(AnalysisPart::Face);
                let (depth, depth_checks) = if take.mode.parts().contains(&AnalysisPart::Depth) {
                    let (depth, checks) = status(AnalysisPart::Depth);
                    (Some(depth), checks)
                } else {
                    (None, None)
                };
                let checks = match (face_checks, depth_checks) {
                    (Some(face), Some(depth)) => Some(face.merged(&depth)),
                    (face, depth) => face.or(depth),
                };
                TakeAnalysisView {
                    mode: take.mode,
                    face,
                    depth,
                    checks,
                    redone: analyses.iter().any(|analysis| {
                        analysis.position == position
                            && !analysis.fits(capture.id, position, take.revision)
                    }),
                }
            })
            .collect())
    }

    /// Queues the analyses take `index` of `capture` needs and has neither
    /// done nor under way. A failed or cancelled one is resumed when
    /// `resume`, else left for the user.
    fn queue_take_analyses(
        &self,
        capture: &Capture,
        index: usize,
        resume: bool,
    ) -> Result<(), CaptureError> {
        let Some(take) = capture.takes.as_slice().get(index) else {
            return Ok(());
        };
        let position = index as u32;
        let analyses = self.captures.analyses(capture.project)?;
        let jobs = self.analysis_jobs(capture.id);
        for &part in take.mode.parts() {
            if analyses.iter().any(|analysis| {
                analysis.part == part && analysis.fits(capture.id, position, take.revision)
            }) {
                continue;
            }
            let latest = jobs.iter().rev().find(|(_, payload)| {
                payload.position == position
                    && payload.revision == take.revision
                    && payload.part == part.code()
            });
            match latest.map(|(job, _)| job) {
                Some(job) if job.state().is_active() => {}
                Some(job) if job.can_retry() => {
                    if resume {
                        // It resumes from its last finished chunk.
                        let _ = self.jobs.retry(job.id());
                    }
                }
                _ => {
                    if let Some(job) = analysis_job(self.profile.id, capture, index, part) {
                        self.jobs.enqueue(job)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Starts what take `index`'s analysis still needs: a new run, or the
    /// stopped one resumed.
    pub fn analyze_take(&self, project: VideoProjectId, index: usize) -> Result<(), CaptureError> {
        let (capture, _) = self.current_capture(project)?;
        if index >= capture.takes.as_slice().len() {
            return Err(CaptureError::Boundary(
                bardo_domain::BoundaryError::NoSuchTake,
            ));
        }
        self.queue_take_analyses(&capture, index, true)
    }

    /// Sets what take `index` goes to its take model as. Depth mode queues
    /// its depth and person mask; leaving it stops them at their next
    /// chunk.
    pub fn set_take_mode(
        &self,
        project: VideoProjectId,
        index: usize,
        mode: TakeMode,
    ) -> Result<(), CaptureError> {
        let (mut capture, _) = self.current_capture(project)?;
        capture.takes.set_mode(index, mode)?;
        self.captures
            .save_takes(capture.project, capture.id, &capture.takes)?;
        self.queue_take_analyses(&capture, index, false)
    }

    /// Queues again the analyses of the takes either side of the cut
    /// after take `index`, which just moved.
    pub(crate) fn reanalyze_around(
        &self,
        capture: &Capture,
        index: usize,
    ) -> Result<(), CaptureError> {
        for take in [index, index + 1] {
            self.queue_take_analyses(capture, take, false)?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use bardo_domain::{
        DepthEstimator, DepthMap, Device, FACE_LANDMARKS, FaceMesh, FaceObservation, FaceTracker,
        MASK_FACE_SKIN, ModelFile, ModelInfo, PersonMask, PersonSegmenter, Region, RgbFrame,
        VisionError, VisionModels,
    };

    fn model(name: &str) -> ModelInfo {
        ModelInfo {
            name: name.into(),
            version: "test/1".into(),
            files: vec![ModelFile {
                file: format!("{name}.onnx"),
                sha256: "ab".repeat(32),
            }],
        }
    }

    /// What the fake models share with the test.
    #[derive(Default)]
    pub(crate) struct Seen {
        /// Faces in every frame; one unless set.
        pub(crate) faces: Mutex<Option<u32>>,
        /// The models are not installed.
        pub(crate) missing: AtomicBool,
        /// Run on the processor, the graphics card having failed.
        pub(crate) on_cpu: AtomicBool,
        /// Frames the tracker and the depth model saw, over every run.
        pub(crate) tracked: AtomicU32,
        pub(crate) depth_pushed: AtomicU32,
        /// Each tracker waits at its frame of this number (counted from 1,
        /// over resumed runs) while `hold` is set, with `holding` set
        /// meanwhile.
        pub(crate) hold_at: Mutex<Option<u32>>,
        pub(crate) hold: AtomicBool,
        pub(crate) holding: AtomicU32,
    }

    /// The vision models, faked: every frame shows the faces `Seen` says,
    /// each a third of the frame high, and a frame's depth comes out
    /// [`DEPTH_LAG`] frames later, nearer down and to the right and a
    /// little nearer every frame.
    #[derive(Default)]
    pub(crate) struct FakeVision {
        pub(crate) seen: Arc<Seen>,
    }

    pub(crate) const DEPTH_LAG: usize = 3;
    pub(crate) const DEPTH_SIZE: (u32, u32) = (8, 6);

    fn device(seen: &Seen) -> Device {
        if seen.on_cpu.load(Ordering::SeqCst) {
            Device::Cpu {
                fallback: Some("No devices detected".into()),
            }
        } else {
            Device::Gpu
        }
    }

    fn mesh() -> FaceMesh {
        let mut landmarks = vec![[0.5, 0.5, 0.0]; FACE_LANDMARKS];
        landmarks[10][1] = 0.33;
        landmarks[152][1] = 0.66;
        FaceMesh {
            landmarks,
            blendshapes: [0.25; 52],
        }
    }

    struct Tracker {
        seen: Arc<Seen>,
        model: ModelInfo,
        device: Device,
        /// Frames this tracker followed, carried in its snapshot.
        frames: u32,
    }

    impl FaceTracker for Tracker {
        fn model(&self) -> &ModelInfo {
            &self.model
        }

        fn device(&self) -> &Device {
            &self.device
        }

        fn track(
            &mut self,
            _frame: &RgbFrame,
            _at: Duration,
            _head: &mut dyn FnMut() -> Result<Option<Region>, VisionError>,
        ) -> Result<FaceObservation, VisionError> {
            self.seen.tracked.fetch_add(1, Ordering::SeqCst);
            if *self.seen.hold_at.lock().unwrap() == Some(self.frames + 1) {
                self.seen.holding.fetch_add(1, Ordering::SeqCst);
                while self.seen.hold.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            self.frames += 1;
            let faces = self.seen.faces.lock().unwrap().unwrap_or(1);
            Ok(FaceObservation {
                mesh: (faces > 0).then(mesh),
                detected: Some(faces),
            })
        }

        fn detect_in(
            &mut self,
            _frame: &RgbFrame,
            _region: &Region,
        ) -> Result<Vec<Region>, VisionError> {
            Ok(Vec::new())
        }

        fn snapshot(&self) -> Vec<u8> {
            self.frames.to_le_bytes().to_vec()
        }

        fn restore(&mut self, snapshot: &[u8]) -> Result<(), VisionError> {
            let bytes = <[u8; 4]>::try_from(snapshot)
                .map_err(|_| VisionError::State("not a fake tracker".into()))?;
            self.frames = u32::from_le_bytes(bytes);
            Ok(())
        }
    }

    struct Segmenter {
        model: ModelInfo,
        device: Device,
    }

    impl PersonSegmenter for Segmenter {
        fn model(&self) -> &ModelInfo {
            &self.model
        }

        fn device(&self) -> &Device {
            &self.device
        }

        fn segment(&mut self, _frame: &RgbFrame) -> Result<PersonMask, VisionError> {
            // A person in the middle, with a face.
            let mut person = vec![0; 16];
            let mut class = vec![0; 16];
            for cell in [5, 6, 9, 10] {
                person[cell] = 255;
                class[cell] = MASK_FACE_SKIN;
            }
            Ok(PersonMask {
                width: 4,
                height: 4,
                person,
                class,
            })
        }
    }

    struct Depth {
        seen: Arc<Seen>,
        model: ModelInfo,
        device: Device,
        /// Frames pushed and frames given out.
        pushed: u32,
        given: u32,
    }

    impl Depth {
        fn map(index: u32) -> DepthMap {
            let (width, height) = DEPTH_SIZE;
            DepthMap {
                width,
                height,
                data: (0..width * height)
                    .map(|cell| index as f32 / 2_000.0 + cell as f32 / 100.0)
                    .collect(),
            }
        }
    }

    impl DepthEstimator for Depth {
        fn model(&self) -> &ModelInfo {
            &self.model
        }

        fn device(&self) -> &Device {
            &self.device
        }

        fn size(&self) -> (u32, u32) {
            DEPTH_SIZE
        }

        fn push(&mut self, _frame: &RgbFrame) -> Result<Vec<DepthMap>, VisionError> {
            self.seen.depth_pushed.fetch_add(1, Ordering::SeqCst);
            self.pushed += 1;
            let mut out = Vec::new();
            while self.given + (DEPTH_LAG as u32) < self.pushed {
                out.push(Depth::map(self.given));
                self.given += 1;
            }
            Ok(out)
        }

        fn finish(&mut self) -> Result<Vec<DepthMap>, VisionError> {
            let out = (self.given..self.pushed).map(Depth::map).collect();
            self.given = self.pushed;
            Ok(out)
        }

        fn snapshot(&self) -> Vec<u8> {
            [self.pushed.to_le_bytes(), self.given.to_le_bytes()].concat()
        }

        fn restore(&mut self, snapshot: &[u8]) -> Result<(), VisionError> {
            let bytes = <[u8; 8]>::try_from(snapshot)
                .map_err(|_| VisionError::State("not a fake depth".into()))?;
            self.pushed = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            self.given = u32::from_le_bytes(bytes[4..].try_into().unwrap());
            Ok(())
        }
    }

    impl FakeVision {
        fn check(&self) -> Result<(), VisionError> {
            if self.seen.missing.load(Ordering::SeqCst) {
                return Err(VisionError::Missing("face_detector.onnx".into()));
            }
            Ok(())
        }
    }

    impl VisionModels for FakeVision {
        fn face_tracker(&self) -> Result<Box<dyn FaceTracker>, VisionError> {
            self.check()?;
            Ok(Box::new(Tracker {
                seen: Arc::clone(&self.seen),
                model: model("Face Landmarker"),
                device: device(&self.seen),
                frames: 0,
            }))
        }

        fn person_segmenter(&self) -> Result<Box<dyn PersonSegmenter>, VisionError> {
            self.check()?;
            Ok(Box::new(Segmenter {
                model: model("Selfie multiclass"),
                device: device(&self.seen),
            }))
        }

        fn depth_estimator(&self, _vertical: bool) -> Result<Box<dyn DepthEstimator>, VisionError> {
            self.check()?;
            Ok(Box::new(Depth {
                seen: Arc::clone(&self.seen),
                model: model("Video Depth Anything Small"),
                device: device(&self.seen),
                pushed: 0,
                given: 0,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    use bardo_domain::{BoundaryMove, FACE_LANDMARKS, FaceLandmarks};

    use super::testing::DEPTH_SIZE;
    use super::*;
    use crate::CaptureVoiceChoice;
    use crate::captures::tests::{Setup, setup};
    use crate::narrations::tests::{project, wait_done};

    /// A capture of two takes, imported; its face mesh jobs are queued.
    fn imported(s: &Setup) -> (VideoProjectId, Capture) {
        // 600 ms a character: long enough for two takes.
        *s.h.aligner.step.lock().unwrap() = Some(Duration::from_millis(600));
        let project = project(&s.app);
        let (_, capture, _) = s.imported(project.id, CaptureVoiceChoice::Persona);
        assert_eq!(capture.takes.as_slice().len(), 2);
        (project.id, capture)
    }

    /// Waits until `count` analysis jobs exist and none is under way.
    fn settled(app: &Bardo, count: usize) -> Vec<Job> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let jobs: Vec<Job> = app
                .jobs()
                .into_iter()
                .filter(|job| job.kind() == JobKind::TakeAnalysis)
                .collect();
            if jobs.len() >= count && jobs.iter().all(|job| !job.state().is_active()) {
                return jobs;
            }
            assert!(
                Instant::now() < deadline,
                "the analyses never settled: {jobs:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn frames_of(capture: &Capture, index: usize) -> u32 {
        let take = &capture.takes.as_slice()[index];
        take_frames(take.end - take.start, analysis_fps(capture.video.fps_milli))
    }

    #[test]
    fn importing_a_capture_writes_every_takes_face_mesh() {
        let s = setup(13);
        let (project, capture) = imported(&s);
        let jobs = settled(&s.app, 2);
        assert!(
            jobs.iter().all(|job| job.state() == JobState::Done),
            "{jobs:?}"
        );

        let views = s.app.take_analyses(project).unwrap();
        assert_eq!(views.len(), 2);
        for (index, view) in views.iter().enumerate() {
            assert_eq!(view.mode, TakeMode::Recording);
            assert_eq!(
                view.face,
                AnalysisStatus::Done {
                    device: Device::Gpu
                }
            );
            assert_eq!(view.depth, None);
            let checks = view.checks.unwrap();
            assert_eq!(checks.frames, frames_of(&capture, index));
            assert_eq!(checks.no_face + checks.several_faces + checks.small_face, 0);
            assert!(!view.blocked());

            // The take's face_landmarks.json reads back, every frame with
            // its face's 478 points.
            let take = &capture.takes.as_slice()[index];
            let name = face_file(capture.id, index as u32, take.revision);
            let json = s.h.files.read(project, &name).unwrap();
            let file = FaceLandmarks::from_json(std::str::from_utf8(&json).unwrap()).unwrap();
            assert_eq!(file.source.frames, frames_of(&capture, index));
            assert_eq!(file.source.file, capture.video_file);
            assert_eq!(file.source.fps_milli, 29_970);
            assert_eq!(file.frames.len() as u32, frames_of(&capture, index));
            assert!(
                file.frames.iter().all(|frame| frame.faces.len() == 1
                    && frame.faces[0].landmarks.len() == FACE_LANDMARKS)
            );
            assert_eq!(
                file.frames[1].at.as_millis(),
                take_frame_time(1, 29_970).as_millis()
            );
            // What the run kept between chunks is gone.
            let work = AnalysisPayload::parse(jobs[index].payload())
                .unwrap()
                .work()
                .unwrap();
            for file in leftovers(&work) {
                assert!(!s.h.files.exists(project, &file), "{file}");
            }
        }
        // Each take's own stretch of the recording, at its frame rate.
        let decoded = s.h.media.decoded.lock().unwrap().clone();
        for take in capture.takes.as_slice() {
            assert!(
                decoded.contains(&(take.start, take.end - take.start, 29_970)),
                "{decoded:?}"
            );
        }
    }

    #[test]
    fn a_take_with_two_people_in_frame_cannot_go_on() {
        let s = setup(13);
        *s.h.vision.seen.faces.lock().unwrap() = Some(2);
        let (project, _) = imported(&s);
        settled(&s.app, 2);
        let views = s.app.take_analyses(project).unwrap();
        assert!(views.iter().all(TakeAnalysisView::blocked));
        let checks = views[0].checks.unwrap();
        assert_eq!(checks.several_faces, checks.frames);
    }

    #[test]
    fn depth_mode_writes_the_depth_and_mask_normalized_over_the_whole_take() {
        let s = setup(13);
        let (project, capture) = imported(&s);
        settled(&s.app, 2);

        s.app.set_take_mode(project, 0, TakeMode::Depth).unwrap();
        settled(&s.app, 3);
        let views = s.app.take_analyses(project).unwrap();
        assert_eq!(views[0].mode, TakeMode::Depth);
        assert_eq!(
            views[0].depth,
            Some(AnalysisStatus::Done {
                device: Device::Gpu
            })
        );
        assert_eq!(views[1].depth, None);

        let frames = frames_of(&capture, 0) as usize;
        let cells = (DEPTH_SIZE.0 * DEPTH_SIZE.1) as usize;
        let revision = capture.takes.as_slice()[0].revision;
        let depth =
            s.h.files
                .read(project, &depth_file(capture.id, 0, revision))
                .unwrap();
        let mask =
            s.h.files
                .read(project, &mask_file(capture.id, 0, revision))
                .unwrap();
        // Every frame's depth came out, the last ones after the take ended.
        assert_eq!(depth.len(), frames * cells * 2);
        assert_eq!(mask.len(), frames * cells);
        assert!(mask.contains(&0) && mask.contains(&255));
        // Normalized over the take, not per frame: the far corner of the
        // first frame is the farthest, the near corner of the last the
        // nearest, and the same pixel comes nearer frame by frame.
        let value = |frame: usize, cell: usize| {
            let at = (frame * cells + cell) * 2;
            u16::from_le_bytes([depth[at], depth[at + 1]])
        };
        assert_eq!(value(0, 0), 0);
        assert_eq!(value(frames - 1, cells - 1), u16::MAX);
        assert!(value(frames / 2, 20) < value(frames - 1, 20));
        assert!(value(frames / 2, 20) > value(1, 20));
        let analysis = s.h.db.analyses(project).unwrap();
        assert!(
            analysis
                .iter()
                .any(|a| a.part == AnalysisPart::Depth && a.depth_range.is_some())
        );

        // Back to Recording: no depth to show.
        s.app
            .set_take_mode(project, 0, TakeMode::Recording)
            .unwrap();
        assert_eq!(s.app.take_analyses(project).unwrap()[0].depth, None);
    }

    #[test]
    fn a_cancelled_analysis_resumes_at_its_last_finished_chunk() {
        let s = setup(13);
        let seen = &s.h.vision.seen;
        // Each run waits at the 100th frame of the take, past one chunk.
        *seen.hold_at.lock().unwrap() = Some(100);
        seen.hold.store(true, Ordering::SeqCst);
        let (project, capture) = imported(&s);
        let deadline = Instant::now() + Duration::from_secs(20);
        // Both takes are longer than 100 frames.
        while seen.holding.load(Ordering::SeqCst) < 2 {
            assert!(Instant::now() < deadline, "never reached the 100th frame");
            std::thread::sleep(Duration::from_millis(2));
        }
        let running: Vec<Job> = s
            .app
            .jobs()
            .into_iter()
            .filter(|job| job.kind() == JobKind::TakeAnalysis && job.state() == JobState::Running)
            .collect();
        assert_eq!(running.len(), 2);
        for job in &running {
            s.app.cancel_job(job.id()).unwrap();
        }
        seen.hold.store(false, Ordering::SeqCst);
        let held: Vec<u32> = running
            .iter()
            .map(|job| {
                assert_eq!(wait_done(&s.app, job.id()).state(), JobState::Cancelled);
                AnalysisPayload::parse(job.payload()).unwrap().position
            })
            .collect();
        settled(&s.app, 2);
        let views = s.app.take_analyses(project).unwrap();
        for &position in &held {
            assert_eq!(views[position as usize].face, AnalysisStatus::Cancelled);
        }

        // Analyze resumes it: decoding starts at the second chunk, and the
        // file has every frame once.
        s.h.media.decoded.lock().unwrap().clear();
        for &position in &held {
            s.app.analyze_take(project, position as usize).unwrap();
        }
        settled(&s.app, 2);
        let decoded = s.h.media.decoded.lock().unwrap().clone();
        for &position in &held {
            let index = position as usize;
            let take = &capture.takes.as_slice()[index];
            let from = take.start + take_frame_time(ANALYSIS_CHUNK_FRAMES, 29_970);
            assert!(decoded.iter().any(|(at, ..)| *at == from), "{decoded:?}");
            let json =
                s.h.files
                    .read(project, &face_file(capture.id, position, take.revision))
                    .unwrap();
            let file = FaceLandmarks::from_json(std::str::from_utf8(&json).unwrap()).unwrap();
            assert_eq!(file.frames.len() as u32, frames_of(&capture, index));
            assert_eq!(
                s.app.take_analyses(project).unwrap()[index]
                    .checks
                    .unwrap()
                    .frames,
                frames_of(&capture, index)
            );
        }
    }

    #[test]
    fn moving_a_cut_analyzes_both_takes_again() {
        let s = setup(13);
        let (project, capture) = imported(&s);
        settled(&s.app, 2);
        let before = capture.takes.as_slice().to_vec();

        s.app
            .move_take_boundary(project, 0, BoundaryMove::Earlier)
            .unwrap();
        let views = s.app.take_analyses(project).unwrap();
        assert!(views.iter().all(|view| view.redone
            || view.face
                == AnalysisStatus::Done {
                    device: Device::Gpu
                }));
        settled(&s.app, 4);
        let moved = s.app.narration(project).unwrap().capture.unwrap();
        for (index, take) in moved.takes.as_slice().iter().enumerate() {
            assert_eq!(take.revision, 2);
            let view = &s.app.take_analyses(project).unwrap()[index];
            assert_eq!(
                view.face,
                AnalysisStatus::Done {
                    device: Device::Gpu
                }
            );
            assert!(!view.redone);
            assert_eq!(view.checks.unwrap().frames, frames_of(&moved, index));
            assert!(
                s.h.files
                    .exists(project, &face_file(capture.id, index as u32, 2))
            );
            // The first revision's file went with it.
            assert!(!s.h.files.exists(
                project,
                &face_file(capture.id, index as u32, before[index].revision)
            ));
        }

        // Removing the capture takes its analysis files.
        s.app.remove_capture(project).unwrap();
        for index in 0..2 {
            assert!(!s.h.files.exists(project, &face_file(capture.id, index, 2)));
        }
    }

    #[test]
    fn without_the_models_the_analysis_fails_and_runs_again_once_they_are_back() {
        let s = setup(13);
        s.h.vision.seen.missing.store(true, Ordering::SeqCst);
        let (project, _) = imported(&s);
        let jobs = settled(&s.app, 2);
        assert!(jobs.iter().all(|job| job.state() == JobState::Failed
            && job.failure().unwrap().kind == JobFailureKind::VisionModels));
        assert_eq!(
            s.app.take_analyses(project).unwrap()[0].face,
            AnalysisStatus::Failed {
                kind: JobFailureKind::VisionModels
            }
        );

        // On a machine whose graphics card does not run them, the
        // processor does.
        s.h.vision.seen.missing.store(false, Ordering::SeqCst);
        s.h.vision.seen.on_cpu.store(true, Ordering::SeqCst);
        s.app.analyze_take(project, 0).unwrap();
        settled(&s.app, 2);
        assert_eq!(
            s.app.take_analyses(project).unwrap()[0].face,
            AnalysisStatus::Done {
                device: Device::Cpu {
                    fallback: Some("No devices detected".into())
                }
            }
        );
        assert!(matches!(
            s.app.analyze_take(project, 7),
            Err(CaptureError::Boundary(_))
        ));
    }

    #[test]
    fn a_state_file_packs_its_header_and_snapshots() {
        let packed = pack(&serde_json::json!({"chunks": 2}), &[b"abc", b""]);
        let sections = unpack(&packed).unwrap();
        assert_eq!(sections, [b"{\"chunks\":2}".as_slice(), b"abc", b""]);
        assert!(
            unpack(&packed[..packed.len() - 4]).is_some_and(|s| s.len() < 3)
                || unpack(&packed[..packed.len() - 4]).is_none()
        );
        assert!(unpack(b"something else").is_none());
    }
}
