//! Project packages (issue #342, ADR-0047 item 4, ADR-0048): a project
//! carried to another computer or profile as one `.bardo-project` file,
//! and a deleted project's folder opened again.
//!
//! A package holds a manifest, the project's records in a SQLite file of
//! the schema of the Bardo that wrote it (with copies of every global item
//! the project uses) and its media. Never a key, a token, an app
//! credential or the YouTube data that expires (ADR-0012).

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::{ProfileId, ProjectColor, ProjectId, TemplateKind, VideoProjectId};

/// A project package's extension, without the dot.
pub const PACKAGE_EXTENSION: &str = "bardo-project";

/// The package layout this build writes. A higher one is a newer Bardo's.
pub const PACKAGE_FORMAT: u32 = 1;

/// The records a deleted project leaves in its folder, beside
/// `bardo-project.json`, so the folder opens again once restored from the
/// Recycle Bin.
pub const PROJECT_RECORDS: &str = "bardo-project.db";

/// What a package holds, as the import dialog lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageContents {
    pub channels: usize,
    pub videos: usize,
    /// Characters, places and objects.
    pub world_items: usize,
    pub narrators: usize,
    pub products: usize,
    /// The template kinds whose global template the project used, carried
    /// in the package: they come in as the project's own only when ticked.
    pub global_templates: Vec<TemplateKind>,
    pub media_files: u64,
    pub media_bytes: u64,
}

/// What a package says about itself: `bardo-package.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub format: u32,
    /// The version of the Bardo that wrote it.
    pub app_version: String,
    /// Its records' schema version.
    pub schema: u32,
    pub exported_at: SystemTime,
    /// The project's id where it was exported; it gets a new one when
    /// imported.
    pub project: ProjectId,
    pub name: String,
    pub color: ProjectColor,
    pub contents: PackageContents,
}

impl PackageManifest {
    pub fn to_json(&self) -> String {
        let millis = self
            .exported_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        let value = json!({
            "format": self.format,
            "app_version": self.app_version,
            "schema": self.schema,
            "exported_at": millis,
            "project": self.project.to_string(),
            "name": self.name,
            "color": self.color.code(),
            "contents": {
                "channels": self.contents.channels,
                "videos": self.contents.videos,
                "world_items": self.contents.world_items,
                "narrators": self.contents.narrators,
                "products": self.contents.products,
                "global_templates": self
                    .contents
                    .global_templates
                    .iter()
                    .map(|kind| kind.code())
                    .collect::<Vec<_>>(),
                "media_files": self.contents.media_files,
                "media_bytes": self.contents.media_bytes,
            },
        });
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }

    /// `None` when the text is no package manifest. Unknown template kinds
    /// (a newer Bardo's) are left out.
    pub fn parse(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        let number = |value: &Value, key: &str| value.get(key).and_then(Value::as_u64);
        let contents = value.get("contents")?;
        let count = |key: &str| number(contents, key).unwrap_or(0) as usize;
        Some(Self {
            format: u32::try_from(number(&value, "format")?).ok()?,
            app_version: value.get("app_version")?.as_str()?.to_owned(),
            schema: u32::try_from(number(&value, "schema")?).ok()?,
            exported_at: SystemTime::UNIX_EPOCH
                + Duration::from_millis(number(&value, "exported_at").unwrap_or(0)),
            project: ProjectId::from(Uuid::parse_str(value.get("project")?.as_str()?).ok()?),
            name: value.get("name")?.as_str()?.to_owned(),
            color: value
                .get("color")
                .and_then(Value::as_str)
                .map(ProjectColor::from_code)
                .unwrap_or_default(),
            contents: PackageContents {
                channels: count("channels"),
                videos: count("videos"),
                world_items: count("world_items"),
                narrators: count("narrators"),
                products: count("products"),
                global_templates: contents
                    .get("global_templates")
                    .and_then(Value::as_array)
                    .map(|kinds| {
                        kinds
                            .iter()
                            .filter_map(Value::as_str)
                            .filter_map(|code| code.parse().ok())
                            .collect()
                    })
                    .unwrap_or_default(),
                media_files: number(contents, "media_files").unwrap_or(0),
                media_bytes: number(contents, "media_bytes").unwrap_or(0),
            },
        })
    }
}

/// What to put in a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageExport {
    pub project: ProjectId,
    /// The package file to write; one already there is replaced only once
    /// the new one is whole.
    pub to: PathBuf,
    /// Each video's folder; one not on disk carries no media.
    pub videos: Vec<(VideoProjectId, PathBuf)>,
    /// The project's `World` folder.
    pub world: Option<PathBuf>,
    /// The global library's `World` folder, for the pictures of the global
    /// items the project uses.
    pub global_world: PathBuf,
    pub app_version: String,
}

/// A package to bring in as a new project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageImport {
    pub package: PathBuf,
    pub owner: ProfileId,
    /// The new project's id, chosen beforehand so a resumed import finds
    /// it done.
    pub project: ProjectId,
    pub name: String,
    pub color: ProjectColor,
    /// The new project's folder: made by the import; one already there
    /// must be empty.
    pub folder: PathBuf,
    /// The data home's folder name, for `bardo-project.json`.
    pub data_home: String,
    /// The carried global templates that come in as the project's own.
    pub templates: Vec<TemplateKind>,
}

/// A deleted project's folder to bring back as a new project, in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderImport {
    pub folder: PathBuf,
    pub owner: ProfileId,
    pub project: ProjectId,
    /// Its name, unless taken meanwhile (then `<name> (2)`…).
    pub name: String,
    pub data_home: String,
}

/// What an import brought in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageImported {
    pub project: ProjectId,
    /// The name it got: the one asked for, or numbered when taken.
    pub name: String,
    /// Posts left out because the profile already has them.
    pub skipped_posts: usize,
    /// Channels renamed because the profile has one of that name: the
    /// name in the package, then the new one.
    pub renamed_channels: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    /// The file is no project package.
    #[error("not a project package")]
    NotAPackage,
    /// A newer Bardo wrote it: its schema is ahead of this one's.
    #[error("written by a newer Bardo")]
    Newer,
    /// The package or the records do not read whole.
    #[error("damaged: {0}")]
    Damaged(String),
    /// Told to stop; nothing was kept.
    #[error("stopped")]
    Stopped,
    /// The new project's folder holds files already.
    #[error("the folder is not empty")]
    FolderNotEmpty,
    #[error("database: {0}")]
    Database(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl PackageError {
    /// Whether it is a problem of the package itself, not of this
    /// computer.
    pub fn is_package(&self) -> bool {
        matches!(
            self,
            PackageError::NotAPackage | PackageError::Newer | PackageError::Damaged(_)
        )
    }
}

/// Project packages on this computer: written, read and brought in.
pub trait ProjectPackages: Send + Sync {
    /// Writes the package. `progress` gets the bytes done and the total,
    /// and stops it when it returns false: nothing is left behind.
    fn export(
        &self,
        request: &PackageExport,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<PackageManifest, PackageError>;

    /// What the package says, without bringing anything in.
    fn inspect(&self, package: &Path) -> Result<PackageManifest, PackageError>;

    /// Brings the package in as a new project: its folder first, then the
    /// records in one transaction. Stopped, or failing, it leaves nothing.
    fn import(
        &self,
        request: &PackageImport,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<PackageImported, PackageError>;

    /// Writes the project's records into `folder` as [`PROJECT_RECORDS`],
    /// before the folder goes to the Recycle Bin.
    fn write_records(&self, project: ProjectId, folder: &Path) -> Result<(), PackageError>;

    /// What a folder's [`PROJECT_RECORDS`] says; `None` when it has none
    /// that reads.
    fn folder_records(&self, folder: &Path) -> Option<PackageManifest>;

    /// Brings a deleted project's folder back as a new project, its media
    /// left in place.
    fn import_folder(&self, request: &FolderImport) -> Result<PackageImported, PackageError>;
}

/// The name a package is saved under by default: the project's, made safe.
pub fn package_file_name(project: &str) -> String {
    format!(
        "{}.{PACKAGE_EXTENSION}",
        crate::safe_folder_name(project, 60, "Project")
    )
}

/// `name`, or `name (2)`, `name (3)`… the first one `taken` says is free.
pub fn free_name(name: &str, mut taken: impl FnMut(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    (2..)
        .map(|n| format!("{name} ({n})"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
}

/// Every id of `map` in `text` replaced by its new one: ids appear whole
/// in records, inside job payloads and in file names alike, so one rule
/// keeps them all pointing at each other.
pub fn rewrite_ids(text: &str, map: &dyn Fn(Uuid) -> Option<Uuid>) -> Option<String> {
    const LEN: usize = 36;
    let bytes = text.as_bytes();
    if bytes.len() < LEN {
        return None;
    }
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut at = 0;
    while at + LEN <= bytes.len() {
        if looks_like_uuid(&bytes[at..at + LEN])
            && let Ok(old) = Uuid::parse_str(&text[at..at + LEN])
            && let Some(new) = map(old)
        {
            let out = out.get_or_insert_with(|| String::with_capacity(text.len()));
            out.push_str(&text[copied..at]);
            out.push_str(&new.to_string());
            at += LEN;
            copied = at;
            continue;
        }
        at += 1;
    }
    let mut out = out?;
    out.push_str(&text[copied..]);
    Some(out)
}

fn looks_like_uuid(bytes: &[u8]) -> bool {
    bytes.iter().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => *byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    })
}

impl fmt::Display for PackageContents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} channels, {} videos, {} world items, {} narrators, {} products, {} media files",
            self.channels,
            self.videos,
            self.world_items,
            self.narrators,
            self.products,
            self.media_files
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_reads_back_what_it_wrote() {
        let manifest = PackageManifest {
            format: PACKAGE_FORMAT,
            app_version: "0.4.0".into(),
            schema: 86,
            exported_at: SystemTime::UNIX_EPOCH + Duration::from_millis(1_790_000_000_000),
            project: ProjectId::new(),
            name: "Space & Sea".into(),
            color: ProjectColor::Teal,
            contents: PackageContents {
                channels: 2,
                videos: 5,
                world_items: 3,
                narrators: 1,
                products: 0,
                global_templates: vec![TemplateKind::Script, TemplateKind::Metadata],
                media_files: 40,
                media_bytes: 1 << 33,
            },
        };
        assert_eq!(PackageManifest::parse(&manifest.to_json()), Some(manifest));
        assert_eq!(PackageManifest::parse("{}"), None);
        assert_eq!(PackageManifest::parse("not json"), None);
    }

    #[test]
    fn ids_are_rewritten_wherever_they_appear_whole() {
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let other = Uuid::new_v4();
        let map = |id: Uuid| (id == old).then_some(new);
        assert_eq!(rewrite_ids(&old.to_string(), &map), Some(new.to_string()));
        let payload = format!(r#"{{"project":"{old}","channel":"{other}","again":"{old}"}}"#);
        assert_eq!(
            rewrite_ids(&payload, &map),
            Some(format!(
                r#"{{"project":"{new}","channel":"{other}","again":"{new}"}}"#
            ))
        );
        assert_eq!(
            rewrite_ids(&format!("peaks-capture-{old}.mp3.json"), &map),
            Some(format!("peaks-capture-{new}.mp3.json"))
        );
        // Not in the map, too short, or only part of an id: unchanged.
        assert_eq!(rewrite_ids(&other.to_string(), &map), None);
        assert_eq!(rewrite_ids("short", &map), None);
        assert_eq!(rewrite_ids(&old.simple().to_string(), &map), None);
    }

    #[test]
    fn a_taken_name_gets_the_first_free_number() {
        let taken = ["Space", "Space (2)"];
        assert_eq!(free_name("Sea", |name| taken.contains(&name)), "Sea");
        assert_eq!(
            free_name("Space", |name| taken.contains(&name)),
            "Space (3)"
        );
    }

    #[test]
    fn a_package_is_named_after_its_project() {
        assert_eq!(package_file_name("Space: Sea"), "Space_ Sea.bardo-project");
    }
}
