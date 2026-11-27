use std::time::{Duration, SystemTime};

use bardo_domain::{
    Channel, ChannelDetails, ChannelDraft, ChannelRepository, Niche, ProfileId, ProfileRepository,
    Project, ProjectDetails, ProjectDraft, ProjectRepository, Theme, ThemeIdea, ThemeRepository,
    UiLanguage, UserProfile, VideoProjectId,
};

use super::*;

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

struct Seeded {
    project: ProjectId,
    channel: Channel,
    video: VideoProjectId,
}

fn profile(db: &Database) -> ProfileId {
    let profile = UserProfile::new(UiLanguage::EnUs);
    ProfileRepository::save(db, &profile).unwrap();
    profile.id
}

/// A project with a channel, a theme and its video.
fn seed(db: &Database, owner: ProfileId, name: &str) -> Seeded {
    let project = Project::new(
        owner,
        ProjectDetails::validate(ProjectDraft {
            name: name.into(),
            color: ProjectColor::Teal,
        })
        .unwrap(),
        time(1),
    );
    db.save_project(&project).unwrap();
    let channel = Channel::new(
        owner,
        project.id,
        ChannelDetails::validate(ChannelDraft {
            name: name.into(),
            ..ChannelDraft::default()
        })
        .unwrap(),
    );
    ChannelRepository::save(db, &channel).unwrap();
    let mut theme = Theme::suggested(
        owner,
        channel.id,
        Niche::new("baking").unwrap(),
        ThemeIdea::new(&format!("{name} at dawn"), "").unwrap(),
        time(1),
        0,
        None,
    );
    db.save_themes(std::slice::from_ref(&theme)).unwrap();
    let video = theme.approve(time(2)).unwrap();
    db.start_project(&theme, &video).unwrap();
    Seeded {
        project: project.id,
        channel,
        video: video.id,
    }
}

fn sql(db: &Database, statement: &str, values: &[&dyn rusqlite::ToSql]) {
    db.conn().execute(statement, values).unwrap();
}

fn count(db: &Database, statement: &str) -> i64 {
    db.conn()
        .query_row(statement, [], |row| row.get(0))
        .unwrap()
}

fn persona(db: &Database, owner: ProfileId, name: &str, project: Option<ProjectId>) -> String {
    let id = Uuid::new_v4().to_string();
    sql(
        db,
        "INSERT INTO persona (id, profile_id, name, voice_provider, voice_id, voice_name, tone,
                              script_style, stability, similarity, style, speed, owner_project_id)
         VALUES (?1, ?2, ?3, 'elevenlabs', 'v', 'Voice', '', '', 50, 75, 0, 100, ?4)",
        &[
            &id,
            &owner.to_string(),
            &name,
            &project.map(|p| p.to_string()),
        ],
    );
    id
}

fn character(db: &Database, owner: ProfileId, name: &str, project: Option<ProjectId>) -> String {
    let id = Uuid::new_v4().to_string();
    sql(
        db,
        "INSERT INTO character (id, profile_id, owner_project_id, name, appearance, created_at)
         VALUES (?1, ?2, ?3, ?4, 'tall', 0)",
        &[
            &id,
            &owner.to_string(),
            &project.map(|p| p.to_string()),
            &name,
        ],
    );
    sql(
        db,
        "INSERT INTO reference_picture (id, character_id, look, file, provider, model, prompt,
                                        origin, generated_at)
         VALUES (?1, ?2, 'cinematic', ?3, 'gemini', 'm', 'p', 'drawn', 0)",
        &[
            &Uuid::new_v4().to_string(),
            &id,
            &format!("portrait-{id}.png"),
        ],
    );
    id
}

fn pick(db: &Database, video: VideoProjectId, character: &str) {
    sql(
        db,
        "INSERT INTO video_cast_pick (project_id, character_id, position) VALUES (?1, ?2, 0)",
        &[&video.to_string(), &character],
    );
}

/// A connected account with a post (published), a post never made and a
/// cost record.
fn publishing(db: &Database, owner: ProfileId, seeded: &Seeded, post: &str) -> String {
    let account = Uuid::new_v4().to_string();
    let owner = owner.to_string();
    sql(
        db,
        "INSERT INTO network_account (id, profile_id, channel_id, network, handle,
                                      description_footer, visibility)
         VALUES (?1, ?2, ?3, 'youtube', '@bakery', '', 'public')",
        &[&account, &owner, &seeded.channel.id.to_string()],
    );
    sql(
        db,
        "INSERT INTO network_connection (account_id, profile_id, status, identity_id,
                                         identity_name, scopes, expires_at, connected_at)
         VALUES (?1, ?2, 'connected', 'UC1', 'Bakery', 'upload', 0, 0)",
        &[&account, &owner],
    );
    for (network, post_id, status) in [
        ("youtube", Some(post), "published"),
        ("tiktok", None, "queued"),
    ] {
        let id = Uuid::new_v4().to_string();
        sql(
            db,
            "INSERT INTO publication (id, project_id, network, profile_id, account_id, render_id,
                                      kind, post_id, url, upload_status, upload_visibility,
                                      upload_job, posted_at, linked_at)
             VALUES (?1, ?2, ?8, ?3, ?4, ?5, 'uploaded', ?6, ?6, ?7, 'public', ?5, 0, 0)",
            &[
                &id,
                &seeded.video.to_string(),
                &owner,
                &account,
                &Uuid::new_v4().to_string(),
                &post_id,
                &status,
                &network,
            ],
        );
        sql(
            db,
            "INSERT INTO metrics_snapshot (publication_id, taken_at, views) VALUES (?1, 0, 9)",
            &[&id],
        );
    }
    sql(
        db,
        "INSERT INTO cost_record (id, profile_id, provider, model, purpose, input_tokens,
                                  output_tokens, image_tokens, characters, basis,
                                  amount_micros, at, owner_project_id)
         VALUES (?1, ?2, 'anthropic', 'm', 'script', 0, 0, 0, 0, 'reported', 5, 0, ?3)",
        &[
            &Uuid::new_v4().to_string(),
            &owner,
            &seeded.project.to_string(),
        ],
    );
    account
}

/// A project with a global narrator, a global character picked for its
/// video, a world character of its own and a post; beside it another
/// project that must not travel.
struct Source {
    db: Arc<Database>,
    owner: ProfileId,
    seeded: Seeded,
    other: Seeded,
    global_persona: String,
    own_character: String,
    global_character: String,
    dir: tempfile::TempDir,
}

fn source() -> Source {
    let db = Arc::new(Database::open_in_memory().unwrap());
    let owner = profile(&db);
    let seeded = seed(&db, owner, "Bakery");
    let other = seed(&db, owner, "Garage");
    let global_persona = persona(&db, owner, "Ana", None);
    persona(&db, owner, "Unused", None);
    persona(&db, owner, "Other's", Some(other.project));
    sql(
        &db,
        "UPDATE video_project SET persona_id = ?1 WHERE id = ?2",
        &[&global_persona, &seeded.video.to_string()],
    );
    let own_character = character(&db, owner, "Rosa", Some(seeded.project));
    let global_character = character(&db, owner, "Max", None);
    character(&db, owner, "Gus", Some(other.project));
    pick(&db, seeded.video, &own_character);
    pick(&db, seeded.video, &global_character);
    publishing(&db, owner, &seeded, "post-1");
    publishing(&db, owner, &other, "post-2");
    sql(
        &db,
        "INSERT INTO job (id, profile_id, kind, payload, state, progress, attempts, owner_project_id)
         VALUES (?1, ?2, 'script_generation', ?3, ?4, 1000, 1, ?5)",
        &[
            &Uuid::new_v4().to_string(),
            &owner.to_string(),
            &format!(r#"{{"project":"{}"}}"#, seeded.video),
            &"done",
            &seeded.project.to_string(),
        ],
    );
    sql(
        &db,
        "INSERT INTO job (id, profile_id, kind, payload, state, progress, attempts, owner_project_id)
         VALUES (?1, ?2, 'scene_images', '{}', 'failed', 0, 3, ?3)",
        &[
            &Uuid::new_v4().to_string(),
            &owner.to_string(),
            &seeded.project.to_string(),
        ],
    );
    let dir = tempfile::tempdir().unwrap();
    Source {
        db,
        owner,
        seeded,
        other,
        global_persona,
        own_character,
        global_character,
        dir,
    }
}

impl Source {
    fn folder(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// The video's folder, the project's world folder and the global one,
    /// with a file each.
    fn media(&self) -> (PathBuf, PathBuf, PathBuf) {
        let video = self.folder("Bakery/Videos/dawn-1234abcd");
        let world = self.folder("Bakery/World");
        let global = self.folder("Global/World");
        for dir in [&video, &world, &global] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(video.join("narration.mp3"), b"voice").unwrap();
        fs::write(
            video.join(format!("peaks-capture-{}.json", self.seeded.video)),
            b"peaks",
        )
        .unwrap();
        fs::write(
            world.join(format!("portrait-{}.png", self.own_character)),
            b"rosa",
        )
        .unwrap();
        fs::write(
            global.join(format!("portrait-{}.png", self.global_character)),
            b"max",
        )
        .unwrap();
        fs::write(global.join("portrait-someone-else.png"), b"no").unwrap();
        (video, world, global)
    }

    fn export(&self) -> (PathBuf, PackageManifest) {
        let (video, world, global) = self.media();
        let to = self.folder("Bakery.bardo-project");
        let packages = LocalPackages::new(Arc::clone(&self.db));
        let manifest = packages
            .export(
                &PackageExport {
                    project: self.seeded.project,
                    to: to.clone(),
                    videos: vec![(self.seeded.video, video)],
                    world: Some(world),
                    global_world: global,
                    app_version: "0.4.0".into(),
                },
                &mut |_, _| true,
            )
            .unwrap();
        (to, manifest)
    }
}

fn entries(package: &Path) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(File::open(package).unwrap()).unwrap();
    (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .unwrap()
                .name()
                .unwrap()
                .into_owned()
        })
        .collect()
}

fn records_of(package: &Path, to: &Path) -> Connection {
    let mut archive = zip::ZipArchive::new(File::open(package).unwrap()).unwrap();
    let mut entry = archive.by_name(RECORDS_ENTRY).unwrap();
    let mut out = File::create(to).unwrap();
    io::copy(&mut entry, &mut out).unwrap();
    Connection::open(to).unwrap()
}

fn one(conn: &Connection, statement: &str) -> i64 {
    conn.query_row(statement, [], |row| row.get(0)).unwrap()
}

#[test]
fn a_package_holds_the_project_and_the_global_items_it_uses_and_nothing_else() {
    let source = source();
    let (package, manifest) = source.export();
    assert_eq!(manifest.name, "Bakery");
    assert_eq!(manifest.contents.channels, 1);
    assert_eq!(manifest.contents.videos, 1);
    assert_eq!(manifest.contents.narrators, 1);
    assert_eq!(manifest.contents.world_items, 2);
    assert_eq!(manifest.contents.media_files, 4);

    let names = entries(&package);
    assert_eq!(names[0], PACKAGE_MANIFEST);
    assert!(names.contains(&format!(
        "{VIDEOS_ENTRY}/{}/narration.mp3",
        source.seeded.video
    )));
    assert!(names.contains(&format!(
        "{GLOBAL_WORLD_ENTRY}/portrait-{}.png",
        source.global_character
    )));
    assert!(!names.iter().any(|name| name.contains("someone-else")));

    let records = records_of(&package, &source.folder("records.db"));
    assert_eq!(one(&records, "SELECT COUNT(*) FROM project"), 1);
    assert_eq!(one(&records, "SELECT COUNT(*) FROM channel"), 1);
    assert_eq!(
        one(
            &records,
            "SELECT COUNT(*) FROM channel WHERE name = 'Garage'"
        ),
        0
    );
    assert_eq!(one(&records, "SELECT COUNT(*) FROM video_project"), 1);
    // The global narrator it uses, not the unused one nor another
    // project's.
    let personas: Vec<String> = records
        .prepare("SELECT id FROM persona")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(personas, vec![source.global_persona.clone()]);
    assert_eq!(one(&records, "SELECT COUNT(*) FROM reference_picture"), 2);
    // No connection, no cost, no post never made, no job but the done one,
    // and none of the YouTube numbers that expire.
    assert_eq!(one(&records, "SELECT COUNT(*) FROM network_connection"), 0);
    assert_eq!(one(&records, "SELECT COUNT(*) FROM network_account"), 1);
    assert_eq!(one(&records, "SELECT COUNT(*) FROM cost_record"), 0);
    assert_eq!(one(&records, "SELECT COUNT(*) FROM publication"), 1);
    assert_eq!(one(&records, "SELECT COUNT(*) FROM metrics_snapshot"), 0);
    assert_eq!(one(&records, "SELECT COUNT(*) FROM job"), 1);
    assert_eq!(one(&records, "SELECT COUNT(*) FROM user_profile"), 1);
    // Nothing of the other project, even in a free page.
    let bytes = fs::read(source.folder("records.db")).unwrap();
    let other = source.other.video.to_string();
    assert!(!bytes.windows(other.len()).any(|w| w == other.as_bytes()));
    assert!(!bytes.windows(6).any(|w| w == b"Garage"));
}

#[test]
fn a_package_comes_into_a_clean_profile_with_new_ids_and_its_media() {
    let source = source();
    let (package, _) = source.export();

    let db = Arc::new(Database::open_in_memory().unwrap());
    let owner = profile(&db);
    let packages = LocalPackages::new(Arc::clone(&db));
    let project = ProjectId::new();
    let folder = source.folder("Imported");
    let imported = packages
        .import(
            &PackageImport {
                package: package.clone(),
                owner,
                project,
                name: "Bakery".into(),
                color: ProjectColor::Rose,
                folder: folder.clone(),
                data_home: "Bardo".into(),
                templates: Vec::new(),
            },
            &mut |_, _| true,
        )
        .unwrap();
    assert_eq!(imported.name, "Bakery");
    assert_eq!(imported.skipped_posts, 0);

    let found = ProjectRepository::project(&*db, project).unwrap().unwrap();
    assert_eq!(found.owner, owner);
    assert_eq!(found.folder.as_deref(), Some(folder.as_path()));
    assert_eq!(found.details.color(), ProjectColor::Rose);
    let channels: Vec<Channel> = ChannelRepository::list(&*db, owner)
        .unwrap()
        .into_iter()
        .filter(|channel| channel.project == project)
        .collect();
    assert_eq!(channels.len(), 1);
    assert_ne!(channels[0].id, source.seeded.channel.id);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM video_project"), 1);
    let video: String = db
        .conn()
        .query_row("SELECT id FROM video_project", [], |row| row.get(0))
        .unwrap();
    assert_ne!(video, source.seeded.video.to_string());
    // The carried narrator and character are the project's own now.
    assert_eq!(
        count(
            &db,
            &format!("SELECT COUNT(*) FROM persona WHERE owner_project_id = '{project}'")
        ),
        1
    );
    assert_eq!(
        count(
            &db,
            &format!("SELECT COUNT(*) FROM character WHERE owner_project_id = '{project}'")
        ),
        2
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM network_connection"), 0);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM network_account"), 1);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM publication"), 1);
    // The job's payload names the new video.
    let payload: String = db
        .conn()
        .query_row("SELECT payload FROM job", [], |row| row.get(0))
        .unwrap();
    assert!(payload.contains(&video));
    assert!(!payload.contains(&source.seeded.video.to_string()));
    // The media, under the video's folder and the world, names rewritten.
    let folder_name: String = db
        .conn()
        .query_row("SELECT folder_name FROM video_project", [], |row| {
            row.get(0)
        })
        .unwrap();
    let videos = folder.join(VIDEOS_DIR).join(folder_name);
    assert!(videos.join("narration.mp3").is_file());
    assert!(videos.join(format!("peaks-capture-{video}.json")).is_file());
    assert_eq!(
        fs::read_dir(folder.join(WORLD_DIR)).unwrap().count(),
        2,
        "the project's and the carried global character's pictures"
    );
    let manifest =
        ProjectManifest::parse(&fs::read_to_string(folder.join(PROJECT_MANIFEST)).unwrap())
            .unwrap();
    assert!(manifest.names(project, owner, "Bardo"));
    // A second run of the same import finds it done.
    let again = packages
        .import(
            &PackageImport {
                package,
                owner,
                project,
                name: "Bakery".into(),
                color: ProjectColor::Rose,
                folder,
                data_home: "Bardo".into(),
                templates: Vec::new(),
            },
            &mut |_, _| true,
        )
        .unwrap();
    assert_eq!(again.project, project);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM project"), 1);
}

#[test]
fn a_package_brought_back_where_it_came_from_is_a_second_project() {
    let source = source();
    let (package, _) = source.export();
    let packages = LocalPackages::new(Arc::clone(&source.db));
    let project = ProjectId::new();
    let imported = packages
        .import(
            &PackageImport {
                package,
                owner: source.owner,
                project,
                name: "Bakery".into(),
                color: ProjectColor::Teal,
                folder: source.folder("Bakery 2"),
                data_home: "Bardo".into(),
                templates: Vec::new(),
            },
            &mut |_, _| true,
        )
        .unwrap();
    assert_eq!(imported.name, "Bakery (2)");
    assert_eq!(imported.skipped_posts, 1);
    assert_eq!(
        imported.renamed_channels,
        vec![("Bakery".to_owned(), "Bakery (Bakery (2))".to_owned())]
    );
    assert_eq!(count(&source.db, "SELECT COUNT(*) FROM project"), 3);
    assert_eq!(count(&source.db, "SELECT COUNT(*) FROM video_project"), 3);
    // No post twice.
    assert_eq!(
        count(
            &source.db,
            "SELECT COUNT(*) FROM publication WHERE post_id = 'post-1'"
        ),
        1
    );
    // The original keeps its own.
    assert_eq!(
        count(
            &source.db,
            &format!(
                "SELECT COUNT(*) FROM video_cast_pick WHERE project_id = '{}'",
                source.seeded.video
            )
        ),
        2
    );
}

#[test]
fn a_package_stopped_or_newer_leaves_nothing() {
    let source = source();
    let (video, world, global) = source.media();
    let packages = LocalPackages::new(Arc::clone(&source.db));
    let to = source.folder("Stopped.bardo-project");
    let stopped = packages.export(
        &PackageExport {
            project: source.seeded.project,
            to: to.clone(),
            videos: vec![(source.seeded.video, video)],
            world: Some(world),
            global_world: global,
            app_version: "0.4.0".into(),
        },
        &mut |_, _| false,
    );
    assert!(matches!(stopped, Err(PackageError::Stopped)));
    assert!(!to.exists());
    assert_eq!(
        fs::read_dir(source.dir.path())
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().path().is_file())
            .count(),
        0,
        "no partial file left"
    );

    // A manifest of a schema ahead of this one.
    let newer = source.folder("Newer.bardo-project");
    {
        let mut zip = zip::ZipWriter::new(File::create(&newer).unwrap());
        zip.start_file(PACKAGE_MANIFEST, zip::write::SimpleFileOptions::default())
            .unwrap();
        let manifest = PackageManifest {
            format: PACKAGE_FORMAT,
            app_version: "9.0.0".into(),
            schema: SCHEMA_VERSION as u32 + 1,
            exported_at: SystemTime::UNIX_EPOCH,
            project: ProjectId::new(),
            name: "Future".into(),
            color: ProjectColor::Amber,
            contents: PackageContents::default(),
        };
        zip.write_all(manifest.to_json().as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    assert!(matches!(packages.inspect(&newer), Err(PackageError::Newer)));
    fs::write(source.folder("plain.bardo-project"), b"not a zip").unwrap();
    assert!(matches!(
        packages.inspect(&source.folder("plain.bardo-project")),
        Err(PackageError::NotAPackage)
    ));
}

#[test]
fn a_deleted_projects_folder_opens_again_as_a_new_project() {
    let source = source();
    let (video, world, _) = source.media();
    let folder = source.folder("Bakery");
    let packages = LocalPackages::new(Arc::clone(&source.db));
    packages
        .write_records(source.seeded.project, &folder)
        .unwrap();
    let read = packages.folder_records(&folder).unwrap();
    assert_eq!(read.project, source.seeded.project);
    assert_eq!(read.name, "Bakery");
    sql(
        &source.db,
        "DELETE FROM channel WHERE id = ?1",
        &[&source.seeded.channel.id.to_string()],
    );
    sql(
        &source.db,
        "DELETE FROM project WHERE id = ?1",
        &[&source.seeded.project.to_string()],
    );
    sql(
        &source.db,
        "DELETE FROM character WHERE owner_project_id IS NULL AND name = 'Max'",
        &[],
    );
    let project = ProjectId::new();
    let imported = packages
        .import_folder(&FolderImport {
            folder: folder.clone(),
            owner: source.owner,
            project,
            name: "Bakery".into(),
            data_home: "Bardo".into(),
        })
        .unwrap();
    assert_eq!(imported.name, "Bakery");
    let found = ProjectRepository::project(&*source.db, project)
        .unwrap()
        .unwrap();
    assert_eq!(found.folder.as_deref(), Some(folder.as_path()));
    assert!(!folder.join(PROJECT_RECORDS).exists());
    let manifest =
        ProjectManifest::parse(&fs::read_to_string(folder.join(PROJECT_MANIFEST)).unwrap())
            .unwrap();
    assert!(manifest.names(project, source.owner, "Bardo"));
    // A file named after an id follows its new id; the others stay.
    let new_video: String = source
        .db
        .conn()
        .query_row(
            "SELECT id FROM video_project WHERE folder_project_id = ?1",
            [project.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(video.join("narration.mp3").is_file());
    assert!(
        video
            .join(format!("peaks-capture-{new_video}.json"))
            .is_file()
    );
    assert_eq!(fs::read_dir(&world).unwrap().count(), 1);
}

#[test]
fn global_templates_come_in_as_the_projects_own_only_when_ticked() {
    let source = source();
    let template = Uuid::new_v4().to_string();
    sql(
        &source.db,
        "INSERT INTO template_version (id, profile_id, kind, number, instructions, prompt,
                                       created_at)
         VALUES (?1, ?2, 'script', 3, 'write', 'about {{theme}}', 0)",
        &[&template, &source.owner.to_string()],
    );
    sql(
        &source.db,
        "INSERT INTO generation (id, profile_id, project_id, provider, model,
                                 template_version_id, instructions, prompt, output,
                                 input_tokens, output_tokens, generated_at)
         VALUES (?1, ?2, ?3, 'anthropic', 'm', ?4, 'write', 'about dawn', 'text', 1, 1, 0)",
        &[
            &Uuid::new_v4().to_string(),
            &source.owner.to_string(),
            &source.seeded.video.to_string(),
            &template,
        ],
    );
    let (package, manifest) = source.export();
    assert_eq!(
        manifest.contents.global_templates,
        vec![TemplateKind::Script]
    );

    let bring = |db: &Arc<Database>, owner: ProfileId, ticked: Vec<TemplateKind>, name: &str| {
        let project = ProjectId::new();
        LocalPackages::new(Arc::clone(db))
            .import(
                &PackageImport {
                    package: package.clone(),
                    owner,
                    project,
                    name: "Bakery".into(),
                    color: ProjectColor::Teal,
                    folder: source.folder(name),
                    data_home: "Bardo".into(),
                    templates: ticked,
                },
                &mut |_, _| true,
            )
            .unwrap();
        project
    };
    let used = |db: &Arc<Database>, project: ProjectId| -> (String, Option<String>, Option<i64>) {
        db.conn()
            .query_row(
                "SELECT t.id, t.owner_project_id, t.retired_at FROM generation g
                 JOIN template_version t ON t.id = g.template_version_id
                 JOIN video_project v ON v.id = g.project_id
                 WHERE v.folder_project_id = ?1",
                [project.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    };

    // A clean profile: the generation keeps its text; unticked, the
    // version comes retired, so it is no template of the project.
    let clean = Arc::new(Database::open_in_memory().unwrap());
    let owner = profile(&clean);
    let unticked = bring(&clean, owner, Vec::new(), "Clean 1");
    let (_, owned, retired) = used(&clean, unticked);
    assert_eq!(owned, Some(unticked.to_string()));
    assert!(retired.is_some());
    let ticked = bring(&clean, owner, vec![TemplateKind::Script], "Clean 2");
    let (_, owned, retired) = used(&clean, ticked);
    assert_eq!(owned, Some(ticked.to_string()));
    assert_eq!(retired, None);

    // Where it came from: unticked, the generation points at the global
    // version it was written with.
    let again = bring(&source.db, source.owner, Vec::new(), "Same");
    let (id, owned, _) = used(&source.db, again);
    assert_eq!(id, template);
    assert_eq!(owned, None);
}
