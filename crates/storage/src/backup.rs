//! Backups on this computer (issue #143, ADR-0010): one `.zip` with a
//! manifest, a consistent copy of the database and, when asked, the
//! project folders; and the restore, unpacked beside the data and swapped
//! in at the next start, before the database opens.
//!
//! Nothing here touches a secret: keys, app credentials and tokens live in
//! the system's secret store, never in the database or the folders.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use bardo_domain::{
    BackupError, BackupInfo, BackupOptions, BackupStore, DUE_GRACE, JobFailureKind, MediaSize,
    Network, Progress, ProgressReport, Provider, RestoreOutcome, SavedBackup,
};
use rusqlite::Connection;
use rusqlite::backup::{Backup, StepResult};
use serde::{Deserialize, Serialize};

use crate::{
    BUSY_TIMEOUT, Database, SCHEMA_VERSION, StorageError, from_unix_millis, to_unix_millis,
};

/// The manifest's name in a backup. It goes first.
pub const MANIFEST: &str = "bardo-backup.toml";

/// The database's name in a backup.
pub const DATABASE_ENTRY: &str = "bardo.db";

/// The folder the project folders go under in a backup.
const PROJECTS_ENTRY: &str = "projects";

/// The backup layout this build writes. A higher one is a newer Bardo's.
pub const BACKUP_FORMAT: u32 = 1;

/// How much is copied between two progress reports (and checks for stop).
const CHUNK: usize = 1024 * 1024;

/// How long a file swap at start keeps trying a file another process
/// still has open (the closing app, an agent on its way out).
const SWAP_PATIENCE: Duration = Duration::from_secs(10);

/// Where Bardo's data is, and where backups are staged and kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    /// The database, `%APPDATA%\Bardo\bardo.db`.
    pub database: PathBuf,
    /// The project folders, `%LOCALAPPDATA%\Bardo\projects`.
    pub projects: PathBuf,
    /// `%LOCALAPPDATA%\Bardo`: the staged restore (`restore`), the
    /// automatic backups (`backups`) and the last restore's outcome.
    pub local: PathBuf,
}

impl DataPaths {
    /// Bardo's own folders for this user.
    pub fn local_default() -> Result<Self, StorageError> {
        let local = crate::data_home().local()?;
        Ok(Self {
            database: crate::default_database_path()?,
            projects: crate::default_projects_dir()?,
            local,
        })
    }

    fn staging(&self) -> PathBuf {
        self.local.join("restore")
    }

    fn incoming(&self) -> PathBuf {
        self.staging().join("incoming")
    }

    fn previous(&self) -> PathBuf {
        self.staging().join("previous")
    }

    /// Written last when a restore is staged: its presence means "swap at
    /// the next start".
    fn ready_marker(&self) -> PathBuf {
        self.staging().join("ready.toml")
    }

    /// Written when a swap could not be undone whole: the previous data in
    /// `restore\previous` is the user's, and nothing deletes it.
    fn keep_marker(&self) -> PathBuf {
        self.staging().join("keep.txt")
    }

    /// Where leftovers move to be deleted, so a new restore can stage at
    /// once.
    fn discarded(&self) -> PathBuf {
        self.local.join("restore.old")
    }

    fn window_lock(&self) -> PathBuf {
        self.local.join("window.lock")
    }

    fn automatic_dir(&self) -> PathBuf {
        self.local.join("backups")
    }

    fn restore_record(&self) -> PathBuf {
        self.local.join("restored.toml")
    }
}

/// A backup's `bardo-backup.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    format: u32,
    bardo_version: String,
    schema_version: u32,
    /// Unix milliseconds.
    created_at: i64,
    media: bool,
    automatic: bool,
    database_bytes: u64,
    media_bytes: u64,
    media_files: u64,
    /// Provider and network codes; ones this build does not know are
    /// ignored on read.
    #[serde(default)]
    saved_keys: Vec<String>,
    #[serde(default)]
    saved_credentials: Vec<String>,
}

impl Manifest {
    fn info(&self) -> BackupInfo {
        BackupInfo {
            created_at: from_unix_millis(self.created_at),
            bardo_version: self.bardo_version.clone(),
            schema_version: self.schema_version,
            media: self.media,
            automatic: self.automatic,
            database_bytes: self.database_bytes,
            media_bytes: self.media_bytes,
            media_files: self.media_files,
            saved_keys: self.keys(),
            saved_credentials: self.credentials(),
        }
    }

    fn keys(&self) -> Vec<Provider> {
        parse_codes(&self.saved_keys)
    }

    fn credentials(&self) -> Vec<Network> {
        parse_codes(&self.saved_credentials)
    }

    fn from_info(info: &BackupInfo) -> Self {
        Self {
            format: BACKUP_FORMAT,
            bardo_version: info.bardo_version.clone(),
            schema_version: info.schema_version,
            created_at: to_unix_millis(info.created_at),
            media: info.media,
            automatic: info.automatic,
            database_bytes: info.database_bytes,
            media_bytes: info.media_bytes,
            media_files: info.media_files,
            saved_keys: info
                .saved_keys
                .iter()
                .map(|p| p.code().to_owned())
                .collect(),
            saved_credentials: info
                .saved_credentials
                .iter()
                .map(|n| n.code().to_owned())
                .collect(),
        }
    }

    /// Refuses a backup this build cannot restore.
    fn check(&self) -> Result<(), BackupError> {
        if self.format > BACKUP_FORMAT || self.schema_version as usize > SCHEMA_VERSION {
            return Err(BackupError::Newer {
                bardo_version: self.bardo_version.clone(),
            });
        }
        Ok(())
    }
}

/// The values of `codes` this build knows, in order.
fn parse_codes<T: std::str::FromStr>(codes: &[String]) -> Vec<T> {
    codes.iter().filter_map(|code| code.parse().ok()).collect()
}

/// The last restore's outcome, as `restored.toml` keeps it.
#[derive(Debug, Serialize, Deserialize)]
struct RestoreRecord {
    backup_created_at: i64,
    media: bool,
    restored_at: i64,
    failure: Option<String>,
    #[serde(default)]
    kept_aside: Option<PathBuf>,
    #[serde(default)]
    saved_keys: Vec<String>,
    #[serde(default)]
    saved_credentials: Vec<String>,
}

fn database_error(error: impl std::fmt::Display) -> BackupError {
    BackupError::Database(error.to_string())
}

fn zip_error(error: zip::result::ZipError) -> BackupError {
    match error {
        zip::result::ZipError::Io(error) => BackupError::Io(error),
        zip::result::ZipError::InvalidArchive(_) | zip::result::ZipError::UnsupportedArchive(_) => {
            BackupError::NotABackup
        }
        other => BackupError::Damaged(other.to_string()),
    }
}

/// The path with `suffix` added to its file name (`a.zip` → `a.zip.part`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Backups of one database and its project folders on this computer.
pub struct LocalBackups {
    db: Arc<Database>,
    paths: DataPaths,
}

impl LocalBackups {
    pub fn new(db: Arc<Database>, paths: DataPaths) -> Self {
        Self { db, paths }
    }

    pub fn paths(&self) -> &DataPaths {
        &self.paths
    }

    fn write_zip(
        &self,
        to: &Path,
        options: BackupOptions,
        progress: &mut ProgressReport<'_>,
    ) -> Result<BackupInfo, BackupError> {
        // Whole milliseconds, as the manifest keeps it.
        let created_at = from_unix_millis(to_unix_millis(SystemTime::now()));
        let media = if options.media {
            media_files(&self.paths.projects)?
        } else {
            Vec::new()
        };
        let copy = with_suffix(to, ".db.part");
        let written = (|| {
            snapshot(&self.db, &copy)?;
            let database_bytes = fs::metadata(&copy)?.len();
            let info = BackupInfo {
                created_at,
                bardo_version: env!("CARGO_PKG_VERSION").to_owned(),
                schema_version: SCHEMA_VERSION as u32,
                media: options.media,
                automatic: options.automatic,
                database_bytes,
                media_bytes: media.iter().map(|file| file.bytes).sum(),
                media_files: media.len() as u64,
                saved_keys: options.saved_keys.clone(),
                saved_credentials: options.saved_credentials.clone(),
            };
            let mut copier = Copier::new(info.database_bytes + info.media_bytes, progress);
            let part = with_suffix(to, ".part");
            let zipped = write_archive(&part, &info, &copy, &media, &mut copier)
                .and_then(|()| Ok(fs::rename(&part, to)?));
            if zipped.is_err() {
                let _ = fs::remove_file(&part);
            }
            zipped.map(|()| info)
        })();
        let _ = fs::remove_file(&copy);
        written
    }

    fn stage_from(
        &self,
        from: &Path,
        progress: &mut ProgressReport<'_>,
    ) -> Result<BackupInfo, BackupError> {
        let mut archive = open_archive(from)?;
        let manifest = read_manifest(&mut archive)?;
        let incoming = self.paths.incoming();
        fs::create_dir_all(&incoming)?;
        let mut entries: Vec<(usize, PathBuf, u64)> = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index).map_err(zip_error)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().map_err(zip_error)?;
            let wanted = name == DATABASE_ENTRY
                || (manifest.media && name.starts_with(&format!("{PROJECTS_ENTRY}/")));
            if !wanted {
                continue;
            }
            let relative = entry
                .enclosed_name()
                .ok_or_else(|| BackupError::Damaged(format!("unsafe path {name}")))?;
            entries.push((index, relative, entry.size()));
        }
        if !entries
            .iter()
            .any(|(_, path, _)| path == Path::new(DATABASE_ENTRY))
        {
            return Err(BackupError::Damaged("no database".into()));
        }
        let mut copier = Copier::new(entries.iter().map(|(_, _, bytes)| bytes).sum(), progress);
        for (index, relative, _) in entries {
            if !relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            {
                return Err(BackupError::Damaged(format!(
                    "unsafe path {}",
                    relative.display()
                )));
            }
            let target = incoming.join(&relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut entry = archive.by_index(index).map_err(zip_error)?;
            let mut out = BufWriter::new(File::create(&target)?);
            copier.copy(&mut entry, &mut out)?;
            out.into_inner()
                .map_err(|error| error.into_error())?
                .sync_all()?;
        }
        prepare_restored(&incoming.join(DATABASE_ENTRY), SystemTime::now())?;
        let info = manifest.info();
        let marker = self.paths.ready_marker();
        let part = with_suffix(&marker, ".part");
        fs::write(&part, toml_text(&manifest)?)?;
        fs::rename(&part, &marker)?;
        Ok(info)
    }
}

fn toml_text(value: &impl Serialize) -> Result<String, BackupError> {
    toml::to_string(value).map_err(|error| BackupError::Damaged(error.to_string()))
}

/// Copies streams, reporting progress over a known total and stopping
/// when the report says so.
struct Copier<'a, 'b> {
    total: u64,
    done: u64,
    progress: &'a mut ProgressReport<'b>,
    buffer: Vec<u8>,
}

impl<'a, 'b> Copier<'a, 'b> {
    fn new(total: u64, progress: &'a mut ProgressReport<'b>) -> Self {
        Self {
            total,
            done: 0,
            progress,
            buffer: vec![0; CHUNK],
        }
    }

    fn report(&mut self) -> Result<(), BackupError> {
        if (self.progress)(Progress::of(self.done, self.total.max(1))) {
            Ok(())
        } else {
            Err(BackupError::Stopped)
        }
    }

    fn copy(&mut self, from: &mut impl Read, to: &mut impl Write) -> Result<u64, BackupError> {
        let mut copied = 0;
        loop {
            self.report()?;
            let read = match from.read(&mut self.buffer) {
                Ok(0) => return Ok(copied),
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            };
            to.write_all(&self.buffer[..read])?;
            copied += read as u64;
            // A file that grew while copied does not push past the end.
            self.done = (self.done + read as u64).min(self.total);
        }
    }
}

/// One file of the project folders a backup carries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaFile {
    path: PathBuf,
    /// Its name in the backup, `projects/<project>/<file>`.
    entry: String,
    bytes: u64,
}

/// The project folders' files a backup carries: everything but the
/// editor's preview copies (made again when the editor opens) and files
/// half written. Folders inside a project folder are walked too.
fn media_files(root: &Path) -> Result<Vec<MediaFile>, BackupError> {
    let mut files = Vec::new();
    if root.is_dir() {
        walk(root, PROJECTS_ENTRY, &mut files)?;
    }
    files.sort_by(|a, b| a.entry.cmp(&b.entry));
    Ok(files)
}

fn walk(dir: &Path, entry: &str, files: &mut Vec<MediaFile>) -> Result<(), BackupError> {
    let mut names = Vec::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        let Some(name) = item.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        names.push((name, item.file_type()?, item.path()));
    }
    let present: std::collections::HashSet<&str> =
        names.iter().map(|(name, _, _)| name.as_str()).collect();
    for (name, kind, path) in &names {
        let entry = format!("{entry}/{name}");
        if kind.is_dir() {
            walk(path, &entry, files)?;
        } else if kind.is_file() && !is_rebuilt(name, &present) && !name.ends_with(".partial") {
            files.push(MediaFile {
                path: path.clone(),
                entry,
                bytes: fs::metadata(path)?.len(),
            });
        }
    }
    Ok(())
}

/// Whether `name` is a preview copy of a file beside it, which the editor
/// makes again: `proxy-<file>.mkv`, `proxy-<file>.jpg`, `peaks-<file>.json`.
fn is_rebuilt(name: &str, present: &std::collections::HashSet<&str>) -> bool {
    let source = name
        .strip_prefix("proxy-")
        .and_then(|rest| {
            rest.strip_suffix(".mkv")
                .or_else(|| rest.strip_suffix(".jpg"))
        })
        .or_else(|| {
            name.strip_prefix("peaks-")
                .and_then(|rest| rest.strip_suffix(".json"))
        });
    source.is_some_and(|source| present.contains(source))
}

fn write_archive(
    part: &Path,
    info: &BackupInfo,
    database: &Path,
    media: &[MediaFile],
    copier: &mut Copier<'_, '_>,
) -> Result<(), BackupError> {
    use zip::write::SimpleFileOptions;
    let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(part)?));
    let deflated = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip_time(info.created_at));
    // Media is compressed already: stored, it costs no time.
    let stored = deflated.compression_method(zip::CompressionMethod::Stored);
    // Zip64 for anything near the 4 GB a plain entry holds.
    let sized = |options: SimpleFileOptions, bytes: u64| options.large_file(bytes >= 1 << 31);

    zip.start_file(MANIFEST, deflated).map_err(zip_error)?;
    zip.write_all(toml_text(&Manifest::from_info(info))?.as_bytes())?;
    zip.start_file(DATABASE_ENTRY, sized(deflated, info.database_bytes))
        .map_err(zip_error)?;
    copier.copy(&mut File::open(database)?, &mut zip)?;
    for file in media {
        // A file removed since the walk is left out.
        let mut source = match File::open(&file.path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        zip.start_file(file.entry.as_str(), sized(stored, file.bytes))
            .map_err(zip_error)?;
        copier.copy(&mut source, &mut zip)?;
    }
    let out = zip.finish().map_err(zip_error)?;
    out.into_inner()
        .map_err(|error| error.into_error())?
        .sync_all()?;
    Ok(())
}

/// Zip keeps local time; without one every entry reads 1980.
fn zip_time(at: SystemTime) -> zip::DateTime {
    let Ok(at) = jiff::Timestamp::try_from(at) else {
        return zip::DateTime::default();
    };
    let at = at.to_zoned(jiff::tz::TimeZone::system());
    zip::DateTime::from_date_and_time(
        u16::try_from(at.year()).unwrap_or(1980),
        at.month().unsigned_abs(),
        at.day().unsigned_abs(),
        at.hour().unsigned_abs(),
        at.minute().unsigned_abs(),
        at.second().unsigned_abs(),
    )
    .unwrap_or_default()
}

/// Copies the database to `to` as it is at one moment, with SQLite's
/// online backup API, while other connections (the app, the agent) keep
/// writing. Then cleans the copy, leaves out the YouTube data a backup may
/// not keep, takes it out of WAL, vacuums it (no page of deleted data
/// survives) and checks it.
fn snapshot(db: &Database, to: &Path) -> Result<(), BackupError> {
    let probe = Instant::now();
    eprintln!("PROBE snapshot start");
    let _ = fs::remove_file(to);
    let mut copy = Connection::open(to).map_err(database_error)?;
    {
        let own;
        let shared;
        let source: &Connection = match db.file_path() {
            // A connection of its own, so the app's statements do not wait
            // for the copy.
            Some(path) => {
                own = Connection::open(path).map_err(database_error)?;
                own.busy_timeout(BUSY_TIMEOUT).map_err(database_error)?;
                &own
            }
            None => {
                shared = db.conn();
                &shared
            }
        };
        eprintln!("PROBE source open {:?}", probe.elapsed());
        let backup = Backup::new(source, &mut copy).map_err(database_error)?;
        // All pages in one step: the copy is one moment of the database.
        let deadline = Instant::now() + BUSY_TIMEOUT;
        loop {
            let step = backup.step(-1).map_err(database_error)?;
            eprintln!("PROBE step {step:?} at {:?}", probe.elapsed());
            match step {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                _ => return Err(BackupError::Database("the database stayed busy".into())),
            }
        }
    }
    eprintln!("PROBE source closed {:?}", probe.elapsed());
    clean(&copy, None)?;
    crate::youtube_data::strip_for_backup(&copy).map_err(database_error)?;
    copy.pragma_update(None, "journal_mode", "DELETE")
        .map_err(database_error)?;
    copy.execute_batch("VACUUM").map_err(database_error)?;
    check_integrity(&copy)?;
    eprintln!("PROBE snapshot done {:?}", probe.elapsed());
    copy.close().map_err(|(_, error)| database_error(error))
}

/// Leaves a copy of the database fit to be another installation's: no
/// job lease, runner or hold of the processes that had it open, and no
/// backup or restore job (restored, it would run the backup that made it).
/// A copy being restored (`restored_at`) also has every claimed scheduled
/// post claimed long enough ago that Bardo asks the network and lists it as
/// missed, instead of resuming it (ADR-0006, `due`).
fn clean(conn: &Connection, restored_at: Option<SystemTime>) -> Result<(), BackupError> {
    conn.execute_batch(
        "DELETE FROM runner_hold;
         DELETE FROM job_runner;
         UPDATE job SET leased_by = NULL, leased_until = NULL;
         DELETE FROM job WHERE kind IN ('backup', 'restore');",
    )
    .map_err(database_error)?;
    if let Some(at) = restored_at {
        let cutoff = at
            .checked_sub(DUE_GRACE + Duration::from_secs(60))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        conn.execute(
            "UPDATE publication SET upload_claimed_at = ?1 WHERE upload_claimed_at > ?1",
            [to_unix_millis(cutoff)],
        )
        .map_err(database_error)?;
        // An upload Bardo does not schedule itself, waiting or sending in the
        // backup, may have gone after it was made: resumed, it would post
        // the video again. It fails instead, and the user checks the network
        // before retrying. Instagram's scheduled posts are left to the due
        // rules above; a video the network is processing is already there.
        conn.execute_batch(&format!(
            "UPDATE job SET state = 'failed', failure_kind = '{kind}',
                            failure_detail = 'under way in a restored backup', retry_at = NULL
             WHERE kind = 'upload' AND state IN ('queued', 'running')
               AND id IN (SELECT upload_job FROM publication WHERE {sending});
             UPDATE publication SET upload_status = 'failed', upload_failure = 'job:{kind}'
             WHERE {sending};",
            kind = JobFailureKind::Restored.code(),
            sending = "kind = 'uploaded' AND upload_status IN ('queued', 'uploading')
                       AND NOT (network = 'instagram_reels' AND upload_publish_at IS NOT NULL)",
        ))
        .map_err(database_error)?;
    }
    Ok(())
}

fn check_integrity(conn: &Connection) -> Result<(), BackupError> {
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(database_error)?;
    if result == "ok" {
        Ok(())
    } else {
        Err(BackupError::Damaged(format!("integrity check: {result}")))
    }
}

/// Checks an unpacked database, migrates it to this build's schema and
/// cleans it, so the next start only has to move it in.
fn prepare_restored(path: &Path, now: SystemTime) -> Result<(), BackupError> {
    {
        let conn = Connection::open(path).map_err(database_error)?;
        check_integrity(&conn)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(database_error)?;
        if version as usize > SCHEMA_VERSION {
            return Err(BackupError::Damaged(
                "schema newer than its manifest".into(),
            ));
        }
    }
    drop(Database::open(path).map_err(database_error)?);
    let conn = Connection::open(path).map_err(database_error)?;
    clean(&conn, Some(now))?;
    conn.pragma_update(None, "journal_mode", "DELETE")
        .map_err(database_error)?;
    check_integrity(&conn)?;
    conn.close().map_err(|(_, error)| database_error(error))
}

fn open_archive(path: &Path) -> Result<zip::ZipArchive<BufReader<File>>, BackupError> {
    let file = File::open(path)?;
    zip::ZipArchive::new(BufReader::new(file)).map_err(zip_error)
}

fn read_manifest(
    archive: &mut zip::ZipArchive<impl Read + io::Seek>,
) -> Result<Manifest, BackupError> {
    let mut text = String::new();
    match archive.by_name(MANIFEST) {
        Ok(entry) => {
            entry
                .take(64 * 1024)
                .read_to_string(&mut text)
                .map_err(|_| BackupError::Damaged("unreadable manifest".into()))?;
        }
        Err(zip::result::ZipError::FileNotFound) => return Err(BackupError::NotABackup),
        Err(error) => return Err(zip_error(error)),
    }
    let manifest: Manifest =
        toml::from_str(&text).map_err(|error| BackupError::Damaged(error.to_string()))?;
    manifest.check()?;
    if archive.index_for_name(DATABASE_ENTRY).is_none() {
        return Err(BackupError::Damaged("no database".into()));
    }
    Ok(manifest)
}

/// `bardo-auto-20261014-031500.zip` for a backup made at `at` (UTC).
fn automatic_name(at: SystemTime) -> String {
    let at = jiff::Timestamp::try_from(at)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .strftime("%Y%m%d-%H%M%S");
    format!("bardo-auto-{at}.zip")
}

impl BackupStore for LocalBackups {
    fn media_size(&self) -> Result<MediaSize, BackupError> {
        let files = media_files(&self.paths.projects)?;
        Ok(MediaSize {
            bytes: files.iter().map(|file| file.bytes).sum(),
            files: files.len() as u64,
        })
    }

    fn write(
        &self,
        to: &Path,
        options: BackupOptions,
        progress: &mut ProgressReport<'_>,
    ) -> Result<BackupInfo, BackupError> {
        if let Some(dir) = to.parent() {
            fs::create_dir_all(dir)?;
        }
        self.write_zip(to, options, progress)
    }

    fn inspect(&self, from: &Path) -> Result<BackupInfo, BackupError> {
        let mut archive = open_archive(from)?;
        Ok(read_manifest(&mut archive)?.info())
    }

    fn stage(
        &self,
        from: &Path,
        progress: &mut ProgressReport<'_>,
    ) -> Result<BackupInfo, BackupError> {
        self.discard_staged()?;
        let staged = self.stage_from(from, progress);
        if staged.is_err() {
            let _ = self.discard_staged();
        }
        staged
    }

    fn staged(&self) -> Option<BackupInfo> {
        read_staged(&self.paths).map(|manifest| manifest.info())
    }

    fn discard_staged(&self) -> Result<(), BackupError> {
        if self.paths.keep_marker().exists() {
            return Err(BackupError::Io(io::Error::other(format!(
                "earlier data is kept in {}",
                self.paths.previous().display()
            ))));
        }
        // The marker goes first: a staging half deleted is never applied.
        for removed in [
            fs::remove_file(self.paths.ready_marker()),
            fs::remove_dir_all(self.paths.staging()),
        ] {
            match removed {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(())
    }

    fn automatic_path(&self, at: SystemTime) -> PathBuf {
        self.paths.automatic_dir().join(automatic_name(at))
    }

    fn automatic(&self) -> Result<Vec<SavedBackup>, BackupError> {
        let mut saved = Vec::new();
        let items = match fs::read_dir(self.paths.automatic_dir()) {
            Ok(items) => items,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(saved),
            Err(error) => return Err(error.into()),
        };
        for item in items {
            let path = item?.path();
            let automatic = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("bardo-auto-") && name.ends_with(".zip"));
            if !automatic {
                continue;
            }
            // One that cannot be read is not offered.
            let Ok(info) = self.inspect(&path) else {
                continue;
            };
            saved.push(SavedBackup {
                bytes: fs::metadata(&path)?.len(),
                created_at: info.created_at,
                path,
            });
        }
        saved.sort_by_key(|saved| std::cmp::Reverse(saved.created_at));
        Ok(saved)
    }

    fn prune_automatic(&self, keep: usize) -> Result<(), BackupError> {
        let dir = self.paths.automatic_dir();
        let mut names: Vec<String> = match fs::read_dir(&dir) {
            Ok(items) => items
                .filter_map(|item| item.ok()?.file_name().into_string().ok())
                .filter(|name| name.starts_with("bardo-auto-") && name.ends_with(".zip"))
                .collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        // The names sort by time.
        names.sort_unstable_by(|a, b| b.cmp(a));
        for name in names.into_iter().skip(keep) {
            fs::remove_file(dir.join(name))?;
        }
        Ok(())
    }

    fn last_restore(&self) -> Option<RestoreOutcome> {
        let text = fs::read_to_string(self.paths.restore_record()).ok()?;
        let record: RestoreRecord = toml::from_str(&text).ok()?;
        Some(RestoreOutcome {
            backup_created_at: from_unix_millis(record.backup_created_at),
            media: record.media,
            restored_at: from_unix_millis(record.restored_at),
            failure: record.failure,
            kept_aside: record.kept_aside,
            saved_keys: parse_codes(&record.saved_keys),
            saved_credentials: parse_codes(&record.saved_credentials),
        })
    }

    fn forget_last_restore(&self) -> Result<(), BackupError> {
        match fs::remove_file(self.paths.restore_record()) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
}

fn read_staged(paths: &DataPaths) -> Option<Manifest> {
    let text = fs::read_to_string(paths.ready_marker()).ok()?;
    toml::from_str(&text).ok()
}

/// Whether a restore is staged to be swapped in at the next start. The
/// background agent does not start while one is.
pub fn restore_pending(paths: &DataPaths) -> bool {
    paths.ready_marker().is_file()
}

/// Run at start, before the database opens: swaps a staged restore in.
/// The current database (and project folders, when the backup has media)
/// moves aside and the restored one moves in; when a move fails, every
/// move is undone and the data stays as it was. The outcome is kept for
/// Settings to show (`BackupStore::last_restore`). `None` when no restore
/// was staged.
pub fn apply_pending_restore(paths: &DataPaths) -> Option<RestoreOutcome> {
    apply_pending_restore_with(paths, SWAP_PATIENCE)
}

pub(crate) fn apply_pending_restore_with(
    paths: &DataPaths,
    patience: Duration,
) -> Option<RestoreOutcome> {
    if !restore_pending(paths) {
        return None;
    }
    let manifest = read_staged(paths);
    let incoming = paths.incoming().join(DATABASE_ENTRY);
    let swapped = match &manifest {
        // The database went in, and Bardo stopped before the rest: the
        // media still waiting goes in too.
        Some(manifest) if !incoming.is_file() => {
            if !paths.previous().exists() {
                // Swapped and tidied up already.
                let _ = fs::remove_file(paths.ready_marker());
                return None;
            }
            finish_media(paths, manifest.media, patience).map_err(|error| SwapFailure {
                reason: error.to_string(),
                undone: false,
            })
        }
        Some(manifest) => swap(paths, manifest.media, patience),
        None => Err(SwapFailure {
            reason: "the staged restore could not be read".to_owned(),
            undone: true,
        }),
    };
    let kept_aside = match &swapped {
        Err(failure) if !failure.undone => {
            keep_previous(paths);
            Some(paths.previous())
        }
        _ => None,
    };
    let _ = fs::remove_file(paths.ready_marker());
    let now = from_unix_millis(to_unix_millis(SystemTime::now()));
    let outcome = RestoreOutcome {
        backup_created_at: manifest
            .as_ref()
            .map_or(now, |manifest| from_unix_millis(manifest.created_at)),
        media: manifest.as_ref().is_some_and(|manifest| manifest.media),
        restored_at: now,
        failure: swapped.err().map(|failure| failure.reason),
        kept_aside,
        saved_keys: manifest.as_ref().map(Manifest::keys).unwrap_or_default(),
        saved_credentials: manifest
            .as_ref()
            .map(Manifest::credentials)
            .unwrap_or_default(),
    };
    let record = RestoreRecord {
        backup_created_at: to_unix_millis(outcome.backup_created_at),
        media: outcome.media,
        restored_at: to_unix_millis(now),
        failure: outcome.failure.clone(),
        kept_aside: outcome.kept_aside.clone(),
        saved_keys: manifest
            .as_ref()
            .map(|m| m.saved_keys.clone())
            .unwrap_or_default(),
        saved_credentials: manifest
            .as_ref()
            .map(|m| m.saved_credentials.clone())
            .unwrap_or_default(),
    };
    if let Ok(text) = toml::to_string(&record) {
        let _ = fs::write(paths.restore_record(), text);
    }
    Some(outcome)
}

/// Marks `restore\previous` as the user's data: nothing deletes it.
fn keep_previous(paths: &DataPaths) {
    let _ = fs::write(
        paths.keep_marker(),
        "A restore could not put Bardo's earlier data back. It is in the \"previous\" \
         folder beside this file; Bardo does not delete it.\n",
    );
}

/// Held for as long as a Bardo window has the data open. A second window
/// does not get it, and leaves restores to the first (ADR-0010).
#[derive(Debug)]
pub struct WindowLock {
    _file: File,
}

/// Takes the window lock, waiting up to `wait` for a window on its way out
/// (Bardo restarting after a restore). `None` when another window keeps it.
pub fn lock_window(paths: &DataPaths, wait: Duration) -> io::Result<Option<WindowLock>> {
    fs::create_dir_all(&paths.local)?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.window_lock())?;
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(WindowLock { _file: file })),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(error)) => return Err(error),
        }
    }
}

/// Removes what a restore left beside the data: the previous data once the
/// restored data is in, or a staging that was never finished. Never data a
/// failed swap kept aside. Deleting a large media folder takes a while: run
/// it off the UI thread.
pub fn clean_restore_leftovers(paths: &DataPaths) {
    let staging = paths.staging();
    if staging.exists() && !restore_pending(paths) && !paths.keep_marker().exists() {
        // Moved out of the way first, so a new restore can stage meanwhile.
        let _ = fs::remove_dir_all(paths.discarded());
        let _ = fs::rename(&staging, paths.discarded());
    }
    let _ = fs::remove_dir_all(paths.discarded());
}

/// Why a swap failed, and whether every file it moved went back.
#[derive(Debug)]
struct SwapFailure {
    reason: String,
    undone: bool,
}

/// The swap itself, undone when any step fails.
fn swap(paths: &DataPaths, media: bool, patience: Duration) -> Result<(), SwapFailure> {
    let previous = paths.previous();
    // An earlier try stopped midway: what it moved aside is not this try's
    // to undo, so a failure keeps it.
    let resumed = previous.join(DATABASE_ENTRY).exists();
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| -> io::Result<()> {
        fs::create_dir_all(&previous)?;
        for suffix in ["", "-wal", "-shm"] {
            let from = with_suffix(&paths.database, suffix);
            if from.exists() {
                let to = with_suffix(&previous.join(DATABASE_ENTRY), suffix);
                move_path(&from, &to, patience)?;
                moved.push((from, to));
            }
        }
        if media && paths.projects.exists() {
            let to = previous.join(PROJECTS_ENTRY);
            move_path(&paths.projects, &to, patience)?;
            moved.push((paths.projects.clone(), to));
        }
        if let Some(dir) = paths.database.parent() {
            fs::create_dir_all(dir)?;
        }
        let incoming = paths.incoming();
        move_path(&incoming.join(DATABASE_ENTRY), &paths.database, patience)?;
        moved.push((incoming.join(DATABASE_ENTRY), paths.database.clone()));
        finish_media(paths, media, patience)
    })();
    result.map_err(|error| {
        // What cannot move back stays in `restore\previous`, kept.
        let mut undone = true;
        for (from, to) in moved.iter().rev() {
            undone &= move_path(to, from, patience).is_ok();
        }
        SwapFailure {
            reason: error.to_string(),
            undone: undone && !resumed,
        }
    })
}

/// Moves the restored project folders in, once the database is: also when
/// Bardo stopped between the two.
fn finish_media(paths: &DataPaths, media: bool, patience: Duration) -> io::Result<()> {
    if !media {
        return Ok(());
    }
    let projects = paths.incoming().join(PROJECTS_ENTRY);
    if !projects.exists() {
        return fs::create_dir_all(&paths.projects);
    }
    if paths.projects.exists() {
        // Bardo stopped after it moved the current folders aside and before
        // the restored ones went in: only an empty folder can be here.
        fs::remove_dir(&paths.projects)?;
    }
    move_path(&projects, &paths.projects, patience)
}

/// Moves a file or folder, waiting up to `patience` for a process that
/// still has it open, and copying when the two are on different drives.
fn move_path(from: &Path, to: &Path, patience: Duration) -> io::Result<()> {
    let deadline = Instant::now() + patience;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
                copy_all(from, to)?;
                return if from.is_dir() {
                    fs::remove_dir_all(from)
                } else {
                    fs::remove_file(from)
                };
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(error),
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(error);
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

fn copy_all(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for item in fs::read_dir(from)? {
            let item = item?;
            copy_all(&item.path(), &to.join(item.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ())
    }
}

/// Backups kept in memory, for tests of the app: what was written, staged
/// and restored, without files.
#[derive(Default)]
pub struct MemoryBackups {
    state: std::sync::Mutex<MemoryState>,
}

#[derive(Default)]
struct MemoryState {
    written: Vec<(PathBuf, BackupInfo)>,
    staged: Option<BackupInfo>,
    media: MediaSize,
    last_restore: Option<RestoreOutcome>,
}

impl MemoryBackups {
    fn state(&self) -> std::sync::MutexGuard<'_, MemoryState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The backups written, oldest first.
    pub fn written(&self) -> Vec<(PathBuf, BackupInfo)> {
        self.state().written.clone()
    }

    pub fn set_media(&self, media: MediaSize) {
        self.state().media = media;
    }

    /// As if the last start had applied a restore.
    pub fn set_last_restore(&self, outcome: Option<RestoreOutcome>) {
        self.state().last_restore = outcome;
    }
}

impl BackupStore for MemoryBackups {
    fn media_size(&self) -> Result<MediaSize, BackupError> {
        Ok(self.state().media)
    }

    fn write(
        &self,
        to: &Path,
        options: BackupOptions,
        progress: &mut ProgressReport<'_>,
    ) -> Result<BackupInfo, BackupError> {
        if !progress(Progress::DONE) {
            return Err(BackupError::Stopped);
        }
        let mut state = self.state();
        let media = if options.media {
            state.media
        } else {
            MediaSize::default()
        };
        let info = BackupInfo {
            created_at: from_unix_millis(to_unix_millis(SystemTime::now())),
            bardo_version: env!("CARGO_PKG_VERSION").to_owned(),
            schema_version: SCHEMA_VERSION as u32,
            media: options.media,
            automatic: options.automatic,
            database_bytes: 4096,
            media_bytes: media.bytes,
            media_files: media.files,
            saved_keys: options.saved_keys,
            saved_credentials: options.saved_credentials,
        };
        state.written.retain(|(path, _)| path != to);
        state.written.push((to.to_owned(), info.clone()));
        Ok(info)
    }

    fn inspect(&self, from: &Path) -> Result<BackupInfo, BackupError> {
        self.state()
            .written
            .iter()
            .find(|(path, _)| path == from)
            .map(|(_, info)| info.clone())
            .ok_or(BackupError::NotABackup)
    }

    fn stage(
        &self,
        from: &Path,
        progress: &mut ProgressReport<'_>,
    ) -> Result<BackupInfo, BackupError> {
        let info = self.inspect(from)?;
        if !progress(Progress::DONE) {
            return Err(BackupError::Stopped);
        }
        self.state().staged = Some(info.clone());
        Ok(info)
    }

    fn staged(&self) -> Option<BackupInfo> {
        self.state().staged.clone()
    }

    fn discard_staged(&self) -> Result<(), BackupError> {
        self.state().staged = None;
        Ok(())
    }

    fn automatic_path(&self, at: SystemTime) -> PathBuf {
        Path::new("backups").join(automatic_name(at))
    }

    fn automatic(&self) -> Result<Vec<SavedBackup>, BackupError> {
        let mut saved: Vec<SavedBackup> = self
            .state()
            .written
            .iter()
            .filter(|(_, info)| info.automatic)
            .map(|(path, info)| SavedBackup {
                path: path.clone(),
                created_at: info.created_at,
                bytes: info.database_bytes,
            })
            .collect();
        saved.sort_by_key(|saved| std::cmp::Reverse(saved.created_at));
        Ok(saved)
    }

    fn prune_automatic(&self, keep: usize) -> Result<(), BackupError> {
        let gone: Vec<PathBuf> = self
            .automatic()?
            .into_iter()
            .skip(keep)
            .map(|saved| saved.path)
            .collect();
        self.state()
            .written
            .retain(|(path, _)| !gone.contains(path));
        Ok(())
    }

    fn last_restore(&self) -> Option<RestoreOutcome> {
        self.state().last_restore.clone()
    }

    fn forget_last_restore(&self) -> Result<(), BackupError> {
        self.state().last_restore = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use bardo_domain::{
        Job, JobKind, JobRepository, JobState, ProfileRepository, RunnerRole, UiLanguage,
        UserProfile,
    };

    use super::*;

    /// One installation's folders under a temporary directory.
    struct Install {
        _dir: tempfile::TempDir,
        paths: DataPaths,
        db: Arc<Database>,
    }

    impl Install {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let paths = DataPaths {
                database: dir.path().join("roaming").join("bardo.db"),
                projects: dir.path().join("local").join("projects"),
                local: dir.path().join("local"),
            };
            let db = Arc::new(Database::open(&paths.database).unwrap());
            Self {
                _dir: dir,
                paths,
                db,
            }
        }

        fn with_profile() -> (Self, UserProfile) {
            let install = Self::new();
            let profile = UserProfile::new(UiLanguage::PtBr);
            ProfileRepository::save(&*install.db, &profile).unwrap();
            (install, profile)
        }

        fn backups(&self) -> LocalBackups {
            LocalBackups::new(Arc::clone(&self.db), self.paths.clone())
        }

        fn file(&self, relative: &str, bytes: &[u8]) {
            let path = self.paths.projects.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }

        /// A place for backups outside the data.
        fn out(&self, name: &str) -> PathBuf {
            self._dir.path().join("out").join(name)
        }
    }

    fn go_on() -> impl FnMut(Progress) -> bool {
        |_| true
    }

    const WITH_MEDIA: BackupOptions = BackupOptions {
        media: true,
        automatic: false,
        saved_keys: Vec::new(),
        saved_credentials: Vec::new(),
    };
    const DATABASE_ONLY: BackupOptions = BackupOptions {
        media: false,
        automatic: false,
        saved_keys: Vec::new(),
        saved_credentials: Vec::new(),
    };

    fn entries(path: &Path) -> Vec<String> {
        let mut archive = open_archive(path).unwrap();
        (0..archive.len())
            .map(|index| {
                archive
                    .by_index_raw(index)
                    .unwrap()
                    .name()
                    .unwrap()
                    .into_owned()
            })
            .collect()
    }

    /// The backup's database, unpacked to its own file.
    fn unpacked_database(path: &Path, to: &Path) -> Connection {
        let mut archive = open_archive(path).unwrap();
        let mut entry = archive.by_name(DATABASE_ENTRY).unwrap();
        let mut out = File::create(to).unwrap();
        io::copy(&mut entry, &mut out).unwrap();
        Connection::open(to).unwrap()
    }

    #[test]
    fn a_backup_made_while_the_app_and_the_agent_write_is_whole_and_checked() {
        let (install, profile) = Install::with_profile();
        let to = install.out("backup.zip");
        // The agent: its own process, with its own connection to the file.
        let agent = Database::open(&install.paths.database).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let stop = Arc::clone(&stop);
            let owner = profile.id;
            std::thread::spawn(move || {
                let begun = Instant::now();
                let mut written = 0;
                let mut slowest = Duration::ZERO;
                while !stop.load(Ordering::Relaxed) {
                    let now = SystemTime::now();
                    let call = Instant::now();
                    if let Err(error) = agent.heartbeat(RunnerRole::Agent, now, now) {
                        eprintln!(
                            "PROBE agent heartbeat failed after {:?} in the call, {:?} since start, {written} written, slowest earlier {slowest:?}: {error}",
                            call.elapsed(),
                            begun.elapsed()
                        );
                        panic!("heartbeat: {error}");
                    }
                    slowest = slowest.max(call.elapsed());
                    let call = Instant::now();
                    if let Err(error) =
                        JobRepository::save(&agent, &Job::new(owner, JobKind::Countdown, "{}"))
                    {
                        eprintln!(
                            "PROBE agent save failed after {:?} in the call, {:?} since start, {written} written, slowest earlier {slowest:?}: {error}",
                            call.elapsed(),
                            begun.elapsed()
                        );
                        panic!("save: {error}");
                    }
                    slowest = slowest.max(call.elapsed());
                    written += 1;
                }
                eprintln!(
                    "PROBE agent wrote {written} in {:?}, slowest call {slowest:?}",
                    begun.elapsed()
                );
                written
            })
        };
        // The app keeps writing too, and holds a lease.
        let leased = Job::new(profile.id, JobKind::Countdown, "{}");
        JobRepository::save(&*install.db, &leased).unwrap();
        let now = SystemTime::now();
        assert!(
            install
                .db
                .lease(leased.id(), now, now + Duration::from_secs(60))
                .unwrap()
        );
        install.db.heartbeat(RunnerRole::App, now, now).unwrap();

        let wal = with_suffix(&install.paths.database, "-wal");
        eprintln!(
            "PROBE wal before {:?}",
            fs::metadata(&wal).map(|m| m.len()).ok()
        );
        let started = Instant::now();
        let info = install
            .backups()
            .write(&to, DATABASE_ONLY, &mut go_on())
            .unwrap();
        eprintln!(
            "PROBE backup took {:?}, wal after {:?}",
            started.elapsed(),
            fs::metadata(&wal).map(|m| m.len()).ok()
        );
        stop.store(true, Ordering::Relaxed);
        assert!(writer.join().unwrap() > 0, "the agent wrote meanwhile");

        assert_eq!(entries(&to), [MANIFEST, DATABASE_ENTRY]);
        assert_eq!(info.schema_version as usize, SCHEMA_VERSION);
        assert!(!info.media);
        let copy = unpacked_database(&to, &install.out("copy.db"));
        check_integrity(&copy).unwrap();
        let mode: String = copy
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete", "a single file, out of WAL");
        let restored = Database::open(&install.out("copy.db")).unwrap();
        assert_eq!(restored.load_default().unwrap(), Some(profile.clone()));
        let jobs = JobRepository::list(&restored, profile.id).unwrap();
        assert!(jobs.iter().any(|job| job.id() == leased.id()));
        assert!(restored.runners(SystemTime::UNIX_EPOCH).unwrap().is_empty());
        assert!(
            restored
                .lease(leased.id(), now, now + Duration::from_secs(1))
                .unwrap(),
            "no lease of the app that made it"
        );
        assert!(!to.with_extension("zip.part").exists());
        assert!(!with_suffix(&to, ".db.part").exists());
    }

    #[test]
    fn a_backup_with_media_leaves_out_preview_copies_and_half_written_files() {
        let (install, _) = Install::with_profile();
        install.file("p1/clip-1.mp4", b"clip");
        install.file("p1/proxy-clip-1.mp4.mkv", b"proxy");
        install.file("p1/narration.mp3", b"voice");
        install.file("p1/peaks-narration.mp3.json", b"[]");
        install.file("p1/scene-2.png", b"image");
        install.file("p1/proxy-scene-2.png.jpg", b"small");
        install.file("p1/render-1.mp4.partial", b"half");
        // A preview copy with no source beside it is the user's file.
        install.file("p2/proxy-gone.mp4.mkv", b"kept");

        let backups = install.backups();
        assert_eq!(
            backups.media_size().unwrap(),
            MediaSize {
                bytes: 18,
                files: 4
            }
        );
        let to = install.out("full.zip");
        let info = backups.write(&to, WITH_MEDIA, &mut go_on()).unwrap();
        assert_eq!(
            entries(&to),
            [
                MANIFEST,
                DATABASE_ENTRY,
                "projects/p1/clip-1.mp4",
                "projects/p1/narration.mp3",
                "projects/p1/scene-2.png",
                "projects/p2/proxy-gone.mp4.mkv",
            ]
        );
        assert!(info.media);
        assert_eq!((info.media_bytes, info.media_files), (18, 4));
        assert_eq!(backups.inspect(&to).unwrap(), info);

        let to = install.out("small.zip");
        backups.write(&to, DATABASE_ONLY, &mut go_on()).unwrap();
        assert_eq!(entries(&to), [MANIFEST, DATABASE_ENTRY]);
    }

    #[test]
    fn a_stopped_backup_leaves_no_file() {
        let (install, _) = Install::with_profile();
        install.file("p1/clip-1.mp4", &vec![7; 3 * CHUNK]);
        let to = install.out("backup.zip");
        let mut reports = 0;
        let mut stop_soon = |_| {
            reports += 1;
            reports < 3
        };
        let result = install.backups().write(&to, WITH_MEDIA, &mut stop_soon);
        assert!(matches!(result, Err(BackupError::Stopped)), "{result:?}");
        assert!(!to.exists());
        let leftovers: Vec<_> = fs::read_dir(to.parent().unwrap()).unwrap().collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn progress_rises_to_the_end() {
        let (install, _) = Install::with_profile();
        install.file("p1/clip-1.mp4", &vec![7; 2 * CHUNK]);
        let mut seen = Vec::new();
        let mut record = |progress: Progress| {
            seen.push(progress);
            true
        };
        install
            .backups()
            .write(&install.out("b.zip"), WITH_MEDIA, &mut record)
            .unwrap();
        assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "{seen:?}");
        assert_eq!(seen.last(), Some(&Progress::DONE));
    }

    /// A zip with these entries.
    fn zip_of(path: &Path, files: &[(&str, &[u8])]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn manifest_text(change: impl FnOnce(&mut Manifest)) -> String {
        let mut manifest = Manifest {
            format: BACKUP_FORMAT,
            bardo_version: "9.9.9".into(),
            schema_version: SCHEMA_VERSION as u32,
            created_at: 0,
            media: false,
            automatic: false,
            database_bytes: 0,
            media_bytes: 0,
            media_files: 0,
            saved_keys: Vec::new(),
            saved_credentials: Vec::new(),
        };
        change(&mut manifest);
        toml::to_string(&manifest).unwrap()
    }

    #[test]
    fn what_is_not_a_backup_or_is_newer_is_refused() {
        let install = Install::new();
        let backups = install.backups();
        let path = install.out("x.zip");

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not a zip").unwrap();
        assert!(matches!(
            backups.inspect(&path),
            Err(BackupError::NotABackup)
        ));

        zip_of(&path, &[("notes.txt", b"hello")]);
        assert!(matches!(
            backups.inspect(&path),
            Err(BackupError::NotABackup)
        ));

        let newer = manifest_text(|m| m.schema_version += 1);
        zip_of(
            &path,
            &[(MANIFEST, newer.as_bytes()), (DATABASE_ENTRY, b"")],
        );
        match backups.inspect(&path) {
            Err(BackupError::Newer { bardo_version }) => assert_eq!(bardo_version, "9.9.9"),
            other => panic!("{other:?}"),
        }
        let newer = manifest_text(|m| m.format += 1);
        zip_of(
            &path,
            &[(MANIFEST, newer.as_bytes()), (DATABASE_ENTRY, b"")],
        );
        assert!(matches!(
            backups.inspect(&path),
            Err(BackupError::Newer { .. })
        ));

        let fine = manifest_text(|_| {});
        zip_of(&path, &[(MANIFEST, fine.as_bytes())]);
        assert!(matches!(
            backups.inspect(&path),
            Err(BackupError::Damaged(_))
        ));
        zip_of(&path, &[(MANIFEST, b"format = \"one\"")]);
        assert!(matches!(
            backups.inspect(&path),
            Err(BackupError::Damaged(_))
        ));
    }

    #[test]
    fn a_broken_database_or_an_unsafe_path_is_not_staged() {
        let install = Install::new();
        let backups = install.backups();
        let path = install.out("x.zip");
        let fine = manifest_text(|m| m.media = true);

        zip_of(
            &path,
            &[(MANIFEST, fine.as_bytes()), (DATABASE_ENTRY, b"garbage")],
        );
        assert!(backups.stage(&path, &mut go_on()).is_err());
        assert!(!restore_pending(&install.paths));
        assert!(!install.paths.staging().exists(), "nothing left staged");

        let (good, _) = Install::with_profile();
        let real = good.out("real.zip");
        good.backups()
            .write(&real, DATABASE_ONLY, &mut go_on())
            .unwrap();
        let database = {
            let mut archive = open_archive(&real).unwrap();
            let mut bytes = Vec::new();
            archive
                .by_name(DATABASE_ENTRY)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        };
        zip_of(
            &path,
            &[
                (MANIFEST, fine.as_bytes()),
                (DATABASE_ENTRY, &database),
                ("projects/../../evil.txt", b"x"),
            ],
        );
        assert!(matches!(
            backups.stage(&path, &mut go_on()),
            Err(BackupError::Damaged(_))
        ));
        assert!(!install._dir.path().join("evil.txt").exists());
        assert!(!restore_pending(&install.paths));
    }

    #[test]
    fn a_restore_swaps_in_the_database_and_media_at_the_next_start() {
        let (source, profile) = Install::with_profile();
        source.file("p1/clip-1.mp4", b"clip");
        let to = source.out("full.zip");
        let options = BackupOptions {
            saved_keys: vec![Provider::Claude, Provider::Gemini],
            saved_credentials: vec![Network::YouTube],
            ..WITH_MEDIA
        };
        let info = source.backups().write(&to, options, &mut go_on()).unwrap();
        assert_eq!(info.saved_keys, [Provider::Claude, Provider::Gemini]);
        assert_eq!(info.saved_credentials, [Network::YouTube]);

        // Another computer, with data of its own.
        let (target, other) = Install::with_profile();
        target.file("p9/old.mp4", b"old");
        let backups = target.backups();
        let staged = backups.stage(&to, &mut go_on()).unwrap();
        assert_eq!(staged, info);
        assert_eq!(backups.staged(), Some(info.clone()));
        assert!(restore_pending(&target.paths));
        // Nothing changes until the next start.
        assert_eq!(target.db.load_default().unwrap(), Some(other));
        drop(backups);
        let Install { _dir, paths, db } = target;
        drop(db);

        let outcome = apply_pending_restore(&paths).unwrap();
        assert!(outcome.succeeded(), "{outcome:?}");
        assert_eq!(outcome.backup_created_at, info.created_at);
        assert!(outcome.media);
        assert_eq!(outcome.saved_keys, info.saved_keys);
        assert_eq!(outcome.saved_credentials, info.saved_credentials);
        assert!(!restore_pending(&paths));
        let db = Arc::new(Database::open(&paths.database).unwrap());
        assert_eq!(db.load_default().unwrap(), Some(profile));
        assert_eq!(
            fs::read(paths.projects.join("p1/clip-1.mp4")).unwrap(),
            b"clip"
        );
        assert!(!paths.projects.join("p9").exists());

        let backups = LocalBackups::new(db, paths.clone());
        assert_eq!(backups.last_restore(), Some(outcome));
        backups.forget_last_restore().unwrap();
        assert_eq!(backups.last_restore(), None);
        assert!(paths.staging().exists(), "the previous data until tidied");
        clean_restore_leftovers(&paths);
        assert!(!paths.staging().exists());
        assert_eq!(apply_pending_restore(&paths), None, "nothing staged");
    }

    #[test]
    fn a_backup_without_media_leaves_the_project_folders_as_they_are() {
        let (source, profile) = Install::with_profile();
        source.file("p1/clip-1.mp4", b"clip");
        let to = source.out("small.zip");
        source
            .backups()
            .write(&to, DATABASE_ONLY, &mut go_on())
            .unwrap();

        let (target, _) = Install::with_profile();
        target.file("p9/mine.mp4", b"mine");
        target.backups().stage(&to, &mut go_on()).unwrap();
        let Install { _dir, paths, db } = target;
        drop(db);
        let outcome = apply_pending_restore(&paths).unwrap();
        assert!(outcome.succeeded() && !outcome.media);
        let db = Database::open(&paths.database).unwrap();
        assert_eq!(db.load_default().unwrap(), Some(profile));
        assert_eq!(
            fs::read(paths.projects.join("p9/mine.mp4")).unwrap(),
            b"mine"
        );
        assert!(!paths.projects.join("p1").exists());
    }

    #[test]
    fn a_failed_swap_puts_the_data_back() {
        let (source, _) = Install::with_profile();
        source.file("p1/clip-1.mp4", b"clip");
        let to = source.out("full.zip");
        source
            .backups()
            .write(&to, WITH_MEDIA, &mut go_on())
            .unwrap();

        let (target, mine) = Install::with_profile();
        target.file("p9/mine.mp4", b"mine");
        target.backups().stage(&to, &mut go_on()).unwrap();
        let Install { _dir, paths, db } = target;
        drop(db);
        // Something in the way of moving the project folders aside.
        fs::create_dir_all(paths.previous().join("projects").join("busy")).unwrap();

        let outcome = apply_pending_restore_with(&paths, Duration::ZERO).unwrap();
        assert!(!outcome.succeeded());
        assert!(!restore_pending(&paths), "not tried again");
        let db = Database::open(&paths.database).unwrap();
        assert_eq!(db.load_default().unwrap(), Some(mine), "the data as it was");
        assert_eq!(
            fs::read(paths.projects.join("p9/mine.mp4")).unwrap(),
            b"mine"
        );
        let backups = LocalBackups::new(Arc::new(db), paths.clone());
        assert_eq!(backups.last_restore(), Some(outcome));
    }

    #[test]
    fn a_swap_cut_short_after_the_move_is_not_applied_again() {
        let install = Install::new();
        fs::create_dir_all(install.paths.staging()).unwrap();
        fs::write(install.paths.ready_marker(), manifest_text(|_| {})).unwrap();
        assert!(restore_pending(&install.paths));
        assert_eq!(apply_pending_restore(&install.paths), None);
        assert!(!restore_pending(&install.paths));
    }

    /// `target` with a backup of `source`'s data and media staged, closed
    /// as Bardo is before a restart.
    fn staged_restore(source: &Install, target: Install) -> (tempfile::TempDir, DataPaths) {
        let to = source.out("full.zip");
        source
            .backups()
            .write(&to, WITH_MEDIA, &mut go_on())
            .unwrap();
        target.backups().stage(&to, &mut go_on()).unwrap();
        let Install { _dir, paths, db } = target;
        drop(db);
        (_dir, paths)
    }

    #[test]
    fn a_swap_cut_short_between_the_database_and_the_media_finishes_the_media() {
        let (source, profile) = Install::with_profile();
        source.file("p1/clip-1.mp4", b"clip");
        let (target, _) = Install::with_profile();
        target.file("p9/mine.mp4", b"mine");
        let (_dir, paths) = staged_restore(&source, target);
        // Bardo stopped right after the restored database went in.
        let previous = paths.previous();
        fs::create_dir_all(&previous).unwrap();
        fs::rename(&paths.database, previous.join(DATABASE_ENTRY)).unwrap();
        fs::rename(&paths.projects, previous.join(PROJECTS_ENTRY)).unwrap();
        fs::rename(paths.incoming().join(DATABASE_ENTRY), &paths.database).unwrap();

        let outcome = apply_pending_restore(&paths).unwrap();
        assert!(outcome.succeeded(), "{outcome:?}");
        assert_eq!(
            fs::read(paths.projects.join("p1/clip-1.mp4")).unwrap(),
            b"clip"
        );
        let db = Database::open(&paths.database).unwrap();
        assert_eq!(db.load_default().unwrap(), Some(profile));
    }

    #[test]
    fn a_swap_that_cannot_put_everything_back_keeps_the_previous_data() {
        let (source, _) = Install::with_profile();
        source.file("p1/clip-1.mp4", b"clip");
        let (target, _) = Install::with_profile();
        target.file("p9/mine.mp4", b"mine");
        let (_dir, paths) = staged_restore(&source, target);
        // An earlier try moved the database aside and stopped; now the
        // project folders cannot move.
        let previous = paths.previous();
        fs::create_dir_all(previous.join(PROJECTS_ENTRY).join("busy")).unwrap();
        fs::rename(&paths.database, previous.join(DATABASE_ENTRY)).unwrap();

        let outcome = apply_pending_restore_with(&paths, Duration::ZERO).unwrap();
        assert!(!outcome.succeeded());
        assert_eq!(outcome.kept_aside.as_deref(), Some(previous.as_path()));
        assert!(!restore_pending(&paths), "not tried again");

        clean_restore_leftovers(&paths);
        assert!(previous.join(DATABASE_ENTRY).is_file(), "never deleted");
        let backups = LocalBackups::new(
            Arc::new(Database::open(&paths.database).unwrap()),
            paths.clone(),
        );
        assert!(backups.discard_staged().is_err(), "nor by a new restore");
        assert!(previous.join(DATABASE_ENTRY).is_file());
        assert_eq!(backups.last_restore(), Some(outcome));
    }

    #[test]
    fn one_window_at_a_time_holds_the_data() {
        let install = Install::new();
        let first = lock_window(&install.paths, Duration::ZERO).unwrap();
        assert!(first.is_some());
        assert!(
            lock_window(&install.paths, Duration::ZERO)
                .unwrap()
                .is_none()
        );
        drop(first);
        assert!(
            lock_window(&install.paths, Duration::from_secs(1))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_restored_copy_keeps_no_lease_runner_hold_or_backup_job() {
        let (source, profile) = Install::with_profile();
        let now = SystemTime::now();
        let kept = Job::new(profile.id, JobKind::Countdown, "{}");
        let backup = Job::new(profile.id, JobKind::Backup, "{}");
        let restore = Job::new(profile.id, JobKind::Restore, "{}");
        for job in [&kept, &backup, &restore] {
            JobRepository::save(&*source.db, job).unwrap();
        }
        assert!(
            source
                .db
                .lease(kept.id(), now, now + Duration::from_secs(600))
                .unwrap()
        );
        source.db.heartbeat(RunnerRole::App, now, now).unwrap();
        assert!(source.db.hold(now, now + Duration::from_secs(600)).unwrap());

        let to = source.out("b.zip");
        source
            .backups()
            .write(&to, DATABASE_ONLY, &mut go_on())
            .unwrap();
        let (target, _) = Install::with_profile();
        target.backups().stage(&to, &mut go_on()).unwrap();
        let Install { _dir, paths, db } = target;
        drop(db);
        apply_pending_restore(&paths).unwrap();

        let db = Database::open(&paths.database).unwrap();
        let jobs = JobRepository::list(&db, profile.id).unwrap();
        assert_eq!(jobs.iter().map(Job::id).collect::<Vec<_>>(), [kept.id()]);
        assert_eq!(jobs[0].state(), JobState::Queued);
        assert!(db.runners(SystemTime::UNIX_EPOCH).unwrap().is_empty());
        assert_eq!(db.holder(now).unwrap(), None);
        assert!(
            db.lease(kept.id(), now, now + Duration::from_secs(1))
                .unwrap()
        );
    }

    #[test]
    fn automatic_backups_are_listed_newest_first_and_pruned() {
        let (install, _) = Install::with_profile();
        let backups = install.backups();
        assert!(backups.automatic().unwrap().is_empty());
        let day = Duration::from_secs(24 * 60 * 60);
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_760_000_000);
        let auto = BackupOptions {
            automatic: true,
            ..BackupOptions::default()
        };
        let mut paths = Vec::new();
        for n in 0..6 {
            let path = backups.automatic_path(start + n * 7 * day);
            assert!(path.starts_with(install.paths.local.join("backups")));
            backups.write(&path, auto.clone(), &mut go_on()).unwrap();
            paths.push(path);
        }
        // Something else in the folder is left alone.
        fs::write(install.paths.automatic_dir().join("notes.txt"), b"x").unwrap();

        let listed = backups.automatic().unwrap();
        assert_eq!(listed.len(), 6);
        assert!(
            listed
                .windows(2)
                .all(|pair| pair[0].created_at >= pair[1].created_at)
        );
        assert!(backups.inspect(&listed[0].path).unwrap().automatic);
        backups.prune_automatic(4).unwrap();
        let kept: Vec<PathBuf> = backups
            .automatic()
            .unwrap()
            .into_iter()
            .map(|b| b.path)
            .collect();
        let mut newest: Vec<PathBuf> = paths[2..].to_vec();
        newest.reverse();
        assert_eq!(kept, newest);
        assert!(install.paths.automatic_dir().join("notes.txt").exists());
    }
}
