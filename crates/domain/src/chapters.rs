//! YouTube chapters (#182, ADR-0021): timestamps in a long video's
//! description that split it into named parts. Claude picks where each
//! chapter starts, at a sentence of the narration, and names it; the user
//! renames, removes or moves them.
//!
//! A chapter starts at a moment of the narration, not of the video, so the
//! editor's trims move it with the cut: its time in the video comes from
//! the cut as it is now ([`place_chapters`]). YouTube shows chapters only
//! when they follow its rules (checked 2026-10-22,
//! <https://support.google.com/youtube/answer/9884579>): the first at
//! 0:00, at least three, in ascending order, each at least 10 seconds.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{Generation, NarrationId, ProfileId, RepositoryError, Timeline, VideoProjectId};

/// The fewest chapters YouTube shows.
pub const MIN_CHAPTERS: usize = 3;

/// The shortest chapter YouTube shows.
pub const MIN_CHAPTER_LENGTH: Duration = Duration::from_secs(10);

/// The longest chapter title, in characters. YouTube sets no limit; a
/// title is a short label in the progress bar.
pub const MAX_CHAPTER_TITLE: usize = 100;

/// Why a chapter title is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChapterFieldError {
    TitleRequired,
    TitleTooLong,
}

impl ChapterFieldError {
    pub const ALL: [ChapterFieldError; 2] = [
        ChapterFieldError::TitleRequired,
        ChapterFieldError::TitleTooLong,
    ];
}

/// The title as a chapter holds it: one line, spaces collapsed, present
/// and within [`MAX_CHAPTER_TITLE`].
pub fn chapter_title(text: &str) -> Result<String, ChapterFieldError> {
    let title = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        Err(ChapterFieldError::TitleRequired)
    } else if title.chars().count() > MAX_CHAPTER_TITLE {
        Err(ChapterFieldError::TitleTooLong)
    } else {
        Ok(title)
    }
}

/// One chapter: where it starts in the narration file and its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    pub start: Duration,
    pub title: String,
}

/// A chapter edit that cannot be kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("chapter {index}: {error:?}")]
pub struct InvalidChapter {
    pub index: usize,
    pub error: ChapterFieldError,
}

/// In start order, one chapter per start (the first title kept), every
/// title valid.
fn ordered(mut list: Vec<Chapter>) -> Result<Vec<Chapter>, InvalidChapter> {
    for (index, chapter) in list.iter_mut().enumerate() {
        chapter.title =
            chapter_title(&chapter.title).map_err(|error| InvalidChapter { index, error })?;
    }
    list.sort_by_key(|chapter| chapter.start);
    list.dedup_by_key(|chapter| chapter.start);
    Ok(list)
}

/// A video project's chapters, as Claude wrote them or as the user edited
/// them, the narration they were written for, and whether they go in the
/// description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapters {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    list: Vec<Chapter>,
    /// They go in YouTube's description.
    pub include: bool,
    /// The narration their starts are times of.
    pub narration: NarrationId,
    generation: Generation,
    edited: bool,
    pub updated_at: SystemTime,
}

impl Chapters {
    /// What `generation` wrote for `narration`, in the description.
    pub fn generated(
        narration: NarrationId,
        list: Vec<Chapter>,
        generation: Generation,
        now: SystemTime,
    ) -> Result<Self, InvalidChapter> {
        Ok(Self {
            project: generation.project,
            owner: generation.owner,
            list: ordered(list)?,
            include: true,
            narration,
            generation,
            edited: false,
            updated_at: now,
        })
    }

    /// Stored chapters as they were saved.
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        project: VideoProjectId,
        owner: ProfileId,
        list: Vec<Chapter>,
        include: bool,
        narration: NarrationId,
        generation: Generation,
        edited: bool,
        updated_at: SystemTime,
    ) -> Self {
        Self {
            project,
            owner,
            list,
            include,
            narration,
            generation,
            edited,
            updated_at,
        }
    }

    pub fn list(&self) -> &[Chapter] {
        &self.list
    }

    pub fn generation(&self) -> &Generation {
        &self.generation
    }

    /// Whether the user changed what Claude wrote.
    pub fn is_edited(&self) -> bool {
        self.edited
    }

    /// Replaces the chapters with the user's and says whether they go in
    /// the description; `false` when nothing changed.
    pub fn edit(
        &mut self,
        list: Vec<Chapter>,
        include: bool,
        now: SystemTime,
    ) -> Result<bool, InvalidChapter> {
        let list = ordered(list)?;
        if list == self.list && include == self.include {
            return Ok(false);
        }
        if list != self.list {
            self.edited = true;
        }
        self.list = list;
        self.include = include;
        self.updated_at = now;
        Ok(true)
    }
}

/// Why YouTube would not show the chapters, as the cut is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChapterProblem {
    /// Fewer than [`MIN_CHAPTERS`].
    TooFew,
    /// Chapter `n` (from 0) runs less than [`MIN_CHAPTER_LENGTH`].
    TooShort(usize),
    /// The cut no longer plays any narration from chapter `n`'s start on.
    CutAway(usize),
}

/// Where each chapter starts in the video, as `timeline` plays the
/// narration: the moment its start plays, or where the cut picks the
/// narration up again after it when that moment was cut. `None` when the
/// cut plays nothing of the narration from there on. The first chapter
/// starts the video, at 0:00.
pub fn place_chapters(chapters: &[Chapter], timeline: &Timeline) -> Vec<Option<Duration>> {
    chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            if index == 0 {
                return Some(Duration::ZERO);
            }
            timeline.on_narration(chapter.start).or_else(|| {
                timeline
                    .narration()
                    .iter()
                    .filter(|item| item.start > chapter.start)
                    .map(|item| item.at)
                    .min()
            })
        })
        .collect()
}

/// Whole seconds, as a timestamp shows a time.
fn seconds(time: Duration) -> u64 {
    time.as_secs()
}

/// What keeps YouTube from showing chapters placed at `placed` in a video
/// of `length`, in chapter order. Lengths go by whole seconds, as the
/// timestamps show them.
pub fn chapter_problems(placed: &[Option<Duration>], length: Duration) -> Vec<ChapterProblem> {
    let mut problems = Vec::new();
    if placed.len() < MIN_CHAPTERS {
        problems.push(ChapterProblem::TooFew);
    }
    let kept: Vec<(usize, u64)> = placed
        .iter()
        .enumerate()
        .filter_map(|(index, at)| Some((index, seconds((*at)?))))
        .collect();
    for (index, at) in placed.iter().enumerate() {
        if at.is_none() {
            problems.push(ChapterProblem::CutAway(index));
        }
    }
    for (position, &(index, start)) in kept.iter().enumerate() {
        let end = kept
            .get(position + 1)
            .map_or(seconds(length), |&(_, next)| next);
        if end.saturating_sub(start) < MIN_CHAPTER_LENGTH.as_secs() {
            problems.push(ChapterProblem::TooShort(index));
        }
    }
    problems.sort_by_key(|problem| match problem {
        ChapterProblem::TooFew => 0,
        ChapterProblem::TooShort(index) | ChapterProblem::CutAway(index) => index + 1,
    });
    problems
}

/// A time as a chapter timestamp: `m:ss`, or `h:mm:ss` in a video of an
/// hour or more.
pub fn timestamp(time: Duration, length: Duration) -> String {
    let total = seconds(time);
    if seconds(length) >= 3_600 {
        format!("{}:{:02}:{:02}", total / 3_600, total / 60 % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    }
}

/// The chapters as YouTube reads them from a description: one line per
/// chapter, its timestamp then its title.
pub fn chapter_lines(chapters: &[(Duration, &str)], length: Duration) -> String {
    chapters
        .iter()
        .map(|(at, title)| format!("{} {title}", timestamp(*at, length)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Persistence port for chapters. Shared with job worker threads.
pub trait ChapterRepository: Send + Sync {
    fn chapters(&self, project: VideoProjectId) -> Result<Option<Chapters>, RepositoryError>;

    /// Saves the chapters, and their generation when it is new.
    fn save_chapters(&self, chapters: &Chapters) -> Result<(), RepositoryError>;
}

impl<T: ChapterRepository + ?Sized> ChapterRepository for Arc<T> {
    fn chapters(&self, project: VideoProjectId) -> Result<Option<Chapters>, RepositoryError> {
        (**self).chapters(project)
    }

    fn save_chapters(&self, chapters: &Chapters) -> Result<(), RepositoryError> {
        (**self).save_chapters(chapters)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::generation;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn chapter(start: u64, title: &str) -> Chapter {
        Chapter {
            start: secs(start),
            title: title.into(),
        }
    }

    #[test]
    fn titles_are_one_line_and_present() {
        assert_eq!(
            chapter_title(" The  dig\nbegins "),
            Ok("The dig begins".into())
        );
        assert_eq!(chapter_title(" "), Err(ChapterFieldError::TitleRequired));
        assert_eq!(
            chapter_title(&"a".repeat(101)),
            Err(ChapterFieldError::TitleTooLong)
        );
    }

    #[test]
    fn written_chapters_are_in_order_one_per_start() {
        let chapters = Chapters::generated(
            NarrationId::new(),
            vec![
                chapter(40, "Later"),
                chapter(0, "Start"),
                chapter(40, "Twice"),
            ],
            generation(),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(chapters.list(), [chapter(0, "Start"), chapter(40, "Later")]);
        assert!(chapters.include);
        assert!(!chapters.is_edited());
        let blank = Chapters::generated(
            NarrationId::new(),
            vec![chapter(0, "Start"), chapter(9, " ")],
            generation(),
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(
            blank,
            Err(InvalidChapter {
                index: 1,
                error: ChapterFieldError::TitleRequired
            })
        );
    }

    #[test]
    fn edits_mark_the_chapters_edited_but_the_switch_alone_does_not() {
        let mut chapters = Chapters::generated(
            NarrationId::new(),
            vec![chapter(0, "Start"), chapter(40, "Later")],
            generation(),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        let now = SystemTime::UNIX_EPOCH + secs(5);
        let same = chapters.list().to_vec();
        assert_eq!(chapters.edit(same.clone(), true, now), Ok(false));
        assert_eq!(chapters.edit(same, false, now), Ok(true));
        assert!(!chapters.is_edited());
        assert!(!chapters.include);
        assert_eq!(
            chapters.edit(vec![chapter(0, "Opening")], false, now),
            Ok(true)
        );
        assert!(chapters.is_edited());
        assert_eq!(chapters.updated_at, now);
    }

    #[test]
    fn youtube_needs_three_chapters_of_ten_seconds() {
        let placed = |times: &[u64]| times.iter().map(|t| Some(secs(*t))).collect::<Vec<_>>();
        assert!(chapter_problems(&placed(&[0, 10, 20]), secs(30)).is_empty());
        assert_eq!(
            chapter_problems(&placed(&[0, 10]), secs(30)),
            [ChapterProblem::TooFew]
        );
        assert_eq!(
            chapter_problems(&placed(&[0, 9, 20]), secs(30)),
            [ChapterProblem::TooShort(0)]
        );
        // The last chapter runs to the end of the video.
        assert_eq!(
            chapter_problems(&placed(&[0, 10, 20]), secs(29)),
            [ChapterProblem::TooShort(2)]
        );
        // Whole seconds, as the timestamps show: 9.9 s reads 0:00 to 0:09.
        let tenths = vec![
            Some(Duration::ZERO),
            Some(Duration::from_millis(9_900)),
            Some(secs(30)),
        ];
        assert_eq!(
            chapter_problems(&tenths, secs(40)),
            [ChapterProblem::TooShort(0)]
        );
        // A chapter the cut removed: the one before runs on to the next.
        let cut = vec![Some(Duration::ZERO), None, Some(secs(20)), Some(secs(40))];
        assert_eq!(
            chapter_problems(&cut, secs(60)),
            [ChapterProblem::CutAway(1)]
        );
    }

    #[test]
    fn chapters_follow_the_narration_through_the_cut() {
        use crate::timeline::tests::{narration, plan, scene};
        let mut timeline = Timeline::rough_cut(
            &plan(vec![scene(0, 120_000)]),
            &narration(120_000),
            crate::AspectRatio::Landscape,
        );
        // The cut keeps 0-30 s, drops 30-50 s, and plays 50-120 s from 30 s.
        let whole = timeline.narration[0].clone();
        timeline.narration = vec![
            crate::AudioItem {
                duration: secs(30),
                ..whole.clone()
            },
            crate::AudioItem {
                start: secs(50),
                at: secs(30),
                duration: secs(70),
                ..whole
            },
        ];
        let chapters = [
            chapter(2, "Start"),
            chapter(20, "Kept"),
            chapter(40, "Cut"),
            chapter(80, "Moved"),
            chapter(119, "End"),
        ];
        assert_eq!(
            place_chapters(&chapters, &timeline),
            [
                Some(Duration::ZERO),
                Some(secs(20)),
                // Cut away: where the narration picks up again.
                Some(secs(30)),
                Some(secs(60)),
                Some(secs(99)),
            ]
        );
        timeline.narration.truncate(1);
        assert_eq!(place_chapters(&chapters, &timeline)[3], None);
    }

    #[test]
    fn timestamps_show_hours_only_in_videos_of_an_hour() {
        assert_eq!(timestamp(secs(0), secs(600)), "0:00");
        assert_eq!(timestamp(Duration::from_millis(83_900), secs(600)), "1:23");
        assert_eq!(timestamp(secs(754), secs(3_600)), "0:12:34");
        assert_eq!(timestamp(secs(3_725), secs(4_000)), "1:02:05");
        assert_eq!(
            chapter_lines(&[(secs(0), "Start"), (secs(75), "The dig")], secs(600)),
            "0:00 Start\n1:15 The dig"
        );
    }
}
