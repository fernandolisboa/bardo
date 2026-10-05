//! Who runs jobs (ADR-0006, issue #87). Bardo's window (the app) and the
//! background agent both run jobs from the same SQLite file, each in its
//! own process: a **runner**.
//!
//! - A runner takes a job with a **lease** before it runs it: one statement
//!   that only succeeds when no other runner's lease on the job is still
//!   live. It renews the lease while the job runs and gives it up when the
//!   handler returns. A runner that dies (closed, crashed, slept) stops
//!   renewing, and its leases run out.
//! - A job stored as running whose lease ran out was abandoned: the next
//!   runner to lease it queues it again and resumes it from its checkpoint.
//! - Each runner says it is up (**presence**) every few seconds. The agent
//!   takes no new job while the app is up, and the app reads whether the
//!   agent runs from it.
//! - The claim on a scheduled post (`due`) is taken only by the runner that
//!   holds the lease on its upload job, so two runners never publish one
//!   post.

use std::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

use crate::RepositoryError;

/// How long a lease holds without being renewed.
pub const JOB_LEASE: Duration = Duration::from_secs(30);

/// How often a runner renews its leases, says it is up and reads what the
/// other runner changed.
pub const RUNNER_TICK: Duration = Duration::from_secs(5);

/// A runner not seen for this long is gone.
pub const RUNNER_ALIVE: Duration = Duration::from_secs(30);

/// Which Bardo process runs jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunnerRole {
    /// Bardo's window.
    App,
    /// The background agent (`bardo --agent`), started by Windows when the
    /// user signs in.
    Agent,
}

impl RunnerRole {
    pub fn code(self) -> &'static str {
        match self {
            RunnerRole::App => "app",
            RunnerRole::Agent => "agent",
        }
    }
}

impl fmt::Display for RunnerRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown runner role: {0}")]
pub struct UnknownRunnerRole(pub String);

impl FromStr for RunnerRole {
    type Err = UnknownRunnerRole;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [RunnerRole::App, RunnerRole::Agent]
            .into_iter()
            .find(|role| role.code() == s)
            .ok_or_else(|| UnknownRunnerRole(s.to_owned()))
    }
}

/// Another runner, as its presence says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerSeen {
    pub role: RunnerRole,
    /// When it started running jobs.
    pub started_at: SystemTime,
    /// When it last said it is up.
    pub seen_at: SystemTime,
}

/// A save refused because another runner holds the job's lease: that
/// runner's process owns the job until it gives it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the job is running in another Bardo process")]
pub struct JobHeldElsewhere;

impl RepositoryError {
    /// Whether the job was left alone because another runner holds it.
    pub fn is_held_elsewhere(&self) -> bool {
        self.0.downcast_ref::<JobHeldElsewhere>().is_some()
    }
}

impl From<JobHeldElsewhere> for RepositoryError {
    fn from(error: JobHeldElsewhere) -> Self {
        RepositoryError(Box::new(error))
    }
}

/// Since when Bardo has been running, for the due-time rules: this runner's
/// start, or an earlier one of another runner still up (`others`), since a
/// due time that passed while either ran did not pass while Bardo was
/// closed.
pub fn running_since(started_at: SystemTime, others: &[RunnerSeen]) -> SystemTime {
    others
        .iter()
        .map(|other| other.started_at)
        .fold(started_at, SystemTime::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_round_trip() {
        for role in [RunnerRole::App, RunnerRole::Agent] {
            assert_eq!(role.code().parse::<RunnerRole>(), Ok(role));
        }
        assert!("service".parse::<RunnerRole>().is_err());
    }

    #[test]
    fn a_held_elsewhere_error_is_recognised_through_the_repository_error() {
        assert!(RepositoryError::from(JobHeldElsewhere).is_held_elsewhere());
        assert!(!RepositoryError("disk full".into()).is_held_elsewhere());
    }

    #[test]
    fn bardo_runs_since_the_earliest_runner_still_up() {
        let at = |secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        assert_eq!(running_since(at(100), &[]), at(100));
        let agent = RunnerSeen {
            role: RunnerRole::Agent,
            started_at: at(40),
            seen_at: at(99),
        };
        assert_eq!(running_since(at(100), &[agent]), at(40));
        let later = RunnerSeen {
            started_at: at(150),
            ..agent
        };
        assert_eq!(running_since(at(100), &[later]), at(100));
    }
}
