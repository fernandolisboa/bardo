//! Export (PRD stories 76-78): the project's Publishing stage. After the
//! render, Claude writes each network's title, description and tags from
//! the script and the account's defaults (the metadata template); the user
//! edits them, and exports a ready-to-post package: one folder per network
//! with its rendered file and a metadata file to copy from, for posting by
//! hand.
//!
//! Each network's limits are domain rules (`bardo_domain::problems`): an
//! export holds only metadata that fits them. When the narrator's voice is
//! a clone or a realistic synthetic voice of a real person, the screen and
//! every metadata file remind the user to apply the network's synthetic-
//! content disclosure when posting.
//!
//! Generating calls Claude and exporting copies large files, so both run
//! as jobs. An export job keeps what the user saw when it started (each
//! network's render and post), writes one network after another and
//! checkpoints each; a cancelled or failed export resumes with the networks
//! left.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, Cost, CostPurpose, Export, ExportFiles, ExportRepository, Generation, GenerationId,
    Job, JobFailure, JobFailureKind, JobId, JobKind, METADATA_FILE, MetadataProblem,
    NarrationSource, Network, NetworkAccount, NetworkAccountId, Post, ProfileId, Progress,
    ProjectFiles, Provider, Render, RenderedPrompt, RepositoryError, SecretStore, TagPlacement,
    TemplateKind, TemplateUsed, TemplateValues, TemplateVariable, TemplateVersion,
    TemplateVersionId, TextFormat, TextGenerator, TextRequest, UiLanguage, VideoMetadata,
    VideoMetadataDraft, VideoProject, VideoProjectId, Visibility, package_folder, problems,
    video_file_name,
};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::render::RenderError;
use crate::scenes::{id, parse, to_json};
use crate::{Bardo, Catalog, KeyState, ScriptError, TemplateError, Text};

/// What Claude answers: one post per network asked for.
const METADATA_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "posts": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "network": {
            "type": "string",
            "enum": ["youtube", "tiktok", "instagram_reels", "x", "kick"]
          },
          "title": { "type": "string" },
          "description": { "type": "string" },
          "tags": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["network", "title", "description", "tags"],
        "additionalProperties": false
      }
    }
  },
  "required": ["posts"],
  "additionalProperties": false
}"#;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("video project not found")]
    ProjectNotFound,
    /// The channel has no network accounts to write or export for.
    #[error("the channel has no network accounts")]
    NoAccounts,
    /// Generation calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// The project's metadata is being generated.
    #[error("metadata is already being generated for this project")]
    Busy,
    /// The generation would reach a provider's budget; the screen asks
    /// before starting it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    /// The network has no metadata yet: generate it first.
    #[error("no metadata for this network yet")]
    NoMetadata,
    #[error("no network chosen")]
    NothingChosen,
    /// A chosen network has no render or its metadata breaks its rules.
    #[error("a chosen network cannot be exported")]
    Blocked,
    /// An export of this project is running or waiting.
    #[error("the project is already exporting")]
    AlreadyExporting,
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ExportError {
    /// What the Publishing stage says.
    pub fn message(&self) -> Text {
        match self {
            ExportError::ProjectNotFound => Text::ProjectNotFound,
            ExportError::NoAccounts => Text::ExportNoAccounts,
            ExportError::MissingKey(_) => Text::MetadataMissingKey,
            ExportError::Busy => Text::MetadataBusy,
            ExportError::OverBudget(_) => Text::BudgetReachedTitle,
            ExportError::NoMetadata => Text::MetadataMissing,
            ExportError::NothingChosen => Text::ExportNothingChosen,
            ExportError::Blocked => Text::ExportBlocked,
            ExportError::AlreadyExporting => Text::ExportAlreadyRunning,
            ExportError::Template(error) => error.message(),
            ExportError::Repository(_) => Text::ExportNotLoaded,
        }
    }
}

impl From<ScriptError> for ExportError {
    /// The project lookups are shared with scripts; only these come back.
    fn from(error: ScriptError) -> Self {
        match error {
            ScriptError::Repository(error) => ExportError::Repository(error),
            ScriptError::Template(error) => ExportError::Template(error),
            _ => ExportError::ProjectNotFound,
        }
    }
}

impl From<RenderError> for ExportError {
    fn from(error: RenderError) -> Self {
        match error {
            RenderError::Repository(error) => ExportError::Repository(error),
            _ => ExportError::ProjectNotFound,
        }
    }
}

/// Why a network cannot be exported now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportBlock {
    /// Nothing rendered for its account yet.
    NoRender,
    /// No metadata generated yet.
    NoMetadata,
    /// The metadata breaks the network's rules.
    Problems,
}

/// One network account of the channel at the Publishing stage.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportTarget {
    pub account: NetworkAccountId,
    pub network: Network,
    pub handle: String,
    /// The account's default visibility, written in the metadata file.
    pub visibility: Visibility,
    /// The account's description footer, added to the text.
    pub footer: String,
    /// The file last rendered for this account.
    pub render: Option<Render>,
    /// Whether `render` was made from the cut and preset as they are now.
    pub render_current: bool,
    pub metadata: Option<VideoMetadata>,
    /// What in the metadata breaks the network's rules.
    pub problems: Vec<MetadataProblem>,
    /// The network's last export.
    pub last: Option<Export>,
    /// Whether `last` holds the render and post as they are now.
    pub last_current: bool,
}

impl ExportTarget {
    /// The post as the network would take it now.
    pub fn post(&self) -> Option<Post> {
        self.metadata
            .as_ref()
            .map(|metadata| metadata.post(&self.footer, self.visibility))
    }

    /// What keeps the network from exporting, first thing first.
    pub fn block(&self) -> Option<ExportBlock> {
        if self.render.is_none() {
            Some(ExportBlock::NoRender)
        } else if self.metadata.is_none() {
            Some(ExportBlock::NoMetadata)
        } else if !self.problems.is_empty() {
            Some(ExportBlock::Problems)
        } else {
            None
        }
    }

    pub fn can_export(&self) -> bool {
        self.block().is_none()
    }
}

/// The project's Publishing stage: every network account and where its
/// metadata and export stand.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportView {
    pub project: VideoProjectId,
    /// Every network account of the channel, in `Network::ALL` order.
    pub targets: Vec<ExportTarget>,
    /// Whether the narrator's voice needs a synthetic-content disclosure.
    pub disclosure: bool,
    /// The project's latest metadata job.
    pub metadata_job: Option<Job>,
    /// The project's latest export job.
    pub export_job: Option<Job>,
    /// The template version the next generation uses.
    pub template: TemplateVersion,
    /// What generating the metadata would cost.
    pub estimate: SpendEstimate,
    /// What the current metadata cost, once generated.
    pub metadata_cost: Option<Cost>,
    /// The project's folder under the export root.
    pub package: String,
}

impl ExportView {
    /// The networks the user may pick: those with nothing in the way.
    pub fn exportable(&self) -> Vec<Network> {
        self.targets
            .iter()
            .filter(|target| target.can_export())
            .map(|target| target.network)
            .collect()
    }

    /// The generation the networks' metadata came from, if any.
    pub fn generation(&self) -> Option<&Generation> {
        self.targets
            .iter()
            .find_map(|target| target.metadata.as_ref())
            .map(VideoMetadata::generation)
    }
}

/// Where a project's exports stand, for its Publishing stage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportSummary {
    /// Networks with a rendered file.
    pub rendered: usize,
    /// Of those, exported as they are now.
    pub exported: usize,
    /// Exported from an earlier render or metadata.
    pub outdated: usize,
    /// The project's latest export job.
    pub job: Option<Job>,
}

/// The metadata job's payload: the rendered prompt, where it came from,
/// and the networks it writes for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MetadataPayload {
    project: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
    networks: Vec<String>,
}

impl MetadataPayload {
    fn template(&self) -> Result<TemplateUsed, JobFailure> {
        Ok(TemplateUsed {
            id: id::<TemplateVersionId>(&self.template)?,
            number: self.template_number,
        })
    }
}

/// What Claude answers, as the schema says.
#[derive(Deserialize)]
struct Answer {
    posts: Vec<AnsweredPost>,
}

#[derive(Deserialize)]
struct AnsweredPost {
    network: String,
    title: String,
    description: String,
    tags: Vec<String>,
}

/// One network of an export job: what to copy and write, and what it was
/// made from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExportOrder {
    network: String,
    account: String,
    render: String,
    render_file: String,
    video_file: String,
    /// The metadata file's text, in the language of the screen it was
    /// exported from.
    metadata: String,
    post: String,
    /// The video file of the network's last export, removed when the new
    /// one has another name.
    previous: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExportPayload {
    project: String,
    package: String,
    orders: Vec<ExportOrder>,
}

/// The networks an export job has written, by network code.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ExportCheckpoint {
    done: Vec<String>,
}

/// How many networks an export job has written, of how many.
pub fn export_job_networks(job: &Job) -> (usize, usize) {
    let total = serde_json::from_str::<ExportPayload>(job.payload())
        .map_or(0, |payload| payload.orders.len());
    let done = job
        .checkpoint()
        .and_then(|checkpoint| serde_json::from_str::<ExportCheckpoint>(checkpoint).ok())
        .map_or(0, |checkpoint| checkpoint.done.len());
    (done.min(total), total)
}

/// The project a metadata or export job serves.
#[derive(Deserialize)]
struct ProjectOf {
    project: String,
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// The Claude call that writes the metadata from `rendered`.
fn metadata_call(rendered: &RenderedPrompt) -> PlannedCall {
    PlannedCall::new(Provider::Claude, CostPurpose::Metadata, 1)
        .with_prompt(&rendered.instructions, &rendered.prompt)
}

/// One account as the metadata template's networks list describes it to
/// Claude, in English: its fields and limits, language and default tags.
fn network_line(account: &NetworkAccount, language: &str) -> String {
    let rules = account.network.metadata_rules();
    let mut parts = Vec::new();
    match rules.title {
        Some(limit) => parts.push(format!("title up to {limit} characters")),
        None => parts.push("no title (leave it empty)".to_owned()),
    }
    match (rules.text, rules.title) {
        (Some(limit), Some(_)) => parts.push(format!("description up to {limit} characters")),
        (Some(limit), None) => parts.push(format!(
            "caption (the description field) up to {limit} characters, hashtags included"
        )),
        (None, _) => parts.push("no description (leave it empty)".to_owned()),
    }
    let tags = match (rules.tags, rules.max_tags, rules.max_tag_chars) {
        (TagPlacement::Field, _, Some(chars)) => {
            format!("tags in a field of their own, up to {chars} characters in all")
        }
        (TagPlacement::Field, Some(count), None) => format!("up to {count} tags"),
        (TagPlacement::Hashtags, Some(count), _) => {
            format!("up to {count} tags, posted as hashtags at the end of the caption")
        }
        _ => "a few tags, posted as hashtags at the end of the caption".to_owned(),
    };
    parts.push(tags);
    parts.push(format!("written in {language}"));
    let metadata = account.details.metadata();
    if !metadata.tags().is_empty() {
        parts.push(format!(
            "account's default tags: {}",
            metadata.tags().join(", ")
        ));
    }
    let footer = metadata.description_footer().chars().count();
    if footer > 0 && rules.text.is_some() {
        parts.push(format!("the account adds a footer of {footer} characters"));
    }
    format!(
        "- {} (@{}): {}.",
        account.network.code(),
        account.details.handle(),
        parts.join("; ")
    )
}

/// A length as `m:ss`.
fn clock(length: Duration) -> String {
    let seconds = length.as_secs_f64().round() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Runs metadata jobs.
pub(crate) struct MetadataHandler {
    pub(crate) owner: ProfileId,
    pub(crate) exports: Arc<dyn ExportRepository>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for MetadataHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.generate(payload, cx.id())
    }
}

impl MetadataHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::Claude)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no Claude key is saved"))
    }

    /// Generates the metadata `job` asked for and saves it in place of
    /// the networks' metadata.
    fn generate(&self, payload: &str, job: JobId) -> Result<(), JobFailure> {
        let payload: MetadataPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        // An earlier attempt may have saved and stopped before the queue
        // recorded it as done.
        let saved = self.exports.video_metadata(project).map_err(unexpected)?;
        if saved
            .iter()
            .any(|metadata| metadata.generation().job == Some(job))
        {
            return Ok(());
        }

        let key = self.key()?;
        let request = TextRequest {
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            format: TextFormat::Json {
                schema: METADATA_SCHEMA.to_owned(),
            },
        };
        let generated = self.text.generate(&key, &request).map_err(|failure| {
            JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
        })?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::Metadata,
                usage: generated.usage.into(),
                job,
                reported: None,
            },
            project,
        );
        let not_usable = |detail: String| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Claude: the metadata is not usable ({detail})"),
            )
        };
        let answer: Answer =
            serde_json::from_str(&generated.text).map_err(|e| not_usable(e.to_string()))?;
        let now = SystemTime::now();
        let generation = Generation {
            id: GenerationId::new(),
            owner: self.owner,
            project,
            provider: Provider::Claude,
            model: generated.model,
            template: payload.template()?,
            instructions: payload.instructions,
            prompt: payload.prompt,
            output: generated.text,
            usage: generated.usage,
            generated_at: now,
            job: Some(job),
        };
        // The first post for each network asked for; others are ignored.
        let metadata: Vec<VideoMetadata> = payload
            .networks
            .iter()
            .filter_map(|code| {
                let network: Network = code.parse().ok()?;
                let post = answer.posts.iter().find(|post| post.network == *code)?;
                Some(VideoMetadata::generated(
                    network,
                    VideoMetadataDraft {
                        title: post.title.clone(),
                        description: post.description.clone(),
                        tags: post.tags.clone(),
                    },
                    generation.clone(),
                    now,
                ))
            })
            .collect();
        if metadata.is_empty() {
            return Err(not_usable("no post for any network asked".to_owned()));
        }
        self.exports
            .save_video_metadata(&metadata)
            .map_err(unexpected)
    }
}

/// Writes export packages, one network after another.
pub(crate) struct ExportHandler {
    pub(crate) owner: ProfileId,
    pub(crate) exports: Arc<dyn ExportRepository>,
    pub(crate) files: Arc<dyn ProjectFiles>,
    pub(crate) export_files: Arc<dyn ExportFiles>,
}

impl JobHandler for ExportHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: ExportPayload = parse(payload)?;
        let project: VideoProjectId = id(&payload.project)?;
        let mut checkpoint: ExportCheckpoint = match cx.checkpoint() {
            Some(text) => parse(text)?,
            None => ExportCheckpoint::default(),
        };
        let total = payload.orders.len() as u64;
        for order in &payload.orders {
            if checkpoint.done.contains(&order.network) {
                continue;
            }
            if cx.should_stop() {
                return Ok(());
            }
            let network: Network = order
                .network
                .parse()
                .map_err(|_| JobFailure::unexpected("unknown network"))?;
            let file_failure = |error: bardo_domain::ProjectFileError| {
                JobFailure::new(JobFailureKind::Media, format!("{network}: {error}"))
            };
            let mut source = self
                .files
                .open(project, &order.render_file)
                .map_err(file_failure)?;
            self.export_files
                .copy_from(&payload.package, network, &order.video_file, &mut source)
                .map_err(file_failure)?;
            self.export_files
                .write(
                    &payload.package,
                    network,
                    METADATA_FILE,
                    order.metadata.as_bytes(),
                )
                .map_err(file_failure)?;
            if let Some(previous) = order
                .previous
                .as_ref()
                .filter(|previous| **previous != order.video_file)
            {
                // A leftover only costs disk space; the export stands.
                if let Err(error) = self
                    .export_files
                    .remove(&payload.package, network, previous)
                {
                    tracing::warn!("could not remove the last export's video: {error}");
                }
            }
            let export = Export {
                project,
                owner: self.owner,
                network,
                account: id(&order.account)?,
                package: payload.package.clone(),
                video_file: order.video_file.clone(),
                render: id(&order.render)?,
                post: order.post.clone(),
                exported_at: SystemTime::now(),
            };
            self.exports.save_export(&export).map_err(unexpected)?;
            checkpoint.done.push(order.network.clone());
            let progress = Progress::of(checkpoint.done.len() as u64, total);
            cx.save_checkpoint(to_json(&checkpoint), progress)
                .map_err(unexpected)?;
        }
        Ok(())
    }
}

impl Bardo {
    fn export_project(&self, id: VideoProjectId) -> Result<VideoProject, ExportError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ExportError::ProjectNotFound)
    }

    fn latest_job_of(&self, kind: JobKind, project: VideoProjectId) -> Option<Job> {
        let project = project.to_string();
        self.jobs().into_iter().rev().find(|job| {
            job.kind() == kind
                && serde_json::from_str::<ProjectOf>(job.payload())
                    .is_ok_and(|of| of.project == project)
        })
    }

    /// The facts the metadata template is filled with: the script's, plus
    /// the script itself and the networks to write for.
    fn metadata_values(
        &self,
        project: &VideoProject,
        accounts: &[NetworkAccount],
    ) -> Result<TemplateValues, ExportError> {
        let mut values = self.script_values(project)?;
        let script = self
            .scripts
            .script(project.id)?
            .map(|script| script.text().as_str().to_owned())
            .unwrap_or_else(|| "no script yet".to_owned());
        let channel_language = self
            .channels
            .get(project.channel)?
            .map(|channel| channel.details.language())
            .ok_or(ExportError::ProjectNotFound)?;
        let english = Catalog::load(UiLanguage::EnUs);
        let networks = accounts
            .iter()
            .map(|account| {
                let language = account
                    .details
                    .metadata()
                    .language()
                    .unwrap_or(channel_language);
                network_line(account, &english.get(Text::ContentLanguageName(language)))
            })
            .collect::<Vec<_>>()
            .join("\n");
        values.insert(TemplateVariable::Script, script);
        values.insert(TemplateVariable::Networks, networks);
        Ok(values)
    }

    fn render_metadata_prompt(
        &self,
        project: &VideoProject,
        accounts: &[NetworkAccount],
        template: &TemplateVersion,
    ) -> Result<RenderedPrompt, ExportError> {
        template
            .body
            .render(&self.metadata_values(project, accounts)?)
            .map_err(|missing| {
                // Every metadata variable has a value above.
                ExportError::Repository(RepositoryError(Box::new(missing)))
            })
    }

    /// Whether posts of the project need a synthetic-content disclosure:
    /// its narrator's voice is flagged as a clone or a realistic synthetic
    /// voice, and the narration was generated with it (a recording the
    /// user imported is their own voice).
    pub fn needs_disclosure(&self, project: VideoProjectId) -> Result<bool, ExportError> {
        let project = self.export_project(project)?;
        let imported = self
            .narrations
            .narration(project.id)?
            .is_some_and(|narration| matches!(narration.source, NarrationSource::Imported { .. }));
        if imported {
            return Ok(false);
        }
        Ok(self
            .narrator(&project)?
            .is_some_and(|persona| persona.details.realistic_voice()))
    }

    /// What the job that wrote the current metadata cost, priced with
    /// today's rates when it had none: unpriced if any of its calls still
    /// has no rate.
    fn generation_cost(&self, project: VideoProjectId, job: Option<JobId>) -> Option<Cost> {
        let job = job?;
        let rates = self.cost_book.rates().ok()?;
        let costs: Vec<Cost> = self
            .cost_book
            .costs
            .project_costs(project)
            .ok()?
            .into_iter()
            .filter(|record| record.job == Some(job))
            .map(|record| record.priced_with(&rates).cost)
            .collect();
        if costs.is_empty() {
            None
        } else if costs.contains(&Cost::Unpriced) {
            Some(Cost::Unpriced)
        } else {
            Some(Cost::Estimated(
                costs.iter().map(|cost| cost.amount()).sum(),
            ))
        }
    }

    /// The project's Publishing stage.
    pub fn export_view(&self, project: VideoProjectId) -> Result<ExportView, ExportError> {
        let project = self.export_project(project)?;
        let accounts = self.network_accounts.list(project.channel)?;
        let renders = self.renders.renders(project.id)?;
        let current: Vec<NetworkAccountId> = match self.render_review(project.id) {
            Ok(review) => review
                .targets
                .iter()
                .filter(|target| target.last_current)
                .map(|target| target.account)
                .collect(),
            Err(RenderError::NoCut) => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        let metadata = self.exports.video_metadata(project.id)?;
        let exports = self.exports.exports(project.id)?;
        let targets = accounts
            .iter()
            .map(|account| {
                let defaults = account.details.metadata();
                let footer = defaults.description_footer().to_owned();
                let visibility = defaults.visibility();
                let render = renders
                    .iter()
                    .find(|render| render.account == account.id)
                    .cloned();
                let metadata = metadata
                    .iter()
                    .find(|metadata| metadata.network == account.network)
                    .cloned();
                let problems = metadata
                    .as_ref()
                    .map(|metadata| metadata.problems(&footer))
                    .unwrap_or_default();
                let post = metadata
                    .as_ref()
                    .map(|metadata| metadata.post(&footer, visibility));
                let last = exports
                    .iter()
                    .find(|export| export.network == account.network)
                    .cloned();
                let last_current = last.as_ref().is_some_and(|last| {
                    render
                        .as_ref()
                        .is_some_and(|render| render.id == last.render)
                        && post
                            .as_ref()
                            .is_some_and(|post| post.fingerprint() == last.post)
                });
                ExportTarget {
                    account: account.id,
                    network: account.network,
                    handle: account.details.handle().to_owned(),
                    visibility,
                    footer,
                    render_current: current.contains(&account.id),
                    render,
                    metadata,
                    problems,
                    last,
                    last_current,
                }
            })
            .collect::<Vec<_>>();
        let template = self.current_template(TemplateKind::Metadata)?;
        let rendered = self.render_metadata_prompt(&project, &accounts, &template)?;
        let metadata_job = self.latest_job_of(JobKind::Metadata, project.id);
        let metadata_cost = targets
            .iter()
            .find_map(|target| target.metadata.as_ref())
            .and_then(|metadata| self.generation_cost(project.id, metadata.generation().job));
        Ok(ExportView {
            project: project.id,
            disclosure: self.needs_disclosure(project.id)?,
            metadata_job,
            export_job: self.latest_job_of(JobKind::Export, project.id),
            estimate: self.estimate(&[metadata_call(&rendered)])?,
            template,
            metadata_cost,
            package: package_folder(&project.title, project.id),
            targets,
        })
    }

    /// Where the project's exports stand: how many networks have a render,
    /// how many are exported as they are now, and the latest export job.
    pub fn export_summary(&self, project: VideoProjectId) -> Result<ExportSummary, ExportError> {
        let view = self.export_view(project)?;
        let rendered = view.targets.iter().filter(|t| t.render.is_some());
        let exported = rendered.clone().filter(|t| t.last_current).count();
        let outdated = rendered
            .clone()
            .filter(|t| t.last.is_some() && !t.last_current)
            .count();
        Ok(ExportSummary {
            rendered: rendered.count(),
            exported,
            outdated,
            job: view.export_job,
        })
    }

    /// Starts a job in which Claude writes every network's title,
    /// description and tags from the current metadata template. The new
    /// metadata replaces the current one, edits included. Past Claude's
    /// budget it needs `consent`.
    pub fn generate_metadata(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, ExportError> {
        let project = self.export_project(project)?;
        if self
            .latest_job_of(JobKind::Metadata, project.id)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(ExportError::Busy);
        }
        let accounts = self.network_accounts.list(project.channel)?;
        if accounts.is_empty() {
            return Err(ExportError::NoAccounts);
        }
        if self.provider_key(Provider::Claude).state == KeyState::NotSet {
            return Err(ExportError::MissingKey(Provider::Claude));
        }
        let template = self.current_template(TemplateKind::Metadata)?;
        let rendered = self.render_metadata_prompt(&project, &accounts, &template)?;
        if let Err(estimate) = self.check_budget(&[metadata_call(&rendered)], consent)? {
            return Err(ExportError::OverBudget(estimate));
        }
        let payload = MetadataPayload {
            project: project.id.to_string(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
            networks: accounts
                .iter()
                .map(|account| account.network.code().to_owned())
                .collect(),
        };
        let job = Job::new(self.profile.id, JobKind::Metadata, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// Replaces a network's metadata with the user's. Text over the
    /// network's limits is kept, flagged, and keeps that network out of an
    /// export until it fits.
    pub fn edit_metadata(
        &self,
        project: VideoProjectId,
        network: Network,
        draft: VideoMetadataDraft,
    ) -> Result<VideoMetadata, ExportError> {
        let project = self.export_project(project)?;
        let mut metadata = self
            .exports
            .video_metadata(project.id)?
            .into_iter()
            .find(|metadata| metadata.network == network)
            .ok_or(ExportError::NoMetadata)?;
        if metadata.edit(draft, SystemTime::now()) {
            self.exports
                .save_video_metadata(std::slice::from_ref(&metadata))?;
        }
        Ok(metadata)
    }

    /// Queues the export of the chosen networks as `view` shows them:
    /// each one's render and post as they are now. Refuses a network that
    /// cannot be exported.
    pub fn start_export(
        &self,
        view: &ExportView,
        chosen: &[Network],
    ) -> Result<JobId, ExportError> {
        let project = self.export_project(view.project)?;
        if self
            .latest_job_of(JobKind::Export, project.id)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(ExportError::AlreadyExporting);
        }
        // What is stored now, not what the screen last read.
        let now = self.export_view(project.id)?;
        let targets: Vec<&ExportTarget> = now
            .targets
            .iter()
            .filter(|target| chosen.contains(&target.network))
            .collect();
        if targets.is_empty() {
            return Err(ExportError::NothingChosen);
        }
        if targets.iter().any(|target| !target.can_export()) {
            return Err(ExportError::Blocked);
        }
        let orders = targets
            .iter()
            .map(|target| {
                let render = target.render.as_ref().ok_or(ExportError::Blocked)?;
                let post = target.post().ok_or(ExportError::Blocked)?;
                let video_file = video_file_name(post.title.as_deref().unwrap_or(&project.title));
                Ok(ExportOrder {
                    network: target.network.code().to_owned(),
                    account: target.account.to_string(),
                    render: render.id.to_string(),
                    render_file: render.file.clone(),
                    metadata: self.metadata_file(
                        target,
                        &post,
                        render,
                        &video_file,
                        now.disclosure,
                    ),
                    post: post.fingerprint(),
                    previous: target
                        .last
                        .as_ref()
                        .filter(|last| last.package == now.package)
                        .map(|last| last.video_file.clone()),
                    video_file,
                })
            })
            .collect::<Result<Vec<_>, ExportError>>()?;
        let payload = ExportPayload {
            project: project.id.to_string(),
            package: now.package.clone(),
            orders,
        };
        let job = Job::new(self.profile.id, JobKind::Export, to_json(&payload));
        Ok(self.jobs.enqueue(job)?)
    }

    /// The project's latest export job.
    pub fn export_job(&self, project: VideoProjectId) -> Option<Job> {
        self.latest_job_of(JobKind::Export, project)
    }

    /// The project's latest metadata job.
    pub fn metadata_job(&self, project: VideoProjectId) -> Option<Job> {
        self.latest_job_of(JobKind::Metadata, project)
    }

    /// Where a network's export folder is on disk.
    pub fn export_folder(&self, export: &Export) -> PathBuf {
        self.export_files.folder(&export.package, export.network)
    }

    /// The metadata file of one network's export, in the interface
    /// language: what to paste in each field of the upload form, the
    /// visibility to pick, and the disclosure to apply when it is needed.
    fn metadata_file(
        &self,
        target: &ExportTarget,
        post: &Post,
        render: &Render,
        video_file: &str,
        disclosure: bool,
    ) -> String {
        let (width, height) = render.preset.dimensions();
        let mut blocks = vec![
            format!(
                "{} · @{}",
                self.text(Text::NetworkName(target.network)),
                target.handle
            ),
            self.text_with(
                Text::ExportFileVideo,
                &[
                    ("file", video_file),
                    ("size", &format!("{width}×{height}")),
                    ("aspect", render.preset.aspect.code()),
                    ("length", &clock(render.duration)),
                ],
            ),
        ];
        let field = |label: Text, value: &str| format!("{}\n{value}", self.text(label));
        if let Some(title) = &post.title {
            blocks.push(field(Text::ExportFileTitle, title));
        }
        if let Some(text) = &post.text {
            let label = if post.title.is_some() {
                Text::ExportFileDescription
            } else {
                Text::ExportFileCaption
            };
            blocks.push(field(label, text));
        }
        if !post.tags.is_empty() {
            blocks.push(field(Text::ExportFileTags, &post.tags.join(", ")));
        }
        blocks.push(self.text_with(
            Text::ExportFileVisibility,
            &[(
                "visibility",
                &self.text(Text::VisibilityName(post.visibility)),
            )],
        ));
        if disclosure {
            blocks.push(self.disclosure_text(target.network));
        }
        let mut text = blocks.join("\n\n");
        text.push('\n');
        // Notepad and every upload form take Windows line ends.
        text.replace('\n', "\r\n")
    }

    /// The disclosure reminder for one network, as one sentence.
    pub fn disclosure_text(&self, network: Network) -> String {
        format!(
            "{} {}",
            self.text(Text::DisclosureReminder),
            self.text(Text::DisclosureHow(network))
        )
    }

    /// A metadata problem as one sentence, with the network's limit.
    pub fn metadata_problem_text(&self, problem: MetadataProblem, network: Network) -> String {
        let rules = network.metadata_rules();
        let limit = match problem {
            MetadataProblem::TitleTooLong => rules.title,
            MetadataProblem::TextTooLong => rules.text,
            MetadataProblem::TooManyTags => rules.max_tags,
            MetadataProblem::TagsTooLong => rules.max_tag_chars,
            _ => None,
        };
        self.text_with(
            Text::MetadataProblem(problem),
            &[("limit", &limit.map(|n| n.to_string()).unwrap_or_default())],
        )
    }

    /// The live counters of a draft for `network`: what breaks its rules
    /// with the account's footer, before saving.
    pub fn metadata_draft_problems(
        &self,
        network: Network,
        draft: &VideoMetadataDraft,
        footer: &str,
    ) -> Vec<MetadataProblem> {
        problems(network, draft, footer)
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        CostRepository, JobState, Meter, Money, NetworkAccountDraft, PersonaDetails, PersonaDraft,
        PersonaRepository, TokenUsage,
    };
    use bardo_storage::MemoryExportFiles;

    use super::*;
    use crate::render::tests::render_all;
    use crate::scenes::tests::{Harness, done, wait_done};

    const CLAUDE_KEY: &str = "sk-ant-api03-test-key-0001";

    fn add_account(
        app: &Bardo,
        project: &VideoProject,
        network: Network,
        draft: NetworkAccountDraft,
    ) {
        app.add_network_account(
            project.channel,
            network,
            NetworkAccountDraft {
                handle: "archives".into(),
                ..draft
            },
        )
        .unwrap();
    }

    fn answer(posts: serde_json::Value) -> String {
        serde_json::json!({ "posts": posts }).to_string()
    }

    fn youtube_and_tiktok() -> String {
        answer(serde_json::json!([
            {
                "network": "youtube",
                "title": "The probe nobody found",
                "description": "In 1969 a probe went silent.",
                "tags": ["space history", "nasa"]
            },
            {
                "network": "tiktok",
                "title": "",
                "description": "The probe nobody found.",
                "tags": ["#space", "nasa"]
            }
        ]))
    }

    struct Setup {
        h: Harness,
        app: Bardo,
        exports: Arc<MemoryExportFiles>,
        project: VideoProject,
    }

    /// A drawn project with YouTube and TikTok accounts, rendered for both.
    fn rendered() -> Setup {
        let h = Harness::new();
        let exports: Arc<MemoryExportFiles> = Arc::default();
        let mut app = h.start_with_exports(Arc::clone(&exports) as _);
        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        let (project, _) = h.drawn_project(&app);
        add_account(
            &app,
            &project,
            Network::YouTube,
            NetworkAccountDraft {
                tags: vec!["space".into()],
                description_footer: "Sources in the channel.".into(),
                ..NetworkAccountDraft::default()
            },
        );
        add_account(
            &app,
            &project,
            Network::TikTok,
            NetworkAccountDraft::default(),
        );
        render_all(&app, project.id);
        Setup {
            h,
            app,
            exports,
            project,
        }
    }

    fn generated(s: &Setup) -> ExportView {
        s.h.text.answers.lock().unwrap().push(youtube_and_tiktok());
        *s.h.text.usage.lock().unwrap() = TokenUsage {
            input_tokens: 2_000,
            output_tokens: 600,
        };
        done(
            &s.app,
            s.app
                .generate_metadata(s.project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        s.app.export_view(s.project.id).unwrap()
    }

    fn target(view: &ExportView, network: Network) -> &ExportTarget {
        view.targets
            .iter()
            .find(|target| target.network == network)
            .unwrap()
    }

    #[test]
    fn claude_writes_each_networks_metadata_from_the_script_and_the_accounts() {
        let s = rendered();
        let before = s.app.export_view(s.project.id).unwrap();
        assert_eq!(before.targets.len(), 2);
        assert!(
            before
                .targets
                .iter()
                .all(|t| t.block() == Some(ExportBlock::NoMetadata))
        );
        assert!(before.exportable().is_empty());

        let view = generated(&s);
        let request = s.h.text.requests().pop().unwrap();
        assert!(matches!(request.format, TextFormat::Json { .. }));
        assert!(
            request
                .prompt
                .contains("- youtube (@archives): title up to 100 characters")
        );
        assert!(request.prompt.contains("account's default tags: space"));
        assert!(
            request
                .prompt
                .contains("the account adds a footer of 23 characters")
        );
        assert!(request.prompt.contains("- tiktok (@archives): no title"));
        assert!(request.prompt.contains("written in Portuguese"));
        let script = s.app.script(s.project.id).unwrap().script.unwrap();
        assert!(request.prompt.contains(script.text().as_str()));

        let youtube = target(&view, Network::YouTube);
        let metadata = youtube.metadata.as_ref().unwrap();
        assert_eq!(metadata.title(), "The probe nobody found");
        assert_eq!(metadata.tags(), ["space history", "nasa"]);
        assert!(youtube.problems.is_empty());
        let post = youtube.post().unwrap();
        assert_eq!(
            post.text.as_deref(),
            Some("In 1969 a probe went silent.\n\nSources in the channel.")
        );
        let tiktok = target(&view, Network::TikTok);
        assert_eq!(tiktok.metadata.as_ref().unwrap().tags(), ["space", "nasa"]);
        assert_eq!(
            tiktok.post().unwrap().text.as_deref(),
            Some("The probe nobody found.\n\n#space #nasa")
        );
        assert_eq!(view.exportable(), [Network::YouTube, Network::TikTok]);

        // Provenance and cost.
        let generation = view.generation().unwrap();
        assert_eq!(generation.model, "claude-fake");
        assert_eq!(generation.template.number, 1);
        assert_eq!(generation.usage.input_tokens, 2_000);
        assert_eq!(view.metadata_cost, Some(Cost::Unpriced));
        let records = s.h.db.project_costs(s.project.id).unwrap();
        let record = records
            .iter()
            .find(|record| record.purpose == CostPurpose::Metadata)
            .unwrap();
        assert_eq!(record.usage.input_tokens, 2_000);
        // A rate added later prices it: $4 and $20 per million tokens.
        s.app
            .save_rate(Provider::Claude, "claude-fake", Meter::InputTokens, "4")
            .unwrap();
        s.app
            .save_rate(Provider::Claude, "claude-fake", Meter::OutputTokens, "20")
            .unwrap();
        assert_eq!(
            s.app.export_view(s.project.id).unwrap().metadata_cost,
            Some(Cost::Estimated(Money::from_micros(8_000 + 12_000)))
        );
    }

    #[test]
    fn edits_over_a_limit_are_kept_flagged_and_block_the_export() {
        let s = rendered();
        generated(&s);
        let long = VideoMetadataDraft {
            title: "t".repeat(101),
            description: "Edited.".into(),
            tags: vec!["moon".into()],
        };
        let edited = s
            .app
            .edit_metadata(s.project.id, Network::YouTube, long)
            .unwrap();
        assert!(edited.is_edited());
        let view = s.app.export_view(s.project.id).unwrap();
        let youtube = target(&view, Network::YouTube);
        assert_eq!(youtube.problems, [MetadataProblem::TitleTooLong]);
        assert_eq!(youtube.block(), Some(ExportBlock::Problems));
        assert_eq!(view.exportable(), [Network::TikTok]);
        assert!(matches!(
            s.app.start_export(&view, &[Network::YouTube]),
            Err(ExportError::Blocked)
        ));
        assert!(
            s.app
                .metadata_problem_text(MetadataProblem::TitleTooLong, Network::YouTube)
                .contains("100")
        );
        assert!(matches!(
            s.app
                .edit_metadata(s.project.id, Network::X, VideoMetadataDraft::default()),
            Err(ExportError::NoMetadata)
        ));
    }

    #[test]
    fn exporting_writes_a_folder_per_network_with_the_video_and_metadata() {
        let s = rendered();
        let view = generated(&s);
        assert!(matches!(
            s.app.start_export(&view, &[]),
            Err(ExportError::NothingChosen)
        ));
        let job = done(
            &s.app,
            s.app
                .start_export(&view, &[Network::YouTube, Network::TikTok])
                .unwrap(),
        );
        assert_eq!(export_job_networks(&job), (2, 2));

        let package = &view.package;
        assert!(package.starts_with(&s.project.title.chars().take(8).collect::<String>()));
        assert_eq!(
            s.exports.names(package, Network::YouTube),
            ["The probe nobody found.mp4", "metadata.txt"]
        );
        let rendered = s.h.files.read(s.project.id, "render-youtube.mp4").unwrap();
        assert_eq!(
            s.exports
                .read(package, Network::YouTube, "The probe nobody found.mp4")
                .unwrap(),
            rendered
        );
        // TikTok has no title: the video is named after the project.
        let tiktok_video = video_file_name(&s.project.title);
        assert_eq!(
            s.exports.names(package, Network::TikTok),
            [tiktok_video.as_str(), "metadata.txt"]
        );

        let text = String::from_utf8(
            s.exports
                .read(package, Network::YouTube, METADATA_FILE)
                .unwrap(),
        )
        .unwrap();
        assert!(text.contains("YouTube · @archives\r\n"), "{text}");
        assert!(text.contains("\r\nThe probe nobody found\r\n"), "{text}");
        assert!(
            text.contains("In 1969 a probe went silent.\r\n\r\nSources in the channel."),
            "{text}"
        );
        assert!(text.contains("space history, nasa"), "{text}");
        assert!(text.contains("1080×1920"), "{text}");
        assert!(!text.contains('\u{26a0}') && !text.to_lowercase().contains("synthetic"));
        let caption = String::from_utf8(
            s.exports
                .read(package, Network::TikTok, METADATA_FILE)
                .unwrap(),
        )
        .unwrap();
        assert!(caption.contains("#space #nasa"), "{caption}");

        let view = s.app.export_view(s.project.id).unwrap();
        assert!(view.targets.iter().all(|t| t.last_current));
        let summary = s.app.export_summary(s.project.id).unwrap();
        assert_eq!(
            (summary.rendered, summary.exported, summary.outdated),
            (2, 2, 0)
        );
        assert_eq!(
            s.app
                .export_folder(target(&view, Network::YouTube).last.as_ref().unwrap()),
            PathBuf::from("memory").join(package).join("YouTube")
        );
    }

    #[test]
    fn a_new_title_replaces_the_exported_video_and_outdates_the_export() {
        let s = rendered();
        let view = generated(&s);
        done(
            &s.app,
            s.app.start_export(&view, &[Network::YouTube]).unwrap(),
        );
        s.app
            .edit_metadata(
                s.project.id,
                Network::YouTube,
                VideoMetadataDraft {
                    title: "A better title".into(),
                    description: "In 1969 a probe went silent.".into(),
                    tags: vec!["nasa".into()],
                },
            )
            .unwrap();
        let view = s.app.export_view(s.project.id).unwrap();
        let youtube = target(&view, Network::YouTube);
        assert!(youtube.last.is_some() && !youtube.last_current);
        assert_eq!(s.app.export_summary(s.project.id).unwrap().outdated, 1);

        done(
            &s.app,
            s.app.start_export(&view, &[Network::YouTube]).unwrap(),
        );
        assert_eq!(
            s.exports.names(&view.package, Network::YouTube),
            ["A better title.mp4", "metadata.txt"],
            "the old video is gone"
        );
    }

    #[test]
    fn a_network_without_a_render_cannot_export() {
        let s = rendered();
        add_account(
            &s.app,
            &s.project,
            Network::X,
            NetworkAccountDraft::default(),
        );
        let view = s.app.export_view(s.project.id).unwrap();
        let x = target(&view, Network::X);
        assert_eq!(x.block(), Some(ExportBlock::NoRender));
        assert!(matches!(
            s.app.start_export(&view, &[Network::X]),
            Err(ExportError::Blocked)
        ));
    }

    #[test]
    fn a_realistic_voice_adds_the_disclosure_to_the_screen_and_each_file() {
        let s = rendered();
        assert!(!s.app.needs_disclosure(s.project.id).unwrap());
        // Flag the channel's narrator.
        let narrator = s
            .app
            .personas()
            .unwrap()
            .into_iter()
            .find(|p| p.details.name() == "Documentary Narrator (en-US)")
            .unwrap();
        let mut flagged = narrator.clone();
        flagged.details = PersonaDetails::validate(PersonaDraft {
            realistic_voice: true,
            ..PersonaDraft::from(&narrator.details)
        })
        .unwrap();
        PersonaRepository::save(&*s.h.db, &flagged).unwrap();

        let view = generated(&s);
        assert!(view.disclosure);
        done(
            &s.app,
            s.app
                .start_export(&view, &[Network::YouTube, Network::TikTok])
                .unwrap(),
        );
        for network in [Network::YouTube, Network::TikTok] {
            let text = String::from_utf8(
                s.exports
                    .read(&view.package, network, METADATA_FILE)
                    .unwrap(),
            )
            .unwrap();
            assert!(text.contains(&s.app.disclosure_text(network)), "{text}");
        }
        assert_ne!(
            s.app.disclosure_text(Network::YouTube),
            s.app.disclosure_text(Network::X)
        );
    }

    #[test]
    fn generating_needs_accounts_a_key_and_no_running_job() {
        let h = Harness::new();
        let mut app = h.start();
        let (project, _) = h.drawn_project(&app);
        assert!(matches!(
            app.generate_metadata(project.id, BudgetConsent::Ask),
            Err(ExportError::NoAccounts)
        ));
        add_account(
            &app,
            &project,
            Network::Kick,
            NetworkAccountDraft::default(),
        );
        app.remove_provider_key(Provider::Claude).unwrap();
        assert!(matches!(
            app.generate_metadata(project.id, BudgetConsent::Ask),
            Err(ExportError::MissingKey(Provider::Claude))
        ));
        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        *h.text.delay.lock().unwrap() = Duration::from_millis(200);
        h.text
            .answers
            .lock()
            .unwrap()
            .push(answer(serde_json::json!([
                {"network": "kick", "title": "The probe", "description": "", "tags": ["space"]}
            ])));
        let job = app
            .generate_metadata(project.id, BudgetConsent::Ask)
            .unwrap();
        assert!(matches!(
            app.generate_metadata(project.id, BudgetConsent::Ask),
            Err(ExportError::Busy)
        ));
        done(&app, job);
        let view = app.export_view(project.id).unwrap();
        let kick = target(&view, Network::Kick);
        assert_eq!(kick.metadata.as_ref().unwrap().title(), "The probe");
        assert_eq!(kick.block(), Some(ExportBlock::NoRender));
    }

    #[test]
    fn an_answer_without_any_network_asked_fails_the_job() {
        let s = rendered();
        s.h.text
            .answers
            .lock()
            .unwrap()
            .push(answer(serde_json::json!([
                {"network": "x", "title": "", "description": "Hi", "tags": []}
            ])));
        let job = wait_done(
            &s.app,
            s.app
                .generate_metadata(s.project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().map(|failure| failure.kind),
            Some(JobFailureKind::UnexpectedAnswer)
        );
        let view = s.app.export_view(s.project.id).unwrap();
        assert!(view.targets.iter().all(|t| t.metadata.is_none()));
    }
}
