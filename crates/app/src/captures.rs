//! Captures (#189, docs/spec/performance-capture.md): the user records
//! themselves reading the project's script on camera and imports the
//! video. Its speech becomes the project's narration, in the persona's
//! voice (ElevenLabs Voice Changer, which keeps the performer's timing) or
//! in the user's own (cleaned by the Voice Isolator), and the recording is
//! split into takes the take models perform later.
//!
//! Choosing a file reads it with ffprobe first (`CaptureReader`, off the
//! UI thread), so the import card can show what it holds and what
//! importing it would cost before anything is paid. Only the user's word
//! that the recording shows only them lets it in; the sentence they ticked
//! is kept with the capture.
//!
//! Importing runs as a job: copy the video into the project folder,
//! extract its sound, transcribe the sound and check it follows the script
//! (before the paid voice step), voice it in parts, join them, time the
//! words against the script and cut the takes. Each paid result is written
//! in the project folder before its cost is recorded, so a cancelled,
//! failed or interrupted job resumes without paying for it again. Only the
//! sound leaves the computer; the video stays in the project folder.
//!
//! A capture lives as long as its narration: generating or importing
//! another narration removes it (`forget_capture`), and removing it keeps
//! the narration.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bardo_domain::{
    Alignment, AlignmentRequest, ApiKey, ApprovalBlock, BoundaryError, BoundaryMove, Capture,
    CaptureConfirmation, CaptureId, CaptureProblem, CaptureRepository, CaptureVideo, CaptureVoice,
    CharTiming, CostPurpose, GenerationPresets, HeardWord, Job, JobFailure, JobFailureKind, JobId,
    JobKind, JobRepository, MIN_SCRIPT_COVERAGE, Metered, Narration, NarrationId,
    NarrationRepository, NarrationSource, Persona, ProfileId, Progress, ProjectFiles, Provider,
    ProviderFailure, RecordingFacts, RepositoryError, ScriptText, SecretStore, SpeechAligner,
    SpeechTranscriber, TakeMode, TakeSource, Takes, TranscriptionRequest, VideoProjectId,
    VoiceChangeRequest, VoiceChanger, VoiceFlag, VoiceIsolator, VoiceRef, WordTimings, audio_parts,
    script_coverage,
};
use bardo_media::ffmpeg::{MediaError, Monitor};
use bardo_media::{MediaEngine, audio, mp3};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, KeyState, Text};

/// Every provider a capture calls: ElevenLabs.
const PROVIDER: Provider = Provider::ElevenLabs;
/// More parts than a capture is ever voiced in: parts are cut in the second
/// half of the provider's longest (about five minutes), so 15 minutes take
/// seven at most.
const MAX_PARTS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("video project not found")]
    ProjectNotFound,
    /// The capture reads the script: write one first.
    #[error("the project has no script yet")]
    NoScript,
    /// Neither the project nor its channel has a persona whose voice the
    /// speech could take.
    #[error("no persona reads this project")]
    NoPersona,
    /// The persona's voice is flagged: fix it on the personas screen.
    #[error("the persona's voice is flagged ({0:?})")]
    VoiceFlagged(VoiceFlag),
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// A narration of the project is being made or imported.
    #[error("a narration is already being made for this project")]
    Busy,
    /// The import would reach the provider's budget; the card asks before
    /// starting it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    /// The user has not confirmed the recording shows only them.
    #[error("the recording was not confirmed")]
    NotConfirmed,
    /// The chosen file is gone or cannot be read.
    #[error("the recording cannot be read")]
    Unreadable,
    #[error("ffmpeg is not installed")]
    NoFfmpeg,
    /// ffmpeg reads the file, and it cannot be a capture.
    #[error("{0}")]
    Unsupported(CaptureProblem),
    /// The project has no capture of its current narration.
    #[error("the project has no capture")]
    NoCapture,
    /// The take's control video is not built from the take as it is now.
    #[error("the take's control video is not ready")]
    ControlNotReady,
    /// The take cannot go to a take model as it is.
    #[error("the take cannot be approved ({0:?})")]
    NotApprovable(ApprovalBlock),
    /// The set picture is gone, or not a PNG or JPEG.
    #[error("the set picture cannot be read")]
    SetUnreadable,
    /// No take model is offered by that name.
    #[error("the take model is not offered")]
    TakeModelNotOffered,
    /// The take cannot go to its take model yet.
    #[error("the take cannot be performed yet ({0:?})")]
    Performed(crate::PerformedBlock),
    /// A prompt or a performed take of it is being made.
    #[error("the take is busy")]
    TakeBusy,
    /// Higgsfield took a request of it that was stopped: checking again
    /// comes before paying for another.
    #[error("a take request is pending")]
    TakePending,
    /// The take's prompt cannot go to its take model.
    #[error("the take prompt cannot be used: {0}")]
    Prompt(bardo_domain::TakePromptError),
    /// The take model makes no drafts.
    #[error("the take model makes no drafts")]
    NoDrafts,
    /// A final needs the draft approved first.
    #[error("the draft is not approved")]
    DraftNotApproved,
    /// Nothing made for the take to review or use.
    #[error("nothing was made for the take")]
    NoTakeResult,
    #[error(transparent)]
    Template(#[from] crate::TemplateError),
    #[error(transparent)]
    Boundary(#[from] BoundaryError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl CaptureError {
    /// What the narration panel or the Capture stage says.
    pub fn message(&self) -> Text {
        match self {
            CaptureError::ProjectNotFound => Text::ProjectNotFound,
            CaptureError::NoScript => Text::NarrationNoScript,
            CaptureError::NoPersona => Text::CaptureNoPersona,
            CaptureError::VoiceFlagged(flag) => Text::NarrationVoiceFlagged(*flag),
            // Only the performed clips of a capture ask Claude and Higgsfield.
            CaptureError::MissingKey(Provider::Claude) => Text::PerformedMissingClaudeKey,
            CaptureError::MissingKey(Provider::Higgsfield) => Text::PerformedMissingHiggsfieldKey,
            CaptureError::MissingKey(_) => Text::NarrationMissingKey,
            CaptureError::Busy => Text::NarrationBusy,
            CaptureError::OverBudget(_) => Text::BudgetReachedTitle,
            CaptureError::NotConfirmed => Text::CaptureNotConfirmed,
            CaptureError::Unreadable => Text::RecordingUnreadable,
            CaptureError::NoFfmpeg => Text::EditorFfmpegMissing,
            CaptureError::Unsupported(problem) => Text::CaptureProblem(*problem),
            CaptureError::NoCapture => Text::CaptureMissing,
            CaptureError::ControlNotReady => Text::ControlNotReady,
            CaptureError::NotApprovable(ApprovalBlock::SeveralFaces) => {
                Text::ControlSeveralFacesBlocked
            }
            CaptureError::NotApprovable(ApprovalBlock::Uncovered) => Text::ControlUncoveredBlocked,
            CaptureError::NotApprovable(ApprovalBlock::Length(_)) => Text::ControlLengthBlocked,
            CaptureError::SetUnreadable => Text::SetUnreadable,
            CaptureError::TakeModelNotOffered => Text::TakeModelGone,
            CaptureError::Performed(block) => Text::PerformedBlocked(*block),
            CaptureError::TakeBusy => Text::PerformedBusy,
            CaptureError::TakePending => Text::PerformedPending,
            CaptureError::Prompt(error) => Text::TakePromptInvalid(*error),
            CaptureError::NoDrafts | CaptureError::NoTakeResult => Text::PerformedNothing,
            CaptureError::DraftNotApproved => Text::PerformedDraftFirst,
            CaptureError::Template(error) => error.message(),
            CaptureError::Boundary(_) => Text::CaptureNoCutPoint,
            CaptureError::Repository(_) => Text::CaptureNotSaved,
        }
    }
}

/// A recording the user chose, read and checked, ready to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenCapture {
    pub path: PathBuf,
    /// The file's name as the user has it.
    pub file_name: String,
    pub video: CaptureVideo,
    /// Its size in bytes, so the job can tell it was not replaced.
    pub size: u64,
}

impl ChosenCapture {
    /// Seconds of sound each provider bills, rounded up.
    fn billed_seconds(&self) -> u64 {
        billed_seconds(self.video.duration)
    }
}

fn billed_seconds(duration: Duration) -> u64 {
    duration.as_millis().div_ceil(1_000) as u64
}

/// Reads recordings the user chooses. It holds no UI state, so the screen
/// moves it to a background thread for the file chosen.
#[derive(Clone)]
pub struct CaptureReader {
    media: Arc<dyn MediaEngine>,
}

impl CaptureReader {
    /// Reads the file at `path` with ffprobe and checks it can be a
    /// capture. Blocks while ffprobe runs: call it off the UI thread.
    pub fn open(&self, path: &Path) -> Result<ChosenCapture, CaptureError> {
        let size = std::fs::metadata(path)
            .ok()
            .filter(|metadata| metadata.is_file())
            .ok_or(CaptureError::Unreadable)?
            .len();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        // The extension decides before ffprobe is asked.
        if let Err(problem @ CaptureProblem::NotMp4OrMov) = CaptureVideo::check(
            &file_name,
            &RecordingFacts {
                duration: Duration::ZERO,
                video: None,
                has_audio: false,
            },
        ) {
            return Err(CaptureError::Unsupported(problem));
        }
        let facts = recording_facts(self.media.as_ref(), path).map_err(|error| match error {
            MediaError::NotFound { .. } => CaptureError::NoFfmpeg,
            MediaError::Io(_) | MediaError::Spawn { .. } => CaptureError::Unreadable,
            // ffprobe does not read it as video.
            _ => CaptureError::Unsupported(CaptureProblem::NoVideo),
        })?;
        let video = CaptureVideo::check(&file_name, &facts).map_err(CaptureError::Unsupported)?;
        Ok(ChosenCapture {
            path: path.to_owned(),
            file_name,
            video,
            size,
        })
    }
}

/// What ffprobe finds in the recording at `path`.
fn recording_facts(media: &dyn MediaEngine, path: &Path) -> Result<RecordingFacts, MediaError> {
    let info = media.probe(path)?;
    Ok(RecordingFacts {
        duration: info.duration,
        video: info
            .video
            .map(|video| (video.width, video.height, video.fps())),
        has_audio: info.audio.is_some(),
    })
}

/// Whose voice the user picked on the import card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureVoiceChoice {
    /// The project's narrator: its own persona, else the channel's.
    Persona,
    /// The user's own voice, cleaned.
    Own,
}

/// The calls importing `chosen` makes: the script check, the voice and
/// the timing of the words, each billed per second of sound.
fn capture_calls(chosen: &ChosenCapture, voice: CaptureVoiceChoice) -> Vec<PlannedCall> {
    let seconds = chosen.billed_seconds();
    let call = |purpose, model| {
        PlannedCall::new(PROVIDER, purpose, 1)
            .with_model(model)
            .with_audio_seconds(seconds)
    };
    let voicing = match voice {
        CaptureVoiceChoice::Persona => call(
            CostPurpose::VoiceChange,
            bardo_ai::elevenlabs::VOICE_CHANGER_MODEL,
        ),
        CaptureVoiceChoice::Own => call(
            CostPurpose::VoiceIsolation,
            bardo_ai::elevenlabs::ISOLATION_MODEL,
        ),
    };
    vec![
        call(
            CostPurpose::ScriptCheck,
            bardo_ai::elevenlabs::TRANSCRIPTION_MODEL,
        ),
        voicing,
        call(
            CostPurpose::NarrationAlignment,
            bardo_ai::elevenlabs::ALIGNMENT_MODEL,
        ),
    ]
}

/// The persona voice a capture job converts into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PayloadVoice {
    provider: String,
    id: String,
    name: String,
    stability: u8,
    similarity: u8,
    style: u8,
    speed: u8,
}

/// The capture job's payload: the recording as it was chosen, the user's
/// confirmation, the voice and the script it reads, and the ids of the
/// capture and narration it makes (their files are named after them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CapturePayload {
    pub(crate) project: String,
    capture: String,
    narration: String,
    source: PathBuf,
    file_name: String,
    size: u64,
    width: u32,
    height: u32,
    fps_milli: u32,
    duration_ms: u64,
    confirmation: String,
    /// Unix time in milliseconds.
    confirmed_at: u64,
    /// `None` for the user's own voice.
    voice: Option<PayloadVoice>,
    /// The ISO 639-1 code of the channel's language.
    language: String,
    text: String,
    /// The mode the takes start in: the first of the channel's take model
    /// (#192). Missing in jobs queued before, which start in Recording.
    #[serde(default)]
    mode: Option<String>,
}

impl CapturePayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a capture payload serializes")
    }

    pub(crate) fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid capture payload: {e}")))
    }

    fn capture(&self) -> Result<CaptureId, JobFailure> {
        uuid::Uuid::parse_str(&self.capture)
            .map(CaptureId::from)
            .map_err(unexpected)
    }

    fn voice(&self) -> Result<CaptureVoice, JobFailure> {
        let Some(voice) = &self.voice else {
            return Ok(CaptureVoice::Own);
        };
        let provider: Provider = voice.provider.parse().map_err(unexpected)?;
        Ok(CaptureVoice::Persona {
            voice: VoiceRef::new(provider, &voice.id, &voice.name).map_err(unexpected)?,
            presets: GenerationPresets {
                stability: voice.stability,
                similarity: voice.similarity,
                style: voice.style,
                speed: voice.speed,
            },
        })
    }

    fn video(&self) -> CaptureVideo {
        CaptureVideo {
            width: self.width,
            height: self.height,
            fps_milli: self.fps_milli,
            duration: Duration::from_millis(self.duration_ms),
        }
    }

    /// The copy's extension: the original's, in lower case.
    fn extension(&self) -> String {
        self.file_name
            .rsplit_once('.')
            .map_or_else(String::new, |(_, extension)| extension.to_ascii_lowercase())
    }
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

fn provider_failure(failure: ProviderFailure) -> JobFailure {
    JobFailure::new(
        failure.kind.into(),
        format!("ElevenLabs: {}", failure.detail),
    )
}

/// The capture's video in the project folder.
fn video_file(capture: CaptureId, extension: &str) -> String {
    format!("capture-{capture}.{extension}")
}

/// Files a capture job keeps between attempts and removes once it saved.
fn sound_file(capture: CaptureId) -> String {
    format!("capture-{capture}.wav")
}

fn heard_file(capture: CaptureId) -> String {
    format!("capture-{capture}.heard.json")
}

fn part_audio(capture: CaptureId, index: usize) -> String {
    format!("capture-{capture}.part{:03}.mp3", index + 1)
}

fn part_record(capture: CaptureId, index: usize) -> String {
    format!("capture-{capture}.part{:03}.json", index + 1)
}

fn aligned_file(capture: CaptureId) -> String {
    format!("capture-{capture}.aligned.json")
}

/// Microseconds, as the cached answers store times.
fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// What the speech-to-text heard, kept until the capture is saved.
#[derive(Debug, Serialize, Deserialize)]
struct HeardRecord {
    model: String,
    /// Text, start and end in microseconds.
    words: Vec<(String, u64, u64)>,
}

impl HeardRecord {
    fn words(&self) -> Vec<HeardWord> {
        self.words
            .iter()
            .map(|(text, start, end)| HeardWord {
                text: text.clone(),
                start: Duration::from_micros(*start),
                end: Duration::from_micros(*end),
            })
            .collect()
    }
}

/// The model that voiced a part, saved after its audio.
#[derive(Debug, Serialize, Deserialize)]
struct PartRecord {
    model: String,
}

/// The aligner's character timings, kept until the capture is saved.
#[derive(Debug, Serialize, Deserialize)]
struct AlignedRecord {
    model: String,
    /// Text, start and end in microseconds.
    chars: Vec<(String, u64, u64)>,
}

impl AlignedRecord {
    fn alignment(&self) -> Alignment {
        Alignment {
            chars: self
                .chars
                .iter()
                .map(|(text, start, end)| CharTiming {
                    text: text.clone(),
                    start: Duration::from_micros(*start),
                    end: Duration::from_micros(*end),
                })
                .collect(),
        }
    }
}

/// Stops ffmpeg when the job is cancelled.
struct JobMonitor<'a>(Option<&'a JobContext>);

impl Monitor for JobMonitor<'_> {
    fn should_stop(&self) -> bool {
        self.0.is_some_and(JobContext::should_stop)
    }
}

/// Runs capture jobs.
pub(crate) struct CaptureHandler {
    pub(crate) owner: ProfileId,
    pub(crate) captures: Arc<dyn CaptureRepository>,
    pub(crate) narrations: Arc<dyn NarrationRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) transcriber: Arc<dyn SpeechTranscriber>,
    pub(crate) voice_changer: Arc<dyn VoiceChanger>,
    pub(crate) voice_isolator: Arc<dyn VoiceIsolator>,
    pub(crate) aligner: Arc<dyn SpeechAligner>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
    /// Where the takes' face mesh jobs are queued once they exist.
    pub(crate) jobs: Arc<dyn JobRepository>,
}

impl JobHandler for CaptureHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.import(payload, cx.id(), Some(cx))
    }
}

/// What a capture job works on, from its payload.
struct Work {
    project: VideoProjectId,
    capture: CaptureId,
    narration: NarrationId,
    text: ScriptText,
    voice: CaptureVoice,
    job: JobId,
}

impl CaptureHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, PROVIDER)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(JobFailureKind::MissingKey, "no ElevenLabs key is saved")
            })
    }

    fn record(
        &self,
        work: &Work,
        purpose: CostPurpose,
        model: &str,
        sound: Duration,
    ) -> Result<(), JobFailure> {
        self.costs.record_for_project(
            PaidCall {
                provider: PROVIDER,
                model,
                purpose,
                usage: Metered::audio_seconds(billed_seconds(sound)),
                job: Some(work.job),
                reported: None,
            },
            work.project,
        );
        Ok(())
    }

    /// Imports the recording `job` asks for. Without a context (tests
    /// replaying a job) nothing reports progress or stops it.
    fn import(
        &self,
        payload: &str,
        job: JobId,
        mut cx: Option<&mut JobContext>,
    ) -> Result<(), JobFailure> {
        let payload = CapturePayload::parse(payload)?;
        let work = Work {
            project: uuid::Uuid::parse_str(&payload.project)
                .map(VideoProjectId::from)
                .map_err(unexpected)?,
            capture: payload.capture()?,
            narration: uuid::Uuid::parse_str(&payload.narration)
                .map(NarrationId::from)
                .map_err(unexpected)?,
            text: ScriptText::new(&payload.text).map_err(|e| unexpected(format!("{e:?}")))?,
            voice: payload.voice()?,
            job,
        };
        let project = work.project;
        // An earlier attempt may have saved both and stopped before the
        // queue recorded the job as done.
        let saved = |job_of: Option<JobId>| job_of == Some(job);
        if self
            .narrations
            .narration(project)
            .map_err(unexpected)?
            .is_some_and(|narration| saved(narration.job))
            && self
                .captures
                .capture(project)
                .map_err(unexpected)?
                .is_some_and(|capture| saved(capture.job))
        {
            self.remove_leftovers(project, work.capture);
            return Ok(());
        }
        let stopped = |cx: &Option<&mut JobContext>| cx.as_ref().is_some_and(|cx| cx.should_stop());
        let checkpoint =
            |cx: &mut Option<&mut JobContext>, step: &str, permille: u16| match cx.as_mut() {
                Some(cx) => cx
                    .save_checkpoint(step, Progress::from_permille(permille))
                    .map_err(unexpected),
                None => Ok(()),
            };

        // Copy the video in, and check it is still the one chosen: a
        // longer one would cost more than the user agreed to.
        let video = video_file(work.capture, &payload.extension());
        if !self.files.exists(project, &video) {
            self.files
                .copy_in(project, &video, &payload.source)
                .map_err(|_| {
                    JobFailure::new(
                        JobFailureKind::Files,
                        format!(
                            "could not copy the recording {}: it was moved or cannot be opened",
                            payload.file_name
                        ),
                    )
                })?;
        }
        let copy = self.files.path(project, &video);
        let same = self.files.size(project, &video).ok() == Some(payload.size)
            && recording_facts(self.media.as_ref(), &copy)
                .ok()
                .and_then(|facts| CaptureVideo::check(&payload.file_name, &facts).ok())
                == Some(payload.video());
        if !same {
            let _ = self.files.remove(project, &video);
            return Err(JobFailure::unexpected(format!(
                "{} is no longer the recording that was chosen",
                payload.file_name
            )));
        }
        if stopped(&cx) {
            return Ok(());
        }
        checkpoint(&mut cx, "copied", 50)?;

        // Its sound on its own: all that leaves the computer.
        let sound_name = sound_file(work.capture);
        if !self.files.exists(project, &sound_name) {
            let monitor = JobMonitor(cx.as_deref());
            match self
                .media
                .extract_sound(&copy, &self.files.path(project, &sound_name), &monitor)
            {
                Ok(()) => {}
                Err(MediaError::Cancelled) => return Ok(()),
                Err(error) => {
                    return Err(JobFailure::new(
                        JobFailureKind::Media,
                        format!("{}: {error}", payload.file_name),
                    ));
                }
            }
        }
        let sound = self.files.read(project, &sound_name).map_err(unexpected)?;
        let length = audio::probe(&sound)
            .map_err(|error| {
                JobFailure::new(
                    JobFailureKind::Media,
                    format!("{}: the sound is not usable ({error})", payload.file_name),
                )
            })?
            .duration;
        checkpoint(&mut cx, "sound", 100)?;

        // Does it follow the script? Checked before the voice is paid for.
        let mut key = None;
        let heard_name = heard_file(work.capture);
        let heard = if self.files.exists(project, &heard_name) {
            let record = self.files.read(project, &heard_name).map_err(unexpected)?;
            serde_json::from_slice::<HeardRecord>(&record)
                .map_err(unexpected)?
                .words()
        } else {
            if stopped(&cx) {
                return Ok(());
            }
            let key = key.insert(self.key()?);
            let transcript = self
                .transcriber
                .transcribe(
                    key,
                    &TranscriptionRequest {
                        audio: &sound,
                        file_name: &sound_name,
                        language: Some(&payload.language),
                    },
                )
                .map_err(provider_failure)?;
            let record = HeardRecord {
                model: transcript.model.clone(),
                words: transcript
                    .words
                    .iter()
                    .map(|word| (word.text.clone(), micros(word.start), micros(word.end)))
                    .collect(),
            };
            self.files
                .write(
                    project,
                    &heard_name,
                    &serde_json::to_vec(&record).map_err(unexpected)?,
                )
                .map_err(unexpected)?;
            self.record(&work, CostPurpose::ScriptCheck, &transcript.model, length)?;
            transcript.words
        };
        let coverage = script_coverage(work.text.as_str(), &heard);
        if coverage < MIN_SCRIPT_COVERAGE {
            return Err(JobFailure::new(
                JobFailureKind::OffScript,
                format!(
                    "the recording says {}% of the script in order; at least {}% is needed",
                    coverage / 10,
                    MIN_SCRIPT_COVERAGE / 10
                ),
            ));
        }
        checkpoint(&mut cx, "checked", 200)?;

        // The voice, in parts the voice changer takes; the user's own is
        // cleaned whole.
        let spans = match &work.voice {
            CaptureVoice::Persona { .. } => {
                audio_parts(length, &heard, self.voice_changer.max_length())
            }
            CaptureVoice::Own => vec![Duration::ZERO..length],
        };
        for (index, span) in spans.iter().enumerate() {
            if self
                .files
                .exists(project, &part_record(work.capture, index))
                && self.files.exists(project, &part_audio(work.capture, index))
            {
                continue;
            }
            if stopped(&cx) {
                return Ok(());
            }
            let key = match &key {
                Some(key) => key,
                None => key.insert(self.key()?),
            };
            let piece = if spans.len() == 1 {
                sound.clone()
            } else {
                audio::wav_slice(&sound, span.clone()).map_err(unexpected)?
            };
            let file_name = format!("capture-{}.part{:03}.wav", work.capture, index + 1);
            let request = VoiceChangeRequest {
                audio: &piece,
                file_name: &file_name,
            };
            let (changed, purpose) = match &work.voice {
                CaptureVoice::Persona { voice, presets } => (
                    self.voice_changer.change(key, voice, *presets, &request),
                    CostPurpose::VoiceChange,
                ),
                CaptureVoice::Own => (
                    self.voice_isolator.isolate(key, &request),
                    CostPurpose::VoiceIsolation,
                ),
            };
            let changed = changed.map_err(provider_failure)?;
            self.files
                .write(project, &part_audio(work.capture, index), &changed.audio)
                .map_err(unexpected)?;
            let record = PartRecord {
                model: changed.model.clone(),
            };
            self.files
                .write(
                    project,
                    &part_record(work.capture, index),
                    &serde_json::to_vec(&record).map_err(unexpected)?,
                )
                .map_err(unexpected)?;
            self.record(&work, purpose, &changed.model, span.end - span.start)?;
            let permille = 200 + (index + 1) * 600 / spans.len();
            checkpoint(
                &mut cx,
                &format!("{} of {} parts", index + 1, spans.len()),
                permille as u16,
            )?;
        }

        // Joined into the narration's audio.
        let mut parts = Vec::with_capacity(spans.len());
        let mut model = String::new();
        for index in 0..spans.len() {
            parts.push(
                self.files
                    .read(project, &part_audio(work.capture, index))
                    .map_err(unexpected)?,
            );
            if index == 0 {
                let record = self
                    .files
                    .read(project, &part_record(work.capture, index))
                    .map_err(unexpected)?;
                model = serde_json::from_slice::<PartRecord>(&record)
                    .map_err(unexpected)?
                    .model;
            }
        }
        let slices: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        let joined = mp3::join(&slices).map_err(|error| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("ElevenLabs: the audio is not usable ({error})"),
            )
        })?;
        let audio_file = crate::narrations::audio_file(work.narration);
        self.files
            .write(project, &audio_file, &joined.bytes)
            .map_err(unexpected)?;

        // Its words timed against the script.
        let aligned_name = aligned_file(work.capture);
        let aligned = if self.files.exists(project, &aligned_name) {
            let record = self
                .files
                .read(project, &aligned_name)
                .map_err(unexpected)?;
            serde_json::from_slice::<AlignedRecord>(&record).map_err(unexpected)?
        } else {
            if stopped(&cx) {
                return Ok(());
            }
            let key = match &key {
                Some(key) => key,
                None => key.insert(self.key()?),
            };
            let aligned = self
                .aligner
                .align(
                    key,
                    &AlignmentRequest {
                        audio: &joined.bytes,
                        file_name: &audio_file,
                        text: work.text.as_str(),
                    },
                )
                .map_err(provider_failure)?;
            let record = AlignedRecord {
                model: aligned.model.clone(),
                chars: aligned
                    .alignment
                    .chars
                    .iter()
                    .map(|c| (c.text.clone(), micros(c.start), micros(c.end)))
                    .collect(),
            };
            self.files
                .write(
                    project,
                    &aligned_name,
                    &serde_json::to_vec(&record).map_err(unexpected)?,
                )
                .map_err(unexpected)?;
            self.record(
                &work,
                CostPurpose::NarrationAlignment,
                &aligned.model,
                joined.duration,
            )?;
            record
        };
        checkpoint(&mut cx, "aligned", 900)?;

        let words = WordTimings::from_alignment(work.text.as_str(), &aligned.alignment());
        let mut takes = Takes::split(&TakeSource {
            text: work.text.as_str(),
            words: words.as_slice(),
            duration: joined.duration,
        });
        if let Some(mode) = payload.mode.as_deref().and_then(TakeMode::from_code) {
            takes.start_in(mode);
        }
        let now = SystemTime::now();
        let capture = Capture {
            id: work.capture,
            project,
            owner: self.owner,
            file_name: payload.file_name.clone(),
            video_file: video.clone(),
            video: payload.video(),
            confirmation: CaptureConfirmation {
                text: payload.confirmation.clone(),
                at: UNIX_EPOCH + Duration::from_millis(payload.confirmed_at),
            },
            voice: work.voice.clone(),
            narration: work.narration,
            coverage,
            takes,
            imported_at: now,
            job: Some(job),
        };
        let narration = Narration {
            id: work.narration,
            project,
            owner: self.owner,
            words,
            text: work.text,
            source: NarrationSource::Captured {
                file_name: payload.file_name,
                voice: work.voice,
                model,
                aligner: PROVIDER,
            },
            audio_file,
            duration: joined.duration,
            generated_at: now,
            job: Some(job),
        };
        // The capture first: it shows only once its narration is the
        // project's, so a failure between the two leaves nothing half done.
        let previous_capture = self.captures.capture(project).map_err(unexpected)?;
        let previous = self.narrations.narration(project).map_err(unexpected)?;
        // Their rows go with the capture they were made from.
        let previous_controls = self.captures.control_videos(project).map_err(unexpected)?;
        let previous_results = self.captures.take_results(project).map_err(unexpected)?;
        self.captures.save_capture(&capture).map_err(unexpected)?;
        self.narrations
            .save_narration(&narration)
            .map_err(unexpected)?;

        self.remove_leftovers(project, work.capture);
        if let Some(previous) = previous.filter(|p| p.audio_file != narration.audio_file) {
            crate::proxies::remove_media(self.files.as_ref(), project, &previous.audio_file);
        }
        if let Some(previous) = previous_capture.filter(|p| p.id != capture.id) {
            if previous.video_file != capture.video_file {
                let _ = self.files.remove(project, &previous.video_file);
            }
            crate::take_analysis::remove_analysis_files(self.files.as_ref(), &previous);
            crate::control_videos::remove_control_files(
                self.files.as_ref(),
                &previous,
                &previous_controls,
            );
            crate::performed_takes::remove_take_files(
                self.files.as_ref(),
                &previous,
                &previous_results,
            );
        }
        // The face mesh runs on every take as soon as it exists.
        crate::take_analysis::queue_face_analyses(self.owner, &capture, self.jobs.as_ref());
        Ok(())
    }
}

impl CaptureHandler {
    /// Removes what the import of `capture` made on the way: its sound, the
    /// transcript, the voiced parts and the alignment. Leftovers only cost
    /// disk space: a file that will not go (open in a player) stays rather
    /// than failing a finished import.
    fn remove_leftovers(&self, project: VideoProjectId, capture: CaptureId) {
        let mut files = vec![
            sound_file(capture),
            heard_file(capture),
            aligned_file(capture),
        ];
        for index in 0..MAX_PARTS {
            files.push(part_audio(capture, index));
            files.push(part_record(capture, index));
        }
        for file in files {
            let _ = self.files.remove(project, &file);
        }
    }
}

/// Removes the project's capture, and its video, unless it is the one of
/// narration `kept`: a narration made another way replaced its speech.
/// Called by the narration jobs once their narration is saved.
pub(crate) fn forget_capture(
    captures: &dyn CaptureRepository,
    files: &dyn ProjectFiles,
    project: VideoProjectId,
    kept: NarrationId,
) -> Result<(), RepositoryError> {
    let Some(capture) = captures.capture(project)? else {
        return Ok(());
    };
    if capture.narration == kept {
        return Ok(());
    }
    let controls = captures.control_videos(project)?;
    let results = captures.take_results(project)?;
    captures.delete_capture(project)?;
    // One that will not go (open in a player) is left behind.
    let _ = files.remove(project, &capture.video_file);
    crate::take_analysis::remove_analysis_files(files, &capture);
    crate::control_videos::remove_control_files(files, &capture, &controls);
    crate::performed_takes::remove_take_files(files, &capture, &results);
    Ok(())
}

/// The narration a capture's takes are cut from.
pub fn take_source(narration: &Narration) -> TakeSource<'_> {
    TakeSource {
        text: narration.text.as_str(),
        words: narration.words.as_slice(),
        duration: narration.duration,
    }
}

impl Bardo {
    /// Reads recordings for `import_capture`, off the UI thread.
    pub fn capture_reader(&self) -> CaptureReader {
        CaptureReader {
            media: Arc::clone(&self.media),
        }
    }

    /// The persona whose voice a capture of the project would take, or why
    /// none can.
    pub fn capture_persona(&self, project: VideoProjectId) -> Result<Persona, CaptureError> {
        let project = self
            .narration_project(project)
            .map_err(|_| CaptureError::ProjectNotFound)?;
        let persona = self.narrator(&project)?.ok_or(CaptureError::NoPersona)?;
        if let Some(flag) = persona.voice_flag {
            return Err(CaptureError::VoiceFlagged(flag));
        }
        Ok(persona)
    }

    /// What importing `chosen` with `voice` would cost.
    pub fn capture_estimate(
        &self,
        chosen: &ChosenCapture,
        voice: CaptureVoiceChoice,
    ) -> Result<SpendEstimate, CaptureError> {
        Ok(self.estimate(&capture_calls(chosen, voice))?)
    }

    /// Starts a job that imports `chosen` as the project's capture: its
    /// speech, in the chosen voice, becomes the narration once it follows
    /// the current script. Needs the user's confirmation that it shows only
    /// them; past ElevenLabs' budget it needs `consent`.
    pub fn import_capture(
        &self,
        project: VideoProjectId,
        chosen: &ChosenCapture,
        voice: CaptureVoiceChoice,
        confirmed: bool,
        consent: BudgetConsent,
    ) -> Result<JobId, CaptureError> {
        let project = self
            .narration_project(project)
            .map_err(|_| CaptureError::ProjectNotFound)?;
        let script = self
            .scripts
            .script(project.id)?
            .ok_or(CaptureError::NoScript)?;
        if !confirmed {
            return Err(CaptureError::NotConfirmed);
        }
        let voice_payload = match voice {
            CaptureVoiceChoice::Persona => {
                let persona = self.capture_persona(project.id)?;
                let (voice, presets) = (persona.details.voice(), persona.details.presets());
                Some(PayloadVoice {
                    provider: voice.provider().code().to_owned(),
                    id: voice.id().to_owned(),
                    name: voice.name().to_owned(),
                    stability: presets.stability,
                    similarity: presets.similarity,
                    style: presets.style,
                    speed: presets.speed,
                })
            }
            CaptureVoiceChoice::Own => None,
        };
        if self.narration_busy(project.id) {
            return Err(CaptureError::Busy);
        }
        if self.takes_busy(project.id) {
            return Err(CaptureError::TakeBusy);
        }
        if self.provider_key(PROVIDER).state == KeyState::NotSet {
            return Err(CaptureError::MissingKey(PROVIDER));
        }
        if let Err(estimate) = self.check_budget(&capture_calls(chosen, voice), consent)? {
            return Err(CaptureError::OverBudget(estimate));
        }
        let language = self
            .channels
            .get(project.channel)?
            .map_or("en", |channel| channel.details.language().code());
        let confirmed_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let payload = CapturePayload {
            project: project.id.to_string(),
            capture: CaptureId::new().to_string(),
            narration: NarrationId::new().to_string(),
            source: chosen.path.clone(),
            file_name: chosen.file_name.clone(),
            size: chosen.size,
            width: chosen.video.width,
            height: chosen.video.height,
            fps_milli: chosen.video.fps_milli,
            duration_ms: chosen.video.duration.as_millis() as u64,
            confirmation: self.text(Text::CaptureConfirm).into_owned(),
            confirmed_at: confirmed_at.as_millis() as u64,
            voice: voice_payload,
            language: language.to_owned(),
            text: script.text().as_str().to_owned(),
            mode: self
                .capture_take_model(project.channel)
                .map(|model| model.default_mode().code().to_owned()),
        };
        // Only now, when the import goes ahead: what an earlier capture
        // job left can no longer be retried once this one is queued.
        self.forget_abandoned_captures(project.id)?;
        let job = Job::new(self.profile.id, JobKind::Capture, payload.to_json());
        Ok(self.jobs.enqueue(job)?)
    }

    /// Removes what earlier capture jobs of the project left in its folder
    /// when they failed or were cancelled; a new capture supersedes them,
    /// and a superseded job is not retried ([`Bardo::superseded_capture`]).
    /// Called while no narration job of the project runs.
    fn forget_abandoned_captures(&self, project: VideoProjectId) -> Result<(), CaptureError> {
        let current = self.captures.capture(project)?.map(|capture| capture.id);
        let id = project.to_string();
        for job in self.jobs() {
            if job.kind() != JobKind::Capture {
                continue;
            }
            let Ok(payload) = CapturePayload::parse(job.payload()) else {
                continue;
            };
            let Ok(capture) = payload.capture() else {
                continue;
            };
            if payload.project != id || current == Some(capture) {
                continue;
            }
            let mut files = vec![
                video_file(capture, &payload.extension()),
                sound_file(capture),
                heard_file(capture),
                aligned_file(capture),
            ];
            for index in 0..MAX_PARTS {
                files.push(part_audio(capture, index));
                files.push(part_record(capture, index));
            }
            if let Ok(narration) = uuid::Uuid::parse_str(&payload.narration) {
                let narration = NarrationId::from(narration);
                let current = self.narrations.narration(project)?.map(|n| n.id);
                if current != Some(narration) {
                    files.push(crate::narrations::audio_file(narration));
                }
            }
            for file in files {
                // One that will not go (open in a player) is left behind.
                let _ = self.files.remove(project, &file);
            }
        }
        Ok(())
    }

    /// Whether `job` is a capture import that a newer narration job of its
    /// project (generated, imported or captured) has replaced: retrying it
    /// would put back an older narration over the newer one, and what it
    /// left in the project folder may be gone.
    pub(crate) fn superseded_capture(&self, job: JobId) -> bool {
        let jobs = self.jobs();
        let Some(payload) = jobs
            .iter()
            .find(|candidate| candidate.id() == job && candidate.kind() == JobKind::Capture)
            .and_then(|job| CapturePayload::parse(job.payload()).ok())
        else {
            return false;
        };
        let Ok(project) = payload.project.parse::<uuid::Uuid>() else {
            return false;
        };
        self.latest_narration_job(VideoProjectId::from(project))
            .is_some_and(|latest| latest.id() != job)
    }

    /// The project's capture with the narration it made, when that is
    /// still the project's narration.
    pub(crate) fn current_capture(
        &self,
        project: VideoProjectId,
    ) -> Result<(Capture, Narration), CaptureError> {
        let project = self
            .narration_project(project)
            .map_err(|_| CaptureError::ProjectNotFound)?;
        let narration = self
            .narrations
            .narration(project.id)?
            .ok_or(CaptureError::NoCapture)?;
        let capture = self
            .captures
            .capture(project.id)?
            .filter(|capture| capture.narration == narration.id)
            .ok_or(CaptureError::NoCapture)?;
        Ok((capture, narration))
    }

    /// Moves the boundary after take `index` to the previous or next cut
    /// point; both takes get a new revision.
    pub fn move_take_boundary(
        &self,
        project: VideoProjectId,
        index: usize,
        to: BoundaryMove,
    ) -> Result<(), CaptureError> {
        let (mut capture, narration) = self.current_capture(project)?;
        if self.performed_busy(&capture, index) || self.performed_busy(&capture, index + 1) {
            return Err(CaptureError::TakeBusy);
        }
        capture
            .takes
            .move_boundary(&take_source(&narration), index, to)?;
        self.captures
            .save_takes(capture.project, capture.id, &capture.takes)?;
        // Their analyses are stale now.
        self.reanalyze_around(&capture, index)
    }

    /// Removes the project's capture and its video; the narration its
    /// speech made stays.
    pub fn remove_capture(&self, project: VideoProjectId) -> Result<(), CaptureError> {
        let (capture, _) = self.current_capture(project)?;
        if self.narration_busy(capture.project) {
            return Err(CaptureError::Busy);
        }
        if self.takes_busy(capture.project) {
            return Err(CaptureError::TakeBusy);
        }
        let controls = self.captures.control_videos(capture.project)?;
        let results = self.captures.take_results(capture.project)?;
        self.captures.delete_capture(capture.project)?;
        let _ = self.files.remove(capture.project, &capture.video_file);
        crate::take_analysis::remove_analysis_files(self.files.as_ref(), &capture);
        crate::control_videos::remove_control_files(self.files.as_ref(), &capture, &controls);
        crate::performed_takes::remove_take_files(self.files.as_ref(), &capture, &results);
        Ok(())
    }

    /// The project's capture, when its narration is the project's.
    pub(crate) fn capture_of(
        &self,
        narration: Option<&Narration>,
        project: VideoProjectId,
    ) -> Result<Option<Capture>, RepositoryError> {
        let Some(narration) = narration else {
            return Ok(None);
        };
        Ok(self
            .captures
            .capture(project)?
            .filter(|capture| capture.narration == narration.id))
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Mutex;
    use std::time::Duration;

    use bardo_domain::{
        ApiKey, ChangedSpeech, GenerationPresets, HeardWord, ProviderFailure, SpeechTranscriber,
        Transcript, TranscriptionRequest, VoiceChangeRequest, VoiceChanger, VoiceIsolator,
        VoiceRef,
    };

    use crate::testing::PART_AUDIO;

    /// Hears `words` in any sound (nothing unless set); `failure` wins when
    /// set. Keeps the requests: the sound's size and the language.
    #[derive(Default)]
    pub(crate) struct FakeTranscriber {
        pub(crate) words: Mutex<Vec<HeardWord>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        pub(crate) requests: Mutex<Vec<(usize, Option<String>)>>,
    }

    impl FakeTranscriber {
        /// Hears `text` one word a `step` after the other.
        pub(crate) fn hears(&self, text: &str, step: Duration) {
            *self.words.lock().unwrap() = text
                .split_whitespace()
                .enumerate()
                .map(|(n, word)| HeardWord {
                    text: word.to_owned(),
                    start: step * n as u32,
                    end: step * n as u32 + step / 2,
                })
                .collect();
        }
    }

    impl SpeechTranscriber for FakeTranscriber {
        fn transcribe(
            &self,
            _key: &ApiKey,
            request: &TranscriptionRequest<'_>,
        ) -> Result<Transcript, ProviderFailure> {
            self.requests
                .lock()
                .unwrap()
                .push((request.audio.len(), request.language.map(str::to_owned)));
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            Ok(Transcript {
                words: self.words.lock().unwrap().clone(),
                model: "scribe_v2".into(),
            })
        }
    }

    /// A voice change as the fake saw it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct SeenChange {
        pub(crate) voice: VoiceRef,
        pub(crate) presets: GenerationPresets,
        pub(crate) bytes: usize,
        pub(crate) file_name: String,
    }

    /// Answers every part with a short MP3; `failure` wins when set. Takes
    /// parts up to `max` (290 s unless set).
    #[derive(Default)]
    pub(crate) struct FakeVoiceChanger {
        pub(crate) max: Mutex<Option<Duration>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        pub(crate) requests: Mutex<Vec<SeenChange>>,
    }

    impl VoiceChanger for FakeVoiceChanger {
        fn max_length(&self) -> Duration {
            self.max.lock().unwrap().unwrap_or(Duration::from_secs(290))
        }

        fn change(
            &self,
            _key: &ApiKey,
            voice: &VoiceRef,
            presets: GenerationPresets,
            request: &VoiceChangeRequest<'_>,
        ) -> Result<ChangedSpeech, ProviderFailure> {
            self.requests.lock().unwrap().push(SeenChange {
                voice: voice.clone(),
                presets,
                bytes: request.audio.len(),
                file_name: request.file_name.to_owned(),
            });
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            Ok(ChangedSpeech {
                audio: PART_AUDIO.to_vec(),
                model: "eleven_multilingual_sts_v2".into(),
            })
        }
    }

    /// Answers with a short MP3; `failure` wins when set. Keeps the sizes
    /// of the sounds it cleaned.
    #[derive(Default)]
    pub(crate) struct FakeVoiceIsolator {
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        pub(crate) requests: Mutex<Vec<usize>>,
    }

    impl VoiceIsolator for FakeVoiceIsolator {
        fn isolate(
            &self,
            _key: &ApiKey,
            request: &VoiceChangeRequest<'_>,
        ) -> Result<ChangedSpeech, ProviderFailure> {
            self.requests.lock().unwrap().push(request.audio.len());
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            Ok(ChangedSpeech {
                audio: PART_AUDIO.to_vec(),
                model: "audio_isolation".into(),
            })
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::Ordering;

    use bardo_domain::{
        CostRepository, JobState, PersonaDetails, PersonaDraft, PersonaRepository, ProviderFailure,
        ProviderFailureKind, TakeFlag,
    };
    use bardo_media::ffmpeg::{AudioStream, MediaInfo, VideoStream};
    use bardo_storage::LocalProjectFiles;

    use super::*;
    use crate::narrations::tests::{
        Harness, SCRIPT, documentary_narrator, project, project_without_persona, wait_done,
    };
    use crate::stages::{Stage, StageNote, StageState, StageStatus, project_stages};
    use crate::testing::PART_AUDIO;

    /// A WAV file of `seconds` of silence: 16-bit PCM, 8 kHz, mono.
    fn silent_wav(seconds: u32) -> Vec<u8> {
        let data = vec![0u8; 16_000 * seconds as usize];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        for field in [16u32.to_le_bytes(), [1, 0, 1, 0], 8_000u32.to_le_bytes()] {
            wav.extend_from_slice(&field);
        }
        wav.extend_from_slice(&16_000u32.to_le_bytes());
        wav.extend_from_slice(&[2, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        wav
    }

    /// A test with project folders on disk (ffmpeg writes the sound there)
    /// and a folder of the user's to record into.
    pub(crate) struct Setup {
        pub(crate) h: Harness,
        pub(crate) app: Bardo,
        user: tempfile::TempDir,
        pub(crate) _folders: tempfile::TempDir,
    }

    /// Records `seconds` of sound reading the script, one word a second.
    pub(crate) fn setup(seconds: u32) -> Setup {
        let folders = tempfile::tempdir().unwrap();
        let h = Harness::with_files(Arc::new(LocalProjectFiles::new(folders.path())));
        *h.media.sound.lock().unwrap() = Some(silent_wav(seconds));
        h.transcriber.hears(SCRIPT, Duration::from_secs(1));
        let app = h.start();
        Setup {
            h,
            app,
            user: tempfile::tempdir().unwrap(),
            _folders: folders,
        }
    }

    fn info(seconds: u64, video: Option<(u32, u32, (u32, u32))>, audio: bool) -> MediaInfo {
        MediaInfo {
            // ffprobe reads lengths finer than a millisecond.
            duration: Duration::from_secs(seconds) + Duration::from_nanos(33_333_333),
            video: video.map(|(width, height, frame_rate)| VideoStream {
                codec: "h264".into(),
                width,
                height,
                frame_rate,
            }),
            audio: audio.then(|| AudioStream {
                codec: "aac".into(),
                sample_rate: 48_000,
                channels: 2,
            }),
        }
    }

    impl Setup {
        /// Closes the app and opens it again over the same data.
        pub(crate) fn restart(self) -> Setup {
            let Setup {
                h,
                app,
                user,
                _folders,
            } = self;
            drop(app);
            let app = h.start();
            Setup {
                h,
                app,
                user,
                _folders,
            }
        }

        /// A recording in the user's folder that ffprobe reads as `info`.
        fn recording(&self, name: &str, info: MediaInfo) -> PathBuf {
            let path = self.user.path().join(name);
            // Probed by its contents, whichever copy is read.
            std::fs::write(&path, name.as_bytes()).unwrap();
            self.h
                .media
                .probes
                .lock()
                .unwrap()
                .insert(name.into(), info);
            path
        }

        /// A phone recording: 1080 × 1920 at 29.97 fps with sound.
        pub(crate) fn chosen(&self, seconds: u64) -> ChosenCapture {
            self.chosen_at(seconds, (1080, 1920))
        }

        /// A recording of `size` at 29.97 fps with sound.
        pub(crate) fn chosen_at(&self, seconds: u64, (width, height): (u32, u32)) -> ChosenCapture {
            let path = self.recording(
                "Take 1.MOV",
                info(seconds, Some((width, height, (30_000, 1001))), true),
            );
            self.app.capture_reader().open(&path).unwrap()
        }

        fn import(
            &self,
            project: VideoProjectId,
            chosen: &ChosenCapture,
            voice: CaptureVoiceChoice,
        ) -> Job {
            let id = self
                .app
                .import_capture(project, chosen, voice, true, BudgetConsent::Ask)
                .unwrap();
            wait_done(&self.app, id)
        }

        pub(crate) fn imported(
            &self,
            project: VideoProjectId,
            voice: CaptureVoiceChoice,
        ) -> (Job, Capture, Narration) {
            self.imported_at(project, voice, (1080, 1920))
        }

        /// `imported`, from a recording of `size`.
        pub(crate) fn imported_at(
            &self,
            project: VideoProjectId,
            voice: CaptureVoiceChoice,
            size: (u32, u32),
        ) -> (Job, Capture, Narration) {
            let job = self.import(project, &self.chosen_at(13, size), voice);
            assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
            let view = self.app.narration(project).unwrap();
            (job, view.capture.unwrap(), view.narration.unwrap())
        }

        fn costs(&self, project: VideoProjectId) -> Vec<(CostPurpose, String, u64)> {
            let mut costs: Vec<_> = self
                .h
                .db
                .project_costs(project)
                .unwrap()
                .into_iter()
                .filter(|record| record.provider == PROVIDER)
                .map(|record| (record.purpose, record.model, record.usage.audio_seconds))
                .collect();
            costs.sort_by_key(|(purpose, ..)| purpose.code());
            costs
        }

        fn stages(&self, project: VideoProjectId) -> Vec<StageStatus> {
            project_stages(
                &self.app.script(project).unwrap(),
                &self.app.narration(project).unwrap(),
                &self.app.scenes(project).unwrap(),
                &self.app.render_summary(project).unwrap(),
                &self.app.export_summary(project).unwrap(),
            )
        }
    }

    #[test]
    fn a_chosen_recording_is_read_and_checked_before_anything_is_paid() {
        let s = setup(13);
        let chosen = s.chosen(13);
        assert_eq!(chosen.file_name, "Take 1.MOV");
        assert_eq!(
            chosen.video,
            CaptureVideo {
                width: 1080,
                height: 1920,
                fps_milli: 29_970,
                // ffprobe's 13.033333 s, in whole milliseconds.
                duration: Duration::from_millis(13_033),
            }
        );
        assert_eq!(chosen.size, "Take 1.MOV".len() as u64);

        let reader = s.app.capture_reader();
        let open = |name: &str, info: MediaInfo| reader.open(&s.recording(name, info));
        let refused = |result: Result<ChosenCapture, CaptureError>| match result {
            Err(CaptureError::Unsupported(problem)) => problem,
            other => panic!("{other:?}"),
        };
        let hd = Some((1280, 720, (25, 1)));
        assert_eq!(
            refused(open("take.mkv", info(13, hd, true))),
            CaptureProblem::NotMp4OrMov
        );
        assert_eq!(
            refused(open("voice.mp4", info(13, None, true))),
            CaptureProblem::NoVideo
        );
        assert_eq!(
            refused(open("silent.mp4", info(13, hd, false))),
            CaptureProblem::NoAudio
        );
        assert_eq!(
            refused(open("small.mp4", info(13, Some((640, 360, (30, 1))), true))),
            CaptureProblem::TooSmall
        );
        assert_eq!(
            refused(open(
                "slow.mp4",
                info(13, Some((1920, 1080, (15, 1))), true)
            )),
            CaptureProblem::FrameRate
        );
        assert_eq!(
            refused(open("long.mp4", info(16 * 60, hd, true))),
            CaptureProblem::TooLong
        );
        // ffprobe cannot read it at all.
        let garbage = s.user.path().join("garbage.mp4");
        std::fs::write(&garbage, b"not a video").unwrap();
        assert_eq!(refused(reader.open(&garbage)), CaptureProblem::NoVideo);
        assert!(matches!(
            reader.open(&s.user.path().join("gone.mp4")),
            Err(CaptureError::Unreadable)
        ));
        s.h.media.not_found.store(true, Ordering::SeqCst);
        assert!(matches!(reader.open(&garbage), Err(CaptureError::NoFfmpeg)));
        for problem in CaptureProblem::ALL {
            assert_eq!(
                CaptureError::Unsupported(problem).message(),
                Text::CaptureProblem(problem)
            );
        }
    }

    #[test]
    fn a_capture_becomes_the_narration_in_the_personas_voice_and_is_split_into_takes() {
        let s = setup(13);
        let project = project(&s.app);
        let persona = documentary_narrator(&s.app);
        let chosen = s.chosen(13);
        let estimate = s
            .app
            .capture_estimate(&chosen, CaptureVoiceChoice::Persona)
            .unwrap();
        // Every call has a rate; the voice change is most of it.
        assert!(!estimate.is_partial(), "{estimate:?}");
        assert!(
            estimate.total() > bardo_domain::Money::from_micros(26_000),
            "{estimate:?}"
        );

        let (job, capture, narration) = s.imported(project.id, CaptureVoiceChoice::Persona);

        assert_eq!(job.kind(), JobKind::Capture);
        assert_eq!(
            narration.source,
            NarrationSource::Captured {
                file_name: "Take 1.MOV".into(),
                voice: CaptureVoice::Persona {
                    voice: persona.details.voice().clone(),
                    presets: persona.details.presets(),
                },
                model: "eleven_multilingual_sts_v2".into(),
                aligner: Provider::ElevenLabs,
            }
        );
        assert_eq!(narration.text.as_str(), SCRIPT);
        assert_eq!(narration.job, Some(job.id()));
        assert_eq!(capture.narration, narration.id);
        assert_eq!(capture.file_name, "Take 1.MOV");
        assert_eq!(capture.video, chosen.video);
        assert_eq!(capture.confirmation.text, "This recording shows only me.");
        assert_eq!(capture.coverage, 1000);
        assert_eq!(capture.job, Some(job.id()));
        // Every word is in a take, in order.
        let takes = capture.takes.as_slice();
        assert_eq!(takes.first().unwrap().words.start, 0);
        assert_eq!(takes.last().unwrap().words.end, narration.words.len());

        // The video stays in the project folder; only its sound was sent.
        let files = &s.h.files;
        assert_eq!(capture.video_file, format!("capture-{}.mov", capture.id));
        assert!(files.exists(project.id, &capture.video_file));
        assert!(files.exists(project.id, &narration.audio_file));
        let sound = silent_wav(13);
        assert_eq!(
            *s.h.transcriber.requests.lock().unwrap(),
            [(sound.len(), Some("pt".to_owned()))],
            "the channel's language"
        );
        let changes = s.h.voice_changer.requests.lock().unwrap().clone();
        assert_eq!(changes.len(), 1, "13 seconds go in one part");
        assert_eq!(changes[0].voice, *persona.details.voice());
        assert_eq!(changes[0].presets, persona.details.presets());
        assert_eq!(changes[0].bytes, sound.len());
        assert_eq!(s.h.aligner.requests()[0].audio, PART_AUDIO);
        assert_eq!(s.h.aligner.requests()[0].text, SCRIPT);
        // What was kept between attempts is gone.
        for file in [
            sound_file(capture.id),
            heard_file(capture.id),
            aligned_file(capture.id),
            part_audio(capture.id, 0),
            part_record(capture.id, 0),
        ] {
            assert!(!files.exists(project.id, &file), "{file}");
        }

        // The check and the voice billed the sound's 13 seconds, the
        // timing the voiced narration's.
        assert_eq!(narration.duration.as_millis().div_ceil(1000), 2);
        assert_eq!(
            s.costs(project.id),
            [
                (
                    CostPurpose::NarrationAlignment,
                    "forced_alignment".into(),
                    2
                ),
                (CostPurpose::ScriptCheck, "scribe_v2".into(), 13),
                (
                    CostPurpose::VoiceChange,
                    "eleven_multilingual_sts_v2".into(),
                    13
                ),
            ]
            .map(|(purpose, model, seconds): (_, String, _)| (purpose, model, seconds))
        );

        // The Capture stage shows its takes after the narration's.
        let stages = s.stages(project.id);
        let order: Vec<Stage> = stages.iter().map(|status| status.stage).collect();
        assert_eq!(
            order[..3],
            [Stage::Script, Stage::Narration, Stage::Capture]
        );
        let status = stages[2];
        // One second of narration is one take, too short to perform.
        assert_eq!(capture.takes.as_slice()[0].flag(), Some(TakeFlag::TooShort));
        assert_eq!(
            (status.state, status.note),
            (StageState::Attention, StageNote::TakesFlagged(1))
        );
        assert_eq!(s.app.stage_note(status.note), "1 to fix");
    }

    #[test]
    fn the_users_own_voice_is_cleaned_whole_and_needs_no_disclosure() {
        let s = setup(13);
        let project = project(&s.app);
        // The narrator's voice is realistic: its narration is disclosed.
        let narrator = documentary_narrator(&s.app);
        let mut realistic = narrator.clone();
        realistic.details = PersonaDetails::validate(PersonaDraft {
            realistic_voice: true,
            ..PersonaDraft::from(&narrator.details)
        })
        .unwrap();
        PersonaRepository::save(&*s.h.db, &realistic).unwrap();

        let (_, capture, narration) = s.imported(project.id, CaptureVoiceChoice::Own);
        assert_eq!(capture.voice, CaptureVoice::Own);
        assert!(matches!(
            narration.source,
            NarrationSource::Captured {
                voice: CaptureVoice::Own,
                ref model,
                ..
            } if model == "audio_isolation"
        ));
        assert_eq!(
            *s.h.voice_isolator.requests.lock().unwrap(),
            [silent_wav(13).len()]
        );
        assert!(s.h.voice_changer.requests.lock().unwrap().is_empty());
        assert!(
            s.costs(project.id)
                .iter()
                .any(|(purpose, ..)| *purpose == CostPurpose::VoiceIsolation)
        );
        assert!(!s.app.needs_disclosure(project.id).unwrap());

        // In the persona's voice it is disclosed.
        s.imported(project.id, CaptureVoiceChoice::Persona);
        assert!(s.app.needs_disclosure(project.id).unwrap());
    }

    #[test]
    fn a_recording_off_the_script_is_refused_before_its_voice_is_paid_for() {
        let s = setup(13);
        let project = project(&s.app);
        s.h.transcriber.hears(
            "Hoje vamos falar de outra coisa completamente diferente",
            Duration::from_secs(1),
        );

        let job = s.import(project.id, &s.chosen(13), CaptureVoiceChoice::Persona);

        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::OffScript);
        assert_eq!(job.attempts(), 1, "not retried");
        assert!(s.h.voice_changer.requests.lock().unwrap().is_empty());
        assert_eq!(
            s.costs(project.id),
            [(CostPurpose::ScriptCheck, "scribe_v2".to_owned(), 13)]
        );
        let view = s.app.narration(project.id).unwrap();
        assert_eq!(view.narration, None);
        assert_eq!(view.capture, None);
        assert_eq!(view.job.map(|job| job.id()), Some(job.id()));
    }

    #[test]
    fn a_long_capture_is_voiced_in_parts_and_a_retry_pays_for_nothing_twice() {
        let s = setup(13);
        let project = project(&s.app);
        // Parts of five seconds at most: the 13 seconds go in three.
        *s.h.voice_changer.max.lock().unwrap() = Some(Duration::from_secs(5));
        *s.h.aligner.failure.lock().unwrap() = Some(ProviderFailure {
            kind: ProviderFailureKind::Rejected,
            detail: "invalid key".into(),
        });

        let job = s.import(project.id, &s.chosen(13), CaptureVoiceChoice::Persona);
        assert_eq!(job.state(), JobState::Failed);
        let parts = s.h.voice_changer.requests.lock().unwrap().clone();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts
                .iter()
                .map(|part| part.file_name.clone())
                .collect::<Vec<_>>(),
            ["001", "002", "003"].map(|n| format!(
                "capture-{}.part{n}.wav",
                CapturePayload::parse(job.payload()).unwrap().capture
            ))
        );
        assert!(parts.iter().all(|part| part.bytes < silent_wav(13).len()));

        *s.h.aligner.failure.lock().unwrap() = None;
        s.app.retry_job(job.id()).unwrap();
        let job = wait_done(&s.app, job.id());
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());

        assert_eq!(s.h.transcriber.requests.lock().unwrap().len(), 1);
        assert_eq!(s.h.voice_changer.requests.lock().unwrap().len(), 3);
        let costs = s.costs(project.id);
        let count = |purpose| costs.iter().filter(|(p, ..)| *p == purpose).count();
        assert_eq!(count(CostPurpose::ScriptCheck), 1);
        assert_eq!(count(CostPurpose::VoiceChange), 3);
        assert_eq!(count(CostPurpose::NarrationAlignment), 1);
        let narration = s.app.narration(project.id).unwrap().narration.unwrap();
        let part = mp3::frames(PART_AUDIO).unwrap().duration();
        assert_eq!(
            narration.duration.as_millis(),
            (part * 3).as_millis(),
            "the parts joined"
        );
    }

    #[test]
    fn a_failed_import_stays_retryable_until_a_newer_one_replaces_it() {
        let mut s = setup(13);
        let project = project(&s.app);
        *s.h.aligner.failure.lock().unwrap() = Some(ProviderFailure {
            kind: ProviderFailureKind::Rejected,
            detail: "invalid key".into(),
        });
        let failed = s.import(project.id, &s.chosen(13), CaptureVoiceChoice::Persona);
        assert_eq!(failed.state(), JobState::Failed);
        let left = CapturePayload::parse(failed.payload())
            .unwrap()
            .capture()
            .unwrap();
        let folder = s._folders.path().join(project.id.to_string());
        let paid = folder.join(heard_file(left));
        assert!(paid.exists(), "the paid transcript is kept for a retry");

        // An import refused before it is queued leaves it as it was.
        let key =
            s.h.secrets
                .get(s.app.profile.id, PROVIDER)
                .unwrap()
                .unwrap();
        s.app.remove_provider_key(PROVIDER).unwrap();
        assert!(matches!(
            s.app.import_capture(
                project.id,
                &s.chosen(13),
                CaptureVoiceChoice::Persona,
                true,
                BudgetConsent::Ask
            ),
            Err(CaptureError::MissingKey(_))
        ));
        assert!(paid.exists());
        assert!(!s.app.superseded_capture(failed.id()));

        // A newer import replaces it: its files go and it does not run again.
        s.app.save_provider_key(PROVIDER, key.expose()).unwrap();
        *s.h.aligner.failure.lock().unwrap() = None;
        s.imported(project.id, CaptureVoiceChoice::Persona);
        assert!(!paid.exists());
        assert!(matches!(
            s.app.retry_job(failed.id()),
            Err(crate::jobs::JobActionError::Superseded)
        ));
    }

    #[test]
    fn a_capture_needs_the_users_word_a_script_and_a_voice() {
        let s = setup(13);
        let chosen = s.chosen(13);
        let project = project(&s.app);
        assert!(matches!(
            s.app.import_capture(
                project.id,
                &chosen,
                CaptureVoiceChoice::Persona,
                false,
                BudgetConsent::Ask
            ),
            Err(CaptureError::NotConfirmed)
        ));
        s.h.text.answers.lock().unwrap().push(SCRIPT.to_owned());
        let bare = project_without_persona(&s.app, "My Own Channel");
        assert!(matches!(
            s.app.capture_persona(bare.id),
            Err(CaptureError::NoPersona)
        ));
        assert!(matches!(
            s.app.import_capture(
                bare.id,
                &chosen,
                CaptureVoiceChoice::Persona,
                true,
                BudgetConsent::Ask
            ),
            Err(CaptureError::NoPersona)
        ));
        // The user's own voice needs no persona.
        let job = s.import(bare.id, &chosen, CaptureVoiceChoice::Own);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert!(s.app.narration(bare.id).unwrap().capture.is_some());
    }

    #[test]
    fn a_recording_replaced_after_it_was_chosen_is_not_imported() {
        let s = setup(13);
        let project = project(&s.app);
        let chosen = s.chosen(13);
        // The user records over it, longer, before the job copies it.
        std::fs::write(&chosen.path, b"a longer take, recorded again").unwrap();

        let job = s.import(project.id, &chosen, CaptureVoiceChoice::Persona);

        assert_eq!(job.state(), JobState::Failed);
        assert!(
            job.failure()
                .unwrap()
                .detail
                .contains("no longer the recording"),
            "{:?}",
            job.failure()
        );
        assert!(s.h.transcriber.requests.lock().unwrap().is_empty());
        assert!(s.costs(project.id).is_empty());
    }

    #[test]
    fn takes_move_their_cuts_and_go_with_their_narration() {
        let s = setup(13);
        // Long enough for two takes: 600 ms a character, so each space is
        // a pause to cut at.
        *s.h.aligner.step.lock().unwrap() = Some(Duration::from_millis(600));
        let project = project(&s.app);
        let (_, capture, _) = s.imported(project.id, CaptureVoiceChoice::Persona);
        let before = capture.takes.as_slice().to_vec();
        assert_eq!(before.len(), 2, "{before:?}");

        s.app
            .move_take_boundary(project.id, 0, BoundaryMove::Earlier)
            .unwrap();
        let moved = s.app.narration(project.id).unwrap().capture.unwrap();
        let after = moved.takes.as_slice();
        assert_eq!(after[0].words.end, before[0].words.end - 1);
        assert_eq!(after[1].words.start, before[1].words.start - 1);
        assert_eq!((after[0].revision, after[1].revision), (2, 2));
        assert!(matches!(
            s.app.move_take_boundary(project.id, 1, BoundaryMove::Later),
            Err(CaptureError::Boundary(BoundaryError::NoSuchBoundary))
        ));

        // Removing the capture keeps its narration.
        s.app.remove_capture(project.id).unwrap();
        let view = s.app.narration(project.id).unwrap();
        assert_eq!(view.capture, None);
        assert!(view.narration.is_some());
        assert!(!s.h.files.exists(project.id, &capture.video_file));
        assert!(
            !s.stages(project.id)
                .iter()
                .any(|status| status.stage == Stage::Capture)
        );

        // Another narration takes the capture with it.
        let (_, capture, _) = s.imported(project.id, CaptureVoiceChoice::Persona);
        let job = s
            .app
            .generate_narration(project.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(wait_done(&s.app, job).state(), JobState::Done);
        assert_eq!(s.app.narration(project.id).unwrap().capture, None);
        assert_eq!(s.h.db.capture(project.id).unwrap(), None);
        assert!(!s.h.files.exists(project.id, &capture.video_file));
        assert!(matches!(
            s.app.move_take_boundary(project.id, 0, BoundaryMove::Later),
            Err(CaptureError::NoCapture)
        ));
    }
}
