//! Project packages (issue #342, ADR-0048): a project's records and media
//! in one `.bardo-project` zip, and a deleted project's folder opened
//! again.
//!
//! **What belongs to a project** is read from the schema, not listed by
//! hand: the project's row, then every row a foreign key with `ON DELETE
//! CASCADE` (or an `owner_project_id`) ties to a row already taken, until
//! nothing more comes. The global library items those rows point at
//! (narrators, products, world items, template versions) are carried
//! too, with their own parts. A table added later comes along by its keys.
//!
//! **Left out**, as profile-wide or secret: the profile's other rows, cost
//! records, budgets and rates, network connections (tokens live in the
//! system's secret store anyway), research caches, staging, unfinished
//! jobs, the posts never made, and the YouTube data that expires
//! (ADR-0012).
//!
//! **Brought in**, every id gets a new one, wherever it appears whole: in
//! a key, in a job's payload or in a file's name. The records go in one
//! transaction, after the media is in its folder.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    FolderImport, PACKAGE_FORMAT, PACKED_VIDEOS, PROJECT_MANIFEST, PROJECT_RECORDS,
    PackageContents, PackageError, PackageExport, PackageImport, PackageImported, PackageManifest,
    ProjectColor, ProjectId, ProjectManifest, ProjectPackages, TemplateKind, VIDEOS_DIR, WORLD_DIR,
    free_name, rewrite_ids, video_folder_name,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use uuid::Uuid;

use crate::backup::{MediaFile, copy_whole, walk};
use crate::{Database, SCHEMA_VERSION, migrations, sharing, to_unix_millis};

/// The manifest's name in a package. It goes first.
pub const PACKAGE_MANIFEST: &str = "bardo-package.json";

/// The records' name in a package.
const RECORDS_ENTRY: &str = "records.db";

/// Each video's folder in a package: `media/videos/<video id>/…`.
const VIDEOS_ENTRY: &str = "media/videos";

/// The project's `World` folder in a package.
const WORLD_ENTRY: &str = "media/world";

/// The pictures of the global world items the project uses.
const GLOBAL_WORLD_ENTRY: &str = "media/global-world";

/// How much is copied between two progress reports (and checks for stop).
const CHUNK: usize = 1024 * 1024;

/// Tables that never travel: the profile's own, secrets' companions,
/// caches and the queue's bookkeeping. `job` travels apart (done ones
/// only), `user_profile` as the one row the records hang from.
const LEFT_OUT: [&str; 15] = [
    "user_profile",
    "job",
    "network_connection",
    "cost_record",
    "deleted_work",
    "take_staging",
    "niche_research",
    "niche_research_upload",
    "budget",
    // A project's own keys stay on this computer (#343), and so do
    // the budgets that count what they pay.
    "project_key",
    "project_budget",
    "rate",
    "tour_progress",
    "runner_hold",
    "job_runner",
];

/// The library's items: owned by a project or global, carried when the
/// project uses a global one.
const LIBRARY: [&str; 4] = ["persona", "product", "character", "template_version"];

/// Library items whose names are unique per owner.
const NAMED: [&str; 3] = ["persona", "product", "character"];

fn database_error(error: impl std::fmt::Display) -> PackageError {
    PackageError::Database(error.to_string())
}

fn zip_error(error: zip::result::ZipError) -> PackageError {
    match error {
        zip::result::ZipError::Io(error) => PackageError::Io(error),
        zip::result::ZipError::InvalidArchive(_) | zip::result::ZipError::UnsupportedArchive(_) => {
            PackageError::NotAPackage
        }
        other => PackageError::Damaged(other.to_string()),
    }
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// A foreign key, as SQLite lists it.
#[derive(Debug, Clone)]
struct ForeignKey {
    parent: String,
    from: Vec<String>,
    to: Vec<String>,
    cascade: bool,
}

impl ForeignKey {
    /// Whether the row it is on belongs to the row it points at: deleted
    /// with it, or owned by a project.
    fn owns(&self) -> bool {
        self.parent != "user_profile" && (self.cascade || self.from == ["owner_project_id"])
    }

    /// `c.a = p.x AND c.b = p.y`.
    fn joined(&self, child: &str, parent: &str) -> String {
        self.from
            .iter()
            .zip(&self.to)
            .map(|(from, to)| format!("{child}.{} = {parent}.{}", quoted(from), quoted(to)))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

#[derive(Debug, Clone)]
struct Table {
    name: String,
    columns: Vec<String>,
    nullable: HashSet<String>,
    keys: Vec<ForeignKey>,
}

impl Table {
    fn column_list(&self) -> String {
        self.columns
            .iter()
            .map(|column| quoted(column))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn travels(&self) -> bool {
        !LEFT_OUT.contains(&self.name.as_str())
    }
}

/// The tables of `schema` (`main`, or an attached one), with their columns
/// and foreign keys.
fn tables(conn: &Connection, schema: &str) -> rusqlite::Result<Vec<Table>> {
    let names: Vec<String> = conn
        .prepare(&format!(
            "SELECT name FROM {schema}.sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        ))?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        let mut columns = Vec::new();
        let mut nullable = HashSet::new();
        let mut statement =
            conn.prepare(&format!("PRAGMA {schema}.table_info({})", quoted(&name)))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let column: String = row.get(1)?;
            let not_null: bool = row.get(3)?;
            let primary: i64 = row.get(5)?;
            if !not_null && primary == 0 {
                nullable.insert(column.clone());
            }
            columns.push(column);
        }
        let mut keys: Vec<(i64, ForeignKey)> = Vec::new();
        let mut statement = conn.prepare(&format!(
            "PRAGMA {schema}.foreign_key_list({})",
            quoted(&name)
        ))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let parent: String = row.get(2)?;
            let from: String = row.get(3)?;
            let to: Option<String> = row.get(4)?;
            let on_delete: String = row.get(6)?;
            let to = to.unwrap_or_else(|| "id".to_owned());
            match keys.iter_mut().find(|(key, _)| *key == id) {
                Some((_, key)) => {
                    key.from.push(from);
                    key.to.push(to);
                }
                None => keys.push((
                    id,
                    ForeignKey {
                        parent,
                        from: vec![from],
                        to: vec![to],
                        cascade: on_delete.eq_ignore_ascii_case("CASCADE"),
                    },
                )),
            }
        }
        tables.push(Table {
            name,
            columns,
            nullable,
            keys: keys.into_iter().map(|(_, key)| key).collect(),
        });
    }
    Ok(tables)
}

/// The tables a channel's work reaches by owning keys: a global item's
/// parts never pull rows from them (an ad angle of a global product is
/// another project's).
fn work_tables(tables: &[Table]) -> HashSet<String> {
    let mut work: HashSet<String> = HashSet::from(["channel".to_owned()]);
    loop {
        let before = work.len();
        for table in tables {
            if table
                .keys
                .iter()
                .any(|key| key.owns() && work.contains(&key.parent))
            {
                work.insert(table.name.clone());
            }
        }
        if work.len() == before {
            return work;
        }
    }
}

/// Copies into `main` the rows of `src` that `key` ties to rows `main`
/// already holds; how many came.
fn pull_children(conn: &Connection, table: &Table, key: &ForeignKey) -> rusqlite::Result<usize> {
    let columns = table.column_list();
    conn.execute(
        &format!(
            "INSERT OR IGNORE INTO main.{t} ({columns}) SELECT {columns} FROM src.{t} c
             WHERE EXISTS (SELECT 1 FROM main.{p} p WHERE {on})",
            t = quoted(&table.name),
            p = quoted(&key.parent),
            on = key.joined("c", "p"),
        ),
        [],
    )
}

/// Copies into `main` the rows of `src`'s `parent` that `main`'s rows of
/// `table` point at with `key`; how many came.
fn pull_parents(
    conn: &Connection,
    table: &Table,
    key: &ForeignKey,
    parent: &Table,
) -> rusqlite::Result<usize> {
    let columns = parent.column_list();
    // A library item comes only when it is global or the project's own:
    // another project's never travels (a moved channel's pick of it).
    let owned = if parent
        .columns
        .iter()
        .any(|column| column == "owner_project_id")
    {
        " AND (p.owner_project_id IS NULL
               OR p.owner_project_id IN (SELECT id FROM main.project))"
    } else {
        ""
    };
    conn.execute(
        &format!(
            "INSERT OR IGNORE INTO main.{p} ({columns}) SELECT {columns} FROM src.{p} p
             WHERE EXISTS (SELECT 1 FROM main.{t} c WHERE {on}){owned}",
            t = quoted(&table.name),
            p = quoted(&parent.name),
            on = key.joined("c", "p"),
        ),
        [],
    )
}

/// Leaves no reference pointing nowhere: a nullable one is cleared, a row
/// that cannot be without it goes (and what hangs from it, next round).
fn settle_references(conn: &Connection, tables: &[Table]) -> rusqlite::Result<()> {
    loop {
        let mut changed = 0;
        for table in tables {
            for key in &table.keys {
                let set = key
                    .from
                    .iter()
                    .map(|from| format!("c.{} IS NOT NULL", quoted(from)))
                    .collect::<Vec<_>>()
                    .join(" AND ");
                let dangling = format!(
                    "{set} AND NOT EXISTS (SELECT 1 FROM main.{p} p WHERE {on})",
                    p = quoted(&key.parent),
                    on = key.joined("c", "p"),
                );
                let t = quoted(&table.name);
                // An item of a project that is not here is not made global:
                // it goes.
                let owner = key.from == ["owner_project_id"];
                if !owner && key.from.iter().all(|from| table.nullable.contains(from)) {
                    let cleared = key
                        .from
                        .iter()
                        .map(|from| format!("{} = NULL", quoted(from)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    changed += conn.execute(
                        &format!("UPDATE main.{t} AS c SET {cleared} WHERE {dangling}"),
                        [],
                    )?;
                } else {
                    changed +=
                        conn.execute(&format!("DELETE FROM main.{t} AS c WHERE {dangling}"), [])?;
                }
            }
        }
        if changed == 0 {
            return Ok(());
        }
    }
}

/// Writes `project`'s records from `db` into a new SQLite file at `to`:
/// the current schema, only what belongs to the project and the global
/// items it uses, checked and vacuumed (no page of anything else
/// survives).
fn write_records_file(db: &Database, project: ProjectId, to: &Path) -> Result<(), PackageError> {
    // The whole profile's copy stays in this computer's temporary folder,
    // never beside the package or in a project folder (which a sync client
    // may upload, or the Recycle Bin keep).
    let scratch = Scratch::new();
    let source = scratch.0.clone();
    let _ = fs::remove_file(to);
    let result = (|| {
        {
            let mut copy = Connection::open(&source).map_err(database_error)?;
            copy_whole(db, &mut copy).map_err(|error| PackageError::Database(error.to_string()))?;
            copy.close().map_err(|(_, error)| database_error(error))?;
        }
        let mut conn = Connection::open(to).map_err(database_error)?;
        migrations::run(&mut conn).map_err(database_error)?;
        sharing::drop_mirrors(&conn).map_err(database_error)?;
        // Rows come over in no particular order; references are settled
        // once all are in.
        conn.pragma_update(None, "foreign_keys", "OFF")
            .map_err(database_error)?;
        let own = tables(&conn, "main").map_err(database_error)?;
        for table in &own {
            conn.execute(&format!("DELETE FROM main.{}", quoted(&table.name)), [])
                .map_err(database_error)?;
        }
        conn.execute(
            "ATTACH DATABASE ?1 AS src",
            [source.to_string_lossy().as_ref()],
        )
        .map_err(database_error)?;
        select(&conn, &own, project).map_err(database_error)?;
        conn.execute_batch("DETACH DATABASE src")
            .map_err(database_error)?;
        tidy(&conn, &own).map_err(database_error)?;
        conn.pragma_update(None, "journal_mode", "DELETE")
            .map_err(database_error)?;
        conn.execute_batch("VACUUM").map_err(database_error)?;
        let check: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(database_error)?;
        if check != "ok" {
            return Err(PackageError::Damaged(check));
        }
        conn.close().map_err(|(_, error)| database_error(error))
    })();
    drop(scratch);
    if result.is_err() {
        let _ = fs::remove_file(to);
    }
    result
}

/// Copies the project's rows from `src` into `main`.
fn select(conn: &Connection, tables: &[Table], project: ProjectId) -> rusqlite::Result<()> {
    let by_name: HashMap<&str, &Table> = tables.iter().map(|t| (t.name.as_str(), t)).collect();
    let project_id = project.to_string();
    let copy_where = |name: &str, filter: &str| -> rusqlite::Result<usize> {
        let columns = by_name[name].column_list();
        conn.execute(
            &format!(
                "INSERT OR IGNORE INTO main.{t} ({columns}) SELECT {columns} FROM src.{t} WHERE {filter}",
                t = quoted(name)
            ),
            [&project_id],
        )
    };
    copy_where("project", "id = ?1")?;
    copy_where(
        "user_profile",
        "id = (SELECT profile_id FROM src.project WHERE id = ?1)",
    )?;
    // Done jobs only: a queued or failed one brought in could run (and pay)
    // again elsewhere.
    copy_where("job", "owner_project_id = ?1 AND state = 'done'")?;

    let travelling: Vec<&Table> = tables.iter().filter(|table| table.travels()).collect();
    // What the project owns.
    loop {
        let mut came = 0;
        for table in &travelling {
            for key in table.keys.iter().filter(|key| key.owns()) {
                if by_name
                    .get(key.parent.as_str())
                    .is_some_and(|parent| parent.travels())
                {
                    came += pull_children(conn, table, key)?;
                }
            }
        }
        if came == 0 {
            break;
        }
    }
    // The library items it uses that it does not own, and their parts.
    let work = work_tables(tables);
    loop {
        let mut came = 0;
        for table in &travelling {
            for key in &table.keys {
                // A linked copy's original stays where it is.
                if !LIBRARY.contains(&key.parent.as_str()) || key.from == ["linked_from"] {
                    continue;
                }
                came += pull_parents(conn, table, key, by_name[key.parent.as_str()])?;
            }
        }
        for table in travelling
            .iter()
            .filter(|table| !work.contains(&table.name))
        {
            for key in table.keys.iter().filter(|key| key.owns()) {
                let library_part = LIBRARY.contains(&key.parent.as_str());
                let part_of_part = key.parent != "project"
                    && by_name
                        .get(key.parent.as_str())
                        .is_some_and(|parent| !work.contains(&parent.name) && parent.travels());
                if library_part || part_of_part {
                    came += pull_children(conn, table, key)?;
                }
            }
        }
        if came == 0 {
            break;
        }
    }
    // The global template a kind uses now: its latest version, beside the
    // ones the project's generations were written with.
    if by_name.contains_key("template_version") {
        let columns = by_name["template_version"].column_list();
        conn.execute(
            &format!(
                "INSERT OR IGNORE INTO main.template_version ({columns})
                 SELECT {columns} FROM src.template_version t
                 WHERE t.owner_project_id IS NULL AND t.retired_at IS NULL
                   AND t.profile_id = (SELECT profile_id FROM main.project)
                   AND t.kind IN (SELECT kind FROM main.template_version
                                  WHERE owner_project_id IS NULL)
                   AND t.number = (SELECT MAX(number) FROM src.template_version u
                                   WHERE u.profile_id = t.profile_id AND u.kind = t.kind
                                     AND u.owner_project_id IS NULL AND u.retired_at IS NULL)"
            ),
            [],
        )?;
    }
    Ok(())
}

/// Leaves the selected records fit to travel: no folder, star or link to
/// another project, no post that was never made, no lease, and none of the
/// YouTube data that expires.
fn tidy(conn: &Connection, tables: &[Table]) -> rusqlite::Result<()> {
    conn.execute_batch(
        "UPDATE project SET folder = NULL, favorite_position = NULL, opened_at = NULL,
                            state = 'active';
         UPDATE persona SET linked_from = NULL, follows_name = NULL, follows_project = NULL;
         UPDATE product SET linked_from = NULL, follows_name = NULL, follows_project = NULL;
         UPDATE character SET linked_from = NULL, follows_name = NULL, follows_project = NULL;
         UPDATE product_image SET copied_from = NULL;
         UPDATE product_claim SET copied_from = NULL;
         UPDATE product_concept SET copied_from = NULL;
         UPDATE reference_picture SET copied_from = NULL;
         UPDATE template_share SET linked_from = NULL, follows_project = NULL;
         UPDATE video_project SET loose_folder = NULL;
         DELETE FROM publication
         WHERE post_id IS NULL
            OR NOT (kind = 'manual' OR upload_status IN ('published', 'restricted'));
         UPDATE publication SET upload_claimed_at = NULL;
         UPDATE job SET leased_by = NULL, leased_until = NULL, checkpoint = NULL,
                        external_handle = NULL;
         UPDATE user_profile SET base_folder = NULL;
         UPDATE take_result SET cost_micros = NULL;",
    )?;
    // No connection travels: every YouTube number goes, as a backup leaves
    // out an unconnected channel's.
    crate::youtube_data::strip_for_backup(conn)?;
    settle_references(conn, tables)
}

fn count(conn: &Connection, sql: &str) -> rusqlite::Result<usize> {
    conn.query_row(sql, [], |row| row.get::<_, i64>(0))
        .map(|n| usize::try_from(n).unwrap_or(0))
}

/// The template kinds the records carry a global template of, and the
/// project has none of its own: the ones the import offers to tick.
fn offered_templates(conn: &Connection) -> rusqlite::Result<Vec<TemplateKind>> {
    let codes: Vec<String> = conn
        .prepare(
            "SELECT DISTINCT kind FROM template_version
             WHERE owner_project_id IS NULL
               AND kind NOT IN (SELECT kind FROM template_version
                                WHERE owner_project_id IS NOT NULL AND retired_at IS NULL)
             ORDER BY kind",
        )?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(codes.iter().filter_map(|code| code.parse().ok()).collect())
}

/// What the records hold, as a manifest says it.
fn contents(conn: &Connection) -> rusqlite::Result<PackageContents> {
    Ok(PackageContents {
        channels: count(conn, "SELECT COUNT(*) FROM channel")?,
        videos: count(conn, "SELECT COUNT(*) FROM video_project")?,
        world_items: count(conn, "SELECT COUNT(*) FROM character")?,
        narrators: count(conn, "SELECT COUNT(*) FROM persona")?,
        products: count(conn, "SELECT COUNT(*) FROM product")?,
        global_templates: offered_templates(conn)?,
        media_files: 0,
        media_bytes: 0,
    })
}

/// The manifest of a records file: its project and what it holds.
fn records_manifest(path: &Path) -> Result<PackageManifest, PackageError> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| PackageError::NotAPackage)?;
    let schema: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(database_error)?;
    let (id, name, color): (String, String, String) = conn
        .query_row("SELECT id, name, color FROM project", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|_| PackageError::NotAPackage)?;
    Ok(PackageManifest {
        format: PACKAGE_FORMAT,
        app_version: String::new(),
        schema: u32::try_from(schema).unwrap_or(0),
        exported_at: SystemTime::now(),
        project: ProjectId::from(Uuid::parse_str(&id).map_err(|_| PackageError::NotAPackage)?),
        name,
        color: ProjectColor::from_code(&color),
        contents: contents(&conn).map_err(database_error)?,
    })
}

/// Bytes copied, reported as they go; stops when told.
struct Copier<'a> {
    done: u64,
    total: u64,
    progress: &'a mut dyn FnMut(u64, u64) -> bool,
}

impl Copier<'_> {
    fn copy(&mut self, from: &mut impl Read, to: &mut impl Write) -> Result<(), PackageError> {
        let mut buffer = vec![0; CHUNK];
        loop {
            let read = from.read(&mut buffer)?;
            if read == 0 {
                return Ok(());
            }
            to.write_all(&buffer[..read])?;
            self.done += read as u64;
            if !(self.progress)(self.done.min(self.total), self.total) {
                return Err(PackageError::Stopped);
            }
        }
    }
}

/// A folder or a file of media, and where it goes in the package.
fn media_under(path: &Path, entry: &str, files: &mut Vec<MediaFile>) -> Result<(), PackageError> {
    if path.is_dir() {
        walk(path, entry, files).map_err(|error| PackageError::Damaged(error.to_string()))?;
    }
    Ok(())
}

/// The safe relative path of a package entry under `prefix`: `None` for
/// any other entry, or one that would leave its folder.
fn entry_under<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = name.strip_prefix(prefix)?.strip_prefix('/')?;
    let safe = !rest.is_empty()
        && rest
            .split('/')
            .all(|part| fine_part(part) && !part.contains('\\'));
    safe.then_some(rest)
}

/// A file or folder name Windows takes as it is, inside its folder: no
/// `.` or `..`, no drive or stream (`:`), no device name (`CON`, `NUL`,
/// `COM1`…), no trailing dot or space.
fn fine_part(part: &str) -> bool {
    const DEVICES: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = part.split('.').next().unwrap_or(part).trim_end();
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.contains(':')
        && !part.ends_with(['.', ' '])
        && !DEVICES
            .iter()
            .any(|device| stem.eq_ignore_ascii_case(device))
}

fn join_relative(base: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

/// The plan of an import: each old id's new one, and where each video's
/// media goes.
struct Plan {
    ids: HashMap<Uuid, Uuid>,
    /// Old video id, then its folder's name.
    videos: HashMap<Uuid, String>,
    name: String,
    skipped_posts: usize,
    renamed_channels: Vec<(String, String)>,
}

impl Plan {
    fn rewrite(&self, text: &str) -> Option<String> {
        rewrite_ids(text, &|id| self.ids.get(&id).copied())
    }

    fn rewritten(&self, text: &str) -> String {
        self.rewrite(text).unwrap_or_else(|| text.to_owned())
    }
}

/// What a records file is brought in as.
struct Bringing<'a> {
    owner: bardo_domain::ProfileId,
    project: ProjectId,
    name: &'a str,
    color: Option<ProjectColor>,
    folder: &'a Path,
    templates: &'a [TemplateKind],
    /// Comes back archived: a deleted archived project's folder whose
    /// media is still packed in `Videos.zip`, which *Unarchive* unpacks.
    archived: bool,
}

/// Drops every trigger of the database: the records being readied need
/// none, and a package's own could undo what is checked.
fn drop_triggers(conn: &Connection) -> rusqlite::Result<()> {
    let names: Vec<String> = conn
        .prepare("SELECT name FROM main.sqlite_master WHERE type = 'trigger'")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for name in names {
        conn.execute(&format!("DROP TRIGGER main.{}", quoted(&name)), [])?;
    }
    Ok(())
}

/// Columns naming a file or a folder inside a project's folder: joined to
/// it later, so a package's own values must stay inside.
fn names_a_file(table: &str, column: &str) -> bool {
    // `capture.file_name` is the name the user's recording had: shown,
    // never joined to a folder.
    table != "export"
        && table != "user_profile"
        && (column == "file" || column.ends_with("_file") || column == "folder_name")
}

/// Whether `name` is a relative path that stays under its folder on
/// Windows and elsewhere.
fn stays_inside(name: &str) -> bool {
    name.is_empty()
        || !name.starts_with('/')
            && !name.starts_with('\\')
            && name
                .split(['/', '\\'])
                .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(':'))
}

/// Treats a package as anything a file can hold: one project; nothing of
/// what never travels (an unfinished job would run, and pay, here; a
/// connection would read as signed in); the same tidying as an export;
/// and no file name that would lead out of the project's folder.
fn untrusted(conn: &Connection, tables: &[Table]) -> Result<(), PackageError> {
    let projects = count(conn, "SELECT COUNT(*) FROM project").map_err(database_error)?;
    if projects != 1 {
        return Err(PackageError::Damaged(format!("{projects} projects")));
    }
    for table in tables {
        let name = table.name.as_str();
        if name == "job" {
            conn.execute("DELETE FROM job WHERE state <> 'done'", [])
                .map_err(database_error)?;
        } else if name != "user_profile" && !table.travels() {
            conn.execute(&format!("DELETE FROM main.{}", quoted(name)), [])
                .map_err(database_error)?;
        }
    }
    tidy(conn, tables).map_err(database_error)?;
    for table in tables {
        for column in table
            .columns
            .iter()
            .filter(|column| names_a_file(&table.name, column))
        {
            let values: Vec<String> = conn
                .prepare(&format!(
                    "SELECT {c} FROM main.{t} WHERE typeof({c}) = 'text'",
                    c = quoted(column),
                    t = quoted(&table.name)
                ))
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| row.get(0))?
                        .collect::<rusqlite::Result<_>>()
                })
                .map_err(database_error)?;
            if let Some(bad) = values.iter().find(|value| !stays_inside(value)) {
                return Err(PackageError::Damaged(format!(
                    "{}.{column} leaves its folder: {bad}",
                    table.name
                )));
            }
        }
    }
    Ok(())
}

/// Readies the records at `path` (a copy of the package's) to come into
/// `live`: migrated to this schema, the carried items made the project's,
/// names that would clash changed, posts the profile has left out; then
/// gives each id its new one.
fn prepare(live: &Database, path: &Path, bringing: &Bringing<'_>) -> Result<Plan, PackageError> {
    let mut conn = Connection::open(path).map_err(database_error)?;
    let schema: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(database_error)?;
    if usize::try_from(schema).unwrap_or(usize::MAX) > SCHEMA_VERSION {
        return Err(PackageError::Newer);
    }
    sharing::drop_mirrors(&conn).map_err(database_error)?;
    migrations::run(&mut conn).map_err(|error| PackageError::Damaged(error.to_string()))?;
    // No trigger of the package's fires on the changes made here: every
    // one goes before anything is checked.
    drop_triggers(&conn).map_err(database_error)?;
    conn.pragma_update(None, "foreign_keys", "OFF")
        .map_err(database_error)?;
    let own = tables(&conn, "main").map_err(database_error)?;
    untrusted(&conn, &own)?;

    let (old_project, old_owner): (String, String) = conn
        .query_row("SELECT id, profile_id FROM project", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|_| PackageError::NotAPackage)?;
    let owner = bringing.owner.to_string();
    // What the profile has already, read once.
    let live_names = |sql: &str| -> Result<HashSet<String>, PackageError> {
        live.conn()
            .prepare(sql)
            .and_then(|mut statement| {
                statement
                    .query_map([&owner], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<HashSet<String>>>()
            })
            .map_err(database_error)
    };
    let lower = |names: HashSet<String>| -> HashSet<String> {
        names.into_iter().map(|name| name.to_lowercase()).collect()
    };
    let projects = lower(live_names(
        "SELECT name FROM project WHERE profile_id = ?1",
    )?);
    let mut channel_names = lower(live_names(
        "SELECT name FROM channel WHERE profile_id = ?1",
    )?);
    let global_templates = live_names(
        "SELECT id FROM template_version WHERE profile_id = ?1 AND owner_project_id IS NULL",
    )?;
    let posts = live_names(
        "SELECT network || ' ' || post_id FROM publication
         WHERE profile_id = ?1 AND post_id IS NOT NULL",
    )?;

    // The project's name, or the first free one after it.
    let name = free_name(bringing.name.trim(), |candidate| {
        projects.contains(&candidate.to_lowercase())
    });
    let now = to_unix_millis(SystemTime::now());
    conn.execute(
        "UPDATE project SET name = ?1, folder = ?2, state = ?4, opened_at = NULL,
                            favorite_position = NULL, created_at = ?3",
        params![
            name,
            bringing.folder.to_string_lossy(),
            now,
            if bringing.archived {
                "archived"
            } else {
                "active"
            }
        ],
    )
    .map_err(database_error)?;
    if let Some(color) = bringing.color {
        conn.execute("UPDATE project SET color = ?1", [color.code()])
            .map_err(database_error)?;
    }

    // Carried global items become the project's own, renamed where the
    // project has one of that name.
    for table in NAMED {
        let kind = if table == "character" { "kind" } else { "''" };
        let carried: Vec<(String, String, String)> = conn
            .prepare(&format!(
                "SELECT id, name, {kind} FROM {table} WHERE owner_project_id IS NULL ORDER BY name"
            ))
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                    .collect::<rusqlite::Result<_>>()
            })
            .map_err(database_error)?;
        for (id, item, kind_value) in carried {
            let taken = |candidate: &str| {
                conn.query_row(
                    &format!(
                        "SELECT EXISTS (SELECT 1 FROM {table} WHERE owner_project_id = ?1
                         AND name = ?2 COLLATE NOCASE AND {kind} = ?3)"
                    ),
                    params![old_project, candidate, kind_value],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap_or(true)
            };
            let free = free_name(&item, taken);
            conn.execute(
                &format!(
                    "UPDATE {table} SET owner_project_id = ?1, name = ?2, shareable = 0
                     WHERE id = ?3"
                ),
                params![old_project, free, id],
            )
            .map_err(database_error)?;
        }
    }

    // Carried template versions: ticked kinds become the project's
    // template; the others stay references where this profile has them,
    // else come in retired, so the generations keep what they were written
    // with and nothing wins over the global template.
    let offered = offered_templates(&conn).map_err(database_error)?;
    let mut kept_references = Vec::new();
    let carried: Vec<(String, String)> = conn
        .prepare(
            "SELECT id, kind FROM template_version WHERE owner_project_id IS NULL
             ORDER BY kind, number",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()
        })
        .map_err(database_error)?;
    for (id, kind) in carried {
        let ticked = kind
            .parse::<TemplateKind>()
            .is_ok_and(|kind| offered.contains(&kind) && bringing.templates.contains(&kind));
        if !ticked && global_templates.contains(&id) {
            kept_references.push(id);
            continue;
        }
        let number: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(number), 0) + 1 FROM template_version
                 WHERE owner_project_id = ?1 AND kind = ?2",
                params![old_project, kind],
                |row| row.get(0),
            )
            .map_err(database_error)?;
        conn.execute(
            "UPDATE template_version SET owner_project_id = ?1, number = ?2, retired_at = ?3
             WHERE id = ?4",
            params![old_project, number, (!ticked).then_some(now), id],
        )
        .map_err(database_error)?;
    }

    // Channels keep their names unless the profile has them.
    let channels: Vec<(String, String)> = conn
        .prepare("SELECT id, name FROM channel ORDER BY name")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()
        })
        .map_err(database_error)?;
    let mut renamed_channels = Vec::new();
    for (id, channel) in channels {
        let taken = |candidate: &str| channel_names.contains(&candidate.to_lowercase());
        let new_name = if taken(&channel) {
            free_name(&format!("{channel} ({name})"), taken)
        } else {
            channel.clone()
        };
        channel_names.insert(new_name.to_lowercase());
        if new_name != channel {
            conn.execute(
                "UPDATE channel SET name = ?1 WHERE id = ?2",
                params![new_name, id],
            )
            .map_err(database_error)?;
            renamed_channels.push((channel, new_name));
        }
    }

    // A post the profile has already comes in once.
    let skipped_posts = conn
        .prepare("SELECT id, network || ' ' || post_id FROM publication WHERE post_id IS NOT NULL")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(database_error)?
        .into_iter()
        .filter(|(_, post)| posts.contains(post))
        .map(|(id, _)| conn.execute("DELETE FROM publication WHERE id = ?1", [&id]))
        .sum::<rusqlite::Result<usize>>()
        .map_err(database_error)?;

    conn.execute(
        "UPDATE video_project SET folder_project_id = ?1, loose_folder = NULL",
        [&old_project],
    )
    .map_err(database_error)?;
    // What a post left out left hanging goes with it.
    settle_references(&conn, &own).map_err(database_error)?;
    // The versions this profile has stay out, their generations pointing
    // at them as they are: kept out of the new ids too.
    for id in kept_references {
        conn.execute("DELETE FROM template_version WHERE id = ?1", [&id])
            .map_err(database_error)?;
    }

    // Every id gets a new one; the project the one chosen, the profile the
    // one bringing it in.
    let mut ids: HashMap<Uuid, Uuid> = HashMap::new();
    for table in own.iter().filter(|table| table.name != "user_profile") {
        for column in table.columns.iter().filter(|column| *column == "id") {
            let values: Vec<String> = conn
                .prepare(&format!(
                    "SELECT {} FROM {} WHERE typeof({0}) = 'text'",
                    quoted(column),
                    quoted(&table.name)
                ))
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| row.get(0))?
                        .collect::<rusqlite::Result<_>>()
                })
                .map_err(database_error)?;
            for value in values {
                if let Ok(old) = Uuid::parse_str(&value) {
                    ids.entry(old).or_insert_with(Uuid::new_v4);
                }
            }
        }
    }
    if let Ok(old) = Uuid::parse_str(&old_project) {
        ids.insert(old, bringing.project.as_uuid());
    }
    if let Ok(old) = Uuid::parse_str(&old_owner) {
        ids.insert(old, bringing.owner.as_uuid());
    }

    // Each video's folder name: kept, or made from its title and new id.
    let rows: Vec<(String, String, Option<String>)> = conn
        .prepare("SELECT id, title, folder_name FROM video_project")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                .collect::<rusqlite::Result<_>>()
        })
        .map_err(database_error)?;
    let mut videos = HashMap::new();
    for (id, title, folder) in rows {
        let Ok(old) = Uuid::parse_str(&id) else {
            continue;
        };
        let folder = match folder {
            Some(folder) if !folder.is_empty() => folder,
            _ => {
                let new = ids.get(&old).copied().unwrap_or(old);
                let made = video_folder_name(&title, new.into());
                conn.execute(
                    "UPDATE video_project SET folder_name = ?1 WHERE id = ?2",
                    params![made, id],
                )
                .map_err(database_error)?;
                made
            }
        };
        videos.insert(old, folder);
    }
    conn.close().map_err(|(_, error)| database_error(error))?;
    Ok(Plan {
        ids,
        videos,
        name,
        skipped_posts,
        renamed_channels,
    })
}

/// Brings the prepared records at `path` into `live`, every id rewritten,
/// in one transaction checked for references pointing nowhere. Rolls back
/// when `proceed` says to stop just before the commit.
fn bring_in(
    live: &Database,
    path: &Path,
    plan: &Plan,
    proceed: &mut dyn FnMut() -> bool,
) -> Result<(), PackageError> {
    let conn = live.conn();
    conn.execute(
        "ATTACH DATABASE ?1 AS pkg",
        [path.to_string_lossy().as_ref()],
    )
    .map_err(database_error)?;
    let result = (|| {
        let theirs = tables(&conn, "pkg").map_err(database_error)?;
        let ours: HashMap<String, Table> = tables(&conn, "main")
            .map_err(database_error)?
            .into_iter()
            .map(|table| (table.name.clone(), table))
            .collect();
        conn.execute_batch("BEGIN IMMEDIATE; PRAGMA defer_foreign_keys = ON;")
            .map_err(database_error)?;
        let mut filled = Vec::new();
        let inserted = (|| {
            for table in theirs.iter().filter(|table| table.name != "user_profile") {
                let Some(target) = ours.get(&table.name) else {
                    continue;
                };
                let columns: Vec<&String> = table
                    .columns
                    .iter()
                    .filter(|column| target.columns.contains(column))
                    .collect();
                if columns.is_empty() {
                    continue;
                }
                let list = columns
                    .iter()
                    .map(|column| quoted(column))
                    .collect::<Vec<_>>()
                    .join(", ");
                let marks = (1..=columns.len())
                    .map(|n| format!("?{n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut insert = conn.prepare(&format!(
                    "INSERT INTO main.{t} ({list}) VALUES ({marks})",
                    t = quoted(&table.name)
                ))?;
                let mut read =
                    conn.prepare(&format!("SELECT {list} FROM pkg.{}", quoted(&table.name)))?;
                let mut rows = read.query([])?;
                while let Some(row) = rows.next()? {
                    let values = (0..columns.len())
                        .map(|index| {
                            Ok(match row.get::<_, Value>(index)? {
                                Value::Text(text) => Value::Text(plan.rewritten(&text)),
                                other => other,
                            })
                        })
                        .collect::<rusqlite::Result<Vec<Value>>>()?;
                    insert.execute(params_from_iter(values))?;
                    if filled.last() != Some(&table.name) {
                        filled.push(table.name.clone());
                    }
                }
            }
            // Only the tables it filled: a reference broken elsewhere in
            // the profile is not this package's.
            for table in &filled {
                let dangling: Option<String> = conn
                    .query_row(
                        &format!("PRAGMA main.foreign_key_check({})", quoted(table)),
                        [],
                        |row| row.get(0),
                    )
                    .optional()?;
                if dangling.is_some() {
                    return Ok(dangling);
                }
            }
            Ok::<_, rusqlite::Error>(None)
        })();
        match inserted {
            Ok(None) if proceed() => conn.execute_batch("COMMIT").map_err(database_error),
            Ok(None) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(PackageError::Stopped)
            }
            Ok(Some(table)) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(PackageError::Damaged(format!(
                    "a reference in {table} points nowhere"
                )))
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(database_error(error))
            }
        }
    })();
    // Left attached, the next import could not attach its own; but a
    // commit made stands, so it is only said.
    if let Err(error) = conn.execute_batch("DETACH DATABASE pkg") {
        tracing::warn!("could not detach an imported package's records: {error}");
    }
    result
}

/// A scratch copy of a records file, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("bardo-records-{}.db", Uuid::new_v4())))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut path = self.0.clone().into_os_string();
            path.push(suffix);
            let _ = fs::remove_file(path);
        }
    }
}

/// Whether `folder` is missing or holds nothing.
fn is_empty_folder(folder: &Path) -> io::Result<bool> {
    match fs::read_dir(folder) {
        Ok(mut entries) => Ok(entries.next().is_none()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

/// Writes `bardo-project.json` naming the new project.
fn write_manifest(folder: &Path, manifest: &ProjectManifest) -> io::Result<()> {
    fs::create_dir_all(folder.join(VIDEOS_DIR))?;
    fs::create_dir_all(folder.join(WORLD_DIR))?;
    let partial = folder.join(format!("{PROJECT_MANIFEST}.partial"));
    fs::write(&partial, manifest.to_json())?;
    fs::rename(&partial, folder.join(PROJECT_MANIFEST))
}

/// Project packages on this computer, over the app's database.
pub struct LocalPackages {
    db: Arc<Database>,
}

impl LocalPackages {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn project_exists(&self, project: ProjectId) -> Result<bool, PackageError> {
        self.db
            .conn()
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM project WHERE id = ?1)",
                [project.to_string()],
                |row| row.get(0),
            )
            .map_err(database_error)
    }

    fn project_name(&self, project: ProjectId) -> Result<String, PackageError> {
        self.db
            .conn()
            .query_row(
                "SELECT name FROM project WHERE id = ?1",
                [project.to_string()],
                |row| row.get(0),
            )
            .map_err(database_error)
    }

    fn write_package(
        &self,
        request: &PackageExport,
        part: &Path,
        records: &Path,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<PackageManifest, PackageError> {
        write_records_file(&self.db, request.project, records)?;
        let (mut manifest, videos, global_files) = {
            let conn =
                Connection::open_with_flags(records, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(database_error)?;
            let manifest = records_manifest(records)?;
            let videos: HashSet<String> = conn
                .prepare("SELECT id FROM video_project")
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| row.get(0))?
                        .collect::<rusqlite::Result<_>>()
                })
                .map_err(database_error)?;
            let global_files: Vec<String> = conn
                .prepare(
                    "SELECT r.file FROM reference_picture r
                     JOIN character c ON c.id = r.character_id
                     WHERE c.owner_project_id IS NULL AND r.file IS NOT NULL",
                )
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| row.get(0))?
                        .collect::<rusqlite::Result<_>>()
                })
                .map_err(database_error)?;
            (manifest, videos, global_files)
        };

        let mut media = Vec::new();
        for (video, folder) in &request.videos {
            if videos.contains(&video.to_string()) {
                media_under(folder, &format!("{VIDEOS_ENTRY}/{video}"), &mut media)?;
            }
        }
        if let Some(world) = &request.world {
            media_under(world, WORLD_ENTRY, &mut media)?;
        }
        for file in global_files {
            if file.contains(['/', '\\']) || file.contains("..") {
                continue;
            }
            let path = request.global_world.join(&file);
            if let Ok(meta) = fs::metadata(&path)
                && meta.is_file()
            {
                media.push(MediaFile {
                    path,
                    entry: format!("{GLOBAL_WORLD_ENTRY}/{file}"),
                    bytes: meta.len(),
                });
            }
        }
        media.sort_by(|a, b| a.entry.cmp(&b.entry));
        media.dedup_by(|a, b| a.entry == b.entry);

        manifest.app_version = request.app_version.clone();
        manifest.exported_at = SystemTime::now();
        manifest.schema = u32::try_from(SCHEMA_VERSION).unwrap_or(u32::MAX);
        manifest.contents.media_files = media.len() as u64;
        manifest.contents.media_bytes = media.iter().map(|file| file.bytes).sum();
        let records_bytes = fs::metadata(records)?.len();
        let mut copier = Copier {
            done: 0,
            total: records_bytes + manifest.contents.media_bytes,
            progress,
        };

        use zip::write::SimpleFileOptions;
        let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(part)?));
        let deflated =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        // Media is compressed already: stored, it costs no time.
        let stored = deflated.compression_method(zip::CompressionMethod::Stored);
        let sized = |options: SimpleFileOptions, bytes: u64| options.large_file(bytes >= 1 << 31);
        zip.start_file(PACKAGE_MANIFEST, deflated)
            .map_err(zip_error)?;
        zip.write_all(manifest.to_json().as_bytes())?;
        zip.start_file(RECORDS_ENTRY, sized(deflated, records_bytes))
            .map_err(zip_error)?;
        copier.copy(&mut File::open(records)?, &mut zip)?;
        for file in &media {
            // A file removed since the walk is left out; a folder gone
            // (a drive unplugged) stops the package, so none is written
            // without its media.
            let mut source = match File::open(&file.path) {
                Ok(source) => source,
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        && file.path.parent().is_some_and(Path::is_dir) =>
                {
                    continue;
                }
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
        Ok(manifest)
    }

    /// Extracts the package's media into the new project's folder: each
    /// video under its folder's name, the world's pictures in `World`,
    /// every name with its ids rewritten.
    fn extract_media(
        &self,
        archive: &mut zip::ZipArchive<BufReader<File>>,
        folder: &Path,
        plan: &Plan,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<(), PackageError> {
        let total: u64 = (0..archive.len())
            .filter_map(|index| archive.by_index_raw(index).ok().map(|entry| entry.size()))
            .sum();
        let mut copier = Copier {
            done: 0,
            total,
            progress,
        };
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(zip_error)?;
            if entry.is_dir() {
                continue;
            }
            let Ok(name) = entry.name().map(|name| name.into_owned()) else {
                continue;
            };
            let target = if let Some(rest) = entry_under(&name, VIDEOS_ENTRY) {
                let Some((video, inside)) = rest.split_once('/') else {
                    continue;
                };
                let Some(folder_name) = Uuid::parse_str(video)
                    .ok()
                    .and_then(|video| plan.videos.get(&video))
                else {
                    continue;
                };
                join_relative(
                    &folder.join(VIDEOS_DIR).join(folder_name),
                    &plan.rewritten(inside),
                )
            } else if let Some(rest) =
                entry_under(&name, WORLD_ENTRY).or_else(|| entry_under(&name, GLOBAL_WORLD_ENTRY))
            {
                join_relative(&folder.join(WORLD_DIR), &plan.rewritten(rest))
            } else {
                continue;
            };
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = BufWriter::new(File::create(&target)?);
            copier.copy(&mut entry, &mut out)?;
            out.flush()?;
        }
        Ok(())
    }
}

/// Opens a package: its manifest, checked.
fn open_package(
    path: &Path,
) -> Result<(zip::ZipArchive<BufReader<File>>, PackageManifest), PackageError> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file)).map_err(zip_error)?;
    let mut text = String::new();
    match archive.by_name(PACKAGE_MANIFEST) {
        Ok(entry) => {
            entry
                .take(256 * 1024)
                .read_to_string(&mut text)
                .map_err(|_| PackageError::NotAPackage)?;
        }
        Err(zip::result::ZipError::FileNotFound) => return Err(PackageError::NotAPackage),
        Err(error) => return Err(zip_error(error)),
    }
    let manifest = PackageManifest::parse(&text).ok_or(PackageError::NotAPackage)?;
    if manifest.format > PACKAGE_FORMAT
        || usize::try_from(manifest.schema).unwrap_or(usize::MAX) > SCHEMA_VERSION
    {
        return Err(PackageError::Newer);
    }
    Ok((archive, manifest))
}

impl ProjectPackages for LocalPackages {
    fn export(
        &self,
        request: &PackageExport,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<PackageManifest, PackageError> {
        let mut part = request.to.clone().into_os_string();
        part.push(".partial");
        let part = PathBuf::from(part);
        let records = request.to.with_extension("records.partial");
        let written = self.write_package(request, &part, &records, progress);
        let _ = fs::remove_file(&records);
        match written {
            Ok(manifest) => {
                if let Err(error) = fs::rename(&part, &request.to) {
                    let _ = fs::remove_file(&part);
                    return Err(error.into());
                }
                tracing::info!(
                    files = manifest.contents.media_files,
                    "exported a project package"
                );
                Ok(manifest)
            }
            Err(error) => {
                let _ = fs::remove_file(&part);
                Err(error)
            }
        }
    }

    fn inspect(&self, package: &Path) -> Result<PackageManifest, PackageError> {
        open_package(package).map(|(_, manifest)| manifest)
    }

    fn import(
        &self,
        request: &PackageImport,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<PackageImported, PackageError> {
        // Done already, by a run cut short after its commit.
        if self.project_exists(request.project)? {
            return Ok(PackageImported {
                project: request.project,
                name: self.project_name(request.project)?,
                skipped_posts: 0,
                renamed_channels: Vec::new(),
            });
        }
        let (mut archive, _) = open_package(&request.package)?;
        let scratch = Scratch::new();
        {
            let mut entry = archive
                .by_name(RECORDS_ENTRY)
                .map_err(|error| match error {
                    zip::result::ZipError::FileNotFound => PackageError::NotAPackage,
                    other => zip_error(other),
                })?;
            let mut out = BufWriter::new(File::create(&scratch.0)?);
            io::copy(&mut entry, &mut out)?;
            out.flush()?;
        }
        let plan = prepare(
            &self.db,
            &scratch.0,
            &Bringing {
                owner: request.owner,
                project: request.project,
                name: &request.name,
                color: Some(request.color),
                folder: &request.folder,
                templates: &request.templates,
                archived: false,
            },
        )?;
        if !is_empty_folder(&request.folder)? {
            // Left by a run of this same import cut short (Bardo closed,
            // or the computer went off): it starts over.
            let ours = fs::read_to_string(request.folder.join(PROJECT_MANIFEST))
                .ok()
                .and_then(|text| ProjectManifest::parse(&text))
                .is_some_and(|manifest| {
                    manifest.names(request.project, request.owner, &request.data_home)
                });
            if !ours {
                return Err(PackageError::FolderNotEmpty);
            }
            fs::remove_dir_all(&request.folder)?;
        }
        let made = (|| {
            write_manifest(
                &request.folder,
                &ProjectManifest {
                    project: request.project,
                    name: plan.name.clone(),
                    profile: request.owner,
                    data_home: request.data_home.clone(),
                },
            )?;
            self.extract_media(&mut archive, &request.folder, &plan, progress)?;
            bring_in(&self.db, &scratch.0, &plan, &mut || progress(1, 1))
        })();
        match made {
            Ok(()) => {}
            // Stopped: the caller knows whether the user cancelled (the
            // folder goes) or Bardo is closing (it stays, and the next run
            // starts over in it).
            Err(PackageError::Stopped) => return Err(PackageError::Stopped),
            Err(error) => {
                let _ = fs::remove_dir_all(&request.folder);
                return Err(error);
            }
        }
        tracing::info!(
            skipped = plan.skipped_posts,
            renamed = plan.renamed_channels.len(),
            "imported a project package"
        );
        Ok(PackageImported {
            project: request.project,
            name: plan.name,
            skipped_posts: plan.skipped_posts,
            renamed_channels: plan.renamed_channels,
        })
    }

    fn write_records(&self, project: ProjectId, folder: &Path) -> Result<(), PackageError> {
        // No folder on this computer's disks (one kept in memory, as the
        // app's tests keep them): nothing to write into.
        if !folder.is_dir() {
            return Ok(());
        }
        let part = folder.join(format!("{PROJECT_RECORDS}.partial"));
        let written = write_records_file(&self.db, project, &part)
            .and_then(|()| Ok(fs::rename(&part, folder.join(PROJECT_RECORDS))?));
        if written.is_err() {
            let _ = fs::remove_file(&part);
        }
        written
    }

    fn folder_records(&self, folder: &Path) -> Option<PackageManifest> {
        let path = folder.join(PROJECT_RECORDS);
        if !path.is_file() {
            return None;
        }
        records_manifest(&path).ok()
    }

    fn import_folder(&self, request: &FolderImport) -> Result<PackageImported, PackageError> {
        let records = request.folder.join(PROJECT_RECORDS);
        let scratch = Scratch::new();
        fs::copy(&records, &scratch.0)?;
        let plan = prepare(
            &self.db,
            &scratch.0,
            &Bringing {
                owner: request.owner,
                project: request.project,
                name: &request.name,
                color: None,
                folder: &request.folder,
                templates: &[],
                archived: request.folder.join(PACKED_VIDEOS).is_file(),
            },
        )?;
        // The files whose names carry an id follow first, and the folder
        // names the new project, so the records only go in once the disk
        // agrees with them; anything that fails puts it all back.
        let manifest_path = request.folder.join(PROJECT_MANIFEST);
        let old_manifest = fs::read(&manifest_path).ok();
        let packed = request.folder.join(PACKED_VIDEOS);
        let repacked = request.folder.join(format!("{PACKED_VIDEOS}.partial"));
        let mut renamed = Vec::new();
        let brought = (|| {
            for dir in [VIDEOS_DIR, WORLD_DIR] {
                rename_ids_in(&request.folder.join(dir), &plan, &mut renamed)?;
            }
            // Compressed media: its files' names follow inside a new copy
            // of the archive, swapped in once the records are.
            if packed.is_file() {
                rename_ids_in_zip(&packed, &repacked, &plan)?;
            }
            write_manifest(
                &request.folder,
                &ProjectManifest {
                    project: request.project,
                    name: plan.name.clone(),
                    profile: request.owner,
                    data_home: request.data_home.clone(),
                },
            )?;
            bring_in(&self.db, &scratch.0, &plan, &mut || true)
        })();
        if brought.is_ok()
            && repacked.is_file()
            && let Err(error) = fs::rename(&repacked, &packed)
        {
            tracing::warn!("could not swap in an opened folder's renamed archive: {error}");
        }
        if let Err(error) = brought {
            let _ = fs::remove_file(&repacked);
            for (old, new) in renamed.iter().rev() {
                let _ = fs::rename(new, old);
            }
            match &old_manifest {
                Some(old) => {
                    let _ = fs::write(&manifest_path, old);
                }
                None => {
                    let _ = fs::remove_file(&manifest_path);
                }
            }
            return Err(error);
        }
        // Gone, so opening the folder again relinks the project.
        if let Err(error) = fs::remove_file(&records) {
            tracing::warn!("could not remove an opened folder's records: {error}");
        }
        tracing::info!("opened a deleted project's folder as a new project");
        Ok(PackageImported {
            project: request.project,
            name: plan.name,
            skipped_posts: plan.skipped_posts,
            renamed_channels: plan.renamed_channels,
        })
    }
}

/// Copies the archive at `from` to `to`, each entry renamed where its
/// name carries an id that changed; the bytes are copied as they are.
fn rename_ids_in_zip(from: &Path, to: &Path, plan: &Plan) -> Result<(), PackageError> {
    let mut archive = zip::ZipArchive::new(BufReader::new(File::open(from)?)).map_err(zip_error)?;
    let mut out = zip::ZipWriter::new(BufWriter::new(File::create(to)?));
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(zip_error)?;
        let name = entry.name().map_err(zip_error)?.into_owned();
        let renamed = plan.rewrite(&name).unwrap_or(name);
        out.raw_copy_file_rename(entry, renamed)
            .map_err(zip_error)?;
    }
    out.finish()
        .map_err(zip_error)?
        .into_inner()
        .map_err(|error| error.into_error())?
        .sync_all()?;
    Ok(())
}

/// Renames every file under `dir` whose name carries an id that changed,
/// noting each rename (old path, new path) so it can be undone.
fn rename_ids_in(dir: &Path, plan: &Plan, renamed: &mut Vec<(PathBuf, PathBuf)>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            rename_ids_in(&path, plan, renamed)?;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(new) = plan.rewrite(&name) {
            let new = dir.join(new);
            fs::rename(&path, &new)?;
            renamed.push((path, new));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
