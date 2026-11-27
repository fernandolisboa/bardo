//! What narrators, products and templates share as library items (#339,
//! spec "Library scope and sharing"): each is owned by a project or by the
//! global library. A project uses its own items and global ones by
//! reference; editing a global item first shows where it is used, and
//! *Bring into the project* makes an independent copy the project owns.

use bardo_domain::{ChannelId, ProjectId, RepositoryError, VideoProject};

use crate::Bardo;

/// One place a library item is used: a channel's default (no video), or
/// one video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryUse {
    /// The project's name.
    pub project: String,
    pub channel: String,
    /// The video's title, when the use is a video's own pick.
    pub video: Option<String>,
}

/// Whether an item owned by `item` (`None`: global) may be used by work
/// of `project`: the project's own and every global item, never another
/// project's.
pub fn usable_in(item: Option<ProjectId>, project: Option<ProjectId>) -> bool {
    item.is_none() || item == project
}

impl Bardo {
    /// The project's name, or an empty one when it is gone.
    pub(crate) fn project_name(&self, id: ProjectId) -> String {
        self.projects
            .project(id)
            .ok()
            .flatten()
            .map(|project| project.details.name().to_owned())
            .unwrap_or_default()
    }

    /// Fails unless `project` is one of the profile's projects.
    pub(crate) fn own_project_exists(&self, project: ProjectId) -> Result<bool, RepositoryError> {
        Ok(self
            .projects
            .project(project)?
            .is_some_and(|found| found.owner == self.profile.id))
    }

    /// The project a channel belongs to.
    pub(crate) fn channel_project(
        &self,
        channel: ChannelId,
    ) -> Result<Option<ProjectId>, RepositoryError> {
        Ok(self.channels.get(channel)?.map(|channel| channel.project))
    }

    /// Every video of the profile with its channel's name and project, for
    /// the uses of a library item.
    pub(crate) fn videos_with_places(
        &self,
    ) -> Result<Vec<(VideoProject, String, ProjectId)>, RepositoryError> {
        let mut videos = Vec::new();
        for channel in self.channels.list(self.profile.id)? {
            for video in self.themes.projects(channel.id)? {
                videos.push((video, channel.details.name().to_owned(), channel.project));
            }
        }
        Ok(videos)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::SystemTime;

    use bardo_domain::{
        ChannelDraft, Persona, PersonaDraft, Product, ProjectColor, ProjectDraft, TemplateKind,
        TemplateSource, VideoProject,
    };
    use bardo_storage::{Database, MemoryProjectFolders, MemorySecretStore, MemoryTrash};

    use super::*;
    use crate::{
        MoveBlock, PersonaError, ProductError, ProjectError, Removable, Repositories,
        TemplateError, testing,
    };

    fn start() -> Bardo {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let repositories = Repositories {
            trash: Arc::new(MemoryTrash::default()) as _,
            ..Repositories::shared(Arc::clone(&db), Arc::new(MemorySecretStore::default()))
        };
        let _disk = MemoryProjectFolders::for_database(&db);
        Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap()
    }

    fn project(app: &Bardo, name: &str) -> ProjectId {
        app.create_project(ProjectDraft {
            name: name.into(),
            color: ProjectColor::Teal,
        })
        .unwrap()
        .id
    }

    fn channel(app: &Bardo, name: &str, narrator: Option<&Persona>) -> bardo_domain::Channel {
        app.create_channel(ChannelDraft {
            name: name.into(),
            default_persona: narrator.map(|persona| persona.id),
            ..ChannelDraft::default()
        })
        .unwrap()
    }

    fn video(app: &Bardo, channel: ChannelId, title: &str) -> VideoProject {
        video_of(app, channel, title, None)
    }

    /// A video of `channel`, an ad of `product` when there is one.
    fn video_of(
        app: &Bardo,
        channel: ChannelId,
        title: &str,
        product: Option<bardo_domain::ProductId>,
    ) -> VideoProject {
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel,
            bardo_domain::Niche::new("space history").unwrap(),
            bardo_domain::ThemeIdea::new(title, "").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let mut video = theme.approve(SystemTime::now()).unwrap();
        video.product = product;
        app.themes.start_project(&theme, &video).unwrap();
        video
    }

    fn narrator_draft(app: &Bardo, name: &str) -> PersonaDraft {
        let global = app.narrators(None).unwrap().remove(0);
        PersonaDraft {
            name: name.into(),
            ..PersonaDraft::from(&global.details)
        }
    }

    fn names(personas: &[Persona]) -> Vec<&str> {
        personas.iter().map(|p| p.details.name()).collect()
    }

    fn product_names(products: &[Product]) -> Vec<&str> {
        products.iter().map(|p| p.brief.name()).collect()
    }

    #[test]
    fn a_project_uses_its_own_items_and_global_ones_only() {
        let mine = ProjectId::new();
        let other = ProjectId::new();
        assert!(usable_in(None, Some(mine)), "a global item");
        assert!(usable_in(Some(mine), Some(mine)), "its own");
        assert!(
            !usable_in(Some(other), Some(mine)),
            "never another project's"
        );
        assert!(
            usable_in(None, None),
            "work of no project uses global items"
        );
        assert!(!usable_in(Some(mine), None));
    }

    #[test]
    fn a_projects_narrators_are_its_own_and_the_global_ones_it_uses() {
        let app = start();
        let defaults = app.narrators(None).unwrap();
        assert_eq!(defaults.len(), 4, "the defaults are global");
        let space = project(&app, "Space");
        let own = app
            .create_persona_in(Some(space), narrator_draft(&app, "Ana"))
            .unwrap();
        assert_eq!(own.owner_project, Some(space));
        assert!(
            app.narrators(Some(space))
                .unwrap()
                .iter()
                .all(|persona| persona.owner_project == Some(space)),
            "no global narrator is used yet"
        );

        let used = &defaults[1];
        let channel = channel(&app, "Space Archives", Some(used));
        let listed = app.narrators(Some(space)).unwrap();
        assert_eq!(names(&listed), ["Ana", used.details.name()]);
        let other_global = &defaults[2];
        let apollo = video(&app, channel.id, "Apollo");
        app.set_project_persona(apollo.id, Some(other_global.id))
            .unwrap();
        assert_eq!(app.narrators(Some(space)).unwrap().len(), 3);

        // Pickers offer the project's own and every global one.
        assert_eq!(app.personas().unwrap().len(), 5);
        // The global library lists only global ones.
        assert_eq!(app.narrators(None).unwrap().len(), 4);

        // Another project neither lists nor may use Ana.
        let sea = project(&app, "Sea");
        assert!(app.narrators(Some(sea)).unwrap().is_empty());
        assert!(!names(&app.personas().unwrap()).contains(&"Ana"));
        assert!(
            app.create_channel(ChannelDraft {
                name: "Deep Sea".into(),
                default_persona: Some(own.id),
                ..ChannelDraft::default()
            })
            .is_err(),
            "a channel never takes another project's narrator"
        );
        let deep = channel_in(&app, "Deep Sea");
        let sea_video = video(&app, deep.id, "Abyss");
        assert!(matches!(
            app.set_project_persona(sea_video.id, Some(own.id)),
            Err(PersonaError::OtherProject)
        ));
    }

    fn channel_in(app: &Bardo, name: &str) -> bardo_domain::Channel {
        channel(app, name, None)
    }

    #[test]
    fn narrator_names_are_unique_per_owner() {
        let app = start();
        let space = project(&app, "Space");
        let global = app.narrators(None).unwrap().remove(0);
        let same = app
            .create_persona_in(Some(space), narrator_draft(&app, global.details.name()))
            .unwrap();
        assert_eq!(same.owner_project, Some(space), "free inside the project");
        assert!(matches!(
            app.create_persona_in(Some(space), narrator_draft(&app, global.details.name())),
            Err(PersonaError::NameTaken)
        ));
        assert!(matches!(
            app.create_persona(narrator_draft(&app, global.details.name())),
            Err(PersonaError::NameTaken)
        ));
        let copy = app.duplicate_persona(same.id).unwrap();
        assert_eq!(
            copy.owner_project,
            Some(space),
            "a duplicate keeps its owner"
        );
    }

    #[test]
    fn bringing_a_global_narrator_into_a_project_makes_an_independent_copy() {
        let app = start();
        let space = project(&app, "Space");
        let global = app.narrators(None).unwrap().remove(0);
        let copy = app
            .bring_persona_into_project(global.id, space, None)
            .unwrap();
        assert_eq!(copy.owner_project, Some(space));
        assert_eq!(copy.details, global.details);
        assert_ne!(copy.id, global.id);
        assert_eq!(
            app.narrators(None).unwrap().len(),
            4,
            "the global one stays"
        );

        // The name is taken now: a suggestion comes back.
        let error = app
            .bring_persona_into_project(global.id, space, None)
            .unwrap_err();
        let PersonaError::NameClash { suggestion } = &error else {
            panic!("expected a clash, got {error:?}");
        };
        assert_eq!(suggestion, &format!("{} (Global)", global.details.name()));
        assert_eq!(error.form_message(), Some(crate::Text::LibraryNameClash));
        let renamed = app
            .bring_persona_into_project(global.id, space, Some(suggestion))
            .unwrap();
        assert_eq!(renamed.details.name(), suggestion);

        // A project's narrator is not brought again.
        assert!(matches!(
            app.bring_persona_into_project(copy.id, space, Some("Other")),
            Err(PersonaError::NotGlobal)
        ));
        // Editing the copy leaves the global one.
        app.update_persona(
            copy.id,
            PersonaDraft {
                tone: "Quieter.".into(),
                ..PersonaDraft::from(&copy.details)
            },
            false,
        )
        .unwrap();
        assert_eq!(
            app.personas.get(global.id).unwrap().unwrap().details,
            global.details
        );
    }

    #[test]
    fn editing_a_global_narrator_lists_its_uses_in_every_project() {
        let app = start();
        let global = app.narrators(None).unwrap().remove(0);
        project(&app, "Space");
        let space_channel = channel(&app, "Space Archives", None);
        let apollo = video(&app, space_channel.id, "Apollo");
        app.set_project_persona(apollo.id, Some(global.id)).unwrap();
        project(&app, "Sea");
        channel(&app, "Deep Sea", Some(&global));

        let mut uses = app.persona_usage(global.id).unwrap();
        uses.sort_by(|a, b| a.project.cmp(&b.project));
        assert_eq!(
            uses,
            [
                LibraryUse {
                    project: "Sea".into(),
                    channel: "Deep Sea".into(),
                    video: None,
                },
                LibraryUse {
                    project: "Space".into(),
                    channel: "Space Archives".into(),
                    video: Some(apollo.title.clone()),
                },
            ]
        );
    }

    #[test]
    fn a_projects_products_are_its_own_and_the_global_ones_its_ads_use() {
        let app = start();
        let global = app
            .create_product(crate::ad_angles::testing::draft())
            .unwrap();
        let space = project(&app, "Space");
        let own = app
            .create_product_in(
                Some(space),
                bardo_domain::ProductDraft {
                    name: "Lamp".into(),
                    ..crate::ad_angles::testing::draft()
                },
            )
            .unwrap();
        assert_eq!(
            product_names(&app.library_products(Some(space)).unwrap()),
            ["Lamp"]
        );
        assert_eq!(
            product_names(&app.products().unwrap()),
            ["Lamp", "Notebook"],
            "an ad may pick the project's own and global ones"
        );
        assert_eq!(
            product_names(&app.library_products(None).unwrap()),
            ["Notebook"]
        );

        // Same name in the project is free; twice is not.
        let same = app
            .create_product_in(Some(space), crate::ad_angles::testing::draft())
            .unwrap();
        assert!(matches!(
            app.create_product_in(Some(space), crate::ad_angles::testing::draft()),
            Err(ProductError::NameTaken)
        ));
        assert_eq!(
            app.duplicate_product(own.id).unwrap().owner_project,
            Some(space)
        );

        // Bringing the global one in clashes with the project's Notebook.
        let error = app
            .bring_product_into_project(global.id, space, None)
            .unwrap_err();
        let ProductError::NameClash { suggestion } = error else {
            panic!("expected a clash, got {error:?}");
        };
        assert_eq!(suggestion, "Notebook (Global)");
        let copy = app
            .bring_product_into_project(global.id, space, Some(&suggestion))
            .unwrap();
        assert_eq!(copy.owner_project, Some(space));
        assert_ne!(copy.id, same.id);

        let sea = project(&app, "Sea");
        assert!(app.library_products(Some(sea)).unwrap().is_empty());
        assert!(
            !product_names(&app.products().unwrap()).contains(&"Lamp"),
            "another project's product is not offered"
        );
    }

    #[test]
    fn editing_a_used_global_product_shows_its_videos_before_saving() {
        let app = start();
        let global = app
            .create_product(crate::ad_angles::testing::draft())
            .unwrap();
        project(&app, "Space");
        let channel = channel(&app, "Space Archives", None);
        let ad = video_of(&app, channel.id, "Notebook ad", Some(global.id));
        let changed = bardo_domain::ProductDraft {
            offer: "Two for one.".into(),
            ..crate::ad_angles::testing::draft()
        };
        let error = app
            .update_product(global.id, changed.clone(), false)
            .unwrap_err();
        let ProductError::UsedIn(uses) = error else {
            panic!("expected the uses, got {error:?}");
        };
        assert_eq!(
            uses,
            [LibraryUse {
                project: "Space".into(),
                channel: "Space Archives".into(),
                video: Some(ad.title.clone()),
            }]
        );
        let saved = app.update_product(global.id, changed, true).unwrap();
        assert_eq!(saved.brief.offer(), "Two for one.");
    }

    #[test]
    fn a_project_uses_its_own_template_once_it_makes_one() {
        let app = start();
        let space = project(&app, "Space");
        let sea = project(&app, "Sea");
        for kind in [TemplateKind::Script, TemplateKind::AdScript] {
            let global = app.current_template_in(kind, None).unwrap();
            let in_use = app.template_in_use(kind, Some(space)).unwrap();
            assert_eq!(in_use.source, TemplateSource::Global);
            assert_eq!(in_use.versions[0].id, global.id);

            let own = app.make_project_template(kind, space).unwrap();
            assert_eq!(own.number, 1);
            assert_eq!(own.body, global.body, "it starts as the latest global text");
            assert!(matches!(
                app.make_project_template(kind, space),
                Err(TemplateError::AlreadyOwn)
            ));
            let in_use = app.template_in_use(kind, Some(space)).unwrap();
            assert_eq!(in_use.source, TemplateSource::Project);
            assert_eq!(
                app.current_template_in(kind, Some(space)).unwrap().id,
                own.id
            );
            assert_eq!(
                app.current_template_in(kind, Some(sea)).unwrap().id,
                global.id,
                "another project keeps the global one"
            );

            // Saving the project's own adds its next version and leaves the
            // global one.
            let saved = app
                .save_template_in(
                    kind,
                    Some(space),
                    (own.body.instructions(), "Own prompt."),
                    false,
                )
                .unwrap();
            assert_eq!(saved.number, 2);
            assert_eq!(saved.owner_project, Some(space));
            assert_eq!(app.current_template_in(kind, None).unwrap().id, global.id);
        }
    }

    #[test]
    fn saving_the_global_template_lists_the_projects_that_use_it_first() {
        let app = start();
        let space = project(&app, "Space");
        project(&app, "Sea");
        app.make_project_template(TemplateKind::Script, space)
            .unwrap();
        let global = app.current_template_in(TemplateKind::Script, None).unwrap();
        let error = app
            .save_template_in(
                TemplateKind::Script,
                None,
                (global.body.instructions(), "Changed."),
                false,
            )
            .unwrap_err();
        let TemplateError::UsedByProjects(projects) = error else {
            panic!("expected the projects, got {error:?}");
        };
        assert_eq!(projects, ["Sea"], "Space has its own");
        let saved = app
            .save_template_in(
                TemplateKind::Script,
                None,
                (global.body.instructions(), "Changed."),
                true,
            )
            .unwrap();
        assert_eq!(saved.number, global.number + 1);
        assert_eq!(saved.owner_project, None);
    }

    #[test]
    fn a_videos_generations_use_its_own_projects_template_whatever_is_open() {
        let app = start();
        let space = project(&app, "Space");
        let channel = channel(&app, "Space Archives", None);
        let own = app
            .make_project_template(TemplateKind::Thumbnail, space)
            .unwrap();
        project(&app, "Sea");
        assert_eq!(
            app.channel_template(TemplateKind::Thumbnail, channel.id)
                .unwrap()
                .id,
            own.id
        );
        assert_ne!(
            app.current_template(TemplateKind::Thumbnail).unwrap().id,
            own.id,
            "the open project uses the global one"
        );
    }

    #[test]
    fn a_channel_using_its_projects_items_does_not_move() {
        let app = start();
        let space = project(&app, "Space");
        let own = app
            .create_persona_in(Some(space), narrator_draft(&app, "Ana"))
            .unwrap();
        let channel = channel(&app, "Space Archives", Some(&own));
        let sea = project(&app, "Sea");
        assert_eq!(
            app.move_channel_blocks(channel.id).unwrap(),
            [MoveBlock::ProjectItems]
        );
        assert!(matches!(
            app.move_channel(channel.id, sea),
            Err(ProjectError::MoveBlocked(_))
        ));
        let global = app.narrators(None).unwrap().remove(0);
        app.update_channel(
            channel.id,
            ChannelDraft {
                default_persona: Some(global.id),
                ..ChannelDraft::from(&channel.details)
            },
        )
        .unwrap();
        assert!(app.move_channel_blocks(channel.id).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_project_deletes_its_own_items_and_leaves_global_ones() {
        let mut app = start();
        let space = project(&app, "Space");
        app.create_persona_in(Some(space), narrator_draft(&app, "Ana"))
            .unwrap();
        app.create_product_in(Some(space), crate::ad_angles::testing::draft())
            .unwrap();
        app.make_project_template(TemplateKind::Script, space)
            .unwrap();
        let global = app.narrators(None).unwrap().remove(0);
        channel(&app, "Space Archives", Some(&global));
        app.deletion(Removable::Project(space), "Space", true)
            .unwrap()
            .run()
            .unwrap();

        assert_eq!(app.all_personas().unwrap().len(), 4, "only the global ones");
        assert!(app.all_products().unwrap().is_empty());
        assert!(
            app.templates
                .projects_with_template(app.profile().id, TemplateKind::Script)
                .unwrap()
                .is_empty()
        );
        assert!(app.personas.get(global.id).unwrap().is_some());
    }

    #[test]
    fn a_global_item_duplicated_on_a_projects_screen_goes_to_that_project() {
        let app = start();
        let space = project(&app, "Space");
        let sea = project(&app, "Sea");
        let global = app.narrators(None).unwrap().remove(0);
        let copy = app.duplicate_persona_into(global.id, Some(space)).unwrap();
        assert_eq!(copy.owner_project, Some(space));
        assert!(names(&app.narrators(Some(space)).unwrap()).contains(&copy.details.name()));
        assert_eq!(
            app.narrators(None).unwrap().len(),
            4,
            "the global ones stay"
        );
        assert!(matches!(
            app.duplicate_persona_into(copy.id, Some(sea)),
            Err(PersonaError::OtherProject)
        ));
        assert!(matches!(
            app.duplicate_persona_into(copy.id, None),
            Err(PersonaError::OtherProject)
        ));

        let product = app
            .create_product_in(None, crate::ad_angles::testing::draft())
            .unwrap();
        let copy = app.duplicate_product_into(product.id, Some(space)).unwrap();
        assert_eq!(copy.owner_project, Some(space));
        assert!(app.duplicate_product_into(copy.id, Some(sea)).is_err());
        assert_eq!(app.library_products(None).unwrap().len(), 1);
    }
}
