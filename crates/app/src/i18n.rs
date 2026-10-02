//! UI strings, loaded from one resource file per language in `locales/`.

use std::borrow::Cow;
use std::collections::BTreeMap;

use std::time::Duration;

use bardo_domain::{
    ApiKeyError, ChannelFieldError, ContentLanguage, Country, JobFailureKind, JobKind, JobState,
    KeyCheckOutcome, NicheSeedError, PersonaFieldError, Provider, ScriptFieldError, TemplateKind,
    TemplateProblem, TemplateVariable, ThemeFieldError, UiLanguage, VoiceCategory,
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
    PersonaFieldError(PersonaFieldError),
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
    ProvenanceTitle,
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
            Text::ProvenanceTitle => "provenance.title",
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
            Text::ProvenanceTitle,
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
        texts.extend(VoiceCategory::ALL.map(Text::VoiceCategoryName));
        texts.extend(ThemeFieldError::ALL.map(Text::ThemeFieldError));
        texts.extend(ScriptFieldError::ALL.map(Text::ScriptFieldError));
        texts.extend(TemplateKind::ALL.map(Text::TemplateKindName));
        texts.extend(TemplateVariable::ALL.map(Text::TemplateVariableHint));
        texts.extend(TemplateProblem::ALL.map(Text::TemplateProblem));
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
