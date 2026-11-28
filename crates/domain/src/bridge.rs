//! AI bridges over a cut (#363, ADR-0050): two pictures
//! side by side can cut abruptly, the scenery jumping while the narration
//! goes on about the same thing. A bridge is a short clip a model makes
//! from the frame the first picture shows just before the cut to the frame
//! the second shows just after it, and it goes **over** the cut: it takes
//! the end of the first picture and the start of the second, half of its
//! length each, and never adds time. The video keeps its length, so the
//! narration and every cut after it stay where they were.
//!
//! A bridge is made by a job (`JobKind::BridgeClip`) and waits on its cut
//! for the user's review: **Use** covers its stretch with one
//! `Edit::Replace`, undoable like any edit. The cut it was made for is
//! remembered by the two pictures around it ([`BridgeCut`]); trim, move or
//! remove either of them and the bridge no longer fits, and says so.

use std::time::{Duration, SystemTime};

use crate::{
    Edit, EditError, FPS, ItemRef, JobId, ProfileId, Provider, RepositoryError, Timeline,
    VideoItem, VideoProjectId, VideoSource, frame_at, frame_time, min_length, nearest_frame,
};

uuid_id!(
    /// Identifies one bridge of a project.
    BridgeId
);

/// What every bridge asks of its model, before the user's note: one shot
/// that gets from the first frame to the last with nothing in between
/// that would read as a cut.
pub const BRIDGE_PROMPT: &str = "One continuous shot that moves smoothly and naturally from the \
first frame to the last frame, as a camera move or a motion within the scene. No cuts, no fades, \
no flashes, no new scene, no text and no one speaking.";

/// The most a user's note to the model may hold.
pub const BRIDGE_NOTE_MAX_CHARS: usize = 500;

/// The prompt a bridge is made with: the fixed one, then the user's note.
pub fn bridge_prompt(note: &str) -> String {
    match note.trim() {
        "" => BRIDGE_PROMPT.to_owned(),
        note => format!("{BRIDGE_PROMPT} {note}"),
    }
}

/// Why a bridge cannot go over a cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BridgeRefusal {
    /// There is no cut there: the first picture, or past the last one.
    #[error("there is no cut there")]
    NoCut,
    /// Half the bridge is more than one of the two pictures can give while
    /// keeping a frame of its own.
    #[error("the pictures around the cut are too short for a bridge that long")]
    TooLong,
    /// One of the two pictures has nothing to show yet: a scene without
    /// its image.
    #[error("a picture around the cut has nothing to show")]
    NoPicture,
}

/// The stretch of the video track a bridge covers: the end of the picture
/// before cut `cut` and the start of the one after it, from frame `from`
/// to frame `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeSpan {
    /// The cut, by the index of the picture after it.
    pub cut: usize,
    pub from: u64,
    pub to: u64,
}

impl BridgeSpan {
    pub fn start(&self) -> Duration {
        frame_time(self.from)
    }

    pub fn end(&self) -> Duration {
        frame_time(self.to)
    }

    pub fn length(&self) -> Duration {
        self.end() - self.start()
    }
}

/// Where a bridge's first or last frame is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameSource {
    /// The frame a video file shows at `at`, in the file's own time.
    Video { file: String, at: Duration },
    /// A still image, whole.
    Still { file: String },
}

impl FrameSource {
    pub fn file(&self) -> &str {
        match self {
            FrameSource::Video { file, .. } | FrameSource::Still { file } => file,
        }
    }
}

/// The cut a bridge was made for, by the two pictures around it: what each
/// shows, from where in its source, where it sits and for how long. A cut
/// whose pictures changed in any of these is another cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeCut {
    pub before: String,
    pub after: String,
}

impl BridgeCut {
    /// The cut before picture `cut` of `timeline`.
    pub fn of(timeline: &Timeline, cut: usize) -> Option<BridgeCut> {
        let before = timeline.video.get(cut.checked_sub(1)?)?;
        let after = timeline.video.get(cut)?;
        Some(BridgeCut {
            before: side_key(before),
            after: side_key(after),
        })
    }

    /// Where the cut is in `timeline` now, by the index of the picture
    /// after it; `None` once either picture changed.
    pub fn find(&self, timeline: &Timeline) -> Option<usize> {
        (1..timeline.video.len()).find(|&cut| {
            side_key(&timeline.video[cut - 1]) == self.before
                && side_key(&timeline.video[cut]) == self.after
        })
    }
}

/// One picture of a cut, as a key: its source file (or none), where it
/// starts in it, where it sits and for how long, to the frame.
fn side_key(item: &VideoItem) -> String {
    format!(
        "{}|{}|{}|{}",
        item.source.file().unwrap_or("-"),
        nearest_frame(item.start),
        nearest_frame(item.at),
        nearest_frame(item.duration)
    )
}

/// How many frames a bridge of `seconds` runs.
pub fn bridge_frames(seconds: u32) -> u64 {
    u64::from(seconds) * u64::from(FPS)
}

impl Timeline {
    /// The stretch a bridge of `seconds` covers over the cut before
    /// picture `cut`: half its frames from the end of the picture before,
    /// the rest from the start of the one after. Each picture keeps at
    /// least a frame of its own.
    pub fn bridge_span(&self, cut: usize, seconds: u32) -> Result<BridgeSpan, BridgeRefusal> {
        let before = cut
            .checked_sub(1)
            .and_then(|before| self.video.get(before))
            .ok_or(BridgeRefusal::NoCut)?;
        let after = self.video.get(cut).ok_or(BridgeRefusal::NoCut)?;
        let length = bridge_frames(seconds);
        if length == 0 {
            return Err(BridgeRefusal::TooLong);
        }
        let at = nearest_frame(after.at);
        let from = at.checked_sub(length / 2).ok_or(BridgeRefusal::TooLong)?;
        let to = from + length;
        if from <= nearest_frame(before.at) || to >= nearest_frame(after.end()) {
            return Err(BridgeRefusal::TooLong);
        }
        Ok(BridgeSpan { cut, from, to })
    }

    /// The bridge's first frame (what the picture before the cut shows
    /// on the frame just before the stretch) and its last (what the one
    /// after shows on the frame the stretch ends on), read from their
    /// sources.
    pub fn bridge_ends(
        &self,
        span: &BridgeSpan,
    ) -> Result<(FrameSource, FrameSource), BridgeRefusal> {
        let before = span
            .cut
            .checked_sub(1)
            .and_then(|before| self.video.get(before))
            .ok_or(BridgeRefusal::NoCut)?;
        let after = self.video.get(span.cut).ok_or(BridgeRefusal::NoCut)?;
        let first = frame_source(before, frame_time(span.from - 1))?;
        let last = frame_source(after, frame_time(span.to))?;
        Ok((first, last))
    }

    /// The edit that puts `item` (the bridge, placed as footage) on
    /// `span`, cutting the pictures on both sides back to what the
    /// stretch leaves them, and the item it becomes.
    pub fn put_bridge(
        &self,
        span: &BridgeSpan,
        item: VideoItem,
    ) -> Result<(Edit, ItemRef), EditError> {
        let before = span
            .cut
            .checked_sub(1)
            .and_then(|before| self.video.get(before))
            .ok_or(EditError::NoSuchItem)?;
        let after = self.video.get(span.cut).ok_or(EditError::NoSuchItem)?;
        // Over the cut, each side keeping a frame of its own.
        let cut = nearest_frame(after.at);
        if span.from <= nearest_frame(before.at)
            || span.from >= cut
            || span.to <= cut
            || span.to >= nearest_frame(after.end())
        {
            return Err(EditError::OutsideItem);
        }
        self.overwrite(span.from, span.to, item)
    }
}

/// Where `item` shows what it shows at `time` (timeline time): a clip's
/// frame in the file's own time (a stretched clip plays slowed, so its
/// file is shorter), a still's image.
fn frame_source(item: &VideoItem, time: Duration) -> Result<FrameSource, BridgeRefusal> {
    match &item.source {
        VideoSource::Clip {
            file,
            length,
            stretched_from,
        } => {
            let played = (item.start + time.saturating_sub(item.at))
                .min(length.saturating_sub(min_length()));
            let at = match stretched_from {
                Some(recorded) if !length.is_zero() => Duration::from_nanos(
                    (played.as_nanos() * recorded.as_nanos() / length.as_nanos()) as u64,
                ),
                _ => played,
            };
            // On a frame of its own, as the timeline counts them.
            Ok(FrameSource::Video {
                file: file.clone(),
                at: frame_time(frame_at(at)),
            })
        }
        VideoSource::Still(file) => Ok(FrameSource::Still { file: file.clone() }),
        VideoSource::Missing => Err(BridgeRefusal::NoPicture),
    }
}

/// A bridge asked for over a cut: being made, failed, or made and waiting
/// for the user to use or discard it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bridge {
    pub id: BridgeId,
    pub project: VideoProjectId,
    pub owner: ProfileId,
    /// The cut it goes over.
    pub cut: BridgeCut,
    /// The stretch it covers, in frames.
    pub from: u64,
    pub to: u64,
    pub first: FrameSource,
    pub last: FrameSource,
    pub provider: Provider,
    /// The model, as the provider names it.
    pub model: String,
    pub seconds: u32,
    /// What the user asked of the model on top of the fixed prompt.
    pub note: String,
    /// The job that makes it.
    pub job: JobId,
    /// The clip, in the project folder, once made.
    pub file: Option<String>,
    pub asked_at: SystemTime,
}

impl Bridge {
    /// Where it goes in `timeline`: its cut, if the two pictures around
    /// it are still as they were, and the stretch it was made for.
    pub fn span_in(&self, timeline: &Timeline) -> Option<BridgeSpan> {
        let cut = self.cut.find(timeline)?;
        Some(BridgeSpan {
            cut,
            from: self.from,
            to: self.to,
        })
    }

    /// The prompt it is made with.
    pub fn prompt(&self) -> String {
        bridge_prompt(&self.note)
    }

    /// The name its clip gets in the project folder.
    pub fn file_name(id: BridgeId) -> String {
        format!("bridge-{id}.mp4")
    }

    /// The names its two frames get in the project folder, while it is
    /// being made.
    pub fn frame_names(id: BridgeId) -> (String, String) {
        (
            format!("bridge-{id}-first.jpg"),
            format!("bridge-{id}-last.jpg"),
        )
    }
}

/// Persistence port for bridges.
pub trait BridgeRepository: Send + Sync {
    /// The project's bridges, oldest first.
    fn bridges(&self, project: VideoProjectId) -> Result<Vec<Bridge>, RepositoryError>;

    fn bridge(&self, id: BridgeId) -> Result<Option<Bridge>, RepositoryError>;

    fn save_bridge(&self, bridge: &Bridge) -> Result<(), RepositoryError>;

    /// Records the made clip of bridge `id`; `false`, and nothing saved,
    /// when the bridge is gone.
    fn set_bridge_file(&self, id: BridgeId, file: &str) -> Result<bool, RepositoryError>;

    fn delete_bridge(&self, id: BridgeId) -> Result<(), RepositoryError>;
}

impl<T: BridgeRepository + ?Sized> BridgeRepository for std::sync::Arc<T> {
    fn bridges(&self, project: VideoProjectId) -> Result<Vec<Bridge>, RepositoryError> {
        (**self).bridges(project)
    }

    fn bridge(&self, id: BridgeId) -> Result<Option<Bridge>, RepositoryError> {
        (**self).bridge(id)
    }

    fn save_bridge(&self, bridge: &Bridge) -> Result<(), RepositoryError> {
        (**self).save_bridge(bridge)
    }

    fn set_bridge_file(&self, id: BridgeId, file: &str) -> Result<bool, RepositoryError> {
        (**self).set_bridge_file(id, file)
    }

    fn delete_bridge(&self, id: BridgeId) -> Result<(), RepositoryError> {
        (**self).delete_bridge(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::{image, ms, narration, plan, scene};
    use crate::{AspectRatio, Framing, History, Picture};

    /// Four stills over an 8 s narration, 2 s each.
    fn cut() -> Timeline {
        let scenes = (0..4)
            .map(|n| {
                let mut s = scene(n * 2_000, (n + 1) * 2_000);
                s.image = Some(image(&format!("{n}.png")));
                s
            })
            .collect();
        Timeline::rough_cut(&plan(scenes), &narration(8_000), AspectRatio::Landscape)
    }

    fn bridge_item(file: &str) -> VideoItem {
        VideoItem {
            picture: Picture::Footage(file.into()),
            source: VideoSource::Clip {
                file: file.into(),
                length: ms(1_000),
                stretched_from: None,
            },
            start: Duration::ZERO,
            at: Duration::ZERO,
            duration: ms(1_000),
            framing: Framing::default(),
            transition: None,
            motion: None,
        }
    }

    fn layout(timeline: &Timeline) -> Vec<(Option<String>, u64, u64, u64)> {
        timeline
            .video()
            .iter()
            .map(|item| {
                (
                    item.source.file().map(str::to_owned),
                    item.at.as_millis() as u64,
                    item.duration.as_millis() as u64,
                    item.start.as_millis() as u64,
                )
            })
            .collect()
    }

    #[test]
    fn a_bridge_covers_half_its_length_on_each_side_of_its_cut() {
        let timeline = cut();
        let span = timeline.bridge_span(2, 1).unwrap();
        // The cut at 4 s; 30 frames, 15 before it and 15 after.
        assert_eq!(
            span,
            BridgeSpan {
                cut: 2,
                from: 105,
                to: 135
            }
        );
        assert_eq!(span.start(), ms(3_500));
        assert_eq!(span.length(), ms(1_000));
        // An odd number of frames gives the extra one to the side after.
        let timeline = {
            let mut timeline = timeline;
            timeline.video[1].duration = frame_time(61);
            timeline.video[2].at = frame_time(91);
            timeline
        };
        let span = timeline.bridge_span(2, 1).unwrap();
        assert_eq!((span.from, span.to), (76, 106));
    }

    #[test]
    fn a_bridge_is_refused_where_there_is_no_cut_or_a_side_would_be_used_up() {
        let timeline = cut();
        assert_eq!(timeline.bridge_span(0, 1), Err(BridgeRefusal::NoCut));
        assert_eq!(timeline.bridge_span(4, 1), Err(BridgeRefusal::NoCut));
        assert_eq!(timeline.bridge_span(2, 0), Err(BridgeRefusal::TooLong));
        // 4 s takes the 2 s pictures whole: each must keep a frame.
        assert_eq!(timeline.bridge_span(2, 4), Err(BridgeRefusal::TooLong));
        // 3 s leaves each half a second.
        assert!(timeline.bridge_span(2, 3).is_ok());
        // The first cut: the picture before it starts at 0.
        assert_eq!(timeline.bridge_span(1, 4), Err(BridgeRefusal::TooLong));
        // A short picture after the cut.
        let mut short = cut();
        short.video[2].duration = ms(400);
        assert_eq!(short.bridge_span(2, 1), Err(BridgeRefusal::TooLong));
    }

    #[test]
    fn a_bridge_is_refused_over_a_picture_with_nothing_to_show() {
        let mut timeline = cut();
        timeline.video[1].source = VideoSource::Missing;
        let span = timeline.bridge_span(2, 1).unwrap();
        assert_eq!(timeline.bridge_ends(&span), Err(BridgeRefusal::NoPicture));
    }

    #[test]
    fn a_still_gives_its_image_for_both_ends() {
        let timeline = cut();
        let span = timeline.bridge_span(2, 1).unwrap();
        assert_eq!(
            timeline.bridge_ends(&span),
            Ok((
                FrameSource::Still {
                    file: "1.png".into()
                },
                FrameSource::Still {
                    file: "2.png".into()
                }
            ))
        );
    }

    #[test]
    fn a_clip_gives_the_frame_it_shows_beside_the_stretch_in_its_own_time() {
        let mut timeline = cut();
        // The picture before plays a clip from 1 s into it; the one after
        // from its start.
        timeline.video[1].source = VideoSource::Clip {
            file: "a.mp4".into(),
            length: ms(5_000),
            stretched_from: None,
        };
        timeline.video[1].start = ms(1_000);
        timeline.video[2].source = VideoSource::Clip {
            file: "b.mp4".into(),
            length: ms(5_000),
            stretched_from: None,
        };
        let span = timeline.bridge_span(2, 1).unwrap();
        let (first, last) = timeline.bridge_ends(&span).unwrap();
        // Frame 104 is at 3.4667 s: 1.4667 s into the picture, 2.4667 s
        // into its clip.
        assert_eq!(
            first,
            FrameSource::Video {
                file: "a.mp4".into(),
                at: frame_time(74),
            }
        );
        // Frame 135, half a second into the picture after.
        assert_eq!(
            last,
            FrameSource::Video {
                file: "b.mp4".into(),
                at: frame_time(15),
            }
        );
    }

    #[test]
    fn a_stretched_clip_gives_the_frame_of_its_file() {
        let mut timeline = cut();
        // 5 s of file played over 7.5 s: two thirds of the speed.
        timeline.video[2].source = VideoSource::Clip {
            file: "b.mp4".into(),
            length: ms(7_500),
            stretched_from: Some(ms(5_000)),
        };
        timeline.video[2].start = ms(1_500);
        let span = timeline.bridge_span(2, 1).unwrap();
        let (_, last) = timeline.bridge_ends(&span).unwrap();
        // 2 s into what it plays (1.5 + 0.5) is 1.333 s into its file.
        assert_eq!(
            last,
            FrameSource::Video {
                file: "b.mp4".into(),
                at: frame_time(40),
            }
        );
    }

    #[test]
    fn a_clip_cut_past_its_end_gives_its_last_frame() {
        let mut timeline = cut();
        timeline.video[1].source = VideoSource::Clip {
            file: "a.mp4".into(),
            length: ms(1_000),
            stretched_from: None,
        };
        let span = timeline.bridge_span(2, 1).unwrap();
        let (first, _) = timeline.bridge_ends(&span).unwrap();
        assert_eq!(
            first,
            FrameSource::Video {
                file: "a.mp4".into(),
                at: frame_time(29),
            }
        );
    }

    #[test]
    fn using_a_bridge_covers_its_stretch_in_one_edit_and_keeps_the_length() {
        let before = cut();
        let span = before.bridge_span(2, 1).unwrap();
        let (edit, placed) = before.put_bridge(&span, bridge_item("bridge.mp4")).unwrap();
        assert!(matches!(
            edit,
            Edit::Replace {
                index: 1,
                count: 2,
                ..
            }
        ));
        assert_eq!(placed, ItemRef::video(2));
        let mut history = History::default();
        let mut after = before.clone();
        history.apply(&mut after, &edit).unwrap();
        assert_eq!(
            layout(&after),
            vec![
                (Some("0.png".into()), 0, 2_000, 0),
                (Some("1.png".into()), 2_000, 1_500, 0),
                (Some("bridge.mp4".into()), 3_500, 1_000, 0),
                (Some("2.png".into()), 4_500, 1_500, 500),
                (Some("3.png".into()), 6_000, 2_000, 0),
            ]
        );
        assert_eq!(after.duration(), before.duration());
        assert!(history.undo(&mut after).unwrap());
        assert_eq!(after, before);
    }

    #[test]
    fn a_bridge_knows_its_cut_until_a_picture_around_it_changes() {
        let timeline = cut();
        let mark = BridgeCut::of(&timeline, 2).unwrap();
        assert_eq!(mark.find(&timeline), Some(2));
        assert_eq!(BridgeCut::of(&timeline, 0), None);
        // A cut before it moves both pictures: another cut.
        let mut trimmed = timeline.clone();
        trimmed
            .apply(&Edit::Trim {
                track: crate::Track::Video,
                index: 0,
                edge: crate::Edge::End,
                by: crate::Shift::earlier(ms(500)),
            })
            .unwrap();
        assert_eq!(mark.find(&trimmed), None);
        // The picture after it trimmed at its end.
        let mut shorter = timeline.clone();
        shorter.video[2].duration = ms(1_500);
        assert_eq!(mark.find(&shorter), None);
        // Gone.
        let mut removed = timeline.clone();
        removed
            .apply(&Edit::Delete {
                track: crate::Track::Video,
                index: 1,
            })
            .unwrap();
        assert_eq!(mark.find(&removed), None);
        // A transition on the cut changes neither picture.
        let mut eased = timeline.clone();
        eased.video[2].transition = Some(crate::Transition::default());
        assert_eq!(mark.find(&eased), Some(2));
        // And the edit is refused where the stretch is no longer over a
        // cut with room on both sides.
        let span = timeline.bridge_span(2, 1).unwrap();
        let mut used_up = timeline.clone();
        used_up.video[2].duration = ms(400);
        assert_eq!(
            used_up.put_bridge(&span, bridge_item("bridge.mp4")),
            Err(EditError::OutsideItem)
        );
        assert_eq!(
            timeline.put_bridge(&BridgeSpan { cut: 3, ..span }, bridge_item("bridge.mp4")),
            Err(EditError::OutsideItem)
        );
    }

    #[test]
    fn the_prompt_is_the_fixed_one_then_the_user_s_note() {
        assert_eq!(bridge_prompt("  "), BRIDGE_PROMPT);
        assert_eq!(
            bridge_prompt(" a slow pan "),
            format!("{BRIDGE_PROMPT} a slow pan")
        );
    }
}
