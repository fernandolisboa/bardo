//! A paid answer kept on its job until it is applied.

use bardo_domain::{GeneratedText, Job, Progress, TokenUsage};
use serde::{Deserialize, Serialize};

use super::JobContext;

/// What a text job keeps in its checkpoint: Claude's answer, from when it
/// is paid for and recorded until what it says is saved. It is not asked
/// for twice: a job stopped as the answer arrived (cancelled, or cut
/// short by the app closing) goes on from it on its next run, and so does
/// one that could not save what it made. (The request itself is noted on
/// the job while it is out, `CostBook::paying`: a run cut short on it by
/// a close is not sent again by itself, ADR-0030.)
///
/// For a job that keeps nothing else in its checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kept {
    /// Nothing is kept: the answer was applied, or let go.
    Settled,
    /// Claude's answer, paid for and recorded.
    Answered(KeptAnswer),
}

/// An answer kept in a job's checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KeptAnswer {
    text: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
}

impl From<&GeneratedText> for KeptAnswer {
    fn from(generated: &GeneratedText) -> Self {
        Self {
            text: generated.text.clone(),
            model: generated.model.clone(),
            input_tokens: generated.usage.input_tokens,
            output_tokens: generated.usage.output_tokens,
        }
    }
}

impl From<KeptAnswer> for GeneratedText {
    fn from(kept: KeptAnswer) -> Self {
        Self {
            text: kept.text,
            model: kept.model,
            usage: TokenUsage {
                input_tokens: kept.input_tokens,
                output_tokens: kept.output_tokens,
            },
        }
    }
}

/// Where a handler keeps a paid answer until it is applied: the job's
/// checkpoint ([`Kept`]), or a plain slot for a handler run outside the
/// queue.
pub(crate) trait Keeps {
    /// The answer an earlier run kept, if one did.
    fn kept(&self) -> Option<GeneratedText>;
    /// Keeps `generated`, paid for and recorded, until it is applied.
    fn keep(&mut self, generated: &GeneratedText);
    /// Lets the kept answer go: it was applied, or is of no use.
    fn settle(&mut self);
}

impl Keeps for JobContext {
    fn kept(&self) -> Option<GeneratedText> {
        Kept::answer(self)
    }

    fn keep(&mut self, generated: &GeneratedText) {
        Kept::keep(self, generated);
    }

    fn settle(&mut self) {
        Kept::settle(self);
    }
}

impl Keeps for Option<GeneratedText> {
    fn kept(&self) -> Option<GeneratedText> {
        self.clone()
    }

    fn keep(&mut self, generated: &GeneratedText) {
        *self = Some(generated.clone());
    }

    fn settle(&mut self) {
        *self = None;
    }
}

impl Kept {
    /// The answer an earlier run of the job kept, if one did.
    pub(crate) fn answer(cx: &JobContext) -> Option<GeneratedText> {
        match serde_json::from_str(cx.checkpoint()?).ok()? {
            Self::Answered(answer) => Some(answer.into()),
            Self::Settled => None,
        }
    }

    /// Keeps `generated` on the job, for a run that was told to stop, too
    /// (`save_checkpoint`). A save that fails costs at worst the answer
    /// it would have kept.
    pub(crate) fn keep(cx: &mut JobContext, generated: &GeneratedText) {
        Self::Answered(generated.into()).save(cx);
    }

    /// Lets the kept answer go: it was applied, or is of no use.
    pub(crate) fn settle(cx: &mut JobContext) {
        Self::Settled.save(cx);
    }

    /// Whether `job` holds an answer it has yet to apply.
    pub(crate) fn held_by(job: &Job) -> bool {
        matches!(
            job.checkpoint().map(serde_json::from_str::<Self>),
            Some(Ok(Self::Answered(_)))
        )
    }

    fn save(&self, cx: &mut JobContext) {
        let json = serde_json::to_string(self).expect("a kept answer serializes");
        if let Err(error) = cx.save_checkpoint(json, Progress::ZERO) {
            tracing::warn!(job = %cx.id(), "could not save the answer a job keeps: {error}");
        }
    }
}
