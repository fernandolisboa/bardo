//! Providers Bardo signs in to instead of saving a key (ADR-0028): the
//! Higgsfield plan, whose credits are spent through Higgsfield's MCP
//! server. Signing in is OAuth 2.1 the way MCP asks for it: the app
//! registers itself as a client (dynamic client registration), the user
//! consents in the browser, and the code comes back to the loopback
//! listener networks use. The client id and the tokens are kept together
//! in the secret store; the client id is needed again for every refresh.

use std::fmt;

use zeroize::Zeroizing;

use crate::{ConsentRequest, Provider, SecretText, SignInFailure, TokenGrant, TokenSet};

/// A plan's credits, in hundredths: Higgsfield prices some generations in
/// half credits (17.5), never finer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Credits(u64);

impl Credits {
    pub const ZERO: Credits = Credits(0);

    pub const fn from_hundredths(hundredths: u64) -> Self {
        Credits(hundredths)
    }

    /// The credits a provider wrote as a JSON number. Negative, missing
    /// or not finite is `None`; finer than hundredths is rounded up, so an
    /// estimate never comes out low.
    pub fn from_number(value: f64) -> Option<Self> {
        (value.is_finite() && (0.0..1e15).contains(&value))
            .then(|| Credits((value * 100.0 - 1e-6).ceil().max(0.0) as u64))
    }

    pub fn hundredths(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Credits {
    /// Whole credits without decimals, others with as few as they need:
    /// `10`, `17.5`, `0.25`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (whole, cents) = (self.0 / 100, self.0 % 100);
        match cents {
            0 => write!(f, "{whole}"),
            cents if cents % 10 == 0 => write!(f, "{whole}.{}", cents / 10),
            cents => write!(f, "{whole}.{cents:02}"),
        }
    }
}

/// The account a plan's tokens act as, as its provider describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanAccount {
    /// The plan's name as the provider gives it (`free`, `plus`, `ultra`).
    pub plan: String,
    /// The credits left to spend.
    pub credits: Credits,
}

/// What the secret store keeps for a plan: the client id Bardo registered
/// at sign-in and the tokens it got.
#[derive(Clone, PartialEq, Eq)]
pub struct PlanSession {
    client_id: String,
    tokens: TokenSet,
}

impl PlanSession {
    const FORMAT: &str = "bardo-plan-1";

    /// `None` when the client id is empty or has characters a client id
    /// never has.
    pub fn new(client_id: &str, tokens: TokenSet) -> Option<Self> {
        is_client_id(client_id).then(|| Self {
            client_id: client_id.to_owned(),
            tokens,
        })
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn tokens(&self) -> &TokenSet {
        &self.tokens
    }

    /// The same client with renewed tokens.
    pub fn with_tokens(&self, tokens: TokenSet) -> Self {
        Self {
            client_id: self.client_id.clone(),
            tokens,
        }
    }

    /// The form the secret store keeps: the format, the client id, then
    /// the token set as `TokenSet::encode` writes it. Over
    /// `TokenSet::MAX_STORED_BYTES` it is refused.
    pub fn encode(&self) -> Result<Zeroizing<String>, crate::TokenSetTooLarge> {
        let tokens = self.tokens.encode()?;
        let encoded = Zeroizing::new(format!("{}\n{}\n{}", Self::FORMAT, self.client_id, *tokens));
        if encoded.len() > TokenSet::MAX_STORED_BYTES {
            return Err(crate::TokenSetTooLarge {
                bytes: encoded.len(),
                max: TokenSet::MAX_STORED_BYTES,
            });
        }
        Ok(encoded)
    }

    /// Reads what `encode` wrote; `None` for anything else.
    pub fn decode(stored: &str) -> Option<Self> {
        let (format, rest) = stored.split_once('\n')?;
        if format != Self::FORMAT {
            return None;
        }
        let (client_id, tokens) = rest.split_once('\n')?;
        Self::new(client_id, TokenSet::decode(tokens)?)
    }

    /// Every string the redactor must mask.
    pub fn sensitive_parts(&self) -> Vec<&str> {
        self.tokens.sensitive_parts()
    }
}

impl fmt::Debug for PlanSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlanSession")
            .field("client_id", &self.client_id)
            .field("tokens", &self.tokens)
            .finish()
    }
}

/// Client ids are short identifiers; anything with spaces, line breaks or
/// past a few hundred characters is not one.
fn is_client_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 256
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~:".contains(c))
}

/// A provider's sign-in (ADR-0028). Calls the provider and blocks, so the
/// app runs it off the UI thread.
pub trait PlanSignIn: Send + Sync {
    fn provider(&self) -> Provider;

    /// The path the provider sends the browser back to on the loopback
    /// listener.
    fn redirect_path(&self) -> &'static str {
        "/callback"
    }

    /// Registers Bardo as a client that returns to `redirect_uri`, and
    /// gives its client id.
    fn register(&self, redirect_uri: &str) -> Result<String, SignInFailure>;

    /// A fresh PKCE verifier and `state`, and the consent address that
    /// carries them.
    fn consent_request(&self, client_id: &str, redirect_uri: &str) -> ConsentRequest;

    /// Trades the code the callback brought for tokens.
    fn exchange(
        &self,
        client_id: &str,
        code: &SecretText,
        verifier: &SecretText,
        redirect_uri: &str,
    ) -> Result<TokenGrant, SignInFailure>;

    /// Renews the tokens. The grant replaces the access token, and the
    /// refresh token when it carries one.
    fn refresh(&self, client_id: &str, refresh_token: &str) -> Result<TokenGrant, SignInFailure>;

    /// The plan and the credits the access token reaches.
    fn account(&self, access_token: &str) -> Result<PlanAccount, SignInFailure>;
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    fn tokens() -> TokenSet {
        TokenSet::granted(
            &TokenGrant {
                access_token: SecretText::new("access-0001"),
                refresh_token: Some(SecretText::new("refresh-0001")),
                expires_in: Duration::from_secs(3_600),
                scopes: vec!["openid".into()],
            },
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        )
    }

    #[test]
    fn credits_keep_half_credits_and_print_without_trailing_zeros() {
        assert_eq!(Credits::from_number(17.5).unwrap().to_string(), "17.5");
        assert_eq!(Credits::from_number(10.0).unwrap().to_string(), "10");
        assert_eq!(Credits::from_number(0.25).unwrap().to_string(), "0.25");
        assert_eq!(Credits::from_number(0.001).unwrap().hundredths(), 1);
        assert_eq!(Credits::from_number(-1.0), None);
        assert_eq!(Credits::from_number(f64::NAN), None);
    }

    #[test]
    fn a_session_round_trips_through_the_secret_store_form() {
        let session = PlanSession::new("qUWc3yOYYb9VDWGT", tokens()).unwrap();
        let stored = session.encode().unwrap();
        assert!(stored.starts_with("bardo-plan-1\nqUWc3yOYYb9VDWGT\nbardo-tokens-1\n"));
        assert_eq!(PlanSession::decode(&stored), Some(session));
        assert_eq!(PlanSession::decode("bardo-tokens-1\n1\na\nb"), None);
        assert_eq!(PlanSession::decode(""), None);
    }

    #[test]
    fn client_ids_are_identifiers() {
        assert!(PlanSession::new("abc-123_x", tokens()).is_some());
        assert!(PlanSession::new("", tokens()).is_none());
        assert!(PlanSession::new("a b", tokens()).is_none());
        assert!(PlanSession::new("a\nb", tokens()).is_none());
    }

    #[test]
    fn debug_never_shows_the_tokens() {
        let session = PlanSession::new("client", tokens()).unwrap();
        let shown = format!("{session:?}");
        assert!(!shown.contains("access-0001") && !shown.contains("refresh-0001"));
    }
}
