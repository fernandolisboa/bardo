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

mod channel;
mod decision;
mod generation;
mod job;
mod market;
mod persona;
mod profile;
mod provider_key;
mod redaction;
mod repository;
mod research;
mod script;
mod template;
mod text;
mod theme;
mod voice;

pub use channel::{
    Channel, ChannelDetails, ChannelDraft, ChannelFieldError, ChannelId, ChannelRepository,
};
pub use decision::{
    Answer, ChoiceAnswer, Confidence, DecisionEngine, Decisions, InvalidQuestion, Question,
    Questions, ScoreAnswer, YesNoAnswer,
};
pub use generation::{Generation, GenerationId, TemplateUsed};
pub use job::{
    InconsistentJob, InvalidJobTransition, Job, JobFailure, JobFailureKind, JobId, JobKind,
    JobRecord, JobRepository, JobState, Progress, RetryPolicy, UnknownJobFailureKind,
    UnknownJobKind, UnknownJobState,
};
pub use market::{
    ContentLanguage, Country, Market, UnsupportedContentLanguage, UnsupportedCountry,
};
pub use persona::{
    GenerationPresets, Persona, PersonaDetails, PersonaDraft, PersonaFieldError, PersonaId,
    PersonaRepository,
};
pub use profile::{ProfileId, ProfileRepository, UiLanguage, UnsupportedLanguage, UserProfile};
pub use provider_key::{
    ApiKey, ApiKeyError, KeyCheck, KeyCheckOutcome, KeyChecker, Provider, ProviderFailure,
    ProviderFailureKind, SecretStore, SecretStoreError, UnknownProvider,
};
pub use redaction::Redactor;
pub use repository::RepositoryError;
pub use research::{
    MarketData, MarketSample, Niche, NicheResearch, NicheResearchRepository, NicheScores,
    NicheSeedError, NicheSeeds, NicheStatistics, Score, UploadSample, rank,
};
pub use script::{
    GeneratedScript, NoPendingScript, Script, ScriptFieldError, ScriptRecord, ScriptRepository,
    ScriptText,
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
pub use voice::{InvalidVoiceRef, Voice, VoiceCategory, VoiceLibrary, VoiceRef};
