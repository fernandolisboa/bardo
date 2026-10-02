//! Entities, value objects, domain services and provider interfaces.
//!
//! No I/O lives here; adapters in other crates implement the interfaces.

/// Declares a UUID-backed identifier type.
macro_rules! uuid_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(::uuid::Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(::uuid::Uuid::new_v4())
            }

            pub fn as_uuid(&self) -> ::uuid::Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<::uuid::Uuid> for $name {
            fn from(value: ::uuid::Uuid) -> Self {
                Self(value)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

mod appearance;
mod budget;
mod caption;
mod channel;
mod clip;
mod cost;
mod decision;
mod edit;
mod files;
mod framing;
mod generation;
mod image;
mod job;
mod market;
mod media;
mod metadata;
mod mix;
mod music_prompt;
mod narration;
mod network;
mod network_account;
mod persona;
mod profile;
mod provider_key;
mod publication;
mod redaction;
mod render;
mod repository;
mod research;
mod scene;
mod script;
mod speech;
mod template;
mod text;
mod theme;
mod timeline;
mod voice;

pub use appearance::{
    LayoutId, ThemeFamily, ThemeMode, UiTheme, UiThemePreference, UnknownLayout, UnknownUiTheme,
    UnknownUiThemePreference,
};
pub use budget::{Budget, BudgetLevel, Month};
pub use caption::{
    Caption, CaptionStyle, Captions, LINE_RULES, LineRules, MAX_CAPTION_CHARS, SavedCaptions,
    UnknownCaptionStyle, caption_lines, caption_text,
};
pub use channel::{
    Channel, ChannelDetails, ChannelDraft, ChannelFieldError, ChannelId, ChannelRepository,
};
pub use clip::{
    ClipDurations, ClipGenerator, ClipHandle, ClipImage, ClipModel, ClipModelError, ClipModelRef,
    ClipRequest, ClipStatus, ClipSubmission, GeneratedClip, StagedImage,
};
pub use cost::{
    Cost, CostPurpose, CostRecord, CostRecordId, CostRepository, Meter, Metered, Money, MoneyError,
    Rate, RateFieldError, RateTable, Spend, UnknownCostPurpose, UnknownMeter,
};
pub use decision::{
    Answer, ChoiceAnswer, Confidence, DecisionEngine, Decisions, InvalidQuestion, Question,
    Questions, ScoreAnswer, YesNoAnswer,
};
pub use edit::{Edge, Edit, EditError, HISTORY_DEPTH, History, Item, ItemRef, Shift, Track};
pub use files::{ProjectFileError, ProjectFiles};
pub use framing::{CROP_STEPS, CropPosition, CropRect, Framing, PictureSize, crop_window};
pub use generation::{Generation, GenerationId, TemplateUsed};
pub use image::{GeneratedImage, ImageFormat, ImageGenerator, ImageRequest};
pub use job::{
    InconsistentJob, InvalidJobTransition, Job, JobFailure, JobFailureKind, JobId, JobKind,
    JobRecord, JobRepository, JobState, Progress, RetryPolicy, UnknownJobFailureKind,
    UnknownJobKind, UnknownJobState,
};
pub use market::{
    ContentLanguage, Country, Market, UnsupportedContentLanguage, UnsupportedCountry,
};
pub use media::{
    AssetSource, MediaAsset, MediaAssetId, MediaAssetRepository, MediaKind, UnknownAssetSource,
    UnknownMediaKind,
};
pub use metadata::{
    DisclosureLabel, Export, ExportFiles, ExportRepository, METADATA_FILE, MetadataProblem,
    MetadataRules, Post, TagPlacement, VideoMetadata, VideoMetadataDraft, compose, normalize_tags,
    package_folder, problems, text_length, video_file_name,
};
pub use mix::{
    AudioLane, DEFAULT_DUCK, DUCK_ATTACK, DUCK_HOLD, DUCK_RANGE, DUCK_RELEASE, Decibels, Dip,
    DuckEnvelope, Ducking, GAIN_RANGE, LaneMix, Mix,
};
pub use music_prompt::{MusicPrompt, MusicPromptFieldError, MusicPromptRepository};
pub use narration::{
    InvalidWordTimings, Narration, NarrationId, NarrationRepository, NarrationSource, WordTiming,
    WordTimings, spoken_words,
};
pub use network::{
    AspectRatio, Bitrate, Loudness, MaxDuration, Network, OutOfRange, PresetOverrides,
    RenderPreset, Resolution, UnknownAspectRatio, UnknownNetwork, UnknownVideoCodec,
    UnknownVisibility, VideoCodec, Visibility,
};
pub use network_account::{
    MetadataDefaults, NetworkAccount, NetworkAccountDetails, NetworkAccountDraft,
    NetworkAccountFieldError, NetworkAccountId, NetworkAccountRepository,
};
pub use persona::{
    GenerationPresets, Persona, PersonaDetails, PersonaDraft, PersonaFieldError, PersonaId,
    PersonaRepository, VoiceFlag,
};
pub use profile::{ProfileId, ProfileRepository, UiLanguage, UnsupportedLanguage, UserProfile};
pub use provider_key::{
    ApiKey, ApiKeyError, KeyCheck, KeyCheckOutcome, KeyChecker, Provider, ProviderFailure,
    ProviderFailureKind, SecretStore, SecretStoreError, UnknownProvider,
};
pub use publication::{
    ChannelPoint, MetricsSnapshot, MetricsSyncOnStart, MetricsTotals, PostLink, PostLinkError,
    Publication, PublicationId, PublicationRepository, STATS_BATCH, UnknownMetricsSync,
    VideoStatistics, VideoStats, channel_history, latest_of_each, sync_quota_units,
};
pub use redaction::Redactor;
pub use render::{
    CutFacts, Gate, GateLevel, LOUDNESS_GAIN_WARNING, LOUDNESS_TOLERANCE, MeasuredLoudness, Render,
    RenderId, RenderRepository, SILENCE, TRUE_PEAK_CEILING, cut_gates, output_gates,
};
pub use repository::RepositoryError;
pub use research::{
    MarketData, MarketSample, Niche, NicheResearch, NicheResearchRepository, NicheScores,
    NicheSeedError, NicheSeeds, NicheStatistics, Score, UploadSample, rank,
};
pub use scene::{
    MAX_SENTENCE_WORDS, NoPendingClip, NoPendingImage, NoScenes, NoSuchScene, Scene, SceneClip,
    SceneDraft, SceneFieldError, SceneImage, ScenePlan, ScenePlanId, ScenePlanRecord,
    ScenePlanRepository, ScenePrompt, SceneRecord, Sentence, sentences,
};
pub use script::{
    GeneratedScript, NoPendingScript, Script, ScriptFieldError, ScriptRecord, ScriptRepository,
    ScriptText,
};
pub use speech::{
    AlignedSpeech, Alignment, AlignmentRequest, CharTiming, Speech, SpeechAligner, SpeechRequest,
    SpeechSynthesizer, split_for_speech,
};
pub use template::{
    MissingValue, RenderedPrompt, TemplateBody, TemplateField, TemplateFieldError, TemplateKind,
    TemplateProblem, TemplateRepository, TemplateValues, TemplateVariable, TemplateVersion,
    TemplateVersionId, UnknownTemplateKind,
};
pub use text::{GeneratedText, TextFormat, TextGenerator, TextRequest, TokenUsage};
pub use theme::{
    Reason, Theme, ThemeFieldError, ThemeId, ThemeIdea, ThemeNotSuggested, ThemeRanking,
    ThemeRecord, ThemeRepository, ThemeStatus, UnknownThemeStatus, VideoProject, VideoProjectId,
    rank_themes,
};
pub use timeline::{
    AudioItem, CaptionSpan, FPS, Picture, SavedAudioItem, SavedTimeline, SavedVideoItem, Timeline,
    TimelineRepository, VideoItem, VideoSource, frame_at, frame_time, min_length, nearest_frame,
    snap, timecode,
};
pub use voice::{InvalidVoiceRef, Voice, VoiceCategory, VoiceLibrary, VoiceRef};
