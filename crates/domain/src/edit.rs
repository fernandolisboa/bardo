//! Cut editing (PRD stories 58, 59, 70): split, trim, move, reorder and
//! delete items of a timeline, each an [`Edit`] that, applied, hands back
//! the edit undoing it. A [`History`] keeps those to undo and redo. Mix
//! changes (stories 61-63: a lane's level, mute and solo, the ducking, an
//! audio item's fades) are edits too, so they undo the same way, and so are
//! caption changes (stories 65, 66: a caption's text and ends, deleting
//! one, showing them or not, their style), and so is framing (story 67:
//! the frame's shape, each clip's crop), and so are video transitions
//! (issue #167: the transition into a clip, or into every clip at once),
//! and so is motion on stills (issue #180: one clip's, or every still's).
//!
//! Edits are exact: they cut where they are told, and the caller snaps the
//! time first (to a frame, and to a word when snapping is on). Trims and
//! moves stop where the item runs out: at one frame long, at the start or
//! end of its file, at the item beside it on an audio track. A video clip
//! is the one exception at its end: played past it, it holds its last
//! frame, as the rough cut does with a clip shorter than its scene.
//!
//! The three audio tracks edit alike: narration, music and SFX items split,
//! trim, move, fade and go the same way. Placing an imported asset on a
//! track is an insert ([`Timeline::place`]), undone by removing it.
//!
//! Captions live in narration-file time (`crate::Captions`): trimming one
//! moves its end in the file by the shift asked, and it stops at the
//! captions beside it, at one frame long and at the narration's ends.

use std::time::Duration;

use crate::{
    AspectRatio, AudioItem, AudioLane, Caption, CaptionHighlight, CaptionStyle, DUCK_RANGE,
    Ducking, Framing, GAIN_RANGE, LaneMix, MediaAsset, Motion, MotionDefault, Timeline, Transition,
    VideoItem, VideoSource, caption_text, min_length,
};

/// A track of the timeline that edits reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Track {
    /// V1: the magnetic video track.
    Video,
    /// A1: the narration.
    Narration,
    /// A2: the music.
    Music,
    /// A3: the sound effects.
    Sfx,
    /// CC: the captions, by index in `Captions::lines`.
    Captions,
}

impl Track {
    /// The audio lane it is, for an audio track.
    pub fn lane(self) -> Option<AudioLane> {
        match self {
            Track::Narration => Some(AudioLane::Narration),
            Track::Music => Some(AudioLane::Music),
            Track::Sfx => Some(AudioLane::Sfx),
            Track::Video | Track::Captions => None,
        }
    }

    /// The track of an audio lane.
    pub fn of_lane(lane: AudioLane) -> Track {
        match lane {
            AudioLane::Narration => Track::Narration,
            AudioLane::Music => Track::Music,
            AudioLane::Sfx => Track::Sfx,
        }
    }

    pub fn is_audio(self) -> bool {
        self.lane().is_some()
    }
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

    pub fn caption(index: usize) -> Self {
        Self {
            track: Track::Captions,
            index,
        }
    }

    /// Item `index` of an audio lane's track.
    pub fn audio(lane: AudioLane, index: usize) -> Self {
        Self {
            track: Track::of_lane(lane),
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

    /// How far, in seconds: negative when earlier.
    pub fn seconds(self) -> f64 {
        self.0 as f64 / 1e9
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
    Caption(Caption),
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
    Join {
        track: Track,
        index: usize,
    },
    /// Moves one edge of the item. On the video track the items after it
    /// follow (the track is magnetic); moving the start edge of a video
    /// item cuts into or out of its source while the item stays put.
    Trim {
        track: Track,
        index: usize,
        edge: Edge,
        by: Shift,
    },
    /// Moves an audio item to start at `to`, short of the items beside it
    /// on its track.
    Move {
        track: Track,
        index: usize,
        to: Duration,
    },
    /// Takes the video item at `from` to position `to`.
    Reorder {
        from: usize,
        to: usize,
    },
    /// Removes the item; on the video track the ones after it close up.
    Delete {
        track: Track,
        index: usize,
    },
    /// Puts an item at `index`: the undo of `Delete`, and how media is
    /// placed. An audio item keeps its time and must fit there.
    Insert {
        track: Track,
        index: usize,
        item: Item,
    },
    /// Sets an audio lane's level (kept within [`GAIN_RANGE`]), mute and
    /// solo.
    SetLane {
        lane: AudioLane,
        mix: LaneMix,
    },
    /// Turns the music's ducking on or off and sets its depth (kept within
    /// [`DUCK_RANGE`]).
    SetDucking(Ducking),
    /// Sets an audio item's fades. They are kept as given, so undoing gives
    /// back exactly what was there; an item shorter than its fades plays
    /// them cut short (`AudioItem::fades`).
    SetFades {
        track: Track,
        index: usize,
        fade_in: Duration,
        fade_out: Duration,
    },
    /// Sets a caption's text, kept as `crate::caption_text` gives it.
    SetCaptionText {
        index: usize,
        text: String,
    },
    /// Burns the captions in, or not.
    ShowCaptions(bool),
    SetCaptionStyle(CaptionStyle),
    /// Lights the spoken word in a colour, or not.
    SetCaptionHighlight(CaptionHighlight),
    /// Cuts for a frame of another shape.
    SetAspect(AspectRatio),
    /// Sets how a video item's picture fills the frame.
    SetFraming {
        index: usize,
        framing: Framing,
    },
    /// Sets the transition into a video item from the one before (`None`
    /// for a cut). It is kept as given; items shorter than it play it cut
    /// short (`Timeline::transition_into`).
    SetTransition {
        index: usize,
        transition: Option<Transition>,
    },
    /// Sets the transition into every video item, in order: how a
    /// transition goes on every cut at once, and its undo.
    SetTransitions(Vec<Option<Transition>>),
    /// Sets how the view moves over a video item's picture (`None`: it
    /// stays still).
    SetMotion {
        index: usize,
        motion: Option<Motion>,
    },
    /// Sets the motion of every video item, in order: how one motion, or a
    /// varied pattern, goes on every still at once, and its undo.
    SetMotions(Vec<Option<Motion>>),
    /// Puts `items` in place of the `count` video items from `index`: how
    /// a stretch of the video track is covered at once (a performed clip
    /// put on the cut, a scene picture over part of it, #193), and its
    /// undo.
    Replace {
        index: usize,
        count: usize,
        items: Vec<VideoItem>,
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
    /// Moving is for audio items, reordering for video ones; media goes
    /// on the tracks of its kind.
    #[error("the edit does not apply to that track")]
    WrongTrack,
    #[error("the item overlaps another")]
    Overlaps,
    /// Media shorter than a frame has nothing to place.
    #[error("the media is shorter than a frame")]
    TooShort,
    #[error("the items are not two halves of one cut")]
    CannotJoin,
    /// The item is already there, or cannot go further that way.
    #[error("the edit changes nothing")]
    NoChange,
    /// A caption needs some text, and not too much.
    #[error("the caption text is empty or too long")]
    InvalidText,
    /// A performed clip goes where the cut plays its narration, and the
    /// cut has cut that moment away.
    #[error("the cut does not play that narration")]
    NotHeard,
}

impl Timeline {
    fn len_of(&self, track: Track) -> usize {
        match track.lane() {
            Some(lane) => self.audio(lane).len(),
            None if track == Track::Video => self.video.len(),
            None => self.captions.lines.len(),
        }
    }

    /// The items of an audio track; `None` for the others.
    fn audio_track(&self, track: Track) -> Option<&[AudioItem]> {
        Some(self.audio(track.lane()?))
    }

    fn audio_track_mut(&mut self, track: Track) -> Option<&mut Vec<AudioItem>> {
        match track {
            Track::Narration => Some(&mut self.narration),
            Track::Music => Some(&mut self.music),
            Track::Sfx => Some(&mut self.sfx),
            Track::Video | Track::Captions => None,
        }
    }

    /// The audio items of `track`, or `WrongTrack`.
    fn audio_items(&self, track: Track) -> Result<&[AudioItem], EditError> {
        self.audio_track(track).ok_or(EditError::WrongTrack)
    }

    fn audio_items_mut(&mut self, track: Track) -> Result<&mut Vec<AudioItem>, EditError> {
        self.audio_track_mut(track).ok_or(EditError::WrongTrack)
    }

    fn check(&self, track: Track, index: usize) -> Result<(), EditError> {
        if index < self.len_of(track) {
            Ok(())
        } else {
            Err(EditError::NoSuchItem)
        }
    }

    /// Where on the timeline the item starts and how long it is; for a
    /// caption, from its first stretch shown to its last, and `None` when
    /// the cut shows none of it.
    pub fn span(&self, item: ItemRef) -> Option<(Duration, Duration)> {
        match item.track {
            Track::Video => self.video.get(item.index).map(|v| (v.at, v.duration)),
            Track::Captions => self.caption_hull(item.index),
            audio => self
                .audio_track(audio)?
                .get(item.index)
                .map(|a| (a.at, a.duration)),
        }
    }

    /// The end of the item before `index` on an audio track, or zero.
    fn audio_floor(items: &[AudioItem], index: usize) -> Duration {
        index
            .checked_sub(1)
            .map_or(Duration::ZERO, |before| items[before].end())
    }

    /// How far each edge of an item can go: the shortest and longest shift
    /// a trim of `edge` makes.
    pub fn trim_limits(&self, item: ItemRef, edge: Edge) -> Result<(Shift, Shift), EditError> {
        self.check(item.track, item.index)?;
        // A caption's ends live in the narration file, cut away or not.
        if item.track == Track::Captions {
            return Ok(self.caption_trim_limits(item.index, edge));
        }
        let (_, duration) = self.span(item).ok_or(EditError::NoSuchItem)?;
        // Each edge stops a frame short of the other.
        let shrink = Shift::later(duration.saturating_sub(min_length()));
        Ok(match (item.track, edge) {
            (Track::Video, Edge::Start) => {
                let video = &self.video[item.index];
                match &video.source {
                    // A clip's start stays a frame short of its end.
                    VideoSource::Clip { length, .. } => {
                        let last = length.saturating_sub(min_length());
                        let room = Shift::between(video.start, last.max(video.start));
                        (Shift::earlier(video.start), shrink.min(room))
                    }
                    _ => (-Shift::FAR, shrink),
                }
            }
            (Track::Video, Edge::End) => (-shrink, Shift::FAR),
            (Track::Captions, _) => unreachable!("captions return above"),
            (track, Edge::Start) => {
                let items = self.audio_items(track)?;
                let audio = &items[item.index];
                let room = audio.at - Self::audio_floor(items, item.index);
                (Shift::earlier(audio.start.min(room)), shrink)
            }
            (track, Edge::End) => {
                let items = self.audio_items(track)?;
                let audio = &items[item.index];
                let in_file = audio.length.saturating_sub(audio.start + audio.duration);
                let room = items
                    .get(item.index + 1)
                    .map_or(in_file, |next| in_file.min(next.at - audio.end()));
                (-shrink, Shift::later(room))
            }
        })
    }

    /// How far a caption's edge can go, in the narration file: to the
    /// caption beside it or the file's end, and a frame short of its other
    /// edge.
    fn caption_trim_limits(&self, index: usize, edge: Edge) -> (Shift, Shift) {
        let lines = &self.captions.lines;
        let caption = &lines[index];
        let shrink = Shift::later((caption.end - caption.start).saturating_sub(min_length()));
        match edge {
            Edge::Start => {
                let floor = index
                    .checked_sub(1)
                    .map_or(Duration::ZERO, |before| lines[before].end);
                (Shift::earlier(caption.start.saturating_sub(floor)), shrink)
            }
            Edge::End => {
                let ceiling = lines
                    .get(index + 1)
                    .map_or(self.captions.length, |next| next.start);
                (-shrink, Shift::later(ceiling.saturating_sub(caption.end)))
            }
        }
    }

    /// The earliest and latest start an audio item can move to; the latest
    /// is `None` when nothing comes after it.
    pub fn move_limits(&self, item: ItemRef) -> Result<(Duration, Option<Duration>), EditError> {
        let items = self.audio_items(item.track)?;
        let audio = items.get(item.index).ok_or(EditError::NoSuchItem)?;
        let latest = items
            .get(item.index + 1)
            .map(|next| next.at - audio.duration);
        Ok((Self::audio_floor(items, item.index), latest))
    }

    /// The edit that places `asset` on `track` at `at` (a frame boundary),
    /// and where the new item goes: footage goes whole into the video
    /// track at the cut nearest `at`; audio starts at `at`, or where the
    /// item playing there ends, and plays whole or until the next item on
    /// its track. Refused for a track of another kind (`WrongTrack`), for
    /// media shorter than a frame (`TooShort`) or where not a frame of it
    /// fits (`Overlaps`).
    pub fn place(
        &self,
        asset: &MediaAsset,
        track: Track,
        at: Duration,
    ) -> Result<(Edit, ItemRef), EditError> {
        if !asset.fits(track) {
            return Err(EditError::WrongTrack);
        }
        if track == Track::Video {
            let item = VideoItem::footage(asset).ok_or(EditError::TooShort)?;
            // Before the clip whose middle is past `at`.
            let index = self
                .video
                .iter()
                .position(|clip| at < clip.at + clip.duration / 2)
                .unwrap_or(self.video.len());
            let edit = Edit::Insert {
                track,
                index,
                item: Item::Video(item),
            };
            return Ok((edit, ItemRef { track, index }));
        }
        if asset.duration < min_length() {
            return Err(EditError::TooShort);
        }
        let items = self.audio_items(track)?;
        let at = items
            .iter()
            .find(|item| item.at <= at && at < item.end())
            .map_or(at, AudioItem::end);
        let index = items.partition_point(|item| item.at < at);
        let room = items
            .get(index)
            .map_or(asset.duration, |next| next.at.saturating_sub(at));
        let duration = asset.duration.min(room);
        if duration < min_length() {
            return Err(EditError::Overlaps);
        }
        let edit = Edit::Insert {
            track,
            index,
            item: Item::Audio(AudioItem {
                file: asset.file.clone(),
                start: Duration::ZERO,
                at,
                duration,
                length: asset.duration,
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
            }),
        };
        Ok((edit, ItemRef { track, index }))
    }

    /// The cut with the generated track `asset` on the music track
    /// (#169): the music items playing `previous`, the track it takes
    /// over from, come out first, while music the user placed stays. It
    /// goes in from the start, or where the music playing there ends, as
    /// `place` puts it. A cut whose music track already plays `asset` is
    /// left as it is.
    pub fn use_music(
        &self,
        previous: Option<&str>,
        asset: &MediaAsset,
    ) -> Result<Timeline, EditError> {
        let mut timeline = self.clone();
        if let Some(previous) = previous.filter(|previous| *previous != asset.file) {
            timeline.music.retain(|item| item.file != previous);
        }
        if timeline.music.iter().any(|item| item.file == asset.file) {
            return Ok(timeline);
        }
        let (edit, _) = timeline.place(asset, Track::Music, Duration::ZERO)?;
        timeline.apply(&edit)?;
        Ok(timeline)
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
            Edit::SetLane { lane, mix } => self.set_lane(*lane, *mix)?,
            Edit::SetDucking(ducking) => self.set_ducking(*ducking)?,
            Edit::SetFades {
                track,
                index,
                fade_in,
                fade_out,
            } => self.set_fades(*track, *index, *fade_in, *fade_out)?,
            Edit::SetCaptionText { index, text } => self.set_caption_text(*index, text)?,
            Edit::ShowCaptions(shown) => self.show_captions(*shown)?,
            Edit::SetCaptionStyle(style) => self.set_caption_style(*style)?,
            Edit::SetCaptionHighlight(highlight) => self.set_caption_highlight(*highlight)?,
            Edit::SetAspect(aspect) => self.set_aspect(*aspect)?,
            Edit::SetFraming { index, framing } => self.set_framing(*index, *framing)?,
            Edit::SetTransition { index, transition } => {
                self.set_transition(*index, *transition)?
            }
            Edit::SetTransitions(transitions) => self.set_transitions(transitions)?,
            Edit::SetMotion { index, motion } => self.set_motion(*index, *motion)?,
            Edit::SetMotions(motions) => self.set_motions(motions)?,
            Edit::Replace {
                index,
                count,
                items,
            } => self.replace(*index, *count, items)?,
        };
        self.relayout();
        Ok(undo)
    }

    fn split(&mut self, track: Track, index: usize, at: Duration) -> Result<Edit, EditError> {
        if track == Track::Captions {
            return Err(EditError::WrongTrack);
        }
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
                // The cut between the halves is seamless: no transition.
                let mut second = self.video[index].clone();
                second.transition = None;
                second.start += first;
                second.at = at;
                second.duration = end - at;
                self.video[index].duration = first;
                self.video.insert(index + 1, second);
            }
            Track::Captions => return Err(EditError::WrongTrack),
            audio => {
                // The fade-in stays with the first part, the fade-out goes
                // with the second; a split inside a fade ends it there.
                let items = self.audio_items_mut(audio)?;
                let mut second = items[index].clone();
                second.start += first;
                second.at = at;
                second.duration = end - at;
                second.fade_in = Duration::ZERO;
                let first_part = &mut items[index];
                first_part.duration = first;
                first_part.fade_out = Duration::ZERO;
                items.insert(index + 1, second);
            }
        }
        Ok(Edit::Join { track, index })
    }

    fn join(&mut self, track: Track, index: usize) -> Result<Edit, EditError> {
        if track == Track::Captions {
            return Err(EditError::WrongTrack);
        }
        self.check(track, index)?;
        self.check(track, index + 1)
            .map_err(|_| EditError::CannotJoin)?;
        let at = match track {
            Track::Video => {
                let (first, second) = (&self.video[index], &self.video[index + 1]);
                // A framing or motion set on one half alone, or a
                // transition between them, would be lost.
                if first.picture != second.picture
                    || first.source != second.source
                    || first.framing != second.framing
                    || first.motion != second.motion
                    || second.transition.is_some()
                    || second.start != first.start + first.duration
                {
                    return Err(EditError::CannotJoin);
                }
                let second = self.video.remove(index + 1);
                self.video[index].duration += second.duration;
                second.at
            }
            Track::Captions => return Err(EditError::WrongTrack),
            audio => {
                let items = self.audio_items_mut(audio)?;
                let (first, second) = (&items[index], &items[index + 1]);
                // Fades where they meet would be lost.
                if first.file != second.file
                    || first.end() != second.at
                    || second.start != first.start + first.duration
                    || !first.fade_out.is_zero()
                    || !second.fade_in.is_zero()
                {
                    return Err(EditError::CannotJoin);
                }
                let second = items.remove(index + 1);
                let joined = &mut items[index];
                joined.duration += second.duration;
                joined.fade_out = second.fade_out;
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
            (Track::Captions, Edge::Start) => {
                let caption = &mut self.captions.lines[index];
                caption.start = moved(caption.start)?;
            }
            (Track::Captions, Edge::End) => {
                let caption = &mut self.captions.lines[index];
                caption.end = moved(caption.end)?;
            }
            (audio, Edge::Start) => {
                let item = &mut self.audio_items_mut(audio)?[index];
                let (at, start, duration) =
                    (moved(item.at)?, moved(item.start)?, shrunk(item.duration)?);
                (item.at, item.start, item.duration) = (at, start, duration);
            }
            (audio, Edge::End) => {
                let item = &mut self.audio_items_mut(audio)?[index];
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
        let item = &mut self.audio_items_mut(track)?[index];
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
            Track::Captions => Item::Caption(self.captions.lines.remove(index)),
            audio => Item::Audio(self.audio_items_mut(audio)?.remove(index)),
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
            (track, Item::Audio(audio)) if track.is_audio() => {
                let items = self.audio_items_mut(track)?;
                let after_previous = Self::audio_floor(items, index) <= audio.at;
                let before_next = items.get(index).is_none_or(|next| audio.end() <= next.at);
                if !after_previous || !before_next {
                    return Err(EditError::Overlaps);
                }
                if audio.duration < min_length() || audio.start + audio.duration > audio.length {
                    return Err(EditError::OutsideItem);
                }
                items.insert(index, audio.clone());
            }
            (Track::Captions, Item::Caption(caption)) => {
                let lines = &self.captions.lines;
                let after_previous = index
                    .checked_sub(1)
                    .is_none_or(|before| lines[before].end <= caption.start);
                let before_next = lines
                    .get(index)
                    .is_none_or(|next| caption.end <= next.start);
                if !after_previous || !before_next {
                    return Err(EditError::Overlaps);
                }
                if caption.start + min_length() > caption.end || caption.end > self.captions.length
                {
                    return Err(EditError::OutsideItem);
                }
                if caption_text(&caption.text).as_deref() != Some(caption.text.as_str()) {
                    return Err(EditError::InvalidText);
                }
                self.captions.lines.insert(index, caption.clone());
            }
            _ => return Err(EditError::WrongTrack),
        }
        Ok(Edit::Delete { track, index })
    }

    fn replace(
        &mut self,
        index: usize,
        count: usize,
        items: &[VideoItem],
    ) -> Result<Edit, EditError> {
        if index + count > self.video.len() {
            return Err(EditError::NoSuchItem);
        }
        if items.iter().any(|item| item.duration < min_length()) {
            return Err(EditError::OutsideItem);
        }
        if count == 0 && items.is_empty() {
            return Err(EditError::NoChange);
        }
        let removed: Vec<VideoItem> = self
            .video
            .splice(index..index + count, items.iter().cloned())
            .collect();
        Ok(Edit::Replace {
            index,
            count: items.len(),
            items: removed,
        })
    }

    fn set_lane(&mut self, lane: AudioLane, mix: LaneMix) -> Result<Edit, EditError> {
        let mix = LaneMix {
            gain: mix.gain.clamp(GAIN_RANGE.0, GAIN_RANGE.1),
            ..mix
        };
        let current = self.mix.lane_mut(lane);
        if *current == mix {
            return Err(EditError::NoChange);
        }
        let before = std::mem::replace(current, mix);
        Ok(Edit::SetLane { lane, mix: before })
    }

    fn set_ducking(&mut self, ducking: Ducking) -> Result<Edit, EditError> {
        let ducking = Ducking {
            depth: ducking.depth.clamp(DUCK_RANGE.0, DUCK_RANGE.1),
            ..ducking
        };
        if self.mix.ducking == ducking {
            return Err(EditError::NoChange);
        }
        let before = std::mem::replace(&mut self.mix.ducking, ducking);
        Ok(Edit::SetDucking(before))
    }

    fn set_fades(
        &mut self,
        track: Track,
        index: usize,
        fade_in: Duration,
        fade_out: Duration,
    ) -> Result<Edit, EditError> {
        let item = self
            .audio_items_mut(track)?
            .get_mut(index)
            .ok_or(EditError::NoSuchItem)?;
        if (item.fade_in, item.fade_out) == (fade_in, fade_out) {
            return Err(EditError::NoChange);
        }
        let before = (
            std::mem::replace(&mut item.fade_in, fade_in),
            std::mem::replace(&mut item.fade_out, fade_out),
        );
        Ok(Edit::SetFades {
            track,
            index,
            fade_in: before.0,
            fade_out: before.1,
        })
    }
}

impl Timeline {
    fn set_caption_text(&mut self, index: usize, text: &str) -> Result<Edit, EditError> {
        self.check(Track::Captions, index)?;
        let text = caption_text(text).ok_or(EditError::InvalidText)?;
        let caption = &mut self.captions.lines[index];
        if caption.text == text {
            return Err(EditError::NoChange);
        }
        let before = std::mem::replace(&mut caption.text, text);
        Ok(Edit::SetCaptionText {
            index,
            text: before,
        })
    }

    fn show_captions(&mut self, shown: bool) -> Result<Edit, EditError> {
        if self.captions.shown == shown {
            return Err(EditError::NoChange);
        }
        self.captions.shown = shown;
        Ok(Edit::ShowCaptions(!shown))
    }

    fn set_caption_style(&mut self, style: CaptionStyle) -> Result<Edit, EditError> {
        if self.captions.style == style {
            return Err(EditError::NoChange);
        }
        let before = std::mem::replace(&mut self.captions.style, style);
        Ok(Edit::SetCaptionStyle(before))
    }

    fn set_caption_highlight(&mut self, highlight: CaptionHighlight) -> Result<Edit, EditError> {
        if self.captions.highlight == highlight {
            return Err(EditError::NoChange);
        }
        let before = std::mem::replace(&mut self.captions.highlight, highlight);
        Ok(Edit::SetCaptionHighlight(before))
    }

    fn set_aspect(&mut self, aspect: AspectRatio) -> Result<Edit, EditError> {
        if self.aspect == aspect {
            return Err(EditError::NoChange);
        }
        let before = std::mem::replace(&mut self.aspect, aspect);
        Ok(Edit::SetAspect(before))
    }

    fn set_framing(&mut self, index: usize, framing: Framing) -> Result<Edit, EditError> {
        let item = self.video.get_mut(index).ok_or(EditError::NoSuchItem)?;
        if item.framing == framing {
            return Err(EditError::NoChange);
        }
        let before = std::mem::replace(&mut item.framing, framing);
        Ok(Edit::SetFraming {
            index,
            framing: before,
        })
    }

    fn set_transition(
        &mut self,
        index: usize,
        transition: Option<Transition>,
    ) -> Result<Edit, EditError> {
        let item = self.video.get_mut(index).ok_or(EditError::NoSuchItem)?;
        if item.transition == transition {
            return Err(EditError::NoChange);
        }
        let before = std::mem::replace(&mut item.transition, transition);
        Ok(Edit::SetTransition {
            index,
            transition: before,
        })
    }

    fn set_transitions(&mut self, transitions: &[Option<Transition>]) -> Result<Edit, EditError> {
        if transitions.len() != self.video.len() {
            return Err(EditError::NoSuchItem);
        }
        let before: Vec<Option<Transition>> =
            self.video.iter().map(|item| item.transition).collect();
        if before == transitions {
            return Err(EditError::NoChange);
        }
        for (item, transition) in self.video.iter_mut().zip(transitions) {
            item.transition = *transition;
        }
        Ok(Edit::SetTransitions(before))
    }

    fn set_motion(&mut self, index: usize, motion: Option<Motion>) -> Result<Edit, EditError> {
        let item = self.video.get_mut(index).ok_or(EditError::NoSuchItem)?;
        if item.motion == motion {
            return Err(EditError::NoChange);
        }
        let before = std::mem::replace(&mut item.motion, motion);
        Ok(Edit::SetMotion {
            index,
            motion: before,
        })
    }

    fn set_motions(&mut self, motions: &[Option<Motion>]) -> Result<Edit, EditError> {
        if motions.len() != self.video.len() {
            return Err(EditError::NoSuchItem);
        }
        let before: Vec<Option<Motion>> = self.video.iter().map(|item| item.motion).collect();
        if before == motions {
            return Err(EditError::NoChange);
        }
        for (item, motion) in self.video.iter_mut().zip(motions) {
            item.motion = *motion;
        }
        Ok(Edit::SetMotions(before))
    }

    /// The edit that puts `motion` (`None`: none) on every scene of the
    /// video track, leaving footage and black as they are. Scenes showing a
    /// clip or no picture yet get it too, unplayed, so a scene that shows
    /// its still again moves as the others do.
    pub fn motion_everywhere(&self, motion: Option<Motion>) -> Edit {
        self.motions_on_scenes(|_| motion)
    }

    /// The edit that gives the scenes of the video track the motions of
    /// `default`'s pattern, in turn: the n-th scene the pattern's n-th
    /// motion, so no still moves the way the one before it did.
    pub fn motion_pattern(&self, default: MotionDefault) -> Edit {
        self.motions_on_scenes(|position| default.at(position))
    }

    /// Every item's motion, the scenes' from `motion` by their place among
    /// the scenes.
    fn motions_on_scenes(&self, motion: impl Fn(usize) -> Option<Motion>) -> Edit {
        let mut scenes = 0;
        Edit::SetMotions(
            self.video
                .iter()
                .map(|item| {
                    if item.scene().is_none() {
                        return item.motion;
                    }
                    scenes += 1;
                    motion(scenes - 1)
                })
                .collect(),
        )
    }

    /// The edit that puts `transition` (`None`: a plain cut) on every cut
    /// between video items, leaving the first item's as it is.
    pub fn transition_everywhere(&self, transition: Option<Transition>) -> Edit {
        Edit::SetTransitions(
            self.video
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    if index == 0 {
                        item.transition
                    } else {
                        transition
                    }
                })
                .collect(),
        )
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
    use crate::timeline::tests::{asset, image, ms, narration, plan, scene};
    use crate::{CropPosition, MediaKind, SceneClip, frame_at, frame_time};

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
        Timeline::rough_cut(
            &plan(vec![first, second, third]),
            &narration(4_000),
            AspectRatio::Landscape,
        )
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
        // Shown the same either way, but an animation added later plays on
        // from the first piece.
        assert_eq!(video[1].start, ms(400));
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
    fn a_clip_start_trim_stops_a_frame_short_of_the_file_end() {
        // The clip (5 s file) plays 2 s; stretch it to 8 s, then cut its
        // start as late as it goes.
        let mut timeline = timeline();
        let clip = ItemRef::video(1);
        timeline
            .apply(&Edit::Trim {
                track: Track::Video,
                index: 1,
                edge: Edge::End,
                by: Shift::later(ms(6_000)),
            })
            .unwrap();
        let (_, latest) = timeline.trim_limits(clip, Edge::Start).unwrap();
        assert_eq!(
            latest,
            Shift::between(Duration::ZERO, ms(5_000) - frame_time(1))
        );
        let after = round_trip(
            &timeline,
            Edit::Trim {
                track: Track::Video,
                index: 1,
                edge: Edge::Start,
                by: Shift::FAR,
            },
        );
        assert_eq!(after.video()[1].start, ms(5_000) - frame_time(1));
        assert_eq!(
            after.video()[1].duration,
            ms(8_000) - after.video()[1].start
        );
    }

    #[test]
    fn a_piece_cut_past_the_clip_end_holds_its_last_frame() {
        let mut timeline = timeline();
        timeline
            .apply(&Edit::Trim {
                track: Track::Video,
                index: 1,
                edge: Edge::End,
                by: Shift::later(ms(6_000)),
            })
            .unwrap();
        let after = round_trip(
            &timeline,
            Edit::Split {
                track: Track::Video,
                index: 1,
                at: ms(8_000),
            },
        );
        let piece = &after.video()[2];
        assert_eq!(piece.start, ms(7_000));
        assert_eq!(piece.source_start(), ms(5_000) - frame_time(1));
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
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
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
        let scenes: Vec<usize> = after.video().iter().filter_map(VideoItem::scene).collect();
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
        let scenes: Vec<usize> = after.video().iter().filter_map(VideoItem::scene).collect();
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
        let mut other = Timeline::rough_cut(
            &plan(vec![scene(0, 1_000)]),
            &narration(1_000),
            AspectRatio::Landscape,
        );
        assert!(history.undo(&mut other).is_err());
        assert!(!history.can_undo() && !history.can_redo());
    }

    #[test]
    fn lane_levels_mutes_and_solos_undo() {
        let music = LaneMix {
            gain: crate::Decibels::from_tenths(-60),
            muted: false,
            solo: true,
        };
        let after = round_trip(
            &timeline(),
            Edit::SetLane {
                lane: AudioLane::Music,
                mix: music,
            },
        );
        assert_eq!(after.mix().lane(AudioLane::Music), music);
        assert!(!after.mix().is_audible(AudioLane::Narration), "soloed away");

        // Past the range it stops at the loudest.
        let after = round_trip(
            &timeline(),
            Edit::SetLane {
                lane: AudioLane::Sfx,
                mix: LaneMix {
                    gain: crate::Decibels::from_tenths(400),
                    ..LaneMix::default()
                },
            },
        );
        assert_eq!(after.mix().lane(AudioLane::Sfx).gain, GAIN_RANGE.1);
        assert_unchanged(
            Edit::SetLane {
                lane: AudioLane::Narration,
                mix: LaneMix::default(),
            },
            EditError::NoChange,
        );
    }

    #[test]
    fn ducking_turns_off_and_changes_depth_within_its_range() {
        let after = round_trip(
            &timeline(),
            Edit::SetDucking(Ducking {
                on: false,
                depth: crate::DEFAULT_DUCK,
            }),
        );
        assert!(!after.mix().ducking.on);
        let after = round_trip(
            &timeline(),
            Edit::SetDucking(Ducking {
                on: true,
                depth: crate::Decibels::from_tenths(900),
            }),
        );
        assert_eq!(after.mix().ducking.depth, DUCK_RANGE.1);
        assert_unchanged(Edit::SetDucking(Ducking::default()), EditError::NoChange);
    }

    #[test]
    fn fades_set_on_narration_undo_exactly() {
        let after = round_trip(
            &timeline(),
            Edit::SetFades {
                track: Track::Narration,
                index: 0,
                fade_in: ms(300),
                fade_out: ms(1_200),
            },
        );
        assert_eq!(after.narration()[0].fades(), (ms(300), ms(1_200)));

        // Fades longer than a piece cut short later come back as they were.
        let mut short = after.clone();
        short
            .apply(&Edit::Trim {
                track: Track::Narration,
                index: 0,
                edge: Edge::End,
                by: Shift::earlier(ms(3_600)),
            })
            .unwrap();
        assert_eq!(short.narration()[0].fades(), (ms(300), ms(100)));
        let refaded = round_trip(
            &short,
            Edit::SetFades {
                track: Track::Narration,
                index: 0,
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
            },
        );
        assert_eq!(
            refaded.narration()[0].fades(),
            (Duration::ZERO, Duration::ZERO)
        );

        assert_unchanged(
            Edit::SetFades {
                track: Track::Video,
                index: 0,
                fade_in: ms(100),
                fade_out: ms(100),
            },
            EditError::WrongTrack,
        );
        assert_unchanged(
            Edit::SetFades {
                track: Track::Narration,
                index: 0,
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
            },
            EditError::NoChange,
        );
    }

    #[test]
    fn a_split_gives_the_fade_in_to_the_first_part_and_the_fade_out_to_the_second() {
        let mut faded = timeline();
        faded
            .apply(&Edit::SetFades {
                track: Track::Narration,
                index: 0,
                fade_in: ms(500),
                fade_out: ms(700),
            })
            .unwrap();
        let after = round_trip(
            &faded,
            Edit::Split {
                track: Track::Narration,
                index: 0,
                at: ms(2_000),
            },
        );
        let pieces = after.narration();
        assert_eq!(pieces[0].fades(), (ms(500), Duration::ZERO));
        assert_eq!(pieces[1].fades(), (Duration::ZERO, ms(700)));

        // A split inside a fade ends the fade at the split.
        let inside = round_trip(
            &faded,
            Edit::Split {
                track: Track::Narration,
                index: 0,
                at: ms(300),
            },
        );
        assert_eq!(inside.narration()[0].fades(), (ms(300), Duration::ZERO));
        assert_eq!(inside.narration()[1].fades(), (Duration::ZERO, ms(700)));

        // A fade added where the halves meet keeps them apart.
        let mut faded_inside = after.clone();
        faded_inside
            .apply(&Edit::SetFades {
                track: Track::Narration,
                index: 1,
                fade_in: ms(100),
                fade_out: ms(700),
            })
            .unwrap();
        assert_eq!(
            faded_inside.apply(&Edit::Join {
                track: Track::Narration,
                index: 0,
            }),
            Err(EditError::CannotJoin)
        );
    }

    #[test]
    fn history_undoes_mix_edits_with_the_cuts() {
        let original = timeline();
        let mut timeline = original.clone();
        let mut history = History::default();
        history
            .apply(
                &mut timeline,
                &Edit::SetLane {
                    lane: AudioLane::Narration,
                    mix: LaneMix {
                        muted: true,
                        ..LaneMix::default()
                    },
                },
            )
            .unwrap();
        history
            .apply(
                &mut timeline,
                &Edit::Split {
                    track: Track::Narration,
                    index: 0,
                    at: ms(1_000),
                },
            )
            .unwrap();
        history.undo(&mut timeline).unwrap();
        history.undo(&mut timeline).unwrap();
        assert_eq!(timeline, original);
    }

    /// "One. Two. Three." over a 3 s narration: each word 0.5 s, a second
    /// apart, each its own caption.
    fn captioned() -> Timeline {
        let narration = crate::timeline::tests::narrated(
            "One. Two. Three.",
            &[(0, 500), (1_000, 1_500), (2_000, 2_500)],
            3_000,
        );
        Timeline::rough_cut(
            &plan(vec![scene(0, 3_000)]),
            &narration,
            AspectRatio::Landscape,
        )
    }

    fn caption_times(timeline: &Timeline) -> Vec<(Duration, Duration)> {
        timeline
            .captions()
            .lines()
            .iter()
            .map(|caption| (caption.start, caption.end))
            .collect()
    }

    #[test]
    fn trimming_a_caption_stops_at_its_neighbours_and_a_frame_short() {
        let before = captioned();
        let after = round_trip(
            &before,
            Edit::Trim {
                track: Track::Captions,
                index: 1,
                edge: Edge::Start,
                by: Shift::earlier(ms(300)),
            },
        );
        assert_eq!(caption_times(&after)[1], (ms(700), ms(1_500)));
        assert_eq!(after.span(ItemRef::caption(1)), Some((ms(700), ms(800))));

        let limits = |edge| before.trim_limits(ItemRef::caption(1), edge).unwrap();
        assert_eq!(
            limits(Edge::Start),
            (
                Shift::earlier(ms(500)),
                Shift::later(ms(500) - frame_time(1))
            )
        );
        assert_eq!(
            limits(Edge::End),
            (
                Shift::earlier(ms(500) - frame_time(1)),
                Shift::later(ms(500))
            )
        );
        // The last one stops at the narration's end, the first at its start.
        let last = before.trim_limits(ItemRef::caption(2), Edge::End).unwrap();
        assert_eq!(last.1, Shift::later(ms(500)));
        let first = before
            .trim_limits(ItemRef::caption(0), Edge::Start)
            .unwrap();
        assert_eq!(first.0, Shift::ZERO);

        let mut far = before.clone();
        far.apply(&Edit::Trim {
            track: Track::Captions,
            index: 1,
            edge: Edge::End,
            by: Shift::later(ms(5_000)),
        })
        .unwrap();
        assert_eq!(
            caption_times(&far)[1],
            (ms(1_000), ms(2_000)),
            "up to the next"
        );
    }

    #[test]
    fn a_caption_s_text_changes_and_undoes() {
        let after = round_trip(
            &captioned(),
            Edit::SetCaptionText {
                index: 0,
                text: "  Uno,\n dos ".into(),
            },
        );
        assert_eq!(after.captions().lines()[0].text, "Uno, dos");
        let mut timeline = captioned();
        for text in ["", "   ", &"a".repeat(crate::MAX_CAPTION_CHARS + 1)] {
            assert_eq!(
                timeline.apply(&Edit::SetCaptionText {
                    index: 0,
                    text: text.into()
                }),
                Err(EditError::InvalidText)
            );
        }
        assert_eq!(
            timeline.apply(&Edit::SetCaptionText {
                index: 0,
                text: "One.".into()
            }),
            Err(EditError::NoChange)
        );
        assert_eq!(
            timeline.apply(&Edit::SetCaptionText {
                index: 9,
                text: "Nine.".into()
            }),
            Err(EditError::NoSuchItem)
        );
        assert_eq!(timeline, captioned());
    }

    #[test]
    fn a_deleted_caption_comes_back_where_it_was() {
        let after = round_trip(
            &captioned(),
            Edit::Delete {
                track: Track::Captions,
                index: 1,
            },
        );
        assert_eq!(
            caption_times(&after),
            [(ms(0), ms(500)), (ms(2_000), ms(2_500))]
        );

        let mut timeline = captioned();
        let overlapping = Item::Caption(crate::Caption {
            text: "Over.".into(),
            start: ms(400),
            end: ms(900),
        });
        assert_eq!(
            timeline.apply(&Edit::Insert {
                track: Track::Captions,
                index: 1,
                item: overlapping
            }),
            Err(EditError::Overlaps)
        );
        let blank = Item::Caption(crate::Caption {
            text: " ".into(),
            start: ms(600),
            end: ms(900),
        });
        assert_eq!(
            timeline.apply(&Edit::Insert {
                track: Track::Captions,
                index: 1,
                item: blank
            }),
            Err(EditError::InvalidText)
        );
        assert_eq!(timeline, captioned());
    }

    #[test]
    fn captions_turn_off_and_change_style_and_undo() {
        let hidden = round_trip(&captioned(), Edit::ShowCaptions(false));
        assert!(!hidden.captions().shown());
        let styled = round_trip(&captioned(), Edit::SetCaptionStyle(CaptionStyle::Punch));
        assert_eq!(styled.captions().style(), CaptionStyle::Punch);
        let lit = round_trip(
            &captioned(),
            Edit::SetCaptionHighlight(CaptionHighlight::Green),
        );
        assert_eq!(lit.captions().highlight(), CaptionHighlight::Green);
        let mut timeline = captioned();
        assert_eq!(
            timeline.apply(&Edit::ShowCaptions(true)),
            Err(EditError::NoChange)
        );
        assert_eq!(
            timeline.apply(&Edit::SetCaptionStyle(CaptionStyle::Clean)),
            Err(EditError::NoChange)
        );
        assert_eq!(
            timeline.apply(&Edit::SetCaptionHighlight(CaptionHighlight::Yellow)),
            Err(EditError::NoChange)
        );
    }

    #[test]
    fn captions_are_not_split_moved_or_faded() {
        let before = captioned();
        for edit in [
            Edit::Split {
                track: Track::Captions,
                index: 0,
                at: ms(200),
            },
            Edit::Join {
                track: Track::Captions,
                index: 0,
            },
            Edit::Move {
                track: Track::Captions,
                index: 0,
                to: ms(200),
            },
            Edit::SetFades {
                track: Track::Captions,
                index: 0,
                fade_in: ms(100),
                fade_out: ms(100),
            },
        ] {
            let mut timeline = before.clone();
            assert_eq!(
                timeline.apply(&edit),
                Err(EditError::WrongTrack),
                "{edit:?}"
            );
            assert_eq!(timeline, before);
        }
    }

    #[test]
    fn cutting_narration_away_takes_its_captions_and_undoing_brings_them_back() {
        let mut timeline = captioned();
        let mut history = History::default();
        history
            .apply(
                &mut timeline,
                &Edit::Split {
                    track: Track::Narration,
                    index: 0,
                    at: ms(1_800),
                },
            )
            .unwrap();
        history
            .apply(
                &mut timeline,
                &Edit::Delete {
                    track: Track::Narration,
                    index: 1,
                },
            )
            .unwrap();
        let shown: Vec<usize> = timeline.caption_spans().iter().map(|s| s.index).collect();
        assert_eq!(shown, [0, 1], "\"Three.\" went with its narration");
        assert_eq!(timeline.span(ItemRef::caption(2)), None);
        assert_eq!(timeline.captions().lines().len(), 3, "kept, to come back");

        history.undo(&mut timeline).unwrap();
        let shown: Vec<usize> = timeline.caption_spans().iter().map(|s| s.index).collect();
        assert_eq!(shown, [0, 1, 2]);
    }

    #[test]
    fn the_frame_shape_changes_and_undoes() {
        let vertical = round_trip(&timeline(), Edit::SetAspect(AspectRatio::Vertical));
        assert_eq!(vertical.aspect(), AspectRatio::Vertical);
        assert_eq!(
            timeline().apply(&Edit::SetAspect(AspectRatio::Landscape)),
            Err(EditError::NoChange)
        );
    }

    #[test]
    fn a_clips_crop_moves_and_undoes() {
        let left = Framing::Crop(CropPosition::new(0, 500));
        let moved = round_trip(
            &timeline(),
            Edit::SetFraming {
                index: 1,
                framing: left,
            },
        );
        assert_eq!(moved.video()[1].framing, left);
        assert_eq!(moved.video()[0].framing, Framing::FILL, "only that clip");
        let fitted = round_trip(
            &moved,
            Edit::SetFraming {
                index: 1,
                framing: Framing::Fit,
            },
        );
        assert_eq!(fitted.video()[1].framing, Framing::Fit);

        let mut timeline = timeline();
        assert_eq!(
            timeline.apply(&Edit::SetFraming {
                index: 0,
                framing: Framing::FILL
            }),
            Err(EditError::NoChange)
        );
        assert_eq!(
            timeline.apply(&Edit::SetFraming {
                index: 3,
                framing: Framing::Fit
            }),
            Err(EditError::NoSuchItem)
        );
    }

    #[test]
    fn a_crop_follows_its_clip_through_splits_and_reorders() {
        let mut timeline = timeline();
        let right = Framing::Crop(CropPosition::new(1_000, 500));
        timeline
            .apply(&Edit::SetFraming {
                index: 1,
                framing: right,
            })
            .unwrap();
        timeline
            .apply(&Edit::Split {
                track: Track::Video,
                index: 1,
                at: ms(2_000),
            })
            .unwrap();
        assert_eq!(timeline.video()[1].framing, right);
        assert_eq!(timeline.video()[2].framing, right, "both halves");
        timeline.apply(&Edit::Reorder { from: 2, to: 0 }).unwrap();
        assert_eq!(timeline.video()[0].framing, right);
        assert_eq!(timeline.video()[1].framing, Framing::FILL);
    }

    #[test]
    fn halves_framed_apart_do_not_join() {
        let mut timeline = timeline();
        timeline
            .apply(&Edit::Split {
                track: Track::Video,
                index: 1,
                at: ms(2_000),
            })
            .unwrap();
        timeline
            .apply(&Edit::SetFraming {
                index: 2,
                framing: Framing::Fit,
            })
            .unwrap();
        assert_eq!(
            timeline.apply(&Edit::Join {
                track: Track::Video,
                index: 1
            }),
            Err(EditError::CannotJoin)
        );
    }

    fn music_bed() -> MediaAsset {
        asset(MediaKind::Audio, "bed.mp3", 3_000)
    }

    /// The timeline with `asset` placed on `track` at `at`.
    fn placed(timeline: &Timeline, asset: &MediaAsset, track: Track, at: u64) -> Timeline {
        let (edit, _) = timeline.place(asset, track, ms(at)).unwrap();
        round_trip(timeline, edit)
    }

    #[test]
    fn audio_is_placed_whole_at_the_playhead_and_removed_by_undo() {
        let after = placed(&timeline(), &music_bed(), Track::Music, 500);
        let music = after.music();
        assert_eq!(music.len(), 1);
        assert_eq!((music[0].at, music[0].duration), (ms(500), ms(3_000)));
        assert_eq!((music[0].start, music[0].length), (ms(0), ms(3_000)));
        assert_eq!(after.duration(), ms(4_000));
        assert!(after.sfx().is_empty() && after.narration().len() == 1);
    }

    #[test]
    fn audio_placed_over_an_item_goes_after_it_and_stops_at_the_next() {
        let boom = asset(MediaKind::Audio, "boom.wav", 2_000);
        let first = placed(&timeline(), &boom, Track::Sfx, 0);
        let second = placed(&first, &boom, Track::Sfx, 3_000);
        // Over the first: it goes where that one ends, short of the second.
        let third = placed(&second, &boom, Track::Sfx, 1_200);
        let spans: Vec<_> = third
            .sfx()
            .iter()
            .map(|item| (item.at, item.duration))
            .collect();
        assert_eq!(
            spans,
            [
                (ms(0), ms(2_000)),
                (ms(2_000), ms(1_000)),
                (ms(3_000), ms(2_000))
            ]
        );
        // No room left between them.
        assert_eq!(
            third.place(&boom, Track::Sfx, ms(2_500)),
            Err(EditError::Overlaps)
        );
    }

    #[test]
    fn media_goes_only_on_the_tracks_of_its_kind() {
        let timeline = timeline();
        let footage = asset(MediaKind::Video, "walk.mp4", 2_000);
        for track in [Track::Music, Track::Sfx, Track::Narration, Track::Captions] {
            assert_eq!(
                timeline.place(&footage, track, ms(0)),
                Err(EditError::WrongTrack)
            );
        }
        for track in [Track::Video, Track::Narration, Track::Captions] {
            assert_eq!(
                timeline.place(&music_bed(), track, ms(0)),
                Err(EditError::WrongTrack)
            );
        }
    }

    #[test]
    fn footage_goes_whole_into_the_cut_nearest_the_playhead() {
        // 2.01 s: whole frames only.
        let footage = asset(MediaKind::Video, "walk.mp4", 2_010);
        // Inside the clip at 1-3 s, before its middle: before it.
        let after = placed(&timeline(), &footage, Track::Video, 1_500);
        let video = after.video();
        assert_eq!(video.len(), 4);
        assert_eq!(video[1].scene(), None);
        assert_eq!(
            video[1].source,
            VideoSource::Clip {
                file: "walk.mp4".into(),
                length: ms(2_010)
            }
        );
        assert_eq!(
            (video[1].at, video[1].duration),
            (ms(1_000), frame_time(60))
        );
        assert_eq!(video[1].framing, Framing::FILL);
        // The scenes after it move on.
        assert_eq!(video[2].at, ms(1_000) + frame_time(60));
        // Past the middle of the last clip: at the end.
        let at_end = placed(&timeline(), &footage, Track::Video, 3_600);
        assert_eq!(at_end.video()[3].scene(), None);
        // Shorter than a frame: nothing to place.
        let blink = asset(MediaKind::Video, "blink.mp4", 20);
        assert_eq!(
            timeline().place(&blink, Track::Video, ms(0)),
            Err(EditError::TooShort)
        );
        let click = asset(MediaKind::Audio, "click.wav", 20);
        assert_eq!(
            timeline().place(&click, Track::Sfx, ms(0)),
            Err(EditError::TooShort)
        );
    }

    #[test]
    fn music_and_sfx_items_split_trim_move_fade_and_go_like_the_narration() {
        let with_music = placed(&timeline(), &music_bed(), Track::Music, 0);
        let split = round_trip(
            &with_music,
            Edit::Split {
                track: Track::Music,
                index: 0,
                at: ms(1_000),
            },
        );
        assert_eq!(split.music()[1].start, ms(1_000));
        let moved = round_trip(
            &split,
            Edit::Move {
                track: Track::Music,
                index: 1,
                to: ms(1_500),
            },
        );
        assert_eq!(moved.music()[1].at, ms(1_500));
        let trimmed = round_trip(
            &moved,
            Edit::Trim {
                track: Track::Music,
                index: 1,
                edge: Edge::Start,
                by: Shift::earlier(ms(2_000)),
            },
        );
        // It stops at the piece before it.
        assert_eq!(trimmed.music()[1].at, ms(1_000));
        let faded = round_trip(
            &trimmed,
            Edit::SetFades {
                track: Track::Music,
                index: 0,
                fade_in: ms(300),
                fade_out: ms(0),
            },
        );
        assert_eq!(faded.music()[0].fade_in, ms(300));
        let gone = round_trip(
            &faded,
            Edit::Delete {
                track: Track::Music,
                index: 0,
            },
        );
        assert_eq!(gone.music().len(), 1);
        assert_eq!(gone.narration(), timeline().narration());
    }

    #[test]
    fn a_generated_track_takes_over_from_the_one_it_replaces_and_leaves_the_rest() {
        let mut first = asset(MediaKind::Audio, "media-first.mp3", 4_000);
        first.source = crate::AssetSource::Generated;
        let mut second = asset(MediaKind::Audio, "media-second.mp3", 4_000);
        second.source = crate::AssetSource::Generated;
        let boom = asset(MediaKind::Audio, "boom.wav", 500);

        // An empty music track: from the start.
        let used = timeline().use_music(None, &first).unwrap();
        let music = used.music();
        assert_eq!(music.len(), 1);
        assert_eq!(
            (music[0].file.as_str(), music[0].at),
            ("media-first.mp3", ms(0))
        );
        assert_eq!(music[0].duration, ms(4_000));
        // Using it again changes nothing.
        assert_eq!(used.use_music(None, &first).unwrap(), used);

        // The user cut the first track and added a hit of their own; the
        // new track takes the first one's place, the hit stays.
        let mut edited = used.clone();
        edited
            .apply(&Edit::Split {
                track: Track::Music,
                index: 0,
                at: ms(2_000),
            })
            .unwrap();
        edited
            .apply(&Edit::Delete {
                track: Track::Music,
                index: 1,
            })
            .unwrap();
        let edited = placed(&edited, &boom, Track::Music, 3_000);
        let swapped = edited.use_music(Some("media-first.mp3"), &second).unwrap();
        let files: Vec<_> = swapped
            .music()
            .iter()
            .map(|item| (item.file.as_str(), item.at, item.duration))
            .collect();
        assert_eq!(
            files,
            [
                ("media-second.mp3", ms(0), ms(3_000)),
                ("boom.wav", ms(3_000), ms(500)),
            ],
            "the new track plays up to the user's hit"
        );
        assert_eq!(swapped.narration(), timeline().narration());
        assert_eq!(swapped.video(), timeline().video());
    }

    fn fade_black(frames: u32) -> Transition {
        Transition::new(crate::TransitionKind::FadeBlack, frames)
    }

    #[test]
    fn a_transition_goes_into_a_clip_and_undoes() {
        let before = timeline();
        let edited = round_trip(
            &before,
            Edit::SetTransition {
                index: 1,
                transition: Some(fade_black(15)),
            },
        );
        assert_eq!(edited.video()[1].transition, Some(fade_black(15)));
        let played = edited.transition_into(1).unwrap();
        assert_eq!(played.kind, crate::TransitionKind::FadeBlack);
        // Centered on the cut at 1 s: the clip comes in 7 frames early and
        // the still plays on 8 frames past the cut.
        assert_eq!(
            (played.start, played.cut, played.end),
            (frame_time(23), ms(1_000), frame_time(38))
        );
        assert_eq!(spans(&edited, Track::Video), spans(&before, Track::Video));
        assert_eq!(edited.duration(), before.duration(), "the cut stays put");
        assert_unchanged(
            Edit::SetTransition {
                index: 1,
                transition: None,
            },
            EditError::NoChange,
        );
        assert_unchanged(
            Edit::SetTransition {
                index: 3,
                transition: Some(fade_black(15)),
            },
            EditError::NoSuchItem,
        );
    }

    #[test]
    fn short_clips_play_their_transitions_cut_short() {
        let mut timeline = timeline();
        timeline
            .apply(&Edit::SetTransition {
                index: 2,
                transition: Some(fade_black(60)),
            })
            .unwrap();
        // Kept as set, played no longer than the 1 s still after the cut.
        assert_eq!(timeline.video()[2].transition, Some(fade_black(60)));
        assert_eq!(
            timeline.transition_into(2).unwrap().duration(),
            frame_time(30)
        );
        // The clip before is trimmed to 3 frames: the transition plays 3.
        let trim = Edit::Trim {
            track: Track::Video,
            index: 1,
            edge: Edge::End,
            by: Shift::earlier(ms(1_900)),
        };
        timeline.apply(&trim).unwrap();
        let played = timeline.transition_into(2).unwrap();
        assert_eq!(played.duration(), frame_time(3));
        assert_eq!(played.cut, frame_time(33));
        // At one frame there is nothing to blend: a cut.
        timeline
            .apply(&Edit::Trim {
                track: Track::Video,
                index: 1,
                edge: Edge::End,
                by: Shift::earlier(frame_time(2)),
            })
            .unwrap();
        assert_eq!(frame_at(timeline.video()[1].duration), 1);
        assert_eq!(timeline.transition_into(2), None);
        // The first clip has none to play, whatever it keeps.
        timeline
            .apply(&Edit::SetTransition {
                index: 0,
                transition: Some(fade_black(15)),
            })
            .unwrap();
        assert_eq!(timeline.transition_into(0), None);
        assert_eq!(timeline.transitions().len(), 0);
    }

    #[test]
    fn transitions_on_both_sides_of_a_short_clip_never_overlap() {
        let mut timeline = timeline();
        timeline
            .apply(&Edit::Trim {
                track: Track::Video,
                index: 1,
                edge: Edge::End,
                by: Shift::earlier(frame_time(49)),
            })
            .unwrap();
        // The middle clip is 11 frames long; both transitions want 2 s.
        let edit = timeline
            .transition_everywhere(Some(Transition::new(crate::TransitionKind::WipeLeft, 60)));
        timeline.apply(&edit).unwrap();
        let played = timeline.transitions();
        assert_eq!(played.len(), 2);
        let (into, out) = (played[0].1, played[1].1);
        assert_eq!(into.duration(), frame_time(11));
        assert_eq!(out.duration(), frame_time(11));
        assert!(into.end <= out.start, "{into:?} {out:?}");
    }

    #[test]
    fn a_transition_moves_with_its_clip_and_a_split_makes_none() {
        let mut timeline = timeline();
        timeline
            .apply(&Edit::SetTransition {
                index: 1,
                transition: Some(fade_black(15)),
            })
            .unwrap();
        let split = round_trip(
            &timeline,
            Edit::Split {
                track: Track::Video,
                index: 1,
                at: ms(2_000),
            },
        );
        assert_eq!(split.video()[1].transition, Some(fade_black(15)));
        assert_eq!(split.video()[2].transition, None, "a seamless cut");

        let reordered = round_trip(&timeline, Edit::Reorder { from: 1, to: 2 });
        assert_eq!(reordered.video()[2].transition, Some(fade_black(15)));
        assert_eq!(reordered.video()[1].transition, None);

        let deleted = round_trip(
            &timeline,
            Edit::Delete {
                track: Track::Video,
                index: 1,
            },
        );
        assert!(deleted.video().iter().all(|item| item.transition.is_none()));

        // Halves with a transition between them do not join: it would be
        // lost.
        let mut halves = split.clone();
        halves
            .apply(&Edit::SetTransition {
                index: 2,
                transition: Some(fade_black(6)),
            })
            .unwrap();
        assert_eq!(
            halves.apply(&Edit::Join {
                track: Track::Video,
                index: 1,
            }),
            Err(EditError::CannotJoin)
        );
    }

    #[test]
    fn a_transition_goes_on_every_cut_at_once_and_undoes_as_one() {
        let mut before = timeline();
        before
            .apply(&Edit::SetTransition {
                index: 0,
                transition: Some(fade_black(9)),
            })
            .unwrap();
        let crossfade = Transition::default();
        let edit = before.transition_everywhere(Some(crossfade));
        let after = round_trip(&before, edit);
        let set: Vec<_> = after.video().iter().map(|item| item.transition).collect();
        assert_eq!(set, [Some(fade_black(9)), Some(crossfade), Some(crossfade)]);
        assert_eq!(
            after
                .clone()
                .apply(&after.transition_everywhere(Some(crossfade))),
            Err(EditError::NoChange)
        );
        // Cuts everywhere take them off again; the first clip keeps its own.
        let cuts = round_trip(&after, after.transition_everywhere(None));
        let set: Vec<_> = cuts.video().iter().map(|item| item.transition).collect();
        assert_eq!(set, [Some(fade_black(9)), None, None]);
        assert_eq!(
            after.clone().apply(&Edit::SetTransitions(vec![None])),
            Err(EditError::NoSuchItem)
        );
    }

    #[test]
    fn the_cut_nearest_a_time_is_found_by_the_clip_after_it() {
        let timeline = timeline();
        assert_eq!(timeline.cut_near(ms(0)), Some(1));
        assert_eq!(timeline.cut_near(ms(1_900)), Some(1));
        assert_eq!(timeline.cut_near(ms(2_100)), Some(2));
        assert_eq!(timeline.cut_near(ms(4_000)), Some(2));
        let mut one = timeline.clone();
        one.apply(&Edit::Delete {
            track: Track::Video,
            index: 0,
        })
        .unwrap();
        one.apply(&Edit::Delete {
            track: Track::Video,
            index: 0,
        })
        .unwrap();
        assert_eq!(one.cut_near(ms(500)), None);
    }

    fn motion(kind: crate::MotionKind) -> Option<Motion> {
        Some(Motion::new(kind, crate::MotionStrength::Medium))
    }

    #[test]
    fn a_motion_goes_on_a_clip_and_undoes() {
        let before = timeline();
        let edited = round_trip(
            &before,
            Edit::SetMotion {
                index: 2,
                motion: motion(crate::MotionKind::PanUp),
            },
        );
        assert_eq!(edited.video()[2].motion, motion(crate::MotionKind::PanUp));
        assert_eq!(
            edited.video()[2].played_motion(),
            motion(crate::MotionKind::PanUp)
        );
        assert_eq!(spans(&edited, Track::Video), spans(&before, Track::Video));
        assert_unchanged(
            Edit::SetMotion {
                index: 0,
                motion: None,
            },
            EditError::NoChange,
        );
        assert_unchanged(
            Edit::SetMotion {
                index: 3,
                motion: None,
            },
            EditError::NoSuchItem,
        );
    }

    #[test]
    fn a_clip_keeps_a_motion_it_does_not_play() {
        let mut timeline = timeline();
        timeline
            .apply(&Edit::SetMotion {
                index: 1,
                motion: motion(crate::MotionKind::ZoomIn),
            })
            .unwrap();
        assert!(timeline.video()[1].has_source_time());
        assert_eq!(timeline.video()[1].played_motion(), None);
    }

    #[test]
    fn one_motion_or_a_varied_pattern_goes_on_every_scene_and_undoes() {
        let before = timeline();
        let zoomed = round_trip(
            &before,
            before.motion_everywhere(motion(crate::MotionKind::ZoomOut)),
        );
        let set: Vec<_> = zoomed.video().iter().map(|item| item.motion).collect();
        assert_eq!(set, [motion(crate::MotionKind::ZoomOut); 3]);
        // The scene showing a clip keeps it unplayed, for when it shows its
        // still again.
        assert_eq!(zoomed.video()[1].played_motion(), None);
        // The scenes take the pattern in turn, so neighbours differ.
        let varied = round_trip(
            &zoomed,
            zoomed.motion_pattern(crate::MotionDefault::new(
                crate::MotionPattern::Varied,
                crate::MotionStrength::Strong,
            )),
        );
        let strong = |kind| Some(Motion::new(kind, crate::MotionStrength::Strong));
        let set: Vec<_> = varied.video().iter().map(|item| item.motion).collect();
        assert_eq!(
            set,
            [
                strong(crate::MotionKind::ZoomIn),
                strong(crate::MotionKind::PanRight),
                strong(crate::MotionKind::ZoomOut)
            ]
        );
        let still = round_trip(&varied, varied.motion_everywhere(None));
        assert!(still.video().iter().all(|item| item.motion.is_none()));
        assert_eq!(
            varied.clone().apply(&Edit::SetMotions(vec![None])),
            Err(EditError::NoSuchItem)
        );
    }

    #[test]
    fn a_split_still_moves_in_each_piece_and_joins_only_with_the_same_motion() {
        let mut timeline = timeline();
        timeline
            .apply(&Edit::SetMotion {
                index: 0,
                motion: motion(crate::MotionKind::PanLeft),
            })
            .unwrap();
        let split = round_trip(
            &timeline,
            Edit::Split {
                track: Track::Video,
                index: 0,
                at: ms(400),
            },
        );
        assert_eq!(split.video()[0].motion, split.video()[1].motion);
        let mut changed = split.clone();
        changed
            .apply(&Edit::SetMotion {
                index: 1,
                motion: None,
            })
            .unwrap();
        assert_eq!(
            changed.apply(&Edit::Join {
                track: Track::Video,
                index: 0,
            }),
            Err(EditError::CannotJoin)
        );
    }
}
