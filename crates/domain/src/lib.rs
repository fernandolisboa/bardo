//! Entities, value objects, domain services and provider interfaces.
//!
//! No I/O lives here; adapters in other crates implement the interfaces.

mod profile;

pub use profile::{
    ProfileId, ProfileRepository, RepositoryError, UiLanguage, UnsupportedLanguage, UserProfile,
};
