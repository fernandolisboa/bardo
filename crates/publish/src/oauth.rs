//! The OAuth core every network reuses (ADR-0008): PKCE and `state`.
//!
//! The verifier and `state` come from the operating system's random
//! generator. The challenge encoding is per network: base64url for Google
//! and Meta, hex for TikTok.

use bardo_domain::SecretText;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// How a network wants the SHA-256 of the verifier written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeEncoding {
    /// RFC 7636 `S256`: base64url without padding (Google, Meta).
    Base64Url,
    /// Lowercase hex (TikTok Login Kit for Desktop).
    Hex,
}

/// A PKCE verifier and its `S256` challenge.
#[derive(Debug, Clone)]
pub struct Pkce {
    verifier: SecretText,
    challenge: String,
}

impl Pkce {
    /// 48 random bytes make a 64-character verifier, inside RFC 7636's 43
    /// to 128 unreserved characters.
    const VERIFIER_BYTES: usize = 48;
    pub const METHOD: &str = "S256";

    pub fn generate(encoding: ChallengeEncoding) -> Self {
        let bytes = random::<{ Self::VERIFIER_BYTES }>();
        Self::from_verifier(&URL_SAFE_NO_PAD.encode(bytes), encoding)
    }

    /// The challenge for a known verifier, for tests against published
    /// examples.
    pub fn from_verifier(verifier: &str, encoding: ChallengeEncoding) -> Self {
        let digest = Sha256::digest(verifier.as_bytes());
        let challenge = match encoding {
            ChallengeEncoding::Base64Url => URL_SAFE_NO_PAD.encode(digest),
            ChallengeEncoding::Hex => digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        };
        Self {
            verifier: SecretText::new(verifier),
            challenge,
        }
    }

    pub fn verifier(&self) -> &SecretText {
        &self.verifier
    }

    pub fn challenge(&self) -> &str {
        &self.challenge
    }

    pub fn into_verifier(self) -> SecretText {
        self.verifier
    }
}

/// A fresh `state`: 32 random bytes, base64url. The callback must carry it
/// back, which ties the browser's answer to this sign-in.
pub fn new_state() -> SecretText {
    SecretText::new(URL_SAFE_NO_PAD.encode(random::<32>()))
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    // Without the operating system's random generator no sign-in can be
    // safe, and Windows always has one.
    getrandom::fill(&mut bytes).expect("the operating system's random generator works");
    bytes
}

/// Compares secrets in time that does not depend on where they differ.
pub fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |diff, (x, y)| diff | (x ^ y))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_matches_rfc_7636_appendix_b() {
        let pkce = Pkce::from_verifier(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            ChallengeEncoding::Base64Url,
        );
        assert_eq!(
            pkce.challenge(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn the_hex_challenge_is_the_same_digest_in_hex() {
        let pkce = Pkce::from_verifier(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            ChallengeEncoding::Hex,
        );
        assert_eq!(
            pkce.challenge(),
            "13d31e961a1ad8ec2f16b10c4c982e0876a878ad6df144566ee1894acb70f9c3"
        );
    }

    #[test]
    fn a_generated_verifier_uses_unreserved_characters_within_the_limits() {
        let pkce = Pkce::generate(ChallengeEncoding::Base64Url);
        let verifier = pkce.verifier().expose();
        assert!((43..=128).contains(&verifier.len()), "{}", verifier.len());
        assert!(
            verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)),
            "{verifier}"
        );
        assert_eq!(
            Pkce::from_verifier(verifier, ChallengeEncoding::Base64Url).challenge(),
            pkce.challenge()
        );
        assert!(!pkce.challenge().contains('='));
    }

    #[test]
    fn verifiers_and_states_are_fresh_every_time() {
        let a = Pkce::generate(ChallengeEncoding::Base64Url);
        let b = Pkce::generate(ChallengeEncoding::Base64Url);
        assert_ne!(a.verifier(), b.verifier());
        let (s, t) = (new_state(), new_state());
        assert_ne!(s, t);
        assert_eq!(s.expose().len(), 43);
    }

    #[test]
    fn secrets_compare_whole() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
        assert!(!same_secret("", "a"));
    }
}
