//! Voice samples: a short clip of a voice to hear while choosing and
//! tuning a persona, before any narration is made with it. A sample is
//! either the provider's stock preview of the voice (free, but deaf to the
//! persona's presets) or a short sentence read with the persona's voice and
//! presets (a paid call). Samples are kept on this machine by a key made of
//! everything that changes the audio, so the same voice, presets and text
//! play again without a new call.

use std::path::PathBuf;
use std::sync::Arc;

use crate::{GenerationPresets, VoiceRef};

/// Why a sample text cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidSampleText {
    #[error("the sample text is empty")]
    Empty,
    /// Samples stay short, so each one costs little.
    #[error("the sample text is too long")]
    TooLong,
}

/// The sentence a sample reads: trimmed, never empty and short.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SampleText(String);

impl SampleText {
    /// In characters. A few seconds of speech, a few cents at most.
    pub const MAX_CHARS: usize = 250;

    pub fn new(text: &str) -> Result<Self, InvalidSampleText> {
        let text = text.trim();
        if text.is_empty() {
            Err(InvalidSampleText::Empty)
        } else if text.chars().count() > Self::MAX_CHARS {
            Err(InvalidSampleText::TooLong)
        } else {
            Ok(Self(text.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a sample plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SampleSource {
    /// The provider's own recording of the voice, from its link.
    Preview { voice: VoiceRef, url: String },
    /// `text` read with `voice` and `presets` by the provider's `model`.
    Reading {
        voice: VoiceRef,
        presets: GenerationPresets,
        text: SampleText,
        model: String,
    },
}

impl SampleSource {
    pub fn voice(&self) -> &VoiceRef {
        match self {
            SampleSource::Preview { voice, .. } | SampleSource::Reading { voice, .. } => voice,
        }
    }

    /// A stable key of everything that changes the audio (FNV-1a), the
    /// same on any machine and Rust version: equal sources, equal keys.
    /// The voice's display name is left out; it changes no sound.
    pub fn key(&self) -> String {
        let parts: Vec<String> = match self {
            SampleSource::Preview { voice, url } => vec![
                "preview".into(),
                voice.provider().code().into(),
                voice.id().into(),
                url.clone(),
            ],
            SampleSource::Reading {
                voice,
                presets,
                text,
                model,
            } => vec![
                "reading".into(),
                voice.provider().code().into(),
                voice.id().into(),
                model.clone(),
                presets.stability.to_string(),
                presets.similarity.to_string(),
                presets.style.to_string(),
                presets.speed.to_string(),
                text.as_str().into(),
            ],
        };
        let hash = parts
            .iter()
            .flat_map(|part| part.bytes().chain(std::iter::once(0)))
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        format!("{hash:016x}")
    }
}

/// A sample could not be kept. The cause is kept for logs.
#[derive(Debug, thiserror::Error)]
#[error("voice sample {key}: {source}")]
pub struct VoiceSampleStoreError {
    pub key: String,
    pub source: std::io::Error,
}

/// Samples kept on this machine, by `SampleSource::key`. Old samples may be
/// dropped to bound the space they take; a dropped one is made again.
/// Shared with the threads that make samples.
pub trait VoiceSampleStore: Send + Sync {
    /// Where the sample kept under `key` is, if it is kept.
    fn find(&self, key: &str) -> Option<PathBuf>;

    /// Keeps the MP3 `audio` under `key`, replacing any sample kept under
    /// it, and says where it is. A reader never sees it half written.
    fn keep(&self, key: &str, audio: &[u8]) -> Result<PathBuf, VoiceSampleStoreError>;
}

impl<T: VoiceSampleStore + ?Sized> VoiceSampleStore for Arc<T> {
    fn find(&self, key: &str) -> Option<PathBuf> {
        (**self).find(key)
    }

    fn keep(&self, key: &str, audio: &[u8]) -> Result<PathBuf, VoiceSampleStoreError> {
        (**self).keep(key, audio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(presets: GenerationPresets, text: &str) -> SampleSource {
        SampleSource::Reading {
            voice: VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap(),
            presets,
            text: SampleText::new(text).unwrap(),
            model: "eleven_multilingual_v2".into(),
        }
    }

    #[test]
    fn sample_texts_are_trimmed_and_short() {
        assert_eq!(SampleText::new("  Olá!  ").unwrap().as_str(), "Olá!");
        assert_eq!(SampleText::new(" \n "), Err(InvalidSampleText::Empty));
        let longest = "é".repeat(SampleText::MAX_CHARS);
        assert!(SampleText::new(&longest).is_ok(), "counts characters");
        assert_eq!(
            SampleText::new(&format!("{longest}x")),
            Err(InvalidSampleText::TooLong)
        );
    }

    #[test]
    fn the_key_changes_with_anything_that_changes_the_sound() {
        let base = reading(GenerationPresets::default(), "Hello.");
        assert_eq!(
            base.key(),
            reading(GenerationPresets::default(), " Hello. ").key()
        );
        assert_eq!(base.key().len(), 16);

        let mut keys = vec![base.key()];
        let presets = GenerationPresets::default();
        for changed in [
            reading(
                GenerationPresets {
                    stability: 51,
                    ..presets
                },
                "Hello.",
            ),
            reading(
                GenerationPresets {
                    similarity: 76,
                    ..presets
                },
                "Hello.",
            ),
            reading(
                GenerationPresets {
                    style: 1,
                    ..presets
                },
                "Hello.",
            ),
            reading(
                GenerationPresets {
                    speed: 99,
                    ..presets
                },
                "Hello.",
            ),
            reading(presets, "Hello!"),
            SampleSource::Reading {
                voice: VoiceRef::elevenlabs("otherVoice01", "Wyatt").unwrap(),
                presets,
                text: SampleText::new("Hello.").unwrap(),
                model: "eleven_multilingual_v2".into(),
            },
            SampleSource::Reading {
                voice: VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap(),
                presets,
                text: SampleText::new("Hello.").unwrap(),
                model: "eleven_v3".into(),
            },
            SampleSource::Preview {
                voice: VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap(),
                url: "https://example.com/preview.mp3".into(),
            },
        ] {
            let key = changed.key();
            assert!(!keys.contains(&key), "{changed:?}");
            keys.push(key);
        }
    }

    #[test]
    fn the_key_ignores_the_voices_display_name_and_is_stable() {
        let named = |name: &str| SampleSource::Preview {
            voice: VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", name).unwrap(),
            url: "https://example.com/preview.mp3".into(),
        };
        assert_eq!(named("Wyatt").key(), named("Renamed").key());
        // Fixed on any machine and Rust version: a kept sample is found
        // after an upgrade.
        assert_eq!(named("Wyatt").key(), "ff27c0981b6aee4e");
    }
}
