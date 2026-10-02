//! Video clips (PRD stories 9, 37-39): a video provider animates a scene's
//! image into a short clip, moved by a prompt. Providers work asynchronously:
//! a submission returns a handle at once and the clip is ready minutes
//! later, so the interface is submit and poll. The job running it saves the
//! handle before waiting, so a restart polls the same work instead of
//! paying for it again.
//!
//! A channel picks the provider and model its clips use; any scene can pick
//! another. Each provider adapter lists the models it offers and the clip
//! lengths each allows.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::{ApiKey, ImageFormat, Money, Provider, ProviderFailure};

/// Why a model reference is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ClipModelError {
    #[error("the model is empty")]
    Required,
    #[error("the model name is too long")]
    TooLong,
    #[error("the model name has spaces")]
    HasSpaces,
}

/// A video model of a provider, as the provider names it (Higgsfield names
/// a model by its endpoint, e.g. `kling-video/v3.0/std/image-to-video`).
/// Always valid: present, no spaces, within limits.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipModelRef {
    provider: Provider,
    model: String,
}

impl ClipModelRef {
    pub const MAX_MODEL_CHARS: usize = 100;

    pub fn new(provider: Provider, model: &str) -> Result<Self, ClipModelError> {
        let model = model.trim();
        if model.is_empty() {
            Err(ClipModelError::Required)
        } else if model.chars().count() > Self::MAX_MODEL_CHARS {
            Err(ClipModelError::TooLong)
        } else if model.chars().any(char::is_whitespace) {
            Err(ClipModelError::HasSpaces)
        } else {
            Ok(Self {
                provider,
                model: model.to_owned(),
            })
        }
    }

    pub fn provider(&self) -> Provider {
        self.provider
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

impl fmt::Display for ClipModelRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider, self.model)
    }
}

/// The clip lengths a model accepts, in whole seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipDurations {
    /// Any length from `min` to `max`.
    Range { min: u32, max: u32 },
    /// Only these lengths, shortest first.
    Choices(Vec<u32>),
}

impl ClipDurations {
    /// The length to ask for to cover `wanted`: the shortest allowed one
    /// that is at least as long (the rough cut trims the rest), else the
    /// longest allowed (the still image covers the rest).
    pub fn fit(&self, wanted: Duration) -> u32 {
        let wanted = u32::try_from(wanted.as_millis().div_ceil(1000)).unwrap_or(u32::MAX);
        match self {
            ClipDurations::Range { min, max } => wanted.clamp(*min, (*max).max(*min)),
            ClipDurations::Choices(choices) => choices
                .iter()
                .copied()
                .filter(|choice| *choice >= wanted)
                .min()
                .or_else(|| choices.iter().copied().max())
                .unwrap_or(wanted),
        }
    }

    /// The longest length the model accepts.
    pub fn longest(&self) -> u32 {
        match self {
            ClipDurations::Range { min, max } => (*max).max(*min),
            ClipDurations::Choices(choices) => choices.iter().copied().max().unwrap_or(0),
        }
    }
}

/// A model a provider offers for clips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipModel {
    pub id: ClipModelRef,
    /// How people call it, e.g. "Kling 3.0 Standard".
    pub name: String,
    pub durations: ClipDurations,
}

/// The first frame of a clip: the scene's image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipImage {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
}

/// An image handed to the provider ahead of a submission (Higgsfield reads
/// it from a URL it gives out), as the provider refers to it. Saved with
/// the job, so a resubmission sends the very same request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedImage(pub String);

/// What to animate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipRequest {
    /// The model, as the provider names it.
    pub model: String,
    /// How the image moves: subject, action, camera.
    pub prompt: String,
    pub image: StagedImage,
    pub seconds: u32,
}

/// The provider's id for submitted work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipHandle(pub String);

/// A submission the provider accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipSubmission {
    pub handle: ClipHandle,
    /// What the provider said the clip costs, when it said so before
    /// starting; the rate table prices it otherwise.
    pub quote: Option<Money>,
}

/// Where submitted work stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipStatus {
    /// Waiting for the provider to start it.
    Queued,
    /// Being generated.
    Running,
    /// Ready to download from `video`.
    Done { video: String },
    /// It ended without a clip: failed, declined by moderation or
    /// cancelled at the provider. Providers do not charge for these.
    Failed(ProviderFailure),
}

/// A downloaded clip, an MP4 file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedClip {
    pub bytes: Vec<u8>,
}

/// Animates images into clips. Calls the network and blocks, so it runs
/// inside a job.
pub trait ClipGenerator: Send + Sync {
    fn provider(&self) -> Provider;

    /// The models it offers, the provider's default first.
    fn models(&self) -> Vec<ClipModel>;

    /// Hands the first frame to the provider.
    fn stage_image(&self, key: &ApiKey, image: &ClipImage) -> Result<StagedImage, ProviderFailure>;

    /// Starts the clip. Submitting the same request again with the same
    /// `submission` id returns the first submission instead of starting
    /// (and charging for) another clip.
    fn submit(
        &self,
        key: &ApiKey,
        request: &ClipRequest,
        submission: &str,
    ) -> Result<ClipSubmission, ProviderFailure>;

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<ClipStatus, ProviderFailure>;

    /// Downloads a finished clip from where `ClipStatus::Done` said.
    fn download(&self, video: &str) -> Result<GeneratedClip, ProviderFailure>;

    /// How long to wait before status check number `polls` (from 0).
    fn poll_delay(&self, polls: u32) -> Duration {
        // Two seconds, growing by half each time up to ten.
        let millis = 2_000.0 * 1.5_f64.powi(polls.min(10) as i32);
        Duration::from_millis(millis.min(10_000.0) as u64)
    }
}

impl<T: ClipGenerator + ?Sized> ClipGenerator for Arc<T> {
    fn provider(&self) -> Provider {
        (**self).provider()
    }

    fn models(&self) -> Vec<ClipModel> {
        (**self).models()
    }

    fn stage_image(&self, key: &ApiKey, image: &ClipImage) -> Result<StagedImage, ProviderFailure> {
        (**self).stage_image(key, image)
    }

    fn submit(
        &self,
        key: &ApiKey,
        request: &ClipRequest,
        submission: &str,
    ) -> Result<ClipSubmission, ProviderFailure> {
        (**self).submit(key, request, submission)
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<ClipStatus, ProviderFailure> {
        (**self).status(key, handle)
    }

    fn download(&self, video: &str) -> Result<GeneratedClip, ProviderFailure> {
        (**self).download(video)
    }

    fn poll_delay(&self, polls: u32) -> Duration {
        (**self).poll_delay(polls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn model_refs_are_trimmed_and_checked() {
        let model = ClipModelRef::new(Provider::Higgsfield, " kling/v3 ").unwrap();
        assert_eq!(model.model(), "kling/v3");
        assert_eq!(model.provider(), Provider::Higgsfield);
        assert_eq!(model.to_string(), "higgsfield:kling/v3");
        assert_eq!(
            ClipModelRef::new(Provider::Higgsfield, " "),
            Err(ClipModelError::Required)
        );
        assert_eq!(
            ClipModelRef::new(Provider::Higgsfield, "kling v3"),
            Err(ClipModelError::HasSpaces)
        );
        let long = "m".repeat(ClipModelRef::MAX_MODEL_CHARS + 1);
        assert_eq!(
            ClipModelRef::new(Provider::Higgsfield, &long),
            Err(ClipModelError::TooLong)
        );
    }

    #[test]
    fn a_range_fits_the_scene_rounded_up_within_its_limits() {
        let range = ClipDurations::Range { min: 3, max: 15 };
        assert_eq!(range.fit(Duration::from_millis(7_200)), 8);
        assert_eq!(range.fit(secs(7)), 7);
        assert_eq!(range.fit(Duration::from_millis(900)), 3, "at least min");
        assert_eq!(range.fit(secs(40)), 15, "at most max");
        assert_eq!(range.longest(), 15);
    }

    #[test]
    fn choices_take_the_shortest_that_covers_the_scene() {
        let choices = ClipDurations::Choices(vec![5, 10]);
        assert_eq!(choices.fit(secs(4)), 5);
        assert_eq!(choices.fit(Duration::from_millis(5_001)), 10);
        assert_eq!(choices.fit(secs(10)), 10);
        assert_eq!(choices.fit(secs(25)), 10, "the longest when none covers");
        assert_eq!(choices.longest(), 10);
    }

    #[test]
    fn polling_starts_at_two_seconds_and_slows_to_ten() {
        struct Polls;
        impl ClipGenerator for Polls {
            fn provider(&self) -> Provider {
                Provider::Higgsfield
            }
            fn models(&self) -> Vec<ClipModel> {
                Vec::new()
            }
            fn stage_image(
                &self,
                _: &ApiKey,
                _: &ClipImage,
            ) -> Result<StagedImage, ProviderFailure> {
                unreachable!()
            }
            fn submit(
                &self,
                _: &ApiKey,
                _: &ClipRequest,
                _: &str,
            ) -> Result<ClipSubmission, ProviderFailure> {
                unreachable!()
            }
            fn status(&self, _: &ApiKey, _: &ClipHandle) -> Result<ClipStatus, ProviderFailure> {
                unreachable!()
            }
            fn download(&self, _: &str) -> Result<GeneratedClip, ProviderFailure> {
                unreachable!()
            }
        }
        let delays: Vec<_> = (0..6).map(|n| Polls.poll_delay(n).as_millis()).collect();
        assert_eq!(delays, [2_000, 3_000, 4_500, 6_750, 10_000, 10_000]);
        assert_eq!(Polls.poll_delay(u32::MAX), secs(10));
    }
}
