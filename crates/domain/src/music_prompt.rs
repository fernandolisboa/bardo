//! The music prompt (PRD story 40): Bardo does not make music. Claude
//! writes a prompt for the video's music from a template; the user edits
//! it, copies it into the music tool they have the rights to use, and
//! imports the file it makes (`crate::MediaAsset`).

use std::sync::Arc;
use std::time::SystemTime;

use crate::{Generation, ProfileId, RepositoryError, VideoProjectId};

/// Why music prompt text is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MusicPromptFieldError {
    TextRequired,
    TextTooLong,
}

impl MusicPromptFieldError {
    pub const ALL: [MusicPromptFieldError; 2] = [
        MusicPromptFieldError::TextRequired,
        MusicPromptFieldError::TextTooLong,
    ];
}

/// A video project's music prompt: the text to copy, as generated or as
/// the user edited it, and the generation it came from. Generating again
/// replaces both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicPrompt {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    text: String,
    generation: Generation,
    pub updated_at: SystemTime,
}

impl MusicPrompt {
    /// In characters, not bytes: music tools take short prompts.
    pub const MAX_CHARS: usize = 4_000;

    /// The text as a prompt holds it: ends trimmed, present, within limits.
    pub fn text_of(text: &str) -> Result<String, MusicPromptFieldError> {
        let text = text.trim();
        if text.is_empty() {
            Err(MusicPromptFieldError::TextRequired)
        } else if text.chars().count() > Self::MAX_CHARS {
            Err(MusicPromptFieldError::TextTooLong)
        } else {
            Ok(text.to_owned())
        }
    }

    /// The prompt `generation` wrote. Fails when its output is not usable
    /// (empty or runaway).
    pub fn generated(
        generation: Generation,
        now: SystemTime,
    ) -> Result<Self, MusicPromptFieldError> {
        Ok(Self {
            project: generation.project,
            owner: generation.owner,
            text: Self::text_of(&generation.output)?,
            generation,
            updated_at: now,
        })
    }

    /// A stored prompt as it was saved.
    pub fn restore(
        project: VideoProjectId,
        owner: ProfileId,
        text: String,
        generation: Generation,
        updated_at: SystemTime,
    ) -> Self {
        Self {
            project,
            owner,
            text,
            generation,
            updated_at,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn generation(&self) -> &Generation {
        &self.generation
    }

    /// Whether the user changed the text Claude wrote.
    pub fn is_edited(&self) -> bool {
        Self::text_of(&self.generation.output).as_deref() != Ok(self.text.as_str())
    }

    /// Replaces the text with the user's; `false` when it is the same.
    pub fn edit(&mut self, text: &str, now: SystemTime) -> Result<bool, MusicPromptFieldError> {
        let text = Self::text_of(text)?;
        if text == self.text {
            return Ok(false);
        }
        self.text = text;
        self.updated_at = now;
        Ok(true)
    }
}

/// Persistence port for music prompts. Shared with job worker threads.
pub trait MusicPromptRepository: Send + Sync {
    fn music_prompt(&self, project: VideoProjectId)
    -> Result<Option<MusicPrompt>, RepositoryError>;

    /// Saves the prompt, and its generation when it is new.
    fn save_music_prompt(&self, prompt: &MusicPrompt) -> Result<(), RepositoryError>;
}

impl<T: MusicPromptRepository + ?Sized> MusicPromptRepository for Arc<T> {
    fn music_prompt(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<MusicPrompt>, RepositoryError> {
        (**self).music_prompt(project)
    }

    fn save_music_prompt(&self, prompt: &MusicPrompt) -> Result<(), RepositoryError> {
        (**self).save_music_prompt(prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::generation;

    fn written(output: &str) -> MusicPrompt {
        let mut generation = generation();
        generation.output = output.into();
        MusicPrompt::generated(generation, SystemTime::UNIX_EPOCH).unwrap()
    }

    #[test]
    fn a_generated_prompt_is_the_output_trimmed_and_not_edited() {
        let prompt = written("  Dark ambient drone, 70 BPM, no vocals.\n");
        assert_eq!(prompt.text(), "Dark ambient drone, 70 BPM, no vocals.");
        assert!(!prompt.is_edited());
    }

    #[test]
    fn an_empty_or_runaway_output_is_not_a_prompt() {
        let mut empty = generation();
        empty.output = " \n".into();
        assert_eq!(
            MusicPrompt::generated(empty, SystemTime::UNIX_EPOCH),
            Err(MusicPromptFieldError::TextRequired)
        );
        let mut long = generation();
        long.output = "a".repeat(MusicPrompt::MAX_CHARS + 1);
        assert_eq!(
            MusicPrompt::generated(long, SystemTime::UNIX_EPOCH),
            Err(MusicPromptFieldError::TextTooLong)
        );
    }

    #[test]
    fn edits_change_the_text_and_keep_the_generation() {
        let mut prompt = written("Dark ambient drone.");
        let later = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
        assert_eq!(prompt.edit(" Dark ambient drone. ", later), Ok(false));
        assert_eq!(prompt.edit("Warm lo-fi beat.", later), Ok(true));
        assert_eq!(prompt.text(), "Warm lo-fi beat.");
        assert_eq!(prompt.generation().output, "Dark ambient drone.");
        assert!(prompt.is_edited());
        assert_eq!(prompt.updated_at, later);
        assert_eq!(
            prompt.edit("   ", later),
            Err(MusicPromptFieldError::TextRequired)
        );
        assert_eq!(prompt.text(), "Warm lo-fi beat.");
    }
}
