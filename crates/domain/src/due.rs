//! The in-app scheduler's rules (ADR-0006, PRD story 87): when Bardo
//! publishes a scheduled post on a network that takes no publish time
//! (Instagram), from its job queue, while the app is open.
//!
//! - A scheduled publication has a **due time**, when Bardo publishes it.
//!   Its upload may run ahead, but only so far ahead that what the network
//!   keeps of an unpublished upload (Instagram's container, 24 hours) is
//!   still there at the due time ([`PREPARE_AHEAD`]). Before that the job
//!   waits, and whatever is left runs at the due time.
//! - The run that publishes it **claims** it first, at or after the due
//!   time, once. The claim is kept with the publication, so a restart tells
//!   a run under way from one that never started, and a second runner (the
//!   background agent, later) never publishes it again.
//! - A publication is **missed**, and goes only once the user sends it now,
//!   reschedules or cancels it, when:
//!   - its due time passed while Bardo was closed and no run claimed it;
//!   - Bardo was open but could not start it within [`DUE_GRACE`] of its
//!     due time (the PC slept, the queue was busy);
//!   - a run claimed it and Bardo closed before it was done, and opened
//!     again more than [`DUE_GRACE`] after the claim. Opened sooner, the run
//!     resumes where it stopped.

use std::time::{Duration, SystemTime};

/// How long a network keeps an upload Bardo has not published yet
/// (Instagram expires an unpublished container after 24 hours).
pub const UNPUBLISHED_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// How long before its due time a scheduled upload may start: the
/// network's lifetime of an unpublished upload, less an hour for sending
/// and processing the file, so it is still there at the due time.
pub const PREPARE_AHEAD: Duration = Duration::from_secs(23 * 60 * 60);

/// How late after its due time a scheduled publication still starts on its
/// own, and how soon after its run was cut short by Bardo closing that run
/// resumes on its own. Later, the user decides.
pub const DUE_GRACE: Duration = Duration::from_secs(15 * 60);

/// When a scheduled publication's upload may start: [`PREPARE_AHEAD`]
/// before it is due.
pub fn prepare_at(due: SystemTime) -> SystemTime {
    due.checked_sub(PREPARE_AHEAD)
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// What a run of a scheduled publication does now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueStep {
    /// Too early to start even the upload: wait until then.
    Wait(SystemTime),
    /// Upload and process it now; publish at the due time.
    Prepare,
    /// Due: claim it and publish it.
    Publish,
    /// Missed: it waits for the user.
    Missed,
}

/// What a run of the publication due at `due` does at `now`, given when a
/// run claimed it (`claimed`) and when this session of Bardo opened
/// (`opened_at`). See the module docs.
pub fn due_step(
    due: SystemTime,
    claimed: Option<SystemTime>,
    now: SystemTime,
    opened_at: SystemTime,
) -> DueStep {
    if let Some(claimed) = claimed {
        // Claimed in this session: the run goes on, however long it takes
        // (a publishing limit may hold it for hours, which the user sees).
        return if opened_at > claimed + DUE_GRACE {
            DueStep::Missed
        } else {
            DueStep::Publish
        };
    }
    if due < opened_at || now > due + DUE_GRACE {
        DueStep::Missed
    } else if now >= due {
        DueStep::Publish
    } else if now >= prepare_at(due) {
        DueStep::Prepare
    } else {
        DueStep::Wait(prepare_at(due))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);
    const HOUR: Duration = Duration::from_secs(60 * 60);

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + secs)
    }

    /// Due at `at(DAY)`, a day after `at(0)`.
    const DAY: u64 = 24 * 60 * 60;

    fn due() -> SystemTime {
        at(DAY)
    }

    #[test]
    fn the_upload_starts_no_earlier_than_the_network_keeps_it_until_the_due_time() {
        let start = prepare_at(due());
        assert_eq!(due().duration_since(start).unwrap(), PREPARE_AHEAD);
        assert!(start + UNPUBLISHED_LIFETIME > due() + 30 * MINUTE);
    }

    #[test]
    fn a_publication_due_later_waits_then_prepares_then_publishes() {
        let opened = at(0);
        assert_eq!(
            due_step(due(), None, at(0), opened),
            DueStep::Wait(prepare_at(due()))
        );
        assert_eq!(
            due_step(due(), None, prepare_at(due()), opened),
            DueStep::Prepare
        );
        assert_eq!(
            due_step(due(), None, due() - MINUTE, opened),
            DueStep::Prepare
        );
        assert_eq!(due_step(due(), None, due(), opened), DueStep::Publish);
        assert_eq!(
            due_step(due(), None, due() + DUE_GRACE, opened),
            DueStep::Publish,
            "on time, within the grace"
        );
    }

    #[test]
    fn a_due_time_that_passed_while_bardo_was_closed_is_missed() {
        let opened = due() + MINUTE;
        assert_eq!(due_step(due(), None, opened, opened), DueStep::Missed);
        assert_eq!(
            due_step(due(), None, opened + HOUR, opened),
            DueStep::Missed,
            "and stays missed"
        );
        let opened_in_time = due() - MINUTE;
        assert_eq!(
            due_step(due(), None, due(), opened_in_time),
            DueStep::Publish
        );
    }

    #[test]
    fn a_publication_bardo_could_not_start_in_time_is_missed_though_open() {
        let opened = at(0);
        assert_eq!(
            due_step(due(), None, due() + DUE_GRACE + MINUTE, opened),
            DueStep::Missed
        );
    }

    #[test]
    fn a_claimed_run_goes_on_however_long_it_takes_while_bardo_stays_open() {
        let opened = at(0);
        let claimed = due() + MINUTE;
        assert_eq!(
            due_step(due(), Some(claimed), due() + 5 * HOUR, opened),
            DueStep::Publish
        );
    }

    #[test]
    fn a_claimed_run_cut_short_resumes_only_when_bardo_opens_again_soon() {
        let claimed = due() + MINUTE;
        let soon = claimed + 10 * MINUTE;
        assert_eq!(
            due_step(due(), Some(claimed), soon, soon),
            DueStep::Publish,
            "a restart during the run"
        );
        let late = claimed + DUE_GRACE + MINUTE;
        assert_eq!(due_step(due(), Some(claimed), late, late), DueStep::Missed);
    }
}
