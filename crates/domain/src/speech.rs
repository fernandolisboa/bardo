//! Speech synthesis (PRD stories 31-32): a voice provider (ElevenLabs)
//! reads text aloud with a persona's voice and says when each character is
//! spoken, so generated narration needs no speech-to-text. The interface
//! hides the provider's protocol; callers say what to read and with which
//! voice, and get MP3 audio back with its character timings.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use crate::{ApiKey, GenerationPresets, ProviderFailure, VoiceRef};

/// When one character of the spoken text is heard. Providers time
/// characters of the text they read, whitespace and punctuation included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharTiming {
    /// Usually one character; providers may group a few.
    pub text: String,
    pub start: Duration,
    pub end: Duration,
}

/// The character timings of a piece of speech, in reading order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Alignment {
    pub chars: Vec<CharTiming>,
}

impl Alignment {
    /// When the last character ends; zero when nothing is timed.
    pub fn end(&self) -> Duration {
        self.chars.iter().map(|c| c.end).max().unwrap_or_default()
    }

    /// Appends `next`, heard `offset` after this alignment's audio began,
    /// with a space between so words of the two never run together.
    pub fn append(&mut self, next: &Alignment, offset: Duration) {
        if !self.chars.is_empty() && !next.chars.is_empty() {
            self.chars.push(CharTiming {
                text: " ".into(),
                start: offset,
                end: offset,
            });
        }
        self.chars.extend(next.chars.iter().map(|c| CharTiming {
            text: c.text.clone(),
            start: c.start + offset,
            end: c.end + offset,
        }));
    }
}

/// What to read, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechRequest {
    pub voice: VoiceRef,
    pub presets: GenerationPresets,
    pub text: String,
    /// Text read just before and after this one, when a long text is read
    /// in parts, so the voice keeps its intonation across the joins. Not
    /// read aloud.
    pub previous_text: Option<String>,
    pub next_text: Option<String>,
}

/// The spoken text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speech {
    /// MP3 audio.
    pub audio: Vec<u8>,
    pub alignment: Alignment,
    /// The model that spoke, as the provider names it.
    pub model: String,
    /// What the provider billed, in its unit (ElevenLabs: characters, i.e.
    /// credits). Cost tracking builds on it.
    pub billed_characters: u64,
}

/// Reads text aloud. Calls the network and blocks, so it runs inside a job.
pub trait SpeechSynthesizer: Send + Sync {
    /// The longest text one request reads, in characters. Longer texts are
    /// read in parts (`split_for_speech`).
    fn max_chars(&self) -> usize;

    fn synthesize(&self, key: &ApiKey, request: &SpeechRequest) -> Result<Speech, ProviderFailure>;
}

impl<T: SpeechSynthesizer + ?Sized> SpeechSynthesizer for Arc<T> {
    fn max_chars(&self) -> usize {
        (**self).max_chars()
    }

    fn synthesize(&self, key: &ApiKey, request: &SpeechRequest) -> Result<Speech, ProviderFailure> {
        (**self).synthesize(key, request)
    }
}

/// Splits `text` into parts of at most `max_chars` characters to read one
/// after the other, cutting where a reader would pause: at a paragraph
/// break, else after a sentence, else between words, and mid-word only for
/// a word longer than a part. Parts are byte ranges of `text`, trimmed and
/// never empty, in order.
pub fn split_for_speech(text: &str, max_chars: usize) -> Vec<Range<usize>> {
    let max_chars = max_chars.max(1);
    let mut parts = Vec::new();
    let mut start = 0;
    loop {
        start += leading_whitespace(&text[start..]);
        if start >= text.len() {
            return parts;
        }
        let rest = &text[start..];
        let end = match rest.char_indices().nth(max_chars) {
            None => text.len(),
            Some((limit, _)) => start + cut_point(&rest[..limit]),
        };
        let trimmed = text[start..end].trim_end();
        parts.push(start..start + trimmed.len());
        start = end;
    }
}

fn leading_whitespace(text: &str) -> usize {
    text.len() - text.trim_start().len()
}

/// Where to end a part that must fit in `window`: the byte just after the
/// best pause in it. Pauses in the first half are passed over, so parts do
/// not get needlessly short.
fn cut_point(window: &str) -> usize {
    let half = window.len() / 2;
    let late = |at: &usize| *at > half;
    let paragraph = window
        .rmatch_indices("\n\n")
        .map(|(at, _)| at + 1)
        .find(late);
    let sentence = || {
        window
            .char_indices()
            .zip(window.char_indices().skip(1))
            .filter(|((_, c), (_, next))| {
                matches!(c, '.' | '!' | '?' | '…' | ';') && next.is_whitespace() || *c == '\n'
            })
            .map(|((at, c), _)| at + c.len_utf8())
            .filter(late)
            .last()
    };
    let word = || {
        window
            .char_indices()
            .filter(|(_, c)| c.is_whitespace())
            .map(|(at, _)| at)
            .rfind(|at| *at > 0)
    };
    paragraph
        .or_else(sentence)
        .or_else(word)
        .unwrap_or(window.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(text: &str, max: usize) -> Vec<&str> {
        split_for_speech(text, max)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    #[test]
    fn a_short_text_is_one_trimmed_part() {
        assert_eq!(parts("  Hello there.\n", 100), ["Hello there."]);
        assert!(parts(" \n ", 100).is_empty());
    }

    #[test]
    fn long_texts_break_at_paragraphs_first() {
        let text = "One two three. Four five.\n\nSix seven eight. Nine.";
        assert_eq!(
            parts(text, 30),
            ["One two three. Four five.", "Six seven eight. Nine."]
        );
    }

    #[test]
    fn then_after_sentences_then_between_words() {
        assert_eq!(
            parts("Alpha beta. Gamma delta epsilon.", 20),
            ["Alpha beta.", "Gamma delta epsilon."]
        );
        assert_eq!(
            parts("alpha beta gamma delta", 12),
            ["alpha beta", "gamma delta"]
        );
    }

    #[test]
    fn a_word_longer_than_a_part_is_cut() {
        assert_eq!(parts("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn limits_count_characters_not_bytes_and_parts_cover_the_text() {
        let text = "Ação é ótima. Coração não pára. Já são três.";
        let split = parts(text, 16);
        assert!(split.iter().all(|p| p.chars().count() <= 16), "{split:?}");
        assert_eq!(split.join(" "), text);
        assert_eq!(split[0], "Ação é ótima.");
    }

    #[test]
    fn early_pauses_do_not_make_tiny_parts() {
        let text = "Hi. A long sentence that keeps going on and on";
        let split = parts(text, 30);
        assert_ne!(split[0], "Hi.", "{split:?}");
        assert!(split.iter().all(|p| p.chars().count() <= 30));
    }

    #[test]
    fn appending_shifts_times_and_separates_words() {
        let timing = |text: &str, start: u64, end: u64| CharTiming {
            text: text.into(),
            start: Duration::from_millis(start),
            end: Duration::from_millis(end),
        };
        let mut first = Alignment {
            chars: vec![timing("a", 0, 100)],
        };
        let second = Alignment {
            chars: vec![timing("b", 0, 50)],
        };
        first.append(&second, Duration::from_millis(400));
        assert_eq!(
            first.chars,
            [
                timing("a", 0, 100),
                timing(" ", 400, 400),
                timing("b", 400, 450)
            ]
        );
        assert_eq!(first.end(), Duration::from_millis(450));
        assert_eq!(Alignment::default().end(), Duration::ZERO);
    }
}
