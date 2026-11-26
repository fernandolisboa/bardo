//! Where a project's media lives on disk (ADR-0047 item 3): a folder per
//! project under the base folder, `Videos\<slug>-<short id>` per video,
//! `World\` and `bardo-project.json`, which names the project so Bardo can
//! tell its folder from any other.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::{ProfileId, ProjectId, RepositoryError, VideoProjectId};

/// The videos' folders, inside a project's folder.
pub const VIDEOS_DIR: &str = "Videos";

/// The world's pictures, inside a project's folder.
pub const WORLD_DIR: &str = "World";

/// The global library's folder, under the base folder.
pub const GLOBAL_DIR: &str = "Global";

/// The file in a project's folder that names the project.
pub const PROJECT_MANIFEST: &str = "bardo-project.json";

/// A project folder's name, at most.
pub const MAX_PROJECT_FOLDER_CHARS: usize = 40;

/// A video folder's slug, at most, before its short id.
pub const MAX_VIDEO_SLUG_CHARS: usize = 30;

/// The longest file name Bardo writes in a video's folder, with room to
/// spare (`peaks-capture-<id>.part001.mp3.json.partial` is 75).
const MAX_FILE_NAME_CHARS: usize = 100;

/// The longest path Windows opens without the long-path prefix
/// (`MAX_PATH`, 260 with its terminating NUL).
const MAX_PATH_CHARS: usize = 259;

/// Names Windows keeps for devices, whatever their case or extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `name` made safe as a Windows folder name, cut to `max` characters:
/// `<>:"/\|?*` and control characters become `_`, as do trailing dots and
/// spaces, and a device name (`CON`, `NUL`, `COM1`…) gets a `_` after it.
/// `fallback` stands in for a name with nothing left.
pub fn safe_folder_name(name: &str, max: usize, fallback: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .take(max)
        .collect();
    let cleaned = if cleaned.is_empty() {
        fallback.chars().take(max).collect()
    } else {
        cleaned
    };
    // Windows drops trailing dots and spaces from a name, so two names
    // would clash; they are kept as `_`.
    let kept = cleaned.trim_end_matches(['.', ' ']);
    let dropped = cleaned.chars().count() - kept.chars().count();
    let mut safe = format!("{kept}{}", "_".repeat(dropped));
    let stem = safe.split('.').next().unwrap_or_default();
    if RESERVED
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(stem))
    {
        if safe.chars().count() < max {
            safe.insert(stem.len(), '_');
        } else {
            // No room for one more: the device name loses its last letter.
            safe.pop();
            safe.push('_');
        }
    }
    safe
}

/// The first eight hex digits of an id: enough to tell folders apart.
pub fn short_id(id: Uuid) -> String {
    id.simple().to_string()[..8].to_owned()
}

/// A video's folder name, `<slug>-<short id>`: the slug from its title when
/// it is created, never following a later rename.
pub fn video_folder_name(title: &str, video: VideoProjectId) -> String {
    let slug = safe_folder_name(title, MAX_VIDEO_SLUG_CHARS, "Video");
    format!("{slug}-{}", short_id(video.as_uuid()))
}

/// A new project's folder name: its name made safe, with its short id
/// added when a folder of that name is `taken`.
pub fn project_folder_name(name: &str, project: ProjectId, taken: impl Fn(&str) -> bool) -> String {
    let safe = safe_folder_name(name, MAX_PROJECT_FOLDER_CHARS, "Project");
    // `Global` is the global folder's, beside the projects' folders.
    if safe.eq_ignore_ascii_case(GLOBAL_DIR) || taken(&safe) {
        format!("{safe}-{}", short_id(project.as_uuid()))
    } else {
        safe
    }
}

/// Whether a project's folder at `folder` leaves room for its deepest
/// file (`Videos\<slug>-<short id>\<file>`) under the Windows path limit.
pub fn leaves_room(folder: &Path) -> bool {
    let deepest = VIDEOS_DIR.len() + 1 + MAX_VIDEO_SLUG_CHARS + 1 + 8 + 1 + MAX_FILE_NAME_CHARS;
    folder.as_os_str().to_string_lossy().chars().count() + 1 + deepest <= MAX_PATH_CHARS
}

/// Whether `path` lies under one of the folders a sync client keeps in the
/// cloud (OneDrive's `OneDrive`, `OneDriveConsumer`, `OneDriveCommercial`).
pub fn is_synced(path: &Path, synced: &[PathBuf]) -> bool {
    synced
        .iter()
        .any(|root| !root.as_os_str().is_empty() && path.starts_with(root))
}

/// The base folder Bardo suggests: `<Documents>\<data home>`, unless
/// Documents is synced to the cloud (or unknown), then
/// `<user profile>\<data home>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultBase {
    pub path: PathBuf,
    /// Documents is synced, so the base left it: Settings says why.
    pub avoided_sync: bool,
}

pub fn default_base_folder(
    documents: Option<&Path>,
    home: &Path,
    synced: &[PathBuf],
    data_home: &str,
) -> DefaultBase {
    match documents {
        Some(documents) if !is_synced(documents, synced) => DefaultBase {
            path: documents.join(data_home),
            avoided_sync: false,
        },
        documents => DefaultBase {
            path: home.join(data_home),
            avoided_sync: documents.is_some(),
        },
    }
}

/// The folders of this computer that decide where media goes: the user's
/// Documents and profile folders, the folders a sync client keeps in the
/// cloud, the data home's name, and the old projects folder media is
/// moved out of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderPlaces {
    pub documents: Option<PathBuf>,
    pub home: PathBuf,
    /// OneDrive's folders (`OneDrive`, `OneDriveConsumer`,
    /// `OneDriveCommercial`), as the environment names them.
    pub synced: Vec<PathBuf>,
    /// The data home's folder name (`Bardo`, `Bardo Dev`).
    pub data_home: String,
    /// `%LOCALAPPDATA%\<data home>\projects`: each video's media before
    /// it had a project folder, as `<video id>`.
    pub old_media: PathBuf,
}

impl FolderPlaces {
    /// The base folder Bardo suggests on this computer.
    pub fn default_base(&self) -> DefaultBase {
        default_base_folder(
            self.documents.as_deref(),
            &self.home,
            &self.synced,
            &self.data_home,
        )
    }

    /// The base folder: the one chosen, else the default.
    pub fn base(&self, chosen: Option<&Path>) -> PathBuf {
        chosen.map_or_else(|| self.default_base().path, Path::to_path_buf)
    }

    /// Whether `path` lies in a folder synced to the cloud.
    pub fn is_synced(&self, path: &Path) -> bool {
        is_synced(path, &self.synced)
    }
}

/// What `bardo-project.json` says: whose project the folder is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectManifest {
    pub project: ProjectId,
    pub name: String,
    pub profile: ProfileId,
    /// The data home's folder name (`Bardo`, `Bardo Dev`).
    pub data_home: String,
}

impl ProjectManifest {
    pub fn to_json(&self) -> String {
        let value = json!({
            "format": 1,
            "project": self.project.to_string(),
            "name": self.name,
            "profile": self.profile.to_string(),
            "data_home": self.data_home,
        });
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }

    /// `None` when the text is no project manifest.
    pub fn parse(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        let id = |key: &str| Uuid::parse_str(value.get(key)?.as_str()?).ok();
        Some(Self {
            project: ProjectId::from(id("project")?),
            name: value.get("name")?.as_str()?.to_owned(),
            profile: ProfileId::from(id("profile")?),
            data_home: value.get("data_home")?.as_str()?.to_owned(),
        })
    }

    /// Whether the folder is this project's, of this profile and data home.
    pub fn names(&self, project: ProjectId, profile: ProfileId, data_home: &str) -> bool {
        self.project == project && self.profile == profile && self.data_home == data_home
    }
}

/// Where a video's media is: in a project's folder, under `Videos\`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFolder {
    /// The project whose folder holds it: its channel's, unless the
    /// channel moved since and the media has not followed yet.
    pub project: ProjectId,
    /// That project's folder; `None` while the project has none yet.
    pub project_folder: Option<PathBuf>,
    /// `<slug>-<short id>`, under `Videos\`.
    pub name: String,
}

impl VideoFolder {
    /// The folder's full path, once its project has a folder.
    pub fn path(&self) -> Option<PathBuf> {
        self.project_folder
            .as_ref()
            .map(|folder| folder.join(VIDEOS_DIR).join(&self.name))
    }
}

/// A video whose media is not where its project keeps it: still in the
/// old projects folder, or in the folder of a project its channel left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoToMove {
    pub video: VideoProjectId,
    pub title: String,
    /// Its channel's project, where the media goes.
    pub to: ProjectId,
    /// Where it is now; `None` in the old projects folder.
    pub from: Option<VideoFolder>,
}

impl VideoToMove {
    /// The folder name it keeps, or the one it gets from its title.
    pub fn folder_name(&self) -> String {
        self.from.as_ref().map_or_else(
            || video_folder_name(&self.title, self.video),
            |from| from.name.clone(),
        )
    }
}

/// Where each video's media lives, for the files store: `None` while it is
/// still in the old projects folder (`<root>\<video id>`).
pub trait VideoFolders: Send + Sync {
    fn video_folder_path(&self, video: VideoProjectId) -> Option<PathBuf>;
}

/// The base folder, project folders and where each video's media is.
pub trait FolderRepository: Send + Sync {
    /// The base folder the user chose; `None` for the default.
    fn base_folder(&self, owner: ProfileId) -> Result<Option<PathBuf>, RepositoryError>;

    fn set_base_folder(
        &self,
        owner: ProfileId,
        folder: Option<&Path>,
    ) -> Result<(), RepositoryError>;

    /// Points the project at its folder (a new one, or one located).
    fn set_project_folder(&self, project: ProjectId, folder: &Path) -> Result<(), RepositoryError>;

    fn video_folder(&self, video: VideoProjectId) -> Result<Option<VideoFolder>, RepositoryError>;

    /// The owner's videos whose media is not in their project's folder.
    fn videos_to_move(&self, owner: ProfileId) -> Result<Vec<VideoToMove>, RepositoryError>;

    /// The videos whose media is in one of `projects`' folders, or whose
    /// channel belongs to one of them.
    fn videos_in(&self, projects: &[ProjectId]) -> Result<Vec<VideoProjectId>, RepositoryError>;

    /// Records that the video's media is now in `project`'s folder, as
    /// `name`.
    fn set_video_folder(
        &self,
        video: VideoProjectId,
        project: ProjectId,
        name: &str,
    ) -> Result<(), RepositoryError>;
}

/// How a video's folder reached its new place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderMove {
    /// There was nothing to move: the video had no media yet.
    Nothing,
    /// Renamed, on the same volume.
    Renamed,
    /// Copied and checked, and the original removed (what could go of it):
    /// another volume.
    Copied,
}

/// Why a folder did not move.
#[derive(Debug, thiserror::Error)]
pub enum FolderMoveError {
    /// Told to stop; the original is whole, the copy may be partial.
    #[error("stopped")]
    Stopped,
    /// A copied file does not match its original; the original stays.
    #[error("the copy of {0} does not match its original")]
    Mismatch(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Project folders on disk: made, recognized and moved.
pub trait ProjectFolders: Send + Sync {
    /// Makes the project's folder, its `Videos` and `World` folders and its
    /// `bardo-project.json`; an existing folder keeps what it holds.
    fn create(&self, folder: &Path, manifest: &ProjectManifest) -> std::io::Result<()>;

    /// What the folder's `bardo-project.json` says; `None` when the folder
    /// cannot be reached or holds none that reads.
    fn manifest(&self, folder: &Path) -> Option<ProjectManifest>;

    /// Whether a folder (or file) of that path exists.
    fn exists(&self, path: &Path) -> bool;

    /// Moves a video's folder from `from` to `to`: renamed when it can be,
    /// else copied file by file, each checked, and the original removed
    /// only once all are. `progress` gets the bytes copied and the total,
    /// and stops the copy when it returns false. Files already copied
    /// whole by a run cut short are not copied again.
    fn move_folder(
        &self,
        from: &Path,
        to: &Path,
        progress: &mut dyn FnMut(u64, u64) -> bool,
    ) -> Result<FolderMove, FolderMoveError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe_for_windows() {
        let safe = |name| safe_folder_name(name, 40, "Project");
        assert_eq!(safe("Space & Sea"), "Space & Sea");
        assert_eq!(safe("a<b>c:d\"e/f\\g|h?i*j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(safe("  Ends with dots... "), "Ends with dots___");
        assert_eq!(safe("tab\there"), "tab_here");
        assert_eq!(safe("con"), "con_");
        assert_eq!(safe("NUL.txt"), "NUL_.txt");
        assert_eq!(safe("COM1"), "COM1_");
        assert_eq!(safe("Console"), "Console");
        assert_eq!(safe("   "), "Project");
        assert_eq!(safe("Histórias do mar"), "Histórias do mar");
    }

    #[test]
    fn names_are_cut_to_their_length_in_characters() {
        let long = "á".repeat(50);
        assert_eq!(safe_folder_name(&long, 40, "x").chars().count(), 40);
        // Cut first, so a cut that ends in a dot or space is caught too.
        assert_eq!(safe_folder_name("abc. def", 4, "x"), "abc_");
        assert_eq!(safe_folder_name("LPT1", 4, "x"), "LPT_");
    }

    #[test]
    fn a_video_folder_is_its_slug_and_short_id() {
        let id =
            VideoProjectId::from(Uuid::parse_str("0123abcd-0000-4000-8000-000000000000").unwrap());
        assert_eq!(
            video_folder_name("The probe: never back?", id),
            "The probe_ never back_-0123abcd"
        );
        let long = "x".repeat(80);
        assert_eq!(video_folder_name(&long, id).chars().count(), 30 + 1 + 8);
        assert_eq!(video_folder_name("", id), "Video-0123abcd");
    }

    #[test]
    fn a_project_folder_that_clashes_gets_its_short_id() {
        let id = ProjectId::from(Uuid::parse_str("feedbeef-0000-4000-8000-000000000000").unwrap());
        assert_eq!(
            project_folder_name("Space & Sea", id, |_| false),
            "Space & Sea"
        );
        assert_eq!(
            project_folder_name("Space & Sea", id, |name| name == "Space & Sea"),
            "Space & Sea-feedbeef"
        );
        assert_eq!(project_folder_name("a?", id, |_| false), "a_");
        // The global folder's name is never a project's.
        assert_eq!(
            project_folder_name("global", id, |_| false),
            "global-feedbeef"
        );
    }

    #[test]
    fn a_deep_folder_leaves_no_room() {
        assert!(leaves_room(Path::new(
            r"C:\Users\ana\Documents\Bardo\Space & Sea"
        )));
        let deep = format!(r"C:\{}", "d".repeat(120));
        assert!(!leaves_room(Path::new(&deep)));
    }

    #[test]
    fn documents_synced_by_onedrive_moves_the_default_to_the_user_folder() {
        let home = Path::new("/home/ana");
        let documents = Path::new("/home/ana/OneDrive/Documents");
        let synced = vec![PathBuf::from("/home/ana/OneDrive")];
        assert_eq!(
            default_base_folder(Some(documents), home, &synced, "Bardo"),
            DefaultBase {
                path: PathBuf::from("/home/ana/Bardo"),
                avoided_sync: true
            }
        );
        let local = Path::new("/home/ana/Documents");
        assert_eq!(
            default_base_folder(Some(local), home, &synced, "Bardo Dev"),
            DefaultBase {
                path: PathBuf::from("/home/ana/Documents/Bardo Dev"),
                avoided_sync: false
            }
        );
        assert_eq!(
            default_base_folder(None, home, &[], "Bardo").path,
            PathBuf::from("/home/ana/Bardo")
        );
        assert!(
            !is_synced(local, &[PathBuf::new()]),
            "an empty variable syncs nothing"
        );
    }

    #[test]
    fn a_manifest_reads_back_and_names_only_its_project() {
        let manifest = ProjectManifest {
            project: ProjectId::new(),
            name: "Space & Sea".into(),
            profile: ProfileId::new(),
            data_home: "Bardo".into(),
        };
        let read = ProjectManifest::parse(&manifest.to_json()).unwrap();
        assert_eq!(read, manifest);
        assert!(read.names(manifest.project, manifest.profile, "Bardo"));
        assert!(!read.names(manifest.project, manifest.profile, "Bardo Dev"));
        assert!(!read.names(ProjectId::new(), manifest.profile, "Bardo"));
        assert!(!read.names(manifest.project, ProfileId::new(), "Bardo"));
        assert_eq!(ProjectManifest::parse("{}"), None);
        assert_eq!(ProjectManifest::parse("not json"), None);
    }
}
