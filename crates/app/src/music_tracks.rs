//! Generated music (#169, ADR-0015): Bardo makes the video's music from the
//! music prompt (`crate::music_prompts`). ElevenLabs Music composes an
//! instrumental track as long as the narration; the track waits on the
//! music card for review, and **Use this track** puts it on the music track
//! from the start of the cut, in place of the generated track used before.
//! **Discard** removes it. A new track takes the place of the one waiting
//! for review, never of the one in use.
//!
//! Composing calls ElevenLabs, so it runs as a job. Its audio is saved in
//! the project folder before the track is recorded, so a job that stops in
//! between does not pay for it again when it resumes.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, AssetSource, CostPurpose, EditError, Job, JobFailure, JobFailureKind, JobId, JobKind,
    MediaAsset, MediaAssetId, MediaKind, Metered, MusicGenerator, MusicRequest, MusicTrack,
    MusicTrackRepository, ProfileId, ProjectFiles, Provider, RepositoryError, SecretStore,
    VideoProject, VideoProjectId, music_length,
};
use bardo_media::{Playback, PlaybackError, mp3};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::editor::EditorError;
use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, KeyState, Text};

#[derive(Debug, thiserror::Error)]
pub enum MusicTrackError {
    #[error("video project not found")]
    ProjectNotFound,
    /// Music is composed from the music prompt: generate one first.
    #[error("the project has no music prompt yet")]
    NoPrompt,
    /// The track is as long as the narration: make it first.
    #[error("the project has no narration yet")]
    NoNarration,
    /// Composing calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// A track is being composed for the project.
    #[error("music is already being composed for this project")]
    Busy,
    /// The track would reach the provider's budget; the card asks before
    /// starting it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    /// No such track in the project, or (to discard) not one in review.
    #[error("no such track")]
    NotFound,
    /// The track's audio file is gone from the project folder.
    #[error("the track's audio file is missing")]
    AudioMissing,
    /// Music the user placed by hand fills the start of the music track.
    #[error("no room for the track on the music track")]
    NoRoom,
    #[error("the cut could not take the track: {0}")]
    Cut(EditorError),
    #[error(transparent)]
    Playback(#[from] PlaybackError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl MusicTrackError {
    /// What the music card says.
    pub fn message(&self) -> Text {
        match self {
            MusicTrackError::ProjectNotFound => Text::ProjectNotFound,
            MusicTrackError::NoPrompt => Text::MusicPromptMissing,
            MusicTrackError::NoNarration => Text::MusicTrackNoNarration,
            MusicTrackError::MissingKey(_) => Text::MusicTrackMissingKey,
            MusicTrackError::Busy => Text::MusicTrackBusy,
            MusicTrackError::OverBudget(_) => Text::BudgetReachedTitle,
            MusicTrackError::NotFound => Text::MusicTrackNotFound,
            MusicTrackError::AudioMissing => Text::MusicTrackAudioMissing,
            MusicTrackError::NoRoom => Text::MusicTrackNoRoom,
            MusicTrackError::Playback(_) => Text::MusicTrackCannotPlay,
            MusicTrackError::Cut(_) | MusicTrackError::Repository(_) => Text::MusicTrackNotSaved,
        }
    }
}

impl From<EditorError> for MusicTrackError {
    fn from(error: EditorError) -> Self {
        match error {
            EditorError::ProjectNotFound => MusicTrackError::ProjectNotFound,
            EditorError::Edit(EditError::Overlaps) => MusicTrackError::NoRoom,
            EditorError::Repository(error) => MusicTrackError::Repository(error),
            other => MusicTrackError::Cut(other),
        }
    }
}

/// A track Bardo made, and the media asset it is.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedTrack {
    pub track: MusicTrack,
    pub asset: MediaAsset,
}

/// A video project's generated music, on the music card.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicTracksView {
    /// The track the cut plays.
    pub in_use: Option<GeneratedTrack>,
    /// The newest track, waiting for the user to use or discard it.
    pub in_review: Option<GeneratedTrack>,
    /// The project's latest music job.
    pub job: Option<Job>,
    /// How long a new track is: the narration's length, within what the
    /// provider composes. `None` until there is a narration.
    pub length: Option<Duration>,
    /// What composing a track that long would cost; `None` without a
    /// narration.
    pub estimate: Option<SpendEstimate>,
}

/// The call that composes `length` of music: billed per second of it.
fn music_call(provider: Provider, length: Duration) -> PlannedCall {
    PlannedCall::new(provider, CostPurpose::Music, 1)
        .with_model(bardo_ai::elevenlabs::MUSIC_MODEL)
        .with_audio_seconds(billed_seconds(length))
}

fn billed_seconds(length: Duration) -> u64 {
    length.as_millis().div_ceil(1_000) as u64
}

/// The project file of track `id`'s audio.
fn track_file(id: MediaAssetId) -> String {
    MediaAsset::file_name(id, "music.mp3")
}

/// What an attempt saved next to the audio before recording the track.
fn track_record(id: MediaAssetId) -> String {
    format!("music-{id}.json")
}

/// The music job's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MusicPayload {
    project: String,
    /// The media asset the track becomes.
    track: String,
    /// The track's number in the project, for its name in the media bin.
    number: usize,
    prompt: String,
    length_ms: u64,
}

impl MusicPayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a music payload serializes")
    }

    fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid music payload: {e}")))
    }

    fn project(&self) -> Result<VideoProjectId, JobFailure> {
        uuid::Uuid::parse_str(&self.project)
            .map(VideoProjectId::from)
            .map_err(unexpected)
    }

    fn track(&self) -> Result<MediaAssetId, JobFailure> {
        uuid::Uuid::parse_str(&self.track)
            .map(MediaAssetId::from)
            .map_err(unexpected)
    }
}

/// The model that composed audio an attempt saved.
#[derive(Serialize, Deserialize)]
struct TrackRecord {
    model: String,
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// Runs music jobs.
pub(crate) struct MusicHandler {
    pub(crate) owner: ProfileId,
    pub(crate) tracks: Arc<dyn MusicTrackRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) music: Arc<dyn MusicGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for MusicHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.compose(payload, cx.id(), Some(cx))
    }
}

impl MusicHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        let provider = self.music.provider();
        self.secrets
            .get(self.owner, provider)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| {
                JobFailure::new(
                    JobFailureKind::MissingKey,
                    format!("no {} key is saved", provider.code()),
                )
            })
    }

    /// The audio and model an earlier attempt of the job paid for and
    /// saved (its record is written after its audio).
    fn saved(&self, project: VideoProjectId, id: MediaAssetId) -> Option<(Vec<u8>, String)> {
        let record = self.files.read(project, &track_record(id)).ok()?;
        let record: TrackRecord = serde_json::from_slice(&record).ok()?;
        let audio = self.files.read(project, &track_file(id)).ok()?;
        Some((audio, record.model))
    }

    /// Composes the track `job` asks for and records it for review. Without
    /// a context (tests replaying a job) nothing stops it.
    pub(crate) fn compose(
        &self,
        payload: &str,
        job: JobId,
        cx: Option<&mut JobContext>,
    ) -> Result<(), JobFailure> {
        let payload = MusicPayload::parse(payload)?;
        let (project, id) = (payload.project()?, payload.track()?);
        // An earlier attempt may have recorded the track and stopped before
        // the queue recorded the job as done.
        let tracks = self.tracks.music_tracks(project).map_err(unexpected)?;
        if tracks.made_by(job).is_some() {
            return Ok(());
        }
        let request = MusicRequest {
            prompt: payload.prompt.clone(),
            length: Duration::from_millis(payload.length_ms),
        };
        let (audio, model) = match self.saved(project, id) {
            Some(saved) => saved,
            None => {
                if cx.as_ref().is_some_and(|cx| cx.should_stop()) {
                    return Ok(());
                }
                let key = self.key()?;
                let composed = self.music.compose(&key, &request).map_err(|failure| {
                    JobFailure::new(
                        failure.kind.into(),
                        format!("{}: {}", self.music.provider().code(), failure.detail),
                    )
                })?;
                self.costs.record_for_project(
                    PaidCall {
                        provider: self.music.provider(),
                        model: &composed.model,
                        purpose: CostPurpose::Music,
                        usage: Metered::audio_seconds(billed_seconds(request.length)),
                        job: Some(job),
                        reported: None,
                    },
                    project,
                );
                let record = TrackRecord {
                    model: composed.model.clone(),
                };
                let record = serde_json::to_vec(&record).map_err(unexpected)?;
                self.files
                    .write(project, &track_file(id), &composed.audio)
                    .map_err(unexpected)?;
                self.files
                    .write(project, &track_record(id), &record)
                    .map_err(unexpected)?;
                (composed.audio, composed.model)
            }
        };
        let duration = mp3::frames(&audio)
            .map_err(|error| {
                JobFailure::new(
                    JobFailureKind::UnexpectedAnswer,
                    format!(
                        "{}: the audio is not usable ({error})",
                        self.music.provider().code()
                    ),
                )
            })?
            .duration();
        let now = SystemTime::now();
        let asset = MediaAsset {
            id,
            project,
            owner: self.owner,
            kind: MediaKind::Audio,
            source: AssetSource::Generated,
            file: track_file(id),
            name: format!("ElevenLabs Music {}.mp3", payload.number),
            duration,
            picture: None,
            imported_at: now,
        };
        let track = MusicTrack {
            asset: id,
            project,
            owner: self.owner,
            provider: self.music.provider(),
            model,
            prompt: payload.prompt,
            length: request.length,
            job: Some(job),
            generated_at: now,
            chosen_at: None,
        };
        self.tracks
            .add_music_track(&track, &asset)
            .map_err(unexpected)?;
        // Leftovers only cost disk space.
        let _ = self.files.remove(project, &track_record(id));
        // The new track takes the place of the one waiting for review.
        for older in tracks.all().iter().filter(|older| older.in_review()) {
            if self.tracks.remove_music_track(older.asset).is_ok() {
                crate::proxies::remove_media(
                    self.files.as_ref(),
                    project,
                    &track_file(older.asset),
                );
            }
        }
        Ok(())
    }
}

/// A track loaded for playback on the music card. Lives with the view that
/// plays it; not shared between threads.
pub struct MusicTrackPlayer {
    asset: MediaAsset,
    playback: Box<dyn Playback>,
}

impl MusicTrackPlayer {
    /// The track being played.
    pub fn asset(&self) -> &MediaAsset {
        &self.asset
    }

    pub fn is_playing(&self) -> bool {
        self.playback.is_playing()
    }

    /// Plays when paused, pauses when playing.
    pub fn toggle(&mut self) -> Result<(), MusicTrackError> {
        if self.playback.is_playing() {
            self.playback.pause();
            Ok(())
        } else {
            Ok(self.playback.play()?)
        }
    }

    pub fn pause(&mut self) {
        self.playback.pause();
    }

    /// Where playback is, never past the end.
    pub fn position(&self) -> Duration {
        self.playback.position().min(self.asset.duration)
    }

    pub fn duration(&self) -> Duration {
        self.asset.duration
    }
}

impl Bardo {
    fn music_project(&self, id: VideoProjectId) -> Result<VideoProject, MusicTrackError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(MusicTrackError::ProjectNotFound)
    }

    fn latest_music_job(&self, project: VideoProjectId) -> Option<Job> {
        let project = project.to_string();
        self.jobs().into_iter().rev().find(|job| {
            job.kind() == JobKind::Music
                && MusicPayload::parse(job.payload()).is_ok_and(|p| p.project == project)
        })
    }

    /// The length of a new track for the project: its narration's.
    fn music_length_for(
        &self,
        project: VideoProjectId,
    ) -> Result<Option<Duration>, RepositoryError> {
        Ok(self
            .narrations
            .narration(project)?
            .map(|narration| music_length(narration.duration)))
    }

    /// The project's generated tracks with their media assets.
    fn generated_tracks(
        &self,
        project: VideoProjectId,
    ) -> Result<(bardo_domain::MusicTracks, Vec<MediaAsset>), RepositoryError> {
        Ok((
            self.music_tracks.music_tracks(project)?,
            self.media_assets.media_assets(project)?,
        ))
    }

    /// The project's music card.
    pub fn music_tracks(
        &self,
        project: VideoProjectId,
    ) -> Result<MusicTracksView, MusicTrackError> {
        let project = self.music_project(project)?;
        let (tracks, assets) = self.generated_tracks(project.id)?;
        let with_asset = |track: Option<&MusicTrack>| {
            let track = track?;
            let asset = assets.iter().find(|asset| asset.id == track.asset)?;
            Some(GeneratedTrack {
                track: track.clone(),
                asset: asset.clone(),
            })
        };
        let length = self.music_length_for(project.id)?;
        let estimate = match length {
            Some(length) => Some(self.estimate(&[music_call(self.music.provider(), length)])?),
            None => None,
        };
        Ok(MusicTracksView {
            in_use: with_asset(tracks.in_use()),
            in_review: with_asset(tracks.in_review()),
            job: self.latest_music_job(project.id),
            length,
            estimate,
        })
    }

    /// Starts a job in which the music provider composes an instrumental
    /// track from the project's music prompt, as long as its narration.
    /// The track waits for review. Past the provider's budget it needs
    /// `consent`.
    pub fn generate_music(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, MusicTrackError> {
        let project = self.music_project(project)?;
        let prompt = self
            .music_prompts
            .music_prompt(project.id)?
            .ok_or(MusicTrackError::NoPrompt)?;
        let length = self
            .music_length_for(project.id)?
            .ok_or(MusicTrackError::NoNarration)?;
        if self
            .latest_music_job(project.id)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(MusicTrackError::Busy);
        }
        let provider = self.music.provider();
        if self.provider_key(provider).state == KeyState::NotSet {
            return Err(MusicTrackError::MissingKey(provider));
        }
        if let Err(estimate) =
            self.check_video_budget(project.id, &[music_call(provider, length)], consent)?
        {
            return Err(MusicTrackError::OverBudget(estimate));
        }
        let tracks = self.music_tracks.music_tracks(project.id)?;
        let payload = MusicPayload {
            project: project.id.to_string(),
            track: MediaAssetId::new().to_string(),
            number: tracks.all().len() + 1,
            prompt: prompt.text().to_owned(),
            length_ms: length.as_millis() as u64,
        };
        let job = Job::new(self.profile.id, JobKind::Music, payload.to_json());
        Ok(self.jobs.enqueue(job)?)
    }

    /// Puts track `asset` on the music track from the start of the cut, in
    /// place of the generated track used before; music the user placed
    /// stays. A cut built later starts with it too.
    pub fn use_music_track(
        &self,
        project: VideoProjectId,
        asset: MediaAssetId,
    ) -> Result<(), MusicTrackError> {
        let project = self.music_project(project)?;
        let (tracks, assets) = self.generated_tracks(project.id)?;
        let chosen = tracks
            .get(asset)
            .and_then(|track| assets.iter().find(|asset| asset.id == track.asset))
            .ok_or(MusicTrackError::NotFound)?;
        let previous = tracks
            .in_use()
            .filter(|track| track.asset != asset)
            .and_then(|track| assets.iter().find(|asset| asset.id == track.asset))
            .map(|asset| asset.file.clone());
        // Chosen first: should the cut not take it, it is in the media bin
        // to place by hand.
        self.music_tracks
            .choose_music_track(asset, SystemTime::now())?;
        let view = self.editor_view(project.id)?;
        if let (Some(timeline), Some(basis)) = (&view.timeline, view.basis) {
            let used = timeline
                .use_music(previous.as_deref(), chosen)
                .map_err(EditorError::from)?;
            if &used != timeline || view.cut_outdated {
                self.save_cut(project.id, &used, basis)?;
            }
        }
        Ok(())
    }

    /// Removes track `asset`, waiting for review, and its file.
    pub fn discard_music_track(
        &self,
        project: VideoProjectId,
        asset: MediaAssetId,
    ) -> Result<(), MusicTrackError> {
        let project = self.music_project(project)?;
        let (tracks, assets) = self.generated_tracks(project.id)?;
        if !tracks.holds_back(asset) {
            return Err(MusicTrackError::NotFound);
        }
        self.music_tracks.remove_music_track(asset)?;
        if let Some(asset) = assets.iter().find(|found| found.id == asset) {
            crate::proxies::remove_media(self.files.as_ref(), project.id, &asset.file);
        }
        Ok(())
    }

    /// Loads track `asset` for playback, paused at the start.
    pub fn play_music_track(
        &self,
        project: VideoProjectId,
        asset: MediaAssetId,
    ) -> Result<MusicTrackPlayer, MusicTrackError> {
        let project = self.music_project(project)?;
        let (tracks, assets) = self.generated_tracks(project.id)?;
        let asset = tracks
            .get(asset)
            .and_then(|track| assets.into_iter().find(|asset| asset.id == track.asset))
            .ok_or(MusicTrackError::NotFound)?;
        if !self.files.exists(project.id, &asset.file) {
            return Err(MusicTrackError::AudioMissing);
        }
        let playback = self.audio.open(&self.files.path(project.id, &asset.file))?;
        Ok(MusicTrackPlayer { asset, playback })
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        CostRepository, JobState, MediaAssetRepository, ProviderFailure, ProviderFailureKind,
    };

    use super::*;
    use crate::scenes::tests::{Harness, done, wait_done};

    const PROMPT: &str = "Slow ambient synth pads, 70 BPM, hopeful, no drums.";

    /// Writes the project's music prompt.
    fn prompt(h: &Harness, app: &Bardo, project: VideoProjectId) {
        h.answer(PROMPT.to_owned());
        done(
            app,
            app.generate_music_prompt(project, BudgetConsent::Ask)
                .unwrap(),
        );
    }

    /// A narrated project with its music prompt, and scenes when `planned`.
    fn ready(h: &Harness, app: &Bardo, planned: bool) -> VideoProject {
        let project = if planned {
            h.planned_project(app).0
        } else {
            h.narrated_project(app)
        };
        prompt(h, app, project.id);
        project
    }

    fn compose(app: &Bardo, project: VideoProjectId) -> GeneratedTrack {
        let job = done(
            app,
            app.generate_music(project, BudgetConsent::Ask).unwrap(),
        );
        let track = app.music_tracks(project).unwrap().in_review.unwrap();
        assert_eq!(track.track.job, Some(job.id()));
        track
    }

    fn music_files(app: &Bardo, project: VideoProjectId) -> Vec<String> {
        let timeline = app.editor_view(project).unwrap().timeline.unwrap();
        timeline
            .music()
            .iter()
            .map(|item| item.file.clone())
            .collect()
    }

    #[test]
    fn a_track_as_long_as_the_narration_waits_for_review() {
        let h = Harness::new();
        let app = h.start();
        let project = ready(&h, &app, true);
        let narration = app.narration(project.id).unwrap().narration.unwrap();
        let view = app.music_tracks(project.id).unwrap();
        assert_eq!(view.length, Some(music_length(narration.duration)));
        assert!(view.estimate.is_some());

        let track = compose(&app, project.id);
        let request = &h.music.requests()[0];
        assert_eq!(request.prompt, PROMPT);
        assert_eq!(request.length, music_length(narration.duration));
        assert_eq!(track.track.prompt, PROMPT);
        assert_eq!(track.track.model, "music-fake");
        assert_eq!(track.track.provider, Provider::ElevenLabs);
        assert_eq!(track.asset.source, AssetSource::Generated);
        assert_eq!(track.asset.kind, MediaKind::Audio);
        assert_eq!(track.asset.name, "ElevenLabs Music 1.mp3");
        assert!(track.asset.duration >= request.length);
        assert!(h.files.exists(project.id, &track.asset.file));
        assert!(!h.files.exists(project.id, &track_record(track.asset.id)));
        assert_eq!(app.music_tracks(project.id).unwrap().in_use, None);

        // In review: not in the media bin, not in the cut.
        let editor = app.editor_view(project.id).unwrap();
        assert!(
            editor
                .media
                .iter()
                .all(|bin| bin.asset.id != track.asset.id)
        );
        assert!(music_files(&app, project.id).is_empty());

        let costs =
            h.db.recent_costs(app.profile().id, CostPurpose::Music, 10)
                .unwrap();
        assert_eq!(costs.len(), 1);
        assert_eq!(costs[0].provider, Provider::ElevenLabs);
        assert_eq!(costs[0].project, Some(project.id));
        assert_eq!(costs[0].usage.audio_seconds, billed_seconds(request.length));
    }

    #[test]
    fn the_track_used_goes_on_the_music_track_and_starts_every_new_cut() {
        let h = Harness::new();
        let app = h.start();
        let project = ready(&h, &app, true);
        let first = compose(&app, project.id);

        // No cut saved yet: the rough cut starts with the track in use.
        app.use_music_track(project.id, first.asset.id).unwrap();
        let view = app.music_tracks(project.id).unwrap();
        assert_eq!(
            view.in_use.as_ref().map(|t| t.asset.id),
            Some(first.asset.id)
        );
        assert_eq!(view.in_review, None);
        assert_eq!(
            music_files(&app, project.id),
            std::slice::from_ref(&first.asset.file)
        );
        let editor = app.editor_view(project.id).unwrap();
        assert!(
            editor
                .media
                .iter()
                .any(|bin| bin.asset.id == first.asset.id)
        );
        let timeline = editor.timeline.unwrap();
        assert_eq!(timeline.music()[0].at, Duration::ZERO);

        // A saved cut swaps the track used before for the new one; a new
        // track waits without touching the cut.
        app.save_cut(project.id, &timeline, editor.basis.unwrap())
            .unwrap();
        let second = compose(&app, project.id);
        assert_eq!(
            music_files(&app, project.id),
            std::slice::from_ref(&first.asset.file)
        );
        assert_eq!(
            app.music_tracks(project.id)
                .unwrap()
                .in_use
                .map(|t| t.asset.id),
            Some(first.asset.id)
        );
        app.use_music_track(project.id, second.asset.id).unwrap();
        assert_eq!(
            music_files(&app, project.id),
            std::slice::from_ref(&second.asset.file)
        );
        let saved = app.timelines.saved_timeline(project.id).unwrap().unwrap();
        assert_eq!(saved.music_items.len(), 1);
        assert_eq!(saved.music_items[0].file, second.asset.file);

        // Using it again changes nothing.
        app.use_music_track(project.id, second.asset.id).unwrap();
        assert_eq!(
            music_files(&app, project.id),
            std::slice::from_ref(&second.asset.file)
        );
    }

    #[test]
    fn a_track_can_be_used_before_the_scenes_are_planned() {
        let h = Harness::new();
        let app = h.start();
        let project = ready(&h, &app, false);
        let track = compose(&app, project.id);
        app.use_music_track(project.id, track.asset.id).unwrap();
        assert!(app.timelines.saved_timeline(project.id).unwrap().is_none());

        h.answer(crate::scenes::tests::plan_answer());
        done(
            &app,
            app.plan_scenes(project.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(music_files(&app, project.id), [track.asset.file]);
    }

    #[test]
    fn a_new_track_replaces_the_one_in_review_and_discard_removes_it() {
        let h = Harness::new();
        let app = h.start();
        let project = ready(&h, &app, false);
        let first = compose(&app, project.id);
        let second = compose(&app, project.id);
        assert_eq!(second.asset.name, "ElevenLabs Music 2.mp3");
        assert!(!h.files.exists(project.id, &first.asset.file));
        let tracks = app.music_tracks.music_tracks(project.id).unwrap();
        assert_eq!(tracks.all().len(), 1);

        app.discard_music_track(project.id, second.asset.id)
            .unwrap();
        assert!(!h.files.exists(project.id, &second.asset.file));
        let view = app.music_tracks(project.id).unwrap();
        assert_eq!((view.in_use, view.in_review), (None, None));
        assert!(h.db.media_assets(project.id).unwrap().is_empty());

        // The track in use is not discarded from here.
        let third = compose(&app, project.id);
        app.use_music_track(project.id, third.asset.id).unwrap();
        assert!(matches!(
            app.discard_music_track(project.id, third.asset.id),
            Err(MusicTrackError::NotFound)
        ));
        assert!(matches!(
            app.use_music_track(project.id, MediaAssetId::new()),
            Err(MusicTrackError::NotFound)
        ));
    }

    #[test]
    fn generating_needs_a_prompt_a_narration_a_key_and_one_job_at_a_time() {
        let h = Harness::new();
        let mut app = h.start();
        let project = crate::scenes::tests::project(&app);
        let error = app
            .generate_music(project.id, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, MusicTrackError::NoPrompt));
        assert_eq!(error.message(), Text::MusicPromptMissing);

        prompt(&h, &app, project.id);
        let error = app
            .generate_music(project.id, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, MusicTrackError::NoNarration));
        assert_eq!(error.message(), Text::MusicTrackNoNarration);
        assert_eq!(app.music_tracks(project.id).unwrap().length, None);

        h.answer(crate::scenes::tests::SCRIPT.to_owned());
        done(
            &app,
            app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
        );
        done(
            &app,
            app.generate_narration(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        app.remove_provider_key(Provider::ElevenLabs).unwrap();
        let error = app
            .generate_music(project.id, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(
            error,
            MusicTrackError::MissingKey(Provider::ElevenLabs)
        ));
        assert_eq!(error.message(), Text::MusicTrackMissingKey);

        app.save_provider_key(Provider::ElevenLabs, crate::scenes::tests::ELEVENLABS_KEY)
            .unwrap();
        *h.music.delay.lock().unwrap() = Duration::from_millis(100);
        let id = app.generate_music(project.id, BudgetConsent::Ask).unwrap();
        assert!(matches!(
            app.generate_music(project.id, BudgetConsent::Ask),
            Err(MusicTrackError::Busy)
        ));
        wait_done(&app, id);
        assert!(app.generate_music(project.id, BudgetConsent::Ask).is_ok());
    }

    #[test]
    fn a_provider_failure_adds_nothing_and_says_why() {
        let h = Harness::new();
        let app = h.start();
        let project = ready(&h, &app, false);
        *h.music.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::NotAllowed,
            "the API key is missing the music_generation permission",
        ));
        let job = wait_done(
            &app,
            app.generate_music(project.id, BudgetConsent::Ask).unwrap(),
        );
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::NotAllowed);
        assert!(
            failure.detail.contains("music_generation"),
            "{}",
            failure.detail
        );
        let view = app.music_tracks(project.id).unwrap();
        assert_eq!(view.job.map(|job| job.id()), Some(job.id()));
        assert_eq!(view.in_review, None);
        assert!(h.db.media_assets(project.id).unwrap().is_empty());
    }

    #[test]
    fn a_resumed_job_never_pays_twice() {
        let h = Harness::new();
        let app = h.start();
        let project = ready(&h, &app, false);
        let handler = MusicHandler {
            owner: app.profile().id,
            tracks: Arc::clone(&h.db) as _,
            files: Arc::clone(&h.files) as _,
            music: Arc::clone(&h.music) as _,
            secrets: Arc::clone(&h.secrets) as _,
            costs: CostBook {
                owner: app.profile().id,
                costs: Arc::clone(&h.db) as _,
                themes: Arc::clone(&h.db) as _,
            },
        };
        let id = MediaAssetId::new();
        let payload = MusicPayload {
            project: project.id.to_string(),
            track: id.to_string(),
            number: 1,
            prompt: PROMPT.into(),
            length_ms: 3_000,
        }
        .to_json();
        let job = JobId::new();

        // An attempt that paid and saved the audio, then stopped.
        let audio = crate::testing::PART_AUDIO.repeat(3);
        h.files.write(project.id, &track_file(id), &audio).unwrap();
        let record = serde_json::to_vec(&TrackRecord {
            model: "music-earlier".into(),
        })
        .unwrap();
        h.files
            .write(project.id, &track_record(id), &record)
            .unwrap();
        handler.compose(&payload, job, None).unwrap();
        assert!(h.music.requests().is_empty(), "ElevenLabs is not asked");
        let tracks = h.db.music_tracks(project.id).unwrap();
        assert_eq!(tracks.made_by(job).unwrap().model, "music-earlier");

        // Once the track is recorded, running the job again does nothing.
        handler.compose(&payload, job, None).unwrap();
        assert!(h.music.requests().is_empty());
        assert_eq!(h.db.music_tracks(project.id).unwrap(), tracks);
    }

    #[test]
    fn a_track_plays_from_the_project_folder() {
        let h = Harness::new();
        let app = h.start();
        let project = ready(&h, &app, false);
        let track = compose(&app, project.id);
        let mut player = app.play_music_track(project.id, track.asset.id).unwrap();
        assert_eq!(player.asset().id, track.asset.id);
        assert_eq!(player.duration(), track.asset.duration);
        assert!(!player.is_playing());
        player.toggle().unwrap();
        assert!(player.is_playing());
        player.pause();
        assert!(!player.is_playing());

        h.files.remove(project.id, &track.asset.file).unwrap();
        assert!(matches!(
            app.play_music_track(project.id, track.asset.id),
            Err(MusicTrackError::AudioMissing)
        ));
        assert!(matches!(
            app.music_tracks(VideoProjectId::new()),
            Err(MusicTrackError::ProjectNotFound)
        ));
    }
}
