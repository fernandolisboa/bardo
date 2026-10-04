//! Imported narration (PRD story 33): the user records the script and
//! imports the file instead of generating the narration. The recording is
//! copied into the project folder and ElevenLabs times its words against
//! the script (forced alignment); from then on it is the project's
//! narration like a generated one: played with the spoken word
//! highlighted, stale once the script changes, and what scenes, captions
//! and cut snapping are timed on.
//!
//! Choosing a file reads it first (`Recording::open`, off the UI thread),
//! so the panel can show its length and what aligning it would cost before
//! anything is paid. Copying and aligning run as a job; the copy is kept
//! in the project folder, so a retried job does not copy again.
//!
//! MP3 and uncompressed WAV are imported as they are; other formats wait
//! for ffmpeg (issue #18).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    AlignmentRequest, ApiKey, CostPurpose, Job, JobFailure, JobFailureKind, JobId, JobKind,
    Metered, Narration, NarrationId, NarrationRepository, NarrationSource, ProfileId, Progress,
    ProjectFiles, Provider, ScriptText, SecretStore, SpeechAligner, VideoProjectId, WordTimings,
};
use bardo_media::{AudioFormat, ProbeError, audio};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::narrations::NarrationError;
use crate::{Bardo, KeyState};

/// The largest recording Bardo imports: about 45 minutes of CD-quality
/// WAV, far more as MP3. The whole file is read into memory to copy and
/// send it.
pub const MAX_RECORDING_BYTES: u64 = 500 * 1024 * 1024;

/// The provider that times imported recordings.
const ALIGNER: Provider = Provider::ElevenLabs;

/// A recording the user chose, read and checked, ready to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub path: PathBuf,
    /// The file's name as the user has it.
    pub file_name: String,
    pub format: AudioFormat,
    pub duration: Duration,
}

impl Recording {
    /// Reads the file at `path` and checks it is audio Bardo plays. Reads
    /// the whole file: call it off the UI thread.
    pub fn open(path: &Path) -> Result<Recording, NarrationError> {
        let bytes = read_recording(path)?;
        let info = audio::probe(&bytes).map_err(NarrationError::from)?;
        Ok(Recording {
            path: path.to_owned(),
            file_name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            format: info.format,
            duration: info.duration,
        })
    }

    /// Seconds of audio the aligner bills, rounded up.
    fn billed_seconds(&self) -> u64 {
        billed_seconds(self.duration)
    }
}

fn billed_seconds(duration: Duration) -> u64 {
    duration.as_millis().div_ceil(1_000) as u64
}

fn read_recording(path: &Path) -> Result<Vec<u8>, NarrationError> {
    let size = std::fs::metadata(path)
        .map_err(|_| NarrationError::RecordingUnreadable)?
        .len();
    if size > MAX_RECORDING_BYTES {
        return Err(NarrationError::RecordingTooLarge);
    }
    std::fs::read(path).map_err(|_| NarrationError::RecordingUnreadable)
}

impl From<ProbeError> for NarrationError {
    fn from(error: ProbeError) -> Self {
        match error {
            ProbeError::UnsupportedFormat => NarrationError::RecordingUnsupported,
            ProbeError::NoAudio => NarrationError::RecordingEmpty,
        }
    }
}

/// The aligner's calls for a recording: billed per second of audio.
fn alignment_call(seconds: u64) -> PlannedCall {
    PlannedCall::new(ALIGNER, CostPurpose::NarrationAlignment, 1)
        .with_model(bardo_ai::elevenlabs::ALIGNMENT_MODEL)
        .with_audio_seconds(seconds)
}

/// The import job's payload: where the recording is, what it reads, and
/// the narration's id (its file is named after it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ImportPayload {
    pub(crate) project: String,
    narration: String,
    source: PathBuf,
    file_name: String,
    /// The extension of the format the file was read as.
    format: String,
    /// Its length when chosen, what the user agreed to pay for.
    duration_ms: u64,
    text: String,
}

impl ImportPayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("an import payload serializes")
    }

    pub(crate) fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid import payload: {e}")))
    }
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// The imported recording's file in the project folder.
fn audio_file(narration: NarrationId, format: AudioFormat) -> String {
    format!("narration-{narration}.{}", format.extension())
}

/// Runs import jobs.
pub(crate) struct NarrationImportHandler {
    pub(crate) owner: ProfileId,
    pub(crate) narrations: Arc<dyn NarrationRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) aligner: Arc<dyn SpeechAligner>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for NarrationImportHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.import(payload, cx.id(), Some(cx))
    }
}

impl NarrationImportHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, ALIGNER)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(JobFailureKind::MissingKey, "no ElevenLabs key is saved")
            })
    }

    /// Copies the recording into the project folder, times its words and
    /// saves it as the project's narration. Without a context (tests
    /// replaying a job) nothing reports progress or stops it.
    fn import(
        &self,
        payload: &str,
        job: JobId,
        mut cx: Option<&mut JobContext>,
    ) -> Result<(), JobFailure> {
        let payload = ImportPayload::parse(payload)?;
        let project = uuid::Uuid::parse_str(&payload.project)
            .map(VideoProjectId::from)
            .map_err(unexpected)?;
        let id = uuid::Uuid::parse_str(&payload.narration)
            .map(NarrationId::from)
            .map_err(unexpected)?;
        let text = ScriptText::new(&payload.text).map_err(|e| unexpected(format!("{e:?}")))?;
        let format = AudioFormat::from_extension(&payload.format)
            .ok_or_else(|| unexpected(format!("unknown format {}", payload.format)))?;
        // An earlier attempt may have saved the narration and stopped
        // before the queue recorded it as done.
        if self
            .narrations
            .narration(project)
            .map_err(unexpected)?
            .is_some_and(|saved| saved.job == Some(job))
        {
            return Ok(());
        }

        let file = audio_file(id, format);
        let audio = if self.files.exists(project, &file) {
            self.files.read(project, &file).map_err(unexpected)?
        } else {
            let audio = read_recording(&payload.source).map_err(|error| {
                JobFailure::unexpected(format!(
                    "could not read the recording {}: {}",
                    payload.file_name,
                    match error {
                        NarrationError::RecordingTooLarge => "it is over the size limit",
                        _ => "it was moved or cannot be opened",
                    }
                ))
            })?;
            self.files
                .write(project, &file, &audio)
                .map_err(unexpected)?;
            audio
        };
        // The file may have been replaced since it was chosen; a longer one
        // would cost more than the user agreed to.
        let info = audio::probe(&audio)
            .ok()
            .filter(|info| {
                info.format == format
                    && info.duration.as_millis() == u128::from(payload.duration_ms)
            })
            .ok_or_else(|| {
                JobFailure::unexpected(format!(
                    "{} is no longer the audio that was chosen",
                    payload.file_name
                ))
            })?;
        if let Some(cx) = cx.as_mut() {
            if cx.should_stop() {
                return Ok(());
            }
            cx.save_checkpoint("copied", Progress::from_permille(100))
                .map_err(unexpected)?;
        }

        let key = self.key()?;
        let aligned = self
            .aligner
            .align(
                &key,
                &AlignmentRequest {
                    audio: &audio,
                    file_name: &file,
                    text: text.as_str(),
                },
            )
            .map_err(|failure| {
                JobFailure::new(
                    failure.kind.into(),
                    format!("ElevenLabs: {}", failure.detail),
                )
            })?;
        self.costs.record_for_project(
            PaidCall {
                provider: ALIGNER,
                model: &aligned.model,
                purpose: CostPurpose::NarrationAlignment,
                usage: Metered::audio_seconds(billed_seconds(info.duration)),
                job: Some(job),
                reported: None,
            },
            project,
        );

        let narration = Narration {
            id,
            project,
            owner: self.owner,
            words: WordTimings::from_alignment(text.as_str(), &aligned.alignment),
            text,
            source: NarrationSource::Imported {
                file_name: payload.file_name,
                aligner: ALIGNER,
                model: aligned.model,
            },
            audio_file: file,
            duration: info.duration,
            generated_at: SystemTime::now(),
            job: Some(job),
        };
        let previous = self.narrations.narration(project).map_err(unexpected)?;
        self.narrations
            .save_narration(&narration)
            .map_err(unexpected)?;
        // A file that will not go (open in a player) stays rather than
        // failing a finished import.
        if let Some(previous) = previous.filter(|p| p.audio_file != narration.audio_file) {
            crate::proxies::remove_media(self.files.as_ref(), project, &previous.audio_file);
        }
        Ok(())
    }
}

impl Bardo {
    /// What aligning `recording` would cost.
    pub fn import_estimate(&self, recording: &Recording) -> Result<SpendEstimate, NarrationError> {
        Ok(self.estimate(&[alignment_call(recording.billed_seconds())])?)
    }

    /// Starts a job that copies `recording` into the project folder and
    /// times its words against the current script; it then replaces the
    /// project's narration. Past ElevenLabs' budget it needs `consent`.
    pub fn import_narration(
        &self,
        project: VideoProjectId,
        recording: &Recording,
        consent: BudgetConsent,
    ) -> Result<JobId, NarrationError> {
        let project = self.narration_project(project)?;
        let script = self
            .scripts
            .script(project.id)?
            .ok_or(NarrationError::NoScript)?;
        if self.narration_busy(project.id) {
            return Err(NarrationError::Busy);
        }
        self.forget_abandoned_imports(project.id)?;
        if self.provider_key(ALIGNER).state == KeyState::NotSet {
            return Err(NarrationError::MissingKey(ALIGNER));
        }
        if let Err(estimate) =
            self.check_budget(&[alignment_call(recording.billed_seconds())], consent)?
        {
            return Err(NarrationError::OverBudget(estimate));
        }
        let payload = ImportPayload {
            project: project.id.to_string(),
            narration: NarrationId::new().to_string(),
            source: recording.path.clone(),
            file_name: recording.file_name.clone(),
            format: recording.format.extension().to_owned(),
            duration_ms: recording.duration.as_millis() as u64,
            text: script.text().as_str().to_owned(),
        };
        let job = Job::new(self.profile.id, JobKind::NarrationImport, payload.to_json());
        Ok(self.jobs.enqueue(job)?)
    }

    /// Removes the copies earlier imports of the project left in its
    /// folder when they failed or were cancelled; a new import supersedes
    /// them. Called while no narration job of the project runs.
    fn forget_abandoned_imports(&self, project: VideoProjectId) -> Result<(), NarrationError> {
        let current = self
            .narrations
            .narration(project)?
            .map(|narration| narration.audio_file);
        let id = project.to_string();
        for job in self.jobs() {
            if job.kind() != JobKind::NarrationImport {
                continue;
            }
            let Ok(payload) = ImportPayload::parse(job.payload()) else {
                continue;
            };
            let (Ok(narration), Some(format)) = (
                uuid::Uuid::parse_str(&payload.narration),
                AudioFormat::from_extension(&payload.format),
            ) else {
                continue;
            };
            let file = audio_file(NarrationId::from(narration), format);
            if payload.project == id && current.as_ref() != Some(&file) {
                // One that will not go (open in a player) is left behind.
                let _ = self.files.remove(project, &file);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Mutex;
    use std::time::Duration;

    use bardo_domain::{
        AlignedSpeech, Alignment, AlignmentRequest, ApiKey, CharTiming, ProviderFailure,
        SpeechAligner,
    };

    /// A request as the fake aligner saw it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct SeenAlignment {
        pub(crate) audio: Vec<u8>,
        pub(crate) file_name: String,
        pub(crate) text: String,
    }

    /// Times each character of the text 50 ms after the one before;
    /// `failure` wins when set. Keeps the requests.
    #[derive(Default)]
    pub(crate) struct FakeAligner {
        pub(crate) requests: Mutex<Vec<SeenAlignment>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
    }

    impl FakeAligner {
        pub(crate) fn requests(&self) -> Vec<SeenAlignment> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl SpeechAligner for FakeAligner {
        fn align(
            &self,
            _key: &ApiKey,
            request: &AlignmentRequest<'_>,
        ) -> Result<AlignedSpeech, ProviderFailure> {
            self.requests.lock().unwrap().push(SeenAlignment {
                audio: request.audio.to_vec(),
                file_name: request.file_name.to_owned(),
                text: request.text.to_owned(),
            });
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            let step = Duration::from_millis(50);
            Ok(AlignedSpeech {
                alignment: Alignment {
                    chars: request
                        .text
                        .chars()
                        .enumerate()
                        .map(|(n, c)| CharTiming {
                            text: c.to_string(),
                            start: step * n as u32,
                            end: step * (n as u32 + 1),
                        })
                        .collect(),
                },
                model: "forced_alignment".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        CostRepository, JobState, Money, ProviderFailure, ProviderFailureKind, spoken_words,
    };
    use bardo_storage::LocalProjectFiles;

    use super::*;
    use crate::Text;
    use crate::narrations::tests::{
        ELEVENLABS_KEY, Harness, SCRIPT, narrate, project, project_without_persona, wait_done,
    };
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

    /// Writes `bytes` as `name` in a folder of the user's, outside Bardo.
    fn user_file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn import(app: &Bardo, project: VideoProjectId, recording: &Recording) -> Job {
        let id = app
            .import_narration(project, recording, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(app, id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        job
    }

    #[test]
    fn a_chosen_file_is_read_for_its_format_and_length() {
        let dir = tempfile::tempdir().unwrap();
        let wav = Recording::open(&user_file(&dir, "Take 3.WAV", &silent_wav(3))).unwrap();
        assert_eq!(wav.file_name, "Take 3.WAV");
        assert_eq!(wav.format, AudioFormat::Wav);
        assert_eq!(wav.duration, Duration::from_secs(3));

        // The format comes from the bytes, not the name.
        let mp3 = Recording::open(&user_file(&dir, "voice.wav", PART_AUDIO)).unwrap();
        assert_eq!(mp3.format, AudioFormat::Mp3);

        let not_audio = user_file(&dir, "notes.m4a", b"ftypM4A not really");
        assert!(matches!(
            Recording::open(&not_audio),
            Err(NarrationError::RecordingUnsupported)
        ));
        let mut empty = silent_wav(1);
        empty.truncate(empty.len() - 16_000);
        let at = empty.len() - 4;
        empty[at..].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            Recording::open(&user_file(&dir, "empty.wav", &empty)),
            Err(NarrationError::RecordingEmpty)
        ));
        assert!(matches!(
            Recording::open(&dir.path().join("gone.wav")),
            Err(NarrationError::RecordingUnreadable)
        ));
        let huge = std::fs::File::create(dir.path().join("huge.wav")).unwrap();
        huge.set_len(MAX_RECORDING_BYTES + 1).unwrap();
        assert!(matches!(
            Recording::open(&dir.path().join("huge.wav")),
            Err(NarrationError::RecordingTooLarge)
        ));
        for (error, text) in [
            (
                NarrationError::RecordingUnreadable,
                Text::RecordingUnreadable,
            ),
            (
                NarrationError::RecordingUnsupported,
                Text::RecordingUnsupported,
            ),
            (NarrationError::RecordingEmpty, Text::RecordingEmpty),
            (NarrationError::RecordingTooLarge, Text::RecordingTooLarge),
        ] {
            assert_eq!(error.message(), text);
        }
    }

    #[test]
    fn importing_copies_the_recording_and_times_its_words_against_the_script() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = h.start();
        // No persona needed: the user is the narrator.
        let project = project_without_persona(&app, "Home Studio");
        let bytes = silent_wav(4);
        let recording = Recording::open(&user_file(&dir, "take 3.wav", &bytes)).unwrap();

        let job = import(&app, project.id, &recording);

        let view = app.narration(project.id).unwrap();
        let narration = view.narration.unwrap();
        assert_eq!(view.job.map(|job| job.id()), Some(job.id()));
        assert_eq!(job.kind(), JobKind::NarrationImport);
        assert!(!view.stale);
        assert_eq!(narration.text.as_str(), SCRIPT);
        assert_eq!(
            narration.source,
            NarrationSource::Imported {
                file_name: "take 3.wav".into(),
                aligner: Provider::ElevenLabs,
                model: "forced_alignment".into(),
            }
        );
        assert_eq!(
            narration.audio_file,
            format!("narration-{}.wav", narration.id)
        );
        assert_eq!(narration.duration, Duration::from_secs(4));
        assert_eq!(narration.job, Some(job.id()));
        assert_eq!(
            h.files.read(project.id, &narration.audio_file).unwrap(),
            bytes
        );

        let sent = h.aligner.requests();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, SCRIPT);
        assert_eq!(sent[0].file_name, narration.audio_file);
        assert_eq!(sent[0].audio, bytes);

        assert_eq!(narration.words.len(), spoken_words(SCRIPT).len());
        let (word, timing) = narration.words().nth(4).unwrap();
        assert_eq!(word, "1969,");
        // The fake times each character 50 ms after the one before.
        let at = SCRIPT.find("1969").unwrap() as u32;
        assert_eq!(timing.start, Duration::from_millis(50) * at);
    }

    #[test]
    fn alignment_is_estimated_and_recorded_per_second_of_audio() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = h.start();
        let project = project_without_persona(&app, "Home Studio");
        let recording = Recording::open(&user_file(&dir, "take.wav", &silent_wav(90))).unwrap();

        let estimate = app.import_estimate(&recording).unwrap();
        assert_eq!(estimate.providers.len(), 1);
        assert_eq!(estimate.providers[0].provider, Provider::ElevenLabs);
        // $0.22 per hour: 90 seconds is $0.0055.
        assert_eq!(estimate.total(), Money::from_micros(5_500));

        let job = import(&app, project.id, &recording);

        let records: Vec<_> =
            h.db.project_costs(project.id)
                .unwrap()
                .into_iter()
                .filter(|record| record.purpose == CostPurpose::NarrationAlignment)
                .collect();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.purpose, CostPurpose::NarrationAlignment);
        assert_eq!(record.provider, Provider::ElevenLabs);
        assert_eq!(record.model, "forced_alignment");
        assert_eq!(record.usage, Metered::audio_seconds(90));
        assert_eq!(record.cost.amount(), Money::from_micros(5_500));
        assert_eq!(record.job, Some(job.id()));

        app.set_budget(Provider::ElevenLabs, "0.001").unwrap();
        assert!(matches!(
            app.import_narration(project.id, &recording, BudgetConsent::Ask),
            Err(NarrationError::OverBudget(_))
        ));
        assert!(
            app.import_narration(project.id, &recording, BudgetConsent::Confirmed)
                .is_ok()
        );
    }

    #[test]
    fn an_imported_narration_plays_goes_stale_and_times_scenes_like_a_generated_one() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        // Long enough for every word the fake aligner times.
        let recording = Recording::open(&user_file(&dir, "take.wav", &silent_wav(5))).unwrap();
        import(&app, project.id, &recording);
        let narration = app.narration(project.id).unwrap().narration.unwrap();

        let mut player = app.play_narration(project.id).unwrap();
        assert_eq!(
            h.audio.opened.lock().unwrap().as_slice(),
            [h.files.path(project.id, &narration.audio_file)]
        );
        for (index, (_, timing)) in narration.words().enumerate() {
            h.audio.set_position(timing.start);
            assert_eq!(player.current_word(), Some(index));
        }
        player.seek_to_word(2).unwrap();
        assert_eq!(player.current_word(), Some(2));

        let scenes = app.scenes(project.id).unwrap();
        assert_eq!(scenes.narration, Some(narration.id));
        assert!(
            scenes.plan_estimate.is_some(),
            "scenes can be planned on it"
        );

        app.edit_script(project.id, "Era uma vez uma sonda perdida.")
            .unwrap();
        let view = app.narration(project.id).unwrap();
        assert!(view.stale);
        assert_eq!(view.narration, Some(narration));
    }

    #[test]
    fn mp3_recordings_keep_their_format() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let recording = Recording::open(&user_file(&dir, "take.mp3", PART_AUDIO)).unwrap();

        import(&app, project.id, &recording);

        let narration = app.narration(project.id).unwrap().narration.unwrap();
        assert_eq!(
            narration.audio_file,
            format!("narration-{}.mp3", narration.id)
        );
        // Stored to the millisecond.
        assert_eq!(
            narration.duration.as_millis(),
            recording.duration.as_millis()
        );
        assert_eq!(
            h.files.read(project.id, &narration.audio_file).unwrap(),
            PART_AUDIO
        );
    }

    #[test]
    fn an_import_replaces_a_generated_narration_and_its_audio() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let generated = narrate(&app, &project);
        let recording = Recording::open(&user_file(&dir, "take.wav", &silent_wav(2))).unwrap();

        import(&app, project.id, &recording);

        let imported = app.narration(project.id).unwrap().narration.unwrap();
        assert_ne!(imported.id, generated.id);
        assert!(h.files.exists(project.id, &imported.audio_file));
        assert!(!h.files.exists(project.id, &generated.audio_file));

        // And generating again replaces the recording.
        let again = narrate(&app, &project);
        assert!(matches!(again.source, NarrationSource::Generated { .. }));
        assert!(!h.files.exists(project.id, &imported.audio_file));
    }

    #[test]
    fn importing_needs_a_script_a_key_and_no_other_narration_running() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let mut app = h.start();
        let recording = Recording::open(&user_file(&dir, "take.wav", &silent_wav(1))).unwrap();
        assert!(matches!(
            app.import_narration(VideoProjectId::new(), &recording, BudgetConsent::Ask),
            Err(NarrationError::ProjectNotFound)
        ));

        let project = project(&app);
        app.remove_provider_key(Provider::ElevenLabs).unwrap();
        assert!(matches!(
            app.import_narration(project.id, &recording, BudgetConsent::Ask),
            Err(NarrationError::MissingKey(Provider::ElevenLabs))
        ));
        app.save_provider_key(Provider::ElevenLabs, ELEVENLABS_KEY)
            .unwrap();

        *h.speech.delay.lock().unwrap() = Duration::from_millis(100);
        let generating = app
            .generate_narration(project.id, BudgetConsent::Ask)
            .unwrap();
        assert!(matches!(
            app.import_narration(project.id, &recording, BudgetConsent::Ask),
            Err(NarrationError::Busy)
        ));
        wait_done(&app, generating);
        let importing = app
            .import_narration(project.id, &recording, BudgetConsent::Ask)
            .unwrap();
        assert!(matches!(
            app.generate_narration(project.id, BudgetConsent::Ask),
            Err(NarrationError::Busy)
        ));
        wait_done(&app, importing);
    }

    #[test]
    fn a_project_without_a_script_has_nothing_to_align_with() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = h.start();
        let channel = app
            .create_channel(bardo_domain::ChannelDraft {
                name: "Empty".into(),
                ..bardo_domain::ChannelDraft::default()
            })
            .unwrap();
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel.id,
            bardo_domain::Niche::new("space").unwrap(),
            bardo_domain::ThemeIdea::new("Idea", "").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let project = theme.approve(SystemTime::now()).unwrap();
        app.themes.start_project(&theme, &project).unwrap();
        let recording = Recording::open(&user_file(&dir, "take.wav", &silent_wav(1))).unwrap();

        assert!(matches!(
            app.import_narration(project.id, &recording, BudgetConsent::Ask),
            Err(NarrationError::NoScript)
        ));
    }

    #[test]
    fn a_failed_alignment_keeps_the_current_narration() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        *h.aligner.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Unexpected,
            "the recording was not accepted: Field required",
        ));
        let app = h.start();
        let project = project(&app);
        let generated = narrate(&app, &project);
        let recording = Recording::open(&user_file(&dir, "take.wav", &silent_wav(1))).unwrap();

        let id = app
            .import_narration(project.id, &recording, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, id);

        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::UnexpectedAnswer);
        assert_eq!(
            failure.detail,
            "ElevenLabs: the recording was not accepted: Field required"
        );
        let view = app.narration(project.id).unwrap();
        assert_eq!(view.narration, Some(generated));
        assert_eq!(view.job.map(|job| job.id()), Some(id), "the panel says why");
        assert!(
            h.db.project_costs(project.id)
                .unwrap()
                .iter()
                .all(|record| { record.purpose != CostPurpose::NarrationAlignment })
        );
    }

    /// Replays an import job as the queue would after a restart.
    fn handler(h: &Harness, app: &Bardo) -> NarrationImportHandler {
        NarrationImportHandler {
            owner: app.profile().id,
            narrations: Arc::clone(&h.db) as _,
            files: Arc::clone(&h.files),
            aligner: Arc::clone(&h.aligner) as _,
            secrets: Arc::clone(&h.secrets) as _,
            costs: CostBook {
                owner: app.profile().id,
                costs: Arc::clone(&h.db) as _,
                themes: Arc::clone(&h.db) as _,
            },
        }
    }

    #[test]
    fn a_resumed_job_neither_aligns_twice_nor_needs_the_original_file() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let path = user_file(&dir, "take.wav", &silent_wav(1));
        let recording = Recording::open(&path).unwrap();
        let job = import(&app, project.id, &recording);
        let saved = app.narration(project.id).unwrap().narration;

        handler(&h, &app)
            .import(job.payload(), job.id(), None)
            .unwrap();
        assert_eq!(h.aligner.requests().len(), 1, "ElevenLabs is asked once");
        assert_eq!(app.narration(project.id).unwrap().narration, saved);

        // A retry after the copy reads the copy, even with the file gone.
        std::fs::remove_file(&path).unwrap();
        let retried = JobId::new();
        handler(&h, &app)
            .import(job.payload(), retried, None)
            .unwrap();
        let narration = app.narration(project.id).unwrap().narration.unwrap();
        assert_eq!(narration.job, Some(retried));
        assert_eq!(narration.duration, Duration::from_secs(1));
    }

    #[test]
    fn a_recording_moved_before_the_job_ran_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let path = user_file(&dir, "take.wav", &silent_wav(1));
        let recording = Recording::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        let id = app
            .import_narration(project.id, &recording, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, id);

        assert_eq!(job.state(), JobState::Failed);
        assert!(
            job.failure()
                .unwrap()
                .detail
                .starts_with("could not read the recording take.wav"),
            "{:?}",
            job.failure()
        );
        assert!(h.aligner.requests().is_empty());
    }

    #[test]
    fn a_recording_replaced_after_it_was_chosen_is_not_aligned() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let path = user_file(&dir, "take.wav", &silent_wav(1));
        let recording = Recording::open(&path).unwrap();
        std::fs::write(&path, silent_wav(30)).unwrap();

        let id = app
            .import_narration(project.id, &recording, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, id);

        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().unwrap().detail,
            "take.wav is no longer the audio that was chosen"
        );
        assert!(h.aligner.requests().is_empty(), "nothing is paid for");
    }

    #[test]
    fn a_new_import_clears_the_copy_a_failed_one_left() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::new();
        *h.aligner.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Unexpected,
            "the recording was not accepted: Field required",
        ));
        let app = h.start();
        let project = project(&app);
        let recording = Recording::open(&user_file(&dir, "take.wav", &silent_wav(1))).unwrap();
        let failed = wait_done(
            &app,
            app.import_narration(project.id, &recording, BudgetConsent::Ask)
                .unwrap(),
        );
        let left = audio_file(
            ImportPayload::parse(failed.payload())
                .unwrap()
                .narration
                .parse::<uuid::Uuid>()
                .map(NarrationId::from)
                .unwrap(),
            AudioFormat::Wav,
        );
        assert!(h.files.exists(project.id, &left), "kept for a retry");

        *h.aligner.failure.lock().unwrap() = None;
        let job = import(&app, project.id, &recording);

        assert!(!h.files.exists(project.id, &left));
        let narration = app.narration(project.id).unwrap().narration.unwrap();
        assert_eq!(narration.job, Some(job.id()));
        assert!(h.files.exists(project.id, &narration.audio_file));
    }

    #[test]
    fn the_recording_lands_in_the_projects_folder_on_disk() {
        let user = tempfile::tempdir().unwrap();
        let bardo = tempfile::tempdir().unwrap();
        let h = Harness::with_files(Arc::new(LocalProjectFiles::new(bardo.path())));
        let app = h.start();
        let project = project(&app);
        let recording = Recording::open(&user_file(&user, "take.wav", &silent_wav(1))).unwrap();

        import(&app, project.id, &recording);

        let narration = app.narration(project.id).unwrap().narration.unwrap();
        let copy = bardo
            .path()
            .join(project.id.to_string())
            .join(&narration.audio_file);
        assert_eq!(std::fs::read(copy).unwrap(), silent_wav(1));
        assert!(user.path().join("take.wav").exists(), "the original stays");
    }
}
