//! How large a scene plan's answer gets, known before it is asked for.
//!
//! The plan is the one text whose size follows the video's length: a few
//! lines once (the cast, the places, the visual story) and then an image
//! prompt and a motion prompt per scene. A one-minute video has about ten
//! scenes, a ten-minute one a hundred, so the room the answer needs and
//! what it costs are sized from the narration instead of guessed once for
//! every video.
//!
//! The figures come from counting a plan written under the default
//! template (#267): a scene took 120 to 165 tokens, the largest 131 to 177,
//! depending on the tokenizer.

use std::time::Duration;

use crate::{Narration, sentences};

/// The fastest the template asks scenes to change.
const FASTEST_SCENE: Duration = Duration::from_secs(4);
/// The pace in the middle of what it asks for (4 to 8 seconds).
const USUAL_SCENE: Duration = Duration::from_secs(6);
/// What a plan says once, whatever its length (its story, its cast and its
/// places), at most and usually.
const ONCE_AT_MOST: u64 = 2_500;
const ONCE_USUALLY: u64 = 600;
/// A scene's tokens: at most (well past the largest counted, for prompts
/// longer than the default template's) and usually.
const SCENE_AT_MOST: u64 = 250;
const SCENE_USUALLY: u64 = 160;

/// The size of the plan a narration asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenePlanSize {
    sentences: u64,
    duration: Duration,
}

impl ScenePlanSize {
    pub fn new(sentences: usize, duration: Duration) -> Self {
        Self {
            sentences: sentences as u64,
            duration,
        }
    }

    pub fn of(narration: &Narration) -> Self {
        Self::new(sentences(narration).len(), narration.duration)
    }

    /// Scenes at `pace` each. A scene starts at a sentence, so there are
    /// never more scenes than sentences, and a plan has at least one.
    fn scenes(self, pace: Duration) -> u64 {
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
        self.scenes(USUAL_SCENE)
    }

    /// The room the answer needs, in tokens, before any thinking: what a
    /// request asks the generator to leave for it.
    pub fn answer_tokens_at_most(self) -> u64 {
        ONCE_AT_MOST.saturating_add(self.scenes_at_most().saturating_mul(SCENE_AT_MOST))
    }

    /// The tokens the answer usually takes, before any thinking: what an
    /// estimate of its cost starts from.
    pub fn answer_tokens_usually(self) -> u64 {
        ONCE_USUALLY.saturating_add(self.scenes_usually().saturating_mul(SCENE_USUALLY))
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
        // A Short fits the room every request gets; a long video does not.
        assert_eq!(minutes(1, 15).answer_tokens_at_most(), 2_500 + 15 * 250);
        assert_eq!(minutes(10, 150).answer_tokens_at_most(), 2_500 + 150 * 250);
        assert_eq!(minutes(15, 225).answer_tokens_at_most(), 58_750);
        assert!(minutes(10, 150).answer_tokens_at_most() > 16_000);
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
    }
}
