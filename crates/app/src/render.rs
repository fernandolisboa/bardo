//! The final render (PRD stories 71-75). From the editor or the project's
//! Render stage the user opens the review: the cut's length, frame shape,
//! loudness and captions, and every network account of the channel as a
//! target in its render preset. Quality gates (`bardo_domain::Gate`) flag
//! what would go wrong; a blocking one keeps that target from rendering.
//!
//! Render is irreversible in time, so nothing starts from the review
//! itself: `Bardo::start_render` takes the review the user saw and the
//! targets they confirmed, refuses a cut that changed since, and queues one
//! job. The job keeps the plans it was given, so what renders is what was
//! reviewed, and renders one file per target with the best encoder this
//! machine has (NVENC first, software when the GPU encoder fails), two-pass
//! loudness to the preset's target. Each finished file is recorded and
//! checkpointed; a cancelled or failed render resumes with the targets left.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    AspectRatio, Bitrate, CutFacts, FPS, Gate, Job, JobFailure, JobFailureKind, JobId, JobKind,
    Loudness, MaxDuration, MeasuredLoudness, Network, NetworkAccountId, ProfileId, Progress,
    ProjectFiles, Render, RenderId, RenderPreset, RenderRepository, RepositoryError, Resolution,
    TRUE_PEAK_CEILING, VideoProjectId, cut_gates, output_gates,
};
use bardo_media::MediaEngine;
use bardo_media::ffmpeg::{
    Encoders, FrameSize, LoudnessTarget, MediaError, Monitor, Output, RenderPlan, VideoEncoder,
};
use serde::{Deserialize, Serialize};

use crate::editor::{ClipMedia, EditorError, EditorView, PlanMedia, timeline_plan};
use crate::jobs::{JobContext, JobHandler};
use crate::scenes::{id, parse, to_json};
use crate::{Bardo, Text};

/// How far under the true-peak ceiling normalization aims: AAC encoding
/// after it adds a few tenths of a dB of inter-sample peak.
const CODEC_HEADROOM: f64 = 0.5;

/// AAC at 48 kHz stereo, at the rate every network takes.
const AUDIO_BITRATE: u32 = 192_000;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("video project not found")]
    ProjectNotFound,
    /// No scene plan and narration yet, so no cut to render.
    #[error("the project has no cut to render")]
    NoCut,
    /// The review's encoders and loudness are not checked yet.
    #[error("the review is not checked yet")]
    NotChecked,
    #[error("no target chosen")]
    NothingChosen,
    /// A chosen target has a blocking gate.
    #[error("a chosen target cannot be rendered")]
    Blocked,
    /// The cut changed after the review: review it again.
    #[error("the cut changed since the review")]
    CutChanged,
    /// A render of this project is running or waiting.
    #[error("the project is already rendering")]
    AlreadyRendering,
    /// An export of this project is running or waiting: it copies the
    /// files a render would rewrite.
    #[error("the project is exporting")]
    Exporting,
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl RenderError {
    /// What the render stage says.
    pub fn message(&self) -> Text {
        match self {
            RenderError::ProjectNotFound => Text::ProjectNotFound,
            RenderError::NoCut => Text::RenderNoCut,
            RenderError::NotChecked => Text::RenderChecking,
            RenderError::NothingChosen => Text::RenderNothingChosen,
            RenderError::Blocked => Text::RenderBlocked,
            RenderError::CutChanged => Text::RenderCutChanged,
            RenderError::AlreadyRendering => Text::RenderAlreadyRunning,
            RenderError::Exporting => Text::RenderWhileExporting,
            RenderError::Media(MediaError::NotFound { .. }) => Text::EditorFfmpegMissing,
            RenderError::Media(_) => Text::RenderCheckFailed,
            RenderError::Repository(_) => Text::RenderNotLoaded,
        }
    }
}

impl From<EditorError> for RenderError {
    fn from(error: EditorError) -> Self {
        match error {
            EditorError::Repository(error) => RenderError::Repository(error),
            EditorError::Preview(error) => RenderError::Media(error),
            _ => RenderError::ProjectNotFound,
        }
    }
}

/// One network account as a render target.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderTarget {
    pub account: NetworkAccountId,
    pub network: Network,
    pub handle: String,
    /// The network's preset with the account's overrides.
    pub preset: RenderPreset,
    /// The encoder the render would use, once the machine's are known.
    pub encoder: Option<VideoEncoder>,
    pub gates: Vec<Gate>,
    /// The file last rendered for this account.
    pub last: Option<Render>,
    /// Whether `last` was rendered from the cut as it is now, in the
    /// preset as it is now.
    pub last_current: bool,
}

impl RenderTarget {
    /// Whether nothing blocks this target, as far as is known.
    pub fn can_render(&self) -> bool {
        !self.gates.iter().any(Gate::blocks)
    }

    /// Width and height of the output.
    pub fn size(&self) -> (u32, u32) {
        self.preset.dimensions()
    }
}

/// What the render review shows: the cut, the targets and their gates.
#[derive(Debug, Clone)]
pub struct RenderReview {
    pub project: VideoProjectId,
    /// The cut's length, frame shape, captions and missing media, and the
    /// mix's loudness once measured.
    pub cut: CutFacts,
    /// The gates about the cut itself.
    pub gates: Vec<Gate>,
    /// Every network account of the channel, in `Network::ALL` order.
    pub targets: Vec<RenderTarget>,
    encoders: Option<Encoders>,
    /// The cut framed for each shape a target needs, from the originals.
    plans: Vec<CutPlan>,
}

/// The cut framed for one frame shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CutPlan {
    aspect: String,
    /// The plan's fingerprint: what a render records as its cut.
    cut: String,
    plan: RenderPlan,
}

impl RenderReview {
    /// Whether the machine's encoders and the mix's loudness are known.
    pub fn is_checked(&self) -> bool {
        self.encoders.is_some() && self.cut.mix.is_some()
    }

    /// The targets the user may pick: checked, with nothing blocking.
    pub fn renderable(&self) -> Vec<NetworkAccountId> {
        if !self.is_checked() {
            return Vec::new();
        }
        self.targets
            .iter()
            .filter(|target| target.can_render())
            .map(|target| target.account)
            .collect()
    }

    /// The cut in its own frame shape: what checking measures.
    fn checked_plan(&self) -> Option<&CutPlan> {
        self.plans
            .iter()
            .find(|plan| plan.aspect == self.cut.aspect.code())
            .or(self.plans.first())
    }

    /// What checking needs, to run off the UI thread.
    pub fn checks(&self) -> RenderChecks {
        RenderChecks {
            media: None,
            plan: self.checked_plan().cloned(),
        }
    }

    /// The review with what checking found: encoders and the mix's
    /// loudness, and the gates that follow from them. Findings about
    /// another cut (it changed since) leave the review unchecked.
    pub fn checked(mut self, found: &CheckFound) -> Self {
        if self.checked_plan().map(|plan| plan.cut.as_str()) != Some(found.cut.as_str()) {
            return self;
        }
        self.cut.mix = Some(found.mix);
        self.encoders = Some(found.encoders.clone());
        self.regate();
        self
    }

    fn regate(&mut self) {
        self.gates = cut_gates(&self.cut);
        for target in &mut self.targets {
            target.encoder = self
                .encoders
                .as_ref()
                .and_then(|encoders| encoders.best(target.preset.codec).ok());
            let works = self.encoders.as_ref().map(|_| target.encoder.is_some());
            target.gates = output_gates(&self.cut, &target.preset, works);
        }
    }
}

/// Measures what the review cannot know without ffmpeg: which encoders
/// work and how loud the mix is. Holds no UI state, so it runs on a
/// background thread.
#[derive(Clone)]
pub struct RenderChecks {
    media: Option<Arc<dyn MediaEngine>>,
    plan: Option<CutPlan>,
}

/// What `RenderChecks::run` found, and about which cut.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckFound {
    cut: String,
    encoders: Encoders,
    mix: MeasuredLoudness,
}

impl RenderChecks {
    /// Tries the encoders (once per app run) and measures the mix. Blocks
    /// for a few seconds: call it off the UI thread.
    pub fn run(&self) -> Result<CheckFound, RenderError> {
        let (Some(media), Some(plan)) = (&self.media, &self.plan) else {
            return Err(RenderError::NoCut);
        };
        let encoders = media.encoders()?;
        let mix = media.measure_mix(&plan.plan, &())?;
        Ok(CheckFound {
            cut: plan.cut.clone(),
            encoders,
            mix: measured(mix),
        })
    }
}

fn measured(loudness: bardo_media::ffmpeg::Loudness) -> MeasuredLoudness {
    MeasuredLoudness {
        integrated: f64::from(loudness.integrated),
        true_peak: f64::from(loudness.true_peak),
    }
}

/// A stable fingerprint of a plan (FNV-1a over its JSON): equal plans,
/// equal fingerprints, on any machine and Rust version.
fn fingerprint(plan: &RenderPlan) -> String {
    let bytes = serde_json::to_vec(plan).expect("a render plan serializes");
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// The file a target renders to, in the project folder: one per network,
/// so a new render replaces the last.
pub(crate) fn render_file(network: Network) -> String {
    format!("render-{}.mp4", network.code())
}

/// Where a project's renders stand, for its Render stage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderSummary {
    /// Whether there is a cut to render.
    pub has_cut: bool,
    /// Files rendered from the cut as it is now.
    pub current: usize,
    /// Files rendered from an earlier cut or preset.
    pub outdated: usize,
    /// The project's latest render job.
    pub job: Option<Job>,
}

/// One output of a render job: a target, its preset and file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct OutputOrder {
    account: String,
    network: String,
    file: String,
    aspect: String,
    resolution: u32,
    codec: String,
    bitrate_kbps: u32,
    max_duration_secs: u32,
    loudness_tenths: i16,
}

impl OutputOrder {
    fn new(target: &RenderTarget) -> Self {
        let preset = &target.preset;
        Self {
            account: target.account.to_string(),
            network: target.network.code().to_owned(),
            file: render_file(target.network),
            aspect: preset.aspect.code().to_owned(),
            resolution: preset.resolution.short_side(),
            codec: preset.codec.code().to_owned(),
            bitrate_kbps: preset.bitrate.kbps(),
            max_duration_secs: preset.max_duration.seconds(),
            loudness_tenths: preset.loudness.tenths(),
        }
    }

    fn preset(&self) -> Result<RenderPreset, JobFailure> {
        let invalid = |what: &str| JobFailure::unexpected(format!("invalid preset {what}"));
        Ok(RenderPreset {
            aspect: self.aspect.parse().map_err(|_| invalid("aspect"))?,
            resolution: Resolution::from_short_side(self.resolution)
                .ok_or_else(|| invalid("resolution"))?,
            codec: self.codec.parse().map_err(|_| invalid("codec"))?,
            bitrate: Bitrate::from_kbps(self.bitrate_kbps).map_err(|_| invalid("bitrate"))?,
            max_duration: MaxDuration::from_seconds(self.max_duration_secs)
                .map_err(|_| invalid("length"))?,
            loudness: Loudness::from_tenths(self.loudness_tenths)
                .map_err(|_| invalid("loudness"))?,
        })
    }
}

/// What a render job renders: the reviewed plans and the confirmed
/// targets, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RenderPayload {
    project: String,
    outputs: Vec<OutputOrder>,
    plans: Vec<CutPlan>,
}

/// The targets a render job has finished, by account.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RenderCheckpoint {
    done: Vec<String>,
}

/// How many files a render job has finished, of how many: `(done, total)`.
pub fn render_job_files(job: &Job) -> (usize, usize) {
    let total = serde_json::from_str::<RenderPayload>(job.payload())
        .map_or(0, |payload| payload.outputs.len());
    let done = job
        .checkpoint()
        .and_then(|checkpoint| serde_json::from_str::<RenderCheckpoint>(checkpoint).ok())
        .map_or(0, |checkpoint| checkpoint.done.len());
    (done.min(total), total)
}

/// The project a render job serves.
#[derive(Deserialize)]
struct ProjectOf {
    project: String,
}

impl Bardo {
    fn latest_render_job(&self, project: VideoProjectId) -> Option<Job> {
        let project = project.to_string();
        self.jobs().into_iter().rev().find(|job| {
            job.kind() == JobKind::Render
                && serde_json::from_str::<ProjectOf>(job.payload())
                    .is_ok_and(|of| of.project == project)
        })
    }

    /// The cut of `view` framed for each shape, from the originals.
    fn cut_plans(
        &self,
        view: &EditorView,
        aspects: impl IntoIterator<Item = AspectRatio>,
    ) -> Result<Vec<CutPlan>, RenderError> {
        let mut plans: Vec<CutPlan> = Vec::new();
        for aspect in aspects {
            if plans.iter().any(|plan| plan.aspect == aspect.code()) {
                continue;
            }
            let plan = timeline_plan(view, &*self.files, aspect, PlanMedia::Originals)
                .ok_or(RenderError::NoCut)?;
            plans.push(CutPlan {
                aspect: aspect.code().to_owned(),
                cut: fingerprint(&plan),
                plan,
            });
        }
        Ok(plans)
    }

    /// The review of `project`'s cut for every network account of its
    /// channel. Encoders and loudness come after, from `RenderChecks`.
    pub fn render_review(&self, project: VideoProjectId) -> Result<RenderReview, RenderError> {
        let view = self.editor_view(project)?;
        let accounts = self.network_accounts.list(view.project.channel)?;
        let renders = self.renders.renders(project)?;
        let presets: Vec<RenderPreset> = accounts
            .iter()
            .map(|account| account.render_preset())
            .collect();
        let timeline = view
            .timeline
            .as_ref()
            .filter(|timeline| !timeline.is_empty())
            .ok_or(RenderError::NoCut)?;
        let aspect = timeline.aspect();
        let plans = self.cut_plans(
            &view,
            std::iter::once(aspect).chain(presets.iter().map(|preset| preset.aspect)),
        )?;
        let cut = CutFacts {
            duration: view.duration(),
            aspect,
            captions_shown: timeline.captions().shown(),
            missing_media: view
                .clips
                .iter()
                .filter(|clip| clip.media == ClipMedia::Missing)
                .count(),
            mix: None,
        };
        let targets = accounts
            .into_iter()
            .zip(presets)
            .map(|(account, preset)| {
                let last = renders
                    .iter()
                    .find(|render| render.account == account.id)
                    .cloned();
                let current_cut = plans
                    .iter()
                    .find(|plan| plan.aspect == preset.aspect.code())
                    .map(|plan| plan.cut.as_str());
                let last_current = last.as_ref().is_some_and(|last| {
                    Some(last.cut.as_str()) == current_cut && last.preset == preset
                });
                RenderTarget {
                    account: account.id,
                    network: account.network,
                    handle: account.details.handle().to_owned(),
                    preset,
                    encoder: None,
                    gates: Vec::new(),
                    last,
                    last_current,
                }
            })
            .collect();
        let mut review = RenderReview {
            project,
            cut,
            gates: Vec::new(),
            targets,
            encoders: None,
            plans,
        };
        review.regate();
        Ok(review)
    }

    /// What checking `review` needs, bound to the bundled ffmpeg.
    pub fn render_checks(&self, review: &RenderReview) -> RenderChecks {
        RenderChecks {
            media: Some(Arc::clone(&self.media)),
            ..review.checks()
        }
    }

    /// Queues the render of the chosen targets of a checked review. Refuses
    /// a target the gates block, and a cut that changed since the review
    /// (the user reviews it again).
    pub fn start_render(
        &self,
        review: &RenderReview,
        chosen: &[NetworkAccountId],
    ) -> Result<JobId, RenderError> {
        if !review.is_checked() {
            return Err(RenderError::NotChecked);
        }
        let targets: Vec<&RenderTarget> = review
            .targets
            .iter()
            .filter(|target| chosen.contains(&target.account))
            .collect();
        if targets.is_empty() {
            return Err(RenderError::NothingChosen);
        }
        if targets.iter().any(|target| !target.can_render()) {
            return Err(RenderError::Blocked);
        }
        if self
            .latest_render_job(review.project)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(RenderError::AlreadyRendering);
        }
        if self
            .export_job(review.project)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(RenderError::Exporting);
        }
        let reviewed: Vec<CutPlan> = review
            .plans
            .iter()
            .filter(|plan| {
                targets
                    .iter()
                    .any(|target| target.preset.aspect.code() == plan.aspect)
            })
            .cloned()
            .collect();
        let view = self.editor_view(review.project)?;
        let now = self
            .cut_plans(&view, targets.iter().map(|target| target.preset.aspect))
            .map_err(|error| match error {
                RenderError::NoCut => RenderError::CutChanged,
                error => error,
            })?;
        let unchanged = reviewed.iter().all(|plan| {
            now.iter()
                .any(|current| current.aspect == plan.aspect && current.cut == plan.cut)
        });
        if !unchanged {
            return Err(RenderError::CutChanged);
        }
        let payload = RenderPayload {
            project: review.project.to_string(),
            outputs: targets
                .iter()
                .map(|target| OutputOrder::new(target))
                .collect(),
            plans: reviewed,
        };
        let job = Job::new(self.profile.id, JobKind::Render, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// The project's rendered files, in `Network::ALL` order.
    pub fn renders(&self, project: VideoProjectId) -> Result<Vec<Render>, RenderError> {
        self.editor_project(project)?;
        Ok(self.renders.renders(project)?)
    }

    /// The project's latest render job.
    pub fn render_job(&self, project: VideoProjectId) -> Option<Job> {
        self.latest_render_job(project)
    }

    /// Where a rendered file is on disk.
    pub fn render_path(&self, render: &Render) -> PathBuf {
        self.files.path(render.project, &render.file)
    }

    /// Where the project's renders stand: how many files match the cut and
    /// presets as they are now, and the latest render job.
    pub fn render_summary(&self, project: VideoProjectId) -> Result<RenderSummary, RenderError> {
        let job = self.latest_render_job(project);
        let review = match self.render_review(project) {
            Ok(review) => review,
            Err(RenderError::NoCut) => {
                return Ok(RenderSummary {
                    has_cut: false,
                    job,
                    ..RenderSummary::default()
                });
            }
            Err(error) => return Err(error),
        };
        let rendered = review.targets.iter().filter(|target| target.last.is_some());
        let current = rendered
            .clone()
            .filter(|target| target.last_current)
            .count();
        Ok(RenderSummary {
            has_cut: true,
            current,
            outdated: rendered.count() - current,
            job,
        })
    }
}

/// What the render review reads, in the interface language.
impl Bardo {
    /// A figure to the tenth, signed: `−14.2`, `+6,3`.
    fn tenths(&self, value: f64) -> String {
        let tenths = (value * 10.0).round() as i64;
        let sign = match tenths {
            ..0 => "\u{2212}",
            0 => "",
            _ if value.is_sign_positive() => "+",
            _ => "",
        };
        let magnitude = tenths.unsigned_abs();
        format!(
            "{sign}{}{}{}",
            magnitude / 10,
            self.text(Text::DecimalSeparator),
            magnitude % 10
        )
    }

    /// A gate as one sentence.
    pub fn gate_text(&self, gate: &Gate) -> String {
        match gate {
            Gate::TooLong { limit } => {
                self.text_with(Text::GateTooLong, &[("limit", &limit.to_string())])
            }
            Gate::NoEncoder(codec) => {
                self.text_with(Text::GateNoEncoder, &[("codec", codec.name())])
            }
            Gate::Reframed { cut, output } => self.text_with(
                Text::GateReframed,
                &[("cut", cut.code()), ("output", output.code())],
            ),
            Gate::CaptionsOff => self.text(Text::GateCaptionsOff).into_owned(),
            Gate::MissingMedia(n) => {
                self.text_with(Text::GateMissingMedia, &[("n", &n.to_string())])
            }
            Gate::Silent => self.text(Text::GateSilent).into_owned(),
            Gate::LoudnessFar { gain } => {
                self.text_with(Text::GateLoudnessFar, &[("gain", &self.tenths(*gain))])
            }
            Gate::PeaksLimited => self.text(Text::GatePeaksLimited).into_owned(),
        }
    }

    /// A measured loudness: `−14.2 LUFS`, or silent.
    pub fn loudness_text(&self, loudness: &MeasuredLoudness) -> String {
        if loudness.is_silent() {
            self.text(Text::RenderSilent).into_owned()
        } else {
            self.text_with(
                Text::RenderLufs,
                &[("value", &self.tenths(loudness.integrated))],
            )
        }
    }

    /// The encoder a render uses, and on which chip.
    pub fn encoder_text(&self, encoder: VideoEncoder) -> String {
        let text = if encoder.is_hardware() {
            Text::RenderEncoderHardware
        } else {
            Text::RenderEncoderSoftware
        };
        self.text_with(text, &[("name", encoder.label())])
    }

    /// A rendered file in one line: size, loudness, encoder.
    pub fn render_file_line(&self, render: &Render) -> String {
        let megabytes = render.size_bytes as f64 / 1_000_000.0;
        let size = self.text_with(
            Text::RenderSize,
            &[("value", &self.tenths(megabytes).replace('+', ""))],
        );
        let loudness = render
            .loudness
            .map(|loudness| self.loudness_text(&loudness))
            .unwrap_or_else(|| self.text(Text::RenderSilent).into_owned());
        let encoder = VideoEncoder::from_name(&render.encoder).map_or_else(
            || render.encoder.clone(),
            |encoder| encoder.label().to_owned(),
        );
        self.text_with(
            Text::RenderLastFile,
            &[
                ("size", &size),
                ("loudness", &loudness),
                ("encoder", &encoder),
            ],
        )
    }
}

/// Renders the outputs of a reviewed cut, one file per target.
pub(crate) struct RenderHandler {
    pub(crate) owner: ProfileId,
    pub(crate) renders: Arc<dyn RenderRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) media: Arc<dyn MediaEngine>,
}

/// One output's share of the job's progress, and its cancel.
struct OutputMonitor<'a> {
    cx: &'a JobContext,
    done: usize,
    total: usize,
}

impl Monitor for OutputMonitor<'_> {
    fn should_stop(&self) -> bool {
        self.cx.should_stop()
    }

    fn progress(&self, fraction: f32) {
        let share = (self.done as f32 + fraction.clamp(0.0, 1.0)) / self.total.max(1) as f32;
        self.cx
            .report_progress(Progress::from_permille((share * 1000.0) as u16));
    }
}

fn media_failure(network: &str, error: &MediaError) -> JobFailure {
    JobFailure::new(JobFailureKind::Media, format!("{network}: {error}"))
}

impl RenderHandler {
    /// Renders one output with `encoder`, then with the software encoder
    /// of the same codec if a hardware one fails (a driver too old, a GPU
    /// busy elsewhere). Returns the encoder that made the file.
    fn render_output(
        &self,
        plan: &RenderPlan,
        preset: &RenderPreset,
        encoders: &Encoders,
        destination: &std::path::Path,
        monitor: &OutputMonitor<'_>,
    ) -> Result<VideoEncoder, MediaError> {
        let (width, height) = preset.dimensions();
        let output = |encoder| Output {
            size: FrameSize::new(width, height),
            fps: (FPS, 1),
            encoder,
            video_bitrate: preset.bitrate.kbps() * 1000,
            audio_bitrate: AUDIO_BITRATE,
        };
        let target = LoudnessTarget {
            integrated: preset.loudness.lufs() as f32,
            true_peak: (TRUE_PEAK_CEILING - CODEC_HEADROOM) as f32,
        };
        let best = encoders.best(preset.codec)?;
        match self
            .media
            .render(plan, &output(best), Some(target), destination, monitor)
        {
            Err(MediaError::Failed { .. }) if best.is_hardware() => {
                let software = encoders.software(preset.codec)?;
                self.media
                    .render(plan, &output(software), Some(target), destination, monitor)?;
                Ok(software)
            }
            result => result.map(|()| best),
        }
    }
}

impl JobHandler for RenderHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: RenderPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let mut checkpoint: RenderCheckpoint = match cx.checkpoint() {
            Some(text) => parse(text)?,
            None => RenderCheckpoint::default(),
        };
        let total = payload.outputs.len();
        let encoders = self
            .media
            .encoders()
            .map_err(|error| media_failure("ffmpeg", &error))?;
        for order in &payload.outputs {
            if checkpoint.done.contains(&order.account) {
                continue;
            }
            if cx.should_stop() {
                return Ok(());
            }
            let preset = order.preset()?;
            let plan = payload
                .plans
                .iter()
                .find(|plan| plan.aspect == order.aspect)
                .ok_or_else(|| JobFailure::unexpected("no plan for the output's frame shape"))?;
            let destination = self.files.path(project, &order.file);
            let monitor = OutputMonitor {
                cx,
                done: checkpoint.done.len(),
                total,
            };
            let encoder =
                match self.render_output(&plan.plan, &preset, &encoders, &destination, &monitor) {
                    Ok(encoder) => encoder,
                    Err(MediaError::Cancelled) => return Ok(()),
                    Err(error) => return Err(media_failure(&order.network, &error)),
                };
            let loudness = self
                .media
                .measure_loudness(&destination, &monitor)
                .ok()
                .map(measured);
            let size_bytes = std::fs::metadata(&destination)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            let render = Render {
                id: RenderId::new(),
                owner: self.owner,
                project,
                account: id(&order.account)?,
                network: order
                    .network
                    .parse()
                    .map_err(|_| JobFailure::unexpected("unknown network"))?,
                preset,
                file: order.file.clone(),
                encoder: encoder.name().to_owned(),
                duration: plan.plan.duration().max(Duration::from_millis(1)),
                size_bytes,
                loudness,
                cut: plan.cut.clone(),
                rendered_at: SystemTime::now(),
            };
            self.renders
                .save_render(&render)
                .map_err(|error| JobFailure::unexpected(error.to_string()))?;
            checkpoint.done.push(order.account.clone());
            let progress = Progress::of(checkpoint.done.len() as u64, total as u64);
            cx.save_checkpoint(to_json(&checkpoint), progress)
                .map_err(|error| JobFailure::unexpected(error.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    use bardo_domain::{
        CaptionStyle, JobState, NetworkAccount, NetworkAccountDraft, VideoCodec, VideoProject,
    };
    use bardo_media::ffmpeg::ClipSource;

    use super::*;
    use crate::EditAction;
    use crate::editor::testing::loudness;
    use crate::scenes::tests::{Harness, done, wait_done};

    fn account(
        app: &Bardo,
        project: &VideoProject,
        network: Network,
        draft: NetworkAccountDraft,
    ) -> NetworkAccount {
        app.add_network_account(
            project.channel,
            network,
            NetworkAccountDraft {
                handle: "archives".into(),
                ..draft
            },
        )
        .unwrap()
    }

    /// A drawn project with a YouTube and a Kick account.
    fn rendered_project(
        h: &Harness,
        app: &Bardo,
    ) -> (VideoProject, NetworkAccount, NetworkAccount) {
        let (project, _) = h.drawn_project(app);
        let youtube = account(
            app,
            &project,
            Network::YouTube,
            NetworkAccountDraft::default(),
        );
        let kick = account(app, &project, Network::Kick, NetworkAccountDraft::default());
        (project, youtube, kick)
    }

    pub(crate) fn checked(app: &Bardo, project: VideoProjectId) -> RenderReview {
        let review = app.render_review(project).unwrap();
        let found = app.render_checks(&review).run().unwrap();
        review.checked(&found)
    }

    /// Checks the project's cut and renders every account it can.
    pub(crate) fn render_all(app: &Bardo, project: VideoProjectId) {
        let review = checked(app, project);
        let chosen = review.renderable();
        done(app, app.start_render(&review, &chosen).unwrap());
    }

    fn target(review: &RenderReview, account: NetworkAccountId) -> &RenderTarget {
        review
            .targets
            .iter()
            .find(|target| target.account == account)
            .unwrap()
    }

    #[test]
    fn the_review_lists_every_account_and_holds_rendering_until_checked() {
        let h = Harness::new();
        let app = h.start();
        let (project, youtube, kick) = rendered_project(&h, &app);

        let review = app.render_review(project.id).unwrap();
        assert_eq!(
            review
                .targets
                .iter()
                .map(|target| target.network)
                .collect::<Vec<_>>(),
            [Network::YouTube, Network::Kick]
        );
        let shown = target(&review, youtube.id);
        assert_eq!(shown.preset, youtube.render_preset());
        assert_eq!(shown.handle, "archives");
        assert_eq!(shown.last, None);
        assert!(review.cut.duration > Duration::ZERO);
        assert_eq!(review.cut.aspect, AspectRatio::Landscape);
        assert!(review.cut.captions_shown);
        assert_eq!(review.cut.mix, None);
        assert!(!review.is_checked());
        assert!(review.renderable().is_empty(), "nothing before the checks");
        assert!(matches!(
            app.start_render(&review, &[youtube.id]),
            Err(RenderError::NotChecked)
        ));

        let review = checked(&app, project.id);
        assert!(review.is_checked());
        assert_eq!(
            review.cut.mix,
            Some(MeasuredLoudness {
                integrated: -20.0,
                true_peak: -8.0
            })
        );
        assert_eq!(review.renderable(), [youtube.id, kick.id]);
        // The cut is 16:9: the vertical Short reframes it, Kick does not.
        assert_eq!(
            target(&review, youtube.id).gates,
            [Gate::Reframed {
                cut: AspectRatio::Landscape,
                output: AspectRatio::Vertical
            }]
        );
        assert!(target(&review, kick.id).gates.is_empty());
        assert_eq!(
            target(&review, kick.id).encoder,
            Some(VideoEncoder::OpenH264)
        );
        assert!(review.gates.is_empty());
    }

    #[test]
    fn gates_block_a_target_over_its_limit_or_without_an_encoder() {
        let h = Harness::new();
        let app = h.start();
        let (project, youtube, kick) = rendered_project(&h, &app);
        let x = account(
            &app,
            &project,
            Network::X,
            NetworkAccountDraft {
                max_duration: "1".into(),
                ..NetworkAccountDraft::default()
            },
        );
        *h.media.encoders.lock().unwrap() = Some(Encoders {
            working: vec![VideoEncoder::OpenH264],
        });
        let tiktok = account(
            &app,
            &project,
            Network::TikTok,
            NetworkAccountDraft {
                codec: Some(VideoCodec::Hevc),
                ..NetworkAccountDraft::default()
            },
        );

        let review = checked(&app, project.id);
        assert!(target(&review, x.id).gates.contains(&Gate::TooLong {
            limit: MaxDuration::from_seconds(1).unwrap()
        }));
        assert!(
            target(&review, tiktok.id)
                .gates
                .contains(&Gate::NoEncoder(VideoCodec::Hevc))
        );
        assert_eq!(target(&review, tiktok.id).encoder, None);
        assert_eq!(review.renderable(), [youtube.id, kick.id]);
        assert!(matches!(
            app.start_render(&review, &[youtube.id, x.id]),
            Err(RenderError::Blocked)
        ));
        assert!(matches!(
            app.start_render(&review, &[]),
            Err(RenderError::NothingChosen)
        ));
        assert!(app.render_job(project.id).is_none(), "nothing was queued");
    }

    #[test]
    fn the_cut_gates_flag_captions_off_and_a_silent_mix() {
        let h = Harness::new();
        let app = h.start();
        let (project, _, _) = rendered_project(&h, &app);
        let mut editor = app.open_editor(project.id).unwrap();
        app.edit(&mut editor, EditAction::ShowCaptions(false))
            .unwrap();
        *h.media.mix.lock().unwrap() = Some(loudness(f32::NEG_INFINITY, f32::NEG_INFINITY));

        let review = checked(&app, project.id);
        assert!(!review.cut.captions_shown);
        assert_eq!(review.gates, [Gate::CaptionsOff, Gate::Silent]);
        assert_eq!(review.renderable().len(), 2, "warnings block nothing");
    }

    #[test]
    fn rendering_makes_one_file_per_chosen_target_in_its_preset() {
        let h = Harness::new();
        let app = h.start();
        let (project, youtube, kick) = rendered_project(&h, &app);
        let review = checked(&app, project.id);

        let job = done(
            &app,
            app.start_render(&review, &[youtube.id, kick.id]).unwrap(),
        );
        assert_eq!(job.kind(), JobKind::Render);

        let calls = h.media.renders();
        assert_eq!(calls.len(), 2);
        let short = &calls[0];
        assert_eq!(short.output.size, FrameSize::new(1080, 1920));
        assert_eq!(short.output.encoder, VideoEncoder::OpenH264);
        assert_eq!(short.output.video_bitrate, 12_000_000);
        assert_eq!(short.output.fps, (FPS, 1));
        let target = short.loudness.unwrap();
        // Half a dB under the ceiling, for what AAC adds.
        assert_eq!((target.integrated, target.true_peak), (-14.0, -1.5));
        assert!(short.destination.ends_with("render-youtube.mp4"));
        // The originals, never the proxies.
        assert!(short.plan.video.iter().all(|clip| match &clip.source {
            ClipSource::Still(path) | ClipSource::Video(path) =>
                !path.to_string_lossy().contains("proxy-"),
            ClipSource::Black => true,
        }));
        let wide = &calls[1];
        assert_eq!(wide.output.size, FrameSize::new(1920, 1080));
        assert_eq!(wide.output.video_bitrate, 8_000_000);
        assert_ne!(short.plan, wide.plan, "each frame shape has its own plan");

        let renders = app.renders(project.id).unwrap();
        assert_eq!(renders.len(), 2);
        assert_eq!(renders[0].account, youtube.id);
        assert_eq!(renders[0].file, "render-youtube.mp4");
        assert_eq!(renders[0].encoder, "libopenh264");
        assert_eq!(renders[0].preset, youtube.render_preset());
        assert_eq!(renders[0].loudness_on_target(), Some(true));
        assert_eq!(renders[1].network, Network::Kick);

        let again = app.render_review(project.id).unwrap();
        assert!(again.targets.iter().all(|target| target.last_current));
        let summary = app.render_summary(project.id).unwrap();
        assert_eq!((summary.current, summary.outdated), (2, 0));
    }

    #[test]
    fn a_cut_changed_after_the_review_is_refused_and_outdates_its_files() {
        let h = Harness::new();
        let app = h.start();
        let (project, youtube, _) = rendered_project(&h, &app);
        let review = checked(&app, project.id);
        done(&app, app.start_render(&review, &[youtube.id]).unwrap());

        let mut editor = app.open_editor(project.id).unwrap();
        app.edit(
            &mut editor,
            EditAction::SetCaptionStyle(CaptionStyle::Punch),
        )
        .unwrap();
        assert!(matches!(
            app.start_render(&review, &[youtube.id]),
            Err(RenderError::CutChanged)
        ));
        let again = app.render_review(project.id).unwrap();
        let shown = target(&again, youtube.id);
        assert!(shown.last.is_some() && !shown.last_current);
        let summary = app.render_summary(project.id).unwrap();
        assert_eq!((summary.current, summary.outdated), (0, 1));
    }

    #[test]
    fn the_review_reads_in_the_interface_language() {
        let h = Harness::new();
        let app = h.start();
        assert_eq!(
            app.gate_text(&Gate::LoudnessFar { gain: 6.34 }),
            "The mix is +6.3 dB from this network's target; the render changes it that much."
        );
        assert_eq!(
            app.loudness_text(&MeasuredLoudness {
                integrated: -14.16,
                true_peak: -1.5
            }),
            "\u{2212}14.2 LUFS"
        );
        assert_eq!(
            app.loudness_text(&MeasuredLoudness {
                integrated: f64::NEG_INFINITY,
                true_peak: f64::NEG_INFINITY
            }),
            "Silent"
        );
        assert_eq!(
            app.encoder_text(VideoEncoder::Nvenc),
            "NVIDIA NVENC, on the graphics card"
        );
        assert_eq!(
            app.gate_text(&Gate::Reframed {
                cut: AspectRatio::Landscape,
                output: AspectRatio::Vertical
            }),
            "The cut is 16:9; this file is 9:16, each clip through its crop window (centered unless you moved it)."
        );
    }

    #[test]
    fn findings_about_an_earlier_cut_leave_the_review_unchecked() {
        let h = Harness::new();
        let app = h.start();
        let (project, _, _) = rendered_project(&h, &app);
        let review = app.render_review(project.id).unwrap();
        let found = app.render_checks(&review).run().unwrap();
        assert!(review.clone().checked(&found).is_checked());

        let mut editor = app.open_editor(project.id).unwrap();
        app.edit(
            &mut editor,
            EditAction::SetCaptionStyle(CaptionStyle::Punch),
        )
        .unwrap();
        let again = app.render_review(project.id).unwrap();
        assert!(!again.checked(&found).is_checked());
    }

    #[test]
    fn a_failed_render_resumes_without_redoing_finished_files() {
        let h = Harness::new();
        let app = h.start();
        let (project, youtube, _) = rendered_project(&h, &app);
        let tiktok = account(
            &app,
            &project,
            Network::TikTok,
            NetworkAccountDraft {
                codec: Some(VideoCodec::Hevc),
                ..NetworkAccountDraft::default()
            },
        );
        h.media
            .broken_encoders
            .lock()
            .unwrap()
            .push(VideoEncoder::Kvazaar);
        let review = checked(&app, project.id);
        let id = app.start_render(&review, &[youtube.id, tiktok.id]).unwrap();
        let job = wait_done(&app, id);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::Media);
        assert!(job.failure().unwrap().detail.starts_with("tiktok: "));
        assert_eq!(app.renders(project.id).unwrap().len(), 1);
        assert_eq!(render_job_files(&job), (1, 2));

        h.media.broken_encoders.lock().unwrap().clear();
        app.retry_job(id).unwrap();
        done(&app, id);
        let made: Vec<_> = h
            .media
            .renders()
            .into_iter()
            .filter(|call| call.made)
            .map(|call| call.output.encoder)
            .collect();
        assert_eq!(made, [VideoEncoder::OpenH264, VideoEncoder::Kvazaar]);
        assert_eq!(app.renders(project.id).unwrap().len(), 2);
        assert_eq!(
            render_job_files(&app.render_job(project.id).unwrap()),
            (2, 2)
        );
    }

    #[test]
    fn a_cancelled_render_saves_nothing_and_resumes() {
        let h = Harness::new();
        let app = h.start();
        let (project, youtube, _) = rendered_project(&h, &app);
        let review = checked(&app, project.id);
        h.media.render_hold.store(true, Ordering::SeqCst);
        let id = app.start_render(&review, &[youtube.id]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.render_job(project.id).unwrap().state() != JobState::Running {
            assert!(Instant::now() < deadline, "the render never started");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(matches!(
            app.start_render(&review, &[youtube.id]),
            Err(RenderError::AlreadyRendering)
        ));

        app.cancel_job(id).unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Cancelled);
        assert!(app.renders(project.id).unwrap().is_empty());

        h.media.render_hold.store(false, Ordering::SeqCst);
        app.retry_job(id).unwrap();
        done(&app, id);
        assert_eq!(app.renders(project.id).unwrap().len(), 1);
    }

    #[test]
    fn a_failing_gpu_encoder_falls_back_to_software() {
        let h = Harness::new();
        let app = h.start();
        let (project, youtube, _) = rendered_project(&h, &app);
        *h.media.encoders.lock().unwrap() = Some(Encoders {
            working: vec![VideoEncoder::Nvenc, VideoEncoder::OpenH264],
        });
        h.media
            .broken_encoders
            .lock()
            .unwrap()
            .push(VideoEncoder::Nvenc);
        let review = checked(&app, project.id);
        assert_eq!(
            target(&review, youtube.id).encoder,
            Some(VideoEncoder::Nvenc)
        );
        done(&app, app.start_render(&review, &[youtube.id]).unwrap());
        assert_eq!(app.renders(project.id).unwrap()[0].encoder, "libopenh264");
    }

    #[test]
    fn a_project_without_a_cut_has_nothing_to_review() {
        let h = Harness::new();
        let app = h.start();
        let project = h.narrated_project(&app);
        assert!(matches!(
            app.render_review(project.id),
            Err(RenderError::NoCut)
        ));
        let summary = app.render_summary(project.id).unwrap();
        assert!(!summary.has_cut);
    }

    #[test]
    fn plans_have_stable_fingerprints() {
        let plan = RenderPlan {
            video: Vec::new(),
            audio: Vec::new(),
            captions: None,
        };
        assert_eq!(fingerprint(&plan), fingerprint(&plan.clone()));
        assert_eq!(fingerprint(&plan).len(), 16);
        let other = RenderPlan {
            captions: Some(bardo_media::ffmpeg::CaptionTrack {
                style: CaptionStyle::Boxed,
                lines: Vec::new(),
            }),
            ..plan.clone()
        };
        assert_ne!(fingerprint(&plan), fingerprint(&other));
    }

    #[test]
    fn a_payload_round_trips_with_its_plans() {
        let order = OutputOrder {
            account: NetworkAccountId::new().to_string(),
            network: "x".into(),
            file: render_file(Network::X),
            aspect: "9:16".into(),
            resolution: 720,
            codec: "h264".into(),
            bitrate_kbps: 6_000,
            max_duration_secs: 140,
            loudness_tenths: -140,
        };
        assert_eq!(order.preset().unwrap(), Network::X.render_preset());
        let payload = RenderPayload {
            project: VideoProjectId::new().to_string(),
            outputs: vec![order],
            plans: Vec::new(),
        };
        let back: RenderPayload = parse(&to_json(&payload)).unwrap();
        assert_eq!(back, payload);
    }
}
