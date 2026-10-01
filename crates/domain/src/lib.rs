//! Entities, value objects, domain services and provider interfaces.
//!
//! No I/O lives here; adapters in other crates implement the interfaces.

mod channel;
mod market;
mod profile;
mod repository;

pub use channel::{
    Channel, ChannelDetails, ChannelDraft, ChannelFieldError, ChannelId, ChannelRepository,
};
pub use market::{ContentLanguage, Country, UnsupportedContentLanguage, UnsupportedCountry};
pub use profile::{ProfileId, ProfileRepository, UiLanguage, UnsupportedLanguage, UserProfile};
pub use repository::RepositoryError;
