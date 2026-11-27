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
    ApiKey, CastMember, CastRecord, ChannelRepository, CostPurpose, FolderPlaces, FolderRepository,
    GLOBAL_DIR, Generation, GenerationId, ImageGenerator, ImageRequest, Job, JobFailure,
    JobFailureKind, JobId, JobKind, Look, PictureOrigin, PictureProvenance, ProfileId,
    ProjectFiles, ProjectId, ProjectRepository, Provider, ReferencePicture, ReferencePictureId,
    RepositoryError, SceneImage, ScenePlan, ScenePrompt, SecretStore, StyleChoice, TokenUsage,
    VideoProject, VideoProjectId, WORLD_DIR, WorldFiles, WorldItem, WorldItemDetails,
    WorldItemDraft, WorldItemFieldError, WorldItemId, WorldItemUse, WorldKind, WorldPick,
    WorldRepository,
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
    /// Linked copies follow their originals' pictures (#340).
    pub(crate) sharing: Arc<dyn bardo_domain::SharingRepository>,
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
    drawing: Mutex<HashSet<(WorldItemId, Look)>>,
    free: Condvar,
}

impl PortraitDesk {
    /// Waits until nobody draws `character` in `look`, then holds it until
    /// the hold is dropped.
    pub(crate) fn hold(&self, character: WorldItemId, look: &Look) -> PortraitHold<'_> {
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
    key: (WorldItemId, Look),
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
        // A loose video's own notes (#341) stand in for a channel's.
        let notes = crate::loose::details_of(&*self.themes, &*self.channels, video)
            .ok()
            .flatten()
            .map(|details| details.aesthetic_notes().to_owned())
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
        let details = crate::loose::details_of(&*self.themes, &*self.channels, video)
            .ok()
            .flatten();
        video.style(details.map_or(StyleChoice::default(), |details| details.style()))
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
        character: WorldItemId,
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
            // A linked copy's file not copied yet: it comes from the
            // original's folder now, or the portrait is drawn (and kept by
            // the original); the copy's row is never let go.
            Err(_) if self.original_of(character).is_some() => {
                if self.follow_into(character) == crate::PictureFollowing::WaitingForFolder {
                    // Nothing paid is drawn again while the original's
                    // folder is away.
                    return Err(JobFailure::new(
                        JobFailureKind::Files,
                        "the original's world folder cannot be reached".to_owned(),
                    ));
                }
                match self.files.read(&folder, &picture.file) {
                    Ok(bytes) => bytes,
                    Err(_) => return Ok(None),
                }
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
                // A character's picture is always drawn.
                provider: picture.provenance.provider.unwrap_or(Provider::Gemini),
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

    /// The item's kept picture for `look` that still shows it as it reads.
    pub(crate) fn usable(
        &self,
        item: &WorldItem,
        look: &Look,
    ) -> Result<Option<ReferencePicture>, RepositoryError> {
        Ok(self
            .kept(item.id, look)?
            .filter(|picture| drawn_from(picture, item.details.appearance())))
    }

    /// The world places and objects that drawing `scenes` of `plan` in
    /// `look` shows (#338), each once, in the order they first appear.
    pub(crate) fn shown_items(&self, plan: &ScenePlan, scenes: &[usize]) -> Vec<WorldItemId> {
        let mut items: Vec<WorldItemId> = Vec::new();
        for &index in scenes {
            let Ok(scene) = plan.scene(index) else {
                continue;
            };
            if scene.is_composed() {
                continue;
            }
            let place = plan
                .scene_place(index)
                .and_then(|place| plan.place(place).ok())
                .and_then(|place| place.item());
            for item in place.into_iter().chain(scene.objects().iter().copied()) {
                if !items.contains(&item) {
                    items.push(item);
                }
            }
        }
        items
    }

    /// How many world pictures drawing `scenes` of `plan` in `look` pays
    /// for (#338): the places and objects they show with no picture for the
    /// look yet.
    pub(crate) fn missing_pictures(
        &self,
        plan: &ScenePlan,
        scenes: &[usize],
        look: &Look,
    ) -> usize {
        self.shown_items(plan, scenes)
            .into_iter()
            .filter(|item| {
                matches!(
                    self.world.character(*item),
                    Ok(Some(found)) if self.usable(&found, look).ok().flatten().is_none()
                )
            })
            .count()
    }

    /// Keeps a picture a video drew for a place or an object as its
    /// picture for `look`, unless one is kept there meanwhile.
    pub(crate) fn keep_drawn(
        &self,
        item: &WorldItem,
        look: Look,
        bytes: &[u8],
        extension: &str,
        provenance: PictureProvenance,
    ) -> Result<Option<ReferencePicture>, RepositoryError> {
        // A linked copy is read-only: its original keeps the picture, which
        // then comes back to the copy (#340).
        if let Some(original) = self.original_of(item.id)
            && let Some(original) = self.world.character(original)?
        {
            return self.keep_drawn(&original, look, bytes, extension, provenance);
        }
        if self.usable(item, &look)?.is_some() {
            return Ok(None);
        }
        let id = ReferencePictureId::new();
        let file = format!("world-{id}.{extension}");
        let folder = self.folder(item.owner_project);
        if let Err(error) = self.files.write(&folder, &file, bytes) {
            tracing::warn!("could not keep a picture in the world: {error}");
            return Ok(None);
        }
        let picture = ReferencePicture {
            id,
            item: item.id,
            look,
            file,
            provenance,
            kept: true,
        };
        self.save_replacing(item.owner_project, &picture)?;
        Ok(Some(picture))
    }

    /// Keeps a portrait a video drew (or the user saved) for the
    /// character as its picture for `look`, unless it has one there.
    pub(crate) fn keep_portrait(
        &self,
        (character, appearance): (WorldItemId, &str),
        look: Look,
        bytes: &[u8],
        drawn: &Generation,
        origin: PictureOrigin,
    ) -> Result<(), RepositoryError> {
        // A linked copy is read-only: its original keeps the portrait.
        let character = self.original_of(character).unwrap_or(character);
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
                item: character,
                look,
                file,
                provenance: PictureProvenance {
                    provider: Some(drawn.provider),
                    model: drawn.model.clone(),
                    prompt: drawn.prompt.clone(),
                    generated_at: drawn.generated_at,
                    job: drawn.job,
                    origin,
                    confirmed_at: None,
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
            .pictures(picture.item)?
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
        if picture.kept {
            // Its linked copies got the row in the same write; the file
            // follows now.
            self.follow_pictures(picture.item);
        }
        Ok(())
    }

    /// The original a linked copy follows, when `item` is one.
    pub(crate) fn original_of(&self, item: WorldItemId) -> Option<WorldItemId> {
        match self.sharing.sharing(bardo_domain::LibraryItem::World(item)) {
            Ok(sharing) => match sharing.following.and_then(|following| following.original) {
                Some(bardo_domain::LibraryItem::World(original)) => Some(original),
                _ => None,
            },
            Err(error) => {
                tracing::warn!("could not read a world item's sharing: {error}");
                None
            }
        }
    }

    /// Copies the kept pictures' files of `from` that `to` (a copy of it)
    /// lacks into `to`'s folder. `Updating` while one could not be copied.
    fn fill(&self, from: &WorldItem, to: &WorldItem) -> crate::PictureFollowing {
        let source = self.folder(from.owner_project);
        let target = self.folder(to.owner_project);
        if !self.files.reachable(&source) || !self.files.reachable(&target) {
            return crate::PictureFollowing::WaitingForFolder;
        }
        let Ok(pictures) = self.world.pictures(to.id) else {
            return crate::PictureFollowing::Updating;
        };
        let mut state = crate::PictureFollowing::Current;
        for picture in pictures.iter().filter(|picture| picture.kept) {
            if self.files.read(&target, &picture.file).is_ok() {
                continue;
            }
            let copied = self
                .files
                .read(&source, &picture.file)
                .and_then(|bytes| self.files.write(&target, &picture.file, &bytes));
            if let Err(error) = copied {
                tracing::warn!("a followed world picture was not copied yet: {error}");
                state = crate::PictureFollowing::Updating;
            }
        }
        state
    }

    /// Brings a linked copy's picture files over from its original's
    /// folder, and says whether they all arrived.
    pub(crate) fn follow_into(&self, copy: WorldItemId) -> crate::PictureFollowing {
        let Some(original) = self.original_of(copy) else {
            return crate::PictureFollowing::Current;
        };
        match (
            self.world.character(original).ok().flatten(),
            self.world.character(copy).ok().flatten(),
        ) {
            (Some(from), Some(to)) => self.fill(&from, &to),
            _ => crate::PictureFollowing::Current,
        }
    }

    /// Copies the pictures of `from` into its copy `to`, right after it
    /// was made.
    pub(crate) fn copy_pictures(&self, from: WorldItemId, to: WorldItemId) {
        if let (Ok(Some(from)), Ok(Some(to))) =
            (self.world.character(from), self.world.character(to))
            && self.fill(&from, &to) != crate::PictureFollowing::Current
        {
            tracing::warn!("a copied world item's pictures wait for their folders");
        }
    }

    /// Sends the item's new pictures to every linked copy of it; `false`
    /// when some could not be copied yet.
    pub(crate) fn follow_pictures(&self, original: WorldItemId) -> bool {
        let Ok(copies) = self
            .sharing
            .linked_copies(bardo_domain::LibraryItem::World(original))
        else {
            return false;
        };
        let mut all = true;
        for copy in copies {
            if let bardo_domain::LibraryItem::World(copy) = copy {
                all &= self.follow_into(copy) == crate::PictureFollowing::Current;
            }
        }
        all
    }

    /// Copies the item's kept pictures' files from `from`'s world folder to
    /// `to`'s, before the item moves there. Fails when one cannot be.
    pub(crate) fn move_pictures(
        &self,
        item: WorldItemId,
        from: Option<ProjectId>,
        to: Option<ProjectId>,
    ) -> Result<(), ()> {
        let (source, target) = (self.folder(from), self.folder(to));
        let pictures = self.world.pictures(item).map_err(|_| ())?;
        if pictures.is_empty() {
            return Ok(());
        }
        if !self.files.reachable(&source) || !self.files.reachable(&target) {
            return Err(());
        }
        for picture in &pictures {
            let bytes = match self.files.read(&source, &picture.file) {
                Ok(bytes) => bytes,
                // Gone already: the next use draws it again.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(()),
            };
            self.files
                .write(&target, &picture.file, &bytes)
                .map_err(|_| ())?;
        }
        Ok(())
    }

    /// Removes the item's picture files from `owner`'s folder, once they
    /// moved elsewhere.
    pub(crate) fn forget_files(&self, item: WorldItemId, owner: Option<ProjectId>) {
        let folder = self.folder(owner);
        for picture in self.world.pictures(item).unwrap_or_default() {
            let _ = self.files.remove(&folder, &picture.file);
        }
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

/// The prompt `item`'s reference picture is drawn from in `look`: a
/// character's portrait, a place's or an object's picture, with a look
/// from a channel's notes adding the notes. `None` for an appearance past
/// what a cast member holds.
pub(crate) fn picture_in_look(item: &WorldItem, look: &Look) -> Option<String> {
    let prompt = match item.kind {
        WorldKind::Character => return as_cast(item).map(|cast| portrait_in_look(&cast, look)),
        WorldKind::Place | WorldKind::Object => item.picture_prompt(look.style()),
    };
    Some(match look.notes() {
        Some(notes) if !notes.trim().is_empty() => {
            format!(
                "{prompt}\n\nDraw it in the look of the video, from its aesthetic notes: {notes}"
            )
        }
        _ => prompt,
    })
}

/// Whether `picture` shows the item as `appearance` reads: a drawing's
/// prompt holds it, and an imported picture shows it as it is (a new
/// appearance drops it).
fn drawn_from(picture: &ReferencePicture, appearance: &str) -> bool {
    // A duplicate's copy of an imported picture is `Copied` but keeps the
    // confirmation, which only an import has.
    picture.provenance.origin == PictureOrigin::Imported
        || picture.provenance.confirmed_at.is_some()
        || picture.provenance.prompt.contains(appearance)
}

/// The name of a world picture's file, keeping the drawing's extension.
fn picture_file(id: ReferencePictureId, drawn: &str) -> String {
    let extension = std::path::Path::new(drawn)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| extension.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("png");
    format!("world-{id}.{extension}")
}

/// A character as a cast member, for its portrait's prompt.
fn as_cast(character: &WorldItem) -> Option<CastMember> {
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
    /// A linked copy is read-only (#340).
    #[error("a linked copy")]
    Linked,
    #[error("field: {0:?}")]
    Field(WorldItemFieldError),
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
    /// The user did not confirm they may use an imported picture.
    #[error("not confirmed")]
    NotConfirmed,
    /// The imported file will not read.
    #[error("unreadable")]
    ImportUnreadable,
    /// The imported file is not a picture Bardo takes.
    #[error("import: {0:?}")]
    Import(bardo_domain::ProductImageError),
    /// More of a kind than a video keeps.
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
            WorldError::Linked => Text::SharingLinkedReadOnly,
            WorldError::Field(error) => Text::WorldItemFieldError(*error),
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
            WorldError::NotConfirmed => Text::WorldImportNotConfirmed,
            WorldError::ImportUnreadable => Text::WorldImportUnreadable,
            WorldError::Import(error) => Text::ProductImageReason(*error),
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
pub struct WorldItemCard {
    pub character: WorldItem,
    /// Kept pictures, then the ones waiting for review.
    pub pictures: Vec<PictureView>,
    /// The videos that picked it or show it.
    pub uses: Vec<WorldItemUse>,
    /// A picture of it is being drawn.
    pub drawing: bool,
}

/// What a video's *Who appears* shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WhoAppears {
    /// The characters, places and objects picked, in order, with their
    /// instruction.
    pub picked: Vec<(WorldItem, String)>,
    /// The active items of the video's project, then the global
    /// library's, that may be picked.
    pub available: Vec<WorldItem>,
    /// The plan was made before the last change.
    pub changed_after_plan: bool,
}

impl WhoAppears {
    /// The picked items of `kind`, in order.
    pub fn picked_of(&self, kind: WorldKind) -> Vec<&(WorldItem, String)> {
        self.picked
            .iter()
            .filter(|(item, _)| item.kind == kind)
            .collect()
    }

    /// The items of `kind` that may be picked and are not.
    pub fn unpicked_of(&self, kind: WorldKind) -> Vec<&WorldItem> {
        self.available
            .iter()
            .filter(|item| item.kind == kind && !self.is_picked(item.id))
            .collect()
    }

    pub fn is_picked(&self, id: WorldItemId) -> bool {
        self.picked.iter().any(|(item, _)| item.id == id)
    }

    /// The belongings of the picked characters that are not picked: what
    /// picking a character offers (#338), never picked by itself.
    pub fn offered_belongings(&self) -> Vec<&WorldItem> {
        self.available
            .iter()
            .filter(|item| {
                item.belongs_to.is_some_and(|owner| self.is_picked(owner))
                    && !self.is_picked(item.id)
            })
            .collect()
    }
}

/// What the scene planner gets of *Who appears* (#337, #338): the picked
/// characters, places and objects, each with the video's instruction.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct FixedWorld {
    pub(crate) cast: Vec<(WorldItem, String)>,
    pub(crate) places: Vec<(WorldItem, String)>,
    pub(crate) objects: Vec<(WorldItem, String)>,
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
        let character: WorldItemId = id(&payload.character)?;
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
            shape: found.kind.picture_shape(),
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
        let file = format!("world-{id}.{}", image.format.extension());
        self.book
            .files
            .write(&folder, &file, &image.bytes)
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))?;
        self.book
            .save_replacing(
                found.owner_project,
                &ReferencePicture {
                    id,
                    item: character,
                    look,
                    file,
                    provenance: PictureProvenance {
                        provider: Some(Provider::Gemini),
                        model: image.model,
                        prompt: payload.prompt,
                        generated_at: SystemTime::now(),
                        job: Some(job),
                        origin: PictureOrigin::World,
                        confirmed_at: None,
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
    ) -> Result<Vec<WorldItemCard>, WorldError> {
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
                Ok(WorldItemCard {
                    uses: self.world_book.world.character_uses(character.id)?,
                    drawing: drawing.contains(&character.id.to_string()),
                    character,
                    pictures,
                })
            })
            .collect()
    }

    /// The character, when it is the profile's.
    /// The item, refused when it is a linked copy (#340): what an edit of
    /// it or its pictures needs.
    fn item_to_change(&self, id: WorldItemId) -> Result<WorldItem, WorldError> {
        let found = self.own_item(id)?;
        if self.is_linked_copy(bardo_domain::LibraryItem::World(id)) {
            return Err(WorldError::Linked);
        }
        Ok(found)
    }

    fn own_item(&self, id: WorldItemId) -> Result<WorldItem, WorldError> {
        self.world_book
            .world
            .character(id)?
            .filter(|character| character.owner == self.profile.id)
            .ok_or(WorldError::NotFound)
    }

    /// Fails when another item of `owner` and `kind` has the name.
    fn name_free(
        &self,
        (owner, kind): (Option<ProjectId>, WorldKind),
        name: &str,
        except: Option<WorldItemId>,
    ) -> Result<(), WorldError> {
        let taken = self
            .world_book
            .world
            .characters(self.profile.id, owner)?
            .into_iter()
            .any(|other| {
                Some(other.id) != except && other.kind == kind && other.details.same_name(name)
            });
        match taken {
            true => Err(WorldError::NameTaken),
            false => Ok(()),
        }
    }

    /// Adds a character, place or object to `owner`'s world (`None`: the
    /// global library).
    pub fn create_world_item(
        &self,
        owner: Option<ProjectId>,
        kind: WorldKind,
        draft: WorldItemDraft,
    ) -> Result<WorldItem, WorldError> {
        if let Some(project) = owner {
            self.projects
                .project(project)?
                .filter(|found| found.owner == self.profile.id)
                .ok_or(WorldError::NotFound)?;
        }
        let details = WorldItemDetails::validate(draft).map_err(WorldError::Field)?;
        self.name_free((owner, kind), details.name(), None)?;
        let character = WorldItem::new(self.profile.id, owner, kind, details, SystemTime::now());
        self.world_book.world.save_character(&character)?;
        Ok(character)
    }

    /// Saves the character's new fields. A new appearance drops its
    /// pictures in every look and the portraits of the videos that show
    /// it: without `confirmed`, it says which first
    /// (`WorldError::DropsPictures`). Its name and notes keep them.
    pub fn edit_world_item(
        &self,
        id: WorldItemId,
        draft: WorldItemDraft,
        confirmed: bool,
    ) -> Result<WorldItem, WorldError> {
        let mut character = self.item_to_change(id)?;
        let details = WorldItemDetails::validate(draft).map_err(WorldError::Field)?;
        self.name_free(
            (character.owner_project, character.kind),
            details.name(),
            Some(id),
        )?;
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
    fn follow_new_look(&self, character: &WorldItem) -> Result<(), WorldError> {
        let appearance = ScenePrompt::new(character.details.appearance())
            .map_err(|_| WorldError::Field(WorldItemFieldError::AppearanceTooLong))?;
        for used in self.world_book.world.character_uses(character.id)? {
            if used.archived {
                continue;
            }
            let Some(mut plan) = self.scene_plans.scene_plan(used.video)? else {
                continue;
            };
            // A world place's description is the plan's place's (#338).
            let places: Vec<usize> = plan
                .places()
                .iter()
                .enumerate()
                .filter(|(_, place)| place.item() == Some(character.id))
                .map(|(index, _)| index)
                .collect();
            for place in places {
                if let Ok(linked) = plan.place_mut(place, SystemTime::now())
                    && linked.edit_description(appearance.clone())
                {
                    self.scene_plans.save_place(&plan, place)?;
                }
            }
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
    pub fn duplicate_world_item(&self, id: WorldItemId) -> Result<WorldItem, WorldError> {
        let original = self.own_item(id)?;
        let others = self
            .world_book
            .world
            .characters(self.profile.id, original.owner_project)?;
        let name = (2..)
            .map(|n| {
                let suffix = format!(" ({n})");
                let room = WorldItemDetails::MAX_NAME_CHARS - suffix.chars().count();
                let base: String = original.name().chars().take(room).collect();
                format!("{}{suffix}", base.trim_end())
            })
            .find(|name| {
                !others
                    .iter()
                    .any(|other| other.kind == original.kind && other.details.same_name(name))
            })
            .unwrap_or_default();
        let mut draft = WorldItemDraft::from(&original.details);
        draft.name = name;
        let details = WorldItemDetails::validate(draft).map_err(WorldError::Field)?;
        let mut copy = WorldItem::new(
            self.profile.id,
            original.owner_project,
            original.kind,
            details,
            SystemTime::now(),
        );
        copy.belongs_to = original.belongs_to;
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
                item: copy.id,
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
    pub fn archive_world_item(&self, id: WorldItemId, archived: bool) -> Result<(), WorldError> {
        let mut character = self.own_item(id)?;
        character.archived_at = archived.then(SystemTime::now);
        Ok(self.world_book.world.save_character(&character)?)
    }

    /// Deletes the character with its pictures. Refused while a video that
    /// is not archived picked it or shows it; archived videos keep their
    /// cast member as their own.
    pub fn delete_world_item(&self, id: WorldItemId) -> Result<(), WorldError> {
        let character = self.own_item(id)?;
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
        // Its linked copies become independent: they keep its pictures,
        // so the files reach them before they go.
        if !self.world_book.follow_pictures(id) {
            return Err(WorldError::FolderNotFound);
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
                    Some(channel.id),
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
        id: WorldItemId,
        look: Look,
        consent: BudgetConsent,
    ) -> Result<JobId, WorldError> {
        let character = self.item_to_change(id)?;
        let busy = self.jobs().into_iter().any(|job| {
            job.kind() == JobKind::WorldPicture
                && job.state().is_active()
                && serde_json::from_str::<PicturePayload>(job.payload())
                    .is_ok_and(|payload| payload.character == id.to_string())
        });
        if busy {
            return Err(WorldError::Busy);
        }
        // The job runs for the look's channel's project, else the item's
        // (as the job store files it): that project's own key pays.
        let work = look
            .channel()
            .and_then(|channel| self.project_of_channel(channel))
            .or(character.owner_project);
        if self.key_state_for(work, Provider::Gemini) == crate::KeyState::NotSet {
            return Err(WorldError::MissingKey);
        }
        let folder = self.world_book.folder(character.owner_project);
        if !self.world_book.files.reachable(&folder) {
            return Err(WorldError::FolderNotFound);
        }
        if let Err(estimate) = self.check_budget_for(work, &[picture_call()], consent)? {
            return Err(WorldError::OverBudget(estimate));
        }
        let prompt = picture_in_look(&character, &look)
            .ok_or(WorldError::Field(WorldItemFieldError::AppearanceTooLong))?;
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
        character: WorldItemId,
        picture: ReferencePictureId,
    ) -> Result<(), WorldError> {
        let found = self.item_to_change(character)?;
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
        character: WorldItemId,
        picture: ReferencePictureId,
    ) -> Result<(), WorldError> {
        let found = self.item_to_change(character)?;
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
        // A loose video (#341) belongs to none: global items only.
        match video.channel {
            Some(channel) => Ok(self.channels.get(channel)?.map(|channel| channel.project)),
            None => Ok(None),
        }
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
            if let Some(character) = available.iter().find(|c| c.id == pick.item) {
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

    /// Sets who and what appears in the video. Each item is the video's
    /// project's or global, at most [`WorldKind::max_picked`] of a kind,
    /// each instruction within its length. A change after planning marks the plan for review.
    pub fn set_who_appears(
        &self,
        video: VideoProjectId,
        picks: Vec<WorldPick>,
    ) -> Result<(), WorldError> {
        let found = self
            .themes
            .project(video)?
            .filter(|found| found.owner == self.profile.id)
            .ok_or(WorldError::NotFound)?;
        let owner = self.video_owner(&found)?;
        let mut picks: Vec<WorldPick> = picks
            .into_iter()
            .map(|pick| WorldPick {
                instruction: pick.instruction.trim().to_owned(),
                ..pick
            })
            .collect();
        let mut seen: Vec<WorldItemId> = Vec::new();
        picks.retain(|pick| {
            let first = !seen.contains(&pick.item);
            seen.push(pick.item);
            first
        });
        let before = self.world_book.world.picks(video)?;
        let mut kinds: Vec<WorldKind> = Vec::new();
        for pick in &picks {
            if pick.instruction.chars().count() > WorldPick::MAX_INSTRUCTION_CHARS {
                return Err(WorldError::InstructionTooLong);
            }
            let character = self.own_item(pick.item)?;
            if character.owner_project.is_some() && character.owner_project != owner {
                return Err(WorldError::OtherProject);
            }
            // An archived item stays where it was picked, and is never
            // picked anew.
            if character.is_archived() && !before.iter().any(|old| old.item == pick.item) {
                return Err(WorldError::NotFound);
            }
            kinds.push(character.kind);
        }
        if WorldKind::ALL
            .into_iter()
            .any(|kind| kinds.iter().filter(|picked| **picked == kind).count() > kind.max_picked())
        {
            return Err(WorldError::TooManyPicked);
        }
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
    ) -> Result<WorldItem, WorldError> {
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
        let character = self.create_world_item(
            owner,
            WorldKind::Character,
            WorldItemDraft {
                name: if name.is_empty() {
                    cast.name.clone()
                } else {
                    name
                },
                appearance: cast.description().as_str().to_owned(),
                ..WorldItemDraft::default()
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
        self.pick_saved(video, &character)?;
        Ok(character)
    }

    /// Picks an item just saved from the video's plan, while its kind has
    /// room: planning again keeps it. The plan already has it.
    fn pick_saved(&self, video: VideoProjectId, item: &WorldItem) -> Result<(), WorldError> {
        let mut picks = self.world_book.world.picks(video)?;
        let mut same = 0;
        for pick in &picks {
            if self
                .world_book
                .world
                .character(pick.item)?
                .is_some_and(|other| other.kind == item.kind)
            {
                same += 1;
            }
        }
        if same < item.kind.max_picked() {
            picks.push(WorldPick {
                item: item.id,
                instruction: String::new(),
            });
            self.world_book.world.set_picks(video, &picks, false)?;
        }
        Ok(())
    }

    /// Saves place `place` of the video's plan to the world of the video's
    /// project, or to the global library's with `global`, and makes the
    /// plan's place that world place (#338). A plan's place has no picture
    /// of its own: the place's first picture is drawn when a video needs
    /// it.
    pub fn save_place_to_world(
        &self,
        video: VideoProjectId,
        place: usize,
        global: bool,
    ) -> Result<WorldItem, WorldError> {
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
        let saved = plan.place(place).map_err(|_| WorldError::NotFound)?.clone();
        if saved.item().is_some() {
            return Err(WorldError::NameTaken);
        }
        if self.scenes_idle(video).is_err() {
            return Err(WorldError::VideoBusy);
        }
        let item = self.create_world_item(
            owner,
            WorldKind::Place,
            WorldItemDraft {
                name: saved.name.clone(),
                appearance: saved.description().as_str().to_owned(),
                ..WorldItemDraft::default()
            },
        )?;
        if let Ok(linked) = plan.place_mut(place, SystemTime::now()) {
            *linked = linked.clone().with_item(Some(item.id));
            self.scene_plans.save_place(&plan, place)?;
        }
        self.pick_saved(video, &item)?;
        Ok(item)
    }

    /// Makes `object` belong to `character`, or to nobody (#338). The
    /// character is of the object's owner or of the global library; a
    /// global object belongs only to a global character.
    pub fn set_belongs_to(
        &self,
        object: WorldItemId,
        character: Option<WorldItemId>,
    ) -> Result<WorldItem, WorldError> {
        let mut found = self.item_to_change(object)?;
        if found.kind != WorldKind::Object {
            return Err(WorldError::NotFound);
        }
        if let Some(character) = character {
            let owner = self.own_item(character)?;
            if owner.kind != WorldKind::Character {
                return Err(WorldError::NotFound);
            }
            if owner.owner_project.is_some() && owner.owner_project != found.owner_project {
                return Err(WorldError::OtherProject);
            }
        }
        found.belongs_to = character;
        self.world_book.world.save_character(&found)?;
        Ok(found)
    }

    /// The objects that belong to `character`, by name.
    pub fn belongings(&self, character: WorldItemId) -> Result<Vec<WorldItem>, WorldError> {
        let found = self.own_item(character)?;
        let mut owners = vec![found.owner_project];
        if found.owner_project.is_none() {
            // A global character's belongings may be any project's.
            owners.extend(
                self.projects
                    .projects(self.profile.id)?
                    .into_iter()
                    .map(|p| Some(p.id)),
            );
        }
        let mut belongings = Vec::new();
        for owner in owners {
            belongings.extend(
                self.world_book
                    .world
                    .characters(self.profile.id, owner)?
                    .into_iter()
                    .filter(|item| item.belongs_to == Some(character)),
            );
        }
        Ok(belongings)
    }

    /// Imports the picture at `path` as `item`'s picture for `look`, kept at
    /// once (#338): only a place's or an object's, a PNG, JPEG or WebP
    /// within the limits of a product image, once the user `confirmed`
    /// they may use it. It replaces the kept picture of that look.
    pub fn import_world_picture(
        &self,
        item: WorldItemId,
        look: Look,
        path: &std::path::Path,
        confirmed: bool,
    ) -> Result<ReferencePicture, WorldError> {
        let found = self.item_to_change(item)?;
        if !found.kind.imports() {
            return Err(WorldError::NotFound);
        }
        if !confirmed {
            return Err(WorldError::NotConfirmed);
        }
        let bytes = crate::products::read_picture(path).map_err(|error| match error {
            crate::ProductError::Image(reason) => WorldError::Import(reason),
            _ => WorldError::ImportUnreadable,
        })?;
        let picture = bardo_domain::ProductPicture::image(bytes).map_err(WorldError::Import)?;
        let folder = self.world_book.folder(found.owner_project);
        if !self.world_book.files.reachable(&folder) {
            return Err(WorldError::FolderNotFound);
        }
        let id = ReferencePictureId::new();
        let file = format!("world-{id}.{}", picture.format().extension());
        self.world_book
            .files
            .write(&folder, &file, picture.bytes())
            .map_err(|_| WorldError::FolderNotFound)?;
        let now = crate::publications::whole_millis(SystemTime::now());
        let saved = ReferencePicture {
            id,
            item,
            look,
            file,
            provenance: PictureProvenance {
                provider: None,
                model: String::new(),
                prompt: String::new(),
                generated_at: now,
                job: None,
                origin: PictureOrigin::Imported,
                confirmed_at: Some(now),
            },
            kept: true,
        };
        self.world_book
            .save_replacing(found.owner_project, &saved)?;
        tracing::info!("imported a world picture");
        Ok(saved)
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

    /// What the video picked, with the instructions, as the scene
    /// planner gets it: an item gone, or of another project, is left out.
    pub(crate) fn fixed_world(&self, video: &VideoProject) -> Result<FixedWorld, RepositoryError> {
        let owner = self.video_owner(video)?;
        let mut fixed = FixedWorld::default();
        for pick in self.world_book.world.picks(video.id)? {
            if let Some(item) = self.world_book.world.character(pick.item)?
                && (item.owner_project.is_none() || item.owner_project == owner)
            {
                match item.kind {
                    WorldKind::Character => fixed.cast.push((item, pick.instruction)),
                    WorldKind::Place => fixed.places.push((item, pick.instruction)),
                    WorldKind::Object => fixed.objects.push((item, pick.instruction)),
                }
            }
        }
        Ok(fixed)
    }
}

/// What the scene planner is told of the fixed cast: one numbered entry
/// per picked character, with the picked objects it owns, or that none was
/// picked.
pub(crate) fn fixed_cast_lines(world: &FixedWorld) -> String {
    let fixed = &world.cast;
    if fixed.is_empty() {
        return "None: no character was picked for this video.".to_owned();
    }
    let mut text = String::from(
        "Put each of these characters in the cast under this exact name and with this \
         exact appearance, and show them in the scenes their notes and the story call for. \
         Add other people to the cast only for someone none of them can be.",
    );
    for (number, (character, instruction)) in fixed.iter().enumerate() {
        let belongings: Vec<&str> = world
            .objects
            .iter()
            .filter(|(object, _)| object.belongs_to == Some(character.id))
            .map(|(object, _)| object.name())
            .collect();
        text.push_str(&format!(
            "\n{}. {}",
            number + 1,
            character.planner_lines_with(instruction, &belongings)
        ));
    }
    text
}

/// What the scene planner is told of the fixed places (#338): one numbered
/// entry per picked place, or that none was picked.
pub(crate) fn fixed_places_lines(world: &FixedWorld) -> String {
    if world.places.is_empty() {
        return "None: no place was picked for this video.".to_owned();
    }
    let mut text = String::from(
        "Put each of these places in the places under this exact name and with this exact \
         description, and set the scenes their notes and the story call for in them. Add \
         other places only for somewhere none of them can be.",
    );
    for (number, (place, instruction)) in world.places.iter().enumerate() {
        text.push_str(&format!(
            "\n{}. {}",
            number + 1,
            place.planner_lines(instruction)
        ));
    }
    text
}

/// What the scene planner is told of the fixed objects (#338): one
/// numbered entry per picked object, by the number a scene's `objects`
/// gives, or that none was picked.
pub(crate) fn fixed_objects_lines(world: &FixedWorld) -> String {
    if world.objects.is_empty() {
        return "None: no object was picked for this video.".to_owned();
    }
    let mut text = String::from(
        "Show these objects in the scenes their notes and the story call for, under this \
         exact name and as described, and list in each scene's `objects` the numbers of \
         those it shows. Never list an object a scene does not show.",
    );
    for (number, (object, instruction)) in world.objects.iter().enumerate() {
        let owner = object
            .belongs_to
            .and_then(|owner| {
                world
                    .cast
                    .iter()
                    .find(|(character, _)| character.id == owner)
            })
            .map(|(character, _)| format!("\n   Belongs to: {}", character.name()))
            .unwrap_or_default();
        text.push_str(&format!(
            "\n{}. {}{owner}",
            number + 1,
            object.planner_lines(instruction)
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

    fn character(app: &Bardo, owner: Option<ProjectId>, name: &str, look: &str) -> WorldItem {
        app.create_world_item(
            owner,
            WorldKind::Character,
            WorldItemDraft {
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
            video.channel.unwrap(),
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

    fn pick(app: &Bardo, video: &VideoProject, picked: &[(&WorldItem, &str)]) {
        app.set_who_appears(
            video.id,
            picked
                .iter()
                .map(|(character, instruction)| WorldPick {
                    item: character.id,
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

    fn kept_pictures(app: &Bardo, character: WorldItemId) -> Vec<ReferencePicture> {
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
            .find(|channel| Some(channel.id) == second.channel)
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

    const BAKERY: &str = "A tiny corner bakery, blue tiles, wooden shelves of bread, morning light";
    const LAMP: &str = "A dented brass oil lamp with a green glass chimney";

    fn thing(app: &Bardo, kind: WorldKind, name: &str, look: &str) -> WorldItem {
        app.create_world_item(
            owner(app),
            kind,
            WorldItemDraft {
                name: name.into(),
                appearance: look.into(),
                ..WorldItemDraft::default()
            },
        )
        .unwrap()
    }

    /// The bakery in the first and last scenes, the lamp only in the first,
    /// and a place of the planner's own in the second.
    fn world_answer() -> String {
        serde_json::json!({
            "cast": [],
            "places": [
                {"name": "the Bakery", "description": "Some shop the planner imagined"},
                {"name": "The pier", "description": "A wooden pier at dusk"},
            ],
            "scenes": [
                {"first_sentence": 0, "prompt": "Dawn at the bakery, the lamp lit", "cast": [],
                 "place": 0, "objects": [1, 7], "motion": "Slow push in"},
                {"first_sentence": 1, "prompt": "The probe over the pier", "cast": [],
                 "place": 1, "motion": "Drift"},
                {"first_sentence": 2, "prompt": "Closing time at the bakery", "cast": [],
                 "place": 0, "motion": "Static"},
            ],
        })
        .to_string()
    }

    fn plan_world(h: &Harness, app: &Bardo, video: &VideoProject) {
        h.answer(world_answer());
        done(
            app,
            app.plan_scenes(video.id, true, BudgetConsent::Ask).unwrap(),
        );
    }

    fn scene_prompts(h: &Harness) -> Vec<String> {
        h.images
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request.shape, ImageShape::Frame(_)))
            .filter(|request| {
                request.prompt.contains("Dawn")
                    || request.prompt.contains("probe")
                    || request.prompt.contains("Closing")
            })
            .map(|request| request.prompt.clone())
            .collect()
    }

    #[test]
    fn picked_places_and_objects_reach_the_planner_and_are_drawn_once_per_look() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let rosa = character(&app, owner(&app), "Rosa", ROSA);
        let bakery = thing(&app, WorldKind::Place, "The bakery", BAKERY);
        let lamp = thing(&app, WorldKind::Object, "Brass lamp", LAMP);
        app.set_belongs_to(lamp.id, Some(rosa.id)).unwrap();
        let unpicked = thing(
            &app,
            WorldKind::Place,
            "The lighthouse",
            "A white lighthouse",
        );
        pick(
            &app,
            &video,
            &[
                (&rosa, ""),
                (&bakery, "Opens and closes the video"),
                (&lamp, ""),
            ],
        );

        plan_world(&h, &app, &video);
        let request = h.text.requests().pop().unwrap();
        let sent = format!("{}\n{}", request.instructions, request.prompt);
        assert!(
            sent.contains("<fixed_places>") && sent.contains(BAKERY),
            "{sent}"
        );
        assert!(sent.contains("Opens and closes the video"));
        assert!(sent.contains("<fixed_objects>") && sent.contains("1. Brass lamp"));
        assert!(sent.contains("Belongs to: Rosa"));
        assert!(
            sent.contains("Has: Brass lamp"),
            "the character's belongings"
        );
        assert!(!sent.contains("lighthouse"), "never an unpicked place");
        let _ = unpicked;

        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        assert_eq!(plan.places()[0].item(), Some(bakery.id));
        assert_eq!(plan.places()[0].name, "The bakery", "the world's name");
        assert_eq!(plan.places()[0].description().as_str(), BAKERY);
        assert_eq!(plan.places()[1].item(), None, "the planner's own place");
        assert_eq!(
            plan.scenes()[0].objects(),
            [lamp.id],
            "an unknown number is dropped"
        );
        assert!(plan.scenes()[2].objects().is_empty());

        let view = app.scenes(video.id).unwrap();
        assert_eq!(view.missing_world_pictures, 2, "the bakery and the lamp");
        assert_eq!(view.missing_images(), 3 + 2);
        draw(&app, &video);
        let world: Vec<ImageRequest> = h
            .images
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.prompt.starts_with("A reference picture of one"))
            .cloned()
            .collect();
        assert_eq!(world.len(), 2, "one picture each");
        assert!(world.iter().any(|request| request.shape
            == ImageShape::Frame(bardo_domain::AspectRatio::Landscape)
            && request.prompt.contains(BAKERY)));
        let prompts = scene_prompts(&h);
        let first = prompts.iter().find(|p| p.contains("Dawn")).unwrap();
        assert!(first.contains("this place itself"), "{first}");
        assert!(first.contains("Brass lamp, A dented brass"), "{first}");
        let last = prompts.iter().find(|p| p.contains("Closing")).unwrap();
        assert!(last.contains("this place itself"));
        assert!(!last.contains("Brass lamp"), "only scenes that show it");
        let pier = prompts.iter().find(|p| p.contains("probe")).unwrap();
        assert!(!pier.contains("this place itself"));
        for item in [bakery.id, lamp.id] {
            let kept = kept_pictures(&app, item);
            assert_eq!(kept.len(), 1);
            assert_eq!(kept[0].provenance.origin, PictureOrigin::Video);
        }

        // The next video in the look lends them.
        let second = video_on(&app, &video, "The second probe");
        narrate(&h, &app, &second);
        pick(&app, &second, &[(&bakery, ""), (&lamp, "")]);
        plan_world(&h, &app, &second);
        assert_eq!(app.scenes(second.id).unwrap().missing_world_pictures, 0);
        let before = h.images.requests.lock().unwrap().len();
        draw(&app, &second);
        let asked = h.images.requests.lock().unwrap()[before..]
            .iter()
            .filter(|request| request.prompt.starts_with("A reference picture of one"))
            .count();
        assert_eq!(asked, 0, "no paid call for them");
    }

    fn world_requests(h: &Harness) -> usize {
        h.images
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.prompt.starts_with("A reference picture of one"))
            .count()
    }

    #[test]
    fn a_world_picture_that_fails_is_tried_once_and_its_scenes_are_described() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let bakery = thing(&app, WorldKind::Place, "The bakery", BAKERY);
        pick(&app, &video, &[(&bakery, "")]);
        plan_world(&h, &app, &video);
        h.images.decline("A reference picture of one");

        // The job ends well: the scenes are drawn from the description.
        draw(&app, &video);
        assert_eq!(world_requests(&h), 1, "tried once for both scenes");
        let prompts = scene_prompts(&h);
        assert_eq!(prompts.len(), 3, "every scene is drawn");
        for prompt in prompts.iter().filter(|p| !p.contains("probe")) {
            assert!(!prompt.contains("this place itself"), "{prompt}");
            assert!(prompt.contains(BAKERY), "described: {prompt}");
        }
        assert!(kept_pictures(&app, bakery.id).is_empty());
    }

    #[test]
    fn a_world_picture_the_world_cannot_keep_is_paid_once_per_run() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let bakery = thing(&app, WorldKind::Place, "The bakery", BAKERY);
        pick(&app, &video, &[(&bakery, "")]);
        plan_world(&h, &app, &video);
        // Reachable, but full: the picture is drawn and cannot be kept.
        h.world_files
            .refuse_writes(&app.world_book.folder(owner(&app)));

        draw(&app, &video);
        assert_eq!(world_requests(&h), 1, "not paid again for the next scene");
        let prompts = scene_prompts(&h);
        let at_the_bakery: Vec<&String> = prompts.iter().filter(|p| !p.contains("probe")).collect();
        assert_eq!(at_the_bakery.len(), 2);
        assert!(
            at_the_bakery
                .iter()
                .all(|prompt| prompt.contains("this place itself")),
            "both scenes use the one drawing"
        );
        assert!(kept_pictures(&app, bakery.id).is_empty());
    }

    #[test]
    fn a_duplicated_imported_picture_stays_usable() {
        let h = Harness::new();
        let app = h.start();
        let bakery = thing(&app, WorldKind::Place, "The bakery", BAKERY);
        let look = Look::Style(bardo_domain::VisualStyle::Watercolor);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bakery.png");
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(1200u32.to_be_bytes());
        png.extend(800u32.to_be_bytes());
        png.extend([8, 2, 0, 0, 0, 1]);
        std::fs::write(&path, &png).unwrap();
        app.import_world_picture(bakery.id, look.clone(), &path, true)
            .unwrap();

        let copy = app.duplicate_world_item(bakery.id).unwrap();
        let found = app.world_book.world.character(copy.id).unwrap().unwrap();
        let usable = app.world_book.usable(&found, &look).unwrap();
        assert!(usable.is_some(), "the copy is the look's picture");
        assert_eq!(
            usable.unwrap().provenance.origin,
            PictureOrigin::Copied,
            "and says it was copied"
        );
    }

    #[test]
    fn an_edited_plan_place_stops_following_the_world_place() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let bakery = thing(&app, WorldKind::Place, "The bakery", BAKERY);
        pick(&app, &video, &[(&bakery, "")]);
        plan_world(&h, &app, &video);
        let plan = app
            .edit_place_description(video.id, 0, "A night market under paper lanterns")
            .unwrap();
        assert_eq!(plan.places()[0].item(), None, "the video's own now");

        // A new appearance of the world place leaves it as the user wrote it.
        app.edit_world_item(
            bakery.id,
            WorldItemDraft {
                name: "The bakery".into(),
                appearance: "A bakery painted blue".into(),
                ..WorldItemDraft::default()
            },
            true,
        )
        .unwrap();
        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        assert_eq!(
            plan.places()[0].description().as_str(),
            "A night market under paper lanterns"
        );
    }

    #[test]
    fn an_archived_item_is_never_picked_anew_but_stays_where_it_was() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let bakery = thing(&app, WorldKind::Place, "The bakery", BAKERY);
        let lamp = thing(&app, WorldKind::Object, "Brass lamp", LAMP);
        pick(&app, &video, &[(&bakery, "")]);
        app.archive_world_item(bakery.id, true).unwrap();
        app.archive_world_item(lamp.id, true).unwrap();
        // Saving the instructions again keeps the archived bakery.
        pick(&app, &video, &[(&bakery, "At dawn")]);
        assert!(matches!(
            app.set_who_appears(
                video.id,
                vec![WorldPick {
                    item: lamp.id,
                    instruction: String::new(),
                }],
            ),
            Err(WorldError::NotFound)
        ));
    }

    #[test]
    fn deleting_a_place_or_an_object_leaves_the_plan_its_own_and_its_uses_go() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let bakery = thing(&app, WorldKind::Place, "The bakery", BAKERY);
        let lamp = thing(&app, WorldKind::Object, "Brass lamp", LAMP);
        let globe = thing(&app, WorldKind::Object, "Desk globe", "An old desk globe");
        pick(&app, &video, &[(&bakery, ""), (&lamp, ""), (&globe, "")]);
        h.answer(
            serde_json::json!({
                "cast": [],
                "places": [{"name": "The bakery", "description": "Some shop"}],
                "scenes": [
                    {"first_sentence": 0, "prompt": "Dawn at the bakery", "cast": [],
                     "place": 0, "objects": [1, 2], "motion": "Slow push in"},
                    {"first_sentence": 1, "prompt": "The probe", "cast": [], "motion": "Drift"},
                    {"first_sentence": 2, "prompt": "Closing time", "cast": [],
                     "place": 0, "objects": [2], "motion": "Static"},
                ],
            })
            .to_string(),
        );
        done(
            &app,
            app.plan_scenes(video.id, true, BudgetConsent::Ask).unwrap(),
        );
        for item in [bakery.id, lamp.id, globe.id] {
            assert!(
                !app.world_book
                    .world
                    .character_uses(item)
                    .unwrap()
                    .is_empty(),
                "the plan uses it"
            );
        }

        app.world_book.world.delete_character(bakery.id).unwrap();
        app.world_book.world.delete_character(lamp.id).unwrap();
        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        assert_eq!(plan.places()[0].item(), None, "the video's own place");
        assert_eq!(plan.places()[0].description().as_str(), BAKERY);
        assert_eq!(plan.scenes()[0].objects(), [globe.id], "the other stays");
        assert_eq!(plan.scenes()[2].objects(), [globe.id]);
        assert!(
            !app.world_book
                .world
                .character_uses(globe.id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn belongings_are_offered_with_their_character_and_never_picked_alone() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let rosa = character(&app, owner(&app), "Rosa", ROSA);
        let lamp = thing(&app, WorldKind::Object, "Brass lamp", LAMP);
        let global_object = app
            .create_world_item(
                None,
                WorldKind::Object,
                WorldItemDraft {
                    name: "A globe".into(),
                    appearance: "An old desk globe".into(),
                    ..WorldItemDraft::default()
                },
            )
            .unwrap();
        assert!(
            matches!(
                app.set_belongs_to(global_object.id, Some(rosa.id)),
                Err(WorldError::OtherProject)
            ),
            "a global object belongs only to a global character"
        );
        assert!(matches!(
            app.set_belongs_to(rosa.id, Some(rosa.id)),
            Err(WorldError::NotFound)
        ));
        app.set_belongs_to(lamp.id, Some(rosa.id)).unwrap();
        let belongings: Vec<WorldItemId> = app
            .belongings(rosa.id)
            .unwrap()
            .iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(belongings, [lamp.id]);

        assert!(
            app.who_appears(video.id)
                .unwrap()
                .offered_belongings()
                .is_empty()
        );
        pick(&app, &video, &[(&rosa, "")]);
        let who = app.who_appears(video.id).unwrap();
        assert_eq!(
            who.offered_belongings()
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            [lamp.id]
        );
        assert!(!who.is_picked(lamp.id), "only when picked");
        plan(&h, &app, &video);
        let request = h.text.requests().pop().unwrap();
        assert!(!request.prompt.contains("Brass lamp"));
    }

    #[test]
    fn picks_are_counted_per_kind() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        let places: Vec<WorldItem> = (0..=bardo_domain::MAX_PICKED_PLACES)
            .map(|n| thing(&app, WorldKind::Place, &format!("Room {n}"), "A plain room"))
            .collect();
        let three: Vec<WorldItem> = ["Ana", "Bia", "Caio"]
            .iter()
            .map(|name| character(&app, owner(&app), name, ROSA))
            .collect();
        let mut picked: Vec<(&WorldItem, &str)> = three.iter().map(|c| (c, "")).collect();
        picked.extend(
            places[..bardo_domain::MAX_PICKED_PLACES]
                .iter()
                .map(|p| (p, "")),
        );
        pick(&app, &video, &picked);
        assert_eq!(
            app.who_appears(video.id).unwrap().picked.len(),
            3 + bardo_domain::MAX_PICKED_PLACES
        );
        let too_many: Vec<WorldPick> = places
            .iter()
            .map(|place| WorldPick {
                item: place.id,
                instruction: String::new(),
            })
            .collect();
        assert!(matches!(
            app.set_who_appears(video.id, too_many),
            Err(WorldError::TooManyPicked)
        ));
    }

    #[test]
    fn an_imported_picture_needs_a_confirmation_and_never_a_persons() {
        let h = Harness::new();
        let app = h.start();
        let rosa = character(&app, owner(&app), "Rosa", ROSA);
        let bakery = thing(&app, WorldKind::Place, "The bakery", BAKERY);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bakery.png");
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(1200u32.to_be_bytes());
        png.extend(800u32.to_be_bytes());
        png.extend([8, 2, 0, 0, 0, 1]);
        std::fs::write(&path, &png).unwrap();
        let look = Look::Style(bardo_domain::VisualStyle::Watercolor);
        assert!(matches!(
            app.import_world_picture(bakery.id, look.clone(), &path, false),
            Err(WorldError::NotConfirmed)
        ));
        assert!(matches!(
            app.import_world_picture(rosa.id, look.clone(), &path, true),
            Err(WorldError::NotFound)
        ));
        let small = dir.path().join("small.png");
        std::fs::write(&small, &png[..16]).unwrap();
        assert!(matches!(
            app.import_world_picture(bakery.id, look.clone(), &small, true),
            Err(WorldError::Import(_))
        ));
        let imported = app
            .import_world_picture(bakery.id, look.clone(), &path, true)
            .unwrap();
        assert_eq!(imported.provenance.origin, PictureOrigin::Imported);
        assert_eq!(imported.provenance.provider, None);
        assert!(imported.provenance.confirmed_at.is_some());
        assert_eq!(
            kept_pictures(&app, bakery.id),
            std::slice::from_ref(&imported)
        );
        let folder = app.world_book.folder(owner(&app));
        assert_eq!(h.world_files.read(&folder, &imported.file).unwrap(), png);
        // It is the look's picture: nothing is drawn for it.
        let found = app.world_book.world.character(bakery.id).unwrap().unwrap();
        assert!(app.world_book.usable(&found, &look).unwrap().is_some());
    }

    #[test]
    fn a_plans_place_saved_to_the_world_is_picked_and_followed() {
        let h = Harness::new();
        let app = h.start();
        let video = h.narrated_project(&app);
        plan_world(&h, &app, &video);
        let place = app.save_place_to_world(video.id, 1, false).unwrap();
        assert_eq!(place.kind, WorldKind::Place);
        assert_eq!(place.name(), "The pier");
        assert_eq!(place.details.appearance(), "A wooden pier at dusk");
        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        assert_eq!(plan.places()[1].item(), Some(place.id));
        assert!(app.who_appears(video.id).unwrap().is_picked(place.id));
        assert!(
            matches!(
                app.save_place_to_world(video.id, 1, true),
                Err(WorldError::NameTaken)
            ),
            "saved once"
        );
        // A new description reaches the plan's place.
        let draft = WorldItemDraft {
            appearance: "A stone pier at night".into(),
            ..WorldItemDraft::from(&place.details)
        };
        app.edit_world_item(place.id, draft, true).unwrap();
        let plan = app.scenes(video.id).unwrap().plan.unwrap();
        assert_eq!(
            plan.places()[1].description().as_str(),
            "A stone pier at night"
        );
        assert!(matches!(
            app.delete_world_item(place.id),
            Err(WorldError::InUse(_))
        ));
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
        let picked: Vec<WorldItem> = (0..4)
            .map(|n| character(&app, owner, &format!("Person {n}"), ROSA))
            .collect();
        let picks: Vec<WorldPick> = picked
            .iter()
            .map(|c| WorldPick {
                item: c.id,
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
                vec![WorldPick {
                    item: picked[0].id,
                    instruction: "x".repeat(WorldPick::MAX_INSTRUCTION_CHARS + 1),
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
                vec![WorldPick {
                    item: stranger.id,
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
        let mut draft = WorldItemDraft::from(&rosa.details);
        draft.name = "Dona Rosa".into();
        draft.never = "Never shouts".into();
        app.edit_world_item(rosa.id, draft.clone(), false).unwrap();
        assert_eq!(kept_pictures(&app, rosa.id).len(), 1);

        draft.appearance = "A woman in her seventies, short grey hair, red coat".into();
        match app.edit_world_item(rosa.id, draft.clone(), false) {
            Err(WorldError::DropsPictures { looks, videos }) => {
                assert_eq!(looks, std::slice::from_ref(&picture.look));
                assert_eq!(videos, std::slice::from_ref(&video.title));
            }
            other => panic!("asks first: {other:?}"),
        }
        assert_eq!(kept_pictures(&app, rosa.id).len(), 1, "nothing changed yet");

        app.edit_world_item(rosa.id, draft, true).unwrap();
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
        match app.delete_world_item(rosa.id) {
            Err(WorldError::InUse(titles)) => {
                assert_eq!(titles, std::slice::from_ref(&video.title))
            }
            other => panic!("refused: {other:?}"),
        }
        app.archive_world_item(rosa.id, true).unwrap();
        assert!(app.who_appears(video.id).unwrap().available.is_empty());
        assert_eq!(app.who_appears(video.id).unwrap().picked.len(), 1, "kept");

        app.archive_video(video.id).unwrap();
        app.delete_world_item(rosa.id).unwrap();
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
        let taken = app.create_world_item(
            owner,
            WorldKind::Character,
            WorldItemDraft {
                name: " rosa ".into(),
                appearance: BENTO.into(),
                ..WorldItemDraft::default()
            },
        );
        assert!(matches!(taken, Err(WorldError::NameTaken)));
        character(&app, None, "Rosa", ROSA);
        let copy = app
            .duplicate_world_item(character(&app, owner, "Bento", BENTO).id)
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
            .find(|look| look.channel() == video.channel)
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
            .find(|look| look.channel() == video.channel)
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
        let mut draft = WorldItemDraft::from(&rosa.details);
        draft.appearance = BENTO.into();
        app.edit_world_item(rosa.id, draft, true).unwrap();
        app.world_book.world.save_picture(&waiting).unwrap();
        assert!(matches!(
            app.keep_world_picture(rosa.id, waiting.id),
            Err(WorldError::NothingWaiting)
        ));
        assert!(kept_pictures(&app, rosa.id).is_empty());
    }
}
