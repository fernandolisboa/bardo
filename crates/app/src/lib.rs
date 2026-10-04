//! Application state and use cases. The UI talks to Bardo only through this
//! crate, so behavior is tested here instead of through pixels (ADR-0001).

mod appearance;
mod channels;
mod clips;
mod connections;
mod costs;
mod cut_suggestions;
mod editor;
mod export;
pub mod i18n;
mod jobs;
pub mod logging;
mod media_import;
mod music_prompts;
mod narration_import;
mod narrations;
mod navigation;
mod network_accounts;
mod persona_package;
mod personas;
mod provider_keys;
mod proxies;
mod publications;
mod render;
mod research;
mod scenes;
mod scheduler;
mod schedules;
mod scripts;
mod selection;
mod shortcuts;
mod stages;
mod templates;
mod themes;
mod tours;
mod uploads;
mod voice_samples;

use std::borrow::Cow;
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    ChannelRepository, ClipGenerator, ConnectionSecrets, ConsentReceiver, CostRepository,
    CutSuggestionRepository, DecisionEngine, ExportFiles, ExportRepository, ImageGenerator,
    InsightsReader, JobRepository, KeyChecker, LayoutId, MarketData, MediaAssetRepository,
    MusicPromptRepository, NarrationRepository, NetworkAccountRepository,
    NetworkConnectionRepository, NetworkSignIn, NicheResearchRepository, OwnerAnalytics, Persona,
    PersonaRepository, ProfileRepository, ProjectFiles, PublicationRepository, Redactor,
    RenderRepository, RepositoryError, ScenePlanRepository, ScriptRepository, SecretStore,
    SpeechAligner, SpeechSynthesizer, TemplateRepository, TextGenerator, ThemeRepository,
    TimelineRepository, TourProgressRepository, UiLanguage, UiThemePreference, UserProfile,
    VideoStats, VideoUploader, VoiceLibrary, VoicePreviews, VoiceSampleStore, Zone,
};
use bardo_media::{AudioOutput, MediaEngine};
use bardo_storage::{
    Database, MemoryExportFiles, MemoryProjectFiles, MemorySecretStore, MemoryVoiceSamples,
};

pub use appearance::{EditorPalette, Palette, Rgb, Rgba, TrackColors, UiFont, palette};
pub use bardo_domain;
/// How each caption style looks, for the editor's style swatches.
pub use bardo_media::ffmpeg::{CaptionLook, caption_look};
pub use channels::ChannelError;
pub use clips::{ClipsView, SceneClipView};
pub use connections::{
    AccountChoice, AppCredentialsStatus, CONSENT_TIMEOUT, ConnectAttempt, ConnectResult,
    ConnectionCheck, ConnectionError, ConnectionRenewal, ConnectionState, Disconnected,
    Disconnection, TokenConnect, TokenConnectResult, TokenConnected,
};
pub use costs::{
    BudgetConsent, CostError, CostsView, ProviderEstimate, ProviderSpend, RateRow, SpendEstimate,
    SpendRow, SpendSummary,
};
pub use cut_suggestions::{
    CHUNK_LIMITS, CutSuggestionError, SuggestionState, SuggestionView, SuggestionsView,
};
pub use editor::{
    BinScene, ClipMedia, ClipProblem, ClipShows, ClipView, CutBasis, EditAction, Editor,
    EditorError, EditorView, NarrationTrack, PREVIEW_LANDSCAPE, PREVIEW_PORTRAIT, WordMark,
    media_framing, preview_size,
};
pub use export::{
    ExportBlock, ExportError, ExportSummary, ExportTarget, ExportView, export_job_networks,
};
pub use i18n::{Catalog, Text};
pub use jobs::{JobActionError, JobContext, JobGroups, JobHandler, JobSettings, TestJob};
pub use media_import::{MediaImport, MediaImportError};
pub use music_prompts::{MusicPromptError, MusicPromptView};
pub use narration_import::{MAX_RECORDING_BYTES, Recording};
pub use narrations::{NarrationError, NarrationPlayer, NarrationView};
pub use navigation::{Destination, Pillar};
pub use network_accounts::NetworkAccountError;
pub use persona_package::PackageError;
pub use personas::{PersonaError, VoiceList, VoiceListing, VoiceStatus, persona_package_folder};
pub use provider_keys::{KeyState, KeyTest, KeyTestResult, ProviderKeyError, ProviderKeyStatus};
pub use publications::{
    ChannelMetricsView, ChannelPost, MetricsError, MetricsStatus, OwnerAccess, PublicationError,
    PublishedPost,
};
pub use render::{
    CheckFound, RenderChecks, RenderError, RenderReview, RenderSummary, RenderTarget,
    render_job_files,
};
pub use research::{NicheResearchView, NicheResult, NicheRow, ResearchError};
pub use scenes::{SceneError, ScenesView};
pub use scheduler::{MissedPost, MissedPostError};
pub use schedules::{ScheduleError, ScheduleResult, ScheduleUpdate};
pub use scripts::{ScriptError, ScriptView};
pub use selection::{Step, step_selection};
pub use shortcuts::{SHORTCUTS, Shortcut, ShortcutGroup};
pub use stages::{
    SceneState, Stage, StageNote, StageState, StageStatus, opening_stage, project_stages,
    scene_states,
};
pub use templates::{TemplateError, default_template};
pub use themes::{SUGGESTIONS_PER_RUN, ThemeError, ThemesView};
pub use tours::{
    Side, Spot, Tour, TourAnchor, TourError, TourMove, TourPlace, TourStep, TourStepView, WELCOME,
    WhenMissing,
};
pub use uploads::{
    DraftNote, SpecProblem, UploadBlock, UploadChoices, UploadReview, UploadReviewError,
    UploadState, upload_state,
};
pub use voice_samples::{SampleAudio, SamplePlayer, VoiceSample, VoiceSampleError, VoiceSampling};

use crate::clips::ClipHandler;
use crate::connections::ConnectionBook;
use crate::costs::CostBook;
use crate::cut_suggestions::CutSuggestionHandler;
use crate::export::{ExportHandler, MetadataHandler};
use crate::jobs::JobQueue;
use crate::music_prompts::MusicPromptHandler;
use crate::narration_import::NarrationImportHandler;
use crate::narrations::NarrationHandler;
use crate::provider_keys::ProviderKeys;
use crate::proxies::ProxyHandler;
use crate::publications::MetricsSyncHandler;
use crate::render::RenderHandler;
use crate::research::NicheResearchHandler;
use crate::scenes::SceneHandler;
use crate::scripts::ScriptHandler;
use crate::themes::ThemeHandler;
use crate::tours::TourBook;

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
    /// The cuts made in the editor.
    pub timelines: Arc<dyn TimelineRepository>,
    /// Each project's imported media.
    pub media_assets: Arc<dyn MediaAssetRepository>,
    /// Each project's music prompt. Shared with the job queue.
    pub music_prompts: Arc<dyn MusicPromptRepository>,
    /// Each channel's network accounts.
    pub network_accounts: Arc<dyn NetworkAccountRepository>,
    /// Each network account's connection state. Never its tokens.
    pub connections: Arc<dyn NetworkConnectionRepository>,
    /// Each project's rendered files. Shared with the job queue.
    pub renders: Arc<dyn RenderRepository>,
    /// Each project's metadata and exports. Shared with the job queue.
    pub exports: Arc<dyn ExportRepository>,
    /// Where export packages are written. Shared with the job queue.
    pub export_files: Arc<dyn ExportFiles>,
    /// Posts made by hand and their metrics. Shared with the job queue.
    pub publications: Arc<dyn PublicationRepository>,
    /// Each project's AI cut suggestions. Shared with the job queue.
    pub cut_suggestions: Arc<dyn CutSuggestionRepository>,
    /// What generations cost, the user's rates and budgets. Shared with
    /// the job queue.
    pub costs: Arc<dyn CostRepository>,
    /// Each video project's media folder. Shared with the job queue.
    pub files: Arc<dyn ProjectFiles>,
    /// Voice samples heard on the Personas screen, kept on this machine.
    pub voice_samples: Arc<dyn VoiceSampleStore>,
    /// Provider keys. Never the database (ADR-0001). Shared with jobs that
    /// call providers.
    pub secrets: Arc<dyn SecretStore>,
    /// Network app credentials and OAuth tokens. Never the database
    /// (ADR-0008).
    pub connection_secrets: Arc<dyn ConnectionSecrets>,
    /// How far the profile got in each guided tour.
    pub tours: Box<dyn TourProgressRepository>,
}

impl Repositories {
    /// Every data port served by one SQLite database, keys by `secrets`,
    /// app credentials and tokens by `connection_secrets`, media by `files`,
    /// export packages by `export_files` and voice samples by
    /// `voice_samples`.
    pub fn local(
        db: Database,
        secrets: Box<dyn SecretStore>,
        connection_secrets: Box<dyn ConnectionSecrets>,
        files: Box<dyn ProjectFiles>,
        export_files: Box<dyn ExportFiles>,
        voice_samples: Box<dyn VoiceSampleStore>,
    ) -> Self {
        Self {
            export_files: Arc::from(export_files),
            voice_samples: Arc::from(voice_samples),
            connection_secrets: Arc::from(connection_secrets),
            ..Self::shared_with_files(Arc::new(db), Arc::from(secrets), Arc::from(files))
        }
    }

    /// `local` over a database and secret store the caller keeps a handle
    /// to (tests read them directly), with project files in memory.
    pub fn shared(db: Arc<Database>, secrets: Arc<dyn SecretStore>) -> Self {
        Self::shared_with_files(db, secrets, Arc::new(MemoryProjectFiles::default()))
    }

    /// `shared` with the project files the caller chose, and export
    /// packages, voice samples, app credentials and tokens in memory.
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
            timelines: Arc::clone(&db) as _,
            media_assets: Arc::clone(&db) as _,
            music_prompts: Arc::clone(&db) as _,
            network_accounts: Arc::clone(&db) as _,
            connections: Arc::clone(&db) as _,
            renders: Arc::clone(&db) as _,
            exports: Arc::clone(&db) as _,
            export_files: Arc::new(MemoryExportFiles::default()),
            publications: Arc::clone(&db) as _,
            cut_suggestions: Arc::clone(&db) as _,
            costs: Arc::clone(&db) as _,
            tours: Box::new(Arc::clone(&db)),
            research: db,
            files,
            voice_samples: Arc::new(MemoryVoiceSamples::default()),
            secrets,
            connection_secrets: Arc::new(MemorySecretStore::default()),
        }
    }
}

/// The external services the app calls. Each slice that adds a provider
/// adapter adds a field here; tests swap any of them for a fake.
pub struct Providers {
    pub key_checker: Arc<dyn KeyChecker>,
    /// Niche research (YouTube Data API).
    pub market_data: Arc<dyn MarketData>,
    /// Public statistics of the user's posts (YouTube Data API).
    pub video_stats: Arc<dyn VideoStats>,
    /// Generative text (Claude).
    pub text: Arc<dyn TextGenerator>,
    /// Typed decisions (JEV).
    pub decisions: Arc<dyn DecisionEngine>,
    /// The user's voices (ElevenLabs).
    pub voices: Arc<dyn VoiceLibrary>,
    /// Narration and voice samples (ElevenLabs).
    pub speech: Arc<dyn SpeechSynthesizer>,
    /// The voices' stock previews (ElevenLabs).
    pub previews: Arc<dyn VoicePreviews>,
    /// Word timings of narration the user recorded (ElevenLabs forced
    /// alignment).
    pub aligner: Arc<dyn SpeechAligner>,
    /// Scene images (Nano Banana, through the Gemini API).
    pub images: Arc<dyn ImageGenerator>,
    /// Scene clips, one adapter per video provider (Higgsfield, then Google
    /// through the Gemini API).
    pub clips: Vec<Arc<dyn ClipGenerator>>,
    /// The local audio device, for playback.
    pub audio: Arc<dyn AudioOutput>,
    /// The bundled ffmpeg: proxies, waveforms and the editor's preview.
    pub media: Arc<dyn MediaEngine>,
    /// Sign-in per network that has one (ADR-0008): YouTube for now.
    pub sign_ins: Vec<Arc<dyn NetworkSignIn>>,
    /// Where the browser comes back after consent: a loopback listener.
    pub consent: Arc<dyn ConsentReceiver>,
    /// Upload per network that has one (ADR-0008): YouTube and Instagram
    /// Reels for now.
    pub uploaders: Vec<Arc<dyn VideoUploader>>,
    /// Owner metrics per network that has them (#79): YouTube Analytics
    /// for now.
    pub analytics: Vec<Arc<dyn OwnerAnalytics>>,
    /// Posts' numbers read through the connected account, per network
    /// that has them (#85): Instagram insights and TikTok's video query.
    pub post_insights: Vec<Arc<dyn InsightsReader>>,
}

impl Providers {
    /// The real providers, over HTTPS.
    pub fn live() -> Self {
        Self {
            key_checker: Arc::new(bardo_ai::HttpKeyChecker::new()),
            market_data: Arc::new(bardo_ai::YouTubeMarketData::new()),
            video_stats: Arc::new(bardo_ai::YouTubeStats::new()),
            text: Arc::new(bardo_ai::ClaudeTextGenerator::new()),
            decisions: Arc::new(bardo_ai::JevDecisionEngine::new()),
            voices: Arc::new(bardo_ai::ElevenLabsVoices::new()),
            speech: Arc::new(bardo_ai::ElevenLabsSpeech::new()),
            previews: Arc::new(bardo_ai::ElevenLabsPreviews::new()),
            aligner: Arc::new(bardo_ai::ElevenLabsAlignment::new()),
            images: Arc::new(bardo_ai::GeminiImages::new()),
            clips: vec![
                Arc::new(bardo_ai::HiggsfieldClips::new()),
                Arc::new(bardo_ai::GoogleClips::new()),
            ],
            audio: Arc::new(bardo_media::DeviceAudio),
            media: Arc::new(bardo_media::BundledFfmpeg::new()),
            sign_ins: vec![
                Arc::new(bardo_publish::YouTubeSignIn::new()),
                Arc::new(bardo_publish::TikTokSignIn::new()),
                Arc::new(bardo_publish::InstagramSignIn::new()),
            ],
            consent: Arc::new(bardo_publish::LoopbackReceiver),
            uploaders: vec![
                Arc::new(bardo_publish::YouTubeUploader::new()),
                Arc::new(bardo_publish::InstagramUploader::new()),
                Arc::new(bardo_publish::TikTokUploader::new()),
            ],
            analytics: vec![Arc::new(bardo_publish::YouTubeAnalytics::new())],
            post_insights: vec![
                Arc::new(bardo_publish::InstagramInsights::new()),
                Arc::new(bardo_publish::TikTokInsights::new()),
            ],
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
    timelines: Arc<dyn TimelineRepository>,
    media_assets: Arc<dyn MediaAssetRepository>,
    music_prompts: Arc<dyn MusicPromptRepository>,
    network_accounts: Arc<dyn NetworkAccountRepository>,
    renders: Arc<dyn RenderRepository>,
    exports: Arc<dyn ExportRepository>,
    export_files: Arc<dyn ExportFiles>,
    publications: Arc<dyn PublicationRepository>,
    cut_suggestions: Arc<dyn CutSuggestionRepository>,
    cost_book: CostBook,
    files: Arc<dyn ProjectFiles>,
    voice_samples: Arc<dyn VoiceSampleStore>,
    audio: Arc<dyn AudioOutput>,
    speech: Arc<dyn SpeechSynthesizer>,
    previews: Arc<dyn VoicePreviews>,
    samples_in_flight: Arc<voice_samples::SamplesInFlight>,
    media: Arc<dyn MediaEngine>,
    market_data: Arc<dyn MarketData>,
    voices: Arc<dyn VoiceLibrary>,
    clips: Vec<Arc<dyn ClipGenerator>>,
    uploaders: Vec<Arc<dyn VideoUploader>>,
    /// What the network's specs (a Reel's, TikTok's) found in each
    /// render, by render and size.
    spec_checks: std::sync::Mutex<
        std::collections::HashMap<(bardo_domain::RenderId, u64), Vec<uploads::SpecProblem>>,
    >,
    /// The last voice listing of this session, for the voice picker.
    voice_list: Option<VoiceList>,
    jobs: JobQueue,
    provider_keys: ProviderKeys,
    connection_book: ConnectionBook,
    tours: TourBook,
    profile: UserProfile,
    catalog: Catalog,
    /// The system's time zone, which publish times are typed and shown in.
    /// Read once at start: a zone changed while Bardo runs applies after a
    /// restart.
    zone: Zone,
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
            timelines,
            media_assets,
            music_prompts,
            network_accounts,
            connections,
            renders,
            exports,
            export_files,
            publications,
            cut_suggestions,
            costs,
            files,
            voice_samples,
            secrets,
            connection_secrets,
            tours,
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
        // When this session opened: a due time before it passed while Bardo
        // was closed.
        let opened_at = SystemTime::now();
        let catalog = Catalog::load(profile.ui_language);
        let tours = TourBook::load(profile.id, tours);
        let redactor = Redactor::new();
        let cost_book = CostBook {
            owner: profile.id,
            costs,
            themes: Arc::clone(&themes),
        };
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
            costs: cost_book.clone(),
        };
        let script_handler = ScriptHandler {
            owner: profile.id,
            scripts: Arc::clone(&scripts),
            text: Arc::clone(&providers.text),
            secrets: Arc::clone(&secrets),
            costs: cost_book.clone(),
        };
        let narration_handler = NarrationHandler {
            owner: profile.id,
            narrations: Arc::clone(&narrations),
            files: Arc::clone(&files),
            speech: Arc::clone(&providers.speech),
            secrets: Arc::clone(&secrets),
            costs: cost_book.clone(),
        };
        let import_handler = NarrationImportHandler {
            owner: profile.id,
            narrations: Arc::clone(&narrations),
            files: Arc::clone(&files),
            aligner: Arc::clone(&providers.aligner),
            secrets: Arc::clone(&secrets),
            costs: cost_book.clone(),
        };
        let scene_handler = SceneHandler {
            owner: profile.id,
            plans: Arc::clone(&scene_plans),
            narrations: Arc::clone(&narrations),
            files: Arc::clone(&files),
            text: Arc::clone(&providers.text),
            images: Arc::clone(&providers.images),
            secrets: Arc::clone(&secrets),
            costs: cost_book.clone(),
        };
        let clip_handler = ClipHandler {
            owner: profile.id,
            plans: Arc::clone(&scene_plans),
            files: Arc::clone(&files),
            generators: providers.clips.clone(),
            secrets: Arc::clone(&secrets),
            costs: cost_book.clone(),
        };
        let proxy_handler = ProxyHandler {
            files: Arc::clone(&files),
            media: Arc::clone(&providers.media),
        };
        let render_handler = RenderHandler {
            owner: profile.id,
            renders: Arc::clone(&renders),
            files: Arc::clone(&files),
            media: Arc::clone(&providers.media),
        };
        let music_prompt_handler = MusicPromptHandler {
            owner: profile.id,
            prompts: Arc::clone(&music_prompts),
            text: Arc::clone(&providers.text),
            secrets: Arc::clone(&secrets),
            costs: cost_book.clone(),
        };
        let metadata_handler = MetadataHandler {
            owner: profile.id,
            exports: Arc::clone(&exports),
            text: Arc::clone(&providers.text),
            secrets: Arc::clone(&secrets),
            costs: cost_book.clone(),
        };
        let export_handler = ExportHandler {
            owner: profile.id,
            exports: Arc::clone(&exports),
            renders: Arc::clone(&renders),
            files: Arc::clone(&files),
            export_files: Arc::clone(&export_files),
        };
        let connection_book = ConnectionBook::load(
            profile.id,
            connection_secrets,
            connections,
            providers.sign_ins,
            providers.consent,
            redactor.clone(),
        );
        let metrics_handler = MetricsSyncHandler {
            owner: profile.id,
            publications: Arc::clone(&publications),
            stats: Arc::clone(&providers.video_stats),
            secrets: Arc::clone(&secrets),
            accounts: Arc::clone(&network_accounts),
            connections: connection_book.connections().clone(),
            uploaders: providers.uploaders.clone(),
            analytics: providers.analytics,
            post_insights: providers.post_insights,
        };
        let cut_handler = CutSuggestionHandler {
            owner: profile.id,
            suggestions: Arc::clone(&cut_suggestions),
            narrations: Arc::clone(&narrations),
            decisions: Arc::clone(&providers.decisions),
            secrets: Arc::clone(&secrets),
            costs: cost_book.clone(),
        };
        let provider_keys =
            ProviderKeys::load(secrets, providers.key_checker, redactor.clone(), profile.id);
        let upload_handler = crate::uploads::UploadHandler {
            publications: Arc::clone(&publications),
            accounts: Arc::clone(&network_accounts),
            renders: Arc::clone(&renders),
            files: Arc::clone(&files),
            connections: connection_book.connections().clone(),
            uploaders: providers.uploaders.clone(),
            jobs: Arc::clone(&jobs),
            opened_at,
        };
        // Scheduled posts whose time passed while Bardo was closed wait for
        // the user; their jobs must not start first.
        crate::scheduler::mark_missed(&*publications, &*jobs, profile.id, opened_at)?;
        let jobs = JobQueue::start(
            jobs,
            profile.id,
            crate::jobs::built_in_handlers(crate::jobs::BuiltInHandlers {
                research: research_handler,
                themes: theme_handler,
                scripts: script_handler,
                narrations: narration_handler,
                imports: import_handler,
                scenes: scene_handler,
                clips: clip_handler,
                proxies: proxy_handler,
                music_prompts: music_prompt_handler,
                renders: render_handler,
                metadata: metadata_handler,
                exports: export_handler,
                metrics: metrics_handler,
                cuts: cut_handler,
                uploads: upload_handler,
            }),
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
            timelines,
            media_assets,
            music_prompts,
            network_accounts,
            renders,
            exports,
            export_files,
            publications,
            cut_suggestions,
            cost_book,
            files,
            voice_samples,
            audio: providers.audio,
            speech: providers.speech,
            previews: providers.previews,
            samples_in_flight: Arc::default(),
            media: providers.media,
            market_data: providers.market_data,
            voices: providers.voices,
            clips: providers.clips,
            uploaders: providers.uploaders,
            spec_checks: std::sync::Mutex::default(),
            voice_list: None,
            jobs,
            provider_keys,
            connection_book,
            tours,
            profile,
            catalog,
            zone: Zone::new(jiff::tz::TimeZone::system()),
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

    /// How the profile picks its interface theme.
    pub fn ui_theme(&self) -> UiThemePreference {
        self.profile.ui_theme
    }

    /// Changes how the interface theme is picked and remembers it. On
    /// failure the current preference stays.
    pub fn set_ui_theme(&mut self, preference: UiThemePreference) -> Result<(), AppError> {
        if preference == self.profile.ui_theme {
            return Ok(());
        }
        let updated = UserProfile {
            ui_theme: preference,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        Ok(())
    }

    /// Where the profile's screens place their parts.
    pub fn ui_layout(&self) -> LayoutId {
        self.profile.ui_layout
    }

    /// Changes the layout and remembers it. On failure the current layout
    /// stays.
    pub fn set_ui_layout(&mut self, layout: LayoutId) -> Result<(), AppError> {
        if layout == self.profile.ui_layout {
            return Ok(());
        }
        let updated = UserProfile {
            ui_layout: layout,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
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

    /// A level to the tenth of a decibel, e.g. `−6.0 dB`, `+1,5 dB`.
    pub fn decibels(&self, level: bardo_domain::Decibels) -> String {
        self.catalog.decibels(level)
    }

    /// A 0–100 score as a fraction, e.g. `0.82`, `0,82`.
    pub fn score(&self, score: bardo_domain::Score) -> String {
        self.catalog.score(score)
    }

    /// A share as a percentage to the tenth, e.g. `71.6%`, `71,6%`.
    pub fn percent(&self, share: bardo_domain::Share) -> String {
        self.catalog.percent(share)
    }

    /// Minutes watched, e.g. `45 min`, `1.2K h`.
    pub fn watch_time(&self, minutes: u64) -> String {
        self.catalog.watch_time(minutes)
    }

    /// A length on a clock, e.g. `0:34`, `1:02:09`.
    pub fn clock(&self, seconds: u64) -> String {
        self.catalog.clock(seconds)
    }

    /// An amount to the cent, e.g. `$1,234.56`, `US$ 0,05`.
    pub fn money(&self, amount: bardo_domain::Money) -> String {
        self.catalog.money(amount)
    }

    /// A price with the digits it needs, e.g. `$0.042`.
    pub fn price(&self, amount: bardo_domain::Money) -> String {
        self.catalog.price(amount)
    }

    /// A month and its year, e.g. `October 2026`.
    pub fn month_name(&self, month: bardo_domain::Month) -> String {
        self.catalog.month(month)
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
        Alignment, Answer, ApiKey, CharTiming, ClipDurations, ClipGenerator, ClipHandle, ClipImage,
        ClipModel, ClipModelRef, ClipRequest, ClipStatus, ClipSubmission, Confidence,
        DecisionEngine, Decisions, GeneratedClip, GeneratedImage, GeneratedText, ImageFormat,
        ImageGenerator, ImageRequest, KeyCheck, KeyCheckOutcome, KeyChecker, Market, MarketData,
        MarketSample, Money, Niche, Provider, ProviderFailure, Question, Questions, ScoreAnswer,
        Speech, SpeechRequest, SpeechSynthesizer, StagedImage, TextGenerator, TextRequest,
        TokenUsage, UploadSample, Voice, VoiceLibrary, YesNoAnswer,
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
        /// Yes/no questions whose instructions contain the text get that
        /// probability of yes; 0.2 otherwise.
        pub(crate) yes: Mutex<Vec<(String, f64)>>,
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
                yes: Mutex::default(),
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

        /// Yes/no questions whose instructions contain `needle` get
        /// `probability` of yes.
        pub(crate) fn set_yes(&self, needle: &str, probability: f64) {
            self.yes
                .lock()
                .unwrap()
                .push((needle.to_owned(), probability));
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
            let yes = self.yes.lock().unwrap();
            let confidence = Confidence::new(*self.confidence.lock().unwrap());
            let mut answers = HashMap::new();
            for (id, question) in questions.iter() {
                let answer = match question {
                    Question::Score {
                        instructions,
                        levels: names,
                    } => {
                        let level = levels
                            .iter()
                            .find(|(needle, _)| instructions.contains(needle.as_str()))
                            .map_or((names.len() - 1) as f64 / 2.0, |(_, level)| *level);
                        Answer::Score(ScoreAnswer {
                            level,
                            probabilities: vec![0.0; names.len()],
                            confidence,
                        })
                    }
                    Question::YesNo { instructions, .. } => Answer::YesNo(YesNoAnswer::new(
                        yes.iter()
                            .find(|(needle, _)| instructions.contains(needle.as_str()))
                            .map_or(0.2, |(_, probability)| *probability),
                    )),
                    Question::Choice { .. } => panic!("the fake answers no choice"),
                };
                answers.insert(id.to_owned(), answer);
            }
            Ok(Decisions {
                answers,
                model: "jev-fake".into(),
                usage: TokenUsage {
                    input_tokens: 1_500,
                    output_tokens: 30,
                },
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

    /// Has no preview to give; tests of previews bring their own fake.
    pub(crate) struct NoPreviews;

    impl bardo_domain::VoicePreviews for NoPreviews {
        fn download(&self, _url: &str) -> Result<Vec<u8>, ProviderFailure> {
            Err(ProviderFailure::new(
                bardo_domain::ProviderFailureKind::Unreachable,
                "no previews in tests",
            ))
        }
    }

    /// One second of MP3 audio, the unit of every fake part.
    pub(crate) const PART_AUDIO: &[u8] =
        include_bytes!("../../media/tests/fixtures/tone-1s-raw.mp3");

    /// Reads every request as `length` (one second by default) of tone, its
    /// characters timed evenly across it; `failure` wins when set. Keeps the
    /// requests.
    pub(crate) struct FakeSpeech {
        pub(crate) requests: Mutex<Vec<SpeechRequest>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        pub(crate) max_chars: Mutex<usize>,
        /// Fails this request (counting from 0) once, as a provider outage.
        pub(crate) fail_at: Mutex<Option<usize>>,
        /// How long each call takes, to catch a job mid-run.
        pub(crate) delay: Mutex<Duration>,
        /// How long each request reads, its characters evenly spread.
        pub(crate) length: Mutex<Duration>,
    }

    impl Default for FakeSpeech {
        fn default() -> Self {
            Self {
                requests: Mutex::default(),
                failure: Mutex::default(),
                max_chars: Mutex::new(5_000),
                fail_at: Mutex::default(),
                delay: Mutex::default(),
                length: Mutex::new(Duration::from_secs(1)),
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
            let length = *self.length.lock().unwrap();
            let step = length / chars.len().max(1) as u32;
            // Whole seconds of tone, so the audio lasts as long as it reads.
            let seconds = length.as_secs_f64().ceil().max(1.0) as usize;
            Ok(Speech {
                audio: PART_AUDIO.repeat(seconds),
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
                usage: bardo_domain::Metered {
                    input_tokens: 12,
                    output_tokens: 210,
                    image_tokens: 1_680,
                    ..bardo_domain::Metered::default()
                },
            })
        }
    }

    /// The bytes of every fake clip: the start of an MP4 file.
    pub(crate) const CLIP: &[u8] = b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom";

    /// One fake request.
    #[derive(Debug, Clone)]
    pub(crate) struct FakeClipRequest {
        pub(crate) request: ClipRequest,
        pub(crate) submission: String,
    }

    /// A video provider with two models: `fake/range` (3-15 s) and
    /// `fake/choices` (5 or 10 s). Answers every request as running while
    /// `hold` is set, else as done; a prompt containing a word in `failing`
    /// fails at the provider. `submit_failure` makes submissions fail, and
    /// `status_outages` the first status checks.
    /// Resending a submission id returns its first request, as Higgsfield
    /// does with an idempotency key.
    #[derive(Default)]
    pub(crate) struct FakeClips {
        pub(crate) staged: Mutex<Vec<ClipImage>>,
        pub(crate) requests: Mutex<Vec<FakeClipRequest>>,
        pub(crate) polls: Mutex<Vec<String>>,
        pub(crate) hold: std::sync::atomic::AtomicBool,
        pub(crate) failing: Mutex<Vec<String>>,
        pub(crate) submit_failure: Mutex<Option<ProviderFailure>>,
        pub(crate) quote: Mutex<Option<Money>>,
        /// How many status checks fail on the provider's side first.
        pub(crate) status_outages: Mutex<u32>,
    }

    impl FakeClips {
        pub(crate) fn submissions(&self) -> Vec<ClipRequest> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|sent| sent.request.clone())
                .collect()
        }

        pub(crate) fn hold(&self, hold: bool) {
            self.hold.store(hold, std::sync::atomic::Ordering::SeqCst);
        }

        /// Fails every request whose prompt contains `word` from now on.
        pub(crate) fn fail(&self, word: &str) {
            self.failing.lock().unwrap().push(word.to_owned());
        }

        pub(crate) fn succeed_all(&self) {
            self.failing.lock().unwrap().clear();
        }
    }

    impl ClipGenerator for FakeClips {
        fn provider(&self) -> Provider {
            Provider::Higgsfield
        }

        fn models(&self) -> Vec<ClipModel> {
            vec![
                ClipModel {
                    id: ClipModelRef::new(Provider::Higgsfield, "fake/range").unwrap(),
                    name: "Fake Range".into(),
                    durations: ClipDurations::Range { min: 3, max: 15 },
                },
                ClipModel {
                    id: ClipModelRef::new(Provider::Higgsfield, "fake/choices").unwrap(),
                    name: "Fake Choices".into(),
                    durations: ClipDurations::Choices(vec![5, 10]),
                },
            ]
        }

        fn stage_image(
            &self,
            _key: &ApiKey,
            image: &ClipImage,
        ) -> Result<StagedImage, ProviderFailure> {
            let mut staged = self.staged.lock().unwrap();
            staged.push(image.clone());
            Ok(StagedImage(format!("https://fake/image-{}", staged.len())))
        }

        fn submit(
            &self,
            _key: &ApiKey,
            request: &ClipRequest,
            submission: &str,
        ) -> Result<ClipSubmission, ProviderFailure> {
            if let Some(failure) = self.submit_failure.lock().unwrap().clone() {
                return Err(failure);
            }
            let mut requests = self.requests.lock().unwrap();
            let index = match requests
                .iter()
                .position(|sent| sent.submission == submission)
            {
                Some(index) => index,
                None => {
                    requests.push(FakeClipRequest {
                        request: request.clone(),
                        submission: submission.to_owned(),
                    });
                    requests.len() - 1
                }
            };
            Ok(ClipSubmission {
                handle: ClipHandle(format!("req-{index}")),
                quote: *self.quote.lock().unwrap(),
            })
        }

        fn status(
            &self,
            _key: &ApiKey,
            handle: &ClipHandle,
        ) -> Result<ClipStatus, ProviderFailure> {
            self.polls.lock().unwrap().push(handle.0.clone());
            let mut outages = self.status_outages.lock().unwrap();
            if *outages > 0 {
                *outages -= 1;
                return Err(ProviderFailure::new(
                    bardo_domain::ProviderFailureKind::ProviderDown,
                    "HTTP 503",
                ));
            }
            drop(outages);
            let index: usize = handle.0.trim_start_matches("req-").parse().unwrap();
            let prompt = self.requests.lock().unwrap()[index].request.prompt.clone();
            if self.hold.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(ClipStatus::Running);
            }
            if let Some(word) = self
                .failing
                .lock()
                .unwrap()
                .iter()
                .find(|word| prompt.contains(word.as_str()))
            {
                return Ok(ClipStatus::Failed(ProviderFailure::new(
                    bardo_domain::ProviderFailureKind::Unexpected,
                    format!("could not make the clip: {word}"),
                )));
            }
            Ok(ClipStatus::Done {
                video: format!("https://fake/{}.mp4", handle.0),
            })
        }

        fn download(&self, _key: &ApiKey, _video: &str) -> Result<GeneratedClip, ProviderFailure> {
            Ok(GeneratedClip {
                bytes: CLIP.to_vec(),
            })
        }

        fn poll_delay(&self, _polls: u32) -> Duration {
            Duration::from_millis(1)
        }
    }

    /// Public statistics from a table the test fills; remembers each call.
    #[derive(Default)]
    pub(crate) struct FakeVideoStats {
        pub(crate) found: Mutex<Vec<bardo_domain::VideoStatistics>>,
        pub(crate) calls: Mutex<Vec<Vec<String>>>,
        pub(crate) failure: Mutex<Option<ProviderFailure>>,
        /// While set, calls wait (a test can catch a sync running).
        pub(crate) hold: std::sync::atomic::AtomicBool,
    }

    impl FakeVideoStats {
        /// The post's statistics from now on, replacing earlier ones.
        pub(crate) fn set(&self, post_id: &str, views: u64, likes: Option<u64>) {
            let mut found = self.found.lock().unwrap();
            found.retain(|stats| stats.post_id != post_id);
            found.push(bardo_domain::VideoStatistics {
                post_id: post_id.to_owned(),
                published_at: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000)),
                views,
                likes,
                comments: Some(views / 100),
            });
        }

        pub(crate) fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl bardo_domain::VideoStats for FakeVideoStats {
        fn statistics(
            &self,
            _key: &ApiKey,
            ids: &[&str],
        ) -> Result<Vec<bardo_domain::VideoStatistics>, ProviderFailure> {
            while self.hold.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
            self.calls
                .lock()
                .unwrap()
                .push(ids.iter().map(|id| (*id).to_owned()).collect());
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            Ok(self
                .found
                .lock()
                .unwrap()
                .iter()
                .filter(|stats| ids.contains(&stats.post_id.as_str()))
                .cloned()
                .collect())
        }
    }

    /// Answers owner reports from what a test set per video, and keeps
    /// every call.
    #[derive(Default)]
    pub(crate) struct FakeAnalytics {
        pub(crate) reports: Mutex<HashMap<String, bardo_domain::VideoReport>>,
        /// Money reports answer 403.
        pub(crate) not_monetized: std::sync::atomic::AtomicBool,
        pub(crate) failure: Mutex<Option<bardo_domain::AnalyticsError>>,
        /// (video, money asked) per report; video per curve.
        pub(crate) calls: Mutex<Vec<(String, bool)>>,
        pub(crate) curves: Mutex<Vec<String>>,
        /// The token each call carried.
        pub(crate) tokens: Mutex<Vec<String>>,
    }

    impl FakeAnalytics {
        /// `views` for `video`, the other numbers derived from them.
        pub(crate) fn set(&self, video: &str, views: u64) {
            self.reports.lock().unwrap().insert(
                video.to_owned(),
                bardo_domain::VideoReport {
                    views,
                    engaged_views: views * 7 / 10,
                    minutes_watched: views / 4,
                    average_view_seconds: 33,
                    average_view_share: bardo_domain::Share::of_percent(62.5),
                    money: None,
                },
            );
        }

        pub(crate) fn calls(&self) -> Vec<(String, bool)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl bardo_domain::OwnerAnalytics for FakeAnalytics {
        fn network(&self) -> bardo_domain::Network {
            bardo_domain::Network::YouTube
        }

        fn video_report(
            &self,
            token: &bardo_domain::SecretText,
            video: &str,
            _: &bardo_domain::ReportPeriod,
            money: bool,
        ) -> Result<Option<bardo_domain::VideoReport>, bardo_domain::AnalyticsError> {
            self.calls.lock().unwrap().push((video.to_owned(), money));
            self.tokens.lock().unwrap().push(token.expose().to_owned());
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            if money && self.not_monetized.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(bardo_domain::AnalyticsError::new(
                    bardo_domain::AnalyticsErrorKind::Forbidden,
                    "HTTP 403: forbidden",
                ));
            }
            Ok(self
                .reports
                .lock()
                .unwrap()
                .get(video)
                .map(|report| bardo_domain::VideoReport {
                    money: money.then(|| bardo_domain::MoneyReport {
                        revenue: Money::from_micros(report.views * 2_000),
                        cpm: Money::from_cents(410),
                        playback_cpm: Money::from_cents(380),
                    }),
                    ..*report
                }))
        }

        fn retention(
            &self,
            _: &bardo_domain::SecretText,
            video: &str,
            _: &bardo_domain::ReportPeriod,
        ) -> Result<Vec<bardo_domain::RetentionPoint>, bardo_domain::AnalyticsError> {
            self.curves.lock().unwrap().push(video.to_owned());
            let share = bardo_domain::Share::from_ten_thousandths;
            Ok((1..=100)
                .map(|i| bardo_domain::RetentionPoint {
                    elapsed: share(i * 100),
                    watch: share(11_000 - i * 80),
                    relative: Some(share(5_500)),
                })
                .collect())
        }
    }

    /// Instagram or TikTok post numbers of a connected account.
    pub(crate) struct FakeInsights {
        network: bardo_domain::Network,
        batch: usize,
        /// Numbers per post id; a post left out is not the account's.
        pub(crate) posts: Mutex<HashMap<String, bardo_domain::PostNumbers>>,
        /// The account's media listing, in pages.
        pub(crate) media: Mutex<Vec<bardo_domain::MediaPage>>,
        pub(crate) failure: Mutex<Option<bardo_domain::AnalyticsError>>,
        /// The posts of each read; the cursor of each listed page.
        pub(crate) reads: Mutex<Vec<Vec<String>>>,
        pub(crate) pages: Mutex<Vec<Option<String>>>,
        /// The token each call carried.
        pub(crate) tokens: Mutex<Vec<String>>,
    }

    impl FakeInsights {
        pub(crate) fn instagram() -> Self {
            Self::of(bardo_domain::Network::InstagramReels, 1)
        }

        pub(crate) fn tiktok() -> Self {
            Self::of(bardo_domain::Network::TikTok, 20)
        }

        fn of(network: bardo_domain::Network, batch: usize) -> Self {
            Self {
                network,
                batch,
                posts: Mutex::default(),
                media: Mutex::default(),
                failure: Mutex::default(),
                reads: Mutex::default(),
                pages: Mutex::default(),
                tokens: Mutex::default(),
            }
        }

        /// `views` for `post`, the other numbers derived from them.
        pub(crate) fn set(&self, post: &str, views: u64) {
            self.set_numbers(
                post,
                bardo_domain::PostNumbers {
                    views: Some(views),
                    likes: Some(views / 10),
                    comments: Some(views / 100),
                    insights: bardo_domain::Insights {
                        shares: Some(views / 50),
                        ..bardo_domain::Insights::default()
                    },
                    posted_at: None,
                },
            );
        }

        pub(crate) fn set_numbers(&self, post: &str, numbers: bardo_domain::PostNumbers) {
            self.posts.lock().unwrap().insert(post.to_owned(), numbers);
        }

        /// Lists `(media id, shortcode)` on one page.
        pub(crate) fn list(&self, media: &[(&str, &str)]) {
            self.media.lock().unwrap().push(bardo_domain::MediaPage {
                items: media
                    .iter()
                    .map(|(id, code)| bardo_domain::MediaItem {
                        id: (*id).to_owned(),
                        permalink: Some(format!("https://www.instagram.com/reel/{code}/")),
                        shortcode: Some((*code).to_owned()),
                        posted_at: None,
                    })
                    .collect(),
                next: None,
            });
        }

        pub(crate) fn reads(&self) -> Vec<Vec<String>> {
            self.reads.lock().unwrap().clone()
        }

        pub(crate) fn pages(&self) -> Vec<Option<String>> {
            self.pages.lock().unwrap().clone()
        }
    }

    impl bardo_domain::InsightsReader for FakeInsights {
        fn network(&self) -> bardo_domain::Network {
            self.network
        }

        fn batch(&self) -> usize {
            self.batch
        }

        fn read(
            &self,
            token: &bardo_domain::SecretText,
            posts: &[&str],
        ) -> Result<Vec<(String, bardo_domain::PostNumbers)>, bardo_domain::AnalyticsError>
        {
            self.reads
                .lock()
                .unwrap()
                .push(posts.iter().map(|post| (*post).to_owned()).collect());
            self.tokens.lock().unwrap().push(token.expose().to_owned());
            if let Some(failure) = self.failure.lock().unwrap().clone() {
                return Err(failure);
            }
            let known = self.posts.lock().unwrap();
            Ok(posts
                .iter()
                .filter_map(|post| {
                    known
                        .get(*post)
                        .map(|numbers| ((*post).to_owned(), *numbers))
                })
                .collect())
        }

        fn media_page(
            &self,
            token: &bardo_domain::SecretText,
            _: &str,
            after: Option<&str>,
        ) -> Result<bardo_domain::MediaPage, bardo_domain::AnalyticsError> {
            self.pages.lock().unwrap().push(after.map(str::to_owned));
            self.tokens.lock().unwrap().push(token.expose().to_owned());
            let page = after.map_or(0, |cursor| cursor.parse().unwrap_or(usize::MAX));
            let media = self.media.lock().unwrap();
            let Some(found) = media.get(page) else {
                return Ok(bardo_domain::MediaPage::default());
            };
            Ok(bardo_domain::MediaPage {
                items: found.items.clone(),
                next: (page + 1 < media.len()).then(|| (page + 1).to_string()),
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
            video_stats: Arc::new(FakeVideoStats::default()),
            text: Arc::new(FakeTextGenerator::default()),
            decisions: Arc::new(FakeDecisionEngine::default()),
            voices: Arc::new(FakeVoiceLibrary::default()),
            speech: Arc::new(FakeSpeech::default()),
            previews: Arc::new(NoPreviews),
            aligner: Arc::new(crate::narration_import::testing::FakeAligner::default()),
            images: Arc::new(FakeImages::default()),
            clips: vec![Arc::new(FakeClips::default())],
            audio: Arc::new(crate::narrations::testing::FakeAudioOutput::default()),
            media: Arc::new(crate::editor::testing::FakeMedia::default()),
            sign_ins: Vec::new(),
            consent: Arc::new(crate::connections::testing::NoConsent),
            uploaders: Vec::new(),
            analytics: Vec::new(),
            post_insights: Vec::new(),
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
            tours: Box::new(Arc::clone(&db)),
            channels: Box::new(Arc::clone(&db)),
            jobs: Arc::clone(&db) as _,
            themes: Arc::clone(&db) as _,
            templates: Arc::clone(&db) as _,
            scripts: Arc::clone(&db) as _,
            personas: Arc::new(FakePersonas::default()),
            narrations: Arc::clone(&db) as _,
            scene_plans: Arc::clone(&db) as _,
            timelines: Arc::clone(&db) as _,
            media_assets: Arc::clone(&db) as _,
            music_prompts: Arc::clone(&db) as _,
            network_accounts: Arc::clone(&db) as _,
            connections: Arc::clone(&db) as _,
            renders: Arc::clone(&db) as _,
            exports: Arc::clone(&db) as _,
            publications: Arc::clone(&db) as _,
            cut_suggestions: Arc::clone(&db) as _,
            export_files: Arc::new(MemoryExportFiles::default()),
            costs: Arc::clone(&db) as _,
            files: Arc::new(MemoryProjectFiles::default()),
            voice_samples: Arc::new(bardo_storage::MemoryVoiceSamples::default()),
            research: db,
            secrets: Arc::new(MemorySecretStore::default()),
            connection_secrets: Arc::new(MemorySecretStore::default()),
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
    fn a_new_profile_follows_the_system_with_paper_and_graphite() {
        let app = start(&FakeProfiles::default(), None);
        assert_eq!(app.ui_theme(), UiThemePreference::default());
    }

    #[test]
    fn theme_choice_survives_a_restart() {
        let profiles = FakeProfiles::default();
        let fixed = UiThemePreference::Fixed(bardo_domain::UiTheme::BlackGold);
        start(&profiles, None).set_ui_theme(fixed).unwrap();
        assert_eq!(start(&profiles, None).ui_theme(), fixed);
    }

    #[test]
    fn failed_save_keeps_the_current_theme() {
        let profiles = FakeProfiles::default();
        let mut app = start(&profiles, None);
        profiles.fail_saves.set(true);

        let fixed = UiThemePreference::Fixed(bardo_domain::UiTheme::Phosphor);
        assert!(app.set_ui_theme(fixed).is_err());
        assert_eq!(app.ui_theme(), UiThemePreference::default());
    }

    #[test]
    fn a_new_profile_uses_the_workspace_layout() {
        let app = start(&FakeProfiles::default(), None);
        assert_eq!(app.ui_layout(), LayoutId::Workspace);
    }

    #[test]
    fn layout_choice_survives_a_restart() {
        let profiles = FakeProfiles::default();
        start(&profiles, None)
            .set_ui_layout(LayoutId::Studio)
            .unwrap();
        assert_eq!(start(&profiles, None).ui_layout(), LayoutId::Studio);
    }

    #[test]
    fn failed_save_keeps_the_current_layout() {
        let profiles = FakeProfiles::default();
        let mut app = start(&profiles, None);
        profiles.fail_saves.set(true);

        assert!(app.set_ui_layout(LayoutId::Studio).is_err());
        assert_eq!(app.ui_layout(), LayoutId::Workspace);
    }

    #[test]
    fn works_against_real_sqlite() {
        let db = Database::open_in_memory().unwrap();
        let repositories = Repositories::local(
            db,
            Box::new(MemorySecretStore::default()),
            Box::new(MemorySecretStore::default()),
            Box::new(MemoryProjectFiles::default()),
            Box::new(MemoryExportFiles::default()),
            Box::new(MemoryVoiceSamples::default()),
        );
        let mut app = Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap();
        app.set_ui_language(UiLanguage::PtBr).unwrap();
        assert_eq!(app.ui_language(), UiLanguage::PtBr);
        let fixed = UiThemePreference::Fixed(bardo_domain::UiTheme::Brass);
        app.set_ui_theme(fixed).unwrap();
        assert_eq!(app.ui_theme(), fixed);
        app.set_ui_layout(LayoutId::Studio).unwrap();
        assert_eq!(app.ui_layout(), LayoutId::Studio);
    }
}
