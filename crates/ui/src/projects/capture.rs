//! Captures (#189): importing a recording of the user reading the script
//! on camera from the Narration stage, and the Capture stage that shows
//! its takes. The file is read off the UI thread before anything is paid;
//! the import card asks whose voice the speech takes and for the user's
//! word that the recording shows only them. The takes and their cuts come
//! from `bardo_app`; this view lays them out the same in every layout.

use bardo_app::bardo_domain::{BoundaryMove, Capture, CaptureVoice, Narration, TakeFlag};
use bardo_app::{
    BudgetConsent, CaptureError, CaptureVoiceChoice, ChosenCapture, Control, SpendEstimate, Text,
    TourAnchor, take_source,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::radio::Radio;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, PathPromptOptions, SharedString, Task, Window, div};

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
        }
    }
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
        for (index, take) in takes.iter().enumerate() {
            let said = match (
                words.get(take.words.start),
                words.get(take.words.end.saturating_sub(1)),
            ) {
                (Some(first), Some(last)) => text[first.text.start..last.text.end].to_owned(),
                _ => String::new(),
            };
            let (tone, state) = match take.flag() {
                None => (Tone::Success, Text::CaptureTakeOk),
                Some(TakeFlag::TooLong) => (Tone::Warning, Text::CaptureTakeTooLong),
                Some(TakeFlag::TooShort) => (Tone::Warning, Text::CaptureTakeTooShort),
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
                        .child(kit::status(
                            tone,
                            tr(
                                bardo,
                                if take.flag().is_none() {
                                    state
                                } else {
                                    Text::CaptureTakeFlagged
                                },
                            ),
                            cx,
                        )),
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
}
