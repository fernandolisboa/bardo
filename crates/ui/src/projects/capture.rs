//! Captures (#189): importing a recording of the user reading the script
//! on camera from the Narration stage, and the Capture stage that shows
//! its takes. The file is read off the UI thread before anything is paid;
//! the import card asks whose voice the speech takes and for the user's
//! word that the recording shows only them. The takes and their cuts come
//! from `bardo_app`; this view lays them out the same in every layout.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bardo_app::bardo_domain::{
    AnalysisPart, ApprovalBlock, BoundaryMove, Capture, CaptureVoice, Device, Narration, SetSource,
    TakeChecks, TakeFlag, TakeMode, TakeSet,
};
use bardo_app::{
    AnalysisStatus, BudgetConsent, CaptureError, CaptureVoiceChoice, ChosenCapture, Control,
    ControlPlayer, ControlStatus, ControlVideoView, SetChoice, SpendEstimate, TakeAnalysisView,
    Text, TourAnchor, take_source,
};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
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
use crate::spend::{budget_question, estimate_note};

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
    /// The take whose scene images are offered as its set.
    pub(super) scene_picker: Option<usize>,
    /// Choosing a set picture; dropping it stops waiting.
    pub(super) choosing_set: Option<Task<()>>,
}

/// A take's control video in its player, with the picture showing.
pub(super) struct Watching {
    pub(super) take: usize,
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
        }
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
            let _ = this.update_in(cx, |this, window, cx| {
                this.capture.choosing_set = None;
                match path {
                    Some(path) => this.set_take_set(index, SetChoice::Picture(path), window, cx),
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
            .filter(|watching| watching.take == index)
        {
            watching.player.toggle();
            cx.notify();
            return;
        }
        self.close_control(usize::MAX, window, cx);
        match self.bardo.read(cx).control_player(id, index) {
            Ok(mut player) => {
                player.play();
                self.capture.watching = Some(Watching {
                    take: index,
                    player,
                    frame: None,
                });
            }
            Err(error) => self.capture.error = Some(error.message()),
        }
        cx.notify();
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

    /// Moves the control video player on and swaps in a new picture when
    /// one is due. A player whose take was built again since closes.
    pub(super) fn advance_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(watching) = self.capture.watching.as_ref() else {
            return;
        };
        let current = self
            .narration
            .as_ref()
            .and_then(|view| view.controls.get(watching.take))
            .and_then(|view| view.built.as_ref())
            .is_some_and(|built| {
                self.bardo.read(cx).control_video_path(built) == watching.player.path()
            });
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
                                    ("length", &format!("{:.1} s", take.length().as_secs_f64())),
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
                    row.child(anchored(TourAnchor::Control(Control::CaptureMode), mode))
                        .child(anchored(
                            TourAnchor::Control(Control::CaptureAnalysis),
                            lines,
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
            list = list.child(row);
            if index + 1 < takes.len() {
                let can = |to| !busy && capture.takes.boundary_target(&source, index, to).is_ok();
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
                list = list.child(if index == 0 {
                    kit::anchor_in(
                        TourAnchor::Control(Control::CaptureCut),
                        cut,
                        Some(&self.page_scroll),
                    )
                    .into_any_element()
                } else {
                    cut.into_any_element()
                });
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
        let modes = ButtonGroup::new(("take-mode", index))
            .outline()
            .xsmall()
            .children(TakeMode::ALL.map(|mode| {
                Button::new(mode.code())
                    .label(tr(bardo, Text::TakeModeName(mode)))
                    .selected(view.mode == mode)
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
                let length = self
                    .narration
                    .as_ref()
                    .and_then(|n| n.capture.as_ref())
                    .and_then(|c| c.takes.as_slice().get(index))
                    .map_or(Duration::ZERO, |take| take.length());
                (
                    bardo.text_with(
                        Text::ControlReady,
                        &[
                            ("length", &format!("{:.1} s", length.as_secs_f64())),
                            ("size", &format!("{megabytes:.1}")),
                        ],
                    ),
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
                Tone::Warning,
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
        if let Some(ApprovalBlock::Length(_)) = view.block {
            section = section.child(kit::notice(
                Tone::Danger,
                tr(bardo, Text::ControlLengthBlocked),
                cx,
            ));
        }

        if let ControlStatus::Ready { approved } = view.status {
            let watching = self
                .capture
                .watching
                .as_ref()
                .filter(|watching| watching.take == index);
            if let Some(watching) = watching {
                let size = watching.player.size();
                // Upright or wide, at most 180 px high.
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
                section = section.child(picture);
                if let Some(error) = watching.player.error() {
                    section = section.child(small(tr(bardo, *error)).text_color(muted));
                }
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
                        tr(bardo, Text::ControlHint),
                        guide::refs::CAPTURE_REVIEW,
                    ))
                });
            section = section.child(actions);
        }
        section.into_any_element()
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
        let choices = ButtonGroup::new(("take-set", index))
            .outline()
            .xsmall()
            .child(
                Button::new("room")
                    .label(tr(bardo, Text::SetRoom))
                    .selected(picked == 0),
            )
            .child(
                Button::new("scene")
                    .label(tr(bardo, Text::SetSceneImage))
                    .selected(picked == 1),
            )
            .child(
                Button::new("picture")
                    .label(tr(bardo, Text::SetImport))
                    .selected(picked == 2)
                    .disabled(choosing),
            )
            .on_click(cx.listener(move |this, clicked: &Vec<usize>, window, cx| {
                match clicked.first() {
                    Some(0) => this.set_take_set(index, SetChoice::Room, window, cx),
                    Some(1) => {
                        this.capture.scene_picker =
                            (this.capture.scene_picker != Some(index)).then_some(index);
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
        if self.capture.scene_picker == Some(index) {
            let scenes = self
                .project
                .and_then(|project| bardo.set_scenes(project).ok())
                .unwrap_or_default();
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
