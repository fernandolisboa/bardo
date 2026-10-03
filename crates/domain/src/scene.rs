//! Scenes (PRD stories 34-36, 42, 44): the narration split into scenes,
//! each with the image prompt that draws it and the image drawn. Claude
//! plans the scenes on sentence boundaries of the narration, so every scene
//! starts and ends where the word timings say; the user edits any prompt;
//! the image provider draws one image per scene. Regenerating one scene's
//! image leaves the others alone and keeps the current image until the user
//! accepts the new one.

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{
    ClipModelRef, Generation, GenerationId, JobFailureKind, Narration, NarrationId, ProfileId,
    RepositoryError, VideoProjectId,
};

uuid_id!(
    /// Identifies one scene plan of a video project.
    ScenePlanId
);

/// The longest sentence a scene can start inside of, in words. A run-on
/// paragraph without punctuation is cut here, so a scene can still change
/// within it.
pub const MAX_SENTENCE_WORDS: usize = 40;

/// A sentence of the narration: where scenes may start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    /// Indexes of its words in the narration's word timings.
    pub words: Range<usize>,
    /// Byte range in the narration's text.
    pub text: Range<usize>,
    /// When its first word starts and its last word ends.
    pub start: Duration,
    pub end: Duration,
}

/// The sentences of the narration, in order: a word ending with `.`, `!`,
/// `?` or `…` (closing quotes and brackets aside) or followed by a line
/// break ends one, and none runs longer than `MAX_SENTENCE_WORDS`.
pub fn sentences(narration: &Narration) -> Vec<Sentence> {
    let text = narration.text.as_str();
    let words = narration.words.as_slice();
    let mut sentences = Vec::new();
    let mut first = 0;
    for (index, word) in words.iter().enumerate() {
        let next = words.get(index + 1);
        let ends = next.is_none_or(|next| {
            ends_sentence(&text[word.text.clone()])
                || text[word.text.end..next.text.start].contains('\n')
                || index + 1 - first >= MAX_SENTENCE_WORDS
        });
        if ends {
            let opening = &words[first];
            sentences.push(Sentence {
                words: first..index + 1,
                text: opening.text.start..word.text.end,
                start: opening.start,
                end: word.end.max(opening.start),
            });
            first = index + 1;
        }
    }
    sentences
}

/// A word that ends a sentence: `.`, `!`, `?` or `…`, past closing quotes
/// and brackets.
pub(crate) fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '”', '’', '»', ')', ']', '*'])
        .ends_with(['.', '!', '?', '…'])
}

/// Why prompt text is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SceneFieldError {
    PromptRequired,
    PromptTooLong,
}

impl SceneFieldError {
    pub const ALL: [SceneFieldError; 2] = [
        SceneFieldError::PromptRequired,
        SceneFieldError::PromptTooLong,
    ];
}

/// A scene's image prompt. Always valid: present, ends trimmed, within
/// limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePrompt(String);

impl ScenePrompt {
    /// In characters. Image models read a paragraph; more is a mistake.
    pub const MAX_CHARS: usize = 4_000;

    pub fn new(text: &str) -> Result<Self, SceneFieldError> {
        let text = text.trim();
        if text.is_empty() {
            Err(SceneFieldError::PromptRequired)
        } else if text.chars().count() > Self::MAX_CHARS {
            Err(SceneFieldError::PromptTooLong)
        } else {
            Ok(Self(text.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An image drawn for a scene: the file in the project folder and the
/// generation that drew it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneImage {
    /// The file's name in the project folder.
    pub file: String,
    pub generation: Generation,
}

/// A user action on a scene's image that does not apply right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no new image of this scene is waiting for review")]
pub struct NoPendingImage;

/// A clip made from a scene's image: the file in the project folder, its
/// length, the image it animated and the generation that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneClip {
    /// The file's name in the project folder.
    pub file: String,
    /// The length asked for, in seconds.
    pub seconds: u32,
    /// The generation of the image it starts from.
    pub source_image: GenerationId,
    pub generation: Generation,
}

/// A user action on a scene's clip that does not apply right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no new clip of this scene is waiting for review")]
pub struct NoPendingClip;

/// One scene: a stretch of the narration and the image shown over it, or
/// a clip animating that image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    pub start: Duration,
    pub end: Duration,
    /// The narration spoken over it.
    pub text: String,
    /// The prompt as planned. The user's edits change `prompt` only.
    generated_prompt: ScenePrompt,
    prompt: ScenePrompt,
    image: Option<SceneImage>,
    /// A regenerated image waiting for the user to accept or reject it.
    pending: Option<SceneImage>,
    /// Why the last attempt to draw it failed, until one succeeds.
    failure: Option<JobFailureKind>,
    /// How the clip moves, when the user wrote it; the image prompt
    /// otherwise.
    motion_prompt: Option<ScenePrompt>,
    /// The video model for this scene, instead of the channel's.
    clip_model: Option<ClipModelRef>,
    /// The clip the rough cut uses instead of the still image.
    clip: Option<SceneClip>,
    /// A new clip waiting for the user to accept or reject it.
    pending_clip: Option<SceneClip>,
    /// Why the last attempt to make a clip failed, until one succeeds.
    clip_failure: Option<JobFailureKind>,
}

/// Every stored field of a scene, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneRecord {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
    pub generated_prompt: ScenePrompt,
    pub prompt: ScenePrompt,
    pub image: Option<SceneImage>,
    pub pending: Option<SceneImage>,
    pub failure: Option<JobFailureKind>,
    pub motion_prompt: Option<ScenePrompt>,
    pub clip_model: Option<ClipModelRef>,
    pub clip: Option<SceneClip>,
    pub pending_clip: Option<SceneClip>,
    pub clip_failure: Option<JobFailureKind>,
}

impl Scene {
    pub fn restore(record: SceneRecord) -> Self {
        Self {
            start: record.start,
            end: record.end,
            text: record.text,
            generated_prompt: record.generated_prompt,
            prompt: record.prompt,
            image: record.image,
            pending: record.pending,
            failure: record.failure,
            motion_prompt: record.motion_prompt,
            clip_model: record.clip_model,
            clip: record.clip,
            pending_clip: record.pending_clip,
            clip_failure: record.clip_failure,
        }
    }

    /// A scene of a new plan: no image or clip yet.
    fn planned(start: Duration, end: Duration, text: String, prompt: ScenePrompt) -> Self {
        Self {
            start,
            end,
            text,
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

    pub fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }

    pub fn prompt(&self) -> &ScenePrompt {
        &self.prompt
    }

    pub fn generated_prompt(&self) -> &ScenePrompt {
        &self.generated_prompt
    }

    /// Whether the user changed the prompt since it was planned.
    pub fn is_edited(&self) -> bool {
        self.prompt != self.generated_prompt
    }

    pub fn image(&self) -> Option<&SceneImage> {
        self.image.as_ref()
    }

    pub fn pending(&self) -> Option<&SceneImage> {
        self.pending.as_ref()
    }

    pub fn failure(&self) -> Option<JobFailureKind> {
        self.failure
    }

    /// Replaces the prompt with the user's. Returns whether it changed.
    pub fn edit_prompt(&mut self, prompt: ScenePrompt) -> bool {
        if prompt == self.prompt {
            return false;
        }
        self.prompt = prompt;
        true
    }

    /// Takes a newly drawn image: it becomes the scene's image when there
    /// is none, else it waits for review beside the current one. Returns
    /// the image it pushed out (an earlier one waiting), whose file can go.
    pub fn add_image(&mut self, image: SceneImage) -> Option<SceneImage> {
        self.failure = None;
        if self.image.is_none() {
            self.image = Some(image);
            None
        } else {
            self.pending.replace(image)
        }
    }

    /// Records that drawing it failed. The images it has stay.
    pub fn fail(&mut self, kind: JobFailureKind) {
        self.failure = Some(kind);
    }

    /// Makes the image waiting for review the scene's image. Returns the
    /// replaced one, whose file can go.
    pub fn accept_image(&mut self) -> Result<Option<SceneImage>, NoPendingImage> {
        let pending = self.pending.take().ok_or(NoPendingImage)?;
        Ok(self.image.replace(pending))
    }

    /// Drops the image waiting for review and keeps the current one.
    /// Returns the dropped one, whose file can go.
    pub fn reject_image(&mut self) -> Result<SceneImage, NoPendingImage> {
        self.pending.take().ok_or(NoPendingImage)
    }

    /// What moves the clip: the user's motion prompt, else the image
    /// prompt.
    pub fn motion_prompt(&self) -> &ScenePrompt {
        self.motion_prompt.as_ref().unwrap_or(&self.prompt)
    }

    /// The motion prompt the user wrote, if any.
    pub fn own_motion_prompt(&self) -> Option<&ScenePrompt> {
        self.motion_prompt.as_ref()
    }

    /// Sets the user's motion prompt; `None` goes back to the image prompt.
    /// Returns whether it changed.
    pub fn set_motion_prompt(&mut self, prompt: Option<ScenePrompt>) -> bool {
        let prompt = prompt.filter(|prompt| *prompt != self.prompt);
        if prompt == self.motion_prompt {
            return false;
        }
        self.motion_prompt = prompt;
        true
    }

    /// The video model the scene picked over its channel's.
    pub fn clip_model(&self) -> Option<&ClipModelRef> {
        self.clip_model.as_ref()
    }

    /// Picks the scene's video model; `None` follows the channel. Returns
    /// whether it changed.
    pub fn set_clip_model(&mut self, model: Option<ClipModelRef>) -> bool {
        if model == self.clip_model {
            return false;
        }
        self.clip_model = model;
        true
    }

    pub fn clip(&self) -> Option<&SceneClip> {
        self.clip.as_ref()
    }

    pub fn pending_clip(&self) -> Option<&SceneClip> {
        self.pending_clip.as_ref()
    }

    pub fn clip_failure(&self) -> Option<JobFailureKind> {
        self.clip_failure
    }

    /// Whether the clip animates an image the scene no longer shows.
    pub fn is_clip_stale(&self) -> bool {
        self.clip.as_ref().is_some_and(|clip| {
            self.image
                .as_ref()
                .is_none_or(|image| image.generation.id != clip.source_image)
        })
    }

    /// Takes a new clip. Clips cost money and replace the still image in
    /// the cut, so every one waits for review, beside the current clip if
    /// there is one. Returns the clip it pushed out (an earlier one
    /// waiting), whose file can go.
    pub fn add_clip(&mut self, clip: SceneClip) -> Option<SceneClip> {
        self.clip_failure = None;
        self.pending_clip.replace(clip)
    }

    /// Records that making a clip failed. The clips it has stay.
    pub fn fail_clip(&mut self, kind: JobFailureKind) {
        self.clip_failure = Some(kind);
    }

    /// Makes the clip waiting for review the scene's clip. Returns the
    /// replaced one, whose file can go.
    pub fn accept_clip(&mut self) -> Result<Option<SceneClip>, NoPendingClip> {
        let pending = self.pending_clip.take().ok_or(NoPendingClip)?;
        Ok(self.clip.replace(pending))
    }

    /// Drops the clip waiting for review and keeps what the scene had.
    /// Returns the dropped one, whose file can go.
    pub fn reject_clip(&mut self) -> Result<SceneClip, NoPendingClip> {
        self.pending_clip.take().ok_or(NoPendingClip)
    }

    /// Goes back to the still image. Returns the clip it dropped, whose
    /// file can go.
    pub fn remove_clip(&mut self) -> Option<SceneClip> {
        self.clip.take()
    }

    /// Whether a clip can be made: it needs the image to start from.
    pub fn can_animate(&self) -> bool {
        self.image.is_some()
    }

    /// Every image and clip file the scene holds.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.image
            .iter()
            .chain(&self.pending)
            .map(|image| image.file.as_str())
            .chain(
                self.clip
                    .iter()
                    .chain(&self.pending_clip)
                    .map(|clip| clip.file.as_str()),
            )
    }
}

/// One scene as the planner proposed it: the sentence it starts at (an
/// index into `sentences`) and its image prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneDraft {
    pub first_sentence: usize,
    pub prompt: String,
}

/// The planner's answer has no usable scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the scene plan has no usable scene")]
pub struct NoScenes;

/// A scene plan that does not apply right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the plan has no scene {0}")]
pub struct NoSuchScene(pub usize);

/// The scenes of a video project, planned on its narration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePlan {
    pub id: ScenePlanId,
    pub project: VideoProjectId,
    pub owner: ProfileId,
    /// The narration the scenes are timed on.
    pub narration: NarrationId,
    /// The planner's generation; never changes.
    pub generation: Generation,
    scenes: Vec<Scene>,
    pub updated_at: SystemTime,
}

/// Every stored field of a scene plan, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenePlanRecord {
    pub id: ScenePlanId,
    pub project: VideoProjectId,
    pub owner: ProfileId,
    pub narration: NarrationId,
    pub generation: Generation,
    pub scenes: Vec<Scene>,
    pub updated_at: SystemTime,
}

impl ScenePlan {
    /// The plan the planner proposed, made to fit the narration: scenes in
    /// the order of their first sentence, one per sentence at most, the
    /// first starting at the narration's start and each running until the
    /// next starts, so together they cover all of it. Drafts with a
    /// sentence the narration does not have or an unusable prompt are
    /// dropped.
    pub fn from_drafts(
        generation: Generation,
        narration: &Narration,
        mut drafts: Vec<SceneDraft>,
        now: SystemTime,
    ) -> Result<Self, NoScenes> {
        let sentences = sentences(narration);
        drafts.sort_by_key(|draft| draft.first_sentence);
        let mut starts: Vec<(usize, ScenePrompt)> = Vec::new();
        for draft in drafts {
            let Ok(prompt) = ScenePrompt::new(&draft.prompt) else {
                continue;
            };
            let taken = starts
                .last()
                .is_some_and(|(first, _)| *first == draft.first_sentence);
            if draft.first_sentence < sentences.len() && !taken {
                starts.push((draft.first_sentence, prompt));
            }
        }
        // The opening sentences belong to the first scene.
        match starts.first_mut() {
            Some((first, _)) => *first = 0,
            None => return Err(NoScenes),
        }
        let text = narration.text.as_str();
        let end = narration
            .duration
            .max(sentences.last().map_or(Duration::ZERO, |s| s.end));
        let scenes = starts
            .iter()
            .enumerate()
            .map(|(index, (first, prompt))| {
                let next = starts.get(index + 1).map(|(next, _)| *next);
                let last = &sentences[next.unwrap_or(sentences.len()) - 1];
                Scene::planned(
                    if index == 0 {
                        Duration::ZERO
                    } else {
                        sentences[*first].start
                    },
                    next.map_or(end, |next| sentences[next].start),
                    text[sentences[*first].text.start..last.text.end].to_owned(),
                    prompt.clone(),
                )
            })
            .collect();
        Ok(Self {
            id: ScenePlanId::new(),
            project: generation.project,
            owner: generation.owner,
            narration: narration.id,
            generation,
            scenes,
            updated_at: now,
        })
    }

    pub fn restore(record: ScenePlanRecord) -> Self {
        Self {
            id: record.id,
            project: record.project,
            owner: record.owner,
            narration: record.narration,
            generation: record.generation,
            scenes: record.scenes,
            updated_at: record.updated_at,
        }
    }

    pub fn scenes(&self) -> &[Scene] {
        &self.scenes
    }

    pub fn scene(&self, index: usize) -> Result<&Scene, NoSuchScene> {
        self.scenes.get(index).ok_or(NoSuchScene(index))
    }

    /// The scene at `index`, to change; the plan's time moves to `now`.
    pub fn scene_mut(&mut self, index: usize, now: SystemTime) -> Result<&mut Scene, NoSuchScene> {
        let scene = self.scenes.get_mut(index).ok_or(NoSuchScene(index))?;
        self.updated_at = now;
        Ok(scene)
    }

    /// Whether the narration was generated again after this plan, so the
    /// scenes' times no longer match what is heard.
    pub fn is_stale(&self, narration: Option<&Narration>) -> bool {
        narration.is_none_or(|narration| narration.id != self.narration)
    }

    /// Scenes without an image, which a new image job draws.
    pub fn missing_images(&self) -> Vec<usize> {
        (0..self.scenes.len())
            .filter(|&index| self.scenes[index].image.is_none())
            .collect()
    }

    /// Scenes a clip can be made for that have none, accepted or waiting:
    /// what a new clip run animates.
    pub fn missing_clips(&self) -> Vec<usize> {
        (0..self.scenes.len())
            .filter(|&index| {
                let scene = &self.scenes[index];
                scene.can_animate() && scene.clip.is_none() && scene.pending_clip.is_none()
            })
            .collect()
    }

    /// Every image and clip file of every scene.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.scenes.iter().flat_map(Scene::files)
    }
}

/// Persistence port for scene plans. Shared with job worker threads.
pub trait ScenePlanRepository: Send + Sync {
    /// The project's current plan.
    fn scene_plan(&self, project: VideoProjectId) -> Result<Option<ScenePlan>, RepositoryError>;

    /// Makes `plan` the project's plan with all its scenes, replacing any
    /// other, and saves the generations it refers to; all or none.
    fn save_scene_plan(&self, plan: &ScenePlan) -> Result<(), RepositoryError>;

    /// Saves scene `index` of the saved plan `plan` (prompts, model,
    /// images, clips, failures) and the generations of its images and
    /// clips, leaving the other scenes as they are. Fails when the plan is
    /// no longer saved.
    fn save_scene(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError>;
}

impl<T: ScenePlanRepository + ?Sized> ScenePlanRepository for Arc<T> {
    fn scene_plan(&self, project: VideoProjectId) -> Result<Option<ScenePlan>, RepositoryError> {
        (**self).scene_plan(project)
    }

    fn save_scene_plan(&self, plan: &ScenePlan) -> Result<(), RepositoryError> {
        (**self).save_scene_plan(plan)
    }

    fn save_scene(&self, plan: &ScenePlan, index: usize) -> Result<(), RepositoryError> {
        (**self).save_scene(plan, index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Alignment, CharTiming, GenerationId, Provider, ScriptText, TemplateUsed, TemplateVersionId,
        TokenUsage, WordTimings,
    };

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A narration of `text` with every character 100 ms long, lasting
    /// `tail` ms past its last character.
    fn narration(text: &str, tail: u64) -> Narration {
        let alignment = Alignment {
            chars: text
                .chars()
                .enumerate()
                .map(|(n, c)| CharTiming {
                    text: c.to_string(),
                    start: ms(n as u64 * 100),
                    end: ms(n as u64 * 100 + 100),
                })
                .collect(),
        };
        Narration {
            id: NarrationId::new(),
            project: VideoProjectId::new(),
            owner: ProfileId::new(),
            text: ScriptText::new(text).unwrap(),
            // Scenes follow the words, whoever spoke them.
            source: crate::NarrationSource::Imported {
                file_name: "take 3.wav".into(),
                aligner: Provider::ElevenLabs,
                model: "forced_alignment".into(),
            },
            audio_file: "narration.wav".into(),
            duration: alignment.end() + ms(tail),
            words: WordTimings::from_alignment(text, &alignment),
            generated_at: SystemTime::UNIX_EPOCH,
            job: None,
        }
    }

    fn generation(narration: &Narration, provider: Provider, output: &str) -> Generation {
        Generation {
            id: GenerationId::new(),
            owner: narration.owner,
            project: narration.project,
            provider,
            model: "model".into(),
            template: TemplateUsed {
                id: TemplateVersionId::new(),
                number: 1,
            },
            instructions: String::new(),
            prompt: "Plan.".into(),
            output: output.into(),
            usage: TokenUsage::default(),
            generated_at: SystemTime::UNIX_EPOCH,
            job: None,
        }
    }

    fn draft(first_sentence: usize, prompt: &str) -> SceneDraft {
        SceneDraft {
            first_sentence,
            prompt: prompt.into(),
        }
    }

    fn plan(narration: &Narration, drafts: Vec<SceneDraft>) -> ScenePlan {
        ScenePlan::from_drafts(
            generation(narration, Provider::Claude, "{}"),
            narration,
            drafts,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap()
    }

    fn image(narration: &Narration, file: &str) -> SceneImage {
        SceneImage {
            file: file.into(),
            generation: generation(narration, Provider::Gemini, file),
        }
    }

    const STORY: &str = "Hi there. Who are you?\nI am \"the probe.\" It ends";

    #[test]
    fn sentences_end_at_marks_and_line_breaks() {
        let n = narration(STORY, 0);
        let texts: Vec<_> = sentences(&n)
            .iter()
            .map(|s| &STORY[s.text.clone()])
            .collect();
        assert_eq!(
            texts,
            [
                "Hi there.",
                "Who are you?",
                "I am \"the probe.\"",
                "It ends"
            ]
        );
        let first = &sentences(&n)[0];
        assert_eq!(first.words, 0..2);
        assert_eq!(
            (first.start, first.end),
            (ms(0), ms(800)),
            "'Hi' to 'there'"
        );
        let second = &sentences(&n)[1];
        assert_eq!(second.start, ms(1000));
    }

    #[test]
    fn a_run_on_sentence_is_cut_so_scenes_can_change_inside_it() {
        let text = vec!["word"; MAX_SENTENCE_WORDS + 5].join(" ");
        let n = narration(&text, 0);
        let lengths: Vec<_> = sentences(&n).iter().map(|s| s.words.len()).collect();
        assert_eq!(lengths, [MAX_SENTENCE_WORDS, 5]);
    }

    #[test]
    fn scenes_cover_the_whole_narration_back_to_back() {
        let n = narration(STORY, 500);
        let sentences = sentences(&n);
        let plan = plan(&n, vec![draft(2, "A probe."), draft(0, "A greeting.")]);
        let scenes = plan.scenes();
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].start, Duration::ZERO);
        assert_eq!(scenes[0].end, sentences[2].start);
        assert_eq!(scenes[1].start, sentences[2].start);
        assert_eq!(scenes[1].end, n.duration, "to the end, silence included");
        assert_eq!(scenes[0].text, "Hi there. Who are you?");
        assert_eq!(scenes[1].text, "I am \"the probe.\" It ends");
        assert_eq!(scenes[0].prompt().as_str(), "A greeting.");
        assert!(!scenes[0].is_edited());
        assert_eq!(plan.narration, n.id);
        assert_eq!(plan.project, n.project);
        assert_eq!(plan.missing_images(), [0, 1]);
    }

    #[test]
    fn the_first_scene_starts_at_the_beginning_whatever_the_planner_said() {
        let n = narration(STORY, 0);
        let plan = plan(&n, vec![draft(1, "Who."), draft(3, "End.")]);
        assert_eq!(plan.scenes()[0].start, Duration::ZERO);
        assert_eq!(
            plan.scenes()[0].text,
            "Hi there. Who are you?\nI am \"the probe.\""
        );
    }

    #[test]
    fn unusable_drafts_are_dropped_and_none_left_is_an_error() {
        let n = narration(STORY, 0);
        let plan = plan(
            &n,
            vec![
                draft(0, "Opening."),
                draft(0, "Same sentence again."),
                draft(2, "  "),
                draft(9, "No such sentence."),
                draft(3, "End."),
            ],
        );
        let prompts: Vec<_> = plan.scenes().iter().map(|s| s.prompt().as_str()).collect();
        assert_eq!(prompts, ["Opening.", "End."]);

        let none = ScenePlan::from_drafts(
            generation(&n, Provider::Claude, "{}"),
            &n,
            vec![draft(7, "Out of range."), draft(1, "")],
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(none, Err(NoScenes));
    }

    #[test]
    fn prompts_are_trimmed_required_and_limited() {
        assert_eq!(ScenePrompt::new("  A ship. ").unwrap().as_str(), "A ship.");
        assert_eq!(ScenePrompt::new(" "), Err(SceneFieldError::PromptRequired));
        let longest = "é".repeat(ScenePrompt::MAX_CHARS);
        assert!(ScenePrompt::new(&longest).is_ok());
        assert_eq!(
            ScenePrompt::new(&format!("{longest}é")),
            Err(SceneFieldError::PromptTooLong)
        );
    }

    #[test]
    fn editing_a_prompt_keeps_the_planned_one() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "Planned.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert!(!scene.edit_prompt(ScenePrompt::new("Planned.").unwrap()));
        assert!(scene.edit_prompt(ScenePrompt::new("Mine.").unwrap()));
        assert_eq!(scene.prompt().as_str(), "Mine.");
        assert_eq!(scene.generated_prompt().as_str(), "Planned.");
        assert!(scene.is_edited());
        assert_eq!(plan.scene(3), Err(NoSuchScene(3)));
    }

    #[test]
    fn a_new_image_waits_for_review_beside_the_current_one() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A."), draft(2, "B.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        scene.fail(JobFailureKind::Declined);
        assert_eq!(scene.add_image(image(&n, "one.png")), None);
        assert_eq!(scene.failure(), None, "a success clears the failure");
        assert_eq!(scene.image().unwrap().file, "one.png");

        assert_eq!(scene.add_image(image(&n, "two.png")), None);
        assert_eq!(
            scene.image().unwrap().file,
            "one.png",
            "kept until accepted"
        );
        assert_eq!(scene.pending().unwrap().file, "two.png");
        let pushed = scene.add_image(image(&n, "three.png")).unwrap();
        assert_eq!(
            pushed.file, "two.png",
            "a newer one replaces the one waiting"
        );

        let replaced = scene.accept_image().unwrap().unwrap();
        assert_eq!(replaced.file, "one.png");
        assert_eq!(scene.image().unwrap().file, "three.png");
        assert_eq!(scene.pending(), None);
        assert_eq!(scene.accept_image(), Err(NoPendingImage));

        scene.add_image(image(&n, "four.png"));
        assert_eq!(scene.reject_image().unwrap().file, "four.png");
        assert_eq!(scene.image().unwrap().file, "three.png");
        assert_eq!(scene.reject_image(), Err(NoPendingImage));
        assert_eq!(plan.missing_images(), [1], "the other scene is untouched");
        assert_eq!(plan.files().collect::<Vec<_>>(), ["three.png"]);
    }

    fn clip(narration: &Narration, file: &str, source: &SceneImage) -> SceneClip {
        SceneClip {
            file: file.into(),
            seconds: 5,
            source_image: source.generation.id,
            generation: generation(narration, Provider::Higgsfield, file),
        }
    }

    #[test]
    fn every_new_clip_waits_for_review_and_the_still_can_come_back() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A."), draft(2, "B.")]);
        assert_eq!(plan.missing_clips(), Vec::<usize>::new(), "no image yet");
        let still = image(&n, "still.png");
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert!(!scene.can_animate());
        scene.add_image(still.clone());
        assert_eq!(plan.missing_clips(), [0]);

        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        scene.fail_clip(JobFailureKind::Declined);
        assert_eq!(scene.add_clip(clip(&n, "one.mp4", &still)), None);
        assert_eq!(scene.clip_failure(), None, "a success clears the failure");
        assert_eq!(scene.clip(), None, "even the first clip is reviewed");
        assert_eq!(scene.pending_clip().unwrap().file, "one.mp4");
        assert_eq!(plan.missing_clips(), Vec::<usize>::new(), "one is waiting");

        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        let pushed = scene.add_clip(clip(&n, "two.mp4", &still)).unwrap();
        assert_eq!(
            pushed.file, "one.mp4",
            "a newer one replaces the one waiting"
        );
        assert_eq!(scene.accept_clip().unwrap(), None);
        assert_eq!(scene.clip().unwrap().file, "two.mp4");
        assert_eq!(scene.accept_clip(), Err(NoPendingClip));

        scene.add_clip(clip(&n, "three.mp4", &still));
        assert_eq!(scene.reject_clip().unwrap().file, "three.mp4");
        assert_eq!(scene.clip().unwrap().file, "two.mp4", "kept");
        assert_eq!(scene.reject_clip(), Err(NoPendingClip));
        assert_eq!(
            plan.files().collect::<Vec<_>>(),
            ["still.png", "two.mp4"],
            "images and clips"
        );

        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(scene.remove_clip().unwrap().file, "two.mp4");
        assert_eq!(scene.clip(), None, "back to the still image");
        assert_eq!(scene.image().unwrap().file, "still.png");
    }

    #[test]
    fn a_clip_goes_stale_when_the_scene_takes_another_image() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        let first = image(&n, "first.png");
        scene.add_image(first.clone());
        scene.add_clip(clip(&n, "clip.mp4", &first));
        scene.accept_clip().unwrap();
        assert!(!scene.is_clip_stale());
        scene.add_image(image(&n, "second.png"));
        assert!(!scene.is_clip_stale(), "the new image is only waiting");
        scene.accept_image().unwrap();
        assert!(scene.is_clip_stale());
    }

    #[test]
    fn the_motion_prompt_follows_the_image_prompt_until_the_user_writes_one() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A ship at dawn.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(scene.motion_prompt().as_str(), "A ship at dawn.");
        assert_eq!(scene.own_motion_prompt(), None);
        let slow = ScenePrompt::new("Slow push in, waves rolling.").unwrap();
        assert!(scene.set_motion_prompt(Some(slow.clone())));
        assert!(!scene.set_motion_prompt(Some(slow)));
        assert_eq!(
            scene.motion_prompt().as_str(),
            "Slow push in, waves rolling."
        );
        scene.edit_prompt(ScenePrompt::new("A ship at dusk.").unwrap());
        assert_eq!(
            scene.motion_prompt().as_str(),
            "Slow push in, waves rolling.",
            "the user's motion prompt stays"
        );
        assert!(scene.set_motion_prompt(None));
        assert_eq!(scene.motion_prompt().as_str(), "A ship at dusk.");
        let same = ScenePrompt::new("A ship at dusk.").unwrap();
        assert!(
            !scene.set_motion_prompt(Some(same)),
            "same as the image prompt"
        );
        assert_eq!(scene.own_motion_prompt(), None);
    }

    #[test]
    fn a_scene_can_pick_its_own_video_model() {
        let n = narration(STORY, 0);
        let mut plan = plan(&n, vec![draft(0, "A.")]);
        let scene = plan.scene_mut(0, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(scene.clip_model(), None, "follows the channel");
        let model = ClipModelRef::new(Provider::Higgsfield, "kling").unwrap();
        assert!(scene.set_clip_model(Some(model.clone())));
        assert!(!scene.set_clip_model(Some(model.clone())));
        assert_eq!(scene.clip_model(), Some(&model));
        assert!(scene.set_clip_model(None));
    }

    #[test]
    fn a_plan_goes_stale_when_the_narration_is_generated_again() {
        let n = narration(STORY, 0);
        let plan = plan(&n, vec![draft(0, "A.")]);
        assert!(!plan.is_stale(Some(&n)));
        let again = Narration {
            id: NarrationId::new(),
            ..n.clone()
        };
        assert!(plan.is_stale(Some(&again)));
        assert!(plan.is_stale(None));
    }
}
