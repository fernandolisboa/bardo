//! Sharing library items between projects (#340, spec "Library scope and
//! sharing", ADR-0047 item 5): one rule for characters, places, objects,
//! narrators, products and templates.
//!
//! - **Shareable.** A project's own item is shared only when the user marks
//!   it.
//! - **Import from another project** lists the other projects' shareable
//!   items (archived projects too) and copies one into the open project as
//!   a **linked copy** (read-only; it follows every change of its original,
//!   which the store writes in the same transaction) or an **independent
//!   copy**. A world item's pictures follow by a file copy right after.
//! - **Unlink** makes a linked copy independent; losing the original
//!   (deleted, unmarked, its project deleted) does too, and the copy says
//!   so once.
//! - **Export to another project** makes an independent copy there; **Make
//!   global** moves the item to the global library and turns its linked
//!   copies into references to it.
//! - **Item files** (`item_file`) save any item, and open one as an
//!   independent copy.

use std::path::Path;
use std::time::SystemTime;

use bardo_domain::{
    CopyKind, LibraryItem, LibraryKind, PersonaDetails, PersonaDraft, ProductBrief, ProductDraft,
    ProjectId, RepositoryError, ShareableItem, Sharing, TemplateKind, WorldItemDetails,
    WorldItemDraft, WorldItemId,
};

use crate::{Bardo, Text};

/// Why a sharing action was refused.
#[derive(Debug, thiserror::Error)]
pub enum SharingError {
    #[error("item not found")]
    NotFound,
    #[error("project not found")]
    ProjectNotFound,
    /// Only a project's own item, not linked, may be marked, exported or
    /// made global.
    #[error("not a project's own item")]
    NotOwnItem,
    /// A linked copy is read-only: unlink it first.
    #[error("the item is a linked copy")]
    Linked,
    /// Unlinking an item that follows nothing.
    #[error("the item is no linked copy")]
    NotLinked,
    /// Only items another project marked shareable are imported.
    #[error("the item is not shareable")]
    NotShareable,
    /// Copying an item into its own project.
    #[error("the item is in that project already")]
    SameProject,
    /// The owner has an item of the name: the user picks another, starting
    /// from the suggestion.
    #[error("the name is taken")]
    NameClash { suggestion: String },
    #[error("the name is not valid")]
    InvalidName,
    /// Making a template global changes the template these projects use;
    /// nothing was saved until the user confirms.
    #[error("projects use the global template")]
    UsedByProjects(Vec<String>),
    /// A world folder that cannot be reached holds the item's pictures.
    #[error("a world folder cannot be reached")]
    FolderUnreachable,
    /// The file holds product pictures: the user confirms they may use
    /// them.
    #[error("the pictures' use was not confirmed")]
    NotConfirmed,
    /// The file holds another kind of item than the screen shows.
    #[error("the file holds another kind of item")]
    WrongKind,
    #[error(transparent)]
    File(#[from] crate::item_file::ItemFileError),
    #[error("could not read the file: {0}")]
    Unreadable(String),
    #[error("could not write the file: {0}")]
    WriteFailed(String),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl SharingError {
    /// What the screens say.
    pub fn message(&self) -> Text {
        match self {
            SharingError::NotFound | SharingError::ProjectNotFound => Text::SharingNotFound,
            SharingError::NotOwnItem => Text::SharingNotOwn,
            SharingError::Linked => Text::SharingLinkedReadOnly,
            SharingError::NotLinked => Text::SharingNotFound,
            SharingError::NotShareable => Text::SharingNotShareable,
            SharingError::SameProject => Text::SharingSameProject,
            SharingError::NameClash { .. } => Text::SharingNameClash,
            SharingError::InvalidName => Text::SharingInvalidName,
            SharingError::UsedByProjects(_) => Text::SharingGlobalUsedBy,
            SharingError::FolderUnreachable => Text::SharingFolderUnreachable,
            SharingError::NotConfirmed => Text::SharingConfirmPictures,
            SharingError::WrongKind => Text::SharingWrongKind,
            SharingError::File(error) => error.message(),
            SharingError::Unreadable(_) => Text::SharingFileUnreadable,
            SharingError::WriteFailed(_) => Text::SharingFileNotWritten,
            SharingError::Repository(_) => Text::SharingNotSaved,
        }
    }
}

/// Whether a linked world item's pictures have arrived in its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureFollowing {
    /// Every picture is there (or the item has none).
    Current,
    /// Some are still being copied.
    Updating,
    /// Its folder, or its original's, cannot be reached.
    WaitingForFolder,
}

/// How an item takes part in sharing, as its screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSharing {
    pub sharing: Sharing,
    /// The item is a project's own (not global): it may be marked,
    /// exported and made global unless it is a linked copy.
    pub own: bool,
    /// For a linked world item, whether its pictures arrived.
    pub pictures: PictureFollowing,
}

/// What an item is, as sharing needs it.
pub(crate) struct ItemFacts {
    pub(crate) owner: Option<ProjectId>,
    pub(crate) name: String,
    pub(crate) kind: LibraryKind,
}

/// A copy's name for `name` from `source`: `<name> (<source>)`, then
/// `<name> (<source> 2)` and on until it is free, cut to `max` characters.
pub(crate) fn suggested_name(
    name: &str,
    source: &str,
    max: usize,
    taken: impl Fn(&str) -> bool,
) -> String {
    // A long source leaves room for at least half of the name.
    let source: String = source.chars().take(max / 2).collect();
    let source = source.trim_end();
    (1..)
        .map(|n| {
            let tag = if n == 1 {
                format!(" ({source})")
            } else {
                format!(" ({source} {n})")
            };
            let room = max.saturating_sub(tag.chars().count());
            let base: String = name.chars().take(room).collect();
            format!("{}{tag}", base.trim_end())
        })
        .find(|candidate| !taken(candidate))
        .expect("a free name")
}

/// The longest name an item of `kind` may have.
pub(crate) fn max_name_chars(kind: LibraryKind) -> usize {
    match kind {
        LibraryKind::Narrator => bardo_domain::PersonaDetails::MAX_NAME_CHARS,
        LibraryKind::Product => bardo_domain::ProductBrief::MAX_NAME_CHARS,
        LibraryKind::World(_) => bardo_domain::WorldItemDetails::MAX_NAME_CHARS,
        LibraryKind::Template => usize::MAX,
    }
}

impl Bardo {
    /// What sharing needs of the item; `NotFound` for another profile's.
    pub(crate) fn item_facts(&self, item: LibraryItem) -> Result<ItemFacts, SharingError> {
        Ok(match item {
            LibraryItem::Narrator(id) => {
                let found = self.own_persona(id).map_err(|_| SharingError::NotFound)?;
                ItemFacts {
                    owner: found.owner_project,
                    name: found.details.name().to_owned(),
                    kind: LibraryKind::Narrator,
                }
            }
            LibraryItem::Product(id) => {
                let found = self
                    .products
                    .get(id)?
                    .filter(|product| product.owner == self.profile.id)
                    .ok_or(SharingError::NotFound)?;
                ItemFacts {
                    owner: found.owner_project,
                    name: found.brief.name().to_owned(),
                    kind: LibraryKind::Product,
                }
            }
            LibraryItem::World(id) => {
                let found = self
                    .world_book
                    .world
                    .character(id)?
                    .filter(|item| item.owner == self.profile.id)
                    .ok_or(SharingError::NotFound)?;
                ItemFacts {
                    owner: found.owner_project,
                    name: found.details.name().to_owned(),
                    kind: LibraryKind::World(found.kind),
                }
            }
            LibraryItem::Template { project, kind } => {
                if let Some(project) = project {
                    if !self.own_project_exists(project)? {
                        return Err(SharingError::NotFound);
                    }
                    if self
                        .templates
                        .template_versions(self.profile.id, Some(project), kind)?
                        .is_empty()
                    {
                        return Err(SharingError::NotFound);
                    }
                }
                ItemFacts {
                    owner: project,
                    name: String::new(),
                    kind: LibraryKind::Template,
                }
            }
        })
    }

    /// Whether `owner` has an item of `kind` named `name`.
    fn name_taken(
        &self,
        owner: Option<ProjectId>,
        kind: LibraryKind,
        name: &str,
    ) -> Result<bool, SharingError> {
        let same = |other: &str| other.trim().to_lowercase() == name.trim().to_lowercase();
        Ok(match kind {
            LibraryKind::Narrator => self
                .personas
                .list(self.profile.id)?
                .iter()
                .any(|other| other.owner_project == owner && same(other.details.name())),
            LibraryKind::Product => self
                .products
                .list(self.profile.id)?
                .iter()
                .any(|other| other.owner_project == owner && same(other.brief.name())),
            LibraryKind::World(kind) => self
                .world_book
                .world
                .characters(self.profile.id, owner)?
                .iter()
                .any(|other| other.kind == kind && same(other.details.name())),
            LibraryKind::Template => false,
        })
    }

    /// The name a copy of `item` takes in `into`: `name`, else the
    /// original's, checked as the item's kind checks names and refused
    /// with a suggestion when `into` has it.
    fn copy_name(
        &self,
        item: LibraryItem,
        facts: &ItemFacts,
        into: Option<ProjectId>,
        name: Option<&str>,
    ) -> Result<String, SharingError> {
        if facts.kind == LibraryKind::Template {
            return Ok(String::new());
        }
        let name = name.map_or_else(|| facts.name.clone(), |name| name.trim().to_owned());
        let valid = match item {
            LibraryItem::Narrator(id) => {
                let found = self.own_persona(id).map_err(|_| SharingError::NotFound)?;
                PersonaDetails::validate(PersonaDraft {
                    name: name.clone(),
                    ..PersonaDraft::from(&found.details)
                })
                .map(|details| details.name().to_owned())
                .ok()
            }
            LibraryItem::Product(id) => {
                let found = self.products.get(id)?.ok_or(SharingError::NotFound)?;
                ProductBrief::validate(ProductDraft {
                    name: name.clone(),
                    ..ProductDraft::from(&found.brief)
                })
                .map(|brief| brief.name().to_owned())
                .ok()
            }
            LibraryItem::World(id) => {
                let found = self
                    .world_book
                    .world
                    .character(id)?
                    .ok_or(SharingError::NotFound)?;
                WorldItemDetails::validate(WorldItemDraft {
                    name: name.clone(),
                    ..WorldItemDraft::from(&found.details)
                })
                .map(|details| details.name().to_owned())
                .ok()
            }
            LibraryItem::Template { .. } => Some(String::new()),
        };
        let name = valid.ok_or(SharingError::InvalidName)?;
        if self.name_taken(into, facts.kind, &name)? {
            let source = match facts.owner {
                Some(project) => self.project_name(project),
                None => self.text(Text::LibraryGlobalSource).into_owned(),
            };
            let max = max_name_chars(facts.kind);
            let suggestion = suggested_name(&facts.name, &source, max, |candidate| {
                self.name_taken(into, facts.kind, candidate).unwrap_or(true)
            });
            return Err(SharingError::NameClash { suggestion });
        }
        Ok(name)
    }

    /// How the item takes part in sharing, with its pictures' state for a
    /// linked world item.
    pub fn item_sharing(&self, item: LibraryItem) -> Result<ItemSharing, SharingError> {
        let facts = self.item_facts(item)?;
        let sharing = self.world_book.sharing.sharing(item)?;
        let pictures = match item {
            LibraryItem::World(id) if sharing.is_linked() => self.world_book.follow_into(id),
            _ => PictureFollowing::Current,
        };
        Ok(ItemSharing {
            own: facts.owner.is_some(),
            sharing,
            pictures,
        })
    }

    /// The sharing of `project`'s items of `kind` that take part in it
    /// (shareable ones, linked copies, and copies whose original was lost),
    /// for the lists' tags.
    pub fn sharing_in(
        &self,
        project: Option<ProjectId>,
        kind: LibraryKind,
    ) -> Result<Vec<(LibraryItem, Sharing)>, SharingError> {
        Ok(self
            .world_book
            .sharing
            .sharing_of(self.profile.id, project, kind)?)
    }

    /// Fails when the item is a linked copy: read-only until unlinked.
    pub(crate) fn refuse_linked(&self, item: LibraryItem) -> Result<(), SharingError> {
        if self.world_book.sharing.sharing(item)?.is_linked() {
            return Err(SharingError::Linked);
        }
        Ok(())
    }

    /// Whether the item is a linked copy, for the edits it refuses.
    pub(crate) fn is_linked_copy(&self, item: LibraryItem) -> bool {
        match self.world_book.sharing.sharing(item) {
            Ok(sharing) => sharing.is_linked(),
            Err(error) => {
                tracing::warn!(%error, "could not read an item's sharing");
                false
            }
        }
    }

    /// Marks a project's own item shareable, or not: unmarking makes every
    /// linked copy of it independent, each saying so once.
    pub fn set_item_shareable(
        &self,
        item: LibraryItem,
        shareable: bool,
    ) -> Result<(), SharingError> {
        let facts = self.item_facts(item)?;
        if facts.owner.is_none() {
            return Err(SharingError::NotOwnItem);
        }
        if shareable {
            self.refuse_linked(item)?;
        } else if let LibraryItem::World(id) = item
            && !self.world_book.follow_pictures(id)
        {
            // Copies turned independent keep only what reached them.
            return Err(SharingError::FolderUnreachable);
        }
        self.world_book.sharing.set_shareable(item, shareable)?;
        tracing::info!(shareable, "marked a library item");
        Ok(())
    }

    /// The shareable items of `kind` that the open project may import:
    /// every other project's, archived projects' too.
    pub fn importable_items(&self, kind: LibraryKind) -> Result<Vec<ShareableItem>, SharingError> {
        let open = self
            .open_project_id()?
            .ok_or(SharingError::ProjectNotFound)?;
        self.importable_items_for(kind, open)
    }

    /// The shareable items of `kind` that `project` may import.
    pub fn importable_items_for(
        &self,
        kind: LibraryKind,
        project: ProjectId,
    ) -> Result<Vec<ShareableItem>, SharingError> {
        Ok(self
            .world_book
            .sharing
            .shareable_items(self.profile.id, kind)?
            .into_iter()
            .filter(|item| item.project != project)
            .collect())
    }

    /// Imports another project's shareable item into the open project,
    /// linked or independent, as `name` (`None`: the original's). A name
    /// the project has is refused with a suggestion.
    pub fn import_item(
        &self,
        item: LibraryItem,
        copy: CopyKind,
        name: Option<&str>,
    ) -> Result<LibraryItem, SharingError> {
        let open = self
            .open_project_id()?
            .ok_or(SharingError::ProjectNotFound)?;
        self.import_item_into(item, open, copy, name)
    }

    /// [`Bardo::import_item`] into `into`, the project the screen shows.
    pub fn import_item_into(
        &self,
        item: LibraryItem,
        into: ProjectId,
        copy: CopyKind,
        name: Option<&str>,
    ) -> Result<LibraryItem, SharingError> {
        if !self.own_project_exists(into)? {
            return Err(SharingError::ProjectNotFound);
        }
        let facts = self.item_facts(item)?;
        if facts.owner.is_none() {
            return Err(SharingError::NotShareable);
        }
        if facts.owner == Some(into) {
            return Err(SharingError::SameProject);
        }
        if !self.world_book.sharing.sharing(item)?.shareable {
            return Err(SharingError::NotShareable);
        }
        self.copy_into(item, &facts, Some(into), copy, name)
    }

    /// Copies a project's item into another project as an independent copy.
    pub fn export_item(
        &self,
        item: LibraryItem,
        to: ProjectId,
        name: Option<&str>,
    ) -> Result<LibraryItem, SharingError> {
        let facts = self.item_facts(item)?;
        if facts.owner.is_none() {
            return Err(SharingError::NotOwnItem);
        }
        if facts.owner == Some(to) {
            return Err(SharingError::SameProject);
        }
        self.refuse_linked(item)?;
        // An archived project is read-only: only an import source.
        let active = self.projects.project(to)?.is_some_and(|found| {
            found.owner == self.profile.id && found.state == bardo_domain::ProjectState::Active
        });
        if !active {
            return Err(SharingError::ProjectNotFound);
        }
        self.copy_into(item, &facts, Some(to), CopyKind::Independent, name)
    }

    fn copy_into(
        &self,
        item: LibraryItem,
        facts: &ItemFacts,
        into: Option<ProjectId>,
        copy: CopyKind,
        name: Option<&str>,
    ) -> Result<LibraryItem, SharingError> {
        let name = self.copy_name(item, facts, into, name)?;
        if let LibraryItem::World(_) = item {
            // The pictures are copied right after; a folder that cannot be
            // reached would leave the copy without them.
            let (source, target) = (
                self.world_book.folder(facts.owner),
                self.world_book.folder(into),
            );
            if !self.world_book.files.reachable(&source)
                || !self.world_book.files.reachable(&target)
            {
                return Err(SharingError::FolderUnreachable);
            }
        }
        let copied =
            self.world_book
                .sharing
                .copy_item(item, into, &name, copy, SystemTime::now())?;
        if let (LibraryItem::World(from), LibraryItem::World(to)) = (item, copied) {
            self.world_book.copy_pictures(from, to);
        }
        tracing::info!(?copy, "copied a library item into a project");
        Ok(copied)
    }

    /// Makes a linked copy independent, with what it holds.
    pub fn unlink_item(&self, item: LibraryItem) -> Result<(), SharingError> {
        self.item_facts(item)?;
        if !self.world_book.sharing.sharing(item)?.is_linked() {
            return Err(SharingError::NotLinked);
        }
        if let LibraryItem::World(id) = item
            && self.world_book.follow_into(id) != PictureFollowing::Current
        {
            // Its pictures are its own from now on: the last ones arrive
            // first, or it stays linked.
            return Err(SharingError::FolderUnreachable);
        }
        self.world_book.sharing.unlink(item)?;
        tracing::info!("unlinked a library item");
        Ok(())
    }

    /// The user saw that the copy became independent.
    pub fn dismiss_unlinked(&self, item: LibraryItem) -> Result<(), SharingError> {
        self.item_facts(item)?;
        self.world_book.sharing.forget_following(item)?;
        Ok(())
    }

    /// The projects whose global template of `kind` making `item` global
    /// would change: every project using the global one.
    pub fn make_global_users(&self, item: LibraryItem) -> Result<Vec<String>, SharingError> {
        match item {
            LibraryItem::Template { kind, project } => Ok(self
                .global_template_users(kind)
                .map_err(|_| SharingError::NotFound)?
                .into_iter()
                .filter(|name| project.is_none_or(|own| *name != self.project_name(own)))
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    /// Moves a project's own item to the global library as `name` (`None`:
    /// its own). The project keeps using it by reference, and every linked
    /// copy of it becomes a reference to the global item. A template adds
    /// its text as the next global version, which needs `confirmed` while
    /// other projects use the global template (`UsedByProjects`).
    pub fn make_item_global(
        &self,
        item: LibraryItem,
        name: Option<&str>,
        confirmed: bool,
    ) -> Result<(), SharingError> {
        let facts = self.item_facts(item)?;
        let Some(project) = facts.owner else {
            return Err(SharingError::NotOwnItem);
        };
        self.refuse_linked(item)?;
        let name = self.copy_name(item, &facts, None, name)?;
        if !confirmed {
            let users = self.make_global_users(item)?;
            if !users.is_empty() {
                return Err(SharingError::UsedByProjects(users));
            }
        }
        if let LibraryItem::World(id) = item {
            // Its pictures go to the global library's folder first.
            self.world_book
                .move_pictures(id, Some(project), None)
                .map_err(|_| SharingError::FolderUnreachable)?;
        }
        self.world_book
            .sharing
            .make_global(item, &name, SystemTime::now())?;
        if let LibraryItem::World(id) = item {
            // Its linked copies' files in their own folders only cost disk
            // space, and are left.
            self.world_book.forget_files(id, Some(project));
        }
        tracing::info!("made a library item global");
        Ok(())
    }

    /// The file name the save dialog suggests for the item's file.
    pub fn item_file_name(&self, item: LibraryItem) -> Result<String, SharingError> {
        let facts = self.item_facts(item)?;
        let stem = match item {
            LibraryItem::Template { kind, .. } => {
                self.text(Text::TemplateKindName(kind)).into_owned()
            }
            _ => facts.name,
        };
        Ok(crate::item_file::file_name(&stem))
    }

    /// The item as an item file: what another project, profile or machine
    /// opens.
    pub fn item_file(&self, item: LibraryItem) -> Result<String, SharingError> {
        self.item_facts(item)?;
        crate::item_file::write(self, item)
    }

    /// Writes the item's file to `path`, which the user chose.
    pub fn save_item_file(&self, item: LibraryItem, path: &Path) -> Result<(), SharingError> {
        let text = self.item_file(item)?;
        // Written beside it, then put in place: a failed write never
        // leaves a cut file, nor spoils one that was there.
        let partial = path.with_extension("bardo-item-partial");
        std::fs::write(&partial, text)
            .and_then(|()| std::fs::rename(&partial, path))
            .map_err(|error| {
                let _ = std::fs::remove_file(&partial);
                tracing::warn!(%error, "could not write an item file");
                SharingError::WriteFailed(error.to_string())
            })?;
        tracing::info!("saved an item file");
        Ok(())
    }

    /// Opens the item file at `path` (or a `.bardo-persona` package) as an
    /// independent copy owned by `into` (`None`: the global library). A
    /// file with product pictures needs `confirmed`.
    pub fn open_item_file(
        &self,
        path: &Path,
        into: Option<ProjectId>,
        confirmed: bool,
    ) -> Result<LibraryItem, SharingError> {
        let text = crate::item_file::read_file(path)?;
        self.open_item_text(&text, into, confirmed)
    }

    /// [`Bardo::open_item_file`] from the screen of `kind`: a file that
    /// holds another kind of item is refused (`WrongKind`).
    pub fn open_item_file_as(
        &self,
        path: &Path,
        kind: LibraryKind,
        into: Option<ProjectId>,
        confirmed: bool,
    ) -> Result<LibraryItem, SharingError> {
        let text = crate::item_file::read_file(path)?;
        if !crate::item_file::holds(&text, kind) {
            return Err(SharingError::WrongKind);
        }
        self.open_item_text(&text, into, confirmed)
    }

    /// As `open_item_file`, from the file's text.
    pub fn open_item_text(
        &self,
        text: &str,
        into: Option<ProjectId>,
        confirmed: bool,
    ) -> Result<LibraryItem, SharingError> {
        if let Some(project) = into
            && !self.own_project_exists(project)?
        {
            return Err(SharingError::ProjectNotFound);
        }
        let opened = crate::item_file::open(self, text, into, confirmed)?;
        tracing::info!("opened an item file");
        Ok(opened)
    }

    /// The template kind's item in `project`, for the Templates screen.
    pub fn template_item(project: Option<ProjectId>, kind: TemplateKind) -> LibraryItem {
        LibraryItem::Template { project, kind }
    }

    /// Every linked world item's pictures, copied where missing: run at
    /// start, so copies made while a folder could not be reached catch up.
    pub fn follow_world_pictures(&self) {
        let owner = self.profile.id;
        let projects = match self.projects.projects(owner) {
            Ok(projects) => projects,
            Err(error) => {
                tracing::warn!(%error, "could not list projects to follow pictures");
                return;
            }
        };
        for project in projects {
            let Ok(items) = self.world_book.world.characters(owner, Some(project.id)) else {
                continue;
            };
            for item in items {
                if self.is_linked_copy(LibraryItem::World(item.id)) {
                    self.world_book.follow_into(item.id);
                }
            }
        }
    }

    /// Whether the world item `id` is a linked copy.
    pub fn world_item_linked(&self, id: WorldItemId) -> bool {
        self.is_linked_copy(LibraryItem::World(id))
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        Look, PictureOrigin, PictureProvenance, ProductKind, ProductOwnership, ProjectColor,
        ProjectDraft, Provider, TemplateSource, VisualStyle, WorldItem, WorldKind,
    };

    use super::*;
    use crate::scenes::tests::Harness;
    use crate::{PersonaError, ProductError, TemplateError, WorldError};

    fn project(app: &Bardo, name: &str) -> ProjectId {
        app.create_project(ProjectDraft {
            name: name.into(),
            color: ProjectColor::Teal,
        })
        .unwrap()
        .id
    }

    /// Space shares, Sea imports; Sea is open at the end.
    fn two_projects(app: &Bardo) -> (ProjectId, ProjectId) {
        let space = project(app, "Space");
        let sea = project(app, "Sea");
        (space, sea)
    }

    fn place(app: &Bardo, owner: ProjectId, name: &str) -> WorldItem {
        app.create_world_item(
            Some(owner),
            WorldKind::Place,
            WorldItemDraft {
                name: name.into(),
                appearance: "A harbour at dawn, wooden boats".into(),
                ..WorldItemDraft::default()
            },
        )
        .unwrap()
    }

    fn png(mark: u8) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(1200u32.to_be_bytes());
        png.extend(800u32.to_be_bytes());
        png.extend([8, 2, 0, 0, 0, mark]);
        png
    }

    const LOOK: Look = Look::Style(VisualStyle::Watercolor);

    /// Keeps a drawn picture for `item` in the watercolor look.
    fn draw_kept(app: &Bardo, item: &WorldItem, mark: u8) {
        app.world_book
            .keep_drawn(
                item,
                LOOK,
                &png(mark),
                "png",
                PictureProvenance {
                    provider: Some(Provider::Gemini),
                    model: "gemini-image".into(),
                    prompt: format!("A harbour, drawing {mark}"),
                    generated_at: SystemTime::now(),
                    job: None,
                    origin: PictureOrigin::World,
                    confirmed_at: None,
                },
            )
            .unwrap()
            .expect("kept");
    }

    fn kept_file(app: &Bardo, item: WorldItemId) -> Option<Vec<u8>> {
        let found = app.world_book.world.character(item).unwrap().unwrap();
        let picture = app.world_book.kept(item, &LOOK).unwrap()?;
        app.world_book
            .files
            .read(&app.world_book.folder(found.owner_project), &picture.file)
            .ok()
    }

    fn import_linked(app: &Bardo, item: LibraryItem) -> LibraryItem {
        app.import_item(item, CopyKind::Linked, None).unwrap()
    }

    #[test]
    fn a_suggested_name_names_its_source_then_counts() {
        assert_eq!(suggested_name("Ana", "Space", 80, |_| false), "Ana (Space)");
        assert_eq!(
            suggested_name("Ana", "Space", 80, |name| name == "Ana (Space)"),
            "Ana (Space 2)"
        );
        let long = "N".repeat(80);
        let suggested = suggested_name(&long, &"P".repeat(100), 80, |_| false);
        assert_eq!(suggested.chars().count(), 80);
        assert!(suggested.starts_with("NNNN") && suggested.ends_with("P)"));
    }

    #[test]
    fn only_other_projects_shareable_items_are_offered_for_import() {
        let h = Harness::new();
        let app = h.start();
        let (space, sea) = two_projects(&app);
        let harbour = place(&app, space, "Harbour");
        place(&app, space, "Lighthouse");
        let item = LibraryItem::World(harbour.id);
        let kind = LibraryKind::World(WorldKind::Place);
        assert!(
            app.importable_items(kind).unwrap().is_empty(),
            "none marked"
        );
        assert!(matches!(
            app.import_item(item, CopyKind::Linked, None),
            Err(SharingError::NotShareable)
        ));
        app.set_item_shareable(item, true).unwrap();
        let offered = app.importable_items(kind).unwrap();
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].project_name, "Space");
        app.switch_project(space).unwrap();
        assert!(
            app.importable_items(kind).unwrap().is_empty(),
            "a project's own are not offered to it"
        );
        app.switch_project(sea).unwrap();
        // A global item is never marked.
        let global = app
            .create_world_item(
                None,
                WorldKind::Place,
                WorldItemDraft {
                    name: "Moon".into(),
                    appearance: "Grey dust".into(),
                    ..WorldItemDraft::default()
                },
            )
            .unwrap();
        assert!(matches!(
            app.set_item_shareable(LibraryItem::World(global.id), true),
            Err(SharingError::NotOwnItem)
        ));
    }

    #[test]
    fn a_linked_world_item_follows_its_original_and_refuses_edits() {
        let h = Harness::new();
        let app = h.start();
        let (space, _sea) = two_projects(&app);
        let harbour = place(&app, space, "Harbour");
        draw_kept(&app, &harbour, 1);
        let item = LibraryItem::World(harbour.id);
        app.set_item_shareable(item, true).unwrap();
        let LibraryItem::World(copy) = import_linked(&app, item) else {
            panic!("a world item");
        };
        assert_eq!(
            kept_file(&app, copy),
            Some(png(1)),
            "its picture came along"
        );
        let shown = app.item_sharing(LibraryItem::World(copy)).unwrap();
        assert!(shown.sharing.is_linked());
        assert_eq!(shown.pictures, PictureFollowing::Current);
        let following = shown.sharing.following.unwrap();
        assert_eq!(
            (following.name.as_str(), following.project.as_str()),
            ("Harbour", "Space")
        );

        // Every edit of the copy is refused.
        let draft = WorldItemDraft {
            name: "Harbour".into(),
            appearance: "Changed".into(),
            ..WorldItemDraft::default()
        };
        assert!(matches!(
            app.edit_world_item(copy, draft.clone(), true),
            Err(WorldError::Linked)
        ));
        assert!(matches!(
            app.draw_world_picture(copy, LOOK, crate::BudgetConsent::Ask),
            Err(WorldError::Linked)
        ));
        assert!(matches!(
            app.set_item_shareable(LibraryItem::World(copy), true),
            Err(SharingError::Linked)
        ));

        // The original's edits and new pictures reach it.
        app.edit_world_item(
            harbour.id,
            WorldItemDraft {
                name: "Harbour".into(),
                appearance: "A harbour at night".into(),
                ..WorldItemDraft::default()
            },
            true,
        )
        .unwrap();
        let followed = app.world_book.world.character(copy).unwrap().unwrap();
        assert_eq!(followed.details.appearance(), "A harbour at night");
        let harbour = app.world_book.world.character(harbour.id).unwrap().unwrap();
        draw_kept(&app, &harbour, 2);
        assert_eq!(
            kept_file(&app, copy),
            Some(png(2)),
            "the new picture followed"
        );
    }

    #[test]
    fn a_linked_copys_pictures_wait_for_an_unreachable_folder_then_arrive() {
        let h = Harness::new();
        let app = h.start();
        let (space, sea) = two_projects(&app);
        let harbour = place(&app, space, "Harbour");
        let item = LibraryItem::World(harbour.id);
        app.set_item_shareable(item, true).unwrap();
        let LibraryItem::World(copy) = import_linked(&app, item) else {
            panic!("a world item");
        };
        let sea_folder = app.world_book.folder(Some(sea));
        h.world_files.unplug(&sea_folder);
        draw_kept(&app, &harbour, 3);
        assert_eq!(
            app.item_sharing(LibraryItem::World(copy)).unwrap().pictures,
            PictureFollowing::WaitingForFolder
        );
        // Turned independent now, it would lose the picture for good.
        assert!(matches!(
            app.unlink_item(LibraryItem::World(copy)),
            Err(SharingError::FolderUnreachable)
        ));
        assert!(matches!(
            app.set_item_shareable(item, false),
            Err(SharingError::FolderUnreachable)
        ));
        assert!(matches!(
            app.delete_world_item(harbour.id),
            Err(WorldError::FolderNotFound)
        ));
        h.world_files.plug(&sea_folder);
        app.follow_world_pictures();
        assert_eq!(kept_file(&app, copy), Some(png(3)));
        assert_eq!(
            app.item_sharing(LibraryItem::World(copy)).unwrap().pictures,
            PictureFollowing::Current
        );
    }

    #[test]
    fn unmarking_or_deleting_the_original_leaves_independent_copies_that_say_so_once() {
        let h = Harness::new();
        let app = h.start();
        let (space, _sea) = two_projects(&app);
        let harbour = place(&app, space, "Harbour");
        let lighthouse = place(&app, space, "Lighthouse");
        draw_kept(&app, &harbour, 4);
        let mut copies = Vec::new();
        for original in [&harbour, &lighthouse] {
            let item = LibraryItem::World(original.id);
            app.set_item_shareable(item, true).unwrap();
            copies.push(import_linked(&app, item));
        }
        app.set_item_shareable(LibraryItem::World(harbour.id), false)
            .unwrap();
        app.delete_world_item(lighthouse.id).unwrap();
        for copy in &copies {
            let shown = app.item_sharing(*copy).unwrap();
            assert!(!shown.sharing.is_linked());
            assert!(shown.sharing.lost().is_some(), "it says so");
            app.dismiss_unlinked(*copy).unwrap();
            assert!(
                app.item_sharing(*copy).unwrap().sharing.lost().is_none(),
                "once"
            );
        }
        let LibraryItem::World(harbour_copy) = copies[0] else {
            panic!("a world item");
        };
        assert_eq!(
            kept_file(&app, harbour_copy),
            Some(png(4)),
            "content intact"
        );
        // Independent now: it may be edited.
        app.edit_world_item(
            harbour_copy,
            WorldItemDraft {
                name: "Harbour".into(),
                appearance: "Our own harbour".into(),
                ..WorldItemDraft::default()
            },
            true,
        )
        .unwrap();
    }

    #[test]
    fn an_import_under_a_taken_name_suggests_one_naming_its_project() {
        let h = Harness::new();
        let app = h.start();
        let (space, sea) = two_projects(&app);
        let harbour = place(&app, space, "Harbour");
        place(&app, sea, "Harbour");
        let item = LibraryItem::World(harbour.id);
        app.set_item_shareable(item, true).unwrap();
        let Err(SharingError::NameClash { suggestion }) =
            app.import_item(item, CopyKind::Independent, None)
        else {
            panic!("refused");
        };
        assert_eq!(suggestion, "Harbour (Space)");
        let copy = app
            .import_item(item, CopyKind::Independent, Some(&suggestion))
            .unwrap();
        let shown = app.item_sharing(copy).unwrap();
        assert_eq!(
            shown.sharing,
            Sharing::default(),
            "independent from the start"
        );
    }

    fn narrator_in(app: &Bardo, project: ProjectId, name: &str) -> bardo_domain::Persona {
        let global = app.narrators(None).unwrap().remove(0);
        app.create_persona_in(
            Some(project),
            PersonaDraft {
                name: name.into(),
                ..PersonaDraft::from(&global.details)
            },
        )
        .unwrap()
    }

    #[test]
    fn a_linked_narrator_and_product_refuse_edits_and_follow_saves() {
        let h = Harness::new();
        let app = h.start();
        let (space, sea) = two_projects(&app);
        let ana = narrator_in(&app, space, "Ana");
        app.set_item_shareable(LibraryItem::Narrator(ana.id), true)
            .unwrap();
        let LibraryItem::Narrator(copy) = import_linked(&app, LibraryItem::Narrator(ana.id)) else {
            panic!("a narrator");
        };
        let draft = |tone: &str| PersonaDraft {
            tone: tone.into(),
            ..PersonaDraft::from(&ana.details)
        };
        assert!(matches!(
            app.update_persona(copy, draft("Loud."), true),
            Err(PersonaError::Linked)
        ));
        app.update_persona(ana.id, draft("Soft."), true).unwrap();
        assert_eq!(
            app.own_persona(copy).unwrap().details.tone(),
            "Soft.",
            "the original's save reached it"
        );

        let lamp = app
            .create_product_in(
                Some(space),
                ProductDraft {
                    name: "Lamp".into(),
                    kind: ProductKind::Physical,
                    description: "A folding lamp.".into(),
                    audience: "Campers.".into(),
                    facts: "Folds flat".into(),
                    link: "https://example.com/lamp".into(),
                    call_to_action: "Get yours".into(),
                    ownership: ProductOwnership::Mine,
                    ..ProductDraft::default()
                },
            )
            .unwrap();
        app.set_item_shareable(LibraryItem::Product(lamp.id), true)
            .unwrap();
        let LibraryItem::Product(lamp_copy) = import_linked(&app, LibraryItem::Product(lamp.id))
        else {
            panic!("a product");
        };
        let found = app.products.get(lamp_copy).unwrap().unwrap();
        assert_eq!(found.owner_project, Some(sea));
        assert!(matches!(
            app.update_product(
                lamp_copy,
                ProductDraft {
                    description: "Changed.".into(),
                    ..ProductDraft::from(&found.brief)
                },
                true
            ),
            Err(ProductError::Linked)
        ));
        assert!(matches!(
            app.remove_product_logo(lamp_copy),
            Err(ProductError::Linked)
        ));
        assert!(matches!(
            app.confirm_claim(lamp_copy, bardo_domain::ClaimId::new()),
            Err(crate::SheetError::Linked)
        ));
        assert!(matches!(
            app.remove_look_asset(copy, Provider::BytePlus),
            Err(crate::LookError::Linked)
        ));
        // A linked copy is exported by its original only.
        let desert = project(&app, "Desert");
        assert!(matches!(
            app.export_item(LibraryItem::Product(lamp_copy), desert, None),
            Err(SharingError::Linked)
        ));
        app.archive_project(desert, false).unwrap();
        assert!(matches!(
            app.export_item(LibraryItem::Product(lamp.id), desert, None),
            Err(SharingError::ProjectNotFound)
        ));
    }

    #[test]
    fn making_an_item_global_turns_linked_copies_into_references() {
        let h = Harness::new();
        let app = h.start();
        let (space, sea) = two_projects(&app);
        let harbour = place(&app, space, "Harbour");
        draw_kept(&app, &harbour, 5);
        let item = LibraryItem::World(harbour.id);
        app.set_item_shareable(item, true).unwrap();
        let LibraryItem::World(copy) = import_linked(&app, item) else {
            panic!("a world item");
        };
        app.switch_project(space).unwrap();
        app.make_item_global(item, None, false).unwrap();
        let global = app.world_book.world.character(harbour.id).unwrap().unwrap();
        assert_eq!(global.owner_project, None);
        assert_eq!(
            kept_file(&app, harbour.id),
            Some(png(5)),
            "its pictures moved"
        );
        assert!(
            app.world_book.world.character(copy).unwrap().is_none(),
            "the copy is gone"
        );
        assert!(
            app.world_book
                .world
                .characters(app.profile().id, Some(sea))
                .unwrap()
                .is_empty(),
            "no copy follows a global item"
        );
        assert!(matches!(
            app.make_item_global(item, None, false),
            Err(SharingError::NotOwnItem)
        ));
    }

    #[test]
    fn a_linked_template_refuses_saves_and_making_it_global_asks_first() {
        let h = Harness::new();
        let app = h.start();
        let (space, sea) = two_projects(&app);
        let kind = TemplateKind::Script;
        app.make_project_template(kind, space).unwrap();
        let item = LibraryItem::template(space, kind);
        app.set_item_shareable(item, true).unwrap();
        let copy = import_linked(&app, item);
        assert_eq!(copy, LibraryItem::template(sea, kind));
        let in_use = app.template_in_use(kind, Some(sea)).unwrap();
        assert_eq!(in_use.source, TemplateSource::Project);
        let body = in_use.versions[0].body.clone();
        let changed = format!("{} Keep it short.", body.instructions());
        assert!(matches!(
            app.save_template_in(kind, Some(sea), (&changed, body.prompt()), true),
            Err(TemplateError::Linked)
        ));
        app.save_template_in(kind, Some(space), (&changed, body.prompt()), true)
            .unwrap();
        let followed = app.template_in_use(kind, Some(sea)).unwrap();
        assert_eq!(followed.versions[0].body.instructions(), changed);

        let other = project(&app, "Desert");
        let Err(SharingError::UsedByProjects(users)) = app.make_item_global(item, None, false)
        else {
            panic!("asks first");
        };
        assert_eq!(users, ["Desert"]);
        app.make_item_global(item, None, true).unwrap();
        for project in [space, sea, other] {
            let in_use = app.template_in_use(kind, Some(project)).unwrap();
            assert_eq!(in_use.source, TemplateSource::Global);
            assert_eq!(in_use.versions[0].body.instructions(), changed);
        }
        // Its own template again, numbered after the retired versions.
        let again = app.make_project_template(kind, space).unwrap();
        assert_eq!(again.number, 3);
    }

    #[test]
    fn an_item_file_opens_on_its_own_screen_and_a_global_template_asks_first() {
        let h = Harness::new();
        let app = h.start();
        let (space, sea) = two_projects(&app);
        let ana = narrator_in(&app, space, "Ana");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ana.bardo-item");
        app.save_item_file(LibraryItem::Narrator(ana.id), &path)
            .unwrap();
        assert!(matches!(
            app.open_item_file_as(&path, LibraryKind::Product, Some(sea), false),
            Err(SharingError::WrongKind)
        ));
        assert!(matches!(
            app.open_item_file_as(&path, LibraryKind::Narrator, Some(sea), false),
            Ok(LibraryItem::Narrator(_))
        ));

        let kind = TemplateKind::Script;
        app.make_project_template(kind, space).unwrap();
        project(&app, "Desert");
        let text = app.item_file(LibraryItem::template(space, kind)).unwrap();
        let Err(SharingError::UsedByProjects(users)) = app.open_item_text(&text, None, false)
        else {
            panic!("asks before changing the global template");
        };
        assert!(users.contains(&"Desert".to_owned()));
        app.open_item_text(&text, None, true).unwrap();
    }

    #[test]
    fn item_files_round_trip_every_kind() {
        let h = Harness::new();
        let app = h.start();
        let (space, sea) = two_projects(&app);
        // A narrator.
        let ana = narrator_in(&app, space, "Ana");
        let text = app.item_file(LibraryItem::Narrator(ana.id)).unwrap();
        let LibraryItem::Narrator(opened) = app.open_item_text(&text, Some(sea), false).unwrap()
        else {
            panic!("a narrator");
        };
        let opened = app.own_persona(opened).unwrap();
        assert_eq!(opened.owner_project, Some(sea));
        assert_eq!(opened.details.tone(), ana.details.tone());
        // A product, with its pictures once confirmed.
        let lamp = app
            .create_product_in(
                Some(space),
                ProductDraft {
                    name: "Lamp".into(),
                    kind: ProductKind::Physical,
                    description: "A folding lamp.".into(),
                    audience: "Campers.".into(),
                    facts: "Folds flat".into(),
                    link: "https://example.com/lamp".into(),
                    call_to_action: "Get yours".into(),
                    ownership: ProductOwnership::Mine,
                    ..ProductDraft::default()
                },
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("lamp.png");
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(800u32.to_be_bytes());
        bytes.extend(1000u32.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 7]);
        std::fs::write(&photo, &bytes).unwrap();
        app.import_product_image(lamp.id, &photo, true).unwrap();
        let text = app.item_file(LibraryItem::Product(lamp.id)).unwrap();
        assert!(matches!(
            app.open_item_text(&text, Some(sea), false),
            Err(SharingError::NotConfirmed)
        ));
        let LibraryItem::Product(opened) = app.open_item_text(&text, Some(sea), true).unwrap()
        else {
            panic!("a product");
        };
        let opened = app.products.get(opened).unwrap().unwrap();
        assert_eq!(opened.brief, lamp.brief);
        assert_eq!(app.products.images(opened.id).unwrap().len(), 1);
        // A place, with its drawn picture.
        let harbour = place(&app, space, "Harbour");
        draw_kept(&app, &harbour, 8);
        let path = dir
            .path()
            .join(app.item_file_name(LibraryItem::World(harbour.id)).unwrap());
        app.save_item_file(LibraryItem::World(harbour.id), &path)
            .unwrap();
        let LibraryItem::World(opened) = app.open_item_file(&path, Some(sea), false).unwrap()
        else {
            panic!("a world item");
        };
        let found = app.world_book.world.character(opened).unwrap().unwrap();
        assert_eq!(found.kind, WorldKind::Place);
        assert_eq!(found.details.appearance(), harbour.details.appearance());
        assert_eq!(kept_file(&app, opened), Some(png(8)));
        // Opened twice: the name is taken, the second says it was imported.
        let LibraryItem::World(again) = app.open_item_file(&path, Some(sea), false).unwrap() else {
            panic!("a world item");
        };
        let again = app.world_book.world.character(again).unwrap().unwrap();
        assert_ne!(again.details.name(), "Harbour");
        // A template, into the global library.
        let kind = TemplateKind::Chapters;
        app.make_project_template(kind, space).unwrap();
        let current = app.current_template_in(kind, Some(space)).unwrap();
        let changed = format!("{} From a file.", current.body.instructions());
        app.save_template_in(kind, Some(space), (&changed, current.body.prompt()), true)
            .unwrap();
        let text = app.item_file(LibraryItem::template(space, kind)).unwrap();
        app.open_item_text(&text, None, true).unwrap();
        assert_eq!(
            app.current_template_in(kind, None)
                .unwrap()
                .body
                .instructions(),
            changed
        );
    }

    #[test]
    fn an_old_persona_package_opens_and_a_newer_item_file_is_refused() {
        let h = Harness::new();
        let app = h.start();
        let (_space, sea) = two_projects(&app);
        let ana = narrator_in(&app, sea, "Ana");
        let package = app.export_persona(ana.id).unwrap();
        assert!(package.contains("\"bardo-persona\""));
        let LibraryItem::Narrator(opened) = app.open_item_text(&package, None, false).unwrap()
        else {
            panic!("a narrator");
        };
        assert!(app.own_persona(opened).unwrap().is_global());

        let newer = r#"{"format": "bardo-item", "version": 2, "kind": "template"}"#;
        assert!(matches!(
            app.open_item_text(newer, None, false),
            Err(SharingError::File(crate::ItemFileError::NewerVersion(2)))
        ));
        assert!(matches!(
            app.open_item_text("{}", None, false),
            Err(SharingError::File(crate::ItemFileError::NotAnItemFile))
        ));
    }
}
