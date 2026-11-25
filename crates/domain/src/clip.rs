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

use crate::{
    ApiKey, AspectRatio, ImageFormat, Money, Provider, ProviderFailure, image_dimensions, shape_of,
};

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

/// How clips are asked of a model that only makes fixed lengths (5 or 10
/// seconds, say), when a scene falls between two of them (#260).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClipFit {
    /// The shorter length when the scene is only a little longer (within
    /// the channel's [`StretchLimit`]), played slowed to end with the
    /// scene: a 6 second scene pays for 5.
    #[default]
    Stretch,
    /// Always a length that covers the scene, the rest trimmed: a 6
    /// second scene pays for 10. For clips with fast action, where slow
    /// motion shows.
    Cover,
}

impl ClipFit {
    pub const ALL: [ClipFit; 2] = [ClipFit::Stretch, ClipFit::Cover];

    /// The stable name it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            ClipFit::Stretch => "stretch",
            ClipFit::Cover => "cover",
        }
    }

    /// The fit stored as `code`; one this version does not know reads as
    /// the default.
    pub fn from_code(code: &str) -> ClipFit {
        Self::ALL
            .into_iter()
            .find(|fit| fit.code() == code)
            .unwrap_or_default()
    }
}

/// How much longer than itself a stretched clip may play, in percent: at
/// 40, a 5 second clip covers a scene of up to 7 and plays no slower than
/// five sevenths of its speed. Always within [`StretchLimit::MIN`] and
/// [`StretchLimit::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StretchLimit(u8);

impl StretchLimit {
    pub const MIN: u8 = 5;
    /// Twice its length: the clip at half speed.
    pub const MAX: u8 = 100;
    /// The limits a form offers are this far apart.
    pub const STEP: u8 = 5;
    /// A clip at 71% of its speed or faster: on the cautious side of
    /// where slow motion starts to show on ordinary movement.
    pub const DEFAULT: StretchLimit = StretchLimit(40);

    /// The limit nearest `percent`: within the range and on a step, so
    /// what is stored is always a limit the form can show.
    pub fn new(percent: u32) -> StretchLimit {
        let step = u32::from(Self::STEP);
        let nearest = (percent.min(Self::MAX.into()) + step / 2) / step * step;
        StretchLimit(nearest.clamp(Self::MIN.into(), Self::MAX.into()) as u8)
    }

    pub fn percent(self) -> u8 {
        self.0
    }

    /// The limits a form offers, lowest first.
    pub fn choices() -> impl Iterator<Item = StretchLimit> {
        (Self::MIN..=Self::MAX)
            .step_by(Self::STEP.into())
            .map(StretchLimit)
    }

    /// The longest a clip of `seconds` plays when stretched.
    pub fn longest(self, seconds: u32) -> Duration {
        Duration::from_secs(seconds.into()) * (100 + u32::from(self.0)) / 100
    }

    /// Whether a clip of `seconds`, played slowed, covers a scene of
    /// `scene` within the limit.
    pub fn covers(self, seconds: u32, scene: Duration) -> bool {
        scene <= self.longest(seconds)
    }
}

impl Default for StretchLimit {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The length to ask a model for, for one scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipLength {
    pub seconds: u32,
    /// Set when that is shorter than the scene on purpose: the clip plays
    /// slowed, to end when the scene does, within this limit.
    pub stretch: Option<StretchLimit>,
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
    ///
    /// With a `stretch` limit, a model of fixed lengths gives the longest
    /// one short of `wanted` instead, when playing it slowed covers the
    /// scene within the limit: a scene a little over one length does not
    /// pay for the next. A model of any length in a range is asked for
    /// what the scene needs either way.
    pub fn fit(&self, wanted: Duration, stretch: Option<StretchLimit>) -> ClipLength {
        let whole = u32::try_from(wanted.as_millis().div_ceil(1000)).unwrap_or(u32::MAX);
        let exact = |seconds| ClipLength {
            seconds,
            stretch: None,
        };
        match self {
            ClipDurations::Range { min, max } => exact(whole.clamp(*min, (*max).max(*min))),
            ClipDurations::Choices(choices) => {
                let slowed = stretch.and_then(|limit| {
                    choices
                        .iter()
                        .copied()
                        .filter(|&choice| {
                            Duration::from_secs(choice.into()) < wanted
                                && limit.covers(choice, wanted)
                        })
                        .max()
                });
                if let Some(seconds) = slowed {
                    return ClipLength { seconds, stretch };
                }
                if let Some(seconds) = choices.iter().copied().filter(|c| *c >= whole).min() {
                    return exact(seconds);
                }
                // Longer than every length: the longest one, which covers
                // more of the scene slowed than at its own speed, for the
                // same price. It holds its last frame for the rest.
                match choices.iter().copied().max() {
                    Some(seconds) => ClipLength { seconds, stretch },
                    None => exact(whole),
                }
            }
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

/// A model a provider stopped serving, and the one it offers in its place.
/// A channel or scene saved with `id` uses `replacement` from then on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetiredClipModel {
    pub id: ClipModelRef,
    /// How people called it, e.g. "Veo 3.1 Fast 720p".
    pub name: String,
    /// One of the provider's `models`.
    pub replacement: ClipModelRef,
}

/// The offered model a saved choice names today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentClipModel<'a> {
    pub model: &'a ClipModel,
    /// The retired model that was saved, when `model` takes its place.
    pub replaces: Option<&'a RetiredClipModel>,
}

/// What `saved` names among `models` today: itself while it is offered,
/// else the model its provider serves in its place. `None` when neither is
/// offered.
pub fn current_clip_model<'a>(
    models: &'a [ClipModel],
    retired: &'a [RetiredClipModel],
    saved: &ClipModelRef,
) -> Option<CurrentClipModel<'a>> {
    let offered = |id: &ClipModelRef| models.iter().find(|model| model.id == *id);
    if let Some(model) = offered(saved) {
        return Some(CurrentClipModel {
            model,
            replaces: None,
        });
    }
    let gone = retired.iter().find(|gone| gone.id == *saved)?;
    Some(CurrentClipModel {
        model: offered(&gone.replacement)?,
        replaces: Some(gone),
    })
}

/// The first frame of a clip: the scene's image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipImage {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
}

impl ClipImage {
    /// The frame shape of the image, which the clip keeps: landscape when
    /// its header cannot be read.
    pub fn aspect(&self) -> AspectRatio {
        image_dimensions(&self.bytes).map_or(AspectRatio::Landscape, |(width, height)| {
            shape_of(width, height)
        })
    }
}

/// An image handed to the provider ahead of a submission (Higgsfield reads
/// it from a URL it gives out), as the provider refers to it. Saved with
/// the job, so a resubmission sends the very same request. Providers that
/// take the image inline with the submission (Google) stage nothing and
/// read `ClipRequest::first_frame` instead.
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
    /// The image itself, for providers that take it inline.
    pub first_frame: ClipImage,
    pub seconds: u32,
    /// The clip's frame shape: its first frame's (#179).
    pub aspect: AspectRatio,
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
    /// It ended without a clip, and the provider said so: failed, declined
    /// by moderation or cancelled there. Providers do not charge for these.
    Failed(ProviderFailure),
    /// No clip can be had from it, and the provider did not say it made
    /// none: it no longer knows the request (results are kept for a while
    /// only), or it stopped part-way. It may have been charged.
    Lost(ProviderFailure),
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

    /// The models it offered once and the provider no longer serves, each
    /// with the offered model that takes its place.
    fn retired(&self) -> Vec<RetiredClipModel> {
        Vec::new()
    }

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

    /// Whether `submit` keeps its promise above. A provider without an
    /// idempotency key cannot: the job then writes down that it sent a
    /// request before sending it, and never sends it again by itself once
    /// the request may have reached the provider. `submit` says so with a
    /// failure of kind `AnswerLost`; any other failure means nothing was
    /// started.
    fn dedupes_submissions(&self) -> bool {
        true
    }

    /// Whether `find_submitted` can look for the clip a lost answer
    /// started. Where it cannot, such a submission ends there: it may have
    /// started, and nobody can tell. Only asked when `dedupes_submissions`
    /// is false.
    fn lists_submissions(&self) -> bool {
        false
    }

    /// Free checks before a submission that cannot be repeated safely
    /// (credits, for one). Only asked when `dedupes_submissions` is false.
    fn check_submit(&self, _key: &ApiKey, _request: &ClipRequest) -> Result<(), ProviderFailure> {
        Ok(())
    }

    /// The clip an earlier `submit` of this request started, when its
    /// answer was lost. `Ok(None)` means the provider shows none. Only
    /// asked when `lists_submissions` is true.
    fn find_submitted(
        &self,
        _key: &ApiKey,
        _request: &ClipRequest,
    ) -> Result<Option<ClipHandle>, ProviderFailure> {
        Ok(None)
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<ClipStatus, ProviderFailure>;

    /// Downloads a finished clip from where `ClipStatus::Done` said. Some
    /// providers serve it only with the key (Google); others must never
    /// see it (Higgsfield's CDN).
    fn download(&self, key: &ApiKey, video: &str) -> Result<GeneratedClip, ProviderFailure>;

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

    fn retired(&self) -> Vec<RetiredClipModel> {
        (**self).retired()
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

    fn dedupes_submissions(&self) -> bool {
        (**self).dedupes_submissions()
    }

    fn lists_submissions(&self) -> bool {
        (**self).lists_submissions()
    }

    fn check_submit(&self, key: &ApiKey, request: &ClipRequest) -> Result<(), ProviderFailure> {
        (**self).check_submit(key, request)
    }

    fn find_submitted(
        &self,
        key: &ApiKey,
        request: &ClipRequest,
    ) -> Result<Option<ClipHandle>, ProviderFailure> {
        (**self).find_submitted(key, request)
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<ClipStatus, ProviderFailure> {
        (**self).status(key, handle)
    }

    fn download(&self, key: &ApiKey, video: &str) -> Result<GeneratedClip, ProviderFailure> {
        (**self).download(key, video)
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

    const COVER: Option<StretchLimit> = None;
    const STRETCH: Option<StretchLimit> = Some(StretchLimit::DEFAULT);

    fn exact(seconds: u32) -> ClipLength {
        ClipLength {
            seconds,
            stretch: None,
        }
    }

    fn slowed(seconds: u32) -> ClipLength {
        ClipLength {
            seconds,
            stretch: STRETCH,
        }
    }

    #[test]
    fn a_saved_model_names_itself_or_what_replaced_it() {
        let model = |id: &str| ClipModelRef::new(Provider::Gemini, id).unwrap();
        let offer = |id: &str| ClipModel {
            id: model(id),
            name: id.to_uppercase(),
            durations: ClipDurations::Range { min: 3, max: 15 },
        };
        let retire = |id: &str, replacement: &str| RetiredClipModel {
            id: model(id),
            name: id.to_uppercase(),
            replacement: model(replacement),
        };
        let models = [offer("new/720p"), offer("back/720p")];
        let retired = [
            retire("old/720p", "new/720p"),
            // Its replacement is not offered (any more).
            retire("older/720p", "old/720p"),
            // Offered again: the offer wins.
            retire("back/720p", "new/720p"),
        ];
        let current = |id: &str| current_clip_model(&models, &retired, &model(id));

        let kept = current("new/720p").unwrap();
        assert_eq!((kept.model, kept.replaces), (&models[0], None));
        let replaced = current("old/720p").unwrap();
        assert_eq!(
            (replaced.model, replaced.replaces),
            (&models[0], Some(&retired[0]))
        );
        let back = current("back/720p").unwrap();
        assert_eq!((back.model, back.replaces), (&models[1], None));
        assert_eq!(current("older/720p"), None);
        assert_eq!(current("unknown/720p"), None);
        // Another provider's model of the same name is another model.
        let elsewhere = ClipModelRef::new(Provider::Kling, "old/720p").unwrap();
        assert_eq!(current_clip_model(&models, &retired, &elsewhere), None);
    }

    #[test]
    fn a_range_fits_the_scene_rounded_up_within_its_limits() {
        let range = ClipDurations::Range { min: 3, max: 15 };
        for stretch in [COVER, STRETCH] {
            assert_eq!(range.fit(Duration::from_millis(7_200), stretch), exact(8));
            assert_eq!(range.fit(secs(7), stretch), exact(7));
            assert_eq!(
                range.fit(Duration::from_millis(900), stretch),
                exact(3),
                "at least min"
            );
            assert_eq!(range.fit(secs(40), stretch), exact(15), "at most max");
        }
        assert_eq!(range.longest(), 15);
    }

    #[test]
    fn choices_take_the_shortest_that_covers_the_scene() {
        let choices = ClipDurations::Choices(vec![5, 10]);
        assert_eq!(choices.fit(secs(4), COVER), exact(5));
        assert_eq!(choices.fit(Duration::from_millis(5_001), COVER), exact(10));
        assert_eq!(choices.fit(Duration::from_millis(6_500), COVER), exact(10));
        assert_eq!(choices.fit(secs(10), COVER), exact(10));
        assert_eq!(
            choices.fit(secs(25), COVER),
            exact(10),
            "the longest when none covers"
        );
        assert_eq!(choices.longest(), 10);
    }

    #[test]
    fn a_scene_a_little_over_a_length_takes_it_and_plays_it_slowed() {
        let kling = ClipDurations::Choices(vec![5, 10]);
        assert_eq!(kling.fit(Duration::from_millis(6_500), STRETCH), slowed(5));
        assert_eq!(kling.fit(Duration::from_millis(5_001), STRETCH), slowed(5));
        assert_eq!(kling.fit(secs(7), STRETCH), slowed(5), "40% over five");
        assert_eq!(
            kling.fit(Duration::from_millis(7_001), STRETCH),
            exact(10),
            "past the stretch, the longer one trimmed"
        );
        assert_eq!(kling.fit(secs(8), STRETCH), exact(10));
        assert_eq!(kling.fit(secs(5), STRETCH), exact(5), "a clip that fits");
        assert_eq!(kling.fit(secs(4), STRETCH), exact(5));
        assert_eq!(kling.fit(secs(12), STRETCH), slowed(10));
        assert_eq!(kling.fit(secs(14), STRETCH), slowed(10));
        assert_eq!(
            kling.fit(secs(25), STRETCH),
            slowed(10),
            "too long for any: the longest, slowed to cover more of it"
        );
    }

    #[test]
    fn the_longest_length_short_of_the_scene_is_the_one_stretched() {
        let veo = ClipDurations::Choices(vec![4, 6, 8]);
        // 4 s stretches to 5.6 and 6 s to 8.4.
        assert_eq!(veo.fit(secs(5), STRETCH), slowed(4));
        assert_eq!(veo.fit(Duration::from_millis(5_700), STRETCH), exact(6));
        assert_eq!(veo.fit(secs(7), STRETCH), slowed(6));
        assert_eq!(veo.fit(secs(8), STRETCH), slowed(6));
        assert_eq!(veo.fit(Duration::from_millis(8_500), STRETCH), slowed(8));
        assert_eq!(veo.fit(secs(11), STRETCH), slowed(8));
        assert_eq!(veo.fit(secs(12), STRETCH), slowed(8), "longer than any");
    }

    #[test]
    fn the_limit_says_how_far_a_clip_stretches() {
        let kling = ClipDurations::Choices(vec![5, 10]);
        let at = |percent: u32| Some(StretchLimit::new(percent));
        let stretched = |seconds, percent| ClipLength {
            seconds,
            stretch: at(percent),
        };
        // At 20%, five seconds cover six and no more.
        assert_eq!(kling.fit(secs(6), at(20)), stretched(5, 20));
        assert_eq!(kling.fit(Duration::from_millis(6_500), at(20)), exact(10));
        // At 100%, five seconds cover ten at half speed: only a scene
        // past that pays for the longer clip, itself stretched.
        assert_eq!(kling.fit(secs(9), at(100)), stretched(5, 100));
        assert_eq!(kling.fit(secs(10), at(100)), stretched(5, 100));
        assert_eq!(kling.fit(secs(11), at(100)), stretched(10, 100));
    }

    #[test]
    fn a_stretch_limit_stays_within_its_range() {
        assert_eq!(StretchLimit::default().percent(), 40);
        assert_eq!(StretchLimit::new(0).percent(), StretchLimit::MIN);
        assert_eq!(StretchLimit::new(37).percent(), 35);
        assert_eq!(StretchLimit::new(38).percent(), 40);
        assert_eq!(StretchLimit::new(3).percent(), StretchLimit::MIN);
        assert_eq!(StretchLimit::new(400).percent(), StretchLimit::MAX);
        let choices: Vec<u8> = StretchLimit::choices().map(StretchLimit::percent).collect();
        assert_eq!(choices.len(), 20);
        assert_eq!((choices[0], choices[19]), (5, 100));
        assert!(choices.contains(&StretchLimit::DEFAULT.percent()));
        assert!(choices.windows(2).all(|pair| pair[1] - pair[0] == 5));

        let limit = StretchLimit::default();
        assert_eq!(limit.longest(5), secs(7));
        assert_eq!(limit.longest(4), Duration::from_millis(5_600));
        assert!(limit.covers(5, secs(7)));
        assert!(!limit.covers(5, Duration::from_millis(7_001)));
        assert_eq!(StretchLimit::new(100).longest(5), secs(10));
    }

    #[test]
    fn a_fit_is_stored_by_a_stable_name() {
        for fit in ClipFit::ALL {
            assert_eq!(ClipFit::from_code(fit.code()), fit);
        }
        assert_eq!(ClipFit::Stretch.code(), "stretch");
        assert_eq!(ClipFit::Cover.code(), "cover");
        assert_eq!(
            ClipFit::from_code("from-a-newer-version"),
            ClipFit::default()
        );
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
            fn download(&self, _: &ApiKey, _: &str) -> Result<GeneratedClip, ProviderFailure> {
                unreachable!()
            }
        }
        let delays: Vec<_> = (0..6).map(|n| Polls.poll_delay(n).as_millis()).collect();
        assert_eq!(delays, [2_000, 3_000, 4_500, 6_750, 10_000, 10_000]);
        assert_eq!(Polls.poll_delay(u32::MAX), secs(10));
    }
}
