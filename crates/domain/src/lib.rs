//! Entities, value objects, domain services and provider interfaces.
//!
//! No I/O lives here; adapters in other crates implement the interfaces.

mod channel;
mod job;
mod market;
mod profile;
mod provider_key;
mod redaction;
mod repository;

pub use channel::{
    Channel, ChannelDetails, ChannelDraft, ChannelFieldError, ChannelId, ChannelRepository,
};
pub use job::{
    InconsistentJob, InvalidJobTransition, Job, JobFailure, JobFailureKind, JobId, JobKind,
    JobRecord, JobRepository, JobState, Progress, RetryPolicy, UnknownJobFailureKind,
    UnknownJobKind, UnknownJobState,
};
pub use market::{ContentLanguage, Country, UnsupportedContentLanguage, UnsupportedCountry};
pub use profile::{ProfileId, ProfileRepository, UiLanguage, UnsupportedLanguage, UserProfile};
pub use provider_key::{
    ApiKey, ApiKeyError, KeyCheck, KeyCheckOutcome, KeyChecker, Provider, SecretStore,
    SecretStoreError, UnknownProvider,
};
pub use redaction::Redactor;
pub use repository::RepositoryError;
