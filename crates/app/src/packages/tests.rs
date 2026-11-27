use std::path::Path;
use std::time::SystemTime;

use bardo_domain::{
    ChannelDraft, JobState, Project, ProjectColor, ProjectDraft, VideoProject, WorldItemDraft,
    WorldKind,
};
use bardo_storage::{Database, LocalProjectFiles, LocalProjectFolders, MemorySecretStore};

use super::*;
use crate::testing::arriving::ended;
use crate::{Removable, Repositories, testing};

/// Bardo with its media in real folders under `dir`.
fn on_disk(dir: &Path) -> Bardo {
    let db = Arc::new(Database::open_in_memory().unwrap());
    let files = Arc::new(LocalProjectFiles::with_folders(
        dir.join("old"),
        Arc::clone(&db) as _,
    ));
    let repositories = Repositories {
        project_folders: Arc::new(LocalProjectFolders::new()),
        places: FolderPlaces {
            documents: Some(dir.join("Documents")),
            home: dir.to_owned(),
            synced: Vec::new(),
            data_home: "Bardo".into(),
            old_media: dir.join("old"),
        },
        ..Repositories::shared_with_files(db, Arc::new(MemorySecretStore::default()), files)
    };
    Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap()
}

fn project(app: &Bardo, name: &str) -> Project {
    app.create_project(ProjectDraft {
        name: name.into(),
        color: ProjectColor::Teal,
    })
    .unwrap()
}

/// A project with a channel, a video whose folder holds a file, and a
/// place in its world.
fn filled(app: &Bardo, name: &str) -> (Project, VideoProject) {
    let project = project(app, name);
    let channel = app
        .create_channel(ChannelDraft {
            name: format!("{name} channel"),
            ..ChannelDraft::default()
        })
        .unwrap();
    let mut theme = bardo_domain::Theme::suggested(
        app.profile().id,
        channel.id,
        bardo_domain::Niche::new("baking").unwrap(),
        bardo_domain::ThemeIdea::new("Sourdough", "The starter.").unwrap(),
        SystemTime::now(),
        0,
        None,
    );
    app.themes
        .save_themes(std::slice::from_ref(&theme))
        .unwrap();
    let video = theme.approve(SystemTime::now()).unwrap();
    app.themes.start_project(&theme, &video).unwrap();
    let folder = app.video_folder(video.id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("script.json"), b"{\"lines\":[]}").unwrap();
    app.create_world_item(
        Some(project.id),
        WorldKind::Place,
        WorldItemDraft {
            name: "The bakery".into(),
            appearance: "A small bakery with a blue door".into(),
            ..WorldItemDraft::default()
        },
    )
    .unwrap();
    (project, video)
}

fn exported(app: &Bardo, project: ProjectId, to: &Path) {
    let job = app.export_project_package(project, to.to_owned()).unwrap();
    assert_eq!(ended(app, job).state(), JobState::Done);
    assert!(to.is_file());
}

/// Imports `package` under the name offered, and waits for the job.
fn imported(app: &Bardo, package: &Path) -> ProjectId {
    let offer = app.inspect_package(package).unwrap();
    let before: Vec<_> = app.jobs().iter().map(|job| job.id()).collect();
    let project = app
        .import_package(PackageChoice {
            id: ProjectId::new(),
            package: package.to_owned(),
            name: offer.name,
            color: offer.manifest.color,
            place: None,
            templates: Vec::new(),
        })
        .unwrap();
    let job = app
        .jobs()
        .into_iter()
        .find(|job| !before.contains(&job.id()))
        .unwrap();
    assert_eq!(ended(app, job.id()).state(), JobState::Done);
    project
}

fn videos(app: &Bardo, project: ProjectId) -> Vec<VideoProject> {
    app.project_channels(project)
        .unwrap()
        .iter()
        .flat_map(|channel| app.video_projects(channel.id).unwrap())
        .collect()
}

#[test]
fn an_exported_project_opens_on_another_profile_with_its_media() {
    let dir = tempfile::tempdir().unwrap();
    let from = on_disk(&dir.path().join("a"));
    let (bakery, video) = filled(&from, "Bakery");
    let package = dir.path().join(bardo_domain::package_file_name("Bakery"));
    exported(&from, bakery.id, &package);

    let to = on_disk(&dir.path().join("b"));
    let offer = to.inspect_package(&package).unwrap();
    assert_eq!(offer.name, "Bakery");
    assert_eq!(offer.manifest.contents.videos, 1);
    assert_eq!(offer.manifest.contents.world_items, 1);
    let id = imported(&to, &package);

    let project = to.projects().unwrap().into_iter().find(|p| p.id == id);
    assert_eq!(project.unwrap().name(), "Bakery");
    let came = videos(&to, id);
    assert_eq!(came.len(), 1);
    assert_ne!(came[0].id, video.id, "every record gets a new id");
    assert_eq!(came[0].title, "Sourdough");
    let script = to.video_folder(came[0].id).join("script.json");
    assert_eq!(std::fs::read(script).unwrap(), b"{\"lines\":[]}");
    let world = to
        .world_book
        .world
        .characters(to.profile().id, Some(id))
        .unwrap();
    assert_eq!(world.len(), 1);
    assert_eq!(world[0].name(), "The bakery");
}

#[test]
fn importing_into_the_same_profile_makes_a_second_project() {
    let dir = tempfile::tempdir().unwrap();
    let app = on_disk(dir.path());
    let (bakery, video) = filled(&app, "Bakery");
    let package = dir.path().join("bakery.bardo-project");
    exported(&app, bakery.id, &package);

    let offer = app.inspect_package(&package).unwrap();
    assert_eq!(offer.name, "Bakery (2)");
    let copy = imported(&app, &package);
    assert_ne!(copy, bakery.id);
    let names: Vec<_> = app
        .projects()
        .unwrap()
        .iter()
        .map(|p| p.name().to_owned())
        .collect();
    assert!(names.contains(&"Bakery".to_owned()));
    assert!(names.contains(&"Bakery (2)".to_owned()));
    assert_eq!(videos(&app, bakery.id)[0].id, video.id, "the first stays");
    assert_ne!(videos(&app, copy)[0].id, video.id);
}

#[test]
fn a_taken_name_or_a_package_that_will_not_read_is_refused_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let app = on_disk(dir.path());
    let (bakery, _) = filled(&app, "Bakery");
    let package = dir.path().join("bakery.bardo-project");
    exported(&app, bakery.id, &package);

    let taken = app.import_package(PackageChoice {
        id: ProjectId::new(),
        package: package.clone(),
        name: "bakery".into(),
        color: ProjectColor::Teal,
        place: None,
        templates: Vec::new(),
    });
    assert!(matches!(
        taken,
        Err(PackageProblem::Project(ProjectError::NameTaken))
    ));

    let junk = dir.path().join("junk.bardo-project");
    std::fs::write(&junk, b"not a zip").unwrap();
    let error = app.inspect_package(&junk).unwrap_err();
    assert_eq!(error.message(), Text::PackageNotAPackage);
}

#[test]
fn a_deleted_project_restored_from_the_bin_opens_from_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = on_disk(dir.path());
    let (bakery, _) = filled(&app, "Bakery");
    let folder = app.project_folder(bakery.id).unwrap();
    app.deletion(Removable::Project(bakery.id), "Bakery", false)
        .unwrap()
        .run()
        .unwrap();
    assert!(!app.projects().unwrap().iter().any(|p| p.id == bakery.id));
    // The bin here is in memory: the folder stays where it was, as it
    // does once restored.
    assert!(folder.join(bardo_domain::PROJECT_RECORDS).is_file());

    let opened = app.open_project_folder(&folder).unwrap();
    let FolderOpened::Imported(back) = opened else {
        panic!("{opened:?}");
    };
    assert_eq!(back.name, "Bakery");
    assert_eq!(videos(&app, back.project).len(), 1);
    assert!(!folder.join(bardo_domain::PROJECT_RECORDS).exists());
    assert_eq!(app.project_folder(back.project).as_deref(), Some(&*folder));
}

#[test]
fn a_moved_known_project_is_relinked_and_a_stray_folder_refused() {
    let dir = tempfile::tempdir().unwrap();
    let app = on_disk(dir.path());
    let (bakery, _) = filled(&app, "Bakery");
    let folder = app.project_folder(bakery.id).unwrap();
    let moved = dir.path().join("Moved");
    std::fs::rename(&folder, &moved).unwrap();

    let opened = app
        .open_project_folder(&moved.join(PROJECT_MANIFEST))
        .unwrap();
    assert_eq!(opened, FolderOpened::Relinked(bakery.id));
    assert_eq!(app.project_folder(bakery.id).as_deref(), Some(&*moved));

    let stray = dir.path().join("Stray");
    std::fs::create_dir_all(&stray).unwrap();
    let error = app.open_project_folder(&stray).unwrap_err();
    assert!(matches!(error, OpenFolderError::NotAProjectFolder));
    assert_eq!(error.message(), Text::OpenFolderNotAProject);

    let other = on_disk(&dir.path().join("other"));
    let error = other.open_project_folder(&moved).unwrap_err();
    assert!(matches!(error, OpenFolderError::OtherCopy));
}

#[test]
fn a_second_export_of_the_same_project_waits_for_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let app = on_disk(dir.path());
    let (bakery, _) = filled(&app, "Bakery");
    app.jobs.pause(true);
    app.export_project_package(bakery.id, dir.path().join("one.bardo-project"))
        .unwrap();
    assert!(app.project_export_job(bakery.id).is_some());
    let busy = app.export_project_package(bakery.id, dir.path().join("two.bardo-project"));
    assert!(matches!(busy, Err(PackageProblem::Busy)));
}
