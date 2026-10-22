//! Retention over the cut (#183, ADR-0022): a published video's audience
//! retention curve (`crate::RetentionCurve`, #79) read against the cut that
//! made it, so the creator sees which scene and which sentence lost
//! viewers.
//!
//! - A **drop** is a run of steep steps of the curve: steps between the
//!   report's points (each a hundredth of the video) that lose at least
//!   [`STEEP_TIMES`] the curve's average loss per step and at least
//!   [`MIN_STEP_LOSS`]. The thresholds follow the video's own curve, so a
//!   Short and a long video are judged against themselves. The
//!   [`MAX_DROPS`] largest are kept, in time order; rises (rewatched
//!   parts) are not drops.
//! - The curve's times are shares of the published file, so they line up
//!   with the timeline only while the cut's **timing** is the one rendered
//!   ([`cut_timing`]): where each clip and narration piece sits and what
//!   each clip shows. Levels, framing, captions and motion leave it as it
//!   is. A render records it, and Bardo keeps it per rendered file
//!   ([`RenderedCut`]) while the project's render, a post or an export
//!   points to that file.
//! - Drops are worked out when shown and never stored: nothing new is kept
//!   of what YouTube reports (ADR-0012).

use std::time::Duration;

use crate::{AspectRatio, Picture, RenderId, RetentionCurve, Sentence, Share, Timeline};

/// The most drops marked on a curve.
pub const MAX_DROPS: usize = 5;

/// How many times the curve's average loss per step a step must lose to be
/// steep.
pub const STEEP_TIMES: u32 = 2;

/// The least a step must lose to be steep: half a percentage point of the
/// audience.
pub const MIN_STEP_LOSS: Share = Share::from_ten_thousandths(50);

/// A steep fall of a retention curve, from one of its points to a later
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionDrop {
    /// Where it starts and ends in the video, as shares of its length.
    pub from: Share,
    pub to: Share,
    /// The share still watching at its start and at its end.
    pub before: Share,
    pub after: Share,
}

impl RetentionDrop {
    /// The share of the audience it loses.
    pub fn lost(&self) -> Share {
        Share::from_ten_thousandths(
            self.before
                .ten_thousandths()
                .saturating_sub(self.after.ten_thousandths()),
        )
    }

    /// Halfway through it: where its scene and sentence are read.
    pub fn middle(&self) -> Share {
        Share::from_ten_thousandths((self.from.ten_thousandths() + self.to.ten_thousandths()) / 2)
    }
}

/// The moment `share` of a video of `length`, to the nanosecond.
pub fn time_at(share: Share, length: Duration) -> Duration {
    let nanos = length.as_nanos() * u128::from(share.ten_thousandths()) / 10_000;
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// The steepest falls of `curve`: at most [`MAX_DROPS`], the largest by the
/// share lost, in time order.
pub fn retention_drops(curve: &RetentionCurve) -> Vec<RetentionDrop> {
    let points = curve.points();
    if points.len() < 2 {
        return Vec::new();
    }
    let watch = |index: usize| points[index].watch.ten_thousandths();
    let steps = (points.len() - 1) as u32;
    let average = watch(0).saturating_sub(watch(points.len() - 1)) / steps;
    let floor = (average * STEEP_TIMES).max(MIN_STEP_LOSS.ten_thousandths());
    let steep = |index: usize| watch(index).saturating_sub(watch(index + 1)) >= floor;
    let mut drops = Vec::new();
    let mut index = 0;
    while index + 1 < points.len() {
        if !steep(index) {
            index += 1;
            continue;
        }
        let start = index;
        while index + 1 < points.len() && steep(index) {
            index += 1;
        }
        drops.push(RetentionDrop {
            from: points[start].elapsed,
            to: points[index].elapsed,
            before: points[start].watch,
            after: points[index].watch,
        });
    }
    drops.sort_by(|a, b| b.lost().cmp(&a.lost()).then(a.from.cmp(&b.from)));
    drops.truncate(MAX_DROPS);
    drops.sort_by_key(|drop| drop.from);
    drops
}

/// A drop where the cut plays it: the scene shown and the sentence spoken
/// in its middle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedDrop {
    pub drop: RetentionDrop,
    /// Where it starts and ends on the timeline.
    pub start: Duration,
    pub end: Duration,
    /// The scene shown, by index in the scene plan; `None` over footage or
    /// past the video's end.
    pub scene: Option<usize>,
    /// The sentence spoken, by index in `crate::sentences`, or the last one
    /// said before it when a pause plays there; `None` with no narration
    /// playing.
    pub sentence: Option<usize>,
}

/// Where `drops` of a video of `length` fall on `timeline`, the cut it was
/// rendered from, with `sentences` of the narration it plays.
pub fn place_drops(
    drops: &[RetentionDrop],
    length: Duration,
    timeline: &Timeline,
    sentences: &[Sentence],
) -> Vec<PlacedDrop> {
    drops
        .iter()
        .map(|drop| {
            let middle = time_at(drop.middle(), length);
            let scene = timeline
                .video_at(middle)
                .and_then(|index| timeline.video()[index].scene());
            let sentence = timeline.narration_at(middle).and_then(|index| {
                let item = &timeline.narration()[index];
                let source = item.start + (middle - item.at);
                sentences
                    .iter()
                    .rposition(|sentence| sentence.start <= source)
            });
            PlacedDrop {
                drop: *drop,
                start: time_at(drop.from, length),
                end: time_at(drop.to, length),
                scene,
                sentence,
            }
        })
        .collect()
}

/// A fingerprint of the cut's timing (FNV-1a): what the video shows when,
/// and which part of which narration file plays when. Pieces that play on
/// from each other count as one, so a split alone leaves it as it is.
/// Equal timings, equal fingerprints, on any machine.
pub fn cut_timing(timeline: &Timeline) -> String {
    let mut pictures: Vec<(Duration, Duration, &Picture)> = Vec::new();
    for item in &timeline.video {
        match pictures.last_mut() {
            Some((_, end, picture)) if *picture == &item.picture && *end == item.at => {
                *end = item.end();
            }
            _ => pictures.push((item.at, item.end(), &item.picture)),
        }
    }
    let mut speech: Vec<(Duration, Duration, Duration, &str)> = Vec::new();
    for item in &timeline.narration {
        match speech.last_mut() {
            Some((at, end, start, file))
                if *file == item.file && *end == item.at && *start + (*end - *at) == item.start =>
            {
                *end = item.end();
            }
            _ => speech.push((item.at, item.end(), item.start, &item.file)),
        }
    }
    let mut text = String::new();
    for (at, end, picture) in pictures {
        let picture = match picture {
            Picture::Scene(index) => format!("s{index}"),
            Picture::Footage(file) => format!("f{file}"),
        };
        text.push_str(&format!(
            "v {} {} {picture}\n",
            at.as_nanos(),
            end.as_nanos()
        ));
    }
    for (at, end, start, file) in speech {
        text.push_str(&format!(
            "n {} {} {} {file}\n",
            at.as_nanos(),
            end.as_nanos(),
            start.as_nanos()
        ));
    }
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// What a rendered file was cut from, kept while the project's render, a
/// post or an export points to it, so a later render of the same account
/// does not lose what a post was made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedCut {
    pub render: RenderId,
    /// How long the file runs.
    pub length: Duration,
    /// The shape it was rendered in.
    pub aspect: AspectRatio,
    /// The render plan's fingerprint (`crate::Render::cut`).
    pub cut: String,
    /// [`cut_timing`] of the timeline it was rendered from; `None` for a
    /// file rendered before the timing was recorded.
    pub timing: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::{ms, narration, plan, scene};
    use crate::{Edit, RetentionPoint, Track};

    /// A curve of `watch` percentages, one point per hundredth from 0.01.
    fn curve(watch: &[f64]) -> RetentionCurve {
        RetentionCurve::of(
            watch
                .iter()
                .enumerate()
                .map(|(n, watch)| RetentionPoint {
                    elapsed: Share::from_ten_thousandths((n as u32 + 1) * 100),
                    watch: Share::of_percent(*watch),
                    relative: None,
                })
                .collect(),
        )
    }

    fn share(percent: f64) -> Share {
        Share::of_percent(percent)
    }

    /// A gentle decline of half a point per step from 90%, with `cliffs`
    /// (step index, extra points lost) cut into it.
    fn declining(cliffs: &[(usize, f64)]) -> Vec<f64> {
        let mut watch = Vec::new();
        let mut at = 90.0;
        for step in 0..100 {
            watch.push(at);
            at -= 0.5;
            if let Some((_, lost)) = cliffs.iter().find(|(index, _)| *index == step) {
                at -= lost;
            }
        }
        watch
    }

    #[test]
    fn a_steady_decline_has_no_drops() {
        assert!(retention_drops(&curve(&declining(&[]))).is_empty());
        assert!(retention_drops(&curve(&[80.0])).is_empty());
        assert!(retention_drops(&RetentionCurve::default()).is_empty());
    }

    #[test]
    fn a_step_steeper_than_the_curve_is_a_drop() {
        let drops = retention_drops(&curve(&declining(&[(20, 6.0)])));
        assert_eq!(drops.len(), 1);
        let drop = drops[0];
        assert_eq!((drop.from, drop.to), (share(21.0), share(22.0)));
        assert_eq!(drop.before, share(80.0));
        assert_eq!(drop.after, share(73.5));
        assert_eq!(drop.lost(), share(6.5));
        assert_eq!(drop.middle(), share(21.5));
    }

    #[test]
    fn steep_steps_next_to_each_other_make_one_drop() {
        let drops = retention_drops(&curve(&declining(&[(40, 3.0), (41, 4.0), (42, 2.0)])));
        assert_eq!(drops.len(), 1);
        assert_eq!((drops[0].from, drops[0].to), (share(41.0), share(44.0)));
        assert_eq!(drops[0].lost(), share(10.5));
    }

    #[test]
    fn only_the_largest_drops_are_kept_in_time_order() {
        let cliffs: Vec<(usize, f64)> = [(5, 2.0), (15, 9.0), (25, 3.0), (35, 8.0), (45, 1.5)]
            .into_iter()
            .chain([(55, 7.0), (65, 4.0), (75, 1.0)])
            .collect();
        let drops = retention_drops(&curve(&declining(&cliffs)));
        assert_eq!(drops.len(), MAX_DROPS);
        let starts: Vec<Share> = drops.iter().map(|drop| drop.from).collect();
        assert_eq!(
            starts,
            [16.0, 26.0, 36.0, 56.0, 66.0].map(share),
            "the five largest, earliest first"
        );
    }

    #[test]
    fn the_threshold_follows_the_curves_own_slope() {
        // Losing a point a step on average, under two points is not steep.
        let steep: Vec<f64> = (0..100)
            .map(|n| 100.0 - n as f64 - if n > 50 { 0.9 } else { 0.0 })
            .collect();
        assert!(retention_drops(&curve(&steep)).is_empty());
        // Nearly flat, a small step still has to lose half a point.
        let flat: Vec<f64> = (0..100).map(|n| if n > 60 { 69.6 } else { 70.0 }).collect();
        assert!(retention_drops(&curve(&flat)).is_empty());
        let flat: Vec<f64> = (0..100).map(|n| if n > 60 { 69.5 } else { 70.0 }).collect();
        assert_eq!(retention_drops(&curve(&flat)).len(), 1);
    }

    #[test]
    fn rises_are_not_drops() {
        let mut watch = declining(&[]);
        watch[30] += 10.0;
        // The rise itself is not marked; the fall back from it is.
        let drops = retention_drops(&curve(&watch));
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].from, share(31.0));
    }

    #[test]
    fn times_are_shares_of_the_video() {
        let length = Duration::from_secs(200);
        assert_eq!(time_at(share(21.0), length), Duration::from_secs(42));
        assert_eq!(time_at(Share::ONE, length), length);
        assert_eq!(time_at(Share::default(), length), Duration::ZERO);
    }

    fn sentence(start: u64, end: u64) -> Sentence {
        Sentence {
            words: 0..1,
            text: 0..1,
            start: ms(start),
            end: ms(end),
        }
    }

    fn drop_at(from: f64, to: f64) -> RetentionDrop {
        RetentionDrop {
            from: share(from),
            to: share(to),
            before: share(70.0),
            after: share(60.0),
        }
    }

    #[test]
    fn a_drop_is_placed_at_the_scene_and_sentence_in_its_middle() {
        // Two scenes over a 10 s narration of three sentences.
        let plan = plan(vec![scene(0, 4_000), scene(4_000, 10_000)]);
        let timeline =
            Timeline::rough_cut(&plan, &narration(10_000), crate::AspectRatio::Landscape);
        let sentences = [
            sentence(0, 3_000),
            sentence(3_500, 6_000),
            sentence(7_000, 9_500),
        ];
        let length = timeline.duration();
        let placed = place_drops(
            &[
                drop_at(20.0, 30.0),
                drop_at(40.0, 50.0),
                drop_at(60.0, 70.0),
            ],
            length,
            &timeline,
            &sentences,
        );
        assert_eq!(placed[0].start, ms(2_000));
        assert_eq!(placed[0].end, ms(3_000));
        assert_eq!((placed[0].scene, placed[0].sentence), (Some(0), Some(0)));
        assert_eq!((placed[1].scene, placed[1].sentence), (Some(1), Some(1)));
        // In the pause after the second sentence: the one just said.
        assert_eq!((placed[2].scene, placed[2].sentence), (Some(1), Some(1)));
    }

    #[test]
    fn a_drop_with_no_narration_playing_has_no_sentence() {
        let plan = plan(vec![scene(0, 4_000), scene(4_000, 10_000)]);
        let mut timeline =
            Timeline::rough_cut(&plan, &narration(10_000), crate::AspectRatio::Landscape);
        timeline
            .apply(&Edit::Split {
                track: Track::Narration,
                index: 0,
                at: ms(5_000),
            })
            .unwrap();
        timeline
            .apply(&Edit::Delete {
                track: Track::Narration,
                index: 1,
            })
            .unwrap();
        let placed = place_drops(
            &[drop_at(70.0, 80.0)],
            ms(10_000),
            &timeline,
            &[sentence(0, 9_000)],
        );
        assert_eq!((placed[0].scene, placed[0].sentence), (Some(1), None));
    }

    #[test]
    fn the_timing_changes_with_cuts_and_not_with_the_mix() {
        let plan = plan(vec![scene(0, 4_000), scene(4_000, 10_000)]);
        let timeline =
            Timeline::rough_cut(&plan, &narration(10_000), crate::AspectRatio::Landscape);
        let timing = cut_timing(&timeline);
        assert_eq!(timing.len(), 16);
        assert_eq!(cut_timing(&timeline.clone()), timing);

        let mut mixed = timeline.clone();
        mixed.apply(&Edit::ShowCaptions(false)).unwrap();
        let framed = mixed.clone().with_aspect(crate::AspectRatio::Vertical);
        assert_eq!(cut_timing(&mixed), timing, "captions keep the timing");
        assert_eq!(cut_timing(&framed), timing, "framing keeps the timing");

        let mut split = timeline.clone();
        for track in [Track::Video, Track::Narration] {
            split
                .apply(&Edit::Split {
                    track,
                    index: 0,
                    at: ms(2_000),
                })
                .unwrap();
        }
        assert_eq!(cut_timing(&split), timing, "a split plays the same");

        let mut trimmed = timeline.clone();
        trimmed
            .apply(&Edit::Trim {
                track: Track::Video,
                index: 0,
                edge: crate::Edge::End,
                by: crate::Shift::earlier(ms(1_000)),
            })
            .unwrap();
        assert_ne!(cut_timing(&trimmed), timing, "a trim changes it");

        let mut moved = timeline.clone();
        moved
            .apply(&Edit::Split {
                track: Track::Narration,
                index: 0,
                at: ms(5_000),
            })
            .unwrap();
        moved
            .apply(&Edit::Move {
                track: Track::Narration,
                index: 1,
                to: ms(5_500),
            })
            .unwrap();
        assert_ne!(cut_timing(&moved), timing, "moved narration changes it");
    }
}
