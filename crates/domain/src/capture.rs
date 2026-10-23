//! Captures (#189, CONTEXT.md): a recording of the user, alone in frame,
//! reading the project's script on camera. Its speech becomes the
//! narration, in the persona's voice or the user's own, and it is split
//! into takes that later become performed clips.
//!
//! Here: what a recording must be to be a capture, how closely its speech
//! must follow the script, where a long recording is cut for the voice
//! changer, and how the narration's words split into takes.

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::narration::key;
use crate::{
    GenerationPresets, JobId, NarrationId, ProfileId, RepositoryError, TakeAnalysis, TakeMode,
    VideoProjectId, VoiceRef, WordTiming, spoken_words,
};

uuid_id!(
    /// Identifies one capture.
    CaptureId
);

/// The shortest recording that can be a capture: one take.
pub const MIN_CAPTURE: Duration = Duration::from_secs(4);
/// The longest recording that can be a capture.
pub const MAX_CAPTURE: Duration = Duration::from_secs(15 * 60);
/// The lowest frame rate a capture may have, so 23.976 counts as 24.
pub const MIN_CAPTURE_FPS: f64 = 23.9;
/// The highest frame rate a capture may have, so 59.94 counts as 60.
pub const MAX_CAPTURE_FPS: f64 = 60.5;
/// The shortest side of a capture's picture, in pixels.
pub const MIN_CAPTURE_SIDE: u32 = 720;
/// The file extensions a capture may have (MP4 and MOV), lowercase.
pub const CAPTURE_EXTENSIONS: [&str; 2] = ["mp4", "mov"];

/// The shortest take a take model makes.
pub const MIN_TAKE: Duration = Duration::from_secs(4);
/// The longest take a take model makes.
pub const MAX_TAKE: Duration = Duration::from_secs(30);
/// The shortest silence inside a sentence that a take may be cut at.
pub const MIN_TAKE_PAUSE: Duration = Duration::from_millis(300);
/// How much of the recording a take keeps before its first word and after
/// its last, where it starts or ends the recording.
pub const TAKE_PADDING: Duration = Duration::from_millis(300);

/// The share, in thousandths, of the script's words that a capture's
/// speech must say in order, and of its words that must be the script's.
pub const MIN_SCRIPT_COVERAGE: u16 = 800;

/// Why a recording cannot be a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum CaptureProblem {
    #[error("the file is not MP4 or MOV")]
    NotMp4OrMov,
    #[error("the file has no picture")]
    NoVideo,
    #[error("the file has no sound")]
    NoAudio,
    #[error("the recording is under 4 seconds")]
    TooShort,
    #[error("the recording is over 15 minutes")]
    TooLong,
    #[error("the frame rate is not between 24 and 60")]
    FrameRate,
    #[error("the picture's short side is under 720 pixels")]
    TooSmall,
}

impl CaptureProblem {
    pub const ALL: [CaptureProblem; 7] = [
        CaptureProblem::NotMp4OrMov,
        CaptureProblem::NoVideo,
        CaptureProblem::NoAudio,
        CaptureProblem::TooShort,
        CaptureProblem::TooLong,
        CaptureProblem::FrameRate,
        CaptureProblem::TooSmall,
    ];
}

/// What a recording's file holds, as ffmpeg reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordingFacts {
    pub duration: Duration,
    /// Width, height and frames per second of its picture, if it has one.
    pub video: Option<(u32, u32, f64)>,
    pub has_audio: bool,
}

/// A capture's picture, as checked when it was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureVideo {
    pub width: u32,
    pub height: u32,
    /// Frames per second in thousandths (29,970 for 29.97), so it stores
    /// as an integer.
    pub fps_milli: u32,
    pub duration: Duration,
}

impl CaptureVideo {
    /// The capture `file_name` with `facts` makes, or why it cannot be one.
    /// Checked in the order the user can fix things: the file, its streams,
    /// then its length and picture.
    pub fn check(file_name: &str, facts: &RecordingFacts) -> Result<Self, CaptureProblem> {
        let extension = file_name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase());
        if !extension.is_some_and(|extension| CAPTURE_EXTENSIONS.contains(&extension.as_str())) {
            return Err(CaptureProblem::NotMp4OrMov);
        }
        let (width, height, fps) = facts.video.ok_or(CaptureProblem::NoVideo)?;
        if !facts.has_audio {
            return Err(CaptureProblem::NoAudio);
        }
        if facts.duration < MIN_CAPTURE {
            return Err(CaptureProblem::TooShort);
        }
        if facts.duration > MAX_CAPTURE {
            return Err(CaptureProblem::TooLong);
        }
        if !(MIN_CAPTURE_FPS..=MAX_CAPTURE_FPS).contains(&fps) {
            return Err(CaptureProblem::FrameRate);
        }
        if width.min(height) < MIN_CAPTURE_SIDE {
            return Err(CaptureProblem::TooSmall);
        }
        Ok(Self {
            width,
            height,
            fps_milli: (fps * 1000.0).round() as u32,
            // Whole milliseconds, as the capture is kept, so a recording
            // read again compares equal to the one read before.
            duration: Duration::from_millis(facts.duration.as_millis() as u64),
        })
    }
}

/// Whose voice a capture's narration speaks with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureVoice {
    /// The speech converted into the persona's voice, keeping the
    /// performer's timing.
    Persona {
        voice: VoiceRef,
        presets: GenerationPresets,
    },
    /// The user's own voice, cleaned of music and room sound.
    Own,
}

impl CaptureVoice {
    /// Stable name stored in the database.
    pub fn code(&self) -> &'static str {
        match self {
            CaptureVoice::Persona { .. } => "persona",
            CaptureVoice::Own => "own",
        }
    }
}

/// The user's word that the recording shows only them: the sentence they
/// were shown, as shown, and when they ticked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfirmation {
    pub text: String,
    pub at: SystemTime,
}

/// A word the speech-to-text heard, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeardWord {
    pub text: String,
    pub start: Duration,
    pub end: Duration,
}

/// How closely the `heard` words follow `script`, in thousandths: the
/// longest run of script words found in the same order (not necessarily
/// next to each other) in what was heard, as a share of the script's words
/// or of the heard words, whichever is less, so a recording that says the
/// script and then much else does not pass. Words match as narration
/// timings do: letters and digits, case and accents ignored. A script
/// without words is followed by anything.
pub fn script_coverage(script: &str, heard: &[HeardWord]) -> u16 {
    let script: Vec<String> = spoken_words(script)
        .into_iter()
        .map(|range| key(&script[range]))
        .filter(|word| !word.is_empty())
        .collect();
    if script.is_empty() {
        return 1000;
    }
    let heard: Vec<String> = heard
        .iter()
        .flat_map(|word| word.text.split_whitespace())
        .map(key)
        .filter(|word| !word.is_empty())
        .collect();
    // The longest common subsequence, one row at a time.
    let mut previous = vec![0u32; heard.len() + 1];
    let mut row = vec![0u32; heard.len() + 1];
    for word in &script {
        for (j, said) in heard.iter().enumerate() {
            row[j + 1] = if word == said {
                previous[j] + 1
            } else {
                row[j].max(previous[j + 1])
            };
        }
        std::mem::swap(&mut previous, &mut row);
    }
    let found = u64::from(previous[heard.len()]);
    let said = script.len().max(heard.len()) as u64;
    (found * 1000 / said) as u16
}

/// Where to cut a recording of `duration` into parts of at most `max` for
/// a provider that takes no more at once: at the longest silence between
/// `heard` words in the second half of each part's window, else at the
/// window's end. Parts are contiguous and cover the whole recording.
pub fn audio_parts(duration: Duration, heard: &[HeardWord], max: Duration) -> Vec<Range<Duration>> {
    let mut parts = Vec::new();
    let mut start = Duration::ZERO;
    while duration - start > max {
        let limit = start + max;
        let earliest = start + max / 2;
        let cut = heard
            .windows(2)
            .filter_map(|pair| {
                let (before, after) = (&pair[0], &pair[1]);
                let middle = before.end + after.start.saturating_sub(before.end) / 2;
                (middle > earliest && middle <= limit)
                    .then(|| (after.start.saturating_sub(before.end), middle))
            })
            .max_by_key(|(silence, _)| *silence)
            .map_or(limit, |(_, middle)| middle);
        parts.push(start..cut);
        start = cut;
    }
    if duration > start || parts.is_empty() {
        parts.push(start..duration);
    }
    parts
}

/// Where a take may start or end: after a sentence, or at a pause inside
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutKind {
    Sentence,
    Pause,
}

/// The narration a capture's takes are cut from: its text, its word
/// timings and how long it plays.
#[derive(Debug, Clone, Copy)]
pub struct TakeSource<'a> {
    pub text: &'a str,
    pub words: &'a [WordTiming],
    pub duration: Duration,
}

impl TakeSource<'_> {
    /// Whether a take may end after word `index` (not the last), and why.
    pub fn cut_after(&self, index: usize) -> Option<CutKind> {
        let (word, next) = (self.words.get(index)?, self.words.get(index + 1)?);
        if ends_sentence(&self.text[word.text.clone()]) {
            Some(CutKind::Sentence)
        } else if next.start.saturating_sub(word.end) >= MIN_TAKE_PAUSE {
            Some(CutKind::Pause)
        } else {
            None
        }
    }

    /// Where the boundary after word `index` sits: in the middle of the
    /// silence before the next word.
    fn boundary_after(&self, index: usize) -> Duration {
        let (word, next) = (&self.words[index], &self.words[index + 1]);
        word.end + next.start.saturating_sub(word.end) / 2
    }

    /// The stretch of the recording the take of `words` covers.
    pub fn span(&self, words: &Range<usize>) -> Range<Duration> {
        let start = if words.start == 0 {
            self.words[0].start.saturating_sub(TAKE_PADDING)
        } else {
            self.boundary_after(words.start - 1)
        };
        let end = if words.end == self.words.len() {
            let last = self.words[words.end - 1].end;
            (last + TAKE_PADDING).min(self.duration.max(last))
        } else {
            self.boundary_after(words.end - 1)
        };
        start..end.max(start)
    }

    fn length(&self, words: &Range<usize>) -> Duration {
        let span = self.span(words);
        span.end - span.start
    }

    fn take(&self, words: Range<usize>, revision: u32) -> Take {
        let span = self.span(&words);
        Take {
            words,
            start: span.start,
            end: span.end,
            revision,
            mode: TakeMode::default(),
        }
    }
}

/// Whether a word closes a sentence: it ends in a full stop, question or
/// exclamation mark or ellipsis, perhaps inside closing quotes or brackets.
fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '”', '’', '»', ')', ']'])
        .ends_with(['.', '!', '?', '…'])
}

/// Why a take cannot go to a take model as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TakeFlag {
    /// Over [`MAX_TAKE`], with no pause to cut it at: the user records that
    /// line again.
    TooLong,
    /// Under [`MIN_TAKE`], with no neighbour it fits with.
    TooShort,
}

/// A stretch of a capture, from sentence end to sentence end, that becomes
/// one performed clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Take {
    /// The narration's words it holds, by index.
    pub words: Range<usize>,
    pub start: Duration,
    pub end: Duration,
    /// Goes up each time the take's stretch changes, so work made from an
    /// earlier stretch can tell it is stale.
    pub revision: u32,
    /// What the take model receives for it (#190).
    pub mode: TakeMode,
}

impl Take {
    pub fn length(&self) -> Duration {
        self.end - self.start
    }

    pub fn flag(&self) -> Option<TakeFlag> {
        if self.length() > MAX_TAKE {
            Some(TakeFlag::TooLong)
        } else if self.length() < MIN_TAKE {
            Some(TakeFlag::TooShort)
        } else {
            None
        }
    }
}

/// Which way a boundary between two takes moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryMove {
    /// To the previous cut point: the earlier take gets shorter.
    Earlier,
    /// To the next cut point: the earlier take gets longer.
    Later,
}

/// Why stored takes do not fit their narration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("takes do not cover the narration's words in order")]
pub struct InvalidTakes;

/// Why a boundary cannot move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BoundaryError {
    #[error("no such boundary")]
    NoSuchBoundary,
    #[error("no such take")]
    NoSuchTake,
    /// No cut point that leaves both takes a word.
    #[error("no cut point that way")]
    NoCutPoint,
}

/// A capture's takes, in order, covering every word of its narration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Takes(Vec<Take>);

impl Takes {
    /// Splits the narration into takes: sentences packed into takes of up
    /// to [`MAX_TAKE`]; a sentence over it cut at its longest pause of
    /// [`MIN_TAKE_PAUSE`] or more, again while a piece is too long; a take
    /// under [`MIN_TAKE`] lengthened from a neighbour where both still fit.
    pub fn split(source: &TakeSource<'_>) -> Self {
        let n = source.words.len();
        if n == 0 {
            return Self::default();
        }
        let mut sentences = Vec::new();
        let mut from = 0;
        for index in 0..n {
            if index + 1 == n || source.cut_after(index) == Some(CutKind::Sentence) {
                sentences.push(from..index + 1);
                from = index + 1;
            }
        }
        let mut pieces = Vec::new();
        for sentence in sentences {
            cut_long(source, sentence, &mut pieces);
        }
        let mut takes: Vec<Range<usize>> = Vec::new();
        for piece in pieces {
            match takes.last_mut() {
                Some(last)
                    if source.length(last) <= MAX_TAKE
                        && source.length(&piece) <= MAX_TAKE
                        && source.length(&(last.start..piece.end)) <= MAX_TAKE =>
                {
                    last.end = piece.end;
                }
                _ => takes.push(piece),
            }
        }
        lengthen_short(source, &mut takes);
        Self(
            takes
                .into_iter()
                .map(|words| source.take(words, 1))
                .collect(),
        )
    }

    /// Rebuilds stored takes, checking they cover `word_count` words in
    /// order, each with at least one.
    pub fn restore(word_count: usize, takes: Vec<Take>) -> Result<Self, InvalidTakes> {
        let mut next = 0;
        for take in &takes {
            if take.words.start != next || take.words.end <= take.words.start {
                return Err(InvalidTakes);
            }
            next = take.words.end;
        }
        if next != word_count || takes.iter().any(|take| take.start > take.end) {
            return Err(InvalidTakes);
        }
        Ok(Self(takes))
    }

    pub fn as_slice(&self) -> &[Take] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many takes are flagged.
    pub fn flagged(&self) -> usize {
        self.0.iter().filter(|take| take.flag().is_some()).count()
    }

    /// Where the boundary after take `index` (between it and the next)
    /// would go moving `to`: the word the later take would start at, at the
    /// nearest cut point that does not leave either take newly over
    /// [`MAX_TAKE`] or under [`MIN_TAKE`].
    pub fn boundary_target(
        &self,
        source: &TakeSource<'_>,
        index: usize,
        to: BoundaryMove,
    ) -> Result<usize, BoundaryError> {
        let (before, after) = match (self.0.get(index), self.0.get(index + 1)) {
            (Some(before), Some(after)) => (before, after),
            _ => return Err(BoundaryError::NoSuchBoundary),
        };
        let current = before.words.end;
        let flags = |earlier: &Take, later: &Take| {
            usize::from(earlier.flag().is_some()) + usize::from(later.flag().is_some())
        };
        let now = flags(before, after);
        // A cut after `word`, keeping both takes as fit as they are.
        let fits = |word: &usize| {
            let at = *word + 1;
            source.cut_after(*word).is_some()
                && flags(
                    &source.take(before.words.start..at, 0),
                    &source.take(at..after.words.end, 0),
                ) <= now
        };
        let target = match to {
            // The earlier take keeps at least its first word.
            BoundaryMove::Earlier => (before.words.start..current - 1).rev().find(fits),
            // The later take keeps at least its last word.
            BoundaryMove::Later => (current..after.words.end - 1).find(fits),
        };
        target.map(|word| word + 1).ok_or(BoundaryError::NoCutPoint)
    }

    /// Gives take `index` `mode`. Its stretch, and so its revision, stay.
    pub fn set_mode(&mut self, index: usize, mode: TakeMode) -> Result<(), BoundaryError> {
        let take = self.0.get_mut(index).ok_or(BoundaryError::NoSuchTake)?;
        take.mode = mode;
        Ok(())
    }

    /// Moves the boundary after take `index` to the previous or next cut
    /// point. Both takes get a new revision and keep their modes.
    pub fn move_boundary(
        &mut self,
        source: &TakeSource<'_>,
        index: usize,
        to: BoundaryMove,
    ) -> Result<(), BoundaryError> {
        let at = self.boundary_target(source, index, to)?;
        let (before, after) = (&self.0[index], &self.0[index + 1]);
        let earlier = Take {
            mode: before.mode,
            ..source.take(before.words.start..at, before.revision + 1)
        };
        let later = Take {
            mode: after.mode,
            ..source.take(at..after.words.end, after.revision + 1)
        };
        self.0[index] = earlier;
        self.0[index + 1] = later;
        Ok(())
    }
}

/// Cuts `piece` at its longest pause while it is over [`MAX_TAKE`],
/// pushing the pieces in order. A piece with no pause stays whole.
fn cut_long(source: &TakeSource<'_>, piece: Range<usize>, pieces: &mut Vec<Range<usize>>) {
    if source.length(&piece) <= MAX_TAKE {
        pieces.push(piece);
        return;
    }
    let silence = |index: usize| {
        let (word, next) = (&source.words[index], &source.words[index + 1]);
        next.start.saturating_sub(word.end)
    };
    let longest = (piece.start..piece.end - 1)
        .filter(|&index| source.cut_after(index).is_some())
        // The earliest of equal pauses, so the result does not depend on
        // how ties fall.
        .max_by_key(|&index| (silence(index), std::cmp::Reverse(index)));
    match longest {
        Some(index) => {
            cut_long(source, piece.start..index + 1, pieces);
            cut_long(source, index + 1..piece.end, pieces);
        }
        None => pieces.push(piece),
    }
}

/// Lengthens each take under [`MIN_TAKE`] by moving the boundary before
/// it back to an earlier cut point of the previous take, else the one after
/// it forward, as long as both takes stay within [`MIN_TAKE`] and
/// [`MAX_TAKE`]. Packing already joined every pair that fits whole.
fn lengthen_short(source: &TakeSource<'_>, takes: &mut [Range<usize>]) {
    let fits = |words: &Range<usize>| (MIN_TAKE..=MAX_TAKE).contains(&source.length(words));
    for index in 0..takes.len() {
        if source.length(&takes[index]) >= MIN_TAKE {
            continue;
        }
        let short = takes[index].clone();
        if index > 0 {
            let before = takes[index - 1].clone();
            let at = (before.start..before.end - 1)
                .rev()
                .filter(|&word| source.cut_after(word).is_some())
                .map(|word| word + 1)
                .find(|&at| fits(&(before.start..at)) && fits(&(at..short.end)));
            if let Some(at) = at {
                takes[index - 1].end = at;
                takes[index].start = at;
                continue;
            }
        }
        if let Some(after) = takes.get(index + 1).cloned() {
            let at = (after.start..after.end - 1)
                .filter(|&word| source.cut_after(word).is_some())
                .map(|word| word + 1)
                .find(|&at| fits(&(short.start..at)) && fits(&(at..after.end)));
            if let Some(at) = at {
                takes[index].end = at;
                takes[index + 1].start = at;
            }
        }
    }
}

/// A recording of the user reading the project's script, imported with
/// their confirmation; its speech is the project's narration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub id: CaptureId,
    pub project: VideoProjectId,
    pub owner: ProfileId,
    /// The file's name as the user had it.
    pub file_name: String,
    /// Its copy in the project folder.
    pub video_file: String,
    pub video: CaptureVideo,
    pub confirmation: CaptureConfirmation,
    pub voice: CaptureVoice,
    /// The narration its speech became.
    pub narration: NarrationId,
    /// How much of the script its speech says in order, in thousandths.
    pub coverage: u16,
    pub takes: Takes,
    pub imported_at: SystemTime,
    /// The job that imported it.
    pub job: Option<JobId>,
}

/// Persistence port for captures. Shared with job worker threads.
pub trait CaptureRepository: Send + Sync {
    /// The project's capture.
    fn capture(&self, project: VideoProjectId) -> Result<Option<Capture>, RepositoryError>;

    /// Makes `capture` the project's one, replacing any other.
    fn save_capture(&self, capture: &Capture) -> Result<(), RepositoryError>;

    /// Replaces the takes of the project's capture, when it is still
    /// `capture`: takes cut from another capture's narration do not fit.
    fn save_takes(
        &self,
        project: VideoProjectId,
        capture: CaptureId,
        takes: &Takes,
    ) -> Result<(), RepositoryError>;

    /// Removes the project's capture, if any, with its analyses.
    fn delete_capture(&self, project: VideoProjectId) -> Result<(), RepositoryError>;

    /// The finished analyses of the project's capture's takes, current or
    /// stale ([`TakeAnalysis::fits`]).
    fn analyses(&self, project: VideoProjectId) -> Result<Vec<TakeAnalysis>, RepositoryError>;

    /// Keeps `analysis` as its take's analysis of its part, replacing the
    /// one before, while the project's capture is still its capture and
    /// the take is still at its revision. Returns whether it was kept.
    fn save_analysis(&self, analysis: &TakeAnalysis) -> Result<bool, RepositoryError>;
}

impl<T: CaptureRepository + ?Sized> CaptureRepository for Arc<T> {
    fn capture(&self, project: VideoProjectId) -> Result<Option<Capture>, RepositoryError> {
        (**self).capture(project)
    }

    fn save_capture(&self, capture: &Capture) -> Result<(), RepositoryError> {
        (**self).save_capture(capture)
    }

    fn save_takes(
        &self,
        project: VideoProjectId,
        capture: CaptureId,
        takes: &Takes,
    ) -> Result<(), RepositoryError> {
        (**self).save_takes(project, capture, takes)
    }

    fn delete_capture(&self, project: VideoProjectId) -> Result<(), RepositoryError> {
        (**self).delete_capture(project)
    }

    fn analyses(&self, project: VideoProjectId) -> Result<Vec<TakeAnalysis>, RepositoryError> {
        (**self).analyses(project)
    }

    fn save_analysis(&self, analysis: &TakeAnalysis) -> Result<bool, RepositoryError> {
        (**self).save_analysis(analysis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(seconds: f64) -> Duration {
        Duration::from_secs_f64(seconds)
    }

    fn facts(duration: f64, video: Option<(u32, u32, f64)>, has_audio: bool) -> RecordingFacts {
        RecordingFacts {
            duration: s(duration),
            video,
            has_audio,
        }
    }

    #[test]
    fn a_capture_is_an_mp4_or_mov_with_picture_and_sound_in_bounds() {
        let good = facts(60.0, Some((1920, 1080, 29.97)), true);
        let video = CaptureVideo::check("Take 1.MOV", &good).unwrap();
        assert_eq!((video.width, video.height), (1920, 1080));
        assert_eq!(video.fps_milli, 29_970);
        assert_eq!(video.duration, s(60.0));
        // Kept in whole milliseconds, as stored, so reading the same file
        // again gives the same capture.
        let odd = facts(13.033_333, Some((1920, 1080, 29.97)), true);
        let video = CaptureVideo::check("take.mp4", &odd).unwrap();
        assert_eq!(video.duration, Duration::from_millis(13_033));
        assert_eq!(
            CaptureVideo::check("take.mp4", &odd),
            CaptureVideo::check("take.mp4", &odd)
        );
        assert!(CaptureVideo::check("take.mp4", &good).is_ok());
        // A portrait phone recording is fine: its short side counts.
        assert!(
            CaptureVideo::check("take.mp4", &facts(60.0, Some((720, 1280, 30.0)), true)).is_ok()
        );
        for (fps, ok) in [
            (23.976, true),
            (24.0, true),
            (59.94, true),
            (60.0, true),
            (15.0, false),
            (120.0, false),
        ] {
            let result = CaptureVideo::check("t.mp4", &facts(10.0, Some((1280, 720, fps)), true));
            assert_eq!(result.is_ok(), ok, "{fps} fps");
        }

        let refused =
            |name: &str, facts: RecordingFacts| CaptureVideo::check(name, &facts).unwrap_err();
        assert_eq!(refused("take.mkv", good), CaptureProblem::NotMp4OrMov);
        assert_eq!(refused("take", good), CaptureProblem::NotMp4OrMov);
        assert_eq!(
            refused("take.mp4", facts(60.0, None, true)),
            CaptureProblem::NoVideo
        );
        assert_eq!(
            refused("take.mp4", facts(60.0, Some((1920, 1080, 30.0)), false)),
            CaptureProblem::NoAudio
        );
        assert_eq!(
            refused("take.mp4", facts(3.9, Some((1920, 1080, 30.0)), true)),
            CaptureProblem::TooShort
        );
        assert_eq!(
            refused(
                "take.mp4",
                facts(15.0 * 60.0 + 0.5, Some((1920, 1080, 30.0)), true)
            ),
            CaptureProblem::TooLong
        );
        assert_eq!(
            refused("take.mp4", facts(60.0, Some((1280, 719, 30.0)), true)),
            CaptureProblem::TooSmall
        );
    }

    fn heard(text: &str) -> Vec<HeardWord> {
        text.split_whitespace()
            .enumerate()
            .map(|(n, word)| HeardWord {
                text: word.into(),
                start: s(n as f64 * 0.4),
                end: s(n as f64 * 0.4 + 0.3),
            })
            .collect()
    }

    #[test]
    fn coverage_counts_script_words_said_in_order() {
        let script = "Era uma vez, em 1969, uma sonda. Ela partiu — e nunca voltou.";
        // 12 words; the dash goes with "partiu".
        assert_eq!(script_coverage(script, &heard(script)), 1000);
        // Case, accents and punctuation do not matter; a filler word costs
        // a little: 12 of 13 heard.
        assert_eq!(
            script_coverage(
                script,
                &heard("ERA uma, hum, vez em 1969 uma SONDA ela partiu e nunca voltou")
            ),
            923
        );
        // The whole script and then as much again of something else: half
        // of what was heard.
        assert_eq!(
            script_coverage(
                script,
                &heard(
                    "Era uma vez em 1969 uma sonda ela partiu e nunca voltou \
                     a b c d e f g h i j k l"
                )
            ),
            500
        );
        // Two words left out: 10 of 12.
        assert_eq!(
            script_coverage(
                script,
                &heard("Era uma vez em 1969 uma sonda partiu e voltou")
            ),
            833
        );
        // Words said in another order count only once in order.
        assert!(
            script_coverage(
                script,
                &heard("voltou nunca e partiu ela sonda uma 1969 em vez uma era")
            ) < 300
        );
        assert_eq!(
            script_coverage(script, &heard("something else entirely")),
            0
        );
        assert_eq!(script_coverage(script, &[]), 0);
        assert_eq!(script_coverage("…", &[]), 1000);
        // A provider that hears several words as one entry still counts.
        let joined = vec![HeardWord {
            text: script.into(),
            start: s(0.0),
            end: s(5.0),
        }];
        assert_eq!(script_coverage(script, &joined), 1000);
    }

    #[test]
    fn long_recordings_are_cut_at_the_longest_silence_near_each_limit() {
        let word = |start: f64, end: f64| HeardWord {
            text: "w".into(),
            start: s(start),
            end: s(end),
        };
        let max = s(290.0);
        assert_eq!(audio_parts(s(120.0), &[], max), [s(0.0)..s(120.0)]);
        assert_eq!(audio_parts(s(290.0), &[], max), [s(0.0)..s(290.0)]);
        // No words: cut at the limit.
        assert_eq!(
            audio_parts(s(600.0), &[], max),
            [s(0.0)..s(290.0), s(290.0)..s(580.0), s(580.0)..s(600.0)]
        );
        // A word a second, with longer silences where words are missing:
        // the longest in the second half of the window wins; one in the
        // first half (around 101 s) or past the limit (around 302 s) does
        // not.
        let skipped = [100, 101, 102, 251, 252, 280, 300, 301, 302, 303, 304];
        let words: Vec<_> = (0..400u32)
            .filter(|n| !skipped.contains(n))
            .map(|n| word(f64::from(n), f64::from(n) + 0.9))
            .collect();
        let parts = audio_parts(s(400.0), &words, max);
        assert_eq!(parts, [s(0.0)..s(251.95), s(251.95)..s(400.0)]);
    }

    /// Word timings for `text`: each word `word` seconds long, a `gap`
    /// after it, and `pause` seconds after the words given.
    fn timed(text: &str, word: f64, gap: f64, pauses: &[(usize, f64)]) -> Vec<WordTiming> {
        let mut at = 1.0;
        spoken_words(text)
            .into_iter()
            .enumerate()
            .map(|(index, range)| {
                let timing = WordTiming {
                    text: range,
                    start: s(at),
                    end: s(at + word),
                };
                at += word + gap;
                if let Some((_, pause)) = pauses.iter().find(|(after, _)| *after == index) {
                    at += pause;
                }
                timing
            })
            .collect()
    }

    fn lengths(takes: &Takes) -> Vec<f64> {
        takes
            .as_slice()
            .iter()
            .map(|take| (take.length().as_secs_f64() * 10.0).round() / 10.0)
            .collect()
    }

    /// `count` sentences of `words` words each.
    fn sentences(count: usize, words: usize) -> String {
        (0..count)
            .map(|n| {
                let mut sentence = vec![format!("s{n}")];
                sentence.extend((1..words).map(|w| format!("w{w}")));
                format!("{}.", sentence.join(" "))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_minute_of_sentences_makes_takes_cut_at_sentence_ends() {
        // Six sentences of 20 words at 0.5 s a word: 10 s each, 60 s.
        let text = sentences(6, 20);
        let words = timed(&text, 0.4, 0.1, &[]);
        let duration = words.last().unwrap().end + s(1.0);
        let source = TakeSource {
            text: &text,
            words: &words,
            duration,
        };
        let takes = Takes::split(&source);
        assert_eq!(takes.len(), 3);
        for take in takes.as_slice() {
            assert!(take.flag().is_none(), "{take:?}");
            assert_eq!(take.revision, 1);
            let last = &text[words[take.words.end - 1].text.clone()];
            assert!(last.ends_with('.'), "ends at a sentence: {last}");
        }
        // Contiguous, from just before the first word to just after the last.
        let all = takes.as_slice();
        assert_eq!(all[0].start, s(0.7));
        assert!(all.windows(2).all(|pair| pair[0].end == pair[1].start));
        assert_eq!(all[2].end, words.last().unwrap().end + TAKE_PADDING);
        // The boundary sits in the middle of the silence between sentences.
        assert_eq!(all[0].end, words[39].end + s(0.05));
    }

    #[test]
    fn a_long_sentence_is_cut_at_its_longest_pause_or_flagged() {
        // One sentence of 80 words at 0.5 s: 40 s, a 0.6 s pause after
        // word 30 and a 0.35 s one after word 60.
        let text = sentences(1, 80);
        let words = timed(&text, 0.4, 0.1, &[(30, 0.6), (60, 0.25)]);
        let source = TakeSource {
            text: &text,
            words: &words,
            duration: words.last().unwrap().end,
        };
        let takes = Takes::split(&source);
        assert_eq!(takes.len(), 2);
        assert_eq!(takes.as_slice()[0].words, 0..31);
        assert!(takes.as_slice().iter().all(|take| take.flag().is_none()));

        // No pause of 300 ms anywhere: one take, flagged.
        let words = timed(&text, 0.4, 0.1, &[]);
        let source = TakeSource {
            text: &text,
            words: &words,
            duration: words.last().unwrap().end,
        };
        let takes = Takes::split(&source);
        assert_eq!(takes.len(), 1);
        assert_eq!(takes.as_slice()[0].flag(), Some(TakeFlag::TooLong));
        assert_eq!(takes.flagged(), 1);
    }

    fn split(text: &str, pauses: &[(usize, f64)]) -> Takes {
        let words = timed(text, 0.4, 0.1, pauses);
        Takes::split(&TakeSource {
            text,
            words: &words,
            duration: words.last().unwrap().end,
        })
    }

    #[test]
    fn a_short_take_joins_or_borrows_from_a_neighbour_or_is_flagged() {
        // A 20 s sentence then a 2 s one fit together.
        let text = format!("{} {}", sentences(1, 40), sentences(1, 4));
        assert_eq!(lengths(&split(&text, &[])), [22.2]);

        // 15 s and 14 s, then 2 s: the short one takes the sentence
        // before it.
        let text = format!("{} {}", sentences(2, 29), sentences(1, 4));
        let takes = split(&text, &[]);
        assert_eq!(takes.len(), 2);
        assert_eq!(takes.as_slice()[1].words.start, 29);
        assert_eq!(takes.flagged(), 0, "{:?}", lengths(&takes));

        // 29 s with no cut point in it, then 2 s: they do not fit together
        // and nothing can move, so the short one is flagged.
        let takes = split(&format!("{} {}", sentences(1, 58), sentences(1, 4)), &[]);
        assert_eq!(takes.len(), 2);
        assert_eq!(takes.as_slice()[1].flag(), Some(TakeFlag::TooShort));

        // A short opening sentence goes with the next.
        let takes = split(&format!("Hi. {}", sentences(2, 30)), &[]);
        assert_eq!(takes.as_slice()[0].words.start, 0);
        assert!(takes.as_slice()[0].words.end > 1);
        assert_eq!(takes.flagged(), 0);
    }

    #[test]
    fn sentence_ends_inside_quotes_and_ellipses_count() {
        for word in ["end.", "end?\"", "end!”", "end…", "(end.)"] {
            assert!(ends_sentence(word), "{word}");
        }
        for word in ["end,", "end;", "1.5", "end"] {
            assert!(!ends_sentence(word), "{word}");
        }
    }

    #[test]
    fn a_boundary_moves_to_the_previous_or_next_cut_point() {
        // Six sentences of 12 words at 0.5 s: 6 s each, a 0.3 s pause
        // after word 5. Four sentences fit in one take, two in the next.
        let text = sentences(6, 12);
        let words = timed(&text, 0.4, 0.1, &[(5, 0.3)]);
        let source = TakeSource {
            text: &text,
            words: &words,
            duration: words.last().unwrap().end,
        };
        let mut takes = Takes::split(&source);
        let ranges = |takes: &Takes| {
            takes
                .as_slice()
                .iter()
                .map(|take| take.words.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ranges(&takes), [0..48, 48..72]);

        takes
            .move_boundary(&source, 0, BoundaryMove::Earlier)
            .unwrap();
        // Back one sentence, to the end of word 35.
        assert_eq!(ranges(&takes), [0..36, 36..72]);
        assert_eq!(takes.as_slice()[0].revision, 2);
        assert_eq!(takes.as_slice()[1].revision, 2);
        assert_eq!(takes.as_slice()[0].end, takes.as_slice()[1].start);
        takes
            .move_boundary(&source, 0, BoundaryMove::Earlier)
            .unwrap();
        assert_eq!(ranges(&takes), [0..24, 24..72]);

        takes
            .move_boundary(&source, 0, BoundaryMove::Earlier)
            .unwrap();
        assert_eq!(ranges(&takes), [0..12, 12..72]);

        // Not to the pause after word 5: the first take would be under 4
        // seconds and the second over 30.
        assert_eq!(
            takes.move_boundary(&source, 0, BoundaryMove::Earlier),
            Err(BoundaryError::NoCutPoint)
        );
        assert_eq!(ranges(&takes), [0..12, 12..72], "unchanged");

        for _ in 0..3 {
            takes
                .move_boundary(&source, 0, BoundaryMove::Later)
                .unwrap();
        }
        assert_eq!(ranges(&takes), [0..48, 48..72]);
        // Five sentences are over 30 seconds.
        assert_eq!(
            takes.move_boundary(&source, 0, BoundaryMove::Later),
            Err(BoundaryError::NoCutPoint)
        );
        assert_eq!(
            takes.move_boundary(&source, 1, BoundaryMove::Later),
            Err(BoundaryError::NoSuchBoundary)
        );
        assert!(takes.as_slice().iter().all(|take| take.flag().is_none()));
    }

    #[test]
    fn a_take_keeps_its_mode_when_its_cut_moves() {
        let text = sentences(6, 12);
        let words = timed(&text, 0.4, 0.1, &[]);
        let source = TakeSource {
            text: &text,
            words: &words,
            duration: words.last().unwrap().end,
        };
        let mut takes = Takes::split(&source);
        assert_eq!(takes.len(), 2);
        assert!(
            takes
                .as_slice()
                .iter()
                .all(|t| t.mode == TakeMode::Recording)
        );
        takes.set_mode(1, TakeMode::Depth).expect("take 1");
        assert_eq!(takes.as_slice()[1].mode, TakeMode::Depth);
        assert_eq!(takes.as_slice()[1].revision, 1, "same stretch");
        takes
            .move_boundary(&source, 0, BoundaryMove::Earlier)
            .unwrap();
        assert_eq!(takes.as_slice()[0].mode, TakeMode::Recording);
        assert_eq!(takes.as_slice()[1].mode, TakeMode::Depth);
        assert_eq!(
            takes.set_mode(2, TakeMode::Mesh),
            Err(BoundaryError::NoSuchTake)
        );
    }

    #[test]
    fn stored_takes_must_cover_the_words_in_order() {
        let take = |words: Range<usize>| Take {
            words,
            start: s(0.0),
            end: s(1.0),
            revision: 1,
            mode: TakeMode::Recording,
        };
        assert!(Takes::restore(10, vec![take(0..4), take(4..10)]).is_ok());
        assert!(Takes::restore(0, vec![]).is_ok());
        assert_eq!(
            Takes::restore(10, vec![take(0..4), take(5..10)]),
            Err(InvalidTakes)
        );
        assert_eq!(Takes::restore(10, vec![take(0..4)]), Err(InvalidTakes));
        assert_eq!(
            Takes::restore(4, vec![take(0..0), take(0..4)]),
            Err(InvalidTakes)
        );
    }

    #[test]
    fn no_words_make_no_takes() {
        let source = TakeSource {
            text: "",
            words: &[],
            duration: s(5.0),
        };
        assert!(Takes::split(&source).is_empty());
    }
}
