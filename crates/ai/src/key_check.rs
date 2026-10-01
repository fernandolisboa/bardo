//! Tests provider keys with the cheapest authenticated call each provider
//! offers: a read that spends no credits (one quota unit for YouTube).

use std::time::Duration;

use bardo_domain::{ApiKey, KeyCheck, KeyCheckOutcome, KeyChecker, Provider};
use serde_json::Value;

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};

/// Higgsfield has no account endpoint, so its check asks for the status of
/// a request that cannot exist: valid credentials get 404, invalid get 401.
const NO_SUCH_HIGGSFIELD_REQUEST: &str = "00000000-0000-0000-0000-000000000000";

/// Longest provider message passed on; longer ones are cut.
const MAX_DETAIL_CHARS: usize = 300;

/// The request that tests `key` with `provider`.
pub fn check_request(provider: Provider, key: &ApiKey) -> HttpRequest {
    let key = key.expose();
    match provider {
        Provider::Claude => HttpRequest::get("https://api.anthropic.com/v1/models?limit=1")
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        Provider::ElevenLabs => HttpRequest::get("https://api.elevenlabs.io/v2/voices?page_size=1")
            .header("xi-api-key", key),
        Provider::Gemini => {
            HttpRequest::get("https://generativelanguage.googleapis.com/v1beta/models?pageSize=1")
                .header("x-goog-api-key", key)
        }
        Provider::Higgsfield => HttpRequest::get(format!(
            "https://api.higgsfield.ai/requests/{NO_SUCH_HIGGSFIELD_REQUEST}/status"
        ))
        .header("authorization", format!("Key {key}")),
        Provider::TypeSafe => HttpRequest::get("https://api.typesafe.ai/v1/models")
            .header("authorization", format!("Bearer {key}")),
        // The header keeps the key out of the URL, and so out of any error
        // that quotes the URL.
        Provider::YouTubeData => {
            HttpRequest::get("https://www.googleapis.com/youtube/v3/i18nLanguages?part=snippet")
                .header("x-goog-api-key", key)
        }
    }
}

/// What a provider's answer to `check_request` means.
pub fn classify(provider: Provider, response: &HttpResponse) -> KeyCheck {
    let body: Value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
    let outcome = match (provider, response.status) {
        (_, 200..=299) => KeyCheckOutcome::Valid,
        (Provider::Higgsfield, 404) => KeyCheckOutcome::Valid,
        (Provider::Higgsfield, 403) => KeyCheckOutcome::LimitReached,
        (Provider::ElevenLabs, 401)
            if body["detail"]["status"].as_str() == Some("missing_permissions") =>
        {
            KeyCheckOutcome::NotAllowed
        }
        (_, 401) => KeyCheckOutcome::Rejected,
        (Provider::Gemini | Provider::YouTubeData, 400)
            if google_reason(&body, "API_KEY_INVALID") =>
        {
            KeyCheckOutcome::Rejected
        }
        (Provider::Gemini | Provider::YouTubeData, 403 | 429)
            if google_quota_reason(&body) || response.status == 429 =>
        {
            KeyCheckOutcome::LimitReached
        }
        (_, 403) => KeyCheckOutcome::NotAllowed,
        (_, 402 | 429) => KeyCheckOutcome::LimitReached,
        (_, 500..=599) => KeyCheckOutcome::ProviderDown,
        _ => KeyCheckOutcome::Unexpected,
    };
    let detail = message(&body).or_else(|| {
        (outcome == KeyCheckOutcome::Unexpected).then(|| format!("HTTP {}", response.status))
    });
    KeyCheck::new(outcome, detail)
}

/// Google APIs name the cause in `error.details[].reason` (ErrorInfo) and,
/// for YouTube, also in `error.errors[].reason`.
fn google_reasons(body: &Value) -> impl Iterator<Item = &str> {
    let details = body["error"]["details"].as_array().into_iter().flatten();
    let errors = body["error"]["errors"].as_array().into_iter().flatten();
    details
        .chain(errors)
        .filter_map(|entry| entry["reason"].as_str())
}

fn google_reason(body: &Value, reason: &str) -> bool {
    google_reasons(body).any(|found| found == reason)
}

fn google_quota_reason(body: &Value) -> bool {
    google_reasons(body).any(|reason| {
        matches!(
            reason,
            "quotaExceeded" | "rateLimitExceeded" | "dailyLimitExceeded" | "RATE_LIMIT_EXCEEDED"
        )
    })
}

/// The human-readable message, wherever this provider puts it.
fn message(body: &Value) -> Option<String> {
    let text = [
        &body["error"]["message"],
        &body["detail"]["message"],
        &body["detail"],
        &body["message"],
    ]
    .into_iter()
    .find_map(Value::as_str)?
    .trim();
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= MAX_DETAIL_CHARS {
        return Some(text.to_owned());
    }
    let mut cut: String = text.chars().take(MAX_DETAIL_CHARS).collect();
    cut.push('…');
    Some(cut)
}

/// Checks keys over HTTPS.
pub struct HttpKeyChecker<T = UreqTransport> {
    transport: T,
}

impl HttpKeyChecker {
    /// Long enough for a slow provider, short enough that a dead network
    /// does not leave the settings screen waiting.
    pub const TIMEOUT: Duration = Duration::from_secs(15);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for HttpKeyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> HttpKeyChecker<T> {
    pub fn with_transport(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T: Transport> KeyChecker for HttpKeyChecker<T> {
    fn check(&self, provider: Provider, key: &ApiKey) -> KeyCheck {
        match self.transport.send(&check_request(provider, key)) {
            Ok(response) => classify(provider, &response),
            Err(error) => KeyCheck::new(KeyCheckOutcome::Unreachable, Some(error.0)),
        }
    }
}
