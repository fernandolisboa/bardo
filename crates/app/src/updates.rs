//! Updating Bardo from its published releases (issue #144, ADR-0011).
//!
//! Bardo's window asks the release feed for the latest release when it
//! opens and once a day, unless the user turned that off; the user can
//! always check by hand. A newer release shows a banner. Downloading its
//! installer is a job the user starts: the file is checked against the
//! SHA-256 its release publishes, and kept only when it matches. Installing
//! is the user's call after a review: the file is checked again, the
//! installer starts on its own, and Bardo quits so it can be replaced; the
//! installer opens Bardo again when it is done.
//!
//! A copy from the Microsoft Store never checks: the Store updates it. A
//! copy built from source checks, and leaves the download to the user.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    DownloadError, InstallerRunner, InstallerStore, Job, JobFailure, JobFailureKind, JobId,
    JobKind, JobState, Progress, Release, ReleaseFeed, RepositoryError, RunnerRole,
    UpdateCheckError, UserProfile, Version, checksum_for, installer_name, installer_version,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::jobs::{JobContext, JobHandler};
use crate::scenes::{parse, to_json};
use crate::{AppError, Bardo, Text};

/// How often Bardo's window checks while it stays open.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// The largest installer Bardo downloads. Today's is about 50 MB.
pub const MAX_INSTALLER_BYTES: u64 = 1024 * 1024 * 1024;

/// The largest checksum file Bardo reads. Today's is one line.
const MAX_CHECKSUM_BYTES: u64 = 4096;

/// How the installer runs for an update: no wizard, only its progress
/// window; no question boxes; no restart of Windows; and Bardo opens again
/// at the end (`installer/bardo.iss` reads `/RELAUNCH=1`).
pub const INSTALLER_ARGUMENTS: [&str; 4] =
    ["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/RELAUNCH=1"];

/// Where Bardo's release pages are.
const RELEASES_PAGE: &str = "https://github.com/fernandolisboa/bardo/releases/";

/// This build's version.
pub fn current_version() -> Version {
    env!("CARGO_PKG_VERSION")
        .parse()
        .expect("the workspace version is a version")
}

/// How this copy of Bardo was installed, which decides how it updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installation {
    /// By Bardo's installer: Bardo downloads and runs the next one.
    Installer,
    /// From the Microsoft Store, which updates it: Bardo never checks.
    Store,
    /// Built from source: Bardo says when a release is out and links to
    /// it, and the user updates the source.
    Source,
}

impl Installation {
    /// `of`, except that a debug build reads `BARDO_INSTALLATION`
    /// (`installer`, `store` or `source`), to see each one's screens.
    pub(crate) fn here(program: &Path) -> Installation {
        if cfg!(debug_assertions) {
            match std::env::var("BARDO_INSTALLATION").as_deref() {
                Ok("installer") => return Installation::Installer,
                Ok("store") => return Installation::Store,
                Ok("source") => return Installation::Source,
                _ => {}
            }
        }
        Installation::of(program)
    }

    /// From where `program` runs: the Store's `WindowsApps` folder, or the
    /// installed layout (the notices sit next to `bardo.exe`) on Windows.
    pub fn of(program: &Path) -> Installation {
        if program
            .components()
            .any(|part| part.as_os_str().eq_ignore_ascii_case("WindowsApps"))
        {
            return Installation::Store;
        }
        let installed = program
            .parent()
            .is_some_and(|dir| dir.join(crate::about::NOTICES_FILE).is_file());
        if cfg!(windows) && installed {
            Installation::Installer
        } else {
            Installation::Source
        }
    }
}

/// What the last check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateFound {
    /// This copy is the latest release, or newer.
    UpToDate,
    /// A newer release is out.
    Newer,
    /// Nothing is published yet.
    NoRelease,
    /// GitHub could not be reached.
    Offline,
    /// GitHub asked to wait until then.
    RateLimited(Option<SystemTime>),
    /// GitHub's answer made no sense to Bardo.
    Failed,
}

/// A check that ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateLastCheck {
    pub at: SystemTime,
    pub found: UpdateFound,
}

/// Why installing cannot start now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallBlock {
    /// Jobs run in this window; installing waits until they end or are
    /// cancelled.
    JobsRunning(usize),
    /// A restore is under way, or waits for Bardo to restart.
    Restoring,
    /// Another Bardo window is open: the installer would close it.
    OtherWindow,
}

/// A newer release and where this copy is with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub release: Release,
    /// Whether Bardo can download and install it here: an installed copy,
    /// and a release with its installer and checksum.
    pub installable: bool,
    /// The newest download of it.
    pub download: Option<Job>,
    /// Downloaded and checked: it can be installed.
    pub ready: bool,
    /// Why installing waits, when it is ready.
    pub install_block: Option<InstallBlock>,
}

impl AvailableUpdate {
    /// The download's progress while it runs.
    pub fn downloading(&self) -> Option<&Job> {
        self.download.as_ref().filter(|job| job.state().is_active())
    }

    /// The release's page on GitHub, only when it is this repository's.
    pub fn release_page(&self) -> Option<&str> {
        let page = self.release.page.as_str();
        page.strip_prefix(RELEASES_PAGE)
            .is_some_and(|rest| {
                !rest.is_empty() && !rest.contains(['?', '#', '\\']) && !rest.contains("..")
            })
            .then_some(page)
    }

    /// The download failed, and nothing newer replaced it.
    pub fn failed_download(&self) -> Option<&Job> {
        self.download
            .as_ref()
            .filter(|job| job.state() == JobState::Failed)
    }
}

/// What Settings › About and the banner show of updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateView {
    pub installation: Installation,
    /// This copy's version.
    pub current: Version,
    /// Whether Bardo checks by itself.
    pub check_on: bool,
    /// A check is under way.
    pub checking: bool,
    pub last_check: Option<UpdateLastCheck>,
    pub available: Option<AvailableUpdate>,
    /// The banner over the screens shows: a newer release the user has
    /// not put off since Bardo opened.
    pub banner: bool,
}

/// Why an update did not start.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// No newer release is known.
    #[error("no update is available")]
    NotAvailable,
    /// This copy does not install updates itself, or the release has no
    /// installer.
    #[error("this update cannot be installed from Bardo")]
    NotInstallable,
    /// Its download is under way.
    #[error("the update is downloading")]
    Downloading,
    /// Not downloaded, or its file is gone.
    #[error("the update is not downloaded")]
    NotDownloaded,
    #[error("installing waits: {0:?}")]
    Blocked(InstallBlock),
    /// The file no longer matches its checksum: it was deleted.
    #[error("the downloaded installer no longer matches its checksum")]
    Checksum,
    /// The system did not start the installer.
    #[error("the installer did not start: {0}")]
    NotStarted(std::io::Error),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl UpdateError {
    /// What About says.
    pub fn message(&self) -> Text {
        match self {
            UpdateError::NotAvailable | UpdateError::NotInstallable => Text::UpdateNoInstaller,
            UpdateError::Downloading => Text::UpdateAlreadyDownloading,
            UpdateError::NotDownloaded => Text::UpdateFileGone,
            UpdateError::Blocked(block) => install_block_text(*block),
            UpdateError::Checksum => Text::UpdateChecksumChanged,
            UpdateError::NotStarted(_) => Text::UpdateNotStarted,
            UpdateError::Repository(_) => Text::JobNotStarted,
        }
    }
}

/// What a reason to wait says.
pub fn install_block_text(block: InstallBlock) -> Text {
    match block {
        InstallBlock::JobsRunning(_) => Text::UpdateWaitsForJobs,
        InstallBlock::Restoring => Text::UpdateWaitsForRestore,
        InstallBlock::OtherWindow => Text::UpdateOtherWindow,
    }
}

/// What Bardo keeps about updates while it runs.
pub(crate) struct UpdateBook {
    pub(crate) feed: Arc<dyn ReleaseFeed>,
    pub(crate) installers: Arc<dyn InstallerStore>,
    pub(crate) runner: Arc<dyn InstallerRunner>,
    pub(crate) installation: Installation,
    pub(crate) current: Version,
    checking: bool,
    last_check: Option<UpdateLastCheck>,
    /// The newest release found newer than this copy.
    newer: Option<Release>,
    /// The version whose banner the user put off.
    put_off: Option<Version>,
}

impl UpdateBook {
    pub(crate) fn new(
        feed: Arc<dyn ReleaseFeed>,
        installers: Arc<dyn InstallerStore>,
        runner: Arc<dyn InstallerRunner>,
        installation: Installation,
    ) -> Self {
        Self {
            feed,
            installers,
            runner,
            installation,
            current: current_version(),
            checking: false,
            last_check: None,
            newer: None,
            put_off: None,
        }
    }
}

/// A check for a newer release. Run it off the UI thread, then hand its
/// result to `Bardo::record_update_check`.
#[must_use = "nothing is checked until it runs"]
pub struct UpdateCheck {
    feed: Arc<dyn ReleaseFeed>,
}

/// What a check found, for `Bardo::record_update_check`.
#[derive(Debug)]
pub struct UpdateCheckResult {
    at: SystemTime,
    result: Result<Release, UpdateCheckError>,
}

impl UpdateCheck {
    pub fn run(self) -> UpdateCheckResult {
        let result = self.feed.latest();
        UpdateCheckResult {
            at: SystemTime::now(),
            result,
        }
    }
}

/// Installs a downloaded update. Run it off the UI thread: it reads the
/// whole file again. On success, Bardo quits at once, so the installer can
/// replace it.
#[must_use = "nothing is installed until it runs"]
pub struct UpdateInstall {
    installers: Arc<dyn InstallerStore>,
    runner: Arc<dyn InstallerRunner>,
    name: String,
    sha256: String,
    version: Version,
}

impl UpdateInstall {
    /// The version being installed.
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// Checks the file once more, then starts the installer.
    pub fn run(self) -> Result<Version, UpdateError> {
        let Some(mut file) = self
            .installers
            .open(&self.name)
            .map_err(UpdateError::NotStarted)?
        else {
            return Err(UpdateError::NotDownloaded);
        };
        let digest = sha256_of(&mut file).map_err(UpdateError::NotStarted)?;
        drop(file);
        if digest != self.sha256 {
            tracing::warn!(
                version = %self.version,
                "the downloaded installer changed since it was checked; deleting it"
            );
            let _ = self.installers.discard(&self.name);
            return Err(UpdateError::Checksum);
        }
        self.runner
            .run(&self.installers.path(&self.name), &INSTALLER_ARGUMENTS)
            .map_err(UpdateError::NotStarted)?;
        tracing::info!(version = %self.version, "started the installer of an update");
        Ok(self.version)
    }
}

fn sha256_of(reader: &mut dyn Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The download job's payload: what to fetch, as the check found it.
#[derive(Debug, Serialize, Deserialize)]
struct DownloadPayload {
    version: String,
    name: String,
    url: String,
    size: u64,
    /// GitHub's own SHA-256 of the installer, when it gave one.
    #[serde(default)]
    sha256: Option<String>,
    checksum_url: String,
}

/// What the checkpoint of a finished download holds: the SHA-256 the file
/// was checked against, checked again before it runs.
const CHECKPOINT_PREFIX: &str = "sha256:";

/// Hashes what it writes.
struct Hashing<'a> {
    inner: &'a mut dyn Write,
    hasher: Sha256,
}

impl Write for Hashing<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn download_failure(error: DownloadError) -> JobFailure {
    match error {
        DownloadError::Unreachable(detail) => {
            JobFailure::new(JobFailureKind::ProviderUnavailable, detail)
        }
        DownloadError::Write(detail) => JobFailure::new(JobFailureKind::Files, detail),
        error => JobFailure::new(JobFailureKind::UnexpectedAnswer, error.to_string()),
    }
}

/// Downloads an update's installer (`JobKind::UpdateDownload`).
pub(crate) struct UpdateDownloadHandler {
    pub(crate) feed: Arc<dyn ReleaseFeed>,
    pub(crate) installers: Arc<dyn InstallerStore>,
}

impl JobHandler for UpdateDownloadHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: DownloadPayload = parse(payload)?;
        let version: Version = payload
            .version
            .parse()
            .map_err(|_| JobFailure::unexpected("the update's version does not read"))?;
        if payload.name != installer_name(&version) {
            return Err(JobFailure::unexpected("not the installer of this version"));
        }
        if payload.size > MAX_INSTALLER_BYTES {
            return Err(JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("the installer is {} bytes", payload.size),
            ));
        }
        let text = self
            .feed
            .text(&payload.checksum_url, MAX_CHECKSUM_BYTES)
            .map_err(download_failure)?;
        let expected = checksum_for(&text, &payload.name).ok_or_else(|| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                "the checksum file does not give the installer's SHA-256",
            )
        })?;
        if payload
            .sha256
            .as_deref()
            .is_some_and(|digest| digest != expected)
        {
            return Err(JobFailure::new(
                JobFailureKind::Checksum,
                "GitHub's digest of the installer differs from its checksum file",
            ));
        }
        cx.save_checkpoint(format!("{CHECKPOINT_PREFIX}{expected}"), Progress::ZERO)
            .map_err(|error| JobFailure::unexpected(error.to_string()))?;

        let downloaded = {
            let mut file = self
                .installers
                .begin(&payload.name)
                .map_err(|error| JobFailure::new(JobFailureKind::Files, error.to_string()))?;
            let mut hashing = Hashing {
                inner: &mut *file,
                hasher: Sha256::new(),
            };
            let cx = &*cx;
            let size = payload.size;
            let written = self
                .feed
                .download(&payload.url, &mut hashing, size, &mut |done| {
                    cx.report_progress(Progress::of(done, size));
                    !cx.should_stop()
                });
            written.map(|written| (written, hex(&hashing.hasher.finalize())))
        };
        let discard = || {
            if let Err(error) = self.installers.discard(&payload.name) {
                tracing::warn!("could not delete a partial update: {error}");
            }
        };
        let (written, digest) = match downloaded {
            Ok(done) => done,
            Err(DownloadError::Stopped) => {
                discard();
                return Ok(());
            }
            Err(error) => {
                discard();
                return Err(download_failure(error));
            }
        };
        if written != payload.size {
            discard();
            return Err(JobFailure::new(
                JobFailureKind::ProviderUnavailable,
                format!("{written} of {} bytes arrived", payload.size),
            ));
        }
        if digest != expected {
            discard();
            return Err(JobFailure::new(
                JobFailureKind::Checksum,
                format!("expected SHA-256 {expected}, the file has {digest}"),
            ));
        }
        self.installers
            .finish(&payload.name)
            .map_err(|error| JobFailure::new(JobFailureKind::Files, error.to_string()))?;
        tracing::info!(%version, "downloaded and checked an update");
        Ok(())
    }
}

/// The version a download job fetches.
fn download_version(job: &Job) -> Option<Version> {
    serde_json::from_str::<DownloadPayload>(job.payload())
        .ok()?
        .version
        .parse()
        .ok()
}

impl Bardo {
    /// A check for a newer release, unless one is under way, or this copy
    /// does not check (the Store's). `by_hand` checks even when the user
    /// turned the automatic checks off; the background agent never checks.
    pub fn update_check(&mut self, by_hand: bool) -> Option<UpdateCheck> {
        let updates = &mut self.updates;
        if self.role != RunnerRole::App
            || updates.installation == Installation::Store
            || updates.checking
            || (!by_hand && !self.profile.update_check)
        {
            return None;
        }
        updates.checking = true;
        Some(UpdateCheck {
            feed: Arc::clone(&updates.feed),
        })
    }

    /// Keeps what a check found.
    pub fn record_update_check(&mut self, check: UpdateCheckResult) {
        let updates = &mut self.updates;
        updates.checking = false;
        let found = match check.result {
            Ok(release) if release.version > updates.current => {
                tracing::info!(version = %release.version, "a newer Bardo is out");
                updates.newer = Some(release);
                UpdateFound::Newer
            }
            Ok(_) => {
                updates.newer = None;
                UpdateFound::UpToDate
            }
            Err(UpdateCheckError::NoRelease) => {
                updates.newer = None;
                UpdateFound::NoRelease
            }
            // What an earlier check found still holds.
            Err(UpdateCheckError::Offline(detail)) => {
                tracing::info!("could not check for updates: {detail}");
                UpdateFound::Offline
            }
            Err(UpdateCheckError::RateLimited { until }) => {
                tracing::info!("GitHub asked to wait before checking for updates");
                UpdateFound::RateLimited(until)
            }
            Err(UpdateCheckError::Unexpected(detail)) => {
                tracing::warn!("could not check for updates: {detail}");
                UpdateFound::Failed
            }
        };
        updates.last_check = Some(UpdateLastCheck {
            at: check.at,
            found,
        });
    }

    /// The version the banner over the screens names, when it shows. Cheap
    /// enough for every frame, unlike `update_view`.
    pub fn update_banner(&self) -> Option<Version> {
        let updates = &self.updates;
        if updates.installation == Installation::Store {
            return None;
        }
        updates
            .newer
            .as_ref()
            .filter(|release| updates.put_off.as_ref() != Some(&release.version))
            .map(|release| release.version.clone())
    }

    /// Hides the banner until Bardo opens again, or a newer release.
    pub fn put_off_update(&mut self) {
        self.updates.put_off = self
            .updates
            .newer
            .as_ref()
            .map(|release| release.version.clone());
    }

    /// Turns the automatic checks on or off.
    pub fn set_update_check(&mut self, on: bool) -> Result<(), AppError> {
        if on == self.profile.update_check {
            return Ok(());
        }
        let updated = UserProfile {
            update_check: on,
            ..self.profile.clone()
        };
        self.profiles.save(&updated)?;
        self.profile = updated;
        Ok(())
    }

    /// Updates as of now.
    pub fn update_view(&self) -> UpdateView {
        let updates = &self.updates;
        let available = updates.newer.as_ref().map(|release| {
            let jobs = self.jobs();
            let download = jobs
                .iter()
                .rev()
                .find(|job| {
                    job.kind() == JobKind::UpdateDownload
                        && download_version(job).as_ref() == Some(&release.version)
                })
                .cloned();
            let ready = download.as_ref().is_some_and(|job| {
                job.state() == JobState::Done
                    && updates.installers.exists(&installer_name(&release.version))
            });
            AvailableUpdate {
                installable: updates.installation == Installation::Installer
                    && release.installer().is_some()
                    && release.checksum().is_some(),
                install_block: ready.then(|| self.install_block(&jobs)).flatten(),
                release: release.clone(),
                download,
                ready,
            }
        });
        let banner = self.update_banner().is_some();
        UpdateView {
            installation: updates.installation,
            current: updates.current.clone(),
            check_on: self.profile.update_check,
            checking: updates.checking,
            last_check: updates.last_check,
            available,
            banner,
        }
    }

    fn install_block(&self, jobs: &[Job]) -> Option<InstallBlock> {
        if crate::backup::another_window() {
            return Some(InstallBlock::OtherWindow);
        }
        if self.restart_needed()
            || jobs
                .iter()
                .any(|job| job.kind() == JobKind::Restore && job.state().is_active())
        {
            return Some(InstallBlock::Restoring);
        }
        // The installer closes the background agent too; what it runs goes
        // back to the queue through its lease, as at any close.
        let running = jobs
            .iter()
            .filter(|job| job.state() == JobState::Running && !self.job_runs_elsewhere(job.id()))
            .count();
        (running > 0).then_some(InstallBlock::JobsRunning(running))
    }

    /// Downloads the newer release's installer, as a job.
    pub fn start_update_download(&self) -> Result<JobId, UpdateError> {
        let view = self.update_view();
        let available = view.available.ok_or(UpdateError::NotAvailable)?;
        if !available.installable {
            return Err(UpdateError::NotInstallable);
        }
        if available.downloading().is_some() {
            return Err(UpdateError::Downloading);
        }
        let release = &available.release;
        let (Some(installer), Some(checksum)) = (release.installer(), release.checksum()) else {
            return Err(UpdateError::NotInstallable);
        };
        let payload = DownloadPayload {
            version: release.version.to_string(),
            name: installer.name.clone(),
            url: installer.url.clone(),
            size: installer.size,
            sha256: installer.sha256.clone(),
            checksum_url: checksum.url.clone(),
        };
        let job = Job::new(self.profile.id, JobKind::UpdateDownload, to_json(&payload));
        let id = self.jobs.enqueue(job)?;
        tracing::info!(version = %release.version, "downloading an update");
        Ok(id)
    }

    /// The downloaded update, ready to install once the user confirmed.
    pub fn install_update(&self) -> Result<UpdateInstall, UpdateError> {
        let view = self.update_view();
        let available = view.available.ok_or(UpdateError::NotAvailable)?;
        if !available.installable {
            return Err(UpdateError::NotInstallable);
        }
        if !available.ready {
            return Err(UpdateError::NotDownloaded);
        }
        if let Some(block) = available.install_block {
            return Err(UpdateError::Blocked(block));
        }
        let sha256 = available
            .download
            .as_ref()
            .and_then(Job::checkpoint)
            .and_then(|checkpoint| checkpoint.strip_prefix(CHECKPOINT_PREFIX))
            .filter(|hex| bardo_domain::is_sha256(hex))
            .ok_or(UpdateError::NotDownloaded)?
            .to_owned();
        let version = available.release.version;
        Ok(UpdateInstall {
            installers: Arc::clone(&self.updates.installers),
            runner: Arc::clone(&self.updates.runner),
            name: installer_name(&version),
            sha256,
            version,
        })
    }

    /// Deletes installers this copy no longer needs: of versions at or
    /// below its own (installed already), and unfinished ones no download
    /// is writing. Only Bardo's window, alone, tidies them.
    pub(crate) fn prune_installers(&self) {
        if self.role != RunnerRole::App || crate::backup::another_window() {
            return;
        }
        let downloading = self
            .jobs()
            .iter()
            .any(|job| job.kind() == JobKind::UpdateDownload && job.state().is_active());
        let current = &self.updates.current;
        let keep = |name: &str| installer_version(name).is_some_and(|version| version > *current);
        if let Err(error) = self.updates.installers.prune(&keep, downloading) {
            tracing::warn!("could not tidy the downloaded updates: {error}");
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use bardo_domain::{ReleaseAsset, is_sha256};

    use super::*;

    pub(crate) const INSTALLER_URL: &str = "https://github.com/fernandolisboa/bardo/releases/download/v9.0.0/Bardo-Setup-9.0.0-x64.exe";

    /// A release feed in memory: `latest` answers the release set, else
    /// that nothing is published; files come from `files`.
    #[derive(Default)]
    pub(crate) struct FakeReleases {
        pub(crate) latest: Mutex<Option<Result<Release, UpdateCheckError>>>,
        pub(crate) files: Mutex<HashMap<String, Result<Vec<u8>, DownloadError>>>,
        pub(crate) checks: AtomicUsize,
        pub(crate) downloads: AtomicUsize,
        /// Downloads hold on, reporting progress, until told to stop.
        pub(crate) hang: AtomicBool,
    }

    impl FakeReleases {
        /// Publishes `version` with an installer of `bytes` and its
        /// checksum file, both as `cargo xtask package` writes them.
        pub(crate) fn publish(&self, version: &str, bytes: &[u8]) -> Release {
            let version: Version = version.parse().unwrap();
            let name = installer_name(&version);
            let url = format!(
                "https://github.com/fernandolisboa/bardo/releases/download/v{version}/{name}"
            );
            let sha256 = hex(&Sha256::digest(bytes));
            let checksum = format!("{sha256}  {name}\n");
            let release = Release {
                version: version.clone(),
                page: format!("https://github.com/fernandolisboa/bardo/releases/tag/v{version}"),
                notes: "## What's Changed\n* Faster renders".into(),
                published_at: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000)),
                assets: vec![
                    ReleaseAsset {
                        name: name.clone(),
                        url: url.clone(),
                        size: bytes.len() as u64,
                        sha256: Some(sha256),
                    },
                    ReleaseAsset {
                        name: format!("{name}.sha256"),
                        url: format!("{url}.sha256"),
                        size: checksum.len() as u64,
                        sha256: None,
                    },
                ],
            };
            let mut files = self.files.lock().unwrap();
            files.insert(url.clone(), Ok(bytes.to_vec()));
            files.insert(format!("{url}.sha256"), Ok(checksum.into_bytes()));
            *self.latest.lock().unwrap() = Some(Ok(release.clone()));
            release
        }

        pub(crate) fn answer(&self, result: Result<Release, UpdateCheckError>) {
            *self.latest.lock().unwrap() = Some(result);
        }

        fn file(&self, url: &str) -> Result<Vec<u8>, DownloadError> {
            self.files
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .unwrap_or_else(|| Err(DownloadError::Unexpected("answered 404".into())))
        }
    }

    impl ReleaseFeed for FakeReleases {
        fn latest(&self) -> Result<Release, UpdateCheckError> {
            self.checks.fetch_add(1, Ordering::SeqCst);
            self.latest
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(Err(UpdateCheckError::NoRelease))
        }

        fn text(&self, url: &str, _max_bytes: u64) -> Result<String, DownloadError> {
            Ok(String::from_utf8(self.file(url)?).unwrap())
        }

        fn download(
            &self,
            url: &str,
            to: &mut dyn Write,
            max_bytes: u64,
            progress: &mut dyn FnMut(u64) -> bool,
        ) -> Result<u64, DownloadError> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            let bytes = self.file(url)?;
            let half = bytes.len() / 2;
            to.write_all(&bytes[..half])
                .map_err(|e| DownloadError::Write(e.to_string()))?;
            while self.hang.load(Ordering::SeqCst) {
                if !progress(half as u64) {
                    return Err(DownloadError::Stopped);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            if bytes.len() as u64 > max_bytes {
                return Err(DownloadError::TooBig(max_bytes));
            }
            to.write_all(&bytes[half..])
                .map_err(|e| DownloadError::Write(e.to_string()))?;
            progress(bytes.len() as u64);
            Ok(bytes.len() as u64)
        }
    }

    #[test]
    fn the_fake_publishes_what_the_package_writes() {
        let fake = FakeReleases::default();
        let release = fake.publish("9.0.0", b"MZ");
        assert_eq!(release.installer().unwrap().url, INSTALLER_URL);
        assert!(is_sha256(
            release.installer().unwrap().sha256.as_ref().unwrap()
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use bardo_storage::{MemoryInstallerRunner, MemoryInstallers};

    use super::testing::{FakeReleases, INSTALLER_URL};
    use super::*;
    use crate::scenes::tests::{Harness, done, wait_done};

    const INSTALLER: &[u8] = b"MZ the installer of Bardo 9.0.0";
    const NAME: &str = "Bardo-Setup-9.0.0-x64.exe";

    struct Setup {
        harness: Harness,
        releases: Arc<FakeReleases>,
        installers: MemoryInstallers,
        runner: MemoryInstallerRunner,
    }

    impl Setup {
        fn new() -> Self {
            let releases = Arc::new(FakeReleases::default());
            let installers = MemoryInstallers::default();
            let runner = MemoryInstallerRunner::default();
            let mut harness = Harness::new();
            harness.releases = Arc::clone(&releases);
            harness.installers = Arc::new(installers.clone());
            harness.installer_runner = Arc::new(runner.clone());
            Self {
                harness,
                releases,
                installers,
                runner,
            }
        }

        /// Bardo as the installer put it on a Windows computer.
        fn start(&self) -> Bardo {
            let mut app = self.harness.start();
            app.updates.installation = Installation::Installer;
            app
        }
    }

    fn check(app: &mut Bardo, by_hand: bool) -> Option<UpdateFound> {
        let check = app.update_check(by_hand)?;
        let result = check.run();
        app.record_update_check(result);
        Some(app.update_view().last_check.unwrap().found)
    }

    fn downloaded(setup: &Setup, app: &mut Bardo) -> Job {
        setup.releases.publish("9.0.0", INSTALLER);
        check(app, false);
        done(app, app.start_update_download().unwrap())
    }

    #[test]
    fn installations_are_told_apart_by_where_bardo_runs() {
        assert_eq!(
            Installation::of(Path::new(
                r"C:\Program Files\WindowsApps\Bardo_0.2.0.0_x64__abc\bardo.exe"
            )),
            if cfg!(windows) {
                Installation::Store
            } else {
                // Backslashes are no separators outside Windows.
                Installation::Source
            }
        );
        assert_eq!(
            Installation::of(Path::new("/opt/WindowsApps/Bardo/bardo.exe")),
            Installation::Store
        );
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("bardo.exe");
        assert_eq!(Installation::of(&program), Installation::Source);
        std::fs::write(dir.path().join(crate::about::NOTICES_FILE), "notices").unwrap();
        assert_eq!(
            Installation::of(&program),
            if cfg!(windows) {
                Installation::Installer
            } else {
                Installation::Source
            }
        );
    }

    #[test]
    fn the_current_version_is_the_workspace_version() {
        assert_eq!(current_version().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn a_newer_release_shows_the_banner_and_an_older_one_does_not() {
        let setup = Setup::new();
        let mut app = setup.start();
        let view = app.update_view();
        assert!(view.check_on, "on until turned off");
        assert_eq!(view.last_check, None);
        assert!(!view.banner);

        setup.releases.publish("9.0.0", INSTALLER);
        assert_eq!(check(&mut app, false), Some(UpdateFound::Newer));
        let view = app.update_view();
        assert!(view.banner);
        assert!(!view.checking);
        let available = view.available.unwrap();
        assert_eq!(available.release.version, Version::new(9, 0, 0));
        assert!(available.installable);
        assert!(!available.ready);

        setup.releases.publish(env!("CARGO_PKG_VERSION"), INSTALLER);
        assert_eq!(check(&mut app, false), Some(UpdateFound::UpToDate));
        let view = app.update_view();
        assert!(!view.banner);
        assert_eq!(view.available, None);
    }

    #[test]
    fn a_failed_check_keeps_what_an_earlier_one_found() {
        let setup = Setup::new();
        let mut app = setup.start();
        setup.releases.publish("9.0.0", INSTALLER);
        check(&mut app, false);
        for (answer, found) in [
            (
                UpdateCheckError::Offline("dns".into()),
                UpdateFound::Offline,
            ),
            (
                UpdateCheckError::RateLimited { until: None },
                UpdateFound::RateLimited(None),
            ),
            (
                UpdateCheckError::Unexpected("?".into()),
                UpdateFound::Failed,
            ),
        ] {
            setup.releases.answer(Err(answer));
            assert_eq!(check(&mut app, false), Some(found));
            assert!(app.update_view().banner, "{found:?}");
        }
        setup.releases.answer(Err(UpdateCheckError::NoRelease));
        assert_eq!(check(&mut app, false), Some(UpdateFound::NoRelease));
        assert!(!app.update_view().banner, "the release was withdrawn");
    }

    #[test]
    fn checks_by_themselves_stop_when_turned_off_but_by_hand_still_run() {
        let setup = Setup::new();
        let mut app = setup.start();
        app.set_update_check(false).unwrap();
        assert!(!app.update_view().check_on);
        assert!(app.update_check(false).is_none());
        assert_eq!(check(&mut app, true), Some(UpdateFound::NoRelease));
        assert_eq!(setup.releases.checks.load(Ordering::SeqCst), 1);

        // Kept across a restart.
        drop(app);
        let app = setup.start();
        assert!(!app.update_view().check_on);
    }

    #[test]
    fn one_check_runs_at_a_time() {
        let setup = Setup::new();
        let mut app = setup.start();
        let first = app.update_check(true).unwrap();
        assert!(app.update_view().checking);
        assert!(app.update_check(true).is_none());
        app.record_update_check(first.run());
        assert!(app.update_check(true).is_some());
    }

    #[test]
    fn a_store_copy_never_checks_and_a_source_build_cannot_install() {
        let setup = Setup::new();
        let mut app = setup.start();
        setup.releases.publish("9.0.0", INSTALLER);
        app.updates.installation = Installation::Store;
        assert!(app.update_check(true).is_none());
        assert!(!app.update_view().banner);

        app.updates.installation = Installation::Source;
        assert_eq!(check(&mut app, false), Some(UpdateFound::Newer));
        let view = app.update_view();
        assert!(view.banner, "a source build hears of it");
        assert!(!view.available.unwrap().installable);
        assert!(matches!(
            app.start_update_download(),
            Err(UpdateError::NotInstallable)
        ));
    }

    #[test]
    fn the_agent_never_checks() {
        let setup = Setup::new();
        let mut app = setup.start();
        app.set_background_agent(true).unwrap().run().unwrap();
        let mut agent = setup.harness.start_agent().expect("the agent is on");
        assert!(agent.update_check(true).is_none());
    }

    #[test]
    fn only_this_repositorys_release_page_is_linked() {
        let setup = Setup::new();
        let mut app = setup.start();
        let mut release = setup.releases.publish("9.0.0", INSTALLER);
        check(&mut app, false);
        let available = app.update_view().available.unwrap();
        assert_eq!(
            available.release_page(),
            Some("https://github.com/fernandolisboa/bardo/releases/tag/v9.0.0")
        );
        for page in [
            "https://example.test/releases/tag/v9.0.0",
            "http://github.com/fernandolisboa/bardo/releases/tag/v9.0.0",
            "https://github.com/fernandolisboa/bardo/releases/",
            "https://github.com/fernandolisboa/bardo/releases/../../evil",
            "",
        ] {
            release.page = page.into();
            setup.releases.answer(Ok(release.clone()));
            check(&mut app, false);
            assert_eq!(
                app.update_view().available.unwrap().release_page(),
                None,
                "{page}"
            );
        }
    }

    #[test]
    fn putting_the_banner_off_lasts_until_a_newer_release() {
        let setup = Setup::new();
        let mut app = setup.start();
        setup.releases.publish("9.0.0", INSTALLER);
        check(&mut app, false);
        assert_eq!(app.update_banner(), Some(Version::new(9, 0, 0)));
        app.put_off_update();
        let view = app.update_view();
        assert!(!view.banner);
        assert_eq!(app.update_banner(), None);
        assert!(view.available.is_some(), "About still shows it");
        check(&mut app, false);
        assert!(!app.update_view().banner);

        setup.releases.publish("9.1.0", INSTALLER);
        check(&mut app, false);
        assert!(app.update_view().banner);
    }

    #[test]
    fn a_release_without_its_installer_or_checksum_cannot_be_downloaded() {
        let setup = Setup::new();
        let mut app = setup.start();
        let mut release = setup.releases.publish("9.0.0", INSTALLER);
        release
            .assets
            .retain(|asset| !asset.name.ends_with(".sha256"));
        setup.releases.answer(Ok(release));
        check(&mut app, false);
        assert!(!app.update_view().available.unwrap().installable);
        assert!(matches!(
            app.start_update_download(),
            Err(UpdateError::NotInstallable)
        ));
        assert!(matches!(
            Setup::new().start().start_update_download(),
            Err(UpdateError::NotAvailable)
        ));
    }

    #[test]
    fn the_installer_downloads_checked_against_its_release() {
        let setup = Setup::new();
        let mut app = setup.start();
        let job = downloaded(&setup, &mut app);
        assert_eq!(job.kind(), JobKind::UpdateDownload);
        assert_eq!(setup.installers.names(), vec![NAME.to_owned()]);
        let view = app.update_view().available.unwrap();
        assert!(view.ready);
        assert_eq!(view.install_block, None);
        assert_eq!(view.download.unwrap().id(), job.id());
    }

    #[test]
    fn a_file_that_does_not_match_its_checksum_is_deleted() {
        let setup = Setup::new();
        let mut app = setup.start();
        setup.releases.publish("9.0.0", INSTALLER);
        check(&mut app, false);
        // The file changed on the way, but its size did not.
        let mut tampered = INSTALLER.to_vec();
        tampered[3] ^= 1;
        setup
            .releases
            .files
            .lock()
            .unwrap()
            .insert(INSTALLER_URL.into(), Ok(tampered));
        let job = wait_done(&app, app.start_update_download().unwrap());
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::Checksum);
        assert!(setup.installers.names().is_empty(), "nothing kept");
        let view = app.update_view().available.unwrap();
        assert!(!view.ready);
        assert!(view.failed_download().is_some());
        assert!(matches!(
            app.install_update(),
            Err(UpdateError::NotDownloaded)
        ));
    }

    #[test]
    fn a_checksum_file_that_disagrees_with_github_stops_before_downloading() {
        let setup = Setup::new();
        let mut app = setup.start();
        setup.releases.publish("9.0.0", INSTALLER);
        check(&mut app, false);
        let other = format!("{}  {NAME}\n", "0".repeat(64));
        setup
            .releases
            .files
            .lock()
            .unwrap()
            .insert(format!("{INSTALLER_URL}.sha256"), Ok(other.into_bytes()));
        let job = wait_done(&app, app.start_update_download().unwrap());
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::Checksum);
        assert_eq!(setup.releases.downloads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_checksum_file_without_the_installer_is_an_unexpected_answer() {
        let setup = Setup::new();
        let mut app = setup.start();
        setup.releases.publish("9.0.0", INSTALLER);
        check(&mut app, false);
        let other = format!("{}  another.exe\n", "0".repeat(64));
        setup
            .releases
            .files
            .lock()
            .unwrap()
            .insert(format!("{INSTALLER_URL}.sha256"), Ok(other.into_bytes()));
        let job = wait_done(&app, app.start_update_download().unwrap());
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::UnexpectedAnswer
        );
    }

    #[test]
    fn a_download_cut_short_is_retried_and_leaves_nothing_behind() {
        let setup = Setup::new();
        let mut app = setup.start();
        setup.releases.publish("9.0.0", INSTALLER);
        check(&mut app, false);
        setup.releases.files.lock().unwrap().insert(
            INSTALLER_URL.into(),
            Err(DownloadError::Unreachable("connection reset".into())),
        );
        let job = wait_done(&app, app.start_update_download().unwrap());
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::ProviderUnavailable
        );
        assert_eq!(job.attempts(), 2, "retried once in the harness");
        assert!(setup.installers.names().is_empty());
    }

    #[test]
    fn a_file_shorter_than_announced_is_not_kept() {
        let setup = Setup::new();
        let mut app = setup.start();
        let mut release = setup.releases.publish("9.0.0", INSTALLER);
        release.assets[0].size += 10;
        setup.releases.answer(Ok(release));
        check(&mut app, false);
        let job = wait_done(&app, app.start_update_download().unwrap());
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::ProviderUnavailable
        );
        assert!(setup.installers.names().is_empty());
    }

    #[test]
    fn a_cancelled_download_leaves_nothing_behind() {
        let setup = Setup::new();
        let mut app = setup.start();
        setup.releases.publish("9.0.0", INSTALLER);
        setup.releases.hang.store(true, Ordering::SeqCst);
        check(&mut app, false);
        let id = app.start_update_download().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while setup.releases.downloads.load(Ordering::SeqCst) == 0 {
            assert!(std::time::Instant::now() < deadline, "never started");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(app.update_view().available.unwrap().downloading().is_some());
        assert!(matches!(
            app.start_update_download(),
            Err(UpdateError::Downloading)
        ));
        app.cancel_job(id).unwrap();
        let job = wait_done(&app, id);
        assert_eq!(job.state(), JobState::Cancelled);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !setup.installers.names().is_empty() {
            assert!(std::time::Instant::now() < deadline, "the part stayed");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn installing_checks_the_file_again_then_runs_the_installer_silently() {
        let setup = Setup::new();
        let mut app = setup.start();
        downloaded(&setup, &mut app);
        let install = app.install_update().unwrap();
        assert_eq!(install.version(), &Version::new(9, 0, 0));
        assert_eq!(install.run().unwrap(), Version::new(9, 0, 0));
        assert_eq!(
            setup.runner.runs(),
            vec![(
                std::path::PathBuf::from("memory").join(NAME),
                INSTALLER_ARGUMENTS.map(str::to_owned).to_vec()
            )]
        );
    }

    #[test]
    fn a_file_changed_after_its_download_is_deleted_and_never_runs() {
        let setup = Setup::new();
        let mut app = setup.start();
        downloaded(&setup, &mut app);
        setup.installers.put(NAME, b"MZ something else");
        let install = app.install_update().unwrap();
        assert!(matches!(install.run(), Err(UpdateError::Checksum)));
        assert!(setup.runner.runs().is_empty());
        assert!(setup.installers.names().is_empty());
        assert!(!app.update_view().available.unwrap().ready);
    }

    #[test]
    fn installing_waits_for_jobs_running_here() {
        let setup = Setup::new();
        let mut app = setup.start();
        downloaded(&setup, &mut app);
        let id = app
            .start_test_job(crate::TestJob {
                steps: 1000,
                step: Duration::from_millis(20),
                fail_at_step: None,
            })
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !app
            .jobs()
            .iter()
            .any(|job| job.id() == id && job.state() == JobState::Running)
        {
            assert!(std::time::Instant::now() < deadline, "never ran");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            app.update_view().available.unwrap().install_block,
            Some(InstallBlock::JobsRunning(1))
        );
        assert!(matches!(
            app.install_update(),
            Err(UpdateError::Blocked(InstallBlock::JobsRunning(1)))
        ));
        app.cancel_job(id).unwrap();
        wait_done(&app, id);
        assert!(app.install_update().is_ok());
    }

    #[test]
    fn old_installers_are_deleted_and_newer_ones_kept_at_start() {
        let setup = Setup::new();
        let installers = &setup.installers;
        installers.put("Bardo-Setup-0.0.1-x64.exe", b"old");
        installers.put(
            &format!("Bardo-Setup-{}-x64.exe", env!("CARGO_PKG_VERSION")),
            b"installed",
        );
        installers.put(NAME, b"newer");
        installers.put("Bardo-Setup-9.1.0-x64.exe.part", b"cut");
        installers.put("notes.txt", b"?");
        let _app = setup.start();
        assert_eq!(installers.names(), vec![NAME.to_owned()]);
    }

    #[test]
    fn the_jobs_panel_names_the_download() {
        let catalog = crate::Catalog::load(bardo_domain::UiLanguage::EnUs);
        assert_eq!(
            catalog.get(Text::JobKindName(JobKind::UpdateDownload)),
            "Update download"
        );
    }
}
