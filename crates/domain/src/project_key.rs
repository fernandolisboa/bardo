//! A project's own API keys and budgets (#343, `docs/spec/projects.md`
//! "Keys and budgets per project"). A project may keep its own key for a
//! provider; work for the project then pays with it and counts against the
//! project's budget for that provider. Without one, the work uses the app's
//! key and counts against the app's budget, as before.
//!
//! The keys live in the secret store beside the app's (`SecretStore`);
//! what the database keeps is only that a project has one, so jobs, the
//! restore checklist and deletion know where to look.

use std::time::SystemTime;

use crate::{Budget, Money, ProfileId, ProjectId, Provider, RepositoryError};

/// Which key paid for a call: the app's, or the project's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyPayer {
    /// The key saved in Settings › API keys.
    #[default]
    App,
    /// The project's own key, set in its settings.
    Project(ProjectId),
}

impl KeyPayer {
    /// Which key work for `project` uses for a provider: the project's
    /// when it has its own, else the app's. Work for no project (a loose
    /// video, a global item) always uses the app's.
    pub fn for_work(project: Option<ProjectId>, project_has_key: bool) -> KeyPayer {
        match project {
            Some(project) if project_has_key => KeyPayer::Project(project),
            _ => KeyPayer::App,
        }
    }

    /// The project whose key it is; `None` for the app's.
    pub fn project(self) -> Option<ProjectId> {
        match self {
            KeyPayer::App => None,
            KeyPayer::Project(project) => Some(project),
        }
    }
}

/// That a project keeps its own key for a provider. The key itself is in
/// the secret store, never here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectKey {
    pub project: ProjectId,
    pub provider: Provider,
    pub saved_at: SystemTime,
}

/// A project's monthly limit for spending with its own key for a
/// provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectBudget {
    pub project: ProjectId,
    pub budget: Budget,
}

/// The budgets a call can count against: the app's, per provider, and each
/// project's, per provider it has its own key for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Budgets {
    pub app: Vec<Budget>,
    pub projects: Vec<ProjectBudget>,
}

impl Budgets {
    /// The budget spending with `payer`'s key for `provider` counts
    /// against. A project's key counts only against the project's budget,
    /// never the app's: a project key without a budget has none.
    pub fn of(&self, payer: KeyPayer, provider: Provider) -> Option<Money> {
        match payer {
            KeyPayer::App => self
                .app
                .iter()
                .find(|budget| budget.provider == provider)
                .map(|budget| budget.monthly),
            KeyPayer::Project(project) => self
                .projects
                .iter()
                .find(|set| set.project == project && set.budget.provider == provider)
                .map(|set| set.budget.monthly),
        }
    }
}

/// Persistence port for which projects keep their own keys. Shared with
/// job worker threads, which ask it before reading a key.
pub trait ProjectKeyRepository: Send + Sync {
    /// Every project key of the profile, by project then provider.
    fn project_keys(&self, owner: ProfileId) -> Result<Vec<ProjectKey>, RepositoryError>;

    /// Whether `project` keeps its own key for `provider`.
    fn has_project_key(
        &self,
        project: ProjectId,
        provider: Provider,
    ) -> Result<bool, RepositoryError>;

    /// Notes the key, replacing an earlier note for the same provider.
    fn save_project_key(&self, key: &ProjectKey) -> Result<(), RepositoryError>;

    /// Forgets the key, and the project's budget for the provider with
    /// it. Nothing happens when it is not there.
    fn remove_project_key(
        &self,
        project: ProjectId,
        provider: Provider,
    ) -> Result<(), RepositoryError>;
}

impl<T: ProjectKeyRepository + ?Sized> ProjectKeyRepository for std::sync::Arc<T> {
    fn project_keys(&self, owner: ProfileId) -> Result<Vec<ProjectKey>, RepositoryError> {
        (**self).project_keys(owner)
    }

    fn has_project_key(
        &self,
        project: ProjectId,
        provider: Provider,
    ) -> Result<bool, RepositoryError> {
        (**self).has_project_key(project, provider)
    }

    fn save_project_key(&self, key: &ProjectKey) -> Result<(), RepositoryError> {
        (**self).save_project_key(key)
    }

    fn remove_project_key(
        &self,
        project: ProjectId,
        provider: Provider,
    ) -> Result<(), RepositoryError> {
        (**self).remove_project_key(project, provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> ProjectId {
        ProjectId::new()
    }

    #[test]
    fn work_for_a_project_with_its_own_key_pays_with_it() {
        let p = project();
        assert_eq!(KeyPayer::for_work(Some(p), true), KeyPayer::Project(p));
        assert_eq!(KeyPayer::for_work(Some(p), false), KeyPayer::App);
        // A loose video or a global item has no project to pay.
        assert_eq!(KeyPayer::for_work(None, true), KeyPayer::App);
        assert_eq!(KeyPayer::Project(p).project(), Some(p));
        assert_eq!(KeyPayer::App.project(), None);
    }

    #[test]
    fn a_project_key_counts_only_against_the_projects_budget() {
        let (a, b) = (project(), project());
        let budgets = Budgets {
            app: vec![Budget {
                provider: Provider::Claude,
                monthly: Money::from_cents(5_000),
            }],
            projects: vec![ProjectBudget {
                project: a,
                budget: Budget {
                    provider: Provider::Claude,
                    monthly: Money::from_cents(1_000),
                },
            }],
        };
        assert_eq!(
            budgets.of(KeyPayer::App, Provider::Claude),
            Some(Money::from_cents(5_000))
        );
        assert_eq!(
            budgets.of(KeyPayer::Project(a), Provider::Claude),
            Some(Money::from_cents(1_000))
        );
        // Its own key without a budget of its own has none: the app's
        // budget is for the app's key.
        assert_eq!(budgets.of(KeyPayer::Project(b), Provider::Claude), None);
        assert_eq!(budgets.of(KeyPayer::Project(a), Provider::Gemini), None);
        assert_eq!(budgets.of(KeyPayer::App, Provider::Gemini), None);
    }
}
