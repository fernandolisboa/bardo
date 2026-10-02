//! Text generation (CONTEXT.md, PRD): scripts, titles, descriptions,
//! prompts and ideas, written by a generative provider (Claude). The
//! interface hides the provider's protocol; callers say what to write and
//! in which shape, and get the text back with what it used.

use std::sync::Arc;

use crate::{ApiKey, ProviderFailure};

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

/// What to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRequest {
    /// Standing instructions: role, rules, tone.
    pub instructions: String,
    /// The task itself, with its inputs.
    pub prompt: String,
    pub format: TextFormat,
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
