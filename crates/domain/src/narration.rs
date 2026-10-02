//! Narration (PRD stories 31-33): the script read aloud with the persona's
//! voice, or recorded by the user and imported, saved as an audio asset
//! with the word timings of the text it read. Word timings come from the
//! provider's character timings (of its speech, or of its alignment of the
//! user's recording), mapped onto the script's words here; captions, cut
//! snapping and scenes build on them the same way for both.
//! A narration belongs to the text it read: once the script changes, the
//! narration is stale until it is generated again.

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{
    Alignment, GenerationPresets, JobId, ProfileId, Provider, RepositoryError, Script, ScriptText,
    VideoProjectId, VoiceRef,
};

uuid_id!(
    /// Identifies one generated narration.
    NarrationId
);

/// When one word of the narrated text is spoken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordTiming {
    /// Byte range of the word in the narrated text, punctuation attached.
    pub text: Range<usize>,
    pub start: Duration,
    pub end: Duration,
}

/// Why stored word timings do not fit their text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("word timings do not fit the narrated text")]
pub struct InvalidWordTimings;

/// The timing of every word of a text, in reading order. Always fits its
/// text: one entry per word, ranges on word boundaries, never going back in
/// time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WordTimings(Vec<WordTiming>);

/// How far ahead the mapping looks for the next word both sides share
/// after a mismatch (a number the provider spelled out, a word it read
/// differently).
const RESYNC_WINDOW: usize = 8;

impl WordTimings {
    /// Maps a provider's character timings onto the words of `text`.
    ///
    /// Words match on letters and digits only, ignoring case, accents and
    /// punctuation, so curly quotes, decomposed accents or a dropped comma
    /// in the provider's copy still match. A word is timed from its first
    /// to its last letter or digit, leaving out the pause a comma or period
    /// carries. Words the provider read differently (`1969` spoken as
    /// "nineteen sixty-nine" in normalized text) share the time between
    /// their matched neighbours, in proportion to their length.
    pub fn from_alignment(text: &str, alignment: &Alignment) -> Self {
        let words = spoken_words(text);
        let spoken = timed_words(alignment);
        let keys: Vec<String> = words
            .iter()
            .map(|range| key(&text[range.clone()]))
            .collect();

        let mut matched: Vec<Option<(Duration, Duration)>> = vec![None; words.len()];
        let (mut i, mut j) = (0, 0);
        while i < words.len() && j < spoken.len() {
            if !keys[i].is_empty() && keys[i] == spoken[j].key {
                matched[i] = Some((spoken[j].start, spoken[j].end));
                i += 1;
                j += 1;
                continue;
            }
            match resync(&keys[i..], &spoken[j..]) {
                Some((skip_words, skip_spoken)) => {
                    i += skip_words;
                    j += skip_spoken;
                }
                None => {
                    i += 1;
                    j += 1;
                }
            }
        }

        let first = alignment.chars.first().map_or(Duration::ZERO, |c| c.start);
        let last = alignment.end();
        let mut timings = Vec::with_capacity(words.len());
        let mut at = 0;
        while at < words.len() {
            if let Some((start, end)) = matched[at] {
                timings.push((start, end));
                at += 1;
                continue;
            }
            // A run of unmatched words fills the gap between its neighbours.
            let run_end = (at..words.len())
                .find(|&k| matched[k].is_some())
                .unwrap_or(words.len());
            let from = timings.last().map_or(first, |&(_, end)| end);
            let to = matched
                .get(run_end)
                .copied()
                .flatten()
                .map_or(last, |(start, _)| start)
                .max(from);
            let weights: Vec<u32> = keys[at..run_end]
                .iter()
                .map(|k| k.chars().count().max(1) as u32)
                .collect();
            let total: u32 = weights.iter().sum();
            let span = to - from;
            let mut done = 0;
            for weight in weights {
                let start = from + span * done / total;
                done += weight;
                timings.push((start, from + span * done / total));
            }
            at = run_end;
        }

        let mut floor = Duration::ZERO;
        Self(
            words
                .into_iter()
                .zip(timings)
                .map(|(range, (start, end))| {
                    let start = start.max(floor);
                    floor = start;
                    WordTiming {
                        text: range,
                        start,
                        end: end.max(start),
                    }
                })
                .collect(),
        )
    }

    /// Rebuilds stored timings, checking they still fit `text`.
    pub fn restore(text: &str, timings: Vec<WordTiming>) -> Result<Self, InvalidWordTimings> {
        let words = spoken_words(text);
        let fits = words.len() == timings.len()
            && words
                .iter()
                .zip(&timings)
                .all(|(word, timing)| *word == timing.text)
            && timings.iter().all(|t| t.start <= t.end)
            && timings
                .windows(2)
                .all(|pair| pair[0].start <= pair[1].start);
        if fits {
            Ok(Self(timings))
        } else {
            Err(InvalidWordTimings)
        }
    }

    pub fn as_slice(&self) -> &[WordTiming] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The word being spoken at `position`: the last word that started by
    /// then, so a pause keeps the word just said. `None` before the first.
    pub fn word_at(&self, position: Duration) -> Option<usize> {
        self.0
            .partition_point(|word| word.start <= position)
            .checked_sub(1)
    }
}

/// The words of `text` as a narrator reads them: split on whitespace, with
/// a lone mark that is not spoken (a dash, an ellipsis) kept with the word
/// before it, or the one after at the start.
pub fn spoken_words(text: &str) -> Vec<Range<usize>> {
    let mut words: Vec<Range<usize>> = Vec::new();
    let mut pending: Option<Range<usize>> = None;
    for (start, token) in tokens(text) {
        let range = start..start + token.len();
        let spoken = token.chars().any(char::is_alphanumeric);
        match (spoken, words.last_mut()) {
            (false, Some(last)) => last.end = range.end,
            (false, None) => {
                pending = Some(pending.map_or(range.clone(), |p| p.start..range.end));
            }
            (true, _) => words.push(pending.take().map_or(range.clone(), |p| p.start..range.end)),
        }
    }
    if let Some(pending) = pending {
        words.push(pending);
    }
    words
}

/// Whitespace-separated tokens with their byte offsets.
fn tokens(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split_whitespace()
        .map(move |token| (token.as_ptr() as usize - text.as_ptr() as usize, token))
}

/// What a word sounds like for matching: lowercase letters and digits,
/// accents dropped.
fn key(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .filter_map(fold_accent)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// The base letter of an accented Latin letter; combining marks (from
/// decomposed text) vanish.
fn fold_accent(c: char) -> Option<char> {
    if ('\u{0300}'..='\u{036f}').contains(&c) {
        return None;
    }
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    })
}

/// A word as the provider timed it.
struct TimedWord {
    key: String,
    start: Duration,
    end: Duration,
}

/// Groups character timings into words, each timed from its first to its
/// last letter or digit (all its characters when it has none).
fn timed_words(alignment: &Alignment) -> Vec<TimedWord> {
    let mut words = Vec::new();
    let mut current: Vec<&crate::CharTiming> = Vec::new();
    let mut flush = |current: &mut Vec<&crate::CharTiming>| {
        if current.is_empty() {
            return;
        }
        let text: String = current.iter().map(|c| c.text.as_str()).collect();
        let spoken: Vec<_> = current
            .iter()
            .filter(|c| c.text.chars().any(char::is_alphanumeric))
            .collect();
        let timed = if spoken.is_empty() {
            current.iter().collect()
        } else {
            spoken
        };
        words.push(TimedWord {
            key: key(&text),
            start: timed.first().map_or(Duration::ZERO, |c| c.start),
            end: timed.last().map_or(Duration::ZERO, |c| c.end),
        });
        current.clear();
    };
    for timing in &alignment.chars {
        if timing.text.chars().all(char::is_whitespace) {
            flush(&mut current);
        } else {
            current.push(timing);
        }
    }
    flush(&mut current);
    words.retain(|word| !word.key.is_empty());
    words
}

/// The nearest point after a mismatch where both sides have the same word
/// again, as words to skip on each side; skips that are not both zero, the
/// smallest total first.
fn resync(words: &[String], spoken: &[TimedWord]) -> Option<(usize, usize)> {
    (1..=2 * RESYNC_WINDOW).find_map(|total| {
        (0..=total.min(RESYNC_WINDOW))
            .map(|skip_words| (skip_words, total - skip_words))
            .filter(|&(_, skip_spoken)| skip_spoken <= RESYNC_WINDOW)
            .find(|&(skip_words, skip_spoken)| {
                matches!(
                    (words.get(skip_words), spoken.get(skip_spoken)),
                    (Some(word), Some(timed)) if !word.is_empty() && *word == timed.key
                )
            })
    })
}

/// Where a narration's audio came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NarrationSource {
    /// A persona's voice read the script.
    Generated {
        /// The voice that read it; the provider is the voice's.
        voice: VoiceRef,
        presets: GenerationPresets,
        /// The model that spoke, as the provider named it.
        model: String,
        /// What the provider billed, in its unit (ElevenLabs: characters).
        billed_characters: u64,
    },
    /// The user recorded the script and imported the file; a provider
    /// timed its words (PRD story 33).
    Imported {
        /// The file's name as the user had it.
        file_name: String,
        /// The provider that timed the words.
        aligner: Provider,
        /// The aligner, as the provider named it.
        model: String,
    },
}

impl NarrationSource {
    /// The provider that spoke or timed the narration.
    pub fn provider(&self) -> Provider {
        match self {
            NarrationSource::Generated { voice, .. } => voice.provider(),
            NarrationSource::Imported { aligner, .. } => *aligner,
        }
    }

    /// The model that spoke or timed the narration.
    pub fn model(&self) -> &str {
        match self {
            NarrationSource::Generated { model, .. } | NarrationSource::Imported { model, .. } => {
                model
            }
        }
    }
}

/// A video project's narration of its script: generated with a persona's
/// voice, or recorded by the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Narration {
    pub id: NarrationId,
    pub project: VideoProjectId,
    pub owner: ProfileId,
    /// The script text as it was read.
    pub text: ScriptText,
    pub source: NarrationSource,
    /// The audio file's name in the project folder: MP3, or the WAV the
    /// user imported.
    pub audio_file: String,
    pub duration: Duration,
    pub words: WordTimings,
    /// When it was generated or imported.
    pub generated_at: SystemTime,
    /// The job that generated or aligned it, so a resumed job does not pay
    /// twice.
    pub job: Option<JobId>,
}

impl Narration {
    /// Whether the script changed since this narration read it.
    pub fn is_stale(&self, script: &Script) -> bool {
        self.text != *script.text()
    }

    /// The word timings with the words' text.
    pub fn words(&self) -> impl Iterator<Item = (&str, &WordTiming)> {
        self.words
            .as_slice()
            .iter()
            .map(|timing| (&self.text.as_str()[timing.text.clone()], timing))
    }
}

/// Persistence port for narrations. Shared with job worker threads.
pub trait NarrationRepository: Send + Sync {
    /// The project's current narration.
    fn narration(&self, project: VideoProjectId) -> Result<Option<Narration>, RepositoryError>;

    /// Makes `narration` the project's current one, replacing any other.
    fn save_narration(&self, narration: &Narration) -> Result<(), RepositoryError>;
}

impl<T: NarrationRepository + ?Sized> NarrationRepository for Arc<T> {
    fn narration(&self, project: VideoProjectId) -> Result<Option<Narration>, RepositoryError> {
        (**self).narration(project)
    }

    fn save_narration(&self, narration: &Narration) -> Result<(), RepositoryError> {
        (**self).save_narration(narration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CharTiming;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Times every character of `text` 100 ms apart, as a provider would
    /// for the exact text it was sent.
    fn aligned(text: &str) -> Alignment {
        Alignment {
            chars: text
                .chars()
                .enumerate()
                .map(|(n, c)| CharTiming {
                    text: c.to_string(),
                    start: ms(n as u64 * 100),
                    end: ms(n as u64 * 100 + 100),
                })
                .collect(),
        }
    }

    fn words_of<'a>(text: &'a str, timings: &WordTimings) -> Vec<(&'a str, u64, u64)> {
        timings
            .as_slice()
            .iter()
            .map(|t| {
                (
                    &text[t.text.clone()],
                    t.start.as_millis() as u64,
                    t.end.as_millis() as u64,
                )
            })
            .collect()
    }

    #[test]
    fn words_are_split_on_whitespace_with_lone_marks_attached() {
        let text = "  — Wait... what?  It's 1969 — the end …\n";
        let words: Vec<_> = spoken_words(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(
            words,
            ["— Wait...", "what?", "It's", "1969 —", "the", "end …"]
        );
        assert!(spoken_words("  ").is_empty());
        assert_eq!(spoken_words("..."), vec![0..3]);
    }

    #[test]
    fn exact_text_times_each_word_by_its_letters() {
        let text = "Hi, you.";
        let timings = WordTimings::from_alignment(text, &aligned(text));
        // H=0 i=1 ,=2 space=3 y=4 o=5 u=6 .=7; punctuation is not timed.
        assert_eq!(
            words_of(text, &timings),
            [("Hi,", 0, 200), ("you.", 400, 700)]
        );
    }

    #[test]
    fn punctuation_and_quotes_the_provider_changed_still_match() {
        let text = "\"Stop,\" she said — loudly.";
        let provider = "“Stop” she said - loudly";
        let timings = WordTimings::from_alignment(text, &aligned(provider));
        let words = words_of(text, &timings);
        assert_eq!(words[0], ("\"Stop,\"", 100, 500));
        assert_eq!(words[1], ("she", 700, 1000));
        assert_eq!(words[2].0, "said —");
        assert_eq!(words[3], ("loudly.", 1800, 2400));
    }

    #[test]
    fn case_and_accents_are_ignored_composed_or_not() {
        let text = "Ação e emoção";
        // Upper case, and decomposed accents (base letter + combining mark).
        let provider = "AC\u{0327}A\u{0303}O E EMOC\u{0327}A\u{0303}O";
        let timings = WordTimings::from_alignment(text, &aligned(provider));
        let words = words_of(text, &timings);
        assert_eq!(words[0], ("Ação", 0, 600));
        assert_eq!(words[1], ("e", 700, 800));
        assert_eq!(words[2].1, 900);
    }

    #[test]
    fn numbers_read_as_written_keep_their_own_timing() {
        let text = "In 1969, 3.5 million watched.";
        let timings = WordTimings::from_alignment(text, &aligned(text));
        let words = words_of(text, &timings);
        assert_eq!(words[1], ("1969,", 300, 700));
        assert_eq!(words[2], ("3.5", 900, 1200));
    }

    #[test]
    fn numbers_in_normalized_text_take_the_time_between_their_neighbours() {
        let text = "In 1969 the probe flew.";
        let normalized = "In nineteen sixty-nine the probe flew.";
        let timings = WordTimings::from_alignment(text, &aligned(normalized));
        let words = words_of(text, &timings);
        assert_eq!(words[0], ("In", 0, 200));
        // "nineteen sixty-nine" runs from 300 to 2200; "the" starts at 2300.
        assert_eq!(words[1], ("1969", 200, 2300));
        assert_eq!(words[2], ("the", 2300, 2600));
        assert_eq!(words[4], ("flew.", 3300, 3700));
    }

    #[test]
    fn a_word_the_provider_skipped_is_squeezed_in_and_extra_words_are_ignored() {
        let text = "one two three four";
        let skipped = WordTimings::from_alignment(text, &aligned("one three four"));
        let words = words_of(text, &skipped);
        assert_eq!(words[1], ("two", 300, 400));
        assert_eq!(words[2], ("three", 400, 900));

        let extra = WordTimings::from_alignment(text, &aligned("one uh two three four"));
        assert_eq!(words_of(text, &extra)[1], ("two", 700, 1000));
    }

    #[test]
    fn without_any_match_words_share_the_whole_speech() {
        let text = "ab cd";
        let timings = WordTimings::from_alignment(text, &aligned("xyz qrst"));
        assert_eq!(words_of(text, &timings), [("ab", 0, 400), ("cd", 400, 800)]);
        let none = WordTimings::from_alignment(text, &Alignment::default());
        assert_eq!(words_of(text, &none), [("ab", 0, 0), ("cd", 0, 0)]);
    }

    #[test]
    fn timings_never_go_back_in_time() {
        let text = "a b c";
        let mut alignment = aligned(text);
        // A provider glitch: "c" timed before "b".
        alignment.chars[4].start = ms(50);
        alignment.chars[4].end = ms(60);
        let timings = WordTimings::from_alignment(text, &alignment);
        let starts: Vec<_> = timings.as_slice().iter().map(|t| t.start).collect();
        assert!(starts.windows(2).all(|p| p[0] <= p[1]), "{starts:?}");
        assert!(timings.as_slice().iter().all(|t| t.start <= t.end));
    }

    #[test]
    fn the_current_word_is_the_last_one_started() {
        let text = "Hi, you.";
        let timings = WordTimings::from_alignment(text, &aligned(text));
        assert_eq!(timings.word_at(ms(0)), Some(0));
        assert_eq!(timings.word_at(ms(250)), Some(0), "the pause after a word");
        assert_eq!(timings.word_at(ms(400)), Some(1));
        assert_eq!(timings.word_at(ms(9_000)), Some(1));
        let late = WordTimings::from_alignment(
            "x",
            &Alignment {
                chars: vec![CharTiming {
                    text: "x".into(),
                    start: ms(500),
                    end: ms(600),
                }],
            },
        );
        assert_eq!(late.word_at(ms(100)), None);
    }

    #[test]
    fn stored_timings_must_fit_their_text() {
        let text = "Hi, you.";
        let timings = WordTimings::from_alignment(text, &aligned(text));
        let stored = timings.as_slice().to_vec();
        assert_eq!(WordTimings::restore(text, stored.clone()), Ok(timings));
        assert_eq!(
            WordTimings::restore("Hi, you all.", stored.clone()),
            Err(InvalidWordTimings)
        );
        let mut backwards = stored;
        backwards[1].start = ms(0);
        backwards[0].start = ms(10);
        backwards[0].end = ms(10);
        assert_eq!(
            WordTimings::restore(text, backwards),
            Err(InvalidWordTimings)
        );
    }

    #[test]
    fn a_narration_goes_stale_when_the_script_changes() {
        use crate::{
            GeneratedScript, Generation, GenerationId, Provider, TemplateUsed, TemplateVersionId,
            TokenUsage,
        };
        let project = VideoProjectId::new();
        let owner = ProfileId::new();
        let generated = |output: &str| {
            GeneratedScript::new(Generation {
                id: GenerationId::new(),
                owner,
                project,
                provider: Provider::Claude,
                model: "claude-test".into(),
                template: TemplateUsed {
                    id: TemplateVersionId::new(),
                    number: 1,
                },
                instructions: String::new(),
                prompt: String::new(),
                output: output.into(),
                usage: TokenUsage::default(),
                generated_at: SystemTime::UNIX_EPOCH,
                job: None,
            })
            .unwrap()
        };
        let mut script = Script::first(generated("Hi, you."), SystemTime::UNIX_EPOCH);
        let narration = Narration {
            id: NarrationId::new(),
            project,
            owner,
            text: script.text().clone(),
            source: NarrationSource::Generated {
                voice: VoiceRef::elevenlabs("FrS6cKLB1wg4WYgPa9GW", "Wyatt").unwrap(),
                presets: GenerationPresets::default(),
                model: "eleven_multilingual_v2".into(),
                billed_characters: 8,
            },
            audio_file: "narration.mp3".into(),
            duration: ms(800),
            words: WordTimings::from_alignment("Hi, you.", &aligned("Hi, you.")),
            generated_at: SystemTime::UNIX_EPOCH,
            job: None,
        };
        assert!(!narration.is_stale(&script));
        assert_eq!(narration.source.provider(), Provider::ElevenLabs);
        assert_eq!(narration.source.model(), "eleven_multilingual_v2");
        let words: Vec<_> = narration.words().map(|(word, _)| word).collect();
        assert_eq!(words, ["Hi,", "you."]);

        script.edit(
            ScriptText::new("Hi, all of you.").unwrap(),
            SystemTime::UNIX_EPOCH,
        );
        assert!(narration.is_stale(&script));

        script.offer(generated("Hi, you."), SystemTime::UNIX_EPOCH);
        script.accept(SystemTime::UNIX_EPOCH).unwrap();
        assert!(
            !narration.is_stale(&script),
            "the same text again is what was read"
        );
    }
}
