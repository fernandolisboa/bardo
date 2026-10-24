//! Take models (#192, docs/spec/performance-capture.md): the video models
//! that turn a take's approved control video and the persona's look into
//! a performed clip.
//!
//! Like clip providers, take models work asynchronously: the job hands
//! the inputs to the provider, submits, saves the provider's request id
//! and polls it, so a restart polls the same work instead of paying for it
//! again. A model makes a cheap **draft** where it has a cheap tier, and a
//! **final** at 1080p; each is reviewed before anything goes further.
//!
//! A channel picks the take model its takes use; any take can pick
//! another. Each model lists the control video modes it takes, the first
//! being the default.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::{
    ApiKey, ClipHandle, ClipModelRef, GeneratedClip, Metered, Money, Provider, ProviderFailure,
    TakeMode,
};

/// Which of a take model's two results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TakeStep {
    /// The cheap tier, to check the take before paying for the final.
    Draft,
    /// The 1080p result that becomes the performed clip.
    Final,
}

impl TakeStep {
    pub const ALL: [TakeStep; 2] = [TakeStep::Draft, TakeStep::Final];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            TakeStep::Draft => "draft",
            TakeStep::Final => "final",
        }
    }

    pub fn from_code(code: &str) -> Option<TakeStep> {
        TakeStep::ALL.into_iter().find(|step| step.code() == code)
    }
}

/// A take model's tier: what a draft or a final is made at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeTier {
    /// The resolution the request names, for models that take one
    /// (`480p`); `None` where the endpoint fixes it.
    pub resolution: Option<String>,
    /// The picture's short side, in pixels.
    pub height: u32,
}

/// A video model a provider offers for takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeModel {
    pub id: ClipModelRef,
    /// How people call it, e.g. "Kling 3.0 Motion Control".
    pub name: String,
    /// The control video modes it takes, its default first.
    pub modes: Vec<TakeMode>,
    /// Its cheap tier, when it has one.
    pub draft: Option<TakeTier>,
    pub final_tier: TakeTier,
    /// Whether a final finishes the approved draft from the draft's own
    /// parameters; otherwise a final is a new take.
    pub finishes_draft: bool,
    /// How long after it was sent a draft can still be finished, for a
    /// model whose final finishes it (ModelArk: seven days).
    pub draft_lifetime: Option<Duration>,
    /// Whether it takes a look from the provider's own asset library
    /// (`asset://`) in place of the image.
    pub look_assets: bool,
    /// For a model billed by its video's tokens, the frame rate they are
    /// counted at (ModelArk: 24): a take of `L` seconds in and out at
    /// `W` × `H` is `2 × L × W × H × rate / 1024` tokens. `None` for a
    /// model billed per second.
    pub token_frame_rate: Option<u32>,
    /// Whether its result keeps the take's framing (a video edit does), so
    /// the take's face is where it was: the cut starts a 9:16 crop on it
    /// (#193).
    pub keeps_framing: bool,
    /// The take lengths it accepts.
    pub min_length: Duration,
    pub max_length: Duration,
    /// The largest control video and look it reads, in bytes.
    pub max_video_bytes: u64,
    pub max_image_bytes: u64,
}

impl TakeModel {
    /// Whether it takes control videos in `mode`.
    pub fn takes(&self, mode: TakeMode) -> bool {
        self.modes.contains(&mode)
    }

    /// The mode a take of this model starts in.
    pub fn default_mode(&self) -> TakeMode {
        self.modes.first().copied().unwrap_or_default()
    }

    /// The tier `step` is made at; `None` for a draft of a model without
    /// one.
    pub fn tier(&self, step: TakeStep) -> Option<&TakeTier> {
        match step {
            TakeStep::Draft => self.draft.as_ref(),
            TakeStep::Final => Some(&self.final_tier),
        }
    }

    /// The first paid step of a take: its draft where the model has one.
    pub fn first_step(&self) -> TakeStep {
        if self.draft.is_some() {
            TakeStep::Draft
        } else {
            TakeStep::Final
        }
    }

    /// Until when a draft sent at `sent` can be finished, for a model whose
    /// final finishes its draft within a lifetime.
    pub fn draft_deadline(&self, sent: SystemTime) -> Option<SystemTime> {
        self.draft_lifetime
            .filter(|_| self.finishes_draft)
            .map(|lifetime| sent + lifetime)
    }

    /// The tokens `step` of a take of `length` would be billed, for a
    /// model billed by tokens: the take's frame (`width` × `height`)
    /// scaled to the tier's short side, its seconds counted in and out.
    pub fn estimated_tokens(
        &self,
        step: TakeStep,
        length: Duration,
        (width, height): (u32, u32),
    ) -> Option<u64> {
        let rate = u128::from(self.token_frame_rate?);
        let short = u128::from(self.tier(step)?.height);
        let (long_side, short_side) = (width.max(height), width.min(height).max(1));
        let long =
            (short * u128::from(long_side) + u128::from(short_side) / 2) / u128::from(short_side);
        let pixels = short * long;
        let tokens = 2 * length.as_millis() * pixels * rate / (1024 * 1000);
        Some(u64::try_from(tokens).unwrap_or(u64::MAX))
    }

    /// Whether it makes a take of `length`.
    pub fn fits(&self, length: Duration) -> bool {
        (self.min_length..=self.max_length).contains(&length)
    }

    /// The model `step` is priced and recorded as: the model, with the
    /// tier's resolution where the request names one
    /// (`bytedance/seedance-2.5/video-edit/480p`).
    pub fn priced_model(&self, step: TakeStep) -> String {
        match self.tier(step).and_then(|tier| tier.resolution.as_deref()) {
            Some(resolution) => format!("{}/{resolution}", self.id.model()),
            None => self.id.model().to_owned(),
        }
    }
}

/// The take model a take uses: its own when still offered, else the
/// channel's when still offered, else the first offered (the default).
pub fn take_model_of<'a>(
    models: &'a [TakeModel],
    channel: Option<&ClipModelRef>,
    own: Option<&ClipModelRef>,
) -> Option<&'a TakeModel> {
    let offered = |wanted: &ClipModelRef| models.iter().find(|model| model.id == *wanted);
    own.and_then(offered)
        .or_else(|| channel.and_then(offered))
        .or_else(|| models.first())
}

/// A file handed to the provider ahead of a submission, as the provider
/// refers to it (Higgsfield: the public URL of its upload). Saved with the
/// job, so a resubmission sends the very same request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedFile(pub String);

/// A file to hand to the provider: its bytes and media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeInput<'a> {
    pub bytes: &'a [u8],
    /// `video/mp4`, `image/jpeg`.
    pub mime: &'a str,
}

/// What to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeRequest {
    /// The model, as the provider names it.
    pub model: String,
    pub step: TakeStep,
    /// The tier's resolution, for models that take one.
    pub resolution: Option<String>,
    /// The take's prompt, as the user left it.
    pub prompt: String,
    /// The approved control video, with the take's voice inside.
    pub video: StagedFile,
    /// The persona's look.
    pub look: StagedFile,
    /// For a final that finishes a draft: the draft's request. The model
    /// reuses the draft's prompt and inputs, so they are not sent again.
    pub draft_task: Option<ClipHandle>,
}

/// Where a take is on the provider's side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeStatus {
    Queued,
    Running,
    Done(TakeDone),
    /// It ended without a take; the failure says why.
    Failed(ProviderFailure),
}

/// A finished take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeDone {
    /// Where to download it.
    pub video: String,
    /// What the provider says it used, when it says (ModelArk: the
    /// video's tokens), recorded in place of the estimate.
    pub usage: Option<Metered>,
    /// The seed that made it, when the provider says.
    pub seed: Option<i64>,
}

impl TakeDone {
    pub fn at(video: impl Into<String>) -> Self {
        Self {
            video: video.into(),
            usage: None,
            seed: None,
        }
    }
}

/// Makes takes. Calls the network and blocks, so it runs inside a job.
pub trait TakeGenerator: Send + Sync {
    fn provider(&self) -> Provider;

    /// The models it offers, the provider's default first.
    fn models(&self) -> Vec<TakeModel>;

    /// Hands a file to the provider.
    fn stage(&self, key: &ApiKey, input: TakeInput<'_>) -> Result<StagedFile, ProviderFailure>;

    /// What the provider says `request` costs, when it says.
    fn quote(&self, key: &ApiKey, request: &TakeRequest) -> Option<Money>;

    /// Starts the take. Where the provider is `idempotent`, submitting the
    /// same request again with the same `submission` id returns the first
    /// submission instead of starting (and charging for) another.
    fn submit(
        &self,
        key: &ApiKey,
        request: &TakeRequest,
        submission: &str,
    ) -> Result<ClipHandle, ProviderFailure>;

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<TakeStatus, ProviderFailure>;

    /// Downloads a finished take from where `TakeStatus::Done` said.
    fn download(&self, key: &ApiKey, video: &str) -> Result<GeneratedClip, ProviderFailure>;

    /// Whether a resubmission with the same id is safe (Higgsfield takes
    /// an idempotency key). Otherwise a submission whose answer was lost is
    /// looked for with `find_submitted` before sending it again.
    fn idempotent(&self) -> bool {
        true
    }

    /// Every request the provider took like `request` since `since`, for
    /// a provider that is not `idempotent`: the caller leaves out the ones
    /// it knows and decides.
    fn find_submitted(
        &self,
        _key: &ApiKey,
        _request: &TakeRequest,
        _since: SystemTime,
    ) -> Result<Vec<ClipHandle>, ProviderFailure> {
        Ok(Vec::new())
    }

    /// Whether staged files stay where they are until `unstage` removes
    /// them (the user's own bucket); otherwise the provider clears them.
    fn keeps_staged(&self) -> bool {
        false
    }

    /// Removes a staged file. Removing one already gone succeeds.
    fn unstage(&self, _key: &ApiKey, _file: &StagedFile) -> Result<(), ProviderFailure> {
        Ok(())
    }

    /// How long to wait before status check number `polls` (from 0): a
    /// take takes minutes, so from five seconds up to fifteen.
    fn poll_delay(&self, polls: u32) -> Duration {
        let millis = 5_000.0 * 1.5_f64.powi(polls.min(10) as i32);
        Duration::from_millis(millis.min(15_000.0) as u64)
    }
}

impl<T: TakeGenerator + ?Sized> TakeGenerator for Arc<T> {
    fn provider(&self) -> Provider {
        (**self).provider()
    }

    fn models(&self) -> Vec<TakeModel> {
        (**self).models()
    }

    fn stage(&self, key: &ApiKey, input: TakeInput<'_>) -> Result<StagedFile, ProviderFailure> {
        (**self).stage(key, input)
    }

    fn quote(&self, key: &ApiKey, request: &TakeRequest) -> Option<Money> {
        (**self).quote(key, request)
    }

    fn submit(
        &self,
        key: &ApiKey,
        request: &TakeRequest,
        submission: &str,
    ) -> Result<ClipHandle, ProviderFailure> {
        (**self).submit(key, request, submission)
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<TakeStatus, ProviderFailure> {
        (**self).status(key, handle)
    }

    fn download(&self, key: &ApiKey, video: &str) -> Result<GeneratedClip, ProviderFailure> {
        (**self).download(key, video)
    }

    fn idempotent(&self) -> bool {
        (**self).idempotent()
    }

    fn find_submitted(
        &self,
        key: &ApiKey,
        request: &TakeRequest,
        since: SystemTime,
    ) -> Result<Vec<ClipHandle>, ProviderFailure> {
        (**self).find_submitted(key, request, since)
    }

    fn keeps_staged(&self) -> bool {
        (**self).keeps_staged()
    }

    fn unstage(&self, key: &ApiKey, file: &StagedFile) -> Result<(), ProviderFailure> {
        (**self).unstage(key, file)
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

    fn model(name: &str, modes: &[TakeMode], draft: bool) -> TakeModel {
        TakeModel {
            id: ClipModelRef::new(Provider::Higgsfield, name).unwrap(),
            name: name.into(),
            modes: modes.to_vec(),
            draft: draft.then(|| TakeTier {
                resolution: Some("480p".into()),
                height: 480,
            }),
            final_tier: TakeTier {
                resolution: draft.then(|| "1080p".into()),
                height: 1080,
            },
            finishes_draft: false,
            draft_lifetime: None,
            look_assets: false,
            token_frame_rate: None,
            keeps_framing: false,
            min_length: secs(4),
            max_length: secs(30),
            max_video_bytes: 100_000_000,
            max_image_bytes: 10_000_000,
        }
    }

    #[test]
    fn steps_have_stable_codes() {
        for step in TakeStep::ALL {
            assert_eq!(TakeStep::from_code(step.code()), Some(step));
        }
        assert_eq!(TakeStep::from_code("preview"), None);
    }

    #[test]
    fn a_model_takes_its_modes_and_starts_in_the_first() {
        let kling = model("kling/mc/pro", &[TakeMode::Recording], false);
        assert!(kling.takes(TakeMode::Recording));
        assert!(!kling.takes(TakeMode::Mesh));
        assert_eq!(kling.default_mode(), TakeMode::Recording);
        let seedance = model("seedance/edit", &[TakeMode::Mesh, TakeMode::Depth], true);
        assert_eq!(seedance.default_mode(), TakeMode::Mesh);
        assert!(seedance.takes(TakeMode::Depth));
        assert!(!seedance.takes(TakeMode::Recording));
    }

    #[test]
    fn a_draft_comes_first_only_where_the_model_has_a_cheap_tier() {
        let kling = model("kling/mc/pro", &[TakeMode::Recording], false);
        assert_eq!(kling.first_step(), TakeStep::Final);
        assert_eq!(kling.tier(TakeStep::Draft), None);
        assert_eq!(kling.tier(TakeStep::Final).unwrap().height, 1080);
        assert_eq!(kling.priced_model(TakeStep::Final), "kling/mc/pro");
        let seedance = model("seedance/edit", &[TakeMode::Mesh], true);
        assert_eq!(seedance.first_step(), TakeStep::Draft);
        assert_eq!(seedance.priced_model(TakeStep::Draft), "seedance/edit/480p");
        assert_eq!(
            seedance.priced_model(TakeStep::Final),
            "seedance/edit/1080p"
        );
    }

    #[test]
    fn a_draft_lasts_its_lifetime_only_where_the_final_finishes_it() {
        let sent = SystemTime::UNIX_EPOCH + secs(1_800_000_000);
        let mut seedance = model("seedance/edit", &[TakeMode::Mesh], true);
        seedance.draft_lifetime = Some(secs(7 * 24 * 3600));
        assert_eq!(seedance.draft_deadline(sent), None, "a final is a new take");
        seedance.finishes_draft = true;
        assert_eq!(
            seedance.draft_deadline(sent),
            Some(sent + secs(7 * 24 * 3600))
        );
    }

    #[test]
    fn tokens_count_the_seconds_in_and_out_at_the_tiers_frame() {
        let mut seedance = model("seedance/edit", &[TakeMode::Mesh], true);
        let ten = secs(10);
        assert_eq!(
            seedance.estimated_tokens(TakeStep::Final, ten, (720, 1280)),
            None,
            "billed per second"
        );
        seedance.token_frame_rate = Some(24);
        // 1080 × 1920, 24 frames a second, 20 seconds in all.
        assert_eq!(
            seedance.estimated_tokens(TakeStep::Final, ten, (720, 1280)),
            Some(20 * 1080 * 1920 * 24 / 1024)
        );
        assert_eq!(
            seedance.estimated_tokens(TakeStep::Final, ten, (1280, 720)),
            Some(20 * 1080 * 1920 * 24 / 1024),
            "landscape the same"
        );
        // 480 × 853 (ModelArk's 854 rounds the other way).
        assert_eq!(
            seedance.estimated_tokens(TakeStep::Draft, ten, (720, 1280)),
            Some(20 * 480 * 853 * 24 / 1024)
        );
        let kling = model("kling/mc/pro", &[TakeMode::Recording], false);
        assert_eq!(
            kling.estimated_tokens(TakeStep::Draft, ten, (720, 1280)),
            None
        );
    }

    #[test]
    fn lengths_count_inclusively() {
        let kling = model("kling/mc/pro", &[TakeMode::Recording], false);
        assert!(kling.fits(secs(4)));
        assert!(kling.fits(secs(30)));
        assert!(!kling.fits(Duration::from_millis(30_001)));
        assert!(!kling.fits(Duration::from_millis(3_999)));
    }

    #[test]
    fn a_take_uses_its_own_model_then_the_channels_then_the_default() {
        let models = [
            model("kling/mc/pro", &[TakeMode::Recording], false),
            model("seedance/edit", &[TakeMode::Mesh], true),
        ];
        let kling = models[0].id.clone();
        let seedance = models[1].id.clone();
        let gone = ClipModelRef::new(Provider::Higgsfield, "retired/model").unwrap();
        let pick = |channel, own| take_model_of(&models, channel, own).map(|m| m.id.clone());
        assert_eq!(pick(None, None), Some(kling.clone()));
        assert_eq!(pick(Some(&seedance), None), Some(seedance.clone()));
        assert_eq!(pick(Some(&seedance), Some(&kling)), Some(kling.clone()));
        assert_eq!(pick(Some(&gone), Some(&gone)), Some(kling.clone()));
        assert_eq!(pick(Some(&seedance), Some(&gone)), Some(seedance));
        assert_eq!(take_model_of(&[], Some(&kling), None), None);
    }

    #[test]
    fn polling_starts_at_five_seconds_and_slows_to_fifteen() {
        struct Polls;
        impl TakeGenerator for Polls {
            fn provider(&self) -> Provider {
                Provider::Higgsfield
            }
            fn models(&self) -> Vec<TakeModel> {
                Vec::new()
            }
            fn stage(&self, _: &ApiKey, _: TakeInput<'_>) -> Result<StagedFile, ProviderFailure> {
                unreachable!()
            }
            fn quote(&self, _: &ApiKey, _: &TakeRequest) -> Option<Money> {
                unreachable!()
            }
            fn submit(
                &self,
                _: &ApiKey,
                _: &TakeRequest,
                _: &str,
            ) -> Result<ClipHandle, ProviderFailure> {
                unreachable!()
            }
            fn status(&self, _: &ApiKey, _: &ClipHandle) -> Result<TakeStatus, ProviderFailure> {
                unreachable!()
            }
            fn download(&self, _: &ApiKey, _: &str) -> Result<GeneratedClip, ProviderFailure> {
                unreachable!()
            }
        }
        let delays: Vec<_> = (0..5).map(|n| Polls.poll_delay(n).as_millis()).collect();
        assert_eq!(delays, [5_000, 7_500, 11_250, 15_000, 15_000]);
    }
}
