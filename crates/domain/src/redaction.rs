use std::sync::{Arc, RwLock};

use zeroize::Zeroize;

use crate::ApiKey;

/// Masks known secrets in text bound for logs, the database or the screen
/// (PRD story 2). Clones share one set of secrets, so a key saved in
/// settings is masked by every log writer from then on.
#[derive(Clone, Default)]
pub struct Redactor {
    /// Longest first, so a whole key is masked before any of its parts.
    secrets: Arc<RwLock<Vec<String>>>,
}

impl Redactor {
    pub const MASK: &str = "[redacted]";

    pub fn new() -> Self {
        Self::default()
    }

    /// Starts masking `key`. Keys stay known after they are removed or
    /// replaced, because older text may still carry them.
    pub fn add(&self, key: &ApiKey) {
        self.add_parts(&key.sensitive_parts());
    }

    /// Starts masking each of `parts`: the pieces of app credentials or
    /// OAuth tokens. Parts shorter than `ApiKey::MIN_CHARS` are skipped,
    /// since masking very short strings would mangle ordinary text.
    pub fn add_parts(&self, parts: &[&str]) {
        let mut secrets = self
            .secrets
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for part in parts {
            if part.len() >= ApiKey::MIN_CHARS && !secrets.iter().any(|known| known == part) {
                secrets.push((*part).to_owned());
            }
        }
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    }

    /// `text` with every known secret replaced by `MASK`.
    pub fn redact(&self, text: &str) -> String {
        let secrets = self
            .secrets
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        secrets.iter().fold(text.to_owned(), |text, secret| {
            if text.contains(secret.as_str()) {
                text.replace(secret.as_str(), Self::MASK)
            } else {
                text
            }
        })
    }
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self
            .secrets
            .read()
            .map(|secrets| secrets.len())
            .unwrap_or_default();
        write!(f, "Redactor({count} secrets)")
    }
}

impl Drop for Redactor {
    fn drop(&mut self) {
        // Only the last handle wipes the shared set.
        if let Some(lock) = Arc::get_mut(&mut self.secrets) {
            let secrets = lock
                .get_mut()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for secret in secrets.iter_mut() {
                secret.zeroize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Provider;

    fn key(provider: Provider, text: &str) -> ApiKey {
        ApiKey::parse(provider, text).unwrap()
    }

    #[test]
    fn unknown_text_passes_through() {
        let redactor = Redactor::new();
        assert_eq!(redactor.redact("nothing secret"), "nothing secret");
    }

    #[test]
    fn every_occurrence_of_a_known_key_is_masked() {
        let redactor = Redactor::new();
        redactor.add(&key(Provider::Claude, "sk-ant-secret-1234"));
        assert_eq!(
            redactor.redact("x-api-key: sk-ant-secret-1234, again sk-ant-secret-1234."),
            "x-api-key: [redacted], again [redacted]."
        );
    }

    #[test]
    fn clones_share_the_known_secrets() {
        let redactor = Redactor::new();
        let log_writer = redactor.clone();
        redactor.add(&key(Provider::Gemini, "AIzaSecret12345"));
        assert_eq!(log_writer.redact("key=AIzaSecret12345"), "key=[redacted]");
    }

    #[test]
    fn a_compound_key_is_masked_whole_and_by_part() {
        let redactor = Redactor::new();
        redactor.add(&key(Provider::Higgsfield, "key-id-123:secret-456789"));
        assert_eq!(
            redactor.redact("Authorization: Key key-id-123:secret-456789"),
            "Authorization: Key [redacted]"
        );
        assert_eq!(
            redactor.redact("hf-secret: secret-456789"),
            "hf-secret: [redacted]"
        );
    }

    #[test]
    fn longer_secrets_win_over_secrets_they_contain() {
        let redactor = Redactor::new();
        redactor.add(&key(Provider::Claude, "abcdefgh"));
        redactor.add(&key(Provider::Gemini, "abcdefgh-longer"));
        assert_eq!(redactor.redact("abcdefgh-longer"), "[redacted]");
    }

    #[test]
    fn credential_and_token_parts_are_masked_but_short_ones_skipped() {
        let redactor = Redactor::new();
        redactor.add_parts(&["ya29.access-token-0001", "1//refresh-0001", "abc"]);
        assert_eq!(
            redactor.redact("Bearer ya29.access-token-0001 refresh=1//refresh-0001 abc"),
            "Bearer [redacted] refresh=[redacted] abc"
        );
    }

    #[test]
    fn debug_output_shows_no_secret() {
        let redactor = Redactor::new();
        redactor.add(&key(Provider::Claude, "sk-ant-secret-1234"));
        let shown = format!("{redactor:?}");
        assert!(!shown.contains("sk-ant"), "{shown}");
    }
}
