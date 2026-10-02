//! Cut editing (PRD stories 58, 59, 70): split, trim, move, reorder and
//! delete items of a timeline, each an [`Edit`] that, applied, hands back
//! the edit undoing it. A [`History`] keeps those to undo and redo.
//!
//! Edits are exact: they cut where they are told, and the caller snaps the
//! time first (to a frame, and to a word when snapping is on). Trims and
//! moves stop where the item runs out: at one frame long, at the start or
//! end of its file, at the item beside it on an audio track.

use std::time::Duration;

use crate::{AudioItem, Timeline, VideoItem, min_length};

/// A track of the timeline that edits reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Track {
    /// V1: the magnetic video track.
    Video,
    /// A1: the narration.
    Narration,
}

/// One item of the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemRef {
    pub track: Track,
    pub index: usize,
}

impl ItemRef {
    pub fn video(index: usize) -> Self {
        Self {
            track: Track::Video,
            index,
        }
    }

    pub fn narration(index: usize) -> Self {
        Self {
            track: Track::Narration,
            index,
        }
    }
}

/// Which end of an item a trim moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    Start,
    End,
}

/// A signed span of timeline time, in nanoseconds: how far a trim moves an
/// edge (later when positive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Shift(i64);

impl Shift {
    pub const ZERO: Shift = Shift(0);
    /// Further than any edit goes: no bound.
    const FAR: Shift = Shift(i64::MAX / 4);

    fn nanos(duration: Duration) -> i64 {
        i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX / 4)
    }

    pub fn later(by: Duration) -> Self {
        Shift(Self::nanos(by))
    }

    pub fn earlier(by: Duration) -> Self {
        Shift(-Self::nanos(by))
    }

    /// From `from` to `to`.
    pub fn between(from: Duration, to: Duration) -> Self {
        Shift(Self::nanos(to) - Self::nanos(from))
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// `time` moved by this shift; `None` before zero.
    pub fn move_time(self, time: Duration) -> Option<Duration> {
        let moved = Self::nanos(time).checked_add(self.0)?;
        u64::try_from(moved).ok().map(Duration::from_nanos)
    }

    fn reversed(self) -> Self {
        Shift(-self.0)
    }
}

impl std::ops::Neg for Shift {
    type Output = Shift;

    fn neg(self) -> Shift {
        self.reversed()
    }
}

/// An item taken out of a track, to put back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Video(VideoItem),
    Audio(AudioItem),
}

/// One change to a timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Cuts the item in two at `at` (timeline time); each part keeps at
    /// least a frame.
    Split {
        track: Track,
        index: usize,
        at: Duration,
    },
    /// Joins the item to the next one when they are the two halves of a
    /// split: the undo of `Split`.
    Join { track: Track, index: usize },
    /// Moves one edge of the item. On the video track the items after it
    /// follow (the track is magnetic); moving the start edge of a video
    /// item cuts into or out of its source while the item stays put.
    Trim {
        track: Track,
        index: usize,
        edge: Edge,
        by: Shift,
    },
    /// Moves an audio item to start at `to`, short of the items beside it.
    Move {
        track: Track,
        index: usize,
        to: Duration,
    },
    /// Takes the video item at `from` to position `to`.
    Reorder { from: usize, to: usize },
    /// Removes the item; on the video track the ones after it close up.
    Delete { track: Track, index: usize },
    /// Puts an item at `index`: the undo of `Delete`. An audio item keeps
    /// its time and must fit there.
    Insert {
        track: Track,
        index: usize,
        item: Item,
    },
}

/// Why an edit was not made. The timeline is left as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("the track has no such item")]
    NoSuchItem,
    /// A split must leave each part at least a frame long.
    #[error("the cut is not inside the item")]
    OutsideItem,
    /// Moving is for audio items, reordering for video ones.
    #[error("the edit does not apply to that track")]
    WrongTrack,
    #[error("the item overlaps another")]
    Overlaps,
    #[error("the items are not two halves of one cut")]
    CannotJoin,
    /// The item is already there, or cannot go further that way.
    #[error("the edit changes nothing")]
    NoChange,
}

impl Timeline {
    fn len_of(&self, track: Track) -> usize {
        match track {
            Track::Video => self.video.len(),
            Track::Narration => self.narration.len(),
        }
    }

    fn check(&self, track: Track, index: usize) -> Result<(), EditError> {
        if index < self.len_of(track) {
            Ok(())
        } else {
            Err(EditError::NoSuchItem)
        }
    }

    /// Where on the timeline the item starts and how long it is.
    pub fn span(&self, item: ItemRef) -> Option<(Duration, Duration)> {
        match item.track {
            Track::Video => self.video.get(item.index).map(|v| (v.at, v.duration)),
            Track::Narration => self.narration.get(item.index).map(|a| (a.at, a.duration)),
        }
    }

    /// The end of the audio item before `index`, or zero.
    fn audio_floor(&self, index: usize) -> Duration {
        index
            .checked_sub(1)
            .map_or(Duration::ZERO, |before| self.narration[before].end())
    }

    /// How far each edge of an item can go: the shortest and longest shift
    /// a trim of `edge` makes.
    pub fn trim_limits(&self, item: ItemRef, edge: Edge) -> Result<(Shift, Shift), EditError> {
        self.check(item.track, item.index)?;
        let (_, duration) = self.span(item).ok_or(EditError::NoSuchItem)?;
        // Each edge stops a frame short of the other.
        let shrink = Shift::later(duration.saturating_sub(min_length()));
        Ok(match (item.track, edge) {
            (Track::Video, Edge::Start) => {
                let video = &self.video[item.index];
                let earliest = if video.has_source_time() {
                    Shift::earlier(video.start)
                } else {
                    -Shift::FAR
                };
                (earliest, shrink)
            }
            (Track::Video, Edge::End) => (-shrink, Shift::FAR),
            (Track::Narration, Edge::Start) => {
                let audio = &self.narration[item.index];
                let room = audio.at - self.audio_floor(item.index);
                (Shift::earlier(audio.start.min(room)), shrink)
            }
            (Track::Narration, Edge::End) => {
                let audio = &self.narration[item.index];
                let in_file = audio.length.saturating_sub(audio.start + audio.duration);
                let room = self
                    .narration
                    .get(item.index + 1)
                    .map_or(in_file, |next| in_file.min(next.at - audio.end()));
                (-shrink, Shift::later(room))
            }
        })
    }

    /// The earliest and latest start an audio item can move to; the latest
    /// is `None` when nothing comes after it.
    pub fn move_limits(&self, item: ItemRef) -> Result<(Duration, Option<Duration>), EditError> {
        if item.track != Track::Narration {
            return Err(EditError::WrongTrack);
        }
        self.check(item.track, item.index)?;
        let audio = &self.narration[item.index];
        let latest = self
            .narration
            .get(item.index + 1)
            .map(|next| next.at - audio.duration);
        Ok((self.audio_floor(item.index), latest))
    }

    /// Makes `edit` and returns the edit that undoes it. On error the
    /// timeline is unchanged.
    pub fn apply(&mut self, edit: &Edit) -> Result<Edit, EditError> {
        let undo = match edit {
            Edit::Split { track, index, at } => self.split(*track, *index, *at)?,
            Edit::Join { track, index } => self.join(*track, *index)?,
            Edit::Trim {
                track,
                index,
                edge,
                by,
            } => self.trim(*track, *index, *edge, *by)?,
            Edit::Move { track, index, to } => self.move_item(*track, *index, *to)?,
            Edit::Reorder { from, to } => self.reorder(*from, *to)?,
            Edit::Delete { track, index } => self.delete(*track, *index)?,
            Edit::Insert { track, index, item } => self.insert(*track, *index, item)?,
        };
        self.relayout();
        Ok(undo)
    }

    fn split(&mut self, track: Track, index: usize, at: Duration) -> Result<Edit, EditError> {
        self.check(track, index)?;
        let (start, duration) = self
            .span(ItemRef { track, index })
            .ok_or(EditError::NoSuchItem)?;
        let end = start + duration;
        if at < start + min_length() || at + min_length() > end {
            return Err(EditError::OutsideItem);
        }
        let first = at - start;
        match track {
            Track::Video => {
                let mut second = self.video[index].clone();
                if second.has_source_time() {
                    second.start += first;
                }
                second.at = at;
                second.duration = end - at;
                self.video[index].duration = first;
                self.video.insert(index + 1, second);
            }
            Track::Narration => {
                let mut second = self.narration[index].clone();
                second.start += first;
                second.at = at;
                second.duration = end - at;
                self.narration[index].duration = first;
                self.narration.insert(index + 1, second);
            }
        }
        Ok(Edit::Join { track, index })
    }

    fn join(&mut self, track: Track, index: usize) -> Result<Edit, EditError> {
        self.check(track, index)?;
        self.check(track, index + 1)
            .map_err(|_| EditError::CannotJoin)?;
        let at = match track {
            Track::Video => {
                let (first, second) = (&self.video[index], &self.video[index + 1]);
                let resumes = if first.has_source_time() {
                    second.start == first.start + first.duration
                } else {
                    second.start == first.start
                };
                if first.scene != second.scene || first.source != second.source || !resumes {
                    return Err(EditError::CannotJoin);
                }
                let second = self.video.remove(index + 1);
                self.video[index].duration += second.duration;
                second.at
            }
            Track::Narration => {
                let (first, second) = (&self.narration[index], &self.narration[index + 1]);
                if first.file != second.file
                    || first.end() != second.at
                    || second.start != first.start + first.duration
                {
                    return Err(EditError::CannotJoin);
                }
                let second = self.narration.remove(index + 1);
                self.narration[index].duration += second.duration;
                second.at
            }
        };
        Ok(Edit::Split { track, index, at })
    }

    fn trim(
        &mut self,
        track: Track,
        index: usize,
        edge: Edge,
        by: Shift,
    ) -> Result<Edit, EditError> {
        let (earliest, latest) = self.trim_limits(ItemRef { track, index }, edge)?;
        let by = by.clamp(earliest, latest);
        if by.is_zero() {
            return Err(EditError::NoChange);
        }
        // Within the limits every time below stays positive.
        let moved = |time: Duration| by.move_time(time).ok_or(EditError::NoChange);
        let shrunk = |time: Duration| (-by).move_time(time).ok_or(EditError::NoChange);
        match (track, edge) {
            (Track::Video, Edge::Start) => {
                let item = &mut self.video[index];
                let duration = shrunk(item.duration)?;
                if item.has_source_time() {
                    item.start = moved(item.start)?;
                }
                item.duration = duration;
            }
            (Track::Video, Edge::End) => {
                let item = &mut self.video[index];
                item.duration = moved(item.duration)?;
            }
            (Track::Narration, Edge::Start) => {
                let item = &mut self.narration[index];
                let (at, start, duration) =
                    (moved(item.at)?, moved(item.start)?, shrunk(item.duration)?);
                (item.at, item.start, item.duration) = (at, start, duration);
            }
            (Track::Narration, Edge::End) => {
                let item = &mut self.narration[index];
                item.duration = moved(item.duration)?;
            }
        }
        Ok(Edit::Trim {
            track,
            index,
            edge,
            by: -by,
        })
    }

    fn move_item(&mut self, track: Track, index: usize, to: Duration) -> Result<Edit, EditError> {
        let (earliest, latest) = self.move_limits(ItemRef { track, index })?;
        let to = latest.map_or(to, |latest| to.min(latest)).max(earliest);
        let item = &mut self.narration[index];
        if to == item.at {
            return Err(EditError::NoChange);
        }
        let from = std::mem::replace(&mut item.at, to);
        Ok(Edit::Move {
            track,
            index,
            to: from,
        })
    }

    fn reorder(&mut self, from: usize, to: usize) -> Result<Edit, EditError> {
        self.check(Track::Video, from)?;
        self.check(Track::Video, to)?;
        if from == to {
            return Err(EditError::NoChange);
        }
        let item = self.video.remove(from);
        self.video.insert(to, item);
        Ok(Edit::Reorder { from: to, to: from })
    }

    fn delete(&mut self, track: Track, index: usize) -> Result<Edit, EditError> {
        self.check(track, index)?;
        let item = match track {
            Track::Video => Item::Video(self.video.remove(index)),
            Track::Narration => Item::Audio(self.narration.remove(index)),
        };
        Ok(Edit::Insert { track, index, item })
    }

    fn insert(&mut self, track: Track, index: usize, item: &Item) -> Result<Edit, EditError> {
        if index > self.len_of(track) {
            return Err(EditError::NoSuchItem);
        }
        match (track, item) {
            (Track::Video, Item::Video(video)) => {
                if video.duration < min_length() {
                    return Err(EditError::OutsideItem);
                }
                self.video.insert(index, video.clone());
            }
            (Track::Narration, Item::Audio(audio)) => {
                let after_previous = self.audio_floor(index) <= audio.at;
                let before_next = self
                    .narration
                    .get(index)
                    .is_none_or(|next| audio.end() <= next.at);
                if !after_previous || !before_next {
                    return Err(EditError::Overlaps);
                }
                if audio.duration < min_length() || audio.start + audio.duration > audio.length {
                    return Err(EditError::OutsideItem);
                }
                self.narration.insert(index, audio.clone());
            }
            _ => return Err(EditError::WrongTrack),
        }
        Ok(Edit::Delete { track, index })
    }
}

/// How many edits a history keeps to undo.
pub const HISTORY_DEPTH: usize = 200;

/// The edits made to a timeline, to undo and redo: each entry is the edit
/// that undoes (or redoes) one step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl History {
    /// Makes `edit` on `timeline` and remembers how to undo it. A new edit
    /// forgets what was undone.
    pub fn apply(&mut self, timeline: &mut Timeline, edit: &Edit) -> Result<(), EditError> {
        let undo = timeline.apply(edit)?;
        if self.undo.len() == HISTORY_DEPTH {
            self.undo.remove(0);
        }
        self.undo.push(undo);
        self.redo.clear();
        Ok(())
    }

    /// Undoes the last edit; `false` when there is none.
    pub fn undo(&mut self, timeline: &mut Timeline) -> Result<bool, EditError> {
        Self::step(&mut self.undo, &mut self.redo, timeline)
    }

    /// Makes again the last edit undone; `false` when there is none.
    pub fn redo(&mut self, timeline: &mut Timeline) -> Result<bool, EditError> {
        Self::step(&mut self.redo, &mut self.undo, timeline)
    }

    fn step(
        from: &mut Vec<Edit>,
        to: &mut Vec<Edit>,
        timeline: &mut Timeline,
    ) -> Result<bool, EditError> {
        let Some(edit) = from.pop() else {
            return Ok(false);
        };
        match timeline.apply(&edit) {
            Ok(back) => {
                to.push(back);
                Ok(true)
            }
            Err(error) => {
                // The timeline is not the one these edits were made on.
                from.clear();
                to.clear();
                Err(error)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Forgets every edit, as when the timeline changed under them.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::{image, ms, narration, plan, scene};
    use crate::{SceneClip, VideoSource, frame_time};

    /// Three scenes over a 4 s narration: a still (0-1 s), a clip (1-3 s)
    /// and a still (3-4 s).
    fn timeline() -> Timeline {
        let mut first = scene(0, 1_000);
        first.image = Some(image("a.png"));
        let mut second = scene(1_000, 3_000);
        let drawn = image("b.png");
        second.clip = Some(SceneClip {
            file: "b.mp4".into(),
            seconds: 5,
            source_image: drawn.generation.id,
            generation: drawn.generation.clone(),
        });
        second.image = Some(drawn);
        let mut third = scene(3_000, 4_000);
        third.image = Some(image("c.png"));
        Timeline::rough_cut(&plan(vec![first, second, third]), &narration(4_000))
    }

    /// Makes `edit`, checks its undo brings the timeline back and the
    /// undo's undo makes it again; returns the edited timeline.
    fn round_trip(before: &Timeline, edit: Edit) -> Timeline {
        let mut timeline = before.clone();
        let undo = timeline.apply(&edit).unwrap();
        let after = timeline.clone();
        let redo = timeline.apply(&undo).unwrap();
        assert_eq!(&timeline, before, "{edit:?} undone by {undo:?}");
        timeline.apply(&redo).unwrap();
        assert_eq!(timeline, after, "{edit:?} redone by {redo:?}");
        after
    }

    fn spans(timeline: &Timeline, track: Track) -> Vec<(Duration, Duration)> {
        (0..timeline.len_of(track))
            .map(|index| timeline.span(ItemRef { track, index }).unwrap())
            .collect()
    }

    fn assert_unchanged(edit: Edit, error: EditError) {
        let before = timeline();
        let mut after = before.clone();
        assert_eq!(after.apply(&edit), Err(error), "{edit:?}");
        assert_eq!(after, before);
    }

    #[test]
    fn splitting_a_still_makes_two_stills_that_join_back() {
        let after = round_trip(
            &timeline(),
            Edit::Split {
                track: Track::Video,
                index: 0,
                at: ms(400),
            },
        );
        let video = after.video();
        assert_eq!(video.len(), 4);
        assert_eq!((video[0].at, video[0].duration), (ms(0), ms(400)));
        assert_eq!((video[1].at, video[1].duration), (ms(400), ms(600)));
        assert_eq!(video[1].source, VideoSource::Still("a.png".into()));
        assert_eq!(video[1].start, Duration::ZERO, "a still has no time");
        assert_eq!(after.duration(), ms(4_000));
    }

    #[test]
    fn splitting_a_clip_starts_its_second_half_further_in() {
        let after = round_trip(
            &timeline(),
            Edit::Split {
                track: Track::Video,
                index: 1,
                at: ms(2_500),
            },
        );
        assert_eq!(after.video()[2].start, ms(1_500));
        assert_eq!(after.video()[2].at, ms(2_500));
        assert_eq!(after.video()[3].at, ms(3_000));
    }

    #[test]
    fn splitting_the_narration_continues_the_file() {
        let after = round_trip(
            &timeline(),
            Edit::Split {
                track: Track::Narration,
                index: 0,
                at: ms(1_500),
            },
        );
        let pieces = after.narration();
        assert_eq!((pieces[1].start, pieces[1].at), (ms(1_500), ms(1_500)));
        assert_eq!(pieces[1].duration, ms(2_500));
    }

    #[test]
    fn a_split_leaves_each_part_at_least_a_frame() {
        for at in [
            ms(0),
            frame_time(1) - Duration::from_nanos(1),
            ms(1_000),
            ms(5_000),
        ] {
            assert_unchanged(
                Edit::Split {
                    track: Track::Video,
                    index: 0,
                    at,
                },
                EditError::OutsideItem,
            );
        }
        // One frame from either edge, at the track's start and end.
        round_trip(
            &timeline(),
            Edit::Split {
                track: Track::Video,
                index: 0,
                at: frame_time(1),
            },
        );
        let after = round_trip(
            &timeline(),
            Edit::Split {
                track: Track::Video,
                index: 2,
                at: frame_time(119),
            },
        );
        assert_eq!(after.video()[3].duration, frame_time(120) - frame_time(119));
        assert_unchanged(
            Edit::Split {
                track: Track::Video,
                index: 3,
                at: ms(3_500),
            },
            EditError::NoSuchItem,
        );
    }

    #[test]
    fn only_two_halves_of_a_cut_join() {
        assert_unchanged(
            Edit::Join {
                track: Track::Video,
                index: 0,
            },
            EditError::CannotJoin,
        );
        assert_unchanged(
            Edit::Join {
                track: Track::Video,
                index: 2,
            },
            EditError::CannotJoin,
        );
        // Halves moved apart no longer join.
        let mut timeline = timeline();
        timeline
            .apply(&Edit::Split {
                track: Track::Narration,
                index: 0,
                at: ms(2_000),
            })
            .unwrap();
        timeline
            .apply(&Edit::Move {
                track: Track::Narration,
                index: 1,
                to: ms(2_500),
            })
            .unwrap();
        assert_eq!(
            timeline.apply(&Edit::Join {
                track: Track::Narration,
                index: 0,
            }),
            Err(EditError::CannotJoin)
        );
    }

    #[test]
    fn trimming_a_video_end_moves_the_clips_after_it() {
        let after = round_trip(
            &timeline(),
            Edit::Trim {
                track: Track::Video,
                index: 0,
                edge: Edge::End,
                by: Shift::earlier(ms(400)),
            },
        );
        assert_eq!(
            spans(&after, Track::Video),
            vec![
                (ms(0), ms(600)),
                (ms(600), ms(2_000)),
                (ms(2_600), ms(1_000))
            ]
        );
        // The narration does not move: it now runs past the video.
        assert_eq!(after.duration(), ms(4_000));
        assert_eq!(after.video_end(), ms(3_600));

        let longer = round_trip(
            &timeline(),
            Edit::Trim {
                track: Track::Video,
                index: 2,
                edge: Edge::End,
                by: Shift::later(ms(2_000)),
            },
        );
        assert_eq!(
            longer.duration(),
            ms(6_000),
            "a still holds as long as asked"
        );
    }

    #[test]
    fn trimming_a_clip_start_cuts_into_its_source_and_stops_at_its_first_frame() {
        let after = round_trip(
            &timeline(),
            Edit::Trim {
                track: Track::Video,
                index: 1,
                edge: Edge::Start,
                by: Shift::later(ms(500)),
            },
        );
        let clip = &after.video()[1];
        assert_eq!(
            (clip.start, clip.at, clip.duration),
            (ms(500), ms(1_000), ms(1_500))
        );
        assert_eq!(after.video()[2].at, ms(2_500));

        // Back out past the clip's first frame: it stops there.
        let mut back = after.clone();
        back.apply(&Edit::Trim {
            track: Track::Video,
            index: 1,
            edge: Edge::Start,
            by: Shift::earlier(ms(2_000)),
        })
        .unwrap();
        assert_eq!(back, timeline());
        assert_unchanged(
            Edit::Trim {
                track: Track::Video,
                index: 1,
                edge: Edge::Start,
                by: Shift::earlier(ms(100)),
            },
            EditError::NoChange,
        );
    }

    #[test]
    fn trimming_a_still_start_has_no_source_to_run_out_of() {
        let after = round_trip(
            &timeline(),
            Edit::Trim {
                track: Track::Video,
                index: 0,
                edge: Edge::Start,
                by: Shift::earlier(ms(700)),
            },
        );
        assert_eq!(after.video()[0].duration, ms(1_700));
        assert_eq!(after.video()[0].start, Duration::ZERO);
    }

    #[test]
    fn a_trim_never_leaves_less_than_a_frame() {
        for edge in [Edge::Start, Edge::End] {
            let after = round_trip(
                &timeline(),
                Edit::Trim {
                    track: Track::Video,
                    index: 0,
                    edge,
                    by: if edge == Edge::Start {
                        Shift::later(ms(9_000))
                    } else {
                        Shift::earlier(ms(9_000))
                    },
                },
            );
            assert_eq!(after.video()[0].duration, frame_time(1), "{edge:?}");
        }
        let mut one_frame = timeline();
        one_frame
            .apply(&Edit::Trim {
                track: Track::Video,
                index: 0,
                edge: Edge::End,
                by: Shift::between(ms(1_000), frame_time(1)),
            })
            .unwrap();
        assert_eq!(
            one_frame.apply(&Edit::Trim {
                track: Track::Video,
                index: 0,
                edge: Edge::End,
                by: Shift::earlier(Duration::from_nanos(1)),
            }),
            Err(EditError::NoChange)
        );
    }

    /// The narration in two pieces: 0-1 s at 0, and 2-3 s of the file at
    /// 1.5 s.
    fn cut_narration() -> Timeline {
        let mut timeline = timeline();
        timeline
            .apply(&Edit::Split {
                track: Track::Narration,
                index: 0,
                at: ms(1_000),
            })
            .unwrap();
        timeline
            .apply(&Edit::Trim {
                track: Track::Narration,
                index: 1,
                edge: Edge::Start,
                by: Shift::later(ms(1_000)),
            })
            .unwrap();
        timeline
            .apply(&Edit::Trim {
                track: Track::Narration,
                index: 1,
                edge: Edge::End,
                by: Shift::earlier(ms(1_000)),
            })
            .unwrap();
        timeline
            .apply(&Edit::Move {
                track: Track::Narration,
                index: 1,
                to: ms(1_500),
            })
            .unwrap();
        timeline
    }

    #[test]
    fn trimming_narration_stops_at_its_neighbours_and_its_file() {
        let timeline = cut_narration();
        assert_eq!(
            timeline.narration()[1],
            AudioItem {
                file: "narration-1.mp3".into(),
                start: ms(2_000),
                at: ms(1_500),
                duration: ms(1_000),
                length: ms(4_000),
            }
        );
        // The start edge goes back to the first piece's end (0.5 s), not
        // the 2 s the file has before it.
        let after = round_trip(
            &timeline,
            Edit::Trim {
                track: Track::Narration,
                index: 1,
                edge: Edge::Start,
                by: Shift::earlier(ms(2_000)),
            },
        );
        assert_eq!(
            (after.narration()[1].at, after.narration()[1].start),
            (ms(1_000), ms(1_500))
        );
        // The end edge stops where the file does.
        let after = round_trip(
            &timeline,
            Edit::Trim {
                track: Track::Narration,
                index: 1,
                edge: Edge::End,
                by: Shift::later(ms(9_000)),
            },
        );
        assert_eq!(after.narration()[1].duration, ms(2_000));
        // The first piece's end stops at the second piece.
        let after = round_trip(
            &timeline,
            Edit::Trim {
                track: Track::Narration,
                index: 0,
                edge: Edge::End,
                by: Shift::later(ms(9_000)),
            },
        );
        assert_eq!(after.narration()[0].end(), ms(1_500));
        // The first piece cannot start before the file does.
        let mut start = timeline.clone();
        assert_eq!(
            start.apply(&Edit::Trim {
                track: Track::Narration,
                index: 0,
                edge: Edge::Start,
                by: Shift::earlier(ms(100)),
            }),
            Err(EditError::NoChange)
        );
    }

    #[test]
    fn moving_narration_stops_at_the_items_beside_it() {
        let timeline = cut_narration();
        let after = round_trip(
            &timeline,
            Edit::Move {
                track: Track::Narration,
                index: 1,
                to: ms(0),
            },
        );
        assert_eq!(
            after.narration()[1].at,
            ms(1_000),
            "against the first piece"
        );
        let after = round_trip(
            &timeline,
            Edit::Move {
                track: Track::Narration,
                index: 1,
                to: ms(60_000),
            },
        );
        assert_eq!(after.narration()[1].at, ms(60_000), "nothing after it");
        assert_eq!(after.duration(), ms(61_000));
        let after = round_trip(
            &timeline,
            Edit::Move {
                track: Track::Narration,
                index: 0,
                to: ms(900),
            },
        );
        assert_eq!(after.narration()[0].at, ms(500), "against the second piece");
        assert_unchanged(
            Edit::Move {
                track: Track::Video,
                index: 0,
                to: ms(500),
            },
            EditError::WrongTrack,
        );
        assert_unchanged(
            Edit::Move {
                track: Track::Narration,
                index: 0,
                to: ms(0),
            },
            EditError::NoChange,
        );
    }

    #[test]
    fn reordering_moves_a_clip_and_the_others_make_room() {
        let after = round_trip(&timeline(), Edit::Reorder { from: 0, to: 2 });
        let scenes: Vec<usize> = after.video().iter().map(|item| item.scene).collect();
        assert_eq!(scenes, vec![1, 2, 0]);
        assert_eq!(
            spans(&after, Track::Video),
            vec![
                (ms(0), ms(2_000)),
                (ms(2_000), ms(1_000)),
                (ms(3_000), ms(1_000))
            ]
        );
        let after = round_trip(&timeline(), Edit::Reorder { from: 2, to: 0 });
        let scenes: Vec<usize> = after.video().iter().map(|item| item.scene).collect();
        assert_eq!(scenes, vec![2, 0, 1]);
        assert_unchanged(Edit::Reorder { from: 1, to: 1 }, EditError::NoChange);
        assert_unchanged(Edit::Reorder { from: 0, to: 3 }, EditError::NoSuchItem);
    }

    #[test]
    fn deleting_a_clip_closes_the_gap_and_deleting_narration_leaves_one() {
        let after = round_trip(
            &timeline(),
            Edit::Delete {
                track: Track::Video,
                index: 1,
            },
        );
        assert_eq!(
            spans(&after, Track::Video),
            vec![(ms(0), ms(1_000)), (ms(1_000), ms(1_000))]
        );
        // The last clip, then all of them.
        let after = round_trip(
            &after,
            Edit::Delete {
                track: Track::Video,
                index: 1,
            },
        );
        let after = round_trip(
            &after,
            Edit::Delete {
                track: Track::Video,
                index: 0,
            },
        );
        assert!(after.video().is_empty());
        assert_eq!(after.duration(), ms(4_000), "the narration is still there");

        let narration = round_trip(
            &cut_narration(),
            Edit::Delete {
                track: Track::Narration,
                index: 0,
            },
        );
        assert_eq!(narration.narration().len(), 1);
        assert_eq!(narration.narration()[0].at, ms(1_500), "silence before it");
        assert_unchanged(
            Edit::Delete {
                track: Track::Narration,
                index: 1,
            },
            EditError::NoSuchItem,
        );
    }

    #[test]
    fn an_item_put_back_must_fit_where_it_goes() {
        let timeline = cut_narration();
        let mut piece = timeline.narration()[0].clone();
        piece.at = ms(1_000);
        let mut overlapping = timeline.clone();
        assert_eq!(
            overlapping.apply(&Edit::Insert {
                track: Track::Narration,
                index: 1,
                item: Item::Audio(piece.clone()),
            }),
            Err(EditError::Overlaps)
        );
        assert_eq!(overlapping, timeline);
        assert_unchanged(
            Edit::Insert {
                track: Track::Video,
                index: 0,
                item: Item::Audio(piece),
            },
            EditError::WrongTrack,
        );
    }

    #[test]
    fn history_undoes_and_redoes_in_order() {
        let original = timeline();
        let mut timeline = original.clone();
        let mut history = History::default();
        assert!(!history.can_undo());
        assert_eq!(history.undo(&mut timeline), Ok(false));

        history
            .apply(
                &mut timeline,
                &Edit::Split {
                    track: Track::Video,
                    index: 0,
                    at: ms(500),
                },
            )
            .unwrap();
        let split = timeline.clone();
        history
            .apply(
                &mut timeline,
                &Edit::Delete {
                    track: Track::Video,
                    index: 0,
                },
            )
            .unwrap();
        let deleted = timeline.clone();

        assert_eq!(history.undo(&mut timeline), Ok(true));
        assert_eq!(timeline, split);
        assert_eq!(history.undo(&mut timeline), Ok(true));
        assert_eq!(timeline, original);
        assert!(!history.can_undo());
        assert_eq!(history.redo(&mut timeline), Ok(true));
        assert_eq!(history.redo(&mut timeline), Ok(true));
        assert_eq!(timeline, deleted);
        assert_eq!(history.redo(&mut timeline), Ok(false));

        // A new edit after undoing forgets what was undone.
        history.undo(&mut timeline).unwrap();
        history
            .apply(&mut timeline, &Edit::Reorder { from: 0, to: 1 })
            .unwrap();
        assert!(!history.can_redo());
        // A refused edit is not remembered.
        assert!(
            history
                .apply(&mut timeline, &Edit::Reorder { from: 0, to: 0 })
                .is_err()
        );
        history.undo(&mut timeline).unwrap();
        assert_eq!(timeline, split);
    }

    #[test]
    fn history_keeps_its_last_edits() {
        let mut timeline = timeline();
        let mut history = History::default();
        for _ in 0..HISTORY_DEPTH + 5 {
            history
                .apply(&mut timeline, &Edit::Reorder { from: 0, to: 1 })
                .unwrap();
        }
        let mut undone = 0;
        while history.undo(&mut timeline).unwrap() {
            undone += 1;
        }
        assert_eq!(undone, HISTORY_DEPTH);
    }

    #[test]
    fn history_made_on_another_timeline_is_forgotten() {
        let mut timeline = timeline();
        let mut history = History::default();
        history
            .apply(
                &mut timeline,
                &Edit::Delete {
                    track: Track::Video,
                    index: 2,
                },
            )
            .unwrap();
        let mut other = Timeline::rough_cut(&plan(vec![scene(0, 1_000)]), &narration(1_000));
        assert!(history.undo(&mut other).is_err());
        assert!(!history.can_undo() && !history.can_redo());
    }
}
