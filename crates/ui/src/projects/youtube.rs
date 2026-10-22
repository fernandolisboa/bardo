//! The YouTube extras of the Publish stage (#182): a long video's
//! thumbnail and chapters, in the YouTube inspector. Claude writes the
//! thumbnail's text and background prompt; the user picks a background
//! (a scene image or one drawn from the prompt), where the text sits and
//! how it looks, and saves the picture, made off the UI thread. Claude
//! splits the narration into chapters; the user renames, removes or moves
//! them and decides whether they go in the description.

use bardo_app::bardo_domain::{
    BackgroundSource, JobState, Publication, TextPlacement, ThumbnailBackground, ThumbnailLook,
    ThumbnailOutcome, VideoProjectId, timestamp,
};
use bardo_app::{
    Bardo, BudgetConsent, ChaptersError, Control, Text, ThumbnailEdit, ThumbnailError, TourAnchor,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, ObjectFit, SharedString, Window, div, img, px};

use super::{ProjectsScreen, muted};
use crate::guide;
use crate::icons::Lucide;
use crate::kit::{self, Tone};
use crate::shell::tr;
use crate::spend::{budget_question, estimate_note};

/// How sending the thumbnail with a YouTube upload went, under the post's
/// state; nothing before it was sent.
pub(super) fn thumbnail_outcome(
    bardo: &Bardo,
    publication: &Publication,
    cx: &App,
) -> Option<AnyElement> {
    let outcome = publication.upload()?.thumbnail.as_ref()?;
    let notice = match outcome {
        ThumbnailOutcome::Set => {
            kit::notice(Tone::Success, tr(bardo, Text::UploadThumbnailSet), cx)
        }
        ThumbnailOutcome::NotAllowed => kit::notice(
            Tone::Warning,
            tr(bardo, Text::UploadThumbnailNotAllowed),
            cx,
        ),
        ThumbnailOutcome::Failed(reason) => kit::notice(
            Tone::Warning,
            bardo.text_with(Text::UploadThumbnailFailed, &[("reason", reason)]),
            cx,
        ),
    };
    Some(notice.into_any_element())
}

/// What a budget question on the thumbnail card would start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThumbnailCall {
    Write,
    Draw,
}

impl ProjectsScreen {
    /// Reads the thumbnail and chapters cards. The text fields are refilled
    /// only when what is stored changes, so typing stays.
    pub(super) fn load_youtube(
        &mut self,
        id: VideoProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.bardo.read(cx).thumbnail(id) {
            Ok(view) => {
                let stored = (
                    view.thumbnail.design.text.clone(),
                    view.thumbnail.prompt.clone(),
                );
                if self.thumbnail_loaded.as_ref() != Some(&stored) {
                    let (text, prompt) = stored.clone();
                    self.thumbnail_text
                        .update(cx, |input, cx| input.set_value(text, window, cx));
                    self.thumbnail_prompt
                        .update(cx, |input, cx| input.set_value(prompt, window, cx));
                    self.thumbnail_loaded = Some(stored);
                }
                self.thumbnail = Some(view);
            }
            Err(error) => {
                self.thumbnail = None;
                self.thumbnail_error = Some(error.message());
            }
        }
        match self.bardo.read(cx).chapters(id) {
            Ok(view) => {
                if self
                    .chapter_editing
                    .is_some_and(|index| index >= view.rows.len())
                {
                    self.chapter_editing = None;
                }
                self.chapters = Some(view);
            }
            Err(error) => {
                self.chapters = None;
                self.chapters_error = Some(error.message());
            }
        }
    }

    fn thumbnail_typed(&self, cx: &App) -> (String, String) {
        (
            self.thumbnail_text.read(cx).value().to_string(),
            self.thumbnail_prompt.read(cx).value().to_string(),
        )
    }

    fn thumbnail_dirty(&self, cx: &App) -> bool {
        self.thumbnail_loaded
            .as_ref()
            .is_some_and(|loaded| *loaded != self.thumbnail_typed(cx))
    }

    fn thumbnail_call(
        &mut self,
        call: ThumbnailCall,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.thumbnail_ask = None;
        self.thumbnail_notice = None;
        let Some(id) = self.project else {
            return;
        };
        let started = {
            let bardo = self.bardo.read(cx);
            match call {
                ThumbnailCall::Write => bardo.write_thumbnail(id, consent),
                ThumbnailCall::Draw => bardo.draw_thumbnail_background(id, consent),
            }
        };
        self.thumbnail_error = match started {
            Ok(_) => None,
            Err(ThumbnailError::OverBudget(estimate)) => {
                self.thumbnail_ask = Some((call, estimate));
                None
            }
            Err(error) => Some(error.message()),
        };
        self.load(window, cx);
        cx.notify();
    }

    /// Saves the thumbnail with one change applied to what is stored; the
    /// typed text and prompt are saved with "Apply".
    fn change_thumbnail(
        &mut self,
        change: impl FnOnce(&mut ThumbnailEdit),
        typed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(id), Some(view)) = (self.project, self.thumbnail.as_ref()) else {
            return;
        };
        let mut edit = ThumbnailEdit::of(&view.thumbnail);
        if typed {
            (edit.text, edit.prompt) = self.thumbnail_typed(cx);
        }
        change(&mut edit);
        match self.bardo.read(cx).edit_thumbnail(id, edit) {
            Ok(_) => {
                self.thumbnail_error = None;
                self.thumbnail_notice = None;
                if typed {
                    // Read back as stored (trimmed): the fields follow.
                    self.thumbnail_loaded = None;
                }
            }
            Err(error) => {
                self.thumbnail_error = Some(error.message());
                self.thumbnail_notice = None;
            }
        }
        self.load(window, cx);
        cx.notify();
    }

    fn revert_thumbnail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((text, prompt)) = self.thumbnail_loaded.clone() {
            self.thumbnail_text
                .update(cx, |input, cx| input.set_value(text, window, cx));
            self.thumbnail_prompt
                .update(cx, |input, cx| input.set_value(prompt, window, cx));
        }
        self.thumbnail_error = None;
        cx.notify();
    }

    /// Makes the picture off the UI thread, then records it.
    fn save_thumbnail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.project else {
            return;
        };
        let composer = match self.bardo.read(cx).thumbnail_composer(id) {
            Ok(composer) => composer,
            Err(error) => {
                self.thumbnail_error = Some(error.message());
                cx.notify();
                return;
            }
        };
        self.thumbnail_error = None;
        self.thumbnail_notice = None;
        self.thumbnail_saving = Some(cx.spawn_in(window, async move |this, cx| {
            let made = cx
                .background_executor()
                .spawn(async move { composer.run() })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.thumbnail_saving = None;
                if this.project != Some(id) {
                    return;
                }
                let saved =
                    made.and_then(|design| this.bardo.read(cx).thumbnail_composed(id, &design));
                match saved {
                    Ok(_) => this.thumbnail_notice = Some(Text::ThumbnailSaved),
                    Err(error) => this.thumbnail_error = Some(error.message()),
                }
                this.load(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn chapters_running(&self) -> bool {
        self.chapters
            .as_ref()
            .and_then(|view| view.job.as_ref())
            .is_some_and(|job| job.state().is_active())
    }

    fn write_chapters(
        &mut self,
        consent: BudgetConsent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.chapters_ask = None;
        let Some(id) = self.project else {
            return;
        };
        self.chapters_error = match self.bardo.read(cx).write_chapters(id, consent) {
            Ok(_) => None,
            Err(ChaptersError::OverBudget(estimate)) => {
                self.chapters_ask = Some(estimate);
                None
            }
            Err(error) => Some(error.message()),
        };
        self.chapter_editing = None;
        self.load(window, cx);
        cx.notify();
    }

    /// Runs a change to the chapters and reads them again.
    fn change_chapters(
        &mut self,
        change: impl FnOnce(&bardo_app::Bardo, VideoProjectId) -> Result<(), ChaptersError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.project else {
            return;
        };
        self.chapters_error = change(self.bardo.read(cx), id)
            .err()
            .map(|error| error.message());
        self.load(window, cx);
        cx.notify();
    }

    fn edit_chapter_title(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let title = self
            .chapters
            .as_ref()
            .and_then(|view| view.rows.get(index))
            .map(|row| row.title.clone())
            .unwrap_or_default();
        self.chapter_title
            .update(cx, |input, cx| input.set_value(title, window, cx));
        self.chapter_editing = Some(index);
        self.chapters_error = None;
        cx.notify();
    }

    fn save_chapter_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.chapter_editing else {
            return;
        };
        let title = self.chapter_title.read(cx).value().to_string();
        let Some(id) = self.project else {
            return;
        };
        match self.bardo.read(cx).rename_chapter(id, index, &title) {
            Ok(()) => {
                self.chapter_editing = None;
                self.chapters_error = None;
            }
            Err(error) => self.chapters_error = Some(error.message()),
        }
        self.load(window, cx);
        cx.notify();
    }

    /// A job's line on a card: a spinner while it runs, what stopped it
    /// when it failed.
    fn youtube_job(
        &self,
        job: Option<&bardo_app::bardo_domain::Job>,
        running: Text,
        stopped: Text,
        id: &'static str,
        cx: &App,
    ) -> Option<AnyElement> {
        let job = job?;
        let bardo = self.bardo.read(cx);
        match job.state() {
            JobState::Queued | JobState::Running => Some(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(div().text_sm().child(tr(bardo, running)))
                    .into_any_element(),
            ),
            JobState::Failed => {
                let failure = job.failure()?;
                Some(
                    v_flex()
                        .gap_1()
                        .child(kit::notice(Tone::Danger, tr(bardo, stopped), cx))
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .child(tr(bardo, Text::JobFailureKindName(failure.kind))),
                                )
                                .child(kit::details(
                                    id,
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

    /// The thumbnail card of a long video's YouTube post.
    pub(super) fn render_thumbnail(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.thumbnail.as_ref()?;
        let bardo = self.bardo.read(cx);
        let heading = h_flex()
            .gap_1()
            .items_center()
            .child(kit::section_heading(tr(bardo, Text::ThumbnailTitle)))
            .child(guide::info(
                bardo,
                "thumbnail-info",
                tr(bardo, Text::ThumbnailHint),
                guide::refs::UPLOADING_THUMBNAIL,
            ));
        if !view.long {
            return Some(
                v_flex()
                    .gap_2()
                    .child(heading)
                    .child(muted(cx, tr(bardo, Text::ThumbnailShortNote)))
                    .into_any_element(),
            );
        }
        let thumbnail = &view.thumbnail;
        let running = view.is_busy();
        let saving = self.thumbnail_saving.is_some();
        let theme = cx.theme();

        // The picture as saved, or what keeps it from going.
        let state = match (&view.picture, view.is_outdated()) {
            (Some(_), _) => kit::status(Tone::Success, tr(bardo, Text::ThumbnailSaved), cx),
            (None, true) => kit::status(Tone::Warning, tr(bardo, Text::ThumbnailOutdated), cx),
            (None, false) => kit::status(Tone::Neutral, tr(bardo, Text::ThumbnailNotYetSaved), cx),
        };
        let preview = view.picture.clone().map(|path| {
            div()
                .w(px(320.))
                .h(px(180.))
                .rounded_md()
                .overflow_hidden()
                .border_1()
                .border_color(theme.border)
                .child(img(path).size_full().object_fit(ObjectFit::Cover))
        });

        let write = (!running).then(|| {
            let button = Button::new("write-thumbnail")
                .small()
                .label(tr(
                    bardo,
                    if thumbnail.generation.is_some() {
                        Text::ThumbnailRewrite
                    } else {
                        Text::ThumbnailWrite
                    },
                ))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.thumbnail_call(ThumbnailCall::Write, BudgetConsent::Ask, window, cx)
                }));
            let button = if thumbnail.generation.is_some() {
                button.outline()
            } else {
                button.primary()
            };
            h_flex()
                .gap_1()
                .items_center()
                .child(button)
                .child(guide::info(
                    bardo,
                    "thumbnail-write-info",
                    tr(bardo, Text::ThumbnailWriteHint),
                    guide::refs::UPLOADING_THUMBNAIL,
                ))
        });

        // Where the text sits: a 3 × 2 grid laid out like the frame.
        let placements = v_flex().gap_1().children(
            [
                TextPlacement::ALL[..3].to_vec(),
                TextPlacement::ALL[3..].to_vec(),
            ]
            .into_iter()
            .map(|row| {
                h_flex().gap_1().children(row.into_iter().map(|placement| {
                    let picked = thumbnail.design.placement == placement;
                    let button = Button::new(("thumbnail-placement", placement as usize))
                        .xsmall()
                        .label(tr(bardo, Text::TextPlacementName(placement)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.change_thumbnail(
                                |edit| edit.placement = placement,
                                false,
                                window,
                                cx,
                            )
                        }));
                    if picked {
                        button.primary()
                    } else {
                        button.ghost()
                    }
                }))
            }),
        );
        let looks = h_flex()
            .gap_1()
            .flex_wrap()
            .children(ThumbnailLook::ALL.into_iter().map(|look| {
                let picked = thumbnail.design.look == look;
                let button = Button::new(("thumbnail-look", look as usize))
                    .xsmall()
                    .label(tr(bardo, Text::ThumbnailLookName(look)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.change_thumbnail(|edit| edit.look = look, false, window, cx)
                    }));
                if picked {
                    button.primary()
                } else {
                    button.outline()
                }
            }));

        // The pictures it can sit on.
        let strip: AnyElement = if view.backgrounds.is_empty() {
            muted(cx, tr(bardo, Text::ThumbnailBackgroundsEmpty)).into_any_element()
        } else {
            h_flex()
                .gap_2()
                .flex_wrap()
                .children(view.backgrounds.iter().enumerate().map(|(index, choice)| {
                    let picked = thumbnail.design.background.as_ref() == Some(&choice.background);
                    let background: ThumbnailBackground = choice.background.clone();
                    let label = tr(
                        bardo,
                        match background.source {
                            BackgroundSource::Drawn => Text::ThumbnailBackgroundDrawn,
                            BackgroundSource::Scene => Text::ThumbnailBackgroundScene,
                        },
                    );
                    div()
                        .id(("thumbnail-background", index))
                        .w(px(96.))
                        .h(px(54.))
                        .rounded_sm()
                        .overflow_hidden()
                        .cursor_pointer()
                        .border_2()
                        .border_color(if picked { theme.primary } else { theme.border })
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(label.clone())
                                .build(window, cx)
                        })
                        .child(
                            img(choice.path.clone())
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            let background = background.clone();
                            this.change_thumbnail(
                                move |edit| edit.background = Some(background),
                                false,
                                window,
                                cx,
                            )
                        }))
                }))
                .into_any_element()
        };
        let draw = (!running).then(|| {
            Button::new("draw-thumbnail-background")
                .small()
                .outline()
                .label(tr(bardo, Text::ThumbnailDraw))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.thumbnail_call(ThumbnailCall::Draw, BudgetConsent::Ask, window, cx)
                }))
        });
        let dirty = self.thumbnail_dirty(cx);
        let edits = dirty.then(|| {
            h_flex()
                .gap_2()
                .child(
                    Button::new("apply-thumbnail")
                        .small()
                        .primary()
                        .label(tr(bardo, Text::ThumbnailApply))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.change_thumbnail(|_| {}, true, window, cx)
                        })),
                )
                .child(
                    Button::new("revert-thumbnail")
                        .small()
                        .ghost()
                        .label(tr(bardo, Text::MetadataRevert))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.revert_thumbnail(window, cx)
                        })),
                )
        });
        let save = h_flex()
            .gap_2()
            .items_center()
            .child(
                Button::new("save-thumbnail")
                    .small()
                    .primary()
                    .label(tr(bardo, Text::ThumbnailSave))
                    .disabled(
                        saving
                            || dirty
                            || thumbnail.design.background.is_none()
                            || view.picture.is_some(),
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.save_thumbnail(window, cx)
                    })),
            )
            .when(saving, |row| {
                row.child(Spinner::new().small())
                    .child(div().text_sm().child(tr(bardo, Text::ThumbnailSaving)))
            });
        let ask = self.thumbnail_ask.as_ref().map(|(call, estimate)| {
            let call = *call;
            budget_question(
                "thumbnail-budget",
                bardo,
                estimate,
                cx,
                cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.thumbnail_call(call, BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.thumbnail_ask = None;
                    cx.notify();
                }),
            )
        });
        let write_job = self.youtube_job(
            view.write_job.as_ref(),
            Text::ThumbnailWriting,
            Text::ThumbnailWriteStopped,
            "thumbnail-write-failure",
            cx,
        );
        let draw_job = self.youtube_job(
            view.draw_job.as_ref(),
            Text::ThumbnailDrawing,
            Text::ThumbnailDrawStopped,
            "thumbnail-draw-failure",
            cx,
        );
        let field = |label: Text, hint: Option<Text>, input: AnyElement| {
            v_flex()
                .gap_1()
                .child(div().text_xs().font_semibold().child(tr(bardo, label)))
                .child(input)
                .children(hint.map(|hint| muted(cx, tr(bardo, hint))))
        };

        Some(
            kit::anchor_in(
                TourAnchor::Control(Control::PublishThumbnail),
                v_flex()
                    .id("publish-thumbnail")
                    .gap_3()
                    .child(heading)
                    .child(h_flex().gap_2().items_center().flex_wrap().child(state))
                    .children(preview)
                    .children(
                        self.thumbnail_notice
                            .map(|notice| kit::notice(Tone::Success, tr(bardo, notice), cx)),
                    )
                    .children(
                        self.thumbnail_error
                            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
                    )
                    .children(write_job)
                    .children(draw_job)
                    .children(ask)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .flex_wrap()
                            .children(write)
                            .children(estimate_note(
                                bardo,
                                &view.write_estimate,
                                Text::EstimateCost,
                                cx,
                            )),
                    )
                    .child(field(
                        Text::ThumbnailText,
                        Some(Text::ThumbnailTextHint),
                        Input::new(&self.thumbnail_text).into_any_element(),
                    ))
                    .child(field(
                        Text::ThumbnailPlacement,
                        None,
                        placements.into_any_element(),
                    ))
                    .child(field(
                        Text::ThumbnailLookLabel,
                        None,
                        looks.into_any_element(),
                    ))
                    .child(field(
                        Text::ThumbnailBackground,
                        Some(Text::ThumbnailBackgroundHint),
                        strip,
                    ))
                    .child(field(
                        Text::ThumbnailPrompt,
                        Some(Text::ThumbnailPromptHint),
                        Textarea::new(&self.thumbnail_prompt).into_any_element(),
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .flex_wrap()
                            .children(draw)
                            .children(estimate_note(
                                bardo,
                                &view.draw_estimate,
                                Text::EstimateCost,
                                cx,
                            )),
                    )
                    .children(edits)
                    .child(save),
                Some(&self.publish_scroll),
            )
            .into_any_element(),
        )
    }

    /// The chapters card of a long video's YouTube post.
    pub(super) fn render_chapters(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.chapters.as_ref()?;
        let bardo = self.bardo.read(cx);
        let heading = h_flex()
            .gap_1()
            .items_center()
            .child(kit::section_heading(tr(bardo, Text::ChaptersTitle)))
            .child(guide::info(
                bardo,
                "chapters-info",
                tr(bardo, Text::ChaptersHint),
                guide::refs::UPLOADING_CHAPTERS,
            ))
            .when(view.edited, |row| {
                row.child(kit::status(Tone::Info, tr(bardo, Text::ChaptersEdited), cx))
            });
        if !view.long {
            return Some(
                v_flex()
                    .gap_2()
                    .child(heading)
                    .child(muted(cx, tr(bardo, Text::ChaptersShortNote)))
                    .into_any_element(),
            );
        }
        let running = self.chapters_running();
        let job = self.youtube_job(
            view.job.as_ref(),
            Text::ChaptersWriting,
            Text::ChaptersStopped,
            "chapters-failure",
            cx,
        );
        let write: Option<AnyElement> = if !view.has_narration {
            Some(muted(cx, tr(bardo, Text::ChaptersNoNarration)).into_any_element())
        } else {
            (!running).then(|| {
                let button = Button::new("write-chapters")
                    .small()
                    .label(tr(
                        bardo,
                        if view.has_chapters() {
                            Text::ChaptersRewrite
                        } else {
                            Text::ChaptersWrite
                        },
                    ))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.write_chapters(BudgetConsent::Ask, window, cx)
                    }));
                let button = if view.has_chapters() {
                    button.outline()
                } else {
                    button.primary()
                };
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(button)
                    .child(guide::info(
                        bardo,
                        "chapters-write-info",
                        tr(bardo, Text::ChaptersWriteHint),
                        guide::refs::UPLOADING_CHAPTERS,
                    ))
                    .children(view.estimate.as_ref().and_then(|estimate| {
                        estimate_note(bardo, estimate, Text::EstimateCost, cx)
                    }))
                    .into_any_element()
            })
        };
        let ask = self.chapters_ask.as_ref().map(|estimate| {
            budget_question(
                "chapters-budget",
                bardo,
                estimate,
                cx,
                cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.write_chapters(BudgetConsent::Confirmed, window, cx)
                }),
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.chapters_ask = None;
                    cx.notify();
                }),
            )
        });
        let rows = view.rows.iter().enumerate().map(|(index, row)| {
            let time: SharedString = match row.at {
                Some(at) => timestamp(at, view.length).into(),
                None => "–:––".into(),
            };
            let editing = self.chapter_editing == Some(index);
            let title: AnyElement = if editing {
                h_flex()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.chapter_title)),
                    )
                    .child(
                        Button::new(("save-chapter-title", index))
                            .xsmall()
                            .primary()
                            .icon(IconName::Check)
                            .tooltip(tr(bardo, Text::MetadataSave))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.save_chapter_title(window, cx)
                            })),
                    )
                    .child(
                        Button::new(("cancel-chapter-title", index))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Close)
                            .tooltip(tr(bardo, Text::MetadataRevert))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.chapter_editing = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .child(SharedString::from(row.title.clone()))
                    .into_any_element()
            };
            let actions = (!editing).then(|| {
                h_flex()
                    .gap_0p5()
                    .child(
                        Button::new(("rename-chapter", index))
                            .xsmall()
                            .ghost()
                            .icon(Lucide::Pencil)
                            .tooltip(tr(bardo, Text::ChapterRename))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.edit_chapter_title(index, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("chapter-earlier", index))
                            .xsmall()
                            .ghost()
                            .icon(IconName::ArrowUp)
                            .tooltip(tr(bardo, Text::ChapterEarlier))
                            .disabled(!row.can_move_earlier)
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.change_chapters(
                                    |bardo, id| bardo.move_chapter(id, index, false),
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new(("chapter-later", index))
                            .xsmall()
                            .ghost()
                            .icon(IconName::ArrowDown)
                            .tooltip(tr(bardo, Text::ChapterLater))
                            .disabled(!row.can_move_later)
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.change_chapters(
                                    |bardo, id| bardo.move_chapter(id, index, true),
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new(("remove-chapter", index))
                            .xsmall()
                            .ghost()
                            .icon(Lucide::Trash)
                            .tooltip(tr(bardo, Text::ChapterRemove))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.change_chapters(
                                    |bardo, id| bardo.remove_chapter(id, index),
                                    window,
                                    cx,
                                )
                            })),
                    )
            });
            h_flex()
                .id(("chapter-row", index))
                .gap_2()
                .items_center()
                .child(
                    div()
                        .w(px(52.))
                        .flex_none()
                        .text_sm()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(time),
                )
                .child(title)
                .when(row.at.is_none() && !view.stale, |row| {
                    row.child(kit::status(
                        Tone::Warning,
                        tr(bardo, Text::ChapterCutAway),
                        cx,
                    ))
                })
                .children(actions)
                .into_any_element()
        });
        let include = view.has_chapters().then(|| {
            Checkbox::new("chapters-include")
                .label(tr(bardo, Text::ChaptersInclude))
                .checked(view.include)
                .on_click(cx.listener(|this, checked: &bool, window, cx| {
                    let include = *checked;
                    this.change_chapters(
                        move |bardo, id| bardo.include_chapters(id, include),
                        window,
                        cx,
                    )
                }))
        });
        let mut warnings: Vec<AnyElement> = Vec::new();
        if view.has_chapters() {
            if view.stale {
                warnings.push(
                    kit::notice(Tone::Warning, tr(bardo, Text::ChaptersStale), cx)
                        .into_any_element(),
                );
            } else if view.length.is_zero() {
                warnings.push(muted(cx, tr(bardo, Text::ChaptersNoCut)).into_any_element());
            } else if view.include && !view.problems.is_empty() {
                let lines: Vec<String> = view
                    .problems
                    .iter()
                    .map(|problem| bardo.chapter_problem_text(*problem))
                    .collect();
                warnings.push(
                    kit::notice(
                        Tone::Warning,
                        format!("{} {}", bardo.text(Text::ChaptersLeftOut), lines.join(" ")),
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        Some(
            kit::anchor_in(
                TourAnchor::Control(Control::PublishChapters),
                v_flex()
                    .id("publish-chapters")
                    .gap_2()
                    .child(heading)
                    .children(
                        self.chapters_error
                            .map(|error| kit::notice(Tone::Danger, tr(bardo, error), cx)),
                    )
                    .children(job)
                    .children(ask)
                    .when(!view.has_chapters() && view.has_narration, |card| {
                        card.child(muted(cx, tr(bardo, Text::ChaptersEmpty)))
                    })
                    .children(rows)
                    .children(include)
                    .children(warnings)
                    .children(write),
                Some(&self.publish_scroll),
            )
            .into_any_element(),
        )
    }
}
