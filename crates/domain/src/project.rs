//! Projects (ADR-0047): what the user opens to work, one at a time. A
//! project owns its channels, and through them their themes and videos.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

use uuid::Uuid;

use crate::{ChannelId, ProfileId, RepositoryError};

/// Identifies a project. Not a video: `VideoProjectId` names one of those.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for ProjectId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The color that tells a project apart in the switcher, the same in
/// every interface theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProjectColor {
    #[default]
    Amber,
    Teal,
    Violet,
    Rose,
    Sky,
    Lime,
    Orange,
    Slate,
}

impl ProjectColor {
    pub const ALL: [ProjectColor; 8] = [
        ProjectColor::Amber,
        ProjectColor::Teal,
        ProjectColor::Violet,
        ProjectColor::Rose,
        ProjectColor::Sky,
        ProjectColor::Lime,
        ProjectColor::Orange,
        ProjectColor::Slate,
    ];

    pub fn code(self) -> &'static str {
        match self {
            ProjectColor::Amber => "amber",
            ProjectColor::Teal => "teal",
            ProjectColor::Violet => "violet",
            ProjectColor::Rose => "rose",
            ProjectColor::Sky => "sky",
            ProjectColor::Lime => "lime",
            ProjectColor::Orange => "orange",
            ProjectColor::Slate => "slate",
        }
    }

    /// A color of a newer Bardo reads as the first one: the project still
    /// opens.
    pub fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|color| color.code() == code)
            .unwrap_or_default()
    }

    /// The color after the ones `taken`, so new projects start apart; the
    /// least used one once all are taken.
    pub fn next_after(taken: &[ProjectColor]) -> Self {
        Self::ALL
            .into_iter()
            .min_by_key(|color| taken.iter().filter(|used| *used == color).count())
            .unwrap_or_default()
    }
}

/// Whether the project shows in the switcher and takes new work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProjectState {
    #[default]
    Active,
    Archived,
}

impl ProjectState {
    pub fn code(self) -> &'static str {
        match self {
            ProjectState::Active => "active",
            ProjectState::Archived => "archived",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown project state: {0}")]
pub struct UnknownProjectState(pub String);

impl FromStr for ProjectState {
    type Err = UnknownProjectState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(ProjectState::Active),
            "archived" => Ok(ProjectState::Archived),
            other => Err(UnknownProjectState(other.to_owned())),
        }
    }
}

/// A project's fields as the user typed them, before validation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectDraft {
    pub name: String,
    pub color: ProjectColor,
}

/// Why a draft is not a valid project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectFieldError {
    NameRequired,
    NameTooLong,
}

/// The user-editable part of a project, always valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDetails {
    name: String,
    color: ProjectColor,
}

impl ProjectDetails {
    /// In characters, as a channel's name.
    pub const MAX_NAME_CHARS: usize = 100;

    /// Trims the name and checks it.
    pub fn validate(draft: ProjectDraft) -> Result<Self, ProjectFieldError> {
        let name = draft.name.trim().to_owned();
        if name.is_empty() {
            return Err(ProjectFieldError::NameRequired);
        }
        if name.chars().count() > Self::MAX_NAME_CHARS {
            return Err(ProjectFieldError::NameTooLong);
        }
        Ok(Self {
            name,
            color: draft.color,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn color(&self) -> ProjectColor {
        self.color
    }

    /// Whether two project names read as the same to the user: names are
    /// unique per profile, ignoring case.
    pub fn same_name(&self, other: &str) -> bool {
        self.name.to_lowercase() == other.trim().to_lowercase()
    }
}

impl From<&ProjectDetails> for ProjectDraft {
    fn from(details: &ProjectDetails) -> Self {
        Self {
            name: details.name.clone(),
            color: details.color,
        }
    }
}

/// What the user opens to work (CONTEXT.md): a name unique per profile, a
/// color, its channels (none while new) and, through them, their themes
/// and videos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: ProjectId,
    pub owner: ProfileId,
    pub details: ProjectDetails,
    pub state: ProjectState,
    /// Its place among the starred projects, from 0; `None` when not
    /// starred.
    pub favorite: Option<u32>,
    /// When the user last opened it; `None` until then.
    pub opened_at: Option<SystemTime>,
    pub created_at: SystemTime,
}

impl Project {
    /// A new active project, neither starred nor opened yet.
    pub fn new(owner: ProfileId, details: ProjectDetails, now: SystemTime) -> Self {
        Self {
            id: ProjectId::new(),
            owner,
            details,
            state: ProjectState::Active,
            favorite: None,
            opened_at: None,
            created_at: now,
        }
    }

    pub fn name(&self) -> &str {
        self.details.name()
    }

    pub fn is_active(&self) -> bool {
        self.state == ProjectState::Active
    }
}

/// The projects as the switcher lists them: the starred ones in the
/// user's order, the last ones opened, then the rest by name. Archived
/// projects are in none of them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectList {
    pub favorites: Vec<Project>,
    /// Most recent first, favorites left out.
    pub recent: Vec<Project>,
    /// By name, ignoring case.
    pub all: Vec<Project>,
}

impl ProjectList {
    /// How many projects *Recent* holds.
    pub const RECENT: usize = 8;

    /// Arranges `projects`, keeping those whose name holds `query`
    /// (ignoring case; an empty query keeps every one).
    pub fn arrange(projects: Vec<Project>, query: &str) -> Self {
        let query = query.trim().to_lowercase();
        let mut active: Vec<Project> = projects
            .into_iter()
            .filter(|project| project.is_active())
            .filter(|project| query.is_empty() || project.name().to_lowercase().contains(&query))
            .collect();
        active.sort_by(|a, b| {
            a.name()
                .to_lowercase()
                .cmp(&b.name().to_lowercase())
                .then(a.created_at.cmp(&b.created_at))
        });
        let (mut favorites, rest): (Vec<_>, Vec<_>) = active
            .into_iter()
            .partition(|project| project.favorite.is_some());
        favorites.sort_by_key(|project| project.favorite);
        let (mut opened, never): (Vec<_>, Vec<_>) = rest
            .into_iter()
            .partition(|project| project.opened_at.is_some());
        opened.sort_by(|a, b| b.opened_at.cmp(&a.opened_at));
        let older = opened.split_off(opened.len().min(Self::RECENT));
        let mut all: Vec<Project> = older.into_iter().chain(never).collect();
        all.sort_by(|a, b| {
            a.name()
                .to_lowercase()
                .cmp(&b.name().to_lowercase())
                .then(a.created_at.cmp(&b.created_at))
        });
        Self {
            favorites,
            recent: opened,
            all,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.favorites.is_empty() && self.recent.is_empty() && self.all.is_empty()
    }
}

/// The starred projects in the user's order, after `id` moves to `to`
/// (clamped to the end).
pub fn reorder_favorites(order: &[ProjectId], id: ProjectId, to: usize) -> Vec<ProjectId> {
    let mut order: Vec<ProjectId> = order.iter().copied().filter(|other| *other != id).collect();
    order.insert(to.min(order.len()), id);
    order
}

/// Persistence port for projects.
pub trait ProjectRepository: Send + Sync {
    /// The owner's projects, archived ones too, ordered by name.
    fn projects(&self, owner: ProfileId) -> Result<Vec<Project>, RepositoryError>;

    fn project(&self, id: ProjectId) -> Result<Option<Project>, RepositoryError>;

    /// Inserts the project, or updates its name, color and state. Its star
    /// and when it was opened change only through the calls below.
    fn save_project(&self, project: &Project) -> Result<(), RepositoryError>;

    /// Records that the user opened the project at `at`.
    fn mark_opened(&self, id: ProjectId, at: SystemTime) -> Result<(), RepositoryError>;

    /// Stars exactly the owner's projects in `order`, in that order; every
    /// other one loses its star.
    fn set_favorites(&self, owner: ProfileId, order: &[ProjectId]) -> Result<(), RepositoryError>;

    /// Gives the channel to another project, with its themes, videos and
    /// network accounts (they follow their channel).
    fn move_channel(&self, channel: ChannelId, to: ProjectId) -> Result<(), RepositoryError>;
}

impl<T: ProjectRepository + ?Sized> ProjectRepository for Arc<T> {
    fn projects(&self, owner: ProfileId) -> Result<Vec<Project>, RepositoryError> {
        (**self).projects(owner)
    }

    fn project(&self, id: ProjectId) -> Result<Option<Project>, RepositoryError> {
        (**self).project(id)
    }

    fn save_project(&self, project: &Project) -> Result<(), RepositoryError> {
        (**self).save_project(project)
    }

    fn mark_opened(&self, id: ProjectId, at: SystemTime) -> Result<(), RepositoryError> {
        (**self).mark_opened(id, at)
    }

    fn set_favorites(&self, owner: ProfileId, order: &[ProjectId]) -> Result<(), RepositoryError> {
        (**self).set_favorites(owner, order)
    }

    fn move_channel(&self, channel: ChannelId, to: ProjectId) -> Result<(), RepositoryError> {
        (**self).move_channel(channel, to)
    }
}

/// The project, channel and video a job or a cost was for, with their
/// names when it was first saved: they stay after a rename, a move or a
/// deletion. The store fills it in; what a caller sets is ignored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkOrigin {
    pub project: Option<ProjectId>,
    pub project_name: Option<String>,
    pub channel_name: Option<String>,
    pub video_title: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn project(name: &str) -> Project {
        Project::new(
            ProfileId::new(),
            ProjectDetails::validate(ProjectDraft {
                name: name.into(),
                color: ProjectColor::Teal,
            })
            .unwrap(),
            at(1),
        )
    }

    fn names(projects: &[Project]) -> Vec<&str> {
        projects.iter().map(Project::name).collect()
    }

    #[test]
    fn a_name_is_trimmed_required_and_limited() {
        let details = ProjectDetails::validate(ProjectDraft {
            name: "  Space Archives ".into(),
            color: ProjectColor::Rose,
        })
        .unwrap();
        assert_eq!(details.name(), "Space Archives");
        assert_eq!(details.color(), ProjectColor::Rose);
        assert_eq!(
            ProjectDetails::validate(ProjectDraft::default()),
            Err(ProjectFieldError::NameRequired)
        );
        let long = "é".repeat(ProjectDetails::MAX_NAME_CHARS + 1);
        assert_eq!(
            ProjectDetails::validate(ProjectDraft {
                name: long,
                ..ProjectDraft::default()
            }),
            Err(ProjectFieldError::NameTooLong)
        );
        assert!(details.same_name(" space ARCHIVES "));
    }

    #[test]
    fn colors_round_trip_and_an_unknown_one_reads_as_the_first() {
        for color in ProjectColor::ALL {
            assert_eq!(ProjectColor::from_code(color.code()), color);
        }
        assert_eq!(ProjectColor::from_code("plaid"), ProjectColor::Amber);
    }

    #[test]
    fn a_new_project_takes_a_color_no_other_has() {
        assert_eq!(ProjectColor::next_after(&[]), ProjectColor::Amber);
        assert_eq!(
            ProjectColor::next_after(&[ProjectColor::Amber, ProjectColor::Teal]),
            ProjectColor::Violet
        );
        let every: Vec<_> = ProjectColor::ALL
            .into_iter()
            .chain([ProjectColor::Amber])
            .collect();
        assert_eq!(ProjectColor::next_after(&every), ProjectColor::Teal);
    }

    #[test]
    fn states_round_trip() {
        for state in [ProjectState::Active, ProjectState::Archived] {
            assert_eq!(state.code().parse::<ProjectState>(), Ok(state));
        }
        assert!("gone".parse::<ProjectState>().is_err());
    }

    #[test]
    fn favorites_come_in_the_users_order_and_leave_recent() {
        let mut b = project("Bravo");
        b.favorite = Some(1);
        b.opened_at = Some(at(50));
        let mut c = project("Charlie");
        c.favorite = Some(0);
        let mut a = project("alpha");
        a.opened_at = Some(at(10));
        let list = ProjectList::arrange(vec![a, b, c], "");
        assert_eq!(names(&list.favorites), ["Charlie", "Bravo"]);
        assert_eq!(names(&list.recent), ["alpha"]);
        assert!(list.all.is_empty());
    }

    #[test]
    fn recent_holds_the_last_eight_opened_and_the_rest_go_by_name() {
        let projects: Vec<Project> = (0..10)
            .map(|i| {
                let mut project = project(&format!("P{i}"));
                project.opened_at = Some(at(100 + i));
                project
            })
            .chain([project("never opened")])
            .collect();
        let list = ProjectList::arrange(projects, "");
        assert_eq!(
            names(&list.recent),
            ["P9", "P8", "P7", "P6", "P5", "P4", "P3", "P2"]
        );
        assert_eq!(names(&list.all), ["never opened", "P0", "P1"]);
    }

    #[test]
    fn the_search_keeps_matching_names_and_archived_projects_never_show() {
        let mut archived = project("Space Old");
        archived.state = ProjectState::Archived;
        let list = ProjectList::arrange(
            vec![project("Space Archives"), project("Deep Sea"), archived],
            " SPACE ",
        );
        assert_eq!(names(&list.all), ["Space Archives"]);
        assert!(ProjectList::arrange(vec![project("Deep Sea")], "space").is_empty());
    }

    #[test]
    fn a_favorite_moves_to_its_new_place() {
        let [a, b, c] = [ProjectId::new(), ProjectId::new(), ProjectId::new()];
        assert_eq!(reorder_favorites(&[a, b, c], c, 0), [c, a, b]);
        assert_eq!(reorder_favorites(&[a, b, c], a, 1), [b, a, c]);
        assert_eq!(reorder_favorites(&[a, b], c, 9), [a, b, c]);
    }
}
