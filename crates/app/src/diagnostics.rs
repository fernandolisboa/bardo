//! Export diagnostics (issue #140): one `.zip` the user saves and attaches
//! to an issue, with Bardo's logs and what is needed to read them (this
//! computer's setup, the job queue, each account's connection state).
//!
//! Nothing leaves the machine. Every byte written passes through the
//! `Redactor`, taught first every secret Bardo holds (keys at start, app
//! credentials and tokens here), and through a mask of the accounts' names
//! and network ids. Gathering reads the database and the secret store, so
//! it happens on the app's thread (`Bardo::diagnostics`); reading the logs
//! and compressing them happen on a background thread
//! (`DiagnosticsExport::write_to`), like a key test.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bardo_domain::{ConnectionStatus, JobState, NetworkConnection, Redactor, UiTheme};

use crate::{Bardo, ConnectionState};

/// The logs an export carries, by file name in the logs folder: the app's
/// and the background agent's, each with its rotated copy.
pub const LOG_FILES: [&str; 4] = ["bardo.log", "bardo.log.1", "agent.log", "agent.log.1"];

/// A log longer than this goes in by its end only, cut at a line, so the
/// export stays small enough to attach to an issue.
pub const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;

/// Failed jobs listed with their reasons, the newest ones.
pub const FAILURES_LISTED: usize = 20;

/// A failure's detail is cut to this many characters.
const DETAIL_CHARS: usize = 300;

/// What replaces an account's name or network id.
pub const ACCOUNT_MASK: &str = "[account]";

/// What replaces the user's home folder in paths.
const HOME_MASK: &str = "%USERPROFILE%";

/// What the window knows and `Bardo` does not: the graphics adapter and the
/// theme on screen now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayInfo {
    /// The adapter the window renders with, when the platform reports it.
    pub gpu: Option<GpuInfo>,
    /// How the window draws, e.g. `DirectX 11`.
    pub renderer: String,
    /// The theme in use (a "follow the system" preference resolved).
    pub theme: UiTheme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuInfo {
    pub adapter: String,
    pub vendor: String,
    pub driver: String,
    /// A CPU renderer (e.g. Microsoft Basic Render Driver, llvmpipe).
    pub software: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("could not write the diagnostics: {0}")]
pub struct DiagnosticsError(#[from] io::Error);

/// Masks what must not leave the machine: every secret the redactor knows,
/// the accounts' names and ids, and the user's home folder.
#[derive(Clone)]
struct Scrubber {
    redactor: Redactor,
    /// Longest first, so a whole name is masked before a part of it.
    accounts: Vec<String>,
    home: Option<String>,
}

impl Scrubber {
    /// Names shorter than this are left alone: masking a two-letter name
    /// would mangle ordinary words.
    const MIN_ACCOUNT_CHARS: usize = 3;

    fn new(redactor: Redactor, accounts: Vec<String>, home: Option<PathBuf>) -> Self {
        let mut accounts: Vec<String> = accounts
            .iter()
            // `@username` also travels as a bare `username` (in addresses).
            .flat_map(|name| [name.trim(), name.trim().trim_start_matches('@')])
            .filter(|name| name.chars().count() >= Self::MIN_ACCOUNT_CHARS)
            .map(str::to_ascii_lowercase)
            .collect();
        accounts.sort_by_key(|name| std::cmp::Reverse(name.len()));
        accounts.dedup();
        let home = home
            .map(|home| {
                home.to_string_lossy()
                    .trim_end_matches(['/', '\\'])
                    .to_owned()
            })
            // A root or empty home would mask every path.
            .filter(|home| home.len() > 3);
        Self {
            redactor,
            accounts,
            home,
        }
    }

    fn scrub(&self, text: &str) -> String {
        let mut text = self.redactor.redact(text);
        for name in &self.accounts {
            text = replace_whole(&text, name, ACCOUNT_MASK);
        }
        if let Some(home) = &self.home
            && text.contains(home.as_str())
        {
            text = text.replace(home.as_str(), HOME_MASK);
        }
        text
    }
}

/// `text` with each whole-word `needle` (lowercase) replaced by `mask`,
/// ignoring ASCII case, since networks do with handles: one that no letter,
/// digit or `_` touches, so an account named `Bar` leaves `Bardo` alone.
fn replace_whole(text: &str, needle: &str, mask: &str) -> String {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    // ASCII lowercasing keeps every byte where it was.
    let lower = text.to_ascii_lowercase();
    if !lower.contains(needle) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for (at, _) in lower.match_indices(needle) {
        if at < copied {
            continue;
        }
        let end = at + needle.len();
        let before = text[..at].chars().next_back();
        let after = text[end..].chars().next();
        if before.is_some_and(word) || after.is_some_and(word) {
            continue;
        }
        out.push_str(&text[copied..at]);
        out.push_str(mask);
        copied = end;
    }
    out.push_str(&text[copied..]);
    out
}

/// An export ready to write: the reports gathered from Bardo, and the logs
/// to read when it is written.
pub struct DiagnosticsExport {
    /// `system.txt`, `jobs.txt`, `accounts.txt`, not yet scrubbed.
    reports: Vec<(&'static str, String)>,
    logs: PathBuf,
    scrubber: Scrubber,
}

impl DiagnosticsExport {
    /// The file names the zip will hold, in order.
    pub fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .reports
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        names.extend(
            LOG_FILES
                .iter()
                .filter(|name| self.logs.join(name).is_file())
                .map(|name| name.to_string()),
        );
        names
    }

    /// Writes the zip to `path`, replacing it. Reads the logs as they are
    /// now, also while the app or the agent appends to them. Blocks on the
    /// disk: run it off the UI thread. A failed write leaves no file.
    pub fn write_to(&self, path: &Path) -> Result<(), DiagnosticsError> {
        let mut partial = path.as_os_str().to_owned();
        partial.push(".part");
        let partial = PathBuf::from(partial);
        let written = self.write_zip(&partial);
        match written.and_then(|()| std::fs::rename(&partial, path)) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = std::fs::remove_file(&partial);
                Err(error.into())
            }
        }
    }

    fn write_zip(&self, path: &Path) -> io::Result<()> {
        let mut zip = zip::ZipWriter::new(File::create(path)?);
        let mut options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        // Zip keeps local time; without it every entry reads 1980.
        let now = jiff::Zoned::now();
        if let Ok(at) = zip::DateTime::from_date_and_time(
            u16::try_from(now.year()).unwrap_or(1980),
            now.month().unsigned_abs(),
            now.day().unsigned_abs(),
            now.hour().unsigned_abs(),
            now.minute().unsigned_abs(),
            now.second().unsigned_abs(),
        ) {
            options = options.last_modified_time(at);
        }
        for (name, text) in &self.reports {
            zip.start_file(*name, options).map_err(io::Error::other)?;
            zip.write_all(self.scrubber.scrub(text).as_bytes())?;
        }
        for name in LOG_FILES {
            let Some(text) = read_log(&self.logs.join(name))? else {
                continue;
            };
            zip.start_file(name, options).map_err(io::Error::other)?;
            zip.write_all(self.scrubber.scrub(&text).as_bytes())?;
        }
        zip.finish().map_err(io::Error::other)?.sync_all()
    }
}

/// The log's text, its last `MAX_LOG_BYTES` from the start of a line when
/// it is longer; `None` when there is no such file. Opened for reading only,
/// so the process still writing it is not disturbed.
fn read_log(path: &Path) -> io::Result<Option<String>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let len = file.metadata()?.len();
    let cut = len > MAX_LOG_BYTES;
    if cut {
        file.seek(SeekFrom::Start(len - MAX_LOG_BYTES))?;
    }
    let mut bytes = Vec::new();
    file.take(MAX_LOG_BYTES).read_to_end(&mut bytes)?;
    if cut {
        // Start at a whole line: a secret cut in half would not be masked.
        let start = bytes
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |at| at + 1);
        bytes.drain(..start);
    }
    let text = String::from_utf8_lossy(&bytes);
    Ok(Some(if cut {
        format!(
            "[the log's first {} bytes were left out]\n{text}",
            len - bytes.len() as u64
        )
    } else {
        text.into_owned()
    }))
}

/// A file name for an export made at `now`, e.g.
/// `bardo-diagnostics-20261013-192416.zip` (UTC).
pub fn diagnostics_file_name(now: SystemTime) -> String {
    let at = jiff::Timestamp::try_from(now)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .strftime("%Y%m%d-%H%M%S");
    format!("bardo-diagnostics-{at}.zip")
}

/// Where the save dialog opens: the Downloads folder, else the home folder.
pub fn diagnostics_folder() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_default()
}

/// The operating system, e.g. `Windows 11 (10.0.26100.4652)`.
#[cfg(windows)]
fn os_version() -> String {
    let version = windows_version::OsVersion::current();
    let name = if windows_version::is_server() {
        "Windows Server"
    } else if version.major == 10 && version.build >= 22000 {
        "Windows 11"
    } else if version.major == 10 {
        "Windows 10"
    } else {
        "Windows"
    };
    format!(
        "{name} ({}.{}.{}.{})",
        version.major,
        version.minor,
        version.build,
        windows_version::revision()
    )
}

#[cfg(not(windows))]
fn os_version() -> String {
    std::env::consts::OS.to_owned()
}

fn timestamp(at: SystemTime) -> String {
    jiff::Timestamp::try_from(at)
        .map(|at| at.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|_| "?".into())
}

/// One line, cut to `DETAIL_CHARS`.
fn one_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(DETAIL_CHARS) {
        Some((at, _)) => format!("{}…", &flat[..at]),
        None => flat,
    }
}

impl Bardo {
    /// Gathers a diagnostic export of this profile with the logs in `logs`
    /// (`logging::logs_dir`). Never fails: what cannot be read is written
    /// as such. Write it with `DiagnosticsExport::write_to`.
    pub fn diagnostics(&self, logs: &Path, display: &DisplayInfo) -> DiagnosticsExport {
        // First: it teaches the redactor the tokens and collects the names
        // the rest is scrubbed of.
        let mut accounts = Vec::new();
        let accounts_report = self.accounts_report(&mut accounts);
        let scrubber = Scrubber::new(self.redactor(), accounts, dirs::home_dir());
        let reports = vec![
            ("system.txt", self.system_report(logs, display)),
            ("jobs.txt", self.jobs_report(&scrubber)),
            ("accounts.txt", accounts_report),
        ];
        DiagnosticsExport {
            reports,
            logs: logs.to_owned(),
            scrubber,
        }
    }

    fn system_report(&self, logs: &Path, display: &DisplayInfo) -> String {
        let about = self.about();
        let profile = &self.profile;
        let mut out = String::from("Bardo diagnostics\n\n");
        let mut line = |label: &str, value: &str| {
            let _ = writeln!(out, "{label}: {value}");
        };
        line("Exported", &timestamp(SystemTime::now()));
        line("Bardo", about.version);
        line("Running as", self.role.code());
        line(
            "FFmpeg",
            &format!(
                "{} ({})",
                about.ffmpeg_version,
                if about.ffmpeg_license.is_some() {
                    "installed with Bardo"
                } else {
                    "development build"
                }
            ),
        );
        line(
            "System",
            &format!("{}, {}", os_version(), std::env::consts::ARCH),
        );
        match &display.gpu {
            Some(gpu) => {
                line("GPU adapter", &gpu.adapter);
                line("GPU vendor", &gpu.vendor);
                line("GPU driver", &gpu.driver);
                if gpu.software {
                    line("GPU", "software renderer");
                }
            }
            None => line("GPU adapter", "not reported"),
        }
        line("Renderer", &display.renderer);
        line("Layout", profile.ui_layout.code());
        line(
            "Theme",
            &format!("{} (setting: {})", display.theme, profile.ui_theme.code()),
        );
        line("Language", profile.ui_language.tag());
        line("Decision engine", self.decision_engine().code());
        line(
            "Database schema",
            &bardo_storage::SCHEMA_VERSION.to_string(),
        );
        line(
            "Background agent",
            match self.background_agent() {
                crate::AgentStatus::Off => "off",
                crate::AgentStatus::Running => "on, running",
                crate::AgentStatus::NotRunning => "on, not running",
            },
        );
        out.push_str("\nLogs:\n");
        for name in LOG_FILES {
            let size = std::fs::metadata(logs.join(name)).map(|meta| meta.len());
            let _ = match size {
                Ok(bytes) => writeln!(out, "- {name}: {bytes} bytes"),
                Err(_) => writeln!(out, "- {name}: none"),
            };
        }
        out
    }

    /// Failure details are scrubbed before they are cut: a secret or name
    /// cut in half would not be masked.
    fn jobs_report(&self, scrubber: &Scrubber) -> String {
        let jobs = self.jobs();
        let mut out = String::from("Jobs\n\n");
        for state in JobState::ALL {
            let count = jobs.iter().filter(|job| job.state() == state).count();
            let _ = writeln!(out, "{}: {count}", state.code());
        }
        let failures: Vec<_> = jobs.iter().filter(|job| job.failure().is_some()).collect();
        let _ = writeln!(
            out,
            "\nLast failures (by when the job was created, newest last, up to {FAILURES_LISTED}):"
        );
        if failures.is_empty() {
            out.push_str("none\n");
        }
        for job in failures.iter().rev().take(FAILURES_LISTED).rev() {
            let Some(failure) = job.failure() else {
                continue;
            };
            let _ = writeln!(
                out,
                "- {} ({}, attempt {}): {}: {}",
                job.kind().code(),
                job.state().code(),
                job.attempts(),
                failure.kind.code(),
                one_line(&scrubber.scrub(&failure.detail))
            );
        }
        out
    }

    /// Each channel's accounts by number and network, with the connection
    /// state. Collects into `names` every name and id to mask.
    fn accounts_report(&self, names: &mut Vec<String>) -> String {
        let mut out = String::from("Network accounts\n\n");
        let channels = match self.channels() {
            Ok(channels) => channels,
            Err(error) => {
                let _ = writeln!(out, "could not read the channels: {error}");
                return out;
            }
        };
        if channels.is_empty() {
            out.push_str("no channels\n");
        }
        let connections = self.connection_book.connections();
        for (number, channel) in channels.iter().enumerate() {
            let number = number + 1;
            let accounts = match self.network_accounts.list(channel.id) {
                Ok(accounts) => accounts,
                Err(error) => {
                    let _ = writeln!(
                        out,
                        "channel {number}: could not read its accounts: {error}"
                    );
                    continue;
                }
            };
            if accounts.is_empty() {
                let _ = writeln!(out, "channel {number}: no accounts");
            }
            for account in accounts {
                names.push(account.details.handle().to_owned());
                let stored = connections.connection_for_diagnostics(&account);
                if let Ok(Some(connection)) = &stored {
                    names.push(connection.identity.id.clone());
                    names.push(connection.identity.name.clone());
                }
                let state = match self.connection_state(&account) {
                    Ok(state) => state_code(&state).to_owned(),
                    Err(error) => format!("unknown ({error})"),
                };
                let _ = write!(
                    out,
                    "channel {number} · {}: {state}",
                    account.network.code()
                );
                match stored {
                    Ok(Some(connection)) => out.push_str(&connection_detail(&connection)),
                    Ok(None) => {}
                    Err(error) => {
                        let _ = write!(out, " (could not read the connection: {error})");
                    }
                }
                out.push('\n');
            }
        }
        out
    }
}

fn state_code(state: &ConnectionState) -> &'static str {
    match state {
        ConnectionState::Unavailable => "export only",
        ConnectionState::NotConnected => "not connected",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Choosing { .. } => "choosing an account",
        ConnectionState::Connected { .. } => "connected",
        ConnectionState::ReconnectNeeded { .. } => "reconnect needed",
    }
}

/// When the connection was made and renewed and when its token expires;
/// never who it is or the token.
fn connection_detail(connection: &NetworkConnection) -> String {
    let status = match connection.status {
        ConnectionStatus::Connected => "",
        ConnectionStatus::ReconnectNeeded => ", refused a renewal",
    };
    format!(
        " (connected {}, renewed {}, token expires {}, {} scopes{status})",
        timestamp(connection.connected_at),
        connection
            .refreshed_at
            .map_or_else(|| "never".to_owned(), timestamp),
        timestamp(connection.expires_at),
        connection.scopes.len(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use bardo_domain::{
        ApiKey, ChannelDraft, ConnectedIdentity, ConnectionSecrets, ContentLanguage, Job,
        JobFailure, JobFailureKind, JobKind, JobRecord, JobRepository, Network,
        NetworkAccountDraft, NetworkConnectionRepository, Provider, SecretText, TokenGrant,
        TokenSet,
    };
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::{Repositories, testing};

    // Fake values, split so secret scanners do not take them for real ones.
    const CLAUDE_KEY: &str = concat!("sk-ant-", "diagnostics-key-0001");
    const CLIENT_ID: &str = concat!("1234567890-diag0001", ".apps.googleusercontent.com");
    const CLIENT_SECRET: &str = concat!("GOCSPX", "-diagnostics-secret-01");
    const ACCESS: &str = concat!("ya29.", "diagnostics-access-0001");
    const REFRESH: &str = concat!("1//", "diagnostics-refresh-0001");
    const CHANNEL_ID: &str = "UCdiagnosticsChannel0001";
    const CHANNEL_NAME: &str = "Arquivos do Espaço";
    const HANDLE: &str = "arquivosdoespaco";

    fn display() -> DisplayInfo {
        DisplayInfo {
            gpu: Some(GpuInfo {
                adapter: "Fake Adapter 3000".into(),
                vendor: "Fake Corporation".into(),
                driver: "1.2.3".into(),
                software: false,
            }),
            renderer: "DirectX 11".into(),
            theme: UiTheme::Graphite,
        }
    }

    struct Planted {
        app: Bardo,
        logs: tempfile::TempDir,
    }

    /// An app with every kind of secret saved, a connected YouTube account,
    /// a failed job whose detail names them, and logs that carry them.
    fn planted() -> Planted {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let keys = Arc::new(MemorySecretStore::default());
        let connection_secrets = Arc::new(MemorySecretStore::default());
        let start = || {
            let repositories = Repositories {
                connection_secrets: Arc::clone(&connection_secrets) as _,
                ..Repositories::shared(Arc::clone(&db), Arc::clone(&keys) as _)
            };
            Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap()
        };
        let mut app = start();
        let owner = app.profile().id;
        app.accept_privacy_policy().unwrap();
        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        app.save_app_credentials(Network::YouTube, CLIENT_ID, CLIENT_SECRET)
            .unwrap();
        let channel = app
            .create_channel(ChannelDraft {
                name: "Space".into(),
                language: ContentLanguage::Portuguese,
                ..ChannelDraft::default()
            })
            .unwrap()
            .id;
        let account = app
            .add_network_account(
                channel,
                Network::YouTube,
                NetworkAccountDraft {
                    handle: HANDLE.into(),
                    ..NetworkAccountDraft::default()
                },
            )
            .unwrap();
        // Saved by an earlier run (or the agent): this run never read them,
        // so only the export can teach them to the redactor.
        let now = SystemTime::now();
        let tokens = TokenSet::granted(
            &TokenGrant {
                access_token: SecretText::new(ACCESS),
                refresh_token: Some(SecretText::new(REFRESH)),
                expires_in: Duration::from_secs(3600),
                scopes: vec!["youtube.upload".into()],
            },
            now,
        );
        connection_secrets
            .set_tokens(owner, account.id, &tokens)
            .unwrap();
        NetworkConnectionRepository::save(
            &*db,
            &NetworkConnection {
                account: account.id,
                owner,
                status: ConnectionStatus::ReconnectNeeded,
                identity: ConnectedIdentity {
                    id: CHANNEL_ID.into(),
                    name: CHANNEL_NAME.into(),
                },
                scopes: vec!["youtube.upload".into()],
                expires_at: now + Duration::from_secs(3600),
                connected_at: now,
                refreshed_at: None,
            },
        )
        .unwrap();
        let failed = Job::restore(JobRecord {
            state: JobState::Failed,
            attempts: 3,
            failure: Some(JobFailure::new(
                JobFailureKind::KeyRejected,
                format!("401 for {ACCESS} on {CHANNEL_ID} ({CHANNEL_NAME})"),
            )),
            ..JobRecord::from(&Job::new(owner, JobKind::Upload, "{}"))
        })
        .unwrap();
        JobRepository::save(&*db, &failed).unwrap();
        // A long detail whose token and channel id straddle the cut.
        let pad = "x".repeat(DETAIL_CHARS - 15);
        let long = Job::restore(JobRecord {
            state: JobState::Failed,
            attempts: 1,
            failure: Some(JobFailure::new(
                JobFailureKind::NotAllowed,
                format!("{pad} {ACCESS} {CHANNEL_ID}"),
            )),
            ..JobRecord::from(&Job::new(owner, JobKind::MetricsSync, "{}"))
        })
        .unwrap();
        JobRepository::save(&*db, &long).unwrap();
        // A restart: the new run knows the keys and app credentials, not
        // the tokens.
        drop(app);
        let app = start();

        let logs = tempfile::tempdir().unwrap();
        let secrets = [
            CLAUDE_KEY,
            CLIENT_ID,
            CLIENT_SECRET,
            ACCESS,
            REFRESH,
            CHANNEL_ID,
            CHANNEL_NAME,
            HANDLE,
        ];
        for (name, extra) in [
            ("bardo.log", "app"),
            ("bardo.log.1", "old"),
            ("agent.log", "agent"),
        ] {
            let mut text = String::new();
            for secret in secrets {
                text.push_str(&format!("INFO {extra}: called with {secret}\n"));
            }
            std::fs::write(logs.path().join(name), text).unwrap();
        }
        Planted { app, logs }
    }

    fn unzip(path: &Path) -> Vec<(String, String)> {
        let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        (0..archive.len())
            .map(|index| {
                let mut entry = archive.by_index(index).unwrap();
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                (entry.name().unwrap().into_owned(), text)
            })
            .collect()
    }

    #[test]
    fn no_planted_secret_or_account_reaches_the_zip() {
        let Planted { app, logs } = planted();
        let out = tempfile::tempdir().unwrap();
        let path = out.path().join("diagnostics.zip");
        app.diagnostics(logs.path(), &display())
            .write_to(&path)
            .unwrap();

        let entries = unzip(&path);
        let names: Vec<_> = entries.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "system.txt",
                "jobs.txt",
                "accounts.txt",
                "bardo.log",
                "bardo.log.1",
                "agent.log"
            ]
        );
        for (name, text) in &entries {
            for secret in [
                CLAUDE_KEY,
                CLIENT_ID,
                CLIENT_SECRET,
                ACCESS,
                REFRESH,
                CHANNEL_ID,
                CHANNEL_NAME,
                HANDLE,
            ] {
                // Nor a part of one left by a cut.
                let part = &secret[..secret.len().min(10)];
                assert!(!text.contains(part), "{part} in {name}:\n{text}");
            }
        }
        let text = |wanted: &str| &entries.iter().find(|(name, _)| name == wanted).unwrap().1;
        assert_eq!(
            text("bardo.log").matches(Redactor::MASK).count(),
            5,
            "{}",
            text("bardo.log")
        );
        assert_eq!(
            text("bardo.log").matches(ACCOUNT_MASK).count(),
            3,
            "{}",
            text("bardo.log")
        );
        assert!(
            text("agent.log").contains("INFO agent"),
            "{}",
            text("agent.log")
        );
        assert!(
            text("jobs.txt").contains("upload (failed, attempt 3): key_rejected: 401 for [redacted] on [account] ([account])"),
            "{}",
            text("jobs.txt")
        );
        assert!(
            text("accounts.txt").contains("channel 1 · youtube: reconnect needed (connected "),
            "{}",
            text("accounts.txt")
        );
    }

    #[test]
    fn the_system_report_says_what_reading_the_logs_needs() {
        let Planted { app, logs } = planted();
        let export = app.diagnostics(logs.path(), &display());
        let system = &export.reports[0].1;
        for expected in [
            format!("Bardo: {}", env!("CARGO_PKG_VERSION")),
            "Running as: app".into(),
            format!("FFmpeg: {}", app.about().ffmpeg_version),
            format!("System: {}", os_version()),
            "GPU adapter: Fake Adapter 3000".into(),
            "GPU driver: 1.2.3".into(),
            "Renderer: DirectX 11".into(),
            "Layout: workspace".into(),
            "Theme: graphite (setting: system:paper:graphite)".into(),
            "Language: en-US".into(),
            "Decision engine: jev".into(),
            format!("Database schema: {}", bardo_storage::SCHEMA_VERSION),
            "Background agent: off".into(),
            "- agent.log.1: none".into(),
        ] {
            assert!(system.contains(&expected), "{expected} missing:\n{system}");
        }
        let jobs = &export.reports[1].1;
        assert!(
            jobs.contains("failed: 2\n") && jobs.contains("done: 0\n"),
            "{jobs}"
        );
        assert_eq!(
            export.entries(),
            [
                "system.txt",
                "jobs.txt",
                "accounts.txt",
                "bardo.log",
                "bardo.log.1",
                "agent.log"
            ]
        );
    }

    #[test]
    fn a_long_log_goes_in_by_its_end_from_a_whole_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.log");
        let line = "INFO a line of the log that repeats\n";
        let count = MAX_LOG_BYTES as usize / line.len() + 10;
        let mut text = String::from("FIRST line\n");
        text.push_str(&line.repeat(count));
        text.push_str("LAST line\n");
        std::fs::write(&path, &text).unwrap();

        let read = read_log(&path).unwrap().unwrap();
        assert!(read.starts_with("[the log's first "), "{}", &read[..80]);
        assert!(!read.contains("FIRST"));
        assert!(read.ends_with("LAST line\n"));
        let body = read.split_once('\n').unwrap().1;
        assert!(body.starts_with("INFO a line"), "{}", &body[..40]);
        assert!(body.len() as u64 <= MAX_LOG_BYTES);
        assert_eq!(read_log(&dir.path().join("missing.log")).unwrap(), None);
    }

    #[test]
    fn a_log_still_being_written_is_read_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.log");
        let mut writer = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        writer.write_all(b"INFO first\n").unwrap();
        assert_eq!(read_log(&path).unwrap().unwrap(), "INFO first\n");
        writer.write_all(b"INFO second\n").unwrap();
        assert_eq!(
            read_log(&path).unwrap().unwrap(),
            "INFO first\nINFO second\n"
        );
    }

    #[test]
    fn a_failed_write_leaves_no_file() {
        let Planted { app, logs } = planted();
        let out = tempfile::tempdir().unwrap();
        let path = out.path().join("missing-folder").join("diagnostics.zip");
        assert!(
            app.diagnostics(logs.path(), &display())
                .write_to(&path)
                .is_err()
        );
        assert!(!out.path().join("missing-folder").exists());
    }

    #[test]
    fn the_scrubber_masks_whole_account_names_and_home_but_not_short_ones() {
        let redactor = Redactor::new();
        redactor.add(&ApiKey::parse(Provider::Claude, CLAUDE_KEY).unwrap());
        let scrubber = Scrubber::new(
            redactor,
            vec![
                "ab".into(),
                "  space-channel ".into(),
                "space".into(),
                "Bar".into(),
            ],
            Some(PathBuf::from("C:\\Users\\someone\\")),
        );
        assert_eq!(
            scrubber.scrub(&format!(
                "{CLAUDE_KEY} space-channel space ab Bardo (Bar) spaces \
                 C:\\Users\\someone\\AppData"
            )),
            "[redacted] [account] [account] ab Bardo ([account]) spaces \
             %USERPROFILE%\\AppData"
        );
        assert_eq!(
            replace_whole("UCid/ucID?x=UCidX", "ucid", "[a]"),
            "[a]/[a]?x=UCidX"
        );
        let handles = Scrubber::new(Redactor::new(), vec!["@SpaceFacts".into()], None);
        assert_eq!(
            handles.scrub("@spacefacts at instagram.com/SPACEFACTS/"),
            "[account] at instagram.com/[account]/"
        );
        let root = Scrubber::new(Redactor::new(), vec![], Some(PathBuf::from("/")));
        assert_eq!(root.scrub("/var/log"), "/var/log");
    }

    #[test]
    fn file_names_carry_the_time_in_utc() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_919_456);
        assert_eq!(
            diagnostics_file_name(at),
            "bardo-diagnostics-20261013-192416.zip"
        );
    }

    #[test]
    fn details_are_cut_to_one_short_line() {
        assert_eq!(one_line("a\n  b\tc"), "a b c");
        let long = "x".repeat(DETAIL_CHARS + 5);
        let cut = one_line(&long);
        assert_eq!(cut.chars().count(), DETAIL_CHARS + 1);
        assert!(cut.ends_with('…'));
    }
}
