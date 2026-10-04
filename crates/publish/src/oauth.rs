//! The OAuth core every network reuses (ADR-0008): PKCE and `state`.
//!
//! The verifier and `state` come from the operating system's random
//! generator. The challenge encoding is per network: base64url for Google
//! and Meta, hex for TikTok.

use std::time::Duration;

use bardo_domain::{SecretText, SignInFailure, SignInFailureKind, TokenGrant};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
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

fn grant_unexpected(detail: &str) -> SignInFailure {
    SignInFailure::new(SignInFailureKind::Unexpected, detail)
}

/// The scopes, access and refresh token a token endpoint granted. Scopes
/// are split on spaces (RFC 6749) and on commas (TikTok).
pub(crate) fn parse_grant(body: &str) -> Result<TokenGrant, SignInFailure> {
    let body: Value = serde_json::from_str(body)
        .map_err(|error| grant_unexpected(&format!("unreadable token answer: {error}")))?;
    let token = |name: &str| {
        body[name]
            .as_str()
            .filter(|token| !token.is_empty() && token.chars().all(|c| c.is_ascii_graphic()))
            .map(SecretText::new)
    };
    let access_token = token("access_token")
        .ok_or_else(|| grant_unexpected("the token answer has no access token"))?;
    if !body["token_type"]
        .as_str()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"))
    {
        return Err(grant_unexpected("the token answer is not a bearer token"));
    }
    let expires_in = body["expires_in"]
        .as_u64()
        .filter(|secs| *secs > 0)
        .ok_or_else(|| grant_unexpected("the token answer has no lifetime"))?;
    Ok(TokenGrant {
        access_token,
        refresh_token: token("refresh_token"),
        expires_in: Duration::from_secs(expires_in),
        scopes: body["scope"]
            .as_str()
            .unwrap_or_default()
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|scope| !scope.is_empty())
            .map(str::to_owned)
            .collect(),
    })
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
    fn granted_scopes_split_on_spaces_and_commas() {
        let google = parse_grant(
            r#"{"access_token":"a","token_type":"Bearer","expires_in":1,"scope":"x y"}"#,
        )
        .unwrap();
        assert_eq!(google.scopes, ["x", "y"]);
        let tiktok = parse_grant(
            r#"{"access_token":"a","token_type":"Bearer","expires_in":1,"scope":"user.info.basic,video.upload"}"#,
        )
        .unwrap();
        assert_eq!(tiktok.scopes, ["user.info.basic", "video.upload"]);
    }

    #[test]
    fn secrets_compare_whole() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
        assert!(!same_secret("", "a"));
    }
}
