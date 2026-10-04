//! Video projects screen: pick a channel and one of its projects, choose
//! who narrates it (the channel's default persona or one for this video),
//! then work through its stages one at a time: generate, edit and review
//! the script and get a prompt for the video's music; generate (or import
//! a recording of) and play the narration with the spoken word highlighted;
//! plan the scenes and draw their images; animate them into clips; open
//! the editor. The stages and where each stands come from `bardo_app`
//! ([`project_stages`]); the screen hands its parts to the layout
//! ([`crate::layout`]). Generation runs as jobs in
//! `bardo_app`; this view polls the job revision and re-reads the script,
//! narration and scenes when it moves, and re-renders while the narration
//! plays.
//! The editor keeps the user's typing: it is refilled only when the stored
//! text changes (first generation, accepting a new script).

use std::time::Duration;

use bardo_app::bardo_domain::{
    Channel, ChannelId, Generation, Job, JobKind, JobState, Narration, NarrationSource, Network,
    NetworkAccountId, PersonaId, SceneFieldError, ScenePlanId, ScriptFieldError, TemplateKind,
    VideoMetadataDraft, VideoProject, VideoProjectId,
};
use bardo_app::{
    Bardo, BudgetConsent, Control, Destination, ExportSummary, ExportView, MusicPromptView,
    NarrationError, NarrationPlayer, NarrationView, Recording, RenderReview, RenderSummary,
    ScenesView, ScriptError, ScriptView, SpendEstimate, Stage, StageState, StageStatus, Text,
    TourAnchor, UploadChoices, UploadReview, opening_stage, project_stages,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, FocusHandle, PathPromptOptions,
    ScrollHandle, SharedString, Subscription, Task, Window, div, px,
};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::parts::{Header, ScreenParts, Stages};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};
use crate::{guide, layout};

mod export;
mod music;
mod render;
mod scenes;
mod upload;

/// How often the screen checks the job queue for changes, and moves the
/// highlighted word while the narration plays.
const POLL_EVERY: Duration = Duration::from_millis(50);

/// One option of a select: a value and its name.
#[derive(Clone)]
struct Choice<T> {
    value: T,
    title: SharedString,
}

impl<T: Clone + PartialEq> SearchableListItem for Choice<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &T {
        &self.value
    }
}

type ChoiceSelect<T> = Entity<SelectState<SearchableVec<Choice<T>>>>;

/// Which generation's prompt is open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptShown {
    Source,
    Pending,
    ScenePlan,
    MusicPrompt,
    Metadata,
}

/// Asks the window to open a project in the editor.
pub struct OpenEditor(pub VideoProjectId);

impl EventEmitter<OpenEditor> for ProjectsScreen {}

pub struct ProjectsScreen {
    bardo: Entity<Bardo>,
    channels: Vec<Channel>,
    channel_select: ChoiceSelect<ChannelId>,
    /// The project's narrator: the channel's default (`None`) or a persona.
    narrator_select: ChoiceSelect<Option<PersonaId>>,
    channel: Option<ChannelId>,
    projects: Vec<VideoProject>,
    project: Option<VideoProjectId>,
    /// The stage on screen.
    stage: Stage,
    /// The scene open in the inspector at the Scenes and Clips stages.
    selected_scene: Option<usize>,
    /// Takes ↑/↓ and Enter over the scenes.
    scene_keys: FocusHandle,
    /// Scrolls the scenes when a layout lists them in a scroll of their own.
    scene_scroll: ScrollHandle,
    /// Scrolls the Script and Narration pages, so a tour can bring their
    /// controls into view.
    page_scroll: ScrollHandle,
    /// The scene inspector's scroll, for the Scenes and Clips tours.
    inspector_scroll: ScrollHandle,
    /// Whether the scene grid shows only the scenes with something left.
    pending_only: bool,
    view: Option<ScriptView>,
    narration: Option<NarrationView>,
    /// The narration loaded for playback, once the user plays it.
    player: Option<NarrationPlayer>,
    /// Whether the player was playing at the last poll, so the view also
    /// redraws once when playback reaches the end on its own.
    was_playing: bool,
    narration_error: Option<Text>,
    scenes: Option<ScenesView>,
    scenes_error: Option<Text>,
    /// The scene prompt open in `scene_editor`: which scene, and whether
    /// its image or motion prompt.
    editing_scene: Option<(ScenePlanId, usize, scenes::ScenePromptField)>,
    scene_editor: Entity<TextareaState>,
    scene_field_error: Option<SceneFieldError>,
    /// "Plan again" was clicked on scenes that have images: the panel asks
    /// before discarding them.
    confirm_replan: bool,
    /// A scene action held back at a budget, and what it would cost.
    scenes_ask: Option<(scenes::SceneAction, SpendEstimate)>,
    /// A script generation held back at a budget.
    script_ask: Option<SpendEstimate>,
    /// A narration held back at a budget.
    narration_ask: Option<SpendEstimate>,
    /// Choosing or reading a recording to import; dropping it stops
    /// waiting for the result.
    choosing_recording: Option<Task<()>>,
    /// Whether the chosen file is being read.
    reading_recording: bool,
    /// A recording read and waiting for the user to confirm the import,
    /// with what aligning it would cost.
    recording: Option<(Recording, SpendEstimate)>,
    /// An import held back at a budget.
    import_ask: Option<SpendEstimate>,
    /// The music prompt card, and the prompt as the user types it.
    music: Option<MusicPromptView>,
    music_editor: Entity<TextareaState>,
    /// The prompt text the field was last filled with.
    music_loaded: Option<String>,
    music_error: Option<Text>,
    music_notice: Option<Text>,
    /// The music prompt waiting on a budget answer.
    music_ask: Option<SpendEstimate>,
    /// The Render stage's review of the project's cut.
    render_review: Option<RenderReview>,
    /// Where the project's renders stand, for its Render stage.
    render_summary: Option<RenderSummary>,
    /// What the last check found; applies while the cut stays the same.
    render_found: Option<bardo_app::CheckFound>,
    /// The encoders and the mix being checked off the UI thread; dropping
    /// it stops waiting for the result.
    render_checking: Option<Task<()>>,
    /// The last check failed: no new one starts until "Check again".
    render_check_failed: bool,
    /// Targets the user ticked in or out of the render; the others follow
    /// whether their last file is current.
    render_choices: Vec<(NetworkAccountId, bool)>,
    /// The target open in the inspector.
    selected_target: Option<NetworkAccountId>,
    /// "Render" was clicked: the page asks before the job starts.
    confirm_render: bool,
    render_error: Option<Text>,
    /// The Publish stage: every network's metadata and export.
    export_view: Option<ExportView>,
    /// Where the project's exports stand, for its Publish stage.
    export_summary: Option<ExportSummary>,
    export_error: Option<Text>,
    /// Reading the Publish stage failed; cleared by the next good read.
    export_load_error: Option<Text>,
    export_notice: Option<Text>,
    /// Writing the metadata waits on a budget answer.
    metadata_ask: Option<SpendEstimate>,
    /// "Write again" was clicked over edited metadata: the page asks first.
    confirm_metadata: bool,
    /// Networks the user ticked in or out of the export; the others follow
    /// whether their last export is current.
    export_choices: Vec<(Network, bool)>,
    /// The network open in the inspector.
    selected_network: Option<Network>,
    metadata_title: Entity<InputState>,
    metadata_description: Entity<TextareaState>,
    metadata_tags: Entity<InputState>,
    /// The network and stored metadata the fields were last filled with.
    metadata_loaded: Option<(Network, VideoMetadataDraft)>,
    /// The post's link as the user pastes it.
    post_link: Entity<InputState>,
    /// "Change link" was clicked on a linked post.
    post_editing: bool,
    /// "Unlink" was clicked: the inspector asks first.
    post_confirm_remove: bool,
    /// Why the last link, unlink or sync did not happen, as said.
    post_error: Option<SharedString>,
    /// Linking a post was refused because it replaces an upload: the
    /// inspector asks first.
    post_confirm_replace: bool,
    /// The upload review of the shown network as stored now, when Bardo
    /// uploads to it.
    upload_now: Option<UploadReview>,
    /// "Review upload" was clicked: the review the user sees and their
    /// choices, kept until they upload or go back.
    upload_draft: Option<(UploadReview, UploadChoices)>,
    /// Why the upload did not start, or what its job action did not do.
    upload_error: Option<Text>,
    /// "Schedule" is chosen in the open review.
    upload_scheduled: bool,
    /// The publish date and time as the user types them, in the review or
    /// when changing a scheduled upload's time.
    schedule_date: Entity<InputState>,
    schedule_time: Entity<InputState>,
    /// Why the typed publish time cannot be used.
    schedule_problem: Option<Text>,
    /// A Reel's cover time as the user types it, in the review.
    upload_cover: Entity<InputState>,
    /// Why the typed cover time cannot be used.
    cover_problem: Option<Text>,
    /// "Change time" or "Cancel schedule" was clicked on a scheduled upload.
    schedule_edit: Option<upload::ScheduleEdit>,
    /// The change being sent to the network.
    schedule_task: Option<Task<()>>,
    /// How the last change ended.
    schedule_notice: Option<(Tone, Text)>,
    editor: Entity<TextareaState>,
    /// The stored text last placed in the editor.
    loaded: Option<String>,
    field_error: Option<ScriptFieldError>,
    error: Option<Text>,
    notice: Option<Text>,
    prompt_shown: Option<PromptShown>,
    revision: u64,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl ProjectsScreen {
    pub fn new(bardo: Entity<Bardo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let channel_select =
            cx.new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx));
        let narrator_select = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::new()), None, window, cx).searchable(true)
        });
        let editor = cx.new(|cx| TextareaState::new(window, cx).auto_grow(12, 24));
        let scene_editor = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 8));
        let music_editor = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 8));
        let metadata_title = cx.new(|cx| InputState::new(window, cx));
        let metadata_description = cx.new(|cx| TextareaState::new(window, cx).auto_grow(4, 12));
        let metadata_tags = cx.new(|cx| InputState::new(window, cx));
        let post_link = cx.new(|cx| InputState::new(window, cx));
        let schedule_date = cx.new(|cx| InputState::new(window, cx));
        let schedule_time = cx.new(|cx| InputState::new(window, cx));
        let upload_cover = cx.new(|cx| InputState::new(window, cx));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_EVERY).await;
                if this
                    .update_in(cx, |this, window, cx| this.poll(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        // Typing updates the counters and retires a "saved" notice.
        let typed = |this: &mut Self, event: &InputEvent, cx: &mut Context<Self>| {
            if matches!(event, InputEvent::Change) {
                if this.export_notice == Some(Text::MetadataSaved) {
                    this.export_notice = None;
                }
                cx.notify();
            }
        };
        let subscriptions = vec![
            cx.subscribe(&metadata_title, move |this, _, event, cx| {
                typed(this, event, cx)
            }),
            cx.subscribe(&metadata_description, move |this, _, event, cx| {
                typed(this, event, cx)
            }),
            cx.subscribe(&metadata_tags, move |this, _, event, cx| {
                typed(this, event, cx)
            }),
            cx.subscribe(&post_link, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) && this.post_error.is_some() {
                    this.post_error = None;
                    cx.notify();
                }
            }),
            cx.subscribe(&schedule_date, |this, _, event: &InputEvent, cx| {
                this.schedule_typed(event, cx)
            }),
            cx.subscribe(&schedule_time, |this, _, event: &InputEvent, cx| {
                this.schedule_typed(event, cx)
            }),
            cx.subscribe(&upload_cover, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) && this.cover_problem.is_some() {
                    this.cover_problem = None;
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &channel_select,
                window,
                |this, _, event: &SelectEvent<SearchableVec<Choice<ChannelId>>>, window, cx| {
                    let SelectEvent::Confirm(Some(id)) = event else {
                        return;
                    };
                    if this.channel != Some(*id) {
                        this.select_channel(*id, window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &narrator_select,
                window,
                |this,
                 _,
                 event: &SelectEvent<SearchableVec<Choice<Option<PersonaId>>>>,
                 window,
                 cx| {
                    if let SelectEvent::Confirm(Some(persona)) = event {
                        this.set_narrator(*persona, window, cx);
                    }
                },
            ),
        ];
        let revision = bardo.read(cx).jobs_revision();
        let mut screen = Self {
            bardo,
            channels: Vec::new(),
            channel_select,
            narrator_select,
            channel: None,
            projects: Vec::new(),
            project: None,
            stage: Stage::Script,
            selected_scene: None,
            scene_keys: cx.focus_handle(),
            scene_scroll: ScrollHandle::new(),
            page_scroll: ScrollHandle::new(),
            inspector_scroll: ScrollHandle::new(),
            pending_only: false,
            view: None,
            narration: None,
            player: None,
            was_playing: false,
            narration_error: None,
            scenes: None,
            scenes_error: None,
            editing_scene: None,
            scene_editor,
            scene_field_error: None,
            confirm_replan: false,
            scenes_ask: None,
            script_ask: None,
            narration_ask: None,
            choosing_recording: None,
            reading_recording: false,
            recording: None,
            import_ask: None,
            music: None,
            music_editor,
            music_loaded: None,
            music_error: None,
            music_notice: None,
            music_ask: None,
            render_review: None,
            render_summary: None,
            render_found: None,
            render_checking: None,
            render_check_failed: false,
            render_choices: Vec::new(),
            selected_target: None,
            confirm_render: false,
            render_error: None,
            export_view: None,
            export_summary: None,
            export_error: None,
            export_load_error: None,
            export_notice: None,
            metadata_ask: None,
            confirm_metadata: false,
            export_choices: Vec::new(),
            selected_network: None,
            metadata_title,
            metadata_description,
            metadata_tags,
            metadata_loaded: None,
            post_link,
            post_editing: false,
            post_confirm_remove: false,
            post_error: None,
            post_confirm_replace: false,
            upload_now: None,
            upload_draft: None,
            upload_error: None,
            upload_scheduled: false,
            schedule_date,
            schedule_time,
            schedule_problem: None,
            upload_cover,
            cover_problem: None,
            schedule_edit: None,
            schedule_task: None,
            schedule_notice: None,
            editor,
            loaded: None,
            field_error: None,
            error: None,
            notice: None,
            prompt_shown: None,
            revision,
            _poll: poll,
            _subscriptions: subscriptions,
        };
        screen.reload(window, cx);
        screen
    }

    /// Re-reads channels and projects (themes approved elsewhere start new
    /// ones), keeping the selection when it still exists.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.notice = None;
        self.channels = self.bardo.read(cx).channels().unwrap_or_default();
        let choices = SearchableVec::new(
            self.channels
                .iter()
                .map(|channel| Choice {
                    value: channel.id,
                    title: SharedString::from(channel.details.name().to_owned()),
                })
                .collect::<Vec<_>>(),
        );
        let keep = self
            .channel
            .filter(|id| self.channels.iter().any(|c| c.id == *id));
        let target = keep.or_else(|| self.channels.first().map(|c| c.id));
        self.channel_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            if let Some(id) = target {
                select.set_selected_value(&id, window, cx);
            }
        });
        match target {
            Some(_) if keep.is_some() => {
                self.load_projects(cx);
                self.load(window, cx);
                // Personas may have been added or renamed elsewhere.
                self.fill_narrator(window, cx);
            }
            Some(id) => self.select_channel(id, window, cx),
            None => {
                self.channel = None;
                self.projects.clear();
                self.project = None;
                self.view = None;
            }
        }
        cx.notify();
    }

    fn select_channel(&mut self, id: ChannelId, window: &mut Window, cx: &mut Context<Self>) {
        self.channel = Some(id);
        self.project = None;
        self.load_projects(cx);
        let first = self.projects.first().map(|project| project.id);
        self.select_project(first, window, cx);
    }

    fn load_projects(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.channel else {
            return;
        };
        match self.bardo.read(cx).video_projects(channel) {
            Ok(projects) => self.projects = projects,
            Err(_) => {
                self.projects.clear();
                self.error = Some(Text::ProjectsNotLoaded);
            }
        }
        if self
            .project
            .is_some_and(|id| !self.projects.iter().any(|p| p.id == id))
        {
            self.project = self.projects.first().map(|project| project.id);
        }
    }

    fn select_project(
        &mut self,
        id: Option<VideoProjectId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.project = id;
        self.loaded = None;
        self.field_error = None;
        self.error = None;
        self.notice = None;
        self.prompt_shown = None;
        self.player = None;
        self.narration_error = None;
        self.scenes_error = None;
        self.editing_scene = None;
        self.scene_field_error = None;
        self.confirm_replan = false;
        self.scenes_ask = None;
        self.script_ask = None;
        self.narration_ask = None;
        self.choosing_recording = None;
        self.reading_recording = false;
        self.recording = None;
        self.import_ask = None;
        self.music_loaded = None;
        self.music_error = None;
        self.music_notice = None;
        self.music_ask = None;
        self.render_found = None;
        self.render_checking = None;
        self.render_check_failed = false;
        self.render_choices.clear();
        self.selected_target = None;
        self.confirm_render = false;
        self.render_error = None;
        self.export_error = None;
        self.export_load_error = None;
        self.export_notice = None;
        self.metadata_ask = None;
        self.confirm_metadata = false;
        self.export_choices.clear();
        self.selected_network = None;
        self.metadata_loaded = None;
        self.reset_post(window, cx);
        self.selected_scene = None;
        self.pending_only = false;
        self.load(window, cx);
        self.stage = self
            .stages()
            .map_or(Stage::Script, |stages| opening_stage(&stages));
        self.ensure_render_checked(cx);
        self.fill_narrator(window, cx);
        cx.notify();
    }

    /// Every stage of the open project and where it stands.
    fn stages(&self) -> Option<Vec<StageStatus>> {
        Some(project_stages(
            self.view.as_ref()?,
            self.narration.as_ref()?,
            self.scenes.as_ref()?,
            self.render_summary.as_ref()?,
            self.export_summary.as_ref()?,
        ))
    }

    /// Shows a stage; Edit opens the project in the editor instead.
    fn pick_stage(&mut self, stage: Stage, cx: &mut Context<Self>) {
        if stage == Stage::Edit {
            if let Some(id) = self.project {
                cx.emit(OpenEditor(id));
            }
        } else if stage.is_page() {
            self.stage = stage;
            self.ensure_render_checked(cx);
            cx.notify();
        }
    }

    /// The stage on screen, when a project is open.
    pub fn current_stage(&self) -> Option<Stage> {
        self.project?;
        Some(
            self.stages()
                .map_or(self.stage, |stages| self.shown_stage(&stages)),
        )
    }

    /// The stage drawn: the one picked, unless it locked since (the scenes
    /// planned again), then the one the project would open on.
    fn shown_stage(&self, stages: &[StageStatus]) -> Stage {
        if stages
            .iter()
            .any(|status| status.stage == self.stage && status.state != StageState::Locked)
        {
            self.stage
        } else {
            opening_stage(stages)
        }
    }

    /// Whether a project is open: the screen offers its tour only then.
    pub fn has_content(&self) -> bool {
        self.stages().is_some()
    }

    /// The stage on screen, and whether it has made something (a script,
    /// a narration, scenes, a clip): its tour is offered only then.
    pub fn stage_content(&self) -> Option<(Stage, bool)> {
        let stage = self.shown_stage(&self.stages()?);
        let plan = self.scenes.as_ref().and_then(|scenes| scenes.plan.as_ref());
        let made = match stage {
            Stage::Script => self.view.as_ref().is_some_and(|view| view.script.is_some()),
            Stage::Narration => self
                .narration
                .as_ref()
                .is_some_and(|view| view.narration.is_some()),
            Stage::Scenes => plan.is_some(),
            Stage::Clips => plan.is_some_and(|plan| {
                plan.scenes()
                    .iter()
                    .any(|scene| scene.clip().is_some() || scene.pending_clip().is_some())
            }),
            Stage::Edit | Stage::Render | Stage::Publish => false,
        };
        Some((stage, made))
    }

    /// Shows a stage of the open project, as the editor's "Review &
    /// render" asks.
    pub fn show_stage(&mut self, stage: Stage, _window: &mut Window, cx: &mut Context<Self>) {
        self.pick_stage(stage, cx);
    }

    /// The narrator choices: the channel's default (named), then every
    /// persona; selects the project's own persona or the default.
    fn fill_narrator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.view.as_ref().map(|view| view.project.clone()) else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let personas = bardo.personas().unwrap_or_default();
        let channel_default = self
            .channels
            .iter()
            .find(|channel| channel.id == project.channel)
            .and_then(|channel| channel.details.default_persona())
            .and_then(|id| personas.iter().find(|persona| persona.id == id));
        let default_title = match channel_default {
            Some(persona) => SharedString::from(bardo.text_with(
                Text::ProjectNarratorChannel,
                &[("name", persona.details.name())],
            )),
            None => tr(bardo, Text::ProjectNarratorChannelNone),
        };
        let choices = SearchableVec::new(
            std::iter::once(Choice {
                value: None,
                title: default_title,
            })
            .chain(personas.iter().map(|persona| Choice {
                value: Some(persona.id),
                title: SharedString::from(persona.details.name().to_owned()),
            }))
            .collect::<Vec<_>>(),
        );
        self.narrator_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            select.set_selected_value(&project.persona, window, cx);
            if select.selected_value().is_none() {
                select.set_selected_value(&None, window, cx);
            }
        });
    }

    /// Saves the project's narrator and re-reads what depends on it.
    fn set_narrator(
        &mut self,
        persona: Option<PersonaId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.view.as_ref().map(|view| &view.project) else {
            return;
        };
        if project.persona == persona {
            return;
        }
        let id = project.id;
        match self.bardo.read(cx).set_project_persona(id, persona) {
            Ok(_) => {
                self.error = None;
                self.narration_error = None;
            }
            Err(error) => self.error = error.form_message(),
        }
        self.load(window, cx);
        self.fill_narrator(window, cx);
        cx.notify();
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            self.view = None;
            self.narration = None;
            self.player = None;
            self.scenes = None;
            self.music = None;
            self.render_review = None;
            self.render_summary = None;
            self.export_view = None;
            self.export_summary = None;
            return;
        };
        self.load_narration(id, cx);
        self.load_scenes(id, cx);
        self.load_music(id, window, cx);
        self.load_render(id, cx);
        self.load_export(id, window, cx);
        match self.bardo.read(cx).script(id) {
            Ok(view) => {
                let stored = view
                    .script
                    .as_ref()
                    .map(|script| script.text().as_str().to_owned());
                if stored != self.loaded {
                    let text = stored.clone().unwrap_or_default();
                    self.editor
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                    self.loaded = stored;
                }
                self.view = Some(view);
            }
            Err(error) => {
                self.view = None;
                self.error = Some(error.message());
            }
        }
    }

    fn load_narration(&mut self, id: VideoProjectId, cx: &mut Context<Self>) {
        match self.bardo.read(cx).narration(id) {
            Ok(view) => {
                // A new narration replaced the one loaded for playback.
                let current = view.narration.as_ref().map(|n| n.id);
                if self
                    .player
                    .as_ref()
                    .is_some_and(|player| Some(player.narration().id) != current)
                {
                    self.player = None;
                }
                self.narration = Some(view);
            }
            Err(error) => {
                self.narration = None;
                self.player = None;
                self.narration_error = Some(error.message());
            }
        }
    }

    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let revision = self.bardo.read(cx).jobs_revision();
        if revision != self.revision {
            self.revision = revision;
            self.load(window, cx);
            cx.notify();
        } else {
            let playing = self.player.as_ref().is_some_and(|p| p.is_playing());
            if playing || playing != self.was_playing {
                cx.notify();
            }
            self.was_playing = playing;
        }
    }

    fn narration_running(&self) -> bool {
        self.narration
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .is_some_and(|job| job.state().is_active())
    }

    fn generate_narration(
        &mut self,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.narration_ask = None;
        let Some(id) = self.project else {
            return;
        };
        if let Some(player) = self.player.as_mut() {
            player.pause();
        }
        self.narration_error = match self.bardo.read(cx).generate_narration(id, consent) {
            Ok(_) => None,
            Err(NarrationError::OverBudget(estimate)) => {
                self.narration_ask = Some(estimate);
                None
            }
            Err(error) => Some(error.message()),
        };
        self.load(window, cx);
        cx.notify();
    }

    /// Asks for a recording of the script, then reads it in the
    /// background to show its length and cost before importing.
    fn choose_recording(&mut self, cx: &mut Context<Self>) {
        self.narration_error = None;
        self.import_ask = None;
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), Text::ImportNarrationDialog)),
        });
        self.choosing_recording = Some(cx.spawn(async move |this, cx| {
            let (path, failed) = match chosen.await {
                Ok(Ok(Some(paths))) => (paths.into_iter().next(), false),
                Ok(Err(_)) => (None, true),
                Ok(Ok(None)) | Err(_) => (None, false),
            };
            let Some(path) = path else {
                let _ = this.update(cx, |this, cx| {
                    this.choosing_recording = None;
                    if failed {
                        this.narration_error = Some(Text::FileDialogFailed);
                    }
                    cx.notify();
                });
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.reading_recording = true;
                cx.notify();
            });
            let read = cx
                .background_executor()
                .spawn(async move { Recording::open(&path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.choosing_recording = None;
                this.reading_recording = false;
                match read.and_then(|recording| {
                    let estimate = this.bardo.read(cx).import_estimate(&recording)?;
                    Ok((recording, estimate))
                }) {
                    Ok(chosen) => this.recording = Some(chosen),
                    Err(error) => this.narration_error = Some(error.message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Starts importing the chosen recording.
    fn import_recording(
        &mut self,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.import_ask = None;
        let (Some(id), Some((recording, _))) = (self.project, self.recording.as_ref()) else {
            return;
        };
        if let Some(player) = self.player.as_mut() {
            player.pause();
        }
        match self.bardo.read(cx).import_narration(id, recording, consent) {
            Ok(_) => {
                self.recording = None;
                self.narration_error = None;
            }
            Err(NarrationError::OverBudget(estimate)) => self.import_ask = Some(estimate),
            Err(error) => self.narration_error = Some(error.message()),
        }
        self.load(window, cx);
        cx.notify();
    }

    fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        self.recording = None;
        self.import_ask = None;
        cx.notify();
    }

    /// The player, loading the narration on first use.
    fn player(&mut self, cx: &mut Context<Self>) -> Option<&mut NarrationPlayer> {
        if self.player.is_none() {
            let id = self.project?;
            match self.bardo.read(cx).play_narration(id) {
                Ok(player) => self.player = Some(player),
                Err(error) => self.narration_error = Some(error.message()),
            }
        }
        self.player.as_mut()
    }

    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        let result = self.player(cx).map(|player| player.toggle());
        if let Some(Err(error)) = result {
            self.narration_error = Some(error.message());
        } else if result.is_some() {
            self.narration_error = None;
        }
        cx.notify();
    }

    /// Plays from word `index`.
    fn play_from(&mut self, index: usize, cx: &mut Context<Self>) {
        let result = self.player(cx).map(|player| {
            player.seek_to_word(index)?;
            if !player.is_playing() {
                player.toggle()?;
            }
            Ok::<_, bardo_app::NarrationError>(())
        });
        if let Some(Err(error)) = result {
            self.narration_error = Some(error.message());
        } else if result.is_some() {
            self.narration_error = None;
        }
        cx.notify();
    }

    fn running(&self) -> bool {
        self.view
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .is_some_and(|job| job.state().is_active())
    }

    fn generate(&mut self, consent: BudgetConsent, window: &mut Window, cx: &mut Context<Self>) {
        self.script_ask = None;
        let Some(id) = self.project else {
            return;
        };
        self.error = match self.bardo.read(cx).generate_script(id, consent) {
            Ok(_) => None,
            Err(ScriptError::OverBudget(estimate)) => {
                self.script_ask = Some(estimate);
                None
            }
            Err(error) => Some(error.message()),
        };
        self.notice = None;
        self.load(window, cx);
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        let text = self.editor.read(cx).value();
        match self.bardo.read(cx).edit_script(id, &text) {
            Ok(script) => {
                self.field_error = None;
                self.error = None;
                self.notice = Some(Text::ScriptSaved);
                // The editor already shows the saved text; refilling it
                // would only move the caret.
                self.loaded = Some(script.text().as_str().to_owned());
            }
            Err(error) => {
                self.field_error = error.field_error();
                self.error = self.field_error.is_none().then(|| error.message());
                self.notice = None;
            }
        }
        self.load(window, cx);
        cx.notify();
    }

    fn revert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loaded = None;
        self.field_error = None;
        self.notice = None;
        self.load(window, cx);
        cx.notify();
    }

    fn review(&mut self, accept: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let result = if accept {
            bardo.accept_script(id)
        } else {
            bardo.reject_script(id)
        };
        self.error = result.err().map(|error| error.message());
        self.notice = None;
        self.prompt_shown = None;
        self.load(window, cx);
        cx.notify();
    }

    /// The header: Projects › channel, the project and its switcher, its
    /// line, and who narrates it.
    fn header(&self, cx: &mut Context<Self>) -> Header {
        let screen = cx.entity().downgrade();
        let bardo = self.bardo.read(cx);
        let Some(project) = self.view.as_ref().map(|view| &view.project) else {
            let mut header = Header::new(tr(bardo, Text::DestinationName(Destination::Projects)));
            header.trail = vec![
                div()
                    .w(px(200.))
                    .child(Select::new(&self.channel_select).xsmall())
                    .into_any_element(),
            ];
            return header;
        };
        let current = project.id;
        let projects: Vec<(VideoProjectId, SharedString)> = self
            .projects
            .iter()
            .map(|project| (project.id, SharedString::from(project.title.clone())))
            .collect();
        let switcher = Button::new("project-switcher")
            .ghost()
            .small()
            .icon(IconName::ChevronsUpDown)
            .tooltip(tr(bardo, Text::ProjectSwitch))
            .dropdown_menu(move |menu, _, _| {
                projects
                    .iter()
                    .fold(menu.scrollable(true), |menu, (id, title)| {
                        let id = *id;
                        let screen = screen.clone();
                        menu.item(
                            PopupMenuItem::new(title.clone())
                                .checked(id == current)
                                .on_click(move |_, window, cx| {
                                    let _ = screen.update(cx, |this, cx| {
                                        if this.project != Some(id) {
                                            this.select_project(Some(id), window, cx);
                                        }
                                    });
                                }),
                        )
                    })
            });
        let mut header = Header::new(
            kit::anchor(
                TourAnchor::Control(Control::ProjectSwitcher),
                h_flex()
                    .gap_1()
                    .min_w_0()
                    .items_center()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(project.title.clone())),
                    )
                    .child(switcher),
            )
            .min_w_0(),
        );
        header.trail = vec![
            tr(bardo, Text::DestinationName(Destination::Projects)).into_any_element(),
            div()
                .w(px(200.))
                .child(Select::new(&self.channel_select).xsmall())
                .into_any_element(),
        ];
        let mut meta = format!(
            "{} · {}",
            project.niche.label(),
            bardo.time_ago(project.created_at)
        );
        if let Some(view) = self.view.as_ref().filter(|view| !view.spent.is_zero()) {
            meta.push_str(" · ");
            meta.push_str(
                &bardo.text_with(Text::ProjectSpent, &[("amount", &bardo.money(view.spent))]),
            );
        }
        header.meta = Some(SharedString::from(meta));
        header.info = guide::header_tours(
            bardo,
            (Destination::Projects, self.has_content()),
            self.stage_content(),
            None,
            cx,
        );
        header.actions = vec![
            kit::anchor(
                TourAnchor::Control(Control::ProjectNarrator),
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(bardo, Text::ProjectNarrator)),
                    )
                    .child(
                        div().w(px(260.)).child(
                            Select::new(&self.narrator_select)
                                .small()
                                .search_placeholder(tr(bardo, Text::PersonasTitle)),
                        ),
                    )
                    .child(guide::info(
                        bardo,
                        "project-narrator-info",
                        tr(bardo, Text::ProjectNarratorHint),
                        guide::refs::PROJECTS_NARRATOR,
                    )),
            )
            .into_any_element(),
        ];
        header
    }

    /// The Script stage: the script, a new version to review, where it
    /// came from, and the music prompt.
    fn script_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = &self.view else {
            return Vec::new();
        };
        let job = self.render_job(cx);
        let scroll = Some(&self.page_scroll);
        let tagged = |anchor: TourAnchor, element: AnyElement| {
            kit::anchor_in(anchor, element, scroll).into_any_element()
        };
        let pending = view
            .script
            .as_ref()
            .and_then(|script| script.pending())
            .map(|pending| self.render_pending(pending.text().as_str(), pending.generation(), cx))
            .map(|pending| tagged(TourAnchor::Control(Control::ScriptReview), pending));
        let provenance = view
            .script
            .as_ref()
            .map(|script| {
                self.render_provenance(
                    script.source().generation(),
                    TemplateKind::Script,
                    PromptShown::Source,
                    cx,
                )
            })
            .map(|details| tagged(TourAnchor::Control(Control::ScriptDetails), details));
        let music = self
            .render_music(cx)
            .map(|music| tagged(TourAnchor::Control(Control::MusicPrompt), music));
        let bardo = self.bardo.read(cx);
        let estimate =
            estimate_note(bardo, &view.estimate, Text::EstimateCost, cx).map(|estimate| {
                tagged(
                    TourAnchor::Control(Control::ScriptEstimate),
                    estimate.into_any_element(),
                )
            });
        let running = self.running();
        let script = view.script.as_ref();

        let title_row = h_flex()
            .gap_2()
            .items_center()
            .child(kit::section_heading(tr(bardo, Text::ScriptTitle)))
            .children(script.map(|script| {
                Tag::secondary()
                    .small()
                    .child(SharedString::from(bardo.text_with(
                        Text::ScriptWords,
                        &[("n", &script.text().word_count().to_string())],
                    )))
            }))
            .when(script.is_some_and(|script| script.is_edited()), |row| {
                row.child(kit::status(Tone::Info, tr(bardo, Text::ScriptEdited), cx))
            });

        let body: AnyElement = match script {
            None => tagged(
                TourAnchor::Control(Control::ScriptBody),
                v_flex()
                    .gap_2()
                    .child(muted(cx, tr(bardo, Text::ScriptEmpty)))
                    .children(estimate)
                    .when(!running, |body| {
                        body.child(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("generate-script")
                                        .primary()
                                        .label(tr(bardo, Text::GenerateScript))
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.generate(BudgetConsent::Ask, window, cx)
                                            },
                                        )),
                                )
                                .child(guide::info(
                                    bardo,
                                    "generate-script-info",
                                    SharedString::from(bardo.text_with(
                                        Text::GenerateScriptHint,
                                        &[("n", &view.template.number.to_string())],
                                    )),
                                    guide::refs::SCRIPT_WRITE,
                                )),
                        )
                    })
                    .into_any_element(),
            ),
            Some(script) => v_flex()
                .gap_2()
                .when(script.pending().is_some(), |body| {
                    body.child(div().font_medium().child(tr(bardo, Text::ScriptCurrent)))
                })
                .child(tagged(
                    TourAnchor::Control(Control::ScriptBody),
                    v_flex()
                        .child(Textarea::new(&self.editor))
                        .into_any_element(),
                ))
                .children(self.field_error.map(|error| {
                    kit::notice(Tone::Danger, tr(bardo, Text::ScriptFieldError(error)), cx)
                        .text_xs()
                }))
                .child(tagged(
                    TourAnchor::Control(Control::ScriptActions),
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(
                            Button::new("save-script")
                                .primary()
                                .small()
                                .label(tr(bardo, Text::SaveScript))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.save(window, cx)
                                })),
                        )
                        .child(
                            Button::new("revert-script")
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::RevertScript))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.revert(window, cx)
                                })),
                        )
                        .child(div().flex_1())
                        .when(!running, |row| {
                            row.child(
                                Button::new("regenerate-script")
                                    .outline()
                                    .small()
                                    .label(tr(bardo, Text::RegenerateScript))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.generate(BudgetConsent::Ask, window, cx)
                                    })),
                            )
                            .child(guide::info(
                                bardo,
                                "regenerate-script-info",
                                tr(bardo, Text::RegenerateScriptHint),
                                guide::refs::SCRIPT_REVIEW,
                            ))
                        })
                        .into_any_element(),
                ))
                .children(estimate)
                .into_any_element(),
        };

        let ask = self.script_ask.as_ref().map(|estimate| {
            budget_question(
                "script-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.generate(BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.script_ask = None;
                    cx.notify();
                }),
            )
            .into_any_element()
        });

        std::iter::once(title_row.into_any_element())
            .chain(job)
            .chain(ask)
            .chain(pending)
            .chain(Some(body))
            .chain(provenance)
            .chain(music)
            .collect()
    }

    /// A running generation, or why the last one stopped.
    fn render_job(&self, cx: &App) -> Option<AnyElement> {
        let job = self.view.as_ref()?.job.as_ref()?;
        let bardo = self.bardo.read(cx);
        match job.state() {
            JobState::Queued | JobState::Running => Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(tr(bardo, Text::ScriptRunning)))
                    .into_any_element(),
            ),
            JobState::Failed => {
                let failure = job.failure()?;
                Some(
                    v_flex()
                        .gap_1()
                        .child(kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::ScriptStopped),
                            cx,
                        ))
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .child(tr(bardo, Text::JobFailureKindName(failure.kind))),
                                )
                                .child(kit::details(
                                    "script-failure-details",
                                    tr(bardo, Text::Details),
                                    vec![SharedString::from(failure.detail.clone())],
                                )),
                        )
                        .into_any_element(),
                )
            }
            JobState::Cancelled | JobState::Done => None,
        }
    }

    fn render_pending(
        &self,
        text: &str,
        generation: &Generation,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        kit::card(cx)
            .p_3()
            .gap_2()
            .border_color(look(cx).tokens.accent_edge)
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .font_medium()
                            .child(tr(bardo, Text::ScriptPendingTitle)),
                    )
                    .child(guide::info(
                        bardo,
                        "pending-script-info",
                        tr(bardo, Text::ScriptPendingHint),
                        guide::refs::SCRIPT_REVIEW,
                    )),
            )
            .child(
                kit::well(cx)
                    .id("pending-script")
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .child(SharedString::from(text.to_owned())),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("accept-script")
                            .primary()
                            .small()
                            .label(tr(bardo, Text::AcceptScript))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.review(true, window, cx)
                            })),
                    )
                    .child(
                        Button::new("reject-script")
                            .ghost()
                            .small()
                            .label(tr(bardo, Text::RejectScript))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.review(false, window, cx)
                            })),
                    ),
            )
            .child(self.render_provenance(
                generation,
                TemplateKind::Script,
                PromptShown::Pending,
                cx,
            ))
            .into_any_element()
    }

    /// Who generated the text, from what, and what it used: behind
    /// "Details", with the prompt one click away.
    fn render_provenance(
        &self,
        generation: &Generation,
        kind: TemplateKind,
        which: PromptShown,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let tokens = &look(cx).tokens;
        let shown = self.prompt_shown == Some(which);
        let fact = |label: Text, value: String| {
            SharedString::from(format!("{}: {value}", bardo.text(label)))
        };
        let prompt_block = |label: Text, text: &str| {
            v_flex()
                .gap_1()
                .child(div().text_xs().font_medium().child(tr(bardo, label)))
                .child(
                    kit::well(cx)
                        .text_xs()
                        .child(SharedString::from(text.to_owned())),
                )
        };
        let usage = generation.usage;
        let facts = vec![
            fact(
                Text::ProvenanceProvider,
                bardo
                    .text(Text::ProviderName(generation.provider))
                    .into_owned(),
            ),
            fact(Text::ProvenanceModel, generation.model.clone()),
            fact(
                Text::ProvenanceTemplate,
                format!(
                    "{} {}",
                    bardo.text(Text::TemplateKindName(kind)),
                    bardo.text_with(
                        Text::ProvenanceTemplateVersion,
                        &[("n", &generation.template.number.to_string())],
                    )
                ),
            ),
            fact(
                Text::ProvenanceTokens,
                bardo.text_with(
                    Text::ProvenanceTokensValue,
                    &[
                        ("input", &usage.input_tokens.to_string()),
                        ("output", &usage.output_tokens.to_string()),
                    ],
                ),
            ),
            fact(
                Text::ProvenanceGenerated,
                bardo.time_ago(generation.generated_at),
            ),
        ];
        let (details_id, prompt_id) = match which {
            PromptShown::Source => ("source-details", "toggle-source-prompt"),
            PromptShown::Pending => ("pending-details", "toggle-pending-prompt"),
            PromptShown::ScenePlan => ("scene-plan-details", "toggle-scene-plan-prompt"),
            PromptShown::MusicPrompt => ("music-prompt-details", "toggle-music-prompt"),
            PromptShown::Metadata => ("metadata-details", "toggle-metadata-prompt"),
        };

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .text_color(tokens.text2)
                    .child(kit::details(details_id, tr(bardo, Text::Details), facts))
                    .child(
                        Button::new(prompt_id)
                            .ghost()
                            .xsmall()
                            .label(tr(
                                bardo,
                                if shown {
                                    Text::HidePrompt
                                } else {
                                    Text::ShowPrompt
                                },
                            ))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.prompt_shown = if this.prompt_shown == Some(which) {
                                    None
                                } else {
                                    Some(which)
                                };
                                cx.notify();
                            })),
                    ),
            )
            .when(shown, |panel| {
                panel
                    .child(prompt_block(
                        Text::PromptInstructions,
                        &generation.instructions,
                    ))
                    .child(prompt_block(Text::PromptTask, &generation.prompt))
            })
            .into_any_element()
    }
}

/// `m:ss`, as players show time.
fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

impl ProjectsScreen {
    /// The narration: generate it, see whether it still matches the
    /// script, play it with the spoken word highlighted, and its record.
    fn render_narration(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.narration.as_ref()?;
        let narration = view.narration.as_ref();
        let player = narration.map(|narration| self.render_player(narration, cx));
        let job = view
            .job
            .as_ref()
            .and_then(|job| self.render_narration_job(job, cx));
        let record = narration.map(|narration| self.render_narration_record(narration, cx));
        let bardo = self.bardo.read(cx);
        let running = self.narration_running();
        let scroll = Some(&self.page_scroll);
        let tagged = |anchor: TourAnchor, element: AnyElement| {
            kit::anchor_in(anchor, element, scroll).into_any_element()
        };
        let player =
            player.map(|player| tagged(TourAnchor::Control(Control::NarrationPlayer), player));

        let title_row = h_flex()
            .gap_2()
            .items_center()
            .child(kit::section_heading(tr(bardo, Text::NarrationTitle)))
            .when(view.stale, |row| {
                row.child(kit::status(
                    Tone::Warning,
                    tr(bardo, Text::NarrationStaleTag),
                    cx,
                ))
                .child(guide::info(
                    bardo,
                    "narration-stale-info",
                    tr(bardo, Text::NarrationStale),
                    guide::refs::NARRATION_STALE,
                ))
            })
            .children(record);

        let flag = view.persona.as_ref().and_then(|persona| persona.voice_flag);
        // Why narration can't be generated is a state; how it will be
        // generated is an explanation behind the ⓘ.
        let (blocked, hint) = match (&view.persona, flag) {
            _ if view.script.is_none() => (Some(Tone::Info), tr(bardo, Text::NarrationNoScript)),
            (Some(_), Some(flag)) => (
                Some(Tone::Warning),
                tr(bardo, Text::NarrationVoiceFlagged(flag)),
            ),
            (Some(persona), None) => (
                None,
                SharedString::from(bardo.text_with(
                    Text::NarrationGenerateHint,
                    &[
                        ("persona", persona.details.name()),
                        ("voice", persona.details.voice().name()),
                        ("n", &view.characters().to_string()),
                    ],
                )),
            ),
            (None, _) => (Some(Tone::Info), tr(bardo, Text::NarrationNoPersona)),
        };
        let can_generate =
            !running && view.script.is_some() && view.persona.is_some() && flag.is_none();
        let generate = Button::new("generate-narration")
            .label(tr(
                bardo,
                if narration.is_some() {
                    Text::RegenerateNarration
                } else {
                    Text::GenerateNarration
                },
            ))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.generate_narration(BudgetConsent::Ask, window, cx)
            }));
        let generate = if narration.is_some() && !view.stale {
            generate.outline().small()
        } else {
            generate.primary().small()
        };
        let choosing = self.choosing_recording.is_some() || self.recording.is_some();
        let can_import = !running && !choosing && view.script.is_some();
        let import = Button::new("import-narration")
            .label(tr(bardo, Text::ImportNarration))
            .outline()
            .small()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.choose_recording(cx)));
        let chosen = self
            .recording
            .as_ref()
            .filter(|_| self.import_ask.is_none())
            .map(|(recording, estimate)| {
                self.render_chosen_recording(recording, estimate, narration.is_some(), cx)
            });

        Some(
            v_flex()
                .gap_2()
                .child(tagged(
                    TourAnchor::Control(Control::NarrationStatus),
                    title_row.into_any_element(),
                ))
                .children(
                    self.narration_error
                        .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
                )
                .children(job)
                .when(narration.is_none(), |panel| {
                    panel.child(muted(cx, tr(bardo, Text::NarrationEmpty)))
                })
                .children(player)
                .child(
                    v_flex()
                        .gap_1()
                        .children(blocked.map(|tone| kit::notice(tone, hint.clone(), cx)))
                        .when(can_generate, |panel| {
                            panel.children(view.estimate.as_ref().and_then(|estimate| {
                                estimate_note(bardo, estimate, Text::EstimateCost, cx)
                            }))
                        })
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .when(can_generate, |row| {
                                    row.child(tagged(
                                        TourAnchor::Control(Control::NarrationGenerate),
                                        h_flex()
                                            .gap_1()
                                            .items_center()
                                            .child(generate)
                                            .child(guide::info(
                                                bardo,
                                                "generate-narration-info",
                                                hint.clone(),
                                                guide::refs::NARRATION_GENERATE,
                                            ))
                                            .into_any_element(),
                                    ))
                                })
                                .when(can_import, |row| {
                                    row.child(div().w(px(8.))).child(tagged(
                                        TourAnchor::Control(Control::NarrationImport),
                                        h_flex()
                                            .gap_1()
                                            .items_center()
                                            .child(import)
                                            .child(guide::info(
                                                bardo,
                                                "import-narration-info",
                                                tr(bardo, Text::ImportNarrationHint),
                                                guide::refs::NARRATION_IMPORT,
                                            ))
                                            .into_any_element(),
                                    ))
                                }),
                        ),
                )
                .when(self.reading_recording, |panel| {
                    panel.child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(Spinner::new().small())
                            .child(div().text_sm().child(tr(bardo, Text::RecordingReading))),
                    )
                })
                .children(chosen)
                .children(self.narration_ask.as_ref().map(|estimate| {
                    budget_question(
                        "narration-budget",
                        bardo,
                        estimate,
                        cx,
                        cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.generate_narration(BudgetConsent::Confirmed, window, cx)
                        }),
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.narration_ask = None;
                            cx.notify();
                        }),
                    )
                }))
                .children(self.import_ask.as_ref().map(|estimate| {
                    budget_question(
                        "import-budget",
                        bardo,
                        estimate,
                        cx,
                        cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.import_recording(BudgetConsent::Confirmed, window, cx)
                        }),
                        cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_recording(cx)),
                    )
                }))
                .into_any_element(),
        )
    }

    /// A narration being recorded, or why the last one stopped.
    fn render_narration_job(&self, job: &Job, cx: &App) -> Option<AnyElement> {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        match job.state() {
            JobState::Queued | JobState::Running => Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(tr(
                        bardo,
                        if job.kind() == JobKind::NarrationImport {
                            Text::NarrationAligning
                        } else {
                            Text::NarrationRunning
                        },
                    )))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(format!("{}%", job.progress().permille() / 10)),
                    ))
                    .into_any_element(),
            ),
            JobState::Failed => {
                let failure = job.failure()?;
                Some(
                    v_flex()
                        .gap_1()
                        .child(kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::NarrationStopped),
                            cx,
                        ))
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .child(tr(bardo, Text::JobFailureKindName(failure.kind))),
                                )
                                .child(kit::details(
                                    "narration-failure-details",
                                    tr(bardo, Text::Details),
                                    vec![SharedString::from(failure.detail.clone())],
                                )),
                        )
                        .into_any_element(),
                )
            }
            JobState::Cancelled | JobState::Done => None,
        }
    }

    /// Play/pause, the time, and the narrated words with the one being
    /// spoken highlighted; clicking a word plays from it.
    fn render_player(&self, narration: &Narration, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let tokens = look(cx).tokens;
        let player = self
            .player
            .as_ref()
            .filter(|player| player.narration().id == narration.id);
        let playing = player.is_some_and(|player| player.is_playing());
        let position = player.map_or(Duration::ZERO, |player| player.position());
        let current = player.and_then(|player| player.current_word());

        let controls = h_flex()
            .gap_3()
            .items_center()
            .child(
                Button::new("toggle-narration")
                    .primary()
                    .small()
                    .label(tr(
                        bardo,
                        if playing {
                            Text::NarrationPause
                        } else {
                            Text::NarrationPlay
                        },
                    ))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_playback(cx))),
            )
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .child(SharedString::from(format!(
                        "{} / {}",
                        clock(position),
                        clock(narration.duration)
                    ))),
            )
            .child(guide::info(
                bardo,
                "narration-words-info",
                tr(bardo, Text::NarrationWordsHint),
                guide::refs::NARRATION_PLAY,
            ));

        let text = narration.text.as_str();
        let mut words: Vec<AnyElement> = Vec::with_capacity(narration.words.len());
        let mut previous_end = 0;
        for (index, timing) in narration.words.as_slice().iter().enumerate() {
            // A line break in the script starts a new line here too.
            if index > 0 && text[previous_end..timing.text.start].contains('\n') {
                words.push(div().w_full().h(px(6.)).into_any_element());
            }
            previous_end = timing.text.end;
            let spoken = current == Some(index);
            words.push(
                div()
                    .id(("narration-word", index))
                    .px_0p5()
                    .rounded_sm()
                    .cursor_pointer()
                    .when(spoken, |word| {
                        word.bg(tokens.accent).text_color(tokens.on_accent)
                    })
                    .when(!spoken, |word| word.hover(|word| word.bg(theme.list_hover)))
                    .child(SharedString::from(text[timing.text.clone()].to_owned()))
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.play_from(index, cx)),
                    )
                    .into_any_element(),
            );
        }

        v_flex()
            .gap_2()
            .child(controls)
            .child(
                kit::well(cx)
                    .id("narration-words")
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_wrap()
                    .gap_x_1()
                    .gap_y_0p5()
                    .p_2()
                    .children(words),
            )
            .into_any_element()
    }

    /// What generated the narration and what it cost, behind "Details".
    fn render_narration_record(&self, narration: &Narration, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let fact = |label: Text, value: String| {
            SharedString::from(format!("{}: {value}", bardo.text(label)))
        };
        let source = &narration.source;
        let mut facts = vec![
            fact(
                Text::ProvenanceProvider,
                bardo
                    .text(Text::ProviderName(source.provider()))
                    .into_owned(),
            ),
            fact(Text::ProvenanceModel, source.model().to_owned()),
        ];
        match source {
            NarrationSource::Generated {
                voice,
                billed_characters,
                ..
            } => {
                facts.push(fact(Text::NarrationVoice, voice.name().to_owned()));
                facts.push(fact(
                    Text::NarrationCost,
                    bardo.text_with(
                        Text::NarrationCostValue,
                        &[("n", &billed_characters.to_string())],
                    ),
                ));
            }
            NarrationSource::Imported { file_name, .. } => {
                facts.push(fact(Text::NarrationRecording, file_name.clone()));
                facts.push(fact(
                    Text::NarrationCost,
                    bardo.text_with(
                        Text::NarrationAudioValue,
                        &[("length", &clock(narration.duration))],
                    ),
                ));
            }
        }
        let made = match source {
            NarrationSource::Generated { .. } => Text::ProvenanceGenerated,
            NarrationSource::Imported { .. } => Text::NarrationImported,
        };
        facts.push(fact(Text::NarrationDuration, clock(narration.duration)));
        facts.push(fact(made, bardo.time_ago(narration.generated_at)));
        kit::details("narration-details", tr(bardo, Text::Details), facts).into_any_element()
    }

    /// The recording the user chose: its name and length, what aligning it
    /// costs, and whether to go ahead.
    fn render_chosen_recording(
        &self,
        recording: &Recording,
        estimate: &SpendEstimate,
        replaces: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        kit::card(cx)
            .gap_2()
            .p_3()
            .child(div().text_sm().child(SharedString::from(bardo.text_with(
                Text::RecordingChosen,
                &[
                    ("file", &recording.file_name),
                    ("length", &clock(recording.duration)),
                ],
            ))))
            .when(replaces, |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(tr(bardo, Text::RecordingReplaces)),
                )
            })
            .children(estimate_note(bardo, estimate, Text::EstimateCost, cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("use-recording")
                            .primary()
                            .small()
                            .label(tr(bardo, Text::UseRecording))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.import_recording(BudgetConsent::Ask, window, cx)
                            })),
                    )
                    .child(
                        Button::new("cancel-recording")
                            .ghost()
                            .small()
                            .label(tr(bardo, Text::CancelRecording))
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.cancel_recording(cx)
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn muted(cx: &App, text: SharedString) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

impl Render for ProjectsScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.channels.is_empty() {
            let bardo = self.bardo.read(cx);
            let mut parts = ScreenParts::new(Header::place(bardo, Destination::Projects));
            parts
                .content
                .push(muted(cx, tr(bardo, Text::ProjectsNoChannels)));
            return layout::screen(parts, cx);
        }
        let mut parts = ScreenParts::new(self.header(cx));
        let Some(stages) = self.stages() else {
            let bardo = self.bardo.read(cx);
            match self.error {
                Some(error) => parts
                    .notices
                    .push(kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element()),
                None if self.project.is_none() => parts
                    .content
                    .push(muted(cx, tr(bardo, Text::ProjectsEmpty))),
                None => {}
            }
            return layout::screen(parts, cx);
        };
        let current = self.shown_stage(&stages);
        match current {
            Stage::Script => {
                parts.content = self.script_page(cx);
                parts.scroll = Some(self.page_scroll.clone());
            }
            Stage::Narration => {
                parts.content.extend(self.render_narration(cx));
                parts.scroll = Some(self.page_scroll.clone());
            }
            Stage::Scenes | Stage::Clips => self.scene_parts(current, &mut parts, cx),
            Stage::Render => self.render_parts(&mut parts, cx),
            Stage::Publish => self.export_parts(&mut parts, cx),
            Stage::Edit => {}
        }
        let screen = cx.entity().downgrade();
        let bardo = self.bardo.read(cx);
        parts.stages = Some(Stages::new(bardo, &stages, current, move |stage, _, cx| {
            let _ = screen.update(cx, |this, cx| this.pick_stage(stage, cx));
        }));
        // What happened to the project as a whole leads.
        let general: Vec<AnyElement> =
            self.notice
                .map(|notice| kit::notice(Tone::Success, tr(bardo, notice), cx).into_any_element())
                .into_iter()
                .chain(self.error.map(|error| {
                    kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element()
                }))
                .collect();
        parts.notices.splice(0..0, general);
        layout::screen(parts, cx)
    }
}
