//! Localized versions (#184, ADR-0023): a video project made again for
//! another channel's language and country from an existing one. Claude
//! adapts the script sentence by sentence; the new project keeps the
//! source's pictures and cut and gets its own narration. Once it is
//! narrated, each scene moves to where the new narration says the sentence
//! it started at, and the cut is rebuilt on those times with the source
//! cut's look.

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::narration::key;
use crate::{
    AspectRatio, AudioItem, AudioLane, CaptionHighlight, CaptionStyle, ContentLanguage, Country,
    Framing, GenerationId, MediaAsset, MediaKind, Mix, Motion, MusicTrack, Narration, Picture,
    ProfileId, RepositoryError, Scene, ScenePlan, ScenePlanId, ScenePlanRecord, Script, Sentence,
    Theme, Timeline, Transition, VideoProject, VideoProjectId, VideoSource, min_length,
    spoken_words,
};

/// The most word pairs the script and its narration are matched over
/// word by word; past it (scripts far longer than Bardo writes) the words
/// between the shared start and end are matched by their place instead.
const MAX_MATCH_CELLS: usize = 9_000_000;

/// How much a scene's length may change before the Scenes stage points it
/// out: longer than one and a half times, or shorter than two thirds.
pub const STRETCH_FLAG: (u32, u32) = (3, 2);

/// A sentence of the source narration, as the version was made from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSentence {
    pub text: String,
    /// When the source narration starts saying it.
    pub start: Duration,
}

/// How the source cut showed one scene: the framing, motion and transition
/// of the first piece that showed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneLook {
    pub framing: Framing,
    pub motion: Option<Motion>,
    pub transition: Option<Transition>,
}

/// A music or SFX item of the source cut, placed in narration time: where
/// it played against the words, not the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarriedAudio {
    /// `AudioLane::Music` or `AudioLane::Sfx`.
    pub lane: AudioLane,
    pub file: String,
    /// Where in the file it starts.
    pub start: Duration,
    /// Where in the source narration it starts and ends.
    pub at: Duration,
    pub until: Duration,
    pub fade_in: Duration,
    pub fade_out: Duration,
}

/// What a version takes from the source cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarriedCut {
    pub aspect: AspectRatio,
    /// Per scene of the source plan: how the cut showed it, or `None` when
    /// the cut left it out.
    pub scenes: Vec<Option<SceneLook>>,
    pub audio: Vec<CarriedAudio>,
    pub mix: Mix,
    pub caption_style: CaptionStyle,
    pub caption_highlight: CaptionHighlight,
    pub captions_shown: bool,
}

impl CarriedCut {
    /// What `timeline`, a cut on a plan of `scenes` scenes, gives a version.
    pub fn of(timeline: &Timeline, scenes: usize) -> CarriedCut {
        let looks = (0..scenes)
            .map(|scene| {
                timeline
                    .video()
                    .iter()
                    .position(|item| item.picture == Picture::Scene(scene))
                    .map(|index| {
                        let item = &timeline.video()[index];
                        SceneLook {
                            framing: item.framing,
                            motion: item.motion,
                            transition: item.transition.filter(|_| index > 0),
                        }
                    })
            })
            .collect();
        let audio = [AudioLane::Music, AudioLane::Sfx]
            .into_iter()
            .flat_map(|lane| {
                timeline.audio(lane).iter().map(move |item| CarriedAudio {
                    lane,
                    file: item.file.clone(),
                    start: item.start,
                    at: narration_time(timeline, item.at),
                    until: narration_time(timeline, item.end())
                        .max(narration_time(timeline, item.at)),
                    fade_in: item.fade_in,
                    fade_out: item.fade_out,
                })
            })
            .collect();
        CarriedCut {
            aspect: timeline.aspect(),
            scenes: looks,
            audio,
            mix: *timeline.mix(),
            caption_style: timeline.captions.style,
            caption_highlight: timeline.captions.highlight,
            captions_shown: timeline.captions.shown,
        }
    }
}

/// Where in the narration file the cut plays at `time`: by the narration
/// piece playing then, else the one before it, else the first one.
fn narration_time(timeline: &Timeline, time: Duration) -> Duration {
    let items = timeline.narration();
    if let Some(item) = items
        .iter()
        .find(|item| item.at <= time && time < item.end())
    {
        return item.start + (time - item.at);
    }
    if let Some(item) = items.iter().rev().find(|item| item.end() <= time) {
        return item.start + item.duration + (time - item.end());
    }
    match items.first() {
        Some(item) => item.start.saturating_sub(item.at.saturating_sub(time)),
        None => time,
    }
}

/// A version: the project it was made from and what it carried over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Localization {
    /// The version.
    pub project: VideoProjectId,
    pub owner: ProfileId,
    /// The project it was made from; `None` once that project is gone.
    pub source: Option<VideoProjectId>,
    pub source_title: String,
    pub source_language: ContentLanguage,
    pub source_country: Country,
    /// How long the source narration ran.
    pub source_length: Duration,
    /// The source narration's sentences, in order: what Claude adapted.
    pub sentences: Vec<SourceSentence>,
    /// The scene plan carried over; its scenes move with their sentences
    /// while it is the version's plan.
    pub plan: ScenePlanId,
    /// Per scene of that plan, the sentence it starts at.
    pub scene_sentences: Vec<usize>,
    pub cut: CarriedCut,
    pub created_at: SystemTime,
}

impl Localization {
    /// Whether `plan` is the plan carried over, whose scenes can be placed
    /// on a new narration.
    pub fn carries(&self, plan: &ScenePlan) -> bool {
        plan.id == self.plan && plan.scenes().len() == self.scene_sentences.len()
    }

    /// When scene `index` started and ended in the source narration.
    pub fn source_span(&self, index: usize) -> Option<(Duration, Duration)> {
        let start = |index: usize| {
            if index == 0 {
                Some(Duration::ZERO)
            } else {
                let sentence = self.scene_sentences.get(index)?;
                Some(self.sentences.get(*sentence)?.start)
            }
        };
        let from = start(index)?;
        let to = if index + 1 < self.scene_sentences.len() {
            start(index + 1)?
        } else {
            self.source_length
        };
        Some((from, to.max(from)))
    }

    /// Source narration time to the version's: through each sentence's
    /// start in both (`starts`, from [`sentence_times`]), and from the
    /// source's end to `length`, the new narration's.
    pub fn time_map(&self, starts: &[Duration], length: Duration) -> TimeMap {
        let points = std::iter::once((Duration::ZERO, Duration::ZERO))
            .chain(
                self.sentences
                    .iter()
                    .zip(starts)
                    .map(|(sentence, &start)| (sentence.start, start)),
            )
            .chain([(self.source_length, length)]);
        TimeMap::new(points)
    }
}

/// A piecewise straight map from one narration's time to another's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeMap {
    /// (from, to), both never going back.
    points: Vec<(Duration, Duration)>,
}

impl TimeMap {
    fn new(points: impl IntoIterator<Item = (Duration, Duration)>) -> Self {
        let mut kept: Vec<(Duration, Duration)> = Vec::new();
        for (from, to) in points {
            // A point going back in either time is out of order: it is
            // dropped, or held level.
            match kept.last() {
                Some(&(last_from, _)) if from < last_from => {}
                Some(&(_, last_to)) => kept.push((from, to.max(last_to))),
                None => kept.push((from, to)),
            }
        }
        Self { points: kept }
    }

    /// Where `time` falls in the other narration: between the points around
    /// it, in proportion; past the last point, as far past its other side.
    pub fn at(&self, time: Duration) -> Duration {
        let after = self.points.partition_point(|&(from, _)| from <= time);
        let Some(&(from, to)) = after.checked_sub(1).and_then(|i| self.points.get(i)) else {
            return time;
        };
        match self.points.get(after) {
            Some(&(next_from, next_to)) if next_from > from => {
                let span = (next_from - from).as_nanos();
                let gone = (time - from).as_nanos();
                let reach = (next_to - to).as_nanos();
                to + Duration::from_nanos(u64::try_from(reach * gone / span).unwrap_or(u64::MAX))
            }
            Some(_) => to,
            None => to + (time - from),
        }
    }
}

/// Claude's adaptation does not fit the source sentences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AdaptationError {
    #[error("{got} adapted sentences for {expected} source sentences")]
    WrongCount { expected: usize, got: usize },
    #[error("adapted sentence {0} is empty")]
    EmptySentence(usize),
}

/// An adapted script: the text, and where each source sentence starts in
/// it (bytes, ascending, the first at 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adaptation {
    pub text: String,
    pub starts: Vec<usize>,
}

/// What separates two sentences of a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    Space,
    LineBreak,
    BlankLine,
}

impl Gap {
    fn of(between: &str) -> Gap {
        if between.contains("\n\n") {
            Gap::BlankLine
        } else if between.contains('\n') {
            Gap::LineBreak
        } else {
            Gap::Space
        }
    }

    fn text(self) -> &'static str {
        match self {
            Gap::Space => " ",
            Gap::LineBreak => "\n",
            Gap::BlankLine => "\n\n",
        }
    }
}

/// What separates each sentence of `narration` (its [`crate::sentences`])
/// from the next.
pub fn source_gaps(narration: &Narration, sentences: &[Sentence]) -> Vec<Gap> {
    let text = narration.text.as_str();
    sentences
        .windows(2)
        .map(|pair| Gap::of(&text[pair[0].text.end..pair[1].text.start]))
        .collect()
}

/// What separates each sentence of an adaptation from the next: the
/// spacing that ends it.
pub fn adaptation_gaps(text: &str, starts: &[usize]) -> Vec<Gap> {
    starts
        .windows(2)
        .map(|pair| {
            let sentence = text.get(pair[0]..pair[1]).unwrap_or_default();
            let trimmed = sentence.trim_end();
            Gap::of(&sentence[trimmed.len()..])
        })
        .collect()
}

/// The adapted sentences, one per source sentence (one more than `gaps`),
/// trimmed and joined with the source's spacing.
pub fn join_adapted(adapted: &[String], gaps: &[Gap]) -> Result<Adaptation, AdaptationError> {
    if adapted.len() != gaps.len() + 1 {
        return Err(AdaptationError::WrongCount {
            expected: gaps.len() + 1,
            got: adapted.len(),
        });
    }
    let mut joined = String::new();
    let mut starts = Vec::with_capacity(adapted.len());
    for (index, sentence) in adapted.iter().enumerate() {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            return Err(AdaptationError::EmptySentence(index));
        }
        if index > 0 {
            joined.push_str(gaps[index - 1].text());
        }
        starts.push(joined.len());
        joined.push_str(sentence);
    }
    Ok(Adaptation {
        text: joined,
        starts,
    })
}

/// The sentence of `sentences` each scene of `plan` starts at: the first
/// scene at the first sentence, every other at the last sentence starting
/// by its start (scenes are planned on sentence starts).
pub fn scene_sentences(plan: &ScenePlan, sentences: &[Sentence]) -> Vec<usize> {
    plan.scenes()
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            if index == 0 {
                0
            } else {
                sentences
                    .partition_point(|sentence| sentence.start <= scene.start)
                    .saturating_sub(1)
            }
        })
        .collect()
}

/// Where each sentence of an adaptation starts in its narration: the index
/// of the narration word it starts at, or the number of words when nothing
/// of it or after it was narrated. `text` is the adaptation and `starts`
/// its sentence starts; the narration read the script as the user left it,
/// so words are matched between the two in order, and a sentence whose
/// opening words are gone starts at the next word both still share.
pub fn sentence_words(text: &str, starts: &[usize], narration: &Narration) -> Vec<usize> {
    let ours = spoken_words(text);
    let keys = |text: &str, ranges: &mut dyn Iterator<Item = Range<usize>>| -> Vec<String> {
        ranges.map(|range| key(&text[range])).collect()
    };
    let from = keys(text, &mut ours.iter().cloned());
    let spoken = narration.text.as_str();
    let to = keys(
        spoken,
        &mut narration
            .words
            .as_slice()
            .iter()
            .map(|word| word.text.clone()),
    );
    let matched = matched_words(&from, &to);
    starts
        .iter()
        .map(|&start| {
            let first = ours.partition_point(|word| word.end <= start);
            matched[first.min(matched.len())..]
                .iter()
                .find_map(|word| *word)
                .unwrap_or(to.len())
        })
        .collect()
}

/// For each word of `from`, the word of `to` it is where both keep it in
/// the same order (their longest common run of words); `None` for the
/// words `to` dropped or changed.
fn matched_words(from: &[String], to: &[String]) -> Vec<Option<usize>> {
    let (n, m) = (from.len(), to.len());
    let mut matched = vec![None; n];
    let mut head = 0;
    while head < n && head < m && from[head] == to[head] {
        matched[head] = Some(head);
        head += 1;
    }
    let mut tail = 0;
    while tail < n - head && tail < m - head && from[n - 1 - tail] == to[m - 1 - tail] {
        matched[n - 1 - tail] = Some(m - 1 - tail);
        tail += 1;
    }
    let (a, b) = (&from[head..n - tail], &to[head..m - tail]);
    if a.is_empty() || b.is_empty() {
        return matched;
    }
    if a.len().saturating_mul(b.len()) > MAX_MATCH_CELLS {
        for (index, word) in matched[head..n - tail].iter_mut().enumerate() {
            *word = Some(head + index * b.len() / a.len());
        }
        return matched;
    }
    // Longest common subsequence of the middle, from the end.
    let width = b.len() + 1;
    let mut length = vec![0u32; (a.len() + 1) * width];
    let same = |i: usize, j: usize| !a[i].is_empty() && a[i] == b[j];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            length[i * width + j] = if same(i, j) {
                length[(i + 1) * width + j + 1] + 1
            } else {
                length[(i + 1) * width + j].max(length[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if same(i, j) {
            matched[head + i] = Some(head + j);
            i += 1;
            j += 1;
        } else if length[(i + 1) * width + j] >= length[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    matched
}

/// When the narration says each word of `words` (from [`sentence_words`]):
/// that word's start, or the narration's end for a sentence not narrated.
pub fn sentence_times(words: &[usize], narration: &Narration) -> Vec<Duration> {
    let timings = narration.words.as_slice();
    words
        .iter()
        .map(|&word| timings.get(word).map_or(narration.duration, |w| w.start))
        .collect()
}

/// `plan`, the plan a version carried, placed on `narration`: each scene
/// keeps its pictures and prompts and starts where the narration says the
/// sentence it started at (`scene_sentences`, with `words` from
/// [`sentence_words`]), runs until the next one starts and holds the words
/// said meanwhile. A scene whose sentences were all dropped gets no time.
pub fn place_scenes(
    plan: &ScenePlan,
    scene_sentences: &[usize],
    words: &[usize],
    narration: &Narration,
    now: SystemTime,
) -> ScenePlan {
    let timings = narration.words.as_slice();
    let text = narration.text.as_str();
    let end = narration
        .duration
        .max(timings.last().map_or(Duration::ZERO, |word| word.end));
    let mut firsts: Vec<usize> = Vec::with_capacity(plan.scenes().len());
    for index in 0..plan.scenes().len() {
        let word = if index == 0 {
            0
        } else {
            scene_sentences
                .get(index)
                .and_then(|sentence| words.get(*sentence))
                .copied()
                .unwrap_or(timings.len())
        };
        let floor = firsts.last().copied().unwrap_or(0);
        firsts.push(word.max(floor).min(timings.len()));
    }
    let start = |word: usize| timings.get(word).map_or(end, |w| w.start.min(end));
    let scenes: Vec<Scene> = plan
        .scenes()
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            let first = firsts[index];
            let next = firsts.get(index + 1).copied().unwrap_or(timings.len());
            let mut scene = scene.clone();
            scene.start = if index == 0 {
                Duration::ZERO
            } else {
                start(first)
            };
            scene.end = if index + 1 < firsts.len() {
                start(next)
            } else {
                end
            }
            .max(scene.start);
            scene.text = if next > first {
                text[timings[first].text.start..timings[next - 1].text.end].to_owned()
            } else {
                String::new()
            };
            scene
        })
        .collect();
    ScenePlan::restore(ScenePlanRecord {
        id: plan.id,
        project: plan.project,
        owner: plan.owner,
        narration: narration.id,
        generation: plan.generation.clone(),
        scenes,
        updated_at: now,
    })
}

/// A version's cut: the rough cut of `plan` (placed with [`place_scenes`])
/// on `narration`, in the source cut's shape, captions and mix, each scene
/// shown as the source cut showed it, the scenes it left out left out (the
/// scene before runs on), and its music and SFX moved through `map` from
/// the source narration's time to this one's: music keeps its start and end
/// on the words, an SFX its length; neither plays past its file in `media`,
/// nor the music past the picture.
pub fn localized_cut(
    plan: &ScenePlan,
    narration: &Narration,
    cut: &CarriedCut,
    map: &TimeMap,
    media: &[MediaAsset],
) -> Timeline {
    let mut timeline = Timeline::rough_cut(plan, narration, cut.aspect)
        .with_caption_style(cut.caption_style)
        .with_caption_highlight(cut.caption_highlight);
    timeline.captions.shown = cut.captions_shown;
    timeline.mix = cut.mix;
    let shown = |scene: Option<usize>| match scene {
        Some(scene) => cut.scenes.get(scene).copied().flatten(),
        None => None,
    };
    if timeline
        .video
        .iter()
        .any(|item| shown(item.scene()).is_some())
    {
        let mut video: Vec<crate::VideoItem> = Vec::with_capacity(timeline.video.len());
        let mut owed = Duration::ZERO;
        for mut item in std::mem::take(&mut timeline.video) {
            match shown(item.scene()) {
                Some(look) => {
                    item.framing = look.framing;
                    item.motion = look.motion;
                    item.transition = look.transition.filter(|_| !video.is_empty());
                    item.duration += owed;
                    owed = Duration::ZERO;
                    video.push(item);
                }
                None => match video.last_mut() {
                    Some(last) => last.duration += item.duration,
                    None => owed += item.duration,
                },
            }
        }
        timeline.video = video;
        timeline.relayout();
    }
    let picture_end = timeline.video_end();
    for lane in [AudioLane::Music, AudioLane::Sfx] {
        let mut items: Vec<AudioItem> = cut
            .audio
            .iter()
            .filter(|audio| audio.lane == lane)
            .filter_map(|audio| {
                let length = media
                    .iter()
                    .find(|asset| asset.file == audio.file && asset.kind == MediaKind::Audio)?
                    .duration;
                let at = map.at(audio.at);
                let wanted = match lane {
                    AudioLane::Music => map.at(audio.until).min(picture_end).saturating_sub(at),
                    _ => audio.until.saturating_sub(audio.at),
                };
                let duration = wanted.min(length.saturating_sub(audio.start));
                (duration >= min_length()).then(|| AudioItem {
                    file: audio.file.clone(),
                    start: audio.start,
                    at,
                    duration,
                    length,
                    fade_in: audio.fade_in,
                    fade_out: audio.fade_out,
                })
            })
            .collect();
        items.sort_by_key(|item| item.at);
        // Moved closer together, an item ends where the next one starts.
        let mut kept: Vec<AudioItem> = Vec::with_capacity(items.len());
        for item in items {
            if let Some(last) = kept.last_mut()
                && last.end() > item.at
            {
                last.duration = item.at - last.at;
            }
            kept.retain(|item| item.duration >= min_length());
            kept.push(item);
        }
        match lane {
            AudioLane::Music => timeline.music = kept,
            _ => timeline.sfx = kept,
        }
    }
    timeline
}

/// Something about a placed scene worth a look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacedScene {
    /// The scene now runs past its clip, which holds its last frame.
    PastClip { clip: Duration, scene: Duration },
    /// The scene's length changed by more than half.
    Stretched { was: Duration, now: Duration },
    /// The new narration says none of its sentences.
    Dropped,
}

/// What the Scenes stage points out about each scene of `plan`, the plan
/// `localization` carried, placed on a narration.
pub fn placed_scene_notes(
    localization: &Localization,
    plan: &ScenePlan,
) -> Vec<(usize, PlacedScene)> {
    let (more, less) = STRETCH_FLAG;
    plan.scenes()
        .iter()
        .enumerate()
        .filter_map(|(index, scene)| {
            let now = scene.duration();
            if now < min_length() {
                return Some((index, PlacedScene::Dropped));
            }
            if let VideoSource::Clip { length, .. } = VideoSource::of(scene)
                && now > length + Duration::from_millis(500)
            {
                return Some((
                    index,
                    PlacedScene::PastClip {
                        clip: length,
                        scene: now,
                    },
                ));
            }
            let (from, to) = localization.source_span(index)?;
            let was = to - from;
            let stretched = now.as_millis() * u128::from(less) > was.as_millis() * u128::from(more)
                || now.as_millis() * u128::from(more) < was.as_millis() * u128::from(less);
            stretched.then_some((index, PlacedScene::Stretched { was, now }))
        })
        .collect()
}

/// Everything a new version is saved with, at once.
#[derive(Debug, Clone, PartialEq)]
pub struct NewVersion {
    /// The approved theme in the target channel and the project it starts.
    pub theme: Theme,
    pub project: VideoProject,
    /// The adapted script, and where its sentences start.
    pub script: Script,
    pub starts: Vec<usize>,
    /// The scene plan carried over, on copies of the source's files.
    pub plan: ScenePlan,
    /// Copies of the source's media, and of its generated tracks.
    pub media: Vec<MediaAsset>,
    pub tracks: Vec<MusicTrack>,
    pub localization: Localization,
}

/// Persistence port for versions. Shared with job worker threads.
pub trait LocalizationRepository: Send + Sync {
    /// What `project` is a version of, if it is one.
    fn localization(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<Localization>, RepositoryError>;

    /// The versions made from `source`, oldest first.
    fn versions(&self, source: VideoProjectId) -> Result<Vec<VideoProject>, RepositoryError>;

    /// Where each sentence starts in the adaptation `generation` wrote.
    fn adapted_starts(
        &self,
        generation: GenerationId,
    ) -> Result<Option<Vec<usize>>, RepositoryError>;

    /// Saves a new version with everything it starts with; all or none.
    fn start_version(&self, version: &NewVersion) -> Result<(), RepositoryError>;

    /// Saves `script` with where the sentences of the adaptation
    /// `generation` (its source or the one waiting) start; all or none.
    fn save_adaptation(
        &self,
        script: &Script,
        generation: GenerationId,
        starts: &[usize],
    ) -> Result<(), RepositoryError>;

    /// Makes `plan` (placed on a new narration) the version's plan and
    /// `cut` its saved cut; all or none.
    fn place_scenes(
        &self,
        plan: &ScenePlan,
        cut: &crate::SavedTimeline,
    ) -> Result<(), RepositoryError>;
}

impl<T: LocalizationRepository + ?Sized> LocalizationRepository for Arc<T> {
    fn localization(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<Localization>, RepositoryError> {
        (**self).localization(project)
    }

    fn versions(&self, source: VideoProjectId) -> Result<Vec<VideoProject>, RepositoryError> {
        (**self).versions(source)
    }

    fn adapted_starts(
        &self,
        generation: GenerationId,
    ) -> Result<Option<Vec<usize>>, RepositoryError> {
        (**self).adapted_starts(generation)
    }

    fn start_version(&self, version: &NewVersion) -> Result<(), RepositoryError> {
        (**self).start_version(version)
    }

    fn save_adaptation(
        &self,
        script: &Script,
        generation: GenerationId,
        starts: &[usize],
    ) -> Result<(), RepositoryError> {
        (**self).save_adaptation(script, generation, starts)
    }

    fn place_scenes(
        &self,
        plan: &ScenePlan,
        cut: &crate::SavedTimeline,
    ) -> Result<(), RepositoryError> {
        (**self).place_scenes(plan, cut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::{asset, generation, image, ms, plan, scene};
    use crate::{
        CropPosition, Decibels, LaneMix, MotionKind, MotionStrength, NarrationId, NarrationSource,
        Provider, SceneClip, ScriptText, TransitionKind, WordTiming, WordTimings, sentences,
    };

    /// A narration of `text`, each word `step` ms long, back to back.
    fn narrated(text: &str, step: u64) -> Narration {
        let words: Vec<WordTiming> = spoken_words(text)
            .into_iter()
            .enumerate()
            .map(|(index, range)| WordTiming {
                text: range,
                start: ms(index as u64 * step),
                end: ms((index as u64 + 1) * step),
            })
            .collect();
        let duration = ms(words.len() as u64 * step);
        Narration {
            id: NarrationId::new(),
            project: VideoProjectId::new(),
            owner: ProfileId::new(),
            text: ScriptText::new(text).unwrap(),
            source: NarrationSource::Imported {
                file_name: "take.wav".into(),
                aligner: Provider::ElevenLabs,
                model: "aligner".into(),
            },
            audio_file: "narration-2.mp3".into(),
            duration,
            words: WordTimings::restore(text, words).unwrap(),
            generated_at: SystemTime::UNIX_EPOCH,
            job: None,
        }
    }

    const SOURCE: &str = "One two. Three four.\n\nFive six seven.";

    #[test]
    fn adapted_sentences_join_with_the_source_s_spacing() {
        let source = narrated(SOURCE, 500);
        let units = sentences(&source);
        assert_eq!(units.len(), 3);
        let gaps = source_gaps(&source, &units);
        assert_eq!(gaps, [Gap::Space, Gap::BlankLine]);
        let adapted = join_adapted(
            &[
                " Um dois. ".into(),
                "Três quatro.".into(),
                "Cinco seis sete.".into(),
            ],
            &gaps,
        )
        .unwrap();
        assert_eq!(adapted.text, "Um dois. Três quatro.\n\nCinco seis sete.");
        assert_eq!(adapted.starts, [0, 9, 24]);
        assert_eq!(adaptation_gaps(&adapted.text, &adapted.starts), gaps);

        assert_eq!(
            join_adapted(&["Um.".into(), "Dois.".into()], &gaps),
            Err(AdaptationError::WrongCount {
                expected: 3,
                got: 2
            })
        );
        assert_eq!(
            join_adapted(&["Um.".into(), " ".into(), "Três.".into()], &gaps),
            Err(AdaptationError::EmptySentence(1))
        );
    }

    #[test]
    fn each_scene_starts_at_its_sentence() {
        let source = narrated(SOURCE, 500);
        let units = sentences(&source);
        // Words: One two | Three four | Five six seven, 500 ms each.
        let plan = plan(vec![scene(0, 2_000), scene(2_000, 3_500)]);
        assert_eq!(scene_sentences(&plan, &units), [0, 2]);
        let plan = crate::timeline::tests::plan(vec![
            scene(0, 1_000),
            scene(1_000, 2_000),
            scene(2_000, 3_500),
        ]);
        assert_eq!(scene_sentences(&plan, &units), [0, 1, 2]);
    }

    #[test]
    fn an_unedited_script_finds_its_sentences_word_for_word() {
        let text = "Um dois. Três quatro.\n\nCinco seis sete.";
        let narration = narrated(text, 400);
        assert_eq!(sentence_words(text, &[0, 9, 24], &narration), [0, 2, 4]);
        assert_eq!(
            sentence_times(&[0, 2, 4, 7], &narration),
            [ms(0), ms(800), ms(1_600), narration.duration]
        );
    }

    #[test]
    fn edits_to_the_script_move_the_sentences_with_their_words() {
        let text = "Um dois. Três quatro.\n\nCinco seis sete.";
        // A word added up front, the second sentence's first word dropped,
        // a word changed in the last.
        let narration = narrated("Olá, um dois. Quatro! Cinco SEIS oito.", 400);
        assert_eq!(sentence_words(text, &[0, 9, 24], &narration), [1, 3, 4]);
        // The last two sentences gone: they start where nothing is left.
        let narration = narrated("Um dois.", 400);
        assert_eq!(sentence_words(text, &[0, 9, 24], &narration), [0, 2, 2]);
    }

    #[test]
    fn far_longer_texts_match_the_middle_by_place() {
        let from: Vec<String> = (0..3_101).map(|n| format!("a{n}")).collect();
        let mut to: Vec<String> = (0..3_101).map(|n| format!("b{n}")).collect();
        to[0] = "a0".into();
        let matched = matched_words(&from, &to);
        assert_eq!(matched[0], Some(0));
        assert_eq!(matched[1_551], Some(1_551));
        assert!(matched.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    fn drawn_plan() -> ScenePlan {
        let mut records = vec![scene(0, 1_000), scene(1_000, 2_000), scene(2_000, 3_500)];
        for (index, record) in records.iter_mut().enumerate() {
            record.image = Some(image(&format!("scene-{index}.png")));
        }
        plan(records)
    }

    #[test]
    fn placed_scenes_keep_their_pictures_and_take_the_new_words() {
        let source = drawn_plan();
        let narration = narrated("Um dois três. Quatro. Cinco seis sete oito.", 500);
        let placed = place_scenes(
            &source,
            &[0, 1, 2],
            &[0, 3, 4],
            &narration,
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(placed.id, source.id);
        assert_eq!(placed.narration, narration.id);
        let spans: Vec<_> = placed.scenes().iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(
            spans,
            [
                (ms(0), ms(1_500)),
                (ms(1_500), ms(2_000)),
                (ms(2_000), ms(4_000))
            ]
        );
        let texts: Vec<_> = placed.scenes().iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Um dois três.", "Quatro.", "Cinco seis sete oito."]);
        for (old, new) in source.scenes().iter().zip(placed.scenes()) {
            assert_eq!(old.image(), new.image());
            assert_eq!(old.prompt(), new.prompt());
        }

        // A sentence the narration dropped leaves its scene without time.
        let placed = place_scenes(
            &source,
            &[0, 1, 2],
            &[0, 3, 3],
            &narration,
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(placed.scenes()[1].duration(), Duration::ZERO);
        assert_eq!(placed.scenes()[1].text, "");
        assert_eq!(placed.scenes()[2].start, ms(1_500));
    }

    #[test]
    fn the_time_map_runs_through_the_sentences() {
        let localization = localization(&drawn_plan());
        // Source sentences at 0, 1 000 and 2 000; the source ran 3 500.
        let map = localization.time_map(&[ms(0), ms(1_500), ms(2_000)], ms(4_000));
        assert_eq!(map.at(ms(500)), ms(750));
        assert_eq!(map.at(ms(1_000)), ms(1_500));
        assert_eq!(map.at(ms(2_750)), ms(3_000));
        assert_eq!(map.at(ms(3_500)), ms(4_000));
        assert_eq!(map.at(ms(4_000)), ms(4_500), "past the end, as far past");
    }

    fn localization(plan: &ScenePlan) -> Localization {
        Localization {
            project: VideoProjectId::new(),
            owner: ProfileId::new(),
            source: Some(VideoProjectId::new()),
            source_title: "Lighthouses".into(),
            source_language: ContentLanguage::English,
            source_country: Country::UnitedStates,
            source_length: ms(3_500),
            sentences: [0, 1_000, 2_000]
                .into_iter()
                .map(|start| SourceSentence {
                    text: "Words.".into(),
                    start: ms(start),
                })
                .collect(),
            plan: plan.id,
            scene_sentences: vec![0, 1, 2],
            cut: CarriedCut::of(
                &Timeline::rough_cut(plan, &narrated(SOURCE, 500), AspectRatio::Vertical),
                plan.scenes().len(),
            ),
            created_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn audio(lane: AudioLane, file: &str, at: u64, until: u64) -> CarriedAudio {
        CarriedAudio {
            lane,
            file: file.into(),
            start: Duration::ZERO,
            at: ms(at),
            until: ms(until),
            fade_in: ms(100),
            fade_out: ms(200),
        }
    }

    #[test]
    fn the_version_s_cut_carries_the_source_s_look_and_sounds() {
        let source = drawn_plan();
        let narration = narrated("Um dois três. Quatro. Cinco seis sete oito.", 500);
        let placed = place_scenes(
            &source,
            &[0, 1, 2],
            &[0, 3, 4],
            &narration,
            SystemTime::UNIX_EPOCH,
        );
        let zoom = Motion::new(MotionKind::ZoomIn, MotionStrength::Subtle);
        let fade = Transition::new(TransitionKind::FadeBlack, 10);
        let mut mix = Mix::default();
        mix.set_lane(
            AudioLane::Music,
            LaneMix {
                gain: Decibels::from_tenths(-60),
                muted: false,
                solo: false,
            },
        );
        let cut = CarriedCut {
            aspect: AspectRatio::Vertical,
            scenes: vec![
                Some(SceneLook {
                    framing: Framing::Crop(CropPosition::new(100, 900)),
                    motion: Some(zoom),
                    transition: None,
                }),
                // The source cut left the second scene out.
                None,
                Some(SceneLook {
                    framing: Framing::Fit,
                    motion: None,
                    transition: Some(fade),
                }),
            ],
            audio: vec![
                audio(AudioLane::Music, "media-music.mp3", 0, 3_500),
                audio(AudioLane::Sfx, "media-boom.wav", 1_000, 1_800),
                audio(AudioLane::Sfx, "media-hit.wav", 1_300, 1_500),
                audio(AudioLane::Sfx, "media-gone.wav", 0, 500),
            ],
            mix,
            caption_style: CaptionStyle::Punch,
            caption_highlight: CaptionHighlight::default(),
            captions_shown: false,
        };
        let map = localization(&source).time_map(&[ms(0), ms(1_500), ms(2_000)], ms(4_000));
        let media = [
            asset(MediaKind::Audio, "media-music.mp3", 60_000),
            asset(MediaKind::Audio, "media-boom.wav", 800),
            asset(MediaKind::Audio, "media-hit.wav", 200),
        ];
        let timeline = localized_cut(&placed, &narration, &cut, &map, &media);

        let video: Vec<_> = timeline
            .video()
            .iter()
            .map(|item| {
                (
                    item.scene(),
                    item.at,
                    item.duration,
                    item.framing,
                    item.motion,
                    item.transition,
                )
            })
            .collect();
        assert_eq!(
            video,
            [
                (
                    Some(0),
                    ms(0),
                    ms(2_000),
                    Framing::Crop(CropPosition::new(100, 900)),
                    Some(zoom),
                    None
                ),
                (
                    Some(2),
                    ms(2_000),
                    ms(2_000),
                    Framing::Fit,
                    None,
                    Some(fade)
                ),
            ],
            "the left-out scene's time goes to the one before"
        );
        assert_eq!(timeline.aspect(), AspectRatio::Vertical);
        assert_eq!(timeline.mix(), &mix);
        assert_eq!(timeline.captions().style(), CaptionStyle::Punch);
        assert!(!timeline.captions().shown());

        let music: Vec<_> = timeline
            .music()
            .iter()
            .map(|i| (i.at, i.duration))
            .collect();
        assert_eq!(music, [(ms(0), ms(4_000))], "start and end on the words");
        let sfx: Vec<_> = timeline
            .sfx()
            .iter()
            .map(|i| (i.file.as_str(), i.at, i.duration))
            .collect();
        assert_eq!(
            sfx,
            [
                // Moved to 1.5 s, and stopped where the next one starts.
                ("media-boom.wav", ms(1_500), ms(150)),
                ("media-hit.wav", ms(1_650), ms(200)),
            ],
            "an SFX keeps its length within its file; one without its file stays out"
        );
        assert_eq!(timeline.music()[0].fade_in, ms(100));
    }

    #[test]
    fn a_source_cut_gives_each_scene_its_first_piece_s_look() {
        let source = drawn_plan();
        let mut timeline =
            Timeline::rough_cut(&source, &narrated(SOURCE, 500), AspectRatio::Vertical);
        timeline.video[1].framing = Framing::Fit;
        timeline.video[1].transition = Some(Transition::new(TransitionKind::Crossfade, 12));
        timeline.video[0].transition = Some(Transition::new(TransitionKind::Crossfade, 12));
        timeline.video.remove(2);
        // The narration starts a second in.
        timeline.narration[0].at = ms(1_000);
        timeline.sfx.push(AudioItem {
            file: "media-boom.wav".into(),
            start: Duration::ZERO,
            at: ms(1_500),
            duration: ms(500),
            length: ms(800),
            fade_in: Duration::ZERO,
            fade_out: Duration::ZERO,
        });
        let cut = CarriedCut::of(&timeline, 3);
        assert_eq!(
            cut.scenes[0].unwrap().transition,
            None,
            "the first plays none"
        );
        assert_eq!(cut.scenes[1].unwrap().framing, Framing::Fit);
        assert_eq!(cut.scenes[2], None);
        assert_eq!(cut.audio[0].at, ms(500), "in narration time");
        assert_eq!(cut.audio[0].until, ms(1_000));
    }

    #[test]
    fn scenes_past_their_clip_or_far_longer_are_pointed_out() {
        let mut records = vec![scene(0, 1_000), scene(1_000, 2_000), scene(2_000, 3_500)];
        let picture = image("scene-1.png");
        records[1].clip = Some(SceneClip {
            file: "clip-1.mp4".into(),
            seconds: 1,
            source_image: picture.generation.id,
            generation: generation(),
        });
        records[1].image = Some(picture);
        let source = plan(records);
        let localization = localization(&source);
        let narration = narrated("Um dois três quatro. Cinco seis sete oito. Nove.", 500);
        let placed = place_scenes(
            &source,
            &[0, 1, 2],
            &[0, 4, 8],
            &narration,
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(
            placed_scene_notes(&localization, &placed),
            [
                (
                    0,
                    PlacedScene::Stretched {
                        was: ms(1_000),
                        now: ms(2_000)
                    }
                ),
                (
                    1,
                    PlacedScene::PastClip {
                        clip: ms(1_000),
                        scene: ms(2_000)
                    }
                ),
                (
                    2,
                    PlacedScene::Stretched {
                        was: ms(1_500),
                        now: ms(500)
                    }
                ),
            ]
        );
        let placed = place_scenes(
            &source,
            &[0, 1, 2],
            &[0, 4, 4],
            &narration,
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(
            placed_scene_notes(&localization, &placed)[1],
            (1, PlacedScene::Dropped)
        );
    }
}
