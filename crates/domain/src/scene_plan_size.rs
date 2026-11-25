//! How large a scene plan's answer gets, known before it is asked for.
//!
//! The plan is the one text whose size follows the video's length: a few
//! lines once (the visual story, the cast, the places) and then an image
//! prompt and a motion prompt per scene. A one-minute video has about ten
//! scenes, a ten-minute one a hundred, so the room the answer needs and
//! what it costs are sized from the narration instead of guessed once for
//! every video.
//!
//! The figures come from counting, in tokens, a plan written under the
//! default scene template (ADR-0031): a scene took 120 to 165 tokens. The
//! sizes "at most" are padded well past that, but they are not a hard
//! limit: a template the user edited, or long aesthetic notes repeated in
//! every prompt, can make a scene larger. Whoever asks for the plan leaves
//! room for that.

use std::time::Duration;

use crate::{Narration, TYPICAL_AD_SCENE, TYPICAL_SCENE, sentences};

/// The fastest the default template asks scenes to change.
const FASTEST_SCENE: Duration = Duration::from_secs(4);
/// What a plan says once, whatever its length (its story, its cast and its
/// places), at most and usually.
const ONCE_AT_MOST: u64 = 2_500;
const ONCE_USUALLY: u64 = 600;
/// A scene's tokens: at most (half as much again as the largest counted)
/// and usually.
const SCENE_AT_MOST: u64 = 250;
const SCENE_USUALLY: u64 = 160;
/// The thought before a plan, which is billed as output: about this much
/// for a short plan, and half as much as the answer for a long one. An
/// assumption until plans of every length have been paid for.
const THOUGHT_USUALLY: u64 = 10_000;

/// The size of the plan a narration asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenePlanSize {
    sentences: u64,
    duration: Duration,
    /// An ad's plan: a scene for every sentence, whatever their pace.
    a_scene_a_sentence: bool,
}

impl ScenePlanSize {
    pub fn new(sentences: usize, duration: Duration) -> Self {
        Self {
            sentences: sentences as u64,
            duration,
            a_scene_a_sentence: false,
        }
    }

    /// The size of the plan of an ad's `narration` (#278): an ad's plan
    /// has a scene for every sentence.
    pub fn of_ad(narration: &Narration) -> Self {
        Self {
            a_scene_a_sentence: true,
            ..Self::of(narration)
        }
    }

    /// The size for the narration of an ad that is not made yet and will
    /// last about `duration`: a scene every [`TYPICAL_AD_SCENE`].
    pub fn about_ad(duration: Duration) -> Self {
        let scenes = duration.as_millis().div_ceil(TYPICAL_AD_SCENE.as_millis());
        Self {
            sentences: u64::try_from(scenes).unwrap_or(u64::MAX),
            duration,
            a_scene_a_sentence: true,
        }
    }

    pub fn of(narration: &Narration) -> Self {
        Self::new(sentences(narration).len(), narration.duration)
    }

    /// The size for a narration that is not made yet and will last about
    /// `duration`: its sentences are taken to be short enough not to bound
    /// the scenes.
    pub fn about(duration: Duration) -> Self {
        Self {
            sentences: u64::MAX,
            duration,
            a_scene_a_sentence: false,
        }
    }

    /// Scenes at `pace` each. A scene starts at a sentence, so there are
    /// never more scenes than sentences, and a plan has at least one.
    fn scenes(self, pace: Duration) -> u64 {
        if self.a_scene_a_sentence {
            return self.sentences.max(1);
        }
        let by_time = self.duration.as_millis().div_ceil(pace.as_millis());
        let by_time = u64::try_from(by_time).unwrap_or(u64::MAX);
        by_time.min(self.sentences).max(1)
    }

    /// The most scenes the plan is sized for.
    pub fn scenes_at_most(self) -> u64 {
        self.scenes(FASTEST_SCENE)
    }

    /// The scenes a plan of this narration usually has.
    pub fn scenes_usually(self) -> u64 {
        self.scenes(TYPICAL_SCENE)
    }

    /// The room the answer needs, in tokens, before any thinking: what a
    /// request asks the generator to leave for it.
    pub fn answer_tokens_at_most(self) -> u64 {
        ONCE_AT_MOST.saturating_add(self.scenes_at_most().saturating_mul(SCENE_AT_MOST))
    }

    /// The tokens the answer usually takes, before any thinking.
    pub fn answer_tokens_usually(self) -> u64 {
        ONCE_USUALLY.saturating_add(self.scenes_usually().saturating_mul(SCENE_USUALLY))
    }

    /// The output tokens the plan is usually billed: the answer and the
    /// thought before it. What an estimate of its cost is made from.
    pub fn output_tokens_usually(self) -> u64 {
        let answer = self.answer_tokens_usually();
        answer.saturating_add(THOUGHT_USUALLY.max(answer / 2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(minutes: u64, sentences: usize) -> ScenePlanSize {
        ScenePlanSize::new(sentences, Duration::from_secs(minutes * 60))
    }

    #[test]
    fn scenes_follow_the_length_of_the_narration() {
        // Sentences of about four seconds: time is what bounds the scenes.
        assert_eq!(minutes(1, 15).scenes_at_most(), 15);
        assert_eq!(minutes(1, 15).scenes_usually(), 10);
        assert_eq!(minutes(10, 150).scenes_at_most(), 150);
        assert_eq!(minutes(10, 150).scenes_usually(), 100);
    }

    #[test]
    fn there_are_never_more_scenes_than_sentences() {
        // Long sentences: a scene starts at a sentence.
        assert_eq!(minutes(1, 6).scenes_at_most(), 6);
        assert_eq!(minutes(1, 6).scenes_usually(), 6);
    }

    #[test]
    fn an_ads_plan_has_a_scene_for_every_sentence() {
        // Ten sentences in thirty seconds: three seconds each, faster
        // than any other plan changes scenes.
        let ad = ScenePlanSize {
            a_scene_a_sentence: true,
            ..ScenePlanSize::new(10, Duration::from_secs(30))
        };
        assert_eq!(ad.scenes_at_most(), 10);
        assert_eq!(ad.scenes_usually(), 10);
        assert_eq!(
            ScenePlanSize::new(10, Duration::from_secs(30)).scenes_usually(),
            5
        );
        // Before its narration: a scene about every three seconds.
        let ahead = ScenePlanSize::about_ad(Duration::from_secs(30));
        assert_eq!(ahead.scenes_usually(), 10);
        assert_eq!(ahead.scenes_at_most(), 10);
        assert_eq!(
            ScenePlanSize::about_ad(Duration::from_secs(16)).scenes_usually(),
            6
        );
    }

    #[test]
    fn a_plan_has_at_least_one_scene() {
        assert_eq!(ScenePlanSize::new(0, Duration::ZERO).scenes_at_most(), 1);
        assert_eq!(
            ScenePlanSize::new(1, Duration::from_millis(900)).scenes_usually(),
            1
        );
    }

    #[test]
    fn a_part_of_a_scene_counts_as_one() {
        // 61 seconds at 4 seconds a scene is 15 scenes and a part.
        let size = ScenePlanSize::new(40, Duration::from_secs(61));
        assert_eq!(size.scenes_at_most(), 16);
    }

    #[test]
    fn the_room_grows_with_the_scenes() {
        // The plan of a long video is many times a Short's.
        assert_eq!(minutes(1, 15).answer_tokens_at_most(), 2_500 + 15 * 250);
        assert_eq!(minutes(10, 150).answer_tokens_at_most(), 2_500 + 150 * 250);
        assert_eq!(minutes(15, 225).answer_tokens_at_most(), 58_750);
        assert!(
            minutes(10, 150).answer_tokens_at_most() > 6 * minutes(1, 15).answer_tokens_at_most()
        );
    }

    #[test]
    fn the_usual_size_is_below_the_room() {
        for (length, sentences) in [(1, 15), (3, 40), (10, 150), (15, 225)] {
            let size = minutes(length, sentences);
            assert!(size.answer_tokens_usually() < size.answer_tokens_at_most());
        }
        assert_eq!(minutes(10, 150).answer_tokens_usually(), 600 + 100 * 160);
    }

    #[test]
    fn a_narration_of_any_length_never_overflows() {
        let size = ScenePlanSize::new(usize::MAX, Duration::MAX);
        assert_eq!(size.answer_tokens_at_most(), u64::MAX);
        assert_eq!(size.answer_tokens_usually(), u64::MAX);
        assert_eq!(size.output_tokens_usually(), u64::MAX);
    }

    #[test]
    fn a_narration_not_made_yet_is_sized_by_its_length_alone() {
        let size = ScenePlanSize::about(Duration::from_secs(600));
        assert_eq!(size.scenes_usually(), 100);
        assert_eq!(size.scenes_at_most(), 150);
    }

    #[test]
    fn the_bill_counts_the_thought_before_the_answer() {
        // A Short: 600 + 10 scenes of 160, and the thought a plan takes.
        assert_eq!(minutes(1, 15).output_tokens_usually(), 2_200 + 10_000);
        // Ten minutes: 16,600 of answer and the same thought.
        assert_eq!(minutes(10, 150).output_tokens_usually(), 16_600 + 10_000);
        // Twenty: the thought is half the answer once that is more.
        assert_eq!(minutes(20, 300).output_tokens_usually(), 32_600 + 16_300);
    }
}
