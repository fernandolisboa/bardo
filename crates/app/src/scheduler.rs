//! The in-app scheduler (PRD story 87, ADR-0006). A Reel scheduled in the
//! upload review has a due time, when Bardo publishes it from the job
//! queue while the app is open: Instagram takes no publish time. Its job
//! waits until the upload may start (`bardo_domain::prepare_at`), sends and
//! processes the file, then waits for the due time, claims the publication
//! and publishes it (`UploadHandler`).
//!
//! A publication whose due time passed while Bardo was closed is never
//! sent silently. When Bardo starts, before the queue runs anything, each
//! one is marked missed and its job stopped (`mark_missed`); the app lists
//! them (`Bardo::missed_posts`) and the user sends each now, gives it a new
//! time or cancels it. One Bardo could not start in time while open (the PC
//! slept) is marked missed by its job the same way.
//!
//! The due time and the claim live in SQLite with the publication, and a
//! claim is taken in one statement by the runner that holds the upload
//! job's lease, so the background agent (`crate::agent`) sends the same
//! posts while Bardo is closed without publishing one twice. While the
//! agent runs, Bardo counts as open since the agent started.

use std::time::SystemTime;

use bardo_domain::{
    DueStep, JobRepository, Network, ProfileId, Publication, PublicationId, PublicationRepository,
    RepositoryError, ScheduleProblem, UploadStatus, VideoProjectId, check_publish_time, due_step,
    prepare_at,
};

use crate::jobs::JobActionError;
use crate::{Bardo, Text};

/// A scheduled post whose due time passed without it, as the app lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissedPost {
    pub publication: PublicationId,
    pub project: VideoProjectId,
    /// The video project's title.
    pub title: String,
    pub network: Network,
    /// The network account's handle.
    pub handle: String,
    /// When it was due.
    pub due: SystemTime,
}

/// Why a missed post was not sent, rescheduled or cancelled.
#[derive(Debug, thiserror::Error)]
pub enum MissedPostError {
    /// It is no longer missed (sent, rescheduled, cancelled or replaced
    /// meanwhile).
    #[error("the post is not missed any more")]
    NotMissed,
    /// The new time cannot be used (already past).
    #[error("the new time cannot be used: {0}")]
    Schedule(ScheduleProblem),
    #[error(transparent)]
    Job(#[from] JobActionError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl MissedPostError {
    /// What the app says.
    pub fn message(&self) -> Text {
        match self {
            MissedPostError::NotMissed => Text::MissedNotMissed,
            MissedPostError::Schedule(problem) => Text::ScheduleProblem(*problem),
            MissedPostError::Job(_) | MissedPostError::Repository(_) => Text::MissedNotUpdated,
        }
    }
}

/// Marks missed every scheduled publication of `owner` whose job was going
/// to publish it and whose due time passed while Bardo was closed, as of
/// `opened_at`, and stops that job: it waits for the user. Runs as Bardo
/// starts, before the queue does. Returns how many it marked.
pub(crate) fn mark_missed(
    publications: &dyn PublicationRepository,
    jobs: &dyn JobRepository,
    owner: ProfileId,
    opened_at: SystemTime,
) -> Result<usize, RepositoryError> {
    let mut stored = jobs.list(owner)?;
    // A job the other process (the background agent) runs is its own: it
    // publishes the post or marks it missed itself.
    let held = jobs.held_elsewhere(owner, opened_at)?;
    let mut marked = 0;
    for mut publication in publications.all_publications(owner)? {
        let Some(due) = publication.due() else {
            continue;
        };
        let Some(upload) = publication.upload_mut() else {
            continue;
        };
        if !matches!(
            upload.status,
            UploadStatus::Queued | UploadStatus::Uploading | UploadStatus::Processing
        ) {
            continue;
        }
        if held.contains(&upload.job) {
            continue;
        }
        let Some(job) = stored
            .iter_mut()
            .find(|job| job.id() == upload.job && job.state().is_active())
        else {
            // Stopped by the user: it stays stopped, and is missed once
            // resumed.
            continue;
        };
        // A run cut short after its claim may have published it: its job
        // asks the network first (`UploadHandler`).
        if upload.claimed_at.is_some()
            || due_step(due, upload.claimed_at, opened_at, opened_at) != DueStep::Missed
        {
            continue;
        }
        if upload.miss().is_err() {
            continue;
        }
        // The publication first: a job left queued would find it missed.
        if !publications.save_upload(&publication)? {
            continue;
        }
        job.cancel().expect("an active job can be cancelled");
        match jobs.save(job) {
            // Taken by the agent meanwhile: its run finds the post missed
            // and stops.
            Err(error) if error.is_held_elsewhere() => {}
            result => result?,
        }
        marked += 1;
    }
    if marked > 0 {
        tracing::info!(marked, "scheduled posts missed while Bardo was closed");
    }
    Ok(marked)
}

impl Bardo {
    /// Every scheduled post whose due time passed without it, soonest
    /// first, for the user to send now, reschedule or cancel.
    pub fn missed_posts(&self) -> Result<Vec<MissedPost>, RepositoryError> {
        let mut posts = Vec::new();
        for publication in self.publications.all_publications(self.profile.id)? {
            let Some(due) = publication.due().filter(|_| publication.is_missed()) else {
                continue;
            };
            let title = self
                .themes
                .project(publication.project)?
                .map(|project| project.title)
                .unwrap_or_default();
            let handle = self
                .network_accounts
                .get(publication.account)?
                .map(|account| account.details.handle().to_owned())
                .unwrap_or_default();
            posts.push(MissedPost {
                publication: publication.id,
                project: publication.project,
                title,
                network: publication.network(),
                handle,
                due,
            });
        }
        posts.sort_by_key(|post| post.due);
        Ok(posts)
    }

    fn missed(&self, id: PublicationId) -> Result<Publication, MissedPostError> {
        self.publications
            .publication(id)?
            .filter(|publication| publication.owner == self.profile.id)
            .filter(Publication::is_missed)
            .ok_or(MissedPostError::NotMissed)
    }

    /// Sends a missed post now: its job resumes from what the network
    /// already has.
    pub fn send_missed_now(&self, id: PublicationId) -> Result<(), MissedPostError> {
        let missed = self.missed(id)?;
        let mut publication = missed.clone();
        let upload = publication.upload_mut().ok_or(MissedPostError::NotMissed)?;
        upload.send_now().map_err(|_| MissedPostError::NotMissed)?;
        self.requeue_missed(&missed, publication, None)
    }

    /// Gives a missed post a new due time, which must be ahead.
    pub fn reschedule_missed(
        &self,
        id: PublicationId,
        due: SystemTime,
    ) -> Result<(), MissedPostError> {
        check_publish_time(due, SystemTime::now()).map_err(MissedPostError::Schedule)?;
        let due = crate::publications::whole_millis(due);
        let missed = self.missed(id)?;
        let mut publication = missed.clone();
        let upload = publication.upload_mut().ok_or(MissedPostError::NotMissed)?;
        upload
            .due_again(due)
            .map_err(|_| MissedPostError::NotMissed)?;
        let starts_at = Some(prepare_at(due)).filter(|at| *at > SystemTime::now());
        self.requeue_missed(&missed, publication, starts_at)
    }

    /// Saves the post queued again and queues its job for `starts_at`. A
    /// job that cannot be queued leaves the post `missed`, as it was.
    fn requeue_missed(
        &self,
        missed: &Publication,
        publication: Publication,
        starts_at: Option<SystemTime>,
    ) -> Result<(), MissedPostError> {
        let Some(job) = publication.upload().map(|upload| upload.job) else {
            return Err(MissedPostError::NotMissed);
        };
        self.publications.save_publication(&publication)?;
        if let Err(error) = self.jobs.retry_at(job, starts_at) {
            if let Err(error) = self.publications.save_publication(missed) {
                tracing::warn!("could not mark the post missed again: {error}");
            }
            return Err(error.into());
        }
        tracing::info!(
            network = publication.network().code(),
            "a missed post was queued again"
        );
        Ok(())
    }

    /// Cancels a missed post: nothing goes, and the project's publication
    /// on the network is gone, so it can be reviewed again.
    pub fn cancel_missed(&self, id: PublicationId) -> Result<(), MissedPostError> {
        let publication = self.missed(id)?;
        self.publications.remove_publication(publication.id)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use bardo_domain::{
        Job, JobFailureKind, JobId, JobState, UploadErrorKind, VideoState, Visibility,
    };

    use super::*;
    use crate::export::tests::Setup;
    use crate::publications::whole_millis;
    use crate::scenes::tests::{done, wait_done};
    use crate::uploads::reel_tests::{ready, review, state, upload};
    use crate::{UploadChoices, UploadState};

    const MINUTE: Duration = Duration::from_secs(60);
    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(Instant::now() < deadline, "never {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Reviews the Reel to go at `due`.
    fn schedule(s: &Setup, due: SystemTime) -> JobId {
        let review = review(s);
        s.app
            .start_upload(
                &review,
                UploadChoices {
                    publish_at: Some(due),
                    ..review.choices()
                },
            )
            .unwrap()
    }

    fn job(s: &Setup, id: JobId) -> Job {
        s.app.jobs().into_iter().find(|job| job.id() == id).unwrap()
    }

    fn nothing_sent(s: &Setup) -> bool {
        s.h.reels.videos.lock().unwrap().is_empty()
            && s.h.reels.published.lock().unwrap().is_empty()
    }

    /// A Reel due in two days whose due time passed while Bardo was closed.
    fn missed_while_closed() -> (Setup, JobId, SystemTime) {
        let s = ready();
        let job = schedule(&s, SystemTime::now() + 2 * DAY);
        let mut publication = upload(&s);
        let due = whole_millis(SystemTime::now() - 60 * MINUTE);
        publication.upload_mut().unwrap().publish_at = Some(due);
        assert!(s.app.publications.save_upload(&publication).unwrap());
        (s.restart(), job, due)
    }

    #[test]
    fn a_scheduled_reel_is_prepared_ahead_and_published_at_its_due_time() {
        let s = ready();
        let due = whole_millis(SystemTime::now() + Duration::from_millis(1500));
        let job = schedule(&s, due);
        wait_until("processed and waiting", || {
            matches!(state(&s), UploadState::Due(_)) && s.h.reels.files.lock().unwrap().len() == 1
        });
        assert_eq!(state(&s), UploadState::Due(due));
        assert!(s.h.reels.published.lock().unwrap().is_empty(), "not before");
        assert_eq!(upload(&s).upload().unwrap().claimed_at, None);

        done(&s.app, job);
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 1);
        assert_eq!(s.h.reels.files.lock().unwrap().len(), 1, "sent once");
        let claimed = upload(&s).upload().unwrap().claimed_at.unwrap();
        assert!(claimed >= due, "claimed from its due time");
        // Instagram takes no publish time: Bardo keeps it.
        assert_eq!(s.h.reels.videos.lock().unwrap()[0].publish_at, None);
    }

    #[test]
    fn a_reel_due_beyond_what_instagram_keeps_waits_to_start() {
        let s = ready();
        let due = whole_millis(SystemTime::now() + 2 * DAY);
        let id = schedule(&s, due);
        assert_eq!(state(&s), UploadState::Due(due));
        assert!(state(&s).is_active());
        let waiting = job(&s, id);
        assert_eq!(waiting.state(), JobState::Queued);
        assert_eq!(waiting.run_at(), Some(bardo_domain::prepare_at(due)));
        std::thread::sleep(Duration::from_millis(50));
        assert!(nothing_sent(&s));
        assert!(s.app.missed_posts().unwrap().is_empty());
    }

    #[test]
    fn a_due_time_that_passed_while_bardo_was_closed_is_listed_and_nothing_goes() {
        let (s, id, due) = missed_while_closed();
        let missed = s.app.missed_posts().unwrap();
        assert_eq!(missed.len(), 1);
        let post = &missed[0];
        assert_eq!(post.publication, upload(&s).id);
        assert_eq!(post.project, s.project.id);
        assert_eq!(post.title, s.project.title);
        assert_eq!(post.network, Network::InstagramReels);
        assert!(!post.handle.is_empty());
        assert_eq!(post.due, due);
        assert_eq!(state(&s), UploadState::Missed(due));
        assert!(!state(&s).is_active());
        assert_eq!(job(&s, id).state(), JobState::Cancelled);
        std::thread::sleep(Duration::from_millis(50));
        assert!(nothing_sent(&s));

        // Still listed the next time Bardo opens, until the user decides.
        let s = s.restart();
        assert_eq!(s.app.missed_posts().unwrap().len(), 1);
        assert!(nothing_sent(&s));
    }

    #[test]
    fn a_missed_post_sent_now_goes_once() {
        let (s, id, _) = missed_while_closed();
        let publication = upload(&s).id;
        s.app.send_missed_now(publication).unwrap();
        assert!(s.app.missed_posts().unwrap().is_empty());
        done(&s.app, id);
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 1);
        assert_eq!(upload(&s).upload().unwrap().publish_at, None);
        assert!(matches!(
            s.app.send_missed_now(publication),
            Err(MissedPostError::NotMissed)
        ));
    }

    #[test]
    fn a_missed_post_given_a_new_time_waits_for_it() {
        let (s, id, _) = missed_while_closed();
        let publication = upload(&s).id;
        let past = SystemTime::now() - MINUTE;
        assert!(matches!(
            s.app.reschedule_missed(publication, past),
            Err(MissedPostError::Schedule(ScheduleProblem::Past))
        ));
        assert_eq!(s.app.missed_posts().unwrap().len(), 1, "still missed");

        let due = whole_millis(SystemTime::now() + 2 * DAY);
        s.app.reschedule_missed(publication, due).unwrap();
        assert!(s.app.missed_posts().unwrap().is_empty());
        assert_eq!(state(&s), UploadState::Due(due));
        let waiting = job(&s, id);
        assert_eq!(waiting.state(), JobState::Queued);
        assert_eq!(waiting.run_at(), Some(prepare_at(due)));
        assert_eq!(upload(&s).upload().unwrap().claimed_at, None);
        std::thread::sleep(Duration::from_millis(50));
        assert!(nothing_sent(&s));
    }

    #[test]
    fn a_missed_post_cancelled_is_gone_and_can_be_reviewed_again() {
        let (s, id, _) = missed_while_closed();
        s.app.cancel_missed(upload(&s).id).unwrap();
        assert!(s.app.missed_posts().unwrap().is_empty());
        assert!(review(&s).replaces.is_none());
        assert_eq!(job(&s, id).state(), JobState::Cancelled);
        assert!(nothing_sent(&s));
    }

    #[test]
    fn a_reel_bardo_could_not_start_in_time_is_missed_though_open() {
        let s = ready();
        let id = schedule(&s, SystemTime::now() + 2 * DAY);
        // Its time came and went while the job could not run (the PC
        // slept): the run that finally starts sends nothing.
        let mut publication = upload(&s);
        let due = whole_millis(SystemTime::now() - 20 * MINUTE);
        publication.upload_mut().unwrap().publish_at = Some(due);
        assert!(s.app.publications.save_upload(&publication).unwrap());
        s.app.cancel_job(id).unwrap();
        s.app.resume_upload(id).unwrap();
        let ended = wait_done(&s.app, id);
        assert_eq!(ended.state(), JobState::Failed);
        assert_eq!(ended.failure().unwrap().kind, JobFailureKind::Missed);
        assert_eq!(state(&s), UploadState::Missed(due));
        assert_eq!(s.app.missed_posts().unwrap().len(), 1);
        assert!(nothing_sent(&s));
    }

    /// A Reel due now whose run claimed it and is publishing when Bardo
    /// closes; whether Instagram took the post (`published`) is what it
    /// answers next about the container.
    fn closed_while_publishing(published: bool) -> (Setup, JobId) {
        let s = ready();
        s.h.reels.stall_publish.store(true, Ordering::SeqCst);
        let id = schedule(&s, SystemTime::now() + Duration::from_millis(300));
        wait_until("publishing", || s.h.reels.stalled.load(Ordering::SeqCst));
        assert!(upload(&s).upload().unwrap().claimed_at.is_some());
        if published {
            s.h.reels.answer(Ok(VideoState::Ready {
                visibility: Visibility::Public,
                publish_at: None,
                published_at: None,
            }));
        }
        (s, id)
    }

    /// Bardo stays closed past the grace after the claim.
    fn opened_long_after_the_claim(s: &Setup) {
        let mut publication = upload(s);
        let upload_ = publication.upload_mut().unwrap();
        upload_.publish_at = Some(whole_millis(SystemTime::now() - 30 * MINUTE));
        upload_.claimed_at = Some(whole_millis(SystemTime::now() - 29 * MINUTE));
        s.app.publications.save_publication(&publication).unwrap();
    }

    #[test]
    fn a_reel_that_failed_at_its_due_time_goes_when_retried() {
        let s = ready();
        s.h.reels.publish_answer(Err(UploadErrorKind::SignedOut));
        let id = schedule(&s, SystemTime::now() + Duration::from_millis(300));
        let failed = wait_done(&s.app, id);
        assert_eq!(failed.state(), JobState::Failed);
        assert!(matches!(state(&s), UploadState::Failed { .. }));

        s.app.resume_upload(id).unwrap();
        done(&s.app, id);
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 2);
        assert_eq!(s.h.reels.files.lock().unwrap().len(), 1, "sent once");
    }

    #[test]
    fn a_due_reel_runs_exactly_once_when_bardo_restarts_during_it() {
        let (s, id) = closed_while_publishing(true);
        let s = s.restart();
        s.h.reels.stall_publish.store(false, Ordering::SeqCst);
        done(&s.app, id);
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(
            s.h.reels.published.lock().unwrap().len(),
            1,
            "published once"
        );
        assert_eq!(s.h.reels.files.lock().unwrap().len(), 1, "sent once");
        assert!(s.app.missed_posts().unwrap().is_empty());
    }

    #[test]
    fn a_run_cut_short_long_before_bardo_opens_again_records_what_instagram_published() {
        let (s, id) = closed_while_publishing(true);
        opened_long_after_the_claim(&s);
        let s = s.restart();
        s.h.reels.stall_publish.store(false, Ordering::SeqCst);
        done(&s.app, id);
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 1, "not twice");
        assert!(s.app.missed_posts().unwrap().is_empty());
    }

    #[test]
    fn a_run_cut_short_long_before_bardo_opens_again_waits_for_the_user() {
        let (s, id) = closed_while_publishing(false);
        opened_long_after_the_claim(&s);
        let s = s.restart();
        s.h.reels.stall_publish.store(false, Ordering::SeqCst);
        let ended = wait_done(&s.app, id);
        assert_eq!(ended.failure().unwrap().kind, JobFailureKind::Missed);
        assert_eq!(s.app.missed_posts().unwrap().len(), 1);
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 1, "no more");

        // Sent now, it goes once more.
        s.app.send_missed_now(upload(&s).id).unwrap();
        done(&s.app, id);
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 2);
        assert_eq!(s.h.reels.files.lock().unwrap().len(), 1, "sent once");
    }

    #[test]
    fn a_missed_post_whose_job_cannot_be_queued_stays_missed() {
        let (s, id, due) = missed_while_closed();
        // Its job cannot be queued again (it ended as done).
        let mut ended = job(&s, id);
        ended.retry().unwrap();
        ended.start(SystemTime::now()).unwrap();
        ended.complete().unwrap();
        bardo_domain::JobRepository::save(&*s.h.db, &ended).unwrap();
        let s = s.restart();
        let publication = upload(&s).id;
        assert!(s.app.send_missed_now(publication).is_err());
        assert_eq!(state(&s), UploadState::Missed(due));
        assert_eq!(s.app.missed_posts().unwrap().len(), 1);
    }
}
