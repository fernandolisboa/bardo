//! Provenance of generated assets (PRD story 43): every result a provider
//! generates records who made it, with which model, from which final prompt
//! and template version, and what it used, so any result can be reproduced
//! or audited.

use std::time::SystemTime;

use crate::{JobId, ProfileId, Provider, TemplateVersionId, TokenUsage, VideoProjectId};

uuid_id!(
    /// Identifies one generation.
    GenerationId
);

/// The template version a generation was rendered from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateUsed {
    pub id: TemplateVersionId,
    pub number: u32,
}

/// One provider call that produced an asset, as it happened. Never changes
/// once saved; editing the asset afterwards leaves it as generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    pub id: GenerationId,
    pub owner: ProfileId,
    /// The video project the asset belongs to.
    pub project: VideoProjectId,
    pub provider: Provider,
    /// The model that answered, as the provider named it.
    pub model: String,
    pub template: TemplateUsed,
    /// The final prompt sent: the template's instructions and prompt with
    /// the project's facts in place.
    pub instructions: String,
    pub prompt: String,
    /// What the provider returned.
    pub output: String,
    pub usage: TokenUsage,
    pub generated_at: SystemTime,
    /// The job that ran it, so a resumed job does not generate twice.
    pub job: Option<JobId>,
}
