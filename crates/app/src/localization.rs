//! Localized versions (#184): a project made again for a channel in
//! another language or country. Claude adapts the source narration
//! sentence by sentence; the version starts with the adapted script, copies
//! of the source's scenes and media, and what the source cut did with them.
//! Once the version has its own narration, its scenes are placed on it and
//! its cut is built from the carried one.
//!
//! Making a version calls Claude, so it runs as a job, rendered when it is
//! asked for like a script; the new project only exists once the
//! adaptation is usable, saved with everything it starts with at once.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, CarriedCut, CastMember, CastRecord, Channel, ChannelId, ChannelRepository,
    ContentLanguage, CostPurpose, CostRecordId, Country, Gap, GeneratedScript, Generation,
    GenerationId, Job, JobFailure, JobFailureKind, JobId, JobKind, Localization,
    LocalizationRepository, MediaAsset, MediaAssetId, MediaAssetRepository, MusicTrack,
    MusicTrackRepository, Narration, NarrationRepository, NewVersion, PlacedScene, ProfileId,
    ProjectFiles, Provider, RenderedPrompt, RepositoryError, Scene, SceneClip, SceneImage,
    ScenePlan, ScenePlanId, ScenePlanRecord, ScenePlanRepository, SceneRecord, Script,
    ScriptRepository, SecretStore, SourceSentence, Stop, TemplateKind, TemplateUsed,
    TemplateValues, TemplateVariable, TemplateVersion, TemplateVersionId, TextEffort, TextFormat,
    TextGenerator, TextRequest, Theme, ThemeIdea, ThemeRepository, TimelineRepository, UiLanguage,
    VideoProject, VideoProjectId, adaptation_gaps, join_adapted, localized_cut, place_scenes,
    placed_scene_notes, scene_sentences, sentence_times, sentence_words, sentences, source_gaps,
};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::editor::project_cut;
use crate::jobs::{JobContext, JobHandler, Keeps, Work};
use crate::proxies::{ProxyKind, proxy_name};
use crate::scenes::clock;
use crate::{Bardo, Catalog, KeyState, ScriptError, TemplateError, Text};

/// The shape Claude answers in.
const ADAPTATION_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "title": {"type": "string"},
    "sentences": {"type": "array", "items": {"type": "string"}}
  },
  "required": ["title", "sentences"],
  "additionalProperties": false
}"#;

#[derive(Debug, Deserialize)]
struct Adapted {
    title: String,
    sentences: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalizationError {
    #[error("video project not found")]
    ProjectNotFound,
    #[error("channel not found")]
    ChannelNotFound,
    /// The channel has the source's language and country, or is its own.
    #[error("the channel speaks the project's language to its country")]
    SameAudience,
    /// The channel already has a version of the project.
    #[error("the channel already has a version of this project")]
    AlreadyMade,
    /// The source has no narration: its sentences are what is adapted.
    #[error("the project has no narration yet")]
    NoNarration,
    /// The source has no scene plan made on its narration.
    #[error("the project's scenes are not planned on its narration")]
    NoPlan,
    /// A version of the project is being made.
    #[error("a version of this project is being made")]
    Busy,
    /// An ad project has no versions in other languages: its product,
    /// brief and claims are not carried across.
    #[error("an ad project has no versions in other languages")]
    AdProject,
    /// The project is not a version.
    #[error("the project is not a version of another")]
    NotAVersion,
    /// The version's plan is not the one carried over, or it is already
    /// on the narration.
    #[error("there are no scenes to place on this narration")]
    NothingToPlace,
    /// Scenes, images or clips of the project are being made.
    #[error("the project's scenes are being worked on")]
    ScenesBusy,
    #[error("no {0} key saved")]
    MissingKey(Provider),
    #[error("over budget")]
    OverBudget(SpendEstimate),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl LocalizationError {
    /// What the projects screen says.
    pub fn message(&self) -> Text {
        match self {
            LocalizationError::ProjectNotFound => Text::ProjectNotFound,
            LocalizationError::ChannelNotFound => Text::ChannelNotFound,
            LocalizationError::SameAudience => Text::VersionSameAudience,
            LocalizationError::AlreadyMade => Text::VersionAlreadyMade,
            LocalizationError::NoNarration => Text::VersionNoNarration,
            LocalizationError::NoPlan => Text::VersionNoPlan,
            LocalizationError::Busy => Text::VersionBusy,
            LocalizationError::AdProject => Text::VersionAdProject,
            LocalizationError::NotAVersion | LocalizationError::NothingToPlace => {
                Text::PlaceScenesNothing
            }
            LocalizationError::ScenesBusy => Text::ScenesBusy,
            LocalizationError::MissingKey(_) => Text::ScriptMissingKey,
            LocalizationError::OverBudget(_) => Text::BudgetReachedTitle,
            LocalizationError::Template(error) => error.message(),
            LocalizationError::Repository(_) => Text::VersionNotSaved,
        }
    }
}

impl From<ScriptError> for LocalizationError {
    fn from(error: ScriptError) -> Self {
        match error {
            ScriptError::ChannelNotFound => LocalizationError::ChannelNotFound,
            ScriptError::MissingKey(provider) => LocalizationError::MissingKey(provider),
            ScriptError::OverBudget(estimate) => LocalizationError::OverBudget(estimate),
            ScriptError::Template(error) => LocalizationError::Template(error),
            ScriptError::Repository(error) => LocalizationError::Repository(error),
            ScriptError::Busy => LocalizationError::Busy,
            _ => LocalizationError::ProjectNotFound,
        }
    }
}

/// A channel a version can be made for.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionTarget {
    pub channel: ChannelId,
    pub name: String,
    pub language: ContentLanguage,
    pub country: Country,
    /// The channel's default persona, who narrates the version.
    pub narrator: Option<String>,
    /// What the adaptation would cost.
    pub estimate: SpendEstimate,
    /// The localization template the adaptation would use.
    pub template: u32,
}

/// A version made from the project.
#[derive(Debug, Clone, PartialEq)]
pub struct MadeVersion {
    pub project: VideoProject,
    pub channel: String,
    pub language: ContentLanguage,
    pub country: Country,
}

/// What a version was made from.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionSource {
    /// `None` once the source is gone.
    pub project: Option<VideoProjectId>,
    pub title: String,
    /// The source's channel, while it is there.
    pub channel: Option<String>,
    pub language: ContentLanguage,
    pub country: Country,
}

/// Why no version can be made yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionBlock {
    NoNarration,
    NoPlan,
    /// No other channel has another language or country.
    NoChannel,
    /// The project is an ad: its product and brief are not carried to
    /// another language.
    Ad,
}

/// The Script stage's "Versions in other languages".
#[derive(Debug, Clone, PartialEq)]
pub struct VersionsView {
    /// What the project is a version of, if it is one.
    pub source: Option<VersionSource>,
    /// The versions made from it, oldest first.
    pub made: Vec<MadeVersion>,
    /// The channels it can be made for now.
    pub targets: Vec<VersionTarget>,
    /// Why `targets` is empty, when it is.
    pub blocked: Option<VersionBlock>,
    /// The project's latest version job, and the channel it is for.
    pub job: Option<Job>,
    pub job_channel: Option<String>,
}

impl VersionsView {
    pub fn is_busy(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.state().is_active())
    }
}

/// What the Scenes stage of a version shows.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionScenes {
    /// Whether the carried scenes wait to be placed on the narration.
    pub can_place: bool,
    /// Once placed, the scenes worth a look.
    pub notes: Vec<(usize, PlacedScene)>,
}

/// The version job's payload. With `channel`, it makes a version of
/// `project` for that channel as project `version`; without, it adapts the
/// version `project` again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LocalizationPayload {
    project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    /// The source narration the prompt was rendered from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    narration: Option<String>,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

fn parse_id<T: From<uuid::Uuid>>(text: &str) -> Result<T, JobFailure> {
    uuid::Uuid::parse_str(text).map(T::from).map_err(unexpected)
}

impl LocalizationPayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a version payload serializes")
    }

    fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid version payload: {e}")))
    }

    fn of(job: &Job) -> Option<Self> {
        (job.kind() == JobKind::Localization)
            .then(|| Self::parse(job.payload()).ok())
            .flatten()
    }

    fn template(&self) -> Result<TemplateUsed, JobFailure> {
        Ok(TemplateUsed {
            id: parse_id::<TemplateVersionId>(&self.template)?,
            number: self.template_number,
        })
    }
}

/// Whether `job` adapts the version `project` (its id) again.
pub(crate) fn adapts_again(job: &Job, project: &str) -> bool {
    LocalizationPayload::of(job)
        .is_some_and(|payload| payload.channel.is_none() && payload.project == project)
}

/// What a version's prompt says about its source.
struct SourceFacts<'a> {
    language: ContentLanguage,
    country: Country,
    title: &'a str,
    sentences: &'a [SourceSentence],
    length: Duration,
}

/// The source sentences as Claude reads them: one per line, with its
/// number and when it is spoken.
fn sentence_lines(sentences: &[SourceSentence], length: Duration) -> String {
    sentences
        .iter()
        .enumerate()
        .map(|(index, sentence)| {
            let end = sentences.get(index + 1).map_or(length, |next| next.start);
            format!(
                "[{index}] {}-{} {}",
                clock(sentence.start),
                clock(end.max(sentence.start)),
                sentence.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The narration's sentences, as a version keeps them.
fn source_sentences(narration: &Narration) -> Vec<SourceSentence> {
    let text = narration.text.as_str();
    sentences(narration)
        .iter()
        .map(|sentence| SourceSentence {
            text: text[sentence.text.clone()]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            start: sentence.start,
        })
        .collect()
}

/// How long the narration runs.
fn narration_length(narration: &Narration) -> Duration {
    narration.duration.max(
        narration
            .words
            .as_slice()
            .last()
            .map_or(Duration::ZERO, |word| word.end),
    )
}

/// The Claude call that adapts a script from `rendered`.
fn adaptation_call(rendered: &RenderedPrompt) -> PlannedCall {
    PlannedCall::new(Provider::Claude, CostPurpose::Script, 1)
        .with_prompt(&rendered.instructions, &rendered.prompt)
}

/// Whether `channel` speaks to another audience than `language` in
/// `country`.
fn other_audience(channel: &Channel, language: ContentLanguage, country: Country) -> bool {
    channel.details.language() != language || channel.details.country() != country
}

/// A copy of `plan` for `project`: the current images and clips with new
/// generation records, prompts and model picks, the cast with its
/// portraits and the places, and which portraits each image was drawn
/// with; nothing waiting for review, no failures and no clip of an image
/// the scene no longer shows.
fn copy_plan(plan: &ScenePlan, project: VideoProjectId, now: SystemTime) -> ScenePlan {
    let copied = |generation: &Generation| Generation {
        id: GenerationId::new(),
        project,
        ..generation.clone()
    };
    let cast: Vec<CastMember> = plan
        .cast()
        .iter()
        .map(|member| {
            CastMember::restore(CastRecord {
                name: member.name.clone(),
                generated_description: member.generated_description().clone(),
                description: member.description().clone(),
                portrait: member.portrait().map(|portrait| SceneImage {
                    file: portrait.file.clone(),
                    generation: copied(&portrait.generation),
                }),
                failure: None,
            })
            .with_described_only(member.is_described_only())
        })
        .collect();
    // The copied portraits have records of their own: an image drawn with
    // a member's current portrait is one drawn with its copy.
    let copied_portrait = |member: usize, used: Option<GenerationId>| {
        let current = plan.cast().get(member).and_then(CastMember::portrait);
        match (
            used,
            current,
            cast.get(member).and_then(CastMember::portrait),
        ) {
            (Some(used), Some(current), Some(copy)) if used == current.generation.id => {
                Some(copy.generation.id)
            }
            _ => used,
        }
    };
    let scenes = plan
        .scenes()
        .iter()
        .map(|scene| {
            let image = scene.image().map(|image| SceneImage {
                file: image.file.clone(),
                generation: copied(&image.generation),
            });
            let clip = scene
                .clip()
                .filter(|_| !scene.is_clip_stale())
                .zip(image.as_ref())
                .map(|(clip, image)| SceneClip {
                    file: clip.file.clone(),
                    seconds: clip.seconds,
                    stretch: clip.stretch,
                    source_image: image.generation.id,
                    generation: copied(&clip.generation),
                });
            Scene::restore(SceneRecord {
                start: scene.start,
                end: scene.end,
                text: scene.text.clone(),
                generated_prompt: scene.generated_prompt().clone(),
                prompt: scene.prompt().clone(),
                image,
                pending: None,
                failure: None,
                motion_prompt: scene.own_motion_prompt().cloned(),
                clip_model: scene.clip_model().cloned(),
                clip,
                pending_clip: None,
                clip_failure: None,
            })
            .with_cast(scene.cast().to_vec())
            .with_planned_motion(scene.planned_motion().cloned())
            .with_place(scene.place())
            .with_portraits(
                scene.image_portraits().map(|portraits| {
                    portraits
                        .iter()
                        .map(|&(member, used)| (member, copied_portrait(member, used)))
                        .collect()
                }),
                None,
            )
            // An ad's scene keeps what it is, and a composed one its
            // picture (#278): the file is copied with the others.
            .with_kind(scene.kind(), scene.product_image(), scene.shot().cloned())
        })
        .collect();
    ScenePlan::restore(ScenePlanRecord {
        id: ScenePlanId::new(),
        project,
        owner: plan.owner,
        narration: plan.narration,
        generation: copied(&plan.generation),
        scenes,
        updated_at: now,
    })
    .with_cast(cast)
    .with_places(plan.places().to_vec())
}

/// Runs version jobs.
pub(crate) struct LocalizationHandler {
    pub(crate) owner: ProfileId,
    pub(crate) themes: Arc<dyn ThemeRepository>,
    pub(crate) channels: Arc<dyn ChannelRepository>,
    pub(crate) scripts: Arc<dyn ScriptRepository>,
    pub(crate) narrations: Arc<dyn NarrationRepository>,
    pub(crate) plans: Arc<dyn ScenePlanRepository>,
    pub(crate) timelines: Arc<dyn TimelineRepository>,
    pub(crate) media: Arc<dyn MediaAssetRepository>,
    pub(crate) tracks: Arc<dyn MusicTrackRepository>,
    pub(crate) localizations: Arc<dyn LocalizationRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for LocalizationHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload = LocalizationPayload::parse(payload)?;
        match &payload.channel {
            Some(channel) => {
                let channel = parse_id::<ChannelId>(channel)?;
                let (id, stop, stopping) = (cx.id(), cx.stop(), cx.stopping());
                self.make(&payload, channel, id, &stop, &stopping, cx)
            }
            None => self.adapt_again(&payload, cx.id(), &cx.stop()),
        }
    }
}

/// Fails the job: the source changed or went away since it was asked for.
fn source_gone(detail: &str) -> JobFailure {
    JobFailure::new(JobFailureKind::Unexpected, detail.to_owned())
}

impl LocalizationHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::Claude)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no Claude key is saved"))
    }

    /// Asks Claude for the adaptation. An answer that was lost or cut may
    /// still have been billed: it is recorded on `channel` and `project`.
    fn ask(
        &self,
        payload: &LocalizationPayload,
        job: JobId,
        channel: Option<ChannelId>,
        project: Option<VideoProjectId>,
        stop: &Stop,
    ) -> Result<bardo_domain::GeneratedText, JobFailure> {
        let key = self.key()?;
        let request = TextRequest {
            effort: TextEffort::Everyday,
            // The adaptation is about as long as the script it adapts,
            // which the prompt carries, at about three characters a token.
            // A language that takes more tokens fits in the room the
            // adapter leaves around this size. A long script needs more
            // than the usual room.
            answer_tokens: Some((payload.prompt.chars().count() as u64).div_ceil(3)),
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            images: Vec::new(),
            format: TextFormat::Json {
                schema: ADAPTATION_SCHEMA.to_owned(),
            },
        };
        let paid_call = PlannedCall::text(CostPurpose::Script, &request);
        let generated = {
            let _paying = self.costs.paying(job, &paid_call, channel, project)?;
            self.text.generate_until(&key, &request, stop)
        };
        generated.map_err(|failure| {
            self.costs
                .record_failed(&failure, &paid_call, Some(job), channel, project);
            JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
        })
    }

    /// Records what the adaptation cost, on `channel` and, once it exists,
    /// `project`, under `id`: recording the same id again changes nothing.
    fn bill(
        &self,
        id: CostRecordId,
        generated: &bardo_domain::GeneratedText,
        job: JobId,
        channel: Option<ChannelId>,
        project: Option<VideoProjectId>,
    ) {
        self.costs.record_as(
            id,
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::Script,
                usage: generated.usage.into(),
                job: Some(job),
                reported: None,
            },
            channel,
            project,
        );
    }

    /// The adaptation made `version`: its record in Costs, on the channel
    /// alone since it arrived, counts on the version from now on, and the
    /// answer is let go. A run cut short before this finds the version
    /// made on its next run and does it then.
    fn applied(&self, cost: Option<CostRecordId>, version: VideoProjectId, answer: &mut dyn Keeps) {
        if let Some(cost) = cost {
            self.costs.move_to_project(cost, version);
        }
        answer.settle();
    }

    /// Makes the version `payload` asked for, for `channel`.
    ///
    /// The adaptation is kept (`answer`) from the moment it is paid for
    /// and recorded in Costs until the version is started: a run told to
    /// stop before that (the user cancelled, so no project appears in the
    /// other channel behind their back; or the app is closing), cut short
    /// while it copies the source's files, or unable to save the version
    /// leaves it for the job's next run, which does not ask Claude again.
    /// An adaptation that turns out not to be usable is let go, so that
    /// the next run asks again.
    fn make(
        &self,
        payload: &LocalizationPayload,
        channel: ChannelId,
        job: JobId,
        stop: &Stop,
        should_stop: &dyn Fn() -> bool,
        answer: &mut dyn Keeps,
    ) -> Result<(), JobFailure> {
        let source_id = parse_id::<VideoProjectId>(&payload.project)?;
        let version = parse_id::<VideoProjectId>(
            payload
                .version
                .as_deref()
                .ok_or_else(|| JobFailure::unexpected("the version payload has no version"))?,
        )?;
        // An earlier attempt may have saved the version and stopped before
        // the queue recorded it as done.
        if self
            .localizations
            .localization(version)
            .map_err(unexpected)?
            .is_some()
        {
            // Cut short after the version was saved: its adaptation may
            // still be counted on the channel alone.
            if let Some((_, cost)) = answer.kept() {
                self.applied(cost, version, answer);
            }
            return Ok(());
        }
        let source = self
            .themes
            .project(source_id)
            .map_err(unexpected)?
            .ok_or_else(|| source_gone("the project was removed"))?;
        let target = self
            .channels
            .get(channel)
            .map_err(unexpected)?
            .ok_or_else(|| source_gone("the channel was removed"))?;
        let source_channel = self
            .channels
            .get(source.channel)
            .map_err(unexpected)?
            .ok_or_else(|| source_gone("the project's channel was removed"))?;
        let narration = self
            .narrations
            .narration(source.id)
            .map_err(unexpected)?
            .filter(|narration| Some(narration.id.to_string()) == payload.narration)
            .ok_or_else(|| {
                source_gone("the narration changed since the version was asked for; make it again")
            })?;
        let plan = self
            .plans
            .scene_plan(source.id)
            .map_err(unexpected)?
            .filter(|plan| !plan.is_stale(Some(&narration)))
            .ok_or_else(|| source_gone("the scenes are no longer planned on the narration"))?;
        // Asked again from the Jobs screen, the job skips `make_version`:
        // the channel must still take a version.
        let versions = self.localizations.versions(source.id).map_err(unexpected)?;
        if versions.iter().any(|made| made.channel == target.id) {
            return Err(source_gone(
                "that channel already has a version of this project",
            ));
        }
        let came_from = match self
            .localizations
            .localization(source.id)
            .map_err(unexpected)?
            .and_then(|localization| localization.source)
        {
            Some(id) => self
                .themes
                .project(id)
                .map_err(unexpected)?
                .map(|project| project.channel),
            None => None,
        };
        if target.id == source.channel
            || Some(target.id) == came_from
            || !other_audience(
                &target,
                source_channel.details.language(),
                source_channel.details.country(),
            )
        {
            return Err(source_gone(
                "the channel speaks the project's language to its country",
            ));
        }
        let spoken = sentences(&narration);
        let gaps = source_gaps(&narration, &spoken);

        // The adaptation is billed on the channel when it arrives, and its
        // record moved to the version once that exists: an unusable,
        // stopped or failed one stays on the channel. Each answer has a
        // record of its own, kept with it: a job that asks again after an
        // answer it could not use has paid twice.
        let (generated, on_channel) = match answer.kept() {
            Some(kept) => kept,
            None => {
                let generated = self.ask(payload, job, Some(target.id), None, stop)?;
                let cost = CostRecordId::new();
                self.bill(cost, &generated, job, Some(target.id), None);
                answer.keep(&generated, cost);
                (generated, Some(cost))
            }
        };
        let saved = (|| -> Result<bool, JobFailure> {
            let adapted: Adapted =
                serde_json::from_str(&generated.text).map_err(|e| not_usable(&e.to_string()))?;
            let adaptation =
                join_adapted(&adapted.sentences, &gaps).map_err(|e| not_usable(&e.to_string()))?;
            let angle = self
                .themes
                .theme(source.theme)
                .map_err(unexpected)?
                .map(|theme| theme.idea().angle().to_owned())
                .unwrap_or_default();
            let idea = ThemeIdea::new(&adapted.title, &angle)
                .or_else(|_| ThemeIdea::new(&adapted.title, ""))
                .map_err(|e| not_usable(&format!("the title is not usable ({e:?})")))?;
            if should_stop() {
                return Ok(false);
            }

            let now = SystemTime::now();
            let mut theme = Theme::suggested(
                self.owner,
                target.id,
                source.niche.clone(),
                idea,
                now,
                0,
                None,
            );
            let mut project = theme
                .approve(now)
                .map_err(|e| JobFailure::unexpected(e.to_string()))?;
            project.id = version;
            // Its pictures have the source's shape.
            project.format = Some(source.format(source_channel.details.format()));
            // And its length: a version tells the same video.
            project.length = source.length;
            // And its style: the images it carries were drawn in it, and
            // the ones drawn next must belong with them (#259).
            project.style = Some(source.style(source_channel.details.style()));
            let generation = Generation {
                id: GenerationId::new(),
                owner: self.owner,
                project: version,
                provider: Provider::Claude,
                model: generated.model.clone(),
                template: payload.template()?,
                instructions: payload.instructions.clone(),
                prompt: payload.prompt.clone(),
                output: adaptation.text,
                usage: generated.usage,
                generated_at: now,
                job: Some(job),
            };
            let script = Script::first(
                GeneratedScript::new(generation)
                    .map_err(|e| not_usable(&format!("the script is not usable ({e:?})")))?,
                now,
            );

            // What the source cut does with its scenes and sounds, now.
            let tracks = self.tracks.music_tracks(source.id).map_err(unexpected)?;
            let mut assets = self.media.media_assets(source.id).map_err(unexpected)?;
            assets.retain(|asset| !tracks.holds_back(asset.id));
            let music = tracks
                .in_use()
                .and_then(|track| assets.iter().find(|asset| asset.id == track.asset))
                .cloned();
            let timeline = self
                .timelines
                .saved_timeline(source.id)
                .map_err(unexpected)?;
            // Where the source's performed clips play, the version shows
            // the scenes a rough cut would (#193).
            let performed = crate::editor::PerformedCut::of(
                &self.media.performed_clips(source.id).map_err(unexpected)?,
                &assets,
                &narration,
                self.media
                    .performed_in_rough_cut(source.id)
                    .map_err(unexpected)?,
            );
            let (cut, _) = project_cut(
                &source,
                Some(&source_channel),
                &plan,
                &narration,
                timeline.as_ref(),
                &assets,
                music.as_ref(),
                &performed,
            );
            let carried = CarriedCut::of(&cut, &plan);

            let new_plan = copy_plan(&plan, version, now);
            let mut ids: HashMap<MediaAssetId, MediaAssetId> = HashMap::new();
            // Performed clips are lip-synced to the source's narration: the
            // version leaves them out.
            let media: Vec<MediaAsset> = assets
                .iter()
                .filter(|asset| asset.source != bardo_domain::AssetSource::Performed)
                .map(|asset| {
                    let id = MediaAssetId::new();
                    ids.insert(asset.id, id);
                    MediaAsset {
                        id,
                        project: version,
                        ..asset.clone()
                    }
                })
                .collect();
            let copied_tracks: Vec<MusicTrack> = tracks
                .all()
                .iter()
                .filter_map(|track| {
                    Some(MusicTrack {
                        asset: *ids.get(&track.asset)?,
                        project: version,
                        ..track.clone()
                    })
                })
                .collect();
            let localization = Localization {
                project: version,
                owner: self.owner,
                source: Some(source.id),
                source_title: source.title.clone(),
                source_language: source_channel.details.language(),
                source_country: source_channel.details.country(),
                source_length: narration_length(&narration),
                sentences: source_sentences(&narration),
                plan: new_plan.id,
                scene_sentences: scene_sentences(&plan, &spoken),
                cut: carried,
                created_at: now,
            };

            let mut names: Vec<String> = new_plan
                .files()
                .map(str::to_owned)
                .chain(media.iter().map(|asset| asset.file.clone()))
                .collect();
            names.sort();
            names.dedup();
            let mut copied: Vec<String> = Vec::new();
            let remove_copies = |copied: &[String]| {
                for name in copied {
                    let _ = self.files.remove(version, name);
                }
            };
            for name in &names {
                if let Err(error) = self.files.copy_from(source.id, name, version) {
                    remove_copies(&copied);
                    return Err(JobFailure::unexpected(format!(
                        "could not copy {name} into the version: {error}"
                    )));
                }
                copied.push(name.clone());
                // Previews made for the editor come along when they are there.
                for kind in ProxyKind::ALL {
                    let proxy = proxy_name(name, kind);
                    if self.files.exists(source.id, &proxy)
                        && self.files.copy_from(source.id, &proxy, version).is_ok()
                    {
                        copied.push(proxy);
                    }
                }
            }
            if should_stop() {
                remove_copies(&copied);
                return Ok(false);
            }
            let starts = adaptation.starts;
            if let Err(error) = self.localizations.start_version(&NewVersion {
                theme,
                project,
                script,
                starts,
                plan: new_plan,
                media,
                tracks: copied_tracks,
                localization,
            }) {
                remove_copies(&copied);
                return Err(unexpected(error));
            }
            Ok(true)
        })();
        match &saved {
            Ok(true) => self.applied(on_channel, version, answer),
            Err(failure) if failure.kind == JobFailureKind::UnexpectedAnswer => answer.settle(),
            // Stopped, or the version could not be saved: kept.
            Ok(false) | Err(_) => {}
        }
        saved.map(|_| ())
    }

    /// Adapts the version `payload` names again; the new adaptation waits
    /// for review.
    fn adapt_again(
        &self,
        payload: &LocalizationPayload,
        job: JobId,
        stop: &Stop,
    ) -> Result<(), JobFailure> {
        let project = parse_id::<VideoProjectId>(&payload.project)?;
        let saved = self.scripts.script(project).map_err(unexpected)?;
        if saved.as_ref().is_some_and(|script| {
            script.source().generation().job == Some(job)
                || script
                    .pending()
                    .is_some_and(|p| p.generation().job == Some(job))
        }) {
            return Ok(());
        }
        let localization = self
            .localizations
            .localization(project)
            .map_err(unexpected)?
            .ok_or_else(|| source_gone("the project is not a version"))?;
        let gaps = match &saved {
            Some(script) => {
                let generation = script.source().generation();
                match self
                    .localizations
                    .adapted_starts(generation.id)
                    .map_err(unexpected)?
                {
                    Some(starts) if starts.len() == localization.sentences.len() => {
                        adaptation_gaps(&generation.output, &starts)
                    }
                    _ => vec![Gap::Space; localization.sentences.len().saturating_sub(1)],
                }
            }
            None => vec![Gap::Space; localization.sentences.len().saturating_sub(1)],
        };
        let channel = self
            .themes
            .project(project)
            .map_err(unexpected)?
            .map(|project| project.channel);
        let generated = self.ask(payload, job, channel, Some(project), stop)?;
        self.bill(CostRecordId::new(), &generated, job, channel, Some(project));
        let adapted: Adapted =
            serde_json::from_str(&generated.text).map_err(|e| not_usable(&e.to_string()))?;
        let adaptation =
            join_adapted(&adapted.sentences, &gaps).map_err(|e| not_usable(&e.to_string()))?;
        let now = SystemTime::now();
        let generation = Generation {
            id: GenerationId::new(),
            owner: self.owner,
            project,
            provider: Provider::Claude,
            model: generated.model,
            template: payload.template()?,
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            output: adaptation.text,
            usage: generated.usage,
            generated_at: now,
            job: Some(job),
        };
        let id = generation.id;
        let generated = GeneratedScript::new(generation)
            .map_err(|e| not_usable(&format!("the script is not usable ({e:?})")))?;
        // Read again: the user may have edited the script meanwhile.
        let script = match self.scripts.script(project).map_err(unexpected)? {
            Some(mut script) => {
                script.offer(generated, now);
                script
            }
            None => Script::first(generated, now),
        };
        self.localizations
            .save_adaptation(&script, id, &adaptation.starts)
            .map_err(unexpected)
    }
}

/// Claude's adaptation cannot be used.
fn not_usable(detail: &str) -> JobFailure {
    JobFailure::new(
        JobFailureKind::UnexpectedAnswer,
        format!("Claude: the adaptation is not usable ({detail})"),
    )
}

impl Bardo {
    fn version_project(&self, id: VideoProjectId) -> Result<VideoProject, LocalizationError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(LocalizationError::ProjectNotFound)
    }

    /// The version jobs of `source`, oldest first, with their payloads.
    fn version_jobs(&self, source: VideoProjectId) -> Vec<(Job, LocalizationPayload)> {
        let source = source.to_string();
        self.jobs()
            .into_iter()
            .filter_map(|job| {
                let payload = LocalizationPayload::of(&job)?;
                (payload.channel.is_some() && payload.project == source).then_some((job, payload))
            })
            .collect()
    }

    /// The localization template filled for a version in `target`'s
    /// channel (narrated by its narrator) of `source`.
    fn render_localization(
        &self,
        target: &VideoProject,
        source: &SourceFacts<'_>,
        template: &TemplateVersion,
    ) -> Result<RenderedPrompt, ScriptError> {
        let values = self.script_values(target)?;
        let english = Catalog::load(UiLanguage::EnUs);
        let pick = |variable: TemplateVariable| values.get(&variable).cloned().unwrap_or_default();
        let values = TemplateValues::from([
            (
                TemplateVariable::ChannelName,
                pick(TemplateVariable::ChannelName),
            ),
            (
                TemplateVariable::ChannelNiche,
                pick(TemplateVariable::ChannelNiche),
            ),
            (TemplateVariable::Language, pick(TemplateVariable::Language)),
            (TemplateVariable::Country, pick(TemplateVariable::Country)),
            (TemplateVariable::Persona, pick(TemplateVariable::Persona)),
            (
                TemplateVariable::SourceLanguage,
                english
                    .get(Text::ContentLanguageName(source.language))
                    .into_owned(),
            ),
            (
                TemplateVariable::SourceCountry,
                english.get(Text::CountryName(source.country)).into_owned(),
            ),
            (TemplateVariable::ThemeTitle, source.title.to_owned()),
            (
                TemplateVariable::NarrationSentences,
                sentence_lines(source.sentences, source.length),
            ),
        ]);
        template.body.render(&values).map_err(|missing| {
            // Every localization variable has a value above.
            ScriptError::Repository(RepositoryError(Box::new(missing)))
        })
    }

    /// The project `source` would start as in `channel`, to fill the
    /// template with: the channel's facts and narrator.
    fn planned_version(source: &VideoProject, channel: &Channel) -> VideoProject {
        VideoProject {
            id: VideoProjectId::new(),
            channel: channel.id,
            persona: None,
            ..source.clone()
        }
    }

    /// The channel of the project `project` is a version of: a version is
    /// not made back for it.
    fn came_from(&self, project: VideoProjectId) -> Result<Option<ChannelId>, RepositoryError> {
        Ok(
            match self
                .localizations
                .localization(project)?
                .and_then(|localization| localization.source)
            {
                Some(id) => self.themes.project(id)?.map(|project| project.channel),
                None => None,
            },
        )
    }

    /// The Script stage's versions section of `project`.
    pub fn versions(&self, project: VideoProjectId) -> Result<VersionsView, LocalizationError> {
        let project = self.version_project(project)?;
        let channel_name = |id: ChannelId| -> Result<Option<String>, RepositoryError> {
            Ok(self
                .channels
                .get(id)?
                .map(|channel| channel.details.name().to_owned()))
        };
        let localization = self.localizations.localization(project.id)?;
        let source = match &localization {
            Some(localization) => {
                let from = match localization.source {
                    Some(id) => self.themes.project(id)?,
                    None => None,
                };
                Some(VersionSource {
                    project: from.as_ref().map(|project| project.id),
                    title: localization.source_title.clone(),
                    channel: match &from {
                        Some(project) => channel_name(project.channel)?,
                        None => None,
                    },
                    language: localization.source_language,
                    country: localization.source_country,
                })
            }
            None => None,
        };

        let mut made = Vec::new();
        for version in self.localizations.versions(project.id)? {
            let Some(channel) = self.channels.get(version.channel)? else {
                continue;
            };
            made.push(MadeVersion {
                channel: channel.details.name().to_owned(),
                language: channel.details.language(),
                country: channel.details.country(),
                project: version,
            });
        }

        let own = self
            .channels
            .get(project.channel)?
            .ok_or(LocalizationError::ProjectNotFound)?;
        let came_from = self.came_from(project.id)?;
        let narration = self.narrations.narration(project.id)?;
        let plan = self.scene_plans.scene_plan(project.id)?;
        let channels: Vec<Channel> = self
            .channels
            .list(self.profile.id)?
            .into_iter()
            .filter(|channel| {
                channel.id != project.channel
                    && Some(channel.id) != came_from
                    && other_audience(channel, own.details.language(), own.details.country())
                    && !made.iter().any(|made| made.project.channel == channel.id)
            })
            .collect();
        let blocked = match (&narration, &plan) {
            _ if project.is_ad() => Some(VersionBlock::Ad),
            (None, _) => Some(VersionBlock::NoNarration),
            (Some(narration), plan)
                if plan
                    .as_ref()
                    .is_none_or(|plan| plan.is_stale(Some(narration))) =>
            {
                Some(VersionBlock::NoPlan)
            }
            _ if channels.is_empty() => Some(VersionBlock::NoChannel),
            _ => None,
        };
        let mut targets = Vec::new();
        if let (None, Some(narration)) = (blocked, &narration) {
            let template = self.current_template(TemplateKind::Localization)?;
            let sentences = source_sentences(narration);
            let facts = SourceFacts {
                language: own.details.language(),
                country: own.details.country(),
                title: &project.title,
                sentences: &sentences,
                length: narration_length(narration),
            };
            for channel in channels {
                let planned = Self::planned_version(&project, &channel);
                let rendered = self.render_localization(&planned, &facts, &template)?;
                targets.push(VersionTarget {
                    channel: channel.id,
                    name: channel.details.name().to_owned(),
                    language: channel.details.language(),
                    country: channel.details.country(),
                    narrator: self
                        .narrator(&planned)?
                        .map(|persona| persona.details.name().to_owned()),
                    estimate: self.estimate(&[adaptation_call(&rendered)])?,
                    template: template.number,
                });
            }
        }

        let latest = self.version_jobs(project.id).pop();
        let job_channel = match latest.as_ref().and_then(|(_, p)| p.channel.as_deref()) {
            Some(id) => match uuid::Uuid::parse_str(id) {
                Ok(id) => channel_name(ChannelId::from(id))?,
                Err(_) => None,
            },
            None => None,
        };
        Ok(VersionsView {
            source,
            made,
            targets,
            blocked,
            job: latest.map(|(job, _)| job),
            job_channel,
        })
    }

    /// Starts a job in which Claude adapts `project` for `channel`, which
    /// then becomes a new project there. Past Claude's budget it needs
    /// `consent`.
    pub fn make_version(
        &self,
        project: VideoProjectId,
        channel: ChannelId,
        consent: BudgetConsent,
    ) -> Result<JobId, LocalizationError> {
        let project = self.version_project(project)?;
        if project.is_ad() {
            return Err(LocalizationError::AdProject);
        }
        if self.work_busy(&Work::Versions(project.id.to_string())) {
            return Err(LocalizationError::Busy);
        }
        let target = self
            .channels
            .get(channel)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(LocalizationError::ChannelNotFound)?;
        let own = self
            .channels
            .get(project.channel)?
            .ok_or(LocalizationError::ProjectNotFound)?;
        if target.id == own.id
            || Some(target.id) == self.came_from(project.id)?
            || !other_audience(&target, own.details.language(), own.details.country())
        {
            return Err(LocalizationError::SameAudience);
        }
        if self
            .localizations
            .versions(project.id)?
            .iter()
            .any(|version| version.channel == target.id)
        {
            return Err(LocalizationError::AlreadyMade);
        }
        let narration = self
            .narrations
            .narration(project.id)?
            .ok_or(LocalizationError::NoNarration)?;
        if self
            .scene_plans
            .scene_plan(project.id)?
            .is_none_or(|plan| plan.is_stale(Some(&narration)))
        {
            return Err(LocalizationError::NoPlan);
        }
        if self.provider_key(Provider::Claude).state == KeyState::NotSet {
            return Err(LocalizationError::MissingKey(Provider::Claude));
        }
        let template = self.current_template(TemplateKind::Localization)?;
        let sentences = source_sentences(&narration);
        let facts = SourceFacts {
            language: own.details.language(),
            country: own.details.country(),
            title: &project.title,
            sentences: &sentences,
            length: narration_length(&narration),
        };
        let planned = Self::planned_version(&project, &target);
        let rendered = self.render_localization(&planned, &facts, &template)?;
        let call = adaptation_call(&rendered);
        let payload = LocalizationPayload {
            project: project.id.to_string(),
            channel: Some(target.id.to_string()),
            version: Some(planned.id.to_string()),
            narration: Some(narration.id.to_string()),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
        };
        // The same version was asked for and its adaptation paid for, and
        // the version was not started (its job was cancelled as the answer
        // arrived, or could not save it): its job goes on from that
        // adaptation. Only the id it gave the version differs.
        let work = Work::Versions(project.id.to_string());
        if let Some(kept) = self.kept_answer(&work, |job| {
            LocalizationPayload::of(job).is_some_and(|asked| {
                LocalizationPayload {
                    version: payload.version.clone(),
                    ..asked
                } == payload
            })
        }) {
            // Not queued (another window got to it first): nothing new is
            // paid for beside it.
            return match self.jobs.retry(kept) {
                Ok(()) => Ok(kept),
                Err(_) => Err(LocalizationError::Busy),
            };
        }
        if let Err(estimate) = self.check_budget(&[call], consent)? {
            return Err(LocalizationError::OverBudget(estimate));
        }
        let job = Job::new(self.profile.id, JobKind::Localization, payload.to_json());
        Ok(self.jobs.enqueue(job)?)
    }

    /// The prompt adapting the version `project` again, from what it
    /// carried.
    pub(crate) fn render_adaptation(
        &self,
        project: &VideoProject,
        localization: &Localization,
        template: &TemplateVersion,
    ) -> Result<RenderedPrompt, ScriptError> {
        let facts = SourceFacts {
            language: localization.source_language,
            country: localization.source_country,
            title: &localization.source_title,
            sentences: &localization.sentences,
            length: localization.source_length,
        };
        self.render_localization(project, &facts, template)
    }

    /// Starts a job adapting the version `project` again; the new
    /// adaptation waits for review. Called by `generate_script`, which
    /// checked the key and that nothing runs.
    pub(crate) fn adapt_version_again(
        &self,
        project: &VideoProject,
        localization: &Localization,
        consent: BudgetConsent,
    ) -> Result<JobId, ScriptError> {
        let template = self.current_template(TemplateKind::Localization)?;
        let rendered = self.render_adaptation(project, localization, &template)?;
        if let Err(estimate) =
            self.check_video_budget(project.id, &[adaptation_call(&rendered)], consent)?
        {
            return Err(ScriptError::OverBudget(estimate));
        }
        let payload = LocalizationPayload {
            project: project.id.to_string(),
            channel: None,
            version: None,
            narration: None,
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
        };
        let job = Job::new(self.profile.id, JobKind::Localization, payload.to_json());
        Ok(self.jobs.enqueue(job)?)
    }

    /// What adapting the version again would cost, with its template.
    pub(crate) fn adaptation_estimate(
        &self,
        project: &VideoProject,
        localization: &Localization,
    ) -> Result<(TemplateVersion, SpendEstimate), ScriptError> {
        let template = self.current_template(TemplateKind::Localization)?;
        let rendered = self.render_adaptation(project, localization, &template)?;
        let estimate = self.estimate(&[adaptation_call(&rendered)])?;
        Ok((template, estimate))
    }

    /// The Scenes stage's part for a version: whether its carried scenes
    /// can be placed on its narration, and once they are, what to look at.
    pub(crate) fn version_scenes(
        &self,
        project: VideoProjectId,
        plan: Option<&ScenePlan>,
        narration: Option<&Narration>,
    ) -> Result<Option<VersionScenes>, RepositoryError> {
        let Some(localization) = self.localizations.localization(project)? else {
            return Ok(None);
        };
        let Some(plan) = plan.filter(|plan| localization.carries(plan)) else {
            return Ok(Some(VersionScenes {
                can_place: false,
                notes: Vec::new(),
            }));
        };
        let stale = plan.is_stale(narration);
        Ok(Some(VersionScenes {
            can_place: stale && narration.is_some(),
            notes: if stale {
                Vec::new()
            } else {
                placed_scene_notes(&localization, plan)
            },
        }))
    }

    /// Places the scenes the version carried on its narration and builds
    /// its cut from the carried one.
    pub fn place_scenes(&self, project: VideoProjectId) -> Result<ScenePlan, LocalizationError> {
        let project = self.version_project(project)?;
        let localization = self
            .localizations
            .localization(project.id)?
            .ok_or(LocalizationError::NotAVersion)?;
        let narration = self
            .narrations
            .narration(project.id)?
            .ok_or(LocalizationError::NoNarration)?;
        let plan = self
            .scene_plans
            .scene_plan(project.id)?
            .filter(|plan| localization.carries(plan) && plan.is_stale(Some(&narration)))
            .ok_or(LocalizationError::NothingToPlace)?;
        if self.scenes_busy(project.id)
            || self.clips_busy(project.id)
            || self.narration_busy(project.id)
        {
            return Err(LocalizationError::ScenesBusy);
        }
        let script = self
            .scripts
            .script(project.id)?
            .ok_or(LocalizationError::NothingToPlace)?;
        let adaptation = script.source().generation();
        let starts = self
            .localizations
            .adapted_starts(adaptation.id)?
            .filter(|starts| starts.len() == localization.sentences.len())
            .ok_or(LocalizationError::NothingToPlace)?;
        let words = sentence_words(&adaptation.output, &starts, &narration);
        let times = sentence_times(&words, &narration);
        let now = SystemTime::now();
        let placed = place_scenes(
            &plan,
            &localization.scene_sentences,
            &words,
            &narration,
            now,
        );
        let map = localization.time_map(&times, narration_length(&narration));
        let media = self.media_assets.media_assets(project.id)?;
        let cut = localized_cut(&placed, &narration, &localization.cut, &map, &media);
        let saved = cut.to_saved(project.id, self.profile.id, placed.id, narration.id, now);
        self.localizations.place_scenes(&placed, &saved)?;
        Ok(placed)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{ChannelDraft, CostRepository, JobState, Persona};

    use super::*;
    use crate::scenes::tests::{Harness, done, wait_done};

    const ADAPTED: [&str; 4] = [
        "Once upon a time, in 1969, there was a probe.",
        "It flew far away.",
        "The signal vanished in March.",
        "Nobody knows why.",
    ];

    fn answer(h: &Harness, sentences: &[&str]) {
        h.answer(
            serde_json::json!({
                "title": "The Probe That Never Came Home",
                "sentences": sentences,
            })
            .to_string(),
        );
    }

    fn narrator(app: &Bardo) -> Persona {
        app.personas()
            .unwrap()
            .into_iter()
            .find(|p| p.details.name() == "Documentary Narrator (en-US)")
            .unwrap()
    }

    /// An English channel for the United States, narrated as the source.
    fn english_channel(app: &Bardo) -> Channel {
        app.create_channel(ChannelDraft {
            name: "Lost Signals".into(),
            niche: "space history".into(),
            language: ContentLanguage::English,
            country: Country::UnitedStates,
            default_persona: Some(narrator(app).id),
            ..ChannelDraft::default()
        })
        .unwrap()
    }

    /// A drawn source project, an English channel and the version made
    /// for it.
    fn made(h: &Harness, app: &Bardo) -> (VideoProject, Channel, VideoProject) {
        let (source, _) = h.drawn_project(app);
        let channel = english_channel(app);
        answer(h, &ADAPTED);
        done(
            app,
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let version = app.versions(source.id).unwrap().made[0].project.clone();
        (source, channel, version)
    }

    #[test]
    fn a_version_whose_answer_is_lost_is_asked_for_once_and_is_in_costs_of_its_channel() {
        use crate::costs::tests::lost_answer;

        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        let before = h.text.requests().len();
        *h.text.failure.lock().unwrap() = Some(lost_answer());
        let id = app
            .make_version(source.id, channel.id, BudgetConsent::Ask)
            .unwrap();
        let job = wait_done(&app, id);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(job.attempts(), 1, "not made again by the queue");
        assert_eq!(job.failure().unwrap().kind, JobFailureKind::AnswerLost);
        assert_eq!(h.text.requests().len() - before, 1);
        assert_eq!(h.text.unstoppable(), 0, "the job hands over its stop");
        // The source's own script is a Script cost too: this one is the job's.
        let costs: Vec<_> =
            h.db.recent_costs(app.profile().id, CostPurpose::Script, 10)
                .unwrap()
                .into_iter()
                .filter(|cost| cost.job == Some(id))
                .collect();
        assert_eq!(
            costs.len(),
            1,
            "the adaptation may have been written and billed"
        );
        assert_eq!(costs[0].channel, Some(channel.id));
        assert_eq!(costs[0].project, None, "no version exists yet");
    }

    #[test]
    fn a_version_starts_in_the_channel_with_the_adapted_script_and_copies() {
        let h = Harness::new();
        let app = h.start();
        let (source, plan) = h.drawn_project(&app);
        let channel = english_channel(&app);
        let before = app.versions(source.id).unwrap();
        assert_eq!(before.blocked, None);
        assert_eq!(before.source, None);
        let target = &before.targets[0];
        assert_eq!(before.targets.len(), 1);
        assert_eq!(target.channel, channel.id);
        assert_eq!(target.language, ContentLanguage::English);
        assert_eq!(
            target.narrator.as_deref(),
            Some("Documentary Narrator (en-US)")
        );

        answer(&h, &ADAPTED);
        let job = done(
            &app,
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(job.kind(), JobKind::Localization);
        let request = h.text.requests().pop().unwrap();
        assert!(matches!(request.format, TextFormat::Json { .. }));
        // The adaptation is as long as the script: room by its length.
        assert_eq!(
            request.answer_tokens,
            Some((request.prompt.chars().count() as u64).div_ceil(3))
        );
        assert!(
            request.instructions.contains("English"),
            "{}",
            request.instructions
        );
        assert!(request.instructions.contains("Portuguese"));
        assert!(request.instructions.contains("United States"));
        assert!(
            request.prompt.contains("Channel: Lost Signals"),
            "{}",
            request.prompt
        );
        assert!(request.prompt.contains("[0] 0:00-"), "{}", request.prompt);
        assert!(request.prompt.contains("[3] "));
        assert!(request.prompt.contains("Ninguém sabe por quê."));
        assert!(request.prompt.contains("The probe that never came home"));

        let after = app.versions(source.id).unwrap();
        assert_eq!(after.made.len(), 1);
        assert_eq!(after.made[0].channel, "Lost Signals");
        assert!(after.targets.is_empty());
        assert_eq!(after.blocked, Some(VersionBlock::NoChannel));
        let version = &after.made[0].project;
        assert_eq!(version.channel, channel.id);
        assert_eq!(version.title, "The Probe That Never Came Home");
        // The source's channel makes long videos; the English one short.
        assert_eq!(version.format, Some(bardo_domain::VideoFormat::Long));
        assert_eq!(
            app.video_projects(channel.id).unwrap(),
            std::slice::from_ref(version)
        );

        let script = app.script(version.id).unwrap().script.unwrap();
        assert_eq!(script.text().as_str(), ADAPTED.join(" "));
        assert_eq!(script.source().generation().job, Some(job.id()));
        assert_eq!(
            app.script(version.id).unwrap().template.body.kind(),
            TemplateKind::Localization
        );

        let copied = app.scenes(version.id).unwrap().plan.unwrap();
        assert_ne!(copied.id, plan.id);
        assert_eq!(copied.project, version.id);
        assert_eq!(copied.scenes().len(), plan.scenes().len());
        for (copy, original) in copied.scenes().iter().zip(plan.scenes()) {
            let (copy, original) = (copy.image().unwrap(), original.image().unwrap());
            assert_eq!(copy.file, original.file);
            assert_ne!(copy.generation.id, original.generation.id);
            assert_eq!(copy.generation.project, version.id);
            assert_eq!(copy.generation.prompt, original.generation.prompt);
            assert!(h.files.exists(version.id, &copy.file));
            assert!(h.files.exists(source.id, &original.file));
        }
        // The source is as it was.
        assert_eq!(app.scenes(source.id).unwrap().plan.unwrap(), plan);

        let of = app.versions(version.id).unwrap().source.unwrap();
        assert_eq!(of.project, Some(source.id));
        assert_eq!(of.title, "The probe that never came home");
        assert_eq!(of.language, ContentLanguage::Portuguese);

        // Only the adaptation is billed on the version, in its channel.
        let costs = app.cost_book.costs.project_costs(version.id).unwrap();
        assert_eq!(costs.len(), 1);
        assert_eq!(costs[0].purpose, CostPurpose::Script);
        assert_eq!(costs[0].channel, Some(channel.id));
        assert_eq!(costs[0].job, Some(job.id()));
    }

    #[test]
    fn an_adaptation_with_another_count_fails_and_leaves_nothing() {
        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        answer(&h, &ADAPTED[..3]);
        let job = wait_done(
            &app,
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().map(|f| f.kind),
            Some(JobFailureKind::UnexpectedAnswer)
        );
        let view = app.versions(source.id).unwrap();
        assert!(view.made.is_empty());
        assert_eq!(view.job.as_ref().map(Job::id), Some(job.id()));
        assert_eq!(view.job_channel.as_deref(), Some("Lost Signals"));
        assert_eq!(view.targets.len(), 1);
        assert!(app.video_projects(channel.id).unwrap().is_empty());
        // The answer was paid for: on the channel, with no project.
        let costs = app
            .cost_book
            .costs
            .recent_costs(app.profile.id, CostPurpose::Script, 10)
            .unwrap();
        let billed: Vec<_> = costs
            .iter()
            .filter(|cost| cost.job == Some(job.id()))
            .collect();
        assert_eq!(billed.len(), 1);
        assert_eq!(billed[0].channel, Some(channel.id));
        assert_eq!(billed[0].project, None);
    }

    #[test]
    fn only_another_audience_without_a_version_is_offered() {
        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let same = app
            .create_channel(ChannelDraft {
                name: "Arquivos 2".into(),
                language: ContentLanguage::Portuguese,
                country: Country::Brazil,
                ..ChannelDraft::default()
            })
            .unwrap();
        let view = app.versions(source.id).unwrap();
        assert!(view.targets.is_empty());
        assert_eq!(view.blocked, Some(VersionBlock::NoChannel));
        assert!(matches!(
            app.make_version(source.id, same.id, BudgetConsent::Ask),
            Err(LocalizationError::SameAudience)
        ));
        assert!(matches!(
            app.make_version(source.id, source.channel, BudgetConsent::Ask),
            Err(LocalizationError::SameAudience)
        ));

        let channel = english_channel(&app);
        answer(&h, &ADAPTED);
        done(
            &app,
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert!(matches!(
            app.make_version(source.id, channel.id, BudgetConsent::Ask),
            Err(LocalizationError::AlreadyMade)
        ));
        // Nor is the version made again for the channel it came from.
        let version = app.versions(source.id).unwrap().made[0].project.id;
        assert!(matches!(
            app.make_version(version, source.channel, BudgetConsent::Ask),
            Err(LocalizationError::SameAudience)
        ));
    }

    #[test]
    fn a_version_needs_the_narration_and_scenes_planned_on_it() {
        let h = Harness::new();
        let app = h.start();
        let source = h.narrated_project(&app);
        let channel = english_channel(&app);
        assert_eq!(
            app.versions(source.id).unwrap().blocked,
            Some(VersionBlock::NoPlan)
        );
        assert!(matches!(
            app.make_version(source.id, channel.id, BudgetConsent::Ask),
            Err(LocalizationError::NoPlan)
        ));
        let bare = crate::scenes::tests::project(&app);
        assert_eq!(
            app.versions(bare.id).unwrap().blocked,
            Some(VersionBlock::NoNarration)
        );
        assert!(matches!(
            app.make_version(bare.id, channel.id, BudgetConsent::Ask),
            Err(LocalizationError::NoNarration)
        ));
    }

    fn handler(h: &Harness, app: &Bardo) -> LocalizationHandler {
        LocalizationHandler {
            owner: app.profile().id,
            themes: Arc::clone(&h.db) as _,
            channels: Arc::clone(&h.db) as _,
            scripts: Arc::clone(&h.db) as _,
            narrations: Arc::clone(&h.db) as _,
            plans: Arc::clone(&h.db) as _,
            timelines: Arc::clone(&h.db) as _,
            media: Arc::clone(&h.db) as _,
            tracks: Arc::clone(&h.db) as _,
            localizations: Arc::clone(&h.db) as _,
            files: Arc::clone(&h.files) as _,
            text: Arc::clone(&h.text) as _,
            secrets: Arc::clone(&h.secrets) as _,
            // Run here, outside the queue: no job runs to note a call on.
            costs: crate::costs::CostBook {
                in_flight: Arc::default(),
                ..app.cost_book.clone()
            },
        }
    }

    #[test]
    fn a_resumed_job_does_not_make_the_version_twice() {
        let h = Harness::new();
        let app = h.start();
        let (source, channel, version) = made(&h, &app);
        let job = app.versions(source.id).unwrap().job.unwrap();
        let asked = h.text.requests().len();

        let payload = LocalizationPayload::of(&job).unwrap();
        handler(&h, &app)
            .make(
                &payload,
                channel.id,
                job.id(),
                &Stop::never(),
                &|| false,
                &mut None,
            )
            .unwrap();
        assert_eq!(h.text.requests().len(), asked, "Claude is asked once");
        assert_eq!(app.video_projects(channel.id).unwrap(), [version]);
    }

    #[test]
    fn a_stopped_job_leaves_no_project_or_file() {
        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        // Asked for, but held so the handler can run it here.
        app.jobs.pause(true);
        let id = app
            .make_version(source.id, channel.id, BudgetConsent::Ask)
            .unwrap();
        let job = app.jobs().into_iter().find(|job| job.id() == id).unwrap();
        let payload = LocalizationPayload::of(&job).unwrap();
        let version = parse_id::<VideoProjectId>(payload.version.as_deref().unwrap()).unwrap();
        answer(&h, &ADAPTED);
        let calls = std::cell::Cell::new(0);
        let mut kept: Option<(bardo_domain::GeneratedText, Option<CostRecordId>)> = None;
        handler(&h, &app)
            .make(
                &payload,
                channel.id,
                id,
                &Stop::never(),
                &|| {
                    calls.set(calls.get() + 1);
                    calls.get() > 1
                },
                &mut kept,
            )
            .unwrap();
        assert_eq!(calls.get(), 2, "stopped after copying");
        assert!(kept.is_some(), "the paid adaptation is for the job to keep");
        assert!(app.video_projects(channel.id).unwrap().is_empty());
        let plan = app.scenes(source.id).unwrap().plan.unwrap();
        for scene in plan.scenes() {
            assert!(!h.files.exists(version, &scene.image().unwrap().file));
        }
    }

    #[test]
    fn generating_a_version_again_adapts_the_original_for_review() {
        let h = Harness::new();
        let app = h.start();
        let (_, _, version) = made(&h, &app);
        let first = app.script(version.id).unwrap().script.unwrap();

        answer(
            &h,
            &[
                "In 1969, a probe set off.",
                "It went far.",
                "In March, the signal died.",
                "No one knows why.",
            ],
        );
        let job = done(
            &app,
            app.generate_script(version.id, BudgetConsent::Ask).unwrap(),
        );
        assert_eq!(job.kind(), JobKind::Localization);
        let request = h.text.requests().pop().unwrap();
        assert!(request.prompt.contains("Ninguém sabe por quê."));
        assert!(request.prompt.contains("Channel: Lost Signals"));

        let view = app.script(version.id).unwrap();
        assert_eq!(view.job.as_ref().map(Job::id), Some(job.id()));
        let script = view.script.unwrap();
        assert_eq!(script.text(), first.text());
        let pending = script.pending().unwrap();
        assert_eq!(
            pending.text().as_str(),
            "In 1969, a probe set off. It went far. In March, the signal died. No one knows why."
        );
        assert_eq!(
            app.localizations
                .adapted_starts(pending.generation().id)
                .unwrap(),
            Some(vec![0, 26, 39, 66])
        );

        // A wrong count leaves the review as it was.
        answer(&h, &ADAPTED[..2]);
        let failed = wait_done(
            &app,
            app.generate_script(version.id, BudgetConsent::Ask).unwrap(),
        );
        assert_eq!(failed.state(), JobState::Failed);
        assert_eq!(app.script(version.id).unwrap().script.unwrap(), script);
    }

    #[test]
    fn a_version_carries_the_places_and_keeps_them_when_its_scenes_are_placed() {
        let h = Harness::new();
        let app = h.start();
        let source = crate::scenes::tests::storyboard_project(&h, &app);
        done(
            &app,
            app.generate_scene_images(source.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let planned = app.scenes(source.id).unwrap().plan.unwrap();
        assert_eq!(planned.places().len(), 2);
        let channel = english_channel(&app);
        answer(&h, &ADAPTED);
        done(
            &app,
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let version = app.versions(source.id).unwrap().made[0].project.clone();

        let carried = app.scenes(version.id).unwrap().plan.unwrap();
        assert_eq!(carried.places(), planned.places());
        let of = |plan: &ScenePlan| -> Vec<Option<usize>> {
            (0..plan.scenes().len())
                .map(|index| plan.scene_place(index))
                .collect()
        };
        assert_eq!(of(&carried), [Some(0), Some(0), Some(0), Some(1)]);

        done(
            &app,
            app.generate_narration(version.id, crate::PicturesConsent::Ask, BudgetConsent::Ask)
                .unwrap(),
        );
        let placed = app.place_scenes(version.id).unwrap();
        assert_eq!(placed.places(), planned.places());
        assert_eq!(of(&placed), of(&carried));
        // A scene of the version drawn again still follows its place.
        assert_eq!(
            placed.reference_shots(2, bardo_domain::StyleChoice::FromNotes),
            [
                (0, bardo_domain::OtherShot::SamePlace),
                (1, bardo_domain::OtherShot::Before)
            ]
        );
    }

    #[test]
    fn a_version_carries_which_portraits_its_images_were_drawn_with() {
        let h = Harness::new();
        let app = h.start();
        let source = crate::scenes::tests::cast_project(&h, &app);
        done(
            &app,
            app.generate_scene_images(source.id, BudgetConsent::Ask)
                .unwrap(),
        );
        // The chef's portrait is drawn again: his scenes show the old one.
        done(
            &app,
            app.redraw_cast_portrait(source.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        done(
            &app,
            app.regenerate_scene_image(source.id, 0, BudgetConsent::Ask)
                .unwrap(),
        );
        app.accept_scene_image(source.id, 0).unwrap();
        assert_eq!(app.scenes(source.id).unwrap().old_portraits, [2]);

        let channel = english_channel(&app);
        answer(&h, &ADAPTED);
        done(
            &app,
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let version = app.versions(source.id).unwrap().made[0].project.clone();

        // The copy's images go by the copy's portrait: the one drawn with
        // the current portrait matches it, the other is still out of date.
        let view = app.scenes(version.id).unwrap();
        assert_eq!(view.old_portraits, [2]);
        let carried = view.plan.unwrap();
        let portrait = carried.cast()[0].portrait().unwrap().generation.id;
        assert_eq!(
            carried.scenes()[0].image_portraits(),
            Some(&vec![(0, Some(portrait))])
        );
    }

    #[test]
    fn a_version_keeps_the_style_its_images_were_drawn_in() {
        let h = Harness::new();
        let app = h.start();
        let source = h.narrated_project(&app);
        let clay = bardo_domain::StyleChoice::Catalog(bardo_domain::VisualStyle::ClayStopMotion);
        app.set_project_style(source.id, Some(clay)).unwrap();
        h.answer(crate::scenes::tests::plan_answer());
        done(
            &app,
            app.plan_scenes(source.id, false, BudgetConsent::Ask)
                .unwrap(),
        );
        done(
            &app,
            app.generate_scene_images(source.id, BudgetConsent::Ask)
                .unwrap(),
        );
        // The other channel has no style of its own.
        let channel = english_channel(&app);
        assert_eq!(
            channel.details.style(),
            bardo_domain::StyleChoice::FromNotes
        );
        answer(&h, &ADAPTED);
        done(
            &app,
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let version = app.versions(source.id).unwrap().made[0].project.clone();
        assert_eq!(version.style, Some(clay));
        let view = app.scenes(version.id).unwrap();
        assert_eq!(view.style, clay);
        assert!(view.restyled.is_empty(), "its images are in its style");
    }

    #[test]
    fn the_carried_scenes_are_placed_on_the_version_s_narration() {
        let h = Harness::new();
        let app = h.start();
        let (source, _, version) = made(&h, &app);
        let carried = app.scenes(version.id).unwrap();
        assert!(carried.version.as_ref().is_some_and(|v| !v.can_place));
        assert!(matches!(
            app.place_scenes(version.id),
            Err(LocalizationError::NoNarration)
        ));
        assert!(matches!(
            app.place_scenes(source.id),
            Err(LocalizationError::NotAVersion)
        ));

        done(
            &app,
            app.generate_narration(version.id, crate::PicturesConsent::Ask, BudgetConsent::Ask)
                .unwrap(),
        );
        let narration = app.narrations.narration(version.id).unwrap().unwrap();
        let view = app.scenes(version.id).unwrap();
        assert!(view.stale);
        assert!(view.version.as_ref().is_some_and(|v| v.can_place));
        // Placing is the carried scenes' way onto the narration: they are
        // not fitted, and nothing is drawn for them until they are placed.
        assert!(!view.can_fit);
        assert!(matches!(
            app.fit_scenes(version.id),
            Err(crate::SceneError::PlanOutOfDate)
        ));
        assert!(matches!(
            app.regenerate_scene_image(version.id, 0, BudgetConsent::Ask),
            Err(crate::SceneError::PlanOutOfDate)
        ));
        let stages = crate::stages::project_stages(
            &app.script(version.id).unwrap(),
            &app.narration(version.id).unwrap(),
            &view,
            &app.render_summary(version.id).unwrap(),
            &app.export_summary(version.id).unwrap(),
        );
        assert_eq!(stages[2].note, crate::StageNote::ScenesToPlace);

        let placed = app.place_scenes(version.id).unwrap();
        assert_eq!(placed.id, carried.plan.as_ref().unwrap().id);
        assert_eq!(placed.narration, narration.id);
        let scenes = placed.scenes();
        assert_eq!(scenes.len(), 3);
        assert_eq!(scenes[0].start, Duration::ZERO);
        assert_eq!(scenes[0].text, ADAPTED[0]);
        assert_eq!(scenes[1].text, ADAPTED[1]);
        assert_eq!(scenes[2].text, format!("{} {}", ADAPTED[2], ADAPTED[3]));
        let words = narration.words.as_slice();
        let it = words
            .iter()
            .find(|word| &narration.text.as_str()[word.text.clone()] == "It")
            .unwrap();
        assert_eq!(scenes[1].start, it.start);
        assert_eq!(scenes[0].end, scenes[1].start);
        for (scene, copied) in scenes.iter().zip(carried.plan.unwrap().scenes()) {
            assert_eq!(scene.image(), copied.image());
        }

        let after = app.scenes(version.id).unwrap();
        assert!(!after.stale);
        assert!(after.version.as_ref().is_some_and(|v| !v.can_place));
        let saved = app.timelines.saved_timeline(version.id).unwrap().unwrap();
        assert_eq!(saved.scene_plan, placed.id);
        assert_eq!(saved.narration, narration.id);
        assert!(matches!(
            app.place_scenes(version.id),
            Err(LocalizationError::NothingToPlace)
        ));
    }

    /// The costs recorded for `job`'s adaptation.
    fn adaptation_costs(h: &Harness, app: &Bardo, job: JobId) -> Vec<bardo_domain::CostRecord> {
        h.db.recent_costs(app.profile().id, CostPurpose::Script, 10)
            .unwrap()
            .into_iter()
            .filter(|cost| cost.job == Some(job))
            .collect()
    }

    #[test]
    fn a_cancelled_version_job_that_still_stops_keeps_versions_busy_and_its_answer_is_kept() {
        use crate::testing::arriving;

        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        let asked = h.text.requests().len();
        answer(&h, &ADAPTED);
        let id = arriving::cancelled(&app, &h.text, || {
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap()
        });

        // No second request beside the one still out, from the stage or
        // from Jobs.
        assert!(matches!(
            app.make_version(source.id, channel.id, BudgetConsent::Ask),
            Err(LocalizationError::Busy)
        ));
        arriving::retry_refused(&app, id);

        // The answer arrives whole: the user said stop, so no project is
        // started in the other channel behind their back. The adaptation
        // is paid for, in Costs, and kept on the job.
        arriving::lands(&app, &h.text, id);
        assert!(app.video_projects(channel.id).unwrap().is_empty());
        assert!(app.versions(source.id).unwrap().made.is_empty());
        let costs = adaptation_costs(&h, &app, id);
        assert_eq!(costs.len(), 1);
        assert_eq!(
            (costs[0].channel, costs[0].project),
            (Some(channel.id), None)
        );

        // Run again, the job makes the version from the adaptation it
        // kept: Claude is not asked, or paid, twice.
        assert_eq!(arriving::retried(&app, id).state(), JobState::Done);
        assert_eq!(h.text.requests().len(), asked + 1);
        let version = app.versions(source.id).unwrap().made[0].project.clone();
        // Counted once, and on the version now that it exists.
        let costs = adaptation_costs(&h, &app, id);
        assert_eq!(costs.len(), 1);
        assert_eq!(
            (costs[0].channel, costs[0].project),
            (Some(channel.id), Some(version.id))
        );
        assert_eq!(
            app.script(version.id)
                .unwrap()
                .script
                .unwrap()
                .text()
                .as_str(),
            ADAPTED.join(" ")
        );
    }

    #[test]
    fn making_the_version_again_from_the_stage_uses_the_adaptation_its_cancelled_job_kept() {
        use crate::testing::arriving;

        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        let other = app
            .create_channel(ChannelDraft {
                name: "Signals UK".into(),
                niche: "space history".into(),
                language: ContentLanguage::English,
                country: Country::UnitedKingdom,
                default_persona: Some(narrator(&app).id),
                ..ChannelDraft::default()
            })
            .unwrap();
        let asked = h.text.requests().len();
        answer(&h, &ADAPTED);
        let id = arriving::cancelled(&app, &h.text, || {
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap()
        });
        arriving::lands(&app, &h.text, id);

        // Asked for the same channel again, the cancelled job goes on from
        // the adaptation it kept: no new job, no request, one Costs line.
        // Nothing is paid, so nothing is asked about the budget either.
        app.set_budget(Provider::Claude, "0.0001").unwrap();
        let again = app
            .make_version(source.id, channel.id, BudgetConsent::Ask)
            .unwrap();
        assert_eq!(again, id);
        assert_eq!(arriving::ended(&app, id).state(), JobState::Done);
        assert_eq!(h.text.requests().len(), asked + 1);
        assert_eq!(adaptation_costs(&h, &app, id).len(), 1);
        assert_eq!(app.versions(source.id).unwrap().made.len(), 1);

        // A version for another channel is another request: it pays, so
        // it asks first.
        assert!(matches!(
            app.make_version(source.id, other.id, BudgetConsent::Ask),
            Err(LocalizationError::OverBudget(_))
        ));
        answer(&h, &ADAPTED);
        let new = app
            .make_version(source.id, other.id, BudgetConsent::Confirmed)
            .unwrap();
        assert_ne!(new, id);
        assert_eq!(arriving::ended(&app, new).state(), JobState::Done);
        assert_eq!(h.text.requests().len(), asked + 2);
    }

    /// A slot that notes, when an answer is put in it, what `seen` reads.
    struct Watched<F: Fn() -> bool> {
        answer: Option<(bardo_domain::GeneratedText, Option<CostRecordId>)>,
        seen_at_keep: Option<bool>,
        seen: F,
    }

    impl<F: Fn() -> bool> Keeps for Watched<F> {
        fn kept(&self) -> Option<(bardo_domain::GeneratedText, Option<CostRecordId>)> {
            self.answer.clone()
        }

        fn keep(&mut self, generated: &bardo_domain::GeneratedText, cost: CostRecordId) {
            self.seen_at_keep = Some((self.seen)());
            self.answer = Some((generated.clone(), Some(cost)));
        }

        fn settle(&mut self) {
            self.answer = None;
        }
    }

    #[test]
    fn the_adaptation_is_kept_and_in_costs_before_the_version_s_files_are_copied() {
        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        // Asked for, but held so the handler can run it here.
        app.jobs.pause(true);
        let id = app
            .make_version(source.id, channel.id, BudgetConsent::Ask)
            .unwrap();
        let job = app.jobs().into_iter().find(|job| job.id() == id).unwrap();
        let payload = LocalizationPayload::of(&job).unwrap();
        let version = parse_id::<VideoProjectId>(payload.version.as_deref().unwrap()).unwrap();
        answer(&h, &ADAPTED);
        let plan = app.scenes(source.id).unwrap().plan.unwrap();
        let image = plan.scenes()[0].image().unwrap().file.clone();
        // A run cut short while it copies (the app closes, the process
        // dies) must find the answer kept: it is kept, and counted,
        // before the first file is copied.
        let mut slot = Watched {
            answer: None,
            seen_at_keep: None,
            seen: || h.files.exists(version, &image) || adaptation_costs(&h, &app, id).is_empty(),
        };
        let calls = std::cell::Cell::new(0);
        handler(&h, &app)
            .make(
                &payload,
                channel.id,
                id,
                &Stop::never(),
                &|| {
                    calls.set(calls.get() + 1);
                    calls.get() > 1
                },
                &mut slot,
            )
            .unwrap();
        assert_eq!(
            slot.seen_at_keep,
            Some(false),
            "kept after it was counted and before anything was copied"
        );
        assert!(slot.answer.is_some(), "and still kept: no version was made");
    }

    #[test]
    fn a_kept_adaptation_that_turns_out_not_usable_is_let_go() {
        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        app.jobs.pause(true);
        let id = app
            .make_version(source.id, channel.id, BudgetConsent::Ask)
            .unwrap();
        let job = app.jobs().into_iter().find(|job| job.id() == id).unwrap();
        let payload = LocalizationPayload::of(&job).unwrap();
        let asked = h.text.requests().len();
        let mut slot = Some((
            bardo_domain::GeneratedText {
                text: "not an adaptation".into(),
                model: "claude-fake".into(),
                usage: bardo_domain::TokenUsage::default(),
            },
            None,
        ));
        let failure = handler(&h, &app)
            .make(
                &payload,
                channel.id,
                id,
                &Stop::never(),
                &|| false,
                &mut slot,
            )
            .unwrap_err();
        assert_eq!(failure.kind, JobFailureKind::UnexpectedAnswer);
        assert_eq!(slot, None, "the next run asks Claude again");
        assert_eq!(
            h.text.requests().len(),
            asked,
            "this one used what was kept"
        );
        assert!(
            adaptation_costs(&h, &app, id).is_empty(),
            "nothing new in Costs: it was counted when it arrived"
        );
    }

    #[test]
    fn a_run_cut_short_after_the_version_was_saved_moves_its_cost_on_the_next_run() {
        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        app.jobs.pause(true);
        let id = app
            .make_version(source.id, channel.id, BudgetConsent::Ask)
            .unwrap();
        let job = app.jobs().into_iter().find(|job| job.id() == id).unwrap();
        let payload = LocalizationPayload::of(&job).unwrap();
        let version = parse_id::<VideoProjectId>(payload.version.as_deref().unwrap()).unwrap();
        answer(&h, &ADAPTED);
        // The first run makes the version, and is cut short before its
        // answer's record moved: the answer is still kept, with a record
        // on the channel alone.
        let mut first = None;
        handler(&h, &app)
            .make(
                &payload,
                channel.id,
                id,
                &Stop::never(),
                &|| false,
                &mut first,
            )
            .unwrap();
        assert_eq!(first, None, "settled once the version was made");
        let asked = h.text.requests().len();
        let made = adaptation_costs(&h, &app, id);
        assert_eq!(made.len(), 1);
        let generated = bardo_domain::GeneratedText {
            text: String::new(),
            model: made[0].model.clone(),
            usage: bardo_domain::TokenUsage::default(),
        };
        let left = CostRecordId::new();
        handler(&h, &app).bill(left, &generated, id, Some(channel.id), None);
        let mut kept = Some((generated, Some(left)));

        // The next run finds the version made: it asks nothing, and counts
        // the record on the version.
        handler(&h, &app)
            .make(
                &payload,
                channel.id,
                id,
                &Stop::never(),
                &|| false,
                &mut kept,
            )
            .unwrap();
        assert_eq!(kept, None);
        assert_eq!(h.text.requests().len(), asked);
        let costs = adaptation_costs(&h, &app, id);
        assert_eq!(costs.len(), 2);
        assert!(
            costs
                .iter()
                .all(|cost| (cost.channel, cost.project) == (Some(channel.id), Some(version))),
            "{costs:?}"
        );
    }

    #[test]
    fn an_adaptation_that_is_not_usable_is_not_kept() {
        use crate::testing::arriving;

        let h = Harness::new();
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        let asked = h.text.requests().len();
        // One sentence short: not an adaptation of this narration.
        answer(&h, &ADAPTED[..3]);
        let id = arriving::cancelled(&app, &h.text, || {
            app.make_version(source.id, channel.id, BudgetConsent::Ask)
                .unwrap()
        });
        arriving::lands(&app, &h.text, id);
        assert_eq!(adaptation_costs(&h, &app, id).len(), 1);

        // The cancelled run saw it was not usable and kept nothing of it:
        // run again, the job asks Claude again. Both answers were paid, so
        // both are in Costs: the one that made the version on the version,
        // the other on the channel alone.
        answer(&h, &ADAPTED);
        assert_eq!(arriving::retried(&app, id).state(), JobState::Done);
        assert_eq!(h.text.requests().len(), asked + 2);
        let made = app.versions(source.id).unwrap().made;
        assert_eq!(made.len(), 1);
        let mut on: Vec<_> = adaptation_costs(&h, &app, id)
            .into_iter()
            .map(|cost| (cost.channel, cost.project))
            .collect();
        on.sort_by_key(|(_, project)| project.is_some());
        assert_eq!(
            on,
            [
                (Some(channel.id), None),
                (Some(channel.id), Some(made[0].project.id))
            ]
        );
    }

    #[test]
    fn a_cancelled_adaptation_that_still_stops_keeps_the_script_busy_and_its_answer_waits_for_review()
     {
        use crate::testing::arriving;

        let h = Harness::new();
        let app = h.start();
        let (_, _, version) = made(&h, &app);
        let first = app.script(version.id).unwrap().script.unwrap();
        let asked = h.text.requests().len();
        answer(
            &h,
            &[
                "In 1969, a probe set off.",
                "It went far.",
                "In March, the signal died.",
                "No one knows why.",
            ],
        );
        let id = arriving::cancelled(&app, &h.text, || {
            app.generate_script(version.id, BudgetConsent::Ask).unwrap()
        });

        // No second request beside the one still out, from the stage or
        // from Jobs.
        assert!(matches!(
            app.generate_script(version.id, BudgetConsent::Ask),
            Err(ScriptError::Busy)
        ));
        arriving::retry_refused(&app, id);

        // The answer arrives whole: it is paid for, replaces nothing, and
        // waits for review beside the script.
        arriving::lands(&app, &h.text, id);
        let script = app.script(version.id).unwrap().script.unwrap();
        assert_eq!(script.text(), first.text());
        assert!(script.pending().is_some());

        // Run again, the job finds its adaptation: Claude is not asked
        // twice.
        assert_eq!(arriving::retried(&app, id).state(), JobState::Done);
        assert_eq!(h.text.requests().len(), asked + 1);
    }

    #[test]
    fn a_version_answered_while_the_app_closes_is_made_from_that_answer_on_the_next_start() {
        use crate::testing::arriving;

        let mut h = Harness::new();
        // The app waits for the answer, which comes in time.
        h.closing_wait = Some(Duration::from_secs(60));
        let app = h.start();
        let (source, _) = h.drawn_project(&app);
        let channel = english_channel(&app);
        let asked = h.text.requests().len();
        answer(&h, &ADAPTED);
        *h.text.deaf.lock().unwrap() = true;
        h.text.hold();
        let id = app
            .make_version(source.id, channel.id, BudgetConsent::Ask)
            .unwrap();
        arriving::wait_until("the request left", || h.text.requests().len() > asked);
        let probe = app.jobs.probe();

        // The answer arrives once the app has begun to close: the run is
        // told to stop, and starts no version.
        let text = Arc::clone(&h.text);
        let closing = app.jobs.probe();
        let answer = std::thread::spawn(move || {
            arriving::wait_until("the app is closing", || closing.closing());
            assert!(closing.runs(id), "the answer is still on its way");
            text.let_go();
        });
        drop(app);
        answer.join().unwrap();
        assert!(!probe.runs(id), "the app waited for the handler");
        *h.text.deaf.lock().unwrap() = false;

        // The next start takes the job up by itself and makes the version
        // from the adaptation it kept: Claude is not asked, or paid, twice.
        let app = h.start();
        assert_eq!(arriving::ended(&app, id).state(), JobState::Done);
        assert_eq!(h.text.requests().len(), asked + 1);
        assert_eq!(adaptation_costs(&h, &app, id).len(), 1);
        assert_eq!(app.versions(source.id).unwrap().made.len(), 1);
    }
}
