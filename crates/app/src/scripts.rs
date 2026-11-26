//! Script use cases (PRD stories 29-30, 43): Claude writes a video
//! project's script from the script template, filled with the channel,
//! niche, theme and the channel's default persona; the user edits it
//! freely. An ad project's script is written from the ad script template
//! instead, filled with its product, its angle and its brief, and the job
//! that writes it checks its claims (`claims_check`). Regenerating puts the new script up for review next to the
//! current one, which stays until the user accepts the new one. Every generation records its provenance.
//!
//! Generating calls Claude, so it runs as a job. The template is rendered
//! when the job starts: the job sends exactly the prompt it records, even if
//! the template is edited while it runs.

use std::sync::Arc;
use std::time::SystemTime;

use bardo_domain::{
    AdRepository, ApiKey, Channel, ChannelId, CheckedScript, CostPurpose, GeneratedScript,
    Generation, GenerationId, Job, JobFailure, JobFailureKind, JobId, JobKind, Money,
    NoPendingScript, ProfileId, Provider, RepositoryError, Script, ScriptFieldError,
    ScriptRepository, ScriptText, SecretStore, Stop, TemplateKind, TemplateUsed, TemplateValues,
    TemplateVariable, TemplateVersion, TemplateVersionId, TextEffort, TextFormat, TextGenerator,
    TextRequest, UiLanguage, VideoProject, VideoProjectId,
};
use serde::{Deserialize, Serialize};

use crate::ad_briefs::{ScriptBrief, unset_ad_values};
use crate::ads::{as_data, with_ad_rules, with_concept_rules};
use crate::claims_check::{
    CheckBrief, ScriptClaims, check_call_ahead, from_millis, millis, settle_checks,
};
use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler, Work};
use crate::{Bardo, Catalog, KeyState, TemplateError, Text};

/// What a variable says when the channel left it blank, so the prompt
/// never reads as cut off.
pub(crate) const NOT_SET: &str = "not set";
/// The narrator when the channel has no default persona.
pub(crate) const NO_PERSONA: &str = "no persona chosen yet; a clear, engaging documentary narrator";

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// The typed text breaks a rule; the editor shows it.
    #[error("invalid script: {0:?}")]
    Invalid(ScriptFieldError),
    #[error("channel not found")]
    ChannelNotFound,
    #[error("video project not found")]
    ProjectNotFound,
    /// The project has no script yet: generate one first.
    #[error("the project has no script yet")]
    NoScript,
    #[error(transparent)]
    NothingToReview(#[from] NoPendingScript),
    /// Generation calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// A script of the project is being generated.
    #[error("a script is already being generated for this project")]
    Busy,
    /// The generation would reach a provider's budget; the screen asks
    /// before starting it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    /// The typed spend cap is not an amount above zero.
    #[error("invalid spend cap")]
    CapInvalid,
    /// An ad project's script is written once its brief is confirmed.
    #[error("the brief is not confirmed yet")]
    BriefNotConfirmed,
    /// The brief was confirmed and has since lost what confirming needs:
    /// a claim, an answer about a claim that changed in the product, or
    /// the call to action.
    #[error("the brief is incomplete")]
    BriefNotReady,
    /// Only an ad project's script is checked against claims.
    #[error("the project is not an ad")]
    NotAnAd,
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ScriptError {
    /// What the projects screen says.
    pub fn message(&self) -> Text {
        match self {
            ScriptError::Invalid(error) => Text::ScriptFieldError(*error),
            ScriptError::ChannelNotFound => Text::ChannelNotFound,
            ScriptError::ProjectNotFound => Text::ProjectNotFound,
            ScriptError::NoScript => Text::ScriptMissing,
            ScriptError::NothingToReview(_) => Text::ScriptNothingToReview,
            ScriptError::MissingKey(_) => Text::ScriptMissingKey,
            ScriptError::Busy => Text::ScriptBusy,
            ScriptError::OverBudget(_) => Text::BudgetReachedTitle,
            ScriptError::CapInvalid => Text::SpendCapInvalid,
            ScriptError::BriefNotConfirmed => Text::ScriptBriefFirst,
            ScriptError::BriefNotReady => Text::ScriptBriefNotReady,
            ScriptError::NotAnAd => Text::BriefNotAnAd,
            ScriptError::Template(error) => error.message(),
            ScriptError::Repository(_) => Text::ScriptNotSaved,
        }
    }

    pub fn field_error(&self) -> Option<ScriptFieldError> {
        match self {
            ScriptError::Invalid(error) => Some(*error),
            _ => None,
        }
    }
}

/// A video project's script screen.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptView {
    pub project: VideoProject,
    /// `None` until the first script is generated.
    pub script: Option<Script>,
    /// The project's latest script job: writing it, or checking its
    /// claims.
    pub job: Option<Job>,
    /// The template version the next generation uses.
    pub template: TemplateVersion,
    /// What generating the script would cost.
    pub estimate: SpendEstimate,
    /// What the video cost so far, every step included.
    pub spent: Money,
    /// Where the brief stands, for an ad project: its script is written
    /// once the brief is confirmed, and reads it.
    pub brief: Option<crate::BriefStanding>,
    /// The claims checks of an ad project's scripts; `None` for any other
    /// project, and before a script is written.
    pub claims: Option<ScriptClaims>,
}

/// The script job's payload: the rendered prompt and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ScriptPayload {
    project: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
    /// For an ad: when the brief the prompt was rendered from last
    /// changed, in Unix milliseconds. A brief changed after that marks
    /// the script this job writes, however late the job runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brief_at: Option<u64>,
    /// For an ad: what the claims check of the script reads of the brief,
    /// as it was when the prompt was rendered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    check: Option<CheckBrief>,
}

impl ScriptPayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a script payload serializes")
    }

    fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid script payload: {e}")))
    }

    fn project(&self) -> Result<VideoProjectId, JobFailure> {
        uuid::Uuid::parse_str(&self.project)
            .map(VideoProjectId::from)
            .map_err(unexpected)
    }

    fn template(&self) -> Result<TemplateUsed, JobFailure> {
        Ok(TemplateUsed {
            id: uuid::Uuid::parse_str(&self.template)
                .map(TemplateVersionId::from)
                .map_err(unexpected)?,
            number: self.template_number,
        })
    }
}

fn unexpected(error: impl std::fmt::Display) -> JobFailure {
    JobFailure::unexpected(error.to_string())
}

/// Runs script generation jobs.
pub(crate) struct ScriptHandler {
    pub(crate) owner: ProfileId,
    pub(crate) scripts: Arc<dyn ScriptRepository>,
    /// Where an ad script's claims check is kept.
    pub(crate) ads: Arc<dyn AdRepository>,
    /// The post texts a check of an ad's posts reads (#282).
    pub(crate) exports: Arc<dyn bardo_domain::ExportRepository>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl ScriptHandler {
    pub(crate) fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::Claude)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no Claude key is saved"))
    }
}

impl JobHandler for ScriptHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        if cx.kind() == JobKind::ClaimsCheck {
            return self.check_on_request(payload, cx.id(), &cx.stop());
        }
        let (id, stop) = (cx.id(), cx.stop());
        self.generate(payload, id, &stop, cx)
    }
}

/// The script `job` wrote in an earlier attempt, while it is still one of
/// the project's: the new one waiting for review, or the one the current
/// script came from.
fn written_by(
    scripts: &dyn ScriptRepository,
    project: VideoProjectId,
    job: JobId,
) -> Result<Option<ScriptText>, RepositoryError> {
    let by_job = |generated: &GeneratedScript| generated.generation().job == Some(job);
    Ok(scripts.script(project)?.and_then(|script| {
        if let Some(pending) = script.pending().filter(|pending| by_job(pending)) {
            Some(pending.text().clone())
        } else if by_job(script.source()) {
            Some(script.source().text().clone())
        } else {
            None
        }
    }))
}

/// What a script job's checkpoint reads once its script is saved.
const WRITTEN: &str = "written";

/// Where a script job notes that it wrote its script: the job's
/// checkpoint, or a plain flag for a handler run outside the queue. The
/// note outlives the script, which the user may put aside while the job
/// waits to run again.
pub(crate) trait WroteScript {
    fn wrote(&self) -> bool;
    fn note_written(&mut self);
}

impl WroteScript for JobContext {
    fn wrote(&self) -> bool {
        self.checkpoint() == Some(WRITTEN)
    }

    fn note_written(&mut self) {
        if let Err(error) =
            self.save_checkpoint(WRITTEN, bardo_domain::Progress::from_permille(500))
        {
            tracing::warn!(job = %self.id(), "could not note that a job wrote its script: {error}");
        }
    }
}

impl WroteScript for bool {
    fn wrote(&self) -> bool {
        *self
    }

    fn note_written(&mut self) {
        *self = true;
    }
}

impl ScriptHandler {
    /// Generates the script `job` asked for and saves it; an ad's script
    /// is then checked against the claims of its brief.
    fn generate(
        &self,
        payload: &str,
        job: JobId,
        stop: &Stop,
        noted: &mut dyn WroteScript,
    ) -> Result<(), JobFailure> {
        let payload = ScriptPayload::parse(payload)?;
        let project = payload.project()?;
        let brief_at = payload.brief_at.map(from_millis);
        let check = payload.check.clone();
        // An earlier attempt of this job may have saved its script and
        // stopped before its check, or before the queue recorded it as
        // done.
        let text = match written_by(&*self.scripts, project, job).map_err(unexpected)? {
            Some(text) => text,
            // It wrote one, and the user put it aside since: the job
            // neither writes another nor checks one nobody kept.
            None if noted.wrote() => return Ok(()),
            None => {
                let text = self.write(payload, project, job, stop)?;
                noted.note_written();
                text
            }
        };
        match check {
            Some(check) => self.check_claims(
                project,
                &text,
                &check,
                brief_at.unwrap_or_else(SystemTime::now),
                job,
                stop,
            ),
            None => Ok(()),
        }
    }

    /// Has Claude write the script and saves it: the project's first, or
    /// a new one beside the current. Returns its text.
    fn write(
        &self,
        payload: ScriptPayload,
        project: VideoProjectId,
        job: JobId,
        stop: &Stop,
    ) -> Result<ScriptText, JobFailure> {
        let key = self.key()?;
        let request = TextRequest {
            effort: TextEffort::Careful,
            answer_tokens: None,
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            images: Vec::new(),
            format: TextFormat::Prose,
        };
        let call = PlannedCall::text(CostPurpose::Script, &request);
        let paying = self.costs.paying_for_project(job, &call, project)?;
        let generated = self
            .text
            .generate_until(&key, &request, stop)
            .map_err(|failure| {
                // An answer that was lost or cut may still have been billed.
                self.costs
                    .record_failed_for_project(&failure, &call, Some(job), project);
                JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
            });
        drop(paying);
        let generated = generated?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::Script,
                usage: generated.usage.into(),
                job: Some(job),
                reported: None,
            },
            project,
        );
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
        let generated = GeneratedScript::new(generation).map_err(|error| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Claude: the script is not usable ({error:?})"),
            )
        })?;

        let text = generated.text().clone();
        // Read again: the user may have edited the script meanwhile, and
        // their text must survive.
        let script = match self.scripts.script(project).map_err(unexpected)? {
            Some(mut script) => {
                script.offer(generated, now);
                script
            }
            None => Script::first(generated, now),
        };
        self.scripts.save_script(&script).map_err(unexpected)?;
        // The check of a new script this one replaced is no script's now.
        settle_checks(&*self.ads, project, &script).map_err(unexpected)?;
        Ok(text)
    }
}

/// The Claude call that writes a script from `rendered`.
fn script_call(rendered: &bardo_domain::RenderedPrompt) -> PlannedCall {
    PlannedCall::new(Provider::Claude, CostPurpose::Script, 1)
        .with_prompt(&rendered.instructions, &rendered.prompt)
}

/// `value`, or `NOT_SET` when blank.
pub(crate) fn or_not_set(value: &str) -> String {
    if value.trim().is_empty() {
        NOT_SET.to_owned()
    } else {
        value.to_owned()
    }
}

impl Bardo {
    fn own_project(&self, id: VideoProjectId) -> Result<VideoProject, ScriptError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ScriptError::ProjectNotFound)
    }

    /// The project's latest script job; for a version, adapting it again
    /// counts too, and for an ad, checking its claims.
    fn latest_script_job(&self, project: VideoProjectId) -> Option<Job> {
        let id = project.to_string();
        self.jobs().into_iter().rev().find(|job| {
            (job.kind() == JobKind::ScriptGeneration
                && ScriptPayload::parse(job.payload()).is_ok_and(|p| p.project == id))
                || crate::localization::adapts_again(job, &id)
                || Self::checks_claims_of(job, &id)
        })
    }

    /// What a failed or cancelled script job, or claims check, still pays
    /// for when it is run again: the script it has yet to write and the
    /// check it has yet to make. `None` when `job` is neither, or nothing
    /// of it is left to pay for.
    pub(crate) fn script_calls_left(
        &self,
        job: &Job,
    ) -> Option<(VideoProjectId, Vec<PlannedCall>)> {
        if job.kind() == JobKind::ClaimsCheck {
            return self.check_call_left(job);
        }
        if job.kind() != JobKind::ScriptGeneration {
            return None;
        }
        let payload = ScriptPayload::parse(job.payload()).ok()?;
        let project = payload.project().ok()?;
        let calls = match written_by(&*self.scripts, project, job.id()).ok()? {
            Some(text) => {
                let check = payload.check.as_ref()?;
                if !self.check_left(project, &text, job.id()) {
                    return None;
                }
                vec![check.call(&text, bardo_domain::CheckedScript::Current)]
            }
            // It wrote its script, which was put aside since.
            None if job.checkpoint() == Some(WRITTEN) => return None,
            None => {
                let mut calls = vec![
                    PlannedCall::new(Provider::Claude, CostPurpose::Script, 1)
                        .with_prompt(&payload.instructions, &payload.prompt),
                ];
                if payload.check.is_some() {
                    calls.push(check_call_ahead());
                }
                calls
            }
        };
        Some((project, calls))
    }

    /// The template a project's script is written from: the ad script
    /// template for an ad, the script template for every other project.
    fn script_template(&self, project: &VideoProject) -> Result<TemplateVersion, ScriptError> {
        Ok(self.current_template(if project.is_ad() {
            TemplateKind::AdScript
        } else {
            TemplateKind::Script
        })?)
    }

    /// The facts an ad script template is filled with: the channel's and
    /// the video's, as any script's, and the ad's own from `brief`. What
    /// the user typed, a page said or a model wrote goes in as data.
    pub(crate) fn ad_script_values(
        &self,
        project: &VideoProject,
        brief: Option<&ScriptBrief>,
    ) -> Result<TemplateValues, ScriptError> {
        let mut values = self.script_values(project)?;
        for typed in [
            TemplateVariable::ChannelName,
            TemplateVariable::ChannelNiche,
            TemplateVariable::Persona,
            TemplateVariable::ThemeTitle,
            TemplateVariable::ThemeAngle,
        ] {
            if let Some(value) = values.get_mut(&typed) {
                *value = as_data(value);
            }
        }
        values.extend(match brief {
            Some(brief) => {
                let english = Catalog::load(UiLanguage::EnUs);
                brief.values(&english.get(Text::ProductKindName(brief.product.brief.kind())))
            }
            None => unset_ad_values(),
        });
        Ok(values)
    }

    /// The facts a script template is filled with, in English as providers
    /// read them best.
    pub(crate) fn script_values(
        &self,
        project: &VideoProject,
    ) -> Result<TemplateValues, ScriptError> {
        let channel = self
            .channels
            .get(project.channel)?
            .ok_or(ScriptError::ProjectNotFound)?;
        let angle = self
            .themes
            .theme(project.theme)?
            .map(|theme| theme.idea().angle().to_owned())
            .unwrap_or_default();
        let english = Catalog::load(UiLanguage::EnUs);
        let details = &channel.details;
        // The project's own persona, else the channel's default.
        let persona = self
            .narrator(project)?
            .map(|persona| persona.details.describe());
        let format = project.format(details.format());
        Ok(TemplateValues::from([
            (TemplateVariable::ChannelName, details.name().to_owned()),
            (TemplateVariable::ChannelNiche, or_not_set(details.niche())),
            (
                TemplateVariable::ChannelThemes,
                or_not_set(&details.themes().join("; ")),
            ),
            (
                TemplateVariable::AestheticNotes,
                or_not_set(details.aesthetic_notes()),
            ),
            (
                TemplateVariable::Language,
                english
                    .get(Text::ContentLanguageName(details.language()))
                    .into_owned(),
            ),
            (
                TemplateVariable::Country,
                english
                    .get(Text::CountryName(details.country()))
                    .into_owned(),
            ),
            (
                TemplateVariable::Persona,
                persona.unwrap_or_else(|| NO_PERSONA.to_owned()),
            ),
            (TemplateVariable::Niche, project.niche.label().to_owned()),
            (TemplateVariable::ThemeTitle, project.title.clone()),
            (TemplateVariable::ThemeAngle, or_not_set(&angle)),
            (
                TemplateVariable::TargetLength,
                project.target_length(format).to_owned(),
            ),
            (
                TemplateVariable::FrameShape,
                format.frame_shape().to_owned(),
            ),
            (
                TemplateVariable::VisualStyle,
                crate::styles::style_for_planner(project.style(details.style())),
            ),
        ]))
    }

    /// The channel's video projects, newest first.
    pub fn video_projects(&self, channel: ChannelId) -> Result<Vec<VideoProject>, ScriptError> {
        let channel: Channel = self
            .channels
            .get(channel)?
            .filter(|channel| channel.owner == self.profile.id)
            .ok_or(ScriptError::ChannelNotFound)?;
        Ok(self.themes.projects(channel.id)?)
    }

    /// The project's script screen.
    pub fn script(&self, project: VideoProjectId) -> Result<ScriptView, ScriptError> {
        let project = self.own_project(project)?;
        // A version is adapted again from what it carried.
        let (template, estimate, brief) = match self.localizations.localization(project.id)? {
            Some(localization) => {
                let (template, estimate) = self.adaptation_estimate(&project, &localization)?;
                (template, estimate, None)
            }
            None => {
                let template = self.script_template(&project)?;
                let (rendered, brief) = self.render_script(&project, &template)?;
                // An ad's script is checked by the job that writes it.
                let mut calls = vec![script_call(&rendered)];
                if project.is_ad() {
                    calls.push(check_call_ahead());
                }
                (template, self.estimate(&calls)?, brief)
            }
        };
        let script = self.scripts.script(project.id)?;
        Ok(ScriptView {
            claims: self.script_claims(&project, script.as_ref(), brief.as_ref())?,
            brief: self.brief_standing(&project, script.as_ref())?,
            script,
            job: self.latest_script_job(project.id),
            estimate,
            spent: self.project_spend(project.id)?,
            template,
            project,
        })
    }

    /// The project's script template (`script_template`) filled with its
    /// facts. An ad's request carries the rules of every ad after the
    /// template's instructions, and those of a concept for a product that
    /// is one, whatever the template says; with it, what it read of the
    /// ad's brief.
    pub(crate) fn render_script(
        &self,
        project: &VideoProject,
        template: &TemplateVersion,
    ) -> Result<(bardo_domain::RenderedPrompt, Option<ScriptBrief>), ScriptError> {
        let brief = self.script_brief(project)?;
        let values = if project.is_ad() {
            self.ad_script_values(project, brief.as_ref())?
        } else {
            self.script_values(project)?
        };
        let mut rendered = template.body.render(&values).map_err(|missing| {
            // Every variable of the kind has a value above.
            ScriptError::Repository(RepositoryError(Box::new(missing)))
        })?;
        if project.is_ad() {
            rendered.instructions = with_ad_rules(&rendered.instructions);
            if brief.as_ref().is_some_and(ScriptBrief::is_concept) {
                rendered.instructions = with_concept_rules(&rendered.instructions);
            }
            // The rules speak of the numbered claims: an edited template
            // that left them out still sends them.
            let fixed = brief
                .as_ref()
                .map(|brief| brief.fixed_for(&template.body))
                .unwrap_or_default();
            if !fixed.is_empty() {
                rendered.prompt = format!("{}\n\n{fixed}", rendered.prompt);
            }
        }
        Ok((rendered, brief))
    }

    /// When the request that wrote `script` read its project's brief: the
    /// time its job kept, else when the answer arrived.
    pub(crate) fn script_read_brief_at(&self, script: &Script) -> SystemTime {
        let generation = script.source().generation();
        generation
            .job
            .and_then(|id| self.jobs().into_iter().find(|job| job.id() == id))
            .and_then(|job| ScriptPayload::parse(job.payload()).ok())
            .and_then(|payload| payload.brief_at)
            .map_or(generation.generated_at, from_millis)
    }

    /// Starts a job in which Claude writes the project's script from its
    /// current template: the ad script template for an ad, whose script
    /// the job then checks against the brief's claims, and the script
    /// template for every other project. With a script already there, the
    /// new one waits for review. Past Claude's budget it needs `consent`.
    pub fn generate_script(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, ScriptError> {
        let project = self.own_project(project)?;
        if self.work_busy(&Work::Script(project.id.to_string())) {
            return Err(ScriptError::Busy);
        }
        if self.provider_key(Provider::Claude).state == KeyState::NotSet {
            return Err(ScriptError::MissingKey(Provider::Claude));
        }
        if let Some(localization) = self.localizations.localization(project.id)? {
            return self.adapt_version_again(&project, &localization, consent);
        }
        if let Some(brief) = self.brief_standing(&project, None)? {
            if !brief.confirmed {
                return Err(ScriptError::BriefNotConfirmed);
            }
            // Confirmed once is not ready now: a claim the product
            // rejected since waits for an answer before a script reads it.
            if !brief.ready {
                return Err(ScriptError::BriefNotReady);
            }
        }
        let template = self.script_template(&project)?;
        let (rendered, brief) = self.render_script(&project, &template)?;
        let mut calls = vec![script_call(&rendered)];
        if brief.is_some() {
            calls.push(check_call_ahead());
        }
        if let Err(estimate) = self.check_video_budget(project.id, &calls, consent)? {
            return Err(ScriptError::OverBudget(estimate));
        }
        let payload = ScriptPayload {
            project: project.id.to_string(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
            brief_at: brief.as_ref().map(|brief| millis(brief.brief.changed_at())),
            check: brief.as_ref().map(CheckBrief::of),
        };
        let job = Job::new(
            self.profile.id,
            JobKind::ScriptGeneration,
            payload.to_json(),
        );
        Ok(self.jobs.enqueue(job)?)
    }

    fn own_script(&self, project: VideoProjectId) -> Result<Script, ScriptError> {
        let project = self.own_project(project)?;
        self.scripts
            .script(project.id)?
            .ok_or(ScriptError::NoScript)
    }

    /// Replaces the script's text with the user's.
    pub fn edit_script(&self, project: VideoProjectId, text: &str) -> Result<Script, ScriptError> {
        let mut script = self.own_script(project)?;
        let text = ScriptText::new(text).map_err(ScriptError::Invalid)?;
        if script.edit(text, SystemTime::now()) {
            self.scripts.save_script(&script)?;
        }
        Ok(script)
    }

    /// Makes the regenerated script the current one. Its claims check
    /// comes with it, and the check of the script it replaces goes.
    pub fn accept_script(&self, project: VideoProjectId) -> Result<Script, ScriptError> {
        let mut script = self.own_script(project)?;
        script.accept(SystemTime::now())?;
        self.scripts.save_script(&script)?;
        settle_checks(&*self.ads, project, &script)?;
        // A new script accepted without a check of its own does not keep
        // the check of the script it replaced.
        if self
            .ads
            .claims_check(project, CheckedScript::Current)?
            .is_some_and(|check| !check.reads(script.text()))
        {
            self.ads
                .drop_claims_check(project, CheckedScript::Current)?;
        }
        Ok(script)
    }

    /// Drops the regenerated script and keeps the current one, with its
    /// claims check.
    pub fn reject_script(&self, project: VideoProjectId) -> Result<Script, ScriptError> {
        let mut script = self.own_script(project)?;
        script.reject(SystemTime::now())?;
        self.scripts.save_script(&script)?;
        settle_checks(&*self.ads, project, &script)?;
        Ok(script)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use bardo_domain::{
        ChannelDraft, ContentLanguage, Country, JobState, ProviderFailure, ProviderFailureKind,
        TokenUsage,
    };
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::testing::{FakeDecisionEngine, FakeKeyChecker, FakeMarketData, FakeTextGenerator};
    use crate::{JobSettings, Providers, Repositories};

    const CLAUDE_KEY: &str = "sk-ant-api03-test-key-0001";
    const PATIENCE: Duration = Duration::from_secs(10);

    struct Harness {
        db: Arc<Database>,
        text: Arc<FakeTextGenerator>,
        secrets: Arc<MemorySecretStore>,
    }

    impl Harness {
        fn new() -> Self {
            Self::with_db(Arc::new(Database::open_in_memory().unwrap()))
        }

        fn with_db(db: Arc<Database>) -> Self {
            Self {
                db,
                text: Arc::default(),
                secrets: Arc::default(),
            }
        }

        fn start(&self) -> Bardo {
            let providers = Providers {
                key_checker: Arc::new(FakeKeyChecker::default()),
                market_data: Arc::new(FakeMarketData::default()),
                video_stats: Arc::new(crate::testing::FakeVideoStats::default()),
                text: Arc::clone(&self.text) as _,
                researcher: Arc::new(crate::product_sheets::testing::FakeResearcher::default()),
                decisions: vec![Arc::new(FakeDecisionEngine::default())],
                voices: Arc::new(crate::testing::FakeVoiceLibrary::default()),
                speech: Arc::new(crate::testing::FakeSpeech::default()),
                previews: Arc::new(crate::testing::NoPreviews),
                aligner: Arc::new(crate::narration_import::testing::FakeAligner::default()),
                transcriber: Arc::new(crate::captures::testing::FakeTranscriber::default()),
                voice_changer: Arc::new(crate::captures::testing::FakeVoiceChanger::default()),
                voice_isolator: Arc::new(crate::captures::testing::FakeVoiceIsolator::default()),
                vision: Arc::new(crate::take_analysis::testing::FakeVision::default()),
                images: Arc::new(crate::testing::FakeImages::default()),
                music: Arc::new(crate::testing::FakeMusic::default()),
                clips: vec![Arc::new(crate::testing::FakeClips::default())],
                takes: vec![Arc::new(crate::testing::FakeTakes::default())],
                audio: Arc::new(crate::narrations::testing::FakeAudioOutput::default()),
                media: Arc::new(crate::editor::testing::FakeMedia::default()),
                sign_ins: Vec::new(),
                plans: Vec::new(),
                consent: Arc::new(crate::connections::testing::NoConsent),
                uploaders: Vec::new(),
                analytics: Vec::new(),
                post_insights: Vec::new(),
                releases: Arc::new(crate::updates::testing::FakeReleases::default()),
            };
            Bardo::start_with(
                Repositories::shared(Arc::clone(&self.db), Arc::clone(&self.secrets) as _),
                providers,
                Some("en-US"),
                JobSettings {
                    retry: bardo_domain::RetryPolicy {
                        max_attempts: 2,
                        first_delay: Duration::from_millis(20),
                        max_delay: Duration::from_millis(20),
                    },
                    ..JobSettings::default()
                },
            )
            .unwrap()
        }

        fn start_with_key(&self) -> Bardo {
            let mut app = self.start();
            app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
            app
        }

        /// Queues Claude's next answers as plain scripts.
        fn answer(&self, scripts: &[&str]) {
            let mut answers = self.text.answers.lock().unwrap();
            answers.extend(scripts.iter().map(|script| (*script).to_owned()));
        }
    }

    /// A channel with a project started from an approved theme.
    fn project(app: &Bardo) -> VideoProject {
        let channel = app
            .create_channel(ChannelDraft {
                name: "Space Archives".into(),
                niche: "space history".into(),
                themes: vec!["lost missions".into(), "cold war".into()],
                language: ContentLanguage::Portuguese,
                country: Country::Brazil,
                ..ChannelDraft::default()
            })
            .unwrap();
        let mut theme = bardo_domain::Theme::suggested(
            app.profile().id,
            channel.id,
            bardo_domain::Niche::new("space history").unwrap(),
            bardo_domain::ThemeIdea::new(
                "The probe that never came home",
                "A tense retelling of the last signal.",
            )
            .unwrap(),
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

    fn wait_done(app: &Bardo, id: JobId) -> Job {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(job) = app
                .jobs()
                .into_iter()
                .find(|j| j.id() == id && !j.state().is_active())
            {
                return job;
            }
            assert!(Instant::now() < deadline, "job {id} never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn generate(app: &Bardo, project: &VideoProject) -> Job {
        let id = app.generate_script(project.id, BudgetConsent::Ask).unwrap();
        wait_done(app, id)
    }

    fn script(app: &Bardo, project: &VideoProject) -> Script {
        app.script(project.id).unwrap().script.unwrap()
    }

    #[test]
    fn generating_writes_the_script_with_its_provenance() {
        let h = Harness::new();
        h.answer(&["  Era uma vez uma sonda.\n"]);
        *h.text.usage.lock().unwrap() = TokenUsage {
            input_tokens: 812,
            output_tokens: 2_431,
        };
        let app = h.start_with_key();
        let project = project(&app);
        assert_eq!(app.script(project.id).unwrap().script, None);

        let job = generate(&app, &project);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(job.kind(), JobKind::ScriptGeneration);

        let view = app.script(project.id).unwrap();
        assert_eq!(view.job.as_ref().map(Job::id), Some(job.id()));
        let script = view.script.unwrap();
        assert_eq!(script.text().as_str(), "Era uma vez uma sonda.");
        assert_eq!(script.pending(), None);
        let generation = script.source().generation();
        let request = &h.text.requests()[0];
        assert_eq!(h.text.unstoppable(), 0, "the job hands over its stop");
        assert_eq!(
            request.effort,
            TextEffort::Careful,
            "the whole video is built on the script"
        );
        assert_eq!(generation.provider, Provider::Claude);
        assert_eq!(generation.model, "claude-fake");
        assert_eq!(generation.prompt, request.prompt);
        assert_eq!(generation.instructions, request.instructions);
        assert_eq!(generation.template.id, view.template.id);
        assert_eq!(generation.template.number, 1);
        assert_eq!(generation.usage.output_tokens, 2_431);
        assert_eq!(generation.job, Some(job.id()));
        assert_eq!(generation.project, project.id);
    }

    #[test]
    fn the_prompt_is_the_template_filled_with_the_projects_facts() {
        let h = Harness::new();
        h.answer(&["Script."]);
        let app = h.start_with_key();
        let project = project(&app);
        generate(&app, &project);

        let request = &h.text.requests()[0];
        assert_eq!(request.format, TextFormat::Prose);
        assert!(
            request
                .instructions
                .contains("Write in Portuguese; the audience's country is Brazil"),
            "{}",
            request.instructions
        );
        for fact in [
            "Channel: Space Archives",
            "Recurring channel themes: lost missions; cold war",
            "Aesthetic notes: not set",
            "Narrator: no persona chosen yet",
            "Video niche: space history",
            "Video title: The probe that never came home",
            "Angle: A tense retelling of the last signal.",
        ] {
            assert!(request.prompt.contains(fact), "{fact}: {}", request.prompt);
        }
        assert!(!request.prompt.contains("{{"), "{}", request.prompt);
    }

    #[test]
    fn the_narrator_is_the_channels_default_persona() {
        let h = Harness::new();
        h.answer(&["Script."]);
        let app = h.start_with_key();
        let project = project(&app);
        let persona = app
            .personas()
            .unwrap()
            .into_iter()
            .find(|p| p.details.name() == "Documentary Narrator (en-US)")
            .unwrap();
        let channel = app.channels.get(project.channel).unwrap().unwrap();
        app.update_channel(
            channel.id,
            ChannelDraft {
                default_persona: Some(persona.id),
                ..ChannelDraft::from(&channel.details)
            },
        )
        .unwrap();

        generate(&app, &project);
        let prompt = &h.text.requests()[0].prompt;
        assert!(
            prompt.contains(&format!("Narrator: {}", persona.details.describe())),
            "{prompt}"
        );
        assert!(prompt.contains("Voice: Wyatt."), "{prompt}");
        assert!(prompt.contains("Tone: Sober, measured"), "{prompt}");
        assert!(!prompt.contains("no persona chosen"), "{prompt}");
    }

    #[test]
    fn the_videos_own_persona_shapes_its_script() {
        let h = Harness::new();
        h.answer(&["Script."]);
        let app = h.start_with_key();
        let project = project(&app);
        let storyteller = app
            .personas()
            .unwrap()
            .into_iter()
            .find(|p| p.details.name() == "Dramatic Storyteller (en-US)")
            .unwrap();
        app.set_project_persona(project.id, Some(storyteller.id))
            .unwrap();

        generate(&app, &project);
        let prompt = &h.text.requests()[0].prompt;
        assert!(
            prompt.contains(&format!("Narrator: {}", storyteller.details.describe())),
            "{prompt}"
        );
    }

    #[test]
    fn every_script_variable_has_a_value() {
        let h = Harness::new();
        let app = h.start_with_key();
        let project = project(&app);
        let values = app.script_values(&project).unwrap();
        for variable in TemplateKind::Script.variables() {
            assert!(values.contains_key(variable), "{variable:?}");
        }
    }

    #[test]
    fn generation_uses_the_current_template_version() {
        let h = Harness::new();
        h.answer(&["Script."]);
        let app = h.start_with_key();
        let project = project(&app);
        let version = app
            .save_template(
                TemplateKind::Script,
                "Be brief.",
                "One line about {{theme_title}}.",
            )
            .unwrap();

        generate(&app, &project);
        let request = &h.text.requests()[0];
        assert_eq!(request.instructions, "Be brief.");
        assert_eq!(
            request.prompt,
            "One line about The probe that never came home."
        );
        let generation = script(&app, &project).source().generation().clone();
        assert_eq!(generation.template.id, version.id);
        assert_eq!(generation.template.number, 2);
    }

    #[test]
    fn edits_are_saved_and_keep_the_provenance() {
        let h = Harness::new();
        h.answer(&["Draft one."]);
        let app = h.start_with_key();
        let project = project(&app);
        generate(&app, &project);
        let source = script(&app, &project).source().clone();

        let edited = app
            .edit_script(project.id, "  Draft one, tightened by hand.\n")
            .unwrap();
        assert_eq!(edited.text().as_str(), "Draft one, tightened by hand.");
        let stored = script(&app, &project);
        assert_eq!(stored.text(), edited.text());
        assert!(stored.is_edited());
        assert_eq!(stored.source(), &source);

        let error = app.edit_script(project.id, " ").unwrap_err();
        assert_eq!(error.field_error(), Some(ScriptFieldError::TextRequired));
        assert_eq!(
            error.message(),
            Text::ScriptFieldError(ScriptFieldError::TextRequired)
        );
        assert_eq!(script(&app, &project).text(), edited.text());
    }

    #[test]
    fn regenerating_keeps_the_current_script_until_accepted() {
        let h = Harness::new();
        h.answer(&["Draft one.", "Draft two.", "Draft three."]);
        let app = h.start_with_key();
        let project = project(&app);
        generate(&app, &project);
        app.edit_script(project.id, "Draft one, edited.").unwrap();

        generate(&app, &project);
        let waiting = script(&app, &project);
        assert_eq!(waiting.text().as_str(), "Draft one, edited.");
        assert_eq!(
            waiting.pending().map(|p| p.text().as_str()),
            Some("Draft two.")
        );

        let kept = app.reject_script(project.id).unwrap();
        assert_eq!(kept.text().as_str(), "Draft one, edited.");
        assert_eq!(kept.pending(), None);

        generate(&app, &project);
        let accepted = app.accept_script(project.id).unwrap();
        assert_eq!(accepted.text().as_str(), "Draft three.");
        let stored = script(&app, &project);
        assert_eq!(stored.text(), accepted.text());
        assert_eq!(stored.source(), accepted.source());
        assert_eq!(stored.pending(), None);
        assert!(matches!(
            app.accept_script(project.id),
            Err(ScriptError::NothingToReview(_))
        ));
    }

    #[test]
    fn an_edit_while_regenerating_survives() {
        let h = Harness::new();
        h.answer(&["Draft one.", "Draft two."]);
        let app = h.start_with_key();
        let project = project(&app);
        generate(&app, &project);

        *h.text.delay.lock().unwrap() = Duration::from_millis(150);
        let id = app.generate_script(project.id, BudgetConsent::Ask).unwrap();
        let deadline = Instant::now() + PATIENCE;
        while h.text.requests().len() < 2 {
            assert!(Instant::now() < deadline, "Claude was never asked");
            std::thread::sleep(Duration::from_millis(2));
        }
        app.edit_script(project.id, "Edited during the call.")
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);

        let script = script(&app, &project);
        assert_eq!(script.text().as_str(), "Edited during the call.");
        assert_eq!(
            script.pending().map(|p| p.text().as_str()),
            Some("Draft two.")
        );
    }

    #[test]
    fn an_empty_answer_fails_the_job_and_changes_nothing() {
        let h = Harness::new();
        h.answer(&["   "]);
        let app = h.start_with_key();
        let project = project(&app);

        let job = generate(&app, &project);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::UnexpectedAnswer
        );
        assert_eq!(app.script(project.id).unwrap().script, None);
    }

    #[test]
    fn a_declined_request_fails_without_retrying() {
        let h = Harness::new();
        *h.text.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Declined,
            "refused",
        ));
        let app = h.start_with_key();
        let project = project(&app);

        let job = generate(&app, &project);
        assert_eq!(job.state(), JobState::Failed);
        let failure = job.failure().unwrap();
        assert_eq!(failure.kind, JobFailureKind::Declined);
        assert_eq!(failure.detail, "Claude: refused");
        assert_eq!(h.text.requests().len(), 1);
    }

    #[test]
    fn generating_needs_a_claude_key_and_one_job_at_a_time() {
        let h = Harness::new();
        let mut app = h.start();
        let project = project(&app);
        let error = app
            .generate_script(project.id, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, ScriptError::MissingKey(Provider::Claude)));
        assert_eq!(error.message(), Text::ScriptMissingKey);
        assert!(app.jobs().is_empty());

        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        *h.text.delay.lock().unwrap() = Duration::from_millis(100);
        let id = app.generate_script(project.id, BudgetConsent::Ask).unwrap();
        let again = app
            .generate_script(project.id, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(again, ScriptError::Busy));
        assert_eq!(again.message(), Text::ScriptBusy);
        wait_done(&app, id);
        assert!(app.generate_script(project.id, BudgetConsent::Ask).is_ok());
    }

    #[test]
    fn edits_and_reviews_need_a_script() {
        let h = Harness::new();
        let app = h.start_with_key();
        let project = project(&app);
        assert!(matches!(
            app.edit_script(project.id, "Text."),
            Err(ScriptError::NoScript)
        ));
        assert!(matches!(
            app.accept_script(project.id),
            Err(ScriptError::NoScript)
        ));
        assert_eq!(ScriptError::NoScript.message(), Text::ScriptMissing);
    }

    #[test]
    fn other_profiles_cannot_reach_projects() {
        let h = Harness::new();
        let app = h.start_with_key();
        let project = project(&app);
        assert!(matches!(
            app.script(VideoProjectId::new()),
            Err(ScriptError::ProjectNotFound)
        ));
        assert!(matches!(
            app.video_projects(ChannelId::new()),
            Err(ScriptError::ChannelNotFound)
        ));
        let ids: Vec<_> = app
            .video_projects(project.channel)
            .unwrap()
            .iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, [project.id]);
    }

    #[test]
    fn a_resumed_job_does_not_generate_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bardo.db");
        let first = Harness::with_db(Arc::new(Database::open(&path).unwrap()));
        first.answer(&["Draft one."]);
        let app = first.start_with_key();
        let project = project(&app);
        let id = app.generate_script(project.id, BudgetConsent::Ask).unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);
        let saved = script(&app, &project);

        // Replays the job as if the app had stopped right after saving.
        let payload = app
            .jobs()
            .into_iter()
            .find(|job| job.id() == id)
            .unwrap()
            .payload()
            .to_owned();
        let handler = ScriptHandler {
            owner: app.profile().id,
            scripts: Arc::clone(&first.db) as _,
            ads: Arc::clone(&first.db) as _,
            exports: Arc::clone(&first.db) as _,
            text: Arc::clone(&first.text) as _,
            secrets: Arc::clone(&first.secrets) as _,
            costs: crate::costs::CostBook {
                owner: app.profile().id,
                costs: Arc::clone(&first.db) as _,
                themes: Arc::clone(&first.db) as _,
                in_flight: std::sync::Arc::default(),
            },
        };
        handler
            .generate(&payload, id, &Stop::never(), &mut false)
            .unwrap();
        assert_eq!(first.text.requests().len(), 1, "Claude is asked once");
        assert_eq!(script(&app, &project), saved);
    }

    /// Prices the fake Claude model at $4 / $20 per million tokens and
    /// makes each answer cost $1 + $1.
    fn price_claude(h: &Harness, app: &Bardo) {
        use bardo_domain::Meter;
        app.save_rate(Provider::Claude, "claude-fake", Meter::InputTokens, "4")
            .unwrap();
        app.save_rate(Provider::Claude, "claude-fake", Meter::OutputTokens, "20")
            .unwrap();
        *h.text.usage.lock().unwrap() = TokenUsage {
            input_tokens: 250_000,
            output_tokens: 50_000,
        };
    }

    fn dollars(text: &str) -> Money {
        Money::parse(text).unwrap()
    }

    #[test]
    fn generating_a_script_records_its_cost_per_video_channel_and_month() {
        let h = Harness::new();
        h.answer(&["Era uma vez uma sonda."]);
        let app = h.start_with_key();
        price_claude(&h, &app);
        let project = project(&app);

        assert_eq!(generate(&app, &project).state(), JobState::Done);

        let month = app.current_month();
        let costs = app.costs(month).unwrap();
        assert_eq!(costs.total, dollars("2"));
        let claude = costs
            .providers
            .iter()
            .find(|p| p.provider == Provider::Claude)
            .unwrap();
        assert_eq!(claude.spent, dollars("2"));
        assert_eq!(claude.budget, None);
        assert_eq!(costs.channels.len(), 1);
        assert_eq!(costs.channels[0].name.as_deref(), Some("Space Archives"));
        assert_eq!(costs.channels[0].amount, dollars("2"));
        assert_eq!(costs.videos.len(), 1);
        assert_eq!(
            costs.videos[0].name.as_deref(),
            Some(project.title.as_str())
        );
        assert_eq!(costs.videos[0].channel.as_deref(), Some("Space Archives"));
        assert_eq!(costs.videos[0].amount, dollars("2"));
        assert!(costs.unpriced.is_empty());
        assert_eq!(app.script(project.id).unwrap().spent, dollars("2"));

        let before = app.costs(month.previous()).unwrap();
        assert_eq!(before.total, Money::ZERO);
        assert!(before.videos.is_empty());
    }

    #[test]
    fn a_model_without_a_rate_counts_as_nothing_and_is_named() {
        let h = Harness::new();
        h.answer(&["Era uma vez uma sonda."]);
        *h.text.usage.lock().unwrap() = TokenUsage {
            input_tokens: 800,
            output_tokens: 2_400,
        };
        let app = h.start_with_key();
        let project = project(&app);

        assert_eq!(generate(&app, &project).state(), JobState::Done);

        let costs = app.costs(app.current_month()).unwrap();
        assert_eq!(costs.total, Money::ZERO);
        assert_eq!(
            costs.unpriced,
            vec![(Provider::Claude, "claude-fake".to_owned())]
        );

        // A rate added later prices the calls that had none.
        app.save_rate(
            Provider::Claude,
            "claude",
            bardo_domain::Meter::OutputTokens,
            "10",
        )
        .unwrap();
        app.save_rate(
            Provider::Claude,
            "claude",
            bardo_domain::Meter::InputTokens,
            "0",
        )
        .unwrap();
        let costs = app.costs(app.current_month()).unwrap();
        assert_eq!(costs.total, dollars("0.024"));
        assert!(costs.unpriced.is_empty());
        assert_eq!(app.script(project.id).unwrap().spent, dollars("0.024"));
    }

    #[test]
    fn the_estimate_shows_before_generating_and_learns_from_past_scripts() {
        let h = Harness::new();
        h.answer(&["Era uma vez uma sonda."]);
        let app = h.start_with_key();
        let project = project(&app);

        let first = app.script(project.id).unwrap().estimate;
        assert!(!first.is_partial());
        assert_eq!(first.providers.len(), 1);
        assert!(first.total() > Money::ZERO);

        price_claude(&h, &app);
        generate(&app, &project);
        app.accept_script(project.id).ok();

        // 50,000 output tokens at Opus's $20 per million is $1.
        let learned = app.script(project.id).unwrap().estimate;
        assert!(learned.total() >= dollars("1"), "{learned:?}");
        assert!(learned.total() > first.total());
    }

    #[test]
    fn a_budget_warns_from_80_percent_and_asks_from_100() {
        let h = Harness::new();
        h.answer(&["Draft one.", "Draft two."]);
        let app = h.start_with_key();
        price_claude(&h, &app);
        let project = project(&app);
        generate(&app, &project);

        // $2 spent, and the next script estimated at about $1.
        app.set_budget(Provider::Claude, "10").unwrap();
        let estimate = app.script(project.id).unwrap().estimate;
        assert_eq!(estimate.level(), bardo_domain::BudgetLevel::Under);

        app.set_budget(Provider::Claude, "3.5").unwrap();
        let estimate = app.script(project.id).unwrap().estimate;
        assert_eq!(estimate.level(), bardo_domain::BudgetLevel::Warning);
        assert_eq!(estimate.near_budget().count(), 1);

        app.set_budget(Provider::Claude, "2").unwrap();
        let claude = app.costs(app.current_month()).unwrap().providers[0].clone();
        assert_eq!(claude.level, Some(bardo_domain::BudgetLevel::Reached));
        assert_eq!(claude.percent, Some(100));
        let asked = h.text.requests().len();
        match app.generate_script(project.id, BudgetConsent::Ask) {
            Err(ScriptError::OverBudget(estimate)) => {
                let over: Vec<_> = estimate.over_budget().collect();
                assert_eq!(over.len(), 1);
                assert_eq!(over[0].provider, Provider::Claude);
                assert_eq!(over[0].spent, dollars("2"));
                assert_eq!(over[0].budget, Some(dollars("2")));
            }
            other => panic!("expected the budget question, got {other:?}"),
        }
        assert_eq!(h.text.requests().len(), asked, "nothing ran");

        let id = app
            .generate_script(project.id, BudgetConsent::Confirmed)
            .unwrap();
        assert_eq!(wait_done(&app, id).state(), JobState::Done);
        assert_eq!(app.costs(app.current_month()).unwrap().total, dollars("4"));

        app.remove_budget(Provider::Claude).unwrap();
        let estimate = app.script(project.id).unwrap().estimate;
        assert_eq!(estimate.level(), bardo_domain::BudgetLevel::Under);
    }

    #[test]
    fn budgets_and_rates_check_what_is_typed() {
        use crate::CostError;
        use bardo_domain::{Meter, MoneyError, RateFieldError};

        let app = Harness::new().start();
        assert!(matches!(
            app.set_budget(Provider::Claude, "ten"),
            Err(CostError::InvalidBudget(MoneyError::Invalid))
        ));
        assert!(matches!(
            app.set_budget(Provider::YouTubeData, "1"),
            Err(CostError::NotPaid(Provider::YouTubeData))
        ));
        app.set_budget(Provider::Gemini, "5").unwrap();
        app.set_budget(Provider::Gemini, "7,50").unwrap();
        let costs = app.costs(app.current_month()).unwrap();
        let gemini = costs
            .providers
            .iter()
            .find(|p| p.provider == Provider::Gemini)
            .unwrap();
        assert_eq!(gemini.budget, Some(dollars("7.50")));
        assert!(costs.providers.iter().all(|p| p.provider.is_paid()));

        assert!(matches!(
            app.save_rate(Provider::Claude, "my model", Meter::InputTokens, "1"),
            Err(CostError::InvalidRate(RateFieldError::ModelHasSpaces))
        ));
        assert!(matches!(
            app.save_rate(Provider::YouTubeData, "", Meter::InputTokens, "1"),
            Err(CostError::InvalidRate(RateFieldError::NotPaid))
        ));

        let opus = bardo_ai::claude::MODEL;
        let row = |app: &Bardo| {
            app.costs(app.current_month())
                .unwrap()
                .rates
                .into_iter()
                .find(|row| row.rate.model == opus && row.rate.meter == Meter::InputTokens)
                .unwrap()
        };
        assert_eq!(row(&app).rate.price, dollars("4"));
        assert!(!row(&app).changed);
        app.save_rate(Provider::Claude, opus, Meter::InputTokens, "5")
            .unwrap();
        let changed = row(&app);
        assert_eq!(
            (changed.rate.price, changed.default),
            (dollars("5"), Some(dollars("4")))
        );
        assert!(changed.changed);
        app.reset_rate(Provider::Claude, opus, Meter::InputTokens)
            .unwrap();
        assert_eq!(row(&app).rate.price, dollars("4"));
        assert!(!row(&app).changed);
    }

    #[test]
    fn a_cancelled_script_job_that_still_stops_keeps_the_script_busy_and_its_answer_is_saved() {
        use crate::testing::arriving;

        let h = Harness::new();
        h.answer(&["Era uma vez uma sonda."]);
        let app = h.start_with_key();
        let project = project(&app);
        let id = arriving::cancelled(&app, &h.text, || {
            app.generate_script(project.id, BudgetConsent::Ask).unwrap()
        });

        // No second request beside the one still out, from the stage or
        // from Jobs.
        assert!(matches!(
            app.generate_script(project.id, BudgetConsent::Ask),
            Err(ScriptError::Busy)
        ));
        arriving::retry_refused(&app, id);

        // The answer arrives whole: it is paid for, replaces nothing, and
        // is the project's script.
        arriving::lands(&app, &h.text, id);
        assert_eq!(
            script(&app, &project).text().as_str(),
            "Era uma vez uma sonda."
        );

        // Run again, the job finds its script: Claude is not asked twice.
        assert_eq!(arriving::retried(&app, id).state(), JobState::Done);
        assert_eq!(h.text.requests().len(), 1);
    }

    #[test]
    fn an_older_script_job_is_not_run_again_beside_a_newer_one_at_work() {
        use crate::testing::arriving;

        let h = Harness::new();
        let app = h.start_with_key();
        let project = project(&app);
        *h.text.failure.lock().unwrap() = Some(ProviderFailure::new(
            ProviderFailureKind::Rejected,
            "not this time",
        ));
        let older = arriving::ended(
            &app,
            app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
        );
        assert_eq!(older.state(), JobState::Failed);
        *h.text.failure.lock().unwrap() = None;

        // A newer job is out, with its request.
        h.answer(&["Era uma vez uma sonda."]);
        h.text.hold();
        let asked = h.text.requests().len();
        let newer = app.generate_script(project.id, BudgetConsent::Ask).unwrap();
        arriving::wait_until("the request left", || h.text.requests().len() > asked);

        arriving::retry_refused(&app, older.id());

        // Once it ended, the older one runs again as before.
        h.text.let_go();
        assert_eq!(arriving::ended(&app, newer).state(), JobState::Done);
        h.answer(&["Outra sonda."]);
        assert_eq!(arriving::retried(&app, older.id()).state(), JobState::Done);
    }
}
