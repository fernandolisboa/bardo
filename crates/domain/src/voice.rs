//! Voices (CONTEXT.md): a voice lives with its provider (ElevenLabs);
//! a persona keeps only a reference to it, never samples, audio or
//! credentials (ADR-0005). Clips played to hear a voice are a cache of
//! this machine (`voice_sample`), never part of a persona. The voice
//! library interface lists the voices the user's provider account can use,
//! including clones made there, with a link to the provider's stock
//! preview of each voice when it has one.

use std::fmt;
use std::sync::Arc;

use crate::{ApiKey, Provider, ProviderFailure};

/// Why a provider's voice cannot be referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidVoiceRef {
    /// Only voice providers hold voices.
    #[error("{0} holds no voices")]
    NotAVoiceProvider(Provider),
    /// Ids are short, URL-safe tokens; anything else is not an id.
    #[error("not a voice id")]
    InvalidId,
}

/// A pointer to a voice a provider holds: the provider, its id for the
/// voice and the name it showed when picked, so the screen can say which
/// voice it is without asking the provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoiceRef {
    provider: Provider,
    id: String,
    name: String,
}

impl VoiceRef {
    pub const MAX_ID_CHARS: usize = 64;
    /// Longer provider names are cut, not rejected: the name is a label.
    pub const MAX_NAME_CHARS: usize = 100;

    /// The provider that can hold voices.
    pub const PROVIDERS: [Provider; 1] = [Provider::ElevenLabs];

    /// A blank name falls back to the id.
    pub fn new(provider: Provider, id: &str, name: &str) -> Result<Self, InvalidVoiceRef> {
        if !Self::PROVIDERS.contains(&provider) {
            return Err(InvalidVoiceRef::NotAVoiceProvider(provider));
        }
        let id = id.trim();
        if id.is_empty()
            || id.len() > Self::MAX_ID_CHARS
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(InvalidVoiceRef::InvalidId);
        }
        let name = name.trim();
        let name = if name.is_empty() {
            id.to_owned()
        } else {
            name.chars().take(Self::MAX_NAME_CHARS).collect()
        };
        Ok(Self {
            provider,
            id: id.to_owned(),
            name,
        })
    }

    /// An ElevenLabs voice.
    pub fn elevenlabs(id: &str, name: &str) -> Result<Self, InvalidVoiceRef> {
        Self::new(Provider::ElevenLabs, id, name)
    }

    pub fn provider(&self) -> Provider {
        self.provider
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether both point at the same voice, whatever name each recorded.
    pub fn same_voice(&self, other: &VoiceRef) -> bool {
        self.provider == other.provider && self.id == other.id
    }
}

impl fmt::Display for VoiceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}:{})", self.name, self.provider, self.id)
    }
}

/// Where a voice in the account came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VoiceCategory {
    /// Cloned from recordings in the provider's flow (instant clone).
    Cloned,
    /// A professional clone, verified by the provider.
    Professional,
    /// Designed from a text prompt.
    Generated,
    /// One of the provider's voices every account has.
    Default,
    /// Anything else the provider lists (e.g. added from its library).
    Other,
}

impl VoiceCategory {
    /// In picker order: the user's own voices before the provider's.
    pub const ALL: [VoiceCategory; 5] = [
        VoiceCategory::Cloned,
        VoiceCategory::Professional,
        VoiceCategory::Generated,
        VoiceCategory::Default,
        VoiceCategory::Other,
    ];
}

/// A voice the account can use, as the provider lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    pub reference: VoiceRef,
    pub category: VoiceCategory,
    /// The provider's description; may be empty.
    pub description: String,
    /// Short traits (gender, age, accent, use), in the provider's words.
    pub labels: Vec<String>,
    /// An HTTPS link to the provider's own recording of the voice, free to
    /// play. It ignores a persona's presets; clones often have none.
    pub preview_url: Option<String>,
}

impl Voice {
    /// Sorts for the picker: the user's own voices first, then by name.
    pub fn sort_for_picker(voices: &mut [Voice]) {
        voices.sort_by(|a, b| {
            a.category.cmp(&b.category).then_with(|| {
                a.reference
                    .name()
                    .to_lowercase()
                    .cmp(&b.reference.name().to_lowercase())
            })
        });
    }
}

/// Lists the voices a provider account can use. Calls the network and
/// blocks, so it runs off the UI thread.
pub trait VoiceLibrary: Send + Sync {
    fn voices(&self, key: &ApiKey) -> Result<Vec<Voice>, ProviderFailure>;
}

impl<T: VoiceLibrary + ?Sized> VoiceLibrary for Arc<T> {
    fn voices(&self, key: &ApiKey) -> Result<Vec<Voice>, ProviderFailure> {
        (**self).voices(key)
    }
}

/// Downloads a voice's stock preview (`Voice::preview_url`). The link is
/// public: no key goes with it. Calls the network and blocks, so it runs
/// off the UI thread.
pub trait VoicePreviews: Send + Sync {
    /// The preview's MP3 audio.
    fn download(&self, url: &str) -> Result<Vec<u8>, ProviderFailure>;
}

impl<T: VoicePreviews + ?Sized> VoicePreviews for Arc<T> {
    fn download(&self, url: &str) -> Result<Vec<u8>, ProviderFailure> {
        (**self).download(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_keeps_the_provider_id_and_name() {
        let voice = VoiceRef::elevenlabs(" FrS6cKLB1wg4WYgPa9GW ", " Wyatt ").unwrap();
        assert_eq!(voice.provider(), Provider::ElevenLabs);
        assert_eq!(voice.id(), "FrS6cKLB1wg4WYgPa9GW");
        assert_eq!(voice.name(), "Wyatt");
    }

    #[test]
    fn only_voice_providers_hold_voices() {
        assert_eq!(
            VoiceRef::new(Provider::Claude, "abc", "x"),
            Err(InvalidVoiceRef::NotAVoiceProvider(Provider::Claude))
        );
    }

    #[test]
    fn ids_are_short_url_safe_tokens() {
        for bad in ["", "  ", "has space", "a/b", "ação", &"x".repeat(65)] {
            assert_eq!(
                VoiceRef::elevenlabs(bad, "n"),
                Err(InvalidVoiceRef::InvalidId),
                "{bad}"
            );
        }
        assert!(VoiceRef::elevenlabs("abc-DEF_123", "n").is_ok());
    }

    #[test]
    fn a_blank_name_falls_back_to_the_id_and_long_names_are_cut() {
        assert_eq!(
            VoiceRef::elevenlabs("abc123", " ").unwrap().name(),
            "abc123"
        );
        let long = "é".repeat(VoiceRef::MAX_NAME_CHARS + 5);
        let name = VoiceRef::elevenlabs("abc123", &long)
            .unwrap()
            .name()
            .to_owned();
        assert_eq!(name.chars().count(), VoiceRef::MAX_NAME_CHARS);
    }

    #[test]
    fn the_same_voice_ignores_the_recorded_name() {
        let a = VoiceRef::elevenlabs("abc123", "Old name").unwrap();
        let b = VoiceRef::elevenlabs("abc123", "New name").unwrap();
        assert!(a.same_voice(&b));
        assert!(!a.same_voice(&VoiceRef::elevenlabs("xyz789", "Old name").unwrap()));
    }

    #[test]
    fn the_picker_lists_own_voices_first_then_by_name() {
        let voice = |name: &str, category| Voice {
            reference: VoiceRef::elevenlabs(&name.to_lowercase(), name).unwrap(),
            category,
            description: String::new(),
            labels: vec![],
            preview_url: None,
        };
        let mut voices = vec![
            voice("Wyatt", VoiceCategory::Default),
            voice("bella", VoiceCategory::Default),
            voice("Mine", VoiceCategory::Cloned),
            voice("Library", VoiceCategory::Other),
            voice("Designed", VoiceCategory::Generated),
        ];
        Voice::sort_for_picker(&mut voices);
        let names: Vec<_> = voices.iter().map(|v| v.reference.name()).collect();
        assert_eq!(names, ["Mine", "Designed", "bella", "Wyatt", "Library"]);
    }
}
