//! Captures (#189): importing a recording of the user reading the script
//! on camera from the Narration stage, and the Capture stage that shows
//! its takes. The file is read off the UI thread before anything is paid;
//! the import card asks whose voice the speech takes and for the user's
//! word that the recording shows only them. The takes and their cuts come
//! from `bardo_app`; this view lays them out the same in every layout.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bardo_app::bardo_domain::{
    AnalysisPart, ApprovalBlock, BoundaryMove, BudgetLevel, Capture, CaptureVoice, ClipModelRef,
    Device, Job, JobFailureKind, JobState, LookFace, Money, Narration, Preflight, PreflightCheck,
    PreflightOutcome, Provider, SetSource, TakeChecks, TakeFlag, TakeMode, TakeResult, TakeSet,
    TakeSpend, TakeStep, VideoProjectId,
};
use bardo_app::{
    AnalysisStatus, Bardo, BudgetConsent, CaptureError, CaptureVoiceChoice, ChosenCapture, Control,
    ControlPlayer, ControlStatus, ControlVideoView, PerformedStep, PerformedTakeView,
    PerformedView, SetChoice, SetPicture, SetScene, SpendEstimate, Stage, TakeAnalysisView, Text,
    TourAnchor, take_source,
};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, StyledExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ImageSource, ObjectFit, PathPromptOptions, RenderImage,
    SharedString, Task, Window, div, img, px,
};

use super::{ProjectsScreen, clock};
use crate::appearance::look;
use crate::guide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note, over_line};

/// Importing a capture: the file being chosen or read, the one read with
/// what it would cost, and the user's answers on its card.
pub(super) struct CaptureImport {
    /// Choosing or reading a file; dropping it stops waiting.
    pub(super) choosing: Option<Task<()>>,
    pub(super) reading: bool,
    pub(super) chosen: Option<(ChosenCapture, SpendEstimate)>,
    pub(super) voice: CaptureVoiceChoice,
    /// Whether the user ticked that the recording shows only them.
    pub(super) confirmed: bool,
    /// An import held back at a budget.
    pub(super) ask: Option<SpendEstimate>,
    /// What the Capture stage says went wrong.
    pub(super) error: Option<Text>,
    /// Whether Remove capture waits for the user to confirm it.
    pub(super) confirm_remove: bool,
    /// A take's control video being watched.
    pub(super) watching: Option<Watching>,
    /// The take whose scene images are offered as its set, with them.
    pub(super) scene_picker: Option<(usize, Vec<SetScene>)>,
    /// Choosing a set picture; dropping it stops waiting.
    pub(super) choosing_set: Option<Task<()>>,
    /// The take whose prompt is open in the editor.
    pub(super) editing_prompt: Option<usize>,
    /// A take's paid step held back at a budget or the video's cap.
    pub(super) performed_ask: Option<(usize, PerformedStep, PerformedAsk)>,
    /// Whether the cap per video is open for editing.
    pub(super) editing_cap: bool,
    /// What saving the cap said: saved, or why not.
    pub(super) cap_note: Option<(Tone, Text)>,
    /// The take whose final was just added to the media.
    pub(super) used: Option<usize>,
}

/// Why a paid take waits for the user: a provider's monthly budget, with
/// what it would cost, or the video's cap, with the spend it would reach.
pub(super) enum PerformedAsk {
    Budget(SpendEstimate),
    /// Over the cap, with the take's estimate and any budget it reaches.
    Cap(TakeSpend, SpendEstimate),
}

/// A take's control video, or its draft, final or test, in its player,
/// with the picture showing.
pub(super) struct Watching {
    pub(super) take: usize,
    /// `None` for the control video.
    pub(super) step: Option<TakeStep>,
    pub(super) player: ControlPlayer,
    pub(super) frame: Option<Arc<RenderImage>>,
}

impl Default for CaptureImport {
    fn default() -> Self {
        Self {
            choosing: None,
            reading: false,
            chosen: None,
            voice: CaptureVoiceChoice::Persona,
            confirmed: false,
            ask: None,
            error: None,
            confirm_remove: false,
            watching: None,
            scene_picker: None,
            choosing_set: None,
            editing_prompt: None,
            performed_ask: None,
            editing_cap: false,
            cap_note: None,
            used: None,
        }
    }
}

/// What the video's performed takes cost so far and still need, against
/// its cap.
fn spend_line(bardo: &Bardo, spend: &TakeSpend) -> SharedString {
    SharedString::from(bardo.text_with(
        Text::TakeSpendLine,
        &[
            ("spent", &bardo.money(spend.spent)),
            ("to_come", &bardo.money(spend.to_come)),
            ("total", &bardo.money(spend.projected())),
            ("cap", &bardo.money(spend.cap)),
        ],
    ))
}

/// A stable name for a paid step, for element ids.
fn step_code(step: PerformedStep) -> &'static str {
    match step {
        PerformedStep::Prompt => "prompt",
        PerformedStep::Make(step) => step.code(),
    }
}

/// `40 s`, `3 min`: about how long an analysis has left.
fn rough(left: Duration) -> String {
    let seconds = left.as_secs();
    if seconds < 90 {
        format!("{} s", seconds.max(1))
    } else {
        format!("{} min", seconds.div_ceil(60))
    }
}

/// Whether an analysis runs or ran on the processor.
fn on_cpu(status: &AnalysisStatus) -> bool {
    matches!(
        status,
        AnalysisStatus::Running {
            gpu: Some(false),
            ..
        } | AnalysisStatus::Done {
            device: Device::Cpu { .. }
        }
    )
}

/// How much longer or shorter than its take a draft or final is, when
/// the difference shows.
fn length_note(bardo: &Bardo, made: Duration, take: Duration) -> Option<String> {
    let difference = made.abs_diff(take);
    if difference < Duration::from_millis(500) {
        return None;
    }
    let text = if made > take {
        Text::TakeLonger
    } else {
        Text::TakeShorter
    };
    Some(bardo.text_with(text, &[("seconds", &bardo.seconds(difference))]))
}

/// A player's picture, upright or wide at most 180 px high, and what
/// stopped it playing.
fn render_player(bardo: &Bardo, watching: &Watching, muted: gpui_kit::Hsla) -> AnyElement {
    let size = watching.player.size();
    let height = 180.0_f32;
    let width = height * size.width as f32 / size.height.max(1) as f32;
    let picture = div()
        .w(px(width))
        .h(px(height))
        .max_w_full()
        .bg(gpui_kit::black())
        .rounded_md()
        .overflow_hidden()
        .when_some(watching.frame.clone(), |frame, image| {
            frame.child(
                img(ImageSource::Render(image))
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            )
        });
    v_flex()
        .gap_1()
        .child(picture)
        .when_some(watching.player.error(), |column, error| {
            column.child(div().text_xs().text_color(muted).child(tr(bardo, *error)))
        })
        .into_any_element()
}

/// `1080 × 1920`.
fn picture_size(width: u32, height: u32) -> String {
    format!("{width} × {height}")
}

/// `29.97`, `30`.
fn frame_rate(fps_milli: u32) -> String {
    let fps = format!("{:.2}", f64::from(fps_milli) / 1000.0);
    fps.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl ProjectsScreen {
    /// Asks for a recording, then reads it in the background to show what
    /// it holds and what importing it costs.
    pub(super) fn choose_capture(&mut self, cx: &mut Context<Self>) {
        self.narration_error = None;
        self.capture.ask = None;
        let Some(project) = self.project else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let reader = bardo.capture_reader();
        // The persona's voice when the project has one that can take it.
        let voice = if bardo.capture_persona(project).is_ok() {
            CaptureVoiceChoice::Persona
        } else {
            CaptureVoiceChoice::Own
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(bardo, Text::CaptureImportDialog)),
        });
        self.capture.choosing = Some(cx.spawn(async move |this, cx| {
            let (path, failed) = match chosen.await {
                Ok(Ok(Some(paths))) => (paths.into_iter().next(), false),
                Ok(Err(_)) => (None, true),
                Ok(Ok(None)) | Err(_) => (None, false),
            };
            let Some(path) = path else {
                let _ = this.update(cx, |this, cx| {
                    this.capture.choosing = None;
                    if failed {
                        this.narration_error = Some(Text::FileDialogFailed);
                    }
                    cx.notify();
                });
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.capture.reading = true;
                cx.notify();
            });
            let read = cx
                .background_executor()
                .spawn(async move { reader.open(&path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.capture.choosing = None;
                this.capture.reading = false;
                match read.and_then(|chosen| {
                    let estimate = this.bardo.read(cx).capture_estimate(&chosen, voice)?;
                    Ok((chosen, estimate))
                }) {
                    Ok(chosen) => {
                        this.capture.chosen = Some(chosen);
                        this.capture.voice = voice;
                        this.capture.confirmed = false;
                    }
                    Err(error) => this.narration_error = Some(error.message()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Picks whose voice the speech takes; the estimate follows.
    fn pick_capture_voice(&mut self, voice: CaptureVoiceChoice, cx: &mut Context<Self>) {
        self.capture.voice = voice;
        if let Some((chosen, estimate)) = self.capture.chosen.as_mut() {
            match self.bardo.read(cx).capture_estimate(chosen, voice) {
                Ok(new) => *estimate = new,
                Err(error) => self.narration_error = Some(error.message()),
            }
        }
        cx.notify();
    }

    /// Starts importing the chosen capture.
    fn import_capture(
        &mut self,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture.ask = None;
        let (Some(id), Some((chosen, _))) = (self.project, self.capture.chosen.as_ref()) else {
            return;
        };
        if let Some(player) = self.player.as_mut() {
            player.pause();
        }
        match self.bardo.read(cx).import_capture(
            id,
            chosen,
            self.capture.voice,
            self.capture.confirmed,
            consent,
        ) {
            Ok(_) => {
                self.capture.chosen = None;
                self.capture.confirmed = false;
                self.narration_error = None;
            }
            Err(CaptureError::OverBudget(estimate)) => self.capture.ask = Some(estimate),
            Err(error) => self.narration_error = Some(error.message()),
        }
        self.load(window, cx);
        cx.notify();
    }

    fn cancel_capture(&mut self, cx: &mut Context<Self>) {
        self.capture.chosen = None;
        self.capture.ask = None;
        self.capture.confirmed = false;
        cx.notify();
    }

    /// Moves the cut after take `index`.
    fn move_cut(
        &mut self,
        index: usize,
        to: BoundaryMove,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project else {
            return;
        };
        self.capture.confirm_remove = false;
        self.capture.error = self
            .bardo
            .read(cx)
            .move_take_boundary(id, index, to)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    fn set_take_mode(
        &mut self,
        index: usize,
        mode: TakeMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project else {
            return;
        };
        self.capture.error = self
            .bardo
            .read(cx)
            .set_take_mode(id, index, mode)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    fn analyze_take(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        self.capture.error = self
            .bardo
            .read(cx)
            .analyze_take(id, index)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    fn build_control(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        self.capture.error = self
            .bardo
            .read(cx)
            .build_control_video(id, index)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    fn stop_control(&mut self, job: bardo_app::bardo_domain::JobId, cx: &mut Context<Self>) {
        // A job that already ended has nothing to stop.
        let _ = self.bardo.read(cx).cancel_job(job);
        cx.notify();
    }

    fn approve_control(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        self.capture.error = self
            .bardo
            .read(cx)
            .approve_control_video(id, index)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    fn set_take_set(
        &mut self,
        index: usize,
        choice: SetChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project else {
            return;
        };
        self.capture.scene_picker = None;
        self.close_control(index, window, cx);
        self.capture.error = self
            .bardo
            .read(cx)
            .set_take_set(id, index, choice)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    /// Asks for a PNG or JPEG to be take `index`'s set.
    fn choose_set_picture(&mut self, index: usize, cx: &mut Context<Self>) {
        self.capture.scene_picker = None;
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr(self.bardo.read(cx), Text::SetImportDialog)),
        });
        self.capture.choosing_set = Some(cx.spawn(async move |this, cx| {
            let (path, failed) = match chosen.await {
                Ok(Ok(Some(paths))) => (paths.into_iter().next(), false),
                Ok(Err(_)) => (None, true),
                Ok(Ok(None)) | Err(_) => (None, false),
            };
            // Read and checked off this thread: it may be large.
            let picture = match path {
                Some(path) => Some(
                    cx.background_executor()
                        .spawn(async move { SetPicture::read(&path) })
                        .await,
                ),
                None => None,
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.capture.choosing_set = None;
                match picture {
                    Some(Ok(picture)) => {
                        this.set_take_set(index, SetChoice::Picture(picture), window, cx)
                    }
                    Some(Err(error)) => this.capture.error = Some(error.message()),
                    None if failed => this.capture.error = Some(Text::FileDialogFailed),
                    None => {}
                }
                cx.notify();
            });
        }));
    }

    /// Plays take `index`'s control video, or stops it.
    fn toggle_control(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        if let Some(watching) = self
            .capture
            .watching
            .as_mut()
            .filter(|watching| watching.take == index && watching.step.is_none())
        {
            watching.player.toggle();
            cx.notify();
            return;
        }
        self.close_control(usize::MAX, window, cx);
        match self.bardo.read(cx).control_player(id, index) {
            Ok(player) => {
                self.capture.watching = Some(Watching {
                    take: index,
                    step: None,
                    player,
                    frame: None,
                });
            }
            Err(error) => self.capture.error = Some(error.message()),
        }
        cx.notify();
    }

    /// Plays take `index`'s draft or final with the take's voice, or stops
    /// it.
    fn toggle_take_result(
        &mut self,
        index: usize,
        step: TakeStep,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project else {
            return;
        };
        if let Some(watching) = self
            .capture
            .watching
            .as_mut()
            .filter(|watching| watching.take == index && watching.step == Some(step))
        {
            watching.player.toggle();
            cx.notify();
            return;
        }
        self.close_control(usize::MAX, window, cx);
        match self.bardo.read(cx).take_result_player(id, index, step) {
            Ok(player) => {
                self.capture.watching = Some(Watching {
                    take: index,
                    step: Some(step),
                    player,
                    frame: None,
                });
            }
            Err(error) => self.capture.error = Some(error.message()),
        }
        cx.notify();
    }

    /// Whether a prompt or a performed take of take `index` is being made:
    /// its mode, set, cuts and model stay as they are until it ends.
    fn take_busy(&self, index: usize) -> bool {
        self.narration
            .as_ref()
            .and_then(|view| view.performed.as_ref())
            .and_then(|performed| performed.takes.get(index))
            .is_some_and(|take| take.is_busy())
    }

    /// Runs a performed clip action on the project, showing what went
    /// wrong. `true` when it went through.
    fn performed_action(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        action: impl FnOnce(&Bardo, VideoProjectId) -> Result<(), CaptureError>,
    ) -> bool {
        let Some(id) = self.project else {
            return false;
        };
        self.capture.confirm_remove = false;
        self.capture.used = None;
        self.capture.performed_ask = None;
        self.capture.cap_note = None;
        let result = action(self.bardo.read(cx), id);
        let done = result.is_ok();
        self.capture.error = result.err().map(|error| error.message());
        self.load(window, cx);
        cx.notify();
        done
    }

    /// Makes take `index` go to `model`, or to the channel's with `None`.
    fn set_take_model(
        &mut self,
        index: usize,
        model: Option<ClipModelRef>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.capture.editing_prompt == Some(index) {
            self.capture.editing_prompt = None;
        }
        self.performed_action(window, cx, |bardo, id| {
            bardo.set_take_model(id, index, model)
        });
    }

    /// Writes take `index`'s prompt or makes its test, draft or final,
    /// asking first past a budget or the video's cap.
    fn performed_step(
        &mut self,
        index: usize,
        step: PerformedStep,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture.performed_ask = None;
        let mut ask = None;
        self.performed_action(window, cx, |bardo, id| {
            let started = match step {
                PerformedStep::Prompt => bardo.write_take_prompt(id, index, consent),
                PerformedStep::Make(made) => bardo.make_performed_take(id, index, made, consent),
            };
            match started {
                Ok(_) => Ok(()),
                Err(CaptureError::OverBudget(estimate)) => {
                    ask = Some(PerformedAsk::Budget(estimate));
                    Ok(())
                }
                Err(CaptureError::OverCap(spend, estimate)) => {
                    ask = Some(PerformedAsk::Cap(spend, estimate));
                    Ok(())
                }
                Err(error) => Err(error),
            }
        });
        if let Some(ask) = ask {
            self.capture.performed_ask = Some((index, step, ask));
        }
    }

    /// Opens take `index`'s prompt in the editor.
    fn edit_take_prompt(
        &mut self,
        index: usize,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture.editing_prompt = Some(index);
        self.take_prompt_editor
            .update(cx, |input, cx| input.set_value(text, window, cx));
        cx.notify();
    }

    fn save_take_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.capture.editing_prompt else {
            return;
        };
        let text = self.take_prompt_editor.read(cx).value().to_string();
        if self.performed_action(window, cx, |bardo, id| {
            bardo.edit_take_prompt(id, index, &text)
        }) {
            self.capture.editing_prompt = None;
        }
    }

    /// Checks again on take `index`'s draft or final job that was stopped
    /// or failed after Higgsfield took its request: it polls that request.
    fn check_take_again(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.performed_action(window, cx, |bardo, id| bardo.check_take_again(id, index));
    }

    fn use_performed_take(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        match self.bardo.read(cx).use_performed_take(id, index) {
            Ok(_) => {
                self.capture.error = None;
                self.capture.used = Some(index);
            }
            Err(error) => self.capture.error = Some(error.message()),
        }
        self.load(window, cx);
        cx.notify();
    }

    /// Opens the video's cap for editing, filled with the cap as it is.
    fn edit_take_cap(&mut self, cap: Money, window: &mut Window, cx: &mut Context<Self>) {
        self.take_cap_input.update(cx, |input, cx| {
            input.set_value(cap.to_string(), window, cx);
            input.focus(window, cx);
        });
        self.capture.editing_cap = true;
        self.capture.cap_note = None;
        cx.notify();
    }

    /// Saves the cap as typed; nothing typed goes back to the default.
    fn save_take_cap(
        &mut self,
        typed: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let typed = typed.unwrap_or_else(|| self.take_cap_input.read(cx).value().to_string());
        if self.performed_action(window, cx, |bardo, id| bardo.set_take_cap(id, &typed)) {
            self.capture.editing_cap = false;
            self.capture.cap_note = Some((Tone::Success, Text::TakeCapSaved));
        } else {
            self.capture.cap_note = self.capture.error.take().map(|error| (Tone::Danger, error));
        }
    }

    /// Closes the player, unless it is take `keep`'s, freeing its picture.
    pub(super) fn close_control(
        &mut self,
        keep: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .capture
            .watching
            .as_ref()
            .is_some_and(|watching| watching.take != keep)
            && let Some(watching) = self.capture.watching.take()
            && let Some(frame) = watching.frame
        {
            cx.drop_image(frame, Some(window));
        }
    }

    /// Pauses the control video player, if one is playing.
    pub(super) fn pause_control(&mut self) {
        if let Some(watching) = self.capture.watching.as_mut() {
            watching.player.stop();
        }
    }

    /// Moves the control video player on and swaps in a new picture when
    /// one is due. A player whose take was built again since, or off the
    /// Capture stage, closes.
    pub(super) fn advance_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(watching) = self.capture.watching.as_ref() else {
            return;
        };
        let bardo = self.bardo.read(cx);
        let narration = self.narration.as_ref();
        let path = match watching.step {
            None => narration
                .and_then(|view| view.controls.get(watching.take))
                .and_then(|view| view.built.as_ref())
                .map(|built| bardo.control_video_path(built)),
            Some(step) => narration
                .and_then(|view| view.performed.as_ref())
                .and_then(|performed| match step {
                    TakeStep::Draft => performed
                        .takes
                        .get(watching.take)
                        .and_then(|take| take.draft.as_ref()),
                    TakeStep::Final => performed
                        .takes
                        .get(watching.take)
                        .and_then(|take| take.final_take.as_ref()),
                    TakeStep::Test => performed
                        .test
                        .as_ref()
                        .filter(|test| test.position as usize == watching.take),
                })
                .map(|made| bardo.take_result_path(made)),
        };
        let current = self.current_stage() == Some(Stage::Capture)
            && path.is_some_and(|path| path == watching.player.path());
        if !current {
            self.close_control(usize::MAX, window, cx);
            return;
        }
        let Some(watching) = self.capture.watching.as_mut() else {
            return;
        };
        if !watching.player.needs_ticks() {
            return;
        }
        if let Some(frame) = watching.player.tick(Instant::now()) {
            let size = frame.size;
            // RenderImage holds BGRA in an RGBA-typed buffer; no swizzle.
            if let Some(buffer) = image::RgbaImage::from_raw(size.width, size.height, frame.bgra) {
                let next = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                if let Some(old) = watching.frame.replace(next) {
                    cx.drop_image(old, Some(window));
                }
            }
        }
        window.request_animation_frame();
    }

    fn remove_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        self.capture.error = self
            .bardo
            .read(cx)
            .remove_capture(id)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    /// The Narration stage's button that imports a capture, with its ⓘ.
    pub(super) fn render_capture_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        h_flex()
            .gap_1()
            .items_center()
            .child(
                Button::new("import-capture")
                    .label(tr(bardo, Text::CaptureImport))
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.choose_capture(cx))),
            )
            .child(guide::info(
                bardo,
                "import-capture-info",
                tr(bardo, Text::CaptureImportHint),
                guide::refs::CAPTURE_IMPORT,
            ))
            .into_any_element()
    }

    /// The Narration stage's capture parts below its buttons: the file
    /// being read, the chosen one's card, and the budget question.
    pub(super) fn render_capture_import(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let bardo = self.bardo.read(cx);
        let mut parts = Vec::new();
        if self.capture.reading {
            parts.push(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(tr(bardo, Text::RecordingReading)))
                    .into_any_element(),
            );
        }
        if self.capture.ask.is_none()
            && let Some((chosen, estimate)) = &self.capture.chosen
        {
            parts.push(self.render_chosen_capture(chosen, estimate, cx));
        }
        if let Some(estimate) = &self.capture.ask {
            parts.push(budget_question(
                "capture-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.import_capture(BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_capture(cx)),
            ));
        }
        parts
    }

    /// The capture the user chose: what it holds, whose voice its speech
    /// takes, the user's word that it shows only them, the cost, and
    /// whether to go ahead.
    fn render_chosen_capture(
        &self,
        chosen: &ChosenCapture,
        estimate: &SpendEstimate,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let persona = self.project.map(|id| bardo.capture_persona(id));
        let persona_label = match &persona {
            Some(Ok(persona)) => SharedString::from(bardo.text_with(
                Text::CaptureVoicePersona,
                &[
                    ("persona", persona.details.name()),
                    ("voice", persona.details.voice().name()),
                ],
            )),
            _ => tr(bardo, Text::CaptureVoicePersonaNone),
        };
        let persona_problem = match persona {
            Some(Err(error)) => Some(error.message()),
            _ => None,
        };
        let voice = self.capture.voice;
        let radio = |id: &'static str, label: SharedString, choice: CaptureVoiceChoice| {
            Radio::new(id)
                .label(label)
                .checked(voice == choice)
                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    if *checked {
                        this.pick_capture_voice(choice, cx);
                    }
                }))
        };
        let can_persona = persona_problem.is_none();
        let ready = self.capture.confirmed && (voice == CaptureVoiceChoice::Own || can_persona);
        let video = &chosen.video;
        kit::card(cx)
            .gap_2()
            .p_3()
            .child(div().text_sm().child(SharedString::from(bardo.text_with(
                Text::CaptureChosen,
                &[
                    ("file", &chosen.file_name),
                    ("size", &picture_size(video.width, video.height)),
                    ("fps", &frame_rate(video.fps_milli)),
                    ("length", &clock(video.duration)),
                ],
            ))))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .child(tr(bardo, Text::CaptureVoice)),
                    )
                    .child(
                        radio(
                            "capture-voice-persona",
                            persona_label,
                            CaptureVoiceChoice::Persona,
                        )
                        .disabled(!can_persona),
                    )
                    .children(persona_problem.map(|problem| {
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, problem))
                    }))
                    .child(radio(
                        "capture-voice-own",
                        tr(bardo, Text::CaptureVoiceOwn),
                        CaptureVoiceChoice::Own,
                    )),
            )
            .child(
                Checkbox::new("capture-confirm")
                    .label(tr(bardo, Text::CaptureConfirm))
                    .checked(self.capture.confirmed)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.capture.confirmed = *checked;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(tr(bardo, Text::CaptureSteps)),
            )
            .when(
                self.narration
                    .as_ref()
                    .is_some_and(|view| view.narration.is_some()),
                |card| {
                    card.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, Text::RecordingReplaces)),
                    )
                },
            )
            .children(estimate_note(bardo, estimate, Text::EstimateCost, cx))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("use-capture")
                            .primary()
                            .small()
                            .disabled(!ready)
                            .label(tr(bardo, Text::UseCapture))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.import_capture(BudgetConsent::Ask, window, cx)
                            })),
                    )
                    .child(
                        Button::new("cancel-capture")
                            .ghost()
                            .small()
                            .label(tr(bardo, Text::CancelRecording))
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_capture(cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The Capture stage: the recording and its voice, its takes with the
    /// cuts between them, and removing it.
    pub(super) fn capture_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.narration.as_ref() else {
            return Vec::new();
        };
        let (Some(capture), Some(narration)) = (view.capture.as_ref(), view.narration.as_ref())
        else {
            return Vec::new();
        };
        let takes = self.render_takes(capture, narration, cx);
        let bardo = self.bardo.read(cx);
        let scroll = Some(&self.page_scroll);
        let tagged = |anchor: TourAnchor, element: AnyElement| {
            kit::anchor_in(anchor, element, scroll).into_any_element()
        };
        let mut page = Vec::new();
        if let Some(error) = self.capture.error {
            page.push(kit::notice(Tone::Danger, tr(bardo, error), cx).into_any_element());
        }
        if view.stale {
            page.push(
                kit::notice(Tone::Warning, tr(bardo, Text::CaptureStale), cx).into_any_element(),
            );
        }
        page.push(tagged(
            TourAnchor::Control(Control::CaptureSummary),
            self.render_capture_summary(capture, cx),
        ));
        if let Some(performed) = view.performed.as_ref() {
            page.push(tagged(
                TourAnchor::Control(Control::CaptureTest),
                v_flex()
                    .gap_2()
                    .child(self.render_takes_spend(performed, cx))
                    .child(self.render_take_test(performed, cx))
                    .into_any_element(),
            ));
        }
        page.push(tagged(TourAnchor::Control(Control::CaptureTakes), takes));
        let remove = if self.capture.confirm_remove {
            kit::card(cx)
                .p_3()
                .gap_2()
                .border_color(look(cx).tokens.accent_edge)
                .child(div().text_sm().child(tr(bardo, Text::CaptureRemoveConfirm)))
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new("remove-capture-confirm")
                                .danger()
                                .small()
                                .label(tr(bardo, Text::CaptureRemove))
                                .disabled(self.narration_running())
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.remove_capture(window, cx)
                                })),
                        )
                        .child(
                            Button::new("remove-capture-keep")
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::CaptureRemoveKeep))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.capture.confirm_remove = false;
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        } else {
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new("remove-capture")
                        .danger()
                        .outline()
                        .small()
                        .label(tr(bardo, Text::CaptureRemove))
                        .disabled(self.narration_running())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.capture.confirm_remove = true;
                            this.capture.error = None;
                            cx.notify();
                        })),
                )
                .child(guide::info(
                    bardo,
                    "remove-capture-info",
                    tr(bardo, Text::CaptureRemoveHint),
                    guide::refs::CAPTURE_REMOVE,
                ))
                .into_any_element()
        };
        page.push(tagged(TourAnchor::Control(Control::CaptureRemove), remove));
        page
    }

    /// The recording, the voice its speech took, how closely it followed
    /// the script and the sentence the user confirmed.
    fn render_capture_summary(&self, capture: &Capture, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let video = &capture.video;
        let voice = match &capture.voice {
            CaptureVoice::Persona { voice, .. } => SharedString::from(voice.name().to_owned()),
            CaptureVoice::Own => tr(bardo, Text::CaptureVoiceOwnShort),
        };
        let muted = |text: String| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(SharedString::from(text))
        };
        kit::card(cx)
            .gap_1()
            .p_3()
            .child(kit::section_heading(tr(
                bardo,
                Text::StageName(bardo_app::Stage::Capture),
            )))
            .child(div().text_sm().child(SharedString::from(bardo.text_with(
                Text::CaptureSummary,
                &[
                    ("file", &capture.file_name),
                    ("size", &picture_size(video.width, video.height)),
                    ("fps", &frame_rate(video.fps_milli)),
                    ("length", &clock(video.duration)),
                ],
            ))))
            .child(div().text_sm().child(SharedString::from(format!(
                "{}: {voice}",
                bardo.text(Text::CaptureVoice)
            ))))
            .child(muted(bardo.text_with(
                Text::CaptureCoverage,
                &[("percent", &(capture.coverage / 10).to_string())],
            )))
            .child(muted(bardo.text_with(
                Text::CaptureConfirmed,
                &[("text", &capture.confirmation.text)],
            )))
            .into_any_element()
    }

    /// Each take with its words, times and state, and between two takes
    /// the buttons that move their cut.
    fn render_takes(
        &self,
        capture: &Capture,
        narration: &Narration,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let analyses = self
            .narration
            .as_ref()
            .map_or(&[][..], |view| view.analyses.as_slice());
        let mut rendered: Vec<Option<(AnyElement, AnyElement)>> = analyses
            .iter()
            .enumerate()
            .map(|(index, analysis)| Some(self.render_take_analysis(index, analysis, cx)))
            .collect();
        let mut controls: Vec<Option<AnyElement>> = self
            .narration
            .as_ref()
            .map_or(&[][..], |view| view.controls.as_slice())
            .iter()
            .enumerate()
            .map(|(index, control)| Some(self.render_take_control(index, control, cx)))
            .collect();
        let mut performed: Vec<Option<AnyElement>> = match self
            .narration
            .as_ref()
            .and_then(|view| view.performed.as_ref().map(|performed| (view, performed)))
        {
            Some((view, performed)) => performed
                .takes
                .iter()
                .enumerate()
                .map(|(index, take)| {
                    let frame = view
                        .controls
                        .get(index)
                        .filter(|control| control.outbound.sends_set_frame())
                        .map(|control| {
                            if control.mode == TakeMode::Recording {
                                Text::TakePromptFrameNoteRecording
                            } else {
                                Text::TakePromptFrameNote
                            }
                        });
                    Some(self.render_take_performed(index, performed, take, frame, cx))
                })
                .collect(),
            None => Vec::new(),
        };
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let source = take_source(narration);
        let takes = capture.takes.as_slice();
        let words = narration.words.as_slice();
        let text = narration.text.as_str();
        let busy = self.narration_running();
        let mut list = v_flex().gap_2().child(
            h_flex()
                .gap_1()
                .items_center()
                .child(kit::section_heading(tr(bardo, Text::CaptureTakesTitle)))
                .child(guide::info(
                    bardo,
                    "capture-takes-info",
                    tr(bardo, Text::CaptureTakesHint),
                    guide::refs::CAPTURE_TAKES,
                )),
        );
        // Said once: the processor runs it, which takes longer.
        if analyses
            .iter()
            .any(|view| on_cpu(&view.face) || view.depth.as_ref().is_some_and(on_cpu))
        {
            list = list.child(kit::notice(
                Tone::Info,
                tr(bardo, Text::AnalysisCpuNote),
                cx,
            ));
        }
        for (index, take) in takes.iter().enumerate() {
            let said = match (
                words.get(take.words.start),
                words.get(take.words.end.saturating_sub(1)),
            ) {
                (Some(first), Some(last)) => text[first.text.start..last.text.end].to_owned(),
                _ => String::new(),
            };
            let analysis = analyses.get(index);
            let blocked = analysis.is_some_and(TakeAnalysisView::blocked);
            let (tone, state) = match take.flag() {
                None => (Tone::Success, Text::CaptureTakeOk),
                Some(TakeFlag::TooLong) => (Tone::Warning, Text::CaptureTakeTooLong),
                Some(TakeFlag::TooShort) => (Tone::Warning, Text::CaptureTakeTooShort),
            };
            let (tone, pill) = if blocked {
                (Tone::Danger, Text::CaptureTakeBlocked)
            } else if take.flag().is_some() {
                (tone, Text::CaptureTakeFlagged)
            } else {
                (tone, state)
            };
            let row = kit::well(cx)
                .id(("capture-take", index))
                .p_2()
                .gap_1()
                .flex()
                .flex_col()
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(div().text_sm().font_medium().child(SharedString::from(
                            bardo.text_with(Text::CaptureTake, &[("n", &(index + 1).to_string())]),
                        )))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(
                            SharedString::from(bardo.text_with(
                                Text::CaptureTakeSpan,
                                &[
                                    ("start", &clock(take.start)),
                                    ("end", &clock(take.end)),
                                    ("length", &bardo.seconds(take.length())),
                                ],
                            )),
                        ))
                        .child(kit::status(tone, tr(bardo, pill), cx)),
                )
                .child(div().text_sm().child(SharedString::from(said)))
                .when(take.flag().is_some(), |row| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(tr(bardo, state)),
                    )
                });
            let row = match rendered.get_mut(index).and_then(Option::take) {
                Some((mode, lines)) => {
                    let anchored = |anchor, element: AnyElement| {
                        if index == 0 {
                            kit::anchor_in(anchor, element, Some(&self.page_scroll))
                                .into_any_element()
                        } else {
                            element
                        }
                    };
                    // The mode and the analysis it needs, lit together.
                    row.child(anchored(
                        TourAnchor::Control(Control::CaptureAnalysis),
                        v_flex().gap_1().child(mode).child(lines).into_any_element(),
                    ))
                }
                None => row,
            };
            let row = match controls.get_mut(index).and_then(Option::take) {
                Some(control) if index == 0 => row.child(
                    kit::anchor_in(
                        TourAnchor::Control(Control::CaptureControl),
                        control,
                        Some(&self.page_scroll),
                    )
                    .into_any_element(),
                ),
                Some(control) => row.child(control),
                None => row,
            };
            let row = match performed.get_mut(index).and_then(Option::take) {
                Some(section) if index == 0 => row.child(
                    kit::anchor_in(
                        TourAnchor::Control(Control::CapturePerformed),
                        section,
                        Some(&self.page_scroll),
                    )
                    .into_any_element(),
                ),
                Some(section) => row.child(section),
                None => row,
            };
            list = list.child(row);
            if index + 1 < takes.len() {
                let can = |to| {
                    !busy
                        && !self.take_busy(index)
                        && !self.take_busy(index + 1)
                        && capture.takes.boundary_target(&source, index, to).is_ok()
                };
                let cut = h_flex()
                    .gap_2()
                    .justify_center()
                    .child(
                        Button::new(("capture-cut-earlier", index))
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, Text::CaptureCutEarlier))
                            .disabled(!can(BoundaryMove::Earlier))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.move_cut(index, BoundaryMove::Earlier, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("capture-cut-later", index))
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, Text::CaptureCutLater))
                            .disabled(!can(BoundaryMove::Later))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.move_cut(index, BoundaryMove::Later, window, cx)
                            })),
                    );
                list = list.child(cut);
            }
        }
        list.into_any_element()
    }

    /// A take's mode picker, and its analysis: each part's progress and
    /// where it runs, starting or resuming it, and the checks it found.
    fn render_take_analysis(
        &self,
        index: usize,
        view: &TakeAnalysisView,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement) {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        // Only the modes the take's model takes.
        let model = self
            .narration
            .as_ref()
            .and_then(|view| view.performed.as_ref())
            .and_then(|performed| performed.takes.get(index))
            .and_then(|take| take.model.as_ref());
        let take_busy = self.take_busy(index);
        let modes = ButtonGroup::new(("take-mode", index))
            .outline()
            .xsmall()
            .children(TakeMode::ALL.map(|mode| {
                Button::new(mode.code())
                    .label(tr(bardo, Text::TakeModeName(mode)))
                    .selected(view.mode == mode)
                    .disabled(
                        view.mode != mode
                            && (take_busy || model.is_some_and(|model| !model.takes(mode))),
                    )
            }))
            .on_click(cx.listener(move |this, clicked: &Vec<usize>, window, cx| {
                if let Some(&mode) = clicked.first().and_then(|&i| TakeMode::ALL.get(i)) {
                    this.set_take_mode(index, mode, window, cx);
                }
            }));
        let mode = h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(tr(bardo, Text::CaptureModeLabel)),
            )
            .child(modes)
            .when(index == 0, |row| {
                row.child(guide::info(
                    bardo,
                    "capture-mode-info",
                    tr(bardo, Text::CaptureModeHint),
                    guide::refs::CAPTURE_MODE,
                ))
            })
            .into_any_element();

        let mut lines = v_flex().gap_1();
        let parts = [
            (AnalysisPart::Face, Some(&view.face)),
            (AnalysisPart::Depth, view.depth.as_ref()),
        ];
        for (part, status) in parts {
            let Some(status) = status else {
                continue;
            };
            let name = bardo.text(Text::AnalysisPartName(part)).into_owned();
            let device = |gpu: bool| {
                bardo
                    .text(if gpu {
                        Text::AnalysisOnGpu
                    } else {
                        Text::AnalysisOnCpu
                    })
                    .into_owned()
            };
            let (text, button, running) = match status {
                AnalysisStatus::NotStarted => (
                    bardo.text_with(Text::AnalysisNotStarted, &[("part", &name)]),
                    Some(Text::AnalyzeTake),
                    false,
                ),
                AnalysisStatus::Queued => (
                    bardo.text_with(Text::AnalysisQueued, &[("part", &name)]),
                    None,
                    true,
                ),
                AnalysisStatus::Running {
                    progress,
                    gpu,
                    left,
                } => {
                    let percent = progress.percent().to_string();
                    let device = device(gpu.unwrap_or(true));
                    let text = match left {
                        Some(left) => bardo.text_with(
                            Text::AnalysisRunningLeft,
                            &[
                                ("part", &name),
                                ("percent", &percent),
                                ("device", &device),
                                ("left", &rough(*left)),
                            ],
                        ),
                        None => bardo.text_with(
                            Text::AnalysisRunning,
                            &[("part", &name), ("percent", &percent), ("device", &device)],
                        ),
                    };
                    (text, None, true)
                }
                AnalysisStatus::Cancelled => (
                    bardo.text_with(Text::AnalysisCancelled, &[("part", &name)]),
                    Some(Text::ResumeAnalysis),
                    false,
                ),
                AnalysisStatus::Failed { kind } => (
                    format!(
                        "{} {}",
                        bardo.text_with(Text::AnalysisFailed, &[("part", &name)]),
                        bardo.text(Text::JobFailureKindName(*kind))
                    ),
                    Some(Text::AnalyzeTake),
                    false,
                ),
                AnalysisStatus::Done { device: done } => (
                    bardo.text_with(
                        Text::AnalysisDone,
                        &[("part", &name), ("device", &device(*done == Device::Gpu))],
                    ),
                    None,
                    false,
                ),
            };
            let line = h_flex()
                .gap_2()
                .items_center()
                .flex_wrap()
                .when(running, |line| line.child(Spinner::new().xsmall()))
                .child(div().text_xs().child(SharedString::from(text)))
                .when_some(button, |line, label| {
                    line.child(
                        Button::new(("take-analyze", index * 2 + part as usize))
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, label))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.analyze_take(index, window, cx)
                            })),
                    )
                })
                .when(index == 0 && part == AnalysisPart::Face, |line| {
                    line.child(guide::info(
                        bardo,
                        "capture-analysis-info",
                        tr(bardo, Text::AnalysisHint),
                        guide::refs::CAPTURE_ANALYSIS,
                    ))
                });
            lines = lines.child(line);
        }
        if view.redone && !matches!(view.face, AnalysisStatus::Done { .. }) {
            lines = lines.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(tr(bardo, Text::AnalysisRedone)),
            );
        }
        if let Some(checks) = view.checks {
            lines = lines.child(self.render_checks(&checks, cx));
        }
        (mode, lines.into_any_element())
    }

    /// A take's control video: where its build is, the set in Depth mode,
    /// what leaves the computer, its player and Approve.
    fn render_take_control(
        &self,
        index: usize,
        view: &ControlVideoView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let set = (view.mode == TakeMode::Depth).then(|| self.render_take_set(index, view, cx));
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let small = |text: SharedString| div().text_xs().child(text);
        let mut section = v_flex().gap_1().pt_1().child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_medium()
                        .child(tr(bardo, Text::ControlTitle)),
                )
                .when(index == 0, |row| {
                    row.child(guide::info(
                        bardo,
                        "capture-control-info",
                        tr(bardo, Text::ControlHint),
                        guide::refs::CAPTURE_CONTROL,
                    ))
                }),
        );

        if let Some(set) = set {
            section = section.child(set);
        }

        // Where the build is, with what starts or stops it.
        let (text, button, running): (String, Option<Text>, bool) = match &view.status {
            ControlStatus::Waiting => (bardo.text(Text::ControlWaiting).into_owned(), None, false),
            ControlStatus::NotBuilt => (
                bardo.text(Text::ControlNotBuilt).into_owned(),
                Some(Text::ControlBuild),
                false,
            ),
            ControlStatus::Queued => (bardo.text(Text::ControlQueued).into_owned(), None, true),
            ControlStatus::Building { progress } => (
                bardo.text_with(
                    Text::ControlBuilding,
                    &[("percent", &progress.percent().to_string())],
                ),
                None,
                true,
            ),
            ControlStatus::Cancelled => (
                bardo.text(Text::ControlCancelled).into_owned(),
                Some(Text::ControlBuildAgain),
                false,
            ),
            ControlStatus::Failed { kind } => (
                format!(
                    "{} {}",
                    bardo.text(Text::ControlFailed),
                    bardo.text(Text::JobFailureKindName(*kind))
                ),
                Some(Text::ControlBuildAgain),
                false,
            ),
            ControlStatus::Ready { .. } => {
                let built = view.built.as_ref();
                let megabytes = built.map_or(0.0, |b| b.bytes as f64 / 1_000_000.0);
                let size =
                    format!("{megabytes:.1}").replace('.', &bardo.text(Text::DecimalSeparator));
                (
                    bardo.text_with(Text::ControlReady, &[("size", &size)]),
                    None,
                    false,
                )
            }
        };
        let job = view.job;
        section = section.child(
            h_flex()
                .gap_2()
                .items_center()
                .flex_wrap()
                .when(running, |line| line.child(Spinner::new().xsmall()))
                .child(small(SharedString::from(text)))
                .when_some(button, |line, label| {
                    line.child(
                        Button::new(("control-build", index))
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, label))
                            .disabled(self.take_busy(index))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.build_control(index, window, cx)
                            })),
                    )
                })
                .when_some(job.filter(|_| running), |line, job| {
                    line.child(
                        Button::new(("control-stop", index))
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, Text::ControlStopBuild))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.stop_control(job, cx)
                            })),
                    )
                }),
        );

        // What leaves the computer with it.
        section =
            section.child(small(tr(bardo, Text::ControlOutbound(view.outbound))).text_color(muted));
        if view.outbound.sends_set_frame() {
            section = section.child(small(tr(bardo, Text::ControlSetFrame)).text_color(muted));
        }
        if view.approval_lost && !view.is_approved() {
            section = section.child(kit::notice(
                Tone::Warning,
                tr(bardo, Text::ControlApprovalLost),
                cx,
            ));
        }
        if let Some(built) = view.built.as_ref().filter(|b| b.uncovered > 0) {
            section = section.child(kit::notice(
                Tone::Danger,
                SharedString::from(bardo.text_with(
                    Text::ControlUncovered,
                    &[
                        ("n", &built.uncovered.to_string()),
                        ("frames", &built.frames.to_string()),
                    ],
                )),
                cx,
            ));
        }
        // The uncovered frames say why above.
        let blocked = match view.block {
            Some(ApprovalBlock::SeveralFaces) => Some(Text::ControlSeveralFacesBlocked),
            Some(ApprovalBlock::Length(_)) => Some(Text::ControlLengthBlocked),
            Some(ApprovalBlock::Uncovered) | None => None,
        };
        if let Some(blocked) = blocked {
            section = section.child(kit::notice(Tone::Danger, tr(bardo, blocked), cx));
        }

        if let ControlStatus::Ready { approved } = view.status {
            let watching = self
                .capture
                .watching
                .as_ref()
                .filter(|watching| watching.take == index && watching.step.is_none());
            if let Some(watching) = watching {
                section = section.child(render_player(bardo, watching, muted));
            }
            let playing = watching.is_some_and(|watching| watching.player.is_playing());
            let actions = h_flex()
                .gap_2()
                .items_center()
                .flex_wrap()
                .child(
                    Button::new(("control-play", index))
                        .outline()
                        .xsmall()
                        .label(tr(
                            bardo,
                            if playing {
                                Text::ControlStopPlaying
                            } else {
                                Text::ControlPlay
                            },
                        ))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.toggle_control(index, window, cx)
                        })),
                )
                .map(|row| {
                    if approved {
                        row.child(kit::status(
                            Tone::Success,
                            tr(bardo, Text::ControlApproved),
                            cx,
                        ))
                    } else {
                        row.child(
                            Button::new(("control-approve", index))
                                .primary()
                                .xsmall()
                                .label(tr(bardo, Text::ControlApprove))
                                .disabled(!view.can_approve())
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.approve_control(index, window, cx)
                                })),
                        )
                    }
                })
                .when(index == 0, |row| {
                    row.child(guide::info(
                        bardo,
                        "capture-review-info",
                        tr(bardo, Text::ControlReviewHint),
                        guide::refs::CAPTURE_REVIEW,
                    ))
                });
            section = section.child(actions);
        }
        section.into_any_element()
    }

    /// A take's performed clip: its take model, its prompt, and its draft
    /// and final with what makes, reviews and uses them.
    fn render_take_performed(
        &self,
        index: usize,
        performed: &PerformedView,
        view: &PerformedTakeView,
        frame: Option<Text>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let small = |text: SharedString| div().text_xs().child(text);
        let busy = view.is_busy();
        let blocked = view.block.is_some();
        // The provider the take model runs on, as the texts name it.
        let provider = view.model.as_ref().map(|model| model.id.provider());
        let provider_name = provider
            .map(|provider| bardo.text(Text::ProviderName(provider)).into_owned())
            .unwrap_or_default();
        let with_provider =
            |text: Text| SharedString::from(bardo.text_with(text, &[("provider", &provider_name)]));
        let info = |id: &'static str, hint: Text, at| {
            (index == 0).then(|| guide::info(bardo, id, tr(bardo, hint), at))
        };
        let mut section = v_flex().gap_1().pt_1().child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_medium()
                        .child(tr(bardo, Text::PerformedTitle)),
                )
                .children(info(
                    "capture-performed-info",
                    Text::PerformedHint,
                    guide::refs::CAPTURE_PERFORMED,
                )),
        );

        // Its take model, the channel's marked.
        let channel = performed.channel_model.as_ref().map(|model| &model.id);
        let mut models = h_flex()
            .gap_x_3()
            .gap_y_1()
            .flex_wrap()
            .items_center()
            .child(small(tr(bardo, Text::TakeModelLabel)).text_color(muted));
        for (number, model) in performed.models.iter().enumerate() {
            let title = crate::channels::take_model_title(bardo, model);
            let label = if Some(&model.id) == channel {
                bardo.text_with(Text::TakeModelChannelTag, &[("model", &title)])
            } else {
                title
            };
            let picked = view.model.as_ref().is_some_and(|own| own.id == model.id);
            let id = model.id.clone();
            models = models.child(
                Radio::new(SharedString::from(format!("take-model-{index}-{number}")))
                    .label(SharedString::from(label))
                    .xsmall()
                    .checked(picked)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, checked: &bool, window, cx| {
                        if *checked {
                            this.set_take_model(index, Some(id.clone()), window, cx);
                        }
                    })),
            );
        }
        section = section.child(models);
        if let Some(block) = view.block {
            section = section.child(kit::notice(
                Tone::Warning,
                tr(bardo, Text::PerformedBlocked(block)),
                cx,
            ));
        }
        if let Some(preflight) = view.preflight.as_ref().filter(|_| !blocked) {
            section = section.child(self.render_preflight(
                index,
                view,
                preflight,
                performed.look_face,
                cx,
            ));
        }

        // Its prompt; a blocked take with none, and nothing under way, has
        // nothing to show yet.
        if blocked && !busy && view.prompt.is_none() && self.capture.editing_prompt != Some(index) {
            return section.into_any_element();
        }
        section = section.child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_medium()
                        .child(tr(bardo, Text::TakePromptLabel)),
                )
                .children(info(
                    "capture-take-prompt-info",
                    Text::TakePromptHint,
                    guide::refs::CAPTURE_TAKE_PROMPT,
                )),
        );
        let writing = view
            .prompt_job
            .as_ref()
            .filter(|job| job.state().is_active());
        if self.capture.editing_prompt == Some(index) {
            section = section
                .child(Textarea::new(&self.take_prompt_editor))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(("save-take-prompt", index))
                                .primary()
                                .xsmall()
                                .label(tr(bardo, Text::SaveTakePrompt))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.save_take_prompt(window, cx)
                                })),
                        )
                        .child(
                            Button::new(("cancel-take-prompt", index))
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::CancelTakePrompt))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.capture.editing_prompt = None;
                                    cx.notify();
                                })),
                        ),
                );
        } else if let Some(prompt) = &view.prompt {
            let text = prompt.text.clone();
            section = section
                .child(small(SharedString::from(prompt.text.clone())))
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .when(prompt.edited, |row| {
                            row.child(kit::status(
                                Tone::Info,
                                tr(bardo, Text::TakePromptEdited),
                                cx,
                            ))
                        })
                        .child(
                            Button::new(("edit-take-prompt", index))
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::EditTakePrompt))
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.edit_take_prompt(index, text.clone(), window, cx)
                                })),
                        )
                        .child(
                            Button::new(("rewrite-take-prompt", index))
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::WriteTakePromptAgain))
                                .disabled(busy || blocked)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.performed_step(
                                        index,
                                        PerformedStep::Prompt,
                                        BudgetConsent::Ask,
                                        window,
                                        cx,
                                    )
                                })),
                        ),
                );
        } else if writing.is_none() {
            section = section.child(
                h_flex().gap_2().items_center().child(
                    Button::new(("write-take-prompt", index))
                        .primary()
                        .xsmall()
                        .label(tr(bardo, Text::WriteTakePrompt))
                        .disabled(busy || blocked)
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.performed_step(
                                index,
                                PerformedStep::Prompt,
                                BudgetConsent::Ask,
                                window,
                                cx,
                            )
                        })),
                ),
            );
            if let Some(frame) = frame {
                section = section.child(small(tr(bardo, frame)).text_color(muted));
            }
        }
        match view.prompt_job.as_ref() {
            Some(job) if job.state().is_active() => {
                section = section.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Spinner::new().xsmall())
                        .child(small(tr(bardo, Text::TakePromptWriting))),
                );
            }
            Some(job) if job.state() == JobState::Failed => {
                let kind = job.failure().map(|failure| failure.kind);
                let mut text = bardo.text(Text::TakePromptFailed).into_owned();
                if let Some(kind) = kind {
                    text = format!("{text} {}", bardo.text(Text::JobFailureKindName(kind)));
                }
                section = section.child(kit::notice(Tone::Danger, SharedString::from(text), cx));
            }
            _ => {}
        }

        // Its draft and final.
        let check_again = || {
            Button::new(("check-take", index))
                .outline()
                .xsmall()
                .label(tr(bardo, Text::CheckTakeAgain))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.check_take_again(index, window, cx)
                }))
        };
        let job = view.job.as_ref();
        match job {
            Some((job, step)) if job.state().is_active() => {
                let text = match (job.state(), step) {
                    (JobState::Queued, _) => Text::TakeQueued,
                    (_, TakeStep::Draft) => Text::TakeMakingDraft,
                    (_, TakeStep::Final) => Text::TakeMakingFinal,
                    (_, TakeStep::Test) => Text::TakeMakingTest,
                };
                let id = job.id();
                section = section.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(Spinner::new().xsmall())
                        .child(small(with_provider(text)))
                        .child(
                            Button::new(("stop-take", index))
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::StopTakeWaiting))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.stop_control(id, cx)
                                })),
                        ),
                );
            }
            Some((job, _)) if job.state() == JobState::Cancelled && view.pending => {
                section = section
                    .child(kit::notice(
                        Tone::Info,
                        with_provider(Text::TakeStoppedWaiting),
                        cx,
                    ))
                    .child(h_flex().child(check_again()));
            }
            Some((job, _)) if job.state() == JobState::Failed => {
                let failure = job.failure();
                if let Some(failure) =
                    failure.filter(|failure| failure.kind == JobFailureKind::Declined)
                {
                    section = section
                        .child(kit::notice(
                            Tone::Danger,
                            with_provider(Text::TakeRefused),
                            cx,
                        ))
                        // ModelArk says which rule and why; Higgsfield does
                        // not.
                        .when(failure.detail != bardo_app::TAKE_REFUSED, |section| {
                            section.child(
                                small(SharedString::from(failure.detail.clone())).text_color(muted),
                            )
                        });
                } else {
                    let reason = failure.map_or_else(String::new, |failure| {
                        bardo
                            .text(Text::JobFailureKindName(failure.kind))
                            .into_owned()
                    });
                    section = section
                        .child(kit::notice(
                            Tone::Danger,
                            SharedString::from(
                                bardo.text_with(Text::TakeFailed, &[("reason", &reason)]),
                            ),
                            cx,
                        ))
                        .children(failure.map(|failure| {
                            small(SharedString::from(failure.detail.clone())).text_color(muted)
                        }))
                        .when(view.pending, |section| {
                            section.child(h_flex().child(check_again()))
                        });
                }
            }
            _ => {}
        }

        let watching = |step: TakeStep| {
            self.capture
                .watching
                .as_ref()
                .filter(|watching| watching.take == index && watching.step == Some(step))
        };
        let play = |step: TakeStep, label: Text| {
            let playing = watching(step).is_some_and(|watching| watching.player.is_playing());
            Button::new(SharedString::from(format!(
                "play-take-{}-{index}",
                step.code()
            )))
            .outline()
            .xsmall()
            .label(tr(
                bardo,
                if playing {
                    Text::ControlStopPlaying
                } else {
                    label
                },
            ))
        };
        let length = view.length;
        let made_lines = |made: &TakeResult, ready: Option<Text>| {
            let mut lines = Vec::new();
            if let Some(ready) = ready {
                lines.push(
                    small(SharedString::from(bardo.text_with(
                        ready,
                        &[("seconds", &bardo.seconds(made.duration))],
                    )))
                    .into_any_element(),
                );
            }
            if let Some(note) = length_note(bardo, made.duration, length) {
                lines.push(
                    small(SharedString::from(note))
                        .text_color(muted)
                        .into_any_element(),
                );
            }
            lines
        };
        let can_make = !busy
            && !blocked
            && !view.pending
            && !performed.needs_test
            && !view.preflight_holds()
            && view.prompt.is_some()
            && self.capture.editing_prompt != Some(index);
        let make = |id: &'static str, label: Text, step: TakeStep, primary: bool| {
            let button = Button::new((id, index))
                .xsmall()
                .label(tr(bardo, label))
                .disabled(!can_make);
            let button = if primary {
                button.primary()
            } else {
                button.outline()
            };
            button.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.performed_step(
                    index,
                    PerformedStep::Make(step),
                    BudgetConsent::Ask,
                    window,
                    cx,
                )
            }))
        };

        if performed.needs_test && !blocked && view.prompt.is_some() {
            section = section.child(small(tr(bardo, Text::PerformedTestFirst)).text_color(muted));
        }
        if view.has_drafts() {
            match &view.draft {
                Some(draft) => {
                    let approved = draft.accepted_at.is_some();
                    section = section.children(made_lines(
                        draft,
                        Some(if approved {
                            Text::TakeDraftApproved
                        } else {
                            Text::TakeDraftReady
                        }),
                    ));
                    // A draft the final finishes has a deadline.
                    if view.expired {
                        section = section.child(kit::notice(
                            Tone::Warning,
                            tr(bardo, Text::PerformedDraftExpired),
                            cx,
                        ));
                    } else if view.finished {
                        section = section.child(
                            small(tr(bardo, Text::PerformedDraftFinished)).text_color(muted),
                        );
                    } else if let Some(deadline) = draft.deadline {
                        section = section.child(
                            small(SharedString::from(bardo.text_with(
                                Text::TakeDraftDeadline,
                                &[("when", &bardo.take_deadline_text(deadline))],
                            )))
                            .text_color(muted),
                        );
                    }
                    if let Some(watching) = watching(TakeStep::Draft) {
                        section = section.child(render_player(bardo, watching, muted));
                    }
                    let mut row = h_flex().gap_2().flex_wrap().items_center().child(
                        play(TakeStep::Draft, Text::PlayTakeDraft).on_click(cx.listener(
                            move |this, _: &ClickEvent, window, cx| {
                                this.toggle_take_result(index, TakeStep::Draft, window, cx)
                            },
                        )),
                    );
                    row = if approved {
                        row.child(
                            Button::new(("back-to-drafts", index))
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::BackToTakeDrafts))
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.performed_action(window, cx, |bardo, id| {
                                        bardo.back_to_take_drafts(id, index)
                                    });
                                })),
                        )
                        // Past its deadline, or finished, only a new draft
                        // goes on.
                        .when(view.expired || view.finished, |row| {
                            row.child(make(
                                "regenerate-draft",
                                Text::RegenerateTakeDraft,
                                TakeStep::Draft,
                                true,
                            ))
                        })
                    } else if view.expired {
                        row.child(make(
                            "regenerate-draft",
                            Text::RegenerateTakeDraft,
                            TakeStep::Draft,
                            true,
                        ))
                    } else {
                        row.child(
                            Button::new(("approve-draft", index))
                                .primary()
                                .xsmall()
                                .label(tr(bardo, Text::ApproveTakeDraft))
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.performed_action(window, cx, |bardo, id| {
                                        bardo.approve_take_draft(id, index)
                                    });
                                })),
                        )
                        .child(make(
                            "regenerate-draft",
                            Text::RegenerateTakeDraft,
                            TakeStep::Draft,
                            false,
                        ))
                    };
                    section = section.child(row);
                }
                None => {
                    let note = match view.model.as_ref() {
                        Some(model) if model.finishes_draft => Text::TakeDraftFinishNote,
                        Some(model) if model.draft_optional() => Text::TakeDraftOptionalNote,
                        _ => Text::TakeDraftNote,
                    };
                    section = section
                        .child(small(tr(bardo, note)).text_color(muted))
                        .when(view.prompt.is_some(), |section| {
                            section.child(h_flex().child(make(
                                "make-draft",
                                Text::MakeTakeDraft,
                                TakeStep::Draft,
                                true,
                            )))
                        });
                }
            }
        }

        // An optional draft can be skipped: the final straight away.
        if let Some(skip) = view.skip_draft.as_ref().filter(|_| view.prompt.is_some()) {
            section = section.child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(make(
                        "make-final-skip-draft",
                        Text::MakeTakeFinalSkipDraft,
                        TakeStep::Final,
                        false,
                    ))
                    .children(
                        estimate_note(bardo, skip, Text::TakeSpendSkipDraft, cx).filter(|_| !busy),
                    ),
            );
        }
        let final_due = view.next == PerformedStep::Make(TakeStep::Final);
        match &view.final_take {
            Some(made) => {
                section = section.children(made_lines(made, Some(Text::TakeFinalReady)));
                if let Some(watching) = watching(TakeStep::Final) {
                    section = section.child(render_player(bardo, watching, muted));
                }
                section = section.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(
                            play(TakeStep::Final, Text::PlayTakeFinal).on_click(cx.listener(
                                move |this, _: &ClickEvent, window, cx| {
                                    this.toggle_take_result(index, TakeStep::Final, window, cx)
                                },
                            )),
                        )
                        .child(
                            Button::new(("use-take", index))
                                .primary()
                                .xsmall()
                                .label(tr(bardo, Text::UsePerformedTake))
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.use_performed_take(index, window, cx)
                                })),
                        )
                        .when(final_due, |row| {
                            row.child(make(
                                "make-final-again",
                                Text::MakeTakeFinalAgain,
                                TakeStep::Final,
                                false,
                            ))
                        }),
                );
                if final_due {
                    section =
                        section.child(small(tr(bardo, Text::TakeFinalPaidNote)).text_color(muted));
                }
            }
            None if final_due && view.prompt.is_some() => {
                section = section
                    .child(h_flex().child(make(
                        "make-final",
                        Text::MakeTakeFinal,
                        TakeStep::Final,
                        true,
                    )))
                    .child(small(tr(bardo, Text::TakeFinalPaidNote)).text_color(muted));
            }
            None => {}
        }
        let estimate = view
            .estimate
            .as_ref()
            .filter(|_| !busy)
            .and_then(|estimate| estimate_note(bardo, estimate, Text::EstimateCost, cx));
        section = section.child(
            h_flex()
                .gap_1()
                .items_start()
                .children(estimate)
                .children(info(
                    "capture-draft-final-info",
                    Text::TakeMakeHint,
                    // What ModelArk and Kling change has its own section.
                    match view.model.as_ref() {
                        Some(model) if model.finishes_draft => guide::refs::CAPTURE_BYTEPLUS,
                        Some(model) if model.id.provider() == Provider::Kling => {
                            guide::refs::CAPTURE_KLING
                        }
                        _ => guide::refs::CAPTURE_DRAFT_FINAL,
                    },
                )),
        );
        if let Some((_, step, ask)) =
            self.capture
                .performed_ask
                .as_ref()
                .filter(|(take, step, _)| {
                    *take == index && *step != PerformedStep::Make(TakeStep::Test)
                })
        {
            section = section.child(self.render_performed_ask(index, *step, ask, cx));
        }
        if self.capture.used == Some(index) {
            section = section.child(kit::notice(
                Tone::Success,
                tr(bardo, Text::PerformedUsedNow),
                cx,
            ));
        } else if view.used > 0 {
            section = section.child(
                small(SharedString::from(bardo.text_with(
                    Text::PerformedUsed,
                    &[("n", &view.used.to_string())],
                )))
                .text_color(muted),
            );
        }
        if performed.realistic && view.final_take.is_some() {
            section =
                section.child(small(tr(bardo, Text::PerformedRealisticNote)).text_color(muted));
        }
        section.into_any_element()
    }

    /// Asks before a paid take that would reach a budget or the video's
    /// cap: "make it anyway" sends it with the user's word.
    fn render_performed_ask(
        &self,
        index: usize,
        step: PerformedStep,
        ask: &PerformedAsk,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let confirm = cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.performed_step(index, step, BudgetConsent::Confirmed, window, cx)
        });
        let cancel = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.capture.performed_ask = None;
            cx.notify();
        });
        let id = format!("performed-{}-{index}", step_code(step));
        match ask {
            PerformedAsk::Budget(estimate) => {
                budget_question(SharedString::from(id), bardo, estimate, cx, confirm, cancel)
            }
            PerformedAsk::Cap(spend, estimate) => kit::card(cx)
                .p_3()
                .gap_2()
                .border_color(cx.theme().danger)
                .child(
                    kit::notice(Tone::Danger, tr(bardo, Text::TakeCapReachedTitle), cx)
                        .font_semibold(),
                )
                .child(div().text_sm().child(spend_line(bardo, spend)))
                // A budget it also reaches is asked about in the same card.
                .children(
                    estimate
                        .providers
                        .iter()
                        .filter(|provider| provider.level == Some(BudgetLevel::Reached))
                        .map(|provider| div().text_sm().child(over_line(bardo, provider))),
                )
                .child(div().text_sm().child(tr(bardo, Text::TakeCapQuestion)))
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new(SharedString::from(format!("{id}-confirm")))
                                .danger()
                                .small()
                                .label(tr(bardo, Text::TakeCapConfirm))
                                .on_click(confirm),
                        )
                        .child(
                            Button::new(SharedString::from(format!("{id}-cancel")))
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::BudgetCancel))
                                .on_click(cancel),
                        ),
                )
                .into_any_element(),
        }
    }

    /// A take's preflight checks: each one's outcome, why a failed one
    /// failed, and Send anyway while one holds the take back.
    fn render_preflight(
        &self,
        index: usize,
        view: &PerformedTakeView,
        preflight: &Preflight,
        look_face: LookFace,
        cx: &Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let muted = cx.theme().muted_foreground;
        let small = |text: SharedString| div().text_xs().child(text);
        let mut pills = h_flex().gap_1().flex_wrap().items_center();
        for result in &preflight.results {
            let tone = match result.outcome {
                PreflightOutcome::Passed => Tone::Success,
                PreflightOutcome::Failed => Tone::Danger,
                PreflightOutcome::Unknown => Tone::Neutral,
            };
            pills = pills.child(kit::status(
                tone,
                tr(bardo, Text::PreflightCheckName(result.check)),
                cx,
            ));
        }
        let mut section = v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .child(tr(bardo, Text::PreflightTitle)),
                    )
                    .when(index == 0, |row| {
                        row.child(guide::info(
                            bardo,
                            "capture-preflight-info",
                            tr(bardo, Text::PreflightHint),
                            guide::refs::CAPTURE_PREFLIGHT,
                        ))
                    }),
            )
            .child(pills);
        for check in preflight.failed() {
            let line = match (check, preflight.capture, preflight.look) {
                (PreflightCheck::Framing, Some(capture), Some(look)) => {
                    SharedString::from(bardo.text_with(
                        Text::PreflightFramingFailed,
                        &[
                            ("capture", &bardo.text(Text::BodyFramingName(capture))),
                            ("look", &bardo.text(Text::BodyFramingName(look))),
                        ],
                    ))
                }
                (PreflightCheck::Framing, ..) if look_face == LookFace::NoFace => {
                    tr(bardo, Text::PreflightLookNoFace)
                }
                (check, ..) => tr(bardo, Text::PreflightCheckFailed(check)),
            };
            section = section.child(small(line).text_color(muted));
        }
        let read = |check| preflight.outcome(check) == Some(PreflightOutcome::Unknown);
        // Analyzed, but before the measures the checks read existed.
        if read(PreflightCheck::Motion) && !read(PreflightCheck::OnePerson) {
            section = section.child(small(tr(bardo, Text::PreflightReanalyze)).text_color(muted));
        }
        if read(PreflightCheck::Framing) && look_face == LookFace::Unmeasured {
            section =
                section.child(small(tr(bardo, Text::PreflightLookMeasuring)).text_color(muted));
        }
        if view.preflight_holds() {
            section = section
                .child(kit::notice(
                    Tone::Warning,
                    tr(bardo, Text::PreflightFailedNote),
                    cx,
                ))
                .child(
                    h_flex().child(
                        Button::new(("send-take-anyway", index))
                            .outline()
                            .xsmall()
                            .label(tr(bardo, Text::SendTakeAnyway))
                            .disabled(view.is_busy())
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.performed_action(window, cx, |bardo, id| {
                                    bardo.send_take_anyway(id, index)
                                });
                            })),
                    ),
                );
        } else if preflight.holds() {
            section = section.child(kit::status(
                Tone::Warning,
                tr(bardo, Text::PreflightOverridden),
                cx,
            ));
        } else if preflight
            .results
            .iter()
            .all(|result| result.outcome == PreflightOutcome::Passed)
        {
            section = section.child(small(tr(bardo, Text::PreflightPassed)).text_color(muted));
        }
        section.into_any_element()
    }

    /// What the video's performed takes cost so far and still need, and
    /// its cap, which the user may change.
    fn render_takes_spend(&self, performed: &PerformedView, cx: &Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let muted = cx.theme().muted_foreground;
        let spend = &performed.spend;
        let cap = spend.cap;
        let mut card = kit::card(cx)
            .p_3()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(kit::section_heading(tr(bardo, Text::TakeSpendTitle)))
                    .child(guide::info(
                        bardo,
                        "capture-spend-info",
                        tr(bardo, Text::TakeSpendHint),
                        guide::refs::CAPTURE_SPEND,
                    )),
            )
            .child(div().text_sm().child(spend_line(bardo, spend)));
        if spend.partial {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(tr(bardo, Text::TakeSpendPartial)),
            );
        }
        if spend.projected() > cap {
            card = card.child(kit::notice(
                Tone::Warning,
                tr(bardo, Text::TakeSpendOver),
                cx,
            ));
        }
        if self.capture.editing_cap {
            card = card.child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(tr(bardo, Text::TakeCapLabel)),
                    )
                    .child(
                        div()
                            .w(px(120.))
                            .child(Input::new(&self.take_cap_input).small()),
                    )
                    .child(
                        Button::new("save-take-cap")
                            .primary()
                            .xsmall()
                            .label(tr(bardo, Text::SaveTakeCap))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.save_take_cap(None, window, cx)
                            })),
                    )
                    .child(
                        Button::new("cancel-take-cap")
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, Text::BudgetCancel))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.capture.editing_cap = false;
                                this.capture.cap_note = None;
                                cx.notify();
                            })),
                    ),
            );
        } else {
            card = card.child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        Button::new("edit-take-cap")
                            .ghost()
                            .xsmall()
                            .label(tr(bardo, Text::EditTakeCap))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.edit_take_cap(cap, window, cx)
                            })),
                    )
                    .when(performed.own_cap, |row| {
                        row.child(
                            Button::new("reset-take-cap")
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::ResetTakeCap))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.save_take_cap(Some(String::new()), window, cx)
                                })),
                        )
                    }),
            );
        }
        if let Some((tone, note)) = self.capture.cap_note {
            card = card.child(kit::notice(tone, tr(bardo, note), cx));
        }
        card.into_any_element()
    }

    /// The capture's test take: a few seconds of one take, to approve
    /// before any draft or final is paid for.
    fn render_take_test(&self, performed: &PerformedView, cx: &Context<Self>) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let muted = cx.theme().muted_foreground;
        let small = |text: SharedString| div().text_xs().child(text);
        let mut card = kit::card(cx).p_3().gap_1().child(
            h_flex()
                .gap_1()
                .items_center()
                .child(kit::section_heading(tr(bardo, Text::TestTakeTitle)))
                .child(guide::info(
                    bardo,
                    "capture-test-info",
                    tr(bardo, Text::TestTakeHint),
                    guide::refs::CAPTURE_TEST,
                )),
        );
        let take = performed.test_take;
        let model = take
            .and_then(|index| performed.takes.get(index))
            .and_then(|view| view.model.as_ref());
        let provider_name = model
            .map(|model| {
                bardo
                    .text(Text::ProviderName(model.id.provider()))
                    .into_owned()
            })
            .unwrap_or_default();
        let busy = performed.test_busy();
        let job: Option<&Job> = performed.test_job.as_ref();
        match job {
            Some(job) if job.state().is_active() => {
                let text = if job.state() == JobState::Queued {
                    bardo.text(Text::TakeQueued).into_owned()
                } else {
                    bardo.text_with(Text::TakeMakingTest, &[("provider", &provider_name)])
                };
                let id = job.id();
                card = card.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(Spinner::new().xsmall())
                        .child(small(SharedString::from(text)))
                        .child(
                            Button::new("stop-take-test")
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::StopTakeWaiting))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.stop_control(id, cx)
                                })),
                        ),
                );
            }
            Some(job)
                if matches!(job.state(), JobState::Failed | JobState::Cancelled)
                    && performed.test_pending =>
            {
                let reason = job.failure().map_or_else(String::new, |failure| {
                    bardo
                        .text(Text::JobFailureKindName(failure.kind))
                        .into_owned()
                });
                card = card
                    .child(kit::notice(
                        Tone::Info,
                        SharedString::from(if job.state() == JobState::Failed {
                            bardo.text_with(Text::TakeFailed, &[("reason", &reason)])
                        } else {
                            bardo.text_with(
                                Text::TakeStoppedWaiting,
                                &[("provider", &provider_name)],
                            )
                        }),
                        cx,
                    ))
                    .child(
                        h_flex().child(
                            Button::new("check-take-test")
                                .outline()
                                .xsmall()
                                .label(tr(bardo, Text::CheckTakeAgain))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.performed_action(window, cx, |bardo, id| {
                                        bardo.check_test_again(id)
                                    });
                                })),
                        ),
                    );
            }
            Some(job) if job.state() == JobState::Failed => {
                let failure = job.failure();
                let text = match failure {
                    Some(failure) if failure.kind == JobFailureKind::Declined => {
                        bardo.text_with(Text::TakeRefused, &[("provider", &provider_name)])
                    }
                    _ => {
                        let reason = failure.map_or_else(String::new, |failure| {
                            bardo
                                .text(Text::JobFailureKindName(failure.kind))
                                .into_owned()
                        });
                        bardo.text_with(Text::TakeFailed, &[("reason", &reason)])
                    }
                };
                card = card
                    .child(kit::notice(Tone::Danger, SharedString::from(text), cx))
                    .children(
                        failure
                            .filter(|failure| failure.detail != bardo_app::TAKE_REFUSED)
                            .map(|failure| {
                                small(SharedString::from(failure.detail.clone())).text_color(muted)
                            }),
                    );
            }
            _ => {}
        }
        let can_make = !busy
            && !performed.test_pending
            && take.is_some_and(|index| {
                performed
                    .takes
                    .get(index)
                    .is_some_and(|view| !view.is_busy() && !view.preflight_holds())
            })
            && self.capture.editing_prompt.is_none();
        let make = |label: SharedString, primary: bool| {
            let index = take.unwrap_or_default();
            let button = Button::new("make-take-test")
                .xsmall()
                .label(label)
                .disabled(!can_make);
            let button = if primary {
                button.primary()
            } else {
                button.outline()
            };
            button.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.performed_step(
                    index,
                    PerformedStep::Make(TakeStep::Test),
                    BudgetConsent::Ask,
                    window,
                    cx,
                )
            }))
        };
        match &performed.test {
            Some(test) => {
                let approved = test.accepted_at.is_some();
                let position = test.position as usize;
                card = card.child(small(SharedString::from(bardo.text_with(
                    Text::TestTakeReady,
                    &[("seconds", &bardo.seconds(test.duration))],
                ))));
                if approved {
                    card = card.child(kit::status(
                        Tone::Success,
                        tr(bardo, Text::TestTakeApproved),
                        cx,
                    ));
                }
                if let Some(watching) = self.capture.watching.as_ref().filter(|watching| {
                    watching.take == position && watching.step == Some(TakeStep::Test)
                }) {
                    card = card.child(render_player(bardo, watching, muted));
                }
                let playing = self.capture.watching.as_ref().is_some_and(|watching| {
                    watching.take == position
                        && watching.step == Some(TakeStep::Test)
                        && watching.player.is_playing()
                });
                let again = take.map(|_| make(tr(bardo, Text::MakeTestTakeAgain), false));
                card = card.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(
                            Button::new("play-take-test")
                                .outline()
                                .xsmall()
                                .label(tr(
                                    bardo,
                                    if playing {
                                        Text::ControlStopPlaying
                                    } else {
                                        Text::PlayTestTake
                                    },
                                ))
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.toggle_take_result(position, TakeStep::Test, window, cx)
                                })),
                        )
                        .when(!approved, |row| {
                            row.child(
                                Button::new("approve-take-test")
                                    .primary()
                                    .xsmall()
                                    .label(tr(bardo, Text::ApproveTestTake))
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.performed_action(window, cx, |bardo, id| {
                                            bardo.approve_take_test(id)
                                        });
                                    })),
                            )
                        })
                        .child(
                            Button::new("discard-take-test")
                                .ghost()
                                .xsmall()
                                .label(tr(bardo, Text::DiscardTestTake))
                                .disabled(busy)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    let discarded =
                                        this.performed_action(window, cx, |bardo, id| {
                                            bardo.discard_take_test(id)
                                        });
                                    let watching_test =
                                        this.capture.watching.as_ref().is_some_and(|watching| {
                                            watching.step == Some(TakeStep::Test)
                                        });
                                    if discarded && watching_test {
                                        this.close_control(usize::MAX, window, cx);
                                    }
                                })),
                        )
                        .children(again),
                );
            }
            None => match take {
                Some(index) => {
                    card = card.child(h_flex().child(make(
                        SharedString::from(
                            bardo.text_with(Text::MakeTestTake, &[("n", &(index + 1).to_string())]),
                        ),
                        true,
                    )));
                }
                None => {
                    card = card.child(small(tr(bardo, Text::TestTakeNone)).text_color(muted));
                }
            },
        }
        if let Some(estimate) = performed.test_estimate.as_ref().filter(|_| !busy) {
            card = card.children(estimate_note(bardo, estimate, Text::TestTakeCost, cx));
        }
        if take
            .and_then(|index| performed.takes.get(index))
            .is_some_and(PerformedTakeView::preflight_holds)
        {
            card = card.child(small(tr(bardo, Text::PreflightFailedNote)).text_color(muted));
        }
        if performed.needs_test {
            card = card.child(small(tr(bardo, Text::TestTakeNeeded)).text_color(muted));
        }
        if let Some((index, step, ask)) = self
            .capture
            .performed_ask
            .as_ref()
            .filter(|(_, step, _)| *step == PerformedStep::Make(TakeStep::Test))
        {
            card = card.child(self.render_performed_ask(*index, *step, ask, cx));
        }
        card.into_any_element()
    }

    /// A Depth take's set: the room, a scene image or a picture.
    fn render_take_set(
        &self,
        index: usize,
        view: &ControlVideoView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let muted = cx.theme().muted_foreground;
        let picked = match &view.set {
            TakeSet::Room => 0,
            TakeSet::Image {
                from: SetSource::Scene,
                ..
            } => 1,
            TakeSet::Image {
                from: SetSource::Imported,
                ..
            } => 2,
        };
        let choosing = self.capture.choosing_set.is_some();
        let take_busy = self.take_busy(index);
        let choices = ButtonGroup::new(("take-set", index))
            .outline()
            .xsmall()
            .child(
                Button::new("room")
                    .label(tr(bardo, Text::SetRoom))
                    .selected(picked == 0)
                    .disabled(take_busy && picked != 0),
            )
            .child(
                Button::new("scene")
                    .label(tr(bardo, Text::SetSceneImage))
                    .selected(picked == 1)
                    .disabled(take_busy),
            )
            .child(
                Button::new("picture")
                    .label(tr(bardo, Text::SetImport))
                    .selected(picked == 2)
                    .disabled(choosing || take_busy),
            )
            .on_click(cx.listener(move |this, clicked: &Vec<usize>, window, cx| {
                match clicked.first() {
                    Some(0) => this.set_take_set(index, SetChoice::Room, window, cx),
                    Some(1) => {
                        let open = this
                            .capture
                            .scene_picker
                            .as_ref()
                            .is_some_and(|(take, _)| *take == index);
                        this.capture.scene_picker = if open {
                            None
                        } else {
                            let bardo = this.bardo.read(cx);
                            let scenes = this
                                .project
                                .and_then(|project| bardo.set_scenes(project).ok())
                                .unwrap_or_default();
                            Some((index, scenes))
                        };
                        cx.notify();
                    }
                    Some(2) => this.choose_set_picture(index, cx),
                    _ => {}
                }
            }));
        let mut set = v_flex().gap_1().child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(tr(bardo, Text::SetLabel)),
                )
                .child(choices)
                .when(index == 0, |row| {
                    row.child(guide::info(
                        bardo,
                        "capture-set-info",
                        tr(bardo, Text::SetHint),
                        guide::refs::CAPTURE_SET,
                    ))
                }),
        );
        if let Some((_, scenes)) = self
            .capture
            .scene_picker
            .as_ref()
            .filter(|(take, _)| *take == index)
        {
            if scenes.is_empty() {
                set = set.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(tr(bardo, Text::SetNoScenes)),
                );
            } else {
                let mut row = h_flex().gap_1().flex_wrap();
                for scene in scenes {
                    let file = scene.file.clone();
                    row = row.child(
                        Button::new(("set-scene", scene.number))
                            .ghost()
                            .xsmall()
                            .label(SharedString::from(bardo.text_with(
                                Text::SetSceneNumber,
                                &[("n", &scene.number.to_string())],
                            )))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.set_take_set(index, SetChoice::Scene(file.clone()), window, cx)
                            })),
                    );
                }
                set = set.child(row);
            }
        }
        set.into_any_element()
    }

    /// What a take's analysis found, worst first.
    fn render_checks(&self, checks: &TakeChecks, cx: &App) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let frames = checks.frames.to_string();
        let line = |text: Text, n: u32| {
            bardo.text_with(text, &[("n", &n.to_string()), ("frames", &frames)])
        };
        let mut found = v_flex().gap_1();
        if checks.several_faces > 0 {
            found = found.child(kit::notice(
                Tone::Danger,
                SharedString::from(line(Text::CheckSeveralFaces, checks.several_faces)),
                cx,
            ));
        }
        let warnings = [
            (Text::CheckNoFace, checks.no_face),
            (Text::CheckSmallFace, checks.small_face),
            (Text::CheckTurned, checks.turned),
            (Text::CheckMaskJumps, checks.mask_jumps),
        ];
        let mut any = checks.several_faces > 0;
        for (text, n) in warnings {
            if n > 0 {
                any = true;
                found = found.child(kit::notice(
                    Tone::Warning,
                    SharedString::from(line(text, n)),
                    cx,
                ));
            }
        }
        if !any {
            found = found.child(kit::status(
                Tone::Success,
                tr(bardo, Text::ChecksPassed),
                cx,
            ));
        }
        found.into_any_element()
    }
}
