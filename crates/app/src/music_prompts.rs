//! Music prompt use cases (PRD story 40): Bardo does not make music. Claude
//! writes a prompt for the video's music from the music prompt template,
//! filled with the channel, theme and video length; the user edits it,
//! copies it into their music tool and imports the file it makes
//! (`crate::MediaImport`). Generating again replaces the prompt, edits
//! included.
//!
//! Generating calls Claude, so it runs as a job, rendered when it starts
//! like the script job.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bardo_domain::{
    ApiKey, CostPurpose, Generation, GenerationId, Job, JobFailure, JobFailureKind, JobId, JobKind,
    MusicPrompt, MusicPromptFieldError, MusicPromptRepository, ProfileId, Provider, RenderedPrompt,
    RepositoryError, SecretStore, TemplateKind, TemplateUsed, TemplateValues, TemplateVariable,
    TemplateVersion, TemplateVersionId, TextFormat, TextGenerator, TextRequest, VideoProject,
    VideoProjectId,
};
use serde::{Deserialize, Serialize};

use crate::costs::{BudgetConsent, CostBook, PaidCall, PlannedCall, SpendEstimate};
use crate::jobs::{JobContext, JobHandler};
use crate::{Bardo, KeyState, ScriptError, TemplateError, Text};

/// The video length before there is a narration to measure.
const LENGTH_NOT_KNOWN: &str = "not known yet";

#[derive(Debug, thiserror::Error)]
pub enum MusicPromptError {
    /// The typed text breaks a rule; the card shows it.
    #[error("invalid music prompt: {0:?}")]
    Invalid(MusicPromptFieldError),
    #[error("video project not found")]
    ProjectNotFound,
    /// The project has no music prompt yet: generate one first.
    #[error("the project has no music prompt yet")]
    NoPrompt,
    /// Generation calls this provider, and no key is saved for it.
    #[error("no {0} key saved")]
    MissingKey(Provider),
    /// The project's music prompt is being generated.
    #[error("a music prompt is already being generated for this project")]
    Busy,
    /// The generation would reach a provider's budget; the screen asks
    /// before starting it with `BudgetConsent::Confirmed`.
    #[error("over budget")]
    OverBudget(SpendEstimate),
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl MusicPromptError {
    /// What the editor says.
    pub fn message(&self) -> Text {
        match self {
            MusicPromptError::Invalid(error) => Text::MusicPromptFieldError(*error),
            MusicPromptError::ProjectNotFound => Text::ProjectNotFound,
            MusicPromptError::NoPrompt => Text::MusicPromptMissing,
            MusicPromptError::MissingKey(_) => Text::MusicPromptMissingKey,
            MusicPromptError::Busy => Text::MusicPromptBusy,
            MusicPromptError::OverBudget(_) => Text::BudgetReachedTitle,
            MusicPromptError::Template(error) => error.message(),
            MusicPromptError::Repository(_) => Text::MusicPromptNotSaved,
        }
    }
}

impl From<ScriptError> for MusicPromptError {
    /// The project lookups are shared with scripts; only these come back.
    fn from(error: ScriptError) -> Self {
        match error {
            ScriptError::Repository(error) => MusicPromptError::Repository(error),
            ScriptError::Template(error) => MusicPromptError::Template(error),
            _ => MusicPromptError::ProjectNotFound,
        }
    }
}

/// A video project's music prompt card.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicPromptView {
    /// `None` until the first prompt is generated.
    pub prompt: Option<MusicPrompt>,
    /// The project's latest music prompt job.
    pub job: Option<Job>,
    /// The template version the next generation uses.
    pub template: TemplateVersion,
    /// What generating the prompt would cost.
    pub estimate: SpendEstimate,
}

/// The music prompt job's payload: the rendered prompt and where it came
/// from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MusicPromptPayload {
    project: String,
    template: String,
    template_number: u32,
    instructions: String,
    prompt: String,
}

impl MusicPromptPayload {
    fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a music prompt payload serializes")
    }

    fn parse(payload: &str) -> Result<Self, JobFailure> {
        serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid music prompt payload: {e}")))
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

/// Runs music prompt jobs.
pub(crate) struct MusicPromptHandler {
    pub(crate) owner: ProfileId,
    pub(crate) prompts: Arc<dyn MusicPromptRepository>,
    pub(crate) text: Arc<dyn TextGenerator>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) costs: CostBook,
}

impl JobHandler for MusicPromptHandler {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        self.generate(payload, cx.id())
    }
}

impl MusicPromptHandler {
    fn key(&self) -> Result<ApiKey, JobFailure> {
        self.secrets
            .get(self.owner, Provider::Claude)
            .map_err(|e| JobFailure::unexpected(format!("could not read the key: {e}")))?
            .ok_or_else(|| JobFailure::new(JobFailureKind::MissingKey, "no Claude key is saved"))
    }

    /// Generates the prompt `job` asked for and saves it in place of the
    /// project's prompt.
    fn generate(&self, payload: &str, job: JobId) -> Result<(), JobFailure> {
        let payload = MusicPromptPayload::parse(payload)?;
        let project = payload.project()?;
        // An earlier attempt of this job may have saved its prompt and
        // stopped before the queue recorded it as done.
        let saved = self.prompts.music_prompt(project).map_err(unexpected)?;
        if saved.is_some_and(|prompt| prompt.generation().job == Some(job)) {
            return Ok(());
        }

        let key = self.key()?;
        let request = TextRequest {
            instructions: payload.instructions.clone(),
            prompt: payload.prompt.clone(),
            format: TextFormat::Prose,
        };
        let generated = self.text.generate(&key, &request).map_err(|failure| {
            JobFailure::new(failure.kind.into(), format!("Claude: {}", failure.detail))
        })?;
        self.costs.record_for_project(
            PaidCall {
                provider: Provider::Claude,
                model: &generated.model,
                purpose: CostPurpose::MusicPrompt,
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
        let prompt = MusicPrompt::generated(generation, now).map_err(|error| {
            JobFailure::new(
                JobFailureKind::UnexpectedAnswer,
                format!("Claude: the music prompt is not usable ({error:?})"),
            )
        })?;
        self.prompts.save_music_prompt(&prompt).map_err(unexpected)
    }
}

/// The Claude call that writes a music prompt from `rendered`.
fn music_prompt_call(rendered: &RenderedPrompt) -> PlannedCall {
    PlannedCall::new(Provider::Claude, CostPurpose::MusicPrompt, 1)
        .with_prompt(&rendered.instructions, &rendered.prompt)
}

/// A length as a music tool reads it: "8 min 05 s", or "45 s".
fn length_text(length: Duration) -> String {
    let seconds = length.as_secs_f64().round() as u64;
    match (seconds / 60, seconds % 60) {
        (0, seconds) => format!("{seconds} s"),
        (minutes, seconds) => format!("{minutes} min {seconds:02} s"),
    }
}

impl Bardo {
    fn own_music_project(&self, id: VideoProjectId) -> Result<VideoProject, MusicPromptError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(MusicPromptError::ProjectNotFound)
    }

    fn latest_music_prompt_job(&self, project: VideoProjectId) -> Option<Job> {
        let project = project.to_string();
        self.jobs().into_iter().rev().find(|job| {
            job.kind() == JobKind::MusicPrompt
                && MusicPromptPayload::parse(job.payload()).is_ok_and(|p| p.project == project)
        })
    }

    /// The facts a music prompt template is filled with: the script's,
    /// plus the video's length once its narration is there.
    pub(crate) fn music_prompt_values(
        &self,
        project: &VideoProject,
    ) -> Result<TemplateValues, MusicPromptError> {
        let mut values = self.script_values(project)?;
        let length = self
            .narrations
            .narration(project.id)?
            .map(|narration| length_text(narration.duration));
        values.insert(
            TemplateVariable::VideoLength,
            length.unwrap_or_else(|| LENGTH_NOT_KNOWN.to_owned()),
        );
        Ok(values)
    }

    /// The music prompt template filled with the project's facts.
    fn render_music_prompt(
        &self,
        project: &VideoProject,
        template: &TemplateVersion,
    ) -> Result<RenderedPrompt, MusicPromptError> {
        template
            .body
            .render(&self.music_prompt_values(project)?)
            .map_err(|missing| {
                // Every music prompt variable has a value above.
                MusicPromptError::Repository(RepositoryError(Box::new(missing)))
            })
    }

    /// The project's music prompt card.
    pub fn music_prompt(
        &self,
        project: VideoProjectId,
    ) -> Result<MusicPromptView, MusicPromptError> {
        let project = self.own_music_project(project)?;
        let template = self.current_template(TemplateKind::MusicPrompt)?;
        let rendered = self.render_music_prompt(&project, &template)?;
        Ok(MusicPromptView {
            prompt: self.music_prompts.music_prompt(project.id)?,
            job: self.latest_music_prompt_job(project.id),
            estimate: self.estimate(&[music_prompt_call(&rendered)])?,
            template,
        })
    }

    /// Starts a job in which Claude writes the project's music prompt from
    /// the current music prompt template. The new prompt replaces the
    /// current one. Past Claude's budget it needs `consent`.
    pub fn generate_music_prompt(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, MusicPromptError> {
        let project = self.own_music_project(project)?;
        if self
            .latest_music_prompt_job(project.id)
            .is_some_and(|job| job.state().is_active())
        {
            return Err(MusicPromptError::Busy);
        }
        if self.provider_key(Provider::Claude).state == KeyState::NotSet {
            return Err(MusicPromptError::MissingKey(Provider::Claude));
        }
        let template = self.current_template(TemplateKind::MusicPrompt)?;
        let rendered = self.render_music_prompt(&project, &template)?;
        if let Err(estimate) = self.check_budget(&[music_prompt_call(&rendered)], consent)? {
            return Err(MusicPromptError::OverBudget(estimate));
        }
        let payload = MusicPromptPayload {
            project: project.id.to_string(),
            template: template.id.to_string(),
            template_number: template.number,
            instructions: rendered.instructions,
            prompt: rendered.prompt,
        };
        let job = Job::new(self.profile.id, JobKind::MusicPrompt, payload.to_json());
        Ok(self.jobs.enqueue(job)?)
    }

    /// Replaces the music prompt's text with the user's.
    pub fn edit_music_prompt(
        &self,
        project: VideoProjectId,
        text: &str,
    ) -> Result<MusicPrompt, MusicPromptError> {
        let project = self.own_music_project(project)?;
        let mut prompt = self
            .music_prompts
            .music_prompt(project.id)?
            .ok_or(MusicPromptError::NoPrompt)?;
        if prompt
            .edit(text, SystemTime::now())
            .map_err(MusicPromptError::Invalid)?
        {
            self.music_prompts.save_music_prompt(&prompt)?;
        }
        Ok(prompt)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use bardo_domain::{
        ChannelDraft, ContentLanguage, Country, JobState, Narration, NarrationId,
        NarrationRepository, NarrationSource, ScriptText, TokenUsage, WordTiming, WordTimings,
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
            Self {
                db: Arc::new(Database::open_in_memory().unwrap()),
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
                decisions: Arc::new(FakeDecisionEngine::default()),
                voices: Arc::new(crate::testing::FakeVoiceLibrary::default()),
                speech: Arc::new(crate::testing::FakeSpeech::default()),
                previews: Arc::new(crate::testing::NoPreviews),
                aligner: Arc::new(crate::narration_import::testing::FakeAligner::default()),
                images: Arc::new(crate::testing::FakeImages::default()),
                clips: vec![Arc::new(crate::testing::FakeClips::default())],
                audio: Arc::new(crate::narrations::testing::FakeAudioOutput::default()),
                media: Arc::new(crate::editor::testing::FakeMedia::default()),
                sign_ins: Vec::new(),
                consent: Arc::new(crate::connections::testing::NoConsent),
                uploaders: Vec::new(),
                analytics: Vec::new(),
                post_insights: Vec::new(),
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

        fn answer(&self, prompts: &[&str]) {
            let mut answers = self.text.answers.lock().unwrap();
            answers.extend(prompts.iter().map(|prompt| (*prompt).to_owned()));
        }
    }

    /// A channel with a project started from an approved theme.
    fn project(app: &Bardo) -> VideoProject {
        let channel = app
            .create_channel(ChannelDraft {
                name: "Space Archives".into(),
                niche: "space history".into(),
                aesthetic_notes: "cold blues, archival grain".into(),
                language: ContentLanguage::English,
                country: Country::UnitedStates,
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
        let id = app
            .generate_music_prompt(project.id, BudgetConsent::Ask)
            .unwrap();
        wait_done(app, id)
    }

    #[test]
    fn generating_writes_the_prompt_with_its_provenance() {
        let h = Harness::new();
        h.answer(&["  Slow ambient synth pads, 70 BPM, no vocals.\n"]);
        *h.text.usage.lock().unwrap() = TokenUsage {
            input_tokens: 300,
            output_tokens: 90,
        };
        let app = h.start_with_key();
        let project = project(&app);
        assert_eq!(app.music_prompt(project.id).unwrap().prompt, None);

        let job = generate(&app, &project);
        assert_eq!(job.state(), JobState::Done, "{:?}", job.failure());
        assert_eq!(job.kind(), JobKind::MusicPrompt);

        let view = app.music_prompt(project.id).unwrap();
        assert_eq!(view.job.as_ref().map(Job::id), Some(job.id()));
        let prompt = view.prompt.unwrap();
        assert_eq!(prompt.text(), "Slow ambient synth pads, 70 BPM, no vocals.");
        assert!(!prompt.is_edited());
        let generation = prompt.generation();
        let request = &h.text.requests()[0];
        assert_eq!(request.format, TextFormat::Prose);
        assert_eq!(generation.prompt, request.prompt);
        assert_eq!(generation.instructions, request.instructions);
        assert_eq!(generation.template.id, view.template.id);
        assert_eq!(generation.job, Some(job.id()));
        assert_eq!(generation.usage.output_tokens, 90);
    }

    #[test]
    fn the_prompt_is_the_template_filled_with_the_projects_facts() {
        let h = Harness::new();
        h.answer(&["Music."]);
        let app = h.start_with_key();
        let project = project(&app);
        generate(&app, &project);

        let request = &h.text.requests()[0];
        assert!(request.instructions.contains("never name artists"));
        for fact in [
            "Channel: Space Archives",
            "Aesthetic notes: cold blues, archival grain",
            "Audience country: United States",
            "Video title: The probe that never came home",
            "Angle: A tense retelling of the last signal.",
            "Video length: not known yet",
        ] {
            assert!(request.prompt.contains(fact), "{fact}: {}", request.prompt);
        }
        assert!(!request.prompt.contains("{{"), "{}", request.prompt);
    }

    #[test]
    fn the_video_length_comes_from_the_narration() {
        let h = Harness::new();
        h.answer(&["Music."]);
        let app = h.start_with_key();
        let project = project(&app);
        h.db.save_narration(&Narration {
            id: NarrationId::new(),
            project: project.id,
            owner: project.owner,
            text: ScriptText::new("Era.").unwrap(),
            source: NarrationSource::Imported {
                file_name: "me reading.wav".into(),
                aligner: Provider::ElevenLabs,
                model: "aligner".into(),
            },
            audio_file: "narration-1.wav".into(),
            duration: Duration::from_millis(485_400),
            words: WordTimings::restore(
                "Era.",
                vec![WordTiming {
                    text: 0..4,
                    start: Duration::ZERO,
                    end: Duration::from_millis(400),
                }],
            )
            .unwrap(),
            generated_at: SystemTime::now(),
            job: None,
        })
        .unwrap();
        generate(&app, &project);
        let prompt = &h.text.requests()[0].prompt;
        assert!(prompt.contains("Video length: 8 min 05 s"), "{prompt}");
    }

    #[test]
    fn every_music_prompt_variable_has_a_value() {
        let app = Harness::new().start_with_key();
        let project = project(&app);
        let values = app.music_prompt_values(&project).unwrap();
        for variable in TemplateKind::MusicPrompt.variables() {
            assert!(values.contains_key(variable), "{variable:?}");
        }
        assert_eq!(length_text(Duration::from_millis(44_600)), "45 s");
        assert_eq!(length_text(Duration::from_secs(600)), "10 min 00 s");
    }

    #[test]
    fn edits_are_saved_and_generating_again_replaces_them() {
        let h = Harness::new();
        h.answer(&["Dark ambient drone.", "Tense strings, rising."]);
        let app = h.start_with_key();
        let project = project(&app);
        assert!(matches!(
            app.edit_music_prompt(project.id, "Mine."),
            Err(MusicPromptError::NoPrompt)
        ));
        generate(&app, &project);

        let edited = app
            .edit_music_prompt(project.id, " Dark ambient drone, 60 BPM. ")
            .unwrap();
        assert_eq!(edited.text(), "Dark ambient drone, 60 BPM.");
        let stored = app.music_prompt(project.id).unwrap().prompt.unwrap();
        assert_eq!(stored.text(), edited.text());
        assert_eq!(stored.generation(), edited.generation());
        assert!(stored.is_edited());

        let error = app.edit_music_prompt(project.id, "  ").unwrap_err();
        assert_eq!(
            error.message(),
            Text::MusicPromptFieldError(MusicPromptFieldError::TextRequired)
        );

        generate(&app, &project);
        let again = app.music_prompt(project.id).unwrap().prompt.unwrap();
        assert_eq!(again.text(), "Tense strings, rising.");
        assert!(!again.is_edited());
    }

    #[test]
    fn an_empty_answer_fails_the_job_and_keeps_the_prompt() {
        let h = Harness::new();
        h.answer(&["Dark ambient drone.", "  "]);
        let app = h.start_with_key();
        let project = project(&app);
        generate(&app, &project);

        let job = generate(&app, &project);
        assert_eq!(job.state(), JobState::Failed);
        assert_eq!(
            job.failure().unwrap().kind,
            JobFailureKind::UnexpectedAnswer
        );
        let kept = app.music_prompt(project.id).unwrap().prompt.unwrap();
        assert_eq!(kept.text(), "Dark ambient drone.");
    }

    #[test]
    fn generating_needs_a_claude_key_and_one_job_at_a_time() {
        let h = Harness::new();
        let mut app = h.start();
        let project = project(&app);
        let error = app
            .generate_music_prompt(project.id, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(
            error,
            MusicPromptError::MissingKey(Provider::Claude)
        ));
        assert_eq!(error.message(), Text::MusicPromptMissingKey);

        app.save_provider_key(Provider::Claude, CLAUDE_KEY).unwrap();
        *h.text.delay.lock().unwrap() = Duration::from_millis(100);
        let id = app
            .generate_music_prompt(project.id, BudgetConsent::Ask)
            .unwrap();
        let again = app
            .generate_music_prompt(project.id, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(again, MusicPromptError::Busy));
        wait_done(&app, id);
        // Once that job is done, generating again works.
        assert!(
            app.generate_music_prompt(project.id, BudgetConsent::Ask)
                .is_ok()
        );
    }

    #[test]
    fn other_profiles_cannot_reach_projects() {
        let app = Harness::new().start_with_key();
        assert!(matches!(
            app.music_prompt(VideoProjectId::new()),
            Err(MusicPromptError::ProjectNotFound)
        ));
        assert!(matches!(
            app.generate_music_prompt(VideoProjectId::new(), BudgetConsent::Ask),
            Err(MusicPromptError::ProjectNotFound)
        ));
    }

    #[test]
    fn the_cost_is_recorded_for_the_video() {
        use bardo_domain::Meter;
        let h = Harness::new();
        h.answer(&["Music."]);
        let app = h.start_with_key();
        app.save_rate(Provider::Claude, "claude-fake", Meter::InputTokens, "4")
            .unwrap();
        app.save_rate(Provider::Claude, "claude-fake", Meter::OutputTokens, "20")
            .unwrap();
        *h.text.usage.lock().unwrap() = TokenUsage {
            input_tokens: 250_000,
            output_tokens: 50_000,
        };
        let project = project(&app);
        assert!(app.music_prompt(project.id).unwrap().estimate.total() > bardo_domain::Money::ZERO);
        generate(&app, &project);
        let costs = app.costs(app.current_month()).unwrap();
        assert_eq!(costs.videos.len(), 1);
        assert_eq!(
            costs.videos[0].amount,
            bardo_domain::Money::parse("2").unwrap()
        );
    }

    #[test]
    fn a_resumed_job_does_not_generate_twice() {
        let h = Harness::new();
        h.answer(&["Music."]);
        let app = h.start_with_key();
        let project = project(&app);
        let job = generate(&app, &project);
        let saved = app.music_prompt(project.id).unwrap().prompt;

        let handler = MusicPromptHandler {
            owner: app.profile().id,
            prompts: Arc::clone(&h.db) as _,
            text: Arc::clone(&h.text) as _,
            secrets: Arc::clone(&h.secrets) as _,
            costs: CostBook {
                owner: app.profile().id,
                costs: Arc::clone(&h.db) as _,
                themes: Arc::clone(&h.db) as _,
            },
        };
        handler.generate(job.payload(), job.id()).unwrap();
        assert_eq!(h.text.requests().len(), 1, "Claude is asked once");
        assert_eq!(app.music_prompt(project.id).unwrap().prompt, saved);
    }
}
