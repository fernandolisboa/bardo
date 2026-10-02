//! Video projects screen: pick a channel and one of its projects, choose
//! who narrates it (the channel's default persona or one for this video),
//! then generate, edit and review the project's script, generate (or import
//! a recording of) and play its narration with the spoken word highlighted,
//! plan its scenes and draw their images, and get a prompt for the
//! video's music. Generation runs as jobs in
//! `bardo_app`; this view polls the job revision and re-reads the script,
//! narration and scenes when it moves, and re-renders while the narration
//! plays.
//! The editor keeps the user's typing: it is refilled only when the stored
//! text changes (first generation, accepting a new script).

use std::time::Duration;

use bardo_app::bardo_domain::{
    Channel, ChannelId, Generation, Job, JobKind, JobState, Narration, NarrationSource, PersonaId,
    SceneFieldError, ScenePlanId, ScriptFieldError, TemplateKind, VideoProject, VideoProjectId,
};
use bardo_app::{
    Bardo, BudgetConsent, MusicPromptView, NarrationError, NarrationPlayer, NarrationView,
    Recording, ScenesView, ScriptError, ScriptView, SpendEstimate, Text,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, EventEmitter, PathPromptOptions, SharedString,
    Subscription, Task, Window, div, px,
};

use crate::appearance::look;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

mod music;
mod scenes;

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
        let subscriptions = vec![
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
        self.load(window, cx);
        self.fill_narrator(window, cx);
        cx.notify();
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
            return;
        };
        self.load_narration(id, cx);
        self.load_scenes(id, cx);
        self.load_music(id, window, cx);
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

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let rows: Vec<AnyElement> = self
            .projects
            .iter()
            .enumerate()
            .map(|(ix, project)| {
                let id = project.id;
                let selected = self.project == Some(id);
                kit::list_row(("project", ix), selected, cx)
                    .child(SharedString::from(project.title.clone()))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(format!(
                            "{} · {}",
                            project.niche.label(),
                            bardo.time_ago(project.created_at)
                        )),
                    ))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        if this.project != Some(id) {
                            this.select_project(Some(id), window, cx);
                        }
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();

        kit::side_panel(cx)
            .id("projects-list")
            .w(px(300.))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(kit::title(tr(bardo, Text::ProjectsTitle)))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(tr(bardo, Text::ThemesChannel)),
                    )
                    .child(Select::new(&self.channel_select)),
            )
            .when(empty, |list| {
                list.child(muted(cx, tr(bardo, Text::ProjectsEmpty)))
            })
            .children(rows)
    }

    fn render_script(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let Some(view) = &self.view else {
            return v_flex()
                .flex_1()
                .p_4()
                .children(
                    self.error
                        .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
                )
                .into_any_element();
        };
        let running = self.running();
        let project = &view.project;
        let script = view.script.as_ref();

        let header =
            v_flex()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(
                            div()
                                .text_xl()
                                .font_semibold()
                                .child(SharedString::from(project.title.clone())),
                        )
                        .child({
                            let id = project.id;
                            Button::new("open-editor")
                                .small()
                                .outline()
                                .label(tr(bardo, Text::OpenEditor))
                                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                    cx.emit(OpenEditor(id));
                                }))
                        }),
                )
                .child(div().text_xs().text_color(theme.muted_foreground).child(
                    SharedString::from(format!(
                        "{} · {}",
                        project.niche.label(),
                        bardo.time_ago(project.created_at)
                    )),
                ))
                .when(!view.spent.is_zero(), |header| {
                    header.child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(bardo.text_with(
                            Text::ProjectSpent,
                            &[("amount", &bardo.money(view.spent))],
                        )),
                    ))
                })
                .child(
                    h_flex()
                        .pt_2()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .child(tr(bardo, Text::ProjectNarrator)),
                        )
                        .child(
                            div().w(px(440.)).child(
                                Select::new(&self.narrator_select)
                                    .small()
                                    .search_placeholder(tr(bardo, Text::PersonasTitle)),
                            ),
                        )
                        .child(kit::info(
                            "project-narrator-info",
                            None,
                            tr(bardo, Text::ProjectNarratorHint),
                        )),
                );

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
            None => v_flex()
                .gap_2()
                .child(muted(cx, tr(bardo, Text::ScriptEmpty)))
                .children(estimate_note(bardo, &view.estimate, Text::EstimateCost, cx))
                .when(!running, |body| {
                    body.child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("generate-script")
                                    .primary()
                                    .label(tr(bardo, Text::GenerateScript))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.generate(BudgetConsent::Ask, window, cx)
                                    })),
                            )
                            .child(kit::info(
                                "generate-script-info",
                                None,
                                SharedString::from(bardo.text_with(
                                    Text::GenerateScriptHint,
                                    &[("n", &view.template.number.to_string())],
                                )),
                            )),
                    )
                })
                .into_any_element(),
            Some(script) => v_flex()
                .gap_2()
                .when(script.pending().is_some(), |body| {
                    body.child(div().font_medium().child(tr(bardo, Text::ScriptCurrent)))
                })
                .child(Textarea::new(&self.editor))
                .children(self.field_error.map(|error| {
                    kit::notice(Tone::Danger, tr(bardo, Text::ScriptFieldError(error)), cx)
                        .text_xs()
                }))
                .child(
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
                            .child(kit::info(
                                "regenerate-script-info",
                                None,
                                tr(bardo, Text::RegenerateScriptHint),
                            ))
                        }),
                )
                .children(estimate_note(bardo, &view.estimate, Text::EstimateCost, cx))
                .into_any_element(),
        };

        v_flex()
            .id("projects-script")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(header)
            .child(title_row)
            .children(
                self.notice
                    .map(|notice| kit::notice(Tone::Success, tr(bardo, notice), cx)),
            )
            .children(
                self.error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .children(self.render_job(cx))
            .children(self.script_ask.as_ref().map(|estimate| {
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
            }))
            .children(script.and_then(|s| s.pending()).map(|pending| {
                self.render_pending(pending.text().as_str(), pending.generation(), cx)
            }))
            .child(body)
            .children(script.map(|script| {
                self.render_provenance(
                    script.source().generation(),
                    TemplateKind::Script,
                    PromptShown::Source,
                    cx,
                )
            }))
            .children(self.render_narration(cx))
            .children(self.render_scenes(cx))
            .children(self.render_music(cx))
            .into_any_element()
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
                    .child(kit::info(
                        "pending-script-info",
                        None,
                        tr(bardo, Text::ScriptPendingHint),
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
        let theme = cx.theme();
        let running = self.narration_running();

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
                .child(kit::info(
                    "narration-stale-info",
                    None,
                    tr(bardo, Text::NarrationStale),
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
                .pt_3()
                .gap_2()
                .border_t_1()
                .border_color(theme.border)
                .child(title_row)
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
                                    row.child(generate).child(kit::info(
                                        "generate-narration-info",
                                        None,
                                        hint.clone(),
                                    ))
                                })
                                .when(can_import, |row| {
                                    row.child(div().w(px(8.))).child(import).child(kit::info(
                                        "import-narration-info",
                                        None,
                                        tr(bardo, Text::ImportNarrationHint),
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
            .child(kit::info(
                "narration-words-info",
                None,
                tr(bardo, Text::NarrationWordsHint),
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
            return h_flex()
                .size_full()
                .p_6()
                .items_start()
                .child(muted(cx, tr(bardo, Text::ProjectsNoChannels)))
                .into_any_element();
        }
        h_flex()
            .size_full()
            .items_start()
            .child(self.render_list(cx))
            .child(self.render_script(cx))
            .into_any_element()
    }
}
