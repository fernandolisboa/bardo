//! A project's world (ADR-0047 item 6): the characters, places and objects
//! that may appear in its videos (#337, #338), each kept with a reference
//! picture per look so a look draws each one once. Owned by a project, or
//! by the global library. An object may belong to a character.
//!
//! Only what a video picks enters it ("Who appears"): the scene planner
//! gets the picked characters as fixed cast, the picked places as fixed
//! places and the picked objects as fixed objects, and never an unpicked
//! one.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{
    ChannelId, JobId, ProfileId, ProjectId, Provider, RepositoryError, StyleChoice, VideoProjectId,
    VisualStyle,
};

uuid_id!(
    /// Identifies a character, place or object of a world.
    WorldItemId
);

uuid_id!(
    /// Identifies a character's reference picture.
    ReferencePictureId
);

/// The most characters one video may pick: the planner keeps at most this
/// many recurring people (`MAX_CAST`).
pub const MAX_PICKED: usize = crate::MAX_CAST;

/// The most places one video may pick (#338): fewer than a plan's
/// `MAX_PLACES`, so the planner keeps room for places of its own.
pub const MAX_PICKED_PLACES: usize = 3;

/// The most objects one video may pick (a scene shows at most
/// `MAX_SCENE_OBJECTS` of them).
pub const MAX_OBJECTS: usize = 6;

/// What a world item is (#338).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WorldKind {
    /// Someone who appears in scenes, as a cast member.
    #[default]
    Character,
    /// Where scenes happen, as a plan's place.
    Place,
    /// A thing scenes show: a motorbike, a lantern. May belong to a
    /// character.
    Object,
}

impl WorldKind {
    pub const ALL: [WorldKind; 3] = [WorldKind::Character, WorldKind::Place, WorldKind::Object];

    pub fn code(self) -> &'static str {
        match self {
            WorldKind::Character => "character",
            WorldKind::Place => "place",
            WorldKind::Object => "object",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }

    /// How many of this kind one video may pick.
    pub fn max_picked(self) -> usize {
        match self {
            WorldKind::Character => MAX_PICKED,
            WorldKind::Place => MAX_PICKED_PLACES,
            WorldKind::Object => MAX_OBJECTS,
        }
    }

    /// The shape its reference picture is drawn in: a person or a thing
    /// standing, or a place as a wide view.
    pub fn picture_shape(self) -> crate::ImageShape {
        match self {
            WorldKind::Character | WorldKind::Object => crate::ImageShape::Portrait,
            WorldKind::Place => crate::ImageShape::Frame(crate::AspectRatio::Landscape),
        }
    }

    /// Whether the user may import a picture for it: a place or an object,
    /// never a person.
    pub fn imports(self) -> bool {
        self != WorldKind::Character
    }
}

/// A character's fields as the user typed them, before validation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorldItemDraft {
    pub name: String,
    /// How they look, always: face, age, hair, build, clothes.
    pub appearance: String,
    /// What they always do or have, in every video.
    pub always: String,
    /// What they never do, in any video.
    pub never: String,
}

/// Why a draft is not a valid character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldItemFieldError {
    NameRequired,
    NameTooLong,
    AppearanceRequired,
    AppearanceTooLong,
    NotesTooLong,
}

impl WorldItemFieldError {
    pub const ALL: [WorldItemFieldError; 5] = [
        WorldItemFieldError::NameRequired,
        WorldItemFieldError::NameTooLong,
        WorldItemFieldError::AppearanceRequired,
        WorldItemFieldError::AppearanceTooLong,
        WorldItemFieldError::NotesTooLong,
    ];
}

/// The user-editable part of a character, always valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldItemDetails {
    name: String,
    appearance: String,
    always: String,
    never: String,
}

impl WorldItemDetails {
    /// In characters, as a cast member's name.
    pub const MAX_NAME_CHARS: usize = crate::MAX_CAST_NAME_CHARS;
    /// In characters: a paragraph, as a cast member's description.
    pub const MAX_APPEARANCE_CHARS: usize = 1_500;
    /// In characters, each of the two notes.
    pub const MAX_NOTE_CHARS: usize = 1_000;

    /// Trims the fields and checks them.
    pub fn validate(draft: WorldItemDraft) -> Result<Self, WorldItemFieldError> {
        let name = draft.name.trim().to_owned();
        if name.is_empty() {
            return Err(WorldItemFieldError::NameRequired);
        }
        if name.chars().count() > Self::MAX_NAME_CHARS {
            return Err(WorldItemFieldError::NameTooLong);
        }
        let appearance = draft.appearance.trim().to_owned();
        if appearance.is_empty() {
            return Err(WorldItemFieldError::AppearanceRequired);
        }
        if appearance.chars().count() > Self::MAX_APPEARANCE_CHARS {
            return Err(WorldItemFieldError::AppearanceTooLong);
        }
        let always = draft.always.trim().to_owned();
        let never = draft.never.trim().to_owned();
        if always.chars().count() > Self::MAX_NOTE_CHARS
            || never.chars().count() > Self::MAX_NOTE_CHARS
        {
            return Err(WorldItemFieldError::NotesTooLong);
        }
        Ok(Self {
            name,
            appearance,
            always,
            never,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn appearance(&self) -> &str {
        &self.appearance
    }

    pub fn always(&self) -> &str {
        &self.always
    }

    pub fn never(&self) -> &str {
        &self.never
    }

    /// Whether `other` reads as the same name: names are unique per owner,
    /// ignoring case.
    pub fn same_name(&self, other: &str) -> bool {
        same_name(&self.name, other)
    }
}

/// Whether two names read as the same to the user, ignoring case and the
/// spaces around them.
pub fn same_name(one: &str, other: &str) -> bool {
    one.trim().to_lowercase() == other.trim().to_lowercase()
}

impl From<&WorldItemDetails> for WorldItemDraft {
    fn from(details: &WorldItemDetails) -> Self {
        Self {
            name: details.name.clone(),
            appearance: details.appearance.clone(),
            always: details.always.clone(),
            never: details.never.clone(),
        }
    }
}

/// Someone who can appear in a video's scenes (CONTEXT.md): a name, a
/// fixed appearance, what they always and never do. Always an original
/// person, never a real one. Not a voice: the narrator speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldItem {
    pub id: WorldItemId,
    pub owner: ProfileId,
    /// The project that owns it; `None` for the global library.
    pub owner_project: Option<ProjectId>,
    /// A character, a place or an object; fixed once made.
    pub kind: WorldKind,
    /// The character an object belongs to (its belongings, #338): of the
    /// same owner, or of the global library. `None` for anything else.
    pub belongs_to: Option<WorldItemId>,
    pub details: WorldItemDetails,
    /// When it was archived: hidden from the pickers, kept with its
    /// pictures. `None` while active.
    pub archived_at: Option<SystemTime>,
    pub created_at: SystemTime,
}

impl WorldItem {
    pub fn new(
        owner: ProfileId,
        owner_project: Option<ProjectId>,
        kind: WorldKind,
        details: WorldItemDetails,
        now: SystemTime,
    ) -> Self {
        Self {
            id: WorldItemId::new(),
            owner,
            owner_project,
            kind,
            belongs_to: None,
            details,
            archived_at: None,
            created_at: now,
        }
    }

    pub fn name(&self) -> &str {
        self.details.name()
    }

    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    /// The prompt a place's or an object's reference picture is drawn
    /// from in `style` (a character's is its cast portrait's): the thing
    /// alone, so every scene that shows it can be drawn with it.
    pub fn picture_prompt(&self, style: crate::StyleChoice) -> String {
        let (what, framing) = match self.kind {
            WorldKind::Place => (
                "A reference picture of one place of a video",
                "A wide, eye-level view of the whole place, evenly lit, with no people in it. \
                 No text, logos or watermarks.",
            ),
            WorldKind::Object | WorldKind::Character => (
                "A reference picture of one object of a video",
                "The object alone and whole, centered, against a plain, evenly lit background. \
                 No people, no text, logos or watermarks.",
            ),
        };
        style.applied(&format!(
            "{what}, {name}: {appearance}\n\n{framing}",
            name = self.name(),
            appearance = self.details.appearance(),
        ))
    }

    /// What the scene planner is told of it, with the video's own
    /// instruction when there is one.
    pub fn planner_lines(&self, instruction: &str) -> String {
        self.planner_lines_with(instruction, &[])
    }

    /// [`WorldItem::planner_lines`] with the names of a character's
    /// belongings the video picked too.
    pub fn planner_lines_with(&self, instruction: &str, belongings: &[&str]) -> String {
        let mut text = format!("{}: {}", self.name(), self.details.appearance());
        let has = belongings.join(", ");
        for (label, note) in [
            ("Always", self.details.always()),
            ("Never", self.details.never()),
            ("Has", has.as_str()),
            ("In this video", instruction.trim()),
        ] {
            if !note.is_empty() {
                text.push_str(&format!("\n   {label}: {note}"));
            }
        }
        text
    }
}

/// The look a reference picture was drawn in: a style of the catalog, or a
/// channel's aesthetic notes as they read when it was drawn (another text
/// is another look).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Look {
    Style(VisualStyle),
    Notes { channel: ChannelId, notes: String },
}

impl Look {
    /// The look of a video drawn in `style`, for its channel and the
    /// channel's notes as they are now.
    pub fn of(style: StyleChoice, channel: ChannelId, notes: &str) -> Self {
        match style {
            StyleChoice::Catalog(style) => Look::Style(style),
            StyleChoice::FromNotes => Look::Notes {
                channel,
                notes: notes.trim().to_owned(),
            },
        }
    }

    /// The style choice a picture of this look is drawn in.
    pub fn style(&self) -> StyleChoice {
        match self {
            Look::Style(style) => StyleChoice::Catalog(*style),
            Look::Notes { .. } => StyleChoice::FromNotes,
        }
    }

    /// The code its style is stored under (`notes` for the notes).
    pub fn code(&self) -> &'static str {
        self.style().code()
    }

    /// The look stored as `code`, with the channel and notes kept beside it
    /// for the notes. A style code Bardo does not know reads as nothing:
    /// the picture is no reference for any look of this version.
    pub fn from_parts(
        code: &str,
        channel: Option<ChannelId>,
        notes: Option<String>,
    ) -> Option<Self> {
        if code == StyleChoice::NOTES_CODE {
            return Some(Look::Notes {
                channel: channel?,
                notes: notes.unwrap_or_default(),
            });
        }
        VisualStyle::ALL
            .into_iter()
            .find(|style| style.code() == code)
            .map(Look::Style)
    }

    pub fn channel(&self) -> Option<ChannelId> {
        match self {
            Look::Style(_) => None,
            Look::Notes { channel, .. } => Some(*channel),
        }
    }

    pub fn notes(&self) -> Option<&str> {
        match self {
            Look::Style(_) => None,
            Look::Notes { notes, .. } => Some(notes),
        }
    }
}

/// Where a reference picture came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PictureOrigin {
    /// Drawn before the first scene of a video that showed the character
    /// in a look with none, paid under that video.
    Video,
    /// Drawn from the World screen, paid under the character's project.
    World,
    /// A video's cast portrait the user saved to the world with it.
    Saved,
    /// Copied with a duplicated item.
    Copied,
    /// A picture the user imported for a place or an object (#338), after
    /// confirming they may use it.
    Imported,
}

impl PictureOrigin {
    pub const ALL: [PictureOrigin; 5] = [
        PictureOrigin::Video,
        PictureOrigin::World,
        PictureOrigin::Saved,
        PictureOrigin::Copied,
        PictureOrigin::Imported,
    ];

    pub fn code(self) -> &'static str {
        match self {
            PictureOrigin::Video => "video",
            PictureOrigin::World => "world",
            PictureOrigin::Saved => "saved",
            PictureOrigin::Copied => "copied",
            PictureOrigin::Imported => "imported",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "world" => PictureOrigin::World,
            "saved" => PictureOrigin::Saved,
            "copied" => PictureOrigin::Copied,
            "imported" => PictureOrigin::Imported,
            _ => PictureOrigin::Video,
        }
    }
}

/// How a reference picture was made: its own record, owned by the
/// character and not by a video, so deleting the video that first drew it
/// leaves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureProvenance {
    /// Who drew it; `None` for an imported picture.
    pub provider: Option<Provider>,
    /// Empty for an imported picture.
    pub model: String,
    /// The whole prompt it was drawn from; empty for an imported picture.
    pub prompt: String,
    pub generated_at: SystemTime,
    /// The job that drew it; jobs may be cleaned up.
    pub job: Option<JobId>,
    pub origin: PictureOrigin,
    /// When the user confirmed they may use an imported picture; `None`
    /// for a drawn one.
    pub confirmed_at: Option<SystemTime>,
}

/// A world item's picture for one look, kept in the world folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferencePicture {
    pub id: ReferencePictureId,
    /// The item it shows.
    pub item: WorldItemId,
    pub look: Look,
    /// The file's name in the owner's world folder.
    pub file: String,
    pub provenance: PictureProvenance,
    /// Whether the user kept it. One drawn from the World screen waits for
    /// review; only a kept picture is used by videos.
    pub kept: bool,
}

/// A world item a video picked, with what it is told for that video only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldPick {
    pub item: WorldItemId,
    /// "Arrives on the motorbike in the first scene", "never speaks".
    pub instruction: String,
}

impl WorldPick {
    /// In characters.
    pub const MAX_INSTRUCTION_CHARS: usize = 500;
}

/// A video that uses a character: one that picked it, or whose plan has it
/// in its cast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldItemUse {
    pub video: VideoProjectId,
    pub title: String,
    pub archived: bool,
}

/// Characters, their reference pictures and what each video picked.
pub trait WorldRepository: Send + Sync {
    /// The characters `project` owns, or the global library's for `None`,
    /// archived ones too, by name.
    fn characters(
        &self,
        owner: ProfileId,
        project: Option<ProjectId>,
    ) -> Result<Vec<WorldItem>, RepositoryError>;

    fn character(&self, id: WorldItemId) -> Result<Option<WorldItem>, RepositoryError>;

    /// Adds or replaces the character.
    fn save_character(&self, character: &WorldItem) -> Result<(), RepositoryError>;

    /// Removes the character with its pictures and picks; plans that had
    /// it keep their cast member, no longer linked to it.
    fn delete_character(&self, id: WorldItemId) -> Result<(), RepositoryError>;

    /// The character's reference pictures, kept and waiting.
    fn pictures(&self, character: WorldItemId) -> Result<Vec<ReferencePicture>, RepositoryError>;

    /// Adds the picture. A kept picture replaces the kept one of its look;
    /// a waiting one, the waiting one of its look.
    fn save_picture(&self, picture: &ReferencePicture) -> Result<(), RepositoryError>;

    fn remove_picture(&self, id: ReferencePictureId) -> Result<(), RepositoryError>;

    /// The characters the video picked, in order.
    fn picks(&self, video: VideoProjectId) -> Result<Vec<WorldPick>, RepositoryError>;

    /// Replaces what the video picked; with `review`, its saved plan is
    /// marked for review.
    fn set_picks(
        &self,
        video: VideoProjectId,
        picks: &[WorldPick],
        review: bool,
    ) -> Result<(), RepositoryError>;

    /// Whether who appears changed since the video's plan was made.
    fn picks_changed(&self, video: VideoProjectId) -> Result<bool, RepositoryError>;

    /// The videos that picked the character or have it in their plan's
    /// cast.
    fn character_uses(&self, character: WorldItemId) -> Result<Vec<WorldItemUse>, RepositoryError>;
}

/// The world's picture files: in a project's `World` folder, or the global
/// library's under the base folder. Names are plain file names.
pub trait WorldFiles: Send + Sync {
    /// Writes the whole file; a reader never sees it half written.
    fn write(&self, folder: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()>;

    fn read(&self, folder: &Path, name: &str) -> std::io::Result<Vec<u8>>;

    /// Removes the file; one already gone is not an error.
    fn remove(&self, folder: &Path, name: &str) -> std::io::Result<()>;

    /// Whether the folder is there to read from.
    fn reachable(&self, folder: &Path) -> bool;

    /// Where the file is, for the screens that show it.
    fn path(&self, folder: &Path, name: &str) -> PathBuf {
        folder.join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(name: &str, appearance: &str) -> WorldItemDraft {
        WorldItemDraft {
            name: name.into(),
            appearance: appearance.into(),
            ..WorldItemDraft::default()
        }
    }

    #[test]
    fn a_character_needs_a_name_and_an_appearance_within_limits() {
        assert_eq!(
            WorldItemDetails::validate(draft("  ", "tall")),
            Err(WorldItemFieldError::NameRequired)
        );
        assert_eq!(
            WorldItemDetails::validate(draft("Ana", " ")),
            Err(WorldItemFieldError::AppearanceRequired)
        );
        assert_eq!(
            WorldItemDetails::validate(draft(&"a".repeat(61), "tall")),
            Err(WorldItemFieldError::NameTooLong)
        );
        assert_eq!(
            WorldItemDetails::validate(draft("Ana", &"a".repeat(1_501))),
            Err(WorldItemFieldError::AppearanceTooLong)
        );
        let mut long = draft("Ana", "tall");
        long.never = "n".repeat(1_001);
        assert_eq!(
            WorldItemDetails::validate(long),
            Err(WorldItemFieldError::NotesTooLong)
        );
        let details = WorldItemDetails::validate(draft(" Ana ", " tall ")).unwrap();
        assert_eq!((details.name(), details.appearance()), ("Ana", "tall"));
        assert!(details.same_name("ANA "));
    }

    #[test]
    fn the_planner_reads_the_notes_and_the_videos_instruction_only_when_set() {
        let mut typed = draft("Ana", "a baker in her sixties");
        typed.never = "smiles".into();
        let ana = WorldItem::new(
            ProfileId::new(),
            None,
            WorldKind::Character,
            WorldItemDetails::validate(typed).unwrap(),
            SystemTime::UNIX_EPOCH,
        );
        assert_eq!(
            ana.planner_lines(""),
            "Ana: a baker in her sixties\n   Never: smiles"
        );
        assert_eq!(
            ana.planner_lines(" opens the bakery "),
            "Ana: a baker in her sixties\n   Never: smiles\n   In this video: opens the bakery"
        );
        assert_eq!(
            ana.planner_lines_with("", &["the red van", "a whisk"]),
            "Ana: a baker in her sixties\n   Never: smiles\n   Has: the red van, a whisk"
        );
    }

    #[test]
    fn a_kind_reads_back_from_its_code_and_only_places_and_objects_import() {
        for kind in WorldKind::ALL {
            assert_eq!(WorldKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(WorldKind::from_code("vehicle"), None);
        assert!(!WorldKind::Character.imports());
        assert!(WorldKind::Place.imports() && WorldKind::Object.imports());
        assert_eq!(WorldKind::Character.max_picked(), MAX_PICKED);
        assert_eq!(WorldKind::Place.max_picked(), MAX_PICKED_PLACES);
    }

    #[test]
    fn a_look_is_a_style_or_a_channels_notes_as_they_read() {
        let channel = ChannelId::new();
        let clay = StyleChoice::Catalog(VisualStyle::ClayStopMotion);
        assert_eq!(
            Look::of(clay, channel, "warm"),
            Look::Style(VisualStyle::ClayStopMotion)
        );
        let notes = Look::of(StyleChoice::FromNotes, channel, " warm light ");
        assert_eq!(notes.notes(), Some("warm light"));
        assert_ne!(
            notes,
            Look::of(StyleChoice::FromNotes, channel, "cold light")
        );
        assert_ne!(
            notes,
            Look::of(StyleChoice::FromNotes, ChannelId::new(), "warm light")
        );
        for look in [notes.clone(), Look::Style(VisualStyle::ClayStopMotion)] {
            assert_eq!(
                Look::from_parts(look.code(), look.channel(), look.notes().map(str::to_owned)),
                Some(look)
            );
        }
        assert_eq!(Look::from_parts("holo-9000", None, None), None);
    }
}
