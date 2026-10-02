//! Scripts (PRD stories 29-30): what the narrator says in a video project.
//! Claude writes it from a template; the user edits it freely before
//! narration. A regenerated script waits beside the current one until the
//! user accepts it, so nothing is lost by asking again.

use std::sync::Arc;
use std::time::SystemTime;

use crate::{Generation, ProfileId, RepositoryError, VideoProjectId};

/// Why script text is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptFieldError {
    TextRequired,
    TextTooLong,
}

impl ScriptFieldError {
    pub const ALL: [ScriptFieldError; 2] = [
        ScriptFieldError::TextRequired,
        ScriptFieldError::TextTooLong,
    ];
}

/// The narration text. Always valid: present, ends trimmed, within limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptText(String);

impl ScriptText {
    /// In characters, not bytes. About an hour of narration, far beyond
    /// the videos Bardo makes, so the limit only stops runaway text.
    pub const MAX_CHARS: usize = 60_000;

    pub fn new(text: &str) -> Result<Self, ScriptFieldError> {
        let text = text.trim();
        if text.is_empty() {
            Err(ScriptFieldError::TextRequired)
        } else if text.chars().count() > Self::MAX_CHARS {
            Err(ScriptFieldError::TextTooLong)
        } else {
            Ok(Self(text.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Words as the narrator reads them, split on whitespace.
    pub fn word_count(&self) -> usize {
        self.0.split_whitespace().count()
    }
}

/// A generated script: the generation and its output as valid script text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedScript {
    generation: Generation,
    text: ScriptText,
}

impl GeneratedScript {
    /// Fails when the output is not usable as a script (empty or runaway).
    pub fn new(generation: Generation) -> Result<Self, ScriptFieldError> {
        let text = ScriptText::new(&generation.output)?;
        Ok(Self { generation, text })
    }

    pub fn generation(&self) -> &Generation {
        &self.generation
    }

    pub fn text(&self) -> &ScriptText {
        &self.text
    }
}

/// A user action on the script that does not apply right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no regenerated script is waiting for review")]
pub struct NoPendingScript;

/// The script of one video project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    text: ScriptText,
    /// Where the current text came from. The user's edits change the text,
    /// never this record.
    source: GeneratedScript,
    /// A regenerated script waiting for the user to accept or reject it.
    pending: Option<GeneratedScript>,
    pub updated_at: SystemTime,
}

/// Every stored field of a script, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRecord {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    pub text: ScriptText,
    pub source: GeneratedScript,
    pub pending: Option<GeneratedScript>,
    pub updated_at: SystemTime,
}

impl Script {
    /// The project's first script: the generated text as is.
    pub fn first(generated: GeneratedScript, now: SystemTime) -> Self {
        let generation = generated.generation();
        Self {
            project: generation.project,
            owner: generation.owner,
            text: generated.text().clone(),
            source: generated,
            pending: None,
            updated_at: now,
        }
    }

    pub fn restore(record: ScriptRecord) -> Self {
        Self {
            project: record.project,
            owner: record.owner,
            text: record.text,
            source: record.source,
            pending: record.pending,
            updated_at: record.updated_at,
        }
    }

    pub fn text(&self) -> &ScriptText {
        &self.text
    }

    pub fn source(&self) -> &GeneratedScript {
        &self.source
    }

    pub fn pending(&self) -> Option<&GeneratedScript> {
        self.pending.as_ref()
    }

    /// Whether the user changed the text since it was generated.
    pub fn is_edited(&self) -> bool {
        self.text != self.source.text
    }

    /// Replaces the text with the user's. Returns whether it changed.
    pub fn edit(&mut self, text: ScriptText, now: SystemTime) -> bool {
        if text == self.text {
            return false;
        }
        self.text = text;
        self.updated_at = now;
        true
    }

    /// Puts a regenerated script up for review. The current text stays
    /// until the user accepts it; an earlier one still waiting is replaced.
    pub fn offer(&mut self, generated: GeneratedScript, now: SystemTime) {
        self.pending = Some(generated);
        self.updated_at = now;
    }

    /// Makes the regenerated script the current one, edits included in the
    /// replaced text.
    pub fn accept(&mut self, now: SystemTime) -> Result<(), NoPendingScript> {
        let pending = self.pending.take().ok_or(NoPendingScript)?;
        self.text = pending.text.clone();
        self.source = pending;
        self.updated_at = now;
        Ok(())
    }

    /// Drops the regenerated script and keeps the current one.
    pub fn reject(&mut self, now: SystemTime) -> Result<(), NoPendingScript> {
        self.pending.take().ok_or(NoPendingScript)?;
        self.updated_at = now;
        Ok(())
    }
}

/// Persistence port for scripts and their generations. Shared with job
/// worker threads.
pub trait ScriptRepository: Send + Sync {
    fn script(&self, project: VideoProjectId) -> Result<Option<Script>, RepositoryError>;

    /// Inserts or updates the script and saves the generations it refers
    /// to, all or none. Generations are never changed once saved.
    fn save_script(&self, script: &Script) -> Result<(), RepositoryError>;
}

impl<T: ScriptRepository + ?Sized> ScriptRepository for Arc<T> {
    fn script(&self, project: VideoProjectId) -> Result<Option<Script>, RepositoryError> {
        (**self).script(project)
    }

    fn save_script(&self, script: &Script) -> Result<(), RepositoryError> {
        (**self).save_script(script)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        GenerationId, Provider, TemplateUsed, TemplateVersionId, TokenUsage, VideoProjectId,
    };

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    fn generated(project: VideoProjectId, output: &str) -> GeneratedScript {
        GeneratedScript::new(Generation {
            id: GenerationId::new(),
            owner: ProfileId::new(),
            project,
            provider: Provider::Claude,
            model: "claude-test".into(),
            template: TemplateUsed {
                id: TemplateVersionId::new(),
                number: 1,
            },
            instructions: "Rules.".into(),
            prompt: "Task.".into(),
            output: output.into(),
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 20,
            },
            generated_at: at(0),
            job: None,
        })
        .unwrap()
    }

    fn text(text: &str) -> ScriptText {
        ScriptText::new(text).unwrap()
    }

    #[test]
    fn text_is_trimmed_required_and_limited_in_characters() {
        assert_eq!(
            text("\n  Once upon a time. \n").as_str(),
            "Once upon a time."
        );
        assert_eq!(ScriptText::new(" \n"), Err(ScriptFieldError::TextRequired));
        let longest = "é".repeat(ScriptText::MAX_CHARS);
        assert!(ScriptText::new(&longest).is_ok());
        assert_eq!(
            ScriptText::new(&format!("{longest}é")),
            Err(ScriptFieldError::TextTooLong)
        );
        assert_eq!(text("One  two\nthree").word_count(), 3);
    }

    #[test]
    fn an_empty_output_is_not_a_script() {
        let mut generation = generated(VideoProjectId::new(), "x").generation().clone();
        generation.output = "  ".into();
        assert_eq!(
            GeneratedScript::new(generation),
            Err(ScriptFieldError::TextRequired)
        );
    }

    #[test]
    fn the_first_script_is_the_generated_text() {
        let project = VideoProjectId::new();
        let first = generated(project, "Draft one.\n");
        let script = Script::first(first.clone(), at(1));
        assert_eq!(script.project, project);
        assert_eq!(script.owner, first.generation().owner);
        assert_eq!(script.text().as_str(), "Draft one.");
        assert_eq!(script.source(), &first);
        assert_eq!(script.pending(), None);
        assert!(!script.is_edited());
    }

    #[test]
    fn edits_change_the_text_but_not_its_provenance() {
        let first = generated(VideoProjectId::new(), "Draft one.");
        let mut script = Script::first(first.clone(), at(1));

        assert!(!script.edit(text("Draft one."), at(2)), "same text");
        assert_eq!(script.updated_at, at(1));
        assert!(script.edit(text("Draft one, tightened."), at(3)));
        assert_eq!(script.text().as_str(), "Draft one, tightened.");
        assert_eq!(script.source(), &first);
        assert!(script.is_edited());
        assert_eq!(script.updated_at, at(3));
    }

    #[test]
    fn a_regenerated_script_waits_until_accepted() {
        let project = VideoProjectId::new();
        let mut script = Script::first(generated(project, "Draft one."), at(1));
        script.edit(text("Draft one, edited."), at(2));
        let second = generated(project, "Draft two.");

        script.offer(second.clone(), at(3));
        assert_eq!(script.text().as_str(), "Draft one, edited.");
        assert_eq!(script.pending(), Some(&second));

        script.accept(at(4)).unwrap();
        assert_eq!(script.text().as_str(), "Draft two.");
        assert_eq!(script.source(), &second);
        assert_eq!(script.pending(), None);
        assert!(!script.is_edited());
        assert_eq!(script.accept(at(5)), Err(NoPendingScript));
    }

    #[test]
    fn rejecting_keeps_the_current_script() {
        let project = VideoProjectId::new();
        let first = generated(project, "Draft one.");
        let mut script = Script::first(first.clone(), at(1));
        script.offer(generated(project, "Draft two."), at(2));
        script.offer(generated(project, "Draft three."), at(3));
        assert_eq!(
            script.pending().map(|p| p.text().as_str()),
            Some("Draft three."),
            "a newer regeneration replaces the one waiting"
        );

        script.reject(at(4)).unwrap();
        assert_eq!(script.text().as_str(), "Draft one.");
        assert_eq!(script.source(), &first);
        assert_eq!(script.pending(), None);
        assert_eq!(script.reject(at(5)), Err(NoPendingScript));
    }
}
