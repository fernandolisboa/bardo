//! A project's world (ADR-0047 item 6): the characters that may appear in
//! its videos, each kept with a reference picture per look so a look draws
//! each one once. Owned by a project, or by the global library.
//!
//! Only what a video picks enters it ("Who appears"): the scene planner
//! gets the picked characters as fixed cast and never an unpicked one.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{
    ChannelId, JobId, ProfileId, ProjectId, Provider, RepositoryError, StyleChoice, VideoProjectId,
    VisualStyle,
};

uuid_id!(
    /// Identifies a character of a world.
    CharacterId
);

uuid_id!(
    /// Identifies a character's reference picture.
    ReferencePictureId
);

/// The most characters one video may pick: the planner keeps at most this
/// many recurring people (`MAX_CAST`).
pub const MAX_PICKED: usize = crate::MAX_CAST;

/// A character's fields as the user typed them, before validation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CharacterDraft {
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
pub enum CharacterFieldError {
    NameRequired,
    NameTooLong,
    AppearanceRequired,
    AppearanceTooLong,
    NotesTooLong,
}

impl CharacterFieldError {
    pub const ALL: [CharacterFieldError; 5] = [
        CharacterFieldError::NameRequired,
        CharacterFieldError::NameTooLong,
        CharacterFieldError::AppearanceRequired,
        CharacterFieldError::AppearanceTooLong,
        CharacterFieldError::NotesTooLong,
    ];
}

/// The user-editable part of a character, always valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterDetails {
    name: String,
    appearance: String,
    always: String,
    never: String,
}

impl CharacterDetails {
    /// In characters, as a cast member's name.
    pub const MAX_NAME_CHARS: usize = crate::MAX_CAST_NAME_CHARS;
    /// In characters: a paragraph, as a cast member's description.
    pub const MAX_APPEARANCE_CHARS: usize = 1_500;
    /// In characters, each of the two notes.
    pub const MAX_NOTE_CHARS: usize = 1_000;

    /// Trims the fields and checks them.
    pub fn validate(draft: CharacterDraft) -> Result<Self, CharacterFieldError> {
        let name = draft.name.trim().to_owned();
        if name.is_empty() {
            return Err(CharacterFieldError::NameRequired);
        }
        if name.chars().count() > Self::MAX_NAME_CHARS {
            return Err(CharacterFieldError::NameTooLong);
        }
        let appearance = draft.appearance.trim().to_owned();
        if appearance.is_empty() {
            return Err(CharacterFieldError::AppearanceRequired);
        }
        if appearance.chars().count() > Self::MAX_APPEARANCE_CHARS {
            return Err(CharacterFieldError::AppearanceTooLong);
        }
        let always = draft.always.trim().to_owned();
        let never = draft.never.trim().to_owned();
        if always.chars().count() > Self::MAX_NOTE_CHARS
            || never.chars().count() > Self::MAX_NOTE_CHARS
        {
            return Err(CharacterFieldError::NotesTooLong);
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

impl From<&CharacterDetails> for CharacterDraft {
    fn from(details: &CharacterDetails) -> Self {
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
pub struct Character {
    pub id: CharacterId,
    pub owner: ProfileId,
    /// The project that owns it; `None` for the global library.
    pub owner_project: Option<ProjectId>,
    pub details: CharacterDetails,
    /// When it was archived: hidden from the pickers, kept with its
    /// pictures. `None` while active.
    pub archived_at: Option<SystemTime>,
    pub created_at: SystemTime,
}

impl Character {
    pub fn new(
        owner: ProfileId,
        owner_project: Option<ProjectId>,
        details: CharacterDetails,
        now: SystemTime,
    ) -> Self {
        Self {
            id: CharacterId::new(),
            owner,
            owner_project,
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

    /// What the scene planner is told of them, with the video's own
    /// instruction when there is one.
    pub fn planner_lines(&self, instruction: &str) -> String {
        let mut text = format!("{}: {}", self.name(), self.details.appearance());
        for (label, note) in [
            ("Always", self.details.always()),
            ("Never", self.details.never()),
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
    /// Copied with a duplicated character.
    Copied,
}

impl PictureOrigin {
    pub const ALL: [PictureOrigin; 4] = [
        PictureOrigin::Video,
        PictureOrigin::World,
        PictureOrigin::Saved,
        PictureOrigin::Copied,
    ];

    pub fn code(self) -> &'static str {
        match self {
            PictureOrigin::Video => "video",
            PictureOrigin::World => "world",
            PictureOrigin::Saved => "saved",
            PictureOrigin::Copied => "copied",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "world" => PictureOrigin::World,
            "saved" => PictureOrigin::Saved,
            "copied" => PictureOrigin::Copied,
            _ => PictureOrigin::Video,
        }
    }
}

/// How a reference picture was made: its own record, owned by the
/// character and not by a video, so deleting the video that first drew it
/// leaves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureProvenance {
    pub provider: Provider,
    pub model: String,
    /// The whole prompt it was drawn from.
    pub prompt: String,
    pub generated_at: SystemTime,
    /// The job that drew it; jobs may be cleaned up.
    pub job: Option<JobId>,
    pub origin: PictureOrigin,
}

/// A character's portrait for one look, kept in the world folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferencePicture {
    pub id: ReferencePictureId,
    pub character: CharacterId,
    pub look: Look,
    /// The file's name in the owner's world folder.
    pub file: String,
    pub provenance: PictureProvenance,
    /// Whether the user kept it. One drawn from the World screen waits for
    /// review; only a kept picture is used by videos.
    pub kept: bool,
}

/// A character a video picked, with what it is told for that video only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastPick {
    pub character: CharacterId,
    /// "Arrives on the motorbike in the first scene", "never speaks".
    pub instruction: String,
}

impl CastPick {
    /// In characters.
    pub const MAX_INSTRUCTION_CHARS: usize = 500;
}

/// A video that uses a character: one that picked it, or whose plan has it
/// in its cast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterUse {
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
    ) -> Result<Vec<Character>, RepositoryError>;

    fn character(&self, id: CharacterId) -> Result<Option<Character>, RepositoryError>;

    /// Adds or replaces the character.
    fn save_character(&self, character: &Character) -> Result<(), RepositoryError>;

    /// Removes the character with its pictures and picks; plans that had
    /// it keep their cast member, no longer linked to it.
    fn delete_character(&self, id: CharacterId) -> Result<(), RepositoryError>;

    /// The character's reference pictures, kept and waiting.
    fn pictures(&self, character: CharacterId) -> Result<Vec<ReferencePicture>, RepositoryError>;

    /// Adds the picture. A kept picture replaces the kept one of its look;
    /// a waiting one, the waiting one of its look.
    fn save_picture(&self, picture: &ReferencePicture) -> Result<(), RepositoryError>;

    fn remove_picture(&self, id: ReferencePictureId) -> Result<(), RepositoryError>;

    /// The characters the video picked, in order.
    fn picks(&self, video: VideoProjectId) -> Result<Vec<CastPick>, RepositoryError>;

    /// Replaces what the video picked; with `review`, its saved plan is
    /// marked for review.
    fn set_picks(
        &self,
        video: VideoProjectId,
        picks: &[CastPick],
        review: bool,
    ) -> Result<(), RepositoryError>;

    /// Whether who appears changed since the video's plan was made.
    fn picks_changed(&self, video: VideoProjectId) -> Result<bool, RepositoryError>;

    /// The videos that picked the character or have it in their plan's
    /// cast.
    fn character_uses(&self, character: CharacterId) -> Result<Vec<CharacterUse>, RepositoryError>;
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

    fn draft(name: &str, appearance: &str) -> CharacterDraft {
        CharacterDraft {
            name: name.into(),
            appearance: appearance.into(),
            ..CharacterDraft::default()
        }
    }

    #[test]
    fn a_character_needs_a_name_and_an_appearance_within_limits() {
        assert_eq!(
            CharacterDetails::validate(draft("  ", "tall")),
            Err(CharacterFieldError::NameRequired)
        );
        assert_eq!(
            CharacterDetails::validate(draft("Ana", " ")),
            Err(CharacterFieldError::AppearanceRequired)
        );
        assert_eq!(
            CharacterDetails::validate(draft(&"a".repeat(61), "tall")),
            Err(CharacterFieldError::NameTooLong)
        );
        assert_eq!(
            CharacterDetails::validate(draft("Ana", &"a".repeat(1_501))),
            Err(CharacterFieldError::AppearanceTooLong)
        );
        let mut long = draft("Ana", "tall");
        long.never = "n".repeat(1_001);
        assert_eq!(
            CharacterDetails::validate(long),
            Err(CharacterFieldError::NotesTooLong)
        );
        let details = CharacterDetails::validate(draft(" Ana ", " tall ")).unwrap();
        assert_eq!((details.name(), details.appearance()), ("Ana", "tall"));
        assert!(details.same_name("ANA "));
    }

    #[test]
    fn the_planner_reads_the_notes_and_the_videos_instruction_only_when_set() {
        let mut typed = draft("Ana", "a baker in her sixties");
        typed.never = "smiles".into();
        let ana = Character::new(
            ProfileId::new(),
            None,
            CharacterDetails::validate(typed).unwrap(),
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
