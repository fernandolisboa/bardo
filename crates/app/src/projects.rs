//! Project use cases (ADR-0047): the project switcher, opening one project
//! at a time, creating projects, starring them, and moving a channel from
//! one project to another.
//!
//! The open project is the active project the user opened last: opening
//! one records when, so the choice survives a restart with nothing else
//! to keep.

use std::time::SystemTime;

use bardo_domain::{
    Channel, ChannelId, ChannelRepository, Job, LocalizationRepository, Project, ProjectColor,
    ProjectDetails, ProjectDraft, ProjectFieldError, ProjectId, ProjectList, ProjectRepository,
    PublicationRepository, RepositoryError, ThemeRepository, VideoProjectId, reorder_favorites,
};

use crate::{AppError, Bardo, Text};

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("invalid project: {0:?}")]
    Invalid(ProjectFieldError),
    #[error("a project with this name already exists")]
    NameTaken,
    #[error("project not found")]
    NotFound,
    #[error("channel not found")]
    ChannelNotFound,
    /// The project's folder there would leave no room under the Windows
    /// path limit for its deepest file.
    #[error("the folder is too deep")]
    FolderTooDeep,
    /// The project's folder could not be made there.
    #[error("the folder could not be made")]
    FolderNotMade,
    /// The channel cannot leave its project now; each reason once.
    #[error("the channel cannot move: {0:?}")]
    MoveBlocked(Vec<MoveBlock>),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl From<AppError> for ProjectError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Repository(error) => ProjectError::Repository(error),
        }
    }
}

impl ProjectError {
    /// What the form or the switcher says.
    pub fn message(&self) -> Text {
        match self {
            ProjectError::Invalid(ProjectFieldError::NameRequired) => Text::ProjectNameRequired,
            ProjectError::Invalid(ProjectFieldError::NameTooLong) => Text::ProjectNameTooLong,
            ProjectError::NameTaken => Text::ProjectNameTaken,
            ProjectError::NotFound => Text::ProjectNotFoundAny,
            ProjectError::ChannelNotFound => Text::ChannelNotFound,
            ProjectError::FolderTooDeep => Text::ProjectFolderTooDeep,
            ProjectError::FolderNotMade => Text::ProjectFolderNotMade,
            ProjectError::MoveBlocked(_) => Text::MoveChannelBlocked,
            ProjectError::Repository(_) => Text::ProjectNotSaved,
        }
    }
}

/// Why a channel cannot move to another project now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MoveBlock {
    /// A post Bardo publishes at its time (the in-app schedule) waits.
    ScheduledPost,
    /// A post waits on its network to go public at a publish time.
    HeldOnNetwork,
    /// A job for the channel or one of its videos is queued or running.
    ActiveJob,
    /// One of its videos is a localized version of, or has one in,
    /// another channel, which would stay behind: versions stay inside a
    /// project.
    VersionElsewhere,
    /// The channel or one of its videos uses a narrator, product or
    /// template its project owns: nothing points into another project.
    ProjectItems,
}

impl MoveBlock {
    pub const ALL: [MoveBlock; 5] = [
        MoveBlock::ScheduledPost,
        MoveBlock::HeldOnNetwork,
        MoveBlock::ActiveJob,
        MoveBlock::VersionElsewhere,
        MoveBlock::ProjectItems,
    ];

    pub fn message(self) -> Text {
        Text::MoveChannelBlock(self)
    }
}

/// Which projects' jobs or costs a screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProjectFilter {
    /// Every one of the profile's.
    #[default]
    All,
    /// One project's.
    Project(ProjectId),
    /// Those for no project: backups, updates, the persona library.
    NoProject,
}

impl ProjectFilter {
    /// Whether work done for `project` passes the filter.
    pub fn keeps(self, project: Option<ProjectId>) -> bool {
        match self {
            ProjectFilter::All => true,
            ProjectFilter::Project(id) => project == Some(id),
            ProjectFilter::NoProject => project.is_none(),
        }
    }
}

impl Bardo {
    /// Every project of the profile, archived ones too, by name.
    pub fn projects(&self) -> Result<Vec<Project>, AppError> {
        Ok(self.projects.projects(self.profile.id)?)
    }

    /// The projects as the switcher lists them, keeping those whose name
    /// holds `query`.
    pub fn project_list(&self, query: &str) -> Result<ProjectList, AppError> {
        Ok(ProjectList::arrange(self.projects()?, query))
    }

    /// The project the work screens show: the active one opened last.
    /// `None` only while the profile has no active project.
    pub fn open_project(&self) -> Result<Option<Project>, AppError> {
        Ok(self
            .projects()?
            .into_iter()
            .filter(Project::is_active)
            .max_by_key(|project| project.opened_at))
    }

    pub(crate) fn open_project_id(&self) -> Result<Option<ProjectId>, RepositoryError> {
        Ok(self
            .projects
            .projects(self.profile.id)?
            .into_iter()
            .filter(Project::is_active)
            .max_by_key(|project| project.opened_at)
            .map(|project| project.id))
    }

    /// Opens a project the profile has never opened, so the app opens on
    /// one: after the upgrade that made projects, the first by name.
    pub(crate) fn open_some_project(&self) -> Result<(), RepositoryError> {
        let projects = self.projects.projects(self.profile.id)?;
        if projects
            .iter()
            .any(|p| p.is_active() && p.opened_at.is_some())
        {
            return Ok(());
        }
        if let Some(first) = projects.iter().find(|p| p.is_active()) {
            self.projects.mark_opened(first.id, SystemTime::now())?;
        }
        Ok(())
    }

    /// Opens `id`: the work screens show it from now on, and from the next
    /// start.
    pub fn switch_project(&self, id: ProjectId) -> Result<Project, ProjectError> {
        let project = self.profile_project(id)?;
        if !project.is_active() {
            return Err(ProjectError::NotFound);
        }
        self.projects.mark_opened(id, SystemTime::now())?;
        self.profile_project(id)
    }

    fn profile_project(&self, id: ProjectId) -> Result<Project, ProjectError> {
        self.projects
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ProjectError::NotFound)
    }

    /// A color no other project has yet, for the new project form.
    pub fn next_project_color(&self) -> ProjectColor {
        let taken: Vec<ProjectColor> = self
            .projects()
            .unwrap_or_default()
            .iter()
            .filter(|project| project.is_active())
            .map(|project| project.details.color())
            .collect();
        ProjectColor::next_after(&taken)
    }

    /// Creates a project, with its folder under the base folder, and opens
    /// it. It has no channel yet: the screens ask for one.
    pub fn create_project(&self, draft: ProjectDraft) -> Result<Project, ProjectError> {
        self.create_project_in(draft, ProjectId::new(), None)
    }

    /// `create_project` with the id the form showed the folder for, and its
    /// folder in `place` (the base folder when `None`).
    pub fn create_project_in(
        &self,
        draft: ProjectDraft,
        id: ProjectId,
        place: Option<&std::path::Path>,
    ) -> Result<Project, ProjectError> {
        let details = ProjectDetails::validate(draft).map_err(ProjectError::Invalid)?;
        if self
            .projects()?
            .iter()
            .any(|other| other.details.same_name(details.name()))
        {
            return Err(ProjectError::NameTaken);
        }
        let mut project = Project::new(self.profile.id, details, SystemTime::now());
        project.id = id;
        self.make_project_folder(&mut project, place)?;
        self.projects.save_project(&project)?;
        self.switch_project(project.id)
    }

    /// The project a new channel joins: the open one, or, when the profile
    /// has none yet, a new project of the channel's name, opened.
    pub(crate) fn project_for_new_channel(&self, name: &str) -> Result<ProjectId, ProjectError> {
        match self.open_project_id()? {
            Some(open) => Ok(open),
            None => self.add_project_named(name),
        }
    }

    /// The active project called `name`, or a new one of that name (with a
    /// number added when an archived project holds it). A new one is
    /// opened only when no project is open yet.
    pub(crate) fn project_named(&self, name: &str) -> Result<ProjectId, ProjectError> {
        let found = self
            .projects()?
            .into_iter()
            .find(|project| project.is_active() && project.details.same_name(name));
        match found {
            Some(project) => Ok(project.id),
            None => self.add_project_named(name),
        }
    }

    fn add_project_named(&self, name: &str) -> Result<ProjectId, ProjectError> {
        let taken: Vec<String> = self
            .projects()?
            .iter()
            .map(|project| project.name().to_lowercase())
            .collect();
        let name = name.trim();
        let numbered = |n: usize| {
            // Room for the number, so the name stays within the limit.
            let suffix = format!(" {n}");
            let keep = ProjectDetails::MAX_NAME_CHARS.saturating_sub(suffix.chars().count());
            let base: String = name.chars().take(keep).collect();
            format!("{}{suffix}", base.trim_end())
        };
        let name = std::iter::once(name.to_owned())
            .chain((2..).map(numbered))
            .find(|candidate| !taken.contains(&candidate.to_lowercase()))
            .unwrap_or_else(|| name.to_owned());
        let details = ProjectDetails::validate(ProjectDraft {
            name,
            color: self.next_project_color(),
        })
        .map_err(ProjectError::Invalid)?;
        let mut project = Project::new(self.profile.id, details, SystemTime::now());
        // Made for a channel or an import: without its folder now, it gets
        // one at the next start.
        let _ = self.make_project_folder(&mut project, None);
        self.projects.save_project(&project)?;
        if self.open_project_id()?.is_none() {
            self.projects.mark_opened(project.id, SystemTime::now())?;
        }
        Ok(project.id)
    }

    /// Stars `id` at the end of the favorites, or takes its star away.
    pub fn star_project(&self, id: ProjectId, starred: bool) -> Result<(), ProjectError> {
        self.profile_project(id)?;
        let mut order = self.favorite_order()?;
        order.retain(|other| *other != id);
        if starred {
            order.push(id);
        }
        Ok(self.projects.set_favorites(self.profile.id, &order)?)
    }

    /// Moves a starred project to place `to` among the favorites.
    pub fn move_favorite(&self, id: ProjectId, to: usize) -> Result<(), ProjectError> {
        let order = self.favorite_order()?;
        if !order.contains(&id) {
            return Err(ProjectError::NotFound);
        }
        Ok(self
            .projects
            .set_favorites(self.profile.id, &reorder_favorites(&order, id, to))?)
    }

    fn favorite_order(&self) -> Result<Vec<ProjectId>, RepositoryError> {
        let mut starred: Vec<Project> = self
            .projects
            .projects(self.profile.id)?
            .into_iter()
            .filter(|project| project.favorite.is_some())
            .collect();
        starred.sort_by_key(|project| project.favorite);
        Ok(starred.into_iter().map(|project| project.id).collect())
    }

    /// A project's channels, by name.
    pub fn project_channels(&self, project: ProjectId) -> Result<Vec<Channel>, AppError> {
        Ok(self
            .channels
            .list(self.profile.id)?
            .into_iter()
            .filter(|channel| channel.project == project)
            .collect())
    }

    /// Every channel of the profile, in every project, by name: for what
    /// spans the profile (backups, the settings file, diagnostics).
    pub fn all_channels(&self) -> Result<Vec<Channel>, AppError> {
        Ok(self.channels.list(self.profile.id)?)
    }

    /// What stops `channel` from moving to another project now, each
    /// reason once; empty when it can move.
    pub fn move_channel_blocks(&self, channel: ChannelId) -> Result<Vec<MoveBlock>, ProjectError> {
        let channel = self.profile_channel(channel)?;
        let mut blocks = Vec::new();
        let publications = self.publications.channel_publications(channel.id)?;
        if publications.iter().any(|publication| {
            publication.due().is_some()
                && publication.upload().is_some_and(|upload| {
                    matches!(upload.status, bardo_domain::UploadStatus::Queued)
                })
        }) {
            blocks.push(MoveBlock::ScheduledPost);
        }
        if publications.iter().any(|p| p.is_scheduled()) {
            blocks.push(MoveBlock::HeldOnNetwork);
        }
        let all_videos = self.themes.projects(channel.id)?;
        let videos: Vec<VideoProjectId> = all_videos.iter().map(|video| video.id).collect();
        if self
            .jobs()
            .iter()
            .filter(|job| job.state().is_active())
            .any(|job| self.job_is_for(job, channel.id, &videos))
        {
            blocks.push(MoveBlock::ActiveJob);
        }
        for video in &videos {
            let source_elsewhere = self
                .came_from_channel(*video)?
                .is_some_and(|source| source != channel.id);
            let version_elsewhere = self
                .localizations
                .versions(*video)?
                .iter()
                .any(|version| version.channel != channel.id);
            if source_elsewhere || version_elsewhere {
                blocks.push(MoveBlock::VersionElsewhere);
                break;
            }
        }
        if self.uses_project_items(&channel, &all_videos)? {
            blocks.push(MoveBlock::ProjectItems);
        }
        Ok(blocks)
    }

    /// Gives `channel` to project `to`, with its themes, videos and network
    /// accounts, unless something blocks it (`move_channel_blocks`).
    pub fn move_channel(&self, channel: ChannelId, to: ProjectId) -> Result<(), ProjectError> {
        let own = self.profile_channel(channel)?;
        let target = self.profile_project(to)?;
        if !target.is_active() {
            return Err(ProjectError::NotFound);
        }
        if own.project == to {
            return Ok(());
        }
        let blocks = self.move_channel_blocks(channel)?;
        if !blocks.is_empty() {
            return Err(ProjectError::MoveBlocked(blocks));
        }
        self.projects.move_channel(channel, to)?;
        // Its videos' media follows it into the project's folder.
        let _ = self.queue_media_move();
        Ok(())
    }

    /// Whether `channel` or one of its `videos` uses a narrator, product or
    /// template owned by the channel's project.
    fn uses_project_items(
        &self,
        channel: &Channel,
        videos: &[bardo_domain::VideoProject],
    ) -> Result<bool, RepositoryError> {
        let project = Some(channel.project);
        let own_persona = |id: Option<bardo_domain::PersonaId>| -> Result<bool, RepositoryError> {
            Ok(match id {
                Some(id) => self
                    .personas
                    .get(id)?
                    .is_some_and(|persona| persona.owner_project == project),
                None => false,
            })
        };
        if own_persona(channel.details.default_persona())? {
            return Ok(true);
        }
        for video in videos {
            if own_persona(video.persona)? {
                return Ok(true);
            }
            if let Some(product) = video.product
                && self
                    .products
                    .get(product)?
                    .is_some_and(|product| product.owner_project == project)
            {
                return Ok(true);
            }
        }
        let templated = self.templates.videos_using_templates_of(channel.project)?;
        Ok(videos.iter().any(|video| templated.contains(&video.id)))
    }

    fn profile_channel(&self, id: ChannelId) -> Result<Channel, ProjectError> {
        self.channels
            .get(id)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(ProjectError::ChannelNotFound)
    }

    /// The channel of the video `video` was localized from, if any.
    fn came_from_channel(
        &self,
        video: VideoProjectId,
    ) -> Result<Option<ChannelId>, RepositoryError> {
        match self
            .localizations
            .localization(video)?
            .and_then(|localization| localization.source)
        {
            Some(source) => Ok(self.themes.project(source)?.map(|project| project.channel)),
            None => Ok(None),
        }
    }

    /// Whether `job`'s payload names `channel` or one of its `videos`.
    fn job_is_for(&self, job: &Job, channel: ChannelId, videos: &[VideoProjectId]) -> bool {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(job.payload()) else {
            return false;
        };
        let named = |key: &str| payload.get(key).and_then(serde_json::Value::as_str);
        named("channel").is_some_and(|id| id == channel.to_string())
            || named("project").is_some_and(|id| videos.iter().any(|video| video.to_string() == id))
    }
}

/// A project saved straight to the store, for tests that set up another
/// profile's channels.
#[cfg(test)]
pub(crate) fn saved_project(
    store: &dyn ProjectRepository,
    owner: bardo_domain::ProfileId,
) -> ProjectId {
    let project = Project::new(
        owner,
        ProjectDetails::validate(ProjectDraft {
            name: format!("Project {}", ProjectId::new()),
            ..Default::default()
        })
        .expect("valid project"),
        std::time::SystemTime::now(),
    );
    store.save_project(&project).expect("project saved");
    project.id
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bardo_domain::{ChannelDraft, ProjectRepository, ProjectState};
    use bardo_storage::Database;

    use super::*;
    use crate::{Repositories, testing};

    fn start(db: &Arc<Database>) -> Bardo {
        let repositories = Repositories::shared(
            Arc::clone(db),
            Arc::new(bardo_storage::MemorySecretStore::default()),
        );
        Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap()
    }

    fn app() -> (Bardo, Arc<Database>) {
        let db = Arc::new(Database::open_in_memory().unwrap());
        (start(&db), db)
    }

    fn draft(name: &str) -> ProjectDraft {
        ProjectDraft {
            name: name.into(),
            color: ProjectColor::Teal,
        }
    }

    fn channel(app: &Bardo, name: &str) -> Channel {
        app.create_channel(ChannelDraft {
            name: name.into(),
            ..ChannelDraft::default()
        })
        .unwrap()
    }

    /// A video of `channel`, started from an approved theme.
    fn video(app: &Bardo, channel: ChannelId) -> bardo_domain::VideoProject {
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel,
            bardo_domain::Niche::new("space history").unwrap(),
            bardo_domain::ThemeIdea::new("Apollo", "The last signal.").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let video = theme.approve(SystemTime::now()).unwrap();
        app.themes.start_project(&theme, &video).unwrap();
        video
    }

    #[test]
    fn a_new_profile_has_no_project_until_its_first_channel_makes_one() {
        let (app, _) = app();
        assert_eq!(app.open_project().unwrap(), None);
        assert!(app.project_list("").unwrap().is_empty());

        let space = channel(&app, "Space Archives");
        let open = app.open_project().unwrap().unwrap();
        assert_eq!(open.name(), "Space Archives");
        assert_eq!(space.project, open.id);
        // The next channel joins the open project.
        assert_eq!(channel(&app, "Deep Sea").project, open.id);
        assert_eq!(app.projects().unwrap().len(), 1);
    }

    #[test]
    fn a_new_project_opens_with_no_channel_and_the_screens_show_none() {
        let (app, _) = app();
        channel(&app, "Space Archives");
        let created = app.create_project(draft("  Cooking ")).unwrap();
        assert_eq!(created.name(), "Cooking");
        assert_eq!(app.open_project().unwrap().unwrap().id, created.id);
        assert!(app.channels().unwrap().is_empty());
        assert!(app.project_channels(created.id).unwrap().is_empty());
        // The other project's channel is still the profile's.
        assert_eq!(app.all_channels().unwrap().len(), 1);
    }

    #[test]
    fn project_names_are_checked_and_unique_ignoring_case() {
        let (app, _) = app();
        app.create_project(draft("Cooking")).unwrap();
        assert!(matches!(
            app.create_project(draft(" COOKING")),
            Err(ProjectError::NameTaken)
        ));
        let error = app.create_project(draft("  ")).unwrap_err();
        assert_eq!(error.message(), Text::ProjectNameRequired);
        assert_eq!(app.projects().unwrap().len(), 1);
    }

    #[test]
    fn a_first_channel_named_like_a_project_gets_a_free_project_name() {
        let (app, db) = app();
        let cooking = app.create_project(draft("Cooking")).unwrap();
        let mut archived = cooking.clone();
        archived.state = ProjectState::Archived;
        db.save_project(&archived).unwrap();
        assert_eq!(app.open_project().unwrap(), None);

        let channel = channel(&app, "cooking");
        let open = app.open_project().unwrap().unwrap();
        assert_eq!(open.name(), "cooking 2");
        assert_eq!(channel.project, open.id);
    }

    #[test]
    fn a_numbered_project_name_stays_within_the_limit() {
        let (app, db) = app();
        let long = "a".repeat(ProjectDetails::MAX_NAME_CHARS);
        let mut archived = app.create_project(draft(&long)).unwrap();
        archived.state = ProjectState::Archived;
        db.save_project(&archived).unwrap();

        let channel = channel(&app, &long);
        let open = app.open_project().unwrap().unwrap();
        let expected = format!("{} 2", "a".repeat(ProjectDetails::MAX_NAME_CHARS - 2));
        assert_eq!(open.name(), expected);
        assert_eq!(channel.project, open.id);
    }

    #[test]
    fn with_a_project_open_the_scoped_screens_list_only_its_channels() {
        let (app, _) = app();
        let space = channel(&app, "Space Archives");
        let cooking = app.create_project(draft("Cooking")).unwrap();
        let kitchen = channel(&app, "Kitchen");
        assert_eq!(kitchen.project, cooking.id);
        assert_eq!(app.channels().unwrap(), std::slice::from_ref(&kitchen));

        app.switch_project(space.project).unwrap();
        assert_eq!(app.channels().unwrap(), std::slice::from_ref(&space));
    }

    #[test]
    fn the_open_project_favorites_and_recents_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let open = || Arc::new(Database::open(&path).unwrap());
        let (a, b, c) = {
            let app = start(&open());
            let a = app.create_project(draft("Alpha")).unwrap();
            let b = app.create_project(draft("Bravo")).unwrap();
            let c = app.create_project(draft("Charlie")).unwrap();
            app.star_project(c.id, true).unwrap();
            app.star_project(a.id, true).unwrap();
            app.move_favorite(a.id, 0).unwrap();
            app.switch_project(b.id).unwrap();
            (a, b, c)
        };

        let app = start(&open());
        assert_eq!(app.open_project().unwrap().unwrap().id, b.id);
        let list = app.project_list("").unwrap();
        let ids = |projects: &[Project]| projects.iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(&list.favorites), [a.id, c.id]);
        assert_eq!(ids(&list.recent), [b.id]);
        assert!(list.all.is_empty());

        app.star_project(a.id, false).unwrap();
        let list = app.project_list("").unwrap();
        assert_eq!(ids(&list.favorites), [c.id]);
        assert_eq!(ids(&list.recent), [b.id, a.id]);
    }

    #[test]
    fn upgrading_opens_the_first_project_by_name() {
        let (app, db) = app();
        let owner = app.profile().id;
        for name in ["Zeta", "Alpha"] {
            db.save_project(&Project::new(
                owner,
                ProjectDetails::validate(draft(name)).unwrap(),
                SystemTime::UNIX_EPOCH,
            ))
            .unwrap();
        }
        drop(app);
        let app = start(&db);
        assert_eq!(app.open_project().unwrap().unwrap().name(), "Alpha");
    }

    #[test]
    fn an_archived_or_unknown_project_cannot_be_opened() {
        let (app, db) = app();
        let a = app.create_project(draft("Alpha")).unwrap();
        let mut b = app.create_project(draft("Bravo")).unwrap();
        b.state = ProjectState::Archived;
        db.save_project(&b).unwrap();
        assert!(matches!(
            app.switch_project(b.id),
            Err(ProjectError::NotFound)
        ));
        assert!(matches!(
            app.switch_project(ProjectId::new()),
            Err(ProjectError::NotFound)
        ));
        assert_eq!(app.open_project().unwrap().unwrap().id, a.id);
    }

    #[test]
    fn new_projects_take_colors_no_other_has() {
        let (app, _) = app();
        let first = app.next_project_color();
        app.create_project(ProjectDraft {
            name: "Alpha".into(),
            color: first,
        })
        .unwrap();
        assert_ne!(app.next_project_color(), first);
    }

    #[test]
    fn a_channel_moves_with_its_themes_and_videos() {
        let (app, _) = app();
        let space = channel(&app, "Space Archives");
        let video = video(&app, space.id);
        let cooking = app.create_project(draft("Cooking")).unwrap();

        assert_eq!(app.move_channel_blocks(space.id).unwrap(), []);
        app.move_channel(space.id, cooking.id).unwrap();
        assert_eq!(app.channels().unwrap()[0].id, space.id);
        assert_eq!(app.video_projects(space.id).unwrap()[0].id, video.id);
        assert!(app.project_channels(space.project).unwrap().is_empty());
    }

    #[test]
    fn an_active_job_blocks_the_move_and_says_why() {
        let (app, _) = app();
        let space = channel(&app, "Space Archives");
        let cooking = app.create_project(draft("Cooking")).unwrap();
        // Held in the queue, so it stays queued.
        app.jobs.pause(true);
        let job = Job::new(
            app.profile().id,
            bardo_domain::JobKind::Countdown,
            format!("{{\"channel\":\"{}\"}}", space.id),
        );
        app.jobs.enqueue(job).unwrap();
        let error = app.move_channel(space.id, cooking.id).unwrap_err();
        assert!(matches!(
            &error,
            ProjectError::MoveBlocked(blocks) if blocks == &[MoveBlock::ActiveJob]
        ));
        assert_eq!(error.message(), Text::MoveChannelBlocked);
        assert_eq!(app.all_channels().unwrap()[0].project, space.project);
    }

    #[test]
    fn jobs_and_costs_stay_the_profiles_and_filter_by_project() {
        let (app, _) = app();
        let space = channel(&app, "Space Archives");
        let cooking = app.create_project(draft("Cooking")).unwrap();
        let kitchen = channel(&app, "Kitchen");
        // Held in the queue, so they stay queued.
        app.jobs.pause(true);
        for channel in [space.id, kitchen.id] {
            app.jobs
                .enqueue(Job::new(
                    app.profile().id,
                    bardo_domain::JobKind::Countdown,
                    format!("{{\"channel\":\"{channel}\"}}"),
                ))
                .unwrap();
            app.cost_book
                .costs
                .record_cost(&bardo_domain::CostRecord {
                    id: bardo_domain::CostRecordId::new(),
                    owner: app.profile().id,
                    provider: bardo_domain::Provider::Claude,
                    model: "claude-test".into(),
                    purpose: bardo_domain::CostPurpose::Script,
                    usage: bardo_domain::Metered::default(),
                    cost: bardo_domain::Cost::Reported(bardo_domain::Money::parse("1").unwrap()),
                    channel: Some(channel),
                    project: None,
                    job: None,
                    at: SystemTime::now(),
                    origin: Default::default(),
                })
                .unwrap();
        }

        assert_eq!(app.starting_filter(), ProjectFilter::Project(cooking.id));
        assert_eq!(app.job_groups().active(), 2);
        let kept = app.job_groups_for(ProjectFilter::Project(cooking.id));
        assert_eq!(kept.active(), 1);
        assert_eq!(
            kept.queued[0].origin().channel_name.as_deref(),
            Some("Kitchen")
        );
        assert_eq!(
            app.job_groups_for(ProjectFilter::Project(space.project))
                .queued[0]
                .origin()
                .project_name
                .as_deref(),
            Some("Space Archives")
        );
        assert!(app.job_groups_for(ProjectFilter::NoProject).is_empty());

        let month = app.current_month();
        let costs = app
            .costs_for(month, ProjectFilter::Project(cooking.id))
            .unwrap();
        assert_eq!(costs.projects.len(), 2);
        assert_eq!(costs.channels.len(), 1);
        assert_eq!(costs.channels[0].name.as_deref(), Some("Kitchen"));
        assert_eq!(costs.total, bardo_domain::Money::parse("2").unwrap());
        assert_eq!(app.costs(month).unwrap().channels.len(), 2);
    }

    #[test]
    fn filters_keep_their_projects_work() {
        let a = ProjectId::new();
        assert!(ProjectFilter::All.keeps(None));
        assert!(ProjectFilter::Project(a).keeps(Some(a)));
        assert!(!ProjectFilter::Project(a).keeps(Some(ProjectId::new())));
        assert!(!ProjectFilter::Project(a).keeps(None));
        assert!(ProjectFilter::NoProject.keeps(None));
        assert!(!ProjectFilter::NoProject.keeps(Some(a)));
    }
}
