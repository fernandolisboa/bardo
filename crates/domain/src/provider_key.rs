use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use zeroize::Zeroize;

use crate::ProfileId;

/// A paid service Bardo calls with the user's own API key (PRD story 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Provider {
    /// Anthropic's Claude API: scripts, titles, descriptions, prompts.
    Claude,
    /// Narration and the user's voices.
    ElevenLabs,
    /// Google's Gemini API: images (Nano Banana) and video (Veo).
    Gemini,
    /// Video clips through the models Higgsfield aggregates.
    Higgsfield,
    /// The decision engine (JEV).
    TypeSafe,
    /// Market research and public video statistics.
    YouTubeData,
}

impl Provider {
    /// In the order the settings screen lists them.
    pub const ALL: [Provider; 6] = [
        Provider::Claude,
        Provider::ElevenLabs,
        Provider::Gemini,
        Provider::Higgsfield,
        Provider::TypeSafe,
        Provider::YouTubeData,
    ];

    /// Stable identifier for storage and resource keys. Never change one.
    pub fn code(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::ElevenLabs => "elevenlabs",
            Provider::Gemini => "gemini",
            Provider::Higgsfield => "higgsfield",
            Provider::TypeSafe => "typesafe",
            Provider::YouTubeData => "youtube-data",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown provider: {0}")]
pub struct UnknownProvider(pub String);

impl FromStr for Provider {
    type Err = UnknownProvider;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Provider::ALL
            .into_iter()
            .find(|provider| provider.code() == s)
            .ok_or_else(|| UnknownProvider(s.to_owned()))
    }
}

/// Why typed text is not a usable key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ApiKeyError {
    #[error("the key is empty")]
    Required,
    #[error("the key is too short")]
    TooShort,
    #[error("the key is too long")]
    TooLong,
    /// Spaces, line breaks or characters outside printable ASCII. Keys are
    /// sent in HTTP headers, which allow neither.
    #[error("the key has characters keys never have")]
    InvalidCharacters,
    /// Higgsfield credentials are a key ID and a secret, joined by a colon.
    #[error("expected KEY_ID:KEY_SECRET")]
    NotIdAndSecret,
}

impl ApiKeyError {
    pub const ALL: [ApiKeyError; 5] = [
        ApiKeyError::Required,
        ApiKeyError::TooShort,
        ApiKeyError::TooLong,
        ApiKeyError::InvalidCharacters,
        ApiKeyError::NotIdAndSecret,
    ];
}

/// A provider API key. Its `Debug` output never shows the key, it has no
/// `Display`, and its memory is wiped when dropped, so the only way to the
/// characters is `expose`, called where the key goes on the wire or into the
/// secret store.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// No provider issues shorter keys; a shorter value is a paste mistake.
    /// The redactor also relies on it: masking very short strings would
    /// mangle ordinary text.
    pub const MIN_CHARS: usize = 8;
    pub const MAX_CHARS: usize = 512;

    /// Validates a key as the user typed or pasted it. Surrounding
    /// whitespace (a pasted line break) is dropped.
    pub fn parse(provider: Provider, input: &str) -> Result<Self, ApiKeyError> {
        let key = input.trim();
        if key.is_empty() {
            return Err(ApiKeyError::Required);
        }
        if !key.chars().all(|c| c.is_ascii_graphic()) {
            return Err(ApiKeyError::InvalidCharacters);
        }
        if key.len() < Self::MIN_CHARS {
            return Err(ApiKeyError::TooShort);
        }
        if key.len() > Self::MAX_CHARS {
            return Err(ApiKeyError::TooLong);
        }
        if provider == Provider::Higgsfield {
            match key.split_once(':') {
                Some((id, secret))
                    if !id.is_empty() && !secret.is_empty() && !secret.contains(':') => {}
                _ => return Err(ApiKeyError::NotIdAndSecret),
            }
        }
        Ok(Self(key.to_owned()))
    }

    /// The key itself. Only for the request header or the secret store.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The last four characters, so the user can tell which key is saved
    /// without the screen showing it.
    pub fn hint(&self) -> String {
        let start = self.0.len().saturating_sub(4);
        format!("…{}", &self.0[start..])
    }

    /// Every string that must never reach a log: the whole key and, for
    /// keys made of parts (`KEY_ID:KEY_SECRET`), each part long enough to
    /// mask safely, since a part can travel alone.
    pub fn sensitive_parts(&self) -> Vec<&str> {
        let mut parts = vec![self.0.as_str()];
        if self.0.contains(':') {
            parts.extend(
                self.0
                    .split(':')
                    .filter(|part| part.len() >= Self::MIN_CHARS),
            );
        }
        parts
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

impl Drop for ApiKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// The secret store failed. Adapters must not put key material in the
/// cause; the app redacts it again before logging anyway.
#[derive(Debug, thiserror::Error)]
#[error("secret store failed: {0}")]
pub struct SecretStoreError(#[from] pub Box<dyn std::error::Error + Send + Sync>);

/// Where provider keys live: the operating system's credential store for
/// the signed-in user (Windows Credential Manager), never files or the
/// database (ADR-0001). Keys belong to a profile, so more profiles can have
/// their own later.
pub trait SecretStore {
    fn get(&self, owner: ProfileId, provider: Provider)
    -> Result<Option<ApiKey>, SecretStoreError>;

    /// Saves the key, replacing any earlier one.
    fn set(
        &self,
        owner: ProfileId,
        provider: Provider,
        key: &ApiKey,
    ) -> Result<(), SecretStoreError>;

    /// Removes the key. Removing a key that is not there succeeds.
    fn delete(&self, owner: ProfileId, provider: Provider) -> Result<(), SecretStoreError>;
}

impl<T: SecretStore + ?Sized> SecretStore for Arc<T> {
    fn get(
        &self,
        owner: ProfileId,
        provider: Provider,
    ) -> Result<Option<ApiKey>, SecretStoreError> {
        (**self).get(owner, provider)
    }

    fn set(
        &self,
        owner: ProfileId,
        provider: Provider,
        key: &ApiKey,
    ) -> Result<(), SecretStoreError> {
        (**self).set(owner, provider, key)
    }

    fn delete(&self, owner: ProfileId, provider: Provider) -> Result<(), SecretStoreError> {
        (**self).delete(owner, provider)
    }
}

/// What a provider said about a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCheckOutcome {
    /// The provider accepted the key.
    Valid,
    /// Wrong, revoked or expired key.
    Rejected,
    /// The key is genuine but may not do what Bardo needs (missing
    /// permission, API not enabled for the project, IP restriction).
    NotAllowed,
    /// The key is genuine but a rate limit, quota or credit balance stops it
    /// right now.
    LimitReached,
    /// The provider is failing on its side.
    ProviderDown,
    /// No answer: offline, DNS, TLS, proxy or timeout.
    Unreachable,
    /// An answer this check does not understand.
    Unexpected,
}

impl KeyCheckOutcome {
    pub const ALL: [KeyCheckOutcome; 7] = [
        KeyCheckOutcome::Valid,
        KeyCheckOutcome::Rejected,
        KeyCheckOutcome::NotAllowed,
        KeyCheckOutcome::LimitReached,
        KeyCheckOutcome::ProviderDown,
        KeyCheckOutcome::Unreachable,
        KeyCheckOutcome::Unexpected,
    ];

    /// Stable identifier for resource keys and logs.
    pub fn code(self) -> &'static str {
        match self {
            KeyCheckOutcome::Valid => "valid",
            KeyCheckOutcome::Rejected => "rejected",
            KeyCheckOutcome::NotAllowed => "not_allowed",
            KeyCheckOutcome::LimitReached => "limit_reached",
            KeyCheckOutcome::ProviderDown => "provider_down",
            KeyCheckOutcome::Unreachable => "unreachable",
            KeyCheckOutcome::Unexpected => "unexpected",
        }
    }
}

/// The result of testing a key with a cheap authenticated call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCheck {
    pub outcome: KeyCheckOutcome,
    /// The provider's own message or the network error, in English, for the
    /// user and the log. Callers redact it before showing it.
    pub detail: Option<String>,
}

impl KeyCheck {
    pub fn new(outcome: KeyCheckOutcome, detail: Option<String>) -> Self {
        Self { outcome, detail }
    }
}

/// Tests keys against the providers. Calls the network and blocks, so the
/// app runs it off the UI thread.
pub trait KeyChecker: Send + Sync {
    fn check(&self, provider: Provider, key: &ApiKey) -> KeyCheck;
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-ant-api03-abcdefghijklmnop";

    #[test]
    fn provider_codes_round_trip() {
        for provider in Provider::ALL {
            assert_eq!(provider.code().parse::<Provider>(), Ok(provider));
        }
        assert!("openai".parse::<Provider>().is_err());
    }

    #[test]
    fn a_pasted_key_loses_surrounding_whitespace() {
        let key = ApiKey::parse(Provider::Claude, &format!("  {KEY}\r\n")).unwrap();
        assert_eq!(key.expose(), KEY);
    }

    #[test]
    fn empty_and_blank_keys_are_required() {
        for input in ["", "   ", "\n"] {
            assert_eq!(
                ApiKey::parse(Provider::Claude, input),
                Err(ApiKeyError::Required)
            );
        }
    }

    #[test]
    fn inner_spaces_and_non_ascii_are_rejected() {
        for input in ["sk-ant abcdefgh", "sk-ant\tabcdefgh", "sk-ant-ábcdefgh"] {
            assert_eq!(
                ApiKey::parse(Provider::Claude, input),
                Err(ApiKeyError::InvalidCharacters),
                "{input:?}"
            );
        }
    }

    #[test]
    fn length_limits() {
        let short = "x".repeat(ApiKey::MIN_CHARS - 1);
        assert_eq!(
            ApiKey::parse(Provider::Gemini, &short),
            Err(ApiKeyError::TooShort)
        );
        assert!(ApiKey::parse(Provider::Gemini, &"x".repeat(ApiKey::MIN_CHARS)).is_ok());
        assert!(ApiKey::parse(Provider::Gemini, &"x".repeat(ApiKey::MAX_CHARS)).is_ok());
        assert_eq!(
            ApiKey::parse(Provider::Gemini, &"x".repeat(ApiKey::MAX_CHARS + 1)),
            Err(ApiKeyError::TooLong)
        );
    }

    #[test]
    fn higgsfield_needs_an_id_and_a_secret() {
        assert!(ApiKey::parse(Provider::Higgsfield, "key-id-1:secret-abc").is_ok());
        for input in ["only-a-secret", ":secret-abc", "key-id-1:", "a:b:c-defgh"] {
            assert_eq!(
                ApiKey::parse(Provider::Higgsfield, input),
                Err(ApiKeyError::NotIdAndSecret),
                "{input}"
            );
        }
        // Other providers do not care about colons.
        assert!(ApiKey::parse(Provider::Claude, "a:b:c-defgh").is_ok());
    }

    #[test]
    fn debug_output_never_shows_the_key() {
        let key = ApiKey::parse(Provider::Claude, KEY).unwrap();
        let shown = format!("{key:?} {:?}", Some(&key));
        assert!(!shown.contains(KEY), "{shown}");
        assert!(shown.contains("redacted"), "{shown}");
    }

    #[test]
    fn hint_shows_only_the_last_four_characters() {
        let key = ApiKey::parse(Provider::Claude, KEY).unwrap();
        assert_eq!(key.hint(), "…mnop");
    }

    #[test]
    fn sensitive_parts_include_each_long_part_of_a_compound_key() {
        let key = ApiKey::parse(Provider::Higgsfield, "id-12345:secret-67890").unwrap();
        assert_eq!(
            key.sensitive_parts(),
            ["id-12345:secret-67890", "id-12345", "secret-67890"]
        );
        let short_id = ApiKey::parse(Provider::Higgsfield, "id:secret-67890").unwrap();
        assert_eq!(
            short_id.sensitive_parts(),
            ["id:secret-67890", "secret-67890"]
        );
        let plain = ApiKey::parse(Provider::Claude, KEY).unwrap();
        assert_eq!(plain.sensitive_parts(), [KEY]);
    }
}
