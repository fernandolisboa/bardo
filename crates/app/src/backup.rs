//! Backup and restore from the app (issue #143, ADR-0010).
//!
//! A backup is a job: the database is copied while Bardo runs and, when the
//! user asks, the project folders go with it, into one `.zip`. A restore is
//! a job too, started after the user reviewed what the backup holds: it
//! takes the **database hold**, so no other job starts and the background
//! agent stops, unpacks and checks the backup beside the data, and Bardo
//! restarts to swap the files before the database opens
//! (`bardo_storage::apply_pending_restore`).
//!
//! Keys, app credentials and sign-ins never travel. After a restore,
//! `BackupView::set_up_again` lists what this computer still needs.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use bardo_domain::{
    AUTOMATIC_BACKUP_EVERY, AUTOMATIC_BACKUPS_KEPT, BackupError, BackupInfo, BackupOptions,
    BackupStore, ConnectionStatus, Job, JobFailure, JobFailureKind, JobId, JobKind, JobRepository,
    JobState, MediaSize, Network, NetworkAccountId, Progress, Provider, RUNNER_ALIVE,
    RepositoryError, RestoreOutcome, RunnerRole, SavedBackup, UserProfile,
};
use serde::{Deserialize, Serialize};

use crate::jobs::{JobActionError, JobContext, JobHandler};
use crate::scenes::{parse, to_json};
use crate::{AppError, Bardo, KeyState, Text};

/// How long the restore's hold on the database lasts without being
/// renewed. It is renewed as the restore goes, so a restore that crashed
/// frees the database by itself.
pub const RESTORE_HOLD: Duration = Duration::from_secs(2 * 60);

/// How long the hold stays once the backup is unpacked, while Bardo
/// restarts: the background agent must not open the database meanwhile.
pub const RESTART_HOLD: Duration = Duration::from_secs(5 * 60);

/// The share of a restore's progress that unpacking takes; the rest is
/// waiting for the background agent to stop.
const UNPACKED: u16 = 950;

/// The backup job's payload.
#[derive(Debug, Serialize, Deserialize)]
struct BackupPayload {
    to: PathBuf,
    media: bool,
    #[serde(default)]
    automatic: bool,
    /// Provider and network codes, as the manifest keeps them.
    #[serde(default)]
    saved_keys: Vec<String>,
    #[serde(default)]
    saved_credentials: Vec<String>,
}

/// The restore job's payload.
#[derive(Debug, Serialize, Deserialize)]
struct RestorePayload {
    from: PathBuf,
}

fn codes<T: std::str::FromStr>(codes: &[String]) -> Vec<T> {
    codes.iter().filter_map(|code| code.parse().ok()).collect()
}

/// A failed backup or restore as the jobs panel shows it.
fn failure(error: BackupError) -> JobFailure {
    match error {
        BackupError::Database(detail) => JobFailure::unexpected(detail),
        error => JobFailure::new(JobFailureKind::Files, error.to_string()),
    }
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// Writes a backup (`JobKind::Backup`).
pub(crate) struct BackupHandler {
    pub(crate) backups: Arc<dyn BackupStore>,
}

impl JobHandler for BackupHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: BackupPayload = parse(payload)?;
        let options = BackupOptions {
            media: payload.media,
            automatic: payload.automatic,
            saved_keys: codes(&payload.saved_keys),
            saved_credentials: codes(&payload.saved_credentials),
        };
        let cx = &*cx;
        let written = self.backups.write(&payload.to, options, &mut |progress| {
            cx.report_progress(progress);
            !cx.should_stop()
        });
        match written {
            Ok(info) => {
                tracing::info!(
                    media = info.media,
                    automatic = info.automatic,
                    "backed up the data"
                );
                if payload.automatic
                    && let Err(error) = self.backups.prune_automatic(AUTOMATIC_BACKUPS_KEPT)
                {
                    tracing::warn!("could not delete old automatic backups: {error}");
                }
                Ok(())
            }
            Err(BackupError::Stopped) => Ok(()),
            Err(error) => Err(failure(error)),
        }
    }
}

/// Unpacks a backup to replace the data at the next start
/// (`JobKind::Restore`), holding the database meanwhile.
pub(crate) struct RestoreHandler {
    pub(crate) backups: Arc<dyn BackupStore>,
    pub(crate) jobs: Arc<dyn JobRepository>,
    /// This session of Bardo: a restore that started in another one was
    /// cut short by Bardo closing, and is not run again on its own.
    pub(crate) session: String,
    /// How often it looks whether the background agent stopped.
    pub(crate) tick: Duration,
    /// Set once the backup is unpacked: Bardo restarts to apply it.
    pub(crate) restart_needed: Arc<AtomicBool>,
}

impl JobHandler for RestoreHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: RestorePayload = parse(payload)?;
        if cx
            .checkpoint()
            .is_some_and(|session| session != self.session)
        {
            // What it unpacked was removed at start
            // (`clean_restore_leftovers`); the user starts it again.
            let _ = self.backups.discard_staged();
            return Err(JobFailure::new(
                JobFailureKind::CutShort,
                "Bardo closed during the restore",
            ));
        }
        cx.save_checkpoint(self.session.clone(), Progress::ZERO)
            .map_err(unexpected)?;
        match self.restore(&payload.from, cx) {
            Ok(true) => {
                let now = SystemTime::now();
                if let Err(error) = self.jobs.hold(now, now + RESTART_HOLD) {
                    tracing::warn!("could not keep the database hold: {error}");
                }
                self.restart_needed.store(true, Ordering::SeqCst);
                tracing::info!("the backup is unpacked; Bardo restarts to apply it");
                Ok(())
            }
            Ok(false) => {
                self.give_up();
                Ok(())
            }
            Err(failure) => {
                self.give_up();
                Err(failure)
            }
        }
    }
}

impl RestoreHandler {
    /// Unpacks the backup and waits for the agent to stop. False when told
    /// to stop.
    fn restore(&self, from: &Path, cx: &JobContext) -> Result<bool, JobFailure> {
        self.renew_hold()?;
        let mut renewed = SystemTime::now();
        let staged = self.backups.stage(from, &mut |progress| {
            if renewed.elapsed().unwrap_or_default() >= self.tick && self.renew_hold().is_ok() {
                renewed = SystemTime::now();
            }
            let scaled = u32::from(progress.permille()) * u32::from(UNPACKED) / 1000;
            cx.report_progress(Progress::from_permille(scaled as u16));
            !cx.should_stop()
        });
        match staged {
            Ok(_) => {}
            Err(BackupError::Stopped) => return Ok(false),
            Err(error) => return Err(failure(error)),
        }
        // The agent stops under the hold once what it runs ends; the files
        // can be swapped only then.
        loop {
            self.renew_hold()?;
            let now = SystemTime::now();
            let others = self
                .jobs
                .runners(now.checked_sub(RUNNER_ALIVE).unwrap_or(now))
                .map_err(unexpected)?;
            if others.is_empty() {
                return Ok(true);
            }
            cx.report_progress(Progress::from_permille(UNPACKED + 25));
            if !cx.sleep(self.tick) {
                return Ok(false);
            }
        }
    }

    fn renew_hold(&self) -> Result<(), JobFailure> {
        let now = SystemTime::now();
        match self.jobs.hold(now, now + RESTORE_HOLD) {
            Ok(true) => Ok(()),
            Ok(false) => Err(JobFailure::unexpected("another Bardo holds the database")),
            Err(error) => Err(unexpected(error)),
        }
    }

    /// The data stays as it is: nothing staged, the database free.
    fn give_up(&self) {
        if let Err(error) = self.backups.discard_staged() {
            tracing::warn!("could not remove the unpacked backup: {error}");
        }
        if let Err(error) = self.jobs.release_hold() {
            tracing::warn!("could not release the database hold: {error}");
        }
    }
}

/// What Bardo knows about backups and restores, for Settings › Backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupView {
    /// Whether Bardo backs up the database by itself every week.
    pub auto_backup: bool,
    /// The newest backup made by hand.
    pub backup: Option<BackupJob>,
    /// The newest restore.
    pub restore: Option<Job>,
    /// A restore is unpacked: Bardo restarts to apply it.
    pub restart_needed: bool,
    /// Why a restore cannot start now.
    pub restore_block: Option<RestoreBlock>,
    /// How the restore applied at this start went, until the user closes
    /// the notice.
    pub last_restore: Option<RestoreOutcome>,
    /// After a restore: what this computer still needs, as of now.
    pub set_up_again: Vec<SetUpAgain>,
}

/// A backup job and where it writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupJob {
    pub job: Job,
    pub to: PathBuf,
    pub media: bool,
}

/// Why a restore waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreBlock {
    /// Jobs are running; a restore starts once they end or are cancelled.
    JobsRunning(usize),
    /// A restore is under way, or waiting for Bardo to restart.
    Restoring,
    /// Another Bardo window has the data open: only it restores.
    OtherWindow,
}

/// Something a restored profile needs set up again on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetUpAgain {
    /// The key was saved where the backup was made, and is not here.
    Key(Provider),
    /// The app credentials were saved where the backup was made, and are
    /// not here.
    AppCredentials(Network),
    /// The account's sign-in is not on this computer.
    Reconnect {
        account: NetworkAccountId,
        network: Network,
        handle: String,
    },
}

/// What the user reviews before a restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReview {
    pub path: PathBuf,
    pub info: BackupInfo,
}

/// Why a backup or restore did not start.
#[derive(Debug, thiserror::Error)]
pub enum BackupStartError {
    /// A backup is running already.
    #[error("a backup is running")]
    Running,
    #[error("a restore cannot start now: {0:?}")]
    Blocked(RestoreBlock),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl BackupStartError {
    pub fn message(&self) -> Text {
        match self {
            BackupStartError::Running => Text::BackupRunning,
            BackupStartError::Blocked(RestoreBlock::JobsRunning(_)) => Text::RestoreWaitsForJobs,
            BackupStartError::Blocked(RestoreBlock::Restoring) => Text::RestoreRunning,
            BackupStartError::Blocked(RestoreBlock::OtherWindow) => Text::RestoreOtherWindow,
            BackupStartError::Repository(_) => Text::JobNotUpdated,
        }
    }
}

/// What a backup or restore error says to the user.
pub fn backup_error_text(error: &BackupError) -> Text {
    match error {
        BackupError::NotABackup => Text::BackupNotABackup,
        BackupError::Damaged(_) => Text::BackupDamaged,
        BackupError::Newer { .. } => Text::BackupNewer,
        BackupError::Stopped | BackupError::Io(_) | BackupError::Database(_) => {
            Text::BackupUnreadable
        }
    }
}

/// Reads what a backup holds, off the UI thread.
pub struct BackupCheck {
    backups: Arc<dyn BackupStore>,
    path: PathBuf,
}

impl BackupCheck {
    pub fn run(self) -> Result<RestoreReview, BackupError> {
        let info = self.backups.inspect(&self.path)?;
        Ok(RestoreReview {
            path: self.path,
            info,
        })
    }
}

/// Lists the automatic backups, off the UI thread.
pub struct AutomaticBackups {
    backups: Arc<dyn BackupStore>,
}

impl AutomaticBackups {
    pub fn run(self) -> Result<Vec<SavedBackup>, BackupError> {
        self.backups.automatic()
    }
}

/// Measures the media a backup would carry, off the UI thread.
pub struct MediaSizeCheck {
    backups: Arc<dyn BackupStore>,
}

impl MediaSizeCheck {
    pub fn run(self) -> Result<MediaSize, BackupError> {
        self.backups.media_size()
    }
}

/// The name the save dialog offers, e.g.
/// `bardo-backup-20261014-093000.zip` (UTC, like the diagnostics').
pub fn backup_file_name(now: SystemTime) -> String {
    let at = jiff::Timestamp::try_from(now)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .strftime("%Y%m%d-%H%M%S");
    format!("bardo-backup-{at}.zip")
}

/// Where the save and open dialogs start: the Documents folder, else the
/// home folder.
pub fn backup_folder() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_default()
}

/// Set when another Bardo window holds the data (`bardo_storage::lock_window`).
static ANOTHER_WINDOW: AtomicBool = AtomicBool::new(false);

/// Called at start when another Bardo window has the data open: this one
/// leaves restores to it, neither starting one nor ending its holds.
pub fn beside_another_window() {
    ANOTHER_WINDOW.store(true, Ordering::SeqCst);
}

/// Whether another Bardo window holds the data.
pub(crate) fn another_window() -> bool {
    ANOTHER_WINDOW.load(Ordering::SeqCst)
}

/// Ends restores a closed Bardo left under way: they are not run again on
/// their own (a restart the user did not ask for), and the hold they took
/// is gone with the process that took it. Only the window that holds the
/// data restores, so at its start no live hold is left.
pub(crate) fn end_cut_short_restores(
    jobs: &dyn JobRepository,
    owner: bardo_domain::ProfileId,
    role: RunnerRole,
) -> Result<(), RepositoryError> {
    if role != RunnerRole::App || ANOTHER_WINDOW.load(Ordering::SeqCst) {
        return Ok(());
    }
    jobs.clear_hold()?;
    let now = SystemTime::now();
    for mut job in jobs.list(owner)? {
        if job.kind() != JobKind::Restore || !job.state().is_active() {
            continue;
        }
        if job.state() == JobState::Queued && job.start(now).is_err() {
            continue;
        }
        let cut_short =
            JobFailure::new(JobFailureKind::CutShort, "Bardo closed during the restore");
        if job
            .fail_attempt(cut_short, now, &bardo_domain::RetryPolicy::default())
            .is_ok()
        {
            match jobs.save(&job) {
                Err(error) if error.is_held_elsewhere() => {}
                result => result?,
            }
        }
    }
    Ok(())
}

impl Bardo {
    /// Settings › Backup as of now.
    pub fn backup_view(&self) -> BackupView {
        let jobs = self.jobs();
        let backup = jobs
            .iter()
            .rev()
            .filter(|job| job.kind() == JobKind::Backup)
            .find_map(|job| {
                let payload: BackupPayload = serde_json::from_str(job.payload()).ok()?;
                (!payload.automatic).then(|| BackupJob {
                    job: job.clone(),
                    to: payload.to,
                    media: payload.media,
                })
            });
        let restore = jobs
            .iter()
            .rev()
            .find(|job| job.kind() == JobKind::Restore)
            .cloned();
        let last_restore = self.backups.last_restore();
        let set_up_again = last_restore
            .as_ref()
            .filter(|outcome| outcome.succeeded())
            .map(|outcome| self.set_up_again(outcome))
            .unwrap_or_default();
        BackupView {
            auto_backup: self.profile.auto_backup,
            backup,
            restore,
            restart_needed: self.restart_needed(),
            restore_block: self.restore_block(&jobs),
            last_restore,
            set_up_again,
        }
    }

    /// What this computer lacks of what the restored profile used.
    fn set_up_again(&self, outcome: &RestoreOutcome) -> Vec<SetUpAgain> {
        let mut missing: Vec<SetUpAgain> = outcome
            .saved_keys
            .iter()
            .filter(|provider| self.provider_key(**provider).state == KeyState::NotSet)
            .map(|provider| SetUpAgain::Key(*provider))
            .collect();
        let credentials = self.app_credentials();
        missing.extend(
            outcome
                .saved_credentials
                .iter()
                .filter(|network| {
                    credentials.iter().any(|status| {
                        status.network == **network && status.state == KeyState::NotSet
                    })
                })
                .map(|network| SetUpAgain::AppCredentials(*network)),
        );
        let connections = self.connection_book.connections();
        for channel in self.channels().unwrap_or_default() {
            for account in self.network_accounts.list(channel.id).unwrap_or_default() {
                if connections
                    .status(account.id)
                    .is_some_and(|status| status == ConnectionStatus::ReconnectNeeded)
                {
                    missing.push(SetUpAgain::Reconnect {
                        account: account.id,
                        network: account.network,
                        handle: account.details.handle().to_owned(),
                    });
                }
            }
        }
        missing
    }

    fn restore_block(&self, jobs: &[Job]) -> Option<RestoreBlock> {
        if ANOTHER_WINDOW.load(Ordering::SeqCst) {
            return Some(RestoreBlock::OtherWindow);
        }
        if self.restart_needed()
            || jobs
                .iter()
                .any(|job| job.kind() == JobKind::Restore && job.state().is_active())
        {
            return Some(RestoreBlock::Restoring);
        }
        let running = jobs
            .iter()
            .filter(|job| job.state() == JobState::Running)
            .count();
        (running > 0).then_some(RestoreBlock::JobsRunning(running))
    }

    /// A restore is unpacked: Bardo restarts to apply it.
    pub fn restart_needed(&self) -> bool {
        self.restart_needed.load(Ordering::SeqCst)
    }

    /// Backs up the database and, when `media`, the project folders, to
    /// `to` (replacing it), as a job.
    pub fn start_backup(&self, to: PathBuf, media: bool) -> Result<JobId, BackupStartError> {
        let busy = self.jobs().iter().any(|job| {
            job.kind() == JobKind::Backup
                && job.state().is_active()
                && serde_json::from_str::<BackupPayload>(job.payload())
                    .is_ok_and(|payload| !payload.automatic)
        });
        if busy {
            return Err(BackupStartError::Running);
        }
        self.enqueue_backup(to, media, false)
    }

    fn enqueue_backup(
        &self,
        to: PathBuf,
        media: bool,
        automatic: bool,
    ) -> Result<JobId, BackupStartError> {
        let saved_keys = self
            .provider_keys()
            .into_iter()
            .filter(|status| matches!(status.state, KeyState::Saved { .. }))
            .map(|status| status.provider.code().to_owned())
            .collect();
        let saved_credentials = self
            .app_credentials()
            .into_iter()
            .filter(|status| matches!(status.state, KeyState::Saved { .. }))
            .map(|status| status.network.code().to_owned())
            .collect();
        let payload = BackupPayload {
            to,
            media,
            automatic,
            saved_keys,
            saved_credentials,
        };
        let job = Job::new(self.profile.id, JobKind::Backup, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// The weekly automatic backup, queued when the profile has them on
    /// and the newest is a week old (or there is none). Only Bardo's
    /// window backs up.
    pub fn back_up_on_start(&self) {
        if self.role != RunnerRole::App || !self.profile.auto_backup {
            return;
        }
        let pending = self.jobs().iter().any(|job| {
            job.kind() == JobKind::Backup
                && job.state().is_active()
                && serde_json::from_str::<BackupPayload>(job.payload())
                    .is_ok_and(|payload| payload.automatic)
        });
        if pending {
            return;
        }
        let now = SystemTime::now();
        let due = match self.backups.automatic() {
            Ok(saved) => saved.first().is_none_or(|newest| {
                now.duration_since(newest.created_at)
                    .is_ok_and(|age| age >= AUTOMATIC_BACKUP_EVERY)
            }),
            Err(error) => {
                tracing::warn!("could not list the automatic backups: {error}");
                return;
            }
        };
        if due
            && let Err(error) = self.enqueue_backup(self.backups.automatic_path(now), false, true)
        {
            tracing::warn!("could not queue the automatic backup: {error}");
        }
    }

    /// The automatic backups kept, newest first. Run it off the UI
    /// thread: it reads each one's manifest.
    pub fn automatic_backups(&self) -> AutomaticBackups {
        AutomaticBackups {
            backups: Arc::clone(&self.backups),
        }
    }

    /// Turns the weekly automatic backup on or off.
    pub fn set_auto_backup(&mut self, on: bool) -> Result<(), AppError> {
        if on == self.profile.auto_backup {
            return Ok(());
        }
        let updated = UserProfile {
            auto_backup: on,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        Ok(())
    }

    /// How big the media in a backup would be. Run it off the UI thread.
    pub fn media_size_check(&self) -> MediaSizeCheck {
        MediaSizeCheck {
            backups: Arc::clone(&self.backups),
        }
    }

    /// What the backup at `path` holds, for the review. Run it off the UI
    /// thread.
    pub fn backup_check(&self, path: PathBuf) -> BackupCheck {
        BackupCheck {
            backups: Arc::clone(&self.backups),
            path,
        }
    }

    /// Restores the reviewed backup: everything here is replaced at the
    /// next start, which follows at once. Refused while jobs run.
    pub fn start_restore(&self, review: &RestoreReview) -> Result<JobId, BackupStartError> {
        self.restore_from(review.path.clone())
    }

    fn restore_from(&self, from: PathBuf) -> Result<JobId, BackupStartError> {
        self.take_restore_hold()?;
        let job = Job::new(
            self.profile.id,
            JobKind::Restore,
            to_json(&RestorePayload { from }),
        );
        match self.jobs.enqueue(job) {
            Ok(id) => {
                tracing::info!("restoring a backup");
                Ok(id)
            }
            Err(error) => {
                self.release_restore_hold();
                Err(error.into())
            }
        }
    }

    /// Retrying a restore starts it again from the same file, as a new
    /// job, under the same checks as the first time.
    pub(crate) fn retry_restore(&self, job: &Job) -> Result<(), JobActionError> {
        if !job.can_retry() {
            return self.jobs.retry(job.id());
        }
        let Ok(payload) = serde_json::from_str::<RestorePayload>(job.payload()) else {
            return self.jobs.retry(job.id());
        };
        match self.restore_from(payload.from) {
            Ok(_) => Ok(()),
            Err(BackupStartError::Repository(error)) => Err(error.into()),
            Err(_) => Err(JobActionError::Busy),
        }
    }

    /// Takes the database hold, so nothing else starts, then makes sure
    /// nothing runs.
    fn take_restore_hold(&self) -> Result<(), BackupStartError> {
        if let Some(block) = self.restore_block(&self.jobs()) {
            return Err(BackupStartError::Blocked(block));
        }
        let now = SystemTime::now();
        if !self.jobs.hold(now, now + RESTORE_HOLD)? {
            return Err(BackupStartError::Blocked(RestoreBlock::Restoring));
        }
        // A job that started meanwhile, here or in the agent.
        if let Some(block) = self.restore_block(&self.jobs()) {
            self.release_restore_hold();
            return Err(BackupStartError::Blocked(block));
        }
        Ok(())
    }

    fn release_restore_hold(&self) {
        if let Err(error) = self.jobs.release_hold() {
            tracing::warn!("could not release the database hold: {error}");
        }
    }

    /// The user read the notice of the last restore.
    pub fn forget_last_restore(&self) {
        if let Err(error) = self.backups.forget_last_restore() {
            tracing::warn!("could not forget the last restore: {error}");
        }
    }

    /// After a successful restore on another computer: the connections the
    /// backup had are in the database, but their sign-ins are not in this
    /// computer's secret store, so they need reconnecting.
    pub(crate) fn mark_restored_connections(&self) {
        if self.role != RunnerRole::App
            || !self
                .backups
                .last_restore()
                .is_some_and(|outcome| outcome.succeeded())
        {
            return;
        }
        match self.connection_book.connections().mark_signed_out() {
            Ok(0) => {}
            Ok(marked) => tracing::info!(marked, "restored connections need reconnecting"),
            Err(error) => tracing::warn!("could not check the restored connections: {error}"),
        }
    }

    /// When a backup was made, on this computer's clock, e.g. `Wed,
    /// October 14, 2026, 9:30 AM`.
    pub fn backup_time_text(&self, at: SystemTime) -> String {
        self.catalog
            .date_time_here(&bardo_domain::local_time(at, &self.zone))
    }

    /// A size on disk, e.g. `1.2 GB`, `350 MB`, `4,5 MB`, `12 KB`.
    pub fn size_text(&self, bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: f64 = (KB * KB) as f64;
        const GB: f64 = MB * 1024.0;
        if bytes < KB * KB {
            return format!("{} KB", bytes.div_ceil(KB).max(1));
        }
        let bytes = bytes as f64;
        let (value, unit) = if bytes >= GB {
            (bytes / GB, "GB")
        } else {
            (bytes / MB, "MB")
        };
        let digits = if value >= 100.0 { 0 } else { 1 };
        let number = self.localized_decimal(&format!("{value:.digits$}"));
        format!("{number} {unit}")
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    use bardo_domain::{
        ConnectionSecrets as _, JobFailureKind, NetworkConnectionRepository, SecretStore as _,
        UploadFailure,
    };
    use bardo_storage::{DataPaths, Database, LocalBackups, MemoryBackups};

    use super::*;
    use crate::UploadChoices;
    use crate::UploadState;
    use crate::export::tests::Setup;
    use crate::publications::whole_millis;
    use crate::scenes::tests::{Harness, done, wait_done};
    use crate::uploads::reel_tests::{ready, review, state, upload};
    use crate::uploads::testing::Attempt;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);
    const META_ID: &str = "1234567890123456";
    const META_SECRET: &str = concat!("0123456789abcdef", "0123456789abcdef");

    fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(Instant::now() < deadline, "never {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn paths(dir: &Path) -> DataPaths {
        DataPaths {
            database: dir.join("roaming").join("bardo.db"),
            projects: dir.join("local").join("projects"),
            local: dir.join("local"),
        }
    }

    /// `s` restarted with its backups on disk under `dir`.
    fn on_disk(mut s: Setup, dir: &Path) -> Setup {
        let backups = LocalBackups::new(Arc::clone(&s.h.db), paths(dir));
        s.h.backups = Some(Arc::new(backups));
        s.restart()
    }

    /// Bardo starts again after a restore: the staged backup is swapped
    /// in, and Bardo opens on it.
    fn restarted(mut s: Setup, dir: &Path) -> Setup {
        let paths = paths(dir);
        let outcome = bardo_storage::apply_pending_restore(&paths).expect("a staged restore");
        assert!(outcome.succeeded(), "{:?}", outcome.failure);
        let db = Arc::new(Database::open(&paths.database).unwrap());
        s.h.backups = Some(Arc::new(LocalBackups::new(Arc::clone(&db), paths)));
        s.reopen_on(db)
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

    fn back_up(s: &Setup, to: &Path) {
        done(&s.app, s.app.start_backup(to.to_owned(), true).unwrap());
    }

    fn restore(s: &Setup, from: &Path) {
        let review = s.app.backup_check(from.to_owned()).run().unwrap();
        let job = s.app.start_restore(&review).unwrap();
        done(&s.app, job);
        assert!(s.app.restart_needed());
        assert_eq!(
            s.app.backup_view().restore_block,
            Some(RestoreBlock::Restoring)
        );
    }

    fn reels_account(s: &Setup) -> NetworkAccountId {
        s.app
            .network_accounts(s.project.channel)
            .unwrap()
            .into_iter()
            .find(|account| account.network == Network::InstagramReels)
            .unwrap()
            .id
    }

    fn job(s: &Setup, id: JobId) -> Job {
        s.app.jobs().into_iter().find(|job| job.id() == id).unwrap()
    }

    #[test]
    fn a_backup_file_is_named_by_when_it_was_made() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_970_200);
        assert_eq!(backup_file_name(at), "bardo-backup-20261014-093000.zip");
    }

    #[test]
    fn sizes_read_in_the_interface_language() {
        let h = Harness::new();
        let mut app = h.start();
        assert_eq!(app.size_text(0), "1 KB");
        assert_eq!(app.size_text(12 * 1024), "12 KB");
        assert_eq!(app.size_text(4_718_592), "4.5 MB");
        assert_eq!(app.size_text(350 * 1024 * 1024), "350 MB");
        assert_eq!(app.size_text(1_288_490_189), "1.2 GB");
        app.set_ui_language(bardo_domain::UiLanguage::PtBr).unwrap();
        assert_eq!(app.size_text(4_718_592), "4,5 MB");
    }

    #[test]
    fn no_key_credential_or_token_reaches_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ready();
        s.app
            .save_app_credentials(Network::InstagramReels, META_ID, META_SECRET)
            .unwrap();
        let owner = s.app.profile().id;
        let media = paths(dir.path()).projects.join(s.project.id.to_string());
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("narration.mp3"), b"narration").unwrap();
        let s = on_disk(s, dir.path());

        // Every secret Bardo holds for this profile.
        let mut secrets: Vec<String> = Vec::new();
        for provider in Provider::ALL {
            if let Some(key) = s.h.secrets.get(owner, provider).unwrap() {
                secrets.extend(key.sensitive_parts().into_iter().map(str::to_owned));
            }
        }
        let credentials =
            s.h.connection_secrets
                .app_credentials(owner, Network::InstagramReels)
                .unwrap()
                .unwrap();
        secrets.extend(credentials.sensitive_parts().into_iter().map(str::to_owned));
        let account = reels_account(&s);
        let tokens =
            s.h.connection_secrets
                .tokens(owner, account)
                .unwrap()
                .unwrap();
        secrets.extend(tokens.sensitive_parts().into_iter().map(str::to_owned));
        assert!(secrets.len() >= 6, "keys, credentials and tokens");

        let backup = dir.path().join("backup.zip");
        back_up(&s, &backup);

        let mut contents = vec![std::fs::read(&backup).unwrap()];
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&backup).unwrap()).unwrap();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            contents.push(bytes);
        }
        assert!(archive.by_name("projects").is_err());
        for secret in &secrets {
            for bytes in &contents {
                assert!(
                    !bytes
                        .windows(secret.len())
                        .any(|window| window == secret.as_bytes()),
                    "a secret reached the backup"
                );
            }
        }
        // Only the names of what was saved, for the list after a restore.
        let info = s.app.backup_check(backup).run().unwrap().info;
        assert!(info.media);
        assert_eq!(info.media_files, 1);
        assert_eq!(
            info.saved_keys,
            [
                Provider::Claude,
                Provider::ElevenLabs,
                Provider::Gemini,
                Provider::Higgsfield
            ]
        );
        assert_eq!(info.saved_credentials, [Network::InstagramReels]);
    }

    #[test]
    fn a_restored_backup_brings_back_the_projects_and_the_scheduled_posts() {
        let dir = tempfile::tempdir().unwrap();
        let s = on_disk(ready(), dir.path());
        let due = whole_millis(SystemTime::now() + 2 * DAY);
        let id = schedule(&s, due);
        let backup = dir.path().join("backup.zip");
        back_up(&s, &backup);

        // Work after the backup is lost by the restore.
        s.app.cancel_job(id).unwrap();
        restore(&s, &backup);
        assert!(s.app.missed_posts().unwrap().is_empty());
        let s = restarted(s, dir.path());

        assert!(
            s.app
                .video_projects(s.project.channel)
                .unwrap()
                .iter()
                .any(|project| project.id == s.project.id),
            "the project is back"
        );
        assert_eq!(state(&s), UploadState::Due(due));
        let waiting = job(&s, id);
        assert_eq!(waiting.state(), JobState::Queued, "the schedule is back");
        assert_eq!(waiting.run_at(), Some(bardo_domain::prepare_at(due)));
        assert!(
            s.app
                .jobs()
                .iter()
                .all(|job| !matches!(job.kind(), JobKind::Backup | JobKind::Restore)),
            "a restored backup does not run its backup or restore again"
        );

        // Its time comes: it goes, once.
        let mut publication = upload(&s);
        let soon = whole_millis(SystemTime::now() + Duration::from_millis(300));
        publication.upload_mut().unwrap().publish_at = Some(soon);
        assert!(s.app.publications.save_upload(&publication).unwrap());
        s.app.cancel_job(id).unwrap();
        s.app.resume_upload(id).unwrap();
        done(&s.app, id);
        assert_eq!(state(&s), UploadState::Published);
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 1);
        assert_eq!(s.h.reels.files.lock().unwrap().len(), 1, "sent once");

        // Restored on the same computer: nothing to set up again.
        let view = s.app.backup_view();
        assert!(view.last_restore.as_ref().unwrap().succeeded());
        assert!(view.set_up_again.is_empty());
        s.app.forget_last_restore();
        assert!(s.app.backup_view().last_restore.is_none());
    }

    #[test]
    fn a_post_being_sent_when_the_backup_was_made_is_missed_after_a_restore() {
        let dir = tempfile::tempdir().unwrap();
        let s = on_disk(ready(), dir.path());
        s.h.reels.stall_publish.store(true, Ordering::SeqCst);
        let id = schedule(&s, SystemTime::now() + Duration::from_millis(300));
        wait_until("publishing", || s.h.reels.stalled.load(Ordering::SeqCst));
        assert!(upload(&s).upload().unwrap().claimed_at.is_some());
        let backup = dir.path().join("backup.zip");
        back_up(&s, &backup);

        // A restore waits for the running job.
        let review = s.app.backup_check(backup.clone()).run().unwrap();
        assert!(matches!(
            s.app.start_restore(&review),
            Err(BackupStartError::Blocked(RestoreBlock::JobsRunning(1)))
        ));
        assert_eq!(
            s.app.backup_view().restore_block,
            Some(RestoreBlock::JobsRunning(1))
        );
        // Staged as the job would, the post went out meanwhile.
        s.h.backups
            .as_ref()
            .unwrap()
            .stage(&backup, &mut |_| true)
            .unwrap();
        let s = restarted(s, dir.path());
        s.h.reels.stall_publish.store(false, Ordering::SeqCst);

        let ended = wait_done(&s.app, id);
        assert_eq!(ended.failure().unwrap().kind, JobFailureKind::Missed);
        assert_eq!(s.app.missed_posts().unwrap().len(), 1);
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 1, "not twice");
    }

    #[test]
    fn an_upload_under_way_when_the_backup_was_made_is_not_sent_again_after_a_restore() {
        let dir = tempfile::tempdir().unwrap();
        let s = on_disk(ready(), dir.path());
        s.h.reels.will(Attempt::HoldAfter(0));
        let review = review(&s);
        let id = s.app.start_upload(&review, review.choices()).unwrap();
        wait_until("uploading", || s.h.reels.holding.load(Ordering::SeqCst));
        let backup = dir.path().join("backup.zip");
        back_up(&s, &backup);

        // It goes after the backup was made.
        s.app.cancel_job(id).unwrap();
        wait_done(&s.app, id);
        s.app.resume_upload(id).unwrap();
        done(&s.app, id);
        assert_eq!(state(&s), UploadState::Published);
        restore(&s, &backup);
        let s = restarted(s, dir.path());

        let ended = wait_done(&s.app, id);
        assert_eq!(ended.failure().unwrap().kind, JobFailureKind::Restored);
        assert!(matches!(
            state(&s),
            UploadState::Failed {
                failure: UploadFailure::Job(JobFailureKind::Restored),
                ..
            }
        ));
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(s.h.reels.files.lock().unwrap().len(), 1, "sent once");
        assert_eq!(s.h.reels.published.lock().unwrap().len(), 1, "posted once");
    }

    #[test]
    fn after_a_restore_on_another_computer_the_missing_keys_and_sign_ins_are_listed() {
        let mut s = ready();
        let owner = s.app.profile().id;
        let account = reels_account(&s);
        // This computer has the database, not the secrets.
        s.h.connection_secrets
            .delete_tokens(owner, account)
            .unwrap();
        let backups = Arc::new(MemoryBackups::default());
        backups.set_last_restore(Some(RestoreOutcome {
            backup_created_at: SystemTime::now() - DAY,
            media: true,
            restored_at: SystemTime::now(),
            failure: None,
            kept_aside: None,
            saved_keys: vec![Provider::Claude, Provider::YouTubeData],
            saved_credentials: vec![Network::InstagramReels],
        }));
        s.h.backups = Some(Arc::clone(&backups) as _);
        let s = s.restart();

        let view = s.app.backup_view();
        assert_eq!(
            view.set_up_again,
            [
                SetUpAgain::Key(Provider::YouTubeData),
                SetUpAgain::AppCredentials(Network::InstagramReels),
                SetUpAgain::Reconnect {
                    account,
                    network: Network::InstagramReels,
                    handle: s
                        .app
                        .network_accounts(s.project.channel)
                        .unwrap()
                        .into_iter()
                        .find(|found| found.id == account)
                        .unwrap()
                        .details
                        .handle()
                        .to_owned(),
                },
            ]
        );
        assert_eq!(
            NetworkConnectionRepository::get(&*s.h.db, account)
                .unwrap()
                .unwrap()
                .status,
            ConnectionStatus::ReconnectNeeded
        );
    }

    #[test]
    fn bardo_backs_up_the_database_once_a_week_while_turned_on() {
        let h = Harness {
            backups: Some(Arc::new(MemoryBackups::default())),
            ..Harness::new()
        };
        let mut app = h.start();
        app.back_up_on_start();
        let first = app
            .jobs()
            .into_iter()
            .find(|job| job.kind() == JobKind::Backup)
            .unwrap();
        done(&app, first.id());
        let saved = app.automatic_backups().run().unwrap();
        assert_eq!(saved.len(), 1);
        assert!(app.backup_view().backup.is_none(), "not one made by hand");

        app.back_up_on_start();
        assert_eq!(
            app.jobs()
                .iter()
                .filter(|job| job.kind() == JobKind::Backup)
                .count(),
            1,
            "the newest is less than a week old"
        );

        app.set_auto_backup(false).unwrap();
        drop(app);
        let app = h.start();
        assert!(!app.backup_view().auto_backup, "remembered");
    }

    #[test]
    fn a_backup_made_by_hand_shows_where_it_went() {
        let h = Harness::new();
        let app = h.start();
        let to = PathBuf::from("Documents").join("bardo-backup.zip");
        let id = app.start_backup(to.clone(), false).unwrap();
        done(&app, id);
        let shown = app.backup_view().backup.unwrap();
        assert_eq!(shown.job.id(), id);
        assert_eq!(shown.to, to);
        assert!(!shown.media);
    }

    #[test]
    fn a_restore_cut_short_by_closing_is_not_run_again_and_frees_the_database() {
        let h = Harness::new();
        let app = h.start();
        let backup = PathBuf::from("backup.zip");
        // Made and staged on another start, whose restore never ended.
        let mut stuck = Job::new(
            app.profile().id,
            JobKind::Restore,
            to_json(&RestorePayload {
                from: backup.clone(),
            }),
        );
        stuck.start(SystemTime::now()).unwrap();
        stuck
            .record_progress(Progress::ZERO, Some("an earlier session".into()))
            .unwrap();
        bardo_domain::JobRepository::save(&*h.db, &stuck).unwrap();
        let earlier = h.process_db();
        assert!(
            bardo_domain::JobRepository::hold(
                &*earlier,
                SystemTime::now(),
                SystemTime::now() + RESTORE_HOLD
            )
            .unwrap()
        );
        drop(app);

        let app = h.start();
        let ended = job_of(&app, stuck.id());
        assert_eq!(ended.state(), JobState::Failed);
        assert_eq!(ended.failure().unwrap().kind, JobFailureKind::CutShort);
        assert!(!app.restart_needed());
        assert_eq!(
            bardo_domain::JobRepository::holder(&*h.db, SystemTime::now()).unwrap(),
            None,
            "the hold went with that start"
        );
        // Jobs run again.
        let quick = crate::TestJob {
            steps: 1,
            step: Duration::from_millis(1),
            fail_at_step: None,
        };
        done(&app, app.start_test_job(quick).unwrap());
    }

    fn job_of(app: &Bardo, id: JobId) -> Job {
        app.jobs().into_iter().find(|job| job.id() == id).unwrap()
    }
}
