//! UI strings, loaded from one resource file per language in `locales/`.

use std::borrow::Cow;
use std::collections::BTreeMap;

use std::time::Duration;

use crate::{Destination, Pillar, Stage, UploadBlock};

use bardo_domain::{
    ApiKeyError, AppCredentialsFieldError, AspectRatio, CaptionStyle, ChannelFieldError,
    ContentLanguage, Country, DateOrder, Decibels, JobFailureKind, JobKind, JobState,
    KeyCheckOutcome, LayoutId, LocalTime, MetadataProblem, Meter, MetricsSyncOnStart, Money,
    MoneyError, Month, MusicPromptFieldError, Network, NetworkAccountFieldError, NicheSeedError,
    PastedTokenError, PersonaFieldError, PostLinkError, Provider, RateFieldError, SceneFieldError,
    ScheduleProblem, Score, ScriptFieldError, Share, SignInFailureKind, TemplateKind,
    TemplateProblem, TemplateVariable, ThemeFamily, ThemeFieldError, ThemeMode, UiLanguage,
    UiTheme, Visibility, VoiceCategory, VoiceFlag,
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
    ChannelPersona,
    ChannelPersonaNone,
    ChannelPersonaHint,
    ChannelPersonaNotFound,
    ChannelClipModel,
    /// Placeholder: `{model}`.
    ChannelClipModelDefault,
    ChannelClipModelHint,
    ChannelCaptionStyle,
    ChannelCaptionStyleHint,
    ChannelClipModelNotOffered,
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
    ProviderKeysHint,
    ProviderName(Provider),
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
    ProjectsNotLoaded,
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
    ScenesStopped,
    ScenesStale,
    ScenesStaleTag,
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
    SceneNotFound,
    ScenesMissingClaudeKey,
    ScenesMissingGeminiKey,
    ScenesMissingHiggsfieldKey,
    SceneNoImageToAnimate,
    ScenesNothingToAnimate,
    SceneClipModelNotOffered,
    ScenesBusy,
    ScenesWouldDiscardImages,
    ScenesNothingToGenerate,
    SceneNothingToReview,
    ScenesNotLoaded,
    SceneFieldError(SceneFieldError),
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
    EditorSnapWords,
    EditorAiCuts,
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
    CutsMissingKey,
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
    /// Placeholder: `{n}`, signed, with the decimal separator.
    EditorDecibels,
    EditorLaneHint,
    EditorInspectorCaption,
    EditorCaptionText,
    EditorCaptionTextHint,
    EditorCaptionStyle,
    EditorCaptionStyleHint,
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
    MusicPromptFieldError(MusicPromptFieldError),
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
    PillarName(Pillar),
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
    GateCaptionsOff,
    GateMissingMedia,
    GateSilent,
    GateLoudnessFar,
    GatePeaksLimited,
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
    UploadFailureQuota,
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
    UploadReelIrreversible,
    UploadReelStart,
    UploadReelProcessingHint,
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
    UploadReelStartScheduled,
    /// One TikTok spec problem, by `TikTokSpecProblem::code`.
    UploadTikTokSpec(&'static str),
    UploadTikTokSpecsTitle,
    UploadDraftHint,
    UploadDraftNotice,
    UploadFieldDraftCaption,
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
}

impl Text {
    fn key(self) -> Cow<'static, str> {
        let key = match self {
            Text::AppName => "app.name",
            Text::AppTagline => "app.tagline",
            Text::UiLanguageLabel => "settings.ui_language",
            Text::LanguageName(UiLanguage::EnUs) => "language.en-US",
            Text::LanguageName(UiLanguage::PtBr) => "language.pt-BR",
            Text::LanguageNotSaved => "error.language_not_saved",
            Text::ContentLanguageName(language) => {
                return format!("content_language.{}", language.code()).into();
            }
            Text::CountryName(country) => return format!("country.{}", country.code()).into(),
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
            Text::ChannelClipModelHint => "channel.clip_model_hint",
            Text::ChannelCaptionStyle => "channel.caption_style",
            Text::ChannelCaptionStyleHint => "channel.caption_style_hint",
            Text::ChannelClipModelNotOffered => "channel.error.clip_model_not_offered",
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
            Text::ProviderKeysHint => "provider_keys.hint",
            Text::ProviderName(provider) => {
                return format!("provider.{}.name", provider.code()).into();
            }
            Text::ProviderPurpose(provider) => {
                return format!("provider.{}.purpose", provider.code()).into();
            }
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
            Text::ScenesStopped => "scenes.stopped",
            Text::ScenesStale => "scenes.stale",
            Text::ScenesStaleTag => "scenes.stale_tag",
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
            Text::SceneNotFound => "scenes.error.scene_not_found",
            Text::ScenesMissingClaudeKey => "scenes.error.missing_claude_key",
            Text::ScenesMissingGeminiKey => "scenes.error.missing_gemini_key",
            Text::ScenesMissingHiggsfieldKey => "scenes.error.missing_higgsfield_key",
            Text::SceneNoImageToAnimate => "scenes.error.no_image_to_animate",
            Text::ScenesNothingToAnimate => "scenes.error.nothing_to_animate",
            Text::SceneClipModelNotOffered => "scenes.error.clip_model_not_offered",
            Text::ScenesBusy => "scenes.error.busy",
            Text::ScenesWouldDiscardImages => "scenes.error.would_discard_images",
            Text::ScenesNothingToGenerate => "scenes.error.nothing_to_generate",
            Text::SceneNothingToReview => "scenes.error.nothing_to_review",
            Text::ScenesNotLoaded => "scenes.error.not_loaded",
            Text::SceneFieldError(error) => match error {
                SceneFieldError::PromptRequired => "scenes.error.prompt_required",
                SceneFieldError::PromptTooLong => "scenes.error.prompt_too_long",
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
            Text::EditorSnapWords => "editor.tool.snap_words",
            Text::EditorAiCuts => "editor.tool.ai_cuts",
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
            Text::CutsMissingKey => "editor.cuts.missing_key",
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
            Text::EditorDecibels => "editor.mix.decibels",
            Text::EditorLaneHint => "editor.mix.lane_hint",
            Text::EditorInspectorCaption => "editor.inspector.caption",
            Text::EditorCaptionText => "editor.captions.text",
            Text::EditorCaptionTextHint => "editor.captions.text_hint",
            Text::EditorCaptionStyle => "editor.captions.style",
            Text::EditorCaptionStyleHint => "editor.captions.style_hint",
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
            Text::MusicPromptFieldError(error) => match error {
                MusicPromptFieldError::TextRequired => "music.error.text_required",
                MusicPromptFieldError::TextTooLong => "music.error.text_too_long",
            },
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
            Text::StageToReview => "stage.note.to_review",
            Text::StageImages => "stage.note.images",
            Text::StageClips => "stage.note.clips",
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
            Text::PillarName(pillar) => match pillar {
                Pillar::Strategy => "nav.pillar.strategy",
                Pillar::Production => "nav.pillar.production",
                Pillar::Publishing => "nav.pillar.publishing",
            },
            Text::DestinationName(place) => match place {
                Destination::Research => "nav.place.research",
                Destination::Themes => "nav.place.themes",
                Destination::Performance => "nav.place.performance",
                Destination::Projects => "nav.place.projects",
                Destination::Personas => "nav.place.personas",
                Destination::Templates => "nav.place.templates",
                Destination::Channels => "nav.place.channels",
                Destination::Accounts => "nav.place.accounts",
                Destination::Jobs => "nav.place.jobs",
                Destination::Costs => "nav.place.costs",
                Destination::Settings => "nav.place.settings",
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
            Text::GateCaptionsOff => "render.gate.captions_off",
            Text::GateMissingMedia => "render.gate.missing_media",
            Text::GateSilent => "render.gate.silent",
            Text::GateLoudnessFar => "render.gate.loudness_far",
            Text::GatePeaksLimited => "render.gate.peaks_limited",
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
            Text::UploadFailureQuota => "upload.failure.quota",
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
            Text::UploadReelIrreversible => "upload.reel_irreversible",
            Text::UploadReelStart => "upload.reel_start",
            Text::UploadReelProcessingHint => "upload.reel_processing_hint",
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
            Text::UploadReelStartScheduled => "upload.reel_start_scheduled",
            Text::UploadTikTokSpec(code) => return format!("upload.tiktok_spec.{code}").into(),
            Text::UploadTikTokSpecsTitle => "upload.tiktok_specs_title",
            Text::UploadDraftHint => "upload.draft_hint",
            Text::UploadDraftNotice => "upload.draft_notice",
            Text::UploadFieldDraftCaption => "upload.field.draft_caption",
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
                Stage::Script => "stage.name.script",
                Stage::Narration => "stage.name.narration",
                Stage::Scenes => "stage.name.scenes",
                Stage::Clips => "stage.name.clips",
                Stage::Edit => "stage.name.edit",
                Stage::Render => "stage.name.render",
                Stage::Publish => "stage.name.publish",
            },
            Text::StageAfter(stage) => match stage {
                Stage::Script => "stage.after.script",
                Stage::Narration => "stage.after.narration",
                Stage::Scenes => "stage.after.scenes",
                Stage::Clips => "stage.after.clips",
                Stage::Edit => "stage.after.edit",
                Stage::Render => "stage.after.render",
                Stage::Publish => "stage.after.publish",
            },
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
        let time = self.time(local);
        let when = self.format(
            Text::DateTimeFormat,
            &[
                ("weekday", &self.get(Text::WeekdayName(local.weekday))),
                ("month", &self.get(Text::MonthName(local.month))),
                ("day", &local.day.to_string()),
                ("year", &local.year.to_string()),
                ("time", &time),
            ],
        );
        self.format(
            Text::DateTimeWithZone,
            &[("when", &when), ("zone", &self.zone(local))],
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
            Text::ChannelClipModelHint,
            Text::ChannelCaptionStyle,
            Text::ChannelCaptionStyleHint,
            Text::ChannelClipModelNotOffered,
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
            Text::ScenesStopped,
            Text::ScenesStale,
            Text::ScenesStaleTag,
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
            Text::SceneNotFound,
            Text::ScenesMissingClaudeKey,
            Text::ScenesMissingGeminiKey,
            Text::ScenesMissingHiggsfieldKey,
            Text::SceneNoImageToAnimate,
            Text::ScenesNothingToAnimate,
            Text::SceneClipModelNotOffered,
            Text::ScenesBusy,
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
            Text::EditorSnapWords,
            Text::EditorAiCuts,
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
            Text::CutsMissingKey,
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
            Text::EditorDecibels,
            Text::EditorLaneHint,
            Text::EditorInspectorCaption,
            Text::EditorCaptionText,
            Text::EditorCaptionTextHint,
            Text::EditorCaptionStyle,
            Text::EditorCaptionStyleHint,
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
        ];
        texts.extend(NicheSeedError::ALL.map(Text::NicheSeedError));
        texts.extend(Provider::ALL.map(Text::ProviderName));
        texts.extend(Provider::ALL.map(Text::ProviderPurpose));
        texts.extend(Provider::ALL.map(Text::ProviderKeyPlaceholder));
        texts.extend(KeyCheckOutcome::ALL.map(Text::KeyCheckOutcome));
        texts.extend(ApiKeyError::ALL.map(Text::ApiKeyError));
        texts.extend(UiLanguage::ALL.map(Text::LanguageName));
        texts.extend(JobKind::ALL.map(Text::JobKindName));
        texts.extend(JobState::ALL.map(Text::JobStateName));
        texts.extend(JobFailureKind::ALL.map(Text::JobFailureKindName));
        texts.extend(ContentLanguage::ALL.map(Text::ContentLanguageName));
        texts.extend(Country::ALL.map(Text::CountryName));
        texts.extend(ChannelFieldError::ALL.map(Text::ChannelFieldError));
        texts.extend(PersonaFieldError::ALL.map(Text::PersonaFieldError));
        texts.extend(Network::ALL.map(Text::NetworkName));
        texts.extend(Visibility::ALL.map(Text::VisibilityName));
        texts.extend(AspectRatio::ALL.map(Text::AspectRatioName));
        texts.extend(CaptionStyle::ALL.map(Text::CaptionStyleName));
        texts.extend(NetworkAccountFieldError::ALL.map(Text::NetworkAccountFieldError));
        texts.extend(VoiceCategory::ALL.map(Text::VoiceCategoryName));
        for flag in [VoiceFlag::Unchecked, VoiceFlag::Unavailable] {
            texts.push(Text::VoiceFlagTag(flag));
            texts.push(Text::VoiceFlagExplanation(flag));
            texts.push(Text::NarrationVoiceFlagged(flag));
        }
        texts.extend(ThemeFieldError::ALL.map(Text::ThemeFieldError));
        texts.extend(ScriptFieldError::ALL.map(Text::ScriptFieldError));
        texts.extend(MusicPromptFieldError::ALL.map(Text::MusicPromptFieldError));
        texts.extend(SceneFieldError::ALL.map(Text::SceneFieldError));
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
        texts.push(Text::StageToReview);
        texts.push(Text::StageImages);
        texts.push(Text::StageClips);
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
        texts.extend(Pillar::ALL.map(Text::PillarName));
        texts.extend(Destination::ALL.map(Text::DestinationName));
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
        texts.push(Text::GateCaptionsOff);
        texts.push(Text::GateMissingMedia);
        texts.push(Text::GateSilent);
        texts.push(Text::GateLoudnessFar);
        texts.push(Text::GatePeaksLimited);
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
        texts.push(Text::UploadFailureQuota);
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
            Text::UploadReelIrreversible,
            Text::UploadReelStart,
            Text::UploadReelProcessingHint,
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
        texts.push(Text::UploadReelStartScheduled);
        texts.extend(bardo_domain::TikTokSpecProblem::CODES.map(Text::UploadTikTokSpec));
        texts.push(Text::UploadTikTokSpecsTitle);
        texts.push(Text::UploadDraftHint);
        texts.push(Text::UploadDraftNotice);
        texts.push(Text::UploadFieldDraftCaption);
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
        texts.push(Text::TimeFormat);
        texts.push(Text::TimeAm);
        texts.push(Text::TimePm);
        texts.push(Text::DateTimeWithZone);
        texts.push(Text::ZoneWithOffset);
        texts.extend((1..=7).map(Text::WeekdayName));
        texts.extend(ScheduleProblem::ALL.map(Text::ScheduleProblem));
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
                (Text::PersonaUsedBy, &[("channels", "Space Archives")][..]),
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
