//! The background agent's place in the operating system (ADR-0006, issue
//! #87): on Windows, a Task Scheduler task of the signed-in user that runs
//! `bardo --agent` when they sign in. Not a service, so it reads the user's
//! Credential Manager and needs no administrator.

use std::path::Path;

/// Registers, starts and removes the agent's task.
pub trait AgentTask: Send + Sync {
    /// Whether the task is registered for this user.
    fn is_registered(&self) -> Result<bool, AgentTaskError>;

    /// Registers the task to run `program --agent` when the user signs in,
    /// replacing one registered before (with an older program path).
    fn register(&self, program: &Path) -> Result<(), AgentTaskError>;

    /// Starts the registered task now, without waiting for a sign-in.
    fn start(&self) -> Result<(), AgentTaskError>;

    /// Ends the agent if it runs, and removes the task.
    fn remove(&self) -> Result<(), AgentTaskError>;
}

/// The system refused to change or read the task. The detail is for logs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the agent's task: {0}")]
pub struct AgentTaskError(pub String);
