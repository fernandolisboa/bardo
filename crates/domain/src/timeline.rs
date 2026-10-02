//! The editor's timeline (PRD stories 56, 57): one video track of pictures
//! back to back and the narration on its audio track. The rough cut is
//! built from the scene plan: each scene shows its clip, or its still image
//! when it has no clip, for as long as its stretch of narration lasts.
//!
//! Every cut sits on a frame boundary of [`FPS`], so clips placed back to
//! back never drift from the narration however many there are.

use std::time::Duration;

use crate::{Narration, ScenePlan};

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
    /// A video clip in the project folder.
    Clip(String),
    /// A still image in the project folder, shown for the whole stretch.
    Still(String),
    /// Nothing to show yet: the scene has no image.
    Missing,
}

impl VideoSource {
    /// The file in the project folder, if there is one.
    pub fn file(&self) -> Option<&str> {
        match self {
            VideoSource::Clip(file) | VideoSource::Still(file) => Some(file),
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
    /// Where it starts on the timeline.
    pub at: Duration,
    pub duration: Duration,
}

impl VideoItem {
    pub fn end(&self) -> Duration {
        self.at + self.duration
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
}

/// A video project's edit: the video track and the narration track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    video: Vec<VideoItem>,
    narration: Vec<AudioItem>,
}

impl Timeline {
    /// The first cut of a project: every scene of `plan` in order, each at
    /// its own times (moved to the nearest frame), showing its clip, or its
    /// still image when it has no clip or its clip animates an image the
    /// scene no longer shows. The video runs as long as the narration, or
    /// as the scenes when they run longer; the last scene fills to the
    /// end. Scenes too short to fill a frame are left out, and the scene
    /// before them runs on.
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
                let scene = &plan.scenes()[index];
                let source = match (scene.clip(), scene.image()) {
                    (Some(clip), _) if !scene.is_clip_stale() => {
                        VideoSource::Clip(clip.file.clone())
                    }
                    (_, Some(image)) => VideoSource::Still(image.file.clone()),
                    _ => VideoSource::Missing,
                };
                let at = frame_time(start);
                VideoItem {
                    scene: index,
                    source,
                    at,
                    duration: frame_time(end) - at,
                }
            })
            .collect();
        let narration = vec![AudioItem {
            file: narration.audio_file.clone(),
            start: Duration::ZERO,
            at: Duration::ZERO,
            duration: narration.duration,
        }];
        Timeline { video, narration }
    }

    pub fn video(&self) -> &[VideoItem] {
        &self.video
    }

    /// The narration track (A1).
    pub fn narration(&self) -> &[AudioItem] {
        &self.narration
    }

    /// As long as the video track.
    pub fn duration(&self) -> Duration {
        self.video.last().map_or(Duration::ZERO, VideoItem::end)
    }

    pub fn is_empty(&self) -> bool {
        self.video.is_empty()
    }

    /// The video item shown at `time`; the last one at or past the end.
    pub fn video_at(&self, time: Duration) -> Option<usize> {
        if self.video.is_empty() {
            return None;
        }
        let index = self.video.partition_point(|item| item.end() <= time);
        Some(index.min(self.video.len() - 1))
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

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;
    use crate::{
        Generation, GenerationId, JobFailureKind, NarrationId, NarrationSource, ProfileId,
        Provider, SceneClip, SceneImage, ScenePlanId, ScenePlanRecord, ScenePrompt, SceneRecord,
        ScriptText, TemplateUsed, TemplateVersionId, TokenUsage, VideoProjectId, WordTimings,
    };

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn generation() -> Generation {
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

    fn image(file: &str) -> SceneImage {
        SceneImage {
            file: file.into(),
            generation: generation(),
        }
    }

    fn scene(start: u64, end: u64) -> SceneRecord {
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

    fn plan(scenes: Vec<SceneRecord>) -> ScenePlan {
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

    fn narration(duration: u64) -> Narration {
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
            VideoSource::Clip("clip-a.mp4".into())
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
        assert_eq!(timeline.video_at(ms(5_000)), Some(1));
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
}
