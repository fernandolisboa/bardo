//! Text generation (CONTEXT.md, PRD): scripts, titles, descriptions,
//! prompts and ideas, written by a generative provider (Claude). The
//! interface hides the provider's protocol; callers say what to write and
//! in which shape, and get the text back with what it used.

use std::sync::Arc;

use crate::{ApiKey, ImageFormat, ProviderFailure};

/// The shape the text must have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextFormat {
    /// Free text.
    Prose,
    /// A JSON document valid against `schema` (a JSON Schema, as text).
    /// Objects in the schema list every property as required and allow no
    /// others, which is what providers that enforce schemas accept.
    Json { schema: String },
}

/// A picture the writer looks at with the task (#192: a frame of a take
/// whose set it describes).
#[derive(Clone, PartialEq, Eq)]
pub struct TextImage {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
}

impl std::fmt::Debug for TextImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TextImage({} bytes of {:?})",
            self.bytes.len(),
            self.format
        )
    }
}

/// How much thought the writer gives a task before answering. More thought
/// costs more and takes longer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextEffort {
    /// What suits most tasks: titles, prompts, ideas, tags.
    #[default]
    Everyday,
    /// For the tasks the rest of a video is built on (its script and its
    /// scene plan, #258), where a weak answer is paid for again in every
    /// image and clip.
    Careful,
}

/// What to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRequest {
    /// Standing instructions: role, rules, tone.
    pub instructions: String,
    /// The task itself, with its inputs.
    pub prompt: String,
    /// Pictures that go with the task, before it.
    pub images: Vec<TextImage>,
    pub format: TextFormat,
    pub effort: TextEffort,
    /// About how large the answer gets, in tokens as the provider counts
    /// them, when the task can tell (a scene plan grows with its
    /// narration). It is a size to make room for, not a limit: the writer
    /// leaves room for it, for the thought before it and for an answer
    /// that runs longer. Without it the writer gives its usual room,
    /// enough for every task whose answer does not grow.
    pub answer_tokens: Option<u64>,
}

/// Tokens a generation used, as the provider counted them. Cost tracking
/// (budgets) builds on these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// The generated text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedText {
    pub text: String,
    /// The model that wrote it, for the record.
    pub model: String,
    pub usage: TokenUsage,
}

/// Writes text. Calls the network and blocks, so it runs inside a job.
pub trait TextGenerator: Send + Sync {
    fn generate(
        &self,
        key: &ApiKey,
        request: &TextRequest,
    ) -> Result<GeneratedText, ProviderFailure>;
}

impl<T: TextGenerator + ?Sized> TextGenerator for Arc<T> {
    fn generate(
        &self,
        key: &ApiKey,
        request: &TextRequest,
    ) -> Result<GeneratedText, ProviderFailure> {
        (**self).generate(key, request)
    }
}
