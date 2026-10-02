//! Captions (PRD stories 64-66): the narration's words grouped into short
//! lines, each shown while its words are spoken, burned into the video in
//! one of a few styles.
//!
//! A caption belongs to the narration, not to the timeline: its times are
//! times in the narration file, so it shows wherever the cut plays those
//! words (`crate::Timeline::caption_spans`). A cut that removes narration
//! removes its captions with it, moving narration moves them, and undoing
//! the cut brings them back. The user edits a caption's text and its two
//! ends; every change is an edit that can be undone (`crate::Edit`).

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// How captions look when burned in. Each channel picks one for its new
/// projects; a project can change its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CaptionStyle {
    /// White text with a thin dark outline, low in the frame.
    #[default]
    Clean,
    /// White text on a dark band, low in the frame.
    Boxed,
    /// Big condensed capitals with a heavy outline, a third up the frame.
    Punch,
}

impl CaptionStyle {
    pub const ALL: [CaptionStyle; 3] = [
        CaptionStyle::Clean,
        CaptionStyle::Boxed,
        CaptionStyle::Punch,
    ];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            CaptionStyle::Clean => "clean",
            CaptionStyle::Boxed => "boxed",
            CaptionStyle::Punch => "punch",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown caption style: {0}")]
pub struct UnknownCaptionStyle(pub String);

impl FromStr for CaptionStyle {
    type Err = UnknownCaptionStyle;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        CaptionStyle::ALL
            .into_iter()
            .find(|style| style.code() == code)
            .ok_or_else(|| UnknownCaptionStyle(code.to_owned()))
    }
}

impl fmt::Display for CaptionStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// How the narration's words are grouped into lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRules {
    /// The longest a line gets, in characters (a single longer word still
    /// makes a line of its own).
    pub max_chars: usize,
    /// The longest a line stays up, from its first word to its last.
    pub max_duration: Duration,
}

/// The rules captions are made with: lines a viewer reads at a glance.
pub const LINE_RULES: LineRules = LineRules {
    max_chars: 32,
    max_duration: Duration::from_secs(3),
};

/// The longest a caption's text gets once the user edits it.
pub const MAX_CAPTION_CHARS: usize = 200;

/// One caption: its text and when it shows, in narration-file time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caption {
    pub text: String,
    pub start: Duration,
    pub end: Duration,
}

/// A caption's text as kept: trimmed, every run of whitespace (line
/// breaks too) one space. `None` when nothing is left or it is longer than
/// [`MAX_CAPTION_CHARS`].
pub fn caption_text(text: &str) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty() && text.chars().count() <= MAX_CAPTION_CHARS).then_some(text)
}

/// The mark a word ends on, past closing quotes and brackets.
fn last_mark(word: &str) -> Option<char> {
    word.chars()
        .rev()
        .find(|c| !matches!(c, '"' | '\'' | '”' | '’' | '»' | ')' | ']'))
}

fn ends_sentence(word: &str) -> bool {
    matches!(last_mark(word), Some('.' | '!' | '?' | '…'))
}

fn ends_clause(word: &str) -> bool {
    matches!(last_mark(word), Some(',' | ';' | ':' | '–' | '—'))
}

/// Groups timed words (text, start, end; in reading order) into caption
/// lines. A line ends after a word that ends a sentence, after one that
/// ends a clause once the line is half full, and before a word that would
/// take it past `rules`' length or duration. Each line shows from its first
/// word's start to its last word's end.
pub fn caption_lines<'a>(
    words: impl IntoIterator<Item = (&'a str, Duration, Duration)>,
    rules: LineRules,
) -> Vec<Caption> {
    let mut lines = Vec::new();
    let mut line: Option<(Caption, usize)> = None;
    for (word, start, end) in words {
        let word = word.trim();
        if word.is_empty() {
            continue;
        }
        let length = word.chars().count();
        if let Some((caption, chars)) = &line {
            let too_long = chars + 1 + length > rules.max_chars;
            let too_slow = end.saturating_sub(caption.start) > rules.max_duration;
            if too_long || too_slow {
                lines.extend(line.take().map(|(caption, _)| caption));
            }
        }
        let (caption, chars) = match line.take() {
            Some((mut caption, chars)) => {
                caption.text.push(' ');
                caption.text.push_str(word);
                caption.end = end.max(caption.end);
                (caption, chars + 1 + length)
            }
            None => (
                Caption {
                    text: word.to_owned(),
                    start,
                    end: end.max(start),
                },
                length,
            ),
        };
        if ends_sentence(word) || (ends_clause(word) && chars * 2 >= rules.max_chars) {
            lines.push(caption);
        } else {
            line = Some((caption, chars));
        }
    }
    lines.extend(line.map(|(caption, _)| caption));
    lines
}

/// The captions of a timeline: the lines, whether they are burned in, and
/// how they look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captions {
    pub(crate) lines: Vec<Caption>,
    pub(crate) shown: bool,
    pub(crate) style: CaptionStyle,
    /// How long the narration file is: no caption ends past it.
    pub(crate) length: Duration,
}

impl Captions {
    /// Captions for a narration of `length` that reads `words`, made with
    /// [`LINE_RULES`], shown, in the default style.
    pub(crate) fn from_words<'a>(
        words: impl IntoIterator<Item = (&'a str, Duration, Duration)>,
        length: Duration,
    ) -> Self {
        Captions {
            lines: caption_lines(words, LINE_RULES),
            shown: true,
            style: CaptionStyle::default(),
            length,
        }
    }

    /// In narration order.
    pub fn lines(&self) -> &[Caption] {
        &self.lines
    }

    /// Whether the captions are burned in (and shown in the preview).
    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn style(&self) -> CaptionStyle {
        self.style
    }

    /// Every line with text, at least `min_length` long, in order, apart and
    /// within the narration.
    pub(crate) fn holds_together(&self, min_length: Duration) -> bool {
        let lines = self.lines.iter().all(|caption| {
            caption_text(&caption.text).as_deref() == Some(caption.text.as_str())
                && caption.start + min_length <= caption.end
                && caption.end <= self.length
        });
        let apart = self
            .lines
            .windows(2)
            .all(|pair| pair[0].end <= pair[1].start);
        lines && apart
    }
}

/// The captions as a saved cut keeps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedCaptions {
    pub lines: Vec<Caption>,
    pub shown: bool,
    pub style: CaptionStyle,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Words 300 ms apart, each 250 ms long.
    fn timed(text: &str) -> Vec<(&str, Duration, Duration)> {
        text.split_whitespace()
            .enumerate()
            .map(|(index, word)| {
                let start = ms(300 * index as u64);
                (word, start, start + ms(250))
            })
            .collect()
    }

    fn texts(lines: &[Caption]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn a_line_shows_from_its_first_word_to_its_last() {
        let lines = caption_lines(timed("The keeper lit the lamp."), LINE_RULES);
        assert_eq!(
            lines,
            vec![Caption {
                text: "The keeper lit the lamp.".into(),
                start: ms(0),
                end: ms(1_450),
            }]
        );
    }

    #[test]
    fn a_line_breaks_before_it_runs_past_its_characters() {
        let rules = LineRules {
            max_chars: 16,
            max_duration: Duration::from_secs(60),
        };
        let lines = caption_lines(timed("one two three four five six seven"), rules);
        assert_eq!(texts(&lines), ["one two three", "four five six", "seven"]);
        assert!(lines.iter().all(|line| line.text.chars().count() <= 16));
        // Accents count once.
        let lines = caption_lines(timed("ação ação ação ação"), rules);
        assert_eq!(texts(&lines), ["ação ação ação", "ação"]);
    }

    #[test]
    fn a_word_longer_than_a_line_makes_a_line_of_its_own() {
        let rules = LineRules {
            max_chars: 8,
            max_duration: Duration::from_secs(60),
        };
        let lines = caption_lines(timed("an extraordinary tale"), rules);
        assert_eq!(texts(&lines), ["an", "extraordinary", "tale"]);
    }

    #[test]
    fn a_line_breaks_before_it_stays_up_too_long() {
        let rules = LineRules {
            max_chars: 100,
            max_duration: ms(1_000),
        };
        // Up to 1 s from the first word's start to the last word's end.
        let lines = caption_lines(timed("a b c d e f g"), rules);
        assert_eq!(texts(&lines), ["a b c", "d e f", "g"]);
        assert_eq!((lines[1].start, lines[1].end), (ms(900), ms(1_750)));
        // A slow word starts a line of its own.
        let words = [("slow", ms(0), ms(1_500)), ("then", ms(1_600), ms(1_800))];
        assert_eq!(texts(&caption_lines(words, rules)), ["slow", "then"]);
    }

    #[test]
    fn sentences_always_end_a_line() {
        let lines = caption_lines(
            timed("It was dark. Then light! Why? “Go.” Done…"),
            LINE_RULES,
        );
        assert_eq!(
            texts(&lines),
            ["It was dark.", "Then light!", "Why?", "“Go.”", "Done…"]
        );
    }

    #[test]
    fn a_clause_ends_a_line_once_it_is_half_full() {
        let rules = LineRules {
            max_chars: 20,
            max_duration: Duration::from_secs(60),
        };
        let lines = caption_lines(timed("Yes, the old keeper, alone at last; the sea"), rules);
        assert_eq!(
            texts(&lines),
            ["Yes, the old keeper,", "alone at last;", "the sea"]
        );
    }

    #[test]
    fn no_words_make_no_lines() {
        assert!(caption_lines(Vec::new(), LINE_RULES).is_empty());
        assert!(caption_lines(timed("   "), LINE_RULES).is_empty());
    }

    #[test]
    fn caption_text_is_trimmed_and_kept_on_one_line() {
        assert_eq!(
            caption_text("  The keeper\n lit  it "),
            Some("The keeper lit it".into())
        );
        assert_eq!(caption_text(" \n "), None);
        assert_eq!(
            caption_text(&"a".repeat(MAX_CAPTION_CHARS)).map(|t| t.len()),
            Some(200)
        );
        assert_eq!(caption_text(&"a".repeat(MAX_CAPTION_CHARS + 1)), None);
    }

    #[test]
    fn styles_round_trip_through_their_codes() {
        for style in CaptionStyle::ALL {
            assert_eq!(style.code().parse::<CaptionStyle>(), Ok(style));
        }
        assert!("comic".parse::<CaptionStyle>().is_err());
        assert_eq!(CaptionStyle::default(), CaptionStyle::Clean);
    }

    #[test]
    fn captions_hold_together_in_order_apart_and_within_the_narration() {
        let mut captions = Captions::from_words(timed("One. Two. Three."), ms(2_000));
        assert_eq!(captions.lines().len(), 3);
        assert!(captions.shown());
        assert!(captions.holds_together(ms(33)));
        captions.lines[1].start = ms(100);
        assert!(!captions.holds_together(ms(33)), "overlapping");
        captions.lines[1].start = ms(300);
        captions.lines[2].end = ms(2_500);
        assert!(!captions.holds_together(ms(33)), "past the narration");
        captions.lines[2].end = ms(850);
        captions.lines[2].text = " Three ".into();
        assert!(!captions.holds_together(ms(33)), "untrimmed text");
    }
}
