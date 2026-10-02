//! Imported media (PRD story 41): the user brings their own music, sound
//! effects and footage into a video project. Each file is read by ffmpeg
//! first, so a file Bardo cannot play is refused with a clear reason, then
//! copied into the project folder under a name of its own; the original
//! is never changed. The copy is the project's asset from then on: the
//! editor builds its proxy like a generated clip's and places it on the
//! music, SFX or video track.
//!
//! Reading and copying a large file takes a while, so the editor runs it
//! off the UI thread with a `MediaImport`, not as a job: nothing is paid
//! and nothing needs resuming.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    AssetSource, MediaAsset, MediaAssetId, MediaAssetRepository, MediaKind, PictureSize, ProfileId,
    ProjectFiles, RepositoryError, VideoProjectId, frame_time,
};
use bardo_media::MediaEngine;
use bardo_media::ffmpeg::MediaError;

use crate::{Bardo, Text};

#[derive(Debug, thiserror::Error)]
pub enum MediaImportError {
    #[error("video project not found")]
    ProjectNotFound,
    /// The file is gone or cannot be read.
    #[error("the file cannot be read")]
    Unreadable,
    /// ffmpeg cannot read it as audio or video, or it is a still image.
    #[error("not an audio or video file Bardo can play: {0}")]
    Unsupported(String),
    /// Shorter than one frame of the timeline.
    #[error("the file is too short to place")]
    TooShort,
    #[error("ffmpeg is not installed")]
    NoFfmpeg,
    /// The copy into the project folder failed; nothing was imported.
    #[error("the file could not be copied into the project: {0}")]
    NotCopied(String),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl MediaImportError {
    /// What the media bin says.
    pub fn message(&self) -> Text {
        match self {
            MediaImportError::ProjectNotFound => Text::ProjectNotFound,
            MediaImportError::Unreadable => Text::MediaImportUnreadable,
            MediaImportError::Unsupported(_) => Text::MediaImportUnsupported,
            MediaImportError::TooShort => Text::MediaImportTooShort,
            MediaImportError::NoFfmpeg => Text::EditorFfmpegMissing,
            MediaImportError::NotCopied(_) | MediaImportError::Repository(_) => {
                Text::MediaImportNotSaved
            }
        }
    }
}

/// Imports files into one video project. It holds no UI state, so the
/// editor moves it to a background thread for each file the user chose.
#[derive(Clone)]
pub struct MediaImport {
    owner: ProfileId,
    project: VideoProjectId,
    media: Arc<dyn MediaEngine>,
    files: Arc<dyn ProjectFiles>,
    assets: Arc<dyn MediaAssetRepository>,
}

impl MediaImport {
    /// Reads the file at `path`, copies it into the project folder and
    /// records it as an imported asset. Blocks while ffmpeg reads the file
    /// and while it is copied: call it off the UI thread.
    pub fn run(&self, path: &Path) -> Result<MediaAsset, MediaImportError> {
        if !path.is_file() {
            return Err(MediaImportError::Unreadable);
        }
        let info = self.media.probe(path).map_err(|error| match error {
            MediaError::NotFound { .. } => MediaImportError::NoFfmpeg,
            MediaError::Io(_) | MediaError::Spawn { .. } => MediaImportError::Unreadable,
            other => MediaImportError::Unsupported(other.to_string()),
        })?;
        let (kind, picture) = match (&info.video, &info.audio) {
            (Some(video), _) => (
                MediaKind::Video,
                Some(PictureSize::new(video.width, video.height)),
            ),
            (None, Some(_)) => (MediaKind::Audio, None),
            (None, None) => {
                return Err(MediaImportError::Unsupported(
                    "no audio or video stream".into(),
                ));
            }
        };
        if info.duration < frame_time(1) {
            return Err(MediaImportError::TooShort);
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = MediaAssetId::new();
        let file = MediaAsset::file_name(id, &name);
        self.files
            .copy_in(self.project, &file, path)
            .map_err(|error| MediaImportError::NotCopied(error.to_string()))?;
        let asset = MediaAsset {
            id,
            project: self.project,
            owner: self.owner,
            kind,
            source: AssetSource::Imported,
            file,
            name,
            duration: info.duration,
            picture,
            imported_at: whole_millis(SystemTime::now()),
        };
        if let Err(error) = self.assets.save_media_asset(&asset) {
            // Nothing refers to the copy; leave no stray file behind.
            let _ = self.files.remove(self.project, &asset.file);
            return Err(error.into());
        }
        Ok(asset)
    }
}

/// `time` as storage keeps it, to the millisecond, so the asset returned
/// is the asset listed.
fn whole_millis(time: SystemTime) -> SystemTime {
    let since = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    SystemTime::UNIX_EPOCH + Duration::from_millis(since.as_millis() as u64)
}

impl Bardo {
    fn own_media_project(&self, id: VideoProjectId) -> Result<VideoProjectId, MediaImportError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .map(|project| project.id)
            .ok_or(MediaImportError::ProjectNotFound)
    }

    /// What imports files into `project`, to run off the UI thread.
    pub fn media_import(&self, project: VideoProjectId) -> Result<MediaImport, MediaImportError> {
        Ok(MediaImport {
            owner: self.profile.id,
            project: self.own_media_project(project)?,
            media: Arc::clone(&self.media),
            files: Arc::clone(&self.files),
            assets: Arc::clone(&self.media_assets),
        })
    }

    /// The project's imported media, oldest first.
    pub fn media_assets(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<MediaAsset>, MediaImportError> {
        let project = self.own_media_project(project)?;
        Ok(self.media_assets.media_assets(project)?)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    use bardo_domain::{ChannelDraft, VideoProject};
    use bardo_media::ffmpeg::{AudioStream, MediaInfo, VideoStream};
    use bardo_storage::{Database, MemoryProjectFiles, MemorySecretStore};

    use super::*;
    use crate::editor::testing::FakeMedia;
    use crate::testing::{FakeDecisionEngine, FakeKeyChecker, FakeMarketData};
    use crate::{JobSettings, Providers, Repositories};

    struct Harness {
        db: Arc<Database>,
        files: Arc<MemoryProjectFiles>,
        media: Arc<FakeMedia>,
        dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                db: Arc::new(Database::open_in_memory().unwrap()),
                files: Arc::default(),
                media: Arc::default(),
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn start(&self) -> Bardo {
            let providers = Providers {
                key_checker: Arc::new(FakeKeyChecker::default()),
                market_data: Arc::new(FakeMarketData::default()),
                text: Arc::new(crate::testing::FakeTextGenerator::default()),
                decisions: Arc::new(FakeDecisionEngine::default()),
                voices: Arc::new(crate::testing::FakeVoiceLibrary::default()),
                speech: Arc::new(crate::testing::FakeSpeech::default()),
                aligner: Arc::new(crate::narration_import::testing::FakeAligner::default()),
                images: Arc::new(crate::testing::FakeImages::default()),
                clips: vec![Arc::new(crate::testing::FakeClips::default())],
                audio: Arc::new(crate::narrations::testing::FakeAudioOutput::default()),
                media: Arc::clone(&self.media) as _,
            };
            Bardo::start_with(
                Repositories::shared_with_files(
                    Arc::clone(&self.db),
                    Arc::new(MemorySecretStore::default()),
                    Arc::clone(&self.files) as _,
                ),
                providers,
                Some("en-US"),
                JobSettings::default(),
            )
            .unwrap()
        }

        /// A file outside the project with `bytes`, ffmpeg reading it as
        /// `info` when given.
        fn file(&self, name: &str, bytes: &[u8], info: Option<MediaInfo>) -> PathBuf {
            let path = self.dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            if let Some(info) = info {
                self.media.probes.lock().unwrap().insert(name.into(), info);
            }
            path
        }
    }

    fn song(seconds: u64) -> MediaInfo {
        MediaInfo {
            duration: Duration::from_secs(seconds),
            video: None,
            audio: Some(AudioStream {
                codec: "mp3".into(),
                sample_rate: 44_100,
                channels: 2,
            }),
        }
    }

    fn footage(width: u32, height: u32) -> MediaInfo {
        MediaInfo {
            duration: Duration::from_millis(7_500),
            video: Some(VideoStream {
                codec: "h264".into(),
                width,
                height,
                frame_rate: (30, 1),
            }),
            audio: Some(AudioStream {
                codec: "aac".into(),
                sample_rate: 48_000,
                channels: 2,
            }),
        }
    }

    fn project(app: &Bardo) -> VideoProject {
        let channel = app
            .create_channel(ChannelDraft {
                name: "Space Archives".into(),
                ..ChannelDraft::default()
            })
            .unwrap();
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel.id,
            bardo_domain::Niche::new("space history").unwrap(),
            bardo_domain::ThemeIdea::new("The lost probe", "").unwrap(),
            SystemTime::now(),
            0,
            None,
        );
        app.themes
            .save_themes(std::slice::from_ref(&theme))
            .unwrap();
        let project = theme.approve(SystemTime::now()).unwrap();
        app.themes.start_project(&theme, &project).unwrap();
        project
    }

    #[test]
    fn imported_audio_is_copied_in_and_listed_as_an_imported_asset() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let source = h.file("Night Drive.MP3", b"mp3 bytes", Some(song(95)));

        let asset = app.media_import(project.id).unwrap().run(&source).unwrap();
        assert_eq!(asset.kind, MediaKind::Audio);
        assert_eq!(asset.source, AssetSource::Imported);
        assert_eq!(asset.name, "Night Drive.MP3");
        assert_eq!(asset.file, format!("media-{}.mp3", asset.id));
        assert_eq!(asset.duration, Duration::from_secs(95));
        assert_eq!(asset.picture, None);
        assert_eq!(asset.project, project.id);
        // The copy is in the project folder; the original is as it was.
        assert_eq!(h.files.read(project.id, &asset.file).unwrap(), b"mp3 bytes");
        assert_eq!(std::fs::read(&source).unwrap(), b"mp3 bytes");
        assert_eq!(app.media_assets(project.id).unwrap(), [asset]);
    }

    #[test]
    fn imported_video_keeps_its_picture_size() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let music = h.file("bed.wav", b"wav", Some(song(30)));
        let clip = h.file("drone shot.mov", b"mov", Some(footage(1080, 1920)));
        let import = app.media_import(project.id).unwrap();

        let first = import.run(&music).unwrap();
        let second = import.run(&clip).unwrap();
        assert_eq!(second.kind, MediaKind::Video);
        assert_eq!(second.picture, Some(PictureSize::new(1080, 1920)));
        assert_eq!(second.file, format!("media-{}.mov", second.id));
        let listed: Vec<_> = app
            .media_assets(project.id)
            .unwrap()
            .into_iter()
            .map(|asset| asset.id)
            .collect();
        assert_eq!(listed, [first.id, second.id]);
    }

    #[test]
    fn files_ffmpeg_cannot_play_are_refused_and_nothing_is_copied() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        let import = app.media_import(project.id).unwrap();

        // Not media at all: ffmpeg fails on it.
        let text = h.file("notes.mp3", b"not audio", None);
        let error = import.run(&text).unwrap_err();
        assert!(matches!(error, MediaImportError::Unsupported(_)), "{error}");
        assert_eq!(error.message(), Text::MediaImportUnsupported);

        // Media with no audio or video stream (subtitles only).
        let subtitles = MediaInfo {
            duration: Duration::from_secs(60),
            video: None,
            audio: None,
        };
        let empty = h.file("captions.mkv", b"mkv", Some(subtitles));
        assert!(matches!(
            import.run(&empty),
            Err(MediaImportError::Unsupported(_))
        ));

        // A blip shorter than a frame.
        let mut blip = song(0);
        blip.duration = Duration::from_millis(10);
        let short = h.file("click.wav", b"wav", Some(blip));
        let error = import.run(&short).unwrap_err();
        assert!(matches!(error, MediaImportError::TooShort));
        assert_eq!(error.message(), Text::MediaImportTooShort);

        // A file that is gone.
        let error = import.run(&h.dir.path().join("gone.mp3")).unwrap_err();
        assert!(matches!(error, MediaImportError::Unreadable));
        assert_eq!(error.message(), Text::MediaImportUnreadable);

        assert_eq!(app.media_assets(project.id).unwrap(), []);
        assert!(h.files.names(project.id).is_empty());
    }

    #[test]
    fn importing_without_ffmpeg_says_so() {
        let h = Harness::new();
        let app = h.start();
        let project = project(&app);
        h.media.not_found.store(true, Ordering::SeqCst);
        let source = h.file("song.mp3", b"mp3", Some(song(10)));
        let error = app
            .media_import(project.id)
            .unwrap()
            .run(&source)
            .unwrap_err();
        assert!(matches!(error, MediaImportError::NoFfmpeg));
        assert_eq!(error.message(), Text::EditorFfmpegMissing);
    }

    #[test]
    fn other_profiles_cannot_import_into_a_project() {
        let app = Harness::new().start();
        assert!(matches!(
            app.media_import(VideoProjectId::new()),
            Err(MediaImportError::ProjectNotFound)
        ));
        assert!(matches!(
            app.media_assets(VideoProjectId::new()),
            Err(MediaImportError::ProjectNotFound)
        ));
    }
}
