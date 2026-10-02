//! The editor's timeline (PRD stories 56, 57): one video track of pictures
//! back to back and the narration on its audio track. The rough cut is
//! built from the scene plan: each scene shows its clip, or its still image
//! when it has no clip, for as long as its stretch of narration lasts.
//!
//! Every cut sits on a frame boundary of [`FPS`], so clips placed back to
//! back never drift from the narration however many there are.
//!
//! The video track is magnetic: its clips always run back to back from the
//! start, so a cut never leaves a black gap, and removing or reordering a
//! clip moves the ones after it. Audio items sit where they are put, never
//! overlapping on their track, with silence between them. The user's cuts
//! (`crate::Edit`) change a timeline; a saved one ([`SavedTimeline`])
//! remembers them against the scene plan and narration they were made on.
//!
//! The timeline also carries its mix (`crate::Mix`): each audio lane's
//! level, mute and solo, the music's ducking under the narration, and each
//! audio item's fades; and its captions (`crate::Captions`), which show
//! wherever the cut plays the words they caption.
//!
//! And it carries its framing (`crate::Framing`): the shape of the frame
//! it is cut for, and how each clip fills it.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{
    AspectRatio, CaptionStyle, Captions, DuckEnvelope, Edge, Framing, Mix, Narration, NarrationId,
    ProfileId, RepositoryError, SavedCaptions, Scene, ScenePlan, ScenePlanId, VideoProjectId,
};

/// The timeline's frame rate.
pub const FPS: u32 = 30;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// The start of frame `frame`. Rounded down to the nanosecond, so the frame
/// a time falls in is always the one it was made from.
pub fn frame_time(frame: u64) -> Duration {
    let nanos = u128::from(frame) * NANOS_PER_SECOND / u128::from(FPS);
    Duration::from_nanos(nanos as u64)
}

/// The frame shown at `time`.
pub fn frame_at(time: Duration) -> u64 {
    // A time made by `frame_time` lands a hair short of its frame's exact
    // start; the nanosecond of slack puts it back in that frame.
    ((time.as_nanos() + 1) * u128::from(FPS) / NANOS_PER_SECOND) as u64
}

/// The frame boundary nearest `time`.
pub fn nearest_frame(time: Duration) -> u64 {
    ((time.as_nanos() * u128::from(FPS) + NANOS_PER_SECOND / 2) / NANOS_PER_SECOND) as u64
}

/// `time` as an editor timecode, `HH:MM:SS:FF`.
pub fn timecode(time: Duration) -> String {
    let frame = frame_at(time);
    let fps = u64::from(FPS);
    let seconds = frame / fps;
    format!(
        "{:02}:{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        frame % fps
    )
}

/// What a stretch of the video track shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoSource {
    /// A video clip in the project folder, and its length (as asked of
    /// the model that made it).
    Clip { file: String, length: Duration },
    /// A still image in the project folder, shown for the whole stretch.
    Still(String),
    /// Nothing to show yet: the scene has no image.
    Missing,
}

impl VideoSource {
    /// What `scene` shows: its clip, or its still image when it has no clip
    /// or its clip animates an image the scene no longer shows.
    pub fn of(scene: &Scene) -> VideoSource {
        match (scene.clip(), scene.image()) {
            (Some(clip), _) if !scene.is_clip_stale() => VideoSource::Clip {
                file: clip.file.clone(),
                length: Duration::from_secs(clip.seconds.into()),
            },
            (_, Some(image)) => VideoSource::Still(image.file.clone()),
            _ => VideoSource::Missing,
        }
    }

    /// The file in the project folder, if there is one.
    pub fn file(&self) -> Option<&str> {
        match self {
            VideoSource::Clip { file, .. } | VideoSource::Still(file) => Some(file),
            VideoSource::Missing => None,
        }
    }
}

/// One stretch of the video track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoItem {
    /// The scene it shows, by index in the scene plan.
    pub scene: usize,
    pub source: VideoSource,
    /// Where in the clip it starts. A still has no time of its own: its
    /// start only moves on a split, so the pieces of a still that gets
    /// animated later play on from each other.
    pub start: Duration,
    /// Where it starts on the timeline.
    pub at: Duration,
    pub duration: Duration,
    /// How its picture fills a frame of another shape.
    pub framing: Framing,
}

impl VideoItem {
    pub fn end(&self) -> Duration {
        self.at + self.duration
    }

    /// How its picture fills a frame of `aspect`: scene pictures are 16:9,
    /// so a 16:9 frame takes them whole and only 9:16 applies its framing.
    pub fn framing_in(&self, aspect: AspectRatio) -> Framing {
        match aspect {
            AspectRatio::Landscape => Framing::Fit,
            AspectRatio::Vertical => self.framing,
        }
    }

    /// Whether its source plays through time (a clip), so cutting into it
    /// moves where in the source it starts.
    pub fn has_source_time(&self) -> bool {
        matches!(self.source, VideoSource::Clip { .. })
    }

    /// Where playing starts in the source: never past the clip's last
    /// frame, which a piece cut beyond the clip's end holds.
    pub fn source_start(&self) -> Duration {
        match &self.source {
            VideoSource::Clip { length, .. } => self.start.min(length.saturating_sub(min_length())),
            _ => self.start,
        }
    }
}

/// One stretch of an audio track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioItem {
    /// The audio file in the project folder.
    pub file: String,
    /// Where in the file it starts.
    pub start: Duration,
    /// Where it starts on the timeline.
    pub at: Duration,
    pub duration: Duration,
    /// How long the whole file is: no item plays past its end.
    pub length: Duration,
    /// How long it takes to come up from silence, and to go back down to
    /// it, as the user set them.
    pub fade_in: Duration,
    pub fade_out: Duration,
}

impl AudioItem {
    pub fn end(&self) -> Duration {
        self.at + self.duration
    }

    /// The fades it plays with: as set, cut short where the item is now
    /// shorter than they are (the fade-in first).
    pub fn fades(&self) -> (Duration, Duration) {
        let fade_in = self.fade_in.min(self.duration);
        (fade_in, self.fade_out.min(self.duration - fade_in))
    }
}

/// The shortest an item can be: one frame. A cut that would leave less is
/// not made.
pub fn min_length() -> Duration {
    frame_time(1)
}

/// Where a caption shows on the timeline: one stretch of it the cut plays.
/// A caption over a cut shows in two stretches; one cut away, in none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptionSpan {
    /// The caption, by index in `Captions::lines`.
    pub index: usize,
    pub at: Duration,
    pub duration: Duration,
}

impl CaptionSpan {
    pub fn end(&self) -> Duration {
        self.at + self.duration
    }
}

/// A video project's edit: the video track, the narration track, the mix,
/// the captions and the frame's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    pub(crate) video: Vec<VideoItem>,
    pub(crate) narration: Vec<AudioItem>,
    pub(crate) mix: Mix,
    pub(crate) captions: Captions,
    pub(crate) aspect: AspectRatio,
}

impl Timeline {
    /// The first cut of a project: every scene of `plan` in order, each at
    /// its own times (moved to the nearest frame), showing its clip, or its
    /// still image when it has no clip or its clip animates an image the
    /// scene no longer shows. The video runs as long as the narration, or
    /// as the scenes when they run longer; the last scene fills to the
    /// end. Scenes too short to fill a frame are left out, and the scene
    /// before them runs on. The narration's words come captioned
    /// (`crate::caption_lines`), in the default style. The cut is 16:9,
    /// each clip filling a 9:16 frame from the middle.
    pub fn rough_cut(plan: &ScenePlan, narration: &Narration) -> Timeline {
        let end_frame = plan
            .scenes()
            .iter()
            .map(|scene| nearest_frame(scene.end))
            .chain([nearest_frame(narration.duration)])
            .max()
            .unwrap_or(0);
        let mut starts: Vec<(usize, u64)> = Vec::new();
        for (index, scene) in plan.scenes().iter().enumerate() {
            let start = if starts.is_empty() {
                0
            } else {
                nearest_frame(scene.start)
            };
            if start >= end_frame {
                break;
            }
            match starts.last_mut() {
                // Same frame as the scene before: it never shows.
                Some((last, last_start)) if *last_start == start => *last = index,
                _ => starts.push((index, start)),
            }
        }
        let video = starts
            .iter()
            .enumerate()
            .map(|(position, &(index, start))| {
                let end = starts
                    .get(position + 1)
                    .map_or(end_frame, |&(_, next)| next);
                let at = frame_time(start);
                VideoItem {
                    scene: index,
                    source: VideoSource::of(&plan.scenes()[index]),
                    start: Duration::ZERO,
                    at,
                    duration: frame_time(end) - at,
                    framing: Framing::default(),
                }
            })
            .collect();
        let captions = captions_of(narration);
        let narration = vec![AudioItem {
            file: narration.audio_file.clone(),
            start: Duration::ZERO,
            at: Duration::ZERO,
            duration: narration.duration,
            length: narration.duration,
            fade_in: Duration::ZERO,
            fade_out: Duration::ZERO,
        }];
        Timeline {
            video,
            narration,
            mix: Mix::default(),
            captions,
            aspect: AspectRatio::Landscape,
        }
    }

    /// This timeline with its captions in `style` (a channel's default, for
    /// a new cut).
    pub fn with_caption_style(mut self, style: CaptionStyle) -> Timeline {
        self.captions.style = style;
        self
    }

    /// This timeline cut for `aspect` (the shape a cut left behind had, for
    /// the one replacing it).
    pub fn with_aspect(mut self, aspect: AspectRatio) -> Timeline {
        self.aspect = aspect;
        self
    }

    /// The timeline `saved` keeps, on the scenes of `plan` as they are now
    /// (a scene's new image shows where its old one did). `None` when it was
    /// cut on another scene plan or narration, or does not hold together:
    /// the editor then starts over from the rough cut. A cut saved before
    /// captions existed gets them from the narration's words.
    pub fn restore(saved: &SavedTimeline, plan: &ScenePlan, narration: &Narration) -> Option<Self> {
        if saved.scene_plan != plan.id
            || saved.narration != narration.id
            || saved
                .narration_items
                .iter()
                .any(|item| item.file != narration.audio_file)
        {
            return None;
        }
        let captions = match &saved.captions {
            Some(saved) => Captions {
                lines: saved.lines.clone(),
                shown: saved.shown,
                style: saved.style,
                length: narration.duration,
            },
            None => captions_of(narration),
        };
        let video = saved
            .video
            .iter()
            .map(|item| VideoItem {
                scene: item.scene,
                source: plan
                    .scenes()
                    .get(item.scene)
                    .map_or(VideoSource::Missing, VideoSource::of),
                start: item.start,
                at: Duration::ZERO,
                duration: item.duration,
                framing: item.framing,
            })
            .collect();
        let narration = saved
            .narration_items
            .iter()
            .map(|item| AudioItem {
                file: item.file.clone(),
                start: item.start,
                at: item.at,
                duration: item.duration,
                length: narration.duration,
                fade_in: item.fade_in,
                fade_out: item.fade_out,
            })
            .collect();
        let mut timeline = Timeline {
            video,
            narration,
            mix: saved.mix,
            captions,
            aspect: saved.aspect,
        };
        timeline.relayout();
        timeline.holds_together().then_some(timeline)
    }

    /// What saving this timeline keeps.
    pub fn to_saved(
        &self,
        project: VideoProjectId,
        owner: ProfileId,
        scene_plan: ScenePlanId,
        narration: NarrationId,
        now: SystemTime,
    ) -> SavedTimeline {
        SavedTimeline {
            project,
            owner,
            scene_plan,
            narration,
            video: self
                .video
                .iter()
                .map(|item| SavedVideoItem {
                    scene: item.scene,
                    start: item.start,
                    duration: item.duration,
                    framing: item.framing,
                })
                .collect(),
            narration_items: self
                .narration
                .iter()
                .map(|item| SavedAudioItem {
                    file: item.file.clone(),
                    start: item.start,
                    at: item.at,
                    duration: item.duration,
                    fade_in: item.fade_in,
                    fade_out: item.fade_out,
                })
                .collect(),
            mix: self.mix,
            captions: Some(SavedCaptions {
                lines: self.captions.lines.clone(),
                shown: self.captions.shown,
                style: self.captions.style,
            }),
            aspect: self.aspect,
            updated_at: now,
        }
    }

    /// Lays the video track out back to back from the start.
    pub(crate) fn relayout(&mut self) {
        let mut at = Duration::ZERO;
        for item in &mut self.video {
            item.at = at;
            at += item.duration;
        }
    }

    /// Every item at least a frame long; audio items in order, apart, and
    /// within their file; every level of the mix in range; captions in
    /// order, apart and within the narration.
    fn holds_together(&self) -> bool {
        let video = self.video.iter().all(|item| item.duration >= min_length());
        let audio = self
            .narration
            .iter()
            .all(|item| item.duration >= min_length() && item.start + item.duration <= item.length);
        let apart = self
            .narration
            .windows(2)
            .all(|pair| pair[0].end() <= pair[1].at);
        video
            && audio
            && apart
            && self.mix.holds_together()
            && self.captions.holds_together(min_length())
    }

    pub fn video(&self) -> &[VideoItem] {
        &self.video
    }

    /// The narration track (A1).
    pub fn narration(&self) -> &[AudioItem] {
        &self.narration
    }

    pub fn mix(&self) -> &Mix {
        &self.mix
    }

    pub fn captions(&self) -> &Captions {
        &self.captions
    }

    /// The shape of the frame the cut is made for.
    pub fn aspect(&self) -> AspectRatio {
        self.aspect
    }

    /// Where the captions show: each stretch of a caption the cut plays, in
    /// timeline order. A caption over a cut shows in two stretches; one
    /// whose narration was cut away does not show.
    pub fn caption_spans(&self) -> Vec<CaptionSpan> {
        let mut spans: Vec<CaptionSpan> = self
            .narration
            .iter()
            .flat_map(|item| {
                let (from, to) = (item.start, item.start + item.duration);
                self.captions
                    .lines
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, caption)| {
                        let (start, end) = (caption.start.max(from), caption.end.min(to));
                        (start < end).then(|| CaptionSpan {
                            index,
                            at: item.at + (start - from),
                            duration: end - start,
                        })
                    })
            })
            .collect();
        spans.sort_by_key(|span| (span.at, span.index));
        spans
    }

    /// Where on the timeline caption `index` shows, first to last stretch.
    pub fn caption_hull(&self, index: usize) -> Option<(Duration, Duration)> {
        let spans = self.caption_spans();
        let mut shown = spans.iter().filter(|span| span.index == index);
        let first = shown.next()?;
        let end = shown.map(CaptionSpan::end).fold(first.end(), Duration::max);
        Some((first.at, end - first.at))
    }

    /// Where caption `index`'s `edge` shows: on the timeline, and in the
    /// narration file at that point. A caption clipped by a cut shows its
    /// edge where the clip falls, not where the caption ends in the file.
    pub fn caption_edge(&self, index: usize, edge: Edge) -> Option<(Duration, Duration)> {
        let caption = self.captions.lines.get(index)?;
        let shown = self.narration.iter().filter_map(|item| {
            let (start, end) = (
                caption.start.max(item.start),
                caption.end.min(item.start + item.duration),
            );
            (start < end).then(|| match edge {
                Edge::Start => (item.at + (start - item.start), start),
                Edge::End => (item.at + (end - item.start), end),
            })
        });
        match edge {
            Edge::Start => shown.min_by_key(|(at, _)| *at),
            Edge::End => shown.max_by_key(|(at, _)| *at),
        }
    }

    /// The end of the video track.
    pub fn video_end(&self) -> Duration {
        self.video.last().map_or(Duration::ZERO, VideoItem::end)
    }

    /// Until the last item of any track ends, on the nearest frame (as the
    /// rough cut ends); the video shows black after its own end.
    pub fn duration(&self) -> Duration {
        let audio = self
            .narration
            .iter()
            .map(|item| frame_time(nearest_frame(item.end())))
            .max()
            .unwrap_or(Duration::ZERO);
        self.video_end().max(audio)
    }

    /// Whether no track has anything left.
    pub fn is_empty(&self) -> bool {
        self.video.is_empty() && self.narration.is_empty()
    }

    /// The video item shown at `time`, if the video runs that long.
    pub fn video_at(&self, time: Duration) -> Option<usize> {
        let index = self.video.partition_point(|item| item.end() <= time);
        (index < self.video.len()).then_some(index)
    }

    /// The narration item playing at `time`, if any.
    pub fn narration_at(&self, time: Duration) -> Option<usize> {
        self.narration
            .iter()
            .position(|item| item.at <= time && time < item.end())
    }

    /// Where on the timeline the moment `source` of the narration file
    /// plays, if the cut kept it (an item's end counts as kept).
    pub fn on_narration(&self, source: Duration) -> Option<Duration> {
        self.narration
            .iter()
            .find(|item| item.start <= source && source <= item.start + item.duration)
            .map(|item| item.at + (source - item.start))
    }

    /// Where the narration's words start and end on the timeline, in
    /// order: the points cuts snap to. `words` are times in the narration
    /// file; those the cut left out have no place.
    pub fn word_boundaries(
        &self,
        words: impl IntoIterator<Item = (Duration, Duration)>,
    ) -> Vec<Duration> {
        let mut boundaries: Vec<Duration> = words
            .into_iter()
            .flat_map(|(start, end)| [start, end])
            .filter_map(|time| self.on_narration(time))
            .collect();
        boundaries.sort();
        boundaries.dedup();
        boundaries
    }

    /// Where on the timeline the narration speaks: each of `words` (times
    /// in the narration file) where the cut plays it, as much of it as the
    /// cut keeps. A word cut in two plays in two stretches.
    pub fn speech(
        &self,
        words: impl IntoIterator<Item = (Duration, Duration)>,
    ) -> Vec<(Duration, Duration)> {
        let words: Vec<(Duration, Duration)> = words.into_iter().collect();
        let mut speech: Vec<(Duration, Duration)> = self
            .narration
            .iter()
            .flat_map(|item| {
                let (from, to) = (item.start, item.start + item.duration);
                words.iter().filter_map(move |&(start, end)| {
                    let (start, end) = (start.max(from), end.min(to));
                    (start < end).then(|| (item.at + (start - from), item.at + (end - from)))
                })
            })
            .collect();
        speech.sort();
        speech
    }

    /// How the music ducks under the narration's `words`, when ducking is
    /// on. It follows the narration whether or not the narration lane is
    /// heard, so soloing the music plays it as the mix will.
    pub fn ducking(
        &self,
        words: impl IntoIterator<Item = (Duration, Duration)>,
    ) -> Option<DuckEnvelope> {
        let ducking = self.mix.ducking;
        ducking
            .on
            .then(|| DuckEnvelope::under(&self.speech(words), ducking.depth))
    }

    /// Every video and audio file the timeline plays, each once, in order.
    pub fn files(&self) -> Vec<&str> {
        let mut files: Vec<&str> = Vec::new();
        let all = self
            .video
            .iter()
            .filter_map(|item| item.source.file())
            .chain(self.narration.iter().map(|item| item.file.as_str()));
        for file in all {
            if !files.contains(&file) {
                files.push(file);
            }
        }
        files
    }
}

/// The captions a narration's words make.
fn captions_of(narration: &Narration) -> Captions {
    Captions::from_words(
        narration
            .words()
            .map(|(text, timing)| (text, timing.start, timing.end)),
        narration.duration,
    )
}

/// The target nearest `time` within `within` of it, if any: where a cut
/// snaps.
pub fn snap(time: Duration, targets: &[Duration], within: Duration) -> Option<Duration> {
    let distance = |target: &Duration| target.abs_diff(time);
    targets
        .iter()
        .filter(|target| distance(target) <= within)
        .min_by_key(|target| distance(target))
        .copied()
}

/// A video item as saved: the scene it shows, not the file, so a scene's
/// new image takes its place in the cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedVideoItem {
    pub scene: usize,
    pub start: Duration,
    pub duration: Duration,
    pub framing: Framing,
}

/// An audio item as saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAudioItem {
    pub file: String,
    pub start: Duration,
    pub at: Duration,
    pub duration: Duration,
    pub fade_in: Duration,
    pub fade_out: Duration,
}

/// A video project's cut as the user left it, on the scene plan and the
/// narration it was made on. A new plan or narration leaves it behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedTimeline {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    pub scene_plan: ScenePlanId,
    pub narration: NarrationId,
    /// In order; positions follow from the lengths (the track is magnetic).
    pub video: Vec<SavedVideoItem>,
    /// The narration track (A1), in order.
    pub narration_items: Vec<SavedAudioItem>,
    pub mix: Mix,
    /// `None` for a cut saved before captions existed.
    pub captions: Option<SavedCaptions>,
    /// The shape of the frame the cut is made for.
    pub aspect: AspectRatio,
    pub updated_at: SystemTime,
}

/// Persistence port for edited timelines.
pub trait TimelineRepository: Send + Sync {
    /// The project's saved cut, if it was ever edited.
    fn saved_timeline(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<SavedTimeline>, RepositoryError>;

    /// Makes `timeline` the project's saved cut, replacing any other; all or
    /// none.
    fn save_timeline(&self, timeline: &SavedTimeline) -> Result<(), RepositoryError>;
}

impl<T: TimelineRepository + ?Sized> TimelineRepository for Arc<T> {
    fn saved_timeline(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<SavedTimeline>, RepositoryError> {
        (**self).saved_timeline(project)
    }

    fn save_timeline(&self, timeline: &SavedTimeline) -> Result<(), RepositoryError> {
        (**self).save_timeline(timeline)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        Generation, GenerationId, JobFailureKind, NarrationId, NarrationSource, ProfileId,
        Provider, SceneClip, SceneImage, ScenePlanId, ScenePlanRecord, ScenePrompt, SceneRecord,
        ScriptText, TemplateUsed, TemplateVersionId, TokenUsage, VideoProjectId, WordTimings,
    };

    pub(crate) fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    pub(crate) fn generation() -> Generation {
        Generation {
            id: GenerationId::new(),
            owner: ProfileId::new(),
            project: VideoProjectId::new(),
            provider: Provider::Gemini,
            model: "nano-banana".into(),
            template: TemplateUsed {
                id: TemplateVersionId::new(),
                number: 1,
            },
            instructions: String::new(),
            prompt: "a lighthouse".into(),
            output: String::new(),
            usage: TokenUsage::default(),
            generated_at: SystemTime::UNIX_EPOCH,
            job: None,
        }
    }

    pub(crate) fn image(file: &str) -> SceneImage {
        SceneImage {
            file: file.into(),
            generation: generation(),
        }
    }

    pub(crate) fn scene(start: u64, end: u64) -> SceneRecord {
        let prompt = ScenePrompt::new("a lighthouse at dusk").unwrap();
        SceneRecord {
            start: ms(start),
            end: ms(end),
            text: "Words.".into(),
            generated_prompt: prompt.clone(),
            prompt,
            image: None,
            pending: None,
            failure: None,
            motion_prompt: None,
            clip_model: None,
            clip: None,
            pending_clip: None,
            clip_failure: None,
        }
    }

    pub(crate) fn plan(scenes: Vec<SceneRecord>) -> ScenePlan {
        let generation = generation();
        ScenePlan::restore(ScenePlanRecord {
            id: ScenePlanId::new(),
            project: generation.project,
            owner: generation.owner,
            narration: NarrationId::new(),
            generation,
            scenes: scenes.into_iter().map(crate::Scene::restore).collect(),
            updated_at: SystemTime::UNIX_EPOCH,
        })
    }

    pub(crate) fn narration(duration: u64) -> Narration {
        Narration {
            id: NarrationId::new(),
            project: VideoProjectId::new(),
            owner: ProfileId::new(),
            text: ScriptText::new("Words.").unwrap(),
            source: NarrationSource::Imported {
                file_name: "take.wav".into(),
                aligner: Provider::ElevenLabs,
                model: "aligner".into(),
            },
            audio_file: "narration-1.mp3".into(),
            duration: ms(duration),
            words: WordTimings::default(),
            generated_at: SystemTime::UNIX_EPOCH,
            job: None,
        }
    }

    #[test]
    fn frames_round_trip_through_time() {
        for frame in [0, 1, 29, 30, 31, 1_799, 1_800, 108_000] {
            assert_eq!(frame_at(frame_time(frame)), frame, "{frame}");
            assert_eq!(nearest_frame(frame_time(frame)), frame, "{frame}");
        }
        assert_eq!(frame_at(ms(1_000)), 30);
        assert_eq!(frame_at(ms(1_049)), 31);
        assert_eq!(nearest_frame(ms(1_049)), 31);
        assert_eq!(nearest_frame(ms(1_051)), 32);
    }

    #[test]
    fn timecodes_count_hours_minutes_seconds_and_frames() {
        assert_eq!(timecode(Duration::ZERO), "00:00:00:00");
        assert_eq!(timecode(frame_time(29)), "00:00:00:29");
        assert_eq!(timecode(ms(64_500)), "00:01:04:15");
        assert_eq!(timecode(Duration::from_secs(3_725)), "01:02:05:00");
    }

    #[test]
    fn rough_cut_places_each_scene_at_its_times() {
        let mut first = scene(0, 2_000);
        first.image = Some(image("scene-a.png"));
        let mut second = scene(2_000, 5_500);
        second.image = Some(image("scene-b.png"));
        let timeline = Timeline::rough_cut(&plan(vec![first, second]), &narration(5_500));

        let video = timeline.video();
        assert_eq!(video.len(), 2);
        assert_eq!(video[0].scene, 0);
        assert_eq!(video[0].source, VideoSource::Still("scene-a.png".into()));
        assert_eq!((video[0].at, video[0].duration), (ms(0), ms(2_000)));
        assert_eq!(video[1].at, ms(2_000));
        assert_eq!(video[1].end(), ms(5_500));
        assert_eq!(timeline.duration(), ms(5_500));
        assert_eq!(
            timeline.narration(),
            &[AudioItem {
                file: "narration-1.mp3".into(),
                start: Duration::ZERO,
                at: Duration::ZERO,
                duration: ms(5_500),
                length: ms(5_500),
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
            }]
        );
    }

    #[test]
    fn cuts_move_to_the_nearest_frame_and_leave_no_gaps() {
        let scenes = vec![scene(0, 1_010), scene(1_010, 2_049), scene(2_049, 3_333)];
        let timeline = Timeline::rough_cut(&plan(scenes), &narration(3_333));
        let video = timeline.video();
        assert_eq!(video[1].at, frame_time(30));
        assert_eq!(video[2].at, frame_time(61));
        assert_eq!(timeline.duration(), frame_time(100));
        for pair in video.windows(2) {
            assert_eq!(pair[0].end(), pair[1].at);
        }
        for item in video {
            assert_eq!(frame_time(frame_at(item.at)), item.at);
        }
    }

    #[test]
    fn a_clip_wins_over_the_still_unless_it_animates_another_image() {
        let mut animated = scene(0, 1_000);
        let drawn = image("scene-a.png");
        let mut clip = SceneClip {
            file: "clip-a.mp4".into(),
            seconds: 5,
            source_image: drawn.generation.id,
            generation: generation(),
        };
        animated.image = Some(drawn);
        animated.clip = Some(clip.clone());
        let mut redrawn = scene(1_000, 2_000);
        redrawn.image = Some(image("scene-b.png"));
        clip.file = "clip-b.mp4".into();
        redrawn.clip = Some(clip);
        let timeline = Timeline::rough_cut(&plan(vec![animated, redrawn]), &narration(2_000));
        assert_eq!(
            timeline.video()[0].source,
            VideoSource::Clip {
                file: "clip-a.mp4".into(),
                length: Duration::from_secs(5),
            }
        );
        assert_eq!(
            timeline.video()[1].source,
            VideoSource::Still("scene-b.png".into())
        );
    }

    #[test]
    fn a_scene_without_an_image_is_missing_media() {
        let mut failed = scene(0, 1_000);
        failed.failure = Some(JobFailureKind::Declined);
        let timeline = Timeline::rough_cut(&plan(vec![failed]), &narration(1_000));
        assert_eq!(timeline.video()[0].source, VideoSource::Missing);
        assert_eq!(timeline.files(), vec!["narration-1.mp3"]);
    }

    #[test]
    fn the_last_scene_runs_to_the_end_of_the_narration() {
        let timeline = Timeline::rough_cut(&plan(vec![scene(0, 1_000)]), &narration(4_000));
        assert_eq!(timeline.duration(), ms(4_000));
        let timeline = Timeline::rough_cut(&plan(vec![scene(0, 6_000)]), &narration(4_000));
        assert_eq!(timeline.duration(), ms(6_000));
    }

    #[test]
    fn scenes_shorter_than_a_frame_are_left_out() {
        let scenes = vec![scene(0, 1_000), scene(1_000, 1_010), scene(1_010, 2_000)];
        let timeline = Timeline::rough_cut(&plan(scenes), &narration(2_000));
        let shown: Vec<usize> = timeline.video().iter().map(|item| item.scene).collect();
        assert_eq!(shown, vec![0, 2]);
        assert_eq!(timeline.video()[1].at, ms(1_000));
    }

    #[test]
    fn video_at_finds_the_item_under_the_playhead() {
        let scenes = vec![scene(0, 1_000), scene(1_000, 2_000)];
        let timeline = Timeline::rough_cut(&plan(scenes), &narration(2_000));
        assert_eq!(timeline.video_at(ms(0)), Some(0));
        assert_eq!(timeline.video_at(ms(999)), Some(0));
        assert_eq!(timeline.video_at(ms(1_000)), Some(1));
        assert_eq!(timeline.video_at(ms(1_999)), Some(1));
        assert_eq!(timeline.video_at(ms(2_000)), None, "past the video's end");
    }

    #[test]
    fn files_lists_each_file_once() {
        let mut first = scene(0, 1_000);
        first.image = Some(image("scene-a.png"));
        let mut second = scene(1_000, 2_000);
        second.image = Some(image("scene-a.png"));
        let timeline = Timeline::rough_cut(&plan(vec![first, second]), &narration(2_000));
        assert_eq!(timeline.files(), vec!["scene-a.png", "narration-1.mp3"]);
    }

    #[test]
    fn the_timeline_lasts_until_its_last_item_ends() {
        let mut timeline = Timeline::rough_cut(&plan(vec![scene(0, 1_000)]), &narration(1_000));
        assert_eq!(timeline.duration(), ms(1_000));
        timeline.narration[0].at = ms(500);
        assert_eq!(timeline.video_end(), ms(1_000));
        assert_eq!(
            timeline.duration(),
            ms(1_500),
            "the narration runs past the video"
        );
        timeline.video.clear();
        assert!(!timeline.is_empty());
        timeline.narration.clear();
        assert!(timeline.is_empty());
    }

    #[test]
    fn narration_times_follow_the_cut() {
        let mut timeline = Timeline::rough_cut(&plan(vec![scene(0, 4_000)]), &narration(4_000));
        // Two pieces of the file: 0-1 s plays at 0, 2-4 s plays at 1 s.
        timeline.narration = vec![
            AudioItem {
                file: "narration-1.mp3".into(),
                start: ms(0),
                at: ms(0),
                duration: ms(1_000),
                length: ms(4_000),
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
            },
            AudioItem {
                file: "narration-1.mp3".into(),
                start: ms(2_000),
                at: ms(1_000),
                duration: ms(2_000),
                length: ms(4_000),
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
            },
        ];
        assert_eq!(timeline.on_narration(ms(500)), Some(ms(500)));
        assert_eq!(timeline.on_narration(ms(1_500)), None, "cut out");
        assert_eq!(timeline.on_narration(ms(2_500)), Some(ms(1_500)));
        assert_eq!(
            timeline.on_narration(ms(4_000)),
            Some(ms(3_000)),
            "an end counts"
        );
        assert_eq!(timeline.narration_at(ms(999)), Some(0));
        assert_eq!(timeline.narration_at(ms(1_000)), Some(1));
        assert_eq!(timeline.narration_at(ms(3_000)), None);

        let words = [
            (ms(100), ms(400)),
            (ms(1_200), ms(1_800)),
            (ms(2_100), ms(2_600)),
        ];
        assert_eq!(
            timeline.word_boundaries(words),
            vec![ms(100), ms(400), ms(1_100), ms(1_600)]
        );
    }

    #[test]
    fn snap_finds_the_nearest_target_within_reach() {
        let targets = [ms(1_000), ms(1_300), ms(2_000)];
        assert_eq!(snap(ms(1_100), &targets, ms(150)), Some(ms(1_000)));
        assert_eq!(snap(ms(1_200), &targets, ms(150)), Some(ms(1_300)));
        assert_eq!(snap(ms(1_600), &targets, ms(150)), None);
        assert_eq!(snap(ms(1_000), &targets, Duration::ZERO), Some(ms(1_000)));
        assert_eq!(snap(ms(1_001), &[], ms(150)), None);
    }

    fn drawn(scenes: &[(u64, u64, &str)]) -> ScenePlan {
        plan(
            scenes
                .iter()
                .map(|&(start, end, file)| {
                    let mut record = scene(start, end);
                    record.image = Some(image(file));
                    record
                })
                .collect(),
        )
    }

    fn saved(timeline: &Timeline, plan: &ScenePlan, narration: &Narration) -> SavedTimeline {
        timeline.to_saved(
            plan.project,
            plan.owner,
            plan.id,
            narration.id,
            SystemTime::UNIX_EPOCH,
        )
    }

    #[test]
    fn a_saved_timeline_comes_back_as_it_was_cut() {
        let plan = drawn(&[(0, 2_000, "a.png"), (2_000, 4_000, "b.png")]);
        let narration = narration(4_000);
        let mut timeline = Timeline::rough_cut(&plan, &narration);
        timeline.video.swap(0, 1);
        timeline.video[0].duration = ms(1_500);
        timeline.relayout();
        timeline.narration[0].at = ms(100);
        timeline.narration[0].duration = ms(3_900);

        let saved = saved(&timeline, &plan, &narration);
        assert_eq!(saved.video[0].scene, 1);
        assert_eq!(
            Timeline::restore(&saved, &plan, &narration),
            Some(timeline.clone())
        );
    }

    #[test]
    fn a_restored_timeline_shows_each_scene_as_it_is_now() {
        let plan = drawn(&[(0, 2_000, "a.png"), (2_000, 4_000, "b.png")]);
        let narration = narration(4_000);
        let timeline = Timeline::rough_cut(&plan, &narration);
        let saved = saved(&timeline, &plan, &narration);

        let mut redrawn = plan.clone();
        let scene = redrawn.scene_mut(1, SystemTime::UNIX_EPOCH).unwrap();
        scene.add_image(image("b2.png"));
        scene.accept_image().unwrap();
        let restored = Timeline::restore(&saved, &redrawn, &narration).unwrap();
        assert_eq!(
            restored.video()[1].source,
            VideoSource::Still("b2.png".into())
        );

        let mut short = saved.clone();
        short.video[1].scene = 7;
        let restored = Timeline::restore(&short, &plan, &narration).unwrap();
        assert_eq!(restored.video()[1].source, VideoSource::Missing);
    }

    #[test]
    fn a_saved_timeline_on_another_plan_or_narration_is_left_behind() {
        let plan = drawn(&[(0, 2_000, "a.png")]);
        let narration = narration(2_000);
        let saved = saved(&Timeline::rough_cut(&plan, &narration), &plan, &narration);
        let replanned = drawn(&[(0, 2_000, "a.png")]);
        assert_eq!(Timeline::restore(&saved, &replanned, &narration), None);
        let renarrated = self::narration(2_000);
        assert_eq!(Timeline::restore(&saved, &plan, &renarrated), None);
    }

    #[test]
    fn a_saved_timeline_that_does_not_hold_together_is_left_behind() {
        let plan = drawn(&[(0, 2_000, "a.png")]);
        let narration = narration(2_000);
        let mut saved = saved(&Timeline::rough_cut(&plan, &narration), &plan, &narration);
        saved.narration_items[0].duration = ms(2_500);
        assert_eq!(
            Timeline::restore(&saved, &plan, &narration),
            None,
            "past the file"
        );
        let mut saved = saved.clone();
        saved.narration_items[0].duration = ms(1_000);
        let mut second = saved.narration_items[0].clone();
        second.at = ms(500);
        saved.narration_items.push(second);
        assert_eq!(
            Timeline::restore(&saved, &plan, &narration),
            None,
            "overlapping"
        );
        let mut saved = saved.clone();
        saved.narration_items.truncate(1);
        saved.narration_items[0].file = "../elsewhere.mp3".into();
        assert_eq!(
            Timeline::restore(&saved, &plan, &narration),
            None,
            "another file"
        );
    }

    #[test]
    fn fades_are_cut_short_on_an_item_shorter_than_them() {
        let mut timeline = Timeline::rough_cut(&plan(vec![scene(0, 4_000)]), &narration(4_000));
        let item = &mut timeline.narration[0];
        item.fade_in = ms(1_000);
        item.fade_out = ms(2_000);
        assert_eq!(item.fades(), (ms(1_000), ms(2_000)));
        item.duration = ms(2_500);
        assert_eq!(
            item.fades(),
            (ms(1_000), ms(1_500)),
            "the fade-out is cut short first"
        );
        item.duration = ms(600);
        assert_eq!(item.fades(), (ms(600), Duration::ZERO));
    }

    #[test]
    fn speech_is_where_the_cut_plays_each_word() {
        let mut timeline = Timeline::rough_cut(&plan(vec![scene(0, 4_000)]), &narration(4_000));
        // 0-1 s of the file at 0, 2-4 s of it at 1 s.
        timeline.narration = vec![
            AudioItem {
                file: "narration-1.mp3".into(),
                start: ms(0),
                at: ms(0),
                duration: ms(1_000),
                length: ms(4_000),
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
            },
            AudioItem {
                file: "narration-1.mp3".into(),
                start: ms(2_000),
                at: ms(1_000),
                duration: ms(2_000),
                length: ms(4_000),
                fade_in: Duration::ZERO,
                fade_out: Duration::ZERO,
            },
        ];
        let words = [
            (ms(100), ms(400)),
            (ms(800), ms(2_300)),
            (ms(1_200), ms(1_800)),
            (ms(3_500), ms(4_000)),
        ];
        assert_eq!(
            timeline.speech(words),
            vec![
                (ms(100), ms(400)),
                (ms(800), ms(1_000)),
                (ms(1_000), ms(1_300)),
                (ms(2_500), ms(3_000)),
            ],
            "a word cut in two plays in two stretches; one cut away is gone"
        );
    }

    #[test]
    fn the_music_ducks_under_the_words_while_ducking_is_on() {
        let mut timeline = Timeline::rough_cut(&plan(vec![scene(0, 4_000)]), &narration(4_000));
        let words = [(ms(1_000), ms(1_500)), (ms(1_600), ms(2_000))];
        let envelope = timeline.ducking(words).unwrap();
        assert_eq!(envelope.depth, crate::DEFAULT_DUCK);
        assert_eq!(envelope.dips.len(), 1);
        assert_eq!(
            (envelope.dips[0].full, envelope.dips[0].release),
            (ms(1_000), ms(2_000))
        );
        // Muting the narration does not change how the music ducks.
        timeline.mix.lane_mut(crate::AudioLane::Narration).muted = true;
        assert_eq!(timeline.ducking(words), Some(envelope));
        timeline.mix.ducking.on = false;
        assert_eq!(timeline.ducking(words), None);
    }

    /// A narration of `text` whose words are timed `words` (ms).
    pub(crate) fn narrated(text: &str, words: &[(u64, u64)], duration: u64) -> Narration {
        let mut narrated = narration(duration);
        narrated.text = ScriptText::new(text).unwrap();
        let timings = crate::spoken_words(text)
            .into_iter()
            .zip(words)
            .map(|(range, &(start, end))| crate::WordTiming {
                text: range,
                start: ms(start),
                end: ms(end),
            })
            .collect();
        narrated.words = WordTimings::restore(text, timings).unwrap();
        narrated
    }

    #[test]
    fn captions_from_odd_word_timings_still_hold_together() {
        let plan = plan(vec![scene(0, 2_000)]);
        for words in [
            // A word with no length.
            [(500, 500), (600, 900), (900, 1_200), (1_200, 1_500)],
            // A line shorter than a frame.
            [(500, 520), (600, 900), (900, 1_200), (1_200, 1_500)],
            // Lines that overlap.
            [(0, 1_000), (500, 900), (900, 1_200), (1_200, 1_500)],
            // The last word past the end of the file.
            [(0, 500), (600, 900), (900, 1_200), (1_200, 2_100)],
        ] {
            let narration = narrated("Oh. Then the rest.", &words, 2_000);
            let timeline = Timeline::rough_cut(&plan, &narration);
            assert!(
                timeline.captions().holds_together(min_length()),
                "{words:?}"
            );
            let saved = saved(&timeline, &plan, &narration);
            assert_eq!(
                Timeline::restore(&saved, &plan, &narration),
                Some(timeline),
                "{words:?}"
            );
        }
    }

    #[test]
    fn the_rough_cut_captions_the_narrated_words() {
        let narration = narrated(
            "The keeper woke. He lit the lamp.",
            &[
                (0, 300),
                (300, 600),
                (600, 1_000),
                (1_200, 1_400),
                (1_400, 1_600),
                (1_600, 1_800),
                (1_800, 2_200),
            ],
            2_500,
        );
        let timeline = Timeline::rough_cut(&plan(vec![scene(0, 2_500)]), &narration);
        let captions = timeline.captions();
        assert!(captions.shown());
        assert_eq!(captions.style(), CaptionStyle::Clean);
        assert_eq!(
            captions.lines(),
            &[
                crate::Caption {
                    text: "The keeper woke.".into(),
                    start: ms(0),
                    end: ms(1_000),
                },
                crate::Caption {
                    text: "He lit the lamp.".into(),
                    start: ms(1_200),
                    end: ms(2_200),
                },
            ]
        );
        let styled = timeline.with_caption_style(CaptionStyle::Punch);
        assert_eq!(styled.captions().style(), CaptionStyle::Punch);
    }

    #[test]
    fn captions_show_where_the_cut_plays_their_words() {
        let narration = narrated(
            "One. Two. Three.",
            &[(0, 500), (1_000, 1_500), (2_000, 2_500)],
            3_000,
        );
        let mut timeline = Timeline::rough_cut(&plan(vec![scene(0, 3_000)]), &narration);
        let spans = |timeline: &Timeline| -> Vec<(usize, Duration, Duration)> {
            timeline
                .caption_spans()
                .iter()
                .map(|span| (span.index, span.at, span.duration))
                .collect()
        };
        assert_eq!(
            spans(&timeline),
            vec![
                (0, ms(0), ms(500)),
                (1, ms(1_000), ms(500)),
                (2, ms(2_000), ms(500))
            ]
        );
        // 0-1.2 s of the file at 0, then 2.2-3 s of it at 2 s: "Two" is cut
        // short, "Three" partly cut away and later.
        let item = timeline.narration[0].clone();
        timeline.narration = vec![
            AudioItem {
                duration: ms(1_200),
                ..item.clone()
            },
            AudioItem {
                start: ms(2_200),
                at: ms(2_000),
                duration: ms(800),
                ..item
            },
        ];
        assert_eq!(
            spans(&timeline),
            vec![
                (0, ms(0), ms(500)),
                (1, ms(1_000), ms(200)),
                (2, ms(2_000), ms(300))
            ]
        );
        assert_eq!(timeline.caption_hull(1), Some((ms(1_000), ms(200))));
        // Cutting the last piece away takes "Three" with it.
        timeline.narration.truncate(1);
        assert_eq!(spans(&timeline).len(), 2);
        assert_eq!(timeline.caption_hull(2), None);
    }

    #[test]
    fn a_caption_over_a_cut_shows_on_both_sides_of_it() {
        let narration = narrated("Slowly spoken", &[(0, 1_000), (1_000, 2_000)], 2_000);
        let mut timeline = Timeline::rough_cut(&plan(vec![scene(0, 2_000)]), &narration);
        let item = timeline.narration[0].clone();
        timeline.narration = vec![
            AudioItem {
                duration: ms(500),
                ..item.clone()
            },
            AudioItem {
                start: ms(1_500),
                at: ms(1_000),
                duration: ms(500),
                ..item
            },
        ];
        let spans = timeline.caption_spans();
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[1].at, spans[1].end()), (ms(1_000), ms(1_500)));
        assert_eq!(timeline.caption_hull(0), Some((ms(0), ms(1_500))));
    }

    #[test]
    fn a_saved_timeline_keeps_its_captions() {
        let plan = drawn(&[(0, 2_000, "a.png")]);
        let narration = narrated("One. Two.", &[(0, 500), (1_000, 1_500)], 2_000);
        let mut timeline =
            Timeline::rough_cut(&plan, &narration).with_caption_style(CaptionStyle::Boxed);
        timeline.captions.lines[0].text = "Uno.".into();
        timeline.captions.shown = false;
        let saved = saved(&timeline, &plan, &narration);
        assert_eq!(
            saved
                .captions
                .as_ref()
                .map(|c| (c.shown, c.style, c.lines.len())),
            Some((false, CaptionStyle::Boxed, 2))
        );
        assert_eq!(
            Timeline::restore(&saved, &plan, &narration),
            Some(timeline.clone())
        );

        // A cut saved before captions gets them from the words.
        let mut older = saved.clone();
        older.captions = None;
        let restored = Timeline::restore(&older, &plan, &narration).unwrap();
        assert_eq!(texts(restored.captions()), ["One.", "Two."]);
        assert!(restored.captions().shown());

        let mut broken = saved.clone();
        broken.captions.as_mut().unwrap().lines[1].end = ms(2_500);
        assert_eq!(
            Timeline::restore(&broken, &plan, &narration),
            None,
            "a caption past the narration"
        );
    }

    fn texts(captions: &Captions) -> Vec<&str> {
        captions.lines().iter().map(|c| c.text.as_str()).collect()
    }

    #[test]
    fn a_saved_timeline_keeps_its_mix_and_fades() {
        let plan = drawn(&[(0, 2_000, "a.png")]);
        let narration = narration(2_000);
        let mut timeline = Timeline::rough_cut(&plan, &narration);
        timeline.narration[0].fade_in = ms(300);
        timeline.narration[0].fade_out = ms(700);
        timeline.mix.lane_mut(crate::AudioLane::Music).gain = crate::Decibels::from_tenths(-60);
        timeline.mix.lane_mut(crate::AudioLane::Sfx).solo = true;
        timeline.mix.ducking.depth = crate::Decibels::from_tenths(180);

        let saved = saved(&timeline, &plan, &narration);
        assert_eq!(saved.narration_items[0].fade_out, ms(700));
        assert_eq!(saved.mix, timeline.mix);
        assert_eq!(Timeline::restore(&saved, &plan, &narration), Some(timeline));

        let mut loud = saved.clone();
        loud.mix.lane_mut(crate::AudioLane::Music).gain = crate::Decibels::from_tenths(500);
        assert_eq!(
            Timeline::restore(&loud, &plan, &narration),
            None,
            "a level out of range"
        );
    }

    #[test]
    fn a_saved_timeline_keeps_its_frame_shape_and_crops() {
        let plan = drawn(&[(0, 2_000, "a.png"), (2_000, 4_000, "b.png")]);
        let narration = narration(4_000);
        let rough = Timeline::rough_cut(&plan, &narration);
        assert_eq!(rough.aspect(), AspectRatio::Landscape);
        assert!(
            rough
                .video()
                .iter()
                .all(|item| item.framing == Framing::FILL)
        );

        let mut timeline = rough.with_aspect(AspectRatio::Vertical);
        let left = Framing::Crop(crate::CropPosition::new(0, 500));
        timeline.video[0].framing = left;
        timeline.video[1].framing = Framing::Fit;
        let saved = saved(&timeline, &plan, &narration);
        assert_eq!(saved.aspect, AspectRatio::Vertical);
        assert_eq!(saved.video[0].framing, left);
        assert_eq!(Timeline::restore(&saved, &plan, &narration), Some(timeline));
    }

    #[test]
    fn a_16_9_frame_takes_every_picture_whole() {
        let plan = drawn(&[(0, 2_000, "a.png")]);
        let mut item = Timeline::rough_cut(&plan, &narration(2_000)).video[0].clone();
        item.framing = Framing::Crop(crate::CropPosition::new(0, 0));
        assert_eq!(item.framing_in(AspectRatio::Landscape), Framing::Fit);
        assert_eq!(item.framing_in(AspectRatio::Vertical), item.framing);
    }
}
