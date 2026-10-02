//! The music prompt panel of the projects screen: Claude writes a prompt
//! for the user's music tool; the user edits it, copies it and imports the
//! track the tool makes in the editor's Media tab. Generating again
//! replaces the prompt, edits included.

use bardo_app::bardo_domain::{JobState, TemplateKind, VideoProjectId};
use bardo_app::{BudgetConsent, MusicPromptError, Text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, ClipboardItem, SharedString, Window, div};

use super::{ProjectsScreen, PromptShown, muted};
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

impl ProjectsScreen {
    pub(super) fn load_music(
        &mut self,
        id: VideoProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.bardo.read(cx).music_prompt(id) {
            Ok(view) => {
                // Refilled only when the stored text changes, so typing stays.
                let stored = view.prompt.as_ref().map(|prompt| prompt.text().to_owned());
                if stored != self.music_loaded {
                    let text = stored.clone().unwrap_or_default();
                    self.music_editor
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                    self.music_loaded = stored;
                }
                self.music = Some(view);
            }
            Err(error) => {
                self.music = None;
                self.music_error = Some(error.message());
            }
        }
    }

    fn music_running(&self) -> bool {
        self.music
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .is_some_and(|job| job.state().is_active())
    }

    fn generate_music(
        &mut self,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.music_ask = None;
        self.music_notice = None;
        let Some(id) = self.project else {
            return;
        };
        self.music_error = match self.bardo.read(cx).generate_music_prompt(id, consent) {
            Ok(_) => None,
            Err(MusicPromptError::OverBudget(estimate)) => {
                self.music_ask = Some(estimate);
                None
            }
            Err(error) => Some(error.message()),
        };
        self.load(window, cx);
        cx.notify();
    }

    fn save_music(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        let text = self.music_editor.read(cx).value();
        match self.bardo.read(cx).edit_music_prompt(id, &text) {
            Ok(prompt) => {
                self.music_error = None;
                self.music_notice = Some(Text::MusicPromptSaved);
                self.music_loaded = Some(prompt.text().to_owned());
            }
            Err(error) => {
                self.music_error = Some(error.message());
                self.music_notice = None;
            }
        }
        self.load(window, cx);
        cx.notify();
    }

    fn revert_music(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.music_loaded.clone().unwrap_or_default();
        self.music_editor
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.music_error = None;
        self.music_notice = None;
        cx.notify();
    }

    /// Copies what the field holds: the prompt as the user sees it.
    fn copy_music(&mut self, cx: &mut Context<Self>) {
        let text = self.music_editor.read(cx).value().to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.music_notice = Some(Text::MusicPromptCopied);
        cx.notify();
    }

    pub(super) fn render_music(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.music.as_ref()?;
        let job = self.render_music_job(cx);
        let provenance = view.prompt.as_ref().map(|prompt| {
            self.render_provenance(
                prompt.generation(),
                TemplateKind::MusicPrompt,
                PromptShown::MusicPrompt,
                cx,
            )
        });
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let running = self.music_running();
        let prompt = view.prompt.as_ref();

        let title_row = h_flex()
            .gap_2()
            .items_center()
            .child(kit::section_heading(tr(bardo, Text::MusicPromptTitle)))
            .when(prompt.is_some_and(|prompt| prompt.is_edited()), |row| {
                row.child(kit::status(
                    Tone::Info,
                    tr(bardo, Text::MusicPromptEdited),
                    cx,
                ))
            });
        let hint: SharedString = match prompt {
            None => bardo
                .text_with(
                    Text::MusicPromptGenerateHint,
                    &[("n", &view.template.number.to_string())],
                )
                .into(),
            Some(_) => format!(
                "{} {}",
                bardo.text(Text::MusicPromptRegenerateHint),
                bardo.text_with(
                    Text::MusicPromptGenerateHint,
                    &[("n", &view.template.number.to_string())],
                )
            )
            .into(),
        };
        let generate = Button::new("generate-music-prompt")
            .small()
            .label(tr(
                bardo,
                if prompt.is_some() {
                    Text::MusicPromptRegenerate
                } else {
                    Text::MusicPromptGenerate
                },
            ))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.generate_music(BudgetConsent::Ask, window, cx)
            }));
        let generate = if prompt.is_some() {
            generate.outline()
        } else {
            generate.primary()
        };
        // A running generation shows its spinner instead of the button.
        let generate = (!running).then(|| {
            h_flex()
                .gap_1()
                .items_center()
                .child(generate)
                .child(kit::info("music-prompt-info", None, hint))
        });
        let body: AnyElement = match prompt {
            None => v_flex()
                .gap_2()
                .child(muted(cx, tr(bardo, Text::MusicPromptEmpty)))
                .children(estimate_note(bardo, &view.estimate, Text::EstimateCost, cx))
                .children(generate)
                .into_any_element(),
            Some(_) => v_flex()
                .gap_2()
                .child(Textarea::new(&self.music_editor))
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .items_center()
                        .child(
                            Button::new("copy-music-prompt")
                                .primary()
                                .small()
                                .label(tr(bardo, Text::MusicPromptCopy))
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.copy_music(cx)),
                                ),
                        )
                        .child(
                            Button::new("save-music-prompt")
                                .outline()
                                .small()
                                .label(tr(bardo, Text::MusicPromptSave))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.save_music(window, cx)
                                })),
                        )
                        .child(
                            Button::new("revert-music-prompt")
                                .ghost()
                                .small()
                                .label(tr(bardo, Text::MusicPromptRevert))
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.revert_music(window, cx)
                                })),
                        )
                        .child(div().flex_1())
                        .children(generate),
                )
                .children(estimate_note(bardo, &view.estimate, Text::EstimateCost, cx))
                .into_any_element(),
        };
        let ask = self.music_ask.as_ref().map(|estimate| {
            budget_question(
                "music-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.generate_music(BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.music_ask = None;
                    cx.notify();
                }),
            )
        });

        Some(
            v_flex()
                .id("projects-music")
                .pt_3()
                .gap_2()
                .border_t_1()
                .border_color(theme.border)
                .child(title_row)
                .children(
                    self.music_notice
                        .map(|notice| kit::notice(Tone::Success, tr(bardo, notice), cx)),
                )
                .children(
                    self.music_error
                        .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
                )
                .children(job)
                .children(ask)
                .child(body)
                .children(provenance)
                .into_any_element(),
        )
    }

    /// A running generation, or why the last one stopped.
    fn render_music_job(&self, cx: &App) -> Option<AnyElement> {
        let job = self.music.as_ref()?.job.as_ref()?;
        let bardo = self.bardo.read(cx);
        match job.state() {
            JobState::Queued | JobState::Running => Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(tr(bardo, Text::MusicPromptRunning)))
                    .into_any_element(),
            ),
            JobState::Failed => {
                let failure = job.failure()?;
                Some(
                    v_flex()
                        .gap_1()
                        .child(kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::MusicPromptStopped),
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
                                    "music-failure-details",
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
}
