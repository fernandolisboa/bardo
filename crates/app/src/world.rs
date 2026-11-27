//! The world (issue #337, ADR-0047 item 6): the characters a project, or
//! the global library, keeps for its videos.
//!
//! **Only what a video picks enters it.** *Who appears* lists the
//! characters of the video's project and of the global library; the scene
//! planner gets the picked ones as its fixed cast and never an unpicked one.
//! Changing who appears after planning marks the plan for review.
//!
//! **A look draws each portrait once.** A character's reference picture is
//! kept per look (a catalog style, or a channel's notes as they read), in
//! the owner's `World` folder, with a provenance of its own. A video that
//! shows a picked character in a look that has a picture copies it into its
//! folder for free; one that has none draws it as any cast portrait
//! (ADR-0035) and keeps it in the world. Deleting that video leaves it.
//! Two videos drawn at once never both pay for it: the second waits, then
//! takes the picture the first kept.
//!
//! **Editing a character's appearance** drops its pictures in every look and
//! the portraits of the videos that show it, whose drawn scenes then read
//! *old portrait*; its name and notes keep them.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::SystemTime;

use bardo_domain::{
    ApiKey, CastMember, CastPick, CastRecord, ChannelRepository, Character, CharacterDetails,
    CharacterDraft, CharacterFieldError, CharacterId, CharacterUse, CostPurpose, FolderPlaces,
    FolderRepository, GLOBAL_DIR, Generation, GenerationId, ImageGenerator, ImageRequest,
    ImageShape, Job, JobFailure, JobFailureKind, JobId, JobKind, Look, MAX_PICKED, PictureOrigin,
    PictureProvenance, ProfileId, ProjectFiles, ProjectId, ProjectRepository, Provider,
    ReferencePicture, ReferencePictureId, RepositoryError, SceneImage, ScenePlan, ScenePrompt,
    SecretStore, StyleChoice, TokenUsage, VideoProject, VideoProjectId, WORLD_DIR, WorldFiles,
    WorldRepository, same_name,
};
use serde::{Deserialize, Serialize};

use crate::costs::{CostBook, PaidCall, PlannedCall};
use crate::jobs::{JobContext, JobHandler};
use crate::scenes::{id, parse, to_json, unexpected};
use crate::{AppError, Bardo, BudgetConsent, SpendEstimate, Text};

/// Where the world's pictures live and how a video takes one: shared with
/// the scene jobs, which draw and reuse portraits.
#[derive(Clone)]
pub(crate) struct WorldBook {
    pub(crate) world: Arc<dyn WorldRepository>,
    pub(crate) files: Arc<dyn WorldFiles>,
    pub(crate) projects: Arc<dyn ProjectRepository>,
    pub(crate) channels: Arc<dyn ChannelRepository>,
    /// The videos, for the look a job draws in.
    pub(crate) themes: Arc<dyn bardo_domain::ThemeRepository>,
    pub(crate) folders: Arc<dyn FolderRepository>,
    pub(crate) places: FolderPlaces,
    pub(crate) owner: ProfileId,
    /// The portraits being drawn now, so two jobs never pay for the same.
    pub(crate) desk: Arc<PortraitDesk>,
}

/// The character portraits being drawn now, by character and look. Jobs
/// run side by side (two videos showing the same character in the same
/// look): the second waits for the first, then takes the picture it kept
/// instead of paying for its own. Within one process: the background agent
/// draws nothing.
#[derive(Default)]
pub(crate) struct PortraitDesk {
    drawing: Mutex<HashSet<(CharacterId, Look)>>,
    free: Condvar,
}

impl PortraitDesk {
    /// Waits until nobody draws `character` in `look`, then holds it until
    /// the hold is dropped.
    pub(crate) fn hold(&self, character: CharacterId, look: &Look) -> PortraitHold<'_> {
        let key = (character, look.clone());
        let mut drawing = self.drawing.lock().unwrap_or_else(PoisonError::into_inner);
        while drawing.contains(&key) {
            drawing = self
                .free
                .wait(drawing)
                .unwrap_or_else(PoisonError::into_inner);
        }
        drawing.insert(key.clone());
        PortraitHold { desk: self, key }
    }
}

/// A portrait one job draws; dropping it lets the next one look.
pub(crate) struct PortraitHold<'a> {
    desk: &'a PortraitDesk,
    key: (CharacterId, Look),
}

impl Drop for PortraitHold<'_> {
    fn drop(&mut self) {
        self.desk
            .drawing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
        self.desk.free.notify_all();
    }
}

impl WorldBook {
    /// The world folder of `owner`: the project's `World` folder, or the
    /// global library's under the base folder. A project with no folder yet
    /// keeps its world beside the old media folder.
    pub(crate) fn folder(&self, owner: Option<ProjectId>) -> PathBuf {
        match owner {
            Some(project) => match self.projects.project(project).ok().flatten() {
                Some(found) if found.folder.is_some() => found
                    .folder
                    .map(|folder| folder.join(WORLD_DIR))
                    .unwrap_or_default(),
                _ => self
                    .places
                    .old_media
                    .join(WORLD_DIR)
                    .join(project.to_string()),
            },
            None => {
                let chosen = self.folders.base_folder(self.owner).ok().flatten();
                self.places
                    .base(chosen.as_deref())
                    .join(GLOBAL_DIR)
                    .join(WORLD_DIR)
            }
        }
    }

    /// The look a video is drawn in now: its style, and for the notes its
    /// channel and their text.
    pub(crate) fn look_of(&self, video: &VideoProject, style: StyleChoice) -> Look {
        let notes = self
            .channels
            .get(video.channel)
            .ok()
            .flatten()
            .map(|channel| channel.details.aesthetic_notes().to_owned())
            .unwrap_or_default();
        Look::of(style, video.channel, &notes)
    }

    /// The look `video` is drawn in with `style`, read as a job runs.
    pub(crate) fn look_for(&self, video: VideoProjectId, style: StyleChoice) -> Option<Look> {
        let found = self.themes.project(video).ok().flatten()?;
        Some(self.look_of(&found, style))
    }

    /// How `video`'s pictures are rendered: its own style, else its
    /// channel's.
    fn style_of(&self, video: &VideoProject) -> StyleChoice {
        let channel = self.channels.get(video.channel).ok().flatten();
        video.style(channel.map_or(StyleChoice::default(), |channel| channel.details.style()))
    }

    /// [`WorldBook::lend_portrait`] for every member of the video's saved
    /// plan, in the video's look.
    pub(crate) fn lend_all(
        &self,
        files: &dyn ProjectFiles,
        plans: &dyn bardo_domain::ScenePlanRepository,
        video: VideoProjectId,
    ) -> Result<(), RepositoryError> {
        let Some(found) = self.themes.project(video)? else {
            return Ok(());
        };
        let Some(mut plan) = plans.scene_plan(video)? else {
            return Ok(());
        };
        let style = self.style_of(&found);
        let look = self.look_of(&found, style);
        for member in 0..plan.cast().len() {
            match self.lend_portrait(files, &mut plan, member, &look, None) {
                Ok(Some(replaced)) => {
                    plans.save_cast_member(&plan, member)?;
                    for index in plan.settle_portrait_waits(style) {
                        plans.save_scene(&plan, index)?;
                    }
                    if let Some(replaced) = replaced {
                        crate::proxies::remove_media(files, video, &replaced.file);
                    }
                }
                Ok(None) => {}
                // The images job finds it again and draws nothing for it.
                Err(failure) => tracing::warn!("a world portrait was not lent: {}", failure.detail),
            }
        }
        Ok(())
    }

    /// The character's kept picture for `look`.
    pub(crate) fn kept(
        &self,
        character: CharacterId,
        look: &Look,
    ) -> Result<Option<ReferencePicture>, RepositoryError> {
        Ok(self
            .world
            .pictures(character)?
            .into_iter()
            .find(|picture| picture.kept && picture.look == *look))
    }

    /// Gives cast member `member` of `plan` its character's kept picture
    /// for `look`, copied into the video's folder for free. `Ok(None)` when
    /// the member is no character, has a portrait in that look, or the
    /// character has no picture for it; `Ok(Some(replaced))` once it took
    /// one. A picture whose file is gone while its folder is there is let
    /// go, so the next portrait is drawn and kept; an unreachable folder
    /// fails, so nothing paid is drawn again.
    pub(crate) fn lend_portrait(
        &self,
        files: &dyn ProjectFiles,
        plan: &mut ScenePlan,
        member: usize,
        look: &Look,
        job: Option<JobId>,
    ) -> Result<Option<Option<SceneImage>>, JobFailure> {
        let style = look.style();
        let Ok(cast) = plan.cast_member(member) else {
            return Ok(None);
        };
        let Some(character) = cast.character() else {
            return Ok(None);
        };
        if cast.is_described_only() || cast.portrait_in(style).is_some() {
            return Ok(None);
        }
        let Some(found) = self.world.character(character).map_err(unexpected)? else {
            return Ok(None);
        };
        // The member follows the character only while it reads as one.
        if cast.description().as_str() != found.details.appearance() {
            return Ok(None);
        }
        let Some(picture) = self.kept(character, look).map_err(unexpected)? else {
            return Ok(None);
        };
        // A picture of an older appearance is no portrait of this one.
        if !drawn_from(&picture, found.details.appearance()) {
            return Ok(None);
        }
        let folder = self.folder(found.owner_project);
        let bytes = match self.files.read(&folder, &picture.file) {
            Ok(bytes) => bytes,
            Err(_) if !self.files.reachable(&folder) => {
                return Err(JobFailure::new(
                    JobFailureKind::Files,
                    format!("the world folder {} cannot be reached", folder.display()),
                ));
            }
            // Gone: let go, so the next portrait is drawn and kept.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!("a world picture's file is gone and is let go: {error}");
                self.world.remove_picture(picture.id).map_err(unexpected)?;
                return Ok(None);
            }
            // Held by another program for a moment: drawn this time, kept.
            Err(error) => {
                tracing::warn!("a world picture would not read: {error}");
                return Ok(None);
            }
        };
        let generation = GenerationId::new();
        let extension = std::path::Path::new(&picture.file)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("png");
        let file = format!("cast-{generation}.{extension}");
        files
            .write(plan.project, &file, &bytes)
            .map_err(unexpected)?;
        let portrait = SceneImage {
            file: file.clone(),
            generation: Generation {
                id: generation,
                owner: plan.owner,
                project: plan.project,
                provider: picture.provenance.provider,
                model: picture.provenance.model.clone(),
                template: plan.generation.template,
                instructions: format!("Reference picture {} from the world", picture.id),
                prompt: picture.provenance.prompt.clone(),
                output: file,
                usage: TokenUsage::default(),
                generated_at: SystemTime::now(),
                job,
            },
        };
        let replaced = plan
            .cast_mut(member, SystemTime::now())
            .map_err(unexpected)?
            .set_portrait(portrait);
        Ok(Some(replaced))
    }

    /// Keeps a portrait a video drew (or the user saved) for the
    /// character as its picture for `look`, unless it has one there.
    pub(crate) fn keep_portrait(
        &self,
        (character, appearance): (CharacterId, &str),
        look: Look,
        bytes: &[u8],
        drawn: &Generation,
        origin: PictureOrigin,
    ) -> Result<(), RepositoryError> {
        let Some(found) = self.world.character(character)? else {
            return Ok(());
        };
        // Drawn from an appearance the character no longer has.
        if found.details.appearance() != appearance {
            return Ok(());
        }
        if self.kept(character, &look)?.is_some() {
            return Ok(());
        }
        let id = ReferencePictureId::new();
        let file = picture_file(id, &drawn.output);
        let folder = self.folder(found.owner_project);
        if let Err(error) = self.files.write(&folder, &file, bytes) {
            tracing::warn!("could not keep a portrait in the world: {error}");
            return Ok(());
        }
        self.save_replacing(
            found.owner_project,
            &ReferencePicture {
                id,
                character,
                look,
                file,
                provenance: PictureProvenance {
                    provider: drawn.provider,
                    model: drawn.model.clone(),
                    prompt: drawn.prompt.clone(),
                    generated_at: drawn.generated_at,
                    job: drawn.job,
                    origin,
                },
                kept: true,
            },
        )
    }

    /// Saves `picture`, removing the file of the one of the same look and
    /// review state it replaces.
    pub(crate) fn save_replacing(
        &self,
        owner: Option<ProjectId>,
        picture: &ReferencePicture,
    ) -> Result<(), RepositoryError> {
        let replaced: Vec<ReferencePicture> = self
            .world
            .pictures(picture.character)?
            .into_iter()
            .filter(|other| {
                other.id != picture.id
                    && other.look == picture.look
                    && other.kept == picture.kept
                    && other.file != picture.file
            })
            .collect();
        self.world.save_picture(picture)?;
        let folder = self.folder(owner);
        for other in replaced {
            let _ = self.files.remove(&folder, &other.file);
        }
        Ok(())
    }

    /// Removes the picture and its file.
    fn drop_picture(&self, owner: Option<ProjectId>, picture: &ReferencePicture) {
        if let Err(error) = self.world.remove_picture(picture.id) {
            tracing::warn!("could not remove a world picture: {error}");
            return;
        }
        // A file that will not go only costs disk space.
        let _ = self.files.remove(&self.folder(owner), &picture.file);
    }
}

/// A world character's portrait prompt in `look`: a look from a channel's
/// notes adds the notes, since the character's appearance as written says
/// nothing of how the video is drawn (#337).
pub(crate) fn portrait_in_look(cast: &CastMember, look: &Look) -> String {
    let prompt = cast.portrait_prompt(look.style());
    match look.notes() {
        Some(notes) if !notes.trim().is_empty() => {
            format!(
                "{prompt}\n\nDraw it in the look of the video, from its aesthetic notes: {notes}"
            )
        }
        _ => prompt,
    }
}

/// Whether `picture` was drawn from `appearance`: its prompt holds it.
fn drawn_from(picture: &ReferencePicture, appearance: &str) -> bool {
    picture.provenance.prompt.contains(appearance)
}

/// The name of a world picture's file, keeping the drawing's extension.
fn picture_file(id: ReferencePictureId, drawn: &str) -> String {
    let extension = std::path::Path::new(drawn)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| extension.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("png");
    format!("character-{id}.{extension}")
}

/// A character as a cast member, for its portrait's prompt.
fn as_cast(character: &Character) -> Option<CastMember> {
    let description = ScenePrompt::new(character.details.appearance()).ok()?;
    Some(CastMember::restore(CastRecord {
        name: character.name().to_owned(),
        generated_description: description.clone(),
        description,
        portrait: None,
        failure: None,
    }))
}

#[derive(Debug, thiserror::Error)]
pub enum WorldError {
    #[error("not found")]
    NotFound,
    #[error("field: {0:?}")]
    Field(CharacterFieldError),
    /// Another item of the same owner has the name.
    #[error("the name is taken")]
    NameTaken,
    /// A video in progress uses it: archive it instead.
    #[error("in use: {0:?}")]
    InUse(Vec<String>),
    /// The new appearance drops these pictures and the portraits of these
    /// videos: the screen says so and asks again with `confirmed`.
    #[error("the appearance drops pictures")]
    DropsPictures {
        looks: Vec<Look>,
        videos: Vec<String>,
    },
    /// More characters than a video keeps.
    #[error("too many picked")]
    TooManyPicked,
    /// An instruction past its length.
    #[error("instruction too long")]
    InstructionTooLong,
    /// A character of another project.
    #[error("not this project's")]
    OtherProject,
    /// A picture is being drawn for it already.
    #[error("busy")]
    Busy,
    /// A video that shows it is planning or drawing its scenes.
    #[error("a video is busy")]
    VideoBusy,
    /// No picture waits for review.
    #[error("nothing waiting")]
    NothingWaiting,
    #[error("missing key")]
    MissingKey,
    /// The owner's folder cannot be reached: nothing is drawn or kept.
    #[error("the folder cannot be reached")]
    FolderNotFound,
    #[error("over budget")]
    OverBudget(SpendEstimate),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl From<AppError> for WorldError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Repository(error) => WorldError::Repository(error),
        }
    }
}

impl WorldError {
    pub fn message(&self) -> Text {
        match self {
            WorldError::NotFound => Text::WorldNotFound,
            WorldError::Field(error) => Text::CharacterFieldError(*error),
            WorldError::NameTaken => Text::WorldNameTaken,
            WorldError::InUse(_) => Text::CharacterInUse,
            WorldError::DropsPictures { .. } => Text::CharacterDropsPictures,
            WorldError::TooManyPicked => Text::WhoAppearsTooMany,
            WorldError::InstructionTooLong => Text::WhoAppearsInstructionTooLong,
            WorldError::OtherProject => Text::WorldNotFound,
            WorldError::Busy => Text::WorldPictureBusy,
            WorldError::VideoBusy => Text::WorldVideoBusy,
            WorldError::NothingWaiting => Text::WorldPictureNothingWaiting,
            WorldError::MissingKey => Text::WorldPictureMissingKey,
            WorldError::FolderNotFound => Text::FolderNotFound,
            WorldError::OverBudget(_) => Text::WorldPictureOverBudget,
            WorldError::Repository(_) => Text::WorldNotSaved,
        }
    }
}

/// A character's picture as a screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureView {
    pub picture: ReferencePicture,
    /// Where its file is.
    pub path: PathBuf,
}

/// A character as the World screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterCard {
    pub character: Character,
    /// Kept pictures, then the ones waiting for review.
    pub pictures: Vec<PictureView>,
    /// The videos that picked it or show it.
    pub uses: Vec<CharacterUse>,
    /// A picture of it is being drawn.
    pub drawing: bool,
}

/// What a video's *Who appears* shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WhoAppears {
    /// The characters picked, in order, with their instruction.
    pub picked: Vec<(Character, String)>,
    /// The active characters of the video's project, then the global
    /// library's, that may be picked.
    pub available: Vec<Character>,
    /// The plan was made before the last change.
    pub changed_after_plan: bool,
}

/// The world job's payload: the character, the look and the prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PicturePayload {
    character: String,
    /// The project the character belongs to, which the job is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_project: Option<String>,
    style: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    /// The appearance the prompt was written from.
    appearance: String,
    prompt: String,
}

impl PicturePayload {
    fn look(&self) -> Option<Look> {
        let channel = self
            .channel
            .as_deref()
            .and_then(|text| uuid::Uuid::parse_str(text).ok())
            .map(bardo_domain::ChannelId::from);
        Look::from_parts(&self.style, channel, self.notes.clone())
    }
}

/// One picture drawn from the World screen.
fn picture_call() -> PlannedCall {
    PlannedCall::new(Provider::Gemini, CostPurpose::WorldPicture, 1)
}

/// Draws a character's picture from the World screen (`JobKind::WorldPicture`):
/// it waits for the user's review, paid under the character's project.
pub(crate) struct WorldPictureHandler {
    pub(crate) owner: ProfileId,
    pub(crate) book: WorldBook,
    pub(crate) images: Arc<dyn ImageGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl WorldPictureHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::Gemini)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no gemini key is saved"))
    }
}

impl JobHandler for WorldPictureHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let job = cx.id();
        let payload: PicturePayload = parse(payload)?;
        let character: CharacterId = id(&payload.character)?;
        let look = payload
            .look()
            .ok_or_else(|| unexpected(format!("unknown look {}", payload.style)))?;
        let Some(found) = self.book.world.character(character).map_err(unexpected)? else {
            // Deleted meanwhile: nothing to draw for.
            return Ok(());
        };
        // Drawn by an earlier run cut short after saving it.
        let pictures = self.book.world.pictures(character).map_err(unexpected)?;
        if pictures
            .iter()
            .any(|picture| picture.provenance.job == Some(job))
        {
            return Ok(());
        }
        if found.details.appearance() != payload.appearance {
            return Err(unexpected("the character's appearance changed meanwhile"));
        }
        let folder = self.book.folder(found.owner_project);
        if !self.book.files.reachable(&folder) {
            return Err(JobFailure::new(
                JobFailureKind::Files,
                format!("the world folder {} cannot be reached", folder.display()),
            ));
        }
        let key = self.key()?;
        if cx.should_stop() {
            return Ok(());
        }
        let request = ImageRequest {
            prompt: payload.prompt.clone(),
            shape: ImageShape::Portrait,
            references: Vec::new(),
        };
        let call = picture_call();
        let image = {
            let _paying = self.costs.paying(job, &call, None, None)?;
            self.images.generate(&key, &request).map_err(|failure| {
                self.costs
                    .record_failed(&failure, &call, Some(job), None, None);
                JobFailure::new(failure.kind.into(), format!("Gemini: {}", failure.detail))
            })
        }?;
        self.costs.record_for_owner(
            PaidCall {
                provider: Provider::Gemini,
                model: &image.model,
                purpose: CostPurpose::WorldPicture,
                usage: image.usage,
                job: Some(job),
                reported: None,
            },
            found.owner_project,
        );
        // Read again: an appearance changed or a character deleted while
        // it was drawn is not what this picture shows.
        match self.book.world.character(character).map_err(unexpected)? {
            Some(now) if now.details.appearance() == payload.appearance => {}
            _ => return Ok(()),
        }
        let id = ReferencePictureId::new();
        let file = format!("character-{id}.{}", image.format.extension());
        self.book
            .files
            .write(&folder, &file, &image.bytes)
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))?;
        self.book
            .save_replacing(
                found.owner_project,
                &ReferencePicture {
                    id,
                    character,
                    look,
                    file,
                    provenance: PictureProvenance {
                        provider: Provider::Gemini,
                        model: image.model,
                        prompt: payload.prompt,
                        generated_at: SystemTime::now(),
                        job: Some(job),
                        origin: PictureOrigin::World,
                    },
                    kept: false,
                },
            )
            .map_err(unexpected)
    }
}

impl Bardo {
    /// The open project's characters (`Some`), or the global library's
    /// (`None`), with their pictures and uses; archived ones only with
    /// `archived`.
    pub fn world(
        &self,
        owner: Option<ProjectId>,
        archived: bool,
    ) -> Result<Vec<CharacterCard>, WorldError> {
        let drawing: Vec<String> = self
            .jobs()
            .into_iter()
            .filter(|job| job.kind() == JobKind::WorldPicture && job.state().is_active())
            .filter_map(|job| serde_json::from_str::<PicturePayload>(job.payload()).ok())
            .map(|payload| payload.character)
            .collect();
        let folder = self.world_book.folder(owner);
        self.world_book
            .world
            .characters(self.profile.id, owner)?
            .into_iter()
            .filter(|character| archived || !character.is_archived())
            .map(|character| {
                let mut pictures: Vec<PictureView> = self
                    .world_book
                    .world
                    .pictures(character.id)?
                    .into_iter()
                    .map(|picture| PictureView {
                        path: self.world_book.files.path(&folder, &picture.file),
                        picture,
                    })
                    .collect();
                pictures.sort_by_key(|view| !view.picture.kept);
                Ok(CharacterCard {
                    uses: self.world_book.world.character_uses(character.id)?,
                    drawing: drawing.contains(&character.id.to_string()),
                    character,
                    pictures,
                })
            })
            .collect()
    }

    /// The character, when it is the profile's.
    fn own_character(&self, id: CharacterId) -> Result<Character, WorldError> {
        self.world_book
            .world
            .character(id)?
            .filter(|character| character.owner == self.profile.id)
            .ok_or(WorldError::NotFound)
    }

    /// Fails when another character of `owner` has the name.
    fn name_free(
        &self,
        owner: Option<ProjectId>,
        name: &str,
        except: Option<CharacterId>,
    ) -> Result<(), WorldError> {
        let taken = self
            .world_book
            .world
            .characters(self.profile.id, owner)?
            .into_iter()
            .any(|other| Some(other.id) != except && other.details.same_name(name));
        match taken {
            true => Err(WorldError::NameTaken),
            false => Ok(()),
        }
    }

    /// Adds a character to `owner`'s world (`None`: the global library).
    pub fn create_character(
        &self,
        owner: Option<ProjectId>,
        draft: CharacterDraft,
    ) -> Result<Character, WorldError> {
        if let Some(project) = owner {
            self.projects
                .project(project)?
                .filter(|found| found.owner == self.profile.id)
                .ok_or(WorldError::NotFound)?;
        }
        let details = CharacterDetails::validate(draft).map_err(WorldError::Field)?;
        self.name_free(owner, details.name(), None)?;
        let character = Character::new(self.profile.id, owner, details, SystemTime::now());
        self.world_book.world.save_character(&character)?;
        Ok(character)
    }

    /// Saves the character's new fields. A new appearance drops its
    /// pictures in every look and the portraits of the videos that show
    /// it: without `confirmed`, it says which first
    /// (`WorldError::DropsPictures`). Its name and notes keep them.
    pub fn edit_character(
        &self,
        id: CharacterId,
        draft: CharacterDraft,
        confirmed: bool,
    ) -> Result<Character, WorldError> {
        let mut character = self.own_character(id)?;
        let details = CharacterDetails::validate(draft).map_err(WorldError::Field)?;
        self.name_free(character.owner_project, details.name(), Some(id))?;
        let new_look = details.appearance() != character.details.appearance();
        if new_look {
            let busy = self
                .world_book
                .world
                .character_uses(id)?
                .into_iter()
                .any(|used| !used.archived && self.scenes_idle(used.video).is_err());
            if busy {
                return Err(WorldError::VideoBusy);
            }
        }
        let pictures = self.world_book.world.pictures(id)?;
        if new_look && !confirmed {
            let videos: Vec<String> = self
                .world_book
                .world
                .character_uses(id)?
                .into_iter()
                .filter(|used| !used.archived)
                .map(|used| used.title)
                .collect();
            if !pictures.is_empty() || !videos.is_empty() {
                let mut looks: Vec<Look> = Vec::new();
                for picture in &pictures {
                    if !looks.contains(&picture.look) {
                        looks.push(picture.look.clone());
                    }
                }
                return Err(WorldError::DropsPictures { looks, videos });
            }
        }
        character.details = details;
        self.world_book.world.save_character(&character)?;
        if new_look {
            for picture in &pictures {
                self.world_book
                    .drop_picture(character.owner_project, picture);
            }
            self.follow_new_look(&character)?;
        }
        Ok(character)
    }

    /// Gives the videos that show `character` (and are not archived) its
    /// new appearance: each one's portrait goes, so the scenes drawn with
    /// it read *old portrait* and the next one is drawn again.
    fn follow_new_look(&self, character: &Character) -> Result<(), WorldError> {
        let appearance = ScenePrompt::new(character.details.appearance())
            .map_err(|_| WorldError::Field(CharacterFieldError::AppearanceTooLong))?;
        for used in self.world_book.world.character_uses(character.id)? {
            if used.archived {
                continue;
            }
            let Some(mut plan) = self.scene_plans.scene_plan(used.video)? else {
                continue;
            };
            let members: Vec<usize> = plan
                .cast()
                .iter()
                .enumerate()
                .filter(|(_, member)| member.character() == Some(character.id))
                .map(|(index, _)| index)
                .collect();
            for member in members {
                let now = SystemTime::now();
                let Ok(cast) = plan.cast_mut(member, now) else {
                    continue;
                };
                cast.name = character.name().to_owned();
                let (_, dropped) = cast.edit_description(appearance.clone());
                self.scene_plans.save_cast_member(&plan, member)?;
                if let Some(dropped) = dropped {
                    crate::proxies::remove_media(self.files.as_ref(), used.video, &dropped.file);
                }
            }
        }
        Ok(())
    }

    /// A copy of the character in the same world, under a free name, with
    /// copies of its kept pictures.
    pub fn duplicate_character(&self, id: CharacterId) -> Result<Character, WorldError> {
        let original = self.own_character(id)?;
        let others = self
            .world_book
            .world
            .characters(self.profile.id, original.owner_project)?;
        let name = (2..)
            .map(|n| {
                let suffix = format!(" ({n})");
                let room = CharacterDetails::MAX_NAME_CHARS - suffix.chars().count();
                let base: String = original.name().chars().take(room).collect();
                format!("{}{suffix}", base.trim_end())
            })
            .find(|name| !others.iter().any(|other| other.details.same_name(name)))
            .unwrap_or_default();
        let mut draft = CharacterDraft::from(&original.details);
        draft.name = name;
        let details = CharacterDetails::validate(draft).map_err(WorldError::Field)?;
        let copy = Character::new(
            self.profile.id,
            original.owner_project,
            details,
            SystemTime::now(),
        );
        self.world_book.world.save_character(&copy)?;
        let folder = self.world_book.folder(original.owner_project);
        for picture in self.world_book.world.pictures(id)? {
            if !picture.kept {
                continue;
            }
            let Ok(bytes) = self.world_book.files.read(&folder, &picture.file) else {
                continue;
            };
            let id = ReferencePictureId::new();
            let file = picture_file(id, &picture.file);
            if self.world_book.files.write(&folder, &file, &bytes).is_err() {
                continue;
            }
            self.world_book.world.save_picture(&ReferencePicture {
                id,
                character: copy.id,
                file,
                provenance: PictureProvenance {
                    origin: PictureOrigin::Copied,
                    ..picture.provenance
                },
                ..picture
            })?;
        }
        Ok(copy)
    }

    /// Archives the character (hidden from *Who appears*, kept with its
    /// pictures), or brings it back.
    pub fn archive_character(&self, id: CharacterId, archived: bool) -> Result<(), WorldError> {
        let mut character = self.own_character(id)?;
        character.archived_at = archived.then(SystemTime::now);
        Ok(self.world_book.world.save_character(&character)?)
    }

    /// Deletes the character with its pictures. Refused while a video that
    /// is not archived picked it or shows it; archived videos keep their
    /// cast member as their own.
    pub fn delete_character(&self, id: CharacterId) -> Result<(), WorldError> {
        let character = self.own_character(id)?;
        let busy: Vec<String> = self
            .world_book
            .world
            .character_uses(id)?
            .into_iter()
            .filter(|used| !used.archived)
            .map(|used| used.title)
            .collect();
        if !busy.is_empty() {
            return Err(WorldError::InUse(busy));
        }
        let pictures = self.world_book.world.pictures(id)?;
        self.world_book.world.delete_character(id)?;
        let folder = self.world_book.folder(character.owner_project);
        for picture in pictures {
            let _ = self.world_book.files.remove(&folder, &picture.file);
        }
        Ok(())
    }

    /// The looks a character may be drawn in from the World screen: every
    /// catalog style, and the notes of each of the open project's channels.
    pub fn world_looks(&self) -> Result<Vec<(Look, String)>, WorldError> {
        let mut looks: Vec<(Look, String)> = Vec::new();
        for channel in self.channels()? {
            looks.push((
                Look::of(
                    StyleChoice::FromNotes,
                    channel.id,
                    channel.details.aesthetic_notes(),
                ),
                channel.details.name().to_owned(),
            ));
        }
        looks.extend(
            bardo_domain::VisualStyle::ALL
                .into_iter()
                .map(|style| (Look::Style(style), String::new())),
        );
        Ok(looks)
    }

    /// What drawing one picture costs, as the World screen shows it.
    pub fn world_picture_estimate(&self) -> Result<SpendEstimate, WorldError> {
        Ok(self.estimate(&[picture_call()])?)
    }

    /// Starts a job that draws the character's picture for `look`; it waits
    /// for review. Past Gemini's budget it needs `consent`.
    pub fn draw_world_picture(
        &self,
        id: CharacterId,
        look: Look,
        consent: BudgetConsent,
    ) -> Result<JobId, WorldError> {
        let character = self.own_character(id)?;
        let busy = self.jobs().into_iter().any(|job| {
            job.kind() == JobKind::WorldPicture
                && job.state().is_active()
                && serde_json::from_str::<PicturePayload>(job.payload())
                    .is_ok_and(|payload| payload.character == id.to_string())
        });
        if busy {
            return Err(WorldError::Busy);
        }
        if self.provider_key(Provider::Gemini).state == crate::KeyState::NotSet {
            return Err(WorldError::MissingKey);
        }
        let folder = self.world_book.folder(character.owner_project);
        if !self.world_book.files.reachable(&folder) {
            return Err(WorldError::FolderNotFound);
        }
        if let Err(estimate) = self.check_budget(&[picture_call()], consent)? {
            return Err(WorldError::OverBudget(estimate));
        }
        let prompt = as_cast(&character)
            .map(|cast| portrait_in_look(&cast, &look))
            .ok_or(WorldError::Field(CharacterFieldError::AppearanceTooLong))?;
        let payload = PicturePayload {
            character: id.to_string(),
            owner_project: character.owner_project.map(|id| id.to_string()),
            style: look.code().to_owned(),
            channel: look.channel().map(|id| id.to_string()),
            notes: look.notes().map(str::to_owned),
            appearance: character.details.appearance().to_owned(),
            prompt,
        };
        let job = Job::new(self.profile.id, JobKind::WorldPicture, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Keeps a picture waiting for review: videos of its look use it from
    /// now on, in place of the one kept before.
    pub fn keep_world_picture(
        &self,
        character: CharacterId,
        picture: ReferencePictureId,
    ) -> Result<(), WorldError> {
        let found = self.own_character(character)?;
        let pictures = self.world_book.world.pictures(character)?;
        let mut waiting = pictures
            .iter()
            .find(|candidate| candidate.id == picture && !candidate.kept)
            .cloned()
            .ok_or(WorldError::NothingWaiting)?;
        if !drawn_from(&waiting, found.details.appearance()) {
            self.world_book.drop_picture(found.owner_project, &waiting);
            return Err(WorldError::NothingWaiting);
        }
        let replaced = pictures
            .iter()
            .find(|candidate| candidate.kept && candidate.look == waiting.look)
            .cloned();
        waiting.kept = true;
        self.world_book.world.save_picture(&waiting)?;
        if let Some(replaced) = replaced {
            let _ = self
                .world_book
                .files
                .remove(&self.world_book.folder(found.owner_project), &replaced.file);
        }
        Ok(())
    }

    /// Drops a picture: one waiting for review, or a kept one (the next
    /// video of its look draws a new one).
    pub fn discard_world_picture(
        &self,
        character: CharacterId,
        picture: ReferencePictureId,
    ) -> Result<(), WorldError> {
        let found = self.own_character(character)?;
        let picture = self
            .world_book
            .world
            .pictures(character)?
            .into_iter()
            .find(|candidate| candidate.id == picture)
            .ok_or(WorldError::NothingWaiting)?;
        self.world_book.drop_picture(found.owner_project, &picture);
        Ok(())
    }

    /// The project a video belongs to, through its channel.
    fn video_owner(&self, video: &VideoProject) -> Result<Option<ProjectId>, RepositoryError> {
        Ok(self
            .channels
            .get(video.channel)?
            .map(|channel| channel.project))
    }

    /// Who appears in the video: what it picked and what it may pick.
    pub fn who_appears(&self, video: VideoProjectId) -> Result<WhoAppears, WorldError> {
        let found = self
            .themes
            .project(video)?
            .filter(|found| found.owner == self.profile.id)
            .ok_or(WorldError::NotFound)?;
        let owner = self.video_owner(&found)?;
        let mut available = match owner {
            Some(project) => self
                .world_book
                .world
                .characters(self.profile.id, Some(project))?,
            None => Vec::new(),
        };
        available.extend(self.world_book.world.characters(self.profile.id, None)?);
        let mut picked = Vec::new();
        for pick in self.world_book.world.picks(video)? {
            if let Some(character) = available.iter().find(|c| c.id == pick.character) {
                picked.push((character.clone(), pick.instruction));
            }
        }
        available.retain(|character| !character.is_archived());
        Ok(WhoAppears {
            picked,
            available,
            changed_after_plan: self.world_book.world.picks_changed(video)?,
        })
    }

    /// Sets who appears in the video. Each character is the video's
    /// project's or global, at most `MAX_PICKED`, each instruction within
    /// its length. A change after planning marks the plan for review.
    pub fn set_who_appears(
        &self,
        video: VideoProjectId,
        picks: Vec<CastPick>,
    ) -> Result<(), WorldError> {
        let found = self
            .themes
            .project(video)?
            .filter(|found| found.owner == self.profile.id)
            .ok_or(WorldError::NotFound)?;
        let owner = self.video_owner(&found)?;
        let mut picks: Vec<CastPick> = picks
            .into_iter()
            .map(|pick| CastPick {
                instruction: pick.instruction.trim().to_owned(),
                ..pick
            })
            .collect();
        let mut seen: Vec<CharacterId> = Vec::new();
        picks.retain(|pick| {
            let first = !seen.contains(&pick.character);
            seen.push(pick.character);
            first
        });
        if picks.len() > MAX_PICKED {
            return Err(WorldError::TooManyPicked);
        }
        for pick in &picks {
            if pick.instruction.chars().count() > CastPick::MAX_INSTRUCTION_CHARS {
                return Err(WorldError::InstructionTooLong);
            }
            let character = self.own_character(pick.character)?;
            if character.owner_project.is_some() && character.owner_project != owner {
                return Err(WorldError::OtherProject);
            }
        }
        let before = self.world_book.world.picks(video)?;
        if before == picks {
            return Ok(());
        }
        let planned = self.scene_plans.scene_plan(video)?.is_some();
        Ok(self.world_book.world.set_picks(video, &picks, planned)?)
    }

    /// Saves cast member `member` of the video's plan to the world of the
    /// video's project, or to the global library's with `global`, with its
    /// portrait for the video's look, and makes the member that character.
    pub fn save_cast_to_world(
        &self,
        video: VideoProjectId,
        member: usize,
        global: bool,
    ) -> Result<Character, WorldError> {
        let found = self
            .themes
            .project(video)?
            .filter(|found| found.owner == self.profile.id)
            .ok_or(WorldError::NotFound)?;
        let owner = match global {
            true => None,
            false => Some(self.video_owner(&found)?.ok_or(WorldError::NotFound)?),
        };
        let mut plan = self
            .scene_plans
            .scene_plan(video)?
            .ok_or(WorldError::NotFound)?;
        let cast = plan
            .cast_member(member)
            .map_err(|_| WorldError::NotFound)?
            .clone();
        if cast.character().is_some() {
            return Err(WorldError::NameTaken);
        }
        // A job drawing the scenes holds the plan: it would write the member
        // back unlinked.
        if self.scenes_idle(video).is_err() {
            return Err(WorldError::VideoBusy);
        }
        let name = cast
            .name
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .to_owned();
        let character = self.create_character(
            owner,
            CharacterDraft {
                name: if name.is_empty() {
                    cast.name.clone()
                } else {
                    name
                },
                appearance: cast.description().as_str().to_owned(),
                ..CharacterDraft::default()
            },
        )?;
        let style = self.video_style(&found)?;
        if let Some(portrait) = cast.portrait_in(style)
            && let Ok(bytes) = self.files.read(video, &portrait.file)
        {
            let look = self.world_book.look_of(&found, style);
            self.world_book.keep_portrait(
                (character.id, cast.description().as_str()),
                look,
                &bytes,
                &portrait.generation,
                PictureOrigin::Saved,
            )?;
        }
        if let Ok(cast) = plan.cast_mut(member, SystemTime::now()) {
            *cast = cast.clone().with_character(Some(character.id));
            self.scene_plans.save_cast_member(&plan, member)?;
        }
        // The video now shows the character: it is picked, so planning
        // again keeps it, while there is room. The plan already has it.
        let mut picks = self.world_book.world.picks(video)?;
        if picks.len() < MAX_PICKED {
            picks.push(CastPick {
                character: character.id,
                instruction: String::new(),
            });
            self.world_book.world.set_picks(video, &picks, false)?;
        }
        Ok(character)
    }

    /// Gives every member of the video's plan that is a character with a
    /// kept picture for the video's look that picture as portrait, for
    /// free, so estimates count only the portraits still to draw.
    pub(crate) fn lend_world_portraits(
        &self,
        video: VideoProjectId,
    ) -> Result<(), RepositoryError> {
        self.world_book
            .lend_all(self.files.as_ref(), self.scene_plans.as_ref(), video)
    }

    /// The characters the video picked, with their instructions, as the
    /// scene planner gets them: a character gone, or of another project,
    /// is left out.
    pub(crate) fn fixed_cast(
        &self,
        video: &VideoProject,
    ) -> Result<Vec<(Character, String)>, RepositoryError> {
        let owner = self.video_owner(video)?;
        let mut fixed = Vec::new();
        for pick in self.world_book.world.picks(video.id)? {
            if let Some(character) = self.world_book.world.character(pick.character)?
                && (character.owner_project.is_none() || character.owner_project == owner)
            {
                fixed.push((character, pick.instruction));
            }
        }
        Ok(fixed)
    }

    /// Whether `name` is taken by another character of `owner`.
    pub fn character_name_taken(&self, owner: Option<ProjectId>, name: &str) -> bool {
        self.world_book
            .world
            .characters(self.profile.id, owner)
            .is_ok_and(|characters| characters.iter().any(|c| same_name(c.name(), name)))
    }
}

/// What the scene planner is told of the fixed cast: one numbered entry
/// per picked character, or that none was picked.
pub(crate) fn fixed_cast_lines(fixed: &[(Character, String)]) -> String {
    if fixed.is_empty() {
        return "None: no character was picked for this video.".to_owned();
    }
    let mut text = String::from(
        "Put each of these characters in the cast under this exact name and with this \
         exact appearance, and show them in the scenes their notes and the story call for. \
         Add other people to the cast only for someone none of them can be.",
    );
    for (number, (character, instruction)) in fixed.iter().enumerate() {
        text.push_str(&format!(
            "\n{}. {}",
            number + 1,
            character.planner_lines(instruction)
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        ChannelDraft, CostPurpose, CostRepository, ImageShape, JobState, Niche, Theme, ThemeIdea,
        VideoProject,
    };

    use super::*;
    use crate::Removable;
    use crate::scenes::tests::{Harness, SCRIPT, done, project};

    const ROSA: &str = "A woman in her seventies, white hair in a bun, green wool shawl";
    const BENTO: &str = "A boy of ten, red cap, freckles, striped shirt";

    fn owner(app: &Bardo) -> Option<ProjectId> {
        app.open_project().unwrap().map(|project| project.id)
    }

    fn character(app: &Bardo, owner: Option<ProjectId>, name: &str, look: &str) -> Character {
        app.create_character(
            owner,
            CharacterDraft {
                name: name.into(),
                appearance: look.into(),
                always: "Carries a red umbrella".into(),
                never: "Never runs".into(),
            },
        )
        .unwrap()
    }

    /// Another video on the channel of `video`.
    fn video_on(app: &Bardo, video: &VideoProject, title: &str) -> VideoProject {
        let mut theme = Theme::suggested(
            app.profile().id,
            video.channel,
            Niche::new("space history").unwrap(),
            ThemeIdea::new(title, "").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let next = theme.approve(SystemTime::now()).unwrap();
        app.themes.start_project(&theme, &next).unwrap();
        next
    }

    fn narrate(h: &Harness, app: &Bardo, video: &VideoProject) {
        h.answer(SCRIPT.to_owned());
        done(
            app,
            app.generate_script(video.id, BudgetConsent::Ask).unwrap(),
        );
        done(
            app,
            app.generate_narration(video.id, crate::PicturesConsent::Ask, BudgetConsent::Ask)
                .unwrap(),
        );
    }

    /// Rosa, as Claude names and describes her in its own words, in the
    /// first and the last scene.
    fn rosa_answer() -> String {
        serde_json::json!({
            "cast": [{"name": "Rosa", "description": "An old lady with a shawl"}],
            "scenes": [
                {"first_sentence": 0, "prompt": "Rosa at the launch pad", "cast": [0],
                 "motion": "Slow push in"},
                {"first_sentence": 1, "prompt": "The probe drifting away", "cast": [],
                 "motion": "The probe drifts left"},
                {"first_sentence": 2, "prompt": "Rosa in a control room", "cast": [0],
                 "motion": "Static shot"},
            ],
        })
        .to_string()
    }

    fn plan(h: &Harness, app: &Bardo, video: &VideoProject) {
        h.answer(rosa_answer());
        done(
            app,
            app.plan_scenes(video.id, true, BudgetConsent::Ask).unwrap(),
        );
    }

    fn draw(app: &Bardo, video: &VideoProject) {
        done(
            app,
            app.generate_scene_images(video.id, BudgetConsent::Ask)
                .unwrap(),
        );
    }

    fn pick(app: &Bardo, video: &VideoProject, picked: &[(&Character, &str)]) {
        app.set_who_appears(
            video.id,
            picked
                .iter()
                .map(|(character, instruction)| CastPick {
                    character: character.id,
                    instruction: (*instruction).into(),
                })
                .collect(),
        )
        .unwrap();
    }

    fn portraits_asked(h: &Harness) -> usize {
        h.images
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.shape == ImageShape::Portrait)
            .count()
    }

    fn kept_pictures(app: &Bardo, character: CharacterId) -> Vec<ReferencePicture> {
        app.world_book
            .world
            .pictures(character)
            .unwrap()
            .into_iter()
            .filter(|picture| picture.kept)
            .collect()
    }

    #[test]
    fn an_unpicked_character_never_reaches_the_planner() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let owner = owner(&app);
        let rosa = character(&app, owner, "Rosa", ROSA);
        let bento = character(&app, None, "Bento", BENTO);
        pick(&app, &video, &[(&rosa, "Only in the opening and the end")]);

        plan(&h, &app, &video);
        let request = h.text.requests().pop().unwrap();
        let sent = format!("{}\n{}", request.instructions, request.prompt);
        assert!(sent.contains("Rosa") && sent.contains(ROSA));
        assert!(sent.contains("Only in the opening and the end"));
        assert!(sent.contains("Carries a red umbrella"));
        assert!(!sent.contains("Bento") && !sent.contains(BENTO), "{sent}");
        let available = app.who_appears(video.id).unwrap().available;
        assert!(available.iter().any(|c| c.id == bento.id), "it could be");
    }

    #[test]
    fn with_no_character_picked_the_plan_is_asked_as_before() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        character(&app, owner(&app), "Rosa", ROSA);
        plan(&h, &app, &video);
        let request = h.text.requests().pop().unwrap();
        assert!(!request.prompt.contains("fixed_cast"));
        assert!(!request.prompt.contains("Rosa"));
        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        assert_eq!(plan.cast()[0].character(), None, "an extra of the video");
        assert_eq!(
            plan.cast()[0].description().as_str(),
            "An old lady with a shawl"
        );
    }

    #[test]
    fn a_picked_character_is_cast_as_written_and_its_portrait_is_kept_in_the_world() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let owner = owner(&app);
        let rosa = character(&app, owner, "Rosa", ROSA);
        pick(&app, &video, &[(&rosa, "")]);
        plan(&h, &app, &video);

        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        let member = &plan.cast()[0];
        assert_eq!(member.character(), Some(rosa.id));
        assert_eq!(member.name, "Rosa");
        assert_eq!(member.description().as_str(), ROSA, "the world's words");

        draw(&app, &video);
        assert_eq!(portraits_asked(&h), 1);
        let portrait = h.images.requests.lock().unwrap()[0].prompt.clone();
        assert!(portrait.contains(ROSA));
        assert!(
            portrait.contains("grainy 1960s film"),
            "the channel's notes are the look: {portrait}"
        );
        let kept = kept_pictures(&app, rosa.id);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].provenance.origin, PictureOrigin::Video);
        let folder = app.world_book.folder(owner);
        assert_eq!(h.world_files.names(&folder), [kept[0].file.clone()]);
    }

    #[test]
    fn a_second_video_in_the_same_look_reuses_the_portrait_and_other_notes_draw_again() {
        let h = Harness::new();
        let mut app = h.start();
        let first = h.narrated_project(&app);
        let owner = owner(&app);
        let rosa = character(&app, owner, "Rosa", ROSA);
        pick(&app, &first, &[(&rosa, "")]);
        plan(&h, &app, &first);
        draw(&app, &first);
        assert_eq!(portraits_asked(&h), 1);
        let picture = kept_pictures(&app, rosa.id).remove(0);

        let second = video_on(&app, &first, "The second probe");
        narrate(&h, &app, &second);
        pick(&app, &second, &[(&rosa, "")]);
        plan(&h, &app, &second);
        let view = app.scenes(second.id).unwrap();
        assert_eq!(
            view.missing_images(),
            3,
            "the scenes only: her portrait is lent"
        );
        draw(&app, &second);
        assert_eq!(portraits_asked(&h), 1, "no paid call for her portrait");
        let planned = app.scenes(second.id).unwrap().plan.unwrap();
        let portrait = planned.cast()[0].portrait().unwrap();
        let world = app.world_book.folder(owner);
        assert_eq!(
            h.files.read(second.id, &portrait.file).unwrap(),
            h.world_files.read(&world, &picture.file).unwrap()
        );
        assert_eq!(portrait.generation.usage, TokenUsage::default(), "free");

        // Deleting the video that drew it leaves the picture.
        app.deletion(Removable::Video(first.id), &first.title, true)
            .unwrap()
            .run()
            .unwrap();
        assert_eq!(kept_pictures(&app, rosa.id), std::slice::from_ref(&picture));
        assert!(h.world_files.read(&world, &picture.file).is_ok());

        // Other notes on the channel are another look.
        let channel = app
            .channels()
            .unwrap()
            .into_iter()
            .find(|channel| channel.id == second.channel)
            .unwrap();
        let mut draft = ChannelDraft::from(&channel.details);
        draft.aesthetic_notes = "bright watercolor".into();
        app.update_channel(channel.id, draft).unwrap();
        let third = video_on(&app, &second, "The third probe");
        narrate(&h, &app, &third);
        pick(&app, &third, &[(&rosa, "")]);
        plan(&h, &app, &third);
        draw(&app, &third);
        assert_eq!(portraits_asked(&h), 2, "a new look draws a new portrait");
        assert_eq!(kept_pictures(&app, rosa.id).len(), 2);
    }

    #[test]
    fn two_videos_drawn_at_once_pay_for_a_shared_portrait_once() {
        let h = Harness::new();
        let app = h.start();
        let first = h.narrated_project(&app);
        let rosa = character(&app, owner(&app), "Rosa", ROSA);
        pick(&app, &first, &[(&rosa, "")]);
        plan(&h, &app, &first);
        let second = video_on(&app, &first, "The second probe");
        narrate(&h, &app, &second);
        pick(&app, &second, &[(&rosa, "")]);
        plan(&h, &app, &second);

        // Each call takes a while, so both jobs reach her portrait together.
        *h.images.delay.lock().unwrap() = std::time::Duration::from_millis(150);
        let one = app
            .generate_scene_images(first.id, BudgetConsent::Ask)
            .unwrap();
        let two = app
            .generate_scene_images(second.id, BudgetConsent::Ask)
            .unwrap();
        done(&app, one);
        done(&app, two);

        assert_eq!(portraits_asked(&h), 1, "one paid call for her portrait");
        assert_eq!(kept_pictures(&app, rosa.id).len(), 1);
        for video in [&first, &second] {
            let planned = app.scenes(video.id).unwrap().plan.unwrap();
            assert!(planned.cast()[0].portrait().is_some(), "both show her");
        }
    }

    #[test]
    fn changing_who_appears_after_planning_marks_the_plan_for_review() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let rosa = character(&app, owner(&app), "Rosa", ROSA);
        pick(&app, &video, &[(&rosa, "")]);
        assert!(!app.who_appears(video.id).unwrap().changed_after_plan);
        plan(&h, &app, &video);
        assert!(!app.who_appears(video.id).unwrap().changed_after_plan);

        pick(&app, &video, &[(&rosa, "Smiles at the end")]);
        assert!(app.who_appears(video.id).unwrap().changed_after_plan);
        plan(&h, &app, &video);
        assert!(
            !app.who_appears(video.id).unwrap().changed_after_plan,
            "a new plan"
        );
    }

    #[test]
    fn who_appears_keeps_at_most_three_of_the_videos_project_or_the_library() {
        let h = Harness::new();
        let app = h.start();
        let video = project(&app);
        let owner = owner(&app);
        let picked: Vec<Character> = (0..4)
            .map(|n| character(&app, owner, &format!("Person {n}"), ROSA))
            .collect();
        let picks: Vec<CastPick> = picked
            .iter()
            .map(|c| CastPick {
                character: c.id,
                instruction: String::new(),
            })
            .collect();
        assert!(matches!(
            app.set_who_appears(video.id, picks.clone()),
            Err(WorldError::TooManyPicked)
        ));
        assert!(matches!(
            app.set_who_appears(
                video.id,
                vec![CastPick {
                    character: picked[0].id,
                    instruction: "x".repeat(CastPick::MAX_INSTRUCTION_CHARS + 1),
                }]
            ),
            Err(WorldError::InstructionTooLong)
        ));
        let other = app
            .create_project(bardo_domain::ProjectDraft {
                name: "Another".into(),
                ..Default::default()
            })
            .unwrap();
        let stranger = character(&app, Some(other.id), "Stranger", ROSA);
        assert!(matches!(
            app.set_who_appears(
                video.id,
                vec![CastPick {
                    character: stranger.id,
                    instruction: String::new(),
                }]
            ),
            Err(WorldError::OtherProject)
        ));
        let global = character(&app, None, "Everyone's", BENTO);
        pick(&app, &video, &[(&picked[0], "first"), (&global, "")]);
        let shown = app.who_appears(video.id).unwrap();
        let names: Vec<&str> = shown.picked.iter().map(|(c, _)| c.name()).collect();
        assert_eq!(names, ["Person 0", "Everyone's"]);
        assert!(!shown.available.iter().any(|c| c.id == stranger.id));
    }

    #[test]
    fn a_new_appearance_asks_first_then_drops_every_picture_and_the_videos_portraits() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let owner = owner(&app);
        let rosa = character(&app, owner, "Rosa", ROSA);
        pick(&app, &video, &[(&rosa, "")]);
        plan(&h, &app, &video);
        draw(&app, &video);
        let picture = kept_pictures(&app, rosa.id).remove(0);
        let world = app.world_book.folder(owner);

        // A new name and notes keep the pictures.
        let mut draft = CharacterDraft::from(&rosa.details);
        draft.name = "Dona Rosa".into();
        draft.never = "Never shouts".into();
        app.edit_character(rosa.id, draft.clone(), false).unwrap();
        assert_eq!(kept_pictures(&app, rosa.id).len(), 1);

        draft.appearance = "A woman in her seventies, short grey hair, red coat".into();
        match app.edit_character(rosa.id, draft.clone(), false) {
            Err(WorldError::DropsPictures { looks, videos }) => {
                assert_eq!(looks, std::slice::from_ref(&picture.look));
                assert_eq!(videos, std::slice::from_ref(&video.title));
            }
            other => panic!("asks first: {other:?}"),
        }
        assert_eq!(kept_pictures(&app, rosa.id).len(), 1, "nothing changed yet");

        app.edit_character(rosa.id, draft, true).unwrap();
        assert!(kept_pictures(&app, rosa.id).is_empty());
        assert!(h.world_files.read(&world, &picture.file).is_err());
        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        assert_eq!(
            plan.cast()[0].description().as_str(),
            "A woman in her seventies, short grey hair, red coat"
        );
        assert!(
            plan.cast()[0].portrait().is_none(),
            "drawn again when needed"
        );
    }

    #[test]
    fn a_character_in_use_cannot_be_deleted_but_can_be_archived() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let rosa = character(&app, owner(&app), "Rosa", ROSA);
        pick(&app, &video, &[(&rosa, "")]);
        plan(&h, &app, &video);
        match app.delete_character(rosa.id) {
            Err(WorldError::InUse(titles)) => {
                assert_eq!(titles, std::slice::from_ref(&video.title))
            }
            other => panic!("refused: {other:?}"),
        }
        app.archive_character(rosa.id, true).unwrap();
        assert!(app.who_appears(video.id).unwrap().available.is_empty());
        assert_eq!(app.who_appears(video.id).unwrap().picked.len(), 1, "kept");

        app.archive_video(video.id).unwrap();
        app.delete_character(rosa.id).unwrap();
        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        assert_eq!(plan.cast()[0].character(), None, "an extra now");
        assert!(app.who_appears(video.id).unwrap().picked.is_empty());
    }

    #[test]
    fn names_are_unique_per_owner_ignoring_case() {
        let h = Harness::new();
        let app = h.start();
        project(&app);
        let owner = owner(&app);
        assert!(owner.is_some());
        character(&app, owner, "Rosa", ROSA);
        let taken = app.create_character(
            owner,
            CharacterDraft {
                name: " rosa ".into(),
                appearance: BENTO.into(),
                ..CharacterDraft::default()
            },
        );
        assert!(matches!(taken, Err(WorldError::NameTaken)));
        character(&app, None, "Rosa", ROSA);
        let copy = app
            .duplicate_character(character(&app, owner, "Bento", BENTO).id)
            .unwrap();
        assert_ne!(copy.name(), "Bento");
        assert_eq!(copy.details.appearance(), BENTO);
    }

    #[test]
    fn a_picture_drawn_on_the_world_screen_waits_for_review_and_is_paid_under_the_project() {
        let h = Harness::new();
        let app = h.start();
        let video = project(&app);
        let owner = owner(&app);
        let rosa = character(&app, owner, "Rosa", ROSA);
        let look = app
            .world_looks()
            .unwrap()
            .into_iter()
            .map(|(look, _)| look)
            .find(|look| look.channel() == Some(video.channel))
            .unwrap();
        let job = app
            .draw_world_picture(rosa.id, look.clone(), BudgetConsent::Ask)
            .unwrap();
        let finished = crate::scenes::tests::wait_done(&app, job);
        assert_eq!(finished.state(), JobState::Done, "{:?}", finished.failure());
        let card = app
            .world(owner, false)
            .unwrap()
            .into_iter()
            .find(|card| card.character.id == rosa.id)
            .unwrap();
        let waiting: Vec<&PictureView> = card
            .pictures
            .iter()
            .filter(|view| !view.picture.kept)
            .collect();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].picture.provenance.origin, PictureOrigin::World);
        assert!(
            kept_pictures(&app, rosa.id).is_empty(),
            "not used before review"
        );

        let records =
            h.db.recent_costs(app.profile().id, CostPurpose::WorldPicture, 5)
                .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].origin.project, owner);
        assert_eq!(records[0].project, None, "no video asked");

        app.keep_world_picture(rosa.id, waiting[0].picture.id)
            .unwrap();
        assert_eq!(kept_pictures(&app, rosa.id).len(), 1);
        assert!(matches!(
            app.keep_world_picture(rosa.id, waiting[0].picture.id),
            Err(WorldError::NothingWaiting)
        ));
    }

    #[test]
    fn a_cast_member_saved_to_the_world_keeps_its_portrait_for_the_next_video() {
        let h = Harness::new();
        let app = h.start();
        let first = h.narrated_project(&app);
        plan(&h, &app, &first);
        draw(&app, &first);
        assert_eq!(portraits_asked(&h), 1);

        let saved = app.save_cast_to_world(first.id, 0, true).unwrap();
        assert_eq!(saved.owner_project, None, "the global library");
        assert_eq!(saved.details.appearance(), "An old lady with a shawl");
        let kept = kept_pictures(&app, saved.id);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].provenance.origin, PictureOrigin::Saved);
        let plan_now = app.scenes(first.id).unwrap().plan.unwrap();
        assert_eq!(plan_now.cast()[0].character(), Some(saved.id));
        assert!(matches!(
            app.save_cast_to_world(first.id, 0, true),
            Err(WorldError::NameTaken)
        ));

        let second = video_on(&app, &first, "The second probe");
        narrate(&h, &app, &second);
        pick(&app, &second, &[(&saved, "")]);
        plan(&h, &app, &second);
        draw(&app, &second);
        assert_eq!(portraits_asked(&h), 1, "the saved portrait is reused");
    }

    #[test]
    fn a_name_with_accents_or_an_article_is_matched_without_failing() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let avo = character(&app, owner(&app), "Avó Aïcha", ROSA);
        pick(&app, &video, &[(&avo, "")]);
        h.answer(
            serde_json::json!({
                "cast": [{"name": "A avó Aïcha", "description": "An old lady"},
                         {"name": "Ébano", "description": "A black dog"}],
                "scenes": [
                    {"first_sentence": 0, "prompt": "Her", "cast": [0, 1], "motion": "Still"},
                    {"first_sentence": 2, "prompt": "Her again", "cast": [0, 1], "motion": "Still"},
                ],
            })
            .to_string(),
        );
        done(
            &app,
            app.plan_scenes(video.id, true, BudgetConsent::Ask).unwrap(),
        );
        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        assert_eq!(plan.cast().len(), 2);
        assert_eq!(plan.cast()[1].character(), None, "an extra");
    }

    #[test]
    fn a_new_drawing_replaces_the_one_waiting_and_a_stale_one_is_not_kept() {
        let h = Harness::new();
        let app = h.start();
        let video = project(&app);
        let owner = owner(&app);
        let rosa = character(&app, owner, "Rosa", ROSA);
        let look = app
            .world_looks()
            .unwrap()
            .into_iter()
            .map(|(look, _)| look)
            .find(|look| look.channel() == Some(video.channel))
            .unwrap();
        for _ in 0..2 {
            let job = app
                .draw_world_picture(rosa.id, look.clone(), BudgetConsent::Ask)
                .unwrap();
            done(&app, job);
        }
        let folder = app.world_book.folder(owner);
        assert_eq!(h.world_files.names(&folder).len(), 1, "no file left behind");
        let request = h.images.requests.lock().unwrap()[0].prompt.clone();
        assert!(request.contains("grainy 1960s film"), "{request}");

        // An appearance changed since: the waiting picture shows another face.
        let waiting = app.world_book.world.pictures(rosa.id).unwrap().remove(0);
        let mut draft = CharacterDraft::from(&rosa.details);
        draft.appearance = BENTO.into();
        app.edit_character(rosa.id, draft, true).unwrap();
        app.world_book.world.save_picture(&waiting).unwrap();
        assert!(matches!(
            app.keep_world_picture(rosa.id, waiting.id),
            Err(WorldError::NothingWaiting)
        ));
        assert!(kept_pictures(&app, rosa.id).is_empty());
    }
}
