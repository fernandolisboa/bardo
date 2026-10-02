//! Narration use cases (PRD stories 31-32): the persona's voice reads a
//! video project's script; the audio goes into the project folder with the
//! timing of every word, and the user plays it back with the current word
//! highlighted.
//!
//! The reader is the channel's default persona (a per-video override comes
//! with persona sharing). Generating calls ElevenLabs, so it runs as a job.
//! Long scripts are read in parts, one request each; every finished part
//! is saved in the project folder before the next starts, so a cancelled,
//! failed or interrupted job resumes without paying for parts again. When
//! all parts are in, they are joined into one MP3 and the word timings are
//! mapped onto the script's words.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    Alignment, ApiKey, CharTiming, GenerationPresets, Job, JobFailure, JobFailureKind, JobId,
    JobKind, Narration, NarrationId, NarrationRepository, Persona, ProfileId, Progress,
    ProjectFiles, Provider, RepositoryError, Script, ScriptText, SecretStore, SpeechRequest,
    SpeechSynthesizer, VideoProject, VideoProjectId, VoiceRef, WordTimings, split_for_speech,
};
use bardo_media::{Playback, PlaybackError, mp3};
use serde::{Deserialize, Serialize};

use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, KeyState, Text};

#[derive(Debug, thiserror::Error)]
pub enum NarrationError {
    #[error("video project not found")]
    ProjectNotFound,
    /// Narration reads the script: generate one first.
    #[error("the project has no script yet")]
    NoScript,
    /// The channel has no default persona to read it.
    #[error("the channel has no default persona")]
    NoPersona,
    /// Generation calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// A narration of the project is being generated.
    #[error("a narration is already being generated for this project")]
    Busy,
    /// Playback needs a narration.
    #[error("the project has no narration yet")]
    NoNarration,
    /// The narration's audio file is gone from the project folder.
    #[error("the narration's audio file is missing")]
    AudioMissing,
    #[error(transparent)]
    Playback(#[from] PlaybackError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl NarrationError {
    /// What the projects screen says.
    pub fn message(&self) -> Text {
        match self {
            NarrationError::ProjectNotFound => Text::ProjectNotFound,
            NarrationError::NoScript => Text::NarrationNoScript,
            NarrationError::NoPersona => Text::NarrationNoPersona,
            NarrationError::MissingKey(_) => Text::NarrationMissingKey,
            NarrationError::Busy => Text::NarrationBusy,
            NarrationError::NoNarration => Text::NarrationMissing,
            NarrationError::AudioMissing => Text::NarrationAudioMissing,
            NarrationError::Playback(_) => Text::NarrationCannotPlay,
            NarrationError::Repository(_) => Text::NarrationNotLoaded,
        }
    }
}

/// A video project's narration panel.
#[derive(Debug, Clone, PartialEq)]
pub struct NarrationView {
    pub project: VideoProject,
    /// The text the next narration reads; `None` until a script exists.
    pub script: Option<ScriptText>,
    /// The current narration, if one was generated.
    pub narration: Option<Narration>,
    /// Whether the script changed since the narration read it.
    pub stale: bool,
    /// The project's latest narration job.
    pub job: Option<Job>,
    /// Who reads the next narration: the channel's default persona.
    pub persona: Option<Persona>,
}

impl NarrationView {
    /// Characters the next narration sends to the provider, which is what
    /// it bills.
    pub fn characters(&self) -> usize {
        self.script
            .as_ref()
            .map_or(0, |text| text.as_str().chars().count())
    }
}

/// The narration job's payload: what to read, with which voice and
/// presets, and the narration's id (its files are named after it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NarrationPayload {
    project: String,
    narration: String,
    voice_provider: String,
    voice_id: String,
    voice_name: String,
    stability: u8,
    similarity: u8,
    style: u8,
    speed: u8,
    text: String,
}

impl NarrationPayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a narration payload serializes")
    }

    fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid narration payload: {e}")))
    }

    fn project(&self) -> Result<VideoProjectId, JobFailure> {
        uuid::Uuid::parse_str(&self.project)
            .map(VideoProjectId::from)
            .map_err(unexpected)
    }

    fn narration(&self) -> Result<NarrationId, JobFailure> {
        uuid::Uuid::parse_str(&self.narration)
            .map(NarrationId::from)
            .map_err(unexpected)
    }

    fn voice(&self) -> Result<VoiceRef, JobFailure> {
        let provider: Provider = self.voice_provider.parse().map_err(unexpected)?;
        VoiceRef::new(provider, &self.voice_id, &self.voice_name).map_err(unexpected)
    }

    fn presets(&self) -> GenerationPresets {
        GenerationPresets {
            stability: self.stability,
            similarity: self.similarity,
            style: self.style,
            speed: self.speed,
        }
    }
}

/// What a narration job reads, from its payload.
struct Reading {
    project: VideoProjectId,
    id: NarrationId,
    text: ScriptText,
    voice: VoiceRef,
    presets: GenerationPresets,
}

impl Reading {
    fn from_payload(payload: &NarrationPayload) -> Result<Self, JobFailure> {
        Ok(Self {
            project: payload.project()?,
            id: payload.narration()?,
            text: ScriptText::new(&payload.text).map_err(|e| unexpected(format!("{e:?}")))?,
            voice: payload.voice()?,
            presets: payload.presets(),
        })
    }
}

/// One finished part, saved next to its audio until the parts are joined.
#[derive(Debug, Serialize, Deserialize)]
struct PartRecord {
    model: String,
    billed_characters: u64,
    /// Character timings: text, start and end in microseconds.
    chars: Vec<(String, u64, u64)>,
}

impl PartRecord {
    fn new(model: String, billed_characters: u64, alignment: &Alignment) -> Self {
        let micros = |d: Duration| u64::try_from(d.as_micros()).unwrap_or(u64::MAX);
        Self {
            model,
            billed_characters,
            chars: alignment
                .chars
                .iter()
                .map(|c| (c.text.clone(), micros(c.start), micros(c.end)))
                .collect(),
        }
    }

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

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// The narration's audio file in the project folder.
pub(crate) fn audio_file(narration: NarrationId) -> String {
    format!("narration-{narration}.mp3")
}

fn part_audio(narration: NarrationId, index: usize) -> String {
    format!("narration-{narration}.part{:03}.mp3", index + 1)
}

fn part_record(narration: NarrationId, index: usize) -> String {
    format!("narration-{narration}.part{:03}.json", index + 1)
}

/// Runs narration jobs.
pub(crate) struct NarrationHandler {
    pub(crate) owner: ProfileId,
    pub(crate) narrations: Arc<dyn NarrationRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) speech: Arc<dyn SpeechSynthesizer>,
    pub(crate) secrets: Arc<dyn SecretStore>,
}

impl NarrationHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::ElevenLabs)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(JobFailureKind::MissingKey, "no ElevenLabs key is saved")
            })
    }

    /// The part saved by an earlier attempt, if it was saved whole (its
    /// record is written after its audio).
    fn saved_part(&self, project: VideoProjectId, narration: NarrationId, index: usize) -> bool {
        self.files.exists(project, &part_record(narration, index))
            && self.files.exists(project, &part_audio(narration, index))
    }
}

impl JobHandler for NarrationHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.narrate(payload, cx.id(), Some(cx))
    }
}

impl NarrationHandler {
    /// Reads the text `job` asks for and saves the narration. Without a
    /// context (tests replaying a job) nothing reports progress or stops it.
    fn narrate(
        &self,
        payload: &str,
        job: JobId,
        mut cx: Option<&mut JobContext>,
    ) -> Result<(), JobFailure> {
        let reading = Reading::from_payload(&NarrationPayload::parse(payload)?)?;
        let (project, id) = (reading.project, reading.id);
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
        let text = reading.text.as_str();
        let parts = split_for_speech(text, self.speech.max_chars());
        let piece = |index: usize| parts.get(index).map(|range| text[range.clone()].to_owned());

        let mut key = None;
        for index in 0..parts.len() {
            if self.saved_part(project, id, index) {
                continue;
            }
            if cx.as_ref().is_some_and(|cx| cx.should_stop()) {
                return Ok(());
            }
            let key = match &key {
                Some(key) => key,
                None => key.insert(self.key()?),
            };
            let request = SpeechRequest {
                voice: reading.voice.clone(),
                presets: reading.presets,
                text: piece(index).unwrap_or_default(),
                previous_text: index.checked_sub(1).and_then(piece),
                next_text: piece(index + 1),
            };
            let speech = self.speech.synthesize(key, &request).map_err(|failure| {
                JobFailure::new(
                    failure.kind.into(),
                    format!("ElevenLabs: {}", failure.detail),
                )
            })?;
            let record = PartRecord::new(speech.model, speech.billed_characters, &speech.alignment);
            let record = serde_json::to_vec(&record).map_err(unexpected)?;
            self.files
                .write(project, &part_audio(id, index), &speech.audio)
                .map_err(unexpected)?;
            self.files
                .write(project, &part_record(id, index), &record)
                .map_err(unexpected)?;
            // Joining is quick; the parts are most of the work.
            if let Some(cx) = cx.as_mut() {
                let permille = (index + 1) * 950 / parts.len();
                cx.save_checkpoint(
                    format!("{} of {} parts", index + 1, parts.len()),
                    Progress::from_permille(permille as u16),
                )
                .map_err(unexpected)?;
            }
        }

        let narration = self.join(reading, parts.len(), job)?;
        let previous = self.narrations.narration(project).map_err(unexpected)?;
        self.narrations
            .save_narration(&narration)
            .map_err(unexpected)?;
        // Leftovers only cost disk space: a file that will not go (open in
        // a player) stays rather than failing a finished narration.
        for index in 0..parts.len() {
            let _ = self.files.remove(project, &part_audio(id, index));
            let _ = self.files.remove(project, &part_record(id, index));
        }
        if let Some(previous) = previous.filter(|p| p.audio_file != narration.audio_file) {
            let _ = self.files.remove(project, &previous.audio_file);
        }
        Ok(())
    }

    /// Joins the saved parts into the narration's audio file and maps their
    /// timings onto the script's words.
    fn join(&self, reading: Reading, parts: usize, job: JobId) -> Result<Narration, JobFailure> {
        let Reading {
            project,
            id,
            text,
            voice,
            presets,
        } = reading;
        let mut audio = Vec::with_capacity(parts);
        let mut records = Vec::with_capacity(parts);
        for index in 0..parts {
            audio.push(
                self.files
                    .read(project, &part_audio(id, index))
                    .map_err(unexpected)?,
            );
            let record = self
                .files
                .read(project, &part_record(id, index))
                .map_err(unexpected)?;
            records.push(serde_json::from_slice::<PartRecord>(&record).map_err(unexpected)?);
        }
        let slices: Vec<&[u8]> = audio.iter().map(Vec::as_slice).collect();
        let joined = mp3::join(&slices).map_err(|error| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("ElevenLabs: the audio is not usable ({error})"),
            )
        })?;
        let mut alignment = Alignment::default();
        for (record, start) in records.iter().zip(&joined.starts) {
            alignment.append(&record.alignment(), *start);
        }
        let words = WordTimings::from_alignment(text.as_str(), &alignment);
        let file = audio_file(id);
        self.files
            .write(project, &file, &joined.bytes)
            .map_err(unexpected)?;
        Ok(Narration {
            id,
            project,
            owner: self.owner,
            text,
            voice,
            presets,
            model: records
                .first()
                .map(|record| record.model.clone())
                .unwrap_or_default(),
            billed_characters: records.iter().map(|r| r.billed_characters).sum(),
            audio_file: file,
            duration: joined.duration,
            words,
            generated_at: SystemTime::now(),
            job: Some(job),
        })
    }
}

/// A narration loaded for playback, with the word being spoken. Lives with
/// the view that plays it; not shared between threads.
pub struct NarrationPlayer {
    narration: Narration,
    playback: Box<dyn Playback>,
}

impl NarrationPlayer {
    /// The narration being played.
    pub fn narration(&self) -> &Narration {
        &self.narration
    }

    pub fn is_playing(&self) -> bool {
        self.playback.is_playing()
    }

    /// Plays when paused, pauses when playing.
    pub fn toggle(&mut self) -> Result<(), NarrationError> {
        if self.playback.is_playing() {
            self.playback.pause();
            Ok(())
        } else {
            Ok(self.playback.play()?)
        }
    }

    pub fn pause(&mut self) {
        self.playback.pause();
    }

    /// Where playback is, never past the end.
    pub fn position(&self) -> Duration {
        self.playback.position().min(self.narration.duration)
    }

    pub fn duration(&self) -> Duration {
        self.narration.duration
    }

    /// The index of the word being spoken (in `Narration::words`), which
    /// the screen highlights.
    pub fn current_word(&self) -> Option<usize> {
        self.narration.words.word_at(self.position())
    }

    /// Moves playback to where word `index` starts.
    pub fn seek_to_word(&mut self, index: usize) -> Result<(), NarrationError> {
        let Some(word) = self.narration.words.as_slice().get(index) else {
            return Ok(());
        };
        Ok(self.playback.seek(word.start)?)
    }
}

impl Bardo {
    fn narration_project(&self, id: VideoProjectId) -> Result<VideoProject, NarrationError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(NarrationError::ProjectNotFound)
    }

    fn latest_narration_job(&self, project: VideoProjectId) -> Option<Job> {
        let project = project.to_string();
        self.jobs().into_iter().rev().find(|job| {
            job.kind() == JobKind::Narration
                && NarrationPayload::parse(job.payload()).is_ok_and(|p| p.project == project)
        })
    }

    /// The channel's default persona, which reads the project's narration.
    fn narrator(&self, project: &VideoProject) -> Result<Option<Persona>, NarrationError> {
        let Some(channel) = self.channels.get(project.channel)? else {
            return Ok(None);
        };
        let Some(id) = channel.details.default_persona() else {
            return Ok(None);
        };
        Ok(self
            .personas
            .get(id)?
            .filter(|persona| persona.owner == self.profile.id))
    }

    /// The project's narration panel.
    pub fn narration(&self, project: VideoProjectId) -> Result<NarrationView, NarrationError> {
        let project = self.narration_project(project)?;
        let script: Option<Script> = self.scripts.script(project.id)?;
        let narration = self.narrations.narration(project.id)?;
        let stale = match (&narration, &script) {
            (Some(narration), Some(script)) => narration.is_stale(script),
            _ => false,
        };
        Ok(NarrationView {
            script: script.map(|script| script.text().clone()),
            narration,
            stale,
            job: self.latest_narration_job(project.id),
            persona: self.narrator(&project)?,
            project,
        })
    }

    /// Starts a job in which the channel's default persona reads the
    /// project's current script. The new narration replaces the current
    /// one once it is complete.
    pub fn generate_narration(&self, project: VideoProjectId) -> Result<JobId, NarrationError> {
        let project = self.narration_project(project)?;
        let script = self
            .scripts
            .script(project.id)?
            .ok_or(NarrationError::NoScript)?;
        let persona = self.narrator(&project)?.ok_or(NarrationError::NoPersona)?;
        if self
            .latest_narration_job(project.id)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(NarrationError::Busy);
        }
        let provider = persona.details.voice().provider();
        if self.provider_key(provider).state == KeyState::NotSet {
            return Err(NarrationError::MissingKey(provider));
        }
        let voice = persona.details.voice();
        let presets = persona.details.presets();
        let payload = NarrationPayload {
            project: project.id.to_string(),
            narration: NarrationId::new().to_string(),
            voice_provider: voice.provider().code().to_owned(),
            voice_id: voice.id().to_owned(),
            voice_name: voice.name().to_owned(),
            stability: presets.stability,
            similarity: presets.similarity,
            style: presets.style,
            speed: presets.speed,
            text: script.text().as_str().to_owned(),
        };
        let job = Job::new(self.profile.id, JobKind::Narration, payload.to_json());
        Ok(self.jobs.enqueue(job)?)
    }

    /// Loads the project's narration for playback, paused at the start.
    pub fn play_narration(
        &self,
        project: VideoProjectId,
    ) -> Result<NarrationPlayer, NarrationError> {
        let project = self.narration_project(project)?;
        let narration = self
            .narrations
            .narration(project.id)?
            .ok_or(NarrationError::NoNarration)?;
        if !self.files.exists(project.id, &narration.audio_file) {
            return Err(NarrationError::AudioMissing);
        }
        let playback = self
            .audio
            .open(&self.files.path(project.id, &narration.audio_file))?;
        Ok(NarrationPlayer {
            narration,
            playback,
        })
    }
}

/// Plays nothing; a test moves the position by hand.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use bardo_media::{AudioOutput, Playback, PlaybackError};

    #[derive(Debug, Default)]
    pub(crate) struct FakePlaybackState {
        pub(crate) position: Duration,
        pub(crate) playing: bool,
    }

    /// Opens every path; remembers which, and shares the state of the last
    /// playback with the test.
    #[derive(Default)]
    pub(crate) struct FakeAudioOutput {
        pub(crate) opened: Mutex<Vec<PathBuf>>,
        pub(crate) state: Arc<Mutex<FakePlaybackState>>,
        pub(crate) failure: Mutex<Option<PlaybackError>>,
    }

    impl FakeAudioOutput {
        pub(crate) fn set_position(&self, position: Duration) {
            self.state.lock().unwrap().position = position;
        }
    }

    struct FakePlayback(Arc<Mutex<FakePlaybackState>>);

    impl Playback for FakePlayback {
        fn play(&mut self) -> Result<(), PlaybackError> {
            self.0.lock().unwrap().playing = true;
            Ok(())
        }

        fn pause(&mut self) {
            self.0.lock().unwrap().playing = false;
        }

        fn is_playing(&self) -> bool {
            self.0.lock().unwrap().playing
        }

        fn seek(&mut self, position: Duration) -> Result<(), PlaybackError> {
            self.0.lock().unwrap().position = position;
            Ok(())
        }

        fn position(&self) -> Duration {
            self.0.lock().unwrap().position
        }
    }

    impl AudioOutput for FakeAudioOutput {
        fn open(&self, path: &Path) -> Result<Box<dyn Playback>, PlaybackError> {
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            self.opened.lock().unwrap().push(path.to_owned());
            *self.state.lock().unwrap() = FakePlaybackState::default();
            Ok(Box::new(FakePlayback(Arc::clone(&self.state))))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use bardo_domain::{
        ChannelDraft, ContentLanguage, Country, JobState, ProviderFailure, ProviderFailureKind,
        spoken_words,
    };
    use bardo_storage::{Database, LocalProjectFiles, MemoryProjectFiles, MemorySecretStore};

    use super::testing::FakeAudioOutput;
    use super::*;
    use crate::testing::{
        FakeDecisionEngine, FakeKeyChecker, FakeMarketData, FakeSpeech, FakeTextGenerator,
        PART_AUDIO,
    };
    use crate::{JobSettings, Providers, Repositories};

    const CLAUDE_KEY: &str = "sk-ant-api03-test-key-0001";
    const ELEVENLABS_KEY: &str = "sk_test_elevenlabs_key_0001";
    const PATIENCE: Duration = Duration::from_secs(10);
    const SCRIPT: &str = "Era uma vez, em 1969, uma sonda. Ela partiu — e nunca voltou.";

    struct Harness {
        db: Arc<Database>,
        files: Arc<dyn ProjectFiles>,
        speech: Arc<FakeSpeech>,
        audio: Arc<FakeAudioOutput>,
        secrets: Arc<MemorySecretStore>,
    }

    impl Harness {
        fn new() -> Self {
            Self::with_files(Arc::new(MemoryProjectFiles::default()))
        }

        fn with_files(files: Arc<dyn ProjectFiles>) -> Self {
            Self {
                db: Arc::new(Database::open_in_memory().unwrap()),
                files,
                speech: Arc::default(),
                audio: Arc::default(),
                secrets: Arc::default(),
            }
        }

        fn start(&self) -> Bardo {
            let text = FakeTextGenerator::default();
            text.answers.lock().unwrap().push(SCRIPT.to_owned());
            let providers = Providers {
                key_checker: Arc::new(FakeKeyChecker::default()),
                market_data: Arc::new(FakeMarketData::default()),
                text: Arc::new(text),
                decisions: Arc::new(FakeDecisionEngine::default()),
                voices: Arc::new(crate::testing::FakeVoiceLibrary::default()),
                speech: Arc::clone(&self.speech) as _,
                images: Arc::new(crate::testing::FakeImages::default()),
                audio: Arc::clone(&self.audio) as _,
            };
            let mut app = Bardo::start_with(
                Repositories::shared_with_files(
                    Arc::clone(&self.db),
                    Arc::clone(&self.secrets) as _,
                    Arc::clone(&self.files),
                ),
                providers,
                Some("en-US"),
                JobSettings {
                    retry: bardo_domain::RetryPolicy {
                        max_attempts: 2,
                        first_delay: Duration::from_millis(20),
                        max_delay: Duration::from_millis(20),
                    },
                    ..JobSettings::default()
                },
            )
            .unwrap();
            app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
            app.save_provider_key(Provider::ElevenLabs, ELEVENLABS_KEY)
                .unwrap();
            app
        }

        /// The fake reads `max_chars` characters per request.
        fn parts_of(&self, max_chars: usize) {
            *self.speech.max_chars.lock().unwrap() = max_chars;
        }
    }

    fn wait_done(app: &Bardo, id: JobId) -> Job {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(job) = app
                .jobs()
                .into_iter()
                .find(|j| j.id() == id && !j.state().is_active())
            {
                return job;
            }
            assert!(Instant::now() < deadline, "job {id} never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// A channel whose default persona is the documentary narrator, and a
    /// project with a generated script.
    fn project(app: &Bardo) -> VideoProject {
        let project = project_without_persona(app, "Space Archives");
        let persona = documentary_narrator(app);
        let channel = app.channels.get(project.channel).unwrap().unwrap();
        app.update_channel(
            channel.id,
            ChannelDraft {
                default_persona: Some(persona.id),
                ..ChannelDraft::from(&channel.details)
            },
        )
        .unwrap();
        project
    }

    fn documentary_narrator(app: &Bardo) -> Persona {
        app.personas()
            .unwrap()
            .into_iter()
            .find(|p| p.details.name() == "Documentary Narrator (en-US)")
            .unwrap()
    }

    fn project_without_persona(app: &Bardo, channel: &str) -> VideoProject {
        let channel = app
            .create_channel(ChannelDraft {
                name: channel.into(),
                niche: "space history".into(),
                language: ContentLanguage::Portuguese,
                country: Country::Brazil,
                ..ChannelDraft::default()
            })
            .unwrap();
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel.id,
            bardo_domain::Niche::new("space history").unwrap(),
            bardo_domain::ThemeIdea::new("The probe that never came home", "").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let project = theme.approve(SystemTime::now()).unwrap();
        app.themes.start_project(&theme, &project).unwrap();
        let job = app.generate_script(project.id).unwrap();
        assert_eq!(wait_done(app, job).state(), JobState::Done);
        project
    }

    fn narrate(app: &Bardo, project: &VideoProject) -> Narration {
        let id = app.generate_narration(project.id).unwrap();
        let job = wait_done(app, id);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        app.narration(project.id).unwrap().narration.unwrap()
    }

    /// How long a fake part plays, in whole milliseconds as stored.
    fn part_duration() -> Duration {
        stored(mp3::frames(PART_AUDIO).unwrap().duration())
    }

    fn stored(duration: Duration) -> Duration {
        Duration::from_millis(duration.as_millis() as u64)
    }

    #[test]
    fn the_channel_persona_reads_the_script_into_the_project_folder() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let persona = documentary_narrator(&app);
        let before = app.narration(project.id).unwrap();
        assert_eq!(before.narration, None);
        assert_eq!(before.persona.as_ref(), Some(&persona));
        assert_eq!(before.characters(), SCRIPT.chars().count());

        let narration = narrate(&app, &project);

        let requests = h.speech.requests();
        assert_eq!(requests.len(), 1, "a short script is one request");
        assert_eq!(requests[0].text, SCRIPT);
        assert_eq!(&requests[0].voice, persona.details.voice());
        assert_eq!(requests[0].presets, persona.details.presets());
        assert_eq!(requests[0].previous_text, None);

        assert_eq!(narration.text.as_str(), SCRIPT);
        assert_eq!(narration.voice.name(), "Wyatt");
        assert_eq!(narration.voice.provider(), Provider::ElevenLabs);
        assert_eq!(narration.presets, persona.details.presets());
        assert_eq!(narration.model, "eleven-fake");
        assert_eq!(narration.billed_characters, SCRIPT.chars().count() as u64);
        assert_eq!(narration.audio_file, audio_file(narration.id));
        assert_eq!(narration.duration, part_duration());
        let job = app.narration(project.id).unwrap().job.unwrap();
        assert_eq!(narration.job, Some(job.id()));
        assert_eq!(job.kind(), JobKind::Narration);

        let audio = h.files.read(project.id, &narration.audio_file).unwrap();
        assert_eq!(
            mp3::frames(&audio).unwrap().duration(),
            mp3::frames(PART_AUDIO).unwrap().duration()
        );
        assert_eq!(narration.words.len(), spoken_words(SCRIPT).len());
        let words: Vec<_> = narration.words().map(|(word, _)| word).collect();
        assert_eq!(words[4], "1969,");
        assert_eq!(words[8], "partiu —");
    }

    #[test]
    fn long_scripts_are_read_in_parts_and_joined_in_time() {
        let h = Harness::new();
        h.parts_of(36);
        let app = h.start();
        let project = project(&app);

        let narration = narrate(&app, &project);

        let requests = h.speech.requests();
        let texts: Vec<_> = requests.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Era uma vez, em 1969, uma sonda.",
                "Ela partiu — e nunca voltou."
            ]
        );
        assert_eq!(requests[0].next_text.as_deref(), Some(texts[1]));
        assert_eq!(requests[1].previous_text.as_deref(), Some(texts[0]));
        assert_eq!(
            narration.duration,
            stored(mp3::frames(PART_AUDIO).unwrap().duration() * 2)
        );

        // "Ela" opens the second part, which starts when the first ends.
        let (word, timing) = narration.words().nth(7).unwrap();
        assert_eq!(word, "Ela");
        assert_eq!(timing.start, part_duration());
        let (_, last) = narration.words().last().unwrap();
        assert!(last.end <= part_duration() + Duration::from_secs(1));
        assert_eq!(
            h.files
                .as_ref()
                .read(project.id, &narration.audio_file)
                .map(|audio| mp3::frames(&audio).unwrap().frames.len())
                .unwrap(),
            mp3::frames(PART_AUDIO).unwrap().frames.len() * 2
        );
        for index in 0..2 {
            assert!(!h.files.exists(project.id, &part_audio(narration.id, index)));
            assert!(
                !h.files
                    .exists(project.id, &part_record(narration.id, index))
            );
        }
    }

    #[test]
    fn a_failed_part_resumes_without_reading_earlier_parts_again() {
        let h = Harness::new();
        h.parts_of(36);
        *h.speech.fail_at.lock().unwrap() = Some(1);
        let app = h.start();
        let project = project(&app);

        narrate(&app, &project);

        let texts: Vec<_> = h.speech.requests().into_iter().map(|r| r.text).collect();
        assert_eq!(
            texts,
            [
                "Era uma vez, em 1969, uma sonda.",
                "Ela partiu — e nunca voltou.",
                "Ela partiu — e nunca voltou.",
            ],
            "the first part is paid once"
        );
    }

    #[test]
    fn a_resumed_job_does_not_read_twice() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let saved = narrate(&app, &project);
        let job = app
            .jobs()
            .into_iter()
            .find(|job| Some(job.id()) == saved.job)
            .unwrap();

        // Replays the job as if the app had stopped right after saving.
        let handler = NarrationHandler {
            owner: app.profile().id,
            narrations: Arc::clone(&h.db) as _,
            files: Arc::clone(&h.files),
            speech: Arc::clone(&h.speech) as _,
            secrets: Arc::clone(&h.secrets) as _,
        };
        handler.narrate(job.payload(), job.id(), None).unwrap();

        assert_eq!(h.speech.requests().len(), 1, "ElevenLabs is asked once");
        assert_eq!(app.narration(project.id).unwrap().narration, Some(saved));
    }

    #[test]
    fn playback_highlights_the_word_being_spoken() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let narration = narrate(&app, &project);

        let mut player = app.play_narration(project.id).unwrap();
        assert_eq!(
            h.audio.opened.lock().unwrap().as_slice(),
            [h.files.path(project.id, &narration.audio_file)]
        );
        assert!(!player.is_playing(), "opens paused");
        assert_eq!(player.duration(), narration.duration);
        assert_eq!(player.current_word(), Some(0));

        player.toggle().unwrap();
        assert!(player.is_playing());
        for (index, (_, timing)) in narration.words().enumerate() {
            h.audio.set_position(timing.start);
            assert_eq!(player.current_word(), Some(index));
        }
        h.audio.set_position(Duration::from_secs(60));
        assert_eq!(player.position(), narration.duration, "never past the end");

        player.seek_to_word(3).unwrap();
        assert_eq!(player.current_word(), Some(3));
        assert_eq!(player.position(), narration.words.as_slice()[3].start);
        player.toggle().unwrap();
        assert!(!player.is_playing());
    }

    #[test]
    fn changing_the_script_makes_the_narration_stale_until_read_again() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let first = narrate(&app, &project);
        assert!(!app.narration(project.id).unwrap().stale);

        app.edit_script(project.id, "Era uma vez uma sonda perdida.")
            .unwrap();
        let view = app.narration(project.id).unwrap();
        assert!(view.stale);
        assert_eq!(view.narration.as_ref(), Some(&first), "kept until replaced");

        let second = narrate(&app, &project);
        assert!(!app.narration(project.id).unwrap().stale);
        assert_eq!(second.text.as_str(), "Era uma vez uma sonda perdida.");
        assert!(h.files.exists(project.id, &second.audio_file));
        assert!(
            !h.files.exists(project.id, &first.audio_file),
            "the replaced audio is removed"
        );
    }

    #[test]
    fn generating_needs_a_script_a_persona_a_key_and_one_job_at_a_time() {
        let h = Harness::new();
        let mut app = h.start();
        let bare = project_without_persona(&app, "No Narrator");
        assert!(matches!(
            app.generate_narration(bare.id),
            Err(NarrationError::NoPersona)
        ));
        assert_eq!(
            NarrationError::NoPersona.message(),
            Text::NarrationNoPersona
        );
        assert!(matches!(
            app.generate_narration(VideoProjectId::new()),
            Err(NarrationError::ProjectNotFound)
        ));

        let project = project(&app);
        app.remove_provider_key(Provider::ElevenLabs).unwrap();
        let error = app.generate_narration(project.id).unwrap_err();
        assert!(matches!(
            error,
            NarrationError::MissingKey(Provider::ElevenLabs)
        ));
        assert_eq!(error.message(), Text::NarrationMissingKey);
        app.save_provider_key(Provider::ElevenLabs, ELEVENLABS_KEY)
            .unwrap();

        *h.speech.delay.lock().unwrap() = Duration::from_millis(100);
        let id = app.generate_narration(project.id).unwrap();
        let busy = app.generate_narration(project.id).unwrap_err();
        assert!(matches!(busy, NarrationError::Busy));
        assert_eq!(busy.message(), Text::NarrationBusy);
        wait_done(&app, id);
        assert!(app.generate_narration(project.id).is_ok());
    }

    #[test]
    fn a_project_without_a_script_cannot_be_narrated() {
        let h = Harness::new();
        let app = h.start();
        let channel = app
            .create_channel(ChannelDraft {
                name: "Empty".into(),
                ..ChannelDraft::default()
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

        assert_eq!(app.narration(project.id).unwrap().script, None);
        assert!(matches!(
            app.generate_narration(project.id),
            Err(NarrationError::NoScript)
        ));
        assert!(matches!(
            app.play_narration(project.id),
            Err(NarrationError::NoNarration)
        ));
    }

    #[test]
    fn a_provider_failure_saves_nothing() {
        let h = Harness::new();
        *h.speech.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "Invalid API key",
        ));
        let app = h.start();
        let project = project(&app);
        let id = app.generate_narration(project.id).unwrap();

        let job = wait_done(&app, id);
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::KeyRejected);
        assert_eq!(failure.detail, "ElevenLabs: Invalid API key");
        assert_eq!(app.narration(project.id).unwrap().narration, None);
    }

    #[test]
    fn missing_audio_or_output_is_reported() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let narration = narrate(&app, &project);

        *h.audio.failure.lock().unwrap() = Some(PlaybackError::NoOutput("no device".into()));
        let error = app.play_narration(project.id).err().unwrap();
        assert!(matches!(error, NarrationError::Playback(_)));
        assert_eq!(error.message(), Text::NarrationCannotPlay);

        h.files.remove(project.id, &narration.audio_file).unwrap();
        let error = app.play_narration(project.id).err().unwrap();
        assert!(matches!(error, NarrationError::AudioMissing));
        assert_eq!(error.message(), Text::NarrationAudioMissing);
    }

    #[test]
    fn the_audio_lands_in_the_projects_folder_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let h = Harness::with_files(Arc::new(LocalProjectFiles::new(dir.path())));
        let app = h.start();
        let project = project(&app);

        let narration = narrate(&app, &project);

        let path = dir
            .path()
            .join(project.id.to_string())
            .join(format!("narration-{}.mp3", narration.id));
        assert_eq!(app.play_narration(project.id).map(|_| ()).ok(), Some(()));
        assert_eq!(
            h.audio.opened.lock().unwrap().as_slice(),
            std::slice::from_ref(&path)
        );
        let files: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            files,
            [path.file_name().unwrap()],
            "only the narration stays"
        );
    }
}
