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

/// Asked while an answer is awaited: whether whoever wanted it no longer
/// does (the job was cancelled, the app is closing). A writer that sees it
/// gives the answer up there, so a provider that bills what it writes is
/// not left writing an answer nobody reads.
#[derive(Clone, Default)]
pub struct Stop(Option<Arc<dyn Fn() -> bool + Send + Sync>>);

impl Stop {
    /// Nobody stops this answer.
    pub fn never() -> Self {
        Self(None)
    }

    /// Stops once `asked` says so. It is asked often, from the thread that
    /// waits for the answer.
    pub fn when(asked: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(asked)))
    }

    /// Whether the answer is no longer wanted.
    pub fn asked(&self) -> bool {
        self.0.as_ref().is_some_and(|asked| asked())
    }

    /// Whether anybody may stop the answer at all.
    pub fn may_ask(&self) -> bool {
        self.0.is_some()
    }
}

impl std::fmt::Debug for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            None => f.write_str("Stop(never)"),
            // Not asked here: asking may take a lock.
            Some(_) => f.write_str("Stop(..)"),
        }
    }
}

/// Writes text. Calls the network and blocks, so it runs inside a job.
pub trait TextGenerator: Send + Sync {
    /// Writes the text, and gives up as soon as `stop` asks. An answer
    /// given up after its request left fails as a lost answer
    /// (`ProviderFailureKind::AnswerLost`): the provider bills what it
    /// wrote of it. One given up before anything was sent fails as
    /// `Unreachable`, which nothing is billed for. An answer that arrived
    /// whole is returned although `stop` asked meanwhile: it is paid for,
    /// and the caller decides what to do with it.
    fn generate_until(
        &self,
        key: &ApiKey,
        request: &TextRequest,
        stop: &Stop,
    ) -> Result<GeneratedText, ProviderFailure>;

    /// Writes the text and waits for all of it.
    fn generate(
        &self,
        key: &ApiKey,
        request: &TextRequest,
    ) -> Result<GeneratedText, ProviderFailure> {
        self.generate_until(key, request, &Stop::never())
    }
}

impl<T: TextGenerator + ?Sized> TextGenerator for Arc<T> {
    fn generate_until(
        &self,
        key: &ApiKey,
        request: &TextRequest,
        stop: &Stop,
    ) -> Result<GeneratedText, ProviderFailure> {
        (**self).generate_until(key, request, stop)
    }
}
