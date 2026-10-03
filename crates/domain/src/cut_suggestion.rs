//! AI cut suggestions (PRD story 60): points in the narration where the
//! picture could change, found in code, scored by the decision engine, and
//! accepted or rejected one by one in the editor.
//!
//! A candidate sits between two words of the narration: where a sentence
//! ends, where the narrator pauses, or where the scene plan changes scene.
//! The cut goes where the next word starts, a word boundary like any cut
//! snapped to the words. Candidates live in narration-file time, as
//! captions do, so they keep their place however the cut moves the
//! narration, and show wherever the cut plays their moment.
//!
//! The engine judges words, not numbers: the pause lengths and clip
//! lengths stay in code, and the engine reads the script around each point
//! with the point marked, a stretch of the script at a time
//! ([`cut_chunks`]), small enough for its context and free of text the
//! question does not need.
//!
//! Accepting a suggestion splits the video clip under it, an edit like any
//! other; a suggestion whose point already has a cut counts as accepted, so
//! undoing the split brings it back.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{
    Confidence, JobId, Narration, NarrationId, ProfileId, RepositoryError, ScenePlan, Score,
    Timeline, VideoProjectId, frame_time, min_length, nearest_frame, sentences,
};

/// The score a suggestion needs to show unless the user picks another.
pub const DEFAULT_CUT_FLOOR: Score = Score::new(50);
/// "Accept all above": suggestions this strong are accepted together.
pub const STRONG_CUT: Score = Score::new(80);

/// What code found at a candidate point, and what the engine said of the
/// topic there: the typed reasons a suggestion shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CutReasons {
    /// The word before the point ends a sentence.
    pub sentence_end: bool,
    /// The narrator pauses this long at the point, when it is at least
    /// [`CutRules::min_pause`].
    pub pause: Option<Duration>,
    /// The scene plan starts a new scene here.
    pub scene_change: bool,
    /// The engine judged that the narration moves to a new topic, place,
    /// time or step here.
    pub topic_shift: bool,
}

/// Where candidates are looked for, and how much room a new shot needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutRules {
    /// A silence between words at least this long counts as a pause.
    pub min_pause: Duration,
    /// No point is offered closer than this to a cut the picture already
    /// has, or to the start or end of the video: a shot shorter than this
    /// flashes by.
    pub min_shot: Duration,
}

impl Default for CutRules {
    fn default() -> Self {
        Self {
            min_pause: Duration::from_millis(300),
            min_shot: Duration::from_secs(1),
        }
    }
}

/// A point where the picture could change, before the engine scores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutCandidate {
    /// The last word before the point, by index in the narration's words.
    pub word: usize,
    /// Where the cut goes in the narration file: when the next word starts.
    pub source: Duration,
    /// What code found there (never a topic shift: that is the engine's).
    pub reasons: CutReasons,
}

/// The candidate points of `narration`, in order. `plan` adds its scene
/// changes when it was planned on this narration.
pub fn cut_candidates(
    narration: &Narration,
    plan: Option<&ScenePlan>,
    rules: &CutRules,
) -> Vec<CutCandidate> {
    let words = narration.words.as_slice();
    let text = narration.text.as_str();
    let scene_starts: Vec<Duration> = plan
        .filter(|plan| plan.narration == narration.id)
        .map(|plan| {
            plan.scenes()
                .iter()
                .skip(1)
                .map(|scene| scene.start)
                .collect()
        })
        .unwrap_or_default();
    let sentence_ends: Vec<usize> = sentences(narration)
        .into_iter()
        .filter(|sentence| ends_sentence(&text[words[sentence.words.end - 1].text.clone()]))
        .map(|sentence| sentence.words.end - 1)
        .collect();
    words
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| {
            let (word, next) = (&pair[0], &pair[1]);
            let silence = next.start.saturating_sub(word.end);
            let reasons = CutReasons {
                sentence_end: sentence_ends.contains(&index),
                pause: (silence >= rules.min_pause).then_some(silence),
                scene_change: scene_starts
                    .iter()
                    .any(|start| word.start < *start && *start <= next.start),
                topic_shift: false,
            };
            (reasons.sentence_end || reasons.pause.is_some() || reasons.scene_change).then_some(
                CutCandidate {
                    word: index,
                    source: next.start,
                    reasons,
                },
            )
        })
        .collect()
}

/// A word that ends a sentence: `.`, `!`, `?` or `…`, past closing quotes
/// and brackets. A sentence the scene planner closed for its length or at
/// a line break did not end in the text.
fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '”', '’', '»', ')', ']', '*'])
        .ends_with(['.', '!', '?', '…'])
}

/// Where a point of the narration stands on a timeline's picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutPlace {
    /// No cut there yet: splitting video clip `clip` at `at` makes one.
    Open { clip: usize, at: Duration },
    /// The picture already cuts at `at`.
    Made { at: Duration },
}

impl CutPlace {
    pub fn at(self) -> Duration {
        match self {
            CutPlace::Open { at, .. } | CutPlace::Made { at } => at,
        }
    }
}

/// Where the moment `source` of the narration file falls on `timeline`'s
/// picture, on its nearest frame. `None` when the cut does not play that
/// moment, or no clip runs there with a frame to spare on each side.
pub fn place_cut(timeline: &Timeline, source: Duration) -> Option<CutPlace> {
    let at = frame_time(nearest_frame(timeline.on_narration(source)?));
    let video = timeline.video();
    let index = timeline.video_at(at)?;
    let clip = &video[index];
    if index > 0 && clip.at == at {
        return Some(CutPlace::Made { at });
    }
    (clip.at + min_length() <= at && at + min_length() <= clip.end())
        .then_some(CutPlace::Open { clip: index, at })
}

/// The candidates worth asking about on `timeline`: those whose moment
/// the cut plays, with no cut there yet and room for a shot on each side.
pub fn open_candidates(
    timeline: &Timeline,
    candidates: Vec<CutCandidate>,
    rules: &CutRules,
) -> Vec<CutCandidate> {
    let video = timeline.video();
    candidates
        .into_iter()
        .filter(|candidate| match place_cut(timeline, candidate.source) {
            Some(CutPlace::Open { clip, at }) => {
                let clip = &video[clip];
                at >= clip.at + rules.min_shot && at + rules.min_shot <= clip.end()
            }
            _ => false,
        })
        .collect()
}

/// How much script one engine call reads, and how many points it judges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLimits {
    /// Characters of script a chunk holds, besides the sentence before and
    /// after it read as context. A sentence longer than this still goes
    /// whole, alone.
    pub max_chars: usize,
    /// Points marked in one chunk.
    pub max_cuts: usize,
}

/// One engine call's share: a stretch of the script with its points marked
/// ([`cut_marker`]), and which candidates they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutChunk {
    pub text: String,
    /// Indexes into the candidates.
    pub cuts: Vec<usize>,
}

/// How point `index` (into the candidates) is marked in a chunk's text.
pub fn cut_marker(index: usize) -> String {
    format!("[CUT {}]", index + 1)
}

/// Splits the script into chunks on sentence boundaries, each within
/// `limits`, so every candidate is judged once with the words around it.
/// Each chunk's text runs from the sentence before its first to the
/// sentence after its last, so a point at either end has words on both
/// sides; only its own points are marked. Chunks without a point are left
/// out.
pub fn cut_chunks(
    narration: &Narration,
    candidates: &[CutCandidate],
    limits: &ChunkLimits,
) -> Vec<CutChunk> {
    let text = narration.text.as_str();
    let words = narration.words.as_slice();
    let sentences = sentences(narration);
    let points_in = |range: &std::ops::Range<usize>| {
        candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| range.contains(&candidate.word))
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };

    // Sentence ranges (by index into `sentences`) of each chunk.
    let mut groups: Vec<std::ops::Range<usize>> = Vec::new();
    let mut first = 0;
    let (mut chars, mut cuts) = (0, 0);
    for (index, sentence) in sentences.iter().enumerate() {
        let length = text[sentence.text.clone()].chars().count();
        let points = points_in(&sentence.words).len();
        if index > first && (chars + length > limits.max_chars || cuts + points > limits.max_cuts) {
            groups.push(first..index);
            first = index;
            (chars, cuts) = (0, 0);
        }
        chars += length;
        cuts += points;
    }
    if first < sentences.len() {
        groups.push(first..sentences.len());
    }

    groups
        .into_iter()
        .filter_map(|group| {
            let own = sentences[group.start].words.start..sentences[group.end - 1].words.end;
            let cuts = points_in(&own);
            if cuts.is_empty() {
                return None;
            }
            let from = group.start.saturating_sub(1);
            let to = (group.end + 1).min(sentences.len());
            let span = sentences[from].text.start..sentences[to - 1].text.end;
            let mut marked = String::new();
            let mut at = span.start;
            for &cut in &cuts {
                let after = words[candidates[cut].word].text.end;
                marked.push_str(&text[at..after]);
                marked.push(' ');
                marked.push_str(&cut_marker(cut));
                at = after;
            }
            marked.push_str(&text[at..span.end]);
            Some(CutChunk { text: marked, cuts })
        })
        .collect()
}

/// Whether the user turned a suggestion down. Accepted is not kept: a
/// suggestion is accepted while the picture cuts at its point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuggestionStatus {
    #[default]
    Pending,
    Rejected,
}

impl SuggestionStatus {
    /// Stable identifier for storage. Never change one.
    pub fn code(self) -> &'static str {
        match self {
            SuggestionStatus::Pending => "pending",
            SuggestionStatus::Rejected => "rejected",
        }
    }

    /// The status a stored code names; an unknown one reads as pending.
    pub fn from_code_or_default(code: &str) -> Self {
        match code {
            "rejected" => SuggestionStatus::Rejected,
            _ => SuggestionStatus::Pending,
        }
    }
}

/// One scored point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutSuggestion {
    /// Where the cut goes in the narration file.
    pub source: Duration,
    pub reasons: CutReasons,
    /// How good a cut the engine judged it, for pacing.
    pub score: Score,
    pub confidence: Confidence,
    pub status: SuggestionStatus,
}

/// A project's suggestions from one request, in narration order. A new
/// request replaces them all.
#[derive(Debug, Clone, PartialEq)]
pub struct CutSuggestions {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    /// The narration they point into; another narration leaves them behind.
    pub narration: NarrationId,
    /// The job that scored them.
    pub job: JobId,
    /// The engine's model, for the record.
    pub model: String,
    pub made_at: SystemTime,
    pub suggestions: Vec<CutSuggestion>,
}

impl CutSuggestions {
    /// Turns suggestion `index` down, or takes that back; `false` when
    /// there is no such suggestion or it already has that status.
    pub fn set_status(&mut self, index: usize, status: SuggestionStatus) -> bool {
        match self.suggestions.get_mut(index) {
            Some(suggestion) if suggestion.status != status => {
                suggestion.status = status;
                true
            }
            _ => false,
        }
    }
}

/// Persistence port for cut suggestions.
pub trait CutSuggestionRepository: Send + Sync {
    /// The project's latest suggestions, if it has any.
    fn cut_suggestions(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<CutSuggestions>, RepositoryError>;

    /// Makes `suggestions` the project's, replacing any before.
    fn save_cut_suggestions(&self, suggestions: &CutSuggestions) -> Result<(), RepositoryError>;
}

impl<T: CutSuggestionRepository + ?Sized> CutSuggestionRepository for Arc<T> {
    fn cut_suggestions(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<CutSuggestions>, RepositoryError> {
        (**self).cut_suggestions(project)
    }

    fn save_cut_suggestions(&self, suggestions: &CutSuggestions) -> Result<(), RepositoryError> {
        (**self).save_cut_suggestions(suggestions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::{ms, narrated, plan, scene};
    use crate::{Edit, Track};

    /// Three sentences: "One two." (0-0.9 s), a 600 ms pause, "Three, four
    /// five." (1.5-3.4 s, a 350 ms pause after the comma), "Six seven."
    /// (3.5-4.6 s).
    fn story() -> Narration {
        narrated(
            "One two. Three, four five. Six seven.",
            &[
                (0, 400),
                (500, 900),
                (1_500, 1_900),
                (2_250, 2_700),
                (2_800, 3_400),
                (3_500, 4_000),
                (4_100, 4_600),
            ],
            5_000,
        )
    }

    fn sources(candidates: &[CutCandidate]) -> Vec<Duration> {
        candidates.iter().map(|c| c.source).collect()
    }

    #[test]
    fn candidates_come_from_sentence_ends_and_pauses() {
        let candidates = cut_candidates(&story(), None, &CutRules::default());
        assert_eq!(sources(&candidates), [ms(1_500), ms(2_250), ms(3_500)]);
        assert_eq!(
            candidates[0].reasons,
            CutReasons {
                sentence_end: true,
                pause: Some(ms(600)),
                ..CutReasons::default()
            }
        );
        assert_eq!(candidates[0].word, 1);
        assert_eq!(
            candidates[1].reasons,
            CutReasons {
                pause: Some(ms(350)),
                ..CutReasons::default()
            },
            "a pause inside a sentence"
        );
        assert_eq!(
            candidates[2].reasons,
            CutReasons {
                sentence_end: true,
                ..CutReasons::default()
            },
            "a short breath is no pause"
        );
    }

    #[test]
    fn the_pause_threshold_is_a_rule() {
        let rules = CutRules {
            min_pause: ms(100),
            ..CutRules::default()
        };
        let candidates = cut_candidates(&story(), None, &rules);
        assert_eq!(candidates.len(), 6, "every gap of 100 ms or more");
    }

    #[test]
    fn scene_changes_of_the_plan_on_this_narration_are_reasons() {
        let narration = story();
        let mut planned = plan(vec![scene(0, 3_500), scene(3_500, 5_000)]);
        planned.narration = narration.id;
        let candidates = cut_candidates(&narration, Some(&planned), &CutRules::default());
        assert!(candidates[2].reasons.scene_change);
        assert!(!candidates[0].reasons.scene_change);

        planned.narration = NarrationId::new();
        let candidates = cut_candidates(&narration, Some(&planned), &CutRules::default());
        assert!(
            candidates.iter().all(|c| !c.reasons.scene_change),
            "a plan of another narration says nothing"
        );
    }

    #[test]
    fn a_scene_change_alone_makes_a_candidate() {
        let narration = narrated(
            "No break here at all",
            &[
                (0, 300),
                (300, 600),
                (600, 900),
                (900, 1_200),
                (1_200, 1_500),
            ],
            2_000,
        );
        let mut planned = plan(vec![scene(0, 600), scene(600, 2_000)]);
        planned.narration = narration.id;
        let candidates = cut_candidates(&narration, Some(&planned), &CutRules::default());
        assert_eq!(sources(&candidates), [ms(600)]);
        assert!(candidates[0].reasons.scene_change);
    }

    #[test]
    fn a_closing_quote_still_ends_a_sentence() {
        let narration = narrated(
            "He said “stop.” Then left",
            &[(0, 100), (150, 300), (350, 600), (650, 800), (850, 1_000)],
            1_200,
        );
        let candidates = cut_candidates(&narration, None, &CutRules::default());
        assert_eq!(sources(&candidates), [ms(650)]);
    }

    /// One still over the whole story.
    fn one_shot() -> Timeline {
        let mut first = scene(0, 5_000);
        first.image = Some(crate::timeline::tests::image("a.png"));
        Timeline::rough_cut(&plan(vec![first]), &story())
    }

    #[test]
    fn a_point_lands_on_the_nearest_frame_of_the_cut() {
        let timeline = one_shot();
        assert_eq!(
            place_cut(&timeline, ms(1_500)),
            Some(CutPlace::Open {
                clip: 0,
                at: frame_time(45)
            })
        );
        assert_eq!(
            place_cut(&timeline, ms(2_250)),
            Some(CutPlace::Open {
                clip: 0,
                at: frame_time(nearest_frame(ms(2_250)))
            })
        );
    }

    #[test]
    fn a_point_with_a_cut_is_made_and_undoing_opens_it_again() {
        let mut timeline = one_shot();
        let at = place_cut(&timeline, ms(1_500)).unwrap().at();
        let undo = timeline
            .apply(&Edit::Split {
                track: Track::Video,
                index: 0,
                at,
            })
            .unwrap();
        assert_eq!(place_cut(&timeline, ms(1_500)), Some(CutPlace::Made { at }));
        timeline.apply(&undo).unwrap();
        assert!(matches!(
            place_cut(&timeline, ms(1_500)),
            Some(CutPlace::Open { .. })
        ));
    }

    #[test]
    fn a_point_the_cut_left_out_has_no_place() {
        let mut timeline = one_shot();
        // Cut the narration's first two seconds away.
        timeline
            .apply(&Edit::Trim {
                track: Track::Narration,
                index: 0,
                edge: crate::Edge::Start,
                by: crate::Shift::later(ms(2_000)),
            })
            .unwrap();
        assert_eq!(place_cut(&timeline, ms(1_500)), None);
        assert!(place_cut(&timeline, ms(3_500)).is_some());
    }

    #[test]
    fn only_open_points_with_room_for_a_shot_are_asked_about() {
        let mut timeline = one_shot();
        let candidates = cut_candidates(&story(), None, &CutRules::default());
        assert_eq!(
            sources(&open_candidates(
                &timeline,
                candidates.clone(),
                &CutRules::default()
            )),
            [ms(1_500), ms(2_250), ms(3_500)]
        );
        let at = place_cut(&timeline, ms(1_500)).unwrap().at();
        timeline
            .apply(&Edit::Split {
                track: Track::Video,
                index: 0,
                at,
            })
            .unwrap();
        // 1.5 s has its cut; 2.25 s is under a second after it.
        assert_eq!(
            sources(&open_candidates(
                &timeline,
                candidates,
                &CutRules::default()
            )),
            [ms(3_500)]
        );
    }

    #[test]
    fn a_point_near_the_end_of_the_video_leaves_no_room() {
        let timeline = one_shot();
        let late = CutCandidate {
            word: 5,
            source: ms(4_500),
            reasons: CutReasons::default(),
        };
        assert!(open_candidates(&timeline, vec![late], &CutRules::default()).is_empty());
    }

    #[test]
    fn a_chunk_marks_its_points_with_a_sentence_of_context_each_side() {
        let narration = story();
        let candidates = cut_candidates(&narration, None, &CutRules::default());
        let limits = ChunkLimits {
            max_chars: 1_000,
            max_cuts: 40,
        };
        assert_eq!(
            cut_chunks(&narration, &candidates, &limits),
            [CutChunk {
                text: "One two. [CUT 1] Three, [CUT 2] four five. [CUT 3] Six seven.".into(),
                cuts: vec![0, 1, 2],
            }]
        );
    }

    #[test]
    fn long_scripts_are_chunked_within_the_limits() {
        let narration = story();
        let candidates = cut_candidates(&narration, None, &CutRules::default());
        let by_chars = ChunkLimits {
            max_chars: 12,
            max_cuts: 40,
        };
        assert_eq!(
            cut_chunks(&narration, &candidates, &by_chars),
            [
                CutChunk {
                    text: "One two. [CUT 1] Three, four five.".into(),
                    cuts: vec![0],
                },
                CutChunk {
                    text: "One two. Three, [CUT 2] four five. [CUT 3] Six seven.".into(),
                    cuts: vec![1, 2],
                },
            ],
            "sentences of 8, 17 and 10 characters go alone is its own chunk; the last has no point"
        );
        let by_cuts = ChunkLimits {
            max_chars: 1_000,
            max_cuts: 1,
        };
        let chunks = cut_chunks(&narration, &candidates, &by_cuts);
        let cuts: Vec<Vec<usize>> = chunks.iter().map(|chunk| chunk.cuts.clone()).collect();
        assert_eq!(cuts, [vec![0], vec![1, 2]], "a sentence is never split");
    }

    #[test]
    fn every_candidate_is_in_exactly_one_chunk() {
        let text = (0..60)
            .map(|i| format!("Sentence number {i} goes here."))
            .collect::<Vec<_>>()
            .join(" ");
        let words: Vec<(u64, u64)> = (0..300).map(|i| (i * 500, i * 500 + 300)).collect();
        let narration = narrated(&text, &words, 150_000);
        let candidates = cut_candidates(&narration, None, &CutRules::default());
        assert_eq!(candidates.len(), 59);
        let chunks = cut_chunks(
            &narration,
            &candidates,
            &ChunkLimits {
                max_chars: 200,
                max_cuts: 4,
            },
        );
        assert!(chunks.len() > 10);
        let mut seen: Vec<usize> = chunks.iter().flat_map(|c| c.cuts.clone()).collect();
        seen.sort();
        assert_eq!(seen, (0..59).collect::<Vec<_>>());
        for chunk in &chunks {
            assert!(chunk.cuts.len() <= 4);
            for cut in &chunk.cuts {
                assert!(chunk.text.contains(&cut_marker(*cut)));
            }
        }
    }

    #[test]
    fn a_status_changes_once() {
        let mut suggestions = CutSuggestions {
            project: VideoProjectId::new(),
            owner: ProfileId::new(),
            narration: NarrationId::new(),
            job: JobId::new(),
            model: "jev".into(),
            made_at: SystemTime::UNIX_EPOCH,
            suggestions: vec![CutSuggestion {
                source: ms(1_500),
                reasons: CutReasons::default(),
                score: Score::new(70),
                confidence: Confidence::new(0.8),
                status: SuggestionStatus::Pending,
            }],
        };
        assert!(suggestions.set_status(0, SuggestionStatus::Rejected));
        assert!(!suggestions.set_status(0, SuggestionStatus::Rejected));
        assert!(!suggestions.set_status(1, SuggestionStatus::Rejected));
        assert!(suggestions.set_status(0, SuggestionStatus::Pending));
    }

    #[test]
    fn statuses_round_trip_through_their_codes() {
        for status in [SuggestionStatus::Pending, SuggestionStatus::Rejected] {
            assert_eq!(
                SuggestionStatus::from_code_or_default(status.code()),
                status
            );
        }
        assert_eq!(
            SuggestionStatus::from_code_or_default("accepted"),
            SuggestionStatus::Pending
        );
    }
}
