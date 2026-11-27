//! Sharing library items between projects (#340, spec "Library scope and
//! sharing", ADR-0047 item 5). A project's item is shared only when the
//! user marks it *shareable*; another project then imports it as a
//! **linked copy**, which follows every change of its original, or as an
//! **independent copy**. A linked copy is read-only, and becomes
//! independent when the user unlinks it or its original is lost (deleted,
//! unmarked, or its project deleted), which it says once.

use std::time::SystemTime;

use crate::{
    PersonaId, ProductId, ProfileId, ProjectId, RepositoryError, TemplateKind, WorldItemId,
    WorldKind,
};

/// One library item, whatever its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryItem {
    Narrator(PersonaId),
    Product(ProductId),
    /// A character, place or object of a world.
    World(WorldItemId),
    /// The template of `kind` that `project` owns, or the global one for
    /// `None`: a template's versions are one item.
    Template {
        project: Option<ProjectId>,
        kind: TemplateKind,
    },
}

/// What kind of library item, for the lists of what may be imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryKind {
    Narrator,
    Product,
    World(WorldKind),
    Template,
}

impl LibraryItem {
    /// The template `project` owns of `kind`.
    pub fn template(project: ProjectId, kind: TemplateKind) -> Self {
        LibraryItem::Template {
            project: Some(project),
            kind,
        }
    }
}

/// What a linked copy follows, by name: shown as *Follows <name> from
/// <project>* while linked, and once more when the original is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Following {
    /// The original's name (for a template, empty: its kind names it).
    pub name: String,
    /// The original's project's name.
    pub project: String,
    /// The original while the copy follows it; `None` once it was lost
    /// and the copy became independent, until the user saw that.
    pub original: Option<LibraryItem>,
}

/// How an item takes part in sharing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sharing {
    /// Other projects may import it.
    pub shareable: bool,
    /// What it follows, or followed until it was lost.
    pub following: Option<Following>,
}

impl Sharing {
    /// A linked copy: read-only, it follows its original.
    pub fn is_linked(&self) -> bool {
        self.following
            .as_ref()
            .is_some_and(|following| following.original.is_some())
    }

    /// What it followed until the original was lost, while the user has
    /// not seen that yet.
    pub fn lost(&self) -> Option<&Following> {
        self.following
            .as_ref()
            .filter(|following| following.original.is_none())
    }
}

/// A shareable item of a project, as another project's import lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareableItem {
    pub item: LibraryItem,
    /// Its name (for a template, empty: its kind names it).
    pub name: String,
    pub project: ProjectId,
    pub project_name: String,
    /// Its project is archived: its items may still be imported.
    pub archived_project: bool,
}

/// How an item is copied into another owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyKind {
    /// Read-only, following every change of the original.
    Linked,
    /// The owner's own from then on.
    Independent,
}

/// Where sharing is kept. Every change of an original reaches its linked
/// copies' rows in the same transaction, whoever writes it (the user or a
/// job), so the store does that itself; this port marks, copies, unlinks
/// and moves items.
pub trait SharingRepository: Send + Sync {
    /// How the item takes part in sharing; the default for one not found.
    fn sharing(&self, item: LibraryItem) -> Result<Sharing, RepositoryError>;

    /// The sharing of every item of `kind` that `project` owns, the global
    /// library's for `None`; items taking no part are left out.
    fn sharing_of(
        &self,
        owner: ProfileId,
        project: Option<ProjectId>,
        kind: LibraryKind,
    ) -> Result<Vec<(LibraryItem, Sharing)>, RepositoryError>;

    /// Marks a project's item shareable or not. Unmarking makes every
    /// linked copy of it independent, each saying so once.
    fn set_shareable(&self, item: LibraryItem, shareable: bool) -> Result<(), RepositoryError>;

    /// The shareable items of `kind` in every project of the profile,
    /// archived projects too.
    fn shareable_items(
        &self,
        owner: ProfileId,
        kind: LibraryKind,
    ) -> Result<Vec<ShareableItem>, RepositoryError>;

    /// Copies the item into `into` (`None`: the global library) as
    /// `name`, with all it holds; a template copies only its latest
    /// version, as the next version of `into`'s own. Returns the copy.
    fn copy_item(
        &self,
        item: LibraryItem,
        into: Option<ProjectId>,
        name: &str,
        copy: CopyKind,
        now: SystemTime,
    ) -> Result<LibraryItem, RepositoryError>;

    /// The linked copies that follow the item.
    fn linked_copies(&self, item: LibraryItem) -> Result<Vec<LibraryItem>, RepositoryError>;

    /// Makes a linked copy independent, with what it holds; it says
    /// nothing more.
    fn unlink(&self, item: LibraryItem) -> Result<(), RepositoryError>;

    /// The user saw that the copy became independent.
    fn forget_following(&self, item: LibraryItem) -> Result<(), RepositoryError>;

    /// Moves a project's item to the global library as `name`. Every
    /// linked copy of it becomes a reference to the global item: its uses
    /// point at it and its rows are dropped. A template adds its latest
    /// text as the next global version and its project, like its copies'
    /// projects, uses the global template again.
    fn make_global(
        &self,
        item: LibraryItem,
        name: &str,
        now: SystemTime,
    ) -> Result<(), RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_linked_while_it_has_an_original_and_lost_once_it_has_none() {
        let mut sharing = Sharing::default();
        assert!(!sharing.is_linked());
        assert!(sharing.lost().is_none());
        sharing.following = Some(Following {
            name: "Ana".into(),
            project: "Space".into(),
            original: Some(LibraryItem::Narrator(PersonaId::new())),
        });
        assert!(sharing.is_linked());
        assert!(sharing.lost().is_none());
        sharing.following.as_mut().unwrap().original = None;
        assert!(!sharing.is_linked());
        assert_eq!(sharing.lost().unwrap().name, "Ana");
    }
}
