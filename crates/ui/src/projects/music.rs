//! The music panel of the projects screen: Claude writes a prompt for the
//! video's music; the user edits it, then has ElevenLabs Music compose a
//! track from it (reviewed here: play, use or discard), or copies it into
//! their own music tool and imports the track in the editor's Media tab.
//! Generating the prompt again replaces it, edits included.

use bardo_app::bardo_domain::{JobState, MediaAssetId, TemplateKind, VideoProjectId};
use bardo_app::{
    BudgetConsent, Control, GeneratedTrack, MusicPromptError, MusicTrackError, Text, TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, ClipboardItem, SharedString, Window, div};

use super::{ProjectsScreen, PromptShown, clock, muted};
use crate::guide;
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
        match self.bardo.read(cx).music_tracks(id) {
            Ok(view) => {
                // A track discarded or replaced is not played any more.
                let shown = |asset: MediaAssetId| {
                    [&view.in_use, &view.in_review]
                        .into_iter()
                        .flatten()
                        .any(|track| track.asset.id == asset)
                };
                if self
                    .track_player
                    .as_ref()
                    .is_some_and(|player| !shown(player.asset().id))
                {
                    self.track_player = None;
                }
                self.tracks = Some(view);
            }
            Err(error) => {
                self.tracks = None;
                self.track_player = None;
                self.track_error = Some(error.message());
            }
        }
    }

    fn track_running(&self) -> bool {
        self.tracks
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .is_some_and(|job| job.state().is_active())
    }

    fn generate_track(
        &mut self,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.track_ask = None;
        self.track_notice = None;
        let Some(id) = self.project else {
            return;
        };
        self.track_error = match self.bardo.read(cx).generate_music(id, consent) {
            Ok(_) => None,
            Err(MusicTrackError::OverBudget(estimate)) => {
                self.track_ask = Some(estimate);
                None
            }
            Err(error) => Some(error.message()),
        };
        self.load(window, cx);
        cx.notify();
    }

    /// Plays track `asset` when paused, pauses it when playing; another
    /// track playing stops.
    fn toggle_track(&mut self, asset: MediaAssetId, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        if self
            .track_player
            .as_ref()
            .is_some_and(|player| player.asset().id != asset)
        {
            self.track_player = None;
        }
        if self.track_player.is_none() {
            match self.bardo.read(cx).play_music_track(id, asset) {
                Ok(player) => self.track_player = Some(player),
                Err(error) => {
                    self.track_error = Some(error.message());
                    cx.notify();
                    return;
                }
            }
        }
        if let Some(player) = self.track_player.as_mut() {
            self.track_error = player.toggle().err().map(|error| error.message());
        }
        cx.notify();
    }

    fn use_track(&mut self, asset: MediaAssetId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        match self.bardo.read(cx).use_music_track(id, asset) {
            Ok(()) => {
                self.track_error = None;
                self.track_notice = Some(Text::MusicTrackUsed);
            }
            Err(error) => {
                self.track_error = Some(error.message());
                self.track_notice = None;
            }
        }
        self.load(window, cx);
        cx.notify();
    }

    fn discard_track(&mut self, asset: MediaAssetId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        if self
            .track_player
            .as_ref()
            .is_some_and(|player| player.asset().id == asset)
        {
            self.track_player = None;
        }
        match self.bardo.read(cx).discard_music_track(id, asset) {
            Ok(()) => {
                self.track_error = None;
                self.track_notice = Some(Text::MusicTrackDiscarded);
            }
            Err(error) => {
                self.track_error = Some(error.message());
                self.track_notice = None;
            }
        }
        self.load(window, cx);
        cx.notify();
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
        let tracks = self.render_tracks(cx);
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
                .child(guide::info(
                    bardo,
                    "music-prompt-info",
                    hint,
                    guide::refs::SCRIPT_MUSIC,
                ))
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
                .children(tracks)
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

    /// The music made from the prompt: the track in use, the one waiting
    /// for review, and the button that composes a new one.
    fn render_tracks(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.tracks.as_ref()?;
        let job = self.render_track_job(cx);
        let in_use = view
            .in_use
            .as_ref()
            .map(|track| self.render_track(track, false, cx));
        let in_review = view
            .in_review
            .as_ref()
            .map(|track| self.render_track(track, true, cx));
        let bardo = self.bardo.read(cx);
        let any = view.in_use.is_some() || view.in_review.is_some();
        let generate = match view.length {
            None => muted(cx, tr(bardo, Text::MusicTrackNoNarration)).into_any_element(),
            Some(length) => {
                let hint =
                    bardo.text_with(Text::MusicTrackGenerateHint, &[("length", &clock(length))]);
                let hint: SharedString = if any {
                    format!("{hint} {}", bardo.text(Text::MusicTrackRegenerateHint)).into()
                } else {
                    hint.into()
                };
                let button = Button::new("generate-music-track")
                    .small()
                    .label(tr(
                        bardo,
                        if any {
                            Text::MusicTrackRegenerate
                        } else {
                            Text::MusicTrackGenerate
                        },
                    ))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.generate_track(BudgetConsent::Ask, window, cx)
                    }));
                let button = if any {
                    button.outline()
                } else {
                    button.primary()
                };
                v_flex()
                    .gap_1()
                    .children((!self.track_running()).then(|| {
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(button)
                            .child(guide::info(
                                bardo,
                                "music-track-info",
                                hint,
                                guide::refs::SCRIPT_MUSIC_TRACK,
                            ))
                    }))
                    .children(view.estimate.as_ref().and_then(|estimate| {
                        estimate_note(bardo, estimate, Text::EstimateCost, cx)
                    }))
                    .into_any_element()
            }
        };
        let ask = self.track_ask.as_ref().map(|estimate| {
            budget_question(
                "music-track-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.generate_track(BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.track_ask = None;
                    cx.notify();
                }),
            )
        });
        let section = v_flex()
            .id("projects-music-track")
            .pt_2()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .child(tr(bardo, Text::MusicTrackHeading)),
            )
            .children(
                self.track_notice
                    .map(|notice| kit::notice(Tone::Success, tr(bardo, notice), cx)),
            )
            .children(
                self.track_error
                    .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
            )
            .children(job)
            .children(ask)
            .when(!any, |section| {
                section.child(muted(cx, tr(bardo, Text::MusicTrackEmpty)))
            })
            .children(in_review)
            .children(in_use)
            .child(generate)
            .into_any_element();
        Some(
            kit::anchor_in(
                TourAnchor::Control(Control::MusicTrack),
                section,
                Some(&self.page_scroll),
            )
            .into_any_element(),
        )
    }

    /// One track: its status, name, length and model, and what can be done
    /// with it.
    fn render_track(
        &self,
        track: &GeneratedTrack,
        in_review: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bardo = self.bardo.read(cx);
        let theme = cx.theme();
        let asset = track.asset.id;
        let player = self
            .track_player
            .as_ref()
            .filter(|player| player.asset().id == asset);
        let playing = player.is_some_and(|player| player.is_playing());
        let position = player.map_or(std::time::Duration::ZERO, |player| player.position());
        let details = bardo.text_with(
            Text::MusicTrackDetails,
            &[
                ("length", &clock(track.asset.duration)),
                ("model", &track.track.model),
            ],
        );
        let (tone, label) = if in_review {
            (Tone::Info, Text::MusicTrackNew)
        } else {
            (Tone::Success, Text::MusicTrackInUse)
        };
        let play = Button::new(("toggle-music-track", usize::from(in_review)))
            .outline()
            .small()
            .label(tr(
                bardo,
                if playing {
                    Text::MusicTrackPause
                } else {
                    Text::MusicTrackPlay
                },
            ))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_track(asset, cx)));
        let actions = h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .child(play)
            .when(player.is_some(), |row| {
                row.child(div().text_sm().child(SharedString::from(format!(
                    "{} / {}",
                    clock(position),
                    clock(track.asset.duration)
                ))))
            })
            .when(in_review, |row| {
                row.child(
                    Button::new("use-music-track")
                        .primary()
                        .small()
                        .label(tr(bardo, Text::MusicTrackUse))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.use_track(asset, window, cx)
                        })),
                )
                .child(guide::info(
                    bardo,
                    "music-track-use-info",
                    tr(bardo, Text::MusicTrackUseHint),
                    guide::refs::SCRIPT_MUSIC_TRACK,
                ))
                .child(
                    Button::new("discard-music-track")
                        .ghost()
                        .small()
                        .label(tr(bardo, Text::MusicTrackDiscard))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.discard_track(asset, window, cx)
                        })),
                )
            });
        v_flex()
            .p_2()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(kit::status(tone, tr(bardo, label), cx))
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(track.asset.name.clone()),
                    )
                    .child(muted(cx, details.into())),
            )
            .child(actions)
            .into_any_element()
    }

    /// A track being composed, or why the last one stopped.
    fn render_track_job(&self, cx: &App) -> Option<AnyElement> {
        let job = self.tracks.as_ref()?.job.as_ref()?;
        let bardo = self.bardo.read(cx);
        match job.state() {
            JobState::Queued | JobState::Running => Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(tr(bardo, Text::MusicTrackRunning)))
                    .into_any_element(),
            ),
            JobState::Failed => {
                let failure = job.failure()?;
                Some(
                    v_flex()
                        .gap_1()
                        .child(kit::notice(
                            Tone::Danger,
                            tr(bardo, Text::MusicTrackStopped),
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
                                    "music-track-failure-details",
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
