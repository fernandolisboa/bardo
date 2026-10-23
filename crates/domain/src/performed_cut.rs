//! Performed clips on the cut (#193, docs/spec/performance-capture.md):
//! each performed clip belongs to its take's stretch of the narration, so
//! the persona speaks when the voice does.
//!
//! - **Put on the cut.** A performed clip goes on the video track where the
//!   cut plays its take's start, covering what lay there for its real
//!   length or its take's, whichever is shorter. One [`Edit::Replace`],
//!   undone at once.
//! - **Sync.** Every frame of a performed clip belongs to a moment of the
//!   narration: its take's start plus the frame's time in the clip.
//!   Trimming and splitting keep that (they move where in the clip a piece
//!   starts), and so does a scene picture covering part of it. A reorder or
//!   a removal on the magnetic track can move a piece off its moment: the
//!   cut says how far ([`PerformedSync`]), and putting it back in sync
//!   lengthens or shortens the item before it.
//! - **Rough cut.** A new cut can start with each take's performed clip on
//!   its stretch, scene pictures everywhere else.

use std::time::Duration;

use crate::{
    Edit, EditError, Framing, ItemRef, Picture, ScenePlan, Shift, Timeline, Track, VideoItem,
    VideoSource, frame_at, frame_time, min_length, nearest_frame,
};

/// Where a performed clip belongs: the take it performs, as a stretch of
/// the narration file. Its first frame belongs to the take's start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformedPlace {
    /// The clip's file in the project folder.
    pub file: String,
    /// The clip's real length, which may differ from its take's.
    pub length: Duration,
    /// The take's stretch of the narration file.
    pub start: Duration,
    pub end: Duration,
    /// How it starts filling a 9:16 frame: on the performer's face, when
    /// its take model keeps the take's framing; else centered.
    pub framing: Framing,
}

impl PerformedPlace {
    /// How long it covers on the cut: its real length or its take's,
    /// whichever is shorter, in whole frames.
    pub fn covers(&self) -> Duration {
        frame_time(frame_at(self.length)).min(frame_time(nearest_frame(
            self.end.saturating_sub(self.start),
        )))
    }

    /// The clip whole, as a video item.
    fn item(&self, duration: Duration) -> VideoItem {
        VideoItem {
            picture: Picture::Footage(self.file.clone()),
            source: VideoSource::Clip {
                file: self.file.clone(),
                length: self.length,
            },
            start: Duration::ZERO,
            at: Duration::ZERO,
            duration,
            framing: self.framing,
            transition: None,
            motion: None,
        }
    }
}

/// Whether a performed piece on the cut plays with the narration it
/// belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformedSync {
    /// Its first frame is on the frame where the cut plays its moment.
    InSync,
    /// It plays this much later (positive) or earlier (negative) than its
    /// narration, in whole frames.
    Off(Shift),
    /// The cut does not play the narration its first frame belongs to.
    Unheard,
}

/// The scene a rough cut shows at `time` of the narration: the one whose
/// stretch holds it, else the last one starting before it.
pub fn scene_at_narration(plan: &ScenePlan, time: Duration) -> Option<usize> {
    let scenes = plan.scenes();
    scenes
        .iter()
        .position(|scene| scene.start <= time && time < scene.end)
        .or_else(|| scenes.iter().rposition(|scene| scene.start <= time))
        .or_else(|| (!scenes.is_empty()).then_some(0))
}

impl VideoItem {
    /// Scene `index` of `plan`, as a rough cut shows it: its clip or its
    /// still, from its start, centered, without motion.
    pub fn of_scene(plan: &ScenePlan, index: usize, duration: Duration) -> Option<VideoItem> {
        let scene = plan.scenes().get(index)?;
        Some(VideoItem {
            picture: Picture::Scene(index),
            source: VideoSource::of(scene),
            start: Duration::ZERO,
            at: Duration::ZERO,
            duration,
            framing: Framing::default(),
            transition: None,
            motion: None,
        })
    }
}

impl Timeline {
    /// This timeline knowing where the project's performed clips belong.
    pub fn with_performed(mut self, places: Vec<PerformedPlace>) -> Timeline {
        self.performed = places;
        self
    }

    /// Where the project's performed clips belong.
    pub fn performed(&self) -> &[PerformedPlace] {
        &self.performed
    }

    /// Where the clip video item `index` plays belongs, when it is a
    /// performed clip of the narration.
    pub fn performed_at(&self, index: usize) -> Option<&PerformedPlace> {
        let item = self.video.get(index)?;
        let Picture::Footage(file) = &item.picture else {
            return None;
        };
        self.performed.iter().find(|place| place.file == *file)
    }

    /// Whether performed piece `index` plays with its narration; `None`
    /// for an item that is not a performed clip of the narration.
    pub fn sync(&self, index: usize) -> Option<PerformedSync> {
        let place = self.performed_at(index)?;
        let item = &self.video[index];
        let Some(heard) = self.on_narration(place.start + item.start) else {
            return Some(PerformedSync::Unheard);
        };
        let target = frame_time(nearest_frame(heard));
        Some(if target == item.at {
            PerformedSync::InSync
        } else {
            PerformedSync::Off(Shift::between(target, item.at))
        })
    }

    /// The edit that puts `place`'s clip on the video track where the cut
    /// plays its take's start, covering what lies there for
    /// [`PerformedPlace::covers`], and the item it becomes. Refused when
    /// the cut has cut that moment of the narration away.
    pub fn put_performed(&self, place: &PerformedPlace) -> Result<(Edit, ItemRef), EditError> {
        let heard = self.on_narration(place.start).ok_or(EditError::NotHeard)?;
        let length = place.covers();
        if length < min_length() {
            return Err(EditError::TooShort);
        }
        let from = frame_time(nearest_frame(heard));
        self.overwrite(from, from + length, place.item(length))
    }

    /// The edit that covers the video track from `from` for `length` with
    /// `item`, cut short where the item under `from` ends: a scene picture
    /// over part of a performed clip (B-roll), and the item it becomes.
    /// What the cover leaves of the item plays on as it did.
    pub fn cover(
        &self,
        from: Duration,
        length: Duration,
        item: VideoItem,
    ) -> Result<(Edit, ItemRef), EditError> {
        let under = self.video_at(from).ok_or(EditError::NoSuchItem)?;
        let to = (from + length).min(self.video[under].end());
        self.overwrite(from, to, item)
    }

    /// The edit that puts `item` from `from` to `to` on the video track,
    /// cutting the items at both ends and replacing what lies between; the
    /// track's last item runs on to `from` when the track ends before it.
    fn overwrite(
        &self,
        from: Duration,
        to: Duration,
        mut item: VideoItem,
    ) -> Result<(Edit, ItemRef), EditError> {
        if to < from + min_length() {
            return Err(EditError::OutsideItem);
        }
        if self.video.is_empty() {
            return Err(EditError::NoSuchItem);
        }
        item.duration = to - from;
        item.transition = None;
        let first = self.video_at(from).unwrap_or(self.video.len() - 1);
        let last = self
            .video
            .iter()
            .rposition(|video| video.at < to)
            .unwrap_or(first)
            .max(first);
        let mut items = Vec::with_capacity(3);
        let head = &self.video[first];
        // What the first item keeps before the cover (a last item running
        // on to it when the track ends first).
        if from > head.at {
            let mut kept = head.clone();
            kept.duration = from - head.at;
            if kept.duration < min_length() {
                return Err(EditError::OutsideItem);
            }
            items.push(kept);
        }
        let index = first + items.len();
        items.push(item);
        let tail = &self.video[last];
        if to < tail.end() {
            // As a split leaves it: further into its source, no
            // transition into it.
            let mut kept = tail.clone();
            let cut = to.saturating_sub(tail.at);
            kept.start += cut;
            kept.duration = tail.end() - to;
            kept.transition = None;
            if kept.duration < min_length() {
                return Err(EditError::OutsideItem);
            }
            items.push(kept);
        }
        Ok((
            Edit::Replace {
                index: first,
                count: last - first + 1,
                items,
            },
            ItemRef::video(index),
        ))
    }

    /// The edit that puts performed piece `index` back in sync: the item
    /// before it gets longer or shorter by how far off it is, so it (and
    /// the cut after it) moves to its narration. Refused when it is in
    /// sync (`NoChange`), when its narration is not heard (`NotHeard`), and
    /// when there is no item before it or that item would end up shorter
    /// than a frame (`OutsideItem`).
    pub fn back_in_sync(&self, index: usize) -> Result<Edit, EditError> {
        let off = match self.sync(index).ok_or(EditError::NoSuchItem)? {
            PerformedSync::InSync => return Err(EditError::NoChange),
            PerformedSync::Unheard => return Err(EditError::NotHeard),
            PerformedSync::Off(off) => off,
        };
        let before = index.checked_sub(1).ok_or(EditError::OutsideItem)?;
        let shortened = (-off)
            .move_time(self.video[before].duration)
            .ok_or(EditError::OutsideItem)?;
        if shortened < min_length() {
            return Err(EditError::OutsideItem);
        }
        Ok(Edit::Trim {
            track: Track::Video,
            index: before,
            edge: crate::Edge::End,
            by: -off,
        })
    }

    /// This timeline with each of `places`' clips put on the cut, in turn
    /// (a rough cut's performed takes). A clip the cut cannot place stays
    /// off it.
    pub fn with_performed_takes(mut self, places: &[PerformedPlace]) -> Timeline {
        for place in places {
            if let Ok((edit, _)) = self.put_performed(place) {
                let _ = self.apply(&edit);
            }
        }
        self
    }

    /// The scenes a rough cut shows where the cut plays performed clips:
    /// each scene whose stretch of the narration a performed piece covers.
    pub fn scenes_under_performed(&self, plan: &ScenePlan) -> Vec<usize> {
        let mut scenes: Vec<usize> = Vec::new();
        for index in 0..self.video.len() {
            let Some(place) = self.performed_at(index) else {
                continue;
            };
            let item = &self.video[index];
            let start = place.start + item.start;
            let end = start + item.duration;
            for (scene, stretch) in plan.scenes().iter().enumerate() {
                if stretch.start < end && start < stretch.end && !scenes.contains(&scene) {
                    scenes.push(scene);
                }
            }
        }
        scenes.sort_unstable();
        scenes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::{image, ms, narration, plan, scene};
    use crate::{AspectRatio, CropPosition, History};

    /// Four stills over an 8 s narration, 2 s each.
    fn cut() -> (ScenePlan, Timeline) {
        let scenes = (0..4)
            .map(|n| {
                let mut s = scene(n * 2_000, (n + 1) * 2_000);
                s.image = Some(image(&format!("{n}.png")));
                s
            })
            .collect();
        let plan = plan(scenes);
        let timeline = Timeline::rough_cut(&plan, &narration(8_000), AspectRatio::Landscape);
        (plan, timeline)
    }

    /// A performed clip of the take from `start` to `end` ms, `length` ms
    /// long.
    fn take(file: &str, start: u64, end: u64, length: u64) -> PerformedPlace {
        PerformedPlace {
            file: file.into(),
            length: ms(length),
            start: ms(start),
            end: ms(end),
            framing: Framing::default(),
        }
    }

    fn put(timeline: &Timeline, place: &PerformedPlace) -> Timeline {
        let mut timeline = timeline.clone().with_performed(vec![place.clone()]);
        let (edit, _) = timeline.put_performed(place).unwrap();
        timeline.apply(&edit).unwrap();
        timeline
    }

    fn layout(timeline: &Timeline) -> Vec<(Picture, u64, u64)> {
        timeline
            .video()
            .iter()
            .map(|item| {
                (
                    item.picture.clone(),
                    item.at.as_millis() as u64,
                    item.duration.as_millis() as u64,
                )
            })
            .collect()
    }

    fn footage(file: &str) -> Picture {
        Picture::Footage(file.into())
    }

    #[test]
    fn a_performed_clip_goes_on_its_take_s_stretch_in_one_undoable_edit() {
        let (_, before) = cut();
        let place = take("p1.mp4", 1_000, 5_000, 4_000);
        let before = before.with_performed(vec![place.clone()]);
        let mut timeline = before.clone();
        let mut history = History::default();
        let (edit, placed) = timeline.put_performed(&place).unwrap();
        history.apply(&mut timeline, &edit).unwrap();
        assert_eq!(placed, ItemRef::video(1));
        assert_eq!(
            layout(&timeline),
            [
                (Picture::Scene(0), 0, 1_000),
                (footage("p1.mp4"), 1_000, 4_000),
                (Picture::Scene(2), 5_000, 1_000),
                (Picture::Scene(3), 6_000, 2_000),
            ]
        );
        assert_eq!(timeline.sync(1), Some(PerformedSync::InSync));
        assert_eq!(timeline.video_end(), ms(8_000), "nothing after it moved");
        history.undo(&mut timeline).unwrap();
        assert_eq!(timeline, before);
    }

    #[test]
    fn a_shorter_clip_covers_its_own_length_and_the_scene_fills_the_rest() {
        let (_, timeline) = cut();
        // One second shorter than its take.
        let timeline = put(&timeline, &take("p1.mp4", 2_000, 6_000, 3_000));
        assert_eq!(
            layout(&timeline),
            [
                (Picture::Scene(0), 0, 2_000),
                (footage("p1.mp4"), 2_000, 3_000),
                (Picture::Scene(2), 5_000, 1_000),
                (Picture::Scene(3), 6_000, 2_000),
            ]
        );
        assert_eq!(timeline.sync(1), Some(PerformedSync::InSync));
        // The scene's piece is its own still, as a split leaves it.
        assert_eq!(
            timeline.video()[2].source,
            VideoSource::Still("2.png".into())
        );
    }

    #[test]
    fn a_longer_clip_is_cut_at_its_take_s_end() {
        let (_, timeline) = cut();
        let timeline = put(&timeline, &take("p1.mp4", 2_000, 4_000, 2_600));
        assert_eq!(timeline.video()[1].duration, ms(2_000));
        assert_eq!(timeline.video()[2].at, ms(4_000));
        // It can be lengthened by hand, from its own frames.
        let VideoSource::Clip { length, .. } = &timeline.video()[1].source else {
            panic!("a clip");
        };
        assert_eq!(*length, ms(2_600));
    }

    #[test]
    fn a_take_off_the_frames_lands_on_the_nearest_frame() {
        let (_, timeline) = cut();
        let timeline = put(&timeline, &take("p1.mp4", 1_010, 3_000, 5_000));
        assert_eq!(timeline.video()[1].at, frame_time(nearest_frame(ms(1_010))));
        assert_eq!(timeline.sync(1), Some(PerformedSync::InSync));
    }

    #[test]
    fn a_clip_whose_narration_was_cut_away_is_refused() {
        let (_, mut timeline) = cut();
        // Cut the narration's first three seconds away.
        timeline
            .apply(&Edit::Trim {
                track: Track::Narration,
                index: 0,
                edge: crate::Edge::Start,
                by: Shift::later(ms(3_000)),
            })
            .unwrap();
        let before = timeline.clone();
        assert_eq!(
            timeline.put_performed(&take("p1.mp4", 1_000, 2_500, 1_500)),
            Err(EditError::NotHeard)
        );
        assert_eq!(timeline, before);
    }

    #[test]
    fn covering_part_of_a_take_with_a_scene_keeps_the_rest_in_sync_and_undoes_whole() {
        let (plan, timeline) = cut();
        let before = put(&timeline, &take("p1.mp4", 1_000, 5_000, 4_000));
        let mut timeline = before.clone();
        let mut history = History::default();
        let scene = scene_at_narration(&plan, ms(2_500)).unwrap();
        assert_eq!(scene, 1);
        let picture = VideoItem::of_scene(&plan, scene, Duration::ZERO).unwrap();
        let (edit, covered) = timeline.cover(ms(2_000), ms(2_000), picture).unwrap();
        history.apply(&mut timeline, &edit).unwrap();
        assert_eq!(covered, ItemRef::video(2));
        assert_eq!(
            layout(&timeline),
            [
                (Picture::Scene(0), 0, 1_000),
                (footage("p1.mp4"), 1_000, 1_000),
                (Picture::Scene(1), 2_000, 2_000),
                (footage("p1.mp4"), 4_000, 1_000),
                (Picture::Scene(2), 5_000, 1_000),
                (Picture::Scene(3), 6_000, 2_000),
            ]
        );
        assert_eq!(timeline.sync(1), Some(PerformedSync::InSync));
        assert_eq!(timeline.sync(3), Some(PerformedSync::InSync));
        assert_eq!(timeline.video()[3].start, ms(3_000));
        history.undo(&mut timeline).unwrap();
        assert_eq!(timeline, before, "the take comes back whole");
        assert_eq!(timeline.video().len(), 4);
    }

    #[test]
    fn a_cover_stops_at_the_end_of_the_piece_it_starts_on() {
        let (plan, timeline) = cut();
        let mut timeline = put(&timeline, &take("p1.mp4", 1_000, 5_000, 4_000));
        let picture = VideoItem::of_scene(&plan, 1, Duration::ZERO).unwrap();
        let (edit, _) = timeline.cover(ms(4_000), ms(3_000), picture).unwrap();
        timeline.apply(&edit).unwrap();
        assert_eq!(
            layout(&timeline)[1..3],
            [
                (footage("p1.mp4"), 1_000, 3_000),
                (Picture::Scene(1), 4_000, 1_000),
            ]
        );
        assert_eq!(timeline.video()[3].at, ms(5_000));
    }

    #[test]
    fn trimming_and_splitting_keep_where_each_frame_belongs() {
        let (_, timeline) = cut();
        let mut timeline = put(&timeline, &take("p1.mp4", 1_000, 5_000, 4_000));
        timeline
            .apply(&Edit::Split {
                track: Track::Video,
                index: 1,
                at: ms(3_000),
            })
            .unwrap();
        assert_eq!(timeline.sync(1), Some(PerformedSync::InSync));
        assert_eq!(timeline.sync(2), Some(PerformedSync::InSync));
        // Cutting half a second off its head plays it half a second early.
        timeline
            .apply(&Edit::Trim {
                track: Track::Video,
                index: 2,
                edge: crate::Edge::Start,
                by: Shift::later(ms(500)),
            })
            .unwrap();
        assert_eq!(
            timeline.sync(2),
            Some(PerformedSync::Off(Shift::earlier(ms(500))))
        );
    }

    #[test]
    fn a_removal_before_it_shows_how_far_off_it_is_and_puts_it_back() {
        let (_, timeline) = cut();
        let mut timeline = put(&timeline, &take("p1.mp4", 4_000, 6_000, 2_000));
        // Scene 0 goes: everything after it moves two seconds earlier.
        timeline
            .apply(&Edit::Delete {
                track: Track::Video,
                index: 0,
            })
            .unwrap();
        assert_eq!(
            timeline.sync(1),
            Some(PerformedSync::Off(Shift::earlier(ms(2_000))))
        );
        let before = timeline.clone();
        let edit = timeline.back_in_sync(1).unwrap();
        let mut history = History::default();
        history.apply(&mut timeline, &edit).unwrap();
        assert_eq!(timeline.sync(1), Some(PerformedSync::InSync));
        assert_eq!(timeline.video()[0].duration, ms(4_000));
        assert_eq!(timeline.video()[1].at, ms(4_000));
        history.undo(&mut timeline).unwrap();
        assert_eq!(timeline, before);
    }

    #[test]
    fn a_late_piece_goes_back_by_shortening_the_item_before() {
        let (_, timeline) = cut();
        let mut timeline = put(&timeline, &take("p1.mp4", 2_000, 4_000, 2_000));
        // Scene 0 runs half a second longer: half a second late.
        timeline
            .apply(&Edit::Trim {
                track: Track::Video,
                index: 0,
                edge: crate::Edge::End,
                by: Shift::later(ms(500)),
            })
            .unwrap();
        assert_eq!(
            timeline.sync(1),
            Some(PerformedSync::Off(Shift::later(ms(500))))
        );
        let edit = timeline.back_in_sync(1).unwrap();
        timeline.apply(&edit).unwrap();
        assert_eq!(timeline.sync(1), Some(PerformedSync::InSync));
        assert_eq!(timeline.video()[0].duration, ms(2_000));
    }

    #[test]
    fn a_piece_too_late_for_the_item_before_stays_off() {
        let (_, timeline) = cut();
        let mut timeline = put(&timeline, &take("p1.mp4", 2_000, 4_000, 2_000));
        // Scene 2 goes before it: two seconds late, and the item before
        // is exactly two seconds, which shortening would leave empty.
        timeline.apply(&Edit::Reorder { from: 2, to: 1 }).unwrap();
        assert_eq!(
            timeline.sync(2),
            Some(PerformedSync::Off(Shift::later(ms(2_000))))
        );
        assert_eq!(timeline.back_in_sync(2), Err(EditError::OutsideItem));
    }

    #[test]
    fn nothing_to_put_back_first_in_line_or_in_sync() {
        let (_, timeline) = cut();
        let mut timeline = put(&timeline, &take("p1.mp4", 2_000, 4_000, 2_000));
        assert_eq!(timeline.back_in_sync(1), Err(EditError::NoChange));
        timeline.apply(&Edit::Reorder { from: 1, to: 0 }).unwrap();
        assert_eq!(
            timeline.sync(0),
            Some(PerformedSync::Off(Shift::earlier(ms(2_000))))
        );
        assert_eq!(timeline.back_in_sync(0), Err(EditError::OutsideItem));
        assert_eq!(timeline.sync(1), None, "a scene has no sync");
    }

    #[test]
    fn a_piece_whose_narration_is_cut_away_is_unheard() {
        let (_, timeline) = cut();
        let mut timeline = put(&timeline, &take("p1.mp4", 2_000, 4_000, 2_000));
        timeline
            .apply(&Edit::Trim {
                track: Track::Narration,
                index: 0,
                edge: crate::Edge::Start,
                by: Shift::later(ms(3_000)),
            })
            .unwrap();
        assert_eq!(timeline.sync(1), Some(PerformedSync::Unheard));
        assert_eq!(timeline.back_in_sync(1), Err(EditError::NotHeard));
    }

    #[test]
    fn footage_of_another_narration_is_plain_footage() {
        let (_, timeline) = cut();
        let mut timeline = put(&timeline, &take("p1.mp4", 2_000, 4_000, 2_000));
        timeline = timeline.with_performed(Vec::new());
        assert_eq!(timeline.performed_at(1), None);
        assert_eq!(timeline.sync(1), None);
    }

    #[test]
    fn a_rough_cut_plays_each_take_and_scenes_elsewhere() {
        let (plan, timeline) = cut();
        let takes = [
            take("p1.mp4", 0, 2_500, 2_500),
            take("p2.mp4", 2_500, 5_000, 2_500),
            // A second shorter than its take.
            take("p3.mp4", 6_000, 8_000, 1_000),
        ];
        let timeline = timeline
            .with_performed(takes.to_vec())
            .with_performed_takes(&takes);
        assert_eq!(
            layout(&timeline),
            [
                (footage("p1.mp4"), 0, 2_500),
                (footage("p2.mp4"), 2_500, 2_500),
                (Picture::Scene(2), 5_000, 1_000),
                (footage("p3.mp4"), 6_000, 1_000),
                (Picture::Scene(3), 7_000, 1_000),
            ]
        );
        for index in [0, 1, 3] {
            assert_eq!(timeline.sync(index), Some(PerformedSync::InSync), "{index}");
        }
        assert_eq!(timeline.scenes_under_performed(&plan), [0, 1, 2, 3]);
    }

    #[test]
    fn a_clip_starts_with_the_framing_it_was_given() {
        let (_, timeline) = cut();
        let mut place = take("p1.mp4", 2_000, 4_000, 2_000);
        place.framing = Framing::Crop(CropPosition::new(200, 500));
        let timeline = put(&timeline, &place);
        assert_eq!(timeline.video()[1].framing, place.framing);
    }

    #[test]
    fn a_track_shorter_than_the_take_runs_its_last_item_on_to_it() {
        let (_, mut timeline) = cut();
        for _ in 0..2 {
            timeline
                .apply(&Edit::Delete {
                    track: Track::Video,
                    index: 2,
                })
                .unwrap();
        }
        let timeline = put(&timeline, &take("p1.mp4", 5_000, 7_000, 2_000));
        assert_eq!(
            layout(&timeline),
            [
                (Picture::Scene(0), 0, 2_000),
                (Picture::Scene(1), 2_000, 3_000),
                (footage("p1.mp4"), 5_000, 2_000),
            ]
        );
        assert_eq!(timeline.sync(2), Some(PerformedSync::InSync));
    }

    #[test]
    fn the_rough_cut_s_scene_at_a_moment() {
        let (plan, _) = cut();
        assert_eq!(scene_at_narration(&plan, ms(0)), Some(0));
        assert_eq!(scene_at_narration(&plan, ms(3_999)), Some(1));
        assert_eq!(
            scene_at_narration(&plan, ms(9_000)),
            Some(3),
            "past the last"
        );
    }
}
