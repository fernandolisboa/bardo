//! Generated music (#169, ADR-0015): Bardo makes the video's music from
//! its music prompt (`crate::MusicPrompt`). A music provider (ElevenLabs
//! Music) composes an instrumental track as long as the narration; the
//! track arrives for review, and the one the user chooses plays on the
//! music track from the start of the cut.
//!
//! Each track is also a media asset of the project (`crate::MediaAsset`,
//! source `Generated`), so the editor plays, proxies and renders it like
//! an imported file. A track in review is not in the media bin yet: only
//! a chosen one can be placed by hand.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{
    ApiKey, JobId, MediaAsset, MediaAssetId, ProfileId, Provider, ProviderFailure, RepositoryError,
    VideoProjectId,
};

/// The shortest track a provider composes.
pub const MIN_MUSIC_LENGTH: Duration = Duration::from_secs(3);
/// The longest track a provider composes in one go.
pub const MAX_MUSIC_LENGTH: Duration = Duration::from_secs(600);

/// The length of the track for a narration of `narration`: as long as it,
/// within what a provider composes. Whole milliseconds, as providers take.
pub fn music_length(narration: Duration) -> Duration {
    let millis = narration.as_millis().min(u128::from(u64::MAX)) as u64;
    Duration::from_millis(millis).clamp(MIN_MUSIC_LENGTH, MAX_MUSIC_LENGTH)
}

/// What to compose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicRequest {
    /// The music prompt as the user left it.
    pub prompt: String,
    /// Within [`MIN_MUSIC_LENGTH`] and [`MAX_MUSIC_LENGTH`].
    pub length: Duration,
}

/// The composed track: always instrumental, since the narration plays
/// over it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedMusic {
    /// MP3 audio.
    pub audio: Vec<u8>,
    /// The model that composed it, as the provider names it.
    pub model: String,
}

/// Composes music. Calls the network and blocks, so it runs inside a job.
pub trait MusicGenerator: Send + Sync {
    /// The provider whose key it needs and whose prices it is billed at.
    fn provider(&self) -> Provider;

    fn compose(
        &self,
        key: &ApiKey,
        request: &MusicRequest,
    ) -> Result<ComposedMusic, ProviderFailure>;
}

impl<T: MusicGenerator + ?Sized> MusicGenerator for Arc<T> {
    fn provider(&self) -> Provider {
        (**self).provider()
    }

    fn compose(
        &self,
        key: &ApiKey,
        request: &MusicRequest,
    ) -> Result<ComposedMusic, ProviderFailure> {
        (**self).compose(key, request)
    }
}

/// A track Bardo made for a project, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicTrack {
    /// The media asset it is; its file is the track's audio.
    pub asset: MediaAssetId,
    pub project: VideoProjectId,
    pub owner: ProfileId,
    pub provider: Provider,
    pub model: String,
    /// The prompt it was composed from.
    pub prompt: String,
    /// The length asked for.
    pub length: Duration,
    /// The job that made it.
    pub job: Option<JobId>,
    pub generated_at: SystemTime,
    /// When the user chose it for the cut; `None` while it waits for
    /// review.
    pub chosen_at: Option<SystemTime>,
}

impl MusicTrack {
    pub fn in_review(&self) -> bool {
        self.chosen_at.is_none()
    }
}

/// A project's tracks, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MusicTracks(Vec<MusicTrack>);

impl MusicTracks {
    pub fn new(tracks: Vec<MusicTrack>) -> Self {
        Self(tracks)
    }

    pub fn all(&self) -> &[MusicTrack] {
        &self.0
    }

    /// The track the cut plays: the one chosen last.
    pub fn in_use(&self) -> Option<&MusicTrack> {
        self.0
            .iter()
            .filter(|track| track.chosen_at.is_some())
            .max_by_key(|track| track.chosen_at)
    }

    /// The newest track waiting for review.
    pub fn in_review(&self) -> Option<&MusicTrack> {
        self.0
            .iter()
            .filter(|track| track.in_review())
            .max_by_key(|track| track.generated_at)
    }

    /// The track `job` made.
    pub fn made_by(&self, job: JobId) -> Option<&MusicTrack> {
        self.0.iter().find(|track| track.job == Some(job))
    }

    pub fn get(&self, asset: MediaAssetId) -> Option<&MusicTrack> {
        self.0.iter().find(|track| track.asset == asset)
    }

    /// Whether `asset` is a track still waiting for review, which stays out
    /// of the media bin.
    pub fn holds_back(&self, asset: MediaAssetId) -> bool {
        self.get(asset).is_some_and(MusicTrack::in_review)
    }
}

/// Persistence port for generated tracks. A track and its media asset are
/// saved and removed together.
pub trait MusicTrackRepository: Send + Sync {
    fn music_tracks(&self, project: VideoProjectId) -> Result<MusicTracks, RepositoryError>;

    /// Saves `track` with `asset`, the media asset it is; all or none.
    fn add_music_track(
        &self,
        track: &MusicTrack,
        asset: &MediaAsset,
    ) -> Result<(), RepositoryError>;

    /// Marks the track `asset` as chosen at `at`.
    fn choose_music_track(
        &self,
        asset: MediaAssetId,
        at: SystemTime,
    ) -> Result<(), RepositoryError>;

    /// Removes the track `asset` and its media asset; all or none.
    fn remove_music_track(&self, asset: MediaAssetId) -> Result<(), RepositoryError>;
}

impl<T: MusicTrackRepository + ?Sized> MusicTrackRepository for Arc<T> {
    fn music_tracks(&self, project: VideoProjectId) -> Result<MusicTracks, RepositoryError> {
        (**self).music_tracks(project)
    }

    fn add_music_track(
        &self,
        track: &MusicTrack,
        asset: &MediaAsset,
    ) -> Result<(), RepositoryError> {
        (**self).add_music_track(track, asset)
    }

    fn choose_music_track(
        &self,
        asset: MediaAssetId,
        at: SystemTime,
    ) -> Result<(), RepositoryError> {
        (**self).choose_music_track(asset, at)
    }

    fn remove_music_track(&self, asset: MediaAssetId) -> Result<(), RepositoryError> {
        (**self).remove_music_track(asset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(generated: u64, chosen: Option<u64>) -> MusicTrack {
        let at = |secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        MusicTrack {
            asset: MediaAssetId::new(),
            project: VideoProjectId::new(),
            owner: ProfileId::new(),
            provider: Provider::ElevenLabs,
            model: "music_v1".into(),
            prompt: "Calm piano".into(),
            length: Duration::from_secs(60),
            job: Some(JobId::new()),
            generated_at: at(generated),
            chosen_at: chosen.map(at),
        }
    }

    #[test]
    fn the_track_is_as_long_as_the_narration_within_the_provider_s_limits() {
        assert_eq!(
            music_length(Duration::from_millis(83_456_789)),
            MAX_MUSIC_LENGTH
        );
        assert_eq!(music_length(Duration::from_millis(1_500)), MIN_MUSIC_LENGTH);
        assert_eq!(
            music_length(Duration::from_nanos(95_123_999_999)),
            Duration::from_millis(95_123)
        );
    }

    #[test]
    fn the_cut_plays_the_track_chosen_last_and_the_newest_new_one_waits() {
        let first = track(10, Some(20));
        let second = track(30, Some(40));
        let older_review = track(50, None);
        let review = track(60, None);
        let tracks = MusicTracks::new(vec![
            second.clone(),
            first.clone(),
            review.clone(),
            older_review.clone(),
        ]);
        assert_eq!(tracks.in_use(), Some(&second));
        assert_eq!(tracks.in_review(), Some(&review));
        assert!(tracks.holds_back(review.asset));
        assert!(!tracks.holds_back(first.asset));
        assert!(!tracks.holds_back(MediaAssetId::new()));
        assert_eq!(tracks.made_by(review.job.unwrap()), Some(&review));

        let none = MusicTracks::default();
        assert_eq!(none.in_use(), None);
        assert_eq!(none.in_review(), None);
    }
}
