//! Scheduled uploads (#78; PRD story 86, ADR-0006): the publish time a
//! review takes, typed and shown in the user's time zone, and changing or
//! cancelling the schedule of a video the network holds private until then.
//!
//! The network keeps the schedule, so Bardo and the PC can be off when the
//! video goes live. Bardo keeps the time it set and reads it back: the
//! metrics sync (`publications`) tells when the video went public, or that
//! the network kept it private past its time.
//!
//! A change goes to the network only while the video is still private and
//! scheduled. It runs off the UI thread (`ScheduleUpdate::run`), like a
//! connection check: a short call, not a queued job.

use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    NetworkAccount, NetworkAccountRepository, PublicationId, PublicationRepository,
    RepositoryError, ScheduleChange, ScheduleOutcome, ScheduleProblem, ScheduleReading,
    UploadErrorKind, VideoState, VideoUploader, Visibility, check_publish_time,
    default_publish_time, local_time, publish_time,
};

use crate::connections::Connections;
use crate::uploads::access_token;
use crate::{Bardo, Text};

/// Why a schedule change did not happen.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    /// The publication is not a scheduled upload any more (it went live,
    /// was replaced or removed).
    #[error("the upload is not scheduled")]
    NotScheduled,
    #[error("the publish time cannot be used: {0}")]
    Problem(ScheduleProblem),
    /// The account's sign-in no longer works.
    #[error("the account needs to reconnect")]
    ReconnectNeeded,
    /// The network no longer has the video.
    #[error("the video is gone")]
    Removed,
    /// The network takes no publish time for the video any more: it was
    /// public once.
    #[error("the network takes no publish time for the video")]
    NotAllowed,
    /// The network did not take the change: offline, refused or down.
    #[error("the network did not take the change: {0}")]
    Failed(String),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ScheduleError {
    /// What the Publish stage says; `{network}` is the network's name.
    pub fn message(&self) -> Text {
        match self {
            ScheduleError::NotScheduled => Text::ScheduleNotScheduled,
            ScheduleError::Problem(problem) => Text::ScheduleProblem(*problem),
            ScheduleError::ReconnectNeeded => Text::UploadFailureReconnect,
            ScheduleError::Removed => Text::UploadFailureRemoved,
            ScheduleError::NotAllowed => Text::ScheduleNotAllowed,
            ScheduleError::Failed(_) | ScheduleError::Repository(_) => Text::ScheduleFailed,
        }
    }
}

/// How a schedule change ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleResult {
    /// The video goes live at the new time.
    Rescheduled(SystemTime),
    /// The video stays private, with no publish time.
    Cancelled,
    /// Too late: the network had already published it. The publication
    /// now says so.
    AlreadyLive,
}

/// A schedule change ready to run on a background thread.
pub struct ScheduleUpdate {
    publication: PublicationId,
    account: NetworkAccount,
    video: String,
    change: ScheduleChange,
    connections: Connections,
    uploader: Arc<dyn VideoUploader>,
    publications: Arc<dyn PublicationRepository>,
}

impl ScheduleUpdate {
    /// Sends the change and records it. Blocks on the network.
    pub fn run(self) -> Result<ScheduleResult, ScheduleError> {
        let token = access_token(&self.connections, &self.account).map_err(failed)?;
        let outcome = self.uploader.reschedule(&token, &self.video, &self.change);
        let Some(mut publication) = self
            .publications
            .publication(self.publication)?
            .filter(|p| p.is_scheduled() && p.post_id() == Some(self.video.as_str()))
        else {
            // Replaced while the change ran: nothing to record on it.
            return match outcome {
                Ok(_) => Err(ScheduleError::NotScheduled),
                Err(error) => Err(failed(error)),
            };
        };
        let from = publication
            .upload()
            .and_then(|upload| upload.publish_at)
            .ok_or(ScheduleError::NotScheduled)?;
        let now = SystemTime::now();
        let result = match outcome {
            Ok(ScheduleOutcome::Changed) => match self.change.publish_at {
                Some(at) => {
                    publication.rescheduled(at).map_err(not_scheduled)?;
                    ScheduleResult::Rescheduled(at)
                }
                None => {
                    publication.unscheduled().map_err(not_scheduled)?;
                    ScheduleResult::Cancelled
                }
            },
            Ok(ScheduleOutcome::Live { published_at, .. }) => {
                publication.schedule_seen(ScheduleReading::Live { published_at }, now);
                ScheduleResult::AlreadyLive
            }
            Err(error) if error.kind == UploadErrorKind::NotFound => {
                publication.schedule_seen(ScheduleReading::Missing, now);
                self.publications
                    .save_sync(std::slice::from_ref(&publication), &[])?;
                return Err(ScheduleError::Removed);
            }
            Err(error) => return Err(failed(error)),
        };
        // A sync that recorded something else meanwhile (the video went
        // live, or was kept private) has the later word.
        if !self.publications.save_schedule(&publication, from)? {
            return Err(ScheduleError::NotScheduled);
        }
        if result == ScheduleResult::AlreadyLive {
            self.publications
                .save_sync(std::slice::from_ref(&publication), &[])?;
        }
        tracing::info!(network = self.account.network.code(), "changed a schedule");
        Ok(result)
    }
}

fn not_scheduled(_: bardo_domain::InvalidUploadTransition) -> ScheduleError {
    ScheduleError::NotScheduled
}

fn failed(error: bardo_domain::UploadError) -> ScheduleError {
    match error.kind {
        UploadErrorKind::SignedOut | UploadErrorKind::Refused => ScheduleError::ReconnectNeeded,
        UploadErrorKind::Late => ScheduleError::Problem(ScheduleProblem::Past),
        UploadErrorKind::NotFound => ScheduleError::Removed,
        UploadErrorKind::ScheduleRefused => ScheduleError::NotAllowed,
        _ => ScheduleError::Failed(error.detail),
    }
}

/// What a read of a scheduled video says about its schedule; `None` while
/// the network says nothing useful (still processing, or failed).
fn schedule_reading(state: VideoState) -> Option<ScheduleReading> {
    match state {
        VideoState::Ready {
            visibility: Visibility::Private,
            publish_at,
            ..
        } => Some(ScheduleReading::Private { publish_at }),
        VideoState::Ready { published_at, .. } => Some(ScheduleReading::Live { published_at }),
        VideoState::Removed => Some(ScheduleReading::Missing),
        VideoState::Processing
        | VideoState::Processed
        | VideoState::Expired
        | VideoState::Failed(_)
        | VideoState::Rejected(_) => None,
    }
}

/// Reads where a scheduled video stands, through the owner's connection.
pub(crate) fn read_schedule(
    connections: &Connections,
    accounts: &dyn NetworkAccountRepository,
    uploaders: &[Arc<dyn VideoUploader>],
    publication: &bardo_domain::Publication,
) -> Result<Option<ScheduleReading>, String> {
    let video = publication.post_id().ok_or("no video yet")?;
    let uploader = uploaders
        .iter()
        .find(|uploader| uploader.network() == publication.network())
        .ok_or("no uploader for the network")?;
    let account = accounts
        .get(publication.account)
        .map_err(|error| error.to_string())?
        .ok_or("the account was removed")?;
    let token = access_token(connections, &account).map_err(|error| error.detail)?;
    let state = uploader
        .state(&token, video)
        .map_err(|error| error.detail)?;
    Ok(schedule_reading(state))
}

impl Bardo {
    /// The time zone publish times are typed and shown in.
    pub fn time_zone(&self) -> &bardo_domain::Zone {
        &self.zone
    }

    #[cfg(test)]
    pub(crate) fn set_time_zone(&mut self, zone: bardo_domain::Zone) {
        self.zone = zone;
    }

    /// The instant the user means by `date` and `time`, typed in the
    /// interface language's order and the system's time zone. Whether it
    /// is still ahead is checked when they confirm.
    pub fn publish_time(&self, date: &str, time: &str) -> Result<SystemTime, ScheduleProblem> {
        publish_time(date, time, self.catalog.date_order(), &self.zone)
    }

    /// `at` as the date and time fields show it.
    pub fn publish_time_fields(&self, at: SystemTime) -> (String, String) {
        let local = local_time(at, &self.zone);
        (
            bardo_domain::date_text(&local, self.catalog.date_order()),
            self.catalog.time(&local),
        )
    }

    /// The publish time a review starts with.
    pub fn default_publish_time(&self) -> SystemTime {
        default_publish_time(SystemTime::now(), &self.zone)
    }

    /// `at` in words with its zone, e.g. `Sun, October 4, 2026, 6:00 PM
    /// (America/Sao_Paulo, UTC−03:00)`.
    pub fn publish_time_text(&self, at: SystemTime) -> String {
        self.catalog.date_time(&local_time(at, &self.zone))
    }

    /// The time zone in words, e.g. `America/Sao_Paulo, UTC−03:00`.
    pub fn time_zone_text(&self) -> String {
        self.catalog
            .zone(&local_time(SystemTime::now(), &self.zone))
    }

    /// Prepares changing the publish time of a scheduled upload, or
    /// cancelling it (`publish_at` is `None`). Refuses a publication that is
    /// not scheduled, one whose job still runs, and a time already past.
    pub fn schedule_change(
        &self,
        publication: PublicationId,
        publish_at: Option<SystemTime>,
    ) -> Result<ScheduleUpdate, ScheduleError> {
        let publication = self
            .publications
            .publication(publication)?
            .filter(|p| p.owner == self.profile.id && p.is_scheduled())
            .ok_or(ScheduleError::NotScheduled)?;
        let upload = publication.upload().ok_or(ScheduleError::NotScheduled)?;
        if self.job_active(upload.job) {
            return Err(ScheduleError::NotScheduled);
        }
        let video = publication
            .post_id()
            .ok_or(ScheduleError::NotScheduled)?
            .to_owned();
        if let Some(at) = publish_at {
            check_publish_time(at, SystemTime::now()).map_err(ScheduleError::Problem)?;
        }
        let uploader = self
            .uploaders
            .iter()
            .find(|uploader| uploader.network() == publication.network())
            .cloned()
            .ok_or(ScheduleError::NotScheduled)?;
        let account = self
            .network_accounts
            .get(publication.account)?
            .ok_or(ScheduleError::ReconnectNeeded)?;
        let (made_for_kids, synthetic) = self.upload_declarations(upload.job);
        Ok(ScheduleUpdate {
            publication: publication.id,
            account,
            video,
            change: ScheduleChange {
                publish_at: publish_at.map(crate::publications::whole_millis),
                made_for_kids,
                synthetic,
            },
            connections: self.connection_book.connections().clone(),
            uploader,
            publications: Arc::clone(&self.publications),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bardo_domain::{
        JobKind, Network, Provider, Publication, UploadStatus, VideoMetadataDraft, Zone,
        parse_rfc3339,
    };

    use super::*;
    use crate::UploadChoices;
    use crate::UploadReviewError;
    use crate::UploadState;
    use crate::export::tests::Setup;
    use crate::scenes::tests::done;
    use crate::uploads::tests::{ready, review, start, upload};

    /// Tomorrow at a whole minute, as a review takes it.
    fn tomorrow() -> SystemTime {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        SystemTime::UNIX_EPOCH + Duration::from_secs(now - now % 60 + 24 * 3600)
    }

    fn schedule_choices(s: &Setup, at: SystemTime) -> UploadChoices {
        UploadChoices {
            visibility: Visibility::Unlisted,
            made_for_kids: true,
            synthetic: true,
            publish_at: Some(at),
            ..review(s).choices()
        }
    }

    /// The project's YouTube upload, scheduled for `at` and processed.
    fn scheduled(at: SystemTime) -> Setup {
        let s = ready();
        let choices = schedule_choices(&s, at);
        done(&s.app, start(&s, choices));
        s
    }

    fn sync(s: &Setup) {
        done(&s.app, s.app.sync_metrics().unwrap());
    }

    #[test]
    fn a_scheduled_upload_goes_up_private_with_its_time_and_waits_for_it() {
        let at = tomorrow();
        let s = scheduled(at);

        let sent = s.h.uploader.videos.lock().unwrap().clone();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].publish_at, Some(at));
        assert_eq!(
            sent[0].visibility,
            Visibility::Public,
            "public at its time, whatever visibility was picked"
        );
        assert!(sent[0].made_for_kids && sent[0].synthetic);

        let publication = upload(&s);
        let up = publication.upload().unwrap();
        assert_eq!(up.status, UploadStatus::Scheduled);
        assert_eq!(up.publish_at, Some(at));
        assert_eq!(publication.posted_at, at, "goes live at its time");
        assert_eq!(
            s.app.upload_state(&publication),
            Some(UploadState::Scheduled(at))
        );
        assert!(!publication.is_posted(), "not live yet");
    }

    #[test]
    fn a_publish_time_already_past_is_refused_when_confirmed() {
        let s = ready();
        let past = SystemTime::now() - Duration::from_secs(60);
        let review = review(&s);
        assert!(matches!(
            s.app.start_upload(&review, schedule_choices(&s, past)),
            Err(UploadReviewError::Schedule(ScheduleProblem::Past))
        ));
        assert!(
            s.app.jobs().iter().all(|job| job.kind() != JobKind::Upload),
            "nothing queued"
        );
        assert!(
            s.app
                .publications
                .publications(s.project.id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_change_after_the_review_needs_a_new_review_when_scheduled_too() {
        let s = ready();
        let seen = review(&s);
        s.app
            .edit_metadata(
                s.project.id,
                Network::YouTube,
                VideoMetadataDraft {
                    title: "A better title".into(),
                    description: "In 1969 a probe went silent.".into(),
                    tags: vec!["nasa".into()],
                },
            )
            .unwrap();
        assert!(matches!(
            s.app.start_upload(&seen, schedule_choices(&s, tomorrow())),
            Err(UploadReviewError::Changed)
        ));
    }

    #[test]
    fn a_scheduled_publication_becomes_published_after_a_sync_shows_it_public() {
        let s = scheduled(tomorrow());
        let went_live = parse_rfc3339("2026-10-04T22:00:03Z").unwrap();
        s.h.uploader.answer(Ok(VideoState::Ready {
            visibility: Visibility::Public,
            publish_at: None,
            published_at: Some(went_live),
        }));
        let status = s.app.metrics_sync_status().unwrap();
        assert!(status.can_sync(), "no key needed for schedules alone");

        sync(&s);

        let publication = upload(&s);
        assert_eq!(
            publication.upload().unwrap().status,
            UploadStatus::Published
        );
        assert_eq!(publication.posted_at, went_live, "YouTube's publish time");
        assert!(publication.checked_at.is_some());
        assert!(publication.has_public_metrics(), "tracked from now on");
        assert_eq!(
            s.app.upload_state(&publication),
            Some(UploadState::Published)
        );
        let video = publication.post_id().unwrap().to_owned();
        assert_eq!(s.h.uploader.checks.lock().unwrap().last(), Some(&video));
        assert!(s.h.stats.calls().is_empty(), "its numbers wait for a key");
    }

    #[test]
    fn without_a_key_a_sync_still_reads_the_schedules_next_to_public_posts() {
        let s = scheduled(tomorrow());
        // A post linked by hand on another project: its numbers need a key.
        let (second, _) = s.h.drawn_project(&s.app);
        let manual = Publication {
            id: bardo_domain::PublicationId::new(),
            project: second.id,
            kind: bardo_domain::PublicationKind::Manual,
            link: Some(
                bardo_domain::PostLink::parse(Network::YouTube, "https://youtu.be/dQw4w9WgXcQ")
                    .unwrap(),
            ),
            ..upload(&s)
        };
        s.app.publications.save_publication(&manual).unwrap();
        let status = s.app.metrics_sync_status().unwrap();
        assert!(status.can_sync());
        s.h.uploader.answer(Ok(VideoState::Ready {
            visibility: Visibility::Public,
            publish_at: None,
            published_at: Some(tomorrow()),
        }));

        sync(&s);

        assert_eq!(upload(&s).upload().unwrap().status, UploadStatus::Published);
        assert!(s.h.stats.calls().is_empty(), "no key: no statistics read");
        let manual = s.app.publications.publication(manual.id).unwrap().unwrap();
        assert_eq!(manual.checked_at, None, "left for a sync with a key");
    }

    #[test]
    fn a_post_that_went_live_gets_its_numbers_in_the_same_sync_with_a_key() {
        let mut s = scheduled(tomorrow());
        s.app
            .save_provider_key(Provider::YouTubeData, "AIzaSyTestKey0001abcdefghij")
            .unwrap();
        let video = upload(&s).post_id().unwrap().to_owned();
        s.h.uploader.answer(Ok(VideoState::Ready {
            visibility: Visibility::Public,
            publish_at: None,
            published_at: None,
        }));
        s.h.stats.set(&video, 42, Some(3));

        sync(&s);

        assert_eq!(s.h.stats.calls(), [vec![video]]);
        let snapshots = s.app.publications.snapshots(upload(&s).id).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].views, 42);
    }

    #[test]
    fn a_sync_reads_back_the_time_and_spots_a_video_kept_private() {
        let s = scheduled(tomorrow());
        let later = tomorrow() + Duration::from_secs(3600);
        s.h.uploader.answer(Ok(VideoState::Ready {
            visibility: Visibility::Private,
            publish_at: Some(later),
            published_at: None,
        }));
        sync(&s);
        let publication = upload(&s);
        assert_eq!(publication.upload().unwrap().publish_at, Some(later));
        assert_eq!(publication.posted_at, later, "changed on YouTube");

        let past = SystemTime::now() - Duration::from_secs(3600);
        s.h.uploader.answer(Ok(VideoState::Ready {
            visibility: Visibility::Private,
            publish_at: Some(past),
            published_at: None,
        }));
        sync(&s);
        let publication = upload(&s);
        assert_eq!(
            publication.upload().unwrap().status,
            UploadStatus::Restricted,
            "still private an hour after its time"
        );
        assert_eq!(
            s.app.upload_state(&publication),
            Some(UploadState::Restricted)
        );
    }

    #[test]
    fn a_failed_read_leaves_the_schedule_for_the_next_sync() {
        let s = scheduled(tomorrow());
        s.h.uploader.answer(Err(UploadErrorKind::NetworkDown));
        sync(&s);
        assert!(upload(&s).is_scheduled());
    }

    fn change(s: &Setup, at: Option<SystemTime>) -> Result<ScheduleResult, ScheduleError> {
        s.app.schedule_change(upload(s).id, at)?.run()
    }

    #[test]
    fn changing_the_time_sends_it_with_what_the_upload_declared() {
        let s = scheduled(tomorrow());
        let later = tomorrow() + Duration::from_secs(2 * 3600);
        assert_eq!(
            change(&s, Some(later)).unwrap(),
            ScheduleResult::Rescheduled(later)
        );

        let publication = upload(&s);
        let changes = s.h.uploader.changes.lock().unwrap().clone();
        assert_eq!(
            changes,
            [(
                publication.post_id().unwrap().to_owned(),
                ScheduleChange {
                    publish_at: Some(later),
                    made_for_kids: true,
                    synthetic: true,
                }
            )]
        );
        assert!(publication.is_scheduled());
        assert_eq!(publication.upload().unwrap().publish_at, Some(later));
        assert_eq!(publication.posted_at, later);
    }

    #[test]
    fn cancelling_leaves_a_private_unscheduled_video() {
        let s = scheduled(tomorrow());
        assert_eq!(change(&s, None).unwrap(), ScheduleResult::Cancelled);
        assert_eq!(s.h.uploader.changes.lock().unwrap()[0].1.publish_at, None);
        let publication = upload(&s);
        let up = publication.upload().unwrap();
        assert_eq!(
            (&up.status, up.visibility, up.publish_at),
            (&UploadStatus::Published, Visibility::Private, None)
        );
        assert!(matches!(
            change(&s, Some(tomorrow())),
            Err(ScheduleError::NotScheduled)
        ));
    }

    #[test]
    fn a_change_too_late_records_that_the_video_went_live() {
        let s = scheduled(tomorrow());
        let went_live = parse_rfc3339("2026-10-04T22:00:03Z").unwrap();
        s.h.uploader
            .reschedules
            .lock()
            .unwrap()
            .push_back(Ok(ScheduleOutcome::Live {
                visibility: Visibility::Public,
                published_at: Some(went_live),
            }));
        assert_eq!(change(&s, None).unwrap(), ScheduleResult::AlreadyLive);
        let publication = upload(&s);
        assert_eq!(
            publication.upload().unwrap().status,
            UploadStatus::Published
        );
        assert_eq!(publication.posted_at, went_live);
    }

    #[test]
    fn changes_that_cannot_go_are_refused_with_why() {
        let s = scheduled(tomorrow());
        let past = SystemTime::now() - Duration::from_secs(60);
        assert!(matches!(
            change(&s, Some(past)),
            Err(ScheduleError::Problem(ScheduleProblem::Past))
        ));
        assert!(
            s.h.uploader.changes.lock().unwrap().is_empty(),
            "nothing sent"
        );

        let push = |kind| {
            s.h.uploader
                .reschedules
                .lock()
                .unwrap()
                .push_back(Err(kind))
        };
        push(UploadErrorKind::NetworkDown);
        let error = change(&s, None).unwrap_err();
        assert!(matches!(error, ScheduleError::Failed(_)));
        assert_eq!(error.message(), Text::ScheduleFailed);
        assert!(upload(&s).is_scheduled(), "unchanged");

        push(UploadErrorKind::Refused);
        assert!(matches!(
            change(&s, None),
            Err(ScheduleError::ReconnectNeeded)
        ));

        push(UploadErrorKind::ScheduleRefused);
        let error = change(&s, Some(tomorrow())).unwrap_err();
        assert!(matches!(error, ScheduleError::NotAllowed));
        assert_eq!(error.message(), Text::ScheduleNotAllowed);

        push(UploadErrorKind::NotFound);
        assert!(matches!(change(&s, None), Err(ScheduleError::Removed)));
        assert!(upload(&s).missing_since.is_some(), "flagged as gone");
    }

    #[test]
    fn only_a_scheduled_upload_has_a_schedule_to_change() {
        let s = ready();
        done(&s.app, start(&s, review(&s).choices()));
        let immediate: Publication = upload(&s);
        assert!(matches!(
            s.app.schedule_change(immediate.id, None),
            Err(ScheduleError::NotScheduled)
        ));
    }

    #[test]
    fn publish_times_are_typed_and_shown_in_the_system_zone() {
        let mut s = ready();
        s.app.set_time_zone(Zone::new(
            jiff::tz::TimeZone::get("America/Sao_Paulo").unwrap(),
        ));
        let at = s.app.publish_time("10/04/2026", "6:30 PM").unwrap();
        assert_eq!(at, parse_rfc3339("2026-10-04T21:30:00Z").unwrap());
        assert_eq!(
            s.app.publish_time_text(at),
            "Sun, October 4, 2026, 6:30 PM (America/Sao_Paulo, UTC−03:00)"
        );
        assert_eq!(
            s.app.publish_time_fields(at),
            ("10/04/2026".to_owned(), "6:30 PM".to_owned())
        );
        assert_eq!(
            s.app.publish_time("31/12/2026", "18:30"),
            Err(ScheduleProblem::Date),
            "en-US types the month first"
        );
        assert_eq!(s.app.time_zone_text(), "America/Sao_Paulo, UTC−03:00");

        s.app
            .set_ui_language(bardo_domain::UiLanguage::PtBr)
            .unwrap();
        assert_eq!(
            s.app.publish_time("04/10/2026", "18:30"),
            Ok(at),
            "pt-BR types the day first"
        );
        assert_eq!(
            s.app.publish_time_text(at),
            "dom., 4 de outubro de 2026, 18:30 (America/Sao_Paulo, UTC−03:00)"
        );
        assert!(s.app.default_publish_time() > SystemTime::now());
    }
}
