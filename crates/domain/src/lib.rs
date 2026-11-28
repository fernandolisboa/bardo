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

mod ad_angle;
mod ad_brief;
mod ad_export;
mod ad_framework;
mod ad_review;
mod agent_task;
mod appearance;
mod backup;
mod budget;
mod byteplus;
mod caption;
mod capture;
mod channel;
mod chapters;
mod claims_check;
mod clip;
mod connection;
mod control_video;
mod cost;
mod cut_retention;
mod cut_suggestion;
mod decision;
mod due;
mod edit;
mod end_card;
mod files;
mod folders;
mod format;
mod framing;
mod generation;
mod image;
mod job;
mod localization;
mod loose;
mod market;
mod media;
mod metadata;
mod mix;
mod model_name;
mod motion;
mod music_prompt;
mod music_track;
mod narration;
mod network;
mod network_account;
mod owner_metrics;
mod package;
mod performance;
mod performed;
mod performed_cut;
mod persona;
mod persona_look;
mod plan;
mod post_insights;
mod preflight;
mod product;
mod product_sheet;
mod product_shot;
mod production;
mod profile;
mod project;
mod project_key;
mod provider_key;
mod publication;
mod redaction;
mod reel;
mod removal;
mod render;
mod repository;
mod research;
mod runner;
mod scene;
mod scene_kind;
mod scene_plan_size;
mod schedule;
mod script;
mod sharing;
mod shot_look;
mod speech;
mod staging;
mod take_analysis;
mod take_model;
mod template;
mod text;
mod theme;
mod thumbnail;
mod tiktok;
mod timeline;
mod tour;
mod transition;
mod update;
mod upload;
mod visual_style;
mod voice;
mod voice_sample;
mod web_research;
mod world;
mod youtube_policy;

pub use ad_angle::{
    AD_START_LENGTH, AdAngle, AdAngleFieldError, AdAngleId, AdAngleIdea, AdAngleNotSuggested,
    AdAngleRecord, AdRanking, AdRepository, AdStart, rank_ad_angles,
};
pub use ad_brief::{
    AdBrief, AdBriefRecord, BriefClaim, BriefClaimError, BriefFieldError, BriefNotReady, BriefText,
    ClaimDrift, ClaimSeen,
};
pub use ad_export::{
    AD_HEADLINE_MAX, AD_PRIMARY_MAX, AdExport, AdExportRepository, AdTextProblem, AdTexts,
    AdWindow, ad_clip_key,
};
pub use ad_framework::AdFramework;
pub use ad_review::{
    AdPostReview, AdReviewItem, CommercialDisclosure, DisclosureStep, MAX_SPONSORS, SponsorError,
    ad_synthetic_default, disclosure_step, sponsor_ids, unacknowledged,
};
pub use agent_task::{AgentTask, AgentTaskError};
pub use appearance::{
    LayoutId, ThemeFamily, ThemeMode, UiTheme, UiThemePreference, UnknownLayout, UnknownUiTheme,
    UnknownUiThemePreference,
};
pub use backup::{
    AUTOMATIC_BACKUP_EVERY, AUTOMATIC_BACKUPS_KEPT, BackupError, BackupInfo, BackupOptions,
    BackupStore, MediaSize, ProgressReport, RestoreOutcome, SavedBackup,
};
pub use budget::{Budget, BudgetLevel, Month};
pub use byteplus::{
    BYTEPLUS_DEFAULT_REGION, BYTEPLUS_SEPARATOR, BytePlusCredentials, BytePlusField,
    BytePlusFieldError, is_bucket_name,
};
pub use caption::{
    CAPTION_BAND, Caption, CaptionHighlight, CaptionStyle, Captions, LINE_RULES, LineRules,
    MAX_CAPTION_CHARS, SavedCaptions, UnknownCaptionHighlight, UnknownCaptionStyle, caption_band,
    caption_lines, caption_text,
};
pub use capture::{
    BoundaryError, BoundaryMove, CAPTURE_EXTENSIONS, Capture, CaptureConfirmation, CaptureId,
    CaptureProblem, CaptureRepository, CaptureVideo, CaptureVoice, CutKind, HeardWord,
    InvalidTakes, MAX_CAPTURE, MAX_CAPTURE_FPS, MAX_TAKE, MIN_CAPTURE, MIN_CAPTURE_FPS,
    MIN_CAPTURE_SIDE, MIN_SCRIPT_COVERAGE, MIN_TAKE, MIN_TAKE_PAUSE, RecordingFacts, TAKE_PADDING,
    Take, TakeFlag, TakeSource, Takes, audio_parts, script_coverage,
};
pub use channel::{
    Channel, ChannelDetails, ChannelDraft, ChannelFieldError, ChannelId, ChannelRepository,
};
pub use chapters::{
    Chapter, ChapterFieldError, ChapterProblem, ChapterRepository, Chapters, InvalidChapter,
    MAX_CHAPTER_TITLE, MIN_CHAPTER_LENGTH, MIN_CHAPTERS, chapter_lines, chapter_problems,
    chapter_title, place_chapters, timestamp,
};
pub use claims_check::{
    CheckedScript, CheckedStatement, ClaimsAnswerError, ClaimsCheck, checked_statements,
    claims_check_schema, posts_text,
};
pub use clip::{
    ClipDurations, ClipFit, ClipGenerator, ClipHandle, ClipImage, ClipLength, ClipModel,
    ClipModelError, ClipModelRef, ClipRequest, ClipStatus, ClipSubmission, CurrentClipModel,
    GeneratedClip, RetiredClipModel, StagedImage, StretchLimit, current_clip_model,
};
pub use connection::{
    AppCredentials, AppCredentialsFieldError, BrowserSignIn, ConnectedIdentity, ConnectionSecrets,
    ConnectionStatus, ConsentCallback, ConsentError, ConsentPages, ConsentReceiver, ConsentRequest,
    DiscoveredAccount, NetworkConnection, NetworkConnectionRepository, NetworkSignIn,
    PastedTokenError, PastedTokenSignIn, SecretText, SignInFailure, SignInFailureKind,
    SignInMethod, TokenGrant, TokenSet, TokenSetTooLarge, parse_pasted_token,
};
pub use control_video::{
    ApprovalBlock, CONTROL_AUDIO_BITRATE, CONTROL_BUDGET_BYTES, CONTROL_KEYFRAME_EVERY,
    CONTROL_LONG_SIDE, CONTROL_SHORT_SIDE, ControlVideo, FACE_HOLD, FIGURE_FAR_PERMILLE,
    FIGURE_MASK_FROM, FIGURE_NEAR_PERMILLE, FaceCover, HELD_GROWTH, MAX_CONTROL_BITRATE,
    MAX_CONTROL_BYTES, MIN_CONTROL_BITRATE, Outbound, SET_EXTENSIONS, SetSource, TakeSet,
    approval_block, control_bitrate, control_fingerprint, control_fps, control_size, face_cover,
    merged_checks, needed_analyses,
};
pub use cost::{
    Cost, CostPurpose, CostRecord, CostRecordId, CostRepository, Meter, Metered, Money, MoneyError,
    Rate, RateFieldError, RateTable, Spend, UnknownCostPurpose, UnknownMeter, X_POST,
    X_POST_WITH_LINK, x_post_model,
};
pub use cut_retention::{
    MAX_DROPS, MIN_STEP_LOSS, PlacedDrop, RenderedCut, RetentionDrop, STEEP_TIMES, cut_timing,
    place_drops, retention_drops, time_at,
};
pub use cut_suggestion::{
    ChunkLimits, CutCandidate, CutChunk, CutPlace, CutReasons, CutRules, CutSuggestion,
    CutSuggestionRepository, CutSuggestions, DEFAULT_CUT_FLOOR, STRONG_CUT, SuggestionStatus,
    cut_candidates, cut_chunks, cut_marker, open_candidates, place_cut,
};
pub use decision::{
    Answer, ChoiceAnswer, Confidence, DecisionEngine, DecisionEngineId, Decisions, InvalidQuestion,
    Question, Questions, ScoreAnswer, UnknownDecisionEngine, YesNoAnswer,
};
pub use due::{DUE_GRACE, DueStep, PREPARE_AHEAD, UNPUBLISHED_LIFETIME, due_step, prepare_at};
pub use edit::{Edge, Edit, EditError, HISTORY_DEPTH, History, Item, ItemRef, Shift, Track};
pub use end_card::{
    CardDesign, CardFieldError, CardLayout, CardLogoPlace, CardText, CardTextAlign, CardTextPlace,
    END_CARD_MIN, END_CARD_TAIL, EndCard, EndCardRepository, MAX_CARD_HEADLINE,
    MAX_CARD_SECOND_LINE, card_fingerprint, card_room, end_card_end,
};
pub use files::{ProjectFileError, ProjectFiles};
pub use folders::{
    DefaultBase, FolderMove, FolderMoveError, FolderPlaces, FolderRepository, GLOBAL_DIR,
    MAX_PROJECT_FOLDER_CHARS, MAX_VIDEO_SLUG_CHARS, PROJECT_MANIFEST, ProjectFolders,
    ProjectManifest, VIDEOS_DIR, VideoFolder, VideoFolders, VideoToMove, WORLD_DIR,
    default_base_folder, is_synced, leaves_room, project_folder_name, safe_folder_name, short_id,
    video_folder_name,
};
pub use format::{AD_TAIL, UnknownVideoFormat, UnknownVideoLength, VideoFormat, VideoLength};
pub use framing::{
    CROP_STEPS, CropPosition, CropRect, FrameShape, Framing, PictureSize, UnknownFrameShape,
    crop_window,
};
pub use generation::{Generation, GenerationId, TemplateUsed};
pub use image::{
    GeneratedImage, ImageFormat, ImageGenerator, ImageRequest, ImageShape, ReferenceImage,
    image_declares_transparency, image_dimensions, shape_of,
};
pub use job::{
    InconsistentJob, InvalidJobTransition, Job, JobFailure, JobFailureKind, JobId, JobKind,
    JobRecord, JobRepository, JobState, Progress, RetryPolicy, UnknownJobFailureKind,
    UnknownJobKind, UnknownJobState,
};
pub use localization::{
    Adaptation, AdaptationError, CarriedAudio, CarriedCut, Gap, Localization,
    LocalizationRepository, NewVersion, PlacedScene, STRETCH_FLAG, SceneLook, SourceSentence,
    TimeMap, adaptation_gaps, join_adapted, localized_cut, place_scenes, placed_scene_notes,
    scene_sentences, sentence_times, sentence_words, source_gaps,
};
pub use loose::{LOOSE_VIDEOS_DIR, SettingDifference, VideoSettings, loose_video};
pub use market::{
    ContentLanguage, Country, Market, UnsupportedContentLanguage, UnsupportedCountry,
};
pub use media::{
    AssetSource, MediaAsset, MediaAssetId, MediaAssetRepository, MediaKind, UnknownAssetSource,
    UnknownMediaKind,
};
pub use metadata::{
    DisclosureLabel, Export, ExportFiles, ExportRepository, METADATA_FILE, MetadataProblem,
    MetadataRules, Post, TagPlacement, VideoMetadata, VideoMetadataDraft, compose, compose_with,
    fitting_chapters, normalize_tags, package_folder, problems, problems_with, text_length,
    video_file_name,
};
pub use mix::{
    AudioLane, DEFAULT_DUCK, DUCK_ATTACK, DUCK_HOLD, DUCK_RANGE, DUCK_RELEASE, Decibels, Dip,
    DuckEnvelope, Ducking, GAIN_RANGE, LaneMix, Mix,
};
pub use model_name::{
    LevelChoices, LevelPick, ModelLevel, ModelName, ModelOption, model_levels, pick_model,
};
pub use motion::{
    MOTION_VARIED, Motion, MotionDefault, MotionKind, MotionPattern, MotionStrength, MotionView,
    UnknownMotion,
};
pub use music_prompt::{MusicPrompt, MusicPromptFieldError, MusicPromptRepository};
pub use music_track::{
    ComposedMusic, MAX_MUSIC_LENGTH, MIN_MUSIC_LENGTH, MusicGenerator, MusicRequest, MusicTrack,
    MusicTrackRepository, MusicTracks, music_length,
};
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
pub use owner_metrics::{
    AnalyticsError, AnalyticsErrorKind, Earnings, Monetization, MoneyReport, OwnerAnalytics,
    OwnerMetrics, OwnerReading, ReportPeriod, RetentionCurve, RetentionPoint, Share, VideoReport,
    dollars, read_owner_metrics, rpm, whole,
};
pub use package::{
    FolderImport, PACKAGE_EXTENSION, PACKAGE_FORMAT, PROJECT_RECORDS, PackageContents,
    PackageError, PackageExport, PackageImport, PackageImported, PackageManifest, ProjectPackages,
    free_name, package_file_name, rewrite_ids,
};
pub use performance::{
    EvidenceScope, FIRST_WEEK, FIRST_WEEK_LATEST_SYNC, FIRST_WEEK_MIN_AGE, FirstWeek,
    PastPerformance, PerformanceEvidence, PerformanceReason, PublishedVideo, Standing, first_week,
};
pub use performed::{
    LENGTH_TOLERANCE, MAX_TAKE_PROMPT, PerformedClip, PreflightOverride, TakePrompt,
    TakePromptError, TakeResult, TakeStretch, take_prompt_text,
};
pub use performed_cut::{PerformedPlace, PerformedSync, scene_at_narration};
pub use persona::{
    GenerationPresets, Persona, PersonaDetails, PersonaDraft, PersonaFieldError, PersonaId,
    PersonaRepository, VoiceFlag,
};
pub use persona_look::{
    LOOK_MAX_BYTES, LOOK_MAX_RATIO_TENTHS, LOOK_MAX_SIDE, LOOK_MIN_RATIO_TENTHS, LOOK_MIN_SIDE,
    LookAsset, LookAssetError, LookAssetKind, LookDescriptionError, LookDrawing, LookFaceMeasure,
    LookImage, LookImageError, LookSource, MAX_LOOK_DESCRIPTION, MAX_LOOK_PROMPT, PersonaLook,
    UnknownLookSource, look_description,
};
pub use plan::{Credits, PlanAccount, PlanSession, PlanSignIn};
pub use post_insights::{
    Insights, InsightsReader, MEDIA_PAGES, MediaItem, MediaLookup, MediaPage, PostNumbers,
    PostReading, find_media, read_insights,
};
pub use preflight::{
    BodyFraming, HALF_BODY_FACE, LookFace, MAX_FAST_FRAMES, MAX_OUT_OF_FRAME, MIN_PREFLIGHT_FACE,
    Preflight, PreflightCheck, PreflightOutcome, PreflightResult, preflight,
};
pub use product::{ConceptDrawing, ProductConcept, ad_images, may_draw_concept};
pub use product::{
    MAX_PRODUCT_IMAGES, PRODUCT_IMAGE_MAX_BYTES, PRODUCT_IMAGE_MAX_RATIO, PRODUCT_IMAGE_MAX_SIDE,
    PRODUCT_IMAGE_MIN_SIDE, PRODUCT_LOGO_MAX_RATIO, PRODUCT_LOGO_MIN_LONG_SIDE,
    PRODUCT_LOGO_MIN_SIDE, Product, ProductBrief, ProductDraft, ProductFieldError, ProductId,
    ProductImage, ProductImageError, ProductImageId, ProductImageKind, ProductInUse, ProductKind,
    ProductLogo, ProductOwnership, ProductPicture, ProductRepository, split_facts,
};
pub use product_sheet::{
    Alternative, AudienceSegment, Claim, ClaimError, ClaimId, ClaimKind, ClaimRecord, ClaimSource,
    ClaimState, Objection, ProductSheet, RestrictedCategory, SheetClaimError, SheetFieldError,
    SheetSource, SheetTaken, SheetText, WrittenClaim, WrittenSheet, WrittenSource,
};
pub use product_shot::{
    BackdropPaint, CardRecipe, ComposedPicture, DeviceOutline, SHADOW_BLUR, SHADOW_DROP,
    SHOT_SHORT_SIDE, ShotArea, ShotComposer, ShotError, ShotLayout, ShotRecipe, shot_frame,
    shot_room,
};
pub use production::{
    AnimatedScenes, HOOK_LENGTH, NARRATION_CHARS_PER_SECOND, ProductionMode, TYPICAL_AD_SCENE,
    TYPICAL_PRODUCT_SHOTS, TYPICAL_SCENE, UnknownProductionMode, ad_scenes_on, typical_ad_scenes,
    typical_length, typical_scenes,
};
pub use profile::{ProfileId, ProfileRepository, UiLanguage, UnsupportedLanguage, UserProfile};
pub use project::{
    Project, ProjectColor, ProjectDetails, ProjectDraft, ProjectFieldError, ProjectId, ProjectList,
    ProjectRepository, ProjectState, UnknownProjectState, WorkOrigin, reorder_favorites,
};
pub use project_key::{Budgets, KeyPayer, ProjectBudget, ProjectKey, ProjectKeyRepository};
pub use provider_key::{
    ApiKey, ApiKeyError, BilledUse, KeyCheck, KeyCheckOutcome, KeyChecker, Provider,
    ProviderFailure, ProviderFailureKind, SecretStore, SecretStoreError, UnknownProvider,
};
pub use publication::{
    ChannelPoint, MetricsSnapshot, MetricsSyncOnStart, MetricsTotals, OwnerTotals, PostLink,
    PostLinkError, PostRetention, Publication, PublicationId, PublicationKind,
    PublicationRepository, STATS_BATCH, ScheduleReading, UnknownMetricsSync, VideoStatistics,
    VideoStats, channel_history, latest_of_each, sync_quota_units,
};
pub use redaction::Redactor;
pub use reel::{
    Container, LIMIT_RECHECK, Mp4Layout, NextTry, PublishingLimit, REEL_FPS, REEL_MAX_BYTES,
    REEL_MAX_DURATION, REEL_MAX_WIDTH, REEL_MIN_DURATION, ReelFile, ReelSpecProblem, check_reel,
    format_cover_time, mp4_layout, parse_cover_time,
};
pub use removal::{
    DeletedKind, DeletedWork, Discard, Disposal, PACKED_VIDEOS, PermanentReason, RemovalBlock,
    RemovalRepository, Trash, confirms_name,
};
pub use render::{
    CutFacts, Gate, GateLevel, LOUDNESS_GAIN_WARNING, LOUDNESS_TOLERANCE, MeasuredLoudness, Render,
    RenderId, RenderRepository, SILENCE, TRUE_PEAK_CEILING, cut_gates, output_gates,
};
pub use repository::RepositoryError;
pub use research::{
    MarketData, MarketSample, Niche, NicheResearch, NicheResearchRepository, NicheScores,
    NicheSeedError, NicheSeeds, NicheStatistics, Score, UploadSample, rank,
};
pub use runner::{
    Holder, JOB_LEASE, JobHeldElsewhere, RUNNER_ALIVE, RUNNER_TICK, RunnerRole, RunnerSeen,
    UnknownRunnerRole, running_since,
};
pub use scene::{
    CastDraft, CastKind, CastMember, CastRecord, MAX_CAST, MAX_CAST_NAME_CHARS, MAX_PLACES,
    MAX_PROPS, MAX_SCENE_OBJECTS, MAX_SCENE_PROPS, MAX_SENTENCE_WORDS, NoPendingClip,
    NoPendingImage, NoScenes, NoSuchCastMember, NoSuchPlace, NoSuchScene, OtherShot, OtherText,
    PRODUCT_MOTION_RULE, PRODUCT_REFERENCE_RULE, PaidPictures, Place, PlaceDraft, PlaceRecord,
    PlanDraft, PortraitsUsed, ProductCheckError, ProductChecks, Scene, SceneClip, SceneDraft,
    SceneFieldError, SceneImage, SceneObject, ScenePlan, ScenePlanId, ScenePlanRecord,
    ScenePlanRepository, ScenePrompt, SceneRecord, SceneSetting, Sentence, scene_image_prompt,
    sentences,
};
pub use scene_kind::{ComposedShot, KindChange, ProductPictures, SceneKind, SceneKindError};
pub use scene_plan_size::ScenePlanSize;
pub use schedule::{
    DateOrder, LocalTime, Rfc3339, SCHEDULE_GRACE, ScheduleProblem, Zone, check_publish_time,
    date_text, default_publish_time, local_time, parse_rfc3339, publish_time,
};
pub use script::{
    GeneratedScript, NoPendingScript, Script, ScriptFieldError, ScriptRecord, ScriptRepository,
    ScriptText,
};
pub use sharing::{
    CopyKind, Following, LibraryItem, LibraryKind, ShareableItem, Sharing, SharingRepository,
};
pub use shot_look::{
    BackdropDirection, BackdropId, DeviceKind, ProductLayout, ShotBackdrop, ShotBackdropDrawing,
    ShotColor, ShotLook, ShotPlacement,
};
pub use speech::{
    AlignedSpeech, Alignment, AlignmentRequest, ChangedSpeech, CharTiming, Speech, SpeechAligner,
    SpeechRequest, SpeechSynthesizer, SpeechTranscriber, Transcript, TranscriptionRequest,
    VoiceChangeRequest, VoiceChanger, VoiceIsolator, split_for_speech,
};
pub use staging::{StagedUpload, StagingRepository};
pub use take_analysis::{
    ANALYSIS_CHUNK_FRAMES, AnalysisPart, BLENDSHAPE_NAMES, DEPTH_FAR_PERMILLE, DEPTH_NEAR_PERMILLE,
    DepthEstimator, DepthHistogram, DepthMap, DepthRange, DepthScale, Device, EDGE_MARGIN,
    FACE_FILE_VERSION, FACE_LANDMARKS, FaceFileError, FaceFrame, FaceLandmarks, FaceMeasures,
    FaceMesh, FaceObservation, FaceSource, FaceTracker, MASK_BACKGROUND, MASK_FACE_SKIN,
    MAX_FACE_SPEED, MAX_HEAD_TURN_DEGREES, MAX_MASK_CHANGE, MIN_FACE_HEIGHT, ModelFile, ModelInfo,
    PersonMask, PersonSegmenter, REGION_COUNT_EVERY, Region, RgbFrame, TakeAnalysis, TakeChecks,
    TakeMode, VisionError, VisionModels, analysis_fps, distinct_faces, take_frame_time,
    take_frames,
};
pub use take_model::{
    DEFAULT_TAKE_CAP, MIN_TAKE_CAP, StagedFile, TEST_TAKE_LENGTH, TakeDone, TakeGenerator,
    TakeInput, TakeModel, TakeRequest, TakeSpend, TakeStatus, TakeStep, TakeTier, take_model_of,
};
pub use template::{
    MissingValue, RenderedPrompt, TemplateBody, TemplateField, TemplateFieldError, TemplateKind,
    TemplateProblem, TemplateRepository, TemplateSource, TemplateValues, TemplateVariable,
    TemplateVersion, TemplateVersionId, UnknownTemplateKind, template_in_use,
};
pub use text::{
    GeneratedText, Stop, TextEffort, TextFormat, TextGenerator, TextImage, TextRequest, TokenUsage,
};
pub use theme::{
    DEFAULT_LONG_CAP, DEFAULT_SHORT_CAP, Reason, SpendCap, Theme, ThemeFieldError, ThemeId,
    ThemeIdea, ThemeNotSuggested, ThemeRanking, ThemeRecord, ThemeRepository, ThemeStatus,
    UnknownSpendCap, UnknownThemeStatus, VideoProject, VideoProjectId, default_spend_cap,
    rank_themes,
};
pub use thumbnail::{
    BackgroundSource, DrawnBackground, MAX_THUMBNAIL_BYTES, MAX_THUMBNAIL_TEXT, THUMBNAIL_HEIGHT,
    THUMBNAIL_WIDTH, TextPlacement, Thumbnail, ThumbnailBackground, ThumbnailDesign,
    ThumbnailFieldError, ThumbnailLook, ThumbnailRepository, UnknownBackgroundSource,
    UnknownTextPlacement, UnknownThumbnailLook, thumbnail_text,
};
pub use tiktok::{
    ChunkPlan, ChunkPlanError, DraftRoom, TIKTOK_CHUNK, TIKTOK_DRAFT_WINDOW, TIKTOK_FPS,
    TIKTOK_MAX_BYTES, TIKTOK_MAX_CHUNK, TIKTOK_MAX_CHUNKS, TIKTOK_MAX_DURATION,
    TIKTOK_MAX_LAST_CHUNK, TIKTOK_MIN_CHUNK, TIKTOK_PENDING_DRAFTS, TIKTOK_SIDE, TikTokFile,
    TikTokSpecProblem, VideoContainer, check_tiktok, draft_room, video_container,
};
pub use timeline::{
    AudioItem, CaptionSpan, FPS, Picture, SavedAudioItem, SavedTimeline, SavedVideoItem, Timeline,
    TimelineRepository, VideoItem, VideoSource, frame_at, frame_time, min_length, nearest_frame,
    snap, timecode,
};
pub use tour::{TourId, TourProgress, TourProgressRepository, TourState};
pub use transition::{PlayedTransition, Transition, TransitionKind, UnknownTransition};
pub use update::{
    DownloadError, InstallerRunner, InstallerStore, InvalidVersion, PreRelease, Release,
    ReleaseAsset, ReleaseFeed, UpdateCheckError, Version, checksum_for, installer_name,
    installer_version, is_sha256,
};
pub use upload::{
    InvalidUploadTransition, NetworkPost, ScheduleChange, ScheduleOutcome, ThumbnailOutcome,
    UPLOAD_CHUNK, UPLOAD_CHUNK_UNIT, Upload, UploadError, UploadErrorKind, UploadFailure,
    UploadOutcome, UploadRun, UploadStatus, UploadedVideo, VideoState, VideoUpload, VideoUploader,
};
pub use visual_style::{StyleChoice, VisualStyle};
pub use voice::{InvalidVoiceRef, Voice, VoiceCategory, VoiceLibrary, VoicePreviews, VoiceRef};
pub use voice_sample::{
    InvalidSampleText, SampleSource, SampleText, VoiceSampleStore, VoiceSampleStoreError,
};
pub use web_research::{PageRead, ResearchLimits, ResearchNotes, ResearchRequest, WebResearcher};
pub use world::{
    Look, MAX_OBJECTS, MAX_PICKED, MAX_PICKED_PLACES, PictureOrigin, PictureProvenance,
    ReferencePicture, ReferencePictureId, WorldFiles, WorldItem, WorldItemDetails, WorldItemDraft,
    WorldItemFieldError, WorldItemId, WorldItemUse, WorldKind, WorldPick, WorldRepository,
    same_name,
};
pub use youtube_policy::{
    Forgotten, KEEP_AT_MOST, PRIVACY_POLICY_VERSION, PrivacyConsent, REFRESH_AFTER, UPKEEP_EVERY,
    YouTubeDataStore, YouTubeRetention, authorization_current, confirmed_at, keep_since,
};
