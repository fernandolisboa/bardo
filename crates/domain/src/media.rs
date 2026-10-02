//! Imported media (PRD stories 40, 41): audio and video files the user
//! brings into a video project, such as music they have the rights to, sound
//! effects or footage of their own. Each is copied into the project folder
//! (the original stays where it was) and becomes an asset of the project
//! that the editor places on the timeline: audio on the music or SFX track,
//! video on the video track.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{PictureSize, ProfileId, RepositoryError, Track, VideoProjectId};

uuid_id!(
    /// Identifies one media asset of a project.
    MediaAssetId
);

/// What an asset holds, as probing its file found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// Sound only: music, an effect, a recording.
    Audio,
    /// A moving picture, with or without sound.
    Video,
}

impl MediaKind {
    pub const ALL: [MediaKind; 2] = [MediaKind::Audio, MediaKind::Video];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
        }
    }

    /// The timeline tracks it can be placed on. A video plays its picture
    /// only, as generated clips do.
    pub fn tracks(self) -> &'static [Track] {
        match self {
            MediaKind::Audio => &[Track::Music, Track::Sfx],
            MediaKind::Video => &[Track::Video],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown media kind: {0}")]
pub struct UnknownMediaKind(pub String);

impl std::str::FromStr for MediaKind {
    type Err = UnknownMediaKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MediaKind::ALL
            .into_iter()
            .find(|kind| kind.code() == s)
            .ok_or_else(|| UnknownMediaKind(s.to_owned()))
    }
}

/// Where an asset came from. Generated music would be another source; the
/// MVP only imports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetSource {
    /// A file the user brought in.
    Imported,
}

impl AssetSource {
    pub const ALL: [AssetSource; 1] = [AssetSource::Imported];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            AssetSource::Imported => "imported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown asset source: {0}")]
pub struct UnknownAssetSource(pub String);

impl std::str::FromStr for AssetSource {
    type Err = UnknownAssetSource;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AssetSource::ALL
            .into_iter()
            .find(|source| source.code() == s)
            .ok_or_else(|| UnknownAssetSource(s.to_owned()))
    }
}

/// A media file of a project, ready to place on the timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaAsset {
    pub id: MediaAssetId,
    pub project: VideoProjectId,
    pub owner: ProfileId,
    pub kind: MediaKind,
    pub source: AssetSource,
    /// Its copy in the project folder.
    pub file: String,
    /// The file's name as the user had it.
    pub name: String,
    pub duration: Duration,
    /// The size of its picture, for a video.
    pub picture: Option<PictureSize>,
    pub imported_at: SystemTime,
}

impl MediaAsset {
    /// The name the copy of `original` gets in the project folder: the
    /// asset's id, keeping a short plain extension so players and tools
    /// still know the file. Never a path, whatever the original was named.
    pub fn file_name(id: MediaAssetId, original: &str) -> String {
        let extension = original
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .filter(|extension| {
                (1..=5).contains(&extension.len())
                    && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
            });
        match extension {
            Some(extension) => format!("media-{id}.{extension}"),
            None => format!("media-{id}"),
        }
    }

    /// Whether it can be placed on `track`.
    pub fn fits(&self, track: Track) -> bool {
        self.kind.tracks().contains(&track)
    }
}

/// Persistence port for a project's media assets.
pub trait MediaAssetRepository: Send + Sync {
    /// The project's assets, oldest first.
    fn media_assets(&self, project: VideoProjectId) -> Result<Vec<MediaAsset>, RepositoryError>;

    fn save_media_asset(&self, asset: &MediaAsset) -> Result<(), RepositoryError>;
}

impl<T: MediaAssetRepository + ?Sized> MediaAssetRepository for Arc<T> {
    fn media_assets(&self, project: VideoProjectId) -> Result<Vec<MediaAsset>, RepositoryError> {
        (**self).media_assets(project)
    }

    fn save_media_asset(&self, asset: &MediaAsset) -> Result<(), RepositoryError> {
        (**self).save_media_asset(asset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_named_after_the_asset_and_keeps_a_plain_extension() {
        let id = MediaAssetId::new();
        assert_eq!(
            MediaAsset::file_name(id, "Lo-Fi Beat (final).MP3"),
            format!("media-{id}.mp3")
        );
        assert_eq!(
            MediaAsset::file_name(id, "clip.mov"),
            format!("media-{id}.mov")
        );
        // Nothing of the original name that is not a short plain extension.
        assert_eq!(MediaAsset::file_name(id, "noise"), format!("media-{id}"));
        assert_eq!(
            MediaAsset::file_name(id, "a.b/../../evil"),
            format!("media-{id}")
        );
        assert_eq!(
            MediaAsset::file_name(id, "x.wav\\..\\y"),
            format!("media-{id}")
        );
        assert_eq!(
            MediaAsset::file_name(id, "x.toolongext"),
            format!("media-{id}")
        );
    }

    #[test]
    fn audio_goes_on_music_or_sfx_and_video_on_the_video_track() {
        assert_eq!(MediaKind::Audio.tracks(), &[Track::Music, Track::Sfx]);
        assert_eq!(MediaKind::Video.tracks(), &[Track::Video]);
        for kind in MediaKind::ALL {
            assert_eq!(kind.code().parse::<MediaKind>(), Ok(kind));
        }
        assert_eq!("imported".parse::<AssetSource>(), Ok(AssetSource::Imported));
    }
}
