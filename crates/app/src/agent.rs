//! The background publishing agent (ADR-0006, issue #87): `bardo --agent`,
//! started by Windows when the user signs in, sends the posts Bardo
//! publishes itself (Instagram Reels with a due time) while Bardo's window
//! is closed.
//!
//! It is Bardo without a window: the same database, secrets and upload job
//! (`UploadHandler`), with a job queue that takes only the uploads of
//! scheduled posts. Two processes never run one job, because each leases a
//! job before running it, and never publish one post, because only the
//! lease holder claims it (`bardo_domain::runner`). While the window is
//! open, the agent starts nothing new and lets the app run the jobs; it
//! finishes what it started.
//!
//! The user turns it on and off in Settings › Publishing: on registers the
//! task with the system and starts it; off removes the task, and the agent
//! sees the choice and stops.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use bardo_domain::{
    AgentTask, AgentTaskError, Job, PublicationRepository, RUNNER_ALIVE, RUNNER_TICK,
    RepositoryError, RunnerRole, RunnerSeen, UserProfile,
};

use crate::jobs::{JobSettings, Runner};
use crate::{AppError, Bardo, Providers, Repositories, Text};

/// How often the agent renews the network connections it posts with.
const RENEW_CONNECTIONS_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// The agent's job runner: it takes the uploads of scheduled posts still to
/// go, and starts paused while the app is up.
pub(crate) fn runner(publications: Arc<dyn PublicationRepository>, others: &[RunnerSeen]) -> Runner {
    Runner {
        role: RunnerRole::Agent,
        scope: Some(Arc::new(move |job: &Job| {
            scheduled_upload(&*publications, job)
        })),
        paused: app_is_up(others),
    }
}

/// Whether `job` is the upload of a post Bardo publishes at its due time,
/// still to run.
fn scheduled_upload(publications: &dyn PublicationRepository, job: &Job) -> bool {
    if !job.state().is_active() {
        return false;
    }
    let Some(id) = crate::uploads::job_publication(job) else {
        return false;
    };
    publications
        .publication(id)
        .ok()
        .flatten()
        .is_some_and(|publication| {
            publication.due().is_some()
                && publication
                    .upload()
                    .is_some_and(|upload| upload.job == job.id())
        })
}

fn app_is_up(others: &[RunnerSeen]) -> bool {
    others.iter().any(|other| other.role == RunnerRole::App)
}

/// Whether Bardo publishes scheduled posts while it is closed, as Settings
/// shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// Off: scheduled posts go only while Bardo is open.
    Off,
    /// On, and the agent is running.
    Running,
    /// On, but the agent is not running now: Windows starts it at the next
    /// sign-in, or the user starts it.
    NotRunning,
}

/// Why the agent's task did not change.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// The system refused to register or start it.
    #[error("the agent could not be set up: {0}")]
    NotSetUp(AgentTaskError),
    /// The system refused to remove it. The agent stops by itself, and
    /// Bardo removes the task the next time it opens.
    #[error("the agent's task could not be removed: {0}")]
    NotRemoved(AgentTaskError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl AgentError {
    /// What Settings says.
    pub fn message(&self) -> Text {
        match self {
            AgentError::NotSetUp(_) => Text::AgentNotSetUp,
            AgentError::NotRemoved(_) => Text::AgentNotRemoved,
            AgentError::Repository(_) => Text::AgentNotSaved,
        }
    }
}

/// A change to the agent's task. It starts a system tool, so it runs off
/// the UI thread.
#[must_use = "the task changes only when the work runs"]
pub struct AgentWork {
    task: Arc<dyn AgentTask>,
    step: AgentStep,
}

enum AgentStep {
    Nothing,
    /// Registers the task for `program`, and starts it.
    SetUp { program: PathBuf },
    Remove,
    /// Removes the task only if it is there (Bardo opening with the agent
    /// off).
    RemoveLeftover,
}

impl AgentWork {
    pub fn run(self) -> Result<(), AgentError> {
        let task = self.task;
        match self.step {
            AgentStep::Nothing => Ok(()),
            AgentStep::SetUp { program } => {
                task.register(&program)
                    .and_then(|()| task.start())
                    .map_err(AgentError::NotSetUp)?;
                tracing::info!("the background agent's task is set up");
                Ok(())
            }
            AgentStep::Remove => {
                task.remove().map_err(AgentError::NotRemoved)?;
                tracing::info!("the background agent's task is removed");
                Ok(())
            }
            AgentStep::RemoveLeftover => {
                if task.is_registered().map_err(AgentError::NotRemoved)? {
                    task.remove().map_err(AgentError::NotRemoved)?;
                    tracing::info!("a background agent's task left over was removed");
                }
                Ok(())
            }
        }
    }
}

impl Bardo {
    /// Opens Bardo as the background agent, or `None` when it has nothing
    /// to do: no profile yet, the agent is off, or another agent runs.
    pub fn start_agent(
        repositories: Repositories,
        providers: Providers,
    ) -> Result<Option<Self>, AppError> {
        Self::start_agent_with(repositories, providers, JobSettings::default())
    }

    /// `start_agent` with explicit job queue settings.
    pub fn start_agent_with(
        repositories: Repositories,
        providers: Providers,
        job_settings: JobSettings,
    ) -> Result<Option<Self>, AppError> {
        let Some(profile) = repositories.profiles.load_default()? else {
            return Ok(None);
        };
        if !profile.background_agent {
            tracing::info!("the background agent is off");
            return Ok(None);
        }
        let now = SystemTime::now();
        let others = repositories
            .jobs
            .runners(now.checked_sub(RUNNER_ALIVE).unwrap_or(now))?;
        if others.iter().any(|other| other.role == RunnerRole::Agent) {
            tracing::info!("another background agent is running");
            return Ok(None);
        }
        Self::open(
            repositories,
            providers,
            None,
            job_settings,
            RunnerRole::Agent,
        )
        .map(Some)
    }

    /// Whether this is the background agent rather than Bardo's window.
    pub fn is_agent(&self) -> bool {
        self.role == RunnerRole::Agent
    }

    /// Runs the agent until the user turns it off (or the process ends).
    pub fn run_agent(&self) {
        let mut renewed_at = None;
        while self.agent_step(&mut renewed_at) {
            std::thread::sleep(RUNNER_TICK);
        }
        tracing::info!("the background agent stops");
    }

    /// One round of the agent: false once the user turned it off. Takes no
    /// new job while Bardo's window is open, and renews the connections
    /// once a day while it is closed (the app renews them when it opens).
    pub(crate) fn agent_step(&self, renewed_at: &mut Option<Instant>) -> bool {
        match self.profiles.load_default() {
            Ok(Some(UserProfile {
                id,
                background_agent: true,
                ..
            })) if id == self.profile.id => {}
            Ok(_) => return false,
            Err(error) => {
                tracing::warn!("could not read whether the agent is on: {error}");
                return true;
            }
        }
        let app_open = app_is_up(&self.jobs.others());
        self.jobs.pause(app_open);
        if !app_open && renewed_at.is_none_or(|at| at.elapsed() >= RENEW_CONNECTIONS_EVERY) {
            self.connection_renewal().run();
            *renewed_at = Some(Instant::now());
        }
        true
    }

    /// Whether the background agent is on, and running.
    pub fn background_agent(&self) -> AgentStatus {
        if !self.profile.background_agent {
            return AgentStatus::Off;
        }
        if self
            .jobs
            .others()
            .iter()
            .any(|other| other.role == RunnerRole::Agent)
        {
            AgentStatus::Running
        } else {
            AgentStatus::NotRunning
        }
    }

    /// Turns the background agent on or off and remembers it; the returned
    /// work sets up or removes its task with the system. When setting it up
    /// fails, call `background_agent_not_set_up`.
    pub fn set_background_agent(&mut self, on: bool) -> Result<AgentWork, AgentError> {
        if on == self.profile.background_agent {
            return Ok(self.agent_work(AgentStep::Nothing));
        }
        // On is saved first, so the agent finds itself on when it starts;
        // off is saved first, so the agent stops even if the task stays.
        self.save_background_agent(on)?;
        let step = if on {
            AgentStep::SetUp {
                program: self.agent_program.clone(),
            }
        } else {
            AgentStep::Remove
        };
        Ok(self.agent_work(step))
    }

    /// Setting the agent up failed: it is off again.
    pub fn background_agent_not_set_up(&mut self) -> Result<(), AgentError> {
        if self.profile.background_agent {
            self.save_background_agent(false)?;
        }
        Ok(())
    }

    /// Sets the task up again and starts the agent, when it is on but not
    /// running.
    pub fn start_background_agent(&self) -> AgentWork {
        if !self.profile.background_agent {
            return self.agent_work(AgentStep::Nothing);
        }
        self.agent_work(AgentStep::SetUp {
            program: self.agent_program.clone(),
        })
    }

    /// Run when Bardo opens: the task matches the user's choice. When on
    /// and the agent is not running (Bardo moved, or the task went
    /// missing), it is set up again with this program and started; when
    /// off, a task left over is removed.
    pub fn agent_upkeep(&self) -> AgentWork {
        match self.background_agent() {
            AgentStatus::Off => self.agent_work(AgentStep::RemoveLeftover),
            AgentStatus::Running => self.agent_work(AgentStep::Nothing),
            AgentStatus::NotRunning => self.start_background_agent(),
        }
    }

    fn agent_work(&self, step: AgentStep) -> AgentWork {
        AgentWork {
            task: Arc::clone(&self.agent_task),
            step,
        }
    }

    fn save_background_agent(&mut self, on: bool) -> Result<(), RepositoryError> {
        let updated = UserProfile {
            background_agent: on,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant, SystemTime};

    use bardo_domain::{
        AgentTask, JobId, JobRepository, JobState, Network, ProfileRepository,
        PublicationRepository, UploadStatus,
    };

    use super::*;
    use crate::export::tests::Setup;
    use crate::publications::whole_millis;
    use crate::scenes::tests::Harness;
    use crate::uploads::reel_tests::{ready, review};
    use crate::UploadChoices;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !check() {
            assert!(Instant::now() < deadline, "never {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn turn_on(s: &mut Setup) {
        s.app.set_background_agent(true).unwrap().run().unwrap();
    }

    /// Reviews the project's Reel to go in two days.
    fn schedule(s: &Setup) -> JobId {
        let review = review(s);
        s.app
            .start_upload(
                &review,
                UploadChoices {
                    publish_at: Some(SystemTime::now() + 2 * DAY),
                    ..review.choices()
                },
            )
            .unwrap()
    }

    /// Moves the Reel's due time to `due` and lets its job start now, as
    /// if two days went by while Bardo was closed.
    fn due_soon(h: &Harness, job: JobId, due: SystemTime) {
        let mut reel = h
            .db
            .all_publications(profile(h).id)
            .unwrap()
            .into_iter()
            .find(|publication| publication.network() == Network::InstagramReels)
            .unwrap();
        reel.upload_mut().unwrap().publish_at = Some(whole_millis(due));
        assert!(h.db.save_upload(&reel).unwrap());
        let mut stored = JobRepository::job(&*h.db, job).unwrap().unwrap();
        stored.wait_until(SystemTime::now()).unwrap();
        JobRepository::save(&*h.db, &stored).unwrap();
    }

    fn profile(h: &Harness) -> UserProfile {
        h.db.load_default().unwrap().unwrap()
    }

    fn reel_status(h: &Harness) -> UploadStatus {
        h.db.all_publications(profile(h).id)
            .unwrap()
            .into_iter()
            .find(|publication| publication.network() == Network::InstagramReels)
            .and_then(|publication| publication.upload().map(|upload| upload.status.clone()))
            .unwrap()
    }

    fn assert_posted_once(h: &Harness) {
        assert_eq!(h.reels.published.lock().unwrap().len(), 1, "published once");
        assert_eq!(h.reels.files.lock().unwrap().len(), 1, "sent once");
    }

    #[test]
    fn the_agent_runs_only_when_turned_on_and_alone() {
        let mut s = ready();
        assert!(s.h.start_agent().is_none(), "off");
        turn_on(&mut s);
        let agent = s.h.start_agent().expect("on");
        assert!(agent.is_agent());
        assert!(!s.app.is_agent());
        // The app hears of it at its next tick.
        wait_until("seen running", || {
            s.app.background_agent() == AgentStatus::Running
        });
        assert!(s.h.start_agent().is_none(), "one agent at a time");
    }

    #[test]
    fn a_scheduled_reel_goes_out_from_the_agent_while_bardo_is_closed() {
        let mut s = ready();
        turn_on(&mut s);
        let job = schedule(&s);
        let (h, _) = s.close();
        due_soon(&h, job, SystemTime::now() + Duration::from_millis(600));

        let agent = h.start_agent().unwrap();
        wait_until("published", || reel_status(&h) == UploadStatus::Published);
        assert_posted_once(&h);
        let ran = JobRepository::job(&*h.db, job).unwrap().unwrap();
        assert_eq!(ran.state(), JobState::Done);
        drop(agent);

        // Bardo opens later: nothing is missed, and nothing goes again.
        let app = h.start();
        assert!(app.missed_posts().unwrap().is_empty());
        std::thread::sleep(Duration::from_millis(100));
        assert_posted_once(&h);
    }

    #[test]
    fn the_app_and_the_agent_racing_for_a_due_reel_post_it_once() {
        let mut s = ready();
        turn_on(&mut s);
        let job = schedule(&s);
        let (h, _) = s.close();
        due_soon(&h, job, SystemTime::now() + Duration::from_millis(800));

        // Both open at once: the agent before it hears of the app, the app
        // while the agent runs. Each one's queue finds the job due.
        let agent = h.start_agent().unwrap();
        let app = h.start();
        wait_until("published", || reel_status(&h) == UploadStatus::Published);
        std::thread::sleep(Duration::from_millis(200));
        assert_posted_once(&h);
        for runner in [&app, &agent] {
            wait_until("done for both", || {
                runner
                    .jobs()
                    .iter()
                    .all(|ran| ran.id() != job || ran.state() == JobState::Done)
            });
        }
    }

    #[test]
    fn the_agent_takes_no_new_job_while_bardo_is_open() {
        let mut s = ready();
        turn_on(&mut s);
        let agent = s.h.start_agent().unwrap();
        assert!(agent.jobs.is_paused(), "Bardo is open");
        let mut renewed = None;
        assert!(agent.agent_step(&mut renewed));
        assert!(agent.jobs.is_paused());
        assert!(renewed.is_none(), "Bardo renews its connections itself");

        let (h, _) = s.close();
        wait_until("Bardo closed", || {
            agent.agent_step(&mut renewed) && !agent.jobs.is_paused()
        });
        assert!(renewed.is_some());
        drop(h);
    }

    #[test]
    fn turning_the_agent_off_stops_it_and_removes_its_task_and_on_restores_it() {
        let mut s = ready();
        assert_eq!(s.app.background_agent(), AgentStatus::Off);
        turn_on(&mut s);
        assert!(profile(&s.h).background_agent);
        assert_eq!(
            s.h.agent_task.program(),
            Some(std::env::current_exe().unwrap())
        );
        assert_eq!(s.h.agent_task.starts(), 1, "started without a sign-in");
        assert_eq!(s.app.background_agent(), AgentStatus::NotRunning);

        let agent = s.h.start_agent().unwrap();
        let mut renewed = None;
        assert!(agent.agent_step(&mut renewed));
        s.app.set_background_agent(false).unwrap().run().unwrap();
        assert!(!s.h.agent_task.is_registered().unwrap());
        assert!(!agent.agent_step(&mut renewed), "the agent stops");
        assert_eq!(s.app.background_agent(), AgentStatus::Off);
        drop(agent);

        turn_on(&mut s);
        assert!(s.h.agent_task.is_registered().unwrap(), "restored");
        assert_eq!(s.h.agent_task.starts(), 2);
    }

    #[test]
    fn an_agent_windows_refuses_stays_off() {
        let mut s = ready();
        s.h.agent_task.refuse(true);
        let work = s.app.set_background_agent(true).unwrap();
        let error = work.run().unwrap_err();
        assert!(matches!(error, AgentError::NotSetUp(_)));
        assert_eq!(error.message(), Text::AgentNotSetUp);
        s.app.background_agent_not_set_up().unwrap();
        assert_eq!(s.app.background_agent(), AgentStatus::Off);
        assert!(!profile(&s.h).background_agent);
    }

    #[test]
    fn opening_bardo_sets_the_task_up_again_or_removes_a_leftover() {
        let mut s = ready();
        turn_on(&mut s);
        // The task went missing (removed by hand, Bardo moved).
        s.h.agent_task.remove().unwrap();
        s.app.agent_upkeep().run().unwrap();
        assert!(s.h.agent_task.is_registered().unwrap());
        assert_eq!(s.h.agent_task.starts(), 2);

        // Running: left alone.
        let agent = s.h.start_agent().unwrap();
        wait_until("seen running", || {
            s.app.background_agent() == AgentStatus::Running
        });
        s.app.agent_upkeep().run().unwrap();
        assert_eq!(s.h.agent_task.starts(), 2);
        drop(agent);

        // Off, with the task still there (removing it failed before).
        s.h.agent_task.refuse(true);
        assert!(s.app.set_background_agent(false).unwrap().run().is_err());
        s.h.agent_task.refuse(false);
        assert!(s.h.agent_task.is_registered().unwrap());
        s.app.agent_upkeep().run().unwrap();
        assert!(!s.h.agent_task.is_registered().unwrap());
    }
}
