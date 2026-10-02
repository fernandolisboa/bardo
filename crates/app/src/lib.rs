//! Application state and use cases. The UI talks to Bardo only through this
//! crate, so behavior is tested here instead of through pixels (ADR-0001).

mod channels;
pub mod i18n;
mod jobs;
pub mod logging;
mod narrations;
mod personas;
mod provider_keys;
mod research;
mod scenes;
mod scripts;
mod templates;
mod themes;

use std::borrow::Cow;
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ChannelRepository, DecisionEngine, ImageGenerator, JobRepository, KeyChecker, MarketData,
    NarrationRepository, NicheResearchRepository, Persona, PersonaRepository, ProfileRepository,
    ProjectFiles, Redactor, RepositoryError, ScenePlanRepository, ScriptRepository, SecretStore,
    SpeechSynthesizer, TemplateRepository, TextGenerator, ThemeRepository, UiLanguage, UserProfile,
    VoiceLibrary,
};
use bardo_media::AudioOutput;
use bardo_storage::{Database, MemoryProjectFiles};

pub use bardo_domain;
pub use channels::ChannelError;
pub use i18n::{Catalog, Text};
pub use jobs::{JobActionError, JobContext, JobGroups, JobHandler, JobSettings, TestJob};
pub use narrations::{NarrationError, NarrationPlayer, NarrationView};
pub use personas::{PersonaError, VoiceList, VoiceListing, VoiceStatus};
pub use provider_keys::{KeyState, KeyTest, KeyTestResult, ProviderKeyError, ProviderKeyStatus};
pub use research::{NicheResearchView, NicheResult, NicheRow, ResearchError};
pub use scenes::{SceneError, ScenesView};
pub use scripts::{ScriptError, ScriptView};
pub use templates::{TemplateError, default_template};
pub use themes::{SUGGESTIONS_PER_RUN, ThemeError, ThemesView};

use crate::jobs::JobQueue;
use crate::narrations::NarrationHandler;
use crate::provider_keys::ProviderKeys;
use crate::research::NicheResearchHandler;
use crate::scenes::SceneHandler;
use crate::scripts::ScriptHandler;
use crate::themes::ThemeHandler;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

/// The storage ports the app runs on. Each slice that adds a repository adds
/// a field here; tests swap any of them for a fake.
pub struct Repositories {
    pub profiles: Box<dyn ProfileRepository>,
    pub channels: Box<dyn ChannelRepository>,
    /// Shared with the job queue's worker threads.
    pub jobs: Arc<dyn JobRepository>,
    /// Niche research results and seeds. Shared with the job queue.
    pub research: Arc<dyn NicheResearchRepository>,
    /// Themes and the video projects they start. Shared with the job queue.
    pub themes: Arc<dyn ThemeRepository>,
    /// Template versions. Shared with the job queue.
    pub templates: Arc<dyn TemplateRepository>,
    /// Scripts and their generations. Shared with the job queue.
    pub scripts: Arc<dyn ScriptRepository>,
    /// The persona library.
    pub personas: Arc<dyn PersonaRepository>,
    /// Narrations and their word timings. Shared with the job queue.
    pub narrations: Arc<dyn NarrationRepository>,
    /// Scene plans, their prompts and images. Shared with the job queue.
    pub scene_plans: Arc<dyn ScenePlanRepository>,
    /// Each video project's media folder. Shared with the job queue.
    pub files: Arc<dyn ProjectFiles>,
    /// Provider keys. Never the database (ADR-0001). Shared with jobs that
    /// call providers.
    pub secrets: Arc<dyn SecretStore>,
}

impl Repositories {
    /// Every data port served by one SQLite database, keys by `secrets`
    /// and media by `files`.
    pub fn local(
        db: Database,
        secrets: Box<dyn SecretStore>,
        files: Box<dyn ProjectFiles>,
    ) -> Self {
        Self::shared_with_files(Arc::new(db), Arc::from(secrets), Arc::from(files))
    }

    /// `local` over a database and secret store the caller keeps a handle
    /// to (tests read them directly), with project files in memory.
    pub fn shared(db: Arc<Database>, secrets: Arc<dyn SecretStore>) -> Self {
        Self::shared_with_files(db, secrets, Arc::new(MemoryProjectFiles::default()))
    }

    /// `shared` with the project files the caller chose.
    pub fn shared_with_files(
        db: Arc<Database>,
        secrets: Arc<dyn SecretStore>,
        files: Arc<dyn ProjectFiles>,
    ) -> Self {
        Self {
            profiles: Box::new(Arc::clone(&db)),
            channels: Box::new(Arc::clone(&db)),
            jobs: Arc::clone(&db) as Arc<dyn JobRepository>,
            themes: Arc::clone(&db) as _,
            templates: Arc::clone(&db) as _,
            scripts: Arc::clone(&db) as _,
            personas: Arc::clone(&db) as _,
            narrations: Arc::clone(&db) as _,
            scene_plans: Arc::clone(&db) as _,
            research: db,
            files,
            secrets,
        }
    }
}

/// The external services the app calls. Each slice that adds a provider
/// adapter adds a field here; tests swap any of them for a fake.
pub struct Providers {
    pub key_checker: Arc<dyn KeyChecker>,
    /// Niche research (YouTube Data API).
    pub market_data: Arc<dyn MarketData>,
    /// Generative text (Claude).
    pub text: Arc<dyn TextGenerator>,
    /// Typed decisions (JEV).
    pub decisions: Arc<dyn DecisionEngine>,
    /// The user's voices (ElevenLabs).
    pub voices: Arc<dyn VoiceLibrary>,
    /// Narration (ElevenLabs).
    pub speech: Arc<dyn SpeechSynthesizer>,
    /// Scene images (Nano Banana, through the Gemini API).
    pub images: Arc<dyn ImageGenerator>,
    /// The local audio device, for playback.
    pub audio: Arc<dyn AudioOutput>,
}

impl Providers {
    /// The real providers, over HTTPS.
    pub fn live() -> Self {
        Self {
            key_checker: Arc::new(bardo_ai::HttpKeyChecker::new()),
            market_data: Arc::new(bardo_ai::YouTubeMarketData::new()),
            text: Arc::new(bardo_ai::ClaudeTextGenerator::new()),
            decisions: Arc::new(bardo_ai::JevDecisionEngine::new()),
            voices: Arc::new(bardo_ai::ElevenLabsVoices::new()),
            speech: Arc::new(bardo_ai::ElevenLabsSpeech::new()),
            images: Arc::new(bardo_ai::GeminiImages::new()),
            audio: Arc::new(bardo_media::DeviceAudio),
        }
    }
}

/// The running app for the local user profile.
pub struct Bardo {
    profiles: Box<dyn ProfileRepository>,
    channels: Box<dyn ChannelRepository>,
    research: Arc<dyn NicheResearchRepository>,
    themes: Arc<dyn ThemeRepository>,
    templates: Arc<dyn TemplateRepository>,
    scripts: Arc<dyn ScriptRepository>,
    personas: Arc<dyn PersonaRepository>,
    narrations: Arc<dyn NarrationRepository>,
    scene_plans: Arc<dyn ScenePlanRepository>,
    files: Arc<dyn ProjectFiles>,
    audio: Arc<dyn AudioOutput>,
    market_data: Arc<dyn MarketData>,
    voices: Arc<dyn VoiceLibrary>,
    /// The last voice listing of this session, for the voice picker.
    voice_list: Option<VoiceList>,
    jobs: JobQueue,
    provider_keys: ProviderKeys,
    profile: UserProfile,
    catalog: Catalog,
}

impl Bardo {
    /// Loads the local profile, creating it on first start (no login), gives
    /// a profile without personas the default ones, and starts the job
    /// queue, resuming jobs the last session left running.
    /// `system_locale` (e.g. `pt-BR`) picks the language of a new profile.
    pub fn start(
        repositories: Repositories,
        providers: Providers,
        system_locale: Option<&str>,
    ) -> Result<Self, AppError> {
        Self::start_with(
            repositories,
            providers,
            system_locale,
            JobSettings::default(),
        )
    }

    /// `start` with explicit job queue settings.
    pub fn start_with(
        repositories: Repositories,
        providers: Providers,
        system_locale: Option<&str>,
        job_settings: JobSettings,
    ) -> Result<Self, AppError> {
        let Repositories {
            profiles,
            channels,
            jobs,
            research,
            themes,
            templates,
            scripts,
            personas,
            narrations,
            scene_plans,
            files,
            secrets,
        } = repositories;
        let profile = match profiles.load_default()? {
            Some(profile) => profile,
            None => {
                let profile = UserProfile::new(language_for_locale(system_locale));
                profiles.save(&profile)?;
                profile
            }
        };
        if personas.list(profile.id)?.is_empty() {
            personas.insert_all(&Persona::defaults(profile.id))?;
        }
        let catalog = Catalog::load(profile.ui_language);
        let redactor = Redactor::new();
        let research_handler = NicheResearchHandler {
            owner: profile.id,
            research: Arc::clone(&research),
            market_data: Arc::clone(&providers.market_data),
            secrets: Arc::clone(&secrets),
        };
        let theme_handler = ThemeHandler {
            owner: profile.id,
            themes: Arc::clone(&themes),
            text: Arc::clone(&providers.text),
            decisions: Arc::clone(&providers.decisions),
            secrets: Arc::clone(&secrets),
        };
        let script_handler = ScriptHandler {
            owner: profile.id,
            scripts: Arc::clone(&scripts),
            text: Arc::clone(&providers.text),
            secrets: Arc::clone(&secrets),
        };
        let narration_handler = NarrationHandler {
            owner: profile.id,
            narrations: Arc::clone(&narrations),
            files: Arc::clone(&files),
            speech: Arc::clone(&providers.speech),
            secrets: Arc::clone(&secrets),
        };
        let scene_handler = SceneHandler {
            owner: profile.id,
            plans: Arc::clone(&scene_plans),
            narrations: Arc::clone(&narrations),
            files: Arc::clone(&files),
            text: Arc::clone(&providers.text),
            images: Arc::clone(&providers.images),
            secrets: Arc::clone(&secrets),
        };
        let provider_keys =
            ProviderKeys::load(secrets, providers.key_checker, redactor.clone(), profile.id);
        let jobs = JobQueue::start(
            jobs,
            profile.id,
            crate::jobs::built_in_handlers(
                research_handler,
                theme_handler,
                script_handler,
                narration_handler,
                scene_handler,
            ),
            job_settings,
            redactor,
        )?;
        Ok(Self {
            profiles,
            channels,
            research,
            themes,
            templates,
            scripts,
            personas,
            narrations,
            scene_plans,
            files,
            audio: providers.audio,
            market_data: providers.market_data,
            voices: providers.voices,
            voice_list: None,
            jobs,
            provider_keys,
            profile,
            catalog,
        })
    }

    /// Masks every key the app knows. Hand it to the log (`logging::init`)
    /// and to anything else that writes text out of the app.
    pub fn redactor(&self) -> Redactor {
        self.provider_keys.redactor().clone()
    }

    pub fn profile(&self) -> &UserProfile {
        &self.profile
    }

    pub fn ui_language(&self) -> UiLanguage {
        self.profile.ui_language
    }

    /// Switches the interface language and remembers it. On failure the
    /// current language stays.
    pub fn set_ui_language(&mut self, language: UiLanguage) -> Result<(), AppError> {
        if language == self.profile.ui_language {
            return Ok(());
        }
        let updated = UserProfile {
            ui_language: language,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        self.catalog = Catalog::load(language);
        Ok(())
    }

    pub fn text(&self, text: Text) -> Cow<'_, str> {
        self.catalog.get(text)
    }

    /// `text` with its `{name}` placeholders filled in.
    pub fn text_with(&self, text: Text, args: &[(&str, &str)]) -> String {
        self.catalog.format(text, args)
    }

    /// A count in the interface language's short form (`48K`, `48 mil`).
    pub fn compact_count(&self, n: u64) -> String {
        self.catalog.compact(n)
    }

    /// How long ago `at` was, e.g. `3 h ago`. A time in the future reads
    /// as just now.
    pub fn time_ago(&self, at: SystemTime) -> String {
        let elapsed = SystemTime::now().duration_since(at).unwrap_or_default();
        self.catalog.age(elapsed)
    }
}

/// Portuguese system locales (`pt-BR`, `pt_BR`, `pt`) start in pt-BR;
/// everything else starts in en-US.
fn language_for_locale(locale: Option<&str>) -> UiLanguage {
    match locale {
        Some(tag) if tag.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("pt")) => UiLanguage::PtBr,
        _ => UiLanguage::EnUs,
    }
}

/// Test doubles shared by the use case tests.
#[cfg(test)]
pub(crate) mod testing {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        Alignment, Answer, ApiKey, CharTiming, Confidence, DecisionEngine, Decisions,
        GeneratedImage, GeneratedText, ImageFormat, ImageGenerator, ImageRequest, KeyCheck,
        KeyCheckOutcome, KeyChecker, Market, MarketData, MarketSample, Niche, Provider,
        ProviderFailure, Question, Questions, ScoreAnswer, Speech, SpeechRequest,
        SpeechSynthesizer, TextGenerator, TextRequest, TokenUsage, UploadSample, Voice,
        VoiceLibrary,
    };

    use crate::Providers;

    /// Answers every check with `answer`, and remembers which keys it was
    /// asked about.
    #[derive(Default)]
    pub(crate) struct FakeKeyChecker {
        pub(crate) answer: Mutex<Option<KeyCheck>>,
        pub(crate) asked: Mutex<Vec<(Provider, String)>>,
    }

    impl KeyChecker for FakeKeyChecker {
        fn check(&self, provider: Provider, key: &ApiKey) -> KeyCheck {
            self.asked
                .lock()
                .unwrap()
                .push((provider, key.expose().to_owned()));
            self.answer
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(KeyCheck::new(KeyCheckOutcome::Valid, None))
        }
    }

    /// One market data call, as the fake saw it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct MarketCall {
        pub(crate) niche: String,
        pub(crate) market: Market,
        pub(crate) key: String,
        pub(crate) since: SystemTime,
    }

    /// Counts every call. Answers with the sample set for the niche's
    /// label, a default one-upload sample otherwise, or `failure` when set.
    #[derive(Default)]
    pub(crate) struct FakeMarketData {
        pub(crate) calls: Mutex<Vec<MarketCall>>,
        pub(crate) samples: Mutex<HashMap<String, MarketSample>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        /// How long each call takes, to catch a job mid-run.
        pub(crate) delay: Mutex<Duration>,
    }

    impl FakeMarketData {
        pub(crate) fn calls(&self) -> Vec<MarketCall> {
            self.calls.lock().unwrap().clone()
        }

        /// A sample whose uploads all have `views`, uploaded a day ago.
        pub(crate) fn set_views(&self, label: &str, views: u64) {
            let sample = MarketSample {
                upload_volume: 500,
                uploads: vec![UploadSample {
                    channel_id: "UCfake".into(),
                    published_at: SystemTime::now() - Duration::from_secs(86_400),
                    views,
                    channel_subscribers: Some(5_000),
                }],
            };
            self.samples
                .lock()
                .unwrap()
                .insert(label.to_owned(), sample);
        }
    }

    impl MarketData for FakeMarketData {
        fn recent_uploads(
            &self,
            key: &ApiKey,
            niche: &Niche,
            market: Market,
            since: SystemTime,
        ) -> Result<MarketSample, ProviderFailure> {
            self.calls.lock().unwrap().push(MarketCall {
                niche: niche.label().to_owned(),
                market,
                key: key.expose().to_owned(),
                since,
            });
            let delay = *self.delay.lock().unwrap();
            std::thread::sleep(delay);
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            if let Some(sample) = self.samples.lock().unwrap().get(niche.label()) {
                return Ok(sample.clone());
            }
            Ok(MarketSample {
                upload_volume: 100,
                uploads: vec![UploadSample {
                    channel_id: "UCfake".into(),
                    published_at: SystemTime::now() - Duration::from_secs(86_400),
                    views: 1_000,
                    channel_subscribers: Some(5_000),
                }],
            })
        }

        fn quota_units_per_niche(&self) -> u32 {
            102
        }
    }

    /// Answers each request with the next queued text, or with ideas named
    /// "Idea 1".."Idea 10" once the queue is empty; `failure` wins when set.
    #[derive(Default)]
    pub(crate) struct FakeTextGenerator {
        pub(crate) requests: Mutex<Vec<TextRequest>>,
        pub(crate) answers: Mutex<Vec<String>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        /// Reported with every answer.
        pub(crate) usage: Mutex<TokenUsage>,
        /// How long each call takes, to catch a job mid-run.
        pub(crate) delay: Mutex<Duration>,
    }

    impl FakeTextGenerator {
        pub(crate) fn requests(&self) -> Vec<TextRequest> {
            self.requests.lock().unwrap().clone()
        }

        /// Queues an answer proposing these titles, each with an angle.
        pub(crate) fn answer_with(&self, titles: &[&str]) {
            self.answers.lock().unwrap().push(ideas_json(titles));
        }
    }

    pub(crate) fn ideas_json(titles: &[&str]) -> String {
        let themes: Vec<_> = titles
            .iter()
            .map(|title| serde_json::json!({"title": title, "angle": format!("Why {title} matters.")}))
            .collect();
        serde_json::json!({ "themes": themes }).to_string()
    }

    impl TextGenerator for FakeTextGenerator {
        fn generate(
            &self,
            _key: &ApiKey,
            request: &TextRequest,
        ) -> Result<GeneratedText, ProviderFailure> {
            self.requests.lock().unwrap().push(request.clone());
            let delay = *self.delay.lock().unwrap();
            std::thread::sleep(delay);
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            let mut answers = self.answers.lock().unwrap();
            let text = if answers.is_empty() {
                let titles: Vec<String> = (1..=10).map(|n| format!("Idea {n}")).collect();
                ideas_json(&titles.iter().map(String::as_str).collect::<Vec<_>>())
            } else {
                answers.remove(0)
            };
            Ok(GeneratedText {
                text,
                model: "claude-fake".into(),
                usage: *self.usage.lock().unwrap(),
            })
        }
    }

    /// Answers every score question from what its instructions mention:
    /// the level set in `levels` for the first matching needle, the middle
    /// level otherwise, always with `confidence`.
    pub(crate) struct FakeDecisionEngine {
        pub(crate) calls: Mutex<Vec<(String, Questions)>>,
        pub(crate) levels: Mutex<Vec<(String, f64)>>,
        pub(crate) confidence: Mutex<f64>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        /// How long each call takes, to catch a job mid-run.
        pub(crate) delay: Mutex<Duration>,
    }

    impl Default for FakeDecisionEngine {
        fn default() -> Self {
            Self {
                calls: Mutex::default(),
                levels: Mutex::default(),
                confidence: Mutex::new(0.8),
                failure: Mutex::default(),
                delay: Mutex::default(),
            }
        }
    }

    impl FakeDecisionEngine {
        pub(crate) fn calls(&self) -> Vec<(String, Questions)> {
            self.calls.lock().unwrap().clone()
        }

        /// Questions whose instructions contain `needle` get `level`.
        pub(crate) fn set_level(&self, needle: &str, level: f64) {
            self.levels.lock().unwrap().push((needle.to_owned(), level));
        }
    }

    impl DecisionEngine for FakeDecisionEngine {
        fn decide(
            &self,
            _key: &ApiKey,
            state: &str,
            questions: &Questions,
        ) -> Result<Decisions, ProviderFailure> {
            self.calls
                .lock()
                .unwrap()
                .push((state.to_owned(), questions.clone()));
            let delay = *self.delay.lock().unwrap();
            std::thread::sleep(delay);
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            let levels = self.levels.lock().unwrap();
            let confidence = Confidence::new(*self.confidence.lock().unwrap());
            let mut answers = HashMap::new();
            for (id, question) in questions.iter() {
                let Question::Score {
                    instructions,
                    levels: names,
                } = question
                else {
                    panic!("the fake only answers score questions");
                };
                let level = levels
                    .iter()
                    .find(|(needle, _)| instructions.contains(needle.as_str()))
                    .map_or((names.len() - 1) as f64 / 2.0, |(_, level)| *level);
                answers.insert(
                    id.to_owned(),
                    Answer::Score(ScoreAnswer {
                        level,
                        probabilities: vec![0.0; names.len()],
                        confidence,
                    }),
                );
            }
            Ok(Decisions {
                answers,
                model: "jev-fake".into(),
            })
        }
    }

    /// Answers every listing with `voices`, or `failure` when set; counts
    /// the calls.
    #[derive(Default)]
    pub(crate) struct FakeVoiceLibrary {
        pub(crate) voices: Mutex<Vec<Voice>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        pub(crate) calls: Mutex<Vec<String>>,
    }

    impl VoiceLibrary for FakeVoiceLibrary {
        fn voices(&self, key: &ApiKey) -> Result<Vec<Voice>, ProviderFailure> {
            self.calls.lock().unwrap().push(key.expose().to_owned());
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            Ok(self.voices.lock().unwrap().clone())
        }
    }

    /// One second of MP3 audio, the length of every fake part.
    pub(crate) const PART_AUDIO: &[u8] =
        include_bytes!("../../media/tests/fixtures/tone-1s-raw.mp3");

    /// Reads every request as one second of tone, its characters timed
    /// evenly across that second; `failure` wins when set. Keeps the
    /// requests.
    pub(crate) struct FakeSpeech {
        pub(crate) requests: Mutex<Vec<SpeechRequest>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        pub(crate) max_chars: Mutex<usize>,
        /// Fails this request (counting from 0) once, as a provider outage.
        pub(crate) fail_at: Mutex<Option<usize>>,
        /// How long each call takes, to catch a job mid-run.
        pub(crate) delay: Mutex<Duration>,
    }

    impl Default for FakeSpeech {
        fn default() -> Self {
            Self {
                requests: Mutex::default(),
                failure: Mutex::default(),
                max_chars: Mutex::new(5_000),
                fail_at: Mutex::default(),
                delay: Mutex::default(),
            }
        }
    }

    impl FakeSpeech {
        pub(crate) fn requests(&self) -> Vec<SpeechRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl SpeechSynthesizer for FakeSpeech {
        fn max_chars(&self) -> usize {
            *self.max_chars.lock().unwrap()
        }

        fn synthesize(
            &self,
            _key: &ApiKey,
            request: &SpeechRequest,
        ) -> Result<Speech, ProviderFailure> {
            let delay = *self.delay.lock().unwrap();
            std::thread::sleep(delay);
            let mut requests = self.requests.lock().unwrap();
            requests.push(request.clone());
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            let mut fail_at = self.fail_at.lock().unwrap();
            if *fail_at == Some(requests.len() - 1) {
                *fail_at = None;
                return Err(ProviderFailure::new(
                    bardo_domain::ProviderFailureKind::ProviderDown,
                    "busy",
                ));
            }
            let chars: Vec<char> = request.text.chars().collect();
            let step = Duration::from_secs(1) / chars.len().max(1) as u32;
            Ok(Speech {
                audio: PART_AUDIO.to_vec(),
                alignment: Alignment {
                    chars: chars
                        .iter()
                        .enumerate()
                        .map(|(n, c)| CharTiming {
                            text: c.to_string(),
                            start: step * n as u32,
                            end: step * (n as u32 + 1),
                        })
                        .collect(),
                },
                model: "eleven-fake".into(),
                billed_characters: chars.len() as u64,
            })
        }
    }

    /// A tiny PNG, the image of every fake drawing.
    pub(crate) const SCENE_IMAGE: &[u8] =
        include_bytes!("../../ai/tests/fixtures/gemini/scene-16x9.png");

    /// Draws every prompt as `SCENE_IMAGE`; a prompt containing a word in
    /// `declined` is declined, and `failure` wins when set. Keeps the
    /// requests.
    #[derive(Default)]
    pub(crate) struct FakeImages {
        pub(crate) requests: Mutex<Vec<ImageRequest>>,
        pub(crate) declined: Mutex<Vec<String>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        /// How long each call takes, to catch a job mid-run.
        pub(crate) delay: Mutex<Duration>,
    }

    impl FakeImages {
        pub(crate) fn prompts(&self) -> Vec<String> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.prompt.clone())
                .collect()
        }

        /// Declines every prompt containing `word` from now on.
        pub(crate) fn decline(&self, word: &str) {
            self.declined.lock().unwrap().push(word.to_owned());
        }

        /// Draws everything again.
        pub(crate) fn accept_all(&self) {
            self.declined.lock().unwrap().clear();
        }
    }

    impl ImageGenerator for FakeImages {
        fn generate(
            &self,
            _key: &ApiKey,
            request: &ImageRequest,
        ) -> Result<GeneratedImage, ProviderFailure> {
            let delay = *self.delay.lock().unwrap();
            std::thread::sleep(delay);
            self.requests.lock().unwrap().push(request.clone());
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            if let Some(word) = self
                .declined
                .lock()
                .unwrap()
                .iter()
                .find(|word| request.prompt.contains(word.as_str()))
            {
                return Err(ProviderFailure::new(
                    bardo_domain::ProviderFailureKind::Declined,
                    format!("blocked: {word}"),
                ));
            }
            Ok(GeneratedImage {
                bytes: SCENE_IMAGE.to_vec(),
                format: ImageFormat::Png,
                model: "nano-banana-fake".into(),
                usage: TokenUsage {
                    input_tokens: 12,
                    output_tokens: 1_290,
                },
            })
        }
    }

    /// Providers that never touch the network.
    pub(crate) fn providers() -> Providers {
        providers_with(Arc::new(FakeMarketData::default()))
    }

    pub(crate) fn providers_with(market_data: Arc<FakeMarketData>) -> Providers {
        Providers {
            key_checker: Arc::new(FakeKeyChecker::default()),
            market_data,
            text: Arc::new(FakeTextGenerator::default()),
            decisions: Arc::new(FakeDecisionEngine::default()),
            voices: Arc::new(FakeVoiceLibrary::default()),
            speech: Arc::new(FakeSpeech::default()),
            images: Arc::new(FakeImages::default()),
            audio: Arc::new(crate::narrations::testing::FakeAudioOutput::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use bardo_storage::MemorySecretStore;

    use super::*;

    /// In-memory repository that shares its state with the test, so a second
    /// `Bardo::start` sees what the first one saved (a restart).
    #[derive(Clone, Default)]
    struct FakeProfiles {
        saved: Rc<RefCell<Option<UserProfile>>>,
        fail_saves: Rc<Cell<bool>>,
    }

    impl ProfileRepository for FakeProfiles {
        fn load_default(&self) -> Result<Option<UserProfile>, RepositoryError> {
            Ok(self.saved.borrow().clone())
        }

        fn save(&self, profile: &UserProfile) -> Result<(), RepositoryError> {
            if self.fail_saves.get() {
                return Err(RepositoryError("disk full".into()));
            }
            *self.saved.borrow_mut() = Some(profile.clone());
            Ok(())
        }
    }

    /// Personas kept in memory: the profile these tests start with lives
    /// in `FakeProfiles`, not in the database personas would reference.
    #[derive(Default)]
    struct FakePersonas(std::sync::Mutex<Vec<Persona>>);

    impl PersonaRepository for FakePersonas {
        fn list(&self, owner: bardo_domain::ProfileId) -> Result<Vec<Persona>, RepositoryError> {
            let saved = self.0.lock().unwrap();
            Ok(saved.iter().filter(|p| p.owner == owner).cloned().collect())
        }

        fn get(&self, id: bardo_domain::PersonaId) -> Result<Option<Persona>, RepositoryError> {
            Ok(self.0.lock().unwrap().iter().find(|p| p.id == id).cloned())
        }

        fn save(&self, persona: &Persona) -> Result<(), RepositoryError> {
            self.insert_all(std::slice::from_ref(persona))
        }

        fn insert_all(&self, personas: &[Persona]) -> Result<(), RepositoryError> {
            let mut saved = self.0.lock().unwrap();
            for persona in personas {
                saved.retain(|p| p.id != persona.id);
                saved.push(persona.clone());
            }
            Ok(())
        }
    }

    fn start(profiles: &FakeProfiles, locale: Option<&str>) -> Bardo {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let repositories = Repositories {
            profiles: Box::new(profiles.clone()),
            channels: Box::new(Arc::clone(&db)),
            jobs: Arc::clone(&db) as _,
            themes: Arc::clone(&db) as _,
            templates: Arc::clone(&db) as _,
            scripts: Arc::clone(&db) as _,
            personas: Arc::new(FakePersonas::default()),
            narrations: Arc::clone(&db) as _,
            scene_plans: Arc::clone(&db) as _,
            files: Arc::new(MemoryProjectFiles::default()),
            research: db,
            secrets: Arc::new(MemorySecretStore::default()),
        };
        Bardo::start(repositories, testing::providers(), locale).unwrap()
    }

    #[test]
    fn first_start_creates_and_saves_a_profile() {
        let profiles = FakeProfiles::default();
        let app = start(&profiles, None);
        assert_eq!(profiles.saved.borrow().as_ref(), Some(app.profile()));
    }

    #[test]
    fn later_starts_reuse_the_saved_profile() {
        let profiles = FakeProfiles::default();
        let first = start(&profiles, None).profile().id;
        let second = start(&profiles, Some("pt-BR")).profile().id;
        assert_eq!(first, second);
    }

    #[test]
    fn first_start_follows_a_portuguese_system_locale() {
        for locale in ["pt-BR", "pt_BR", "pt", "PT-pt"] {
            let app = start(&FakeProfiles::default(), Some(locale));
            assert_eq!(app.ui_language(), UiLanguage::PtBr, "{locale}");
        }
    }

    #[test]
    fn first_start_defaults_to_english() {
        for locale in [None, Some("en-GB"), Some("fr-FR"), Some("")] {
            let app = start(&FakeProfiles::default(), locale);
            assert_eq!(app.ui_language(), UiLanguage::EnUs, "{locale:?}");
        }
    }

    #[test]
    fn switching_language_changes_every_string_without_restart() {
        let mut app = start(&FakeProfiles::default(), Some("en-US"));
        let english = app.text(Text::AppTagline).into_owned();

        app.set_ui_language(UiLanguage::PtBr).unwrap();

        assert_eq!(app.ui_language(), UiLanguage::PtBr);
        assert_eq!(
            app.text(Text::AppTagline),
            Catalog::load(UiLanguage::PtBr).get(Text::AppTagline)
        );
        assert_ne!(app.text(Text::AppTagline), english);
    }

    #[test]
    fn language_choice_survives_a_restart() {
        let profiles = FakeProfiles::default();
        start(&profiles, Some("en-US"))
            .set_ui_language(UiLanguage::PtBr)
            .unwrap();
        assert_eq!(
            start(&profiles, Some("en-US")).ui_language(),
            UiLanguage::PtBr
        );
    }

    #[test]
    fn failed_save_keeps_the_current_language() {
        let profiles = FakeProfiles::default();
        let mut app = start(&profiles, Some("en-US"));
        profiles.fail_saves.set(true);

        assert!(app.set_ui_language(UiLanguage::PtBr).is_err());
        assert_eq!(app.ui_language(), UiLanguage::EnUs);
        assert_eq!(
            app.text(Text::AppTagline),
            Catalog::load(UiLanguage::EnUs).get(Text::AppTagline)
        );
    }

    #[test]
    fn works_against_real_sqlite() {
        let db = Database::open_in_memory().unwrap();
        let repositories = Repositories::local(
            db,
            Box::new(MemorySecretStore::default()),
            Box::new(MemoryProjectFiles::default()),
        );
        let mut app = Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap();
        app.set_ui_language(UiLanguage::PtBr).unwrap();
        assert_eq!(app.ui_language(), UiLanguage::PtBr);
    }
}
