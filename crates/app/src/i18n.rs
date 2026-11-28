//! UI strings, loaded from one resource file per language in `locales/`.

use std::borrow::Cow;
use std::collections::BTreeMap;

use std::time::Duration;

use crate::{AdAnglesMissing, Destination, GuideGroup, NavEntry, NavSection, Stage, UploadBlock};

use bardo_domain::ModelLevel;
use bardo_domain::ProductionMode;
use bardo_domain::ProjectColor;
use bardo_domain::SceneKind;
use bardo_domain::VideoLength;
use bardo_domain::{AdAngleFieldError, AdFramework, BriefFieldError, BriefNotReady};
use bardo_domain::{
    AnalysisPart, ApiKeyError, AppCredentialsFieldError, AspectRatio, BackdropDirection,
    CaptionHighlight, CaptionStyle, CaptureProblem, ChannelFieldError, ContentLanguage, Country,
    DateOrder, Decibels, DecisionEngineId, JobFailureKind, JobKind, JobState, KeyCheckOutcome,
    LayoutId, LocalTime, LookDescriptionError, LookImageError, MetadataProblem, Meter,
    MetricsSyncOnStart, Money, MoneyError, Month, MotionKind, MotionStrength,
    MusicPromptFieldError, Network, NetworkAccountFieldError, NicheSeedError, Outbound,
    PastedTokenError, PersonaFieldError, PostLinkError, ProductLayout, Provider, RateFieldError,
    SceneFieldError, ScheduleProblem, Score, ScriptFieldError, Share, ShotPlacement,
    SignInFailureKind, TakeMode, TemplateKind, TemplateProblem, TemplateVariable, ThemeFamily,
    ThemeFieldError, ThemeMode, TourId, TransitionKind, UiLanguage, UiTheme, VideoFormat,
    Visibility, VoiceCategory, VoiceFlag,
};
use bardo_domain::{
    BytePlusField, BytePlusFieldError, ChapterFieldError, ChapterProblem, LookAssetError,
    LookAssetKind, TextPlacement, ThumbnailFieldError, ThumbnailLook,
};
use bardo_domain::{CardFieldError, CardLogoPlace, CardTextPlace};
use bardo_domain::{ClaimError, ClaimKind, RestrictedCategory, SheetClaimError, SheetFieldError};
use bardo_domain::{ClipFit, StyleChoice, VisualStyle};
use bardo_domain::{CommercialDisclosure, SponsorError};
use bardo_domain::{PictureOrigin, WorldItemFieldError};
use bardo_domain::{
    ProductFieldError, ProductImageError, ProductImageKind, ProductKind, ProductOwnership,
};

/// Every string the UI shows. Adding a variant without adding its key to all
/// resource files fails the catalog tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    AppName,
    AppTagline,
    UiLanguageLabel,
    LanguageName(UiLanguage),
    LanguageNotSaved,
    ContentLanguageName(ContentLanguage),
    CountryName(Country),
    ChannelsTitle,
    ChannelsEmpty,
    ChannelsNotLoaded,
    NewChannel,
    NewChannelTitle,
    EditChannelTitle,
    ChannelName,
    ChannelNamePlaceholder,
    ChannelNiche,
    ChannelNichePlaceholder,
    ChannelThemes,
    ChannelThemesPlaceholder,
    ChannelThemesHint,
    ChannelAestheticNotes,
    ChannelAestheticNotesPlaceholder,
    ChannelLanguage,
    ChannelCountry,
    ChannelCountrySearch,
    CreateChannel,
    SaveChannel,
    ChannelSaved,
    ChannelNameTaken,
    ChannelNotFound,
    ChannelNotSaved,
    ChannelFieldError(ChannelFieldError),
    OverviewHint,
    OverviewNotLoaded,
    OverviewNoProject,
    OverviewPickUp,
    OverviewNoVideos,
    OverviewContinue,
    OverviewOpen,
    OverviewNextLabel,
    OverviewNextWithCost,
    OverviewWorking,
    OverviewWaitingAt,
    OverviewWaiting,
    OverviewIdeasTitle,
    OverviewIdeasNone,
    OverviewIdeasUnranked,
    OverviewIdeasRanked,
    OverviewIdeasOrphans,
    OverviewOpenIdeas,
    OverviewChannels,
    OverviewSpent,
    OverviewNarrators,
    OverviewNoNarrator,
    OverviewAd,
    StepConfirmBrief,
    StepDecideClaims,
    StepCompleteBrief,
    StepWriteScript,
    StepReviewScript,
    StepRewriteScript,
    StepRecordNarration,
    StepRecordAgain,
    StepFixTakes,
    StepPlanScenes,
    StepPlaceScenes,
    StepPlanAgain,
    StepReviewImages,
    StepDrawImages,
    StepReviewClips,
    StepAnimateClips,
    StepBuildCut,
    StepRender,
    StepRenderAgain,
    StepResumeRender,
    StepExport,
    StepExportAgain,
    StepExportRest,
    StepResumeExport,
    StepPost,
    LibraryHint,
    CostsUnknownProject,
    LooseNotSaved,
    LooseVideoNotFound,
    LooseMoveBusy,
    LooseMoveArchived,
    LooseMoveVideoArchived,
    DeleteVideoNoIdeaHint,
    ChannelNeedsProject,
    LooseSwitcherRow,
    LooseCountNone,
    LooseCountOne,
    LooseCountMany,
    LooseTitle,
    LooseHint,
    LooseNew,
    LooseFormTitle,
    LooseFormHint,
    LooseIdeaTitle,
    LooseIdeaTitlePlaceholder,
    LooseIdeaAngle,
    LooseIdeaAnglePlaceholder,
    LooseCopyFrom,
    LooseCopyDefaults,
    LooseCreate,
    LooseCancel,
    LooseEmpty,
    LooseExportOnly,
    LooseFileTarget,
    LooseMove,
    LooseMoveTitle,
    LooseMoveHint,
    LooseMoveChannel,
    LooseMoveConfirm,
    LooseMoveNoChannels,
    LooseMoved,
    LooseDiffersLanguage,
    LooseDiffersCountry,
    LooseOverview,
    LooseOpenVideos,
    OwnIdea,
    OwnIdeaTitle,
    OwnIdeaHint,
    OwnIdeaChannel,
    OwnIdeaSave,
    OwnIdeaCancel,
    OwnIdeaSaved,
    DeleteLooseVideoHint,
    SwitcherNoProject,
    SwitcherSearch,
    SwitcherFavorites,
    SwitcherRecent,
    SwitcherAll,
    SwitcherNewProject,
    SwitcherStar,
    SwitcherUnstar,
    SwitcherMoveUp,
    SwitcherNoMatch,
    SwitcherEmpty,
    SwitcherOpen,
    ProjectChannelsNone,
    ProjectChannelsOne,
    ProjectChannelsMany,
    ProjectNeverOpened,
    NewProjectTitle,
    NewProjectHint,
    ProjectNameLabel,
    ProjectNamePlaceholder,
    ProjectColorLabel,
    CreateProject,
    ProjectFormCancel,
    ProjectNameRequired,
    ProjectNameTooLong,
    ProjectNameTaken,
    ProjectNotFoundAny,
    ProjectNotSaved,
    ProjectsNotLoaded,
    MoveChannel,
    MoveChannelTitle,
    MoveChannelHint,
    MoveChannelTarget,
    MoveChannelConfirm,
    MoveChannelDone,
    MoveChannelBlocked,
    MoveChannelNoOther,
    ChannelProjectLabel,
    FilterAllProjects,
    FilterNoProject,
    FilterProjectLabel,
    CostsByProject,
    ProjectColorName(ProjectColor),
    MoveChannelBlock(crate::MoveBlock),
    // Archive and delete (#336).
    RemovalNotFound,
    RemovalBlocked,
    RemovalWrongState,
    RemovalNotSaved,
    JobWaitsArchived,
    DeleteNameMismatch,
    DeleteFolderNotRemoved,
    DeleteNotDisconnected,
    SwitcherArchiveProject,
    SwitcherDeleteProject,
    /// Placeholders: `{n}`.
    SwitcherArchivedProjects,
    SwitcherBack,
    /// Placeholders: `{name}`.
    ArchiveProjectTitle,
    ArchiveProjectHint,
    ArchiveCompress,
    ArchiveCompressHint,
    ArchiveConfirm,
    /// Placeholders: `{name}`.
    ArchiveVideoTitle,
    ArchiveVideoHint,
    ArchivedProjectsTitle,
    ArchivedProjectsEmpty,
    ArchivedProjectsHint,
    ArchivedPacked,
    ArchivedBusy,
    Unarchive,
    UnarchiveUnpacking,
    DeleteAction,
    /// Placeholders: `{name}`.
    DeleteProjectTitle,
    /// Placeholders: `{name}`.
    DeleteVideoTitle,
    /// Placeholders: `{videos}`, `{connected}`.
    DeleteProjectHint,
    DeleteVideoHint,
    /// Placeholders: `{size}`, `{folder}`.
    DeleteFolderRecycle,
    /// Placeholders: `{size}`, `{folder}`, `{reason}`.
    DeleteFolderPermanent,
    DeleteNoFolder,
    /// Placeholders: `{name}`.
    DeleteTypeName,
    DeletePermanently,
    DeletePermanentlyHint,
    DeleteConfirm,
    DeleteRunning,
    VideosShowArchived,
    VideosArchivedEmpty,
    ArchiveVideoAction,
    ArchivedTag,
    VideoArchivedNotice,
    DeletedProjects,
    /// Placeholders: `{name}`, `{date}`.
    DeletedOn,
    /// Placeholders: `{id}`.
    DeletedProjectForgotten,
    /// Placeholders: `{id}`.
    DeletedVideoForgotten,
    ForgetName,
    ForgetNameHint,
    ThemeStartAgain,
    ThemeNoVideo,
    RemovalBlock(bardo_domain::RemovalBlock),
    PermanentReasonText(bardo_domain::PermanentReason),
    ChannelPersona,
    ChannelPersonaNone,
    ChannelPersonaHint,
    ChannelPersonaNotFound,
    ChannelClipModel,
    /// Placeholder: `{model}`.
    ChannelClipModelDefault,
    /// The settings bar's groups on an open project (#330).
    ProjectSettingsVideo,
    ProjectSettingsProduction,
    ChannelClipModelHint,
    ChannelTakeModel,
    ChannelTakeModelHint,
    ChannelCaptionStyle,
    ChannelCaptionStyleHint,
    ChannelCaptionHighlight,
    ChannelCaptionHighlightHint,
    ChannelFormat,
    ChannelFormatHint,
    ChannelTransition,
    ChannelTransitionHint,
    ChannelTransitionLength,
    ChannelMotion,
    ChannelMotionHint,
    ChannelMotionStrength,
    ChannelClipModelNotOffered,
    ChannelTakeModelNotOffered,
    ChannelAccountsTitle,
    ChannelAccountsHint,
    ChannelAccountsSaveFirst,
    ChannelAccountsEmpty,
    ChannelAccountsNotLoaded,
    AllNetworksAdded,
    /// Placeholder: `{network}`.
    AddNetworkAccount,
    /// Placeholder: `{network}`.
    NewNetworkAccountTitle,
    /// Placeholder: `{network}`.
    EditNetworkAccountTitle,
    AccountHandle,
    AccountHandlePlaceholder,
    AccountMetadataTitle,
    AccountMetadataHint,
    AccountLanguage,
    /// Placeholder: `{language}`.
    AccountLanguageChannel,
    AccountTags,
    AccountTagsPlaceholder,
    AccountTagsHint,
    AccountFooter,
    AccountFooterPlaceholder,
    AccountVisibility,
    /// Placeholder: `{network}`.
    AccountVisibilityOnlyPublic,
    RenderPresetTitle,
    RenderPresetHint,
    RenderPresetAspect,
    RenderPresetResolution,
    RenderPresetCodec,
    RenderPresetBitrate,
    RenderPresetMaxDuration,
    RenderPresetMaxDurationHint,
    RenderPresetLoudness,
    /// Placeholder: `{value}`.
    RenderPresetNetworkDefault,
    /// Placeholders: `{aspect}`, `{width}`, `{height}`, `{codec}`, `{bitrate}`, `{duration}`, `{loudness}`.
    RenderPresetSummary,
    RenderPresetCustom,
    RenderPresetDefault,
    /// Placeholder: `{summary}`.
    RenderPresetEffective,
    CreateNetworkAccount,
    SaveNetworkAccount,
    CancelNetworkAccount,
    EditNetworkAccount,
    RemoveNetworkAccount,
    /// Placeholder: `{network}`.
    NetworkAccountRemoveConfirm,
    ConfirmRemoveNetworkAccount,
    KeepNetworkAccount,
    NetworkAccountSaved,
    NetworkAccountRemoved,
    NetworkAccountTaken,
    NetworkAccountNotFound,
    NetworkAccountNotSaved,
    NetworkName(Network),
    VisibilityName(Visibility),
    AspectRatioName(AspectRatio),
    CaptionStyleName(CaptionStyle),
    CaptionHighlightName(CaptionHighlight),
    TransitionName(TransitionKind),
    VideoFormatName(VideoFormat),
    /// The format in one word, where the full name does not fit.
    VideoFormatBrief(VideoFormat),
    /// How long a video of the format runs, as a range.
    VideoFormatLength(VideoFormat),
    VideoLengthName(VideoLength),
    /// No transition: a hard cut.
    TransitionCut,
    MotionName(MotionKind),
    /// No motion: the still stays still.
    MotionNone,
    /// A different motion from one scene to the next.
    MotionVaried,
    MotionStrengthName(MotionStrength),
    NetworkAccountFieldError(NetworkAccountFieldError),
    PersonasTitle,
    PersonasHint,
    PersonasEmpty,
    PersonasNotLoaded,
    NewPersona,
    NewPersonaTitle,
    EditPersonaTitle,
    PersonaName,
    PersonaNamePlaceholder,
    PersonaVoice,
    PersonaVoiceNone,
    PersonaVoiceHint,
    ChooseVoice,
    ReloadVoices,
    HideVoices,
    LoadingVoices,
    VoicesTitle,
    VoicesEmpty,
    VoicesFailed,
    VoicesMissingKey,
    VoiceCategoryName(VoiceCategory),
    VoiceAvailable,
    VoiceMissing,
    VoiceSelected,
    VoicePreviewPlay,
    VoicePreviewStop,
    VoicePreviewNone,
    /// Placeholder: `{voice}`.
    VoicePreviewNote,
    VoiceSampleTitle,
    VoiceSampleHint,
    VoiceSampleSentence,
    VoiceSamplePlay,
    VoiceSampleStop,
    VoiceSampleReading,
    VoiceSampleFree,
    /// Placeholder: `{n}`.
    VoiceSampleBilled,
    VoiceSampleNoVoice,
    VoiceSampleMissingKey,
    VoiceSampleTooLong,
    VoiceSampleFailed,
    VoiceSampleNotKept,
    VoiceSampleCannotPlay,
    PersonaTone,
    PersonaTonePlaceholder,
    PersonaScriptStyle,
    PersonaScriptStylePlaceholder,
    PersonaPresets,
    PersonaPresetsHint,
    PresetStability,
    PresetStabilityHint,
    PresetSimilarity,
    PresetSimilarityHint,
    PresetStyle,
    PresetStyleHint,
    PresetSpeed,
    PresetSpeedHint,
    /// Placeholder: `{n}`.
    PresetPercent,
    CreatePersona,
    SavePersona,
    DuplicatePersona,
    PersonaSaved,
    /// Placeholder: `{name}`.
    PersonaDuplicated,
    /// Placeholder: `{channels}`.
    PersonaUsedBy,
    PersonaUnused,
    /// Placeholder: `{n}`.
    PersonaConfirmTitle,
    PersonaConfirmHint,
    ConfirmPersonaSave,
    CancelPersonaSave,
    PersonaNameTaken,
    PersonaNotFound,
    PersonaNotSaved,
    /// Appended to a duplicated persona's name, e.g. `Narrator (copy)`.
    PersonaCopySuffix,
    /// Appended to an imported persona's name when the name is taken.
    PersonaImportedSuffix,
    PersonaFieldError(PersonaFieldError),
    ImportPersona,
    ImportPersonaDialog,
    ExportPersona,
    PersonaExportHint,
    /// Placeholder: `{path}`.
    PersonaExported,
    /// Placeholder: `{name}`.
    PersonaImported,
    VoiceFlagTag(VoiceFlag),
    VoiceFlagExplanation(VoiceFlag),
    CheckVoices,
    PersonaPackageUnreadable,
    PersonaPackageNotOne,
    PersonaPackageNewer,
    PersonaPackageInvalid,
    PersonaExportFailed,
    FileDialogFailed,
    ProjectNarrator,
    /// Placeholder: `{name}`.
    ProjectNarratorChannel,
    ProjectNarratorChannelNone,
    ProjectNarratorHint,
    ProjectFormat,
    /// Placeholder: `{format}`.
    ProjectFormatChannel,
    ProjectFormatHint,
    ProjectLength,
    /// Placeholder: `{length}`.
    ProjectLengthFormat,
    ProjectLengthHint,
    ProductionModeName(ProductionMode),
    /// One sentence on what the mode makes.
    ProductionModeAbout(ProductionMode),
    /// A visual style of the catalog, or the channel's notes (#259).
    VisualStyleName(StyleChoice),
    ChannelStyle,
    ChannelStyleHint,
    ProjectStyle,
    /// Placeholder: `{style}`.
    ProjectStyleChannel,
    /// The channel's choice when it is its notes.
    ProjectStyleChannelNotes,
    ProjectStyleHint,
    /// Placeholder: `{n}`.
    ScenesRestyled,
    /// Placeholder: `{style}`.
    ScenesRestyledHint,
    RedrawRestyled,
    /// How many scenes show a person as an older portrait did, or were
    /// drawn without one (#269). Placeholder: `{n}`.
    ScenesOldPortraits,
    ScenesOldPortraitsHint,
    RedrawOldPortraits,
    /// On such a scene.
    SceneOldPortrait,
    /// On such a scene's card.
    SceneCardOldPortrait,
    ChannelProduction,
    ChannelProductionHint,
    /// How clips are asked of a model of fixed lengths (#260).
    ChannelClipFit,
    ChannelClipFitHint,
    ClipFitName(ClipFit),
    /// How far a clip is stretched (`StretchLimit`).
    ChannelStretchLimit,
    ChannelStretchLimitHint,
    /// One limit in the menu. Placeholders: `{percent}`, `{seconds}`
    /// (what 10 seconds of clip cover).
    StretchLimitChoice,
    /// After the next clip's length and price, when it would play slowed.
    SceneClipSlowedNext,
    /// On a clip made to play slowed.
    SceneClipSlowed,
    SceneClipSlowedHint,
    /// Placeholders: `{recorded}`, `{played}`.
    EditorSourceSlowed,
    ProjectMode,
    /// Placeholder: `{mode}`.
    ProjectModeChannel,
    ProjectModeHint,
    /// Placeholders: `{mode}`, `{amount}`.
    ProjectModeOption,
    /// Placeholder: `{provider}`.
    ProjectModeMissingKey,
    ProjectModeMissingSignIn,
    /// Placeholders: `{total}`, `{spent}`.
    ProjectEstimate,
    /// Placeholders: `{total}`, `{spent}`.
    ProjectEstimatePartial,
    SpendCap,
    SpendCapHint,
    SpendCapPlaceholder,
    SaveSpendCap,
    SpendCapSaved,
    SpendCapInvalid,
    /// Placeholder: `{cap}`.
    SpendCapOver,
    /// Marks the cap as the default of the video's format.
    SpendCapDefault,
    /// Goes back to the default cap. Placeholder: `{cap}`.
    SpendCapUseDefault,
    /// Offers the default cap to a video from before it. Placeholder:
    /// `{cap}`.
    SpendCapOffer,
    /// Placeholder: `{cap}`.
    SpendCapOfferAccept,
    SpendCapOfferDecline,
    SpendCapKept,
    CapReachedTitle,
    /// Placeholders: `{spent}`, `{cap}`, `{amount}`, `{rest}`.
    CapReachedLine,
    ClipsEconomy,
    /// Placeholder: `{n}`.
    AnimateHookClips,
    JobsTitle,
    JobsEmpty,
    JobsRunning,
    JobsQueued,
    JobsFailed,
    JobsFinished,
    JobKindName(JobKind),
    JobStateName(JobState),
    JobFailureKindName(JobFailureKind),
    /// Placeholders: `{attempt}`, `{max}`.
    JobRetryScheduled,
    /// Placeholder: `{attempts}`.
    JobFailedAfter,
    CancelJob,
    RetryJob,
    JobNotUpdated,
    JobNotStarted,
    TestJobsTitle,
    TestJobsHint,
    StartTestJob,
    StartFailingTestJob,
    SettingsTitle,
    ProviderKeysTitle,
    /// A card's button to its setup screen.
    SetupStepByStep,
    /// Placeholder: `{name}`, the provider or the network's app.
    SetupTitle,
    /// Placeholder: `{tab}`.
    SetupBack,
    SetupPasteHere,
    SetupOpenInGuide,
    /// A network app card's button to its audits and reviews.
    SetupAudits,
    /// Placeholder: `{network}`.
    SetupAuditTitle,
    /// Copies the steps' block for a form on the network's site.
    SetupCopyBlock,
    SetupCopied,
    ProviderKeysHint,
    ProviderName(Provider),
    /// A level of a model picker: Provider, Model, Version, Variant (#329).
    ModelLevelName(ModelLevel),
    BytePlusFieldLabel(BytePlusField),
    BytePlusFieldError(BytePlusFieldError),
    BytePlusFieldsInvalid,
    BytePlusCardNote,
    ProviderPurpose(Provider),
    ProviderKeyPlaceholder(Provider),
    KeyNotSet,
    /// Placeholder: `{hint}`.
    KeySaved,
    KeyUnreadable,
    SaveKey,
    ReplaceKey,
    TestKey,
    TestingKey,
    RemoveKey,
    KeyCheckOutcome(KeyCheckOutcome),
    /// Placeholder: `{detail}`.
    KeyCheckDetail,
    ApiKeyError(ApiKeyError),
    ProviderKeyNotSet,
    ProviderKeyStoreFailed,
    ResearchTitle,
    ResearchNoChannels,
    ResearchChannel,
    ResearchSeeds,
    ResearchSeedsPlaceholder,
    ResearchSeedsHint,
    RunResearch,
    RefreshResearch,
    /// Placeholder: `{units}`.
    RefreshResearchCost,
    /// Placeholder: `{units}`.
    ResearchCost,
    ResearchCostNone,
    ResearchMissingKey,
    ResearchNotStarted,
    ResearchNotLoaded,
    ResearchRunning,
    ResearchStopped,
    NicheSeedError(NicheSeedError),
    ResearchResultsTitle,
    ResearchScoresHint,
    ResearchOpportunity,
    ResearchCompetition,
    ResearchTrend,
    ResearchUploads,
    ResearchMedianViews,
    ResearchViewsPerDay,
    ResearchMedianSubscribers,
    ResearchSmallChannels,
    /// Placeholders: `{videos}`, `{channels}`.
    ResearchSample,
    ResearchFetched,
    ResearchNotFetched,
    ResearchNoUploads,
    ResearchStale,
    ThemesTitle,
    ThemesNoChannels,
    ThemesChannel,
    ThemesNiche,
    ThemesNoNiche,
    /// Placeholder: `{n}`.
    SuggestThemesHint,
    SuggestThemes,
    RankThemes,
    /// Placeholder: `{n}`.
    ThemesUnranked,
    ThemesRunning,
    ThemesStopped,
    ThemesListTitle,
    ThemesRankingHint,
    ThemesEmpty,
    /// Placeholder: `{n}`.
    ThemesDiscarded,
    ThemePriority,
    ThemeConfidence,
    ThemeFit,
    ThemeTrend,
    ThemeCompetition,
    ThemePerformance,
    ThemePerformanceNiche,
    ThemePerformanceNicheOne,
    ThemePerformanceChannel,
    ThemePerformanceChannelOne,
    ThemePerformanceProjected,
    ThemesNoHistory,
    ThemesBeforeHistory,
    ThemeNotRanked,
    /// Placeholder: `{model}`.
    ThemeRankedBy,
    ThemeTitle,
    ThemeAngle,
    EditTheme,
    SaveTheme,
    CancelThemeEdit,
    DiscardTheme,
    ApproveTheme,
    ThemeApproved,
    ProjectsTitle,
    ProjectsEmpty,
    /// Placeholder: `{title}`.
    ProjectStarted,
    ThemeNotSaved,
    ThemeFieldError(ThemeFieldError),
    ThemesPickNiche,
    ThemeNotFound,
    /// Only Claude and TypeSafe have a message.
    ThemesMissingKey(Provider),
    ThemesBusy,
    ThemesNothingToRank,
    ThemesNotLoaded,
    /// The top bar's short name for the video projects screen.
    ProjectsNav,
    ProjectsNoChannels,
    ProjectListNotLoaded,
    ProjectFolderTooDeep,
    ProjectFolderNotMade,
    ProjectFolderLabel,
    ProjectFolderChoose,
    ProjectFolderHint,
    JobWaitsForFolder,
    FolderNotFound,
    FolderNotFoundHint,
    WorldNotFound,
    WorldNameTaken,
    CharacterInUse,
    WorldNotSaved,
    WorldVideoBusy,
    WorldHint,
    WorldEmpty,
    WorldNotLoaded,
    WorldNew,
    WorldShowArchived,
    WorldTabCharacters,
    WorldTabPlaces,
    WorldTabObjects,
    WorldNewPlace,
    WorldNewObject,
    WorldEmptyPlaces,
    WorldEmptyObjects,
    PlaceNewTitle,
    PlaceEditTitle,
    PlaceFormHint,
    PlaceNamePlaceholder,
    PlaceAppearance,
    PlaceAppearancePlaceholder,
    PlaceAppearanceHint,
    PlaceAlwaysPlaceholder,
    PlaceNeverPlaceholder,
    PlaceCreate,
    ObjectNewTitle,
    ObjectEditTitle,
    ObjectFormHint,
    ObjectNamePlaceholder,
    ObjectAppearancePlaceholder,
    ObjectAppearanceHint,
    ObjectAlwaysPlaceholder,
    ObjectNeverPlaceholder,
    ObjectCreate,
    ObjectBelongsTo,
    ObjectBelongsToNobody,
    ObjectBelongsToHint,
    WorldNoProject,
    WorldOwnerProject,
    WorldOwnerGlobal,
    CharacterNewTitle,
    CharacterEditTitle,
    CharacterFormHint,
    CharacterName,
    CharacterNamePlaceholder,
    CharacterAppearance,
    CharacterAppearancePlaceholder,
    CharacterAppearanceHint,
    CharacterAlways,
    CharacterAlwaysPlaceholder,
    CharacterNever,
    CharacterNeverPlaceholder,
    CharacterNotesHint,
    CharacterCreate,
    CharacterSave,
    CharacterDuplicate,
    CharacterArchive,
    CharacterRestore,
    CharacterDelete,
    CharacterDeleteTitle,
    CharacterDeleteHint,
    CharacterConfirmDelete,
    CharacterCancel,
    CharacterActionsHint,
    CharacterSaved,
    CharacterDuplicated,
    CharacterArchived,
    CharacterRestored,
    CharacterDeleted,
    CharacterArchivedTag,
    CharacterDropsTitle,
    CharacterDropsPictures,
    CharacterDropsNone,
    CharacterDropsConfirm,
    CharacterUses,
    CharacterUsesNone,
    CharacterUseArchived,
    CharacterCopySuffix,
    CharacterBelongings,
    CharacterBelongingsNone,
    WorldPicturesTitle,
    WorldPicturesHint,
    WorldPicturesSaveFirst,
    WorldPicturesNone,
    WorldPictureLook,
    WorldPictureLookNotes,
    WorldPictureDraw,
    WorldPictureDrawing,
    WorldPictureWaiting,
    WorldPictureKeep,
    WorldPictureDiscard,
    WorldPictureRemove,
    WorldPictureRemoveAsk,
    WorldPictureKept,
    WorldPictureDiscarded,
    WorldPictureRemoved,
    WorldPictureStarted,
    WorldPictureBusy,
    WorldPictureNothingWaiting,
    WorldPictureMissingKey,
    WorldPictureOverBudget,
    WorldImportNotConfirmed,
    WorldImportUnreadable,
    WorldPictureImport,
    WorldImportTitle,
    WorldImportHint,
    WorldImportConfirm,
    WorldImportChoose,
    WorldImportDialog,
    WorldImported,
    WhoAppearsTitle,
    WhoAppearsHint,
    WhoAppearsNone,
    WhoAppearsNoneAvailable,
    WhoAppearsPickable,
    WhoAppearsAdd,
    WhoAppearsRemove,
    WhoAppearsInstruction,
    WhoAppearsInstructionPlaceholder,
    WhoAppearsSave,
    WhoAppearsSaved,
    WhoAppearsChanged,
    WhoAppearsFull,
    WhoAppearsCharacters,
    WhoAppearsPlaces,
    WhoAppearsObjects,
    WhoAppearsBelongings,
    WhoAppearsBelongsTo,
    WhoAppearsFullPlaces,
    WhoAppearsFullObjects,
    ScenesPlaceSaveProject,
    ScenesPlaceSaveGlobal,
    ScenesPlaceSaveHint,
    ScenesPlaceFromWorld,
    WhoAppearsTooMany,
    WhoAppearsInstructionTooLong,
    CastSaveToWorld,
    CastSaveToLibrary,
    CastSaveHint,
    CastFromWorld,
    WorldItemFieldError(WorldItemFieldError),
    WorldPictureOrigin(PictureOrigin),
    LocateFolder,
    LocateFolderPrompt,
    ShowFolder,
    SettingsStorageTab,
    BaseFolderLabel,
    BaseFolderHint,
    BaseFolderChange,
    BaseFolderPrompt,
    BaseFolderUseDefault,
    BaseFolderIsDefault,
    BaseFolderAvoidedSync,
    BaseFolderSynced,
    BaseFolderNotSaved,
    StorageMoveHint,
    LocateRefused(crate::LocateError),
    PackageExportBusy,
    PackageNotAPackage,
    PackageNewer,
    PackageDamaged,
    PackageFolderNotEmpty,
    PackageNotRead,
    OpenFolderNotAProject,
    OpenFolderOtherCopy,
    OpenFolderNoRecords,
    DeleteRecordsNotWritten,
    SwitcherExportProject,
    SwitcherExporting,
    SwitcherImportPackage,
    SwitcherOpenFolder,
    PackageImportPrompt,
    OpenFolderPrompt,
    PackageExportStarted,
    PackageImportStarted,
    FolderRelinked,
    FolderReopened,
    FolderOpenedArchived,
    ImportPackageTitle,
    ImportPackageHint,
    ImportPackageFrom,
    ImportPackageContents,
    ImportPackageTemplates,
    ImportPackageTemplatesHint,
    ImportPackageConfirm,
    ProjectNotFound,
    ScriptTitle,
    ScriptEmpty,
    GenerateScript,
    /// Placeholder: `{n}`, the template version.
    GenerateScriptHint,
    RegenerateScript,
    RegenerateScriptHint,
    ScriptRunning,
    ScriptStopped,
    SaveScript,
    RevertScript,
    ScriptSaved,
    ScriptEdited,
    /// Placeholder: `{n}`.
    ScriptWords,
    ScriptCurrent,
    ScriptPendingTitle,
    ScriptPendingHint,
    AcceptScript,
    RejectScript,
    ProvenanceProvider,
    ProvenanceModel,
    ProvenanceTemplate,
    /// Placeholder: `{n}`.
    ProvenanceTemplateVersion,
    ProvenanceTokens,
    /// Placeholders: `{input}`, `{output}`.
    ProvenanceTokensValue,
    ProvenanceGenerated,
    ShowPrompt,
    HidePrompt,
    PromptInstructions,
    PromptTask,
    ScriptMissing,
    ScriptNothingToReview,
    ScriptMissingKey,
    ScriptBusy,
    ScriptNotSaved,
    ScriptFieldError(ScriptFieldError),
    VersionsTitle,
    VersionsHint,
    /// Placeholders: `{language}`, `{country}`, `{narrator}`.
    VersionTargetLine,
    /// Placeholders: `{language}`, `{country}`.
    VersionTargetNoNarrator,
    MakeVersion,
    /// Placeholder: `{n}` (template version).
    MakeVersionHint,
    /// Placeholder: `{channel}`.
    VersionMaking,
    VersionStopped,
    VersionsMade,
    /// Placeholders: `{channel}`, `{language}`, `{country}`.
    VersionMadeLine,
    OpenVersion,
    /// Placeholders: `{title}`, `{language}`, `{country}`.
    VersionOf,
    /// Placeholder: `{channel}`.
    VersionOfChannel,
    VersionSourceGone,
    OpenVersionSource,
    VersionAdaptHint,
    VersionSameAudience,
    VersionAlreadyMade,
    VersionNoNarration,
    VersionNoPlan,
    VersionNoChannel,
    VersionBusy,
    VersionNotSaved,
    PlaceScenesTitle,
    PlaceScenesHint,
    PlaceScenes,
    PlaceScenesNothing,
    PlacedScenesTitle,
    PlacedScenesAll,
    /// Placeholder: `{clip}` (seconds).
    PlacedScenePastClip,
    /// Placeholders: `{was}`, `{now}` (seconds).
    PlacedSceneStretched,
    PlacedSceneDropped,
    StageScenesToPlace,
    NarrationTitle,
    NarrationEmpty,
    GenerateNarration,
    /// Placeholders: `{persona}`, `{voice}`, `{n}` (characters).
    NarrationGenerateHint,
    RegenerateNarration,
    NarrationRunning,
    NarrationStopped,
    NarrationStale,
    NarrationStaleTag,
    NarrationPlay,
    NarrationPause,
    NarrationWordsHint,
    NarrationVoice,
    NarrationCost,
    /// Placeholder: `{n}`.
    NarrationCostValue,
    NarrationDuration,
    NarrationNoScript,
    NarrationNoPersona,
    NarrationVoiceFlagged(VoiceFlag),
    NarrationMissingKey,
    NarrationBusy,
    NarrationScenesBusy,
    NarrationOutdatesPictures,
    /// Placeholders: `{images}`, `{clips}`.
    NarrationOutdatesFit,
    /// Placeholders: `{images}`, `{clips}`.
    NarrationOutdatesReplan,
    NarrationOutdatesConfirm,
    NarrationOutdatesCancel,
    NarrationMissing,
    NarrationAudioMissing,
    NarrationCannotPlay,
    NarrationNotLoaded,
    ImportNarration,
    ImportNarrationDialog,
    ImportNarrationHint,
    RecordingReading,
    /// Placeholders: `{file}`, `{length}` (m:ss).
    RecordingChosen,
    RecordingReplaces,
    UseRecording,
    CancelRecording,
    NarrationAligning,
    NarrationRecording,
    /// Placeholder: `{length}` (m:ss).
    NarrationAudioValue,
    NarrationImported,
    RecordingUnreadable,
    RecordingUnsupported,
    RecordingEmpty,
    RecordingTooLarge,
    CaptureImport,
    CaptureImportDialog,
    CaptureImportHint,
    CaptureChosen,
    CaptureVoice,
    CaptureVoicePersona,
    CaptureVoiceOwn,
    CaptureVoiceOwnShort,
    CaptureConfirm,
    CaptureSteps,
    UseCapture,
    CaptureRunning,
    CaptureReplacesNote,
    NarrationCaptured,
    CaptureSummary,
    CaptureConfirmed,
    CaptureCoverage,
    CaptureTakesTitle,
    CaptureTakesHint,
    CaptureTake,
    CaptureTakeSpan,
    CaptureTakeOk,
    CaptureTakeTooLong,
    CaptureTakeTooShort,
    CaptureCutEarlier,
    CaptureCutLater,
    CaptureStale,
    CaptureRemove,
    CaptureRemoveHint,
    CaptureRemoveConfirm,
    CaptureRemoveKeep,
    CaptureNoPersona,
    CaptureNotConfirmed,
    CaptureMissing,
    CaptureNoCutPoint,
    CaptureNotSaved,
    StageTakes,
    StageTakesOne,
    StageTakesFlagged,
    CaptureTakeFlagged,
    CaptureVoicePersonaNone,
    CaptureModeLabel,
    CaptureModeHint,
    AnalysisHint,
    AnalysisNotStarted,
    AnalysisQueued,
    AnalysisRunning,
    AnalysisRunningLeft,
    AnalysisDone,
    AnalysisCancelled,
    AnalysisFailed,
    AnalysisOnGpu,
    AnalysisOnCpu,
    AnalysisCpuNote,
    AnalysisRedone,
    AnalyzeTake,
    ResumeAnalysis,
    CheckNoFace,
    CheckSeveralFaces,
    CheckSmallFace,
    CheckTurned,
    CheckMaskJumps,
    ChecksPassed,
    CaptureTakeBlocked,
    ControlTitle,
    ControlHint,
    ControlWaiting,
    ControlNotBuilt,
    ControlQueued,
    /// Placeholder: `{percent}`.
    ControlBuilding,
    ControlCancelled,
    ControlFailed,
    /// Placeholder: `{size}` (megabytes).
    ControlReady,
    ControlApproved,
    ControlApprovalLost,
    /// Placeholders: `{n}`, `{frames}`.
    ControlUncovered,
    ControlUncoveredBlocked,
    ControlSeveralFacesBlocked,
    ControlReviewHint,
    ControlBuild,
    ControlBuildAgain,
    ControlStopBuild,
    ControlApprove,
    ControlPlay,
    ControlStopPlaying,
    ControlSetFrame,
    ControlNotReady,
    ControlLengthBlocked,
    ControlCannotPlay,
    ControlOutbound(Outbound),
    SetLabel,
    SetHint,
    SetRoom,
    SetSceneImage,
    /// Placeholder: `{n}`.
    SetSceneNumber,
    SetImport,
    SetImportDialog,
    SetNoScenes,
    SetUnreadable,
    PerformedTitle,
    PerformedHint,
    TakeModelLabel,
    /// Placeholder: `{model}`.
    TakeModelChannelTag,
    PerformedBusy,
    PerformedPending,
    PerformedMissingClaudeKey,
    PerformedMissingHiggsfieldKey,
    PerformedMissingBytePlusKey,
    PerformedNothing,
    PerformedDraftFirst,
    PerformedDraftExpired,
    PerformedDraftFinished,
    /// Placeholder: `{when}`.
    TakeDraftDeadline,
    TakeDraftFinishNote,
    TakeModelGone,
    /// Placeholder: `{n}` (the take, from 1).
    PerformedClipName,
    TakePromptLabel,
    TakePromptHint,
    WriteTakePrompt,
    WriteTakePromptAgain,
    TakePromptWriting,
    TakePromptFailed,
    EditTakePrompt,
    SaveTakePrompt,
    CancelTakePrompt,
    TakePromptEdited,
    TakePromptFrameNote,
    TakePromptFrameNoteRecording,
    MakeTakeDraft,
    MakeTakeFinal,
    RegenerateTakeDraft,
    MakeTakeFinalAgain,
    ApproveTakeDraft,
    BackToTakeDrafts,
    UsePerformedTake,
    PlayTakeDraft,
    PlayTakeFinal,
    TakeMakingDraft,
    TakeMakingFinal,
    TakeQueued,
    StopTakeWaiting,
    TakeStoppedWaiting,
    CheckTakeAgain,
    /// Placeholder: `{reason}`.
    TakeFailed,
    TakeRefused,
    /// Placeholder: `{seconds}`.
    TakeDraftReady,
    TakeDraftApproved,
    /// Placeholder: `{seconds}`.
    TakeFinalReady,
    /// Placeholder: `{seconds}`.
    TakeLonger,
    /// Placeholder: `{seconds}`.
    TakeShorter,
    TakeDraftNote,
    TakeFinalPaidNote,
    TakeMakeHint,
    /// Placeholder: `{n}`.
    PerformedUsed,
    PerformedUsedNow,
    PerformedRealisticNote,
    PerformedBlocked(crate::PerformedBlock),
    PerformedMissingKlingKey,
    PerformedTestFirst,
    TakeCapReachedTitle,
    TakeCapInvalid,
    TakeSpendTitle,
    TakeSpendHint,
    TakeSpendPartial,
    TakeSpendOver,
    TakeCapLabel,
    SaveTakeCap,
    ResetTakeCap,
    EditTakeCap,
    TakeCapSaved,
    TakeCapQuestion,
    TakeCapConfirm,
    TestTakeTitle,
    TestTakeHint,
    TestTakeNone,
    TestTakeNeeded,
    TestTakeApproved,
    ApproveTestTake,
    DiscardTestTake,
    PlayTestTake,
    MakeTestTakeAgain,
    PreflightTitle,
    PreflightHint,
    PreflightFailedNote,
    PreflightPassed,
    PreflightReanalyze,
    PreflightLookMeasuring,
    PreflightLookNoFace,
    SendTakeAnyway,
    PreflightOverridden,
    MakeTakeFinalSkipDraft,
    TakeDraftOptionalNote,
    /// Placeholder: `{n}`.
    MakeTestTake,
    /// Placeholder: `{seconds}`.
    TestTakeReady,
    /// Placeholder: `{provider}`.
    TakeMakingTest,
    /// Placeholder: `{spent}`, `{to_come}`, `{total}`, `{cap}`.
    TakeSpendLine,
    /// Placeholder: `{amount}`.
    TakeSpendSkipDraft,
    /// Placeholder: `{amount}`.
    TestTakeCost,
    /// Placeholder: `{capture}`, `{look}`.
    PreflightFramingFailed,
    PreflightCheckName(bardo_domain::PreflightCheck),
    PreflightCheckFailed(bardo_domain::PreflightCheck),
    BodyFramingName(bardo_domain::BodyFraming),
    TakePromptInvalid(bardo_domain::TakePromptError),
    TakeModeName(TakeMode),
    AnalysisPartName(AnalysisPart),
    CaptureProblem(CaptureProblem),
    ScenesTitle,
    ScenesEmpty,
    PlanScenes,
    /// Placeholder: `{n}` (template version).
    PlanScenesHint,
    ReplanScenes,
    /// Placeholder: `{n}` (image and clip files).
    ReplanScenesConfirm,
    ConfirmReplanScenes,
    CancelReplanScenes,
    ScenesPlanning,
    ScenesDrawing,
    /// A cancelled scene job is still letting go of what it had sent.
    ScenesStopping,
    ScenesStopped,
    ScenesStale,
    /// `ScenesStale` when the scenes can be fitted to the narration.
    ScenesStaleFit,
    ScenesStaleTag,
    FitScenes,
    FitScenesTitle,
    FitScenesHint,
    /// On a button that pays for an image or a clip, while the scenes are
    /// out of date.
    ScenesPaidHeld,
    /// `ScenesPaidHeld` when the scenes cannot be fitted: only planning
    /// again brings them back.
    ScenesPaidHeldReplan,
    /// `ScenesPaidHeld` on a version whose scenes wait to be placed.
    ScenesPaidHeldVersion,
    /// Placeholder: `{n}`.
    ScenesCount,
    /// Placeholder: `{n}`.
    GenerateSceneImages,
    SceneImagesHint,
    /// Placeholders: `{n}`, `{start}`, `{end}`.
    SceneLabel,
    SceneEdited,
    EditScenePrompt,
    SaveScenePrompt,
    CancelScenePrompt,
    RegenerateSceneImage,
    SceneNoImage,
    SceneFailed,
    /// Placeholders: `{model}`, `{tokens}`, `{n}` (template version).
    SceneImageRecord,
    ScenePendingTitle,
    ScenePendingHint,
    AcceptSceneImage,
    RejectSceneImage,
    ClipsHint,
    /// Placeholder: `{n}`.
    AnimateMissingClips,
    AnimateScene,
    AnimateSceneAgain,
    SceneAnimating,
    SceneClipFailed,
    SceneClipModel,
    /// Placeholder: `{model}`.
    SceneClipModelChannel,
    SceneClipModelGone,
    /// Placeholders: `{old}`, `{new}`.
    SceneClipModelReplaced,
    /// Placeholders: `{old}`, `{new}`.
    ClipsChannelModelReplaced,
    /// Placeholders: `{seconds}`, `{price}`.
    SceneClipPlan,
    /// Placeholder: `{seconds}`.
    SceneClipPlanUnpriced,
    SceneMotionPrompt,
    SceneMotionFromImage,
    EditMotionPrompt,
    MotionPromptHint,
    /// Placeholders: `{seconds}`, `{model}`.
    SceneClipRecord,
    SceneClipStale,
    PlayClip,
    UseSceneStill,
    ScenePendingClipTitle,
    ScenePendingClipHint,
    AcceptSceneClip,
    RejectSceneClip,
    ScenesNoNarration,
    ScenesNoPlan,
    ScenesNarrationTooLong,
    SceneNotFound,
    ScenesMissingClaudeKey,
    ScenesMissingGeminiKey,
    ScenesMissingHiggsfieldKey,
    ScenesMissingHiggsfieldPlan,
    ScenesMissingKlingKey,
    SceneNoImageToAnimate,
    ScenesNothingToAnimate,
    SceneClipModelNotOffered,
    ScenesBusy,
    ScenesNarrationBusy,
    ScenesPlanOutOfDate,
    ScenesOtherText,
    ScenesWouldDiscardImages,
    ScenesNothingToGenerate,
    SceneNothingToReview,
    ScenesNotLoaded,
    SceneFieldError(SceneFieldError),
    /// An ad's end card (#281), in the Scenes inspector.
    EndCardTitle,
    EndCardHint,
    EndCardHeadline,
    EndCardSecondLine,
    EndCardSecondLineHint,
    EndCardShowOffer,
    /// Placeholder: `{offer}`.
    EndCardOfferIs,
    EndCardNoOffer,
    EndCardOfferTooLong,
    EndCardLook,
    EndCardTextPlace,
    EndCardLogoPlace,
    EndCardNoLogo,
    EndCardSave,
    EndCardSaved,
    EndCardOutOfDate,
    EndCardOff,
    AdItemSensitive,
    AdItemUnbackedScript,
    AdItemUnbackedPosts,
    AdItemScriptNotChecked,
    AdItemPostsNotChecked,
    AdItemUnconfirmed,
    AdItemCategory,
    AdItemConcept,
    AdItemsHeading,
    AdNotAcknowledged,
    AdReviewTitle,
    AdReviewHint,
    AdAcknowledgeAll,
    AdAcknowledgeLeft,
    CommercialTitle,
    CommercialHint,
    CommercialName(CommercialDisclosure),
    CommercialSent(Network),
    CommercialInStudio,
    CommercialTikTokYourBrand,
    CommercialTikTokBranded,
    CommercialReminder,
    CommercialReminderLabel,
    CommercialReminderTikTokYourBrand,
    CommercialReminderTikTokBranded,
    CommercialReminderNoLabel,
    CommercialReminderNothing,
    CommercialChangeNote,
    SponsorsLabel,
    SponsorsHint,
    SponsorError(SponsorError),
    StudioLink,
    PostsCheckTitle,
    PostsCheckHint,
    PostsCheckRun,
    PostsCheckRunning,
    PostsCheckNone,
    PostsCheckOutdated,
    PostsCheckAllBacked,
    PostsCheckUnbacked,
    PostsCheckSaveFirst,
    PostsCheckStopped,
    PlacementTitle,
    PlacementHint,
    PlacementShapeName,
    PlacementTextsTitle,
    PlacementTextsHint,
    PlacementTextsNone,
    PlacementPrimary,
    PlacementHeadline,
    PlacementCount,
    PlacementTextsSave,
    PlacementTextsReplaced,
    PlacementSaveFirst,
    PlacementPrimaryEmpty,
    PlacementPrimaryTooLong,
    PlacementHeadlineEmpty,
    PlacementHeadlineTooLong,
    PlacementWindowsTitle,
    PlacementWindowsHint,
    PlacementWholeWithBars,
    PlacementScene,
    PlacementFootage,
    PlacementLeft,
    PlacementRight,
    PlacementUp,
    PlacementDown,
    PlacementCenter,
    PlacementMoved,
    PlacementStart,
    PlacementRunning,
    PlacementFailed,
    PlacementCurrent,
    PlacementOutdated,
    PlacementNever,
    PlacementShowFolder,
    PlacementTextsNotReady,
    PlacementNothingChosen,
    PlacementPicturesWait,
    PlacementAlreadyRunning,
    PlacementFileVideo,
    PlacementCall,
    PlacementLink,
    PlacementCommercialReminder,
    PlacementSyntheticReminder,
    CardTextPlaceName(CardTextPlace),
    CardLogoPlaceName(CardLogoPlace),
    CardFieldError(CardFieldError),
    TemplatesTitle,
    TemplatesHint,
    TemplateKindName(TemplateKind),
    TemplateVersionsTitle,
    /// Placeholder: `{n}`.
    TemplateVersionLabel,
    TemplateCurrent,
    /// Placeholders: `{n}`, `{next}`.
    TemplateEditing,
    TemplateInstructions,
    TemplateInstructionsHint,
    TemplatePrompt,
    TemplatePromptHint,
    TemplateVariablesTitle,
    TemplateVariablesHint,
    TemplateVariableHint(TemplateVariable),
    SaveTemplate,
    RevertTemplate,
    DefaultTemplate,
    /// Placeholder: `{n}`.
    TemplateSaved,
    TemplateUnchanged,
    TemplateNotSaved,
    TemplateNotFound,
    TemplatesNotLoaded,
    TemplateGlobalUsedBy,
    TemplateAlreadyOwn,
    LibraryNameClash,
    LibraryOtherProject,
    LibraryGlobalSource,
    LibraryGlobalTag,
    LibraryGlobalNote,
    /// Placeholder: `{n}`.
    LibraryUsesTitle,
    LibrarySaveAnyway,
    LibraryCancel,
    LibraryBring,
    /// Placeholder: `{project}`.
    LibraryBringTitle,
    LibraryBringHint,
    LibraryBringName,
    LibraryBringConfirm,
    SharingNotFound,
    SharingNotOwn,
    SharingLinkedReadOnly,
    SharingNotShareable,
    SharingSameProject,
    SharingNameClash,
    SharingInvalidName,
    SharingFolderUnreachable,
    SharingConfirmPictures,
    SharingFileUnreadable,
    SharingFileNotWritten,
    SharingNotSaved,
    SharingNotAnItemFile,
    SharingNewerItemFile,
    SharingInvalidItemFile,
    SharingTitle,
    SharingShareable,
    SharingShareableHint,
    SharingFollows,
    SharingFollowsTemplate,
    SharingLinkedHint,
    SharingUpdating,
    SharingWaitingForFolder,
    SharingUnlink,
    SharingLost,
    SharingLostTemplate,
    SharingLostOk,
    SharingExport,
    SharingExportTitle,
    SharingExportHint,
    SharingExportTo,
    SharingExportDone,
    SharingNoOtherProject,
    SharingMakeGlobal,
    SharingMakeGlobalTitle,
    SharingMakeGlobalHint,
    SharingMakeGlobalTemplateHint,
    SharingMadeGlobal,
    SharingSaveFile,
    SharingSaved,
    SharingOpenFile,
    SharingOpened,
    SharingImport,
    SharingImportHint,
    SharingImportEmpty,
    SharingImportLinked,
    SharingImportIndependent,
    SharingImported,
    SharingArchivedProject,
    SharingLinkedTag,
    SharingShareableTag,
    SharingNameLabel,
    SharingGlobalName,
    SharingConfirmPicturesCheck,
    SharingClose,
    SharingWrongKind,
    SharingGlobalUsedBy,
    /// Placeholders: `{name}`, `{project}`.
    LibraryBrought,
    LibraryNoProject,
    TemplateSourceProject,
    TemplateSourceGlobal,
    TemplateSourceProjectHint,
    TemplateSourceGlobalHint,
    TemplateMakeOwn,
    TemplateMadeOwn,
    TemplateProblem(TemplateProblem),
    FetchedJustNow,
    /// Placeholder: `{n}`.
    FetchedMinutesAgo,
    /// Placeholder: `{n}`.
    FetchedHoursAgo,
    /// Placeholder: `{n}`.
    FetchedDaysAgo,
    /// Placeholder: `{n}`, already formatted with the decimal separator.
    NumberThousands,
    /// Placeholder: `{n}`.
    NumberMillions,
    /// Placeholder: `{n}`.
    NumberBillions,
    DecimalSeparator,
    /// Placeholder: `{n}`, the amount with its separators.
    MoneyFormat,
    ThousandsSeparator,
    /// Placeholder: `{amount}`.
    MoneyUnder,
    MoneyError(MoneyError),
    /// Placeholders: `{month}`, `{year}`.
    MonthFormat,
    /// 1 to 12.
    MonthName(u8),
    MeterName(Meter),
    MeterUnit(Meter),
    CostsTitle,
    CostsHint,
    CostsPreviousMonth,
    CostsNextMonth,
    /// Placeholder: `{month}`.
    CostsTotal,
    /// Placeholder: `{month}`.
    CostsEmpty,
    /// Placeholder: `{models}`.
    CostsUnpriced,
    CostsNotLoaded,
    CostsNotSaved,
    CostsNotPaid,
    ProjectBudgetHint,
    BackupSetUpProjectKey,
    DeleteRemovesKeys,
    EstimateProviderOwnKey,
    CostsPaidByProjects,
    CostsDeletedProjectName,
    CostsProjectKeyGone,
    CostsProjectKeysHint,
    CostsProjectKeysTitle,
    SwitcherProjectKeys,
    ProjectBudgetNone,
    ProjectBudgetAmount,
    ProjectBudgetLabel,
    ProjectKeyUsesApp,
    ProjectKeysNoProjectOpen,
    ProjectKeysInfo,
    ProjectKeysHint,
    ProjectKeysTitle,
    SettingsProjectTab,
    DeleteKeysNotRemoved,
    ProjectKeysNoProject,
    ProjectBudgetNeedsKey,
    CostsProvidersTitle,
    CostsProvidersHint,
    CostsChannelsTitle,
    CostsVideosTitle,
    CostsUnknownChannel,
    CostsUnknownVideo,
    BudgetNone,
    /// Placeholders: `{budget}`, `{percent}`.
    BudgetUsed,
    BudgetNear,
    BudgetReached,
    SetBudget,
    ChangeBudget,
    RemoveBudget,
    SaveBudget,
    CancelBudget,
    BudgetPlaceholder,
    BudgetSaved,
    BudgetRemoved,
    RatesTitle,
    RatesHint,
    RateAllModels,
    RateChanged,
    RateAdded,
    EditRate,
    SaveRate,
    CancelRate,
    /// Placeholder: `{price}`.
    ResetRate,
    RemoveRate,
    RateSaved,
    AddRateTitle,
    RateProvider,
    RateModel,
    RateModelPlaceholder,
    RateMeter,
    RatePrice,
    RatePricePlaceholder,
    AddRate,
    RateFieldError(RateFieldError),
    /// Placeholder: `{amount}`.
    EstimateCost,
    /// Placeholder: `{amount}`.
    EstimatePartial,
    EstimateUnknown,
    /// Placeholders: `{provider}`, `{spent}`, `{budget}`.
    EstimateNear,
    /// Placeholder: `{amount}`.
    EstimateRedraw,
    /// Placeholder: `{amount}`.
    ProjectSpent,
    BudgetReachedTitle,
    /// Placeholders: `{provider}`, `{spent}`, `{budget}`, `{amount}`.
    BudgetReachedLine,
    /// The question's title when nothing is reached and a price is
    /// unknown.
    PriceUnknownTitle,
    /// A model no rate covers. Placeholders: `{provider}`, `{model}`.
    PriceUnknownLine,
    BudgetQuestion,
    BudgetConfirm,
    BudgetCancel,
    OpenEditor,
    EditorBack,
    /// Placeholders: `{duration}`, `{fps}`.
    EditorDuration,
    EditorUndo,
    EditorRedo,
    EditorReviewRender,
    /// Placeholders: `{ready}`, `{total}`.
    EditorJobsProxies,
    EditorJobsIdle,
    EditorJobFailed,
    EditorBinScenes,
    EditorBinMedia,
    EditorBinCaptionStyles,
    /// Placeholder: `{n}`.
    EditorSceneLabel,
    EditorProxyBadge,
    EditorPlay,
    EditorPause,
    EditorPreviousFrame,
    EditorNextFrame,
    EditorInspectorEmpty,
    EditorInspectorClip,
    /// Placeholders: `{provider}`, `{model}`.
    EditorSourceImage,
    /// Placeholders: `{provider}`, `{model}`.
    EditorSourceClip,
    EditorSourceStill,
    EditorSourceNone,
    EditorIn,
    EditorOut,
    EditorLength,
    EditorFile,
    EditorNarrationLabel,
    EditorToolSelect,
    EditorToolSplit,
    EditorToolTransition,
    EditorSnapWords,
    EditorAiCuts,
    EditorRetention,
    CutsSuggest,
    CutsSuggestAgain,
    CutsHint,
    CutsAgainHint,
    CutsNoPoints,
    CutsRunning,
    CutsStop,
    CutsStopped,
    CutsRetry,
    CutsEmpty,
    CutsAcceptStrong,
    CutsFloor,
    CutsHidden,
    CutsShown,
    CutsShowHidden,
    CutsHideLow,
    CutsTitle,
    CutsAccept,
    CutsReject,
    CutsNext,
    CutsUndo,
    CutsAccepted,
    CutsConfidence,
    CutsReasonSentence,
    CutsReasonPause,
    CutsReasonScene,
    CutsReasonTopic,
    /// Only TypeSafe has a message.
    CutsMissingKey(Provider),
    CutsBusy,
    CutsGone,
    CutsNotSaved,
    CutsFloorNotSaved,
    EditorDuckMusic,
    EditorZoomIn,
    EditorZoomOut,
    EditorTrackCaptions,
    EditorTrackVideo,
    EditorTrackNarration,
    EditorTrackMusic,
    EditorTrackSfx,
    EditorEmptyTitle,
    EditorEmptyNoNarration,
    EditorEmptyNoScenes,
    EditorBackToProject,
    /// Placeholders: `{ready}`, `{total}`.
    EditorBuildingProxies,
    EditorKeepEditing,
    EditorMediaMissing,
    EditorProxyFailed,
    EditorProxyCancelled,
    EditorProxyBuilding,
    EditorProblemsOne,
    /// Placeholder: `{count}`.
    EditorProblemsMany,
    /// Placeholders: `{n}`, `{file}`, `{reason}`.
    EditorProblemLine,
    EditorProblemNoImage,
    EditorProblemFileGone,
    EditorProblemCancelled,
    EditorRetryProxies,
    EditorMediaOffline,
    EditorStale,
    EditorPreviewBuilding,
    EditorNothingToRetry,
    EditorFfmpegMissing,
    EditorPreviewFailed,
    EditorNotLoaded,
    EditorNoSound,
    EditorNothingToCut,
    EditorCannotEdit,
    EditorCaptionTextInvalid,
    EditorPerformedNotHeard,
    EditorMediaTake,
    EditorMediaPutOnCut,
    EditorMediaPerformedRoughCut,
    EditorMediaPerformedRoughCutHint,
    EditorPerformedTitle,
    EditorPerformedInSync,
    EditorPerformedEarly,
    EditorPerformedLate,
    EditorPerformedUnheard,
    EditorPerformedPutBack,
    EditorPerformedPutBackHint,
    EditorPerformedCannotSync,
    EditorPerformedUnheardHint,
    EditorPerformedCover,
    EditorPerformedCoverHint,
    EditorPerformedCoverScene,
    EditorPerformedCoverLength,
    EditorPerformedCoverButton,
    EditorEditNotSaved,
    EditorCutReset,
    EditorInspectorAudio,
    EditorSourceIn,
    EditorRemove,
    EditorShortcuts,
    EditorInspectorTrack,
    EditorLevel,
    EditorMute,
    EditorSolo,
    EditorMuteShort,
    EditorSoloShort,
    EditorDuck,
    EditorDuckDepth,
    EditorDuckHint,
    /// Placeholder: `{depth}`.
    EditorDuckReadout,
    EditorNoMusic,
    EditorFades,
    EditorFadeIn,
    EditorFadeOut,
    EditorTransition,
    EditorTransitionLength,
    EditorTransitionEverywhere,
    EditorTransitionFirst,
    EditorTransitionHint,
    /// Placeholder: `{length}`.
    EditorTransitionShort,
    EditorMotion,
    EditorMotionStrength,
    EditorMotionEverywhere,
    EditorMotionVary,
    EditorMotionHint,
    EditorMotionClip,
    /// Placeholder: `{n}`.
    EditorSeconds,
    /// Placeholder: `{n}`, signed, with the decimal separator.
    EditorDecibels,
    EditorLaneHint,
    EditorInspectorCaption,
    EditorCaptionText,
    EditorCaptionTextHint,
    EditorCaptionStyle,
    EditorCaptionStyleHint,
    EditorCaptionHighlight,
    EditorCaptionHighlightHint,
    EditorShowCaptions,
    EditorFraming,
    EditorFramingFit,
    EditorFramingFill,
    EditorFramingCustom,
    EditorFramingPosition,
    EditorFramingLandscapeHint,
    EditorFramingDragHint,
    EditorFramingFitHint,
    EditorFramingSource,
    EditorMediaImport,
    EditorMediaImportHint,
    EditorMediaImportingOne,
    /// Placeholder: `{count}`.
    EditorMediaImporting,
    EditorMediaEmpty,
    EditorMediaAddMusic,
    EditorMediaAddSfx,
    EditorMediaAddVideo,
    EditorMediaAddHint,
    /// Placeholders: `{file}`, `{reason}`.
    EditorMediaImportFailed,
    EditorMediaAudio,
    EditorMediaVideo,
    EditorSourceFootage,
    EditorSourcePerformed,
    /// Placeholders: `{name}`, `{file}`, `{reason}`.
    EditorProblemFootageLine,
    MediaImportUnreadable,
    MediaImportUnsupported,
    MediaImportTooShort,
    MediaImportNotSaved,
    MusicPromptTitle,
    MusicPromptEmpty,
    MusicPromptGenerate,
    /// Placeholder: `{n}`, the template version.
    MusicPromptGenerateHint,
    MusicPromptRegenerate,
    MusicPromptRegenerateHint,
    MusicPromptRunning,
    MusicPromptStopped,
    MusicPromptSave,
    MusicPromptRevert,
    MusicPromptSaved,
    MusicPromptCopy,
    MusicPromptCopied,
    MusicPromptEdited,
    MusicPromptMissing,
    MusicPromptMissingKey,
    MusicPromptBusy,
    MusicPromptNotSaved,
    ChapterRename,
    ChaptersStopped,
    ThumbnailWriteStopped,
    ThumbnailDrawStopped,
    MetadataChaptersNote,
    ThumbnailTitle,
    ThumbnailHint,
    ThumbnailShortNote,
    ThumbnailWrite,
    ThumbnailWriteHint,
    ThumbnailRewrite,
    ThumbnailWriting,
    ThumbnailText,
    ThumbnailTextHint,
    ThumbnailPlacement,
    ThumbnailLookLabel,
    ThumbnailBackground,
    ThumbnailBackgroundHint,
    ThumbnailBackgroundsEmpty,
    ThumbnailBackgroundDrawn,
    ThumbnailBackgroundScene,
    ThumbnailPrompt,
    ThumbnailPromptHint,
    ThumbnailDraw,
    ThumbnailDrawing,
    ThumbnailSave,
    ThumbnailSaving,
    ThumbnailSaved,
    ThumbnailOutdated,
    ThumbnailNotYetSaved,
    ThumbnailApply,
    ThumbnailWriteMissingKey,
    ThumbnailDrawMissingKey,
    ThumbnailBusy,
    ThumbnailNoPrompt,
    ThumbnailNoBackground,
    ThumbnailBackgroundMissing,
    ThumbnailNotComposed,
    ThumbnailNotSaved,
    ChaptersTitle,
    ChaptersHint,
    ChaptersShortNote,
    ChaptersWrite,
    ChaptersWriteHint,
    ChaptersRewrite,
    ChaptersWriting,
    ChaptersEmpty,
    ChaptersInclude,
    ChaptersEdited,
    ChapterEarlier,
    ChapterLater,
    ChapterRemove,
    ChapterCutAway,
    ChaptersStale,
    ChaptersLeftOut,
    ChaptersNoCut,
    ChaptersNoNarration,
    ChaptersMissing,
    ChapterCannotMove,
    ChaptersMissingKey,
    ChaptersBusy,
    ChaptersNotSaved,
    UploadThumbnail,
    UploadThumbnailHint,
    UploadThumbnailSet,
    UploadThumbnailNotAllowed,
    UploadThumbnailFailed,
    ExportFileThumbnail,
    ChannelThumbnailLook,
    ChannelThumbnailLookHint,
    ThumbnailFieldError(ThumbnailFieldError),
    ChapterFieldError(ChapterFieldError),
    ChapterProblem(ChapterProblem),
    TextPlacementName(TextPlacement),
    ThumbnailLookName(ThumbnailLook),
    MusicPromptFieldError(MusicPromptFieldError),
    MusicTrackHeading,
    MusicTrackEmpty,
    MusicTrackGenerate,
    /// Placeholder: `{length}`, the narration's length.
    MusicTrackGenerateHint,
    MusicTrackRegenerate,
    MusicTrackRegenerateHint,
    MusicTrackRunning,
    MusicTrackStopped,
    MusicTrackNew,
    MusicTrackInUse,
    /// Placeholders: `{length}`, `{model}`.
    MusicTrackDetails,
    MusicTrackPlay,
    MusicTrackPause,
    MusicTrackUse,
    MusicTrackUseHint,
    MusicTrackDiscard,
    MusicTrackUsed,
    MusicTrackDiscarded,
    MusicTrackNoNarration,
    MusicTrackMissingKey,
    MusicTrackBusy,
    MusicTrackNotFound,
    MusicTrackAudioMissing,
    MusicTrackCannotPlay,
    MusicTrackNotSaved,
    MusicTrackNoRoom,
    SettingsKeysTab,
    SettingsAppearanceTab,
    AppearanceTheme,
    AppearanceFollowSystem,
    AppearanceFollowSystemHint,
    AppearanceLight,
    AppearanceDark,
    AppearanceFixed,
    AppearanceFixedHint,
    AppearanceContrast,
    UiThemeNotSaved,
    UiThemeName(UiTheme),
    UiThemeKind(UiTheme),
    ProviderKeysInfo,
    SettingsNetworksTab,
    AppCredentialsTitle,
    AppCredentialsHint,
    AppCredentialsInfo,
    AppCredentialsName(Network),
    AppCredentialsPurpose(Network),
    ClientId(Network),
    ClientIdPlaceholder(Network),
    ClientSecret(Network),
    ClientSecretPlaceholder(Network),
    AppCredentialsNotSet,
    /// Placeholder: `{hint}`.
    AppCredentialsSaved,
    AppCredentialsUnreadable,
    SaveAppCredentials,
    ReplaceAppCredentials,
    RemoveAppCredentials,
    AppCredentialsFieldError(Network, AppCredentialsFieldError),
    ConnectionNotConnectedLabel,
    ConnectionConnecting(Network),
    /// Placeholder: `{channel}`.
    ConnectionConnected,
    /// Placeholder: `{channel}`.
    ConnectionReconnectNeeded,
    Connect,
    Reconnect,
    CancelConnect,
    Disconnect,
    Disconnecting,
    CheckConnection,
    CheckingConnection,
    OpenNetworkSettings,
    /// Placeholder: `{channel}`.
    ConnectionChecked(Network),
    ConnectionDisconnected,
    /// Placeholder: `{network}`.
    ConnectionDisconnectedNotRevoked,
    /// Placeholder: `{network}`.
    ConnectionDisconnectedKeptForOthers,
    /// Placeholder: `{network}`.
    ConnectionDisconnectedCredentialsRefused,
    ConnectionReconnectHint(Network),
    ConnectionNotOffered,
    ConnectionNeedsAppCredentials(Network),
    ConnectionNotConnected,
    ConnectionDenied,
    ConnectionTimedOut,
    ConnectionCancelled,
    ConnectionListenFailed,
    ConnectionPortInUse,
    ConnectionMissingScopes(Network),
    ConnectionTokensTooLarge,
    ConnectionStoreFailed,
    ConnectionNotSaved,
    ConnectionNoPages,
    ConnectionNoLinkedAccount,
    ConnectionChoiceGone,
    /// The pasted-token form of a network that signs in that way.
    ConnectionTokenLabel(Network),
    ConnectionTokenPlaceholder(Network),
    ConnectionTokenHelp(Network),
    ConnectionOpenTokenTool(Network),
    PastedTokenError(PastedTokenError),
    /// Heading over the accounts a pasted token reached.
    ConnectionChoose(Network),
    /// Placeholder: `{via}`.
    ConnectionChoiceVia(Network),
    ConnectionUseAccount,
    /// The status of an account waiting for the user's choice.
    ConnectionChooseLabel,
    SignInFailure(Network, SignInFailureKind),
    /// Plans Bardo signs in to instead of a key (ADR-0028).
    PlansTitle,
    PlansHint,
    PlanNotConnected,
    /// Placeholders: `{plan}`, `{credits}`.
    PlanConnected,
    PlanConnectedPlain,
    PlanReconnectNeeded,
    PlanUnreadable,
    PlanSignIn,
    PlanSignInAgain,
    PlanWaiting,
    PlanCancel,
    PlanCheck,
    PlanChecking,
    PlanSignOut,
    /// Placeholders: `{plan}`, `{credits}`.
    PlanChecked,
    PlanNotSignedIn,
    PlanSignInAgainHint,
    PlanNotOffered,
    PlanDenied,
    PlanTimedOut,
    PlanCancelled,
    PlanListenFailed,
    PlanPortInUse,
    PlanTokensTooLarge,
    PlanStoreFailed,
    PlanFailure(SignInFailureKind),
    NetworkAccountStillConnected,
    ConsentPageDone,
    ConsentPageFailed,
    Details,
    NavBudgetsUsed,
    AppearanceLayout,
    AppearanceLayoutHint,
    UiLayoutName(LayoutId),
    UiLayoutDescription(LayoutId),
    UiLayoutNotSaved,
    StatusJobs,
    StatusOneJob,
    StatusNoJobs,
    StatusMonthSpend,
    SceneColumnPicture,
    SceneColumnTime,
    SceneColumnNarration,
    SceneColumnPrompt,
    SceneColumnImage,
    SceneColumnClip,
    SceneColumnModel,
    SceneColumnCost,
    SceneStateDone,
    SceneKeysHint,
    CostsBriefSpent,
    CostsBriefOver,
    CostsBriefNear,
    CostsBriefUnpriced,
    StageScriptMissing,
    StageScriptToReview,
    StageScriptWords,
    StageNarrationMissing,
    StageNarrationStale,
    StageScenesMissing,
    StageScenesStale,
    StageToReview,
    StageImages,
    StageClips,
    StageClipsNone,
    StageEditorReady,
    StageWorking,
    FilterAll,
    FilterPending,
    FilterPendingEmpty,
    SceneCardReview,
    SceneCardFailed,
    SceneCardToDraw,
    SceneCardToAnimate,
    SceneCardAnimating,
    SceneImagePrompt,
    SceneCast,
    SceneCastHint,
    CastPortraitLater,
    EditCastDescription,
    RedrawCastPortrait,
    CastPortraitFailed,
    CastEdited,
    /// A person the user chose to draw from the description alone.
    CastDescribedOnly,
    CastDescribedOnlyHint,
    CastDrawWithoutPortrait,
    CastUsePortrait,
    /// A recurring object's texts on Scenes (#361), beside the people's.
    SceneProps,
    ScenePropsHint,
    PropPictureLater,
    EditPropDescription,
    RedrawPropPicture,
    PropPictureFailed,
    PropDescribedOnly,
    PropDescribedOnlyHint,
    PropDrawWithoutPicture,
    PropUsePicture,
    PropSaveHint,
    ScenePlace,
    ScenePlaceHint,
    EditPlaceDescription,
    PlaceEdited,
    ScenePlaceLeft,
    SceneCurrentImage,
    SceneNewImage,
    ProjectSwitch,
    GenerationDetails,
    CostsOverview,
    CostsAcrossProviders,
    CostsBudgetsTile,
    CostsBudgetsInAlert,
    CostsNoBudgets,
    CostsUnpricedTitle,
    CostsUnpricedModel,
    CostsUnpricedModels,
    CostsUnpricedNone,
    AddPrice,
    CostsColumnProvider,
    CostsColumnUsage,
    CostsColumnSpent,
    CostsColumnBudget,
    CostsColumnState,
    NavSectionName(NavSection),
    /// An entry of the navigation: a place, or places under tabs.
    NavEntryName(NavEntry),
    DestinationName(Destination),
    StageName(Stage),
    /// Why a stage is locked: it waits on that stage.
    StageAfter(Stage),
    RenderNoCut,
    RenderChecking,
    RenderNothingChosen,
    RenderBlocked,
    RenderCutChanged,
    RenderAlreadyRunning,
    RenderWhileExporting,
    RenderWhileUploading,
    RenderCheckFailed,
    RenderNotLoaded,
    RenderNoAccounts,
    RenderInfo,
    RenderFigureLength,
    RenderFigureFrame,
    RenderFigureLoudness,
    RenderFigureCaptions,
    RenderCaptionsOn,
    RenderCaptionsOff,
    RenderMeasuring,
    RenderLufs,
    RenderSilent,
    RenderTargets,
    RenderChosen,
    RenderInclude,
    GateTooLong,
    GateNoEncoder,
    GateReframed,
    /// A 16:9 file of a 9:16 cut: tall pictures show whole, with bars.
    GateReframedWide,
    GateCaptionsOff,
    GateMissingMedia,
    GateSilent,
    GateLoudnessFar,
    GatePeaksLimited,
    GateLongForUnverified,
    GateBlocks,
    GateWarning,
    RenderStateReady,
    RenderStateBlocked,
    RenderStateWarnings,
    RenderStateChecking,
    RenderLastCurrent,
    RenderLastOutdated,
    RenderLastNone,
    RenderLastFile,
    RenderLastOutdatedHint,
    RenderLoudnessOnTarget,
    RenderLoudnessOffTarget,
    RenderShowFile,
    RenderSize,
    RenderColumnPreset,
    RenderColumnChecks,
    RenderColumnLast,
    RenderEncoder,
    RenderEncoderHardware,
    RenderEncoderSoftware,
    RenderStart,
    RenderCheckAgain,
    RenderConfirmTitle,
    RenderConfirmBody,
    RenderConfirmWarnings,
    RenderConfirm,
    RenderConfirmBack,
    RenderRunning,
    RenderStopped,
    RenderCancelled,
    RenderResume,
    StageRenderReady,
    StageRendered,
    StageRenderOutdated,
    StageRendering,
    StageRenderStopped,
    ExportNoAccounts,
    ExportNothingChosen,
    ExportBlocked,
    ExportAlreadyRunning,
    ExportWhileRendering,
    ExportNotLoaded,
    MetadataMissingKey,
    MetadataBusy,
    MetadataMissing,
    MetadataNotSaved,
    ExportFileVideo,
    ExportFileTitle,
    ExportFileDescription,
    ExportFileCaption,
    ExportFileTags,
    ExportFileVisibility,
    DisclosureReminder,
    DisclosureNotice,
    StageExportReady,
    StageExported,
    StageExportOutdated,
    StageExporting,
    StageExportStopped,
    PersonaRealisticVoice,
    PersonaRealisticVoiceHint,
    LookTitle,
    LookHint,
    LookSaveFirst,
    LookEmpty,
    LookDrawnTag,
    LookImportedTag,
    LookFromDescription,
    LookRemovePhotoConfirm,
    LookNotLoaded,
    LookDescription,
    LookDescriptionPlaceholder,
    LookDraw,
    LookDrawAgain,
    LookDrawHint,
    LookDrawing,
    LookDeclined,
    LookFailed,
    LookStopped,
    LookPendingTitle,
    LookPendingHint,
    LookKeep,
    LookDiscard,
    LookImport,
    LookImportTitle,
    LookImportHint,
    LookShowsMe,
    LookChoosePhoto,
    LookImportDialog,
    LookCancel,
    LookImported,
    LookKept,
    LookRemove,
    LookRemoveConfirm,
    LookRemoveYes,
    LookRemoved,
    LookRealistic,
    LookRealisticHint,
    PersonaExportedNoLook,
    LookMissingClaudeKey,
    LookMissingImageKey,
    LookBusy,
    LookNothingWaiting,
    LookNone,
    LookNotConfirmed,
    LookUnreadable,
    LookImageRefused,
    LookNotSaved,
    LookDescriptionError(LookDescriptionError),
    LookImageReason(LookImageError),
    LookAssetTitle,
    LookAssetHint,
    LookAssetKindLabel,
    LookAssetKindName(LookAssetKind),
    LookAssetIdLabel,
    LookAssetIdPlaceholder,
    LookAssetSave,
    LookAssetRemove,
    /// Placeholders: `{kind}`, `{id}`.
    LookAssetHeld,
    LookAssetSaved,
    LookAssetRemoved,
    LookAssetError(LookAssetError),
    ExportInfo,
    ExportFigureRendered,
    ExportFigureMetadata,
    ExportFigureExported,
    ExportFigureCost,
    ExportFigureNetworks,
    ExportChosen,
    ExportTargets,
    ExportInclude,
    ExportStart,
    ExportRunning,
    ExportStopped,
    ExportCancelled,
    ExportShowFolder,
    ExportColumnRender,
    ExportColumnLast,
    ExportRenderOutdatedHint,
    ExportNoRenderHint,
    ExportStateReady,
    ExportStateNoRender,
    ExportStateNoMetadata,
    ExportStateProblems,
    ExportLastCurrent,
    ExportLastOutdated,
    ExportLastNone,
    ExportLastOutdatedHint,
    MetadataGenerate,
    MetadataRegenerate,
    MetadataGenerateHint,
    MetadataRegenerateHint,
    MetadataRunning,
    MetadataStopped,
    MetadataConfirmTitle,
    MetadataConfirmBody,
    MetadataNone,
    MetadataEmpty,
    MetadataStateNone,
    MetadataStateGenerated,
    MetadataStateEdited,
    MetadataEdited,
    MetadataFieldTitle,
    MetadataFieldDescription,
    MetadataFieldCaption,
    MetadataFieldTags,
    MetadataTagsPlaceholder,
    MetadataCounter,
    MetadataTagsCount,
    MetadataTagsCountOf,
    MetadataFooterNote,
    MetadataHashtagsNote,
    MetadataSave,
    MetadataRevert,
    MetadataSaved,
    MetadataCopy,
    MetadataCopied,
    MetadataPreview,
    MetadataCostUnpriced,
    /// Where the network takes the synthetic-content label.
    DisclosureHow(Network),
    /// A metadata rule broken, with `{limit}` where it has one.
    MetadataProblem(MetadataProblem),
    PublicationNoAccount,
    PublicationNotExported,
    PublicationAlreadyLinked,
    PublicationNotFound,
    PublicationNotSaved,
    MetricsMissingKey,
    MetricsNothingToSync,
    MetricsAlreadySyncing,
    MetricsNotLoaded,
    PublicationTitle,
    PublicationNeedsExport,
    PublicationLinkPlaceholder,
    PublicationMark,
    PublicationMarkHint,
    PublicationPosted,
    PublicationMissing,
    PublicationMissingHint,
    PublicationOpen,
    PublicationChange,
    PublicationSave,
    PublicationCancel,
    PublicationRemove,
    PublicationRemoveConfirm,
    PublicationRemoveKeep,
    PublicationSaved,
    PublicationRemoved,
    PublicationPostedAt,
    PublicationNoMetrics,
    PublicationConnectForMetrics,
    PublicationReconnectForMetrics,
    PublicationFigure,
    PublicationTileViews,
    PublicationTileEngaged,
    MetricViews,
    MetricLikes,
    MetricComments,
    MetricHidden,
    MetricHiddenHint,
    MetricsSyncedAgo,
    MetricsNotSynced,
    MetricsSyncNow,
    MetricsSyncing,
    MetricsSyncStopped,
    MetricsSyncHint,
    MetricsHistory,
    MetricsChange,
    MetricsViewsChange,
    MetricEngagedViews,
    MetricEngagedViewsHint,
    MetricWatchTime,
    MetricAverageView,
    MetricAverageViewed,
    MetricRevenue,
    MetricRpm,
    MetricRpmHint,
    MetricCpm,
    MetricCpmHint,
    MetricPlaybackCpm,
    MetricPlaybackCpmHint,
    MetricNotMonetized,
    MetricNotMonetizedHint,
    MetricsOwnerLine,
    MetricsRetention,
    MetricsRetentionHint,
    MetricsRetentionStart,
    MetricsRetentionEnd,
    RetentionTitle,
    RetentionNone,
    RetentionUnavailable,
    RetentionRead,
    RetentionDrops,
    RetentionDropsHint,
    RetentionNoDrops,
    RetentionChanged,
    RetentionNoLength,
    RetentionLost,
    RetentionAtShare,
    RetentionScene,
    RetentionFootage,
    RetentionNoPicture,
    RetentionNoSpeech,
    RetentionShowInEditor,
    MetricsHours,
    MetricsMinutes,
    MetricsPercent,
    MetricShares,
    MetricSaves,
    MetricReach,
    MetricReachHint,
    MetricInteractions,
    MetricInteractionsHint,
    MetricAverageWatch,
    MetricNotReportedHint,
    MetricsInstagramLine,
    MetricsTikTokLine,
    MetricsInsightsEmpty,
    MetricsInsightsPending,
    PerformanceInfo,
    PerformanceEmpty,
    PerformanceNoChannels,
    PerformanceHistoryTitle,
    PerformanceHistoryEmpty,
    PerformancePosts,
    PerformanceTracked,
    PerformanceTile,
    PerformanceOwnerNotConnected,
    PerformanceOwnerReconnect,
    PerformanceTileEngaged,
    DecisionEngineLabel,
    DecisionEngineHint,
    DecisionEngineCaption,
    DecisionEngineNotSaved,
    DecisionEngineName(DecisionEngineId),
    MetricsSettingsTab,
    MetricsSettingLabel,
    MetricsSettingHint,
    MetricsSettingNotSaved,
    PostLinkProblem(PostLinkError),
    MetricsSyncOption(MetricsSyncOnStart),
    /// Why an upload cannot start now.
    UploadBlocked(UploadBlock),
    UploadChanged,
    UploadReplaceNotConfirmed,
    UploadNotStarted,
    PublicationReplacesUpload,
    PublicationUploading,
    UploadTitle,
    UploadHint,
    UploadOpenReview,
    UploadReviewTitle,
    UploadFieldFile,
    UploadFieldChannel,
    UploadFieldVisibility,
    UploadMadeForKids,
    UploadMadeForKidsHint,
    UploadSynthetic,
    UploadSyntheticHint,
    UploadSyntheticOn,
    UploadReplacePost,
    UploadReplaceUpload,
    UploadIrreversible,
    UploadStart,
    UploadBack,
    UploadQueued,
    UploadStateWaiting,
    UploadStateUploading,
    UploadStateRetrying,
    UploadStateProcessing,
    UploadStateStillProcessing,
    UploadStatePublished,
    UploadStateRestricted,
    UploadStateStopped,
    UploadStateFailed,
    UploadRetryingHint,
    UploadProcessingHint,
    UploadStillProcessingHint,
    UploadStoppedHint,
    UploadRestrictedHint,
    /// Opens the guide's list of why YouTube keeps a video private.
    UploadRestrictedCauses,
    UploadFailureQuota,
    UploadFailureCredits,
    /// A post that was sent and got no answer: it may be live.
    UploadFailureAnswerLost,
    UploadFailureUploadLimit,
    UploadFailureReconnect,
    UploadFailureRejected,
    UploadFailureProcessing,
    UploadFailureRemoved,
    UploadFailureAccountChanged,
    UploadFailureRenderChanged,
    UploadStop,
    UploadResume,
    UploadRetry,
    UploadCheckAgain,
    UploadSentAt,
    PublicationReplaceUploadConfirm,
    PublicationReplaceUploadYes,
    UploadFieldWhen,
    UploadWhenNow,
    UploadWhenSchedule,
    UploadStartScheduled,
    UploadStateScheduled,
    UploadScheduledAt,
    UploadOverLimitAt,
    UploadOverLimitRetryAt,
    UploadScheduledHint,
    UploadRestrictedScheduledHint,
    UploadFailureScheduleMissed,
    UploadReelHint,
    UploadPostHint,
    UploadFieldAccount,
    UploadFieldCaption,
    UploadFieldCover,
    UploadCoverHint,
    UploadCoverInvalid,
    UploadCoverPastEnd,
    UploadShareToFeed,
    UploadShareToFeedHint,
    UploadAiLabel,
    UploadAiLabelHint,
    UploadPostAiLabel,
    UploadPostAiLabelHint,
    UploadReelIrreversible,
    UploadPostIrreversible,
    UploadReelStart,
    UploadPostStart,
    UploadPostCost,
    UploadPostCostHint,
    UploadReelProcessingHint,
    UploadPostProcessingHint,
    UploadStateOverLimit,
    UploadOverLimitHint,
    UploadOverLimitSoonHint,
    UploadOverLimitRecheckHint,
    UploadIssue,
    UploadSpecsTitle,
    /// One Reel spec problem, by `ReelSpecProblem::code`.
    UploadSpec(&'static str),
    ScheduleDate,
    ScheduleTime,
    ScheduleDatePlaceholder,
    ScheduleTimePlaceholder,
    ScheduleZone,
    ScheduleHint,
    ScheduleChange,
    ScheduleCancel,
    ScheduleSave,
    ScheduleCancelConfirm,
    ScheduleCancelYes,
    ScheduleKeep,
    ScheduleWorking,
    ScheduleChanged,
    ScheduleCancelled,
    ScheduleAlreadyLive,
    ScheduleNotScheduled,
    ScheduleFailed,
    ScheduleNotAllowed,
    DateTimeFormat,
    /// A day without its time, e.g. `October 4, 2026`.
    DateFormat,
    TimeFormat,
    TimeAm,
    TimePm,
    DateTimeWithZone,
    ZoneWithOffset,
    WeekdayName(u8),
    JobWaitsUntil,
    UploadStateDue,
    UploadStateMissed,
    UploadDueAt,
    UploadDueHint,
    UploadMissedAt,
    UploadMissedHint,
    UploadReelScheduleHint,
    /// `UploadDueHint` while the background agent is on.
    UploadDueHintAgent,
    /// `UploadReelScheduleHint` while the background agent is on.
    /// Placeholder: `{network}`.
    UploadReelScheduleHintAgent,
    /// `UploadMissedHint` while the background agent is on.
    UploadMissedHintAgent,
    JobRunsElsewhere,
    JobSuperseded,
    JobRestyled,
    /// A scene plan job retried after a newer scene job of its project.
    JobScenesMovedOn,
    /// An image or clip job retried while another job works on one of its
    /// scenes.
    JobScenesTaken,
    /// A performed take that never reached its provider, retried from the
    /// jobs panel.
    JobTakeFromItsPage,
    /// A scene job retried while a narration of its project is being made.
    JobNarrationBusy,
    /// A narration or scene job retried while scenes of its project are
    /// being made, or a cancelled scene job is still stopping.
    JobScenesBusy,
    /// A text job retried while another job is at work on the same thing,
    /// or while its own cancelled run is still stopping.
    JobSameWork,
    JobAsksFirst,
    /// An image or clip job retried where the question about its cost
    /// cannot be asked.
    JobOverBudget,
    SettingsPublishingTab,
    AgentTitle,
    AgentQuestion,
    AgentExplanation,
    AgentSwitch,
    AgentSends,
    AgentLimits,
    AgentHint,
    AgentStatusOff,
    AgentStatusRunning,
    AgentStatusNotRunning,
    AgentStartNow,
    AgentWorking,
    AgentNotSetUp,
    AgentNotRemoved,
    AgentNotSaved,
    SettingsAboutTab,
    AboutTitle,
    /// Placeholder: `{version}`.
    AboutVersion,
    AboutSummary,
    AboutFfmpegTitle,
    /// Placeholder: `{version}`.
    AboutFfmpeg,
    AboutFfmpegSource,
    AboutFfmpegLicense,
    AboutNoticesTitle,
    AboutNotices,
    AboutOpenNotices,
    AboutNoNotices,
    AboutDiagnosticsTitle,
    AboutDiagnostics,
    AboutExportDiagnostics,
    AboutExportingDiagnostics,
    AboutOpenLogs,
    /// Placeholder: `{path}`.
    AboutDiagnosticsSaved,
    AboutShowDiagnostics,
    AboutDiagnosticsFailed,
    PrivacyNotAccepted,
    PrivacyTitle,
    PrivacySummary,
    /// Placeholder: `{when}`.
    PrivacyAcceptedOn,
    PrivacyNotYet,
    PrivacyChanged,
    PrivacyRead,
    PrivacyOnline,
    PrivacyAccept,
    PrivacyConsent,
    PrivacyUsesYouTube,
    YouTubeTerms,
    GooglePrivacyPolicy,
    GooglePermissions,
    DeleteYouTubeData,
    DeleteYouTubeDataConfirm,
    DeleteYouTubeDataNow,
    DeleteYouTubeDataCancel,
    YouTubeDataDeleted,
    YouTubeDataNotDeleted,
    PrivacyBanner,
    PrivacyBannerReview,
    UpdatesTitle,
    UpdatesSummary,
    UpdatesSwitch,
    UpdatesCheckNow,
    UpdatesChecking,
    UpdatesNever,
    /// Placeholder: `{when}`.
    UpdatesUpToDate,
    /// Placeholder: `{when}`.
    UpdatesNoRelease,
    /// Placeholder: `{when}`.
    UpdatesFoundNewer,
    /// Placeholder: `{when}`.
    UpdatesOffline,
    /// Placeholder: `{when}`.
    UpdatesRateLimited,
    UpdatesRateLimitedLater,
    /// Placeholder: `{when}`.
    UpdatesFailed,
    UpdatesStore,
    UpdatesSource,
    /// Placeholder: `{version}`.
    UpdateAvailable,
    /// Placeholders: `{when}`, `{current}`.
    UpdatePublished,
    UpdateWhatsNew,
    UpdateReleasePage,
    UpdateDownload,
    /// Placeholder: `{percent}`.
    UpdateDownloading,
    UpdateCancel,
    UpdateDownloadFailed,
    UpdateDownloadAgain,
    UpdateReady,
    UpdateInstall,
    /// Placeholder: `{version}`.
    UpdateReviewTitle,
    UpdateReviewBody,
    UpdateReviewAgent,
    UpdateInstallAndRestart,
    UpdateStarting,
    UpdateNoInstaller,
    UpdateAlreadyDownloading,
    UpdateFileGone,
    UpdateChecksumChanged,
    UpdateNotStarted,
    UpdateWaitsForJobs,
    UpdateWaitsForRestore,
    UpdateOtherWindow,
    UpdateBannerSee,
    UpdateBannerLater,
    UpdateNotesOpenAbout,
    UpdateNotesClose,
    UpdateDownloadUnreachable,
    UpdateWaitingToRetry,
    UpdateSwitchNotSaved,
    UpdateNoneAvailable,
    SettingsBackupTab,
    BackupTitle,
    BackupSummary,
    BackupIncludeMedia,
    BackupMediaHint,
    /// Placeholders: `{size}`, `{files}`.
    BackupMediaSize,
    BackupMediaMeasuring,
    BackupMediaNone,
    BackupMediaUnknown,
    BackupBackUp,
    /// Placeholders: `{percent}`.
    BackupBackingUp,
    /// Placeholders: `{path}`.
    BackupSaved,
    BackupShow,
    BackupFailed,
    BackupRunning,
    BackupCancel,
    BackupAutomaticTitle,
    BackupAutomaticSwitch,
    BackupAutomaticHint,
    BackupAutomaticNone,
    /// Placeholders: `{when}`, `{size}`.
    BackupAutomaticRow,
    BackupOpenFolder,
    BackupRestoreTitle,
    BackupRestoreSummary,
    SettingsFileNotOne,
    SettingsFileNewer,
    SettingsFileWrongPassword,
    SettingsFileNotSaved,
    SettingsFilePasswordShort,
    SettingsFilePasswordsDiffer,
    SettingsFileNothingTicked,
    SettingsFileUnreadable,
    SettingsFilePreferences,
    SettingsFileCosts,
    SettingsFileSignInLabel,
    SettingsFileItemFailed,
    SettingsFileSomeNotRead,
    SettingsFileItemNotRead,
    SettingsFileTitle,
    SettingsFileSummary,
    SettingsFileExport,
    SettingsFileImport,
    SettingsFileExportTitle,
    SettingsFileGroupKeys,
    SettingsFileGroupApps,
    SettingsFileGroupPreferences,
    SettingsFileGroupPersonas,
    SettingsFileGroupChannels,
    SettingsFileGroupSignIns,
    SettingsFileSignInsHint,
    SettingsFilePassword,
    SettingsFilePasswordAgain,
    SettingsFilePasswordHint,
    SettingsFileSave,
    SettingsFileSaved,
    SettingsFileMade,
    SettingsFileOpen,
    SettingsFileImportTitle,
    SettingsFileAdds,
    SettingsFileReplaces,
    SettingsFileModelNotOffered,
    SettingsFilePartsNotRead,
    SettingsFileReplaceNote,
    SettingsFileBringIn,
    SettingsFileBrought,
    SettingsFileNotBrought,
    SettingsOfferFirst,
    SettingsOfferFile,
    SettingsOfferOpen,
    SettingsOfferNotNow,
    DevCopyChip,
    AboutDevCopy,
    BackupRestoreFromFile,
    BackupRestore,
    BackupChecking,
    BackupReviewTitle,
    /// Placeholders: `{when}`, `{version}`.
    BackupReviewMade,
    /// Placeholders: `{database}`, `{media}`, `{files}`.
    BackupReviewWithMedia,
    /// Placeholders: `{database}`.
    BackupReviewWithoutMedia,
    BackupReviewWarning,
    BackupReviewPosts,
    BackupRestoreAndRestart,
    /// Placeholders: `{percent}`.
    BackupRestoring,
    BackupWaitingForAgent,
    BackupRestarting,
    BackupRestoreFailed,
    RestoreWaitsForJobs,
    RestoreOtherWindow,
    BackupKeptAside,
    RestoreRunning,
    BackupNotABackup,
    BackupDamaged,
    BackupNewer,
    BackupUnreadable,
    BackupRestoredTitle,
    /// Placeholders: `{when}`.
    BackupRestored,
    BackupRolledBack,
    BackupSetUpAgain,
    /// Placeholders: `{provider}`.
    BackupSetUpKey,
    BackupSetUpPlan,
    /// Placeholders: `{network}`.
    BackupSetUpCredentials,
    /// Placeholders: `{handle}`, `{network}`.
    BackupSetUpReconnect,
    BackupSetUpOpen,
    BackupNothingToSetUp,
    BackupDone,
    UploadReelStartScheduled,
    /// One TikTok spec problem, by `TikTokSpecProblem::code`.
    UploadTikTokSpec(&'static str),
    UploadTikTokSpecsTitle,
    UploadDraftHint,
    UploadDraftNotice,
    UploadFieldDraftCaption,
    UploadFieldPostText,
    UploadDraftAiLabel,
    UploadDraftAiLabelHint,
    UploadDraftIrreversible,
    UploadDraftStart,
    UploadDraftProcessingHint,
    UploadStateDraftSent,
    UploadDraftSentHint,
    UploadDraftAiReminder,
    UploadDraftLinkHint,
    UploadDraftCopy,
    UploadDraftCopied,
    UploadDraftLimitHint,
    UploadDraftLimitHeldHint,
    UploadDraftLimitRecheckHint,
    MissedTitle,
    MissedHint,
    MissedDue,
    MissedSendNow,
    MissedNewTime,
    MissedSave,
    MissedCancel,
    MissedCancelConfirm,
    MissedCancelYes,
    MissedKeep,
    MissedLater,
    MissedLaterHint,
    MissedSent,
    MissedRescheduled,
    MissedCancelled,
    MissedNotMissed,
    MissedNotUpdated,
    ScheduleProblem(ScheduleProblem),
    /// A tour by name, in the Guide menu.
    TourName(TourId),
    /// A tour step's title, by tour and step key.
    TourStepTitle(TourId, &'static str),
    /// A tour step's body: at most three sentences.
    TourStepBody(TourId, &'static str),
    /// Placeholders: `{n}`, `{count}`.
    TourStepCount,
    TourEscHint,
    TourBack,
    TourNext,
    TourFinish,
    TourSkip,
    TourNew,
    TourOfferTitle,
    TourOfferBody,
    TourOfferStart,
    TourOfferNotNow,
    TourOfferNever,
    GuideResumeTour,
    GuideShortcuts,
    GuideResetTours,
    GuideResetFailed,
    ShortcutsTitle,
    ShortcutsClose,
    /// The Guide menu's first row, and the Guide screen.
    GuideUserGuide,
    GuideGroupName(GuideGroup),
    GuideSearchPlaceholder,
    /// What the guide's keys do, behind an ⓘ.
    GuideSearchKeys,
    /// Placeholder: `{query}`.
    GuideSearchEmpty,
    GuideOnThisPage,
    /// Starts the page's tour.
    GuideShowMe,
    /// Placeholder: `{place}`.
    GuideGoTo,
    /// A tour card's way into the guide.
    TourLearnMore,
    /// A screen's button, and the Guide menu row, that start its tour.
    TourThisScreen,
    TourThisList,
    /// The Jobs panel's tour button.
    TourThisPanel,
    /// A project stage's button, and the Guide menu row, that start its
    /// tour.
    TourThisStage,
    /// An ⓘ's link to the guide section that says more.
    GuideMoreInGuide,
    /// Over the editor, the guide's way back to it.
    GuideBackToEditor,
    /// The Appearance tab's tour settings.
    ToursSettingTitle,
    ToursSettingOffer,
    ToursSettingHint,
    ToursSettingNotSaved,
    /// A group of keyboard shortcuts, by key.
    ShortcutGroup(&'static str),
    /// What a keyboard shortcut does, by key.
    ShortcutAction(&'static str),
    // Products (#273).
    ProductsHint,
    ProductsEmpty,
    ProductsNotLoaded,
    NewProduct,
    /// Placeholder: `{n}`.
    ProductsShowArchived,
    NewProductTitle,
    EditProductTitle,
    ProductBriefHint,
    ProductName,
    ProductNamePlaceholder,
    ProductKind,
    ProductKindHint,
    ProductOwnership,
    ProductOwnershipHint,
    ProductDescription,
    ProductDescriptionPlaceholder,
    ProductAudience,
    ProductAudiencePlaceholder,
    ProductFacts,
    ProductFactsPlaceholder,
    ProductFactsHint,
    /// Placeholder: `{n}`.
    ProductFactsCount,
    ProductOffer,
    ProductOfferPlaceholder,
    ProductLink,
    ProductLinkPlaceholder,
    ProductCallToAction,
    ProductCallToActionPlaceholder,
    ProductMustSay,
    ProductMustSayPlaceholder,
    ProductNeverSay,
    ProductNeverSayPlaceholder,
    CreateProduct,
    SaveProduct,
    DuplicateProduct,
    ArchiveProduct,
    RestoreProduct,
    DeleteProduct,
    /// Placeholder: `{name}`.
    ProductDeleteTitle,
    ProductDeleteHint,
    ProductActionsHint,
    ConfirmDeleteProduct,
    ProductCancel,
    ProductSaved,
    /// Placeholder: `{name}`.
    ProductDuplicated,
    ProductArchived,
    ProductRestored,
    /// Placeholder: `{name}`.
    ProductDeleted,
    ProductArchivedTag,
    ProductCopySuffix,
    ProductNameTaken,
    ProductNotFound,
    ProductInUse,
    ProductNotSaved,
    ProductPicturesTitle,
    ProductPicturesHint,
    ProductPicturesSaveFirst,
    ProductPicturesNotLoaded,
    ProductImagesNone,
    /// Placeholder: `{n}`.
    ProductImageCount,
    AddProductImage,
    ProductImageKindHint,
    ProductImageCutOut,
    /// Placeholders: `{width}`, `{height}`.
    ProductImageSize,
    RemoveProductImage,
    /// Placeholder: `{when}`.
    ProductRightsConfirmedAt,
    ProductRightsTitle,
    ProductRightsHint,
    ProductRightsConfirm,
    ProductChooseImage,
    ProductChooseLogo,
    ProductImageDialog,
    ProductLogoDialog,
    ProductLogoTitle,
    ProductLogoHint,
    ProductLogoNone,
    AddProductLogo,
    ReplaceProductLogo,
    RemoveProductLogo,
    ProductImageRemoveAsk,
    ProductLogoRemoveAsk,
    ProductImageNotConfirmed,
    ProductTooManyImages,
    ProductImageRefused,
    ProductImageUnreadable,
    ProductImageNotFound,
    ProductNoLogo,
    ProductImageAdded,
    ProductImageRemoved,
    ProductLogoAdded,
    ProductLogoRemoved,
    SettingsFileGroupProducts,
    ProductKindName(ProductKind),
    ProductOwnershipName(ProductOwnership),
    ProductImageKindName(ProductImageKind),
    ProductFieldError(ProductFieldError),
    /// Why a picture cannot be a product image or logo. Placeholders:
    /// `{width}`, `{height}`, `{mb}`.
    ProductImageReason(ProductImageError),
    // The product sheet (#274).
    ProductTabBrief,
    ProductTabSheet,
    SheetTitle,
    SheetHint,
    SheetInfo,
    SheetSaveFirst,
    SheetBriefUnsaved,
    SheetNotLoaded,
    SheetFillTitle,
    SheetResearch,
    SheetResearchAgain,
    /// Placeholders: `{fetches}`, `{searches}`.
    SheetResearchHint,
    SheetResearchNoLink,
    /// Placeholders: `{amount}`.
    SheetResearchCost,
    /// Placeholders: `{amount}`.
    SheetResearchUsual,
    SheetFromBrief,
    SheetFromBriefHint,
    /// Placeholders: `{amount}`.
    SheetFromBriefCost,
    SheetByHand,
    SheetByHandHint,
    SheetFixFirst,
    SheetResearching,
    SheetWriting,
    SheetJobStopped,
    SheetJobStopping,
    SheetJobFailed,
    SheetJobLost,
    /// Placeholders: `{reason}`.
    SheetJobDeclined,
    /// Placeholders: `{n}`.
    SheetJobDone,
    /// Placeholders: `{n}`.
    SheetJobLeftOut,
    SheetBusyNote,
    /// Placeholders: `{category}`.
    SheetRestrictedNotice,
    SheetAboutTitle,
    SheetSummary,
    SheetCategory,
    SheetRestricted,
    SheetRestrictedHint,
    SheetRestrictedNone,
    SheetLooks,
    SheetSegmentsTitle,
    SheetSegmentName,
    SheetSegmentWants,
    SheetSegmentHoldsBack,
    SheetAddSegment,
    SheetObjectionsTitle,
    SheetObjection,
    SheetObjectionAnswer,
    SheetAddObjection,
    SheetAlternativesTitle,
    SheetAlternativeName,
    SheetAlternativeDiffers,
    SheetAddAlternative,
    SheetRemoveRow,
    SheetRowsEmpty,
    SaveSheet,
    SheetSaved,
    SheetClaimsTitle,
    SheetClaimsHint,
    /// Placeholders: `{n}`.
    SheetClaimsWaiting,
    /// Placeholders: `{n}`.
    SheetClaimsConfirmed,
    /// Placeholders: `{n}`.
    SheetClaimsRejected,
    SheetClaimsNone,
    ConfirmClaim,
    RejectClaim,
    BringClaimBack,
    EditClaim,
    SaveClaim,
    CancelClaimEdit,
    ClaimSensitive,
    ClaimSensitiveHint,
    /// Placeholders: `{when}`.
    ClaimConfirmedAt,
    /// Placeholders: `{when}`.
    ClaimEditedAt,
    ClaimOpenSource,
    ClaimSourceTyped,
    ClaimSourceBrief,
    ClaimSourcePage,
    ClaimSourceWeb,
    AddClaim,
    AddClaimPlaceholder,
    SheetSourcesTitle,
    /// Placeholders: `{when}`.
    SheetResearchedAt,
    SheetSourcesNone,
    SheetNone,
    SheetMissingKey,
    SheetBusy,
    SheetNotSaved,
    ClaimKindName(ClaimKind),
    RestrictedCategoryName(RestrictedCategory),
    SheetFieldError(SheetFieldError),
    SheetClaimError(SheetClaimError),
    AdAngleNotSaved,
    AdAngleNotFound,
    AdAnglesBusy,
    AdAnglesNothingToRank,
    AdAnglesNotLoaded,
    AdAngleFieldError(AdAngleFieldError),
    /// What a product lacks before angles can be proposed for it.
    AdAnglesMissing(AdAnglesMissing),
    /// Only Claude and TypeSafe have a message.
    AdAnglesMissingKey(Provider),
    StageBriefToConfirm,
    StageBriefClaimChanged,
    /// Placeholder: `{n}`.
    StageBriefClaimsChanged,
    StageBriefIncomplete,
    StageBriefClaim,
    /// Placeholder: `{n}`.
    StageBriefClaims,
    StageScriptBriefChanged,
    ScriptBriefFirst,
    VersionAdProject,
    BriefNotAnAd,
    BriefClaimNotConfirmed,
    BriefClaimNotUsed,
    BriefNotSaved,
    BriefNotLoaded,
    /// What stands in the way of confirming a brief.
    BriefNotReady(BriefNotReady),
    BriefFieldError(BriefFieldError),
    /// The tab of a product that lists its ad angles.
    ProductTabAngles,
    AdAnglesTitle,
    AdAnglesInfo,
    AdAnglesHint,
    AdAnglesSaveFirst,
    AdAnglesBriefUnsaved,
    AdAnglesProposeTitle,
    AdAnglesChannel,
    AdAnglesChannelHint,
    SuggestAdAngles,
    SuggestAdAnglesHint,
    RankAdAngles,
    AdAnglesUnranked,
    AdAnglesRankCost,
    AdAnglesRunning,
    AdAnglesStopped,
    AdAnglesEmpty,
    AdAnglesDiscarded,
    AdAngleApproved,
    AdAngleNotRanked,
    /// Bardo's own mark on an angle, no model's.
    AdAngleSensitive,
    AdAngleFramework,
    AdAngleSegment,
    AdAngleHook,
    AdAngleClaims,
    AdAngleClaimsNone,
    AdAngleClaimGone,
    AdAngleHookScore,
    AdAngleAudienceFit,
    AdAngleClarity,
    ApproveAdAngle,
    ApproveAdAngleHint,
    AdAngleEditHint,
    AdAngleStarted,
    AdAngleOpenProject,
    /// Marks an ad project in the list of projects.
    ProjectAd,
    ProjectAdFor,
    ScriptBriefChanged,
    ScriptKeepAfterBrief,
    BriefInfo,
    BriefHint,
    BriefAngleTitle,
    BriefProduct,
    BriefClaimsTitle,
    BriefClaimsInfo,
    BriefClaimsHint,
    BriefClaimsNone,
    BriefClaimsLeftOut,
    BriefClaimInAngle,
    BriefClaimLeaveOut,
    BriefClaimUse,
    BriefClaimEdited,
    BriefClaimUnconfirmed,
    BriefClaimKeep,
    BriefClaimTake,
    BriefWordsTitle,
    BriefWordsInfo,
    BriefLink,
    BriefSave,
    BriefSaved,
    BriefLengthHint,
    BriefConfirm,
    BriefConfirmHint,
    BriefConfirmed,
    BriefConfirmedNotice,
    BriefToScript,
    BriefAfterConfirm,
    /// An ad framework, as pickers and angle cards name it.
    AdFrameworkName(AdFramework),
    /// What a script in an ad framework goes through, in one line.
    AdFrameworkSteps(AdFramework),
    /// An ad's script is asked for while its confirmed brief is incomplete again.
    ScriptBriefNotReady,
    /// An ad project's hint on Generate script. Placeholder: `{n}`.
    GenerateAdScriptHint,
    ScriptClaimsRunning,
    ScriptClaimsStopped,
    /// The claims check of an ad script (#276).
    ScriptClaimsTitle,
    ScriptClaimsInfo,
    ScriptClaimsNotChecked,
    ScriptClaimsClean,
    /// Placeholder: `{n}`.
    ScriptClaimsUnbacked,
    ScriptClaimsUnbackedOne,
    ScriptClaimsOutdated,
    ScriptClaimsOutdatedHint,
    ScriptClaimsUnbackedTitle,
    ScriptClaimsNoSentence,
    ScriptClaimsUsedTitle,
    ScriptClaimsSensitiveTitle,
    ScriptClaimsNoneUsed,
    CheckScriptClaims,
    CheckScriptClaimsAgain,
    /// Placeholder: `{when}`, as `time_ago` says it.
    ScriptClaimsCheckedAt,
    ScriptClaimsPendingMissing,
    /// Beside a narration's estimate, for an ad. Placeholder: `{n}`.
    NarrationClaimsUnbacked,
    NarrationClaimsUnbackedOne,
    NarrationClaimsNotChecked,
    /// Beside the check button while the editor holds unsaved edits.
    ScriptClaimsSaveFirst,
    /// A script job that saved its script and failed in the check.
    ScriptWrittenCheckStopped,
    /// The out-of-date hint of a new script's check, which has no button of its own.
    ScriptClaimsPendingOutdatedHint,
    /// The chip of a check that found no statement at all.
    ScriptClaimsNothingStated,
    /// A scene cannot be of the kind asked for (#278).
    SceneKindNotAllowed,
    SceneKindLabel,
    SceneKindHint,
    SceneProductImage,
    SceneComposed,
    SceneImageTooSmall,
    ComposeScene,
    /// Placeholders: `{n}`.
    ComposeScenes,
    SceneNotComposed,
    SceneClipComposed,
    EditorMotionComposed,
    /// What a scene of an ad is (#278).
    SceneKindName(SceneKind),
    /// A scene card whose composed picture is still to be made (#278).
    SceneCardToCompose,
    /// Where a composed picture comes from (#278).
    EditorSourceComposed,
    /// A composed scene whose product image was removed (#278).
    SceneImageRemoved,
    /// A drawn backdrop that was removed or never kept.
    ShotBackdropGone,
    /// A backdrop a scene shows cannot be discarded.
    ShotBackdropInUse,
    ShotLayoutLabel,
    ShotLayoutHint,
    ShotPlacementLabel,
    ShotBackdropLabel,
    ShotBackdropHint,
    ShotBackdropAuto,
    ShotBackdropFrom,
    ShotBackdropTo,
    ShotBackdropDrawn,
    DrawShotBackdrop,
    DrawShotBackdropHint,
    DrawingShotBackdrop,
    ShotBackdropToReview,
    KeepShotBackdrop,
    DiscardShotBackdrop,
    ProductLayoutName(ProductLayout),
    ShotPlacementName(ShotPlacement),
    BackdropDirectionName(BackdropDirection),
    /// A render refused while scenes of the product in use wait for their check (#279).
    RenderProductUnchecked,
    RenderProductUncheckedScenes,
    RenderEndCardOutOfDate,
    ProductCheckRefused,
    ProductUncheckedClip,
    ProductCheckTitle,
    ProductCheckHint,
    ProductMatches,
    ProductCheckedNote,
    ProductDrawAgain,
    ProductUseRealImage,
    ProductNotDrawn,
    ProductRealImage,
    ProductToCheckTag,
    ClipProductCheckTitle,
    ClipProductCheckHint,
    ClipProductMatches,
    ClipProductCheckedNote,
    /// A product of kind idea's drawn concept image (#280).
    ConceptTitle,
    ConceptHint,
    DrawConcept,
    DrawConceptAgain,
    ConceptDrawingNow,
    ConceptNewTitle,
    KeepConcept,
    DiscardConcept,
    ConceptKeptNote,
    ConceptUnusedNote,
    ConceptNotOffered,
    ConceptMissingClaudeKey,
    ConceptMissingImageKey,
    ConceptBusy,
    ConceptNothingWaiting,
    ConceptNotSaved,
    ConceptBadge,
    ConceptBadgeHint,
    ConceptStopped,
    ConceptFailed,
    ConceptKept,
    ConceptCheckTitle,
    ConceptCheckHint,
    ConceptUseImage,
}

impl Text {
    fn key(self) -> Cow<'static, str> {
        let key = match self {
            Text::AppName => "app.name",
            Text::OverviewHint => "overview.hint",
            Text::OverviewNotLoaded => "overview.not_loaded",
            Text::OverviewNoProject => "overview.no_project",
            Text::OverviewPickUp => "overview.pick_up",
            Text::OverviewNoVideos => "overview.no_videos",
            Text::OverviewContinue => "overview.continue",
            Text::OverviewOpen => "overview.open",
            Text::OverviewNextLabel => "overview.next",
            Text::OverviewNextWithCost => "overview.next_cost",
            Text::OverviewWorking => "overview.working",
            Text::OverviewWaitingAt => "overview.waiting_at",
            Text::OverviewWaiting => "overview.waiting",
            Text::OverviewIdeasTitle => "overview.ideas",
            Text::OverviewIdeasNone => "overview.ideas_none",
            Text::OverviewIdeasUnranked => "overview.ideas_unranked",
            Text::OverviewIdeasRanked => "overview.ideas_ranked",
            Text::OverviewIdeasOrphans => "overview.ideas_orphans",
            Text::OverviewOpenIdeas => "overview.open_ideas",
            Text::OverviewChannels => "overview.channels",
            Text::OverviewSpent => "overview.spent",
            Text::OverviewNarrators => "overview.narrators",
            Text::OverviewNoNarrator => "overview.no_narrator",
            Text::OverviewAd => "overview.ad",
            Text::StepConfirmBrief => "overview.step.confirm_brief",
            Text::StepDecideClaims => "overview.step.decide_claims",
            Text::StepCompleteBrief => "overview.step.complete_brief",
            Text::StepWriteScript => "overview.step.write_script",
            Text::StepReviewScript => "overview.step.review_script",
            Text::StepRewriteScript => "overview.step.rewrite_script",
            Text::StepRecordNarration => "overview.step.record_narration",
            Text::StepRecordAgain => "overview.step.record_again",
            Text::StepFixTakes => "overview.step.fix_takes",
            Text::StepPlanScenes => "overview.step.plan_scenes",
            Text::StepPlaceScenes => "overview.step.place_scenes",
            Text::StepPlanAgain => "overview.step.plan_again",
            Text::StepReviewImages => "overview.step.review_images",
            Text::StepDrawImages => "overview.step.draw_images",
            Text::StepReviewClips => "overview.step.review_clips",
            Text::StepAnimateClips => "overview.step.animate_clips",
            Text::StepBuildCut => "overview.step.build_cut",
            Text::StepRender => "overview.step.render",
            Text::StepRenderAgain => "overview.step.render_again",
            Text::StepResumeRender => "overview.step.resume_render",
            Text::StepExport => "overview.step.export",
            Text::StepExportAgain => "overview.step.export_again",
            Text::StepExportRest => "overview.step.export_rest",
            Text::StepResumeExport => "overview.step.resume_export",
            Text::StepPost => "overview.step.post",
            Text::LibraryHint => "library.hint",
            Text::CostsUnknownProject => "project_spend.unknown",
            Text::LooseNotSaved => "loose.not_saved",
            Text::LooseVideoNotFound => "loose.not_found",
            Text::LooseMoveBusy => "loose.move_busy",
            Text::LooseMoveArchived => "loose.move_archived",
            Text::LooseMoveVideoArchived => "loose.move_video_archived",
            Text::ChannelNeedsProject => "channel.error.needs_project",
            Text::LooseSwitcherRow => "loose.switcher_row",
            Text::LooseCountNone => "loose.count_none",
            Text::LooseCountOne => "loose.count_one",
            Text::LooseCountMany => "loose.count_many",
            Text::LooseTitle => "loose.title",
            Text::LooseHint => "loose.hint",
            Text::LooseNew => "loose.new",
            Text::LooseFormTitle => "loose.form_title",
            Text::LooseFormHint => "loose.form_hint",
            Text::LooseIdeaTitle => "loose.idea_title",
            Text::LooseIdeaTitlePlaceholder => "loose.idea_title_placeholder",
            Text::LooseIdeaAngle => "loose.idea_angle",
            Text::LooseIdeaAnglePlaceholder => "loose.idea_angle_placeholder",
            Text::LooseCopyFrom => "loose.copy_from",
            Text::LooseCopyDefaults => "loose.copy_defaults",
            Text::LooseCreate => "loose.create",
            Text::LooseCancel => "loose.cancel",
            Text::LooseEmpty => "loose.empty",
            Text::LooseExportOnly => "loose.export_only",
            Text::LooseFileTarget => "loose.file_target",
            Text::LooseMove => "loose.move",
            Text::LooseMoveTitle => "loose.move_title",
            Text::LooseMoveHint => "loose.move_hint",
            Text::LooseMoveChannel => "loose.move_channel",
            Text::LooseMoveConfirm => "loose.move_confirm",
            Text::LooseMoveNoChannels => "loose.move_no_channels",
            Text::LooseMoved => "loose.moved",
            Text::LooseDiffersLanguage => "loose.differs_language",
            Text::LooseDiffersCountry => "loose.differs_country",
            Text::LooseOverview => "loose.overview",
            Text::LooseOpenVideos => "loose.open_videos",
            Text::OwnIdea => "own_idea.button",
            Text::OwnIdeaTitle => "own_idea.title",
            Text::OwnIdeaHint => "own_idea.hint",
            Text::OwnIdeaChannel => "own_idea.channel",
            Text::OwnIdeaSave => "own_idea.save",
            Text::OwnIdeaCancel => "own_idea.cancel",
            Text::OwnIdeaSaved => "own_idea.saved",
            Text::DeleteLooseVideoHint => "loose.delete_hint",
            Text::SwitcherNoProject => "project_switcher.no_project",
            Text::SwitcherSearch => "project_switcher.search",
            Text::SwitcherFavorites => "project_switcher.favorites",
            Text::SwitcherRecent => "project_switcher.recent",
            Text::SwitcherAll => "project_switcher.all",
            Text::SwitcherNewProject => "project_switcher.new_project",
            Text::SwitcherStar => "project_switcher.star",
            Text::SwitcherUnstar => "project_switcher.unstar",
            Text::SwitcherMoveUp => "project_switcher.move_up",
            Text::SwitcherNoMatch => "project_switcher.no_match",
            Text::SwitcherEmpty => "project_switcher.empty",
            Text::SwitcherOpen => "project_switcher.open",
            Text::ProjectChannelsNone => "project_switcher.channels_none",
            Text::ProjectChannelsOne => "project_switcher.channels_one",
            Text::ProjectChannelsMany => "project_switcher.channels_many",
            Text::ProjectNeverOpened => "project_switcher.never_opened",
            Text::NewProjectTitle => "project_form.title",
            Text::NewProjectHint => "project_form.hint",
            Text::ProjectNameLabel => "project_form.name",
            Text::ProjectNamePlaceholder => "project_form.name_placeholder",
            Text::ProjectColorLabel => "project_form.color",
            Text::CreateProject => "project_form.create",
            Text::ProjectFormCancel => "project_form.cancel",
            Text::ProjectNameRequired => "project_error.name_required",
            Text::ProjectNameTooLong => "project_error.name_too_long",
            Text::ProjectNameTaken => "project_error.name_taken",
            Text::ProjectNotFoundAny => "project_error.not_found",
            Text::ProjectNotSaved => "project_error.not_saved",
            Text::ProjectListNotLoaded => "project_error.not_loaded",
            Text::ProjectFolderTooDeep => "project_error.folder_too_deep",
            Text::ProjectFolderNotMade => "project_error.folder_not_made",
            Text::ProjectFolderLabel => "project_form.folder",
            Text::ProjectFolderChoose => "project_form.folder_choose",
            Text::ProjectFolderHint => "project_form.folder_hint",
            Text::JobWaitsForFolder => "job.waits_for_folder",
            Text::FolderNotFound => "project_folder.not_found",
            Text::FolderNotFoundHint => "project_folder.not_found_hint",
            Text::WorldNotFound => "world.error.not_found",
            Text::WorldNameTaken => "world.error.name_taken",
            Text::CharacterInUse => "world.error.in_use",
            Text::WorldNotSaved => "world.error.not_saved",
            Text::WorldVideoBusy => "world.error.video_busy",
            Text::WorldHint => "world.hint",
            Text::WorldEmpty => "world.empty",
            Text::WorldNotLoaded => "world.not_loaded",
            Text::WorldNew => "world.new",
            Text::WorldShowArchived => "world.show_archived",
            Text::WorldTabCharacters => "world.tab.characters",
            Text::WorldTabPlaces => "world.tab.places",
            Text::WorldTabObjects => "world.tab.objects",
            Text::WorldNewPlace => "world.new_place",
            Text::WorldNewObject => "world.new_object",
            Text::WorldEmptyPlaces => "world.empty_places",
            Text::WorldEmptyObjects => "world.empty_objects",
            Text::PlaceNewTitle => "place.new_title",
            Text::PlaceEditTitle => "place.edit_title",
            Text::PlaceFormHint => "place.form_hint",
            Text::PlaceNamePlaceholder => "place.name_placeholder",
            Text::PlaceAppearance => "place.appearance",
            Text::PlaceAppearancePlaceholder => "place.appearance_placeholder",
            Text::PlaceAppearanceHint => "place.appearance_hint",
            Text::PlaceAlwaysPlaceholder => "place.always_placeholder",
            Text::PlaceNeverPlaceholder => "place.never_placeholder",
            Text::PlaceCreate => "place.create",
            Text::ObjectNewTitle => "object.new_title",
            Text::ObjectEditTitle => "object.edit_title",
            Text::ObjectFormHint => "object.form_hint",
            Text::ObjectNamePlaceholder => "object.name_placeholder",
            Text::ObjectAppearancePlaceholder => "object.appearance_placeholder",
            Text::ObjectAppearanceHint => "object.appearance_hint",
            Text::ObjectAlwaysPlaceholder => "object.always_placeholder",
            Text::ObjectNeverPlaceholder => "object.never_placeholder",
            Text::ObjectCreate => "object.create",
            Text::ObjectBelongsTo => "object.belongs_to",
            Text::ObjectBelongsToNobody => "object.belongs_to_nobody",
            Text::ObjectBelongsToHint => "object.belongs_to_hint",
            Text::WorldNoProject => "world.no_project",
            Text::WorldOwnerProject => "world.owner_project",
            Text::WorldOwnerGlobal => "world.owner_global",
            Text::CharacterNewTitle => "character.new_title",
            Text::CharacterEditTitle => "character.edit_title",
            Text::CharacterFormHint => "character.form_hint",
            Text::CharacterName => "character.name",
            Text::CharacterNamePlaceholder => "character.name_placeholder",
            Text::CharacterAppearance => "character.appearance",
            Text::CharacterAppearancePlaceholder => "character.appearance_placeholder",
            Text::CharacterAppearanceHint => "character.appearance_hint",
            Text::CharacterAlways => "character.always",
            Text::CharacterAlwaysPlaceholder => "character.always_placeholder",
            Text::CharacterNever => "character.never",
            Text::CharacterNeverPlaceholder => "character.never_placeholder",
            Text::CharacterNotesHint => "character.notes_hint",
            Text::CharacterCreate => "character.create",
            Text::CharacterSave => "character.save",
            Text::CharacterDuplicate => "character.duplicate",
            Text::CharacterArchive => "character.archive",
            Text::CharacterRestore => "character.restore",
            Text::CharacterDelete => "character.delete",
            Text::CharacterDeleteTitle => "character.delete_title",
            Text::CharacterDeleteHint => "character.delete_hint",
            Text::CharacterConfirmDelete => "character.confirm_delete",
            Text::CharacterCancel => "character.cancel",
            Text::CharacterActionsHint => "character.actions_hint",
            Text::CharacterSaved => "character.saved",
            Text::CharacterDuplicated => "character.duplicated",
            Text::CharacterArchived => "character.archived",
            Text::CharacterRestored => "character.restored",
            Text::CharacterDeleted => "character.deleted",
            Text::CharacterArchivedTag => "character.archived_tag",
            Text::CharacterDropsTitle => "character.drops_title",
            Text::CharacterDropsPictures => "character.drops_hint",
            Text::CharacterDropsNone => "character.drops_none",
            Text::CharacterDropsConfirm => "character.drops_confirm",
            Text::CharacterUses => "character.uses",
            Text::CharacterUsesNone => "character.uses_none",
            Text::CharacterUseArchived => "character.use_archived",
            Text::CharacterCopySuffix => "character.copy_suffix",
            Text::CharacterBelongings => "character.belongings",
            Text::CharacterBelongingsNone => "character.belongings_none",
            Text::WorldPicturesTitle => "world.pictures.title",
            Text::WorldPicturesHint => "world.pictures.hint",
            Text::WorldPicturesSaveFirst => "world.pictures.save_first",
            Text::WorldPicturesNone => "world.pictures.none",
            Text::WorldPictureLook => "world.pictures.look",
            Text::WorldPictureLookNotes => "world.pictures.look_notes",
            Text::WorldPictureDraw => "world.pictures.draw",
            Text::WorldPictureDrawing => "world.pictures.drawing",
            Text::WorldPictureWaiting => "world.pictures.waiting",
            Text::WorldPictureKeep => "world.pictures.keep",
            Text::WorldPictureDiscard => "world.pictures.discard",
            Text::WorldPictureRemove => "world.pictures.remove",
            Text::WorldPictureRemoveAsk => "world.pictures.remove_ask",
            Text::WorldPictureKept => "world.pictures.kept",
            Text::WorldPictureDiscarded => "world.pictures.discarded",
            Text::WorldPictureRemoved => "world.pictures.removed",
            Text::WorldPictureStarted => "world.pictures.started",
            Text::WorldPictureBusy => "world.pictures.busy",
            Text::WorldPictureNothingWaiting => "world.pictures.nothing_waiting",
            Text::WorldPictureMissingKey => "world.pictures.missing_key",
            Text::WorldPictureOverBudget => "world.pictures.over_budget",
            Text::WorldImportNotConfirmed => "world.import.not_confirmed",
            Text::WorldImportUnreadable => "world.import.unreadable",
            Text::WorldPictureImport => "world.import.button",
            Text::WorldImportTitle => "world.import.title",
            Text::WorldImportHint => "world.import.hint",
            Text::WorldImportConfirm => "world.import.confirm",
            Text::WorldImportChoose => "world.import.choose",
            Text::WorldImportDialog => "world.import.dialog",
            Text::WorldImported => "world.import.done",
            Text::WhoAppearsTitle => "who_appears.title",
            Text::WhoAppearsHint => "who_appears.hint",
            Text::WhoAppearsNone => "who_appears.none",
            Text::WhoAppearsNoneAvailable => "who_appears.none_available",
            Text::WhoAppearsPickable => "who_appears.pickable",
            Text::WhoAppearsAdd => "who_appears.add",
            Text::WhoAppearsRemove => "who_appears.remove",
            Text::WhoAppearsInstruction => "who_appears.instruction",
            Text::WhoAppearsInstructionPlaceholder => "who_appears.instruction_placeholder",
            Text::WhoAppearsSave => "who_appears.save",
            Text::WhoAppearsSaved => "who_appears.saved",
            Text::WhoAppearsChanged => "who_appears.changed",
            Text::WhoAppearsFull => "who_appears.full",
            Text::WhoAppearsCharacters => "who_appears.characters",
            Text::WhoAppearsPlaces => "who_appears.places",
            Text::WhoAppearsObjects => "who_appears.objects",
            Text::WhoAppearsBelongings => "who_appears.belongings",
            Text::WhoAppearsBelongsTo => "who_appears.belongs_to",
            Text::WhoAppearsFullPlaces => "who_appears.full_places",
            Text::WhoAppearsFullObjects => "who_appears.full_objects",
            Text::ScenesPlaceSaveProject => "scenes.place_world.save_project",
            Text::ScenesPlaceSaveGlobal => "scenes.place_world.save_global",
            Text::ScenesPlaceSaveHint => "scenes.place_world.hint",
            Text::ScenesPlaceFromWorld => "scenes.place_world.from_world",
            Text::WhoAppearsTooMany => "who_appears.error.too_many",
            Text::WhoAppearsInstructionTooLong => "who_appears.error.instruction_too_long",
            Text::CastSaveToWorld => "cast_world.save_project",
            Text::CastSaveToLibrary => "cast_world.save_global",
            Text::CastSaveHint => "cast_world.hint",
            Text::CastFromWorld => "cast_world.from_world",
            Text::WorldItemFieldError(error) => match error {
                WorldItemFieldError::NameRequired => "character.error.name_required",
                WorldItemFieldError::NameTooLong => "character.error.name_too_long",
                WorldItemFieldError::AppearanceRequired => "character.error.appearance_required",
                WorldItemFieldError::AppearanceTooLong => "character.error.appearance_too_long",
                WorldItemFieldError::NotesTooLong => "character.error.notes_too_long",
            },
            Text::WorldPictureOrigin(origin) => {
                return format!("world.pictures.origin.{}", origin.code()).into();
            }
            Text::LocateFolder => "project_folder.locate",
            Text::LocateFolderPrompt => "project_folder.locate_prompt",
            Text::ShowFolder => "project_folder.show",
            Text::SettingsStorageTab => "settings.storage_tab",
            Text::BaseFolderLabel => "storage.base_folder",
            Text::BaseFolderHint => "storage.base_folder_hint",
            Text::BaseFolderChange => "storage.change",
            Text::BaseFolderPrompt => "storage.prompt",
            Text::BaseFolderUseDefault => "storage.use_default",
            Text::BaseFolderIsDefault => "storage.is_default",
            Text::BaseFolderAvoidedSync => "storage.avoided_sync",
            Text::BaseFolderSynced => "storage.synced",
            Text::BaseFolderNotSaved => "storage.not_saved",
            Text::StorageMoveHint => "storage.move_hint",
            Text::LocateRefused(error) => match error {
                crate::LocateError::NotAProjectFolder => "locate_error.not_a_project_folder",
                crate::LocateError::OtherProject => "locate_error.other_project",
                crate::LocateError::OtherCopy => "locate_error.other_copy",
                crate::LocateError::NotFound => "locate_error.not_found",
                crate::LocateError::NotSaved => "locate_error.not_saved",
            },
            Text::PackageExportBusy => "package.export_busy",
            Text::PackageNotAPackage => "package.not_a_package",
            Text::PackageNewer => "package.newer",
            Text::PackageDamaged => "package.damaged",
            Text::PackageFolderNotEmpty => "package.folder_not_empty",
            Text::PackageNotRead => "package.not_read",
            Text::OpenFolderNotAProject => "open_folder.not_a_project",
            Text::OpenFolderOtherCopy => "open_folder.other_copy",
            Text::OpenFolderNoRecords => "open_folder.no_records",
            Text::DeleteRecordsNotWritten => "delete.records_not_written",
            Text::SwitcherExportProject => "switcher_package.export",
            Text::SwitcherExporting => "switcher_package.exporting",
            Text::SwitcherImportPackage => "switcher_package.import",
            Text::SwitcherOpenFolder => "switcher_package.open_folder",
            Text::PackageImportPrompt => "switcher_package.import_prompt",
            Text::OpenFolderPrompt => "switcher_package.open_folder_prompt",
            Text::PackageExportStarted => "switcher_package.export_started",
            Text::PackageImportStarted => "switcher_package.import_started",
            Text::FolderRelinked => "switcher_package.relinked",
            Text::FolderReopened => "switcher_package.reopened",
            Text::FolderOpenedArchived => "switcher_package.opened_archived",
            Text::ImportPackageTitle => "package_import.title",
            Text::ImportPackageHint => "package_import.hint",
            Text::ImportPackageFrom => "package_import.from",
            Text::ImportPackageContents => "package_import.contents",
            Text::ImportPackageTemplates => "package_import.templates",
            Text::ImportPackageTemplatesHint => "package_import.templates_hint",
            Text::ImportPackageConfirm => "package_import.confirm",
            Text::MoveChannel => "move_channel.button",
            Text::MoveChannelTitle => "move_channel.title",
            Text::MoveChannelHint => "move_channel.hint",
            Text::MoveChannelTarget => "move_channel.target",
            Text::MoveChannelConfirm => "move_channel.confirm",
            Text::MoveChannelDone => "move_channel.done",
            Text::MoveChannelBlocked => "move_channel.blocked",
            Text::MoveChannelNoOther => "move_channel.no_other",
            Text::ChannelProjectLabel => "move_channel.in_project",
            Text::FilterAllProjects => "project_filter.all",
            Text::FilterNoProject => "project_filter.none",
            Text::FilterProjectLabel => "project_filter.label",
            Text::CostsByProject => "project_filter.costs_by_project",
            Text::ProjectColorName(color) => {
                return format!("project_color.{}", color.code()).into();
            }
            Text::MoveChannelBlock(block) => match block {
                crate::MoveBlock::ScheduledPost => "move_channel_block.scheduled_post",
                crate::MoveBlock::HeldOnNetwork => "move_channel_block.held_on_network",
                crate::MoveBlock::ActiveJob => "move_channel_block.active_job",
                crate::MoveBlock::VersionElsewhere => "move_channel_block.version_elsewhere",
                crate::MoveBlock::ProjectItems => "move_channel_block.project_items",
            },
            Text::RemovalNotFound => "removal.not_found",
            Text::RemovalBlocked => "removal.blocked",
            Text::RemovalWrongState => "removal.wrong_state",
            Text::RemovalNotSaved => "removal.not_saved",
            Text::JobWaitsArchived => "removal.job_waits",
            Text::DeleteNameMismatch => "delete.name_mismatch",
            Text::DeleteFolderNotRemoved => "delete.folder_not_removed",
            Text::DeleteNotDisconnected => "delete.not_disconnected",
            Text::SwitcherArchiveProject => "switcher_removal.archive",
            Text::SwitcherDeleteProject => "switcher_removal.delete",
            Text::SwitcherArchivedProjects => "switcher_removal.archived",
            Text::SwitcherBack => "switcher_removal.back",
            Text::ArchiveProjectTitle => "archive.project_title",
            Text::ArchiveProjectHint => "archive.project_hint",
            Text::ArchiveCompress => "archive.compress",
            Text::ArchiveCompressHint => "archive.compress_hint",
            Text::ArchiveConfirm => "archive.confirm",
            Text::ArchiveVideoTitle => "archive.video_title",
            Text::ArchiveVideoHint => "archive.video_hint",
            Text::ArchivedProjectsTitle => "archived.title",
            Text::ArchivedProjectsEmpty => "archived.empty",
            Text::ArchivedProjectsHint => "archived.hint",
            Text::ArchivedPacked => "archived.packed",
            Text::ArchivedBusy => "archived.busy",
            Text::Unarchive => "archived.unarchive",
            Text::UnarchiveUnpacking => "archived.unpacking",
            Text::DeleteAction => "delete.action",
            Text::DeleteProjectTitle => "delete.project_title",
            Text::DeleteVideoTitle => "delete.video_title",
            Text::DeleteProjectHint => "delete.project_hint",
            Text::DeleteVideoHint => "delete.video_hint",
            Text::DeleteVideoNoIdeaHint => "delete.video_no_idea_hint",
            Text::DeleteFolderRecycle => "delete.folder_recycle",
            Text::DeleteFolderPermanent => "delete.folder_permanent",
            Text::DeleteNoFolder => "delete.no_folder",
            Text::DeleteTypeName => "delete.type_name",
            Text::DeletePermanently => "delete.permanently",
            Text::DeletePermanentlyHint => "delete.permanently_hint",
            Text::DeleteConfirm => "delete.confirm",
            Text::DeleteRunning => "delete.running",
            Text::VideosShowArchived => "videos_removal.show_archived",
            Text::VideosArchivedEmpty => "videos_removal.archived_empty",
            Text::ArchiveVideoAction => "videos_removal.archive",
            Text::ArchivedTag => "videos_removal.archived_tag",
            Text::VideoArchivedNotice => "videos_removal.archived_notice",
            Text::DeletedProjects => "costs_deleted.projects",
            Text::DeletedOn => "costs_deleted.deleted_on",
            Text::DeletedProjectForgotten => "costs_deleted.project_forgotten",
            Text::DeletedVideoForgotten => "costs_deleted.video_forgotten",
            Text::ForgetName => "costs_deleted.forget",
            Text::ForgetNameHint => "costs_deleted.forget_hint",
            Text::ThemeStartAgain => "themes_restart.start",
            Text::ThemeNoVideo => "themes_restart.no_video",
            Text::RemovalBlock(block) => match block {
                bardo_domain::RemovalBlock::ScheduledPost => "removal_block.scheduled_post",
                bardo_domain::RemovalBlock::HeldOnNetwork => "removal_block.held_on_network",
                bardo_domain::RemovalBlock::ActiveJob => "removal_block.active_job",
                bardo_domain::RemovalBlock::MediaToMove => "removal_block.media_to_move",
            },
            Text::PermanentReasonText(reason) => match reason {
                bardo_domain::PermanentReason::RemovableDrive => {
                    "delete.permanent_reason.removable_drive"
                }
                bardo_domain::PermanentReason::NetworkDrive => {
                    "delete.permanent_reason.network_drive"
                }
                bardo_domain::PermanentReason::LargerThanBin => {
                    "delete.permanent_reason.larger_than_bin"
                }
                bardo_domain::PermanentReason::BinOff => "delete.permanent_reason.bin_off",
            },
            Text::AppTagline => "app.tagline",
            Text::UiLanguageLabel => "settings.ui_language",
            Text::LanguageName(UiLanguage::EnUs) => "language.en-US",
            Text::LanguageName(UiLanguage::PtBr) => "language.pt-BR",
            Text::LanguageNotSaved => "error.language_not_saved",
            Text::ContentLanguageName(language) => {
                return format!("content_language.{}", language.code()).into();
            }
            Text::CountryName(country) => return format!("country.{}", country.code()).into(),
            Text::TourName(tour) => return format!("tour.{}.name", tour.code()).into(),
            Text::TourStepTitle(tour, step) => {
                return format!("tour.{}.{step}.title", tour.code()).into();
            }
            Text::TourStepBody(tour, step) => {
                return format!("tour.{}.{step}.body", tour.code()).into();
            }
            Text::ShortcutGroup(group) => return format!("shortcuts.group.{group}").into(),
            Text::ShortcutAction(action) => return format!("shortcuts.action.{action}").into(),
            Text::GuideGroupName(group) => return format!("guide.group.{}", group.code()).into(),
            Text::TourStepCount => "tour.step_count",
            Text::TourEscHint => "tour.esc_hint",
            Text::TourBack => "tour.back",
            Text::TourNext => "tour.next",
            Text::TourFinish => "tour.finish",
            Text::TourSkip => "tour.skip",
            Text::TourNew => "tour.new",
            Text::TourOfferTitle => "tour.offer.title",
            Text::TourOfferBody => "tour.offer.body",
            Text::TourOfferStart => "tour.offer.start",
            Text::TourOfferNotNow => "tour.offer.not_now",
            Text::TourOfferNever => "tour.offer.never",
            Text::GuideResumeTour => "guide.resume_tour",
            Text::GuideShortcuts => "guide.shortcuts",
            Text::GuideResetTours => "guide.reset_tours",
            Text::GuideResetFailed => "guide.reset_failed",
            Text::ShortcutsTitle => "shortcuts.title",
            Text::ShortcutsClose => "shortcuts.close",
            Text::GuideUserGuide => "guide.user_guide",
            Text::GuideSearchPlaceholder => "guide.search.placeholder",
            Text::GuideSearchKeys => "guide.search.keys",
            Text::GuideSearchEmpty => "guide.search.empty",
            Text::GuideOnThisPage => "guide.on_this_page",
            Text::GuideShowMe => "guide.show_me",
            Text::GuideGoTo => "guide.go_to",
            Text::TourLearnMore => "tour.learn_more",
            Text::TourThisScreen => "tour.this_screen",
            Text::TourThisList => "tour.this_list",
            Text::TourThisPanel => "tour.this_panel",
            Text::TourThisStage => "tour.this_stage",
            Text::GuideMoreInGuide => "guide.more",
            Text::GuideBackToEditor => "guide.back_to_editor",
            Text::ToursSettingTitle => "tours_setting.title",
            Text::ToursSettingOffer => "tours_setting.offer",
            Text::ToursSettingHint => "tours_setting.hint",
            Text::ToursSettingNotSaved => "tours_setting.not_saved",
            Text::ChannelsTitle => "channels.title",
            Text::ChannelsEmpty => "channels.empty",
            Text::ChannelsNotLoaded => "channels.not_loaded",
            Text::NewChannel => "channels.new",
            Text::NewChannelTitle => "channel.new_title",
            Text::EditChannelTitle => "channel.edit_title",
            Text::ChannelName => "channel.name",
            Text::ChannelNamePlaceholder => "channel.name_placeholder",
            Text::ChannelNiche => "channel.niche",
            Text::ChannelNichePlaceholder => "channel.niche_placeholder",
            Text::ChannelThemes => "channel.themes",
            Text::ChannelThemesPlaceholder => "channel.themes_placeholder",
            Text::ChannelThemesHint => "channel.themes_hint",
            Text::ChannelAestheticNotes => "channel.aesthetic_notes",
            Text::ChannelAestheticNotesPlaceholder => "channel.aesthetic_notes_placeholder",
            Text::ChannelLanguage => "channel.language",
            Text::ChannelCountry => "channel.country",
            Text::ChannelCountrySearch => "channel.country_search",
            Text::CreateChannel => "channel.create",
            Text::SaveChannel => "channel.save",
            Text::ChannelSaved => "channel.saved",
            Text::ChannelNameTaken => "channel.error.name_taken",
            Text::ChannelNotFound => "channel.error.not_found",
            Text::ChannelNotSaved => "channel.error.not_saved",
            Text::ChannelPersona => "channel.persona",
            Text::ChannelPersonaNone => "channel.persona_none",
            Text::ChannelPersonaHint => "channel.persona_hint",
            Text::ChannelPersonaNotFound => "channel.error.persona_not_found",
            Text::ChannelClipModel => "channel.clip_model",
            Text::ChannelClipModelDefault => "channel.clip_model_default",
            Text::ProjectSettingsVideo => "projects.settings_video",
            Text::ProjectSettingsProduction => "projects.settings_production",
            Text::ChannelClipModelHint => "channel.clip_model_hint",
            Text::ChannelTakeModel => "channel.take_model",
            Text::ChannelTakeModelHint => "channel.take_model_hint",
            Text::ChannelCaptionStyle => "channel.caption_style",
            Text::ChannelCaptionStyleHint => "channel.caption_style_hint",
            Text::ChannelCaptionHighlight => "channel.caption_highlight",
            Text::ChannelCaptionHighlightHint => "channel.caption_highlight_hint",
            Text::ChannelFormat => "channel.format",
            Text::ChannelFormatHint => "channel.format_hint",
            Text::ChannelTransition => "channel.transition",
            Text::ChannelTransitionHint => "channel.transition_hint",
            Text::ChannelTransitionLength => "channel.transition_length",
            Text::ChannelMotion => "channel.motion",
            Text::ChannelMotionHint => "channel.motion_hint",
            Text::ChannelMotionStrength => "channel.motion_strength",
            Text::ChannelClipModelNotOffered => "channel.error.clip_model_not_offered",
            Text::ChannelTakeModelNotOffered => "channel.error.take_model_not_offered",
            Text::ChannelAccountsTitle => "network_accounts.title",
            Text::ChannelAccountsHint => "network_accounts.hint",
            Text::ChannelAccountsSaveFirst => "network_accounts.save_first",
            Text::ChannelAccountsEmpty => "network_accounts.empty",
            Text::ChannelAccountsNotLoaded => "network_accounts.not_loaded",
            Text::AllNetworksAdded => "network_accounts.all_added",
            Text::AddNetworkAccount => "network_accounts.add",
            Text::NewNetworkAccountTitle => "network_account.new_title",
            Text::EditNetworkAccountTitle => "network_account.edit_title",
            Text::AccountHandle => "network_account.handle",
            Text::AccountHandlePlaceholder => "network_account.handle_placeholder",
            Text::AccountMetadataTitle => "network_account.metadata_title",
            Text::AccountMetadataHint => "network_account.metadata_hint",
            Text::AccountLanguage => "network_account.language",
            Text::AccountLanguageChannel => "network_account.language_channel",
            Text::AccountTags => "network_account.tags",
            Text::AccountTagsPlaceholder => "network_account.tags_placeholder",
            Text::AccountTagsHint => "network_account.tags_hint",
            Text::AccountFooter => "network_account.footer",
            Text::AccountFooterPlaceholder => "network_account.footer_placeholder",
            Text::AccountVisibility => "network_account.visibility",
            Text::AccountVisibilityOnlyPublic => "network_account.visibility_only_public",
            Text::RenderPresetTitle => "render_preset.title",
            Text::RenderPresetHint => "render_preset.hint",
            Text::RenderPresetAspect => "render_preset.aspect",
            Text::RenderPresetResolution => "render_preset.resolution",
            Text::RenderPresetCodec => "render_preset.codec",
            Text::RenderPresetBitrate => "render_preset.bitrate",
            Text::RenderPresetMaxDuration => "render_preset.max_duration",
            Text::RenderPresetMaxDurationHint => "render_preset.max_duration_hint",
            Text::RenderPresetLoudness => "render_preset.loudness",
            Text::RenderPresetNetworkDefault => "render_preset.network_default",
            Text::RenderPresetSummary => "render_preset.summary",
            Text::RenderPresetCustom => "render_preset.custom",
            Text::RenderPresetDefault => "render_preset.default",
            Text::RenderPresetEffective => "render_preset.effective",
            Text::CreateNetworkAccount => "network_account.create",
            Text::SaveNetworkAccount => "network_account.save",
            Text::CancelNetworkAccount => "network_account.cancel",
            Text::EditNetworkAccount => "network_account.edit",
            Text::RemoveNetworkAccount => "network_account.remove",
            Text::NetworkAccountRemoveConfirm => "network_account.remove_confirm",
            Text::ConfirmRemoveNetworkAccount => "network_account.confirm_remove",
            Text::KeepNetworkAccount => "network_account.keep",
            Text::NetworkAccountSaved => "network_account.saved",
            Text::NetworkAccountRemoved => "network_account.removed",
            Text::NetworkAccountTaken => "network_account.error.taken",
            Text::NetworkAccountNotFound => "network_account.error.not_found",
            Text::NetworkAccountNotSaved => "network_account.error.not_saved",
            Text::NetworkName(network) => match network {
                Network::YouTube => "network.youtube",
                Network::TikTok => "network.tiktok",
                Network::InstagramReels => "network.instagram_reels",
                Network::X => "network.x",
                Network::Kick => "network.kick",
            },
            Text::VisibilityName(visibility) => match visibility {
                Visibility::Public => "visibility.public",
                Visibility::Unlisted => "visibility.unlisted",
                Visibility::Private => "visibility.private",
            },
            Text::AspectRatioName(aspect) => match aspect {
                AspectRatio::Vertical => "aspect_ratio.vertical",
                AspectRatio::Landscape => "aspect_ratio.landscape",
            },
            Text::CaptionStyleName(style) => match style {
                CaptionStyle::Clean => "caption_style.clean",
                CaptionStyle::Boxed => "caption_style.boxed",
                CaptionStyle::Punch => "caption_style.punch",
            },
            Text::CaptionHighlightName(highlight) => match highlight {
                CaptionHighlight::Off => "caption_highlight.off",
                CaptionHighlight::Yellow => "caption_highlight.yellow",
                CaptionHighlight::Green => "caption_highlight.green",
                CaptionHighlight::Cyan => "caption_highlight.cyan",
                CaptionHighlight::Pink => "caption_highlight.pink",
                CaptionHighlight::Orange => "caption_highlight.orange",
            },
            Text::TransitionName(kind) => match kind {
                TransitionKind::Crossfade => "transition.crossfade",
                TransitionKind::FadeBlack => "transition.fade_black",
                TransitionKind::FadeWhite => "transition.fade_white",
                TransitionKind::WipeLeft => "transition.wipe_left",
                TransitionKind::WipeRight => "transition.wipe_right",
                TransitionKind::SlideLeft => "transition.slide_left",
                TransitionKind::SlideRight => "transition.slide_right",
                TransitionKind::SlideUp => "transition.slide_up",
                TransitionKind::SlideDown => "transition.slide_down",
                TransitionKind::ZoomIn => "transition.zoom_in",
            },
            Text::TransitionCut => "transition.cut",
            Text::MotionName(kind) => match kind {
                MotionKind::ZoomIn => "motion.zoom_in",
                MotionKind::ZoomOut => "motion.zoom_out",
                MotionKind::PanLeft => "motion.pan_left",
                MotionKind::PanRight => "motion.pan_right",
                MotionKind::PanUp => "motion.pan_up",
                MotionKind::PanDown => "motion.pan_down",
            },
            Text::MotionNone => "motion.none",
            Text::MotionVaried => "motion.varied",
            Text::MotionStrengthName(strength) => match strength {
                MotionStrength::Subtle => "motion_strength.subtle",
                MotionStrength::Medium => "motion_strength.medium",
                MotionStrength::Strong => "motion_strength.strong",
            },
            Text::VideoFormatName(format) => match format {
                VideoFormat::Short => "video_format.short",
                VideoFormat::Long => "video_format.long",
            },
            Text::VideoFormatBrief(format) => match format {
                VideoFormat::Short => "video_format.short_brief",
                VideoFormat::Long => "video_format.long_brief",
            },
            Text::VideoFormatLength(format) => match format {
                VideoFormat::Short => "video_format.short_length",
                VideoFormat::Long => "video_format.long_length",
            },
            Text::VideoLengthName(length) => match length {
                VideoLength::Seconds15 => "video_length.seconds_15",
                VideoLength::Seconds30 => "video_length.seconds_30",
                VideoLength::Minute1 => "video_length.minute_1",
                VideoLength::Minutes3 => "video_length.minutes_3",
            },
            Text::NetworkAccountFieldError(error) => match error {
                NetworkAccountFieldError::HandleRequired => "network_account.error.handle_required",
                NetworkAccountFieldError::HandleTooLong => "network_account.error.handle_too_long",
                NetworkAccountFieldError::HandleInvalid => "network_account.error.handle_invalid",
                NetworkAccountFieldError::TooManyTags => "network_account.error.too_many_tags",
                NetworkAccountFieldError::TagTooLong => "network_account.error.tag_too_long",
                NetworkAccountFieldError::DescriptionFooterTooLong => {
                    "network_account.error.footer_too_long"
                }
                NetworkAccountFieldError::VisibilityNotOffered => {
                    "network_account.error.visibility_not_offered"
                }
                NetworkAccountFieldError::BitrateInvalid => "network_account.error.bitrate_invalid",
                NetworkAccountFieldError::MaxDurationInvalid => {
                    "network_account.error.max_duration_invalid"
                }
                NetworkAccountFieldError::LoudnessInvalid => {
                    "network_account.error.loudness_invalid"
                }
            },
            Text::PersonasTitle => "personas.title",
            Text::PersonasHint => "personas.hint",
            Text::PersonasEmpty => "personas.empty",
            Text::PersonasNotLoaded => "personas.not_loaded",
            Text::NewPersona => "personas.new",
            Text::NewPersonaTitle => "persona.new_title",
            Text::EditPersonaTitle => "persona.edit_title",
            Text::PersonaName => "persona.name",
            Text::PersonaNamePlaceholder => "persona.name_placeholder",
            Text::PersonaVoice => "persona.voice",
            Text::PersonaVoiceNone => "persona.voice_none",
            Text::PersonaVoiceHint => "persona.voice_hint",
            Text::ChooseVoice => "voices.choose",
            Text::ReloadVoices => "voices.reload",
            Text::HideVoices => "voices.hide",
            Text::LoadingVoices => "voices.loading",
            Text::VoicesTitle => "voices.title",
            Text::VoicesEmpty => "voices.empty",
            Text::VoicesFailed => "voices.failed",
            Text::VoicesMissingKey => "voices.missing_key",
            Text::VoiceCategoryName(category) => match category {
                VoiceCategory::Cloned => "voices.category.cloned",
                VoiceCategory::Professional => "voices.category.professional",
                VoiceCategory::Generated => "voices.category.generated",
                VoiceCategory::Default => "voices.category.default",
                VoiceCategory::Other => "voices.category.other",
            },
            Text::VoiceAvailable => "voices.available",
            Text::VoiceMissing => "voices.missing",
            Text::VoiceSelected => "voices.selected",
            Text::VoicePreviewPlay => "voices.preview.play",
            Text::VoicePreviewStop => "voices.preview.stop",
            Text::VoicePreviewNone => "voices.preview.none",
            Text::VoicePreviewNote => "voices.preview.note",
            Text::VoiceSampleTitle => "voices.sample.title",
            Text::VoiceSampleHint => "voices.sample.hint",
            Text::VoiceSampleSentence => "voices.sample.sentence",
            Text::VoiceSamplePlay => "voices.sample.play",
            Text::VoiceSampleStop => "voices.sample.stop",
            Text::VoiceSampleReading => "voices.sample.reading",
            Text::VoiceSampleFree => "voices.sample.free",
            Text::VoiceSampleBilled => "voices.sample.billed",
            Text::VoiceSampleNoVoice => "voices.sample.no_voice",
            Text::VoiceSampleMissingKey => "voices.sample.missing_key",
            Text::VoiceSampleTooLong => "voices.sample.too_long",
            Text::VoiceSampleFailed => "voices.sample.failed",
            Text::VoiceSampleNotKept => "voices.sample.not_kept",
            Text::VoiceSampleCannotPlay => "voices.sample.cannot_play",
            Text::PersonaTone => "persona.tone",
            Text::PersonaTonePlaceholder => "persona.tone_placeholder",
            Text::PersonaScriptStyle => "persona.script_style",
            Text::PersonaScriptStylePlaceholder => "persona.script_style_placeholder",
            Text::PersonaPresets => "persona.presets",
            Text::PersonaPresetsHint => "persona.presets_hint",
            Text::PresetStability => "persona.preset.stability",
            Text::PresetStabilityHint => "persona.preset.stability_hint",
            Text::PresetSimilarity => "persona.preset.similarity",
            Text::PresetSimilarityHint => "persona.preset.similarity_hint",
            Text::PresetStyle => "persona.preset.style",
            Text::PresetStyleHint => "persona.preset.style_hint",
            Text::PresetSpeed => "persona.preset.speed",
            Text::PresetSpeedHint => "persona.preset.speed_hint",
            Text::PresetPercent => "persona.preset.percent",
            Text::CreatePersona => "persona.create",
            Text::SavePersona => "persona.save",
            Text::DuplicatePersona => "persona.duplicate",
            Text::PersonaSaved => "persona.saved",
            Text::PersonaDuplicated => "persona.duplicated",
            Text::PersonaUsedBy => "persona.used_by",
            Text::PersonaUnused => "persona.unused",
            Text::PersonaConfirmTitle => "persona.confirm.title",
            Text::PersonaConfirmHint => "persona.confirm.hint",
            Text::ConfirmPersonaSave => "persona.confirm.save",
            Text::CancelPersonaSave => "persona.confirm.cancel",
            Text::PersonaNameTaken => "persona.error.name_taken",
            Text::PersonaNotFound => "persona.error.not_found",
            Text::PersonaNotSaved => "persona.error.not_saved",
            Text::PersonaCopySuffix => "persona.copy_suffix",
            Text::PersonaImportedSuffix => "persona.imported_suffix",
            Text::ImportPersona => "personas.import",
            Text::ImportPersonaDialog => "personas.import_dialog",
            Text::ExportPersona => "persona.export",
            Text::PersonaExportHint => "persona.export_hint",
            Text::PersonaExported => "persona.exported",
            Text::PersonaImported => "persona.imported",
            Text::VoiceFlagTag(flag) => match flag {
                VoiceFlag::Unchecked => "persona.voice_flag.unchecked_tag",
                VoiceFlag::Unavailable => "persona.voice_flag.unavailable_tag",
            },
            Text::VoiceFlagExplanation(flag) => match flag {
                VoiceFlag::Unchecked => "persona.voice_flag.unchecked",
                VoiceFlag::Unavailable => "persona.voice_flag.unavailable",
            },
            Text::CheckVoices => "persona.voice_flag.check",
            Text::PersonaPackageUnreadable => "persona.package.unreadable",
            Text::PersonaPackageNotOne => "persona.package.not_a_package",
            Text::PersonaPackageNewer => "persona.package.newer",
            Text::PersonaPackageInvalid => "persona.package.invalid",
            Text::PersonaExportFailed => "persona.package.export_failed",
            Text::FileDialogFailed => "persona.package.dialog_failed",
            Text::ProjectNarrator => "projects.narrator",
            Text::ProjectNarratorChannel => "projects.narrator_channel",
            Text::ProjectNarratorChannelNone => "projects.narrator_channel_none",
            Text::ProjectNarratorHint => "projects.narrator_hint",
            Text::ProjectFormat => "projects.format",
            Text::ProjectFormatChannel => "projects.format_channel",
            Text::ProjectFormatHint => "projects.format_hint",
            Text::ProjectLength => "projects.length",
            Text::ProjectLengthFormat => "projects.length_format",
            Text::ProjectLengthHint => "projects.length_hint",
            Text::ProductionModeName(mode) => match mode {
                ProductionMode::Economy => "production_mode.economy",
                ProductionMode::AnimatedHook => "production_mode.hook",
                ProductionMode::AllAnimated => "production_mode.animated",
                ProductionMode::Premium => "production_mode.premium",
                ProductionMode::ChannelModel => "production_mode.channel_model",
            },
            Text::ChannelStyle => "channel.style",
            Text::ChannelStyleHint => "channel.style_hint",
            Text::ProjectStyle => "projects.style",
            Text::ProjectStyleChannel => "projects.style_channel",
            Text::ProjectStyleChannelNotes => "projects.style_channel_notes",
            Text::ProjectStyleHint => "projects.style_hint",
            Text::ScenesRestyled => "scenes.restyled.title",
            Text::ScenesRestyledHint => "scenes.restyled.hint",
            Text::RedrawRestyled => "scenes.restyled.redraw",
            Text::ScenesOldPortraits => "scenes.old_portraits.title",
            Text::ScenesOldPortraitsHint => "scenes.old_portraits.hint",
            Text::RedrawOldPortraits => "scenes.old_portraits.redraw",
            Text::SceneOldPortrait => "scenes.old_portraits.scene",
            Text::SceneCardOldPortrait => "scenes.old_portraits.card",
            Text::VisualStyleName(style) => match style {
                StyleChoice::FromNotes => "visual_style.notes",
                StyleChoice::Catalog(VisualStyle::Photorealistic) => "visual_style.photo",
                StyleChoice::Catalog(VisualStyle::CinematicFilm) => "visual_style.cinematic",
                StyleChoice::Catalog(VisualStyle::DocumentaryPhoto) => "visual_style.documentary",
                StyleChoice::Catalog(VisualStyle::VintageFilm) => "visual_style.vintage_film",
                StyleChoice::Catalog(VisualStyle::FilmNoir) => "visual_style.noir",
                StyleChoice::Catalog(VisualStyle::Animation3d) => "visual_style.animation_3d",
                StyleChoice::Catalog(VisualStyle::Cartoon2d) => "visual_style.cartoon",
                StyleChoice::Catalog(VisualStyle::Anime) => "visual_style.anime",
                StyleChoice::Catalog(VisualStyle::Manga) => "visual_style.manga",
                StyleChoice::Catalog(VisualStyle::ComicBook) => "visual_style.comic",
                StyleChoice::Catalog(VisualStyle::Storybook) => "visual_style.storybook",
                StyleChoice::Catalog(VisualStyle::Watercolor) => "visual_style.watercolor",
                StyleChoice::Catalog(VisualStyle::OilPainting) => "visual_style.oil",
                StyleChoice::Catalog(VisualStyle::PencilSketch) => "visual_style.pencil",
                StyleChoice::Catalog(VisualStyle::InkDrawing) => "visual_style.ink",
                StyleChoice::Catalog(VisualStyle::FlatVector) => "visual_style.flat_vector",
                StyleChoice::Catalog(VisualStyle::PaperCutout) => "visual_style.paper_cutout",
                StyleChoice::Catalog(VisualStyle::ClayStopMotion) => "visual_style.clay",
                StyleChoice::Catalog(VisualStyle::PixelArt) => "visual_style.pixel_art",
                StyleChoice::Catalog(VisualStyle::LowPoly) => "visual_style.low_poly",
                StyleChoice::Catalog(VisualStyle::Isometric) => "visual_style.isometric",
                StyleChoice::Catalog(VisualStyle::NeonCyberpunk) => "visual_style.cyberpunk",
                StyleChoice::Catalog(VisualStyle::Synthwave) => "visual_style.synthwave",
                StyleChoice::Catalog(VisualStyle::DarkFantasy) => "visual_style.dark_fantasy",
                StyleChoice::Catalog(VisualStyle::WoodblockPrint) => "visual_style.woodblock",
                StyleChoice::Catalog(VisualStyle::Chalkboard) => "visual_style.chalkboard",
            },
            Text::ProductionModeAbout(mode) => match mode {
                ProductionMode::Economy => "production_mode.about.economy",
                ProductionMode::AnimatedHook => "production_mode.about.hook",
                ProductionMode::AllAnimated => "production_mode.about.animated",
                ProductionMode::Premium => "production_mode.about.premium",
                ProductionMode::ChannelModel => "production_mode.about.channel_model",
            },
            Text::ChannelProduction => "channel.production",
            Text::ChannelProductionHint => "channel.production_hint",
            Text::ChannelClipFit => "channel.clip_fit",
            Text::ChannelClipFitHint => "channel.clip_fit_hint",
            Text::ClipFitName(fit) => match fit {
                ClipFit::Stretch => "clip_fit.stretch",
                ClipFit::Cover => "clip_fit.cover",
            },
            Text::ChannelStretchLimit => "channel.stretch_limit",
            Text::ChannelStretchLimitHint => "channel.stretch_limit_hint",
            Text::StretchLimitChoice => "channel.stretch_limit_choice",
            Text::SceneClipSlowedNext => "scenes.clip.slowed_next",
            Text::SceneClipSlowed => "scenes.clip.slowed",
            Text::SceneClipSlowedHint => "scenes.clip.slowed_hint",
            Text::EditorSourceSlowed => "editor.inspector.source_slowed",
            Text::ProjectMode => "projects.mode",
            Text::ProjectModeChannel => "projects.mode_channel",
            Text::ProjectModeHint => "projects.mode_hint",
            Text::ProjectModeOption => "projects.mode_option",
            Text::ProjectModeMissingKey => "projects.mode_missing_key",
            Text::ProjectModeMissingSignIn => "projects.mode_missing_sign_in",
            Text::ProjectEstimate => "projects.estimate",
            Text::ProjectEstimatePartial => "projects.estimate_partial",
            Text::SpendCap => "projects.cap.label",
            Text::SpendCapHint => "projects.cap.hint",
            Text::SpendCapPlaceholder => "projects.cap.placeholder",
            Text::SaveSpendCap => "projects.cap.save",
            Text::SpendCapSaved => "projects.cap.saved",
            Text::SpendCapInvalid => "projects.cap.invalid",
            Text::SpendCapOver => "projects.cap.over",
            Text::SpendCapDefault => "projects.cap.default",
            Text::SpendCapUseDefault => "projects.cap.use_default",
            Text::SpendCapOffer => "projects.cap.offer",
            Text::SpendCapOfferAccept => "projects.cap.offer_accept",
            Text::SpendCapOfferDecline => "projects.cap.offer_decline",
            Text::SpendCapKept => "projects.cap.kept",
            Text::CapReachedTitle => "budget.cap_title",
            Text::CapReachedLine => "budget.cap_line",
            Text::ClipsEconomy => "scenes.clip.economy",
            Text::AnimateHookClips => "scenes.clip.animate_hook",
            Text::PersonaFieldError(error) => match error {
                PersonaFieldError::NameRequired => "persona.error.name_required",
                PersonaFieldError::NameTooLong => "persona.error.name_too_long",
                PersonaFieldError::VoiceRequired => "persona.error.voice_required",
                PersonaFieldError::ToneTooLong => "persona.error.tone_too_long",
                PersonaFieldError::ScriptStyleTooLong => "persona.error.script_style_too_long",
                PersonaFieldError::StabilityOutOfRange => "persona.error.stability_out_of_range",
                PersonaFieldError::SimilarityOutOfRange => "persona.error.similarity_out_of_range",
                PersonaFieldError::StyleOutOfRange => "persona.error.style_out_of_range",
                PersonaFieldError::SpeedOutOfRange => "persona.error.speed_out_of_range",
            },
            Text::ChannelFieldError(error) => match error {
                ChannelFieldError::NameRequired => "channel.error.name_required",
                ChannelFieldError::NameTooLong => "channel.error.name_too_long",
                ChannelFieldError::NicheTooLong => "channel.error.niche_too_long",
                ChannelFieldError::TooManyThemes => "channel.error.too_many_themes",
                ChannelFieldError::ThemeTooLong => "channel.error.theme_too_long",
                ChannelFieldError::AestheticNotesTooLong => {
                    "channel.error.aesthetic_notes_too_long"
                }
            },
            Text::JobsTitle => "jobs.title",
            Text::JobsEmpty => "jobs.empty",
            Text::JobsRunning => "jobs.running",
            Text::JobsQueued => "jobs.queued",
            Text::JobsFailed => "jobs.failed",
            Text::JobsFinished => "jobs.finished",
            Text::JobKindName(kind) => return format!("job.kind.{}", kind.code()).into(),
            Text::JobStateName(state) => return format!("job.state.{}", state.code()).into(),
            Text::JobFailureKindName(kind) => {
                return format!("job.failure.{}", kind.code()).into();
            }
            Text::JobRetryScheduled => "job.retry_scheduled",
            Text::JobFailedAfter => "job.failed_after",
            Text::CancelJob => "job.cancel",
            Text::RetryJob => "job.retry",
            Text::JobNotUpdated => "job.error.not_updated",
            Text::JobNotStarted => "job.error.not_started",
            Text::TestJobsTitle => "jobs.test.title",
            Text::TestJobsHint => "jobs.test.hint",
            Text::StartTestJob => "jobs.test.start",
            Text::StartFailingTestJob => "jobs.test.start_failing",
            Text::SettingsTitle => "settings.title",
            Text::ProviderKeysTitle => "provider_keys.title",
            Text::SetupStepByStep => "setup.step_by_step",
            Text::SetupTitle => "setup.title",
            Text::SetupBack => "setup.back",
            Text::SetupPasteHere => "setup.paste_here",
            Text::SetupOpenInGuide => "setup.open_in_guide",
            Text::SetupAudits => "setup.audits",
            Text::SetupAuditTitle => "setup.audit_title",
            Text::SetupCopyBlock => "setup.copy_block",
            Text::SetupCopied => "setup.copied",
            Text::ProviderKeysHint => "provider_keys.hint",
            Text::ProviderName(provider) => {
                return format!("provider.{}.name", provider.code()).into();
            }
            Text::ModelLevelName(level) => {
                return format!("model_level.{}", level.code()).into();
            }
            Text::ProviderPurpose(provider) => {
                return format!("provider.{}.purpose", provider.code()).into();
            }
            Text::BytePlusFieldLabel(field) => {
                return format!("provider_keys.byteplus.field.{}", field.code()).into();
            }
            Text::BytePlusFieldError(BytePlusFieldError::Required(_)) => {
                "provider_keys.byteplus.required"
            }
            Text::BytePlusFieldError(BytePlusFieldError::Invalid(field)) => {
                return format!("provider_keys.byteplus.invalid.{}", field.code()).into();
            }
            Text::BytePlusFieldsInvalid => "provider_keys.byteplus.fields_invalid",
            Text::BytePlusCardNote => "provider_keys.byteplus.note",
            Text::ProviderKeyPlaceholder(provider) => {
                return format!("provider.{}.placeholder", provider.code()).into();
            }
            Text::KeyNotSet => "provider_keys.not_set",
            Text::KeySaved => "provider_keys.saved",
            Text::KeyUnreadable => "provider_keys.unreadable",
            Text::SaveKey => "provider_keys.save",
            Text::ReplaceKey => "provider_keys.replace",
            Text::TestKey => "provider_keys.test",
            Text::TestingKey => "provider_keys.testing",
            Text::RemoveKey => "provider_keys.remove",
            Text::KeyCheckOutcome(outcome) => {
                return format!("key_check.{}", outcome.code()).into();
            }
            Text::KeyCheckDetail => "key_check.detail",
            Text::ApiKeyError(error) => match error {
                ApiKeyError::Required => "api_key.error.required",
                ApiKeyError::TooShort => "api_key.error.too_short",
                ApiKeyError::TooLong => "api_key.error.too_long",
                ApiKeyError::InvalidCharacters => "api_key.error.invalid_characters",
                ApiKeyError::NotIdAndSecret => "api_key.error.not_id_and_secret",
                ApiKeyError::NotBytePlusCredentials => "api_key.error.not_byteplus_credentials",
            },
            Text::ProviderKeyNotSet => "provider_keys.error.not_set",
            Text::SettingsNetworksTab => "settings.networks_tab",
            Text::AppCredentialsTitle => "app_credentials.title",
            Text::AppCredentialsHint => "app_credentials.hint",
            Text::AppCredentialsInfo => "app_credentials.info",
            Text::AppCredentialsName(network) => {
                return format!("app_credentials.{}.name", network.code()).into();
            }
            Text::AppCredentialsPurpose(network) => {
                return format!("app_credentials.{}.purpose", network.code()).into();
            }
            Text::ClientId(network) => {
                return format!("app_credentials.{}.client_id", network.code()).into();
            }
            Text::ClientIdPlaceholder(network) => {
                return format!("app_credentials.{}.client_id_placeholder", network.code()).into();
            }
            Text::ClientSecret(network) => {
                return format!("app_credentials.{}.client_secret", network.code()).into();
            }
            Text::ClientSecretPlaceholder(network) => {
                return format!(
                    "app_credentials.{}.client_secret_placeholder",
                    network.code()
                )
                .into();
            }
            Text::AppCredentialsNotSet => "app_credentials.not_set",
            Text::AppCredentialsSaved => "app_credentials.saved",
            Text::AppCredentialsUnreadable => "app_credentials.unreadable",
            Text::SaveAppCredentials => "app_credentials.save",
            Text::ReplaceAppCredentials => "app_credentials.replace",
            Text::RemoveAppCredentials => "app_credentials.remove",
            Text::AppCredentialsFieldError(network, error) => {
                let error = match error {
                    AppCredentialsFieldError::ClientIdRequired => "client_id_required",
                    AppCredentialsFieldError::ClientIdInvalid => "client_id_invalid",
                    AppCredentialsFieldError::ClientSecretRequired => "client_secret_required",
                    AppCredentialsFieldError::ClientSecretInvalid => "client_secret_invalid",
                };
                return format!("app_credentials.{}.error.{error}", network.code()).into();
            }
            Text::ConnectionNotConnectedLabel => "connection.not_connected",
            Text::ConnectionConnecting(network) => {
                return format!("connection.{}.connecting", network.code()).into();
            }
            Text::ConnectionConnected => "connection.connected",
            Text::ConnectionReconnectNeeded => "connection.reconnect_needed",
            Text::Connect => "connection.connect",
            Text::Reconnect => "connection.reconnect",
            Text::CancelConnect => "connection.cancel",
            Text::Disconnect => "connection.disconnect",
            Text::Disconnecting => "connection.disconnecting",
            Text::CheckConnection => "connection.check",
            Text::CheckingConnection => "connection.checking",
            Text::OpenNetworkSettings => "connection.open_settings",
            Text::ConnectionChecked(network) => {
                return format!("connection.{}.checked", network.code()).into();
            }
            Text::ConnectionDisconnected => "connection.disconnected",
            Text::ConnectionDisconnectedNotRevoked => "connection.disconnected_not_revoked",
            Text::ConnectionDisconnectedKeptForOthers => "connection.disconnected_kept_for_others",
            Text::ConnectionDisconnectedCredentialsRefused => {
                "connection.disconnected_credentials_refused"
            }
            Text::ConnectionReconnectHint(network) => {
                return format!("connection.{}.reconnect_hint", network.code()).into();
            }
            Text::ConnectionNotOffered => "connection.error.not_offered",
            Text::ConnectionNeedsAppCredentials(network) => {
                return format!("connection.{}.needs_app_credentials", network.code()).into();
            }
            Text::ConnectionNotConnected => "connection.error.not_connected",
            Text::ConnectionDenied => "connection.error.denied",
            Text::ConnectionTimedOut => "connection.error.timed_out",
            Text::ConnectionCancelled => "connection.error.cancelled",
            Text::ConnectionListenFailed => "connection.error.listen_failed",
            Text::ConnectionPortInUse => "connection.error.port_in_use",
            Text::ConnectionMissingScopes(network) => {
                return format!("connection.{}.missing_scopes", network.code()).into();
            }
            Text::ConnectionTokensTooLarge => "connection.error.tokens_too_large",
            Text::ConnectionStoreFailed => "connection.error.store_failed",
            Text::ConnectionNotSaved => "connection.error.not_saved",
            Text::ConnectionNoPages => "connection.error.no_pages",
            Text::ConnectionNoLinkedAccount => "connection.error.no_linked_account",
            Text::ConnectionChoiceGone => "connection.error.choice_gone",
            Text::ConnectionTokenLabel(network) => {
                return format!("connection.{}.token_label", network.code()).into();
            }
            Text::ConnectionTokenPlaceholder(network) => {
                return format!("connection.{}.token_placeholder", network.code()).into();
            }
            Text::ConnectionTokenHelp(network) => {
                return format!("connection.{}.token_help", network.code()).into();
            }
            Text::ConnectionOpenTokenTool(network) => {
                return format!("connection.{}.open_token_tool", network.code()).into();
            }
            Text::PastedTokenError(error) => match error {
                PastedTokenError::Required => "connection.error.token_required",
                PastedTokenError::Invalid => "connection.error.token_invalid",
            },
            Text::ConnectionChoose(network) => {
                return format!("connection.{}.choose", network.code()).into();
            }
            Text::ConnectionChoiceVia(network) => {
                return format!("connection.{}.choice_via", network.code()).into();
            }
            Text::ConnectionUseAccount => "connection.use_account",
            Text::ConnectionChooseLabel => "connection.choose_label",
            Text::SignInFailure(network, kind) => {
                let kind = match kind {
                    // The same everywhere.
                    SignInFailureKind::Unreachable => {
                        return "connection.failure.unreachable".into();
                    }
                    SignInFailureKind::Unexpected => return "connection.failure.unexpected".into(),
                    SignInFailureKind::Refused => "refused",
                    SignInFailureKind::ClientRejected => "client_rejected",
                    SignInFailureKind::NotAllowed => "not_allowed",
                    SignInFailureKind::NoChannel => "no_channel",
                    SignInFailureKind::LimitReached => "limit_reached",
                    SignInFailureKind::NetworkDown => "network_down",
                };
                return format!("connection.failure.{}.{kind}", network.code()).into();
            }
            Text::NetworkAccountStillConnected => "network_account.error.still_connected",
            Text::PlansTitle => "plan.title",
            Text::PlansHint => "plan.hint",
            Text::PlanNotConnected => "plan.not_connected",
            Text::PlanConnected => "plan.connected",
            Text::PlanConnectedPlain => "plan.connected_plain",
            Text::PlanReconnectNeeded => "plan.reconnect_needed",
            Text::PlanUnreadable => "plan.unreadable",
            Text::PlanSignIn => "plan.sign_in",
            Text::PlanSignInAgain => "plan.sign_in_again",
            Text::PlanWaiting => "plan.waiting",
            Text::PlanCancel => "plan.cancel",
            Text::PlanCheck => "plan.check",
            Text::PlanChecking => "plan.checking",
            Text::PlanSignOut => "plan.sign_out",
            Text::PlanChecked => "plan.checked",
            Text::PlanNotSignedIn => "plan.error.not_signed_in",
            Text::PlanSignInAgainHint => "plan.error.sign_in_again",
            Text::PlanNotOffered => "plan.error.not_offered",
            Text::PlanDenied => "plan.error.denied",
            Text::PlanTimedOut => "plan.error.timed_out",
            Text::PlanCancelled => "plan.error.cancelled",
            Text::PlanListenFailed => "plan.error.listen_failed",
            Text::PlanPortInUse => "plan.error.port_in_use",
            Text::PlanTokensTooLarge => "plan.error.tokens_too_large",
            Text::PlanStoreFailed => "plan.error.store_failed",
            Text::PlanFailure(kind) => match kind {
                SignInFailureKind::Refused => "plan.failure.refused",
                SignInFailureKind::ClientRejected => "plan.failure.client_rejected",
                SignInFailureKind::NotAllowed => "plan.failure.not_allowed",
                SignInFailureKind::LimitReached => "plan.failure.limit_reached",
                SignInFailureKind::NetworkDown => "plan.failure.provider_down",
                SignInFailureKind::Unreachable => "plan.failure.unreachable",
                SignInFailureKind::NoChannel | SignInFailureKind::Unexpected => {
                    "plan.failure.unexpected"
                }
            },
            Text::ConsentPageDone => "consent_page.done",
            Text::ConsentPageFailed => "consent_page.failed",
            Text::ProviderKeyStoreFailed => "provider_keys.error.store_failed",
            Text::ResearchTitle => "research.title",
            Text::ResearchNoChannels => "research.no_channels",
            Text::ResearchChannel => "research.channel",
            Text::ResearchSeeds => "research.seeds",
            Text::ResearchSeedsPlaceholder => "research.seeds_placeholder",
            Text::ResearchSeedsHint => "research.seeds_hint",
            Text::RunResearch => "research.run",
            Text::RefreshResearch => "research.refresh",
            Text::RefreshResearchCost => "research.refresh_cost",
            Text::ResearchCost => "research.cost",
            Text::ResearchCostNone => "research.cost_none",
            Text::ResearchMissingKey => "research.error.missing_key",
            Text::ResearchNotStarted => "research.error.not_started",
            Text::ResearchNotLoaded => "research.error.not_loaded",
            Text::ResearchRunning => "research.running",
            Text::ResearchStopped => "research.stopped",
            Text::NicheSeedError(error) => match error {
                NicheSeedError::Required => "research.error.seed_required",
                NicheSeedError::TooMany => "research.error.too_many_seeds",
                NicheSeedError::TooLong => "research.error.seed_too_long",
            },
            Text::ResearchResultsTitle => "research.results",
            Text::ResearchScoresHint => "research.scores_hint",
            Text::ResearchOpportunity => "research.opportunity",
            Text::ResearchCompetition => "research.competition",
            Text::ResearchTrend => "research.trend",
            Text::ResearchUploads => "research.uploads",
            Text::ResearchMedianViews => "research.median_views",
            Text::ResearchViewsPerDay => "research.views_per_day",
            Text::ResearchMedianSubscribers => "research.median_subscribers",
            Text::ResearchSmallChannels => "research.small_channels",
            Text::ResearchSample => "research.sample",
            Text::ResearchFetched => "research.fetched",
            Text::ResearchNotFetched => "research.not_fetched",
            Text::ResearchNoUploads => "research.no_uploads",
            Text::ResearchStale => "research.stale",
            Text::ThemesTitle => "themes.title",
            Text::ThemesNoChannels => "themes.no_channels",
            Text::ThemesChannel => "themes.channel",
            Text::ThemesNiche => "themes.niche",
            Text::ThemesNoNiche => "themes.no_niche",
            Text::SuggestThemesHint => "themes.suggest_hint",
            Text::SuggestThemes => "themes.suggest",
            Text::RankThemes => "themes.rank",
            Text::ThemesUnranked => "themes.unranked",
            Text::ThemesRunning => "themes.running",
            Text::ThemesStopped => "themes.stopped",
            Text::ThemesListTitle => "themes.list",
            Text::ThemesRankingHint => "themes.ranking_hint",
            Text::ThemesEmpty => "themes.empty",
            Text::ThemesDiscarded => "themes.discarded",
            Text::ThemePriority => "theme.priority",
            Text::ThemeConfidence => "theme.confidence",
            Text::ThemeFit => "theme.fit",
            Text::ThemeTrend => "theme.trend",
            Text::ThemeCompetition => "theme.competition",
            Text::ThemePerformance => "theme.performance",
            Text::ThemePerformanceNiche => "theme.performance_niche",
            Text::ThemePerformanceNicheOne => "theme.performance_niche_one",
            Text::ThemePerformanceChannel => "theme.performance_channel",
            Text::ThemePerformanceChannelOne => "theme.performance_channel_one",
            Text::ThemePerformanceProjected => "theme.performance_projected",
            Text::ThemesNoHistory => "themes.no_history",
            Text::ThemesBeforeHistory => "themes.before_history",
            Text::ThemeNotRanked => "theme.not_ranked",
            Text::ThemeRankedBy => "theme.ranked_by",
            Text::ThemeTitle => "theme.title",
            Text::ThemeAngle => "theme.angle",
            Text::EditTheme => "theme.edit",
            Text::SaveTheme => "theme.save",
            Text::CancelThemeEdit => "theme.cancel",
            Text::DiscardTheme => "theme.discard",
            Text::ApproveTheme => "theme.approve",
            Text::ThemeApproved => "theme.approved",
            Text::ProjectsTitle => "projects.title",
            Text::ProjectsEmpty => "projects.empty",
            Text::ProjectStarted => "projects.started",
            Text::ThemeNotSaved => "theme.error.not_saved",
            Text::ThemeFieldError(error) => match error {
                ThemeFieldError::TitleRequired => "theme.error.title_required",
                ThemeFieldError::TitleTooLong => "theme.error.title_too_long",
                ThemeFieldError::AngleTooLong => "theme.error.angle_too_long",
            },
            Text::ThemesPickNiche => "themes.error.pick_niche",
            Text::ThemeNotFound => "theme.error.not_found",
            Text::ThemesMissingKey(provider) => {
                return format!("themes.error.missing_key.{}", provider.code()).into();
            }
            Text::ThemesBusy => "themes.error.busy",
            Text::ThemesNothingToRank => "themes.error.nothing_to_rank",
            Text::ThemesNotLoaded => "themes.error.not_loaded",
            Text::ProjectsNav => "projects.nav",
            Text::ProjectsNoChannels => "projects.no_channels",
            Text::ProjectsNotLoaded => "projects.error.not_loaded",
            Text::ProjectNotFound => "projects.error.not_found",
            Text::ScriptTitle => "script.title",
            Text::ScriptEmpty => "script.empty",
            Text::GenerateScript => "script.generate",
            Text::GenerateScriptHint => "script.generate_hint",
            Text::RegenerateScript => "script.regenerate",
            Text::RegenerateScriptHint => "script.regenerate_hint",
            Text::ScriptRunning => "script.running",
            Text::ScriptStopped => "script.stopped",
            Text::SaveScript => "script.save",
            Text::RevertScript => "script.revert",
            Text::ScriptSaved => "script.saved",
            Text::ScriptEdited => "script.edited",
            Text::ScriptWords => "script.words",
            Text::ScriptCurrent => "script.current",
            Text::ScriptPendingTitle => "script.pending.title",
            Text::ScriptPendingHint => "script.pending.hint",
            Text::AcceptScript => "script.pending.accept",
            Text::RejectScript => "script.pending.reject",
            Text::ProvenanceProvider => "provenance.provider",
            Text::ProvenanceModel => "provenance.model",
            Text::ProvenanceTemplate => "provenance.template",
            Text::ProvenanceTemplateVersion => "provenance.template_version",
            Text::ProvenanceTokens => "provenance.tokens",
            Text::ProvenanceTokensValue => "provenance.tokens_value",
            Text::ProvenanceGenerated => "provenance.generated",
            Text::ShowPrompt => "provenance.show_prompt",
            Text::HidePrompt => "provenance.hide_prompt",
            Text::PromptInstructions => "provenance.instructions",
            Text::PromptTask => "provenance.prompt",
            Text::ScriptMissing => "script.error.missing",
            Text::ScriptNothingToReview => "script.error.nothing_to_review",
            Text::ScriptMissingKey => "script.error.missing_key",
            Text::ScriptBusy => "script.error.busy",
            Text::ScriptNotSaved => "script.error.not_saved",
            Text::ScriptFieldError(error) => match error {
                ScriptFieldError::TextRequired => "script.error.text_required",
                ScriptFieldError::TextTooLong => "script.error.text_too_long",
            },
            Text::NarrationTitle => "narration.title",
            Text::NarrationEmpty => "narration.empty",
            Text::GenerateNarration => "narration.generate",
            Text::NarrationGenerateHint => "narration.generate_hint",
            Text::RegenerateNarration => "narration.regenerate",
            Text::NarrationRunning => "narration.running",
            Text::NarrationStopped => "narration.stopped",
            Text::NarrationStale => "narration.stale",
            Text::NarrationStaleTag => "narration.stale_tag",
            Text::NarrationPlay => "narration.play",
            Text::NarrationPause => "narration.pause",
            Text::NarrationWordsHint => "narration.words_hint",
            Text::NarrationVoice => "narration.voice",
            Text::NarrationCost => "narration.cost",
            Text::NarrationCostValue => "narration.cost_value",
            Text::NarrationDuration => "narration.duration",
            Text::NarrationNoScript => "narration.error.no_script",
            Text::NarrationNoPersona => "narration.error.no_persona",
            Text::NarrationVoiceFlagged(flag) => match flag {
                VoiceFlag::Unchecked => "narration.error.voice_unchecked",
                VoiceFlag::Unavailable => "narration.error.voice_unavailable",
            },
            Text::NarrationMissingKey => "narration.error.missing_key",
            Text::NarrationBusy => "narration.error.busy",
            Text::NarrationScenesBusy => "narration.error.scenes_busy",
            Text::NarrationOutdatesPictures => "narration.error.outdates_pictures",
            Text::NarrationOutdatesFit => "narration.outdates.fit",
            Text::NarrationOutdatesReplan => "narration.outdates.replan",
            Text::NarrationOutdatesConfirm => "narration.outdates.confirm",
            Text::NarrationOutdatesCancel => "narration.outdates.cancel",
            Text::NarrationMissing => "narration.error.missing",
            Text::NarrationAudioMissing => "narration.error.audio_missing",
            Text::NarrationCannotPlay => "narration.error.cannot_play",
            Text::NarrationNotLoaded => "narration.error.not_loaded",
            Text::ImportNarration => "narration.import",
            Text::ImportNarrationDialog => "narration.import_dialog",
            Text::ImportNarrationHint => "narration.import_hint",
            Text::RecordingReading => "narration.recording_reading",
            Text::RecordingChosen => "narration.recording_chosen",
            Text::RecordingReplaces => "narration.recording_replaces",
            Text::UseRecording => "narration.use_recording",
            Text::CancelRecording => "narration.cancel_recording",
            Text::NarrationAligning => "narration.aligning",
            Text::NarrationRecording => "narration.recording",
            Text::NarrationAudioValue => "narration.audio_value",
            Text::NarrationImported => "narration.imported",
            Text::RecordingUnreadable => "narration.error.recording_unreadable",
            Text::RecordingUnsupported => "narration.error.recording_unsupported",
            Text::RecordingEmpty => "narration.error.recording_empty",
            Text::RecordingTooLarge => "narration.error.recording_too_large",
            Text::CaptureImport => "capture.import",
            Text::CaptureImportDialog => "capture.import_dialog",
            Text::CaptureImportHint => "capture.import_hint",
            Text::CaptureChosen => "capture.chosen",
            Text::CaptureVoice => "capture.voice",
            Text::CaptureVoicePersona => "capture.voice_persona",
            Text::CaptureVoiceOwn => "capture.voice_own",
            Text::CaptureVoiceOwnShort => "capture.voice_own_short",
            Text::CaptureConfirm => "capture.confirm",
            Text::CaptureSteps => "capture.steps",
            Text::UseCapture => "capture.use",
            Text::CaptureRunning => "capture.running",
            Text::CaptureReplacesNote => "capture.replaces_note",
            Text::NarrationCaptured => "capture.source",
            Text::CaptureSummary => "capture.summary",
            Text::CaptureConfirmed => "capture.confirmed",
            Text::CaptureCoverage => "capture.coverage",
            Text::CaptureTakesTitle => "capture.takes",
            Text::CaptureTakesHint => "capture.takes_hint",
            Text::CaptureTake => "capture.take",
            Text::CaptureTakeSpan => "capture.take_span",
            Text::CaptureTakeOk => "capture.take_ok",
            Text::CaptureTakeTooLong => "capture.take_too_long",
            Text::CaptureTakeTooShort => "capture.take_too_short",
            Text::CaptureCutEarlier => "capture.cut_earlier",
            Text::CaptureCutLater => "capture.cut_later",
            Text::CaptureStale => "capture.stale",
            Text::CaptureRemove => "capture.remove",
            Text::CaptureRemoveHint => "capture.remove_hint",
            Text::CaptureRemoveConfirm => "capture.remove_confirm",
            Text::CaptureRemoveKeep => "capture.remove_keep",
            Text::CaptureNoPersona => "capture.error.no_persona",
            Text::CaptureNotConfirmed => "capture.error.not_confirmed",
            Text::CaptureMissing => "capture.error.missing",
            Text::CaptureNoCutPoint => "capture.error.no_cut_point",
            Text::CaptureNotSaved => "capture.error.not_saved",
            Text::StageTakes => "stage.note.takes",
            Text::StageTakesOne => "stage.note.takes_one",
            Text::StageTakesFlagged => "stage.note.takes_flagged",
            Text::CaptureTakeFlagged => "capture.take_flagged",
            Text::CaptureVoicePersonaNone => "capture.voice_persona_none",
            Text::CaptureModeLabel => "capture.mode_label",
            Text::CaptureModeHint => "capture.mode_hint",
            Text::AnalysisHint => "capture.analysis.hint",
            Text::AnalysisNotStarted => "capture.analysis.not_started",
            Text::AnalysisQueued => "capture.analysis.queued",
            Text::AnalysisRunning => "capture.analysis.running",
            Text::AnalysisRunningLeft => "capture.analysis.running_left",
            Text::AnalysisDone => "capture.analysis.done",
            Text::AnalysisCancelled => "capture.analysis.cancelled",
            Text::AnalysisFailed => "capture.analysis.failed",
            Text::AnalysisOnGpu => "capture.analysis.on_gpu",
            Text::AnalysisOnCpu => "capture.analysis.on_cpu",
            Text::AnalysisCpuNote => "capture.analysis.cpu_note",
            Text::AnalysisRedone => "capture.analysis.redone",
            Text::AnalyzeTake => "capture.analysis.analyze",
            Text::ResumeAnalysis => "capture.analysis.resume",
            Text::CheckNoFace => "capture.check.no_face",
            Text::CheckSeveralFaces => "capture.check.several_faces",
            Text::CheckSmallFace => "capture.check.small_face",
            Text::CheckTurned => "capture.check.turned",
            Text::CheckMaskJumps => "capture.check.mask_jumps",
            Text::ChecksPassed => "capture.check.passed",
            Text::CaptureTakeBlocked => "capture.take_blocked",
            Text::ControlTitle => "capture.control.title",
            Text::ControlHint => "capture.control.hint",
            Text::ControlWaiting => "capture.control.waiting",
            Text::ControlNotBuilt => "capture.control.not_built",
            Text::ControlQueued => "capture.control.queued",
            Text::ControlBuilding => "capture.control.building",
            Text::ControlCancelled => "capture.control.cancelled",
            Text::ControlFailed => "capture.control.failed",
            Text::ControlReady => "capture.control.ready",
            Text::ControlApproved => "capture.control.approved",
            Text::ControlApprovalLost => "capture.control.approval_lost",
            Text::ControlUncovered => "capture.control.uncovered",
            Text::ControlUncoveredBlocked => "capture.control.uncovered_blocked",
            Text::ControlSeveralFacesBlocked => "capture.control.several_faces_blocked",
            Text::ControlReviewHint => "capture.control.review_hint",
            Text::ControlBuild => "capture.control.build",
            Text::ControlBuildAgain => "capture.control.build_again",
            Text::ControlStopBuild => "capture.control.stop_build",
            Text::ControlApprove => "capture.control.approve",
            Text::ControlPlay => "capture.control.play",
            Text::ControlStopPlaying => "capture.control.stop_playing",
            Text::ControlSetFrame => "capture.control.set_frame",
            Text::ControlNotReady => "capture.control.not_ready",
            Text::ControlLengthBlocked => "capture.control.length_blocked",
            Text::ControlCannotPlay => "capture.control.cannot_play",
            Text::ControlOutbound(outbound) => {
                return format!("capture.control.outbound.{}", outbound.code()).into();
            }
            Text::SetLabel => "capture.set.label",
            Text::SetHint => "capture.set.hint",
            Text::SetRoom => "capture.set.room",
            Text::SetSceneImage => "capture.set.scene_image",
            Text::SetSceneNumber => "capture.set.scene_number",
            Text::SetImport => "capture.set.import",
            Text::SetImportDialog => "capture.set.import_dialog",
            Text::SetNoScenes => "capture.set.no_scenes",
            Text::SetUnreadable => "capture.set.unreadable",
            Text::PerformedTitle => "capture.performed.title",
            Text::PerformedHint => "capture.performed.hint",
            Text::TakeModelLabel => "capture.performed.model_label",
            Text::TakeModelChannelTag => "capture.performed.model_channel",
            Text::PerformedBusy => "capture.performed.busy",
            Text::PerformedPending => "capture.performed.pending",
            Text::PerformedMissingClaudeKey => "capture.performed.missing_claude_key",
            Text::PerformedMissingHiggsfieldKey => "capture.performed.missing_higgsfield_key",
            Text::PerformedMissingBytePlusKey => "capture.performed.missing_byteplus_key",
            Text::PerformedNothing => "capture.performed.nothing",
            Text::PerformedDraftFirst => "capture.performed.draft_first",
            Text::PerformedDraftExpired => "capture.performed.draft_expired",
            Text::PerformedDraftFinished => "capture.performed.draft_finished",
            Text::TakeDraftDeadline => "capture.performed.draft_deadline",
            Text::TakeDraftFinishNote => "capture.performed.draft_finish_note",
            Text::TakeModelGone => "capture.performed.model_gone",
            Text::PerformedClipName => "capture.performed.clip_name",
            Text::TakePromptLabel => "capture.performed.prompt_label",
            Text::TakePromptHint => "capture.performed.prompt_hint",
            Text::WriteTakePrompt => "capture.performed.write_prompt",
            Text::WriteTakePromptAgain => "capture.performed.write_prompt_again",
            Text::TakePromptWriting => "capture.performed.prompt_writing",
            Text::TakePromptFailed => "capture.performed.prompt_failed",
            Text::EditTakePrompt => "capture.performed.edit_prompt",
            Text::SaveTakePrompt => "capture.performed.save_prompt",
            Text::CancelTakePrompt => "capture.performed.cancel_prompt",
            Text::TakePromptEdited => "capture.performed.prompt_edited",
            Text::TakePromptFrameNote => "capture.performed.prompt_frame_note",
            Text::TakePromptFrameNoteRecording => "capture.performed.prompt_frame_note_recording",
            Text::MakeTakeDraft => "capture.performed.make_draft",
            Text::MakeTakeFinal => "capture.performed.make_final",
            Text::RegenerateTakeDraft => "capture.performed.regenerate_draft",
            Text::MakeTakeFinalAgain => "capture.performed.make_final_again",
            Text::ApproveTakeDraft => "capture.performed.approve_draft",
            Text::BackToTakeDrafts => "capture.performed.back_to_drafts",
            Text::UsePerformedTake => "capture.performed.use",
            Text::PlayTakeDraft => "capture.performed.play_draft",
            Text::PlayTakeFinal => "capture.performed.play_final",
            Text::TakeMakingDraft => "capture.performed.making_draft",
            Text::TakeMakingFinal => "capture.performed.making_final",
            Text::TakeQueued => "capture.performed.queued",
            Text::StopTakeWaiting => "capture.performed.stop_waiting",
            Text::TakeStoppedWaiting => "capture.performed.stopped",
            Text::CheckTakeAgain => "capture.performed.check_again",
            Text::TakeFailed => "capture.performed.failed",
            Text::TakeRefused => "capture.performed.refused",
            Text::TakeDraftReady => "capture.performed.draft_ready",
            Text::TakeDraftApproved => "capture.performed.draft_approved",
            Text::TakeFinalReady => "capture.performed.final_ready",
            Text::TakeLonger => "capture.performed.longer",
            Text::TakeShorter => "capture.performed.shorter",
            Text::TakeDraftNote => "capture.performed.draft_note",
            Text::TakeFinalPaidNote => "capture.performed.final_paid_note",
            Text::TakeMakeHint => "capture.performed.make_hint",
            Text::PerformedUsed => "capture.performed.used",
            Text::PerformedUsedNow => "capture.performed.used_now",
            Text::PerformedRealisticNote => "capture.performed.realistic_note",
            Text::PerformedBlocked(block) => {
                return format!("capture.performed.blocked.{}", block.code()).into();
            }
            Text::PerformedMissingKlingKey => "capture.performed.missing_kling_key",
            Text::PerformedTestFirst => "capture.performed.test_first",
            Text::TakeCapReachedTitle => "capture.spend.cap_reached",
            Text::TakeCapInvalid => "capture.spend.cap_invalid",
            Text::TakeSpendTitle => "capture.spend.title",
            Text::TakeSpendHint => "capture.spend.hint",
            Text::TakeSpendPartial => "capture.spend.partial",
            Text::TakeSpendOver => "capture.spend.over",
            Text::TakeCapLabel => "capture.spend.cap_label",
            Text::SaveTakeCap => "capture.spend.save_cap",
            Text::ResetTakeCap => "capture.spend.reset_cap",
            Text::EditTakeCap => "capture.spend.edit_cap",
            Text::TakeCapSaved => "capture.spend.cap_saved",
            Text::TakeCapQuestion => "capture.spend.question",
            Text::TakeCapConfirm => "capture.spend.confirm",
            Text::TestTakeTitle => "capture.test.title",
            Text::TestTakeHint => "capture.test.hint",
            Text::TestTakeNone => "capture.test.none",
            Text::TestTakeNeeded => "capture.test.needed",
            Text::TestTakeApproved => "capture.test.approved",
            Text::ApproveTestTake => "capture.test.approve",
            Text::DiscardTestTake => "capture.test.discard",
            Text::PlayTestTake => "capture.test.play",
            Text::MakeTestTakeAgain => "capture.test.make_again",
            Text::PreflightTitle => "capture.preflight.title",
            Text::PreflightHint => "capture.preflight.hint",
            Text::PreflightFailedNote => "capture.preflight.failed",
            Text::PreflightPassed => "capture.preflight.passed",
            Text::PreflightReanalyze => "capture.preflight.reanalyze",
            Text::PreflightLookMeasuring => "capture.preflight.look_measuring",
            Text::PreflightLookNoFace => "capture.preflight.look_no_face",
            Text::SendTakeAnyway => "capture.preflight.send_anyway",
            Text::PreflightOverridden => "capture.preflight.overridden",
            Text::MakeTakeFinalSkipDraft => "capture.performed.make_final_skip_draft",
            Text::TakeDraftOptionalNote => "capture.performed.draft_optional_note",
            Text::MakeTestTake => "capture.test.make",
            Text::TestTakeReady => "capture.test.ready",
            Text::TakeMakingTest => "capture.performed.making_test",
            Text::TakeSpendLine => "capture.spend.line",
            Text::TakeSpendSkipDraft => "capture.performed.skip_draft_cost",
            Text::TestTakeCost => "capture.test.cost",
            Text::PreflightFramingFailed => "capture.preflight.framing_failed",
            Text::PreflightCheckName(check) => {
                return format!("capture.preflight.check.{}", check.code()).into();
            }
            Text::PreflightCheckFailed(check) => {
                return format!("capture.preflight.fail.{}", check.code()).into();
            }
            Text::BodyFramingName(framing) => {
                return format!("capture.preflight.framing.{}", framing.code()).into();
            }
            Text::TakePromptInvalid(error) => match error {
                bardo_domain::TakePromptError::Empty => "capture.performed.prompt_empty",
                bardo_domain::TakePromptError::TooLong => "capture.performed.prompt_too_long",
            },
            Text::TakeModeName(mode) => return format!("capture.mode.{}", mode.code()).into(),
            Text::AnalysisPartName(part) => {
                return format!("capture.analysis.part.{}", part.code()).into();
            }
            Text::CaptureProblem(problem) => match problem {
                CaptureProblem::NotMp4OrMov => "capture.problem.not_mp4_or_mov",
                CaptureProblem::NoVideo => "capture.problem.no_video",
                CaptureProblem::NoAudio => "capture.problem.no_audio",
                CaptureProblem::TooShort => "capture.problem.too_short",
                CaptureProblem::TooLong => "capture.problem.too_long",
                CaptureProblem::FrameRate => "capture.problem.frame_rate",
                CaptureProblem::TooSmall => "capture.problem.too_small",
            },
            Text::ScenesTitle => "scenes.title",
            Text::ScenesEmpty => "scenes.empty",
            Text::PlanScenes => "scenes.plan",
            Text::PlanScenesHint => "scenes.plan_hint",
            Text::ReplanScenes => "scenes.replan",
            Text::ReplanScenesConfirm => "scenes.replan_confirm",
            Text::ConfirmReplanScenes => "scenes.replan_confirm_button",
            Text::CancelReplanScenes => "scenes.replan_cancel",
            Text::ScenesPlanning => "scenes.planning",
            Text::ScenesDrawing => "scenes.drawing",
            Text::ScenesStopping => "scenes.stopping",
            Text::ScenesStopped => "scenes.stopped",
            Text::ScenesStale => "scenes.stale",
            Text::ScenesStaleFit => "scenes.stale_fit",
            Text::ScenesStaleTag => "scenes.stale_tag",
            Text::FitScenes => "scenes.fit",
            Text::FitScenesTitle => "scenes.fit_title",
            Text::FitScenesHint => "scenes.fit_hint",
            Text::ScenesPaidHeld => "scenes.paid_held",
            Text::ScenesPaidHeldReplan => "scenes.paid_held_replan",
            Text::ScenesPaidHeldVersion => "scenes.paid_held_version",
            Text::ScenesCount => "scenes.count",
            Text::GenerateSceneImages => "scenes.generate_images",
            Text::SceneImagesHint => "scenes.images_hint",
            Text::SceneLabel => "scenes.scene",
            Text::SceneEdited => "scenes.edited",
            Text::EditScenePrompt => "scenes.edit_prompt",
            Text::SaveScenePrompt => "scenes.save_prompt",
            Text::CancelScenePrompt => "scenes.cancel_prompt",
            Text::RegenerateSceneImage => "scenes.regenerate_image",
            Text::SceneNoImage => "scenes.no_image",
            Text::SceneFailed => "scenes.failed",
            Text::SceneImageRecord => "scenes.image_record",
            Text::ScenePendingTitle => "scenes.pending.title",
            Text::ScenePendingHint => "scenes.pending.hint",
            Text::AcceptSceneImage => "scenes.pending.accept",
            Text::RejectSceneImage => "scenes.pending.reject",
            Text::ClipsHint => "scenes.clip.hint",
            Text::AnimateMissingClips => "scenes.clip.animate_missing",
            Text::AnimateScene => "scenes.clip.animate",
            Text::AnimateSceneAgain => "scenes.clip.animate_again",
            Text::SceneAnimating => "scenes.clip.animating",
            Text::SceneClipFailed => "scenes.clip.failed",
            Text::SceneClipModel => "scenes.clip.model",
            Text::SceneClipModelChannel => "scenes.clip.model_channel",
            Text::SceneClipModelGone => "scenes.clip.model_gone",
            Text::SceneClipModelReplaced => "scenes.clip.model_replaced",
            Text::ClipsChannelModelReplaced => "scenes.clip.channel_model_replaced",
            Text::SceneClipPlan => "scenes.clip.plan",
            Text::SceneClipPlanUnpriced => "scenes.clip.plan_unpriced",
            Text::SceneMotionPrompt => "scenes.clip.motion_prompt",
            Text::SceneMotionFromImage => "scenes.clip.motion_from_image",
            Text::EditMotionPrompt => "scenes.clip.edit_motion",
            Text::MotionPromptHint => "scenes.clip.motion_hint",
            Text::SceneClipRecord => "scenes.clip.record",
            Text::SceneClipStale => "scenes.clip.stale",
            Text::PlayClip => "scenes.clip.play",
            Text::UseSceneStill => "scenes.clip.use_still",
            Text::ScenePendingClipTitle => "scenes.clip.pending_title",
            Text::ScenePendingClipHint => "scenes.clip.pending_hint",
            Text::AcceptSceneClip => "scenes.clip.accept",
            Text::RejectSceneClip => "scenes.clip.reject",
            Text::ScenesNoNarration => "scenes.error.no_narration",
            Text::ScenesNoPlan => "scenes.error.no_plan",
            Text::ScenesNarrationTooLong => "scenes.error.narration_too_long",
            Text::SceneNotFound => "scenes.error.scene_not_found",
            Text::ScenesMissingClaudeKey => "scenes.error.missing_claude_key",
            Text::ScenesMissingGeminiKey => "scenes.error.missing_gemini_key",
            Text::ScenesMissingHiggsfieldKey => "scenes.error.missing_higgsfield_key",
            Text::ScenesMissingHiggsfieldPlan => "scenes.error.missing_higgsfield_plan",
            Text::ScenesMissingKlingKey => "scenes.error.missing_kling_key",
            Text::SceneNoImageToAnimate => "scenes.error.no_image_to_animate",
            Text::ScenesNothingToAnimate => "scenes.error.nothing_to_animate",
            Text::SceneClipModelNotOffered => "scenes.error.clip_model_not_offered",
            Text::ScenesBusy => "scenes.error.busy",
            Text::ScenesNarrationBusy => "scenes.error.narration_busy",
            Text::ScenesPlanOutOfDate => "scenes.error.plan_out_of_date",
            Text::ScenesOtherText => "scenes.error.other_text",
            Text::ScenesWouldDiscardImages => "scenes.error.would_discard_images",
            Text::ScenesNothingToGenerate => "scenes.error.nothing_to_generate",
            Text::SceneNothingToReview => "scenes.error.nothing_to_review",
            Text::ScenesNotLoaded => "scenes.error.not_loaded",
            Text::SceneFieldError(error) => match error {
                SceneFieldError::PromptRequired => "scenes.error.prompt_required",
                SceneFieldError::PromptTooLong => "scenes.error.prompt_too_long",
            },
            Text::EndCardTitle => "scenes.end_card.title",
            Text::EndCardHint => "scenes.end_card.hint",
            Text::EndCardHeadline => "scenes.end_card.headline",
            Text::EndCardSecondLine => "scenes.end_card.second_line",
            Text::EndCardSecondLineHint => "scenes.end_card.second_line_hint",
            Text::EndCardShowOffer => "scenes.end_card.show_offer",
            Text::EndCardOfferIs => "scenes.end_card.offer_is",
            Text::EndCardNoOffer => "scenes.end_card.no_offer",
            Text::EndCardOfferTooLong => "scenes.end_card.offer_too_long",
            Text::EndCardLook => "scenes.end_card.look",
            Text::EndCardTextPlace => "scenes.end_card.text_place",
            Text::EndCardLogoPlace => "scenes.end_card.logo_place",
            Text::EndCardNoLogo => "scenes.end_card.no_logo",
            Text::EndCardSave => "scenes.end_card.save",
            Text::EndCardSaved => "scenes.end_card.saved",
            Text::EndCardOutOfDate => "scenes.end_card.out_of_date",
            Text::EndCardOff => "scenes.end_card.off",
            Text::AdItemSensitive => "ad_publishing.item.sensitive",
            Text::AdItemUnbackedScript => "ad_publishing.item.unbacked_script",
            Text::AdItemUnbackedPosts => "ad_publishing.item.unbacked_posts",
            Text::AdItemScriptNotChecked => "ad_publishing.item.script_not_checked",
            Text::AdItemPostsNotChecked => "ad_publishing.item.posts_not_checked",
            Text::AdItemUnconfirmed => "ad_publishing.item.unconfirmed",
            Text::AdItemCategory => "ad_publishing.item.category",
            Text::AdItemConcept => "ad_publishing.item.concept",
            Text::AdItemsHeading => "ad_publishing.items_heading",
            Text::AdNotAcknowledged => "ad_publishing.not_acknowledged",
            Text::AdReviewTitle => "ad_publishing.review_title",
            Text::AdReviewHint => "ad_publishing.review_hint",
            Text::AdAcknowledgeAll => "ad_publishing.acknowledge_all",
            Text::AdAcknowledgeLeft => "ad_publishing.acknowledge_left",
            Text::CommercialTitle => "ad_publishing.commercial.title",
            Text::CommercialHint => "ad_publishing.commercial.hint",
            Text::CommercialName(disclosure) => {
                return format!("ad_publishing.commercial.name.{}", disclosure.code()).into();
            }
            Text::CommercialSent(network) => {
                return format!("ad_publishing.commercial.sent.{}", network.code()).into();
            }
            Text::CommercialInStudio => "ad_publishing.commercial.in_studio",
            Text::CommercialTikTokYourBrand => "ad_publishing.commercial.tiktok_your_brand",
            Text::CommercialTikTokBranded => "ad_publishing.commercial.tiktok_branded",
            Text::CommercialReminder => "ad_publishing.commercial.reminder",
            Text::CommercialReminderLabel => "ad_publishing.commercial.reminder_label",
            Text::CommercialReminderTikTokYourBrand => {
                "ad_publishing.commercial.reminder_tiktok_your_brand"
            }
            Text::CommercialReminderTikTokBranded => {
                "ad_publishing.commercial.reminder_tiktok_branded"
            }
            Text::CommercialReminderNoLabel => "ad_publishing.commercial.reminder_no_label",
            Text::CommercialReminderNothing => "ad_publishing.commercial.reminder_nothing",
            Text::CommercialChangeNote => "ad_publishing.commercial.change_note",
            Text::SponsorsLabel => "ad_publishing.sponsors.label",
            Text::SponsorsHint => "ad_publishing.sponsors.hint",
            Text::SponsorError(error) => match error {
                SponsorError::NotAnId => "ad_publishing.sponsors.not_an_id",
                SponsorError::TooMany => "ad_publishing.sponsors.too_many",
            },
            Text::StudioLink => "ad_publishing.studio_link",
            Text::PostsCheckTitle => "ad_publishing.posts_check.title",
            Text::PostsCheckHint => "ad_publishing.posts_check.hint",
            Text::PostsCheckRun => "ad_publishing.posts_check.run",
            Text::PostsCheckRunning => "ad_publishing.posts_check.running",
            Text::PostsCheckNone => "ad_publishing.posts_check.none",
            Text::PostsCheckOutdated => "ad_publishing.posts_check.outdated",
            Text::PostsCheckAllBacked => "ad_publishing.posts_check.all_backed",
            Text::PostsCheckUnbacked => "ad_publishing.posts_check.unbacked",
            Text::PostsCheckSaveFirst => "ad_publishing.posts_check.save_first",
            Text::PostsCheckStopped => "ad_publishing.posts_check.stopped",
            Text::PlacementTitle => "ad_export.title",
            Text::PlacementHint => "ad_export.hint",
            Text::PlacementShapeName => "ad_export.shape",
            Text::PlacementTextsTitle => "ad_export.texts_title",
            Text::PlacementTextsHint => "ad_export.texts_hint",
            Text::PlacementTextsNone => "ad_export.texts_none",
            Text::PlacementPrimary => "ad_export.primary",
            Text::PlacementHeadline => "ad_export.headline",
            Text::PlacementCount => "ad_export.count",
            Text::PlacementTextsSave => "ad_export.texts_save",
            Text::PlacementTextsReplaced => "ad_export.texts_replaced",
            Text::PlacementSaveFirst => "ad_export.save_first",
            Text::PlacementPrimaryEmpty => "ad_export.primary_empty",
            Text::PlacementPrimaryTooLong => "ad_export.primary_too_long",
            Text::PlacementHeadlineEmpty => "ad_export.headline_empty",
            Text::PlacementHeadlineTooLong => "ad_export.headline_too_long",
            Text::PlacementWindowsTitle => "ad_export.windows_title",
            Text::PlacementWindowsHint => "ad_export.windows_hint",
            Text::PlacementWholeWithBars => "ad_export.whole_with_bars",
            Text::PlacementScene => "ad_export.scene",
            Text::PlacementFootage => "ad_export.footage",
            Text::PlacementLeft => "ad_export.left",
            Text::PlacementRight => "ad_export.right",
            Text::PlacementUp => "ad_export.up",
            Text::PlacementDown => "ad_export.down",
            Text::PlacementCenter => "ad_export.center",
            Text::PlacementMoved => "ad_export.moved",
            Text::PlacementStart => "ad_export.start",
            Text::PlacementRunning => "ad_export.running",
            Text::PlacementFailed => "ad_export.failed",
            Text::PlacementCurrent => "ad_export.current",
            Text::PlacementOutdated => "ad_export.outdated",
            Text::PlacementNever => "ad_export.never",
            Text::PlacementShowFolder => "ad_export.show_folder",
            Text::PlacementTextsNotReady => "ad_export.error.texts",
            Text::PlacementNothingChosen => "ad_export.error.nothing_chosen",
            Text::PlacementPicturesWait => "ad_export.error.pictures_wait",
            Text::PlacementAlreadyRunning => "ad_export.error.already_running",
            Text::PlacementFileVideo => "ad_export.file.video",
            Text::PlacementCall => "ad_export.file.call",
            Text::PlacementLink => "ad_export.file.link",
            Text::PlacementCommercialReminder => "ad_export.file.commercial",
            Text::PlacementSyntheticReminder => "ad_export.file.synthetic",
            Text::CardTextPlaceName(place) => {
                return format!("scenes.end_card.text_place_name.{}", place.code()).into();
            }
            Text::CardLogoPlaceName(place) => {
                return format!("scenes.end_card.logo_place_name.{}", place.code()).into();
            }
            Text::CardFieldError(error) => match error {
                CardFieldError::HeadlineEmpty => "scenes.end_card.error.headline_empty",
                CardFieldError::HeadlineTooLong => "scenes.end_card.error.headline_too_long",
                CardFieldError::SecondLineTooLong => "scenes.end_card.error.second_line_too_long",
            },
            Text::TemplatesTitle => "templates.title",
            Text::TemplatesHint => "templates.hint",
            Text::TemplateKindName(kind) => {
                return format!("templates.kind.{}", kind.code()).into();
            }
            Text::TemplateVersionsTitle => "templates.versions",
            Text::TemplateVersionLabel => "templates.version",
            Text::TemplateCurrent => "templates.current",
            Text::TemplateEditing => "templates.editing",
            Text::TemplateInstructions => "templates.instructions",
            Text::TemplateInstructionsHint => "templates.instructions_hint",
            Text::TemplatePrompt => "templates.prompt",
            Text::TemplatePromptHint => "templates.prompt_hint",
            Text::TemplateVariablesTitle => "templates.variables",
            Text::TemplateVariablesHint => "templates.variables_hint",
            Text::TemplateVariableHint(variable) => {
                return format!("templates.variable.{}", variable.name()).into();
            }
            Text::SaveTemplate => "templates.save",
            Text::RevertTemplate => "templates.revert",
            Text::DefaultTemplate => "templates.default",
            Text::TemplateSaved => "templates.saved",
            Text::TemplateUnchanged => "templates.unchanged",
            Text::TemplateNotSaved => "templates.error.not_saved",
            Text::TemplateNotFound => "templates.error.not_found",
            Text::TemplatesNotLoaded => "templates.error.not_loaded",
            Text::TemplateGlobalUsedBy => "templates.error.global_used_by",
            Text::TemplateAlreadyOwn => "templates.error.already_own",
            Text::LibraryNameClash => "library.error.name_clash",
            Text::LibraryOtherProject => "library.error.other_project",
            Text::LibraryGlobalSource => "library.global_source",
            Text::LibraryGlobalTag => "library.global_tag",
            Text::LibraryGlobalNote => "library.global_note",
            Text::LibraryUsesTitle => "library.uses_title",
            Text::LibrarySaveAnyway => "library.save_anyway",
            Text::LibraryCancel => "library.cancel",
            Text::LibraryBring => "library.bring",
            Text::LibraryBringTitle => "library.bring_title",
            Text::LibraryBringHint => "library.bring_hint",
            Text::LibraryBringName => "library.bring_name",
            Text::LibraryBringConfirm => "library.bring_confirm",
            Text::SharingNotFound => "sharing.not_found",
            Text::SharingNotOwn => "sharing.not_own",
            Text::SharingLinkedReadOnly => "sharing.linked_read_only",
            Text::SharingNotShareable => "sharing.not_shareable",
            Text::SharingSameProject => "sharing.same_project",
            Text::SharingNameClash => "sharing.name_clash",
            Text::SharingInvalidName => "sharing.invalid_name",
            Text::SharingFolderUnreachable => "sharing.folder_unreachable",
            Text::SharingConfirmPictures => "sharing.confirm_pictures",
            Text::SharingFileUnreadable => "sharing.file_unreadable",
            Text::SharingFileNotWritten => "sharing.file_not_written",
            Text::SharingNotSaved => "sharing.not_saved",
            Text::SharingNotAnItemFile => "sharing.not_an_item_file",
            Text::SharingNewerItemFile => "sharing.newer_item_file",
            Text::SharingInvalidItemFile => "sharing.invalid_item_file",
            Text::SharingTitle => "sharing.title",
            Text::SharingShareable => "sharing.shareable",
            Text::SharingShareableHint => "sharing.shareable_hint",
            Text::SharingFollows => "sharing.follows",
            Text::SharingFollowsTemplate => "sharing.follows_template",
            Text::SharingLinkedHint => "sharing.linked_hint",
            Text::SharingUpdating => "sharing.updating",
            Text::SharingWaitingForFolder => "sharing.waiting_for_folder",
            Text::SharingUnlink => "sharing.unlink",
            Text::SharingLost => "sharing.lost",
            Text::SharingLostTemplate => "sharing.lost_template",
            Text::SharingLostOk => "sharing.lost_ok",
            Text::SharingExport => "sharing.export",
            Text::SharingExportTitle => "sharing.export_title",
            Text::SharingExportHint => "sharing.export_hint",
            Text::SharingExportTo => "sharing.export_to",
            Text::SharingExportDone => "sharing.export_done",
            Text::SharingNoOtherProject => "sharing.no_other_project",
            Text::SharingMakeGlobal => "sharing.make_global",
            Text::SharingMakeGlobalTitle => "sharing.make_global_title",
            Text::SharingMakeGlobalHint => "sharing.make_global_hint",
            Text::SharingMakeGlobalTemplateHint => "sharing.make_global_template_hint",
            Text::SharingMadeGlobal => "sharing.made_global",
            Text::SharingSaveFile => "sharing.save_file",
            Text::SharingSaved => "sharing.saved",
            Text::SharingOpenFile => "sharing.open_file",
            Text::SharingOpened => "sharing.opened",
            Text::SharingImport => "sharing.import",
            Text::SharingImportHint => "sharing.import_hint",
            Text::SharingImportEmpty => "sharing.import_empty",
            Text::SharingImportLinked => "sharing.import_linked",
            Text::SharingImportIndependent => "sharing.import_independent",
            Text::SharingImported => "sharing.imported",
            Text::SharingArchivedProject => "sharing.archived_project",
            Text::SharingLinkedTag => "sharing.linked_tag",
            Text::SharingShareableTag => "sharing.shareable_tag",
            Text::SharingNameLabel => "sharing.name_label",
            Text::SharingGlobalName => "sharing.global_name",
            Text::SharingConfirmPicturesCheck => "sharing.confirm_pictures_check",
            Text::SharingClose => "sharing.close",
            Text::SharingWrongKind => "sharing.wrong_kind",
            Text::SharingGlobalUsedBy => "sharing.global_used_by",
            Text::LibraryBrought => "library.brought",
            Text::LibraryNoProject => "library.no_project",
            Text::TemplateSourceProject => "templates.source_project",
            Text::TemplateSourceGlobal => "templates.source_global",
            Text::TemplateSourceProjectHint => "templates.source_project_hint",
            Text::TemplateSourceGlobalHint => "templates.source_global_hint",
            Text::TemplateMakeOwn => "templates.make_own",
            Text::TemplateMadeOwn => "templates.made_own",
            Text::TemplateProblem(problem) => match problem {
                TemplateProblem::Required => "templates.error.required",
                TemplateProblem::TooLong => "templates.error.too_long",
                TemplateProblem::UnknownVariable => "templates.error.unknown_variable",
                TemplateProblem::UnclosedVariable => "templates.error.unclosed_variable",
            },
            Text::FetchedJustNow => "age.just_now",
            Text::FetchedMinutesAgo => "age.minutes",
            Text::FetchedHoursAgo => "age.hours",
            Text::FetchedDaysAgo => "age.days",
            Text::NumberThousands => "number.thousands",
            Text::NumberMillions => "number.millions",
            Text::NumberBillions => "number.billions",
            Text::DecimalSeparator => "number.decimal_separator",
            Text::MoneyFormat => "money.format",
            Text::ThousandsSeparator => "money.thousands_separator",
            Text::MoneyUnder => "money.under",
            Text::MoneyError(error) => match error {
                MoneyError::Required => "money.error.required",
                MoneyError::Invalid => "money.error.invalid",
                MoneyError::TooPrecise => "money.error.too_precise",
                MoneyError::TooLarge => "money.error.too_large",
            },
            Text::MonthFormat => "month.format",
            Text::MonthName(number) => return format!("month.{number}").into(),
            Text::MeterName(meter) => return format!("meter.{}", meter.code()).into(),
            Text::MeterUnit(meter) => return format!("meter.unit.{}", meter.code()).into(),
            Text::CostsTitle => "costs.title",
            Text::CostsHint => "costs.hint",
            Text::CostsPreviousMonth => "costs.previous_month",
            Text::CostsNextMonth => "costs.next_month",
            Text::CostsTotal => "costs.total",
            Text::CostsEmpty => "costs.empty",
            Text::CostsUnpriced => "costs.unpriced",
            Text::CostsNotLoaded => "costs.not_loaded",
            Text::CostsNotSaved => "costs.not_saved",
            Text::CostsNotPaid => "costs.not_paid",
            Text::ProjectBudgetHint => "project_keys.budget_hint",
            Text::BackupSetUpProjectKey => "backup.set_up_project_key",
            Text::DeleteRemovesKeys => "delete.removes_keys",
            Text::EstimateProviderOwnKey => "estimate.provider_own_key",
            Text::CostsPaidByProjects => "costs.paid_by_projects",
            Text::CostsDeletedProjectName => "costs.deleted_project_name",
            Text::CostsProjectKeyGone => "costs.project_key_gone",
            Text::CostsProjectKeysHint => "costs.project_keys_hint",
            Text::CostsProjectKeysTitle => "costs.project_keys_title",
            Text::SwitcherProjectKeys => "switcher_package.keys",
            Text::ProjectBudgetNone => "project_keys.budget_none",
            Text::ProjectBudgetAmount => "project_keys.budget_amount",
            Text::ProjectBudgetLabel => "project_keys.budget_label",
            Text::ProjectKeyUsesApp => "project_keys.uses_app",
            Text::ProjectKeysNoProjectOpen => "project_keys.no_project_open",
            Text::ProjectKeysInfo => "project_keys.info",
            Text::ProjectKeysHint => "project_keys.hint",
            Text::ProjectKeysTitle => "project_keys.title",
            Text::SettingsProjectTab => "settings.project_tab",
            Text::DeleteKeysNotRemoved => "delete.keys_not_removed",
            Text::ProjectKeysNoProject => "project_keys.no_such_project",
            Text::ProjectBudgetNeedsKey => "project_keys.budget_needs_key",
            Text::CostsProvidersTitle => "costs.providers_title",
            Text::CostsProvidersHint => "costs.providers_hint",
            Text::CostsChannelsTitle => "costs.channels_title",
            Text::CostsVideosTitle => "costs.videos_title",
            Text::CostsUnknownChannel => "costs.unknown_channel",
            Text::CostsUnknownVideo => "costs.unknown_video",
            Text::BudgetNone => "costs.budget.none",
            Text::BudgetUsed => "costs.budget.used",
            Text::BudgetNear => "costs.budget.near",
            Text::BudgetReached => "costs.budget.reached",
            Text::SetBudget => "costs.budget.set",
            Text::ChangeBudget => "costs.budget.change",
            Text::RemoveBudget => "costs.budget.remove",
            Text::SaveBudget => "costs.budget.save",
            Text::CancelBudget => "costs.budget.cancel",
            Text::BudgetPlaceholder => "costs.budget.placeholder",
            Text::BudgetSaved => "costs.budget.saved",
            Text::BudgetRemoved => "costs.budget.removed",
            Text::RatesTitle => "costs.rates.title",
            Text::RatesHint => "costs.rates.hint",
            Text::RateAllModels => "costs.rates.all_models",
            Text::RateChanged => "costs.rates.changed",
            Text::RateAdded => "costs.rates.added",
            Text::EditRate => "costs.rates.edit",
            Text::SaveRate => "costs.rates.save",
            Text::CancelRate => "costs.rates.cancel",
            Text::ResetRate => "costs.rates.reset",
            Text::RemoveRate => "costs.rates.remove",
            Text::RateSaved => "costs.rates.saved",
            Text::AddRateTitle => "costs.rates.add_title",
            Text::RateProvider => "costs.rates.provider",
            Text::RateModel => "costs.rates.model",
            Text::RateModelPlaceholder => "costs.rates.model_placeholder",
            Text::RateMeter => "costs.rates.meter",
            Text::RatePrice => "costs.rates.price",
            Text::RatePricePlaceholder => "costs.rates.price_placeholder",
            Text::AddRate => "costs.rates.add",
            Text::RateFieldError(error) => match error {
                RateFieldError::ModelTooLong => "costs.rates.error.model_too_long",
                RateFieldError::ModelHasSpaces => "costs.rates.error.model_has_spaces",
                RateFieldError::NotPaid => "costs.not_paid",
                RateFieldError::Price(error) => return Text::MoneyError(error).key(),
            },
            Text::EstimateCost => "estimate.cost",
            Text::EstimatePartial => "estimate.partial",
            Text::EstimateUnknown => "estimate.unknown",
            Text::EstimateNear => "estimate.near",
            Text::EstimateRedraw => "estimate.redraw",
            Text::ProjectSpent => "estimate.project_spent",
            Text::BudgetReachedTitle => "budget.reached_title",
            Text::BudgetReachedLine => "budget.reached_line",
            Text::PriceUnknownTitle => "budget.unknown_title",
            Text::PriceUnknownLine => "budget.unknown_line",
            Text::BudgetQuestion => "budget.question",
            Text::BudgetConfirm => "budget.confirm",
            Text::BudgetCancel => "budget.cancel",
            Text::OpenEditor => "editor.open",
            Text::EditorBack => "editor.back",
            Text::EditorDuration => "editor.duration",
            Text::EditorUndo => "editor.undo",
            Text::EditorRedo => "editor.redo",
            Text::EditorReviewRender => "editor.review_render",
            Text::EditorJobsProxies => "editor.jobs_proxies",
            Text::EditorJobsIdle => "editor.jobs_idle",
            Text::EditorJobFailed => "editor.job_failed",
            Text::EditorBinScenes => "editor.bin.scenes",
            Text::EditorBinMedia => "editor.bin.media",
            Text::EditorBinCaptionStyles => "editor.bin.caption_styles",
            Text::EditorSceneLabel => "editor.scene",
            Text::EditorProxyBadge => "editor.proxy_badge",
            Text::EditorPlay => "editor.play",
            Text::EditorPause => "editor.pause",
            Text::EditorPreviousFrame => "editor.previous_frame",
            Text::EditorNextFrame => "editor.next_frame",
            Text::EditorInspectorEmpty => "editor.inspector.empty",
            Text::EditorInspectorClip => "editor.inspector.clip",
            Text::EditorSourceImage => "editor.inspector.source_image",
            Text::EditorSourceClip => "editor.inspector.source_clip",
            Text::EditorSourceStill => "editor.inspector.source_still",
            Text::EditorSourceNone => "editor.inspector.source_none",
            Text::EditorIn => "editor.inspector.in",
            Text::EditorOut => "editor.inspector.out",
            Text::EditorLength => "editor.inspector.length",
            Text::EditorFile => "editor.inspector.file",
            Text::EditorNarrationLabel => "editor.inspector.narration",
            Text::EditorToolSelect => "editor.tool.select",
            Text::EditorToolSplit => "editor.tool.split",
            Text::EditorToolTransition => "editor.tool.transition",
            Text::EditorSnapWords => "editor.tool.snap_words",
            Text::EditorAiCuts => "editor.tool.ai_cuts",
            Text::EditorRetention => "editor.tool.retention",
            Text::CutsSuggest => "editor.cuts.suggest",
            Text::CutsSuggestAgain => "editor.cuts.suggest_again",
            Text::CutsHint => "editor.cuts.hint",
            Text::CutsAgainHint => "editor.cuts.again_hint",
            Text::CutsNoPoints => "editor.cuts.no_points",
            Text::CutsRunning => "editor.cuts.running",
            Text::CutsStop => "editor.cuts.stop",
            Text::CutsStopped => "editor.cuts.stopped",
            Text::CutsRetry => "editor.cuts.retry",
            Text::CutsEmpty => "editor.cuts.empty",
            Text::CutsAcceptStrong => "editor.cuts.accept_strong",
            Text::CutsFloor => "editor.cuts.floor",
            Text::CutsHidden => "editor.cuts.hidden",
            Text::CutsShown => "editor.cuts.shown",
            Text::CutsShowHidden => "editor.cuts.show_hidden",
            Text::CutsHideLow => "editor.cuts.hide_low",
            Text::CutsTitle => "editor.cuts.title",
            Text::CutsAccept => "editor.cuts.accept",
            Text::CutsReject => "editor.cuts.reject",
            Text::CutsNext => "editor.cuts.next",
            Text::CutsUndo => "editor.cuts.undo",
            Text::CutsAccepted => "editor.cuts.accepted",
            Text::CutsConfidence => "editor.cuts.confidence",
            Text::CutsReasonSentence => "editor.cuts.reason.sentence_end",
            Text::CutsReasonPause => "editor.cuts.reason.pause",
            Text::CutsReasonScene => "editor.cuts.reason.scene_change",
            Text::CutsReasonTopic => "editor.cuts.reason.topic_shift",
            Text::CutsMissingKey(provider) => {
                return format!("editor.cuts.missing_key.{}", provider.code()).into();
            }
            Text::CutsBusy => "editor.cuts.busy",
            Text::CutsGone => "editor.cuts.gone",
            Text::CutsNotSaved => "editor.cuts.not_saved",
            Text::CutsFloorNotSaved => "editor.cuts.floor_not_saved",
            Text::EditorDuckMusic => "editor.tool.duck_music",
            Text::EditorZoomIn => "editor.tool.zoom_in",
            Text::EditorZoomOut => "editor.tool.zoom_out",
            Text::EditorTrackCaptions => "editor.track.captions",
            Text::EditorTrackVideo => "editor.track.video",
            Text::EditorTrackNarration => "editor.track.narration",
            Text::EditorTrackMusic => "editor.track.music",
            Text::EditorTrackSfx => "editor.track.sfx",
            Text::EditorEmptyTitle => "editor.empty.title",
            Text::EditorEmptyNoNarration => "editor.empty.no_narration",
            Text::EditorEmptyNoScenes => "editor.empty.no_scenes",
            Text::EditorBackToProject => "editor.empty.back",
            Text::EditorBuildingProxies => "editor.building",
            Text::EditorKeepEditing => "editor.keep_editing",
            Text::EditorMediaMissing => "editor.clip.missing",
            Text::EditorProxyFailed => "editor.clip.proxy_failed",
            Text::EditorProxyCancelled => "editor.clip.proxy_cancelled",
            Text::EditorProxyBuilding => "editor.clip.proxy_building",
            Text::EditorProblemsOne => "editor.problems.one",
            Text::EditorProblemsMany => "editor.problems.many",
            Text::EditorProblemLine => "editor.problems.line",
            Text::EditorProblemNoImage => "editor.problems.no_image",
            Text::EditorProblemFileGone => "editor.problems.file_gone",
            Text::EditorProblemCancelled => "editor.problems.cancelled",
            Text::EditorRetryProxies => "editor.problems.retry",
            Text::EditorMediaOffline => "editor.preview.offline",
            Text::EditorStale => "editor.stale",
            Text::EditorPreviewBuilding => "editor.error.building",
            Text::EditorNothingToRetry => "editor.error.nothing_to_retry",
            Text::EditorFfmpegMissing => "editor.error.ffmpeg_missing",
            Text::EditorPreviewFailed => "editor.error.preview_failed",
            Text::EditorNotLoaded => "editor.error.not_loaded",
            Text::EditorNoSound => "editor.error.no_sound",
            Text::EditorNothingToCut => "editor.error.nothing_to_cut",
            Text::EditorCannotEdit => "editor.error.cannot_edit",
            Text::EditorCaptionTextInvalid => "editor.error.caption_text",
            Text::EditorPerformedNotHeard => "editor.error.performed_not_heard",
            Text::EditorMediaTake => "editor.media.take",
            Text::EditorMediaPutOnCut => "editor.media.put_on_cut",
            Text::EditorMediaPerformedRoughCut => "editor.media.performed_rough_cut",
            Text::EditorMediaPerformedRoughCutHint => "editor.media.performed_rough_cut_hint",
            Text::EditorPerformedTitle => "editor.performed.title",
            Text::EditorPerformedInSync => "editor.performed.in_sync",
            Text::EditorPerformedEarly => "editor.performed.early",
            Text::EditorPerformedLate => "editor.performed.late",
            Text::EditorPerformedUnheard => "editor.performed.unheard",
            Text::EditorPerformedPutBack => "editor.performed.put_back",
            Text::EditorPerformedPutBackHint => "editor.performed.put_back_hint",
            Text::EditorPerformedCannotSync => "editor.performed.cannot_sync",
            Text::EditorPerformedUnheardHint => "editor.performed.unheard_hint",
            Text::EditorPerformedCover => "editor.performed.cover",
            Text::EditorPerformedCoverHint => "editor.performed.cover_hint",
            Text::EditorPerformedCoverScene => "editor.performed.cover_scene",
            Text::EditorPerformedCoverLength => "editor.performed.cover_length",
            Text::EditorPerformedCoverButton => "editor.performed.cover_button",
            Text::EditorEditNotSaved => "editor.error.edit_not_saved",
            Text::EditorCutReset => "editor.cut_reset",
            Text::EditorInspectorAudio => "editor.inspector.audio",
            Text::EditorSourceIn => "editor.inspector.source_in",
            Text::EditorRemove => "editor.inspector.remove",
            Text::EditorShortcuts => "editor.inspector.shortcuts",
            Text::EditorInspectorTrack => "editor.mix.inspector_track",
            Text::EditorLevel => "editor.mix.level",
            Text::EditorMute => "editor.mix.mute",
            Text::EditorSolo => "editor.mix.solo",
            Text::EditorMuteShort => "editor.mix.mute_short",
            Text::EditorSoloShort => "editor.mix.solo_short",
            Text::EditorDuck => "editor.mix.duck",
            Text::EditorDuckDepth => "editor.mix.duck_depth",
            Text::EditorDuckHint => "editor.mix.duck_hint",
            Text::EditorDuckReadout => "editor.mix.duck_readout",
            Text::EditorNoMusic => "editor.mix.no_music",
            Text::EditorFades => "editor.mix.fades",
            Text::EditorFadeIn => "editor.mix.fade_in",
            Text::EditorFadeOut => "editor.mix.fade_out",
            Text::EditorTransition => "editor.transition.title",
            Text::EditorTransitionLength => "editor.transition.length",
            Text::EditorTransitionEverywhere => "editor.transition.everywhere",
            Text::EditorTransitionFirst => "editor.transition.first",
            Text::EditorTransitionHint => "editor.transition.hint",
            Text::EditorTransitionShort => "editor.transition.short",
            Text::EditorMotion => "editor.motion.title",
            Text::EditorMotionStrength => "editor.motion.strength",
            Text::EditorMotionEverywhere => "editor.motion.everywhere",
            Text::EditorMotionVary => "editor.motion.vary",
            Text::EditorMotionHint => "editor.motion.hint",
            Text::EditorMotionClip => "editor.motion.clip",
            Text::EditorSeconds => "editor.seconds",
            Text::EditorDecibels => "editor.mix.decibels",
            Text::EditorLaneHint => "editor.mix.lane_hint",
            Text::EditorInspectorCaption => "editor.inspector.caption",
            Text::EditorCaptionText => "editor.captions.text",
            Text::EditorCaptionTextHint => "editor.captions.text_hint",
            Text::EditorCaptionStyle => "editor.captions.style",
            Text::EditorCaptionStyleHint => "editor.captions.style_hint",
            Text::EditorCaptionHighlight => "editor.captions.highlight",
            Text::EditorCaptionHighlightHint => "editor.captions.highlight_hint",
            Text::EditorShowCaptions => "editor.captions.show",
            Text::EditorFraming => "editor.framing.title",
            Text::EditorFramingFit => "editor.framing.fit",
            Text::EditorFramingFill => "editor.framing.fill",
            Text::EditorFramingCustom => "editor.framing.custom",
            Text::EditorFramingPosition => "editor.framing.position",
            Text::EditorFramingLandscapeHint => "editor.framing.landscape_hint",
            Text::EditorFramingDragHint => "editor.framing.drag_hint",
            Text::EditorFramingFitHint => "editor.framing.fit_hint",
            Text::EditorFramingSource => "editor.framing.source",
            Text::EditorMediaImport => "editor.media.import",
            Text::EditorMediaImportHint => "editor.media.import_hint",
            Text::EditorMediaImportingOne => "editor.media.importing_one",
            Text::EditorMediaImporting => "editor.media.importing",
            Text::EditorMediaEmpty => "editor.media.empty",
            Text::EditorMediaAddMusic => "editor.media.add_music",
            Text::EditorMediaAddSfx => "editor.media.add_sfx",
            Text::EditorMediaAddVideo => "editor.media.add_video",
            Text::EditorMediaAddHint => "editor.media.add_hint",
            Text::EditorMediaImportFailed => "editor.media.import_failed",
            Text::EditorMediaAudio => "editor.media.audio",
            Text::EditorMediaVideo => "editor.media.video",
            Text::EditorSourceFootage => "editor.inspector.source_footage",
            Text::EditorSourcePerformed => "editor.inspector.source_performed",
            Text::EditorProblemFootageLine => "editor.problems.footage_line",
            Text::MediaImportUnreadable => "editor.media.error.unreadable",
            Text::MediaImportUnsupported => "editor.media.error.unsupported",
            Text::MediaImportTooShort => "editor.media.error.too_short",
            Text::MediaImportNotSaved => "editor.media.error.not_saved",
            Text::MusicPromptTitle => "music.title",
            Text::MusicPromptEmpty => "music.empty",
            Text::MusicPromptGenerate => "music.generate",
            Text::MusicPromptGenerateHint => "music.generate_hint",
            Text::MusicPromptRegenerate => "music.regenerate",
            Text::MusicPromptRegenerateHint => "music.regenerate_hint",
            Text::MusicPromptRunning => "music.running",
            Text::MusicPromptStopped => "music.stopped",
            Text::MusicPromptSave => "music.save",
            Text::MusicPromptRevert => "music.revert",
            Text::MusicPromptSaved => "music.saved",
            Text::MusicPromptCopy => "music.copy",
            Text::MusicPromptCopied => "music.copied",
            Text::MusicPromptEdited => "music.edited",
            Text::MusicPromptMissing => "music.error.missing",
            Text::MusicPromptMissingKey => "music.error.missing_key",
            Text::MusicPromptBusy => "music.error.busy",
            Text::MusicPromptNotSaved => "music.error.not_saved",
            Text::ChapterRename => "chapters.rename",
            Text::ChaptersStopped => "chapters.stopped",
            Text::ThumbnailWriteStopped => "thumbnail.write_stopped",
            Text::ThumbnailDrawStopped => "thumbnail.draw_stopped",
            Text::MetadataChaptersNote => "metadata.chapters_note",
            Text::ThumbnailTitle => "thumbnail.title",
            Text::ThumbnailHint => "thumbnail.hint",
            Text::ThumbnailShortNote => "thumbnail.short_note",
            Text::ThumbnailWrite => "thumbnail.write",
            Text::ThumbnailWriteHint => "thumbnail.write_hint",
            Text::ThumbnailRewrite => "thumbnail.rewrite",
            Text::ThumbnailWriting => "thumbnail.writing",
            Text::ThumbnailText => "thumbnail.text",
            Text::ThumbnailTextHint => "thumbnail.text_hint",
            Text::ThumbnailPlacement => "thumbnail.placement",
            Text::ThumbnailLookLabel => "thumbnail.look",
            Text::ThumbnailBackground => "thumbnail.background",
            Text::ThumbnailBackgroundHint => "thumbnail.background_hint",
            Text::ThumbnailBackgroundsEmpty => "thumbnail.backgrounds_empty",
            Text::ThumbnailBackgroundDrawn => "thumbnail.background_drawn",
            Text::ThumbnailBackgroundScene => "thumbnail.background_scene",
            Text::ThumbnailPrompt => "thumbnail.prompt",
            Text::ThumbnailPromptHint => "thumbnail.prompt_hint",
            Text::ThumbnailDraw => "thumbnail.draw",
            Text::ThumbnailDrawing => "thumbnail.drawing",
            Text::ThumbnailSave => "thumbnail.save",
            Text::ThumbnailSaving => "thumbnail.saving",
            Text::ThumbnailSaved => "thumbnail.saved",
            Text::ThumbnailOutdated => "thumbnail.outdated",
            Text::ThumbnailNotYetSaved => "thumbnail.not_yet_saved",
            Text::ThumbnailApply => "thumbnail.apply",
            Text::ThumbnailWriteMissingKey => "thumbnail.error.missing_key",
            Text::ThumbnailDrawMissingKey => "thumbnail.error.draw_missing_key",
            Text::ThumbnailBusy => "thumbnail.error.busy",
            Text::ThumbnailNoPrompt => "thumbnail.error.no_prompt",
            Text::ThumbnailNoBackground => "thumbnail.error.no_background",
            Text::ThumbnailBackgroundMissing => "thumbnail.error.background_missing",
            Text::ThumbnailNotComposed => "thumbnail.error.not_composed",
            Text::ThumbnailNotSaved => "thumbnail.error.not_saved",
            Text::ChaptersTitle => "chapters.title",
            Text::ChaptersHint => "chapters.hint",
            Text::ChaptersShortNote => "chapters.short_note",
            Text::ChaptersWrite => "chapters.write",
            Text::ChaptersWriteHint => "chapters.write_hint",
            Text::ChaptersRewrite => "chapters.rewrite",
            Text::ChaptersWriting => "chapters.writing",
            Text::ChaptersEmpty => "chapters.empty",
            Text::ChaptersInclude => "chapters.include",
            Text::ChaptersEdited => "chapters.edited",
            Text::ChapterEarlier => "chapters.earlier",
            Text::ChapterLater => "chapters.later",
            Text::ChapterRemove => "chapters.remove",
            Text::ChapterCutAway => "chapters.cut_away",
            Text::ChaptersStale => "chapters.stale",
            Text::ChaptersLeftOut => "chapters.left_out",
            Text::ChaptersNoCut => "chapters.no_cut",
            Text::ChaptersNoNarration => "chapters.error.no_narration",
            Text::ChaptersMissing => "chapters.error.missing",
            Text::ChapterCannotMove => "chapters.error.cannot_move",
            Text::ChaptersMissingKey => "chapters.error.missing_key",
            Text::ChaptersBusy => "chapters.error.busy",
            Text::ChaptersNotSaved => "chapters.error.not_saved",
            Text::UploadThumbnail => "upload.thumbnail.send",
            Text::UploadThumbnailHint => "upload.thumbnail.send_hint",
            Text::UploadThumbnailSet => "upload.thumbnail.set",
            Text::UploadThumbnailNotAllowed => "upload.thumbnail.not_allowed",
            Text::UploadThumbnailFailed => "upload.thumbnail.failed",
            Text::ExportFileThumbnail => "export.file.thumbnail",
            Text::ChannelThumbnailLook => "channel.thumbnail_look",
            Text::ChannelThumbnailLookHint => "channel.thumbnail_look_hint",
            Text::ThumbnailFieldError(ThumbnailFieldError::TextTooLong) => {
                "thumbnail.error.text_too_long"
            }
            Text::ChapterFieldError(error) => match error {
                ChapterFieldError::TitleRequired => "chapters.error.title_required",
                ChapterFieldError::TitleTooLong => "chapters.error.title_too_long",
            },
            Text::ChapterProblem(problem) => match problem {
                ChapterProblem::TooFew => "chapters.problem.too_few",
                ChapterProblem::TooShort(_) => "chapters.problem.too_short",
                ChapterProblem::CutAway(_) => "chapters.problem.cut_away",
                ChapterProblem::DoesNotFit => "chapters.problem.does_not_fit",
            },
            Text::TextPlacementName(placement) => {
                return format!("thumbnail.placement_name.{}", placement.code()).into();
            }
            Text::ThumbnailLookName(look) => {
                return format!("thumbnail.look_name.{}", look.code()).into();
            }
            Text::MusicPromptFieldError(error) => match error {
                MusicPromptFieldError::TextRequired => "music.error.text_required",
                MusicPromptFieldError::TextTooLong => "music.error.text_too_long",
            },
            Text::MusicTrackHeading => "music.track.heading",
            Text::MusicTrackEmpty => "music.track.empty",
            Text::MusicTrackGenerate => "music.track.generate",
            Text::MusicTrackGenerateHint => "music.track.generate_hint",
            Text::MusicTrackRegenerate => "music.track.regenerate",
            Text::MusicTrackRegenerateHint => "music.track.regenerate_hint",
            Text::MusicTrackRunning => "music.track.running",
            Text::MusicTrackStopped => "music.track.stopped",
            Text::MusicTrackNew => "music.track.new",
            Text::MusicTrackInUse => "music.track.in_use",
            Text::MusicTrackDetails => "music.track.details",
            Text::MusicTrackPlay => "music.track.play",
            Text::MusicTrackPause => "music.track.pause",
            Text::MusicTrackUse => "music.track.use",
            Text::MusicTrackUseHint => "music.track.use_hint",
            Text::MusicTrackDiscard => "music.track.discard",
            Text::MusicTrackUsed => "music.track.used",
            Text::MusicTrackDiscarded => "music.track.discarded",
            Text::MusicTrackNoNarration => "music.error.no_narration",
            Text::MusicTrackMissingKey => "music.error.track_missing_key",
            Text::MusicTrackBusy => "music.error.track_busy",
            Text::MusicTrackNotFound => "music.error.track_not_found",
            Text::MusicTrackAudioMissing => "music.error.track_audio_missing",
            Text::MusicTrackCannotPlay => "music.error.track_cannot_play",
            Text::MusicTrackNotSaved => "music.error.track_not_saved",
            Text::MusicTrackNoRoom => "music.error.track_no_room",
            Text::SettingsKeysTab => "settings.keys_tab",
            Text::SettingsAppearanceTab => "settings.appearance_tab",
            Text::AppearanceTheme => "appearance.theme",
            Text::AppearanceFollowSystem => "appearance.follow_system",
            Text::AppearanceFollowSystemHint => "appearance.follow_system_hint",
            Text::AppearanceLight => "appearance.light",
            Text::AppearanceDark => "appearance.dark",
            Text::AppearanceFixed => "appearance.fixed",
            Text::AppearanceFixedHint => "appearance.fixed_hint",
            Text::AppearanceContrast => "appearance.contrast",
            Text::UiThemeNotSaved => "error.ui_theme_not_saved",
            Text::UiThemeName(theme) => match theme {
                UiTheme::Paper => "ui_theme.paper",
                UiTheme::Sand => "ui_theme.sand",
                UiTheme::Graphite => "ui_theme.graphite",
                UiTheme::Slate => "ui_theme.slate",
                UiTheme::HighContrastLight => "ui_theme.hc-light",
                UiTheme::HighContrastDark => "ui_theme.hc-dark",
                UiTheme::BlackGold => "ui_theme.black-gold",
                UiTheme::Brass => "ui_theme.brass",
                UiTheme::Phosphor => "ui_theme.phosphor",
                UiTheme::PhosphorLight => "ui_theme.phosphor-light",
            },
            Text::UiThemeKind(theme) => match (theme.family(), theme.mode()) {
                (ThemeFamily::HighContrast, _) => "ui_theme_kind.high_contrast",
                (ThemeFamily::Terminal, ThemeMode::Light) => "ui_theme_kind.terminal_light",
                (ThemeFamily::Terminal, ThemeMode::Dark) => "ui_theme_kind.terminal_dark",
                (ThemeFamily::Base, ThemeMode::Light) => "ui_theme_kind.light",
                (ThemeFamily::Base, ThemeMode::Dark) => "ui_theme_kind.dark",
            },
            Text::ProviderKeysInfo => "provider_keys.info",
            Text::Details => "app.details",
            Text::NavBudgetsUsed => "nav.budgets_used",
            Text::AppearanceLayout => "appearance.layout.title",
            Text::AppearanceLayoutHint => "appearance.layout.hint",
            Text::UiLayoutName(layout) => match layout {
                LayoutId::Workspace => "ui_layout.workspace",
                LayoutId::Studio => "ui_layout.studio",
            },
            Text::UiLayoutDescription(layout) => match layout {
                LayoutId::Workspace => "ui_layout.description.workspace",
                LayoutId::Studio => "ui_layout.description.studio",
            },
            Text::UiLayoutNotSaved => "ui_layout.not_saved",
            Text::StatusJobs => "status.jobs",
            Text::StatusOneJob => "status.one_job",
            Text::StatusNoJobs => "status.no_jobs",
            Text::StatusMonthSpend => "status.month_spend",
            Text::SceneColumnPicture => "scenes.column.picture",
            Text::SceneColumnTime => "scenes.column.time",
            Text::SceneColumnNarration => "scenes.column.narration",
            Text::SceneColumnPrompt => "scenes.column.prompt",
            Text::SceneColumnImage => "scenes.column.image",
            Text::SceneColumnClip => "scenes.column.clip",
            Text::SceneColumnModel => "scenes.column.model",
            Text::SceneColumnCost => "scenes.column.cost",
            Text::SceneStateDone => "scenes.state.done",
            Text::SceneKeysHint => "scenes.keys.hint",
            Text::CostsBriefSpent => "costs.brief.spent",
            Text::CostsBriefOver => "costs.brief.over",
            Text::CostsBriefNear => "costs.brief.near",
            Text::CostsBriefUnpriced => "costs.brief.unpriced",
            Text::StageScriptMissing => "stage.note.script_missing",
            Text::StageScriptToReview => "stage.note.script_to_review",
            Text::StageScriptWords => "stage.note.script_words",
            Text::StageNarrationMissing => "stage.note.narration_missing",
            Text::StageNarrationStale => "stage.note.narration_stale",
            Text::StageScenesMissing => "stage.note.scenes_missing",
            Text::StageScenesStale => "stage.note.scenes_stale",
            Text::VersionsTitle => "versions.title",
            Text::VersionsHint => "versions.hint",
            Text::VersionTargetLine => "versions.target",
            Text::VersionTargetNoNarrator => "versions.target_no_narrator",
            Text::MakeVersion => "versions.make",
            Text::MakeVersionHint => "versions.make_hint",
            Text::VersionMaking => "versions.making",
            Text::VersionStopped => "versions.stopped",
            Text::VersionsMade => "versions.made",
            Text::VersionMadeLine => "versions.made_line",
            Text::OpenVersion => "versions.open",
            Text::VersionOf => "versions.of",
            Text::VersionOfChannel => "versions.of_channel",
            Text::VersionSourceGone => "versions.source_gone",
            Text::OpenVersionSource => "versions.open_source",
            Text::VersionAdaptHint => "versions.adapt_hint",
            Text::VersionSameAudience => "versions.error.same_audience",
            Text::VersionAlreadyMade => "versions.error.already_made",
            Text::VersionNoNarration => "versions.error.no_narration",
            Text::VersionNoPlan => "versions.error.no_plan",
            Text::VersionNoChannel => "versions.error.no_channel",
            Text::VersionBusy => "versions.error.busy",
            Text::VersionNotSaved => "versions.error.not_saved",
            Text::PlaceScenesTitle => "scenes.place.title",
            Text::PlaceScenesHint => "scenes.place.hint",
            Text::PlaceScenes => "scenes.place.button",
            Text::PlaceScenesNothing => "scenes.place.nothing",
            Text::PlacedScenesTitle => "scenes.placed.title",
            Text::PlacedScenesAll => "scenes.placed.all",
            Text::PlacedScenePastClip => "scenes.placed.past_clip",
            Text::PlacedSceneStretched => "scenes.placed.stretched",
            Text::PlacedSceneDropped => "scenes.placed.dropped",
            Text::StageScenesToPlace => "stage.note.scenes_to_place",
            Text::StageToReview => "stage.note.to_review",
            Text::StageImages => "stage.note.images",
            Text::StageClips => "stage.note.clips",
            Text::StageClipsNone => "stage.note.clips_none",
            Text::StageEditorReady => "stage.note.editor_ready",
            Text::StageWorking => "stage.note.working",
            Text::FilterAll => "scenes.filter.all",
            Text::FilterPending => "scenes.filter.pending",
            Text::FilterPendingEmpty => "scenes.filter.pending_empty",
            Text::SceneCardReview => "scenes.card.review",
            Text::SceneCardFailed => "scenes.card.failed",
            Text::SceneCardToDraw => "scenes.card.to_draw",
            Text::SceneCardToAnimate => "scenes.card.to_animate",
            Text::SceneCardAnimating => "scenes.card.animating",
            Text::SceneImagePrompt => "scenes.inspector.image_prompt",
            Text::SceneCast => "scenes.cast.title",
            Text::SceneCastHint => "scenes.cast.hint",
            Text::CastPortraitLater => "scenes.cast.later",
            Text::EditCastDescription => "scenes.cast.edit",
            Text::RedrawCastPortrait => "scenes.cast.redraw",
            Text::CastPortraitFailed => "scenes.cast.failed",
            Text::CastEdited => "scenes.cast.edited",
            Text::CastDescribedOnly => "scenes.cast.described_only",
            Text::CastDescribedOnlyHint => "scenes.cast.described_only_hint",
            Text::CastDrawWithoutPortrait => "scenes.cast.draw_without",
            Text::CastUsePortrait => "scenes.cast.use_portrait",
            Text::SceneProps => "scenes.props.title",
            Text::ScenePropsHint => "scenes.props.hint",
            Text::PropPictureLater => "scenes.props.later",
            Text::EditPropDescription => "scenes.props.edit",
            Text::RedrawPropPicture => "scenes.props.redraw",
            Text::PropPictureFailed => "scenes.props.failed",
            Text::PropDescribedOnly => "scenes.props.described_only",
            Text::PropDescribedOnlyHint => "scenes.props.described_only_hint",
            Text::PropDrawWithoutPicture => "scenes.props.draw_without",
            Text::PropUsePicture => "scenes.props.use_picture",
            Text::PropSaveHint => "scenes.props.save_hint",
            Text::ScenePlace => "scenes.same_place.title",
            Text::ScenePlaceHint => "scenes.same_place.hint",
            Text::EditPlaceDescription => "scenes.same_place.edit",
            Text::PlaceEdited => "scenes.same_place.edited",
            Text::ScenePlaceLeft => "scenes.same_place.left",
            Text::SceneCurrentImage => "scenes.inspector.current",
            Text::SceneNewImage => "scenes.inspector.new",
            Text::ProjectSwitch => "projects.switch",
            Text::GenerationDetails => "scenes.inspector.generation_details",
            Text::CostsOverview => "costs.overview",
            Text::CostsAcrossProviders => "costs.across_providers",
            Text::CostsBudgetsTile => "costs.budgets_tile",
            Text::CostsBudgetsInAlert => "costs.budgets_in_alert",
            Text::CostsNoBudgets => "costs.no_budgets",
            Text::CostsUnpricedTitle => "costs.unpriced_title",
            Text::CostsUnpricedModel => "costs.unpriced_model",
            Text::CostsUnpricedModels => "costs.unpriced_models",
            Text::CostsUnpricedNone => "costs.unpriced_none",
            Text::AddPrice => "costs.add_price",
            Text::CostsColumnProvider => "costs.column.provider",
            Text::CostsColumnUsage => "costs.column.usage",
            Text::CostsColumnSpent => "costs.column.spent",
            Text::CostsColumnBudget => "costs.column.budget",
            Text::CostsColumnState => "costs.column.state",
            Text::NavSectionName(section) => match section {
                NavSection::Project => "nav.section.project",
                NavSection::Library => "nav.section.library",
            },
            Text::NavEntryName(entry) => match entry {
                NavEntry::Overview => "nav.place.overview",
                NavEntry::Ideas => "nav.entry.ideas",
                NavEntry::Videos => "nav.place.videos",
                NavEntry::World => "nav.place.world",
                NavEntry::Narrators => "nav.place.narrators",
                NavEntry::Products => "nav.place.products",
                NavEntry::Templates => "nav.place.templates",
                NavEntry::ChannelsAndAccounts => "nav.entry.channels_and_accounts",
                NavEntry::Performance => "nav.place.performance",
                NavEntry::Library => "nav.place.library",
            },
            Text::DestinationName(place) => match place {
                Destination::Overview => "nav.place.overview",
                Destination::Research => "nav.place.research",
                Destination::Themes => "nav.place.themes",
                Destination::Products => "nav.place.products",
                Destination::Performance => "nav.place.performance",
                Destination::Videos => "nav.place.videos",
                Destination::World => "nav.place.world",
                Destination::Narrators => "nav.place.narrators",
                Destination::Templates => "nav.place.templates",
                Destination::Channels => "nav.place.channels",
                Destination::Accounts => "nav.place.accounts",
                Destination::Library => "nav.place.library",
                Destination::Jobs => "nav.place.jobs",
                Destination::Costs => "nav.place.costs",
                Destination::Settings => "nav.place.settings",
                Destination::Guide => "nav.place.guide",
            },
            Text::RenderNoCut => "render.error.no_cut",
            Text::RenderChecking => "render.checking",
            Text::RenderNothingChosen => "render.error.nothing_chosen",
            Text::RenderBlocked => "render.error.blocked",
            Text::RenderCutChanged => "render.error.cut_changed",
            Text::RenderAlreadyRunning => "render.error.already_running",
            Text::RenderWhileExporting => "render.error.while_exporting",
            Text::RenderWhileUploading => "render.error.while_uploading",
            Text::RenderCheckFailed => "render.error.check_failed",
            Text::RenderNotLoaded => "render.error.not_loaded",
            Text::RenderNoAccounts => "render.no_accounts",
            Text::RenderInfo => "render.info",
            Text::RenderFigureLength => "render.figure.length",
            Text::RenderFigureFrame => "render.figure.frame",
            Text::RenderFigureLoudness => "render.figure.loudness",
            Text::RenderFigureCaptions => "render.figure.captions",
            Text::RenderCaptionsOn => "render.captions_on",
            Text::RenderCaptionsOff => "render.captions_off",
            Text::RenderMeasuring => "render.measuring",
            Text::RenderLufs => "render.lufs",
            Text::RenderSilent => "render.silent",
            Text::RenderTargets => "render.targets",
            Text::RenderChosen => "render.chosen",
            Text::RenderInclude => "render.include",
            Text::GateTooLong => "render.gate.too_long",
            Text::GateNoEncoder => "render.gate.no_encoder",
            Text::GateReframed => "render.gate.reframed",
            Text::GateReframedWide => "render.gate.reframed_wide",
            Text::GateCaptionsOff => "render.gate.captions_off",
            Text::GateMissingMedia => "render.gate.missing_media",
            Text::GateSilent => "render.gate.silent",
            Text::GateLoudnessFar => "render.gate.loudness_far",
            Text::GatePeaksLimited => "render.gate.peaks_limited",
            Text::GateLongForUnverified => "render.gate.long_for_unverified",
            Text::GateBlocks => "render.gate.blocks",
            Text::GateWarning => "render.gate.warning",
            Text::RenderStateReady => "render.state.ready",
            Text::RenderStateBlocked => "render.state.blocked",
            Text::RenderStateWarnings => "render.state.warnings",
            Text::RenderStateChecking => "render.state.checking",
            Text::RenderLastCurrent => "render.last.current",
            Text::RenderLastOutdated => "render.last.outdated",
            Text::RenderLastNone => "render.last.none",
            Text::RenderLastFile => "render.last.file",
            Text::RenderLastOutdatedHint => "render.last.outdated_hint",
            Text::RenderLoudnessOnTarget => "render.loudness_on_target",
            Text::RenderLoudnessOffTarget => "render.loudness_off_target",
            Text::RenderShowFile => "render.show_file",
            Text::RenderSize => "render.size",
            Text::RenderColumnPreset => "render.column.preset",
            Text::RenderColumnChecks => "render.column.checks",
            Text::RenderColumnLast => "render.column.last",
            Text::RenderEncoder => "render.encoder",
            Text::RenderEncoderHardware => "render.encoder_hardware",
            Text::RenderEncoderSoftware => "render.encoder_software",
            Text::RenderStart => "render.start",
            Text::RenderCheckAgain => "render.check_again",
            Text::RenderConfirmTitle => "render.confirm.title",
            Text::RenderConfirmBody => "render.confirm.body",
            Text::RenderConfirmWarnings => "render.confirm.warnings",
            Text::RenderConfirm => "render.confirm.go",
            Text::RenderConfirmBack => "render.confirm.back",
            Text::RenderRunning => "render.running",
            Text::RenderStopped => "render.stopped",
            Text::RenderCancelled => "render.cancelled",
            Text::RenderResume => "render.resume",
            Text::StageRenderReady => "stage.note.render_ready",
            Text::StageRendered => "stage.note.rendered",
            Text::StageRenderOutdated => "stage.note.render_outdated",
            Text::StageRendering => "stage.note.rendering",
            Text::StageRenderStopped => "stage.note.render_stopped",
            Text::ExportNoAccounts => "export.error.no_accounts",
            Text::ExportNothingChosen => "export.error.nothing_chosen",
            Text::ExportBlocked => "export.error.blocked",
            Text::ExportAlreadyRunning => "export.error.already_running",
            Text::ExportWhileRendering => "export.error.while_rendering",
            Text::ExportNotLoaded => "export.error.not_loaded",
            Text::MetadataMissingKey => "metadata.error.missing_key",
            Text::MetadataBusy => "metadata.error.busy",
            Text::MetadataMissing => "metadata.error.missing",
            Text::MetadataNotSaved => "metadata.error.not_saved",
            Text::ExportFileVideo => "export.file.video",
            Text::ExportFileTitle => "export.file.title",
            Text::ExportFileDescription => "export.file.description",
            Text::ExportFileCaption => "export.file.caption",
            Text::ExportFileTags => "export.file.tags",
            Text::ExportFileVisibility => "export.file.visibility",
            Text::DisclosureReminder => "disclosure.reminder",
            Text::DisclosureNotice => "disclosure.notice",
            Text::StageExportReady => "stage.note.export_ready",
            Text::StageExported => "stage.note.exported",
            Text::StageExportOutdated => "stage.note.export_outdated",
            Text::StageExporting => "stage.note.exporting",
            Text::StageExportStopped => "stage.note.export_stopped",
            Text::PersonaRealisticVoice => "persona.realistic_voice",
            Text::PersonaRealisticVoiceHint => "persona.realistic_voice_hint",
            Text::LookTitle => "persona.look.title",
            Text::LookHint => "persona.look.hint",
            Text::LookSaveFirst => "persona.look.save_first",
            Text::LookEmpty => "persona.look.empty",
            Text::LookDrawnTag => "persona.look.drawn_tag",
            Text::LookImportedTag => "persona.look.imported_tag",
            Text::LookFromDescription => "persona.look.from_description",
            Text::LookRemovePhotoConfirm => "persona.look.remove_photo_confirm",
            Text::LookNotLoaded => "persona.look.not_loaded",
            Text::LookDescription => "persona.look.description",
            Text::LookDescriptionPlaceholder => "persona.look.description_placeholder",
            Text::LookDraw => "persona.look.draw",
            Text::LookDrawAgain => "persona.look.draw_again",
            Text::LookDrawHint => "persona.look.draw_hint",
            Text::LookDrawing => "persona.look.drawing",
            Text::LookDeclined => "persona.look.declined",
            Text::LookFailed => "persona.look.failed",
            Text::LookStopped => "persona.look.stopped",
            Text::LookPendingTitle => "persona.look.pending_title",
            Text::LookPendingHint => "persona.look.pending_hint",
            Text::LookKeep => "persona.look.keep",
            Text::LookDiscard => "persona.look.discard",
            Text::LookImport => "persona.look.import",
            Text::LookImportTitle => "persona.look.import_title",
            Text::LookImportHint => "persona.look.import_hint",
            Text::LookShowsMe => "persona.look.shows_me",
            Text::LookChoosePhoto => "persona.look.choose_photo",
            Text::LookImportDialog => "persona.look.import_dialog",
            Text::LookCancel => "persona.look.cancel",
            Text::LookImported => "persona.look.imported",
            Text::LookKept => "persona.look.kept",
            Text::LookRemove => "persona.look.remove",
            Text::LookRemoveConfirm => "persona.look.remove_confirm",
            Text::LookRemoveYes => "persona.look.remove_yes",
            Text::LookRemoved => "persona.look.removed",
            Text::LookRealistic => "persona.look.realistic",
            Text::LookRealisticHint => "persona.look.realistic_hint",
            Text::PersonaExportedNoLook => "persona.exported_no_look",
            Text::LookMissingClaudeKey => "persona.look.error.missing_claude_key",
            Text::LookMissingImageKey => "persona.look.error.missing_image_key",
            Text::LookBusy => "persona.look.error.busy",
            Text::LookNothingWaiting => "persona.look.error.nothing_waiting",
            Text::LookNone => "persona.look.error.none",
            Text::LookNotConfirmed => "persona.look.error.not_confirmed",
            Text::LookUnreadable => "persona.look.error.unreadable",
            Text::LookImageRefused => "persona.look.error.image_refused",
            Text::LookNotSaved => "persona.look.error.not_saved",
            Text::LookAssetTitle => "persona.look.asset.title",
            Text::LookAssetHint => "persona.look.asset.hint",
            Text::LookAssetKindLabel => "persona.look.asset.kind",
            Text::LookAssetKindName(kind) => {
                return format!("persona.look.asset.kind_name.{}", kind.code()).into();
            }
            Text::LookAssetIdLabel => "persona.look.asset.id",
            Text::LookAssetIdPlaceholder => "persona.look.asset.id_placeholder",
            Text::LookAssetSave => "persona.look.asset.save",
            Text::LookAssetRemove => "persona.look.asset.remove",
            Text::LookAssetHeld => "persona.look.asset.held",
            Text::LookAssetSaved => "persona.look.asset.saved",
            Text::LookAssetRemoved => "persona.look.asset.removed",
            Text::LookAssetError(error) => match error {
                LookAssetError::Empty => "persona.look.asset.error.empty",
                LookAssetError::Invalid => "persona.look.asset.error.invalid",
                LookAssetError::KindNotForLook => "persona.look.asset.error.kind_not_for_look",
            },
            Text::LookDescriptionError(error) => match error {
                LookDescriptionError::Required => "persona.look.error.description_required",
                LookDescriptionError::TooLong => "persona.look.error.description_too_long",
            },
            Text::LookImageReason(error) => match error {
                LookImageError::NotAnImage => "persona.look.image.not_an_image",
                LookImageError::TooSmall { .. } => "persona.look.image.too_small",
                LookImageError::TooLarge { .. } => "persona.look.image.too_large",
                LookImageError::Shape { .. } => "persona.look.image.shape",
                LookImageError::FileTooLarge(_) => "persona.look.image.file_too_large",
            },
            Text::ExportInfo => "export.info",
            Text::ExportFigureRendered => "export.figure.rendered",
            Text::ExportFigureMetadata => "export.figure.metadata",
            Text::ExportFigureExported => "export.figure.exported",
            Text::ExportFigureCost => "export.figure.cost",
            Text::ExportFigureNetworks => "export.figure.networks",
            Text::ExportChosen => "export.chosen",
            Text::ExportTargets => "export.targets",
            Text::ExportInclude => "export.include",
            Text::ExportStart => "export.start",
            Text::ExportRunning => "export.running",
            Text::ExportStopped => "export.stopped",
            Text::ExportCancelled => "export.cancelled",
            Text::ExportShowFolder => "export.show_folder",
            Text::ExportColumnRender => "export.column_render",
            Text::ExportColumnLast => "export.column_last",
            Text::ExportRenderOutdatedHint => "export.render_outdated_hint",
            Text::ExportNoRenderHint => "export.no_render_hint",
            Text::ExportStateReady => "export.state.ready",
            Text::ExportStateNoRender => "export.state.no_render",
            Text::ExportStateNoMetadata => "export.state.no_metadata",
            Text::ExportStateProblems => "export.state.problems",
            Text::ExportLastCurrent => "export.last.current",
            Text::ExportLastOutdated => "export.last.outdated",
            Text::ExportLastNone => "export.last.none",
            Text::ExportLastOutdatedHint => "export.last.outdated_hint",
            Text::MetadataGenerate => "metadata.generate",
            Text::MetadataRegenerate => "metadata.regenerate",
            Text::MetadataGenerateHint => "metadata.generate_hint",
            Text::MetadataRegenerateHint => "metadata.regenerate_hint",
            Text::MetadataRunning => "metadata.running",
            Text::MetadataStopped => "metadata.stopped",
            Text::MetadataConfirmTitle => "metadata.confirm_title",
            Text::MetadataConfirmBody => "metadata.confirm_body",
            Text::MetadataNone => "metadata.none",
            Text::MetadataEmpty => "metadata.empty",
            Text::MetadataStateNone => "metadata.state_none",
            Text::MetadataStateGenerated => "metadata.state_generated",
            Text::MetadataStateEdited => "metadata.state_edited",
            Text::MetadataEdited => "metadata.edited",
            Text::MetadataFieldTitle => "metadata.title",
            Text::MetadataFieldDescription => "metadata.description",
            Text::MetadataFieldCaption => "metadata.caption",
            Text::MetadataFieldTags => "metadata.tags",
            Text::MetadataTagsPlaceholder => "metadata.tags_placeholder",
            Text::MetadataCounter => "metadata.counter",
            Text::MetadataTagsCount => "metadata.tags_count",
            Text::MetadataTagsCountOf => "metadata.tags_count_of",
            Text::MetadataFooterNote => "metadata.footer_note",
            Text::MetadataHashtagsNote => "metadata.hashtags_note",
            Text::MetadataSave => "metadata.save",
            Text::MetadataRevert => "metadata.revert",
            Text::MetadataSaved => "metadata.saved",
            Text::MetadataCopy => "metadata.copy",
            Text::MetadataCopied => "metadata.copied",
            Text::MetadataPreview => "metadata.preview",
            Text::MetadataCostUnpriced => "metadata.cost_unpriced",
            Text::DisclosureHow(network) => {
                return format!("disclosure.how.{}", network.code()).into();
            }
            Text::PublicationNoAccount => "publication.error.no_account",
            Text::PublicationNotExported => "publication.error.not_exported",
            Text::PublicationAlreadyLinked => "publication.error.already_linked",
            Text::PublicationNotFound => "publication.error.not_found",
            Text::PublicationNotSaved => "publication.error.not_saved",
            Text::MetricsMissingKey => "metrics.error.missing_key",
            Text::MetricsNothingToSync => "metrics.error.nothing_to_sync",
            Text::MetricsAlreadySyncing => "metrics.error.already_syncing",
            Text::MetricsNotLoaded => "metrics.error.not_loaded",
            Text::PublicationTitle => "publication.title",
            Text::PublicationNeedsExport => "publication.needs_export",
            Text::PublicationLinkPlaceholder => "publication.link_placeholder",
            Text::PublicationMark => "publication.mark",
            Text::PublicationMarkHint => "publication.mark_hint",
            Text::PublicationPosted => "publication.posted",
            Text::PublicationMissing => "publication.missing",
            Text::PublicationMissingHint => "publication.missing_hint",
            Text::PublicationOpen => "publication.open",
            Text::PublicationChange => "publication.change",
            Text::PublicationSave => "publication.save",
            Text::PublicationCancel => "publication.cancel",
            Text::PublicationRemove => "publication.remove",
            Text::PublicationRemoveConfirm => "publication.remove_confirm",
            Text::PublicationRemoveKeep => "publication.remove_keep",
            Text::PublicationSaved => "publication.saved",
            Text::PublicationRemoved => "publication.removed",
            Text::PublicationPostedAt => "publication.posted_at",
            Text::PublicationNoMetrics => "publication.no_metrics",
            Text::PublicationConnectForMetrics => "publication.connect_for_metrics",
            Text::PublicationReconnectForMetrics => "publication.reconnect_for_metrics",
            Text::PublicationFigure => "publication.figure",
            Text::PublicationTileViews => "publication.tile_views",
            Text::PublicationTileEngaged => "publication.tile_engaged",
            Text::MetricViews => "metrics.views",
            Text::MetricLikes => "metrics.likes",
            Text::MetricComments => "metrics.comments",
            Text::MetricHidden => "metrics.hidden",
            Text::MetricHiddenHint => "metrics.hidden_hint",
            Text::MetricsSyncedAgo => "metrics.synced_ago",
            Text::MetricsNotSynced => "metrics.not_synced",
            Text::MetricsSyncNow => "metrics.sync_now",
            Text::MetricsSyncing => "metrics.syncing",
            Text::MetricsSyncStopped => "metrics.sync_stopped",
            Text::MetricsSyncHint => "metrics.sync_hint",
            Text::MetricsHistory => "metrics.history",
            Text::MetricsChange => "metrics.change",
            Text::MetricsViewsChange => "metrics.views_change",
            Text::MetricEngagedViews => "metrics.engaged_views",
            Text::MetricEngagedViewsHint => "metrics.engaged_views_hint",
            Text::MetricWatchTime => "metrics.watch_time",
            Text::MetricAverageView => "metrics.average_view",
            Text::MetricAverageViewed => "metrics.average_viewed",
            Text::MetricRevenue => "metrics.revenue",
            Text::MetricRpm => "metrics.rpm",
            Text::MetricRpmHint => "metrics.rpm_hint",
            Text::MetricCpm => "metrics.cpm",
            Text::MetricCpmHint => "metrics.cpm_hint",
            Text::MetricPlaybackCpm => "metrics.playback_cpm",
            Text::MetricPlaybackCpmHint => "metrics.playback_cpm_hint",
            Text::MetricNotMonetized => "metrics.not_monetized",
            Text::MetricNotMonetizedHint => "metrics.not_monetized_hint",
            Text::MetricsOwnerLine => "metrics.owner_line",
            Text::MetricsRetention => "metrics.retention",
            Text::MetricsRetentionHint => "metrics.retention_hint",
            Text::MetricsRetentionStart => "metrics.retention_start",
            Text::MetricsRetentionEnd => "metrics.retention_end",
            Text::RetentionTitle => "retention.title",
            Text::RetentionNone => "retention.none",
            Text::RetentionUnavailable => "retention.unavailable",
            Text::RetentionRead => "retention.read",
            Text::RetentionDrops => "retention.drops",
            Text::RetentionDropsHint => "retention.drops_hint",
            Text::RetentionNoDrops => "retention.no_drops",
            Text::RetentionChanged => "retention.changed",
            Text::RetentionNoLength => "retention.no_length",
            Text::RetentionLost => "retention.lost",
            Text::RetentionAtShare => "retention.at_share",
            Text::RetentionScene => "retention.scene",
            Text::RetentionFootage => "retention.footage",
            Text::RetentionNoPicture => "retention.no_picture",
            Text::RetentionNoSpeech => "retention.no_speech",
            Text::RetentionShowInEditor => "retention.show_in_editor",
            Text::MetricsHours => "metrics.hours",
            Text::MetricsMinutes => "metrics.minutes",
            Text::MetricsPercent => "metrics.percent",
            Text::MetricShares => "metrics.shares",
            Text::MetricSaves => "metrics.saves",
            Text::MetricReach => "metrics.reach",
            Text::MetricReachHint => "metrics.reach_hint",
            Text::MetricInteractions => "metrics.interactions",
            Text::MetricInteractionsHint => "metrics.interactions_hint",
            Text::MetricAverageWatch => "metrics.average_watch",
            Text::MetricNotReportedHint => "metrics.not_reported_hint",
            Text::MetricsInstagramLine => "metrics.instagram_line",
            Text::MetricsTikTokLine => "metrics.tiktok_line",
            Text::MetricsInsightsEmpty => "metrics.insights_empty",
            Text::MetricsInsightsPending => "metrics.insights_pending",
            Text::PerformanceOwnerNotConnected => "performance.owner_not_connected",
            Text::PerformanceOwnerReconnect => "performance.owner_reconnect",
            Text::PerformanceTileEngaged => "performance.tile_engaged",
            Text::PerformanceInfo => "performance.info",
            Text::PerformanceEmpty => "performance.empty",
            Text::PerformanceNoChannels => "performance.no_channels",
            Text::PerformanceHistoryTitle => "performance.history_title",
            Text::PerformanceHistoryEmpty => "performance.history_empty",
            Text::PerformancePosts => "performance.posts",
            Text::PerformanceTracked => "performance.tracked",
            Text::PerformanceTile => "performance.tile",
            Text::DecisionEngineLabel => "decision_engine.label",
            Text::DecisionEngineHint => "decision_engine.hint",
            Text::DecisionEngineCaption => "decision_engine.caption",
            Text::DecisionEngineNotSaved => "decision_engine.not_saved",
            Text::DecisionEngineName(engine) => {
                return format!("decision_engine.name.{}", engine.code()).into();
            }
            Text::MetricsSettingsTab => "metrics.settings.tab",
            Text::MetricsSettingLabel => "metrics.settings.label",
            Text::MetricsSettingHint => "metrics.settings.hint",
            Text::MetricsSettingNotSaved => "metrics.settings.not_saved",
            Text::PostLinkProblem(error) => match error {
                PostLinkError::Empty => "publication.link.empty",
                PostLinkError::NotALink => "publication.link.not_a_link",
                PostLinkError::OtherSite(Some(_)) => "publication.link.other_network",
                PostLinkError::OtherSite(None) => "publication.link.other_site",
                PostLinkError::ShortLink => "publication.link.short_link",
                PostLinkError::NotAPost => "publication.link.not_a_post",
            },
            Text::MetricsSyncOption(setting) => {
                return format!("metrics.sync_option.{}", setting.code()).into();
            }
            Text::UploadChanged => "upload.error.changed",
            Text::UploadReplaceNotConfirmed => "upload.error.replace_not_confirmed",
            Text::UploadNotStarted => "upload.error.not_started",
            Text::PublicationReplacesUpload => "publication.error.replaces_upload",
            Text::PublicationUploading => "publication.error.uploading",
            Text::UploadTitle => "upload.title",
            Text::UploadHint => "upload.hint",
            Text::UploadOpenReview => "upload.open_review",
            Text::UploadReviewTitle => "upload.review_title",
            Text::UploadFieldFile => "upload.field.file",
            Text::UploadFieldChannel => "upload.field.channel",
            Text::UploadFieldVisibility => "upload.field.visibility",
            Text::UploadMadeForKids => "upload.made_for_kids",
            Text::UploadMadeForKidsHint => "upload.made_for_kids_hint",
            Text::UploadSynthetic => "upload.synthetic",
            Text::UploadSyntheticHint => "upload.synthetic_hint",
            Text::UploadSyntheticOn => "upload.synthetic_on",
            Text::UploadReplacePost => "upload.replace_post",
            Text::UploadReplaceUpload => "upload.replace_upload",
            Text::UploadIrreversible => "upload.irreversible",
            Text::UploadStart => "upload.start",
            Text::UploadBack => "upload.back",
            Text::UploadQueued => "upload.queued",
            Text::UploadStateWaiting => "upload.state.waiting",
            Text::UploadStateUploading => "upload.state.uploading",
            Text::UploadStateRetrying => "upload.state.retrying",
            Text::UploadStateProcessing => "upload.state.processing",
            Text::UploadStateStillProcessing => "upload.state.still_processing",
            Text::UploadStatePublished => "upload.state.published",
            Text::UploadStateRestricted => "upload.state.restricted",
            Text::UploadStateStopped => "upload.state.stopped",
            Text::UploadStateFailed => "upload.state.failed",
            Text::UploadRetryingHint => "upload.retrying_hint",
            Text::UploadProcessingHint => "upload.processing_hint",
            Text::UploadStillProcessingHint => "upload.still_processing_hint",
            Text::UploadStoppedHint => "upload.stopped_hint",
            Text::UploadRestrictedHint => "upload.restricted_hint",
            Text::UploadRestrictedCauses => "upload.restricted_causes",
            Text::UploadFailureQuota => "upload.failure.quota",
            Text::UploadFailureCredits => "upload.failure.credits",
            Text::UploadFailureAnswerLost => "upload.failure.answer_lost",
            Text::UploadFailureUploadLimit => "upload.failure.upload_limit",
            Text::UploadFailureReconnect => "upload.failure.reconnect",
            Text::UploadFailureRejected => "upload.failure.rejected",
            Text::UploadFailureProcessing => "upload.failure.processing",
            Text::UploadFailureRemoved => "upload.failure.removed",
            Text::UploadFailureAccountChanged => "upload.failure.account_changed",
            Text::UploadFailureRenderChanged => "upload.failure.render_changed",
            Text::UploadStop => "upload.stop",
            Text::UploadResume => "upload.resume",
            Text::UploadRetry => "upload.retry",
            Text::UploadCheckAgain => "upload.check_again",
            Text::UploadSentAt => "upload.sent_at",
            Text::PublicationReplaceUploadConfirm => "publication.replace_upload_confirm",
            Text::PublicationReplaceUploadYes => "publication.replace_upload_yes",
            Text::UploadBlocked(block) => return format!("upload.block.{}", block.code()).into(),
            Text::UploadFieldWhen => "upload.field.when",
            Text::UploadWhenNow => "upload.when.now",
            Text::UploadWhenSchedule => "upload.when.schedule",
            Text::UploadStartScheduled => "upload.start_scheduled",
            Text::UploadStateScheduled => "upload.state.scheduled",
            Text::UploadScheduledAt => "upload.scheduled_at",
            Text::UploadOverLimitAt => "upload.over_limit_at",
            Text::UploadOverLimitRetryAt => "upload.over_limit_retry_at",
            Text::UploadScheduledHint => "upload.scheduled_hint",
            Text::UploadRestrictedScheduledHint => "upload.restricted_scheduled_hint",
            Text::UploadFailureScheduleMissed => "upload.failure.schedule_missed",
            Text::UploadReelHint => "upload.reel_hint",
            Text::UploadPostHint => "upload.post_hint",
            Text::UploadFieldAccount => "upload.field.account",
            Text::UploadFieldCaption => "upload.field.caption",
            Text::UploadFieldCover => "upload.field.cover",
            Text::UploadCoverHint => "upload.cover_hint",
            Text::UploadCoverInvalid => "upload.cover_invalid",
            Text::UploadCoverPastEnd => "upload.error.cover_past_end",
            Text::UploadShareToFeed => "upload.share_to_feed",
            Text::UploadShareToFeedHint => "upload.share_to_feed_hint",
            Text::UploadAiLabel => "upload.ai_label",
            Text::UploadAiLabelHint => "upload.ai_label_hint",
            Text::UploadPostAiLabel => "upload.post_ai_label",
            Text::UploadPostAiLabelHint => "upload.post_ai_label_hint",
            Text::UploadReelIrreversible => "upload.reel_irreversible",
            Text::UploadPostIrreversible => "upload.post_irreversible",
            Text::UploadReelStart => "upload.reel_start",
            Text::UploadPostStart => "upload.post_start",
            Text::UploadPostCost => "upload.post_cost",
            Text::UploadPostCostHint => "upload.post_cost_hint",
            Text::UploadReelProcessingHint => "upload.reel_processing_hint",
            Text::UploadPostProcessingHint => "upload.post_processing_hint",
            Text::UploadStateOverLimit => "upload.state.over_limit",
            Text::UploadOverLimitHint => "upload.over_limit_hint",
            Text::UploadOverLimitSoonHint => "upload.over_limit_soon_hint",
            Text::UploadOverLimitRecheckHint => "upload.over_limit_recheck_hint",
            Text::UploadIssue => "upload.issue",
            Text::UploadSpecsTitle => "upload.specs_title",
            Text::UploadSpec(code) => return format!("upload.spec.{code}").into(),
            Text::ScheduleDate => "schedule.date",
            Text::ScheduleTime => "schedule.time",
            Text::ScheduleDatePlaceholder => "schedule.date_placeholder",
            Text::ScheduleTimePlaceholder => "schedule.time_placeholder",
            Text::ScheduleZone => "schedule.zone",
            Text::ScheduleHint => "schedule.hint",
            Text::ScheduleChange => "schedule.change",
            Text::ScheduleCancel => "schedule.cancel",
            Text::ScheduleSave => "schedule.save",
            Text::ScheduleCancelConfirm => "schedule.cancel_confirm",
            Text::ScheduleCancelYes => "schedule.cancel_yes",
            Text::ScheduleKeep => "schedule.keep",
            Text::ScheduleWorking => "schedule.working",
            Text::ScheduleChanged => "schedule.changed",
            Text::ScheduleCancelled => "schedule.cancelled",
            Text::ScheduleAlreadyLive => "schedule.already_live",
            Text::ScheduleNotScheduled => "schedule.not_scheduled",
            Text::ScheduleFailed => "schedule.failed",
            Text::ScheduleNotAllowed => "schedule.not_allowed",
            Text::JobWaitsUntil => "job.waits_until",
            Text::UploadStateDue => "upload.state.due",
            Text::UploadStateMissed => "upload.state.missed",
            Text::UploadDueAt => "upload.due_at",
            Text::UploadDueHint => "upload.due_hint",
            Text::UploadMissedAt => "upload.missed_at",
            Text::UploadMissedHint => "upload.missed_hint",
            Text::UploadReelScheduleHint => "upload.reel_schedule_hint",
            Text::UploadDueHintAgent => "upload.due_hint_agent",
            Text::UploadReelScheduleHintAgent => "upload.reel_schedule_hint_agent",
            Text::UploadMissedHintAgent => "upload.missed_hint_agent",
            Text::JobRunsElsewhere => "job.error.runs_elsewhere",
            Text::JobSuperseded => "job.error.superseded",
            Text::JobRestyled => "job.error.restyled",
            Text::JobScenesMovedOn => "job.error.scenes_moved_on",
            Text::JobScenesTaken => "job.error.scenes_taken",
            Text::JobTakeFromItsPage => "job.error.take_from_page",
            Text::JobNarrationBusy => "job.error.narration_busy",
            Text::JobScenesBusy => "job.error.scenes_busy",
            Text::JobSameWork => "job.error.same_work",
            Text::JobAsksFirst => "job.error.asks_first",
            Text::JobOverBudget => "job.error.over_budget",
            Text::SettingsPublishingTab => "settings.publishing_tab",
            Text::AgentTitle => "agent.title",
            Text::AgentQuestion => "agent.question",
            Text::AgentExplanation => "agent.explanation",
            Text::AgentSwitch => "agent.switch",
            Text::AgentSends => "agent.sends",
            Text::AgentLimits => "agent.limits",
            Text::AgentHint => "agent.hint",
            Text::AgentStatusOff => "agent.status.off",
            Text::AgentStatusRunning => "agent.status.running",
            Text::AgentStatusNotRunning => "agent.status.not_running",
            Text::AgentStartNow => "agent.start_now",
            Text::AgentWorking => "agent.working",
            Text::AgentNotSetUp => "agent.error.not_set_up",
            Text::AgentNotRemoved => "agent.error.not_removed",
            Text::AgentNotSaved => "agent.error.not_saved",
            Text::SettingsAboutTab => "settings.about_tab",
            Text::AboutTitle => "about.title",
            Text::AboutVersion => "about.version",
            Text::AboutSummary => "about.summary",
            Text::AboutFfmpegTitle => "about.ffmpeg_title",
            Text::AboutFfmpeg => "about.ffmpeg",
            Text::AboutFfmpegSource => "about.ffmpeg_source",
            Text::AboutFfmpegLicense => "about.ffmpeg_license",
            Text::AboutNoticesTitle => "about.notices_title",
            Text::AboutNotices => "about.notices",
            Text::AboutOpenNotices => "about.open_notices",
            Text::AboutNoNotices => "about.no_notices",
            Text::AboutDiagnosticsTitle => "about.diagnostics_title",
            Text::AboutDiagnostics => "about.diagnostics",
            Text::AboutExportDiagnostics => "about.export_diagnostics",
            Text::AboutExportingDiagnostics => "about.exporting_diagnostics",
            Text::AboutOpenLogs => "about.open_logs",
            Text::AboutDiagnosticsSaved => "about.diagnostics_saved",
            Text::AboutShowDiagnostics => "about.show_diagnostics",
            Text::AboutDiagnosticsFailed => "about.diagnostics_failed",
            Text::PrivacyNotAccepted => "privacy.not_accepted",
            Text::PrivacyTitle => "privacy.title",
            Text::PrivacySummary => "privacy.summary",
            Text::PrivacyAcceptedOn => "privacy.accepted_on",
            Text::PrivacyNotYet => "privacy.not_yet",
            Text::PrivacyChanged => "privacy.changed",
            Text::PrivacyRead => "privacy.read",
            Text::PrivacyOnline => "privacy.online",
            Text::PrivacyAccept => "privacy.accept",
            Text::PrivacyConsent => "privacy.consent",
            Text::PrivacyUsesYouTube => "privacy.uses_youtube",
            Text::YouTubeTerms => "privacy.youtube_terms",
            Text::GooglePrivacyPolicy => "privacy.google_privacy",
            Text::GooglePermissions => "privacy.google_permissions",
            Text::DeleteYouTubeData => "privacy.delete",
            Text::DeleteYouTubeDataConfirm => "privacy.delete_confirm",
            Text::DeleteYouTubeDataNow => "privacy.delete_now",
            Text::DeleteYouTubeDataCancel => "privacy.delete_cancel",
            Text::YouTubeDataDeleted => "privacy.deleted",
            Text::YouTubeDataNotDeleted => "privacy.not_deleted",
            Text::PrivacyBanner => "privacy.banner",
            Text::PrivacyBannerReview => "privacy.banner_review",
            Text::UpdatesTitle => "update.title",
            Text::UpdatesSummary => "update.summary",
            Text::UpdatesSwitch => "update.switch",
            Text::UpdatesCheckNow => "update.check_now",
            Text::UpdatesChecking => "update.checking",
            Text::UpdatesNever => "update.never",
            Text::UpdatesUpToDate => "update.up_to_date",
            Text::UpdatesNoRelease => "update.no_release",
            Text::UpdatesFoundNewer => "update.found_newer",
            Text::UpdatesOffline => "update.offline",
            Text::UpdatesRateLimited => "update.rate_limited",
            Text::UpdatesRateLimitedLater => "update.rate_limited_later",
            Text::UpdatesFailed => "update.failed",
            Text::UpdatesStore => "update.store",
            Text::UpdatesSource => "update.source",
            Text::UpdateAvailable => "update.available",
            Text::UpdatePublished => "update.published",
            Text::UpdateWhatsNew => "update.whats_new",
            Text::UpdateReleasePage => "update.release_page",
            Text::UpdateDownload => "update.download",
            Text::UpdateDownloading => "update.downloading",
            Text::UpdateCancel => "update.cancel",
            Text::UpdateDownloadFailed => "update.download_failed",
            Text::UpdateDownloadAgain => "update.download_again",
            Text::UpdateReady => "update.ready",
            Text::UpdateInstall => "update.install",
            Text::UpdateReviewTitle => "update.review_title",
            Text::UpdateReviewBody => "update.review_body",
            Text::UpdateReviewAgent => "update.review_agent",
            Text::UpdateInstallAndRestart => "update.install_and_restart",
            Text::UpdateStarting => "update.starting",
            Text::UpdateNoInstaller => "update.no_installer",
            Text::UpdateAlreadyDownloading => "update.already_downloading",
            Text::UpdateFileGone => "update.file_gone",
            Text::UpdateChecksumChanged => "update.checksum_changed",
            Text::UpdateNotStarted => "update.not_started",
            Text::UpdateWaitsForJobs => "update.waits_for_jobs",
            Text::UpdateWaitsForRestore => "update.waits_for_restore",
            Text::UpdateOtherWindow => "update.other_window",
            Text::UpdateBannerSee => "update.banner_see",
            Text::UpdateBannerLater => "update.banner_later",
            Text::UpdateNotesOpenAbout => "update.notes_open_about",
            Text::UpdateNotesClose => "update.notes_close",
            Text::UpdateDownloadUnreachable => "update.download_unreachable",
            Text::UpdateWaitingToRetry => "update.waiting_to_retry",
            Text::UpdateSwitchNotSaved => "update.switch_not_saved",
            Text::UpdateNoneAvailable => "update.none_available",
            Text::SettingsBackupTab => "settings.backup_tab",
            Text::BackupTitle => "backup.title",
            Text::BackupSummary => "backup.summary",
            Text::BackupIncludeMedia => "backup.include_media",
            Text::BackupMediaHint => "backup.media_hint",
            Text::BackupMediaSize => "backup.media_size",
            Text::BackupMediaMeasuring => "backup.media_measuring",
            Text::BackupMediaNone => "backup.media_none",
            Text::BackupMediaUnknown => "backup.media_unknown",
            Text::BackupBackUp => "backup.back_up",
            Text::BackupBackingUp => "backup.backing_up",
            Text::BackupSaved => "backup.saved",
            Text::BackupShow => "backup.show",
            Text::BackupFailed => "backup.failed",
            Text::BackupRunning => "backup.running",
            Text::BackupCancel => "backup.cancel",
            Text::BackupAutomaticTitle => "backup.automatic_title",
            Text::BackupAutomaticSwitch => "backup.automatic_switch",
            Text::BackupAutomaticHint => "backup.automatic_hint",
            Text::BackupAutomaticNone => "backup.automatic_none",
            Text::BackupAutomaticRow => "backup.automatic_row",
            Text::BackupOpenFolder => "backup.open_folder",
            Text::BackupRestoreTitle => "backup.restore_title",
            Text::BackupRestoreSummary => "backup.restore_summary",
            Text::SettingsFileNotOne => "settings_file.not_one",
            Text::SettingsFileNewer => "settings_file.newer",
            Text::SettingsFileWrongPassword => "settings_file.wrong_password",
            Text::SettingsFileNotSaved => "settings_file.not_saved",
            Text::SettingsFilePasswordShort => "settings_file.password_short",
            Text::SettingsFilePasswordsDiffer => "settings_file.passwords_differ",
            Text::SettingsFileNothingTicked => "settings_file.nothing_ticked",
            Text::SettingsFileUnreadable => "settings_file.unreadable",
            Text::SettingsFilePreferences => "settings_file.preferences",
            Text::SettingsFileCosts => "settings_file.costs",
            Text::SettingsFileSignInLabel => "settings_file.sign_in_label",
            Text::SettingsFileItemFailed => "settings_file.item_failed",
            Text::SettingsFileSomeNotRead => "settings_file.some_not_read",
            Text::SettingsFileItemNotRead => "settings_file.item_not_read",
            Text::SettingsFileTitle => "settings_file.title",
            Text::SettingsFileSummary => "settings_file.summary",
            Text::SettingsFileExport => "settings_file.export",
            Text::SettingsFileImport => "settings_file.import",
            Text::SettingsFileExportTitle => "settings_file.export_title",
            Text::SettingsFileGroupKeys => "settings_file.group_keys",
            Text::SettingsFileGroupApps => "settings_file.group_apps",
            Text::SettingsFileGroupPreferences => "settings_file.group_preferences",
            Text::SettingsFileGroupPersonas => "settings_file.group_personas",
            Text::SettingsFileGroupChannels => "settings_file.group_channels",
            Text::SettingsFileGroupSignIns => "settings_file.group_sign_ins",
            Text::SettingsFileSignInsHint => "settings_file.sign_ins_hint",
            Text::SettingsFilePassword => "settings_file.password",
            Text::SettingsFilePasswordAgain => "settings_file.password_again",
            Text::SettingsFilePasswordHint => "settings_file.password_hint",
            Text::SettingsFileSave => "settings_file.save",
            Text::SettingsFileSaved => "settings_file.saved",
            Text::SettingsFileMade => "settings_file.made",
            Text::SettingsFileOpen => "settings_file.open",
            Text::SettingsFileImportTitle => "settings_file.import_title",
            Text::SettingsFileAdds => "settings_file.adds",
            Text::SettingsFileReplaces => "settings_file.replaces",
            Text::SettingsFileModelNotOffered => "settings_file.model_not_offered",
            Text::SettingsFilePartsNotRead => "settings_file.parts_not_read",
            Text::SettingsFileReplaceNote => "settings_file.replace_note",
            Text::SettingsFileBringIn => "settings_file.bring_in",
            Text::SettingsFileBrought => "settings_file.brought",
            Text::SettingsFileNotBrought => "settings_file.not_brought",
            Text::SettingsOfferFirst => "settings_offer.first",
            Text::SettingsOfferFile => "settings_offer.file",
            Text::SettingsOfferOpen => "settings_offer.open",
            Text::SettingsOfferNotNow => "settings_offer.not_now",
            Text::DevCopyChip => "about.dev_chip",
            Text::AboutDevCopy => "about.dev_copy",
            Text::BackupRestoreFromFile => "backup.restore_from_file",
            Text::BackupRestore => "backup.restore",
            Text::BackupChecking => "backup.checking",
            Text::BackupReviewTitle => "backup.review_title",
            Text::BackupReviewMade => "backup.review_made",
            Text::BackupReviewWithMedia => "backup.review_with_media",
            Text::BackupReviewWithoutMedia => "backup.review_without_media",
            Text::BackupReviewWarning => "backup.review_warning",
            Text::BackupReviewPosts => "backup.review_posts",
            Text::BackupRestoreAndRestart => "backup.restore_and_restart",
            Text::BackupRestoring => "backup.restoring",
            Text::BackupWaitingForAgent => "backup.waiting_for_agent",
            Text::BackupRestarting => "backup.restarting",
            Text::BackupRestoreFailed => "backup.restore_failed",
            Text::RestoreWaitsForJobs => "backup.restore_waits_for_jobs",
            Text::RestoreOtherWindow => "backup.restore_other_window",
            Text::BackupKeptAside => "backup.kept_aside",
            Text::RestoreRunning => "backup.restore_running",
            Text::BackupNotABackup => "backup.not_a_backup",
            Text::BackupDamaged => "backup.damaged",
            Text::BackupNewer => "backup.newer",
            Text::BackupUnreadable => "backup.unreadable",
            Text::BackupRestoredTitle => "backup.restored_title",
            Text::BackupRestored => "backup.restored",
            Text::BackupRolledBack => "backup.rolled_back",
            Text::BackupSetUpAgain => "backup.set_up_again",
            Text::BackupSetUpKey => "backup.set_up_key",
            Text::BackupSetUpPlan => "backup.set_up_plan",
            Text::BackupSetUpCredentials => "backup.set_up_credentials",
            Text::BackupSetUpReconnect => "backup.set_up_reconnect",
            Text::BackupSetUpOpen => "backup.set_up_open",
            Text::BackupNothingToSetUp => "backup.nothing_to_set_up",
            Text::BackupDone => "backup.done",
            Text::UploadReelStartScheduled => "upload.reel_start_scheduled",
            Text::UploadTikTokSpec(code) => return format!("upload.tiktok_spec.{code}").into(),
            Text::UploadTikTokSpecsTitle => "upload.tiktok_specs_title",
            Text::UploadDraftHint => "upload.draft_hint",
            Text::UploadDraftNotice => "upload.draft_notice",
            Text::UploadFieldDraftCaption => "upload.field.draft_caption",
            Text::UploadFieldPostText => "upload.field.post_text",
            Text::UploadDraftAiLabel => "upload.draft_ai_label",
            Text::UploadDraftAiLabelHint => "upload.draft_ai_label_hint",
            Text::UploadDraftIrreversible => "upload.draft_irreversible",
            Text::UploadDraftStart => "upload.draft_start",
            Text::UploadDraftProcessingHint => "upload.draft_processing_hint",
            Text::UploadStateDraftSent => "upload.state.draft_sent",
            Text::UploadDraftSentHint => "upload.draft_sent_hint",
            Text::UploadDraftAiReminder => "upload.draft_ai_reminder",
            Text::UploadDraftLinkHint => "upload.draft_link_hint",
            Text::UploadDraftCopy => "upload.draft_copy",
            Text::UploadDraftCopied => "upload.draft_copied",
            Text::UploadDraftLimitHint => "upload.draft_limit_hint",
            Text::UploadDraftLimitHeldHint => "upload.draft_limit_held_hint",
            Text::UploadDraftLimitRecheckHint => "upload.draft_limit_recheck_hint",
            Text::MissedTitle => "missed.title",
            Text::MissedHint => "missed.hint",
            Text::MissedDue => "missed.due",
            Text::MissedSendNow => "missed.send_now",
            Text::MissedNewTime => "missed.new_time",
            Text::MissedSave => "missed.save",
            Text::MissedCancel => "missed.cancel",
            Text::MissedCancelConfirm => "missed.cancel_confirm",
            Text::MissedCancelYes => "missed.cancel_yes",
            Text::MissedKeep => "missed.keep",
            Text::MissedLater => "missed.later",
            Text::MissedLaterHint => "missed.later_hint",
            Text::MissedSent => "missed.sent",
            Text::MissedRescheduled => "missed.rescheduled",
            Text::MissedCancelled => "missed.cancelled",
            Text::MissedNotMissed => "missed.not_missed",
            Text::MissedNotUpdated => "missed.not_updated",
            Text::DateTimeFormat => "date_time.format",
            Text::DateFormat => "date_time.date",
            Text::TimeFormat => "date_time.time",
            Text::TimeAm => "date_time.am",
            Text::TimePm => "date_time.pm",
            Text::DateTimeWithZone => "date_time.with_zone",
            Text::ZoneWithOffset => "date_time.zone",
            Text::WeekdayName(number) => return format!("weekday.{number}").into(),
            Text::ScheduleProblem(problem) => {
                return format!("schedule.problem.{}", problem.code()).into();
            }
            Text::MetadataProblem(problem) => match problem {
                MetadataProblem::TitleRequired => "metadata.problem.title_required",
                MetadataProblem::TitleTooLong => "metadata.problem.title_too_long",
                MetadataProblem::TextRequired => "metadata.problem.text_required",
                MetadataProblem::TextTooLong => "metadata.problem.text_too_long",
                MetadataProblem::TooManyTags => "metadata.problem.too_many_tags",
                MetadataProblem::TagsTooLong => "metadata.problem.tags_too_long",
                MetadataProblem::AngleBrackets => "metadata.problem.angle_brackets",
            },
            Text::StageName(stage) => match stage {
                Stage::Brief => "stage.name.brief",
                Stage::Script => "stage.name.script",
                Stage::Narration => "stage.name.narration",
                Stage::Capture => "stage.name.capture",
                Stage::Scenes => "stage.name.scenes",
                Stage::Clips => "stage.name.clips",
                Stage::Edit => "stage.name.edit",
                Stage::Render => "stage.name.render",
                Stage::Publish => "stage.name.publish",
            },
            Text::StageAfter(stage) => match stage {
                Stage::Brief => "stage.after.brief",
                Stage::Script => "stage.after.script",
                Stage::Narration => "stage.after.narration",
                Stage::Capture => "stage.after.capture",
                Stage::Scenes => "stage.after.scenes",
                Stage::Clips => "stage.after.clips",
                Stage::Edit => "stage.after.edit",
                Stage::Render => "stage.after.render",
                Stage::Publish => "stage.after.publish",
            },
            Text::ProductsHint => "products.hint",
            Text::ProductsEmpty => "products.empty",
            Text::ProductsNotLoaded => "products.not_loaded",
            Text::NewProduct => "products.new",
            Text::ProductsShowArchived => "products.show_archived",
            Text::NewProductTitle => "product.new_title",
            Text::EditProductTitle => "product.edit_title",
            Text::ProductBriefHint => "product.brief_hint",
            Text::ProductName => "product.name",
            Text::ProductNamePlaceholder => "product.name_placeholder",
            Text::ProductKind => "product.kind",
            Text::ProductKindHint => "product.kind_hint",
            Text::ProductOwnership => "product.ownership",
            Text::ProductOwnershipHint => "product.ownership_hint",
            Text::ProductDescription => "product.description",
            Text::ProductDescriptionPlaceholder => "product.description_placeholder",
            Text::ProductAudience => "product.audience",
            Text::ProductAudiencePlaceholder => "product.audience_placeholder",
            Text::ProductFacts => "product.facts",
            Text::ProductFactsPlaceholder => "product.facts_placeholder",
            Text::ProductFactsHint => "product.facts_hint",
            Text::ProductFactsCount => "product.facts_count",
            Text::ProductOffer => "product.offer",
            Text::ProductOfferPlaceholder => "product.offer_placeholder",
            Text::ProductLink => "product.link",
            Text::ProductLinkPlaceholder => "product.link_placeholder",
            Text::ProductCallToAction => "product.call_to_action",
            Text::ProductCallToActionPlaceholder => "product.call_to_action_placeholder",
            Text::ProductMustSay => "product.must_say",
            Text::ProductMustSayPlaceholder => "product.must_say_placeholder",
            Text::ProductNeverSay => "product.never_say",
            Text::ProductNeverSayPlaceholder => "product.never_say_placeholder",
            Text::CreateProduct => "product.create",
            Text::SaveProduct => "product.save",
            Text::DuplicateProduct => "product.duplicate",
            Text::ArchiveProduct => "product.archive",
            Text::RestoreProduct => "product.restore",
            Text::DeleteProduct => "product.delete",
            Text::ProductDeleteTitle => "product.delete_title",
            Text::ProductDeleteHint => "product.delete_hint",
            Text::ProductActionsHint => "product.actions_hint",
            Text::ConfirmDeleteProduct => "product.confirm_delete",
            Text::ProductCancel => "product.cancel",
            Text::ProductSaved => "product.saved",
            Text::ProductDuplicated => "product.duplicated",
            Text::ProductArchived => "product.archived",
            Text::ProductRestored => "product.restored",
            Text::ProductDeleted => "product.deleted",
            Text::ProductArchivedTag => "product.archived_tag",
            Text::ProductCopySuffix => "product.copy_suffix",
            Text::ProductNameTaken => "product.error.name_taken",
            Text::ProductNotFound => "product.error.not_found",
            Text::ProductInUse => "product.error.in_use",
            Text::ProductNotSaved => "product.error.not_saved",
            Text::ProductPicturesTitle => "product.pictures.title",
            Text::ProductPicturesHint => "product.pictures.hint",
            Text::ProductPicturesSaveFirst => "product.pictures.save_first",
            Text::ProductPicturesNotLoaded => "product.pictures.not_loaded",
            Text::ProductImagesNone => "product.pictures.none",
            Text::ProductImageCount => "product.pictures.count",
            Text::AddProductImage => "product.pictures.add_image",
            Text::ProductImageKindHint => "product.pictures.kind_hint",
            Text::ProductImageCutOut => "product.pictures.cut_out",
            Text::ProductImageSize => "product.pictures.size",
            Text::RemoveProductImage => "product.pictures.remove_image",
            Text::ProductRightsConfirmedAt => "product.pictures.confirmed_at",
            Text::ProductRightsTitle => "product.pictures.rights_title",
            Text::ProductRightsHint => "product.pictures.rights_hint",
            Text::ProductRightsConfirm => "product.pictures.rights_confirm",
            Text::ProductChooseImage => "product.pictures.choose_image",
            Text::ProductChooseLogo => "product.pictures.choose_logo",
            Text::ProductImageDialog => "product.pictures.image_dialog",
            Text::ProductLogoDialog => "product.pictures.logo_dialog",
            Text::ProductLogoTitle => "product.pictures.logo_title",
            Text::ProductLogoHint => "product.pictures.logo_hint",
            Text::ProductLogoNone => "product.pictures.logo_none",
            Text::AddProductLogo => "product.pictures.add_logo",
            Text::ReplaceProductLogo => "product.pictures.replace_logo",
            Text::RemoveProductLogo => "product.pictures.remove_logo",
            Text::ProductImageRemoveAsk => "product.pictures.remove_image_ask",
            Text::ProductLogoRemoveAsk => "product.pictures.remove_logo_ask",
            Text::ProductImageNotConfirmed => "product.pictures.error.not_confirmed",
            Text::ProductTooManyImages => "product.pictures.error.too_many",
            Text::ProductImageRefused => "product.pictures.error.refused",
            Text::ProductImageUnreadable => "product.pictures.error.unreadable",
            Text::ProductImageNotFound => "product.pictures.error.not_found",
            Text::ProductNoLogo => "product.pictures.error.no_logo",
            Text::ProductImageAdded => "product.pictures.image_added",
            Text::ProductImageRemoved => "product.pictures.image_removed",
            Text::ProductLogoAdded => "product.pictures.logo_added",
            Text::ProductLogoRemoved => "product.pictures.logo_removed",
            Text::SettingsFileGroupProducts => "settings_file.group_products",
            Text::ProductKindName(value) => match value {
                ProductKind::Digital => "product.kind_name.digital",
                ProductKind::Physical => "product.kind_name.physical",
                ProductKind::Service => "product.kind_name.service",
                ProductKind::Idea => "product.kind_name.idea",
            },
            Text::ProductOwnershipName(value) => match value {
                ProductOwnership::Mine => "product.ownership_name.mine",
                ProductOwnership::Client => "product.ownership_name.client",
            },
            Text::ProductImageKindName(value) => match value {
                ProductImageKind::Photo => "product.pictures.kind.photo",
                ProductImageKind::Screen => "product.pictures.kind.screen",
                ProductImageKind::Concept => "product.pictures.kind.concept",
            },
            Text::ProductFieldError(value) => match value {
                ProductFieldError::NameRequired => "product.error.name_required",
                ProductFieldError::NameTooLong => "product.error.name_too_long",
                ProductFieldError::DescriptionRequired => "product.error.description_required",
                ProductFieldError::DescriptionTooLong => "product.error.description_too_long",
                ProductFieldError::AudienceTooLong => "product.error.audience_too_long",
                ProductFieldError::FactTooLong => "product.error.fact_too_long",
                ProductFieldError::TooManyFacts => "product.error.too_many_facts",
                ProductFieldError::OfferTooLong => "product.error.offer_too_long",
                ProductFieldError::LinkInvalid => "product.error.link_invalid",
                ProductFieldError::LinkTooLong => "product.error.link_too_long",
                ProductFieldError::CallToActionTooLong => "product.error.call_to_action_too_long",
                ProductFieldError::MustSayTooLong => "product.error.must_say_too_long",
                ProductFieldError::NeverSayTooLong => "product.error.never_say_too_long",
            },
            Text::ProductImageReason(reason) => match reason {
                ProductImageError::NotAnImage => "product.pictures.reason.not_an_image",
                ProductImageError::LogoNotPng => "product.pictures.reason.logo_not_png",
                ProductImageError::TooSmall { .. } => "product.pictures.reason.too_small",
                ProductImageError::TooLarge { .. } => "product.pictures.reason.too_large",
                ProductImageError::Shape { .. } => "product.pictures.reason.shape",
                ProductImageError::FileTooLarge(_) => "product.pictures.reason.file_too_large",
            },
            Text::ProductTabBrief => "product.tab.brief",
            Text::ProductTabSheet => "product.tab.sheet",
            Text::SheetTitle => "sheet.title",
            Text::SheetHint => "sheet.hint",
            Text::SheetInfo => "sheet.info",
            Text::SheetSaveFirst => "sheet.save_first",
            Text::SheetBriefUnsaved => "sheet.brief_unsaved",
            Text::SheetNotLoaded => "sheet.not_loaded",
            Text::SheetFillTitle => "sheet.fill.title",
            Text::SheetResearch => "sheet.fill.research",
            Text::SheetResearchAgain => "sheet.fill.research_again",
            Text::SheetResearchHint => "sheet.fill.research_hint",
            Text::SheetResearchNoLink => "sheet.fill.no_link",
            Text::SheetResearchCost => "sheet.fill.research_cost",
            Text::SheetResearchUsual => "sheet.fill.research_usual",
            Text::SheetFromBrief => "sheet.fill.from_brief",
            Text::SheetFromBriefHint => "sheet.fill.from_brief_hint",
            Text::SheetFromBriefCost => "sheet.fill.from_brief_cost",
            Text::SheetByHand => "sheet.fill.by_hand",
            Text::SheetByHandHint => "sheet.fill.by_hand_hint",
            Text::SheetResearching => "sheet.job.researching",
            Text::SheetWriting => "sheet.job.writing",
            Text::SheetJobStopped => "sheet.job.stopped",
            Text::SheetJobStopping => "sheet.job.stopping",
            Text::SheetJobFailed => "sheet.job.failed",
            Text::SheetJobLost => "sheet.job.lost",
            Text::SheetJobDeclined => "sheet.job.declined",
            Text::SheetJobDone => "sheet.job.done",
            Text::SheetJobLeftOut => "sheet.job.left_out",
            Text::SheetFixFirst => "sheet.fill.fix_first",
            Text::SheetBusyNote => "sheet.busy_note",
            Text::SheetRestrictedNotice => "sheet.restricted.notice",
            Text::SheetAboutTitle => "sheet.about.title",
            Text::SheetSummary => "sheet.about.summary",
            Text::SheetCategory => "sheet.about.category",
            Text::SheetRestricted => "sheet.about.restricted",
            Text::SheetRestrictedHint => "sheet.about.restricted_hint",
            Text::SheetRestrictedNone => "sheet.about.restricted_none",
            Text::SheetLooks => "sheet.about.looks",
            Text::SheetSegmentsTitle => "sheet.segments.title",
            Text::SheetSegmentName => "sheet.segments.name",
            Text::SheetSegmentWants => "sheet.segments.wants",
            Text::SheetSegmentHoldsBack => "sheet.segments.holds_back",
            Text::SheetAddSegment => "sheet.segments.add",
            Text::SheetObjectionsTitle => "sheet.objections.title",
            Text::SheetObjection => "sheet.objections.objection",
            Text::SheetObjectionAnswer => "sheet.objections.answer",
            Text::SheetAddObjection => "sheet.objections.add",
            Text::SheetAlternativesTitle => "sheet.alternatives.title",
            Text::SheetAlternativeName => "sheet.alternatives.name",
            Text::SheetAlternativeDiffers => "sheet.alternatives.differs",
            Text::SheetAddAlternative => "sheet.alternatives.add",
            Text::SheetRemoveRow => "sheet.rows.remove",
            Text::SheetRowsEmpty => "sheet.rows.empty",
            Text::SaveSheet => "sheet.save",
            Text::SheetSaved => "sheet.saved",
            Text::SheetClaimsTitle => "sheet.claims.title",
            Text::SheetClaimsHint => "sheet.claims.hint",
            Text::SheetClaimsWaiting => "sheet.claims.waiting",
            Text::SheetClaimsConfirmed => "sheet.claims.confirmed",
            Text::SheetClaimsRejected => "sheet.claims.rejected",
            Text::SheetClaimsNone => "sheet.claims.none",
            Text::ConfirmClaim => "sheet.claims.confirm",
            Text::RejectClaim => "sheet.claims.reject",
            Text::BringClaimBack => "sheet.claims.bring_back",
            Text::EditClaim => "sheet.claims.edit",
            Text::SaveClaim => "sheet.claims.save",
            Text::CancelClaimEdit => "sheet.claims.cancel",
            Text::ClaimSensitive => "sheet.claims.sensitive",
            Text::ClaimSensitiveHint => "sheet.claims.sensitive_hint",
            Text::ClaimConfirmedAt => "sheet.claims.confirmed_at",
            Text::ClaimEditedAt => "sheet.claims.edited_at",
            Text::ClaimOpenSource => "sheet.claims.open_source",
            Text::ClaimSourceTyped => "sheet.claims.source.typed",
            Text::ClaimSourceBrief => "sheet.claims.source.brief",
            Text::ClaimSourcePage => "sheet.claims.source.page",
            Text::ClaimSourceWeb => "sheet.claims.source.web",
            Text::AddClaim => "sheet.claims.add",
            Text::AddClaimPlaceholder => "sheet.claims.add_placeholder",
            Text::SheetSourcesTitle => "sheet.sources.title",
            Text::SheetResearchedAt => "sheet.sources.researched_at",
            Text::SheetSourcesNone => "sheet.sources.none",
            Text::SheetNone => "sheet.error.none",
            Text::SheetMissingKey => "sheet.error.missing_key",
            Text::SheetBusy => "sheet.error.busy",
            Text::SheetNotSaved => "sheet.error.not_saved",
            Text::ClaimKindName(value) => match value {
                ClaimKind::Benefit => "sheet.claims.kind.benefit",
                ClaimKind::Feature => "sheet.claims.kind.feature",
                ClaimKind::Proof => "sheet.claims.kind.proof",
                ClaimKind::Offer => "sheet.claims.kind.offer",
                ClaimKind::Comparison => "sheet.claims.kind.comparison",
            },
            Text::RestrictedCategoryName(value) => match value {
                RestrictedCategory::Health => "sheet.restricted.health",
                RestrictedCategory::Finance => "sheet.restricted.finance",
                RestrictedCategory::Alcohol => "sheet.restricted.alcohol",
                RestrictedCategory::Gambling => "sheet.restricted.gambling",
                RestrictedCategory::WeightLoss => "sheet.restricted.weight_loss",
                RestrictedCategory::Adult => "sheet.restricted.adult",
                RestrictedCategory::Political => "sheet.restricted.political",
            },
            Text::SheetFieldError(value) => match value {
                SheetFieldError::SummaryTooLong => "sheet.error.field.summary_too_long",
                SheetFieldError::CategoryTooLong => "sheet.error.field.category_too_long",
                SheetFieldError::LooksTooLong => "sheet.error.field.looks_too_long",
                SheetFieldError::TooManySegments => "sheet.error.field.too_many_segments",
                SheetFieldError::Segment => "sheet.error.field.segment",
                SheetFieldError::TooManyObjections => "sheet.error.field.too_many_objections",
                SheetFieldError::Objection => "sheet.error.field.objection",
                SheetFieldError::TooManyAlternatives => "sheet.error.field.too_many_alternatives",
                SheetFieldError::Alternative => "sheet.error.field.alternative",
            },
            Text::SheetClaimError(value) => match value {
                SheetClaimError::Claim(ClaimError::TextRequired) => {
                    "sheet.error.claim.text_required"
                }
                SheetClaimError::Claim(ClaimError::TextTooLong) => {
                    "sheet.error.claim.text_too_long"
                }
                SheetClaimError::Claim(ClaimError::NotFound) => "sheet.error.claim.not_found",
                SheetClaimError::Claim(ClaimError::AlreadyRejected) => {
                    "sheet.error.claim.already_rejected"
                }
                SheetClaimError::Claim(ClaimError::NotRejected) => "sheet.error.claim.not_rejected",
                SheetClaimError::Known => "sheet.error.claim.known",
                SheetClaimError::TooMany => "sheet.error.claim.too_many",
                SheetClaimError::Gone => "sheet.error.claim.gone",
            },
            Text::AdAngleNotSaved => "ad_angle.error.not_saved",
            Text::AdAngleNotFound => "ad_angle.error.not_found",
            Text::AdAnglesBusy => "ad_angles.error.busy",
            Text::AdAnglesNothingToRank => "ad_angles.error.nothing_to_rank",
            Text::AdAnglesNotLoaded => "ad_angles.error.not_loaded",
            Text::AdAngleFieldError(error) => match error {
                AdAngleFieldError::TitleRequired => "ad_angle.error.title_required",
                AdAngleFieldError::TitleTooLong => "ad_angle.error.title_too_long",
                AdAngleFieldError::AngleTooLong => "ad_angle.error.angle_too_long",
                AdAngleFieldError::SegmentTooLong => "ad_angle.error.segment_too_long",
                AdAngleFieldError::HookRequired => "ad_angle.error.hook_required",
                AdAngleFieldError::HookTooLong => "ad_angle.error.hook_too_long",
            },
            Text::AdAnglesMissing(missing) => match missing {
                AdAnglesMissing::Sheet => "ad_angles.missing.sheet",
                AdAnglesMissing::ConfirmedClaim => "ad_angles.missing.confirmed_claim",
                AdAnglesMissing::Archived => "ad_angles.missing.archived",
                AdAnglesMissing::Channel => "ad_angles.missing.channel",
            },
            Text::AdAnglesMissingKey(provider) => {
                return format!("ad_angles.error.missing_key.{}", provider.code()).into();
            }
            Text::StageBriefToConfirm => "stage.note.brief_to_confirm",
            Text::StageBriefClaimChanged => "stage.note.brief_claim_changed",
            Text::StageBriefClaimsChanged => "stage.note.brief_claims_changed",
            Text::StageBriefIncomplete => "stage.note.brief_incomplete",
            Text::StageBriefClaim => "stage.note.brief_claim",
            Text::StageBriefClaims => "stage.note.brief_claims",
            Text::StageScriptBriefChanged => "stage.note.script_brief_changed",
            Text::ScriptBriefFirst => "script.error.brief_first",
            Text::VersionAdProject => "versions.error.ad_project",
            Text::BriefNotAnAd => "brief.error.not_an_ad",
            Text::BriefClaimNotConfirmed => "brief.error.claim_not_confirmed",
            Text::BriefClaimNotUsed => "brief.error.claim_not_used",
            Text::BriefNotSaved => "brief.error.not_saved",
            Text::BriefNotLoaded => "brief.error.not_loaded",
            Text::BriefNotReady(reason) => match reason {
                BriefNotReady::NoClaim => "brief.not_ready.no_claim",
                BriefNotReady::ClaimsToDecide => "brief.not_ready.claims_to_decide",
                BriefNotReady::NoCallToAction => "brief.not_ready.no_call_to_action",
            },
            Text::BriefFieldError(error) => match error {
                BriefFieldError::OfferTooLong => "brief.error.offer_too_long",
                BriefFieldError::CallToActionTooLong => "brief.error.call_to_action_too_long",
                BriefFieldError::LinkTooLong => "brief.error.link_too_long",
            },
            Text::ProductTabAngles => "product.tab.angles",
            Text::AdAnglesTitle => "ad_angles.title",
            Text::AdAnglesInfo => "ad_angles.info",
            Text::AdAnglesHint => "ad_angles.hint",
            Text::AdAnglesSaveFirst => "ad_angles.save_first",
            Text::AdAnglesBriefUnsaved => "ad_angles.brief_unsaved",
            Text::AdAnglesProposeTitle => "ad_angles.propose_title",
            Text::AdAnglesChannel => "ad_angles.channel",
            Text::AdAnglesChannelHint => "ad_angles.channel_hint",
            Text::SuggestAdAngles => "ad_angles.suggest",
            Text::SuggestAdAnglesHint => "ad_angles.suggest_hint",
            Text::RankAdAngles => "ad_angles.rank",
            Text::AdAnglesUnranked => "ad_angles.unranked",
            Text::AdAnglesRankCost => "ad_angles.rank_cost",
            Text::AdAnglesRunning => "ad_angles.running",
            Text::AdAnglesStopped => "ad_angles.stopped",
            Text::AdAnglesEmpty => "ad_angles.empty",
            Text::AdAnglesDiscarded => "ad_angles.discarded",
            Text::AdAngleApproved => "ad_angle.approved",
            Text::AdAngleNotRanked => "ad_angle.not_ranked",
            Text::AdAngleSensitive => "ad_angle.sensitive",
            Text::AdAngleFramework => "ad_angle.framework",
            Text::AdAngleSegment => "ad_angle.segment",
            Text::AdAngleHook => "ad_angle.hook",
            Text::AdAngleClaims => "ad_angle.claims",
            Text::AdAngleClaimsNone => "ad_angle.claims_none",
            Text::AdAngleClaimGone => "ad_angle.claim_gone",
            Text::AdAngleHookScore => "ad_angle.score.hook",
            Text::AdAngleAudienceFit => "ad_angle.score.audience_fit",
            Text::AdAngleClarity => "ad_angle.score.clarity",
            Text::ApproveAdAngle => "ad_angle.approve",
            Text::ApproveAdAngleHint => "ad_angle.approve_hint",
            Text::AdAngleEditHint => "ad_angle.edit_hint",
            Text::AdAngleStarted => "ad_angle.started",
            Text::AdAngleOpenProject => "ad_angle.open_project",
            Text::ProjectAd => "projects.ad",
            Text::ProjectAdFor => "projects.ad_for",
            Text::ScriptBriefChanged => "script.brief_changed",
            Text::ScriptKeepAfterBrief => "script.keep_after_brief",
            Text::BriefInfo => "brief.info",
            Text::BriefHint => "brief.hint",
            Text::BriefAngleTitle => "brief.angle_title",
            Text::BriefProduct => "brief.product",
            Text::BriefClaimsTitle => "brief.claims.title",
            Text::BriefClaimsInfo => "brief.claims.info",
            Text::BriefClaimsHint => "brief.claims.hint",
            Text::BriefClaimsNone => "brief.claims.none",
            Text::BriefClaimsLeftOut => "brief.claims.left_out",
            Text::BriefClaimInAngle => "brief.claim.in_angle",
            Text::BriefClaimLeaveOut => "brief.claim.leave_out",
            Text::BriefClaimUse => "brief.claim.use",
            Text::BriefClaimEdited => "brief.claim.edited",
            Text::BriefClaimUnconfirmed => "brief.claim.unconfirmed",
            Text::BriefClaimKeep => "brief.claim.keep",
            Text::BriefClaimTake => "brief.claim.take",
            Text::BriefWordsTitle => "brief.words.title",
            Text::BriefWordsInfo => "brief.words.info",
            Text::BriefLink => "brief.link",
            Text::BriefSave => "brief.save",
            Text::BriefSaved => "brief.saved",
            Text::BriefLengthHint => "brief.length_hint",
            Text::BriefConfirm => "brief.confirm",
            Text::BriefConfirmHint => "brief.confirm_hint",
            Text::BriefConfirmed => "brief.confirmed",
            Text::BriefConfirmedNotice => "brief.confirmed_notice",
            Text::BriefToScript => "brief.to_script",
            Text::BriefAfterConfirm => "brief.after_confirm",
            Text::AdFrameworkName(framework) => match framework {
                AdFramework::ProblemSolution => "ad_framework.name.problem_solution",
                AdFramework::Demonstration => "ad_framework.name.demonstration",
                AdFramework::BeforeAfter => "ad_framework.name.before_after",
                AdFramework::HowItWorks => "ad_framework.name.how_it_works",
                AdFramework::StoryBehind => "ad_framework.name.story_behind",
                AdFramework::OldWayNewWay => "ad_framework.name.old_way_new_way",
                AdFramework::ThreeReasons => "ad_framework.name.three_reasons",
                AdFramework::MythFact => "ad_framework.name.myth_fact",
                AdFramework::Offer => "ad_framework.name.offer",
            },
            Text::AdFrameworkSteps(framework) => match framework {
                AdFramework::ProblemSolution => "ad_framework.steps.problem_solution",
                AdFramework::Demonstration => "ad_framework.steps.demonstration",
                AdFramework::BeforeAfter => "ad_framework.steps.before_after",
                AdFramework::HowItWorks => "ad_framework.steps.how_it_works",
                AdFramework::StoryBehind => "ad_framework.steps.story_behind",
                AdFramework::OldWayNewWay => "ad_framework.steps.old_way_new_way",
                AdFramework::ThreeReasons => "ad_framework.steps.three_reasons",
                AdFramework::MythFact => "ad_framework.steps.myth_fact",
                AdFramework::Offer => "ad_framework.steps.offer",
            },
            Text::ScriptBriefNotReady => "script.error.brief_not_ready",
            Text::GenerateAdScriptHint => "script.generate_hint_ad",
            Text::ScriptClaimsRunning => "script.claims.running",
            Text::ScriptClaimsStopped => "script.claims.stopped",
            Text::ScriptClaimsTitle => "script.claims.title",
            Text::ScriptClaimsInfo => "script.claims.info",
            Text::ScriptClaimsNotChecked => "script.claims.not_checked",
            Text::ScriptClaimsClean => "script.claims.clean",
            Text::ScriptClaimsUnbacked => "script.claims.unbacked",
            Text::ScriptClaimsUnbackedOne => "script.claims.unbacked_one",
            Text::ScriptClaimsOutdated => "script.claims.outdated",
            Text::ScriptClaimsOutdatedHint => "script.claims.outdated_hint",
            Text::ScriptClaimsUnbackedTitle => "script.claims.unbacked_title",
            Text::ScriptClaimsNoSentence => "script.claims.no_sentence",
            Text::ScriptClaimsUsedTitle => "script.claims.used_title",
            Text::ScriptClaimsSensitiveTitle => "script.claims.sensitive_title",
            Text::ScriptClaimsNoneUsed => "script.claims.none_used",
            Text::CheckScriptClaims => "script.claims.check",
            Text::CheckScriptClaimsAgain => "script.claims.check_again",
            Text::ScriptClaimsCheckedAt => "script.claims.checked_at",
            Text::ScriptClaimsPendingMissing => "script.claims.pending_missing",
            Text::NarrationClaimsUnbacked => "narration.claims.unbacked",
            Text::NarrationClaimsUnbackedOne => "narration.claims.unbacked_one",
            Text::NarrationClaimsNotChecked => "narration.claims.not_checked",
            Text::ScriptClaimsSaveFirst => "script.claims.save_first",
            Text::ScriptWrittenCheckStopped => "script.claims.written_stopped",
            Text::ScriptClaimsPendingOutdatedHint => "script.claims.pending_outdated_hint",
            Text::ScriptClaimsNothingStated => "script.claims.nothing_stated",
            Text::SceneKindNotAllowed => "scenes.error.kind_not_allowed",
            Text::SceneKindLabel => "scenes.kind.label",
            Text::SceneKindHint => "scenes.kind.hint",
            Text::SceneProductImage => "scenes.kind.image",
            Text::SceneComposed => "scenes.kind.composed",
            Text::SceneImageTooSmall => "scenes.kind.too_small",
            Text::ComposeScene => "scenes.kind.compose",
            Text::ComposeScenes => "scenes.kind.compose_missing",
            Text::SceneNotComposed => "scenes.kind.not_composed",
            Text::SceneClipComposed => "scenes.kind.no_clip",
            Text::EditorMotionComposed => "editor.motion.composed",
            Text::SceneKindName(value) => match value {
                SceneKind::Drawn => "scenes.kind.name.drawn",
                SceneKind::ProductInUse => "scenes.kind.name.product_in_use",
                SceneKind::ProductShot => "scenes.kind.name.product_shot",
                SceneKind::EndCard => "scenes.kind.name.end_card",
            },
            Text::SceneCardToCompose => "scenes.card.to_compose",
            Text::EditorSourceComposed => "editor.inspector.source_composed",
            Text::SceneImageRemoved => "scenes.kind.image_removed",
            Text::ShotBackdropGone => "scenes.look.backdrop_gone",
            Text::ShotBackdropInUse => "scenes.look.backdrop_in_use",
            Text::ShotLayoutLabel => "scenes.look.layout",
            Text::ShotLayoutHint => "scenes.look.layout_hint",
            Text::ShotPlacementLabel => "scenes.look.placement",
            Text::ShotBackdropLabel => "scenes.look.backdrop",
            Text::ShotBackdropHint => "scenes.look.backdrop_hint",
            Text::ShotBackdropAuto => "scenes.look.auto",
            Text::ShotBackdropFrom => "scenes.look.from",
            Text::ShotBackdropTo => "scenes.look.to",
            Text::ShotBackdropDrawn => "scenes.look.drawn",
            Text::DrawShotBackdrop => "scenes.look.draw",
            Text::DrawShotBackdropHint => "scenes.look.draw_hint",
            Text::DrawingShotBackdrop => "scenes.look.drawing",
            Text::ShotBackdropToReview => "scenes.look.to_review",
            Text::KeepShotBackdrop => "scenes.look.keep",
            Text::DiscardShotBackdrop => "scenes.look.discard",
            Text::ProductLayoutName(value) => match value {
                ProductLayout::Card => "scenes.look.layout_name.card",
                ProductLayout::Cutout => "scenes.look.layout_name.cutout",
                ProductLayout::Device => "scenes.look.layout_name.device",
            },
            Text::ShotPlacementName(value) => match value {
                ShotPlacement::Left => "scenes.look.placement_name.left",
                ShotPlacement::Center => "scenes.look.placement_name.center",
                ShotPlacement::Right => "scenes.look.placement_name.right",
            },
            Text::BackdropDirectionName(value) => match value {
                BackdropDirection::Down => "scenes.look.direction.down",
                BackdropDirection::Across => "scenes.look.direction.across",
                BackdropDirection::Diagonal => "scenes.look.direction.diagonal",
            },
            Text::RenderProductUnchecked => "render.product_unchecked",
            Text::RenderEndCardOutOfDate => "render.end_card_out_of_date",
            Text::RenderProductUncheckedScenes => "render.product_unchecked_scenes",
            Text::ProductCheckRefused => "scenes.product.check_refused",
            Text::ProductUncheckedClip => "scenes.product.unchecked_clip",
            Text::ProductCheckTitle => "scenes.product.check_title",
            Text::ProductCheckHint => "scenes.product.check_hint",
            Text::ProductMatches => "scenes.product.matches",
            Text::ProductCheckedNote => "scenes.product.checked",
            Text::ProductDrawAgain => "scenes.product.draw_again",
            Text::ProductUseRealImage => "scenes.product.use_real",
            Text::ProductNotDrawn => "scenes.product.not_drawn",
            Text::ProductRealImage => "scenes.product.real",
            Text::ProductToCheckTag => "scenes.product.to_check",
            Text::ClipProductCheckTitle => "clips.product.check_title",
            Text::ClipProductCheckHint => "clips.product.check_hint",
            Text::ClipProductMatches => "clips.product.matches",
            Text::ClipProductCheckedNote => "clips.product.checked",
            Text::ConceptTitle => "product.concept.title",
            Text::ConceptHint => "product.concept.hint",
            Text::DrawConcept => "product.concept.draw",
            Text::DrawConceptAgain => "product.concept.draw_again",
            Text::ConceptDrawingNow => "product.concept.drawing",
            Text::ConceptNewTitle => "product.concept.new",
            Text::KeepConcept => "product.concept.keep",
            Text::DiscardConcept => "product.concept.discard",
            Text::ConceptKeptNote => "product.concept.kept",
            Text::ConceptUnusedNote => "product.concept.unused",
            Text::ConceptNotOffered => "product.concept.error.not_offered",
            Text::ConceptMissingClaudeKey => "product.concept.error.claude_key",
            Text::ConceptMissingImageKey => "product.concept.error.image_key",
            Text::ConceptBusy => "product.concept.error.busy",
            Text::ConceptNothingWaiting => "product.concept.error.nothing_waiting",
            Text::ConceptNotSaved => "product.concept.error.not_saved",
            Text::ConceptBadge => "projects.concept.badge",
            Text::ConceptBadgeHint => "projects.concept.hint",
            Text::ConceptStopped => "product.concept.stopped",
            Text::ConceptFailed => "product.concept.failed",
            Text::ConceptKept => "product.concept.kept_notice",
            Text::ConceptCheckTitle => "scenes.product.concept_check_title",
            Text::ConceptCheckHint => "scenes.product.concept_check_hint",
            Text::ConceptUseImage => "scenes.product.use_concept",
        };
        Cow::Borrowed(key)
    }
}

fn source(language: UiLanguage) -> &'static str {
    match language {
        UiLanguage::EnUs => include_str!("../locales/en-US.toml"),
        UiLanguage::PtBr => include_str!("../locales/pt-BR.toml"),
    }
}

/// Strings of one language, keyed by dotted path (`app.name`).
#[derive(Debug, Clone)]
pub struct Catalog {
    language: UiLanguage,
    strings: BTreeMap<String, String>,
}

impl Catalog {
    /// Loads the embedded resource file. The files ship inside the binary and
    /// are checked by tests, so a parse failure is a build defect.
    pub fn load(language: UiLanguage) -> Self {
        let table: toml::Table = source(language)
            .parse()
            .unwrap_or_else(|e| panic!("locales/{language}.toml is invalid: {e}"));
        let mut strings = BTreeMap::new();
        flatten("", &table, &mut strings);
        Self { language, strings }
    }

    pub fn language(&self) -> UiLanguage {
        self.language
    }

    /// Whether the resource file has a string for `text`.
    pub fn has(&self, text: Text) -> bool {
        self.strings.contains_key(text.key().as_ref())
    }

    /// The string for `text`, or its key when missing so a gap is visible
    /// instead of blank.
    pub fn get(&self, text: Text) -> Cow<'_, str> {
        let key = text.key();
        match self.strings.get(key.as_ref()) {
            Some(text) => Cow::Borrowed(text.as_str()),
            None => Cow::Owned(key.into_owned()),
        }
    }

    /// The string for `text` with each `{name}` replaced by its value.
    pub fn format(&self, text: Text, args: &[(&str, &str)]) -> String {
        args.iter()
            .fold(self.get(text).into_owned(), |out, (name, value)| {
                out.replace(&format!("{{{name}}}"), value)
            })
    }

    /// A count in a few characters: `950`, `8.7K`, `48K`, `1.8M` in en-US.
    /// One decimal below ten units, cut rather than rounded, so `999,999`
    /// never shows as `1000K`.
    pub fn compact(&self, n: u64) -> String {
        let (unit, text) = match n {
            0..1_000 => return n.to_string(),
            1_000..1_000_000 => (1_000, Text::NumberThousands),
            1_000_000..1_000_000_000 => (1_000_000, Text::NumberMillions),
            _ => (1_000_000_000, Text::NumberBillions),
        };
        let tenths = n / (unit / 10);
        let number = if tenths < 100 && !tenths.is_multiple_of(10) {
            format!(
                "{}{}{}",
                tenths / 10,
                self.get(Text::DecimalSeparator),
                tenths % 10
            )
        } else {
            (tenths / 10).to_string()
        };
        self.format(text, &[("n", &number)])
    }

    /// Digits grouped by thousands: `1,234,567` in en-US.
    fn grouped(&self, n: u64) -> String {
        let digits = n.to_string();
        let separator = self.get(Text::ThousandsSeparator);
        let mut out = String::new();
        for (index, digit) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index).is_multiple_of(3) {
                out.push_str(&separator);
            }
            out.push(digit);
        }
        out
    }

    /// Dollars with `decimals` decimal places (2 to 6) and trailing zeros
    /// past the cents dropped, rounded half up.
    fn dollars(&self, amount: Money, decimals: u32) -> String {
        let unit = 10u64.pow(6 - decimals);
        let rounded = amount.micros().saturating_add(unit / 2) / unit;
        let scale = 10u64.pow(decimals);
        let (whole, fraction) = (rounded / scale, rounded % scale);
        let mut fraction = format!("{fraction:0width$}", width = decimals as usize);
        while fraction.len() > 2 && fraction.ends_with('0') {
            fraction.pop();
        }
        let n = format!(
            "{}{}{fraction}",
            self.grouped(whole),
            self.get(Text::DecimalSeparator)
        );
        self.format(Text::MoneyFormat, &[("n", &n)])
    }

    /// A level to the tenth of a decibel, signed: `−6.0 dB`, `+1,5 dB`,
    /// `0.0 dB`.
    pub fn decibels(&self, level: Decibels) -> String {
        let tenths = level.tenths();
        let sign = match tenths {
            ..0 => "\u{2212}",
            0 => "",
            _ => "+",
        };
        let magnitude = tenths.unsigned_abs();
        let n = format!(
            "{sign}{}{}{}",
            magnitude / 10,
            self.get(Text::DecimalSeparator),
            magnitude % 10
        );
        self.format(Text::EditorDecibels, &[("n", &n)])
    }

    /// A length to the tenth of a second: `0.5 s`, `1,2 s`.
    pub fn seconds(&self, length: Duration) -> String {
        let tenths = (length.as_millis() + 50) / 100;
        let n = format!(
            "{}{}{}",
            tenths / 10,
            self.get(Text::DecimalSeparator),
            tenths % 10
        );
        self.format(Text::EditorSeconds, &[("n", &n)])
    }

    /// A share as a percentage to the tenth: `71.6%`, `71,6%`, `100%`.
    pub fn percent(&self, share: Share) -> String {
        let tenths = (u64::from(share.ten_thousandths()) + 5) / 10;
        let n = if tenths.is_multiple_of(10) {
            self.grouped(tenths / 10)
        } else {
            format!(
                "{}{}{}",
                self.grouped(tenths / 10),
                self.get(Text::DecimalSeparator),
                tenths % 10
            )
        };
        self.format(Text::MetricsPercent, &[("n", &n)])
    }

    /// Minutes watched, in minutes under an hour and in short hours from
    /// there: `45 min`, `12 h`, `1.2K h`.
    pub fn watch_time(&self, minutes: u64) -> String {
        if minutes < 60 {
            return self.format(Text::MetricsMinutes, &[("n", &minutes.to_string())]);
        }
        self.format(Text::MetricsHours, &[("n", &self.compact(minutes / 60))])
    }

    /// A length on a clock: `0:34`, `12:05`, `1:02:09`.
    pub fn clock(&self, seconds: u64) -> String {
        let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
        if hours > 0 {
            format!("{hours}:{minutes:02}:{seconds:02}")
        } else {
            format!("{minutes}:{seconds:02}")
        }
    }

    /// A 0–100 score read as a fraction, as cut suggestions show it:
    /// `0.82`, `0,82`, `1.00`.
    pub fn score(&self, score: Score) -> String {
        let value = score.value();
        format!(
            "{}{}{:02}",
            value / 100,
            self.get(Text::DecimalSeparator),
            value % 100
        )
    }

    /// An amount spent or estimated, to the cent: `$1,234.56`, `US$ 0,05`.
    /// A non-zero amount that rounds to nothing reads `under $0.01`.
    pub fn money(&self, amount: Money) -> String {
        if !amount.is_zero() && amount < Money::from_micros(5_000) {
            let cent = self.dollars(Money::from_cents(1), 2);
            return self.format(Text::MoneyUnder, &[("amount", &cent)]);
        }
        self.dollars(amount, 2)
    }

    /// A price, to the millionth when it needs it: `$4.00`, `$0.042`.
    pub fn price(&self, amount: Money) -> String {
        self.dollars(amount, 6)
    }

    /// A plan's credits, grouped, with only the decimals they need:
    /// `1,250`, `982.5`, `982,5`.
    pub fn credits(&self, credits: bardo_domain::Credits) -> String {
        let hundredths = credits.hundredths();
        let (whole, cents) = (self.grouped(hundredths / 100), hundredths % 100);
        let separator = self.get(Text::DecimalSeparator);
        match cents {
            0 => whole,
            cents if cents % 10 == 0 => format!("{whole}{separator}{}", cents / 10),
            cents => format!("{whole}{separator}{cents:02}"),
        }
    }

    /// A month and its year: `October 2026`, `outubro de 2026`.
    pub fn month(&self, month: Month) -> String {
        self.format(
            Text::MonthFormat,
            &[
                ("month", &self.get(Text::MonthName(month.number()))),
                ("year", &month.year().to_string()),
            ],
        )
    }

    /// The order dates are typed and shown in.
    pub fn date_order(&self) -> DateOrder {
        match self.language {
            UiLanguage::EnUs => DateOrder::MonthFirst,
            UiLanguage::PtBr => DateOrder::DayFirst,
        }
    }

    /// A moment on a wall clock with its zone, e.g. `Sun, October 4, 2026,
    /// 6:00 PM (America/Sao_Paulo, UTC−03:00)`.
    pub fn date_time(&self, local: &LocalTime) -> String {
        let when = self.date_time_here(local);
        self.format(
            Text::DateTimeWithZone,
            &[("when", &when), ("zone", &self.zone(local))],
        )
    }

    /// `date_time` without the zone, for moments on this computer (when a
    /// backup was made), e.g. `Sun, October 4, 2026, 6:00 PM`.
    pub fn date_time_here(&self, local: &LocalTime) -> String {
        let time = self.time(local);
        self.format(
            Text::DateTimeFormat,
            &[
                ("weekday", &self.get(Text::WeekdayName(local.weekday))),
                ("month", &self.get(Text::MonthName(local.month))),
                ("day", &local.day.to_string()),
                ("year", &local.year.to_string()),
                ("time", &time),
            ],
        )
    }

    /// A day on this computer's calendar, e.g. `October 4, 2026`.
    pub fn date_here(&self, local: &LocalTime) -> String {
        self.format(
            Text::DateFormat,
            &[
                ("month", &self.get(Text::MonthName(local.month))),
                ("day", &local.day.to_string()),
                ("year", &local.year.to_string()),
            ],
        )
    }

    /// The time of day on a wall clock, e.g. `6:00 PM` or `18:00`.
    pub fn time(&self, local: &LocalTime) -> String {
        let (hour12, after_noon) = local.twelve_hour();
        let period = self.get(if after_noon {
            Text::TimePm
        } else {
            Text::TimeAm
        });
        self.format(
            Text::TimeFormat,
            &[
                ("hour", &format!("{:02}", local.hour)),
                ("hour12", &hour12.to_string()),
                ("minute", &format!("{:02}", local.minute)),
                ("period", &period),
            ],
        )
    }

    /// The zone of `local`: its name with its offset, or the offset alone
    /// for a zone without a name.
    pub fn zone(&self, local: &LocalTime) -> String {
        if local.zone == local.offset {
            local.offset.clone()
        } else {
            self.format(
                Text::ZoneWithOffset,
                &[("name", &local.zone), ("offset", &local.offset)],
            )
        }
    }

    /// How long ago something happened, e.g. `3 h ago`.
    pub fn age(&self, elapsed: Duration) -> String {
        let minutes = elapsed.as_secs() / 60;
        let (text, n) = match minutes {
            0 => return self.get(Text::FetchedJustNow).into_owned(),
            1..60 => (Text::FetchedMinutesAgo, minutes),
            60..2_880 => (Text::FetchedHoursAgo, minutes / 60),
            _ => (Text::FetchedDaysAgo, minutes / 1_440),
        };
        self.format(text, &[("n", &n.to_string())])
    }
}

fn flatten(prefix: &str, table: &toml::Table, out: &mut BTreeMap<String, String>) {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::Table(nested) => flatten(&key, nested, out),
            toml::Value::String(text) => {
                out.insert(key, text.clone());
            }
            other => panic!(
                "locale key {key} must be a string, found {}",
                other.type_str()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_texts() -> Vec<Text> {
        let mut texts = vec![
            Text::AppName,
            Text::OverviewHint,
            Text::OverviewNotLoaded,
            Text::OverviewNoProject,
            Text::OverviewPickUp,
            Text::OverviewNoVideos,
            Text::OverviewContinue,
            Text::OverviewOpen,
            Text::OverviewNextLabel,
            Text::OverviewNextWithCost,
            Text::OverviewWorking,
            Text::OverviewWaitingAt,
            Text::OverviewWaiting,
            Text::OverviewIdeasTitle,
            Text::OverviewIdeasNone,
            Text::OverviewIdeasUnranked,
            Text::OverviewIdeasRanked,
            Text::OverviewIdeasOrphans,
            Text::OverviewOpenIdeas,
            Text::OverviewChannels,
            Text::OverviewSpent,
            Text::OverviewNarrators,
            Text::OverviewNoNarrator,
            Text::OverviewAd,
            Text::StepConfirmBrief,
            Text::StepDecideClaims,
            Text::StepCompleteBrief,
            Text::StepWriteScript,
            Text::StepReviewScript,
            Text::StepRewriteScript,
            Text::StepRecordNarration,
            Text::StepRecordAgain,
            Text::StepFixTakes,
            Text::StepPlanScenes,
            Text::StepPlaceScenes,
            Text::StepPlanAgain,
            Text::StepReviewImages,
            Text::StepDrawImages,
            Text::StepReviewClips,
            Text::StepAnimateClips,
            Text::StepBuildCut,
            Text::StepRender,
            Text::StepRenderAgain,
            Text::StepResumeRender,
            Text::StepExport,
            Text::StepExportAgain,
            Text::StepExportRest,
            Text::StepResumeExport,
            Text::StepPost,
            Text::LibraryHint,
            Text::CostsUnknownProject,
            Text::LooseNotSaved,
            Text::LooseVideoNotFound,
            Text::LooseMoveBusy,
            Text::LooseMoveArchived,
            Text::LooseMoveVideoArchived,
            Text::DeleteVideoNoIdeaHint,
            Text::ChannelNeedsProject,
            Text::LooseSwitcherRow,
            Text::LooseCountNone,
            Text::LooseCountOne,
            Text::LooseCountMany,
            Text::LooseTitle,
            Text::LooseHint,
            Text::LooseNew,
            Text::LooseFormTitle,
            Text::LooseFormHint,
            Text::LooseIdeaTitle,
            Text::LooseIdeaTitlePlaceholder,
            Text::LooseIdeaAngle,
            Text::LooseIdeaAnglePlaceholder,
            Text::LooseCopyFrom,
            Text::LooseCopyDefaults,
            Text::LooseCreate,
            Text::LooseCancel,
            Text::LooseEmpty,
            Text::LooseExportOnly,
            Text::LooseFileTarget,
            Text::LooseMove,
            Text::LooseMoveTitle,
            Text::LooseMoveHint,
            Text::LooseMoveChannel,
            Text::LooseMoveConfirm,
            Text::LooseMoveNoChannels,
            Text::LooseMoved,
            Text::LooseDiffersLanguage,
            Text::LooseDiffersCountry,
            Text::LooseOverview,
            Text::LooseOpenVideos,
            Text::OwnIdea,
            Text::OwnIdeaTitle,
            Text::OwnIdeaHint,
            Text::OwnIdeaChannel,
            Text::OwnIdeaSave,
            Text::OwnIdeaCancel,
            Text::OwnIdeaSaved,
            Text::DeleteLooseVideoHint,
            Text::SwitcherNoProject,
            Text::SwitcherSearch,
            Text::SwitcherFavorites,
            Text::SwitcherRecent,
            Text::SwitcherAll,
            Text::SwitcherNewProject,
            Text::SwitcherStar,
            Text::SwitcherUnstar,
            Text::SwitcherMoveUp,
            Text::SwitcherNoMatch,
            Text::SwitcherEmpty,
            Text::SwitcherOpen,
            Text::ProjectChannelsNone,
            Text::ProjectChannelsOne,
            Text::ProjectChannelsMany,
            Text::ProjectNeverOpened,
            Text::NewProjectTitle,
            Text::NewProjectHint,
            Text::ProjectNameLabel,
            Text::ProjectNamePlaceholder,
            Text::ProjectColorLabel,
            Text::CreateProject,
            Text::ProjectFormCancel,
            Text::ProjectNameRequired,
            Text::ProjectNameTooLong,
            Text::ProjectNameTaken,
            Text::ProjectNotFoundAny,
            Text::ProjectNotSaved,
            Text::ProjectListNotLoaded,
            Text::ProjectFolderTooDeep,
            Text::ProjectFolderNotMade,
            Text::ProjectFolderLabel,
            Text::ProjectFolderChoose,
            Text::ProjectFolderHint,
            Text::JobWaitsForFolder,
            Text::FolderNotFound,
            Text::FolderNotFoundHint,
            Text::PackageExportBusy,
            Text::PackageNotAPackage,
            Text::PackageNewer,
            Text::PackageDamaged,
            Text::PackageFolderNotEmpty,
            Text::PackageNotRead,
            Text::OpenFolderNotAProject,
            Text::OpenFolderOtherCopy,
            Text::OpenFolderNoRecords,
            Text::DeleteRecordsNotWritten,
            Text::SwitcherExportProject,
            Text::SwitcherExporting,
            Text::SwitcherImportPackage,
            Text::SwitcherOpenFolder,
            Text::PackageImportPrompt,
            Text::OpenFolderPrompt,
            Text::PackageExportStarted,
            Text::PackageImportStarted,
            Text::FolderRelinked,
            Text::FolderReopened,
            Text::FolderOpenedArchived,
            Text::ImportPackageTitle,
            Text::ImportPackageHint,
            Text::ImportPackageFrom,
            Text::ImportPackageContents,
            Text::ImportPackageTemplates,
            Text::ImportPackageTemplatesHint,
            Text::ImportPackageConfirm,
            Text::WorldNotFound,
            Text::WorldNameTaken,
            Text::CharacterInUse,
            Text::WorldNotSaved,
            Text::WorldVideoBusy,
            Text::WorldHint,
            Text::WorldEmpty,
            Text::WorldNotLoaded,
            Text::WorldNew,
            Text::WorldShowArchived,
            Text::WorldTabCharacters,
            Text::WorldTabPlaces,
            Text::WorldTabObjects,
            Text::WorldNewPlace,
            Text::WorldNewObject,
            Text::WorldEmptyPlaces,
            Text::WorldEmptyObjects,
            Text::PlaceNewTitle,
            Text::PlaceEditTitle,
            Text::PlaceFormHint,
            Text::PlaceNamePlaceholder,
            Text::PlaceAppearance,
            Text::PlaceAppearancePlaceholder,
            Text::PlaceAppearanceHint,
            Text::PlaceAlwaysPlaceholder,
            Text::PlaceNeverPlaceholder,
            Text::PlaceCreate,
            Text::ObjectNewTitle,
            Text::ObjectEditTitle,
            Text::ObjectFormHint,
            Text::ObjectNamePlaceholder,
            Text::ObjectAppearancePlaceholder,
            Text::ObjectAppearanceHint,
            Text::ObjectAlwaysPlaceholder,
            Text::ObjectNeverPlaceholder,
            Text::ObjectCreate,
            Text::ObjectBelongsTo,
            Text::ObjectBelongsToNobody,
            Text::ObjectBelongsToHint,
            Text::WorldNoProject,
            Text::WorldOwnerProject,
            Text::WorldOwnerGlobal,
            Text::CharacterNewTitle,
            Text::CharacterEditTitle,
            Text::CharacterFormHint,
            Text::CharacterName,
            Text::CharacterNamePlaceholder,
            Text::CharacterAppearance,
            Text::CharacterAppearancePlaceholder,
            Text::CharacterAppearanceHint,
            Text::CharacterAlways,
            Text::CharacterAlwaysPlaceholder,
            Text::CharacterNever,
            Text::CharacterNeverPlaceholder,
            Text::CharacterNotesHint,
            Text::CharacterCreate,
            Text::CharacterSave,
            Text::CharacterDuplicate,
            Text::CharacterArchive,
            Text::CharacterRestore,
            Text::CharacterDelete,
            Text::CharacterDeleteTitle,
            Text::CharacterDeleteHint,
            Text::CharacterConfirmDelete,
            Text::CharacterCancel,
            Text::CharacterActionsHint,
            Text::CharacterSaved,
            Text::CharacterDuplicated,
            Text::CharacterArchived,
            Text::CharacterRestored,
            Text::CharacterDeleted,
            Text::CharacterArchivedTag,
            Text::CharacterDropsTitle,
            Text::CharacterDropsPictures,
            Text::CharacterDropsNone,
            Text::CharacterDropsConfirm,
            Text::CharacterUses,
            Text::CharacterUsesNone,
            Text::CharacterUseArchived,
            Text::CharacterCopySuffix,
            Text::CharacterBelongings,
            Text::CharacterBelongingsNone,
            Text::WorldPicturesTitle,
            Text::WorldPicturesHint,
            Text::WorldPicturesSaveFirst,
            Text::WorldPicturesNone,
            Text::WorldPictureLook,
            Text::WorldPictureLookNotes,
            Text::WorldPictureDraw,
            Text::WorldPictureDrawing,
            Text::WorldPictureWaiting,
            Text::WorldPictureKeep,
            Text::WorldPictureDiscard,
            Text::WorldPictureRemove,
            Text::WorldPictureRemoveAsk,
            Text::WorldPictureKept,
            Text::WorldPictureDiscarded,
            Text::WorldPictureRemoved,
            Text::WorldPictureStarted,
            Text::WorldPictureBusy,
            Text::WorldPictureNothingWaiting,
            Text::WorldPictureMissingKey,
            Text::WorldPictureOverBudget,
            Text::WorldImportNotConfirmed,
            Text::WorldImportUnreadable,
            Text::WorldPictureImport,
            Text::WorldImportTitle,
            Text::WorldImportHint,
            Text::WorldImportConfirm,
            Text::WorldImportChoose,
            Text::WorldImportDialog,
            Text::WorldImported,
            Text::WhoAppearsTitle,
            Text::WhoAppearsHint,
            Text::WhoAppearsNone,
            Text::WhoAppearsNoneAvailable,
            Text::WhoAppearsPickable,
            Text::WhoAppearsAdd,
            Text::WhoAppearsRemove,
            Text::WhoAppearsInstruction,
            Text::WhoAppearsInstructionPlaceholder,
            Text::WhoAppearsSave,
            Text::WhoAppearsSaved,
            Text::WhoAppearsChanged,
            Text::WhoAppearsFull,
            Text::WhoAppearsCharacters,
            Text::WhoAppearsPlaces,
            Text::WhoAppearsObjects,
            Text::WhoAppearsBelongings,
            Text::WhoAppearsBelongsTo,
            Text::WhoAppearsFullPlaces,
            Text::WhoAppearsFullObjects,
            Text::ScenesPlaceSaveProject,
            Text::ScenesPlaceSaveGlobal,
            Text::ScenesPlaceSaveHint,
            Text::ScenesPlaceFromWorld,
            Text::WhoAppearsTooMany,
            Text::WhoAppearsInstructionTooLong,
            Text::CastSaveToWorld,
            Text::CastSaveToLibrary,
            Text::CastSaveHint,
            Text::CastFromWorld,
            Text::LocateFolder,
            Text::LocateFolderPrompt,
            Text::ShowFolder,
            Text::SettingsStorageTab,
            Text::BaseFolderLabel,
            Text::BaseFolderHint,
            Text::BaseFolderChange,
            Text::BaseFolderPrompt,
            Text::BaseFolderUseDefault,
            Text::BaseFolderIsDefault,
            Text::BaseFolderAvoidedSync,
            Text::BaseFolderSynced,
            Text::BaseFolderNotSaved,
            Text::StorageMoveHint,
            Text::MoveChannel,
            Text::MoveChannelTitle,
            Text::MoveChannelHint,
            Text::MoveChannelTarget,
            Text::MoveChannelConfirm,
            Text::MoveChannelDone,
            Text::MoveChannelBlocked,
            Text::MoveChannelNoOther,
            Text::ChannelProjectLabel,
            Text::FilterAllProjects,
            Text::FilterNoProject,
            Text::FilterProjectLabel,
            Text::CostsByProject,
            Text::RemovalNotFound,
            Text::RemovalBlocked,
            Text::RemovalWrongState,
            Text::RemovalNotSaved,
            Text::JobWaitsArchived,
            Text::DeleteNameMismatch,
            Text::DeleteFolderNotRemoved,
            Text::DeleteNotDisconnected,
            Text::SwitcherArchiveProject,
            Text::SwitcherDeleteProject,
            Text::SwitcherArchivedProjects,
            Text::SwitcherBack,
            Text::ArchiveProjectTitle,
            Text::ArchiveProjectHint,
            Text::ArchiveCompress,
            Text::ArchiveCompressHint,
            Text::ArchiveConfirm,
            Text::ArchiveVideoTitle,
            Text::ArchiveVideoHint,
            Text::ArchivedProjectsTitle,
            Text::ArchivedProjectsEmpty,
            Text::ArchivedProjectsHint,
            Text::ArchivedPacked,
            Text::ArchivedBusy,
            Text::Unarchive,
            Text::UnarchiveUnpacking,
            Text::DeleteAction,
            Text::DeleteProjectTitle,
            Text::DeleteVideoTitle,
            Text::DeleteProjectHint,
            Text::DeleteVideoHint,
            Text::DeleteFolderRecycle,
            Text::DeleteFolderPermanent,
            Text::DeleteNoFolder,
            Text::DeleteTypeName,
            Text::DeletePermanently,
            Text::DeletePermanentlyHint,
            Text::DeleteConfirm,
            Text::DeleteRunning,
            Text::VideosShowArchived,
            Text::VideosArchivedEmpty,
            Text::ArchiveVideoAction,
            Text::ArchivedTag,
            Text::VideoArchivedNotice,
            Text::DeletedProjects,
            Text::DeletedOn,
            Text::DeletedProjectForgotten,
            Text::DeletedVideoForgotten,
            Text::ForgetName,
            Text::ForgetNameHint,
            Text::ThemeStartAgain,
            Text::ThemeNoVideo,
            Text::AppTagline,
            Text::UiLanguageLabel,
            Text::LanguageNotSaved,
            Text::ChannelsTitle,
            Text::ChannelsEmpty,
            Text::ChannelsNotLoaded,
            Text::NewChannel,
            Text::NewChannelTitle,
            Text::EditChannelTitle,
            Text::ChannelName,
            Text::ChannelNamePlaceholder,
            Text::ChannelNiche,
            Text::ChannelNichePlaceholder,
            Text::ChannelThemes,
            Text::ChannelThemesPlaceholder,
            Text::ChannelThemesHint,
            Text::ChannelAestheticNotes,
            Text::ChannelAestheticNotesPlaceholder,
            Text::ChannelLanguage,
            Text::ChannelCountry,
            Text::ChannelCountrySearch,
            Text::CreateChannel,
            Text::SaveChannel,
            Text::ChannelSaved,
            Text::ChannelNameTaken,
            Text::ChannelNotFound,
            Text::ChannelNotSaved,
            Text::ChannelPersona,
            Text::ChannelPersonaNone,
            Text::ChannelPersonaHint,
            Text::ChannelPersonaNotFound,
            Text::ChannelClipModel,
            Text::ChannelClipModelDefault,
            Text::ProjectSettingsVideo,
            Text::ProjectSettingsProduction,
            Text::ChannelClipModelHint,
            Text::ChannelTakeModel,
            Text::ChannelTakeModelHint,
            Text::ChannelCaptionStyle,
            Text::ChannelCaptionStyleHint,
            Text::ChannelCaptionHighlight,
            Text::ChannelCaptionHighlightHint,
            Text::ChannelFormat,
            Text::ChannelFormatHint,
            Text::ChannelTransition,
            Text::ChannelTransitionHint,
            Text::ChannelTransitionLength,
            Text::ChannelMotion,
            Text::ChannelMotionHint,
            Text::ChannelMotionStrength,
            Text::ChannelClipModelNotOffered,
            Text::ChannelTakeModelNotOffered,
            Text::ChannelAccountsTitle,
            Text::ChannelAccountsHint,
            Text::ChannelAccountsSaveFirst,
            Text::ChannelAccountsEmpty,
            Text::ChannelAccountsNotLoaded,
            Text::AllNetworksAdded,
            Text::AddNetworkAccount,
            Text::NewNetworkAccountTitle,
            Text::EditNetworkAccountTitle,
            Text::AccountHandle,
            Text::AccountHandlePlaceholder,
            Text::AccountMetadataTitle,
            Text::AccountMetadataHint,
            Text::AccountLanguage,
            Text::AccountLanguageChannel,
            Text::AccountTags,
            Text::AccountTagsPlaceholder,
            Text::AccountTagsHint,
            Text::AccountFooter,
            Text::AccountFooterPlaceholder,
            Text::AccountVisibility,
            Text::AccountVisibilityOnlyPublic,
            Text::RenderPresetTitle,
            Text::RenderPresetHint,
            Text::RenderPresetAspect,
            Text::RenderPresetResolution,
            Text::RenderPresetCodec,
            Text::RenderPresetBitrate,
            Text::RenderPresetMaxDuration,
            Text::RenderPresetMaxDurationHint,
            Text::RenderPresetLoudness,
            Text::RenderPresetNetworkDefault,
            Text::RenderPresetSummary,
            Text::RenderPresetCustom,
            Text::RenderPresetDefault,
            Text::RenderPresetEffective,
            Text::CreateNetworkAccount,
            Text::SaveNetworkAccount,
            Text::CancelNetworkAccount,
            Text::EditNetworkAccount,
            Text::RemoveNetworkAccount,
            Text::NetworkAccountRemoveConfirm,
            Text::ConfirmRemoveNetworkAccount,
            Text::KeepNetworkAccount,
            Text::NetworkAccountSaved,
            Text::NetworkAccountRemoved,
            Text::NetworkAccountTaken,
            Text::NetworkAccountNotFound,
            Text::NetworkAccountNotSaved,
            Text::PersonasTitle,
            Text::PersonasHint,
            Text::PersonasEmpty,
            Text::PersonasNotLoaded,
            Text::NewPersona,
            Text::NewPersonaTitle,
            Text::EditPersonaTitle,
            Text::PersonaName,
            Text::PersonaNamePlaceholder,
            Text::PersonaVoice,
            Text::PersonaVoiceNone,
            Text::PersonaVoiceHint,
            Text::ChooseVoice,
            Text::ReloadVoices,
            Text::HideVoices,
            Text::LoadingVoices,
            Text::VoicesTitle,
            Text::VoicesEmpty,
            Text::VoicesFailed,
            Text::VoicesMissingKey,
            Text::VoiceAvailable,
            Text::VoiceMissing,
            Text::VoiceSelected,
            Text::VoicePreviewPlay,
            Text::VoicePreviewStop,
            Text::VoicePreviewNone,
            Text::VoicePreviewNote,
            Text::VoiceSampleTitle,
            Text::VoiceSampleHint,
            Text::VoiceSampleSentence,
            Text::VoiceSamplePlay,
            Text::VoiceSampleStop,
            Text::VoiceSampleReading,
            Text::VoiceSampleFree,
            Text::VoiceSampleBilled,
            Text::VoiceSampleNoVoice,
            Text::VoiceSampleMissingKey,
            Text::VoiceSampleTooLong,
            Text::VoiceSampleFailed,
            Text::VoiceSampleNotKept,
            Text::VoiceSampleCannotPlay,
            Text::PersonaTone,
            Text::PersonaTonePlaceholder,
            Text::PersonaScriptStyle,
            Text::PersonaScriptStylePlaceholder,
            Text::PersonaPresets,
            Text::PersonaPresetsHint,
            Text::PresetStability,
            Text::PresetStabilityHint,
            Text::PresetSimilarity,
            Text::PresetSimilarityHint,
            Text::PresetStyle,
            Text::PresetStyleHint,
            Text::PresetSpeed,
            Text::PresetSpeedHint,
            Text::PresetPercent,
            Text::CreatePersona,
            Text::SavePersona,
            Text::DuplicatePersona,
            Text::PersonaSaved,
            Text::PersonaDuplicated,
            Text::PersonaUsedBy,
            Text::PersonaUnused,
            Text::PersonaConfirmTitle,
            Text::PersonaConfirmHint,
            Text::ConfirmPersonaSave,
            Text::CancelPersonaSave,
            Text::PersonaNameTaken,
            Text::PersonaNotFound,
            Text::PersonaNotSaved,
            Text::PersonaCopySuffix,
            Text::PersonaImportedSuffix,
            Text::ImportPersona,
            Text::ImportPersonaDialog,
            Text::ExportPersona,
            Text::PersonaExportHint,
            Text::PersonaExported,
            Text::PersonaImported,
            Text::CheckVoices,
            Text::PersonaPackageUnreadable,
            Text::PersonaPackageNotOne,
            Text::PersonaPackageNewer,
            Text::PersonaPackageInvalid,
            Text::PersonaExportFailed,
            Text::FileDialogFailed,
            Text::ProjectNarrator,
            Text::ProjectNarratorChannel,
            Text::ProjectNarratorChannelNone,
            Text::ProjectNarratorHint,
            Text::ProjectFormat,
            Text::ProjectFormatChannel,
            Text::ProjectFormatHint,
            Text::ProjectLength,
            Text::ProjectLengthFormat,
            Text::ProjectLengthHint,
            Text::ChannelProduction,
            Text::ChannelProductionHint,
            Text::ProjectMode,
            Text::ProjectModeChannel,
            Text::ProjectModeHint,
            Text::ProjectModeOption,
            Text::ProjectModeMissingKey,
            Text::ProjectModeMissingSignIn,
            Text::ProjectEstimate,
            Text::ProjectEstimatePartial,
            Text::SpendCap,
            Text::SpendCapHint,
            Text::SpendCapPlaceholder,
            Text::SaveSpendCap,
            Text::SpendCapSaved,
            Text::SpendCapInvalid,
            Text::SpendCapOver,
            Text::SpendCapDefault,
            Text::SpendCapUseDefault,
            Text::SpendCapOffer,
            Text::SpendCapOfferAccept,
            Text::SpendCapOfferDecline,
            Text::SpendCapKept,
            Text::CapReachedTitle,
            Text::CapReachedLine,
            Text::ClipsEconomy,
            Text::AnimateHookClips,
            Text::JobsTitle,
            Text::JobsEmpty,
            Text::JobsRunning,
            Text::JobsQueued,
            Text::JobsFailed,
            Text::JobsFinished,
            Text::JobRetryScheduled,
            Text::JobFailedAfter,
            Text::CancelJob,
            Text::RetryJob,
            Text::JobNotUpdated,
            Text::JobNotStarted,
            Text::TestJobsTitle,
            Text::TestJobsHint,
            Text::StartTestJob,
            Text::StartFailingTestJob,
            Text::SettingsTitle,
            Text::ProviderKeysTitle,
            Text::ProviderKeysHint,
            Text::SetupStepByStep,
            Text::SetupTitle,
            Text::SetupBack,
            Text::SetupPasteHere,
            Text::SetupOpenInGuide,
            Text::SetupAudits,
            Text::SetupAuditTitle,
            Text::SetupCopyBlock,
            Text::SetupCopied,
            Text::KeyNotSet,
            Text::KeySaved,
            Text::KeyUnreadable,
            Text::SaveKey,
            Text::ReplaceKey,
            Text::TestKey,
            Text::TestingKey,
            Text::RemoveKey,
            Text::KeyCheckDetail,
            Text::ProviderKeyNotSet,
            Text::ProviderKeyStoreFailed,
            Text::ResearchTitle,
            Text::ResearchNoChannels,
            Text::ResearchChannel,
            Text::ResearchSeeds,
            Text::ResearchSeedsPlaceholder,
            Text::ResearchSeedsHint,
            Text::RunResearch,
            Text::RefreshResearch,
            Text::RefreshResearchCost,
            Text::ResearchCost,
            Text::ResearchCostNone,
            Text::ResearchMissingKey,
            Text::ResearchNotStarted,
            Text::ResearchNotLoaded,
            Text::ResearchRunning,
            Text::ResearchStopped,
            Text::ResearchResultsTitle,
            Text::ResearchScoresHint,
            Text::ResearchOpportunity,
            Text::ResearchCompetition,
            Text::ResearchTrend,
            Text::ResearchUploads,
            Text::ResearchMedianViews,
            Text::ResearchViewsPerDay,
            Text::ResearchMedianSubscribers,
            Text::ResearchSmallChannels,
            Text::ResearchSample,
            Text::ResearchFetched,
            Text::ResearchNotFetched,
            Text::ResearchNoUploads,
            Text::ResearchStale,
            Text::ThemesTitle,
            Text::ThemesNoChannels,
            Text::ThemesChannel,
            Text::ThemesNiche,
            Text::ThemesNoNiche,
            Text::SuggestThemesHint,
            Text::SuggestThemes,
            Text::RankThemes,
            Text::ThemesUnranked,
            Text::ThemesRunning,
            Text::ThemesStopped,
            Text::ThemesListTitle,
            Text::ThemesRankingHint,
            Text::ThemesEmpty,
            Text::ThemesDiscarded,
            Text::ThemePriority,
            Text::ThemeConfidence,
            Text::ThemeFit,
            Text::ThemeTrend,
            Text::ThemeCompetition,
            Text::ThemePerformance,
            Text::ThemePerformanceNiche,
            Text::ThemePerformanceNicheOne,
            Text::ThemePerformanceChannel,
            Text::ThemePerformanceChannelOne,
            Text::ThemePerformanceProjected,
            Text::ThemesNoHistory,
            Text::ThemesBeforeHistory,
            Text::ThemeNotRanked,
            Text::ThemeRankedBy,
            Text::ThemeTitle,
            Text::ThemeAngle,
            Text::EditTheme,
            Text::SaveTheme,
            Text::CancelThemeEdit,
            Text::DiscardTheme,
            Text::ApproveTheme,
            Text::ThemeApproved,
            Text::ProjectsTitle,
            Text::ProjectsEmpty,
            Text::ProjectStarted,
            Text::ThemeNotSaved,
            Text::ThemesPickNiche,
            Text::ThemeNotFound,
            Text::ThemesMissingKey(Provider::Claude),
            Text::ThemesMissingKey(Provider::TypeSafe),
            Text::ThemesBusy,
            Text::ThemesNothingToRank,
            Text::ThemesNotLoaded,
            Text::ProjectsNav,
            Text::ProjectsNoChannels,
            Text::ProjectsNotLoaded,
            Text::ProjectNotFound,
            Text::ScriptTitle,
            Text::ScriptEmpty,
            Text::GenerateScript,
            Text::GenerateScriptHint,
            Text::RegenerateScript,
            Text::RegenerateScriptHint,
            Text::ScriptRunning,
            Text::ScriptStopped,
            Text::SaveScript,
            Text::RevertScript,
            Text::ScriptSaved,
            Text::ScriptEdited,
            Text::ScriptWords,
            Text::ScriptCurrent,
            Text::ScriptPendingTitle,
            Text::ScriptPendingHint,
            Text::AcceptScript,
            Text::RejectScript,
            Text::ProvenanceProvider,
            Text::ProvenanceModel,
            Text::ProvenanceTemplate,
            Text::ProvenanceTemplateVersion,
            Text::ProvenanceTokens,
            Text::ProvenanceTokensValue,
            Text::ProvenanceGenerated,
            Text::ShowPrompt,
            Text::HidePrompt,
            Text::PromptInstructions,
            Text::PromptTask,
            Text::ScriptMissing,
            Text::ScriptNothingToReview,
            Text::ScriptMissingKey,
            Text::ScriptBusy,
            Text::ScriptNotSaved,
            Text::NarrationTitle,
            Text::NarrationEmpty,
            Text::GenerateNarration,
            Text::NarrationGenerateHint,
            Text::RegenerateNarration,
            Text::NarrationRunning,
            Text::NarrationStopped,
            Text::NarrationStale,
            Text::NarrationStaleTag,
            Text::NarrationPlay,
            Text::NarrationPause,
            Text::NarrationWordsHint,
            Text::NarrationVoice,
            Text::NarrationCost,
            Text::NarrationCostValue,
            Text::NarrationDuration,
            Text::NarrationNoScript,
            Text::NarrationNoPersona,
            Text::NarrationMissingKey,
            Text::NarrationBusy,
            Text::NarrationScenesBusy,
            Text::NarrationOutdatesPictures,
            Text::NarrationOutdatesFit,
            Text::NarrationOutdatesReplan,
            Text::NarrationOutdatesConfirm,
            Text::NarrationOutdatesCancel,
            Text::NarrationMissing,
            Text::NarrationAudioMissing,
            Text::NarrationCannotPlay,
            Text::NarrationNotLoaded,
            Text::ImportNarration,
            Text::ImportNarrationDialog,
            Text::ImportNarrationHint,
            Text::RecordingReading,
            Text::RecordingChosen,
            Text::RecordingReplaces,
            Text::UseRecording,
            Text::CancelRecording,
            Text::NarrationAligning,
            Text::NarrationRecording,
            Text::NarrationAudioValue,
            Text::NarrationImported,
            Text::RecordingUnreadable,
            Text::RecordingUnsupported,
            Text::RecordingEmpty,
            Text::RecordingTooLarge,
            Text::CaptureImport,
            Text::CaptureImportDialog,
            Text::CaptureImportHint,
            Text::CaptureChosen,
            Text::CaptureVoice,
            Text::CaptureVoicePersona,
            Text::CaptureVoiceOwn,
            Text::CaptureVoiceOwnShort,
            Text::CaptureConfirm,
            Text::CaptureSteps,
            Text::UseCapture,
            Text::CaptureRunning,
            Text::CaptureReplacesNote,
            Text::NarrationCaptured,
            Text::CaptureSummary,
            Text::CaptureConfirmed,
            Text::CaptureCoverage,
            Text::CaptureTakesTitle,
            Text::CaptureTakesHint,
            Text::CaptureTake,
            Text::CaptureTakeSpan,
            Text::CaptureTakeOk,
            Text::CaptureTakeTooLong,
            Text::CaptureTakeTooShort,
            Text::CaptureCutEarlier,
            Text::CaptureCutLater,
            Text::CaptureStale,
            Text::CaptureRemove,
            Text::CaptureRemoveHint,
            Text::CaptureRemoveConfirm,
            Text::CaptureRemoveKeep,
            Text::CaptureNoPersona,
            Text::CaptureNotConfirmed,
            Text::CaptureMissing,
            Text::CaptureNoCutPoint,
            Text::CaptureNotSaved,
            Text::StageTakes,
            Text::StageTakesOne,
            Text::CaptureTakeFlagged,
            Text::CaptureVoicePersonaNone,
            Text::CaptureModeLabel,
            Text::CaptureModeHint,
            Text::AnalysisHint,
            Text::AnalysisNotStarted,
            Text::AnalysisQueued,
            Text::AnalysisRunning,
            Text::AnalysisRunningLeft,
            Text::AnalysisDone,
            Text::AnalysisCancelled,
            Text::AnalysisFailed,
            Text::AnalysisOnGpu,
            Text::AnalysisOnCpu,
            Text::AnalysisCpuNote,
            Text::AnalysisRedone,
            Text::AnalyzeTake,
            Text::ResumeAnalysis,
            Text::CheckNoFace,
            Text::CheckSeveralFaces,
            Text::CheckSmallFace,
            Text::CheckTurned,
            Text::CheckMaskJumps,
            Text::ChecksPassed,
            Text::CaptureTakeBlocked,
            Text::ControlTitle,
            Text::ControlHint,
            Text::ControlWaiting,
            Text::ControlNotBuilt,
            Text::ControlQueued,
            Text::ControlBuilding,
            Text::ControlCancelled,
            Text::ControlFailed,
            Text::ControlReady,
            Text::ControlApproved,
            Text::ControlApprovalLost,
            Text::ControlUncovered,
            Text::ControlUncoveredBlocked,
            Text::ControlSeveralFacesBlocked,
            Text::ControlReviewHint,
            Text::ControlBuild,
            Text::ControlBuildAgain,
            Text::ControlStopBuild,
            Text::ControlApprove,
            Text::ControlPlay,
            Text::ControlStopPlaying,
            Text::ControlSetFrame,
            Text::ControlNotReady,
            Text::ControlLengthBlocked,
            Text::ControlCannotPlay,
            Text::SetLabel,
            Text::SetHint,
            Text::SetRoom,
            Text::SetSceneImage,
            Text::SetSceneNumber,
            Text::SetImport,
            Text::SetImportDialog,
            Text::SetNoScenes,
            Text::SetUnreadable,
            Text::PerformedTitle,
            Text::PerformedHint,
            Text::TakeModelLabel,
            Text::TakeModelChannelTag,
            Text::PerformedBusy,
            Text::PerformedPending,
            Text::PerformedMissingClaudeKey,
            Text::PerformedMissingHiggsfieldKey,
            Text::PerformedNothing,
            Text::PerformedDraftFirst,
            Text::TakeModelGone,
            Text::PerformedClipName,
            Text::TakePromptLabel,
            Text::TakePromptHint,
            Text::WriteTakePrompt,
            Text::WriteTakePromptAgain,
            Text::TakePromptWriting,
            Text::TakePromptFailed,
            Text::EditTakePrompt,
            Text::SaveTakePrompt,
            Text::CancelTakePrompt,
            Text::TakePromptEdited,
            Text::TakePromptFrameNote,
            Text::TakePromptFrameNoteRecording,
            Text::MakeTakeDraft,
            Text::MakeTakeFinal,
            Text::RegenerateTakeDraft,
            Text::MakeTakeFinalAgain,
            Text::ApproveTakeDraft,
            Text::BackToTakeDrafts,
            Text::UsePerformedTake,
            Text::PlayTakeDraft,
            Text::PlayTakeFinal,
            Text::TakeMakingDraft,
            Text::TakeMakingFinal,
            Text::TakeQueued,
            Text::StopTakeWaiting,
            Text::TakeStoppedWaiting,
            Text::CheckTakeAgain,
            Text::TakeFailed,
            Text::TakeRefused,
            Text::TakeDraftReady,
            Text::TakeDraftApproved,
            Text::TakeFinalReady,
            Text::TakeLonger,
            Text::TakeShorter,
            Text::TakeDraftNote,
            Text::TakeFinalPaidNote,
            Text::TakeMakeHint,
            Text::PerformedUsed,
            Text::PerformedUsedNow,
            Text::PerformedRealisticNote,
            Text::StageTakesFlagged,
            Text::ScenesTitle,
            Text::ScenesEmpty,
            Text::PlanScenes,
            Text::PlanScenesHint,
            Text::ReplanScenes,
            Text::ReplanScenesConfirm,
            Text::ConfirmReplanScenes,
            Text::CancelReplanScenes,
            Text::ScenesPlanning,
            Text::ScenesDrawing,
            Text::ScenesStopping,
            Text::ScenesStopped,
            Text::ScenesStale,
            Text::ScenesStaleFit,
            Text::ScenesStaleTag,
            Text::FitScenes,
            Text::FitScenesTitle,
            Text::FitScenesHint,
            Text::ScenesPaidHeld,
            Text::ScenesPaidHeldReplan,
            Text::ScenesPaidHeldVersion,
            Text::ScenesCount,
            Text::GenerateSceneImages,
            Text::SceneImagesHint,
            Text::SceneLabel,
            Text::SceneEdited,
            Text::EditScenePrompt,
            Text::SaveScenePrompt,
            Text::CancelScenePrompt,
            Text::RegenerateSceneImage,
            Text::SceneNoImage,
            Text::SceneFailed,
            Text::SceneImageRecord,
            Text::ScenePendingTitle,
            Text::ScenePendingHint,
            Text::AcceptSceneImage,
            Text::RejectSceneImage,
            Text::ClipsHint,
            Text::AnimateMissingClips,
            Text::AnimateScene,
            Text::AnimateSceneAgain,
            Text::SceneAnimating,
            Text::SceneClipFailed,
            Text::SceneClipModel,
            Text::SceneClipModelChannel,
            Text::SceneClipModelGone,
            Text::SceneClipModelReplaced,
            Text::ClipsChannelModelReplaced,
            Text::SceneClipPlan,
            Text::SceneClipPlanUnpriced,
            Text::SceneMotionPrompt,
            Text::SceneMotionFromImage,
            Text::EditMotionPrompt,
            Text::MotionPromptHint,
            Text::SceneClipRecord,
            Text::SceneClipStale,
            Text::PlayClip,
            Text::UseSceneStill,
            Text::ScenePendingClipTitle,
            Text::ScenePendingClipHint,
            Text::AcceptSceneClip,
            Text::RejectSceneClip,
            Text::ScenesNoNarration,
            Text::ScenesNoPlan,
            Text::ScenesNarrationTooLong,
            Text::SceneNotFound,
            Text::ScenesMissingClaudeKey,
            Text::ScenesMissingGeminiKey,
            Text::ScenesMissingHiggsfieldKey,
            Text::ScenesMissingKlingKey,
            Text::SceneNoImageToAnimate,
            Text::ScenesNothingToAnimate,
            Text::SceneClipModelNotOffered,
            Text::ScenesBusy,
            Text::ScenesNarrationBusy,
            Text::ScenesPlanOutOfDate,
            Text::ScenesOtherText,
            Text::ScenesWouldDiscardImages,
            Text::ScenesNothingToGenerate,
            Text::SceneNothingToReview,
            Text::ScenesNotLoaded,
            Text::TemplatesTitle,
            Text::TemplatesHint,
            Text::TemplateVersionsTitle,
            Text::TemplateVersionLabel,
            Text::TemplateCurrent,
            Text::TemplateEditing,
            Text::TemplateInstructions,
            Text::TemplateInstructionsHint,
            Text::TemplatePrompt,
            Text::TemplatePromptHint,
            Text::TemplateVariablesTitle,
            Text::TemplateVariablesHint,
            Text::SaveTemplate,
            Text::RevertTemplate,
            Text::DefaultTemplate,
            Text::TemplateSaved,
            Text::TemplateUnchanged,
            Text::TemplateNotSaved,
            Text::TemplateNotFound,
            Text::TemplatesNotLoaded,
            Text::TemplateGlobalUsedBy,
            Text::TemplateAlreadyOwn,
            Text::LibraryNameClash,
            Text::LibraryOtherProject,
            Text::LibraryGlobalSource,
            Text::LibraryGlobalTag,
            Text::LibraryGlobalNote,
            Text::LibraryUsesTitle,
            Text::LibrarySaveAnyway,
            Text::LibraryCancel,
            Text::LibraryBring,
            Text::LibraryBringTitle,
            Text::LibraryBringHint,
            Text::LibraryBringName,
            Text::LibraryBringConfirm,
            Text::SharingNotFound,
            Text::SharingNotOwn,
            Text::SharingLinkedReadOnly,
            Text::SharingNotShareable,
            Text::SharingSameProject,
            Text::SharingNameClash,
            Text::SharingInvalidName,
            Text::SharingFolderUnreachable,
            Text::SharingConfirmPictures,
            Text::SharingFileUnreadable,
            Text::SharingFileNotWritten,
            Text::SharingNotSaved,
            Text::SharingNotAnItemFile,
            Text::SharingNewerItemFile,
            Text::SharingInvalidItemFile,
            Text::SharingTitle,
            Text::SharingShareable,
            Text::SharingShareableHint,
            Text::SharingFollows,
            Text::SharingFollowsTemplate,
            Text::SharingLinkedHint,
            Text::SharingUpdating,
            Text::SharingWaitingForFolder,
            Text::SharingUnlink,
            Text::SharingLost,
            Text::SharingLostTemplate,
            Text::SharingLostOk,
            Text::SharingExport,
            Text::SharingExportTitle,
            Text::SharingExportHint,
            Text::SharingExportTo,
            Text::SharingExportDone,
            Text::SharingNoOtherProject,
            Text::SharingMakeGlobal,
            Text::SharingMakeGlobalTitle,
            Text::SharingMakeGlobalHint,
            Text::SharingMakeGlobalTemplateHint,
            Text::SharingMadeGlobal,
            Text::SharingSaveFile,
            Text::SharingSaved,
            Text::SharingOpenFile,
            Text::SharingOpened,
            Text::SharingImport,
            Text::SharingImportHint,
            Text::SharingImportEmpty,
            Text::SharingImportLinked,
            Text::SharingImportIndependent,
            Text::SharingImported,
            Text::SharingArchivedProject,
            Text::SharingLinkedTag,
            Text::SharingShareableTag,
            Text::SharingNameLabel,
            Text::SharingGlobalName,
            Text::SharingConfirmPicturesCheck,
            Text::SharingClose,
            Text::SharingWrongKind,
            Text::SharingGlobalUsedBy,
            Text::LibraryBrought,
            Text::LibraryNoProject,
            Text::TemplateSourceProject,
            Text::TemplateSourceGlobal,
            Text::TemplateSourceProjectHint,
            Text::TemplateSourceGlobalHint,
            Text::TemplateMakeOwn,
            Text::TemplateMadeOwn,
            Text::FetchedJustNow,
            Text::FetchedMinutesAgo,
            Text::FetchedHoursAgo,
            Text::FetchedDaysAgo,
            Text::NumberThousands,
            Text::NumberMillions,
            Text::NumberBillions,
            Text::DecimalSeparator,
            Text::MoneyFormat,
            Text::ThousandsSeparator,
            Text::MoneyUnder,
            Text::MonthFormat,
            Text::CostsTitle,
            Text::CostsHint,
            Text::CostsPreviousMonth,
            Text::CostsNextMonth,
            Text::CostsTotal,
            Text::CostsEmpty,
            Text::CostsUnpriced,
            Text::CostsNotLoaded,
            Text::CostsNotSaved,
            Text::CostsNotPaid,
            Text::ProjectBudgetHint,
            Text::BackupSetUpProjectKey,
            Text::DeleteRemovesKeys,
            Text::EstimateProviderOwnKey,
            Text::CostsPaidByProjects,
            Text::CostsDeletedProjectName,
            Text::CostsProjectKeyGone,
            Text::CostsProjectKeysHint,
            Text::CostsProjectKeysTitle,
            Text::SwitcherProjectKeys,
            Text::ProjectBudgetNone,
            Text::ProjectBudgetAmount,
            Text::ProjectBudgetLabel,
            Text::ProjectKeyUsesApp,
            Text::ProjectKeysNoProjectOpen,
            Text::ProjectKeysInfo,
            Text::ProjectKeysHint,
            Text::ProjectKeysTitle,
            Text::SettingsProjectTab,
            Text::DeleteKeysNotRemoved,
            Text::ProjectKeysNoProject,
            Text::ProjectBudgetNeedsKey,
            Text::CostsProvidersTitle,
            Text::CostsProvidersHint,
            Text::CostsChannelsTitle,
            Text::CostsVideosTitle,
            Text::CostsUnknownChannel,
            Text::CostsUnknownVideo,
            Text::BudgetNone,
            Text::BudgetUsed,
            Text::BudgetNear,
            Text::BudgetReached,
            Text::SetBudget,
            Text::ChangeBudget,
            Text::RemoveBudget,
            Text::SaveBudget,
            Text::CancelBudget,
            Text::BudgetPlaceholder,
            Text::BudgetSaved,
            Text::BudgetRemoved,
            Text::RatesTitle,
            Text::RatesHint,
            Text::RateAllModels,
            Text::RateChanged,
            Text::RateAdded,
            Text::EditRate,
            Text::SaveRate,
            Text::CancelRate,
            Text::ResetRate,
            Text::RemoveRate,
            Text::RateSaved,
            Text::AddRateTitle,
            Text::RateProvider,
            Text::RateModel,
            Text::RateModelPlaceholder,
            Text::RateMeter,
            Text::RatePrice,
            Text::RatePricePlaceholder,
            Text::AddRate,
            Text::EstimateCost,
            Text::EstimatePartial,
            Text::EstimateUnknown,
            Text::EstimateNear,
            Text::EstimateRedraw,
            Text::ProjectSpent,
            Text::BudgetReachedTitle,
            Text::BudgetReachedLine,
            Text::PriceUnknownTitle,
            Text::PriceUnknownLine,
            Text::BudgetQuestion,
            Text::BudgetConfirm,
            Text::BudgetCancel,
            Text::OpenEditor,
            Text::EditorBack,
            Text::EditorDuration,
            Text::EditorUndo,
            Text::EditorRedo,
            Text::EditorReviewRender,
            Text::EditorJobsProxies,
            Text::EditorJobsIdle,
            Text::EditorJobFailed,
            Text::EditorBinScenes,
            Text::EditorBinMedia,
            Text::EditorBinCaptionStyles,
            Text::EditorSceneLabel,
            Text::EditorProxyBadge,
            Text::EditorPlay,
            Text::EditorPause,
            Text::EditorPreviousFrame,
            Text::EditorNextFrame,
            Text::EditorInspectorEmpty,
            Text::EditorInspectorClip,
            Text::EditorSourceImage,
            Text::EditorSourceClip,
            Text::EditorSourceStill,
            Text::EditorSourceNone,
            Text::EditorIn,
            Text::EditorOut,
            Text::EditorLength,
            Text::EditorFile,
            Text::EditorNarrationLabel,
            Text::EditorToolSelect,
            Text::EditorToolSplit,
            Text::EditorToolTransition,
            Text::EditorSnapWords,
            Text::EditorAiCuts,
            Text::EditorRetention,
            Text::CutsSuggest,
            Text::CutsSuggestAgain,
            Text::CutsHint,
            Text::CutsAgainHint,
            Text::CutsNoPoints,
            Text::CutsRunning,
            Text::CutsStop,
            Text::CutsStopped,
            Text::CutsRetry,
            Text::CutsEmpty,
            Text::CutsAcceptStrong,
            Text::CutsFloor,
            Text::CutsHidden,
            Text::CutsShown,
            Text::CutsShowHidden,
            Text::CutsHideLow,
            Text::CutsTitle,
            Text::CutsAccept,
            Text::CutsReject,
            Text::CutsNext,
            Text::CutsUndo,
            Text::CutsAccepted,
            Text::CutsConfidence,
            Text::CutsReasonSentence,
            Text::CutsReasonPause,
            Text::CutsReasonScene,
            Text::CutsReasonTopic,
            Text::CutsMissingKey(Provider::TypeSafe),
            Text::CutsBusy,
            Text::CutsGone,
            Text::CutsNotSaved,
            Text::CutsFloorNotSaved,
            Text::EditorDuckMusic,
            Text::EditorZoomIn,
            Text::EditorZoomOut,
            Text::EditorTrackCaptions,
            Text::EditorTrackVideo,
            Text::EditorTrackNarration,
            Text::EditorTrackMusic,
            Text::EditorTrackSfx,
            Text::EditorEmptyTitle,
            Text::EditorEmptyNoNarration,
            Text::EditorEmptyNoScenes,
            Text::EditorBackToProject,
            Text::EditorBuildingProxies,
            Text::EditorKeepEditing,
            Text::EditorMediaMissing,
            Text::EditorProxyFailed,
            Text::EditorProxyCancelled,
            Text::EditorProxyBuilding,
            Text::EditorProblemsOne,
            Text::EditorProblemsMany,
            Text::EditorProblemLine,
            Text::EditorProblemNoImage,
            Text::EditorProblemFileGone,
            Text::EditorProblemCancelled,
            Text::EditorRetryProxies,
            Text::EditorMediaOffline,
            Text::EditorStale,
            Text::EditorPreviewBuilding,
            Text::EditorNothingToRetry,
            Text::EditorFfmpegMissing,
            Text::EditorPreviewFailed,
            Text::EditorNotLoaded,
            Text::EditorNoSound,
            Text::EditorNothingToCut,
            Text::EditorCannotEdit,
            Text::EditorCaptionTextInvalid,
            Text::EditorPerformedNotHeard,
            Text::EditorMediaTake,
            Text::EditorMediaPutOnCut,
            Text::EditorMediaPerformedRoughCut,
            Text::EditorMediaPerformedRoughCutHint,
            Text::EditorPerformedTitle,
            Text::EditorPerformedInSync,
            Text::EditorPerformedEarly,
            Text::EditorPerformedLate,
            Text::EditorPerformedUnheard,
            Text::EditorPerformedPutBack,
            Text::EditorPerformedPutBackHint,
            Text::EditorPerformedCannotSync,
            Text::EditorPerformedUnheardHint,
            Text::EditorPerformedCover,
            Text::EditorPerformedCoverHint,
            Text::EditorPerformedCoverScene,
            Text::EditorPerformedCoverLength,
            Text::EditorPerformedCoverButton,
            Text::EditorEditNotSaved,
            Text::EditorCutReset,
            Text::EditorInspectorAudio,
            Text::EditorSourceIn,
            Text::EditorRemove,
            Text::EditorShortcuts,
            Text::EditorInspectorTrack,
            Text::EditorLevel,
            Text::EditorMute,
            Text::EditorSolo,
            Text::EditorMuteShort,
            Text::EditorSoloShort,
            Text::EditorDuck,
            Text::EditorDuckDepth,
            Text::EditorDuckHint,
            Text::EditorDuckReadout,
            Text::EditorNoMusic,
            Text::EditorFades,
            Text::EditorFadeIn,
            Text::EditorFadeOut,
            Text::EditorTransition,
            Text::EditorTransitionLength,
            Text::EditorTransitionEverywhere,
            Text::EditorTransitionFirst,
            Text::EditorTransitionHint,
            Text::EditorTransitionShort,
            Text::EditorMotion,
            Text::EditorMotionStrength,
            Text::EditorMotionEverywhere,
            Text::EditorMotionVary,
            Text::EditorMotionHint,
            Text::EditorMotionClip,
            Text::EditorSeconds,
            Text::EditorDecibels,
            Text::EditorLaneHint,
            Text::EditorInspectorCaption,
            Text::EditorCaptionText,
            Text::EditorCaptionTextHint,
            Text::EditorCaptionStyle,
            Text::EditorCaptionStyleHint,
            Text::EditorCaptionHighlight,
            Text::EditorCaptionHighlightHint,
            Text::EditorShowCaptions,
            Text::EditorFraming,
            Text::EditorFramingFit,
            Text::EditorFramingFill,
            Text::EditorFramingCustom,
            Text::EditorFramingPosition,
            Text::EditorFramingLandscapeHint,
            Text::EditorFramingDragHint,
            Text::EditorFramingFitHint,
            Text::EditorFramingSource,
            Text::EditorMediaImport,
            Text::EditorMediaImportHint,
            Text::EditorMediaImportingOne,
            Text::EditorMediaImporting,
            Text::EditorMediaEmpty,
            Text::EditorMediaAddMusic,
            Text::EditorMediaAddSfx,
            Text::EditorMediaAddVideo,
            Text::EditorMediaAddHint,
            Text::EditorMediaImportFailed,
            Text::EditorMediaAudio,
            Text::EditorMediaVideo,
            Text::EditorSourceFootage,
            Text::EditorSourcePerformed,
            Text::EditorProblemFootageLine,
            Text::MediaImportUnreadable,
            Text::MediaImportUnsupported,
            Text::MediaImportTooShort,
            Text::MediaImportNotSaved,
            Text::MusicPromptTitle,
            Text::MusicPromptEmpty,
            Text::MusicPromptGenerate,
            Text::MusicPromptGenerateHint,
            Text::MusicPromptRegenerate,
            Text::MusicPromptRegenerateHint,
            Text::MusicPromptRunning,
            Text::MusicPromptStopped,
            Text::MusicPromptSave,
            Text::MusicPromptRevert,
            Text::MusicPromptSaved,
            Text::MusicPromptCopy,
            Text::MusicPromptCopied,
            Text::MusicPromptEdited,
            Text::MusicPromptMissing,
            Text::MusicPromptMissingKey,
            Text::MusicPromptBusy,
            Text::MusicPromptNotSaved,
            Text::ChapterRename,
            Text::ChaptersStopped,
            Text::ThumbnailWriteStopped,
            Text::ThumbnailDrawStopped,
            Text::MetadataChaptersNote,
            Text::ThumbnailTitle,
            Text::ThumbnailHint,
            Text::ThumbnailShortNote,
            Text::ThumbnailWrite,
            Text::ThumbnailWriteHint,
            Text::ThumbnailRewrite,
            Text::ThumbnailWriting,
            Text::ThumbnailText,
            Text::ThumbnailTextHint,
            Text::ThumbnailPlacement,
            Text::ThumbnailLookLabel,
            Text::ThumbnailBackground,
            Text::ThumbnailBackgroundHint,
            Text::ThumbnailBackgroundsEmpty,
            Text::ThumbnailBackgroundDrawn,
            Text::ThumbnailBackgroundScene,
            Text::ThumbnailPrompt,
            Text::ThumbnailPromptHint,
            Text::ThumbnailDraw,
            Text::ThumbnailDrawing,
            Text::ThumbnailSave,
            Text::ThumbnailSaving,
            Text::ThumbnailSaved,
            Text::ThumbnailOutdated,
            Text::ThumbnailNotYetSaved,
            Text::ThumbnailApply,
            Text::ThumbnailWriteMissingKey,
            Text::ThumbnailDrawMissingKey,
            Text::ThumbnailBusy,
            Text::ThumbnailNoPrompt,
            Text::ThumbnailNoBackground,
            Text::ThumbnailBackgroundMissing,
            Text::ThumbnailNotComposed,
            Text::ThumbnailNotSaved,
            Text::ChaptersTitle,
            Text::ChaptersHint,
            Text::ChaptersShortNote,
            Text::ChaptersWrite,
            Text::ChaptersWriteHint,
            Text::ChaptersRewrite,
            Text::ChaptersWriting,
            Text::ChaptersEmpty,
            Text::ChaptersInclude,
            Text::ChaptersEdited,
            Text::ChapterEarlier,
            Text::ChapterLater,
            Text::ChapterRemove,
            Text::ChapterCutAway,
            Text::ChaptersStale,
            Text::ChaptersLeftOut,
            Text::ChaptersNoCut,
            Text::ChaptersNoNarration,
            Text::ChaptersMissing,
            Text::ChapterCannotMove,
            Text::ChaptersMissingKey,
            Text::ChaptersBusy,
            Text::ChaptersNotSaved,
            Text::UploadThumbnail,
            Text::UploadThumbnailHint,
            Text::UploadThumbnailSet,
            Text::UploadThumbnailNotAllowed,
            Text::UploadThumbnailFailed,
            Text::ExportFileThumbnail,
            Text::ChannelThumbnailLook,
            Text::ChannelThumbnailLookHint,
            Text::MusicTrackHeading,
            Text::MusicTrackEmpty,
            Text::MusicTrackGenerate,
            Text::MusicTrackGenerateHint,
            Text::MusicTrackRegenerate,
            Text::MusicTrackRegenerateHint,
            Text::MusicTrackRunning,
            Text::MusicTrackStopped,
            Text::MusicTrackNew,
            Text::MusicTrackInUse,
            Text::MusicTrackDetails,
            Text::MusicTrackPlay,
            Text::MusicTrackPause,
            Text::MusicTrackUse,
            Text::MusicTrackUseHint,
            Text::MusicTrackDiscard,
            Text::MusicTrackUsed,
            Text::MusicTrackDiscarded,
            Text::MusicTrackNoNarration,
            Text::MusicTrackMissingKey,
            Text::MusicTrackBusy,
            Text::MusicTrackNotFound,
            Text::MusicTrackAudioMissing,
            Text::MusicTrackCannotPlay,
            Text::MusicTrackNotSaved,
            Text::MusicTrackNoRoom,
        ];
        texts.extend(NicheSeedError::ALL.map(Text::NicheSeedError));
        texts.extend(Provider::ALL.map(Text::ProviderName));
        texts.extend(ModelLevel::ALL.map(Text::ModelLevelName));
        texts.extend(Provider::ALL.map(Text::ProviderPurpose));
        texts.extend(Provider::with_keys().map(Text::ProviderKeyPlaceholder));
        texts.extend(KeyCheckOutcome::ALL.map(Text::KeyCheckOutcome));
        texts.extend(ApiKeyError::ALL.map(Text::ApiKeyError));
        texts.extend(UiLanguage::ALL.map(Text::LanguageName));
        texts.extend(JobKind::ALL.map(Text::JobKindName));
        texts.extend(JobState::ALL.map(Text::JobStateName));
        texts.extend(JobFailureKind::ALL.map(Text::JobFailureKindName));
        texts.extend(ContentLanguage::ALL.map(Text::ContentLanguageName));
        texts.extend(Country::ALL.map(Text::CountryName));
        texts.extend(ChannelFieldError::ALL.map(Text::ChannelFieldError));
        texts.extend(ProjectColor::ALL.map(Text::ProjectColorName));
        texts.extend(crate::MoveBlock::ALL.map(Text::MoveChannelBlock));
        texts.extend(bardo_domain::RemovalBlock::ALL.map(Text::RemovalBlock));
        texts.extend(
            [
                bardo_domain::PermanentReason::RemovableDrive,
                bardo_domain::PermanentReason::NetworkDrive,
                bardo_domain::PermanentReason::LargerThanBin,
                bardo_domain::PermanentReason::BinOff,
            ]
            .map(Text::PermanentReasonText),
        );
        texts.extend(crate::LocateError::ALL.map(Text::LocateRefused));
        texts.extend(PersonaFieldError::ALL.map(Text::PersonaFieldError));
        texts.extend(WorldItemFieldError::ALL.map(Text::WorldItemFieldError));
        texts.extend(PictureOrigin::ALL.map(Text::WorldPictureOrigin));
        texts.extend(Network::ALL.map(Text::NetworkName));
        texts.extend(Visibility::ALL.map(Text::VisibilityName));
        texts.extend(AspectRatio::ALL.map(Text::AspectRatioName));
        texts.extend(CaptionStyle::ALL.map(Text::CaptionStyleName));
        texts.extend(CaptionHighlight::ALL.map(Text::CaptionHighlightName));
        texts.extend(TransitionKind::ALL.map(Text::TransitionName));
        texts.extend(VideoFormat::ALL.map(Text::VideoFormatName));
        texts.extend(VideoFormat::ALL.map(Text::VideoFormatBrief));
        texts.extend(VideoFormat::ALL.map(Text::VideoFormatLength));
        texts.extend(VideoLength::ALL.map(Text::VideoLengthName));
        texts.extend(ProductionMode::ALL.map(Text::ProductionModeName));
        texts.extend(ProductionMode::ALL.map(Text::ProductionModeAbout));
        texts.extend(StyleChoice::all().map(Text::VisualStyleName));
        texts.extend(ClipFit::ALL.map(Text::ClipFitName));
        texts.extend([
            Text::ChannelClipFit,
            Text::ChannelClipFitHint,
            Text::ChannelStretchLimit,
            Text::ChannelStretchLimitHint,
            Text::StretchLimitChoice,
            Text::SceneClipSlowedNext,
            Text::SceneClipSlowed,
            Text::SceneClipSlowedHint,
            Text::EditorSourceSlowed,
            Text::ChannelStyle,
            Text::ChannelStyleHint,
            Text::ProjectStyle,
            Text::ProjectStyleChannel,
            Text::ProjectStyleChannelNotes,
            Text::ProjectStyleHint,
            Text::ScenesRestyled,
            Text::ScenesRestyledHint,
            Text::RedrawRestyled,
            Text::ScenesOldPortraits,
            Text::ScenesOldPortraitsHint,
            Text::RedrawOldPortraits,
            Text::SceneOldPortrait,
            Text::SceneCardOldPortrait,
        ]);
        texts.extend(TakeMode::ALL.map(Text::TakeModeName));
        texts.extend(AnalysisPart::ALL.map(Text::AnalysisPartName));
        texts.extend(Outbound::ALL.map(Text::ControlOutbound));
        texts.extend(crate::PerformedBlock::ALL.map(Text::PerformedBlocked));
        texts.extend([
            Text::PerformedMissingKlingKey,
            Text::PerformedTestFirst,
            Text::TakeCapReachedTitle,
            Text::TakeCapInvalid,
            Text::TakeSpendTitle,
            Text::TakeSpendHint,
            Text::TakeSpendPartial,
            Text::TakeSpendOver,
            Text::TakeCapLabel,
            Text::SaveTakeCap,
            Text::ResetTakeCap,
            Text::EditTakeCap,
            Text::TakeCapSaved,
            Text::TakeCapQuestion,
            Text::TakeCapConfirm,
            Text::TestTakeTitle,
            Text::TestTakeHint,
            Text::TestTakeNone,
            Text::TestTakeNeeded,
            Text::TestTakeApproved,
            Text::ApproveTestTake,
            Text::DiscardTestTake,
            Text::PlayTestTake,
            Text::MakeTestTakeAgain,
            Text::PreflightTitle,
            Text::PreflightHint,
            Text::PreflightFailedNote,
            Text::PreflightPassed,
            Text::PreflightReanalyze,
            Text::PreflightLookMeasuring,
            Text::PreflightLookNoFace,
            Text::SendTakeAnyway,
            Text::PreflightOverridden,
            Text::MakeTakeFinalSkipDraft,
            Text::TakeDraftOptionalNote,
            Text::MakeTestTake,
            Text::TestTakeReady,
            Text::TakeMakingTest,
            Text::TakeSpendLine,
            Text::TakeSpendSkipDraft,
            Text::TestTakeCost,
            Text::PreflightFramingFailed,
        ]);
        texts.extend(bardo_domain::PreflightCheck::ALL.map(Text::PreflightCheckName));
        texts.extend(bardo_domain::PreflightCheck::ALL.map(Text::PreflightCheckFailed));
        texts.extend(bardo_domain::BodyFraming::ALL.map(Text::BodyFramingName));
        texts.extend(
            [
                bardo_domain::TakePromptError::Empty,
                bardo_domain::TakePromptError::TooLong,
            ]
            .map(Text::TakePromptInvalid),
        );
        texts.push(Text::TransitionCut);
        texts.extend(MotionKind::ALL.map(Text::MotionName));
        texts.extend(MotionStrength::ALL.map(Text::MotionStrengthName));
        texts.push(Text::MotionNone);
        texts.push(Text::MotionVaried);
        texts.extend(NetworkAccountFieldError::ALL.map(Text::NetworkAccountFieldError));
        texts.extend(VoiceCategory::ALL.map(Text::VoiceCategoryName));
        texts.extend(CaptureProblem::ALL.map(Text::CaptureProblem));
        for flag in [VoiceFlag::Unchecked, VoiceFlag::Unavailable] {
            texts.push(Text::VoiceFlagTag(flag));
            texts.push(Text::VoiceFlagExplanation(flag));
            texts.push(Text::NarrationVoiceFlagged(flag));
        }
        texts.extend(ThemeFieldError::ALL.map(Text::ThemeFieldError));
        texts.extend(ScriptFieldError::ALL.map(Text::ScriptFieldError));
        texts.extend(MusicPromptFieldError::ALL.map(Text::MusicPromptFieldError));
        texts.push(Text::ThumbnailFieldError(ThumbnailFieldError::TextTooLong));
        texts.extend(ChapterFieldError::ALL.map(Text::ChapterFieldError));
        texts.extend(
            [
                ChapterProblem::TooFew,
                ChapterProblem::TooShort(0),
                ChapterProblem::CutAway(0),
                ChapterProblem::DoesNotFit,
            ]
            .map(Text::ChapterProblem),
        );
        texts.extend(TextPlacement::ALL.map(Text::TextPlacementName));
        texts.extend(ThumbnailLook::ALL.map(Text::ThumbnailLookName));
        texts.extend(SceneFieldError::ALL.map(Text::SceneFieldError));
        texts.extend([
            Text::EndCardTitle,
            Text::EndCardHint,
            Text::EndCardHeadline,
            Text::EndCardSecondLine,
            Text::EndCardSecondLineHint,
            Text::EndCardShowOffer,
            Text::EndCardOfferIs,
            Text::EndCardNoOffer,
            Text::EndCardOfferTooLong,
            Text::EndCardLook,
            Text::EndCardTextPlace,
            Text::EndCardLogoPlace,
            Text::EndCardNoLogo,
            Text::EndCardSave,
            Text::EndCardSaved,
            Text::EndCardOutOfDate,
            Text::EndCardOff,
        ]);
        texts.extend(CardTextPlace::ALL.map(Text::CardTextPlaceName));
        texts.extend(CardLogoPlace::ALL.map(Text::CardLogoPlaceName));
        texts.extend(CardFieldError::ALL.map(Text::CardFieldError));
        texts.extend([
            Text::AdItemSensitive,
            Text::AdItemUnbackedScript,
            Text::AdItemUnbackedPosts,
            Text::AdItemScriptNotChecked,
            Text::AdItemPostsNotChecked,
            Text::AdItemUnconfirmed,
            Text::AdItemCategory,
            Text::AdItemConcept,
            Text::AdItemsHeading,
            Text::AdNotAcknowledged,
            Text::AdReviewTitle,
            Text::AdReviewHint,
            Text::AdAcknowledgeAll,
            Text::AdAcknowledgeLeft,
            Text::CommercialTitle,
            Text::CommercialHint,
            Text::CommercialInStudio,
            Text::CommercialTikTokYourBrand,
            Text::CommercialTikTokBranded,
            Text::CommercialReminder,
            Text::CommercialReminderLabel,
            Text::CommercialReminderTikTokYourBrand,
            Text::CommercialReminderTikTokBranded,
            Text::CommercialReminderNoLabel,
            Text::CommercialReminderNothing,
            Text::CommercialChangeNote,
            Text::SponsorsLabel,
            Text::SponsorsHint,
            Text::StudioLink,
            Text::PostsCheckTitle,
            Text::PostsCheckHint,
            Text::PostsCheckRun,
            Text::PostsCheckRunning,
            Text::PostsCheckNone,
            Text::PostsCheckOutdated,
            Text::PostsCheckAllBacked,
            Text::PostsCheckUnbacked,
            Text::PostsCheckSaveFirst,
            Text::PostsCheckStopped,
        ]);
        texts.extend([
            Text::PlacementTitle,
            Text::PlacementHint,
            Text::PlacementShapeName,
            Text::PlacementTextsTitle,
            Text::PlacementTextsHint,
            Text::PlacementTextsNone,
            Text::PlacementPrimary,
            Text::PlacementHeadline,
            Text::PlacementCount,
            Text::PlacementTextsSave,
            Text::PlacementTextsReplaced,
            Text::PlacementSaveFirst,
            Text::PlacementPrimaryEmpty,
            Text::PlacementPrimaryTooLong,
            Text::PlacementHeadlineEmpty,
            Text::PlacementHeadlineTooLong,
            Text::PlacementWindowsTitle,
            Text::PlacementWindowsHint,
            Text::PlacementWholeWithBars,
            Text::PlacementScene,
            Text::PlacementFootage,
            Text::PlacementLeft,
            Text::PlacementRight,
            Text::PlacementUp,
            Text::PlacementDown,
            Text::PlacementCenter,
            Text::PlacementMoved,
            Text::PlacementStart,
            Text::PlacementRunning,
            Text::PlacementFailed,
            Text::PlacementCurrent,
            Text::PlacementOutdated,
            Text::PlacementNever,
            Text::PlacementShowFolder,
            Text::PlacementTextsNotReady,
            Text::PlacementNothingChosen,
            Text::PlacementPicturesWait,
            Text::PlacementAlreadyRunning,
            Text::PlacementFileVideo,
            Text::PlacementCall,
            Text::PlacementLink,
            Text::PlacementCommercialReminder,
            Text::PlacementSyntheticReminder,
        ]);
        texts.extend(CommercialDisclosure::ALL.map(Text::CommercialName));
        texts.extend(
            [Network::YouTube, Network::InstagramReels, Network::X].map(Text::CommercialSent),
        );
        texts.extend([SponsorError::NotAnId, SponsorError::TooMany].map(Text::SponsorError));
        texts.extend(TemplateKind::ALL.map(Text::TemplateKindName));
        texts.extend(TemplateVariable::ALL.map(Text::TemplateVariableHint));
        texts.extend(TemplateProblem::ALL.map(Text::TemplateProblem));
        texts.extend(
            [
                MoneyError::Required,
                MoneyError::Invalid,
                MoneyError::TooPrecise,
                MoneyError::TooLarge,
            ]
            .map(Text::MoneyError),
        );
        texts.extend(
            [
                RateFieldError::ModelTooLong,
                RateFieldError::ModelHasSpaces,
                RateFieldError::Price(MoneyError::Invalid),
                RateFieldError::NotPaid,
            ]
            .map(Text::RateFieldError),
        );
        texts.extend((1..=12).map(Text::MonthName));
        texts.extend(Meter::ALL.map(Text::MeterName));
        texts.extend(Meter::ALL.map(Text::MeterUnit));
        texts.push(Text::SettingsKeysTab);
        texts.push(Text::SettingsAppearanceTab);
        texts.push(Text::AppearanceTheme);
        texts.push(Text::AppearanceFollowSystem);
        texts.push(Text::AppearanceFollowSystemHint);
        texts.push(Text::AppearanceLight);
        texts.push(Text::AppearanceDark);
        texts.push(Text::AppearanceFixed);
        texts.push(Text::AppearanceFixedHint);
        texts.push(Text::AppearanceContrast);
        texts.push(Text::UiThemeNotSaved);
        texts.extend(UiTheme::ALL.map(Text::UiThemeName));
        texts.extend(UiTheme::ALL.map(Text::UiThemeKind));
        texts.push(Text::ProviderKeysInfo);
        texts.extend([
            Text::SettingsNetworksTab,
            Text::AppCredentialsTitle,
            Text::AppCredentialsHint,
            Text::AppCredentialsInfo,
            Text::AppCredentialsNotSet,
            Text::AppCredentialsSaved,
            Text::AppCredentialsUnreadable,
            Text::SaveAppCredentials,
            Text::ReplaceAppCredentials,
            Text::RemoveAppCredentials,
            Text::ConnectionNotConnectedLabel,
            Text::ConnectionConnected,
            Text::ConnectionReconnectNeeded,
            Text::Connect,
            Text::Reconnect,
            Text::CancelConnect,
            Text::Disconnect,
            Text::Disconnecting,
            Text::CheckConnection,
            Text::CheckingConnection,
            Text::OpenNetworkSettings,
            Text::ConnectionDisconnected,
            Text::ConnectionDisconnectedNotRevoked,
            Text::ConnectionDisconnectedKeptForOthers,
            Text::ConnectionDisconnectedCredentialsRefused,
            Text::ConnectionNotOffered,
            Text::ConnectionNotConnected,
            Text::ConnectionDenied,
            Text::ConnectionTimedOut,
            Text::ConnectionCancelled,
            Text::ConnectionListenFailed,
            Text::ConnectionPortInUse,
            Text::ConnectionTokensTooLarge,
            Text::ConnectionStoreFailed,
            Text::ConnectionNotSaved,
            Text::ConnectionNoPages,
            Text::ConnectionNoLinkedAccount,
            Text::ConnectionChoiceGone,
            Text::PastedTokenError(PastedTokenError::Required),
            Text::PastedTokenError(PastedTokenError::Invalid),
            Text::ConnectionUseAccount,
            Text::ConnectionChooseLabel,
            Text::NetworkAccountStillConnected,
            Text::ConsentPageDone,
            Text::ConsentPageFailed,
        ]);
        for network in Network::sign_in_networks() {
            texts.extend([
                Text::AppCredentialsName(network),
                Text::AppCredentialsPurpose(network),
                Text::ClientId(network),
                Text::ClientIdPlaceholder(network),
                Text::ClientSecret(network),
                Text::ClientSecretPlaceholder(network),
                Text::ConnectionConnecting(network),
                Text::ConnectionChecked(network),
                Text::ConnectionReconnectHint(network),
                Text::ConnectionNeedsAppCredentials(network),
                Text::ConnectionMissingScopes(network),
            ]);
            texts.extend(
                AppCredentialsFieldError::ALL
                    .map(|error| Text::AppCredentialsFieldError(network, error)),
            );
            texts.extend(
                [
                    SignInFailureKind::Refused,
                    SignInFailureKind::ClientRejected,
                    SignInFailureKind::NotAllowed,
                    SignInFailureKind::NoChannel,
                    SignInFailureKind::LimitReached,
                    SignInFailureKind::NetworkDown,
                    SignInFailureKind::Unreachable,
                    SignInFailureKind::Unexpected,
                ]
                .map(|kind| Text::SignInFailure(network, kind)),
            );
            if network.sign_in_method() == Some(bardo_domain::SignInMethod::PastedToken) {
                texts.extend([
                    Text::ConnectionTokenLabel(network),
                    Text::ConnectionTokenPlaceholder(network),
                    Text::ConnectionTokenHelp(network),
                    Text::ConnectionOpenTokenTool(network),
                    Text::ConnectionChoose(network),
                    Text::ConnectionChoiceVia(network),
                ]);
            }
        }
        texts.push(Text::Details);
        texts.extend([
            Text::PlansTitle,
            Text::PlansHint,
            Text::PlanNotConnected,
            Text::PlanConnected,
            Text::PlanConnectedPlain,
            Text::PlanReconnectNeeded,
            Text::PlanUnreadable,
            Text::PlanSignIn,
            Text::PlanSignInAgain,
            Text::PlanWaiting,
            Text::PlanCancel,
            Text::PlanCheck,
            Text::PlanChecking,
            Text::PlanSignOut,
            Text::PlanChecked,
            Text::PlanNotSignedIn,
            Text::PlanSignInAgainHint,
            Text::PlanNotOffered,
            Text::PlanDenied,
            Text::PlanTimedOut,
            Text::PlanCancelled,
            Text::PlanListenFailed,
            Text::PlanPortInUse,
            Text::PlanTokensTooLarge,
            Text::PlanStoreFailed,
            Text::ScenesMissingHiggsfieldPlan,
        ]);
        texts.extend(
            [
                SignInFailureKind::Refused,
                SignInFailureKind::ClientRejected,
                SignInFailureKind::NotAllowed,
                SignInFailureKind::LimitReached,
                SignInFailureKind::NetworkDown,
                SignInFailureKind::Unreachable,
                SignInFailureKind::Unexpected,
            ]
            .map(Text::PlanFailure),
        );
        texts.push(Text::NavBudgetsUsed);
        texts.extend([
            Text::AppearanceLayout,
            Text::AppearanceLayoutHint,
            Text::UiLayoutNotSaved,
            Text::StatusJobs,
            Text::StatusOneJob,
            Text::StatusNoJobs,
            Text::StatusMonthSpend,
            Text::SceneColumnPicture,
            Text::SceneColumnTime,
            Text::SceneColumnNarration,
            Text::SceneColumnPrompt,
            Text::SceneColumnImage,
            Text::SceneColumnClip,
            Text::SceneColumnModel,
            Text::SceneColumnCost,
            Text::SceneStateDone,
            Text::SceneKeysHint,
            Text::CostsBriefSpent,
            Text::CostsBriefOver,
            Text::CostsBriefNear,
            Text::CostsBriefUnpriced,
        ]);
        texts.extend(LayoutId::ALL.map(Text::UiLayoutName));
        texts.extend(LayoutId::ALL.map(Text::UiLayoutDescription));
        texts.push(Text::StageScriptMissing);
        texts.push(Text::StageScriptToReview);
        texts.push(Text::StageScriptWords);
        texts.push(Text::StageNarrationMissing);
        texts.push(Text::StageNarrationStale);
        texts.push(Text::StageScenesMissing);
        texts.push(Text::StageScenesStale);
        texts.push(Text::VersionsTitle);
        texts.push(Text::VersionsHint);
        texts.push(Text::VersionTargetLine);
        texts.push(Text::VersionTargetNoNarrator);
        texts.push(Text::MakeVersion);
        texts.push(Text::MakeVersionHint);
        texts.push(Text::VersionMaking);
        texts.push(Text::VersionStopped);
        texts.push(Text::VersionsMade);
        texts.push(Text::VersionMadeLine);
        texts.push(Text::OpenVersion);
        texts.push(Text::VersionOf);
        texts.push(Text::VersionOfChannel);
        texts.push(Text::VersionSourceGone);
        texts.push(Text::OpenVersionSource);
        texts.push(Text::VersionAdaptHint);
        texts.push(Text::VersionSameAudience);
        texts.push(Text::VersionAlreadyMade);
        texts.push(Text::VersionNoNarration);
        texts.push(Text::VersionNoPlan);
        texts.push(Text::VersionNoChannel);
        texts.push(Text::VersionBusy);
        texts.push(Text::VersionNotSaved);
        texts.push(Text::PlaceScenesTitle);
        texts.push(Text::PlaceScenesHint);
        texts.push(Text::PlaceScenes);
        texts.push(Text::PlaceScenesNothing);
        texts.push(Text::PlacedScenesTitle);
        texts.push(Text::PlacedScenesAll);
        texts.push(Text::PlacedScenePastClip);
        texts.push(Text::PlacedSceneStretched);
        texts.push(Text::PlacedSceneDropped);
        texts.push(Text::StageScenesToPlace);
        texts.push(Text::StageToReview);
        texts.push(Text::StageImages);
        texts.push(Text::StageClips);
        texts.push(Text::StageClipsNone);
        texts.push(Text::StageEditorReady);
        texts.push(Text::StageWorking);
        texts.push(Text::FilterAll);
        texts.push(Text::FilterPending);
        texts.push(Text::FilterPendingEmpty);
        texts.push(Text::SceneCardReview);
        texts.push(Text::SceneCardFailed);
        texts.push(Text::SceneCardToDraw);
        texts.push(Text::SceneCardToAnimate);
        texts.push(Text::SceneCardAnimating);
        texts.push(Text::SceneImagePrompt);
        texts.push(Text::SceneCast);
        texts.push(Text::SceneCastHint);
        texts.push(Text::CastPortraitLater);
        texts.push(Text::EditCastDescription);
        texts.push(Text::RedrawCastPortrait);
        texts.push(Text::CastPortraitFailed);
        texts.push(Text::CastEdited);
        texts.push(Text::CastDescribedOnly);
        texts.push(Text::CastDescribedOnlyHint);
        texts.push(Text::CastDrawWithoutPortrait);
        texts.push(Text::CastUsePortrait);
        texts.push(Text::SceneProps);
        texts.push(Text::ScenePropsHint);
        texts.push(Text::PropPictureLater);
        texts.push(Text::EditPropDescription);
        texts.push(Text::RedrawPropPicture);
        texts.push(Text::PropPictureFailed);
        texts.push(Text::PropDescribedOnly);
        texts.push(Text::PropDescribedOnlyHint);
        texts.push(Text::PropDrawWithoutPicture);
        texts.push(Text::PropUsePicture);
        texts.push(Text::PropSaveHint);
        texts.push(Text::ScenePlace);
        texts.push(Text::ScenePlaceHint);
        texts.push(Text::EditPlaceDescription);
        texts.push(Text::PlaceEdited);
        texts.push(Text::ScenePlaceLeft);
        texts.push(Text::SceneCurrentImage);
        texts.push(Text::SceneNewImage);
        texts.push(Text::ProjectSwitch);
        texts.push(Text::GenerationDetails);
        texts.push(Text::CostsOverview);
        texts.push(Text::CostsAcrossProviders);
        texts.push(Text::CostsBudgetsTile);
        texts.push(Text::CostsBudgetsInAlert);
        texts.push(Text::CostsNoBudgets);
        texts.push(Text::CostsUnpricedTitle);
        texts.push(Text::CostsUnpricedModel);
        texts.push(Text::CostsUnpricedModels);
        texts.push(Text::CostsUnpricedNone);
        texts.push(Text::AddPrice);
        texts.push(Text::CostsColumnProvider);
        texts.push(Text::CostsColumnUsage);
        texts.push(Text::CostsColumnSpent);
        texts.push(Text::CostsColumnBudget);
        texts.push(Text::CostsColumnState);
        texts.extend(NavSection::ALL.map(Text::NavSectionName));
        texts.extend(
            Destination::SECTIONS
                .iter()
                .flat_map(|(_, entries)| entries.iter())
                .map(|entry| Text::NavEntryName(*entry)),
        );
        texts.extend(Destination::ALL.map(Text::DestinationName));
        texts.extend([
            Text::ProductsHint,
            Text::ProductsEmpty,
            Text::ProductsNotLoaded,
            Text::NewProduct,
            Text::ProductsShowArchived,
            Text::NewProductTitle,
            Text::EditProductTitle,
            Text::ProductBriefHint,
            Text::ProductName,
            Text::ProductNamePlaceholder,
            Text::ProductKind,
            Text::ProductKindHint,
            Text::ProductOwnership,
            Text::ProductOwnershipHint,
            Text::ProductDescription,
            Text::ProductDescriptionPlaceholder,
            Text::ProductAudience,
            Text::ProductAudiencePlaceholder,
            Text::ProductFacts,
            Text::ProductFactsPlaceholder,
            Text::ProductFactsHint,
            Text::ProductFactsCount,
            Text::ProductOffer,
            Text::ProductOfferPlaceholder,
            Text::ProductLink,
            Text::ProductLinkPlaceholder,
            Text::ProductCallToAction,
            Text::ProductCallToActionPlaceholder,
            Text::ProductMustSay,
            Text::ProductMustSayPlaceholder,
            Text::ProductNeverSay,
            Text::ProductNeverSayPlaceholder,
            Text::CreateProduct,
            Text::SaveProduct,
            Text::DuplicateProduct,
            Text::ArchiveProduct,
            Text::RestoreProduct,
            Text::DeleteProduct,
            Text::ProductDeleteTitle,
            Text::ProductDeleteHint,
            Text::ProductActionsHint,
            Text::ConfirmDeleteProduct,
            Text::ProductCancel,
            Text::ProductSaved,
            Text::ProductDuplicated,
            Text::ProductArchived,
            Text::ProductRestored,
            Text::ProductDeleted,
            Text::ProductArchivedTag,
            Text::ProductCopySuffix,
            Text::ProductNameTaken,
            Text::ProductNotFound,
            Text::ProductInUse,
            Text::ProductNotSaved,
            Text::ProductPicturesTitle,
            Text::ProductPicturesHint,
            Text::ProductPicturesSaveFirst,
            Text::ProductPicturesNotLoaded,
            Text::ProductImagesNone,
            Text::ProductImageCount,
            Text::AddProductImage,
            Text::ProductImageKindHint,
            Text::ProductImageCutOut,
            Text::ProductImageSize,
            Text::RemoveProductImage,
            Text::ProductRightsConfirmedAt,
            Text::ProductRightsTitle,
            Text::ProductRightsHint,
            Text::ProductRightsConfirm,
            Text::ProductChooseImage,
            Text::ProductChooseLogo,
            Text::ProductImageDialog,
            Text::ProductLogoDialog,
            Text::ProductLogoTitle,
            Text::ProductLogoHint,
            Text::ProductLogoNone,
            Text::AddProductLogo,
            Text::ReplaceProductLogo,
            Text::RemoveProductLogo,
            Text::ProductImageRemoveAsk,
            Text::ProductLogoRemoveAsk,
            Text::ProductImageNotConfirmed,
            Text::ProductTooManyImages,
            Text::ProductImageRefused,
            Text::ProductImageUnreadable,
            Text::ProductImageNotFound,
            Text::ProductNoLogo,
            Text::ProductImageAdded,
            Text::ProductImageRemoved,
            Text::ProductLogoAdded,
            Text::ProductLogoRemoved,
            Text::SettingsFileGroupProducts,
        ]);
        texts.extend(ProductKind::ALL.map(Text::ProductKindName));
        texts.extend(ProductOwnership::ALL.map(Text::ProductOwnershipName));
        texts.extend(ProductImageKind::ALL.map(Text::ProductImageKindName));
        texts.push(Text::ProductImageKindName(ProductImageKind::Concept));
        texts.extend(ProductFieldError::ALL.map(Text::ProductFieldError));
        texts.extend(
            [
                ProductImageError::NotAnImage,
                ProductImageError::LogoNotPng,
                ProductImageError::TooSmall {
                    width: 1,
                    height: 1,
                },
                ProductImageError::TooLarge {
                    width: 1,
                    height: 1,
                },
                ProductImageError::Shape {
                    width: 1,
                    height: 1,
                },
                ProductImageError::FileTooLarge(1),
            ]
            .map(Text::ProductImageReason),
        );
        texts.extend([
            Text::ProductTabBrief,
            Text::ProductTabSheet,
            Text::SheetTitle,
            Text::SheetHint,
            Text::SheetInfo,
            Text::SheetSaveFirst,
            Text::SheetBriefUnsaved,
            Text::SheetNotLoaded,
            Text::SheetFillTitle,
            Text::SheetResearch,
            Text::SheetResearchAgain,
            Text::SheetResearchHint,
            Text::SheetResearchNoLink,
            Text::SheetResearchCost,
            Text::SheetResearchUsual,
            Text::SheetFromBrief,
            Text::SheetFromBriefHint,
            Text::SheetFromBriefCost,
            Text::SheetByHand,
            Text::SheetByHandHint,
            Text::SheetResearching,
            Text::SheetWriting,
            Text::SheetJobStopped,
            Text::SheetJobStopping,
            Text::SheetJobFailed,
            Text::SheetJobLost,
            Text::SheetJobDeclined,
            Text::SheetJobDone,
            Text::SheetJobLeftOut,
            Text::SheetFixFirst,
            Text::SheetBusyNote,
            Text::SheetRestrictedNotice,
            Text::SheetAboutTitle,
            Text::SheetSummary,
            Text::SheetCategory,
            Text::SheetRestricted,
            Text::SheetRestrictedHint,
            Text::SheetRestrictedNone,
            Text::SheetLooks,
            Text::SheetSegmentsTitle,
            Text::SheetSegmentName,
            Text::SheetSegmentWants,
            Text::SheetSegmentHoldsBack,
            Text::SheetAddSegment,
            Text::SheetObjectionsTitle,
            Text::SheetObjection,
            Text::SheetObjectionAnswer,
            Text::SheetAddObjection,
            Text::SheetAlternativesTitle,
            Text::SheetAlternativeName,
            Text::SheetAlternativeDiffers,
            Text::SheetAddAlternative,
            Text::SheetRemoveRow,
            Text::SheetRowsEmpty,
            Text::SaveSheet,
            Text::SheetSaved,
            Text::SheetClaimsTitle,
            Text::SheetClaimsHint,
            Text::SheetClaimsWaiting,
            Text::SheetClaimsConfirmed,
            Text::SheetClaimsRejected,
            Text::SheetClaimsNone,
            Text::ConfirmClaim,
            Text::RejectClaim,
            Text::BringClaimBack,
            Text::EditClaim,
            Text::SaveClaim,
            Text::CancelClaimEdit,
            Text::ClaimSensitive,
            Text::ClaimSensitiveHint,
            Text::ClaimConfirmedAt,
            Text::ClaimEditedAt,
            Text::ClaimOpenSource,
            Text::ClaimSourceTyped,
            Text::ClaimSourceBrief,
            Text::ClaimSourcePage,
            Text::ClaimSourceWeb,
            Text::AddClaim,
            Text::AddClaimPlaceholder,
            Text::SheetSourcesTitle,
            Text::SheetResearchedAt,
            Text::SheetSourcesNone,
            Text::SheetNone,
            Text::SheetMissingKey,
            Text::SheetBusy,
            Text::SheetNotSaved,
        ]);
        texts.extend(ClaimKind::ALL.map(Text::ClaimKindName));
        texts.extend(RestrictedCategory::ALL.map(Text::RestrictedCategoryName));
        texts.extend(SheetFieldError::ALL.map(Text::SheetFieldError));
        texts.extend(
            [
                SheetClaimError::Claim(ClaimError::TextRequired),
                SheetClaimError::Claim(ClaimError::TextTooLong),
                SheetClaimError::Claim(ClaimError::NotFound),
                SheetClaimError::Claim(ClaimError::AlreadyRejected),
                SheetClaimError::Claim(ClaimError::NotRejected),
                SheetClaimError::Known,
                SheetClaimError::TooMany,
                SheetClaimError::Gone,
            ]
            .map(Text::SheetClaimError),
        );
        texts.push(Text::AdAngleNotSaved);
        texts.push(Text::AdAngleNotFound);
        texts.push(Text::AdAnglesBusy);
        texts.push(Text::AdAnglesNothingToRank);
        texts.push(Text::AdAnglesNotLoaded);
        texts.extend(AdAngleFieldError::ALL.map(Text::AdAngleFieldError));
        texts.extend(AdAnglesMissing::ALL.map(Text::AdAnglesMissing));
        texts.push(Text::AdAnglesMissingKey(Provider::Claude));
        texts.push(Text::AdAnglesMissingKey(Provider::TypeSafe));
        texts.push(Text::StageBriefToConfirm);
        texts.push(Text::StageBriefClaimChanged);
        texts.push(Text::StageBriefClaimsChanged);
        texts.push(Text::StageBriefIncomplete);
        texts.push(Text::StageBriefClaim);
        texts.push(Text::StageBriefClaims);
        texts.push(Text::StageScriptBriefChanged);
        texts.push(Text::ScriptBriefFirst);
        texts.push(Text::VersionAdProject);
        texts.push(Text::BriefNotAnAd);
        texts.push(Text::BriefClaimNotConfirmed);
        texts.push(Text::BriefClaimNotUsed);
        texts.push(Text::BriefNotSaved);
        texts.push(Text::BriefNotLoaded);
        texts.extend(BriefNotReady::ALL.map(Text::BriefNotReady));
        texts.extend(BriefFieldError::ALL.map(Text::BriefFieldError));
        texts.push(Text::ProductTabAngles);
        texts.push(Text::AdAnglesTitle);
        texts.push(Text::AdAnglesInfo);
        texts.push(Text::AdAnglesHint);
        texts.push(Text::AdAnglesSaveFirst);
        texts.push(Text::AdAnglesBriefUnsaved);
        texts.push(Text::AdAnglesProposeTitle);
        texts.push(Text::AdAnglesChannel);
        texts.push(Text::AdAnglesChannelHint);
        texts.push(Text::SuggestAdAngles);
        texts.push(Text::SuggestAdAnglesHint);
        texts.push(Text::RankAdAngles);
        texts.push(Text::AdAnglesUnranked);
        texts.push(Text::AdAnglesRankCost);
        texts.push(Text::AdAnglesRunning);
        texts.push(Text::AdAnglesStopped);
        texts.push(Text::AdAnglesEmpty);
        texts.push(Text::AdAnglesDiscarded);
        texts.push(Text::AdAngleApproved);
        texts.push(Text::AdAngleNotRanked);
        texts.push(Text::AdAngleSensitive);
        texts.push(Text::AdAngleFramework);
        texts.push(Text::AdAngleSegment);
        texts.push(Text::AdAngleHook);
        texts.push(Text::AdAngleClaims);
        texts.push(Text::AdAngleClaimsNone);
        texts.push(Text::AdAngleClaimGone);
        texts.push(Text::AdAngleHookScore);
        texts.push(Text::AdAngleAudienceFit);
        texts.push(Text::AdAngleClarity);
        texts.push(Text::ApproveAdAngle);
        texts.push(Text::ApproveAdAngleHint);
        texts.push(Text::AdAngleEditHint);
        texts.push(Text::AdAngleStarted);
        texts.push(Text::AdAngleOpenProject);
        texts.push(Text::ProjectAd);
        texts.push(Text::ProjectAdFor);
        texts.push(Text::ScriptBriefChanged);
        texts.push(Text::ScriptKeepAfterBrief);
        texts.push(Text::BriefInfo);
        texts.push(Text::BriefHint);
        texts.push(Text::BriefAngleTitle);
        texts.push(Text::BriefProduct);
        texts.push(Text::BriefClaimsTitle);
        texts.push(Text::BriefClaimsInfo);
        texts.push(Text::BriefClaimsHint);
        texts.push(Text::BriefClaimsNone);
        texts.push(Text::BriefClaimsLeftOut);
        texts.push(Text::BriefClaimInAngle);
        texts.push(Text::BriefClaimLeaveOut);
        texts.push(Text::BriefClaimUse);
        texts.push(Text::BriefClaimEdited);
        texts.push(Text::BriefClaimUnconfirmed);
        texts.push(Text::BriefClaimKeep);
        texts.push(Text::BriefClaimTake);
        texts.push(Text::BriefWordsTitle);
        texts.push(Text::BriefWordsInfo);
        texts.push(Text::BriefLink);
        texts.push(Text::BriefSave);
        texts.push(Text::BriefSaved);
        texts.push(Text::BriefLengthHint);
        texts.push(Text::BriefConfirm);
        texts.push(Text::BriefConfirmHint);
        texts.push(Text::BriefConfirmed);
        texts.push(Text::BriefConfirmedNotice);
        texts.push(Text::BriefToScript);
        texts.push(Text::BriefAfterConfirm);
        texts.extend(AdFramework::ALL.map(Text::AdFrameworkName));
        texts.extend(AdFramework::ALL.map(Text::AdFrameworkSteps));
        texts.push(Text::ScriptBriefNotReady);
        texts.push(Text::GenerateAdScriptHint);
        texts.push(Text::ScriptClaimsRunning);
        texts.push(Text::ScriptClaimsStopped);
        texts.push(Text::ScriptClaimsTitle);
        texts.push(Text::ScriptClaimsInfo);
        texts.push(Text::ScriptClaimsNotChecked);
        texts.push(Text::ScriptClaimsClean);
        texts.push(Text::ScriptClaimsUnbacked);
        texts.push(Text::ScriptClaimsUnbackedOne);
        texts.push(Text::ScriptClaimsOutdated);
        texts.push(Text::ScriptClaimsOutdatedHint);
        texts.push(Text::ScriptClaimsUnbackedTitle);
        texts.push(Text::ScriptClaimsNoSentence);
        texts.push(Text::ScriptClaimsUsedTitle);
        texts.push(Text::ScriptClaimsSensitiveTitle);
        texts.push(Text::ScriptClaimsNoneUsed);
        texts.push(Text::CheckScriptClaims);
        texts.push(Text::CheckScriptClaimsAgain);
        texts.push(Text::ScriptClaimsCheckedAt);
        texts.push(Text::ScriptClaimsPendingMissing);
        texts.push(Text::NarrationClaimsUnbacked);
        texts.push(Text::NarrationClaimsUnbackedOne);
        texts.push(Text::NarrationClaimsNotChecked);
        texts.push(Text::ScriptClaimsSaveFirst);
        texts.push(Text::ScriptWrittenCheckStopped);
        texts.push(Text::ScriptClaimsPendingOutdatedHint);
        texts.push(Text::ScriptClaimsNothingStated);
        texts.push(Text::SceneKindNotAllowed);
        texts.push(Text::SceneKindLabel);
        texts.push(Text::SceneKindHint);
        texts.push(Text::SceneProductImage);
        texts.push(Text::SceneComposed);
        texts.push(Text::SceneImageTooSmall);
        texts.push(Text::ComposeScene);
        texts.push(Text::ComposeScenes);
        texts.push(Text::SceneNotComposed);
        texts.push(Text::SceneClipComposed);
        texts.push(Text::EditorMotionComposed);
        texts.extend(SceneKind::ALL.map(Text::SceneKindName));
        texts.push(Text::SceneCardToCompose);
        texts.push(Text::EditorSourceComposed);
        texts.push(Text::SceneImageRemoved);
        texts.push(Text::ShotBackdropGone);
        texts.push(Text::ShotBackdropInUse);
        texts.push(Text::ShotLayoutLabel);
        texts.push(Text::ShotLayoutHint);
        texts.push(Text::ShotPlacementLabel);
        texts.push(Text::ShotBackdropLabel);
        texts.push(Text::ShotBackdropHint);
        texts.push(Text::ShotBackdropAuto);
        texts.push(Text::ShotBackdropFrom);
        texts.push(Text::ShotBackdropTo);
        texts.push(Text::ShotBackdropDrawn);
        texts.push(Text::DrawShotBackdrop);
        texts.push(Text::DrawShotBackdropHint);
        texts.push(Text::DrawingShotBackdrop);
        texts.push(Text::ShotBackdropToReview);
        texts.push(Text::KeepShotBackdrop);
        texts.push(Text::DiscardShotBackdrop);
        texts.extend(ProductLayout::ALL.map(Text::ProductLayoutName));
        texts.extend(ShotPlacement::ALL.map(Text::ShotPlacementName));
        texts.extend(BackdropDirection::ALL.map(Text::BackdropDirectionName));
        texts.push(Text::RenderProductUnchecked);
        texts.push(Text::RenderEndCardOutOfDate);
        texts.push(Text::RenderProductUncheckedScenes);
        texts.push(Text::ProductCheckRefused);
        texts.push(Text::ProductUncheckedClip);
        texts.push(Text::ProductCheckTitle);
        texts.push(Text::ProductCheckHint);
        texts.push(Text::ProductMatches);
        texts.push(Text::ProductCheckedNote);
        texts.push(Text::ProductDrawAgain);
        texts.push(Text::ProductUseRealImage);
        texts.push(Text::ProductNotDrawn);
        texts.push(Text::ProductRealImage);
        texts.push(Text::ProductToCheckTag);
        texts.push(Text::ClipProductCheckTitle);
        texts.push(Text::ClipProductCheckHint);
        texts.push(Text::ClipProductMatches);
        texts.push(Text::ClipProductCheckedNote);
        texts.push(Text::ConceptTitle);
        texts.push(Text::ConceptHint);
        texts.push(Text::DrawConcept);
        texts.push(Text::DrawConceptAgain);
        texts.push(Text::ConceptDrawingNow);
        texts.push(Text::ConceptNewTitle);
        texts.push(Text::KeepConcept);
        texts.push(Text::DiscardConcept);
        texts.push(Text::ConceptKeptNote);
        texts.push(Text::ConceptUnusedNote);
        texts.push(Text::ConceptNotOffered);
        texts.push(Text::ConceptMissingClaudeKey);
        texts.push(Text::ConceptMissingImageKey);
        texts.push(Text::ConceptBusy);
        texts.push(Text::ConceptNothingWaiting);
        texts.push(Text::ConceptNotSaved);
        texts.push(Text::ConceptBadge);
        texts.push(Text::ConceptBadgeHint);
        texts.push(Text::ConceptStopped);
        texts.push(Text::ConceptFailed);
        texts.push(Text::ConceptKept);
        texts.push(Text::ConceptCheckTitle);
        texts.push(Text::ConceptCheckHint);
        texts.push(Text::ConceptUseImage);
        texts.extend(Stage::ALL.map(Text::StageName));
        texts.extend(Stage::ALL.map(Text::StageAfter));
        texts.push(Text::RenderNoCut);
        texts.push(Text::RenderChecking);
        texts.push(Text::RenderNothingChosen);
        texts.push(Text::RenderBlocked);
        texts.push(Text::RenderCutChanged);
        texts.push(Text::RenderAlreadyRunning);
        texts.push(Text::RenderWhileExporting);
        texts.push(Text::RenderWhileUploading);
        texts.push(Text::RenderCheckFailed);
        texts.push(Text::RenderNotLoaded);
        texts.push(Text::RenderNoAccounts);
        texts.push(Text::RenderInfo);
        texts.push(Text::RenderFigureLength);
        texts.push(Text::RenderFigureFrame);
        texts.push(Text::RenderFigureLoudness);
        texts.push(Text::RenderFigureCaptions);
        texts.push(Text::RenderCaptionsOn);
        texts.push(Text::RenderCaptionsOff);
        texts.push(Text::RenderMeasuring);
        texts.push(Text::RenderLufs);
        texts.push(Text::RenderSilent);
        texts.push(Text::RenderTargets);
        texts.push(Text::RenderChosen);
        texts.push(Text::RenderInclude);
        texts.push(Text::GateTooLong);
        texts.push(Text::GateNoEncoder);
        texts.push(Text::GateReframed);
        texts.push(Text::GateReframedWide);
        texts.push(Text::GateCaptionsOff);
        texts.push(Text::GateMissingMedia);
        texts.push(Text::GateSilent);
        texts.push(Text::GateLoudnessFar);
        texts.push(Text::GatePeaksLimited);
        texts.push(Text::GateLongForUnverified);
        texts.push(Text::GateBlocks);
        texts.push(Text::GateWarning);
        texts.push(Text::RenderStateReady);
        texts.push(Text::RenderStateBlocked);
        texts.push(Text::RenderStateWarnings);
        texts.push(Text::RenderStateChecking);
        texts.push(Text::RenderLastCurrent);
        texts.push(Text::RenderLastOutdated);
        texts.push(Text::RenderLastNone);
        texts.push(Text::RenderLastFile);
        texts.push(Text::RenderLastOutdatedHint);
        texts.push(Text::RenderLoudnessOnTarget);
        texts.push(Text::RenderLoudnessOffTarget);
        texts.push(Text::RenderShowFile);
        texts.push(Text::RenderSize);
        texts.push(Text::RenderColumnPreset);
        texts.push(Text::RenderColumnChecks);
        texts.push(Text::RenderColumnLast);
        texts.push(Text::RenderEncoder);
        texts.push(Text::RenderEncoderHardware);
        texts.push(Text::RenderEncoderSoftware);
        texts.push(Text::RenderStart);
        texts.push(Text::RenderCheckAgain);
        texts.push(Text::RenderConfirmTitle);
        texts.push(Text::RenderConfirmBody);
        texts.push(Text::RenderConfirmWarnings);
        texts.push(Text::RenderConfirm);
        texts.push(Text::RenderConfirmBack);
        texts.push(Text::RenderRunning);
        texts.push(Text::RenderStopped);
        texts.push(Text::RenderCancelled);
        texts.push(Text::RenderResume);
        texts.push(Text::StageRenderReady);
        texts.push(Text::StageRendered);
        texts.push(Text::StageRenderOutdated);
        texts.push(Text::StageRendering);
        texts.push(Text::StageRenderStopped);
        texts.push(Text::ExportNoAccounts);
        texts.push(Text::ExportNothingChosen);
        texts.push(Text::ExportBlocked);
        texts.push(Text::ExportAlreadyRunning);
        texts.push(Text::ExportWhileRendering);
        texts.push(Text::ExportNotLoaded);
        texts.push(Text::MetadataMissingKey);
        texts.push(Text::MetadataBusy);
        texts.push(Text::MetadataMissing);
        texts.push(Text::MetadataNotSaved);
        texts.push(Text::ExportFileVideo);
        texts.push(Text::ExportFileTitle);
        texts.push(Text::ExportFileDescription);
        texts.push(Text::ExportFileCaption);
        texts.push(Text::ExportFileTags);
        texts.push(Text::ExportFileVisibility);
        texts.push(Text::DisclosureReminder);
        texts.push(Text::DisclosureNotice);
        texts.push(Text::StageExportReady);
        texts.push(Text::StageExported);
        texts.push(Text::StageExportOutdated);
        texts.push(Text::StageExporting);
        texts.push(Text::StageExportStopped);
        texts.push(Text::PersonaRealisticVoice);
        texts.push(Text::PersonaRealisticVoiceHint);
        texts.push(Text::LookTitle);
        texts.push(Text::LookHint);
        texts.push(Text::LookSaveFirst);
        texts.push(Text::LookEmpty);
        texts.push(Text::LookDrawnTag);
        texts.push(Text::LookImportedTag);
        texts.push(Text::LookFromDescription);
        texts.push(Text::LookRemovePhotoConfirm);
        texts.push(Text::LookNotLoaded);
        texts.push(Text::LookDescription);
        texts.push(Text::LookDescriptionPlaceholder);
        texts.push(Text::LookDraw);
        texts.push(Text::LookDrawAgain);
        texts.push(Text::LookDrawHint);
        texts.push(Text::LookDrawing);
        texts.push(Text::LookDeclined);
        texts.push(Text::LookFailed);
        texts.push(Text::LookStopped);
        texts.push(Text::LookPendingTitle);
        texts.push(Text::LookPendingHint);
        texts.push(Text::LookKeep);
        texts.push(Text::LookDiscard);
        texts.push(Text::LookImport);
        texts.push(Text::LookImportTitle);
        texts.push(Text::LookImportHint);
        texts.push(Text::LookShowsMe);
        texts.push(Text::LookChoosePhoto);
        texts.push(Text::LookImportDialog);
        texts.push(Text::LookCancel);
        texts.push(Text::LookImported);
        texts.push(Text::LookKept);
        texts.push(Text::LookRemove);
        texts.push(Text::LookRemoveConfirm);
        texts.push(Text::LookRemoveYes);
        texts.push(Text::LookRemoved);
        texts.push(Text::LookRealistic);
        texts.push(Text::LookRealisticHint);
        texts.push(Text::PersonaExportedNoLook);
        texts.push(Text::LookMissingClaudeKey);
        texts.push(Text::LookMissingImageKey);
        texts.push(Text::LookBusy);
        texts.push(Text::LookNothingWaiting);
        texts.push(Text::LookNone);
        texts.push(Text::LookNotConfirmed);
        texts.push(Text::LookUnreadable);
        texts.push(Text::LookImageRefused);
        texts.push(Text::LookNotSaved);
        texts.extend([
            Text::LookAssetTitle,
            Text::LookAssetHint,
            Text::LookAssetKindLabel,
            Text::LookAssetIdLabel,
            Text::LookAssetIdPlaceholder,
            Text::LookAssetSave,
            Text::LookAssetRemove,
            Text::LookAssetHeld,
            Text::LookAssetSaved,
            Text::LookAssetRemoved,
        ]);
        texts.extend(LookAssetKind::ALL.map(Text::LookAssetKindName));
        texts.extend(LookAssetError::ALL.map(Text::LookAssetError));
        texts.extend(BytePlusField::ALL.map(Text::BytePlusFieldLabel));
        texts.extend(BytePlusFieldError::all().map(Text::BytePlusFieldError));
        texts.extend([
            Text::BytePlusFieldsInvalid,
            Text::BytePlusCardNote,
            Text::PerformedMissingBytePlusKey,
            Text::PerformedDraftExpired,
            Text::PerformedDraftFinished,
            Text::TakeDraftDeadline,
            Text::TakeDraftFinishNote,
        ]);
        texts.extend(
            [
                LookDescriptionError::Required,
                LookDescriptionError::TooLong,
            ]
            .map(Text::LookDescriptionError),
        );
        texts.extend(
            [
                LookImageError::NotAnImage,
                LookImageError::TooSmall {
                    width: 1,
                    height: 1,
                },
                LookImageError::TooLarge {
                    width: 1,
                    height: 1,
                },
                LookImageError::Shape {
                    width: 1,
                    height: 1,
                },
                LookImageError::FileTooLarge(1),
            ]
            .map(Text::LookImageReason),
        );
        texts.push(Text::ExportInfo);
        texts.push(Text::ExportFigureRendered);
        texts.push(Text::ExportFigureMetadata);
        texts.push(Text::ExportFigureExported);
        texts.push(Text::ExportFigureCost);
        texts.push(Text::ExportFigureNetworks);
        texts.push(Text::ExportChosen);
        texts.push(Text::ExportTargets);
        texts.push(Text::ExportInclude);
        texts.push(Text::ExportStart);
        texts.push(Text::ExportRunning);
        texts.push(Text::ExportStopped);
        texts.push(Text::ExportCancelled);
        texts.push(Text::ExportShowFolder);
        texts.push(Text::ExportColumnRender);
        texts.push(Text::ExportColumnLast);
        texts.push(Text::ExportRenderOutdatedHint);
        texts.push(Text::ExportNoRenderHint);
        texts.push(Text::ExportStateReady);
        texts.push(Text::ExportStateNoRender);
        texts.push(Text::ExportStateNoMetadata);
        texts.push(Text::ExportStateProblems);
        texts.push(Text::ExportLastCurrent);
        texts.push(Text::ExportLastOutdated);
        texts.push(Text::ExportLastNone);
        texts.push(Text::ExportLastOutdatedHint);
        texts.push(Text::MetadataGenerate);
        texts.push(Text::MetadataRegenerate);
        texts.push(Text::MetadataGenerateHint);
        texts.push(Text::MetadataRegenerateHint);
        texts.push(Text::MetadataRunning);
        texts.push(Text::MetadataStopped);
        texts.push(Text::MetadataConfirmTitle);
        texts.push(Text::MetadataConfirmBody);
        texts.push(Text::MetadataNone);
        texts.push(Text::MetadataEmpty);
        texts.push(Text::MetadataStateNone);
        texts.push(Text::MetadataStateGenerated);
        texts.push(Text::MetadataStateEdited);
        texts.push(Text::MetadataEdited);
        texts.push(Text::MetadataFieldTitle);
        texts.push(Text::MetadataFieldDescription);
        texts.push(Text::MetadataFieldCaption);
        texts.push(Text::MetadataFieldTags);
        texts.push(Text::MetadataTagsPlaceholder);
        texts.push(Text::MetadataCounter);
        texts.push(Text::MetadataTagsCount);
        texts.push(Text::MetadataTagsCountOf);
        texts.push(Text::MetadataFooterNote);
        texts.push(Text::MetadataHashtagsNote);
        texts.push(Text::MetadataSave);
        texts.push(Text::MetadataRevert);
        texts.push(Text::MetadataSaved);
        texts.push(Text::MetadataCopy);
        texts.push(Text::MetadataCopied);
        texts.push(Text::MetadataPreview);
        texts.push(Text::MetadataCostUnpriced);
        texts.extend(Network::ALL.map(Text::DisclosureHow));
        texts.extend(MetadataProblem::ALL.map(Text::MetadataProblem));
        texts.push(Text::PublicationNoAccount);
        texts.push(Text::PublicationNotExported);
        texts.push(Text::PublicationAlreadyLinked);
        texts.push(Text::PublicationNotFound);
        texts.push(Text::PublicationNotSaved);
        texts.push(Text::MetricsMissingKey);
        texts.push(Text::MetricsNothingToSync);
        texts.push(Text::MetricsAlreadySyncing);
        texts.push(Text::MetricsNotLoaded);
        texts.push(Text::PublicationTitle);
        texts.push(Text::PublicationNeedsExport);
        texts.push(Text::PublicationLinkPlaceholder);
        texts.push(Text::PublicationMark);
        texts.push(Text::PublicationMarkHint);
        texts.push(Text::PublicationPosted);
        texts.push(Text::PublicationMissing);
        texts.push(Text::PublicationMissingHint);
        texts.push(Text::PublicationOpen);
        texts.push(Text::PublicationChange);
        texts.push(Text::PublicationSave);
        texts.push(Text::PublicationCancel);
        texts.push(Text::PublicationRemove);
        texts.push(Text::PublicationRemoveConfirm);
        texts.push(Text::PublicationRemoveKeep);
        texts.push(Text::PublicationSaved);
        texts.push(Text::PublicationRemoved);
        texts.push(Text::PublicationPostedAt);
        texts.push(Text::PublicationNoMetrics);
        texts.push(Text::PublicationConnectForMetrics);
        texts.push(Text::PublicationReconnectForMetrics);
        texts.push(Text::PublicationFigure);
        texts.push(Text::PublicationTileViews);
        texts.push(Text::PublicationTileEngaged);
        texts.push(Text::MetricViews);
        texts.push(Text::MetricLikes);
        texts.push(Text::MetricComments);
        texts.push(Text::MetricHidden);
        texts.push(Text::MetricHiddenHint);
        texts.push(Text::MetricsSyncedAgo);
        texts.push(Text::MetricsNotSynced);
        texts.push(Text::MetricsSyncNow);
        texts.push(Text::MetricsSyncing);
        texts.push(Text::MetricsSyncStopped);
        texts.push(Text::MetricsSyncHint);
        texts.push(Text::MetricsHistory);
        texts.push(Text::MetricsChange);
        texts.push(Text::MetricsViewsChange);
        texts.push(Text::MetricEngagedViews);
        texts.push(Text::MetricEngagedViewsHint);
        texts.push(Text::MetricWatchTime);
        texts.push(Text::MetricAverageView);
        texts.push(Text::MetricAverageViewed);
        texts.push(Text::MetricRevenue);
        texts.push(Text::MetricRpm);
        texts.push(Text::MetricRpmHint);
        texts.push(Text::MetricCpm);
        texts.push(Text::MetricCpmHint);
        texts.push(Text::MetricPlaybackCpm);
        texts.push(Text::MetricPlaybackCpmHint);
        texts.push(Text::MetricNotMonetized);
        texts.push(Text::MetricNotMonetizedHint);
        texts.push(Text::MetricsOwnerLine);
        texts.push(Text::MetricsRetention);
        texts.push(Text::MetricsRetentionHint);
        texts.push(Text::MetricsRetentionStart);
        texts.push(Text::MetricsRetentionEnd);
        texts.push(Text::RetentionTitle);
        texts.push(Text::RetentionNone);
        texts.push(Text::RetentionUnavailable);
        texts.push(Text::RetentionRead);
        texts.push(Text::RetentionDrops);
        texts.push(Text::RetentionDropsHint);
        texts.push(Text::RetentionNoDrops);
        texts.push(Text::RetentionChanged);
        texts.push(Text::RetentionNoLength);
        texts.push(Text::RetentionLost);
        texts.push(Text::RetentionAtShare);
        texts.push(Text::RetentionScene);
        texts.push(Text::RetentionFootage);
        texts.push(Text::RetentionNoPicture);
        texts.push(Text::RetentionNoSpeech);
        texts.push(Text::RetentionShowInEditor);
        texts.push(Text::MetricsHours);
        texts.push(Text::MetricsMinutes);
        texts.push(Text::MetricsPercent);
        texts.push(Text::MetricShares);
        texts.push(Text::MetricSaves);
        texts.push(Text::MetricReach);
        texts.push(Text::MetricReachHint);
        texts.push(Text::MetricInteractions);
        texts.push(Text::MetricInteractionsHint);
        texts.push(Text::MetricAverageWatch);
        texts.push(Text::MetricNotReportedHint);
        texts.push(Text::MetricsInstagramLine);
        texts.push(Text::MetricsTikTokLine);
        texts.push(Text::MetricsInsightsEmpty);
        texts.push(Text::MetricsInsightsPending);
        texts.push(Text::PerformanceOwnerNotConnected);
        texts.push(Text::PerformanceOwnerReconnect);
        texts.push(Text::PerformanceTileEngaged);
        texts.push(Text::PerformanceInfo);
        texts.push(Text::PerformanceEmpty);
        texts.push(Text::PerformanceNoChannels);
        texts.push(Text::PerformanceHistoryTitle);
        texts.push(Text::PerformanceHistoryEmpty);
        texts.push(Text::PerformancePosts);
        texts.push(Text::PerformanceTracked);
        texts.push(Text::PerformanceTile);
        texts.push(Text::DecisionEngineLabel);
        texts.push(Text::DecisionEngineHint);
        texts.push(Text::DecisionEngineCaption);
        texts.push(Text::DecisionEngineNotSaved);
        texts.extend(DecisionEngineId::ALL.map(Text::DecisionEngineName));
        texts.push(Text::MetricsSettingsTab);
        texts.push(Text::MetricsSettingLabel);
        texts.push(Text::MetricsSettingHint);
        texts.push(Text::MetricsSettingNotSaved);
        texts.extend(
            [
                PostLinkError::Empty,
                PostLinkError::NotALink,
                PostLinkError::OtherSite(Some(Network::TikTok)),
                PostLinkError::OtherSite(None),
                PostLinkError::ShortLink,
                PostLinkError::NotAPost,
            ]
            .map(Text::PostLinkProblem),
        );
        texts.extend(MetricsSyncOnStart::ALL.map(Text::MetricsSyncOption));
        texts.push(Text::UploadChanged);
        texts.push(Text::UploadReplaceNotConfirmed);
        texts.push(Text::UploadNotStarted);
        texts.push(Text::PublicationReplacesUpload);
        texts.push(Text::PublicationUploading);
        texts.push(Text::UploadTitle);
        texts.push(Text::UploadHint);
        texts.push(Text::UploadOpenReview);
        texts.push(Text::UploadReviewTitle);
        texts.push(Text::UploadFieldFile);
        texts.push(Text::UploadFieldChannel);
        texts.push(Text::UploadFieldVisibility);
        texts.push(Text::UploadMadeForKids);
        texts.push(Text::UploadMadeForKidsHint);
        texts.push(Text::UploadSynthetic);
        texts.push(Text::UploadSyntheticHint);
        texts.push(Text::UploadSyntheticOn);
        texts.push(Text::UploadReplacePost);
        texts.push(Text::UploadReplaceUpload);
        texts.push(Text::UploadIrreversible);
        texts.push(Text::UploadStart);
        texts.push(Text::UploadBack);
        texts.push(Text::UploadQueued);
        texts.push(Text::UploadStateWaiting);
        texts.push(Text::UploadStateUploading);
        texts.push(Text::UploadStateRetrying);
        texts.push(Text::UploadStateProcessing);
        texts.push(Text::UploadStateStillProcessing);
        texts.push(Text::UploadStatePublished);
        texts.push(Text::UploadStateRestricted);
        texts.push(Text::UploadStateStopped);
        texts.push(Text::UploadStateFailed);
        texts.push(Text::UploadRetryingHint);
        texts.push(Text::UploadProcessingHint);
        texts.push(Text::UploadStillProcessingHint);
        texts.push(Text::UploadStoppedHint);
        texts.push(Text::UploadRestrictedHint);
        texts.push(Text::UploadRestrictedCauses);
        texts.push(Text::UploadFailureQuota);
        texts.push(Text::UploadFailureCredits);
        texts.push(Text::UploadFailureAnswerLost);
        texts.push(Text::UploadFailureUploadLimit);
        texts.push(Text::UploadFailureReconnect);
        texts.push(Text::UploadFailureRejected);
        texts.push(Text::UploadFailureProcessing);
        texts.push(Text::UploadFailureRemoved);
        texts.push(Text::UploadFailureAccountChanged);
        texts.push(Text::UploadFailureRenderChanged);
        texts.push(Text::UploadStop);
        texts.push(Text::UploadResume);
        texts.push(Text::UploadRetry);
        texts.push(Text::UploadCheckAgain);
        texts.push(Text::UploadSentAt);
        texts.push(Text::PublicationReplaceUploadConfirm);
        texts.push(Text::PublicationReplaceUploadYes);
        texts.extend(UploadBlock::ALL.map(Text::UploadBlocked));
        texts.push(Text::UploadFieldWhen);
        texts.push(Text::UploadWhenNow);
        texts.push(Text::UploadWhenSchedule);
        texts.push(Text::UploadStartScheduled);
        texts.push(Text::UploadStateScheduled);
        texts.push(Text::UploadScheduledAt);
        texts.push(Text::UploadOverLimitAt);
        texts.push(Text::UploadOverLimitRetryAt);
        texts.push(Text::UploadScheduledHint);
        texts.push(Text::UploadRestrictedScheduledHint);
        texts.push(Text::UploadFailureScheduleMissed);
        texts.extend([
            Text::UploadReelHint,
            Text::UploadPostHint,
            Text::UploadFieldAccount,
            Text::UploadFieldCaption,
            Text::UploadFieldCover,
            Text::UploadCoverHint,
            Text::UploadCoverInvalid,
            Text::UploadCoverPastEnd,
            Text::UploadShareToFeed,
            Text::UploadShareToFeedHint,
            Text::UploadAiLabel,
            Text::UploadAiLabelHint,
            Text::UploadPostAiLabel,
            Text::UploadPostAiLabelHint,
            Text::UploadReelIrreversible,
            Text::UploadPostIrreversible,
            Text::UploadReelStart,
            Text::UploadPostStart,
            Text::UploadPostCost,
            Text::UploadPostCostHint,
            Text::UploadReelProcessingHint,
            Text::UploadPostProcessingHint,
            Text::UploadStateOverLimit,
            Text::UploadOverLimitHint,
            Text::UploadOverLimitSoonHint,
            Text::UploadOverLimitRecheckHint,
            Text::UploadIssue,
            Text::UploadSpecsTitle,
        ]);
        texts.extend(bardo_domain::ReelSpecProblem::CODES.map(Text::UploadSpec));
        texts.push(Text::ScheduleDate);
        texts.push(Text::ScheduleTime);
        texts.push(Text::ScheduleDatePlaceholder);
        texts.push(Text::ScheduleTimePlaceholder);
        texts.push(Text::ScheduleZone);
        texts.push(Text::ScheduleHint);
        texts.push(Text::ScheduleChange);
        texts.push(Text::ScheduleCancel);
        texts.push(Text::ScheduleSave);
        texts.push(Text::ScheduleCancelConfirm);
        texts.push(Text::ScheduleCancelYes);
        texts.push(Text::ScheduleKeep);
        texts.push(Text::ScheduleWorking);
        texts.push(Text::ScheduleChanged);
        texts.push(Text::ScheduleCancelled);
        texts.push(Text::ScheduleAlreadyLive);
        texts.push(Text::ScheduleNotScheduled);
        texts.push(Text::ScheduleFailed);
        texts.push(Text::ScheduleNotAllowed);
        texts.push(Text::JobWaitsUntil);
        texts.push(Text::UploadStateDue);
        texts.push(Text::UploadStateMissed);
        texts.push(Text::UploadDueAt);
        texts.push(Text::UploadDueHint);
        texts.push(Text::UploadMissedAt);
        texts.push(Text::UploadMissedHint);
        texts.push(Text::UploadReelScheduleHint);
        texts.extend([
            Text::UploadDueHintAgent,
            Text::UploadReelScheduleHintAgent,
            Text::UploadMissedHintAgent,
            Text::JobRunsElsewhere,
            Text::JobSuperseded,
            Text::JobRestyled,
            Text::JobScenesMovedOn,
            Text::JobScenesTaken,
            Text::JobTakeFromItsPage,
            Text::JobNarrationBusy,
            Text::JobScenesBusy,
            Text::JobSameWork,
            Text::JobAsksFirst,
            Text::JobOverBudget,
            Text::SettingsPublishingTab,
            Text::AgentTitle,
            Text::AgentQuestion,
            Text::AgentExplanation,
            Text::AgentSwitch,
            Text::AgentSends,
            Text::AgentLimits,
            Text::AgentHint,
            Text::AgentStatusOff,
            Text::AgentStatusRunning,
            Text::AgentStatusNotRunning,
            Text::AgentStartNow,
            Text::AgentWorking,
            Text::AgentNotSetUp,
            Text::AgentNotRemoved,
            Text::AgentNotSaved,
            Text::SettingsAboutTab,
            Text::AboutTitle,
            Text::AboutVersion,
            Text::AboutSummary,
            Text::AboutFfmpegTitle,
            Text::AboutFfmpeg,
            Text::AboutFfmpegSource,
            Text::AboutFfmpegLicense,
            Text::AboutNoticesTitle,
            Text::AboutNotices,
            Text::AboutOpenNotices,
            Text::AboutNoNotices,
            Text::AboutDiagnosticsTitle,
            Text::AboutDiagnostics,
            Text::AboutExportDiagnostics,
            Text::AboutExportingDiagnostics,
            Text::AboutOpenLogs,
            Text::AboutDiagnosticsSaved,
            Text::AboutShowDiagnostics,
            Text::AboutDiagnosticsFailed,
            Text::PrivacyNotAccepted,
            Text::PrivacyTitle,
            Text::PrivacySummary,
            Text::PrivacyAcceptedOn,
            Text::PrivacyNotYet,
            Text::PrivacyChanged,
            Text::PrivacyRead,
            Text::PrivacyOnline,
            Text::PrivacyAccept,
            Text::PrivacyConsent,
            Text::PrivacyUsesYouTube,
            Text::YouTubeTerms,
            Text::GooglePrivacyPolicy,
            Text::GooglePermissions,
            Text::DeleteYouTubeData,
            Text::DeleteYouTubeDataConfirm,
            Text::DeleteYouTubeDataNow,
            Text::DeleteYouTubeDataCancel,
            Text::YouTubeDataDeleted,
            Text::YouTubeDataNotDeleted,
            Text::PrivacyBanner,
            Text::PrivacyBannerReview,
            Text::UpdatesTitle,
            Text::UpdatesSummary,
            Text::UpdatesSwitch,
            Text::UpdatesCheckNow,
            Text::UpdatesChecking,
            Text::UpdatesNever,
            Text::UpdatesUpToDate,
            Text::UpdatesNoRelease,
            Text::UpdatesFoundNewer,
            Text::UpdatesOffline,
            Text::UpdatesRateLimited,
            Text::UpdatesRateLimitedLater,
            Text::UpdatesFailed,
            Text::UpdatesStore,
            Text::UpdatesSource,
            Text::UpdateAvailable,
            Text::UpdatePublished,
            Text::UpdateWhatsNew,
            Text::UpdateReleasePage,
            Text::UpdateDownload,
            Text::UpdateDownloading,
            Text::UpdateCancel,
            Text::UpdateDownloadFailed,
            Text::UpdateDownloadAgain,
            Text::UpdateReady,
            Text::UpdateInstall,
            Text::UpdateReviewTitle,
            Text::UpdateReviewBody,
            Text::UpdateReviewAgent,
            Text::UpdateInstallAndRestart,
            Text::UpdateStarting,
            Text::UpdateNoInstaller,
            Text::UpdateAlreadyDownloading,
            Text::UpdateFileGone,
            Text::UpdateChecksumChanged,
            Text::UpdateNotStarted,
            Text::UpdateWaitsForJobs,
            Text::UpdateWaitsForRestore,
            Text::UpdateOtherWindow,
            Text::UpdateBannerSee,
            Text::UpdateBannerLater,
            Text::UpdateNotesOpenAbout,
            Text::UpdateNotesClose,
            Text::UpdateDownloadUnreachable,
            Text::UpdateWaitingToRetry,
            Text::UpdateSwitchNotSaved,
            Text::UpdateNoneAvailable,
            Text::SettingsBackupTab,
            Text::BackupTitle,
            Text::BackupSummary,
            Text::BackupIncludeMedia,
            Text::BackupMediaHint,
            Text::BackupMediaSize,
            Text::BackupMediaMeasuring,
            Text::BackupMediaNone,
            Text::BackupMediaUnknown,
            Text::BackupBackUp,
            Text::BackupBackingUp,
            Text::BackupSaved,
            Text::BackupShow,
            Text::BackupFailed,
            Text::BackupRunning,
            Text::BackupCancel,
            Text::BackupAutomaticTitle,
            Text::BackupAutomaticSwitch,
            Text::BackupAutomaticHint,
            Text::BackupAutomaticNone,
            Text::BackupAutomaticRow,
            Text::BackupOpenFolder,
            Text::BackupRestoreTitle,
            Text::BackupRestoreSummary,
            Text::SettingsFileNotOne,
            Text::SettingsFileNewer,
            Text::SettingsFileWrongPassword,
            Text::SettingsFileNotSaved,
            Text::SettingsFilePasswordShort,
            Text::SettingsFilePasswordsDiffer,
            Text::SettingsFileNothingTicked,
            Text::SettingsFileUnreadable,
            Text::SettingsFilePreferences,
            Text::SettingsFileCosts,
            Text::SettingsFileSignInLabel,
            Text::SettingsFileItemFailed,
            Text::SettingsFileSomeNotRead,
            Text::SettingsFileItemNotRead,
            Text::SettingsFileTitle,
            Text::SettingsFileSummary,
            Text::SettingsFileExport,
            Text::SettingsFileImport,
            Text::SettingsFileExportTitle,
            Text::SettingsFileGroupKeys,
            Text::SettingsFileGroupApps,
            Text::SettingsFileGroupPreferences,
            Text::SettingsFileGroupPersonas,
            Text::SettingsFileGroupChannels,
            Text::SettingsFileGroupSignIns,
            Text::SettingsFileSignInsHint,
            Text::SettingsFilePassword,
            Text::SettingsFilePasswordAgain,
            Text::SettingsFilePasswordHint,
            Text::SettingsFileSave,
            Text::SettingsFileSaved,
            Text::SettingsFileMade,
            Text::SettingsFileOpen,
            Text::SettingsFileImportTitle,
            Text::SettingsFileAdds,
            Text::SettingsFileReplaces,
            Text::SettingsFileModelNotOffered,
            Text::SettingsFilePartsNotRead,
            Text::SettingsFileReplaceNote,
            Text::SettingsFileBringIn,
            Text::SettingsFileBrought,
            Text::SettingsFileNotBrought,
            Text::SettingsOfferFirst,
            Text::SettingsOfferFile,
            Text::SettingsOfferOpen,
            Text::SettingsOfferNotNow,
            Text::DevCopyChip,
            Text::AboutDevCopy,
            Text::BackupRestoreFromFile,
            Text::BackupRestore,
            Text::BackupChecking,
            Text::BackupReviewTitle,
            Text::BackupReviewMade,
            Text::BackupReviewWithMedia,
            Text::BackupReviewWithoutMedia,
            Text::BackupReviewWarning,
            Text::BackupReviewPosts,
            Text::BackupRestoreAndRestart,
            Text::BackupRestoring,
            Text::BackupWaitingForAgent,
            Text::BackupRestarting,
            Text::BackupRestoreFailed,
            Text::RestoreWaitsForJobs,
            Text::RestoreOtherWindow,
            Text::BackupKeptAside,
            Text::RestoreRunning,
            Text::BackupNotABackup,
            Text::BackupDamaged,
            Text::BackupNewer,
            Text::BackupUnreadable,
            Text::BackupRestoredTitle,
            Text::BackupRestored,
            Text::BackupRolledBack,
            Text::BackupSetUpAgain,
            Text::BackupSetUpKey,
            Text::BackupSetUpPlan,
            Text::BackupSetUpCredentials,
            Text::BackupSetUpReconnect,
            Text::BackupSetUpOpen,
            Text::BackupNothingToSetUp,
            Text::BackupDone,
        ]);
        texts.push(Text::UploadReelStartScheduled);
        texts.extend(bardo_domain::TikTokSpecProblem::CODES.map(Text::UploadTikTokSpec));
        texts.push(Text::UploadTikTokSpecsTitle);
        texts.push(Text::UploadDraftHint);
        texts.push(Text::UploadDraftNotice);
        texts.push(Text::UploadFieldDraftCaption);
        texts.push(Text::UploadFieldPostText);
        texts.push(Text::UploadDraftAiLabel);
        texts.push(Text::UploadDraftAiLabelHint);
        texts.push(Text::UploadDraftIrreversible);
        texts.push(Text::UploadDraftStart);
        texts.push(Text::UploadDraftProcessingHint);
        texts.push(Text::UploadStateDraftSent);
        texts.push(Text::UploadDraftSentHint);
        texts.push(Text::UploadDraftAiReminder);
        texts.push(Text::UploadDraftLinkHint);
        texts.push(Text::UploadDraftCopy);
        texts.push(Text::UploadDraftCopied);
        texts.push(Text::UploadDraftLimitHint);
        texts.push(Text::UploadDraftLimitHeldHint);
        texts.push(Text::UploadDraftLimitRecheckHint);
        texts.push(Text::MissedTitle);
        texts.push(Text::MissedHint);
        texts.push(Text::MissedDue);
        texts.push(Text::MissedSendNow);
        texts.push(Text::MissedNewTime);
        texts.push(Text::MissedSave);
        texts.push(Text::MissedCancel);
        texts.push(Text::MissedCancelConfirm);
        texts.push(Text::MissedCancelYes);
        texts.push(Text::MissedKeep);
        texts.push(Text::MissedLater);
        texts.push(Text::MissedLaterHint);
        texts.push(Text::MissedSent);
        texts.push(Text::MissedRescheduled);
        texts.push(Text::MissedCancelled);
        texts.push(Text::MissedNotMissed);
        texts.push(Text::MissedNotUpdated);
        texts.push(Text::DateTimeFormat);
        texts.push(Text::DateFormat);
        texts.push(Text::TimeFormat);
        texts.push(Text::TimeAm);
        texts.push(Text::TimePm);
        texts.push(Text::DateTimeWithZone);
        texts.push(Text::ZoneWithOffset);
        texts.extend((1..=7).map(Text::WeekdayName));
        texts.extend(ScheduleProblem::ALL.map(Text::ScheduleProblem));
        for tour in crate::Tour::ALL {
            texts.push(Text::TourName(tour.id));
            for step in tour.steps {
                texts.push(Text::TourStepTitle(tour.id, step.key));
                texts.push(Text::TourStepBody(tour.id, step.key));
            }
        }
        for group in crate::SHORTCUTS {
            texts.push(group.name);
            texts.extend(group.shortcuts.iter().map(|shortcut| shortcut.action));
        }
        texts.extend([
            Text::TourStepCount,
            Text::TourEscHint,
            Text::TourBack,
            Text::TourNext,
            Text::TourFinish,
            Text::TourSkip,
            Text::TourNew,
            Text::TourOfferTitle,
            Text::TourOfferBody,
            Text::TourOfferStart,
            Text::TourOfferNotNow,
            Text::TourOfferNever,
            Text::GuideResumeTour,
            Text::GuideShortcuts,
            Text::GuideResetTours,
            Text::GuideResetFailed,
            Text::ShortcutsTitle,
            Text::ShortcutsClose,
            Text::GuideUserGuide,
            Text::GuideSearchPlaceholder,
            Text::GuideSearchKeys,
            Text::GuideSearchEmpty,
            Text::GuideOnThisPage,
            Text::GuideShowMe,
            Text::GuideGoTo,
            Text::TourLearnMore,
            Text::TourThisScreen,
            Text::TourThisList,
            Text::TourThisPanel,
            Text::TourThisStage,
            Text::GuideMoreInGuide,
            Text::GuideBackToEditor,
            Text::ToursSettingTitle,
            Text::ToursSettingOffer,
            Text::ToursSettingHint,
            Text::ToursSettingNotSaved,
        ]);
        texts.extend(GuideGroup::ALL.map(Text::GuideGroupName));
        texts
    }

    #[test]
    fn every_text_exists_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for text in all_texts() {
                assert!(
                    catalog.strings.contains_key(text.key().as_ref()),
                    "{language} is missing {}",
                    text.key()
                );
            }
        }
    }

    #[test]
    fn all_languages_have_the_same_keys() {
        let reference = Catalog::load(UiLanguage::EnUs);
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            let expected: Vec<_> = reference.strings.keys().collect();
            let actual: Vec<_> = catalog.strings.keys().collect();
            assert_eq!(actual, expected, "{language} keys differ from en-US");
        }
    }

    #[test]
    fn limit_messages_match_the_domain_limits() {
        use bardo_domain::ChannelDetails;

        let catalog = Catalog::load(UiLanguage::EnUs);
        for (error, limit) in [
            (
                ChannelFieldError::NameTooLong,
                ChannelDetails::MAX_NAME_CHARS,
            ),
            (
                ChannelFieldError::NicheTooLong,
                ChannelDetails::MAX_NICHE_CHARS,
            ),
            (ChannelFieldError::TooManyThemes, ChannelDetails::MAX_THEMES),
            (
                ChannelFieldError::ThemeTooLong,
                ChannelDetails::MAX_THEME_CHARS,
            ),
        ] {
            let message = catalog.get(Text::ChannelFieldError(error));
            assert!(message.contains(&limit.to_string()), "{message}");
        }
    }

    #[test]
    fn placeholders_are_filled_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            let text = catalog.format(Text::JobRetryScheduled, &[("attempt", "2"), ("max", "4")]);
            assert!(text.contains('2') && text.contains('4'), "{text}");
            assert!(!text.contains('{'), "{text}");
            let text = catalog.format(Text::JobFailedAfter, &[("attempts", "4")]);
            assert!(text.contains('4') && !text.contains('{'), "{text}");
            let text = catalog.format(Text::KeySaved, &[("hint", "…abcd")]);
            assert!(text.contains("…abcd") && !text.contains('{'), "{text}");
            let text = catalog.format(Text::KeyCheckDetail, &[("detail", "Invalid API key")]);
            assert!(
                text.contains("Invalid API key") && !text.contains('{'),
                "{text}"
            );
            let text = catalog.format(Text::SetupTitle, &[("name", "Claude")]);
            assert!(text.contains("Claude") && !text.contains('{'), "{text}");
            let text = catalog.format(Text::SetupBack, &[("tab", "API keys")]);
            assert!(text.contains("API keys") && !text.contains('{'), "{text}");
            let text = catalog.format(Text::SetupAuditTitle, &[("network", "YouTube")]);
            assert!(text.contains("YouTube") && !text.contains('{'), "{text}");
        }
    }

    #[test]
    fn scene_messages_match_the_domain_and_fill_their_placeholders() {
        let catalog = Catalog::load(UiLanguage::EnUs);
        let too_long = catalog.get(Text::SceneFieldError(SceneFieldError::PromptTooLong));
        assert!(
            too_long.contains(&bardo_domain::ScenePrompt::MAX_CHARS.to_string()),
            "{too_long}"
        );
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for (text, args) in [
                (Text::PlanScenesHint, &[("n", "3")][..]),
                (Text::ReplanScenesConfirm, &[("n", "12")][..]),
                (
                    Text::NarrationOutdatesFit,
                    &[("images", "12"), ("clips", "3")][..],
                ),
                (
                    Text::NarrationOutdatesReplan,
                    &[("images", "12"), ("clips", "3")][..],
                ),
                (Text::ScenesCount, &[("n", "24")][..]),
                (Text::GenerateSceneImages, &[("n", "24")][..]),
                (
                    Text::SceneLabel,
                    &[("n", "7"), ("start", "0:41"), ("end", "0:47")][..],
                ),
                (
                    Text::SceneImageRecord,
                    &[("model", "nano"), ("tokens", "1290"), ("n", "2")][..],
                ),
            ] {
                let filled = catalog.format(text, args);
                assert!(!filled.contains('{'), "{filled}");
                for (_, value) in args {
                    assert!(filled.contains(value), "{language}: {filled}");
                }
            }
        }
    }

    #[test]
    fn scores_read_as_fractions_in_each_language() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        assert_eq!(en.score(Score::new(82)), "0.82");
        assert_eq!(pt.score(Score::new(5)), "0,05");
        assert_eq!(en.score(Score::new(100)), "1.00");
    }

    #[test]
    fn decibels_read_signed_in_each_language() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        assert_eq!(en.decibels(Decibels::from_tenths(-60)), "\u{2212}6.0 dB");
        assert_eq!(pt.decibels(Decibels::from_tenths(15)), "+1,5 dB");
        assert_eq!(en.decibels(Decibels::ZERO), "0.0 dB");
        assert_eq!(en.decibels(Decibels::from_tenths(-300)), "\u{2212}30.0 dB");
    }

    #[test]
    fn money_reads_in_each_language() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        let amount = Money::from_micros(1_234_567_891);
        assert_eq!(en.money(amount), "$1,234.57");
        assert_eq!(pt.money(amount), "US$ 1.234,57");
        assert_eq!(en.money(Money::ZERO), "$0.00");
        assert_eq!(en.money(Money::from_micros(5_000)), "$0.01");
        assert_eq!(en.money(Money::from_micros(4_999)), "under $0.01");
        assert_eq!(pt.money(Money::from_micros(1)), "menos de US$ 0,01");
    }

    #[test]
    fn plan_credits_read_in_each_language() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        let credits = bardo_domain::Credits::from_hundredths;
        assert_eq!(en.credits(credits(125_000)), "1,250");
        assert_eq!(pt.credits(credits(125_000)), "1.250");
        assert_eq!(en.credits(credits(98_250)), "982.5");
        assert_eq!(pt.credits(credits(98_250)), "982,5");
        assert_eq!(en.credits(credits(25)), "0.25");
    }

    #[test]
    fn owner_numbers_read_in_each_language() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        let share = Share::from_ten_thousandths;
        assert_eq!(en.percent(share(7_162)), "71.6%");
        assert_eq!(pt.percent(share(7_162)), "71,6%");
        assert_eq!(en.percent(share(10_000)), "100%");
        assert_eq!(en.percent(share(11_805)), "118.1%", "rewatched");
        assert_eq!(en.watch_time(45), "45 min");
        assert_eq!(en.watch_time(18_021), "300 h");
        assert_eq!(pt.watch_time(120_000), "2 mil h");
        assert_eq!(en.clock(34), "0:34");
        assert_eq!(en.clock(725), "12:05");
        assert_eq!(en.clock(3_729), "1:02:09");
    }

    #[test]
    fn prices_keep_the_digits_they_need() {
        let en = Catalog::load(UiLanguage::EnUs);
        assert_eq!(en.price(Money::from_cents(400)), "$4.00");
        assert_eq!(en.price(Money::from_micros(42_000)), "$0.042");
        assert_eq!(en.price(Money::from_micros(1)), "$0.000001");
        assert_eq!(en.price(Money::from_micros(60_000_000)), "$60.00");
    }

    #[test]
    fn months_read_in_each_language() {
        let october = Month::new(2026, 10).unwrap();
        assert_eq!(
            Catalog::load(UiLanguage::EnUs).month(october),
            "October 2026"
        );
        assert_eq!(
            Catalog::load(UiLanguage::PtBr).month(october),
            "outubro de 2026"
        );
    }

    #[test]
    fn publish_times_read_in_each_language_with_their_zone() {
        let local = LocalTime {
            year: 2026,
            month: 10,
            day: 4,
            hour: 18,
            minute: 5,
            weekday: 7,
            zone: "America/Sao_Paulo".into(),
            offset: "UTC−03:00".into(),
        };
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        assert_eq!(
            en.date_time(&local),
            "Sun, October 4, 2026, 6:05 PM (America/Sao_Paulo, UTC−03:00)"
        );
        assert_eq!(
            pt.date_time(&local),
            "dom., 4 de outubro de 2026, 18:05 (America/Sao_Paulo, UTC−03:00)"
        );
        let midnight = LocalTime {
            hour: 0,
            zone: "UTC+05:00".into(),
            offset: "UTC+05:00".into(),
            ..local
        };
        assert_eq!(
            en.date_time(&midnight),
            "Sun, October 4, 2026, 12:05 AM (UTC+05:00)",
            "a zone without a name shows its offset once"
        );
        assert_eq!(en.date_order(), DateOrder::MonthFirst);
        assert_eq!(pt.date_order(), DateOrder::DayFirst);
    }

    #[test]
    fn cost_placeholders_are_filled_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for (text, args) in [
                (Text::CostsTotal, &[("month", "May 2026")][..]),
                (Text::CostsEmpty, &[("month", "May 2026")][..]),
                (Text::CostsUnpriced, &[("models", "claude-x")][..]),
                (Text::BudgetUsed, &[("budget", "$9"), ("percent", "81")][..]),
                (Text::ResetRate, &[("price", "$4.00")][..]),
                (Text::EstimateCost, &[("amount", "$0.12")][..]),
                (Text::EstimatePartial, &[("amount", "$0.12")][..]),
                (Text::ProjectSpent, &[("amount", "$0.12")][..]),
                (Text::EstimateRedraw, &[("amount", "$0.12")][..]),
                (
                    Text::EstimateNear,
                    &[("provider", "Claude"), ("spent", "$8"), ("budget", "$9")][..],
                ),
                (
                    Text::BudgetReachedLine,
                    &[
                        ("provider", "Claude"),
                        ("spent", "$8"),
                        ("budget", "$9"),
                        ("amount", "$2"),
                    ][..],
                ),
                (
                    Text::PriceUnknownLine,
                    &[("provider", "Kling"), ("model", "kling-9")][..],
                ),
                (Text::SpendCapUseDefault, &[("cap", "$5")][..]),
                (Text::SpendCapOffer, &[("cap", "$5")][..]),
                (Text::SpendCapOfferAccept, &[("cap", "$5")][..]),
                (
                    Text::CapReachedLine,
                    &[
                        ("spent", "$4"),
                        ("cap", "$5"),
                        ("amount", "$2"),
                        ("rest", "$3"),
                    ][..],
                ),
                (
                    Text::ProjectEstimate,
                    &[("total", "$1.80"), ("spent", "$0.40")][..],
                ),
                (
                    Text::ProjectModeOption,
                    &[("mode", "Economy"), ("amount", "$1.30")][..],
                ),
            ] {
                let filled = catalog.format(text, args);
                assert!(!filled.contains('{'), "{filled}");
                for (_, value) in args {
                    assert!(filled.contains(value), "{language}: {filled}");
                }
            }
        }
    }

    #[test]
    fn key_length_message_matches_the_domain_limit() {
        let catalog = Catalog::load(UiLanguage::EnUs);
        let message = catalog.get(Text::ApiKeyError(ApiKeyError::TooLong));
        assert!(
            message.contains(&bardo_domain::ApiKey::MAX_CHARS.to_string()),
            "{message}"
        );
    }

    #[test]
    fn seed_limit_messages_match_the_domain_limits() {
        use bardo_domain::{Niche, NicheSeeds};

        let catalog = Catalog::load(UiLanguage::EnUs);
        let too_many = catalog.get(Text::NicheSeedError(NicheSeedError::TooMany));
        assert!(
            too_many.contains(&NicheSeeds::MAX.to_string()),
            "{too_many}"
        );
        let too_long = catalog.get(Text::NicheSeedError(NicheSeedError::TooLong));
        assert!(
            too_long.contains(&Niche::MAX_CHARS.to_string()),
            "{too_long}"
        );
    }

    #[test]
    fn research_placeholders_are_filled_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for (text, args) in [
                (Text::ResearchCost, &[("units", "204")][..]),
                (Text::RefreshResearchCost, &[("units", "510")][..]),
                (
                    Text::ResearchSample,
                    &[("videos", "47"), ("channels", "38")][..],
                ),
            ] {
                let filled = catalog.format(text, args);
                assert!(!filled.contains('{'), "{filled}");
                for (_, value) in args {
                    assert!(filled.contains(value), "{filled}");
                }
            }
        }
    }

    #[test]
    fn theme_limit_messages_match_the_domain_limits() {
        use bardo_domain::ThemeIdea;

        let catalog = Catalog::load(UiLanguage::EnUs);
        for (error, limit) in [
            (ThemeFieldError::TitleTooLong, ThemeIdea::MAX_TITLE_CHARS),
            (ThemeFieldError::AngleTooLong, ThemeIdea::MAX_ANGLE_CHARS),
        ] {
            let message = catalog.get(Text::ThemeFieldError(error));
            assert!(message.contains(&limit.to_string()), "{message}");
        }
    }

    #[test]
    fn theme_placeholders_are_filled_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for (text, args) in [
                (Text::SuggestThemesHint, &[("n", "10")][..]),
                (Text::CutsHint, &[("n", "12")][..]),
                (Text::CutsAgainHint, &[("n", "12")][..]),
                (Text::CutsRunning, &[("percent", "40")][..]),
                (Text::CutsAcceptStrong, &[("score", "0.80")][..]),
                (Text::CutsHidden, &[("n", "3"), ("score", "0.50")][..]),
                (Text::CutsShown, &[("n", "3"), ("score", "0.50")][..]),
                (Text::CutsConfidence, &[("n", "82")][..]),
                (Text::CutsReasonPause, &[("ms", "420")][..]),
                (Text::ThemesUnranked, &[("n", "3")][..]),
                (Text::ThemesDiscarded, &[("n", "4")][..]),
                (Text::ThemeRankedBy, &[("model", "jev-1.13.0")][..]),
                (Text::ProjectStarted, &[("title", "The Lost Probe")][..]),
            ] {
                let filled = catalog.format(text, args);
                assert!(!filled.contains('{'), "{filled}");
                for (_, value) in args {
                    assert!(filled.contains(value), "{filled}");
                }
            }
        }
    }

    #[test]
    fn script_and_template_limit_messages_match_the_domain_limits() {
        use bardo_domain::{ScriptText, TemplateBody};

        let catalog = Catalog::load(UiLanguage::EnUs);
        let script = catalog.get(Text::ScriptFieldError(ScriptFieldError::TextTooLong));
        assert!(
            script.contains(&ScriptText::MAX_CHARS.to_string()),
            "{script}"
        );
        let template = catalog.get(Text::TemplateProblem(TemplateProblem::TooLong));
        assert!(
            template.contains(&TemplateBody::MAX_CHARS.to_string()),
            "{template}"
        );
    }

    #[test]
    fn script_and_template_placeholders_are_filled_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for (text, args) in [
                (Text::GenerateScriptHint, &[("n", "3")][..]),
                (Text::ScriptWords, &[("n", "1250")][..]),
                (Text::ProvenanceTemplateVersion, &[("n", "3")][..]),
                (
                    Text::ProvenanceTokensValue,
                    &[("input", "812"), ("output", "2431")][..],
                ),
                (Text::TemplateVersionLabel, &[("n", "3")][..]),
                (Text::TemplateEditing, &[("n", "3"), ("next", "4")][..]),
                (Text::TemplateSaved, &[("n", "4")][..]),
                (
                    Text::NarrationGenerateHint,
                    &[("persona", "Narrator"), ("voice", "Wyatt"), ("n", "1520")][..],
                ),
                (Text::NarrationCostValue, &[("n", "1520")][..]),
                (
                    Text::RecordingChosen,
                    &[("file", "take 3.wav"), ("length", "3:04")][..],
                ),
                (Text::NarrationAudioValue, &[("length", "3:04")][..]),
            ] {
                let filled = catalog.format(text, args);
                assert!(!filled.contains('{'), "{filled}");
                for (_, value) in args {
                    assert!(filled.contains(value), "{filled}");
                }
            }
        }
    }

    #[test]
    fn persona_limit_messages_match_the_domain_limits() {
        use bardo_domain::{GenerationPresets, PersonaDetails};

        let catalog = Catalog::load(UiLanguage::EnUs);
        for (error, limit) in [
            (
                PersonaFieldError::NameTooLong,
                PersonaDetails::MAX_NAME_CHARS.to_string(),
            ),
            (
                PersonaFieldError::ToneTooLong,
                format!("{}", PersonaDetails::MAX_TONE_CHARS / 1000),
            ),
            (
                PersonaFieldError::ScriptStyleTooLong,
                format!("{}", PersonaDetails::MAX_SCRIPT_STYLE_CHARS / 1000),
            ),
            (
                PersonaFieldError::SpeedOutOfRange,
                GenerationPresets::SPEED.start().to_string(),
            ),
            (
                PersonaFieldError::SpeedOutOfRange,
                GenerationPresets::SPEED.end().to_string(),
            ),
        ] {
            let message = catalog.get(Text::PersonaFieldError(error));
            assert!(message.contains(&limit), "{message}");
        }
    }

    #[test]
    fn persona_placeholders_are_filled_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for (text, args) in [
                (Text::PresetPercent, &[("n", "75")][..]),
                (Text::PersonaDuplicated, &[("name", "Narrator (copy)")][..]),
                (
                    Text::PersonaUsedBy,
                    &[("places", "Space › Space Archives")][..],
                ),
                (Text::PersonaConfirmTitle, &[("n", "2")][..]),
            ] {
                let filled = catalog.format(text, args);
                assert!(!filled.contains('{'), "{filled}");
                for (_, value) in args {
                    assert!(filled.contains(value), "{filled}");
                }
            }
        }
    }

    #[test]
    fn network_account_limit_messages_match_the_domain_limits() {
        use bardo_domain::NetworkAccountDetails;

        let catalog = Catalog::load(UiLanguage::EnUs);
        for (error, limit) in [
            (
                NetworkAccountFieldError::HandleTooLong,
                NetworkAccountDetails::MAX_HANDLE_CHARS.to_string(),
            ),
            (
                NetworkAccountFieldError::TooManyTags,
                NetworkAccountDetails::MAX_TAGS.to_string(),
            ),
            (
                NetworkAccountFieldError::TagTooLong,
                NetworkAccountDetails::MAX_TAG_CHARS.to_string(),
            ),
            (
                NetworkAccountFieldError::DescriptionFooterTooLong,
                "1,000".to_owned(),
            ),
        ] {
            let message = catalog.get(Text::NetworkAccountFieldError(error));
            assert!(message.contains(&limit), "{message}");
        }
        assert_eq!(NetworkAccountDetails::MAX_DESCRIPTION_FOOTER_CHARS, 1_000);
    }

    #[test]
    fn network_account_placeholders_are_filled_in_every_language() {
        for language in UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for text in [
                Text::AddNetworkAccount,
                Text::NewNetworkAccountTitle,
                Text::EditNetworkAccountTitle,
                Text::AccountVisibilityOnlyPublic,
                Text::NetworkAccountRemoveConfirm,
            ] {
                let text = catalog.format(text, &[("network", "TikTok")]);
                assert!(text.contains("TikTok") && !text.contains('{'), "{text}");
            }
            let text = catalog.format(Text::AccountLanguageChannel, &[("language", "Inglês")]);
            assert!(text.contains("Inglês") && !text.contains('{'), "{text}");
            let text = catalog.format(Text::RenderPresetNetworkDefault, &[("value", "3:00")]);
            assert!(text.contains("3:00") && !text.contains('{'), "{text}");
            let text = catalog.format(Text::RenderPresetEffective, &[("summary", "9:16")]);
            assert!(text.contains("9:16") && !text.contains('{'), "{text}");
        }
    }

    #[test]
    fn counts_are_compact_and_localized() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        let cases = [
            (0, "0", "0"),
            (999, "999", "999"),
            (1_000, "1K", "1 mil"),
            (8_790, "8.7K", "8,7 mil"),
            (48_213, "48K", "48 mil"),
            (999_999, "999K", "999 mil"),
            (1_840_000, "1.8M", "1,8 mi"),
            (12_000_000, "12M", "12 mi"),
            (2_100_000_000, "2.1B", "2,1 bi"),
        ];
        for (n, english, portuguese) in cases {
            assert_eq!(en.compact(n), english);
            assert_eq!(pt.compact(n), portuguese);
        }
    }

    #[test]
    fn ages_pick_a_readable_unit() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        let minutes = |m: u64| Duration::from_secs(m * 60);
        assert_eq!(en.age(Duration::from_secs(30)), "just now");
        assert_eq!(en.age(minutes(5)), "5 min ago");
        assert_eq!(en.age(minutes(90)), "1 h ago");
        assert_eq!(en.age(minutes(47 * 60)), "47 h ago");
        assert_eq!(en.age(minutes(3 * 1_440)), "3 days ago");
        assert_eq!(pt.age(minutes(3 * 1_440)), "há 3 dias");
        assert_eq!(pt.age(minutes(5)), "há 5 min");
    }

    #[test]
    fn strings_differ_between_languages() {
        let en = Catalog::load(UiLanguage::EnUs);
        let pt = Catalog::load(UiLanguage::PtBr);
        assert_ne!(en.get(Text::AppTagline), pt.get(Text::AppTagline));
    }
}
