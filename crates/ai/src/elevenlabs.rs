//! The user's ElevenLabs voices (`GET /v2/voices`): every voice the
//! account can use, its own clones and designed voices included, page by
//! page. Only what a persona needs to point at a voice is kept: id, name,
//! category, description and labels; samples and settings are dropped.

use std::time::Duration;

use bardo_domain::{
    ApiKey, Provider, ProviderFailure, ProviderFailureKind, Voice, VoiceCategory, VoiceLibrary,
    VoiceRef,
};
use serde_json::Value;

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

pub const VOICES_URL: &str = "https://api.elevenlabs.io/v2/voices";
/// The most the endpoint returns per page.
pub const PAGE_SIZE: u32 = 100;
/// Stops a provider that keeps saying "more" from looping forever; no
/// account comes near this many voices.
pub const MAX_PAGES: usize = 20;

/// Labels shown, in this order. Providers add others; those are dropped.
const LABELS: [&str; 5] = ["gender", "age", "accent", "descriptive", "use_case"];

/// The request for one page; `page_token` continues a listing.
pub fn voices_request(key: &ApiKey, page_token: Option<&str>) -> HttpRequest {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("page_size", &PAGE_SIZE.to_string())
        // The count costs the provider work and Bardo does not show it.
        .append_pair("include_total_count", "false");
    if let Some(token) = page_token {
        query.append_pair("next_page_token", token);
    }
    HttpRequest::get(format!("{VOICES_URL}?{}", query.finish())).header("xi-api-key", key.expose())
}

/// One page of voices and the token for the next page, if any. Voices
/// whose id is not usable are skipped.
pub fn parse_page(
    response: &HttpResponse,
) -> Result<(Vec<Voice>, Option<String>), ProviderFailure> {
    let body: Value = serde_json::from_str(&response.body).map_err(|error| {
        ProviderFailure::new(
            ProviderFailureKind::Unexpected,
            format!("unreadable voice list: {error}"),
        )
    })?;
    let Some(entries) = body["voices"].as_array() else {
        return Err(ProviderFailure::new(
            ProviderFailureKind::Unexpected,
            "the answer has no voice list",
        ));
    };
    let voices = entries.iter().filter_map(voice).collect();
    let next = body["next_page_token"]
        .as_str()
        .filter(|token| body["has_more"].as_bool() == Some(true) && !token.is_empty())
        .map(str::to_owned);
    Ok((voices, next))
}

fn voice(entry: &Value) -> Option<Voice> {
    let reference = VoiceRef::elevenlabs(
        entry["voice_id"].as_str()?,
        entry["name"].as_str().unwrap_or_default(),
    )
    .ok()?;
    let category = match entry["category"].as_str() {
        Some("premade") => VoiceCategory::Default,
        Some("cloned") => VoiceCategory::Cloned,
        Some("professional") => VoiceCategory::Professional,
        Some("generated") => VoiceCategory::Generated,
        _ => VoiceCategory::Other,
    };
    let labels = LABELS
        .iter()
        .filter_map(|name| entry["labels"][name].as_str())
        .map(|label| label.trim().replace('_', " "))
        .filter(|label| !label.is_empty())
        .collect();
    Some(Voice {
        reference,
        category,
        description: entry["description"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned(),
        labels,
    })
}

/// Lists ElevenLabs voices over HTTPS.
pub struct ElevenLabsVoices<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl ElevenLabsVoices {
    /// A listing is a quick read; the picker waits for it.
    pub const TIMEOUT: Duration = Duration::from_secs(20);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for ElevenLabsVoices {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> ElevenLabsVoices<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            backoff: Backoff::default(),
            sleep: thread_sleeper(),
        }
    }

    /// Replaces how the adapter waits between retries (tests record the
    /// waits instead).
    pub fn with_sleeper(mut self, sleep: Sleeper) -> Self {
        self.sleep = sleep;
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T: Transport> VoiceLibrary for ElevenLabsVoices<T> {
    fn voices(&self, key: &ApiKey) -> Result<Vec<Voice>, ProviderFailure> {
        let mut voices: Vec<Voice> = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let response = self
                .backoff
                .send(
                    &self.transport,
                    &voices_request(key, token.as_deref()),
                    &self.sleep,
                )
                .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
            if !(200..=299).contains(&response.status) {
                return Err(failure(Provider::ElevenLabs, &response));
            }
            let (page, next) = parse_page(&response)?;
            for voice in page {
                // A voice listed twice (pages shifting meanwhile) shows once.
                if !voices
                    .iter()
                    .any(|seen| seen.reference.same_voice(&voice.reference))
                {
                    voices.push(voice);
                }
            }
            match next {
                Some(next) => token = Some(next),
                None => break,
            }
        }
        Voice::sort_for_picker(&mut voices);
        Ok(voices)
    }
}
