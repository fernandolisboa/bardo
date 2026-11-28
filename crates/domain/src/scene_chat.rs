//! The scene chat (#367, ADR-0049): the user's requests about one scene's
//! image or clip, Claude's replies, and what was applied of them.
//!
//! A request is answered before anything is paid but the answer: Claude
//! replies with a question, or with a proposal of one or more options,
//! each the English instruction it would send. Applying an option starts
//! its paid job, whose result waits for review as every new image does.
//! An option is tied to the image it was proposed for: once the scene's
//! image changed, it no longer applies.
//!
//! The chat belongs to its scene plan: it stays when the plan is fitted to
//! a new narration and goes when the scenes are planned again.

use std::time::SystemTime;

use serde_json::{Value, json};

use crate::{GenerationId, JobId, RepositoryError, ScenePlanId};

uuid_id!(
    /// Identifies one message of a scene chat.
    SceneChatMessageId
);

/// The most characters a request may have.
pub const MAX_CHAT_REQUEST_CHARS: usize = 1_000;

/// How many of a scene's earlier messages go with a request to Claude.
pub const CHAT_HISTORY_MESSAGES: usize = 12;

/// What a request is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatTarget {
    /// The scene's image.
    Image,
    /// The scene's clip.
    Clip,
}

impl ChatTarget {
    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            ChatTarget::Image => "image",
            ChatTarget::Clip => "clip",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "image" => Some(ChatTarget::Image),
            "clip" => Some(ChatTarget::Clip),
            _ => None,
        }
    }
}

/// One way to apply a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatOptionKind {
    /// A new image drawn from the scene's current one and the instruction.
    EditImage,
}

impl ChatOptionKind {
    /// Stable name stored with the reply.
    pub fn code(self) -> &'static str {
        match self {
            ChatOptionKind::EditImage => "edit_image",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "edit_image" => Some(ChatOptionKind::EditImage),
            _ => None,
        }
    }

    /// What the option changes.
    pub fn target(self) -> ChatTarget {
        match self {
            ChatOptionKind::EditImage => ChatTarget::Image,
        }
    }
}

/// An option of a proposal: what it does and the English text it sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatOption {
    pub kind: ChatOptionKind,
    /// What to change, for the image model.
    pub instruction: String,
    /// The scene's image prompt with the change written in, which becomes
    /// the scene's prompt when the new image is used.
    pub prompt: String,
}

/// Claude's options for a request it understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    options: Vec<ChatOption>,
    recommended: usize,
    /// The scene's image the options were proposed for.
    image: GenerationId,
    /// The scene's image prompt the options' prompts were rewritten from;
    /// `None` when not known.
    from_prompt: Option<String>,
}

/// Why a proposal cannot be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProposalError {
    #[error("a proposal needs at least one option")]
    NoOptions,
    #[error("an option offers the same thing as another")]
    Repeated,
    #[error("an option's instruction or prompt is empty")]
    Empty,
}

impl Proposal {
    /// A proposal of `options` for the scene's image `image`. The
    /// recommended option is the first one when `recommended` names none.
    pub fn new(
        options: Vec<ChatOption>,
        recommended: usize,
        image: GenerationId,
    ) -> Result<Self, ProposalError> {
        if options.is_empty() {
            return Err(ProposalError::NoOptions);
        }
        if options
            .iter()
            .any(|option| option.instruction.trim().is_empty() || option.prompt.trim().is_empty())
        {
            return Err(ProposalError::Empty);
        }
        for (index, option) in options.iter().enumerate() {
            if options[..index]
                .iter()
                .any(|other| other.kind == option.kind)
            {
                return Err(ProposalError::Repeated);
            }
        }
        let recommended = if recommended < options.len() {
            recommended
        } else {
            0
        };
        Ok(Self {
            options,
            recommended,
            image,
            from_prompt: None,
        })
    }

    /// The proposal, noting the scene prompt its options' prompts were
    /// rewritten from: a prompt the user changed since is not replaced.
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.from_prompt = Some(prompt.into());
        self
    }

    /// The scene prompt the options' prompts were rewritten from.
    pub fn from_prompt(&self) -> Option<&str> {
        self.from_prompt.as_deref()
    }

    pub fn options(&self) -> &[ChatOption] {
        &self.options
    }

    pub fn recommended(&self) -> usize {
        self.recommended
    }

    pub fn image(&self) -> GenerationId {
        self.image
    }
}

/// What Claude answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatReply {
    /// It asks the user something before proposing anything.
    Question,
    Proposal(Proposal),
}

/// How the review of an applied option ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatOutcome {
    /// The user used the new image.
    Used,
    /// The user kept the current one.
    Kept,
}

impl ChatOutcome {
    pub fn code(self) -> &'static str {
        match self {
            ChatOutcome::Used => "used",
            ChatOutcome::Kept => "kept",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "used" => Some(ChatOutcome::Used),
            "kept" => Some(ChatOutcome::Kept),
            _ => None,
        }
    }
}

/// An option the user applied: its job, and how its review ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    pub option: usize,
    pub job: JobId,
    pub outcome: Option<ChatOutcome>,
}

/// What a message is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEntry {
    /// The user's request about the scene's image or clip.
    Request { target: ChatTarget },
    /// Claude's reply, and the option the user applied of it.
    Reply {
        reply: ChatReply,
        applied: Option<Applied>,
    },
}

/// A request the user cannot send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChatRequestError {
    #[error("the request is empty")]
    Empty,
    #[error("the request is too long")]
    TooLong,
}

/// An option that cannot be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChatApplyError {
    #[error("the message proposes nothing")]
    NotAProposal,
    #[error("the proposal has no such option")]
    NoSuchOption,
    /// The scene's image changed since the option was proposed.
    #[error("the scene changed since the option was proposed")]
    OutOfDate,
    /// The user already used what an option of this reply made.
    #[error("an option of this reply was already used")]
    AlreadyUsed,
}

/// One message of a scene chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneChatMessage {
    pub id: SceneChatMessageId,
    pub plan: ScenePlanId,
    /// The scene's position in its plan.
    pub scene: usize,
    /// The user's words, or Claude's reply in the interface language.
    pub text: String,
    pub entry: ChatEntry,
    /// The scene chat job that answers a request, or that wrote a reply.
    pub job: Option<JobId>,
    pub at: SystemTime,
}

impl SceneChatMessage {
    /// The user's request about scene `scene`'s `target`, trimmed.
    pub fn request(
        plan: ScenePlanId,
        scene: usize,
        target: ChatTarget,
        text: &str,
        at: SystemTime,
    ) -> Result<Self, ChatRequestError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ChatRequestError::Empty);
        }
        if text.chars().count() > MAX_CHAT_REQUEST_CHARS {
            return Err(ChatRequestError::TooLong);
        }
        Ok(Self {
            id: SceneChatMessageId::new(),
            plan,
            scene,
            text: text.to_owned(),
            entry: ChatEntry::Request { target },
            job: None,
            at,
        })
    }

    /// Claude's reply to a request about scene `scene`, written by `job`.
    pub fn reply(
        plan: ScenePlanId,
        scene: usize,
        text: &str,
        reply: ChatReply,
        job: JobId,
        at: SystemTime,
    ) -> Self {
        Self {
            id: SceneChatMessageId::new(),
            plan,
            scene,
            text: text.trim().to_owned(),
            entry: ChatEntry::Reply {
                reply,
                applied: None,
            },
            job: Some(job),
            at,
        }
    }

    /// Whether the user wrote it.
    pub fn is_request(&self) -> bool {
        matches!(self.entry, ChatEntry::Request { .. })
    }

    /// What a request is about; `None` for a reply.
    pub fn target(&self) -> Option<ChatTarget> {
        match self.entry {
            ChatEntry::Request { target } => Some(target),
            ChatEntry::Reply { .. } => None,
        }
    }

    /// A reply's proposal; `None` for a question or a request.
    pub fn proposal(&self) -> Option<&Proposal> {
        match &self.entry {
            ChatEntry::Reply {
                reply: ChatReply::Proposal(proposal),
                ..
            } => Some(proposal),
            _ => None,
        }
    }

    /// The option the user applied of a reply.
    pub fn applied(&self) -> Option<Applied> {
        match &self.entry {
            ChatEntry::Reply { applied, .. } => *applied,
            ChatEntry::Request { .. } => None,
        }
    }

    /// Option `option` of this reply, if it may be applied to a scene whose
    /// current image is `image`: an option is proposed for one image, and
    /// a reply whose result the user used is done with.
    pub fn option_for(
        &self,
        option: usize,
        image: Option<GenerationId>,
    ) -> Result<&ChatOption, ChatApplyError> {
        let proposal = self.proposal().ok_or(ChatApplyError::NotAProposal)?;
        let chosen = proposal
            .options
            .get(option)
            .ok_or(ChatApplyError::NoSuchOption)?;
        if self
            .applied()
            .is_some_and(|applied| applied.outcome == Some(ChatOutcome::Used))
        {
            return Err(ChatApplyError::AlreadyUsed);
        }
        if image != Some(proposal.image) {
            return Err(ChatApplyError::OutOfDate);
        }
        Ok(chosen)
    }

    /// Notes that the user applied `option` of this reply in `job`. A
    /// reply that is not a proposal is left as it is.
    pub fn apply(&mut self, option: usize, job: JobId) {
        if let ChatEntry::Reply {
            reply: ChatReply::Proposal(_),
            applied,
        } = &mut self.entry
        {
            *applied = Some(Applied {
                option,
                job,
                outcome: None,
            });
        }
    }

    /// Notes how the review of what `job` made ended. `false` when this is
    /// not the reply applied in `job`, or its review already ended.
    pub fn settle(&mut self, job: JobId, outcome: ChatOutcome) -> bool {
        match &mut self.entry {
            ChatEntry::Reply {
                applied: Some(applied),
                ..
            } if applied.job == job && applied.outcome.is_none() => {
                applied.outcome = Some(outcome);
                true
            }
            _ => false,
        }
    }

    /// A reply as stored: its kind, its options and what was applied.
    pub fn reply_json(&self) -> Option<String> {
        let ChatEntry::Reply { reply, applied } = &self.entry else {
            return None;
        };
        let mut value = match reply {
            ChatReply::Question => json!({ "kind": "question" }),
            ChatReply::Proposal(proposal) => json!({
                "kind": "proposal",
                "image": proposal.image.to_string(),
                "from_prompt": proposal.from_prompt,
                "recommended": proposal.recommended,
                "options": proposal.options.iter().map(|option| json!({
                    "kind": option.kind.code(),
                    "instruction": option.instruction,
                    "prompt": option.prompt,
                })).collect::<Vec<_>>(),
            }),
        };
        if let Some(applied) = applied {
            value["applied"] = json!({
                "option": applied.option,
                "job": applied.job.to_string(),
                "outcome": applied.outcome.map(ChatOutcome::code),
            });
        }
        Some(value.to_string())
    }

    /// A reply read back from [`SceneChatMessage::reply_json`]; `None` for
    /// text that is not one.
    pub fn entry_from_json(text: &str) -> Option<ChatEntry> {
        let value: Value = serde_json::from_str(text).ok()?;
        let reply = match value["kind"].as_str()? {
            "question" => ChatReply::Question,
            "proposal" => {
                let options = value["options"]
                    .as_array()?
                    .iter()
                    .map(|option| {
                        Some(ChatOption {
                            kind: ChatOptionKind::from_code(option["kind"].as_str()?)?,
                            instruction: option["instruction"].as_str()?.to_owned(),
                            prompt: option["prompt"].as_str()?.to_owned(),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                let image = uuid::Uuid::parse_str(value["image"].as_str()?).ok()?;
                let recommended = usize::try_from(value["recommended"].as_u64()?).ok()?;
                let proposal = Proposal::new(options, recommended, image.into()).ok()?;
                ChatReply::Proposal(match value["from_prompt"].as_str() {
                    Some(prompt) => proposal.with_prompt(prompt),
                    None => proposal,
                })
            }
            _ => return None,
        };
        let applied = match &value["applied"] {
            Value::Null => None,
            applied => Some(Applied {
                option: usize::try_from(applied["option"].as_u64()?).ok()?,
                job: uuid::Uuid::parse_str(applied["job"].as_str()?).ok()?.into(),
                outcome: match &applied["outcome"] {
                    Value::Null => None,
                    outcome => Some(ChatOutcome::from_code(outcome.as_str()?)?),
                },
            }),
        };
        Some(ChatEntry::Reply { reply, applied })
    }
}

/// Where scene chats are kept.
pub trait SceneChatRepository: Send + Sync {
    /// Scene `scene`'s messages in plan `plan`, oldest first.
    fn scene_chat(
        &self,
        plan: ScenePlanId,
        scene: usize,
    ) -> Result<Vec<SceneChatMessage>, RepositoryError>;

    /// One message, wherever it is.
    fn chat_message(
        &self,
        id: SceneChatMessageId,
    ) -> Result<Option<SceneChatMessage>, RepositoryError>;

    /// Adds `message`, or writes it over the saved one of the same id.
    /// Nothing is saved for a plan that is gone.
    fn save_chat_message(&self, message: &SceneChatMessage) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(instruction: &str) -> ChatOption {
        ChatOption {
            kind: ChatOptionKind::EditImage,
            instruction: instruction.to_owned(),
            prompt: format!("A kitchen. {instruction}"),
        }
    }

    fn reply_for(image: GenerationId) -> SceneChatMessage {
        let proposal = Proposal::new(vec![edit("Make the cup white.")], 0, image).unwrap();
        SceneChatMessage::reply(
            ScenePlanId::new(),
            2,
            "Vou deixar a xícara branca.",
            ChatReply::Proposal(proposal),
            JobId::new(),
            SystemTime::UNIX_EPOCH,
        )
    }

    #[test]
    fn a_request_is_trimmed_and_neither_empty_nor_too_long() {
        let request = |text: &str| {
            SceneChatMessage::request(
                ScenePlanId::new(),
                0,
                ChatTarget::Image,
                text,
                SystemTime::UNIX_EPOCH,
            )
        };
        assert_eq!(
            request("  a xícara branca \n").unwrap().text,
            "a xícara branca"
        );
        assert_eq!(request(" \n ").unwrap_err(), ChatRequestError::Empty);
        let long = "é".repeat(MAX_CHAT_REQUEST_CHARS + 1);
        assert_eq!(request(&long).unwrap_err(), ChatRequestError::TooLong);
        assert!(request(&"é".repeat(MAX_CHAT_REQUEST_CHARS)).is_ok());
    }

    #[test]
    fn a_proposal_needs_options_each_with_its_texts_and_none_twice() {
        let image = GenerationId::new();
        assert_eq!(
            Proposal::new(Vec::new(), 0, image).unwrap_err(),
            ProposalError::NoOptions
        );
        assert_eq!(
            Proposal::new(vec![edit(" ")], 0, image).unwrap_err(),
            ProposalError::Empty
        );
        assert_eq!(
            Proposal::new(vec![edit("a"), edit("b")], 0, image).unwrap_err(),
            ProposalError::Repeated
        );
        // A recommendation that names no option falls back to the first.
        assert_eq!(
            Proposal::new(vec![edit("a")], 7, image)
                .unwrap()
                .recommended(),
            0
        );
    }

    #[test]
    fn an_option_applies_only_to_the_image_it_was_proposed_for() {
        let image = GenerationId::new();
        let reply = reply_for(image);
        assert_eq!(
            reply.option_for(0, Some(image)).unwrap().kind,
            ChatOptionKind::EditImage
        );
        assert_eq!(
            reply.option_for(0, Some(GenerationId::new())).unwrap_err(),
            ChatApplyError::OutOfDate
        );
        assert_eq!(
            reply.option_for(0, None).unwrap_err(),
            ChatApplyError::OutOfDate
        );
        assert_eq!(
            reply.option_for(1, Some(image)).unwrap_err(),
            ChatApplyError::NoSuchOption
        );
    }

    #[test]
    fn a_question_and_a_request_propose_nothing() {
        let question = SceneChatMessage::reply(
            ScenePlanId::new(),
            0,
            "Qual xícara?",
            ChatReply::Question,
            JobId::new(),
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(
            question
                .option_for(0, Some(GenerationId::new()))
                .unwrap_err(),
            ChatApplyError::NotAProposal
        );
        let request = SceneChatMessage::request(
            ScenePlanId::new(),
            0,
            ChatTarget::Image,
            "branca",
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
        assert!(request.is_request());
        assert!(request.reply_json().is_none());
    }

    #[test]
    fn an_applied_option_is_settled_once_by_its_own_job_and_a_used_one_is_done() {
        let image = GenerationId::new();
        let mut reply = reply_for(image);
        let job = JobId::new();
        reply.apply(0, job);
        assert!(!reply.settle(JobId::new(), ChatOutcome::Used));
        assert!(reply.settle(job, ChatOutcome::Kept));
        assert!(!reply.settle(job, ChatOutcome::Used));
        // Kept: the user may apply it again.
        assert!(reply.option_for(0, Some(image)).is_ok());
        let again = JobId::new();
        reply.apply(0, again);
        assert!(reply.settle(again, ChatOutcome::Used));
        assert_eq!(
            reply.option_for(0, Some(image)).unwrap_err(),
            ChatApplyError::AlreadyUsed
        );
    }

    #[test]
    fn a_reply_reads_back_as_it_was_stored() {
        let mut reply = reply_for(GenerationId::new());
        let stored = SceneChatMessage::entry_from_json(&reply.reply_json().unwrap()).unwrap();
        assert_eq!(stored, reply.entry);
        let job = JobId::new();
        reply.apply(0, job);
        reply.settle(job, ChatOutcome::Kept);
        let stored = SceneChatMessage::entry_from_json(&reply.reply_json().unwrap()).unwrap();
        assert_eq!(stored, reply.entry);
        let question = SceneChatMessage::reply(
            ScenePlanId::new(),
            0,
            "Qual?",
            ChatReply::Question,
            JobId::new(),
            SystemTime::UNIX_EPOCH,
        );
        let stored = SceneChatMessage::entry_from_json(&question.reply_json().unwrap()).unwrap();
        assert_eq!(stored, question.entry);
        assert!(SceneChatMessage::entry_from_json("{\"kind\":\"other\"}").is_none());
    }
}
