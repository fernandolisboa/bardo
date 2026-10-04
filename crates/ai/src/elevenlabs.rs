//! ElevenLabs: the user's voices and narration.
//!
//! Voices (`GET /v2/voices`): every voice the account can use, its own
//! clones and designed voices included, page by page. Only what a persona
//! needs to point at a voice is kept: id, name, category, description,
//! labels and the link to its stock preview; samples and settings are
//! dropped.
//!
//! Stock previews (`preview_url`): a public MP3 ElevenLabs hosts for most
//! voices. Downloading one is free and sends no key.
//!
//! Narration (`POST /v1/text-to-speech/{voice_id}/with-timestamps`): the
//! text read aloud as MP3, with the time each character is spoken, in one
//! JSON answer. What the call billed comes in the `character-cost` header.
//!
//! Forced alignment (`POST /v1/forced-alignment`): a recording the user
//! made and the text it reads, sent as a form; the answer times each
//! character of the text. ElevenLabs bills it like speech-to-text, per
//! hour of audio, and says nothing of it in the answer.

use std::time::Duration;

use bardo_domain::{
    AlignedSpeech, Alignment, AlignmentRequest, ApiKey, CharTiming, Provider, ProviderFailure,
    ProviderFailureKind, Speech, SpeechAligner, SpeechRequest, SpeechSynthesizer, Voice,
    VoiceCategory, VoiceLibrary, VoicePreviews, VoiceRef,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::http::{FormPart, HttpRequest, HttpResponse, Transport, UreqTransport};
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
        preview_url: entry["preview_url"]
            .as_str()
            .map(str::trim)
            .filter(|url| is_https(url))
            .map(str::to_owned),
    })
}

/// Only HTTPS links are followed; anything else is dropped.
fn is_https(url: &str) -> bool {
    (9..=2048).contains(&url.len())
        && url
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
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

/// The largest stock preview downloaded; they are a few seconds long.
pub const MAX_PREVIEW_BYTES: u64 = 5 * 1024 * 1024;

/// Downloads stock previews over HTTPS, redirects included.
pub struct ElevenLabsPreviews<T = UreqTransport> {
    transport: T,
}

impl ElevenLabsPreviews {
    /// A preview is small; the picker waits for it.
    pub const TIMEOUT: Duration = Duration::from_secs(20);

    pub fn new() -> Self {
        Self::with_transport(
            UreqTransport::https_only(Self::TIMEOUT).with_max_body(MAX_PREVIEW_BYTES),
        )
    }
}

impl Default for ElevenLabsPreviews {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> ElevenLabsPreviews<T> {
    pub fn with_transport(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T: Transport> VoicePreviews for ElevenLabsPreviews<T> {
    fn download(&self, url: &str) -> Result<Vec<u8>, ProviderFailure> {
        if !is_https(url) {
            return Err(unexpected("the preview link is not an HTTPS link"));
        }
        // A public link: no key goes with it.
        let response = self
            .transport
            .send_for_bytes(&HttpRequest::get(url), MAX_PREVIEW_BYTES)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        match response.status {
            200..=299 if response.bytes.is_empty() => Err(unexpected("the preview is empty")),
            200..=299 => Ok(response.bytes),
            403 | 404 | 410 => Err(unexpected(
                "ElevenLabs no longer has this preview; list the voices again",
            )),
            status => Err(ProviderFailure::new(
                ProviderFailureKind::ProviderDown,
                format!("the preview download failed (HTTP {status})"),
            )),
        }
    }
}

pub const SPEECH_URL: &str = "https://api.elevenlabs.io/v1/text-to-speech";
/// The model for long-form narration: the most natural voice in every
/// language Bardo targets, Portuguese included.
pub const SPEECH_MODEL: &str = "eleven_multilingual_v2";
/// MP3 at 44.1 kHz and 128 kbit/s: every plan may ask for it.
pub const OUTPUT_FORMAT: &str = "mp3_44100_128";
/// Characters read per request. The model takes 10,000; half keeps each
/// call (and its answer, about 5 MB of audio) short enough to retry cheaply.
pub const MAX_SPEECH_CHARS: usize = 5_000;
/// Context sent around a part, so the voice keeps its intonation across
/// joins. More adds nothing the model uses.
pub const CONTEXT_CHARS: usize = 300;

/// The request that reads `request.text` aloud.
pub fn speech_request(key: &ApiKey, request: &SpeechRequest) -> HttpRequest {
    let presets = request.presets;
    let ratio = |percent: u8| f64::from(percent) / 100.0;
    let mut body = json!({
        "text": request.text,
        "model_id": SPEECH_MODEL,
        "voice_settings": {
            "stability": ratio(presets.stability),
            "similarity_boost": ratio(presets.similarity),
            "style": ratio(presets.style),
            "speed": ratio(presets.speed),
            "use_speaker_boost": true,
        },
    });
    // The end of the text before and the start of the text after.
    if let Some(previous) = &request.previous_text {
        let skip = previous.chars().count().saturating_sub(CONTEXT_CHARS);
        body["previous_text"] = json!(previous.chars().skip(skip).collect::<String>());
    }
    if let Some(next) = &request.next_text {
        body["next_text"] = json!(next.chars().take(CONTEXT_CHARS).collect::<String>());
    }
    HttpRequest::post_json(
        format!(
            "{SPEECH_URL}/{}/with-timestamps?output_format={OUTPUT_FORMAT}",
            request.voice.id()
        ),
        body.to_string(),
    )
    .header("xi-api-key", key.expose())
}

fn unexpected(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

/// Reads a successful answer: the audio, the character timings (of the
/// text as sent; of the normalized text when only those come back) and
/// what was billed (the text's length when the header is missing).
pub fn parse_speech(response: &HttpResponse, text: &str) -> Result<Speech, ProviderFailure> {
    let body: Value = serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable speech: {error}")))?;
    let audio = body["audio_base64"]
        .as_str()
        .ok_or_else(|| unexpected("the answer has no audio"))?;
    let audio = base64::engine::general_purpose::STANDARD
        .decode(audio)
        .map_err(|error| unexpected(format!("unreadable audio: {error}")))?;
    if audio.is_empty() {
        return Err(unexpected("the answer has no audio"));
    }
    let alignment = [&body["alignment"], &body["normalized_alignment"]]
        .into_iter()
        .find(|alignment| alignment.is_object())
        .ok_or_else(|| unexpected("the answer has no timings"))
        .and_then(alignment)?;
    let billed_characters = response
        .header("character-cost")
        .and_then(|cost| cost.trim().parse().ok())
        .unwrap_or(text.chars().count() as u64);
    Ok(Speech {
        audio,
        alignment,
        model: SPEECH_MODEL.to_owned(),
        billed_characters,
    })
}

fn alignment(value: &Value) -> Result<Alignment, ProviderFailure> {
    let list = |name: &str| {
        value[name]
            .as_array()
            .ok_or_else(|| unexpected(format!("the timings have no {name}")))
    };
    let (chars, starts, ends) = (
        list("characters")?,
        list("character_start_times_seconds")?,
        list("character_end_times_seconds")?,
    );
    if chars.len() != starts.len() || chars.len() != ends.len() {
        return Err(unexpected("the timings do not line up"));
    }
    let seconds = |value: &Value| {
        value
            .as_f64()
            .filter(|s| s.is_finite() && *s >= 0.0)
            .map(Duration::from_secs_f64)
            .ok_or_else(|| unexpected("a timing is not a time"))
    };
    let chars = chars
        .iter()
        .zip(starts.iter().zip(ends))
        .map(|(text, (start, end))| {
            let start = seconds(start)?;
            Ok(CharTiming {
                text: text.as_str().unwrap_or_default().to_owned(),
                start,
                end: seconds(end)?.max(start),
            })
        })
        .collect::<Result<_, ProviderFailure>>()?;
    Ok(Alignment { chars })
}

/// Reads text aloud with ElevenLabs over HTTPS.
pub struct ElevenLabsSpeech<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl ElevenLabsSpeech {
    /// A part of `MAX_SPEECH_CHARS` takes the provider a minute or two.
    pub const TIMEOUT: Duration = Duration::from_secs(300);
    /// Base64 audio of the longest part, its timings and headroom.
    pub const MAX_BODY_BYTES: u64 = 32 * 1024 * 1024;

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT).with_max_body(Self::MAX_BODY_BYTES))
    }
}

impl Default for ElevenLabsSpeech {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> ElevenLabsSpeech<T> {
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

impl<T: Transport> SpeechSynthesizer for ElevenLabsSpeech<T> {
    fn max_chars(&self) -> usize {
        MAX_SPEECH_CHARS
    }

    fn synthesize(&self, key: &ApiKey, request: &SpeechRequest) -> Result<Speech, ProviderFailure> {
        if request.voice.provider() != Provider::ElevenLabs {
            return Err(unexpected(format!(
                "{} is not an ElevenLabs voice",
                request.voice
            )));
        }
        let response = self
            .backoff
            .send(&self.transport, &speech_request(key, request), &self.sleep)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        if !(200..=299).contains(&response.status) {
            return Err(failure(Provider::ElevenLabs, &response));
        }
        parse_speech(&response, &request.text)
    }
}

pub const ALIGNMENT_URL: &str = "https://api.elevenlabs.io/v1/forced-alignment";
/// ElevenLabs names no model for forced alignment; its costs are kept
/// under this name.
pub const ALIGNMENT_MODEL: &str = "forced_alignment";
/// The largest file the endpoint takes is just under 1 GB.
pub const MAX_ALIGNMENT_BYTES: u64 = 1_000_000_000 - 1;

/// The request that aligns `request.audio` with `request.text`.
pub fn alignment_request(key: &ApiKey, request: &AlignmentRequest<'_>) -> HttpRequest {
    let content_type = match request.file_name.rsplit_once('.') {
        Some((_, extension)) if extension.eq_ignore_ascii_case("wav") => "audio/wav",
        Some((_, extension)) if extension.eq_ignore_ascii_case("mp3") => "audio/mpeg",
        _ => "application/octet-stream",
    };
    HttpRequest::post_multipart(
        ALIGNMENT_URL,
        &[
            FormPart::File {
                name: "file",
                file_name: request.file_name,
                content_type,
                bytes: request.audio,
            },
            FormPart::Text {
                name: "text",
                value: request.text,
            },
        ],
    )
    .header("xi-api-key", key.expose())
}

/// Reads a successful alignment: the timing of each character of the text.
pub fn parse_alignment(response: &HttpResponse) -> Result<AlignedSpeech, ProviderFailure> {
    let body: Value = serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable alignment: {error}")))?;
    let characters = body["characters"]
        .as_array()
        .filter(|characters| !characters.is_empty())
        .ok_or_else(|| unexpected("the answer has no timings"))?;
    let seconds = |value: &Value| {
        value
            .as_f64()
            .filter(|s| s.is_finite() && *s >= 0.0)
            .map(Duration::from_secs_f64)
            .ok_or_else(|| unexpected("a timing is not a time"))
    };
    let chars = characters
        .iter()
        .map(|entry| {
            let start = seconds(&entry["start"])?;
            Ok(CharTiming {
                text: entry["text"].as_str().unwrap_or_default().to_owned(),
                start,
                end: seconds(&entry["end"])?.max(start),
            })
        })
        .collect::<Result<_, ProviderFailure>>()?;
    Ok(AlignedSpeech {
        alignment: Alignment { chars },
        model: ALIGNMENT_MODEL.to_owned(),
    })
}

/// Times the characters of a recording with ElevenLabs over HTTPS.
pub struct ElevenLabsAlignment<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl ElevenLabsAlignment {
    /// Uploading a long WAV over a slow connection takes a while; the
    /// alignment itself is quick.
    pub const TIMEOUT: Duration = Duration::from_secs(30 * 60);
    /// Timings of the longest script Bardo keeps, with headroom.
    pub const MAX_BODY_BYTES: u64 = 32 * 1024 * 1024;

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT).with_max_body(Self::MAX_BODY_BYTES))
    }
}

impl Default for ElevenLabsAlignment {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> ElevenLabsAlignment<T> {
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

impl<T: Transport> SpeechAligner for ElevenLabsAlignment<T> {
    fn align(
        &self,
        key: &ApiKey,
        request: &AlignmentRequest<'_>,
    ) -> Result<AlignedSpeech, ProviderFailure> {
        if request.audio.len() as u64 > MAX_ALIGNMENT_BYTES {
            return Err(unexpected(
                "the recording is over 1 GB, the most ElevenLabs aligns",
            ));
        }
        let response = self
            .backoff
            .send(
                &self.transport,
                &alignment_request(key, request),
                &self.sleep,
            )
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        if !(200..=299).contains(&response.status) {
            return Err(alignment_failure(&response));
        }
        parse_alignment(&response)
    }
}

/// A refused alignment. A 400 or 422 means ElevenLabs could not use the
/// file or text (its message says which), not that the key is wrong.
fn alignment_failure(response: &HttpResponse) -> ProviderFailure {
    if matches!(response.status, 400 | 422) {
        let body: Value = serde_json::from_str(&response.body).unwrap_or_default();
        let detail = body["detail"]["message"]
            .as_str()
            .or_else(|| body["detail"][0]["msg"].as_str())
            .or_else(|| body["detail"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("HTTP {}", response.status));
        return unexpected(format!("the recording was not accepted: {detail}"));
    }
    failure(Provider::ElevenLabs, response)
}
